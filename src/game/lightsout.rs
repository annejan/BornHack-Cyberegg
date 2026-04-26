//! Lights Out mini-game.
//!
//! A 5×5 grid of cells that are either lit (filled) or dark (empty).
//! Toggling a cell also toggles its orthogonal neighbours.
//! Goal: turn all lights off.
//!
//! Winning awards inspiration (reduces `drained`).
//!
//! State is held in module-level atomics — no heap, no alloc.

use core::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, Ordering};

use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::mono_font::ascii::FONT_7X13_BOLD;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{
    PrimitiveStyle, PrimitiveStyleBuilder, Rectangle, StrokeAlignment,
};
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
    ACTIVE.store(true, Ordering::Relaxed);
}

pub fn close() {
    ACTIVE.store(false, Ordering::Relaxed);
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
    if c % (GRID as u8) > 0 {
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
        super::lifecycle::award_inspiration();
        super::show_toast(super::Toast::Inspired);
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

        // Cursor: thick red border.  Drawn with `Inside` alignment so
        // the 6 px stroke sits inside the rectangle, leaving the black
        // cell outline visible around it.
        if i == cursor && !solved {
            let cursor_style = PrimitiveStyleBuilder::new()
                .stroke_color(crate::RED)
                .stroke_width(6)
                .stroke_alignment(StrokeAlignment::Inside)
                .build();
            Rectangle::new(
                Point::new(x + 1, y + 1),
                Size::new((CELL - 2) as u32, (CELL - 2) as u32),
            )
            .into_styled(cursor_style)
            .draw(display)?;
        }
    }

    // Status text below the grid.
    let centered = TextStyleBuilder::new()
        .baseline(Baseline::Top)
        .alignment(Alignment::Center)
        .build();
    let font = MonoTextStyle::new(&FONT_7X13_BOLD, BLACK);

    if solved {
        let mut buf: heapless::String<32> = heapless::String::new();
        let _ = core::fmt::Write::write_fmt(&mut buf, format_args!("Solved in {} moves!", moves));
        Text::with_text_style(buf.as_str(), Point::new(76, 134), font, centered).draw(display)?;
    } else {
        let mut buf: heapless::String<24> = heapless::String::new();
        let _ = core::fmt::Write::write_fmt(&mut buf, format_args!("Moves: {}", moves));
        Text::with_text_style(buf.as_str(), Point::new(76, 134), font, centered).draw(display)?;
    }

    Ok(())
}
