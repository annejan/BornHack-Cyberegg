//! BornPets v3 demo mode tests (plan 3).

mod state {
    use bornhack_aegg::game::engine::demo::{BLOCK_AT, BLOCK_LEN, Demo};
    use bornhack_aegg::game::engine::doses::{Reason, Refusal};
    use bornhack_aegg::game::engine::hint::Remedy;
    use bornhack_aegg::game::engine::needs::P;
    use bornhack_aegg::game::engine::{
        FoodKind, GRADE_WIN, Fought, GameState, MiniGame, PetKind, Phase, SAVE_KEY, SAVE_SIZE,
    };

    fn active() -> GameState {
        let mut s = GameState::new_egg(7, PetKind(0));
        s.phase = Phase::Active;
        s
    }

    fn full_demo() -> Demo {
        Demo {
            on: true,
            soft_armed: true,
            clock_armed: true,
            lazy: 11,
            clock_min: 777,
            whim_min: 650,
            lazy_min: 300,
            next_min: 29,
            pause_min: 30,
            animated: true,
            pet_action: true,
            demo_min: 3 * 60 + 59,
        }
    }

    #[test]
    fn plan2_save_loads_with_demo_off() {
        let mut b = active().to_bytes();
        for x in &mut b[BLOCK_AT..SAVE_SIZE] {
            *x = 0;
        }
        let s = GameState::from_bytes(&b).expect("loads");
        assert!(!s.demo.on);
        assert_eq!(s.demo, Demo::default());
        assert!(!s.is_demo());
    }

    #[test]
    fn demo_block_round_trips() {
        let mut s = active();
        s.demo = full_demo();
        let back = GameState::from_bytes(&s.to_bytes()).unwrap();
        assert_eq!(
            back.demo,
            Demo {
                demo_min: 180,
                ..full_demo()
            }
        );

        s.demo.demo_min = 40_000 * 60;
        let b = s.to_bytes();
        let d = Demo::read(b[BLOCK_AT..BLOCK_AT + BLOCK_LEN].try_into().unwrap());
        assert_eq!(d.demo_min, 32767 * 60);
        assert!(d.pet_action);
    }

    #[test]
    fn tail_after_demo_block_stays_zero() {
        let mut s = active();
        s.demo = Demo {
            on: true,
            soft_armed: true,
            clock_armed: true,
            lazy: 15,
            clock_min: u16::MAX,
            whim_min: u16::MAX,
            lazy_min: u16::MAX,
            next_min: u8::MAX,
            pause_min: u8::MAX,
            animated: true,
            pet_action: true,
            demo_min: u32::MAX,
        };
        let b = s.to_bytes();
        assert!(b[BLOCK_AT + BLOCK_LEN..].iter().all(|&x| x == 0));
        assert_eq!(b[0], 3);
        assert_eq!(b.len(), 177);
    }

    #[test]
    fn demo_never_writes_old_keys() {
        // Behaviour, not a grep: demo state lives only in bytes 149.., the
        // head is the same with demo on or off, and the key stays state_v3.
        assert_eq!(SAVE_KEY, "state_v3");
        let off = active();
        let mut on = off.clone();
        on.demo = full_demo();
        assert_eq!(on.to_bytes()[..BLOCK_AT], off.to_bytes()[..BLOCK_AT]);
        assert_ne!(on.to_bytes()[BLOCK_AT..], off.to_bytes()[BLOCK_AT..]);
    }

    #[test]
    fn demo_turns_on_only_for_a_pet_in_fair_shape() {
        let mut egg = GameState::new_egg(7, PetKind(0));
        assert_eq!(egg.set_demo(true), Err(Reason::Egg));

        let mut s = active();
        s.health = (39 * P) as u16;
        assert_eq!(s.set_demo(true), Err(Reason::Refused(Refusal::FixMeFirst)));
        let mut l = active();
        l.phase = Phase::Leaving;
        assert_eq!(l.set_demo(true), Err(Reason::Refused(Refusal::FixMeFirst)));

        let mut ok = active();
        ok.health = (60 * P) as u16;
        ok.demo.demo_min = 120;
        assert_eq!(ok.set_demo(true), Ok(()));
        assert!(ok.is_demo());
        assert_eq!(ok.demo.lazy_min, 0);
        assert!(ok.demo.whim_min > 0);
        assert_eq!(ok.set_demo(false), Ok(()));
        assert!(!ok.is_demo());
        assert_eq!(ok.demo.demo_min, 120);
        assert_eq!(ok.demo.whim_min, 0);
    }

    #[test]
    fn set_demo_edges() {
        use bornhack_aegg::game::engine::thresholds::{DEMO_ON_HEALTH_PCT, DEMO_WHIM_BASE_MIN};

        // A running action does not block the toggle.
        let mut busy = active();
        assert!(busy.feed(FoodKind::Apple));
        assert!(busy.active_action.is_some());
        assert_eq!(busy.set_demo(true), Ok(()));

        let mut h = active();
        h.hibernating = true;
        assert_eq!(h.set_demo(true), Err(Reason::Hibernating));
        let mut g = active();
        g.phase = Phase::Gone;
        assert_eq!(g.set_demo(true), Err(Reason::Gone));

        // Exactly the line is fair shape; one level under is not.
        let line = DEMO_ON_HEALTH_PCT() as i32 * P;
        let mut edge = active();
        edge.health = line as u16;
        assert_eq!(edge.set_demo(true), Ok(()));
        assert_eq!(edge.demo.whim_min, DEMO_WHIM_BASE_MIN());
        let mut under = active();
        under.health = (line - 1) as u16;
        assert_eq!(
            under.set_demo(true),
            Err(Reason::Refused(Refusal::FixMeFirst))
        );

        // Care mode and whose action runs survive both ways.
        let mut k = active();
        k.demo.animated = true;
        k.demo.pet_action = true;
        k.demo.soft_armed = true;
        assert_eq!(k.set_demo(true), Ok(()));
        assert!(k.demo.animated && k.demo.pet_action && !k.demo.soft_armed);
        assert_eq!(k.set_demo(false), Ok(()));
        assert!(k.demo.animated && k.demo.pet_action);
    }

    #[test]
    fn demo_work_reward_pays() {
        use bornhack_aegg::game::engine::{Action, ONLYPETS_HOBBY_REWARD, doses};
        let mut s = active();
        s.money_enabled = true;
        s.money = 50;
        s.demo.on = true;
        doses::apply(&mut s, Action::OnlyPets).unwrap();
        assert_eq!(s.money, 50 + ONLYPETS_HOBBY_REWARD);

        let mut off = active();
        off.money_enabled = false;
        off.money = 50;
        off.demo.on = true;
        doses::apply(&mut off, Action::OnlyPets).unwrap();
        assert_eq!(off.money, 50, "no money layer, no pay");
    }

    #[test]
    fn demo_keeps_hax_live() {
        use bornhack_aegg::game::engine::MINIGAME_HAX_REWARD;
        use bornhack_aegg::game::engine::thresholds::BATTLE_PRIZE_FIRST_HAX;
        let mut s = active();
        s.money_enabled = true;
        s.money = 50;
        s.demo.on = true;
        assert!(s.feed(FoodKind::Pizza));
        assert_eq!(s.money, 40, "the price is charged in demo");
        s.award_inspiration(MiniGame::BlackHole, GRADE_WIN);
        assert_eq!(s.money, 40 + MINIGAME_HAX_REWARD);
        let before = s.money;
        s.fight_over(Fought { won: true, hp_left_pct: 50, costs: true, paid: true });
        assert_eq!(s.money, before + BATTLE_PRIZE_FIRST_HAX() as u32);
        s.add_money(10);
        assert_eq!(s.money, before + BATTLE_PRIZE_FIRST_HAX() as u32 + 10);
        assert!(s.stats(0).money_enabled);
        assert!(s.stats(0).demo);
    }

    #[test]
    fn demo_without_the_money_layer_stays_moneyless() {
        let mut s = active();
        s.money_enabled = false;
        s.money = 50;
        s.demo.on = true;
        assert!(s.feed(FoodKind::Pizza));
        assert_eq!(s.money, 50);
        s.award_inspiration(MiniGame::BlackHole, GRADE_WIN);
        s.fight_over(Fought { won: true, hp_left_pct: 50, costs: true, paid: true });
        assert_eq!(s.money, 50);
        assert!(!s.stats(0).money_enabled);
    }

    #[test]
    fn broke_demo_pet_is_told_cant_afford() {
        let mut s = active();
        s.money_enabled = true;
        s.money = 0;
        s.fullness = (10 * P) as u16;
        s.demo.on = true;
        assert_eq!(s.remedy_reason(Remedy::Feed), Some(Reason::CantAfford));
        s.demo.on = false;
        assert_eq!(s.remedy_reason(Remedy::Feed), Some(Reason::CantAfford));
    }

    #[test]
    fn self_raised_needs_a_quarter_of_the_life() {
        let mut s = active();
        s.age_ticks = 400 * 6;
        s.demo.demo_min = 100;
        assert!(!s.self_raised());
        s.demo.demo_min = 101;
        assert!(s.self_raised());
        assert!(s.stats(0).self_raised);
    }

    #[test]
    fn fix_me_first_fits() {
        let r = Reason::Refused(Refusal::FixMeFirst);
        assert_eq!(r.text().as_str(), "fix me first");
        assert!(r.text().len() <= 16);
        let (k, s) = r.pack();
        assert_eq!(Reason::unpack(k, s), r);
    }
}

mod tunables {
    use bornhack_aegg::game::engine::thresholds::{BORNPETS_CFG_KEYS, Thresholds, validate};

    fn with(key: &str, v: u32) -> Thresholds {
        let mut t = Thresholds::CLASSIC;
        let set = BORNPETS_CFG_KEYS.iter().find(|(k, _)| *k == key).unwrap().1;
        set(&mut t, v);
        t
    }

    #[test]
    fn presets_validate_unchanged() {
        assert_eq!(
            validate(&Thresholds::CLASSIC, &Thresholds::CLASSIC),
            Thresholds::CLASSIC
        );
        assert_eq!(
            validate(&Thresholds::CASUAL, &Thresholds::CASUAL),
            Thresholds::CASUAL
        );
    }

    #[test]
    fn huge_values_are_clamped_to_their_saved_size() {
        let t = with("DEMO_LAZY_MAX_PCT", 99);
        assert_eq!(t.DEMO_LAZY_MAX_PCT, 15);
        assert_eq!(with("DEMO_PAUSE_MIN", 9999).DEMO_PAUSE_MIN, 255);
        assert_eq!(with("DEMO_BED_AT_MIN", 5000).DEMO_BED_AT_MIN, 1439);
        assert_eq!(with("DEMO_UP_SPREAD_MIN", 5000).DEMO_UP_SPREAD_MIN, 600);
        let r = validate(&with("DEMO_REACT_BASE_MIN", 60000), &Thresholds::CLASSIC);
        assert!(r.DEMO_REACT_BASE_MIN + r.DEMO_REACT_SPREAD_MIN <= 255);
        let w = validate(&with("DEMO_WHIM_SPREAD_MIN", 65535), &Thresholds::CLASSIC);
        assert!(w.DEMO_WHIM_BASE_MIN as u32 + w.DEMO_WHIM_SPREAD_MIN as u32 <= 65535);
        assert_eq!(with("DEMO_DRY_SPREAD_MIN", 0).DEMO_DRY_SPREAD_MIN, 1);
    }

    #[test]
    fn a_wake_line_above_the_eat_line_is_clamped() {
        let ok = validate(&with("DEMO_WAKE_FULL_PCT", 60), &Thresholds::CLASSIC);
        assert_eq!(ok.DEMO_WAKE_FULL_PCT, ok.DEMO_EDGE_FULL_PCT);
        let mut t = Thresholds::CLASSIC;
        t.DEMO_SOFT_FUN_PCT = 90;
        t.DEMO_WIRED_FUN_PCT = 30;
        let ok = validate(&t, &Thresholds::CLASSIC);
        assert!(ok.DEMO_SOFT_FUN_PCT < ok.DEMO_WIRED_FUN_PCT);
        // Ordered values pass untouched.
        assert_eq!(
            validate(&Thresholds::CASUAL, &Thresholds::CASUAL),
            Thresholds::CASUAL
        );
    }

    #[test]
    fn bad_orders_and_sums_fall_back() {
        let mut t = Thresholds::CLASSIC;
        t.DEMO_PICK_APPLE_PCT = 90;
        t.DEMO_PICK_COFFEE_PCT = 90;
        t.DEMO_HELP_HEALTH_PCT = 20;
        t.DEMO_RICH_FOOD_PCT = 70;
        assert_eq!(validate(&t, &Thresholds::CLASSIC), Thresholds::CLASSIC);
    }
}

mod floors {
    use bornhack_aegg::game::engine::advance::advance;
    use bornhack_aegg::game::engine::demo::MINUTES_PER_DAY;
    use bornhack_aegg::game::engine::needs::{MAX, P};
    use bornhack_aegg::game::engine::solver;
    use bornhack_aegg::game::engine::thresholds as t;
    use bornhack_aegg::game::engine::{GameState, PetKind, Phase, TICKS_PER_MINUTE};

    const DAY: u32 = MINUTES_PER_DAY as u32;

    fn pct(p: i32) -> u16 {
        (p * P) as u16
    }

    fn calm(demo: bool) -> GameState {
        let mut s = GameState::new_egg(1, PetKind(0));
        s.phase = Phase::Active;
        s.is_sleeping = false;
        s.fullness = pct(60);
        s.hydration = pct(65);
        s.energy = pct(80);
        s.fun = pct(50);
        s.dirt = pct(20);
        s.booze = pct(30);
        s.strain = pct(20);
        s.fitness = pct(55);
        s.weight = pct(50);
        s.mood = MAX as u16;
        s.health = pct(70);
        s.demo.on = demo;
        s
    }

    fn starving(demo: bool) -> GameState {
        let mut s = calm(demo);
        s.fullness = 0;
        s.hydration = 0;
        s.health = pct(30);
        s
    }

    fn run(s: &mut GameState, minutes: u32) {
        s.update(s.last_update_tick + minutes * TICKS_PER_MINUTE);
    }

    /// The policy is out of the picture: the pet's care is paused for as
    /// long as the run lasts, so only the engine's own floors show.
    fn run_unattended(s: &mut GameState, minutes: u32) {
        const CHUNK: u32 = 200;
        let mut left = minutes;
        while left > 0 {
            s.demo.pause_min = u8::MAX;
            let step = left.min(CHUNK);
            run(s, step);
            left -= step;
        }
        s.demo.pause_min = 0;
    }

    fn fit_floor() -> u16 {
        pct(t::DEMO_FIT_FLOOR_PCT() as i32)
    }

    #[test]
    fn fitness_floor_stops_the_fall_without_raising() {
        let mut s = calm(true);
        s.fitness = pct(50);
        run_unattended(&mut s, 10 * DAY);
        assert_eq!(s.fitness, fit_floor());

        let mut s = calm(true);
        s.fitness = pct(30);
        run_unattended(&mut s, DAY);
        assert_eq!(s.fitness, pct(30), "below the floor: not raised, not lower");

        let mut s = calm(false);
        s.fitness = pct(50);
        run(&mut s, 10 * DAY);
        assert!(s.fitness < fit_floor(), "demo off: no floor");
    }

    #[test]
    fn health_floor_stops_damage() {
        let mut s = starving(true);
        run_unattended(&mut s, 3 * DAY);
        assert_eq!(s.health, pct(t::DEMO_HEALTH_FLOOR_PCT() as i32));
    }

    #[test]
    fn demo_pet_never_leaves() {
        let mut s = starving(true);
        run_unattended(&mut s, 3 * DAY);
        assert_eq!(s.phase, Phase::Active);
        assert_eq!(s.leaving_min, 0);

        let mut s = starving(false);
        run(&mut s, 3 * DAY);
        assert!(matches!(s.phase, Phase::Leaving | Phase::Gone));
    }

    #[test]
    fn demo_never_becomes_dependent() {
        for demo in [true, false] {
            let mut s = calm(demo);
            for _ in 0..(2 * DAY / 30) {
                s.booze = pct(80);
                run(&mut s, 30);
            }
            assert_eq!(s.dependent, !demo, "demo {demo}");
        }
        let mut s = calm(true);
        s.booze = pct(80);
        s.dependence_min = u32::MAX / 2;
        run(&mut s, 60);
        assert!(!s.dependent, "a counter past the line is not onset");

        // Three days of drinking in demo leave the counter alone, so
        // switching demo off does not make the pet dependent.
        let mut s = calm(true);
        s.dependence_min = 7;
        for _ in 0..(3 * DAY / 30) {
            s.booze = pct(80);
            run(&mut s, 30);
        }
        assert_eq!(s.dependence_min, 7);
        s.demo.on = false;
        s.booze = pct(80);
        run(&mut s, 30);
        assert!(!s.dependent);
    }

    #[test]
    fn demo_time_is_not_care_time() {
        let mut s = calm(true);
        s.care_awake_min = 5;
        s.care_good_min = 3;
        let before = s.demo.demo_min;
        run(&mut s, 600);
        assert_eq!((s.care_awake_min, s.care_good_min), (5, 3));
        assert_eq!(s.demo.demo_min, before + 600);

        let mut s = calm(false);
        run(&mut s, 600);
        assert!(s.care_awake_min > 0);
        assert_eq!(s.demo.demo_min, 0);
    }

    #[test]
    fn floors_are_segment_lines() {
        // A few minutes above the floor, so the floor is the nearest event
        // (the other needs bound this state at about 40 minutes).
        let mut s = calm(true);
        s.booze = 0;
        s.fitness = fit_floor() + 10;
        let len = solver::seg_len(&s, 100_000);
        let mut oracle = s.clone();
        let mut to_floor = 0;
        while oracle.fitness > fit_floor() && to_floor <= len + 1 {
            advance(&mut oracle, 1);
            to_floor += 1;
        }
        assert_eq!(oracle.fitness, fit_floor(), "the oracle reaches the floor");
        assert!(to_floor > 2, "the fall is slow enough to test");
        assert_eq!(len, to_floor - 1, "stops one minute before the floor");
        let mut seg = s.clone();
        advance(&mut seg, len);
        assert!(seg.fitness > fit_floor());
        s.demo.on = false;
        assert!(
            solver::seg_len(&s, 100_000) > len,
            "without demo there is no floor line"
        );
    }

    #[test]
    fn health_floor_is_a_sig_line() {
        let mut a = starving(true);
        let mut b = a.clone();
        a.health = pct(t::DEMO_HEALTH_FLOOR_PCT() as i32) + 1;
        b.health = pct(t::DEMO_HEALTH_FLOOR_PCT() as i32);
        assert_ne!(solver::sig(&a), solver::sig(&b));
        // The help line is "under": the flip is between the line and one
        // level below it.
        a.health = pct(t::DEMO_HELP_HEALTH_PCT() as i32);
        b.health = pct(t::DEMO_HELP_HEALTH_PCT() as i32) - 1;
        assert_ne!(solver::sig(&a), solver::sig(&b));
    }

    #[test]
    fn pinned_on_the_floor_takes_long_segments() {
        // Without the skip a pet resting on the floor gets 1-minute segments
        // (the floor line is "already reached"); it is bounded by the other
        // needs only, as with demo off (no 45 % line, the 43 % edge is far).
        let mut s = calm(true);
        s.booze = 0;
        s.fitness = fit_floor();
        let pinned = solver::seg_len(&s, 100_000);
        s.demo.on = false;
        assert_eq!(pinned, solver::seg_len(&s, 100_000));
        assert!(pinned > 1);
    }

    #[test]
    fn health_zero_demo_pet_comes_back() {
        let mut start = calm(true);
        start.health = 0;
        start.phase = Phase::Leaving;
        start.leaving_min = 100;
        let mut whole = start.clone();
        run(&mut whole, 60);
        assert_eq!(whole.phase, Phase::Leaving);
        assert_eq!(whole.leaving_min, 40, "counts down, not up");
        let mut ones = start.clone();
        for _ in 0..60 {
            run(&mut ones, 1);
        }
        assert_eq!(whole.to_bytes(), ones.to_bytes());
        run(&mut whole, 60);
        assert_eq!(whole.phase, Phase::Active);
        assert_eq!(whole.leaving_min, 0);
        run(&mut whole, DAY);
        assert_ne!(whole.phase, Phase::Gone);
    }

    #[test]
    fn floors_hold_across_splits() {
        for start in [
            starving(true),
            calm(true),
            {
                let mut s = calm(true);
                s.fitness = fit_floor() + 20;
                s
            },
            {
                let mut s = calm(true);
                s.fitness = pct(30);
                s.health = pct(20);
                s
            },
        ] {
            let mut whole = start.clone();
            run(&mut whole, 3 * DAY);
            let mut steps = start.clone();
            for _ in 0..(3 * DAY / 37) {
                run(&mut steps, 37);
            }
            run(&mut steps, 3 * DAY - 37 * (3 * DAY / 37));
            let mut single = start.clone();
            for _ in 0..(6 * 60) {
                run(&mut single, 1);
            }
            let mut whole6 = start.clone();
            run(&mut whole6, 6 * 60);
            assert_eq!(
                whole.to_bytes(),
                steps.to_bytes(),
                "3 days, 37-minute steps"
            );
            assert_eq!(
                whole6.to_bytes(),
                single.to_bytes(),
                "6 hours, 1-minute steps"
            );
            assert!(whole.fitness >= fit_floor().min(start.fitness));
            assert!(whole.health >= pct(t::DEMO_HEALTH_FLOOR_PCT() as i32).min(start.health));
        }
    }

    /// The demo events (lazy re-rolls, soft delays, the clock, whims) fire
    /// at boundaries the solver computes: one long advance and many small
    /// ones leave the same save, the same dice and the same pending choice.
    #[test]
    fn events_hold_across_splits() {
        use bornhack_aegg::game::engine::demo;
        let at = |s: &mut GameState, minute: u16| s.sync_clock(s.last_update_tick, Some(minute));
        let mut starts: Vec<(&str, GameState)> = Vec::new();
        for (name, minute) in [
            ("none", None),
            ("evening", Some(20 * 60)),
            ("night", Some(60)),
        ] {
            for (kind, asleep, energy, fullness) in [
                ("rested", false, 90, 70),
                ("tired", false, 40, 70),
                ("hungry", false, 70, 30),
                ("asleep", true, 55, 70),
                ("asleep, rested", true, 98, 70),
            ] {
                let mut s = calm(true);
                s.is_sleeping = asleep;
                s.energy = pct(energy);
                s.fullness = pct(fullness);
                s.demo.whim_min = 90;
                s.demo.lazy_min = 200;
                s.sync_clock(s.last_update_tick, minute);
                starts.push((Box::leak(format!("{name} {kind}").into_boxed_str()), s));
            }
        }
        let mut late = calm(true);
        run(&mut late, 3 * 60);
        at(&mut late, 20 * 60);
        starts.push(("clock set late", late));
        for (name, start) in starts {
            for (total, step) in [(3 * DAY, 37), (DAY, 11), (6 * 60, 1)] {
                let mut whole = start.clone();
                run(&mut whole, total);
                let mut split = start.clone();
                for _ in 0..total / step {
                    run(&mut split, step);
                }
                run(&mut split, total - step * (total / step));
                assert_eq!(
                    whole.to_bytes(),
                    split.to_bytes(),
                    "{name}: {total} min by {step}"
                );
                assert_eq!(demo::decide(&whole), demo::decide(&split), "{name}: choice");
                assert_eq!(demo::timers(&whole), demo::timers(&split), "{name}: timers");
                assert_eq!(
                    whole.next_wake_tick(),
                    split.next_wake_tick(),
                    "{name}: wake"
                );
            }
        }
    }

    /// The torn-muscle dose takes 5 % health. A demo pet on the floor keeps
    /// it; a pet without demo falls through.
    #[test]
    fn injury_dose_does_not_break_the_health_floor() {
        let floor = pct(t::DEMO_HEALTH_FLOOR_PCT() as i32);
        for (demo, holds) in [(true, true), (false, false)] {
            let mut s = calm(demo);
            // Close enough above the floor that the dose would cross it, and
            // far enough from the ceiling that no recovery masks the fall.
            s.health = floor + pct(2);
            s.strain = pct(95);
            assert!(s.exercise(), "demo {demo}: the session started");
            run(&mut s, 30);
            assert!(s.injured_min > 0, "demo {demo}: the muscle tore");
            assert_eq!(s.health >= floor, holds, "demo {demo}: health {}", s.health);
        }
    }

    #[test]
    fn relax_dose_does_not_break_the_floor() {
        for (demo, holds) in [(true, true), (false, false)] {
            let mut s = calm(demo);
            s.fitness = fit_floor();
            assert!(s.relax());
            run(&mut s, 30);
            assert_eq!(s.fitness >= fit_floor(), holds, "demo {demo}");
        }
    }
}

mod policy {
    use bornhack_aegg::game::engine::advance::advance;
    use bornhack_aegg::game::engine::demo::{self, Choice};
    use bornhack_aegg::game::engine::needs::{MAX, P};
    use bornhack_aegg::game::engine::thresholds as t;
    use bornhack_aegg::game::engine::{
        DrinkKind, FoodKind, GameState, MiniGame, PetKind, Phase, TICKS_PER_MINUTE,
    };

    const HOUR: u16 = 60;

    pub(super) fn pct(p: i32) -> u16 {
        (p * P) as u16
    }

    /// A demo pet with every line quiet: nothing to choose.
    pub(super) fn quiet() -> GameState {
        let mut s = GameState::new_egg(7, PetKind(0));
        s.phase = Phase::Active;
        s.is_sleeping = false;
        s.money_enabled = false;
        s.fullness = pct(70);
        s.hydration = pct(70);
        s.energy = pct(80);
        s.fun = pct(60);
        s.dirt = pct(20);
        s.booze = pct(30);
        s.strain = pct(20);
        s.fitness = pct(55);
        s.weight = pct(50);
        s.health = pct(80);
        s.mood = MAX as u16;
        s.demo.on = true;
        s.demo.whim_min = 100;
        s.demo.lazy_min = 100;
        s.demo.lazy = 0;
        s.set_demo_rng(10);
        s
    }

    pub(super) fn at(s: &mut GameState, minute_of_day: u16) {
        s.sync_clock(s.last_update_tick, Some(minute_of_day));
    }

    pub(super) fn soft(s: &mut GameState) {
        s.demo.soft_armed = true;
        s.demo.next_min = 0;
    }

    #[test]
    fn decide_follows_the_table() {
        assert_eq!(demo::decide(&quiet()), None);
        // Each case: the one row made true, and what the pet does (rng 10:
        // rng % 100 is 10, % 5 is 0, % 3 is 1).
        type Make = fn(&mut GameState);
        let cases: [(&str, Make, Option<Choice>); 20] = [
            (
                "1 wake on the clock",
                |s| {
                    s.is_sleeping = true;
                    s.demo.clock_armed = true;
                    s.demo.clock_min = 0;
                },
                Some(Choice::Wake),
            ),
            (
                "1 wake on hunger",
                |s| {
                    s.is_sleeping = true;
                    s.fullness = pct(t::DEMO_WAKE_FULL_PCT() as i32 - 1);
                },
                Some(Choice::Wake),
            ),
            ("2 asleep", |s| s.is_sleeping = true, None),
            (
                "3 insulin",
                |s| {
                    s.diabetic = true;
                    s.insulin_min = 0;
                },
                Some(Choice::Insulin),
            ),
            (
                "4 rehab",
                |s| {
                    s.dependent = true;
                    s.rehab_min = 0;
                },
                Some(Choice::Rehab),
            ),
            (
                "5 hungry, light pet: pizza",
                |s| s.fullness = pct(t::DEMO_EDGE_FULL_PCT() as i32 - 1),
                Some(Choice::Eat(FoodKind::Pizza)),
            ),
            (
                "5 hungry, heavy pet: apple",
                |s| {
                    s.fullness = pct(t::DEMO_EDGE_FULL_PCT() as i32 - 1);
                    s.weight = pct(t::DEMO_LIGHT_FOOD_PCT() as i32 + 2);
                },
                Some(Choice::Eat(FoodKind::Apple)),
            ),
            (
                "6 thirsty",
                |s| s.hydration = pct(t::DEMO_EDGE_WATER_PCT() as i32 - 1),
                Some(Choice::Drink(DrinkKind::Water)),
            ),
            (
                "7 tired",
                |s| s.energy = pct(t::DEMO_EDGE_ENERGY_PCT() as i32 - 1),
                Some(Choice::Sleep),
            ),
            (
                "8 unwell",
                |s| s.health = pct(t::DEMO_MED_HEALTH_PCT() as i32 - 1),
                Some(Choice::Medicine),
            ),
            (
                "9 drunk, delay done",
                |s| {
                    s.booze = pct(t::DEMO_EDGE_BOOZE_PCT() as i32 + 1);
                    soft(s);
                },
                Some(Choice::Drink(DrinkKind::Water)),
            ),
            (
                "9 drunk, delay running: the rest waits",
                |s| {
                    s.booze = pct(t::DEMO_EDGE_BOOZE_PCT() as i32 + 1);
                    s.demo.clock_armed = true;
                    s.demo.clock_min = 0;
                    s.demo.whim_min = 0;
                },
                None,
            ),
            (
                "10 bedtime",
                |s| {
                    s.demo.clock_armed = true;
                    s.demo.clock_min = 0;
                },
                Some(Choice::Sleep),
            ),
            (
                "11 wired",
                |s| s.fun = pct(t::DEMO_WIRED_FUN_PCT() as i32 + 2),
                Some(Choice::Relax),
            ),
            (
                "12 soft thirst",
                |s| {
                    s.hydration = pct(t::DEMO_SOFT_WATER_PCT() as i32 - 3);
                    soft(s);
                },
                Some(Choice::Drink(DrinkKind::Water)),
            ),
            (
                "12 soft hunger",
                |s| {
                    s.fullness = pct(t::DEMO_SOFT_FULL_PCT() as i32 - 3);
                    soft(s);
                },
                Some(Choice::Eat(FoodKind::Apple)),
            ),
            (
                "12 soft dirt",
                |s| {
                    s.dirt = pct(t::DEMO_SOFT_DIRT_PCT() as i32 + 3);
                    soft(s);
                },
                Some(Choice::Wash),
            ),
            (
                "12 soft boredom: play",
                |s| {
                    s.fun = pct(t::DEMO_SOFT_FUN_PCT() as i32 - 3);
                    s.set_demo_rng(90);
                    soft(s);
                },
                Some(Choice::Play),
            ),
            (
                "12 soft boredom: a game",
                |s| {
                    s.fun = pct(t::DEMO_SOFT_FUN_PCT() as i32 - 3);
                    soft(s);
                },
                Some(Choice::Game(MiniGame::BlackHole, 1)),
            ),
            (
                "13 whim: beer",
                |s| {
                    s.booze = pct(t::DEMO_WHIM_BOOZE_LO_PCT() as i32 - 5);
                    s.demo.whim_min = 0;
                },
                Some(Choice::Drink(DrinkKind::Beer)),
            ),
        ];
        for (name, make, want) in cases {
            let mut s = quiet();
            make(&mut s);
            assert_eq!(demo::decide(&s), want, "{name}");
        }
        // The cake and the nap whims, and "soft energy" without a clock.
        let mut s = quiet();
        s.booze = pct(t::DEMO_WHIM_BOOZE_LO_PCT() as i32 - 5);
        s.demo.whim_min = 0;
        s.set_demo_rng(60);
        assert_eq!(
            demo::decide(&s),
            Some(Choice::Eat(FoodKind::Cake)),
            "13 whim: cake"
        );
        let mut s = quiet();
        s.booze = pct(t::DEMO_WHIM_BOOZE_HI_PCT() as i32 + 5);
        s.energy = pct(t::DEMO_NAP_ENERGY_PCT() as i32 - 5);
        s.fun = pct(t::DEMO_NAP_FUN_PCT() as i32 - 5);
        s.demo.whim_min = 0;
        assert_eq!(demo::decide(&s), Some(Choice::Nap), "13 whim: nap");
        let mut s = quiet();
        s.energy = pct(t::DEMO_SOFT_ENERGY_PCT() as i32 - 5);
        soft(&mut s);
        assert_eq!(
            demo::decide(&s),
            Some(Choice::Sleep),
            "12 soft energy, own rhythm"
        );
        at(&mut s, 12 * HOUR);
        assert_eq!(demo::decide(&s), None, "12 soft energy waits for the clock");
    }

    /// A small deterministic generator for the seeded state tests.
    pub(super) fn lcg(seed: u64) -> impl FnMut(u64) -> u64 {
        let mut x = seed;
        move |bound| {
            x = x
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (x >> 33) % (bound + 1)
        }
    }

    pub(super) fn random_demo(next: &mut impl FnMut(u64) -> u64) -> GameState {
        let mut s = quiet();
        let mut level = |n: &mut dyn FnMut(u64) -> u64| pct(n(100) as i32);
        s.fullness = level(next);
        s.hydration = level(next);
        s.energy = level(next);
        s.fun = level(next);
        s.dirt = level(next);
        s.booze = level(next);
        s.fitness = level(next);
        s.weight = level(next);
        s.health = level(next);
        s.is_sleeping = next(1) == 1;
        s.diabetic = next(3) == 0;
        s.dependent = next(3) == 0;
        s.insulin_min = if next(1) == 1 { 0 } else { next(300) as u16 };
        s.rehab_min = if next(1) == 1 { 0 } else { next(300) as u16 };
        s.injured_min = if next(2) == 0 { next(100) as u16 } else { 0 };
        s.med_cd_min = if next(2) == 0 { next(100) as u16 } else { 0 };
        s.cooldown_tictactoe = if next(2) == 0 { next(600) as u16 } else { 0 };
        s.cooldown_bornjeweled = if next(2) == 0 { next(600) as u16 } else { 0 };
        s.cooldown_blackhole = if next(2) == 0 { next(600) as u16 } else { 0 };
        s.demo.lazy = next(15) as u8;
        s.demo.soft_armed = next(1) == 1;
        s.demo.next_min = if next(1) == 1 { 0 } else { next(30) as u8 };
        s.demo.clock_armed = next(1) == 1;
        s.demo.clock_min = if next(1) == 1 { 0 } else { next(200) as u16 };
        s.demo.whim_min = if next(1) == 1 { 0 } else { next(200) as u16 };
        s.demo.pause_min = 0;
        s.set_demo_rng(1 + next(1_000_000) as u32);
        s
    }

    /// `random_demo`, some groggy, some with caffeine (a separate draw, so
    /// the older tests keep their pets).
    pub(super) fn random_coffee_demo(next: &mut impl FnMut(u64) -> u64) -> GameState {
        let mut s = random_demo(next);
        s.groggy_min = if next(3) == 0 { next(120) as u16 } else { 0 };
        s.caffeine_min = if next(1) == 1 { 0 } else { next(600) as u16 };
        s
    }

    /// Coffee perks a groggy pet up, but the pet never takes the cup that
    /// would wire it: not for grogginess, not for thirst.
    #[test]
    fn a_groggy_pet_has_a_coffee_but_never_the_one_that_wires_it() {
        use bornhack_aegg::game::engine::doses::caffeine_safe_min;
        let safe = caffeine_safe_min() as u16;
        let mut s = quiet();
        s.groggy_min = 60;
        assert_eq!(demo::decide(&s), Some(Choice::Drink(DrinkKind::Coffee)));
        s.caffeine_min = safe;
        assert_eq!(demo::decide(&s), Some(Choice::Drink(DrinkKind::Coffee)));
        s.caffeine_min = safe + 1;
        assert_eq!(demo::decide(&s), None, "one more cup would wire it");

        // Thirsty and tipsy (rng 80): the draw is coffee, or water over the line.
        let mut s = quiet();
        s.hydration = pct(t::DEMO_SOFT_WATER_PCT() as i32 - 3);
        soft(&mut s);
        s.set_demo_rng(80);
        assert_eq!(demo::decide(&s), Some(Choice::Drink(DrinkKind::Coffee)));
        s.caffeine_min = safe + 1;
        assert_eq!(demo::decide(&s), Some(Choice::Drink(DrinkKind::Water)));
    }

    /// Demo pets left alone for days, woken at every engine wake: caffeine
    /// never goes past the wired line and no cup wires a pet, though they do
    /// drink coffee.
    #[test]
    fn a_demo_pet_never_wires_itself_on_coffee() {
        use bornhack_aegg::game::engine::causes::Cause;
        use bornhack_aegg::game::engine::doses::caffeine_safe_min;
        let line = caffeine_safe_min() as u16 + t::COFFEE_WINDOW_MIN();
        let mut next = lcg(77);
        let mut cups = 0;
        for _ in 0..40 {
            let mut s = random_coffee_demo(&mut next);
            let end = s.last_update_tick + 3 * 1440 * TICKS_PER_MINUTE;
            while s.last_update_tick < end {
                let before = s.caffeine_min;
                let to = s.next_wake_tick().max(s.last_update_tick + 1).min(end);
                s.update(to);
                if s.caffeine_min > before {
                    cups += 1;
                }
                assert!(s.caffeine_min <= line.max(before), "caffeine {}", s.caffeine_min);
                assert!(!s.take_causes().iter().any(|&c| c == Cause::CoffeeWired));
            }
        }
        assert!(cups > 10, "only {cups} cups of coffee");
    }

    #[test]
    fn demo_never_takes_a_refused_choice() {
        let mut next = lcg(11);
        let mut seen = 0;
        for i in 0..2000 {
            let s = random_demo(&mut next);
            let Some(choice) = demo::decide(&s) else {
                continue;
            };
            seen += 1;
            let mut c = s.clone();
            let ok = match choice {
                Choice::Eat(f) => c.feed(f),
                Choice::Drink(d) => c.drink(d),
                Choice::Sleep | Choice::Nap => c.sleep(),
                Choice::Wake => c.wake(),
                Choice::Wash => c.wash(),
                Choice::Play => c.play(),
                Choice::Game(g, _) => c.game_refusal(g).is_none(),
                Choice::Relax => c.relax(),
                Choice::Medicine => c.heal(),
                Choice::Insulin => c.medicate(),
                Choice::Rehab => c.rehab(),
                Choice::Work => c.only_pets(),
            };
            assert!(ok, "state {i}: {choice:?} would be refused");
        }
        assert!(seen > 1000, "only {seen} of 2000 states chose something");
    }

    #[test]
    fn decide_waits_for_the_player_and_for_actions() {
        let mut s = quiet();
        s.fullness = pct(5);
        assert!(demo::decide(&s).is_some());
        let mut paused = s.clone();
        paused.demo.pause_min = 5;
        assert_eq!(demo::decide(&paused), None);
        let mut busy = s.clone();
        assert!(busy.play());
        assert_eq!(demo::decide(&busy), None);
        let mut off = s.clone();
        off.demo.on = false;
        assert_eq!(demo::decide(&off), None);
        let mut hibernating = s.clone();
        hibernating.hibernating = true;
        assert_eq!(demo::decide(&hibernating), None);
    }

    #[test]
    fn a_heavy_pet_eats_light() {
        let heavy = pct(t::DEMO_LIGHT_FOOD_PCT() as i32 + 4);
        for rng in 0..100u32 {
            let mut s = quiet();
            s.weight = heavy;
            s.fullness = pct(t::DEMO_SOFT_FULL_PCT() as i32 - 3);
            soft(&mut s);
            s.set_demo_rng(rng);
            match demo::decide(&s) {
                Some(Choice::Eat(FoodKind::Apple | FoodKind::Salad)) => {}
                other => panic!("rng {rng}: {other:?}"),
            }
            let mut whim = quiet();
            whim.weight = heavy;
            whim.booze = pct(t::DEMO_WHIM_BOOZE_LO_PCT() as i32 - 5);
            whim.demo.whim_min = 0;
            whim.set_demo_rng(rng);
            assert_ne!(
                demo::decide(&whim),
                Some(Choice::Eat(FoodKind::Cake)),
                "rng {rng}"
            );
        }
        // A thin pet gets rich food instead of apples and salad.
        for rng in 0..100u32 {
            let mut s = quiet();
            s.weight = pct(t::DEMO_RICH_FOOD_PCT() as i32 - 4);
            s.fullness = pct(t::DEMO_SOFT_FULL_PCT() as i32 - 3);
            soft(&mut s);
            s.set_demo_rng(rng);
            match demo::decide(&s) {
                Some(Choice::Eat(FoodKind::Apple | FoodKind::Salad)) => {
                    panic!("rng {rng}: a thin pet ate light")
                }
                Some(Choice::Eat(_)) => {}
                other => panic!("rng {rng}: {other:?}"),
            }
        }
    }

    #[test]
    fn bedtime_follows_the_clock() {
        let bed = t::DEMO_BED_AT_MIN();
        let up = t::DEMO_UP_AT_MIN();
        // 21:00 awake, energy 60: bed in 60..=150 minutes.
        let mut s = quiet();
        s.energy = pct(60);
        at(&mut s, 21 * HOUR);
        demo::settle(&mut s);
        assert!(s.demo.clock_armed);
        let want = (bed - 21 * HOUR)..=(bed - 21 * HOUR + t::DEMO_BED_SPREAD_MIN());
        assert!(want.contains(&s.demo.clock_min), "{}", s.demo.clock_min);
        // 01:00 awake and tired: bed at once.
        let mut s = quiet();
        s.energy = pct(60);
        at(&mut s, HOUR);
        demo::settle(&mut s);
        assert_eq!((s.demo.clock_armed, s.demo.clock_min), (true, 0));
        assert_eq!(demo::decide(&s), Some(Choice::Sleep));
        // 01:00 awake at full energy: the next evening, not the next minutes.
        let mut s = quiet();
        s.energy = pct(98);
        at(&mut s, HOUR);
        demo::settle(&mut s);
        let to_bed = (up - HOUR) + (bed - up);
        let want = to_bed..=to_bed + t::DEMO_BED_SPREAD_MIN();
        assert!(want.contains(&s.demo.clock_min), "{}", s.demo.clock_min);
        assert_eq!(demo::decide(&s), None);
        // 23:00 asleep: up at 07:00 to 08:30.
        let mut s = quiet();
        s.is_sleeping = true;
        s.energy = pct(60);
        at(&mut s, 23 * HOUR);
        demo::settle(&mut s);
        let to_up = 24 * HOUR - 23 * HOUR + up;
        let want = to_up..=to_up + t::DEMO_UP_SPREAD_MIN();
        assert!(want.contains(&s.demo.clock_min), "{}", s.demo.clock_min);
        // A daytime sleep with no whim nap behind it: the edge nap.
        let mut s = quiet();
        s.is_sleeping = true;
        at(&mut s, 14 * HOUR);
        demo::settle(&mut s);
        assert_eq!(s.demo.clock_min, t::DEMO_EDGE_NAP_MIN());
    }

    #[test]
    fn without_a_clock_the_pet_keeps_its_own_rhythm() {
        // Awake, no clock: the clock never arms; tiredness arms the soft delay.
        let mut s = quiet();
        s.energy = pct(t::DEMO_SOFT_ENERGY_PCT() as i32 - 5);
        demo::settle(&mut s);
        assert!(!s.demo.clock_armed);
        assert!(s.demo.soft_armed);
        let delay = s.demo.next_min as u16;
        let base = t::DEMO_REACT_BASE_MIN();
        assert!((base..=base + t::DEMO_REACT_SPREAD_MIN()).contains(&delay));
        s.demo.next_min = 0;
        assert_eq!(demo::decide(&s), Some(Choice::Sleep));
        // Asleep and rested: up in 10..=120 minutes, then Wake.
        let mut s = quiet();
        s.is_sleeping = true;
        s.energy = pct(t::DEMO_RISE_ENERGY_PCT() as i32 + 1);
        demo::settle(&mut s);
        assert!(s.demo.clock_armed);
        let base = t::DEMO_RISE_BASE_MIN();
        assert!((base..=base + t::DEMO_RISE_SPREAD_MIN()).contains(&s.demo.clock_min));
        assert_eq!(demo::decide(&s), None);
        s.demo.clock_min = 0;
        assert_eq!(demo::decide(&s), Some(Choice::Wake));
        // Asleep and still tired: nothing armed.
        let mut s = quiet();
        s.is_sleeping = true;
        s.energy = pct(40);
        demo::settle(&mut s);
        assert!(!s.demo.clock_armed);
    }

    #[test]
    fn setting_the_clock_late_arms_the_bedtime() {
        let mut s = quiet();
        s.energy = pct(70);
        demo::settle(&mut s);
        assert!(!s.demo.clock_armed);
        s.update(s.last_update_tick + 3 * 60 * TICKS_PER_MINUTE);
        s.sync_clock(s.last_update_tick, Some(20 * HOUR));
        demo::settle(&mut s);
        assert!(s.demo.clock_armed);
        let to_bed = t::DEMO_BED_AT_MIN() - 20 * HOUR;
        assert!((to_bed..=to_bed + t::DEMO_BED_SPREAD_MIN()).contains(&s.demo.clock_min));
    }

    #[test]
    fn the_clock_minute_is_stored_at_the_last_update() {
        let mut s = quiet();
        let t0 = s.last_update_tick;
        // 10 minutes after the last update it is 00:05: it was 23:55 then.
        s.sync_clock(t0 + 10 * TICKS_PER_MINUTE, Some(5));
        s.update(t0 + 10 * TICKS_PER_MINUTE);
        let mut c = quiet();
        c.sync_clock(t0, Some(23 * HOUR + 55));
        c.update(t0 + 10 * TICKS_PER_MINUTE);
        assert_eq!(s.to_bytes(), c.to_bytes());
    }

    /// Everything the player sees, with the demo choice bit masked out.
    fn seen(s: &GameState) -> String {
        format!(
            "{:?}",
            (
                bornhack_aegg::game::engine::status::status_line(s),
                s.display_anim(),
                s.is_sleeping,
                s.phase,
                s.active_action,
                s.onset_bits(),
                demo::wake_view(s) & !1,
            )
        )
    }

    #[test]
    fn settle_never_changes_what_the_player_sees() {
        let mut next = lcg(5);
        for i in 0..2000 {
            let mut s = random_demo(&mut next);
            s.demo.lazy_min = if next(1) == 1 { 0 } else { 100 };
            s.demo.clock_armed = false;
            if next(1) == 1 {
                at(&mut s, next(1439) as u16);
            }
            let before = seen(&s);
            demo::settle(&mut s);
            assert_eq!(before, seen(&s), "state {i}");
        }
    }

    #[test]
    fn settle_is_idempotent() {
        let mut next = lcg(9);
        for i in 0..500 {
            let mut s = random_demo(&mut next);
            demo::settle(&mut s);
            let once = s.to_bytes();
            demo::settle(&mut s);
            assert_eq!(once, s.to_bytes(), "state {i}");
        }
    }

    #[test]
    fn timers_name_what_is_running() {
        let mut s = quiet();
        s.demo.clock_armed = false;
        s.demo.clock_min = 50;
        s.demo.soft_armed = false;
        s.demo.next_min = 7;
        s.demo.whim_min = 0;
        s.demo.lazy_min = 9;
        s.demo.pause_min = 3;
        s.cooldown_blackhole = 7; // a minute and a bit: two minutes to go
        let tm = demo::timers(&s);
        assert_eq!(tm[0], u32::MAX, "an unarmed clock is not running");
        assert_eq!(tm[1], u32::MAX, "whim at 0");
        assert_eq!(tm[2], 9);
        assert_eq!(tm[3], u32::MAX, "a soft delay that is not armed");
        assert_eq!(tm[4], 3);
        assert!(tm.contains(&2), "the game cooldown remainder: {tm:?}");
        s.demo.clock_armed = true;
        s.demo.soft_armed = true;
        let tm = demo::timers(&s);
        assert_eq!((tm[0], tm[3]), (50, 7));
        s.demo.on = false;
        assert!(demo::timers(&s).iter().all(|&m| m == u32::MAX));
    }

    #[test]
    fn beer_stops_while_a_carried_in_counter_is_high() {
        use bornhack_aegg::game::engine::advance::SLOW_SCALE;
        use bornhack_aegg::game::engine::solver;
        let stop = t::DEMO_BEER_STOP_MIN() as u32 * SLOW_SCALE;
        let mut s = quiet();
        s.booze = 0;
        s.demo.whim_min = 0;
        s.dependence_min = stop + 20;
        // Rng 10 would take the beer (10 < the beer chance): the counter says no.
        assert_ne!(demo::decide(&s), Some(Choice::Drink(DrinkKind::Beer)));
        // The counter decays 1 per minute while sober: 20 minutes to the line.
        assert_eq!(demo::timers(&s)[demo::T_DEPENDENCE], 20);
        assert!(solver::seg_len(&s, 1000) <= 20);
        let mut at_line = s.clone();
        // `advance` alone: the pet's own whim would act at the boundary.
        advance(&mut at_line, 19);
        assert_ne!(demo::decide(&at_line), Some(Choice::Drink(DrinkKind::Beer)));
        advance(&mut at_line, 1);
        assert_eq!(at_line.dependence_min, stop);
        assert_eq!(demo::decide(&at_line), Some(Choice::Drink(DrinkKind::Beer)));
        // A drunk pet's counter does not decay: no timer.
        let mut drunk = s.clone();
        drunk.booze = pct(t::BOOZE_CLEAR_PCT() as i32 + 5);
        assert_eq!(demo::timers(&drunk)[demo::T_DEPENDENCE], u32::MAX);
    }

    #[test]
    fn the_soft_delay_follows_sober_and_drunk() {
        use bornhack_aegg::platform::xorshift32;
        let spread = |base: u16, spread: u16, rng: u32| base as u32 + rng % (spread as u32 + 1);
        let mut s = quiet();
        s.hydration = pct(t::DEMO_SOFT_WATER_PCT() as i32 - 3);
        let r0 = 12_345;
        s.set_demo_rng(r0);
        demo::settle(&mut s);
        let react = spread(t::DEMO_REACT_BASE_MIN(), t::DEMO_REACT_SPREAD_MIN(), r0);
        assert_eq!(s.demo.next_min as u32, react);
        // Drunk with the delay running: re-armed as the dry-out interval.
        s.booze = pct(t::DEMO_EDGE_BOOZE_PCT() as i32 + 1);
        demo::settle(&mut s);
        let r1 = xorshift32(r0);
        let dry = spread(t::DEMO_DRY_BASE_MIN(), t::DEMO_DRY_SPREAD_MIN(), r1);
        assert_eq!(s.demo.next_min as u32, dry);
        // Settling again changes nothing.
        let again = s.to_bytes();
        demo::settle(&mut s);
        assert_eq!(again, s.to_bytes());
        // Sober again: back to a reaction delay.
        s.booze = pct(t::DEMO_EDGE_BOOZE_PCT() as i32 - 20);
        demo::settle(&mut s);
        let r2 = xorshift32(r1);
        let react = spread(t::DEMO_REACT_BASE_MIN(), t::DEMO_REACT_SPREAD_MIN(), r2);
        assert_eq!(s.demo.next_min as u32, react);
    }

    #[test]
    fn health_at_the_line_is_below_it() {
        // The solver's line is `health > MED`: on the line counts as below.
        let med = pct(t::DEMO_MED_HEALTH_PCT() as i32);
        let mut s = quiet();
        s.health = med;
        assert_eq!(demo::decide(&s), Some(Choice::Medicine));
        s.health = med + 1;
        assert_eq!(demo::decide(&s), None);
    }

    #[test]
    fn an_unarmed_clock_does_not_count() {
        let mut s = quiet();
        s.demo.clock_armed = false;
        s.demo.clock_min = 50;
        s.update(s.last_update_tick + 10 * TICKS_PER_MINUTE);
        assert_eq!(s.demo.clock_min, 50);
        s.demo.clock_armed = true;
        s.update(s.last_update_tick + 10 * TICKS_PER_MINUTE);
        assert_eq!(s.demo.clock_min, 40);
    }

    #[test]
    fn a_game_on_cooldown_is_skipped_for_the_next_one() {
        let mut s = quiet();
        s.fun = pct(t::DEMO_SOFT_FUN_PCT() as i32 - 3);
        soft(&mut s);
        s.set_demo_rng(5);
        s.cooldown_tictactoe = 12;
        match demo::decide(&s) {
            Some(Choice::Game(g, _)) => assert_ne!(g, MiniGame::TicTacToe),
            other => panic!("{other:?}"),
        }
        for g in MiniGame::ALL {
            s.award_inspiration(g, 1);
        }
        assert_eq!(s.demo.pause_min, t::DEMO_PAUSE_MIN(), "the player played");
        s.demo.pause_min = 0;
        s.fun = pct(t::DEMO_SOFT_FUN_PCT() as i32 - 3);
        assert_eq!(
            demo::decide(&s),
            Some(Choice::Play),
            "every game cooling down"
        );
    }
}

mod wake {
    use bornhack_aegg::game::engine::demo;
    use bornhack_aegg::game::engine::needs::{MAX, P};
    use bornhack_aegg::game::engine::thresholds::MAX_SLEEP_TICKS;
    use bornhack_aegg::game::engine::{GameState, PetKind, Phase, TICKS_PER_MINUTE};

    fn pct(p: i32) -> u16 {
        (p * P) as u16
    }

    fn obs(s: &GameState) -> String {
        format!("{:?}", s.wake_view_for_test())
    }

    #[test]
    fn demo_wake_is_exact_on_random_states() {
        const STATES: usize = 2000;
        const LOOKAHEAD_MIN: u32 = 120;
        let mut x = 3u64;
        let mut next = |bound: u64| {
            x = x
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (x >> 33) % (bound + 1)
        };
        assert_eq!(MAX_SLEEP_TICKS(), LOOKAHEAD_MIN * TICKS_PER_MINUTE);
        let (mut pending, mut timed, mut flipped) = (0, 0, 0);
        for i in 0..STATES {
            let mut s = GameState::new_egg(7, PetKind(0));
            s.phase = Phase::Active;
            s.money_enabled = false;
            s.tick_rem = 0;
            let mut level = |n: &mut dyn FnMut(u64) -> u64| pct(n(100) as i32);
            s.fullness = level(&mut next);
            s.hydration = level(&mut next);
            s.energy = level(&mut next);
            s.fun = level(&mut next);
            s.dirt = level(&mut next);
            s.booze = level(&mut next);
            s.fitness = level(&mut next);
            s.weight = level(&mut next);
            s.health = pct(30 + next(70) as i32);
            s.mood = MAX as u16;
            s.is_sleeping = next(1) == 1;
            s.diabetic = next(4) == 0;
            s.dependent = next(4) == 0;
            s.insulin_min = next(200) as u16;
            s.injured_min = if next(2) == 0 { next(100) as u16 } else { 0 };
            s.cooldown_bornjeweled = if next(2) == 0 { next(900) as u16 } else { 0 };
            s.cooldown_lightsout = if next(3) == 0 { next(900) as u16 } else { 0 };
            s.demo.on = true;
            s.demo.animated = i % 2 == 0;
            s.demo.lazy = next(15) as u8;
            s.demo.lazy_min = next(300) as u16;
            s.demo.whim_min = next(300) as u16;
            s.demo.pause_min = if next(1) == 1 { 0 } else { next(90) as u8 };
            s.demo.soft_armed = next(2) == 0;
            s.demo.next_min = if s.demo.soft_armed { next(40) as u8 } else { 0 };
            s.demo.clock_armed = next(2) == 0;
            s.demo.clock_min = next(200) as u16;
            s.set_demo_rng(1 + next(1_000_000) as u32);
            let clock = if next(1) == 1 {
                Some(next(1439) as u16)
            } else {
                None
            };
            s.sync_clock(s.last_update_tick, clock);

            let t0 = s.last_update_tick;
            let mut c = s.clone();
            demo::settle(&mut c);
            let expect = if demo::decide(&c).is_some() {
                pending += 1;
                t0 + 1
            } else {
                let o = obs(&c);
                // The pet acts at the wake: a minute where it did something
                // counts as a change, whether or not the screen differs.
                let first = (1..=LOOKAHEAD_MIN).find(|m| {
                    c.update(t0 + m * TICKS_PER_MINUTE);
                    obs(&c) != o || c.note().is_some()
                });
                if first.is_some() {
                    timed += 1;
                    if c.note().is_some() {
                        flipped += 1;
                    }
                }
                first.map_or(t0 + MAX_SLEEP_TICKS(), |m| t0 + m * TICKS_PER_MINUTE)
            };
            assert_eq!(s.next_wake_tick(), expect, "random state {i}");
        }
        assert!(pending > 100, "{pending} states had a choice waiting");
        assert!(timed > 100, "{timed} states changed within the look-ahead");
        assert!(flipped > 20, "{flipped} states woke for a new choice");
    }

    #[test]
    fn a_pending_choice_wakes_the_badge_at_once() {
        let mut s = GameState::new_egg(7, PetKind(0));
        s.phase = Phase::Active;
        s.money_enabled = false;
        s.fullness = pct(5);
        s.set_demo(true).unwrap();
        assert!(demo::decide(&s).is_some());
        assert_eq!(s.next_wake_tick(), s.last_update_tick + 1);
        // The same pet with the demo mode off waits for its own changes.
        s.set_demo(false).unwrap();
        assert!(s.next_wake_tick() > s.last_update_tick + 1);
    }
}

mod acts {
    use super::policy::{at, lcg, pct, quiet, random_demo};
    use bornhack_aegg::game::engine::causes::Cause;
    use bornhack_aegg::game::engine::demo::{self, Choice, Note};
    use bornhack_aegg::game::engine::sim;
    use bornhack_aegg::game::engine::thresholds as t;
    use bornhack_aegg::game::engine::{DrinkKind, FoodKind, GameState, TICKS_PER_MINUTE};

    const HOUR: u16 = 60;
    const SIX_HOURS: u32 = 6 * 60;

    fn minutes(s: &mut GameState, n: u32) {
        s.update(s.last_update_tick + n * TICKS_PER_MINUTE);
    }

    /// Run the player's action out (its ticks age nothing).
    fn finish(s: &mut GameState) {
        s.update(s.last_update_tick + s.action_ticks_remaining as u32);
    }

    #[test]
    fn the_pet_eats_when_hungry() {
        let mut s = quiet();
        s.money_enabled = true;
        s.money = 100;
        at(&mut s, 12 * HOUR);
        s.fullness = pct(19);
        let (money, age) = (s.money, s.age_ticks);
        minutes(&mut s, 1);
        assert!(s.fullness > pct(19), "fullness {}", s.fullness);
        assert_eq!(s.money, money - 10, "the pet pays for its pizza");
        assert_eq!(s.active_action, None);
        assert!(
            matches!(s.note(), Some((Note::Ate(FoodKind::Pizza), _))),
            "{:?}",
            s.note()
        );
        assert_eq!(s.age_ticks - age, TICKS_PER_MINUTE);
    }

    #[test]
    fn the_pet_uses_the_real_doses() {
        for food in FoodKind::ALL {
            let mut a = quiet();
            a.money_enabled = true;
            a.money = 100;
            a.fullness = pct(30);
            let mut b = a.clone();
            demo::act(&mut a, Choice::Eat(food));
            let price = bornhack_aegg::game::engine::doses::price(
                bornhack_aegg::game::engine::Action::Feed,
                Some(food),
                None,
                a.hard_mode,
            )
            .unwrap();
            assert_eq!(a.money, 100 - price, "the pet pays its own care: {food:?}");
            assert!(b.feed(food));
            finish(&mut b);
            let needs = |s: &GameState| {
                (
                    s.fullness,
                    s.hydration,
                    s.energy,
                    s.fun,
                    s.dirt,
                    s.booze,
                    s.weight,
                    s.health,
                    s.fitness,
                    s.mood,
                )
            };
            assert_eq!(needs(&a), needs(&b), "{food:?}");
            assert_eq!(a.active_action, None);
        }
        let mut a = quiet();
        let mut b = a.clone();
        demo::act(&mut a, Choice::Drink(DrinkKind::Coffee));
        assert!(b.drink(DrinkKind::Coffee));
        finish(&mut b);
        assert_eq!(
            (a.hydration, a.energy, a.mood),
            (b.hydration, b.energy, b.mood)
        );
    }

    #[test]
    fn the_player_pauses_the_pet() {
        let mut s = quiet();
        at(&mut s, 12 * HOUR);
        s.hydration = pct(15);
        s.fullness = pct(10);
        assert!(s.drink(DrinkKind::Water));
        assert_eq!(s.demo.pause_min, t::DEMO_PAUSE_MIN());
        finish(&mut s);
        minutes(&mut s, t::DEMO_PAUSE_MIN() as u32 - 1);
        assert_eq!(s.note(), None, "the pet acted during the pause");
        assert_eq!(s.demo.pause_min, 1);
        minutes(&mut s, 2);
        assert!(
            matches!(s.note(), Some((Note::Ate(_) | Note::Drank(_), _))),
            "{:?}",
            s.note()
        );
    }

    #[test]
    fn a_player_action_clears_the_note() {
        let mut s = quiet();
        s.dirt = pct(80);
        demo::act(&mut s, Choice::Wash);
        assert!(matches!(s.note(), Some((Note::Washing, _))));
        assert!(s.play());
        assert_eq!(s.note(), None);
        let mut s = quiet();
        s.dirt = pct(80);
        demo::act(&mut s, Choice::Wash);
        assert_eq!(s.demo.pause_min, 0, "the pet's own act does not pause it");
    }

    #[test]
    fn helping_gets_a_thanks() {
        let mut s = quiet();
        s.fullness = pct(10);
        assert!(s.feed(FoodKind::Apple));
        finish(&mut s);
        assert!(s.take_causes().iter().any(|&c| c == Cause::Thanks));
    }

    #[test]
    fn no_thanks_for_a_bad_dose() {
        let mut s = quiet();
        s.fullness = pct(88);
        assert!(s.feed(FoodKind::Pizza));
        finish(&mut s);
        let got: Vec<Cause> = s.take_causes().iter().copied().collect();
        assert!(got.contains(&Cause::FoodComa), "{got:?}");
        assert!(!got.contains(&Cause::Thanks), "{got:?}");
    }

    #[test]
    fn no_thanks_for_a_grumpy_wake() {
        let mut s = quiet();
        s.is_sleeping = true;
        s.energy = pct(5);
        s.fullness = pct(10);
        assert!(s.feed(FoodKind::Apple));
        finish(&mut s);
        let got: Vec<Cause> = s.take_causes().iter().copied().collect();
        assert!(got.contains(&Cause::Grumpy), "{got:?}");
        assert!(!got.contains(&Cause::Thanks), "{got:?}");
    }

    #[test]
    fn the_sleep_forecast_leaves_the_policy_out() {
        let mut s = quiet();
        s.energy = pct(30);
        s.fullness = pct(25);
        let mut off = s.clone();
        off.demo.on = false;
        assert_eq!(s.preview_sleep(), off.preview_sleep());
        assert!(s.demo.on);
    }

    #[test]
    fn the_pets_own_acts_get_no_thanks() {
        let mut s = quiet();
        s.fullness = pct(10);
        demo::act(&mut s, Choice::Eat(FoodKind::Apple));
        assert!(!s.take_causes().iter().any(|&c| c == Cause::Thanks));
    }

    #[test]
    fn thanks_is_a_cause_but_not_a_warning() {
        assert!(!Cause::Thanks.is_warning());
        assert_eq!(
            (Cause::Thanks.what(), Cause::Thanks.why()),
            ("thanks!", "that helped")
        );
        assert_eq!(Cause::unpack(Cause::Thanks.pack()), Some(Cause::Thanks));
        assert!(Cause::all().any(|c| c == Cause::Thanks));
    }

    #[test]
    fn a_self_wake_is_not_grumpy() {
        let mut s = quiet();
        s.is_sleeping = true;
        s.energy = pct(5);
        let mood = s.mood;
        demo::act(&mut s, Choice::Wake);
        assert!(!s.is_sleeping);
        assert_eq!(s.mood, mood);
        assert!(!s.take_causes().iter().any(|&c| c == Cause::Grumpy));
        assert!(matches!(s.note(), Some((Note::UpAndAbout, _))));
    }

    #[test]
    fn run_settles_to_nothing_to_do() {
        let mut next = lcg(5);
        let mut acted = 0;
        for i in 0..2000 {
            let mut s = random_demo(&mut next);
            let n = demo::run(&mut s);
            assert!(n <= demo::MAX_ACTS_PER_BOUNDARY, "state {i}: {n} acts");
            assert!(
                demo::decide(&s).is_none(),
                "state {i}: {:?} still pending after {n} acts",
                demo::decide(&s)
            );
            assert!(n < demo::MAX_ACTS_PER_BOUNDARY, "state {i} hit the cap");
            acted += (n > 0) as u32;
        }
        assert!(acted > 1000, "only {acted} of 2000 states acted");
    }

    #[test]
    fn a_whim_nap_ends_on_its_own() {
        let mut s = quiet();
        s.demo.whim_min = 0;
        s.energy = pct(50);
        s.fun = pct(40);
        s.booze = pct(50);
        assert_eq!(demo::decide(&s), Some(Choice::Nap));
        assert_eq!(demo::run(&mut s), 1);
        assert!(s.is_sleeping);
        assert!(matches!(s.note(), Some((Note::Napping, _))));
        assert!(s.demo.clock_armed);
        let base = t::DEMO_NAP_BASE_MIN();
        assert!(
            (base..=base + t::DEMO_NAP_SPREAD_MIN()).contains(&s.demo.clock_min),
            "{}",
            s.demo.clock_min
        );
        assert!(s.demo.whim_min > 0, "the whim is re-armed");
        let nap = s.demo.clock_min as u32;
        minutes(&mut s, nap - 1);
        assert!(s.is_sleeping);
        minutes(&mut s, 1);
        assert!(!s.is_sleeping);
        assert!(matches!(s.note(), Some((Note::UpAndAbout, _))));
    }

    #[test]
    fn a_soft_act_clears_the_soft_delay() {
        let mut s = quiet();
        s.hydration = pct(t::DEMO_SOFT_WATER_PCT() as i32 - 8);
        s.demo.soft_armed = true;
        s.demo.next_min = 0;
        demo::run(&mut s);
        assert!(
            matches!(s.note(), Some((Note::Drank(_), _))),
            "{:?}",
            s.note()
        );
        // Drunk, the pet is no longer thirsty: the delay is not running.
        assert!(!s.demo.soft_armed);
    }

    #[test]
    fn the_badge_wakes_when_the_pet_acts() {
        let mut s = quiet();
        at(&mut s, 12 * HOUR);
        s.hydration = pct(t::DEMO_SOFT_WATER_PCT() as i32 - 2);
        s.demo.soft_armed = true;
        s.demo.next_min = 10;
        let t0 = s.last_update_tick;
        let wake = s.next_wake_tick();
        assert_eq!(wake, t0 + 10 * TICKS_PER_MINUTE);
        s.update(wake);
        assert!(
            matches!(s.note(), Some((Note::Drank(_), _))),
            "{:?}",
            s.note()
        );
        assert!(s.next_wake_tick() > wake + 1);
    }

    /// A pending choice is acted on at the first update after the wake: the
    /// badge never steps tick by tick.
    #[test]
    fn a_demo_pet_never_wakes_tick_by_tick() {
        let mut s = quiet();
        at(&mut s, 8 * HOUR);
        s.fullness = pct(12);
        s.hydration = pct(18);
        s.demo.whim_min = 0;
        let (mut wakes, mut ones) = (0u32, 0u32);
        let acts0 = sim::self_care_acts();
        let end = s.last_update_tick + SIX_HOURS * TICKS_PER_MINUTE;
        while s.last_update_tick < end {
            let before = s.last_update_tick;
            let wake = s.next_wake_tick();
            assert!(wake > before);
            s.update(wake);
            wakes += 1;
            if wake - before == 1 {
                // Only the first wake, for the choice the setup left pending.
                ones += 1;
            } else {
                assert_eq!(s.tick_rem, 0, "wake {wakes} fell off a whole minute");
            }
        }
        assert_eq!(ones, 1, "single-tick wakes");
        // The shortest delay between the pet's own soft acts is the soft
        // delay (the urgent rows are state edges, a few an hour at most), so
        // that many wakes is the bound.
        let shortest = t::DEMO_REACT_BASE_MIN().min(t::DEMO_DRY_BASE_MIN()) as u32;
        let acts = sim::self_care_acts() - acts0;
        assert!(acts > 0);
        assert!(
            wakes <= SIX_HOURS / shortest,
            "{wakes} wakes in 6 hours, {acts} acts"
        );
    }

    /// A player's action can leave the clock part-way through a minute
    /// (`tick_rem`). The first wake after it may land off a whole minute: the
    /// pause it starts ends there. Every wake after that is on a whole
    /// minute again.
    #[test]
    fn a_player_action_is_the_one_wake_off_a_whole_minute() {
        let mut s = quiet();
        at(&mut s, 8 * HOUR);
        s.demo.whim_min = 0;
        // Part-way through a minute, then the player feeds the pet.
        s.update(s.last_update_tick + TICKS_PER_MINUTE + 1);
        assert_ne!(s.tick_rem, 0, "the update left part of a minute");
        assert!(s.feed(FoodKind::Apple));
        finish(&mut s);
        assert_ne!(s.tick_rem, 0, "the action left part of a minute");
        assert!(s.demo.pause_min > 0, "the player's action paused the pet");
        let (mut off, mut wakes) = (0u32, 0u32);
        let end = s.last_update_tick + SIX_HOURS * TICKS_PER_MINUTE;
        while s.last_update_tick < end {
            let before = s.last_update_tick;
            let wake = s.next_wake_tick();
            assert!(wake > before);
            s.update(wake);
            wakes += 1;
            if s.tick_rem != 0 {
                off += 1;
                assert_eq!(wakes, 1, "wake {wakes} fell off a whole minute");
            }
        }
        assert!(off <= 1, "{off} wakes off a whole minute");
    }

    #[test]
    fn note_words_fit_the_line() {
        let mut notes = vec![
            Note::OneTooMany,
            Note::Napping,
            Note::OffToBed,
            Note::UpAndAbout,
            Note::Washing,
            Note::Played,
            Note::Rested,
            Note::Medicine,
            Note::Insulin,
            Note::Rehab,
            Note::Worked,
        ];
        notes.extend(FoodKind::ALL.map(Note::Ate));
        notes.extend(DrinkKind::ALL.map(Note::Drank));
        for n in notes {
            assert!(!n.text().is_empty() && n.text().len() <= 21, "{n:?}");
        }
        assert_eq!(Note::Ate(FoodKind::Apple).text(), "had an apple");
        assert_eq!(Note::Drank(DrinkKind::Beer).text(), "fancied a beer");
        assert_eq!(Note::OneTooMany.text(), "one too many...");
    }
}

mod split {
    use super::policy::{lcg, random_coffee_demo, random_demo};
    use bornhack_aegg::game::engine::sim;
    use bornhack_aegg::game::engine::thresholds as t;
    use bornhack_aegg::game::engine::{DrinkKind, GameState, TICKS_PER_MINUTE};

    const DAY_TICKS: u32 = 1440 * TICKS_PER_MINUTE;

    fn snapshot(s: &GameState) -> (Vec<u8>, u32, u8, u32, String) {
        (
            s.to_bytes().to_vec(),
            s.age_ticks,
            s.tick_rem,
            s.demo_rng(),
            format!("{:?}", s.note()),
        )
    }

    /// Run `start` to `t0 + span` with updates cut at `cuts` (sorted). A
    /// player drink lands at `player` (a tick offset, also a cut).
    pub(super) fn drive(
        start: &GameState,
        span: u32,
        cuts: &[u32],
        player: Option<u32>,
    ) -> GameState {
        let t0 = start.last_update_tick;
        let mut s = start.clone();
        let mut did = false;
        for &c in cuts.iter().chain(&[span]) {
            if let Some(p) = player
                && !did
                && c >= p
            {
                s.update(t0 + p);
                s.drink(DrinkKind::Water);
                did = true;
            }
            s.update(t0 + c);
        }
        s
    }

    /// The ticks where the pet acts, found by stepping a copy a minute at a
    /// time (a test oracle: the engine itself never steps).
    fn act_ticks(start: &GameState, span: u32, player: Option<u32>) -> Vec<u32> {
        let mut s = start.clone();
        let t0 = s.last_update_tick;
        let mut found = Vec::new();
        let mut at = 0;
        while at < span {
            let next = (at + TICKS_PER_MINUTE).min(span);
            if player.is_some_and(|p| (at..next).contains(&p)) {
                s.update(t0 + player.unwrap());
                s.drink(DrinkKind::Water);
            }
            let before = sim::self_care_acts();
            s.update(t0 + next);
            if sim::self_care_acts() != before {
                found.push(next);
            }
            at = next;
        }
        found
    }

    /// The pet's own acts are events of the engine: one long update and the
    /// same span cut anywhere (at random, and at, just before and just after
    /// each act and each pause expiry) leave the same pet, the same dice and
    /// the same note.
    #[test]
    fn demo_update_is_split_invariant() {
        let (mut acted, mut cut_at_acts) = (0, 0);
        for seed in 0..200u64 {
            let mut next = lcg(1000 + seed);
            let start = random_demo(&mut next);
            let span = 3 * DAY_TICKS;
            let mut cuts: Vec<u32> = (0..50).map(|_| next(span as u64 - 1) as u32 + 1).collect();
            // Every fourth pet is helped by the player somewhere in the span.
            let player = (seed % 4 == 0).then(|| next(span as u64 - 1) as u32 + 1);
            if let Some(p) = player {
                let pause = t::DEMO_PAUSE_MIN() as u32 * TICKS_PER_MINUTE;
                cuts.extend([p, p + pause - 1, p + pause, p + pause + 1]);
            }
            for tick in act_ticks(&start, span, player) {
                cuts.extend([tick.saturating_sub(1), tick, tick + 1]);
                cut_at_acts += 1;
            }
            cuts.retain(|&c| c > 0 && c < span);
            cuts.sort_unstable();
            cuts.dedup();
            let whole = drive(&start, span, &[], player);
            let split = drive(&start, span, &cuts, player);
            assert_eq!(snapshot(&whole), snapshot(&split), "seed {seed}");
            acted += whole.note().is_some() as u32;
        }
        assert!(acted > 100, "only {acted} of 200 pets acted");
        assert!(cut_at_acts > 500, "only {cut_at_acts} act points were cut");
    }

    /// The same with groggy and caffeinated pets: the coffee row and the
    /// caffeine line are events of the engine too.
    #[test]
    fn demo_update_with_coffee_is_split_invariant() {
        let mut coffee = 0;
        for seed in 0..120u64 {
            let mut next = lcg(5000 + seed);
            let start = random_coffee_demo(&mut next);
            let span = 2 * DAY_TICKS;
            let mut cuts: Vec<u32> = (0..40).map(|_| next(span as u64 - 1) as u32 + 1).collect();
            for tick in act_ticks(&start, span, None) {
                cuts.extend([tick.saturating_sub(1), tick, tick + 1]);
            }
            cuts.retain(|&c| c > 0 && c < span);
            cuts.sort_unstable();
            cuts.dedup();
            let whole = drive(&start, span, &[], None);
            let split = drive(&start, span, &cuts, None);
            assert_eq!(snapshot(&whole), snapshot(&split), "seed {seed}");
            coffee += (start.caffeine_min > 0 || start.groggy_min > 0) as u32;
        }
        assert!(coffee > 40, "only {coffee} pets started groggy or caffeinated");
    }
}

mod animated {
    use super::policy::{at, lcg, pct, quiet, random_demo};
    use super::split::drive;
    use bornhack_aegg::game::engine::causes::Cause;
    use bornhack_aegg::game::engine::demo::{self, Choice, Note};
    use bornhack_aegg::game::engine::doses::Reason;
    use bornhack_aegg::game::engine::thresholds as t;
    use bornhack_aegg::game::engine::{
        Action, DisplayAnim, DrinkKind, FoodKind, GameState, Phase, TICKS_PER_MINUTE,
    };

    const HOUR: u16 = 60;
    const DAY_TICKS: u32 = 1440 * TICKS_PER_MINUTE;

    /// A hungry animated pet at noon.
    fn hungry() -> GameState {
        let mut s = quiet();
        s.money_enabled = true;
        s.money = 100;
        s.set_demo_care(true);
        at(&mut s, 12 * HOUR);
        s.fullness = pct(19);
        s
    }

    /// A hungry animated pet with its meal under way (one tick in).
    fn eating() -> GameState {
        let mut s = hungry();
        s.update(s.last_update_tick + 1);
        assert!(s.demo.pet_action);
        s
    }

    #[test]
    fn set_demo_care_sets_the_mode_and_saves() {
        let mut s = quiet();
        s.mark_saved();
        s.set_demo_care(true);
        assert!(s.demo.animated && s.needs_save());
        s.mark_saved();
        s.set_demo_care(false);
        assert!(!s.demo.animated && s.needs_save());
    }

    #[test]
    fn animated_care_runs_the_real_action() {
        let mut s = hungry();
        let (money, age) = (s.money, s.age_ticks);
        // The wake after a pending choice is one tick: the meal starts.
        s.update(s.last_update_tick + 1);
        assert_eq!(s.active_action, Some(Action::Feed));
        assert!(matches!(s.display_anim(), DisplayAnim::Feeding));
        assert!(s.demo.pet_action);
        assert_eq!(s.fullness, pct(19), "the dose lands at the end");
        assert!(s.note().is_none(), "the note comes with the end");
        assert_eq!(s.age_ticks - age, 1);
        s.update(s.last_update_tick + t::FEED_DURATION() as u32);
        assert!(s.fullness > pct(19), "fullness {}", s.fullness);
        assert_eq!(s.active_action, None);
        assert!(!s.demo.pet_action);
        assert!(
            matches!(s.note(), Some((Note::Ate(FoodKind::Pizza), _))),
            "{:?}",
            s.note()
        );
        assert_eq!(s.demo.pause_min, 0);
        assert!(!s.take_causes().iter().any(|&c| c == Cause::Thanks));
        assert_eq!(s.money, money - 10, "the pet paid for its pizza");
    }

    #[test]
    fn a_pet_action_spends_the_remainder() {
        let mut s = hungry();
        s.tick_rem = 4;
        assert!(matches!(demo::decide(&s), Some(Choice::Eat(_))));
        let needs = (s.hydration, s.energy, s.fun);
        s.update(s.last_update_tick + 1);
        // 4 + 1 = 5 ticks of stream: 2 for the meal, 3 left over.
        assert_eq!(s.active_action, None);
        assert_eq!(s.tick_rem, 3);
        assert!(s.fullness > pct(19));
        assert_eq!((s.hydration, s.energy, s.fun), needs, "needs did not age");
    }

    /// As `split::demo_update_is_split_invariant`, with the pet's care
    /// animated: a pet's action eats the engine-time stream, a player's
    /// action (every fourth pet is helped) eats fresh ticks, and the cuts
    /// fall around both and around the pause that follows the player's.
    #[test]
    fn animated_update_is_split_invariant() {
        let snapshot = |s: &GameState| {
            (
                s.to_bytes().to_vec(),
                s.age_ticks,
                s.tick_rem,
                s.demo_rng(),
                format!("{:?}", s.note()),
                s.active_action,
                s.action_ticks_remaining,
            )
        };
        let (mut acted, mut mid_action, mut helped) = (0, 0, 0);
        for seed in 0..200u64 {
            let mut next = lcg(5000 + seed);
            let mut start = random_demo(&mut next);
            start.set_demo_care(true);
            let span = 3 * DAY_TICKS;
            let mut cuts: Vec<u32> = (0..50).map(|_| next(span as u64 - 1) as u32 + 1).collect();
            // Every fourth pet is helped by the player somewhere in the span.
            let player = (seed % 4 == 0).then(|| next(span as u64 - 1) as u32 + 1);
            if let Some(p) = player {
                let pause = t::DEMO_PAUSE_MIN() as u32 * TICKS_PER_MINUTE;
                cuts.extend([p, p + pause - 1, p + pause, p + pause + 1]);
                helped += 1;
            }
            // Cut at, before and after every tick where a pet action runs.
            let mut probe = start.clone();
            let t0 = probe.last_update_tick;
            let mut at_tick = 0;
            while at_tick < span {
                let step = (at_tick + TICKS_PER_MINUTE).min(span);
                if let Some(p) = player.filter(|p| (at_tick..step).contains(p)) {
                    probe.update(t0 + p);
                    probe.drink(DrinkKind::Water);
                }
                probe.update(t0 + step);
                if probe.demo.pet_action {
                    cuts.extend([step.saturating_sub(1), step, step + 1]);
                    mid_action += 1;
                }
                at_tick = step;
            }
            cuts.retain(|&c| c > 0 && c < span);
            cuts.sort_unstable();
            cuts.dedup();
            let whole = drive(&start, span, &[], player);
            let split = drive(&start, span, &cuts, player);
            assert_eq!(snapshot(&whole), snapshot(&split), "seed {seed}");
            acted += whole.note().is_some() as u32;
        }
        assert!(helped > 40, "only {helped} pets were helped");
        assert!(acted > 100, "only {acted} of 200 pets acted");
        assert!(
            mid_action > 50,
            "only {mid_action} minutes ended inside an action"
        );
    }

    #[test]
    fn pet_action_wake_lands_on_its_end() {
        let mut s = eating();
        s.tick_rem = 3;
        s.action_ticks_remaining = 9;
        let end = s.last_update_tick + 9 - 3;
        assert_eq!(s.action_end_tick(), end);
        assert_eq!(s.next_wake_tick(), end);
        // A player's action does not spend the remainder.
        let mut p = quiet();
        p.demo.on = false;
        p.tick_rem = 3;
        assert!(p.feed(FoodKind::Apple));
        assert_eq!(
            p.action_end_tick(),
            p.last_update_tick + t::FEED_DURATION() as u32
        );
        // The end is at least one tick away.
        let mut s = eating();
        s.tick_rem = 5;
        s.action_ticks_remaining = 2;
        assert_eq!(s.action_end_tick(), s.last_update_tick + 1);
    }

    #[test]
    fn the_player_waits_for_the_pet() {
        let s = eating();
        assert_eq!(s.refusal_for(Action::Feed, None), Some(Reason::Busy));
    }

    #[test]
    fn a_pet_action_survives_a_save() {
        let s = eating();
        let b = GameState::from_bytes(&s.to_bytes()).expect("loads");
        assert!(b.demo.pet_action);
        assert_eq!(b.active_action, s.active_action);
        assert_eq!(b.action_ticks_remaining, s.action_ticks_remaining);
    }

    #[test]
    fn a_pet_action_finishes_after_demo_goes_off() {
        let mut s = eating();
        s.set_demo(false).unwrap();
        assert!(s.demo.pet_action);
        s.update(s.last_update_tick + t::FEED_DURATION() as u32);
        assert!(!s.demo.pet_action && s.active_action.is_none());
        assert!(s.fullness > pct(19));
        assert!(!s.take_causes().iter().any(|&c| c == Cause::Thanks));
    }

    /// C11: a pet's own wake costs nothing, through the whole animated act.
    #[test]
    fn an_animated_self_wake_is_not_grumpy() {
        let mut s = quiet();
        s.set_demo_care(true);
        s.is_sleeping = true;
        s.energy = pct(40);
        s.fullness = pct(5);
        let mood = s.mood;
        // The pet wakes, then eats, all on its own.
        s.update(s.last_update_tick + 1);
        s.update(s.last_update_tick + t::FEED_DURATION() as u32 + 1);
        assert!(!s.is_sleeping);
        assert!(s.fullness > pct(5));
        assert_eq!(s.mood, mood);
        let got: Vec<_> = s.take_causes().iter().copied().collect();
        assert!(!got.contains(&Cause::Grumpy), "{got:?}");
        assert!(!got.contains(&Cause::Thanks), "{got:?}");
        // The act itself, started on a sleeper, wakes it without the dent.
        let mut s = quiet();
        s.set_demo_care(true);
        s.is_sleeping = true;
        s.energy = pct(40);
        demo::act(&mut s, Choice::Drink(DrinkKind::Water));
        assert!(!s.is_sleeping && s.demo.pet_action);
        s.update(s.last_update_tick + t::DRINK_DURATION() as u32);
        assert_eq!(s.mood, mood);
        assert!(!s.take_causes().iter().any(|&c| c == Cause::Grumpy));
    }

    #[test]
    fn animated_pet_alone_for_three_days() {
        let mut s = quiet();
        s.set_demo_care(true);
        s.set_demo(true).unwrap();
        at(&mut s, 8 * HOUR);
        s.fullness = pct(40);
        let t0 = s.last_update_tick;
        let (mut feeding, mut wakes) = (0, 0u32);
        while s.last_update_tick < t0 + 3 * DAY_TICKS {
            let now = s.next_wake_tick().max(s.last_update_tick + 1);
            s.update(now);
            wakes += 1;
            assert_ne!(s.phase, Phase::Leaving);
            assert!(s.health >= pct(t::DEMO_HEALTH_FLOOR_PCT() as i32));
            feeding += matches!(s.display_anim(), DisplayAnim::Feeding) as u32;
        }
        assert!(feeding > 0, "never saw it feeding in {wakes} wakes");
    }
}

mod calm {
    use super::policy::{at, pct, quiet};
    use bornhack_aegg::game::engine::demo::{self, Choice, Note};
    use bornhack_aegg::game::engine::needs::P;
    use bornhack_aegg::game::engine::status::status_line;
    use bornhack_aegg::game::engine::thresholds as t;
    use bornhack_aegg::game::engine::{
        DrinkKind, FoodKind, GameState, PetLine, TICKS_PER_MINUTE,
    };

    #[test]
    fn the_demo_line_is_never_red() {
        let mut s = quiet();
        s.fullness = pct(2);
        assert!(!status_line(&s).danger, "demo");
        assert!(!s.pet_line().red());
        s.demo.on = false;
        assert!(status_line(&s).danger, "demo off");
        assert!(s.pet_line().red());
    }

    #[test]
    fn help_line_below_35() {
        let mut s = quiet();
        s.health = pct(34);
        assert_eq!(s.pet_line(), PetLine::Help);
        assert!(s.pet_line().red());
        s.health = pct(36);
        assert!(!matches!(s.pet_line(), PetLine::Help));
        assert!(!s.pet_line().red());
        // "Under": on the line is not under it, one level below is.
        s.health = pct(t::DEMO_HELP_HEALTH_PCT() as i32);
        assert!(!matches!(s.pet_line(), PetLine::Help), "on the line");
        assert_eq!(demo::wake_view(&s) & 2, 0);
        s.health -= 1;
        assert_eq!(s.pet_line(), PetLine::Help, "one level under");
        assert_eq!(demo::wake_view(&s) & 2, 2);
        // Demo off: never a help line, whatever the health.
        s.health = pct(34);
        s.demo.on = false;
        assert!(matches!(s.pet_line(), PetLine::Status(_)));
    }

    #[test]
    fn a_note_ends_when_the_line_changes() {
        let mut s = quiet();
        at(&mut s, 12 * 60);
        demo::act(&mut s, Choice::Eat(FoodKind::Apple));
        while s.active_action.is_some() {
            s.update(s.last_update_tick + s.action_ticks_remaining as u32);
        }
        assert_eq!(s.pet_line(), PetLine::Note(Note::Ate(FoodKind::Apple)));
        let before = status_line(&s);
        let mut m = 0;
        while status_line(&s) == before {
            m += 1;
            assert!(m < 1500, "the line never changed");
            s.update(s.last_update_tick + TICKS_PER_MINUTE);
        }
        assert!(matches!(s.pet_line(), PetLine::Status(_)), "{:?}", s.pet_line());
    }

    /// An old note does not come back when demo is switched on again.
    #[test]
    fn switching_demo_off_and_on_forgets_the_note() {
        let mut s = quiet();
        s.fullness = pct(19);
        s.update(s.last_update_tick + 1);
        assert!(s.note().is_some(), "the pet ate and left a note");
        s.set_demo(false).unwrap();
        s.set_demo(true).unwrap();
        assert!(s.note().is_none());
        assert!(matches!(s.pet_line(), PetLine::Status(_)));
    }

    #[test]
    fn a_note_never_shows_with_demo_off() {
        let mut s = quiet();
        at(&mut s, 12 * 60);
        demo::act(&mut s, Choice::Eat(FoodKind::Apple));
        s.update(s.last_update_tick + s.action_ticks_remaining as u32);
        assert!(matches!(s.pet_line(), PetLine::Note(_)), "{:?}", s.pet_line());
        s.demo.on = false;
        assert!(matches!(s.pet_line(), PetLine::Status(_)), "{:?}", s.pet_line());
        assert!(s.note().is_some(), "the stale note is still held");
    }

    #[test]
    fn help_flip_is_a_wake() {
        let mut s = quiet();
        s.health = (t::DEMO_HELP_HEALTH_PCT() as i32 * P + P / 2) as u16;
        s.demo.pause_min = 90;
        s.fullness = 0;
        s.hydration = 0;
        s.dirt = pct(99);
        let t0 = s.last_update_tick;
        assert_ne!(s.pet_line(), PetLine::Help);
        let mut c = s.clone();
        let first = (1..=120u32)
            .find(|&m| {
                c.update(t0 + m * TICKS_PER_MINUTE);
                c.pet_line() == PetLine::Help
            })
            .expect("health falls below the help line");
        let wake = s.next_wake_tick();
        assert_eq!(wake, t0 + first * TICKS_PER_MINUTE, "first {first}");
        // The help bit alone: with the other lines unchanged the wake is exact.
        let bit = |g: &GameState| demo::wake_view(g) & 2;
        let mut d = s.clone();
        d.update(t0 + (first - 1) * TICKS_PER_MINUTE);
        assert_eq!(bit(&d), 0);
        d.update(t0 + first * TICKS_PER_MINUTE);
        assert_eq!(bit(&d), 2);
    }

    #[test]
    fn notes_fit() {
        let mut all = vec![
            Note::OneTooMany,
            Note::Napping,
            Note::OffToBed,
            Note::UpAndAbout,
            Note::Washing,
            Note::Played,
            Note::Rested,
            Note::Medicine,
            Note::Insulin,
            Note::Rehab,
            Note::Worked,
        ];
        all.extend(FoodKind::ALL.map(Note::Ate));
        all.extend(DrinkKind::ALL.map(Note::Drank));
        for n in all {
            assert!(n.text().len() <= 21, "{:?}: {}", n, n.text());
        }
        assert!(Note::OneTooMany.text().ends_with("..."));
    }
}

/// HAX in demo mode: the pet pays, earns, and the balance is held near
/// `DEMO_HAX_TARGET`.
mod hax {
    use super::policy::{pct, quiet, soft};
    use bornhack_aegg::game::engine::advance::advance;
    use bornhack_aegg::game::engine::demo::{self, Choice, Note};
    use bornhack_aegg::game::engine::thresholds as t;
    use bornhack_aegg::game::engine::{GameState, MINIGAME_HAX_REWARD, MiniGame, ONLYPETS_HOBBY_REWARD};

    fn rich(money: u32) -> GameState {
        let mut s = quiet();
        s.money_enabled = true;
        s.money = money;
        s
    }

    fn target() -> u32 {
        t::DEMO_HAX_TARGET() as u32
    }

    #[test]
    fn the_pet_works_below_the_target_and_not_at_it() {
        let mut s = rich(target() - 1);
        soft(&mut s);
        assert_eq!(demo::decide(&s), Some(Choice::Work));
        s.money = target();
        assert_eq!(demo::decide(&s), None);
        s.money = target() + 40;
        assert_eq!(demo::decide(&s), None);
    }

    #[test]
    fn a_pet_without_the_money_layer_never_works() {
        let mut s = quiet();
        s.money = 0;
        soft(&mut s);
        assert_eq!(demo::decide(&s), None);
    }

    #[test]
    fn working_waits_for_the_soft_delay_and_for_urgent_care() {
        let mut s = rich(60);
        assert_eq!(demo::decide(&s), None, "no soft delay running");
        soft(&mut s);
        s.fullness = pct(5);
        assert!(matches!(demo::decide(&s), Some(Choice::Eat(_))), "care first");
    }

    #[test]
    fn work_respects_the_starters_gate() {
        let mut s = rich(60);
        soft(&mut s);
        s.energy = pct(t::DEMO_PLAY_ENERGY_PCT() as i32 - 1);
        assert_ne!(demo::decide(&s), Some(Choice::Work), "too tired");
        let mut hurt = rich(60);
        soft(&mut hurt);
        hurt.injured_min = 30;
        assert_ne!(demo::decide(&hurt), Some(Choice::Work), "injured");
    }

    #[test]
    fn a_broke_pet_goes_to_work_at_once_and_is_paid() {
        let mut s = rich(0);
        assert_eq!(demo::decide(&s), Some(Choice::Work), "no delay needed");
        let mut h = s.clone();
        demo::run(&mut s);
        // It works until it is out of the red zone (half the target).
        assert!(
            (target() / 2..target() / 2 + ONLYPETS_HOBBY_REWARD).contains(&s.money),
            "money {}",
            s.money
        );
        let mut one = rich(0);
        demo::act(&mut one, Choice::Work);
        assert_eq!(one.note().map(|(n, _)| n), Some(Note::Worked));
        assert_eq!(one.money, ONLYPETS_HOBBY_REWARD);
        // Hungry but cannot pay: the unaffordable meal is skipped, it works
        // first and eats on the pay.
        h.fullness = pct(5);
        assert_eq!(demo::decide(&h), Some(Choice::Work));
        demo::run(&mut h);
        assert!(h.fullness > pct(5), "it ate after working");
        assert!(h.money < s.money, "and paid for it");
    }

    #[test]
    fn the_pet_pays_for_its_own_care() {
        let mut s = rich(50);
        s.fullness = pct(5);
        s.set_demo_rng(10);
        let Some(Choice::Eat(food)) = demo::decide(&s) else {
            panic!("hungry pet should eat");
        };
        let price = bornhack_aegg::game::engine::doses::price(
            bornhack_aegg::game::engine::Action::Feed,
            Some(food),
            None,
            s.hard_mode,
        )
        .unwrap();
        demo::act(&mut s, Choice::Eat(food));
        assert_eq!(s.money, 50 - price);
    }

    #[test]
    fn a_simulated_mini_game_pays_like_a_real_one() {
        let mut s = rich(10);
        demo::act(&mut s, Choice::Game(MiniGame::BlackHole, 2));
        assert_eq!(s.money, 10 + MINIGAME_HAX_REWARD);
        assert!(s.cooldown_blackhole > 0);
    }

    fn drift(start: u32, demo_min: u32, chunks: &[u32]) -> u32 {
        let mut s = rich(start);
        s.demo.demo_min = demo_min;
        for &dt in chunks {
            advance(&mut s, dt);
        }
        s.money
    }

    #[test]
    fn above_the_target_the_balance_drifts_down_to_it_and_stops() {
        let every = t::DEMO_HAX_DRIFT_MIN() as u32;
        assert_eq!(drift(150, 0, &[every - 1]), 150);
        assert_eq!(drift(150, 0, &[every]), 149);
        assert_eq!(drift(150, 0, &[every * 5]), 145);
        assert_eq!(drift(target() + 3, 0, &[every * 50]), target());
        assert_eq!(drift(target(), 0, &[every * 50]), target());
    }

    #[test]
    fn below_the_target_there_is_no_free_top_up() {
        assert_eq!(drift(40, 0, &[1000]), 40);
        assert_eq!(drift(0, 0, &[1000]), 0);
    }

    #[test]
    fn drift_is_the_same_for_one_update_or_many() {
        let whole = drift(150, 7, &[60]);
        assert_eq!(whole, 144, "67 / 10 - 7 / 10 = 6 steps");
        assert_eq!(drift(150, 7, &[1; 60]), whole);
        assert_eq!(drift(150, 7, &[3, 11, 1, 25, 20]), whole);
        assert_eq!(drift(150, 7, &[59, 1]), whole);
    }

    #[test]
    fn drift_needs_demo_and_the_money_layer() {
        let mut s = rich(150);
        s.demo.on = false;
        advance(&mut s, 600);
        assert_eq!(s.money, 150);
        assert_eq!(drift_off_layer(), 150);
    }

    fn drift_off_layer() -> u32 {
        let mut s = rich(150);
        s.money_enabled = false;
        advance(&mut s, 600);
        s.money
    }

    /// Days of the pet looking after itself on its own pay, polled every
    /// half hour: (lowest balance, highest balance, least health pct).
    fn live(money: u32, days: u32, clock: bool) -> (u32, u32, u8) {
        let mut s = rich(money);
        if clock {
            super::policy::at(&mut s, 8 * 60);
        }
        let (mut lo, mut hi, mut health) = (money, money, 100u8);
        for _ in 0..(days * 48) {
            s.update(s.last_update_tick + 30 * bornhack_aegg::game::engine::TICKS_PER_MINUTE);
            lo = lo.min(s.money);
            hi = hi.max(s.money);
            health = health.min((s.health as i32 * 100 / 65535) as u8);
        }
        (lo, hi, health)
    }

    /// The pet pays for its care and earns the money back (shifts, games):
    /// it is never left without the price of a meal, stays about as healthy
    /// as without the money layer (53 and up in the same run), and never
    /// climbs above the target plus one pay.
    #[test]
    fn a_demo_pet_lives_on_its_own_pay_for_20_days() {
        for clock in [true, false] {
            for start in [0, target()] {
                let (lo, hi, health) = live(start, 20, clock);
                let what = format!("start {start} clock {clock}: lo {lo} hi {hi} health {health}");
                assert!(health >= 40, "{what}");
                assert!(hi <= target() + ONLYPETS_HOBBY_REWARD * 2, "{what}");
                if start > 0 {
                    assert!(lo >= 10, "{what}");
                }
            }
        }
    }

    /// A pet far above the target spends down on its own and then earns:
    /// it does not sit on the surplus, and it is not left broke.
    #[test]
    fn a_rich_demo_pet_spends_down_to_the_target() {
        let (lo, hi, health) = live(500, 20, true);
        assert_eq!(hi, 500);
        assert!(lo >= 10 && health >= 40, "lo {lo} health {health}");
    }

    #[test]
    fn turning_demo_off_keeps_the_balance() {
        let mut s = rich(130);
        s.demo.on = true;
        assert_eq!(s.set_demo(false), Ok(()));
        assert_eq!(s.money, 130);
    }
}

mod hint {
    use super::policy::{pct, quiet};
    use bornhack_aegg::game::engine::doses::{Reason, Refusal};
    use bornhack_aegg::game::engine::FoodKind;
    use bornhack_aegg::game::engine::hint::{Ask, Remedy, Subject, hint, subject_line};
    use bornhack_aegg::game::engine::status::Word;

    #[test]
    fn a_stiff_demo_pet_asks_for_exercise() {
        let mut s = quiet();
        s.fitness = pct(50);
        let h = hint(&s);
        assert_eq!(h.subject, Subject::Stiff);
        assert_eq!(h.ask, Ask::Do(Remedy::Exercise));
        // Demo off, the same pet is just content.
        s.demo.on = false;
        assert!(matches!(hint(&s).subject, Subject::Word(w) if w.is_info()));
        // A fit demo pet has nothing to ask.
        s.demo.on = true;
        s.fitness = pct(80);
        assert!(matches!(hint(&s).subject, Subject::Word(w) if w.is_info()));
    }

    /// C15: the plan's "never refused" assert is true by construction. The
    /// hint pins what it must: a refused remedy says why instead of asking.
    /// Injury is named by the status line (a problem word), so an injured
    /// pet is never stiff; it asks for medicine, and the exercise it must not
    /// be asked for is refused as `Injured`.
    #[test]
    fn an_injured_demo_pet_is_not_asked_to_exercise() {
        let mut s = quiet();
        s.fitness = pct(50);
        s.injured_min = 100;
        assert_eq!(
            s.remedy_reason(Remedy::Exercise),
            Some(Reason::Refused(Refusal::Injured))
        );
        let h = hint(&s);
        assert_ne!(h.subject, Subject::Stiff);
        assert_eq!(h.subject, Subject::Word(Word::Injured));
        assert_eq!(h.ask, Ask::Do(Remedy::Medicine));
    }

    #[test]
    fn a_stiff_demo_pet_in_the_middle_of_an_action_says_it_is_busy() {
        let mut s = quiet();
        s.fitness = pct(50);
        assert!(s.feed(FoodKind::Apple));
        let h = hint(&s);
        assert_eq!(h.subject, Subject::Stiff);
        assert_eq!(h.ask, Ask::Refused(Reason::Busy));
        assert_eq!(h.lines().1.as_str(), "but busy");
    }

    #[test]
    fn a_broke_stiff_demo_pet_still_asks_for_exercise_and_food() {
        // No money layer: money never refuses a hint.
        let mut s = quiet();
        s.money_enabled = false;
        s.money = 0;
        s.fitness = pct(50);
        assert_eq!(hint(&s).ask, Ask::Do(Remedy::Exercise));
        s.fitness = pct(60);
        s.fullness = pct(20);
        assert_eq!(hint(&s).ask, Ask::Do(Remedy::Feed));
    }

    #[test]
    fn stiff_fits() {
        assert_eq!(subject_line(Subject::Stiff), "I feel stiff.");
        assert!(subject_line(Subject::Stiff).len() <= 18);
    }
}

/// Long balance runs of the demo pet (spec 18.3, tuned in plan 3). The long
/// ones are `#[ignore]`d: CI runs them with `--include-ignored` (Task 9).
mod balance {
    use bornhack_aegg::game::engine::sim::{self, Driver, Outcome, Setup};

    const SEEDS: [u64; 5] = [1, 2, 3, 4, 5];
    const DEMO: Setup = Setup {
        demo: true,
        clock: true,
        animated: false,
        money: false,
    };
    const ANIMATED: Setup = Setup {
        animated: true,
        ..DEMO
    };

    fn run(
        routine: &dyn Fn(u32, u32, &bornhack_aegg::game::engine::GameState) -> sim::Plan,
        days: u32,
        driver: Driver,
        seed: u64,
        setup: Setup,
    ) -> Outcome {
        sim::run_with(routine, days, driver, seed, setup).outcome
    }

    /// The bounds every unattended 30-day demo pet keeps, clock or not.
    fn healthy(o: &Outcome, what: &str) {
        assert!(o.alive && !o.ever_leaving, "{what}: left: {o:?}");
        assert!(o.min_health_pct >= 40, "{what}: min health {o:?}");
        assert!(o.max_weight_pct <= 70, "{what}: max weight {o:?}");
        assert!(
            (38..=66).contains(&o.end_weight_pct),
            "{what}: end weight {o:?}"
        );
        assert!(!o.diabetic, "{what}: diabetic {o:?}");
        assert!(o.engine_wakes_per_day <= 60.0, "{what}: wakes {o:?}");
    }

    /// The full set for a clock run; `refresh_cap` is per care mode.
    fn alone(o: &Outcome, refresh_cap: f32, what: &str) {
        healthy(o, what);
        assert!((50..=80).contains(&o.avg_mood_pct), "{what}: mood {o:?}");
        assert!(o.miserable_pct <= 2, "{what}: miserable {o:?}");
        assert!(o.thriving_pct >= 5, "{what}: thriving {o:?}");
        assert!(o.min_fitness_pct >= 45, "{what}: min fitness {o:?}");
        assert!(
            (10.0..=20.0).contains(&o.self_care_per_day),
            "{what}: self-care {o:?}"
        );
        assert!(
            o.refreshes_per_day <= refresh_cap,
            "{what}: refreshes {o:?}"
        );
        assert!(o.causes_per_day <= 6.0, "{what}: causes {o:?}");
        assert!(o.night_asleep_pct >= 85, "{what}: night asleep {o:?}");
        assert!(o.day_asleep_pct <= 20, "{what}: day asleep {o:?}");
    }

    /// The pet's own care is not so eager that it lives in the top band
    /// (spec 18.3: mood 57 to 65). Ten days and three seeds, short enough
    /// to run locally; the 30-day runs below hold the full bounds.
    #[test]
    fn demo_mood_stays_in_the_band_for_10_days() {
        for seed in [1, 2, 3] {
            let o = run(&sim::absent, 10, Driver::Delta, seed, DEMO);
            eprintln!("10 days seed {seed}: {o:?}");
            assert!(
                (50..=80).contains(&o.avg_mood_pct),
                "seed {seed}: mood {o:?}"
            );
            assert!(o.miserable_pct <= 2, "seed {seed}: miserable {o:?}");
        }
    }

    #[test]
    #[ignore]
    fn demo_alone_for_30_days() {
        for seed in SEEDS {
            let o = run(&sim::absent, 30, Driver::Delta, seed, DEMO);
            eprintln!("instant seed {seed}: {o:?}");
            alone(&o, 60.0, &format!("instant seed {seed}"));
        }
    }

    #[test]
    #[ignore]
    fn animated_demo_alone_for_30_days() {
        for seed in SEEDS {
            let i = run(&sim::absent, 30, Driver::Delta, seed, DEMO);
            let a = run(&sim::absent, 30, Driver::Delta, seed, ANIMATED);
            eprintln!(
                "seed {seed}: refreshes/day instant {:.1}, animated {:.1}",
                i.refreshes_per_day, a.refreshes_per_day
            );
            eprintln!("animated seed {seed}: {a:?}");
            alone(&a, 220.0, &format!("animated seed {seed}"));
        }
    }

    #[test]
    #[ignore]
    fn demo_without_a_clock_for_30_days() {
        let setup = Setup {
            clock: false,
            ..DEMO
        };
        for seed in SEEDS {
            let o = run(&sim::absent, 30, Driver::Delta, seed, setup);
            eprintln!("no clock seed {seed}: {o:?}");
            healthy(&o, &format!("no clock seed {seed}"));
        }
    }

    #[test]
    #[ignore]
    fn demo_with_a_helper_for_30_days() {
        for seed in SEEDS {
            let nobody = run(&sim::absent, 30, Driver::Delta, seed, DEMO);
            let helped = run(&sim::helper, 30, Driver::Delta, seed, DEMO);
            eprintln!("helper seed {seed}: {helped:?}");
            assert!(
                helped.alive && !helped.ever_leaving,
                "seed {seed}: {helped:?}"
            );
            assert!(
                helped.avg_mood_pct >= nobody.avg_mood_pct,
                "seed {seed}: helper {} < nobody {}",
                helped.avg_mood_pct,
                nobody.avg_mood_pct
            );
            assert!(helped.max_weight_pct <= 70, "seed {seed}: {helped:?}");
        }
    }

    fn near(a: u8, b: u8, tol: u8, what: &str) {
        assert!(a.abs_diff(b) <= tol, "{what}: oracle {a} vs delta {b}");
    }

    /// The two drivers agree by the rule of `oracle::delta_matches_oracle_all_routines`
    /// (alive equal; then mood within 1, end health and fitness within 2, injuries
    /// equal), plus the figures this plan adds: the discrete facts equal, the
    /// integrated figures within the same 1 or 2 points, the event counts (self-care,
    /// causes) within one event over the run. `Outcome` has f32 fields, so there is no
    /// `PartialEq` to lean on.
    fn same(o: &Outcome, d: &Outcome, days: u32, what: &str) {
        assert_eq!(o.alive, d.alive, "{what}: alive");
        if !(o.alive && d.alive) {
            return;
        }
        assert_eq!(o.phase, d.phase, "{what}: phase");
        assert_eq!(o.ever_leaving, d.ever_leaving, "{what}: leaving");
        assert_eq!(o.diabetic, d.diabetic, "{what}: diabetic");
        assert_eq!(o.injuries, d.injuries, "{what}: injuries");
        near(o.avg_mood_pct, d.avg_mood_pct, 1, &format!("{what}: mood"));
        near(
            o.end_health_pct,
            d.end_health_pct,
            2,
            &format!("{what}: end health"),
        );
        near(
            o.min_health_pct,
            d.min_health_pct,
            2,
            &format!("{what}: min health"),
        );
        near(
            o.end_fitness_pct,
            d.end_fitness_pct,
            2,
            &format!("{what}: end fitness"),
        );
        near(
            o.min_fitness_pct,
            d.min_fitness_pct,
            2,
            &format!("{what}: min fitness"),
        );
        near(
            o.end_weight_pct,
            d.end_weight_pct,
            2,
            &format!("{what}: end weight"),
        );
        near(
            o.max_weight_pct,
            d.max_weight_pct,
            2,
            &format!("{what}: max weight"),
        );
        let shares = |x: &Outcome| {
            [
                x.thriving_pct,
                x.fine_pct,
                x.off_pct,
                x.poorly_pct,
                x.miserable_pct,
            ]
        };
        for (i, (a, b)) in shares(o).into_iter().zip(shares(d)).enumerate() {
            near(a, b, 2, &format!("{what}: mood band {i}"));
        }
        near(
            o.night_asleep_pct,
            d.night_asleep_pct,
            2,
            &format!("{what}: night asleep"),
        );
        near(
            o.day_asleep_pct,
            d.day_asleep_pct,
            2,
            &format!("{what}: day asleep"),
        );
        let events = |a: f32, b: f32, name: &str| {
            let gap = (a - b).abs() * o.days.max(days as f32);
            assert!(gap <= 1.01, "{what}: {name} oracle {a} vs delta {b}");
        };
        events(o.self_care_per_day, d.self_care_per_day, "self-care");
        events(o.causes_per_day, d.causes_per_day, "causes");
    }

    type Routine = fn(u32, u32, &bornhack_aegg::game::engine::GameState) -> sim::Plan;
    const ROUTINES: [(&str, Routine); 2] = [("nobody", sim::absent), ("helper", sim::helper)];

    /// Clock off: the drivers see the same pet, so they agree by the strict
    /// reference rule (`same`).
    fn agree_without_a_clock(days: u32, seeds: &[u64]) {
        for (name, r) in ROUTINES {
            for animated in [false, true] {
                for &seed in seeds {
                    let setup = Setup {
                        clock: false,
                        animated,
                        ..DEMO
                    };
                    let o = run(&r, days, Driver::Oracle, seed, setup);
                    let d = run(&r, days, Driver::Delta, seed, setup);
                    same(
                        &o,
                        &d,
                        days,
                        &format!("{name} animated {animated} seed {seed}"),
                    );
                }
            }
        }
    }

    // Clock on: not exact, not even per seed. `day_min` only moves in
    // `advance`, so it lags by the ticks an action spends, and the two drivers
    // re-sync the clock at different moments (the oracle every minute, the
    // delta at its wakes). `arm_clock` then reads a minute off now and then,
    // and from there each seed's dice take another path (debug-runaway-report.md:
    // no loop, no battery cost). What the drivers share is the distribution:
    // per seed neither runs away; over the seeds the mean mood and the mean
    // self-care rate agree. Only the Delta driver has engine wakes, so only it
    // is held to the wake bound.
    fn agree_with_a_clock(days: u32, seeds: &[u64]) {
        for (name, r) in ROUTINES {
            for animated in [false, true] {
                let setup = Setup { animated, ..DEMO };
                let (mut mood, mut care) = ([0.0f32; 2], [0.0f32; 2]);
                for &seed in seeds {
                    let what = format!("{name} animated {animated} seed {seed}");
                    let o = run(&r, days, Driver::Oracle, seed, setup);
                    let d = run(&r, days, Driver::Delta, seed, setup);
                    assert!(o.alive && d.alive, "{what}: alive");
                    for (i, (driver, x)) in [("oracle", &o), ("delta", &d)].into_iter().enumerate()
                    {
                        assert!(
                            x.self_care_per_day <= 20.0,
                            "{what}: {driver} self-care {}",
                            x.self_care_per_day
                        );
                        mood[i] += x.avg_mood_pct as f32 / seeds.len() as f32;
                        care[i] += x.self_care_per_day / seeds.len() as f32;
                    }
                    assert!(d.engine_wakes_per_day <= 60.0, "{what}: wakes {d:?}");
                }
                let what = format!("{name} animated {animated}");
                assert!(
                    (mood[0] - mood[1]).abs() <= 3.0,
                    "{what}: mean mood oracle {} vs delta {}",
                    mood[0],
                    mood[1]
                );
                assert!(
                    (care[0] - care[1]).abs() <= 0.1 * care[0].max(care[1]),
                    "{what}: mean self-care oracle {} vs delta {}",
                    care[0],
                    care[1]
                );
            }
        }
    }

    #[test]
    #[ignore]
    fn demo_delta_matches_oracle() {
        agree_without_a_clock(10, &[1, 2, 3]);
        agree_with_a_clock(10, &[1, 2, 3]);
    }

    /// Previews and look-aheads run on clones; their causes are not the pet's.
    /// Nobody with instant care is the case where the drivers' pets stay the
    /// same pet (the others diverge, see the Task 8 fix report), so any gap
    /// here is a counting gap.
    #[test]
    fn causes_count_only_the_pets_own_life() {
        for seed in [1, 2, 3] {
            let o = run(&sim::absent, 10, Driver::Oracle, seed, DEMO);
            let d = run(&sim::absent, 10, Driver::Delta, seed, DEMO);
            assert!(o.causes_per_day > 0.0, "seed {seed}: no causes to compare");
            assert_eq!(o.causes_per_day, d.causes_per_day, "seed {seed}");
            assert_eq!(o.self_care_per_day, d.self_care_per_day, "seed {seed}");
        }
    }

    #[test]
    fn demo_smoke_two_days() {
        let instant = run(&sim::absent, 2, Driver::Delta, 1, DEMO);
        let animated = run(&sim::absent, 2, Driver::Delta, 1, ANIMATED);
        for (name, o) in [("instant", &instant), ("animated", &animated)] {
            eprintln!(
                "smoke {name}: self-care {:.1}/day, refreshes {:.1}/day, wakes {:.1}/day, \
                 mood {} thriving {} fine {} off {} poorly {} miserable {} \
                 night asleep {} day asleep {} causes {:.1}/day",
                o.self_care_per_day,
                o.refreshes_per_day,
                o.engine_wakes_per_day,
                o.avg_mood_pct,
                o.thriving_pct,
                o.fine_pct,
                o.off_pct,
                o.poorly_pct,
                o.miserable_pct,
                o.night_asleep_pct,
                o.day_asleep_pct,
                o.causes_per_day,
            );
            assert!(o.alive, "{name}: {o:?}");
            assert!(!o.ever_leaving, "{name}: {o:?}");
            assert!(o.self_care_per_day > 0.0, "{name}: {o:?}");
        }
        assert!(
            animated.refreshes_per_day > instant.refreshes_per_day,
            "animated {} vs instant {}",
            animated.refreshes_per_day,
            instant.refreshes_per_day
        );
    }
}

/// An overslept pet put straight back to bed (debug-runaway report): it was
/// awake below the oversleep reset line, so the oversleep is over and it is
/// not woken (and made groggy) again a minute later, every minute.
mod oversleep {
    use super::policy::{at, lcg, pct, quiet};
    use bornhack_aegg::game::engine::causes::Cause;
    use bornhack_aegg::game::engine::thresholds as t;
    use bornhack_aegg::game::engine::{GameState, TICKS_PER_MINUTE, sim};

    /// Wall minute of the day the runs below start at.
    const START: u32 = 6 * 60 + 20;

    /// A worn-out pet in bed at 06:20 at its deprived energy cap, five
    /// minutes short of the oversleep wake.
    fn overslept(demo: bool, animated: bool) -> GameState {
        let mut s = quiet();
        s.demo.on = demo;
        s.set_demo_care(animated);
        s.deprived = true;
        s.energy = pct(t::ENERGY_CAP_DEPRIVED_PCT() as i32);
        assert!(s.sleep());
        s.demo.pause_min = 0;
        s.sleep_min = 400;
        s.oversleep_min = t::OVERSLEEP_WAKE_MIN() - 5;
        at(&mut s, START as u16);
        s
    }

    /// The badge's pattern: wake at the engine's next wake or at a button
    /// press (irregular minutes, some mid-minute), whichever comes first,
    /// and sync the wall clock before each update. Returns (self-care acts,
    /// updates, groggy wakes).
    fn badge_run(s: &mut GameState, hours: u32, seed: u64) -> (u32, u32, u32) {
        let mut next = lcg(seed);
        let t0 = s.last_update_tick;
        let end = t0 + hours * 60 * TICKS_PER_MINUTE;
        let acts = sim::self_care_acts();
        let (mut updates, mut groggy) = (0, 0);
        while s.last_update_tick < end {
            let press = s.last_update_tick + 1 + (next(23 * TICKS_PER_MINUTE as u64) as u32);
            let to = s.next_wake_tick().min(press).min(end);
            let wall = (START + (to - t0) / TICKS_PER_MINUTE) % 1440;
            s.sync_clock(to, Some(wall as u16));
            s.update(to);
            updates += 1;
            groggy += s
                .take_causes()
                .iter()
                .filter(|&&c| c == Cause::Groggy)
                .count() as u32;
        }
        (sim::self_care_acts() - acts, updates, groggy)
    }

    #[test]
    fn a_demo_pet_back_in_bed_is_not_woken_every_minute() {
        for animated in [false, true] {
            for seed in 1..=4 {
                let mut s = overslept(true, animated);
                let (acts, updates, groggy) = badge_run(&mut s, 6, seed);
                let what = format!("animated {animated}, seed {seed}");
                assert_eq!(groggy, 1, "{what}: groggy wakes");
                assert!(acts <= 12, "{what}: {acts} self-care acts in 6 h");
                assert!(updates <= 6 * 60 / 4, "{what}: {updates} updates in 6 h");
            }
        }
    }

    /// Bedtime under the reset line wipes the carried oversleep, so the
    /// Sleep row does not forecast the old groggy wake for the new night.
    #[test]
    fn the_sleep_forecast_reads_the_oversleep_after_bedtime() {
        let mut s = overslept(false, false);
        s.update(s.last_update_tick + 5 * TICKS_PER_MINUTE);
        assert!(!s.is_sleeping, "the oversleep wake");
        assert!(s.take_causes().iter().any(|&c| c == Cause::Groggy));
        assert_ne!(s.preview_sleep(), Some(Cause::Groggy));
    }

    #[test]
    fn a_player_can_put_an_overslept_pet_back_to_bed() {
        let mut s = overslept(false, false);
        let t0 = s.last_update_tick;
        s.update(t0 + 5 * TICKS_PER_MINUTE);
        assert!(!s.is_sleeping, "the oversleep wake");
        assert!(s.take_causes().iter().any(|&c| c == Cause::Groggy));
        assert!(s.sleep());
        s.update(s.last_update_tick + 30 * TICKS_PER_MINUTE);
        assert!(s.is_sleeping, "woken again: oversleep {}", s.oversleep_min);
        assert!(!s.take_causes().iter().any(|&c| c == Cause::Groggy));
    }
}
