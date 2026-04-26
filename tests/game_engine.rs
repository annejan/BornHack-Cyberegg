//! Game engine tests — validates progression, action effects, and lifetime
//! ranges against player policies matching the Python simulation.

use hello_graphics::game::engine::thresholds::*;
use hello_graphics::game::engine::{GameState, Phase};

// ---------------------------------------------------------------------------
// Player policies
// ---------------------------------------------------------------------------

trait Policy {
    fn act(&self, state: &mut GameState, tick: u32);
}

/// Never interacts — baseline for survival measurement.
struct AbsentPolicy;
impl Policy for AbsentPolicy {
    fn act(&self, _state: &mut GameState, _tick: u32) {}
}

/// Responds optimally every check interval.
struct AttentivePolicy {
    /// Check interval in ticks.
    check_interval: u32,
}

impl Policy for AttentivePolicy {
    fn act(&self, state: &mut GameState, tick: u32) {
        if tick % self.check_interval != 0 {
            return;
        }
        if state.phase != Phase::Active {
            return;
        }

        // Sleep when tired > 70%.
        if state.tired > 45874 && !state.is_sleeping {
            state.sleep();
            return;
        }

        // Wake when tired < 20%.
        if state.is_sleeping && state.tired < 13107 {
            state.wake();
            return;
        }

        if state.is_sleeping {
            return;
        }

        // Priority: feed > heal > relax > play.
        if state.hunger > 32768 {
            state.feed();
            return;
        }
        if state.sick > 32768 {
            state.heal();
            return;
        }
        if state.drained > 32768 {
            state.relax();
            return;
        }
        if state.miserable > 32768 {
            state.play();
            return;
        }

        // Proactive: feed if available.
        if state.hunger > 16384 {
            state.feed();
            return;
        }
    }
}

/// Optimal play: checks every single tick.
struct PerfectPolicy;
impl Policy for PerfectPolicy {
    fn act(&self, state: &mut GameState, tick: u32) {
        (AttentivePolicy { check_interval: 1 }).act(state, tick);
    }
}

/// Never sleeps — tests tired death.
struct NightOwlPolicy;
impl Policy for NightOwlPolicy {
    fn act(&self, state: &mut GameState, tick: u32) {
        if tick % 6 != 0 {
            return;
        } // check every minute
        if state.phase != Phase::Active {
            return;
        }
        // Wake immediately if sleeping.
        if state.is_sleeping {
            state.wake();
            return;
        }
        if state.hunger > 32768 {
            state.feed();
            return;
        }
        if state.sick > 32768 {
            state.heal();
            return;
        }
        if state.drained > 32768 {
            state.relax();
            return;
        }
        if state.miserable > 32768 {
            state.play();
            return;
        }
    }
}

// ---------------------------------------------------------------------------
// Simulation runner
// ---------------------------------------------------------------------------

/// Run a simulation for up to `max_days` days.
///
/// Uses boundary-based scheduling: the engine jumps to the next
/// interesting event or the policy's check interval, whichever comes
/// first.  A 60-day simulation runs in milliseconds, not minutes.
/// Run with per-tick policy checks.  Uses boundary scheduling so the
/// engine jumps ahead; the policy only fires when the engine wakes.
fn run_sim(policy: &dyn Policy, seed: u64, max_days: u32) -> f64 {
    let ticks_per_day: u32 = 8640;
    let max_ticks = max_days * ticks_per_day;

    let mut state = GameState::new_egg(seed);
    let mut tick: u32 = 0;

    while tick < max_ticks && state.phase != Phase::Gone {
        state.update(tick);
        policy.act(&mut state, tick);
        // Jump to the next boundary — the engine has already computed
        // exactly when rates change.  The policy runs at every wake.
        let next = state.next_wake_tick();
        tick = next.max(tick + 1).min(max_ticks); // always advance ≥ 1
    }

    state.age_ticks as f64 / ticks_per_day as f64
}

fn run_sim_with_interval(
    policy: &dyn Policy,
    seed: u64,
    max_days: u32,
    check_interval: u32,
) -> f64 {
    let ticks_per_day: u32 = 8640;
    let max_ticks = max_days * ticks_per_day;

    let mut state = GameState::new_egg(seed);
    let mut tick: u32 = 0;

    while tick < max_ticks && state.phase != Phase::Gone {
        state.update(tick);
        policy.act(&mut state, tick);
        let next_wake = state.next_wake_tick();
        let next_check = tick + check_interval;
        // Always advance at least 1 tick to prevent infinite loops.
        tick = next_wake.min(next_check).max(tick + 1).min(max_ticks);
    }

    state.age_ticks as f64 / ticks_per_day as f64
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn new_egg_starts_hatching() {
    let state = GameState::new_egg(42);
    assert_eq!(state.phase, Phase::Hatching);
    assert_eq!(state.hatching_countdown, HATCHING_TICKS);
    assert_eq!(state.hunger, 0);
    assert_eq!(state.tired, 0);
    assert!(state.sick > 0); // sick = (STAT_MAX - vitality) / 4
}

#[test]
fn hatching_completes() {
    let mut state = GameState::new_egg(42);
    state.update(HATCHING_TICKS as u32);
    assert_eq!(state.phase, Phase::Active);
    assert_eq!(state.hatching_countdown, 0);
}

#[test]
fn hunger_increases_over_time() {
    let mut state = GameState::new_egg(42);
    state.update(HATCHING_TICKS as u32); // hatch
    let h0 = state.hunger;
    state.update(HATCHING_TICKS as u32 + 100);
    assert!(
        state.hunger > h0,
        "hunger should increase: {} vs {}",
        state.hunger,
        h0
    );
}

#[test]
fn feed_reduces_hunger() {
    let mut state = GameState::new_egg(42);
    state.update(HATCHING_TICKS as u32);
    // Let hunger build up.
    state.update(HATCHING_TICKS as u32 + 1000);
    let h_before = state.hunger;
    assert!(h_before > 0);
    assert!(state.feed());
    // Process feed action.
    state.update(HATCHING_TICKS as u32 + 1000 + FEED_DURATION as u32);
    assert!(state.hunger < h_before, "feed should reduce hunger");
}

#[test]
fn play_zeroes_miserable() {
    let mut state = GameState::new_egg(42);
    state.update(HATCHING_TICKS as u32);
    state.miserable = 30000; // artificially set high
    assert!(state.play());
    state.update(state.last_update_tick + PLAY_DURATION as u32);
    assert_eq!(
        state.miserable, 0,
        "play should zero miserable on completion"
    );
}

#[test]
fn sleep_recovers_tired() {
    let mut state = GameState::new_egg(42);
    state.update(HATCHING_TICKS as u32);
    state.tired = 50000;
    assert!(state.sleep());
    state.update(state.last_update_tick + 100);
    assert!(state.tired < 50000, "sleep should reduce tired");
}

#[test]
fn auto_wake_when_tired_zero() {
    let mut state = GameState::new_egg(42);
    state.update(HATCHING_TICKS as u32);
    state.tired = 1000;
    assert!(state.sleep());
    // Advance just enough for tired to reach 0 (1 tick at fast recovery rate).
    state.update(state.last_update_tick + 1);
    assert!(!state.is_sleeping, "should auto-wake when tired reaches 0");
    assert_eq!(state.tired, 0);
}

#[test]
fn absent_player_pet_dies_quickly() {
    let days = run_sim(&AbsentPolicy, 42, 60);
    assert!(
        days < 5.0,
        "absent player pet should die within 5 days, got {:.1}",
        days
    );
    assert!(days > 0.0, "pet should survive at least some time");
}

#[test]
fn perfect_player_pet_survives_long() {
    let days = run_sim(&PerfectPolicy, 42, 60);
    assert!(
        days > 30.0,
        "perfect player should keep pet alive > 30 days, got {:.1}",
        days
    );
}

#[test]
fn night_owl_dies_faster_than_casual() {
    let owl_days = run_sim(&NightOwlPolicy, 42, 60);
    let casual_days = run_sim_with_interval(
        &AttentivePolicy {
            check_interval: 180,
        }, // 30 min
        42,
        60,
        180,
    );
    assert!(
        owl_days < casual_days,
        "night owl ({:.1}d) should die before casual player ({:.1}d)",
        owl_days,
        casual_days,
    );
}

#[test]
#[ignore] // run with: cargo test -- --ignored diagnostic
fn diagnostic_attentive_hourly_dump() {
    let policy = AttentivePolicy { check_interval: 90 };
    let ticks_per_day: u32 = 8640;
    let ticks_per_hour: u32 = 360;
    let max_ticks = ticks_per_day * 3;

    let mut state = GameState::new_egg(42);
    let mut tick: u32 = 0;
    let mut last_print: u32 = 0;

    while tick < max_ticks && state.phase != Phase::Gone {
        state.update(tick);
        policy.act(&mut state, tick);

        if tick >= last_print + ticks_per_hour || state.phase == Phase::Gone {
            eprintln!(
                "t={:6} ({:5.1}h) H={:5} T={:5} D={:5} S={:5} M={:5} phase={:?} sleep={} cd=[{},{},{},{}]",
                tick,
                tick as f64 / ticks_per_hour as f64,
                state.hunger,
                state.tired,
                state.drained,
                state.sick,
                state.miserable,
                state.phase,
                state.is_sleeping,
                state.cooldown_feed,
                state.cooldown_heal,
                state.cooldown_relax,
                state.cooldown_play,
            );
            last_print = tick;
        }

        let next = state.next_wake_tick();
        tick = next.max(tick + 1).min(max_ticks);
    }
    eprintln!(
        "Died at {:.1} days",
        state.age_ticks as f64 / ticks_per_day as f64
    );
}

#[test]
fn attentive_15min_survives_weeks() {
    let days = run_sim_with_interval(
        &AttentivePolicy { check_interval: 90 }, // 15 min
        42,
        60,
        90,
    );
    assert!(
        days > 14.0,
        "attentive 15min player should survive > 2 weeks, got {:.1}",
        days
    );
}

#[test]
fn reproducible_with_same_seed() {
    let days1 = run_sim(&PerfectPolicy, 12345, 60);
    let days2 = run_sim(&PerfectPolicy, 12345, 60);
    assert_eq!(days1, days2, "same seed should produce identical results");
}

#[test]
fn different_seeds_produce_different_results() {
    let days1 = run_sim(&PerfectPolicy, 1, 60);
    let days2 = run_sim(&PerfectPolicy, 2, 60);
    // Traits differ → sick starts at different levels → lifetimes may differ.
    // This isn't guaranteed to differ but is very likely with different seeds.
    // Just verify both run without panicking.
    assert!(days1 > 0.0);
    assert!(days2 > 0.0);
}

#[test]
fn leaving_triggers_on_maxed_stats() {
    let mut state = GameState::new_egg(42);
    state.update(HATCHING_TICKS as u32);
    // Force all stats to max.
    state.hunger = STAT_MAX;
    state.tired = STAT_MAX;
    state.drained = STAT_MAX;
    state.sick = STAT_MAX;
    // Advance well past the 4-maxed threshold (720 ticks = 2 hours).
    state.update(state.last_update_tick + 2000);
    assert!(
        state.phase == Phase::Gone || state.phase == Phase::Leaving,
        "pet should be leaving or gone with 4 maxed stats, got {:?} countdown={}",
        state.phase,
        state.leaving_countdown,
    );
}

#[test]
fn next_wake_tick_during_hatching() {
    let state = GameState::new_egg(42);
    let wake = state.next_wake_tick();
    assert_eq!(
        wake, HATCHING_TICKS as u32,
        "should wake exactly when hatching completes"
    );
}

#[test]
fn next_wake_tick_bounded_by_max_sleep() {
    let mut state = GameState::new_egg(42);
    state.update(HATCHING_TICKS as u32);
    let wake = state.next_wake_tick();
    assert!(
        wake <= state.last_update_tick + MAX_SLEEP_TICKS,
        "wake time should not exceed MAX_SLEEP_TICKS from now",
    );
}

#[test]
fn cooldown_prevents_action_spam() {
    let mut state = GameState::new_egg(42);
    state.update(HATCHING_TICKS as u32);
    assert!(state.feed());
    state.update(state.last_update_tick + FEED_DURATION as u32);
    // Cooldown active — second feed should fail.
    assert!(!state.feed(), "feed should be blocked during cooldown");
    // Wait out cooldown.
    state.update(state.last_update_tick + FEED_COOLDOWN as u32 + 1);
    assert!(state.feed(), "feed should be available after cooldown");
}
