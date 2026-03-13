use core::convert::Infallible;
use core::fmt::Debug;

use defmt_rtt as _;
use panic_probe as _;

use embassy_nrf::{
    Peri, bind_interrupts,
    gpio::{AnyPin, Input, Level, Output, OutputDrive, Pull},
    peripherals,
    spim::{self, Frequency, InterruptHandler, Spim},
};
use embassy_time::Timer;
use embassy_time::Delay;
use embedded_hal_bus::spi::ExclusiveDevice;

use sx126x::SX126x;
use sx126x::conf::Config as LoRaConfig;
use sx126x::op::PacketType::LoRa;
use sx126x::op::irq::IrqMaskBit::*;
use sx126x::op::rxtx::DeviceSel::SX1261;
// use sx126x::op::status::CommandStatus::{CommandTimeout, CommandTxDone, DataAvailable};
use sx126x::op::*;

const RF_FREQUENCY: u32 = 869_400_000; // 868MHz (EU)
const F_XTAL: u32 = 32_000_000; // 32MHz

bind_interrupts!(struct Irqs {
    SPI2 => InterruptHandler<peripherals::SPI2>;
});

#[derive(Debug, defmt::Format)]
pub enum LoraError {
    Spi(&'static str),
    Timeout,
    Buffer(&'static str),
}

pub struct SimpleLoRa<'a> {
    lora: SX126x<
        ExclusiveDevice<Spim<'a>, Output<'a>, Delay>,
        Output<'a>,
        Input<'a>,
        Output<'a>,
        AlwaysHigh,
    >,
    // timer: Timer<'a, peripherals::TIMER0>,
    tx_timeout: RxTxTimeout,
    // rx_timeout: RxTxTimeout,
    crc_type: LoRaCrcType,

    // Gpio
    dio1: Input<'a>,
}

impl<'a> SimpleLoRa<'a> {
    pub fn new(
        spi: Peri<'a, peripherals::SPI2>,
        sck_pin: Peri<'a, AnyPin>,
        mosi_pin: Peri<'a, AnyPin>,
        miso_pin: Peri<'a, AnyPin>,
        nrst_pin: Peri<'a, AnyPin>,
        nss_pin: Peri<'a, AnyPin>,
        busy_pin: Peri<'a, AnyPin>,
        dio1_pin: Peri<'a, AnyPin>,
        ant_pin: Peri<'a, AnyPin>,
    ) -> SimpleLoRa<'a> {
        // SPI master configuration
        let mut spi_cfg = spim::Config::default();
        spi_cfg.frequency = Frequency::M1;
        let spim = Spim::new(spi, Irqs, sck_pin, mosi_pin, miso_pin, spi_cfg);

        // GPIO configuration
        let nss = Output::new(nss_pin, Level::High, OutputDrive::Standard);
        let nreset = Output::new(nrst_pin, Level::High, OutputDrive::Standard);
        let busy = Input::new(busy_pin, Pull::None);
        let ant = Output::new(ant_pin, Level::High, OutputDrive::Standard);
        let dio1 = Input::new(dio1_pin, Pull::None);

        // ExclusiveDevice combines the SPI bus + CS pin into a SpiDevice, as required by sx126x 0.3.
        // AlwaysHigh is a dummy DIO1 for the sx126x struct; real async DIO1 waiting is done below
        // via lora_dio1.wait_for_rising_edge() so the executor is not blocked.
        let spi_dev = ExclusiveDevice::new(spim, nss, Delay).unwrap();

        let conf = build_config();
        let mut lora = SX126x::new(spi_dev, (nreset, busy, ant, AlwaysHigh));
        lora.init(conf).unwrap();

        let tx_timeout = 0.into();
        let rx_timeout = RxTxTimeout::from_ms(3000);
        let crc_type = LoRaCrcType::CrcOn;

        lora.set_rx(rx_timeout).unwrap();

        // // Start with the radio in receiving mode
        // self.lora.set_ant_enabled(false).unwrap();

        SimpleLoRa {
            lora,
            tx_timeout,
            // rx_timeout,
            crc_type,
            dio1,
        }
    }

    // Wait for event
    pub async fn wait_for_status(&mut self) -> Result<Status, LoraError> {
        self.dio1.wait_for_rising_edge().await;

        self.get_status()
    }

    // Get lora radio status
    pub fn get_status(&mut self) -> Result<Status, LoraError> {
        self.lora.clear_irq_status(IrqMask::all()).unwrap();
        self.lora
            .get_status()
            .map_err(|_| LoraError::Spi("Failed to get status"))
    }

    // Send a message to the LoRa network

    // Receive a message from the LoRa network
    pub async fn receive_message(&mut self, buffer: &mut [u8]) -> Result<usize, LoraError> {
        let buffer_status = self.lora.get_rx_buffer_status().unwrap();
        let payload_len = buffer_status.payload_length_rx() as usize;

        if buffer.len() < payload_len {
            Err(LoraError::Buffer("Buffer too small for payload"))?;
        }

        let start_offset = buffer_status.rx_start_buffer_pointer();

        for i in (0..payload_len).step_by(buffer.len()) {
            let end = (payload_len - i).min(buffer.len());
            self.lora
                .read_buffer(i as u8 + start_offset, &mut buffer[..end])
                .map_err(|_| LoraError::Buffer("Failed to read buffer"))?;
        }

        // Return result as string
        Ok(payload_len.into())
    }

    // Wait for an RX or TX done interrupt
    pub async fn send_message(&mut self, message: &str) -> Result<(), LoraError> {
        // RF switch to TX mode
        self.lora.set_ant_enabled(true).unwrap();

        // Manual TX: write payload, configure packet, start TX, then await DIO1 async.
        // (Avoids write_bytes which polls DIO1 blocking inside the executor.)
        self.lora.write_buffer(0x00, message.as_bytes()).unwrap();
        let packet_params = LoRaPacketParams::default()
            .set_preamble_len(8)
            .set_payload_len(message.len() as u8)
            .set_crc_type(self.crc_type)
            .into();
        self.lora.set_packet_params(packet_params).unwrap();
        self.lora
            .set_tx(self.tx_timeout)
            .map_err(|_| LoraError::Timeout)?;

        // Await TX done via DIO1 instead of blocking poll
        self.dio1.wait_for_rising_edge().await;
        self.lora.clear_irq_status(IrqMask::all()).unwrap();
        // TX done back to RX mode
        self.lora.set_ant_enabled(false).unwrap();

        Ok(())
    }
}

pub async fn run_lora_test<'a>(
    spi: Peri<'a, peripherals::SPI2>,
    sck_pin: Peri<'a, AnyPin>,
    mosi_pin: Peri<'a, AnyPin>,
    miso_pin: Peri<'a, AnyPin>,
    nrst_pin: Peri<'a, AnyPin>,
    nss_pin: Peri<'a, AnyPin>,
    busy_pin: Peri<'a, AnyPin>,
    dio1_pin: Peri<'a, AnyPin>,
    ant_pin: Peri<'a, AnyPin>,
) -> Result<(), LoraError> {
    let mut lora = SimpleLoRa::new(
        spi, sck_pin, mosi_pin, miso_pin, nss_pin, nrst_pin, busy_pin, dio1_pin, ant_pin,
    );

    let message = "Hello, LoRa!";
    loop {
        lora.send_message(message)
            .await
            .map_err(|_| LoraError::Spi("Send message failed"))?;
        defmt::info!("Sent: {}", message);
        Timer::after_millis(1000).await;
    }
}

fn build_config() -> LoRaConfig {
    let mod_params = LoraModParams::default().into();
    let tx_params = TxParams::default()
        .set_power_dbm(14)
        .set_ramp_time(RampTime::Ramp200u);
    let pa_config = PaConfig::default()
        .set_device_sel(SX1261)
        .set_pa_duty_cycle(0x04);

    let dio1_irq_mask = IrqMask::none()
        .combine(TxDone)
        .combine(Timeout)
        .combine(RxDone);
    let packet_params = LoRaPacketParams::default().into();
    let rf_freq = sx126x::calc_rf_freq(RF_FREQUENCY as f32, F_XTAL as f32);

    LoRaConfig {
        packet_type: LoRa,
        sync_word: 0x1424,
        calib_param: CalibParam::from(0x7F),
        mod_params,
        tx_params,
        pa_config,
        packet_params: Some(packet_params),
        dio1_irq_mask,
        dio2_irq_mask: IrqMask::none(),
        dio3_irq_mask: IrqMask::none(),
        rf_frequency: RF_FREQUENCY,
        rf_freq,
        tcxo_opts: Option::None,
    }
}

/// Dummy DIO1 pin passed to SX126x. Reports "always high" (asserted) so that
/// the library's internal wait_on_dio1 spin-loop exits immediately.
/// Actual interrupt waiting is done externally with wait_for_rising_edge().
struct AlwaysHigh;

impl embedded_hal::digital::ErrorType for AlwaysHigh {
    type Error = Infallible;
}

impl embedded_hal::digital::InputPin for AlwaysHigh {
    fn is_high(&mut self) -> Result<bool, Infallible> {
        Ok(true)
    }
    fn is_low(&mut self) -> Result<bool, Infallible> {
        Ok(false)
    }
}
