//! Shared by the budget tests of the scene and Casual binaries. The threshold
//! table is write-once per process, so each binary installs its own preset first.

/// Over 2000 random states, with demo on and off: the cross shows exactly
/// when the pet is poorly, whenever a background exists.
pub fn the_cross_shows_whenever_the_pet_is_poorly() {
    use bornhack_aegg::game::engine::needs::P;
    use bornhack_aegg::game::engine::scene::background_state;
    use bornhack_aegg::game::engine::status::poorly;
    use bornhack_aegg::game::engine::{GameState, PetKind, Phase};

    let mut x = 5u64;
    let mut next = |bound: u64| {
        x = x
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (x >> 33) % (bound + 1)
    };
    let pct = |p: u64| (p as i32 * P) as u16;
    let (mut with_bg, mut sick) = (0, 0);
    for i in 0..2000 {
        let mut s = GameState::new_egg(7, PetKind(0));
        s.phase = Phase::Active;
        s.fullness = pct(next(100));
        s.hydration = pct(next(100));
        s.energy = pct(next(100));
        s.fun = pct(next(100));
        s.dirt = pct(next(100));
        s.booze = pct(next(100));
        s.health = pct(next(100));
        s.mood = pct(next(100));
        s.is_sleeping = next(1) == 1;
        s.demo.on = i % 2 == 0;
        s.set_demo_rng(1 + next(1_000_000) as u32);
        let Some(bg) = background_state(&s) else {
            continue;
        };
        with_bg += 1;
        sick += poorly(&s) as u32;
        assert_eq!(bg.poorly, poorly(&s), "random state {i}");
    }
    assert!(
        with_bg > 1000 && sick > 100,
        "{with_bg} states, {sick} poorly"
    );
}
