//! Battery voltage measurement via SAADC.
//!
//! P0_31 (AIN7) is connected to a 1/3 voltage divider enabled by P0_07
//! (vbat_rd). Pull vbat_rd low to enable the divider, sample, then pull high to
//! save power.
//!
//! The SAADC is configured with:
//!   - Gain 1/6, reference 0.6 V  →  full-scale input = 3.6 V
//!   - 12-bit resolution           →  4096 counts = 3600 mV at the pin
//!
//! The pin sees 1/3 of Vbat, so:
//!   Vbat_mV = (raw * 3600 * 3) / 4096  =  (raw * 10800) / 4096
//!
//! # Usage
//!
//! Once at startup:
//! ```
//! let monitor = battery::init(p.SAADC, board!(p, vbat), board!(p, vbat_rd)).await?;
//! spawner.must_spawn(battery::battery_task(monitor));
//! ```
//!
//! From any task (sync, no await needed):
//! ```
//! let pct = battery::read_pct();
//! let mv = battery::read_mv();
//! ```

use core::sync::atomic::{AtomicU8, AtomicU16, Ordering};

use embassy_nrf::gpio::{AnyPin, Input, Level, Output, OutputDrive, Pull};
use embassy_nrf::saadc::{self, Config, Saadc};
use embassy_nrf::{Peri, peripherals};
use embassy_time::{Duration, Timer};

embassy_nrf::bind_interrupts!(pub struct BatteryIrqs {
    SAADC => saadc::InterruptHandler;
});

// ---------------------------------------------------------------------------
// Cached state — updated by battery_task, read by anyone synchronously
// ---------------------------------------------------------------------------

static CACHED_MV: AtomicU16 = AtomicU16::new(0);
static CACHED_PCT: AtomicU8 = AtomicU8::new(0);
static CACHED_CHARGING: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

/// Read the last measured battery voltage in millivolts.
/// Returns 0 until the first measurement has completed.
pub fn read_mv() -> u16 {
    CACHED_MV.load(Ordering::Relaxed)
}

/// Read the last measured battery state-of-charge as a percentage (0–100).
/// Returns 0 until the first measurement has completed.
pub fn read_pct() -> u8 {
    CACHED_PCT.load(Ordering::Relaxed)
}

/// Returns true when the battery is currently charging.
pub fn is_charging() -> bool {
    CACHED_CHARGING.load(Ordering::Relaxed)
}

// ---------------------------------------------------------------------------
// BatteryMonitor — owns the peripherals, reborrowed on each measurement
// ---------------------------------------------------------------------------

pub struct BatteryMonitor {
    saadc: Peri<'static, peripherals::SAADC>,
    vbat: Peri<'static, peripherals::P0_31>,
    vbat_rd: Output<'static>,
    charge: Input<'static>,
}

impl BatteryMonitor {
    fn new(
        saadc: Peri<'static, peripherals::SAADC>,
        vbat: Peri<'static, peripherals::P0_31>,
        vbat_rd: Peri<'static, AnyPin>,
        charge: Peri<'static, AnyPin>,
    ) -> Self {
        Self {
            saadc,
            vbat,
            // Keep the divider disabled (high) between measurements.
            vbat_rd: Output::new(vbat_rd, Level::High, OutputDrive::Standard),
            // External 10k pullup on PCB; no internal pull needed.
            charge: Input::new(charge, Pull::None),
        }
    }

    async fn read_mv(&mut self) -> u16 {
        // Enable the voltage divider and let the RC network settle.
        self.vbat_rd.set_low();
        Timer::after_millis(5).await;

        // Take 3 samples and average to reduce noise.
        let mut sum: u32 = 0;
        for _ in 0..3 {
            let ch_cfg = saadc::ChannelConfig::single_ended(self.vbat.reborrow());
            let mut saadc = Saadc::new(
                self.saadc.reborrow(),
                BatteryIrqs,
                Config::default(),
                [ch_cfg],
            );
            let mut buf = [0i16; 1];
            saadc.sample(&mut buf).await;
            sum += buf[0].max(0) as u32;
        }

        // Disable the divider to save power.
        self.vbat_rd.set_high();

        let raw = sum / 3;
        ((raw * 10800) / 4096) as u16
    }
}

// ---------------------------------------------------------------------------
// Voltage → percentage lookup table
// ---------------------------------------------------------------------------

/// Voltages outside this range trigger a `BatteryError` during `init()`.
/// The display shows "Battery voltage critical" when init errors.
const VBAT_MIN_MV: u16 = 3000; // trickle-charge threshold
const VBAT_MAX_MV: u16 = 4400; // above this is treated as critical — a real
// overcharge or a broken divider, not just a
// CV-peak overshoot.  Readings between the
// curve's top (4250 mV = 100%) and this limit
// are clamped to 100% by `mv_to_pct`.

/// (millivolts, percent) pairs, sorted ascending by voltage.
/// Edit this table to recalibrate for a different cell chemistry or pack.
static BATTERY_CURVE: &[(u16, u8)] = &[
    (3200, 0),
    (3300, 5),
    (3400, 10),
    (3500, 15),
    (3550, 20),
    (3600, 30),
    (3650, 40),
    (3700, 50),
    (3750, 60),
    (3800, 70),
    (3850, 75),
    (3900, 80),
    (3950, 85),
    (4000, 90),
    (4050, 95),
    (4200, 100),
];

/// Convert a millivolt reading to a percentage using linear interpolation
/// between the nearest two entries in `BATTERY_CURVE`.
fn mv_to_pct(mv: u16) -> u8 {
    let &(lo_mv, lo_pct) = BATTERY_CURVE.first().unwrap();
    let &(hi_mv, hi_pct) = BATTERY_CURVE.last().unwrap();

    if mv <= lo_mv {
        return lo_pct;
    }
    if mv >= hi_mv {
        return hi_pct;
    }

    for window in BATTERY_CURVE.windows(2) {
        let (a_mv, a_pct) = window[0];
        let (b_mv, b_pct) = window[1];
        if mv <= b_mv {
            let num = (mv - a_mv) as u32 * (b_pct - a_pct) as u32;
            let den = (b_mv - a_mv) as u32;
            return a_pct + (num / den) as u8;
        }
    }
    hi_pct
}

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Voltage is outside the expected range for a single Li-ion cell.
#[derive(Debug, defmt::Format)]
pub enum BatteryError {
    /// Measured voltage is below 3000 mV — cell may be dead, missing, or
    /// shorted.
    TooLow(u16),
    /// Measured voltage is above 4300 mV — cell may be overcharged or reading
    /// is wrong.
    TooHigh(u16),
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Perform the first measurement, populate the cache, and return a
/// `BatteryMonitor` to hand off to [`battery_task`].
///
/// Returns `Err(BatteryError)` if the measured voltage is outside
/// [3000 mV, 4300 mV]; the cache is still populated so callers can
/// choose to continue with a warning rather than panicking.
pub async fn init(
    saadc: Peri<'static, peripherals::SAADC>,
    vbat: Peri<'static, peripherals::P0_31>,
    vbat_rd: Peri<'static, AnyPin>,
    charge: Peri<'static, AnyPin>,
) -> Result<BatteryMonitor, BatteryError> {
    let mut monitor = BatteryMonitor::new(saadc, vbat, vbat_rd, charge);
    let mv = monitor.read_mv().await;

    CACHED_MV.store(mv, Ordering::Relaxed);
    CACHED_PCT.store(mv_to_pct(mv), Ordering::Relaxed);
    CACHED_CHARGING.store(monitor.charge.is_low(), Ordering::Relaxed);

    defmt::debug!(
        "Battery: {} mV ({}%) charging={}",
        mv,
        mv_to_pct(mv),
        monitor.charge.is_low()
    );

    if mv < VBAT_MIN_MV {
        defmt::warn!("Battery voltage too low: {} mV", mv);
        return Err(BatteryError::TooLow(mv));
    }
    if mv > VBAT_MAX_MV {
        defmt::warn!("Battery voltage too high: {} mV", mv);
        return Err(BatteryError::TooHigh(mv));
    }

    Ok(monitor)
}

/// Background task that wakes once per minute, measures the battery, and
/// updates the cached values read by [`read_mv`] and [`read_pct`].
///
/// Spawn this task after calling [`init`].
#[embassy_executor::task]
pub async fn battery_task(mut monitor: BatteryMonitor) {
    loop {
        Timer::after(Duration::from_secs(60)).await;

        let mv = monitor.read_mv().await;
        CACHED_MV.store(mv, Ordering::Relaxed);
        CACHED_PCT.store(mv_to_pct(mv), Ordering::Relaxed);
        CACHED_CHARGING.store(monitor.charge.is_low(), Ordering::Relaxed);

        defmt::debug!(
            "Battery: {} mV ({}%) charging={}",
            mv,
            mv_to_pct(mv),
            monitor.charge.is_low()
        );
    }
}
