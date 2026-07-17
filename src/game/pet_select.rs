//! Pet selection screen — shown before hatching to choose a pet kind.
//!
//! Displayed full-screen when the player starts a new game or a new
//! generation. Up/Down cycles through available pet kinds, Fire confirms.

use core::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{PrimitiveStyle, Rectangle};
use embedded_graphics::text::{Alignment, Baseline, Text, TextStyleBuilder};

use super::engine::PetKind;
use crate::ui::{self, TEXT_BLACK, TEXT_BOLD_WHITE};
use crate::{BLACK, TriColor, WHITE};

// ── State ────────────────────────────────────────────────────────────────────

static ACTIVE: AtomicBool = AtomicBool::new(false);
static SELECTION: AtomicU8 = AtomicU8::new(0);

/// What to do after selection: 0 = new game, 1 = new generation.
static MODE: AtomicU8 = AtomicU8::new(0);

const MODE_NEW_GAME: u8 = 0;
const MODE_NEW_GEN: u8 = 1;

// ── Public API ───────────────────────────────────────────────────────────────

pub fn is_active() -> bool {
    ACTIVE.load(Ordering::Relaxed)
}

/// Open the pet selection screen for a brand new game.
pub fn open_new_game() {
    SELECTION.store(0, Ordering::Relaxed);
    MODE.store(MODE_NEW_GAME, Ordering::Relaxed);
    ACTIVE.store(true, Ordering::Relaxed);
}

/// Open the pet selection screen for a new generation (pet left / reset).
pub fn open_new_generation() {
    SELECTION.store(0, Ordering::Relaxed);
    MODE.store(MODE_NEW_GEN, Ordering::Relaxed);
    ACTIVE.store(true, Ordering::Relaxed);
}

pub fn close() {
    ACTIVE.store(false, Ordering::Relaxed);
}

// ── Input ────────────────────────────────────────────────────────────────────

pub fn cursor_up() {
    let s = SELECTION.load(Ordering::Relaxed);
    if s > 0 {
        SELECTION.store(s - 1, Ordering::Relaxed);
    } else {
        // Wrap to the last pet instead of doing nothing at the top.
        let max = PetKind::roster().len() as u8;
        if max > 0 {
            SELECTION.store(max - 1, Ordering::Relaxed);
        }
    }
}

pub fn cursor_down() {
    let s = SELECTION.load(Ordering::Relaxed);
    let max = PetKind::roster().len() as u8;
    if s + 1 < max {
        SELECTION.store(s + 1, Ordering::Relaxed);
    } else if max > 0 {
        // Wrap to the top instead of doing nothing at the bottom.
        SELECTION.store(0, Ordering::Relaxed);
    }
}

/// Confirm selection — starts the game with the chosen pet kind.
pub fn confirm() {
    let idx = SELECTION.load(Ordering::Relaxed) as usize;
    let kind = PetKind::roster()
        .get(idx)
        .copied()
        .unwrap_or(PetKind::Bartholomeus);
    let mode = MODE.load(Ordering::Relaxed);

    match mode {
        MODE_NEW_GAME => super::lifecycle::start_new_game(kind),
        MODE_NEW_GEN => super::lifecycle::new_generation(kind),
        _ => {}
    }

    close();
}

// ── Drawing ──────────────────────────────────────────────────────────────────

pub fn draw<D>(display: &mut D) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let selection = SELECTION.load(Ordering::Relaxed) as usize;

    // Background.
    Rectangle::new(Point::zero(), Size::new(152, 152))
        .into_styled(PrimitiveStyle::with_fill(WHITE))
        .draw(display)?;

    // Title bar.
    ui::draw_title_bar(display, "Choose your Pet", Point::zero(), 152)?;

    // Pet list.
    let centered = TextStyleBuilder::new()
        .baseline(Baseline::Middle)
        .alignment(Alignment::Center)
        .build();

    let start_y = 40;
    let row_h = 24i32;

    for (i, kind) in PetKind::roster().iter().enumerate() {
        let y = start_y + i as i32 * row_h;
        let is_selected = i == selection;

        if is_selected {
            Rectangle::new(
                Point::new(10, y - row_h / 2 + 1),
                Size::new(132, row_h as u32 - 2),
            )
            .into_styled(PrimitiveStyle::with_fill(BLACK))
            .draw(display)?;
        }

        let f = if is_selected {
            TEXT_BOLD_WHITE
        } else {
            TEXT_BLACK
        };
        Text::with_text_style(kind.name(), Point::new(76, y), f, centered).draw(display)?;
    }

    // Hint at bottom.
    let hint_style = TextStyleBuilder::new()
        .baseline(Baseline::Bottom)
        .alignment(Alignment::Center)
        .build();
    Text::with_text_style(
        "Fire to confirm",
        Point::new(76, 148),
        TEXT_BLACK,
        hint_style,
    )
    .draw(display)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Up at the top of the roster used to be a no-op; it should now wrap
    /// to the last pet, and Down at the bottom should wrap back to the
    /// first — same "wrap instead of doing nothing" fix applied across
    /// every scrollable menu on the badge.
    #[test]
    fn cursor_wraps_at_both_ends() {
        let max = PetKind::roster().len() as u8;
        SELECTION.store(0, Ordering::Relaxed);

        cursor_up();
        assert_eq!(SELECTION.load(Ordering::Relaxed), max - 1);

        cursor_down();
        assert_eq!(SELECTION.load(Ordering::Relaxed), 0);

        // Leave global state clean for any other test touching SELECTION.
        SELECTION.store(0, Ordering::Relaxed);
    }
}
