//! Bornjeweled-like match-3 game on a 6×6 grid.
//!
//! Swap adjacent gems to make rows/columns of 3+ identical gems.
//! Matched gems disappear and new ones fall from the top.
//! Score points for each match — game ends after 30 moves.

use core::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, Ordering};

use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::mono_font::ascii::FONT_6X10;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{Circle, PrimitiveStyle, Rectangle, Triangle};
use embedded_graphics::text::{Alignment, Text};

use crate::{BLACK, TriColor, WHITE};

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

const SWAP_NONE: u8 = 255;
const MOVES_LIMIT: u8 = 30;

const RESULT_PLAYING: u8 = 0;
const RESULT_GAME_OVER: u8 = 1;

/// Hard cap on cascade iterations after a swap.  In practice cascades
/// converge in 1–3 rounds; this only triggers if the RNG keeps refilling
/// matching gems, which is unlikely but not impossible.  Without this
/// the executor could block longer than is comfortable on a button press.
const MAX_CASCADE_ITERS: u8 = 32;

static ACTIVE: AtomicBool = AtomicBool::new(false);
static CURSOR: AtomicU8 = AtomicU8::new(0);
static SWAP_START: AtomicU8 = AtomicU8::new(SWAP_NONE);
static MOVE_COUNT: AtomicU8 = AtomicU8::new(0);
static SCORE: AtomicU32 = AtomicU32::new(0);
static RESULT: AtomicU8 = AtomicU8::new(RESULT_PLAYING);
static BOARD: [AtomicU8; CELLS] = [const { AtomicU8::new(0) }; CELLS];
static GAME_RNG: AtomicU32 = AtomicU32::new(0xDEAD_BEEF);

// ── RNG ────────────────────────────────────────────────────────────────────

/// Seed `GAME_RNG` from a real entropy source so two consecutive games
/// don't produce identical board layouts.  On hardware: low 32 bits of
/// the embassy monotonic clock.  On the simulator: process-uptime in
/// milliseconds.  Elsewhere (tests): fall back to a fixed seed.
fn seed_rng() {
    #[cfg(feature = "embassy-base")]
    {
        let s = embassy_time::Instant::now().as_ticks() as u32;
        GAME_RNG.store(s.max(1), Ordering::Relaxed);
        return;
    }
    #[cfg(all(feature = "simulator", not(feature = "embassy-base")))]
    {
        let s = super::lifecycle::sim_elapsed_ms() as u32;
        GAME_RNG.store(s.max(1), Ordering::Relaxed);
        return;
    }
    #[cfg(not(any(feature = "embassy-base", feature = "simulator")))]
    {
        GAME_RNG.store(0xDEAD_BEEF, Ordering::Relaxed);
    }
}

fn entropy_byte() -> u8 {
    let mut s = GAME_RNG.load(Ordering::Relaxed);
    if s == 0 {
        s = 0xDEAD_BEEF;
    }
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
    seed_rng();
    for cell in &BOARD {
        cell.store(random_gem(), Ordering::Relaxed);
    }
    CURSOR.store(0, Ordering::Relaxed);
    SWAP_START.store(SWAP_NONE, Ordering::Relaxed);
    MOVE_COUNT.store(0, Ordering::Relaxed);
    SCORE.store(0, Ordering::Relaxed);
    RESULT.store(RESULT_PLAYING, Ordering::Relaxed);
    // Settle any 3-in-a-row that the random initial fill happens to
    // produce — players shouldn't start with free pre-cleared matches
    // from luck-of-the-draw.  Score from this settle isn't counted.
    SCORE.store(0, Ordering::Relaxed);
    let _ = run_cascade();
    SCORE.store(0, Ordering::Relaxed);
    ACTIVE.store(true, Ordering::Relaxed);
}

pub fn close() {
    if ACTIVE.swap(false, Ordering::Relaxed) {
        // Award inspiration scaled by score earned during the game.
        // Match-3 scores ~30 per cleared row; cap the relief at a
        // reasonable per-game max so a fluke can't trivialise the
        // pet's drained stat.
        let score = SCORE.load(Ordering::Relaxed);
        if score > 0 {
            let relief: u16 = (score / 6).min(4096) as u16;
            super::lifecycle::award_inspiration(super::engine::MiniGame::BornJeweled);
            super::lifecycle::add_drained_relief(relief);
            super::show_toast(super::Toast::Inspired);
        }
    }
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

/// Detect runs of 3+ identical gems (horizontal *or* vertical), mark
/// every cell in those runs for clearing, then clear them in a second
/// pass.  Two-pass so a run of 4+ clears all of its cells (the previous
/// implementation only ever cleared the first 3).  Score is 10 per
/// cleared cell, so a plain 3-match still scores 30.
fn try_match() -> bool {
    let mut to_clear = [false; CELLS];

    // Horizontal runs.
    for row in 0..BOARD_H {
        let base = row * BOARD_W;
        let mut col = 0;
        while col < BOARD_W {
            let g = BOARD[base + col].load(Ordering::Relaxed);
            if g == 0 {
                col += 1;
                continue;
            }
            let mut end = col + 1;
            while end < BOARD_W && BOARD[base + end].load(Ordering::Relaxed) == g {
                end += 1;
            }
            if end - col >= 3 {
                for c in col..end {
                    to_clear[base + c] = true;
                }
            }
            col = end;
        }
    }

    // Vertical runs.
    for col in 0..BOARD_W {
        let mut row = 0;
        while row < BOARD_H {
            let g = BOARD[row * BOARD_W + col].load(Ordering::Relaxed);
            if g == 0 {
                row += 1;
                continue;
            }
            let mut end = row + 1;
            while end < BOARD_H && BOARD[end * BOARD_W + col].load(Ordering::Relaxed) == g {
                end += 1;
            }
            if end - row >= 3 {
                for r in row..end {
                    to_clear[r * BOARD_W + col] = true;
                }
            }
            row = end;
        }
    }

    // Apply clears + score (count every cleared cell, including
    // intersections between a horizontal and a vertical run).
    let mut cleared = 0u32;
    for (i, marked) in to_clear.iter().enumerate() {
        if *marked {
            BOARD[i].store(0, Ordering::Relaxed);
            cleared += 1;
        }
    }
    if cleared > 0 {
        SCORE.fetch_add(cleared * 10, Ordering::Relaxed);
        true
    } else {
        false
    }
}

fn apply_gravity() {
    for col in 0..BOARD_W {
        // Pack non-zero gems toward the bottom of the column.  `write_row`
        // tracks the next slot to fill, walking up from one-past-bottom
        // each time we see a non-zero gem.
        let mut write_row = BOARD_H;
        for row in (0..BOARD_H).rev() {
            let idx = row * BOARD_W + col;
            let gem = BOARD[idx].load(Ordering::Relaxed);
            if gem != 0 {
                write_row -= 1;
                if write_row != row {
                    BOARD[write_row * BOARD_W + col].store(gem, Ordering::Relaxed);
                    BOARD[idx].store(0, Ordering::Relaxed);
                }
            }
        }
        // Fill remaining empty rows above with new random gems.
        for row in 0..write_row {
            BOARD[row * BOARD_W + col].store(random_gem(), Ordering::Relaxed);
        }
    }
}

/// Run the match-and-gravity cascade up to [`MAX_CASCADE_ITERS`]
/// iterations.  Returns the number of iterations executed (0 means
/// nothing matched on the very first pass).
fn run_cascade() -> u8 {
    for i in 0..MAX_CASCADE_ITERS {
        if !try_match() {
            return i;
        }
        apply_gravity();
    }
    MAX_CASCADE_ITERS
}

fn fire() {
    if RESULT.load(Ordering::Relaxed) == RESULT_GAME_OVER {
        // Any Fire on the game-over screen closes.
        close();
        return;
    }

    let swap = SWAP_START.load(Ordering::Relaxed);
    if swap == SWAP_NONE {
        SWAP_START.store(CURSOR.load(Ordering::Relaxed), Ordering::Relaxed);
        return;
    }

    let a = swap as usize;
    let b = CURSOR.load(Ordering::Relaxed) as usize;
    let adj = (a % BOARD_W == b % BOARD_W && (a as i32 - b as i32).abs() == BOARD_W as i32)
        || ((a / BOARD_W == b / BOARD_W) && (a as i32 - b as i32).abs() == 1);
    if adj {
        do_swap(a, b);
        let moves = MOVE_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
        let _ = run_cascade();
        if moves >= MOVES_LIMIT {
            RESULT.store(RESULT_GAME_OVER, Ordering::Relaxed);
        }
    }
    SWAP_START.store(SWAP_NONE, Ordering::Relaxed);
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

/// Draw one gem at `(cx, cy)`.  Six visually-distinct shapes, all in
/// black so every redraw stays on the fast Mode1 B/W refresh path —
/// the slow tri-color refresh would otherwise be needed to keep red
/// pixels current, and any red gem moving during gravity would leave
/// a stale ghost at its previous position until the next full update.
///
/// | Gem | Shape          |
/// |-----|----------------|
/// |  1  | Filled circle  |
/// |  2  | Hollow circle  |
/// |  3  | Filled square  |
/// |  4  | Hollow square  |
/// |  5  | Filled diamond |
/// |  6  | Filled triangle|
fn draw_gem<D>(display: &mut D, gem: u8, cx: i32, cy: i32) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let r = GEM_R;
    match gem {
        // Filled circle.
        1 => Circle::new(Point::new(cx - r, cy - r), r as u32 * 2)
            .into_styled(PrimitiveStyle::with_fill(BLACK))
            .draw(display)?,
        // Hollow circle (ring).
        2 => Circle::new(Point::new(cx - r, cy - r), r as u32 * 2)
            .into_styled(PrimitiveStyle::with_stroke(BLACK, 2))
            .draw(display)?,
        // Filled square — fits inside the gem inscribed circle so it
        // doesn't touch the cell border.
        3 => Rectangle::new(
            Point::new(cx - r + 1, cy - r + 1),
            Size::new(r as u32 * 2 - 2, r as u32 * 2 - 2),
        )
        .into_styled(PrimitiveStyle::with_fill(BLACK))
        .draw(display)?,
        // Hollow square (frame).
        4 => Rectangle::new(
            Point::new(cx - r + 1, cy - r + 1),
            Size::new(r as u32 * 2 - 2, r as u32 * 2 - 2),
        )
        .into_styled(PrimitiveStyle::with_stroke(BLACK, 2))
        .draw(display)?,
        // Filled diamond (two triangles sharing the horizontal axis).
        5 => {
            Triangle::new(
                Point::new(cx, cy - r),
                Point::new(cx - r, cy),
                Point::new(cx + r, cy),
            )
            .into_styled(PrimitiveStyle::with_fill(BLACK))
            .draw(display)?;
            Triangle::new(
                Point::new(cx, cy + r),
                Point::new(cx - r, cy),
                Point::new(cx + r, cy),
            )
            .into_styled(PrimitiveStyle::with_fill(BLACK))
            .draw(display)?;
        }
        // Filled triangle, point up.
        6 => Triangle::new(
            Point::new(cx, cy - r),
            Point::new(cx - r, cy + r - 1),
            Point::new(cx + r, cy + r - 1),
        )
        .into_styled(PrimitiveStyle::with_fill(BLACK))
        .draw(display)?,
        _ => {}
    }
    Ok(())
}

pub fn draw<D>(display: &mut D) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    // Full-screen white wipe so the previous frame's content (game
    // icons, pet area, leftover toast, …) doesn't bleed through under
    // and around the board.
    Rectangle::new(Point::zero(), Size::new(152, 152))
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
        draw_gem(display, gem, cx, cy)?;
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

    // Draw swap-start highlight — thicker BLACK ring so it's
    // distinguishable from the cursor's 1px stroke without resorting
    // to the red plane (Mode1 fast refresh skips red, so a red
    // highlight ghosts visibly on every move).
    let swap = SWAP_START.load(Ordering::Relaxed);
    if swap != SWAP_NONE {
        let col = (swap as usize % BOARD_W) as i32;
        let row = (swap as usize / BOARD_W) as i32;
        let x = BOARD_X + col * CELL;
        let y = BOARD_Y + row * CELL;
        Rectangle::new(Point::new(x, y), Size::new(CELL as u32, CELL as u32))
            .into_styled(PrimitiveStyle::with_stroke(BLACK, 3))
            .draw(display)?;
    }

    // Status bar
    let moves = MOVE_COUNT.load(Ordering::Relaxed);
    let score = SCORE.load(Ordering::Relaxed);

    let style = MonoTextStyle::new(&FONT_6X10, BLACK);

    let mut buf: heapless::String<24> = heapless::String::new();
    let _ = core::fmt::Write::write_fmt(&mut buf, format_args!("Moves: {}", moves));
    Text::with_alignment(
        buf.as_str(),
        Point::new(4, STATUS_Y),
        style,
        Alignment::Left,
    )
    .draw(display)?;

    buf.clear();
    let _ = core::fmt::Write::write_fmt(&mut buf, format_args!("Score: {}", score));
    Text::with_alignment(
        buf.as_str(),
        Point::new(148, STATUS_Y),
        style,
        Alignment::Right,
    )
    .draw(display)?;

    let moves_left = MOVES_LIMIT.saturating_sub(moves);
    buf.clear();
    let _ = core::fmt::Write::write_fmt(&mut buf, format_args!("Left: {}", moves_left));
    Text::with_alignment(
        buf.as_str(),
        Point::new(76, MOVES_Y),
        style,
        Alignment::Center,
    )
    .draw(display)?;

    // ── Game-over overlay ────────────────────────────────────────────
    if RESULT.load(Ordering::Relaxed) == RESULT_GAME_OVER {
        // Frame the centre of the board in white so the overlay text is
        // readable on top of whatever gem layout was last shown.
        Rectangle::new(Point::new(20, 60), Size::new(112, 50))
            .into_styled(PrimitiveStyle::with_fill(WHITE))
            .draw(display)?;
        Rectangle::new(Point::new(20, 60), Size::new(112, 50))
            .into_styled(PrimitiveStyle::with_stroke(BLACK, 2))
            .draw(display)?;
        Text::with_alignment(
            "Game Over",
            Point::new(76, 78),
            MonoTextStyle::new(&FONT_6X10, BLACK),
            Alignment::Center,
        )
        .draw(display)?;
        buf.clear();
        let _ = core::fmt::Write::write_fmt(&mut buf, format_args!("Score: {}", score));
        Text::with_alignment(
            buf.as_str(),
            Point::new(76, 92),
            MonoTextStyle::new(&FONT_6X10, BLACK),
            Alignment::Center,
        )
        .draw(display)?;
        Text::with_alignment(
            "Press Fire",
            Point::new(76, 104),
            MonoTextStyle::new(&FONT_6X10, BLACK),
            Alignment::Center,
        )
        .draw(display)?;
    }

    Ok(())
}
