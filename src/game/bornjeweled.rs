//! Bornjeweled-like match-3 game on a 6×6 grid.
//!
//! Swap adjacent gems to make rows/columns of 3+ identical gems.
//! Matched gems disappear and new ones fall from the top.
//! Score points for each match — game ends after 30 moves.

use core::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, Ordering};

use crate::format;
use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::mono_font::ascii::FONT_6X10;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{Circle, PrimitiveStyle, PrimitiveStyleBuilder, Rectangle};
use embedded_graphics::text::{Alignment, Text};

use crate::{BLACK, RED, TriColor, WHITE};

// ── Layout ──────────────────────────────────────────────────────────────────

const BOARD_W: usize = 6;
const BOARD_H: usize = 6;
const CELLS: usize = BOARD_W * BOARD_H;

const BOARD_X: i32 = 14;
const BOARD_Y: i32 = 20;
const CELL: i32 = 22;
const GEM_R: i32 = 8;

const STATUS_Y: i32 = 8;
const MOVES_Y: i32 = 156;

// ── Game state (atomics) ──────────────────────────────────────────────────

static ACTIVE: AtomicBool = AtomicBool::new(false);
static CURSOR: AtomicU8 = AtomicU8::new(0);
static SWAP_START: AtomicU8 = AtomicU8::new(255);
static MOVE_COUNT: AtomicU8 = AtomicU8::new(0);
static SCORE: AtomicU32 = AtomicU32::new(0);
static BOARD: [AtomicU8; CELLS] = [const { AtomicU8::new(0) }; CELLS];
static GAME_RNG: AtomicU32 = AtomicU32::new(0xDEAD_BEEF);

// ── Public API ─────────────────────────────────────────────────────────────

fn entropy_byte() -> u8 {
    let mut s = GAME_RNG.load(Ordering::Relaxed);
    s ^= s << 13;
    s ^= s >> 17;
    s ^= s << 5;
    GAME_RNG.store(s, Ordering::Relaxed);
    (s >> 24) as u8
}

fn random_gem() -> u8 {
    (entropy_byte() % 6) + 1
}

pub fn is_active() -> bool {
    ACTIVE.load(Ordering::Relaxed)
}

pub fn open() {
    for cell in &BOARD {
        cell.store(random_gem(), Ordering::Relaxed);
    }
    CURSOR.store(0, Ordering::Relaxed);
    SWAP_START.store(255, Ordering::Relaxed);
    MOVE_COUNT.store(0, Ordering::Relaxed);
    SCORE.store(0, Ordering::Relaxed);
    GAME_RNG.store(0xDEAD_BEEF, Ordering::Relaxed);
    ACTIVE.store(true, Ordering::Relaxed);
}

pub fn close() {
    ACTIVE.store(false, Ordering::Relaxed);
    crate::FULL_REFRESH_PENDING.store(true, Ordering::Relaxed);
}

// ── Input ──────────────────────────────────────────────────────────────────

fn cursor_up() {
    let c = CURSOR.load(Ordering::Relaxed) as usize;
    let new = if c >= BOARD_W { c - BOARD_W } else { c };
    CURSOR.store(new as u8, Ordering::Relaxed);
}

fn cursor_down() {
    let c = CURSOR.load(Ordering::Relaxed) as usize;
    let new = if c + BOARD_W < CELLS { c + BOARD_W } else { c };
    CURSOR.store(new as u8, Ordering::Relaxed);
}

fn cursor_left() {
    let c = CURSOR.load(Ordering::Relaxed) as usize;
    let new = if c % BOARD_W > 0 { c - 1 } else { c };
    CURSOR.store(new as u8, Ordering::Relaxed);
}

fn cursor_right() {
    let c = CURSOR.load(Ordering::Relaxed) as usize;
    let new = if c % BOARD_W < BOARD_W - 1 { c + 1 } else { c };
    CURSOR.store(new as u8, Ordering::Relaxed);
}

fn do_swap(a: usize, b: usize) {
    let tmp = BOARD[a].load(Ordering::Relaxed);
    BOARD[a].store(BOARD[b].load(Ordering::Relaxed), Ordering::Relaxed);
    BOARD[b].store(tmp, Ordering::Relaxed);
}

fn try_match() -> bool {
    let mut found = false;
    for idx in 0..CELLS {
        let gem = BOARD[idx].load(Ordering::Relaxed);
        if gem == 0 { continue; }
        if idx % BOARD_W <= BOARD_W - 3 {
            let g1 = BOARD[idx + 1].load(Ordering::Relaxed);
            let g2 = BOARD[idx + 2].load(Ordering::Relaxed);
            if gem == g1 && gem == g2 {
                BOARD[idx].store(0, Ordering::Relaxed);
                BOARD[idx + 1].store(0, Ordering::Relaxed);
                BOARD[idx + 2].store(0, Ordering::Relaxed);
                SCORE.fetch_add(30, Ordering::Relaxed);
                found = true;
            }
        }
        if idx + 2 * BOARD_W < CELLS {
            let g1 = BOARD[idx + BOARD_W].load(Ordering::Relaxed);
            let g2 = BOARD[idx + 2 * BOARD_W].load(Ordering::Relaxed);
            if gem == g1 && gem == g2 {
                BOARD[idx].store(0, Ordering::Relaxed);
                BOARD[idx + BOARD_W].store(0, Ordering::Relaxed);
                BOARD[idx + 2 * BOARD_W].store(0, Ordering::Relaxed);
                SCORE.fetch_add(30, Ordering::Relaxed);
                found = true;
            }
        }
    }
    found
}

fn apply_gravity() {
    for col in 0..BOARD_W {
        let mut write_row = BOARD_H - 1;
        for row in (0..BOARD_H).rev() {
            let idx = row * BOARD_W + col;
            if BOARD[idx].load(Ordering::Relaxed) != 0 {
                if write_row >= row {
                    // no move needed
                } else {
                    BOARD[(write_row * BOARD_W + col) as usize]
                        .store(BOARD[idx].load(Ordering::Relaxed), Ordering::Relaxed);
                    BOARD[idx].store(0, Ordering::Relaxed);
                }
                if write_row > 0 {
                    write_row -= 1;
                }
            }
        }
        for row in 0..=write_row {
            BOARD[(row * BOARD_W + col) as usize].store(random_gem(), Ordering::Relaxed);
        }
    }
}

fn fire() {
    let swap = SWAP_START.load(Ordering::Relaxed);
    if swap == 255 {
        SWAP_START.store(CURSOR.load(Ordering::Relaxed), Ordering::Relaxed);
    } else {
        let a = swap as usize;
        let b = CURSOR.load(Ordering::Relaxed) as usize;
        let adj = (a % BOARD_W == b % BOARD_W && (a as i32 - b as i32).abs() == BOARD_W as i32)
            || ((a / BOARD_W == b / BOARD_W) && (a as i32 - b as i32).abs() == 1);
        if adj {
            do_swap(a, b);
            MOVE_COUNT.fetch_add(1, Ordering::Relaxed);
            while try_match() {
                apply_gravity();
            }
        }
        SWAP_START.store(255, Ordering::Relaxed);
    }
}

pub fn dispatch(btn: crate::menu::ButtonId) -> bool {
    if !is_active() {
        return false;
    }
    match btn {
        crate::menu::ButtonId::Up => cursor_up(),
        crate::menu::ButtonId::Down => cursor_down(),
        crate::menu::ButtonId::Left => cursor_left(),
        crate::menu::ButtonId::Right => cursor_right(),
        crate::menu::ButtonId::Fire | crate::menu::ButtonId::Execute => fire(),
        crate::menu::ButtonId::Cancel => close(),
    }
    true
}

// ── Rendering ─────────────────────────────────────────────────────────────

fn gem_style(gem: u8) -> PrimitiveStyle<TriColor> {
    match gem {
        1 => PrimitiveStyle::with_fill(BLACK),                              // solid black
        2 => PrimitiveStyle::with_fill(WHITE),                              // solid white
        3 => PrimitiveStyleBuilder::new()                                   // red ring
            .stroke_color(RED)
            .stroke_width(4)
            .build(),
        4 => PrimitiveStyleBuilder::new()                                   // black center, white ring
            .fill_color(BLACK)
            .stroke_color(WHITE)
            .stroke_width(2)
            .build(),
        5 => PrimitiveStyleBuilder::new()                                   // white center, black ring
            .fill_color(WHITE)
            .stroke_color(BLACK)
            .stroke_width(2)
            .build(),
        6 => PrimitiveStyleBuilder::new()                                   // red center, white ring
            .fill_color(RED)
            .stroke_color(WHITE)
            .stroke_width(2)
            .build(),
        _ => PrimitiveStyle::with_fill(WHITE),
    }
}

pub fn draw<D>(display: &mut D) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let _ = Rectangle::new(Point::new(BOARD_X - 1, BOARD_Y - 1), Size::new(BOARD_W as u32 * CELL as u32 + 2, BOARD_H as u32 * CELL as u32 + 2))
        .into_styled(PrimitiveStyle::with_fill(WHITE))
        .draw(display)?;

    for idx in 0..CELLS {
        let gem = BOARD[idx].load(Ordering::Relaxed);
        if gem == 0 {
            continue;
        }
        let col = (idx % BOARD_W) as i32;
        let row = (idx / BOARD_W) as i32;
        let cx = BOARD_X + col * CELL + CELL / 2;
        let cy = BOARD_Y + row * CELL + CELL / 2;

        let style = gem_style(gem);
        let _ = Circle::new(Point::new(cx - GEM_R, cy - GEM_R), GEM_R as u32 * 2)
            .into_styled(style)
            .draw(display)?;
    }

    // Draw cursor
    let c = CURSOR.load(Ordering::Relaxed) as usize;
    let col = (c % BOARD_W) as i32;
    let row = (c / BOARD_W) as i32;
    let x = BOARD_X + col * CELL;
    let y = BOARD_Y + row * CELL;
    let _ = Rectangle::new(Point::new(x, y), Size::new(CELL as u32, CELL as u32))
        .into_styled(PrimitiveStyle::with_stroke(BLACK, 1))
        .draw(display)?;

    // Draw swap-start highlight
    let swap = SWAP_START.load(Ordering::Relaxed);
    if swap != 255 {
        let col = (swap as usize % BOARD_W) as i32;
        let row = (swap as usize / BOARD_W) as i32;
        let x = BOARD_X + col * CELL;
        let y = BOARD_Y + row * CELL;
        let _ = Rectangle::new(Point::new(x, y), Size::new(CELL as u32, CELL as u32))
            .into_styled(PrimitiveStyle::with_stroke(RED, 2))
            .draw(display)?;
    }

    // Status bar
    let moves = MOVE_COUNT.load(Ordering::Relaxed);
    let score = SCORE.load(Ordering::Relaxed);

    let style = MonoTextStyle::new(&FONT_6X10, BLACK);
    let _ = Text::with_alignment(
        &format!(20; "Moves: {}", moves).unwrap(),
        Point::new(4, STATUS_Y),
        style,
        Alignment::Left,
    )
    .draw(display)?;

    let _ = Text::with_alignment(
        &format!(20; "Score: {}", score).unwrap(),
        Point::new(148, STATUS_Y),
        style,
        Alignment::Right,
    )
    .draw(display)?;

    let moves_left = 30u8.saturating_sub(moves);
    let _ = Text::with_alignment(
        &format!(20; "Left: {}", moves_left).unwrap(),
        Point::new(76, MOVES_Y),
        style,
        Alignment::Center,
    )
    .draw(display)?;

    Ok(())
}
