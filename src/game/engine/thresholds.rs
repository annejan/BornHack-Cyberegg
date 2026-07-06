//! Game balance — runtime-loadable rates, cooldowns, and thresholds.
//!
//! All stat values are u16 (0–65535).  655 ≈ 1% on a 0–100 scale.
//! Time is measured in ticks (1 tick = 10 real seconds).
//!
//! These values used to be `pub const`s.  They are now fields of a
//! [`Thresholds`] struct held in a single boot-installed static so they
//! can be picked from one of two presets ([`Thresholds::CLASSIC`] /
//! [`Thresholds::CASUAL`]) or overridden by a `BORNPETS.CFG` file on
//! the badge's USB drive.  Each old identifier still exists as an
//! `#[inline] pub fn FOO() -> T` accessor so consumer code reads the
//! active value without ceremony.
//!
//! ## Adding a new field
//!
//! 1. Add it to the `thresholds_table!` invocation with its type.
//! 2. Add a value in both `CLASSIC` and `CASUAL`.
//! 3. (Optional) add a `KEY=VALUE` row to [`BORNPETS_CFG_KEYS`] for
//!    user overrides via the config file.
//!
//! ## Hot-path note
//!
//! Accessors are `#[inline(always)]` and the active struct is a single
//! static — codegen folds to one load.  Array fields (e.g.
//! `LEAVING_THRESHOLDS`) are returned by-value (Copy) per access; cache
//! the result in a local if you index it in a tight loop.

use core::sync::atomic::{AtomicBool, Ordering};

// ---------------------------------------------------------------------------
// Field table — keep in sync with the CLASSIC / CASUAL constants below.
// ---------------------------------------------------------------------------

macro_rules! thresholds_table {
    ( $( $name:ident : $ty:ty ),* $(,)? ) => {
        /// Snapshot of all tunable game-balance values.  See module docs.
        #[allow(non_snake_case)]
        #[derive(Clone, Copy, Debug)]
        pub struct Thresholds {
            $( pub $name: $ty, )*
        }

        $(
            #[allow(non_snake_case)]
            #[inline(always)]
            pub fn $name() -> $ty {
                cfg().$name
            }
        )*
    };
}

thresholds_table! {
    // Scale
    STAT_MAX: u16,
    UNIT: u16,
    // Hunger
    HUNGER_RATE: u16,
    HUNGER_MISERABLE_BOOST: u16,
    // Tired
    TIRED_RATE: u16,
    TIRED_MISERABLE_BOOST: u16,
    TIRED_PASSIVE_RECOVERY: u16,
    TIRED_PASSIVE_INTERVAL: u32,
    SLEEP_RECOVERY_SLOW: u16,
    SLEEP_RECOVERY_MEDIUM: u16,
    SLEEP_RECOVERY_FAST: u16,
    SLEEP_HUNGER_COST: u16,
    SLEEP_TIER_SLOW: u16,
    SLEEP_TIER_MEDIUM: u16,
    // Drained
    DRAINED_AMOUNT: u16,
    DRAINED_INTERVAL: u32,
    DRAINED_INTERVAL_MISERABLE: u32,
    DRAINED_SLEEP_RECOVERY: u16,
    // Sick
    SICK_RATE: u16,
    SICK_CONDITION_RATE: u16,
    SICK_CONDITION_MISERABLE_RATE: u16,
    SICK_TRIGGER_HUNGER: u16,
    SICK_TRIGGER_TIRED: u16,
    SICK_TRIGGER_DRAINED: u16,
    // Warnings
    WARNING_HUNGER: u16,
    WARNING_TIRED: u16,
    WARNING_DRAINED: u16,
    WARNING_SICK: u16,
    WARNING_MISERABLE: u16,
    // Miserable
    MISERABLE_AMOUNT: u16,
    MISERABLE_INTERVAL_BASE: u32,
    MISERABLE_INTERVAL_PER_STAT: u32,
    MISERABLE_INTERVAL_MIN: u32,
    MISERABLE_STAT_THRESHOLD: u16,
    MISERABLE_BOOST_THRESHOLD: u16,
    MISERABLE_DRAIN_THRESHOLD: u16,
    // Actions
    FEED_DURATION: u8,
    FEED_COOLDOWN: u16,
    FEED_HUNGER_RELIEF: u16,
    FEED_DRAINED_RELIEF: u16,
    HEAL_DURATION: u8,
    HEAL_COOLDOWN: u16,
    HEAL_SICK_RELIEF: u16,
    RELAX_DURATION: u8,
    RELAX_COOLDOWN: u16,
    RELAX_DRAINED_RELIEF: u16,
    RELAX_HUNGER_COST: u16,
    PLAY_DURATION: u8,
    PLAY_COOLDOWN: u16,
    PLAY_HUNGER_COST: u16,
    PLAY_TIRED_COST: u16,
    PLAY_DRAINED_COST: u16,
    MINIGAME_COOLDOWN: u16,
    MINIGAME_HUNGER_COST: u16,
    // Lifecycle
    HATCHING_TICKS: u16,
    LEAVING_THRESHOLDS: [u32; 5],
    MAX_SLEEP_TICKS: u32,
    SAVE_INTERVAL_TICKS: u32,
    // Traits
    MIN_TRAIT: u16,
    MAX_TRAIT: u16,
}

// ---------------------------------------------------------------------------
// Presets
// ---------------------------------------------------------------------------

impl Thresholds {
    /// The original balance — what the badge ships with.
    pub const CLASSIC: Thresholds = Thresholds {
        STAT_MAX: 65535,
        UNIT: 655,

        HUNGER_RATE: 9,
        HUNGER_MISERABLE_BOOST: 0,

        TIRED_RATE: 14,
        TIRED_MISERABLE_BOOST: 0,
        TIRED_PASSIVE_RECOVERY: 655,
        TIRED_PASSIVE_INTERVAL: 120,
        SLEEP_RECOVERY_SLOW: 3275,
        SLEEP_RECOVERY_MEDIUM: 6550,
        SLEEP_RECOVERY_FAST: 9825,
        SLEEP_HUNGER_COST: 3100,
        SLEEP_TIER_SLOW: 49807,
        SLEEP_TIER_MEDIUM: 30145,

        DRAINED_AMOUNT: 655,
        DRAINED_INTERVAL: 90,
        DRAINED_INTERVAL_MISERABLE: 30,
        DRAINED_SLEEP_RECOVERY: 655,

        SICK_RATE: 1,
        SICK_CONDITION_RATE: 655,
        SICK_CONDITION_MISERABLE_RATE: 1310,
        SICK_TRIGGER_HUNGER: 39321,
        SICK_TRIGGER_TIRED: 49151,
        SICK_TRIGGER_DRAINED: 43908,

        WARNING_HUNGER: 19660,
        WARNING_TIRED: 26214,
        WARNING_DRAINED: 22937,
        WARNING_SICK: 26214,
        WARNING_MISERABLE: 32768,

        MISERABLE_AMOUNT: 655,
        MISERABLE_INTERVAL_BASE: 240,
        MISERABLE_INTERVAL_PER_STAT: 25,
        MISERABLE_INTERVAL_MIN: 40,
        MISERABLE_STAT_THRESHOLD: 39321,
        MISERABLE_BOOST_THRESHOLD: 45874,
        MISERABLE_DRAIN_THRESHOLD: 52428,

        FEED_DURATION: 2,
        FEED_COOLDOWN: 12,
        FEED_HUNGER_RELIEF: 3930,
        FEED_DRAINED_RELIEF: 1310,
        HEAL_DURATION: 3,
        HEAL_COOLDOWN: 24,
        HEAL_SICK_RELIEF: 9825,
        RELAX_DURATION: 2,
        RELAX_COOLDOWN: 24,
        RELAX_DRAINED_RELIEF: 6550,
        RELAX_HUNGER_COST: 6550,
        PLAY_DURATION: 4,
        PLAY_COOLDOWN: 48,
        PLAY_HUNGER_COST: 655,
        PLAY_TIRED_COST: 1310,
        PLAY_DRAINED_COST: 1965,
        MINIGAME_COOLDOWN: 18,
        MINIGAME_HUNGER_COST: 3000,

        #[cfg(not(feature = "simulator"))]
        HATCHING_TICKS: 6,
        #[cfg(feature = "simulator")]
        HATCHING_TICKS: 1,
        LEAVING_THRESHOLDS: [u32::MAX, 7200, 3600, 1800, 720],
        MAX_SLEEP_TICKS: 180,
        SAVE_INTERVAL_TICKS: 90,

        MIN_TRAIT: 16384,
        MAX_TRAIT: 49152,
    };

    /// Half-speed decay, doubled lifetimes, more generous action relief.
    /// Aim: a forgiving balance for badge holders who don't want to baby-sit.
    pub const CASUAL: Thresholds = Thresholds {
        // Everything not listed below is identical to CLASSIC.
        STAT_MAX: 65535,
        UNIT: 655,

        HUNGER_RATE: 4,
        HUNGER_MISERABLE_BOOST: 0,

        TIRED_RATE: 7,
        TIRED_MISERABLE_BOOST: 0,
        TIRED_PASSIVE_RECOVERY: 655,
        TIRED_PASSIVE_INTERVAL: 120,
        SLEEP_RECOVERY_SLOW: 3275,
        SLEEP_RECOVERY_MEDIUM: 6550,
        SLEEP_RECOVERY_FAST: 9825,
        SLEEP_HUNGER_COST: 1500,
        SLEEP_TIER_SLOW: 49807,
        SLEEP_TIER_MEDIUM: 30145,

        DRAINED_AMOUNT: 655,
        DRAINED_INTERVAL: 180,
        DRAINED_INTERVAL_MISERABLE: 60,
        DRAINED_SLEEP_RECOVERY: 655,

        SICK_RATE: 1,
        SICK_CONDITION_RATE: 328,
        SICK_CONDITION_MISERABLE_RATE: 655,
        SICK_TRIGGER_HUNGER: 39321,
        SICK_TRIGGER_TIRED: 49151,
        SICK_TRIGGER_DRAINED: 43908,

        WARNING_HUNGER: 19660,
        WARNING_TIRED: 26214,
        WARNING_DRAINED: 22937,
        WARNING_SICK: 26214,
        WARNING_MISERABLE: 32768,

        MISERABLE_AMOUNT: 655,
        MISERABLE_INTERVAL_BASE: 360,
        MISERABLE_INTERVAL_PER_STAT: 25,
        MISERABLE_INTERVAL_MIN: 60,
        MISERABLE_STAT_THRESHOLD: 39321,
        MISERABLE_BOOST_THRESHOLD: 45874,
        MISERABLE_DRAIN_THRESHOLD: 52428,

        FEED_DURATION: 2,
        FEED_COOLDOWN: 12,
        FEED_HUNGER_RELIEF: 6550,
        FEED_DRAINED_RELIEF: 2620,
        HEAL_DURATION: 3,
        HEAL_COOLDOWN: 24,
        HEAL_SICK_RELIEF: 16384,
        RELAX_DURATION: 2,
        RELAX_COOLDOWN: 24,
        RELAX_DRAINED_RELIEF: 9825,
        RELAX_HUNGER_COST: 3275,
        PLAY_DURATION: 4,
        PLAY_COOLDOWN: 48,
        PLAY_HUNGER_COST: 328,
        PLAY_TIRED_COST: 655,
        PLAY_DRAINED_COST: 983,
        MINIGAME_COOLDOWN: 12,
        MINIGAME_HUNGER_COST: 1500,

        #[cfg(not(feature = "simulator"))]
        HATCHING_TICKS: 6,
        #[cfg(feature = "simulator")]
        HATCHING_TICKS: 1,
        LEAVING_THRESHOLDS: [u32::MAX, 14400, 7200, 3600, 1440],
        MAX_SLEEP_TICKS: 180,
        SAVE_INTERVAL_TICKS: 90,

        MIN_TRAIT: 16384,
        MAX_TRAIT: 49152,
    };
}

// ---------------------------------------------------------------------------
// Mode (which preset is active)
// ---------------------------------------------------------------------------

/// Game-balance preset.  Selected via the badge menu and persisted in KV.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "embassy-base", derive(defmt::Format))]
#[repr(u8)]
pub enum Mode {
    Classic = 0,
    Casual = 1,
}

impl Mode {
    pub const DEFAULT: Mode = Mode::Classic;

    pub fn label(self) -> &'static str {
        match self {
            Mode::Classic => "Classic",
            Mode::Casual => "Casual",
        }
    }

    pub fn from_u8(v: u8) -> Mode {
        match v {
            1 => Mode::Casual,
            _ => Mode::Classic,
        }
    }

    /// Preset values for this mode.
    pub fn preset(self) -> Thresholds {
        match self {
            Mode::Classic => Thresholds::CLASSIC,
            Mode::Casual => Thresholds::CASUAL,
        }
    }
}

// ---------------------------------------------------------------------------
// Active instance
// ---------------------------------------------------------------------------

// `static mut` is fine here: written exactly once during boot
// (`install`) before any game tick fires and read-only thereafter.
// Live mode-switching is intentionally not supported — switching the
// preset requires a reboot so an in-flight pet doesn't see fields
// change between ticks.
static mut ACTIVE: Thresholds = Thresholds::CLASSIC;
static INSTALLED: AtomicBool = AtomicBool::new(false);
static IS_CUSTOM: AtomicBool = AtomicBool::new(false);
static ACTIVE_MODE: AtomicBool = AtomicBool::new(false); // false=Classic, true=Casual

/// Install the active threshold set.  Call once, before any game tick.
///
/// `custom` should be `true` whenever the values differ from the bare
/// preset for the chosen [`Mode`] (e.g. because a `BORNPETS.CFG` file
/// overrode one or more fields).  The UI uses it to surface a `*`
/// indicator next to the pet's name.
pub fn install(values: Thresholds, mode: Mode, custom: bool) {
    if INSTALLED.swap(true, Ordering::SeqCst) {
        return;
    }
    // Safety: write happens once, before any reader exists.
    unsafe {
        ACTIVE = values;
    }
    ACTIVE_MODE.store(matches!(mode, Mode::Casual), Ordering::SeqCst);
    IS_CUSTOM.store(custom, Ordering::SeqCst);
}

/// Active threshold values.  Cheap (one static load).
#[inline]
pub fn cfg() -> &'static Thresholds {
    #[allow(static_mut_refs)]
    unsafe {
        &ACTIVE
    }
}

/// `true` if any field of the active set differs from the bare preset.
pub fn is_custom() -> bool {
    IS_CUSTOM.load(Ordering::Relaxed)
}

/// The mode picked at install time.
pub fn active_mode() -> Mode {
    if ACTIVE_MODE.load(Ordering::Relaxed) {
        Mode::Casual
    } else {
        Mode::Classic
    }
}

// ---------------------------------------------------------------------------
// BORNPETS.CFG key set — single source of truth for the parser.
// ---------------------------------------------------------------------------

/// Identifiers accepted in a `BORNPETS.CFG` file.  Each row is
/// `(KEY, setter)`; the setter takes a parsed `u32` and writes the
/// clamped value into a `Thresholds` instance.  Unknown keys are
/// ignored by the parser, so adding a new tunable later does not break
/// older config files.
pub type Setter = fn(&mut Thresholds, u32);

pub const BORNPETS_CFG_KEYS: &[(&str, Setter)] = &[
    ("HUNGER_RATE", |t, v| t.HUNGER_RATE = clamp_u16(v)),
    ("TIRED_RATE", |t, v| t.TIRED_RATE = clamp_u16(v)),
    ("SLEEP_HUNGER_COST", |t, v| {
        t.SLEEP_HUNGER_COST = clamp_u16(v)
    }),
    // Interval keys divide the tick counter (see interval_fires); floor to 1
    // so a BORNPETS.CFG value of 0 can't cause a divide-by-zero.
    ("DRAINED_INTERVAL", |t, v| t.DRAINED_INTERVAL = v.max(1)),
    ("DRAINED_INTERVAL_MISERABLE", |t, v| {
        t.DRAINED_INTERVAL_MISERABLE = v.max(1)
    }),
    ("SICK_RATE", |t, v| t.SICK_RATE = clamp_u16(v)),
    ("SICK_CONDITION_RATE", |t, v| {
        t.SICK_CONDITION_RATE = clamp_u16(v)
    }),
    ("SICK_CONDITION_MISERABLE_RATE", |t, v| {
        t.SICK_CONDITION_MISERABLE_RATE = clamp_u16(v)
    }),
    ("MISERABLE_INTERVAL_BASE", |t, v| {
        t.MISERABLE_INTERVAL_BASE = v.max(1)
    }),
    ("MISERABLE_INTERVAL_MIN", |t, v| {
        t.MISERABLE_INTERVAL_MIN = v.max(1)
    }),
    ("FEED_HUNGER_RELIEF", |t, v| {
        t.FEED_HUNGER_RELIEF = clamp_u16(v)
    }),
    ("FEED_DRAINED_RELIEF", |t, v| {
        t.FEED_DRAINED_RELIEF = clamp_u16(v)
    }),
    ("HEAL_SICK_RELIEF", |t, v| t.HEAL_SICK_RELIEF = clamp_u16(v)),
    ("RELAX_DRAINED_RELIEF", |t, v| {
        t.RELAX_DRAINED_RELIEF = clamp_u16(v)
    }),
    ("RELAX_HUNGER_COST", |t, v| {
        t.RELAX_HUNGER_COST = clamp_u16(v)
    }),
    ("PLAY_HUNGER_COST", |t, v| t.PLAY_HUNGER_COST = clamp_u16(v)),
    ("PLAY_TIRED_COST", |t, v| t.PLAY_TIRED_COST = clamp_u16(v)),
    ("PLAY_DRAINED_COST", |t, v| {
        t.PLAY_DRAINED_COST = clamp_u16(v)
    }),
    ("MINIGAME_HUNGER_COST", |t, v| {
        t.MINIGAME_HUNGER_COST = clamp_u16(v)
    }),
    ("MINIGAME_COOLDOWN", |t, v| {
        t.MINIGAME_COOLDOWN = clamp_u16(v)
    }),
    ("HATCHING_TICKS", |t, v| t.HATCHING_TICKS = clamp_u16(v)),
    ("MAX_SLEEP_TICKS", |t, v| t.MAX_SLEEP_TICKS = v),
];

#[inline]
fn clamp_u16(v: u32) -> u16 {
    if v > u16::MAX as u32 {
        u16::MAX
    } else {
        v as u16
    }
}
