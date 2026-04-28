//! Game balance constants — rates, cooldowns, and thresholds.
//!
//! All stat values are u16 (0–65535).  655 ≈ 1% on a 0–100 scale.
//! Time is measured in ticks (1 tick = 10 real seconds).

/// Maximum stat value (u16::MAX).
pub const STAT_MAX: u16 = 65535;

/// Conversion: 1 "spec unit" (1% on 0–100 scale) in u16 space.
pub const UNIT: u16 = 655;

// ---------------------------------------------------------------------------
// Hunger
// ---------------------------------------------------------------------------

/// Hunger increase per tick (fills in ~20 hours).
pub const HUNGER_RATE: u16 = 9;
/// Extra hunger per tick when miserable ≥ 70%.
///
/// Tuned to 0 because the new severe / leaving floor on `miserable`
/// keeps `miserable_high` triggered whenever stats are critical, and
/// any non-zero boost pushed several long-sleep simulator profiles
/// (feed_and_sleep, feed_sleep_heal, perfect_no_rest) more than 10%
/// off baseline lifetimes.  The hunger feedback loop is now driven
/// purely by HUNGER_RATE for any miserable level.
pub const HUNGER_MISERABLE_BOOST: u16 = 0;

// ---------------------------------------------------------------------------
// Tired
// ---------------------------------------------------------------------------

/// Tired increase per tick (fills in ~13.3 hours).
pub const TIRED_RATE: u16 = 14;
/// Extra tired per tick when miserable ≥ 70%.
///
/// Tuned to 0 alongside `HUNGER_MISERABLE_BOOST` for the same reason —
/// the severe / leaving floor pushes `miserable` over the trigger
/// threshold whenever stats are critical, and reintroducing the boost
/// pulls long-sleep simulator profiles outside the ±10 % band against
/// baseline lifetimes.
pub const TIRED_MISERABLE_BOOST: u16 = 0;
/// Passive tired recovery amount (while awake).
pub const TIRED_PASSIVE_RECOVERY: u16 = 655; // 1 unit
/// Passive recovery interval (ticks).
pub const TIRED_PASSIVE_INTERVAL: u32 = 120;

/// Sleep recovery tiers (per tick):
pub const SLEEP_RECOVERY_SLOW: u16 = 3275; // tired ≥ 76%
pub const SLEEP_RECOVERY_MEDIUM: u16 = 6550; // tired ≥ 46%
pub const SLEEP_RECOVERY_FAST: u16 = 9825; // tired < 46%
/// Tired threshold for slow sleep recovery.
pub const SLEEP_TIER_SLOW: u16 = 49807; // 76%
/// Tired threshold for medium sleep recovery.
pub const SLEEP_TIER_MEDIUM: u16 = 30145; // 46%

// ---------------------------------------------------------------------------
// Drained
// ---------------------------------------------------------------------------

/// Drained increase amount (interval-based).
pub const DRAINED_AMOUNT: u16 = 655; // 1 unit
/// Normal drained interval (ticks).
pub const DRAINED_INTERVAL: u32 = 90;
/// Accelerated drained interval when miserable ≥ 80%.
pub const DRAINED_INTERVAL_MISERABLE: u32 = 30;
/// Drained recovery per tick during sleep.
pub const DRAINED_SLEEP_RECOVERY: u16 = 655;

// ---------------------------------------------------------------------------
// Sick
// ---------------------------------------------------------------------------

/// Baseline sick increase per tick (~7.6 days to fill).
pub const SICK_RATE: u16 = 1;
/// Sick condition decay per tick (when hunger/tired/drained are bad).
pub const SICK_CONDITION_RATE: u16 = 655; // 1 unit
/// Sick condition rate when miserable ≥ 70%.
pub const SICK_CONDITION_MISERABLE_RATE: u16 = 1310; // 2 units

/// Hunger threshold that triggers sick condition decay (60%).
pub const SICK_TRIGGER_HUNGER: u16 = 39321;
/// Tired threshold that triggers sick condition decay (75%).
pub const SICK_TRIGGER_TIRED: u16 = 49151;
/// Drained threshold that triggers sick condition decay (67%).
pub const SICK_TRIGGER_DRAINED: u16 = 43908;

// ---------------------------------------------------------------------------
// Display warning thresholds (below critical, player should act soon)
// ---------------------------------------------------------------------------

/// Hunger warning threshold (~30%).  Pet looks peckish.
pub const WARNING_HUNGER: u16 = 19660;
/// Tired warning threshold (~40%).  Pet looks sleepy.
pub const WARNING_TIRED: u16 = 26214;
/// Drained warning threshold (~35%).  Pet looks listless.
pub const WARNING_DRAINED: u16 = 22937;
/// Sick warning threshold (~40%).  Pet looks unwell.
pub const WARNING_SICK: u16 = 26214;
/// Miserable warning threshold (~50%).  Pet looks unhappy.
pub const WARNING_MISERABLE: u16 = 32768;

// ---------------------------------------------------------------------------
// Miserable
// ---------------------------------------------------------------------------

/// Miserable increase amount (interval-based).
pub const MISERABLE_AMOUNT: u16 = 655;
/// Base interval for miserable decay (ticks).
pub const MISERABLE_INTERVAL_BASE: u32 = 240;
/// Interval reduction per stat above 60%.
pub const MISERABLE_INTERVAL_PER_STAT: u32 = 25;
/// Minimum miserable interval.
pub const MISERABLE_INTERVAL_MIN: u32 = 40;
/// Threshold for "stat above 60%" check.
pub const MISERABLE_STAT_THRESHOLD: u16 = 39321; // 60%

/// Miserable level that boosts hunger/tired/sick rates (70%).
pub const MISERABLE_BOOST_THRESHOLD: u16 = 45874;
/// Miserable level that accelerates drained interval (80%).
pub const MISERABLE_DRAIN_THRESHOLD: u16 = 52428;

// ---------------------------------------------------------------------------
// Actions
// ---------------------------------------------------------------------------

/// Feed: duration in ticks, cooldown in ticks.
pub const FEED_DURATION: u8 = 2;
pub const FEED_COOLDOWN: u16 = 12;
/// Feed: hunger reduction per action tick.
pub const FEED_HUNGER_RELIEF: u16 = 3930;
/// Feed: drained reduction per action tick.
pub const FEED_DRAINED_RELIEF: u16 = 1310;

/// Heal: duration, cooldown.
pub const HEAL_DURATION: u8 = 3;
pub const HEAL_COOLDOWN: u16 = 24;
/// Heal: sick reduction per action tick.
pub const HEAL_SICK_RELIEF: u16 = 9825;

/// Relax: duration, cooldown.
pub const RELAX_DURATION: u8 = 2;
pub const RELAX_COOLDOWN: u16 = 24;
/// Relax: drained reduction per action tick.
pub const RELAX_DRAINED_RELIEF: u16 = 6550;
/// Relax: hunger increase per action tick (costs energy).
pub const RELAX_HUNGER_COST: u16 = 13100;

/// Play: duration, cooldown.
pub const PLAY_DURATION: u8 = 4;
pub const PLAY_COOLDOWN: u16 = 48;

/// Mini-game cooldown (Tic Tac Toe / Lights Out).
///
/// Triggered when `award_inspiration` runs, i.e. when the player
/// successfully completes either mini-game.  Same magnitude as the
/// `Play` action's cooldown for parity — both award the same kind of
/// reward (drained relief) on the same Play menu.
pub const MINIGAME_COOLDOWN: u16 = 48;
/// Play: base costs per action tick (modified by curiosity).
pub const PLAY_HUNGER_COST: u16 = 655;
pub const PLAY_TIRED_COST: u16 = 1310;
pub const PLAY_DRAINED_COST: u16 = 1965;

// ---------------------------------------------------------------------------
// Lifecycle
// ---------------------------------------------------------------------------

/// Hatching duration (ticks).  6 ticks = 1 minute on hardware.
/// Simulator: 1 tick = 10 s, fast enough to debug without waiting.
#[cfg(not(feature = "simulator"))]
pub const HATCHING_TICKS: u16 = 6;
#[cfg(feature = "simulator")]
pub const HATCHING_TICKS: u16 = 1;

/// Ticks of maxed stats before pet leaves, indexed by count of maxed stats.
/// Index 0 unused, 1 = one maxed stat, etc.
pub const LEAVING_THRESHOLDS: [u32; 5] = [
    u32::MAX, // 0 maxed stats: never leaves
    7200,     // 1 maxed: 20 hours
    3600,     // 2 maxed: 10 hours
    1800,     // 3 maxed: 5 hours
    720,      // 4 maxed: 2 hours
];

/// Maximum sleep between wake-ups (ticks).  180 ticks = 30 minutes.
pub const MAX_SLEEP_TICKS: u32 = 180;

/// Minimum interval between saves to flash (ticks).  90 ticks = 15 minutes.
/// Saves piggyback on update cycles — no extra wake-ups.
pub const SAVE_INTERVAL_TICKS: u32 = 90;

// ---------------------------------------------------------------------------
// Traits
// ---------------------------------------------------------------------------

/// Trait range at hatch: [MIN_TRAIT, MAX_TRAIT].
pub const MIN_TRAIT: u16 = 16384; // 25%
pub const MAX_TRAIT: u16 = 49152; // 75%
