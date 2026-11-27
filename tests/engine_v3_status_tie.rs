//! Custom `BPETS_V3.CFG` values can make two half-weight terms (underweight,
//! unfit) rise at the same odd rate, so their truncated values tie every other
//! minute. The status line must rank on exact values, or the top word flips
//! back and forth inside one segment and the wake lands late. The threshold
//! table is write-once per process, so this has its own binary.

use bornhack_aegg::game::engine::needs::P;
use bornhack_aegg::game::engine::sim;
use bornhack_aegg::game::engine::status::{Word, status_line};
use bornhack_aegg::game::engine::thresholds::{HATCHING_TICKS, Mode, Thresholds};
use bornhack_aegg::game::engine::{GameState, PetKind, Phase};

const BELLY_PM: u16 = 1;
const LOOKAHEAD_MIN: u32 = 120;
const TICKS_PER_MIN: u32 = 6;
const SWEEP_PCT_STEPS: i32 = 40;
const SWEEP_LEVEL_STEP: i32 = 7;
// Classic band floors: the two gaps are equal when fitness sits this far above weight.
const WEIGHT_LO_PCT: i32 = 38;
const FIT_LO_PCT: i32 = 43;
const START_FULLNESS_PCT: i32 = 36;

fn install_tie() {
    let mut t = Thresholds::CLASSIC;
    t.WEIGHT_BELLY_PM = BELLY_PM;
    assert_eq!(t.FIT_DECAY_LOW_PM, BELLY_PM, "the tie needs equal rates");
    sim::install_preset(t, Mode::Classic, true);
}

fn pet() -> GameState {
    let mut s = GameState::new_egg(7, PetKind(0));
    s.update(HATCHING_TICKS() as u32);
    assert_eq!(s.phase, Phase::Active);
    s.is_sleeping = false;
    s.tick_rem = 0;
    s.money_enabled = false;
    s
}

fn first_change_min(s: &GameState) -> Option<u32> {
    let mut c = s.clone();
    let o = format!("{:?}", c.wake_view_for_test());
    let t0 = c.last_update_tick;
    (1..=LOOKAHEAD_MIN).find(|m| {
        c.update(t0 + m * TICKS_PER_MIN);
        format!("{:?}", c.wake_view_for_test()) != o
    })
}

#[test]
fn wake_is_exact_when_two_half_terms_tie_on_rounding() {
    install_tie();
    let mut seen_underweight_unfit = false;
    for step in 0..SWEEP_PCT_STEPS {
        for odd in [-1, 0, 1] {
            let mut s = pet();
            // Empty belly: weight falls; low fitness decays: both gaps grow at one rate.
            s.fullness = (START_FULLNESS_PCT * P) as u16;
            s.energy = (100 * P) as u16 - 1;
            s.weight = (3 * P + step * SWEEP_LEVEL_STEP) as u16;
            s.fitness =
                (3 * P + step * SWEEP_LEVEL_STEP + (FIT_LO_PCT - WEIGHT_LO_PCT) * P + odd) as u16;
            let line = status_line(&s);
            seen_underweight_unfit |= [line.first, line.second.unwrap_or(Word::Content)]
                .contains(&Word::Underweight)
                && [line.first, line.second.unwrap_or(Word::Content)].contains(&Word::Unfit);
            let t0 = s.last_update_tick;
            let expect = first_change_min(&s).map_or(t0 + LOOKAHEAD_MIN * TICKS_PER_MIN, |m| {
                t0 + m * TICKS_PER_MIN
            });
            assert_eq!(s.next_wake_tick(), expect, "step {step} odd {odd}");
        }
    }
    assert!(
        seen_underweight_unfit,
        "the sweep never showed the tied pair"
    );
}
