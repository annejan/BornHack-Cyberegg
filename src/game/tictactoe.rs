//! Tic-tac-toe mini-game.
//!
//! The player (X, red) plays against a simple AI (O, black).
//! Winning reduces the pet's `drained` stat (more inspired).
//!
//! State is held in module-level atomics so it integrates with the
//! existing single-threaded game loop without allocations.

use core::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use embedded_graphics::{
    mono_font::{ascii::FONT_7X13_BOLD, MonoTextStyle},
    prelude::*,
    primitives::{PrimitiveStyle, Rectangle, Line},
    text::{Alignment, Baseline, Text, TextStyleBuilder},
};

use crate::{BLACK, TriColor, WHITE};

// ── Constants ────────────────────────────────────────────────────────────────

/// Board origin (top-left of the 3×3 grid).
const BOARD_X: i32 = 16;
const BOARD_Y: i32 = 16;
/// Size of each cell in pixels.
const CELL: i32 = 38;
/// Padding inside each cell for X/O marks.
const PAD: i32 = 6;

// Cell values stored in BOARD[0..9]:
const EMPTY: u8 = 0;
const PLAYER: u8 = 1; // X
const AI: u8 = 2;     // O

// ── Global state ─────────────────────────────────────────────────────────────

/// Whether the tic-tac-toe screen is active.
static ACTIVE: AtomicBool = AtomicBool::new(false);
/// Cursor position (0..8).
static CURSOR: AtomicU8 = AtomicU8::new(4);
/// Board cells packed into two u32s wouldn't work nicely; use 9 AtomicU8s.
/// Index: 0=top-left, 1=top-mid, 2=top-right, 3=mid-left, ... 8=bot-right.
static BOARD: [AtomicU8; 9] = [
    AtomicU8::new(0), AtomicU8::new(0), AtomicU8::new(0),
    AtomicU8::new(0), AtomicU8::new(0), AtomicU8::new(0),
    AtomicU8::new(0), AtomicU8::new(0), AtomicU8::new(0),
];
/// Game result: 0 = in progress, 1 = player won, 2 = AI won, 3 = draw.
static RESULT: AtomicU8 = AtomicU8::new(0);
/// Whose turn: true = player, false = AI.
static PLAYER_TURN: AtomicBool = AtomicBool::new(true);

// ── Public API ───────────────────────────────────────────────────────────────

pub fn is_active() -> bool {
    ACTIVE.load(Ordering::Relaxed)
}

pub fn open() {
    // Reset board.
    for cell in &BOARD {
        cell.store(EMPTY, Ordering::Relaxed);
    }
    CURSOR.store(4, Ordering::Relaxed);
    RESULT.store(0, Ordering::Relaxed);
    PLAYER_TURN.store(true, Ordering::Relaxed);
    ACTIVE.store(true, Ordering::Relaxed);
}

pub fn close() {
    ACTIVE.store(false, Ordering::Relaxed);
}

// ── Input handling ───────────────────────────────────────────────────────────

pub fn cursor_up() {
    let c = CURSOR.load(Ordering::Relaxed);
    if c >= 3 { CURSOR.store(c - 3, Ordering::Relaxed); }
}

pub fn cursor_down() {
    let c = CURSOR.load(Ordering::Relaxed);
    if c <= 5 { CURSOR.store(c + 3, Ordering::Relaxed); }
}

pub fn cursor_left() {
    let c = CURSOR.load(Ordering::Relaxed);
    if c % 3 > 0 { CURSOR.store(c - 1, Ordering::Relaxed); }
}

pub fn cursor_right() {
    let c = CURSOR.load(Ordering::Relaxed);
    if c % 3 < 2 { CURSOR.store(c + 1, Ordering::Relaxed); }
}

/// Player places their mark. Returns true if the game ended (win/draw).
pub fn place() -> bool {
    // If game is over, any Fire press closes.
    if RESULT.load(Ordering::Relaxed) != 0 {
        // Award inspiration if player won.
        if RESULT.load(Ordering::Relaxed) == 1 {
            super::lifecycle::award_inspiration();
        }
        close();
        return true;
    }

    if !PLAYER_TURN.load(Ordering::Relaxed) { return false; }

    let pos = CURSOR.load(Ordering::Relaxed) as usize;
    if pos >= 9 { return false; }
    if BOARD[pos].load(Ordering::Relaxed) != EMPTY { return false; }

    // Player move.
    BOARD[pos].store(PLAYER, Ordering::Relaxed);
    if let Some(result) = check_board() {
        RESULT.store(result, Ordering::Relaxed);
        return true;
    }

    // AI move.
    PLAYER_TURN.store(false, Ordering::Relaxed);
    let board = read_board();
    if let Some(ai_pos) = ai_move(&board) {
        BOARD[ai_pos].store(AI, Ordering::Relaxed);
        if let Some(result) = check_board() {
            RESULT.store(result, Ordering::Relaxed);
            return true;
        }
    }
    PLAYER_TURN.store(true, Ordering::Relaxed);
    false
}

// ── Board helpers ────────────────────────────────────────────────────────────

fn read_board() -> [u8; 9] {
    let mut b = [0u8; 9];
    for (i, cell) in BOARD.iter().enumerate() {
        b[i] = cell.load(Ordering::Relaxed);
    }
    b
}

/// Check for win or draw. Returns Some(1)=player, Some(2)=AI, Some(3)=draw.
fn check_board() -> Option<u8> {
    let b = read_board();
    const LINES: [[usize; 3]; 8] = [
        [0,1,2], [3,4,5], [6,7,8], // rows
        [0,3,6], [1,4,7], [2,5,8], // cols
        [0,4,8], [2,4,6],          // diags
    ];
    for line in &LINES {
        let a = b[line[0]];
        if a != EMPTY && a == b[line[1]] && a == b[line[2]] {
            return Some(a); // 1=player, 2=AI
        }
    }
    if b.iter().all(|&c| c != EMPTY) {
        return Some(3); // draw
    }
    None
}

// ── AI (minimax) ─────────────────────────────────────────────────────────────

fn ai_move(board: &[u8; 9]) -> Option<usize> {
    let mut best_score = i8::MIN;
    let mut best_pos = None;
    for i in 0..9 {
        if board[i] == EMPTY {
            let mut b = *board;
            b[i] = AI;
            let score = minimax(&b, false, 0);
            if score > best_score {
                best_score = score;
                best_pos = Some(i);
            }
        }
    }
    best_pos
}

fn minimax(board: &[u8; 9], is_ai: bool, depth: u8) -> i8 {
    // Check terminal states.
    if let Some(winner) = winner(board) {
        return match winner {
            AI => 10 - depth as i8,     // AI wins (prefer faster)
            PLAYER => depth as i8 - 10, // player wins
            _ => 0,                     // draw
        };
    }
    // Check draw.
    if board.iter().all(|&c| c != EMPTY) {
        return 0;
    }

    if is_ai {
        let mut best = i8::MIN;
        for i in 0..9 {
            if board[i] == EMPTY {
                let mut b = *board;
                b[i] = AI;
                best = best.max(minimax(&b, false, depth + 1));
            }
        }
        best
    } else {
        let mut best = i8::MAX;
        for i in 0..9 {
            if board[i] == EMPTY {
                let mut b = *board;
                b[i] = PLAYER;
                best = best.min(minimax(&b, true, depth + 1));
            }
        }
        best
    }
}

fn winner(board: &[u8; 9]) -> Option<u8> {
    const LINES: [[usize; 3]; 8] = [
        [0,1,2], [3,4,5], [6,7,8],
        [0,3,6], [1,4,7], [2,5,8],
        [0,4,8], [2,4,6],
    ];
    for line in &LINES {
        let a = board[line[0]];
        if a != EMPTY && a == board[line[1]] && a == board[line[2]] {
            return Some(a);
        }
    }
    None
}

// ── Drawing ──────────────────────────────────────────────────────────────────

pub fn draw<D>(display: &mut D) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let board = read_board();
    let cursor = CURSOR.load(Ordering::Relaxed) as usize;
    let result = RESULT.load(Ordering::Relaxed);

    // Background.
    Rectangle::new(Point::zero(), Size::new(152, 152))
        .into_styled(PrimitiveStyle::with_fill(WHITE))
        .draw(display)?;

    // Grid lines (2 horizontal + 2 vertical).
    let grid_style = PrimitiveStyle::with_stroke(BLACK, 2);
    for i in 1..3 {
        let offset = BOARD_Y + i as i32 * CELL;
        // Horizontal.
        Line::new(
            Point::new(BOARD_X, offset),
            Point::new(BOARD_X + 3 * CELL, offset),
        ).into_styled(grid_style).draw(display)?;
        // Vertical.
        let offset_x = BOARD_X + i as i32 * CELL;
        Line::new(
            Point::new(offset_x, BOARD_Y),
            Point::new(offset_x, BOARD_Y + 3 * CELL),
        ).into_styled(grid_style).draw(display)?;
    }

    // Draw marks and cursor.
    for i in 0..9 {
        let col = (i % 3) as i32;
        let row = (i / 3) as i32;
        let x = BOARD_X + col * CELL;
        let y = BOARD_Y + row * CELL;

        // Cursor highlight (red border).
        if i == cursor && result == 0 {
            Rectangle::new(
                Point::new(x + 2, y + 2),
                Size::new((CELL - 4) as u32, (CELL - 4) as u32),
            )
            .into_styled(PrimitiveStyle::with_stroke(crate::RED, 2))
            .draw(display)?;
        }

        match board[i] {
            PLAYER => draw_x(display, x + PAD, y + PAD, CELL - 2 * PAD)?,
            AI => draw_o(display, x + PAD, y + PAD, CELL - 2 * PAD)?,
            _ => {}
        }
    }

    // Status text below the grid.
    let centered = TextStyleBuilder::new()
        .baseline(Baseline::Top)
        .alignment(Alignment::Center)
        .build();
    let font = MonoTextStyle::new(&FONT_7X13_BOLD, BLACK);
    let msg = match result {
        1 => "You win! +inspired",
        2 => "AI wins!",
        3 => "Draw!",
        _ => "Your turn (X)",
    };
    Text::with_text_style(msg, Point::new(76, 134), font, centered)
        .draw(display)?;

    Ok(())
}

/// Draw an X mark (two diagonal lines, red).
fn draw_x<D>(display: &mut D, x: i32, y: i32, size: i32) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let style = PrimitiveStyle::with_stroke(crate::RED, 2);
    Line::new(Point::new(x, y), Point::new(x + size, y + size))
        .into_styled(style).draw(display)?;
    Line::new(Point::new(x + size, y), Point::new(x, y + size))
        .into_styled(style).draw(display)?;
    Ok(())
}

/// Draw an O mark (rectangle outline, black).
fn draw_o<D>(display: &mut D, x: i32, y: i32, size: i32) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    Rectangle::new(Point::new(x, y), Size::new(size as u32, size as u32))
        .into_styled(PrimitiveStyle::with_stroke(BLACK, 2))
        .draw(display)?;
    Ok(())
}
