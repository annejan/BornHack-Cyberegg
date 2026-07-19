//! Nim mini-game (misère, 4 rows of [1, 3, 5, 7] sticks).
//!
//! Player and EI alternate removing ≥1 stick from one row per turn.
//! Whoever is forced to take the last stick **loses**.
//!
//! Input is two-phase:
//!   1. Row select — Up/Down moves a dithered cursor between rows; Fire commits
//!      the row.
//!   2. Count select — Left grows the take-count (rightmost sticks turn
//!      dithered grey to mark them for removal); Right shrinks it; Fire commits
//!      the move.
//!
//! Difficulty:
//!   - **Normal**: 35 % chance per EI move of playing a random legal move
//!     instead of the optimal Nim strategy.
//!   - **Hard**: always optimal — XOR-strategy for standard Nim, switching to
//!     "leave an odd count of 1-piles" once every pile has ≤1 stick (misère
//!     endgame rule).
//!
//! State held in module-level atomics — no heap, no alloc.

use core::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::mono_font::ascii::FONT_6X10;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{PrimitiveStyle, Rectangle};
use embedded_graphics::text::{Alignment, Baseline, Text, TextStyleBuilder};

use crate::{BLACK, TriColor, WHITE, ui};

// ── Layout ────────────────────────────────────────────────────────────────────

const ROW_COUNT: usize = 4;
const INITIAL_ROWS: [u8; ROW_COUNT] = [1, 3, 5, 7];
const STICK_W: i32 = 6;
const STICK_H: i32 = 18;
const STICK_PITCH: i32 = 18;
const ROW_PITCH: i32 = 26;
const TOP_Y: i32 = 12;
const SCREEN_W: i32 = 152;

// ── Game state (atomics)
// ──────────────────────────────────────────────────────

const PHASE_ROW_SELECT: u8 = 0;
const PHASE_COUNT_SELECT: u8 = 1;
const PHASE_GAME_OVER: u8 = 2;

const DIFFICULTY_HARD: u8 = 1;
const NORMAL_MISTAKE_PCT: u32 = 35;
const MENU_HARD: u8 = 1;

const RESULT_IN_PROGRESS: u8 = 0;
const RESULT_HUMAN_WINS: u8 = 1;
const RESULT_EI_WINS: u8 = 2;

static ACTIVE: AtomicBool = AtomicBool::new(false);
static ROWS: [AtomicU8; ROW_COUNT] = [const { AtomicU8::new(0) }; ROW_COUNT];
static PHASE: AtomicU8 = AtomicU8::new(PHASE_ROW_SELECT);
static CURSOR_ROW: AtomicU8 = AtomicU8::new(ROW_COUNT as u8 - 1);
static TAKE_COUNT: AtomicU8 = AtomicU8::new(1);
static HUMAN_TURN: AtomicBool = AtomicBool::new(true);
static RESULT: AtomicU8 = AtomicU8::new(RESULT_IN_PROGRESS);

static DIFFICULTY: AtomicU8 = AtomicU8::new(DIFFICULTY_HARD);
static MENU_OPEN: AtomicBool = AtomicBool::new(false);
static MENU_POS: AtomicU8 = AtomicU8::new(MENU_HARD);

// ── Public API
// ────────────────────────────────────────────────────────────────

pub fn is_active() -> bool {
    ACTIVE.load(Ordering::Relaxed)
}

pub fn open() {
    for (i, &n) in INITIAL_ROWS.iter().enumerate() {
        ROWS[i].store(n, Ordering::Relaxed);
    }
    PHASE.store(PHASE_ROW_SELECT, Ordering::Relaxed);
    CURSOR_ROW.store(ROW_COUNT as u8 - 1, Ordering::Relaxed);
    TAKE_COUNT.store(1, Ordering::Relaxed);
    RESULT.store(RESULT_IN_PROGRESS, Ordering::Relaxed);
    // Coin flip for first turn — EI may strike first.
    HUMAN_TURN.store(entropy_byte() & 1 == 0, Ordering::Relaxed);
    MENU_POS.store(MENU_HARD, Ordering::Relaxed);
    MENU_OPEN.store(true, Ordering::Relaxed);
    ACTIVE.store(true, Ordering::Relaxed);
}

pub fn close() {
    ACTIVE.store(false, Ordering::Relaxed);
    crate::FULL_REFRESH_PENDING.store(true, Ordering::Relaxed);
}

// ── Input ─────────────────────────────────────────────────────────────────────

pub fn cursor_up() {
    if MENU_OPEN.load(Ordering::Relaxed) {
        let p = MENU_POS.load(Ordering::Relaxed);
        if p > 0 {
            MENU_POS.store(p - 1, Ordering::Relaxed);
        }
        return;
    }
    if PHASE.load(Ordering::Relaxed) != PHASE_ROW_SELECT || !HUMAN_TURN.load(Ordering::Relaxed) {
        return;
    }
    let r = CURSOR_ROW.load(Ordering::Relaxed);
    // Skip empty rows.
    let mut nr = r;
    while nr > 0 {
        nr -= 1;
        if ROWS[nr as usize].load(Ordering::Relaxed) > 0 {
            CURSOR_ROW.store(nr, Ordering::Relaxed);
            return;
        }
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
    if PHASE.load(Ordering::Relaxed) != PHASE_ROW_SELECT || !HUMAN_TURN.load(Ordering::Relaxed) {
        return;
    }
    let r = CURSOR_ROW.load(Ordering::Relaxed);
    let mut nr = r;
    while (nr as usize) + 1 < ROW_COUNT {
        nr += 1;
        if ROWS[nr as usize].load(Ordering::Relaxed) > 0 {
            CURSOR_ROW.store(nr, Ordering::Relaxed);
            return;
        }
    }
}

pub fn cursor_left() {
    if MENU_OPEN.load(Ordering::Relaxed) {
        return;
    }
    if PHASE.load(Ordering::Relaxed) != PHASE_COUNT_SELECT {
        return;
    }
    let row = CURSOR_ROW.load(Ordering::Relaxed) as usize;
    let max = ROWS[row].load(Ordering::Relaxed);
    let n = TAKE_COUNT.load(Ordering::Relaxed);
    if n < max {
        TAKE_COUNT.store(n + 1, Ordering::Relaxed);
    }
}

pub fn cursor_right() {
    if MENU_OPEN.load(Ordering::Relaxed) {
        return;
    }
    if PHASE.load(Ordering::Relaxed) != PHASE_COUNT_SELECT {
        return;
    }
    let n = TAKE_COUNT.load(Ordering::Relaxed);
    if n > 1 {
        TAKE_COUNT.store(n - 1, Ordering::Relaxed);
    }
}

pub fn activate() {
    // Difficulty picker: confirm and start.
    if MENU_OPEN.load(Ordering::Relaxed) {
        DIFFICULTY.store(MENU_POS.load(Ordering::Relaxed), Ordering::Relaxed);
        MENU_OPEN.store(false, Ordering::Relaxed);
        // If the EI was randomly chosen to start, play its first move now.
        if !HUMAN_TURN.load(Ordering::Relaxed) {
            ei_turn();
        }
        return;
    }

    // Game over: any Fire closes (and awards inspiration on a win).
    let r = RESULT.load(Ordering::Relaxed);
    if r != RESULT_IN_PROGRESS {
        if r == RESULT_HUMAN_WINS {
            super::lifecycle::award_inspiration(super::engine::MiniGame::Nim);
            super::show_toast(super::Toast::MinigameWin);
        }
        close();
        return;
    }

    if !HUMAN_TURN.load(Ordering::Relaxed) {
        return;
    }

    let phase = PHASE.load(Ordering::Relaxed);
    if phase == PHASE_ROW_SELECT {
        // Confirm row → enter count phase with 1 stick selected.
        let row = CURSOR_ROW.load(Ordering::Relaxed) as usize;
        if ROWS[row].load(Ordering::Relaxed) == 0 {
            return;
        }
        TAKE_COUNT.store(1, Ordering::Relaxed);
        PHASE.store(PHASE_COUNT_SELECT, Ordering::Relaxed);
    } else if phase == PHASE_COUNT_SELECT {
        // Commit the move.
        let row = CURSOR_ROW.load(Ordering::Relaxed) as usize;
        let n = TAKE_COUNT.load(Ordering::Relaxed);
        let cur = ROWS[row].load(Ordering::Relaxed);
        if n == 0 || n > cur {
            return;
        }
        ROWS[row].store(cur - n, Ordering::Relaxed);
        if all_empty() {
            // Human just took the last stick — human loses.
            RESULT.store(RESULT_EI_WINS, Ordering::Relaxed);
            PHASE.store(PHASE_GAME_OVER, Ordering::Relaxed);
            return;
        }
        // Hand to EI.
        HUMAN_TURN.store(false, Ordering::Relaxed);
        PHASE.store(PHASE_ROW_SELECT, Ordering::Relaxed);
        ei_turn();
    }
}

// ── EI turn
// ───────────────────────────────────────────────────────────────────

fn ei_turn() {
    let mut piles = read_rows();
    let (row, take) = ei_move(&piles);
    piles[row] = piles[row].saturating_sub(take);
    ROWS[row].store(piles[row], Ordering::Relaxed);

    if all_empty() {
        // EI just took the last stick — EI loses.
        RESULT.store(RESULT_HUMAN_WINS, Ordering::Relaxed);
        PHASE.store(PHASE_GAME_OVER, Ordering::Relaxed);
        return;
    }
    HUMAN_TURN.store(true, Ordering::Relaxed);
    PHASE.store(PHASE_ROW_SELECT, Ordering::Relaxed);
    // Park the row cursor on the first non-empty row.
    let r = CURSOR_ROW.load(Ordering::Relaxed);
    if ROWS[r as usize].load(Ordering::Relaxed) == 0 {
        for (i, p) in piles.iter().enumerate() {
            if *p > 0 {
                CURSOR_ROW.store(i as u8, Ordering::Relaxed);
                break;
            }
        }
    }
}

/// Pick (row, take_count) for the EI based on difficulty and pile state.
fn ei_move(piles: &[u8; ROW_COUNT]) -> (usize, u8) {
    let hard = DIFFICULTY.load(Ordering::Relaxed) == DIFFICULTY_HARD;

    if !hard && (entropy_byte() as u32 * 100 / 256) < NORMAL_MISTAKE_PCT {
        return random_move(piles);
    }

    misere_optimal(piles).unwrap_or_else(|| random_move(piles))
}

/// Optimal misère-Nim move.  Returns `None` only when no legal move
/// exists (all piles empty — game already over).
///
/// Strategy:
/// * If at least one pile has ≥2 sticks: play standard Nim — XOR all pile
///   sizes; if non-zero, reduce a pile so the XOR becomes 0. Special case: when
///   exactly one pile has ≥2 sticks, switch to the misère endgame plan and
///   leave an odd count of 1-piles for the opponent.
/// * If every pile has ≤1 stick: leave an odd count of 1-piles.
fn misere_optimal(piles: &[u8; ROW_COUNT]) -> Option<(usize, u8)> {
    let big_piles = piles.iter().filter(|&&p| p >= 2).count();
    let one_piles = piles.iter().filter(|&&p| p == 1).count();

    // Endgame: every pile is 0 or 1.
    if big_piles == 0 {
        if one_piles == 0 {
            return None;
        }
        // Leave opponent with an odd count of 1-piles → take 1 from
        // any pile when we have an even count (we go from even→odd).
        // If count is already odd, we're losing — take from any pile.
        let target = piles.iter().position(|&p| p == 1)?;
        return Some((target, 1));
    }

    // Transition zone: exactly one pile ≥ 2.  Misère trick: reduce
    // that pile to 0 or 1 so the remaining 1-piles total an odd count.
    if big_piles == 1 {
        let big = piles.iter().position(|&p| p >= 2)?;
        let other_ones = one_piles; // the big pile is not a 1-pile
        // After the move, we want (other_ones + leave) to be odd, where
        // `leave` ∈ {0, 1}.  Pick `leave` to satisfy that.
        let leave = if other_ones % 2 == 0 { 1 } else { 0 };
        let take = piles[big] - leave;
        return Some((big, take));
    }

    // Standard Nim: at least two piles ≥ 2.  XOR-strategy.
    let xor = piles.iter().fold(0u8, |a, &b| a ^ b);
    if xor != 0 {
        for (i, &p) in piles.iter().enumerate() {
            let target = p ^ xor;
            if target < p {
                return Some((i, p - target));
            }
        }
    }
    // XOR == 0 (or no winning reduction found): we're in a P-position,
    // any move loses with perfect opposing play.  Take the smallest
    // legal move from the largest pile.
    let (idx, _) = piles
        .iter()
        .enumerate()
        .filter(|&(_, &p)| p > 0)
        .max_by_key(|&(_, &p)| p)?;
    Some((idx, 1))
}

fn random_move(piles: &[u8; ROW_COUNT]) -> (usize, u8) {
    // Uniformly pick a non-empty row, then a random count 1..=row.
    let total: u32 = piles.iter().map(|&p| p as u32).sum();
    if total == 0 {
        return (0, 0);
    }
    let mut t = (entropy_byte() as u32) * total / 256;
    for (i, &p) in piles.iter().enumerate() {
        if t < p as u32 {
            let take = (entropy_byte() as u32 % p as u32) as u8 + 1;
            return (i, take);
        }
        t -= p as u32;
    }
    // Fallback: take 1 from the first non-empty row.
    for (i, &p) in piles.iter().enumerate() {
        if p > 0 {
            return (i, 1);
        }
    }
    (0, 0)
}

// ── Helpers
// ───────────────────────────────────────────────────────────────────

fn read_rows() -> [u8; ROW_COUNT] {
    let mut out = [0u8; ROW_COUNT];
    for (i, a) in ROWS.iter().enumerate() {
        out[i] = a.load(Ordering::Relaxed);
    }
    out
}

fn all_empty() -> bool {
    ROWS.iter().all(|a| a.load(Ordering::Relaxed) == 0)
}

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

// ── Drawing
// ───────────────────────────────────────────────────────────────────

pub fn draw<D>(display: &mut D) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let phase = PHASE.load(Ordering::Relaxed);
    let cursor = CURSOR_ROW.load(Ordering::Relaxed) as usize;
    let take = TAKE_COUNT.load(Ordering::Relaxed) as usize;
    let result = RESULT.load(Ordering::Relaxed);
    let human_turn = HUMAN_TURN.load(Ordering::Relaxed);
    let rows = read_rows();

    // Background.
    Rectangle::new(Point::zero(), Size::new(SCREEN_W as u32, 152))
        .into_styled(PrimitiveStyle::with_fill(WHITE))
        .draw(display)?;

    // Sticks for each row.
    for (r, &count) in rows.iter().enumerate() {
        let row_y = TOP_Y + r as i32 * ROW_PITCH;
        let initial = INITIAL_ROWS[r] as i32;
        let row_w = initial * STICK_PITCH - (STICK_PITCH - STICK_W);
        let x0 = (SCREEN_W - row_w) / 2;

        // Cursor highlight on the active row during human's row-select.
        let highlight =
            human_turn && result == RESULT_IN_PROGRESS && r == cursor && phase == PHASE_ROW_SELECT;
        if highlight {
            draw_row_highlight(display, x0 - 4, row_y - 3, row_w + 8, STICK_H + 6)?;
        }

        for c in 0..count as i32 {
            let x = x0 + c * STICK_PITCH;
            // Sticks selected for removal in count-phase: rightmost
            // `take` of the current row turn dithered.
            let to_remove = phase == PHASE_COUNT_SELECT
                && r == cursor
                && (c as usize) >= (count as usize - take);
            if to_remove {
                draw_dithered_stick(display, x, row_y)?;
            } else {
                Rectangle::new(
                    Point::new(x, row_y),
                    Size::new(STICK_W as u32, STICK_H as u32),
                )
                .into_styled(PrimitiveStyle::with_fill(BLACK))
                .draw(display)?;
            }
        }
    }

    // Status line.
    let centred = TextStyleBuilder::new()
        .baseline(Baseline::Top)
        .alignment(Alignment::Center)
        .build();
    let font = MonoTextStyle::new(&FONT_6X10, BLACK);
    let msg: &str = match result {
        RESULT_HUMAN_WINS => "You win!",
        RESULT_EI_WINS => "EI wins!",
        _ => match (human_turn, phase) {
            (true, PHASE_ROW_SELECT) => "Pick a row",
            (true, PHASE_COUNT_SELECT) => "Left = more, Fire = take",
            (true, _) => "Your turn",
            _ => "EI thinking...",
        },
    };
    Text::with_text_style(msg, Point::new(76, 124), font, centred).draw(display)?;
    if result == RESULT_IN_PROGRESS && phase == PHASE_COUNT_SELECT {
        let mut buf: heapless::String<16> = heapless::String::new();
        let _ = core::fmt::Write::write_fmt(&mut buf, format_args!("Take {}", take));
        Text::with_text_style(buf.as_str(), Point::new(76, 138), font, centred).draw(display)?;
    }

    if MENU_OPEN.load(Ordering::Relaxed) {
        ui::draw_picker_menu(
            display,
            "Difficulty",
            &["Normal", "Hard"],
            MENU_POS.load(Ordering::Relaxed) as usize,
        )?;
    }

    Ok(())
}

/// Dithered B/W rectangle ring around a row to mark the cursor.  Pure
/// B/W so it survives the fast LUT refresh.
fn draw_row_highlight<D>(display: &mut D, x: i32, y: i32, w: i32, h: i32) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    const THICK: i32 = 2;
    let pixels = (0..h).flat_map(move |dy| {
        (0..w).filter_map(move |dx| {
            let on_border = dx < THICK || dy < THICK || dx >= w - THICK || dy >= h - THICK;
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

/// Dithered (50 % checkerboard) stick — used to mark sticks selected
/// for removal in the count-select phase.
fn draw_dithered_stick<D>(display: &mut D, x: i32, y: i32) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let pixels = (0..STICK_H).flat_map(move |dy| {
        (0..STICK_W).map(move |dx| {
            let px = x + dx;
            let py = y + dy;
            let color = if (px + py) & 1 == 0 { BLACK } else { WHITE };
            Pixel(Point::new(px, py), color)
        })
    });
    display.draw_iter(pixels)
}
