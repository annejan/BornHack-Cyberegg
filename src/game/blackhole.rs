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
use embedded_graphics::primitives::{Circle, PrimitiveStyle};
use embedded_graphics::text::Text;

use super::minigame::DifficultyPicker;
use crate::menu::ButtonId;
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
/// Difficulty picker shown at the start of each game (0 = Easy, 1 = Hard).
static PICKER: DifficultyPicker = DifficultyPicker::new(DIFFICULTY_HARD);

// ── Public API
// ────────────────────────────────────────────────────────────────

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
    RNG.store(crate::platform::seed_ticks_or_fixed(), Ordering::Relaxed);
    // Show the difficulty picker first; play starts after Fire.
    PICKER.show();
    ACTIVE.store(true, Ordering::Relaxed);
}

pub fn close() {
    // A finished game pays by result, once, however the screen is left.
    if ACTIVE.swap(false, Ordering::Relaxed) {
        let grade = match RESULT.load(Ordering::Relaxed) {
            1 => Some(super::engine::GRADE_WIN),
            2 => Some(super::engine::GRADE_LOSS),
            3 => Some(super::engine::GRADE_DRAW),
            _ => None,
        };
        if let Some(grade) = grade {
            super::minigame::pay_result(super::engine::MiniGame::BlackHole, grade);
        }
    }
    // Clear ghosting from the many fast LUT refreshes during play.
    crate::FULL_REFRESH_PENDING.store(true, Ordering::Relaxed);
}

// ── Index helpers
// ─────────────────────────────────────────────────────────────

const ROW_START: [u8; ROWS as usize + 1] = [0, 1, 3, 6, 10, 15, 21];

fn row_of(idx: u8) -> u8 {
    let mut r = 0u8;
    while r < ROWS && idx >= ROW_START[(r + 1) as usize] {
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

// ── Cursor navigation
// ─────────────────────────────────────────────────────────

fn cursor_left() {
    if PICKER.is_open() {
        return;
    }
    let i = CURSOR.load(Ordering::Relaxed);
    let c = col_of(i);
    if c > 0 {
        CURSOR.store(i - 1, Ordering::Relaxed);
    }
}

fn cursor_right() {
    if PICKER.is_open() {
        return;
    }
    let i = CURSOR.load(Ordering::Relaxed);
    let r = row_of(i);
    let c = col_of(i);
    if c < r {
        CURSOR.store(i + 1, Ordering::Relaxed);
    }
}

fn cursor_up() {
    if PICKER.is_open() {
        PICKER.up();
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

fn cursor_down() {
    if PICKER.is_open() {
        PICKER.down(2);
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

// ── Activate (Fire)
// ───────────────────────────────────────────────────────────

fn activate() {
    // Difficulty picker open: confirm selection and start the game.
    if PICKER.is_open() {
        PICKER.confirm();
        return;
    }

    let result = RESULT.load(Ordering::Relaxed);
    if result != 0 {
        // Game over: any Fire closes (`close` pays the result).
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

// ── Adjacency
// ─────────────────────────────────────────────────────────────────

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
    let hard = PICKER.chosen() == DIFFICULTY_HARD;

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
        let norm = if weight > 0 { score * 1000 / weight } else { 0 };

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
    rng = crate::platform::xorshift32(rng | 1);
    RNG.store(rng, Ordering::Relaxed);
    let pick = if best_count > 0 {
        best_cells[(rng as usize) % best_count] as usize
    } else {
        // Fallback: any empty cell (shouldn't happen as long as MOVE_NUM < 20).
        board.iter().position(|&c| c == 0).unwrap_or(0)
    };

    BOARD[pick].store(PLAYER_B_BIT | n, Ordering::Relaxed);
}

// ── Drawing
// ───────────────────────────────────────────────────────────────────

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
    crate::ui::clear_screen(display)?;

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
    let centred_top = crate::ui::CENTERED_TOP;
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
            Text::with_text_style("You = white", Point::new(76, 138), footer_font, centred_top)
                .draw(display)?;
        }
        1 => {
            let mut s: heapless::String<24> = heapless::String::new();
            let _ = core::fmt::Write::write_fmt(&mut s, format_args!("You {} - {} EI", sa, sb));
            Text::with_text_style(s.as_str(), Point::new(76, 124), footer_font, centred_top)
                .draw(display)?;
            Text::with_text_style("You win!", Point::new(76, 138), footer_font, centred_top)
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
            Text::with_text_style("Tie game!", Point::new(76, 138), footer_font, centred_top)
                .draw(display)?;
        }
    }

    PICKER.draw(display, "Difficulty", &["Easy", "Hard"])?;

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

/// Render a number in FONT_6X10 with the shared fake-bold effect
/// ([`crate::ui::draw_text_fake_bold`]) — easier to read on a
/// low-contrast EPD.
fn draw_bold_number<D>(display: &mut D, s: &str, p: Point, color: TriColor) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let style = MonoTextStyle::new(&FONT_6X10, color);
    let centred = crate::ui::CENTERED;
    crate::ui::draw_text_fake_bold(display, s, p, style, centred)
}

#[cfg(test)]
mod pay_tests {
    use super::*;
    use crate::game::engine::{MiniGame, MINIGAME_HAX_DRAW, MINIGAME_HAX_LOSS, MINIGAME_HAX_REWARD};
    use crate::game::{current_toast, minigame_pay_read, minigame_pay_setup, Toast};

    const GAME: MiniGame = MiniGame::BlackHole;

    /// Open the game, put it in the finished state `result`, leave with Cancel.
    fn finish_and_cancel(result: u8) {
        open();

        RESULT.store(result, Ordering::Relaxed);
        dispatch(ButtonId::Cancel);
    }

    #[test]
    fn a_won_game_closed_with_cancel_pays_the_win() {
        let _g = minigame_pay_setup();
        finish_and_cancel(1);
        assert_eq!(minigame_pay_read(GAME), (MINIGAME_HAX_REWARD, true));
        assert_eq!(current_toast(), Some(Toast::MinigameWin));
        assert!(!is_active());
    }

    #[test]
    fn closing_twice_pays_once() {
        let _g = minigame_pay_setup();
        finish_and_cancel(1);
        close();
        close();
        assert_eq!(minigame_pay_read(GAME).0, MINIGAME_HAX_REWARD);
    }

    #[test]
    fn a_game_in_progress_pays_nothing() {
        let _g = minigame_pay_setup();
        finish_and_cancel(0);
        assert_eq!(minigame_pay_read(GAME), (0, false));
        assert_eq!(current_toast(), None);
    }

    #[test]
    fn a_lost_game_pays_the_loss() {
        let _g = minigame_pay_setup();
        finish_and_cancel(2);
        assert_eq!(minigame_pay_read(GAME), (MINIGAME_HAX_LOSS, true));
        assert_eq!(current_toast(), Some(Toast::MinigameTry));
    }

    #[test]
    fn a_drawn_game_pays_the_draw() {
        let _g = minigame_pay_setup();
        finish_and_cancel(3);
        assert_eq!(minigame_pay_read(GAME), (MINIGAME_HAX_DRAW, true));
        assert_eq!(current_toast(), Some(Toast::MinigameWin));
    }
}
