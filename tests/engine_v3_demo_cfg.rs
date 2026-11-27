//! A hostile `BPETS_V3.CFG` must not make the demo pet cycle. The threshold
//! table is write-once per process, so this has its own binary.

use bornhack_aegg::game::engine::needs::{MAX, P};
use bornhack_aegg::game::engine::sim;
use bornhack_aegg::game::engine::thresholds::{
    self as t, BORNPETS_CFG_KEYS, Mode, Thresholds, validate,
};
use bornhack_aegg::game::engine::{GameState, PetKind, Phase, TICKS_PER_MINUTE};

const DAY_MIN: u32 = 1440;
const DAYS: u32 = 5;
const SEEDS: u64 = 12;

/// Wake lines above the eat line and a soft fun line above the wired one:
/// the two orders the policy would cycle on.
fn hostile() -> Thresholds {
    let mut c = Thresholds::CLASSIC;
    for (key, v) in [
        ("DEMO_WAKE_FULL_PCT", 60),
        ("DEMO_SOFT_FUN_PCT", 90),
        ("DEMO_WIRED_FUN_PCT", 30),
    ] {
        let set = BORNPETS_CFG_KEYS.iter().find(|(k, _)| *k == key).unwrap().1;
        set(&mut c, v);
    }
    c
}

fn install() {
    let raw = hostile();
    assert!(
        raw.DEMO_WAKE_FULL_PCT > raw.DEMO_EDGE_FULL_PCT,
        "the CFG is hostile"
    );
    let ok = validate(&raw, &Thresholds::CLASSIC);
    sim::install_preset(ok, Mode::Classic, true);
}

fn pct(p: u64) -> u16 {
    (p as i32 * P) as u16
}

#[test]
fn a_hostile_cfg_is_clamped_and_the_pet_never_cycles() {
    install();
    assert!(t::DEMO_WAKE_FULL_PCT() <= t::DEMO_EDGE_FULL_PCT());
    assert!(t::DEMO_SOFT_FUN_PCT() < t::DEMO_WIRED_FUN_PCT());
    let mut x = 77u64;
    let mut next = |bound: u64| {
        x = x
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (x >> 33) % (bound + 1)
    };
    for seed in 0..SEEDS {
        let mut s = GameState::new_egg(seed, PetKind(0));
        s.phase = Phase::Active;
        s.money_enabled = false;
        s.mood = MAX as u16;
        s.health = pct(70);
        // Hungry enough for the wake line, rested enough not to sleep, tired
        // enough to want bed: the state the cycle needs.
        s.fullness = pct(20 + next(40));
        s.energy = pct(next(14));
        s.hydration = pct(60);
        s.fun = pct(next(100));
        s.demo.on = true;
        s.demo.whim_min = 100;
        s.demo.lazy_min = 100;
        s.set_demo_rng(1 + next(1_000_000) as u32);
        s.sync_clock(s.last_update_tick, Some(next(1439) as u16));
        // `run` asserts (debug) when the cap stops it with a choice left.
        let mut wakes = 0u32;
        let end = s.last_update_tick + DAYS * DAY_MIN * TICKS_PER_MINUTE;
        while s.last_update_tick < end && s.phase != Phase::Gone {
            let wake = s.next_wake_tick();
            assert!(wake > s.last_update_tick);
            s.update(wake);
            wakes += 1;
        }
        assert!(
            wakes < DAYS * DAY_MIN,
            "seed {seed}: {wakes} wakes in {DAYS} days"
        );
    }
}
