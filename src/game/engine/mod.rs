//! Cyber Ægg game engine — delta-T progression with boundary-based wake
//! scheduling.
//!
//! Instead of ticking every 10 seconds, the engine:
//! 1. Computes elapsed ticks since the last update.
//! 2. Applies all stat changes for that delta in one step.
//! 3. Computes the next boundary crossing time across all stats.
//! 4. Schedules a wake-up at the earliest boundary (or MAX_SLEEP_TICKS()).
//!
//! This lets the CPU sleep for minutes or hours when nothing interesting
//! is about to happen, saving significant battery on the badge.

pub mod anim_files;
pub mod drink;
pub mod food;
pub mod thresholds;
pub mod to_display;

use thresholds::*;
pub use drink::DrinkKind;
pub use food::FoodKind;
pub use to_display::DisplayAnim;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Kind of pet — determines which sprite set to use.
///
/// New variants can be added here for future pets; the filename
/// generation in `anim_files` and the selection screen will pick
/// them up automatically.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "embassy-base", derive(defmt::Format))]
///
/// Represented as a single byte — the sprite-prefix (`PP` in `PPAAFF.PCX`).
/// The three built-ins are consts; extra pets can be installed at runtime
/// from a `PETS.CFG` manifest on the USB partition (see
/// [`crate::game::pet_registry`]) with no firmware reflash.  Persisted as
/// this byte, so both existing and custom-pet saves round-trip.
#[repr(transparent)]
pub struct PetKind(pub u8);

#[allow(non_upper_case_globals)]
impl PetKind {
    /// Pet kind 0 — formerly named `Snail`, renamed to Bartholomeus when the
    /// snail artwork was reworked.  Persisted byte value remains 0 so existing
    /// saves still load.
    pub const Bartholomeus: PetKind = PetKind(0);
    pub const Cat: PetKind = PetKind(1);
    pub const Slug: PetKind = PetKind(2);

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
    /// Resolves through the runtime registry (built-ins + `PETS.CFG` pets).
    pub fn name(self) -> &'static str {
        crate::game::pet_registry::name_of(self.0)
    }

    /// All selectable pet kinds, in order — built-ins plus any installed via
    /// `PETS.CFG`.  Falls back to the three built-ins before install.
    pub fn roster() -> &'static [PetKind] {
        crate::game::pet_registry::roster()
    }
}

/// Lifecycle phase of the pet.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "embassy-base", derive(defmt::Format))]
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

/// HEX earned by the "Only pets" hobby (non-broke) branch.
pub const ONLYPETS_HOBBY_REWARD: u32 = 20;
/// HEX earned for winning a mini-game.
pub const MINIGAME_HEX_REWARD: u32 = 15;
/// HEX earned for winning a mesh Battle.
pub const BATTLE_HEX_REWARD: u32 = 20;
/// Happiness change per Play / Only-pets completion = 30% of STAT_MAX (65535).
pub const HAPPINESS_STEP: u16 = 19660;
/// HEX cost of the basic Play action.
pub const PLAY_HEX_COST: u32 = 10;
/// HEX cost of a drug dose (Ozempic / Medicate / Rehab).
pub const DRUG_HEX_COST: u32 = 15;
/// HEX cost of an Aspirine (the Heal action).
pub const ASPIRINE_HEX_COST: u32 = 1;
/// Hard-mode multiplier on medication (Insulin / Ozempic).
pub const MEDICATION_HARD_MULT: u32 = 3;
/// Hard-mode multiplier on Rehab.
pub const REHAB_HARD_MULT: u32 = 5;

/// HEX cost of a medication dose (Insulin / Ozempic) in the current mode.
pub fn medication_price(hard: bool) -> u32 {
    DRUG_HEX_COST * if hard { MEDICATION_HARD_MULT } else { 1 }
}

/// HEX cost of Rehab in the current mode.
pub fn rehab_price(hard: bool) -> u32 {
    DRUG_HEX_COST * if hard { REHAB_HARD_MULT } else { 1 }
}
/// Below this balance the pet is "broke" — Only-pets forces the low-pay,
/// happiness-draining work branch instead of the hobby branch.
pub const BROKE_THRESHOLD: u32 = 20;
/// HEX from the Only-pets BROKE (forced-work) branch.
pub const ONLYPETS_BROKE_REWARD: u32 = 100;

/// Active user action (mutually exclusive).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "embassy-base", derive(defmt::Format))]
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
    /// "Only pets" work/hobby action — earns HEX. Only reachable when
    /// `money_enabled`. See `GameState::only_pets`.
    OnlyPets,
}

impl Action {
    /// Persisted discriminant. Explicit (not a bare `as u8` cast) and
    /// paired 1:1 with `from_u8` right below so adding a new variant is
    /// a visible two-place edit — `to_bytes`/`from_bytes` previously
    /// used an `as u8` cast on the write side but a hand-written match
    /// on the read side that only covered the first 4 variants,
    /// silently discarding an in-progress Exercise/Medicate/Ozempic/
    /// Drink/Rehab action on every reboot that landed mid-action.
    ///
    /// `2` is a deliberate gap — it used to be `Relax`, removed along
    /// with the `drained` stat it existed to relieve. Left unassigned
    /// (rather than renumbering Play onward) so old persisted bytes for
    /// the other actions keep meaning across the removal.
    fn to_u8(self) -> u8 {
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
        }
    }

    fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Action::Feed),
            1 => Some(Action::Heal),
            2 => None, // formerly Relax — see `to_u8`.
            3 => Some(Action::Play),
            4 => Some(Action::Exercise),
            5 => Some(Action::Medicate),
            6 => Some(Action::Ozempic),
            7 => Some(Action::Drink),
            8 => Some(Action::Rehab),
            9 => Some(Action::OnlyPets),
            _ => None,
        }
    }
}

/// Mini-games each track their own post-win cooldown so winning one
/// doesn't lock the player out of the others — nudges variety.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "embassy-base", derive(defmt::Format))]
pub enum MiniGame {
    TicTacToe,
    LightsOut,
    BlackHole,
    Nim,
    BornJeweled,
}

/// The complete game state.  Serialisable to ekv for save/restore.
#[derive(Clone)]
pub struct GameState {
    // Pet kind.
    pub pet_kind: PetKind,

    // Primary stats (0 = best, STAT_MAX() = worst).
    pub hunger: u16,
    pub tired: u16,
    pub sick: u16,
    pub miserable: u16,
    pub weight: u16,
    pub drunk: u16,

    // Diabetes — permanent once triggered by sustained overweight.
    pub diabetic: bool,
    /// Ticks accumulated while `weight` has stayed above
    /// `OVERWEIGHT_TRIGGER()`.  Resets to 0 whenever weight drops back
    /// below the trigger.  Once this reaches `DIABETES_ONSET_TICKS()`,
    /// `diabetic` flips true and stays true for the rest of this pet's life.
    overweight_ticks: u32,

    // Alcoholism — permanent once triggered by sustained drunkenness.
    // Same pattern as diabetes/overweight above, just on the `drunk` stat.
    pub alcoholic: bool,
    drunk_ticks: u32,

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
    pub leaving_countdown: u32,
    pub generation: u16,

    // Mesh Battle record — lifetime counters, never reset by anything
    // other than a fresh egg (see `new_egg`/`new_generation`).
    pub wins: u16,
    pub losses: u16,

    // HEX currency — belongs to the current pet, resets with a new egg.
    /// HEX balance. New egg starts at 100.
    pub money: u32,
    /// Chosen at pet creation, persisted. When `false` the whole money
    /// layer is inert: no HEX display, no prices, no rewards.
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
    /// Transient (not persisted) — only meaningful mid-action; defaults
    /// to `FoodKind::Apple`-equivalent multipliers if `None` (e.g. a
    /// reboot mid-feed for the remaining tick or two of that action).
    pub active_food: Option<FoodKind>,
    /// Which drink is being drunk during an in-progress `Action::Drink`.
    /// Same transient/not-persisted treatment as `active_food`.
    pub active_drink: Option<DrinkKind>,
    pub action_ticks_remaining: u8,
    pub cooldown_feed: u16,
    pub cooldown_heal: u16,
    /// No longer settable by anything (the `Relax` action it gated was
    /// removed along with the `drained` stat it existed to relieve) —
    /// kept purely so the persisted save layout doesn't shift. Always
    /// 0 for any pet created after this change.
    pub cooldown_relax: u16,
    pub cooldown_play: u16,
    pub cooldown_exercise: u16,
    /// Doubles as the medication "protection window": while this is
    /// above 0 the diabetes sick-penalty is suppressed, same counter
    /// gates the "Give medication" menu item's cooldown.
    pub cooldown_medicate: u16,
    /// Ozempic cooldown — separate from `cooldown_medicate` since
    /// Ozempic isn't gated on being diabetic and doesn't affect the
    /// sick-penalty suppression.
    pub cooldown_ozempic: u16,
    pub cooldown_drink: u16,
    /// Doubles as the alcoholism-treatment protection window, mirrors
    /// `cooldown_medicate`.
    pub cooldown_rehab: u16,
    /// Cooldown between mesh Battles — resolves instantly (no in-progress
    /// duration), so this is the only battle-related field that behaves
    /// like the other primary-action cooldowns above.
    pub cooldown_battle: u16,
    /// Per-mini-game cooldown after winning.  Each game tracks its
    /// own counter so winning one doesn't gate the others.  None of
    /// these are persisted to flash — rebooting clears them.
    pub cooldown_tictactoe: u16,
    pub cooldown_lightsout: u16,
    pub cooldown_blackhole: u16,
    pub cooldown_nim: u16,
    pub cooldown_bornjeweled: u16,
    /// Cooldown after completing "Only pets". Transient (not persisted),
    /// same policy as the mini-game cooldowns above — rebooting mid-cooldown
    /// just makes the action available again a little early.
    pub cooldown_onlypets: u16,

    // Interval counters (track ticks since last interval fire).
    miserable_interval_counter: u32,
    tired_passive_counter: u32,

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

            hunger: 0,
            tired: 0,
            sick: (STAT_MAX() - vitality) / 4,
            miserable: 0,
            weight: 0,
            drunk: 0,

            diabetic: false,
            overweight_ticks: 0,

            alcoholic: false,
            drunk_ticks: 0,

            vitality,
            curiosity,
            resilience,

            last_update_tick: 0,
            age_ticks: 0,

            phase: Phase::Hatching,
            hatching_countdown: HATCHING_TICKS(),
            leaving_countdown: 0,
            generation: 0,

            wins: 0,
            losses: 0,

            money: 100,
            money_enabled: true,
            hard_mode: false,

            active_action: None,
            active_food: None,
            active_drink: None,
            action_ticks_remaining: 0,
            cooldown_feed: 0,
            cooldown_heal: 0,
            cooldown_relax: 0,
            cooldown_play: 0,
            cooldown_exercise: 0,
            cooldown_medicate: 0,
            cooldown_ozempic: 0,
            cooldown_drink: 0,
            cooldown_rehab: 0,
            cooldown_battle: 0,
            cooldown_tictactoe: 0,
            cooldown_lightsout: 0,
            cooldown_blackhole: 0,
            cooldown_nim: 0,
            cooldown_bornjeweled: 0,
            cooldown_onlypets: 0,

            miserable_interval_counter: 0,
            tired_passive_counter: 0,

            is_sleeping: false,
            hibernating: false,
            hibernate_ticks: 0,
            last_save_tick: 0,
            save_pending: true,
            realm_pending: false,
            naming_pending: false,
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
// Helpers
// ---------------------------------------------------------------------------

/// Saturating add for u16 stats (capped at STAT_MAX()).
fn sat_add(val: u16, delta: u16) -> u16 {
    val.saturating_add(delta)
}

/// Saturating sub for u16 stats (floored at 0).
fn sat_sub(val: u16, delta: u16) -> u16 {
    val.saturating_sub(delta)
}

/// Multiply rate × delta in u32 space, capped to u16 range.
/// This is the `y += m * dt` step — safe for large deltas.
/// Takes dt as u32 to avoid truncation on large piecewise segments.
fn mul_dt(rate: u16, dt: u32) -> u16 {
    (rate as u32 * dt).min(STAT_MAX() as u32) as u16
}

/// How many times an interval fires in `delta` ticks, given a counter
/// that has already accumulated `counter` ticks since the last fire.
/// Returns (fire_count, new_counter).
fn interval_fires(delta: u32, counter: u32, interval: u32) -> (u32, u32) {
    let interval = interval.max(1); // never divide by zero, whatever the source
    let total = counter + delta;
    let fires = total / interval;
    let new_counter = total % interval;
    (fires, new_counter)
}

/// Count how many of the three primary stats exceed the 60% threshold.
fn count_above_60(state: &GameState) -> u32 {
    let t = MISERABLE_STAT_THRESHOLD();
    (state.hunger > t) as u32 + (state.tired > t) as u32 + (state.sick > t) as u32
}

/// Check if any stat triggers sick condition decay.
fn sick_condition_active(state: &GameState) -> bool {
    state.hunger > SICK_TRIGGER_HUNGER() || state.tired > SICK_TRIGGER_TIRED()
}

/// Extra `sick` accrual for `delta` ticks while diabetic and unmedicated.
/// Zero while medication is protecting (`cooldown_medicate > 0`) or mid-dose.
fn diabetes_penalty(state: &GameState, delta: u32, miserable_high: bool) -> u16 {
    if !state.diabetic
        || state.cooldown_medicate > 0
        || state.active_action == Some(Action::Medicate)
    {
        return 0;
    }
    let rate = if miserable_high {
        DIABETES_SICK_MISERABLE_RATE()
    } else {
        DIABETES_SICK_RATE()
    };
    mul_dt(rate, delta)
}

/// Extra `sick` accrual for `delta` ticks while alcoholic and untreated.
/// Mirrors `diabetes_penalty` exactly — same pattern, different
/// permanent condition and its own treatment (rehab instead of insulin).
fn alcoholism_penalty(state: &GameState, delta: u32, miserable_high: bool) -> u16 {
    if !state.alcoholic || state.cooldown_rehab > 0 || state.active_action == Some(Action::Rehab) {
        return 0;
    }
    let rate = if miserable_high {
        ALCOHOLIC_SICK_MISERABLE_RATE()
    } else {
        ALCOHOLIC_SICK_RATE()
    };
    mul_dt(rate, delta)
}

/// Curiosity modifier for play costs: 0–10 range, higher = cheaper.
fn curiosity_modifier(curiosity: u16) -> u16 {
    (curiosity as u32 * 10 / STAT_MAX() as u32) as u16
}

/// Count of maxed stats (= STAT_MAX()).
fn count_maxed(state: &GameState) -> usize {
    (state.hunger == STAT_MAX()) as usize
        + (state.tired == STAT_MAX()) as usize
        + (state.sick == STAT_MAX()) as usize
}

// ---------------------------------------------------------------------------
// Delta-T update
// ---------------------------------------------------------------------------

impl GameState {
    /// Advance the game state by `(now_tick - last_update_tick)` ticks.
    ///
    /// Processes the elapsed time **piecewise**: at each step, the engine
    /// computes the ticks until the next rate-change boundary, applies
    /// stat decay at the current rates for that segment (one multiply
    /// per stat), then recalculates rates.  A 60-day delta with ~10
    /// boundary crossings per day takes ~600 iterations — instant.
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

        let mut remaining = total_delta;

        // Consume action ticks first (these are short, ≤ 4 ticks).
        remaining = self.consume_action_ticks(remaining);
        // Action completion may have zeroed `miserable` (Play does); the
        // floor re-asserts the severe / leaving caps immediately so the
        // reset can't undercut them.
        self.apply_miserable_floor();

        // Piecewise decay: advance to the next boundary, apply, repeat.
        while remaining > 0 && self.phase != Phase::Gone {
            // How far can we go at current rates before something changes?
            let segment = self.ticks_to_next_rate_change().min(remaining);
            let segment = segment.max(1); // always advance at least 1 tick

            self.consume_cooldowns(segment);

            if self.is_sleeping {
                self.apply_sleep_decay(segment);
            } else {
                self.apply_awake_decay(segment);
            }

            self.check_leaving(segment);
            self.check_diabetes(segment);
            self.check_alcoholism(segment);
            // Apply the severe/leaving floor after stats and phase have
            // been updated for this segment, so the next iteration's
            // rate calculation sees the bumped `miserable`.
            self.apply_miserable_floor();
            remaining -= segment;
        }
    }

    /// Enforce the minimum-`miserable` floor required by the severe and
    /// leaving caps:
    ///
    /// * `Phase::Leaving` → miserable ≥ 50 % of `STAT_MAX()` (≡ displayed Happy ≤
    ///   50 %).  This is a flat cap and does *not* add to the per-stat severe
    ///   penalties.
    /// * Each primary stat above its critical threshold → an additional −20 %
    ///   cap on Happy (= +20 % miserable per critical stat).  Up to 3 stats can
    ///   be critical, so the severe path can push miserable to 60 %.
    /// * The two rules are evaluated independently and the **higher** floor
    ///   wins (= lower Happy displayed).
    ///
    /// Recovery via `Play` only goes down to whichever floor is active
    /// at the time, so the player can never make a leaving / severely
    /// distressed pet appear happy.  Once the conditions clear (stats
    /// drop below critical AND phase returns to Active), the floor is
    /// 0 again and Play's reset works normally.
    fn apply_miserable_floor(&mut self) {
        let critical = (self.hunger > SICK_TRIGGER_HUNGER()) as u32
            + (self.tired > SICK_TRIGGER_TIRED()) as u32
            + (self.sick > SICK_TRIGGER_TIRED()) as u32;
        let floor_severe = (critical * (STAT_MAX() as u32 / 5)).min(STAT_MAX() as u32) as u16;
        let floor_leaving = if self.phase == Phase::Leaving {
            STAT_MAX() / 2
        } else {
            0
        };
        let floor = floor_severe.max(floor_leaving);
        if self.miserable < floor {
            self.miserable = floor;
        }
    }

    /// Ticks until a threshold crossing changes the rate equation.
    ///
    /// Every boundary where a stat's rate (or another stat's rate that
    /// depends on it) changes is checked.  Returns the minimum across all.
    fn ticks_to_next_rate_change(&self) -> u32 {
        let mut m = u32::MAX;
        let miserable_high = self.miserable >= MISERABLE_BOOST_THRESHOLD();

        // Helper: ticks for a linearly-increasing stat to reach `target`.
        let ticks_up = |val: u16, target: u16, rate: u16| -> u32 {
            if val >= target || rate == 0 {
                return u32::MAX;
            }
            (target - val) as u32 / rate as u32
        };

        // Helper: ticks for a linearly-decreasing stat to reach `target`.
        let ticks_down = |val: u16, target: u16, rate: u16| -> u32 {
            if val <= target || rate == 0 {
                return u32::MAX;
            }
            (val - target) as u32 / rate as u32
        };

        // Helper: ticks for an interval-based stat to reach `target`.
        let ticks_interval = |val: u16, target: u16, amount: u16, interval: u32| -> u32 {
            if val >= target || amount == 0 {
                return u32::MAX;
            }
            let fires = (target - val) as u32 / amount as u32;
            fires.saturating_mul(interval)
        };

        // Current hunger rate.
        let hunger_rate = if self.cooldown_feed > 0 {
            0
        } else {
            HUNGER_RATE()
                + if miserable_high {
                    HUNGER_MISERABLE_BOOST()
                } else {
                    0
                }
        };

        // Current tired rate (never suppressed).
        let tired_rate = TIRED_RATE()
            + if miserable_high {
                TIRED_MISERABLE_BOOST()
            } else {
                0
            };

        // Current miserable interval.
        let mis_interval = if self.cooldown_play > 0 {
            u32::MAX
        } else {
            let above = count_above_60(self);
            MISERABLE_INTERVAL_BASE()
                .saturating_sub(MISERABLE_INTERVAL_PER_STAT() * above)
                .max(MISERABLE_INTERVAL_MIN())
        };

        // ── Boundaries that change the miserable interval (count_above_60) ──

        // Each primary stat crossing 60% changes the miserable decay rate.
        let t60 = MISERABLE_STAT_THRESHOLD();
        m = m.min(ticks_up(self.hunger, t60, hunger_rate));
        m = m.min(ticks_up(self.tired, t60, tired_rate));
        // Sick rate mirrors apply_awake_decay/apply_sleep_decay's sick term
        // exactly (base + condition + diabetes + alcoholism, suppressed
        // during Heal) so the boundary estimate can't undershoot the real
        // rate. Omitting diabetes/alcoholism here previously made this an
        // underestimate for diabetic-unmedicated/alcoholic-untreated pets,
        // which oversized the piecewise segment and let check_leaving()
        // charge a whole oversized segment as "maxed" in one shot — enough
        // to jump straight past the Leaving phase into Gone on a long
        // fast-forward (e.g. the Skip 1 day cheat).
        let sick_rate_approx = if self.cooldown_heal > 0 || self.active_action == Some(Action::Heal)
        {
            0
        } else {
            let condition = if sick_condition_active(self) {
                if miserable_high {
                    SICK_CONDITION_MISERABLE_RATE()
                } else {
                    SICK_CONDITION_RATE()
                }
            } else {
                0
            };
            let diabetes = if self.diabetic
                && self.cooldown_medicate == 0
                && self.active_action != Some(Action::Medicate)
            {
                if miserable_high {
                    DIABETES_SICK_MISERABLE_RATE()
                } else {
                    DIABETES_SICK_RATE()
                }
            } else {
                0
            };
            let alcoholism = if self.alcoholic
                && self.cooldown_rehab == 0
                && self.active_action != Some(Action::Rehab)
            {
                if miserable_high {
                    ALCOHOLIC_SICK_MISERABLE_RATE()
                } else {
                    ALCOHOLIC_SICK_RATE()
                }
            } else {
                0
            };
            SICK_RATE()
                .saturating_add(condition)
                .saturating_add(diabetes)
                .saturating_add(alcoholism)
        };
        m = m.min(ticks_up(self.sick, t60, sick_rate_approx));

        // ── Boundaries that change sick condition decay ──

        m = m.min(ticks_up(self.hunger, SICK_TRIGGER_HUNGER(), hunger_rate));
        m = m.min(ticks_up(self.tired, SICK_TRIGGER_TIRED(), tired_rate));

        // ── Miserable thresholds (change hunger/tired rates) ──

        m = m.min(ticks_interval(
            self.miserable,
            MISERABLE_BOOST_THRESHOLD(),
            MISERABLE_AMOUNT(),
            mis_interval,
        ));

        // ── Stats reaching STAT_MAX (changes leaving behavior) ──

        m = m.min(ticks_up(self.hunger, STAT_MAX(), hunger_rate));
        m = m.min(ticks_up(self.tired, STAT_MAX(), tired_rate));
        m = m.min(ticks_up(self.sick, STAT_MAX(), sick_rate_approx));

        // ── Cooldown expiry (suppression ends → rate resumes) ──

        if self.cooldown_feed > 0 {
            m = m.min(self.cooldown_feed as u32);
        }
        if self.cooldown_heal > 0 {
            m = m.min(self.cooldown_heal as u32);
        }
        if self.cooldown_play > 0 {
            m = m.min(self.cooldown_play as u32);
        }
        if self.cooldown_exercise > 0 {
            m = m.min(self.cooldown_exercise as u32);
        }
        if self.cooldown_medicate > 0 {
            m = m.min(self.cooldown_medicate as u32);
        }
        if self.cooldown_ozempic > 0 {
            m = m.min(self.cooldown_ozempic as u32);
        }
        if self.cooldown_drink > 0 {
            m = m.min(self.cooldown_drink as u32);
        }
        if self.cooldown_rehab > 0 {
            m = m.min(self.cooldown_rehab as u32);
        }

        // ── Sleep tier transitions ──

        if self.is_sleeping {
            m = m.min(ticks_down(self.tired, SLEEP_TIER_SLOW(), SLEEP_RECOVERY_SLOW()).max(1));
            m = m.min(ticks_down(self.tired, SLEEP_TIER_MEDIUM(), SLEEP_RECOVERY_MEDIUM()).max(1));
            // Auto-wake: tired → 0.
            let wake_rate = if self.tired >= SLEEP_TIER_SLOW() {
                SLEEP_RECOVERY_SLOW()
            } else if self.tired >= SLEEP_TIER_MEDIUM() {
                SLEEP_RECOVERY_MEDIUM()
            } else {
                SLEEP_RECOVERY_FAST()
            };
            m = m.min(ticks_down(self.tired, 0, wake_rate).max(1));
        }

        m
    }

    /// Consume action ticks from delta, applying action effects.
    /// Returns remaining delta after action ticks are consumed.
    fn consume_action_ticks(&mut self, mut delta: u32) -> u32 {
        if let Some(action) = self.active_action {
            let ticks = delta.min(self.action_ticks_remaining as u32);
            self.action_ticks_remaining -= ticks as u8;
            delta -= ticks;

            let t = ticks as u16;
            match action {
                Action::Feed => {
                    let food = self.active_food.unwrap_or(FoodKind::Apple);
                    let hunger_relief = food.scale_hunger_relief(FEED_HUNGER_RELIEF());
                    let weight_gain = food.scale_weight_gain(FEED_WEIGHT_GAIN());
                    self.hunger = sat_sub(self.hunger, mul_dt(hunger_relief, t as u32));
                    // Overfeeding compounds the passive weight gain — how much
                    // depends entirely on what was eaten (see FoodKind).
                    self.weight = sat_add(self.weight, mul_dt(weight_gain, t as u32));
                }
                Action::Heal => {
                    self.sick = sat_sub(self.sick, mul_dt(HEAL_SICK_RELIEF(), t as u32));
                }
                Action::Play => {
                    let cm = curiosity_modifier(self.curiosity);
                    let cost_mul = (10u16.saturating_sub(cm)) as u32;
                    let apply = |base: u16| -> u16 {
                        mul_dt((base as u32 * cost_mul / 10) as u16, t as u32)
                    };
                    self.hunger = sat_add(self.hunger, apply(PLAY_HUNGER_COST()));
                    self.tired = sat_add(self.tired, apply(PLAY_TIRED_COST()));
                }
                Action::Exercise => {
                    self.weight = sat_sub(self.weight, mul_dt(EXERCISE_WEIGHT_RELIEF(), t as u32));
                    self.tired = sat_add(self.tired, mul_dt(EXERCISE_TIRED_COST(), t as u32));
                    self.hunger = sat_add(self.hunger, mul_dt(EXERCISE_HUNGER_COST(), t as u32));
                }
                Action::Medicate => {}
                Action::Ozempic => {
                    self.weight = sat_sub(self.weight, mul_dt(OZEMPIC_WEIGHT_RELIEF(), t as u32));
                    self.hunger = sat_sub(self.hunger, mul_dt(OZEMPIC_HUNGER_RELIEF(), t as u32));
                }
                Action::Drink => {
                    let drink = self.active_drink.unwrap_or(DrinkKind::Beer);
                    let drunk_gain = drink.scale_drunk_gain(DRINK_DRUNK_GAIN());
                    let weight_gain = drink.scale_weight_gain(DRINK_WEIGHT_GAIN());
                    self.drunk = sat_add(self.drunk, mul_dt(drunk_gain, t as u32));
                    self.weight = sat_add(self.weight, mul_dt(weight_gain, t as u32));
                }
                Action::Rehab => {}
                Action::OnlyPets => {}
            }

            if self.action_ticks_remaining == 0 {
                // Action complete — set cooldown.
                match action {
                    Action::Feed => self.cooldown_feed = FEED_COOLDOWN(),
                    Action::Heal => self.cooldown_heal = HEAL_COOLDOWN(),
                    Action::Play => {
                        // +30% happiness (was: zero it out entirely) — see
                        // `HAPPINESS_STEP` doc comment.
                        self.miserable = sat_sub(self.miserable, HAPPINESS_STEP);
                        self.cooldown_play = PLAY_COOLDOWN();
                    }
                    Action::Exercise => self.cooldown_exercise = EXERCISE_COOLDOWN(),
                    Action::Medicate => self.cooldown_medicate = MEDICATE_COOLDOWN(),
                    Action::Ozempic => self.cooldown_ozempic = OZEMPIC_COOLDOWN(),
                    Action::Drink => self.cooldown_drink = DRINK_COOLDOWN(),
                    Action::Rehab => self.cooldown_rehab = REHAB_COOLDOWN(),
                    Action::OnlyPets => {
                        // Only reachable when `money_enabled` (see `only_pets()`
                        // doc comment) — balance is unchanged during the
                        // animation, so checking it here is equivalent to
                        // checking it at start time.
                        if self.money_enabled {
                            if self.money < BROKE_THRESHOLD {
                                // Broke: forced work — big pay, but happiness drops.
                                self.add_money(ONLYPETS_BROKE_REWARD);
                                self.miserable = sat_add(self.miserable, HAPPINESS_STEP);
                            } else {
                                // Has money: hobby — small pay, happiness rises.
                                self.add_money(ONLYPETS_HOBBY_REWARD);
                                self.miserable = sat_sub(self.miserable, HAPPINESS_STEP);
                            }
                        }
                        self.cooldown_onlypets = PLAY_COOLDOWN();
                    }
                }
                self.active_action = None;
                self.active_food = None;
            }
        }
        delta
    }

    /// Consume cooldown ticks from delta.
    fn consume_cooldowns(&mut self, delta: u32) {
        // Cooldowns are u16; delta may exceed u16 on large jumps.
        let d = delta.min(u16::MAX as u32) as u16;
        self.cooldown_feed = self.cooldown_feed.saturating_sub(d);
        self.cooldown_heal = self.cooldown_heal.saturating_sub(d);
        self.cooldown_relax = self.cooldown_relax.saturating_sub(d);
        self.cooldown_play = self.cooldown_play.saturating_sub(d);
        self.cooldown_exercise = self.cooldown_exercise.saturating_sub(d);
        self.cooldown_medicate = self.cooldown_medicate.saturating_sub(d);
        self.cooldown_ozempic = self.cooldown_ozempic.saturating_sub(d);
        self.cooldown_drink = self.cooldown_drink.saturating_sub(d);
        self.cooldown_rehab = self.cooldown_rehab.saturating_sub(d);
        self.cooldown_battle = self.cooldown_battle.saturating_sub(d);
        self.cooldown_tictactoe = self.cooldown_tictactoe.saturating_sub(d);
        self.cooldown_lightsout = self.cooldown_lightsout.saturating_sub(d);
        self.cooldown_blackhole = self.cooldown_blackhole.saturating_sub(d);
        self.cooldown_nim = self.cooldown_nim.saturating_sub(d);
        self.cooldown_bornjeweled = self.cooldown_bornjeweled.saturating_sub(d);
        self.cooldown_onlypets = self.cooldown_onlypets.saturating_sub(d);
    }

    /// Apply stat decay while awake for `delta` ticks.
    fn apply_awake_decay(&mut self, delta: u32) {
        let miserable_high = self.miserable >= MISERABLE_BOOST_THRESHOLD();

        // Hunger (suppressed during feed action + cooldown).
        if self.cooldown_feed == 0 && self.active_action != Some(Action::Feed) {
            let rate = HUNGER_RATE()
                + if miserable_high {
                    HUNGER_MISERABLE_BOOST()
                } else {
                    0
                };
            self.hunger = sat_add(self.hunger, mul_dt(rate, delta));
        }

        // Tired (never suppressed).
        {
            let rate = TIRED_RATE()
                + if miserable_high {
                    TIRED_MISERABLE_BOOST()
                } else {
                    0
                };
            self.tired = sat_add(self.tired, mul_dt(rate, delta));
        }

        // Tired passive recovery.
        {
            let (fires, new_counter) =
                interval_fires(delta, self.tired_passive_counter, TIRED_PASSIVE_INTERVAL());
            self.tired_passive_counter = new_counter;
            if fires > 0 {
                self.tired = sat_sub(self.tired, mul_dt(TIRED_PASSIVE_RECOVERY(), fires));
            }
        }

        // Sick (suppressed during heal action + cooldown).
        if self.cooldown_heal == 0 && self.active_action != Some(Action::Heal) {
            let base = mul_dt(SICK_RATE(), delta);
            let condition = if sick_condition_active(self) {
                let rate = if miserable_high {
                    SICK_CONDITION_MISERABLE_RATE()
                } else {
                    SICK_CONDITION_RATE()
                };
                mul_dt(rate, delta)
            } else {
                0
            };
            let diabetes = diabetes_penalty(self, delta, miserable_high);
            let alcoholism = alcoholism_penalty(self, delta, miserable_high);
            self.sick = sat_add(
                self.sick,
                base.saturating_add(condition)
                    .saturating_add(diabetes)
                    .saturating_add(alcoholism),
            );
        }

        // Weight (passive gain — a slow, multi-day drift; Exercise is the
        // relief valve, Feed adds a small extra bump on completion above).
        self.weight = sat_add(self.weight, mul_dt(WEIGHT_RATE(), delta));

        // Drunk (passive sobering — unlike weight, this decays on its
        // own; repeated drinking is what's needed to keep it elevated
        // long enough to trigger alcoholism).
        self.drunk = sat_sub(self.drunk, mul_dt(DRUNK_SOBER_RATE(), delta));

        // Miserable (suppressed during play action + cooldown).
        if self.cooldown_play == 0 && self.active_action != Some(Action::Play) {
            let above = count_above_60(self);
            let interval = MISERABLE_INTERVAL_BASE()
                .saturating_sub(MISERABLE_INTERVAL_PER_STAT() * above)
                .max(MISERABLE_INTERVAL_MIN());
            let (fires, new_counter) =
                interval_fires(delta, self.miserable_interval_counter, interval);
            self.miserable_interval_counter = new_counter;
            if fires > 0 {
                self.miserable = sat_add(self.miserable, mul_dt(MISERABLE_AMOUNT(), fires));
            }
        }
    }

    /// Apply stat changes during sleep for `delta` ticks.
    fn apply_sleep_decay(&mut self, delta: u32) {
        let miserable_high = self.miserable >= MISERABLE_BOOST_THRESHOLD();

        // Tired recovery (tiered by current level).
        let recovery_rate = if self.tired >= SLEEP_TIER_SLOW() {
            SLEEP_RECOVERY_SLOW()
        } else if self.tired >= SLEEP_TIER_MEDIUM() {
            SLEEP_RECOVERY_MEDIUM()
        } else {
            SLEEP_RECOVERY_FAST()
        };
        self.tired = sat_sub(self.tired, mul_dt(recovery_rate, delta));

        // Auto-wake when tired reaches 0.
        if self.tired == 0 {
            self.is_sleeping = false;
        }

        // Hunger still decays during sleep, and faster than awake —
        // sleeping is restorative, not free.
        if self.cooldown_feed == 0 {
            let rate = HUNGER_RATE()
                + SLEEP_HUNGER_COST()
                + if miserable_high {
                    HUNGER_MISERABLE_BOOST()
                } else {
                    0
                };
            self.hunger = sat_add(self.hunger, mul_dt(rate, delta));
        }

        // Sick still decays during sleep.
        if self.cooldown_heal == 0 {
            let base = mul_dt(SICK_RATE(), delta);
            let condition = if sick_condition_active(self) {
                let rate = if miserable_high {
                    SICK_CONDITION_MISERABLE_RATE()
                } else {
                    SICK_CONDITION_RATE()
                };
                mul_dt(rate, delta)
            } else {
                0
            };
            let diabetes = diabetes_penalty(self, delta, miserable_high);
            let alcoholism = alcoholism_penalty(self, delta, miserable_high);
            self.sick = sat_add(
                self.sick,
                base.saturating_add(condition)
                    .saturating_add(diabetes)
                    .saturating_add(alcoholism),
            );
        }

        // Weight still drifts upward during sleep — metabolism doesn't stop.
        self.weight = sat_add(self.weight, mul_dt(WEIGHT_RATE(), delta));
        // Same for sobering up.
        self.drunk = sat_sub(self.drunk, mul_dt(DRUNK_SOBER_RATE(), delta));
    }

    /// Check leaving conditions and update leaving countdown.
    fn check_leaving(&mut self, delta: u32) {
        let maxed = count_maxed(self);
        if maxed == 0 {
            self.leaving_countdown = 0;
            if self.phase == Phase::Leaving {
                self.phase = Phase::Active;
                self.save_pending = true;
            }
            return;
        }

        self.leaving_countdown += delta;
        let threshold = LEAVING_THRESHOLDS()[maxed.min(4)];

        if self.leaving_countdown >= threshold {
            self.phase = Phase::Gone;
            self.save_pending = true;
            self.realm_pending = true;
        } else if self.phase == Phase::Active {
            self.phase = Phase::Leaving;
            self.save_pending = true;
        }
    }

    /// Track sustained overweight duration and trigger permanent
    /// diabetes once `DIABETES_ONSET_TICKS()` is reached.  Mirrors the
    /// existing "neglect one stat long enough → penalty elsewhere"
    /// pattern used by `sick_condition_active()`, but the trigger here
    /// is a one-way flag rather than a continuously-reevaluated
    /// condition — real type 2 diabetes doesn't reverse once it sets in.
    fn check_diabetes(&mut self, delta: u32) {
        if self.diabetic {
            return;
        }
        if self.weight > OVERWEIGHT_TRIGGER() {
            self.overweight_ticks = self.overweight_ticks.saturating_add(delta);
            if self.overweight_ticks >= DIABETES_ONSET_TICKS() {
                self.diabetic = true;
            }
        } else {
            self.overweight_ticks = 0;
        }
    }

    /// Track sustained drunkenness and trigger permanent alcoholism once
    /// `ALCOHOLIC_ONSET_TICKS()` is reached — mirrors `check_diabetes()`
    /// exactly, just keyed off `drunk` instead of `weight`.
    fn check_alcoholism(&mut self, delta: u32) {
        if self.alcoholic {
            return;
        }
        if self.drunk > DRUNK_TRIGGER() {
            self.drunk_ticks = self.drunk_ticks.saturating_add(delta);
            if self.drunk_ticks >= ALCOHOLIC_ONSET_TICKS() {
                self.alcoholic = true;
            }
        } else {
            self.drunk_ticks = 0;
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

    /// Start the feed action with the chosen food.  Returns false if not
    /// available.
    pub fn feed(&mut self, food: FoodKind) -> bool {
        if !self.is_alive() || self.is_sleeping {
            return false;
        }
        if self.active_action.is_some() || self.cooldown_feed > 0 {
            return false;
        }
        // Affordability gate: reject the action outright when unaffordable
        // rather than letting it proceed for free (see `spend_money`).
        if self.money_enabled && !self.spend_money(food.hex_price(self.hard_mode)) {
            return false; // can't afford — action rejected
        }
        self.active_action = Some(Action::Feed);
        self.active_food = Some(food);
        self.action_ticks_remaining = FEED_DURATION();
        true
    }

    /// Start the heal action.
    pub fn heal(&mut self) -> bool {
        if !self.is_alive() || self.is_sleeping {
            return false;
        }
        if self.active_action.is_some() || self.cooldown_heal > 0 {
            return false;
        }
        // Aspirine costs HEX; Stage 5 affordability gate (reject if broke).
        if self.money_enabled && !self.spend_money(ASPIRINE_HEX_COST) {
            return false;
        }
        self.active_action = Some(Action::Heal);
        self.action_ticks_remaining = HEAL_DURATION();
        true
    }

    /// Start the play action.
    pub fn play(&mut self) -> bool {
        if !self.is_alive() || self.is_sleeping {
            return false;
        }
        if self.active_action.is_some() || self.cooldown_play > 0 {
            return false;
        }
        // Affordability gate — see the comment in `feed()`.
        if self.money_enabled && !self.spend_money(PLAY_HEX_COST) {
            return false; // can't afford — action rejected
        }
        self.active_action = Some(Action::Play);
        self.action_ticks_remaining = PLAY_DURATION();
        true
    }

    /// Send the pet to "Only pets" to earn HEX. Timed action like Play.
    pub fn only_pets(&mut self) -> bool {
        if !self.is_alive() || self.is_sleeping {
            return false;
        }
        if self.active_action.is_some() || self.cooldown_onlypets > 0 {
            return false;
        }
        self.active_action = Some(Action::OnlyPets);
        self.action_ticks_remaining = PLAY_DURATION();
        true
    }

    /// Start the exercise action — the primary relief valve for `weight`.
    pub fn exercise(&mut self) -> bool {
        if !self.is_alive() || self.is_sleeping {
            return false;
        }
        if self.active_action.is_some() || self.cooldown_exercise > 0 {
            return false;
        }
        self.active_action = Some(Action::Exercise);
        self.action_ticks_remaining = EXERCISE_DURATION();
        true
    }

    /// Administer diabetes medication.  Only meaningful once `diabetic`
    /// is set — resets `cooldown_medicate`, which doubles as the
    /// protection window during which the diabetes sick-penalty is
    /// suppressed.
    pub fn medicate(&mut self) -> bool {
        if !self.diabetic {
            return false;
        }
        if !self.is_alive() || self.is_sleeping {
            return false;
        }
        if self.active_action.is_some() || self.cooldown_medicate > 0 {
            return false;
        }
        // Affordability gate — see the comment in `feed()`.
        if self.money_enabled && !self.spend_money(medication_price(self.hard_mode)) {
            return false; // can't afford — action rejected
        }
        self.active_action = Some(Action::Medicate);
        self.action_ticks_remaining = MEDICATE_DURATION();
        true
    }

    /// Administer Ozempic — a stronger, faster-acting weight-loss
    /// treatment than Exercise. Unlike `medicate()`, this is *not*
    /// gated on being diabetic — any pet can take it.
    pub fn ozempic(&mut self) -> bool {
        if !self.is_alive() || self.is_sleeping {
            return false;
        }
        if self.active_action.is_some() || self.cooldown_ozempic > 0 {
            return false;
        }
        // Affordability gate — see the comment in `feed()`.
        if self.money_enabled && !self.spend_money(medication_price(self.hard_mode)) {
            return false; // can't afford — action rejected
        }
        self.active_action = Some(Action::Ozempic);
        self.action_ticks_remaining = OZEMPIC_DURATION();
        true
    }

    /// Start the drink action with the chosen drink.  Returns false if
    /// not available.
    pub fn drink(&mut self, drink: DrinkKind) -> bool {
        if !self.is_alive() || self.is_sleeping {
            return false;
        }
        if self.active_action.is_some() || self.cooldown_drink > 0 {
            return false;
        }
        // Affordability gate — see the comment in `feed()`.
        if self.money_enabled && !self.spend_money(drink.hex_price()) {
            return false; // can't afford — action rejected
        }
        self.active_action = Some(Action::Drink);
        self.active_drink = Some(drink);
        self.action_ticks_remaining = DRINK_DURATION();
        true
    }

    /// Administer rehab treatment for alcoholism.  Only meaningful once
    /// `alcoholic` is set — mirrors `medicate()`.
    pub fn rehab(&mut self) -> bool {
        if !self.alcoholic {
            return false;
        }
        if !self.is_alive() || self.is_sleeping {
            return false;
        }
        if self.active_action.is_some() || self.cooldown_rehab > 0 {
            return false;
        }
        // Affordability gate — see the comment in `feed()`.
        if self.money_enabled && !self.spend_money(rehab_price(self.hard_mode)) {
            return false; // can't afford — action rejected
        }
        self.active_action = Some(Action::Rehab);
        self.action_ticks_remaining = REHAB_DURATION();
        true
    }

    /// Put the pet to sleep.
    pub fn sleep(&mut self) -> bool {
        if !self.is_alive() || self.is_sleeping {
            return false;
        }
        self.is_sleeping = true;
        true
    }

    /// Wake the pet up.
    pub fn wake(&mut self) -> bool {
        if !self.is_sleeping {
            return false;
        }
        self.is_sleeping = false;
        true
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

    /// Award the mini-game win reward: HEX (when `money_enabled`) plus the
    /// per-game cooldown.  Starts only that game's cooldown so other
    /// mini-games stay available — encourages variety.  Also bumps
    /// hunger: playing burns calories.
    pub fn award_inspiration(&mut self, game: MiniGame) {
        if self.phase != Phase::Active {
            return;
        }
        self.hunger = sat_add(self.hunger, MINIGAME_HUNGER_COST());
        match game {
            MiniGame::TicTacToe => self.cooldown_tictactoe = MINIGAME_COOLDOWN(),
            MiniGame::LightsOut => self.cooldown_lightsout = MINIGAME_COOLDOWN(),
            MiniGame::BlackHole => self.cooldown_blackhole = MINIGAME_COOLDOWN(),
            MiniGame::Nim => self.cooldown_nim = MINIGAME_COOLDOWN(),
            MiniGame::BornJeweled => self.cooldown_bornjeweled = MINIGAME_COOLDOWN(),
        }
        if self.money_enabled {
            self.add_money(MINIGAME_HEX_REWARD);
        }
    }

    /// Total hours the pet has spent in hibernation during its life.
    pub fn hibernate_hours(&self) -> u32 {
        self.hibernate_ticks / 360 // 360 ticks = 1 hour
    }
}

// ---------------------------------------------------------------------------
// Debug cheats — reachable only via the hidden button sequence in
// `crate::game::debug_cheats`. Not part of normal gameplay; exist purely so
// the multi-day weight/diabetes arc can be tested in seconds instead of days.
// ---------------------------------------------------------------------------

impl GameState {
    /// Push `weight` just over the overweight trigger.
    pub fn debug_force_overweight(&mut self) {
        self.weight = OVERWEIGHT_TRIGGER().saturating_add(1);
    }

    /// Flip `diabetic` on directly, skipping the sustained-overweight timer.
    pub fn debug_force_diabetic(&mut self) {
        self.diabetic = true;
    }

    /// Clear diabetes and the overweight-duration counter, so the arc can
    /// be re-tested from scratch without starting a new pet.
    pub fn debug_clear_diabetes(&mut self) {
        self.diabetic = false;
        self.overweight_ticks = 0;
    }

    /// Fast-forward the engine by `ticks` in one shot — runs the same
    /// `update()` path real time would, just compressed. Used for "Skip
    /// 1 hour" / "Skip 1 day" cheat items.
    pub fn debug_skip_ticks(&mut self, ticks: u32) {
        let target = self.last_update_tick.saturating_add(ticks);
        self.update(target);
    }

    /// Push `drunk` just over the alcoholism trigger.
    pub fn debug_force_drunk(&mut self) {
        self.drunk = DRUNK_TRIGGER().saturating_add(1);
    }

    /// Flip `alcoholic` on directly, skipping the sustained-drunk timer.
    pub fn debug_force_alcoholic(&mut self) {
        self.alcoholic = true;
    }

    /// Clear alcoholism and the drunk-duration counter, so the arc can
    /// be re-tested from scratch without starting a new pet.
    pub fn debug_clear_alcoholism(&mut self) {
        self.alcoholic = false;
        self.drunk_ticks = 0;
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
        self.cooldown_battle = 0;
    }
}

// ---------------------------------------------------------------------------
// Next wake time — boundary-based scheduling
// ---------------------------------------------------------------------------

impl GameState {
    /// Compute the number of ticks until the next interesting event.
    ///
    /// Returns the absolute tick time to wake up at.  The caller should
    /// set a timer for `next_wake_tick - last_update_tick` ticks.
    pub fn next_wake_tick(&self) -> u32 {
        if self.phase == Phase::Gone || self.hibernating {
            return u32::MAX;
        }

        let now = self.last_update_tick;
        let mut earliest = now + MAX_SLEEP_TICKS();

        // Hatching countdown.
        if self.phase == Phase::Hatching {
            return now + self.hatching_countdown as u32;
        }

        // Active action completion.
        if self.active_action.is_some() {
            earliest = earliest.min(now + self.action_ticks_remaining as u32);
        }

        // Cooldown expiry.
        if self.cooldown_feed > 0 {
            earliest = earliest.min(now + self.cooldown_feed as u32);
        }
        if self.cooldown_heal > 0 {
            earliest = earliest.min(now + self.cooldown_heal as u32);
        }
        if self.cooldown_relax > 0 {
            earliest = earliest.min(now + self.cooldown_relax as u32);
        }
        if self.cooldown_play > 0 {
            earliest = earliest.min(now + self.cooldown_play as u32);
        }
        if self.cooldown_exercise > 0 {
            earliest = earliest.min(now + self.cooldown_exercise as u32);
        }
        if self.cooldown_medicate > 0 {
            earliest = earliest.min(now + self.cooldown_medicate as u32);
        }
        if self.cooldown_ozempic > 0 {
            earliest = earliest.min(now + self.cooldown_ozempic as u32);
        }
        if self.cooldown_drink > 0 {
            earliest = earliest.min(now + self.cooldown_drink as u32);
        }
        if self.cooldown_rehab > 0 {
            earliest = earliest.min(now + self.cooldown_rehab as u32);
        }
        if self.cooldown_battle > 0 {
            earliest = earliest.min(now + self.cooldown_battle as u32);
        }

        // Stat boundary crossings.
        let miserable_high = self.miserable >= MISERABLE_BOOST_THRESHOLD();

        // Hunger → sick trigger threshold.
        if self.hunger < SICK_TRIGGER_HUNGER() && self.cooldown_feed == 0 {
            let rate = HUNGER_RATE()
                + if miserable_high {
                    HUNGER_MISERABLE_BOOST()
                } else {
                    0
                };
            if rate > 0 {
                let ticks = (SICK_TRIGGER_HUNGER() - self.hunger) as u32 / rate as u32;
                earliest = earliest.min(now + ticks);
            }
        }

        // Tired → sick trigger threshold.
        if self.tired < SICK_TRIGGER_TIRED() {
            let rate = TIRED_RATE()
                + if miserable_high {
                    TIRED_MISERABLE_BOOST()
                } else {
                    0
                };
            if rate > 0 {
                let ticks = (SICK_TRIGGER_TIRED() - self.tired) as u32 / rate as u32;
                earliest = earliest.min(now + ticks);
            }
        }

        // Miserable → 70% boost threshold.
        if self.miserable < MISERABLE_BOOST_THRESHOLD() && self.cooldown_play == 0 {
            let above = count_above_60(self);
            let interval = MISERABLE_INTERVAL_BASE()
                .saturating_sub(MISERABLE_INTERVAL_PER_STAT() * above)
                .max(MISERABLE_INTERVAL_MIN());
            // Average rate: MISERABLE_AMOUNT() / interval.
            let fires_to_threshold =
                (MISERABLE_BOOST_THRESHOLD() - self.miserable) as u32 / MISERABLE_AMOUNT() as u32;
            let ticks = fires_to_threshold * interval;
            earliest = earliest.min(now + ticks);
        }

        // Leaving countdown.
        if self.phase == Phase::Leaving {
            let maxed = count_maxed(self);
            if maxed > 0 {
                let threshold = LEAVING_THRESHOLDS()[maxed.min(4)];
                let remaining = threshold.saturating_sub(self.leaving_countdown);
                earliest = earliest.min(now + remaining);
            }
        }

        // Sleep: tired reaching 0 (auto-wake).
        if self.is_sleeping && self.tired > 0 {
            let rate = if self.tired >= SLEEP_TIER_SLOW() {
                SLEEP_RECOVERY_SLOW()
            } else if self.tired >= SLEEP_TIER_MEDIUM() {
                SLEEP_RECOVERY_MEDIUM()
            } else {
                SLEEP_RECOVERY_FAST()
            };
            let ticks = self.tired as u32 / rate as u32;
            earliest = earliest.min(now + ticks.max(1));
        }

        earliest
    }
}

// ---------------------------------------------------------------------------
// PetStats — display-friendly snapshot (0 = bad, 100 = good)
// ---------------------------------------------------------------------------

/// Display-friendly snapshot of the pet's state.
///
/// All stats are scaled 0–100 with **positive semantics**: 100 = perfect,
/// 0 = critical.  This is the inverse of the internal u16 representation
/// where 0 = best.
///
/// Obtain via [`GameState::stats()`] which triggers a state update first.
/// The result is cached — calling `stats()` again at the same tick is free.
#[derive(Clone, Copy, Debug)]
#[cfg_attr(feature = "embassy-base", derive(defmt::Format))]
pub struct PetStats {
    /// How well-fed the pet is (100 = full, 0 = starving).
    pub hunger: u8,
    /// How rested the pet is (100 = alert, 0 = exhausted).
    pub tired: u8,
    /// How healthy the pet is (100 = healthy, 0 = critically ill).
    pub healthy: u8,
    /// How happy the pet is (100 = happy, 0 = miserable).
    pub happy: u8,
    /// How lean/fit the pet is (100 = lean, 0 = obese).
    pub weight: u8,
    /// How sober the pet is (100 = stone sober, 0 = maximally drunk).
    pub drunk: u8,

    /// Whether the pet has developed type 2 diabetes (permanent once set).
    pub diabetic: bool,
    /// Whether weight is currently above the overweight trigger — the
    /// same condition that (if sustained long enough) leads to
    /// diabetes. Independent of `diabetic`, which is permanent once
    /// set: a pet can be overweight without (yet) being diabetic, or
    /// diabetic while currently back under the overweight line.
    pub overweight: bool,
    /// Whether the pet has developed alcoholism (permanent once set) —
    /// same relationship to `drunk` as `diabetic` has to `overweight`.
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

    /// Current HEX balance. Only meaningful (displayed/priced) when
    /// `money_enabled`.
    pub money: u32,
    /// Whether the money layer is active for this pet — gates HEX display,
    /// pricing, and menu affordability checks.
    pub money_enabled: bool,
    /// Whether Hard (US) prices are in effect. Only meaningful when
    /// `money_enabled` is true — see `GameState::hard_mode`.
    pub hard_mode: bool,

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
    pub can_sleep: bool,
    pub can_wake: bool,
    /// Per-mini-game availability.  False while that game's post-win
    /// cooldown is running; the others stay independently playable.
    pub can_play_tictactoe: bool,
    pub can_play_lightsout: bool,
    pub can_play_blackhole: bool,
    pub can_play_nim: bool,
    pub can_play_bornjeweled: bool,
    /// Whether a mesh Battle can be started right now (cooldown-gated,
    /// same shape as the mini-game `can_play_*` flags above). Does not
    /// factor in whether any friends exist — the Battle screen handles
    /// the "no friends yet" empty state itself.
    pub can_battle: bool,

    /// Remaining action cooldowns in ticks (1 tick = 10 s).  0 = ready.
    /// Mirrored from the matching `GameState` fields so the modal can
    /// show the exact remaining time on a disabled menu row.
    pub cooldown_feed: u16,
    pub cooldown_heal: u16,
    pub cooldown_play: u16,
    pub cooldown_onlypets: u16,
    pub cooldown_exercise: u16,
    pub cooldown_medicate: u16,
    pub cooldown_ozempic: u16,
    pub cooldown_drink: u16,
    pub cooldown_rehab: u16,
    pub cooldown_tictactoe: u16,
    pub cooldown_lightsout: u16,
    pub cooldown_blackhole: u16,
    pub cooldown_nim: u16,
    pub cooldown_bornjeweled: u16,
    pub cooldown_battle: u16,

    /// Whether the pet is hibernating (all progression frozen).
    pub hibernating: bool,
    /// Total hours spent in hibernation during this pet's life.
    pub hibernate_hours: u32,
}

/// Convert internal stat (0=good, 65535=bad) to display (0=bad, 100=good).
fn to_display_pct(val: u16) -> u8 {
    100 - (val as u32 * 100 / STAT_MAX() as u32) as u8
}

impl GameState {
    /// Update the game state to `now_tick` and return a display snapshot.
    ///
    /// This is the primary API for the UI layer.  It triggers a state
    /// update, then returns the snapshot.  The result is cached internally —
    /// calling `stats()` again at the same tick skips the update and
    /// returns the cached snapshot.
    pub fn stats(&mut self, now_tick: u32) -> PetStats {
        // Only update if time has advanced since last call.
        if now_tick != self.last_update_tick {
            self.update(now_tick);
        }

        let action_idle = self.active_action.is_none();
        let alive = self.phase == Phase::Active || self.phase == Phase::Leaving;
        let awake_active = alive && !self.is_sleeping;

        PetStats {
            hunger: to_display_pct(self.hunger),
            tired: to_display_pct(self.tired),
            healthy: to_display_pct(self.sick),
            happy: to_display_pct(self.miserable),
            weight: to_display_pct(self.weight),
            drunk: to_display_pct(self.drunk),

            diabetic: self.diabetic,
            overweight: self.weight > OVERWEIGHT_TRIGGER(),
            alcoholic: self.alcoholic,

            phase: self.phase,
            is_sleeping: self.is_sleeping,
            age_ticks: self.age_ticks,
            generation: self.generation,

            wins: self.wins,
            losses: self.losses,

            money: self.money,
            money_enabled: self.money_enabled,
            hard_mode: self.hard_mode,

            active_action: self.active_action,
            action_ticks_remaining: self.action_ticks_remaining,

            can_feed: awake_active && action_idle && self.cooldown_feed == 0,
            can_heal: awake_active && action_idle && self.cooldown_heal == 0,
            can_play: awake_active && action_idle && self.cooldown_play == 0,
            can_only_pets: awake_active && action_idle && self.cooldown_onlypets == 0,
            can_exercise: awake_active && action_idle && self.cooldown_exercise == 0,
            can_medicate: awake_active
                && action_idle
                && self.diabetic
                && self.cooldown_medicate == 0,
            can_ozempic: awake_active && action_idle && self.cooldown_ozempic == 0,
            can_drink: awake_active && action_idle && self.cooldown_drink == 0,
            can_rehab: awake_active && action_idle && self.alcoholic && self.cooldown_rehab == 0,
            can_sleep: alive && !self.is_sleeping,
            can_wake: self.is_sleeping,
            can_play_tictactoe: awake_active && action_idle && self.cooldown_tictactoe == 0,
            can_play_lightsout: awake_active && action_idle && self.cooldown_lightsout == 0,
            can_play_blackhole: awake_active && action_idle && self.cooldown_blackhole == 0,
            can_play_nim: awake_active && action_idle && self.cooldown_nim == 0,
            can_play_bornjeweled: awake_active && action_idle && self.cooldown_bornjeweled == 0,
            can_battle: awake_active && action_idle && self.cooldown_battle == 0,

            cooldown_feed: self.cooldown_feed,
            cooldown_heal: self.cooldown_heal,
            cooldown_play: self.cooldown_play,
            cooldown_onlypets: self.cooldown_onlypets,
            cooldown_exercise: self.cooldown_exercise,
            cooldown_medicate: self.cooldown_medicate,
            cooldown_ozempic: self.cooldown_ozempic,
            cooldown_drink: self.cooldown_drink,
            cooldown_rehab: self.cooldown_rehab,
            cooldown_tictactoe: self.cooldown_tictactoe,
            cooldown_lightsout: self.cooldown_lightsout,
            cooldown_blackhole: self.cooldown_blackhole,
            cooldown_nim: self.cooldown_nim,
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

    /// Reduce `miserable` from meeting (or spending time with) a mesh
    /// friend over the SHDW channel — see `crate::game::friends`.
    /// `big` is a bigger one-time bump for a brand-new friend; a smaller
    /// bump applies to a cooldown-gated recurring reunion with someone
    /// already known.  Just another `miserable` reduction, so it's
    /// naturally re-clamped by `apply_miserable_floor()` on the next tick
    /// if the pet is Leaving/critical, the same way `Play` already works.
    pub fn friend_boost(&mut self, big: bool) {
        // STAT_MAX() is 65535, so `* 2` overflows u16 before the `/ 5` can
        // bring it back down — widen to u32 for the multiply.
        let amount = if big {
            (STAT_MAX() as u32 * 2 / 5) as u16 // ~40%
        } else {
            STAT_MAX() / 10 // ~10%
        };
        self.miserable = self.miserable.saturating_sub(amount);
    }

    /// Record the outcome of a mesh Battle — see `crate::game::battle`.
    /// Purely a lifetime win/loss tally plus the cooldown gate; battle HP
    /// itself is never persisted or connected to any real stat, so a loss
    /// here has no effect on the pet's actual health/lifecycle.
    pub fn record_battle(&mut self, won: bool) {
        if won {
            self.wins = self.wins.saturating_add(1);
        } else {
            self.losses = self.losses.saturating_add(1);
        }
        self.cooldown_battle = BATTLE_COOLDOWN();
        if won && self.money_enabled {
            self.add_money(BATTLE_HEX_REWARD);
        }
    }

    /// Credit `delta` HEX, saturating at `u32::MAX`. Immediately flags the
    /// state for save (see `request_save`) rather than waiting for the
    /// next 15-minute save interval — same immediate-persist pattern as
    /// pet name / hibernate, since a HEX balance is worth protecting from
    /// an unlucky reboot right after being earned.
    pub fn add_money(&mut self, delta: u32) {
        self.money = self.money.saturating_add(delta);
        self.request_save();
    }

    /// Debit `price` HEX if affordable. Returns `true` and flags the state
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

/// Format version stamped into byte 0 of every save. Bump this whenever
/// the field layout below changes so an old-layout blob from a prior
/// firmware fails the guard in `from_bytes` (→ clean fresh egg) instead
/// of being silently reinterpreted with the new offsets. v1 was the
/// original unversioned 96-byte layout; v2 adds this byte + HEX money.
pub const SAVE_FORMAT_VERSION: u8 = 2;

/// Serialized size of GameState in bytes (1 version byte + 96 fields).
pub const SAVE_SIZE: usize = 97;

impl GameState {
    /// Serialize the game state to a fixed-size byte buffer for ekv.
    #[allow(unused_assignments)]
    pub fn to_bytes(&self) -> [u8; SAVE_SIZE] {
        let mut b = [0u8; SAVE_SIZE];
        // Byte 0 = format version so a differently-laid-out save written by
        // another firmware is rejected by from_bytes instead of silently
        // misparsed with these offsets. Fields start at index 1.
        b[0] = SAVE_FORMAT_VERSION;
        let mut i = 1;

        macro_rules! w16 {
            ($v:expr) => {
                b[i..i + 2].copy_from_slice(&$v.to_le_bytes());
                i += 2;
            };
        }
        macro_rules! w32 {
            ($v:expr) => {
                b[i..i + 4].copy_from_slice(&$v.to_le_bytes());
                i += 4;
            };
        }
        macro_rules! w8 {
            ($v:expr) => {
                b[i] = $v;
                i += 1;
            };
        }

        // Stats (8 bytes).
        w16!(self.hunger);
        w16!(self.tired);
        w16!(self.sick);
        w16!(self.miserable);
        // Weight (2 bytes).
        w16!(self.weight);
        // Drunk (2 bytes).
        w16!(self.drunk);
        // Traits (6 bytes).
        w16!(self.vitality);
        w16!(self.curiosity);
        w16!(self.resilience);
        // Timing (8 bytes).
        w32!(self.last_update_tick);
        w32!(self.age_ticks);
        // Lifecycle (9 bytes).
        w8!(self.phase as u8);
        w16!(self.hatching_countdown);
        w32!(self.leaving_countdown);
        w16!(self.generation);
        // Action state (10 bytes).
        w8!(self.active_action.map_or(0xFF, Action::to_u8));
        w8!(self.action_ticks_remaining);
        w16!(self.cooldown_feed);
        w16!(self.cooldown_heal);
        w16!(self.cooldown_relax);
        w16!(self.cooldown_play);
        w16!(self.cooldown_exercise);
        w16!(self.cooldown_medicate);
        w16!(self.cooldown_ozempic);
        w16!(self.cooldown_drink);
        w16!(self.cooldown_rehab);
        // Interval counters (8 bytes).
        w32!(self.miserable_interval_counter);
        w32!(self.tired_passive_counter);
        // Overweight/drunk duration counters (8 bytes).
        w32!(self.overweight_ticks);
        w32!(self.drunk_ticks);
        // Flags (4 bytes).
        w8!(self.is_sleeping as u8);
        w8!(self.hibernating as u8);
        w8!(self.diabetic as u8);
        w8!(self.alcoholic as u8);
        // Hibernation (4 bytes).
        w32!(self.hibernate_ticks);
        // Save tick (4 bytes).
        w32!(self.last_save_tick);
        // Pet kind (1 byte).
        w8!(self.pet_kind.0);
        // Battle record + cooldown (6 bytes).
        w16!(self.wins);
        w16!(self.losses);
        w16!(self.cooldown_battle);
        // HEX money (5 bytes).
        w32!(self.money);
        w8!(self.money_enabled as u8);
        // Hard mode (1 byte).
        w8!(self.hard_mode as u8);
        // Total: 97 bytes (1 version + 96 fields).
        b
    }

    /// Deserialize a game state from a byte buffer.
    /// Returns `None` if the buffer is too short.
    #[allow(unused_assignments)]
    pub fn from_bytes(b: &[u8]) -> Option<Self> {
        // Accept only current-version, current-size saves. Any other blob —
        // a pre-pet_kind (65-byte) save, or a same-size but differently-laid-
        // out save from another firmware (e.g. the pre-money 96-byte layout
        // that dropped `drained`) — fails the version+length guard and is
        // treated as "no save" so the player starts fresh, rather than being
        // silently reinterpreted with the wrong field offsets.
        if b.len() != SAVE_SIZE || b[0] != SAVE_FORMAT_VERSION {
            return None;
        }
        let mut i = 1;

        macro_rules! r16 {
            () => {{
                let v = u16::from_le_bytes([b[i], b[i + 1]]);
                i += 2;
                v
            }};
        }
        macro_rules! r32 {
            () => {{
                let v = u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
                i += 4;
                v
            }};
        }
        macro_rules! r8 {
            () => {{
                let v = b[i];
                i += 1;
                v
            }};
        }

        let hunger = r16!();
        let tired = r16!();
        let sick = r16!();
        let miserable = r16!();
        let weight = r16!();
        let drunk = r16!();
        let vitality = r16!();
        let curiosity = r16!();
        let resilience = r16!();
        let last_update_tick = r32!();
        let age_ticks = r32!();
        let phase = match r8!() {
            0 => Phase::Hatching,
            1 => Phase::Active,
            2 => Phase::Leaving,
            _ => Phase::Gone,
        };
        let hatching_countdown = r16!();
        let leaving_countdown = r32!();
        let generation = r16!();
        let action_byte = r8!();
        let active_action = Action::from_u8(action_byte);
        let action_ticks_remaining = r8!();
        let cooldown_feed = r16!();
        let cooldown_heal = r16!();
        let cooldown_relax = r16!();
        let cooldown_play = r16!();
        let cooldown_exercise = r16!();
        let cooldown_medicate = r16!();
        let cooldown_ozempic = r16!();
        let cooldown_drink = r16!();
        let cooldown_rehab = r16!();
        let miserable_interval_counter = r32!();
        let tired_passive_counter = r32!();
        let overweight_ticks = r32!();
        let drunk_ticks = r32!();
        let is_sleeping = r8!() != 0;
        let hibernating = r8!() != 0;
        let diabetic = r8!() != 0;
        let alcoholic = r8!() != 0;
        let hibernate_ticks = r32!();
        let last_save_tick = r32!();
        let pet_kind = PetKind::from_u8(r8!());
        let wins = r16!();
        let losses = r16!();
        let cooldown_battle = r16!();
        let money = r32!();
        let money_enabled = r8!() != 0;
        let hard_mode = r8!() != 0;

        Some(Self {
            pet_kind,
            hunger,
            tired,
            sick,
            miserable,
            weight,
            drunk,
            diabetic,
            overweight_ticks,
            alcoholic,
            drunk_ticks,
            vitality,
            curiosity,
            resilience,
            last_update_tick,
            age_ticks,
            phase,
            hatching_countdown,
            leaving_countdown,
            generation,
            wins,
            losses,
            money,
            money_enabled,
            hard_mode,
            active_action,
            // Not persisted — see the field doc on `active_food`.
            active_food: None,
            active_drink: None,
            action_ticks_remaining,
            cooldown_feed,
            cooldown_heal,
            cooldown_relax,
            cooldown_play,
            cooldown_exercise,
            cooldown_medicate,
            cooldown_ozempic,
            cooldown_drink,
            cooldown_rehab,
            cooldown_battle,
            // Not persisted: rebooting clears all mini-game cooldowns.
            cooldown_tictactoe: 0,
            cooldown_lightsout: 0,
            cooldown_blackhole: 0,
            cooldown_nim: 0,
            cooldown_bornjeweled: 0,
            // Not persisted — see the field doc on `cooldown_onlypets`.
            cooldown_onlypets: 0,
            miserable_interval_counter,
            tired_passive_counter,
            is_sleeping,
            hibernating,
            hibernate_ticks,
            last_save_tick,
            save_pending: false,
            realm_pending: false,
            naming_pending: false,
        })
    }
}

// ---------------------------------------------------------------------------
// Unicorn Realm — past pet records
// ---------------------------------------------------------------------------

/// Maximum length of a pet name in bytes.
pub const PET_NAME_MAX: usize = 12;

/// Compact record of a past pet for the Unicorn Realm.
#[derive(Clone, Copy)]
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
            PetKind::Bartholomeus
        } else {
            PetKind::Bartholomeus
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
        // name_len comes from flash and could be corrupt/> the array; clamp so
        // the slice can't go out of bounds (would panic when the realm opens).
        let n = (self.name_len as usize).min(PET_NAME_MAX);
        core::str::from_utf8(&self.name[..n]).unwrap_or("")
    }

    /// Format age as "Xd Xh".
    pub fn age_str(&self) -> heapless::String<12> {
        let hours = self.age_ticks / 360;
        let days = hours / 24;
        let mut s = heapless::String::new();
        let _ = core::fmt::Write::write_fmt(&mut s, format_args!("{}d {}h", days, hours % 24));
        s
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
                pet_kind: PetKind::Bartholomeus,
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

    /// New fields round-trip through `to_bytes`/`from_bytes` at the
    /// versioned 97-byte `SAVE_SIZE` (1 version byte + 96 fields).
    #[test]
    fn save_round_trip_includes_new_fields() {
        let mut state = GameState::new_egg(42, PetKind::Cat);
        state.weight = 41000;
        state.diabetic = true;
        state.cooldown_exercise = 12;
        state.cooldown_medicate = 345;
        state.wins = 3;
        state.losses = 1;
        state.cooldown_battle = 77;
        state.money = 12345;
        state.money_enabled = false;
        state.hard_mode = true;

        let bytes = state.to_bytes();
        assert_eq!(bytes.len(), SAVE_SIZE);
        assert_eq!(SAVE_SIZE, 97);
        assert_eq!(bytes[0], SAVE_FORMAT_VERSION);

        let restored = GameState::from_bytes(&bytes).expect("valid save should parse");
        assert_eq!(restored.weight, 41000);
        assert!(restored.diabetic);
        assert_eq!(restored.cooldown_exercise, 12);
        assert_eq!(restored.cooldown_medicate, 345);
        assert_eq!(restored.wins, 3);
        assert_eq!(restored.losses, 1);
        assert_eq!(restored.cooldown_battle, 77);
        assert_eq!(restored.pet_kind.id(), PetKind::Cat.id());
        assert_eq!(restored.money, 12345);
        assert!(!restored.money_enabled);
        assert!(restored.hard_mode);
    }

    /// A save blob with a different layout/version is rejected (→ fresh egg)
    /// rather than silently misparsed. Regression guard for the pre-money
    /// 96-byte layout (which dropped `drained` and kept SAVE_SIZE at 96)
    /// being reinterpreted with the new field offsets and corrupting the pet.
    #[test]
    fn old_or_wrong_version_save_is_rejected() {
        // A valid current save round-trips.
        let good = GameState::new_egg(7, PetKind::Slug).to_bytes();
        assert!(GameState::from_bytes(&good).is_some());

        // Old-layout blob one byte short of the versioned size — rejected on length.
        let old = [0xABu8; SAVE_SIZE - 1];
        assert!(GameState::from_bytes(&old).is_none());

        // Right size but wrong version byte — rejected on version.
        let mut wrong_ver = good;
        wrong_ver[0] = SAVE_FORMAT_VERSION.wrapping_add(1);
        assert!(GameState::from_bytes(&wrong_ver).is_none());
    }

    /// A fresh egg starts with the Stage-1 HEX default: 100 balance, money
    /// mode enabled.
    #[test]
    fn new_egg_starts_with_100_hex_and_money_enabled() {
        let state = GameState::new_egg(1, PetKind::Bartholomeus);
        assert_eq!(state.money, 100);
        assert!(state.money_enabled);
    }

    /// `add_money` saturates at `u32::MAX` instead of overflowing/panicking,
    /// and every credit flags the state for an immediate save (so the new
    /// balance survives a reboot right after being earned rather than
    /// waiting for the next 15-minute save interval).
    #[test]
    fn add_money_saturates_and_flags_save() {
        let mut state = GameState::new_egg(2, PetKind::Bartholomeus);
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
        let mut state = GameState::new_egg(3, PetKind::Bartholomeus);
        state.money = 50;
        assert!(state.spend_money(30));
        assert_eq!(state.money, 20);
    }

    /// Spending fails and leaves the balance unchanged when the pet can't
    /// afford it — money should never go negative (or wrap, in u32 terms).
    #[test]
    fn spend_money_fails_when_too_poor() {
        let mut state = GameState::new_egg(4, PetKind::Bartholomeus);
        state.money = 10;
        assert!(!state.spend_money(30));
        assert_eq!(state.money, 10);
    }

    /// `can_afford` reports the exact threshold: affordable at the price,
    /// not affordable one HEX above it.
    #[test]
    fn can_afford_reports_threshold() {
        let mut state = GameState::new_egg(5, PetKind::Bartholomeus);
        state.money = 20;
        assert!(state.can_afford(20));
        assert!(!state.can_afford(21));
    }

    /// Every `Action` variant must round-trip through the persisted byte
    /// exactly. Regression test for a real bug: `to_bytes` wrote the
    /// discriminant via a bare `as u8` cast, but `from_bytes`'s
    /// hand-written match only recognized the first four discriminants
    /// (0-3, at the time Feed/Heal/Relax/Play) — Exercise/Medicate/
    /// Ozempic/Drink/Rehab (4-8) all silently came back as `None`,
    /// discarding an in-progress action (and, for Drink specifically,
    /// dropping `active_drink` context too, so the remainder of the
    /// action would have applied under the wrong drink's multipliers
    /// had the match not dropped the action entirely first). `2`
    /// (formerly `Relax`) is now a deliberate gap — see `Action::to_u8`.
    #[test]
    fn every_action_round_trips_through_save() {
        let all = [
            Action::Feed,
            Action::Heal,
            Action::Play,
            Action::Exercise,
            Action::Medicate,
            Action::Ozempic,
            Action::Drink,
            Action::Rehab,
            Action::OnlyPets,
        ];
        for action in all {
            let mut state = GameState::new_egg(5, PetKind::Bartholomeus);
            state.active_action = Some(action);
            state.action_ticks_remaining = 3;

            let bytes = state.to_bytes();
            let restored = GameState::from_bytes(&bytes).expect("valid save should parse");
            assert_eq!(
                restored.active_action,
                Some(action),
                "{action:?} did not survive a save/restore round-trip"
            );
            assert_eq!(restored.action_ticks_remaining, 3);
        }

        // No action in progress still round-trips as None, not some
        // stray Some(_).
        let mut state = GameState::new_egg(6, PetKind::Bartholomeus);
        state.active_action = None;
        let restored = GameState::from_bytes(&state.to_bytes()).unwrap();
        assert_eq!(restored.active_action, None);
    }

    /// `record_battle` bumps the right counter and always sets the
    /// cooldown, regardless of win/loss.
    #[test]
    fn record_battle_updates_counters_and_cooldown() {
        let mut state = GameState::new_egg(1, PetKind::Bartholomeus);
        assert_eq!(state.wins, 0);
        assert_eq!(state.losses, 0);

        state.record_battle(true);
        assert_eq!(state.wins, 1);
        assert_eq!(state.losses, 0);
        assert_eq!(state.cooldown_battle, BATTLE_COOLDOWN());

        state.cooldown_battle = 0;
        state.record_battle(false);
        assert_eq!(state.wins, 1);
        assert_eq!(state.losses, 1);
        assert_eq!(state.cooldown_battle, BATTLE_COOLDOWN());
    }

    /// Sustained overweight for `DIABETES_ONSET_TICKS()` flips `diabetic`
    /// permanently true; dropping weight back down afterward does not
    /// reverse it.  Exercises `check_diabetes()` directly (it's a private
    /// method, reachable from this same-file test module) so the test
    /// isolates the weight/diabetes mechanic from the unrelated
    /// hunger/tired/leaving lifecycle — driving thousands of ticks
    /// through the full `update()` with hunger/tired left unattended
    /// would let the *existing* neglect mechanics kill the pet
    /// (`Phase::Gone`) long before the multi-day diabetes window elapses.
    #[test]
    fn sustained_overweight_triggers_permanent_diabetes() {
        let mut state = GameState::new_egg(1, PetKind::Bartholomeus);
        state.weight = OVERWEIGHT_TRIGGER() + 1;

        state.check_diabetes(DIABETES_ONSET_TICKS() + 10);
        assert!(state.diabetic, "should become diabetic after sustained overweight");

        // Dropping weight back down afterward must not clear the flag.
        state.weight = 0;
        state.check_diabetes(10);
        assert!(state.diabetic, "diabetes should be permanent");
    }

    /// Overweight time resets if weight drops back below the trigger
    /// before the onset threshold is reached — no premature diabetes.
    #[test]
    fn overweight_progress_resets_on_recovery() {
        let mut state = GameState::new_egg(2, PetKind::Bartholomeus);
        state.weight = OVERWEIGHT_TRIGGER() + 1;

        // Advance partway toward onset, then recover.
        state.check_diabetes(DIABETES_ONSET_TICKS() / 2);
        state.weight = 0;
        state.check_diabetes(1);
        assert!(!state.diabetic);

        // Even after further time at low weight, no diabetes should appear.
        state.check_diabetes(DIABETES_ONSET_TICKS());
        assert!(!state.diabetic, "recovering before onset should reset progress");
    }

    /// Pizza should pack on far more weight than Salad for feeding the
    /// same duration — the whole point of the food system tying into
    /// the overweight/diabetes mechanic.
    #[test]
    fn unhealthy_food_gains_more_weight_than_healthy_food() {
        let run = |food: FoodKind| -> u16 {
            let mut state = GameState::new_egg(7, PetKind::Bartholomeus);
            state.update(HATCHING_TICKS() as u32);
            assert!(state.feed(food));
            state.update(state.last_update_tick + FEED_DURATION() as u32);
            state.weight
        };

        let salad_weight = run(FoodKind::Salad);
        let apple_weight = run(FoodKind::Apple);
        let pizza_weight = run(FoodKind::Pizza);
        let cake_weight = run(FoodKind::Cake);

        assert!(
            salad_weight < apple_weight,
            "Salad ({salad_weight}) should gain less weight than Apple ({apple_weight})"
        );
        assert!(
            apple_weight < pizza_weight,
            "Apple ({apple_weight}) should gain less weight than Pizza ({pizza_weight})"
        );
        assert!(
            pizza_weight < cake_weight,
            "Pizza ({pizza_weight}) should gain less weight than Cake ({cake_weight})"
        );
    }

    /// Food choice must not change hunger relief ordering in the wrong
    /// direction — Cake is the worst hunger relief (dessert, not
    /// filling), Pizza the best (very filling).
    #[test]
    fn cake_is_least_filling_pizza_is_most_filling() {
        let run = |food: FoodKind| -> u16 {
            let mut state = GameState::new_egg(9, PetKind::Bartholomeus);
            state.update(HATCHING_TICKS() as u32);
            state.hunger = HUNGER_RATE().saturating_mul(10_000); // build up hunger first
            assert!(state.feed(food));
            state.update(state.last_update_tick + FEED_DURATION() as u32);
            state.hunger
        };

        let cake_hunger = run(FoodKind::Cake);
        let pizza_hunger = run(FoodKind::Pizza);
        assert!(
            pizza_hunger < cake_hunger,
            "Pizza should relieve more hunger than Cake (pizza={pizza_hunger}, cake={cake_hunger})"
        );
    }

    /// Water and Cola shouldn't move `drunk` at all; Whiskey should move
    /// it the most, Beer less. Ties the drink system to the same
    /// weight/diabetes-style permanent-condition mechanic.
    #[test]
    fn alcoholic_drinks_raise_drunk_non_alcoholic_dont() {
        let run = |drink: DrinkKind| -> u16 {
            let mut state = GameState::new_egg(11, PetKind::Bartholomeus);
            state.update(HATCHING_TICKS() as u32);
            assert!(state.drink(drink));
            state.update(state.last_update_tick + DRINK_DURATION() as u32);
            state.drunk
        };

        let water_drunk = run(DrinkKind::Water);
        let cola_drunk = run(DrinkKind::Cola);
        let beer_drunk = run(DrinkKind::Beer);
        let whiskey_drunk = run(DrinkKind::Whiskey);

        assert_eq!(water_drunk, 0, "Water should never raise drunk");
        assert_eq!(cola_drunk, 0, "Cola should never raise drunk");
        assert!(
            beer_drunk < whiskey_drunk,
            "Beer ({beer_drunk}) should raise drunk less than Whiskey ({whiskey_drunk})"
        );
        assert!(whiskey_drunk > 0, "Whiskey should raise drunk");
    }

    /// Sustained drunkenness for `ALCOHOLIC_ONSET_TICKS()` flips
    /// `alcoholic` permanently true — mirrors the diabetes onset test.
    #[test]
    fn sustained_drunk_triggers_permanent_alcoholism() {
        let mut state = GameState::new_egg(13, PetKind::Bartholomeus);
        state.drunk = DRUNK_TRIGGER() + 1;

        state.check_alcoholism(ALCOHOLIC_ONSET_TICKS() + 10);
        assert!(state.alcoholic, "should become alcoholic after sustained drunkenness");

        state.drunk = 0;
        state.check_alcoholism(10);
        assert!(state.alcoholic, "alcoholism should be permanent");
    }

    /// `friend_boost(true)` used to compute `STAT_MAX() * 2 / 5` with the
    /// multiply in u16 space — since STAT_MAX() is 65535, that overflows
    /// before the divide ever runs (panics under overflow-checks, silently
    /// wraps to roughly half the intended ~40% otherwise). Meeting a brand
    /// new mesh friend must not panic and must actually apply a bigger cut
    /// than the "already-known friend" ~10% boost.
    #[test]
    fn friend_boost_does_not_overflow_and_scales_with_big() {
        let mut state = GameState::new_egg(17, PetKind::Bartholomeus);
        state.miserable = STAT_MAX();

        state.friend_boost(true);
        let after_big = state.miserable;
        assert!(
            after_big < STAT_MAX(),
            "a big friend boost should reduce miserable"
        );

        state.miserable = STAT_MAX();
        state.friend_boost(false);
        let after_small = state.miserable;

        let big_drop = STAT_MAX() - after_big;
        let small_drop = STAT_MAX() - after_small;
        assert!(
            big_drop > small_drop,
            "meeting a new friend (big={big_drop}) should relieve more misery than an already-known one (small={small_drop})"
        );
    }

    /// `ticks_to_next_rate_change()`'s sick-boundary estimate used to omit
    /// the diabetes/alcoholism penalty entirely, treating a diabetic pet's
    /// `sick` growth as if it were only the ~1/tick base rate. That made
    /// the piecewise update loop size a segment far larger than reality,
    /// so `check_leaving()` could charge the whole oversized segment as
    /// "maxed" in one shot — enough to jump straight from Active to Gone
    /// on a long fast-forward (e.g. Skip 1 day), skipping Leaving
    /// entirely. With the penalty folded in, the boundary should be close
    /// (tens to low hundreds of ticks), not tens of thousands.
    #[test]
    fn sick_rate_estimate_includes_diabetes_penalty() {
        let mut state = GameState::new_egg(23, PetKind::Bartholomeus);
        state.update(HATCHING_TICKS() as u32);
        state.diabetic = true;
        state.cooldown_medicate = 0;
        state.active_action = None;
        state.cooldown_heal = 0;
        state.sick = 0;
        // Keep every other boundary far away so sick's own crossing is the
        // one this test's estimate is exercising.
        state.hunger = 0;
        state.tired = 0;
        state.cooldown_feed = 0;
        state.cooldown_relax = 0;
        state.cooldown_play = 0;

        let ticks = state.ticks_to_next_rate_change();
        assert!(
            ticks < 10_000,
            "sick boundary estimate ({ticks} ticks) should reflect the diabetes penalty rate, not just the ~1/tick base rate"
        );
    }

    /// `next_wake_tick()` must account for every cooldown that can gate an
    /// action, including Battle's — otherwise the scheduler never wakes the
    /// CPU specifically for a battle cooldown expiring (it self-corrects on
    /// the next unrelated wake, but the omission was a real gap relative to
    /// every other action cooldown this function tracks).
    #[test]
    fn next_wake_tick_accounts_for_battle_cooldown() {
        let mut state = GameState::new_egg(19, PetKind::Bartholomeus);
        state.update(HATCHING_TICKS() as u32);
        state.last_update_tick = 1000;
        state.cooldown_battle = 42;

        // No other cooldown/action pending shorter than this, so the
        // battle cooldown should be the binding constraint.
        let wake = state.next_wake_tick();
        assert!(
            wake <= 1000 + 42,
            "next_wake_tick ({wake}) should wake no later than the battle cooldown expiring at 1042"
        );
    }

    // ── Stage 2: "user can make money" ──────────────────────────────────

    /// Winning a mini-game credits `MINIGAME_HEX_REWARD` HEX when money
    /// mode is on, and grants nothing (but still runs the existing
    /// cooldown/hunger-cost path) when money mode is off.
    #[test]
    fn minigame_win_awards_hex_when_money_enabled() {
        let mut state = GameState::new_egg(30, PetKind::Bartholomeus);
        state.update(HATCHING_TICKS() as u32);
        state.money = 0;
        state.money_enabled = true;

        state.award_inspiration(MiniGame::Nim);
        assert_eq!(state.money, MINIGAME_HEX_REWARD);

        state.money = 0;
        state.money_enabled = false;
        state.cooldown_nim = 0;
        state.award_inspiration(MiniGame::Nim);
        assert_eq!(state.money, 0, "money off should grant no HEX");
    }

    /// A mesh Battle win credits `BATTLE_HEX_REWARD` HEX; a loss grants
    /// nothing. With money mode off, even a win grants nothing.
    #[test]
    fn battle_win_awards_hex_winner_only() {
        let mut state = GameState::new_egg(31, PetKind::Bartholomeus);
        state.money_enabled = true;
        state.money = 0;

        state.record_battle(true);
        assert_eq!(state.money, BATTLE_HEX_REWARD);

        state.money = 0;
        state.record_battle(false);
        assert_eq!(state.money, 0, "a loss should not award HEX");

        state.money = 0;
        state.money_enabled = false;
        state.record_battle(true);
        assert_eq!(state.money, 0, "money off should grant no HEX even on a win");
    }

    /// `only_pets()` starts the action like `play()`; a second call while
    /// the action is still in progress is rejected.
    #[test]
    fn only_pets_starts_action() {
        let mut state = GameState::new_egg(32, PetKind::Bartholomeus);
        state.update(HATCHING_TICKS() as u32);

        assert!(state.only_pets());
        assert_eq!(state.active_action, Some(Action::OnlyPets));
        assert!(!state.only_pets(), "action already in progress");
    }

    /// Driving the Only-pets action to completion (via the same public
    /// `update()` path real time would use — mirrors how Feed/Drink
    /// completion is exercised elsewhere in this test module), starting
    /// above `BROKE_THRESHOLD`, awards the "hobby" branch: +HEX and a
    /// happiness bump, plus its own cooldown.
    #[test]
    fn only_pets_hobby_completion_awards_hex_and_happiness() {
        let mut state = GameState::new_egg(33, PetKind::Bartholomeus);
        state.update(HATCHING_TICKS() as u32);
        state.money_enabled = true;
        state.money = 50; // >= BROKE_THRESHOLD, so this exercises the hobby branch.
        state.miserable = STAT_MAX();

        assert!(state.only_pets());
        state.update(state.last_update_tick + PLAY_DURATION() as u32);

        assert_eq!(state.money, 50 + ONLYPETS_HOBBY_REWARD);
        assert_eq!(state.miserable, STAT_MAX() - HAPPINESS_STEP);
        assert!(state.cooldown_onlypets > 0);
        assert_eq!(state.active_action, None);
    }

    /// Stage 5: below `BROKE_THRESHOLD`, Only-pets completion forces the
    /// "broke" branch instead — big pay, but happiness *drops* rather than
    /// rises.
    #[test]
    fn only_pets_broke_branch_pays_100_and_drops_happiness() {
        let mut state = GameState::new_egg(34, PetKind::Bartholomeus);
        state.update(HATCHING_TICKS() as u32);
        state.money_enabled = true;
        state.money = 5; // < BROKE_THRESHOLD
        state.miserable = 1000;

        assert!(state.only_pets());
        state.update(state.last_update_tick + PLAY_DURATION() as u32);

        assert_eq!(state.money, 5 + ONLYPETS_BROKE_REWARD);
        assert_eq!(state.miserable, 1000 + HAPPINESS_STEP);
        assert!(state.cooldown_onlypets > 0);
        assert_eq!(state.active_action, None);
    }

    // ── Stage 3: "things cost money" ────────────────────────────────────

    /// Feeding charges by the food's health tier — healthy (Salad) costs
    /// more than unhealthy (Burger), mirroring `FoodKind::hex_price`.
    #[test]
    fn feed_charges_by_food_health() {
        let mut state = GameState::new_egg(40, PetKind::Bartholomeus);
        state.update(HATCHING_TICKS() as u32);
        state.money_enabled = true;
        state.money = 100;

        assert!(state.feed(FoodKind::Salad));
        assert_eq!(state.money, 85, "healthy food (Salad) should cost 15 HEX");

        // Fresh egg so the cooldown from the first feed doesn't reject this one.
        let mut state = GameState::new_egg(41, PetKind::Bartholomeus);
        state.update(HATCHING_TICKS() as u32);
        state.money_enabled = true;
        state.money = 100;

        assert!(state.feed(FoodKind::Frikandel));
        assert_eq!(state.money, 90, "unhealthy food (Frikandel) should cost 10 HEX");
    }

    /// Drinking charges by the drink's health tier — healthy (Water) costs
    /// more than unhealthy (Cola), mirroring `DrinkKind::hex_price`.
    #[test]
    fn drink_charges_by_health() {
        let mut state = GameState::new_egg(42, PetKind::Bartholomeus);
        state.update(HATCHING_TICKS() as u32);
        state.money_enabled = true;
        state.money = 100;

        assert!(state.drink(DrinkKind::Water));
        assert_eq!(state.money, 85, "healthy drink (Water) should cost 15 HEX");

        let mut state = GameState::new_egg(43, PetKind::Bartholomeus);
        state.update(HATCHING_TICKS() as u32);
        state.money_enabled = true;
        state.money = 100;

        assert!(state.drink(DrinkKind::Cola));
        assert_eq!(state.money, 90, "unhealthy drink (Cola) should cost 10 HEX");
    }

    /// The basic Play action costs a flat `PLAY_HEX_COST` (10 HEX).
    #[test]
    fn play_costs_10() {
        let mut state = GameState::new_egg(44, PetKind::Bartholomeus);
        state.update(HATCHING_TICKS() as u32);
        state.money_enabled = true;
        state.money = 100;

        assert!(state.play());
        assert_eq!(state.money, 90);
    }

    /// Aspirine (the Heal action) costs a flat `ASPIRINE_HEX_COST` (1 HEX),
    /// and is rejected when the pet can't afford it.
    #[test]
    fn aspirine_costs_1_hex() {
        let mut state = GameState::new_egg(44, PetKind::Bartholomeus);
        state.update(HATCHING_TICKS() as u32);
        state.money_enabled = true;
        state.money = 100;
        assert!(state.heal());
        assert_eq!(state.money, 99);

        // Broke (0 HEX): heal is rejected, no charge, no action started.
        let mut broke = GameState::new_egg(45, PetKind::Bartholomeus);
        broke.update(HATCHING_TICKS() as u32);
        broke.money_enabled = true;
        broke.money = 0;
        assert!(!broke.heal());
        assert_eq!(broke.money, 0);
        assert_eq!(broke.active_action, None);
    }

    /// Each drug action (Ozempic, Medicate, Rehab) costs a flat
    /// `DRUG_HEX_COST` (15 HEX). Medicate/Rehab are gated on
    /// diabetic/alcoholic respectively, so those flags are set first.
    #[test]
    fn drug_costs_15() {
        let mut state = GameState::new_egg(45, PetKind::Bartholomeus);
        state.update(HATCHING_TICKS() as u32);
        state.money_enabled = true;
        state.money = 100;
        assert!(state.ozempic());
        assert_eq!(state.money, 85, "ozempic should cost 15 HEX");

        let mut state = GameState::new_egg(46, PetKind::Bartholomeus);
        state.update(HATCHING_TICKS() as u32);
        state.money_enabled = true;
        state.money = 100;
        state.diabetic = true;
        assert!(state.medicate());
        assert_eq!(state.money, 85, "medicate should cost 15 HEX");

        let mut state = GameState::new_egg(47, PetKind::Bartholomeus);
        state.update(HATCHING_TICKS() as u32);
        state.money_enabled = true;
        state.money = 100;
        state.alcoholic = true;
        assert!(state.rehab());
        assert_eq!(state.money, 85, "rehab should cost 15 HEX");
    }

    /// With `money_enabled = false`, priced actions proceed but charge
    /// nothing at all — the whole money layer stays inert.
    #[test]
    fn actions_free_when_money_disabled() {
        let mut state = GameState::new_egg(48, PetKind::Bartholomeus);
        state.update(HATCHING_TICKS() as u32);
        state.money_enabled = false;
        state.money = 100;

        assert!(state.feed(FoodKind::Salad));
        assert_eq!(state.money, 100, "feed should not charge when money is off");

        let mut state = GameState::new_egg(49, PetKind::Bartholomeus);
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
        let mut state = GameState::new_egg(50, PetKind::Bartholomeus);
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
        let mut state = GameState::new_egg(51, PetKind::Bartholomeus);
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
        let mut state = GameState::new_egg(60, PetKind::Bartholomeus);
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
        let mut state = GameState::new_egg(61, PetKind::Bartholomeus);
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
        let mut state = GameState::new_egg(62, PetKind::Bartholomeus);
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
        let mut state = GameState::new_egg(63, PetKind::Bartholomeus);
        state.update(HATCHING_TICKS() as u32);
        state.money_enabled = true;
        state.money = 0;

        assert!(state.only_pets());
    }

    // ── Hard (US) mode: pricier healthy food / medication / rehab ──────

    /// In Hard mode, healthy food (Salad) costs 20 HEX instead of the
    /// normal 15. Normal (non-hard) mode is unaffected — regression check.
    #[test]
    fn hard_mode_healthy_food_costs_20() {
        let mut state = GameState::new_egg(70, PetKind::Bartholomeus);
        state.update(HATCHING_TICKS() as u32);
        state.money_enabled = true;
        state.hard_mode = true;
        state.money = 100;

        assert!(state.feed(FoodKind::Salad));
        assert_eq!(state.money, 80, "hard-mode healthy food should cost 20 HEX");

        let mut normal = GameState::new_egg(71, PetKind::Bartholomeus);
        normal.update(HATCHING_TICKS() as u32);
        normal.money_enabled = true;
        normal.hard_mode = false;
        normal.money = 100;

        assert!(normal.feed(FoodKind::Salad));
        assert_eq!(normal.money, 85, "normal-mode healthy food should still cost 15 HEX");
    }

    /// In Hard mode, medication (Ozempic / Insulin) costs 45 HEX — 3x the
    /// normal 15. Normal mode is unaffected.
    #[test]
    fn hard_mode_medication_costs_45() {
        let mut state = GameState::new_egg(72, PetKind::Bartholomeus);
        state.update(HATCHING_TICKS() as u32);
        state.money_enabled = true;
        state.hard_mode = true;
        state.money = 100;

        assert!(state.ozempic());
        assert_eq!(state.money, 55, "hard-mode ozempic should cost 45 HEX");

        let mut normal = GameState::new_egg(73, PetKind::Bartholomeus);
        normal.update(HATCHING_TICKS() as u32);
        normal.money_enabled = true;
        normal.hard_mode = false;
        normal.money = 100;

        assert!(normal.ozempic());
        assert_eq!(normal.money, 85, "normal-mode ozempic should still cost 15 HEX");
    }

    /// In Hard mode, Rehab costs 75 HEX — 5x the normal 15. Normal mode
    /// is unaffected.
    #[test]
    fn hard_mode_rehab_costs_75() {
        let mut state = GameState::new_egg(74, PetKind::Bartholomeus);
        state.update(HATCHING_TICKS() as u32);
        state.money_enabled = true;
        state.hard_mode = true;
        state.alcoholic = true;
        state.money = 100;

        assert!(state.rehab());
        assert_eq!(state.money, 25, "hard-mode rehab should cost 75 HEX");

        let mut normal = GameState::new_egg(75, PetKind::Bartholomeus);
        normal.update(HATCHING_TICKS() as u32);
        normal.money_enabled = true;
        normal.hard_mode = false;
        normal.alcoholic = true;
        normal.money = 100;

        assert!(normal.rehab());
        assert_eq!(normal.money, 85, "normal-mode rehab should still cost 15 HEX");
    }

    /// Hard mode only raises healthy food / medication / rehab — unhealthy
    /// food and the flat Play cost are untouched.
    #[test]
    fn hard_mode_unhealthy_food_and_play_unchanged() {
        let mut state = GameState::new_egg(76, PetKind::Bartholomeus);
        state.update(HATCHING_TICKS() as u32);
        state.money_enabled = true;
        state.hard_mode = true;
        state.money = 100;

        assert!(state.feed(FoodKind::Frikandel));
        assert_eq!(state.money, 90, "hard mode should not touch unhealthy food price");

        let mut state = GameState::new_egg(77, PetKind::Bartholomeus);
        state.update(HATCHING_TICKS() as u32);
        state.money_enabled = true;
        state.hard_mode = true;
        state.money = 100;

        assert!(state.play());
        assert_eq!(state.money, 90, "hard mode should not touch the flat Play price");
    }
}
