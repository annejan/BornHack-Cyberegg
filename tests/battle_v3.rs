//! BornPets v3 battles (plan 5). Task 1: the snapshot.

mod snapshot {
    use bornhack_aegg::game::bornpets_cfg::apply_overrides;
    use bornhack_aegg::game::engine::combat::{
        Body, Factor, Flags, WIRE_FOCUS, WIRE_GUARD, WIRE_HP, WIRE_POWER, WIRE_SPEED, WIRE_STAMINA,
        derive, derive_raw, felt, felt_with,
    };
    use bornhack_aegg::game::engine::needs::P;
    use bornhack_aegg::game::engine::thresholds::Thresholds;
    use bornhack_aegg::game::engine::{GameState, PetKind, Phase};

    /// The prototype builds (`battle::builds`) as `Body`, traits 50.
    pub(super) fn builds() -> [(&'static str, Body); 20] {
        let b = Body::REFERENCE;
        let fit = Body {
            fit: 90,
            energy: 75,
            mood: 85,
            health: 95,
            ..b
        };
        let sumo = Body {
            weight: 80,
            fit: 80,
            energy: 75,
            mood: 85,
            health: 90,
            ..b
        };
        let fat = Body {
            weight: 85,
            fit: 15,
            energy: 65,
            mood: 70,
            health: 55,
            ..b
        };
        [
            ("fit, normal weight", fit),
            ("fit, one beer", Body { booze: 15, ..fit }),
            ("thin and fit", Body { weight: 30, ..fit }),
            ("sumo (heavy, fit)", sumo),
            ("sumo, two beers", Body { booze: 30, ..sumo }),
            (
                "well rested",
                Body {
                    fit: 75,
                    energy: 95,
                    mood: 85,
                    health: 90,
                    rested: true,
                    ..b
                },
            ),
            ("average", b),
            ("fat and unfit", fat),
            ("fat, unfit, three beers", Body { booze: 45, ..fat }),
            (
                "thin and unfit",
                Body {
                    weight: 25,
                    fit: 20,
                    health: 55,
                    mood: 65,
                    ..b
                },
            ),
            (
                "wasted",
                Body {
                    booze: 70,
                    energy: 60,
                    ..b
                },
            ),
            (
                "dependent, sober (craving)",
                Body {
                    craving: true,
                    mood: 55,
                    ..b
                },
            ),
            (
                "wired",
                Body {
                    fun: 92,
                    energy: 55,
                    ..b
                },
            ),
            (
                "bored",
                Body {
                    fun: 8,
                    mood: 55,
                    ..b
                },
            ),
            (
                "hungry",
                Body {
                    full: 18,
                    mood: 60,
                    ..b
                },
            ),
            (
                "stuffed",
                Body {
                    full: 95,
                    energy: 60,
                    ..b
                },
            ),
            ("thirsty", Body { water: 25, ..b }),
            (
                "exhausted",
                Body {
                    energy: 15,
                    mood: 55,
                    ..b
                },
            ),
            (
                "hung over",
                Body {
                    hangover: true,
                    energy: 45,
                    mood: 50,
                    water: 30,
                    ..b
                },
            ),
            (
                "injured",
                Body {
                    injured: true,
                    strain: 50,
                    mood: 55,
                    ..b
                },
            ),
        ]
    }

    pub(super) fn build(name: &str) -> Body {
        builds().iter().find(|(n, _)| *n == name).unwrap().1
    }

    #[test]
    fn builds_match_section_18_4() {
        // (power, guard, speed, hp, stamina, focus), Classic preset, from
        // `cargo run -- battle` at 5e59574.
        let want = [
            (39, 16, 71, 75, 4, 0),
            (43, 15, 71, 75, 4, -2),
            (33, 14, 75, 70, 4, 0),
            (47, 23, 49, 80, 3, 0),
            (52, 21, 49, 80, 3, -3),
            (39, 16, 70, 72, 6, 5),
            (36, 16, 59, 68, 3, 0),
            (45, 25, 20, 60, 1, 0),
            (52, 22, 20, 60, 1, -4),
            (33, 14, 48, 60, 2, 0),
            (36, 14, 59, 68, 3, -48),
            (33, 16, 59, 68, 3, -10),
            (36, 16, 59, 68, 3, -19),
            (36, 16, 59, 68, 3, -12),
            (36, 16, 59, 68, 1, 0),
            (36, 16, 43, 68, 3, 0),
            (36, 16, 59, 68, 2, -4),
            (36, 16, 59, 68, 1, 0),
            (36, 16, 51, 68, 1, -14),
            (20, 16, 35, 68, 3, 0),
        ];
        for ((name, body), w) in builds().iter().zip(want) {
            let s = derive(body);
            let got = (
                s.power as i32,
                s.guard as i32,
                s.speed as i32,
                s.hp as i32,
                s.stamina as i32,
                s.focus as i32,
            );
            assert_eq!(got, w, "{name}");
        }
    }

    #[test]
    fn flags_name_the_conditions() {
        let on = |name: &str| derive(&build(name)).flags;
        let sumo = on("sumo, two beers");
        assert!(sumo.has(Factor::Heavy) && sumo.has(Factor::Tipsy));
        assert!(!sumo.has(Factor::Sloppy));
        assert!(on("wasted").has(Factor::Sloppy));
        let over = on("hung over");
        assert!(over.has(Factor::HungOver) && over.has(Factor::Thirsty));
        let hurt = on("injured");
        assert!(hurt.has(Factor::Injured) && hurt.has(Factor::Sore));
        assert!(on("well rested").has(Factor::Rested));
        assert_eq!(on("average").0 & 0xF_FFFF, 0, "average pet has no factor");
        assert!(!on("average").demo());
        let demo = derive(&Body {
            demo: true,
            ..Body::REFERENCE
        })
        .flags;
        assert!(demo.demo());
        assert_eq!(demo.0 & (1 << 23), 1 << 23);
        assert_eq!(demo.0 & (1 << 22), 0, "bit 22 stays reserved");
        assert!(Flags::default().with(Factor::Filthy).has(Factor::Filthy));
        assert!(Flags::default().with_demo(true).demo());
    }

    fn xorshift(s: &mut u32) -> u32 {
        let mut x = *s;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        *s = x;
        x
    }

    fn body_from(fields: [i32; 14], flags: [bool; 7]) -> Body {
        Body {
            fit: fields[0],
            energy: fields[1],
            booze: fields[2],
            mood: fields[3],
            health: fields[4],
            strain: fields[5],
            full: fields[6],
            water: fields[7],
            fun: fields[8],
            dirt: fields[9],
            weight: fields[10],
            vit: fields[11],
            cur: fields[12],
            res: fields[13],
            rested: flags[0],
            injured: flags[1],
            groggy: flags[2],
            hangover: flags[3],
            craving: flags[4],
            deprived: flags[5],
            demo: flags[6],
        }
    }

    fn check_raw(b: &Body) {
        let r = derive_raw(b);
        let inside = |v: i32, (lo, hi): (u8, u8)| (lo as i32..=hi as i32).contains(&v);
        assert!(inside(r[0], WIRE_POWER), "power {} for {b:?}", r[0]);
        assert!(inside(r[1], WIRE_GUARD), "guard {} for {b:?}", r[1]);
        assert!(inside(r[2], WIRE_SPEED), "speed {} for {b:?}", r[2]);
        assert!(inside(r[3], WIRE_HP), "hp {} for {b:?}", r[3]);
        assert!(inside(r[4], WIRE_STAMINA), "stamina {} for {b:?}", r[4]);
        // Only focus is clamped: it has no floor in the formula.
        let s = derive(b);
        assert_eq!(
            s.focus as i32,
            r[5].clamp(WIRE_FOCUS.0 as i32, WIRE_FOCUS.1 as i32)
        );
        assert_eq!(
            [
                s.power as i32,
                s.guard as i32,
                s.speed as i32,
                s.hp as i32,
                s.stamina as i32
            ],
            [r[0], r[1], r[2], r[3], r[4]]
        );
    }

    #[test]
    fn snapshot_stays_in_wire_ranges() {
        let mut seed = 0x9E37_79B9;
        for _ in 0..20_000 {
            let mut f = [0i32; 14];
            for v in &mut f {
                *v = (xorshift(&mut seed) % 101) as i32;
            }
            let bits = xorshift(&mut seed);
            let mut fl = [false; 7];
            for (i, v) in fl.iter_mut().enumerate() {
                *v = bits >> i & 1 == 1;
            }
            check_raw(&body_from(f, fl));
        }
        for level in [0, 100] {
            for mask in 0..128u32 {
                let mut fl = [false; 7];
                for (i, v) in fl.iter_mut().enumerate() {
                    *v = mask >> i & 1 == 1;
                }
                check_raw(&body_from([level; 14], fl));
            }
        }
    }

    /// One body per number that reaches its true maximum under the presets.
    /// Pins the headroom of the WIRE ranges: random sampling never gets there.
    #[test]
    fn worst_cases_reach_the_wire_maxima() {
        let b = Body::REFERENCE;
        let cases = [
            (
                "power",
                Body {
                    weight: 100,
                    booze: 60,
                    mood: 85,
                    ..b
                },
                0,
                61,
                WIRE_POWER,
            ),
            (
                "guard",
                Body {
                    res: 100,
                    weight: 100,
                    ..b
                },
                1,
                39,
                WIRE_GUARD,
            ),
            (
                "speed",
                Body {
                    weight: 0,
                    fit: 100,
                    vit: 100,
                    rested: true,
                    ..b
                },
                2,
                100,
                WIRE_SPEED,
            ),
            (
                "hp",
                Body {
                    vit: 100,
                    health: 100,
                    weight: 100,
                    ..b
                },
                3,
                117,
                WIRE_HP,
            ),
            (
                "stamina",
                Body {
                    energy: 100,
                    fit: 100,
                    vit: 100,
                    rested: true,
                    ..b
                },
                4,
                9,
                WIRE_STAMINA,
            ),
        ];
        for (name, body, i, max, (lo, hi)) in cases {
            let v = derive_raw(&body)[i];
            assert_eq!(v, max, "{name}");
            assert!(
                (lo as i32..=hi as i32).contains(&v),
                "{name} {v} outside wire"
            );
        }
    }

    #[test]
    fn felt_never_goes_negative() {
        // A custom base below the weight: booze * (base - weight) is negative.
        let heavy_drunk = Body {
            booze: 100,
            weight: 100,
            ..Body::REFERENCE
        };
        assert_eq!(felt_with(&heavy_drunk, 20), 0);
        assert_eq!(felt_with(&heavy_drunk, 1000), 100);
        assert_eq!(felt_with(&heavy_drunk, 150), felt(&heavy_drunk));
    }

    #[test]
    fn mood_band_pair_is_validated() {
        use bornhack_aegg::game::engine::thresholds::validate;
        let mut t = Thresholds::CLASSIC;
        t.BATTLE_LOW_MOOD_PCT = 80;
        t.BATTLE_HIGH_MOOD_PCT = 60;
        let v = validate(&t, &Thresholds::CLASSIC);
        assert_eq!((v.BATTLE_LOW_MOOD_PCT, v.BATTLE_HIGH_MOOD_PCT), (40, 80));
    }

    #[test]
    fn felt_is_the_one_formula() {
        // booze * (150 - weight) / 100, at most 100
        assert_eq!(
            felt(&Body {
                booze: 30,
                weight: 80,
                ..Body::REFERENCE
            }),
            21
        );
        assert_eq!(
            felt(&Body {
                booze: 100,
                weight: 0,
                ..Body::REFERENCE
            }),
            100
        );
        assert_eq!(
            felt(&Body {
                booze: 0,
                ..Body::REFERENCE
            }),
            0
        );
    }

    #[test]
    fn snapshot_of_a_pet_reads_its_state() {
        let mut s = GameState::new_egg(7, PetKind(0));
        s.phase = Phase::Active;
        s.weight = (80 * P) as u16;
        s.fitness = (80 * P) as u16;
        s.booze = (20 * P) as u16;
        let b = Body::of(&s);
        assert_eq!(b.weight, 80);
        assert_eq!(b.fit, 80);
        assert_eq!(b.booze, 20);
        assert!(!b.demo && !b.injured && !b.rested);
        assert_eq!(s.snapshot(), derive(&b));
        s.injured_min = 5;
        assert!(Body::of(&s).injured && s.snapshot().flags.has(Factor::Injured));
    }

    #[test]
    fn bands_move_the_fight() {
        // Classic FULL_LO is 38: 36 is hungry, 40 is not.
        let hungry = |full| {
            derive(&Body {
                full,
                ..Body::REFERENCE
            })
            .flags
            .has(Factor::Hungry)
        };
        assert!(hungry(36));
        assert!(!hungry(40));
        // The accessors decide, not literals: none of the band literals of
        // the prototype survive in the source (comments stripped).
        let src: String = include_str!("../src/game/engine/combat.rs")
            .lines()
            .map(|l| l.split("//").next().unwrap())
            .collect::<Vec<_>>()
            .join("\n");
        for lit in [" 78", " 83", " 38", " 33", " 73", " 58", " 63"] {
            assert!(!src.contains(lit), "band literal{lit} in combat.rs");
        }
        // Stricter: the number as a whole token, whatever precedes it.
        let b = src.as_bytes();
        for lit in ["78", "83", "38", "33", "73", "58", "63"] {
            for (i, _) in src.match_indices(lit) {
                let word = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
                let before = i > 0 && word(b[i - 1]);
                let after = b.get(i + 2).is_some_and(|&c| word(c));
                assert!(before || after, "band literal {lit} in combat.rs");
            }
        }
    }

    #[test]
    fn felt_base_survives_the_cfg_load() {
        // A percent key would clamp to 1..=99 and break every cell.
        let mut t = Thresholds::CLASSIC;
        apply_overrides(&mut t, b"BATTLE_FELT_BASE_PCT=150\n");
        assert_eq!(t.BATTLE_FELT_BASE_PCT, 150);
        apply_overrides(&mut t, b"BATTLE_FELT_BASE_PCT=200\n");
        assert_eq!(t.BATTLE_FELT_BASE_PCT, 200);
    }

    #[test]
    fn factor_words_are_listed_once() {
        let mut seen = std::collections::HashSet::new();
        for (i, f) in Factor::ALL.iter().enumerate() {
            assert!(seen.insert(f.text()), "duplicate {}", f.text());
            assert_eq!(f.helps(), i < 4, "{}", f.text());
            assert_eq!(*f as usize, i);
            assert!(f.text().len() <= 11, "{} too wide", f.text());
        }
        assert_eq!(seen.len(), 20);
    }
}

mod fight {
    use super::snapshot::{build, builds};
    use bornhack_aegg::game::engine::combat::{
        Body, CHANCE_FIGHTS, Decisive, Factor, Form, ROUNDS, Snapshot, chance_seed, decisive,
        derive, factors, fight, form, seed, win_chance,
    };
    use bornhack_aegg::game::engine::thresholds as t;

    const GOLDEN_SEED: u32 = 1_560_395_003;

    fn snap(name: &str) -> Snapshot {
        derive(&build(name))
    }

    /// `w` wins of `CHANCE_FIGHTS` as a percent, rounded to the nearest 5.
    fn round5(w: u32) -> u8 {
        (((w * 100 + CHANCE_FIGHTS / 2) / CHANCE_FIGHTS + 2) / 5 * 5) as u8
    }

    #[test]
    fn matrix_matches_section_18_4() {
        let bs = builds();
        let snaps: Vec<Snapshot> = bs.iter().map(|(_, b)| derive(b)).collect();
        let mut avgs = Vec::new();
        let mut cells = vec![vec![0u32; 20]; 20];
        for i in 0..20 {
            let (mut tot, mut cnt) = (0, 0);
            for j in 0..20 {
                let mut w = 0u32;
                let base = ((i * 131 + j * 17) as u32).wrapping_add(7);
                for k in 0..2000u32 {
                    let s = k.wrapping_mul(2654435761).wrapping_add(base);
                    if fight(s, &snaps[i], &snaps[j]).a_won {
                        w += 1;
                    }
                }
                let p = w * 100 / 2000;
                cells[i][j] = p;
                if i != j {
                    tot += p;
                    cnt += 1;
                }
            }
            avgs.push(tot / cnt);
        }
        assert_eq!(
            avgs,
            [
                82, 84, 74, 85, 86, 81, 60, 41, 42, 22, 9, 44, 41, 48, 33, 48, 44, 34, 20, 2
            ]
        );
        assert_eq!(cells[0][3], 44, "fit vs sumo");
        assert_eq!(cells[3][0], 53, "sumo vs fit");
    }

    #[test]
    fn fight_is_deterministic_and_bounded() {
        let bs = builds();
        for (i, (_, a)) in bs.iter().enumerate() {
            for (j, (_, b)) in bs.iter().enumerate() {
                let (sa, sb) = (derive(a), derive(b));
                for k in 0..8u32 {
                    let s = k.wrapping_mul(2654435761).wrapping_add((i * 20 + j) as u32);
                    let o = fight(s, &sa, &sb);
                    assert_eq!(o, fight(s, &sa, &sb));
                    assert!(o.rounds <= ROUNDS && o.rounds >= 1);
                    assert!(o.a_hp_pct <= 100 && o.b_hp_pct <= 100);
                }
            }
        }
    }

    #[test]
    fn fight_reads_no_config() {
        let src = include_str!("../src/game/engine/combat.rs");
        let from = src.find("pub const ROUNDS").expect("consts");
        let to = src.find("pub fn form").expect("form");
        assert!(from < to);
        let order = [
            "pub const ROUNDS",
            "fn strike",
            "pub fn fight",
            "pub fn seed",
            "pub fn chance_seed",
            "pub fn win_chance",
            "pub fn form",
        ];
        let mut at = 0;
        for o in order {
            let p = src[at..]
                .find(o)
                .unwrap_or_else(|| panic!("{o} out of order"));
            at += p + o.len();
        }
        let stripped: String = src[from..to]
            .lines()
            .map(|l| l.split("//").next().unwrap())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(!stripped.contains("thresholds"), "fight reads thresholds");
        let b = stripped.as_bytes();
        let word = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
        for (i, _) in stripped.match_indices("t::") {
            let before = i > 0 && word(b[i - 1]);
            assert!(
                before,
                "fight reads a threshold: {}",
                &stripped[i.saturating_sub(10)..i + 10]
            );
        }
    }

    #[test]
    fn seed_is_symmetric_in_roles() {
        let (na, nb, ia, ib) = (0x1234, 0x5678, [1, 2], [3, 4]);
        let s = seed(na, nb, ia, ib);
        assert_eq!(s, seed(na, nb, ia, ib));
        // Swapping the roles changes the seed: the challenger goes first on both badges.
        assert_ne!(s, seed(na, nb, ib, ia));
        assert_ne!(s, seed(nb, na, ia, ib));
        assert_ne!(s, seed(na ^ 1, nb, ia, ib));
        assert_ne!(s, seed(na, nb ^ 1, ia, ib));
        assert_ne!(s, seed(na, nb, ia, [ib[0] ^ 1, ib[1]]));
        // Golden values: the wire contract must not drift between builds.
        assert_eq!(s, GOLDEN_SEED);
        assert_eq!(chance_seed(0), 7);
        assert_eq!(chance_seed(1), 2_654_435_768);
        assert_eq!(
            chance_seed(64),
            64u32.wrapping_mul(2_654_435_761).wrapping_add(7)
        );
        // A fallback fight uses nonce_b = 0.
        assert_ne!(seed(na, 0, ia, ib), s);
    }

    #[test]
    fn chance_is_honest() {
        let bs = builds();
        for (_, a) in bs.iter() {
            for (_, b) in bs.iter() {
                let (sa, sb) = (derive(a), derive(b));
                let w = (0..CHANCE_FIGHTS)
                    .filter(|&k| fight(chance_seed(k), &sa, &sb).a_won)
                    .count() as u32;
                let c = win_chance(&sa, &sb);
                assert_eq!(c, round5(w));
                assert_eq!(c % 5, 0);
            }
        }
        for (n, b) in bs.iter() {
            let s = derive(b);
            assert!((40..=60).contains(&win_chance(&s, &s)), "{n} mirror");
        }
        for n in ["average", "fit, normal weight"] {
            let s = snap(n);
            assert!((45..=55).contains(&win_chance(&s, &s)), "{n} mirror");
        }
        assert!(win_chance(&snap("fit, normal weight"), &snap("injured")) >= 95);
        assert!(win_chance(&snap("injured"), &snap("fit, normal weight")) <= 5);
    }

    #[test]
    fn form_words_follow_the_chance() {
        assert_eq!(form(&snap("fit, normal weight")), Form::Top);
        assert_eq!(form(&snap("hung over")), Form::Poor);
        assert_eq!(form(&snap("injured")), Form::Poor);
        let reference = derive(&Body::REFERENCE);
        for (n, b) in builds().iter() {
            let s = derive(b);
            let c = win_chance(&s, &reference) as u32;
            let want = if c >= t::BATTLE_FORM_TOP_PCT() as u32 {
                Form::Top
            } else if c >= t::BATTLE_FORM_IN_PCT() as u32 {
                Form::In
            } else if c >= t::BATTLE_FORM_OFF_PCT() as u32 {
                Form::Off
            } else {
                Form::Poor
            };
            assert_eq!(form(&s), want, "{n}");
        }
    }

    #[test]
    fn factors_pick_one_of_each_sign() {
        assert_eq!(
            factors(&snap("sumo, two beers")),
            [Some(Factor::Heavy), Some(Factor::Tipsy)]
        );
        assert_eq!(
            factors(&snap("hung over")),
            [Some(Factor::HungOver), Some(Factor::Thirsty)]
        );
        let rested = derive(&Body {
            strain: 50,
            ..build("well rested")
        });
        assert_eq!(factors(&rested), [Some(Factor::Rested), Some(Factor::Sore)]);
        assert_eq!(factors(&snap("average")), [None, None]);
    }

    #[test]
    fn decisive_names_the_losers_problem() {
        let (wasted, fit) = (snap("wasted"), snap("fit, normal weight"));
        let k = (0..100u32)
            .find(|&k| !fight(k, &wasted, &fit).a_won)
            .expect("fit wins somewhere");
        let o = fight(k, &wasted, &fit);
        assert_eq!(
            decisive(&o, &wasted, &fit),
            Some((true, Decisive::Factor(Factor::Sloppy)))
        );
        let fat = snap("fat and unfit");
        assert_eq!(fat.stamina, 1);
        let k = (0..100u32)
            .find(|&k| {
                let o = fight(k, &fat, &fit);
                !o.a_won && o.rounds > 1
            })
            .expect("fit wins a long fight");
        let o = fight(k, &fat, &fit);
        assert_eq!(
            decisive(&o, &fat, &fit),
            Some((true, Decisive::OutOfSteam(2)))
        );
    }

    #[test]
    fn factor_words_fit_half_a_line() {
        for f in Factor::ALL {
            assert!(1 + f.text().len() <= 12, "{}", f.text());
        }
    }

    #[test]
    fn form_words_fit_the_row() {
        for f in [Form::Top, Form::In, Form::Off, Form::Poor] {
            assert!("Battle: ".len() + f.text().len() <= 17, "{}", f.text());
        }
    }

    #[test]
    fn form_keys_are_validated_as_a_triple() {
        use bornhack_aegg::game::engine::thresholds::{Thresholds, validate};
        let mut t = Thresholds::CLASSIC;
        t.BATTLE_FORM_OFF_PCT = 60;
        t.BATTLE_FORM_IN_PCT = 50;
        let v = validate(&t, &Thresholds::CLASSIC);
        assert_eq!(
            (
                v.BATTLE_FORM_TOP_PCT,
                v.BATTLE_FORM_IN_PCT,
                v.BATTLE_FORM_OFF_PCT
            ),
            (65, 50, 30)
        );
    }
}

mod rules {
    use bornhack_aegg::game::engine::causes::{Cause, StrainWhy};
    use bornhack_aegg::game::engine::doses::{Reason, Refusal};
    use bornhack_aegg::game::engine::needs::P;
    use bornhack_aegg::game::engine::thresholds as t;
    use bornhack_aegg::game::engine::{Fought, GameState, PetKind, Phase};

    fn pct(v: i32) -> u16 {
        (v * P) as u16
    }

    /// A fit adult: nothing hungry, tired, thirsty or rested, no strain.
    fn fit() -> GameState {
        let mut s = GameState::new_egg(7, PetKind(0));
        s.update(t::HATCHING_TICKS() as u32);
        assert_eq!(s.phase, Phase::Active);
        s.is_sleeping = false;
        s.tick_rem = 0;
        s.money_enabled = false;
        s.energy = pct(80);
        s.fullness = pct(60);
        s.hydration = pct(60);
        s.fun = pct(50);
        s.dirt = pct(20);
        s.mood = pct(50);
        s.health = pct(80);
        s.weight = pct(50);
        s.fitness = pct(50);
        s.strain = 0;
        s.booze = 0;
        s
    }

    fn won(hp: u8) -> Fought {
        Fought {
            won: true,
            hp_left_pct: hp,
            costs: true,
            paid: false,
        }
    }

    #[test]
    fn refusals_follow_the_list() {
        assert_eq!(fit().battle_refusal(), None);

        let mut s = fit();
        s.hibernating = true;
        assert_eq!(s.battle_refusal(), Some(Reason::Hibernating));

        let mut s = fit();
        s.is_sleeping = true;
        assert_eq!(s.battle_refusal(), Some(Reason::Refused(Refusal::Asleep)));

        let mut s = fit();
        s.injured_min = 30;
        assert_eq!(s.battle_refusal(), Some(Reason::Refused(Refusal::Injured)));

        let mut s = fit();
        s.health = pct(29);
        assert_eq!(s.battle_refusal(), Some(Reason::Refused(Refusal::TooWeak)));
        s.health = pct(30);
        assert_eq!(s.battle_refusal(), None);

        // Felt drink: booze * (150 - weight) / 100.
        let mut s = fit();
        s.booze = pct(60);
        s.weight = pct(50);
        assert_eq!(s.battle_refusal(), None);
        s.booze = pct(50);
        s.weight = 0;
        assert_eq!(s.battle_refusal(), Some(Reason::Refused(Refusal::TooDrunk)));

        // No age limit: a pet just past hatching fights.
        let mut s = fit();
        s.age_ticks = 60;
        assert_eq!(s.battle_refusal(), None);

        let mut s = fit();
        s.cooldown_battle = 6;
        assert_eq!(s.battle_refusal(), Some(Reason::Wait(60)));
    }

    #[test]
    fn refusals_have_a_fixed_order() {
        // Asleep and injured: asleep comes first for the challenger ...
        let mut s = fit();
        s.is_sleeping = true;
        s.injured_min = 30;
        s.health = pct(10);
        s.booze = pct(90);
        s.cooldown_battle = 6;
        assert_eq!(s.battle_refusal(), Some(Reason::Refused(Refusal::Asleep)));
        // ... but the body check alone names the injury first.
        assert_eq!(s.body_refusal(), Some(Reason::Refused(Refusal::Injured)));
        s.injured_min = 0;
        assert_eq!(s.body_refusal(), Some(Reason::Refused(Refusal::TooWeak)));
        s.health = pct(80);
        assert_eq!(s.body_refusal(), Some(Reason::Refused(Refusal::TooDrunk)));
        s.booze = 0;
        assert_eq!(s.body_refusal(), None);
        // The cooldown is last.
        s.is_sleeping = false;
        assert_eq!(s.battle_refusal(), Some(Reason::Wait(60)));
    }

    #[test]
    fn injured_pet_cannot_battle() {
        let mut s = fit();
        assert!(s.stats(s.last_update_tick).can_battle);
        s.injured_min = 10;
        assert!(!s.stats(s.last_update_tick).can_battle);
    }

    #[test]
    fn a_fight_costs_what_the_spec_says() {
        let mut s = fit();
        let before = s.clone();
        assert_eq!(s.fight_over(won(80)), 0);
        assert_eq!(s.energy as i32, before.energy as i32 - 10 * P);
        assert_eq!(s.hydration as i32, before.hydration as i32 - 8 * P);
        assert_eq!(s.fun as i32, before.fun as i32 + 20 * P);
        assert_eq!(s.dirt as i32, before.dirt as i32 + 8 * P);
        assert_eq!(s.strain as i32, 20 * P);
        assert_eq!(s.mood as i32, before.mood as i32 + 8 * P);
        // (100 - 80) * 5 / 100 = 1 %
        assert_eq!(s.health as i32, before.health as i32 - P);
        assert_eq!(s.cooldown_battle, 180);
        assert_eq!(s.wins, 1);
        assert_eq!(s.losses, 0);

        let mut s = fit();
        s.fight_over(Fought {
            won: false,
            hp_left_pct: 0,
            costs: true,
            paid: false,
        });
        assert_eq!(s.mood as i32, 42 * P);
        assert_eq!(s.health as i32, 75 * P);
        assert_eq!((s.wins, s.losses), (0, 1));
    }

    #[test]
    fn a_tired_fighter_strains_more_and_can_tear() {
        let mut s = fit();
        s.fullness = pct(5);
        s.fight_over(won(100));
        assert_eq!(s.strain as i32, 30 * P);
        assert_eq!(s.injured_min, 0);

        let mut s = fit();
        s.fullness = pct(5);
        s.strain = pct(45);
        s.fight_over(won(100));
        assert_eq!(s.injured_min, t::DOSE_INJURY_MIN());
        let causes: Vec<Cause> = s.take_causes().iter().copied().collect();
        assert_eq!(causes, vec![Cause::TornMuscle(StrainWhy::Fight)]);

        let mut s = fit();
        s.rested_min = 60;
        s.fight_over(won(100));
        assert_eq!(s.strain as i32, 14 * P);
    }

    #[test]
    fn a_sore_fighter_is_told_so() {
        let mut s = fit();
        s.strain = pct(30);
        s.fight_over(won(100));
        let causes: Vec<Cause> = s.take_causes().iter().copied().collect();
        assert_eq!(causes, vec![Cause::Strained(StrainWhy::Fight)]);
        // Below the sore line nothing is said.
        let mut s = fit();
        s.fight_over(won(100));
        assert!(s.take_causes().is_empty());
    }

    #[test]
    fn sport_injury_is_unchanged() {
        let mut s = fit();
        s.strain = pct(55);
        let health = s.health;
        assert!(s.exercise());
        s.update(s.last_update_tick + t::EXERCISE_DURATION() as u32);
        assert_eq!(s.injured_min, t::DOSE_INJURY_MIN());
        assert_eq!(
            s.health as i32,
            health as i32 - t::DOSE_INJURY_HEALTH_PCT() as i32 * P
        );
        let causes: Vec<Cause> = s.take_causes().iter().copied().collect();
        assert_eq!(causes, vec![Cause::TornMuscle(StrainWhy::Overdone)]);
    }

    #[test]
    fn first_win_of_the_day_pays_100_then_20() {
        let mut s = fit();
        s.money_enabled = true;
        s.money = 0;
        s.sync_clock(s.last_update_tick, Some(20 * 60));
        let paid = Fought { paid: true, ..won(100) };
        assert_eq!(s.fight_over(paid), 100);
        assert_eq!(s.prize_min, 240);
        assert_eq!(s.fight_over(paid), 20);
        assert_eq!(s.money, 120);
        assert_eq!(s.prize_min, 240);
        // The clock runs the prize timer down; at midnight the first prize is back.
        s.update(s.last_update_tick + 240 * 6);
        assert_eq!(s.prize_min, 0);
        assert_eq!(s.fight_over(paid), 100);

        // No wall clock: a day.
        let mut s = fit();
        s.money_enabled = true;
        assert_eq!(s.fight_over(paid), 100);
        assert_eq!(s.prize_min, 1440);

        // A loss pays nothing and leaves the timer.
        let mut s = fit();
        s.money_enabled = true;
        let lost = Fought { won: false, ..paid };
        assert_eq!(s.fight_over(lost), 0);
        assert_eq!(s.prize_min, 0);
        assert_eq!(s.money, 100);
    }

    #[test]
    fn fallback_target_is_never_paid() {
        let mut s = fit();
        s.money_enabled = true;
        s.money = 50;
        let (energy, mood) = (s.energy, s.mood);
        s.record_only(true);
        assert_eq!(s.money, 50);
        assert_eq!(s.wins, 1);
        assert_eq!((s.energy, s.mood, s.cooldown_battle), (energy, mood, 0));
        s.record_only(false);
        assert_eq!((s.wins, s.losses), (1, 1));
        assert_eq!(s.prize_min, 0);
    }

    #[test]
    fn unpaid_fight_pays_nothing() {
        let mut s = fit();
        s.money_enabled = true;
        s.money = 50;
        assert_eq!(s.fight_over(won(90)), 0);
        assert_eq!(s.money, 50);
        assert_eq!(s.prize_min, 0);
        assert_eq!(s.cooldown_battle, 180);
        assert!(s.strain > 0);
    }

    #[test]
    fn a_fight_without_costs_still_rests_the_pet() {
        let mut s = fit();
        let (energy, mood) = (s.energy, s.mood);
        let f = Fought {
            costs: false,
            ..won(100)
        };
        s.fight_over(f);
        assert_eq!((s.energy, s.mood, s.strain), (energy, mood, 0));
        assert_eq!(s.cooldown_battle, 180);
        assert_eq!(s.wins, 1);
    }

    #[test]
    fn demo_fights_have_their_own_record() {
        let mut s = fit();
        s.money_enabled = true;
        s.demo.on = true;
        s.health = pct(26);
        let paid = Fought { paid: true, ..won(0) };
        s.money = 0;
        // HAX stays live in demo: a paid win pays like any other.
        assert_eq!(s.fight_over(paid), 100);
        assert_eq!(s.money, 100);
        assert_eq!((s.demo_wins, s.demo_losses), (1, 0));
        assert_eq!((s.wins, s.losses), (0, 0));
        assert_eq!(s.battle_record(), (1, 0));
        assert_eq!(s.health as i32, 25 * P, "the demo floor holds");
        s.record_only(false);
        assert_eq!(s.battle_record(), (1, 1));
        s.demo.on = false;
        assert_eq!(s.battle_record(), (0, 0));
    }

    #[test]
    fn battle_reasons_fit() {
        for r in [
            Refusal::TooWeak,
            Refusal::TooDrunk,
            Refusal::DemoOnly,
            Refusal::NoDemo,
        ] {
            let why = Reason::Refused(r);
            assert!(why.text().len() <= 16, "{}", why.text());
            let (kind, secs) = why.pack();
            assert_eq!(Reason::unpack(kind, secs), why);
        }
        assert_eq!(Refusal::TooWeak.text(), "too weak");
        assert_eq!(Refusal::TooDrunk.text(), "too drunk");
        assert_eq!(Refusal::DemoOnly.text(), "demo pets only");
        assert_eq!(Refusal::NoDemo.text(), "no demo pets");
    }

    #[test]
    fn fight_word_fits() {
        let c = Cause::TornMuscle(StrainWhy::Fight);
        assert_eq!(c.why(), "in a fight");
        assert!(c.why().len() <= 15 && c.what().len() <= 12);
        assert!(StrainWhy::ALL.contains(&StrainWhy::Fight));
        assert_eq!(Cause::unpack(c.pack()), Some(c));
        let c = Cause::Strained(StrainWhy::Fight);
        assert_eq!(Cause::unpack(c.pack()), Some(c));
    }

    #[test]
    fn battle_keys_load_and_validate() {
        use bornhack_aegg::game::bornpets_cfg::apply_overrides;
        use bornhack_aegg::game::engine::thresholds::{Thresholds, validate};
        let mut v = Thresholds::CLASSIC;
        apply_overrides(&mut v, b"BATTLE_STRAIN_NEED_PCT=150\nBATTLE_STRAIN_RESTED_PCT=70\n");
        assert_eq!((v.BATTLE_STRAIN_NEED_PCT, v.BATTLE_STRAIN_RESTED_PCT), (150, 70));
        assert_eq!(Thresholds::CASUAL.BATTLE_BRUISE_MAX_PCT, 2);
        assert_eq!(Thresholds::CLASSIC.BATTLE_BRUISE_MAX_PCT, 5);
        let mut bad = Thresholds::CLASSIC;
        bad.BATTLE_PRIZE_LATER_HAX = 101;
        bad.BATTLE_COOLDOWN = 0;
        let ok = validate(&bad, &Thresholds::CLASSIC);
        assert_eq!(
            (ok.BATTLE_PRIZE_FIRST_HAX, ok.BATTLE_PRIZE_LATER_HAX),
            (100, 20)
        );
        assert_eq!(ok.BATTLE_COOLDOWN, 1);
    }
}

mod save {
    use bornhack_aegg::game::engine::{GameState, PetKind, SAVE_SIZE};

    fn state() -> GameState {
        let mut s = GameState::new_egg(7, PetKind(0));
        s.demo_wins = 0x1234;
        s.demo_losses = 0x0567;
        s.prize_min = 777;
        s.decline_min = 59;
        s
    }

    #[test]
    fn battle_fields_round_trip() {
        let s = state();
        let b = s.to_bytes();
        assert_eq!(&b[38..40], &0x1234u16.to_le_bytes());
        assert_eq!(&b[42..44], &0x0567u16.to_le_bytes());
        assert_eq!(&b[44..46], &777u16.to_le_bytes());
        assert_eq!(&b[46..48], &59u16.to_le_bytes());
        let back = GameState::from_bytes(&b).expect("decodes");
        assert_eq!(
            (back.demo_wins, back.demo_losses, back.prize_min, back.decline_min),
            (0x1234, 0x0567, 777, 59)
        );
        assert_eq!(back.to_bytes(), b);
    }

    #[test]
    fn plan3_save_loads_with_empty_battle_slots() {
        let mut b = state().to_bytes();
        // 40..42 is med_cd_min: it stays.
        b[38..40].fill(0);
        b[42..48].fill(0);
        let s = GameState::from_bytes(&b).expect("decodes");
        assert_eq!((s.demo_wins, s.demo_losses, s.prize_min, s.decline_min), (0, 0, 0, 0));
    }

    #[test]
    fn tail_stays_zero_for_plan_6() {
        let mut s = state();
        s.demo_wins = u16::MAX;
        s.demo_losses = u16::MAX;
        s.prize_min = u16::MAX;
        s.decline_min = u16::MAX;
        s.wins = u16::MAX;
        s.losses = u16::MAX;
        let b = s.to_bytes();
        assert_eq!(b.len(), SAVE_SIZE);
        assert_eq!(SAVE_SIZE, 177);
        assert_eq!(b[0], 3);
        assert!(b[160..177].iter().all(|&x| x == 0));
    }
}

mod decline {
    use bornhack_aegg::game::engine::needs::P;
    use bornhack_aegg::game::engine::regime::mood_target;
    use bornhack_aegg::game::engine::status::status_line;
    use bornhack_aegg::game::engine::thresholds as t;
    use bornhack_aegg::game::engine::{GameState, PetKind, Phase};

    fn pct(v: i32) -> u16 {
        (v * P) as u16
    }

    /// A content adult with the mood on its target.
    fn content() -> GameState {
        let mut s = GameState::new_egg(7, PetKind(0));
        s.update(t::HATCHING_TICKS() as u32);
        assert_eq!(s.phase, Phase::Active);
        s.is_sleeping = false;
        s.tick_rem = 0;
        s.money_enabled = false;
        s.energy = pct(80);
        s.fullness = pct(60);
        s.hydration = pct(60);
        s.fun = pct(50);
        s.dirt = pct(20);
        s.health = pct(80);
        s.weight = pct(50);
        s.fitness = pct(50);
        s.strain = 0;
        s.booze = 0;
        s.mood = mood_target(&s) as u16;
        s
    }

    fn rng(seed: u64) -> impl FnMut(u64) -> u64 {
        let mut r = seed;
        move |bound| {
            r = r
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (r >> 33) % (bound + 1)
        }
    }

    #[test]
    fn decline_lowers_the_mood_target_for_an_hour() {
        let mut s = content();
        let before = mood_target(&s);
        s.declined();
        assert_eq!(s.decline_min, t::BATTLE_DECLINE_MIN());
        assert_eq!(
            before - mood_target(&s),
            t::BATTLE_DECLINE_MOOD_PCT() as i32 * P
        );
        // The hour must cross no need floor, or the target moves for other reasons.
        let t0 = s.last_update_tick;
        let mut probe = content();
        probe.update(t0 + t::BATTLE_DECLINE_MIN() as u32 * 6);
        assert_eq!(mood_target(&probe), before, "fixture drifts over the hour");
        s.update(t0 + t::BATTLE_DECLINE_MIN() as u32 * 6);
        assert_eq!(s.decline_min, 0);
        assert_eq!(mood_target(&s), before);
    }

    #[test]
    fn decline_end_is_exact() {
        // (a) one update across the minute decline_min reaches 0 equals the
        // same span cut at that minute.
        let mut s = content();
        s.declined();
        s.decline_min = 5;
        let t0 = s.last_update_tick;
        let end = t0 + 5 * 6;
        let mut whole = s.clone();
        whole.update(end + 60);
        let mut cut = s.clone();
        cut.update(end);
        assert_eq!(cut.decline_min, 0);
        cut.update(end + 60);
        assert_eq!(whole.to_bytes(), cut.to_bytes());
        // One minute earlier decline_min is 1: a bound at `remaining` or none
        // at all splits the span differently and the bytes differ.
        let mut early = s.clone();
        early.update(end - 6);
        assert_eq!(early.decline_min, 1);
        early.update(end + 60);
        assert_eq!(whole.to_bytes(), early.to_bytes());
        // (b) the target term is exactly 3 * P at decline_min = 1.
        let mut a = content();
        a.decline_min = 1;
        let mut b = a.clone();
        b.decline_min = 0;
        assert_eq!(
            mood_target(&b) - mood_target(&a),
            t::BATTLE_DECLINE_MOOD_PCT() as i32 * P
        );
    }

    fn obs(s: &GameState) -> String {
        format!("{:?}", s.wake_view_for_test())
    }

    #[test]
    fn decline_wake_is_exact_on_random_states() {
        let mut next = rng(11);
        for i in 0..1000 {
            let mut s = content();
            s.fullness = pct(next(100) as i32);
            s.hydration = pct(next(100) as i32);
            s.fun = pct(next(100) as i32);
            s.mood = pct(next(100) as i32);
            s.decline_min = next(60) as u16;
            let t0 = s.last_update_tick;
            let o = obs(&s);
            let mut c = s.clone();
            let first = (1..=120u32).find(|m| {
                c.update(t0 + m * 6);
                obs(&c) != o
            });
            let expect = first.map_or(t0 + 120 * 6, |m| t0 + m * 6);
            assert_eq!(s.next_wake_tick(), expect, "random state {i}");
        }
    }

    #[test]
    fn decline_is_split_invariant() {
        let mut next = rng(5);
        for i in 0..200 {
            let mut s = content();
            s.mood = pct(next(100) as i32);
            s.fun = pct(next(100) as i32);
            s.decline_min = next(60) as u16;
            let t0 = s.last_update_tick;
            let span = 3 * 360u32;
            let mut whole = s.clone();
            whole.update(t0 + span);
            let mut cuts: Vec<u32> = (0..30).map(|_| t0 + next(span as u64) as u32).collect();
            cuts.sort_unstable();
            let mut cut = s.clone();
            for c in cuts {
                cut.update(c);
            }
            cut.update(t0 + span);
            assert_eq!(whole.to_bytes(), cut.to_bytes(), "seed {i}");
        }
    }
}

mod wire {
    use bornhack_aegg::game::battle::{
        AcceptMsg, ChallengeMsg, DeclineMsg, PET_ACCEPT_TYPE, PET_BATTLE_TYPE, PET_CHALLENGE_TYPE,
        PET_DECLINE_TYPE, WHY_DECLINED,
    };
    use bornhack_aegg::game::engine::combat::{
        Flags, SNAPSHOT_LEN, Snapshot, WIRE_FOCUS, WIRE_GUARD, WIRE_HP, WIRE_POWER, WIRE_SPEED,
        WIRE_STAMINA,
    };
    use bornhack_aegg::game::friends::PET_BEACON_TYPE;

    fn lcg(s: &mut u32) -> u32 {
        *s = s.wrapping_mul(1664525).wrapping_add(1013904223);
        *s >> 8
    }

    fn pick(s: &mut u32, r: (u8, u8)) -> u8 {
        r.0 + (lcg(s) % (r.1 - r.0 + 1) as u32) as u8
    }

    fn good() -> Snapshot {
        Snapshot {
            power: 40,
            guard: 20,
            speed: 50,
            hp: 80,
            stamina: 6,
            focus: -10,
            flags: Flags(0b101).with_demo(true),
        }
    }

    #[test]
    fn snapshot_wire_round_trips_every_field() {
        let mut s = 7u32;
        for i in 0..2000 {
            let focus = match i {
                0 => WIRE_FOCUS.0,
                1 => WIRE_FOCUS.1,
                _ => (WIRE_FOCUS.0 as i32 + (lcg(&mut s) % 121) as i32) as i8,
            };
            // Condition bits 0..=19 and the demo bit 23.
            let flags = Flags(lcg(&mut s) & 0xF_FFFF | (lcg(&mut s) & 1) << 23);
            let snap = Snapshot {
                power: pick(&mut s, WIRE_POWER),
                guard: pick(&mut s, WIRE_GUARD),
                speed: pick(&mut s, WIRE_SPEED),
                hp: pick(&mut s, WIRE_HP),
                stamina: pick(&mut s, WIRE_STAMINA),
                focus,
                flags,
            };
            let b = snap.to_wire();
            assert_eq!(b.len(), SNAPSHOT_LEN);
            assert_eq!(Snapshot::from_wire(&b), Some(snap), "case {i}");
        }
    }

    #[test]
    fn out_of_range_snapshot_is_refused() {
        assert_eq!(Snapshot::from_wire(&good().to_wire()), Some(good()));
        // One field at a time, just past each end (and 0 / 255 where they are
        // the dangerous values: fight divides by hp, decisive adds 1 to stamina).
        let mut bad: Vec<[u8; SNAPSHOT_LEN]> = Vec::new();
        for (idx, vals) in [
            (0usize, vec![WIRE_POWER.0 - 1, WIRE_POWER.1 + 1, 0, 255]),
            (1, vec![WIRE_GUARD.1 + 1, 255]),
            (2, vec![WIRE_SPEED.0 - 1, WIRE_SPEED.1 + 1, 0, 255]),
            (3, vec![0, WIRE_HP.0 - 1, WIRE_HP.1 + 1, 255]),
            (4, vec![WIRE_STAMINA.1 + 1, 255]),
            (5, vec![(WIRE_FOCUS.0 - 1) as u8, (WIRE_FOCUS.1 + 1) as u8, 127, 128]),
        ] {
            for v in vals {
                let mut b = good().to_wire();
                b[idx] = v;
                bad.push(b);
            }
        }
        for bit in [20u32, 21, 22] {
            let mut b = good().to_wire();
            let f = u32::from_le_bytes([b[6], b[7], b[8], 0]) | 1 << bit;
            b[6..9].copy_from_slice(&f.to_le_bytes()[..3]);
            bad.push(b);
        }
        for b in &bad {
            assert_eq!(Snapshot::from_wire(b), None, "{b:?}");
        }
        // Too short.
        assert_eq!(Snapshot::from_wire(&good().to_wire()[..SNAPSHOT_LEN - 1]), None);
        // Every u8 in every number slot: accepted only inside its range.
        let ranges = [WIRE_POWER, WIRE_GUARD, WIRE_SPEED, WIRE_HP, WIRE_STAMINA];
        for (idx, r) in ranges.iter().enumerate() {
            for v in 0..=255u8 {
                let mut b = good().to_wire();
                b[idx] = v;
                assert_eq!(
                    Snapshot::from_wire(&b).is_some(),
                    v >= r.0 && v <= r.1,
                    "slot {idx} value {v}"
                );
            }
        }
    }

    #[test]
    fn beacon_numbers_are_clamped() {
        let s = Snapshot::from_beacon(100, 100, 100, 150, 3, 0, false);
        assert_eq!((s.power, s.guard, s.speed, s.hp), (70, 40, 100, 120));
        assert_eq!((s.stamina, s.focus), (3, 0));
        assert!(!s.flags.demo());
        assert_eq!(s.flags.0 & 0xF_FFFF, 0);
        let low = Snapshot::from_beacon(0, 0, 0, 0, 255, -128, true);
        assert_eq!((low.power, low.guard, low.speed, low.hp), (5, 0, 5, 15));
        assert_eq!((low.stamina, low.focus), (10, -100));
        assert!(low.flags.demo());
        assert!(Snapshot::from_wire(&low.to_wire()).is_some());
    }

    #[test]
    fn packets_round_trip() {
        let mut name = [0u8; 12];
        name[..4].copy_from_slice(b"Bram");
        let ch = ChallengeMsg {
            challenger_id: [1, 2],
            target_id: [3, 4],
            snap: good(),
            nonce: 0xBEEF,
            kind: 5,
            name,
            name_len: 4,
        };
        let b = ch.to_bytes();
        assert_eq!(b.len(), 29);
        assert_eq!(ChallengeMsg::from_bytes(&b), Some(ch));
        assert_eq!(ChallengeMsg::from_bytes(&b[..28]), None);

        let ac = AcceptMsg {
            challenger_id: [1, 2],
            target_id: [3, 4],
            snap: good(),
            nonce: 0x1234,
        };
        let b = ac.to_bytes();
        assert_eq!(b.len(), 15);
        assert_eq!(AcceptMsg::from_bytes(&b), Some(ac));
        assert_eq!(AcceptMsg::from_bytes(&b[..14]), None);

        let de = DeclineMsg {
            challenger_id: [1, 2],
            target_id: [3, 4],
            why: 6,
            secs: 0x0102,
        };
        let b = de.to_bytes();
        assert_eq!(b.len(), 7);
        assert_eq!(&b[5..7], &[0x02, 0x01]);
        assert_eq!(DeclineMsg::from_bytes(&b), Some(de));
        assert_eq!(DeclineMsg::from_bytes(&b[..6]), None);
        assert_eq!(WHY_DECLINED, 0xFE);

        // Longer is tolerated.
        let mut long = ac.to_bytes().to_vec();
        long.push(0);
        assert_eq!(AcceptMsg::from_bytes(&long), Some(ac));
    }

    #[test]
    fn invalid_snapshot_in_a_packet_is_refused() {
        let mut ac = AcceptMsg {
            challenger_id: [1, 2],
            target_id: [3, 4],
            snap: good(),
            nonce: 1,
        }
        .to_bytes();
        ac[4 + 3] = 0; // hp 0
        assert_eq!(AcceptMsg::from_bytes(&ac), None);
        let mut ch = ChallengeMsg {
            challenger_id: [1, 2],
            target_id: [3, 4],
            snap: good(),
            nonce: 1,
            kind: 0,
            name: [0; 12],
            name_len: 0,
        }
        .to_bytes();
        ch[4 + 4] = 255; // stamina 255
        assert_eq!(ChallengeMsg::from_bytes(&ch), None);
    }

    #[test]
    fn packet_types_are_distinct() {
        let t = [
            PET_BEACON_TYPE,
            PET_BATTLE_TYPE,
            PET_CHALLENGE_TYPE,
            PET_ACCEPT_TYPE,
            PET_DECLINE_TYPE,
        ];
        for i in 0..t.len() {
            for j in i + 1..t.len() {
                assert_ne!(t[i], t[j]);
            }
        }
    }
}

mod protocol {
    use bornhack_aegg::game::Toast;
    use bornhack_aegg::game::battle::{
        ANSWER_WINDOW_MS, AcceptMsg, BattleResultMsg, ChallengeMsg, DEMO_ACCEPT_MS, DeclineMsg,
        Fight, LATE_RESULT_MS, Me, Note, Out, Outs, Packet, Session, TARGET_WINDOW_MS,
        WHY_DECLINED,
    };
    use bornhack_aegg::game::engine::combat::win_chance;
    use bornhack_aegg::game::engine::doses::{Reason, Refusal};
    use bornhack_aegg::game::engine::needs::P;
    use bornhack_aegg::game::engine::thresholds as t;
    use bornhack_aegg::game::engine::{GameState, PetKind, Phase};
    use bornhack_aegg::game::friends::{self, FriendRecord};

    const TICK: u32 = 10_000;
    const MS: u32 = 1_000_000;

    fn pct(v: i32) -> u16 {
        (v * P) as u16
    }

    /// A fit adult, as `rules::fit` builds it; `seed` varies the pet.
    fn adult(seed: u64, money: bool) -> GameState {
        let mut s = GameState::new_egg(seed, PetKind(0));
        s.update(t::HATCHING_TICKS() as u32);
        assert_eq!(s.phase, Phase::Active);
        s.is_sleeping = false;
        s.tick_rem = 0;
        s.money_enabled = money;
        s.money = if money { 50 } else { 0 };
        s.energy = pct(80);
        s.fullness = pct(60);
        s.hydration = pct(60);
        s.fun = pct(50);
        s.dirt = pct(20);
        s.mood = pct(50);
        s.health = pct(80);
        s.weight = pct(50);
        s.fitness = pct(50);
        s.strain = 0;
        s.booze = 0;
        s
    }

    fn me<'a>(id: [u8; 2], name: &'a str, now_ms: u32, now_tick: u32, nonce: u16) -> Me<'a> {
        Me {
            id,
            kind: 3,
            name,
            now_ms,
            now_tick,
            nonce,
        }
    }

    /// The friend record `owner` would hold of `s`, last heard `ago` ticks ago.
    fn friend_of(s: &GameState, id: [u8; 2], ago: u32) -> FriendRecord {
        let n = s.snapshot();
        FriendRecord {
            device_id: id,
            pet_kind: 1,
            name: *b"Bee\0\0\0\0\0\0\0\0\0",
            name_len: 3,
            first_seen_tick: 1,
            last_boost_tick: 1,
            last_seen_tick: TICK - ago,
            power: n.power,
            guard: n.guard,
            speed: n.speed,
            hp: n.hp,
            stamina: n.stamina,
            focus: n.focus,
            demo: s.demo.on,
            v3: true,
            fought_tick: 0,
            wins: 0,
            losses: 0,
            heard_this_boot: true,
        }
    }

    struct Rig {
        a: GameState,
        b: GameState,
        sa: Session,
        sb: Session,
        ida: [u8; 2],
        idb: [u8; 2],
    }

    impl Rig {
        fn new() -> Rig {
            Rig::with_ids([1, 0], [2, 0], false)
        }
        fn with_ids(ida: [u8; 2], idb: [u8; 2], money: bool) -> Rig {
            Rig {
                a: adult(11, money),
                b: adult(12, money),
                sa: Session::new(),
                sb: Session::new(),
                ida,
                idb,
            }
        }
        fn ma(&self, ms: u32, nonce: u16) -> Me<'static> {
            me(self.ida, "Ant", ms, TICK, nonce)
        }
        fn mb(&self, ms: u32, nonce: u16) -> Me<'static> {
            me(self.idb, "Bee", ms, TICK, nonce)
        }
        fn fb(&self) -> FriendRecord {
            friend_of(&self.b, self.idb, 0)
        }
        /// A challenges B at `MS`; returns the CHALLENGE.
        fn challenge(&mut self) -> ChallengeMsg {
            let fb = self.fb();
            let m = self.ma(MS, 0x1111);
            let outs = self.sa.challenge(&m, &mut self.a, &fb).expect("challenge");
            match one_send(&outs) {
                Packet::Challenge(c) => c,
                p => panic!("not a challenge: {p:?}"),
            }
        }
    }

    fn one_send(o: &Outs) -> Packet {
        let v: Vec<Packet> = sends(o);
        assert_eq!(v.len(), 1, "{o:?}");
        v[0]
    }

    /// Every packet crosses the wire: encoded, decoded, the decode must work.
    fn wire(p: Packet) -> Packet {
        match p {
            Packet::Challenge(m) => {
                Packet::Challenge(ChallengeMsg::from_bytes(&m.to_bytes()).expect("challenge"))
            }
            Packet::Accept(m) => {
                Packet::Accept(AcceptMsg::from_bytes(&m.to_bytes()).expect("accept"))
            }
            Packet::Decline(m) => {
                Packet::Decline(DeclineMsg::from_bytes(&m.to_bytes()).expect("decline"))
            }
            Packet::Result(m) => {
                Packet::Result(BattleResultMsg::from_bytes(&m.to_bytes()).expect("result"))
            }
        }
    }

    fn sends(o: &Outs) -> Vec<Packet> {
        o.iter()
            .filter_map(|x| {
                if let Out::Send(p) = x {
                    Some(wire(*p))
                } else {
                    None
                }
            })
            .collect()
    }

    fn has_buzz(o: &Outs) -> bool {
        o.iter().any(|x| matches!(x, Out::Buzz))
    }

    fn has_show(o: &Outs) -> bool {
        o.iter().any(|x| matches!(x, Out::Show))
    }

    fn fought(o: &Outs) -> Option<Fight> {
        o.iter().find_map(|x| {
            if let Out::Fought(f) = x {
                Some(*f)
            } else {
                None
            }
        })
    }

    fn recorded(o: &Outs) -> Option<([u8; 2], bool)> {
        o.iter().find_map(|x| {
            if let Out::Recorded { peer, won } = x {
                Some((*peer, *won))
            } else {
                None
            }
        })
    }

    fn toast(o: &Outs) -> Option<Toast> {
        o.iter().find_map(|x| {
            if let Out::Toast(t) = x {
                Some(*t)
            } else {
                None
            }
        })
    }

    fn accept_of(o: &Outs) -> AcceptMsg {
        match one_send(o) {
            Packet::Accept(a) => a,
            p => panic!("not an accept: {p:?}"),
        }
    }

    fn decline_of(o: &Outs) -> DeclineMsg {
        match one_send(o) {
            Packet::Decline(d) => d,
            p => panic!("not a decline: {p:?}"),
        }
    }

    /// A challenges, B accepts at once; returns (A's fight, B's fight).
    fn live(r: &mut Rig) -> (Fight, Fight) {
        let c = r.challenge();
        let (mb, mb2) = (r.mb(MS + 1000, 0x2222), r.mb(MS + 2000, 0x2222));
        let outs = r.sb.on_challenge(&mb, &mut r.b, &c, None);
        assert!(has_show(&outs));
        let outs = r.sb.answer(&mb2, &mut r.b, true);
        let acc = accept_of(&outs);
        let fb = fought(&outs).expect("B fought");
        let ma = r.ma(MS + 3000, 0);
        let outs = r.sa.on_accept(&ma, &mut r.a, &acc);
        (fought(&outs).expect("A fought"), fb)
    }

    #[test]
    fn live_fight_agrees_on_both_badges() {
        let mut r = Rig::new();
        let (fa, fb) = live(&mut r);
        assert_eq!(fa.won, !fb.won);
        assert_eq!(fa.my_hp, fb.their_hp);
        assert_eq!(fa.their_hp, fb.my_hp);
        assert!(fa.live && fb.live);
        assert_eq!(fa.peer_id, [2, 0]);
        assert_eq!(fb.peer_id, [1, 0]);
    }

    #[test]
    fn chances_add_up_to_100() {
        let mut r = Rig::new();
        let c = r.challenge();
        let mb = r.mb(MS + 1000, 0x2222);
        r.sb.on_challenge(&mb, &mut r.b, &c, None);
        let b_chance = r.sb.view().expect("B screen").chance;
        let outs = r.sb.answer(&mb, &mut r.b, true);
        let acc = accept_of(&outs);
        let ma = r.ma(MS + 3000, 0);
        let fa = fought(&r.sa.on_accept(&ma, &mut r.a, &acc)).unwrap();
        assert_eq!(b_chance as u32 + fa.chance as u32, 100);
    }

    #[test]
    fn accept_shows_the_exact_chance_before_the_fight() {
        let mut r = Rig::new();
        let c = r.challenge();
        let est = r.sa.view().unwrap();
        assert!(est.estimate);
        let mb = r.mb(MS + 1000, 0x2222);
        r.sb.on_challenge(&mb, &mut r.b, &c, None);
        let acc = accept_of(&r.sb.answer(&mb, &mut r.b, true));
        let ma = r.ma(MS + 3000, 0);
        let outs = r.sa.on_accept(&ma, &mut r.a, &acc);
        let show = outs
            .iter()
            .position(|x| matches!(x, Out::Show))
            .expect("Show");
        let fight = outs
            .iter()
            .position(|x| matches!(x, Out::Fought(_)))
            .expect("Fought");
        assert!(show < fight);
        let v = r.sa.view().expect("exact screen kept for the glue");
        assert!(!v.estimate);
        assert_eq!(v.chance, fought(&outs).unwrap().chance);
        assert_eq!(v.chance, win_chance(&v.mine, &v.theirs));
    }

    #[test]
    fn both_pay_costs_in_a_live_fight() {
        let mut r = Rig::new();
        let (ea, eb) = (r.a.energy, r.b.energy);
        live(&mut r);
        assert_eq!(r.a.energy as i32, ea as i32 - 10 * P);
        assert_eq!(r.b.energy as i32, eb as i32 - 10 * P);
        assert_eq!(r.a.cooldown_battle, 180);
        assert_eq!(r.b.cooldown_battle, 180);
    }

    #[test]
    fn exactly_one_side_is_paid() {
        let mut r = Rig::with_ids([1, 0], [2, 0], true);
        let (fa, fb) = live(&mut r);
        let prize = fa.prize + fb.prize;
        assert!(prize > 0);
        assert_eq!(r.a.money + r.b.money, 100 + prize);
        let (winner, loser) = if fa.won { (&r.a, &r.b) } else { (&r.b, &r.a) };
        assert_eq!(loser.money, 50);
        assert_eq!(winner.money, 50 + prize);
    }

    /// A challenges B and B never answers: A's fallback fight, with `nonce`.
    fn fallback(a: &GameState, ago: u32, nonce: u16) -> (Outs, GameState, Session) {
        let mut a = a.clone();
        let mut sa = Session::new();
        let fb = friend_of(&adult(12, false), [2, 0], ago);
        let m = me([1, 0], "Ant", MS, TICK, nonce);
        sa.challenge(&m, &mut a, &fb).expect("challenge");
        let late = me([1, 0], "Ant", MS + ANSWER_WINDOW_MS, TICK, nonce);
        let outs = sa.on_deadline(&late, &mut a, Some(&fb));
        (outs, a, sa)
    }

    #[test]
    fn fallback_when_no_answer() {
        let mut r = Rig::new();
        let fb = r.fb();
        let m = r.ma(MS, 0x1111);
        r.sa.challenge(&m, &mut r.a, &fb).unwrap();
        assert_eq!(r.sa.deadline_ms(), Some(MS + ANSWER_WINDOW_MS));
        let late = r.ma(MS + ANSWER_WINDOW_MS, 0x1111);
        let outs = r.sa.on_deadline(&late, &mut r.a, Some(&fb));
        let fa = fought(&outs).expect("fallback fight");
        assert!(!fa.live);
        assert_eq!(recorded(&outs), Some(([2, 0], fa.won)));
        let Packet::Result(res) = one_send(&outs) else {
            panic!("no RESULT")
        };
        assert_eq!(res.challenger_id, [1, 0]);
        assert_eq!(res.target_id, [2, 0]);
        assert_eq!(res.challenger_won, fa.won);
        assert_eq!(res.to_bytes().len(), 9);
        assert!(r.sa.view().is_none());

        // B hears it later: record only.
        let before = r.b.clone();
        let mb = r.mb(MS + 30_000, 0x2222);
        let outs = r.sb.on_result(&mb, &mut r.b, &res, Some(false));
        assert_eq!(recorded(&outs), Some(([1, 0], !fa.won)));
        assert_eq!(
            toast(&outs),
            Some(if fa.won {
                Toast::BattleLost
            } else {
                Toast::BattleWon
            })
        );
        assert_eq!(r.b.energy, before.energy);
        assert_eq!(r.b.mood, before.mood);
        assert_eq!(r.b.money, before.money);
        assert_eq!(r.b.cooldown_battle, before.cooldown_battle);
        let (w, l) = r.b.battle_record();
        assert_eq!(w + l, 1);
    }

    #[test]
    fn fallback_prize_needs_a_fresh_beacon() {
        let base = adult(11, true);
        let won_with = |ago: u32| -> (Fight, GameState) {
            for nonce in 1..500u16 {
                let (outs, a, _) = fallback(&base, ago, nonce);
                let f = fought(&outs).unwrap();
                if f.won {
                    return (f, a);
                }
            }
            panic!("no winning nonce");
        };
        // 29 minutes: paid, costs land.
        let (f, a) = won_with(29 * 6);
        assert_eq!(f.prize, 100);
        assert_eq!(a.money, 150);
        assert_eq!(a.energy as i32, base.energy as i32 - 10 * P);
        // 31 minutes: a record only (owner answer 1): no prize, no costs,
        // but the rest still starts.
        let (f, a) = won_with(31 * 6);
        assert_eq!(f.prize, 0);
        assert_eq!(a.money, 50);
        assert_eq!(a.energy, base.energy);
        assert_eq!(a.mood, base.mood);
        assert_eq!(a.cooldown_battle, 180);
        assert_eq!(a.battle_record(), (1, 0));
    }

    /// After a reboot every stored beacon looks recent (the ticks are rebased
    /// onto the new uptime), so only a beacon heard this boot is fresh.
    #[test]
    fn fallback_after_reboot_needs_a_beacon_heard_this_boot() {
        let base = adult(11, true);
        for nonce in 1..500u16 {
            let mut a = base.clone();
            let mut sa = Session::new();
            let mut fb = friend_of(&adult(12, false), [2, 0], 5);
            fb.heard_this_boot = false; // loaded from the save, not heard since
            let m = me([1, 0], "Ant", MS, TICK, nonce);
            sa.challenge(&m, &mut a, &fb).expect("challenge");
            let late = me([1, 0], "Ant", MS + ANSWER_WINDOW_MS, TICK, nonce);
            let outs = sa.on_deadline(&late, &mut a, Some(&fb));
            let f = fought(&outs).unwrap();
            if f.won {
                assert_eq!(f.prize, 0);
                assert_eq!(a.money, 50);
                assert_eq!(a.energy, base.energy);
                assert_eq!(a.battle_record(), (1, 0));
                return;
            }
        }
        panic!("no winning nonce");
    }

    #[test]
    fn heard_result_raises_the_toast() {
        let mut r = Rig::new();
        let bytes = [9, 9, 2, 0, 1, 80, 0];
        let m = BattleResultMsg::from_bytes(&bytes).unwrap();
        let mb = r.mb(MS, 1);
        let outs = r.sb.on_result(&mb, &mut r.b, &m, None);
        assert_eq!(toast(&outs), Some(Toast::BattleLost));
        assert_eq!(recorded(&outs), Some(([9, 9], false)));
        assert_eq!(r.b.battle_record(), (0, 1));
    }

    #[test]
    fn decline_costs_the_decliner() {
        let mut r = Rig::new();
        let c = r.challenge();
        let mb = r.mb(MS + 1000, 0x2222);
        r.sb.on_challenge(&mb, &mut r.b, &c, None);
        let outs = r.sb.answer(&mb, &mut r.b, false);
        let d = decline_of(&outs);
        assert_eq!(d.why, WHY_DECLINED);
        assert!(outs.iter().any(|x| matches!(x, Out::Close)));
        assert_eq!(r.b.decline_min, 60);
        assert!(r.sb.view().is_none());
        assert!(recorded(&outs).is_none());
        let ma = r.ma(MS + 4000, 0);
        let outs = r.sa.on_decline(&ma, &d);
        assert_eq!(r.sa.view().unwrap().note, Note::Declined);
        assert!(recorded(&outs).is_none() && fought(&outs).is_none());
        assert_eq!(r.a.decline_min, 0);
        assert_eq!(r.a.battle_record(), (0, 0));
    }

    #[test]
    fn other_league_is_declined_silently() {
        let mut r = Rig::new();
        let fb = r.fb(); // a stale beacon: B was a player pet
        r.b.demo.on = true;
        let m = r.ma(MS, 0x1111);
        let outs = r.sa.challenge(&m, &mut r.a, &fb).unwrap();
        let Packet::Challenge(c) = one_send(&outs) else {
            panic!()
        };
        let mb = r.mb(MS + 1000, 0x2222);
        let before = r.b.to_bytes();
        let outs = r.sb.on_challenge(&mb, &mut r.b, &c, None);
        assert_eq!(outs.len(), 1, "{outs:?}");
        let d = decline_of(&outs);
        assert_eq!(d.reason(), Some(Reason::Refused(Refusal::NoDemo)));
        assert!(r.sb.view().is_none());
        assert_eq!(r.b.to_bytes(), before);
        r.sa.on_decline(&m, &d);
        assert_eq!(
            r.sa.view().unwrap().note,
            Note::Refused(Reason::Refused(Refusal::NoDemo))
        );
    }

    #[test]
    fn picker_refuses_the_other_league() {
        let mut r = Rig::new();
        let fb = r.fb();
        r.a.demo.on = true;
        let m = r.ma(MS, 1);
        assert!(matches!(
            r.sa.challenge(&m, &mut r.a, &fb),
            Err(Reason::Refused(Refusal::DemoOnly))
        ));
        // And the other way round.
        let mut r = Rig::new();
        r.b.demo.on = true;
        let fb = r.fb();
        let m = r.ma(MS, 1);
        assert!(matches!(
            r.sa.challenge(&m, &mut r.a, &fb),
            Err(Reason::Refused(Refusal::NoDemo))
        ));
    }

    #[test]
    fn demo_target_auto_accepts() {
        let mut r = Rig::new();
        r.a.demo.on = true;
        r.b.demo.on = true;
        let c = r.challenge();
        let mb = r.mb(MS + 1000, 0x2222);
        let outs = r.sb.on_challenge(&mb, &mut r.b, &c, None);
        assert!(has_show(&outs));
        assert_eq!(r.sb.deadline_ms(), Some(MS + 1000 + DEMO_ACCEPT_MS));
        let late = r.mb(MS + 1000 + DEMO_ACCEPT_MS, 0x2222);
        let outs = r.sb.on_deadline(&late, &mut r.b, None);
        let acc = accept_of(&outs);
        let fb = fought(&outs).expect("demo target fought");
        let ma = r.ma(MS + 12_000, 0);
        let fa = fought(&r.sa.on_accept(&ma, &mut r.a, &acc)).unwrap();
        assert_eq!(fa.won, !fb.won);
        assert_eq!(r.b.battle_record().0 + r.b.battle_record().1, 1);
    }

    #[test]
    fn sleeping_demo_target_declines_at_once() {
        let mut r = Rig::new();
        r.a.demo.on = true;
        r.b.demo.on = true;
        r.b.is_sleeping = true;
        let c = r.challenge();
        let mb = r.mb(MS + 1000, 0x2222);
        let before = r.b.to_bytes();
        let outs = r.sb.on_challenge(&mb, &mut r.b, &c, None);
        assert_eq!(outs.len(), 1);
        assert_eq!(
            decline_of(&outs).reason(),
            Some(Reason::Refused(Refusal::Asleep))
        );
        assert!(r.b.is_sleeping);
        assert_eq!(r.b.to_bytes(), before);
        assert!(r.sb.view().is_none());
    }

    #[test]
    fn hibernating_target_is_untouched() {
        let mut r = Rig::new();
        r.b.hibernating = true;
        let c = r.challenge();
        let mb = r.mb(MS + 1000, 0x2222);
        let before = r.b.to_bytes();
        let outs = r.sb.on_challenge(&mb, &mut r.b, &c, None);
        assert_eq!(outs.len(), 1, "{outs:?}");
        let acc = accept_of(&outs);
        assert_eq!(r.b.to_bytes(), before);
        assert!(r.sb.view().is_none());
        let ma = r.ma(MS + 3000, 0);
        let fa = fought(&r.sa.on_accept(&ma, &mut r.a, &acc)).expect("normal fight");
        assert!(fa.live);
    }

    #[test]
    fn hibernating_target_checks_the_league() {
        let mut r = Rig::new();
        let c = r.challenge(); // the beacon says player
        r.b.hibernating = true;
        r.b.demo.on = true;
        let mb = r.mb(MS + 1000, 0x2222);
        let outs = r.sb.on_challenge(&mb, &mut r.b, &c, None);
        assert_eq!(
            decline_of(&outs).reason(),
            Some(Reason::Refused(Refusal::NoDemo))
        );
    }

    #[test]
    fn accept_from_the_other_league_is_ignored() {
        let mut r = Rig::new();
        let c = r.challenge();
        let mb = r.mb(MS + 1000, 0x2222);
        r.sb.on_challenge(&mb, &mut r.b, &c, None);
        let mut acc = accept_of(&r.sb.answer(&mb, &mut r.b, true));
        acc.snap.flags = acc.snap.flags.with_demo(true);
        let before = r.a.to_bytes();
        let ma = r.ma(MS + 3000, 0);
        let outs = r.sa.on_accept(&ma, &mut r.a, &acc);
        assert!(outs.is_empty());
        assert_eq!(r.a.to_bytes(), before);
    }

    #[test]
    fn injured_target_declines_with_its_reason() {
        let mut r = Rig::new();
        let c = r.challenge();
        r.b.injured_min = 30;
        let mb = r.mb(MS + 1000, 0x2222);
        let outs = r.sb.on_challenge(&mb, &mut r.b, &c, None);
        assert!(!has_show(&outs) && !has_buzz(&outs));
        let d = decline_of(&outs);
        assert_eq!(d.reason(), Some(Reason::Refused(Refusal::Injured)));
        assert_eq!(r.b.decline_min, 0);
        let ma = r.ma(MS + 2000, 0);
        r.sa.on_decline(&ma, &d);
        assert_eq!(
            r.sa.view().unwrap().note,
            Note::Refused(Reason::Refused(Refusal::Injured))
        );
    }

    #[test]
    fn sleeping_injured_target_declines() {
        let mut r = Rig::new();
        let c = r.challenge();
        r.b.injured_min = 30;
        r.b.is_sleeping = true;
        let mb = r.mb(MS + 1000, 0x2222);
        let outs = r.sb.on_challenge(&mb, &mut r.b, &c, None);
        assert!(!has_show(&outs) && !has_buzz(&outs));
        assert_eq!(
            decline_of(&outs).reason(),
            Some(Reason::Refused(Refusal::Injured))
        );
        assert!(r.b.is_sleeping);
    }

    #[test]
    fn sleeping_target_wakes_to_fight() {
        let mut r = Rig::new();
        let c = r.challenge();
        r.b.is_sleeping = true;
        let mb = r.mb(MS + 1000, 0x2222);
        let outs = r.sb.on_challenge(&mb, &mut r.b, &c, None);
        assert!(has_show(&outs) && has_buzz(&outs));
        assert!(r.b.is_sleeping, "not woken by the challenge alone");
        let outs = r.sb.answer(&mb, &mut r.b, true);
        assert!(!r.b.is_sleeping);
        assert!(fought(&outs).is_some());
    }

    #[test]
    fn injured_challenger_is_refused() {
        let mut r = Rig::new();
        r.a.injured_min = 30;
        let fb = r.fb();
        let m = r.ma(MS, 1);
        assert!(matches!(
            r.sa.challenge(&m, &mut r.a, &fb),
            Err(Reason::Refused(Refusal::Injured))
        ));
        assert!(r.sa.view().is_none());
    }

    #[test]
    fn old_badge_friend_is_never_challenged() {
        let mut r = Rig::new();
        let mut fb = r.fb();
        fb.v3 = false;
        let m = r.ma(MS, 1);
        let outs = r.sa.challenge(&m, &mut r.a, &fb).expect("resolved at once");
        assert!(sends(&outs).is_empty());
        assert_eq!(r.sa.view().unwrap().note, Note::NotReady);
        assert_eq!(r.sa.deadline_ms(), None);
        assert_eq!(r.a.cooldown_battle, 0);
    }

    #[test]
    fn friend_without_numbers_is_a_guard_refusal() {
        let mut r = Rig::new();
        let mut fb = r.fb();
        fb.hp = 0;
        let m = r.ma(MS, 1);
        assert!(matches!(
            r.sa.challenge(&m, &mut r.a, &fb),
            Err(Reason::Refused(Refusal::NoNeed))
        ));
    }

    #[test]
    fn second_challenge_is_busy() {
        let mut r = Rig::new();
        r.challenge();
        let fb = r.fb();
        let m = r.ma(MS + 100, 2);
        assert!(matches!(
            r.sa.challenge(&m, &mut r.a, &fb),
            Err(Reason::Busy)
        ));
    }

    #[test]
    fn target_window_closes_first() {
        let mut r = Rig::new();
        let c = r.challenge();
        assert_eq!(r.sa.deadline_ms(), Some(MS + ANSWER_WINDOW_MS));
        let mb = r.mb(MS + 1000, 0x2222);
        r.sb.on_challenge(&mb, &mut r.b, &c, None);
        let b_end = r.sb.deadline_ms().unwrap();
        assert_eq!(b_end - (MS + 1000), TARGET_WINDOW_MS);
        assert!(b_end < MS + ANSWER_WINDOW_MS);
        // The window runs out without a Fight: closed, nothing paid.
        let late = r.mb(b_end, 0x2222);
        let before = r.b.to_bytes();
        let outs = r.sb.on_deadline(&late, &mut r.b, None);
        assert!(outs.iter().any(|x| matches!(x, Out::Close)));
        assert!(sends(&outs).is_empty() && recorded(&outs).is_none());
        assert_eq!(r.b.to_bytes(), before);
        assert!(r.sb.view().is_none());
    }

    #[test]
    fn late_accept_is_ignored() {
        let mut r = Rig::new();
        let c = r.challenge();
        let mb = r.mb(MS + 1000, 0x2222);
        r.sb.on_challenge(&mb, &mut r.b, &c, None);
        let acc = accept_of(&r.sb.answer(&mb, &mut r.b, true));
        let ma = r.ma(MS + ANSWER_WINDOW_MS + 1, 0);
        assert!(r.sa.on_accept(&ma, &mut r.a, &acc).is_empty());
    }

    #[test]
    fn late_result_after_accept_is_ignored() {
        let mut r = Rig::new();
        let c = r.challenge();
        let mb = r.mb(MS + 1000, 0x2222);
        r.sb.on_challenge(&mb, &mut r.b, &c, None);
        r.sb.answer(&mb, &mut r.b, true);
        let before = r.b.battle_record();
        let res = BattleResultMsg {
            challenger_id: [1, 0],
            target_id: [2, 0],
            challenger_won: true,
            challenger_hp_pct: 50,
            target_hp_pct: 0,
            challenger_kind: 3,
            target_kind: 1,
        };
        let soon = r.mb(MS + 1000 + LATE_RESULT_MS - 1, 0);
        assert!(
            r.sb.on_result(&soon, &mut r.b, &res, Some(false))
                .is_empty()
        );
        assert_eq!(r.b.battle_record(), before);
        // Past the window the same packet counts again.
        let later = r.mb(MS + 1000 + LATE_RESULT_MS + 1, 0);
        let outs = r.sb.on_result(&later, &mut r.b, &res, Some(false));
        assert!(toast(&outs).is_some());
    }

    #[test]
    fn result_is_ignored_in_the_wrong_league_or_hibernating() {
        let mut r = Rig::new();
        let res = BattleResultMsg {
            challenger_id: [1, 0],
            target_id: [2, 0],
            challenger_won: true,
            challenger_hp_pct: 50,
            target_hp_pct: 0,
            challenger_kind: 3,
            target_kind: 1,
        };
        let mb = r.mb(MS, 0);
        // A demo sender, a player target.
        assert!(r.sb.on_result(&mb, &mut r.b, &res, Some(true)).is_empty());
        r.b.hibernating = true;
        assert!(r.sb.on_result(&mb, &mut r.b, &res, None).is_empty());
        assert_eq!(r.b.battle_record(), (0, 0));
    }

    #[test]
    fn busy_target_declines() {
        let mut r = Rig::new();
        let from_c = ChallengeMsg {
            challenger_id: [3, 0],
            target_id: [2, 0],
            snap: r.a.snapshot(),
            nonce: 5,
            kind: 1,
            name: [0; 12],
            name_len: 0,
        };
        let mb = r.mb(MS, 1);
        assert!(has_show(&r.sb.on_challenge(&mb, &mut r.b, &from_c, None)));
        let c = r.challenge();
        let mb = r.mb(MS + 500, 2);
        let outs = r.sb.on_challenge(&mb, &mut r.b, &c, None);
        assert_eq!(decline_of(&outs).reason(), Some(Reason::Busy));
        assert!(!has_show(&outs) && !has_buzz(&outs));
    }

    #[test]
    fn pair_cooldown_blocks_a_rematch() {
        let ida = [1, 0];
        let idb = [0x7C, 0x22];
        let mut r = Rig::with_ids(ida, idb, false);
        friends::insert_for_test(friend_of(&r.b, idb, 0));
        let (fa, _) = live(&mut r);
        // The glue applies Recorded through the real note_fight.
        friends::note_fight(idb, fa.won, TICK);
        let rec = friends::by_id(idb).unwrap();
        assert_eq!(rec.fought_tick, TICK);

        // 45 minutes on, the per-pet rest is over but the pair rule holds.
        r.a.cooldown_battle = 0;
        let now = TICK + 45 * 6;
        let m = me(ida, "Ant", MS + 10_000_000, now, 3);
        match r.sa.challenge(&m, &mut r.a, &rec) {
            Err(Reason::Wait(w)) => assert_eq!(w, 900),
            other => panic!("{other:?}"),
        }
        let now = TICK + 61 * 6;
        let m = me(ida, "Ant", MS + 20_000_000, now, 3);
        assert!(r.sa.challenge(&m, &mut r.a, &rec).is_ok());
    }

    #[test]
    fn packets_not_for_me_are_ignored() {
        let mut r = Rig::new();
        let c = r.challenge();
        let mut other = c;
        other.target_id = [9, 9];
        let mb = r.mb(MS, 1);
        assert!(r.sb.on_challenge(&mb, &mut r.b, &other, None).is_empty());
        // A challenge from myself.
        let mut mine = c;
        mine.challenger_id = [2, 0];
        assert!(r.sb.on_challenge(&mb, &mut r.b, &mine, None).is_empty());
        assert!(r.sb.view().is_none());

        let acc = AcceptMsg {
            challenger_id: [7, 7],
            target_id: [2, 0],
            snap: r.b.snapshot(),
            nonce: 1,
        };
        let ma = r.ma(MS + 100, 0);
        assert!(r.sa.on_accept(&ma, &mut r.a, &acc).is_empty());
        let res = BattleResultMsg {
            challenger_id: [1, 0],
            target_id: [9, 9],
            challenger_won: true,
            challenger_hp_pct: 1,
            target_hp_pct: 0,
            challenger_kind: 0,
            target_kind: 0,
        };
        assert!(r.sb.on_result(&mb, &mut r.b, &res, None).is_empty());
        let d = DeclineMsg::new([1, 0], [9, 9], Reason::Busy);
        assert!(r.sa.on_decline(&ma, &d).is_empty());
        assert_eq!(r.sa.view().unwrap().note, Note::Waiting);
    }

    #[test]
    fn cancel_drops_the_challenge() {
        let mut r = Rig::new();
        assert!(r.sa.cancel().is_empty());
        r.challenge();
        assert!(r.sa.view().is_some());
        let outs = r.sa.cancel();
        assert!(outs.iter().any(|x| matches!(x, Out::Close)));
        assert!(r.sa.view().is_none());
        assert_eq!(r.sa.deadline_ms(), None);
    }

    /// A challenges B at `t0`, B presses Fight 1 s later, A hears it at +3 s.
    fn live_at(seed: u64, t0: u32, na: u16, nb: u16) -> (Fight, Fight) {
        let mut r = Rig::with_ids([1, 0], [2, 0], false);
        r.a = adult(11 + seed, false);
        r.b = adult(40 + seed * 3, false);
        let fb = r.fb();
        let m = r.ma(t0, na);
        let outs = r.sa.challenge(&m, &mut r.a, &fb).expect("challenge");
        let Packet::Challenge(c) = one_send(&outs) else {
            panic!()
        };
        let mb = r.mb(t0.wrapping_add(1000), nb);
        r.sb.on_challenge(&mb, &mut r.b, &c, None);
        let outs = r.sb.answer(&mb, &mut r.b, true);
        let acc = accept_of(&outs);
        let fb = fought(&outs).expect("B fought");
        let ma = r.ma(t0.wrapping_add(3000), 0);
        let fa = fought(&r.sa.on_accept(&ma, &mut r.a, &acc)).expect("A fought");
        (fa, fb)
    }

    #[test]
    fn both_badges_agree_over_many_seeds_and_a_wrapping_clock() {
        let mut wins = 0;
        for i in 0..12u64 {
            let na = (i as u16).wrapping_mul(7919).wrapping_add(3);
            let nb = (i as u16).wrapping_mul(39_208).wrapping_add(1);
            for t0 in [MS, u32::MAX - 5_000] {
                let (fa, fb) = live_at(i, t0, na, nb);
                assert_eq!(fa.won, !fb.won, "seed {i} t0 {t0}");
                assert_eq!((fa.my_hp, fa.their_hp), (fb.their_hp, fb.my_hp), "seed {i}");
                assert_eq!(fa.chance as u32 + fb.chance as u32, 100);
                wins += fa.won as u32;
            }
        }
        // Both outcomes occur: the check is not vacuous.
        assert!(wins > 0 && wins < 24, "{wins}");
    }

    #[test]
    fn windows_survive_the_clock_wrap() {
        let t0 = u32::MAX - 5_000;
        let mut r = Rig::new();
        let fb = r.fb();
        let m = r.ma(t0, 0x1111);
        let outs = r.sa.challenge(&m, &mut r.a, &fb).unwrap();
        let Packet::Challenge(c) = one_send(&outs) else {
            panic!()
        };
        assert_eq!(r.sa.deadline_ms(), Some(t0.wrapping_add(ANSWER_WINDOW_MS)));
        let mb = r.mb(t0.wrapping_add(1000), 0x2222);
        r.sb.on_challenge(&mb, &mut r.b, &c, None);
        // A late ACCEPT (past +20 s, after the wrap) is ignored.
        let mut sb2 = Session::new();
        let mut b2 = r.b.clone();
        sb2.on_challenge(&mb, &mut b2, &c, None);
        let acc = accept_of(&sb2.answer(&mb, &mut b2, true));
        let late = r.ma(t0.wrapping_add(ANSWER_WINDOW_MS), 0);
        assert!(r.sa.on_accept(&late, &mut r.a, &acc).is_empty());
        let ok = r.ma(t0.wrapping_add(ANSWER_WINDOW_MS - 1), 0);
        assert!(fought(&r.sa.on_accept(&ok, &mut r.a, &acc)).is_some());
        // B: the accepted peer's RESULT is ignored for 60 s across the wrap.
        let res = BattleResultMsg {
            challenger_id: [1, 0],
            target_id: [2, 0],
            challenger_won: true,
            challenger_hp_pct: 50,
            target_hp_pct: 0,
            challenger_kind: 3,
            target_kind: 1,
        };
        let answered = r.mb(t0.wrapping_add(2000), 0x2222);
        r.sb.answer(&answered, &mut r.b, true);
        let soon = r.mb(t0.wrapping_add(2000 + LATE_RESULT_MS - 1), 0);
        assert!(
            r.sb.on_result(&soon, &mut r.b, &res, Some(false))
                .is_empty()
        );
        let later = r.mb(t0.wrapping_add(2000 + LATE_RESULT_MS), 0);
        assert!(toast(&r.sb.on_result(&later, &mut r.b, &res, Some(false))).is_some());
    }

    fn refused_reason(o: &Outs) -> Option<Reason> {
        o.iter().find_map(|x| if let Out::Refused(r) = x { Some(*r) } else { None })
    }

    #[test]
    fn a_refused_fight_press_names_its_reason() {
        let mut r = Rig::new();
        let c = r.challenge();
        let mb = r.mb(MS + 1000, 0x2222);
        r.sb.on_challenge(&mb, &mut r.b, &c, None);
        r.b.injured_min = 30;
        let outs = r.sb.answer(&mb, &mut r.b, true);
        assert_eq!(refused_reason(&outs), Some(Reason::Refused(Refusal::Injured)));
        assert_eq!(
            decline_of(&outs).reason(),
            Some(Reason::Refused(Refusal::Injured))
        );
    }

    #[test]
    fn a_sleeping_demo_target_names_its_reason() {
        let mut r = Rig::new();
        // Both demo, so the league lets the challenge in: only the answer's
        // own rule is under test, the demo pet fell asleep in the window.
        r.a.demo.on = true;
        r.b.demo.on = true;
        let c = r.challenge();
        let mb = r.mb(MS + 1000, 0x2222);
        r.sb.on_challenge(&mb, &mut r.b, &c, None);
        assert!(r.sb.view().is_some(), "the demo target took the challenge");
        r.b.is_sleeping = true;
        let outs = r.sb.answer(&mb, &mut r.b, true);
        assert_eq!(refused_reason(&outs), Some(Reason::Refused(Refusal::Asleep)));
    }

    #[test]
    fn a_timeout_names_no_refusal() {
        let mut r = Rig::new();
        let c = r.challenge();
        let mb = r.mb(MS + 1000, 0x2222);
        r.sb.on_challenge(&mb, &mut r.b, &c, None);
        r.b.injured_min = 30;
        let end = r.sb.deadline_ms().unwrap();
        let late = r.mb(end + 1, 0x2222);
        let outs = r.sb.answer(&late, &mut r.b, true);
        assert!(refused_reason(&outs).is_none());
        let outs = r.sb.on_deadline(&late, &mut r.b, None);
        assert!(refused_reason(&outs).is_none());
    }

    #[test]
    fn late_fight_press_is_the_window_running_out() {
        let mut r = Rig::new();
        let c = r.challenge();
        let mb = r.mb(MS + 1000, 0x2222);
        r.sb.on_challenge(&mb, &mut r.b, &c, None);
        let end = r.sb.deadline_ms().unwrap();
        let before = r.b.to_bytes();
        let late = r.mb(end + 1, 0x2222);
        let outs = r.sb.answer(&late, &mut r.b, true);
        assert!(sends(&outs).is_empty() && fought(&outs).is_none());
        assert!(outs.iter().any(|x| matches!(x, Out::Close)));
        assert_eq!(r.b.to_bytes(), before);
        assert!(r.sb.view().is_none());
    }

    #[test]
    fn early_deadline_is_a_no_op() {
        let mut r = Rig::new();
        let fb = r.fb();
        let m = r.ma(MS, 0x1111);
        r.sa.challenge(&m, &mut r.a, &fb).unwrap();
        let before = r.a.to_bytes();
        let early = r.ma(MS + ANSWER_WINDOW_MS - 1, 0x1111);
        assert!(r.sa.on_deadline(&early, &mut r.a, Some(&fb)).is_empty());
        assert_eq!(r.a.to_bytes(), before);
        assert_eq!(r.sa.deadline_ms(), Some(MS + ANSWER_WINDOW_MS));
        // The target side too.
        let mut r = Rig::new();
        let c = r.challenge();
        let mb = r.mb(MS + 1000, 0x2222);
        r.sb.on_challenge(&mb, &mut r.b, &c, None);
        let before = r.b.to_bytes();
        let early = r.mb(MS + 1000 + TARGET_WINDOW_MS - 1, 0x2222);
        assert!(r.sb.on_deadline(&early, &mut r.b, None).is_empty());
        assert_eq!(r.b.to_bytes(), before);
        assert!(r.sb.view().is_some());
    }

    #[test]
    fn late_press_means_one_charged_fight_per_side() {
        let mut r = Rig::new();
        let fb = r.fb();
        let m = r.ma(MS, 0x1111);
        let outs = r.sa.challenge(&m, &mut r.a, &fb).unwrap();
        let Packet::Challenge(c) = one_send(&outs) else {
            panic!()
        };
        let mb = r.mb(MS + 1000, 0x2222);
        r.sb.on_challenge(&mb, &mut r.b, &c, None);
        let (ea, eb) = (r.a.energy, r.b.energy);
        // B's player presses Fight after B's window: B does not fight.
        let late = r.mb(MS + 1000 + TARGET_WINDOW_MS, 0x2222);
        let outs = r.sb.answer(&late, &mut r.b, true);
        assert!(fought(&outs).is_none() && sends(&outs).is_empty());
        // A's window runs out: the fallback fight, charged once.
        let end = r.ma(MS + ANSWER_WINDOW_MS, 0x1111);
        let outs = r.sa.on_deadline(&end, &mut r.a, Some(&fb));
        let fa = fought(&outs).expect("fallback");
        assert!(!fa.live);
        assert_eq!(r.a.energy as i32, ea as i32 - 10 * P);
        let Packet::Result(res) = one_send(&outs) else {
            panic!()
        };
        // B only hears the record.
        let hear = r.mb(MS + 21_000, 0x2222);
        let outs = r.sb.on_result(&hear, &mut r.b, &res, Some(false));
        assert!(toast(&outs).is_some());
        assert_eq!(r.b.energy, eb);
        assert_eq!(r.b.cooldown_battle, 0);
    }

    #[test]
    fn repeated_challenge_is_not_answered_busy() {
        let mut r = Rig::new();
        let c = r.challenge();
        let mb = r.mb(MS + 1000, 0x2222);
        assert!(has_show(&r.sb.on_challenge(&mb, &mut r.b, &c, None)));
        let again = r.mb(MS + 1500, 0x2223);
        assert!(r.sb.on_challenge(&again, &mut r.b, &c, None).is_empty());
        assert!(r.sb.view().is_some());
        // A different nonce from the same peer is a new challenge: busy.
        let mut other = c;
        other.nonce ^= 1;
        let outs = r.sb.on_challenge(&again, &mut r.b, &other, None);
        assert_eq!(decline_of(&outs).reason(), Some(Reason::Busy));
    }

    #[test]
    fn crossing_challenges_are_both_busy() {
        let mut r = Rig::new();
        let fa = friend_of(&r.a, r.ida, 0);
        let fb = r.fb();
        let (ma, mb) = (r.ma(MS, 0x1111), r.mb(MS, 0x2222));
        let oa = r.sa.challenge(&ma, &mut r.a, &fb).unwrap();
        let ob = r.sb.challenge(&mb, &mut r.b, &fa).unwrap();
        let (Packet::Challenge(ca), Packet::Challenge(cb)) = (one_send(&oa), one_send(&ob)) else {
            panic!()
        };
        let (ea, eb) = (r.a.energy, r.b.energy);
        let da = r.sa.on_challenge(&ma, &mut r.a, &cb, None);
        let db = r.sb.on_challenge(&mb, &mut r.b, &ca, None);
        assert_eq!(decline_of(&da).reason(), Some(Reason::Busy));
        assert_eq!(decline_of(&db).reason(), Some(Reason::Busy));
        assert!(fought(&da).is_none() && fought(&db).is_none());
        r.sa.on_decline(&ma, &decline_of(&db));
        r.sb.on_decline(&mb, &decline_of(&da));
        assert_eq!(r.sa.view().unwrap().note, Note::Refused(Reason::Busy));
        assert_eq!(r.sb.view().unwrap().note, Note::Refused(Reason::Busy));
        assert_eq!((r.a.energy, r.b.energy), (ea, eb));
        assert_eq!(r.a.battle_record(), (0, 0));
        assert_eq!(r.b.battle_record(), (0, 0));
        // a Busy refusal is not a decline: no decline penalty on either side
        assert_eq!((r.a.decline_min, r.b.decline_min), (0, 0));
    }

    #[test]
    fn injured_in_the_window_declines_with_its_reason() {
        let mut r = Rig::new();
        let c = r.challenge();
        let mb = r.mb(MS + 1000, 0x2222);
        r.sb.on_challenge(&mb, &mut r.b, &c, None);
        r.b.injured_min = 30;
        let energy = r.b.energy;
        let outs = r.sb.answer(&mb, &mut r.b, true);
        assert!(fought(&outs).is_none());
        assert_eq!(
            decline_of(&outs).reason(),
            Some(Reason::Refused(Refusal::Injured))
        );
        assert_eq!(r.b.energy, energy);
        assert_eq!(r.b.decline_min, 0);
        assert!(r.sb.view().is_none());
    }

    #[test]
    fn heard_result_closes_the_target_screen() {
        let mut r = Rig::new();
        let c = r.challenge();
        let mb = r.mb(MS + 1000, 0x2222);
        r.sb.on_challenge(&mb, &mut r.b, &c, None);
        let res = BattleResultMsg {
            challenger_id: [1, 0],
            target_id: [2, 0],
            challenger_won: false,
            challenger_hp_pct: 0,
            target_hp_pct: 40,
            challenger_kind: 3,
            target_kind: 1,
        };
        let hear = r.mb(MS + 2000, 0);
        let outs = r.sb.on_result(&hear, &mut r.b, &res, Some(false));
        assert!(outs.iter().any(|x| matches!(x, Out::Close)));
        assert_eq!(toast(&outs), Some(Toast::BattleWon));
        assert!(r.sb.view().is_none());
        assert_eq!(r.sb.deadline_ms(), None);
    }

    #[test]
    fn session_uses_no_statics() {
        let src = include_str!("../src/game/battle.rs");
        for bad in ["SyncCell", "static ", "lifecycle::", "fw::"] {
            assert!(!src.contains(bad), "battle.rs mentions {bad}");
        }
    }
}

/// Task 10: battle lifestyles and money in the simulator (spec 18.4, 18.5).
/// The long runs are `#[ignore]`d and run on CI (`--include-ignored`).
mod balance {
    use bornhack_aegg::game::engine::GameState;
    use bornhack_aegg::game::engine::sim::{self, Driver, Outcome, Plan, Setup};

    const SEEDS: [u64; 3] = [1, 2, 3];
    const MONEY: Setup = Setup {
        demo: false,
        clock: true,
        animated: false,
        money: true,
    };
    const CLOCK: Setup = Setup {
        money: false,
        ..MONEY
    };

    type Routine = fn(u32, u32, &GameState) -> Plan;

    fn run(
        r: impl Fn(u32, u32, &GameState) -> Plan,
        days: u32,
        driver: Driver,
        seed: u64,
        setup: Setup,
    ) -> Outcome {
        sim::run_average(&r, days, driver, seed, setup).outcome
    }

    fn runs(name: &str, r: Routine, setup: Setup) -> Vec<Outcome> {
        SEEDS
            .iter()
            .map(|&seed| {
                let o = run(r, 30, Driver::Delta, seed, setup);
                eprintln!(
                    "{name} seed {seed}: alive={} min_health={} win%={} fights={} won={} refused={} fight_injuries={} prizes={} mood={}",
                    o.alive,
                    o.min_health_pct,
                    o.win_pct_vs_reference,
                    o.fights,
                    o.fights_won,
                    o.fights_refused,
                    o.fight_injuries,
                    o.prizes_hax,
                    o.avg_mood_pct
                );
                o
            })
            .collect()
    }

    fn diff(a: u8, b: u8) -> i32 {
        (a as i32 - b as i32).abs()
    }

    #[test]
    fn battle_smoke_three_days() {
        let o = run(sim::fighter, 3, Driver::Delta, 1, CLOCK);
        eprintln!("smoke: {o:?}");
        assert!(o.alive);
        assert_eq!(o.fights, 3);
        assert_eq!(o.fight_injuries, 0);
        assert_eq!(o.prizes_hax, 0, "money is off");
    }

    /// The lifestyle rows of spec 18.4 were measured on the prototype's pet,
    /// whose three traits are all 50 % (`Cond`: vit, cur, res). The balance
    /// runs use that pet, so a seed moves the fights, not the build (seed 1
    /// hatches a 37/32/39 pet, seed 3 a 62/48/55 one).
    #[test]
    fn balance_runs_use_the_prototype_traits() {
        use bornhack_aegg::game::engine::{Phase, trait_pct};
        use std::cell::Cell;
        let seen = Cell::new(None);
        let r = |d: u32, m: u32, s: &GameState| {
            if s.phase == Phase::Active {
                let t = |v: u16| trait_pct(v);
                seen.set(Some((t(s.vitality), t(s.curiosity), t(s.resilience))));
            }
            sim::fighter(d, m, s)
        };
        run(r, 1, Driver::Delta, 1, CLOCK);
        assert_eq!(seen.get(), Some((50, 50, 50)));
    }

    /// The prototype's fit fighter comes to its 20:00 fight fresh (stamina
    /// 4 on day 20, spec 18.4); two sessions of sport a day leave it at 1.
    #[test]
    fn fit_fighter_comes_fresh_to_its_fight() {
        use bornhack_aegg::game::engine::Phase;
        use std::cell::RefCell;
        let stamina = RefCell::new(Vec::new());
        let r = |d: u32, m: u32, s: &GameState| {
            let p = sim::fighter(d, m, s);
            // the routine is also asked with probe eggs; only the pet counts
            if p.contains(&sim::Act::Battle) && s.phase != Phase::Hatching {
                stamina.borrow_mut().push(s.snapshot().stamina);
            }
            p
        };
        let o = run(r, 10, Driver::Delta, 1, CLOCK);
        let st = stamina.into_inner();
        eprintln!("fighter stamina at 20:00: {st:?}");
        assert!(o.alive);
        assert_eq!(st.len(), 10);
        // day 0 starts at 08:00, after the 07:30 breakfast (as in the prototype)
        assert!(st[1..].iter().all(|&k| k >= 3), "{st:?}");
    }

    /// The prototype's brawler holds about 78 % weight on rich food and two
    /// beers (spec 18.4); it is not the party routine with more beer.
    #[test]
    fn brawler_holds_its_weight() {
        let o = run(sim::brawler, 10, Driver::Delta, 1, CLOCK);
        eprintln!("brawler 10 days: {o:?}");
        assert!(o.alive);
        assert!(o.max_weight_pct <= 82, "max weight {}", o.max_weight_pct);
        assert!(!o.diabetic);
    }

    /// The prototype's sumo holds about 73 % weight (spec 18.4).
    #[test]
    fn sumo_holds_its_weight() {
        let o = run(sim::sumo, 10, Driver::Delta, 1, CLOCK);
        eprintln!("sumo 10 days: {o:?}");
        assert!(o.alive);
        assert!(o.max_weight_pct <= 80, "max weight {}", o.max_weight_pct);
        assert!(!o.diabetic);
    }

    /// A money run measures the prizes, not the price list: the prototype
    /// lets HAX go negative (spec 15.9), so its fighter, which never works,
    /// keeps eating. With the badge's prices and a 100 HAX start the sim's
    /// fighter spent 80 HAX before its first fight and starved by day 3.
    #[test]
    fn money_run_is_paid_its_prizes() {
        let o = run(sim::fighter, 10, Driver::Delta, 1, MONEY);
        eprintln!("fighter money 10 days: {o:?}");
        assert!(o.alive);
        assert_eq!(o.fights, 10);
        assert_eq!(o.prizes_hax, 100 * o.fights_won, "first win of each day");
        assert!(o.fights_won >= 5, "won {}", o.fights_won);
    }

    #[test]
    #[ignore]
    fn fit_fighter_one_a_day() {
        for o in runs("fighter", sim::fighter, CLOCK) {
            assert!(o.alive);
            assert!(o.min_health_pct >= 70, "min health {}", o.min_health_pct);
            assert_eq!(o.fight_injuries, 0);
            assert!(o.win_pct_vs_reference >= 70, "{}", o.win_pct_vs_reference);
            assert_eq!(o.fights_refused, 0);
        }
    }

    #[test]
    #[ignore]
    fn fit_fighter_six_a_day_pays_for_it() {
        for o in runs("fighter_six", sim::fighter_six, CLOCK) {
            assert!(o.alive);
            assert!(o.fight_injuries >= 5, "injuries {}", o.fight_injuries);
            assert!(o.win_pct_vs_reference <= 50, "{}", o.win_pct_vs_reference);
            assert!(o.fights_refused >= 20, "refused {}", o.fights_refused);
        }
    }

    #[test]
    #[ignore]
    fn brawler_one_a_day() {
        for o in runs("brawler", sim::brawler, CLOCK) {
            assert!(o.alive);
            assert!(o.win_pct_vs_reference <= 45, "{}", o.win_pct_vs_reference);
        }
    }

    #[test]
    #[ignore]
    fn brawler_six_a_day_is_ruinous() {
        for o in runs("brawler_six", sim::brawler_six, CLOCK) {
            assert!(
                !o.alive || o.min_health_pct < 30,
                "alive and min health {}",
                o.min_health_pct
            );
        }
    }

    #[test]
    #[ignore]
    fn sumo_matches_the_fit_fighter() {
        let fit = runs("fighter", sim::fighter, CLOCK);
        for (o, f) in runs("sumo", sim::sumo, CLOCK).iter().zip(&fit) {
            assert!(o.alive);
            assert!(
                diff(o.win_pct_vs_reference, f.win_pct_vs_reference) <= 10,
                "sumo {} vs fighter {}",
                o.win_pct_vs_reference,
                f.win_pct_vs_reference
            );
        }
    }

    #[test]
    #[ignore]
    fn prize_rule_caps_income() {
        for o in runs("fighter_six money", sim::fighter_six, MONEY) {
            assert!(o.prizes_hax <= 30 * (100 + 5 * 20), "{}", o.prizes_hax);
        }
        for o in runs("fighter money", sim::fighter, MONEY) {
            assert!(o.prizes_hax <= 30 * 100, "{}", o.prizes_hax);
            // spec 18.5: about 2600 a month (26 of 30 won)
            assert!(o.alive && o.prizes_hax >= 1500, "{}", o.prizes_hax);
        }
    }

    #[test]
    #[ignore]
    fn battle_delta_matches_oracle() {
        let both = [
            ("fighter", sim::fighter as Routine),
            ("brawler", sim::brawler),
        ];
        for (name, r) in both {
            for seed in [1u64, 2] {
                let o = run(r, 10, Driver::Oracle, seed, CLOCK);
                let d = run(r, 10, Driver::Delta, seed, CLOCK);
                eprintln!("{name} seed {seed}: oracle {o:?}\n  delta {d:?}");
                assert_eq!(o.alive, d.alive, "{name}: alive");
                assert_eq!(o.fights, d.fights, "{name}: fights");
                assert_eq!(o.fights_won, d.fights_won, "{name}: won");
                assert_eq!(o.fights_refused, d.fights_refused, "{name}: refused");
                assert_eq!(o.fight_injuries, d.fight_injuries, "{name}: injuries");
                assert!(diff(o.avg_mood_pct, d.avg_mood_pct) <= 1, "{name}: mood");
                assert!(
                    diff(o.end_health_pct, d.end_health_pct) <= 1,
                    "{name}: health"
                );
            }
        }
    }
}
