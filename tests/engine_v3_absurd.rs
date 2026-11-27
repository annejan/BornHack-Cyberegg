//! Absurd `BPETS_V3.CFG` values (every key 65535) must not break the engine.
//! The threshold table is write-once per process, so this has its own binary.

use bornhack_aegg::game::engine::needs::P;
use bornhack_aegg::game::engine::sim;
use bornhack_aegg::game::engine::thresholds::{
    self, BORNPETS_CFG_KEYS, Mode, Thresholds, validate,
};
use bornhack_aegg::game::engine::{DrinkKind, GameState, PetKind, Phase};

fn absurd() -> Thresholds {
    let mut t = Thresholds::CLASSIC;
    for (_, setter) in BORNPETS_CFG_KEYS {
        setter(&mut t, 65535);
    }
    validate(&t, &Thresholds::CLASSIC)
}

fn install_absurd() {
    sim::install_preset(absurd(), Mode::Classic, true);
}

#[test]
fn setters_clamp_multipliers_and_mood_step() {
    let t = absurd();
    assert!(t.MOOD_STEP >= 1);
    assert_eq!(t.DOSE_TIRED_BOOZE_PCT, 1000);
    assert_eq!(t.ENERGY_MULT_DRUNK_PCT, 1000);
    assert_eq!(t.THIRST_MULT_HIGH_PCT, 1000);
    let mut z = Thresholds::CLASSIC;
    for (name, setter) in BORNPETS_CFG_KEYS {
        if *name == "MOOD_STEP" {
            setter(&mut z, 0);
        }
    }
    assert_eq!(z.MOOD_STEP, 1);
}

#[test]
fn validate_orders_the_booze_and_diabetes_pairs() {
    let mut t = Thresholds::CLASSIC;
    t.BOOZE_HI_PCT = 90;
    t.BOOZE_DANGER_PCT = 80;
    t.DIABETES_WEIGHT_LO_PCT = 95;
    t.DIABETES_WEIGHT_HI_PCT = 90;
    let v = validate(&t, &Thresholds::CLASSIC);
    let c = Thresholds::CLASSIC;
    assert_eq!(
        (v.BOOZE_HI_PCT, v.BOOZE_DANGER_PCT),
        (c.BOOZE_HI_PCT, c.BOOZE_DANGER_PCT)
    );
    assert_eq!(
        (v.DIABETES_WEIGHT_LO_PCT, v.DIABETES_WEIGHT_HI_PCT),
        (c.DIABETES_WEIGHT_LO_PCT, c.DIABETES_WEIGHT_HI_PCT)
    );
}

#[test]
fn absurd_config_gives_a_sane_engine() {
    install_absurd();
    let mut s = GameState::new_egg(7, PetKind(0));
    s.update(thresholds::HATCHING_TICKS() as u32);
    assert_eq!(s.phase, Phase::Active);
    s.is_sleeping = false;
    s.tick_rem = 0;
    s.money_enabled = false;
    s.energy = (20 * P) as u16;
    s.booze = (30 * P) as u16;
    let before = s.booze;
    assert!(s.drink(DrinkKind::Whiskey));
    let end = s.last_update_tick + thresholds::DRINK_DURATION() as u32;
    s.update(end);
    assert!(
        s.booze >= before,
        "a drink lowered booze: {} -> {}",
        before,
        s.booze
    );

    // A day of ticks: no panic, a state that still answers every query.
    let day = s.last_update_tick + 1440 * 6;
    s.update(day);
    assert!(matches!(
        s.phase,
        Phase::Active | Phase::Leaving | Phase::Gone
    ));
}
