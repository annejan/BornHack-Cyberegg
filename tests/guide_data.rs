//! The guide's numbers dump, Classic. One process per preset: the engine's
//! threshold table installs once.
use bornhack_aegg::game::engine::doses::price;
use bornhack_aegg::game::engine::sim;
use bornhack_aegg::game::engine::thresholds::{BORNPETS_CFG_KEYS, Mode, Thresholds, badge_preset};
use bornhack_aegg::game::engine::{Action, FoodKind};
use bornhack_aegg::guide::data::{self, dump, fixed, unit_of};
use bornhack_aegg::guide::json::J;

fn rendered() -> String {
    dump(Mode::Classic).render()
}

fn install() {
    sim::install_preset(badge_preset(Mode::Classic), Mode::Classic, false);
}

#[test]
fn hatching_time_is_the_badges() {
    assert!(rendered().contains("\"HATCHING_TICKS\":6"));
    assert_eq!(badge_preset(Mode::Classic).HATCHING_TICKS, 6);
}

#[test]
fn every_field_has_a_unit() {
    let out = rendered();
    for k in Thresholds::FIELDS {
        assert!(out.contains(&format!("\"{k}\":\"")), "no unit for {k}");
        let _ = unit_of(k);
    }
    let fixed_keys: Vec<_> = fixed().iter().map(|(k, _, _)| *k).collect();
    for k in &fixed_keys {
        assert!(
            !Thresholds::FIELDS.contains(k),
            "{k} is both fixed and a field"
        );
    }
    let mut sorted = fixed_keys.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted.len(), fixed_keys.len(), "duplicate fixed key");
}

#[test]
fn overrides_win_over_suffix_rules() {
    assert_eq!(unit_of("HANGOVER_ASLEEP_MULT").name(), "count");
    assert_eq!(unit_of("DOSE_TIRED_FUN_MULT").name(), "raw");
    assert_eq!(unit_of("FULL_DRAIN_AWAKE_PM").name(), "raw");
    assert_eq!(unit_of("MINIGAME_COOLDOWN").name(), "ticks");
}

#[test]
fn values_match_the_presets() {
    let out = rendered();
    for k in Thresholds::FIELDS {
        if *k == "HATCHING_TICKS" {
            continue;
        }
        let v = Thresholds::CLASSIC.value_of(k).unwrap();
        assert!(
            out.contains(&format!("\"{k}\":{v},")) || out.contains(&format!("\"{k}\":{v}}}")),
            "{k} should be {v}"
        );
    }
}

#[test]
fn cfg_keys_in_table_order() {
    let want: Vec<String> = BORNPETS_CFG_KEYS
        .iter()
        .map(|(k, _)| format!("\"{k}\""))
        .collect();
    let want = format!("\"cfg_keys\":[{}]", want.join(","));
    assert!(rendered().contains(&want));
}

#[test]
fn prices_come_from_the_price_table() {
    let normal = data::prices(false);
    let hard = data::prices(true);
    let get = |v: &[(&str, Option<u32>)], k: &str| v.iter().find(|(n, _)| *n == k).unwrap().1;
    assert_eq!(
        get(&normal, "Salad"),
        price(Action::Feed, Some(FoodKind::Salad), None, false)
    );
    assert!(get(&hard, "Salad") > get(&normal, "Salad"));
    assert_eq!(get(&normal, "Exercise"), None);
}

#[test]
fn the_reference_pet_gets_peckish_then_sleeps_and_wakes() {
    install();
    let u = data::until();
    let peckish = u.iter().find(|(k, _)| *k == "peckish").unwrap().1;
    let m = peckish.expect("peckish within three days");
    assert!(m > 0 && m < 4320);
    assert_eq!(peckish, Some(243));
    assert_eq!(data::night_min(), Some(672));
    // Same numbers straight from the engine.
    use bornhack_aegg::game::engine::{GameState, PetKind, Phase, debug::DebugPoke, needs};
    let mut s = GameState::new_egg(1, PetKind(0));
    s.phase = Phase::Active;
    s.hatching_countdown = 0;
    s.is_sleeping = false;
    s.debug_poke(DebugPoke::Comfort).unwrap();
    let mut first = None;
    for m in 1..=4320u32 {
        s.catch_up(1);
        if first.is_none() && (s.fullness as i32) < needs::full_lo() {
            first = Some(m);
        }
    }
    assert_eq!(first, peckish);
}

#[test]
fn doses_report_effects_or_reasons() {
    install();
    let d = data::doses();
    let find = |k: &str| {
        d.iter()
            .find(|(n, _)| *n == k)
            .unwrap()
            .1
            .as_ref()
            .unwrap()
            .clone()
    };
    let has = |v: &[(&str, i32)], f: &str| v.iter().find(|(n, _)| *n == f).map(|(_, x)| *x);
    assert!(has(&find("Salad"), "full").unwrap() > 0);
    assert!(has(&find("Wash"), "dirt").unwrap() < 0);
    assert!(has(&find("Heal"), "injured_min").is_some());
}

#[test]
fn json_escapes_quotes() {
    assert_eq!(J::Str("a\"b".into()).render(), "\"a\\\"b\"");
    assert_eq!(J::Str("a\\b\n".into()).render(), "\"a\\\\b\\n\"");
    assert_eq!(
        J::Obj(vec![("k".into(), J::Arr(vec![J::Int(1), J::Null]))]).render(),
        "{\"k\":[1,null]}"
    );
}

#[test]
fn values_have_no_duplicate_keys() {
    let out = rendered();
    let start = out.find("\"values\":{").unwrap() + "\"values\":{".len();
    let end = start + out[start..].find('}').unwrap();
    let mut keys: Vec<&str> = out[start..end]
        .split(',')
        .map(|kv| kv.split(':').next().unwrap())
        .collect();
    let n = keys.len();
    keys.sort();
    keys.dedup();
    assert_eq!(keys.len(), n);
}
