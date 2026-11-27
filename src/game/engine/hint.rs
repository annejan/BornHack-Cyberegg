//! What the pet says it needs (the Stats menu's Hint row).
//!
//! One table, derived from the status line: the subject is the line's first
//! word (danger first), with the egg, a gone pet and hibernation in front and
//! "worn out" ahead of an info word while awake. The ask is the first remedy the
//! engine would accept ([`GameState::remedy_reason`]) and does not warn
//! ([`GameState::remedy_warning`]), else the first one it would accept, so
//! the pet never asks for something the menu would refuse; when every remedy
//! is refused it says why instead.

use super::GameState;
use super::Phase;
use super::doses::Reason;
use super::needs;
use super::thresholds::CEILING_FIT_STEP3_PCT;
use super::status::{Word, status_line};

/// Something the player can do for the pet, as the hint names it.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[cfg_attr(feature = "embassy-core", derive(defmt::Format))]
pub enum Remedy {
    Feed,
    Water,
    Sleep,
    Play,
    Game,
    Relax,
    Wash,
    Exercise,
    Medicine,
    Insulin,
    Rehab,
    Defrost,
}

/// What the pet is talking about.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "embassy-core", derive(defmt::Format))]
pub enum Subject {
    Word(Word),
    /// Sleep-deprived (a condition, not a status word).
    WornOut,
    Egg,
    Gone,
    Hibernating,
    /// A demo pet with nothing to say that is not fit: it asks for exercise,
    /// which it never takes by itself.
    Stiff,
}

/// The second line.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "embassy-core", derive(defmt::Format))]
pub enum Ask {
    /// The pet asks for this; the engine would accept it.
    Do(Remedy),
    /// Every remedy is refused; this is the first one's reason.
    Refused(Reason),
    /// Nothing to ask for: a fixed line.
    Say(&'static str),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "embassy-core", derive(defmt::Format))]
pub struct Hint {
    pub subject: Subject,
    pub ask: Ask,
}

/// Widest line of the popover body, in columns.
const COLS: usize = 18;

/// The worn-out pet's line 1 (the guide lists it as a template word).
pub const WORN_OUT_LINE: &str = "I'm worn out!";

/// Line 1: the pet names its state.
pub fn subject_line(subject: Subject) -> &'static str {
    match subject {
        Subject::Word(w) => match w {
            Word::Peckish => "I'm peckish!",
            Word::Hungry => "I'm hungry!",
            Word::Starving => "I'm starving!",
            Word::Stuffed => "I'm stuffed!",
            Word::Thirsty => "I'm thirsty!",
            Word::Parched => "I'm parched!",
            Word::Waterlogged => "I'm waterlogged!",
            Word::Sleepy => "I'm sleepy!",
            Word::Exhausted => "I'm exhausted!",
            Word::Bored => "I'm bored!",
            Word::Wired => "I'm wired!",
            Word::Tipsy => "I'm tipsy!",
            Word::Sloppy => "I'm sloppy!",
            Word::Wasted => "I'm wasted!",
            Word::Grubby => "I'm grubby!",
            Word::Filthy => "I'm filthy!",
            Word::Overweight => "I'm overweight!",
            Word::Underweight => "I'm underweight!",
            Word::Unfit => "I'm unfit!",
            Word::HungOver => "I'm hung over!",
            Word::Sore => "I'm sore!",
            Word::Injured => "I'm injured!",
            Word::Groggy => "I'm groggy!",
            Word::Poorly => "I'm poorly!",
            Word::NoInsulin => "I'm diabetic!",
            Word::NeedsRehab => "I'm dependent!",
            Word::Asleep => "Zzz...",
            Word::WellRested => "I feel great!",
            Word::Content => "I'm happy!",
        },
        Subject::WornOut => WORN_OUT_LINE,
        Subject::Egg => "*wobble*",
        Subject::Gone => "...",
        Subject::Hibernating => "Brrr, frozen!",
        Subject::Stiff => "I feel stiff.",
    }
}

/// What helps, in preference order. The first one the engine accepts is asked
/// for.
pub fn remedies(subject: Subject) -> &'static [Remedy] {
    use Remedy::*;
    match subject {
        Subject::Word(w) => match w {
            Word::Peckish | Word::Hungry | Word::Starving | Word::Underweight => &[Feed],
            Word::Thirsty | Word::Parched => &[Water],
            // Water lowers booze; so does sleep (and it is open when the pet
            // is too full of water to take more).
            Word::Sloppy | Word::Wasted => &[Water, Sleep],
            Word::Sleepy | Word::Exhausted => &[Sleep],
            Word::Bored => &[Play, Game],
            Word::Wired | Word::Sore => &[Relax],
            Word::Grubby | Word::Filthy => &[Wash],
            Word::Overweight | Word::Unfit => &[Exercise],
            Word::HungOver => &[Water, Sleep],
            Word::Groggy => &[Water, Play],
            Word::Injured | Word::Poorly => &[Medicine],
            Word::NoInsulin => &[Insulin],
            Word::NeedsRehab => &[Rehab],
            Word::Stuffed
            | Word::Waterlogged
            | Word::Tipsy
            | Word::Asleep
            | Word::WellRested
            | Word::Content => &[],
        },
        Subject::WornOut => &[Sleep],
        Subject::Hibernating => &[Defrost],
        Subject::Stiff => &[Exercise],
        Subject::Egg | Subject::Gone => &[],
    }
}

/// Line 2 for a subject with no remedy (empty for one that has).
pub fn otherwise(subject: Subject) -> &'static str {
    match subject {
        Subject::Word(Word::Stuffed) => "Let me digest.",
        Subject::Word(Word::Waterlogged) => "No more drinks.",
        Subject::Word(Word::Tipsy) => "Hic! Thanks!",
        Subject::Word(Word::Asleep) => "Let me sleep.",
        Subject::Word(Word::WellRested | Word::Content) => "Thanks!",
        Subject::Egg => "I'm hatching!",
        Subject::Gone => "Your pet left.",
        _ => "",
    }
}

impl Remedy {
    /// Line 2 when the pet asks for it.
    pub fn ask(self) -> &'static str {
        match self {
            Remedy::Feed => "Please feed me.",
            Remedy::Water => "Water, please.",
            Remedy::Sleep => "Put me to bed.",
            Remedy::Play => "Play with me!",
            Remedy::Game => "Play a game!",
            Remedy::Relax => "Let me relax.",
            Remedy::Wash => "Wash me, please.",
            Remedy::Exercise => "Let's work out!",
            Remedy::Medicine => "I need medicine.",
            Remedy::Insulin => "I need insulin.",
            Remedy::Rehab => "I need rehab.",
            Remedy::Defrost => "Defrost me!",
        }
    }
}

impl Hint {
    /// The two lines of the speech, each within the popover body.
    pub fn lines(&self) -> (heapless::String<COLS>, heapless::String<COLS>) {
        let mut one = heapless::String::new();
        let _ = one.push_str(subject_line(self.subject));
        let mut two = heapless::String::new();
        let _ = match self.ask {
            Ask::Do(r) => two.push_str(r.ask()),
            Ask::Say(line) => two.push_str(line),
            Ask::Refused(why) => {
                let _ = two.push_str("but ");
                two.push_str(why.text().as_str())
            }
        };
        (one, two)
    }
}

/// What the pet talks about right now.
fn subject(s: &GameState) -> Subject {
    if s.hibernating {
        return Subject::Hibernating;
    }
    match s.phase {
        Phase::Hatching => return Subject::Egg,
        Phase::Gone => return Subject::Gone,
        Phase::Active | Phase::Leaving => {}
    }
    let first = status_line(s).first;
    // Worn out speaks over any info word, except an asleep pet's "Zzz...".
    if s.deprived && !s.is_sleeping && first.is_info() {
        Subject::WornOut
    } else if s.demo.on
        && !s.is_sleeping
        && first.is_info()
        && (s.fitness as i32) < CEILING_FIT_STEP3_PCT() as i32 * needs::P
    {
        Subject::Stiff
    } else {
        Subject::Word(first)
    }
}

/// The pet's hint for its state right now.
pub fn hint(s: &GameState) -> Hint {
    let subject = subject(s);
    let list = remedies(subject);
    let mut accepted = list.iter().filter(|&&r| s.remedy_reason(r).is_none());
    // The first accepted remedy that goes well, else the first accepted one:
    // the pet does not ask for what the menu would mark risky while another
    // way out is clean, but it still asks when every way has a catch.
    let pick = accepted
        .clone()
        .find(|&&r| s.remedy_warning(r).is_none())
        .or_else(|| accepted.next());
    let ask = match pick {
        Some(&r) => Ask::Do(r),
        None => match list.first().and_then(|&r| s.remedy_reason(r)) {
            Some(why) => Ask::Refused(why),
            None => Ask::Say(otherwise(subject)),
        },
    };
    Hint { subject, ask }
}
