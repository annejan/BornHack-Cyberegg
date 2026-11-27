//! Per-minute need rates of the pet's current regime (awake or asleep), plus
//! the mood target and health terms.
//!
//! Rates are signed levels per engine minute on the `u16` scale. Every
//! magnitude and multiplier is a [`thresholds`] field; the sign lives here.
//! Multipliers are applied in the order listed, each as an integer
//! `x * pct / 100` that truncates toward zero.

use super::GameState;
use super::causes::Unsettled;
use super::needs::{self, MAX, NEEDS, Need, P};
use super::thresholds as t;

#[inline]
fn lvl(pct: u8) -> i32 {
    pct as i32 * P
}

#[inline]
fn pct_of(x: i32, pct: u16) -> i32 {
    x * pct as i32 / 100
}

/// Thirst multiplier (percent) from the booze level: it dries the pet out.
pub fn thirst_pct(s: &GameState) -> i32 {
    let b = s.booze as i32;
    (if b < lvl(t::THIRST_BOOZE_LOW_PCT()) {
        t::THIRST_MULT_BASE_PCT()
    } else if b < lvl(t::THIRST_BOOZE_MID_PCT()) {
        t::THIRST_MULT_LOW_PCT()
    } else if b < lvl(t::THIRST_BOOZE_HIGH_PCT()) {
        t::THIRST_MULT_MID_PCT()
    } else {
        t::THIRST_MULT_HIGH_PCT()
    }) as i32
}

/// Energy ceiling while asleep: lower when the pet is deprived.
pub fn energy_cap(s: &GameState) -> i32 {
    if s.deprived {
        lvl(t::ENERGY_CAP_DEPRIVED_PCT())
    } else {
        MAX
    }
}

/// What keeps a sleeper from resting, first reason first; `None` when the pet
/// is settled. The one predicate: sleep quality is poor exactly when this
/// is `Some`.
pub fn unsettled_by(s: &GameState) -> Option<Unsettled> {
    let (b, f, h, fun) = (
        s.booze as i32,
        s.fullness as i32,
        s.hydration as i32,
        s.fun as i32,
    );
    if b > lvl(t::UNSETTLED_BOOZE_PCT()) {
        Some(Unsettled::Drinks)
    } else if f < lvl(t::UNSETTLED_FULL_LO_PCT()) {
        Some(Unsettled::Hungry)
    } else if f > lvl(t::UNSETTLED_FULL_HI_PCT()) {
        Some(Unsettled::Stuffed)
    } else if h < lvl(t::UNSETTLED_WATER_LO_PCT()) {
        Some(Unsettled::Thirsty)
    } else if h > lvl(t::UNSETTLED_WATER_HI_PCT()) {
        Some(Unsettled::Waterlogged)
    } else if fun > needs::fun_hi() {
        Some(Unsettled::Wired)
    } else if s.rehab_min > 0 {
        Some(Unsettled::Withdrawal)
    } else {
        None
    }
}

/// Sleep recovery quality in per-mille: poor when the pet is unsettled.
pub fn sleep_quality_pm(s: &GameState) -> i32 {
    (if unsettled_by(s).is_some() {
        t::SLEEP_QUALITY_POOR_PM()
    } else {
        t::SLEEP_QUALITY_GOOD_PM()
    }) as i32
}

/// Awake energy drain multiplier in per-mille (1000 = base rate).
fn energy_drain_pm(s: &GameState) -> i32 {
    let (fit, weight) = (s.fitness as i32, s.weight as i32);
    let mut m = 1000;
    if fit < needs::fit_lo() {
        m = pct_of(m, t::ENERGY_MULT_UNFIT_PCT());
    }
    if fit >= lvl(t::ENERGY_FIT_GOOD_PCT()) {
        m = pct_of(m, t::ENERGY_MULT_FIT_PCT());
    }
    if s.booze as i32 > needs::booze_hi() {
        m = pct_of(m, t::ENERGY_MULT_DRUNK_PCT());
    }
    if s.fun as i32 > needs::fun_hi() {
        m = pct_of(m, t::ENERGY_MULT_WIRED_PCT());
    }
    if s.hangover_min > 0 {
        m = pct_of(m, t::ENERGY_MULT_HANGOVER_PCT());
    }
    if weight > needs::weight_hi() || weight < needs::weight_lo() {
        m = pct_of(m, t::ENERGY_MULT_WEIGHT_PCT());
    }
    m
}

/// Per-minute rate of each need, indexed by `Need as usize`.
pub fn rates(s: &GameState) -> [i32; NEEDS] {
    let thirst = thirst_pct(s);
    let (full, water, energy, fun);
    if s.is_sleeping {
        full = -(t::FULL_DRAIN_ASLEEP_PM() as i32);
        water = -(t::WATER_DRAIN_ASLEEP_PM() as i32) * thirst / 100;
        energy = if (s.energy as i32) < energy_cap(s) {
            t::ENERGY_GAIN_ASLEEP_PM() as i32 * sleep_quality_pm(s) / 1000
        } else {
            0
        };
        fun = if s.fun as i32 > lvl(t::SLEEP_FUN_FLOOR_PCT()) {
            -(t::FUN_DRAIN_PM() as i32)
        } else {
            0
        };
    } else {
        full = -(t::FULL_DRAIN_AWAKE_PM() as i32);
        water = -(t::WATER_DRAIN_AWAKE_PM() as i32) * thirst / 100;
        energy = -(t::ENERGY_DRAIN_AWAKE_PM() as i32) * energy_drain_pm(s) / 1000;
        fun = -(t::FUN_DRAIN_PM() as i32);
    }
    let (fit, weight, fullness) = (s.fitness as i32, s.weight as i32, s.fullness as i32);
    let heavy = weight > needs::weight_hi();

    let mut booze = t::BOOZE_CLEAR_PM() as i32;
    if (s.hydration as i32) < needs::water_lo() {
        booze = pct_of(booze, t::BOOZE_CLEAR_SLOW_PCT());
    }
    if s.dependent {
        booze = pct_of(booze, t::BOOZE_CLEAR_SLOW_PCT());
    }

    let mut fit_decay = if fit > lvl(t::FIT_DECAY_HIGH_ABOVE_PCT()) {
        t::FIT_DECAY_HIGH_PM()
    } else if fit > lvl(t::FIT_DECAY_MID_ABOVE_PCT()) {
        t::FIT_DECAY_MID_PM()
    } else {
        t::FIT_DECAY_LOW_PM()
    } as i32;
    if heavy {
        fit_decay = pct_of(fit_decay, t::FIT_DECAY_HEAVY_PCT());
    }

    // Metabolism pulls weight back toward its band; an empty or stuffed belly shifts it.
    let metabolism = if heavy {
        -(t::WEIGHT_PULL_HEAVY_PM() as i32)
    } else if weight < needs::weight_lo() {
        0
    } else {
        -(t::WEIGHT_PULL_PM() as i32)
    };
    let belly = if fullness < needs::full_lo() {
        -(t::WEIGHT_BELLY_PM() as i32)
    } else if fullness > needs::full_hi() {
        t::WEIGHT_BELLY_PM() as i32
    } else {
        0
    };

    let mut r = [0; NEEDS];
    r[Need::Full as usize] = full;
    r[Need::Water as usize] = water;
    r[Need::Energy as usize] = energy;
    r[Need::Fun as usize] = fun;
    r[Need::Dirt as usize] = t::DIRT_GAIN_PM() as i32;
    r[Need::Booze as usize] = -booze;
    r[Need::Strain as usize] = -(t::STRAIN_CLEAR_PM() as i32);
    r[Need::Fit as usize] = -fit_decay;
    r[Need::Weight as usize] = metabolism + belly;
    r
}

/// Mood the pet drifts toward (`MAX` when nothing is wrong), moving at most
/// `MOOD_STEP` per minute. Each need contributes a penalty only when outside
/// its band. The 3/2 and 1/2 weights are structure (how much a need hurts
/// relative to its distance), not balance, so they stay literals.
pub fn mood_target(s: &GameState) -> i32 {
    needs::clamp_level(MAX - mood_penalty(s))
}

/// Terms of [`penalty_terms`] that truncate in integer division: four
/// `* 3 / 2` (hungry, thirsty, tired, bored-high fun) and three `/ 2`
/// (overweight, underweight, unfit). Each wobbles the mood target by under 1
/// per minute against a straight line, which `Self::segment` must allow for
/// (`R = TRUNCATING_TERMS + 1`). Add a truncating term to `penalty_terms` and
/// this count MUST go up with it, or the segmenting is no longer exact (R21).
pub const TRUNCATING_TERMS: i32 = 4 + 3;

/// Number of per-need and per-condition terms of [`penalty_terms`].
pub const PENALTY_TERMS: usize = 16;

/// Scale of [`penalty_terms_x2`] against [`penalty_terms`].
pub const EXACT_SCALE: i32 = 2;

/// The mood penalty terms that have a name for the player (see
/// `status::status`), in status-word order: hungry, stuffed, thirsty,
/// waterlogged, tired, bored, wired, sloppy, grubby, overweight,
/// underweight, unfit, hung over, injured, groggy, poorly. Tired and bored
/// only count while awake.
pub fn penalty_terms(s: &GameState) -> [i32; PENALTY_TERMS] {
    penalty_terms_x2(s).map(|t| t / EXACT_SCALE)
}

/// [`penalty_terms`] times [`EXACT_SCALE`], before any division: every term
/// is a straight line in the levels, so the order of two terms is exact
/// (the truncated values can tie on alternate minutes). The status line ranks
/// on these; [`penalty_terms`] is these halved.
pub fn penalty_terms_x2(s: &GameState) -> [i32; PENALTY_TERMS] {
    let gap = |x: i32| x.max(0);
    let awake = !s.is_sleeping;
    let (full, water, energy, fun) = (
        s.fullness as i32,
        s.hydration as i32,
        s.energy as i32,
        s.fun as i32,
    );
    let (weight, fit) = (s.weight as i32, s.fitness as i32);
    let flat = |on: bool, pct: u16| if on { pct as i32 * P * EXACT_SCALE } else { 0 };
    [
        gap(needs::full_lo() - full) * 3,
        gap(full - needs::full_hi()) * 2,
        gap(needs::water_lo() - water) * 3,
        gap(water - needs::water_hi()) * 2,
        if awake {
            gap(needs::energy_lo() - energy) * 3
        } else {
            0
        },
        if awake {
            gap(needs::fun_lo() - fun) * 2
        } else {
            0
        },
        gap(fun - needs::fun_hi()) * 3,
        gap(s.booze as i32 - needs::booze_hi()) * 2,
        gap(s.dirt as i32 - needs::dirt_hi()) * 2,
        gap(weight - needs::weight_hi()),
        gap(needs::weight_lo() - weight),
        gap(needs::fit_lo() - fit),
        flat(s.hangover_min > 0, t::MOOD_PEN_HANGOVER_PCT()),
        flat(s.injured_min > 0, t::MOOD_PEN_INJURED_PCT()),
        flat(s.groggy_min > 0, t::MOOD_PEN_GROGGY_PCT()),
        gap(lvl(t::MOOD_HEALTH_FLOOR_PCT()) - s.health as i32) * 2,
    ]
}

/// Total mood penalty before the clamp to the level range; `>= MAX` means the
/// target sits at 0 (a kink the segment solver tracks through `sig`).
pub fn mood_penalty(s: &GameState) -> i32 {
    let flat = |on: bool, pct: u16| if on { pct as i32 * P } else { 0 };
    let mut pen: i32 = penalty_terms(s).iter().sum();
    pen += flat(s.deprived, t::MOOD_PEN_DEPRIVED_PCT());
    pen += flat(s.decline_min > 0, t::BATTLE_DECLINE_MOOD_PCT());
    pen += if s.rehab_min > 0 {
        flat(true, t::MOOD_PEN_REHAB_PCT())
    } else {
        flat(craving(s), t::MOOD_PEN_CRAVING_PCT())
    };
    pen
}

/// Craving: dependent, nearly sober, awake and not in rehab. The mood
/// penalty and the fight's snapshot read this one rule.
pub fn craving(s: &GameState) -> bool {
    s.rehab_min == 0
        && s.dependent
        && (s.booze as i32) < lvl(t::MOOD_CRAVING_BOOZE_PCT())
        && !s.is_sleeping
}

/// Highest health the pet can recover to: steps up with fitness; an
/// untreated diabetic loses a fixed slice.
pub fn health_ceiling(s: &GameState) -> i32 {
    ceiling_with(t::cfg(), s.fitness as i32, s.diabetic && s.insulin_min == 0)
}

/// [`health_ceiling`] for explicit settings and inputs, clamped to the level
/// range so a config value can never push it past `MAX` or below zero.
pub fn ceiling_with(th: &t::Thresholds, fit: i32, untreated_diabetic: bool) -> i32 {
    let step = if fit < lvl(th.CEILING_FIT_STEP1_PCT) {
        th.CEILING_STEP0_PCT
    } else if fit < lvl(th.CEILING_FIT_STEP2_PCT) {
        th.CEILING_STEP1_PCT
    } else if fit < lvl(th.CEILING_FIT_STEP3_PCT) {
        th.CEILING_STEP2_PCT
    } else {
        th.CEILING_STEP3_PCT
    } as i32;
    let cut = if untreated_diabetic {
        th.CEILING_DIABETIC_PCT as i32
    } else {
        0
    };
    ((step - cut) * P).clamp(0, MAX)
}

/// Health lost per engine minute.
pub fn health_damage(s: &GameState) -> i32 {
    let (full, water, energy) = (s.fullness as i32, s.hydration as i32, s.energy as i32);
    let (low, high) = (lvl(t::DMG_NEED_LOW_PCT()), lvl(t::DMG_NEED_HIGH_PCT()));
    let weight = s.weight as i32;
    let terms = [
        (full < low, t::DMG_FULL_LOW_PM()),
        (full > high, t::DMG_FULL_HIGH_PM()),
        (water < low, t::DMG_WATER_LOW_PM()),
        (water > high, t::DMG_WATER_HIGH_PM()),
        // sleep shortage alone never takes health under the floor
        (
            energy < lvl(t::DMG_ENERGY_PCT())
                && !s.is_sleeping
                && s.health as i32 > lvl(t::DMG_ENERGY_HEALTH_PCT()),
            t::DMG_ENERGY_PM(),
        ),
        (s.booze as i32 > needs::booze_danger(), t::DMG_BOOZE_PM()),
        ((s.mood as i32) < lvl(t::DMG_MOOD_PCT()), t::DMG_MOOD_PM()),
        (s.dirt as i32 > lvl(t::DMG_DIRT_PCT()), t::DMG_DIRT_PM()),
        (
            weight > lvl(t::DMG_WEIGHT_HIGH_PCT()),
            t::DMG_WEIGHT_HIGH_PM(),
        ),
        (
            weight < lvl(t::DMG_WEIGHT_LOW_PCT()),
            t::DMG_WEIGHT_LOW_PM(),
        ),
    ];
    terms
        .iter()
        .filter(|(on, _)| *on)
        .map(|(_, d)| *d as i32)
        .sum()
}

/// Health regained per engine minute while below the ceiling (the caller
/// applies the ceiling).
pub fn health_recovery(s: &GameState) -> i32 {
    let mood = s.mood as i32;
    let base = if mood >= lvl(t::REC_GOOD_MOOD_PCT()) {
        t::REC_GOOD_PM()
    } else if mood >= lvl(t::REC_FAIR_MOOD_PCT()) {
        t::REC_FAIR_PM()
    } else {
        0
    } as i32;
    if s.deprived {
        pct_of(base, t::REC_DEPRIVED_PCT())
    } else {
        base
    }
}
