//! BornPets v3 balance simulator: plays every routine for 30 days with the
//! minute-stepping oracle and with the delta engine and prints both.
//!
//! Usage: `cargo run --release --bin simulate_game --features simulator [-- --preset classic|casual]`
//! Or:    `make simulate-game`

use bornhack_aegg::game::engine::GameState;
use bornhack_aegg::game::engine::sim::{self, Driver, Plan};
use bornhack_aegg::game::engine::thresholds::Mode;

type Routine = fn(u32, u32, &GameState) -> Plan;

const ROUTINES: [(&str, Routine); 9] = [
    ("balanced", sim::balanced),
    ("sporty", sim::sporty),
    ("lazy_treats", sim::lazy_treats),
    ("party", sim::party),
    ("short_sleeper", sim::short_sleeper),
    ("spam_water", sim::spam_water),
    ("spam_wash", sim::spam_wash),
    ("once_a_day", sim::once_a_day),
    ("absent", sim::absent),
];

fn preset_from_args() -> Mode {
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        if a == "--preset" {
            match args.next().as_deref() {
                Some("classic") => return Mode::Classic,
                Some("casual") => return Mode::Casual,
                other => {
                    eprintln!("--preset needs classic or casual, got {other:?}");
                    std::process::exit(2);
                }
            }
        }
    }
    Mode::Classic
}

fn main() {
    let mode = preset_from_args();
    sim::install_preset(mode.preset(), mode, false);
    println!(
        "preset: {}   each cell: step oracle / delta engine, 30 days\n",
        mode.label()
    );
    println!(
        "{:<14} {:>14} {:>9} {:>11} {:>11} {:>8} {:>8} {:>6} {:>8}",
        "routine",
        "outcome",
        "mood %",
        "end health",
        "min health",
        "fit %",
        "weight %",
        "inj",
        "wakes/d"
    );
    for (name, r) in ROUTINES {
        let o = sim::run_routine(&r, 30, Driver::Oracle);
        let d = sim::run_routine(&r, 30, Driver::Delta);
        let out = |x: &sim::Outcome| {
            if x.alive {
                "alive".to_string()
            } else {
                format!("gone d{:.1}", x.days)
            }
        };
        println!(
            "{:<14} {:>14} {:>9} {:>11} {:>11} {:>8} {:>8} {:>6} {:>8.1}",
            name,
            format!("{} / {}", out(&o), out(&d)),
            format!("{}/{}", o.avg_mood_pct, d.avg_mood_pct),
            format!("{}/{}", o.end_health_pct, d.end_health_pct),
            format!("{}/{}", o.min_health_pct, d.min_health_pct),
            format!("{}/{}", o.end_fitness_pct, d.end_fitness_pct),
            format!("{}/{}", o.end_weight_pct, d.end_weight_pct),
            format!("{}/{}", o.injuries, d.injuries),
            d.engine_wakes_per_day,
        );
    }
}
