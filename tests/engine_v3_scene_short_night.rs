//! A one-minute night: the sky switch is a solver boundary, not just a view
//! difference, or a segment that starts and ends in day steps over it. The
//! threshold table is write-once per process, so this has its own binary.

use bornhack_aegg::game::engine::needs::P;
use bornhack_aegg::game::engine::scene::{Sky, background_state};
use bornhack_aegg::game::engine::sim;
use bornhack_aegg::game::engine::thresholds::{BORNPETS_CFG_KEYS, Mode, Thresholds, validate};
use bornhack_aegg::game::engine::{GameState, PetKind, Phase, TICKS_PER_MINUTE};

fn install() {
    let mut c = Thresholds::CLASSIC;
    for (key, value) in [("BG_NIGHT_FROM_MIN", 1260), ("BG_NIGHT_TO_MIN", 1261)] {
        let set = BORNPETS_CFG_KEYS.iter().find(|(k, _)| *k == key).unwrap().1;
        set(&mut c, value);
    }
    sim::install_preset(validate(&c, &Thresholds::CLASSIC), Mode::Classic, true);
}

fn active() -> GameState {
    let mut s = GameState::new_egg(7, PetKind(0));
    s.phase = Phase::Active;
    s.money_enabled = false;
    s.tick_rem = 0;
    s.is_sleeping = false;
    s.fullness = (60 * P) as u16;
    s.hydration = (60 * P) as u16;
    s.energy = (80 * P) as u16;
    s.health = (95 * P) as u16;
    s.mood = (90 * P) as u16;
    s.fun = (60 * P) as u16;
    s.dirt = 0;
    s.booze = 0;
    s
}

#[test]
fn a_one_minute_night_is_two_wakes() {
    install();
    let mut s = active();
    let t0 = s.last_update_tick;
    s.sync_clock(t0, Some(20 * 60 + 50));
    assert_eq!(s.next_wake_tick(), t0 + 10 * TICKS_PER_MINUTE);
    let t1 = t0 + 10 * TICKS_PER_MINUTE;
    s.update(t1);
    assert_eq!(background_state(&s).unwrap().sky, Sky::Night);
    assert_eq!(s.next_wake_tick(), t1 + TICKS_PER_MINUTE);
}
