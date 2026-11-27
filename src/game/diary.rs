//! The diary: the last few cause events, newest first. RAM only, never
//! saved (a reboot or a new pet starts it empty).

use core::fmt::Write;

use heapless::{Deque, String};

use super::engine::causes::Cause;

/// How many entries the diary keeps.
pub const DIARY_LEN: usize = 6;

/// One logged cause and the pet age it happened at.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Entry {
    pub cause: Cause,
    /// Pet age in minutes.
    pub at_min: u32,
}

#[derive(Clone, Default)]
pub struct Diary(Deque<Entry, DIARY_LEN>);

impl Diary {
    pub const fn new() -> Self {
        Self(Deque::new())
    }

    /// Log a cause; when full the oldest entry is dropped.
    pub fn push(&mut self, cause: Cause, at_min: u32) {
        if self.0.is_full() {
            self.0.pop_back();
        }
        let _ = self.0.push_front(Entry { cause, at_min });
    }

    pub fn newest_first(&self) -> impl Iterator<Item = &Entry> {
        self.0.iter()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn clear(&mut self) {
        self.0.clear();
    }
}

/// An age at a glance: "now", "59m", "23h", "9d", capped at "99d".
pub fn fmt_age(min: u32) -> String<4> {
    let mut out = String::new();
    let _ = match min {
        0 => write!(out, "now"),
        1..60 => write!(out, "{min}m"),
        60..1440 => write!(out, "{}h", min / 60),
        _ => write!(out, "{}d", (min / 1440).min(99)),
    };
    out
}

/// The two lines of a diary row: `"{age:>3} {what}"` and `"    {why}"`.
pub fn entry_lines(e: &Entry, now_min: u32) -> (String<21>, String<21>) {
    let mut first = String::new();
    let mut second = String::new();
    let age = fmt_age(now_min.saturating_sub(e.at_min));
    let _ = write!(first, "{:>3} {}", age, e.cause.what());
    let _ = write!(second, "    {}", e.cause.why());
    (first, second)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::engine::causes::{Cause, StrainWhy};

    #[test]
    fn ages_read_at_a_glance() {
        for (min, want) in [
            (0, "now"),
            (59, "59m"),
            (60, "1h"),
            (1439, "23h"),
            (1440, "1d"),
            (999_999, "99d"),
        ] {
            assert_eq!(fmt_age(min).as_str(), want, "{min} min");
        }
    }

    #[test]
    fn the_diary_keeps_the_last_six_newest_first() {
        let mut d = Diary::default();
        let seven = [
            Cause::FoodComa,
            Cause::Groggy,
            Cause::Grumpy,
            Cause::DryMouth,
            Cause::Sloshing,
            Cause::WornOut,
            Cause::Strained(StrainWhy::Tired),
        ];
        for (i, c) in seven.iter().enumerate() {
            d.push(*c, i as u32);
        }
        assert_eq!(d.len(), DIARY_LEN);
        let got: std::vec::Vec<Cause> = d.newest_first().map(|e| e.cause).collect();
        assert_eq!(
            got,
            [seven[6], seven[5], seven[4], seven[3], seven[2], seven[1]]
        );
        assert_eq!(d.newest_first().next().unwrap().at_min, 6);
    }

    #[test]
    fn ages_are_right_aligned_in_three_columns() {
        let e = Entry { cause: Cause::Groggy, at_min: 0 };
        assert_eq!(entry_lines(&e, 5).0.as_str(), " 5m groggy");
        assert_eq!(entry_lines(&e, 0).0.as_str(), "now groggy");
        assert_eq!(entry_lines(&e, 59).0.as_str(), "59m groggy");
    }

    #[test]
    fn diary_lines_fit_the_screen() {
        for cause in Cause::all() {
            let e = Entry {
                cause,
                at_min: 99 * 1440,
            };
            let (a, b) = entry_lines(&e, 999_999);
            assert!(!a.is_empty() && !b.is_empty(), "{cause:?}");
            assert!(a.len() <= 21 && b.len() <= 21, "{cause:?}: {a} / {b}");
            assert!(a.ends_with(cause.what()), "{a}");
            assert!(b.ends_with(cause.why()), "{b}");
        }
    }
}
