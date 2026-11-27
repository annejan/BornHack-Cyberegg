//! The on-screen word list (`guide words`): every fixed word the badge draws,
//! with the group the guide files it under. The guide marks each word it
//! explains with `#w("...")`, and `build_guide.py` checks the marks against
//! this list both ways.
//!
//! `collect()` plays a few games' worth of menus against the game's global
//! state, so it runs once per process behind a `OnceLock` and every caller
//! reads that one result.

extern crate alloc;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use std::sync::OnceLock;

use super::json::J;
use crate::game::engine::causes::Cause;
use crate::game::engine::doses::Reason;
use crate::game::engine::hint::{
    Remedy, Subject, WORN_OUT_LINE, otherwise, remedies, subject_line,
};
use crate::game::engine::status::{Condition, StatusWord, Word};
use crate::game::engine::{GameState, PetKind, Phase};
use crate::game::pet_registry;
use crate::game::{battle_view, friends_view, realm_view, diary, diary_view, lifecycle, modal, pet_select, screen_words, stats_pages};

pub const GROUPS: [&str; 16] = [
    "status",
    "conditions",
    "causes",
    "reasons",
    "hint",
    "diary",
    "stats",
    "menu",
    "hard",
    "debug",
    "engine-info",
    "toasts",
    "screen",
    "pet-select",
    "naming",
    "social",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub text: String,
    pub group: &'static str,
}

/// Every word, sorted by (group, text), no (group, text) twice. Computed on
/// the first call; later calls return a copy of the same list.
pub fn collect() -> Vec<Entry> {
    static WORDS: OnceLock<Vec<Entry>> = OnceLock::new();
    WORDS.get_or_init(build).clone()
}

/// `{ "groups": [...], "words": [ { "text": .., "group": .. } ] }`.
pub fn dump() -> J {
    J::Obj(alloc::vec![
        (
            "groups".to_string(),
            J::Arr(GROUPS.iter().map(|g| J::Str(g.to_string())).collect()),
        ),
        (
            "words".to_string(),
            J::Arr(
                collect()
                    .into_iter()
                    .map(|e| {
                        J::Obj(alloc::vec![
                            ("text".to_string(), J::Str(e.text)),
                            ("group".to_string(), J::Str(e.group.to_string())),
                        ])
                    })
                    .collect(),
            ),
        ),
    ])
}

/// Put the repo's pets in the roster when nothing has (the badge reads `PETS.CFG`).
fn ensure_roster() {
    if pet_registry::roster().is_empty() {
        crate::sim_support::install_pets_from_repo_cfg();
    }
}

/// The menus as a fresh game with these money modes shows them.
fn menus(money: bool, hard: bool) -> Vec<(&'static str, String)> {
    lifecycle::start_new_game(PetKind::roster()[0], money, hard);
    modal::menu_words()
}

fn build() -> Vec<Entry> {
    ensure_roster();
    let mut out: Vec<Entry> = Vec::new();
    let mut add = |group: &'static str, text: &str| {
        out.push(Entry {
            text: text.to_string(),
            group,
        });
    };

    for w in Word::ALL {
        add("status", w.text());
    }
    for c in Condition::ALL {
        add("conditions", c.text());
    }
    add("conditions", "+<n>");
    for c in Cause::all() {
        add("causes", c.what());
        add("causes", c.why());
    }
    for k in 0..=u8::MAX {
        match Reason::unpack(k, 0) {
            Reason::Wait(_) => add("reasons", "wait <time>"),
            r => add("reasons", r.text().as_str()),
        }
    }

    // The pet's speech.
    let mut subjects: Vec<Subject> = Word::ALL.into_iter().map(Subject::Word).collect();
    subjects.extend([
        Subject::WornOut,
        Subject::Egg,
        Subject::Gone,
        Subject::Hibernating,
        Subject::Stiff,
    ]);
    let mut asks: Vec<Remedy> = Vec::new();
    for s in &subjects {
        let line = subject_line(*s);
        match *s {
            Subject::Word(w) if line == alloc::format!("I'm {}!", w.text()) => {
                add("hint", "I'm <word>!")
            }
            Subject::WornOut if line == WORN_OUT_LINE => {
                add("hint", "I'm <word>!");
                add("hint", WORN_OUT_LINE)
            }
            _ => add("hint", line),
        }
        for r in remedies(*s) {
            if !asks.contains(r) {
                asks.push(*r);
            }
        }
        let rest = otherwise(*s);
        if !rest.is_empty() {
            add("hint", rest);
        }
    }
    for r in asks {
        add("hint", r.ask());
    }

    for t in diary_view::words() {
        add("diary", &t);
    }
    add("diary", diary::fmt_age(0).as_str());
    for t in ["<n>m", "<n>h", "<n>d"] {
        add("diary", t);
    }

    let mut pet = GameState::new_egg(7, PetKind(0));
    pet.phase = Phase::Active;
    let stats = pet.stats(0);
    for page in [stats_pages::Page::Needs, stats_pages::Page::Body] {
        add("stats", stats_pages::title(page));
        for row in stats_pages::bars(page, &stats) {
            add("stats", row.label);
        }
    }

    // Three games: no money, money, Hard (US). A word only the last one shows
    // is a Hard mode word.
    let plain = menus(false, false);
    let money = menus(true, false);
    let hard = menus(true, true);
    for (g, t) in plain.iter().chain(&money) {
        add(g, t);
    }
    for (g, t) in &hard {
        let seen = plain
            .iter()
            .chain(&money)
            .any(|(g2, t2)| g2 == g && t2 == t);
        add(if *g == "menu" && !seen { "hard" } else { g }, t);
    }

    for w in StatusWord::ALL {
        add("engine-info", w.name());
    }
    for t in [
        "!",
        "zz",
        "nt",
        "m<mood>/<target>",
        "h<health>",
        "w<wait>",
        "w-",
    ] {
        add("engine-info", t);
    }

    for (g, t) in screen_words() {
        add(g, t);
    }
    for t in pet_select::words() {
        add("pet-select", t);
    }

    for t in crate::text_entry::words() {
        add("naming", t);
    }
    for t in friends_view::words()
        .into_iter()
        .chain(battle_view::words())
        .chain(realm_view::words())
    {
        add("social", t);
    }
    for t in crate::menu::main_menu_words()
        .into_iter()
        .chain(lifecycle::demo_menu_words())
    {
        add("menu", t);
    }

    out.sort_by(|a, b| (a.group, &a.text).cmp(&(b.group, &b.text)));
    out.dedup();
    out
}
