//! Display pacing: how often a sprite animation advances a frame, and how
//! long the display loop sleeps before the next redraw.
//!
//! This is display pacing only. The engine still advances solely through
//! `update()` at its own wake ticks (the delta engine); nothing here steps or
//! polls it. An idle pet shows one frame per [`IDLE_FRAME_SECS`] to spare the
//! battery and the e-paper panel; action animations still play at full speed.

use super::engine::DisplayAnim;

/// Seconds in one engine tick.
pub const SECS_PER_TICK: u64 = 10;
/// One idle frame every 5 minutes (spec 17 row 29).
pub const IDLE_FRAME_SECS: u64 = 300;
/// The idle frame in engine ticks, derived so the two can never drift apart.
pub const IDLE_FRAME_TICKS: u32 = (IDLE_FRAME_SECS / SECS_PER_TICK) as u32;
/// Shortest time between two frames of a fast animation.
pub const MIN_FRAME_SECS: u64 = 2;

/// The action animations are "fast" (hatching is fast too, see
/// [`frame_interval_secs`]); everything else (Idle,
/// Happy, Warning*, Critical*, Sleeping, Leaving, Gone, Hibernating) is idle.
pub fn is_action_anim(anim: DisplayAnim) -> bool {
    matches!(
        anim,
        DisplayAnim::Feeding
            | DisplayAnim::Healing
            | DisplayAnim::Playing
            | DisplayAnim::Exercising
            | DisplayAnim::Medicating
            | DisplayAnim::Drinking
            | DisplayAnim::Ozempic
            | DisplayAnim::Rehab
            | DisplayAnim::OnlyPets
            | DisplayAnim::Relaxing
            | DisplayAnim::Washing
    )
}

/// Seconds per frame, `None` for a single-frame animation. Hatching spreads
/// its remaining ticks over the frames, action animations spread
/// `action_ticks_remaining`; both are floored at [`MIN_FRAME_SECS`]. Every
/// other animation is idle: [`IDLE_FRAME_SECS`].
pub fn frame_interval_secs(
    anim: DisplayAnim,
    frames: u32,
    action_ticks_remaining: u8,
) -> Option<u64> {
    if frames <= 1 {
        return None;
    }
    let frames = frames as u64;
    let spread = |ticks: u64| (ticks * SECS_PER_TICK / frames).max(MIN_FRAME_SECS);
    match anim {
        DisplayAnim::Hatching { ticks_remaining } => Some(spread(ticks_remaining as u64)),
        a if is_action_anim(a) => Some(spread(action_ticks_remaining as u64)),
        _ => Some(IDLE_FRAME_SECS),
    }
}

/// How long to wait, and whether the wait ends on a frame (`true`) or on an
/// engine/UI wake (`false`: redraw without advancing the frame).
pub fn sprite_wait(frame_secs: Option<u64>, wake_secs: u64) -> (u64, bool) {
    match frame_secs {
        Some(f) if f <= wake_secs => (f, true),
        _ => (wake_secs, false),
    }
}

/// The absolute deadline of the current animation frame. It lives in the
/// display loop state, so a loop re-entry (minute tick, mesh signal), an
/// engine wake or a toast redraw cannot push the frame back: only a frame
/// advance ([`FramePacer::on_frame`]) or an animation change restarts it.
pub struct FramePacer {
    anim_id: u8,
    /// Uptime second the current frame is due, once armed.
    due_at: Option<u64>,
}

impl Default for FramePacer {
    fn default() -> Self {
        Self::new()
    }
}

impl FramePacer {
    pub const fn new() -> Self {
        Self {
            anim_id: 0xFF,
            due_at: None,
        }
    }

    /// How long to wait from `now_secs` (uptime), and whether the wait ends
    /// on a frame (`true`) or on a wake (`false`, redraw without advancing).
    /// Arms the deadline on first use and keeps it until `on_frame`.
    pub fn wait(
        &mut self,
        now_secs: u64,
        anim_id: u8,
        frame_secs: Option<u64>,
        wake_secs: u64,
    ) -> (u64, bool) {
        if anim_id != self.anim_id {
            self.anim_id = anim_id;
            self.due_at = None;
        }
        let Some(interval) = frame_secs else {
            self.due_at = None;
            return sprite_wait(None, wake_secs);
        };
        let due = *self.due_at.get_or_insert(now_secs.saturating_add(interval));
        sprite_wait(Some(due.saturating_sub(now_secs)), wake_secs)
    }

    /// The frame advanced: the next one is due a full interval later.
    pub fn on_frame(&mut self) {
        self.due_at = None;
    }
}

/// Seconds until the next wake the display must honour: the engine's
/// `next_wake_tick` or the UI deadline, whichever is first, at least one tick
/// away. Saturating, because a hibernating pet's wake is `u32::MAX`.
pub fn wake_secs(next_wake_tick: u32, ui_deadline_tick: u32, now_tick: u32) -> u64 {
    let ticks = next_wake_tick
        .min(ui_deadline_tick)
        .saturating_sub(now_tick)
        .max(1);
    ticks as u64 * SECS_PER_TICK
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_frame_is_five_minutes_actions_stay_fast() {
        assert_eq!(frame_interval_secs(DisplayAnim::Idle, 3, 0), Some(300));
        assert_eq!(frame_interval_secs(DisplayAnim::Sleeping, 4, 0), Some(300));
        assert_eq!(
            frame_interval_secs(DisplayAnim::WarningHungry, 2, 0),
            Some(300)
        );
        assert_eq!(frame_interval_secs(DisplayAnim::Exercising, 3, 6), Some(20));
        assert_eq!(frame_interval_secs(DisplayAnim::Feeding, 5, 1), Some(2));
        assert_eq!(
            frame_interval_secs(
                DisplayAnim::Hatching {
                    ticks_remaining: 12
                },
                4,
                0
            ),
            Some(30)
        );
        assert_eq!(frame_interval_secs(DisplayAnim::Idle, 1, 0), None);
    }

    #[test]
    fn sprite_wait_ends_at_the_engine_wake() {
        assert_eq!(sprite_wait(Some(300), 120), (120, false));
        assert_eq!(sprite_wait(Some(300), 300), (300, true));
        assert_eq!(sprite_wait(Some(300), 7200), (300, true));
        assert_eq!(sprite_wait(None, 50), (50, false));
    }

    #[test]
    fn wake_secs_never_underflows_or_overflows() {
        assert_eq!(wake_secs(105, u32::MAX, 100), 50);
        assert_eq!(wake_secs(90, u32::MAX, 100), 10, "a past wake is one tick");
        assert_eq!(wake_secs(u32::MAX, u32::MAX, 0), u32::MAX as u64 * 10);
        assert_eq!(
            wake_secs(500, 120, 100),
            200,
            "the UI deadline can come first"
        );
    }

    #[test]
    fn idle_frame_constants_agree() {
        assert_eq!(IDLE_FRAME_TICKS as u64 * SECS_PER_TICK, IDLE_FRAME_SECS);
    }

    #[test]
    fn only_actions_and_hatching_are_fast() {
        use DisplayAnim::*;
        let fast = [
            Feeding, Healing, Playing, Exercising, Medicating, Drinking, Ozempic, Rehab, OnlyPets,
            Relaxing, Washing,
        ];
        let all = [
            Gone,
            Hibernating,
            Hatching {
                ticks_remaining: 12,
            },
            Feeding,
            Healing,
            Playing,
            Sleeping,
            Exercising,
            Medicating,
            Drinking,
            Ozempic,
            Rehab,
            OnlyPets,
            Relaxing,
            Washing,
            Tipsy,
            Wasted,
            ThrowingUp,
            HungOver,
            Sore,
            Injured,
            Groggy,
            Stuffed,
            Thirsty,
            Wired,
            NeedsRehab,
            Starving,
            Leaving { maxed_count: 1 },
            CriticalSick,
            CriticalTired,
            CriticalHungry,
            WarningSick,
            WarningTired,
            WarningHungry,
            WarningMiserable,
            Happy,
            Idle,
        ];
        for a in all {
            assert_eq!(is_action_anim(a), fast.contains(&a), "{a:?}");
            let want = if is_action_anim(a) || matches!(a, Hatching { .. }) {
                30
            } else {
                300
            };
            assert_eq!(frame_interval_secs(a, 4, 12), Some(want), "{a:?}");
        }
    }

    #[test]
    fn an_action_end_before_the_frame_ends_the_wait() {
        assert_eq!(sprite_wait(Some(20), 10), (10, false));
        let mut p = FramePacer::new();
        assert_eq!(p.wait(0, 1, Some(20), 10), (10, false));
    }

    #[test]
    fn reentry_and_wakes_do_not_push_the_frame_back() {
        let mut p = FramePacer::new();
        assert_eq!(p.wait(1000, 1, Some(300), 7200), (300, true));
        // a minute tick re-enters the loop 60 s later; an engine wake at 120 s
        assert_eq!(p.wait(1060, 1, Some(300), 7200), (240, true));
        assert_eq!(p.wait(1120, 1, Some(300), 60), (60, false));
        assert_eq!(p.wait(1180, 1, Some(300), 7200), (120, true));
    }

    #[test]
    fn a_frame_advance_restarts_the_deadline() {
        let mut p = FramePacer::new();
        assert_eq!(p.wait(0, 1, Some(300), 7200), (300, true));
        p.on_frame();
        assert_eq!(p.wait(300, 1, Some(300), 7200), (300, true));
    }

    #[test]
    fn a_cleared_deadline_waits_a_full_interval() {
        let mut p = FramePacer::new();
        assert_eq!(p.wait(0, 1, Some(300), 7200), (300, true));
        // away from the pet screen for 10 minutes, then cleared on return
        p.on_frame();
        assert_eq!(p.wait(600, 1, Some(300), 7200), (300, true));
    }

    #[test]
    fn an_anim_change_restarts_the_deadline() {
        let mut p = FramePacer::new();
        assert_eq!(p.wait(0, 1, Some(300), 7200), (300, true));
        assert_eq!(p.wait(100, 2, Some(300), 7200), (300, true));
        assert_eq!(
            p.wait(150, 2, None, 50),
            (50, false),
            "single frame: no deadline"
        );
    }

    #[test]
    fn a_day_of_minute_ticks_is_288_idle_frames() {
        let mut p = FramePacer::new();
        let (mut now, mut frames) = (0u64, 0u32);
        while now < 86_400 {
            // every pass the minute tick (60 s) can cut the wait short
            let (w, due) = p.wait(now, 1, Some(IDLE_FRAME_SECS), 7200);
            let step = w.min(60);
            now += step;
            if due && step == w {
                p.on_frame();
                frames += 1;
            }
        }
        assert_eq!(frames, 288);
    }
}
