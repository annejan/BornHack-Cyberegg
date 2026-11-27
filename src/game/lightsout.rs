//! Lights Out mini-game.
//!
//! A 5×5 grid of cells that are either lit (filled) or dark (empty).
//! Toggling a cell also toggles its orthogonal neighbours.
//! Goal: turn all lights off.
//!
//! Solving it awards HAX (when money mode is on) plus this game's cooldown,
//! paid in `close()`.
//!
//! State is held in module-level atomics — no heap, no alloc.

use core::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, Ordering};

use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{PrimitiveStyle, Rectangle};
use embedded_graphics::text::Text;

use crate::menu::ButtonId;
use crate::{BLACK, TriColor, ui};

// ── Constants ────────────────────────────────────────────────────────────────

const GRID: usize = 5;
/// Board origin (top-left of the 5×5 grid).
const BOARD_X: i32 = 16;
const BOARD_Y: i32 = 10;
/// Size of each cell in pixels.
const CELL: i32 = 24;
/// Inner fill inset from cell border.  Wider inset = more white margin
/// around each lit-cell square so the red cursor stands out clearly on
/// low-contrast EPDs.
const INSET: i32 = 4;

// ── Global state ─────────────────────────────────────────────────────────────

/// Whether the lights-out screen is active.
static ACTIVE: AtomicBool = AtomicBool::new(false);
/// Cursor position (0..24, row-major).
static CURSOR: AtomicU8 = AtomicU8::new(12); // centre
/// Board state: 25 bits packed into a u32. Bit i = cell i is lit.
static BOARD: AtomicU32 = AtomicU32::new(0);
/// Move counter.
static MOVES: AtomicU8 = AtomicU8::new(0);
/// True when the puzzle is solved (all lights off).
static SOLVED: AtomicBool = AtomicBool::new(false);
/// True when the board is in an unsolvable equivalence class.  Set
/// defensively after every move (in theory unreachable from a solvable
/// `open()` start, but acts as an assert if `toggle()` is ever broken).
static UNSOLVABLE: AtomicBool = AtomicBool::new(false);

// ── Solvability test (Jaap Scherphuis 5×5 quiet-pattern parity)
// ───────────────
//
// 5×5 Lights Out's null-space has dimension 2.  Two row-major bitmasks
// pick the cells whose XOR-parity must be even for the board to be
// solvable.  See <https://www.jaapsch.net/puzzles/lomath.htm#solvtest>.
//
// `SOLVE_MASK_ROWS`: rows {0, 2, 4} × cols {0, 1, 3, 4}  →  12 cells.
// `SOLVE_MASK_COLS`: cols {0, 2, 4} × rows {0, 1, 3, 4}  →  12 cells.
const SOLVE_MASK_ROWS: u32 = 0x01B0_6C1B;
const SOLVE_MASK_COLS: u32 = 0x015A_82B5;

const _: () = assert!(SOLVE_MASK_ROWS.count_ones() == 12);
const _: () = assert!(SOLVE_MASK_COLS.count_ones() == 12);

fn is_solvable(board: u32) -> bool {
    (board & SOLVE_MASK_ROWS).count_ones() & 1 == 0
        && (board & SOLVE_MASK_COLS).count_ones() & 1 == 0
}

// ── Public API ───────────────────────────────────────────────────────────────

pub fn is_active() -> bool {
    ACTIVE.load(Ordering::Relaxed)
}

pub fn open() {
    // Generate a solvable random puzzle by applying random toggles
    // from a solved (all-off) board.
    let seed = crate::platform::seed_ticks_or_fixed();
    let mut board: u32 = 0;
    let mut rng = seed;
    // Apply 8–12 random toggles to guarantee solvability.
    let n_toggles = 8 + (rng % 5) as usize;
    for _ in 0..n_toggles {
        rng = crate::platform::xorshift32(rng);
        let cell = (rng % 25) as usize;
        board = toggle(board, cell);
    }
    // If we accidentally got all-off, toggle centre.
    if board == 0 {
        board = toggle(board, 12);
    }
    BOARD.store(board, Ordering::Relaxed);
    CURSOR.store(12, Ordering::Relaxed);
    MOVES.store(0, Ordering::Relaxed);
    SOLVED.store(false, Ordering::Relaxed);
    UNSOLVABLE.store(false, Ordering::Relaxed);
    ACTIVE.store(true, Ordering::Relaxed);
}

pub fn close() {
    // A solved puzzle pays as a win, once, however the screen is left.
    if ACTIVE.swap(false, Ordering::Relaxed) && SOLVED.load(Ordering::Relaxed) {
        super::minigame::pay_result(
            super::engine::MiniGame::LightsOut,
            super::engine::GRADE_WIN,
        );
    }
    // Many fast LUT (Mode 1) refreshes happen during play; switch the
    // next refresh to the full OTP waveform (Mode 2) so any residual
    // ghosting from the cell grid is cleared in one cycle when we
    // return to the game screen.
    crate::FULL_REFRESH_PENDING.store(true, core::sync::atomic::Ordering::Relaxed);
}

/// Route a button press while this mini-game is active.
pub fn dispatch(btn: ButtonId) {
    match btn {
        ButtonId::Cancel => close(),
        ButtonId::Up => cursor_up(),
        ButtonId::Down => cursor_down(),
        ButtonId::Left => cursor_left(),
        ButtonId::Right => cursor_right(),
        ButtonId::Fire | ButtonId::Execute => {
            activate();
        }
    }
}

// ── Input handling ───────────────────────────────────────────────────────────

fn cursor_up() {
    let c = CURSOR.load(Ordering::Relaxed);
    if c >= GRID as u8 {
        CURSOR.store(c - GRID as u8, Ordering::Relaxed);
    }
}

fn cursor_down() {
    let c = CURSOR.load(Ordering::Relaxed);
    if c + GRID as u8 <= 24 {
        CURSOR.store(c + GRID as u8, Ordering::Relaxed);
    }
}

fn cursor_left() {
    let c = CURSOR.load(Ordering::Relaxed);
    if !c.is_multiple_of(GRID as u8) {
        CURSOR.store(c - 1, Ordering::Relaxed);
    }
}

fn cursor_right() {
    let c = CURSOR.load(Ordering::Relaxed);
    if c % (GRID as u8) < (GRID as u8 - 1) {
        CURSOR.store(c + 1, Ordering::Relaxed);
    }
}

/// Toggle the cell under the cursor (+ neighbours).
fn activate() {
    if SOLVED.load(Ordering::Relaxed) {
        // Puzzle already solved — Fire closes (`close` pays the win).
        close();
        return;
    }
    if UNSOLVABLE.load(Ordering::Relaxed) {
        // Defensive end-state — Fire just closes, no reward.
        close();
        return;
    }

    let pos = CURSOR.load(Ordering::Relaxed) as usize;
    if pos >= 25 {
        return;
    }

    let board = toggle(BOARD.load(Ordering::Relaxed), pos);
    BOARD.store(board, Ordering::Relaxed);

    let moves = MOVES.load(Ordering::Relaxed).saturating_add(1);
    MOVES.store(moves, Ordering::Relaxed);

    if board == 0 {
        SOLVED.store(true, Ordering::Relaxed);
    } else if !is_solvable(board) {
        // Should be unreachable from a `open()` start, but if it
        // ever happens (toggle bug, cosmic-ray bit-flip, future
        // change), don't strand the player on an unwinnable puzzle.
        UNSOLVABLE.store(true, Ordering::Relaxed);
    }
}

// ── Board helpers ────────────────────────────────────────────────────────────

/// Toggle cell `pos` and its orthogonal neighbours. Returns new board.
fn toggle(mut board: u32, pos: usize) -> u32 {
    board ^= 1 << pos;
    let row = pos / GRID;
    let col = pos % GRID;
    if row > 0 {
        board ^= 1 << (pos - GRID);
    }
    if row < GRID - 1 {
        board ^= 1 << (pos + GRID);
    }
    if col > 0 {
        board ^= 1 << (pos - 1);
    }
    if col < GRID - 1 {
        board ^= 1 << (pos + 1);
    }
    board
}

// ── Drawing ──────────────────────────────────────────────────────────────────

pub fn draw<D>(display: &mut D) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let board = BOARD.load(Ordering::Relaxed);
    let cursor = CURSOR.load(Ordering::Relaxed) as usize;
    let solved = SOLVED.load(Ordering::Relaxed);
    let moves = MOVES.load(Ordering::Relaxed);

    // Background.
    crate::ui::clear_screen(display)?;

    // Draw 5×5 grid.
    for i in 0..25 {
        let col = (i % GRID) as i32;
        let row = (i / GRID) as i32;
        let x = BOARD_X + col * CELL;
        let y = BOARD_Y + row * CELL;
        let lit = (board >> i) & 1 != 0;

        // Cell outline.
        Rectangle::new(Point::new(x, y), Size::new(CELL as u32, CELL as u32))
            .into_styled(PrimitiveStyle::with_stroke(BLACK, 1))
            .draw(display)?;

        // Lit cell: filled black square.
        if lit {
            Rectangle::new(
                Point::new(x + INSET, y + INSET),
                Size::new((CELL - 2 * INSET) as u32, (CELL - 2 * INSET) as u32),
            )
            .into_styled(PrimitiveStyle::with_fill(BLACK))
            .draw(display)?;
        }

        // Cursor: 6 px dithered (50 % checkerboard) border.  Pure B&W so it
        // survives the fast LUT refresh — the previous red stroke needed a
        // tri-colour update that's too slow for an interactive cursor.
        if i == cursor && !solved {
            ui::draw_dither_ring(
                display,
                Rectangle::new(
                    Point::new(x + 1, y + 1),
                    Size::new((CELL - 2) as u32, (CELL - 2) as u32),
                ),
                6,
            )?;
        }
    }

    // Status text below the grid.
    let centered = crate::ui::CENTERED_TOP;
    let font = crate::ui::TEXT_BOLD_BLACK;

    let unsolvable = UNSOLVABLE.load(Ordering::Relaxed);
    if solved {
        let mut buf: heapless::String<32> = heapless::String::new();
        let _ = core::fmt::Write::write_fmt(&mut buf, format_args!("Solved in {} moves!", moves));
        Text::with_text_style(buf.as_str(), Point::new(76, 134), font, centered).draw(display)?;
    } else if unsolvable {
        Text::with_text_style(
            // Two short centered lines — the single string overflowed 152px.
            "Unsolvable",
            Point::new(76, 127),
            font,
            centered,
        )
        .draw(display)?;
        Text::with_text_style("Fire to exit", Point::new(76, 141), font, centered).draw(display)?;
    } else {
        let mut buf: heapless::String<24> = heapless::String::new();
        let _ = core::fmt::Write::write_fmt(&mut buf, format_args!("Moves: {}", moves));
        Text::with_text_style(buf.as_str(), Point::new(76, 134), font, centered).draw(display)?;
    }

    Ok(())
}

#[cfg(test)]
mod pay_tests {
    use super::*;
    use crate::game::engine::{MiniGame, MINIGAME_HAX_REWARD};
    use crate::game::{current_toast, minigame_pay_read, minigame_pay_setup, Toast};

    const GAME: MiniGame = MiniGame::LightsOut;

    fn open_and_cancel(solved: bool) {
        open();
        SOLVED.store(solved, Ordering::Relaxed);
        dispatch(ButtonId::Cancel);
    }

    #[test]
    fn a_solved_puzzle_closed_with_cancel_pays_the_win() {
        let _g = minigame_pay_setup();
        open_and_cancel(true);
        assert_eq!(minigame_pay_read(GAME), (MINIGAME_HAX_REWARD, true));
        assert_eq!(current_toast(), Some(Toast::MinigameWin));
        assert!(!is_active());
    }

    #[test]
    fn closing_twice_pays_once() {
        let _g = minigame_pay_setup();
        open_and_cancel(true);
        close();
        close();
        assert_eq!(minigame_pay_read(GAME).0, MINIGAME_HAX_REWARD);
    }

    #[test]
    fn an_unsolved_puzzle_pays_nothing() {
        let _g = minigame_pay_setup();
        open_and_cancel(false);
        assert_eq!(minigame_pay_read(GAME), (0, false));
        assert_eq!(current_toast(), None);
    }
}
