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
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ModalKind {
    None       = 0,
    Feed       = 1,   // top row, col 0: fork
    Light      = 2,   // top row, col 1: bulb
    Play       = 3,   // top row, col 2: bat
    Medicine   = 4,   // top row, col 3: syringe
    Rest       = 5,   // bot row, col 0: duck
    Stats      = 6,   // bot row, col 1: meter
    Discipline = 7,   // bot row, col 2: face
    Attention  = 8,   // bot row, col 3: twofaces
}

impl ModalKind {
    pub fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::Feed,
            2 => Self::Light,
            3 => Self::Play,
            4 => Self::Medicine,
            5 => Self::Rest,
            6 => Self::Stats,
            7 => Self::Discipline,
            8 => Self::Attention,
            _ => Self::None,
        }
    }

    fn title(self) -> &'static str {
        match self {
            Self::None       => "",
            Self::Feed       => "Feed",
            Self::Light      => "Light",
            Self::Play       => "Play",
            Self::Medicine   => "Medicine",
            Self::Rest       => "Rest",
            Self::Stats      => "Stats",
            Self::Discipline => "Discipline",
            Self::Attention  => "Attention",
        }
    }

    fn items(self) -> &'static [&'static str] {
        match self {
            Self::Feed       => &["Feed now",     "Cancel"],
            Self::Light      => &["Light on",     "Light off", "Cancel"],
            Self::Play       => &["Mini-game",    "Music",     "Cancel"],
            Self::Medicine   => &["Give dose",    "Cancel"],
            Self::Rest       => &["Sleep",        "Relax",     "Cancel"],
            Self::Stats      => &["View stats",   "Cancel"],
            Self::Discipline => &["Scold",        "Cancel"],
            Self::Attention  => &["Acknowledge",  "Cancel"],
            Self::None       => &[],
        }
    }
}

/// Map an icon (row, col) to the modal it should open.
pub fn kind_for_icon(row: Row, col: u8) -> ModalKind {
    match (row, col) {
        (Row::Top,    0) => ModalKind::Feed,
        (Row::Top,    1) => ModalKind::Light,
        (Row::Top,    2) => ModalKind::Play,
        (Row::Top,    3) => ModalKind::Medicine,
        (Row::Bottom, 0) => ModalKind::Rest,
        (Row::Bottom, 1) => ModalKind::Stats,
        (Row::Bottom, 2) => ModalKind::Discipline,
        _                => ModalKind::Attention,
    }
}

// ── Global state ──────────────────────────────────────────────────────────────

static MODAL_KIND: AtomicU8 = AtomicU8::new(0);
static MODAL_POS:  AtomicU8 = AtomicU8::new(0);

pub fn open(kind: ModalKind) {
    MODAL_POS.store(0, Ordering::Relaxed);
    MODAL_KIND.store(kind as u8, Ordering::Relaxed);
}

pub fn close() {
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
/// "Cancel" items (last item in every list) close the modal.
/// All other items are stubs until Phase 3 wires in `GameState`.
pub fn activate() {
    let kind = ModalKind::from_u8(MODAL_KIND.load(Ordering::Relaxed));
    let pos  = MODAL_POS.load(Ordering::Relaxed) as usize;
    let items = kind.items();
    if let Some(&label) = items.get(pos) {
        if label == "Cancel" {
            close();
        }
        // TODO(Phase 3): dispatch real game actions here.
    }
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

        if i == pos {
            // Selected: inverted row
            Rectangle::new(Point::new(inner_x, row_top), Size::new(inner_w, ITEM_H as u32))
                .into_styled(PrimitiveStyle::with_fill(BLACK))
                .draw(display)?;
            Text::with_text_style(
                label,
                Point::new(list_x + 4, row_mid),
                MonoTextStyle::new(&FONT_7X13, WHITE),
                left_style,
            )
            .draw(display)?;
        } else {
            Text::with_text_style(
                label,
                Point::new(list_x + 4, row_mid),
                MonoTextStyle::new(&FONT_7X13, BLACK),
                left_style,
            )
            .draw(display)?;
        }
    }

    Ok(())
}
