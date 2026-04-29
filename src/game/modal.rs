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
use embedded_graphics::primitives::{PrimitiveStyle, Rectangle};
use embedded_graphics::text::{Alignment, Baseline, Text, TextStyleBuilder};

use super::nav::Row;
use crate::ui::{self, TEXT_BLACK, TEXT_BOLD_BLACK, TEXT_WHITE};
use crate::{BLACK, TriColor};

// ── Modal kind
// ────────────────────────────────────────────────────────────────

/// Which in-game modal is currently open.  Stored as a `u8` in [`MODAL_KIND`].
///
/// Layout:
///   Top row (info/meta):    Stats, Hibernate, (empty), (empty)
///   Bottom row (actions):   Feed,  Heal,      Play,    Rest
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
        }
    }

    fn items(self) -> &'static [Item] {
        match self {
            Self::Stats => &[Item::ViewStats, Item::RolledStats, Item::Cancel],
            Self::Hibernate => &[Item::Hibernate, Item::WakeUp, Item::Cancel],
            Self::Feed => &[Item::FeedNow, Item::Cancel],
            Self::Heal => &[Item::GiveMedicine, Item::Cancel],
            Self::Play => &[
                Item::PlayNow,
                Item::TicTacToe,
                Item::LightsOut,
                Item::BlackHole,
                Item::Nim,
                Item::PlayMusic,
                Item::Cancel,
            ],
            Self::Music => &[
                Item::Song(crate::SONG_STARTUP_INDEX),
                Item::Song(crate::SONG_RICKROLL_INDEX),
                Item::Song(crate::SONG_IMPERIAL_MARCH_INDEX),
                Item::Song(crate::SONG_SANDSTORM_INDEX),
                Item::Song(crate::SONG_PINK_PANTHER_INDEX),
                Item::Song(crate::SONG_TROLOLO_INDEX),
                Item::Cancel,
            ],
            Self::Rest => &[Item::Sleep, Item::Relax, Item::Cancel],
            Self::None => &[],
        }
    }
}

// ── Items
// ──────────────────────────────────────────────────────────────────────

/// One row in the modal's item list.  Each variant carries its own
/// label, availability check, cooldown source, and activation handler
/// — no string matching anywhere else in the file.
#[derive(Clone, Copy)]
enum Item {
    Cancel,
    ViewStats,
    RolledStats,
    FeedNow,
    GiveMedicine,
    Sleep,
    Relax,
    PlayNow,
    TicTacToe,
    LightsOut,
    BlackHole,
    Nim,
    PlayMusic,
    /// Buzzer song index passed to [`crate::fw::buzzer::play`].
    Song(u8),
    Hibernate,
    WakeUp,
}

impl Item {
    fn label(self) -> &'static str {
        match self {
            Self::Cancel => "Cancel",
            Self::ViewStats => "View stats",
            Self::RolledStats => "Rolled stats",
            Self::FeedNow => "Feed now",
            Self::GiveMedicine => "Give medicine",
            Self::Sleep => "Sleep",
            Self::Relax => "Relax",
            Self::PlayNow => "Play now",
            Self::TicTacToe => "Tic Tac Toe",
            Self::LightsOut => "Lights Out",
            Self::BlackHole => "Black Hole",
            Self::Nim => "Nim",
            Self::PlayMusic => "Play music",
            Self::Song(crate::SONG_STARTUP_INDEX) => "Startup",
            Self::Song(crate::SONG_RICKROLL_INDEX) => "Rickroll",
            Self::Song(crate::SONG_IMPERIAL_MARCH_INDEX) => "Imp. March",
            Self::Song(crate::SONG_SANDSTORM_INDEX) => "Sandstorm",
            Self::Song(crate::SONG_PINK_PANTHER_INDEX) => "Pink Panther",
            Self::Song(crate::SONG_TROLOLO_INDEX) => "Trololo",
            Self::Song(_) => "?",
            Self::Hibernate => "Hibernate",
            Self::WakeUp => "Wake up",
        }
    }

    /// Is the action currently available?  Cooldown-gated items
    /// return false until the cooldown decays to 0.
    fn available(self, stats: &super::engine::PetStats) -> bool {
        match self {
            Self::Cancel
            | Self::ViewStats
            | Self::RolledStats
            | Self::PlayMusic
            | Self::Song(_) => true,
            Self::FeedNow => stats.can_feed,
            Self::GiveMedicine => stats.can_heal,
            Self::Sleep => stats.can_sleep,
            Self::Relax => stats.can_relax,
            Self::PlayNow => stats.can_play,
            Self::TicTacToe => stats.can_play_tictactoe,
            Self::LightsOut => stats.can_play_lightsout,
            Self::BlackHole => stats.can_play_blackhole,
            Self::Nim => stats.can_play_nim,
            Self::Hibernate => !stats.hibernating,
            Self::WakeUp => stats.hibernating,
        }
    }

    /// Time-until-ready in 10-second ticks for a disabled item.
    /// During the in-progress action this is `action_ticks_remaining`;
    /// after the action ends, the post-action cooldown takes over and
    /// is reported instead.  0 = ready / not gated by a cooldown.
    fn cooldown_ticks(self, stats: &super::engine::PetStats) -> u16 {
        use super::engine::Action;
        let action_remaining = |a: Action| -> u16 {
            if stats.active_action == Some(a) {
                stats.action_ticks_remaining as u16
            } else {
                0
            }
        };
        match self {
            Self::FeedNow => action_remaining(Action::Feed).max(stats.cooldown_feed),
            Self::GiveMedicine => action_remaining(Action::Heal).max(stats.cooldown_heal),
            Self::Relax => action_remaining(Action::Relax).max(stats.cooldown_relax),
            Self::PlayNow => action_remaining(Action::Play).max(stats.cooldown_play),
            Self::TicTacToe => stats.cooldown_tictactoe,
            Self::LightsOut => stats.cooldown_lightsout,
            Self::BlackHole => stats.cooldown_blackhole,
            Self::Nim => stats.cooldown_nim,
            _ => 0,
        }
    }

    fn activate(self) {
        use super::lifecycle;
        match self {
            Self::Cancel => close(),
            Self::ViewStats => STATS_VIEW.store(true, Ordering::Relaxed),
            Self::RolledStats => {
                super::traits_view::open();
                close();
            }
            Self::FeedNow => {
                lifecycle::feed();
                super::show_toast(super::Toast::Feed);
                close();
            }
            Self::GiveMedicine => {
                lifecycle::heal();
                super::show_toast(super::Toast::Heal);
                close();
            }
            Self::Sleep => {
                lifecycle::sleep();
                super::show_toast(super::Toast::Sleep);
                close();
            }
            Self::Relax => {
                lifecycle::relax();
                super::show_toast(super::Toast::Relax);
                close();
            }
            Self::PlayNow => {
                lifecycle::play();
                super::show_toast(super::Toast::Play);
                close();
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
            Self::Nim => {
                super::nim::open();
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
        }
    }
}

/// Format a cooldown count (ticks of 10 s each) as a " (Ns)" suffix
/// for a disabled menu row.  Returns `None` when the cooldown is 0.
fn cooldown_string(ticks: u16) -> Option<heapless::String<10>> {
    if ticks == 0 {
        return None;
    }
    let secs = ticks.saturating_mul(10);
    let mut s = heapless::String::new();
    let _ = core::fmt::Write::write_fmt(&mut s, format_args!(" ({}s)", secs));
    Some(s)
}

/// Map an icon (row, col) to the modal it should open.
pub fn kind_for_icon(row: Row, col: u8) -> ModalKind {
    match (row, col) {
        // Top row: info / meta.
        (Row::Top, 0) => ModalKind::Stats,
        (Row::Top, 1) => ModalKind::Hibernate,
        // Top row cols 2-3: empty (no modal).
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

pub fn open(kind: ModalKind) {
    MODAL_POS.store(0, Ordering::Relaxed);
    MODAL_KIND.store(kind as u8, Ordering::Relaxed);
}

pub fn close() {
    STATS_VIEW.store(false, Ordering::Relaxed);
    MODAL_KIND.store(ModalKind::None as u8, Ordering::Relaxed);
    MODAL_POS.store(0, Ordering::Relaxed);
}

pub fn is_open() -> bool {
    MODAL_KIND.load(Ordering::Relaxed) != 0
}

// ── Cursor navigation
// ─────────────────────────────────────────────────────────

pub fn cursor_up() {
    let pos = MODAL_POS.load(Ordering::Relaxed);
    if pos > 0 {
        MODAL_POS.store(pos - 1, Ordering::Relaxed);
    }
}

pub fn cursor_down() {
    let kind = ModalKind::from_u8(MODAL_KIND.load(Ordering::Relaxed));
    let len = kind.items().len() as u8;
    let pos = MODAL_POS.load(Ordering::Relaxed);
    if pos + 1 < len {
        MODAL_POS.store(pos + 1, Ordering::Relaxed);
    }
}

/// Activate the currently selected item.
///
/// "Cancel" closes the modal.  Action items dispatch to the game engine
/// via [`lifecycle`].  "View stats" opens the stats bar display.
pub fn activate() {
    // If stats view is showing, any activation closes it.
    if STATS_VIEW.load(Ordering::Relaxed) {
        STATS_VIEW.store(false, Ordering::Relaxed);
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
            return;
        }
    }

    item.activate();
}

fn play_song(_index: usize) {
    #[cfg(feature = "embassy-base")]
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

/// Draw the modal overlay.  Call this after [`draw_screen_game`] so it renders
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

    let pos = MODAL_POS.load(Ordering::Relaxed) as usize;
    let items = kind.items();

    // White popover frame with 2 px black border.
    ui::draw_popover_frame(
        display,
        Point::new(MARGIN, MARGIN),
        Size::new(MODAL_W, MODAL_H),
        BORDER,
    )?;

    // Title bar — black fill, bold white text.
    let inner_x = MARGIN + BORDER as i32;
    let inner_y = MARGIN + BORDER as i32;
    let inner_w = MODAL_W - BORDER * 2;
    ui::draw_title_bar(display, kind.title(), Point::new(inner_x, inner_y), inner_w)?;

    // Item list
    let left_style = TextStyleBuilder::new()
        .baseline(Baseline::Middle)
        .alignment(Alignment::Left)
        .build();

    let list_x = inner_x;
    let list_y = inner_y + TITLE_H;
    let list_bottom = MARGIN + MODAL_H as i32 - BORDER as i32;

    let stats = super::lifecycle::cycle();

    for (i, item) in items.iter().enumerate() {
        let row_top = list_y + i as i32 * ITEM_H;
        let row_mid = row_top + ITEM_H / 2;
        if row_top + ITEM_H > list_bottom {
            break;
        }

        // Cancel is always available; other items defer to the engine
        // snapshot.  When the snapshot is missing (very early boot)
        // treat everything-but-Cancel as locked out.
        let available =
            matches!(item, Item::Cancel) || stats.as_ref().is_some_and(|s| item.available(s));

        // Build display text: append " (Ns)" with the remaining
        // cooldown seconds when the item has a known cooldown source,
        // " (wait)" for anything else that's just disabled.
        let mut display_label: heapless::String<24> = heapless::String::new();
        let _ = display_label.push_str(item.label());
        if !available {
            let suffix = stats
                .as_ref()
                .and_then(|s| cooldown_string(item.cooldown_ticks(s)));
            match suffix {
                Some(s) => {
                    let _ = display_label.push_str(s.as_str());
                }
                None => {
                    let _ = display_label.push_str(" (wait)");
                }
            }
        }

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
                Point::new(list_x + 4, row_mid),
                TEXT_WHITE,
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
                Point::new(list_x + 4, row_mid),
                TEXT_BLACK,
                left_style,
            )
            .draw(display)?;
        } else {
            // Not selected.
            Text::with_text_style(
                display_label.as_str(),
                Point::new(list_x + 4, row_mid),
                TEXT_BLACK,
                left_style,
            )
            .draw(display)?;
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
// Stats view — 5 labeled bars showing pet health at a glance
// ---------------------------------------------------------------------------

/// Bar width in pixels (modal inner is 128 px; subtract label width + margins).
const BAR_MAX_W: u32 = 60;
/// Bar height — tall enough to render the inline percentage text.
const BAR_H: u32 = 16;
/// Vertical spacing between bars (height + small gap).
const BAR_SPACING: i32 = 18;

fn draw_stats_view<D>(display: &mut D) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    use super::lifecycle;
    use super::stat_bar::draw_stat_bar;
    use crate::RED;

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

    // Title bar.
    let inner_x = MARGIN + BORDER as i32;
    let inner_y = MARGIN + BORDER as i32;
    let inner_w = MODAL_W - BORDER * 2;
    ui::draw_title_bar(display, "Pet Stats", Point::new(inner_x, inner_y), inner_w)?;

    // Stat bars.
    let bars: [(&str, u8); 5] = [
        ("Hunger", stats.hunger),
        ("Rested", stats.tired),
        ("Inspired", stats.inspired),
        ("Healthy", stats.healthy),
        ("Happy", stats.happy),
    ];

    // Layout: label at left margin, bar to the right of the longest
    // label ("Inspired" = 8 chars × 7 px = 56 px from `label_x`).
    let label_x = inner_x + 4;
    let bar_x = label_x + 60; // 4 px clearance after the longest label

    for (i, (label, value)) in bars.iter().enumerate() {
        let y = inner_y + TITLE_H + 4 + i as i32 * BAR_SPACING;
        // Label vertically centred against the bar.
        let label_y = y + (BAR_H as i32 - 13) / 2;
        // Critical (< 25 %) values are highlighted in red.
        let fill_color = if *value < 25 { RED } else { BLACK };

        draw_stat_bar(
            display,
            label,
            *value,
            Point::new(label_x, label_y),
            Point::new(bar_x, y),
            Size::new(BAR_MAX_W, BAR_H),
            fill_color,
        )?;
    }

    // Footer: name, generation + age.
    let name = super::lifecycle::pet_name();
    let age_hours = stats.age_ticks / 360;
    let age_days = age_hours / 24;
    // Footer baseline; moved down 8 px so it no longer collides with
    // the bottom of the last stat bar (which now occupies y ≤ 122).
    let footer_y = MARGIN + MODAL_H as i32 - BORDER as i32 - 6;
    let mut footer: heapless::String<32> = heapless::String::new();
    if !name.is_empty() {
        let _ = core::fmt::Write::write_fmt(
            &mut footer,
            format_args!("{} | {}d {}h", name, age_days, age_hours % 24),
        );
    } else {
        let _ = core::fmt::Write::write_fmt(
            &mut footer,
            format_args!(
                "Gen {} | {}d {}h",
                stats.generation,
                age_days,
                age_hours % 24
            ),
        );
    }
    Text::with_text_style(
        footer.as_str(),
        Point::new(MARGIN + MODAL_W as i32 / 2, footer_y),
        TEXT_BOLD_BLACK,
        TextStyleBuilder::new()
            .baseline(Baseline::Bottom)
            .alignment(Alignment::Center)
            .build(),
    )
    .draw(display)?;

    Ok(())
}
