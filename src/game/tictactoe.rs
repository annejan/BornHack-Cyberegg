//! Tic-tac-toe mini-game.
//!
//! The player (X, red) plays against a simple AI (O, black).
//! Winning awards HAX (when money mode is on) plus this game's cooldown.
//!
//! State is held in module-level atomics so it integrates with the
//! existing single-threaded game loop without allocations.

use core::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::mono_font::iso_8859_1::FONT_7X13_BOLD;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{Line, PrimitiveStyle, Rectangle};
use embedded_graphics::text::{Alignment, Baseline, Text, TextStyleBuilder};

use crate::{BLACK, TriColor, WHITE, ui};

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
const AI: u8 = 2; // O

// ── Global state ─────────────────────────────────────────────────────────────

/// Whether the tic-tac-toe screen is active.
static ACTIVE: AtomicBool = AtomicBool::new(false);
/// Cursor position (0..8).
static CURSOR: AtomicU8 = AtomicU8::new(4);
/// Board cells packed into two u32s wouldn't work nicely; use 9 AtomicU8s.
/// Index: 0=top-left, 1=top-mid, 2=top-right, 3=mid-left, ... 8=bot-right.
static BOARD: [AtomicU8; 9] = [
    AtomicU8::new(0),
    AtomicU8::new(0),
    AtomicU8::new(0),
    AtomicU8::new(0),
    AtomicU8::new(0),
    AtomicU8::new(0),
    AtomicU8::new(0),
    AtomicU8::new(0),
    AtomicU8::new(0),
];
/// Game result: 0 = in progress, 1 = player won, 2 = AI won, 3 = draw.
static RESULT: AtomicU8 = AtomicU8::new(0);
/// Whose turn: true = player, false = AI.
static PLAYER_TURN: AtomicBool = AtomicBool::new(true);

/// Difficulty: 0 = Normal (mistakes possible), 1 = Impossible (perfect
/// minimax).
const DIFFICULTY_IMPOSSIBLE: u8 = 1;
static DIFFICULTY: AtomicU8 = AtomicU8::new(DIFFICULTY_IMPOSSIBLE);
/// Difficulty-picker overlay open at the start of each game.
static MENU_OPEN: AtomicBool = AtomicBool::new(false);
/// Difficulty-picker selection (0 = Normal, 1 = Impossible).
static MENU_POS: AtomicU8 = AtomicU8::new(DIFFICULTY_IMPOSSIBLE);
/// Probability (out of 100) that the Normal AI ignores minimax and
/// plays a random legal move instead.  35 % gives the player a real
/// shot at winning without making the AI feel completely braindead.
const NORMAL_MISTAKE_PCT: u32 = 35;

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
    // Difficulty picker first; play starts after Fire confirms.
    MENU_POS.store(DIFFICULTY_IMPOSSIBLE, Ordering::Relaxed);
    MENU_OPEN.store(true, Ordering::Relaxed);
    ACTIVE.store(true, Ordering::Relaxed);
}

pub fn close() {
    ACTIVE.store(false, Ordering::Relaxed);
}

// ── Input handling ───────────────────────────────────────────────────────────

pub fn cursor_up() {
    if MENU_OPEN.load(Ordering::Relaxed) {
        let p = MENU_POS.load(Ordering::Relaxed);
        if p > 0 {
            MENU_POS.store(p - 1, Ordering::Relaxed);
        }
        return;
    }
    let c = CURSOR.load(Ordering::Relaxed);
    if c >= 3 {
        CURSOR.store(c - 3, Ordering::Relaxed);
    }
}

pub fn cursor_down() {
    if MENU_OPEN.load(Ordering::Relaxed) {
        let p = MENU_POS.load(Ordering::Relaxed);
        if p < 1 {
            MENU_POS.store(p + 1, Ordering::Relaxed);
        }
        return;
    }
    let c = CURSOR.load(Ordering::Relaxed);
    if c <= 5 {
        CURSOR.store(c + 3, Ordering::Relaxed);
    }
}

pub fn cursor_left() {
    if MENU_OPEN.load(Ordering::Relaxed) {
        return;
    }
    let c = CURSOR.load(Ordering::Relaxed);
    if !c.is_multiple_of(3) {
        CURSOR.store(c - 1, Ordering::Relaxed);
    }
}

pub fn cursor_right() {
    if MENU_OPEN.load(Ordering::Relaxed) {
        return;
    }
    let c = CURSOR.load(Ordering::Relaxed);
    if c % 3 < 2 {
        CURSOR.store(c + 1, Ordering::Relaxed);
    }
}

/// Player places their mark. Returns true if the game ended (win/draw).
pub fn place() -> bool {
    // Difficulty picker open: confirm selection and start the game.
    if MENU_OPEN.load(Ordering::Relaxed) {
        DIFFICULTY.store(MENU_POS.load(Ordering::Relaxed), Ordering::Relaxed);
        MENU_OPEN.store(false, Ordering::Relaxed);
        return false;
    }

    // If game is over, any Fire press closes.
    if RESULT.load(Ordering::Relaxed) != 0 {
        // Award inspiration on win or draw.
        let r = RESULT.load(Ordering::Relaxed);
        if r == 1 || r == 3 {
            super::lifecycle::award_inspiration(super::engine::MiniGame::TicTacToe);
            super::show_toast(super::Toast::MinigameWin);
        }
        close();
        return true;
    }

    if !PLAYER_TURN.load(Ordering::Relaxed) {
        return false;
    }

    let pos = CURSOR.load(Ordering::Relaxed) as usize;
    if pos >= 9 {
        return false;
    }
    if BOARD[pos].load(Ordering::Relaxed) != EMPTY {
        return false;
    }

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
        [0, 1, 2],
        [3, 4, 5],
        [6, 7, 8], // rows
        [0, 3, 6],
        [1, 4, 7],
        [2, 5, 8], // cols
        [0, 4, 8],
        [2, 4, 6], // diags
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

/// Pseudo-random byte derived from the current uptime — good enough for
/// the Normal-mode coin flip; no persistent state needed.
fn entropy_byte() -> u8 {
    #[cfg(feature = "embassy-base")]
    {
        embassy_time::Instant::now().as_ticks() as u8
    }
    #[cfg(not(feature = "embassy-base"))]
    {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u8)
            .unwrap_or(0xCB)
    }
}

fn ai_move(board: &[u8; 9]) -> Option<usize> {
    let perfect = DIFFICULTY.load(Ordering::Relaxed) == DIFFICULTY_IMPOSSIBLE;

    // Normal mode: roll a die.  On a "mistake" outcome, pick a random
    // legal cell instead of the minimax best.  Threats the player has
    // built up may go unblocked — that's the player's chance to win.
    if !perfect && (entropy_byte() as u32 * 100 / 256) < NORMAL_MISTAKE_PCT {
        let empties = board.iter().filter(|&&c| c == EMPTY).count();
        if empties == 0 {
            return None;
        }
        let pick = (entropy_byte() as usize) % empties;
        return board
            .iter()
            .enumerate()
            .filter(|&(_, &c)| c == EMPTY)
            .nth(pick)
            .map(|(i, _)| i);
    }

    // Otherwise: full minimax — perfect play.
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
        [0, 1, 2],
        [3, 4, 5],
        [6, 7, 8],
        [0, 3, 6],
        [1, 4, 7],
        [2, 5, 8],
        [0, 4, 8],
        [2, 4, 6],
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
    let grid_style = PrimitiveStyle::with_stroke(BLACK, 3);
    for i in 1..3 {
        let offset = BOARD_Y + i * CELL;
        // Horizontal.
        Line::new(
            Point::new(BOARD_X, offset),
            Point::new(BOARD_X + 3 * CELL, offset),
        )
        .into_styled(grid_style)
        .draw(display)?;
        // Vertical.
        let offset_x = BOARD_X + i * CELL;
        Line::new(
            Point::new(offset_x, BOARD_Y),
            Point::new(offset_x, BOARD_Y + 3 * CELL),
        )
        .into_styled(grid_style)
        .draw(display)?;
    }

    // Draw marks and cursor.
    for (i, &cell) in board.iter().enumerate() {
        let col = (i % 3) as i32;
        let row = (i / 3) as i32;
        let x = BOARD_X + col * CELL;
        let y = BOARD_Y + row * CELL;

        // Cursor highlight: dithered B/W ring tracing the same 3 px
        // box the red stroke previously drew.  Pure B&W so it survives
        // the fast LUT refresh.
        if i == cursor && result == 0 {
            draw_cursor_ring(display, x + 2, y + 2, CELL - 4)?;
        }

        match cell {
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
        1 => "You win!",
        2 => "EI wins!",
        3 => "Draw!",
        _ => "Your turn (X)",
    };
    Text::with_text_style(msg, Point::new(76, 134), font, centered).draw(display)?;

    if MENU_OPEN.load(Ordering::Relaxed) {
        ui::draw_picker_menu(
            display,
            "Difficulty",
            &["Normal", "Impossible"],
            MENU_POS.load(Ordering::Relaxed) as usize,
        )?;
    }

    Ok(())
}

/// Draw an X mark (two diagonal black lines).
fn draw_x<D>(display: &mut D, x: i32, y: i32, size: i32) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let style = PrimitiveStyle::with_stroke(BLACK, 3);
    Line::new(Point::new(x, y), Point::new(x + size, y + size))
        .into_styled(style)
        .draw(display)?;
    Line::new(Point::new(x + size, y), Point::new(x, y + size))
        .into_styled(style)
        .draw(display)?;
    Ok(())
}

/// Draw an O mark (black rectangle outline).
fn draw_o<D>(display: &mut D, x: i32, y: i32, size: i32) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    Rectangle::new(Point::new(x, y), Size::new(size as u32, size as u32))
        .into_styled(PrimitiveStyle::with_stroke(BLACK, 3))
        .draw(display)?;
    Ok(())
}

/// Dithered (50 % checkerboard) B/W ring around a cell — same 3 px
/// border thickness as the previous red stroke, but pure black/white
/// so the fast LUT refresh keeps it visible during play.
fn draw_cursor_ring<D>(display: &mut D, x: i32, y: i32, size: i32) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    const THICK: i32 = 3;
    let pixels = (0..size).flat_map(move |dy| {
        (0..size).filter_map(move |dx| {
            let on_border = dx < THICK || dy < THICK || dx >= size - THICK || dy >= size - THICK;
            if !on_border {
                return None;
            }
            let px = x + dx;
            let py = y + dy;
            let color = if (px + py) & 1 == 0 { BLACK } else { WHITE };
            Some(Pixel(Point::new(px, py), color))
        })
    });
    display.draw_iter(pixels)
}
