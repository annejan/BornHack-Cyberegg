use core::convert::Infallible;
use core::fmt::Debug;
use core::sync::atomic::Ordering;

use defmt_rtt as _;
use embassy_nrf::gpio::{AnyPin, Input, Level, Output, OutputDrive, Pull};
use embassy_nrf::spim::{self, Frequency, InterruptHandler, Spim};
use embassy_nrf::{Peri, bind_interrupts, peripherals};
use embassy_time::{Delay, Duration, Instant, Timer, with_deadline, with_timeout};
use embedded_hal::spi::{Operation, SpiDevice};
use embedded_hal_bus::spi::ExclusiveDevice;
use panic_probe as _;
use sx126x::SX126x;
use sx126x::conf::Config as LoRaConfig;
use sx126x::op::PacketType::LoRa;
use sx126x::op::irq::IrqMaskBit::{
    CadDetected, CadDone, CrcErr, HeaderError, HeaderValid, PreambleDetected, RxDone,
    SyncwordValid, Timeout, TxDone,
};
use sx126x::op::irq::IrqStatus;
use sx126x::op::rxtx::DeviceSel::SX1262;
use sx126x::op::status::ChipMode;
use sx126x::op::tcxo::{TcxoDelay, TcxoVoltage};
use sx126x::op::*;
use sx126x::reg::Register;

pub use crate::lora_timing::preamble_length_for_sf;
use crate::lora_timing::{RxDutyPeriods, rx_power_save_periods};

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
    /// (e.g. eByte E22). Set to None if the module uses a plain crystal
    /// instead.
    pub tcxo: Option<(TcxoVoltage, TcxoDelay)>,
    /// Spreading factor 5–12 (numeric, used for airtime estimation).
    pub sf_num: u8,
    /// Bandwidth in Hz (numeric, used for airtime estimation).
    pub bw_hz_num: u32,
    /// Coding rate in MeshCore protocol encoding: 5 = CR4/5, …, 8 = CR4/8.
    pub cr_num: u8,
}

impl MeshCoreConfig {
    /// MeshCore EU/UK narrow band preset — matches the meshcore-dev/MeshCore
    /// firmware defaults.
    ///
    /// 869.618 MHz · BW 62.5 kHz · SF8 · CR 4/5 · sync word 0x1424 (private
    /// LoRa)
    ///
    /// Preamble: 32 symbols, what stock MeshCore 1.16+ sends at SF8
    /// ([`preamble_length_for_sf`]), so duty-cycling peers can hear us.
    ///
    /// TCXO: 1.8 V / 5 ms startup — typical for eByte E22-900M22S and similar
    /// modules. If the board uses a plain crystal, set `tcxo: None`.
    ///
    /// See <https://www.m7spi.co.uk/switching-to-uk-narrow-band-a-guide-for-meshcore-users/>
    pub const UK_NARROW_BAND: Self = Self {
        frequency_hz: 869_618_000,
        spread_factor: LoRaSpreadFactor::SF8,
        bandwidth: LoRaBandWidth::BW62,
        coding_rate: LoraCodingRate::CR4_5,
        sync_word: 0x1424, // RADIOLIB_SX126X_SYNC_WORD_PRIVATE
        tx_power_dbm: 22,
        preamble_len: preamble_length_for_sf(8),
        tcxo: None, // External 32 MHz crystal on XTA/XTB — no DIO3 TCXO control needed
        sf_num: 8,
        bw_hz_num: 62_500,
        cr_num: 5,
    };

    /// Build a [`MeshCoreConfig`] from user-configurable
    /// [`super::settings::RadioParams`].
    ///
    /// Hardware-fixed fields (`sync_word`, `tcxo`) are inherited from
    /// [`Self::UK_NARROW_BAND`] and are never user-configurable via the
    /// companion app. `preamble_len` follows the SF the way stock MeshCore
    /// 1.16+ does ([`preamble_length_for_sf`]: 32 symbols at SF ≤ 8, else 16).
    ///
    /// `settings::RadioParams.cr` uses **MeshCore protocol encoding** (5 = CR
    /// 4/5, 6 = CR 4/6, …).  The sx126x hardware encoding (CR4_5 = 1, …) is
    /// handled here.
    pub fn from_radio_params(p: &super::settings::RadioParams) -> Self {
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
            sync_word: Self::UK_NARROW_BAND.sync_word,
            tx_power_dbm: p.tx_power,
            preamble_len: preamble_length_for_sf(p.sf),
            tcxo: Self::UK_NARROW_BAND.tcxo,
            sf_num: p.sf,
            bw_hz_num: p.bw_hz,
            cr_num: p.cr,
        }
    }
}

// ---------------------------------------------------------------------------

const F_XTAL: u32 = 32_000_000; // 32 MHz crystal

/// The SX1262's SPI device (bus + NSS).
type SpiDev<'a> = ExclusiveDevice<Spim<'a>, Output<'a>, Delay>;

/// The sx126x driver as a short-lived view over [`RadioHw`]. The crate keeps
/// its SPI device private and has no SetRxDutyCycle or raw register write,
/// so `SimpleLoRa` owns the bus and pins itself and lends them out per call
/// (`SX126x::new` only stores its arguments; embedded-hal 1.0 implements the
/// SPI and pin traits for `&mut T`).
type Radio<'r, 'a> = SX126x<
    &'r mut SpiDev<'a>,
    &'r mut Output<'a>,
    &'r mut Input<'a>,
    &'r mut Output<'a>,
    AlwaysHigh,
>;

/// RxGain retention list (datasheet §9.6): writing `[1, 0x08, 0xAC]` from
/// 0x029F makes the chip restore RxGain (0x08AC) after each warm-start
/// sleep, which SetRxDutyCycle needs to keep boosted gain.
const RX_GAIN_RETENTION_REG: u16 = 0x029F;
const RX_GAIN_RETENTION: [u8; 3] = [0x01, 0x08, 0xAC];

/// StopTimerOnPreamble (datasheet §13.1.6) with `enable` = 0x01: after a
/// preamble the chip stays in RX until the packet ends, instead of giving up
/// after `2·rxPeriod + sleepPeriod` (§13.1.7), which is shorter than the
/// rest of a preamble plus header with our duty-cycle periods.
const STOP_TIMER_ON_PREAMBLE: [u8; 2] = [0x9F, 0x01];

/// Waking from duty-cycle sleep: BUSY should drop within the warm-start time
/// (340 µs, datasheet Table 8-1); give each NSS pulse this long.
const WAKE_BUSY_TIMEOUT_US: u64 = 2_000;
/// NSS pulses tried before giving up on BUSY (an edge during the ~500 µs
/// context save is not guaranteed to wake the chip).
const WAKE_TRIES: u8 = 3;

/// The bus and pins behind [`Radio`].
struct RadioHw<'a> {
    spi: SpiDev<'a>,
    nreset: Output<'a>,
    busy: Input<'a>,
    ant: Output<'a>,
}

impl<'a> RadioHw<'a> {
    /// Borrow the sx126x driver for one or more calls.
    fn sx(&mut self) -> Radio<'_, 'a> {
        SX126x::new(
            &mut self.spi,
            (&mut self.nreset, &mut self.busy, &mut self.ant, AlwaysHigh),
        )
    }

    /// Send a raw command (opcode + parameters) the crate does not wrap.
    /// Callers wait on BUSY first, as for every other command.
    fn command(&mut self, bytes: &[u8]) {
        if self.spi.write(bytes).is_err() {
            defmt::warn!("SX1262 command {=u8:#04x} failed", bytes[0]);
        }
    }

    /// WriteRegister (0x0D) to an address outside the crate's `Register`
    /// enum.
    fn write_register_raw(&mut self, addr: u16, data: &[u8]) {
        let addr = addr.to_be_bytes();
        let mut ops = [
            Operation::Write(&[0x0D]),
            Operation::Write(&addr),
            Operation::Write(data),
        ];
        if self.spi.transaction(&mut ops).is_err() {
            defmt::warn!("SX1262 register write failed");
        }
    }

    /// Spin until BUSY is low, for at most `timeout_us`. Unlike the crate's
    /// `wait_on_busy()` this is bounded: BUSY stays high for the whole of a
    /// duty-cycle sleep (§13.1.7 / §13.1.1).
    fn wait_busy_low(&mut self, timeout_us: u64) -> bool {
        let start = Instant::now();
        while self.busy.is_high() {
            if start.elapsed() > Duration::from_micros(timeout_us) {
                return false;
            }
        }
        true
    }

    /// An NSS falling edge with no command: wakes the chip from sleep
    /// (§13.1.7 "must first be waken up by a falling edge of NSS").
    fn nss_pulse(&mut self) {
        self.spi
            .transaction(&mut [Operation::DelayNs(1_000)])
            .ok();
    }
}

// Extension trait that adds RF-switch helpers directly to the SX126x type used
// in this module, so callers don't need to remember which set_ant_enabled()
// value means RX vs TX.
trait RfSwitch {
    fn rf_switch_rx(&mut self);
    fn rf_switch_tx(&mut self);
}

impl RfSwitch for Radio<'_, '_> {
    fn rf_switch_rx(&mut self) {
        self.set_ant_enabled(true).ok();
    }
    fn rf_switch_tx(&mut self) {
        self.set_ant_enabled(false).ok();
    }
}

bind_interrupts!(pub struct Irqs {
    SPI2 => InterruptHandler<peripherals::SPI2>;
});

#[derive(Debug, defmt::Format)]
pub enum LoraError {
    Spi(&'static str),
    Timeout,
    Buffer(&'static str),
    /// TX skipped — 1-hour airtime budget exhausted.
    DutyCycle,
    /// TX deferred — channel busy (preamble in progress).
    ChannelBusy,
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
    let n = payload_len as i64;
    let sf_i = sf as i64;
    let num = 8 * n - 4 * sf_i + 44; // 44 = 28 + 16*CRC - 20*IH
    let denom = 4 * (sf_i - 2 * de);
    let extra = if num > 0 && denom > 0 {
        ((num + denom - 1) / denom) * (cr + 4) // ceil(num/denom) * (CR+4)
    } else {
        0
    };
    let payload_syms = 8 + extra;

    // Total = (preamble + 4.25) + N_payload  →  (preamble + 4) + N_payload
    // (integer)
    let total_syms = preamble as i64 + 4 + payload_syms;
    let t_us = total_syms as u64 * sym_us;

    ((t_us / 1000) as u32).max(1)
}

/// MeshCore `MAX_TRANS_UNIT` — the largest frame any node puts on air.
const MAX_TRANS_UNIT: usize = 255;

/// While no reception is in flight, check this often that the chip is still
/// in RX (badge-specific; upstream re-arms from its main loop instead).
const RX_IDLE_WATCHDOG_MS: u64 = 5_000;

/// Added to upstream's `est_airtime × 1.5` TxDone deadline to absorb SPI and
/// executor latency between `set_tx()` and the DIO1 wait.
const TX_DONE_SLACK_MS: u64 = 100;

/// Receive-watchdog windows (upstream `PacketMillis`, RadioLibWrappers.h).
#[derive(Clone, Copy, Debug, PartialEq, Eq, defmt::Format)]
pub struct PacketMillis {
    /// PreambleDetected → HeaderValid deadline.
    pub preamble_ms: u32,
    /// HeaderValid → RxDone deadline.
    pub payload_ms: u32,
}

/// Port of upstream `RadioLibWrapper::calcMaxPacketMillis()` (MeshCore
/// 1.17.0, PR #2977): how long a genuine reception may legitimately stay in
/// each phase before the latched IRQ state is declared stuck.
///
/// - `preamble_ms`: `(preamble + 8 + 4.25) · T_sym` — rest of the preamble,
///   sync word, SFD and the 8-symbol explicit header.
/// - `payload_ms`: payload-only airtime of a [`MAX_TRANS_UNIT`] frame (RadioLib
///   `calculateTimeOnAir()` minus the preamble part above), scaled to CR 4/8
///   because the sender's CR rides in the header and may be more robust than
///   ours.
///
/// Parameters use the same MeshCore conventions as [`lora_airtime_ms`];
/// `preamble_syms` should be the longest preamble expected from peers.
/// SF8 / BW62.5 / CR4/5 / 32 symbols → 182 ms + 2130 ms.
pub fn max_packet_millis(sf: u8, bw_hz: u32, cr_proto: u8, preamble_syms: u16) -> PacketMillis {
    let sf = sf.clamp(5, 12);
    let tsym_us = (1_000_000u64 << sf) / bw_hz.max(1) as u64;
    // Semtech magic numbers for sync word + SFD (6.25 / 4.25 symbols, ×4).
    let sf_coeff1_x4: u64 = if sf <= 6 { 25 } else { 17 };
    let preamble_us = (((preamble_syms as u64 + 8) * 4 + sf_coeff1_x4) * tsym_us) / 4;

    // Coded payload symbols: explicit header (20), CRC on (16), auto-LDRO.
    let sf_coeff2: i64 = if sf <= 6 { 0 } else { 8 };
    let ldro = tsym_us > 16_000;
    let sf_div = 4 * (sf as i64 - if ldro { 2 } else { 0 });
    let bits = (8 * MAX_TRANS_UNIT as i64 + 16 - 4 * sf as i64 + sf_coeff2 + 20).max(0);
    let n_sym = ((bits + sf_div - 1) / sf_div) as u64;

    let cr = cr_proto.clamp(5, 8) as u64;
    let mut payload_us = n_sym * cr * tsym_us;
    if cr < 8 {
        payload_us = payload_us * 8 / cr; // rescale for max possible CR
    }

    PacketMillis {
        preamble_ms: preamble_us.div_ceil(1000) as u32,
        payload_ms: payload_us.div_ceil(1000) as u32,
    }
}

/// Raw bit pattern of a latched IRQ status (the sx126x crate keeps it
/// private), so exactly the flags that were read can be cleared — clearing
/// `IrqMask::all()` would also eat a flag that latched after the read.
fn irq_bits(irq: IrqStatus) -> u16 {
    [
        (irq.tx_done(), TxDone),
        (irq.rx_done(), RxDone),
        (irq.preamble_detected(), PreambleDetected),
        (irq.syncword_valid(), SyncwordValid),
        (irq.header_valid(), HeaderValid),
        (irq.header_error(), HeaderError),
        (irq.crc_err(), CrcErr),
        (irq.cad_done(), CadDone),
        (irq.cad_detected(), CadDetected),
        (irq.timeout(), Timeout),
    ]
    .into_iter()
    .filter(|(set, _)| *set)
    .fold(0, |acc, (_, bit)| acc | bit as u16)
}

/// Where an in-flight reception is, tracked in software across
/// `receive_packet()` calls (the select loop can cancel it mid-packet).
/// Mirrors upstream `CustomSX1262::_activityAt` / `_headerSeen`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum RxActivity {
    /// Nothing on air that we know of.
    Idle,
    /// PreambleDetected latched at this instant; HeaderValid is due within
    /// `PacketMillis::preamble_ms`.
    Preamble(Instant),
    /// HeaderValid latched at this instant; RxDone/CrcErr is due within
    /// `PacketMillis::payload_ms`.
    Header(Instant),
}

/// Result of one look at the latched IRQ flags.
enum RxPoll {
    /// No reception in flight — the channel is clear as far as we can hear.
    Clear,
    /// A preamble or header is in flight and still inside its window.
    Busy,
    /// A reception finished (RxDone/CrcErr). `Some` carries
    /// `(len, rssi, snr_x4)` of a good packet; `None` is a CRC error or an
    /// unreadable buffer.
    Done(Option<(usize, i16, i8)>),
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
    const WINDOW_MS: u32 = 3_600_000; // 1 hour in ms
    const MIN_RESERVE_MS: u32 = 100; // min budget before blocking TX
    const MIN_TX_DIV: u32 = 2; // require est_airtime / N as budget

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
        let now = embassy_time::Instant::now();
        let elapsed = (now - self.last_update).as_millis() as u32;
        let denom = 1_000 + self.af_x1000;
        let max_bud = (Self::WINDOW_MS as u64 * 1_000 / denom as u64) as u32;
        let refill = (elapsed as u64 * 1_000 / denom as u64) as u32;
        if refill > 0 {
            self.budget_ms = self.budget_ms.saturating_add(refill).min(max_bud);
            self.last_update = now;
        }
    }

    /// Returns `true` if TX is allowed for a packet with estimated airtime
    /// `est_ms`.
    ///
    /// Requires `budget_ms >= est_ms / MIN_TX_DIV` (same guard as C++
    /// Dispatcher).
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
    hw: RadioHw<'a>,
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
    /// RSSI of the last successfully received packet (dBm, negative).
    pub last_rssi: i16,
    /// SNR × 4 of the last successfully received packet.
    pub last_snr_x4: i8,
    /// Accumulated TX airtime in milliseconds (sum of measured TX durations).
    pub tx_air_ms: u32,
    /// Accumulated RX airtime in milliseconds (sum of per-packet on-air
    /// durations).
    pub rx_air_ms: u32,
    /// In-flight reception state. Survives a select() cancelling
    /// receive_packet() mid-packet, so send_message() knows to defer TX —
    /// and, unlike a bare flag, it expires on the [`PacketMillis`] windows
    /// so a false preamble can't hold the channel "busy" forever.
    rx_activity: RxActivity,
    /// Watchdog windows for `rx_activity`, recomputed on every reconfigure.
    rx_windows: PacketMillis,
    /// A packet that completed while send_message() was listening before
    /// talk. It is read out of the chip before the TX payload overwrites the
    /// shared data buffer, and handed out by the next receive_packet().
    stash: Option<(usize, i16, i8)>,
    stash_buf: [u8; MAX_TRANS_UNIT],
    /// SetRxDutyCycle is armed: between windows the chip sleeps with BUSY
    /// high, and any NSS edge wakes it and ends the cycle. Cleared whenever
    /// the chip is brought back to STDBY_RC.
    duty_armed: bool,
    /// send_message() is listening before talk / transmitting, so re-arms go
    /// to continuous RX. Reset by receive_packet() too, in case a caller
    /// ever cancels send_message() midway.
    duty_suspended: bool,
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
        // SX1262 datasheet rates SCK up to 16 MHz; SPIM2 (this peripheral)
        // caps at 8 MHz, so M8 is the ceiling here.  8× the previous M1,
        // which drops radio-command latency on every mesh hop.
        spi_cfg.frequency = Frequency::M8;
        let spim = Spim::new(spi, Irqs, sck_pin, mosi_pin, miso_pin, spi_cfg);

        let nss = Output::new(nss_pin, Level::High, OutputDrive::Standard);
        let nreset = Output::new(nrst_pin, Level::High, OutputDrive::Standard);
        let busy = Input::new(busy_pin, Pull::None);
        let ant = Output::new(ant_pin, Level::Low, OutputDrive::Standard);
        let dio1 = Input::new(dio1_pin, Pull::None);

        // AlwaysHigh is a dummy DIO1 for the sx126x struct; real async DIO1 waiting
        // is done externally via wait_for_high() so the executor is not blocked.
        let spi_dev = ExclusiveDevice::new(spim, nss, Delay).unwrap();

        let conf = build_lora_config(config);
        let mut hw = RadioHw {
            spi: spi_dev,
            nreset,
            busy,
            ant,
        };
        let mut lora = hw.sx();
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
            hw,
            tx_timeout: 0.into(),
            crc_type: LoRaCrcType::CrcOn,
            preamble_len: config.preamble_len,
            dio1,
            sf: config.sf_num,
            bw_hz: config.bw_hz_num,
            cr_proto: config.cr_num,
            tx_budget: None,
            last_rssi: 0,
            last_snr_x4: 0,
            tx_air_ms: 0,
            rx_air_ms: 0,
            rx_activity: RxActivity::Idle,
            rx_windows: rx_windows_for(config),
            stash: None,
            stash_buf: [0; MAX_TRANS_UNIT],
            duty_armed: false,
            duty_suspended: false,
        };
        radio.apply_rx_gain();
        if radio.duty_periods().is_some() {
            radio.rearm_rx();
        }
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
        self.hw.sx().write_register(Register::RxGain, &[value]).ok();
    }

    /// Put the radio into standby mode (STDBY_RC, ~600 µA).
    pub fn standby(&mut self) {
        if self.duty_armed {
            self.wake_to_standby();
        } else {
            self.hw.sx().wait_on_busy().ok();
            self.hw.sx()
                .set_standby(sx126x::op::StandbyConfig::StbyRc)
                .ok();
        }
        // Whatever was on air is gone from the receiver's point of view.
        self.rx_activity = RxActivity::Idle;
    }

    /// Bring a duty-cycling chip to STDBY_RC without spinning through its
    /// sleep: pulse NSS to wake it, wait (bounded) for BUSY, then
    /// SetStandby. A command clocked in during the wake-up is lost, hence the
    /// bare pulse first.
    fn wake_to_standby(&mut self) {
        let mut awake = false;
        for _ in 0..WAKE_TRIES {
            self.hw.nss_pulse();
            if self.hw.wait_busy_low(WAKE_BUSY_TIMEOUT_US) {
                awake = true;
                break;
            }
        }
        if !awake {
            defmt::warn!("SX1262: BUSY still high after wake pulses");
        }
        self.hw.sx()
            .set_standby(sx126x::op::StandbyConfig::StbyRc)
            .ok();
        self.hw.wait_busy_low(WAKE_BUSY_TIMEOUT_US);
        self.duty_armed = false;
    }

    /// Duty-cycle periods for the current setting and modulation, or `None`
    /// for continuous RX (power save off, preamble too short, or
    /// send_message() is listening / transmitting). Read on every re-arm, so
    /// a setting or preset change takes effect at the next one.
    fn duty_periods(&self) -> Option<RxDutyPeriods> {
        if self.duty_suspended || !crate::RX_POWER_SAVE.load(Ordering::Relaxed) {
            return None;
        }
        rx_power_save_periods(self.sf, self.bw_hz)
    }

    /// Resume continuous RX after standby.
    pub fn resume_rx(&mut self) {
        // Drop flags latched before standby so DIO1 starts low.
        self.hw.sx().wait_on_busy().ok();
        self.hw.sx().clear_irq_status(IrqMask::all()).ok();
        self.rx_activity = RxActivity::Idle;
        self.rearm_rx();
    }

    /// (Re-)enter RX with the configured gain and RF path: duty-cycled when
    /// the power-save mode allows it, continuous otherwise.
    fn rearm_rx(&mut self) {
        if let Some(periods) = self.duty_periods() {
            self.arm_rx_duty_cycle(periods);
            return;
        }
        if self.duty_armed {
            self.wake_to_standby();
        }
        self.hw.sx().wait_on_busy().ok();
        self.hw.sx().set_rx(RxTxTimeout::continuous_rx()).ok();
        self.apply_rx_gain();
        self.hw.sx().rf_switch_rx();
    }

    /// Arm SetRxDutyCycle (datasheet §13.1.7). The command must be sent from
    /// STDBY_RC, and the chip's warm-start sleep drops RxGain unless it is on
    /// the retention list (§9.6), so: standby, gain, retention list,
    /// StopTimerOnPreamble, then 0x94. No BUSY wait afterwards: BUSY stays
    /// high through every sleep.
    fn arm_rx_duty_cycle(&mut self, periods: RxDutyPeriods) {
        if self.duty_armed {
            self.wake_to_standby();
        } else {
            self.hw.sx().wait_on_busy().ok();
            self.hw.sx()
                .set_standby(sx126x::op::StandbyConfig::StbyRc)
                .ok();
            self.hw.sx().wait_on_busy().ok();
        }
        self.apply_rx_gain();
        self.hw.sx().wait_on_busy().ok();
        self.hw.write_register_raw(RX_GAIN_RETENTION_REG, &RX_GAIN_RETENTION);
        self.hw.sx().wait_on_busy().ok();
        self.hw.command(&STOP_TIMER_ON_PREAMBLE);
        self.hw.sx().wait_on_busy().ok();
        self.hw.sx().rf_switch_rx();
        self.hw.command(&periods.command());
        self.duty_armed = true;
    }

    /// Reprogram the radio in-place with new frequency / modulation / TX
    /// parameters.  Used by the listener loop to apply changes from the
    /// menu or BLE companion without rebooting.
    ///
    /// Every command is followed by `wait_on_busy()` because the chip's
    /// BUSY pin stays high between SPI ops and a back-to-back command
    /// is silently dropped while BUSY=1.  Image calibration is mandatory
    /// when changing band (Semtech §13.1.3).  `set_pa_config` is required
    /// alongside `set_tx_params` — TX power straddles both registers.
    pub fn reconfigure_radio(&mut self, config: &MeshCoreConfig) {
        self.standby();
        self.hw.sx().wait_on_busy().ok();

        let conf = build_lora_config(config);

        self.hw.sx().set_rf_frequency(conf.rf_freq).ok();
        self.hw.sx().wait_on_busy().ok();

        self.hw.sx()
            .calibrate_image(sx126x::op::CalibImageFreq::from_rf_frequency(
                conf.rf_frequency,
            ))
            .ok();
        self.hw.sx().wait_on_busy().ok();

        self.hw.sx().set_pa_config(conf.pa_config).ok();
        self.hw.sx().wait_on_busy().ok();

        self.hw.sx().set_tx_params(conf.tx_params).ok();
        self.hw.sx().wait_on_busy().ok();

        self.hw.sx().set_mod_params(conf.mod_params).ok();
        self.hw.sx().wait_on_busy().ok();

        // The preamble follows the SF (32 symbols at SF ≤ 8, else 16).
        if let Some(packet_params) = conf.packet_params {
            self.hw.sx().set_packet_params(packet_params).ok();
            self.hw.sx().wait_on_busy().ok();
        }

        // Update internal state used by airtime / path-len accounting.
        self.sf = config.sf_num;
        self.bw_hz = config.bw_hz_num;
        self.cr_proto = config.cr_num;
        self.preamble_len = config.preamble_len;
        self.rx_windows = rx_windows_for(config);
        defmt::debug!(
            "SX1262 RX watchdog: preamble={=u32}ms payload={=u32}ms",
            self.rx_windows.preamble_ms,
            self.rx_windows.payload_ms
        );
        match self.duty_periods() {
            Some(p) => defmt::info!(
                "SX1262 RX duty cycle: rx={=u32} sleep={=u32} (x15.625us)",
                p.rx,
                p.sleep
            ),
            None => defmt::info!("SX1262 RX: continuous"),
        }

        // Re-enters RX with the timings recomputed for the new params / mode.
        self.resume_rx();
    }

    /// Wait for the chip to enter RX mode, polling every 50 ms for up to 500
    /// ms. Returns true if RX mode is confirmed.
    ///
    /// The check runs in continuous RX (reading the status would wake a
    /// duty-cycling chip anyway); duty cycling is armed afterwards when the
    /// power-save mode is on.
    pub async fn ensure_rx(&mut self) -> bool {
        if self.duty_armed {
            self.wake_to_standby();
        }
        self.hw.sx().wait_on_busy().ok();
        self.hw.sx().set_rx(RxTxTimeout::continuous_rx()).ok();
        self.apply_rx_gain();
        self.hw.sx().rf_switch_rx();

        let mut in_rx = false;
        for _ in 0..10u8 {
            Timer::after_millis(50).await;
            if let Ok(s) = self.hw.sx().get_status()
                && matches!(s.chip_mode(), Some(ChipMode::RX))
            {
                in_rx = true;
                break;
            }
        }
        if in_rx && self.duty_periods().is_some() {
            self.rearm_rx();
        }
        in_rx
    }

    /// Wait for the next LoRa packet in continuous RX mode.
    ///
    /// Event-driven RX state machine, a port of upstream MeshCore 1.17.0
    /// `CustomSX1262::isReceiving()` (PRs #2977 / #3036):
    ///
    /// ```text
    /// Idle ─PreambleDetected─▶ Preamble ─HeaderValid─▶ Header ─RxDone/CrcErr─▶ Idle
    ///   ▲                         │ preamble_ms              │ payload_ms
    ///   └─────────────────────────┴── elapsed, or HeaderError┘
    /// ```
    ///
    /// DIO1 is awaited as a *level*, not an edge: anything latched — even a
    /// flag that landed between clearing the IRQs and arming the wait — is
    /// serviced instead of wedging the wait.  While a reception is in flight
    /// the wait is bounded by its [`PacketMillis`] window; while idle, a
    /// periodic check re-arms RX if the chip has silently left it.
    ///
    /// Cancel-safe: the state lives in `self`, so if `select()` cancels this
    /// mid-packet, `send_message()` still sees the channel busy (CANL:
    /// collision avoidance by neighbour listening) — until the window runs
    /// out.  Returns `Ok(None)` on a CRC error.
    ///
    /// Duty-cycled RX: the chip sleeps between windows and any NSS edge
    /// would wake it, so the IRQ status is only read once DIO1 is high, the
    /// idle watchdog re-issues SetRxDutyCycle instead of reading the chip
    /// mode, and a false preamble that times out re-arms the cycle.
    pub async fn receive_packet(
        &mut self,
        buf: &mut [u8],
    ) -> Result<Option<(usize, i16, i8)>, LoraError> {
        // A packet that completed while send_message() was listening.
        if let Some((len, rssi, snr_x4)) = self.stash.take() {
            if len <= buf.len() {
                buf[..len].copy_from_slice(&self.stash_buf[..len]);
                return Ok(Some((len, rssi, snr_x4)));
            }
            defmt::warn!(
                "LoRa RX buffer too small: pkt={=usize} buf={=usize}",
                len,
                buf.len()
            );
        }

        // Duty cycling stays suspended only while send_message() runs; if
        // that was ever cut short, pick the cycle back up here.
        if self.duty_suspended {
            self.duty_suspended = false;
            self.rearm_rx();
        }

        loop {
            if let RxPoll::Done(pkt) = self.poll_rx(Some(&mut *buf))? {
                return Ok(pkt);
            }

            // A phase deadline expiring is handled by the next poll_rx().
            let deadline = self
                .rx_deadline()
                .unwrap_or_else(|| Instant::now() + Duration::from_millis(RX_IDLE_WATCHDOG_MS));
            if with_deadline(deadline, self.dio1.wait_for_high())
                .await
                .is_err()
                && self.rx_activity == RxActivity::Idle
            {
                if self.duty_armed {
                    // Reading the chip mode would wake it: re-issue the duty
                    // cycle instead, which also recovers a chip that lost it.
                    self.rearm_rx();
                } else {
                    self.check_rx_mode();
                }
            }
        }
    }

    /// Service whatever is latched in the IRQ status and advance
    /// `rx_activity`, applying the upstream watchdog windows.
    ///
    /// Only the flags that were read get cleared, so DIO1 drops unless a new
    /// event latched meanwhile.  A finished packet is read into `buf`, or
    /// into the stash when `buf` is `None` (the TX path, which is about to
    /// overwrite the chip's shared data buffer).
    ///
    /// While duty-cycling, the status is only read with DIO1 high (the chip
    /// raised an IRQ, so it is awake); otherwise only the watchdog windows
    /// below are applied, without touching SPI.
    fn poll_rx(&mut self, buf: Option<&mut [u8]>) -> Result<RxPoll, LoraError> {
        let irq = if self.duty_armed && self.dio1.is_low() {
            IrqStatus::from(0u16)
        } else {
            self.hw.sx().wait_on_busy().ok();
            self.hw
                .sx()
                .get_irq_status()
                .map_err(|_| LoraError::Spi("get_irq_status failed"))?
        };
        let bits = irq_bits(irq);
        if bits != 0 {
            self.hw.sx().wait_on_busy().ok();
            self.hw.sx().clear_irq_status(IrqMask::from(bits)).ok();
        }
        let now = Instant::now();

        // ── Terminal event: RxDone / CrcErr (Timeout can't fire in
        // continuous RX, but if it ever latches, re-arming is right too) ──
        if irq.rx_done() || irq.crc_err() || irq.timeout() {
            self.rx_activity = RxActivity::Idle;
            let pkt = if irq.crc_err() {
                let rssi = self
                    .hw
                    .sx()
                    .get_packet_status()
                    .map(|s| s.rssi_pkt() as i16)
                    .unwrap_or(0);
                defmt::warn!(
                    "LoRa CRC error {=i16}dBm — header decoded OK (SF/BW/CR/syncword match) but payload bytes corrupted",
                    rssi
                );
                crate::PACKET_STATS
                    .recv_errors
                    .fetch_add(1, Ordering::Relaxed);
                None
            } else if irq.rx_done() {
                let pkt = match buf {
                    Some(buf) => read_rx_packet(&mut self.hw, buf),
                    None => {
                        if self.stash.is_some() {
                            defmt::warn!("LoRa RX stash overwritten — earlier packet dropped");
                        }
                        self.stash = read_rx_packet(&mut self.hw, &mut self.stash_buf);
                        self.stash
                    }
                };
                if let Some((len, rssi, snr_x4)) = pkt {
                    self.last_rssi = rssi;
                    self.last_snr_x4 = snr_x4;
                    let pkt_air_ms =
                        lora_airtime_ms(len, self.sf, self.bw_hz, self.cr_proto, self.preamble_len);
                    self.rx_air_ms = self.rx_air_ms.saturating_add(pkt_air_ms);
                }
                pkt
            } else {
                None
            };

            // Re-arm continuous RX after every terminal event.
            self.rearm_rx();
            return Ok(RxPoll::Done(pkt));
        }

        // ── HeaderError: the "preamble" was noise or a collision ──────────
        if irq.header_error() {
            defmt::debug!("LoRa header error — dropping in-flight RX");
            self.rx_activity = RxActivity::Idle;
            self.rearm_rx();
            return Ok(RxPoll::Clear);
        }

        if irq.header_valid() {
            // A fresh HeaderValid always starts a fresh payload window.
            self.rx_activity = RxActivity::Header(now);
        } else if irq.preamble_detected() && self.rx_activity == RxActivity::Idle {
            // Re-detections don't extend the window (upstream keeps the
            // first `_activityAt`), so steady noise can't pin the channel.
            self.rx_activity = RxActivity::Preamble(now);
        }

        // ── Watchdog windows ──────────────────────────────────────────────
        match self.rx_activity {
            RxActivity::Idle => Ok(RxPoll::Clear),
            RxActivity::Preamble(since)
                if (now - since).as_millis() > self.rx_windows.preamble_ms as u64 =>
            {
                // Upstream only drops the flag here: the modem has already
                // gone back to searching, and restarting RX could clip a
                // peer's unusually long preamble.
                defmt::debug!(
                    "LoRa preamble without header after {=u32}ms — channel clear",
                    self.rx_windows.preamble_ms
                );
                self.rx_activity = RxActivity::Idle;
                if self.duty_armed {
                    // StopTimerOnPreamble keeps the chip in RX after a
                    // preamble; nothing else ends that on a false one.
                    self.rearm_rx();
                }
                Ok(RxPoll::Clear)
            }
            RxActivity::Header(since)
                if (now - since).as_millis() > self.rx_windows.payload_ms as u64 =>
            {
                defmt::warn!(
                    "LoRa HeaderValid but no RxDone after {=u32}ms — restarting RX",
                    self.rx_windows.payload_ms
                );
                self.rx_activity = RxActivity::Idle;
                self.rearm_rx();
                Ok(RxPoll::Clear)
            }
            _ => Ok(RxPoll::Busy),
        }
    }

    /// When the current phase's watchdog window runs out (+1 ms so the
    /// strict `>` in poll_rx() has definitely passed), or `None` when idle.
    fn rx_deadline(&self) -> Option<Instant> {
        let (since, window_ms) = match self.rx_activity {
            RxActivity::Idle => return None,
            RxActivity::Preamble(since) => (since, self.rx_windows.preamble_ms),
            RxActivity::Header(since) => (since, self.rx_windows.payload_ms),
        };
        Some(since + Duration::from_millis(window_ms as u64 + 1))
    }

    /// Idle-RX watchdog: the chip should sit in continuous RX indefinitely,
    /// but if a dropped SPI command or a brown-out leaves it elsewhere,
    /// nothing would ever raise DIO1 again.  Re-arm if so.
    fn check_rx_mode(&mut self) {
        self.hw.sx().wait_on_busy().ok();
        let in_rx = matches!(
            self.hw.sx().get_status().ok().and_then(|s| s.chip_mode()),
            Some(ChipMode::RX)
        );
        if !in_rx {
            defmt::warn!("SX1262 watchdog: radio not in RX — re-arming");
            self.hw.sx().wait_on_busy().ok();
            self.hw.sx().clear_irq_status(IrqMask::all()).ok();
            self.rearm_rx();
        }
    }

    /// Listen before talk while duty-cycling. Asleep, the chip misses a
    /// preamble that starts between windows, so the "channel busy" view
    /// send_message() relies on would be stale. Switch to continuous RX and
    /// listen for one full cycle plus one window: the sleep was sized so
    /// every peer preamble is at least that long, so one already in progress
    /// is still caught and flagged by poll_rx().
    ///
    /// A continuous listen rather than CAD: it keeps full receiver
    /// sensitivity (CAD loses reliability beyond ~400 m, see send_message()),
    /// feeds the same PreambleDetected → HeaderValid state machine the rest
    /// of the LBT logic uses, and still catches a packet that completes
    /// meanwhile (stashed for receive_packet()). A packet already past its
    /// preamble cannot be missed either: by construction one duty window
    /// fell inside that preamble, so rx_activity already says busy.
    ///
    /// Duty cycling stays suspended (re-arms go to continuous RX) until the
    /// transmit has finished.
    async fn listen_continuously_before_talk(&mut self) {
        let Some(periods) = self.duty_periods() else {
            return;
        };
        self.duty_suspended = true;
        if !self.duty_armed {
            // Already in continuous RX (e.g. the mode was just switched on
            // and not applied yet): the busy view is current.
            return;
        }
        // Anything already latched is serviced first (DIO1 high: awake). A
        // finished packet is stashed and re-arms into continuous RX.
        if self.dio1.is_high() {
            let _ = self.poll_rx(None);
        }
        if self.rx_activity != RxActivity::Idle {
            // Mid-reception the chip stays in RX (StopTimerOnPreamble);
            // the LBT loop in send_message() waits it out.
            return;
        }
        if self.duty_armed {
            self.rearm_rx(); // suspended → wakes the chip, continuous RX
        }
        with_timeout(
            Duration::from_micros(periods.lbt_listen_us()),
            self.dio1.wait_for_high(),
        )
        .await
        .ok();
    }

    /// Leave the send_message() suspension and re-arm duty-cycled RX.
    fn resume_duty_cycle(&mut self) {
        if self.duty_suspended {
            self.duty_suspended = false;
            self.rearm_rx();
        }
    }

    pub async fn send_message(&mut self, message: &[u8]) -> Result<(), LoraError> {
        // ── Listen before talk (CANL) ─────────────────────────────────────────
        // The radio is in continuous RX here (duty-cycled RX is switched to
        // continuous first, see listen_continuously_before_talk()), so it
        // detects ongoing transmissions with full receiver sensitivity —
        // strictly better than CAD which loses reliability beyond ~400 m.  poll_rx() is upstream's
        // isReceiving(): busy from PreambleDetected until RxDone, bounded by
        // the PacketMillis windows, so a false preamble costs at most
        // `preamble_ms` instead of the full give-up time.  Retry cadence and
        // give-up mirror upstream Dispatcher::checkSend()
        // (getCADFailRetryDelay / getCADFailMaxDuration).
        const INITIAL_WINDOW_MS: u64 = 200;
        const CAD_FAIL_RETRY_DELAY_MS: u64 = 200;
        const CAD_FAIL_MAX_DURATION_MS: u64 = 4_000;

        // Random pre-TX jitter: prevents all nodes transmitting simultaneously
        // after a shared event (e.g. all wanting to relay the same flood).
        Timer::after_millis(random_backoff_ms(INITIAL_WINDOW_MS)).await;

        self.listen_continuously_before_talk().await;

        let lbt_start = Instant::now();
        let mut deferred = false;
        loop {
            // A packet finishing here is stashed for the next receive_packet().
            let busy = matches!(self.poll_rx(None), Ok(RxPoll::Busy));
            if !busy {
                if !deferred {
                    break;
                }
                // The reception we waited out just ended: random backoff so
                // every node that deferred on it doesn't key up at once, then
                // look again.
                deferred = false;
                defmt::debug!(
                    "TX: channel clear after {=u64}ms",
                    lbt_start.elapsed().as_millis()
                );
                Timer::after_millis(random_backoff_ms(INITIAL_WINDOW_MS)).await;
                continue;
            }
            if lbt_start.elapsed() > Duration::from_millis(CAD_FAIL_MAX_DURATION_MS) {
                // Upstream ERR_EVENT_CAD_TIMEOUT: activity has gone on too
                // long (radio may be in a bad state) — force the transmit.
                defmt::warn!(
                    "TX: channel busy for {=u64}ms — transmitting anyway",
                    CAD_FAIL_MAX_DURATION_MS
                );
                break;
            }
            if !deferred {
                defmt::debug!("TX deferred: reception in progress");
                deferred = true;
            }
            // Wake on the next RX event or the retry tick, whichever is first.
            with_timeout(
                Duration::from_millis(CAD_FAIL_RETRY_DELAY_MS),
                self.dio1.wait_for_high(),
            )
            .await
            .ok();
        }

        // ── Duty-cycle budget ─────────────────────────────────────────────────
        let est_ms = lora_airtime_ms(
            message.len(),
            self.sf,
            self.bw_hz,
            self.cr_proto,
            self.preamble_len,
        );
        if let Some(ref mut budget) = self.tx_budget
            && !budget.can_tx(est_ms)
        {
            defmt::warn!(
                "TX duty-cycle limit: est={=u32}ms budget={=u32}ms — packet dropped",
                est_ms,
                budget.budget_ms,
            );
            self.resume_duty_cycle();
            return Err(LoraError::DutyCycle);
        }

        // Leave RX before staging the payload: otherwise an incoming packet
        // could overwrite the shared data buffer, or latch an RX IRQ that the
        // level-triggered TxDone wait below would mistake for completion.
        // (RadioLib remaps DIO1 to TxDone-only for the same reason.)  No
        // await since the last poll_rx(), so nothing finished needs stashing;
        // whatever was still in flight is lost to us either way.
        self.standby();
        self.hw.sx().wait_on_busy().ok();
        self.hw.sx().clear_irq_status(IrqMask::all()).ok();

        // High-current PA window — hold buck/boost in boost until TxDone.
        let _tx_boost = crate::fw::power::boost(crate::fw::power::Source::Lora);
        self.hw.sx().rf_switch_tx();

        self.hw.sx().write_buffer(0x00, message).unwrap();
        let packet_params = LoRaPacketParams::default()
            .set_preamble_len(self.preamble_len)
            .set_payload_len(message.len() as u8)
            .set_crc_type(self.crc_type)
            .into();
        self.hw.sx().set_packet_params(packet_params).unwrap();

        let tx_start = Instant::now();
        if self.hw.sx().set_tx(self.tx_timeout).is_err() {
            drop(_tx_boost);
            self.duty_suspended = false;
            self.rearm_rx();
            return Err(LoraError::Timeout);
        }

        // TxDone watchdog: upstream Dispatcher gives a send est_airtime × 1.5
        // before giving up on it; the slack covers our SPI / scheduling
        // latency, which upstream's estimate doesn't have to.
        let tx_deadline_ms = est_ms as u64 * 3 / 2 + TX_DONE_SLACK_MS;
        let tx_done = with_timeout(
            Duration::from_millis(tx_deadline_ms),
            self.dio1.wait_for_high(),
        )
        .await
        .is_ok();
        let actual_ms = tx_start.elapsed().as_millis() as u32;
        // TxDone — PA is off; drop back to power-save (unless another vote holds).
        drop(_tx_boost);

        if !tx_done {
            defmt::warn!(
                "TX: no TxDone after {=u32}ms (est {=u32}ms) — aborting",
                actual_ms,
                est_ms
            );
            self.standby();
        }
        self.hw.sx().wait_on_busy().ok();
        self.hw.sx().clear_irq_status(IrqMask::all()).ok();

        self.tx_air_ms = self.tx_air_ms.saturating_add(actual_ms);

        // Deduct measured airtime from budget (also on abort: the PA was on).
        if let Some(ref mut budget) = self.tx_budget {
            budget.deduct(actual_ms);
            defmt::debug!(
                "TX done: actual={=u32}ms budget_remaining={=u32}ms",
                actual_ms,
                budget.budget_ms
            );
        }

        // Re-arm RX (duty-cycled again if enabled) so receive_packet() finds
        // the chip receiving.
        self.duty_suspended = false;
        self.rearm_rx();

        if tx_done {
            crate::PACKET_STATS.count_sent(message[0]);
            Ok(())
        } else {
            Err(LoraError::Timeout)
        }
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Return a pseudo-random delay in `[0, window_ms)` using the embassy tick
/// counter as entropy.  Suitable for CSMA backoff — not cryptographic.
///
/// Uses a single LCG iteration to spread the tick value uniformly across
/// the window, avoiding the clustering that plain `ticks % window` can produce
/// when `window` is not a power of two.
fn random_backoff_ms(window_ms: u64) -> u64 {
    let ticks = embassy_time::Instant::now().as_ticks();
    // Knuth multiplicative hash (LCG constant from Numerical Recipes).
    let mixed = ticks
        .wrapping_mul(0x5851_F42D_4C95_7F2D)
        .wrapping_add(0x1405_7B7E_F767_814F);
    mixed % window_ms.max(1)
}

/// Pull the packet RxDone just flagged out of the chip's data buffer.
/// Returns `(len, rssi_dBm, snr×4)`, or `None` if it doesn't fit / SPI fails.
fn read_rx_packet(hw: &mut RadioHw<'_>, buf: &mut [u8]) -> Option<(usize, i16, i8)> {
    let mut lora = hw.sx();
    lora.wait_on_busy().ok();
    let Ok(status) = lora.get_rx_buffer_status() else {
        defmt::warn!("LoRa RX: get_rx_buffer_status failed");
        return None;
    };
    let len = status.payload_length_rx() as usize;
    if len > buf.len() {
        defmt::warn!(
            "LoRa RX buffer too small: pkt={=usize} buf={=usize}",
            len,
            buf.len()
        );
        return None;
    }
    lora.read_buffer(status.rx_start_buffer_pointer(), &mut buf[..len])
        .ok()?;
    let (rssi, snr_x4) = lora
        .get_packet_status()
        .map(|s| (s.rssi_pkt() as i16, (s.snr_pkt() * 4.0) as i8))
        .unwrap_or((0, 0));
    Some((len, rssi, snr_x4))
}

/// Receive-watchdog windows for `config`. The preamble window covers the
/// longer of our own preamble and the one stock MeshCore sends at this SF,
/// since that is what peers actually put on air.
fn rx_windows_for(config: &MeshCoreConfig) -> PacketMillis {
    let preamble = config
        .preamble_len
        .max(preamble_length_for_sf(config.sf_num));
    max_packet_millis(config.sf_num, config.bw_hz_num, config.cr_num, preamble)
}

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

    // PreambleDetected / HeaderValid / HeaderError drive the RX watchdog in
    // SimpleLoRa::poll_rx(): every step of a reception raises DIO1, so a
    // false preamble is caught by HeaderError or the preamble window rather
    // than leaving the channel "busy" until the next real packet.
    let dio1_irq_mask = IrqMask::none()
        .combine(TxDone)
        .combine(RxDone)
        .combine(CrcErr)
        .combine(Timeout)
        .combine(PreambleDetected)
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
