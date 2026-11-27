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
//!   6. Status word in danger (urgent distress — needs immediate action)
//!   7. Status word (attention needed soon — not yet in danger)
//!   8. Idle / Happy (content; happy from 80 % mood)

use super::needs::{self, HAPPY_MOOD_PCT, P, TIPSY_PCT};
use super::status::{Condition, StatusWord, conditions, status};
use super::{Action, GameState, Phase};

/// What the pet display area should show right now.
///
/// The display layer matches on this and selects the appropriate
/// animation / sprite sequence.  Variants are ordered by priority —
/// only the highest-priority active state is returned.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "embassy-core", derive(defmt::Format))]
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
    Playing,
    Sleeping,
    Exercising,
    Medicating,
    /// Drinking a beverage (Beer / Wine / Whiskey / Water / Coffee).
    Drinking,
    /// Receiving an Ozempic injection.
    Ozempic,
    /// Rehab / sobering-up treatment.
    Rehab,
    /// "Only pets" work/hobby action.
    OnlyPets,
    /// Relaxing (the Relax action). Art row 0x0E.
    Relaxing,
    /// Washing (the Wash action). Art row 0x35.
    Washing,

    // ── Group 3: leaving danger ─────────────────────────────────────
    /// Pet is about to leave.  `maxed_count` (1–4) indicates urgency.
    Leaving {
        maxed_count: u8,
    },

    // ── Group 4: critical stats (needs immediate action) ────────────
    CriticalSick,
    CriticalTired,
    CriticalHungry,

    // ── Group 5: warning stats (attention needed soon) ──────────────
    WarningSick,
    WarningTired,
    WarningHungry,
    WarningMiserable,

    // ── v3 states (art rows 0x29-0x34; see `v3_anim`) ───────────────
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

    // ── Group 6: content ────────────────────────────────────────────
    /// Pet is happy (all stats well below warning thresholds).
    Happy,
    /// Default resting state (no warnings, no special happiness).
    Idle,
}

/// Count of needs at level 0.
fn count_maxed(state: &GameState) -> u8 {
    (state.fullness == 0) as u8 + (state.energy == 0) as u8 + (state.health == 0) as u8
}

impl DisplayAnim {
    /// Whether the anim comes from the status words (groups 4 to 6), the
    /// only ones the v3 states may replace.
    fn is_status_driven(self) -> bool {
        matches!(
            self,
            DisplayAnim::CriticalSick
                | DisplayAnim::CriticalTired
                | DisplayAnim::CriticalHungry
                | DisplayAnim::WarningSick
                | DisplayAnim::WarningTired
                | DisplayAnim::WarningHungry
                | DisplayAnim::WarningMiserable
                | DisplayAnim::Happy
                | DisplayAnim::Idle
        )
    }
}

impl GameState {
    /// The anim the pet should show when art exists for every state: the v3
    /// states, else what [`Self::legacy_anim`] says. Cheap, read-only.
    pub fn display_anim(&self) -> DisplayAnim {
        self.display_anim_with(|_| true)
    }

    /// [`Self::display_anim`], but a v3 anim is used only when `has_art`
    /// says its sprite row exists for this pet; otherwise the anim the pet
    /// showed before the v3 art (`legacy_anim`). The one place the fallback
    /// is decided.
    pub fn display_anim_with(&self, has_art: impl Fn(DisplayAnim) -> bool) -> DisplayAnim {
        let legacy = self.legacy_anim();
        let wanted = self.v3_anim(legacy);
        if wanted != legacy && has_art(wanted) {
            wanted
        } else {
            legacy
        }
    }

    /// The v3 refinement of `legacy`: Relax and Wash, the drunk levels, the
    /// status words with their own art and the conditions that take over
    /// Idle / Happy. Anything else stays `legacy`.
    fn v3_anim(&self, legacy: DisplayAnim) -> DisplayAnim {
        match self.active_action {
            Some(Action::Relax) => return DisplayAnim::Relaxing,
            Some(Action::Wash) => return DisplayAnim::Washing,
            _ => {}
        }
        if !legacy.is_status_driven() {
            return legacy;
        }
        let booze = self.booze as i32;
        if booze > needs::booze_danger() {
            return DisplayAnim::ThrowingUp;
        }
        let (word, danger) = status(self);
        if booze > needs::booze_hi() && !danger {
            return DisplayAnim::Wasted;
        }
        match word {
            StatusWord::Hungry if danger => DisplayAnim::Starving,
            StatusWord::Thirsty => DisplayAnim::Thirsty,
            StatusWord::HungOver => DisplayAnim::HungOver,
            StatusWord::Groggy => DisplayAnim::Groggy,
            StatusWord::Injured => DisplayAnim::Injured,
            StatusWord::Wired => DisplayAnim::Wired,
            StatusWord::Stuffed => DisplayAnim::Stuffed,
            StatusWord::Content if matches!(legacy, DisplayAnim::Idle | DisplayAnim::Happy) => {
                let conds = conditions(self);
                if conds.has(Condition::Dependent) {
                    DisplayAnim::NeedsRehab
                } else if conds.has(Condition::Sore) {
                    DisplayAnim::Sore
                } else if booze >= TIPSY_PCT as i32 * P {
                    DisplayAnim::Tipsy
                } else {
                    legacy
                }
            }
            _ => legacy,
        }
    }

    /// The anim before the v3 art: Wash and Relax show `Playing`, and the
    /// status words fold into the older sprites.
    ///
    /// This is a **cheap read-only lookup** — no update is triggered.
    /// Call it from the display loop on every refresh.
    fn legacy_anim(&self) -> DisplayAnim {
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
                Action::Play => DisplayAnim::Playing,
                Action::Exercise => DisplayAnim::Exercising,
                Action::Medicate => DisplayAnim::Medicating,
                Action::Ozempic => DisplayAnim::Ozempic,
                Action::Rehab => DisplayAnim::Rehab,
                Action::Drink => DisplayAnim::Drinking,
                Action::OnlyPets => DisplayAnim::OnlyPets,
                // Wash and Relax have no sprites of their own; they show the play animation.
                Action::Wash | Action::Relax => DisplayAnim::Playing,
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
        // surfaced instead as the "no insulin" prompt word on the status
        // line, drawn under whatever the pet's normal animation is — see
        // `status::status_line` and `game::draw_status_line`.

        // ── Groups 4 to 6: the status word ──────────────────────────
        let (word, danger) = status(self);
        let pick = |warning, critical| if danger { critical } else { warning };
        match word {
            StatusWord::Hungry | StatusWord::Thirsty => {
                pick(DisplayAnim::WarningHungry, DisplayAnim::CriticalHungry)
            }
            StatusWord::Tired | StatusWord::Groggy | StatusWord::HungOver => {
                pick(DisplayAnim::WarningTired, DisplayAnim::CriticalTired)
            }
            StatusWord::Poorly | StatusWord::Injured => {
                pick(DisplayAnim::WarningSick, DisplayAnim::CriticalSick)
            }
            StatusWord::Bored
            | StatusWord::Wired
            | StatusWord::Sloppy
            | StatusWord::Grubby
            | StatusWord::Stuffed
            | StatusWord::Waterlogged
            | StatusWord::Overweight
            | StatusWord::Underweight
            | StatusWord::Unfit => DisplayAnim::WarningMiserable,
            StatusWord::Content if self.mood as i32 >= HAPPY_MOOD_PCT as i32 * P => {
                DisplayAnim::Happy
            }
            StatusWord::Content => DisplayAnim::Idle,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::engine::PetKind;
    use crate::game::engine::status::StatusWord;

    fn pct(p: i32) -> u16 {
        (p * P) as u16
    }

    fn pet() -> GameState {
        let mut s = GameState::new_egg(7, PetKind(0));
        s.update(crate::game::engine::HATCHING_TICKS() as u32);
        assert_eq!(s.phase, Phase::Active);
        s.is_sleeping = false;
        s.tick_rem = 0;
        s.mood = pct(50);
        s.energy = pct(90);
        s.fullness = pct(50);
        s.hydration = pct(50);
        s.weight = pct(50);
        s.dirt = pct(10);
        s.fitness = pct(50);
        s.fun = pct(50);
        s
    }

    #[test]
    fn actions_relax_and_wash() {
        let mut s = pet();
        s.active_action = Some(Action::Relax);
        assert_eq!(s.display_anim(), DisplayAnim::Relaxing);
        s.active_action = Some(Action::Wash);
        assert_eq!(s.display_anim(), DisplayAnim::Washing);
        s.active_action = Some(Action::Feed);
        assert_eq!(s.display_anim(), DisplayAnim::Feeding);
    }

    #[test]
    fn status_words_get_their_own_art() {
        let mut s = pet();
        s.hydration = pct(20);
        assert_eq!(status(&s).0, StatusWord::Thirsty);
        assert_eq!(s.display_anim(), DisplayAnim::Thirsty);
        let mut s = pet();
        s.hangover_min = 30;
        s.energy = pct(90);
        assert_eq!(status(&s).0, StatusWord::HungOver);
        assert_eq!(s.display_anim(), DisplayAnim::HungOver);
        let mut s = pet();
        s.groggy_min = 30;
        assert_eq!(status(&s).0, StatusWord::Groggy);
        assert_eq!(s.display_anim(), DisplayAnim::Groggy);
        let mut s = pet();
        s.injured_min = 30;
        assert_eq!(status(&s).0, StatusWord::Injured);
        assert_eq!(s.display_anim(), DisplayAnim::Injured);
        let mut s = pet();
        s.fun = pct(100);
        assert_eq!(status(&s).0, StatusWord::Wired);
        assert_eq!(s.display_anim(), DisplayAnim::Wired);
        let mut s = pet();
        s.fullness = pct(97);
        assert_eq!(status(&s).0, StatusWord::Stuffed);
        assert_eq!(s.display_anim(), DisplayAnim::Stuffed);
    }

    #[test]
    fn hungry_in_danger_is_starving_warning_stays() {
        let mut s = pet();
        s.fullness = pct(30);
        assert_eq!(s.display_anim(), DisplayAnim::WarningHungry);
        s.fullness = pct(10);
        assert_eq!(s.display_anim(), DisplayAnim::Starving);
    }

    #[test]
    fn drunk_levels() {
        let mut s = pet();
        s.booze = (needs::booze_danger() + P) as u16;
        assert_eq!(s.display_anim(), DisplayAnim::ThrowingUp);
        s.booze = (needs::booze_hi() + P) as u16;
        assert_eq!(s.display_anim(), DisplayAnim::Wasted);
        // A critical status word in danger still wins over Wasted.
        s.fullness = 0;
        assert_eq!(status(&s), (StatusWord::Hungry, true));
        assert_eq!(s.display_anim(), DisplayAnim::Starving);
        // ... but not over ThrowingUp.
        s.booze = (needs::booze_danger() + P) as u16;
        assert_eq!(s.display_anim(), DisplayAnim::ThrowingUp);
    }

    #[test]
    fn tipsy_only_replaces_idle_and_happy() {
        let mut s = pet();
        s.booze = pct(TIPSY_PCT as i32);
        assert_eq!(s.display_anim(), DisplayAnim::Tipsy);
        s.mood = pct(90);
        assert_eq!(s.display_anim(), DisplayAnim::Tipsy);
        // Mildly hungry: the warning anim stays.
        let mut s = pet();
        s.booze = pct(TIPSY_PCT as i32);
        s.fullness = pct(30);
        assert_eq!(s.display_anim(), DisplayAnim::WarningHungry);
    }

    #[test]
    fn rehab_and_sore_take_over_content() {
        let mut s = pet();
        s.dependent = true;
        s.rehab_min = 0;
        assert_eq!(s.display_anim(), DisplayAnim::NeedsRehab);
        // Rehab outranks sore and tipsy; being in rehab is not "needs rehab".
        s.strain = pct(90);
        s.booze = pct(TIPSY_PCT as i32);
        assert_eq!(s.display_anim(), DisplayAnim::NeedsRehab);
        s.rehab_min = 30;
        assert_eq!(s.display_anim(), DisplayAnim::Sore);
        s.strain = 0;
        assert_eq!(s.display_anim(), DisplayAnim::Tipsy);
    }

    #[test]
    fn missing_art_falls_back_to_the_old_anim() {
        let none = |_| false;
        // An action.
        let mut s = pet();
        s.active_action = Some(Action::Wash);
        assert_eq!(s.display_anim_with(none), DisplayAnim::Playing);
        s.active_action = Some(Action::Relax);
        assert_eq!(s.display_anim_with(none), DisplayAnim::Playing);
        // A status word.
        let mut s = pet();
        s.hydration = pct(20);
        assert_eq!(s.display_anim_with(none), DisplayAnim::WarningHungry);
        s.fullness = pct(10);
        assert_eq!(s.display_anim_with(none), DisplayAnim::CriticalHungry);
        // A content-level one.
        let mut s = pet();
        s.booze = pct(TIPSY_PCT as i32);
        assert_eq!(s.display_anim_with(none), DisplayAnim::Idle);
        s.booze = needs::booze_danger() as u16;
        assert_eq!(s.display_anim(), DisplayAnim::Wasted);
        assert_eq!(s.display_anim_with(none), DisplayAnim::WarningMiserable);
        let mut s = pet();
        s.dependent = true;
        assert_eq!(s.display_anim_with(none), DisplayAnim::Idle);
        // Art for just one row: only that row is used.
        let mut s = pet();
        s.hydration = pct(20);
        assert_eq!(
            s.display_anim_with(|a| a == DisplayAnim::Thirsty),
            DisplayAnim::Thirsty
        );
        assert_eq!(
            s.display_anim_with(|a| a == DisplayAnim::Wired),
            DisplayAnim::WarningHungry
        );
    }
}
