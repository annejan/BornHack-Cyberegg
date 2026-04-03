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

use sx126x::SX126x;
use sx126x::conf::Config as LoRaConfig;
use sx126x::op::PacketType::LoRa;
use sx126x::op::irq::IrqMaskBit::{CrcErr, RxDone, Timeout, TxDone};
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
    /// Spreading factor 5–12 (numeric, used for airtime estimation).
    pub sf_num: u8,
    /// Bandwidth in Hz (numeric, used for airtime estimation).
    pub bw_hz_num: u32,
    /// Coding rate in MeshCore protocol encoding: 5 = CR4/5, …, 8 = CR4/8.
    pub cr_num: u8,
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
        tx_power_dbm: 22,
        preamble_len: 8,
        tcxo: None, // External 32 MHz crystal on XTA/XTB — no DIO3 TCXO control needed
        sf_num:    8,
        bw_hz_num: 62_500,
        cr_num:    5,
    };

    /// Build a [`MeshCoreConfig`] from user-configurable [`settings::RadioParams`].
    ///
    /// Hardware-fixed fields (`sync_word`, `preamble_len`, `tcxo`) are inherited
    /// from [`UK_NARROW_BAND`] and are never user-configurable via the companion app.
    ///
    /// `settings::RadioParams.cr` uses **MeshCore protocol encoding** (5 = CR 4/5,
    /// 6 = CR 4/6, …).  The sx126x hardware encoding (CR4_5 = 1, …) is handled here.
    pub fn from_radio_params(p: &crate::fw::settings::RadioParams) -> Self {
        Self {
            frequency_hz: p.freq_hz,
            spread_factor: match p.sf {
                5 => LoRaSpreadFactor::SF5,
                6 => LoRaSpreadFactor::SF6,
                7 => LoRaSpreadFactor::SF7,
                9 => LoRaSpreadFactor::SF9,
                10 => LoRaSpreadFactor::SF10,
                11 => LoRaSpreadFactor::SF11,
                12 => LoRaSpreadFactor::SF12,
                _ => LoRaSpreadFactor::SF8, // default / SF8
            },
            bandwidth: match p.bw_hz {
                0..=9_999 => LoRaBandWidth::BW7,
                10_000..=14_999 => LoRaBandWidth::BW10,
                15_000..=19_999 => LoRaBandWidth::BW15,
                20_000..=30_999 => LoRaBandWidth::BW20,
                31_000..=40_999 => LoRaBandWidth::BW31,
                41_000..=61_999 => LoRaBandWidth::BW41,
                62_000..=124_999 => LoRaBandWidth::BW62,
                125_000..=249_999 => LoRaBandWidth::BW125,
                250_000..=499_999 => LoRaBandWidth::BW250,
                _ => LoRaBandWidth::BW500,
            },
            coding_rate: match p.cr {
                6 => LoraCodingRate::CR4_6,
                7 => LoraCodingRate::CR4_7,
                8 => LoraCodingRate::CR4_8,
                _ => LoraCodingRate::CR4_5, // protocol cr=5 → CR 4/5
            },
            sync_word:    Self::UK_NARROW_BAND.sync_word,
            tx_power_dbm: p.tx_power,
            preamble_len: Self::UK_NARROW_BAND.preamble_len,
            tcxo:         Self::UK_NARROW_BAND.tcxo,
            sf_num:       p.sf,
            bw_hz_num:    p.bw_hz,
            cr_num:       p.cr,
        }
    }
}

// ---------------------------------------------------------------------------

const F_XTAL: u32 = 32_000_000; // 32 MHz crystal

// Extension trait that adds RF-switch helpers directly to the SX126x type used
// in this module, so callers don't need to remember which set_ant_enabled() value
// means RX vs TX.
trait RfSwitch {
    fn rf_switch_rx(&mut self);
    fn rf_switch_tx(&mut self);
}

impl<'a> RfSwitch
    for SX126x<
        ExclusiveDevice<Spim<'a>, Output<'a>, Delay>,
        Output<'a>,
        Input<'a>,
        Output<'a>,
        AlwaysHigh,
    >
{
    fn rf_switch_rx(&mut self) {
        self.set_ant_enabled(true).ok();
    }
    fn rf_switch_tx(&mut self) {
        self.set_ant_enabled(false).ok();
    }
}

bind_interrupts!(struct Irqs {
    SPI2 => InterruptHandler<peripherals::SPI2>;
});

#[derive(Debug, defmt::Format)]
pub enum LoraError {
    Spi(&'static str),
    Timeout,
    Buffer(&'static str),
    /// TX skipped — 1-hour airtime budget exhausted.
    DutyCycle,
}

// ---------------------------------------------------------------------------
// Airtime estimation
// ---------------------------------------------------------------------------

/// Standard LoRa time-on-air calculation (Semtech SX1261/2 datasheet §6.1.4).
///
/// Returns the estimated on-air time in milliseconds for a packet of
/// `payload_len` bytes.  Parameters use MeshCore conventions:
/// - `sf`       — spreading factor 5–12
/// - `bw_hz`    — bandwidth in Hz (e.g. 62_500)
/// - `cr_proto` — coding rate in MeshCore encoding: 5 = CR4/5, …, 8 = CR4/8
/// - `preamble` — programmed preamble length (e.g. 8)
///
/// Assumes explicit header mode and CRC enabled (both standard for MeshCore).
pub fn lora_airtime_ms(payload_len: usize, sf: u8, bw_hz: u32, cr_proto: u8, preamble: u16) -> u32 {
    // Symbol duration: T_sym_us = 2^SF * 1_000_000 / BW_Hz
    let sym_us = (1u64 << sf) * 1_000_000 / bw_hz as u64;

    // Low data rate optimisation: enabled when T_sym > 16 ms
    let de: i64 = if sym_us > 16_000 { 1 } else { 0 };

    // CR value: proto encoding 5 → 1, 6 → 2, 7 → 3, 8 → 4
    let cr: i64 = cr_proto.saturating_sub(4) as i64;

    // Payload symbol count — explicit header (IH=0), CRC=1 → constant 44
    let n      = payload_len as i64;
    let sf_i   = sf as i64;
    let num    = 8 * n - 4 * sf_i + 44;    // 44 = 28 + 16*CRC - 20*IH
    let denom  = 4 * (sf_i - 2 * de);
    let extra  = if num > 0 && denom > 0 {
        ((num + denom - 1) / denom) * (cr + 4)  // ceil(num/denom) * (CR+4)
    } else {
        0
    };
    let payload_syms = 8 + extra;

    // Total = (preamble + 4.25) + N_payload  →  (preamble + 4) + N_payload (integer)
    let total_syms = preamble as i64 + 4 + payload_syms;
    let t_us = total_syms as u64 * sym_us;

    ((t_us / 1000) as u32).max(1)
}

// ---------------------------------------------------------------------------
// TX duty-cycle budget
// ---------------------------------------------------------------------------

/// Token-bucket TX airtime budget matching the C++ `Dispatcher` logic.
///
/// The budget refills continuously at rate `duty_cycle` per millisecond elapsed
/// and is deducted by actual measured TX airtime after each transmission.
///
/// `duty_cycle = 1 / (1 + airtime_factor)` where `airtime_factor` is encoded
/// as an integer × 1000 (e.g. 9000 = factor 9.0 → 10 % duty cycle).
pub struct TxBudget {
    budget_ms: u32,
    last_update: embassy_time::Instant,
    /// Airtime factor × 1000; denominator = 1000 + af_x1000.
    af_x1000: u32,
}

impl TxBudget {
    const WINDOW_MS:      u32 = 3_600_000; // 1 hour in ms
    const MIN_RESERVE_MS: u32 = 100;       // min budget before blocking TX
    const MIN_TX_DIV:     u32 = 2;         // require est_airtime / N as budget

    pub fn new(af_x1000: u32) -> Self {
        let denom = 1_000 + af_x1000;
        // Initial budget = max budget = window * duty_cycle = window * 1000 / denom
        let max_budget = (Self::WINDOW_MS as u64 * 1_000 / denom as u64) as u32;
        Self {
            budget_ms: max_budget,
            last_update: embassy_time::Instant::now(),
            af_x1000,
        }
    }

    /// Update the factor (e.g. when the user changes it via the companion app).
    /// Resets the budget to the new max to avoid instant exhaustion.
    pub fn update_factor(&mut self, af_x1000: u32) {
        self.af_x1000 = af_x1000;
        let denom = 1_000 + af_x1000;
        let max_budget = (Self::WINDOW_MS as u64 * 1_000 / denom as u64) as u32;
        self.budget_ms = max_budget;
        self.last_update = embassy_time::Instant::now();
    }

    /// Refill the budget based on time elapsed since the last update.
    fn refill(&mut self) {
        let now      = embassy_time::Instant::now();
        let elapsed  = (now - self.last_update).as_millis() as u32;
        let denom    = 1_000 + self.af_x1000;
        let max_bud  = (Self::WINDOW_MS as u64 * 1_000 / denom as u64) as u32;
        let refill   = (elapsed as u64 * 1_000 / denom as u64) as u32;
        if refill > 0 {
            self.budget_ms = self.budget_ms.saturating_add(refill).min(max_bud);
            self.last_update = now;
        }
    }

    /// Returns `true` if TX is allowed for a packet with estimated airtime `est_ms`.
    ///
    /// Requires `budget_ms >= est_ms / MIN_TX_DIV` (same guard as C++ Dispatcher).
    pub fn can_tx(&mut self, est_ms: u32) -> bool {
        self.refill();
        self.budget_ms >= est_ms / Self::MIN_TX_DIV
    }

    /// Deduct actual measured TX airtime after a successful transmission.
    pub fn deduct(&mut self, actual_ms: u32) {
        if actual_ms >= self.budget_ms {
            self.budget_ms = 0;
        } else {
            self.budget_ms -= actual_ms;
        }
        if self.budget_ms < Self::MIN_RESERVE_MS {
            defmt::warn!("TX budget low: {=u32}ms remaining", self.budget_ms);
        }
    }
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
    /// Radio params stored for airtime estimation.
    sf: u8,
    bw_hz: u32,
    cr_proto: u8,
    /// TX duty-cycle budget; `None` until `init_budget()` is called.
    tx_budget: Option<TxBudget>,
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
        match lora.set_dio2_as_rf_switch_ctrl(true) {
            Ok(_) => (),
            Err(_) => return Err(LoraError::Spi("lora set_dio2_as_rf_switch_ctrl failed")),
        };

        lora.set_rx(RxTxTimeout::continuous_rx())
            .map_err(|_| LoraError::Spi("lora set_rx failed"))?;
        lora.rf_switch_rx();

        let mut radio = SimpleLoRa {
            lora,
            tx_timeout: 0.into(),
            crc_type: LoRaCrcType::CrcOn,
            preamble_len: config.preamble_len,
            dio1,
            sf:        config.sf_num,
            bw_hz:     config.bw_hz_num,
            cr_proto:  config.cr_num,
            tx_budget: None,
        };
        radio.apply_rx_gain();
        Ok(radio)
    }

    /// Initialise the TX duty-cycle budget.
    ///
    /// Must be called once at startup with the persisted `airtime_factor_x1000`
    /// from settings.  Can be called again whenever the factor changes.
    pub fn init_budget(&mut self, af_x1000: u32) {
        match self.tx_budget {
            Some(ref mut b) => b.update_factor(af_x1000),
            None => self.tx_budget = Some(TxBudget::new(af_x1000)),
        }
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

    /// Wait for the chip to enter RX mode (0x05), polling every 50 ms for up to 500 ms.
    /// Returns true if RX mode is confirmed.
    pub async fn ensure_rx(&mut self) -> bool {
        // wait_on_busy before set_rx: the crate's set_rx() skips the mandatory busy
        // check, so sending the command while BUSY is asserted would silently drop it.
        self.lora.wait_on_busy().ok();
        self.lora.set_rx(RxTxTimeout::continuous_rx()).ok();
        self.apply_rx_gain();
        self.lora.rf_switch_rx();

        for _ in 0..10u8 {
            Timer::after_millis(50).await;
            if let Ok(s) = self.lora.get_status() {
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
    /// Returns `Ok(Some((len, rssi_dbm, snr_x4)))` on a valid receive,
    /// where `snr_x4` is SNR in units of 0.25 dB (same encoding as SX1262's snr_pkt).
    /// `Ok(None)` on timeout or CRC error (RX is re-armed in both cases).
    pub async fn receive_packet(
        &mut self,
        buf: &mut [u8],
    ) -> Result<Option<(usize, i16, i8)>, LoraError> {
        // Clear any stale IRQ so DIO1 is deasserted before we arm the rising-edge wait.
        // Without this, a leftover HIGH from the init sequence would block wait_for_rising_edge()
        // forever (it only fires on LOW→HIGH transitions).
        self.lora
            .clear_irq_status(IrqMask::all())
            .map_err(|_| LoraError::Spi("clear_irq before wait failed"))?;

        // In continuous RX mode the chip stays in RX indefinitely; DIO1 only fires
        // on real radio events (RxDone, CrcErr, PreambleDetected, …).
        self.dio1.wait_for_rising_edge().await;

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

            let (rssi, snr_x4) = self
                .lora
                .get_packet_status()
                .map(|s| (s.rssi_pkt() as i16, (s.snr_pkt() * 4.0) as i8))
                .unwrap_or((0, 0));

            Some((len, rssi, snr_x4))
        } else if irq.crc_err() {
            let (rssi, _snr_x4) = self
                .lora
                .get_packet_status()
                .map(|s| (s.rssi_pkt() as i16, (s.snr_pkt() * 4.0) as i8))
                .unwrap_or((0, 0));
            defmt::warn!(
                "LoRa CRC error {=i16}dBm — header decoded OK (SF/BW/CR/syncword match) but payload bytes corrupted (collision, interference, or sender has CRC disabled)",
                rssi
            );
            None
        } else {
            None
        };

        // Re-arm continuous RX
        self.lora.wait_on_busy().ok();
        self.lora
            .set_rx(RxTxTimeout::continuous_rx())
            .map_err(|_| LoraError::Timeout)?;
        self.apply_rx_gain();
        self.lora.rf_switch_rx();

        Ok(result)
    }

    pub async fn send_message(&mut self, message: &[u8]) -> Result<(), LoraError> {
        // Check TX duty-cycle budget before transmitting.
        if let Some(ref mut budget) = self.tx_budget {
            let est_ms = lora_airtime_ms(message.len(), self.sf, self.bw_hz, self.cr_proto, self.preamble_len);
            if !budget.can_tx(est_ms) {
                defmt::warn!(
                    "TX duty-cycle limit: est={=u32}ms budget={=u32}ms — packet dropped",
                    est_ms, budget.budget_ms,
                );
                return Err(LoraError::DutyCycle);
            }
        }

        self.lora.rf_switch_tx();

        self.lora.write_buffer(0x00, message).unwrap();
        let packet_params = LoRaPacketParams::default()
            .set_preamble_len(self.preamble_len)
            .set_payload_len(message.len() as u8)
            .set_crc_type(self.crc_type)
            .into();
        self.lora.set_packet_params(packet_params).unwrap();

        let tx_start = embassy_time::Instant::now();
        self.lora
            .set_tx(self.tx_timeout)
            .map_err(|_| LoraError::Timeout)?;

        self.dio1.wait_for_rising_edge().await;
        let actual_ms = tx_start.elapsed().as_millis() as u32;

        self.lora.clear_irq_status(IrqMask::all()).unwrap();

        // Deduct measured airtime from budget.
        if let Some(ref mut budget) = self.tx_budget {
            budget.deduct(actual_ms);
            defmt::debug!("TX done: actual={=u32}ms budget_remaining={=u32}ms", actual_ms, budget.budget_ms);
        }

        // Re-arm continuous RX so receive_packet() finds the chip already in RX mode.
        self.lora.wait_on_busy().ok();
        self.lora.set_rx(RxTxTimeout::continuous_rx()).ok();
        self.apply_rx_gain();
        self.lora.rf_switch_rx();

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

    // SX1262 datasheet Table 13-21: pa_duty_cycle=0x04 + hp_max=0x07 → +22 dBm max
    // hp_max defaults to 0x00 which caps the PA to its minimum output power.
    let pa_config = PaConfig::default()
        .set_device_sel(SX1262)
        .set_pa_duty_cycle(0x04)
        .set_hp_max(0x07);

    let dio1_irq_mask = IrqMask::none()
        .combine(TxDone)
        .combine(RxDone)
        .combine(CrcErr)
        .combine(Timeout);

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
