//! A "takeover is showing" flag plus the uptime it started at.
//!
//! Toasts, the diabetes/alcoholism alerts and the battle animation all follow
//! the same pattern: set a flag when shown, remember the start time, and let
//! the renderer clear the flag at the first draw at or after a minimum
//! visibility. State is relaxed atomics read from synchronous draw code.

use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

pub struct TimedFlag {
    active: AtomicBool,
    started_ms: AtomicU32,
}

impl TimedFlag {
    pub const fn new() -> Self {
        Self {
            active: AtomicBool::new(false),
            started_ms: AtomicU32::new(0),
        }
    }

    /// Mark active, starting the clock now. The start time is stored before
    /// the flag so a reader that sees `active` never sees a stale start.
    pub fn start(&self) {
        self.started_ms
            .store(crate::platform::uptime_ms_u32(), Ordering::Relaxed);
        self.active.store(true, Ordering::Relaxed);
    }

    pub fn is_active(&self) -> bool {
        self.active.load(Ordering::Relaxed)
    }

    /// Milliseconds since [`start`](Self::start). Wrapping subtraction keeps
    /// this right across the ~49-day uptime wraparound.
    pub fn elapsed_ms(&self) -> u32 {
        crate::platform::uptime_ms_u32().wrapping_sub(self.started_ms.load(Ordering::Relaxed))
    }

    /// Clear the flag if at least `min_ms` have elapsed.
    pub fn clear_if_elapsed(&self, min_ms: u32) {
        if self.elapsed_ms() >= min_ms {
            self.clear();
        }
    }

    pub fn clear(&self) {
        self.active.store(false, Ordering::Relaxed);
    }

    /// The tick of the first draw that should clear this flag: after `min_ms`
    /// since [`start`](Self::start), and never at or before `now` (a deadline
    /// in the past would spin the display loop). Pacing only: the clear itself
    /// stays in [`clear_if_elapsed`](Self::clear_if_elapsed).
    pub fn clear_tick(&self, min_ms: u32, now: u32) -> u32 {
        const MS_PER_TICK: u32 = 10_000;
        let remaining = min_ms.saturating_sub(self.elapsed_ms());
        now.saturating_add(remaining.div_ceil(MS_PER_TICK).max(1))
    }

    /// Pretend the flag started `ms` earlier, so tests need no real wait.
    #[cfg(test)]
    pub(crate) fn backdate_for_test(&self, ms: u32) {
        let at = self.started_ms.load(Ordering::Relaxed);
        self.started_ms.store(at.wrapping_sub(ms), Ordering::Relaxed);
    }
}
