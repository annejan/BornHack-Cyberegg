//! The guide's screens and sprites (`guide shots`, `guide shot`, `guide sprites`).
//! Each shot runs in its own process: the game's state is global.
use std::path::PathBuf;
use std::process::{Command, Output};

use bornhack_aegg::sim_support::PNG_SCALE;

/// The screen's side in pixels.
const SCREEN: u32 = 152;

fn guide(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_guide"))
        .args(args)
        .output()
        .expect("run the guide bin")
}

fn out_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("guide_shots_{}_{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn text(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

#[test]
fn every_shot_reaches_its_state() {
    let listed = guide(&["shots"]);
    assert!(listed.status.success(), "guide shots failed");
    let names = text(&listed);
    let names: Vec<&str> = names.lines().collect();
    assert_eq!(names.len(), bornhack_aegg::guide::shots::SHOTS.len());
    assert!(names.len() >= 30, "only {} shots", names.len());
    let dir = out_dir("shots");
    let side = SCREEN * PNG_SCALE;
    for name in names {
        let o = guide(&["shot", name, "--out", dir.to_str().unwrap()]);
        let err = String::from_utf8_lossy(&o.stderr);
        assert!(o.status.success(), "shot {name} failed: {err}");
        let line = text(&o);
        let rest = line
            .trim()
            .strip_prefix(&format!("{name} {side}x{side} non_white="))
            .unwrap_or_else(|| panic!("shot {name}: unexpected output {line:?}"));
        let n: usize = rest.parse().expect("non_white is a number");
        assert!(n > 0, "shot {name} is blank");
        assert!(dir.join(format!("{name}.png")).exists(), "{name}.png missing");
    }
}

#[test]
fn a_failed_expectation_names_the_shot() {
    let o = guide(&["shot", "no-such-shot", "--out", out_dir("bad").to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&o.stderr).contains("no-such-shot"));
}

#[test]
fn screen_saves_the_screen_after_the_keys() {
    let dir = out_dir("screen");
    let side = SCREEN * PNG_SCALE;
    let mut pngs = Vec::new();
    for keys in ["", "RRRRR"] {
        let file = dir.join(format!("k{keys}.png"));
        let o = guide(&["screen", keys, "--out", file.to_str().unwrap()]);
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        assert!(text(&o).contains(&format!(" {side}x{side} non_white=")), "{}", text(&o));
        pngs.push(std::fs::read(&file).expect("png written"));
    }
    // The start screen and the watch (five screens to the right) differ.
    assert_ne!(pngs[0], pngs[1]);
}

#[test]
fn screen_rejects_an_unknown_key() {
    let file = out_dir("screen-bad").join("x.png");
    let o = guide(&["screen", "RQ", "--out", file.to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&o.stderr).contains("'Q'"));
    assert!(!file.exists());
}

#[test]
fn sprites_cover_the_roster_without_sponsors() {
    let dir = out_dir("sprites");
    let o = guide(&["sprites", "--out", dir.to_str().unwrap()]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let json = std::fs::read_to_string(dir.join("roster.json")).expect("roster.json");
    let cfg = include_str!("../assets/to-badge/PETS.CFG");
    let prefixes: Vec<u8> = cfg
        .lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .filter_map(|l| l.split('=').next()?.trim().parse().ok())
        .collect();
    assert!(prefixes.len() >= 5);
    for p in prefixes {
        let hex = format!("{p:02X}");
        if p == 3 || p == 4 {
            assert!(!json.contains(&format!("\"prefix\":\"{hex}\"")), "{hex} listed");
            continue;
        }
        assert!(json.contains(&format!("\"prefix\":\"{hex}\"")), "pet {hex} missing");
        assert!(
            dir.join(format!("{hex}-idle-0.png")).exists(),
            "{hex} has no idle frame"
        );
    }
    assert!(json.contains("\"hero\":{\"name\":\"Bob\""));
    assert!(dir.join("start.png").exists());
    for f in std::fs::read_dir(&dir).unwrap() {
        let n = f.unwrap().file_name().into_string().unwrap();
        assert!(!n.starts_with("03") && !n.starts_with("04"), "sponsor/icon art {n}");
    }
}

/// Run `guide shot NAME` and return its PNG bytes and non_white count.
fn shot_png(name: &str, tag: &str) -> (Vec<u8>, usize) {
    let dir = out_dir(tag);
    let o = guide(&["shot", name, "--out", dir.to_str().unwrap()]);
    assert!(o.status.success(), "shot {name}: {}", String::from_utf8_lossy(&o.stderr));
    let line = text(&o);
    let n: usize = line.trim().rsplit('=').next().unwrap().parse().expect("non_white");
    (std::fs::read(dir.join(format!("{name}.png"))).unwrap(), n)
}

#[test]
fn every_shot_is_drawn_at_noon_unless_it_says_night() {
    let (a, na) = shot_png("content", "noon_a");
    let (b, nb) = shot_png("content", "noon_b");
    assert_eq!(na, nb);
    assert!(a == b, "two runs of the same shot differ: the host clock leaks in");
    let (_, night) = shot_png("night", "night");
    assert!(night >= na + 3_000, "night {night} vs content {na}: no dithered sky");
    let (_, day) = shot_png("day", "day");
    assert_eq!(day, na, "day is a copy of content at noon");
}
