//! Debug menu support (the hidden Konami-code page, `game::debug_cheats`).
//!
//! Pokes write the same fields the rules read, so the next `update` behaves
//! as if the pet had got there by itself. Not part of normal gameplay.

use super::doses::{Reason, fmt_wait};
use super::needs::{self, DIRT_DANGER_PCT, MAX, NEED_DANGER_LO_PCT, P, pct};
use super::status::{StatusWord, status};
use super::thresholds as t;
use super::{GameState, TICKS_PER_MINUTE, regime};

/// One thing the debug menu can set. The need pokes land clearly past the
/// line that names them (past the danger line where the need has one, a few
/// points past the band edge otherwise) so `status()` names them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DebugPoke {
    Hungry,
    Thirsty,
    Stuffed,
    Tired,
    Bored,
    Dirty,
    Fat,
    Thin,
    Unfit,
    Drunk,
    HungOver,
    Groggy,
    Injured,
    Diabetic,
    Dependent,
    /// Every need mid-band, mood and health 70 %.
    Comfort,
    ClearConditions,
    ClearCooldowns,
}

/// Points past a band edge that make the status word name the need: the
/// word needs a penalty term above 3 points, and the weight and fitness
/// terms count half.
const PAST_EDGE: i32 = 8 * P;

/// Mood and health `Comfort` sets, in percent.
pub const COMFORT_PCT: i32 = 70;

/// What the engine is doing, for the "Engine info" row. Percent values.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DebugInfo {
    pub status: StatusWord,
    pub danger: bool,
    pub mood: u8,
    pub mood_target: u8,
    pub health: u8,
    /// Minutes until the engine next wants to be woken, `None` when it
    /// sleeps until something outside changes (hibernating, gone).
    pub wake_in_min: Option<u32>,
    pub asleep: bool,
    pub night: bool,
}

impl StatusWord {
    /// The word as shown on screen.
    pub fn name(self) -> &'static str {
        match self {
            StatusWord::Content => "Content",
            StatusWord::Hungry => "Hungry",
            StatusWord::Stuffed => "Stuffed",
            StatusWord::Thirsty => "Thirsty",
            StatusWord::Waterlogged => "Waterlogged",
            StatusWord::Tired => "Tired",
            StatusWord::Bored => "Bored",
            StatusWord::Wired => "Wired",
            StatusWord::Sloppy => "Sloppy",
            StatusWord::Grubby => "Grubby",
            StatusWord::Overweight => "Overweight",
            StatusWord::Underweight => "Underweight",
            StatusWord::Unfit => "Unfit",
            StatusWord::HungOver => "Hung over",
            StatusWord::Injured => "Injured",
            StatusWord::Groggy => "Groggy",
            StatusWord::Poorly => "Poorly",
        }
    }
}

impl DebugInfo {
    /// Short lines for the toast (7x13 font; the toast budget is 15 columns,
    /// the lines stay within 11): the word, a flag line (`!` danger, `zz`
    /// asleep, `nt` night) when any is set, mood / target, then health and the
    /// wait to the next wake in the countdown format ("w23m", "w2h05").
    pub fn line(&self) -> heapless::String<48> {
        use core::fmt::Write;
        let mut out = heapless::String::new();
        let _ = out.push_str(self.status.name());
        let flags = [(self.danger, "!"), (self.asleep, "zz"), (self.night, "nt")];
        if flags.iter().any(|f| f.0) {
            let _ = out.push('\n');
            for (n, (_, f)) in flags.iter().filter(|f| f.0).enumerate() {
                if n > 0 {
                    let _ = out.push(' ');
                }
                let _ = out.push_str(f);
            }
        }
        let _ = write!(
            out,
            "\nm{}/{}\nh{} w",
            self.mood, self.mood_target, self.health
        );
        match self.wake_in_min {
            Some(m) => {
                let _ = out.push_str(fmt_wait(m * 60).as_str());
            }
            None => {
                let _ = out.push('-');
            }
        }
        out
    }
}

impl GameState {
    /// Why no poke can apply, `None` when one can. A running action does not
    /// stop a poke (it is left running).
    fn poke_refusal(&self) -> Option<Reason> {
        match self.cannot_act() {
            Some(Reason::Busy) | None => None,
            why => why,
        }
    }

    /// Set one thing straight away. A pet that is hatching, gone or
    /// hibernating takes none (the reason is the one every refused action
    /// shows). Never starts or ends an action, never charges HAX; asks for
    /// a save.
    pub fn debug_poke(&mut self, p: DebugPoke) -> Result<(), Reason> {
        if let Some(why) = self.poke_refusal() {
            return Err(why);
        }
        let danger_lo = NEED_DANGER_LO_PCT as i32 * P;
        let level = |v: i32| v.clamp(0, MAX) as u16;
        match p {
            DebugPoke::Hungry => self.fullness = level(danger_lo - P),
            DebugPoke::Thirsty => self.hydration = level(danger_lo - P),
            DebugPoke::Stuffed => self.fullness = level(needs::full_hi() + PAST_EDGE),
            DebugPoke::Tired => self.energy = level((t::DMG_ENERGY_PCT() as i32 - 1) * P),
            DebugPoke::Bored => self.fun = level(needs::fun_lo() - PAST_EDGE),
            DebugPoke::Dirty => self.dirt = level((DIRT_DANGER_PCT as i32 + 1) * P),
            DebugPoke::Fat => self.weight = level(needs::weight_hi() + PAST_EDGE),
            DebugPoke::Thin => self.weight = level(needs::weight_lo() - PAST_EDGE),
            DebugPoke::Unfit => self.fitness = level(needs::fit_lo() - PAST_EDGE),
            DebugPoke::Drunk => self.booze = level(needs::booze_danger() + P),
            DebugPoke::HungOver => self.hangover_min = t::HANGOVER_MIN(),
            DebugPoke::Groggy => self.groggy_min = t::GROGGY_MIN(),
            DebugPoke::Injured => self.injured_min = t::DOSE_INJURY_MIN(),
            DebugPoke::Diabetic => self.diabetic = true,
            DebugPoke::Dependent => self.dependent = true,
            DebugPoke::Comfort => {
                self.fullness = level((needs::full_lo() + needs::full_hi()) / 2);
                self.hydration = level((needs::water_lo() + needs::water_hi()) / 2);
                self.energy = level((needs::energy_lo() + MAX) / 2);
                self.fun = level((needs::fun_lo() + needs::fun_hi()) / 2);
                self.dirt = level(needs::dirt_hi() / 2);
                self.booze = 0;
                self.strain = level(needs::strain_inj() / 2);
                self.fitness = level((needs::fit_lo() + MAX) / 2);
                self.weight = level((needs::weight_lo() + needs::weight_hi()) / 2);
                self.mood = level(COMFORT_PCT * P);
                self.health = level(COMFORT_PCT * P);
            }
            DebugPoke::ClearConditions => {
                self.injured_min = 0;
                self.hangover_min = 0;
                self.groggy_min = 0;
                self.dependent = false;
                self.dependence_min = 0;
                self.diabetic = false;
                self.diabetes_min = 0;
                self.rehab_min = 0;
                self.reversal_min = 0;
            }
            DebugPoke::ClearCooldowns => {
                self.med_cd_min = 0;
                self.ozempic_cd_min = 0;
                self.insulin_min = 0;
                self.cooldown_tictactoe = 0;
                self.cooldown_lightsout = 0;
                self.cooldown_blackhole = 0;
                self.cooldown_bornjeweled = 0;
                self.cooldown_battle = 0;
            }
        }
        self.request_save();
        Ok(())
    }

    /// Fast-forward `minutes` through the normal `update` path, like real
    /// time, without moving the tick base (`catch_up`): the next real update
    /// still sees a forward clock. Works on any phase; hibernation and a
    /// finished pet are handled by `update` itself.
    pub fn debug_skip_minutes(&mut self, minutes: u32) {
        self.catch_up(minutes);
        self.request_save();
    }

    /// What the engine is doing right now.
    pub fn debug_info(&self) -> DebugInfo {
        let (status, danger) = status(self);
        let wake = self.next_wake_tick();
        let to_pct = |level: i32| pct(level).clamp(0, 100) as u8;
        DebugInfo {
            status,
            danger,
            mood: to_pct(self.mood as i32),
            mood_target: to_pct(regime::mood_target(self)),
            health: to_pct(self.health as i32),
            wake_in_min: (wake != u32::MAX).then(|| {
                wake.saturating_sub(self.last_update_tick)
                    .div_ceil(TICKS_PER_MINUTE)
            }),
            asleep: self.is_sleeping,
            night: self.night,
        }
    }

    /// Zero this pet's lifetime mesh Battle record and cooldown. Paired
    /// with `crate::game::friends::reset_all_battle_records` (which
    /// zeros the per-friend head-to-head numbers) by the
    /// `lifecycle::debug_reset_battle_record` wrapper — a badge that
    /// picked up inflated counts from a duplicate-delivery bug before it
    /// was fixed has no other way to get back to a clean baseline.
    pub fn debug_reset_battle_record(&mut self) {
        self.wins = 0;
        self.losses = 0;
        self.demo_wins = 0;
        self.demo_losses = 0;
        self.cooldown_battle = 0;
    }
}
