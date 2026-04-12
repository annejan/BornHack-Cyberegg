//! Cyber Ægg game engine — delta-T progression with boundary-based wake scheduling.
//!
//! Instead of ticking every 10 seconds, the engine:
//! 1. Computes elapsed ticks since the last update.
//! 2. Applies all stat changes for that delta in one step.
//! 3. Computes the next boundary crossing time across all stats.
//! 4. Schedules a wake-up at the earliest boundary (or MAX_SLEEP_TICKS).
//!
//! This lets the CPU sleep for minutes or hours when nothing interesting
//! is about to happen, saving significant battery on the badge.

pub mod anim_files;
pub mod thresholds;
pub mod to_display;

use thresholds::*;
pub use to_display::DisplayAnim;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

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

/// Active user action (mutually exclusive).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "embassy-base", derive(defmt::Format))]
pub enum Action {
    Feed,
    Heal,
    Relax,
    Play,
}

/// The complete game state.  Serialisable to ekv for save/restore.
#[derive(Clone)]
pub struct GameState {
    // Primary stats (0 = best, STAT_MAX = worst).
    pub hunger: u16,
    pub tired: u16,
    pub drained: u16,
    pub sick: u16,
    pub miserable: u16,

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

    // Action state.
    pub active_action: Option<Action>,
    pub action_ticks_remaining: u8,
    pub cooldown_feed: u16,
    pub cooldown_heal: u16,
    pub cooldown_relax: u16,
    pub cooldown_play: u16,

    // Interval counters (track ticks since last interval fire).
    drained_interval_counter: u32,
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
}

// ---------------------------------------------------------------------------
// Construction
// ---------------------------------------------------------------------------

impl GameState {
    /// Create a new egg with randomised traits from a seed.
    pub fn new_egg(seed: u64) -> Self {
        // Simple xorshift64 for deterministic trait generation.
        let mut rng = seed;
        let mut next = || -> u16 {
            rng ^= rng << 13;
            rng ^= rng >> 7;
            rng ^= rng << 17;
            let range = (MAX_TRAIT - MIN_TRAIT) as u64;
            MIN_TRAIT + ((rng % range) as u16)
        };

        let vitality = next();
        let curiosity = next();
        let resilience = next();

        Self {
            hunger: 0,
            tired: 0,
            drained: 0,
            sick: (STAT_MAX - vitality) / 4,
            miserable: 0,

            vitality,
            curiosity,
            resilience,

            last_update_tick: 0,
            age_ticks: 0,

            phase: Phase::Hatching,
            hatching_countdown: HATCHING_TICKS,
            leaving_countdown: 0,
            generation: 0,

            active_action: None,
            action_ticks_remaining: 0,
            cooldown_feed: 0,
            cooldown_heal: 0,
            cooldown_relax: 0,
            cooldown_play: 0,

            drained_interval_counter: 0,
            miserable_interval_counter: 0,
            tired_passive_counter: 0,

            is_sleeping: false,
            hibernating: false,
            hibernate_ticks: 0,
            last_save_tick: 0,
        }
    }

    /// Start a new generation (pet left, hatch new egg).
    pub fn new_generation(&mut self, seed: u64) {
        let next_gen = self.generation + 1;
        *self = Self::new_egg(seed);
        self.generation = next_gen;
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Saturating add for u16 stats (capped at STAT_MAX).
fn sat_add(val: u16, delta: u16) -> u16 {
    val.saturating_add(delta).min(STAT_MAX)
}

/// Saturating sub for u16 stats (floored at 0).
fn sat_sub(val: u16, delta: u16) -> u16 {
    val.saturating_sub(delta)
}

/// Multiply rate × delta in u32 space, capped to u16 range.
/// This is the `y += m * dt` step — safe for large deltas.
/// Takes dt as u32 to avoid truncation on large piecewise segments.
fn mul_dt(rate: u16, dt: u32) -> u16 {
    (rate as u32 * dt).min(STAT_MAX as u32) as u16
}

/// How many times an interval fires in `delta` ticks, given a counter
/// that has already accumulated `counter` ticks since the last fire.
/// Returns (fire_count, new_counter).
fn interval_fires(delta: u32, counter: u32, interval: u32) -> (u32, u32) {
    let total = counter + delta;
    let fires = total / interval;
    let new_counter = total % interval;
    (fires, new_counter)
}

/// Count how many of the four primary stats exceed the 60% threshold.
fn count_above_60(state: &GameState) -> u32 {
    let t = MISERABLE_STAT_THRESHOLD;
    (state.hunger > t) as u32
        + (state.tired > t) as u32
        + (state.drained > t) as u32
        + (state.sick > t) as u32
}

/// Check if any stat triggers sick condition decay.
fn sick_condition_active(state: &GameState) -> bool {
    state.hunger > SICK_TRIGGER_HUNGER
        || state.tired > SICK_TRIGGER_TIRED
        || state.drained > SICK_TRIGGER_DRAINED
}

/// Curiosity modifier for play costs: 0–10 range, higher = cheaper.
fn curiosity_modifier(curiosity: u16) -> u16 {
    (curiosity as u32 * 10 / STAT_MAX as u32) as u16
}

/// Count of maxed stats (= STAT_MAX).
fn count_maxed(state: &GameState) -> usize {
    (state.hunger >= STAT_MAX) as usize
        + (state.tired >= STAT_MAX) as usize
        + (state.drained >= STAT_MAX) as usize
        + (state.sick >= STAT_MAX) as usize
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
        if total_delta == 0 { return; }
        self.last_update_tick = now_tick;

        // Hibernation: time stands still.  Track hibernated time but
        // don't advance age or any game state.
        if self.hibernating {
            self.hibernate_ticks += total_delta;
            return;
        }

        self.age_ticks += total_delta;

        match self.phase {
            Phase::Gone => return,
            Phase::Hatching => {
                let consumed = total_delta.min(self.hatching_countdown as u32);
                self.hatching_countdown -= consumed as u16;
                if self.hatching_countdown == 0 {
                    self.phase = Phase::Active;
                }
                return;
            }
            Phase::Leaving | Phase::Active => {}
        }

        let mut remaining = total_delta;

        // Consume action ticks first (these are short, ≤ 4 ticks).
        remaining = self.consume_action_ticks(remaining);

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
            remaining -= segment;
        }
    }

    /// Ticks until a threshold crossing changes the rate equation.
    ///
    /// Every boundary where a stat's rate (or another stat's rate that
    /// depends on it) changes is checked.  Returns the minimum across all.
    fn ticks_to_next_rate_change(&self) -> u32 {
        let mut m = u32::MAX;
        let miserable_high = self.miserable >= MISERABLE_BOOST_THRESHOLD;

        // Helper: ticks for a linearly-increasing stat to reach `target`.
        let ticks_up = |val: u16, target: u16, rate: u16| -> u32 {
            if val >= target || rate == 0 { return u32::MAX; }
            (target - val) as u32 / rate as u32
        };

        // Helper: ticks for a linearly-decreasing stat to reach `target`.
        let ticks_down = |val: u16, target: u16, rate: u16| -> u32 {
            if val <= target || rate == 0 { return u32::MAX; }
            (val - target) as u32 / rate as u32
        };

        // Helper: ticks for an interval-based stat to reach `target`.
        let ticks_interval = |val: u16, target: u16, amount: u16, interval: u32| -> u32 {
            if val >= target || amount == 0 { return u32::MAX; }
            let fires = (target - val) as u32 / amount as u32;
            fires.saturating_mul(interval)
        };

        // Current hunger rate.
        let hunger_rate = if self.cooldown_feed > 0 { 0 }
            else { HUNGER_RATE + if miserable_high { HUNGER_MISERABLE_BOOST } else { 0 } };

        // Current tired rate (never suppressed).
        let tired_rate = TIRED_RATE + if miserable_high { TIRED_MISERABLE_BOOST } else { 0 };

        // Current drained interval.
        let drained_interval = if self.cooldown_relax > 0 { u32::MAX }
            else if self.miserable >= MISERABLE_DRAIN_THRESHOLD { DRAINED_INTERVAL_MISERABLE }
            else { DRAINED_INTERVAL };

        // Current miserable interval.
        let mis_interval = if self.cooldown_play > 0 { u32::MAX }
            else {
                let above = count_above_60(self);
                MISERABLE_INTERVAL_BASE
                    .saturating_sub(MISERABLE_INTERVAL_PER_STAT * above)
                    .max(MISERABLE_INTERVAL_MIN)
            };

        // ── Boundaries that change the miserable interval (count_above_60) ──

        // Each primary stat crossing 60% changes the miserable decay rate.
        let t60 = MISERABLE_STAT_THRESHOLD;
        m = m.min(ticks_up(self.hunger, t60, hunger_rate));
        m = m.min(ticks_up(self.tired, t60, tired_rate));
        m = m.min(ticks_interval(self.drained, t60, DRAINED_AMOUNT, drained_interval));
        // sick rate is complex (base + condition); use base rate as lower bound.
        let sick_rate_approx = SICK_RATE + if sick_condition_active(self) {
            if miserable_high { SICK_CONDITION_MISERABLE_RATE } else { SICK_CONDITION_RATE }
        } else { 0 };
        m = m.min(ticks_up(self.sick, t60, sick_rate_approx));

        // ── Boundaries that change sick condition decay ──

        m = m.min(ticks_up(self.hunger, SICK_TRIGGER_HUNGER, hunger_rate));
        m = m.min(ticks_up(self.tired, SICK_TRIGGER_TIRED, tired_rate));
        m = m.min(ticks_interval(self.drained, SICK_TRIGGER_DRAINED, DRAINED_AMOUNT, drained_interval));

        // ── Miserable thresholds (change hunger/tired/drained rates) ──

        m = m.min(ticks_interval(self.miserable, MISERABLE_BOOST_THRESHOLD, MISERABLE_AMOUNT, mis_interval));
        m = m.min(ticks_interval(self.miserable, MISERABLE_DRAIN_THRESHOLD, MISERABLE_AMOUNT, mis_interval));

        // ── Stats reaching STAT_MAX (changes leaving behavior) ──

        m = m.min(ticks_up(self.hunger, STAT_MAX, hunger_rate));
        m = m.min(ticks_up(self.tired, STAT_MAX, tired_rate));
        m = m.min(ticks_up(self.sick, STAT_MAX, sick_rate_approx));
        m = m.min(ticks_interval(self.drained, STAT_MAX, DRAINED_AMOUNT, drained_interval));

        // ── Cooldown expiry (suppression ends → rate resumes) ──

        if self.cooldown_feed > 0 { m = m.min(self.cooldown_feed as u32); }
        if self.cooldown_heal > 0 { m = m.min(self.cooldown_heal as u32); }
        if self.cooldown_relax > 0 { m = m.min(self.cooldown_relax as u32); }
        if self.cooldown_play > 0 { m = m.min(self.cooldown_play as u32); }

        // ── Sleep tier transitions ──

        if self.is_sleeping {
            m = m.min(ticks_down(self.tired, SLEEP_TIER_SLOW, SLEEP_RECOVERY_SLOW).max(1));
            m = m.min(ticks_down(self.tired, SLEEP_TIER_MEDIUM, SLEEP_RECOVERY_MEDIUM).max(1));
            // Auto-wake: tired → 0.
            let wake_rate = if self.tired >= SLEEP_TIER_SLOW { SLEEP_RECOVERY_SLOW }
                else if self.tired >= SLEEP_TIER_MEDIUM { SLEEP_RECOVERY_MEDIUM }
                else { SLEEP_RECOVERY_FAST };
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
                    self.hunger = sat_sub(self.hunger, mul_dt(FEED_HUNGER_RELIEF, t as u32));
                    self.drained = sat_sub(self.drained, mul_dt(FEED_DRAINED_RELIEF, t as u32));
                }
                Action::Heal => {
                    self.sick = sat_sub(self.sick, mul_dt(HEAL_SICK_RELIEF, t as u32));
                }
                Action::Relax => {
                    self.drained = sat_sub(self.drained, mul_dt(RELAX_DRAINED_RELIEF, t as u32));
                    self.hunger = sat_add(self.hunger, mul_dt(RELAX_HUNGER_COST, t as u32));
                }
                Action::Play => {
                    let cm = curiosity_modifier(self.curiosity);
                    let cost_mul = (10u16.saturating_sub(cm)) as u32;
                    let apply = |base: u16| -> u16 {
                        mul_dt((base as u32 * cost_mul / 10) as u16, t as u32)
                    };
                    self.hunger = sat_add(self.hunger, apply(PLAY_HUNGER_COST));
                    self.tired = sat_add(self.tired, apply(PLAY_TIRED_COST));
                    self.drained = sat_add(self.drained, apply(PLAY_DRAINED_COST));
                }
            }

            if self.action_ticks_remaining == 0 {
                // Action complete — set cooldown.
                match action {
                    Action::Feed => self.cooldown_feed = FEED_COOLDOWN,
                    Action::Heal => self.cooldown_heal = HEAL_COOLDOWN,
                    Action::Relax => self.cooldown_relax = RELAX_COOLDOWN,
                    Action::Play => {
                        self.miserable = 0; // play zeroes miserable on completion
                        self.cooldown_play = PLAY_COOLDOWN;
                    }
                }
                self.active_action = None;
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
    }

    /// Apply stat decay while awake for `delta` ticks.
    fn apply_awake_decay(&mut self, delta: u32) {
        let miserable_high = self.miserable >= MISERABLE_BOOST_THRESHOLD;

        // Hunger (suppressed during feed action + cooldown).
        if self.cooldown_feed == 0 && self.active_action != Some(Action::Feed) {
            let rate = HUNGER_RATE + if miserable_high { HUNGER_MISERABLE_BOOST } else { 0 };
            self.hunger = sat_add(self.hunger, mul_dt(rate, delta));
        }

        // Tired (never suppressed).
        {
            let rate = TIRED_RATE + if miserable_high { TIRED_MISERABLE_BOOST } else { 0 };
            self.tired = sat_add(self.tired, mul_dt(rate, delta));
        }

        // Tired passive recovery.
        {
            let (fires, new_counter) = interval_fires(
                delta, self.tired_passive_counter, TIRED_PASSIVE_INTERVAL,
            );
            self.tired_passive_counter = new_counter;
            if fires > 0 {
                self.tired = sat_sub(self.tired, mul_dt(TIRED_PASSIVE_RECOVERY, fires));
            }
        }

        // Drained (suppressed during relax action + cooldown).
        if self.cooldown_relax == 0 && self.active_action != Some(Action::Relax) {
            let interval = if self.miserable >= MISERABLE_DRAIN_THRESHOLD {
                DRAINED_INTERVAL_MISERABLE
            } else {
                DRAINED_INTERVAL
            };
            let (fires, new_counter) = interval_fires(
                delta, self.drained_interval_counter, interval,
            );
            self.drained_interval_counter = new_counter;
            if fires > 0 {
                self.drained = sat_add(self.drained, mul_dt(DRAINED_AMOUNT, fires));
            }
        }

        // Sick (suppressed during heal action + cooldown).
        if self.cooldown_heal == 0 && self.active_action != Some(Action::Heal) {
            let base = mul_dt(SICK_RATE, delta);
            let condition = if sick_condition_active(self) {
                let rate = if miserable_high { SICK_CONDITION_MISERABLE_RATE } else { SICK_CONDITION_RATE };
                mul_dt(rate, delta)
            } else {
                0
            };
            self.sick = sat_add(self.sick, base.saturating_add(condition));
        }

        // Miserable (suppressed during play action + cooldown).
        if self.cooldown_play == 0 && self.active_action != Some(Action::Play) {
            let above = count_above_60(self);
            let interval = MISERABLE_INTERVAL_BASE
                .saturating_sub(MISERABLE_INTERVAL_PER_STAT * above)
                .max(MISERABLE_INTERVAL_MIN);
            let (fires, new_counter) = interval_fires(
                delta, self.miserable_interval_counter, interval,
            );
            self.miserable_interval_counter = new_counter;
            if fires > 0 {
                self.miserable = sat_add(self.miserable, mul_dt(MISERABLE_AMOUNT, fires));
            }
        }
    }

    /// Apply stat changes during sleep for `delta` ticks.
    fn apply_sleep_decay(&mut self, delta: u32) {
        let miserable_high = self.miserable >= MISERABLE_BOOST_THRESHOLD;

        // Tired recovery (tiered by current level).
        let recovery_rate = if self.tired >= SLEEP_TIER_SLOW {
            SLEEP_RECOVERY_SLOW
        } else if self.tired >= SLEEP_TIER_MEDIUM {
            SLEEP_RECOVERY_MEDIUM
        } else {
            SLEEP_RECOVERY_FAST
        };
        self.tired = sat_sub(self.tired, mul_dt(recovery_rate, delta));

        // Auto-wake when tired reaches 0.
        if self.tired == 0 {
            self.is_sleeping = false;
        }

        // Drained recovers during sleep.
        self.drained = sat_sub(self.drained, mul_dt(DRAINED_SLEEP_RECOVERY, delta));

        // Hunger still decays during sleep.
        if self.cooldown_feed == 0 {
            let rate = HUNGER_RATE + if miserable_high { HUNGER_MISERABLE_BOOST } else { 0 };
            self.hunger = sat_add(self.hunger, mul_dt(rate, delta));
        }

        // Sick still decays during sleep.
        if self.cooldown_heal == 0 {
            let base = mul_dt(SICK_RATE, delta);
            let condition = if sick_condition_active(self) {
                let rate = if miserable_high { SICK_CONDITION_MISERABLE_RATE } else { SICK_CONDITION_RATE };
                mul_dt(rate, delta)
            } else {
                0
            };
            self.sick = sat_add(self.sick, base.saturating_add(condition));
        }
    }

    /// Check leaving conditions and update leaving countdown.
    fn check_leaving(&mut self, delta: u32) {
        let maxed = count_maxed(self);
        if maxed == 0 {
            self.leaving_countdown = 0;
            if self.phase == Phase::Leaving {
                self.phase = Phase::Active;
            }
            return;
        }

        self.leaving_countdown += delta;
        let threshold = LEAVING_THRESHOLDS[maxed.min(4)];

        if self.leaving_countdown >= threshold {
            self.phase = Phase::Gone;
        } else if self.phase == Phase::Active {
            self.phase = Phase::Leaving;
        }
    }
}

// ---------------------------------------------------------------------------
// User actions
// ---------------------------------------------------------------------------

impl GameState {
    /// Start the feed action.  Returns false if not available.
    pub fn feed(&mut self) -> bool {
        if self.phase != Phase::Active || self.is_sleeping { return false; }
        if self.active_action.is_some() || self.cooldown_feed > 0 { return false; }
        self.active_action = Some(Action::Feed);
        self.action_ticks_remaining = FEED_DURATION;
        true
    }

    /// Start the heal action.
    pub fn heal(&mut self) -> bool {
        if self.phase != Phase::Active || self.is_sleeping { return false; }
        if self.active_action.is_some() || self.cooldown_heal > 0 { return false; }
        self.active_action = Some(Action::Heal);
        self.action_ticks_remaining = HEAL_DURATION;
        true
    }

    /// Start the relax action.
    pub fn relax(&mut self) -> bool {
        if self.phase != Phase::Active || self.is_sleeping { return false; }
        if self.active_action.is_some() || self.cooldown_relax > 0 { return false; }
        self.active_action = Some(Action::Relax);
        self.action_ticks_remaining = RELAX_DURATION;
        true
    }

    /// Start the play action.
    pub fn play(&mut self) -> bool {
        if self.phase != Phase::Active || self.is_sleeping { return false; }
        if self.active_action.is_some() || self.cooldown_play > 0 { return false; }
        self.active_action = Some(Action::Play);
        self.action_ticks_remaining = PLAY_DURATION;
        true
    }

    /// Put the pet to sleep.
    pub fn sleep(&mut self) -> bool {
        if self.phase != Phase::Active || self.is_sleeping { return false; }
        self.is_sleeping = true;
        true
    }

    /// Wake the pet up.
    pub fn wake(&mut self) -> bool {
        if !self.is_sleeping { return false; }
        self.is_sleeping = false;
        true
    }

    /// Hibernate the pet — all progression freezes.
    pub fn hibernate(&mut self) -> bool {
        if self.hibernating || self.phase == Phase::Gone { return false; }
        self.hibernating = true;
        true
    }

    /// End hibernation — progression resumes from this moment.
    pub fn wake_from_hibernation(&mut self) -> bool {
        if !self.hibernating { return false; }
        self.hibernating = false;
        true
    }

    /// Total hours the pet has spent in hibernation during its life.
    pub fn hibernate_hours(&self) -> u32 {
        self.hibernate_ticks / 360 // 360 ticks = 1 hour
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
        if self.phase == Phase::Gone || self.hibernating { return u32::MAX; }

        let now = self.last_update_tick;
        let mut earliest = now + MAX_SLEEP_TICKS;

        // Hatching countdown.
        if self.phase == Phase::Hatching {
            return now + self.hatching_countdown as u32;
        }

        // Active action completion.
        if self.active_action.is_some() {
            earliest = earliest.min(now + self.action_ticks_remaining as u32);
        }

        // Cooldown expiry.
        if self.cooldown_feed > 0 { earliest = earliest.min(now + self.cooldown_feed as u32); }
        if self.cooldown_heal > 0 { earliest = earliest.min(now + self.cooldown_heal as u32); }
        if self.cooldown_relax > 0 { earliest = earliest.min(now + self.cooldown_relax as u32); }
        if self.cooldown_play > 0 { earliest = earliest.min(now + self.cooldown_play as u32); }

        // Stat boundary crossings.
        let miserable_high = self.miserable >= MISERABLE_BOOST_THRESHOLD;

        // Hunger → sick trigger threshold.
        if self.hunger < SICK_TRIGGER_HUNGER && self.cooldown_feed == 0 {
            let rate = HUNGER_RATE + if miserable_high { HUNGER_MISERABLE_BOOST } else { 0 };
            if rate > 0 {
                let ticks = (SICK_TRIGGER_HUNGER - self.hunger) as u32 / rate as u32;
                earliest = earliest.min(now + ticks);
            }
        }

        // Tired → sick trigger threshold.
        if self.tired < SICK_TRIGGER_TIRED {
            let rate = TIRED_RATE + if miserable_high { TIRED_MISERABLE_BOOST } else { 0 };
            if rate > 0 {
                let ticks = (SICK_TRIGGER_TIRED - self.tired) as u32 / rate as u32;
                earliest = earliest.min(now + ticks);
            }
        }

        // Miserable → 70% boost threshold.
        if self.miserable < MISERABLE_BOOST_THRESHOLD && self.cooldown_play == 0 {
            let above = count_above_60(self);
            let interval = MISERABLE_INTERVAL_BASE
                .saturating_sub(MISERABLE_INTERVAL_PER_STAT * above)
                .max(MISERABLE_INTERVAL_MIN);
            // Average rate: MISERABLE_AMOUNT / interval.
            let fires_to_threshold = (MISERABLE_BOOST_THRESHOLD - self.miserable) as u32
                / MISERABLE_AMOUNT as u32;
            let ticks = fires_to_threshold * interval;
            earliest = earliest.min(now + ticks);
        }

        // Leaving countdown.
        if self.phase == Phase::Leaving {
            let maxed = count_maxed(self);
            if maxed > 0 {
                let threshold = LEAVING_THRESHOLDS[maxed.min(4)];
                let remaining = threshold.saturating_sub(self.leaving_countdown);
                earliest = earliest.min(now + remaining);
            }
        }

        // Sleep: tired reaching 0 (auto-wake).
        if self.is_sleeping && self.tired > 0 {
            let rate = if self.tired >= SLEEP_TIER_SLOW {
                SLEEP_RECOVERY_SLOW
            } else if self.tired >= SLEEP_TIER_MEDIUM {
                SLEEP_RECOVERY_MEDIUM
            } else {
                SLEEP_RECOVERY_FAST
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
    /// How inspired/energised the pet is (100 = energised, 0 = burnt out).
    pub inspired: u8,
    /// How healthy the pet is (100 = healthy, 0 = critically ill).
    pub healthy: u8,
    /// How happy the pet is (100 = happy, 0 = miserable).
    pub happy: u8,

    /// Current lifecycle phase.
    pub phase: Phase,
    /// Whether the pet is sleeping.
    pub is_sleeping: bool,
    /// Age in ticks (1 tick = 10 seconds).
    pub age_ticks: u32,
    /// Generation number (0 = first pet).
    pub generation: u16,

    /// Currently active action (if any).
    pub active_action: Option<Action>,
    /// Ticks remaining on the active action.
    pub action_ticks_remaining: u8,

    /// Action availability (true = can be started right now).
    pub can_feed: bool,
    pub can_heal: bool,
    pub can_relax: bool,
    pub can_play: bool,
    pub can_sleep: bool,
    pub can_wake: bool,

    /// Whether the pet is hibernating (all progression frozen).
    pub hibernating: bool,
    /// Total hours spent in hibernation during this pet's life.
    pub hibernate_hours: u32,
}

/// Convert internal stat (0=good, 65535=bad) to display (0=bad, 100=good).
fn to_display_pct(val: u16) -> u8 {
    100 - (val as u32 * 100 / STAT_MAX as u32) as u8
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
        let awake_active = self.phase == Phase::Active && !self.is_sleeping;

        PetStats {
            hunger:   to_display_pct(self.hunger),
            tired:    to_display_pct(self.tired),
            inspired: to_display_pct(self.drained),
            healthy:  to_display_pct(self.sick),
            happy:    to_display_pct(self.miserable),

            phase: self.phase,
            is_sleeping: self.is_sleeping,
            age_ticks: self.age_ticks,
            generation: self.generation,

            active_action: self.active_action,
            action_ticks_remaining: self.action_ticks_remaining,

            can_feed:  awake_active && action_idle && self.cooldown_feed == 0,
            can_heal:  awake_active && action_idle && self.cooldown_heal == 0,
            can_relax: awake_active && action_idle && self.cooldown_relax == 0,
            can_play:  awake_active && action_idle && self.cooldown_play == 0,
            can_sleep: self.phase == Phase::Active && !self.is_sleeping,
            can_wake:  self.is_sleeping,

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
    /// Becomes true when at least `SAVE_INTERVAL_TICKS` (15 minutes)
    /// have elapsed since the last save.  The caller does the async
    /// save and then calls [`mark_saved()`].
    ///
    /// No extra wake-ups are scheduled for saving — this check
    /// piggybacks on whatever triggered the current update.
    pub fn needs_save(&self) -> bool {
        self.age_ticks.saturating_sub(self.last_save_tick) >= SAVE_INTERVAL_TICKS
    }

    /// Mark the state as successfully saved.  Resets the save timer.
    pub fn mark_saved(&mut self) {
        self.last_save_tick = self.age_ticks;
    }
}

// ---------------------------------------------------------------------------
// Serialization — manual, no serde, fixed-size
// ---------------------------------------------------------------------------

/// Serialized size of GameState in bytes.
pub const SAVE_SIZE: usize = 64;

impl GameState {
    /// Serialize the game state to a fixed-size byte buffer for ekv.
    pub fn to_bytes(&self) -> [u8; SAVE_SIZE] {
        let mut b = [0u8; SAVE_SIZE];
        let mut i = 0;

        macro_rules! w16 { ($v:expr) => { b[i..i+2].copy_from_slice(&$v.to_le_bytes()); i += 2; }; }
        macro_rules! w32 { ($v:expr) => { b[i..i+4].copy_from_slice(&$v.to_le_bytes()); i += 4; }; }
        macro_rules! w8  { ($v:expr) => { b[i] = $v; i += 1; }; }

        // Stats (10 bytes).
        w16!(self.hunger); w16!(self.tired); w16!(self.drained);
        w16!(self.sick); w16!(self.miserable);
        // Traits (6 bytes).
        w16!(self.vitality); w16!(self.curiosity); w16!(self.resilience);
        // Timing (8 bytes).
        w32!(self.last_update_tick); w32!(self.age_ticks);
        // Lifecycle (9 bytes).
        w8!(self.phase as u8);
        w16!(self.hatching_countdown);
        w32!(self.leaving_countdown);
        w16!(self.generation);
        // Action state (9 bytes).
        w8!(self.active_action.map_or(0xFF, |a| a as u8));
        w8!(self.action_ticks_remaining);
        w16!(self.cooldown_feed); w16!(self.cooldown_heal);
        w16!(self.cooldown_relax); w16!(self.cooldown_play);
        // Interval counters (12 bytes).
        w32!(self.drained_interval_counter);
        w32!(self.miserable_interval_counter);
        w32!(self.tired_passive_counter);
        // Flags (2 bytes).
        w8!(self.is_sleeping as u8);
        w8!(self.hibernating as u8);
        // Hibernation (4 bytes).
        w32!(self.hibernate_ticks);
        // Save tick (4 bytes).
        w32!(self.last_save_tick);
        // Total: 64 bytes.
        b
    }

    /// Deserialize a game state from a byte buffer.
    /// Returns `None` if the buffer is too short.
    pub fn from_bytes(b: &[u8]) -> Option<Self> {
        if b.len() < SAVE_SIZE { return None; }
        let mut i = 0;

        macro_rules! r16 { () => {{ let v = u16::from_le_bytes([b[i], b[i+1]]); i += 2; v }}; }
        macro_rules! r32 { () => {{ let v = u32::from_le_bytes([b[i], b[i+1], b[i+2], b[i+3]]); i += 4; v }}; }
        macro_rules! r8  { () => {{ let v = b[i]; i += 1; v }}; }

        let hunger = r16!(); let tired = r16!(); let drained = r16!();
        let sick = r16!(); let miserable = r16!();
        let vitality = r16!(); let curiosity = r16!(); let resilience = r16!();
        let last_update_tick = r32!(); let age_ticks = r32!();
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
        let active_action = match action_byte {
            0 => Some(Action::Feed),
            1 => Some(Action::Heal),
            2 => Some(Action::Relax),
            3 => Some(Action::Play),
            _ => None,
        };
        let action_ticks_remaining = r8!();
        let cooldown_feed = r16!(); let cooldown_heal = r16!();
        let cooldown_relax = r16!(); let cooldown_play = r16!();
        let drained_interval_counter = r32!();
        let miserable_interval_counter = r32!();
        let tired_passive_counter = r32!();
        let is_sleeping = r8!() != 0;
        let hibernating = r8!() != 0;
        let hibernate_ticks = r32!();
        let last_save_tick = r32!();

        Some(Self {
            hunger, tired, drained, sick, miserable,
            vitality, curiosity, resilience,
            last_update_tick, age_ticks,
            phase, hatching_countdown, leaving_countdown, generation,
            active_action, action_ticks_remaining,
            cooldown_feed, cooldown_heal, cooldown_relax, cooldown_play,
            drained_interval_counter, miserable_interval_counter, tired_passive_counter,
            is_sleeping, hibernating, hibernate_ticks,
            last_save_tick,
        })
    }
}
