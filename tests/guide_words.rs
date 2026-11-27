//! The on-screen word list the guide marks and indexes (`guide words`).
//! `collect()` caches its result, so every test reads one run of it.
use bornhack_aegg::guide::words::{Entry, GROUPS, collect, dump};

fn has(group: &str, text: &str) -> bool {
    collect().iter().any(|e| e.group == group && e.text == text)
}

fn any_group(text: &str) -> bool {
    collect().iter().any(|e| e.text == text)
}

#[test]
fn plan_2_words_are_listed() {
    for t in [
        "peckish",
        "no insulin",
        "well rested",
        "worn out",
        "+<n>",
        "food coma",
        "ate too much",
        "wait <time>",
        "too tired",
        "I'm <word>!",
        "Zzz...",
        "Please feed me.",
        "Diary 1/2",
        "Nothing yet",
        "Needs 1/2",
        "Clean",
        "View stats",
        "Frikandel spec",
        "Frikandel speciaal",
        "risk: <what>",
        "Engine info",
        "Hung over",
        "cheat applied",
        "Give rehab soon",
        "Hard mode (US)",
    ] {
        assert!(any_group(t), "missing word: {t}");
    }
}

#[test]
fn hamburger_is_hard_mode_only() {
    assert!(has("hard", "Hamburger"));
    assert!(!has("menu", "Hamburger"));
}

#[test]
fn groups_are_known_and_texts_non_empty() {
    let all = collect();
    assert!(!all.is_empty());
    for e in &all {
        assert!(GROUPS.contains(&e.group), "unknown group {}", e.group);
        assert!(!e.text.is_empty(), "empty text in {}", e.group);
    }
    for g in GROUPS {
        assert!(all.iter().any(|e| e.group == g), "group {g} is empty");
    }
}

#[test]
fn sorted_without_duplicates() {
    let all = collect();
    let keys: Vec<(&str, &str)> = all.iter().map(|e| (e.group, e.text.as_str())).collect();
    let mut sorted = keys.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(keys, sorted);
}

#[test]
fn dump_lists_groups_and_words() {
    let out = dump().render();
    assert!(out.starts_with("{\"groups\":[\"status\","));
    assert!(out.contains("{\"text\":\"peckish\",\"group\":\"status\"}"));
}

/// Every string literal at a place the game draws text is a listed word.
/// The files are the screens' sources; the scan is a check on the list, so a
/// new on-screen string that nobody lists fails here.
#[test]
fn every_drawn_literal_is_listed() {
    // (file, source, label tables too?, exempt literals). Only the two files
    // whose text lives in label tables get the table scan.
    const FILES: [(&str, &str, bool, &[&str]); 9] = [
        ("mod.rs", include_str!("../src/game/mod.rs"), false, &[]),
        (
            "modal.rs",
            include_str!("../src/game/modal.rs"),
            true,
            &["?"],
        ),
        (
            "pet_select.rs",
            include_str!("../src/game/pet_select.rs"),
            false,
            &[],
        ),
        (
            "diary_view.rs",
            include_str!("../src/game/diary_view.rs"),
            false,
            &[],
        ),
        (
            "text_entry.rs",
            include_str!("../src/text_entry.rs"),
            false,
            // The text cursor is a glyph, not a word.
            &["_"],
        ),
        (
            "battle_view.rs",
            include_str!("../src/game/battle_view.rs"),
            false,
            &[],
        ),
        (
            "friends_view.rs",
            include_str!("../src/game/friends_view.rs"),
            false,
            &[],
        ),
        (
            "realm_view.rs",
            include_str!("../src/game/realm_view.rs"),
            false,
            &[],
        ),
        (
            "stats_pages.rs",
            include_str!("../src/game/stats_pages.rs"),
            true,
            &[],
        ),
    ];
    let texts: Vec<String> = collect().iter().map(|e: &Entry| e.text.clone()).collect();
    for (name, src, tables, exempt) in FILES {
        let found = scan(src, tables);
        assert!(!found.is_empty(), "{name}: the scan found no literal");
        for lit in found {
            assert!(
                exempt.contains(&lit.as_str()) || texts.contains(&lit),
                "{name}: drawn literal {lit:?} is not in the word list"
            );
        }
    }
}

/// String literals in the call sites that draw text, in the `const` tables the
/// screens draw from, and in the label tables (`=> "..."`, `label: "..."`,
/// `band("...`, `floor("...`, a debug row's `("...", DebugRow`). The test
/// modules are cut off first.
fn scan(src: &str, tables: bool) -> Vec<String> {
    let src = before_tests(src);
    let mut out = Vec::new();
    for pat in [
        "Text::with_text_style(",
        "Text::new(",
        "draw_text(",
        "draw_alert(",
        "draw_centered_message(",
    ] {
        for (at, _) in src.match_indices(pat) {
            let call = balanced(&src[at + pat.len()..]);
            literals(call, &mut out);
        }
    }
    for (at, _) in src.match_indices("const ") {
        // `const NAME: <type> = <value>;`: the type may hold a `;` (`[&str; 2]`).
        let stmt = &src[at..];
        let Some(eq) = stmt.find(" = ") else { continue };
        let (ty, value) = stmt.split_at(eq);
        if ty.contains("&str") {
            literals(&value[..value.find(';').unwrap()], &mut out);
        }
    }
    for line in src.lines().filter(|_| tables) {
        let t = line.trim_start();
        let tails = [
            ("=> \"", 3),
            ("label: \"", 7),
            ("band(\"", 5),
            ("floor(\"", 6),
        ];
        for (p, skip) in tails {
            for (at, _) in line.match_indices(p) {
                literals(&line[at + skip..], &mut out);
                break;
            }
        }
        if t.starts_with("(\"") && t.contains(", DebugRow") {
            literals(t, &mut out);
        }
    }
    out.retain(|s| !s.is_empty());
    out.sort();
    out.dedup();
    out
}

/// The source up to its first inline test module (`#[cfg(test)]` then
/// `mod x {`).
fn before_tests(src: &str) -> &str {
    let lines: Vec<&str> = src.lines().collect();
    for i in 1..lines.len() {
        if lines[i - 1].trim() == "#[cfg(test)]"
            && lines[i].starts_with("mod ")
            && lines[i].trim_end().ends_with('{')
        {
            let end = lines[..i - 1].iter().map(|l| l.len() + 1).sum::<usize>();
            return &src[..end];
        }
    }
    src
}

/// The text of a call up to its closing parenthesis (the opening one is
/// already consumed).
fn balanced(s: &str) -> &str {
    let (mut depth, mut in_str, mut prev) = (1i32, false, ' ');
    for (i, c) in s.char_indices() {
        if in_str {
            if c == '"' && prev != '\\' {
                in_str = false;
            }
        } else {
            match c {
                '"' => in_str = true,
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        return &s[..i];
                    }
                }
                _ => {}
            }
        }
        prev = c;
    }
    s
}

/// Push every string literal of `s`.
fn literals(s: &str, out: &mut Vec<String>) {
    let mut chars = s.char_indices();
    let mut in_str = false;
    let mut cur = String::new();
    let mut prev = ' ';
    while let Some((_, c)) = chars.next() {
        if in_str {
            if c == '"' && prev != '\\' {
                in_str = false;
                out.push(std::mem::take(&mut cur));
            } else {
                cur.push(c);
            }
        } else if c == '"' {
            in_str = true;
        }
        prev = c;
    }
}
