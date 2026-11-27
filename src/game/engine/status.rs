//! What the player reads under the pet.
//!
//! [`status`] is the worst problem as one [`StatusWord`] (the largest term of
//! the mood penalty, [`regime::penalty_terms`]; under 3 points nothing is
//! shown) plus the danger flag. [`status_line`] widens that to up to two
//! graded [`Word`]s: the danger word, the worst term, a prompt and a second
//! term, else an info word. [`conditions`] lists the lasting states for the
//! stats view. Terms are ranked on their exact values
//! ([`regime::penalty_terms_x2`]); every line read here is a fixed edge, a
//! solver signature line or a timer bound, so the wake stays exact.

use heapless::String;

use super::GameState;
use super::needs::{
    self, DIRT_DANGER_PCT, EXHAUSTED_PCT, HEALTH_DANGER_PCT, HUNGRY_PCT, NEED_DANGER_LO_PCT, P,
    SORE_PCT, TIPSY_PCT,
};
use super::regime::{EXACT_SCALE, PENALTY_TERMS, penalty_terms_x2};

/// Largest term must beat this to be named.
const CONTENT_UNDER: i32 = 3 * P;
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "embassy-core", derive(defmt::Format))]
#[repr(u8)]
pub enum StatusWord {
    Content,
    Hungry,
    Stuffed,
    Thirsty,
    Waterlogged,
    Tired,
    Bored,
    Wired,
    Sloppy,
    Grubby,
    Overweight,
    Underweight,
    Unfit,
    HungOver,
    Injured,
    Groggy,
    Poorly,
}

impl StatusWord {
    /// Every word, for the guide's word list (host-only).
    #[cfg(any(test, feature = "simulator"))]
    pub const ALL: [StatusWord; 17] = [
        StatusWord::Content,
        StatusWord::Hungry,
        StatusWord::Stuffed,
        StatusWord::Thirsty,
        StatusWord::Waterlogged,
        StatusWord::Tired,
        StatusWord::Bored,
        StatusWord::Wired,
        StatusWord::Sloppy,
        StatusWord::Grubby,
        StatusWord::Overweight,
        StatusWord::Underweight,
        StatusWord::Unfit,
        StatusWord::HungOver,
        StatusWord::Injured,
        StatusWord::Groggy,
        StatusWord::Poorly,
    ];
}

/// The words in [`regime::penalty_terms`] order.
const WORDS: [StatusWord; PENALTY_TERMS] = [
    StatusWord::Hungry,
    StatusWord::Stuffed,
    StatusWord::Thirsty,
    StatusWord::Waterlogged,
    StatusWord::Tired,
    StatusWord::Bored,
    StatusWord::Wired,
    StatusWord::Sloppy,
    StatusWord::Grubby,
    StatusWord::Overweight,
    StatusWord::Underweight,
    StatusWord::Unfit,
    StatusWord::HungOver,
    StatusWord::Injured,
    StatusWord::Groggy,
    StatusWord::Poorly,
];

// The danger lines. `danger_word` and `graded` both read these, so a danger
// word and its graded word can never disagree.
fn starving(s: &GameState) -> bool {
    (s.fullness as i32) < NEED_DANGER_LO_PCT as i32 * P
}
fn parched(s: &GameState) -> bool {
    (s.hydration as i32) < NEED_DANGER_LO_PCT as i32 * P
}
fn wasted(s: &GameState) -> bool {
    s.booze as i32 > needs::booze_danger()
}
fn filthy(s: &GameState) -> bool {
    s.dirt as i32 > DIRT_DANGER_PCT as i32 * P
}
/// Health under the danger line.
pub fn poorly(s: &GameState) -> bool {
    (s.health as i32) < HEALTH_DANGER_PCT as i32 * P
}

/// The first danger word, in the order fullness, hydration, booze, dirt,
/// health. `status` and the status line both read it.
pub fn danger_word(s: &GameState) -> Option<Word> {
    if starving(s) {
        Some(Word::Starving)
    } else if parched(s) {
        Some(Word::Parched)
    } else if wasted(s) {
        Some(Word::Wasted)
    } else if filthy(s) {
        Some(Word::Filthy)
    } else if poorly(s) {
        Some(Word::Poorly)
    } else {
        None
    }
}

/// Index of the term that is largest on exact values, skipping `skip`; an
/// exact tie goes to the earlier term. It must also beat `above` once
/// truncated (the on-screen threshold lines are in truncated points).
fn largest_term(exact: &[i32; PENALTY_TERMS], above: i32, skip: Option<usize>) -> Option<usize> {
    let mut best: Option<usize> = None;
    for (i, &t) in exact.iter().enumerate() {
        if Some(i) != skip && best.is_none_or(|b| t > exact[b]) {
            best = Some(i);
        }
    }
    best.filter(|&i| exact[i] / EXACT_SCALE > above)
}

/// The status word and whether the pet is in danger (a need or the health
/// is past its danger line). A tie goes to the earlier word.
pub fn status(s: &GameState) -> (StatusWord, bool) {
    let word = largest_term(&penalty_terms_x2(s), CONTENT_UNDER, None)
        .map_or(StatusWord::Content, |i| WORDS[i]);
    (word, danger_word(s).is_some())
}

/// A second word must beat this penalty to be shown beside the first.
pub const SECOND_WORD_UNDER: i32 = 10 * P;

/// Widest line of the status line, in columns (7 px font on a 152 px panel).
const LINE_COLS: usize = 21;

/// Everything the status line can say: graded problems, prompts and info.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "embassy-core", derive(defmt::Format))]
#[repr(u8)]
pub enum Word {
    Peckish,
    Hungry,
    Starving,
    Stuffed,
    Thirsty,
    Parched,
    Waterlogged,
    Sleepy,
    Exhausted,
    Bored,
    Wired,
    Tipsy,
    Sloppy,
    Wasted,
    Grubby,
    Filthy,
    Overweight,
    Underweight,
    Unfit,
    HungOver,
    Sore,
    Injured,
    Groggy,
    Poorly,
    NoInsulin,
    NeedsRehab,
    Asleep,
    WellRested,
    Content,
}

impl Word {
    pub const ALL: [Word; 29] = [
        Word::Peckish,
        Word::Hungry,
        Word::Starving,
        Word::Stuffed,
        Word::Thirsty,
        Word::Parched,
        Word::Waterlogged,
        Word::Sleepy,
        Word::Exhausted,
        Word::Bored,
        Word::Wired,
        Word::Tipsy,
        Word::Sloppy,
        Word::Wasted,
        Word::Grubby,
        Word::Filthy,
        Word::Overweight,
        Word::Underweight,
        Word::Unfit,
        Word::HungOver,
        Word::Sore,
        Word::Injured,
        Word::Groggy,
        Word::Poorly,
        Word::NoInsulin,
        Word::NeedsRehab,
        Word::Asleep,
        Word::WellRested,
        Word::Content,
    ];

    /// Whether this word is worth a buzz: the danger words and the graded
    /// problems that are a real alarm. The band-edge grades (peckish, sleepy)
    /// and the "miserable" words stay quiet (spec 5.7).
    pub fn alerts(self) -> bool {
        matches!(
            self,
            Word::Hungry
                | Word::Starving
                | Word::Thirsty
                | Word::Parched
                | Word::Exhausted
                | Word::Wasted
                | Word::Filthy
                | Word::HungOver
                | Word::Injured
                | Word::Groggy
                | Word::Poorly
        )
    }

    /// Whether `info_word` can return this word (see [`INFO_WORDS`]).
    pub fn is_info(self) -> bool {
        INFO_WORDS.contains(&self)
    }

    pub fn text(self) -> &'static str {
        match self {
            Word::Peckish => "peckish",
            Word::Hungry => "hungry",
            Word::Starving => "starving",
            Word::Stuffed => "stuffed",
            Word::Thirsty => "thirsty",
            Word::Parched => "parched",
            Word::Waterlogged => "waterlogged",
            Word::Sleepy => "sleepy",
            Word::Exhausted => "exhausted",
            Word::Bored => "bored",
            Word::Wired => "wired",
            Word::Tipsy => "tipsy",
            Word::Sloppy => "sloppy",
            Word::Wasted => "wasted",
            Word::Grubby => "grubby",
            Word::Filthy => "filthy",
            Word::Overweight => "overweight",
            Word::Underweight => "underweight",
            Word::Unfit => "unfit",
            Word::HungOver => "hung over",
            Word::Sore => "sore",
            Word::Injured => "injured",
            Word::Groggy => "groggy",
            Word::Poorly => "poorly",
            Word::NoInsulin => "no insulin",
            Word::NeedsRehab => "needs rehab",
            Word::Asleep => "asleep",
            Word::WellRested => "well rested",
            Word::Content => "content",
        }
    }
}

/// The graded word of a penalty term: the same problem, worse when it
/// crosses a fixed edge.
fn graded(s: &GameState, w: StatusWord) -> Word {
    match w {
        StatusWord::Hungry if starving(s) => Word::Starving,
        StatusWord::Hungry if (s.fullness as i32) < HUNGRY_PCT as i32 * P => Word::Hungry,
        StatusWord::Hungry => Word::Peckish,
        StatusWord::Thirsty if parched(s) => Word::Parched,
        StatusWord::Thirsty => Word::Thirsty,
        StatusWord::Tired if (s.energy as i32) < EXHAUSTED_PCT as i32 * P => Word::Exhausted,
        StatusWord::Tired => Word::Sleepy,
        StatusWord::Sloppy if wasted(s) => Word::Wasted,
        StatusWord::Sloppy => Word::Sloppy,
        StatusWord::Grubby if filthy(s) => Word::Filthy,
        StatusWord::Grubby => Word::Grubby,
        StatusWord::Stuffed => Word::Stuffed,
        StatusWord::Waterlogged => Word::Waterlogged,
        StatusWord::Bored => Word::Bored,
        StatusWord::Wired => Word::Wired,
        StatusWord::Overweight => Word::Overweight,
        StatusWord::Underweight => Word::Underweight,
        StatusWord::Unfit => Word::Unfit,
        StatusWord::HungOver => Word::HungOver,
        StatusWord::Injured => Word::Injured,
        StatusWord::Groggy => Word::Groggy,
        StatusWord::Poorly => Word::Poorly,
        StatusWord::Content => Word::Content,
    }
}

/// The one or two words under the pet, and whether the pet is in danger.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "embassy-core", derive(defmt::Format))]
pub struct StatusLine {
    pub first: Word,
    pub second: Option<Word>,
    pub danger: bool,
}

impl StatusLine {
    /// Whether the line is worth a buzz: in danger, or the first word is a
    /// real alarm. The one rule the severity buzzer reads.
    pub fn alerts(&self) -> bool {
        self.danger || self.first.alerts()
    }

    /// "a, b" when it fits one line, else "a\nb".
    pub fn text(&self) -> String<32> {
        let mut out = String::new();
        let a = self.first.text();
        let _ = out.push_str(a);
        if let Some(b) = self.second {
            let b = b.text();
            let sep = if a.len() + 2 + b.len() <= LINE_COLS {
                ", "
            } else {
                "\n"
            };
            let _ = out.push_str(sep);
            let _ = out.push_str(b);
        }
        out
    }
}

/// The status line: the danger word, the worst graded problem, a prompt, a
/// second problem (when big enough), else an info word. Every threshold it
/// reads is a fixed edge, a `solver::sig` line or a timer bound, so the wake
/// stays exact.
pub fn status_line(s: &GameState) -> StatusLine {
    let exact = penalty_terms_x2(s);
    let top = largest_term(&exact, CONTENT_UNDER, None);
    let next = top.and_then(|t| largest_term(&exact, SECOND_WORD_UNDER, Some(t)));
    let prompt = if s.is_diabetic_unmedicated() {
        Some(Word::NoInsulin)
    } else if s.is_alcoholic_untreated() {
        Some(Word::NeedsRehab)
    } else {
        None
    };
    let danger = danger_word(s);
    let candidates = [
        danger,
        top.map(|i| graded(s, WORDS[i])),
        prompt,
        next.map(|i| graded(s, WORDS[i])),
    ];
    let mut first = None;
    let mut second = None;
    for w in candidates.into_iter().flatten() {
        if first == Some(w) {
            continue;
        }
        if first.is_none() {
            first = Some(w);
        } else if second.is_none() {
            second = Some(w);
        }
    }
    StatusLine {
        first: first.unwrap_or_else(|| info_word(s)),
        second,
        // A demo pet shows no red: the word stays, the alarm goes.
        danger: danger.is_some() && !s.demo.on,
    }
}

/// Every word `info_word` can return: what the line says when no penalty term
/// is large enough to be named. The one list; `Word::is_info` reads it.
pub const INFO_WORDS: [Word; 5] = [
    Word::Asleep,
    Word::Sore,
    Word::Tipsy,
    Word::WellRested,
    Word::Content,
];

/// What to say when nothing is out of line.
fn info_word(s: &GameState) -> Word {
    if s.is_sleeping {
        Word::Asleep
    } else if is_sore(s) {
        Word::Sore
    } else if s.booze as i32 >= TIPSY_PCT as i32 * P {
        Word::Tipsy
    } else if s.rested_min > 0 {
        Word::WellRested
    } else {
        Word::Content
    }
}

/// Strain past the sore line, unless an injury already says it.
fn is_sore(s: &GameState) -> bool {
    s.strain as i32 > SORE_PCT as i32 * P && s.injured_min == 0
}

/// Lasting states shown on the stats view, most pressing first.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "embassy-core", derive(defmt::Format))]
#[repr(u8)]
pub enum Condition {
    Injured,
    Sore,
    HungOver,
    Groggy,
    WornOut,
    Diabetic,
    Dependent,
    InRehab,
    WellRested,
}

impl Condition {
    pub const ALL: [Condition; 9] = [
        Condition::Injured,
        Condition::Sore,
        Condition::HungOver,
        Condition::Groggy,
        Condition::WornOut,
        Condition::Diabetic,
        Condition::Dependent,
        Condition::InRehab,
        Condition::WellRested,
    ];

    pub fn text(self) -> &'static str {
        match self {
            Condition::Injured => "injured",
            Condition::Sore => "sore",
            Condition::HungOver => "hung over",
            Condition::Groggy => "groggy",
            Condition::WornOut => "worn out",
            Condition::Diabetic => "diabetic",
            Condition::Dependent => "dependent",
            Condition::InRehab => "in rehab",
            Condition::WellRested => "well rested",
        }
    }
}

/// One bit per [`Condition`].
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
#[cfg_attr(feature = "embassy-core", derive(defmt::Format))]
pub struct Conditions(pub u16);

/// Most conditions named on one line before the rest is counted.
const CONDITIONS_NAMED: usize = 2;

impl Conditions {
    pub fn has(self, c: Condition) -> bool {
        self.0 & (1 << c as u8) != 0
    }

    pub fn iter(self) -> impl Iterator<Item = Condition> {
        Condition::ALL.into_iter().filter(move |&c| self.has(c))
    }

    /// The most pressing conditions that fit `cols`, then " +n" for the rest.
    pub fn line(self, cols: usize) -> String<24> {
        let n = self.iter().count();
        for k in (1..=n.min(CONDITIONS_NAMED)).rev() {
            let mut out: String<24> = String::new();
            for (i, c) in self.iter().take(k).enumerate() {
                if i > 0 {
                    let _ = out.push_str(", ");
                }
                let _ = out.push_str(c.text());
            }
            if n > k {
                let mut rest: String<8> = String::new();
                let _ = core::fmt::write(&mut rest, format_args!(" +{}", n - k));
                let _ = out.push_str(&rest);
            }
            if out.len() <= cols {
                return out;
            }
        }
        String::new()
    }
}

/// The lasting conditions of the pet right now.
pub fn conditions(s: &GameState) -> Conditions {
    let bits = [
        (Condition::Injured, s.injured_min > 0),
        (Condition::Sore, is_sore(s)),
        (Condition::HungOver, s.hangover_min > 0),
        (Condition::Groggy, s.groggy_min > 0),
        (Condition::WornOut, s.deprived),
        (Condition::Diabetic, s.diabetic),
        (Condition::Dependent, s.dependent && s.rehab_min == 0),
        (Condition::InRehab, s.rehab_min > 0),
        (Condition::WellRested, s.rested_min > 0),
    ];
    Conditions(
        bits.into_iter()
            .fold(0, |b, (c, on)| b | (on as u16) << c as u8),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::engine::PetKind;

    #[test]
    fn info_words_are_exactly_what_info_word_returns() {
        let mut seen = [false; INFO_WORDS.len()];
        for bits in 0u32..32 {
            let mut s = GameState::new_egg(7, PetKind(0));
            s.is_sleeping = bits & 1 != 0;
            s.strain = if bits & 2 != 0 { 90 * P as u16 } else { 0 };
            s.booze = if bits & 4 != 0 { 60 * P as u16 } else { 0 };
            s.rested_min = if bits & 8 != 0 { 100 } else { 0 };
            s.injured_min = if bits & 16 != 0 { 100 } else { 0 };
            let w = info_word(&s);
            assert!(INFO_WORDS.contains(&w), "{w:?} is not in INFO_WORDS");
            assert!(w.is_info());
            seen[INFO_WORDS.iter().position(|&i| i == w).unwrap()] = true;
        }
        assert!(seen.iter().all(|&b| b), "INFO_WORDS lists a word info_word never says: {seen:?}");
        for w in Word::ALL {
            assert_eq!(w.is_info(), INFO_WORDS.contains(&w));
        }
    }
}
