use core::convert::Infallible;
use core::fmt::Debug;
use core::sync::atomic::Ordering;

use defmt_rtt as _;
use panic_probe as _;

use embassy_nrf::{
    Peri, bind_interrupts,
    gpio::{AnyPin, Input, Level, Output, OutputDrive, Pull},
    peripherals,
    spim::{self, Frequency, InterruptHandler, Spim},
};
use embassy_time::Delay;
use embassy_time::Timer;
use embedded_hal_bus::spi::ExclusiveDevice;

use embassy_futures::select::{Either, select};
use sx126x::SX126x;
use sx126x::conf::Config as LoRaConfig;
use sx126x::op::PacketType::LoRa;
use sx126x::op::irq::IrqMaskBit::{
    CrcErr, HeaderError, HeaderValid, PreambleDetected, RxDone, SyncwordValid, Timeout, TxDone,
};
use sx126x::op::rxtx::DeviceSel::SX1262;
use sx126x::op::status::ChipMode;
use sx126x::op::tcxo::{TcxoDelay, TcxoVoltage};
use sx126x::op::*;
use sx126x::reg::Register;

// ---------------------------------------------------------------------------
// MeshCore LoRa configuration
// ---------------------------------------------------------------------------

/// Configurable LoRa radio settings for MeshCore.
pub struct MeshCoreConfig {
    pub frequency_hz: u32,
    pub spread_factor: LoRaSpreadFactor,
    pub bandwidth: LoRaBandWidth,
    pub coding_rate: LoraCodingRate,
    /// SX1262 sync word (written to registers 0x0740/0x0741).
    /// MeshCore uses 0x1424 for private networks.
    pub sync_word: u16,
    pub tx_power_dbm: i8,
    pub preamble_len: u16,
    /// TCXO voltage and startup delay for modules that power the TCXO via DIO3
    /// (e.g. eByte E22). Set to None if the module uses a plain crystal instead.
    pub tcxo: Option<(TcxoVoltage, TcxoDelay)>,
}

impl MeshCoreConfig {
    /// MeshCore EU/UK narrow band preset — matches the meshcore-dev/MeshCore firmware defaults.
    ///
    /// 869.618 MHz · BW 62.5 kHz · SF8 · CR 4/5 · sync word 0x1424 (private LoRa)
    ///
    /// TCXO: 1.8 V / 5 ms startup — typical for eByte E22-900M22S and similar modules.
    /// If the board uses a plain crystal, set `tcxo: None`.
    ///
    /// See <https://www.m7spi.co.uk/switching-to-uk-narrow-band-a-guide-for-meshcore-users/>
    pub const UK_NARROW_BAND: Self = Self {
        frequency_hz: 869_618_000,
        spread_factor: LoRaSpreadFactor::SF8,
        bandwidth: LoRaBandWidth::BW62,
        coding_rate: LoraCodingRate::CR4_5,
        sync_word: 0x1424, // RADIOLIB_SX126X_SYNC_WORD_PRIVATE
        tx_power_dbm: 14,
        preamble_len: 8,
        tcxo: None, // External 32 MHz crystal on XTA/XTB — no DIO3 TCXO control needed
    };
}

// ---------------------------------------------------------------------------

const F_XTAL: u32 = 32_000_000; // 32 MHz crystal

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
    pub(super) lora: SX126x<
        ExclusiveDevice<Spim<'a>, Output<'a>, Delay>,
        Output<'a>,
        Input<'a>,
        Output<'a>,
        AlwaysHigh,
    >,
    pub(super) tx_timeout: RxTxTimeout,
    pub(super) crc_type: LoRaCrcType,
    pub(super) preamble_len: u16,
    pub(super) dio1: Input<'a>,
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
        config: &MeshCoreConfig,
    ) -> Result<SimpleLoRa<'a>, LoraError> {
        let mut spi_cfg = spim::Config::default();
        spi_cfg.frequency = Frequency::M1;
        let spim = Spim::new(spi, Irqs, sck_pin, mosi_pin, miso_pin, spi_cfg);

        let nss = Output::new(nss_pin, Level::High, OutputDrive::Standard);
        let nreset = Output::new(nrst_pin, Level::High, OutputDrive::Standard);
        let busy = Input::new(busy_pin, Pull::None);
        let ant = Output::new(ant_pin, Level::Low, OutputDrive::Standard);
        let dio1 = Input::new(dio1_pin, Pull::None);

        // AlwaysHigh is a dummy DIO1 for the sx126x struct; real async DIO1 waiting
        // is done externally via wait_for_rising_edge() so the executor is not blocked.
        let spi_dev = ExclusiveDevice::new(spim, nss, Delay).unwrap();

        let conf = build_lora_config(config);
        let mut lora = SX126x::new(spi_dev, (nreset, busy, ant, AlwaysHigh));
        lora.init(conf)
            .map_err(|_| LoraError::Spi("lora init failed"))?;

        lora.set_rx(RxTxTimeout::continuous_rx())
            .map_err(|_| LoraError::Spi("lora set_rx failed"))?;
        lora.set_ant_enabled(false).ok(); // RF switch → RX path (LOW)

        let mut radio = SimpleLoRa {
            lora,
            tx_timeout: 0.into(),
            crc_type: LoRaCrcType::CrcOn,
            preamble_len: config.preamble_len,
            dio1,
        };
        radio.apply_rx_gain();
        Ok(radio)
    }

    /// Write the RxGain register according to the BOOSTED_RX_GAIN flag.
    fn apply_rx_gain(&mut self) {
        let value = if crate::BOOSTED_RX_GAIN.load(Ordering::Relaxed) {
            0x96u8
        } else {
            0x94u8
        };
        self.lora.write_register(Register::RxGain, &[value]).ok();
    }

    /// Route the RF switch to the RX path (pin LOW).
    fn rf_switch_rx(&mut self) {
        self.lora.set_ant_enabled(true).ok();
    }

    /// Route the RF switch to the TX path (pin HIGH).
    fn rf_switch_tx(&mut self) {
        self.lora.set_ant_enabled(false).ok();
    }

    /// Wait for the chip to enter RX mode (0x05), polling every 50 ms for up to 500 ms.
    /// Returns true if RX mode is confirmed.
    pub async fn ensure_rx(&mut self) -> bool {
        // wait_on_busy before set_rx: the crate's set_rx() skips the mandatory busy
        // check, so sending the command while BUSY is asserted would silently drop it.
        self.lora.wait_on_busy().ok();
        self.lora.set_rx(RxTxTimeout::continuous_rx()).ok();
        self.apply_rx_gain();
        self.rf_switch_rx();

        for i in 0..10u8 {
            Timer::after_millis(50).await;
            if let Ok(s) = self.lora.get_status() {
                let mode = s.chip_mode().map(|m| m as u8).unwrap_or(0xFF);
                let cmd = s.command_status().map(|c| c as u8).unwrap_or(0xFF);
                defmt::debug!(
                    "ensure_rx poll {=u8}: chip_mode={=u8:#04x} cmd={=u8:#04x}",
                    i,
                    mode,
                    cmd
                );
                if matches!(s.chip_mode(), Some(ChipMode::RX)) {
                    return true;
                }
            }
        }
        false
    }

    /// Wait for the next LoRa event (RxDone or Timeout), read the payload if
    /// present, and re-arm RX.
    ///
    /// Returns `Ok(Some((len, rssi_dbm)))` on a valid receive,
    /// `Ok(None)` on timeout or CRC error (RX is re-armed in both cases).
    pub async fn receive_packet(
        &mut self,
        buf: &mut [u8],
    ) -> Result<Option<(usize, i16)>, LoraError> {
        // Log chip mode so we can confirm the chip is actually in RX before waiting.
        // Expected: 0x05 (RX). If 0x02 (StbyRC), set_rx() hasn't taken effect yet.
        if let Ok(s) = self.lora.get_status() {
            let mode = s.chip_mode().map(|m| m as u8).unwrap_or(0xFF);
            defmt::debug!("RX wait: chip_mode={=u8:#04x} (0x05=RX, 0x02=StbyRC)", mode);
        }

        // Clear any stale IRQ so DIO1 is deasserted before we arm the rising-edge wait.
        // Without this, a leftover HIGH from the init sequence would block wait_for_rising_edge()
        // forever (it only fires on LOW→HIGH transitions).
        self.lora
            .clear_irq_status(IrqMask::all())
            .map_err(|_| LoraError::Spi("clear_irq before wait failed"))?;

        // TODO: Replace active wait + watchdog timer with deep sleep + GPIO wake-up once
        // reception is confirmed working. The current approach keeps HFCLK running and
        // prevents low-power sleep, significantly increasing battery drain.
        match select(self.dio1.wait_for_rising_edge(), Timer::after_secs(15)).await {
            Either::First(_) => {} // DIO1 fired — read IRQ below
            Either::Second(_) => {
                defmt::debug!("LoRa: no DIO1 in 15s — re-arming RX (continuous mode)");
                self.lora.wait_on_busy().ok();
                self.lora
                    .set_rx(RxTxTimeout::continuous_rx())
                    .map_err(|_| LoraError::Timeout)?;
                self.apply_rx_gain();
                self.rf_switch_rx();
                return Ok(None);
            }
        }

        // sx126x 0.3.0 does not call wait_on_busy() before get_irq_status();
        // without this the chip may still be busy processing the just-received
        // packet and the SPI read returns 0x00 (all flags false).
        self.lora.wait_on_busy().ok();
        let irq = self
            .lora
            .get_irq_status()
            .map_err(|_| LoraError::Spi("get_irq_status failed"))?;
        self.lora.wait_on_busy().ok();
        self.lora.clear_irq_status(IrqMask::all()).unwrap();

        // Log every IRQ event so we can diagnose reception issues:
        //   timeout only           → wrong frequency
        //   preamble, no sync_word  → frequency OK, sync word mismatch
        //   sync_word, header_err   → SF/BW/CR mismatch
        //   rx_done + crc_err      → modem settings OK, payload error
        //   rx_done, no crc_err    → full receive
        defmt::debug!(
            "DIO1: rx={} crc_err={} timeout={} | preamble={} syncword={} header_ok={} header_err={}",
            irq.rx_done(),
            irq.crc_err(),
            irq.timeout(),
            irq.preamble_detected(),
            irq.syncword_valid(),
            irq.header_valid(),
            irq.header_error(),
        );

        let result = if irq.rx_done() && !irq.crc_err() {
            let buf_status = self
                .lora
                .get_rx_buffer_status()
                .map_err(|_| LoraError::Buffer("get_rx_buffer_status failed"))?;
            let len = buf_status.payload_length_rx() as usize;
            let offset = buf_status.rx_start_buffer_pointer();

            if len > buf.len() {
                return Err(LoraError::Buffer("buffer too small"));
            }

            self.lora
                .read_buffer(offset, &mut buf[..len])
                .map_err(|_| LoraError::Buffer("read_buffer failed"))?;

            let rssi = self
                .lora
                .get_packet_status()
                .map(|s| s.rssi_pkt() as i16)
                .unwrap_or(0);

            Some((len, rssi))
        } else if irq.crc_err() {
            let rssi = self
                .lora
                .get_packet_status()
                .map(|s| s.rssi_pkt() as i16)
                .unwrap_or(0);
            defmt::warn!(
                "LoRa CRC error {=i16}dBm — header decoded OK (SF/BW/CR/syncword match) but payload bytes corrupted (collision, interference, or sender has CRC disabled)",
                rssi
            );
            None
        } else {
            None
        };

        // Re-arm continuous RX
        defmt::debug!("RX re-armed");
        self.lora.wait_on_busy().ok();
        self.lora
            .set_rx(RxTxTimeout::continuous_rx())
            .map_err(|_| LoraError::Timeout)?;
        self.apply_rx_gain();
        self.rf_switch_rx();

        Ok(result)
    }

    pub async fn send_message(&mut self, message: &[u8]) -> Result<(), LoraError> {
        self.rf_switch_tx();

        self.lora.write_buffer(0x00, message).unwrap();
        let packet_params = LoRaPacketParams::default()
            .set_preamble_len(self.preamble_len)
            .set_payload_len(message.len() as u8)
            .set_crc_type(self.crc_type)
            .into();
        self.lora.set_packet_params(packet_params).unwrap();
        self.lora
            .set_tx(self.tx_timeout)
            .map_err(|_| LoraError::Timeout)?;

        self.dio1.wait_for_rising_edge().await;
        self.lora.clear_irq_status(IrqMask::all()).unwrap();

        // Re-arm continuous RX immediately so receive_packet() doesn't spend
        // 15 s in standby waiting for the timeout before calling set_rx().
        self.lora.wait_on_busy().ok();
        self.lora.set_rx(RxTxTimeout::continuous_rx()).ok();
        self.apply_rx_gain();
        self.rf_switch_rx();

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

pub(super) fn build_lora_config(config: &MeshCoreConfig) -> LoRaConfig {
    let mod_params = LoraModParams::default()
        .set_spread_factor(config.spread_factor)
        .set_bandwidth(config.bandwidth)
        .set_coding_rate(config.coding_rate)
        .into();

    let tx_params = TxParams::default()
        .set_power_dbm(config.tx_power_dbm)
        .set_ramp_time(RampTime::Ramp200u);

    let pa_config = PaConfig::default()
        .set_device_sel(SX1262)
        .set_pa_duty_cycle(0x04);

    let dio1_irq_mask = IrqMask::none()
        .combine(TxDone)
        .combine(RxDone)
        .combine(CrcErr)
        .combine(Timeout)
        .combine(PreambleDetected)
        .combine(SyncwordValid)
        .combine(HeaderValid)
        .combine(HeaderError);

    let packet_params = LoRaPacketParams::default()
        .set_preamble_len(config.preamble_len)
        .into();

    let rf_freq = sx126x::calc_rf_freq(config.frequency_hz as f32, F_XTAL as f32);

    LoRaConfig {
        packet_type: LoRa,
        sync_word: config.sync_word,
        calib_param: CalibParam::from(0x7F),
        mod_params,
        tx_params,
        pa_config,
        packet_params: Some(packet_params),
        dio1_irq_mask,
        dio2_irq_mask: IrqMask::none(),
        dio3_irq_mask: IrqMask::none(),
        rf_frequency: config.frequency_hz,
        rf_freq,
        tcxo_opts: config.tcxo,
    }
}

/// Dummy DIO1 pin passed to SX126x.  Reports "always high" so that the
/// library's internal wait_on_dio1 spin-loop exits immediately.
/// Real interrupt waiting is done externally with `wait_for_rising_edge()`.
pub(super) struct AlwaysHigh;

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
