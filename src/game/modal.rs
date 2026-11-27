//! BornPets in-game modal overlay.
//!
//! A modal is a pop-over window drawn on top of the game screen when the player
//! activates an icon.  It shows a short action list; the selected item is
//! inverted.  The cancel button always dismisses it.
//!
//! ```text
//! ┌──────────────────────────────┐  y = 10
//! │▓▓▓▓▓▓▓▓▓ Feed ▓▓▓▓▓▓▓▓▓▓▓▓▓│  title bar (black fill, white text)
//! ├──────────────────────────────┤  y = 30
//! │  Feed now                    │
//! │ ►► Cancel ◄◄                 │  ← selected item, inverted
//! └──────────────────────────────┘  y = 141
//! ```
//!
//! 10 px margin on all sides keeps the underlying game screen visible.

use core::sync::atomic::{AtomicU8, Ordering};

use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{PrimitiveStyle, Rectangle, Triangle};
use embedded_graphics::text::{Text};

use super::engine::DebugPoke;
use super::nav::Row;
use crate::ui::{self, TEXT_BLACK, TEXT_BOLD_BLACK, TEXT_BOLD_WHITE};
use crate::{BLACK, TriColor, WHITE};

// ── Modal kind
// ────────────────────────────────────────────────────────────────

/// Which in-game modal is currently open.  Stored as a `u8` in `MODAL_KIND`.
///
/// Layout:
///   Top row (info/meta):    Stats, Hibernate, Exercise, (empty)
///   Bottom row (actions):   Feed,  Heal,      Play,     Rest
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ModalKind {
    None = 0,
    // Top row — info / meta.
    Stats = 1,     // top row, col 0
    Hibernate = 2, // top row, col 1
    // Bottom row — actions.
    Feed = 3, // bot row, col 0
    Heal = 4, // bot row, col 1
    Play = 5, // bot row, col 2
    Rest = 6, // bot row, col 3
    // Sub-modal opened from Play.
    Music = 7,
    Exercise = 8, // top row, col 2
    // Hidden — opened only via the button sequence in `debug_cheats`,
    // not reachable from any icon.
    Debug = 9,
    Drink = 10, // top row, col 3
}

impl ModalKind {
    pub fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::Stats,
            2 => Self::Hibernate,
            3 => Self::Feed,
            4 => Self::Heal,
            5 => Self::Play,
            6 => Self::Rest,
            7 => Self::Music,
            8 => Self::Exercise,
            9 => Self::Debug,
            10 => Self::Drink,
            _ => Self::None,
        }
    }

    fn title(self) -> &'static str {
        match self {
            Self::None => "",
            Self::Stats => "Stats",
            Self::Hibernate => "Hibernate",
            Self::Feed => "Feed",
            Self::Heal => "Heal",
            Self::Play => "Play",
            Self::Rest => "Rest",
            Self::Music => "Music",
            Self::Exercise => "Exercise",
            Self::Debug => "Debug",
            Self::Drink => "Drink",
        }
    }

    fn items(self) -> &'static [Item] {
        match self {
            Self::Stats => &[
                Item::ViewStats,
                Item::Hint,
                Item::Diary,
                Item::RolledStats,
                Item::Friends,
                Item::Cancel,
            ],
            // Demo sits with Hibernate: both put the pet's clock aside.
            Self::Hibernate => &[Item::Hibernate, Item::WakeUp, Item::Demo, Item::Cancel],
            Self::Feed => &[
                Item::FeedFood(super::engine::FoodKind::Salad),
                Item::FeedFood(super::engine::FoodKind::Apple),
                Item::FeedFood(super::engine::FoodKind::Frikandel),
                Item::FeedFood(super::engine::FoodKind::Pizza),
                Item::FeedFood(super::engine::FoodKind::Cake),
                Item::Cancel,
            ],
            Self::Heal => &[
                Item::GiveMedicine,
                Item::GiveMedication,
                Item::Ozempic,
                Item::Rehab,
                Item::Cancel,
            ],
            Self::Exercise => &[Item::ExerciseNow, Item::Cancel],
            Self::Debug => &DEBUG_ITEMS,
            Self::Drink => &[
                Item::DrinkChoice(super::engine::DrinkKind::Water),
                Item::DrinkChoice(super::engine::DrinkKind::Coffee),
                Item::DrinkChoice(super::engine::DrinkKind::Beer),
                Item::DrinkChoice(super::engine::DrinkKind::Wine),
                Item::DrinkChoice(super::engine::DrinkKind::Whiskey),
                Item::Cancel,
            ],
            Self::Play => {
                if super::lifecycle::money_mode() {
                    &[
                        Item::Battle,
                        Item::PlayNow,
                        Item::OnlyPets,
                        Item::PlayMusic,
                        Item::TicTacToe,
                        Item::LightsOut,
                        Item::BlackHole,
                        Item::BornJeweled,
                        Item::Cancel,
                    ]
                } else {
                    &[
                        Item::Battle,
                        Item::PlayNow,
                        Item::PlayMusic,
                        Item::TicTacToe,
                        Item::LightsOut,
                        Item::BlackHole,
                        Item::BornJeweled,
                        Item::Cancel,
                    ]
                }
            }
            Self::Music => &[
                Item::Song(crate::SONG_STARTUP_INDEX),
                Item::Song(crate::SONG_RICKROLL_INDEX),
                Item::Song(crate::SONG_IMPERIAL_MARCH_INDEX),
                Item::Song(crate::SONG_SANDSTORM_INDEX),
                Item::Song(crate::SONG_PINK_PANTHER_INDEX),
                Item::Song(crate::SONG_TROLOLO_INDEX),
                Item::Song(crate::SONG_DAISY_BELL_INDEX),
                Item::Song(crate::SONG_NOKIA_INDEX),
                Item::Song(crate::SONG_OVER_THE_HORIZON_INDEX),
                Item::Cancel,
            ],
            Self::Rest => &[
                Item::Sleep,
                Item::Relax,
                Item::Wash,
                Item::WakeFromSleep,
                Item::Cancel,
            ],
            Self::None => &[],
        }
    }
}

// ── Items
// ──────────────────────────────────────────────────────────────────────

/// One row in the modal's item list.  Each variant carries its own
/// label, availability check, cooldown source, and activation handler
/// — no string matching anywhere else in the file.
#[derive(Clone, Copy, PartialEq)]
enum Item {
    Cancel,
    ViewStats,
    Hint,
    Diary,
    RolledStats,
    Friends,
    Battle,
    FeedFood(super::engine::FoodKind),
    GiveMedicine,
    GiveMedication,
    Ozempic,
    ExerciseNow,
    /// A row of [`DEBUG_ROWS`] (the hidden Konami-code page).
    Debug(u8),
    DrinkChoice(super::engine::DrinkKind),
    Rehab,
    Sleep,
    Relax,
    Wash,
    /// End a sleep (distinct from `WakeUp`, the hibernation defrost).
    WakeFromSleep,
    PlayNow,
    OnlyPets,
    TicTacToe,
    LightsOut,
    BlackHole,
    BornJeweled,
    PlayMusic,
    /// Buzzer song index passed to [`crate::fw::buzzer::play`].
    Song(u8),
    Hibernate,
    WakeUp,
    /// The demo mode switch (the same switch as the game settings row).
    Demo,
}

/// What a debug row does.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum DebugRow {
    /// Show `engine::DebugInfo` as a toast.
    Info,
    /// Fast-forward this many minutes.
    Skip(u32),
    Poke(DebugPoke),
    /// Zero the Battle record, this pet's and every friend's.
    ResetBattle,
}

/// The hidden debug page: label and action of every row, in menu order. The
/// page list and the dispatch both read this one table.
const DEBUG_ROWS: [(&str, DebugRow); 23] = [
    ("Engine info", DebugRow::Info),
    ("Skip 10 min", DebugRow::Skip(10)),
    ("Skip 1 hour", DebugRow::Skip(60)),
    ("Skip 1 day", DebugRow::Skip(24 * 60)),
    ("Comfort", DebugRow::Poke(DebugPoke::Comfort)),
    ("Hungry", DebugRow::Poke(DebugPoke::Hungry)),
    ("Thirsty", DebugRow::Poke(DebugPoke::Thirsty)),
    ("Stuffed", DebugRow::Poke(DebugPoke::Stuffed)),
    ("Tired", DebugRow::Poke(DebugPoke::Tired)),
    ("Bored", DebugRow::Poke(DebugPoke::Bored)),
    ("Dirty", DebugRow::Poke(DebugPoke::Dirty)),
    ("Fat", DebugRow::Poke(DebugPoke::Fat)),
    ("Thin", DebugRow::Poke(DebugPoke::Thin)),
    ("Unfit", DebugRow::Poke(DebugPoke::Unfit)),
    ("Drunk", DebugRow::Poke(DebugPoke::Drunk)),
    ("Hung over", DebugRow::Poke(DebugPoke::HungOver)),
    ("Groggy", DebugRow::Poke(DebugPoke::Groggy)),
    ("Injured", DebugRow::Poke(DebugPoke::Injured)),
    ("Diabetic", DebugRow::Poke(DebugPoke::Diabetic)),
    ("Dependent", DebugRow::Poke(DebugPoke::Dependent)),
    ("Clear conditions", DebugRow::Poke(DebugPoke::ClearConditions)),
    ("Clear cooldowns", DebugRow::Poke(DebugPoke::ClearCooldowns)),
    ("Reset battle record", DebugRow::ResetBattle),
];

/// The debug page's items: one per table row, then Cancel.
static DEBUG_ITEMS: [Item; DEBUG_ROWS.len() + 1] = {
    let mut items = [Item::Cancel; DEBUG_ROWS.len() + 1];
    let mut i = 0;
    while i < DEBUG_ROWS.len() {
        items[i] = Item::Debug(i as u8);
        i += 1;
    }
    items
};

/// Do one debug row. A poke the pet cannot take shows its reason (the one
/// every refused action shows) and leaves the menu open.
fn run_debug_row(row: DebugRow) {
    use super::lifecycle;
    match row {
        DebugRow::Info => super::show_toast(super::Toast::DebugInfo),
        DebugRow::Skip(minutes) => {
            lifecycle::debug_skip_minutes(minutes);
            super::show_toast(super::Toast::DebugCheat);
        }
        DebugRow::Poke(poke) => match lifecycle::debug_poke(poke) {
            Ok(()) => super::show_toast(super::Toast::DebugCheat),
            Err(why) => return super::show_reason(why),
        },
        DebugRow::ResetBattle => {
            lifecycle::debug_reset_battle_record();
            super::show_toast(super::Toast::DebugCheat);
        }
    }
    close();
}

impl Item {
    fn label(self) -> &'static str {
        match self {
            Self::Cancel => "Cancel",
            Self::ViewStats => "View stats",
            Self::Hint => "Hint",
            Self::Diary => "Diary",
            Self::RolledStats => "Rolled stats",
            Self::Friends => "Friends",
            Self::Battle => "Battle",
            Self::FeedFood(food) => food.label(),
            Self::GiveMedicine => "Aspirine",
            Self::GiveMedication => "Insulin",
            Self::Ozempic => "Ozempic",
            Self::ExerciseNow => "Exercise now",
            Self::Debug(row) => DEBUG_ROWS[row as usize].0,
            Self::DrinkChoice(drink) => drink.label(),
            Self::Rehab => "Rehab",
            Self::Sleep => "Sleep",
            Self::Relax => "Relax",
            Self::Wash => "Wash",
            Self::WakeFromSleep => "Wake up",
            Self::PlayNow => "Play now",
            Self::OnlyPets => "Only pets",
            Self::TicTacToe => "Tic Tac Toe",
            Self::LightsOut => "Lights Out",
            Self::BlackHole => "Black Hole",
            Self::BornJeweled => "BornJeweled",
            Self::PlayMusic => "Play music",
            Self::Song(crate::SONG_STARTUP_INDEX) => "Startup",
            Self::Song(crate::SONG_RICKROLL_INDEX) => "Rickroll",
            Self::Song(crate::SONG_IMPERIAL_MARCH_INDEX) => "Imp. March",
            Self::Song(crate::SONG_SANDSTORM_INDEX) => "Sandstorm",
            Self::Song(crate::SONG_PINK_PANTHER_INDEX) => "Pink Panther",
            Self::Song(crate::SONG_TROLOLO_INDEX) => "Trololo",
            Self::Song(crate::SONG_DAISY_BELL_INDEX) => "Daisy Bell",
            Self::Song(crate::SONG_NOKIA_INDEX) => "Nokia Tune",
            Self::Song(crate::SONG_OVER_THE_HORIZON_INDEX) => "Samsung",
            Self::Song(_) => "?",
            Self::Hibernate => "Hibernate",
            Self::WakeUp => "Defrost",
            Self::Demo => super::lifecycle::demo_menu_label(),
        }
    }

    /// HAX price of this item in the current mode, if it costs money.
    /// Shown as a " NNH" suffix in the menu (only when money mode is
    /// on). Only-pets earns HAX rather than costing it, so it has no
    /// price. Takes `stats` so Hard (US) mode can report the actual
    /// effective price rather than the normal-mode base.
    fn hax_price(self, stats: &super::engine::PetStats) -> Option<u32> {
        use super::engine::doses::price;
        match self {
            Self::FeedFood(food) => price(super::engine::Action::Feed, Some(food), None, stats.hard_mode),
            _ => {
                let (action, drink) = self.action()?;
                price(action, None, drink, stats.hard_mode)
            }
        }
    }

    /// Is the action currently available?  Cooldown-gated items
    /// return false until the cooldown decays to 0.
    ///
    /// While the pet is hibernating EVERY action is locked except the
    /// Defrost button, plus passive info screens (Cancel / ViewStats /
    /// RolledStats).  The pet must be defrosted before mini-games or
    /// stat actions can run again.
    fn available(self, stats: &super::engine::PetStats) -> bool {
        // The demo switch asks the engine itself: off always works, on is
        // refused while hibernating, for a pet in poor shape, or with no pet.
        if self == Self::Demo {
            return super::lifecycle::demo_switch_refusal().is_none();
        }
        // Debug rows are always selectable: a poke the pet cannot take says
        // why when it runs, and the skips work on a hibernating pet too.
        if stats.hibernating && !matches!(self, Self::Debug(_)) {
            return matches!(
                self,
                Self::Cancel
                    | Self::ViewStats
                    | Self::Hint
                    | Self::Diary
                    | Self::RolledStats
                    | Self::WakeUp,
            );
        }
        // Money mode: the price must be covered, the same check as
        // `GameState::remedy_reason` (the hint) makes.
        let affordable = self
            .hax_price(stats)
            .is_none_or(|price| !stats.money_enabled || stats.money >= price);
        affordable
            && match self {
                Self::Cancel
                | Self::ViewStats
                | Self::Hint
                | Self::Diary
                | Self::RolledStats
                | Self::Friends
                | Self::PlayMusic
                | Self::Song(_)
                | Self::Debug(_) => true,
                Self::DrinkChoice(drink) => {
                    stats.can_drink
                        && (drink != super::engine::DrinkKind::Water || stats.can_drink_water)
                }
                Self::Rehab => stats.can_rehab,
                Self::Battle => stats.can_battle,
                Self::FeedFood(_) => stats.can_feed,
                Self::GiveMedicine => stats.can_heal,
                Self::GiveMedication => stats.can_medicate,
                Self::Ozempic => stats.can_ozempic,
                Self::ExerciseNow => stats.can_exercise,
                Self::Sleep => stats.can_sleep,
                Self::Relax => stats.can_relax,
                Self::Wash => stats.can_wash,
                Self::WakeFromSleep => stats.can_wake,
                Self::PlayNow => stats.can_play,
                // Never gated by price: the escape hatch when broke.
                Self::OnlyPets => stats.can_only_pets,
                Self::TicTacToe => stats.can_play_tictactoe,
                Self::LightsOut => stats.can_play_lightsout,
                Self::BlackHole => stats.can_play_blackhole,
                Self::BornJeweled => stats.can_play_bornjeweled,
                Self::Hibernate => true,
                Self::WakeUp => false,
                Self::Demo => true, // answered at the top, before the hibernation gate
            }
    }

    /// The warning this row's dose would cause (the engine's pure preview), or
    /// `None` for an unavailable row, a row that is no dose, or a clean dose.
    /// Each call clones the state (a night's sleep for `Sleep`): call it once
    /// per drawn row, never per glyph.
    fn risk(self, stats: &super::engine::PetStats) -> Option<super::engine::causes::Cause> {
        use super::lifecycle;
        if !self.available(stats) {
            return None;
        }
        let cause = match self {
            Self::Sleep => lifecycle::preview_sleep(),
            Self::WakeFromSleep => lifecycle::preview_wake(),
            Self::FeedFood(food) => lifecycle::preview(super::engine::Action::Feed, Some(food), None),
            Self::DrinkChoice(d) => lifecycle::preview(super::engine::Action::Drink, None, Some(d)),
            _ => {
                let (a, _) = self.action()?;
                lifecycle::preview(a, None, None)
            }
        };
        cause.filter(|c| c.is_warning())
    }

    /// Can the cursor rest on this row?  Cancel always can (so every
    /// list has at least one selectable row); everything else needs an
    /// engine snapshot that says it's available.  Missing snapshot
    /// (very early boot) locks out everything but Cancel.
    fn selectable(self, stats: Option<&super::engine::PetStats>) -> bool {
        matches!(self, Self::Cancel) || stats.is_some_and(|s| self.available(s))
    }

    /// Time-until-ready in 10-second ticks for a disabled item: the running
    /// action's remaining ticks for its own row, otherwise the wait the
    /// engine itself reports (`Reason::Wait`, the same one the toast shows).
    /// 0 = ready / not gated by a wait.
    fn cooldown_ticks(self, stats: &super::engine::PetStats) -> u32 {
        use super::engine::doses::Reason;
        if self.action().is_some_and(|(a, _)| stats.active_action == Some(a)) {
            return stats.action_ticks_remaining as u32;
        }
        match self.reason(stats) {
            Reason::Wait(secs) => secs / 10,
            _ => 0,
        }
    }

    /// The engine action (and chosen drink) behind this row, if it is one.
    fn action(
        self,
    ) -> Option<(super::engine::Action, Option<super::engine::DrinkKind>)> {
        use super::engine::Action;
        Some(match self {
            Self::FeedFood(_) => (Action::Feed, None),
            Self::GiveMedicine => (Action::Heal, None),
            Self::GiveMedication => (Action::Medicate, None),
            Self::Ozempic => (Action::Ozempic, None),
            Self::ExerciseNow => (Action::Exercise, None),
            Self::DrinkChoice(d) => (Action::Drink, Some(d)),
            Self::Rehab => (Action::Rehab, None),
            Self::Relax => (Action::Relax, None),
            Self::Wash => (Action::Wash, None),
            Self::PlayNow => (Action::Play, None),
            Self::OnlyPets => (Action::OnlyPets, None),
            _ => return None,
        })
    }

    /// Why this row is disabled (or its starter just refused), asked of the
    /// engine; "can't afford" is what is left when the engine has no
    /// objection.
    fn reason(self, stats: &super::engine::PetStats) -> super::engine::doses::Reason {
        use super::engine::MiniGame;
        use super::engine::doses::{Reason, Refusal};
        use super::lifecycle;
        if stats.hibernating {
            return Reason::Hibernating;
        }
        let engine_says = match self {
            Self::Battle => lifecycle::battle_refusal(),
            Self::TicTacToe => lifecycle::game_refusal(MiniGame::TicTacToe),
            Self::LightsOut => lifecycle::game_refusal(MiniGame::LightsOut),
            Self::BlackHole => lifecycle::game_refusal(MiniGame::BlackHole),
            Self::BornJeweled => lifecycle::game_refusal(MiniGame::BornJeweled),
            Self::Sleep => lifecycle::sleep_refusal(),
            Self::WakeFromSleep => (!stats.is_sleeping).then_some(Reason::Refused(Refusal::NoNeed)),
            Self::Demo => lifecycle::demo_switch_refusal(),
            _ => match self.action() {
                Some((a, d)) => lifecycle::refusal_for(a, d),
                None => None,
            },
        };
        match (engine_says, self.action()) {
            (Some(why), _) => why,
            (None, Some(_)) => Reason::CantAfford,
            (None, None) => Reason::Refused(Refusal::NoNeed),
        }
    }

    /// A starter just returned: a toast for success (and close), or the
    /// reason it refused (menu stays open).
    fn started(self, ok: bool, toast: super::Toast) {
        if ok {
            super::show_toast(toast);
            close();
        } else if let Some(stats) = super::lifecycle::cycle() {
            super::show_reason(self.reason(&stats));
        }
    }

    fn activate(self) {
        use super::lifecycle;
        match self {
            Self::Cancel => close(),
            Self::ViewStats => {
                STATS_PAGE.store(0, Ordering::Relaxed);
                STATS_VIEW.store(true, Ordering::Relaxed);
                super::note_live_view_drawn();
            }
            Self::Hint => {
                HINT_VIEW.store(true, Ordering::Relaxed);
                super::note_live_view_drawn();
            }
            Self::Diary => {
                super::diary_view::open();
                close();
            }
            Self::RolledStats => {
                super::traits_view::open();
                close();
            }
            Self::Friends => {
                super::friends_view::open();
                close();
            }
            Self::Battle => {
                super::battle_view::open();
                close();
            }
            Self::FeedFood(food) => {
                self.started(lifecycle::feed(food), super::Toast::Feed);
            }
            Self::GiveMedicine => {
                self.started(lifecycle::heal(), super::Toast::Heal);
            }
            Self::GiveMedication => {
                self.started(lifecycle::medicate(), super::Toast::Medicate);
            }
            Self::Ozempic => {
                self.started(lifecycle::ozempic(), super::Toast::Ozempic);
            }
            Self::ExerciseNow => {
                self.started(lifecycle::exercise(), super::Toast::Exercise);
            }
            Self::Debug(row) => run_debug_row(DEBUG_ROWS[row as usize].1),
            Self::DrinkChoice(drink) => {
                let toast = if drink.is_alcoholic() { super::Toast::Drink } else { super::Toast::Refreshed };
                self.started(lifecycle::drink(drink), toast);
            }
            Self::Rehab => {
                self.started(lifecycle::rehab(), super::Toast::Rehab);
            }
            Self::Sleep => {
                self.started(lifecycle::sleep(), super::Toast::Sleep);
            }
            Self::Relax => {
                self.started(lifecycle::relax(), super::Toast::Relax);
            }
            Self::Wash => {
                self.started(lifecycle::wash(), super::Toast::Wash);
            }
            Self::WakeFromSleep => {
                self.started(lifecycle::wake(), super::Toast::Wake);
            }
            Self::PlayNow => {
                self.started(lifecycle::play(), super::Toast::Play);
            }
            Self::OnlyPets => {
                self.started(lifecycle::only_pets(), super::Toast::OnlyPets);
            }
            Self::TicTacToe => {
                super::tictactoe::open();
                close();
            }
            Self::LightsOut => {
                super::lightsout::open();
                close();
            }
            Self::BlackHole => {
                super::blackhole::open();
                close();
            }
            Self::BornJeweled => {
                super::bornjeweled::open();
                close();
            }
            Self::PlayMusic => open(ModalKind::Music),
            Self::Song(idx) => play_song(idx as usize),
            Self::Hibernate => {
                lifecycle::hibernate();
                super::show_toast(super::Toast::Hibernate);
                close();
            }
            Self::WakeUp => {
                lifecycle::wake_from_hibernation();
                super::show_toast(super::Toast::Wake);
                close();
            }
            // The main screen's demo mark and help line show the new state.
            Self::Demo => {
                let _ = lifecycle::toggle_demo();
                close();
            }
        }
    }
}

/// Format a cooldown count (ticks of 10 s each) as a " 50s", " 59m" or
/// " 2h05" suffix for a disabled menu row.  Returns `None` when the cooldown is 0.
fn cooldown_string(ticks: u32) -> Option<heapless::String<12>> {
    if ticks == 0 {
        return None;
    }
    let mut s = heapless::String::new();
    let _ = core::fmt::Write::write_fmt(
        &mut s,
        format_args!(" {}", super::engine::doses::fmt_wait(ticks.saturating_mul(10))),
    );
    Some(s)
}

/// Map an icon (row, col) to the modal it should open.
///
/// While the pet is hibernating, every action icon (bottom row) is a
/// no-op — only Stats and Hibernate (which holds the Defrost button)
/// can be opened.  Defence-in-depth alongside [`Item::available`].
pub fn kind_for_icon(row: Row, col: u8) -> ModalKind {
    if super::lifecycle::is_hibernating() {
        return match (row, col) {
            (Row::Top, 0) => ModalKind::Stats,
            (Row::Top, 1) => ModalKind::Hibernate,
            _ => ModalKind::None,
        };
    }
    match (row, col) {
        // Top row: info / meta.
        (Row::Top, 0) => ModalKind::Stats,
        (Row::Top, 1) => ModalKind::Hibernate,
        (Row::Top, 2) => ModalKind::Exercise,
        (Row::Top, 3) => ModalKind::Drink,
        (Row::Top, _) => ModalKind::None,
        // Bottom row: actions.
        (Row::Bottom, 0) => ModalKind::Feed,
        (Row::Bottom, 1) => ModalKind::Heal,
        (Row::Bottom, 2) => ModalKind::Play,
        (Row::Bottom, 3) => ModalKind::Rest,
        _ => ModalKind::None,
    }
}

// ── Global state
// ──────────────────────────────────────────────────────────────

static MODAL_KIND: AtomicU8 = AtomicU8::new(0);
static MODAL_POS: AtomicU8 = AtomicU8::new(0);
/// When true, the Stats modal shows stat bars instead of the menu list.
static STATS_VIEW: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);
/// Which stats page shows: 0 = Needs, 1 = Body. Opening the view resets it.
static STATS_PAGE: AtomicU8 = AtomicU8::new(0);
/// When true, the Stats modal shows the pet's hint (a speech view) instead of
/// the menu list.
static HINT_VIEW: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

pub fn open(kind: ModalKind) {
    // Start on the first row that can be used, so a disabled first row
    // (e.g. Battle on cooldown) doesn't need to be stepped past.
    // Mark the menu open first: a cause drained by this cycle is then logged
    // but not toasted under the popover that opens on the same frame.
    MODAL_KIND.store(kind as u8, Ordering::Relaxed);
    let stats = super::lifecycle::cycle();
    let items = kind.items();
    let pos = first_selectable(items.len(), |i| items[i].selectable(stats.as_ref()));
    MODAL_POS.store(pos as u8, Ordering::Relaxed);
    // Menu open → redraw the whole frame next refresh (grey-out overlay
    // covers everything); avoids partial-delta artifacts at the overlay edge.
    crate::FULL_REFRESH_PENDING.store(true, Ordering::Relaxed);
}

pub fn close() {
    HINT_VIEW.store(false, Ordering::Relaxed);
    STATS_VIEW.store(false, Ordering::Relaxed);
    STATS_PAGE.store(0, Ordering::Relaxed);
    MODAL_KIND.store(ModalKind::None as u8, Ordering::Relaxed);
    MODAL_POS.store(0, Ordering::Relaxed);
    // Menu close → redraw the whole frame (remove the overlay cleanly).
    crate::FULL_REFRESH_PENDING.store(true, Ordering::Relaxed);
}

/// A modal sub-view whose numbers change by themselves: the stats bars and
/// the hint (its wait and the pet's need drift).
pub fn live_modal_view_open() -> bool {
    STATS_VIEW.load(Ordering::Relaxed) || HINT_VIEW.load(Ordering::Relaxed)
}

/// Leave the hint view for the Stats list. True when it was showing (the
/// button is spent); false when there was nothing to dismiss.
pub fn dismiss_hint() -> bool {
    HINT_VIEW.swap(false, Ordering::Relaxed)
}

/// Show the other stats page. Only while the stats view shows.
pub fn flip_page() {
    if dismiss_hint() {
        return;
    }
    if STATS_VIEW.load(Ordering::Relaxed) {
        STATS_PAGE.store(stats_page().other() as u8, Ordering::Relaxed);
    }
}

/// The stats page showing (or about to show).
pub fn stats_page() -> super::stats_pages::Page {
    use super::stats_pages::Page;
    if STATS_PAGE.load(Ordering::Relaxed) == Page::Body as u8 {
        Page::Body
    } else {
        Page::Needs
    }
}

pub fn is_open() -> bool {
    MODAL_KIND.load(Ordering::Relaxed) != 0
}

/// The menu that is open (`ModalKind::None` when none is). Host-only, for the
/// guide's shot checks.
#[cfg(any(test, feature = "simulator"))]
pub fn open_kind() -> ModalKind {
    ModalKind::from_u8(MODAL_KIND.load(Ordering::Relaxed))
}

/// Whether the hint view is showing. Host-only, for the guide's shot checks.
#[cfg(any(test, feature = "simulator"))]
pub fn hint_view_open() -> bool {
    HINT_VIEW.load(Ordering::Relaxed)
}

/// Whether the stats bars are showing. Host-only, for the guide's shot checks.
#[cfg(any(test, feature = "simulator"))]
pub fn stats_view_open() -> bool {
    STATS_VIEW.load(Ordering::Relaxed)
}

/// How many rows of the open menu carry the risk mark, from the stats of the
/// last cycle (the caller cycles first; this does not). Host-only, for the
/// guide's shot checks (the mark is drawn, not typed).
#[cfg(any(test, feature = "simulator"))]
pub fn risk_row_count(stats: &super::engine::PetStats) -> usize {
    open_kind()
        .items()
        .iter()
        .filter(|item| item.risk(stats).is_some())
        .count()
}

// ── Cursor navigation
// ─────────────────────────────────────────────────────────

/// Index of the first row `selectable` accepts, or 0 when none does.
fn first_selectable(len: usize, selectable: impl Fn(usize) -> bool) -> usize {
    (0..len).find(|&i| selectable(i)).unwrap_or(0)
}

/// The next row after `from` (before it when `forward` is false) that
/// `enabled` accepts, wrapping around the ends of a list of `len` rows.
/// Disabled rows are never returned; when no other row is enabled the
/// cursor stays at `from`.
fn next_enabled(len: usize, from: usize, forward: bool, enabled: impl Fn(usize) -> bool) -> usize {
    let mut i = from;
    for _ in 0..len {
        i = if forward { (i + 1) % len } else { (i + len - 1) % len };
        if enabled(i) {
            return i;
        }
    }
    from
}

/// Move the cursor to the next enabled row up or down, wrapping at the
/// ends. Disabled rows are skipped, never highlighted; with no other row
/// enabled the cursor stays put.
fn move_cursor(forward: bool) {
    if dismiss_hint() {
        return;
    }
    // While the stats-bar sub-view is showing there's no item list on
    // screen to move a cursor through — ignore Up/Down so it doesn't
    // silently advance MODAL_POS underneath the bars (surfacing as a
    // seemingly random row when the view is later dismissed back to the list).
    if STATS_VIEW.load(Ordering::Relaxed) {
        return;
    }
    let kind = ModalKind::from_u8(MODAL_KIND.load(Ordering::Relaxed));
    let items = kind.items();
    if items.is_empty() {
        return;
    }
    let stats = super::lifecycle::cycle();
    let pos = (MODAL_POS.load(Ordering::Relaxed) as usize).min(items.len() - 1);
    let next = next_enabled(items.len(), pos, forward, |i| items[i].selectable(stats.as_ref()));
    MODAL_POS.store(next as u8, Ordering::Relaxed);
}

pub fn cursor_up() {
    move_cursor(false);
}

pub fn cursor_down() {
    move_cursor(true);
}

/// Activate the currently selected item.
///
/// "Cancel" closes the modal.  Action items dispatch to the game engine
/// via `lifecycle`.  "View stats" opens the stats bar display.
pub fn activate() {
    if dismiss_hint() {
        return;
    }
    // If stats view is showing, any activation closes it.
    if STATS_VIEW.load(Ordering::Relaxed) {
        STATS_VIEW.store(false, Ordering::Relaxed);
        STATS_PAGE.store(0, Ordering::Relaxed);
        return;
    }

    let kind = ModalKind::from_u8(MODAL_KIND.load(Ordering::Relaxed));
    let pos = MODAL_POS.load(Ordering::Relaxed) as usize;
    let items = kind.items();
    let Some(&item) = items.get(pos) else {
        return;
    };

    // Cancel is always allowed.  Everything else gates on the item's
    // own availability check.
    if !matches!(item, Item::Cancel) {
        let stats = match super::lifecycle::cycle() {
            Some(s) => s,
            None => return,
        };
        if !item.available(&stats) {
            // Never a silent no-op: say why this row is disabled.
            super::show_reason(item.reason(&stats));
            return;
        }
    }

    item.activate();
}

fn play_song(_index: usize) {
    #[cfg(feature = "embassy-core")]
    crate::fw::buzzer::play(_index);
    close();
}

// ── Drawing
// ───────────────────────────────────────────────────────────────────

const MARGIN: i32 = 10;
const MODAL_W: u32 = 132; // 152 - 2 × MARGIN
const MODAL_H: u32 = 132;
const BORDER: u32 = 2;
const TITLE_H: i32 = 18;
const ITEM_H: i32 = 16;

/// Most rows one popover shows at once (the window is under 120 px tall).
const MAX_VISIBLE_ROWS: usize = 8;

/// Where a row's text starts, relative to the popover's inner left edge.
/// The Feed page starts at the edge: "Frikandel spec 10H" is 18 columns
/// (126 px) and only fits the 128 px inner width without the usual indent.
fn row_indent(kind: ModalKind) -> i32 {
    if kind == ModalKind::Feed { 0 } else { 4 }
}

/// The row's text: label, then the wait while disabled, else the price in
/// money mode. A row that cannot be bought now shows its wait instead of its
/// price ("Frikandel spec 20s"), so the longest wait ("Ozempic 47h59") still
/// fits the popover. The risk mark is drawn, not typed, so it takes no
/// columns.
fn row_text(item: Item, stats: Option<&super::engine::PetStats>) -> heapless::String<40> {
    // 40 covers the longest label (19 bytes) and a price (" 15H") or the
    // longest wait (" 72h00"); `push_str` silently no-ops past capacity.
    let mut text: heapless::String<40> = heapless::String::new();
    let _ = text.push_str(shown_label(item, stats.is_some_and(|s| s.hard_mode)));
    if let Some(s) = stats {
        if !item.available(s)
            && let Some(wait) = cooldown_string(item.cooldown_ticks(s))
        {
            // Rows with no wait to show get no suffix: selecting one tells
            // the player why (R24).
            let _ = text.push_str(wait.as_str());
        } else if matches!(item, Item::Battle)
            && item.available(s)
            && let Some(form) = super::lifecycle::battle_form()
        {
            // The pet's form tells the player what a fight is worth now.
            let _ = text.push_str(": ");
            let _ = text.push_str(form.text());
        } else if let Some(price) = item.hax_price(s)
            && s.money_enabled
        {
            let _ = core::fmt::Write::write_fmt(&mut text, format_args!(" {price}H"));
        }
    }
    text
}

/// US "Hard mode" reskins the Frikandel speciaal as a Hamburger.
fn shown_label(item: Item, hard: bool) -> &'static str {
    match item {
        Item::FeedFood(super::engine::FoodKind::Frikandel) if hard => "Hamburger",
        _ => item.label(),
    }
}

/// Every word the menus draw with the game's current money and hard modes
/// (host-only, for the guide's word list): `(group, text)`. Group "menu" is
/// the titles and rows, "debug" the hidden debug page, "hint" the hint
/// view's title and refusal line. The templates are the drawn text with its
/// value left out: "<price>H", "risk: <what>", "!" (the risk mark's name in
/// the guide), "<name> says" and "but <reason>".
#[cfg(any(test, feature = "simulator"))]
pub fn menu_words() -> std::vec::Vec<(&'static str, std::string::String)> {
    use std::string::{String, ToString};
    let hard = super::lifecycle::hard_mode();
    let mut out: std::vec::Vec<(&'static str, String)> = std::vec::Vec::new();
    for v in 1..=u8::MAX {
        let kind = ModalKind::from_u8(v);
        if kind == ModalKind::None {
            continue;
        }
        let group = if kind == ModalKind::Debug { "debug" } else { "menu" };
        out.push((group, kind.title().to_string()));
        for &item in kind.items() {
            let group = if matches!(item, Item::Cancel) { "menu" } else { group };
            out.push((group, shown_label(item, hard).to_string()));
        }
    }
    for food in super::engine::FoodKind::ALL {
        let name = match food {
            super::engine::FoodKind::Frikandel if hard => "Hamburger",
            f => f.full_name(),
        };
        out.push(("menu", name.to_string()));
    }
    out.push(("menu", "Battle: <form>".to_string()));
    for form in super::engine::combat::Form::ALL {
        out.push(("menu", form.text().to_string()));
    }
    for t in ["<price>H", "risk: <what>", "!"] {
        out.push(("menu", t.to_string()));
    }
    for t in ["<name> says", "Your pet says", "but <reason>"] {
        out.push(("hint", t.to_string()));
    }
    out
}

/// The title bar: the reason a refused row just gave (its toast sits under the
/// popover), else the selected row's risk, else the selected food's whole
/// name on the Feed page, else the menu title.
fn title_text(
    kind: ModalKind,
    selected: Option<Item>,
    stats: Option<&super::engine::PetStats>,
    risk: Option<super::engine::causes::Cause>,
) -> heapless::String<24> {
    let mut t: heapless::String<24> = heapless::String::new();
    if let Some(why) = super::reason_toast() {
        let _ = t.push_str(why.text().as_str());
    } else if let Some(cause) = risk {
        let _ = t.push_str("risk: ");
        let _ = t.push_str(cause.what());
    } else if let Some(Item::FeedFood(food)) = selected {
        let hard = stats.is_some_and(|s| s.hard_mode);
        let _ = t.push_str(match food {
            super::engine::FoodKind::Frikandel if hard => "Hamburger",
            f => f.full_name(),
        });
    } else {
        let _ = t.push_str(kind.title());
    }
    t
}

/// The risk mark's box. It sits at the right inner edge with one white
/// column between it and the border (which starts at x = 141). A 7 px glyph cell ends in a blank
/// column, so a mark starting at or after `text_end` (the x just past the
/// row's last cell) has at least one white column between it and the text
/// ink. It is 2 px wide when the text leaves room, else 1 px (the 18 column
/// Frikandel row ends at x = 138).
fn risk_mark_rect(row_top: i32, text_end: i32) -> Rectangle {
    let right = MARGIN + (MODAL_W - BORDER) as i32 - 1;
    let w = if text_end <= right - 2 { 2 } else { 1 };
    Rectangle::new(
        Point::new(right - w, row_top + (ITEM_H - RISK_MARK_H as i32) / 2),
        Size::new(w as u32, RISK_MARK_H),
    )
}

const RISK_MARK_H: u32 = 9;

/// A bar and a dot.
fn draw_risk_mark<D>(
    display: &mut D,
    row_top: i32,
    text_end: i32,
    colour: TriColor,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let r = risk_mark_rect(row_top, text_end);
    let style = PrimitiveStyle::with_fill(colour);
    Rectangle::new(r.top_left, Size::new(r.size.width, RISK_MARK_H - 3))
        .into_styled(style)
        .draw(display)?;
    Rectangle::new(
        Point::new(r.top_left.x, r.top_left.y + RISK_MARK_H as i32 - 2),
        Size::new(r.size.width, 2),
    )
    .into_styled(style)
    .draw(display)
}

/// Draw the modal overlay.  Call this after `draw_screen_game` so it renders
/// on top.  Does nothing when no modal is open.
pub fn draw_modal<D>(display: &mut D) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let kind = ModalKind::from_u8(MODAL_KIND.load(Ordering::Relaxed));
    if kind == ModalKind::None {
        return Ok(());
    }

    // Stats view: show stat bars instead of the menu.
    if STATS_VIEW.load(Ordering::Relaxed) {
        return draw_stats_view(display);
    }
    if HINT_VIEW.load(Ordering::Relaxed) {
        return draw_hint_view(display);
    }

    let pos = MODAL_POS.load(Ordering::Relaxed) as usize;
    let items = kind.items();

    // White popover frame with 2 px black border.
    ui::draw_popover_frame(
        display,
        Point::new(MARGIN, MARGIN),
        Size::new(MODAL_W, MODAL_H),
        BORDER,
    )?;

    let inner_x = MARGIN + BORDER as i32;
    let inner_y = MARGIN + BORDER as i32;
    let inner_w = MODAL_W - BORDER * 2;

    // Item list
    let left_style = crate::ui::MIDDLE_LEFT;

    let list_y = inner_y + TITLE_H;
    let list_bottom = MARGIN + MODAL_H as i32 - BORDER as i32;

    let stats = super::lifecycle::cycle();

    // ── Scroll window ─────────────────────────────────────────────────
    // When the items list is taller than the visible area, reserve a
    // strip above and below for up/down arrow indicators and slide a
    // window over `items` so the selected `pos` is always visible.
    // Stateless: scroll position is derived from `pos` each frame.
    const ARROW_H: i32 = 10;
    let visible_no_arrows = ((list_bottom - list_y) / ITEM_H).max(1) as usize;
    let needs_scroll = items.len() > visible_no_arrows;
    let arrow_pad = if needs_scroll { ARROW_H } else { 0 };
    let inner_top = list_y + arrow_pad;
    let inner_bottom = list_bottom - arrow_pad;
    let visible_rows = ((inner_bottom - inner_top) / ITEM_H).max(1) as usize;
    let max_top = items.len().saturating_sub(visible_rows);
    // Center the selected row in the window when possible; clamp at
    // the start and end of the items list.
    let scroll_top = pos.saturating_sub(visible_rows / 2).min(max_top);

    // Risks once per draw: each is a clone of the pet (a night for Sleep).
    let mut risks = [None; MAX_VISIBLE_ROWS];
    if let Some(s) = stats.as_ref() {
        for (slot, item) in risks
            .iter_mut()
            .zip(items.iter().skip(scroll_top).take(visible_rows))
        {
            *slot = item.risk(s);
        }
    }
    let selected_risk = pos
        .checked_sub(scroll_top)
        .and_then(|v| risks.get(v).copied().flatten());
    let title = title_text(kind, items.get(pos).copied(), stats.as_ref(), selected_risk);
    ui::draw_title_bar(display, title.as_str(), Point::new(inner_x, inner_y), inner_w)?;

    for (vis_i, item) in items
        .iter()
        .enumerate()
        .skip(scroll_top)
        .take(visible_rows)
        .map(|(i, it)| (i - scroll_top, it))
    {
        let i = scroll_top + vis_i;
        let row_top = inner_top + vis_i as i32 * ITEM_H;
        let row_mid = row_top + ITEM_H / 2;
        if row_top + ITEM_H > inner_bottom {
            break;
        }

        // The cursor normally skips disabled rows, but one can still be
        // under it when it became unavailable while the modal was open
        // (e.g. a cooldown started elsewhere) — the "selected but locked
        // out" rendering below covers that case.
        let available = item.selectable(stats.as_ref());

        let display_label = row_text(*item, stats.as_ref());
        let text_x = inner_x + row_indent(kind);
        let text_end =
            text_x + display_label.chars().count() as i32 * TEXT_BOLD_BLACK.font.character_size.width as i32;
        let risk = risks.get(vis_i).copied().flatten().is_some();

        if i == pos && available {
            // Selected and available: inverted row.
            Rectangle::new(
                Point::new(inner_x, row_top),
                Size::new(inner_w, ITEM_H as u32),
            )
            .into_styled(PrimitiveStyle::with_fill(BLACK))
            .draw(display)?;
            Text::with_text_style(
                display_label.as_str(),
                Point::new(text_x, row_mid),
                TEXT_BOLD_WHITE,
                left_style,
            )
            .draw(display)?;
        } else if i == pos && !available {
            // Selected but on cooldown: outline only, black text on
            // white.  The dim pass below halftones both into grey so
            // the row reads as "selected but locked out".
            Rectangle::new(
                Point::new(inner_x, row_top),
                Size::new(inner_w, ITEM_H as u32),
            )
            .into_styled(PrimitiveStyle::with_stroke(BLACK, 1))
            .draw(display)?;
            Text::with_text_style(
                display_label.as_str(),
                Point::new(text_x, row_mid),
                TEXT_BOLD_BLACK,
                left_style,
            )
            .draw(display)?;
        } else {
            // Not selected.
            Text::with_text_style(
                display_label.as_str(),
                Point::new(text_x, row_mid),
                TEXT_BOLD_BLACK,
                left_style,
            )
            .draw(display)?;
        }

        if risk {
            let colour = if i == pos { WHITE } else { BLACK };
            draw_risk_mark(display, row_top, text_end, colour)?;
        }

        // Apply the grey-out overlay last so it covers everything we
        // just drew for this row.
        if !available {
            dim_rect(
                display,
                Rectangle::new(
                    Point::new(inner_x, row_top),
                    Size::new(inner_w, ITEM_H as u32),
                ),
            )?;
        }
    }

    // ── Scroll arrows ─────────────────────────────────────────────────
    // Drawn after the rows so they overlay correctly.  Each is a
    // filled triangle horizontally centred in `inner_w`, occupying
    // most of the `ARROW_H`-tall padding strip with a 1 px gap on
    // both sides for breathing room.
    if needs_scroll {
        let cx = inner_x + inner_w as i32 / 2;
        let half_w: i32 = 7;
        if scroll_top > 0 {
            // Up arrow: apex at top, base at bottom.
            let top_y = list_y + 1;
            let bot_y = list_y + ARROW_H - 2;
            Triangle::new(
                Point::new(cx, top_y),
                Point::new(cx - half_w, bot_y),
                Point::new(cx + half_w, bot_y),
            )
            .into_styled(PrimitiveStyle::with_fill(BLACK))
            .draw(display)?;
        }
        if scroll_top + visible_rows < items.len() {
            // Down arrow: base at top, apex at bottom.
            let top_y = list_bottom - ARROW_H + 2;
            let bot_y = list_bottom - 1;
            Triangle::new(
                Point::new(cx - half_w, top_y),
                Point::new(cx + half_w, top_y),
                Point::new(cx, bot_y),
            )
            .into_styled(PrimitiveStyle::with_fill(BLACK))
            .draw(display)?;
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Disabled-state dimming
// ---------------------------------------------------------------------------

/// Overdraw a rectangle with a 1-in-3 black diagonal stripe pattern,
/// darkening the white background of disabled menu rows so they read
/// as "unavailable" on a low-contrast EPD.  Black text on top stays
/// black; the surrounding background drops to ~33 % black.
fn dim_rect<D>(display: &mut D, rect: Rectangle) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let x0 = rect.top_left.x;
    let y0 = rect.top_left.y;
    let x1 = x0 + rect.size.width as i32;
    let y1 = y0 + rect.size.height as i32;
    let pixels = (y0..y1).flat_map(move |y| {
        (x0..x1).filter_map(move |x| {
            if (x + y).rem_euclid(3) == 0 {
                Some(Pixel(Point::new(x, y), BLACK))
            } else {
                None
            }
        })
    });
    display.draw_iter(pixels)
}

// ---------------------------------------------------------------------------
// Hint view — the pet says what it needs
// ---------------------------------------------------------------------------

/// Longest pet name the title shows: the popover title budget (18 columns)
/// less " says".
const HINT_NAME_COLS: usize = 13;

/// "<name> says", the name cut to fit the title; "Your pet says" unnamed.
fn hint_title(name: &str) -> heapless::String<24> {
    let mut t: heapless::String<24> = heapless::String::new();
    if name.is_empty() {
        let _ = t.push_str("Your pet");
    } else {
        for c in name.chars().take(HINT_NAME_COLS) {
            let _ = t.push(c);
        }
    }
    let _ = t.push_str(" says");
    t
}

fn draw_hint_view<D>(display: &mut D) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    use super::lifecycle;

    super::note_live_view_drawn();
    // Bring the pet up to date, then ask it.
    if lifecycle::cycle().is_none() {
        return Ok(());
    }
    let Some(hint) = lifecycle::hint() else {
        return Ok(());
    };
    let (one, two) = hint.lines();

    ui::draw_popover_frame(
        display,
        Point::new(MARGIN, MARGIN),
        Size::new(MODAL_W, MODAL_H),
        BORDER,
    )?;
    let inner_x = MARGIN + BORDER as i32;
    let inner_y = MARGIN + BORDER as i32;
    let inner_w = MODAL_W - BORDER * 2;
    let title = hint_title(super::lifecycle::pet_name());
    ui::draw_title_bar(display, title.as_str(), Point::new(inner_x, inner_y), inner_w)?;

    let mid_x = MARGIN + MODAL_W as i32 / 2;
    let body_y = inner_y + TITLE_H + (MODAL_H as i32 - TITLE_H - 2 * BORDER as i32) / 2;
    crate::ui::draw_text(
        display,
        one.as_str(),
        Point::new(mid_x, body_y - 10),
        TEXT_BOLD_BLACK,
        crate::ui::CENTERED,
    )?;
    crate::ui::draw_text(
        display,
        two.as_str(),
        Point::new(mid_x, body_y + 10),
        TEXT_BLACK,
        crate::ui::CENTERED,
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Stats view — 5 labeled bars showing pet health at a glance
// ---------------------------------------------------------------------------

/// Bar width in pixels (modal inner is 128 px; subtract label width + margins).
const BAR_MAX_W: u32 = 60;
/// Bar height — the documented minimum for the 13px inline percentage
/// font to fit without clipping.
const BAR_H: u32 = 15;
/// Vertical spacing between bars (== BAR_H, zero extra gap). Five bars
/// at 15 px from 2 px below the title bar end at y = 107; page 2's
/// condition row (13 px) and the footer follow, still above the frame.
const BAR_SPACING: i32 = 15;
/// Gap between the title bar and the first bar.
const BARS_TOP_GAP: i32 = 2;
/// Bars per stats page.
const STAT_ROWS: i32 = 5;
/// Width of the label column ("Energy", "Health", "Weight" fit with room).
const LABEL_COL_W: i32 = 60;
/// Gap between the last bar and the condition row on page 2.
const CONDITION_GAP: i32 = 1;
/// The condition row's width in columns (the popover body budget).
const CONDITION_COLS: usize = 18;

fn draw_stats_view<D>(display: &mut D) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    use super::lifecycle;
    use super::stat_bar::draw_band_bar;
    use super::stats_pages::{self, Page};

    super::note_live_view_drawn();

    // Get fresh stats (triggers an update if needed).
    let stats = match lifecycle::cycle() {
        Some(s) => s,
        None => return Ok(()),
    };

    // White popover frame + black border.
    ui::draw_popover_frame(
        display,
        Point::new(MARGIN, MARGIN),
        Size::new(MODAL_W, MODAL_H),
        BORDER,
    )?;

    // Title bar: the page name and its number.
    let page = stats_page();
    let inner_x = MARGIN + BORDER as i32;
    let inner_y = MARGIN + BORDER as i32;
    let inner_w = MODAL_W - BORDER * 2;
    ui::draw_title_bar(display, stats_pages::title(page), Point::new(inner_x, inner_y), inner_w)?;

    // Layout: label at left margin, bar to the right of the label column.
    let label_x = inner_x + 4;
    let bar_x = label_x + LABEL_COL_W;

    for (i, row) in stats_pages::bars(page, &stats).iter().enumerate() {
        let y = inner_y + TITLE_H + BARS_TOP_GAP + i as i32 * BAR_SPACING;
        // Label vertically centred against the bar.
        let label_y = y + (BAR_H as i32 - 13) / 2;
        draw_band_bar(
            display,
            row,
            Point::new(label_x, label_y),
            Point::new(bar_x, y),
            Size::new(BAR_MAX_W, BAR_H),
        )?;
    }

    // Page 2 ends with the lasting conditions, just below the last bar.
    if page == Page::Body {
        let y = inner_y + TITLE_H + BARS_TOP_GAP + STAT_ROWS * BAR_SPACING + CONDITION_GAP;
        crate::ui::draw_text(
            display,
            stats.conditions.line(CONDITION_COLS).as_str(),
            Point::new(label_x, y),
            TEXT_BLACK,
            crate::ui::TOP_LEFT,
        )?;
    }

    // Footer: name, generation + age.
    let name = super::lifecycle::pet_name();
    let age_hours = stats.age_ticks / 360;
    let age_days = age_hours / 24;
    // Footer baseline; moved down 8 px so it no longer collides with
    // the bottom of the page-2 condition row (the bars end at y = 107).
    let footer_y = MARGIN + MODAL_H as i32 - BORDER as i32 - 6;
    let mut footer: heapless::String<32> = heapless::String::new();
    // `*` after the name when BPETS_V3.CFG overrode at least one
    // threshold field — lets the player tell at a glance that their
    // pet is running on a non-standard balance.
    let custom_mark = if super::engine::thresholds::is_custom() {
        "*"
    } else {
        ""
    };
    if !name.is_empty() {
        let _ = core::fmt::Write::write_fmt(
            &mut footer,
            format_args!(
                "{}{} | {}d {}h",
                name,
                custom_mark,
                age_days,
                age_hours % 24
            ),
        );
    } else {
        let _ = core::fmt::Write::write_fmt(
            &mut footer,
            format_args!(
                "Gen {}{} | {}d {}h",
                stats.generation,
                custom_mark,
                age_days,
                age_hours % 24
            ),
        );
    }
    crate::ui::draw_text(
        display,
        footer.as_str(),
        Point::new(MARGIN + MODAL_W as i32 / 2, footer_y),
        TEXT_BOLD_BLACK,
        crate::ui::CENTERED_BOTTOM,
    )?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Item, ModalKind, cooldown_string, first_selectable, next_enabled};
    use crate::game::engine::{GameState, PetKind, Phase};
    use crate::game::lifecycle;
    use crate::game::stats_pages::Page;

    fn active_pet() -> GameState {
        let mut s = GameState::new_egg(7, PetKind(0));
        s.phase = Phase::Active;
        s
    }

    #[test]
    fn countdowns_are_readable_up_to_three_days() {
        assert_eq!(cooldown_string(0), None);
        assert_eq!(cooldown_string(5).as_deref(), Some(" 50s"));
        assert_eq!(cooldown_string(59 * 6).as_deref(), Some(" 59m"));
        assert_eq!(cooldown_string(2 * 360).as_deref(), Some(" 2h00"));
        assert_eq!(cooldown_string(2 * 360 + 30).as_deref(), Some(" 2h05"));
        // Rehab: 4320 minutes = 72 h = 25920 ticks (x10 s overflowed u16).
        assert_eq!(cooldown_string(25920).as_deref(), Some(" 72h00"));
    }

    #[test]
    fn rest_menu_offers_sleep_relax_wash_wake() {
        let items = ModalKind::Rest.items();
        for want in [Item::Sleep, Item::Relax, Item::Wash, Item::WakeFromSleep, Item::Cancel] {
            assert!(items.contains(&want), "missing {}", want.label());
        }
        assert_ne!(Item::WakeFromSleep.label(), Item::WakeUp.label());

        let mut s = active_pet();
        let awake = s.stats(0);
        assert!(Item::Sleep.available(&awake));
        assert!(!Item::WakeFromSleep.available(&awake));
        assert!(s.sleep());
        let asleep = s.stats(0);
        assert!(!Item::Sleep.available(&asleep));
        assert!(Item::WakeFromSleep.available(&asleep));
    }

    #[test]
    fn wash_row_follows_the_engine_refusal() {
        let _guard = crate::game::lock_test_state();
        use crate::game::engine::doses::{Reason, Refusal};
        lifecycle::start_new_game(PetKind(0), false, false);
        lifecycle::with_state(|s| {
            s.phase = Phase::Active;
            s.dirt = 0;
            true
        });
        let clean = lifecycle::cycle().unwrap();
        assert!(!Item::Wash.available(&clean));
        assert!(matches!(Item::Wash.reason(&clean), Reason::Refused(Refusal::AlreadyClean)));
        assert_eq!(Item::Wash.cooldown_ticks(&clean), 0);
        lifecycle::with_state(|s| {
            s.dirt = u16::MAX / 2;
            true
        });
        assert!(Item::Wash.available(&lifecycle::cycle().unwrap()));
    }

    #[test]
    fn sleep_row_says_busy_while_an_action_runs() {
        let _guard = crate::game::lock_test_state();
        use crate::game::engine::doses::Reason;
        lifecycle::start_new_game(PetKind(0), false, false);
        lifecycle::with_state(|s| {
            s.phase = Phase::Active;
            true
        });
        assert!(Item::Sleep.available(&lifecycle::cycle().unwrap()));
        lifecycle::with_state(|s| {
            s.dirt = u16::MAX / 2;
            true
        });
        assert!(lifecycle::wash());
        let busy = lifecycle::cycle().unwrap();
        assert!(!Item::Sleep.available(&busy));
        assert!(matches!(Item::Sleep.reason(&busy), Reason::Busy));
        assert!(!lifecycle::sleep());
    }

    #[test]
    fn debug_page_is_the_v3_table() {
        let want = [
            "Engine info", "Skip 10 min", "Skip 1 hour", "Skip 1 day", "Comfort", "Hungry",
            "Thirsty", "Stuffed", "Tired", "Bored", "Dirty", "Fat", "Thin", "Unfit", "Drunk",
            "Hung over", "Groggy", "Injured", "Diabetic", "Dependent", "Clear conditions",
            "Clear cooldowns", "Reset battle record",
        ];
        let items = ModalKind::Debug.items();
        let labels: std::vec::Vec<&str> = items.iter().map(|i| i.label()).collect();
        assert_eq!(labels[..want.len()], want);
        assert_eq!(labels.len(), want.len() + 1);
        assert!(items[want.len()] == Item::Cancel);
    }

    #[test]
    fn a_refused_poke_says_why() {
        use crate::game::engine::DebugPoke;
        use crate::game::engine::doses::Reason;
        let _guard = crate::game::lock_test_state();
        let hungry = ModalKind::Debug
            .items()
            .iter()
            .copied()
            .find(|i| i.label() == "Hungry")
            .unwrap();
        // A fresh egg is still hatching.
        lifecycle::start_new_game(PetKind(0), false, false);
        assert_eq!(lifecycle::debug_poke(DebugPoke::Hungry), Err(Reason::Egg));
        hungry.activate();
        assert_eq!(crate::game::last_reason_for_test(), Reason::Egg);
        lifecycle::with_state(|s| {
            s.phase = Phase::Gone;
            true
        });
        assert_eq!(lifecycle::debug_poke(DebugPoke::Hungry), Err(Reason::Gone));
        hungry.activate();
        assert_eq!(crate::game::last_reason_for_test(), Reason::Gone);
        lifecycle::with_state(|s| {
            s.phase = Phase::Active;
            s.hibernating = true;
            true
        });
        assert_eq!(lifecycle::debug_poke(DebugPoke::Hungry), Err(Reason::Hibernating));
        hungry.activate();
        assert_eq!(crate::game::last_reason_for_test(), Reason::Hibernating);
        super::close();
    }

    #[test]
    fn a_day_through_the_menu() {
        let _guard = crate::game::lock_test_state();
        lifecycle::start_new_game(PetKind(0), false, false);
        lifecycle::with_state(|s| {
            s.phase = Phase::Active;
            s.fullness = u16::MAX / 4;
            s.hydration = u16::MAX / 4;
            s.dirt = u16::MAX / 2;
            s.energy = u16::MAX / 4;
            true
        });
        let st = || lifecycle::cycle().unwrap();
        // Take a menu row, then let its timed action finish.
        let ok = |item: Item| {
            let stats = st();
            assert!(item.available(&stats), "{} unavailable", item.label());
            item.activate();
            super::close();
            lifecycle::debug_skip_minutes(5);
        };
        let before = st();
        ok(Item::FeedFood(crate::game::engine::FoodKind::Apple));
        assert!(st().hunger > before.hunger);
        ok(Item::DrinkChoice(crate::game::engine::DrinkKind::Water));
        assert!(st().water > before.water);
        ok(Item::Wash);
        assert!(st().clean > before.clean);
        ok(Item::Sleep);
        assert!(st().is_sleeping);
        lifecycle::debug_skip_minutes(60);
        let slept = st();
        assert!(slept.tired > before.tired);
        ok(Item::WakeFromSleep);
        assert!(!st().is_sleeping);
        let before_relax = st();
        Item::Relax.activate();
        super::close();
        assert_eq!(st().active_action, Some(crate::game::engine::Action::Relax));
        lifecycle::debug_skip_minutes(5);
        let after_relax = st();
        assert!(after_relax.active_action.is_none());
        assert!(after_relax.tired > before_relax.tired, "relax restores energy");
    }

    /// Rows 0 and 2 disabled, like a Play list with Battle and a
    /// paid game locked out.
    fn sel(i: usize) -> bool {
        !matches!(i, 0 | 2)
    }

    #[test]
    fn open_skips_disabled_first_row() {
        assert_eq!(first_selectable(5, sel), 1);
        assert_eq!(first_selectable(5, |_| true), 0);
        assert_eq!(first_selectable(5, |_| false), 0);
        assert_eq!(first_selectable(0, |_| true), 0);
    }

    #[test]
    fn the_cursor_skips_disabled_rows_and_wraps() {
        let flags = [true, false, false, true, true];
        let en = |i: usize| flags[i];
        // Skips one or several disabled rows, both directions.
        assert_eq!(next_enabled(5, 0, true, en), 3);
        assert_eq!(next_enabled(5, 3, false, en), 0);
        assert_eq!(next_enabled(5, 3, true, en), 4);
        // Wraps from either end onto the first enabled row.
        assert_eq!(next_enabled(5, 4, true, en), 0);
        assert_eq!(next_enabled(5, 0, false, en), 4);
        // Disabled ends wrap past themselves.
        let ends = [false, true, true, false];
        assert_eq!(next_enabled(4, 2, true, |i| ends[i]), 1);
        assert_eq!(next_enabled(4, 1, false, |i| ends[i]), 2);
        // Only the current row enabled: it stays put.
        assert_eq!(next_enabled(5, 2, true, |i| i == 2), 2);
        assert_eq!(next_enabled(5, 2, false, |i| i == 2), 2);
        // Nothing enabled / empty list: stays put.
        assert_eq!(next_enabled(5, 1, true, |_| false), 1);
        assert_eq!(next_enabled(0, 0, false, |_| true), 0);
        // All enabled: plain one-row steps.
        assert_eq!(next_enabled(5, 4, true, |_| true), 0);
        assert_eq!(next_enabled(5, 0, false, |_| true), 4);
        assert_eq!(next_enabled(1, 0, true, |_| true), 0);
    }

    fn cursor_item() -> Item {
        ModalKind::Rest.items()[super::MODAL_POS.load(core::sync::atomic::Ordering::Relaxed) as usize]
    }

    #[test]
    fn the_menu_cursor_never_lands_on_a_disabled_row() {
        let _guard = crate::game::lock_test_state();
        let stats = menu_pet(false, false, |s| s.dirt = 0);
        let items = ModalKind::Rest.items();
        assert!(!Item::Wash.available(&stats));
        assert!(!Item::WakeFromSleep.available(&stats));
        super::open(ModalKind::Rest);
        for _ in 0..2 * items.len() {
            assert!(cursor_item().selectable(Some(&stats)), "down landed on {:?}", cursor_item().label());
            super::cursor_down();
        }
        for _ in 0..2 * items.len() {
            assert!(cursor_item().selectable(Some(&stats)), "up landed on {:?}", cursor_item().label());
            super::cursor_up();
        }
        // Relax -> (Wash, Wake shut) -> Cancel, and back.
        super::MODAL_POS.store(1, core::sync::atomic::Ordering::Relaxed);
        super::cursor_down();
        assert!(cursor_item() == Item::Cancel);
        super::cursor_up();
        assert!(cursor_item() == Item::Relax);
        super::close();
        lifecycle::set_state_for_test(None);
    }

    #[test]
    fn a_menu_opens_on_an_enabled_row_when_the_first_is_shut() {
        let _guard = crate::game::lock_test_state();
        // Asleep: Sleep is shut, Relax/Wash may be too; the cursor starts on a usable row.
        let stats = menu_pet(false, false, |s| s.is_sleeping = true);
        assert!(!Item::Sleep.available(&stats));
        super::open(ModalKind::Rest);
        assert!(cursor_item().selectable(Some(&stats)));
        assert!(cursor_item() != Item::Sleep);
        super::close();
        lifecycle::set_state_for_test(None);
    }

    #[test]
    fn an_action_end_shows_its_cause_and_logs_it() {
        let _guard = crate::game::lock_test_state();
        use crate::game::engine::{causes::Cause, FoodKind};
        lifecycle::start_new_game(PetKind(0), false, false);
        lifecycle::with_state(|s| {
            s.phase = Phase::Active;
            s.fullness = 70 * 655;
            true
        });
        Item::FeedFood(FoodKind::Pizza).activate();
        super::close();
        lifecycle::debug_skip_minutes(5);
        lifecycle::cycle();
        assert_eq!(crate::game::current_toast().unwrap_or(crate::game::Toast::None), crate::game::Toast::Cause);
        assert_eq!(crate::game::last_cause_for_test(), Some(Cause::FoodComa));
        assert_eq!(lifecycle::diary().len(), 1);
        assert_eq!(
            lifecycle::diary().newest_first().next().unwrap().cause,
            Cause::FoodComa
        );
    }

    #[test]
    fn a_refused_row_says_why_in_the_title() {
        use crate::game::engine::DrinkKind;
        let _guard = crate::game::lock_test_state();
        let _stats = menu_pet(false, false, |s| s.hydration = pct(95));
        super::open(ModalKind::Drink);
        let water = Item::DrinkChoice(DrinkKind::Water);
        let row = ModalKind::Drink.items().iter().position(|i| *i == water).unwrap();
        super::MODAL_POS.store(row as u8, core::sync::atomic::Ordering::Relaxed);
        super::activate();
        let stats = lifecycle::cycle().unwrap();
        assert_eq!(
            super::title_text(ModalKind::Drink, Some(water), Some(&stats), None).as_str(),
            "not thirsty"
        );
        // The reason comes before a risk and the menu title.
        let t = super::title_text(ModalKind::Drink, Some(water), Some(&stats), Some(Cause::FoodComa));
        assert_eq!(t.as_str(), "not thirsty");
        lifecycle::set_state_for_test(None);
    }

    /// Put the cursor on `item` in the open Hibernate menu and press it.
    fn press_in_hibernate_menu(item: Item) {
        super::open(ModalKind::Hibernate);
        let row = ModalKind::Hibernate
            .items()
            .iter()
            .position(|i| *i == item)
            .unwrap();
        super::MODAL_POS.store(row as u8, core::sync::atomic::Ordering::Relaxed);
        super::activate();
    }

    #[test]
    fn the_hibernate_menu_has_the_demo_switch_before_cancel() {
        let items = ModalKind::Hibernate.items();
        let demo = items
            .iter()
            .position(|i| *i == Item::Demo)
            .expect("no demo row");
        assert!(items[demo + 1] == Item::Cancel);
    }

    #[test]
    fn the_demo_row_switches_demo_on_and_off() {
        let _guard = crate::game::lock_test_state();
        let stats = menu_pet(false, false, |_| {});
        assert!(Item::Demo.selectable(Some(&stats)));
        assert_eq!(Item::Demo.label(), lifecycle::demo_menu_label());
        press_in_hibernate_menu(Item::Demo);
        assert!(lifecycle::is_demo());
        let stats = lifecycle::cycle().unwrap();
        assert!(Item::Demo.selectable(Some(&stats)));
        assert_eq!(Item::Demo.label(), "Demo mode: on");
        press_in_hibernate_menu(Item::Demo);
        assert!(!lifecycle::is_demo());
        lifecycle::set_state_for_test(None);
    }

    #[test]
    fn a_refused_demo_row_says_why_and_cannot_be_pressed() {
        let _guard = crate::game::lock_test_state();
        let stats = menu_pet(false, false, |s| s.health = pct(5));
        assert!(!Item::Demo.selectable(Some(&stats)));
        assert_eq!(Item::Demo.label(), "Demo: fix me first");
        lifecycle::set_state_for_test(None);

        let _stats = menu_pet(false, false, |_| {});
        lifecycle::hibernate();
        let stats = lifecycle::cycle().unwrap();
        assert!(!Item::Demo.selectable(Some(&stats)));
        assert_eq!(Item::Demo.label(), "Demo: frozen");
        lifecycle::set_state_for_test(None);
    }

    #[test]
    fn only_pets_row_stays_in_demo_mode() {
        // HAX stays live in demo, so the row earns there too.
        let _guard = crate::game::lock_test_state();
        let _stats = menu_pet(true, false, |_| {});
        assert!(ModalKind::Play.items().contains(&Item::OnlyPets));
        lifecycle::with_state(|s| {
            s.demo.on = true;
            true
        });
        assert!(lifecycle::money_enabled());
        assert!(ModalKind::Play.items().contains(&Item::OnlyPets));
        lifecycle::set_state_for_test(None);
    }

    #[test]
    fn every_reason_fits_the_title() {
        use crate::game::engine::doses::{Reason, Refusal};
        let mut all = vec![
            Reason::Wait(72 * 3600),
            Reason::CantAfford,
            Reason::Busy,
            Reason::Hibernating,
            Reason::Egg,
            Reason::Gone,
        ];
        for r in [
            Refusal::NotThirsty,
            Refusal::AlreadyClean,
            Refusal::Injured,
            Refusal::Asleep,
            Refusal::NoNeed,
            Refusal::Tired,
            Refusal::FixMeFirst,
        ] {
            all.push(Reason::Refused(r));
        }
        for r in all {
            assert!(r.text().chars().count() <= 18, "{:?}", r.text());
        }
    }

    #[test]
    fn a_cause_drained_as_the_menu_opens_is_not_toasted_under_it() {
        let _guard = crate::game::lock_test_state();
        let _stats = menu_pet(false, false, |_| {});
        lifecycle::with_state(|s| {
            s.causes.push(Cause::FoodComa);
            true
        });
        super::open(ModalKind::Stats);
        assert_eq!(crate::game::lifecycle::diary().len(), 1);
        assert_eq!(crate::game::current_toast().unwrap_or(crate::game::Toast::None), crate::game::Toast::None);
        lifecycle::set_state_for_test(None);
    }

    #[test]
    fn a_new_pet_starts_an_empty_diary() {
        let _guard = crate::game::lock_test_state();
        use crate::game::engine::causes::Cause;
        lifecycle::start_new_game(PetKind(0), false, false);
        lifecycle::log_cause(Cause::Groggy, 0);
        assert_eq!(lifecycle::diary().len(), 1);
        lifecycle::start_new_game(PetKind(0), false, false);
        assert_eq!(lifecycle::diary().len(), 0);
        lifecycle::log_cause(Cause::Groggy, 0);
        lifecycle::new_generation(PetKind(0), false, false);
        assert_eq!(lifecycle::diary().len(), 0);
    }

    #[test]
    fn stats_menu_has_no_health_status_row() {
        let labels: std::vec::Vec<&str> =
            ModalKind::Stats.items().iter().map(|i| i.label()).collect();
        assert_eq!(
            labels,
            ["View stats", "Hint", "Diary", "Rolled stats", "Friends", "Cancel"]
        );
    }

    #[test]
    fn left_right_flips_the_stats_pages() {
        let _guard = crate::game::lock_test_state();
        lifecycle::set_state_for_test(Some(active_pet()));
        super::open(ModalKind::Stats);
        super::flip_page();
        assert_eq!(super::stats_page(), Page::Needs, "the list has no pages");
        super::activate(); // first row: View stats
        assert!(super::live_modal_view_open());
        assert_eq!(super::stats_page(), Page::Needs);
        crate::FULL_REFRESH_PENDING.store(false, core::sync::atomic::Ordering::Relaxed);
        super::flip_page();
        assert_eq!(super::stats_page(), Page::Body);
        assert!(
            !crate::FULL_REFRESH_PENDING.load(core::sync::atomic::Ordering::Relaxed),
            "a flip is one partial refresh"
        );
        // The live-view deadline covers page 2: its condition row drifts.
        crate::game::LIVE_DRAWN_TICK.store(1000, core::sync::atomic::Ordering::Relaxed);
        assert!(crate::game::live_view_open());
        assert_eq!(
            crate::game::next_ui_deadline_tick(),
            1000 + crate::game::pacing::IDLE_FRAME_TICKS
        );
        super::flip_page();
        assert_eq!(super::stats_page(), Page::Needs);
        super::flip_page();
        assert_eq!(super::stats_page(), Page::Body);
        super::activate(); // any activation closes the view, back to the list
        assert!(!super::live_modal_view_open());
        super::flip_page();
        assert_eq!(super::stats_page(), Page::Needs, "no change on the list");
        super::activate();
        assert_eq!(super::stats_page(), Page::Needs, "reopening starts on page 1");
        super::close();
        lifecycle::set_state_for_test(None);
    }

    #[test]
    fn the_body_page_draws_the_condition_row() {
        use embedded_graphics::prelude::*;
        use embedded_graphics::primitives::Rectangle;
        let _guard = crate::game::lock_test_state();
        let mut pet = active_pet();
        pet.injured_min = 30;
        lifecycle::set_state_for_test(Some(pet));
        super::open(ModalKind::Stats);
        super::activate();
        // Inner y 12 + title 18 + gap 2 + five bars + 1: the row is 13 px tall.
        let row = Rectangle::new(Point::new(14, 108), Size::new(124, 12));
        let mut c = crate::game::test_canvas::Canvas::new();
        super::draw_modal(&mut c).unwrap();
        assert!(!c.any(row, crate::BLACK), "page 1 has no condition row");
        super::flip_page();
        let mut c = crate::game::test_canvas::Canvas::new();
        super::draw_modal(&mut c).unwrap();
        assert!(c.any(row, crate::BLACK), "page 2 names the injury");
        super::close();
        lifecycle::set_state_for_test(None);
    }

    // ── Task 8: risk markers ─────────────────────────────────────────────

    use crate::game::engine::causes::{Cause, StrainWhy, Unsettled};
    use crate::game::engine::needs::P;
    use crate::game::engine::FoodKind;

    fn pct(p: i32) -> u16 {
        (p * P) as u16
    }

    /// A fresh active pet in the global slot, shaped by `f`; its stats.
    fn menu_pet(
        money: bool,
        hard: bool,
        f: impl FnOnce(&mut GameState),
    ) -> crate::game::engine::PetStats {
        lifecycle::start_new_game(PetKind(0), money, hard);
        lifecycle::with_state(|s| {
            s.phase = Phase::Active;
            s.is_sleeping = false;
            s.money = 1000;
            s.fullness = pct(50);
            s.hydration = pct(50);
            s.energy = pct(90);
            f(s);
            true
        });
        lifecycle::cycle().unwrap()
    }

    #[test]
    fn battle_row_shows_the_form() {
        let _guard = crate::game::lock_test_state();
        let mut stats = menu_pet(false, false, |_| {});
        let form = lifecycle::battle_form().expect("a pet is active");
        stats.can_battle = true;
        stats.cooldown_battle = 0;
        let want = format!("Battle: {}", form.text());
        assert_eq!(super::row_text(Item::Battle, Some(&stats)).as_str(), want);
        // Not available: the plain label (the reason shows when selected).
        stats.can_battle = false;
        assert_eq!(super::row_text(Item::Battle, Some(&stats)).as_str(), "Battle");
        // Every form fits the 17 columns with its label.
        use crate::game::engine::combat::Form;
        for f in [Form::Top, Form::In, Form::Off, Form::Poor] {
            assert!(format!("Battle: {}", f.text()).len() <= 17);
        }
    }

    #[test]
    fn a_half_full_pet_marks_pizza_with_the_food_coma() {
        let _guard = crate::game::lock_test_state();
        let stats = menu_pet(false, false, |_| {});
        let pizza = Item::FeedFood(FoodKind::Pizza);
        assert_eq!(pizza.risk(&stats), Some(Cause::FoodComa));
        assert_eq!(Item::FeedFood(FoodKind::Salad).risk(&stats), None);
        assert_eq!(Item::FeedFood(FoodKind::Cake).risk(&stats), None);
        assert_eq!(
            super::title_text(ModalKind::Feed, Some(pizza), Some(&stats), pizza.risk(&stats))
                .as_str(),
            "risk: food coma"
        );
        // Draw it: a mark on the Pizza row's gutter, none on Salad's.
        super::open(ModalKind::Feed);
        super::MODAL_POS.store(5, core::sync::atomic::Ordering::Relaxed); // Cancel
        let mut c = crate::game::test_canvas::Canvas::new();
        super::draw_modal(&mut c).unwrap();
        let row = |i: i32| super::risk_mark_rect(30 + 16 * i, 0);
        assert!(c.any(row(3), crate::BLACK), "Pizza row has the mark");
        assert!(!c.any(row(0), crate::BLACK), "Salad row has none");
        assert!(!c.any(row(4), crate::BLACK), "Cake row has none");
        super::close();
        lifecycle::set_state_for_test(None);
    }

    /// Draw the Feed page with the cursor on `pos` and check the mark of the
    /// risky `food` row: an `ink` mark on a `ground` row, with a `ground` column
    /// on each side of it (the black border starts two columns right of it).
    fn check_mark(money: bool, hard: bool, food: FoodKind, selected: bool) {
        use crate::{BLACK, WHITE};
        let stats = menu_pet(money, hard, |_| {});
        let item = Item::FeedFood(food);
        assert!(item.risk(&stats).is_some(), "{food:?} is risky at fullness 50");
        let row = ModalKind::Feed.items().iter().position(|i| *i == item).unwrap();
        super::open(ModalKind::Feed);
        let pos = if selected { row } else { 5 };
        super::MODAL_POS.store(pos as u8, core::sync::atomic::Ordering::Relaxed);
        let mut c = crate::game::test_canvas::Canvas::new();
        super::draw_modal(&mut c).unwrap();
        let top = 30 + 16 * row as i32;
        let text = super::row_text(item, Some(&stats));
        let text_end = 12 + text.chars().count() as i32 * 7;
        let mark = super::risk_mark_rect(top, text_end);
        let (ink, ground) = if selected { (WHITE, BLACK) } else { (BLACK, WHITE) };
        let (x0, x1) = (mark.top_left.x, mark.top_left.x + mark.size.width as i32);
        let (y0, y1) = (mark.top_left.y, mark.top_left.y + mark.size.height as i32);
        // The bar is solid from the top down to the gap before the dot.
        for y in y0..y1 - 3 {
            for x in x0..x1 {
                assert_eq!(c.get(x, y), ink, "bar ({x},{y}) {text:?} sel={selected}");
            }
        }
        for y in y0..y1 {
            assert_eq!(c.get(x0 - 1, y), ground, "left of mark, {text:?} sel={selected}");
            assert_eq!(c.get(x1, y), ground, "right of mark, {text:?} sel={selected}");
            assert_eq!(c.get(x1 + 2, y), BLACK, "the border follows");
        }
        // No text ink right of the mark's left neighbour column.
        for y in top..top + 16 {
            for x in x0 - 1..x0 {
                assert_eq!(c.get(x, y), ground, "text ink at ({x},{y})");
            }
        }
        super::close();
        lifecycle::set_state_for_test(None);
    }

    #[test]
    fn the_mark_stays_clear_of_the_text_and_the_border() {
        let _guard = crate::game::lock_test_state();
        for money in [false, true] {
            for hard in [false, true] {
                for food in [FoodKind::Frikandel, FoodKind::Pizza] {
                    for selected in [false, true] {
                        check_mark(money, hard, food, selected);
                    }
                }
            }
        }
    }

    #[test]
    fn exercise_row_warns_before_the_tear() {
        let _guard = crate::game::lock_test_state();
        let stats = menu_pet(false, false, |s| {
            s.strain = pct(40);
            s.fullness = pct(30);
        });
        assert_eq!(
            Item::ExerciseNow.risk(&stats),
            Some(Cause::TornMuscle(StrainWhy::Hungry))
        );
        lifecycle::set_state_for_test(None);
    }

    #[test]
    fn wake_row_warns_grumpy_under_fifty() {
        let _guard = crate::game::lock_test_state();
        let stats = menu_pet(false, false, |s| {
            s.energy = pct(30);
            assert!(s.sleep());
        });
        assert!(stats.is_sleeping);
        assert_eq!(Item::WakeFromSleep.risk(&stats), Some(Cause::Grumpy));
        lifecycle::set_state_for_test(None);
    }

    #[test]
    fn sleep_row_warns_after_drinks() {
        let _guard = crate::game::lock_test_state();
        // Booze 50, not 35: a booze-35 night is fine in the real engine.
        let stats = menu_pet(false, false, |s| {
            s.energy = pct(30);
            s.booze = pct(50);
        });
        assert_eq!(
            Item::Sleep.risk(&stats),
            Some(Cause::BadNight(Unsettled::Drinks))
        );
        lifecycle::set_state_for_test(None);
    }

    #[test]
    fn marked_rows_still_work() {
        let _guard = crate::game::lock_test_state();
        let stats = menu_pet(false, false, |_| {});
        let pizza = Item::FeedFood(FoodKind::Pizza);
        assert!(pizza.risk(&stats).is_some());
        assert!(pizza.available(&stats));
        assert!(pizza.selectable(Some(&stats)));
        pizza.activate();
        assert_eq!(
            lifecycle::cycle().unwrap().active_action,
            Some(crate::game::engine::Action::Feed)
        );
        super::close();
        lifecycle::set_state_for_test(None);
    }

    #[test]
    fn unavailable_rows_carry_no_risk() {
        let _guard = crate::game::lock_test_state();
        let stats = menu_pet(true, false, |s| s.money = 0);
        let pizza = Item::FeedFood(FoodKind::Pizza);
        assert!(!pizza.available(&stats));
        assert_eq!(pizza.risk(&stats), None);
        lifecycle::set_state_for_test(None);
    }

    #[test]
    fn frikandel_has_a_full_name_in_the_title_only() {
        let _guard = crate::game::lock_test_state();
        let stats = menu_pet(false, false, |_| {});
        let frik = Item::FeedFood(FoodKind::Frikandel);
        assert_eq!(frik.label(), "Frikandel spec");
        let t = super::title_text(ModalKind::Feed, Some(frik), Some(&stats), None);
        assert_eq!(t.as_str(), "Frikandel speciaal");
        assert!(t.len() <= 18, "fits the 18-column title");
        let salad = Item::FeedFood(FoodKind::Salad);
        assert_eq!(
            super::title_text(ModalKind::Feed, Some(salad), Some(&stats), None).as_str(),
            "Salad"
        );
        assert_eq!(
            super::title_text(ModalKind::Feed, Some(Item::Cancel), Some(&stats), None).as_str(),
            "Feed"
        );
        assert_eq!(
            super::title_text(ModalKind::Heal, Some(Item::Rehab), Some(&stats), None).as_str(),
            ModalKind::Heal.title()
        );
        lifecycle::set_state_for_test(None);
    }

    /// Pet states that put the longest real wait on a row: every engine
    /// cooldown at its longest, and each action running at its longest.
    fn worst_waits() -> std::vec::Vec<std::boxed::Box<dyn Fn(&mut GameState)>> {
        use crate::game::engine::thresholds as t;
        let mut all: std::vec::Vec<std::boxed::Box<dyn Fn(&mut GameState)>> = vec![
            std::boxed::Box::new(|_| {}),
            std::boxed::Box::new(|s| {
                s.diabetic = true;
                s.med_cd_min = t::DOSE_MED_CD_MIN();
                s.ozempic_cd_min = t::DOSE_OZEMPIC_CD_MIN();
                s.rehab_min = t::DOSE_REHAB_MIN();
                s.insulin_min = t::DOSE_INSULIN_MIN();
                s.cooldown_battle = t::BATTLE_COOLDOWN();
                s.cooldown_tictactoe = t::MINIGAME_COOLDOWN();
                s.cooldown_lightsout = t::MINIGAME_COOLDOWN();
                s.cooldown_blackhole = t::MINIGAME_COOLDOWN();
                s.cooldown_bornjeweled = t::MINIGAME_COOLDOWN();
            }),
        ];
        for kind in [ModalKind::Feed, ModalKind::Drink, ModalKind::Heal, ModalKind::Play, ModalKind::Rest, ModalKind::Exercise] {
            for item in kind.items() {
                if let Some((action, _)) = item.action() {
                    all.push(std::boxed::Box::new(move |s| {
                        s.active_action = Some(action);
                        s.action_ticks_remaining = u8::MAX;
                    }));
                }
            }
        }
        all
    }

    /// Every (hard, money, worst-wait state) combination.
    fn wait_scenarios() -> std::vec::Vec<(bool, bool, std::boxed::Box<dyn Fn(&mut GameState)>)> {
        let mut out = std::vec::Vec::new();
        for hard in [false, true] {
            for money in [true, false] {
                for worst in worst_waits() {
                    out.push((hard, money, worst));
                }
            }
        }
        out
    }

    #[test]
    fn rows_and_risk_titles_fit() {
        let _guard = crate::game::lock_test_state();
        let (ink_w, inner_x, indent_gap) = (
            super::TEXT_BOLD_BLACK.font.character_size.width as i32,
            super::MARGIN + super::BORDER as i32,
            4,
        );
        for (hard, money, worst) in wait_scenarios() {
            let stats = menu_pet(money, hard, |s| worst(s));
            for kind in [
                ModalKind::Feed,
                ModalKind::Drink,
                ModalKind::Heal,
                ModalKind::Play,
                ModalKind::Rest,
                ModalKind::Exercise,
            ] {
                for item in kind.items() {
                    let text = super::row_text(*item, Some(&stats));
                    let cols = text.chars().count();
                    let frik = matches!(item, Item::FeedFood(FoodKind::Frikandel));
                    let budget = if frik && !hard { 18 } else { 17 };
                    assert!(cols <= budget, "{:?} {text:?} is {cols} cols", kind.title());
                    if frik && !hard && money && item.available(&stats) {
                        assert_eq!(text.as_str(), "Frikandel spec 10H");
                    }
                    assert!(!text.contains('('), "{text:?}: no parentheses");
                    // The gutter mark must not touch the row text.
                    let indent = if kind == ModalKind::Feed { 0 } else { indent_gap };
                    let text_end = inner_x + indent + cols as i32 * ink_w;
                    let mark = super::risk_mark_rect(0, text_end);
                    assert!(text_end <= mark.top_left.x, "{text:?} runs into the mark");
                    assert!(
                        mark.top_left.x + mark.size.width as i32 <= inner_x + 128,
                        "the mark stays inside the popover"
                    );
                }
            }
        }
        for cause in Cause::all().filter(|c| c.is_warning()) {
            let t = super::title_text(ModalKind::Feed, None, None, Some(cause));
            assert!(t.chars().count() <= 18, "{t:?}");
        }
        lifecycle::set_state_for_test(None);
    }

    // ── Task 9: the hint ─────────────────────────────────────────────────

    /// The menu rows each remedy stands for.
    fn rows_of(r: crate::game::engine::hint::Remedy) -> std::vec::Vec<Item> {
        use crate::game::engine::hint::Remedy;
        match r {
            Remedy::Feed => FoodKind::ALL.iter().map(|&f| Item::FeedFood(f)).collect(),
            Remedy::Water => vec![Item::DrinkChoice(crate::game::engine::DrinkKind::Water)],
            Remedy::Sleep => vec![Item::Sleep],
            Remedy::Play => vec![Item::PlayNow],
            Remedy::Game => vec![
                Item::TicTacToe,
                Item::LightsOut,
                Item::BlackHole,
                Item::BornJeweled,
            ],
            Remedy::Relax => vec![Item::Relax],
            Remedy::Wash => vec![Item::Wash],
            Remedy::Exercise => vec![Item::ExerciseNow],
            Remedy::Medicine => vec![Item::GiveMedicine],
            Remedy::Insulin => vec![Item::GiveMedication],
            Remedy::Rehab => vec![Item::Rehab],
            Remedy::Defrost => vec![Item::WakeUp],
        }
    }

    const ALL_REMEDIES: [crate::game::engine::hint::Remedy; 12] = {
        use crate::game::engine::hint::Remedy::*;
        [Feed, Water, Sleep, Play, Game, Relax, Wash, Exercise, Medicine, Insulin, Rehab, Defrost]
    };

    type Shape = fn(&mut GameState);

    #[test]
    fn hint_and_menu_agree() {
        let _guard = crate::game::lock_test_state();
        let shapes: [(&str, bool, bool, Shape); 17] = [
            ("calm", false, false, |_| {}),
            ("hungry", true, false, |s| s.fullness = pct(20)),
            ("thirsty", true, true, |s| s.hydration = pct(20)),
            ("tired", false, false, |s| s.energy = pct(10)),
            ("bored", false, false, |s| s.fun = pct(10)),
            ("dirty", false, false, |s| s.dirt = pct(80)),
            ("injured", false, false, |s| s.injured_min = 600),
            ("injured cd", false, false, |s| {
                s.injured_min = 600;
                s.med_cd_min = 120;
            }),
            ("diabetic", true, false, |s| s.diabetic = true),
            ("diabetic hard", true, true, |s| s.diabetic = true),
            ("dependent", true, true, |s| s.dependent = true),
            ("in rehab", true, false, |s| {
                s.dependent = true;
                s.rehab_min = 900;
            }),
            ("asleep", false, false, |s| s.is_sleeping = true),
            ("busy", false, false, |s| {
                s.active_action = Some(crate::game::engine::Action::Play);
                s.action_ticks_remaining = 30;
            }),
            ("broke", true, false, |s| {
                s.money = 0;
                s.fullness = pct(20);
                s.diabetic = true;
            }),
            ("games cooling", false, false, |s| {
                s.cooldown_tictactoe = 9;
                s.cooldown_lightsout = 9;
                s.cooldown_blackhole = 9;
            }),
            ("frozen", false, false, |s| s.hibernating = true),
        ];
        for (name, money, hard, shape) in shapes {
            let stats = menu_pet(money, hard, shape);
            for r in ALL_REMEDIES {
                let mut reason = None;
                lifecycle::with_state(|s| {
                    reason = s.remedy_reason(r);
                    false
                });
                let menu = rows_of(r).iter().any(|i| i.available(&stats));
                assert_eq!(reason.is_none(), menu, "{name}: {r:?} reason {reason:?}");
            }
        }
        // Money one under, on and one over every price, in both modes.
        let edges = [
            0u32, 1, 2, 9, 10, 11, 14, 15, 16, 19, 20, 21, 44, 45, 46, 74, 75, 76,
        ];
        for hard in [false, true] {
            for money in edges {
                for injured in [false, true] {
                    let stats = menu_pet(true, hard, move |s| {
                        s.money = money;
                        s.fullness = pct(20);
                        s.hydration = pct(20);
                        s.diabetic = true;
                        s.dependent = true;
                        if injured {
                            s.injured_min = 600;
                        }
                    });
                    for r in ALL_REMEDIES {
                        let mut reason = None;
                        lifecycle::with_state(|s| {
                            reason = s.remedy_reason(r);
                            false
                        });
                        let menu = rows_of(r).iter().any(|i| i.available(&stats));
                        assert_eq!(
                            reason.is_none(),
                            menu,
                            "hard={hard} money={money} injured={injured}: {r:?} reason {reason:?}"
                        );
                    }
                }
            }
        }
        lifecycle::set_state_for_test(None);
    }

    #[test]
    fn hint_row_opens_the_speech_view() {
        let _guard = crate::game::lock_test_state();
        let mut pet = active_pet();
        pet.is_sleeping = false;
        pet.fullness = pct(20);
        lifecycle::set_state_for_test(Some(pet));
        lifecycle::set_pet_name(b"Twelve_chars");
        let labels: std::vec::Vec<&str> =
            ModalKind::Stats.items().iter().map(|i| i.label()).collect();
        assert_eq!(
            labels,
            ["View stats", "Hint", "Diary", "Rolled stats", "Friends", "Cancel"]
        );
        // Names that fit, and names that are cut to fit.
        assert_eq!(super::hint_title("Twelve_chars").as_str(), "Twelve_chars says");
        assert_eq!(super::hint_title("").as_str(), "Your pet says");
        assert_eq!(super::hint_title("A_very_long_name_indeed").chars().count(), 18);
        assert!(super::hint_title("A_very_long_name_indeed").ends_with(" says"));

        super::open(ModalKind::Stats);
        super::cursor_down();
        super::activate();
        assert!(super::live_modal_view_open());
        let mut c = crate::game::test_canvas::Canvas::new();
        super::draw_modal(&mut c).unwrap();
        use embedded_graphics::prelude::*;
        use embedded_graphics::primitives::Rectangle;
        let body = Rectangle::new(Point::new(12, 32), Size::new(128, 98));
        assert!(c.any(body, crate::BLACK), "the speech is drawn");
        // Any button returns to the Stats list; it does not close the modal.
        super::cursor_down();
        assert!(!super::live_modal_view_open());
        assert!(super::is_open());
        super::activate();
        assert!(super::live_modal_view_open());
        super::flip_page();
        assert!(!super::live_modal_view_open());
        super::activate();
        assert!(super::dismiss_hint());
        assert!(super::is_open());
        assert!(!super::dismiss_hint(), "a second Cancel is not a hint dismiss");
        super::activate();
        super::activate();
        assert!(!super::live_modal_view_open());
        assert!(super::is_open());
        super::close();
        lifecycle::set_state_for_test(None);
    }

    #[test]
    fn the_hint_view_is_redrawn_within_an_idle_frame() {
        let _guard = crate::game::lock_test_state();
        lifecycle::set_state_for_test(Some(active_pet()));
        super::open(ModalKind::Stats);
        super::cursor_down();
        super::activate();
        let now = lifecycle::now_tick();
        crate::game::LIVE_DRAWN_TICK.store(now, core::sync::atomic::Ordering::Relaxed);
        assert!(crate::game::live_view_open());
        assert!(
            crate::game::next_ui_deadline_tick() <= now + crate::game::pacing::IDLE_FRAME_TICKS
        );
        super::close();
        assert!(!crate::game::live_view_open());
        lifecycle::set_state_for_test(None);
    }
}
