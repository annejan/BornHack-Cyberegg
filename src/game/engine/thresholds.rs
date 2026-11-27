//! Game balance — runtime-loadable rates, cooldowns, and thresholds.
//!
//! All stat values are u16 (0–65535).  655 ≈ 1% on a 0–100 scale.
//! Time is measured in ticks (1 tick = 10 real seconds).
//!
//! These values used to be `pub const`s.  They are now fields of a
//! [`Thresholds`] struct held in a single boot-installed static so they
//! can be picked from one of two presets ([`Thresholds::CLASSIC`] /
//! [`Thresholds::CASUAL`]) or overridden by a `BPETS_V3.CFG` file on
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
//! static — codegen folds to one load.

use core::sync::atomic::{AtomicBool, Ordering};

// ---------------------------------------------------------------------------
// Field table — keep in sync with the CLASSIC / CASUAL constants below.
// ---------------------------------------------------------------------------

macro_rules! thresholds_table {
    ( $( $name:ident : $ty:ty ),* $(,)? ) => {
        /// Snapshot of all tunable game-balance values.  See module docs.
        #[allow(non_snake_case)]
        #[derive(Clone, Copy, Debug, PartialEq)]
        pub struct Thresholds {
            $( pub $name: $ty, )*
        }

        impl Thresholds {
            /// Field value by config key name (host only; feeds the
            /// `BPETS_V3.CFG` template).
            #[cfg(any(test, feature = "simulator"))]
            pub fn value_of(&self, key: &str) -> Option<u32> {
                match key {
                    $( stringify!($name) => Some(self.$name as u32), )*
                    _ => None,
                }
            }

            /// Every table field name, in table order (host only; the
            /// player's guide dumps them).
            #[cfg(any(test, feature = "simulator"))]
            pub const FIELDS: &'static [&'static str] = &[ $( stringify!($name), )* ];
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
    // Actions
    FEED_DURATION: u8,
    HEAL_DURATION: u8,
    PLAY_DURATION: u8,
    MINIGAME_COOLDOWN: u16,
    // Lifecycle
    HATCHING_TICKS: u16,
    MAX_SLEEP_TICKS: u32,
    SAVE_INTERVAL_TICKS: u32,
    // Traits
    MIN_TRAIT: u16,
    MAX_TRAIT: u16,
    // Exercise
    EXERCISE_DURATION: u8,
    // Medication
    MEDICATE_DURATION: u8,
    // Ozempic — accelerated weight loss, not gated on being diabetic.
    OZEMPIC_DURATION: u8,
    // Drink
    DRINK_DURATION: u8,
    // Rehab
    REHAB_DURATION: u8,
    // Battle — no duration (resolves instantly), just a cooldown between fights.
    BATTLE_COOLDOWN: u16,
    // v3 battle snapshot (plan 5); `_PTS` are points, `_PCT` percents
    BATTLE_POWER_BASE_PTS: u8,
    BATTLE_POWER_WEIGHT_PCT: u8,
    BATTLE_COURAGE_PEAK_PTS: u8,
    BATTLE_HIGH_MOOD_PCT: u8,
    BATTLE_HIGH_MOOD_PTS: u8,
    BATTLE_LOW_MOOD_PCT: u8,
    BATTLE_GUARD_WEIGHT_PCT: u8,
    BATTLE_GUARD_BASE_PTS: u8,
    BATTLE_SPEED_BASE_PTS: u8,
    BATTLE_SPEED_FIT_PCT: u8,
    BATTLE_RESTED_SPEED_PTS: u8,
    BATTLE_RESTED_FOCUS_PTS: u8,
    BATTLE_GROGGY_SPEED_PTS: u8,
    BATTLE_GROGGY_FOCUS_PTS: u8,
    BATTLE_HANGOVER_SPEED_PTS: u8,
    BATTLE_HANGOVER_FOCUS_PTS: u8,
    BATTLE_CRAVING_FOCUS_PTS: u8,
    BATTLE_CRAVING_POWER_PTS: u8,
    BATTLE_HEAVY_UNFIT_FIT_PCT: u8,
    BATTLE_SLOPPY_PCT: u8,
    BATTLE_FELT_BASE_PCT: u16,
    BATTLE_HP_BASE_PTS: u8,
    BATTLE_HP_WEIGHT_PCT: u8,
    BATTLE_POWER_FLOOR_PTS: u8,
    BATTLE_GUARD_FLOOR_PTS: u8,
    BATTLE_HP_FLOOR_PTS: u8,
    BATTLE_STAMINA_FLOOR_PTS: u8,
    BATTLE_INJURED_PCT: u8,
    BATTLE_FORM_TOP_PCT: u8,
    BATTLE_FORM_IN_PCT: u8,
    BATTLE_FORM_OFF_PCT: u8,
    BATTLE_PAIR_COOLDOWN_MIN: u16,
    BATTLE_ENERGY_PCT: u8,
    BATTLE_WATER_PCT: u8,
    BATTLE_FUN_PCT: u8,
    BATTLE_DIRT_PCT: u8,
    BATTLE_STRAIN_PCT: u8,
    BATTLE_STRAIN_NEED_PCT: u16,
    BATTLE_STRAIN_RESTED_PCT: u16,
    BATTLE_BRUISE_MAX_PCT: u8,
    BATTLE_MOOD_PCT: u8,
    BATTLE_MIN_HEALTH_PCT: u8,
    BATTLE_DRUNK_PCT: u8,
    BATTLE_PRIZE_FIRST_HAX: u16,
    BATTLE_PRIZE_LATER_HAX: u16,
    BATTLE_PRIZE_BEACON_MIN: u16,
    BATTLE_DECLINE_MOOD_PCT: u16,
    BATTLE_DECLINE_MIN: u16,
    // v3 need bands (percent, 1..=99); see engine::needs
    FULL_LO_PCT: u8,
    FULL_HI_PCT: u8,
    WATER_LO_PCT: u8,
    WATER_HI_PCT: u8,
    ENERGY_LO_PCT: u8,
    FUN_LO_PCT: u8,
    FUN_HI_PCT: u8,
    DIRT_HI_PCT: u8,
    BOOZE_HI_PCT: u8,
    BOOZE_DANGER_PCT: u8,
    STRAIN_INJ_PCT: u8,
    FIT_LO_PCT: u8,
    WEIGHT_LO_PCT: u8,
    WEIGHT_HI_PCT: u8,
    // v3 regime rates (per engine minute, magnitudes; sign lives in engine::regime).
    // `_PM` = units per minute or per-mille, `_PCT` = percent multiplier or level.
    FULL_DRAIN_AWAKE_PM: u16,
    WATER_DRAIN_AWAKE_PM: u16,
    ENERGY_DRAIN_AWAKE_PM: u16,
    FUN_DRAIN_PM: u16,
    FULL_DRAIN_ASLEEP_PM: u16,
    WATER_DRAIN_ASLEEP_PM: u16,
    ENERGY_GAIN_ASLEEP_PM: u16,
    SLEEP_FUN_FLOOR_PCT: u8,
    DIRT_GAIN_PM: u16,
    BOOZE_CLEAR_PM: u16,
    BOOZE_CLEAR_SLOW_PCT: u16, // approximates 2/3; 36 -> 24 -> 16
    STRAIN_CLEAR_PM: u16,
    FIT_DECAY_HIGH_PM: u16,
    FIT_DECAY_MID_PM: u16,
    FIT_DECAY_LOW_PM: u16,
    FIT_DECAY_HIGH_ABOVE_PCT: u8,
    FIT_DECAY_MID_ABOVE_PCT: u8,
    FIT_DECAY_HEAVY_PCT: u16,
    WEIGHT_PULL_HEAVY_PM: u16,
    WEIGHT_PULL_PM: u16,
    WEIGHT_BELLY_PM: u16,
    ENERGY_MULT_UNFIT_PCT: u16,
    ENERGY_MULT_FIT_PCT: u16,
    ENERGY_FIT_GOOD_PCT: u8,
    ENERGY_MULT_DRUNK_PCT: u16,
    ENERGY_MULT_WIRED_PCT: u16,
    ENERGY_MULT_HANGOVER_PCT: u16,
    ENERGY_MULT_WEIGHT_PCT: u16,
    ENERGY_CAP_DEPRIVED_PCT: u8,
    THIRST_MULT_BASE_PCT: u16,
    THIRST_MULT_LOW_PCT: u16,
    THIRST_MULT_MID_PCT: u16,
    THIRST_MULT_HIGH_PCT: u16,
    THIRST_BOOZE_LOW_PCT: u8,
    THIRST_BOOZE_MID_PCT: u8,
    THIRST_BOOZE_HIGH_PCT: u8,
    SLEEP_QUALITY_POOR_PM: u16,
    SLEEP_QUALITY_GOOD_PM: u16,
    UNSETTLED_BOOZE_PCT: u8,
    UNSETTLED_FULL_LO_PCT: u8,
    UNSETTLED_FULL_HI_PCT: u8,
    UNSETTLED_WATER_LO_PCT: u8,
    UNSETTLED_WATER_HI_PCT: u8,
    // v3 mood / health terms (see engine::regime); DMG_*_PM are per-minute damage
    MOOD_STEP: u16,
    MOOD_PEN_INJURED_PCT: u16,
    MOOD_PEN_GROGGY_PCT: u16,
    MOOD_PEN_HANGOVER_PCT: u16,
    MOOD_PEN_DEPRIVED_PCT: u16,
    MOOD_PEN_REHAB_PCT: u16,
    MOOD_PEN_CRAVING_PCT: u16,
    MOOD_CRAVING_BOOZE_PCT: u8,
    MOOD_HEALTH_FLOOR_PCT: u8,
    CEILING_FIT_STEP1_PCT: u8,
    CEILING_FIT_STEP2_PCT: u8,
    CEILING_FIT_STEP3_PCT: u8,
    CEILING_STEP0_PCT: u16,
    CEILING_STEP1_PCT: u16,
    CEILING_STEP2_PCT: u16,
    CEILING_STEP3_PCT: u16,
    CEILING_DIABETIC_PCT: u16,
    DMG_NEED_LOW_PCT: u8,
    DMG_NEED_HIGH_PCT: u8,
    DMG_ENERGY_PCT: u8,
    DMG_ENERGY_HEALTH_PCT: u8,
    DMG_MOOD_PCT: u8,
    DMG_DIRT_PCT: u8,
    DMG_WEIGHT_HIGH_PCT: u8,
    DMG_WEIGHT_LOW_PCT: u8,
    DMG_FULL_LOW_PM: u16,
    DMG_FULL_HIGH_PM: u16,
    DMG_WATER_LOW_PM: u16,
    DMG_WATER_HIGH_PM: u16,
    DMG_ENERGY_PM: u16,
    DMG_BOOZE_PM: u16,
    DMG_MOOD_PM: u16,
    DMG_DIRT_PM: u16,
    DMG_WEIGHT_HIGH_PM: u16,
    DMG_WEIGHT_LOW_PM: u16,
    REC_GOOD_PM: u16,
    REC_FAIR_PM: u16,
    REC_GOOD_MOOD_PCT: u8,
    REC_FAIR_MOOD_PCT: u8,
    REC_DEPRIVED_PCT: u16,
    // v3 advance: onset/cure times (minutes), sleep rules, collapse, leaving
    DEPENDENCE_ONSET_MIN: u16,
    DEPENDENCE_RELAPSE_MIN: u16,
    DIABETES_ONSET_MIN: u16,
    DIABETES_REVERSAL_MIN: u16,
    HANGOVER_MIN: u16,
    RECOVERING_MIN: u16,
    SELF_WAKE_MIN: u16,
    OVERSLEEP_WAKE_MIN: u16,
    GROGGY_LINE_MIN: u16,
    GROGGY_MIN: u16,
    GROGGY_ENERGY_PCT: u8,
    GROGGY_FIT_PCT: u8,
    RESTED_MIN: u16,
    GOOD_SLEEP_LINE_MIN: u16,
    NIGHT_SHORT_MIN: u16,
    NIGHT_GOOD_MIN: u16,
    DEPRIVED_NIGHTS: u8,
    CLEARING_NIGHTS: u8,
    GRUMPY_PCT: u8,
    GRUMPY_ENERGY_PCT: u8,
    COLLAPSE_DAMAGE: u16,
    COLLAPSE_HEALTH_FLOOR_PCT: u8,
    LEAVING_MIN: u16,
    BOOZE_CLEAR_PCT: u8,
    DIABETES_WEIGHT_HI_PCT: u8,
    DIABETES_WEIGHT_LO_PCT: u8,
    REVERSAL_FIT_PCT: u8,
    HANGOVER_ASLEEP_MULT: u8,
    HEALTH_EXCESS_DECAY_PM: u16,
    CARE_GOOD_MOOD_PCT: u8,
    OVERSLEEP_RESET_PCT: u8,
    // v3 doses: food and drink (percent points of the scale unless noted;
    // `_PM` = per-mille of the scale, so 5 is half a percent)
    DOSE_SALAD_FULL_PCT: u8,
    DOSE_APPLE_FULL_PCT: u8,
    DOSE_FRIKANDEL_FULL_PCT: u8,
    DOSE_PIZZA_FULL_PCT: u8,
    DOSE_CAKE_FULL_PCT: u8,
    DOSE_SALAD_DIRT_PCT: u8,
    DOSE_APPLE_DIRT_PCT: u8,
    DOSE_FRIKANDEL_DIRT_PCT: u8,
    DOSE_PIZZA_DIRT_PCT: u8,
    DOSE_CAKE_DIRT_PCT: u8,
    DOSE_SALAD_WEIGHT_PM: u16,
    DOSE_APPLE_WEIGHT_PM: u16,
    DOSE_FRIKANDEL_WEIGHT_PM: u16,
    DOSE_PIZZA_WEIGHT_PM: u16,
    DOSE_CAKE_WEIGHT_PM: u16,
    DOSE_FRIKANDEL_MOOD_PCT: u8,
    DOSE_PIZZA_MOOD_PCT: u8,
    DOSE_CAKE_MOOD_PCT: u8,
    DOSE_HEALTHY_WATER_PCT: u8,
    DOSE_SALAD_FIT_PCT: u8,
    DOSE_JUNK_REPEAT_DIV: u8,
    DOSE_JUNK_MIN: u16,
    DOSE_FULL_CAP_PCT: u8,
    DOSE_EXCESS_ENERGY_DIV: u8,
    DOSE_EXCESS_WEIGHT_DIV: u8,
    DOSE_WATER_MAX_PCT: u8,
    DOSE_WATER_BELLY_PCT: u8,
    DOSE_WATER_BELLY_MIN_PCT: u8,
    DOSE_WATER_BOOZE_PCT: u8,
    DOSE_WATER_HANGOVER_MIN: u16,
    DOSE_WATER_GROGGY_MIN: u16,
    DOSE_COFFEE_WATER_PCT: u8,
    DOSE_COFFEE_FULL_PCT: u8,
    DOSE_COFFEE_ENERGY_PCT: u8,
    DOSE_COFFEE_MOOD_PCT: u8,
    DOSE_COFFEE_WEIGHT_PCT: u8,
    // Coffee's caffeine: each cup adds COFFEE_WINDOW_MIN minutes, which wear
    // off one a minute; COFFEE_WIRED_CUPS cups within COFFEE_WINDOW_MIN wire
    // the pet for COFFEE_WIRED_MIN minutes.
    COFFEE_WINDOW_MIN: u16,
    COFFEE_WIRED_CUPS: u8,
    COFFEE_WIRED_MIN: u16,
    DOSE_BEER_WATER_PCT: u8,
    DOSE_BEER_BELLY_PCT: u8,
    DOSE_BEER_BOOZE_PCT: u8,
    DOSE_BEER_MOOD_PCT: u8,
    DOSE_WINE_WATER_PCT: u8,
    DOSE_WINE_BELLY_PCT: u8,
    DOSE_WINE_BOOZE_PCT: u8,
    DOSE_WINE_MOOD_PCT: u8,
    DOSE_WHISKEY_WATER_PCT: u8,
    DOSE_WHISKEY_BELLY_PCT: u8,
    DOSE_WHISKEY_BOOZE_PCT: u8,
    DOSE_WHISKEY_MOOD_PCT: u8,
    DOSE_BEER_WEIGHT_PCT: u8,
    DOSE_ALCOHOL_ENERGY_PCT: u8,
    DOSE_ALCOHOL_FUN_PCT: u8,
    DOSE_TIRED_BOOZE_PCT: u16,
    DOSE_DEPENDENT_DIV: u8,
    DOSE_RELAPSE_MOOD_PCT: u8,
    DOSE_PLAY_FUN_PCT: u8,
    DOSE_PLAY_ENERGY_PCT: u8,
    DOSE_PLAY_FULL_PCT: u8,
    DOSE_PLAY_WATER_PCT: u8,
    DOSE_PLAY_FIT_PCT: u8,
    DOSE_PLAY_DIRT_PCT: u8,
    DOSE_PLAY_GROGGY_MIN: u16,
    DOSE_TIRED_FUN_MULT: u8,
    DOSE_MINI_FUN_BASE_PCT: u8,
    DOSE_MINI_FUN_GRADE_PCT: u8,
    DOSE_MINI_ENERGY_PCT: u8,
    DOSE_EXERCISE_STRAIN_PCT: u8,
    DOSE_STRAIN_NEED_PCT: u16,
    DOSE_STRAIN_RESTED_PCT: u16,
    DOSE_STRAIN_FIT_PCT: u16,
    DOSE_STRAIN_FIT_MIN_PCT: u8,
    DOSE_STRAIN_HEAVY_PCT: u16,
    DOSE_STRAIN_DEPRIVED_PCT: u16,
    DOSE_EXERCISE_ENERGY_PCT: u8,
    DOSE_EXERCISE_FULL_PCT: u8,
    DOSE_EXERCISE_WATER_PCT: u8,
    DOSE_EXERCISE_FUN_PCT: u8,
    DOSE_EXERCISE_BOOZE_PCT: u8,
    DOSE_EXERCISE_DIRT_PCT: u8,
    DOSE_EXERCISE_WEIGHT_PCT: u8,
    DOSE_EXERCISE_HANGOVER_MIN: u16,
    DOSE_EXERCISE_FIT_PCT: u8,
    DOSE_INJURY_MIN: u16,
    DOSE_INJURY_HEALTH_PCT: u8,
    DOSE_WASH_DIRT_PCT: u8,
    DOSE_RELAX_FUN_PCT: u8,
    DOSE_RELAX_STRAIN_PCT: u8,
    DOSE_RELAX_ENERGY_PCT: u8,
    DOSE_RELAX_FIT_PCT: u8,
    DOSE_MED_CD_MIN: u16,
    DOSE_HEAL_INJURY_MIN: u16,
    DOSE_MED_BOOZE_PCT: u8,
    DOSE_HEAL_HEALTHY_PCT: u8,
    DOSE_HEAL_HEALTH_PCT: u8,
    DOSE_INSULIN_WINDOW_MIN: u16,
    DOSE_INSULIN_MIN: u16,
    DOSE_OZEMPIC_WEIGHT_PCT: u8,
    DOSE_OZEMPIC_FIT_PCT: u8,
    DOSE_OZEMPIC_MIN: u16,
    DOSE_OZEMPIC_CD_MIN: u16,
    DOSE_REHAB_MIN: u16,
    DOSE_NIGHT_ENERGY_PCT: u8,
    DOSE_WORK_ENERGY_PCT: u8,
    DOSE_WORK_FUN_PCT: u8,
    DOSE_WORK_MOOD_PCT: u8,
    DOSE_FRIEND_BIG_FUN_PCT: u8,
    DOSE_FRIEND_SMALL_FUN_PCT: u8,
    DEMO_FIT_FLOOR_PCT: u8,
    DEMO_HEALTH_FLOOR_PCT: u8,
    DEMO_HELP_HEALTH_PCT: u8,
    DEMO_ON_HEALTH_PCT: u8,
    DEMO_MED_HEALTH_PCT: u8,
    DEMO_WAKE_FULL_PCT: u8,
    DEMO_EDGE_FULL_PCT: u8,
    DEMO_EDGE_WATER_PCT: u8,
    DEMO_EDGE_ENERGY_PCT: u8,
    DEMO_EDGE_BOOZE_PCT: u8,
    DEMO_SOFT_WATER_PCT: u8,
    DEMO_SOFT_FULL_PCT: u8,
    DEMO_SOFT_ENERGY_PCT: u8,
    DEMO_SOFT_DIRT_PCT: u8,
    DEMO_SOFT_FUN_PCT: u8,
    DEMO_PLAY_ENERGY_PCT: u8,
    DEMO_WIRED_FUN_PCT: u8,
    DEMO_RISE_ENERGY_PCT: u8,
    DEMO_NAP_ENERGY_PCT: u8,
    DEMO_NAP_FUN_PCT: u8,
    DEMO_WHIM_BOOZE_LO_PCT: u8,
    DEMO_WHIM_BOOZE_HI_PCT: u8,
    DEMO_WHIM_BEER_LO_PCT: u8,
    DEMO_WHIM_BEER_HI_PCT: u8,
    DEMO_BEER_STOP_MIN: u16,
    DEMO_LIGHT_FOOD_PCT: u8,
    DEMO_RICH_FOOD_PCT: u8,
    DEMO_REACT_BASE_MIN: u16,
    DEMO_REACT_SPREAD_MIN: u16,
    DEMO_DRY_BASE_MIN: u16,
    DEMO_DRY_SPREAD_MIN: u16,
    DEMO_WHIM_BASE_MIN: u16,
    DEMO_WHIM_SPREAD_MIN: u16,
    DEMO_LAZY_MIN: u16,
    DEMO_LAZY_MAX_PCT: u8,
    DEMO_NAP_BASE_MIN: u16,
    DEMO_NAP_SPREAD_MIN: u16,
    DEMO_EDGE_NAP_MIN: u16,
    DEMO_RISE_BASE_MIN: u16,
    DEMO_RISE_SPREAD_MIN: u16,
    DEMO_BED_AT_MIN: u16,
    DEMO_BED_SPREAD_MIN: u16,
    DEMO_UP_AT_MIN: u16,
    DEMO_UP_SPREAD_MIN: u16,
    DEMO_PAUSE_MIN: u8,
    DEMO_PICK_APPLE_PCT: u8,
    DEMO_PICK_SALAD_PCT: u8,
    DEMO_PICK_PIZZA_PCT: u8,
    DEMO_PICK_FRIK_PCT: u8,
    DEMO_PICK_WATER_PCT: u8,
    DEMO_PICK_COFFEE_PCT: u8,
    DEMO_PICK_GAME_PCT: u8,
    DEMO_HAX_TARGET: u16,
    DEMO_HAX_DRIFT_MIN: u16,
    // Backgrounds (plan 7). Display lines, not balance.
    BG_NIGHT_FROM_MIN: u16,
    BG_NIGHT_TO_MIN: u16,
    FLOOR_CRUMBS_DIRT_PCT: u8,
    FLOOR_PEEL_DIRT_PCT: u8,
    FLOOR_CAN_DIRT_PCT: u8,
    FLOOR_SOCK_DIRT_PCT: u8,
    FLOOR_PIZZA_DIRT_PCT: u8,
}

// ---------------------------------------------------------------------------
// Presets
// ---------------------------------------------------------------------------

impl Thresholds {
    /// The original balance — what the badge ships with.
    pub const CLASSIC: Thresholds = Thresholds {
        FEED_DURATION: 2,
        HEAL_DURATION: 3,
        PLAY_DURATION: 4,
        MINIGAME_COOLDOWN: 18,

        #[cfg(not(feature = "simulator"))]
        HATCHING_TICKS: 6,
        #[cfg(feature = "simulator")]
        HATCHING_TICKS: 1,
        MAX_SLEEP_TICKS: 720,
        SAVE_INTERVAL_TICKS: 90,

        MIN_TRAIT: 16384,
        MAX_TRAIT: 49152,

        EXERCISE_DURATION: 3,

        MEDICATE_DURATION: 2,

        OZEMPIC_DURATION: 2,

        DRINK_DURATION: 2,

        REHAB_DURATION: 3,

        BATTLE_COOLDOWN: 180,
        BATTLE_POWER_BASE_PTS: 22,
        BATTLE_POWER_WEIGHT_PCT: 28,
        BATTLE_COURAGE_PEAK_PTS: 8,
        BATTLE_HIGH_MOOD_PCT: 80,
        BATTLE_HIGH_MOOD_PTS: 3,
        BATTLE_LOW_MOOD_PCT: 40,
        BATTLE_GUARD_WEIGHT_PCT: 26,
        BATTLE_GUARD_BASE_PTS: 7,
        BATTLE_SPEED_BASE_PTS: 25,
        BATTLE_SPEED_FIT_PCT: 40,
        BATTLE_RESTED_SPEED_PTS: 5,
        BATTLE_RESTED_FOCUS_PTS: 5,
        BATTLE_GROGGY_SPEED_PTS: 10,
        BATTLE_GROGGY_FOCUS_PTS: 5,
        BATTLE_HANGOVER_SPEED_PTS: 8,
        BATTLE_HANGOVER_FOCUS_PTS: 12,
        BATTLE_CRAVING_FOCUS_PTS: 10,
        BATTLE_CRAVING_POWER_PTS: 3,
        BATTLE_HEAVY_UNFIT_FIT_PCT: 50,
        BATTLE_SLOPPY_PCT: 40,
        BATTLE_FELT_BASE_PCT: 150,
        BATTLE_HP_BASE_PTS: 45,
        BATTLE_HP_WEIGHT_PCT: 25,
        BATTLE_POWER_FLOOR_PTS: 33,
        BATTLE_GUARD_FLOOR_PTS: 14,
        BATTLE_HP_FLOOR_PTS: 60,
        BATTLE_STAMINA_FLOOR_PTS: 1,
        BATTLE_INJURED_PCT: 60,
        BATTLE_FORM_TOP_PCT: 65,
        BATTLE_FORM_IN_PCT: 50,
        BATTLE_FORM_OFF_PCT: 30,
        BATTLE_PAIR_COOLDOWN_MIN: 60,
        BATTLE_ENERGY_PCT: 10,
        BATTLE_WATER_PCT: 8,
        BATTLE_FUN_PCT: 20,
        BATTLE_DIRT_PCT: 8,
        BATTLE_STRAIN_PCT: 20,
        BATTLE_STRAIN_NEED_PCT: 150,
        BATTLE_STRAIN_RESTED_PCT: 70,
        BATTLE_BRUISE_MAX_PCT: 5,
        BATTLE_MOOD_PCT: 8,
        BATTLE_MIN_HEALTH_PCT: 30,
        BATTLE_DRUNK_PCT: 70,
        BATTLE_PRIZE_FIRST_HAX: 100,
        BATTLE_PRIZE_LATER_HAX: 20,
        BATTLE_PRIZE_BEACON_MIN: 30,
        BATTLE_DECLINE_MOOD_PCT: 3,
        BATTLE_DECLINE_MIN: 60,

        FULL_LO_PCT: 38,
        FULL_HI_PCT: 78,
        WATER_LO_PCT: 38,
        WATER_HI_PCT: 83,
        ENERGY_LO_PCT: 33,
        FUN_LO_PCT: 33,
        FUN_HI_PCT: 73,
        DIRT_HI_PCT: 58,
        BOOZE_HI_PCT: 38,
        BOOZE_DANGER_PCT: 70,
        STRAIN_INJ_PCT: 60,
        FIT_LO_PCT: 43,
        WEIGHT_LO_PCT: 38,
        WEIGHT_HI_PCT: 63,

        // v3 regime rates, see engine::regime
        FULL_DRAIN_AWAKE_PM: 54,
        WATER_DRAIN_AWAKE_PM: 45,
        ENERGY_DRAIN_AWAKE_PM: 36,
        FUN_DRAIN_PM: 40,
        FULL_DRAIN_ASLEEP_PM: 30,
        WATER_DRAIN_ASLEEP_PM: 20,
        ENERGY_GAIN_ASLEEP_PM: 120,
        SLEEP_FUN_FLOOR_PCT: 40,
        DIRT_GAIN_PM: 8,
        BOOZE_CLEAR_PM: 36,
        BOOZE_CLEAR_SLOW_PCT: 67, // approximates 2/3; 36 -> 24 -> 16
        STRAIN_CLEAR_PM: 80,
        FIT_DECAY_HIGH_PM: 4,
        FIT_DECAY_MID_PM: 2,
        FIT_DECAY_LOW_PM: 1,
        FIT_DECAY_HIGH_ABOVE_PCT: 50,
        FIT_DECAY_MID_ABOVE_PCT: 25,
        FIT_DECAY_HEAVY_PCT: 150,
        WEIGHT_PULL_HEAVY_PM: 3,
        WEIGHT_PULL_PM: 1,
        WEIGHT_BELLY_PM: 3,
        ENERGY_MULT_UNFIT_PCT: 125,
        ENERGY_MULT_FIT_PCT: 90,
        ENERGY_FIT_GOOD_PCT: 70,
        ENERGY_MULT_DRUNK_PCT: 150,
        ENERGY_MULT_WIRED_PCT: 150,
        ENERGY_MULT_HANGOVER_PCT: 125,
        ENERGY_MULT_WEIGHT_PCT: 115,
        ENERGY_CAP_DEPRIVED_PCT: 70,
        THIRST_MULT_BASE_PCT: 100,
        THIRST_MULT_LOW_PCT: 110,
        THIRST_MULT_MID_PCT: 125,
        THIRST_MULT_HIGH_PCT: 140,
        THIRST_BOOZE_LOW_PCT: 15,
        THIRST_BOOZE_MID_PCT: 40,
        THIRST_BOOZE_HIGH_PCT: 70,
        SLEEP_QUALITY_POOR_PM: 600,
        SLEEP_QUALITY_GOOD_PM: 1000,
        UNSETTLED_BOOZE_PCT: 30,
        UNSETTLED_FULL_LO_PCT: 25,
        UNSETTLED_FULL_HI_PCT: 85,
        UNSETTLED_WATER_LO_PCT: 25,
        UNSETTLED_WATER_HI_PCT: 90,

        // v3 mood / health terms
        MOOD_STEP: 120,
        MOOD_PEN_INJURED_PCT: 20,
        MOOD_PEN_GROGGY_PCT: 10,
        MOOD_PEN_HANGOVER_PCT: 15,
        MOOD_PEN_DEPRIVED_PCT: 10,
        MOOD_PEN_REHAB_PCT: 20,
        MOOD_PEN_CRAVING_PCT: 15,
        MOOD_CRAVING_BOOZE_PCT: 10,
        MOOD_HEALTH_FLOOR_PCT: 50,
        CEILING_FIT_STEP1_PCT: 25,
        CEILING_FIT_STEP2_PCT: 50,
        CEILING_FIT_STEP3_PCT: 70,
        CEILING_STEP0_PCT: 55,
        CEILING_STEP1_PCT: 65,
        CEILING_STEP2_PCT: 80,
        CEILING_STEP3_PCT: 100,
        CEILING_DIABETIC_PCT: 20,
        DMG_NEED_LOW_PCT: 15,
        DMG_NEED_HIGH_PCT: 95,
        DMG_ENERGY_PCT: 10,
        DMG_ENERGY_HEALTH_PCT: 30,
        DMG_MOOD_PCT: 20,
        DMG_DIRT_PCT: 85,
        DMG_WEIGHT_HIGH_PCT: 80,
        DMG_WEIGHT_LOW_PCT: 20,
        DMG_FULL_LOW_PM: 40,
        DMG_FULL_HIGH_PM: 8,
        DMG_WATER_LOW_PM: 40,
        DMG_WATER_HIGH_PM: 8,
        DMG_ENERGY_PM: 8,
        DMG_BOOZE_PM: 40,
        DMG_MOOD_PM: 10,
        DMG_DIRT_PM: 10,
        DMG_WEIGHT_HIGH_PM: 2,
        DMG_WEIGHT_LOW_PM: 10,
        REC_GOOD_PM: 14,
        REC_FAIR_PM: 5,
        REC_GOOD_MOOD_PCT: 60,
        REC_FAIR_MOOD_PCT: 40,
        REC_DEPRIVED_PCT: 50,
        // v3 advance
        DEPENDENCE_ONSET_MIN: 1200,
        DEPENDENCE_RELAPSE_MIN: 400,
        DIABETES_ONSET_MIN: 4320,
        DIABETES_REVERSAL_MIN: 10080,
        HANGOVER_MIN: 480,
        RECOVERING_MIN: 10080,
        SELF_WAKE_MIN: 720,
        OVERSLEEP_WAKE_MIN: 300,
        GROGGY_LINE_MIN: 240,
        GROGGY_MIN: 120,
        GROGGY_ENERGY_PCT: 10,
        GROGGY_FIT_PCT: 2,
        RESTED_MIN: 960,
        GOOD_SLEEP_LINE_MIN: 300,
        NIGHT_SHORT_MIN: 360,
        NIGHT_GOOD_MIN: 390,
        DEPRIVED_NIGHTS: 3,
        CLEARING_NIGHTS: 2,
        GRUMPY_PCT: 10,
        GRUMPY_ENERGY_PCT: 50,
        COLLAPSE_DAMAGE: 1965,
        COLLAPSE_HEALTH_FLOOR_PCT: 33,
        LEAVING_MIN: 720,
        BOOZE_CLEAR_PCT: 15,
        DIABETES_WEIGHT_HI_PCT: 80,
        DIABETES_WEIGHT_LO_PCT: 65,
        REVERSAL_FIT_PCT: 60,
        HANGOVER_ASLEEP_MULT: 2,
        HEALTH_EXCESS_DECAY_PM: 3,
        CARE_GOOD_MOOD_PCT: 60,
        OVERSLEEP_RESET_PCT: 85,

        // v3 doses: food and drink
        DOSE_SALAD_FULL_PCT: 20,
        DOSE_APPLE_FULL_PCT: 25,
        DOSE_FRIKANDEL_FULL_PCT: 35,
        DOSE_PIZZA_FULL_PCT: 40,
        DOSE_CAKE_FULL_PCT: 10,
        DOSE_SALAD_DIRT_PCT: 10,
        DOSE_APPLE_DIRT_PCT: 10,
        DOSE_FRIKANDEL_DIRT_PCT: 10,
        DOSE_PIZZA_DIRT_PCT: 10,
        DOSE_CAKE_DIRT_PCT: 5,
        DOSE_SALAD_WEIGHT_PM: 0,
        DOSE_APPLE_WEIGHT_PM: 5,
        DOSE_FRIKANDEL_WEIGHT_PM: 20,
        DOSE_PIZZA_WEIGHT_PM: 20,
        DOSE_CAKE_WEIGHT_PM: 30,
        DOSE_FRIKANDEL_MOOD_PCT: 6,
        DOSE_PIZZA_MOOD_PCT: 8,
        DOSE_CAKE_MOOD_PCT: 15,
        DOSE_HEALTHY_WATER_PCT: 8,
        DOSE_SALAD_FIT_PCT: 1,
        DOSE_JUNK_REPEAT_DIV: 3,
        DOSE_JUNK_MIN: 180,
        DOSE_FULL_CAP_PCT: 120,
        DOSE_EXCESS_ENERGY_DIV: 2,
        DOSE_EXCESS_WEIGHT_DIV: 4,
        DOSE_WATER_MAX_PCT: 30,
        DOSE_WATER_BELLY_PCT: 2,
        DOSE_WATER_BELLY_MIN_PCT: 15,
        DOSE_WATER_BOOZE_PCT: 5,
        DOSE_WATER_HANGOVER_MIN: 60,
        DOSE_WATER_GROGGY_MIN: 30,
        DOSE_COFFEE_WATER_PCT: 20,
        DOSE_COFFEE_FULL_PCT: 2,
        DOSE_COFFEE_ENERGY_PCT: 8,
        DOSE_COFFEE_MOOD_PCT: 3,
        DOSE_COFFEE_WEIGHT_PCT: 0,
        COFFEE_WINDOW_MIN: 240,
        COFFEE_WIRED_CUPS: 3,
        COFFEE_WIRED_MIN: 120,
        DOSE_BEER_WATER_PCT: 20,
        DOSE_BEER_BELLY_PCT: 2,
        DOSE_BEER_BOOZE_PCT: 15,
        DOSE_BEER_MOOD_PCT: 12,
        DOSE_WINE_WATER_PCT: 8,
        DOSE_WINE_BELLY_PCT: 1,
        DOSE_WINE_BOOZE_PCT: 20,
        DOSE_WINE_MOOD_PCT: 15,
        DOSE_WHISKEY_WATER_PCT: 0,
        DOSE_WHISKEY_BELLY_PCT: 0,
        DOSE_WHISKEY_BOOZE_PCT: 30,
        DOSE_WHISKEY_MOOD_PCT: 20,
        DOSE_BEER_WEIGHT_PCT: 1,
        DOSE_ALCOHOL_ENERGY_PCT: 5,
        DOSE_ALCOHOL_FUN_PCT: 5,
        DOSE_TIRED_BOOZE_PCT: 150,
        DOSE_DEPENDENT_DIV: 2,
        DOSE_RELAPSE_MOOD_PCT: 20,
        DOSE_PLAY_FUN_PCT: 25,
        DOSE_PLAY_ENERGY_PCT: 5,
        DOSE_PLAY_FULL_PCT: 3,
        DOSE_PLAY_WATER_PCT: 4,
        DOSE_PLAY_FIT_PCT: 2,
        DOSE_PLAY_DIRT_PCT: 3,
        DOSE_PLAY_GROGGY_MIN: 60,
        DOSE_TIRED_FUN_MULT: 2,
        DOSE_MINI_FUN_BASE_PCT: 10,
        DOSE_MINI_FUN_GRADE_PCT: 7,
        DOSE_MINI_ENERGY_PCT: 3,
        DOSE_EXERCISE_STRAIN_PCT: 35,
        DOSE_STRAIN_NEED_PCT: 150,
        DOSE_STRAIN_RESTED_PCT: 70,
        DOSE_STRAIN_FIT_PCT: 85,
        DOSE_STRAIN_FIT_MIN_PCT: 70,
        DOSE_STRAIN_HEAVY_PCT: 120,
        DOSE_STRAIN_DEPRIVED_PCT: 125,
        DOSE_EXERCISE_ENERGY_PCT: 10,
        DOSE_EXERCISE_FULL_PCT: 8,
        DOSE_EXERCISE_WATER_PCT: 12,
        DOSE_EXERCISE_FUN_PCT: 10,
        DOSE_EXERCISE_BOOZE_PCT: 5,
        DOSE_EXERCISE_DIRT_PCT: 12,
        DOSE_EXERCISE_WEIGHT_PCT: 3,
        DOSE_EXERCISE_HANGOVER_MIN: 120,
        DOSE_EXERCISE_FIT_PCT: 12,
        DOSE_INJURY_MIN: 1440,
        DOSE_INJURY_HEALTH_PCT: 5,
        DOSE_WASH_DIRT_PCT: 70,
        DOSE_RELAX_FUN_PCT: 25,
        DOSE_RELAX_STRAIN_PCT: 15,
        DOSE_RELAX_ENERGY_PCT: 3,
        DOSE_RELAX_FIT_PCT: 1,
        DOSE_MED_CD_MIN: 240,
        DOSE_HEAL_INJURY_MIN: 720,
        DOSE_MED_BOOZE_PCT: 10,
        DOSE_HEAL_HEALTHY_PCT: 80,
        DOSE_HEAL_HEALTH_PCT: 10,
        DOSE_INSULIN_WINDOW_MIN: 120,
        DOSE_INSULIN_MIN: 480,
        DOSE_OZEMPIC_WEIGHT_PCT: 15,
        DOSE_OZEMPIC_FIT_PCT: 5,
        DOSE_OZEMPIC_MIN: 1440,
        DOSE_OZEMPIC_CD_MIN: 2880,
        DOSE_REHAB_MIN: 4320,
        DOSE_NIGHT_ENERGY_PCT: 50,
        DOSE_WORK_ENERGY_PCT: 15,
        DOSE_WORK_FUN_PCT: 10,
        DOSE_WORK_MOOD_PCT: 5,
        DOSE_FRIEND_BIG_FUN_PCT: 15,
        DOSE_FRIEND_SMALL_FUN_PCT: 5,
        DEMO_FIT_FLOOR_PCT: 45,
        DEMO_HEALTH_FLOOR_PCT: 25,
        DEMO_HELP_HEALTH_PCT: 35,
        DEMO_ON_HEALTH_PCT: 40,
        DEMO_MED_HEALTH_PCT: 45,
        DEMO_WAKE_FULL_PCT: 18,
        DEMO_EDGE_FULL_PCT: 20,
        DEMO_EDGE_WATER_PCT: 22,
        DEMO_EDGE_ENERGY_PCT: 12,
        DEMO_EDGE_BOOZE_PCT: 55,
        DEMO_SOFT_WATER_PCT: 39,
        DEMO_SOFT_FULL_PCT: 27,
        DEMO_SOFT_ENERGY_PCT: 27,
        DEMO_SOFT_DIRT_PCT: 68,
        DEMO_SOFT_FUN_PCT: 15,
        DEMO_PLAY_ENERGY_PCT: 25,
        DEMO_WIRED_FUN_PCT: 78,
        DEMO_RISE_ENERGY_PCT: 97,
        DEMO_NAP_ENERGY_PCT: 55,
        DEMO_NAP_FUN_PCT: 45,
        DEMO_WHIM_BOOZE_LO_PCT: 15,
        DEMO_WHIM_BOOZE_HI_PCT: 45,
        DEMO_WHIM_BEER_LO_PCT: 50,
        DEMO_WHIM_BEER_HI_PCT: 35,
        DEMO_BEER_STOP_MIN: 300,
        DEMO_LIGHT_FOOD_PCT: 56,
        DEMO_RICH_FOOD_PCT: 42,
        DEMO_REACT_BASE_MIN: 30,
        DEMO_REACT_SPREAD_MIN: 80,
        DEMO_DRY_BASE_MIN: 20,
        DEMO_DRY_SPREAD_MIN: 40,
        DEMO_WHIM_BASE_MIN: 180,
        DEMO_WHIM_SPREAD_MIN: 600,
        DEMO_LAZY_MIN: 360,
        DEMO_LAZY_MAX_PCT: 15,
        DEMO_NAP_BASE_MIN: 40,
        DEMO_NAP_SPREAD_MIN: 60,
        DEMO_EDGE_NAP_MIN: 120,
        DEMO_RISE_BASE_MIN: 10,
        DEMO_RISE_SPREAD_MIN: 110,
        DEMO_BED_AT_MIN: 1320,
        DEMO_BED_SPREAD_MIN: 90,
        DEMO_UP_AT_MIN: 420,
        DEMO_UP_SPREAD_MIN: 90,
        DEMO_PAUSE_MIN: 30,
        DEMO_PICK_APPLE_PCT: 40,
        DEMO_PICK_SALAD_PCT: 15,
        DEMO_PICK_PIZZA_PCT: 25,
        DEMO_PICK_FRIK_PCT: 12,
        DEMO_PICK_WATER_PCT: 70,
        DEMO_PICK_COFFEE_PCT: 20,
        DEMO_PICK_GAME_PCT: 30,
        DEMO_HAX_TARGET: 100,
        DEMO_HAX_DRIFT_MIN: 10,
        BG_NIGHT_FROM_MIN: 1260,
        BG_NIGHT_TO_MIN: 420,
        FLOOR_CRUMBS_DIRT_PCT: 40,
        FLOOR_PEEL_DIRT_PCT: 55,
        FLOOR_CAN_DIRT_PCT: 70,
        FLOOR_SOCK_DIRT_PCT: 85,
        FLOOR_PIZZA_DIRT_PCT: 95,
    };

    /// Half-speed decay, doubled lifetimes, more generous action relief.
    /// Aim: a forgiving balance for badge holders who don't want to baby-sit.
    pub const CASUAL: Thresholds = Thresholds {
        // Everything not listed below is identical to CLASSIC.
        FEED_DURATION: 2,
        HEAL_DURATION: 3,
        PLAY_DURATION: 4,
        MINIGAME_COOLDOWN: 12,

        #[cfg(not(feature = "simulator"))]
        HATCHING_TICKS: 6,
        #[cfg(feature = "simulator")]
        HATCHING_TICKS: 1,
        MAX_SLEEP_TICKS: 720,
        SAVE_INTERVAL_TICKS: 90,

        MIN_TRAIT: 16384,
        MAX_TRAIT: 49152,

        EXERCISE_DURATION: 3,

        MEDICATE_DURATION: 2,

        OZEMPIC_DURATION: 2,

        DRINK_DURATION: 2,

        REHAB_DURATION: 3,

        BATTLE_COOLDOWN: 180,
        BATTLE_POWER_BASE_PTS: 22,
        BATTLE_POWER_WEIGHT_PCT: 28,
        BATTLE_COURAGE_PEAK_PTS: 8,
        BATTLE_HIGH_MOOD_PCT: 80,
        BATTLE_HIGH_MOOD_PTS: 3,
        BATTLE_LOW_MOOD_PCT: 40,
        BATTLE_GUARD_WEIGHT_PCT: 26,
        BATTLE_GUARD_BASE_PTS: 7,
        BATTLE_SPEED_BASE_PTS: 25,
        BATTLE_SPEED_FIT_PCT: 40,
        BATTLE_RESTED_SPEED_PTS: 5,
        BATTLE_RESTED_FOCUS_PTS: 5,
        BATTLE_GROGGY_SPEED_PTS: 10,
        BATTLE_GROGGY_FOCUS_PTS: 5,
        BATTLE_HANGOVER_SPEED_PTS: 8,
        BATTLE_HANGOVER_FOCUS_PTS: 12,
        BATTLE_CRAVING_FOCUS_PTS: 10,
        BATTLE_CRAVING_POWER_PTS: 3,
        BATTLE_HEAVY_UNFIT_FIT_PCT: 50,
        BATTLE_SLOPPY_PCT: 40,
        BATTLE_FELT_BASE_PCT: 150,
        BATTLE_HP_BASE_PTS: 45,
        BATTLE_HP_WEIGHT_PCT: 25,
        BATTLE_POWER_FLOOR_PTS: 33,
        BATTLE_GUARD_FLOOR_PTS: 14,
        BATTLE_HP_FLOOR_PTS: 60,
        BATTLE_STAMINA_FLOOR_PTS: 1,
        BATTLE_INJURED_PCT: 60,
        BATTLE_FORM_TOP_PCT: 65,
        BATTLE_FORM_IN_PCT: 50,
        BATTLE_FORM_OFF_PCT: 30,
        BATTLE_PAIR_COOLDOWN_MIN: 60,
        BATTLE_ENERGY_PCT: 10,
        BATTLE_WATER_PCT: 8,
        BATTLE_FUN_PCT: 20,
        BATTLE_DIRT_PCT: 8,
        BATTLE_STRAIN_PCT: 20,
        BATTLE_STRAIN_NEED_PCT: 150,
        BATTLE_STRAIN_RESTED_PCT: 70,
        BATTLE_BRUISE_MAX_PCT: 2,
        BATTLE_MOOD_PCT: 8,
        BATTLE_MIN_HEALTH_PCT: 30,
        BATTLE_DRUNK_PCT: 70,
        BATTLE_PRIZE_FIRST_HAX: 100,
        BATTLE_PRIZE_LATER_HAX: 20,
        BATTLE_PRIZE_BEACON_MIN: 30,
        BATTLE_DECLINE_MOOD_PCT: 3,
        BATTLE_DECLINE_MIN: 60,

        FULL_LO_PCT: 33,
        FULL_HI_PCT: 83,
        WATER_LO_PCT: 33,
        WATER_HI_PCT: 88,
        ENERGY_LO_PCT: 28,
        FUN_LO_PCT: 28,
        FUN_HI_PCT: 78,
        DIRT_HI_PCT: 63,
        BOOZE_HI_PCT: 43,
        BOOZE_DANGER_PCT: 75,
        STRAIN_INJ_PCT: 65,
        FIT_LO_PCT: 38,
        WEIGHT_LO_PCT: 33,
        WEIGHT_HI_PCT: 68,

        // v3 regime rates, see engine::regime
        FULL_DRAIN_AWAKE_PM: 54,
        WATER_DRAIN_AWAKE_PM: 45,
        ENERGY_DRAIN_AWAKE_PM: 36,
        FUN_DRAIN_PM: 40,
        FULL_DRAIN_ASLEEP_PM: 30,
        WATER_DRAIN_ASLEEP_PM: 20,
        ENERGY_GAIN_ASLEEP_PM: 120,
        SLEEP_FUN_FLOOR_PCT: 40,
        DIRT_GAIN_PM: 8,
        BOOZE_CLEAR_PM: 36,
        BOOZE_CLEAR_SLOW_PCT: 67, // approximates 2/3; 36 -> 24 -> 16
        STRAIN_CLEAR_PM: 80,
        FIT_DECAY_HIGH_PM: 4,
        FIT_DECAY_MID_PM: 2,
        FIT_DECAY_LOW_PM: 1,
        FIT_DECAY_HIGH_ABOVE_PCT: 50,
        FIT_DECAY_MID_ABOVE_PCT: 25,
        FIT_DECAY_HEAVY_PCT: 150,
        WEIGHT_PULL_HEAVY_PM: 3,
        WEIGHT_PULL_PM: 1,
        WEIGHT_BELLY_PM: 3,
        ENERGY_MULT_UNFIT_PCT: 125,
        ENERGY_MULT_FIT_PCT: 90,
        ENERGY_FIT_GOOD_PCT: 70,
        ENERGY_MULT_DRUNK_PCT: 150,
        ENERGY_MULT_WIRED_PCT: 150,
        ENERGY_MULT_HANGOVER_PCT: 125,
        ENERGY_MULT_WEIGHT_PCT: 115,
        ENERGY_CAP_DEPRIVED_PCT: 70,
        THIRST_MULT_BASE_PCT: 100,
        THIRST_MULT_LOW_PCT: 110,
        THIRST_MULT_MID_PCT: 125,
        THIRST_MULT_HIGH_PCT: 140,
        THIRST_BOOZE_LOW_PCT: 15,
        THIRST_BOOZE_MID_PCT: 40,
        THIRST_BOOZE_HIGH_PCT: 70,
        SLEEP_QUALITY_POOR_PM: 600,
        SLEEP_QUALITY_GOOD_PM: 1000,
        UNSETTLED_BOOZE_PCT: 30,
        UNSETTLED_FULL_LO_PCT: 25,
        UNSETTLED_FULL_HI_PCT: 85,
        UNSETTLED_WATER_LO_PCT: 25,
        UNSETTLED_WATER_HI_PCT: 90,

        // v3 mood / health terms
        MOOD_STEP: 120,
        MOOD_PEN_INJURED_PCT: 20,
        MOOD_PEN_GROGGY_PCT: 10,
        MOOD_PEN_HANGOVER_PCT: 15,
        MOOD_PEN_DEPRIVED_PCT: 10,
        MOOD_PEN_REHAB_PCT: 20,
        MOOD_PEN_CRAVING_PCT: 15,
        MOOD_CRAVING_BOOZE_PCT: 10,
        MOOD_HEALTH_FLOOR_PCT: 50,
        CEILING_FIT_STEP1_PCT: 25,
        CEILING_FIT_STEP2_PCT: 50,
        CEILING_FIT_STEP3_PCT: 70,
        CEILING_STEP0_PCT: 55,
        CEILING_STEP1_PCT: 65,
        CEILING_STEP2_PCT: 80,
        CEILING_STEP3_PCT: 100,
        CEILING_DIABETIC_PCT: 20,
        DMG_NEED_LOW_PCT: 15,
        DMG_NEED_HIGH_PCT: 95,
        DMG_ENERGY_PCT: 10,
        DMG_ENERGY_HEALTH_PCT: 30,
        DMG_MOOD_PCT: 20,
        DMG_DIRT_PCT: 85,
        DMG_WEIGHT_HIGH_PCT: 80,
        DMG_WEIGHT_LOW_PCT: 20,
        DMG_FULL_LOW_PM: 20,
        DMG_FULL_HIGH_PM: 4,
        DMG_WATER_LOW_PM: 20,
        DMG_WATER_HIGH_PM: 4,
        DMG_ENERGY_PM: 4,
        DMG_BOOZE_PM: 20,
        DMG_MOOD_PM: 5,
        DMG_DIRT_PM: 5,
        DMG_WEIGHT_HIGH_PM: 1,
        DMG_WEIGHT_LOW_PM: 5,
        REC_GOOD_PM: 14,
        REC_FAIR_PM: 5,
        REC_GOOD_MOOD_PCT: 60,
        REC_FAIR_MOOD_PCT: 40,
        REC_DEPRIVED_PCT: 50,
        // v3 advance (collapse health loss halved like every damage rate)
        DEPENDENCE_ONSET_MIN: 1200,
        DEPENDENCE_RELAPSE_MIN: 400,
        DIABETES_ONSET_MIN: 4320,
        DIABETES_REVERSAL_MIN: 10080,
        HANGOVER_MIN: 480,
        RECOVERING_MIN: 10080,
        SELF_WAKE_MIN: 720,
        OVERSLEEP_WAKE_MIN: 300,
        GROGGY_LINE_MIN: 240,
        GROGGY_MIN: 120,
        GROGGY_ENERGY_PCT: 10,
        GROGGY_FIT_PCT: 2,
        RESTED_MIN: 960,
        GOOD_SLEEP_LINE_MIN: 300,
        NIGHT_SHORT_MIN: 360,
        NIGHT_GOOD_MIN: 390,
        DEPRIVED_NIGHTS: 3,
        CLEARING_NIGHTS: 2,
        GRUMPY_PCT: 10,
        GRUMPY_ENERGY_PCT: 50,
        COLLAPSE_DAMAGE: 982,
        COLLAPSE_HEALTH_FLOOR_PCT: 33,
        LEAVING_MIN: 720,
        BOOZE_CLEAR_PCT: 15,
        DIABETES_WEIGHT_HI_PCT: 80,
        DIABETES_WEIGHT_LO_PCT: 65,
        REVERSAL_FIT_PCT: 60,
        HANGOVER_ASLEEP_MULT: 2,
        HEALTH_EXCESS_DECAY_PM: 3,
        CARE_GOOD_MOOD_PCT: 60,
        OVERSLEEP_RESET_PCT: 85,

        // v3 doses: food and drink
        DOSE_SALAD_FULL_PCT: 20,
        DOSE_APPLE_FULL_PCT: 25,
        DOSE_FRIKANDEL_FULL_PCT: 35,
        DOSE_PIZZA_FULL_PCT: 40,
        DOSE_CAKE_FULL_PCT: 10,
        DOSE_SALAD_DIRT_PCT: 10,
        DOSE_APPLE_DIRT_PCT: 10,
        DOSE_FRIKANDEL_DIRT_PCT: 10,
        DOSE_PIZZA_DIRT_PCT: 10,
        DOSE_CAKE_DIRT_PCT: 5,
        DOSE_SALAD_WEIGHT_PM: 0,
        DOSE_APPLE_WEIGHT_PM: 5,
        DOSE_FRIKANDEL_WEIGHT_PM: 20,
        DOSE_PIZZA_WEIGHT_PM: 20,
        DOSE_CAKE_WEIGHT_PM: 30,
        DOSE_FRIKANDEL_MOOD_PCT: 6,
        DOSE_PIZZA_MOOD_PCT: 8,
        DOSE_CAKE_MOOD_PCT: 15,
        DOSE_HEALTHY_WATER_PCT: 8,
        DOSE_SALAD_FIT_PCT: 1,
        DOSE_JUNK_REPEAT_DIV: 3,
        DOSE_JUNK_MIN: 180,
        DOSE_FULL_CAP_PCT: 120,
        DOSE_EXCESS_ENERGY_DIV: 2,
        DOSE_EXCESS_WEIGHT_DIV: 4,
        DOSE_WATER_MAX_PCT: 30,
        DOSE_WATER_BELLY_PCT: 2,
        DOSE_WATER_BELLY_MIN_PCT: 15,
        DOSE_WATER_BOOZE_PCT: 5,
        DOSE_WATER_HANGOVER_MIN: 60,
        DOSE_WATER_GROGGY_MIN: 30,
        DOSE_COFFEE_WATER_PCT: 20,
        DOSE_COFFEE_FULL_PCT: 2,
        DOSE_COFFEE_ENERGY_PCT: 8,
        DOSE_COFFEE_MOOD_PCT: 3,
        DOSE_COFFEE_WEIGHT_PCT: 0,
        COFFEE_WINDOW_MIN: 240,
        COFFEE_WIRED_CUPS: 3,
        COFFEE_WIRED_MIN: 120,
        DOSE_BEER_WATER_PCT: 20,
        DOSE_BEER_BELLY_PCT: 2,
        DOSE_BEER_BOOZE_PCT: 15,
        DOSE_BEER_MOOD_PCT: 12,
        DOSE_WINE_WATER_PCT: 8,
        DOSE_WINE_BELLY_PCT: 1,
        DOSE_WINE_BOOZE_PCT: 20,
        DOSE_WINE_MOOD_PCT: 15,
        DOSE_WHISKEY_WATER_PCT: 0,
        DOSE_WHISKEY_BELLY_PCT: 0,
        DOSE_WHISKEY_BOOZE_PCT: 30,
        DOSE_WHISKEY_MOOD_PCT: 20,
        DOSE_BEER_WEIGHT_PCT: 1,
        DOSE_ALCOHOL_ENERGY_PCT: 5,
        DOSE_ALCOHOL_FUN_PCT: 5,
        DOSE_TIRED_BOOZE_PCT: 150,
        DOSE_DEPENDENT_DIV: 2,
        DOSE_RELAPSE_MOOD_PCT: 20,
        DOSE_PLAY_FUN_PCT: 25,
        DOSE_PLAY_ENERGY_PCT: 5,
        DOSE_PLAY_FULL_PCT: 3,
        DOSE_PLAY_WATER_PCT: 4,
        DOSE_PLAY_FIT_PCT: 2,
        DOSE_PLAY_DIRT_PCT: 3,
        DOSE_PLAY_GROGGY_MIN: 60,
        DOSE_TIRED_FUN_MULT: 2,
        DOSE_MINI_FUN_BASE_PCT: 10,
        DOSE_MINI_FUN_GRADE_PCT: 7,
        DOSE_MINI_ENERGY_PCT: 3,
        DOSE_EXERCISE_STRAIN_PCT: 35,
        DOSE_STRAIN_NEED_PCT: 150,
        DOSE_STRAIN_RESTED_PCT: 70,
        DOSE_STRAIN_FIT_PCT: 85,
        DOSE_STRAIN_FIT_MIN_PCT: 70,
        DOSE_STRAIN_HEAVY_PCT: 120,
        DOSE_STRAIN_DEPRIVED_PCT: 125,
        DOSE_EXERCISE_ENERGY_PCT: 10,
        DOSE_EXERCISE_FULL_PCT: 8,
        DOSE_EXERCISE_WATER_PCT: 12,
        DOSE_EXERCISE_FUN_PCT: 10,
        DOSE_EXERCISE_BOOZE_PCT: 5,
        DOSE_EXERCISE_DIRT_PCT: 12,
        DOSE_EXERCISE_WEIGHT_PCT: 3,
        DOSE_EXERCISE_HANGOVER_MIN: 120,
        DOSE_EXERCISE_FIT_PCT: 12,
        DOSE_INJURY_MIN: 1440,
        DOSE_INJURY_HEALTH_PCT: 5,
        DOSE_WASH_DIRT_PCT: 70,
        DOSE_RELAX_FUN_PCT: 25,
        DOSE_RELAX_STRAIN_PCT: 15,
        DOSE_RELAX_ENERGY_PCT: 3,
        DOSE_RELAX_FIT_PCT: 1,
        DOSE_MED_CD_MIN: 240,
        DOSE_HEAL_INJURY_MIN: 720,
        DOSE_MED_BOOZE_PCT: 10,
        DOSE_HEAL_HEALTHY_PCT: 80,
        DOSE_HEAL_HEALTH_PCT: 10,
        DOSE_INSULIN_WINDOW_MIN: 120,
        DOSE_INSULIN_MIN: 480,
        DOSE_OZEMPIC_WEIGHT_PCT: 15,
        DOSE_OZEMPIC_FIT_PCT: 5,
        DOSE_OZEMPIC_MIN: 1440,
        DOSE_OZEMPIC_CD_MIN: 2880,
        DOSE_REHAB_MIN: 4320,
        DOSE_NIGHT_ENERGY_PCT: 50,
        DOSE_WORK_ENERGY_PCT: 15,
        DOSE_WORK_FUN_PCT: 10,
        DOSE_WORK_MOOD_PCT: 5,
        DOSE_FRIEND_BIG_FUN_PCT: 15,
        DOSE_FRIEND_SMALL_FUN_PCT: 5,
        DEMO_FIT_FLOOR_PCT: 40,
        DEMO_HEALTH_FLOOR_PCT: 25,
        DEMO_HELP_HEALTH_PCT: 35,
        DEMO_ON_HEALTH_PCT: 40,
        DEMO_MED_HEALTH_PCT: 45,
        DEMO_WAKE_FULL_PCT: 18,
        DEMO_EDGE_FULL_PCT: 20,
        DEMO_EDGE_WATER_PCT: 22,
        DEMO_EDGE_ENERGY_PCT: 12,
        DEMO_EDGE_BOOZE_PCT: 55,
        DEMO_SOFT_WATER_PCT: 34,
        DEMO_SOFT_FULL_PCT: 22,
        DEMO_SOFT_ENERGY_PCT: 22,
        DEMO_SOFT_DIRT_PCT: 73,
        DEMO_SOFT_FUN_PCT: 10,
        DEMO_PLAY_ENERGY_PCT: 25,
        DEMO_WIRED_FUN_PCT: 83,
        DEMO_RISE_ENERGY_PCT: 97,
        DEMO_NAP_ENERGY_PCT: 55,
        DEMO_NAP_FUN_PCT: 45,
        DEMO_WHIM_BOOZE_LO_PCT: 15,
        DEMO_WHIM_BOOZE_HI_PCT: 45,
        DEMO_WHIM_BEER_LO_PCT: 50,
        DEMO_WHIM_BEER_HI_PCT: 35,
        DEMO_BEER_STOP_MIN: 300,
        DEMO_LIGHT_FOOD_PCT: 56,
        DEMO_RICH_FOOD_PCT: 42,
        DEMO_REACT_BASE_MIN: 30,
        DEMO_REACT_SPREAD_MIN: 80,
        DEMO_DRY_BASE_MIN: 20,
        DEMO_DRY_SPREAD_MIN: 40,
        DEMO_WHIM_BASE_MIN: 180,
        DEMO_WHIM_SPREAD_MIN: 600,
        DEMO_LAZY_MIN: 360,
        DEMO_LAZY_MAX_PCT: 15,
        DEMO_NAP_BASE_MIN: 40,
        DEMO_NAP_SPREAD_MIN: 60,
        DEMO_EDGE_NAP_MIN: 120,
        DEMO_RISE_BASE_MIN: 10,
        DEMO_RISE_SPREAD_MIN: 110,
        DEMO_BED_AT_MIN: 1320,
        DEMO_BED_SPREAD_MIN: 90,
        DEMO_UP_AT_MIN: 420,
        DEMO_UP_SPREAD_MIN: 90,
        DEMO_PAUSE_MIN: 30,
        DEMO_PICK_APPLE_PCT: 40,
        DEMO_PICK_SALAD_PCT: 15,
        DEMO_PICK_PIZZA_PCT: 25,
        DEMO_PICK_FRIK_PCT: 12,
        DEMO_PICK_WATER_PCT: 70,
        DEMO_PICK_COFFEE_PCT: 20,
        DEMO_PICK_GAME_PCT: 30,
        DEMO_HAX_TARGET: 100,
        DEMO_HAX_DRIFT_MIN: 10,
        BG_NIGHT_FROM_MIN: 1260,
        BG_NIGHT_TO_MIN: 420,
        FLOOR_CRUMBS_DIRT_PCT: 40,
        FLOOR_PEEL_DIRT_PCT: 55,
        FLOOR_CAN_DIRT_PCT: 70,
        FLOOR_SOCK_DIRT_PCT: 85,
        FLOOR_PIZZA_DIRT_PCT: 95,
    };
}

// ---------------------------------------------------------------------------
// Mode (which preset is active)
// ---------------------------------------------------------------------------

/// Game-balance preset.  Selected via the badge menu and persisted in KV.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "embassy-core", derive(defmt::Format))]
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
/// preset for the chosen [`Mode`] (e.g. because a `BPETS_V3.CFG` file
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
    super::needs::install_edges(&values);
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
// BPETS_V3.CFG key set — single source of truth for the parser.
// ---------------------------------------------------------------------------

/// Identifiers accepted in a `BPETS_V3.CFG` file.  Each row is
/// `(KEY, setter)`; the setter takes a parsed `u32` and writes the
/// clamped value into a `Thresholds` instance.  Unknown keys are
/// ignored by the parser, so adding a new tunable later does not break
/// older config files.
pub type Setter = fn(&mut Thresholds, u32);

pub const BORNPETS_CFG_KEYS: &[(&str, Setter)] = &[
    // Setters clamp to the field's type (and to a sane range where a value
    // would otherwise wrap or divide by zero); `validate` fixes band order.
    ("MINIGAME_COOLDOWN", |t, v| {
        t.MINIGAME_COOLDOWN = clamp_u16(v)
    }),
    ("HATCHING_TICKS", |t, v| t.HATCHING_TICKS = clamp_u16(v)),
    ("MAX_SLEEP_TICKS", |t, v| {
        t.MAX_SLEEP_TICKS = v.clamp(30, 1440)
    }),
    ("FULL_LO_PCT", |t, v| t.FULL_LO_PCT = clamp_pct(v)),
    ("FULL_HI_PCT", |t, v| t.FULL_HI_PCT = clamp_pct(v)),
    ("WATER_LO_PCT", |t, v| t.WATER_LO_PCT = clamp_pct(v)),
    ("WATER_HI_PCT", |t, v| t.WATER_HI_PCT = clamp_pct(v)),
    ("ENERGY_LO_PCT", |t, v| t.ENERGY_LO_PCT = clamp_pct(v)),
    ("FUN_LO_PCT", |t, v| t.FUN_LO_PCT = clamp_pct(v)),
    ("FUN_HI_PCT", |t, v| t.FUN_HI_PCT = clamp_pct(v)),
    ("DIRT_HI_PCT", |t, v| t.DIRT_HI_PCT = clamp_pct(v)),
    ("BOOZE_HI_PCT", |t, v| t.BOOZE_HI_PCT = clamp_pct(v)),
    ("BOOZE_DANGER_PCT", |t, v| t.BOOZE_DANGER_PCT = clamp_pct(v)),
    ("STRAIN_INJ_PCT", |t, v| t.STRAIN_INJ_PCT = clamp_pct(v)),
    ("FIT_LO_PCT", |t, v| t.FIT_LO_PCT = clamp_pct(v)),
    ("WEIGHT_LO_PCT", |t, v| t.WEIGHT_LO_PCT = clamp_pct(v)),
    ("WEIGHT_HI_PCT", |t, v| t.WEIGHT_HI_PCT = clamp_pct(v)),
    ("FULL_DRAIN_AWAKE_PM", |t, v| {
        t.FULL_DRAIN_AWAKE_PM = clamp_u16(v)
    }),
    ("WATER_DRAIN_AWAKE_PM", |t, v| {
        t.WATER_DRAIN_AWAKE_PM = clamp_u16(v)
    }),
    ("ENERGY_DRAIN_AWAKE_PM", |t, v| {
        t.ENERGY_DRAIN_AWAKE_PM = clamp_u16(v)
    }),
    ("FUN_DRAIN_PM", |t, v| t.FUN_DRAIN_PM = clamp_u16(v)),
    ("FULL_DRAIN_ASLEEP_PM", |t, v| {
        t.FULL_DRAIN_ASLEEP_PM = clamp_u16(v)
    }),
    ("WATER_DRAIN_ASLEEP_PM", |t, v| {
        t.WATER_DRAIN_ASLEEP_PM = clamp_u16(v)
    }),
    ("ENERGY_GAIN_ASLEEP_PM", |t, v| {
        t.ENERGY_GAIN_ASLEEP_PM = clamp_u16(v)
    }),
    ("SLEEP_FUN_FLOOR_PCT", |t, v| {
        t.SLEEP_FUN_FLOOR_PCT = clamp_pct(v)
    }),
    ("DIRT_GAIN_PM", |t, v| t.DIRT_GAIN_PM = clamp_u16(v)),
    ("BOOZE_CLEAR_PM", |t, v| t.BOOZE_CLEAR_PM = clamp_u16(v)),
    ("BOOZE_CLEAR_SLOW_PCT", |t, v| {
        t.BOOZE_CLEAR_SLOW_PCT = clamp_mult(v)
    }),
    ("STRAIN_CLEAR_PM", |t, v| t.STRAIN_CLEAR_PM = clamp_u16(v)),
    ("FIT_DECAY_HIGH_PM", |t, v| {
        t.FIT_DECAY_HIGH_PM = clamp_u16(v)
    }),
    ("FIT_DECAY_MID_PM", |t, v| t.FIT_DECAY_MID_PM = clamp_u16(v)),
    ("FIT_DECAY_LOW_PM", |t, v| t.FIT_DECAY_LOW_PM = clamp_u16(v)),
    ("FIT_DECAY_HIGH_ABOVE_PCT", |t, v| {
        t.FIT_DECAY_HIGH_ABOVE_PCT = clamp_pct(v)
    }),
    ("FIT_DECAY_MID_ABOVE_PCT", |t, v| {
        t.FIT_DECAY_MID_ABOVE_PCT = clamp_pct(v)
    }),
    ("FIT_DECAY_HEAVY_PCT", |t, v| {
        t.FIT_DECAY_HEAVY_PCT = clamp_mult(v)
    }),
    ("WEIGHT_PULL_HEAVY_PM", |t, v| {
        t.WEIGHT_PULL_HEAVY_PM = clamp_u16(v)
    }),
    ("WEIGHT_PULL_PM", |t, v| t.WEIGHT_PULL_PM = clamp_u16(v)),
    ("WEIGHT_BELLY_PM", |t, v| t.WEIGHT_BELLY_PM = clamp_u16(v)),
    ("ENERGY_MULT_UNFIT_PCT", |t, v| {
        t.ENERGY_MULT_UNFIT_PCT = clamp_mult(v)
    }),
    ("ENERGY_MULT_FIT_PCT", |t, v| {
        t.ENERGY_MULT_FIT_PCT = clamp_mult(v)
    }),
    ("ENERGY_FIT_GOOD_PCT", |t, v| {
        t.ENERGY_FIT_GOOD_PCT = clamp_pct(v)
    }),
    ("ENERGY_MULT_DRUNK_PCT", |t, v| {
        t.ENERGY_MULT_DRUNK_PCT = clamp_mult(v)
    }),
    ("ENERGY_MULT_WIRED_PCT", |t, v| {
        t.ENERGY_MULT_WIRED_PCT = clamp_mult(v)
    }),
    ("ENERGY_MULT_HANGOVER_PCT", |t, v| {
        t.ENERGY_MULT_HANGOVER_PCT = clamp_mult(v)
    }),
    ("ENERGY_MULT_WEIGHT_PCT", |t, v| {
        t.ENERGY_MULT_WEIGHT_PCT = clamp_mult(v)
    }),
    ("ENERGY_CAP_DEPRIVED_PCT", |t, v| {
        t.ENERGY_CAP_DEPRIVED_PCT = clamp_pct(v)
    }),
    ("THIRST_MULT_BASE_PCT", |t, v| {
        t.THIRST_MULT_BASE_PCT = clamp_mult(v)
    }),
    ("THIRST_MULT_LOW_PCT", |t, v| {
        t.THIRST_MULT_LOW_PCT = clamp_mult(v)
    }),
    ("THIRST_MULT_MID_PCT", |t, v| {
        t.THIRST_MULT_MID_PCT = clamp_mult(v)
    }),
    ("THIRST_MULT_HIGH_PCT", |t, v| {
        t.THIRST_MULT_HIGH_PCT = clamp_mult(v)
    }),
    ("THIRST_BOOZE_LOW_PCT", |t, v| {
        t.THIRST_BOOZE_LOW_PCT = clamp_pct(v)
    }),
    ("THIRST_BOOZE_MID_PCT", |t, v| {
        t.THIRST_BOOZE_MID_PCT = clamp_pct(v)
    }),
    ("THIRST_BOOZE_HIGH_PCT", |t, v| {
        t.THIRST_BOOZE_HIGH_PCT = clamp_pct(v)
    }),
    ("SLEEP_QUALITY_POOR_PM", |t, v| {
        t.SLEEP_QUALITY_POOR_PM = clamp_u16(v)
    }),
    ("SLEEP_QUALITY_GOOD_PM", |t, v| {
        t.SLEEP_QUALITY_GOOD_PM = clamp_u16(v)
    }),
    ("UNSETTLED_BOOZE_PCT", |t, v| {
        t.UNSETTLED_BOOZE_PCT = clamp_pct(v)
    }),
    ("UNSETTLED_FULL_LO_PCT", |t, v| {
        t.UNSETTLED_FULL_LO_PCT = clamp_pct(v)
    }),
    ("UNSETTLED_FULL_HI_PCT", |t, v| {
        t.UNSETTLED_FULL_HI_PCT = clamp_pct(v)
    }),
    ("UNSETTLED_WATER_LO_PCT", |t, v| {
        t.UNSETTLED_WATER_LO_PCT = clamp_pct(v)
    }),
    ("UNSETTLED_WATER_HI_PCT", |t, v| {
        t.UNSETTLED_WATER_HI_PCT = clamp_pct(v)
    }),
    ("MOOD_STEP", |t, v| t.MOOD_STEP = clamp_u16(v).max(1)),
    ("MOOD_PEN_INJURED_PCT", |t, v| {
        t.MOOD_PEN_INJURED_PCT = clamp_mult(v)
    }),
    ("MOOD_PEN_GROGGY_PCT", |t, v| {
        t.MOOD_PEN_GROGGY_PCT = clamp_mult(v)
    }),
    ("MOOD_PEN_HANGOVER_PCT", |t, v| {
        t.MOOD_PEN_HANGOVER_PCT = clamp_mult(v)
    }),
    ("MOOD_PEN_DEPRIVED_PCT", |t, v| {
        t.MOOD_PEN_DEPRIVED_PCT = clamp_mult(v)
    }),
    ("MOOD_PEN_REHAB_PCT", |t, v| {
        t.MOOD_PEN_REHAB_PCT = clamp_mult(v)
    }),
    ("MOOD_PEN_CRAVING_PCT", |t, v| {
        t.MOOD_PEN_CRAVING_PCT = clamp_mult(v)
    }),
    ("MOOD_CRAVING_BOOZE_PCT", |t, v| {
        t.MOOD_CRAVING_BOOZE_PCT = clamp_pct(v)
    }),
    ("MOOD_HEALTH_FLOOR_PCT", |t, v| {
        t.MOOD_HEALTH_FLOOR_PCT = clamp_pct(v)
    }),
    ("CEILING_FIT_STEP1_PCT", |t, v| {
        t.CEILING_FIT_STEP1_PCT = clamp_pct(v)
    }),
    ("CEILING_FIT_STEP2_PCT", |t, v| {
        t.CEILING_FIT_STEP2_PCT = clamp_pct(v)
    }),
    ("CEILING_FIT_STEP3_PCT", |t, v| {
        t.CEILING_FIT_STEP3_PCT = clamp_pct(v)
    }),
    ("CEILING_STEP0_PCT", |t, v| {
        t.CEILING_STEP0_PCT = v.min(100) as u16
    }),
    ("CEILING_STEP1_PCT", |t, v| {
        t.CEILING_STEP1_PCT = v.min(100) as u16
    }),
    ("CEILING_STEP2_PCT", |t, v| {
        t.CEILING_STEP2_PCT = v.min(100) as u16
    }),
    ("CEILING_STEP3_PCT", |t, v| {
        t.CEILING_STEP3_PCT = v.min(100) as u16
    }),
    ("CEILING_DIABETIC_PCT", |t, v| {
        t.CEILING_DIABETIC_PCT = v.min(100) as u16
    }),
    ("DMG_NEED_LOW_PCT", |t, v| t.DMG_NEED_LOW_PCT = clamp_pct(v)),
    ("DMG_NEED_HIGH_PCT", |t, v| {
        t.DMG_NEED_HIGH_PCT = clamp_pct(v)
    }),
    ("DMG_ENERGY_PCT", |t, v| t.DMG_ENERGY_PCT = clamp_pct(v)),
    ("DMG_ENERGY_HEALTH_PCT", |t, v| {
        t.DMG_ENERGY_HEALTH_PCT = clamp_pct(v)
    }),
    ("DMG_MOOD_PCT", |t, v| t.DMG_MOOD_PCT = clamp_pct(v)),
    ("DMG_DIRT_PCT", |t, v| t.DMG_DIRT_PCT = clamp_pct(v)),
    ("DMG_WEIGHT_HIGH_PCT", |t, v| {
        t.DMG_WEIGHT_HIGH_PCT = clamp_pct(v)
    }),
    ("DMG_WEIGHT_LOW_PCT", |t, v| {
        t.DMG_WEIGHT_LOW_PCT = clamp_pct(v)
    }),
    ("DMG_FULL_LOW_PM", |t, v| t.DMG_FULL_LOW_PM = clamp_u16(v)),
    ("DMG_FULL_HIGH_PM", |t, v| t.DMG_FULL_HIGH_PM = clamp_u16(v)),
    ("DMG_WATER_LOW_PM", |t, v| t.DMG_WATER_LOW_PM = clamp_u16(v)),
    ("DMG_WATER_HIGH_PM", |t, v| {
        t.DMG_WATER_HIGH_PM = clamp_u16(v)
    }),
    ("DMG_ENERGY_PM", |t, v| t.DMG_ENERGY_PM = clamp_u16(v)),
    ("DMG_BOOZE_PM", |t, v| t.DMG_BOOZE_PM = clamp_u16(v)),
    ("DMG_MOOD_PM", |t, v| t.DMG_MOOD_PM = clamp_u16(v)),
    ("DMG_DIRT_PM", |t, v| t.DMG_DIRT_PM = clamp_u16(v)),
    ("DMG_WEIGHT_HIGH_PM", |t, v| {
        t.DMG_WEIGHT_HIGH_PM = clamp_u16(v)
    }),
    ("DMG_WEIGHT_LOW_PM", |t, v| {
        t.DMG_WEIGHT_LOW_PM = clamp_u16(v)
    }),
    ("REC_GOOD_PM", |t, v| t.REC_GOOD_PM = clamp_u16(v)),
    ("REC_FAIR_PM", |t, v| t.REC_FAIR_PM = clamp_u16(v)),
    ("REC_GOOD_MOOD_PCT", |t, v| {
        t.REC_GOOD_MOOD_PCT = clamp_pct(v)
    }),
    ("REC_FAIR_MOOD_PCT", |t, v| {
        t.REC_FAIR_MOOD_PCT = clamp_pct(v)
    }),
    ("REC_DEPRIVED_PCT", |t, v| {
        t.REC_DEPRIVED_PCT = clamp_mult(v)
    }),
    ("DEPENDENCE_ONSET_MIN", |t, v| {
        t.DEPENDENCE_ONSET_MIN = clamp_u16(v)
    }),
    ("DEPENDENCE_RELAPSE_MIN", |t, v| {
        t.DEPENDENCE_RELAPSE_MIN = clamp_u16(v)
    }),
    ("DIABETES_ONSET_MIN", |t, v| {
        t.DIABETES_ONSET_MIN = clamp_u16(v)
    }),
    ("DIABETES_REVERSAL_MIN", |t, v| {
        t.DIABETES_REVERSAL_MIN = clamp_u16(v)
    }),
    ("HANGOVER_MIN", |t, v| t.HANGOVER_MIN = clamp_u16(v)),
    ("RECOVERING_MIN", |t, v| t.RECOVERING_MIN = clamp_u16(v)),
    ("SELF_WAKE_MIN", |t, v| t.SELF_WAKE_MIN = clamp_u16(v)),
    ("OVERSLEEP_WAKE_MIN", |t, v| {
        t.OVERSLEEP_WAKE_MIN = clamp_u16(v)
    }),
    ("GROGGY_LINE_MIN", |t, v| t.GROGGY_LINE_MIN = clamp_u16(v)),
    ("GROGGY_MIN", |t, v| t.GROGGY_MIN = clamp_u16(v)),
    ("GROGGY_ENERGY_PCT", |t, v| {
        t.GROGGY_ENERGY_PCT = clamp_pct(v)
    }),
    ("GROGGY_FIT_PCT", |t, v| t.GROGGY_FIT_PCT = clamp_pct(v)),
    ("RESTED_MIN", |t, v| t.RESTED_MIN = clamp_u16(v)),
    ("GOOD_SLEEP_LINE_MIN", |t, v| {
        t.GOOD_SLEEP_LINE_MIN = clamp_u16(v)
    }),
    ("NIGHT_SHORT_MIN", |t, v| t.NIGHT_SHORT_MIN = clamp_u16(v)),
    ("NIGHT_GOOD_MIN", |t, v| t.NIGHT_GOOD_MIN = clamp_u16(v)),
    ("DEPRIVED_NIGHTS", |t, v| {
        t.DEPRIVED_NIGHTS = v.clamp(1, 255) as u8
    }),
    ("CLEARING_NIGHTS", |t, v| {
        t.CLEARING_NIGHTS = v.clamp(1, 255) as u8
    }),
    ("GRUMPY_PCT", |t, v| t.GRUMPY_PCT = clamp_pct(v)),
    ("GRUMPY_ENERGY_PCT", |t, v| {
        t.GRUMPY_ENERGY_PCT = clamp_pct(v)
    }),
    ("COLLAPSE_DAMAGE", |t, v| t.COLLAPSE_DAMAGE = clamp_u16(v)),
    ("COLLAPSE_HEALTH_FLOOR_PCT", |t, v| {
        t.COLLAPSE_HEALTH_FLOOR_PCT = clamp_pct(v)
    }),
    ("LEAVING_MIN", |t, v| t.LEAVING_MIN = clamp_u16(v)),
    ("BOOZE_CLEAR_PCT", |t, v| t.BOOZE_CLEAR_PCT = clamp_pct(v)),
    ("DIABETES_WEIGHT_HI_PCT", |t, v| {
        t.DIABETES_WEIGHT_HI_PCT = clamp_pct(v)
    }),
    ("DIABETES_WEIGHT_LO_PCT", |t, v| {
        t.DIABETES_WEIGHT_LO_PCT = clamp_pct(v)
    }),
    ("REVERSAL_FIT_PCT", |t, v| t.REVERSAL_FIT_PCT = clamp_pct(v)),
    ("HANGOVER_ASLEEP_MULT", |t, v| {
        t.HANGOVER_ASLEEP_MULT = v.clamp(1, 255) as u8
    }),
    ("HEALTH_EXCESS_DECAY_PM", |t, v| {
        t.HEALTH_EXCESS_DECAY_PM = clamp_u16(v)
    }),
    ("CARE_GOOD_MOOD_PCT", |t, v| {
        t.CARE_GOOD_MOOD_PCT = clamp_pct(v)
    }),
    ("OVERSLEEP_RESET_PCT", |t, v| {
        t.OVERSLEEP_RESET_PCT = clamp_pct(v)
    }),
    ("DOSE_SALAD_FULL_PCT", |t, v| {
        t.DOSE_SALAD_FULL_PCT = v.min(100) as u8
    }),
    ("DOSE_APPLE_FULL_PCT", |t, v| {
        t.DOSE_APPLE_FULL_PCT = v.min(100) as u8
    }),
    ("DOSE_FRIKANDEL_FULL_PCT", |t, v| {
        t.DOSE_FRIKANDEL_FULL_PCT = v.min(100) as u8
    }),
    ("DOSE_PIZZA_FULL_PCT", |t, v| {
        t.DOSE_PIZZA_FULL_PCT = v.min(100) as u8
    }),
    ("DOSE_CAKE_FULL_PCT", |t, v| {
        t.DOSE_CAKE_FULL_PCT = v.min(100) as u8
    }),
    ("DOSE_SALAD_DIRT_PCT", |t, v| {
        t.DOSE_SALAD_DIRT_PCT = v.min(100) as u8
    }),
    ("DOSE_APPLE_DIRT_PCT", |t, v| {
        t.DOSE_APPLE_DIRT_PCT = v.min(100) as u8
    }),
    ("DOSE_FRIKANDEL_DIRT_PCT", |t, v| {
        t.DOSE_FRIKANDEL_DIRT_PCT = v.min(100) as u8
    }),
    ("DOSE_PIZZA_DIRT_PCT", |t, v| {
        t.DOSE_PIZZA_DIRT_PCT = v.min(100) as u8
    }),
    ("DOSE_CAKE_DIRT_PCT", |t, v| {
        t.DOSE_CAKE_DIRT_PCT = v.min(100) as u8
    }),
    ("DOSE_SALAD_WEIGHT_PM", |t, v| {
        t.DOSE_SALAD_WEIGHT_PM = clamp_u16(v)
    }),
    ("DOSE_APPLE_WEIGHT_PM", |t, v| {
        t.DOSE_APPLE_WEIGHT_PM = clamp_u16(v)
    }),
    ("DOSE_FRIKANDEL_WEIGHT_PM", |t, v| {
        t.DOSE_FRIKANDEL_WEIGHT_PM = clamp_u16(v)
    }),
    ("DOSE_PIZZA_WEIGHT_PM", |t, v| {
        t.DOSE_PIZZA_WEIGHT_PM = clamp_u16(v)
    }),
    ("DOSE_CAKE_WEIGHT_PM", |t, v| {
        t.DOSE_CAKE_WEIGHT_PM = clamp_u16(v)
    }),
    ("DOSE_FRIKANDEL_MOOD_PCT", |t, v| {
        t.DOSE_FRIKANDEL_MOOD_PCT = v.min(100) as u8
    }),
    ("DOSE_PIZZA_MOOD_PCT", |t, v| {
        t.DOSE_PIZZA_MOOD_PCT = v.min(100) as u8
    }),
    ("DOSE_CAKE_MOOD_PCT", |t, v| {
        t.DOSE_CAKE_MOOD_PCT = v.min(100) as u8
    }),
    ("DOSE_HEALTHY_WATER_PCT", |t, v| {
        t.DOSE_HEALTHY_WATER_PCT = v.min(100) as u8
    }),
    ("DOSE_SALAD_FIT_PCT", |t, v| {
        t.DOSE_SALAD_FIT_PCT = v.min(100) as u8
    }),
    ("DOSE_JUNK_REPEAT_DIV", |t, v| {
        t.DOSE_JUNK_REPEAT_DIV = v.clamp(1, 255) as u8
    }),
    ("DOSE_JUNK_MIN", |t, v| t.DOSE_JUNK_MIN = clamp_u16(v)),
    ("DOSE_FULL_CAP_PCT", |t, v| {
        t.DOSE_FULL_CAP_PCT = v.clamp(1, 255) as u8
    }),
    ("DOSE_EXCESS_ENERGY_DIV", |t, v| {
        t.DOSE_EXCESS_ENERGY_DIV = v.clamp(1, 255) as u8
    }),
    ("DOSE_EXCESS_WEIGHT_DIV", |t, v| {
        t.DOSE_EXCESS_WEIGHT_DIV = v.clamp(1, 255) as u8
    }),
    ("DOSE_WATER_MAX_PCT", |t, v| {
        t.DOSE_WATER_MAX_PCT = v.min(100) as u8
    }),
    ("DOSE_WATER_BELLY_PCT", |t, v| {
        t.DOSE_WATER_BELLY_PCT = v.min(100) as u8
    }),
    ("DOSE_WATER_BELLY_MIN_PCT", |t, v| {
        t.DOSE_WATER_BELLY_MIN_PCT = v.min(100) as u8
    }),
    ("DOSE_WATER_BOOZE_PCT", |t, v| {
        t.DOSE_WATER_BOOZE_PCT = v.min(100) as u8
    }),
    ("DOSE_WATER_HANGOVER_MIN", |t, v| {
        t.DOSE_WATER_HANGOVER_MIN = clamp_u16(v)
    }),
    ("DOSE_WATER_GROGGY_MIN", |t, v| {
        t.DOSE_WATER_GROGGY_MIN = clamp_u16(v)
    }),
    ("DOSE_COFFEE_WATER_PCT", |t, v| {
        t.DOSE_COFFEE_WATER_PCT = v.min(100) as u8
    }),
    ("DOSE_COFFEE_FULL_PCT", |t, v| {
        t.DOSE_COFFEE_FULL_PCT = v.min(100) as u8
    }),
    ("DOSE_COFFEE_ENERGY_PCT", |t, v| {
        t.DOSE_COFFEE_ENERGY_PCT = v.min(100) as u8
    }),
    ("DOSE_COFFEE_MOOD_PCT", |t, v| {
        t.DOSE_COFFEE_MOOD_PCT = v.min(100) as u8
    }),
    ("DOSE_COFFEE_WEIGHT_PCT", |t, v| {
        t.DOSE_COFFEE_WEIGHT_PCT = v.min(100) as u8
    }),
    ("COFFEE_WINDOW_MIN", |t, v| t.COFFEE_WINDOW_MIN = clamp_u16(v)),
    ("COFFEE_WIRED_CUPS", |t, v| t.COFFEE_WIRED_CUPS = v.min(u8::MAX as u32) as u8),
    ("COFFEE_WIRED_MIN", |t, v| t.COFFEE_WIRED_MIN = clamp_u16(v)),
    ("DOSE_BEER_WATER_PCT", |t, v| {
        t.DOSE_BEER_WATER_PCT = v.min(100) as u8
    }),
    ("DOSE_BEER_BELLY_PCT", |t, v| {
        t.DOSE_BEER_BELLY_PCT = v.min(100) as u8
    }),
    ("DOSE_BEER_BOOZE_PCT", |t, v| {
        t.DOSE_BEER_BOOZE_PCT = v.min(100) as u8
    }),
    ("DOSE_BEER_MOOD_PCT", |t, v| {
        t.DOSE_BEER_MOOD_PCT = v.min(100) as u8
    }),
    ("DOSE_WINE_WATER_PCT", |t, v| {
        t.DOSE_WINE_WATER_PCT = v.min(100) as u8
    }),
    ("DOSE_WINE_BELLY_PCT", |t, v| {
        t.DOSE_WINE_BELLY_PCT = v.min(100) as u8
    }),
    ("DOSE_WINE_BOOZE_PCT", |t, v| {
        t.DOSE_WINE_BOOZE_PCT = v.min(100) as u8
    }),
    ("DOSE_WINE_MOOD_PCT", |t, v| {
        t.DOSE_WINE_MOOD_PCT = v.min(100) as u8
    }),
    ("DOSE_WHISKEY_WATER_PCT", |t, v| {
        t.DOSE_WHISKEY_WATER_PCT = v.min(100) as u8
    }),
    ("DOSE_WHISKEY_BELLY_PCT", |t, v| {
        t.DOSE_WHISKEY_BELLY_PCT = v.min(100) as u8
    }),
    ("DOSE_WHISKEY_BOOZE_PCT", |t, v| {
        t.DOSE_WHISKEY_BOOZE_PCT = v.min(100) as u8
    }),
    ("DOSE_WHISKEY_MOOD_PCT", |t, v| {
        t.DOSE_WHISKEY_MOOD_PCT = v.min(100) as u8
    }),
    ("DOSE_BEER_WEIGHT_PCT", |t, v| {
        t.DOSE_BEER_WEIGHT_PCT = v.min(100) as u8
    }),
    ("DOSE_ALCOHOL_ENERGY_PCT", |t, v| {
        t.DOSE_ALCOHOL_ENERGY_PCT = v.min(100) as u8
    }),
    ("DOSE_ALCOHOL_FUN_PCT", |t, v| {
        t.DOSE_ALCOHOL_FUN_PCT = v.min(100) as u8
    }),
    ("DOSE_TIRED_BOOZE_PCT", |t, v| {
        t.DOSE_TIRED_BOOZE_PCT = clamp_mult(v)
    }),
    ("DOSE_DEPENDENT_DIV", |t, v| {
        t.DOSE_DEPENDENT_DIV = v.clamp(1, 255) as u8
    }),
    ("DOSE_RELAPSE_MOOD_PCT", |t, v| {
        t.DOSE_RELAPSE_MOOD_PCT = v.min(100) as u8
    }),
    ("DOSE_PLAY_FUN_PCT", |t, v| {
        t.DOSE_PLAY_FUN_PCT = v.min(100) as u8
    }),
    ("DOSE_PLAY_ENERGY_PCT", |t, v| {
        t.DOSE_PLAY_ENERGY_PCT = v.min(100) as u8
    }),
    ("DOSE_PLAY_FULL_PCT", |t, v| {
        t.DOSE_PLAY_FULL_PCT = v.min(100) as u8
    }),
    ("DOSE_PLAY_WATER_PCT", |t, v| {
        t.DOSE_PLAY_WATER_PCT = v.min(100) as u8
    }),
    ("DOSE_PLAY_FIT_PCT", |t, v| {
        t.DOSE_PLAY_FIT_PCT = v.min(100) as u8
    }),
    ("DOSE_PLAY_DIRT_PCT", |t, v| {
        t.DOSE_PLAY_DIRT_PCT = v.min(100) as u8
    }),
    ("DOSE_PLAY_GROGGY_MIN", |t, v| {
        t.DOSE_PLAY_GROGGY_MIN = clamp_u16(v)
    }),
    ("DOSE_TIRED_FUN_MULT", |t, v| {
        t.DOSE_TIRED_FUN_MULT = v.min(100) as u8
    }),
    ("DOSE_MINI_FUN_BASE_PCT", |t, v| {
        t.DOSE_MINI_FUN_BASE_PCT = v.min(100) as u8
    }),
    ("DOSE_MINI_FUN_GRADE_PCT", |t, v| {
        t.DOSE_MINI_FUN_GRADE_PCT = v.min(100) as u8
    }),
    ("DOSE_MINI_ENERGY_PCT", |t, v| {
        t.DOSE_MINI_ENERGY_PCT = v.min(100) as u8
    }),
    ("DOSE_EXERCISE_STRAIN_PCT", |t, v| {
        t.DOSE_EXERCISE_STRAIN_PCT = v.min(100) as u8
    }),
    ("DOSE_STRAIN_NEED_PCT", |t, v| {
        t.DOSE_STRAIN_NEED_PCT = clamp_mult(v)
    }),
    ("DOSE_STRAIN_RESTED_PCT", |t, v| {
        t.DOSE_STRAIN_RESTED_PCT = clamp_mult(v)
    }),
    ("DOSE_STRAIN_FIT_PCT", |t, v| {
        t.DOSE_STRAIN_FIT_PCT = clamp_mult(v)
    }),
    ("DOSE_STRAIN_FIT_MIN_PCT", |t, v| {
        t.DOSE_STRAIN_FIT_MIN_PCT = v.min(100) as u8
    }),
    ("DOSE_STRAIN_HEAVY_PCT", |t, v| {
        t.DOSE_STRAIN_HEAVY_PCT = clamp_mult(v)
    }),
    ("DOSE_STRAIN_DEPRIVED_PCT", |t, v| {
        t.DOSE_STRAIN_DEPRIVED_PCT = clamp_mult(v)
    }),
    ("DOSE_EXERCISE_ENERGY_PCT", |t, v| {
        t.DOSE_EXERCISE_ENERGY_PCT = v.min(100) as u8
    }),
    ("DOSE_EXERCISE_FULL_PCT", |t, v| {
        t.DOSE_EXERCISE_FULL_PCT = v.min(100) as u8
    }),
    ("DOSE_EXERCISE_WATER_PCT", |t, v| {
        t.DOSE_EXERCISE_WATER_PCT = v.min(100) as u8
    }),
    ("DOSE_EXERCISE_FUN_PCT", |t, v| {
        t.DOSE_EXERCISE_FUN_PCT = v.min(100) as u8
    }),
    ("DOSE_EXERCISE_BOOZE_PCT", |t, v| {
        t.DOSE_EXERCISE_BOOZE_PCT = v.min(100) as u8
    }),
    ("DOSE_EXERCISE_DIRT_PCT", |t, v| {
        t.DOSE_EXERCISE_DIRT_PCT = v.min(100) as u8
    }),
    ("DOSE_EXERCISE_WEIGHT_PCT", |t, v| {
        t.DOSE_EXERCISE_WEIGHT_PCT = v.min(100) as u8
    }),
    ("DOSE_EXERCISE_HANGOVER_MIN", |t, v| {
        t.DOSE_EXERCISE_HANGOVER_MIN = clamp_u16(v)
    }),
    ("DOSE_EXERCISE_FIT_PCT", |t, v| {
        t.DOSE_EXERCISE_FIT_PCT = v.min(100) as u8
    }),
    ("DOSE_INJURY_MIN", |t, v| t.DOSE_INJURY_MIN = clamp_u16(v)),
    ("DOSE_INJURY_HEALTH_PCT", |t, v| {
        t.DOSE_INJURY_HEALTH_PCT = v.min(100) as u8
    }),
    ("DOSE_WASH_DIRT_PCT", |t, v| {
        t.DOSE_WASH_DIRT_PCT = v.min(100) as u8
    }),
    ("DOSE_RELAX_FUN_PCT", |t, v| {
        t.DOSE_RELAX_FUN_PCT = v.min(100) as u8
    }),
    ("DOSE_RELAX_STRAIN_PCT", |t, v| {
        t.DOSE_RELAX_STRAIN_PCT = v.min(100) as u8
    }),
    ("DOSE_RELAX_ENERGY_PCT", |t, v| {
        t.DOSE_RELAX_ENERGY_PCT = v.min(100) as u8
    }),
    ("DOSE_RELAX_FIT_PCT", |t, v| {
        t.DOSE_RELAX_FIT_PCT = v.min(100) as u8
    }),
    ("DOSE_MED_CD_MIN", |t, v| t.DOSE_MED_CD_MIN = clamp_u16(v)),
    ("DOSE_HEAL_INJURY_MIN", |t, v| {
        t.DOSE_HEAL_INJURY_MIN = clamp_u16(v)
    }),
    ("DOSE_MED_BOOZE_PCT", |t, v| {
        t.DOSE_MED_BOOZE_PCT = v.min(100) as u8
    }),
    ("DOSE_HEAL_HEALTHY_PCT", |t, v| {
        t.DOSE_HEAL_HEALTHY_PCT = v.min(100) as u8
    }),
    ("DOSE_HEAL_HEALTH_PCT", |t, v| {
        t.DOSE_HEAL_HEALTH_PCT = v.min(100) as u8
    }),
    ("DOSE_INSULIN_WINDOW_MIN", |t, v| {
        t.DOSE_INSULIN_WINDOW_MIN = clamp_u16(v)
    }),
    ("DOSE_INSULIN_MIN", |t, v| t.DOSE_INSULIN_MIN = clamp_u16(v)),
    ("DOSE_OZEMPIC_WEIGHT_PCT", |t, v| {
        t.DOSE_OZEMPIC_WEIGHT_PCT = v.min(100) as u8
    }),
    ("DOSE_OZEMPIC_FIT_PCT", |t, v| {
        t.DOSE_OZEMPIC_FIT_PCT = v.min(100) as u8
    }),
    ("DOSE_OZEMPIC_MIN", |t, v| t.DOSE_OZEMPIC_MIN = clamp_u16(v)),
    ("DOSE_OZEMPIC_CD_MIN", |t, v| {
        t.DOSE_OZEMPIC_CD_MIN = clamp_u16(v)
    }),
    ("DOSE_REHAB_MIN", |t, v| t.DOSE_REHAB_MIN = clamp_u16(v)),
    ("DOSE_NIGHT_ENERGY_PCT", |t, v| {
        t.DOSE_NIGHT_ENERGY_PCT = v.min(100) as u8
    }),
    ("DOSE_WORK_ENERGY_PCT", |t, v| {
        t.DOSE_WORK_ENERGY_PCT = v.min(100) as u8
    }),
    ("DOSE_WORK_FUN_PCT", |t, v| {
        t.DOSE_WORK_FUN_PCT = v.min(100) as u8
    }),
    ("DOSE_WORK_MOOD_PCT", |t, v| {
        t.DOSE_WORK_MOOD_PCT = v.min(100) as u8
    }),
    ("DOSE_FRIEND_BIG_FUN_PCT", |t, v| {
        t.DOSE_FRIEND_BIG_FUN_PCT = v.min(100) as u8
    }),
    ("DOSE_FRIEND_SMALL_FUN_PCT", |t, v| {
        t.DOSE_FRIEND_SMALL_FUN_PCT = v.min(100) as u8
    }),
    ("DEMO_FIT_FLOOR_PCT", |t, v| {
        t.DEMO_FIT_FLOOR_PCT = clamp_pct(v)
    }),
    ("DEMO_HEALTH_FLOOR_PCT", |t, v| {
        t.DEMO_HEALTH_FLOOR_PCT = clamp_pct(v)
    }),
    ("DEMO_HELP_HEALTH_PCT", |t, v| {
        t.DEMO_HELP_HEALTH_PCT = clamp_pct(v)
    }),
    ("DEMO_ON_HEALTH_PCT", |t, v| {
        t.DEMO_ON_HEALTH_PCT = clamp_pct(v)
    }),
    ("DEMO_MED_HEALTH_PCT", |t, v| {
        t.DEMO_MED_HEALTH_PCT = clamp_pct(v)
    }),
    ("DEMO_WAKE_FULL_PCT", |t, v| {
        t.DEMO_WAKE_FULL_PCT = clamp_pct(v)
    }),
    ("DEMO_EDGE_FULL_PCT", |t, v| {
        t.DEMO_EDGE_FULL_PCT = clamp_pct(v)
    }),
    ("DEMO_EDGE_WATER_PCT", |t, v| {
        t.DEMO_EDGE_WATER_PCT = clamp_pct(v)
    }),
    ("DEMO_EDGE_ENERGY_PCT", |t, v| {
        t.DEMO_EDGE_ENERGY_PCT = clamp_pct(v)
    }),
    ("DEMO_EDGE_BOOZE_PCT", |t, v| {
        t.DEMO_EDGE_BOOZE_PCT = clamp_pct(v)
    }),
    ("DEMO_SOFT_WATER_PCT", |t, v| {
        t.DEMO_SOFT_WATER_PCT = clamp_pct(v)
    }),
    ("DEMO_SOFT_FULL_PCT", |t, v| {
        t.DEMO_SOFT_FULL_PCT = clamp_pct(v)
    }),
    ("DEMO_SOFT_ENERGY_PCT", |t, v| {
        t.DEMO_SOFT_ENERGY_PCT = clamp_pct(v)
    }),
    ("DEMO_SOFT_DIRT_PCT", |t, v| {
        t.DEMO_SOFT_DIRT_PCT = clamp_pct(v)
    }),
    ("DEMO_SOFT_FUN_PCT", |t, v| {
        t.DEMO_SOFT_FUN_PCT = clamp_pct(v)
    }),
    ("DEMO_PLAY_ENERGY_PCT", |t, v| {
        t.DEMO_PLAY_ENERGY_PCT = clamp_pct(v)
    }),
    ("DEMO_WIRED_FUN_PCT", |t, v| {
        t.DEMO_WIRED_FUN_PCT = clamp_pct(v)
    }),
    ("DEMO_RISE_ENERGY_PCT", |t, v| {
        t.DEMO_RISE_ENERGY_PCT = clamp_pct(v)
    }),
    ("DEMO_NAP_ENERGY_PCT", |t, v| {
        t.DEMO_NAP_ENERGY_PCT = clamp_pct(v)
    }),
    ("DEMO_NAP_FUN_PCT", |t, v| {
        t.DEMO_NAP_FUN_PCT = clamp_pct(v)
    }),
    ("DEMO_WHIM_BOOZE_LO_PCT", |t, v| {
        t.DEMO_WHIM_BOOZE_LO_PCT = clamp_pct(v)
    }),
    ("DEMO_WHIM_BOOZE_HI_PCT", |t, v| {
        t.DEMO_WHIM_BOOZE_HI_PCT = clamp_pct(v)
    }),
    ("DEMO_WHIM_BEER_LO_PCT", |t, v| {
        t.DEMO_WHIM_BEER_LO_PCT = v.min(100) as u8
    }),
    ("DEMO_WHIM_BEER_HI_PCT", |t, v| {
        t.DEMO_WHIM_BEER_HI_PCT = v.min(100) as u8
    }),
    ("DEMO_BEER_STOP_MIN", |t, v| {
        t.DEMO_BEER_STOP_MIN = clamp_u16(v)
    }),
    ("DEMO_LIGHT_FOOD_PCT", |t, v| {
        t.DEMO_LIGHT_FOOD_PCT = clamp_pct(v)
    }),
    ("DEMO_RICH_FOOD_PCT", |t, v| {
        t.DEMO_RICH_FOOD_PCT = clamp_pct(v)
    }),
    ("DEMO_REACT_BASE_MIN", |t, v| {
        t.DEMO_REACT_BASE_MIN = clamp_u16(v)
    }),
    ("DEMO_REACT_SPREAD_MIN", |t, v| {
        t.DEMO_REACT_SPREAD_MIN = clamp_u16(v).max(1)
    }),
    ("DEMO_DRY_BASE_MIN", |t, v| {
        t.DEMO_DRY_BASE_MIN = clamp_u16(v)
    }),
    ("DEMO_DRY_SPREAD_MIN", |t, v| {
        t.DEMO_DRY_SPREAD_MIN = clamp_u16(v).max(1)
    }),
    ("DEMO_WHIM_BASE_MIN", |t, v| {
        t.DEMO_WHIM_BASE_MIN = clamp_u16(v)
    }),
    ("DEMO_WHIM_SPREAD_MIN", |t, v| {
        t.DEMO_WHIM_SPREAD_MIN = clamp_u16(v).max(1)
    }),
    ("DEMO_LAZY_MIN", |t, v| {
        t.DEMO_LAZY_MIN = clamp_u16(v).max(1)
    }),
    ("DEMO_LAZY_MAX_PCT", |t, v| {
        t.DEMO_LAZY_MAX_PCT = v.clamp(1, LAZY_MAX as u32) as u8
    }),
    ("DEMO_NAP_BASE_MIN", |t, v| {
        t.DEMO_NAP_BASE_MIN = clamp_u16(v)
    }),
    ("DEMO_NAP_SPREAD_MIN", |t, v| {
        t.DEMO_NAP_SPREAD_MIN = clamp_u16(v).max(1)
    }),
    ("DEMO_EDGE_NAP_MIN", |t, v| {
        t.DEMO_EDGE_NAP_MIN = clamp_u16(v)
    }),
    ("DEMO_RISE_BASE_MIN", |t, v| {
        t.DEMO_RISE_BASE_MIN = clamp_u16(v)
    }),
    ("DEMO_RISE_SPREAD_MIN", |t, v| {
        t.DEMO_RISE_SPREAD_MIN = clamp_u16(v).max(1)
    }),
    ("DEMO_BED_AT_MIN", |t, v| {
        t.DEMO_BED_AT_MIN = clamp_u16(v).min(DAY_LAST_MIN)
    }),
    ("DEMO_BED_SPREAD_MIN", |t, v| {
        t.DEMO_BED_SPREAD_MIN = clamp_u16(v).clamp(1, DAY_SPREAD_MAX_MIN)
    }),
    ("DEMO_UP_AT_MIN", |t, v| {
        t.DEMO_UP_AT_MIN = clamp_u16(v).min(DAY_LAST_MIN)
    }),
    ("DEMO_UP_SPREAD_MIN", |t, v| {
        t.DEMO_UP_SPREAD_MIN = clamp_u16(v).clamp(1, DAY_SPREAD_MAX_MIN)
    }),
    ("DEMO_PAUSE_MIN", |t, v| {
        t.DEMO_PAUSE_MIN = v.min(u8::MAX as u32) as u8
    }),
    ("DEMO_PICK_APPLE_PCT", |t, v| {
        t.DEMO_PICK_APPLE_PCT = v.min(100) as u8
    }),
    ("DEMO_PICK_SALAD_PCT", |t, v| {
        t.DEMO_PICK_SALAD_PCT = v.min(100) as u8
    }),
    ("DEMO_PICK_PIZZA_PCT", |t, v| {
        t.DEMO_PICK_PIZZA_PCT = v.min(100) as u8
    }),
    ("DEMO_PICK_FRIK_PCT", |t, v| {
        t.DEMO_PICK_FRIK_PCT = v.min(100) as u8
    }),
    ("DEMO_PICK_WATER_PCT", |t, v| {
        t.DEMO_PICK_WATER_PCT = v.min(100) as u8
    }),
    ("DEMO_PICK_COFFEE_PCT", |t, v| {
        t.DEMO_PICK_COFFEE_PCT = v.min(100) as u8
    }),
    ("DEMO_PICK_GAME_PCT", |t, v| {
        t.DEMO_PICK_GAME_PCT = v.min(100) as u8
    }),
    ("DEMO_HAX_TARGET", |t, v| {
        t.DEMO_HAX_TARGET = clamp_u16(v)
    }),
    ("DEMO_HAX_DRIFT_MIN", |t, v| {
        t.DEMO_HAX_DRIFT_MIN = clamp_u16(v).max(1)
    }),
    ("BATTLE_POWER_BASE_PTS", |t, v| t.BATTLE_POWER_BASE_PTS = v.min(100) as u8),
    ("BATTLE_POWER_WEIGHT_PCT", |t, v| t.BATTLE_POWER_WEIGHT_PCT = clamp_pct(v)),
    ("BATTLE_COURAGE_PEAK_PTS", |t, v| t.BATTLE_COURAGE_PEAK_PTS = v.min(100) as u8),
    ("BATTLE_HIGH_MOOD_PCT", |t, v| t.BATTLE_HIGH_MOOD_PCT = clamp_pct(v)),
    ("BATTLE_HIGH_MOOD_PTS", |t, v| t.BATTLE_HIGH_MOOD_PTS = v.min(100) as u8),
    ("BATTLE_LOW_MOOD_PCT", |t, v| t.BATTLE_LOW_MOOD_PCT = clamp_pct(v)),
    ("BATTLE_GUARD_WEIGHT_PCT", |t, v| t.BATTLE_GUARD_WEIGHT_PCT = clamp_pct(v)),
    ("BATTLE_GUARD_BASE_PTS", |t, v| t.BATTLE_GUARD_BASE_PTS = v.min(100) as u8),
    ("BATTLE_SPEED_BASE_PTS", |t, v| t.BATTLE_SPEED_BASE_PTS = v.min(100) as u8),
    ("BATTLE_SPEED_FIT_PCT", |t, v| t.BATTLE_SPEED_FIT_PCT = clamp_pct(v)),
    ("BATTLE_RESTED_SPEED_PTS", |t, v| t.BATTLE_RESTED_SPEED_PTS = v.min(100) as u8),
    ("BATTLE_RESTED_FOCUS_PTS", |t, v| t.BATTLE_RESTED_FOCUS_PTS = v.min(100) as u8),
    ("BATTLE_GROGGY_SPEED_PTS", |t, v| t.BATTLE_GROGGY_SPEED_PTS = v.min(100) as u8),
    ("BATTLE_GROGGY_FOCUS_PTS", |t, v| t.BATTLE_GROGGY_FOCUS_PTS = v.min(100) as u8),
    ("BATTLE_HANGOVER_SPEED_PTS", |t, v| t.BATTLE_HANGOVER_SPEED_PTS = v.min(100) as u8),
    ("BATTLE_HANGOVER_FOCUS_PTS", |t, v| t.BATTLE_HANGOVER_FOCUS_PTS = v.min(100) as u8),
    ("BATTLE_CRAVING_FOCUS_PTS", |t, v| t.BATTLE_CRAVING_FOCUS_PTS = v.min(100) as u8),
    ("BATTLE_CRAVING_POWER_PTS", |t, v| t.BATTLE_CRAVING_POWER_PTS = v.min(100) as u8),
    ("BATTLE_HEAVY_UNFIT_FIT_PCT", |t, v| t.BATTLE_HEAVY_UNFIT_FIT_PCT = clamp_pct(v)),
    ("BATTLE_SLOPPY_PCT", |t, v| t.BATTLE_SLOPPY_PCT = clamp_pct(v)),
    ("BATTLE_FELT_BASE_PCT", |t, v| t.BATTLE_FELT_BASE_PCT = clamp_mult(v)),
    ("BATTLE_HP_BASE_PTS", |t, v| t.BATTLE_HP_BASE_PTS = v.min(100) as u8),
    ("BATTLE_HP_WEIGHT_PCT", |t, v| t.BATTLE_HP_WEIGHT_PCT = clamp_pct(v)),
    ("BATTLE_POWER_FLOOR_PTS", |t, v| t.BATTLE_POWER_FLOOR_PTS = v.min(100) as u8),
    ("BATTLE_GUARD_FLOOR_PTS", |t, v| t.BATTLE_GUARD_FLOOR_PTS = v.min(100) as u8),
    ("BATTLE_HP_FLOOR_PTS", |t, v| t.BATTLE_HP_FLOOR_PTS = v.min(100) as u8),
    ("BATTLE_STAMINA_FLOOR_PTS", |t, v| t.BATTLE_STAMINA_FLOOR_PTS = v.min(100) as u8),
    ("BATTLE_INJURED_PCT", |t, v| t.BATTLE_INJURED_PCT = clamp_pct(v)),
    ("BATTLE_FORM_TOP_PCT", |t, v| t.BATTLE_FORM_TOP_PCT = clamp_pct(v)),
    ("BATTLE_FORM_IN_PCT", |t, v| t.BATTLE_FORM_IN_PCT = clamp_pct(v)),
    ("BATTLE_FORM_OFF_PCT", |t, v| t.BATTLE_FORM_OFF_PCT = clamp_pct(v)),
    ("BATTLE_COOLDOWN", |t, v| t.BATTLE_COOLDOWN = clamp_u16(v)),
    ("BATTLE_PAIR_COOLDOWN_MIN", |t, v| t.BATTLE_PAIR_COOLDOWN_MIN = clamp_u16(v)),
    ("BATTLE_ENERGY_PCT", |t, v| t.BATTLE_ENERGY_PCT = v.min(100) as u8),
    ("BATTLE_WATER_PCT", |t, v| t.BATTLE_WATER_PCT = v.min(100) as u8),
    ("BATTLE_FUN_PCT", |t, v| t.BATTLE_FUN_PCT = v.min(100) as u8),
    ("BATTLE_DIRT_PCT", |t, v| t.BATTLE_DIRT_PCT = v.min(100) as u8),
    ("BATTLE_STRAIN_PCT", |t, v| t.BATTLE_STRAIN_PCT = v.min(100) as u8),
    ("BATTLE_STRAIN_NEED_PCT", |t, v| t.BATTLE_STRAIN_NEED_PCT = clamp_mult(v)),
    ("BATTLE_STRAIN_RESTED_PCT", |t, v| t.BATTLE_STRAIN_RESTED_PCT = clamp_mult(v)),
    ("BATTLE_BRUISE_MAX_PCT", |t, v| t.BATTLE_BRUISE_MAX_PCT = v.min(100) as u8),
    ("BATTLE_MOOD_PCT", |t, v| t.BATTLE_MOOD_PCT = v.min(100) as u8),
    ("BATTLE_MIN_HEALTH_PCT", |t, v| t.BATTLE_MIN_HEALTH_PCT = clamp_pct(v)),
    ("BATTLE_DRUNK_PCT", |t, v| t.BATTLE_DRUNK_PCT = clamp_pct(v)),
    ("BATTLE_PRIZE_FIRST_HAX", |t, v| t.BATTLE_PRIZE_FIRST_HAX = clamp_u16(v)),
    ("BATTLE_PRIZE_LATER_HAX", |t, v| t.BATTLE_PRIZE_LATER_HAX = clamp_u16(v)),
    ("BATTLE_PRIZE_BEACON_MIN", |t, v| t.BATTLE_PRIZE_BEACON_MIN = clamp_u16(v)),
    ("BATTLE_DECLINE_MOOD_PCT", |t, v| t.BATTLE_DECLINE_MOOD_PCT = v.min(100) as u16),
    ("BATTLE_DECLINE_MIN", |t, v| t.BATTLE_DECLINE_MIN = clamp_u16(v)),
    ("BG_NIGHT_FROM_MIN", |t, v| {
        t.BG_NIGHT_FROM_MIN = clamp_u16(v).min(DAY_LAST_MIN)
    }),
    ("BG_NIGHT_TO_MIN", |t, v| {
        t.BG_NIGHT_TO_MIN = clamp_u16(v).min(DAY_LAST_MIN)
    }),
    ("FLOOR_CRUMBS_DIRT_PCT", |t, v| {
        t.FLOOR_CRUMBS_DIRT_PCT = clamp_pct(v)
    }),
    ("FLOOR_PEEL_DIRT_PCT", |t, v| {
        t.FLOOR_PEEL_DIRT_PCT = clamp_pct(v)
    }),
    ("FLOOR_CAN_DIRT_PCT", |t, v| {
        t.FLOOR_CAN_DIRT_PCT = clamp_pct(v)
    }),
    ("FLOOR_SOCK_DIRT_PCT", |t, v| {
        t.FLOOR_SOCK_DIRT_PCT = clamp_pct(v)
    }),
    ("FLOOR_PIZZA_DIRT_PCT", |t, v| {
        t.FLOOR_PIZZA_DIRT_PCT = clamp_pct(v)
    }),
];

/// Old key names still read from a `BPETS_V3.CFG`, as `(old, new)`: a file
/// edited before a rename keeps working. The template shows only the new
/// names (`BORNPETS_CFG_KEYS`). Cola became coffee.
pub const BORNPETS_CFG_ALIASES: &[(&str, &str)] = &[
    ("DOSE_COLA_WATER_PCT", "DOSE_COFFEE_WATER_PCT"),
    ("DOSE_COLA_FULL_PCT", "DOSE_COFFEE_FULL_PCT"),
    ("DOSE_COLA_ENERGY_PCT", "DOSE_COFFEE_ENERGY_PCT"),
    ("DOSE_COLA_MOOD_PCT", "DOSE_COFFEE_MOOD_PCT"),
    ("DOSE_COLA_WEIGHT_PCT", "DOSE_COFFEE_WEIGHT_PCT"),
    ("DEMO_PICK_COLA_PCT", "DEMO_PICK_COFFEE_PCT"),
];

/// The setter for a config key (any case), through an old name too.
pub fn cfg_setter(key: &str) -> Option<Setter> {
    let key = BORNPETS_CFG_ALIASES
        .iter()
        .find(|(old, _)| key.eq_ignore_ascii_case(old))
        .map_or(key, |&(_, new)| new);
    BORNPETS_CFG_KEYS
        .iter()
        .find(|(name, _)| key.eq_ignore_ascii_case(name))
        .map(|&(_, setter)| setter)
}

/// Last minute of the day, the most a bedtime or getting-up line can be.
const DAY_LAST_MIN: u16 = super::demo::MINUTES_PER_DAY - 1;
/// The widest bedtime or getting-up window (ten hours).
const DAY_SPREAD_MAX_MIN: u16 = 600;
/// Largest temperament the demo block holds.
const LAZY_MAX: u8 = super::demo::LAZY_MAX;
/// The demo soft delay and dry-out interval are saved in one byte.
const BYTE_MAX_MIN: u16 = u8::MAX as u16;

#[inline]
fn clamp_u16(v: u32) -> u16 {
    if v > u16::MAX as u32 {
        u16::MAX
    } else {
        v as u16
    }
}

/// Multiplier percentages (energy, thirst, mood penalties, dose
/// amplifiers) stay at most 1000 %, so no product of levels wraps.
#[inline]
fn clamp_mult(v: u32) -> u16 {
    v.min(1000) as u16
}

/// Band edges are percentages; keep them inside 1..=99.
#[inline]
fn clamp_pct(v: u32) -> u8 {
    v.clamp(1, 99) as u8
}

/// Per band pair (`lo < hi`): a pair that is not strictly ordered falls
/// back, both values, to `preset`.
pub fn validate(t: &Thresholds, preset: &Thresholds) -> Thresholds {
    let mut r = *t;
    if r.FULL_LO_PCT >= r.FULL_HI_PCT {
        r.FULL_LO_PCT = preset.FULL_LO_PCT;
        r.FULL_HI_PCT = preset.FULL_HI_PCT;
    }
    if r.WATER_LO_PCT >= r.WATER_HI_PCT {
        r.WATER_LO_PCT = preset.WATER_LO_PCT;
        r.WATER_HI_PCT = preset.WATER_HI_PCT;
    }
    if r.FUN_LO_PCT >= r.FUN_HI_PCT {
        r.FUN_LO_PCT = preset.FUN_LO_PCT;
        r.FUN_HI_PCT = preset.FUN_HI_PCT;
    }
    if r.WEIGHT_LO_PCT >= r.WEIGHT_HI_PCT {
        r.WEIGHT_LO_PCT = preset.WEIGHT_LO_PCT;
        r.WEIGHT_HI_PCT = preset.WEIGHT_HI_PCT;
    }
    if r.BOOZE_HI_PCT >= r.BOOZE_DANGER_PCT {
        r.BOOZE_HI_PCT = preset.BOOZE_HI_PCT;
        r.BOOZE_DANGER_PCT = preset.BOOZE_DANGER_PCT;
    }
    if r.BATTLE_LOW_MOOD_PCT >= r.BATTLE_HIGH_MOOD_PCT {
        r.BATTLE_LOW_MOOD_PCT = preset.BATTLE_LOW_MOOD_PCT;
        r.BATTLE_HIGH_MOOD_PCT = preset.BATTLE_HIGH_MOOD_PCT;
    }
    if r.BATTLE_FORM_OFF_PCT >= r.BATTLE_FORM_IN_PCT || r.BATTLE_FORM_IN_PCT >= r.BATTLE_FORM_TOP_PCT {
        r.BATTLE_FORM_OFF_PCT = preset.BATTLE_FORM_OFF_PCT;
        r.BATTLE_FORM_IN_PCT = preset.BATTLE_FORM_IN_PCT;
        r.BATTLE_FORM_TOP_PCT = preset.BATTLE_FORM_TOP_PCT;
    }
    if r.BATTLE_PRIZE_LATER_HAX > r.BATTLE_PRIZE_FIRST_HAX {
        r.BATTLE_PRIZE_FIRST_HAX = preset.BATTLE_PRIZE_FIRST_HAX;
        r.BATTLE_PRIZE_LATER_HAX = preset.BATTLE_PRIZE_LATER_HAX;
    }
    // A fight that costs no rest at all would let a pet battle in a loop.
    r.BATTLE_COOLDOWN = r.BATTLE_COOLDOWN.max(1);
    if r.DIABETES_WEIGHT_LO_PCT >= r.DIABETES_WEIGHT_HI_PCT {
        r.DIABETES_WEIGHT_LO_PCT = preset.DIABETES_WEIGHT_LO_PCT;
        r.DIABETES_WEIGHT_HI_PCT = preset.DIABETES_WEIGHT_HI_PCT;
    }
    // Demo (plan 3). Food and drink draws that add up over 100 % fall back.
    if r.DEMO_PICK_APPLE_PCT as u16
        + r.DEMO_PICK_SALAD_PCT as u16
        + r.DEMO_PICK_PIZZA_PCT as u16
        + r.DEMO_PICK_FRIK_PCT as u16
        > 100
    {
        r.DEMO_PICK_APPLE_PCT = preset.DEMO_PICK_APPLE_PCT;
        r.DEMO_PICK_SALAD_PCT = preset.DEMO_PICK_SALAD_PCT;
        r.DEMO_PICK_PIZZA_PCT = preset.DEMO_PICK_PIZZA_PCT;
        r.DEMO_PICK_FRIK_PCT = preset.DEMO_PICK_FRIK_PCT;
    }
    if r.DEMO_PICK_WATER_PCT as u16 + r.DEMO_PICK_COFFEE_PCT as u16 > 100 {
        r.DEMO_PICK_WATER_PCT = preset.DEMO_PICK_WATER_PCT;
        r.DEMO_PICK_COFFEE_PCT = preset.DEMO_PICK_COFFEE_PCT;
    }
    if !(r.DEMO_HEALTH_FLOOR_PCT < r.DEMO_HELP_HEALTH_PCT
        && r.DEMO_HELP_HEALTH_PCT < r.DEMO_ON_HEALTH_PCT
        && r.DEMO_HELP_HEALTH_PCT <= r.DEMO_MED_HEALTH_PCT)
    {
        r.DEMO_HEALTH_FLOOR_PCT = preset.DEMO_HEALTH_FLOOR_PCT;
        r.DEMO_HELP_HEALTH_PCT = preset.DEMO_HELP_HEALTH_PCT;
        r.DEMO_ON_HEALTH_PCT = preset.DEMO_ON_HEALTH_PCT;
        r.DEMO_MED_HEALTH_PCT = preset.DEMO_MED_HEALTH_PCT;
    }
    // A sleeper wakes below `WAKE_FULL` and an awake pet eats below
    // `EDGE_FULL`: a wake line above the eat line would put a tired pet to
    // bed and wake it again at one boundary, for ever. Clamp, never reorder.
    r.DEMO_WAKE_FULL_PCT = r.DEMO_WAKE_FULL_PCT.min(r.DEMO_EDGE_FULL_PCT);
    // Play below the soft fun line, relax above the wired line: the soft line
    // stays under the wired one, or the two undo each other.
    if r.DEMO_SOFT_FUN_PCT >= r.DEMO_WIRED_FUN_PCT {
        r.DEMO_SOFT_FUN_PCT = preset.DEMO_SOFT_FUN_PCT;
        r.DEMO_WIRED_FUN_PCT = preset.DEMO_WIRED_FUN_PCT;
    }
    if r.DEMO_RICH_FOOD_PCT >= r.DEMO_LIGHT_FOOD_PCT {
        r.DEMO_RICH_FOOD_PCT = preset.DEMO_RICH_FOOD_PCT;
        r.DEMO_LIGHT_FOOD_PCT = preset.DEMO_LIGHT_FOOD_PCT;
    }
    // A base plus its spread must fit the field it is stored in: the soft
    // delay and the dry-out interval are one saved byte, the rest a u16.
    for (base, spread, max) in [
        (&mut r.DEMO_REACT_BASE_MIN, &mut r.DEMO_REACT_SPREAD_MIN, BYTE_MAX_MIN),
        (&mut r.DEMO_DRY_BASE_MIN, &mut r.DEMO_DRY_SPREAD_MIN, BYTE_MAX_MIN),
        (&mut r.DEMO_WHIM_BASE_MIN, &mut r.DEMO_WHIM_SPREAD_MIN, u16::MAX),
        (&mut r.DEMO_NAP_BASE_MIN, &mut r.DEMO_NAP_SPREAD_MIN, u16::MAX),
        (&mut r.DEMO_RISE_BASE_MIN, &mut r.DEMO_RISE_SPREAD_MIN, u16::MAX),
    ] {
        *base = (*base).min(max - 1);
        *spread = (*spread).min(max - *base).max(1);
    }
    // Backgrounds (plan 7): the floor lines rise, and the night window has
    // a start and an end that differ.
    let lines = [
        r.FLOOR_CRUMBS_DIRT_PCT,
        r.FLOOR_PEEL_DIRT_PCT,
        r.FLOOR_CAN_DIRT_PCT,
        r.FLOOR_SOCK_DIRT_PCT,
        r.FLOOR_PIZZA_DIRT_PCT,
    ];
    if lines[0] < 1 || lines.windows(2).any(|w| w[0] >= w[1]) {
        r.FLOOR_CRUMBS_DIRT_PCT = preset.FLOOR_CRUMBS_DIRT_PCT;
        r.FLOOR_PEEL_DIRT_PCT = preset.FLOOR_PEEL_DIRT_PCT;
        r.FLOOR_CAN_DIRT_PCT = preset.FLOOR_CAN_DIRT_PCT;
        r.FLOOR_SOCK_DIRT_PCT = preset.FLOOR_SOCK_DIRT_PCT;
        r.FLOOR_PIZZA_DIRT_PCT = preset.FLOOR_PIZZA_DIRT_PCT;
    }
    if r.BG_NIGHT_FROM_MIN == r.BG_NIGHT_TO_MIN {
        r.BG_NIGHT_FROM_MIN = preset.BG_NIGHT_FROM_MIN;
        r.BG_NIGHT_TO_MIN = preset.BG_NIGHT_TO_MIN;
    }
    r
}

#[cfg(any(test, feature = "simulator"))]
pub use template::{badge_preset, cfg_template};

/// Host-only: needs `alloc`, which the badge build does not have.
#[cfg(any(test, feature = "simulator"))]
mod template {
    extern crate alloc;
    use super::{BORNPETS_CFG_KEYS, Mode, Thresholds};
    use alloc::string::String;
    use core::fmt::Write;

    const BADGE_HATCHING_TICKS: u32 = 6;
    const HEAD: &str = "# BornPets v3 balance. Uncomment a line to change it. The old firmware keeps using BORNPETS.CFG.";

    /// A mode's preset as the badge runs it: the simulator build hatches in
    /// 1 tick, the badge in 6 (both presets).
    pub fn badge_preset(mode: Mode) -> Thresholds {
        let mut t = mode.preset();
        t.HATCHING_TICKS = BADGE_HATCHING_TICKS as u16;
        t
    }

    /// Exact text of `assets/to-badge/BPETS_V3.CFG`: every key as a
    /// comment with its Classic value and the Casual value, no active rows.
    pub fn cfg_template() -> String {
        let mut out = String::from(HEAD);
        out.push('\n');
        for (key, _) in BORNPETS_CFG_KEYS {
            let classic = badge_preset(Mode::Classic).value_of(key).expect("key is a field");
            let casual = badge_preset(Mode::Casual).value_of(key).expect("key is a field");
            let _ = writeln!(out, "# {key}={classic}   # Casual {casual}");
        }
        out
    }
}
