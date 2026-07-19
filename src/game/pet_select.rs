//! Pet selection screen — shown before hatching to choose a pet kind.
//!
//! Displayed full-screen when the player starts a new game or a new
//! generation, in two phases:
//!   1. `PHASE_PET`   — Up/Down cycles through available pet kinds, Fire
//!                      advances to the money phase.
//!   2. `PHASE_MONEY` — Up/Down cycles the three money modes ("With
//!                      money" / "Without money" / "Hard (US)"), Fire
//!                      starts the game with the chosen pet kind and
//!                      money mode, then closes the screen.
//! Money mode is chosen fresh every time (new game and new generation
//! alike), defaulting to "With money" (money on, normal prices).
//! "Hard (US)" plays with money on but some prices higher — see
//! `engine::medication_price`/`engine::rehab_price`/`FoodKind::hax_price`.

use core::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{PrimitiveStyle, Rectangle};
use embedded_graphics::text::{Alignment, Baseline, Text, TextStyleBuilder};

use super::engine::PetKind;
use crate::ui::{self, TEXT_BLACK, TEXT_BOLD_WHITE};
use crate::{BLACK, TriColor, WHITE};

// ── State ────────────────────────────────────────────────────────────────────

static ACTIVE: AtomicBool = AtomicBool::new(false);

/// Cursor within the current phase's list. In `PHASE_PET` this indexes
/// `PetKind::roster()`; in `PHASE_MONEY` it's 0 ("With money"), 1
/// ("Without money") or 2 ("Hard (US)").
static SELECTION: AtomicU8 = AtomicU8::new(0);

/// Money-phase option labels. Module scope so the cursor bound below
/// derives from the list — add a label here and the wrap-around count
/// follows automatically instead of silently drifting out of sync.
const MONEY_LABELS: [&str; 3] = ["With money", "Without money", "Hard mode (US)"];
/// Number of rows in the money phase — derived from `MONEY_LABELS`.
const MONEY_OPTION_COUNT: u8 = MONEY_LABELS.len() as u8;

/// What to do after selection: 0 = new game, 1 = new generation.
static MODE: AtomicU8 = AtomicU8::new(0);

const MODE_NEW_GAME: u8 = 0;
const MODE_NEW_GEN: u8 = 1;

/// Which phase of the two-phase screen is showing.
static PHASE: AtomicU8 = AtomicU8::new(PHASE_PET);

/// The pet-kind roster index chosen in `PHASE_PET`, carried over into
/// `PHASE_MONEY` so `confirm()` can resolve it once money is chosen.
static CHOSEN_KIND_IDX: AtomicU8 = AtomicU8::new(0);

const PHASE_PET: u8 = 0;
const PHASE_MONEY: u8 = 1;

// ── Public API ───────────────────────────────────────────────────────────────

pub fn is_active() -> bool {
    ACTIVE.load(Ordering::Relaxed)
}

/// Open the pet selection screen for a brand new game.
pub fn open_new_game() {
    SELECTION.store(0, Ordering::Relaxed);
    PHASE.store(PHASE_PET, Ordering::Relaxed);
    MODE.store(MODE_NEW_GAME, Ordering::Relaxed);
    ACTIVE.store(true, Ordering::Relaxed);
}

/// Open the pet selection screen for a new generation (pet left / reset).
pub fn open_new_generation() {
    SELECTION.store(0, Ordering::Relaxed);
    PHASE.store(PHASE_PET, Ordering::Relaxed);
    MODE.store(MODE_NEW_GEN, Ordering::Relaxed);
    ACTIVE.store(true, Ordering::Relaxed);
}

pub fn close() {
    ACTIVE.store(false, Ordering::Relaxed);
}

// ── Input ────────────────────────────────────────────────────────────────────

pub fn cursor_up() {
    if PHASE.load(Ordering::Relaxed) == PHASE_MONEY {
        // Three-item wrapping cursor over 0/1/2.
        let s = SELECTION.load(Ordering::Relaxed);
        let prev = if s == 0 { MONEY_OPTION_COUNT - 1 } else { s - 1 };
        SELECTION.store(prev, Ordering::Relaxed);
        return;
    }

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
    if PHASE.load(Ordering::Relaxed) == PHASE_MONEY {
        // Three-item wrapping cursor over 0/1/2.
        let s = SELECTION.load(Ordering::Relaxed);
        let next = if s + 1 >= MONEY_OPTION_COUNT { 0 } else { s + 1 };
        SELECTION.store(next, Ordering::Relaxed);
        return;
    }

    let s = SELECTION.load(Ordering::Relaxed);
    let max = PetKind::roster().len() as u8;
    if s + 1 < max {
        SELECTION.store(s + 1, Ordering::Relaxed);
    } else if max > 0 {
        // Wrap to the top instead of doing nothing at the bottom.
        SELECTION.store(0, Ordering::Relaxed);
    }
}

/// Confirm the current phase.
///
/// In `PHASE_PET`, stashes the chosen roster index and advances to
/// `PHASE_MONEY` (defaulting to "With money") without closing the screen.
/// In `PHASE_MONEY`, resolves the pet kind chosen earlier plus the money
/// mode, starts the game (dispatching on `MODE`), resets back to
/// `PHASE_PET` for next time, and closes the screen.
pub fn confirm() {
    if PHASE.load(Ordering::Relaxed) == PHASE_PET {
        let idx = SELECTION.load(Ordering::Relaxed);
        CHOSEN_KIND_IDX.store(idx, Ordering::Relaxed);
        PHASE.store(PHASE_MONEY, Ordering::Relaxed);
        SELECTION.store(0, Ordering::Relaxed); // default: With money
        return;
    }

    // Resolve (money_enabled, hard_mode) from the selected row:
    //   0 = With money    -> (true, false)
    //   1 = Without money  -> (false, false)
    //   2 = Hard (US)      -> (true, true)  — money on, higher prices
    let (money_enabled, hard_mode) = match SELECTION.load(Ordering::Relaxed) {
        1 => (false, false),
        2 => (true, true),
        _ => (true, false),
    };
    let idx = CHOSEN_KIND_IDX.load(Ordering::Relaxed) as usize;
    let kind = PetKind::roster()
        .get(idx)
        .copied()
        .unwrap_or(PetKind::Bartholomeus);
    let mode = MODE.load(Ordering::Relaxed);

    match mode {
        MODE_NEW_GAME => super::lifecycle::start_new_game(kind, money_enabled, hard_mode),
        MODE_NEW_GEN => super::lifecycle::new_generation(kind, money_enabled, hard_mode),
        _ => {}
    }

    PHASE.store(PHASE_PET, Ordering::Relaxed);
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

    let title = if PHASE.load(Ordering::Relaxed) == PHASE_MONEY {
        "Play with money?"
    } else {
        "Choose your Pet"
    };
    ui::draw_title_bar(display, title, Point::zero(), 152)?;

    let centered = TextStyleBuilder::new()
        .baseline(Baseline::Middle)
        .alignment(Alignment::Center)
        .build();

    let start_y = 40;
    let row_h = 24i32;

    let draw_row =
        |display: &mut D, i: usize, label: &str, is_selected: bool| -> Result<(), D::Error> {
            let y = start_y + i as i32 * row_h;
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
            Text::with_text_style(label, Point::new(76, y), f, centered).draw(display)?;
            Ok(())
        };

    if PHASE.load(Ordering::Relaxed) == PHASE_MONEY {
        // Money list — same row layout/idiom as the pet list, over the
        // module-level MONEY_LABELS so it can't drift from MONEY_OPTION_COUNT.
        for (i, label) in MONEY_LABELS.iter().enumerate() {
            draw_row(display, i, label, i == selection)?;
        }
    } else {
        // Pet list.
        for (i, kind) in PetKind::roster().iter().enumerate() {
            draw_row(display, i, kind.name(), i == selection)?;
        }
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
    use std::sync::Mutex;

    /// All tests in this module poke the same process-wide statics
    /// (`SELECTION`/`PHASE`/`ACTIVE`/`MODE`/`CHOSEN_KIND_IDX`, plus, for
    /// the full-flow tests, `lifecycle`'s global `GAME`). `cargo test`
    /// runs test functions on multiple threads by default, so without
    /// serialization these would race each other. Every test takes this
    /// lock first and resets the statics it touches before returning it.
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    /// Up at the top of the roster used to be a no-op; it should now wrap
    /// to the last pet, and Down at the bottom should wrap back to the
    /// first — same "wrap instead of doing nothing" fix applied across
    /// every scrollable menu on the badge.
    #[test]
    fn cursor_wraps_at_both_ends() {
        let _guard = TEST_LOCK.lock().unwrap();
        let max = PetKind::roster().len() as u8;
        PHASE.store(PHASE_PET, Ordering::Relaxed);
        SELECTION.store(0, Ordering::Relaxed);

        cursor_up();
        assert_eq!(SELECTION.load(Ordering::Relaxed), max - 1);

        cursor_down();
        assert_eq!(SELECTION.load(Ordering::Relaxed), 0);

        // Leave global state clean for any other test touching SELECTION.
        SELECTION.store(0, Ordering::Relaxed);
    }

    /// In `PHASE_MONEY` there are three rows ("With money" / "Without
    /// money" / "Hard (US)"), so Up/Down should wrap over 0/1/2 rather
    /// than walking the pet roster.
    #[test]
    fn money_phase_cursor_wraps_over_three_options() {
        let _guard = TEST_LOCK.lock().unwrap();
        PHASE.store(PHASE_MONEY, Ordering::Relaxed);
        SELECTION.store(0, Ordering::Relaxed);

        cursor_down();
        assert_eq!(SELECTION.load(Ordering::Relaxed), 1);

        cursor_down();
        assert_eq!(SELECTION.load(Ordering::Relaxed), 2);

        // Wraps back to 0 past the last option.
        cursor_down();
        assert_eq!(SELECTION.load(Ordering::Relaxed), 0);

        // Wraps to the last option (2) going up from 0.
        cursor_up();
        assert_eq!(SELECTION.load(Ordering::Relaxed), 2);

        cursor_up();
        assert_eq!(SELECTION.load(Ordering::Relaxed), 1);

        // Reset statics for any other test.
        SELECTION.store(0, Ordering::Relaxed);
        PHASE.store(PHASE_PET, Ordering::Relaxed);
    }

    /// Fire on the pet-kind phase must not close the screen — it should
    /// advance straight into the money phase so the player picks a money
    /// mode before the game actually starts.
    #[test]
    fn confirm_advances_pet_phase_to_money_phase() {
        let _guard = TEST_LOCK.lock().unwrap();
        open_new_game();
        assert_eq!(PHASE.load(Ordering::Relaxed), PHASE_PET);
        assert!(is_active());

        confirm();

        assert!(is_active());
        assert_eq!(PHASE.load(Ordering::Relaxed), PHASE_MONEY);

        // Reset statics for any other test.
        close();
        PHASE.store(PHASE_PET, Ordering::Relaxed);
        SELECTION.store(0, Ordering::Relaxed);
    }

    /// Full flow with "Without money" selected in the money phase: the
    /// game should start with money mode off (and hard mode off).
    #[test]
    fn full_flow_money_off_starts_game_without_money() {
        let _guard = TEST_LOCK.lock().unwrap();
        open_new_game();
        confirm(); // pet phase (SELECTION=0) -> money phase

        assert_eq!(PHASE.load(Ordering::Relaxed), PHASE_MONEY);
        SELECTION.store(1, Ordering::Relaxed); // "Without money"
        confirm();

        assert!(crate::game::lifecycle::is_started());
        assert!(!crate::game::lifecycle::money_enabled());
        assert!(!crate::game::lifecycle::hard_mode());

        // Reset statics for any other test.
        close();
        PHASE.store(PHASE_PET, Ordering::Relaxed);
        SELECTION.store(0, Ordering::Relaxed);
    }

    /// Full flow leaving the money-phase default ("With money") in place:
    /// the game should start with money mode on, hard mode off, and the
    /// starting 100 HAX.
    #[test]
    fn full_flow_money_on_starts_game_with_money() {
        let _guard = TEST_LOCK.lock().unwrap();
        open_new_game();
        confirm(); // pet phase (SELECTION=0) -> money phase, default SELECTION=0

        assert_eq!(PHASE.load(Ordering::Relaxed), PHASE_MONEY);
        assert_eq!(SELECTION.load(Ordering::Relaxed), 0); // "With money" default
        confirm();

        assert!(crate::game::lifecycle::money_enabled());
        assert!(!crate::game::lifecycle::hard_mode());
        assert_eq!(crate::game::lifecycle::money(), 100);

        // Reset statics for any other test.
        close();
        PHASE.store(PHASE_PET, Ordering::Relaxed);
        SELECTION.store(0, Ordering::Relaxed);
    }

    /// Full flow with "Hard (US)" (SELECTION=2) selected in the money
    /// phase: the game should start with money mode on AND hard mode on.
    #[test]
    fn full_flow_hard_mode_starts_hard() {
        let _guard = TEST_LOCK.lock().unwrap();
        open_new_game();
        confirm(); // pet phase (SELECTION=0) -> money phase

        assert_eq!(PHASE.load(Ordering::Relaxed), PHASE_MONEY);
        SELECTION.store(2, Ordering::Relaxed); // "Hard (US)"
        confirm();

        assert!(crate::game::lifecycle::is_started());
        assert!(crate::game::lifecycle::money_enabled());
        assert!(crate::game::lifecycle::hard_mode());

        // Reset statics for any other test.
        close();
        PHASE.store(PHASE_PET, Ordering::Relaxed);
        SELECTION.store(0, Ordering::Relaxed);
    }
}
