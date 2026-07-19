//! Lights Out mini-game.
//!
//! A 5×5 grid of cells that are either lit (filled) or dark (empty).
//! Toggling a cell also toggles its orthogonal neighbours.
//! Goal: turn all lights off.
//!
//! Winning awards HAX (when money mode is on) plus this game's cooldown.
//!
//! State is held in module-level atomics — no heap, no alloc.

use core::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, Ordering};

use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::mono_font::iso_8859_1::FONT_7X13_BOLD;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{PrimitiveStyle, Rectangle};
use embedded_graphics::text::{Alignment, Baseline, Text, TextStyleBuilder};

use crate::{BLACK, TriColor, WHITE};

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
    let seed = seed();
    let mut board: u32 = 0;
    let mut rng = seed;
    // Apply 8–12 random toggles to guarantee solvability.
    let n_toggles = 8 + (rng % 5) as usize;
    for _ in 0..n_toggles {
        rng = xorshift(rng);
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
    ACTIVE.store(false, Ordering::Relaxed);
    // Many fast LUT (Mode 1) refreshes happen during play; switch the
    // next refresh to the full OTP waveform (Mode 2) so any residual
    // ghosting from the cell grid is cleared in one cycle when we
    // return to the game screen.
    crate::FULL_REFRESH_PENDING.store(true, core::sync::atomic::Ordering::Relaxed);
}

// ── Input handling ───────────────────────────────────────────────────────────

pub fn cursor_up() {
    let c = CURSOR.load(Ordering::Relaxed);
    if c >= GRID as u8 {
        CURSOR.store(c - GRID as u8, Ordering::Relaxed);
    }
}

pub fn cursor_down() {
    let c = CURSOR.load(Ordering::Relaxed);
    if c + GRID as u8 <= 24 {
        CURSOR.store(c + GRID as u8, Ordering::Relaxed);
    }
}

pub fn cursor_left() {
    let c = CURSOR.load(Ordering::Relaxed);
    if !c.is_multiple_of(GRID as u8) {
        CURSOR.store(c - 1, Ordering::Relaxed);
    }
}

pub fn cursor_right() {
    let c = CURSOR.load(Ordering::Relaxed);
    if c % (GRID as u8) < (GRID as u8 - 1) {
        CURSOR.store(c + 1, Ordering::Relaxed);
    }
}

/// Toggle the cell under the cursor (+ neighbours).
pub fn activate() {
    if SOLVED.load(Ordering::Relaxed) {
        // Puzzle already solved — Fire closes and awards reward.
        super::lifecycle::award_inspiration(super::engine::MiniGame::LightsOut);
        super::show_toast(super::Toast::MinigameWin);
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

/// Simple xorshift32 PRNG.
fn xorshift(mut x: u32) -> u32 {
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    x
}

/// Seed from embassy uptime or a fallback.
fn seed() -> u32 {
    #[cfg(feature = "embassy-base")]
    {
        embassy_time::Instant::now().as_ticks() as u32
    }
    #[cfg(not(feature = "embassy-base"))]
    {
        0xCAFE_BABE
    }
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
    Rectangle::new(Point::zero(), Size::new(152, 152))
        .into_styled(PrimitiveStyle::with_fill(WHITE))
        .draw(display)?;

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
            const THICK: i32 = 6;
            let outer_x = x + 1;
            let outer_y = y + 1;
            let outer_w = CELL - 2;
            let outer_h = CELL - 2;
            let pixels = (0..outer_h).flat_map(move |dy| {
                (0..outer_w).filter_map(move |dx| {
                    let on_border =
                        dx < THICK || dy < THICK || dx >= outer_w - THICK || dy >= outer_h - THICK;
                    if !on_border {
                        return None;
                    }
                    let px = outer_x + dx;
                    let py = outer_y + dy;
                    let color = if (px + py) & 1 == 0 { BLACK } else { WHITE };
                    Some(Pixel(Point::new(px, py), color))
                })
            });
            display.draw_iter(pixels)?;
        }
    }

    // Status text below the grid.
    let centered = TextStyleBuilder::new()
        .baseline(Baseline::Top)
        .alignment(Alignment::Center)
        .build();
    let font = MonoTextStyle::new(&FONT_7X13_BOLD, BLACK);

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
