//! Buck/boost converter power-mode arbiter.
//!
//! The supply's `PS_SYNC` pin (P0_17) selects the converter mode:
//!
//! - **LOW**  → boost mode, high power available
//! - **HIGH** → power-save mode, low quiescent draw
//!
//! The badge runs in power-save by default and only needs boost while a
//! high-current event is active. Several such events can overlap, so this
//! module arbitrates them with a vote bitmask: the pin is driven LOW (boost)
//! whenever **any** source has an outstanding vote, and HIGH (power-save) once
//! all votes clear.
//!
//! Sources take an RAII [`BoostGuard`] via [`boost`]; the vote is released when
//! the guard drops, so a source can never strand the badge in boost.

use core::cell::RefCell;
use core::sync::atomic::{AtomicU8, Ordering};

use embassy_nrf::gpio::{Level, Output};
use embassy_sync::blocking_mutex::{Mutex, raw::CriticalSectionRawMutex};

/// A high-current source that can request boost mode. Each is one bit in the
/// vote mask, so requests OR together and release independently.
#[derive(Clone, Copy)]
pub enum Source {
    /// Held for the whole boot-init sequence.
    Boot = 0b001,
    /// Held across an EPD screen refresh.
    Epd = 0b010,
    /// Held across a LoRa transmission.
    Lora = 0b100,
}

/// Outstanding boost votes (bitwise-OR of active [`Source`]s).
static VOTES: AtomicU8 = AtomicU8::new(0);

/// The `PS_SYNC` output, installed once by [`init`].
static PS_SYNC: Mutex<CriticalSectionRawMutex, RefCell<Option<Output<'static>>>> =
    Mutex::new(RefCell::new(None));

/// Pure vote → boost decision: boost whenever any source votes.
const fn boost_needed(votes: u8) -> bool {
    votes != 0
}

/// Register the `PS_SYNC` output and drive it to match the current votes.
///
/// Call once at boot. The pin should be constructed in the desired boot state
/// (LOW = boost); pair this with a held [`Source::Boot`] guard so it stays in
/// boost until init completes.
pub fn init(pin: Output<'static>) {
    PS_SYNC.lock(|c| {
        c.replace(Some(pin));
    });
    apply();
}

/// Drive the pin to reflect the current vote mask. No-op until [`init`] has
/// installed the pin. GPIO writes are fast and non-async — safe inside the
/// critical-section mutex.
fn apply() {
    let boost = boost_needed(VOTES.load(Ordering::Relaxed));
    PS_SYNC.lock(|c| {
        if let Some(pin) = c.borrow_mut().as_mut() {
            pin.set_level(if boost { Level::Low } else { Level::High });
        }
    });
}

/// Request boost mode for `src` — returns a guard that releases the vote on
/// drop. Hold it across the high-current work:
///
/// ```ignore
/// let _boost = power::boost(Source::Epd);
/// display.update_tc(speed).await;   // runs in boost
/// // guard drops → power-save unless another source still votes
/// ```
#[must_use = "boost is released when the guard is dropped; bind it to a variable"]
pub fn boost(src: Source) -> BoostGuard {
    VOTES.fetch_or(src as u8, Ordering::Relaxed);
    apply();
    BoostGuard(src)
}

/// Run `fut` to completion in boost mode for `src`, releasing the vote when it
/// finishes. Convenience wrapper for guarding a single async operation (e.g. an
/// EPD refresh): the boost vote is held for exactly the future's `.await`.
///
/// ```ignore
/// let _ = power::boosted(Source::Epd, display.update_tc(speed)).await;
/// ```
pub async fn boosted<F: core::future::Future>(src: Source, fut: F) -> F::Output {
    let _guard = boost(src);
    fut.await
}

/// RAII boost vote. Boost is held while this is alive and released on drop.
pub struct BoostGuard(Source);

impl Drop for BoostGuard {
    fn drop(&mut self) {
        VOTES.fetch_and(!(self.0 as u8), Ordering::Relaxed);
        apply();
    }
}
