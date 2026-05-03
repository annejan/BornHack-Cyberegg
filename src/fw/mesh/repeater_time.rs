//! On-air wall-clock seed and refinement.
//!
//! Listens to signed/MAC-verified mesh traffic for Unix timestamps
//! and uses them to seed (and continually refine) the badge wall
//! clock until either the BLE companion app takes over or the
//! firmware reboots.
//!
//! # Strategy
//!
//! - **Year >= 2026.**  Anything before `2026-01-01 00:00 UTC` is
//!   discarded as a stale or unset clock.
//! - **Hop compensation.**  Each sample is corrected by adding
//!   `SECONDS_PER_HOP * hops` seconds to its timestamp before
//!   anything else, on the assumption that timestamps written at the
//!   originator have been ageing in transit.
//! - **First valid sample seeds the clock.**  No averaging yet —
//!   the first plausible timestamp we hear becomes the wall clock.
//! - **Subsequent samples drive a rolling median.**  Each later
//!   sample is converted to a *signed* delta from the running clock.
//!   Any sample whose corrected delta is outside ±1 h is rejected
//!   outright.  The remaining deltas are buffered; once five are
//!   collected, the median is added to the wall clock and the
//!   buffer resets to start a new batch.
//! - **BLE companion is authoritative.**  Once `SET_DEVICE_TIME`
//!   has run, `BLE_TIME_LOCKED` latches and on-air refinement
//!   stops permanently for the boot.
//!
//! Caller is responsible for filtering on the source's trust
//! criteria (signature verified, MAC passed, role appropriate) —
//! this module does no further authentication.

use core::cell::RefCell;
use core::sync::atomic::Ordering;
use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;

/// Per-hop propagation compensation, in seconds.  Added to every
/// incoming timestamp as `corrected = ts + hops * SECONDS_PER_HOP`
/// to account for the time the timestamp spent in flight from the
/// originator.  Tune by editing this value; nothing else depends on
/// it.
pub const SECONDS_PER_HOP: u32 = 4;

/// Unix seconds at 2026-01-01 00:00:00 UTC.  Timestamps strictly
/// less than this are rejected as stale / unset clocks.
const UNIX_2026: u32 = 1_767_225_600;

/// Maximum allowed corrected delta from the running clock, in
/// seconds (signed).  Samples outside ±MAX_DELTA_SECS are rejected
/// as noise before they ever enter the median buffer.
const MAX_DELTA_SECS: i32 = 3600;

/// Number of signed-delta samples required before the rolling
/// median is computed and applied.
const SAMPLE_COUNT: usize = 5;

struct DeltaAcc {
    deltas: [i32; SAMPLE_COUNT],
    count: u8,
}

impl DeltaAcc {
    const fn new() -> Self {
        Self {
            deltas: [0; SAMPLE_COUNT],
            count: 0,
        }
    }

    fn reset(&mut self) {
        self.count = 0;
    }

    /// Push a signed delta.  Returns `Some(median)` when the buffer
    /// just filled; the buffer is reset to empty in that case so the
    /// next call begins a fresh batch.  Returns `None` while the
    /// buffer is still filling.
    fn push(&mut self, d: i32) -> Option<i32> {
        if (self.count as usize) < SAMPLE_COUNT {
            self.deltas[self.count as usize] = d;
            self.count += 1;
        }
        if (self.count as usize) < SAMPLE_COUNT {
            return None;
        }
        let mut sorted = self.deltas;
        sorted.sort_unstable();
        let median = sorted[SAMPLE_COUNT / 2];
        self.count = 0;
        Some(median)
    }
}

static ACC: Mutex<CriticalSectionRawMutex, RefCell<DeltaAcc>> =
    Mutex::new(RefCell::new(DeltaAcc::new()));

/// Feed a wall-clock timestamp from any trusted on-air source
/// (signed advert, MAC-verified channel message, ...) into the
/// seeder.  `hops` is the path length the packet traversed —
/// 0 = direct neighbour, larger = further away.
pub fn observe_timestamp(timestamp: u32, hops: u8) {
    if timestamp < UNIX_2026 {
        return;
    }
    if crate::BLE_TIME_LOCKED.load(Ordering::Relaxed) {
        return;
    }

    let corrected = timestamp.saturating_add(hops as u32 * SECONDS_PER_HOP);

    match crate::unix_now() {
        None => {
            // First-ever sample — seed the clock.
            defmt::info!(
                "wall-clock seed: corrected={=u32} (raw={=u32} hops={=u8})",
                corrected,
                timestamp,
                hops,
            );
            crate::set_wall_clock(corrected);
            // No median possible yet; clear any stale state from
            // earlier failed attempts so the next batch starts fresh.
            ACC.lock(|c| c.borrow_mut().reset());
        }
        Some(now) => {
            // Compute signed delta in i64 to avoid u32 wrap, then
            // clamp/reject before narrowing to i32 for the median
            // buffer.
            let delta_i64 = corrected as i64 - now as i64;
            if delta_i64.abs() > MAX_DELTA_SECS as i64 {
                return;
            }
            let delta = delta_i64 as i32;

            let median = ACC.lock(|c| c.borrow_mut().push(delta));
            if let Some(m) = median {
                // Apply the median offset.  Cast through i64 so
                // negative medians (clock running ahead of the mesh)
                // subtract correctly without u32 wrap.
                let new_clock = (now as i64 + m as i64).max(0) as u32;
                defmt::info!(
                    "wall-clock adjust: median_delta={=i32}s now={=u32} -> new={=u32}",
                    m,
                    now,
                    new_clock,
                );
                crate::set_wall_clock(new_clock);
            }
        }
    }
}
