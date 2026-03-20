//! Battery voltage measurement via SAADC.
//!
//! P0_31 (AIN7) is connected to a 1/3 voltage divider enabled by P0_07 (vbat_rd).
//! Pull vbat_rd low to enable the divider, sample, then pull high to save power.
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
//! Call `init()` once at startup, then call `read_mv().await` from any async task.

use embassy_nrf::{
    Peri,
    gpio::{Level, Output, OutputDrive},
    peripherals,
    saadc::{self, Config, Saadc},
};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, mutex::Mutex};
use embassy_time::Timer;

embassy_nrf::bind_interrupts!(pub struct BatteryIrqs {
    SAADC => saadc::InterruptHandler;
});

// ---------------------------------------------------------------------------
// BatteryMonitor — owns the peripherals, reborrowed on each measurement
// ---------------------------------------------------------------------------

pub struct BatteryMonitor {
    saadc: Peri<'static, peripherals::SAADC>,
    vbat: Peri<'static, peripherals::P0_31>,
    vbat_rd: Output<'static>,
}

impl BatteryMonitor {
    fn new(
        saadc: Peri<'static, peripherals::SAADC>,
        vbat: Peri<'static, peripherals::P0_31>,
        vbat_rd: Peri<'static, peripherals::P0_07>,
    ) -> Self {
        Self {
            saadc,
            vbat,
            // Keep the divider disabled (high) until a measurement is requested.
            vbat_rd: Output::new(vbat_rd, Level::High, OutputDrive::Standard),
        }
    }

    async fn read_mv(&mut self) -> u16 {
        // Enable the voltage divider and let the RC network settle.
        self.vbat_rd.set_low();
        Timer::after_millis(1).await;

        let ch_cfg = saadc::ChannelConfig::single_ended(self.vbat.reborrow());
        let mut saadc = Saadc::new(
            self.saadc.reborrow(),
            BatteryIrqs,
            Config::default(),
            [ch_cfg],
        );

        let mut buf = [0i16; 1];
        saadc.sample(&mut buf).await;

        // Disable the divider to save power.
        self.vbat_rd.set_high();

        let raw = buf[0].max(0) as u32;
        ((raw * 10800) / 4096) as u16
    }
}

// ---------------------------------------------------------------------------
// Voltage → percentage lookup table
// ---------------------------------------------------------------------------

/// Voltages outside this range trigger a `BatteryError` during `init()`.
const VBAT_MIN_MV: u16 = 3000;
const VBAT_MAX_MV: u16 = 4300;

/// (millivolts, percent) pairs, sorted ascending by voltage.
/// Edit this table to recalibrate for a different cell chemistry or pack.
static BATTERY_CURVE: &[(u16, u8)] = &[
    (3300, 0),
    (3400, 5),
    (3500, 10),
    (3600, 15),
    (3650, 20),
    (3700, 30),
    (3750, 40),
    (3800, 50),
    (3850, 60),
    (3900, 70),
    (3950, 75),
    (4000, 80),
    (4050, 85),
    (4100, 90),
    (4150, 95),
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
            // Linear interpolation, all integer arithmetic.
            let num = (mv - a_mv) as u32 * (b_pct - a_pct) as u32;
            let den = (b_mv - a_mv) as u32;
            return a_pct + (num / den) as u8;
        }
    }
    hi_pct
}

// ---------------------------------------------------------------------------
// Async-safe singleton
// ---------------------------------------------------------------------------

/// Voltage is outside the expected range for a single Li-ion cell.
#[derive(Debug, defmt::Format)]
pub enum BatteryError {
    /// Measured voltage is below 3000 mV — cell may be dead, missing, or shorted.
    TooLow(u16),
    /// Measured voltage is above 4300 mV — cell may be overcharged or reading is wrong.
    TooHigh(u16),
}

static MONITOR: Mutex<CriticalSectionRawMutex, Option<BatteryMonitor>> = Mutex::new(None);

/// Initialise the battery monitor and perform a sanity-check measurement.
///
/// Logs the measured voltage and returns `Err(BatteryError)` if it falls
/// outside [3000 mV, 4300 mV].  Call once at startup from the main task.
pub async fn init(
    saadc: Peri<'static, peripherals::SAADC>,
    vbat: Peri<'static, peripherals::P0_31>,
    vbat_rd: Peri<'static, peripherals::P0_07>,
) -> Result<(), BatteryError> {
    let mut monitor = BatteryMonitor::new(saadc, vbat, vbat_rd);
    let mv = monitor.read_mv().await;

    defmt::info!("Battery: {} mV ({}%)", mv, mv_to_pct(mv));

    let result = if mv < VBAT_MIN_MV {
        defmt::warn!("Battery voltage too low: {} mV", mv);
        Err(BatteryError::TooLow(mv))
    } else if mv > VBAT_MAX_MV {
        defmt::warn!("Battery voltage too high: {} mV", mv);
        Err(BatteryError::TooHigh(mv))
    } else {
        Ok(())
    };

    *MONITOR.lock().await = Some(monitor);
    result
}

/// Read the battery voltage in millivolts.  Panics if `init()` has not been called.
pub async fn read_mv() -> u16 {
    MONITOR
        .lock()
        .await
        .as_mut()
        .expect("battery::init() not called")
        .read_mv()
        .await
}

/// Read the battery state-of-charge as a percentage (0–100).
/// Uses linear interpolation between entries in `BATTERY_CURVE`.
/// Panics if `init()` has not been called.
pub async fn read_pct() -> u8 {
    mv_to_pct(read_mv().await)
}
