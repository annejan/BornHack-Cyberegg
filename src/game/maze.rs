//! Maze mini-game.
//!
//! An 18×18 procedurally generated maze rendered on the 152×152 EPD.
//!
//! # Layout
//! Each cell is 8×8 pixels with a 4 px border around the maze area:
//!   - Maze grid: 18 × 8 = 144 px wide / tall
//!   - Border: 4 px on each side  → total 152 × 152 (fits exactly)
//!
//! # Maze generation
//! Recursive Backtracker (aka randomised DFS) — produces a perfect maze
//! (all cells connected, exactly one path between any two cells).
//!
//! # Exits
//! 1–4 exits are punched through the outer wall at randomly chosen
//! border positions.  The player starts at a random interior cell that
//! is at least 5 cells from every exit (Manhattan distance).
//!
//! # Fog of war
//! Only cells the player has already visited are rendered as floor.
//! Unvisited cells are drawn as solid black (unknown territory).
//!
//! # Winning
//! Stepping onto an exit cell triggers a win screen. Pressing any
//! button from the win screen closes the game and awards inspiration.
//!
//! # Integration
//! - Add `pub mod maze;` to `src/game/mod.rs`.
//! - In `draw_screen_game` add the takeover guard (see mod.rs comment below).
//! - In `input.rs` add the input block (see input.rs comment below).
//! - In `modal.rs` add `"Maze"` to the Play items list and `"Maze" => { super::maze::open(); close(); }` to activate.
//!
//! State is held entirely in module-level statics — no heap, no alloc.

use core::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, Ordering};

use embedded_graphics::{
    mono_font::{ascii::FONT_7X13_BOLD, MonoTextStyle},
    prelude::*,
    primitives::{PrimitiveStyle, Rectangle},
    text::{Alignment, Baseline, Text, TextStyleBuilder},
};

use crate::{BLACK, TriColor, WHITE};

// ── Maze dimensions ───────────────────────────────────────────────────────────

/// Number of cells along each axis.
const W: usize = 18;
const H: usize = 18;
const CELLS: usize = W * H; // 324

/// Pixel size of one cell (8×8).
const CELL: i32 = 8;
/// Pixel border around the whole maze.
const BORDER: i32 = 4;

// ── Wall bitmask encoding ─────────────────────────────────────────────────────
//
// For every cell we store which of its FOUR walls are OPEN (passage exists).
// Using a nibble: bit 0=North, bit 1=East, bit 2=South, bit 3=West.
//
// All 324 cells × 4 bits = 1296 bits = 162 bytes — stored as 41 u32s (164 B).
// We round up to 41 u32s for easy packing.
//
// Visited state: 324 bits = 41 u32s (same shape).

const PACK_LEN: usize = (CELLS + 31) / 32; // = 11 u32s  (rounds up)

const NORTH: u8 = 0b0001;
const EAST: u8 = 0b0010;
const SOUTH: u8 = 0b0100;
const WEST: u8 = 0b1000;

// ── Exit cell encoding ────────────────────────────────────────────────────────
//
// Up to 4 exits. Each exit is stored as the (row, col) of the border cell that
// has been "punched through".  0xFF = unused slot.

const MAX_EXITS: usize = 4;

// ── Global state ──────────────────────────────────────────────────────────────

/// Whether the maze screen is currently active.
static ACTIVE: AtomicBool = AtomicBool::new(false);

/// Player position: row (high byte) | col (low byte), packed into u16 → u32.
static PLAYER: AtomicU32 = AtomicU32::new(0);

/// Won flag.
static WON: AtomicBool = AtomicBool::new(false);

/// Number of steps taken.
static STEPS: AtomicU32 = AtomicU32::new(0);

/// How far through the cheat sequence the player has typed (0 = not started).
static CHEAT_POS: AtomicU8 = AtomicU8::new(0);

/// Whether the cheat has been activated (full maze revealed).
static REVEALED: AtomicBool = AtomicBool::new(false);

// Walls: CELLS cells × 4 bits each. We store the open-wall nibble for each
// cell as a u8 in a [AtomicU8; CELLS].  The array is 324 bytes — well within
// the ~250 KB RAM budget of the nRF52840.
//
// Can't use [AtomicU8; 324] as a static directly (no const Default), so we
// use a newtype wrapper with manual initialisation.

/// Packed open-wall nibble for each cell. Index = row*W + col.
static WALLS: [AtomicU8; CELLS] = {
    // const initialiser — all walls closed (0).
    const INIT: AtomicU8 = AtomicU8::new(0);
    [INIT; CELLS]
};

/// Visited bitfield (one bit per cell).  Index i → u32 i/32, bit i%32.
static VISITED: [AtomicU32; PACK_LEN] = {
    const INIT: AtomicU32 = AtomicU32::new(0);
    [INIT; PACK_LEN]
};

/// Exit positions: packed as (row << 8 | col).  0xFFFF = unused.
static EXITS: [AtomicU32; MAX_EXITS] = {
    const INIT: AtomicU32 = AtomicU32::new(0xFFFF);
    [INIT; MAX_EXITS]
};

// ── Maze source ───────────────────────────────────────────────────────────────
//
// Set MAZE_BASE64 to a non-empty base64 string exported from the maze editor
// to load that specific maze instead of generating a random one.
// Leave it as "" to always generate a random maze.
//const MAZE_BASE64: &str = "TVoBEhICEQoAAf////8JCVRkqqqqysbGxvc9qqqqWFVVVVWnqqqqODk5WVVlqqrqqqrqXNdVpqr6qqpYVVXTZar6qijaVVVUFab6qkxUVVVVRWX+ylVVVTVZUVVR0lFVVeN4/vv6+v7bVfJYVXRYVFVUVVRUVTX6m1XXVVVVFaPaollVVfNdRaL6qppVVdZVcap4qqpZVVU1uqq6qqq6XVWjqqqqbKqqWTGqqqqKNaqqmg==";
const MAZE_BASE64: &str = "";

// ── Public API ────────────────────────────────────────────────────────────────

pub fn is_active() -> bool {
    ACTIVE.load(Ordering::Relaxed)
}

/// Open the maze.  If MAZE_BASE64 is non-empty the editor-created maze is
/// decoded and loaded; otherwise a fresh random maze is generated.
pub fn open() {
    if MAZE_BASE64.is_empty() {
        generate();
    } else if !load_from_base64(MAZE_BASE64) {
        // Decoding failed — fall back to random so the game still works.
        generate();
    }
    WON.store(false, Ordering::Relaxed);
    STEPS.store(0, Ordering::Relaxed);
    ACTIVE.store(true, Ordering::Relaxed);
}

/// Close the maze (and award inspiration if the player won).
pub fn close() {
    if WON.load(Ordering::Relaxed) {
        super::lifecycle::award_inspiration();
        super::show_toast(super::Toast::Inspired);
    }
    ACTIVE.store(false, Ordering::Relaxed);
}

// ── Movement ──────────────────────────────────────────────────────────────────

// ── Cheat codes ───────────────────────────────────────────────────────────────
//
// Two independent sequences are tracked simultaneously using separate position
// counters so entering one can't accidentally interfere with the other.
//
// Sequence A — Reveal:     Up Up Down Execute Left Left Right
// Sequence B — New random: Up Execute Execute Down
//
// Button codes: 0=Up  1=Down  2=Left  3=Right  4=Execute

const CHEAT_SEQ_REVEAL: [u8; 7] = [0, 0, 1, 4, 2, 2, 3];
const CHEAT_SEQ_REGEN:  [u8; 4] = [0, 4, 4, 1];

/// Position within the REGEN sequence (0 = not started).
static REGEN_POS: AtomicU8 = AtomicU8::new(0);

/// Advance a cheat sequence tracker.  Returns true when the sequence completes.
/// `pos_atomic` is the AtomicU8 tracking progress; `seq` is the target sequence.
fn advance_seq(pos_atomic: &AtomicU8, seq: &[u8], code: u8) -> bool {
    let pos = pos_atomic.load(Ordering::Relaxed) as usize;
    if seq[pos] == code {
        let next = pos + 1;
        if next == seq.len() {
            pos_atomic.store(0, Ordering::Relaxed);
            return true;
        }
        pos_atomic.store(next as u8, Ordering::Relaxed);
    } else {
        // Wrong button — restart, but credit it if it starts the sequence.
        pos_atomic.store(if seq[0] == code { 1 } else { 0 }, Ordering::Relaxed);
    }
    false
}

/// Feed the next button code into both cheat detectors.
fn check_cheat(code: u8) {
    // Sequence A: Reveal full maze.
    if advance_seq(&CHEAT_POS, &CHEAT_SEQ_REVEAL, code) {
        REVEALED.store(true, Ordering::Relaxed);
    }

    // Sequence B: Generate a brand-new random maze immediately.
    if advance_seq(&REGEN_POS, &CHEAT_SEQ_REGEN, code) {
        generate();
        // Reset round state but keep the game active.
        WON.store(false, Ordering::Relaxed);
        STEPS.store(0, Ordering::Relaxed);
    }
}

pub fn move_up() {
    if WON.load(Ordering::Relaxed) { close(); return; }
    check_cheat(0);
    try_move_dir(0); // North
}

pub fn move_down() {
    if WON.load(Ordering::Relaxed) { close(); return; }
    check_cheat(1);
    try_move_dir(2); // South
}

pub fn move_left() {
    if WON.load(Ordering::Relaxed) { close(); return; }
    check_cheat(2);
    try_move_dir(3); // West
}

pub fn move_right() {
    if WON.load(Ordering::Relaxed) { close(); return; }
    check_cheat(3);
    try_move_dir(1); // East
}

/// Fire / Execute on the win screen also closes.
pub fn activate() {
    if WON.load(Ordering::Relaxed) { close(); return; }
    check_cheat(4);
}

// ── Movement helpers ──────────────────────────────────────────────────────────

/// Try to move in a direction (0=N, 1=E, 2=S, 3=W).
fn try_move_dir(dir: u8) {
    let packed = PLAYER.load(Ordering::Relaxed);
    let row = (packed >> 8) as usize;
    let col = (packed & 0xFF) as usize;

    // Check wall (is passage open in that direction?)
    let wall_nibble = WALLS[row * W + col].load(Ordering::Relaxed);
    let wall_bit = 1u8 << dir;
    if wall_nibble & wall_bit == 0 {
        return; // wall blocks movement
    }

    // Compute new position.
    let (new_row, new_col) = match dir {
        0 => (row.wrapping_sub(1), col),
        1 => (row, col + 1),
        2 => (row + 1, col),
        3 => (row, col.wrapping_sub(1)),
        _ => return,
    };

    // Bounds check — if the move would leave the grid the player is either
    // stepping through an exit (win!) or hitting a corner gap (ignore).
    if new_row >= H || new_col >= W {
        check_exit_escape(row, col, dir);
        return;
    }

    // Move player.
    PLAYER.store(((new_row as u32) << 8) | new_col as u32, Ordering::Relaxed);
    STEPS.fetch_add(1, Ordering::Relaxed);
    mark_visited(new_row, new_col);
}

/// Called when the player presses a direction while already standing on an
/// exit cell and the move would take them off the grid edge.  That is the
/// moment they actually escape — not when they first step onto the exit.
fn check_exit_escape(row: usize, col: usize, dir: u8) {
    // Is this cell an exit whose open wall faces in `dir`?
    let packed = ((row as u32) << 8) | col as u32;
    let is_exit = EXITS.iter().any(|s| s.load(Ordering::Relaxed) == packed);
    if !is_exit { return; }

    // The exit wall must be open in that direction.
    let walls = WALLS[row * W + col].load(Ordering::Relaxed);
    let dir_bit = 1u8 << dir; // 0=N,1=E,2=S,3=W → NORTH/EAST/SOUTH/WEST bits
    if walls & dir_bit != 0 {
        WON.store(true, Ordering::Relaxed);
    }
}

// ── Visited bitfield helpers ──────────────────────────────────────────────────

fn mark_visited(row: usize, col: usize) {
    let i = row * W + col;
    let word = i / 32;
    let bit = i % 32;
    VISITED[word].fetch_or(1u32 << bit, Ordering::Relaxed);
}

fn is_visited(row: usize, col: usize) -> bool {
    let i = row * W + col;
    let word = i / 32;
    let bit = i % 32;
    VISITED[word].load(Ordering::Relaxed) & (1u32 << bit) != 0
}

fn clear_visited() {
    for v in &VISITED {
        v.store(0, Ordering::Relaxed);
    }
}

// ── Wall helpers ──────────────────────────────────────────────────────────────

fn open_wall(row: usize, col: usize, dir: u8) {
    WALLS[row * W + col].fetch_or(dir, Ordering::Relaxed);
}

fn clear_walls() {
    for w in &WALLS {
        w.store(0, Ordering::Relaxed);
    }
}

// ── Simple PRNG ───────────────────────────────────────────────────────────────

/// Global PRNG state (updated by each call).
static RNG: AtomicU32 = AtomicU32::new(0xDEAD_BEEF);

fn rng_next() -> u32 {
    let mut x = RNG.load(Ordering::Relaxed);
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    RNG.store(x, Ordering::Relaxed);
    x
}

/// Seed the PRNG from embassy uptime or a compile-time fallback.
fn rng_seed() -> u32 {
    #[cfg(feature = "embassy-base")]
    { embassy_time::Instant::now().as_ticks() as u32 }
    #[cfg(feature = "simulator")]
    {
        // Use wall-clock nanoseconds so each run gets a different maze.
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.subsec_nanos() ^ (d.as_secs() as u32))
            .unwrap_or(0xFEED_FACE)
    }
    #[cfg(not(any(feature = "embassy-base", feature = "simulator")))]
    { 0xFEED_FACE }
}

// ── Stack for iterative DFS ───────────────────────────────────────────────────
//
// Recursive Backtracker (DFS) requires a stack of cell indices.  Maximum depth
// is CELLS = 324 entries × 2 bytes each = 648 bytes on the stack.  The
// embedded stack on nRF52840 is at least 4 KB so this is fine.
//
// We use a simple fixed-size array stack rather than alloc.

const STACK_MAX: usize = CELLS;

struct Stack {
    buf: [u16; STACK_MAX],
    top: usize,
}

impl Stack {
    const fn new() -> Self {
        Self { buf: [0u16; STACK_MAX], top: 0 }
    }
    fn push(&mut self, v: u16) {
        if self.top < STACK_MAX {
            self.buf[self.top] = v;
            self.top += 1;
        }
    }
    fn pop(&mut self) -> Option<u16> {
        if self.top == 0 { None } else { self.top -= 1; Some(self.buf[self.top]) }
    }
    fn is_empty(&self) -> bool { self.top == 0 }
}

// ── Maze generation ───────────────────────────────────────────────────────────

/// Visited scratch buffer for DFS — independent of the player visited state.
/// 324 bits = 11 u32s.
static GEN_VISITED: [AtomicU32; PACK_LEN] = {
    const INIT: AtomicU32 = AtomicU32::new(0);
    [INIT; PACK_LEN]
};

fn gen_mark(i: usize) {
    GEN_VISITED[i / 32].fetch_or(1 << (i % 32), Ordering::Relaxed);
}

fn gen_is_visited(i: usize) -> bool {
    GEN_VISITED[i / 32].load(Ordering::Relaxed) & (1 << (i % 32)) != 0
}

fn gen_clear() {
    for v in &GEN_VISITED {
        v.store(0, Ordering::Relaxed);
    }
}

/// Generate a new perfect maze using iterative Recursive Backtracker DFS.
// ── Load maze from base64 ─────────────────────────────────────────────────────

/// Decode a base64 string produced by the maze editor and populate the maze
/// state from it.  Returns false and leaves state unchanged if decoding fails.
///
/// Expected binary layout (178 bytes for 18×18):
///   [0..1]  b"MZ"
///   [2]     version = 1
///   [3]     width
///   [4]     height
///   [5]     n_exits
///   [6..13] 4 × (exit_row, exit_col), 0xFF = unused
///   [14]    start_row (0xFF = not set)
///   [15]    start_col
///   [16..]  wall nibbles, 2 cells/byte (low nibble = even cell index)
fn load_from_base64(encoded: &str) -> bool {
    // ── Base64 decode (no_std, no alloc) ─────────────────────────────────
    // Maximum decoded size for an 18×18 maze is 178 bytes; 256 is plenty.
    let mut buf = [0u8; 256];
    let decoded_len = match base64_decode(encoded.as_bytes(), &mut buf) {
        Some(n) => n,
        None    => return false,
    };
    let data = &buf[..decoded_len];

    // ── Validate header ───────────────────────────────────────────────────
    if decoded_len < 17 { return false; }
    if &data[0..2] != b"MZ" { return false; }
    if data[2] != 1 { return false; }   // version

    let w        = data[3] as usize;
    let h        = data[4] as usize;
    let n_exits  = data[5] as usize;

    if w == 0 || h == 0 || w > 32 || h > 32 { return false; }
    if n_exits > MAX_EXITS { return false; }

    let wall_start  = 16usize;
    let cells       = w * h;
    let wall_bytes  = (cells + 1) / 2;
    if decoded_len < wall_start + wall_bytes { return false; }

    // ── Load into statics ─────────────────────────────────────────────────
    // Clear everything first.
    clear_walls();
    clear_visited();
    CHEAT_POS.store(0, Ordering::Relaxed);
    REGEN_POS.store(0, Ordering::Relaxed);
    REVEALED.store(false, Ordering::Relaxed);
    for slot in &EXITS { slot.store(0xFFFF, Ordering::Relaxed); }

    // Wall data
    for idx in 0..cells {
        let byte   = data[wall_start + idx / 2];
        let nibble = if idx % 2 == 0 { byte & 0xF } else { (byte >> 4) & 0xF };
        let r = idx / w;
        let c = idx % w;
        if r < H && c < W {
            WALLS[r * W + c].store(nibble, Ordering::Relaxed);
        }
    }

    // Exits
    let mut exit_count = 0usize;
    for i in 0..MAX_EXITS {
        let er = data[6 + i * 2];
        let ec = data[7 + i * 2];
        if er == 0xFF || exit_count >= n_exits { break; }
        if (er as usize) < H && (ec as usize) < W {
            EXITS[exit_count].store(((er as u32) << 8) | ec as u32, Ordering::Relaxed);
            exit_count += 1;
        }
    }

    // Start position
    let sr = data[14];
    let sc = data[15];
    let (start_r, start_c) = if sr != 0xFF && (sr as usize) < H && (sc as usize) < W {
        (sr as usize, sc as usize)
    } else {
        find_start_pos()
    };
    PLAYER.store(((start_r as u32) << 8) | start_c as u32, Ordering::Relaxed);
    mark_visited(start_r, start_c);

    true
}

/// Minimal base64 decoder (standard alphabet, with or without '=' padding).
/// Writes decoded bytes into `out`, returns number of bytes written or None on error.
fn base64_decode(input: &[u8], out: &mut [u8]) -> Option<usize> {
    #[inline(always)]
    fn dec(c: u8) -> u8 {
        match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+'        => 62,
            b'/'        => 63,
            _           => 0xFF,
        }
    }

    // Strip trailing '=' padding.
    let mut end = input.len();
    while end > 0 && input[end - 1] == b'=' { end -= 1; }
    let input = &input[..end];

    let mut out_pos = 0usize;
    let mut i = 0usize;

    while i + 1 < input.len() {
        let a = dec(input[i]);
        let b = dec(input[i + 1]);
        if a == 0xFF || b == 0xFF { return None; }

        if out_pos >= out.len() { return None; }
        out[out_pos] = (a << 2) | (b >> 4);
        out_pos += 1;

        if i + 2 < input.len() {
            let c = dec(input[i + 2]);
            if c == 0xFF { return None; }
            if out_pos >= out.len() { return None; }
            out[out_pos] = ((b & 0x0F) << 4) | (c >> 2);
            out_pos += 1;

            if i + 3 < input.len() {
                let d = dec(input[i + 3]);
                if d == 0xFF { return None; }
                if out_pos >= out.len() { return None; }
                out[out_pos] = ((c & 0x03) << 6) | d;
                out_pos += 1;
            }
        }

        i += 4;
    }
    Some(out_pos)
}

// ── Exit reachability validation ─────────────────────────────────────────────

/// BFS/flood-fill scratch queue — fixed size, allocated on the call stack.
/// Maximum reachable cells = CELLS = 324.  Each entry is a packed cell index
/// stored as u16.  Queue is 648 bytes on the call stack — fine.
struct Queue {
    buf: [u16; CELLS],
    head: usize,
    tail: usize,
}

impl Queue {
    fn new() -> Self { Self { buf: [0u16; CELLS], head: 0, tail: 0 } }
    fn push(&mut self, v: u16) {
        if self.tail < CELLS { self.buf[self.tail] = v; self.tail += 1; }
    }
    fn pop(&mut self) -> Option<u16> {
        if self.head == self.tail { None }
        else { let v = self.buf[self.head]; self.head += 1; Some(v) }
    }
}

/// Returns true when the new candidate exit at (`cand_row`, `cand_col`) is
/// reachable from all `already_placed` exits (slots 0..already_placed in
/// EXITS) without passing through any of those exits.
///
/// We do a single BFS from the candidate cell.  Exit cells that are already
/// placed act as blocked nodes — the BFS must reach every one of them from
/// their *neighbour*, not by entering them.  Specifically, we check that
/// each existing exit cell has at least one non-exit visited neighbour that
/// has a passage into it, which means the player can walk up to the exit
/// from a non-exit cell.
///
/// Simpler stated: flood from the candidate; every existing exit must be
/// adjacent to a visited (reachable) non-exit cell with an open passage.
fn exits_independently_reachable(cand_row: usize, cand_col: usize, already_placed: usize) -> bool {
    if already_placed == 0 {
        // First exit — nothing to validate against yet.
        return true;
    }

    // Build a quick lookup of already-placed exit positions.
    let mut exit_cells = [0u32; MAX_EXITS];
    for i in 0..already_placed {
        exit_cells[i] = EXITS[i].load(Ordering::Relaxed);
    }

    let is_existing_exit = |r: usize, c: usize| -> bool {
        let p = ((r as u32) << 8) | c as u32;
        exit_cells[..already_placed].iter().any(|&e| e == p)
    };

    // BFS from candidate, treating existing exit cells as walls (blocked).
    gen_clear(); // reuse GEN_VISITED as the BFS visited set

    let start = (cand_row * W + cand_col) as u16;
    let mut q = Queue::new();
    q.push(start);
    gen_mark(start as usize);

    while let Some(idx) = q.pop() {
        let r = idx as usize / W;
        let c = idx as usize % W;
        let walls = WALLS[r * W + c].load(Ordering::Relaxed);

        // Explore all four open passages.
        let neighbours: [(u8, usize, usize); 4] = [
            (NORTH, r.wrapping_sub(1), c),
            (EAST,  r, c + 1),
            (SOUTH, r + 1, c),
            (WEST,  r, c.wrapping_sub(1)),
        ];

        for (dir_bit, nr, nc) in neighbours {
            if nr >= H || nc >= W { continue; }
            if walls & dir_bit == 0 { continue; }           // wall blocks
            if gen_is_visited(nr * W + nc) { continue; }    // already seen
            if is_existing_exit(nr, nc) { continue; }       // treat as wall

            gen_mark(nr * W + nc);
            q.push((nr * W + nc) as u16);
        }
    }

    // Every existing exit must be reachable: at least one of its interior
    // neighbours must have been visited by the BFS.
    for i in 0..already_placed {
        let ev = exit_cells[i];
        let er = (ev >> 8) as usize;
        let ec = (ev & 0xFF) as usize;

        let exit_walls = WALLS[er * W + ec].load(Ordering::Relaxed);

        let reachable = [
            (NORTH, er.wrapping_sub(1), ec),
            (EAST,  er, ec + 1),
            (SOUTH, er + 1, ec),
            (WEST,  er, ec.wrapping_sub(1)),
        ]
        .iter()
        .any(|&(dir_bit, nr, nc)| {
            nr < H && nc < W
                && exit_walls & dir_bit != 0          // passage exists
                && gen_is_visited(nr * W + nc)        // neighbour was reached
                && !is_existing_exit(nr, nc)          // neighbour isn't another exit
        });

        if !reachable {
            return false;
        }
    }

    true
}

fn generate() {
    // Seed the PRNG.
    RNG.store(rng_seed(), Ordering::Relaxed);

    // Clear all walls and generation visited flags.
    clear_walls();
    gen_clear();
    clear_visited();
    CHEAT_POS.store(0, Ordering::Relaxed);
    REGEN_POS.store(0, Ordering::Relaxed);
    REVEALED.store(false, Ordering::Relaxed);

    // Pick a random start cell for DFS.
    let start_row = (rng_next() as usize) % H;
    let start_col = (rng_next() as usize) % W;
    let start_idx = (start_row * W + start_col) as u16;

    // Iterative DFS with a local stack (allocated on the call stack, not heap).
    let mut stack = Stack::new();
    stack.push(start_idx);
    gen_mark(start_idx as usize);

    while !stack.is_empty() {
        let idx = *stack.buf.get(stack.top.saturating_sub(1)).unwrap_or(&0) as usize;
        let row = idx / W;
        let col = idx % W;

        // Collect unvisited neighbours.
        let mut neighbours = [0u8; 4]; // direction indices 0..3
        let mut n_count = 0usize;

        // North
        if row > 0 && !gen_is_visited((row - 1) * W + col) {
            neighbours[n_count] = 0; n_count += 1;
        }
        // East
        if col + 1 < W && !gen_is_visited(row * W + col + 1) {
            neighbours[n_count] = 1; n_count += 1;
        }
        // South
        if row + 1 < H && !gen_is_visited((row + 1) * W + col) {
            neighbours[n_count] = 2; n_count += 1;
        }
        // West
        if col > 0 && !gen_is_visited(row * W + col - 1) {
            neighbours[n_count] = 3; n_count += 1;
        }

        if n_count == 0 {
            // Dead end — backtrack.
            stack.pop();
            continue;
        }

        // Choose a random unvisited neighbour.
        let choice = neighbours[(rng_next() as usize) % n_count];
        let (nrow, ncol) = match choice {
            0 => (row - 1, col),
            1 => (row, col + 1),
            2 => (row + 1, col),
            _ => (row, col - 1),
        };

        // Open wall between current and neighbour (bidirectional).
        let (fwd, rev) = dir_pair(choice);
        open_wall(row, col, fwd);
        open_wall(nrow, ncol, rev);

        let nidx = (nrow * W + ncol) as u16;
        gen_mark(nidx as usize);
        stack.push(nidx);
    }

    // ── Place exits (1–4, on the outer border) ────────────────────────────
    //
    // Exits are placed one at a time.  After each placement we do a
    // flood-fill from that exit that is *blocked* at every other already-
    // placed exit.  If the fill can reach all other exits without passing
    // through any of them, the placement is valid.  This guarantees that
    // every exit has an independent path — the player will never be forced
    // through one exit to reach another.
    let n_exits = 1 + (rng_next() % 4) as usize; // 1..=4

    // Initialise exit slots to 0xFFFF (unused).
    for slot in &EXITS {
        slot.store(0xFFFF, Ordering::Relaxed);
    }

    let mut placed = 0usize;
    let mut attempts = 0usize;

    while placed < n_exits && attempts < 512 {
        attempts += 1;

        // Pick a random border segment and position.
        let side = (rng_next() % 4) as usize;
        let pos  = (rng_next() as usize) % (if side < 2 { W } else { H });

        let (row, col) = match side {
            0 => (0, pos),       // top row
            1 => (H - 1, pos),   // bottom row
            2 => (pos, 0),       // left col
            _ => (pos, W - 1),   // right col
        };

        let out_dir = match side {
            0 => NORTH,
            1 => SOUTH,
            2 => WEST,
            _ => EAST,
        };

        // Reject if this cell is already an exit.
        let packed = ((row as u32) << 8) | col as u32;
        if EXITS.iter().any(|s| s.load(Ordering::Relaxed) == packed) {
            continue;
        }

        // Tentatively open the exit wall so flood-fill sees the opening.
        open_wall(row, col, out_dir);

        // Validate: every previously placed exit must be reachable from
        // this candidate WITHOUT stepping through any other exit.
        // We flood-fill from the candidate cell, treating all other
        // already-placed exit cells as impassable walls.
        let valid = exits_independently_reachable(row, col, placed);

        if valid {
            EXITS[placed].store(packed, Ordering::Relaxed);
            placed += 1;
        } else {
            // Undo the wall opening — close the wall again.
            WALLS[row * W + col].fetch_and(!out_dir, Ordering::Relaxed);
        }
    }

    // ── Place player: random interior cell far from all exits ─────────────
    // "Far" = Manhattan distance ≥ 5 from every exit. Try up to 128 times.
    let player_pos = find_start_pos();
    let (pr, pc) = player_pos;
    PLAYER.store(((pr as u32) << 8) | pc as u32, Ordering::Relaxed);
    mark_visited(pr, pc);
}

/// Returns (NORTH bit, reverse bit) for a direction index 0=N,1=E,2=S,3=W.
fn dir_pair(dir: u8) -> (u8, u8) {
    match dir {
        0 => (NORTH, SOUTH),
        1 => (EAST, WEST),
        2 => (SOUTH, NORTH),
        _ => (WEST, EAST),
    }
}

/// Find a good starting position for the player.
fn find_start_pos() -> (usize, usize) {
    for _ in 0..128 {
        let row = 1 + (rng_next() as usize) % (H - 2);
        let col = 1 + (rng_next() as usize) % (W - 2);

        // Check minimum distance from every exit.
        let far_enough = EXITS.iter().all(|slot| {
            let ev = slot.load(Ordering::Relaxed);
            if ev == 0xFFFF { return true; } // unused slot
            let er = (ev >> 8) as usize;
            let ec = (ev & 0xFF) as usize;
            let dist = row.abs_diff(er) + col.abs_diff(ec);
            dist >= 5
        });

        if far_enough {
            return (row, col);
        }
    }
    // Fallback: centre.
    (H / 2, W / 2)
}

// ── Drawing ───────────────────────────────────────────────────────────────────

pub fn draw<D>(display: &mut D) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    // Clear to white.
    Rectangle::new(Point::zero(), Size::new(152, 152))
        .into_styled(PrimitiveStyle::with_fill(WHITE))
        .draw(display)?;

    if WON.load(Ordering::Relaxed) {
        return draw_win_screen(display);
    }

    let packed = PLAYER.load(Ordering::Relaxed);
    let player_row = (packed >> 8) as usize;
    let player_col = (packed & 0xFF) as usize;

    // If the cheat is active, mark every cell as visited before drawing.
    if REVEALED.load(Ordering::Relaxed) {
        for r in 0..H {
            for c in 0..W {
                mark_visited(r, c);
            }
        }
    }

    // Draw each cell.
    for row in 0..H {
        for col in 0..W {
            let px = BORDER + (col as i32) * CELL;
            let py = BORDER + (row as i32) * CELL;

            let visited = is_visited(row, col);

            if !visited {
                // Unvisited: draw solid black (fog of war).
                Rectangle::new(Point::new(px, py), Size::new(CELL as u32, CELL as u32))
                    .into_styled(PrimitiveStyle::with_fill(BLACK))
                    .draw(display)?;
                continue;
            }

            // Visited cell: draw floor (white) with walls.
            // Floor is already white from the background clear.


            // Draw walls as black edges only where a wall is CLOSED.
            let walls = WALLS[row * W + col].load(Ordering::Relaxed);

            let wall_style = PrimitiveStyle::with_fill(BLACK);

            // North wall (top edge of cell).
            if walls & NORTH == 0 {
                Rectangle::new(Point::new(px, py), Size::new(CELL as u32, 1))
                    .into_styled(wall_style)
                    .draw(display)?;
            }
            // South wall (bottom edge of cell).
            if walls & SOUTH == 0 {
                Rectangle::new(Point::new(px, py + CELL - 1), Size::new(CELL as u32, 1))
                    .into_styled(wall_style)
                    .draw(display)?;
            }
            // West wall (left edge of cell).
            if walls & WEST == 0 {
                Rectangle::new(Point::new(px, py), Size::new(1, CELL as u32))
                    .into_styled(wall_style)
                    .draw(display)?;
            }
            // East wall (right edge of cell).
            if walls & EAST == 0 {
                Rectangle::new(Point::new(px + CELL - 1, py), Size::new(1, CELL as u32))
                    .into_styled(wall_style)
                    .draw(display)?;
            }
        }
    }

    // Draw the outer border walls (4 px thick black border around the maze).
    // Top border.
    Rectangle::new(Point::new(0, 0), Size::new(152, BORDER as u32))
        .into_styled(PrimitiveStyle::with_fill(BLACK))
        .draw(display)?;
    // Bottom border.
    Rectangle::new(Point::new(0, 152 - BORDER), Size::new(152, BORDER as u32))
        .into_styled(PrimitiveStyle::with_fill(BLACK))
        .draw(display)?;
    // Left border.
    Rectangle::new(Point::new(0, 0), Size::new(BORDER as u32, 152))
        .into_styled(PrimitiveStyle::with_fill(BLACK))
        .draw(display)?;
    // Right border.
    Rectangle::new(Point::new(152 - BORDER, 0), Size::new(BORDER as u32, 152))
        .into_styled(PrimitiveStyle::with_fill(BLACK))
        .draw(display)?;

    // Draw exit gaps in the border — only if the player has visited the exit cell.
    for slot in &EXITS {
        let ev = slot.load(Ordering::Relaxed);
        if ev == 0xFFFF { continue; }
        let er = (ev >> 8) as usize;
        let ec = (ev & 0xFF) as usize;

        // Only reveal the exit once the player has stood on that border cell.
        if !is_visited(er, ec) { continue; }

        let walls = WALLS[er * W + ec].load(Ordering::Relaxed);
        let px = BORDER + (ec as i32) * CELL;
        let py = BORDER + (er as i32) * CELL;

        if walls & NORTH != 0 && er == 0 {
            Rectangle::new(Point::new(px + 1, 0), Size::new((CELL - 2) as u32, BORDER as u32))
                .into_styled(PrimitiveStyle::with_fill(crate::RED))
                .draw(display)?;
        }
        if walls & SOUTH != 0 && er == H - 1 {
            Rectangle::new(Point::new(px + 1, 152 - BORDER), Size::new((CELL - 2) as u32, BORDER as u32))
                .into_styled(PrimitiveStyle::with_fill(crate::RED))
                .draw(display)?;
        }
        if walls & WEST != 0 && ec == 0 {
            Rectangle::new(Point::new(0, py + 1), Size::new(BORDER as u32, (CELL - 2) as u32))
                .into_styled(PrimitiveStyle::with_fill(crate::RED))
                .draw(display)?;
        }
        if walls & EAST != 0 && ec == W - 1 {
            Rectangle::new(Point::new(152 - BORDER, py + 1), Size::new(BORDER as u32, (CELL - 2) as u32))
                .into_styled(PrimitiveStyle::with_fill(crate::RED))
                .draw(display)?;
        }
    }

    // Draw player (solid red 4×4 square centred in cell).
    {
        let px = BORDER + (player_col as i32) * CELL;
        let py = BORDER + (player_row as i32) * CELL;
        let ps = (CELL / 2) as u32; // 4px player square
        let offset = (CELL - ps as i32) / 2;
        Rectangle::new(
            Point::new(px + offset, py + offset),
            Size::new(ps, ps),
        )
        .into_styled(PrimitiveStyle::with_fill(crate::RED))
        .draw(display)?;
    }

    Ok(())
}

/// Draw the win / escape screen.
fn draw_win_screen<D>(display: &mut D) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let centered = TextStyleBuilder::new()
        .baseline(Baseline::Middle)
        .alignment(Alignment::Center)
        .build();
    let font_big = MonoTextStyle::new(&FONT_7X13_BOLD, BLACK);

    let steps = STEPS.load(Ordering::Relaxed);

    Text::with_text_style("You escaped!", Point::new(76, 55), font_big, centered)
        .draw(display)?;

    let mut buf: heapless::String<32> = heapless::String::new();
    let _ = core::fmt::Write::write_fmt(&mut buf, format_args!("Steps: {}", steps));
    Text::with_text_style(buf.as_str(), Point::new(76, 80), font_big, centered)
        .draw(display)?;

    Text::with_text_style("Press any button", Point::new(76, 110), font_big, centered)
        .draw(display)?;

    Ok(())
}

// ── Integration notes (for the developer) ─────────────────────────────────────
//
// 1.  src/game/mod.rs — add to the `pub mod` list:
//         pub mod maze;
//
//     In `draw_screen_game`, add BEFORE the tictactoe check:
//         if maze::is_active() {
//             return maze::draw(display);
//         }
//
// 2.  src/game/input.rs — add a new block BEFORE the lightsout block:
//         if super::maze::is_active() {
//             match btn {
//                 ButtonId::Cancel  => super::maze::close(),
//                 ButtonId::Up      => super::maze::move_up(),
//                 ButtonId::Down    => super::maze::move_down(),
//                 ButtonId::Left    => super::maze::move_left(),
//                 ButtonId::Right   => super::maze::move_right(),
//                 ButtonId::Fire | ButtonId::Execute => super::maze::activate(),
//             }
//             return true;
//         }
//
// 3.  src/game/modal.rs — in `ModalKind::items()` for `Self::Play`:
//         Self::Play => &["Play now", "Tic Tac Toe", "Lights Out", "Maze", "Play music", "Cancel"],
//
//     In the `activate_item` match, add:
//         "Maze" => { super::maze::open(); close(); }