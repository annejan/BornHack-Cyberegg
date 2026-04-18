//! nRF52840 die temperature sensor.
//!
//! Reads the built-in TEMP peripheral and caches the result so any task can
//! read the last known temperature without touching the peripheral again.
//!
//! The TEMP peripheral is owned by the BLE/MPSL stack after startup, so the
//! only safe window to take a reading is at boot before MPSL initialises.
//! Call [`read_and_cache`] once at startup; thereafter use [`last_c10`].

use core::sync::atomic::{AtomicI16, Ordering};

use embassy_nrf::{bind_interrupts, peripherals, temp::Temp};

bind_interrupts!(struct Irqs {
    TEMP => embassy_nrf::temp::InterruptHandler;
});

/// Cached die temperature in °C × 10 (e.g. 255 = 25.5 °C).
/// `i16::MIN` until [`read_and_cache`] has been called.
static CACHED_TEMP_C10: AtomicI16 = AtomicI16::new(i16::MIN);

/// Read the nRF52840 die temperature, cache it, and return °C.
///
/// Uses `unsafe { peripherals::TEMP::steal() }` so the caller can retain
/// `p.TEMP` for the BLE stack.  The stolen peripheral is dropped before
/// returning, leaving the register state clean for MPSL.
pub async fn read_and_cache() -> i16 {
    let mut sensor = Temp::new(unsafe { peripherals::TEMP::steal() }, Irqs);
    let t = sensor.read().await.to_num::<i16>();
    drop(sensor);
    CACHED_TEMP_C10.store(t * 10, Ordering::Relaxed);
    t
}

/// Return the last cached die temperature as °C × 10 (e.g. 255 = 25.5 °C),
/// or `i16::MIN` if [`read_and_cache`] has not been called yet.
pub fn last_c10() -> i16 {
    CACHED_TEMP_C10.load(Ordering::Relaxed)
}

/// Read the current temperature via MPSL (safe after MPSL init).
///
/// Returns °C × 10. MPSL's `mpsl_temperature_get()` returns units of
/// 0.25 °C, so we multiply by 2.5 (raw * 10 / 4).
#[cfg(feature = "mesh")]
pub fn read_via_mpsl() -> i16 {
    let raw = unsafe { nrf_mpsl::raw::mpsl_temperature_get() };
    let c10 = (raw * 10 / 4) as i16;
    CACHED_TEMP_C10.store(c10, Ordering::Relaxed);
    c10
}
