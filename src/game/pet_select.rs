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
//! `engine::medication_price`/`engine::rehab_price`/`doses::price`.

use core::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{PrimitiveStyle, Rectangle};
use embedded_graphics::text::Text;

use super::engine::PetKind;
use crate::ui::{self, TEXT_BLACK, TEXT_BOLD_BLACK, TEXT_BOLD_WHITE};
use crate::{BLACK, TriColor};

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

/// Shown in place of the pet list when the roster is empty.  The roster comes
/// only from `PETS.CFG` (see `crate::game::pet_registry`), so an absent or
/// unusable manifest leaves nothing to select — name the file so the player
/// knows what to put back on the USB partition.
const NO_PETS_LINES: [&str; 2] = ["No pets installed", "Check PETS.CFG"];

/// The screen's two titles and the hint at the bottom.
const PROMPT_MONEY: &str = "Play with money?";
const PROMPT_PET: &str = "Choose your Pet";
const PROMPT_CONFIRM: &str = "Fire to confirm";

/// Every fixed word this screen draws (host-only, for the guide's word list).
#[cfg(any(test, feature = "simulator"))]
pub fn words() -> std::vec::Vec<&'static str> {
    let mut out: std::vec::Vec<&'static str> = std::vec::Vec::new();
    out.extend(MONEY_LABELS);
    out.extend(NO_PETS_LINES);
    out.extend([PROMPT_MONEY, PROMPT_PET, PROMPT_CONFIRM]);
    out
}

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

// ── Layout ───────────────────────────────────────────────────────────────────

/// Top of the option-list band — just below the title bar.
const LIST_BAND_TOP: i32 = ui::TITLE_BAR_H as i32 + 2;
/// Baseline of the "Fire to confirm" hint at the bottom of the screen.
const HINT_BASELINE_Y: i32 = 148;
/// Height of the band the option rows are laid out in: everything between
/// the title bar and the top of the hint text (13px font, bottom baseline).
const LIST_BAND_H: i32 = HINT_BASELINE_Y - 13 - LIST_BAND_TOP;

/// Row pitch and the y of the first row's centre for a list of `count`
/// options, as `(row_h, start_y)`.
///
/// The pitch shrinks with the item count so a growing pet roster keeps
/// fitting in the band instead of drawing over the "Fire to confirm"
/// hint (the 5-pet roster overflowed at the old fixed 24px pitch). Capped
/// at 24 so short lists keep the roomy look, floored at 15 so the
/// highlight box never clips the 13px font, and the resulting block is
/// centred vertically in the band.
fn row_layout(count: usize) -> (i32, i32) {
    let count = count.max(1) as i32;
    let row_h = (LIST_BAND_H / count).clamp(15, 24);
    let start_y = LIST_BAND_TOP + (LIST_BAND_H - row_h * count) / 2 + row_h / 2;
    (row_h, start_y)
}

// ── Public API ───────────────────────────────────────────────────────────────

pub fn is_active() -> bool {
    ACTIVE.load(Ordering::Relaxed)
}

/// The two phases of the screen (host-only, for the guide's shot checks).
#[cfg(any(test, feature = "simulator"))]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    Pet,
    Money,
}

/// Which phase is showing. Host-only, for the guide's shot checks.
#[cfg(any(test, feature = "simulator"))]
pub fn phase() -> Phase {
    if PHASE.load(Ordering::Relaxed) == PHASE_MONEY {
        Phase::Money
    } else {
        Phase::Pet
    }
}

/// The highlighted row of the phase that is showing. Host-only, for the
/// guide's shot checks.
#[cfg(any(test, feature = "simulator"))]
pub fn selection() -> u8 {
    SELECTION.load(Ordering::Relaxed)
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
    let s = SELECTION.load(Ordering::Relaxed);
    let count = if PHASE.load(Ordering::Relaxed) == PHASE_MONEY {
        // Three-item wrapping cursor over 0/1/2.
        MONEY_OPTION_COUNT
    } else {
        PetKind::roster().len() as u8
    };
    SELECTION.store(ui::wrap_prev(s, count), Ordering::Relaxed);
}

pub fn cursor_down() {
    let s = SELECTION.load(Ordering::Relaxed);
    let count = if PHASE.load(Ordering::Relaxed) == PHASE_MONEY {
        MONEY_OPTION_COUNT
    } else {
        PetKind::roster().len() as u8
    };
    SELECTION.store(ui::wrap_next(s, count), Ordering::Relaxed);
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
        if PetKind::roster().is_empty() {
            // No `PETS.CFG` (or none of its rows were usable) — there is
            // nothing to confirm.  Stay on the screen showing the "no pets"
            // notice rather than advancing to money and starting a game
            // whose pet has no sprites.
            return;
        }
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
    // The roster is `PETS.CFG`-driven and can be empty, so there is no pet to
    // fall back on: bail out rather than start a game on a made-up prefix.
    let Some(kind) = PetKind::roster().get(idx).copied() else {
        PHASE.store(PHASE_PET, Ordering::Relaxed);
        close();
        return;
    };
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
    crate::ui::clear_screen(display)?;

    let title = if PHASE.load(Ordering::Relaxed) == PHASE_MONEY {
        PROMPT_MONEY
    } else {
        PROMPT_PET
    };
    ui::draw_title_bar(display, title, Point::zero(), 152)?;

    let centered = crate::ui::CENTERED;

    let (row_h, start_y) = row_layout(if PHASE.load(Ordering::Relaxed) == PHASE_MONEY {
        MONEY_LABELS.len()
    } else {
        // An empty roster still draws the two-line "no pets" notice, so lay
        // the band out for those rows instead of collapsing to one.
        PetKind::roster().len().max(NO_PETS_LINES.len())
    });

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
            // Bold either way — the highlight box already carries the
            // selection, so the unselected rows don't need to be lighter to
            // read as unselected, and the bold face is easier on e-ink.
            // Same 7x13 metrics as the regular face, so the layout is
            // unchanged.
            let f = if is_selected {
                TEXT_BOLD_WHITE
            } else {
                TEXT_BOLD_BLACK
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
    } else if PetKind::roster().is_empty() {
        // Nothing to choose from — `PETS.CFG` is missing, empty or declared
        // only unusable prefixes.  Say so instead of drawing a blank screen;
        // the file lives on the USB partition, so this is user-fixable.
        for (i, line) in NO_PETS_LINES.iter().enumerate() {
            draw_row(display, i, line, false)?;
        }
    } else {
        // Pet list.
        for (i, kind) in PetKind::roster().iter().enumerate() {
            draw_row(display, i, kind.name(), i == selection)?;
        }
    }

    // Hint at bottom — suppressed on the empty-roster notice, where Fire has
    // nothing to confirm.
    if !(PHASE.load(Ordering::Relaxed) == PHASE_PET && PetKind::roster().is_empty()) {
        let hint_style = crate::ui::CENTERED_BOTTOM;
        Text::with_text_style(
            PROMPT_CONFIRM,
            Point::new(76, 148),
            TEXT_BLACK,
            hint_style,
        )
        .draw(display)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// All tests in this module poke the same process-wide statics
    /// (`SELECTION`/`PHASE`/`ACTIVE`/`MODE`/`CHOSEN_KIND_IDX`, plus, for
    /// the full-flow tests, `lifecycle`'s global `GAME`). `cargo test`
    /// runs test functions on multiple threads by default, so without
    /// serialization these would race each other. Every test takes the shared
    /// game-wide test lock first and resets the statics it touches.
    use crate::game::pet_registry::install_test_roster;

    /// Up at the top of the roster used to be a no-op; it should now wrap
    /// to the last pet, and Down at the bottom should wrap back to the
    /// first — same "wrap instead of doing nothing" fix applied across
    /// every scrollable menu on the badge.
    #[test]
    fn cursor_wraps_at_both_ends() {
        let _guard = crate::game::lock_test_state();
        install_test_roster();
        let max = PetKind::roster().len() as u8;
        assert!(max > 0, "roster must be installed for the cursor to move");
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
        let _guard = crate::game::lock_test_state();
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
        let _guard = crate::game::lock_test_state();
        install_test_roster();
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
        let _guard = crate::game::lock_test_state();
        install_test_roster();
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
        let _guard = crate::game::lock_test_state();
        install_test_roster();
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
        let _guard = crate::game::lock_test_state();
        install_test_roster();
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
