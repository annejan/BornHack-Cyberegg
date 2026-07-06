//! nRF52840 die temperature sensor.
//!
//! Reads the built-in TEMP peripheral and caches the result so any task can
//! read the last known temperature without touching the peripheral again.
//!
//! The TEMP peripheral is owned by the BLE/MPSL stack after startup, so the
//! only safe window to take a reading is at boot before MPSL initialises.
//! Call [`read_and_cache`] once at startup; thereafter use [`last_c10`].

use core::sync::atomic::{AtomicBool, AtomicI16, AtomicU32, Ordering};

use embassy_nrf::temp::Temp;
use embassy_nrf::{bind_interrupts, peripherals};
use embassy_time::Instant;

bind_interrupts!(struct Irqs {
    TEMP => embassy_nrf::temp::InterruptHandler;
});

/// Cached die temperature in °C × 10 (e.g. 255 = 25.5 °C).
/// `i16::MIN` until [`read_and_cache`] has been called.
static CACHED_TEMP_C10: AtomicI16 = AtomicI16::new(i16::MIN);

/// Read the nRF52840 die temperature, cache it, and return °C.
///
/// Uses `unsafe { peripherals::TEMP::steal() }` so the caller can retain
/// `p.TEMP` for the BLE stack.  The stolen wrapper goes out of scope at
/// the end of the function — `embassy_nrf::temp::Temp` doesn't implement
/// `Drop`, so there's nothing to clean up explicitly.
pub async fn read_and_cache() -> i16 {
    let mut sensor = Temp::new(unsafe { peripherals::TEMP::steal() }, Irqs);
    let t = sensor.read().await.to_num::<i16>();
    CACHED_TEMP_C10.store(t * 10, Ordering::Relaxed);
    // Mark fresh so the mesh-build `refresh_if_stale` doesn't immediately
    // re-read via MPSL (not necessarily up yet) on the first loop iteration.
    LAST_REFRESH_S.store(Instant::now().as_secs() as u32, Ordering::Relaxed);
    t
}

/// Return the last cached die temperature as °C × 10 (e.g. 255 = 25.5 °C),
/// or `i16::MIN` if [`read_and_cache`] has not been called yet.
pub fn last_c10() -> i16 {
    CACHED_TEMP_C10.load(Ordering::Relaxed)
}

/// Set true once MPSL/SDC has been initialised (see [`mark_mpsl_ready`]).
/// Until then `mpsl_temperature_get()` must NOT be called — on a
/// HFXO-degraded boot `init_ble` is skipped entirely, so MPSL stays uninit
/// and the FFI call would be UB.  `read_and_cache` still primes the cache at
/// boot, so a badge that never brings up MPSL just keeps that value.
#[cfg(feature = "mesh")]
static MPSL_READY: AtomicBool = AtomicBool::new(false);

/// Mark MPSL as initialised.  Call once from `init_ble`, after MPSL is up.
#[cfg(feature = "mesh")]
pub fn mark_mpsl_ready() {
    MPSL_READY.store(true, Ordering::Relaxed);
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
    LAST_REFRESH_S.store(Instant::now().as_secs() as u32, Ordering::Relaxed);
    c10
}

/// Monotonic-seconds timestamp of the most recent MPSL-driven refresh of
/// [`CACHED_TEMP_C10`].  `0` until [`read_via_mpsl`] or
/// [`refresh_if_stale`] has run successfully.  Used by the lazy-refresh
/// helper below to keep the cache reasonably fresh without a dedicated
/// timer task.  Uses `u32` seconds — Cortex-M4 has no native 64-bit
/// atomics; wraps after ~136 years.
static LAST_REFRESH_S: AtomicU32 = AtomicU32::new(0);

/// Max age (seconds) the cached temperature may have before
/// [`refresh_if_stale`] re-reads via MPSL.  Five minutes — the badge's
/// MCU die warms up slowly under steady load, and the SSD1675's 4 °C
/// TR-band granularity doesn't benefit from sub-minute polling.
const STALE_AFTER_S: u32 = 5 * 60;


/// Refresh the cached temperature via MPSL if the last refresh was more
/// than [`STALE_AFTER_S`] ago.  Cheap when not stale (one atomic load +
/// one subtraction).  No-op on non-mesh builds (MPSL unavailable —
/// caller stuck with whatever [`read_and_cache`] put in the cache at
/// boot).
///
/// Intended call site: the display refresh loop, immediately before
/// feeding the SSD1675 temperature register.  Couples the temp refresh
/// rate to the screen-redraw rate without adding a timer task.
pub fn refresh_if_stale() {
    #[cfg(feature = "mesh")]
    {
        // Never touch MPSL before it's up.  On a HFXO-degraded boot init_ble
        // is skipped, MPSL is never initialised, and mpsl_temperature_get()
        // would be UB (MPSL assert -> panic -> WDT reset loop, or garbage LUT
        // temperature).  Stay on the boot-cached value instead.
        if !MPSL_READY.load(Ordering::Relaxed) {
            return;
        }
        let now = Instant::now().as_secs() as u32;
        let last = LAST_REFRESH_S.load(Ordering::Relaxed);
        if last == 0 || now.saturating_sub(last) >= STALE_AFTER_S {
            let _ = read_via_mpsl();
        }
    }
}
