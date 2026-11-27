//! Game engine tests: hatching and fresh-egg basics. The v3 rule tests live
//! in `engine_v3.rs`.

use bornhack_aegg::game::engine::thresholds::*;
use bornhack_aegg::game::engine::{GameState, PetKind, Phase};

#[test]
fn new_egg_starts_hatching() {
    let state = GameState::new_egg(42, PetKind::DEFAULT);
    assert_eq!(state.phase, Phase::Hatching);
    assert_eq!(state.hatching_countdown, HATCHING_TICKS());
    assert_eq!(state.fullness, 60 * 655);
    assert_eq!(state.energy, 80 * 655);
    assert!(state.health > 0); // health = 85 % - (MAX - vitality) / 8
}

#[test]
fn hatching_completes() {
    let mut state = GameState::new_egg(42, PetKind::DEFAULT);
    state.update(HATCHING_TICKS() as u32);
    assert_eq!(state.phase, Phase::Active);
    assert_eq!(state.hatching_countdown, 0);
}

#[test]
fn next_wake_tick_during_hatching() {
    let state = GameState::new_egg(42, PetKind::DEFAULT);
    let wake = state.next_wake_tick();
    assert_eq!(
        wake,
        HATCHING_TICKS() as u32,
        "should wake exactly when hatching completes"
    );
}

#[test]
fn reproducible_with_same_seed() {
    use bornhack_aegg::game::engine::sim::{self, Driver};
    for driver in [Driver::Oracle, Driver::Delta] {
        let a = sim::run_full(&sim::lazy_treats, 10, driver, 7).outcome;
        let b = sim::run_full(&sim::lazy_treats, 10, driver, 7).outcome;
        assert_eq!(format!("{a:?}"), format!("{b:?}"));
    }
}

/// The routines are deterministic, so a seed changes only what the egg
/// rolls: the three traits, and through `vitality` the starting health. No
/// need or rule reads a trait, so a routine run differs only in the lowest
/// health it saw (the starting health, now counted) and nothing else.
#[test]
fn different_seeds_produce_different_results() {
    use bornhack_aegg::game::engine::sim::{self, Driver};
    let (a, b) = (
        GameState::new_egg(1, PetKind::DEFAULT),
        GameState::new_egg(2, PetKind::DEFAULT),
    );
    assert_ne!(
        (a.vitality, a.curiosity, a.resilience),
        (b.vitality, b.curiosity, b.resilience)
    );
    assert_ne!(a.health, b.health, "vitality sets the starting health");
    for driver in [Driver::Oracle, Driver::Delta] {
        let ra = sim::run_full(&sim::balanced, 3, driver, 1).outcome;
        let rb = sim::run_full(&sim::balanced, 3, driver, 2).outcome;
        assert_ne!(ra.min_health_pct, rb.min_health_pct, "{driver:?}");
        let same = |o: sim::Outcome| {
            (
                o.alive,
                o.avg_mood_pct,
                o.end_health_pct,
                o.end_fitness_pct,
                o.end_weight_pct,
                o.injuries,
            )
        };
        assert_eq!(same(ra), same(rb), "{driver:?}");
    }
}
