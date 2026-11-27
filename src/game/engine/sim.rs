//! BornPets v3 simulator: a minute-stepping oracle and a delta driver that
//! play the same routine, ported from `prototypes/bornpets-simv3`.
//!
//! Host-only (`simulator` feature); nothing here is linked into firmware.
//! The oracle exists only to prove that `update()` (the delta engine) gives
//! the same pet as stepping one minute at a time. The badge never loops.

use std::cell::Cell;

use super::causes::{Cause, StrainWhy};
use super::combat::{self, Body, derive};
use super::needs::P;
use super::thresholds::{self, Mode, Thresholds};
use super::{Action, DrinkKind, FoodKind, Fought, GameState, MiniGame, PetKind, Phase};
use crate::game::pacing;

/// Engine ticks per minute (1 tick = 10 s).
const TPM: u32 = 6;
/// Ticks in a simulated day.
pub const TICKS_PER_DAY: u32 = 1440 * TPM;
/// Seed of `run_routine`; it only moves the pet's three traits.
pub const DEFAULT_SEED: u64 = 42;
/// Minute of day 0 at which the pet starts (08:00).
const START_MINUTE: u32 = 480;
/// One percent of a level, as an `i32`.
const PCT: i32 = P;

/// How the clock reaches the next routine event.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Driver {
    /// `update()` once per minute: every segment is a one-minute `advance`.
    Oracle,
    /// Like the firmware: `update()` only at routine events, at the engine's
    /// own `next_wake_tick`, and at each day end (a sampling point).
    Delta,
}

/// One player step of a routine. The engine's `Action` cannot carry a food,
/// a drink, sleep or a mini-game, so routines speak this instead; each maps
/// to one public `GameState` call and a refusal is never bypassed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Act {
    Wake,
    Sleep,
    Eat(FoodKind),
    Drink(DrinkKind),
    Play,
    /// Exercise.
    Sport,
    Wash,
    /// A mini-game of grade 1 (draw) or 2 (win).
    Mini(u8),
    /// Meeting a new mesh friend (stimulation +15).
    Friend,
    Insulin,
    /// Fight the reference pet now: refused counts as refused; else the live
    /// costs land, the prize is paid, and the seed comes from the clock and
    /// the run's seed.
    Battle,
}

/// What a routine returns for one minute: at most eight steps.
pub type Plan = heapless::Vec<Act, 8>;

/// Result of one run. Percentages are whole percent of the level.
#[derive(Clone, Copy, Debug)]
pub struct Outcome {
    pub alive: bool,
    /// End phase: a pet at health 0 shows as `Leaving` while still `alive`.
    pub phase: Phase,
    pub days: f32,
    pub avg_mood_pct: u8,
    pub end_health_pct: u8,
    pub min_health_pct: u8,
    pub end_fitness_pct: u8,
    pub end_weight_pct: u8,
    pub injuries: u16,
    /// Delta driver only (the oracle has no engine wakes): wake-ups that came
    /// from `next_wake_tick`, not from routine events or the day-end sample.
    pub engine_wakes_per_day: f32,
    /// Time shares of the mood bands (thriving 80+, fine 60 to 79, off 40 to
    /// 59, poorly 20 to 39, miserable below 20), whole percent.
    pub thriving_pct: u8,
    pub fine_pct: u8,
    pub off_pct: u8,
    pub poorly_pct: u8,
    pub miserable_pct: u8,
    /// The pet's own acts (`demo::act`) a day.
    pub self_care_per_day: f32,
    /// Causes the engine queued a day (each one is a toast).
    pub causes_per_day: f32,
    pub max_weight_pct: u8,
    pub min_fitness_pct: u8,
    /// Share of 00:00..06:00 spent asleep (clock runs only, else 0).
    pub night_asleep_pct: u8,
    /// Share of 10:00..20:00 spent asleep (clock runs only, else 0).
    pub day_asleep_pct: u8,
    /// The pet was in `Leaving` (or `Gone`) at some point.
    pub ever_leaving: bool,
    /// Diabetic at the end of the run.
    pub diabetic: bool,
    /// E-paper refreshes a day, Delta driver only (it needs the wake
    /// count): one per engine wake, plus the frames of every action, paced
    /// at the fastest the waiter allows (`MIN_FRAME_SECS`). An upper bound:
    /// the real count also stops at the sprite's frame count, which the
    /// simulator does not load. Idle frames are not counted. The Oracle has no
    /// wakes, so its figure is the action frames alone: not comparable.
    pub refreshes_per_day: f32,
    /// Fights the pet took (not refused).
    pub fights: u32,
    pub fights_won: u32,
    /// `Act::Battle` steps the engine refused.
    pub fights_refused: u32,
    /// Fights that tore a muscle (`TornMuscle(Fight)` causes).
    pub fight_injuries: u32,
    /// HAX paid as battle prizes.
    pub prizes_hax: u32,
    /// Win share against `Body::REFERENCE`: 200 seeded fights on each day's
    /// first `Act::Battle`, from the pet as it stood then, averaged. 0 when
    /// the run never fought.
    pub win_pct_vs_reference: u8,
    /// Times `scene::wake_view` changed (sky switch, floor item,
    /// cross), per day. Counted from every segment snapshot against the last
    /// one seen, so a change between updates (a wash, a player action) counts
    /// too. One change is at most one background-caused refresh.
    pub scene_changes_per_day: f32,
    /// The scene changes where nothing else visible changed since the last
    /// snapshot. Only these cost a refresh the pet screen would not make.
    pub scene_only_changes_per_day: f32,
}

/// How a run starts. `run_full` is `run_with` and `Setup::default()`.
#[derive(Clone, Copy, Debug, Default)]
pub struct Setup {
    /// Demo mode on (the pet looks after itself).
    pub demo: bool,
    /// The wall clock is set: `day_min` starts at 08:00 and follows the run.
    pub clock: bool,
    /// The pet's own care is animated (else instant with a note).
    pub animated: bool,
    /// Leave the HAX gate on (the routines never earn HAX, so by default it
    /// is switched off); the battle prizes then pay in. The pet starts with
    /// `MONEY_FLOAT_HAX`, so no purchase is refused: the prototype lets HAX
    /// go negative (spec 15.9), and a money run measures the prizes, not
    /// the price list.
    pub money: bool,
}

/// An `Outcome` plus the cost of the `next_wake_tick` calls the delta
/// driver made (`ADVANCE_CALLS` per call).
#[derive(Clone, Copy, Debug)]
pub struct Run {
    pub outcome: Outcome,
    pub wake_queries: u32,
    pub wake_calls_total: u64,
    pub wake_calls_max: u32,
}

// ---------------------------------------------------------------------------
// Probe: mood and health integrated by `update`, so the delta driver sees the
// same per-minute figures as the oracle without stepping.
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct Probe {
    /// Sum over minutes of (mood before + mood after), i.e. twice the integral.
    mood_sum2: i64,
    minutes: u64,
    min_health: u16,
    /// Minutes in each mood band, miserable first.
    band_min: [f64; 5],
    max_weight: u16,
    min_fitness: u16,
    /// Minutes of the night window (00:00..06:00) and of those asleep.
    night_min: u64,
    night_asleep: u64,
    /// The same for the day window (10:00..20:00).
    day_min: u64,
    day_asleep: u64,
    leaving: bool,
    /// Causes queued, and action ticks run, since the run began.
    causes: u32,
    action_ticks: u64,
    fights: u32,
    fights_won: u32,
    fights_refused: u32,
    fight_injuries: u32,
    prizes_hax: u32,
    /// Wins out of `WIN_SAMPLE` fights, summed over `win_days` days, and the
    /// day last sampled.
    win_sum: u32,
    win_days: u32,
    win_day: u32,
    /// The last `scene::wake_view` seen, and how often it changed.
    last_scene: Option<(u8, Rest)>,
    scene_changes: u32,
    scene_only_changes: u32,
}

/// Seeded fights per day behind `win_pct_vs_reference`.
const WIN_SAMPLE: u32 = 200;

const PROBE_NEW: Probe = Probe {
    mood_sum2: 0,
    minutes: 0,
    min_health: u16::MAX,
    band_min: [0.0; 5],
    max_weight: 0,
    min_fitness: u16::MAX,
    night_min: 0,
    night_asleep: 0,
    day_min: 0,
    day_asleep: 0,
    leaving: false,
    causes: 0,
    action_ticks: 0,
    fights: 0,
    fights_won: 0,
    fights_refused: 0,
    fight_injuries: 0,
    prizes_hax: 0,
    win_sum: 0,
    win_days: 0,
    win_day: u32::MAX,
    last_scene: None,
    scene_changes: 0,
    scene_only_changes: 0,
};

/// The figures `note_segment` reads off the pet at one end of a segment.
#[derive(Clone, Copy)]
pub(super) struct Snap {
    mood: u16,
    health: u16,
    weight: u16,
    fitness: u16,
    sleeping: bool,
    day_min: Option<u16>,
    phase: Phase,
    /// The scene byte and the rest of the view; `None` on a muted look-ahead
    /// clone, which `note_segment` never reads.
    view: Option<(u8, Rest)>,
}

/// Everything in `Visible` but the scene byte.
type Rest = (
    super::status::StatusLine,
    super::DisplayAnim,
    bool,
    Phase,
    Option<super::Action>,
    u8,
    u8,
);

impl Snap {
    pub(super) fn of(s: &GameState) -> Self {
        Snap {
            mood: s.mood,
            health: s.health,
            weight: s.weight,
            fitness: s.fitness,
            sleeping: s.is_sleeping,
            day_min: s.day_min,
            phase: s.phase,
            view: (!muted()).then(|| {
                let (a, b, c, d, e, f, g, _) = s.visible();
                (super::scene::wake_view(s), (a, b, c, d, e, f, g))
            }),
        }
    }
}

/// Minutes of `[start, start + dt)` (a minute of the day, wrapping) that fall
/// in the daily window `[lo, hi)`.
fn window_overlap(start: u32, dt: u32, lo: u32, hi: u32) -> u32 {
    let end = start + dt;
    (0..=end / 1440)
        .map(|k| {
            let (a, b) = (lo + 1440 * k, hi + 1440 * k);
            end.min(b).saturating_sub(start.max(a))
        })
        .sum()
}

/// Minutes a mood moving linearly from `m0` to `m1` over `dt` minutes spends
/// in each band, miserable first.
fn band_minutes(m0: u16, m1: u16, dt: u32) -> [f64; 5] {
    let line = |pct: i32| (pct * PCT) as f64;
    let edges = [f64::MIN, line(20), line(40), line(60), line(80), f64::MAX];
    let (a, b) = (m0 as f64, m1 as f64);
    let mut out = [0.0; 5];
    for (i, w) in edges.windows(2).enumerate() {
        let frac = if a == b {
            (a >= w[0] && a < w[1]) as u8 as f64
        } else {
            let (ta, tb) = ((w[0] - a) / (b - a), (w[1] - a) / (b - a));
            (ta.max(tb).min(1.0) - ta.min(tb).max(0.0)).max(0.0)
        };
        out[i] = frac * dt as f64;
    }
    out
}

// Unlike `ADVANCE_CALLS`, this records only the pet's own life: clones made
// for a preview or a look-ahead run muted (`mute`). Only `run_with` resets
// and reads it.
thread_local! {
    static PROBE: Cell<Probe> = const { Cell::new(PROBE_NEW) };
    static SELF_CARE: Cell<u32> = const { Cell::new(0) };
    /// Nonzero while a throwaway clone runs (a preview, the wake look-ahead,
    /// a trial advance): what it does is not the pet's life, so nothing is
    /// counted. `ADVANCE_CALLS` still counts it; that is the wake's cost.
    static MUTED: Cell<u32> = const { Cell::new(0) };
}

/// Keeps the counters silent until dropped. Taken where a clone is made.
pub(super) struct Mute;

pub(super) fn mute() -> Mute {
    MUTED.with(|m| m.set(m.get() + 1));
    Mute
}

impl Drop for Mute {
    fn drop(&mut self) {
        MUTED.with(|m| m.set(m.get() - 1));
    }
}

fn muted() -> bool {
    MUTED.with(|m| m.get() > 0)
}

/// Called by `demo::act`: one self-care act happened.
pub(super) fn note_self_care() {
    if muted() {
        return;
    }
    SELF_CARE.with(|c| c.set(c.get() + 1));
}

/// Self-care acts counted on this thread since it began.
pub fn self_care_acts() -> u32 {
    SELF_CARE.with(|c| c.get())
}

/// Called by `CauseQueue::push`: the engine queued one cause.
pub(super) fn note_cause() {
    if muted() {
        return;
    }
    PROBE.with(|p| {
        let mut v = p.get();
        v.causes += 1;
        p.set(v);
    });
}

/// Called when a running action spends `ticks` (the player's, or the pet's
/// own animated one). Instant care spends none.
pub(super) fn note_action_ticks(ticks: u32) {
    if muted() {
        return;
    }
    PROBE.with(|p| {
        let mut v = p.get();
        v.action_ticks += ticks as u64;
        p.set(v);
    });
}

/// Called by `GameState::update` after each closed-form segment of `dt`
/// minutes: everything measured is linear inside one, so ends are enough.
/// Sleep and the window shares go by the state at the segment's start (the
/// solver ends a segment where either changes).
pub(super) fn note_segment(before: Snap, after: Snap, dt: u32) {
    if muted() {
        return;
    }
    PROBE.with(|p| {
        let mut v = p.get();
        v.mood_sum2 += (before.mood as i64 + after.mood as i64) * dt as i64;
        v.minutes += dt as u64;
        for (scene, rest) in [before.view, after.view].into_iter().flatten() {
            if let Some((last, last_rest)) = v.last_scene
                && last != scene
            {
                v.scene_changes += 1;
                v.scene_only_changes += (last_rest == rest) as u32;
            }
            v.last_scene = Some((scene, rest));
        }
        v.min_health = v.min_health.min(before.health).min(after.health);
        for (acc, m) in v
            .band_min
            .iter_mut()
            .zip(band_minutes(before.mood, after.mood, dt))
        {
            *acc += m;
        }
        v.max_weight = v.max_weight.max(before.weight).max(after.weight);
        v.min_fitness = v.min_fitness.min(before.fitness).min(after.fitness);
        v.leaving |= matches!(after.phase, Phase::Leaving | Phase::Gone);
        if let Some(start) = before.day_min {
            let night = window_overlap(start as u32, dt, 0, 360) as u64;
            let day = window_overlap(start as u32, dt, 600, 1200) as u64;
            v.night_min += night;
            v.day_min += day;
            if before.sleeping {
                v.night_asleep += night;
                v.day_asleep += day;
            }
        }
        p.set(v);
    });
}

// ---------------------------------------------------------------------------
// Threshold install (R14)
// ---------------------------------------------------------------------------

/// Install a preset (or custom `values`) for this process. The engine's
/// table is write-once, so this takes effect only on the first call and only
/// before any pet runs: call it first thing in `main` or in a test binary of
/// its own. Returns whether this call was the one that installed.
pub fn install_preset(values: Thresholds, mode: Mode, custom: bool) -> bool {
    static ONCE: std::sync::Once = std::sync::Once::new();
    let mut first = false;
    ONCE.call_once(|| {
        thresholds::install(values, mode, custom);
        first = true;
    });
    first
}

// ---------------------------------------------------------------------------
// Runner
// ---------------------------------------------------------------------------

type Routine<'a> = &'a dyn Fn(u32, u32, &GameState) -> Plan;

/// Run `routine` for `days` days against a fresh egg (`DEFAULT_SEED`).
pub fn run_routine(routine: Routine, days: u32, driver: Driver) -> Outcome {
    run_full(routine, days, driver, DEFAULT_SEED).outcome
}

/// Does the routine have an event at this minute, whatever the pet's state?
struct Probes {
    lo: GameState,
    hi: GameState,
    mid: GameState,
}

impl Probes {
    fn new() -> Self {
        let mut lo = GameState::new_egg(0, PetKind::DEFAULT);
        lo.fullness = 0;
        lo.hydration = 0;
        lo.energy = u16::MAX;
        lo.fun = 0;
        lo.dirt = u16::MAX;
        lo.strain = 0;
        lo.booze = 0;
        lo.health = 0;
        lo.fitness = 0;
        let mut hi = GameState::new_egg(0, PetKind::DEFAULT);
        hi.fullness = u16::MAX;
        hi.hydration = u16::MAX;
        hi.dirt = 0;
        Probes {
            lo,
            hi,
            mid: GameState::new_egg(0, PetKind::DEFAULT),
        }
    }

    fn fires(&self, r: Routine, d: u32, m: u32) -> bool {
        !r(d, m, &self.lo).is_empty()
            || !r(d, m, &self.hi).is_empty()
            || !r(d, m, &self.mid).is_empty()
    }
}

/// Start one step. `true` when it started (a timed action) or happened.
fn perform(s: &mut GameState, a: Act, seed: u64) -> bool {
    match a {
        Act::Wake => s.wake(),
        Act::Sleep => s.sleep(),
        Act::Eat(f) => s.start_choice(Action::Feed, Some(f), None),
        Act::Drink(d) => s.start_choice(Action::Drink, None, Some(d)),
        Act::Play => s.start_choice(Action::Play, None, None),
        Act::Sport => s.start_choice(Action::Exercise, None, None),
        Act::Wash => s.start_choice(Action::Wash, None, None),
        Act::Insulin => s.start_choice(Action::Medicate, None, None),
        Act::Friend => {
            s.friend_boost(true);
            true
        }
        Act::Battle => battle(s, seed),
        Act::Mini(g) => {
            if s.game_refusal(MiniGame::TicTacToe).is_some() {
                return false;
            }
            s.award_inspiration(MiniGame::TicTacToe, g.min(2));
            true
        }
    }
}

/// One `Act::Battle`: the first of a day also samples the win share; a
/// refusal is counted; else the fight against the reference pet is played
/// with its live costs and prize. `true` when it was fought.
fn battle(s: &mut GameState, seed: u64) -> bool {
    let reference = derive(&Body::REFERENCE);
    let day = s.last_update_tick / TICKS_PER_DAY;
    let mine = s.snapshot();
    PROBE.with(|p| {
        let mut v = p.get();
        if v.win_day != day {
            v.win_day = day;
            let wins = (0..WIN_SAMPLE)
                .filter(|&k| combat::fight(combat::chance_seed(k), &mine, &reference).a_won)
                .count() as u32;
            v.win_sum += wins;
            v.win_days += 1;
        }
        p.set(v);
    });
    if s.battle_refusal().is_some() {
        PROBE.with(|p| {
            let mut v = p.get();
            v.fights_refused += 1;
            p.set(v);
        });
        return false;
    }
    let fight_seed = crate::platform::xorshift32((s.last_update_tick ^ seed as u32) | 1);
    let o = combat::fight(fight_seed, &mine, &reference);
    let (prize, queued) = s.queued_by(|s| {
        s.fight_over(Fought {
            won: o.a_won,
            hp_left_pct: o.a_hp_pct,
            costs: true,
            paid: true,
        })
    });
    let torn = queued
        .iter()
        .any(|c| matches!(c, Cause::TornMuscle(StrainWhy::Fight)));
    PROBE.with(|p| {
        let mut v = p.get();
        v.fights += 1;
        v.fights_won += o.a_won as u32;
        v.fight_injuries += torn as u32;
        v.prizes_hax += prize;
        p.set(v);
    });
    true
}

/// `update` to `to`, after telling the engine the wall clock the way the
/// badge does before each update (tick 0 is 00:00 of day 0).
fn step(s: &mut GameState, to: u32, clock: bool) {
    if clock {
        s.sync_clock(to, Some(((to / TPM) % 1440) as u16));
    }
    s.update(to);
}

/// Play every step of a plan in order. A timed action runs to its end (the
/// action ticks age no need) before the next step; a refused step is skipped.
fn play_plan(s: &mut GameState, plan: &Plan, injuries: &mut u16, clock: bool, seed: u64) {
    for &a in plan.iter() {
        if s.phase == Phase::Gone {
            return;
        }
        if !perform(s, a, seed) {
            continue;
        }
        if s.active_action.is_some() {
            let was_injured = s.injured_min > 0;
            step(
                s,
                s.last_update_tick + s.action_ticks_remaining as u32,
                clock,
            );
            if !was_injured && s.injured_min > 0 {
                *injuries += 1;
            }
        }
    }
}

/// `run_routine` with a seed, plus the wake-cost figures.
pub fn run_full(routine: Routine, days: u32, driver: Driver, seed: u64) -> Run {
    run_with(routine, days, driver, seed, Setup::default())
}

/// `run_full` with a `Setup`: demo mode, the clock, the care mode.
pub fn run_with(routine: Routine, days: u32, driver: Driver, seed: u64, setup: Setup) -> Run {
    run_pet(routine, days, driver, seed, setup, false)
}

/// HAX a money run starts with: more than any routine spends in a run.
pub const MONEY_FLOAT_HAX: u32 = 1_000_000;

/// A trait at 50 % (`trait_pct`).
const AVERAGE_TRAIT: u16 = 32768;

/// `run_with` on the prototype's pet: vitality, curiosity and resilience at
/// 50 %, as its `Cond` has them, so the seed moves only the fights. The
/// battle lifestyles of spec 18.4 were measured on that pet. The start
/// health still follows the seed's vitality (`new_egg`).
pub fn run_average(routine: Routine, days: u32, driver: Driver, seed: u64, setup: Setup) -> Run {
    run_pet(routine, days, driver, seed, setup, true)
}

fn run_pet(
    routine: Routine,
    days: u32,
    driver: Driver,
    seed: u64,
    setup: Setup,
    average: bool,
) -> Run {
    PROBE.with(|p| p.set(PROBE_NEW));
    let care_before = self_care_acts();
    let probes = Probes::new();
    let mut s = GameState::new_egg(seed, PetKind::DEFAULT);
    if average {
        s.vitality = AVERAGE_TRAIT;
        s.curiosity = AVERAGE_TRAIT;
        s.resilience = AVERAGE_TRAIT;
    }
    // The routines earn no HAX but battle prizes; with the gate on they would
    // starve, so only a money run keeps it, with a float to buy from.
    if setup.money {
        s.money = MONEY_FLOAT_HAX;
    } else {
        s.money_enabled = false;
    }
    // The egg hatches and the pet is fresh at 08:00 on day 0, as in the
    // prototype (hatching ticks past the countdown age nothing).
    s.update(START_MINUTE * TPM);
    if setup.clock {
        s.sync_clock(s.last_update_tick, Some(START_MINUTE as u16));
    }
    if setup.animated {
        s.set_demo_care(true);
    }
    if setup.demo {
        s.set_demo(true).expect("a fresh pet takes demo mode");
    }

    let (mut injuries, mut wakes) = (0u16, 0u32);
    let (mut queries, mut calls_total, mut calls_max) = (0u32, 0u64, 0u32);

    'days: for d in 0..days {
        let first = if d == 0 { START_MINUTE } else { 0 };
        let day_end = (d + 1) * TICKS_PER_DAY;
        match driver {
            Driver::Oracle => {
                for m in first..1440 {
                    step(&mut s, d * TICKS_PER_DAY + m * TPM, setup.clock);
                    if s.phase == Phase::Gone {
                        break 'days;
                    }
                    if probes.fires(routine, d, m) {
                        let plan = routine(d, m, &s);
                        play_plan(&mut s, &plan, &mut injuries, setup.clock, seed);
                    }
                }
                step(&mut s, day_end, setup.clock);
            }
            Driver::Delta => {
                let mut m = first;
                loop {
                    // The next routine event today, or the day end.
                    while m < 1440 && !probes.fires(routine, d, m) {
                        m += 1;
                    }
                    let at = if m < 1440 {
                        d * TICKS_PER_DAY + m * TPM
                    } else {
                        day_end
                    };
                    while s.last_update_tick < at && s.phase != Phase::Gone {
                        let before = super::ADVANCE_CALLS.with(|c| c.get());
                        let wake = s.next_wake_tick();
                        let cost = super::ADVANCE_CALLS.with(|c| c.get()) - before;
                        queries += 1;
                        calls_total += cost as u64;
                        calls_max = calls_max.max(cost);
                        if wake < at {
                            wakes += 1;
                        }
                        step(&mut s, wake.min(at), setup.clock);
                    }
                    if m >= 1440 || s.phase == Phase::Gone {
                        break;
                    }
                    let plan = routine(d, m, &s);
                    play_plan(&mut s, &plan, &mut injuries, setup.clock, seed);
                    m += 1;
                }
            }
        }
        if s.phase == Phase::Gone {
            break 'days;
        }
    }

    let probe = PROBE.with(|p| p.get());
    let days_run = (s.age_ticks.saturating_sub(START_MINUTE * TPM) as f32 / TICKS_PER_DAY as f32)
        .max(f32::MIN_POSITIVE);
    let pct = |v: u16| (v as i32 / PCT) as u8;
    let avg = if probe.minutes == 0 {
        0
    } else {
        (probe.mood_sum2 / (2 * probe.minutes as i64) / PCT as i64) as u8
    };
    let band_total: f64 = probe.band_min.iter().sum();
    let share = |i: usize| {
        if band_total > 0.0 {
            (probe.band_min[i] / band_total * 100.0).round() as u8
        } else {
            0
        }
    };
    let window_pct = |asleep: u64, total: u64| {
        if total == 0 {
            0
        } else {
            (asleep as f64 / total as f64 * 100.0).round() as u8
        }
    };
    Run {
        outcome: Outcome {
            alive: s.phase != Phase::Gone,
            phase: s.phase,
            days: s.age_ticks as f32 / TICKS_PER_DAY as f32,
            avg_mood_pct: avg,
            end_health_pct: pct(s.health),
            min_health_pct: pct(probe.min_health.min(s.health)),
            end_fitness_pct: pct(s.fitness),
            end_weight_pct: pct(s.weight),
            injuries,
            engine_wakes_per_day: wakes as f32 / days_run,
            thriving_pct: share(4),
            fine_pct: share(3),
            off_pct: share(2),
            poorly_pct: share(1),
            miserable_pct: share(0),
            self_care_per_day: (self_care_acts() - care_before) as f32 / days_run,
            causes_per_day: probe.causes as f32 / days_run,
            max_weight_pct: pct(probe.max_weight.max(s.weight)),
            min_fitness_pct: pct(probe.min_fitness.min(s.fitness)),
            night_asleep_pct: window_pct(probe.night_asleep, probe.night_min),
            day_asleep_pct: window_pct(probe.day_asleep, probe.day_min),
            ever_leaving: probe.leaving || matches!(s.phase, Phase::Leaving | Phase::Gone),
            diabetic: s.diabetic,
            refreshes_per_day: (wakes as f64
                + probe.action_ticks as f64 * pacing::SECS_PER_TICK as f64
                    / pacing::MIN_FRAME_SECS as f64) as f32
                / days_run,
            fights: probe.fights,
            fights_won: probe.fights_won,
            fights_refused: probe.fights_refused,
            fight_injuries: probe.fight_injuries,
            prizes_hax: probe.prizes_hax,
            win_pct_vs_reference: if probe.win_days == 0 {
                0
            } else {
                (probe.win_sum as f64 * 100.0 / (WIN_SAMPLE * probe.win_days) as f64).round() as u8
            },
            scene_changes_per_day: probe.scene_changes as f32 / days_run,
            scene_only_changes_per_day: probe.scene_only_changes as f32 / days_run,
        },
        wake_queries: queries,
        wake_calls_total: calls_total,
        wake_calls_max: calls_max,
    }
}

// ---------------------------------------------------------------------------
// Routines (ported from prototypes/bornpets-simv3/src/main.rs)
// ---------------------------------------------------------------------------

const fn at(m: u32, h: u32, mi: u32) -> bool {
    m == h * 60 + mi
}

fn plan(acts: &[Act]) -> Plan {
    let mut v = Plan::new();
    for &a in acts.iter().take(Plan::new().capacity()) {
        let _ = v.push(a);
    }
    v
}

fn pct_lvl(p: i32) -> u16 {
    (p * PCT) as u16
}

/// The player's habits, applied to every event of a routine that has them:
/// water at a check-in (one under 55 % hydration, two under 25 %, before a
/// trailing Sleep), and with `weight_habit` richer food when thin and lighter
/// food when heavy (spec 15.8). A diabetic pet gets insulin first when the
/// last dose is nearly worn off. Anything past eight steps (never reached by the ported routines) is dropped.
fn habits(mut acts: Plan, s: &GameState, weight_habit: bool) -> Plan {
    if acts.is_empty() {
        return acts;
    }
    if acts.as_slice() != [Act::Sleep] && s.hydration < pct_lvl(55) {
        let w = if acts.last() == Some(&Act::Sleep) {
            acts.len() - 1
        } else {
            acts.len()
        };
        let n = if s.hydration < pct_lvl(25) { 2 } else { 1 };
        for _ in 0..n {
            let _ = acts.insert(w, Act::Drink(DrinkKind::Water));
        }
        // insert fails only when full; the extra water is then dropped
    }
    if weight_habit {
        for a in acts.iter_mut() {
            if let Act::Eat(f) = a {
                if s.weight < pct_lvl(42) && matches!(f, FoodKind::Salad | FoodKind::Apple) {
                    *f = FoodKind::Pizza;
                } else if s.weight > pct_lvl(60)
                    && matches!(f, FoodKind::Pizza | FoodKind::Frikandel)
                {
                    *f = FoodKind::Apple;
                } else if s.weight > pct_lvl(60) && *f == FoodKind::Cake {
                    *f = FoodKind::Salad;
                }
            }
        }
    }
    if s.diabetic && s.insulin_min as u32 <= thresholds::DOSE_INSULIN_WINDOW_MIN() as u32 {
        if acts.is_full() {
            acts.pop();
        }
        let _ = acts.insert(0, Act::Insulin);
    }
    acts
}

fn balanced_plan(m: u32, s: &GameState) -> Plan {
    use Act::*;
    use FoodKind::*;
    if at(m, 8, 0) {
        plan(&[Wake, Eat(Apple)])
    } else if at(m, 10, 30) {
        plan(&[Play])
    } else if at(m, 18, 0) {
        plan(&[Wash])
    } else if at(m, 13, 0) {
        plan(&[Eat(Apple)])
    } else if at(m, 15, 30) {
        plan(&[Sport])
    } else if at(m, 18, 30) {
        plan(&[Eat(Pizza)])
    } else if at(m, 20, 30) {
        plan(&[Mini(2), Drink(DrinkKind::Beer)])
    } else if at(m, 23, 30) {
        if s.fullness < pct_lvl(55) {
            plan(&[Eat(Salad), Sleep])
        } else {
            plan(&[Sleep])
        }
    } else {
        Plan::new()
    }
}

/// Three meals, a walk, a game, a beer and a decent bedtime.
pub fn balanced(_d: u32, m: u32, s: &GameState) -> Plan {
    habits(balanced_plan(m, s), s, true)
}

/// Two exercise sessions a day and lighter food.
pub fn sporty(_d: u32, m: u32, s: &GameState) -> Plan {
    habits(sporty_plan(m, s), s, true)
}

fn sporty_plan(m: u32, s: &GameState) -> Plan {
    use Act::*;
    use FoodKind::*;
    if at(m, 7, 30) {
        plan(&[Wake, Eat(Apple), Eat(Salad)])
    } else if at(m, 9, 30) {
        plan(&[Sport])
    } else if at(m, 12, 30) {
        if s.dirt > pct_lvl(45) {
            plan(&[Wash, Eat(Pizza)])
        } else {
            plan(&[Eat(Pizza)])
        }
    } else if at(m, 16, 30) {
        plan(&[Sport])
    } else if at(m, 18, 30) {
        if s.fullness < pct_lvl(45) {
            plan(&[Eat(Apple), Eat(Salad)])
        } else {
            plan(&[Eat(Apple)])
        }
    } else if at(m, 17, 0) {
        plan(&[Wash])
    } else if at(m, 20, 30) {
        plan(&[Play])
    } else if at(m, 22, 30) {
        plan(&[Sleep])
    } else {
        Plan::new()
    }
}

/// Sleeps late, lives on pizza, cake and coffee. No weight habit.
pub fn lazy_treats(d: u32, m: u32, s: &GameState) -> Plan {
    use Act::*;
    use FoodKind::*;
    let p = if at(m, 10, 0) {
        plan(&[Wake, Eat(Pizza)])
    } else if at(m, 14, 0) {
        plan(&[Eat(Cake), Drink(DrinkKind::Coffee)])
    } else if at(m, 16, 0) {
        if d % 2 == 0 {
            plan(&[Sport])
        } else {
            plan(&[Play])
        }
    } else if at(m, 19, 0) {
        plan(&[Eat(if s.fullness < pct_lvl(40) {
            Pizza
        } else {
            Frikandel
        })])
    } else if at(m, 21, 30) {
        if s.dirt > pct_lvl(55) {
            plan(&[Wash, Mini(1)])
        } else {
            plan(&[Mini(1)])
        }
    } else if at(m, 23, 59) {
        if s.fullness < pct_lvl(50) {
            plan(&[Eat(Apple)])
        } else {
            Plan::new()
        }
    } else if at(m, 1, 0) {
        plan(&[Sleep])
    } else {
        Plan::new()
    };
    habits(p, s, false)
}

/// Late nights, beer and friends. No weight habit.
pub fn party(d: u32, m: u32, s: &GameState) -> Plan {
    use Act::*;
    use FoodKind::*;
    let beer = Drink(DrinkKind::Beer);
    let water = Drink(DrinkKind::Water);
    let p = if at(m, 11, 0) {
        if s.dirt > pct_lvl(50) {
            plan(&[Wake, Wash, water, Eat(Frikandel)])
        } else {
            plan(&[Wake, water, Eat(Frikandel)])
        }
    } else if at(m, 14, 0) {
        if d % 2 == 0 {
            plan(&[Sport])
        } else {
            plan(&[Eat(Apple)])
        }
    } else if at(m, 16, 30) {
        plan(&[beer, Friend])
    } else if at(m, 19, 0) {
        plan(&[Eat(Pizza)])
    } else if at(m, 21, 0) {
        plan(&[beer, Friend])
    } else if at(m, 23, 30) {
        plan(&[Mini(2)])
    } else if at(m, 1, 30) {
        if s.fullness < pct_lvl(50) {
            plan(&[water, Eat(Apple)])
        } else {
            plan(&[water])
        }
    } else if at(m, 3, 0) {
        plan(&[Sleep])
    } else {
        Plan::new()
    };
    habits(p, s, false)
}

/// Balanced, but woken after five hours every night.
pub fn short_sleeper(_d: u32, m: u32, s: &GameState) -> Plan {
    if at(m, 4, 30) {
        return habits(plan(&[Act::Wake]), s, true);
    }
    let mut p = balanced_plan(m, s);
    p.retain(|a| *a != Act::Wake);
    habits(p, s, true)
}

fn awake_hours(m: u32) -> bool {
    m >= 8 * 60
}

/// Balanced without exercise and beer, plus `a` every `every` minutes awake.
fn spam_p(m: u32, every: u32, a: Act, s: &GameState) -> Plan {
    let mut v = balanced_plan(m, s);
    v.retain(|x| *x != Act::Sport && !matches!(x, Act::Drink(DrinkKind::Beer)));
    if awake_hours(m) && m % every == 0 {
        let _ = v.push(a);
    }
    v
}

/// Water every half hour on top of a sensible day.
pub fn spam_water(_d: u32, m: u32, s: &GameState) -> Plan {
    habits(spam_p(m, 30, Act::Drink(DrinkKind::Water), s), s, true)
}

/// A bath every half hour on top of a sensible day.
pub fn spam_wash(_d: u32, m: u32, s: &GameState) -> Plan {
    habits(spam_p(m, 30, Act::Wash, s), s, true)
}

/// One look a day at noon: a pizza and a play. No weight habit.
pub fn once_a_day(_d: u32, m: u32, s: &GameState) -> Plan {
    let p = if at(m, 12, 0) {
        plan(&[Act::Eat(FoodKind::Pizza), Act::Play])
    } else {
        Plan::new()
    };
    habits(p, s, false)
}

/// Never looks at the pet.
pub fn absent(_d: u32, _m: u32, _s: &GameState) -> Plan {
    Plan::new()
}

/// A helper who looks in once a day, at 18:00: an apple, a glass of water, a
/// run and a wash. A demo pet never exercises by itself; this is the part
/// only a player can do.
pub fn helper(_d: u32, m: u32, _s: &GameState) -> Plan {
    use Act::*;
    if at(m, 18, 0) {
        plan(&[Eat(FoodKind::Apple), Drink(DrinkKind::Water), Sport, Wash])
    } else {
        Plan::new()
    }
}

// ---------------------------------------------------------------------------
// Battle lifestyles (plan 5, spec 18.4)
// ---------------------------------------------------------------------------

/// `p` with a fight last: as in the prototype, the fight comes after the
/// steps of its minute (a 20:30 wash, a 22:30 bedtime). A full plan drops
/// its last step for it.
fn then_fight(mut p: Plan) -> Plan {
    if p.is_full() {
        p.pop();
    }
    let _ = p.push(Act::Battle);
    p
}

/// Minutes from 20:00 on at which the six-a-day lifestyles fight.
fn six_times(m: u32) -> bool {
    (20 * 60..=22 * 60 + 30).contains(&m) && m % 30 == 0
}

/// The prototype's fit fighter: one session of sport, clean food by
/// appetite, a game in the afternoon, a wash after the fight. It has the
/// weight habit.
fn fighter_plan(m: u32, s: &GameState) -> Plan {
    use Act::*;
    use FoodKind::*;
    if at(m, 7, 30) {
        if s.fullness < pct_lvl(40) {
            plan(&[Wake, Eat(Apple), Eat(Salad)])
        } else {
            plan(&[Wake, Eat(Apple)])
        }
    } else if at(m, 9, 30) {
        plan(&[Sport])
    } else if at(m, 12, 30) {
        let f = if s.fullness < pct_lvl(45) {
            Pizza
        } else {
            Apple
        };
        if s.dirt > pct_lvl(45) {
            plan(&[Wash, Eat(f)])
        } else {
            plan(&[Eat(f)])
        }
    } else if at(m, 15, 30) {
        plan(&[Play])
    } else if at(m, 18, 30) {
        if s.fullness < pct_lvl(45) {
            plan(&[Eat(Apple), Eat(Salad)])
        } else if s.fullness < pct_lvl(65) {
            plan(&[Eat(Apple)])
        } else {
            plan(&[Play])
        }
    } else if at(m, 20, 30) {
        plan(&[Wash])
    } else if at(m, 22, 30) {
        plan(&[Sleep])
    } else {
        Plan::new()
    }
}

/// The fit fighter, and one fight a day at 20:00.
pub fn fighter(_d: u32, m: u32, s: &GameState) -> Plan {
    let p = habits(fighter_plan(m, s), s, true);
    if at(m, 20, 0) { then_fight(p) } else { p }
}

/// The fit fighter, and a fight every half hour from 20:00 to 22:30.
pub fn fighter_six(_d: u32, m: u32, s: &GameState) -> Plan {
    let p = habits(fighter_plan(m, s), s, true);
    if six_times(m) { then_fight(p) } else { p }
}

/// The prototype's brawler: no sport, rich food while under 76 % weight,
/// cake or coffee, two beers at 19:30 for the fight. No weight habit.
fn brawler_plan(d: u32, m: u32, s: &GameState) -> Plan {
    use Act::*;
    use FoodKind::*;
    let rich = if s.weight < pct_lvl(76) { Pizza } else { Apple };
    let beer = Drink(DrinkKind::Beer);
    if at(m, 10, 0) {
        if s.dirt > pct_lvl(50) {
            plan(&[Wake, Wash, Eat(rich)])
        } else {
            plan(&[Wake, Eat(rich)])
        }
    } else if at(m, 14, 0) {
        if d % 2 == 0 && s.weight < pct_lvl(76) {
            plan(&[Eat(Cake)])
        } else {
            plan(&[Drink(DrinkKind::Coffee)])
        }
    } else if at(m, 17, 0) {
        plan(&[Play])
    } else if at(m, 18, 30) {
        plan(&[Eat(if s.fullness < pct_lvl(40) {
            rich
        } else {
            Apple
        })])
    } else if at(m, 19, 30) {
        plan(&[beer, beer])
    } else if at(m, 22, 0) {
        plan(&[Drink(DrinkKind::Water), Mini(1)])
    } else if at(m, 1, 0) {
        if s.fullness < pct_lvl(45) {
            plan(&[Eat(Apple), Sleep])
        } else {
            plan(&[Sleep])
        }
    } else {
        Plan::new()
    }
}

/// The brawler, and a fight at 20:00.
pub fn brawler(d: u32, m: u32, s: &GameState) -> Plan {
    let p = habits(brawler_plan(d, m, s), s, false);
    if at(m, 20, 0) { then_fight(p) } else { p }
}

/// The brawler, and a fight every half hour from 20:00 to 22:30.
pub fn brawler_six(d: u32, m: u32, s: &GameState) -> Plan {
    let p = habits(brawler_plan(d, m, s), s, false);
    if six_times(m) { then_fight(p) } else { p }
}

/// The prototype's sumo: sport every morning, rich food and cake while
/// under 74 % weight, and a fight at 20:00. No weight habit.
pub fn sumo(_d: u32, m: u32, s: &GameState) -> Plan {
    use Act::*;
    use FoodKind::*;
    let light = s.weight < pct_lvl(74);
    let rich = if light { Pizza } else { Apple };
    let p = if at(m, 7, 30) {
        plan(&[Wake, Eat(rich)])
    } else if at(m, 9, 30) {
        plan(&[Sport])
    } else if at(m, 12, 30) {
        if s.dirt > pct_lvl(45) {
            plan(&[Wash, Eat(rich)])
        } else {
            plan(&[Eat(rich)])
        }
    } else if at(m, 15, 30) {
        if light {
            plan(&[Eat(Cake), Play])
        } else {
            plan(&[Play])
        }
    } else if at(m, 18, 30) {
        if s.fullness < pct_lvl(60) {
            plan(&[Eat(rich)])
        } else {
            Plan::new()
        }
    } else if at(m, 20, 30) {
        plan(&[Wash])
    } else if at(m, 22, 30) {
        plan(&[Sleep])
    } else {
        Plan::new()
    };
    let p = habits(p, s, false);
    if at(m, 20, 0) { then_fight(p) } else { p }
}
