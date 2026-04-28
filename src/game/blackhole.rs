//! Black Hole mini-game.
//!
//! Six-row triangular pyramid (21 circles).  Human (white tokens) and
//! AI (black tokens) alternate placing the numbers 1..10 in their own
//! colour, in order — both place "1" first, then both "2", and so on.
//! After 20 placements one cell remains empty: the black hole.  Each
//! player's score is the sum of their numbers in cells adjacent to the
//! hole.  Lowest score wins.
//!
//! Player A (human): white circle, black number.
//! Player B (AI):    black circle, white number.
//!
//! State is held in module-level atomics — no heap, no alloc.

use core::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, Ordering};

use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::mono_font::ascii::FONT_6X10;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{Circle, PrimitiveStyle, Rectangle};
use embedded_graphics::text::{Alignment, Baseline, Text, TextStyleBuilder};

use crate::ui;
use crate::{BLACK, TriColor, WHITE};

// ── Layout ────────────────────────────────────────────────────────────────────

const ROWS: u8 = 6;
const CELLS: usize = 21; // 1 + 2 + 3 + 4 + 5 + 6

/// Centre of the top circle.
const TOP_CX: i32 = 76;
const TOP_CY: i32 = 12;
/// Horizontal pitch between circles in a row.
const COL_PITCH: i32 = 22;
/// Vertical pitch between rows.
const ROW_PITCH: i32 = 19;
/// Cell radius.
const CELL_R: i32 = 9;

/// High bit of [`BOARD`] entries marks the AI's tokens.
const PLAYER_B_BIT: u8 = 0x80;

// ── State ─────────────────────────────────────────────────────────────────────

static ACTIVE: AtomicBool = AtomicBool::new(false);
/// Cursor position (0..21).
static CURSOR: AtomicU8 = AtomicU8::new(15); // bottom-left start
/// Move counter (0..20).  Number placed = move/2 + 1.  Even = human, odd = AI.
static MOVE_NUM: AtomicU8 = AtomicU8::new(0);
/// 0 = in progress; 1 = human wins; 2 = AI wins; 3 = tie.
static RESULT: AtomicU8 = AtomicU8::new(0);
/// Index of the empty cell that became the black hole (valid when RESULT != 0).
static BLACK_HOLE: AtomicU8 = AtomicU8::new(0);
static SCORE_A: AtomicU8 = AtomicU8::new(0);
static SCORE_B: AtomicU8 = AtomicU8::new(0);
/// Board cells.  0 = empty; 1..10 = A's number; 0x81..0x8A = B's number.
static BOARD: [AtomicU8; CELLS] = [const { AtomicU8::new(0) }; CELLS];
/// PRNG state for AI tie-breaking.
static RNG: AtomicU32 = AtomicU32::new(0xDEAD_BEEF);

/// Difficulty: 0 = Easy (positional only), 1 = Hard (full heuristic).
const DIFFICULTY_HARD: u8 = 1;
static DIFFICULTY: AtomicU8 = AtomicU8::new(DIFFICULTY_HARD);
/// Difficulty-picker overlay open at the start of each game.
static MENU_OPEN: AtomicBool = AtomicBool::new(false);
/// Difficulty-picker selection (0 = Easy, 1 = Hard).
static MENU_POS: AtomicU8 = AtomicU8::new(DIFFICULTY_HARD);

// ── Public API ────────────────────────────────────────────────────────────────

pub fn is_active() -> bool {
    ACTIVE.load(Ordering::Relaxed)
}

pub fn open() {
    for cell in &BOARD {
        cell.store(0, Ordering::Relaxed);
    }
    CURSOR.store(15, Ordering::Relaxed); // bottom-left
    MOVE_NUM.store(0, Ordering::Relaxed);
    RESULT.store(0, Ordering::Relaxed);
    BLACK_HOLE.store(0, Ordering::Relaxed);
    SCORE_A.store(0, Ordering::Relaxed);
    SCORE_B.store(0, Ordering::Relaxed);
    RNG.store(seed(), Ordering::Relaxed);
    // Show the difficulty picker first; play starts after Fire.
    MENU_POS.store(DIFFICULTY_HARD, Ordering::Relaxed);
    MENU_OPEN.store(true, Ordering::Relaxed);
    ACTIVE.store(true, Ordering::Relaxed);
}

pub fn close() {
    ACTIVE.store(false, Ordering::Relaxed);
    // Clear ghosting from the many fast LUT refreshes during play.
    crate::FULL_REFRESH_PENDING.store(true, Ordering::Relaxed);
}

// ── Index helpers ─────────────────────────────────────────────────────────────

const ROW_START: [u8; ROWS as usize + 1] = [0, 1, 3, 6, 10, 15, 21];

fn row_of(idx: u8) -> u8 {
    let mut r = 0u8;
    while r + 1 <= ROWS && idx >= ROW_START[(r + 1) as usize] {
        r += 1;
    }
    r
}

fn col_of(idx: u8) -> u8 {
    idx - ROW_START[row_of(idx) as usize]
}

fn idx_of(r: u8, c: u8) -> u8 {
    ROW_START[r as usize] + c
}

fn cell_centre(idx: u8) -> Point {
    let r = row_of(idx) as i32;
    let c = col_of(idx) as i32;
    let x = TOP_CX + (2 * c - r) * (COL_PITCH / 2);
    let y = TOP_CY + r * ROW_PITCH;
    Point::new(x, y)
}

// ── Cursor navigation ─────────────────────────────────────────────────────────

pub fn cursor_left() {
    if MENU_OPEN.load(Ordering::Relaxed) {
        return;
    }
    let i = CURSOR.load(Ordering::Relaxed);
    let c = col_of(i);
    if c > 0 {
        CURSOR.store(i - 1, Ordering::Relaxed);
    }
}

pub fn cursor_right() {
    if MENU_OPEN.load(Ordering::Relaxed) {
        return;
    }
    let i = CURSOR.load(Ordering::Relaxed);
    let r = row_of(i);
    let c = col_of(i);
    if c < r {
        CURSOR.store(i + 1, Ordering::Relaxed);
    }
}

pub fn cursor_up() {
    if MENU_OPEN.load(Ordering::Relaxed) {
        let p = MENU_POS.load(Ordering::Relaxed);
        if p > 0 {
            MENU_POS.store(p - 1, Ordering::Relaxed);
        }
        return;
    }
    let i = CURSOR.load(Ordering::Relaxed);
    let r = row_of(i);
    if r == 0 {
        return;
    }
    let c = col_of(i);
    let new_r = r - 1;
    // Row new_r has cols 0..=new_r.  Clamp current column right-edge
    // when the row above is narrower.
    let new_c = c.min(new_r);
    CURSOR.store(idx_of(new_r, new_c), Ordering::Relaxed);
}

pub fn cursor_down() {
    if MENU_OPEN.load(Ordering::Relaxed) {
        let p = MENU_POS.load(Ordering::Relaxed);
        if p < 1 {
            MENU_POS.store(p + 1, Ordering::Relaxed);
        }
        return;
    }
    let i = CURSOR.load(Ordering::Relaxed);
    let r = row_of(i);
    if r >= ROWS - 1 {
        return;
    }
    let c = col_of(i);
    CURSOR.store(idx_of(r + 1, c), Ordering::Relaxed);
}

// ── Activate (Fire) ───────────────────────────────────────────────────────────

pub fn activate() {
    // Difficulty picker open: confirm selection and start the game.
    if MENU_OPEN.load(Ordering::Relaxed) {
        DIFFICULTY.store(MENU_POS.load(Ordering::Relaxed), Ordering::Relaxed);
        MENU_OPEN.store(false, Ordering::Relaxed);
        return;
    }

    let result = RESULT.load(Ordering::Relaxed);
    if result != 0 {
        // Game over: any Fire closes.  Award inspiration on win or tie.
        if result == 1 || result == 3 {
            super::lifecycle::award_inspiration();
            super::show_toast(super::Toast::Inspired);
        }
        close();
        return;
    }

    let m = MOVE_NUM.load(Ordering::Relaxed);
    if m & 1 != 0 {
        // AI's turn — should not happen because activate runs the AI
        // synchronously after the human move.
        return;
    }

    let pos = CURSOR.load(Ordering::Relaxed) as usize;
    if pos >= CELLS || BOARD[pos].load(Ordering::Relaxed) != 0 {
        return; // can't place on a filled cell
    }

    // Human places number m/2 + 1 (1..10).
    let n = m / 2 + 1;
    BOARD[pos].store(n, Ordering::Relaxed);
    let m = m + 1;
    MOVE_NUM.store(m, Ordering::Relaxed);

    if m >= 20 {
        finish_game();
        return;
    }

    // AI moves immediately (move m is odd, B's turn).
    ai_play(m);
    let m = m + 1;
    MOVE_NUM.store(m, Ordering::Relaxed);

    if m >= 20 {
        finish_game();
    }
}

// ── End-of-game scoring ──────────────────────────────────────────────────────

fn finish_game() {
    // The single remaining empty cell is the black hole.
    let mut hole = 0u8;
    for (i, cell) in BOARD.iter().enumerate() {
        if cell.load(Ordering::Relaxed) == 0 {
            hole = i as u8;
            break;
        }
    }
    BLACK_HOLE.store(hole, Ordering::Relaxed);

    let board = read_board();
    let (sa, sb) = scores_around(&board, hole);
    SCORE_A.store(sa, Ordering::Relaxed);
    SCORE_B.store(sb, Ordering::Relaxed);

    let result = if sa < sb {
        1
    } else if sb < sa {
        2
    } else {
        3
    };
    RESULT.store(result, Ordering::Relaxed);
}

fn read_board() -> [u8; CELLS] {
    let mut b = [0u8; CELLS];
    for (i, c) in BOARD.iter().enumerate() {
        b[i] = c.load(Ordering::Relaxed);
    }
    b
}

// ── Adjacency ─────────────────────────────────────────────────────────────────

/// Fill `out` with neighbour indices of `idx` and return the count.
fn neighbours(idx: u8, out: &mut [u8; 6]) -> usize {
    let r = row_of(idx);
    let c = col_of(idx);
    let mut n = 0;

    // Same row.
    if c > 0 {
        out[n] = idx_of(r, c - 1);
        n += 1;
    }
    if c < r {
        out[n] = idx_of(r, c + 1);
        n += 1;
    }
    // Row above (cols 0..=r-1).
    if r > 0 {
        if c > 0 {
            out[n] = idx_of(r - 1, c - 1);
            n += 1;
        }
        if c < r {
            out[n] = idx_of(r - 1, c);
            n += 1;
        }
    }
    // Row below (cols 0..=r+1).
    if r < ROWS - 1 {
        out[n] = idx_of(r + 1, c);
        n += 1;
        out[n] = idx_of(r + 1, c + 1);
        n += 1;
    }

    n
}

fn cell_value(b: u8) -> u8 {
    b & !PLAYER_B_BIT
}

fn cell_is_a(b: u8) -> bool {
    b != 0 && (b & PLAYER_B_BIT) == 0
}

fn cell_is_b(b: u8) -> bool {
    (b & PLAYER_B_BIT) != 0
}

/// Sum each player's numbers adjacent to `hole`.
fn scores_around(board: &[u8; CELLS], hole: u8) -> (u8, u8) {
    let mut nb = [0u8; 6];
    let n = neighbours(hole, &mut nb);
    let mut a = 0u8;
    let mut b = 0u8;
    for &i in &nb[..n] {
        let v = board[i as usize];
        if cell_is_a(v) {
            a = a.saturating_add(cell_value(v));
        } else if cell_is_b(v) {
            b = b.saturating_add(cell_value(v));
        }
    }
    (a, b)
}

// ── AI ────────────────────────────────────────────────────────────────────────

/// Threshold dividing "low" numbers (≤) from "high" numbers (>).  Low
/// numbers prefer well-connected cells (their small contribution to a
/// hole's score is cheap to risk and uses up central spots); high
/// numbers prefer corners/edges so they're less likely to neighbour
/// the eventual hole.
const CONNECTIVITY_THRESHOLD: u8 = 5;
/// Per-neighbour weight added (low n) or subtracted (high n) from a
/// candidate's main score.  Tuned to be comparable in magnitude to
/// the [-1000, 1000] range of the normalised positional score so it
/// influences ties and close calls but doesn't override clear wins.
const CONNECTIVITY_BONUS: i32 = 60;
/// Weight on the per-candidate territory delta (player adj sum minus
/// AI adj sum, scaled by the AI's current number).  Pulls the AI's
/// big tokens toward cells where the player has concentrated tokens
/// — those cells have fewer empty neighbours that could become the
/// black hole, so a big AI number is safe there.
const TERRITORY_BONUS: i32 = 2;

fn xorshift(mut x: u32) -> u32 {
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    x
}

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

/// AI places number `m/2 + 1` on the board.  `m` must be odd (B's turn).
///
/// Heuristic: simulate placing B's number on each empty candidate cell
/// and average (A_adj − B_adj) over every other empty cell as the
/// hypothetical black hole.  Pick the candidate with the highest
/// average — high A score and low B score around the eventual hole are
/// what win for B.  Ties broken randomly.
fn ai_play(m: u8) {
    let n = m / 2 + 1;
    let board = read_board();
    let hard = DIFFICULTY.load(Ordering::Relaxed) == DIFFICULTY_HARD;

    let mut best_score: i32 = i32::MIN;
    let mut best_cells = [0u8; CELLS];
    let mut best_count: usize = 0;

    for cand in 0..CELLS as u8 {
        if board[cand as usize] != 0 {
            continue;
        }
        let mut b = board;
        b[cand as usize] = PLAYER_B_BIT | n;

        let mut score: i32 = 0;
        let mut weight: i32 = 0;
        for hole in 0..CELLS as u8 {
            if b[hole as usize] != 0 {
                continue;
            }
            let (a_sum, b_sum) = scores_around(&b, hole);
            score += a_sum as i32 - b_sum as i32;
            weight += 1;
        }
        // Normalise so candidates aren't biased by the count of
        // remaining empty cells (it's the same for all candidates this
        // turn, but stay defensive).
        let norm = if weight > 0 {
            score * 1000 / weight
        } else {
            0
        };

        // Connectivity + territory bonuses only apply on Hard.  Easy
        // mode uses the positional score only — beatable with
        // straightforward play.
        let total = if hard {
            let mut nb_buf = [0u8; 6];
            let nb_count = neighbours(cand, &mut nb_buf);
            let connectivity = if n <= CONNECTIVITY_THRESHOLD {
                CONNECTIVITY_BONUS * nb_count as i32
            } else {
                -CONNECTIVITY_BONUS * nb_count as i32
            };

            let mut a_adj: i32 = 0;
            let mut b_adj: i32 = 0;
            for &i in &nb_buf[..nb_count] {
                let v = board[i as usize];
                if cell_is_a(v) {
                    a_adj += cell_value(v) as i32;
                } else if cell_is_b(v) {
                    b_adj += cell_value(v) as i32;
                }
            }
            let territory = (a_adj - b_adj) * n as i32 * TERRITORY_BONUS;

            norm + connectivity + territory
        } else {
            norm
        };

        if total > best_score {
            best_score = total;
            best_count = 0;
            best_cells[best_count] = cand;
            best_count += 1;
        } else if total == best_score && best_count < best_cells.len() {
            best_cells[best_count] = cand;
            best_count += 1;
        }
    }

    let mut rng = RNG.load(Ordering::Relaxed);
    rng = xorshift(rng | 1);
    RNG.store(rng, Ordering::Relaxed);
    let pick = if best_count > 0 {
        best_cells[(rng as usize) % best_count] as usize
    } else {
        // Fallback: any empty cell (shouldn't happen as long as MOVE_NUM < 20).
        board.iter().position(|&c| c == 0).unwrap_or(0)
    };

    BOARD[pick].store(PLAYER_B_BIT | n, Ordering::Relaxed);
}

// ── Drawing ───────────────────────────────────────────────────────────────────

pub fn draw<D>(display: &mut D) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let board = read_board();
    let cursor = CURSOR.load(Ordering::Relaxed);
    let result = RESULT.load(Ordering::Relaxed);
    let move_num = MOVE_NUM.load(Ordering::Relaxed);
    let hole = BLACK_HOLE.load(Ordering::Relaxed);

    // Background.
    Rectangle::new(Point::zero(), Size::new(152, 152))
        .into_styled(PrimitiveStyle::with_fill(WHITE))
        .draw(display)?;

    let dia = (CELL_R * 2 + 1) as u32;

    // Cells.
    for i in 0..CELLS as u8 {
        let p = cell_centre(i);
        let tl = Point::new(p.x - CELL_R, p.y - CELL_R);
        let cell = board[i as usize];
        let is_b = cell_is_b(cell);

        // Fill: B = black, A or empty = white.
        Circle::new(tl, dia)
            .into_styled(PrimitiveStyle::with_fill(if is_b { BLACK } else { WHITE }))
            .draw(display)?;
        // Outline.
        Circle::new(tl, dia)
            .into_styled(PrimitiveStyle::with_stroke(BLACK, 1))
            .draw(display)?;

        // Black-hole marker after the game ends.
        if result != 0 && i == hole {
            // Smaller filled black disc inside the empty cell.
            let inner_r = CELL_R - 3;
            Circle::new(
                Point::new(p.x - inner_r, p.y - inner_r),
                (inner_r * 2 + 1) as u32,
            )
            .into_styled(PrimitiveStyle::with_fill(BLACK))
            .draw(display)?;
        }

        // Number.
        let v = cell_value(cell);
        if v > 0 {
            let mut buf: heapless::String<3> = heapless::String::new();
            let _ = core::fmt::Write::write_fmt(&mut buf, format_args!("{}", v));
            let color = if is_b { WHITE } else { BLACK };
            // FONT_6X10 baseline middle aligns cap-line centre — nudge
            // down 1 px so the digit sits visually centred.
            let np = Point::new(p.x, p.y + 1);
            draw_bold_number(display, buf.as_str(), np, color)?;
        }

        // Cursor: dithered ring on the active cell during the human's turn.
        if i == cursor && result == 0 && (move_num & 1) == 0 {
            draw_cursor_ring(display, p)?;
        }
    }

    // Status line(s).
    let centred_top = TextStyleBuilder::new()
        .baseline(Baseline::Top)
        .alignment(Alignment::Center)
        .build();
    let footer_font = MonoTextStyle::new(&FONT_6X10, BLACK);

    let sa = SCORE_A.load(Ordering::Relaxed);
    let sb = SCORE_B.load(Ordering::Relaxed);
    match result {
        0 => {
            let n = move_num / 2 + 1;
            let mut s: heapless::String<24> = heapless::String::new();
            let _ = core::fmt::Write::write_fmt(&mut s, format_args!("Place {}", n));
            Text::with_text_style(s.as_str(), Point::new(76, 124), footer_font, centred_top)
                .draw(display)?;
            Text::with_text_style(
                "You = white",
                Point::new(76, 138),
                footer_font,
                centred_top,
            )
            .draw(display)?;
        }
        1 => {
            let mut s: heapless::String<24> = heapless::String::new();
            let _ = core::fmt::Write::write_fmt(&mut s, format_args!("You {} - {} EI", sa, sb));
            Text::with_text_style(s.as_str(), Point::new(76, 124), footer_font, centred_top)
                .draw(display)?;
            Text::with_text_style(
                "You win! +inspired",
                Point::new(76, 138),
                footer_font,
                centred_top,
            )
            .draw(display)?;
        }
        2 => {
            let mut s: heapless::String<24> = heapless::String::new();
            let _ = core::fmt::Write::write_fmt(&mut s, format_args!("You {} - {} EI", sa, sb));
            Text::with_text_style(s.as_str(), Point::new(76, 124), footer_font, centred_top)
                .draw(display)?;
            Text::with_text_style("EI wins", Point::new(76, 138), footer_font, centred_top)
                .draw(display)?;
        }
        _ => {
            let mut s: heapless::String<24> = heapless::String::new();
            let _ = core::fmt::Write::write_fmt(&mut s, format_args!("Tie {} - {}", sa, sb));
            Text::with_text_style(s.as_str(), Point::new(76, 124), footer_font, centred_top)
                .draw(display)?;
            Text::with_text_style(
                "+inspired",
                Point::new(76, 138),
                footer_font,
                centred_top,
            )
            .draw(display)?;
        }
    }

    if MENU_OPEN.load(Ordering::Relaxed) {
        draw_difficulty_menu(display)?;
    }

    Ok(())
}

/// Difficulty picker overlay — popover with two items, drawn over the
/// playing field at the start of every game.  Reuses the popover
/// frame, title bar, and inverted-row item style from [`crate::ui`]
/// and [`super::modal`] for visual consistency.
fn draw_difficulty_menu<D>(display: &mut D) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    const MARGIN: i32 = 16;
    const W: u32 = 120; // 152 - 2 × 16
    const H: u32 = 80;
    const BORDER: u32 = 2;
    const ITEM_H: i32 = 18;

    ui::draw_popover_frame(
        display,
        Point::new(MARGIN, 36),
        Size::new(W, H),
        BORDER,
    )?;
    let inner_x = MARGIN + BORDER as i32;
    let inner_y = 36 + BORDER as i32;
    let inner_w = W - BORDER * 2;
    ui::draw_title_bar(display, "Difficulty", Point::new(inner_x, inner_y), inner_w)?;

    let items = ["Easy", "Hard"];
    let pos = MENU_POS.load(Ordering::Relaxed) as usize;
    let list_y = inner_y + ui::TITLE_BAR_H as i32 + 4;
    let left_style = TextStyleBuilder::new()
        .baseline(Baseline::Middle)
        .alignment(Alignment::Left)
        .build();

    for (i, label) in items.iter().enumerate() {
        let row_top = list_y + i as i32 * ITEM_H;
        let row_mid = row_top + ITEM_H / 2;
        if i == pos {
            Rectangle::new(Point::new(inner_x, row_top), Size::new(inner_w, ITEM_H as u32))
                .into_styled(PrimitiveStyle::with_fill(BLACK))
                .draw(display)?;
            Text::with_text_style(
                label,
                Point::new(inner_x + 6, row_mid),
                ui::TEXT_WHITE,
                left_style,
            )
            .draw(display)?;
        } else {
            Text::with_text_style(
                label,
                Point::new(inner_x + 6, row_mid),
                ui::TEXT_BLACK,
                left_style,
            )
            .draw(display)?;
        }
    }
    Ok(())
}

/// Dithered B/W ring around a cell to mark the cursor.  Pure B/W so it
/// survives the fast LUT refresh.  4 px wide, straddling the cell
/// outline so it stays visible without overdrawing the centred digit.
fn draw_cursor_ring<D>(display: &mut D, p: Point) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let outer = CELL_R + 3;
    let inner = CELL_R - 1;
    let outer2 = outer * outer;
    let inner2 = inner * inner;
    let pixels = (-outer..=outer).flat_map(move |dy| {
        (-outer..=outer).filter_map(move |dx| {
            let d2 = dx * dx + dy * dy;
            if d2 > outer2 || d2 < inner2 {
                return None;
            }
            let px = p.x + dx;
            let py = p.y + dy;
            let color = if (px + py) & 1 == 0 { BLACK } else { WHITE };
            Some(Pixel(Point::new(px, py), color))
        })
    });
    display.draw_iter(pixels)?;
    Ok(())
}

/// Render a number with a fake-bold effect (FONT_6X10 has no bold
/// variant in `mono_font::ascii`).  Draws the digit twice, second pass
/// shifted 1 px right, so vertical strokes thicken — easier to read on
/// a low-contrast EPD.
fn draw_bold_number<D>(
    display: &mut D,
    s: &str,
    p: Point,
    color: TriColor,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let style = MonoTextStyle::new(&FONT_6X10, color);
    let centred = TextStyleBuilder::new()
        .baseline(Baseline::Middle)
        .alignment(Alignment::Center)
        .build();
    Text::with_text_style(s, p, style, centred).draw(display)?;
    Text::with_text_style(s, Point::new(p.x + 1, p.y), style, centred).draw(display)?;
    Ok(())
}
