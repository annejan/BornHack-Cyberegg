//! Display animation state — determines what the pet area should show.
//!
//! [`DisplayAnim`] is a simple enum that the display layer matches on to
//! pick the right animation.  [`GameState::display_anim()`] is a cheap
//! read-only lookup — no update, no stats computation.  Call it every
//! display refresh without worry.
//!
//! Priority (highest wins):
//!   1. Gone (pet has left)
//!   2. Hibernating
//!   3. Hatching (egg animation)
//!   4. Active action (feeding, healing, relaxing, playing, sleeping)
//!   5. Leaving (countdown warning, with urgency level)
//!   6. Critical stat (urgent distress — needs immediate action)
//!   7. Warning stat (attention needed soon — not yet critical)
//!   8. Idle / Happy (pet is content)

use super::thresholds::*;
use super::{Action, GameState, Phase};

/// What the pet display area should show right now.
///
/// The display layer matches on this and selects the appropriate
/// animation / sprite sequence.  Variants are ordered by priority —
/// only the highest-priority active state is returned.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "embassy-base", derive(defmt::Format))]
pub enum DisplayAnim {
    // ── Group 1: terminal / blocking ────────────────────────────────
    /// Pet has left permanently.
    Gone,
    /// All progression frozen.
    Hibernating,
    /// Egg is hatching.  `ticks_remaining` counts down to zero.
    Hatching {
        ticks_remaining: u16,
    },

    // ── Group 2: active actions (mutually exclusive) ────────────────
    Feeding,
    Healing,
    Relaxing,
    Playing,
    Sleeping,
    Exercising,
    Medicating,
    /// Drinking a beverage (Beer / Wine / Whiskey / Water / Cola).
    Drinking,
    /// Receiving an Ozempic injection.
    Ozempic,
    /// Rehab / sobering-up treatment.
    Rehab,

    // ── Group 3: leaving danger ─────────────────────────────────────
    /// Pet is about to leave.  `maxed_count` (1–4) indicates urgency.
    Leaving {
        maxed_count: u8,
    },

    // ── Group 4: critical stats (needs immediate action) ────────────
    CriticalSick,
    CriticalTired,
    CriticalHungry,
    CriticalDrained,

    // ── Group 5: warning stats (attention needed soon) ──────────────
    WarningSick,
    WarningTired,
    WarningHungry,
    WarningDrained,
    WarningMiserable,

    // ── Group 6: content ────────────────────────────────────────────
    /// Pet is happy (all stats well below warning thresholds).
    Happy,
    /// Default resting state (no warnings, no special happiness).
    Idle,
}

/// Count of maxed stats (= STAT_MAX()).
fn count_maxed(state: &GameState) -> u8 {
    (state.hunger == STAT_MAX()) as u8
        + (state.tired == STAT_MAX()) as u8
        + (state.drained == STAT_MAX()) as u8
        + (state.sick == STAT_MAX()) as u8
}

impl GameState {
    /// Determine which animation the display area should show.
    ///
    /// This is a **cheap read-only lookup** — no update is triggered.
    /// Call it from the display loop on every refresh.
    pub fn display_anim(&self) -> DisplayAnim {
        // ── Group 1: terminal / blocking ────────────────────────────
        if self.phase == Phase::Gone {
            return DisplayAnim::Gone;
        }
        if self.hibernating {
            return DisplayAnim::Hibernating;
        }
        if self.phase == Phase::Hatching {
            return DisplayAnim::Hatching {
                ticks_remaining: self.hatching_countdown,
            };
        }

        // ── Group 2: active action ──────────────────────────────────
        if let Some(action) = self.active_action {
            return match action {
                Action::Feed => DisplayAnim::Feeding,
                Action::Heal => DisplayAnim::Healing,
                Action::Relax => DisplayAnim::Relaxing,
                Action::Play => DisplayAnim::Playing,
                Action::Exercise => DisplayAnim::Exercising,
                Action::Medicate => DisplayAnim::Medicating,
                Action::Ozempic => DisplayAnim::Ozempic,
                Action::Rehab => DisplayAnim::Rehab,
                Action::Drink => DisplayAnim::Drinking,
            };
        }
        if self.is_sleeping {
            return DisplayAnim::Sleeping;
        }

        // ── Group 3: leaving danger ─────────────────────────────────
        if self.phase == Phase::Leaving {
            return DisplayAnim::Leaving {
                maxed_count: count_maxed(self),
            };
        }

        // Note: diabetic-and-unmedicated is deliberately *not* a
        // DisplayAnim state — there's no sprite art for it (or for the
        // weight critical/warning tiers below), and swapping the pet's
        // whole display out for a debug-text-only screen made the pet
        // disappear entirely whenever the condition was active. It's
        // surfaced instead as a persistent on-screen banner drawn
        // alongside whatever the pet's normal animation is — see
        // `lifecycle::is_diabetic_unmedicated()` / `game::mod` render.

        // ── Group 4: critical stats ─────────────────────────────────
        // Ranked by recovery difficulty: sick > tired > hungry > drained.
        if self.sick > SICK_TRIGGER_TIRED() {
            return DisplayAnim::CriticalSick;
        }
        if self.tired > SICK_TRIGGER_TIRED() {
            return DisplayAnim::CriticalTired;
        }
        if self.hunger > SICK_TRIGGER_HUNGER() {
            return DisplayAnim::CriticalHungry;
        }
        if self.drained > SICK_TRIGGER_DRAINED() {
            return DisplayAnim::CriticalDrained;
        }

        // ── Group 5: warning stats ──────────────────────────────────
        // Same priority ranking, lower thresholds.
        if self.sick > WARNING_SICK() {
            return DisplayAnim::WarningSick;
        }
        if self.tired > WARNING_TIRED() {
            return DisplayAnim::WarningTired;
        }
        if self.hunger > WARNING_HUNGER() {
            return DisplayAnim::WarningHungry;
        }
        if self.drained > WARNING_DRAINED() {
            return DisplayAnim::WarningDrained;
        }
        if self.miserable > WARNING_MISERABLE() {
            return DisplayAnim::WarningMiserable;
        }

        // ── Group 6: content ────────────────────────────────────────
        // Happy when all stats are well below warning thresholds.
        if self.hunger < WARNING_HUNGER() / 2
            && self.tired < WARNING_TIRED() / 2
            && self.drained < WARNING_DRAINED() / 2
            && self.sick < WARNING_SICK() / 2
            && self.miserable < WARNING_MISERABLE() / 2
        {
            return DisplayAnim::Happy;
        }

        DisplayAnim::Idle
    }
}
