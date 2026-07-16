//! Triple Born — match-3-and-merge mini-game on a 6×6 grid.
//!
//! BornHack-themed re-skin of the classic Triple Town mechanic:
//! place tiles on the grid, three same-type tiles in a 4-connected
//! group merge into the next tier, cascading upwards through the
//! lineup.  Bears are replaced with cars on the terrain; trapped
//! cars become wrecks, which cascade through their own series.
//!
//! # Tile lineup (camp progression)
//!   1 Grass         2 Bush       3 Tree
//!   4 Small tent    5 Big tent   6 Pavilion
//!   7 Festival camp 8 BornHack   9 CyberAegg (top)
//!
//! # Vehicle series (created when cars get trapped)
//!   Wreck → Junkyard → Scrapheap (top)
//!
//! # Input
//!   Up/Down/Left/Right: move cursor.
//!   Fire/Execute:       place the previewed tile at the cursor.
//!   Cancel:             exit the game.
//!
//! State is held in module-level atomics — no heap, no alloc.

use core::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, Ordering};

use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::mono_font::ascii::FONT_6X13_BOLD;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{Circle, PrimitiveStyle, Rectangle};
use embedded_graphics::text::{Alignment, Baseline, Text, TextStyleBuilder};

use crate::{BLACK, TriColor, WHITE, ui};

// ── Layout ────────────────────────────────────────────────────────────────────

const BOARD_W: usize = 6;
const BOARD_H: usize = 6;
const CELLS: usize = BOARD_W * BOARD_H;
const CELL: i32 = 22;
const BOARD_X: i32 = 10;
/// Status bar height (next-tile preview + score).
const STATUS_H: i32 = 20;
const BOARD_Y: i32 = STATUS_H;
/// Tile-content half-extent for board cells.  At 7 the content disc
/// is 15 px across — fits inside the cell with margin to spare for
/// the L-shaped 1 px cell border.
const TILE_RADIUS: i32 = 7;
/// Tile-content half-extent in the status-bar previews (no border).
const TILE_RADIUS_SMALL: i32 = 6;

// ── Tile types
// ────────────────────────────────────────────────────────────────

const T_EMPTY: u8 = 0;
// Camp series — tile id == tier number, 1 (Grass) through 9 (CyberAegg).
// Intermediate tiers (Small tent, Big tent, Festival camp, BornHack
// flag) aren't named here because they're never referenced by name —
// the merge code uses `tile + 1` and the `T_GRASS..=T_AEGG` range
// pattern.  Their meanings are documented in the file header.
const T_GRASS: u8 = 1;
const T_BUSH: u8 = 2;
const T_TREE: u8 = 3;
const T_FLAG: u8 = 8;
const T_AEGG: u8 = 9;

// Vehicle series — id starts at 10 to keep camp tiers as tier numbers.
const T_CAR: u8 = 10;
const T_WRECK: u8 = 11;
const T_JUNKYARD: u8 = 12;
const T_SCRAPHEAP: u8 = 13;

/// Wildcard tile — when placed, becomes the highest-tier camp or
/// vehicle tile that gives a winning 3-merge against its 4-neighbours.
/// If no neighbouring match is possible the wildcard stays in place
/// and the player can plan around it later.
const T_WILDCARD: u8 = 14;

/// Maximum camp tier flood-fill / cascade ever needs to inspect — the
/// merge group can't span more cells than the board.
const MAX_GROUP: usize = CELLS;

// ── Game state
// ────────────────────────────────────────────────────────────────

const RESULT_IN_PROGRESS: u8 = 0;
const RESULT_GAME_OVER: u8 = 1;

static ACTIVE: AtomicBool = AtomicBool::new(false);
static BOARD: [AtomicU8; CELLS] = [const { AtomicU8::new(0) }; CELLS];
static CURSOR: AtomicU8 = AtomicU8::new(0);
static NEXT_TILE: AtomicU8 = AtomicU8::new(T_GRASS);
static STASH: AtomicU8 = AtomicU8::new(T_EMPTY);
static SCORE: AtomicU32 = AtomicU32::new(0);
static RESULT: AtomicU8 = AtomicU8::new(RESULT_IN_PROGRESS);
/// Per-game xorshift state.  Seeded from uptime in `open()` and
/// advanced deterministically thereafter — same seed plus the same
/// sequence of player moves produces the same outcome.  Triple Town
/// players exploit this to predict bear (here: car) movement.
static GAME_RNG: AtomicU32 = AtomicU32::new(0);

// ── Public API
// ────────────────────────────────────────────────────────────────

pub fn is_active() -> bool {
    ACTIVE.load(Ordering::Relaxed)
}

pub fn open() {
    for cell in &BOARD {
        cell.store(T_EMPTY, Ordering::Relaxed);
    }
    CURSOR.store((CELLS / 2) as u8, Ordering::Relaxed);
    STASH.store(T_EMPTY, Ordering::Relaxed);
    SCORE.store(0, Ordering::Relaxed);
    RESULT.store(RESULT_IN_PROGRESS, Ordering::Relaxed);
    // Seed once from uptime; xorshift evolves deterministically.
    // OR with 1 to avoid the all-zero degenerate state.
    GAME_RNG.store(rng_seed(), Ordering::Relaxed);
    seed_initial_board();
    NEXT_TILE.store(roll_next_tile(), Ordering::Relaxed);
    ACTIVE.store(true, Ordering::Relaxed);
    // Dense 6×6 grid with new tiles — full refresh clears any prior
    // ghosting from the screen we came from.
    crate::FULL_REFRESH_PENDING.store(true, Ordering::Relaxed);
}

/// Seed the empty board with a small starting layout: scattered
/// grass + a starter bush + one car as an initial obstacle.
/// Placements are rejected if they'd form an instant 3-merge — the
/// player should set those up, not the engine.
fn seed_initial_board() {
    /// Number of grass tiles to scatter at game start.
    const INITIAL_GRASS: usize = 5;
    /// One bush — gives the player a head start on building a tree.
    const INITIAL_BUSH: usize = 1;
    /// One car as a moving obstacle from the very first turn.
    const INITIAL_CARS: usize = 1;

    let mut to_place = [
        (T_GRASS, INITIAL_GRASS),
        (T_BUSH, INITIAL_BUSH),
        (T_CAR, INITIAL_CARS),
    ];
    for (tile, count) in to_place.iter_mut() {
        let mut placed = 0;
        // Bounded attempts so a pathological RNG run can't loop.
        let mut attempts = 0;
        while placed < *count && attempts < 64 {
            attempts += 1;
            let pos = (rng_next() as usize) % CELLS;
            if BOARD[pos].load(Ordering::Relaxed) != T_EMPTY {
                continue;
            }
            // Tentatively place; if a mergeable tile would form an
            // instant 3-cluster with what's already seeded, undo and
            // try elsewhere.  Cars can't merge so the check is moot
            // for them, but the same code path handles all cases.
            BOARD[pos].store(*tile, Ordering::Relaxed);
            if *tile != T_CAR {
                let mut group = [0u8; CELLS];
                let cluster = flood_same_type(pos, &mut group);
                if cluster >= 3 {
                    BOARD[pos].store(T_EMPTY, Ordering::Relaxed);
                    continue;
                }
            }
            placed += 1;
        }
    }
}

pub fn close() {
    // The score earned converts to a bonus inspiration on close,
    // even when the player Cancels out so leaving isn't punished.
    // Bonus curve:
    //   score < 100         → 0
    //   100 ≤ score < 1000  → flat 2000
    //   score ≥ 1000        → 2 × score, saturating at u16::MAX.
    let score = SCORE.load(Ordering::Relaxed);
    let bonus: u16 = if score < BONUS_MIN_SCORE {
        0
    } else if score < BONUS_DOUBLE_THRESHOLD {
        BONUS_FLAT
    } else {
        score.saturating_mul(2).min(u16::MAX as u32) as u16
    };
    if bonus > 0 {
        // award_inspiration sets cooldown + base reward + hunger;
        // add_drained_relief layers the score-derived bonus on top.
        super::lifecycle::award_inspiration(super::engine::MiniGame::TripleBorn);
        super::lifecycle::add_drained_relief(bonus);
        super::show_tripleborn_bonus(bonus);
    }
    ACTIVE.store(false, Ordering::Relaxed);
    crate::FULL_REFRESH_PENDING.store(true, Ordering::Relaxed);
}

/// Minimum score that pays a bonus on close.  Below this the player
/// barely played, so no inspiration is awarded.
const BONUS_MIN_SCORE: u32 = 100;
/// Score at which the flat-2000 bonus gives way to the 2×score curve.
const BONUS_DOUBLE_THRESHOLD: u32 = 1000;
/// Flat bonus paid when score is between [`BONUS_MIN_SCORE`] and
/// [`BONUS_DOUBLE_THRESHOLD`].
const BONUS_FLAT: u16 = 2000;

// ── Cursor input
// ──────────────────────────────────────────────────────────────

pub fn cursor_up() {
    let c = CURSOR.load(Ordering::Relaxed) as usize;
    if c >= BOARD_W {
        CURSOR.store((c - BOARD_W) as u8, Ordering::Relaxed);
    }
}

pub fn cursor_down() {
    let c = CURSOR.load(Ordering::Relaxed) as usize;
    if c + BOARD_W < CELLS {
        CURSOR.store((c + BOARD_W) as u8, Ordering::Relaxed);
    }
}

pub fn cursor_left() {
    let c = CURSOR.load(Ordering::Relaxed) as usize;
    if !c.is_multiple_of(BOARD_W) {
        CURSOR.store((c - 1) as u8, Ordering::Relaxed);
    }
}

pub fn cursor_right() {
    let c = CURSOR.load(Ordering::Relaxed) as usize;
    if c % BOARD_W + 1 < BOARD_W {
        CURSOR.store((c + 1) as u8, Ordering::Relaxed);
    }
}

// ── Activate (place)
// ──────────────────────────────────────────────────────────

pub fn activate() {
    if RESULT.load(Ordering::Relaxed) == RESULT_GAME_OVER {
        // Any Fire on the game-over screen closes.
        close();
        return;
    }

    let pos = CURSOR.load(Ordering::Relaxed) as usize;
    if pos >= CELLS || BOARD[pos].load(Ordering::Relaxed) != T_EMPTY {
        return; // can't place on a filled cell
    }
    let placed = NEXT_TILE.load(Ordering::Relaxed);
    BOARD[pos].store(placed, Ordering::Relaxed);

    // Per-tile placement effects.
    match placed {
        // Wildcard becomes the highest-tier neighbour and may cascade.
        T_WILDCARD => {
            place_wildcard(pos);
        }
        // Cars don't merge — only their wrecks do.
        T_CAR => {}
        _ => {
            cascade_from(pos);
        }
    }

    // Move every car: each picks an empty neighbour, otherwise
    // becomes a Wreck in place.  Movement uses the per-game RNG so a
    // patient player can learn to predict cars from the seed (mirrors
    // Triple Town's bear behaviour).  New wrecks may trigger cascades.
    move_cars();

    // Roll the next tile for preview, then check for game over.
    NEXT_TILE.store(roll_next_tile(), Ordering::Relaxed);
    if !any_empty() {
        RESULT.store(RESULT_GAME_OVER, Ordering::Relaxed);
    }
}

/// Swap the next-tile preview with the stash.  Empty stash → the
/// current preview goes into storage and a fresh tile is rolled to
/// take its place.  Triggered by the Execute button.
pub fn swap_stash() {
    if !ACTIVE.load(Ordering::Relaxed) {
        return;
    }
    if RESULT.load(Ordering::Relaxed) == RESULT_GAME_OVER {
        return;
    }
    let cur = NEXT_TILE.load(Ordering::Relaxed);
    let stashed = STASH.load(Ordering::Relaxed);
    if stashed == T_EMPTY {
        STASH.store(cur, Ordering::Relaxed);
        NEXT_TILE.store(roll_next_tile(), Ordering::Relaxed);
    } else {
        STASH.store(cur, Ordering::Relaxed);
        NEXT_TILE.store(stashed, Ordering::Relaxed);
    }
}

/// Wildcard placement: pick the type that yields the best merge from
/// `idx`'s 4-neighbours and rewrite the cell as that type, then
/// cascade.  If no neighbour produces a 3-match, settle on the
/// highest-tier neighbour anyway so the wildcard isn't wasted; if
/// there are no eligible neighbours at all, leave the wildcard in
/// place for later.
fn place_wildcard(idx: usize) {
    let r = idx / BOARD_W;
    let c = idx % BOARD_W;
    let neighbours: [(usize, usize); 4] = [
        (r.wrapping_sub(1), c),
        (r + 1, c),
        (r, c.wrapping_sub(1)),
        (r, c + 1),
    ];
    let mut tried = [false; 16]; // tile id is u8 < 16 in practice
    let mut best: u8 = T_EMPTY;
    let mut best_tier: u8 = 0;
    let mut best_match: bool = false;
    for (nr, nc) in neighbours {
        if nr >= BOARD_H || nc >= BOARD_W {
            continue;
        }
        let t = BOARD[nr * BOARD_W + nc].load(Ordering::Relaxed);
        if t == T_EMPTY || t == T_CAR || t == T_WILDCARD {
            continue;
        }
        if tried[t as usize] {
            continue;
        }
        tried[t as usize] = true;
        // Pretend the wildcard cell is type t and see if a 3-merge
        // would form.  Restore between trials so each candidate is
        // evaluated against the same neighbour set.
        BOARD[idx].store(t, Ordering::Relaxed);
        let mut group = [0u8; CELLS];
        let count = flood_same_type(idx, &mut group);
        let tier_v = wildcard_tier(t);
        let is_match = count >= 3;
        let take = match (is_match, best_match) {
            (true, false) => true,   // upgrade from no-match
            (false, true) => false,  // never downgrade
            _ => tier_v > best_tier, // tie on match status: highest tier wins
        };
        if take {
            best = t;
            best_tier = tier_v;
            best_match = is_match;
        }
    }
    if best == T_EMPTY {
        // No eligible neighbour — leave it as a wildcard.
        BOARD[idx].store(T_WILDCARD, Ordering::Relaxed);
        return;
    }
    BOARD[idx].store(best, Ordering::Relaxed);
    cascade_from(idx);
}

/// Effective "tier value" for choosing which type a wildcard becomes.
/// Camp tiers map directly; vehicle series sits above them so a
/// wildcard prefers a Scrapheap match over a Bush match.
fn wildcard_tier(t: u8) -> u8 {
    match t {
        T_GRASS..=T_AEGG => t,
        T_WRECK => 11,
        T_JUNKYARD => 12,
        T_SCRAPHEAP => 13,
        _ => 0,
    }
}

// ── Cascade-merge
// ─────────────────────────────────────────────────────────────

/// Run the place-merge loop starting at `idx`.  If 3+ same-type tiles
/// are 4-connected to `idx`, they merge into the next tier at `idx`.
/// Repeats while the upgrade itself completes another match.
fn cascade_from(idx: usize) {
    loop {
        let tile = BOARD[idx].load(Ordering::Relaxed);
        let upgrade = match next_tier(tile) {
            Some(t) => t,
            None => break,
        };

        // Flood-fill the connected same-type group.
        let mut group = [0u8; MAX_GROUP];
        let count = flood_same_type(idx, &mut group);
        if count < 3 {
            break;
        }

        // Merge: clear the group except `idx`, place the upgrade
        // there, score the new tier.
        for &i in &group[..count] {
            if (i as usize) != idx {
                BOARD[i as usize].store(T_EMPTY, Ordering::Relaxed);
            }
        }
        BOARD[idx].store(upgrade, Ordering::Relaxed);
        // Score: base tier² plus a per-extra-tile bonus so a 4- or
        // 5-match scores higher than the minimum 3-match.  At tier 1
        // (Grass) every extra tile adds 1 point; at tier 9 (Aegg)
        // every extra adds 9.  Mirrors Triple Town's "more matches =
        // more points" rule.
        let base = score_for_tier(upgrade);
        let extras = (count as u32 - 3) * upgrade as u32;
        SCORE.fetch_add(base + extras, Ordering::Relaxed);

        // Loop again: the upgrade may chain into another match.
        // `idx` stays the same — the upgraded tile sits there.
    }
}

/// Tile that `tile` upgrades to when 3 of it merge.  None at the top
/// of each series (CyberAegg, Scrapheap) and for non-mergeable tiles.
fn next_tier(tile: u8) -> Option<u8> {
    match tile {
        T_GRASS..=T_FLAG => Some(tile + 1),
        T_AEGG => None,
        T_WRECK => Some(T_JUNKYARD),
        T_JUNKYARD => Some(T_SCRAPHEAP),
        T_SCRAPHEAP => None,
        // Empty / Car never merge.
        _ => None,
    }
}

/// 4-connected flood-fill collecting all same-type cells reachable
/// from `start`.  Writes their indices into `out` and returns the
/// count.  Caller-supplied buffer keeps the fill heap-free.
fn flood_same_type(start: usize, out: &mut [u8; MAX_GROUP]) -> usize {
    let target = BOARD[start].load(Ordering::Relaxed);
    if target == T_EMPTY {
        return 0;
    }
    let mut visited = [false; CELLS];
    let mut stack = [0u8; CELLS];
    let mut top = 0usize;
    stack[top] = start as u8;
    top += 1;
    visited[start] = true;

    let mut count = 0usize;
    while top > 0 {
        top -= 1;
        let i = stack[top] as usize;
        out[count] = i as u8;
        count += 1;

        let r = i / BOARD_W;
        let c = i % BOARD_W;
        // 4-connected neighbours.
        let candidates: [(usize, usize); 4] = [
            (r.wrapping_sub(1), c),
            (r + 1, c),
            (r, c.wrapping_sub(1)),
            (r, c + 1),
        ];
        for (nr, nc) in candidates {
            if nr >= BOARD_H || nc >= BOARD_W {
                continue;
            }
            let ni = nr * BOARD_W + nc;
            if visited[ni] {
                continue;
            }
            if BOARD[ni].load(Ordering::Relaxed) != target {
                continue;
            }
            visited[ni] = true;
            stack[top] = ni as u8;
            top += 1;
        }
    }
    count
}

fn score_for_tier(tier: u8) -> u32 {
    // Quadratic in the visual tier: tier 2 (Bush) → 4, tier 9 (Aegg)
    // → 81.  Vehicle series uses a tier offset so a Junkyard (id 12)
    // doesn't outscore the Aegg (id 9).
    let base = match tier {
        T_GRASS..=T_AEGG => tier as u32,
        T_WRECK => 2,
        T_JUNKYARD => 4,
        T_SCRAPHEAP => 6,
        _ => 1,
    };
    base * base
}

// ── Cars ──────────────────────────────────────────────────────────────────────

/// Move every car on the board to a random 4-connected empty cell;
/// cars with no empty neighbour transform into Wrecks in place, then
/// cascade-merge from each wreck position.
fn move_cars() {
    // Snapshot car positions so a car that's moved this turn isn't
    // touched again.
    let mut cars = [0u8; CELLS];
    let mut n = 0usize;
    for i in 0..CELLS {
        if BOARD[i].load(Ordering::Relaxed) == T_CAR {
            cars[n] = i as u8;
            n += 1;
        }
    }

    let mut wreck_indices = [0u8; CELLS];
    let mut n_wrecks = 0usize;

    for &car in &cars[..n] {
        let from = car as usize;
        // Re-check: the car may have already moved (unlikely with
        // snapshot, but stay defensive).
        if BOARD[from].load(Ordering::Relaxed) != T_CAR {
            continue;
        }
        // Find empty neighbours.
        let r = from / BOARD_W;
        let c = from % BOARD_W;
        let candidates: [(usize, usize); 4] = [
            (r.wrapping_sub(1), c),
            (r + 1, c),
            (r, c.wrapping_sub(1)),
            (r, c + 1),
        ];
        let mut empties = [0u8; 4];
        let mut e_count = 0usize;
        for (nr, nc) in candidates {
            if nr >= BOARD_H || nc >= BOARD_W {
                continue;
            }
            let ni = nr * BOARD_W + nc;
            if BOARD[ni].load(Ordering::Relaxed) == T_EMPTY {
                empties[e_count] = ni as u8;
                e_count += 1;
            }
        }
        if e_count == 0 {
            // Trapped → becomes a Wreck.
            BOARD[from].store(T_WRECK, Ordering::Relaxed);
            wreck_indices[n_wrecks] = from as u8;
            n_wrecks += 1;
        } else {
            let pick = empties[(rng_next() as usize) % e_count] as usize;
            BOARD[from].store(T_EMPTY, Ordering::Relaxed);
            BOARD[pick].store(T_CAR, Ordering::Relaxed);
        }
    }

    // Newly-formed wrecks may cascade.
    for &w in &wreck_indices[..n_wrecks] {
        if BOARD[w as usize].load(Ordering::Relaxed) == T_WRECK {
            cascade_from(w as usize);
        }
    }
}

// ── Helpers
// ───────────────────────────────────────────────────────────────────

fn any_empty() -> bool {
    BOARD.iter().any(|c| c.load(Ordering::Relaxed) == T_EMPTY)
}

/// Roll a fresh tile for the preview slot, with Triple-Town-style
/// odds: mostly grass, occasionally bush/tree, rare car/wildcard.
fn roll_next_tile() -> u8 {
    let r = rng_next() & 0xFF;
    if r < 160 {
        T_GRASS // ~62 %
    } else if r < 215 {
        T_BUSH // ~21 %
    } else if r < 232 {
        T_TREE // ~7 %
    } else if r < 245 {
        T_CAR // ~5 %
    } else {
        T_WILDCARD // ~4 %
    }
}

/// Advance the per-game xorshift32 RNG and return its new state.
fn rng_next() -> u32 {
    let mut x = GAME_RNG.load(Ordering::Relaxed);
    if x == 0 {
        x = 0xCAFE_BABE; // defensive — open() seeds from non-zero uptime
    }
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    GAME_RNG.store(x, Ordering::Relaxed);
    x
}

/// Seed for [`GAME_RNG`].  Hardware uses uptime; sim uses wall-clock
/// nanoseconds; both OR with 1 to avoid the all-zero state that
/// xorshift can't escape.
fn rng_seed() -> u32 {
    #[cfg(feature = "embassy-base")]
    {
        (embassy_time::Instant::now().as_ticks() as u32) | 1
    }
    #[cfg(not(feature = "embassy-base"))]
    {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| (d.as_nanos() as u32) | 1)
            .unwrap_or(0xCAFE_BABE)
    }
}

// ── Drawing
// ───────────────────────────────────────────────────────────────────

pub fn draw<D>(display: &mut D) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    // Background.
    Rectangle::new(Point::zero(), Size::new(152, 152))
        .into_styled(PrimitiveStyle::with_fill(WHITE))
        .draw(display)?;

    // ── Status bar (top 20 px) ────────────────────────────────────
    //
    // Layout:  [N: ◐]  [S: ◐]               1234
    //          ^ next   ^ stash    score (right-aligned)
    let font = MonoTextStyle::new(&FONT_6X13_BOLD, BLACK);
    let left = TextStyleBuilder::new()
        .baseline(Baseline::Middle)
        .alignment(Alignment::Left)
        .build();
    let right = TextStyleBuilder::new()
        .baseline(Baseline::Middle)
        .alignment(Alignment::Right)
        .build();

    let bar_y = STATUS_H / 2;

    Text::with_text_style("N:", Point::new(2, bar_y), font, left).draw(display)?;
    let next_tile = NEXT_TILE.load(Ordering::Relaxed);
    draw_tile_small(display, 22, bar_y, next_tile)?;

    Text::with_text_style("S:", Point::new(40, bar_y), font, left).draw(display)?;
    let stash = STASH.load(Ordering::Relaxed);
    if stash != T_EMPTY {
        draw_tile_small(display, 60, bar_y, stash)?;
    } else {
        // Empty placeholder — small open circle.
        Circle::new(Point::new(60 - 5, bar_y - 5), 11)
            .into_styled(PrimitiveStyle::with_stroke(BLACK, 1))
            .draw(display)?;
    }

    let score = SCORE.load(Ordering::Relaxed);
    let mut score_buf: heapless::String<16> = heapless::String::new();
    let _ = core::fmt::Write::write_fmt(&mut score_buf, format_args!("{}", score));
    Text::with_text_style(score_buf.as_str(), Point::new(150, bar_y), font, right).draw(display)?;

    // Divider below status bar.
    Rectangle::new(Point::new(0, STATUS_H - 1), Size::new(152, 1))
        .into_styled(PrimitiveStyle::with_fill(BLACK))
        .draw(display)?;

    // ── Board ─────────────────────────────────────────────────────
    let cursor = CURSOR.load(Ordering::Relaxed) as usize;
    let game_over = RESULT.load(Ordering::Relaxed) == RESULT_GAME_OVER;

    let fill_black = PrimitiveStyle::with_fill(BLACK);
    let board_pixels = (BOARD_W as i32) * CELL;

    // Outer 2-px frame around the whole grid.  Top + right are
    // entirely owned by the frame (cells don't draw their top or
    // right side).  Left + bottom are 1 px from the frame plus 1 px
    // contributed by the leftmost-column / bottom-row cell L's,
    // adding up to 2 px on those sides too.
    // Top — 2 px tall.
    Rectangle::new(
        Point::new(BOARD_X - 1, BOARD_Y - 2),
        Size::new((board_pixels + 2) as u32, 2),
    )
    .into_styled(fill_black)
    .draw(display)?;
    // Right — 2 px wide, full vertical span including the corners
    // (so the top + right corner is closed).
    Rectangle::new(
        Point::new(BOARD_X + board_pixels, BOARD_Y - 2),
        Size::new(2, (board_pixels + 4) as u32),
    )
    .into_styled(fill_black)
    .draw(display)?;
    // Left — 1 px (cell L contributes the other px).
    Rectangle::new(
        Point::new(BOARD_X - 1, BOARD_Y - 1),
        Size::new(1, (board_pixels + 2) as u32),
    )
    .into_styled(fill_black)
    .draw(display)?;
    // Bottom — 1 px (cell L contributes the other px).
    Rectangle::new(
        Point::new(BOARD_X - 1, BOARD_Y + board_pixels),
        Size::new((board_pixels + 2) as u32, 1),
    )
    .into_styled(fill_black)
    .draw(display)?;

    for i in 0..CELLS {
        let r = i / BOARD_W;
        let c = i % BOARD_W;
        let x = BOARD_X + (c as i32) * CELL;
        let y = BOARD_Y + (r as i32) * CELL;
        let tile = BOARD[i].load(Ordering::Relaxed);
        let cx = x + CELL / 2;
        let cy = y + CELL / 2;
        // L-shaped 1 px cell border: left edge + bottom edge.  The
        // right edge is covered by the next column's left, the top by
        // the previous row's bottom (or the outer frame on row 0 /
        // col rightmost / etc.).
        Rectangle::new(Point::new(x, y), Size::new(1, CELL as u32))
            .into_styled(fill_black)
            .draw(display)?;
        Rectangle::new(Point::new(x, y + CELL - 1), Size::new(CELL as u32, 1))
            .into_styled(fill_black)
            .draw(display)?;
        draw_tile(display, cx, cy, tile)?;
        if !game_over && i == cursor {
            draw_cursor_ring(display, x, y)?;
        }
    }

    if game_over {
        let mut s: heapless::String<24> = heapless::String::new();
        let _ = core::fmt::Write::write_fmt(&mut s, format_args!("Score: {}", score));
        ui::draw_picker_menu(display, "Game Over", &[s.as_str(), "Press Fire"], 0)?;
    }

    Ok(())
}

/// Render a tile glyph centred on (cx, cy) at full board cell size.
fn draw_tile<D>(display: &mut D, cx: i32, cy: i32, tile: u8) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    match tile {
        T_EMPTY => Ok(()),
        T_GRASS..=T_AEGG => draw_camp_tile(display, cx, cy, tile, TILE_RADIUS),
        T_CAR => draw_car(display, cx, cy),
        T_WRECK | T_JUNKYARD | T_SCRAPHEAP => draw_wreck_tile(display, cx, cy, tile, TILE_RADIUS),
        T_WILDCARD => draw_wildcard(display, cx, cy, TILE_RADIUS),
        _ => Ok(()),
    }
}

/// Compact tile glyph used in the status-bar previews.
fn draw_tile_small<D>(display: &mut D, cx: i32, cy: i32, tile: u8) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    match tile {
        T_EMPTY => Ok(()),
        T_GRASS..=T_AEGG => draw_camp_tile(display, cx, cy, tile, TILE_RADIUS_SMALL),
        T_CAR => draw_car_small(display, cx, cy),
        T_WRECK | T_JUNKYARD | T_SCRAPHEAP => {
            draw_wreck_tile(display, cx, cy, tile, TILE_RADIUS_SMALL)
        }
        T_WILDCARD => draw_wildcard(display, cx, cy, TILE_RADIUS_SMALL),
        _ => Ok(()),
    }
}

/// Wildcard tile: black disc with a bold "?" in white.
fn draw_wildcard<D>(display: &mut D, cx: i32, cy: i32, radius: i32) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let dia = (radius * 2 + 1) as u32;
    Circle::new(Point::new(cx - radius, cy - radius), dia)
        .into_styled(PrimitiveStyle::with_fill(BLACK))
        .draw(display)?;
    draw_bold_text(display, "?", cx, cy + 1, WHITE)
}

/// Camp tile: filled black disc with the tier number in white,
/// rendered with the smallest bold mono font.
fn draw_camp_tile<D>(
    display: &mut D,
    cx: i32,
    cy: i32,
    tier: u8,
    radius: i32,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let dia = (radius * 2 + 1) as u32;
    Circle::new(Point::new(cx - radius, cy - radius), dia)
        .into_styled(PrimitiveStyle::with_fill(BLACK))
        .draw(display)?;
    let mut buf: heapless::String<2> = heapless::String::new();
    let _ = core::fmt::Write::write_fmt(&mut buf, format_args!("{}", tier));
    draw_bold_text(display, buf.as_str(), cx, cy + 1, WHITE)
}

/// Wreck-series tile: filled black square with bold W/J/S in
/// white.
fn draw_wreck_tile<D>(
    display: &mut D,
    cx: i32,
    cy: i32,
    tile: u8,
    half: i32,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let side = (half * 2 + 1) as u32;
    Rectangle::new(Point::new(cx - half, cy - half), Size::new(side, side))
        .into_styled(PrimitiveStyle::with_fill(BLACK))
        .draw(display)?;
    let letter = match tile {
        T_WRECK => "W",
        T_JUNKYARD => "J",
        _ => "S",
    };
    draw_bold_text(display, letter, cx, cy + 1, WHITE)
}

/// Render a short label centred on `(cx, cy)` in the smallest bold
/// `mono_font::ascii` font (`FONT_6X13_BOLD`).  Real bold strokes
/// survive the EPD's low contrast better than the fake-bold trick
/// (`FONT_6X10` rendered twice with an offset) used elsewhere.
fn draw_bold_text<D>(
    display: &mut D,
    s: &str,
    cx: i32,
    cy: i32,
    color: TriColor,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let style = MonoTextStyle::new(&FONT_6X13_BOLD, color);
    let centred = TextStyleBuilder::new()
        .baseline(Baseline::Middle)
        .alignment(Alignment::Center)
        .build();
    Text::with_text_style(s, Point::new(cx, cy), style, centred).draw(display)?;
    Ok(())
}

/// Car drawn at the board cell centre.
fn draw_car<D>(display: &mut D, cx: i32, cy: i32) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    // Body 14×6 with cabin 8×4 on top and two wheels.
    Rectangle::new(Point::new(cx - 7, cy - 2), Size::new(14, 6))
        .into_styled(PrimitiveStyle::with_fill(BLACK))
        .draw(display)?;
    Rectangle::new(Point::new(cx - 4, cy - 6), Size::new(8, 4))
        .into_styled(PrimitiveStyle::with_fill(BLACK))
        .draw(display)?;
    Circle::new(Point::new(cx - 7, cy + 4), 4)
        .into_styled(PrimitiveStyle::with_fill(BLACK))
        .draw(display)?;
    Circle::new(Point::new(cx + 4, cy + 4), 4)
        .into_styled(PrimitiveStyle::with_fill(BLACK))
        .draw(display)?;
    Ok(())
}

fn draw_car_small<D>(display: &mut D, cx: i32, cy: i32) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    Rectangle::new(Point::new(cx - 5, cy - 1), Size::new(10, 4))
        .into_styled(PrimitiveStyle::with_fill(BLACK))
        .draw(display)?;
    Rectangle::new(Point::new(cx - 3, cy - 4), Size::new(6, 3))
        .into_styled(PrimitiveStyle::with_fill(BLACK))
        .draw(display)?;
    Ok(())
}

/// Cell-wide cursor dither: every other pixel across the entire cell
/// is flipped on a diagonal checkerboard, alternating BLACK and
/// WHITE.  The 50 % coverage gives a clear grey overlay against both
/// empty (white) and filled (black) cells.  Pure B/W keeps it cheap
/// for the fast LUT refresh.
fn draw_cursor_ring<D>(display: &mut D, x: i32, y: i32) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let pixels = (0..CELL).flat_map(move |dy| {
        (0..CELL).map(move |dx| {
            let color = if (dx + dy) & 1 == 0 { BLACK } else { WHITE };
            Pixel(Point::new(x + dx, y + dy), color)
        })
    });
    display.draw_iter(pixels)
}
