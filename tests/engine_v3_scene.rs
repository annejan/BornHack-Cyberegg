//! BornPets v3 backgrounds, the engine part (plan 7, task 1).

mod common;

mod state {
    use bornhack_aegg::game::bornpets_cfg::apply_overrides;
    use bornhack_aegg::game::engine::needs::P;
    use bornhack_aegg::game::engine::scene::{
        BackgroundState, FLOOR_ITEMS, Sky, background_state, floor_items, floor_lines, is_night,
        minutes_to_switch, sky, wake_view,
    };
    use bornhack_aegg::game::engine::thresholds::{BORNPETS_CFG_KEYS, Thresholds, validate};
    use bornhack_aegg::game::engine::{GameState, PetKind, Phase};

    fn active() -> GameState {
        let mut s = GameState::new_egg(7, PetKind(0));
        s.phase = Phase::Active;
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
    fn night_is_nine_to_seven() {
        assert!(!is_night(1259));
        assert!(is_night(1260));
        assert!(is_night(0));
        assert!(is_night(419));
        assert!(!is_night(420));
    }

    #[test]
    fn an_unknown_clock_is_plain() {
        assert_eq!(sky(None), Sky::Plain);
        assert_eq!(sky(Some(720)), Sky::Day);
        assert_eq!(sky(Some(1380)), Sky::Night);
    }

    #[test]
    fn floor_items_build_up_in_order() {
        assert_eq!(floor_lines(), [40, 55, 70, 85, 95]);
        let at = |pct: i32| floor_items((pct * P) as u16);
        assert_eq!(at(39), 0);
        assert_eq!(at(40), 1);
        assert_eq!(at(54), 1);
        assert_eq!(at(55), 2);
        assert_eq!(at(70), 3);
        assert_eq!(at(85), 4);
        assert_eq!(at(95), 5);
        assert_eq!(at(100), 5);
        assert_eq!(FLOOR_ITEMS, 5);
    }

    #[test]
    fn switch_minutes_wrap_midnight() {
        assert_eq!(minutes_to_switch(None), None);
        assert_eq!(minutes_to_switch(Some(1250)), Some(10));
        assert_eq!(minutes_to_switch(Some(1260)), Some(600));
        assert_eq!(minutes_to_switch(Some(0)), Some(420));
        assert_eq!(minutes_to_switch(Some(420)), Some(840));
    }

    #[test]
    fn the_cross_needs_poorly_and_a_red_line() {
        let mut s = active();
        s.health = (20 * P) as u16;
        assert!(s.pet_line().red());
        assert!(background_state(&s).unwrap().poorly);
        let mut s = active();
        s.health = (32 * P) as u16;
        assert!(!background_state(&s).unwrap().poorly);
    }

    #[test]
    fn hibernating_and_eggs_draw_nothing() {
        let mut s = GameState::new_egg(7, PetKind(0));
        assert_eq!(s.phase, Phase::Hatching);
        assert_eq!(background_state(&s), None);
        s.phase = Phase::Active;
        assert!(background_state(&s).is_some());
        s.hibernating = true;
        assert_eq!(background_state(&s), None);
        assert_eq!(wake_view(&s), 0);
        s.hibernating = false;
        s.phase = Phase::Leaving;
        assert!(background_state(&s).is_some());
        s.phase = Phase::Gone;
        assert_eq!(background_state(&s), None);
    }

    #[test]
    fn pack_never_collides() {
        let mut seen = std::collections::HashSet::new();
        for sky in [Sky::Plain, Sky::Day, Sky::Night] {
            for floor in 0..=FLOOR_ITEMS as u8 {
                for poorly in [false, true] {
                    let p = BackgroundState { sky, floor, poorly }.pack();
                    assert_ne!(p, 0);
                    assert!(seen.insert(p), "collision at {p}");
                }
            }
        }
        assert_eq!(seen.len(), 3 * 6 * 2);
    }

    #[test]
    fn floor_lines_must_rise() {
        let preset = Thresholds::CLASSIC;
        let mut t = preset;
        let ok = apply_overrides(&mut t, b"FLOOR_CAN_DIRT_PCT=50\n");
        assert_eq!(ok, 1);
        assert_eq!(t.FLOOR_CAN_DIRT_PCT, 50);
        let v = validate(&t, &preset);
        assert_eq!(
            [
                v.FLOOR_CRUMBS_DIRT_PCT,
                v.FLOOR_PEEL_DIRT_PCT,
                v.FLOOR_CAN_DIRT_PCT,
                v.FLOOR_SOCK_DIRT_PCT,
                v.FLOOR_PIZZA_DIRT_PCT
            ],
            [40, 55, 70, 85, 95]
        );
        let mut t = preset;
        apply_overrides(&mut t, b"BG_NIGHT_FROM_MIN=600\nBG_NIGHT_TO_MIN=600\n");
        let v = validate(&t, &preset);
        assert_eq!((v.BG_NIGHT_FROM_MIN, v.BG_NIGHT_TO_MIN), (1260, 420));
        assert!(
            BORNPETS_CFG_KEYS
                .iter()
                .any(|(k, _)| *k == "BG_NIGHT_FROM_MIN")
        );
    }
}

mod wake {
    use bornhack_aegg::game::engine::needs::{MAX, P};
    use bornhack_aegg::game::engine::scene::{Sky, background_state, floor_items};
    use bornhack_aegg::game::engine::thresholds::MAX_SLEEP_TICKS;
    use bornhack_aegg::game::engine::{GameState, PetKind, Phase, TICKS_PER_MINUTE};

    fn pct(p: i32) -> u16 {
        (p * P) as u16
    }

    fn active() -> GameState {
        let mut s = GameState::new_egg(7, PetKind(0));
        s.phase = Phase::Active;
        s.money_enabled = false;
        s.tick_rem = 0;
        s.is_sleeping = false;
        s.fullness = pct(60);
        s.hydration = pct(60);
        s.energy = pct(80);
        s.health = pct(95);
        s.mood = pct(90);
        s.fun = pct(60);
        s.dirt = 0;
        s.booze = 0;
        s
    }

    /// Oracle: step a clone one minute at a time (a test-only step loop).
    fn first_change_min(s: &GameState, limit: u32) -> Option<u32> {
        let mut c = s.clone();
        let o = format!("{:?}", c.wake_view_for_test());
        let t0 = c.last_update_tick;
        (1..=limit).find(|m| {
            c.update(t0 + m * TICKS_PER_MINUTE);
            format!("{:?}", c.wake_view_for_test()) != o || c.note().is_some()
        })
    }

    #[test]
    fn scene_wake_is_exact_on_random_states() {
        const STATES: usize = 2000;
        let limit = MAX_SLEEP_TICKS().div_ceil(TICKS_PER_MINUTE);
        let mut x = 11u64;
        let mut next = |bound: u64| {
            x = x
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (x >> 33) % (bound + 1)
        };
        let (mut timed, mut with_clock) = (0, 0);
        for i in 0..STATES {
            let mut s = active();
            s.fullness = pct(next(100) as i32);
            s.hydration = pct(next(100) as i32);
            s.energy = pct(next(100) as i32);
            s.fun = pct(next(100) as i32);
            s.dirt = pct(30 + next(70) as i32);
            s.booze = pct(next(100) as i32);
            s.fitness = pct(next(100) as i32);
            s.weight = pct(next(100) as i32);
            s.health = pct(30 + next(70) as i32);
            s.mood = MAX as u16;
            s.is_sleeping = next(1) == 1;
            s.demo.on = i % 2 == 0;
            s.set_demo_rng(1 + next(1_000_000) as u32);
            let clock = if next(1) == 1 {
                with_clock += 1;
                Some(next(1439) as u16)
            } else {
                None
            };
            s.sync_clock(s.last_update_tick, clock);
            let t0 = s.last_update_tick;
            let mut c = s.clone();
            bornhack_aegg::game::engine::demo::settle(&mut c);
            let expect = if bornhack_aegg::game::engine::demo::decide(&c).is_some() {
                t0 + 1
            } else {
                first_change_min(&c, limit).map_or(t0 + MAX_SLEEP_TICKS(), |m| {
                    timed += 1;
                    t0 + m * TICKS_PER_MINUTE
                })
            };
            assert_eq!(s.next_wake_tick(), expect, "random state {i}");
        }
        assert!(timed > 200, "{timed} states changed within the look-ahead");
        assert!(with_clock > 500);
    }

    #[test]
    fn the_sky_switch_is_a_wake() {
        let mut s = active();
        let t0 = s.last_update_tick;
        s.sync_clock(t0, Some(20 * 60 + 50));
        assert_eq!(background_state(&s).unwrap().sky, Sky::Day);
        assert_eq!(s.next_wake_tick(), t0 + 10 * TICKS_PER_MINUTE);
        s.update(t0 + 10 * TICKS_PER_MINUTE);
        assert_eq!(background_state(&s).unwrap().sky, Sky::Night);
    }

    #[test]
    fn a_floor_step_is_a_wake() {
        let mut s = active();
        s.dirt = pct(39);
        let t0 = s.last_update_tick;
        let m = first_change_min(&s, 120).expect("dirt rises to 40 %");
        assert_eq!(s.next_wake_tick(), t0 + m * TICKS_PER_MINUTE);
        let mut c = s.clone();
        c.update(t0 + m * TICKS_PER_MINUTE);
        assert_eq!(floor_items(c.dirt), 1);
        assert_eq!(background_state(&c).unwrap().floor, 1);
    }

    #[test]
    fn the_sky_does_not_change_the_pet() {
        fn mk(day: Option<u16>) -> GameState {
            let mut s = active();
            s.fullness = pct(70);
            s.dirt = pct(20);
            s.sync_clock(s.last_update_tick, day);
            s
        }
        fn view(s: &GameState) -> String {
            format!(
                "{:?}",
                (
                    (s.fullness, s.hydration, s.energy, s.fun, s.dirt, s.booze),
                    (s.fitness, s.weight, s.mood, s.health, s.phase),
                    (s.diabetic, s.dependent, s.injured_min, s.groggy_min),
                    (s.hangover_min, s.is_sleeping),
                )
            )
        }
        let days = 3 * 1440 * TICKS_PER_MINUTE;
        let mut a = mk(None);
        let mut b = mk(Some(6 * 60));
        let t0 = a.last_update_tick;
        a.update(t0 + days);
        b.update(t0 + days);
        assert_eq!(view(&a), view(&b));
        let mut x = 5u64;
        let mut cuts: Vec<u32> = (0..50)
            .map(|_| {
                x = x
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                ((x >> 33) % days as u64) as u32
            })
            .collect();
        cuts.sort();
        let mut c = mk(Some(6 * 60));
        for cut in cuts {
            c.update(t0 + cut);
        }
        c.update(t0 + days);
        assert_eq!(view(&c), view(&a));
    }

    #[test]
    fn washing_clears_the_floor_at_once() {
        let mut s = active();
        s.dirt = pct(96);
        assert_eq!(background_state(&s).unwrap().floor, 5);
        assert!(s.wash());
        s.update(s.action_end_tick());
        assert_eq!(background_state(&s).unwrap().floor, 0);
    }
}

mod budget {
    use bornhack_aegg::game::engine::sim::{self, Driver, Setup};

    #[test]
    fn backgrounds_add_few_wakes() {
        let demo = sim::run_with(
            &sim::absent,
            10,
            Driver::Delta,
            1,
            Setup {
                demo: true,
                clock: true,
                animated: false,
                money: false,
            },
        )
        .outcome;
        // Printed only, never asserted: the demo's scene-only figure may
        // undercount, because demo bit 0 and a line under a Note are in `Rest`
        // but are not drawn.
        eprintln!(
            "absent, demo, clock: {:.2} scene changes a day ({:.2} scene-only), {:.2} engine wakes a day, {:.2} refreshes a day",
            demo.scene_changes_per_day,
            demo.scene_only_changes_per_day,
            demo.engine_wakes_per_day,
            demo.refreshes_per_day
        );
        assert!(
            demo.scene_changes_per_day > 0.0,
            "the counter sees the sky switch"
        );
        assert!(demo.scene_changes_per_day <= 8.0);
        assert!(demo.engine_wakes_per_day <= 60.0);
        // The seed sets the personality, so it sets the dirt rate and the floor
        // steps: the bound holds for each of three.
        for seed in [1, 2, 3] {
            let care = sim::run_with(
                &sim::balanced,
                10,
                Driver::Delta,
                seed,
                Setup {
                    clock: true,
                    ..Default::default()
                },
            )
            .outcome;
            eprintln!(
                "balanced, clock, seed {seed}: {:.2} scene changes a day ({:.2} scene-only), {:.2} engine wakes a day, {:.2} refreshes a day",
                care.scene_changes_per_day,
                care.scene_only_changes_per_day,
                care.engine_wakes_per_day,
                care.refreshes_per_day
            );
            assert!(care.scene_only_changes_per_day <= 6.0, "seed {seed}");
        }
    }

    #[test]
    fn the_cross_shows_whenever_the_pet_is_poorly() {
        super::common::the_cross_shows_whenever_the_pet_is_poorly();
    }
}
