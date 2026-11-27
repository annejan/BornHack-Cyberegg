//! The guide's numbers dump, Casual (its own process: the threshold table
//! installs once).
use bornhack_aegg::game::engine::thresholds::{Mode, Thresholds, badge_preset};
use bornhack_aegg::guide::data::dump;

#[test]
fn hatching_time_is_the_badges() {
    assert!(dump(Mode::Casual).render().contains("\"HATCHING_TICKS\":6"));
    assert_eq!(badge_preset(Mode::Casual).HATCHING_TICKS, 6);
}

#[test]
fn casual_values_match_the_preset() {
    let out = dump(Mode::Casual).render();
    assert!(out.contains("\"preset\":\"Casual\""));
    for k in Thresholds::FIELDS {
        if *k == "HATCHING_TICKS" {
            continue;
        }
        let v = Thresholds::CASUAL.value_of(k).unwrap();
        assert!(
            out.contains(&format!("\"{k}\":{v},")) || out.contains(&format!("\"{k}\":{v}}}")),
            "{k} should be {v}"
        );
    }
}
