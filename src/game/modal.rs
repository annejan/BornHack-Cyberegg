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
use crate::{BLACK, TriColor, WHITE};

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

    fn items(self) -> &'static [&'static str] {
        match self {
            Self::Stats => &["View stats", "Rolled stats", "Cancel"],
            Self::Hibernate => &["Hibernate", "Wake up", "Cancel"],
            Self::Feed => &["Feed now", "Cancel"],
            Self::Heal => &["Give medicine", "Cancel"],
            Self::Play => &[
                "Play now",
                "Tic Tac Toe",
                "Lights Out",
                "Black Hole",
                "Play music",
                "Cancel",
            ],
            Self::Music => &[
                "Startup",
                "Rickroll",
                "Imp. March",
                "Sandstorm",
                "Pink Panther",
                "Trololo",
                "Cancel",
            ],
            Self::Rest => &["Sleep", "Relax", "Cancel"],
            Self::None => &[],
        }
    }
}

/// Check if a menu item action is currently available (not on cooldown).
/// "Cancel", "View stats", and other non-action items are always available.
fn is_item_available(label: &str) -> bool {
    use super::lifecycle;

    let stats = match lifecycle::cycle() {
        Some(s) => s,
        None => return false,
    };

    match label {
        "Feed now" => stats.can_feed,
        "Give medicine" => stats.can_heal,
        "Sleep" => stats.can_sleep,
        "Relax" => stats.can_relax,
        "Play now" => stats.can_play,
        "Play music" => true,
        "Tic Tac Toe" | "Lights Out" | "Black Hole" => stats.can_play_minigame,
        "Startup" | "Rickroll" | "Imp. March" | "Sandstorm" | "Pink Panther" | "Trololo" => true,
        "Hibernate" => !stats.hibernating,
        "Wake up" => stats.hibernating,
        _ => true, // Cancel, View stats, etc.
    }
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
    let Some(&label) = items.get(pos) else {
        return;
    };

    if label == "Cancel" {
        close();
        return;
    }

    // Block actions that are on cooldown.
    if !is_item_available(label) {
        return;
    }

    use super::lifecycle;

    match label {
        "View stats" => {
            STATS_VIEW.store(true, Ordering::Relaxed);
        }
        "Rolled stats" => {
            super::traits_view::open();
            close();
        }
        "Feed now" => {
            lifecycle::feed();
            super::show_toast(super::Toast::Feed);
            close();
        }
        "Give medicine" => {
            lifecycle::heal();
            super::show_toast(super::Toast::Heal);
            close();
        }
        "Sleep" => {
            lifecycle::sleep();
            super::show_toast(super::Toast::Sleep);
            close();
        }
        "Relax" => {
            lifecycle::relax();
            super::show_toast(super::Toast::Relax);
            close();
        }
        "Play now" => {
            lifecycle::play();
            super::show_toast(super::Toast::Play);
            close();
        }
        "Tic Tac Toe" => {
            super::tictactoe::open();
            close();
        }
        "Lights Out" => {
            super::lightsout::open();
            close();
        }
        "Black Hole" => {
            super::blackhole::open();
            close();
        }
        "Play music" => {
            open(ModalKind::Music);
        }
        "Startup" => {
            play_song(0);
        }
        "Rickroll" => {
            play_song(1);
        }
        "Imp. March" => {
            play_song(2);
        }
        "Sandstorm" => {
            play_song(3);
        }
        "Pink Panther" => {
            play_song(4);
        }
        "Trololo" => {
            play_song(5);
        }
        "Hibernate" => {
            lifecycle::hibernate();
            super::show_toast(super::Toast::Hibernate);
            close();
        }
        "Wake up" => {
            lifecycle::wake_from_hibernation();
            super::show_toast(super::Toast::Wake);
            close();
        }
        _ => {}
    }
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

    for (i, label) in items.iter().enumerate() {
        let row_top = list_y + i as i32 * ITEM_H;
        let row_mid = row_top + ITEM_H / 2;
        if row_top + ITEM_H > list_bottom {
            break;
        }

        let available = is_item_available(label);

        // Build display text: append " (wait)" for cooldown items.
        let mut display_label: heapless::String<24> = heapless::String::new();
        let _ = display_label.push_str(label);
        if !available {
            let _ = display_label.push_str(" (wait)");
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

/// Overdraw a rectangle with a 1-in-2 white checkerboard, halftoning
/// any black pixels underneath into a perceived "grey".  Used to mark
/// menu rows whose action is currently on cooldown or otherwise
/// unavailable — solid black fills become 50 % black, black text on
/// white becomes 50 % black on white, both reading visibly dimmer
/// than their available siblings.
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
            if (x + y) & 1 == 0 {
                Some(Pixel(Point::new(x, y), WHITE))
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
