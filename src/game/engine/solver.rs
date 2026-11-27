//! Segment solver: how many minutes `advance` may take in one step.
//!
//! A segment ends one minute before anything `advance` or `regime` branches
//! on can change: a need level reaching a threshold, a minute timer
//! expiring, or a slow counter reaching its onset line. Inside a segment the
//! rates, penalties and mood/health terms are therefore constant, so the
//! closed-form `advance` is exact (R17, R18). Mood and health crossing their
//! own lines are not handled here.

use super::advance::{LEAVING_SCALE, SLOW_SCALE};
use super::demo;
use super::needs::{self, MAX, NEEDS, Need, P};
use super::regime::{energy_cap, health_ceiling, mood_penalty, rates};
use super::scene;
use super::thresholds as t;
use super::{GameState, Phase};

#[inline]
fn lvl(pct: u8) -> i32 {
    pct as i32 * P
}

/// `ceil(a / b)` for an amount that may already be spent (then 0).
#[inline]
fn ceil_div(a: i64, b: i64) -> i64 {
    if a <= 0 { 0 } else { (a + b - 1) / b }
}

/// Levels a need's rate or a penalty branches on that the edge table does
/// not guarantee: its fixed rows hold literals while these are tunable, and
/// the table lacks the thirst steps (booze 40 %) outright.
const MAX_EXTRA: usize = 16;

fn extra_levels(s: &GameState, need: Need) -> ([i32; MAX_EXTRA], usize) {
    let mut v = [0; MAX_EXTRA];
    let n = match need {
        Need::Full => {
            v[..4].copy_from_slice(&[
                lvl(t::UNSETTLED_FULL_LO_PCT()),
                lvl(t::UNSETTLED_FULL_HI_PCT()),
                lvl(t::DMG_NEED_LOW_PCT()),
                lvl(t::DMG_NEED_HIGH_PCT()),
            ]);
            4
        }
        Need::Water => {
            v[..4].copy_from_slice(&[
                lvl(t::UNSETTLED_WATER_LO_PCT()),
                lvl(t::UNSETTLED_WATER_HI_PCT()),
                lvl(t::DMG_NEED_LOW_PCT()),
                lvl(t::DMG_NEED_HIGH_PCT()),
            ]);
            4
        }
        Need::Energy => {
            v[..2].copy_from_slice(&[lvl(t::DMG_ENERGY_PCT()), lvl(t::OVERSLEEP_RESET_PCT())]);
            if s.is_sleeping {
                v[2] = energy_cap(s);
                3
            } else {
                2
            }
        }
        Need::Fun => {
            v[0] = lvl(t::SLEEP_FUN_FLOOR_PCT());
            1
        }
        Need::Dirt => {
            v[0] = lvl(t::DMG_DIRT_PCT());
            // Each floor item appears on its own dirt line. Kept although no
            // test fails without it: `next_wake_tick` invariant 2 (every line
            // the view reads is a segment boundary).
            for (slot, line) in scene::floor_lines().into_iter().enumerate() {
                v[1 + slot] = lvl(line) - 1;
            }
            1 + scene::FLOOR_ITEMS
        }
        Need::Booze => {
            v[..6].copy_from_slice(&[
                lvl(t::THIRST_BOOZE_LOW_PCT()),
                lvl(t::THIRST_BOOZE_MID_PCT()),
                lvl(t::THIRST_BOOZE_HIGH_PCT()),
                lvl(t::BOOZE_CLEAR_PCT()),
                lvl(t::MOOD_CRAVING_BOOZE_PCT()),
                lvl(t::UNSETTLED_BOOZE_PCT()),
            ]);
            6
        }
        Need::Strain => 0,
        Need::Fit => {
            v[..6].copy_from_slice(&[
                lvl(t::ENERGY_FIT_GOOD_PCT()),
                lvl(t::FIT_DECAY_HIGH_ABOVE_PCT()),
                lvl(t::FIT_DECAY_MID_ABOVE_PCT()),
                lvl(t::REVERSAL_FIT_PCT()),
                lvl(t::CEILING_FIT_STEP1_PCT()),
                lvl(t::CEILING_FIT_STEP2_PCT()),
            ]);
            v[6] = lvl(t::CEILING_FIT_STEP3_PCT());
            7
        }
        Need::Weight => {
            v[..4].copy_from_slice(&[
                lvl(t::DIABETES_WEIGHT_HI_PCT()),
                lvl(t::DIABETES_WEIGHT_LO_PCT()),
                lvl(t::DMG_WEIGHT_HIGH_PCT()),
                lvl(t::DMG_WEIGHT_LOW_PCT()),
            ]);
            4
        }
    };
    let mut n = n;
    if s.demo.on {
        let (lines, k) = demo::levels(s, need);
        v[n..n + k].copy_from_slice(&lines[..k]);
        n += k;
    }
    (v, n)
}

const NEED_ORDER: [Need; NEEDS] = [
    Need::Full,
    Need::Water,
    Need::Energy,
    Need::Fun,
    Need::Dirt,
    Need::Booze,
    Need::Strain,
    Need::Fit,
    Need::Weight,
];

/// Minutes until `level`, moving `rate` per minute, is one unit short of
/// `target`; `None` when `target` is behind it. A level already on the target
/// and moving off it flips a `>` / `<` test next minute, so that gives 0.
#[inline]
fn minutes_to(level: i32, rate: i32, target: i32) -> Option<i32> {
    let dist = target - level;
    if rate > 0 && dist >= 0 || rate < 0 && dist <= 0 {
        Some((dist.abs() - 1).max(0) / rate.abs())
    } else {
        None
    }
}

/// Shortest time any moving, unpinned need takes to reach a threshold.
fn level_bound(s: &GameState) -> u32 {
    let r = rates(s);
    let levels = [
        s.fullness,
        s.hydration,
        s.energy,
        s.fun,
        s.dirt,
        s.booze,
        s.strain,
        s.fitness,
        s.weight,
    ];
    let table = needs::edges();
    let mut best = u32::MAX;
    for i in 0..NEEDS {
        let (level, rate) = (levels[i] as i32, r[i]);
        let need = NEED_ORDER[i];
        if rate == 0
            || rate < 0 && level == 0
            || rate > 0 && level == MAX
            || demo::pinned(s, need, level, rate)
        {
            continue;
        }
        let (extra, extras) = extra_levels(s, need);
        let tabled = table.edges[i][..table.len[i] as usize]
            .iter()
            .map(|&p| lvl(p));
        let fixed = [0, MAX];
        for target in tabled.chain(extra[..extras].iter().copied()).chain(fixed) {
            // The clamp it is leaving is behind it, not ahead.
            if rate > 0 && target == 0 || rate < 0 && target == MAX {
                continue;
            }
            if let Some(n) = minutes_to(level, rate, target) {
                best = best.min(n as u32);
            }
        }
    }
    best
}

/// Minutes until the first timer or counter event, so that `advance` over the
/// segment equals stepping it.
///
/// `advance` evaluates the mood and health terms on the post-update state, so
/// a timer, counter or flag those terms read must flip no earlier than the
/// segment's last minute: it bounds the segment at `remaining - 1` (R20; 0
/// becomes a one-minute crossing segment). Timers no regime term reads keep
/// `remaining`.
fn timer_bound(s: &GameState) -> u32 {
    let mut best = i64::MAX;
    // A running timer nothing derived reads.
    let mut plain = |minutes: i64| {
        if minutes > 0 {
            best = best.min(minutes);
        }
    };
    for m in [
        s.rested_min,
        s.med_cd_min,
        s.junk_min,
        s.ozempic_min,
        s.ozempic_cd_min,
        s.recovering_min,
    ] {
        plain(m as i64);
    }
    // Caffeine wearing down to the line where a cup stops wiring the pet
    // (what the demo pet and the drink preview read); below it nothing does.
    if let Some(m) = super::doses::caffeine_to_safe(s) {
        plain(m as i64);
    }
    if s.health != 0 || s.demo.on {
        // Counting down (a demo pet never leaves, so health 0 counts down
        // too); reaching 0 returns a Leaving pet to Active.
        plain(s.leaving_min as i64);
    } else {
        plain(scaled_left(t::LEAVING_MIN(), LEAVING_SCALE, s.leaving_min));
    }
    if s.demo.on {
        for minutes in demo::timers(s) {
            plain(minutes as i64);
        }
    }
    // The sky switch only cuts segments: no regime term reads it.
    if let Some(m) = scene::minutes_to_switch(s.day_min) {
        plain(m as i64);
    }
    let asleep = s.is_sleeping;
    let at_cap = asleep && s.energy as i32 >= energy_cap(s);
    if at_cap {
        plain(t::GROGGY_LINE_MIN() as i64 + 1 - s.oversleep_min as i64);
    }

    // A running timer, or a counter heading for a line, that mood/health read.
    let mut feeds = |minutes: i64| best = best.min((minutes - 1).max(0));
    let hangover_mult = if asleep {
        t::HANGOVER_ASLEEP_MULT() as i64
    } else {
        1
    };
    for left in [
        ceil_div(s.hangover_min as i64, hangover_mult),
        s.injured_min as i64,
        s.decline_min as i64,
        s.groggy_min as i64,
        s.insulin_min as i64,
        s.rehab_min as i64,
    ] {
        if left > 0 {
            feeds(left);
        }
    }
    if asleep {
        feeds(t::SELF_WAKE_MIN() as i64 - s.sleep_min as i64);
        if at_cap {
            feeds(t::OVERSLEEP_WAKE_MIN() as i64 - s.oversleep_min as i64);
        }
    }
    if s.booze as i32 > needs::booze_hi() && !s.dependent && !s.demo.on {
        let line = if s.recovering_min > 0 {
            t::DEPENDENCE_RELAPSE_MIN()
        } else {
            t::DEPENDENCE_ONSET_MIN()
        };
        feeds(scaled_left(line, SLOW_SCALE, s.dependence_min));
    }
    if s.weight as i32 > lvl(t::DIABETES_WEIGHT_HI_PCT()) && !s.diabetic {
        feeds(scaled_left(
            t::DIABETES_ONSET_MIN(),
            SLOW_SCALE,
            s.diabetes_min,
        ));
    }
    if s.diabetic
        && (s.weight as i32) < lvl(t::DIABETES_WEIGHT_LO_PCT())
        && s.fitness as i32 > lvl(t::REVERSAL_FIT_PCT())
    {
        feeds(t::DIABETES_REVERSAL_MIN() as i64 - s.reversal_min as i64);
    }
    best.min(u32::MAX as i64) as u32
}

/// Whole minutes until a counter that rises by `scale` per minute reaches
/// `line_min` minutes (0 when it already has).
fn scaled_left(line_min: u16, scale: u32, counter: u32) -> i64 {
    ceil_div(
        line_min as i64 * scale as i64 - counter as i64,
        scale as i64,
    )
}

/// Longest segment (1..=`max` minutes) over which no threshold is crossed and
/// no timer expires. Pure: reads the state, changes nothing.
///
/// `max == 0` (nothing to advance) also gives 1: callers never ask for it,
/// the result is always at least 1.
pub fn seg_len(s: &GameState, max: u32) -> u32 {
    if s.phase == Phase::Gone {
        return max.max(1);
    }
    max.min(level_bound(s)).min(timer_bound(s)).max(1)
}

/// Signature of every mood and health line a term reads, plus the flags that
/// switch terms on, packed into one word. `update` halves a segment while
/// advancing it changes the signature, so mood and health never cross one of
/// their own lines inside a segment (R18, R20).
///
/// Mood lines (`mood >= line`):
/// - 80 %: top zone (brief; no term reads it yet);
/// - `REC_GOOD_MOOD_PCT`, `REC_FAIR_MOOD_PCT`: `health_recovery` tiers;
/// - `DMG_MOOD_PCT`: `health_damage` mood term (`mood < line`);
/// - `CARE_GOOD_MOOD_PCT`: `care_good_min` in `advance`.
///
/// Health lines (`health > line`, and `== 0`):
/// - 0: leaving, `health_damage` floor;
/// - `needs::HEALTH_DANGER_PCT`: the status word's danger line (shared with `status`);
/// - `DEMO_HEALTH_FLOOR_PCT`: the demo health floor; `DEMO_MED_HEALTH_PCT`
///   and `DEMO_HELP_HEALTH_PCT`: where the demo pet looks after itself;
/// - `DMG_ENERGY_HEALTH_PCT`: energy damage stops at this floor;
/// - `COLLAPSE_HEALTH_FLOOR_PCT`: collapse damage applies above it;
/// - `MOOD_HEALTH_FLOOR_PCT`: `mood_target` term `floor - health`;
/// - `sign(health - health_ceiling)`: recovery up to the ceiling, decay above.
///
/// Flags: asleep, gone, dependent, diabetic, hangover, deprived, and "target
/// clamped at 0" (`mood_penalty >= MAX`, a kink in `mood_target`).
pub fn sig(s: &GameState) -> u32 {
    let (mood, health) = (s.mood as i32, s.health as i32);
    let mood_lines = [
        lvl(needs::HAPPY_MOOD_PCT),
        lvl(t::REC_GOOD_MOOD_PCT()),
        lvl(t::REC_FAIR_MOOD_PCT()),
        lvl(t::CARE_GOOD_MOOD_PCT()),
        lvl(t::DMG_MOOD_PCT()),
    ];
    let health_lines = [
        lvl(needs::HEALTH_DANGER_PCT),
        lvl(t::DEMO_HEALTH_FLOOR_PCT()),
        // The help line is "under": `health > HELP - 1` flips at the line.
        lvl(t::DEMO_HELP_HEALTH_PCT()) - 1,
        lvl(t::DMG_ENERGY_HEALTH_PCT()),
        lvl(t::COLLAPSE_HEALTH_FLOOR_PCT()),
        lvl(t::DEMO_MED_HEALTH_PCT()),
        lvl(t::MOOD_HEALTH_FLOOR_PCT()),
    ];
    let flags = [
        s.is_sleeping,
        s.phase == Phase::Gone,
        s.dependent,
        s.diabetic,
        s.hangover_min > 0,
        s.deprived,
        health == 0,
        mood_penalty(s) >= MAX,
    ];
    let mut bits = 0u32;
    let mut push = |on: bool| bits = bits << 1 | on as u32;
    mood_lines.iter().for_each(|&l| push(mood >= l));
    health_lines.iter().for_each(|&l| push(health > l));
    flags.iter().for_each(|&f| push(f));
    // Three-way: below, on, above the ceiling.
    bits << 2 | (health - health_ceiling(s)).signum().rem_euclid(3) as u32
}
