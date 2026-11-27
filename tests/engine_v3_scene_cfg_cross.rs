//! A player CFG (DEMO_HELP_HEALTH_PCT=27) can leave a poorly demo pet without a red line: no cross then. The threshold table is write-once per process, so this has its own
//! binary.

use bornhack_aegg::game::engine::needs::P;
use bornhack_aegg::game::engine::scene::background_state;
use bornhack_aegg::game::engine::sim;
use bornhack_aegg::game::engine::thresholds::{BORNPETS_CFG_KEYS, Mode, Thresholds, validate};
use bornhack_aegg::game::engine::{GameState, PetKind, Phase};

fn install() {
    let mut c = Thresholds::CLASSIC;
    let set = BORNPETS_CFG_KEYS
        .iter()
        .find(|(k, _)| *k == "DEMO_HELP_HEALTH_PCT")
        .unwrap()
        .1;
    set(&mut c, 27);
    sim::install_preset(validate(&c, &Thresholds::CLASSIC), Mode::Classic, true);
}

fn demo_pet(health_pct: i32) -> GameState {
    let mut s = GameState::new_egg(7, PetKind(0));
    s.phase = Phase::Active;
    s.fullness = (60 * P) as u16;
    s.hydration = (60 * P) as u16;
    s.energy = (80 * P) as u16;
    s.mood = (90 * P) as u16;
    s.fun = (60 * P) as u16;
    s.dirt = 0;
    s.booze = 0;
    s.demo.on = true;
    s.health = (health_pct * P) as u16;
    s
}

#[test]
fn a_poorly_pet_without_a_red_line_shows_no_cross() {
    install();
    let s = demo_pet(28);
    assert!(!s.pet_line().red(), "{:?}", s.pet_line());
    assert!(!background_state(&s).unwrap().poorly);
}
