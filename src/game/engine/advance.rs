//! Closed-form advance of the pet by whole engine minutes, and waking.
//!
//! [`advance`] applies the constant rates of the regime the pet is in at the
//! start of the call for `dt` minutes in one step. The caller (the segment
//! solver) guarantees no threshold is crossed inside the step, so every rate
//! stays valid. Every number is a [`thresholds`] field.

use super::causes::{Cause, Unsettled};
use super::needs::{self, MAX, Need, P};
use super::regime::{
    energy_cap, health_ceiling, health_damage, health_recovery, mood_target, rates,
    sleep_quality_pm, unsettled_by,
};
use super::thresholds as t;
use super::{GameState, Phase};

/// `dependence_min` and `diabetes_min` count quarter-minutes: +`SLOW_SCALE` per
/// minute while rising, -1 per minute while decaying (a quarter of the speed),
/// so a split advance gives the same result as one call (R17). Not tunable:
/// a config change must not change what a saved value means.
pub const SLOW_SCALE: u32 = 4;
/// `leaving_min` counts half-minutes: +`LEAVING_SCALE` per minute at health 0,
/// -1 per minute otherwise.
pub const LEAVING_SCALE: u32 = 2;
/// A night is a bad night when at least this many minutes of it were poor
/// (display constant, not a balance value) ...
pub const RESTLESS_MIN: u16 = 60;
/// ... and the poor minutes are more than 1 in this many of the minutes
/// slept short of oversleeping (more than half).
const BAD_NIGHT_POOR_SHARE_DEN: u32 = 2;

#[inline]
fn lvl(pct: u8) -> i32 {
    pct as i32 * P
}

/// `level + delta`, clamped to the `u16` level range.
#[inline]
fn bump(level: u16, delta: i64) -> u16 {
    (level as i64 + delta).clamp(0, MAX as i64) as u16
}

#[inline]
fn add_min(timer: &mut u16, dt: u32) {
    *timer = (*timer as u32).saturating_add(dt).min(u16::MAX as u32) as u16;
}

#[inline]
fn sub_min(timer: &mut u16, dt: u32) {
    *timer = (*timer as u32).saturating_sub(dt) as u16;
}

/// Advance by `dt` minutes at the rates of the regime at the start.
/// Caller guarantees no threshold is crossed (see the segment solver).
pub fn advance(s: &mut GameState, dt: u32) {
    if s.phase == Phase::Gone || dt == 0 {
        return;
    }
    let r = rates(s);
    let asleep = s.is_sleeping;
    let cap = energy_cap(s);
    let was_full_energy = s.energy as i32 >= cap;
    let health_before = s.health;

    // Sleep counters.
    if asleep {
        add_min(&mut s.sleep_min, dt);
        if was_full_energy {
            add_min(&mut s.oversleep_min, dt);
        } else if sleep_quality_pm(s) == t::SLEEP_QUALITY_GOOD_PM() as i32 {
            add_min(&mut s.good_sleep_min, dt);
        }
    }

    super::demo::tick(s, dt);

    // Mini-game and Battle cooldowns (ticks) run down with time; no regime
    // term reads them, so they never bound a segment (R20).
    let spent = (dt as u64 * super::TICKS_PER_MINUTE as u64).min(u16::MAX as u64) as u16;
    for c in [
        &mut s.cooldown_battle,
        &mut s.cooldown_tictactoe,
        &mut s.cooldown_lightsout,
        &mut s.cooldown_blackhole,
        &mut s.cooldown_bornjeweled,
    ] {
        *c = c.saturating_sub(spent);
    }

    // The prize timer counts down; nothing visible reads it, so it is not a
    // solver boundary.
    sub_min(&mut s.prize_min, dt);

    // Slow counters, by the regime at the start.
    let (booze, weight, fit) = (s.booze as i32, s.weight as i32, s.fitness as i32);
    let (heavy, lean) = (
        lvl(t::DIABETES_WEIGHT_HI_PCT()),
        lvl(t::DIABETES_WEIGHT_LO_PCT()),
    );
    if booze > needs::booze_hi() {
        // A demo pet never drifts toward dependence: nothing is left behind
        // for when demo goes off.
        if !s.demo.on {
            s.dependence_min = s.dependence_min.saturating_add(dt * SLOW_SCALE);
        }
    } else if booze < lvl(t::BOOZE_CLEAR_PCT()) {
        s.dependence_min = s.dependence_min.saturating_sub(dt);
    }
    if weight > heavy {
        s.diabetes_min = s.diabetes_min.saturating_add(dt * SLOW_SCALE);
    } else if weight < lean {
        s.diabetes_min = s.diabetes_min.saturating_sub(dt);
    }
    if s.diabetic && weight < lean && fit > lvl(t::REVERSAL_FIT_PCT()) {
        add_min(&mut s.reversal_min, dt);
    }

    // The nine needs, clamped.
    let step = |need: Need| r[need as usize] as i64 * dt as i64;
    s.fullness = bump(s.fullness, step(Need::Full));
    s.hydration = bump(s.hydration, step(Need::Water));
    s.energy = bump(s.energy, step(Need::Energy));
    if asleep && r[Need::Energy as usize] > 0 {
        s.energy = s.energy.min(cap as u16);
    }
    s.fun = bump(s.fun, step(Need::Fun));
    if asleep && r[Need::Fun as usize] != 0 {
        s.fun = s.fun.max(lvl(t::SLEEP_FUN_FLOOR_PCT()) as u16);
    }
    s.dirt = bump(s.dirt, step(Need::Dirt));
    s.booze = bump(s.booze, step(Need::Booze));
    s.strain = bump(s.strain, step(Need::Strain));
    s.fitness = super::demo::fit_after_fall(s, s.fitness, bump(s.fitness, step(Need::Fit)));
    s.weight = bump(s.weight, step(Need::Weight));

    // Booze peak and hangover.
    if s.booze as i32 > needs::booze_hi() {
        s.booze_peak = true;
    }
    if s.booze_peak && (s.booze as i32) < lvl(t::BOOZE_CLEAR_PCT()) {
        s.booze_peak = false;
        s.hangover_min = t::HANGOVER_MIN();
    }

    // Flags.
    let relapse_window = s.recovering_min > 0;
    let dependence_line = if relapse_window {
        t::DEPENDENCE_RELAPSE_MIN()
    } else {
        t::DEPENDENCE_ONSET_MIN()
    };
    if !s.demo.on && !s.dependent && s.dependence_min >= dependence_line as u32 * SLOW_SCALE {
        s.dependent = true;
    }
    if !s.diabetic && s.diabetes_min >= t::DIABETES_ONSET_MIN() as u32 * SLOW_SCALE {
        s.diabetic = true;
        s.reversal_min = 0;
    }
    if s.diabetic && s.reversal_min >= t::DIABETES_REVERSAL_MIN() {
        s.diabetic = false;
        s.diabetes_min = 0;
    }

    // Timers.
    let hangover_dt = if asleep {
        dt.saturating_mul(t::HANGOVER_ASLEEP_MULT() as u32)
    } else {
        dt
    };
    sub_min(&mut s.hangover_min, hangover_dt);
    let rehab_running = s.rehab_min > 0;
    sub_min(&mut s.rehab_min, dt);
    for timer in [
        &mut s.rested_min,
        &mut s.injured_min,
        &mut s.decline_min,
        &mut s.groggy_min,
        &mut s.med_cd_min,
        &mut s.recovering_min,
        &mut s.junk_min,
        &mut s.insulin_min,
        &mut s.ozempic_min,
        &mut s.ozempic_cd_min,
        &mut s.caffeine_min,
    ] {
        sub_min(timer, dt);
    }
    if rehab_running && s.rehab_min == 0 {
        s.dependent = false;
        s.dependence_min = 0;
        s.recovering_min = t::RECOVERING_MIN();
    }

    // Self-wake while asleep; collapse and the oversleep reset while awake.
    if s.is_sleeping {
        if s.sleep_min >= t::SELF_WAKE_MIN() || s.oversleep_min >= t::OVERSLEEP_WAKE_MIN() {
            wake(s, false);
        }
    } else {
        if (s.energy as i32) < lvl(t::OVERSLEEP_RESET_PCT()) {
            s.oversleep_min = 0;
        }
        if s.energy == 0 {
            if s.health as i32 > lvl(t::COLLAPSE_HEALTH_FLOOR_PCT()) {
                s.health = bump(s.health, -(t::COLLAPSE_DAMAGE() as i64));
            }
            s.night = true;
            s.is_sleeping = true;
            s.demo.clock_armed = false;
            s.sleep_min = 0;
            s.good_sleep_min = 0;
            s.bed_why = unsettled_by(s);
        }
    }

    // Health follows damage, recovery and the ceiling (on the mood zone at the
    // start of the segment); then mood chases its target, which reads the new
    // health, so a segment matches stepping (R22).
    let ceiling = health_ceiling(s) as i64;
    let mut h = s.health as i64 - health_damage(s) as i64 * dt as i64;
    h = if h < ceiling {
        (h + health_recovery(s) as i64 * dt as i64).min(ceiling.max(h))
    } else {
        (h - t::HEALTH_EXCESS_DECAY_PM() as i64 * dt as i64).max(ceiling)
    };
    s.health = h.clamp(0, MAX as i64) as u16;
    if s.demo.on {
        s.health = super::demo::floor(health_before, s.health, super::demo::health_floor());
    }

    let reach = t::MOOD_STEP() as i64 * dt as i64;
    let mood = s.mood as i64;
    s.mood = (mood_target(s) as i64)
        .clamp(mood - reach, mood + reach)
        .clamp(0, MAX as i64) as u16;

    // Leaving.
    if s.health == 0 && !s.demo.on {
        s.leaving_min = s.leaving_min.saturating_add(dt * LEAVING_SCALE);
        if s.leaving_min >= t::LEAVING_MIN() as u32 * LEAVING_SCALE {
            s.phase = Phase::Gone;
            s.save_pending = true;
            s.realm_pending = true;
        } else if s.phase == Phase::Active {
            s.phase = Phase::Leaving;
            s.save_pending = true;
        }
    } else if s.leaving_min > 0 {
        s.leaving_min = s.leaving_min.saturating_sub(dt);
        if s.leaving_min == 0 && s.phase == Phase::Leaving {
            s.phase = Phase::Active;
            s.save_pending = true;
        }
    }

    // Care counters. Demo time is not care time.
    if s.demo.on {
        let before = s.demo.demo_min;
        s.demo.demo_min = before.saturating_add(dt);
        super::demo::drift_hax(s, before);
    } else if !asleep {
        s.care_awake_min = s.care_awake_min.saturating_add(dt);
        if s.mood as i32 >= lvl(t::CARE_GOOD_MOOD_PCT()) {
            s.care_good_min = s.care_good_min.saturating_add(dt);
        }
    }
}

/// Wake a sleeping pet and settle the night's bookkeeping.
pub fn wake(s: &mut GameState, by_player: bool) {
    if !s.is_sleeping {
        return;
    }
    s.is_sleeping = false;
    s.demo.clock_armed = false;
    if by_player && (s.energy as i32) < lvl(t::GRUMPY_ENERGY_PCT()) {
        s.mood = bump(s.mood, -(lvl(t::GRUMPY_PCT()) as i64));
        s.causes.push(Cause::Grumpy);
    }
    // Sleep-deprived after enough short nights in a row; enough good nights clear it.
    // Naps (not `night`) never count.
    if s.night {
        if s.sleep_min < t::NIGHT_SHORT_MIN() {
            s.short_nights = s.short_nights.saturating_add(1);
            s.good_nights = 0;
            if s.short_nights >= t::DEPRIVED_NIGHTS() {
                if !s.deprived {
                    s.causes.push(Cause::WornOut);
                }
                s.deprived = true;
            }
        } else {
            s.short_nights = 0;
            if s.sleep_min >= t::NIGHT_GOOD_MIN() {
                s.good_nights = s.good_nights.saturating_add(1);
                if s.good_nights >= t::CLEARING_NIGHTS() {
                    s.deprived = false;
                }
            }
        }
    }
    // The night as slept, before a station rest forgives the oversleep:
    // minutes slept short of oversleeping, and how many of them were poor.
    let span = s.sleep_min.saturating_sub(s.oversleep_min);
    let poor = span.saturating_sub(s.good_sleep_min);
    if s.no_groggy {
        s.no_groggy = false;
        s.oversleep_min = 0;
    }
    if s.oversleep_min > t::GROGGY_LINE_MIN() {
        s.groggy_min = t::GROGGY_MIN();
        s.energy = bump(s.energy, -(lvl(t::GROGGY_ENERGY_PCT()) as i64));
        let fit = bump(s.fitness, -(lvl(t::GROGGY_FIT_PCT()) as i64));
        s.fitness = super::demo::fit_after_fall(s, s.fitness, fit);
        s.causes.push(Cause::Groggy);
    }
    let bad_night = poor >= RESTLESS_MIN && poor as u32 * BAD_NIGHT_POOR_SHARE_DEN > span as u32;
    if bad_night {
        let why = s.bed_why.or_else(|| unsettled_by(s));
        s.causes
            .push(Cause::BadNight(why.unwrap_or(Unsettled::Unknown)));
    }
    if s.hangover_min > 0 {
        s.causes.push(Cause::StillHungOver);
    }
    if s.good_sleep_min >= t::GOOD_SLEEP_LINE_MIN() {
        s.rested_min = t::RESTED_MIN();
        // A night cannot be both bad and good in the diary.
        if !bad_night {
            s.causes.push(Cause::WellRested);
        }
    }
    s.bed_why = None;
}
