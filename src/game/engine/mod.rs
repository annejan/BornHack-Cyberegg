//! Cyber Ægg game engine — delta-T progression with boundary-based wake
//! scheduling.
//!
//! Instead of ticking every 10 seconds, the engine:
//! 1. Computes elapsed ticks since the last update.
//! 2. Applies all stat changes for that delta in one step.
//! 3. Computes the next boundary crossing time across all stats.
//! 4. Schedules a wake-up at the next visible change (or MAX_SLEEP_TICKS()).
//!
//! This lets the CPU sleep for minutes or hours when nothing interesting
//! is about to happen, saving significant battery on the badge.

pub mod advance;
pub mod anim_files;
pub mod causes;
pub mod combat;
pub mod demo;
pub mod debug;
pub mod doses;
pub mod drink;
pub mod food;
pub mod hint;
pub mod needs;
pub mod regime;
pub mod scene;
#[cfg(feature = "simulator")]
pub mod sim;
pub mod solver;
pub mod status;
pub mod thresholds;
pub mod to_display;

use advance::SLOW_SCALE;
use needs::{MAX, P};
use thresholds::*;
pub use debug::{DebugInfo, DebugPoke};
pub use drink::DrinkKind;
pub use food::FoodKind;
pub use to_display::DisplayAnim;

/// Test hook: `advance` calls made by `update` (trials included). Preview
/// clones (`preview`, `preview_sleep`, `next_wake_tick`) count too; only
/// `sim::run_full` reads it, as a delta around its own `next_wake_tick`, and
/// nothing a run does draws or previews, so the counts stay its own.
#[cfg(feature = "simulator")]
thread_local! {
    pub static ADVANCE_CALLS: core::cell::Cell<u32> = const { core::cell::Cell::new(0) };
}

/// Engine ticks per simulated minute.
pub const TICKS_PER_MINUTE: u32 = 6;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Kind of pet — determines which sprite set to use.
///
/// Pets are not enumerated in code: the roster is whatever `PETS.CFG`
/// declares.  Adding one means dropping its `PPAAFF.PCX` sprites on the USB
/// partition and adding a manifest line — the filename generation in
/// `anim_files` and the selection screen pick it up automatically.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "embassy-core", derive(defmt::Format))]
///
/// Represented as a single byte — the sprite-prefix (`PP` in `PPAAFF.PCX`).
/// Which bytes name a real pet is decided at runtime by
/// [`crate::game::pet_registry`], populated from the `PETS.CFG` manifest.
/// Persisted as this byte, so saves round-trip regardless of the roster.
#[repr(transparent)]
pub struct PetKind(pub u8);

impl PetKind {
    /// Sprite prefix `0` — the placeholder used where a kind is structurally
    /// required but none has been chosen: pre-`pet_kind` saves, empty realm
    /// slots, and an unresolvable battle opponent.  Carries no guarantee of
    /// being in the roster; `PETS.CFG` decides whether prefix 0 is a pet.
    pub const DEFAULT: PetKind = PetKind(0);

    /// Reconstruct from the persisted / sprite-prefix byte.  Any byte is a
    /// valid id; whether it resolves to a real pet is decided by the registry.
    pub fn from_u8(v: u8) -> Self {
        PetKind(v)
    }

    /// The sprite-prefix / persisted byte.
    pub fn id(self) -> u8 {
        self.0
    }

    /// Human-readable name for the selection screen and Unicorn Realm.
    /// Resolves through the runtime registry populated from `PETS.CFG`.
    pub fn name(self) -> &'static str {
        crate::game::pet_registry::name_of(self.0)
    }

    /// All selectable pet kinds, in `PETS.CFG` order.  Empty until the
    /// manifest is installed, and on a badge whose manifest declares none.
    pub fn roster() -> &'static [PetKind] {
        crate::game::pet_registry::roster()
    }
}

/// Lifecycle phase of the pet.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "embassy-core", derive(defmt::Format))]
pub enum Phase {
    /// Waiting to hatch (countdown running).
    Hatching,
    /// Alive and active.
    Active,
    /// Pet is leaving (countdown running).
    Leaving,
    /// Pet has left — ready to start a new egg.
    Gone,
}

/// Level (percent) a `more food` station lifts fullness and hydration to.
pub const STATION_FOOD_FLOOR_PCT: i32 = 60;
/// Level (percent) a `more drugs` station lowers booze to, at most.
pub const STATION_BOOZE_CAP_PCT: i32 = 20;
/// HAX earned by the "Only pets" hobby (non-broke) branch.
pub const ONLYPETS_HOBBY_REWARD: u32 = 20;
/// Mini-game grades (0..=2) handed to `award_inspiration`: an interface of the
/// games, not a tunable. Every finished game gets a call: a loss too, since
/// the player put in the work. Games without a draw pass `GRADE_WIN` or
/// `GRADE_LOSS`.
pub const GRADE_LOSS: u8 = 0;
pub const GRADE_DRAW: u8 = 1;
pub const GRADE_WIN: u8 = 2;
/// HAX earned for winning a mini-game.
pub const MINIGAME_HAX_REWARD: u32 = 15;
/// HAX earned for a drawn mini-game.
pub const MINIGAME_HAX_DRAW: u32 = 10;
/// HAX earned for a lost mini-game.
pub const MINIGAME_HAX_LOSS: u32 = 5;
/// HAX cost of the basic Play action.
pub const PLAY_HAX_COST: u32 = 10;
/// HAX cost of a drug dose (Ozempic / Medicate / Rehab).
pub const DRUG_HAX_COST: u32 = 15;
/// HAX cost of an Aspirine (the Heal action).
pub const ASPIRINE_HAX_COST: u32 = 1;
/// Hard-mode multiplier on medication (Insulin / Ozempic).
pub const MEDICATION_HARD_MULT: u32 = 3;
/// Hard-mode multiplier on Rehab.
pub const REHAB_HARD_MULT: u32 = 5;

/// HAX cost of a medication dose (Insulin / Ozempic) in the current mode.
pub fn medication_price(hard: bool) -> u32 {
    DRUG_HAX_COST * if hard { MEDICATION_HARD_MULT } else { 1 }
}

/// HAX cost of Rehab in the current mode.
pub fn rehab_price(hard: bool) -> u32 {
    DRUG_HAX_COST * if hard { REHAB_HARD_MULT } else { 1 }
}

/// Active user action (mutually exclusive).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "embassy-core", derive(defmt::Format))]
pub enum Action {
    Feed,
    Heal,
    Play,
    Exercise,
    Medicate,
    /// Accelerated weight loss — not gated on being diabetic, unlike
    /// `Medicate` (insulin).
    Ozempic,
    Drink,
    /// Treatment for alcoholism — gated on `alcoholic`, mirrors
    /// `Medicate`'s relationship to diabetic.
    Rehab,
    /// "Only pets" work/hobby action — earns HAX. Only reachable when
    /// `money_enabled`. See `GameState::only_pets`.
    OnlyPets,
    /// Bath: takes the dirt off (dose in Task 9b).
    Wash,
    /// Lie back: trades fun for calm (dose in Task 9b).
    Relax,
}

impl Action {
    /// Persisted discriminant (format 3). Explicit rather than a bare
    /// `as u8` cast, and paired 1:1 with `from_u8`, so a new variant is a
    /// visible two-place edit.
    ///
    /// Byte `2` is unused: it was `Relax` before the `drained` stat was
    /// removed, and a format-2 save never carries a running action into
    /// format 3 (the upgrade drops it). `Wash` and `Relax` take `10` and `11`.
    pub fn to_u8(self) -> u8 {
        match self {
            Action::Feed => 0,
            Action::Heal => 1,
            Action::Play => 3,
            Action::Exercise => 4,
            Action::Medicate => 5,
            Action::Ozempic => 6,
            Action::Drink => 7,
            Action::Rehab => 8,
            Action::OnlyPets => 9,
            Action::Wash => 10,
            Action::Relax => 11,
        }
    }

    /// Inverse of `to_u8`; byte `2` and anything unknown decode as `None`.
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Action::Feed),
            1 => Some(Action::Heal),
            3 => Some(Action::Play),
            4 => Some(Action::Exercise),
            5 => Some(Action::Medicate),
            6 => Some(Action::Ozempic),
            7 => Some(Action::Drink),
            8 => Some(Action::Rehab),
            9 => Some(Action::OnlyPets),
            10 => Some(Action::Wash),
            11 => Some(Action::Relax),
            _ => None,
        }
    }
}

/// Mini-games each track their own post-win cooldown so winning one
/// doesn't lock the player out of the others — nudges variety.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "embassy-core", derive(defmt::Format))]
pub enum MiniGame {
    TicTacToe,
    LightsOut,
    BlackHole,
    BornJeweled,
}

impl MiniGame {
    pub const ALL: [MiniGame; 4] = [
        MiniGame::TicTacToe,
        MiniGame::LightsOut,
        MiniGame::BlackHole,
        MiniGame::BornJeweled,
    ];
}

/// The complete game state.  Serialisable to ekv for save/restore.
#[derive(Clone, Debug, PartialEq)]
pub struct GameState {
    // Pet kind.
    pub pet_kind: PetKind,

    // Needs. v3 levels are good-high (u16 scale, `needs::P` = one percent).
    pub fullness: u16,
    pub energy: u16,
    pub health: u16,
    pub mood: u16,
    /// Body mass: 50 % is ideal, both ends are unhealthy.
    pub weight: u16,
    pub booze: u16,
    pub hydration: u16,
    pub fun: u16,
    pub strain: u16,
    pub dirt: u16,
    pub fitness: u16,

    // Minute timers (1 engine minute = 6 ticks).
    pub sleep_min: u16,
    pub oversleep_min: u16,
    pub good_sleep_min: u16,
    pub rested_min: u16,
    pub injured_min: u16,
    pub groggy_min: u16,
    pub hangover_min: u16,
    pub rehab_min: u16,
    pub recovering_min: u16,
    pub junk_min: u16,
    pub ozempic_min: u16,
    pub med_cd_min: u16,
    pub insulin_min: u16,
    pub ozempic_cd_min: u16,
    pub reversal_min: u16,
    /// Caffeine from coffee, in minutes: each cup adds `COFFEE_WINDOW_MIN`,
    /// one wears off a minute. A cup that takes it past the wired line wires
    /// the pet (`doses::coffee_would_wire`).
    pub caffeine_min: u16,

    // Flags and small counters.
    pub booze_peak: bool,
    pub deprived: bool,
    pub night: bool,
    pub no_groggy: bool,
    pub short_nights: u8,
    pub good_nights: u8,
    /// Ticks left over when `update` converts ticks to whole minutes.
    pub tick_rem: u8,
    pub care_good_min: u32,
    pub care_awake_min: u32,
    /// Wall-clock unix time of the last save, 0 when unknown (clock unset).
    pub saved_unix: u32,

    /// Demo mode (plan 3), saved in bytes 149..160.
    pub demo: demo::Demo,
    /// The demo pet's dice. Transient: `new_egg` seeds it from `seed`,
    /// `from_bytes` derives it from the age and generation; never 0.
    pub(crate) demo_rng: u32,
    /// Local minute of the day (0..1440) at `last_update_tick`; `None` while
    /// the wall clock is unknown (the pet keeps its own rhythm). Transient:
    /// `lifecycle` hands it in through `sync_clock` before each update.
    pub(crate) day_min: Option<u16>,
    /// The armed soft delay was drawn as the dry-out interval. Transient: a
    /// reload at worst re-arms the delay once.
    pub(crate) soft_dry: bool,
    /// True only inside `demo::act`: the starters are the pet's own, so they
    /// pause nobody, wake nobody grumpy and earn no thanks. Transient.
    pub(crate) self_care: bool,
    /// What the demo pet last did by itself, and the status line it was taken
    /// on (the note shows while the line is unchanged). Transient; the
    /// player's next action clears it.
    pub(crate) note: Option<(demo::Note, status::StatusLine)>,
    /// The player's running action woke the pet grumpy: no thanks for it.
    /// Transient.
    pub(crate) woke_grumpy: bool,

    // Diabetes — permanent once triggered by sustained overweight.
    pub diabetic: bool,
    /// Minutes spent overweight; once it reaches the onset time `diabetic`
    /// flips true for the rest of this pet's life.
    /// Counted in quarter-minutes (`advance::SLOW_SCALE`).
    pub diabetes_min: u32,

    // Dependence — permanent once triggered by sustained drinking.
    pub dependent: bool,
    /// Counted in quarter-minutes (`advance::SLOW_SCALE`): +4 per minute
    /// while drunk, -1 per minute while sober.
    pub dependence_min: u32,

    // Traits (higher = better).
    pub vitality: u16,
    pub curiosity: u16,
    pub resilience: u16,

    // Timing.
    pub last_update_tick: u32,
    pub age_ticks: u32,

    // Lifecycle.
    pub phase: Phase,
    pub hatching_countdown: u16,
    /// Leaving countdown in half-minutes (`advance::LEAVING_SCALE`): +2 per
    /// minute at health 0, -1 per minute otherwise; Gone at 720 minutes.
    pub leaving_min: u32,
    pub generation: u16,

    // Mesh Battle record — lifetime counters, never reset by anything
    // other than a fresh egg (see `new_egg`/`new_generation`).
    pub wins: u16,
    pub losses: u16,
    /// The record of fights while demo mode is on, apart from the player's.
    pub demo_wins: u16,
    pub demo_losses: u16,
    /// Minutes until a win pays the first prize of the day again; counts
    /// down with time. Not a solver boundary: nothing visible reads it.
    pub prize_min: u16,
    /// Minutes left of the mood penalty for declining a challenge
    /// (the mood term is Task 4's).
    pub decline_min: u16,

    // HAX currency — belongs to the current pet, resets with a new egg.
    /// HAX balance. New egg starts at 100.
    pub money: u32,
    /// Chosen at pet creation, persisted. When `false` the whole money
    /// layer is inert: no HAX display, no prices, no rewards.
    pub money_enabled: bool,
    /// Chosen at pet creation, persisted. Only meaningful when
    /// `money_enabled` is true (hard mode implies money is on) — changes
    /// PRICE AMOUNTS only (healthy food, medication, rehab all cost
    /// more); it never changes which actions are gated on affordability,
    /// that's still entirely `money_enabled`'s job.
    pub hard_mode: bool,

    // Action state.
    pub active_action: Option<Action>,
    /// Which food is being eaten during an in-progress `Action::Feed`.
    /// Only meaningful mid-action; saved with the action (format 3).
    pub active_food: Option<FoodKind>,
    /// Which drink is being drunk during an in-progress `Action::Drink`.
    /// Saved with the action, like `active_food`.
    pub active_drink: Option<DrinkKind>,
    pub action_ticks_remaining: u8,
    /// Cooldown between mesh Battles — resolves instantly (no in-progress
    /// duration), so this is the only battle-related field of its kind.
    pub cooldown_battle: u16,
    /// Per-mini-game cooldown after winning.  Each game tracks its
    /// own counter so winning one doesn't gate the others.  None of
    /// these are persisted to flash — rebooting clears them.
    pub cooldown_tictactoe: u16,
    pub cooldown_lightsout: u16,
    pub cooldown_blackhole: u16,
    pub cooldown_bornjeweled: u16,

    // Sleep.
    pub is_sleeping: bool,

    // Hibernation — all progression frozen, time stands still.
    pub hibernating: bool,
    /// Total ticks spent in hibernation during this pet's life.
    pub hibernate_ticks: u32,

    // Persistence — tracks when state was last saved to flash.
    /// `age_ticks` at the time of the last successful save.
    /// Not part of the game logic — only used by the save system.
    last_save_tick: u32,

    /// Transient flag (not serialized): when true, the next
    /// `needs_save()` check returns true regardless of the interval.
    /// Set on game start and phase transitions so the save happens
    /// immediately rather than waiting 15 minutes.
    save_pending: bool,

    /// Transient flag (not serialized): set when the pet transitions to
    /// Gone, so lifecycle can record it in the Unicorn Realm.
    pub realm_pending: bool,

    /// Transient flag (not serialized): set when hatching completes,
    /// so lifecycle can prompt the player to name their pet.
    pub naming_pending: bool,

    /// Transient (never saved): why the last doses and wakes went badly,
    /// drained by the UI with `take_causes`.
    pub causes: causes::CauseQueue,
    /// Transient (never saved): what was wrong when the pet went to bed,
    /// read at waking for the "bad night" cause.
    pub bed_why: Option<causes::Unsettled>,
}

// ---------------------------------------------------------------------------
// Construction
// ---------------------------------------------------------------------------

impl GameState {
    /// Create a new egg with randomised traits from a seed.
    pub fn new_egg(seed: u64, kind: PetKind) -> Self {
        // Simple xorshift64 for deterministic trait generation.
        let mut rng = seed;
        let mut next = || -> u16 {
            rng ^= rng << 13;
            rng ^= rng >> 7;
            rng ^= rng << 17;
            let range = (MAX_TRAIT() - MIN_TRAIT()) as u64;
            MIN_TRAIT() + ((rng % range) as u16)
        };

        let vitality = next();
        let curiosity = next();
        let resilience = next();

        Self {
            pet_kind: kind,

            fullness: 60 * P as u16,
            energy: 80 * P as u16,
            health: (85 * P - (MAX - vitality as i32) / 8) as u16,
            mood: 70 * P as u16,
            weight: 50 * P as u16,
            booze: 0,
            hydration: 65 * P as u16,
            fun: 50 * P as u16,
            strain: 0,
            dirt: 10 * P as u16,
            fitness: 60 * P as u16,

            sleep_min: 0,
            oversleep_min: 0,
            good_sleep_min: 0,
            rested_min: 0,
            injured_min: 0,
            groggy_min: 0,
            hangover_min: 0,
            rehab_min: 0,
            recovering_min: 0,
            junk_min: 0,
            ozempic_min: 0,
            med_cd_min: 0,
            insulin_min: 0,
            ozempic_cd_min: 0,
            reversal_min: 0,
            caffeine_min: 0,

            booze_peak: false,
            deprived: false,
            night: false,
            no_groggy: false,
            short_nights: 0,
            good_nights: 0,
            tick_rem: 0,
            care_good_min: 0,
            care_awake_min: 0,
            saved_unix: 0,
            demo: demo::Demo::default(),
            demo_rng: demo::seed_rng(seed, 0),
            day_min: None,
            soft_dry: false,
            self_care: false,
            note: None,
            woke_grumpy: false,

            diabetic: false,
            diabetes_min: 0,

            dependent: false,
            dependence_min: 0,

            vitality,
            curiosity,
            resilience,

            last_update_tick: 0,
            age_ticks: 0,

            phase: Phase::Hatching,
            hatching_countdown: HATCHING_TICKS(),
            leaving_min: 0,
            generation: 0,

            wins: 0,
            losses: 0,
            demo_wins: 0,
            demo_losses: 0,
            prize_min: 0,
            decline_min: 0,

            money: 100,
            money_enabled: true,
            hard_mode: false,

            active_action: None,
            active_food: None,
            active_drink: None,
            action_ticks_remaining: 0,
            cooldown_battle: 0,
            cooldown_tictactoe: 0,
            cooldown_lightsout: 0,
            cooldown_blackhole: 0,
            cooldown_bornjeweled: 0,

            is_sleeping: false,
            hibernating: false,
            hibernate_ticks: 0,
            last_save_tick: 0,
            save_pending: true,
            realm_pending: false,
            naming_pending: false,
            causes: causes::CauseQueue::default(),
            bed_why: None,
        }
    }

    /// Start a new generation (pet left, hatch new egg).
    pub fn new_generation(&mut self, seed: u64, kind: PetKind) {
        let next_gen = self.generation + 1;
        *self = Self::new_egg(seed, kind);
        self.generation = next_gen;
    }
}

// ---------------------------------------------------------------------------
// Delta-T update
// ---------------------------------------------------------------------------

impl GameState {
    /// Age the pet by `minutes` of time the badge was off (boot catch-up).
    /// Runs the normal `update` path, then restores `last_update_tick`, so
    /// the boot tick base is not moved past the real tick counter (else the
    /// next `update` would see a backwards clock and freeze time).
    pub fn catch_up(&mut self, minutes: u32) {
        let base = self.last_update_tick;
        self.update(base.saturating_add(minutes.saturating_mul(TICKS_PER_MINUTE)));
        self.last_update_tick = base;
    }

    /// Advance the game state to `now_tick`.
    ///
    /// Time moves in whole engine minutes (1 minute = 6 ticks); the sub-minute
    /// remainder is carried in `tick_rem`.  A backwards clock counts as zero
    /// elapsed.  Each loop pass advances one solver segment, so a long gap
    /// costs a handful of closed-form steps, not one per minute.
    pub fn update(&mut self, now_tick: u32) {
        let total_delta = now_tick.saturating_sub(self.last_update_tick);
        if total_delta == 0 {
            return;
        }
        self.last_update_tick = now_tick;

        // Hibernation: time stands still.  Track hibernated time but
        // don't advance age or any game state.
        if self.hibernating {
            self.hibernate_ticks += total_delta;
            return;
        }

        if self.phase == Phase::Gone {
            return;
        }

        self.age_ticks += total_delta;

        match self.phase {
            Phase::Gone => unreachable!(),
            Phase::Hatching => {
                let consumed = total_delta.min(self.hatching_countdown as u32);
                self.hatching_countdown -= consumed as u16;
                if self.hatching_countdown == 0 {
                    self.phase = Phase::Active;
                    self.save_pending = true;
                    self.naming_pending = true;
                }
                return;
            }
            Phase::Leaving | Phase::Active => {}
        }

        // A player's running action eats the fresh ticks first and does not
        // age the needs; the sub-minute remainder is never spent on it.
        let mut fresh = total_delta;
        if self.active_action.is_some() && !self.demo.pet_action {
            fresh = self.consume_action_ticks(fresh);
        }
        // The engine-time stream: the fresh ticks and the carried remainder.
        // A pet's own action is spent from this stream, remainder included,
        // so it starts and ends at the same stream position wherever the
        // update is cut.
        let mut ticks = fresh + self.tick_rem as u32;
        // Minutes still to step singly after a runaway target was seen.
        let mut steep = 0u32;
        while self.phase != Phase::Gone {
            // The demo pet looks after itself at every boundary (and at the
            // start, where the player or a switch may have left a choice
            // pending). Instant care takes no ticks; animated care starts
            // an action that the stream then runs.
            demo::run(self);
            if self.demo.pet_action {
                ticks = self.consume_action_ticks(ticks);
                if self.demo.pet_action {
                    break; // still running: the stream is spent
                }
                continue; // it ended: the policy looks again at once
            }
            let minutes = ticks / TICKS_PER_MINUTE;
            if minutes == 0 {
                break;
            }
            let dt = self.segment(minutes, &mut steep);
            #[cfg(feature = "simulator")]
            ADVANCE_CALLS.with(|c| c.set(c.get() + 1));
            #[cfg(feature = "simulator")]
            let before = sim::Snap::of(self);
            advance::advance(self, dt);
            #[cfg(feature = "simulator")]
            sim::note_segment(before, sim::Snap::of(self), dt);
            ticks -= dt * TICKS_PER_MINUTE;
        }
        // Gone partway through the gap: the pet lived only the time it was
        // advanced, not the rest of the gap.
        if self.phase == Phase::Gone && ticks > 0 {
            self.age_ticks -= ticks;
            ticks = 0;
        }
        self.tick_rem = ticks as u8;
    }

    /// Length in minutes of the next closed-form step, at most `minutes`
    /// (`steep` is the caller's single-minute budget after a runaway target).
    /// `advance(self, segment)` is exact: the solver's bound, shrunk further
    /// by trial advances on a copy.
    fn segment(&self, minutes: u32, steep: &mut u32) -> u32 {
        /// Single-minute segments taken after a runaway mood target: long
        /// enough to ride out a steep stretch, short enough to retry soon.
        const STEEP_MINUTES: u32 = 4;
        let mut dt = solver::seg_len(self, minutes).clamp(1, minutes);
        // Mood and health crossing a line of their own is not bounded by
        // the solver: shrink the segment until a trial advance on a copy
        // keeps the signature AND mood cannot have met its target midway
        // (it started on it, or moved the full step all the way), since
        // meeting a falling target turns mood round and can cross a line
        // and return with equal end signatures (R22a). At most log2(dt)
        // trials, each a closed-form step, so a long gap on the badge
        // still costs a handful of steps, not one per minute.
        let before = solver::sig(self);
        let (m0, t0) = (self.mood as i32, regime::mood_target(self));
        if *steep > 0 {
            *steep -= 1;
            dt = 1;
        }
        while dt > 1 {
            let mut trial = self.clone();
            #[cfg(feature = "simulator")]
            let mute = sim::mute();
            advance::advance(&mut trial, dt);
            #[cfg(feature = "simulator")]
            drop(mute);
            #[cfg(feature = "simulator")]
            ADVANCE_CALLS.with(|c| c.set(c.get() + 1));
            let (m1, t1) = (trial.mood as i32, regime::mood_target(&trial));
            // Mood and target are linear inside a segment only up to
            // integer truncation: each truncating term of `mood_target`
            // (`regime::TRUNCATING_TERMS`: four `* 3 / 2`, three `/ 2`) wobbles
            // the target by under 1 per minute, so R = 4 + 3 + 1 (R22c): the
            // terms plus one more for the rounding of the step. Mood on its target
            // is safe if the target moves at most MOOD_STEP - R per
            // minute; a full step is safe if the target led mood by R at
            // both ends, so it was never met midway.
            const R: i32 = regime::TRUNCATING_TERMS + 1;
            let step = thresholds::MOOD_STEP() as i32;
            let on_target = m0 == t0 && (t1 - t0).abs() + R <= (step - R) * dt as i32;
            let full_step = (m1 - m0).abs() == step * dt as i32
                && if m1 > m0 { t0 >= m0 + R && t1 >= m1 + R } else { t0 <= m0 - R && t1 <= m1 - R };
            let sig_same = solver::sig(&trial) == before;
            if sig_same && (on_target || full_step) {
                break;
            }
            // On its target but the target runs away faster than mood can
            // follow: shorter segments see the same slope, go straight to
            // one minute, and skip the trials for the next few minutes
            // (single minutes are always exact), instead of paying
            // log2(dt) trials every minute.
            if sig_same && m0 == t0 && (t1 - t0).abs() > (step - R) * dt as i32 {
                dt = 1;
                // Single minutes are exact; skip the trials for this many
                // segments rather than re-pay the log2(dt) trials each time.
                *steep = STEEP_MINUTES;
                break;
            }
            dt = dt.div_ceil(2);
        }
        dt
    }

    /// Consume action ticks from delta, applying action effects.
    /// Returns remaining delta after action ticks are consumed.
    fn consume_action_ticks(&mut self, delta: u32) -> u32 {
        let used = delta.min(self.action_ticks_remaining as u32);
        #[cfg(feature = "simulator")]
        sim::note_action_ticks(used);
        self.action_ticks_remaining -= used as u8;
        if self.action_ticks_remaining == 0 {
            self.finish_action();
        }
        delta - used
    }

    /// Apply the running action's dose now and end the action. The animation
    /// end calls it; the demo pet's instant care calls it right after the
    /// starter, so the dose lands in the same minute. An action the pet
    /// started itself (`demo.pet_action`) is its own act all through the end:
    /// no pause, no `thanks!`, no grumpy wake, and the note goes up here.
    fn finish_action(&mut self) {
        self.action_ticks_remaining = 0;
        let own = core::mem::take(&mut self.demo.pet_action);
        let was_self_care = self.self_care;
        self.self_care |= own;
        let Some(action) = self.active_action.take() else {
            self.active_food = None;
            self.active_drink = None;
            self.self_care = was_self_care;
            return;
        };
        let player_in_demo = self.demo.on && !self.self_care;
        let penalty_before = regime::mood_penalty(self);
        let ((), queued) = self.queued_by(|s| {
            // A refusal leaves the pet untouched. The starters already turned
            // those away and needs do not age during an action (R23), so it
            // only happens when the saved state changed under a running
            // action (a reboot mid-action).
            if let Err(_refusal) = doses::apply(s, action) {
                #[cfg(feature = "embassy-core")]
                defmt::warn!("action dose refused at animation end: {}", _refusal);
            }
        });
        let warned = queued.iter().any(|c| c.is_warning());
        let helped = regime::mood_penalty(self) < penalty_before;
        // The player's care helped, nothing went wrong with it and it did
        // not wake the pet grumpy.
        if player_in_demo && helped && !warned && !self.woke_grumpy {
            self.causes.push(causes::Cause::Thanks);
        }
        if own {
            demo::note_own_act(self, action, &queued);
        }
        self.woke_grumpy = false;
        self.active_food = None;
        self.active_drink = None;
        self.self_care = was_self_care;
    }

    /// Run `f` and hand back the causes it queued, apart from what was
    /// waiting already (those stay queued, in order, with the new ones).
    pub(crate) fn queued_by<R>(
        &mut self,
        f: impl FnOnce(&mut Self) -> R,
    ) -> (R, causes::CauseQueue) {
        let mut all = core::mem::take(&mut self.causes);
        let out = f(self);
        let queued = core::mem::take(&mut self.causes);
        all.extend_from(&queued);
        self.causes = all;
        (out, queued)
    }

    /// The player did something: the demo pet's policy waits and its last
    /// note goes. The pet's own acts (`self_care`) pause nobody.
    fn player_acted(&mut self) {
        if self.demo.on && !self.self_care {
            self.demo.pause_min = DEMO_PAUSE_MIN();
            self.note = None;
        }
    }
}

// ---------------------------------------------------------------------------
// User actions
// ---------------------------------------------------------------------------

impl GameState {
    /// True when the pet is alive (Active or Leaving) and can receive actions.
    fn is_alive(&self) -> bool {
        self.phase == Phase::Active || self.phase == Phase::Leaving
    }

    /// Shared gate for every timed action starter: the pet must be alive
    /// with no action running, and the optional `price` affordable when money
    /// is enabled. Callers turn their `doses::refusal` away before calling.
    /// Only then is a sleeping pet woken (grumpy, by the player) and the
    /// price charged, so a rejected action costs nothing and wakes nobody.
    /// On success the action and its duration are set. Callers with extra
    /// state (`active_food`, `active_drink`) set it after a `true` return.
    fn start_action(&mut self, action: Action, price: Option<u32>, duration: u8) -> bool {
        if !self.is_alive()
            || self.active_action.is_some()
            || doses::refusal(self, action, None).is_some()
        {
            return false;
        }
        let price = price.filter(|_| self.money_live());
        if let Some(price) = price
            && !self.can_afford(price)
        {
            return false; // can't afford — action rejected
        }
        let ((), woke) = self.queued_by(|s| advance::wake(s, !s.self_care));
        self.woke_grumpy = woke.iter().any(|&c| c == causes::Cause::Grumpy);
        if let Some(price) = price {
            self.spend_money(price);
        }
        self.player_acted();
        self.active_action = Some(action);
        self.action_ticks_remaining = duration;
        true
    }

    /// The price of an action that has no food or drink choice.
    fn flat_price(&self, a: Action) -> Option<u32> {
        doses::price(a, None, None, self.hard_mode)
    }

    /// Start the feed action with the chosen food.  Returns false if not
    /// available.
    pub fn feed(&mut self, food: FoodKind) -> bool {
        let price = doses::price(Action::Feed, Some(food), None, self.hard_mode);
        if !self.start_action(Action::Feed, price, FEED_DURATION()) {
            return false;
        }
        self.active_food = Some(food);
        true
    }

    /// Start the heal action (Aspirine costs HAX).
    pub fn heal(&mut self) -> bool {
        self.start_action(Action::Heal, self.flat_price(Action::Heal), HEAL_DURATION())
    }

    /// Start the play action.
    pub fn play(&mut self) -> bool {
        self.start_action(Action::Play, self.flat_price(Action::Play), PLAY_DURATION())
    }

    /// Send the pet to "Only pets" to earn HAX. Timed action like Play.
    pub fn only_pets(&mut self) -> bool {
        self.start_action(Action::OnlyPets, None, PLAY_DURATION())
    }

    /// Diabetic with the insulin window run out ("no insulin").
    pub fn is_diabetic_unmedicated(&self) -> bool {
        self.diabetic && self.insulin_min == 0
    }

    /// Alcoholic and the rehab-protection window has lapsed ("needs rehab").
    pub fn is_alcoholic_untreated(&self) -> bool {
        self.dependent && self.rehab_min == 0
    }

    /// What the onset takeovers read, one bit each: bit 0 diabetic, bit 1
    /// dependent. Part of the wake view: each flips at a counter bound, so
    /// the wake stays exact. (The "no insulin" and "needs rehab" prompts are
    /// in the status line.)
    pub fn onset_bits(&self) -> u8 {
        (self.diabetic as u8) | (self.dependent as u8) << 1
    }

    /// Start a bath.
    pub fn wash(&mut self) -> bool {
        self.start_action(Action::Wash, None, PLAY_DURATION())
    }

    /// Start a rest.
    pub fn relax(&mut self) -> bool {
        self.start_action(Action::Relax, None, PLAY_DURATION())
    }

    /// Start the exercise action — the primary relief valve for `weight`.
    pub fn exercise(&mut self) -> bool {
        self.start_action(Action::Exercise, None, EXERCISE_DURATION())
    }

    /// Administer insulin. Only for a diabetic pet whose last dose is
    /// nearly worn off (see `doses::refusal`).
    pub fn medicate(&mut self) -> bool {
        self.start_action(
            Action::Medicate,
            self.flat_price(Action::Medicate),
            MEDICATE_DURATION(),
        )
    }

    /// Administer Ozempic — a stronger, faster-acting weight-loss
    /// treatment than Exercise. Unlike `medicate()`, this is *not*
    /// gated on being diabetic — any pet can take it.
    pub fn ozempic(&mut self) -> bool {
        self.start_action(
            Action::Ozempic,
            self.flat_price(Action::Ozempic),
            OZEMPIC_DURATION(),
        )
    }

    /// Start the drink action with the chosen drink.  Returns false if
    /// not available.
    pub fn drink(&mut self, drink: DrinkKind) -> bool {
        if doses::refusal(self, Action::Drink, Some(drink)).is_some() {
            return false;
        }
        if !self.start_action(
            Action::Drink,
            doses::price(Action::Drink, None, Some(drink), self.hard_mode),
            DRINK_DURATION(),
        ) {
            return false;
        }
        self.active_drink = Some(drink);
        true
    }

    /// Administer rehab treatment. Only for a dependent pet not already in
    /// rehab — mirrors `medicate()`.
    pub fn rehab(&mut self) -> bool {
        self.start_action(
            Action::Rehab,
            self.flat_price(Action::Rehab),
            REHAB_DURATION(),
        )
    }

    /// Start `a` through its public starter (`food` and `drink` where the
    /// action takes one). The one place the action-to-starter match lives:
    /// the menu's preview, the demo pet and the simulator all call it.
    pub fn start_choice(
        &mut self,
        a: Action,
        food: Option<FoodKind>,
        drink: Option<DrinkKind>,
    ) -> bool {
        match a {
            Action::Feed => food.is_some_and(|f| self.feed(f)),
            Action::Heal => self.heal(),
            Action::Play => self.play(),
            Action::Exercise => self.exercise(),
            Action::Medicate => self.medicate(),
            Action::Ozempic => self.ozempic(),
            Action::Drink => drink.is_some_and(|d| self.drink(d)),
            Action::Rehab => self.rehab(),
            Action::OnlyPets => self.only_pets(),
            Action::Wash => self.wash(),
            Action::Relax => self.relax(),
        }
    }

    /// Hand the queued causes to the UI and start a new queue.
    pub fn take_causes(&mut self) -> causes::CauseQueue {
        core::mem::take(&mut self.causes)
    }

    /// First warning cause that starting `a` now would produce (a care action
    /// on a sleeper includes the wake), or `None` when nothing goes wrong or
    /// the starter would refuse. Runs the real starter and dose on a clone, so
    /// the pet, its money and the real queue are untouched.
    pub fn preview(
        &self,
        a: Action,
        food: Option<FoodKind>,
        drink: Option<DrinkKind>,
    ) -> Option<causes::Cause> {
        #[cfg(feature = "simulator")]
        let _mute = sim::mute();
        let mut c = self.clone();
        c.causes = causes::CauseQueue::default();
        if !c.start_choice(a, food, drink) {
            return None;
        }
        // The dose lands exactly as `consume_action_ticks` lands it.
        c.finish_action();
        c.causes.iter().copied().find(|cause| cause.is_warning())
    }

    /// First warning cause of a night slept until rested: the pet is put to
    /// bed through the real starter on a clone, driven by the engine's own
    /// next-wake computation (event boundaries, no fixed step) until it wakes
    /// itself or is rested (`oversleep_min` grew past its bedtime value), then woken the way the
    /// player's Wake-up does. Oversleeping itself is Wake-up's business
    /// (`preview_wake`), so `Groggy` shows only for a pet that is not tired
    /// (energy at bedtime at or above `ENERGY_HI_EDGE_PCT`) and has no
    /// station rest (`no_groggy`). `None` when sleep is refused, the night
    /// is fine, or the pet does not survive it (Gone). The run is bounded in
    /// time, not in steps: a living pet always wakes itself once it has slept
    /// `SELF_WAKE_MIN`, so the clone never runs past that many minutes after
    /// bedtime.
    pub fn preview_sleep(&self) -> Option<causes::Cause> {
        if self.sleep_refusal().is_some() {
            return None;
        }
        #[cfg(feature = "simulator")]
        let _mute = sim::mute();
        let mut c = self.clone();
        c.causes = causes::CauseQueue::default();
        // The forecast covers the night only: the demo policy does not act
        // inside the clone.
        c.demo.on = false;
        if !c.sleep() {
            return None;
        }
        let night_end = c
            .last_update_tick
            .saturating_add((SELF_WAKE_MIN() as u32 + 1) * TICKS_PER_MINUTE);
        // Rested means the oversleep grew past what carried into bedtime, so a
        // re-nap with a carried oversleep is still simulated. Bedtime under
        // the reset line wipes the count, so it is read after `sleep()`.
        let carried = c.oversleep_min;
        while c.is_sleeping
            && c.phase != Phase::Gone
            && c.oversleep_min <= carried
            && c.last_update_tick < night_end
        {
            let next = c.next_wake_tick().max(c.last_update_tick + 1).min(night_end);
            c.update(next);
        }
        if c.phase == Phase::Gone {
            // The pet does not survive the night: nothing to forecast.
            return None;
        }
        debug_assert!(
            !c.is_sleeping || c.oversleep_min > carried,
            "preview night ended asleep and unrested"
        );
        let _ = doses::wake_up(&mut c);
        let warned = c.causes.iter().copied().find(|cause| cause.is_warning());
        let rested_at_bedtime = self.energy as i32 >= needs::ENERGY_HI_EDGE_PCT as i32 * needs::P;
        warned.or_else(|| {
            (rested_at_bedtime && !self.no_groggy).then_some(causes::Cause::Groggy)
        })
    }

    /// First warning cause that waking the pet now would produce; `None` when
    /// it is awake or nothing goes wrong.
    pub fn preview_wake(&self) -> Option<causes::Cause> {
        #[cfg(feature = "simulator")]
        let _mute = sim::mute();
        let mut c = self.clone();
        c.causes = causes::CauseQueue::default();
        doses::wake_up(&mut c).ok()?;
        c.causes.iter().copied().find(|cause| cause.is_warning())
    }

    /// Put the pet to sleep.
    pub fn sleep(&mut self) -> bool {
        let ok = self.sleep_refusal().is_none() && doses::fall_asleep(self).is_ok();
        if ok {
            self.player_acted();
        }
        ok
    }

    /// Why the pet cannot be put to sleep now, or `None`. Like every other
    /// action it is turned away while another one runs (Busy), so a feed
    /// never lands on a sleeping pet.
    pub fn sleep_refusal(&self) -> Option<doses::Reason> {
        if let Some(why) = self.cannot_act() {
            return Some(why);
        }
        self.is_sleeping
            .then_some(doses::Reason::Refused(doses::Refusal::Asleep))
    }

    /// Wake the pet up.
    pub fn wake(&mut self) -> bool {
        let ok = doses::wake_up(self).is_ok();
        if ok {
            self.player_acted();
        }
        ok
    }

    /// Hibernate the pet — all progression freezes.  Marks the
    /// state for immediate persistence so a power-off before the
    /// next 15-minute save tick still preserves the toggle.
    pub fn hibernate(&mut self) -> bool {
        if self.hibernating || self.phase == Phase::Gone {
            return false;
        }
        self.hibernating = true;
        self.request_save();
        true
    }

    /// The pet leaves for good now: phase Gone, no running action, not
    /// hibernating, `realm_pending` false (the caller has pushed the record).
    pub fn retire(&mut self) {
        self.phase = Phase::Gone;
        self.active_action = None;
        self.hibernating = false;
        self.realm_pending = false;
        self.naming_pending = false;
        self.request_save();
    }

    /// End hibernation — progression resumes from this moment.
    /// Marks the state for immediate persistence (see `hibernate`).
    pub fn wake_from_hibernation(&mut self) -> bool {
        if !self.hibernating {
            return false;
        }
        self.hibernating = false;
        self.request_save();
        true
    }

    /// Award a finished mini-game, `grade` being `GRADE_WIN`, `GRADE_DRAW` or
    /// `GRADE_LOSS`: HAX for that result (when `money_enabled`) plus the
    /// per-game cooldown, also after a loss.  Starts only that game's cooldown so other
    /// mini-games stay available — encourages variety.  Also gives the
    /// mini-game's fun dose (and costs a little energy).
    pub fn award_inspiration(&mut self, game: MiniGame, grade: u8) {
        if !self.is_alive() {
            return;
        }
        self.player_acted();
        doses::minigame(self, grade);
        match game {
            MiniGame::TicTacToe => self.cooldown_tictactoe = MINIGAME_COOLDOWN(),
            MiniGame::LightsOut => self.cooldown_lightsout = MINIGAME_COOLDOWN(),
            MiniGame::BlackHole => self.cooldown_blackhole = MINIGAME_COOLDOWN(),
            MiniGame::BornJeweled => self.cooldown_bornjeweled = MINIGAME_COOLDOWN(),
        }
        if self.money_live() {
            self.add_money(match grade {
                GRADE_LOSS => MINIGAME_HAX_LOSS,
                GRADE_DRAW => MINIGAME_HAX_DRAW,
                _ => MINIGAME_HAX_REWARD,
            });
        }
    }

    /// Total hours the pet has spent in hibernation during its life.
    pub fn hibernate_hours(&self) -> u32 {
        self.hibernate_ticks / 360 // 360 ticks = 1 hour
    }
}

// ---------------------------------------------------------------------------
// Next wake time — exact, from the closed-form steps
// ---------------------------------------------------------------------------

/// Everything on screen the engine can change by itself.
type Visible = (
    status::StatusLine,
    DisplayAnim,
    bool,
    Phase,
    Option<Action>,
    u8,
    u8,
    u8,
);

/// What is written under the pet.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PetLine {
    Status(status::StatusLine),
    Note(demo::Note),
    Help,
}

impl PetLine {
    /// Whether the line is drawn in red: the help line, or a status line in
    /// danger.
    pub fn red(&self) -> bool {
        match self {
            PetLine::Help => true,
            PetLine::Status(l) => l.danger,
            PetLine::Note(_) => false,
        }
    }
}

impl GameState {
    /// Tell the engine the local time `local_min` (0..1440) at `now_tick`.
    /// Stores the minute at `last_update_tick`, where the next update starts.
    /// `None` (wall clock not set) puts the pet on its own rhythm.
    pub fn sync_clock(&mut self, now_tick: u32, local_min: Option<u16>) {
        let day = demo::MINUTES_PER_DAY as u32;
        let ago = now_tick.saturating_sub(self.last_update_tick) / TICKS_PER_MINUTE % day;
        self.day_min = local_min.map(|m| ((m as u32 % day + day - ago) % day) as u16);
    }

    /// What sits under the pet: the help line when demo and health is low;
    /// else the demo note while the status line still equals the one it was
    /// taken on; else the status line. With demo off it is always the status
    /// line, whatever note a pet action left behind.
    pub fn pet_line(&self) -> PetLine {
        let line = status::status_line(self);
        if demo::needs_help(self) {
            return PetLine::Help;
        }
        match self.note {
            Some((n, at)) if self.demo.on && at == line => PetLine::Note(n),
            _ => PetLine::Status(line),
        }
    }

    /// Host test hook: what the demo pet last did.
    #[cfg(feature = "simulator")]
    pub fn note(&self) -> Option<(demo::Note, status::StatusLine)> {
        self.note
    }

    /// Host test hook: the demo pet's dice.
    #[cfg(feature = "simulator")]
    pub fn demo_rng(&self) -> u32 {
        self.demo_rng
    }

    /// Host test hook: fix the demo pet's dice (never 0).
    #[cfg(feature = "simulator")]
    pub fn set_demo_rng(&mut self, rng: u32) {
        self.demo_rng = rng.max(1);
    }

    /// Why `a` (with the chosen `drink`) would be turned away right now, or
    /// `None` when its starter accepts it (affordability aside). The menu and
    /// the starters share this, so the UI never re-derives a rule.
    pub fn refusal_for(&self, a: Action, drink: Option<DrinkKind>) -> Option<doses::Reason> {
        if let Some(why) = self.cannot_act() {
            return Some(why);
        }
        let r = doses::refusal(self, a, drink)?;
        // A medical "no need" that is really a wait shows the time left.
        let wait_min = match (a, r) {
            (Action::Heal, doses::Refusal::NoNeed) => self.med_cd_min,
            (Action::Ozempic, doses::Refusal::NoNeed) => self.ozempic_cd_min,
            (Action::Rehab, doses::Refusal::NoNeed) => self.rehab_min,
            (Action::Medicate, doses::Refusal::NoNeed) => {
                self.insulin_min.saturating_sub(DOSE_INSULIN_WINDOW_MIN())
            }
            _ => 0,
        };
        Some(if wait_min > 0 {
            doses::Reason::Wait(wait_min as u32 * 60)
        } else {
            doses::Reason::Refused(r)
        })
    }

    /// Why a mini-game cannot start now.
    pub fn game_refusal(&self, game: MiniGame) -> Option<doses::Reason> {
        if let Some(why) = self.cannot_act() {
            return Some(why);
        }
        if self.is_sleeping {
            return Some(doses::Reason::Refused(doses::Refusal::Asleep));
        }
        let cooldown = self.game_cooldown(game);
        (cooldown > 0).then(|| doses::Reason::Wait(cooldown as u32 * 10))
    }

    /// Why this pet cannot start or accept a fight now, or `None`: the
    /// reasons that stop every action, then asleep, then the body, then the
    /// battle rest. There is no age limit.
    pub fn battle_refusal(&self) -> Option<doses::Reason> {
        if let Some(why) = self.cannot_act() {
            return Some(why);
        }
        if self.is_sleeping {
            return Some(doses::Reason::Refused(doses::Refusal::Asleep));
        }
        if let Some(why) = self.body_refusal() {
            return Some(why);
        }
        (self.cooldown_battle > 0).then(|| doses::Reason::Wait(self.cooldown_battle as u32 * 10))
    }

    /// Why this pet cannot be a fight's target now, or `None`: the reasons
    /// that stop every action, then the body. Asleep and the battle rest do
    /// not count (a sleeper is woken by its player's Fight; the rest only
    /// limits starting one).
    pub fn target_refusal(&self) -> Option<doses::Reason> {
        self.cannot_act().or_else(|| self.body_refusal())
    }

    /// The part of `battle_refusal` that is about the body: injured, too
    /// weak or too drunk (the felt drink, as the fight reads it).
    pub fn body_refusal(&self) -> Option<doses::Reason> {
        use doses::{Reason, Refusal};
        if self.injured_min > 0 {
            Some(Reason::Refused(Refusal::Injured))
        } else if (self.health as i32) < BATTLE_MIN_HEALTH_PCT() as i32 * P {
            Some(Reason::Refused(Refusal::TooWeak))
        } else if combat::felt(&combat::Body::of(self)) > BATTLE_DRUNK_PCT() as i32 {
            Some(Reason::Refused(Refusal::TooDrunk))
        } else {
            None
        }
    }

    /// Ticks left of a mini-game's cooldown.
    pub(crate) fn game_cooldown(&self, game: MiniGame) -> u16 {
        match game {
            MiniGame::TicTacToe => self.cooldown_tictactoe,
            MiniGame::LightsOut => self.cooldown_lightsout,
            MiniGame::BlackHole => self.cooldown_blackhole,
            MiniGame::BornJeweled => self.cooldown_bornjeweled,
        }
    }

    /// Why the pet would turn `r` away now, or `None` when it would take it.
    /// Built only from the primitives the menu rows use: `refusal_for`,
    /// `sleep_refusal`, `game_refusal` and the price (`doses::price`,
    /// `can_afford`, money mode only).
    pub fn remedy_reason(&self, r: hint::Remedy) -> Option<doses::Reason> {
        use hint::Remedy;
        let priced = |a: Action, food: Option<FoodKind>, drink: Option<DrinkKind>| {
            let price = doses::price(a, food, drink, self.hard_mode)?;
            (self.money_live() && !self.can_afford(price)).then_some(doses::Reason::CantAfford)
        };
        let gate = |a: Action, drink: Option<DrinkKind>| {
            self.refusal_for(a, drink)
                .or_else(|| priced(a, None, drink))
        };
        match r {
            Remedy::Feed => {
                let cheapest = FoodKind::ALL
                    .into_iter()
                    .min_by_key(|&f| doses::price(Action::Feed, Some(f), None, self.hard_mode))?;
                self.refusal_for(Action::Feed, None)
                    .or_else(|| priced(Action::Feed, Some(cheapest), None))
            }
            Remedy::Water => gate(Action::Drink, Some(DrinkKind::Water)),
            Remedy::Sleep => self.sleep_refusal(),
            Remedy::Play => gate(Action::Play, None),
            Remedy::Game => {
                let mut first = None;
                for g in MiniGame::ALL {
                    match self.game_refusal(g) {
                        None => return None,
                        why => first = first.or(why),
                    }
                }
                first
            }
            Remedy::Relax => gate(Action::Relax, None),
            Remedy::Wash => gate(Action::Wash, None),
            Remedy::Exercise => gate(Action::Exercise, None),
            Remedy::Medicine => gate(Action::Heal, None),
            Remedy::Insulin => gate(Action::Medicate, None),
            Remedy::Rehab => gate(Action::Rehab, None),
            Remedy::Defrost => (!self.hibernating)
                .then_some(doses::Reason::Refused(doses::Refusal::NoNeed)),
        }
    }

    /// The warning cause that doing `r` now would produce, or `None` when it
    /// goes well or is no dose (a game, a defrost). The same previews the menu
    /// marks its rows with; a Feed is clean when any food is.
    pub fn remedy_warning(&self, r: hint::Remedy) -> Option<causes::Cause> {
        use hint::Remedy;
        match r {
            Remedy::Feed => {
                let mut first = None;
                for f in FoodKind::ALL {
                    match self.preview(Action::Feed, Some(f), None) {
                        None => return None,
                        warned => first = first.or(warned),
                    }
                }
                first
            }
            Remedy::Water => self.preview(Action::Drink, None, Some(DrinkKind::Water)),
            Remedy::Sleep => self.preview_sleep(),
            Remedy::Play => self.preview(Action::Play, None, None),
            Remedy::Relax => self.preview(Action::Relax, None, None),
            Remedy::Wash => self.preview(Action::Wash, None, None),
            Remedy::Exercise => self.preview(Action::Exercise, None, None),
            Remedy::Medicine => self.preview(Action::Heal, None, None),
            Remedy::Insulin => self.preview(Action::Medicate, None, None),
            Remedy::Rehab => self.preview(Action::Rehab, None, None),
            Remedy::Game | Remedy::Defrost => None,
        }
    }

    /// Whether demo mode is on.
    pub fn is_demo(&self) -> bool {
        self.demo.on
    }

    /// HAX is live: the pet has money. Demo mode keeps it live (the demo
    /// pet pays its way and earns, `demo::drift_hax` pulls a surplus down).
    pub fn money_live(&self) -> bool {
        self.money_enabled
    }

    /// A quarter or more of the pet's life was lived in demo mode.
    pub fn self_raised(&self) -> bool {
        let age_min = (self.age_ticks / TICKS_PER_MINUTE) as u64;
        self.demo.demo_min as u64 * SELF_RAISED_DIV as u64 > age_min
    }

    /// Why demo mode cannot be turned on now. A running action is `Busy`,
    /// which does not block it.
    pub fn demo_refusal(&self) -> Option<doses::Reason> {
        match self.cannot_act() {
            None | Some(doses::Reason::Busy) => {}
            Some(why) => return Some(why),
        }
        (self.phase == Phase::Leaving
            || (self.health as i32) < DEMO_ON_HEALTH_PCT() as i32 * needs::P)
            .then_some(doses::Reason::Refused(doses::Refusal::FixMeFirst))
    }

    /// Turn demo mode on or off. On needs a living pet in fair shape; a
    /// running action does not block it. Off always works and keeps only the
    /// lifetime, the care mode and whose action is running.
    pub fn set_demo(&mut self, on: bool) -> Result<(), doses::Reason> {
        // An old note must not come back with the next switch-on.
        self.note = None;
        let keep = demo::Demo {
            demo_min: self.demo.demo_min,
            animated: self.demo.animated,
            pet_action: self.demo.pet_action,
            ..demo::Demo::default()
        };
        if on {
            if let Some(why) = self.demo_refusal() {
                return Err(why);
            }
            self.demo = demo::Demo {
                on: true,
                whim_min: DEMO_WHIM_BASE_MIN(),
                ..keep
            };
        } else {
            self.demo = keep;
        }
        self.request_save();
        Ok(())
    }

    /// Choose how the demo pet looks after itself: instantly with a note
    /// (`false`, the default) or by running the real animated action.
    pub fn set_demo_care(&mut self, animated: bool) {
        self.demo.animated = animated;
        self.request_save();
    }

    /// The tick a running action ends. A player's action ends
    /// `action_ticks_remaining` after the last update. A pet's own action
    /// (`demo.pet_action`) is spent from the engine-time stream, so it spends
    /// the carried remainder first: it ends that much sooner, at least one
    /// tick after the last update.
    pub fn action_end_tick(&self) -> u32 {
        let end = self.last_update_tick + self.action_ticks_remaining as u32;
        if self.demo.pet_action {
            end.saturating_sub(self.tick_rem as u32)
                .max(self.last_update_tick + 1)
        } else {
            end
        }
    }

    /// The reasons that stop every action alike.
    fn cannot_act(&self) -> Option<doses::Reason> {
        if self.hibernating {
            Some(doses::Reason::Hibernating)
        } else if self.phase == Phase::Hatching {
            Some(doses::Reason::Egg)
        } else if !self.is_alive() {
            Some(doses::Reason::Gone)
        } else if self.active_action.is_some() {
            Some(doses::Reason::Busy)
        } else {
            None
        }
    }

    fn visible(&self) -> Visible {
        (
            status::status_line(self),
            self.display_anim(),
            self.is_sleeping,
            self.phase,
            self.active_action,
            self.onset_bits(),
            demo::wake_view(self),
            scene::wake_view(self),
        )
    }

    /// Host test hook: everything a wake-up has to show. Oracles compare
    /// this, so they never restate the view by hand.
    #[cfg(any(test, feature = "simulator"))]
    pub fn wake_view_for_test(&self) -> impl PartialEq + core::fmt::Debug {
        self.visible()
    }

    /// The tick at which the engine must run next: when something the
    /// player can see (the [`status::StatusLine`] with its danger flag, animation, sleep, phase,
    /// action, the background: sky and floor items) first changes, but never later than `MAX_SLEEP_TICKS`.
    ///
    /// The bisection is exact only while these hold, so keep them true:
    /// - inside one `segment` every mood term, level and rate is linear, so
    ///   "the view differs" cannot flip back (two linear terms cross once);
    /// - every line the view reads (the danger lines, the happy-mood line, the
    ///   band edges, the floor lines) is a threshold edge or a `solver::sig`
    ///   line (the sky switch is a `timer_bound` timer), sharing one
    ///   const with `status` and `display_anim` (`needs::*_PCT`), so no
    ///   segment steps over it unseen;
    /// - everything that ages is advanced by `advance` alone (needs do not
    ///   age during an action, hence its own bound).
    ///
    /// The caller sleeps for `next_wake_tick - last_update_tick` ticks. Not
    /// polling: a copy of the pet is stepped by whole solver segments, and
    /// the segment that holds the change is bisected, a handful of
    /// closed-form steps for the whole look-ahead.
    pub fn next_wake_tick(&self) -> u32 {
        if self.phase == Phase::Gone || self.hibernating {
            return u32::MAX;
        }
        if self.phase == Phase::Hatching {
            return self.last_update_tick + self.hatching_countdown as u32;
        }
        let cap = self.last_update_tick.saturating_add(MAX_SLEEP_TICKS());
        // A running action ages nothing; it is its own bound.
        if self.active_action.is_some() {
            return self.action_end_tick().max(self.last_update_tick + 1).min(cap);
        }
        #[cfg(feature = "simulator")]
        let _mute = sim::mute();
        let mut c = self.clone();
        // The bookkeeping `update` does at a boundary comes first, so the
        // look-ahead arms what `update` will arm. A pet with a choice waiting
        // (a player's action or a switch left it) acts in the first update
        // after the wake: `update` runs `demo::run` before it looks at the
        // minutes, so one tick is enough and the wakes do not repeat.
        demo::settle(&mut c);
        if demo::decide(&c).is_some() {
            // A cap hit (`run` gave up with a choice left) waits for the next
            // whole minute; it never wakes tick by tick.
            let wake = if demo::run_capped(&mut c.clone()).1 {
                self.last_update_tick + TICKS_PER_MINUTE - self.tick_rem as u32
            } else {
                self.last_update_tick + 1
            };
            return wake.min(cap);
        }
        let start = c.visible();
        // Look as far ahead as the cap allows, and no further.
        let horizon = MAX_SLEEP_TICKS().div_ceil(TICKS_PER_MINUTE).max(1);
        let (mut used, mut steep) = (0u32, 0u32);
        while used < horizon {
            demo::settle(&mut c);
            if used > 0 && c.visible() != start {
                // The boundary's own bookkeeping changed the view.
                break;
            }
            let dt = c.segment(horizon - used, &mut steep);
            let before = c.clone();
            advance::advance(&mut c, dt);
            if c.visible() != start {
                // Inside a segment every term is linear, so once the view
                // differs it stays different: find the first such minute.
                let (mut lo, mut hi) = (1, dt);
                while lo < hi {
                    let mid = (lo + hi) / 2;
                    let mut t = before.clone();
                    advance::advance(&mut t, mid);
                    if t.visible() != start {
                        hi = mid;
                    } else {
                        lo = mid + 1;
                    }
                }
                used += lo;
                break;
            }
            used += dt;
        }
        (self.last_update_tick + used * TICKS_PER_MINUTE - self.tick_rem as u32).min(cap)
    }
}

// ---------------------------------------------------------------------------
// PetStats — display-friendly snapshot (0 = bad, 100 = good)
// ---------------------------------------------------------------------------

/// Display-friendly snapshot of the pet's state.
///
/// All stats are whole percent of the internal u16 level; for the needs
/// 100 = best (`booze` and `weight_pct` report the raw level).
///
/// Obtain via [`GameState::stats()`] which triggers a state update first.
#[derive(Clone, Copy, Debug)]
#[cfg_attr(feature = "embassy-core", derive(defmt::Format))]
pub struct PetStats {
    /// How well-fed the pet is (100 = full, 0 = starving).
    pub hunger: u8,
    /// How rested the pet is (100 = alert, 0 = exhausted).
    pub tired: u8,
    /// How healthy the pet is (100 = healthy, 0 = critically ill).
    pub healthy: u8,
    /// How happy the pet is (100 = happy, 0 = miserable).
    pub happy: u8,

    /// Whether the pet has developed type 2 diabetes (permanent once set).
    pub diabetic: bool,
    /// Whether the pet has developed alcoholism (permanent once set).
    pub alcoholic: bool,

    /// Current lifecycle phase.
    pub phase: Phase,
    /// Whether the pet is sleeping.
    pub is_sleeping: bool,
    /// Age in ticks (1 tick = 10 seconds).
    pub age_ticks: u32,
    /// Generation number (0 = first pet).
    pub generation: u16,

    /// Lifetime mesh Battle wins/losses — see `crate::game::battle`.
    pub wins: u16,
    pub losses: u16,

    /// Current HAX balance. Only meaningful (displayed/priced) when
    /// `money_enabled`.
    pub money: u32,
    /// Whether HAX is live for this pet: the money layer is on, in demo too
    /// (`GameState::money_live`). Gates HAX display,
    /// pricing, and menu affordability checks.
    pub money_enabled: bool,
    /// Whether Hard (US) prices are in effect. Only meaningful when
    /// `money_enabled` is true — see `GameState::hard_mode`.
    pub hard_mode: bool,
    /// Demo mode is on.
    pub demo: bool,
    /// A quarter or more of the pet's life was lived in demo.
    pub self_raised: bool,

    /// Currently active action (if any).
    pub active_action: Option<Action>,
    /// Ticks remaining on the active action.
    pub action_ticks_remaining: u8,

    /// Action availability (true = can be started right now).
    pub can_feed: bool,
    pub can_heal: bool,
    pub can_play: bool,
    /// "Only pets" — same in-progress-action/cooldown gating as Play.
    /// Only ever surfaced in the menu when `money_enabled`, but this flag
    /// itself doesn't know about money — the menu gates visibility.
    pub can_only_pets: bool,
    pub can_exercise: bool,
    /// Only true once `diabetic` is set — administering medication to a
    /// non-diabetic pet is a no-op.
    pub can_medicate: bool,
    /// Unlike `can_medicate`, not gated on diabetic status.
    pub can_ozempic: bool,
    pub can_drink: bool,
    /// Only true once `alcoholic` is set — mirrors `can_medicate`.
    pub can_rehab: bool,
    pub can_relax: bool,
    pub can_wash: bool,
    pub can_sleep: bool,
    pub can_wake: bool,
    /// Per-mini-game availability.  False while that game's post-win
    /// cooldown is running; the others stay independently playable.
    pub can_play_tictactoe: bool,
    pub can_play_lightsout: bool,
    pub can_play_blackhole: bool,
    pub can_play_bornjeweled: bool,
    /// Whether a mesh Battle can be started right now (cooldown-gated,
    /// same shape as the mini-game `can_play_*` flags above). Does not
    /// factor in whether any friends exist — the Battle screen handles
    /// the "no friends yet" empty state itself.
    pub can_battle: bool,

    /// Remaining mini-game and Battle cooldowns in ticks (1 tick = 10 s).
    /// 0 = ready. Mirrored from the matching `GameState` fields so the modal
    /// can show the exact remaining time on a disabled menu row.
    pub cooldown_tictactoe: u16,
    pub cooldown_lightsout: u16,
    pub cooldown_blackhole: u16,
    pub cooldown_bornjeweled: u16,
    pub cooldown_battle: u16,

    /// Whether the pet is hibernating (all progression frozen).
    pub hibernating: bool,
    /// Total hours spent in hibernation during this pet's life.
    pub hibernate_hours: u32,

    /// Hydration, fun and cleanliness (100 = best) and the raw booze, body
    /// weight (50 = ideal) and fitness levels, in percent.
    pub water: u8,
    pub fun: u8,
    pub clean: u8,
    pub booze: u8,
    pub weight_pct: u8,
    pub fit: u8,
    /// The status line under the pet (one source for the words and the
    /// danger flag), see [`status::status_line`].
    pub line: status::StatusLine,
    /// The lasting conditions the stats view lists, see [`status::conditions`].
    pub conditions: status::Conditions,
    /// The highest health the pet can recover to, in percent.
    pub ceiling: u8,
    /// Minutes slept in the current sleep.
    pub asleep_min: u16,
    /// Remaining minutes of the timers behind the menu's disabled rows:
    /// Heal's cooldown, Ozempic's cooldown, the insulin window, rehab and
    /// the injury.
    pub med_cd_min: u16,
    pub ozempic_cd_min: u16,
    pub insulin_min: u16,
    pub rehab_min: u16,
    pub injured_min: u16,
    /// A water drink would be accepted (it is refused when not thirsty,
    /// unlike every other drink).
    pub can_drink_water: bool,
}

/// A stored pet name (`buf`, `len` bytes used) as a str. `len` may come from
/// flash and be corrupt, so it is clamped to the buffer; invalid UTF-8 gives
/// an empty name.
pub fn name_str(buf: &[u8], len: u8) -> &str {
    let n = (len as usize).min(PET_NAME_MAX).min(buf.len());
    core::str::from_utf8(&buf[..n]).unwrap_or("")
}

/// Ticks (1 tick = 10 s) as "Xd Xh".
pub fn fmt_days_hours(ticks: u32) -> heapless::String<12> {
    let hours = ticks / 360;
    let days = hours / 24;
    let mut s = heapless::String::new();
    let _ = core::fmt::Write::write_fmt(&mut s, format_args!("{}d {}h", days, hours % 24));
    s
}

/// A trait value (0..=65535, higher = stronger) as a 0..=100 percentage.
pub fn trait_pct(v: u16) -> u8 {
    (v as u32 * 100 / 65535) as u8
}

/// A level as whole percent (the same lines the band edges use).
fn level_pct(v: u16) -> u8 {
    needs::pct(v as i32).min(100) as u8
}

/// How a fight ended for the pet that fought it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fought {
    pub won: bool,
    /// The pet's own HP left, in percent; sets the bruises.
    pub hp_left_pct: u8,
    /// The costs of a fight land (energy, strain, mood, bruises).
    pub costs: bool,
    /// A win can pay the prize.
    pub paid: bool,
}

impl GameState {
    /// The pet as the fight sees it (plan 5): six numbers and the condition flags.
    pub fn snapshot(&self) -> combat::Snapshot {
        combat::derive(&combat::Body::of(self))
    }

    /// Update the game state to `now_tick` and return a display snapshot.
    ///
    /// This is the primary API for the UI layer: it triggers a state update
    /// (free when the tick has not moved), then builds the snapshot. A menu
    /// row is enabled exactly when its starter would accept the action
    /// (`doses::refusal`), affordability aside.
    pub fn stats(&mut self, now_tick: u32) -> PetStats {
        // Only update if time has advanced since last call.
        if now_tick != self.last_update_tick {
            self.update(now_tick);
        }

        let can = |a: Action| self.refusal_for(a, None).is_none();
        let can_game = |g: MiniGame| self.game_refusal(g).is_none();

        PetStats {
            hunger: level_pct(self.fullness),
            tired: level_pct(self.energy),
            healthy: level_pct(self.health),
            happy: level_pct(self.mood),

            water: level_pct(self.hydration),
            fun: level_pct(self.fun),
            clean: 100 - level_pct(self.dirt),
            booze: level_pct(self.booze),
            weight_pct: level_pct(self.weight),
            fit: level_pct(self.fitness),
            line: status::status_line(self),
            conditions: status::conditions(self),
            ceiling: level_pct(regime::health_ceiling(self).clamp(0, u16::MAX as i32) as u16),
            asleep_min: self.sleep_min,
            med_cd_min: self.med_cd_min,
            ozempic_cd_min: self.ozempic_cd_min,
            insulin_min: self.insulin_min,
            rehab_min: self.rehab_min,
            injured_min: self.injured_min,

            diabetic: self.diabetic,
            alcoholic: self.dependent,

            phase: self.phase,
            is_sleeping: self.is_sleeping,
            age_ticks: self.age_ticks,
            generation: self.generation,

            wins: self.wins,
            losses: self.losses,

            money: self.money,
            money_enabled: self.money_live(),
            hard_mode: self.hard_mode,
            demo: self.demo.on,
            self_raised: self.self_raised(),

            active_action: self.active_action,
            action_ticks_remaining: self.action_ticks_remaining,

            can_feed: can(Action::Feed),
            can_heal: can(Action::Heal),
            can_play: can(Action::Play),
            can_only_pets: can(Action::OnlyPets),
            can_exercise: can(Action::Exercise),
            can_medicate: can(Action::Medicate),
            can_ozempic: can(Action::Ozempic),
            can_drink: can(Action::Drink),
            can_drink_water: self.refusal_for(Action::Drink, Some(DrinkKind::Water)).is_none(),
            can_rehab: can(Action::Rehab),
            can_relax: can(Action::Relax),
            can_wash: can(Action::Wash),
            can_sleep: self.sleep_refusal().is_none(),
            can_wake: self.is_sleeping,
            can_play_tictactoe: can_game(MiniGame::TicTacToe),
            can_play_lightsout: can_game(MiniGame::LightsOut),
            can_play_blackhole: can_game(MiniGame::BlackHole),
            can_play_bornjeweled: can_game(MiniGame::BornJeweled),
            can_battle: self.battle_refusal().is_none(),

            cooldown_tictactoe: self.cooldown_tictactoe,
            cooldown_lightsout: self.cooldown_lightsout,
            cooldown_blackhole: self.cooldown_blackhole,
            cooldown_bornjeweled: self.cooldown_bornjeweled,
            cooldown_battle: self.cooldown_battle,

            hibernating: self.hibernating,
            hibernate_hours: self.hibernate_hours(),
        }
    }
}

// ---------------------------------------------------------------------------
// Persistence helpers
// ---------------------------------------------------------------------------

impl GameState {
    /// Returns `true` if the state should be saved to flash.
    ///
    /// Becomes true when at least `SAVE_INTERVAL_TICKS()` (15 minutes)
    /// have elapsed since the last save.  The caller does the async
    /// save and then calls `mark_saved()`.
    ///
    /// No extra wake-ups are scheduled for saving — this check
    /// piggybacks on whatever triggered the current update.
    pub fn needs_save(&self) -> bool {
        self.save_pending
            || self.age_ticks.saturating_sub(self.last_save_tick) >= SAVE_INTERVAL_TICKS()
    }

    /// Mark the state as successfully saved.  Resets the save timer.
    pub fn mark_saved(&mut self) {
        self.last_save_tick = self.age_ticks;
        self.save_pending = false;
    }

    /// Request that the next `save_if_needed()` persists this state
    /// immediately, rather than waiting for the next 15-minute interval.
    pub fn request_save(&mut self) {
        self.save_pending = true;
    }

    /// Old station phrase `more food` (design 19.3, `fix s=food full>60
    /// water>60`): fullness and hydration are raised to 60 % and never
    /// lowered, so the pet is fed but cannot be stuffed or waterlogged.
    pub fn station_food(&mut self) {
        let floor = (STATION_FOOD_FLOOR_PCT * P) as u16;
        self.fullness = self.fullness.max(floor);
        self.hydration = self.hydration.max(floor);
    }

    /// Old station phrase `more drugs` (19.3, `fix s=drugs health>100
    /// booze<20 clear=hangover,injury,groggy`): health up to the pet's own
    /// ceiling, booze down to 20 % at most, three ailments cleared.
    /// Dependence and diabetes stay.
    pub fn station_drugs(&mut self) {
        self.health = self.health.max(regime::health_ceiling(self) as u16);
        self.booze = self.booze.min((STATION_BOOZE_CAP_PCT * P) as u16);
        self.hangover_min = 0;
        self.injured_min = 0;
        self.groggy_min = 0;
    }

    /// Old station phrase `sleep like a bear` (19.3, `fix s=rest energy>100
    /// clear=groggy,deprived`): energy full, groggy and deprived cleared.
    /// It does not put the pet to sleep. 19.4: it also suppresses the groggy
    /// check of the next sleep.
    pub fn station_rest(&mut self) {
        self.energy = MAX as u16;
        self.groggy_min = 0;
        self.deprived = false;
        self.no_groggy = true;
    }

    /// Meeting (or spending time with) a mesh friend over the SHDW channel —
    /// see `crate::game::friends`. `big` is the bigger one-time bump for a
    /// brand-new friend; the smaller one applies to a cooldown-gated
    /// recurring reunion with someone already known. Both add fun (doubled
    /// while tired). A passive mesh event: it does not wake a sleeping pet.
    pub fn friend_boost(&mut self, big: bool) {
        doses::friend(self, big);
    }

    /// A fight was declined: the mood target drops for `BATTLE_DECLINE_MIN`
    /// minutes (spec 17 row 16).
    pub fn declined(&mut self) {
        self.decline_min = BATTLE_DECLINE_MIN();
        self.request_save();
    }

    /// A fight this pet took part in is over (spec 12.2, 12.5, 18.8). With
    /// `costs` the costs land (`doses::battle`); every call starts the rest
    /// (only fighters reach it), records the result in the league the pet is
    /// in, and pays the prize when `paid` and won: `BATTLE_PRIZE_FIRST_HAX`
    /// for the first paid win until the next local midnight, then
    /// `BATTLE_PRIZE_LATER_HAX`. Returns the HAX paid (0 when none).
    pub fn fight_over(&mut self, f: Fought) -> u32 {
        if f.costs {
            doses::battle(self, f.won, f.hp_left_pct);
        }
        self.cooldown_battle = BATTLE_COOLDOWN();
        self.record_only(f.won);
        let mut prize = 0;
        if f.paid && f.won && self.money_live() {
            if self.prize_min == 0 {
                prize = BATTLE_PRIZE_FIRST_HAX() as u32;
                let day = demo::MINUTES_PER_DAY;
                self.prize_min = self.day_min.map_or(day, |m| day - m.min(day));
            } else {
                prize = BATTLE_PRIZE_LATER_HAX() as u32;
            }
            self.add_money(prize);
        }
        prize
    }

    /// A result for a fight this pet did not take part in (it heard a
    /// fallback RESULT): the record of the current league and nothing else,
    /// no cost, no mood, no rest, no HAX.
    pub fn record_only(&mut self, won: bool) {
        let (wins, losses) = if self.demo.on {
            (&mut self.demo_wins, &mut self.demo_losses)
        } else {
            (&mut self.wins, &mut self.losses)
        };
        let n = if won { wins } else { losses };
        *n = n.saturating_add(1);
        self.request_save();
    }

    /// `(wins, losses)` of the league the pet is in now: the demo record in
    /// demo mode, else the player's.
    pub fn battle_record(&self) -> (u16, u16) {
        if self.demo.on {
            (self.demo_wins, self.demo_losses)
        } else {
            (self.wins, self.losses)
        }
    }

    /// Credit `delta` HAX, saturating at `u32::MAX`. Immediately flags the
    /// state for save (see `request_save`) rather than waiting for the
    /// next 15-minute save interval — same immediate-persist pattern as
    /// pet name / hibernate, since a HAX balance is worth protecting from
    /// an unlucky reboot right after being earned.
    pub fn add_money(&mut self, delta: u32) {
        self.money = self.money.saturating_add(delta);
        self.request_save();
    }

    /// Debit `price` HAX if affordable. Returns `true` and flags the state
    /// for immediate save on success; returns `false` and leaves `money`
    /// unchanged (no save) if the balance is insufficient. Callers are
    /// expected to have already gated the action on `can_afford()` so this
    /// should normally succeed, but the check is repeated here so the
    /// balance can never go negative.
    pub fn spend_money(&mut self, price: u32) -> bool {
        if self.money >= price {
            self.money -= price;
            self.request_save();
            true
        } else {
            false
        }
    }

    /// Whether the current balance covers `price`.
    pub fn can_afford(&self, price: u32) -> bool {
        self.money >= price
    }
}

// ---------------------------------------------------------------------------
// Serialization — manual, no serde, fixed-size
// ---------------------------------------------------------------------------

/// Format version stamped into byte 0 of every save. Bump it whenever the
/// layout below changes so a blob from another firmware fails the guard in
/// `from_bytes` instead of being reinterpreted with the wrong offsets.
/// Format 2 was the BornPets v2 layout (97 bytes, kv key `state`); format 3
/// is the BornPets v3 layout (kv key `state_v3`).
pub const SAVE_FORMAT_VERSION: u8 = 3;

/// Serialized size of a format-3 GameState in bytes.
/// Demo minutes times this must exceed the age for `self_raised`
/// (a quarter of the life).
const SELF_RAISED_DIV: u32 = 4;

pub const SAVE_SIZE: usize = 177;

/// Where `caffeine_min` sits: the reserved tail right after the demo block.
/// An older save has zeros there, which reads as no caffeine, so the format
/// stays 3 (like the demo block and the battle slots before it).
const CAFFEINE_AT: usize = demo::BLOCK_AT + demo::BLOCK_LEN;

/// Size of a format-2 save, read only by the upgrade path.
pub const SAVE_SIZE_V2: usize = 97;

/// kv key (namespace "game") of the format-3 save.
/// The pet name sits beside it under `NAME_KEY` (`name_v3`); the old `name`
/// key is only read as a fallback (`pick_name`), never written.
pub const SAVE_KEY: &str = "state_v3";

/// kv key of the format-2 save. Read for the upgrade, never written or
/// deleted, so the previous firmware still finds its pet.
pub const SAVE_KEY_V2: &str = "state";

/// Version byte of a format-2 save.
const SAVE_FORMAT_VERSION_V2: u8 = 2;

/// Size of the head format 3 shares with format 2.
const HEAD_SIZE: usize = SAVE_SIZE_V2;

/// Byte value for "none" in the optional-enum slots.
const NONE_BYTE: u8 = 0xFF;

// How a format-2 save is read. These describe the old save, not game
// balance, so they are not `Thresholds` fields.
/// Weight of an upgraded pet: this much plus half the old weight (percent).
const MIGRATE_WEIGHT_BASE_PCT: i32 = 50;
/// Lower and upper clamp of an upgraded fullness (percent).
const MIGRATE_FULLNESS_MIN_PCT: i32 = 25;
const MIGRATE_FULLNESS_MAX_PCT: i32 = 78;
/// Levels a format-2 save never had, given to an upgraded pet (percent).
const MIGRATE_FITNESS_PCT: i32 = 60;
const MIGRATE_HYDRATION_PCT: i32 = 60;
const MIGRATE_FUN_PCT: i32 = 50;
const MIGRATE_DIRT_PCT: i32 = 20;
/// Cap of an upgraded `insulin_min` (minutes).
const MIGRATE_INSULIN_CAP_MIN: u32 = 480;
/// Ticks per engine minute, for the format-2 tick counters.
const TICKS_PER_MIN: u32 = 6;

/// Sequential little-endian reader over a save buffer. The caller checked
/// the length, so reads stay in range.
struct Reader<'a> {
    b: &'a [u8],
    i: usize,
}

impl<'a> Reader<'a> {
    fn u8(&mut self) -> u8 {
        let v = self.b[self.i];
        self.i += 1;
        v
    }
    fn u16(&mut self) -> u16 {
        let v = u16::from_le_bytes([self.b[self.i], self.b[self.i + 1]]);
        self.i += 2;
        v
    }
    fn u32(&mut self) -> u32 {
        let v = u32::from_le_bytes([
            self.b[self.i],
            self.b[self.i + 1],
            self.b[self.i + 2],
            self.b[self.i + 3],
        ]);
        self.i += 4;
        v
    }
    fn skip(&mut self, n: usize) {
        self.i += n;
    }
}

/// Sequential little-endian writer into a fixed buffer.
struct Writer<'a> {
    b: &'a mut [u8],
    i: usize,
}

impl Writer<'_> {
    fn u8(&mut self, v: u8) {
        self.b[self.i] = v;
        self.i += 1;
    }
    fn u16(&mut self, v: u16) {
        self.b[self.i..self.i + 2].copy_from_slice(&v.to_le_bytes());
        self.i += 2;
    }
    fn u32(&mut self, v: u32) {
        self.b[self.i..self.i + 4].copy_from_slice(&v.to_le_bytes());
        self.i += 4;
    }
}

fn phase_from_u8(v: u8) -> Phase {
    match v {
        0 => Phase::Hatching,
        1 => Phase::Active,
        2 => Phase::Leaving,
        _ => Phase::Gone,
    }
}

fn food_from_u8(v: u8) -> Option<FoodKind> {
    FoodKind::ALL.get(v as usize).copied()
}

fn food_to_u8(f: FoodKind) -> u8 {
    FoodKind::ALL.iter().position(|&k| k == f).unwrap_or(0) as u8
}

fn drink_from_u8(v: u8) -> Option<DrinkKind> {
    DrinkKind::ALL.get(v as usize).copied()
}

fn drink_to_u8(d: DrinkKind) -> u8 {
    DrinkKind::ALL.iter().position(|&k| k == d).unwrap_or(0) as u8
}

impl GameState {
    /// Serialize the game state (format 3) to a fixed-size buffer for ekv.
    ///
    /// Bytes 0..97 keep the format-2 offsets so the head reads the same
    /// way; the `cooldown_*` slots that no longer exist are written as 0,
    /// and three slots are reused (`cooldown_heal` = `med_cd_min`,
    /// `cooldown_medicate` = `insulin_min`, `cooldown_ozempic` =
    /// `ozempic_cd_min`). Bytes 97..177 hold the format-3 additions.
    pub fn to_bytes(&self) -> [u8; SAVE_SIZE] {
        let mut b = [0u8; SAVE_SIZE];
        let mut w = Writer { b: &mut b, i: 0 };
        w.u8(SAVE_FORMAT_VERSION);

        // Needs (needs are good-high; the old hunger/tired/sick/miserable/
        // drunk slots).
        w.u16(self.fullness);
        w.u16(self.energy);
        w.u16(self.health);
        w.u16(self.mood);
        w.u16(self.weight);
        w.u16(self.booze);
        // Traits.
        w.u16(self.vitality);
        w.u16(self.curiosity);
        w.u16(self.resilience);
        // Timing.
        w.u32(self.last_update_tick);
        w.u32(self.age_ticks);
        // Lifecycle.
        w.u8(self.phase as u8);
        w.u16(self.hatching_countdown);
        w.u32(self.leaving_min);
        w.u16(self.generation);
        // Action state.
        w.u8(self.active_action.map_or(NONE_BYTE, Action::to_u8));
        w.u8(self.action_ticks_remaining);
        w.u16(self.demo_wins); // old cooldown_feed
        w.u16(self.med_cd_min); // cooldown_heal
        w.u16(self.demo_losses); // old cooldown_relax
        w.u16(self.prize_min); // old cooldown_play
        w.u16(self.decline_min); // old cooldown_exercise
        w.u16(self.insulin_min); // cooldown_medicate
        w.u16(self.ozempic_cd_min); // cooldown_ozempic
        w.u16(0); // cooldown_drink (removed)
        w.u16(0); // cooldown_rehab (removed)
        // miserable_interval_counter, tired_passive_counter (removed).
        w.u32(0);
        w.u32(0);
        // Slow counters, in quarter-minutes (`advance::SLOW_SCALE`).
        w.u32(self.diabetes_min); // overweight_ticks
        w.u32(self.dependence_min); // drunk_ticks
        // Flags.
        w.u8(self.is_sleeping as u8);
        w.u8(self.hibernating as u8);
        w.u8(self.diabetic as u8);
        w.u8(self.dependent as u8);
        w.u32(self.hibernate_ticks);
        w.u32(self.last_save_tick);
        w.u8(self.pet_kind.0);
        w.u16(self.wins);
        w.u16(self.losses);
        w.u16(self.cooldown_battle);
        w.u32(self.money);
        w.u8(self.money_enabled as u8);
        w.u8(self.hard_mode as u8);
        debug_assert_eq!(w.i, HEAD_SIZE);

        // Format-3 additions.
        w.u16(self.hydration);
        w.u16(self.fun);
        w.u16(self.strain);
        w.u16(self.dirt);
        w.u16(self.fitness);
        w.u16(self.sleep_min);
        w.u16(self.oversleep_min);
        w.u16(self.good_sleep_min);
        w.u16(self.rested_min);
        w.u16(self.injured_min);
        w.u16(self.groggy_min);
        w.u16(self.hangover_min);
        w.u16(self.rehab_min);
        w.u16(self.recovering_min);
        w.u16(self.junk_min);
        w.u16(self.ozempic_min);
        w.u16(self.reversal_min);
        w.u8(self.booze_peak as u8
            | (self.deprived as u8) << 1
            | (self.night as u8) << 2
            | (self.no_groggy as u8) << 3);
        w.u8(self.short_nights);
        w.u8(self.good_nights);
        w.u8(self.tick_rem);
        w.u32(self.care_good_min);
        w.u32(self.care_awake_min);
        // The running action's food and drink (R23), then the wall-clock time of the
        // save (0 = unknown); the rest is reserved.
        w.u8(self.active_food.map_or(NONE_BYTE, food_to_u8));
        w.u8(self.active_drink.map_or(NONE_BYTE, drink_to_u8));
        w.u32(self.saved_unix);
        debug_assert_eq!(w.i, demo::BLOCK_AT);
        let mut block = [0u8; demo::BLOCK_LEN];
        self.demo.write(&mut block);
        b[demo::BLOCK_AT..demo::BLOCK_AT + demo::BLOCK_LEN].copy_from_slice(&block);
        b[CAFFEINE_AT..CAFFEINE_AT + 2].copy_from_slice(&self.caffeine_min.to_le_bytes());
        b
    }

    /// Deserialize a format-3 save. Any other length or version byte is
    /// `None` (the caller falls back to the format-2 key, or a fresh egg).
    pub fn from_bytes(b: &[u8]) -> Option<Self> {
        if b.len() != SAVE_SIZE || b[0] != SAVE_FORMAT_VERSION {
            return None;
        }
        let mut r = Reader { b, i: 1 };
        let h = Head::read(&mut r);
        let mut s = h.into_state();
        // The slots format 3 reuses (see `to_bytes`).
        s.fullness = h.levels[0];
        s.energy = h.levels[1];
        s.health = h.levels[2];
        s.mood = h.levels[3];
        s.booze = h.booze;
        s.leaving_min = h.leaving;
        // The battle slots (format 3 only): a plan-3 save has zeros here. A
        // damaged value is held to its limit, never a day-long penalty.
        s.demo_wins = h.cooldowns[0];
        s.demo_losses = h.cooldowns[2];
        s.prize_min = h.cooldowns[3].min(demo::MINUTES_PER_DAY);
        s.decline_min = h.cooldowns[4].min(BATTLE_DECLINE_MIN());
        s.med_cd_min = h.cooldowns[1];
        s.insulin_min = h.cooldowns[5];
        s.ozempic_cd_min = h.cooldowns[6];
        s.diabetes_min = h.overweight_ticks;
        s.dependence_min = h.drunk_ticks;
        s.dependent = h.alcoholic;
        s.active_action = Action::from_u8(h.action);
        s.action_ticks_remaining = h.action_ticks;

        s.hydration = r.u16();
        s.fun = r.u16();
        s.strain = r.u16();
        s.dirt = r.u16();
        s.fitness = r.u16();
        s.sleep_min = r.u16();
        s.oversleep_min = r.u16();
        s.good_sleep_min = r.u16();
        s.rested_min = r.u16();
        s.injured_min = r.u16();
        s.groggy_min = r.u16();
        s.hangover_min = r.u16();
        s.rehab_min = r.u16();
        s.recovering_min = r.u16();
        s.junk_min = r.u16();
        s.ozempic_min = r.u16();
        s.reversal_min = r.u16();
        let flags = r.u8();
        s.booze_peak = flags & 1 != 0;
        s.deprived = flags & 2 != 0;
        s.night = flags & 4 != 0;
        s.no_groggy = flags & 8 != 0;
        s.short_nights = r.u8();
        s.good_nights = r.u8();
        s.tick_rem = r.u8();
        s.care_good_min = r.u32();
        s.care_awake_min = r.u32();
        s.active_food = food_from_u8(r.u8());
        s.active_drink = drink_from_u8(r.u8());
        s.saved_unix = r.u32();
        let block: &[u8; demo::BLOCK_LEN] = b[demo::BLOCK_AT..demo::BLOCK_AT + demo::BLOCK_LEN]
            .try_into()
            .expect("length checked above");
        s.demo = demo::Demo::read(block);
        s.caffeine_min = u16::from_le_bytes([b[CAFFEINE_AT], b[CAFFEINE_AT + 1]]);
        Some(s)
    }

    /// Read a format-2 save and make a living format-3 pet of it. The old
    /// 0-high stats are mirrored (`MAX - x`); levels format 2 never had get
    /// the `MIGRATE_*` defaults; a running action is dropped.
    pub fn from_bytes_v2(b: &[u8; SAVE_SIZE_V2]) -> Option<Self> {
        if b[0] != SAVE_FORMAT_VERSION_V2 {
            return None;
        }
        let h = Head::read(&mut Reader { b: &b[..], i: 1 });
        let mut s = h.into_state();
        let pct = |p: i32| (p * P) as u16;
        let flip = |v: u16| (MAX - v as i32) as u16;
        s.fullness = (MAX - h.levels[0] as i32)
            .clamp(MIGRATE_FULLNESS_MIN_PCT * P, MIGRATE_FULLNESS_MAX_PCT * P) as u16;
        s.energy = flip(h.levels[1]);
        s.health = flip(h.levels[2]);
        s.mood = flip(h.levels[3]);
        s.booze = h.booze;
        s.weight = (MIGRATE_WEIGHT_BASE_PCT * P + h.weight as i32 / 2) as u16;
        s.fitness = pct(MIGRATE_FITNESS_PCT);
        s.hydration = pct(MIGRATE_HYDRATION_PCT);
        s.fun = pct(MIGRATE_FUN_PCT);
        s.dirt = pct(MIGRATE_DIRT_PCT);
        s.strain = 0;
        // A leaving pet gets its full countdown again.
        s.leaving_min = 0;
        // cooldown_medicate counted ticks; cap at one dose of insulin.
        s.insulin_min = (h.cooldowns[5] as u32 / TICKS_PER_MIN).min(MIGRATE_INSULIN_CAP_MIN) as u16;
        // The slow counters count quarter-minutes now and stay one
        // quarter-minute under onset, so the upgrade never turns a
        // condition on by itself (R17).
        s.dependence_min = (h.drunk_ticks / TICKS_PER_MIN * SLOW_SCALE)
            .min(DEPENDENCE_ONSET_MIN() as u32 * SLOW_SCALE - 1);
        s.diabetes_min = (h.overweight_ticks / TICKS_PER_MIN * SLOW_SCALE)
            .min(DIABETES_ONSET_MIN() as u32 * SLOW_SCALE - 1);
        s.dependent = h.alcoholic;
        s.night = h.is_sleeping;
        Some(s)
    }
}

/// The 97-byte head formats 2 and 3 share, with the format-2 meanings of
/// the slots. Each format maps them to `GameState` fields itself.
#[derive(Clone, Copy)]
struct Head {
    /// hunger, tired, sick, miserable.
    levels: [u16; 4],
    weight: u16,
    booze: u16,
    vitality: u16,
    curiosity: u16,
    resilience: u16,
    last_update_tick: u32,
    age_ticks: u32,
    phase: u8,
    hatching_countdown: u16,
    leaving: u32,
    generation: u16,
    action: u8,
    action_ticks: u8,
    /// feed, heal, relax, play, exercise, medicate, ozempic, drink, rehab.
    cooldowns: [u16; 9],
    overweight_ticks: u32,
    drunk_ticks: u32,
    is_sleeping: bool,
    hibernating: bool,
    diabetic: bool,
    alcoholic: bool,
    hibernate_ticks: u32,
    last_save_tick: u32,
    pet_kind: u8,
    wins: u16,
    losses: u16,
    cooldown_battle: u16,
    money: u32,
    money_enabled: bool,
    hard_mode: bool,
}

impl Head {
    fn read(r: &mut Reader) -> Self {
        let levels = [r.u16(), r.u16(), r.u16(), r.u16()];
        let weight = r.u16();
        let booze = r.u16();
        let vitality = r.u16();
        let curiosity = r.u16();
        let resilience = r.u16();
        let last_update_tick = r.u32();
        let age_ticks = r.u32();
        let phase = r.u8();
        let hatching_countdown = r.u16();
        let leaving = r.u32();
        let generation = r.u16();
        let action = r.u8();
        let action_ticks = r.u8();
        let mut cooldowns = [0u16; 9];
        for c in &mut cooldowns {
            *c = r.u16();
        }
        r.skip(8); // miserable_interval_counter, tired_passive_counter
        let overweight_ticks = r.u32();
        let drunk_ticks = r.u32();
        let is_sleeping = r.u8() != 0;
        let hibernating = r.u8() != 0;
        let diabetic = r.u8() != 0;
        let alcoholic = r.u8() != 0;
        let hibernate_ticks = r.u32();
        let last_save_tick = r.u32();
        let pet_kind = r.u8();
        let wins = r.u16();
        let losses = r.u16();
        let cooldown_battle = r.u16();
        let money = r.u32();
        let money_enabled = r.u8() != 0;
        let hard_mode = r.u8() != 0;
        debug_assert_eq!(r.i, HEAD_SIZE);
        Self {
            levels,
            weight,
            booze,
            vitality,
            curiosity,
            resilience,
            last_update_tick,
            age_ticks,
            phase,
            hatching_countdown,
            leaving,
            generation,
            action,
            action_ticks,
            cooldowns,
            overweight_ticks,
            drunk_ticks,
            is_sleeping,
            hibernating,
            diabetic,
            alcoholic,
            hibernate_ticks,
            last_save_tick,
            pet_kind,
            wins,
            losses,
            cooldown_battle,
            money,
            money_enabled,
            hard_mode,
        }
    }

    /// The fields both formats read the same way, on top of a fresh egg
    /// (so every field `Head` does not carry starts at its default). The
    /// loaded state is not flagged for saving, and `last_update_tick`
    /// comes from the save as before.
    fn into_state(self) -> GameState {
        let mut s = GameState::new_egg(0, PetKind::from_u8(self.pet_kind));
        s.save_pending = false;
        s.weight = self.weight;
        s.vitality = self.vitality;
        s.curiosity = self.curiosity;
        s.resilience = self.resilience;
        s.last_update_tick = self.last_update_tick;
        s.age_ticks = self.age_ticks;
        s.phase = phase_from_u8(self.phase);
        s.hatching_countdown = self.hatching_countdown;
        s.generation = self.generation;
        // Both load paths seed the dice here, from the age and generation.
        s.demo_rng = demo::seed_rng(self.age_ticks as u64, self.generation as u64);
        s.is_sleeping = self.is_sleeping;
        s.hibernating = self.hibernating;
        s.diabetic = self.diabetic;
        s.hibernate_ticks = self.hibernate_ticks;
        s.last_save_tick = self.last_save_tick;
        s.wins = self.wins;
        s.losses = self.losses;
        s.cooldown_battle = self.cooldown_battle;
        s.money = self.money;
        s.money_enabled = self.money_enabled;
        s.hard_mode = self.hard_mode;
        s
    }
}

/// kv key (namespace "game") of the pet's name under v3, beside `state_v3`.
pub const NAME_KEY: &str = "name_v3";

/// kv key of the old firmware's pet name. Read as a fallback when
/// `name_v3` is missing (an upgraded pet keeps its name), never written or
/// deleted, so a player who reverts still sees the old pet under its own
/// name. (The Unicorn Realm key `realm` stays shared: its format is the
/// same.)
pub const NAME_KEY_V2: &str = "name";

/// Choose the pet name from the raw kv values: `v3` is the value of
/// `name_v3`, `old` the value of `name`. A present `name_v3` wins, even an
/// empty one (an unnamed new pet must not inherit the old pet's name).
pub fn pick_name<'a>(v3: Option<&'a [u8]>, old: Option<&'a [u8]>) -> Option<&'a [u8]> {
    v3.or(old)
}

/// Choose the state to load from the raw kv values: `v3` is the value of
/// key `state_v3`, `v2` the value of the old key `state`. A format-3 save
/// wins; a corrupt or missing one falls through to the format-2 upgrade.
/// The flag is true when the state came from the upgrade, which is then
/// flagged for an immediate save (to `state_v3`; `state` is never touched).
pub fn pick_save(v3: Option<&[u8]>, v2: Option<&[u8]>) -> Option<(GameState, bool)> {
    if let Some(s) = v3.and_then(GameState::from_bytes) {
        return Some((s, false));
    }
    let old: &[u8; SAVE_SIZE_V2] = v2?.try_into().ok()?;
    let mut s = GameState::from_bytes_v2(old)?;
    s.request_save();
    Some((s, true))
}

// ---------------------------------------------------------------------------
// Unicorn Realm — past pet records
// ---------------------------------------------------------------------------

/// Maximum length of a pet name in bytes.
pub const PET_NAME_MAX: usize = 12;

/// Compact record of a past pet for the Unicorn Realm.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PetRecord {
    pub generation: u16,
    pub age_ticks: u32,
    pub vitality: u16,
    pub curiosity: u16,
    pub resilience: u16,
    pub pet_kind: PetKind,
    /// Pet name (UTF-8, up to PET_NAME_MAX bytes, zero-padded).
    pub name: [u8; PET_NAME_MAX],
    pub name_len: u8,
}

/// Serialized size of one PetRecord (12 data + 1 kind + 12 name + 1 name_len).
pub const PET_RECORD_SIZE: usize = 26;
/// Maximum number of past pets stored.
pub const REALM_MAX_PETS: usize = 10;
/// Total serialized size: 1 byte count + N records.
pub const REALM_SAVE_SIZE: usize = 1 + REALM_MAX_PETS * PET_RECORD_SIZE;

impl PetRecord {
    /// Create a record from the current game state and name (call when pet
    /// leaves).
    pub fn from_game_state(state: &GameState, pet_name: &[u8]) -> Self {
        let mut name = [0u8; PET_NAME_MAX];
        let len = pet_name.len().min(PET_NAME_MAX);
        name[..len].copy_from_slice(&pet_name[..len]);
        Self {
            generation: state.generation,
            age_ticks: state.age_ticks,
            vitality: state.vitality,
            curiosity: state.curiosity,
            resilience: state.resilience,
            pet_kind: state.pet_kind,
            name,
            name_len: len as u8,
        }
    }

    fn to_bytes(self, buf: &mut [u8]) {
        buf[0..2].copy_from_slice(&self.generation.to_le_bytes());
        buf[2..6].copy_from_slice(&self.age_ticks.to_le_bytes());
        buf[6..8].copy_from_slice(&self.vitality.to_le_bytes());
        buf[8..10].copy_from_slice(&self.curiosity.to_le_bytes());
        buf[10..12].copy_from_slice(&self.resilience.to_le_bytes());
        buf[12] = self.pet_kind.0;
        buf[13..25].copy_from_slice(&self.name);
        buf[25] = self.name_len;
    }

    fn from_bytes(buf: &[u8]) -> Self {
        let mut name = [0u8; PET_NAME_MAX];
        let pet_kind = if buf.len() >= 26 {
            name.copy_from_slice(&buf[13..25]);
            PetKind::from_u8(buf[12])
        } else if buf.len() >= 25 {
            // Old 25-byte format without pet_kind.
            name.copy_from_slice(&buf[12..24]);
            PetKind::DEFAULT
        } else {
            PetKind::DEFAULT
        };
        Self {
            generation: u16::from_le_bytes([buf[0], buf[1]]),
            age_ticks: u32::from_le_bytes([buf[2], buf[3], buf[4], buf[5]]),
            vitality: u16::from_le_bytes([buf[6], buf[7]]),
            curiosity: u16::from_le_bytes([buf[8], buf[9]]),
            resilience: u16::from_le_bytes([buf[10], buf[11]]),
            pet_kind,
            name,
            name_len: if buf.len() >= 26 {
                buf[25]
            } else if buf.len() >= 25 {
                buf[24]
            } else {
                0
            },
        }
    }

    /// Pet name as a str.
    pub fn name_str(&self) -> &str {
        // name_len comes from flash and could be corrupt; `name_str` clamps it
        // so the realm screen can't panic on a bad record.
        name_str(&self.name, self.name_len)
    }

    /// Format age as "Xd Xh".
    pub fn age_str(&self) -> heapless::String<12> {
        fmt_days_hours(self.age_ticks)
    }
}

/// Ring buffer of past pet records, newest first.
pub struct PetRealm {
    pub pets: [PetRecord; REALM_MAX_PETS],
    pub count: u8,
}

impl Default for PetRealm {
    fn default() -> Self {
        Self::new()
    }
}

impl PetRealm {
    pub const fn new() -> Self {
        Self {
            pets: [PetRecord {
                generation: 0,
                age_ticks: 0,
                vitality: 0,
                curiosity: 0,
                resilience: 0,
                pet_kind: PetKind::DEFAULT,
                name: [0; PET_NAME_MAX],
                name_len: 0,
            }; REALM_MAX_PETS],
            count: 0,
        }
    }

    /// Add a pet record, shifting older entries down. Drops the oldest if full.
    pub fn push(&mut self, record: PetRecord) {
        // Shift everything down by one.
        for i in (1..REALM_MAX_PETS).rev() {
            self.pets[i] = self.pets[i - 1];
        }
        self.pets[0] = record;
        if (self.count as usize) < REALM_MAX_PETS {
            self.count += 1;
        }
    }

    pub fn to_bytes(&self) -> [u8; REALM_SAVE_SIZE] {
        let mut buf = [0u8; REALM_SAVE_SIZE];
        buf[0] = self.count;
        for i in 0..self.count as usize {
            let offset = 1 + i * PET_RECORD_SIZE;
            self.pets[i].to_bytes(&mut buf[offset..offset + PET_RECORD_SIZE]);
        }
        buf
    }

    pub fn from_bytes(buf: &[u8]) -> Self {
        let mut realm = Self::new();
        if buf.is_empty() {
            return realm;
        }
        realm.count = buf[0].min(REALM_MAX_PETS as u8);
        for i in 0..realm.count as usize {
            let offset = 1 + i * PET_RECORD_SIZE;
            if offset + PET_RECORD_SIZE <= buf.len() {
                realm.pets[i] = PetRecord::from_bytes(&buf[offset..]);
            }
        }
        realm
    }
}

#[cfg(test)]
mod overweight_diabetes_tests {
    use super::*;

    /// A fresh egg starts with the Stage-1 HAX default: 100 balance, money
    /// mode enabled.
    #[test]
    fn new_egg_starts_with_100_hax_and_money_enabled() {
        let state = GameState::new_egg(1, PetKind::DEFAULT);
        assert_eq!(state.money, 100);
        assert!(state.money_enabled);
    }

    /// `add_money` saturates at `u32::MAX` instead of overflowing/panicking,
    /// and every credit flags the state for an immediate save (so the new
    /// balance survives a reboot right after being earned rather than
    /// waiting for the next 15-minute save interval).
    #[test]
    fn add_money_saturates_and_flags_save() {
        let mut state = GameState::new_egg(2, PetKind::DEFAULT);
        state.money = u32::MAX - 5;
        state.add_money(100);
        assert_eq!(state.money, u32::MAX);

        state.mark_saved();
        assert!(!state.needs_save());
        state.add_money(1);
        assert!(state.needs_save(), "add_money should request an immediate save");
    }

    /// Spending succeeds and debits the balance when the pet can afford it.
    #[test]
    fn spend_money_succeeds_when_affordable() {
        let mut state = GameState::new_egg(3, PetKind::DEFAULT);
        state.money = 50;
        assert!(state.spend_money(30));
        assert_eq!(state.money, 20);
    }

    /// Spending fails and leaves the balance unchanged when the pet can't
    /// afford it — money should never go negative (or wrap, in u32 terms).
    #[test]
    fn spend_money_fails_when_too_poor() {
        let mut state = GameState::new_egg(4, PetKind::DEFAULT);
        state.money = 10;
        assert!(!state.spend_money(30));
        assert_eq!(state.money, 10);
    }

    /// `can_afford` reports the exact threshold: affordable at the price,
    /// not affordable one HAX above it.
    #[test]
    fn can_afford_reports_threshold() {
        let mut state = GameState::new_egg(5, PetKind::DEFAULT);
        state.money = 20;
        assert!(state.can_afford(20));
        assert!(!state.can_afford(21));
    }

    /// `fight_over` bumps the right counter and always sets the rest,
    /// regardless of win/loss.
    #[test]
    fn fight_over_updates_counters_and_cooldown() {
        let fought = |won| Fought { won, hp_left_pct: 50, costs: true, paid: false };
        let mut state = GameState::new_egg(1, PetKind::DEFAULT);
        assert_eq!(state.wins, 0);
        assert_eq!(state.losses, 0);

        state.fight_over(fought(true));
        assert_eq!(state.wins, 1);
        assert_eq!(state.losses, 0);
        assert_eq!(state.cooldown_battle, BATTLE_COOLDOWN());

        state.cooldown_battle = 0;
        state.fight_over(fought(false));
        assert_eq!(state.wins, 1);
        assert_eq!(state.losses, 1);
        assert_eq!(state.cooldown_battle, BATTLE_COOLDOWN());
    }

    /// Meeting a brand new mesh friend lifts fun more than a reunion with a
    /// known one, and never overflows the level range.
    #[test]
    fn friend_boost_scales_with_big() {
        let mut state = GameState::new_egg(17, PetKind::DEFAULT);
        state.fun = 0;
        state.friend_boost(true);
        let after_big = state.fun;
        assert!(after_big > 0, "a big friend boost should raise fun");

        state.fun = 0;
        state.friend_boost(false);
        assert!(
            after_big > state.fun,
            "a new friend (big={after_big}) should lift fun more than a known one (small={})",
            state.fun
        );

        state.fun = u16::MAX;
        state.friend_boost(true);
        assert_eq!(state.fun, u16::MAX);
    }

    // ── Stage 2: "user can make money" ──────────────────────────────────

    /// Winning a mini-game credits `MINIGAME_HAX_REWARD` HAX when money
    /// mode is on, and grants nothing (but still runs the existing
    /// cooldown/hunger-cost path) when money mode is off.
    #[test]
    fn minigame_win_awards_hax_when_money_enabled() {
        let mut state = GameState::new_egg(30, PetKind::DEFAULT);
        state.update(HATCHING_TICKS() as u32);
        state.money = 0;
        state.money_enabled = true;

        state.award_inspiration(MiniGame::BlackHole, GRADE_WIN);
        assert_eq!(state.money, MINIGAME_HAX_REWARD);

        state.money = 0;
        state.money_enabled = false;
        state.cooldown_blackhole = 0;
        state.award_inspiration(MiniGame::BlackHole, GRADE_WIN);
        assert_eq!(state.money, 0, "money off should grant no HAX");
    }

    /// Every finished mini-game pays by result and starts its cooldown;
    /// money off pays nothing; demo on pays like any other pet.
    #[test]
    fn minigame_pays_by_result() {
        for (grade, hax) in [
            (GRADE_LOSS, MINIGAME_HAX_LOSS),
            (GRADE_DRAW, MINIGAME_HAX_DRAW),
            (GRADE_WIN, MINIGAME_HAX_REWARD),
        ] {
            let mut state = GameState::new_egg(30, PetKind::DEFAULT);
            state.update(HATCHING_TICKS() as u32);
            state.money = 0;
            state.money_enabled = true;
            state.cooldown_tictactoe = 0;
            state.award_inspiration(MiniGame::TicTacToe, grade);
            assert_eq!(state.money, hax);
            assert!(state.cooldown_tictactoe > 0, "grade {grade} starts the cooldown");
        }
        assert_eq!(
            (MINIGAME_HAX_LOSS, MINIGAME_HAX_DRAW, MINIGAME_HAX_REWARD),
            (5, 10, 15)
        );

        let mut state = GameState::new_egg(30, PetKind::DEFAULT);
        state.update(HATCHING_TICKS() as u32);
        state.money = 0;
        state.money_enabled = false;
        state.award_inspiration(MiniGame::BlackHole, GRADE_LOSS);
        assert_eq!(state.money, 0, "money off pays nothing");
        assert!(state.cooldown_blackhole > 0);

        state.money_enabled = true;
        state.demo.on = true;
        state.cooldown_blackhole = 0;
        state.award_inspiration(MiniGame::BlackHole, GRADE_LOSS);
        assert_eq!(state.money, MINIGAME_HAX_LOSS, "demo pays like any pet");
    }

    /// A paid mesh Battle win credits HAX; a loss grants nothing. With
    /// money mode off, even a win grants nothing.
    #[test]
    fn battle_win_awards_hax_winner_only() {
        let fought = |won| Fought { won, hp_left_pct: 50, costs: true, paid: true };
        let mut state = GameState::new_egg(31, PetKind::DEFAULT);
        state.money_enabled = true;
        state.money = 0;

        assert_eq!(state.fight_over(fought(true)), BATTLE_PRIZE_FIRST_HAX() as u32);
        assert_eq!(state.money, BATTLE_PRIZE_FIRST_HAX() as u32);

        state.money = 0;
        state.fight_over(fought(false));
        assert_eq!(state.money, 0, "a loss should not award HAX");

        state.money = 0;
        state.money_enabled = false;
        state.fight_over(fought(true));
        assert_eq!(state.money, 0, "money off should grant no HAX even on a win");
    }

    /// `only_pets()` starts the action like `play()`; a second call while
    /// the action is still in progress is rejected.
    #[test]
    fn only_pets_starts_action() {
        let mut state = GameState::new_egg(32, PetKind::DEFAULT);
        state.update(HATCHING_TICKS() as u32);

        assert!(state.only_pets());
        assert_eq!(state.active_action, Some(Action::OnlyPets));
        assert!(!state.only_pets(), "action already in progress");
    }

    // ── Stage 3: "things cost money" ────────────────────────────────────

    /// Feeding charges by the food's health tier — healthy (Salad) costs
    /// more than unhealthy (Burger), mirroring `doses::price`.
    #[test]
    fn feed_charges_by_food_health() {
        let mut state = GameState::new_egg(40, PetKind::DEFAULT);
        state.update(HATCHING_TICKS() as u32);
        state.money_enabled = true;
        state.money = 100;

        assert!(state.feed(FoodKind::Salad));
        assert_eq!(state.money, 85, "healthy food (Salad) should cost 15 HAX");

        // Fresh egg so the cooldown from the first feed doesn't reject this one.
        let mut state = GameState::new_egg(41, PetKind::DEFAULT);
        state.update(HATCHING_TICKS() as u32);
        state.money_enabled = true;
        state.money = 100;

        assert!(state.feed(FoodKind::Frikandel));
        assert_eq!(state.money, 90, "unhealthy food (Frikandel) should cost 10 HAX");
    }

    /// Drinking charges by the drink's health tier — healthy (Water) costs
    /// more than unhealthy (Coffee), mirroring `doses::price`.
    #[test]
    fn drink_charges_by_health() {
        let mut state = GameState::new_egg(42, PetKind::DEFAULT);
        state.update(HATCHING_TICKS() as u32);
        state.money_enabled = true;
        state.money = 100;

        assert!(state.drink(DrinkKind::Water));
        assert_eq!(state.money, 85, "healthy drink (Water) should cost 15 HAX");

        let mut state = GameState::new_egg(43, PetKind::DEFAULT);
        state.update(HATCHING_TICKS() as u32);
        state.money_enabled = true;
        state.money = 100;

        assert!(state.drink(DrinkKind::Coffee));
        assert_eq!(state.money, 90, "unhealthy drink (Coffee) should cost 10 HAX");
    }

    /// The basic Play action costs a flat `PLAY_HAX_COST` (10 HAX).
    #[test]
    fn play_costs_10() {
        let mut state = GameState::new_egg(44, PetKind::DEFAULT);
        state.update(HATCHING_TICKS() as u32);
        state.money_enabled = true;
        state.money = 100;

        assert!(state.play());
        assert_eq!(state.money, 90);
    }

    /// Aspirine (the Heal action) costs a flat `ASPIRINE_HAX_COST` (1 HAX),
    /// and is rejected when the pet can't afford it.
    #[test]
    fn aspirine_costs_1_hax() {
        let mut state = GameState::new_egg(44, PetKind::DEFAULT);
        state.update(HATCHING_TICKS() as u32);
        state.money_enabled = true;
        state.money = 100;
        assert!(state.heal());
        assert_eq!(state.money, 99);

        // Broke (0 HAX): heal is rejected, no charge, no action started.
        let mut broke = GameState::new_egg(45, PetKind::DEFAULT);
        broke.update(HATCHING_TICKS() as u32);
        broke.money_enabled = true;
        broke.money = 0;
        assert!(!broke.heal());
        assert_eq!(broke.money, 0);
        assert_eq!(broke.active_action, None);
    }

    /// Each drug action (Ozempic, Medicate, Rehab) costs a flat
    /// `DRUG_HAX_COST` (15 HAX). Medicate/Rehab are gated on
    /// diabetic/alcoholic respectively, so those flags are set first.
    #[test]
    fn drug_costs_15() {
        let mut state = GameState::new_egg(45, PetKind::DEFAULT);
        state.update(HATCHING_TICKS() as u32);
        state.money_enabled = true;
        state.money = 100;
        assert!(state.ozempic());
        assert_eq!(state.money, 85, "ozempic should cost 15 HAX");

        let mut state = GameState::new_egg(46, PetKind::DEFAULT);
        state.update(HATCHING_TICKS() as u32);
        state.money_enabled = true;
        state.money = 100;
        state.diabetic = true;
        assert!(state.medicate());
        assert_eq!(state.money, 85, "medicate should cost 15 HAX");

        let mut state = GameState::new_egg(47, PetKind::DEFAULT);
        state.update(HATCHING_TICKS() as u32);
        state.money_enabled = true;
        state.money = 100;
        state.dependent = true;
        assert!(state.rehab());
        assert_eq!(state.money, 85, "rehab should cost 15 HAX");
    }

    /// With `money_enabled = false`, priced actions proceed but charge
    /// nothing at all — the whole money layer stays inert.
    #[test]
    fn actions_free_when_money_disabled() {
        let mut state = GameState::new_egg(48, PetKind::DEFAULT);
        state.update(HATCHING_TICKS() as u32);
        state.money_enabled = false;
        state.money = 100;

        assert!(state.feed(FoodKind::Salad));
        assert_eq!(state.money, 100, "feed should not charge when money is off");

        let mut state = GameState::new_egg(49, PetKind::DEFAULT);
        state.update(HATCHING_TICKS() as u32);
        state.money_enabled = false;
        state.money = 100;

        assert!(state.play());
        assert_eq!(state.money, 100, "play should not charge when money is off");
    }

    /// Stage 5 reverses the Stage-3 best-effort contract: an unaffordable
    /// action is now REJECTED outright rather than proceeding for free —
    /// `feed()` returns false, the balance is untouched, and no action starts.
    #[test]
    fn broke_action_is_now_rejected() {
        let mut state = GameState::new_egg(50, PetKind::DEFAULT);
        state.update(HATCHING_TICKS() as u32);
        state.money_enabled = true;
        state.money = 5;

        assert!(!state.feed(FoodKind::Salad), "unaffordable action should be rejected");
        assert_eq!(state.money, 5, "a rejected charge should not touch the balance");
        assert_eq!(state.active_action, None);
    }

    /// A rejected action (already busy) must never charge — the charge
    /// line sits after the guard checks, so a second `feed()` call while
    /// one is in progress returns false and leaves `money` unchanged.
    #[test]
    fn rejected_action_does_not_charge() {
        let mut state = GameState::new_egg(51, PetKind::DEFAULT);
        state.update(HATCHING_TICKS() as u32);
        state.money_enabled = true;
        state.money = 100;

        assert!(state.feed(FoodKind::Salad));
        state.money = 100; // reset after the first (accepted) charge

        assert!(!state.feed(FoodKind::Frikandel), "feed should be rejected while busy");
        assert_eq!(state.money, 100, "a rejected action must not charge");
    }

    // ── Stage 5: "broke" branch + affordability gating ─────────────────

    /// An unaffordable priced action (Salad costs 15, balance is 5) is
    /// rejected: `feed()` returns false, the balance is untouched, and no
    /// action starts.
    #[test]
    fn unaffordable_action_is_rejected() {
        let mut state = GameState::new_egg(60, PetKind::DEFAULT);
        state.update(HATCHING_TICKS() as u32);
        state.money_enabled = true;
        state.money = 5;

        assert!(!state.feed(FoodKind::Salad));
        assert_eq!(state.money, 5);
        assert_eq!(state.active_action, None);
    }

    /// An affordable priced action charges its price and starts normally.
    #[test]
    fn affordable_action_charges_and_starts() {
        let mut state = GameState::new_egg(61, PetKind::DEFAULT);
        state.update(HATCHING_TICKS() as u32);
        state.money_enabled = true;
        state.money = 100;

        assert!(state.feed(FoodKind::Salad));
        assert_eq!(state.money, 85);
        assert_eq!(state.active_action, Some(Action::Feed));
    }

    /// With `money_enabled = false`, the affordability gate never applies —
    /// the action proceeds free even at 0 balance.
    #[test]
    fn money_disabled_ignores_affordability() {
        let mut state = GameState::new_egg(62, PetKind::DEFAULT);
        state.update(HATCHING_TICKS() as u32);
        state.money_enabled = false;
        state.money = 0;

        assert!(state.feed(FoodKind::Salad));
        assert_eq!(state.money, 0);
    }

    /// Only-pets is the escape hatch when broke and must never be gated on
    /// affordability — it starts even at `money == 0`.
    #[test]
    fn only_pets_never_gated_when_broke() {
        let mut state = GameState::new_egg(63, PetKind::DEFAULT);
        state.update(HATCHING_TICKS() as u32);
        state.money_enabled = true;
        state.money = 0;

        assert!(state.only_pets());
    }

    // ── Hard (US) mode: pricier healthy food / medication / rehab ──────

    /// In Hard mode, healthy food (Salad) costs 20 HAX instead of the
    /// normal 15. Normal (non-hard) mode is unaffected — regression check.
    #[test]
    fn hard_mode_healthy_food_costs_20() {
        let mut state = GameState::new_egg(70, PetKind::DEFAULT);
        state.update(HATCHING_TICKS() as u32);
        state.money_enabled = true;
        state.hard_mode = true;
        state.money = 100;

        assert!(state.feed(FoodKind::Salad));
        assert_eq!(state.money, 80, "hard-mode healthy food should cost 20 HAX");

        let mut normal = GameState::new_egg(71, PetKind::DEFAULT);
        normal.update(HATCHING_TICKS() as u32);
        normal.money_enabled = true;
        normal.hard_mode = false;
        normal.money = 100;

        assert!(normal.feed(FoodKind::Salad));
        assert_eq!(normal.money, 85, "normal-mode healthy food should still cost 15 HAX");
    }

    /// In Hard mode, medication (Ozempic / Insulin) costs 45 HAX — 3x the
    /// normal 15. Normal mode is unaffected.
    #[test]
    fn hard_mode_medication_costs_45() {
        let mut state = GameState::new_egg(72, PetKind::DEFAULT);
        state.update(HATCHING_TICKS() as u32);
        state.money_enabled = true;
        state.hard_mode = true;
        state.money = 100;

        assert!(state.ozempic());
        assert_eq!(state.money, 55, "hard-mode ozempic should cost 45 HAX");

        let mut normal = GameState::new_egg(73, PetKind::DEFAULT);
        normal.update(HATCHING_TICKS() as u32);
        normal.money_enabled = true;
        normal.hard_mode = false;
        normal.money = 100;

        assert!(normal.ozempic());
        assert_eq!(normal.money, 85, "normal-mode ozempic should still cost 15 HAX");
    }

    /// In Hard mode, Rehab costs 75 HAX — 5x the normal 15. Normal mode
    /// is unaffected.
    #[test]
    fn hard_mode_rehab_costs_75() {
        let mut state = GameState::new_egg(74, PetKind::DEFAULT);
        state.update(HATCHING_TICKS() as u32);
        state.money_enabled = true;
        state.hard_mode = true;
        state.dependent = true;
        state.money = 100;

        assert!(state.rehab());
        assert_eq!(state.money, 25, "hard-mode rehab should cost 75 HAX");

        let mut normal = GameState::new_egg(75, PetKind::DEFAULT);
        normal.update(HATCHING_TICKS() as u32);
        normal.money_enabled = true;
        normal.hard_mode = false;
        normal.dependent = true;
        normal.money = 100;

        assert!(normal.rehab());
        assert_eq!(normal.money, 85, "normal-mode rehab should still cost 15 HAX");
    }

    /// Hard mode only raises healthy food / medication / rehab — unhealthy
    /// food and the flat Play cost are untouched.
    #[test]
    fn hard_mode_unhealthy_food_and_play_unchanged() {
        let mut state = GameState::new_egg(76, PetKind::DEFAULT);
        state.update(HATCHING_TICKS() as u32);
        state.money_enabled = true;
        state.hard_mode = true;
        state.money = 100;

        assert!(state.feed(FoodKind::Frikandel));
        assert_eq!(state.money, 90, "hard mode should not touch unhealthy food price");

        let mut state = GameState::new_egg(77, PetKind::DEFAULT);
        state.update(HATCHING_TICKS() as u32);
        state.money_enabled = true;
        state.hard_mode = true;
        state.money = 100;

        assert!(state.play());
        assert_eq!(state.money, 90, "hard mode should not touch the flat Play price");
    }
}
