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

use embedded_graphics::{
    mono_font::{ascii::FONT_7X13, MonoTextStyle},
    prelude::*,
    primitives::{PrimitiveStyle, Rectangle},
    text::{Alignment, Baseline, Text, TextStyleBuilder},
};

use crate::{BLACK, TriColor, WHITE};
use super::nav::Row;

// ── Modal kind ────────────────────────────────────────────────────────────────

/// Which in-game modal is currently open.  Stored as a `u8` in [`MODAL_KIND`].
///
/// Layout:
///   Top row (info/meta):    Stats, Hibernate, (empty), (empty)
///   Bottom row (actions):   Feed,  Heal,      Play,    Rest
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ModalKind {
    None       = 0,
    // Top row — info / meta.
    Stats      = 1,   // top row, col 0
    Hibernate  = 2,   // top row, col 1
    // Bottom row — actions.
    Feed       = 3,   // bot row, col 0
    Heal       = 4,   // bot row, col 1
    Play       = 5,   // bot row, col 2
    Rest       = 6,   // bot row, col 3
    // Sub-modal opened from Play.
    Music      = 7,
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
            Self::None      => "",
            Self::Stats     => "Stats",
            Self::Hibernate => "Hibernate",
            Self::Feed      => "Feed",
            Self::Heal      => "Heal",
            Self::Play      => "Play",
            Self::Rest      => "Rest",
            Self::Music     => "Music",
        }
    }

    fn items(self) -> &'static [&'static str] {
        match self {
            Self::Stats     => &["View stats",   "Cancel"],
            Self::Hibernate => &["Hibernate",    "Wake up",     "Cancel"],
            Self::Feed      => &["Feed now",     "Cancel"],
            Self::Heal      => &["Give medicine",    "Cancel"],
            Self::Play      => &["Play now",     "Tic Tac Toe", "Lights Out",  "Play music",  "Cancel"],
            Self::Music     => &["Startup", "Rickroll", "Imp. March", "Sandstorm", "Pink Panther", "Trololo", "Cancel"],
            Self::Rest      => &["Sleep",        "Relax",       "Cancel"],
            Self::None      => &[],
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
        "Feed now"   => stats.can_feed,
        "Give medicine"  => stats.can_heal,
        "Sleep"      => stats.can_sleep,
        "Relax"      => stats.can_relax,
        "Play now"   => stats.can_play,
        "Play music" => true,
        "Tic Tac Toe" | "Lights Out" => true,
        "Startup" | "Rickroll" | "Imp. March" | "Sandstorm" | "Pink Panther" | "Trololo" => true,
        "Hibernate"  => !stats.hibernating,
        "Wake up"    => stats.hibernating,
        _            => true, // Cancel, View stats, etc.
    }
}

/// Map an icon (row, col) to the modal it should open.
pub fn kind_for_icon(row: Row, col: u8) -> ModalKind {
    match (row, col) {
        // Top row: info / meta.
        (Row::Top,    0) => ModalKind::Stats,
        (Row::Top,    1) => ModalKind::Hibernate,
        // Top row cols 2-3: empty (no modal).
        (Row::Top,    _) => ModalKind::None,
        // Bottom row: actions.
        (Row::Bottom, 0) => ModalKind::Feed,
        (Row::Bottom, 1) => ModalKind::Heal,
        (Row::Bottom, 2) => ModalKind::Play,
        (Row::Bottom, 3) => ModalKind::Rest,
        _                => ModalKind::None,
    }
}

// ── Global state ──────────────────────────────────────────────────────────────

static MODAL_KIND: AtomicU8 = AtomicU8::new(0);
static MODAL_POS:  AtomicU8 = AtomicU8::new(0);
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

// ── Cursor navigation ─────────────────────────────────────────────────────────

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
    let pos  = MODAL_POS.load(Ordering::Relaxed) as usize;
    let items = kind.items();
    let Some(&label) = items.get(pos) else { return; };

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
        "View stats"  => { STATS_VIEW.store(true, Ordering::Relaxed); }
        "Feed now"    => { lifecycle::feed(); super::show_toast(super::Toast::Feed); close(); }
        "Give medicine"   => { lifecycle::heal(); super::show_toast(super::Toast::Heal); close(); }
        "Sleep"       => { lifecycle::sleep(); super::show_toast(super::Toast::Sleep); close(); }
        "Relax"       => { lifecycle::relax(); super::show_toast(super::Toast::Relax); close(); }
        "Play now"    => { lifecycle::play(); super::show_toast(super::Toast::Play); close(); }
        "Tic Tac Toe" => { super::tictactoe::open(); close(); }
        "Lights Out"  => { super::lightsout::open(); close(); }
        "Play music"  => {
            open(ModalKind::Music);
        }
        "Startup"     => { play_song(0); }
        "Rickroll"    => { play_song(1); }
        "Imp. March"  => { play_song(2); }
        "Sandstorm"   => { play_song(3); }
        "Pink Panther" => { play_song(4); }
        "Trololo"     => { play_song(5); }
        "Hibernate"   => { lifecycle::hibernate(); super::show_toast(super::Toast::Hibernate); close(); }
        "Wake up"     => { lifecycle::wake_from_hibernation(); super::show_toast(super::Toast::Wake); close(); }
        _ => {}
    }
}

fn play_song(_index: usize) {
    #[cfg(feature = "embassy-base")]
    crate::fw::buzzer::play(_index);
    close();
}

// ── Drawing ───────────────────────────────────────────────────────────────────

const MARGIN:   i32 = 10;
const MODAL_W:  u32 = 132;  // 152 - 2 × MARGIN
const MODAL_H:  u32 = 132;
const BORDER:   u32 = 2;
const TITLE_H:  i32 = 18;
const ITEM_H:   i32 = 16;

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

    let pos   = MODAL_POS.load(Ordering::Relaxed) as usize;
    let items = kind.items();

    // White background
    Rectangle::new(Point::new(MARGIN, MARGIN), Size::new(MODAL_W, MODAL_H))
        .into_styled(PrimitiveStyle::with_fill(WHITE))
        .draw(display)?;

    // 2 px black border
    Rectangle::new(Point::new(MARGIN, MARGIN), Size::new(MODAL_W, MODAL_H))
        .into_styled(PrimitiveStyle::with_stroke(BLACK, BORDER))
        .draw(display)?;

    // Title bar — black fill, white text
    let inner_x  = MARGIN + BORDER as i32;
    let inner_y  = MARGIN + BORDER as i32;
    let inner_w  = MODAL_W - BORDER * 2;
    Rectangle::new(Point::new(inner_x, inner_y), Size::new(inner_w, TITLE_H as u32))
        .into_styled(PrimitiveStyle::with_fill(BLACK))
        .draw(display)?;

    let title_style = TextStyleBuilder::new()
        .baseline(Baseline::Middle)
        .alignment(Alignment::Center)
        .build();
    Text::with_text_style(
        kind.title(),
        Point::new(MARGIN + MODAL_W as i32 / 2, inner_y + TITLE_H / 2),
        MonoTextStyle::new(&FONT_7X13, WHITE),
        title_style,
    )
    .draw(display)?;

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
            Rectangle::new(Point::new(inner_x, row_top), Size::new(inner_w, ITEM_H as u32))
                .into_styled(PrimitiveStyle::with_fill(BLACK))
                .draw(display)?;
            Text::with_text_style(
                display_label.as_str(),
                Point::new(list_x + 4, row_mid),
                MonoTextStyle::new(&FONT_7X13, WHITE),
                left_style,
            )
            .draw(display)?;
        } else if i == pos && !available {
            // Selected but on cooldown: dashed outline, not filled.
            Rectangle::new(Point::new(inner_x, row_top), Size::new(inner_w, ITEM_H as u32))
                .into_styled(PrimitiveStyle::with_stroke(BLACK, 1))
                .draw(display)?;
            Text::with_text_style(
                display_label.as_str(),
                Point::new(list_x + 4, row_mid),
                MonoTextStyle::new(&FONT_7X13, BLACK),
                left_style,
            )
            .draw(display)?;
        } else {
            // Not selected.
            Text::with_text_style(
                display_label.as_str(),
                Point::new(list_x + 4, row_mid),
                MonoTextStyle::new(&FONT_7X13, BLACK),
                left_style,
            )
            .draw(display)?;
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Stats view — 5 labeled bars showing pet health at a glance
// ---------------------------------------------------------------------------

/// Bar width in pixels for a 0–100 value (fits within modal: 128 - label - margins).
const BAR_MAX_W: u32 = 60;
/// Bar height.
const BAR_H: u32 = 8;
/// Vertical spacing between bars (compact to fit 5 bars + footer in modal).
const BAR_SPACING: i32 = 16;

fn draw_stats_view<D>(display: &mut D) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    use crate::RED;
    use super::lifecycle;

    // Get fresh stats (triggers an update if needed).
    let stats = match lifecycle::cycle() {
        Some(s) => s,
        None => return Ok(()),
    };

    // White background + border.
    Rectangle::new(Point::new(MARGIN, MARGIN), Size::new(MODAL_W, MODAL_H))
        .into_styled(PrimitiveStyle::with_fill(WHITE))
        .draw(display)?;
    Rectangle::new(Point::new(MARGIN, MARGIN), Size::new(MODAL_W, MODAL_H))
        .into_styled(PrimitiveStyle::with_stroke(BLACK, BORDER))
        .draw(display)?;

    // Title bar.
    let inner_x = MARGIN + BORDER as i32;
    let inner_y = MARGIN + BORDER as i32;
    let inner_w = MODAL_W - BORDER * 2;
    Rectangle::new(Point::new(inner_x, inner_y), Size::new(inner_w, TITLE_H as u32))
        .into_styled(PrimitiveStyle::with_fill(BLACK))
        .draw(display)?;
    Text::with_text_style(
        "Pet Stats",
        Point::new(MARGIN + MODAL_W as i32 / 2, inner_y + TITLE_H / 2),
        MonoTextStyle::new(&FONT_7X13, WHITE),
        TextStyleBuilder::new()
            .baseline(Baseline::Middle)
            .alignment(Alignment::Center)
            .build(),
    )
    .draw(display)?;

    // Stat bars.
    let bars: [(&str, u8); 5] = [
        ("Hunger",   stats.hunger),
        ("Rested",   stats.tired),
        ("Inspired", stats.inspired),
        ("Healthy",  stats.healthy),
        ("Happy",    stats.happy),
    ];

    let bar_x = inner_x + 4;
    let label_style = TextStyleBuilder::new()
        .baseline(Baseline::Bottom)
        .alignment(Alignment::Left)
        .build();

    for (i, (label, value)) in bars.iter().enumerate() {
        let y_base = inner_y + TITLE_H + 4 + i as i32 * BAR_SPACING;

        // Label.
        Text::with_text_style(
            label,
            Point::new(bar_x, y_base + 9),
            MonoTextStyle::new(&FONT_7X13, BLACK),
            label_style,
        )
        .draw(display)?;

        // Bar background (empty).
        let bar_left = bar_x + 50;
        let bar_y = y_base;
        Rectangle::new(
            Point::new(bar_left, bar_y),
            Size::new(BAR_MAX_W, BAR_H),
        )
        .into_styled(PrimitiveStyle::with_stroke(BLACK, 1))
        .draw(display)?;

        // Bar fill.
        let fill_w = (*value as u32 * BAR_MAX_W) / 100;
        if fill_w > 0 {
            // Color: red when critical (< 25%), black otherwise.
            let fill_color = if *value < 25 { RED } else { BLACK };
            Rectangle::new(
                Point::new(bar_left + 1, bar_y + 1),
                Size::new(fill_w.min(BAR_MAX_W - 2), BAR_H - 2),
            )
            .into_styled(PrimitiveStyle::with_fill(fill_color))
            .draw(display)?;
        }
    }

    // Footer: name, generation + age.
    let name = super::lifecycle::pet_name();
    let age_hours = stats.age_ticks / 360;
    let age_days = age_hours / 24;
    let footer_y = MARGIN + MODAL_H as i32 - BORDER as i32 - 14;
    let mut footer: heapless::String<32> = heapless::String::new();
    if !name.is_empty() {
        let _ = core::fmt::Write::write_fmt(
            &mut footer,
            format_args!("{} | {}d {}h", name, age_days, age_hours % 24),
        );
    } else {
        let _ = core::fmt::Write::write_fmt(
            &mut footer,
            format_args!("Gen {} | {}d {}h", stats.generation, age_days, age_hours % 24),
        );
    }
    Text::with_text_style(
        footer.as_str(),
        Point::new(MARGIN + MODAL_W as i32 / 2, footer_y),
        MonoTextStyle::new(&FONT_7X13, BLACK),
        TextStyleBuilder::new()
            .baseline(Baseline::Bottom)
            .alignment(Alignment::Center)
            .build(),
    )
    .draw(display)?;

    Ok(())
}
