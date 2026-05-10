//! Tiny B&W emoji atlas for the message-display surfaces.
//!
//! The badge's text fonts (`FONT_7X13` / `FONT_7X13_BOLD` from
//! `embedded-graphics`) only cover ISO-8859-1; SMP emoji codepoints are
//! invisible to them.  This module supplies a hand-drawn 13×13 1-bit
//! bitmap for every supported emoji and wires it into the existing
//! monospaced grid by claiming **2 character columns per emoji** —
//! the surrounding text stays aligned.
//!
//! ## Aliasing
//!
//! ~30 popular emoji codepoints from the upstream MeshCore client list
//! collapse to **9 visual archetypes**.  At 13×13 monochrome there is
//! no perceptible difference between e.g. `❤` and `♥`, or between `😂`
//! and `🤣`; both render to the same heart / laugh bitmap.
//!
//! ## Variation selectors
//!
//! Per <https://www.codejam.info/2021/11/emoji-variation-selector.html>,
//! `U+FE0E` (text style) and `U+FE0F` (emoji style) modify the previous
//! codepoint.  We always render the same monochrome glyph regardless,
//! so [`decode_with_emojis`] silently consumes either selector when it
//! immediately follows a known emoji codepoint.

use core::iter::Peekable;
use core::str::Chars;

use embedded_graphics::{
    Pixel,
    geometry::Point,
    mono_font::MonoTextStyle,
    pixelcolor::PixelColor,
    prelude::*,
    text::{Baseline, Text, TextStyle},
};

/// Width / height of every emoji bitmap, in pixels.
pub const EMOJI_PX: usize = 13;
/// Bytes per row in a packed glyph (`ceil(13 / 8) = 2`).
const ROW_BYTES: usize = 2;
/// Bytes per glyph (`ROW_BYTES * EMOJI_PX`).
const GLYPH_BYTES: usize = ROW_BYTES * EMOJI_PX;

/// Horizontal advance to use when laying out an emoji on a `FONT_7X13`
/// grid: **2 character cells = 14 px**.  Keeps the monospaced text grid
/// intact — word-wrap and right-alignment math count each emoji as 2
/// characters via [`column_width`].
pub const EMOJI_ADVANCE_PX: i32 = 14;
/// Number of text columns occupied by one emoji.
pub const EMOJI_COLUMNS: usize = 2;

// ---------------------------------------------------------------------------
// Glyph packing helper
// ---------------------------------------------------------------------------

/// Pack a 13-row × 13-column `&str` stencil (`#` = on, anything else = off)
/// into a flat byte array.  Bit 7 of each byte is the leftmost pixel.
/// Evaluated entirely at compile time.
const fn pack_glyph(rows: [&str; EMOJI_PX]) -> [u8; GLYPH_BYTES] {
    let mut out = [0u8; GLYPH_BYTES];
    let mut row = 0;
    while row < EMOJI_PX {
        let bytes = rows[row].as_bytes();
        let mut col = 0;
        while col < EMOJI_PX && col < bytes.len() {
            if bytes[col] == b'#' {
                out[row * ROW_BYTES + col / 8] |= 0x80 >> (col % 8);
            }
            col += 1;
        }
        row += 1;
    }
    out
}

// ---------------------------------------------------------------------------
// Atlas — 9 archetype bitmaps
// ---------------------------------------------------------------------------
//
// Refine the pixel art via the ASCII stencils below; they are the source
// of truth.  Keep each row exactly 13 chars wide.

const HEART: [u8; GLYPH_BYTES] = pack_glyph([
    ".............",
    "....##.##....",
    "...#######...",
    "..#########..",
    "..#########..",
    "..#########..",
    "...#######...",
    "....#####....",
    ".....###.....",
    "......#......",
    ".............",
    ".............",
    ".............",
]);

const SMILE: [u8; GLYPH_BYTES] = pack_glyph([
    ".....###.....",
    "...##...##...",
    "..#.......#..",
    ".#..##.##..#.",
    ".#..##.##..#.",
    "#...........#",
    "#...........#",
    "#...........#",
    "#.#.......#.#",
    ".#..#####..#.",
    "..#.......#..",
    "...##...##...",
    ".....###.....",
]);

const LAUGH: [u8; GLYPH_BYTES] = pack_glyph([
    ".....###.....",
    "...##...##...",
    "..#.......#..",
    ".#..#####..#.",
    ".#.#######.#.",
    "#...........#",
    "#...........#",
    "#.##.....##.#",
    "#.##.....##.#",
    ".#.#######.#.",
    "..#.#####.#..",
    "...##...##...",
    ".....###.....",
]);

const CRY: [u8; GLYPH_BYTES] = pack_glyph([
    ".....###.....",
    "...##...##...",
    "..#.......#..",
    ".#..##.##..#.",
    "##..##.##..##",
    "##.........##",
    "##.........##",
    ".#.........#.",
    ".#..#####..#.",
    "..#.#####.#..",
    "...##...##...",
    ".....###.....",
    ".............",
]);

const KISS: [u8; GLYPH_BYTES] = pack_glyph([
    ".....###.....",
    "...##...##...",
    "..#.......#..",
    ".#..##.##..#.",
    ".#..##.##..#.",
    "#...........#",
    "#...........#",
    "#....#.#....#",
    "#...#####...#",
    ".#...###...#.",
    "..#...#...#..",
    "...##...##...",
    ".....###.....",
]);

const THUMBS: [u8; GLYPH_BYTES] = pack_glyph([
    ".......#.....",
    "......##.....",
    "......##.....",
    ".....###.....",
    "..##.####....",
    "..##.######..",
    "..##.#######.",
    "..##.#######.",
    "..##########.",
    "..##########.",
    "..##########.",
    "...########..",
    ".............",
]);

const FIRE: [u8; GLYPH_BYTES] = pack_glyph([
    "......#......",
    "......##.....",
    ".....#.##....",
    "....#..##....",
    "....#..#.#...",
    "...#..#..#...",
    "...#.#...##..",
    "..#.#.....#..",
    "..#.......##.",
    "..##.....##..",
    "..##.....##..",
    "...#######...",
    "....#####....",
]);

const THINKING: [u8; GLYPH_BYTES] = pack_glyph([
    ".....###.....",
    "...##...##...",
    "..#.......#..",
    ".#.##...##.#.",
    ".#.#.....#.#.",
    "#...........#",
    "#...........#",
    "#...####.##.#",
    "#..#.....#..#",
    ".#.#.....##..",
    "..#......#...",
    "...##...##...",
    ".....###.....",
]);

const PRAY: [u8; GLYPH_BYTES] = pack_glyph([
    "......#......",
    ".....#.#.....",
    "....#...#....",
    "....#...#....",
    "...#.....#...",
    "...#.....#...",
    "..#.......#..",
    ".##.......##.",
    ".##.......##.",
    "##.........##",
    "##.........##",
    ".#########.#.",
    "..#######....",
]);

// 📎 — paperclip ("clippy").
const CLIPPY: [u8; GLYPH_BYTES] = pack_glyph([
    "..######.....",
    ".##....#.....",
    "##.....#.....",
    "##.....#.....",
    "##.....#.....",
    "##.....#.....",
    "##.....#.....",
    "##....##.....",
    "##....##.....",
    ".##..###.....",
    "..####.#.....",
    ".......#.....",
    ".......#.....",
]);

// 🗑 — wastebasket ("ranzbak").
const TRASH: [u8; GLYPH_BYTES] = pack_glyph([
    ".............",
    "....#####....",
    "..#########..",
    ".############",
    ".###########.",
    ".##.##.##.##.",
    ".##.##.##.##.",
    ".##.##.##.##.",
    ".##.##.##.##.",
    ".##.##.##.##.",
    ".##.##.##.##.",
    ".##.##.##.##.",
    ".############",
]);

// 🐹 — hamster / cuy.  Round body with two pointed ears + face.
const CUY: [u8; GLYPH_BYTES] = pack_glyph([
    ".............",
    "..#.......#..",
    ".###.....###.",
    ".####...####.",
    ".############",
    ".##.#####.##.",
    ".##.#####.##.",
    ".#####.#####.",
    ".############",
    ".############",
    ".############",
    "..#########..",
    "...##...##...",
]);

// 🦙 / 🐫 / 🐪 — llama / camel silhouette.  Long upright neck on
// the left, body + 4 legs on the right.
const LLAMA: [u8; GLYPH_BYTES] = pack_glyph([
    ".............",
    "..####.......",
    ".######......",
    ".######......",
    ".##.##.......",
    ".#..##.......",
    "....##.......",
    "....##.######",
    "....##########",
    "....##########",
    "....##.##.##.",
    "....##.##.##.",
    "....##.##.##.",
]);

// ⌛ / ⏱ / 🕜 / 🕝 / 🕞 / 🕡 / 🕢 / 🕤 / 🕥 / 🕦 / 🕧 —
// generic clock face (circle outline + hour hand up + minute hand right).
// All "clock at half past hour" + hourglass + stopwatch alias to this.
const CLOCK: [u8; GLYPH_BYTES] = pack_glyph([
    ".....###.....",
    "...##...##...",
    "..#.......#..",
    ".#....#....#.",
    ".#....#....#.",
    "#.....#.....#",
    "#.....#######",
    "#...........#",
    "#...........#",
    ".#.........#.",
    "..#.......#..",
    "...##...##...",
    ".....###.....",
]);

// ❌ — heavy cross mark.
const CROSS: [u8; GLYPH_BYTES] = pack_glyph([
    ".............",
    ".#.........#.",
    ".##.......##.",
    "..##.....##..",
    "...##...##...",
    "....##.##....",
    ".....###.....",
    "....##.##....",
    "...##...##...",
    "..##.....##..",
    ".##.......##.",
    ".#.........#.",
    ".............",
]);

// 💯 — "100" with double underline.
const HUNDRED: [u8; GLYPH_BYTES] = pack_glyph([
    ".............",
    ".............",
    ".#..####.####",
    "##..#..#.#..#",
    ".#..#..#.#..#",
    ".#..#..#.#..#",
    ".#..#..#.#..#",
    ".#..####.####",
    ".............",
    ".###########.",
    ".###########.",
    ".............",
    ".............",
]);

// 😮 — face with open round mouth.
const OPEN_MOUTH: [u8; GLYPH_BYTES] = pack_glyph([
    ".....###.....",
    "...##...##...",
    "..#.......#..",
    ".#..##.##..#.",
    ".#..##.##..#.",
    "#...........#",
    "#....###....#",
    "#...#...#...#",
    "#....###....#",
    ".#.........#.",
    "..#.......#..",
    "...##...##...",
    ".....###.....",
]);

// 😴 — sleeping face (closed eyes shown as horizontal lines).
const SLEEPING: [u8; GLYPH_BYTES] = pack_glyph([
    ".....###.....",
    "...##...##...",
    "..#.......#..",
    ".#.........#.",
    ".#.####.####.",
    ".#.####.####.",
    "#...........#",
    "#...........#",
    "#...........#",
    ".#.........#.",
    "..#..###..#..",
    "...##...##...",
    ".....###.....",
]);

// 😶 — face without mouth (eyes + blank).
const NO_MOUTH: [u8; GLYPH_BYTES] = pack_glyph([
    ".....###.....",
    "...##...##...",
    "..#.......#..",
    ".#..##.##..#.",
    ".#..##.##..#.",
    "#...........#",
    "#...........#",
    "#...........#",
    "#...........#",
    ".#.........#.",
    "..#.......#..",
    "...##...##...",
    ".....###.....",
]);

// 🙃 — upside-down smiling face.  Mouth on top, eyes on bottom.
const UPSIDE_DOWN: [u8; GLYPH_BYTES] = pack_glyph([
    ".....###.....",
    "...##...##...",
    "..#..###..#..",
    ".#.........#.",
    "#...........#",
    "#...........#",
    "#...........#",
    "#.##.....##.#",
    ".#..##.##..#.",
    ".#..##.##..#.",
    "..#.......#..",
    "...##...##...",
    ".....###.....",
]);

// ✅ / ✔️ — check mark.
const CHECK: [u8; GLYPH_BYTES] = pack_glyph([
    ".............",
    ".............",
    "............#",
    "...........##",
    "..........##.",
    ".........##..",
    "#.......##...",
    "##.....##....",
    ".##...##.....",
    "..##.##......",
    "...###.......",
    "....#........",
    ".............",
]);

// 🎵 — musical note (single eighth note).
const NOTE: [u8; GLYPH_BYTES] = pack_glyph([
    ".........###.",
    "........#####",
    "........#####",
    "........#####",
    "........###..",
    "......#.#....",
    "....##..#....",
    "..##....#....",
    ".#......#....",
    "#.......#....",
    "##......#....",
    ".##....##....",
    "..######.....",
]);

// 😎 — smiling face with sunglasses.  Solid eye-bar replaces eyes.
const COOL: [u8; GLYPH_BYTES] = pack_glyph([
    ".....###.....",
    "...##...##...",
    "..#.......#..",
    ".############",
    ".############",
    "##############",
    ".##.#####.##.",
    ".##.#####.##.",
    "#.##.....##.#",
    ".#.........#.",
    "..#.#####.#..",
    "...##...##...",
    ".....###.....",
]);

// ⭐ — 5-pointed star.
const STAR: [u8; GLYPH_BYTES] = pack_glyph([
    "......#......",
    ".....###.....",
    ".....###.....",
    ".....###.....",
    "#############",
    ".###########.",
    "..#########..",
    "...#######...",
    "....#####....",
    "...#######...",
    "..##.....##..",
    ".##.......##.",
    ".............",
]);

// 👾 — alien monster (classic Space Invader silhouette).
const ALIEN: [u8; GLYPH_BYTES] = pack_glyph([
    "..#.......#..",
    "...#.....#...",
    "..#########..",
    ".##.#####.##.",
    "###.#####.###",
    "#############",
    "##.#######.##",
    "##.#.....#.##",
    "...##...##...",
    ".#..#...#..#.",
    "#.#...#...#.#",
    "#...........#",
    ".............",
]);

// 🔆 — high brightness / sun with rays.
const SUN: [u8; GLYPH_BYTES] = pack_glyph([
    "......#......",
    "..#...#...#..",
    "...#..#..#...",
    "....#####....",
    "...#######...",
    "...#######...",
    ".#.#######.#.",
    "##.#######.##",
    ".#.#######.#.",
    "...#######...",
    "....#####....",
    "...#..#..#...",
    "..#...#...#..",
]);

// 🎉 + 🥳 — celebration / party popper.  Center burst with confetti
// dots spraying outward; reads as "party" at 13×13.
const PARTY: [u8; GLYPH_BYTES] = pack_glyph([
    ".............",
    "......#......",
    "....#.#.#....",
    "...#.....#...",
    "....#.#.#....",
    "......#......",
    ".....#.#.....",
    "....#.#.#....",
    "...#.#.#.#...",
    "..#.#####.#..",
    ".##.......##.",
    "##.........##",
    ".............",
]);

// 😇 — smiling face with a halo on top.  Halo eats the top two rows
// of the cell; smiley compressed into the lower 10 rows.
const HALO: [u8; GLYPH_BYTES] = pack_glyph([
    "....#####....",
    "...#.....#...",
    "....#####....",
    "..#.......#..",
    ".#..##.##..#.",
    ".#..##.##..#.",
    "#...........#",
    "#...........#",
    "#...........#",
    ".#.........#.",
    "..#..###..#..",
    "...##...##...",
    ".....###.....",
]);

// 🚀 — rocket.  Pointed nose, fuselage, flame at the bottom.
const ROCKET: [u8; GLYPH_BYTES] = pack_glyph([
    "......#......",
    ".....###.....",
    ".....#.#.....",
    "....#####....",
    "....#.#.#....",
    "....#.#.#....",
    "....#####....",
    "....#.#.#....",
    "...#.....#...",
    "..##..#..##..",
    "..#..#.#..#..",
    "....##.##....",
    ".....#.#.....",
]);

// 🥺 — pleading face.  Oversized teary eyes, small worried mouth.
const PLEADING: [u8; GLYPH_BYTES] = pack_glyph([
    ".....###.....",
    "...##...##...",
    "..#.......#..",
    ".#..###.###.#",
    ".#.#####.####",
    "#..#####.####",
    "#..#####.####",
    "#..#####.####",
    "#...###.###.#",
    ".#.........#.",
    "..#..###..#..",
    "...##...##...",
    ".....###.....",
]);

// 😈 — smiling face with horns (devil counterpart to 😇).
const DEVIL: [u8; GLYPH_BYTES] = pack_glyph([
    "...#.....#...",
    "...##...##...",
    "....##.##....",
    "....#####....",
    "...#######...",
    "..#.#####.#..",
    ".#..##.##..#.",
    ".#..##.##..#.",
    "#...........#",
    "#...........#",
    ".#.........#.",
    "..#..###..#..",
    "...##...##...",
]);

// 🥚 — the Cyber Aegg badge mascot.
const EGG: [u8; GLYPH_BYTES] = pack_glyph([
    ".....###.....",
    "....#####....",
    "....#####....",
    "...#######...",
    "...#######...",
    "..#########..",
    "..#########..",
    "..#########..",
    "..#######.#..",
    "..#######.#..",
    "...####..#...",
    "....#####....",
    ".............",
]);

// 🧥 — coat / jacket.  Small V-neck collar on top, a solid shoulder
// line spans the full width, arms hang from the shoulder line at the
// outer edges with a one-pixel gap separating them from the body
// walls (so the sleeves read as distinct from the torso), single
// button column down the middle, A-line taper to a narrower hem.
const JACKET: [u8; GLYPH_BYTES] = pack_glyph([
    ".....###.....",
    "....##.##....",
    ".###########.",
    "##.#.....#.##",
    "##.#.....#.##",
    "##.#..#..#.##",
    "##.#..#..#.##",
    "##.#..#..#.##",
    "##.#..#..#.##",
    ".#.#.....#.#.",
    "..##.....##..",
    "...#.....#...",
    "...#######...",
]);

// 🎒 — school backpack.  Carry-loop on top, straps fan out to the
// shoulders of the main body, centred front pocket.
const BACKPACK: [u8; GLYPH_BYTES] = pack_glyph([
    "......#......",
    ".....###.....",
    "....##.##....",
    "...##...##...",
    "..##.....##..",
    "#############",
    "#...........#",
    "#...#####...#",
    "#...#...#...#",
    "#...#####...#",
    "#...........#",
    "#...........#",
    "#############",
]);

// 🍓 — strawberry.  Stem on top, calyx leaves spreading with finger
// gaps, body with checker-pattern seeds, body tapers to a point.
const STRAWBERRY: [u8; GLYPH_BYTES] = pack_glyph([
    "......#......",
    "....#####....",
    "...#######...",
    "....##.##....",
    ".###########.",
    "##.#.#.#.#.##",
    "#.#.#.#.#.#.#",
    "##.#.#.#.#.##",
    ".##.......##.",
    "..##.....##..",
    "...##...##...",
    "....#####....",
    "......#......",
]);

// 🦊 — fox face.  Pointy ears that taper to a single pixel at the top
// and widen down to where they merge with the head, triangular head
// with two eyes, snout/jaw tapering to a chin point.  Doubles as 👿
// imp / angry face with horns — the pointy ears read as horns.
const FOX: [u8; GLYPH_BYTES] = pack_glyph([
    ".#.........#.",
    "##.........##",
    "###.......###",
    "####.....####",
    ".###########.",
    ".#..#...#..#.",
    ".#.........#.",
    ".#....#....#.",
    "..#.#####.#..",
    "...#######...",
    "....#####....",
    ".....###.....",
    "......#......",
]);

// 👀 — eyes.  Two eye outlines floating in the centre of the cell with
// pupils as a single column of pixels.
const EYES: [u8; GLYPH_BYTES] = pack_glyph([
    ".............",
    ".............",
    ".............",
    ".............",
    ".####...####.",
    "##.##...##.##",
    "#.#.#...#.#.#",
    "##.##...##.##",
    ".####...####.",
    ".............",
    ".............",
    ".............",
    ".............",
]);

// ⚠ — warning sign.  Outline of an equilateral triangle pointing up,
// with a tall exclamation mark (body + dot) drawn down the middle.
const WARNING: [u8; GLYPH_BYTES] = pack_glyph([
    "......#......",
    ".....#.#.....",
    "....#...#....",
    "....#.#.#....",
    "...#..#..#...",
    "...#..#..#...",
    "..#...#...#..",
    "..#...#...#..",
    ".#....#....#.",
    ".#.........#.",
    ".#....#....#.",
    ".###########.",
    ".............",
]);

// ⚡ — high-voltage lightning bolt.  Z-shape going from upper-right to
// lower-left with a horizontal kink in the middle.
const LIGHTNING: [u8; GLYPH_BYTES] = pack_glyph([
    ".............",
    ".......##....",
    "......##.....",
    ".....##......",
    "....##.......",
    "...########..",
    "......##.....",
    ".....##......",
    "....##.......",
    "...##........",
    "..##.........",
    ".##..........",
    "##...........",
]);

// 💀 — skull face.  Rounded dome at the top, eye sockets dug into the
// cheekbones, small nose ridge, jaw with a row of teeth.
const SKULL: [u8; GLYPH_BYTES] = pack_glyph([
    ".............",
    "....#####....",
    "...#######...",
    "..#########..",
    ".##.##.##.##.",
    "##.#######.##",
    "##.#######.##",
    "##.........##",
    "##....#....##",
    "##....#....##",
    ".###########.",
    ".##.#.#.#.##.",
    "...#######...",
]);

// 👻 — ghost.  Round head outline with two eye dots and a mouth dot,
// scalloped wavy bottom.
const GHOST: [u8; GLYPH_BYTES] = pack_glyph([
    ".............",
    ".....###.....",
    "....#...#....",
    "...#.....#...",
    "..#.......#..",
    ".#.........#.",
    ".#..#...#..#.",
    ".#.........#.",
    ".#.........#.",
    ".#....#....#.",
    ".#.........#.",
    ".#.........#.",
    ".##.##.##.##.",
]);

// 🤖 — robot face.  Antenna on top, rectangular head with two square
// eyes and a speaker-grille mouth, neck + shoulder hint below.
const ROBOT: [u8; GLYPH_BYTES] = pack_glyph([
    "......#......",
    "....#####....",
    ".###########.",
    ".#.##...##.#.",
    ".#.##...##.#.",
    ".#.........#.",
    ".##.#.#.#.##.",
    ".#.........#.",
    ".###########.",
    ".....###.....",
    ".###########.",
    "##.........##",
    "##.........##",
]);

// ☕ — hot beverage.  Two wavy steam wisps rising over a mug with a
// distinct handle loop (3×3 with a 1-pixel hole) protruding to the
// right of the cup body.
const COFFEE: [u8; GLYPH_BYTES] = pack_glyph([
    ".....#.#.....",
    ".....#.#.....",
    "....##.##....",
    ".....#.#.....",
    ".............",
    ".##########..",
    ".#........#..",
    ".#........###",
    ".#........#.#",
    ".#........###",
    ".#........#..",
    ".##########..",
    ".............",
]);

// 🍕 — pizza slice.  Filled triangular wedge pointing up; pepperoni
// rendered as round negative-space cut-outs inside the cheese; a
// slightly narrower crust strip along the bottom edge.
const PIZZA: [u8; GLYPH_BYTES] = pack_glyph([
    "......#......",
    ".....###.....",
    "....#####....",
    "...##.#.##...",
    "...#######...",
    "..####.####..",
    "..#########..",
    ".####.#.####.",
    ".###########.",
    "####.#.#.####",
    "#############",
    ".###########.",
    "...#######...",
]);

// 🌭 — hot dog.  Horizontal sausage in a bun with a mustard zigzag
// down the centre.
const HOTDOG: [u8; GLYPH_BYTES] = pack_glyph([
    ".............",
    ".............",
    ".............",
    ".###########.",
    ".#####.#####.",
    ".#.........#.",
    ".#.#######.#.",
    ".#.#.#.#.#.#.",
    ".#.#######.#.",
    ".#.........#.",
    ".#####.#####.",
    ".###########.",
    ".............",
]);

// 🦄 — unicorn face.  Tall pointy horn rising above the head, wide
// ear flares on either side of the head, snout tapering downward.
const UNICORN: [u8; GLYPH_BYTES] = pack_glyph([
    "......#......",
    ".....###.....",
    ".....#.#.....",
    "....##.##....",
    "....#####....",
    "...#######...",
    "####.....####",
    "##.#######.##",
    "##.#######.##",
    "##..#####..##",
    ".###########.",
    "..#########..",
    "....#####....",
]);

// 🐉 — dragon head.  Curved horns reaching out at the top, broad
// snout with eyes + nostrils, tapering chin underneath.
const DRAGON: [u8; GLYPH_BYTES] = pack_glyph([
    "##.........##",
    "##.........##",
    "###.......###",
    "####.....####",
    ".###########.",
    "##.#.###.#.##",
    "#...#####...#",
    "#...#.#.#...#",
    "#.##.....##.#",
    "##.#######.##",
    ".###########.",
    "..#########..",
    "....#####....",
]);

// 🧙 — wizard.  Tall pointed wizard's hat with a wide brim, oval
// face below the brim with two eye dots, long downward-pointing
// beard with a split tip.
const WIZARD: [u8; GLYPH_BYTES] = pack_glyph([
    "......#......",
    ".....###.....",
    "....#####....",
    "...#######...",
    ".###########.",
    ".............",
    "....#####....",
    "...#.#.#.#...",
    "...#######...",
    "....#####....",
    "...##.#.##...",
    "..##.....##..",
    ".##.......##.",
]);

// 🔔 — bell.  Hanging loop at the top, bell body widening from
// narrow at the shoulder to a full-width rim, small triangular
// clapper poking out below the rim.
const BELL: [u8; GLYPH_BYTES] = pack_glyph([
    "......#......",
    ".....#.#.....",
    "......#......",
    ".....###.....",
    "....#...#....",
    "...#.....#...",
    "...#.....#...",
    "..#.......#..",
    "..#.......#..",
    ".#.........#.",
    ".###########.",
    ".....###.....",
    "......#......",
]);

// ✉ — envelope.  Rectangle outline with the back flap drawn as a
// V pointing down from the upper corners to a vertex in the middle,
// blank body below.
const ENVELOPE: [u8; GLYPH_BYTES] = pack_glyph([
    ".............",
    ".###########.",
    ".#.........#.",
    ".##.......##.",
    ".#.##...##.#.",
    ".#..##.##..#.",
    ".#...###...#.",
    ".#.........#.",
    ".#.........#.",
    ".#.........#.",
    ".#.........#.",
    ".###########.",
    ".............",
]);

/// All atlas bitmaps in their fixed index order.  Indices are referenced
/// by [`EMOJI_LOOKUP`].
const ATLAS: [&[u8; GLYPH_BYTES]; 50] = [
    &HEART,       // 0
    &SMILE,       // 1
    &LAUGH,       // 2
    &CRY,         // 3
    &KISS,        // 4
    &THUMBS,      // 5
    &FIRE,        // 6
    &THINKING,    // 7
    &PRAY,        // 8
    &EGG,         // 9
    &PARTY,       // 10  🎉 / 🥳
    &HALO,        // 11  😇
    &ROCKET,      // 12  🚀
    &PLEADING,    // 13  🥺
    &DEVIL,       // 14  😈
    &SUN,         // 15  🔆 / ☀
    &ALIEN,       // 16  👾
    &CHECK,       // 17  ✅ / ✔
    &NOTE,        // 18  🎵
    &COOL,        // 19  😎
    &STAR,        // 20  ⭐
    &CROSS,       // 21  ❌
    &HUNDRED,     // 22  💯
    &OPEN_MOUTH,  // 23  😮
    &SLEEPING,    // 24  😴
    &NO_MOUTH,    // 25  😶
    &UPSIDE_DOWN, // 26  🙃
    &CLOCK,       // 27  ⌛ / ⏱ / 🕜.. (all clocks + hourglass + stopwatch)
    &CUY,         // 28  🐹
    &LLAMA,       // 29  🦙 / 🐫 / 🐪
    &CLIPPY,      // 30  📎
    &TRASH,       // 31  🗑
    &JACKET,      // 32  🧥
    &BACKPACK,    // 33  🎒
    &STRAWBERRY,  // 34  🍓 / 💣 (bomb alias)
    &FOX,         // 35  🦊 / 👿 (imp alias)
    &EYES,        // 36  👀
    &WARNING,     // 37  ⚠
    &LIGHTNING,   // 38  ⚡
    &SKULL,       // 39  💀
    &GHOST,       // 40  👻
    &ROBOT,       // 41  🤖
    &COFFEE,      // 42  ☕
    &PIZZA,       // 43  🍕
    &HOTDOG,      // 44  🌭
    &UNICORN,     // 45  🦄
    &DRAGON,      // 46  🐉
    &WIZARD,      // 47  🧙
    &BELL,        // 48  🔔
    &ENVELOPE,    // 49  ✉ / 📧 / 📨 / 📩 (email aliases)
];

// ---------------------------------------------------------------------------
// Codepoint → atlas index lookup
// ---------------------------------------------------------------------------
//
// Sorted by codepoint for branch-prediction-friendly binary search.

#[rustfmt::skip]
const EMOJI_LOOKUP: &[(u32, u8)] = &[
    (0x0231B, 27), // ⌛ hourglass done (aliased to CLOCK)
    (0x023F1, 27), // ⏱ stopwatch (aliased to CLOCK)
    (0x02600, 15), // ☀ sun with rays (aliased to SUN)
    (0x02615, 42), // ☕ hot beverage
    (0x0263A, 1),  // ☺ relaxed
    (0x02665, 0),  // ♥ heart suit
    (0x026A0, 37), // ⚠ warning sign
    (0x026A1, 38), // ⚡ high voltage
    (0x02705, 17), // ✅ white heavy check mark
    (0x02709, 49), // ✉ envelope
    (0x02714, 17), // ✔ heavy check mark
    (0x0274C, 21), // ❌ cross mark
    (0x02764, 0),  // ❤ red heart
    (0x02B50, 20), // ⭐ star
    (0x1F32D, 44), // 🌭 hot dog
    (0x1F353, 34), // 🍓 strawberry
    (0x1F355, 43), // 🍕 pizza
    (0x1F389, 10), // 🎉 party popper
    (0x1F38A, 10), // 🎊 confetti ball (aliased to PARTY)
    (0x1F392, 33), // 🎒 backpack
    (0x1F3B5, 18), // 🎵 musical note
    (0x1F3B6, 18), // 🎶 multiple musical notes (aliased to NOTE)
    (0x1F3BC, 18), // 🎼 musical score (aliased to NOTE)
    (0x1F409, 46), // 🐉 dragon
    (0x1F42A, 29), // 🐪 dromedary camel (aliased to LLAMA)
    (0x1F42B, 29), // 🐫 two-hump camel (aliased to LLAMA)
    (0x1F439, 28), // 🐹 hamster (cuy)
    (0x1F440, 36), // 👀 eyes
    (0x1F44C, 5),  // 👌 OK hand
    (0x1F44D, 5), // 👍 thumbs up
    (0x1F44F, 5),  // 👏 clap
    (0x1F47B, 40), // 👻 ghost
    (0x1F47E, 16), // 👾 alien monster
    (0x1F47F, 35), // 👿 imp / angry face with horns (aliased to FOX)
    (0x1F480, 39), // 💀 skull
    (0x1F494, 0),  // 💔 broken heart
    (0x1F495, 0), // 💕 two hearts
    (0x1F496, 0), // 💖 sparkling heart
    (0x1F499, 0),  // 💙 blue heart
    (0x1F4A3, 34), // 💣 bomb (aliased to STRAWBERRY)
    (0x1F4A4, 24), // 💤 ZZZ (aliased to SLEEPING)
    (0x1F4AA, 5),  // 💪 muscle
    (0x1F4AF, 22), // 💯 hundred points
    (0x1F4CE, 30), // 📎 paperclip (clippy)
    (0x1F4E7, 49), // 📧 e-mail (aliased to ENVELOPE)
    (0x1F4E8, 49), // 📨 incoming envelope (aliased to ENVELOPE)
    (0x1F4E9, 49), // 📩 envelope with downward arrow (aliased to ENVELOPE)
    (0x1F506, 15), // 🔆 high brightness (sun)
    (0x1F514, 48), // 🔔 bell
    (0x1F525, 6),  // 🔥 fire
    (0x1F55C, 27), // 🕜 one-thirty (aliased to CLOCK)
    (0x1F55D, 27), // 🕝 two-thirty (aliased to CLOCK)
    (0x1F55E, 27), // 🕞 three-thirty (aliased to CLOCK)
    (0x1F561, 27), // 🕡 six-thirty (aliased to CLOCK)
    (0x1F562, 27), // 🕢 seven-thirty (aliased to CLOCK)
    (0x1F564, 27), // 🕤 nine-thirty (aliased to CLOCK)
    (0x1F565, 27), // 🕥 ten-thirty (aliased to CLOCK)
    (0x1F566, 27), // 🕦 eleven-thirty (aliased to CLOCK)
    (0x1F567, 27), // 🕧 twelve-thirty (aliased to CLOCK)
    (0x1F5D1, 31), // 🗑 wastebasket (ranzbak)
    (0x1F601, 1),  // 😁 beaming grin
    (0x1F602, 2), // 😂 face with tears of joy
    (0x1F605, 2), // 😅 grinning with sweat
    (0x1F606, 2),  // 😆 grinning squinting
    (0x1F607, 11), // 😇 smiling face with halo
    (0x1F608, 14), // 😈 smiling face with horns
    (0x1F609, 1),  // 😉 winking
    (0x1F60A, 1), // 😊 smiling with smiling eyes
    (0x1F60D, 4),  // 😍 smiling with heart eyes
    (0x1F60E, 19), // 😎 smiling face with sunglasses
    (0x1F618, 4),  // 😘 blowing a kiss
    (0x1F622, 3),  // 😢 crying
    (0x1F62D, 3),  // 😭 loudly crying
    (0x1F62E, 23), // 😮 face with open mouth
    (0x1F634, 24), // 😴 sleeping face
    (0x1F636, 25), // 😶 face without mouth
    (0x1F643, 26), // 🙃 upside-down face
    (0x1F644, 7),  // 🙄 face with rolling eyes
    (0x1F64F, 8),  // 🙏 folded hands
    (0x1F680, 12), // 🚀 rocket
    (0x1F914, 7),  // 🤔 thinking
    (0x1F916, 41), // 🤖 robot face
    (0x1F917, 4), // 🤗 hugging
    (0x1F923, 2), // 🤣 rolling on the floor laughing
    (0x1F95A, 9),  // 🥚 egg (Cyber Aegg mascot)
    (0x1F973, 10), // 🥳 partying face (aliased to PARTY)
    (0x1F97A, 13), // 🥺 pleading face
    (0x1F984, 45), // 🦄 unicorn face
    (0x1F98A, 35), // 🦊 fox
    (0x1F999, 29), // 🦙 llama
    (0x1F9D9, 47), // 🧙 mage / wizard
    (0x1F9E5, 32), // 🧥 coat / jacket
];

/// Returns the atlas index for a given codepoint, if any.
pub fn atlas_index(cp: u32) -> Option<u8> {
    EMOJI_LOOKUP
        .binary_search_by_key(&cp, |(c, _)| *c)
        .ok()
        .map(|i| EMOJI_LOOKUP[i].1)
}

// ---------------------------------------------------------------------------
// UTF-8-aware token stream
// ---------------------------------------------------------------------------

/// One element produced by [`decode_with_emojis`].
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Token {
    /// A regular character to draw via the host font.
    Char(char),
    /// An emoji — index into the [`ATLAS`].
    Emoji(u8),
}

/// Iterator over `&str` that yields a `Char` for every regular codepoint
/// and an `Emoji` for every codepoint with an atlas entry.  Variation
/// selectors `U+FE0E` / `U+FE0F` are silently consumed when they follow
/// a known emoji.  Orphan variation selectors anywhere else are dropped.
pub fn decode_with_emojis(s: &str) -> EmojiTokens<'_> {
    EmojiTokens {
        iter: s.chars().peekable(),
    }
}

pub struct EmojiTokens<'a> {
    iter: Peekable<Chars<'a>>,
}

impl Iterator for EmojiTokens<'_> {
    type Item = Token;

    fn next(&mut self) -> Option<Token> {
        loop {
            let c = self.iter.next()?;
            let cp = c as u32;

            // Always drop orphan variation selectors that didn't follow
            // a known emoji.
            if cp == 0xFE0E || cp == 0xFE0F {
                continue;
            }

            if let Some(idx) = atlas_index(cp) {
                // Skip an immediately-following variation selector.
                if matches!(self.iter.peek(), Some('\u{FE0E}' | '\u{FE0F}')) {
                    self.iter.next();
                }
                return Some(Token::Emoji(idx));
            }
            return Some(Token::Char(c));
        }
    }
}

// ---------------------------------------------------------------------------
// Layout helpers
// ---------------------------------------------------------------------------

/// Width of `s` measured in 7-pixel FONT_7X13 columns, with each emoji
/// counted as [`EMOJI_COLUMNS`] cells.  Used by `text_wrap` so line
/// breaks land in sensible places when emojis are present.
pub fn column_width(s: &str) -> usize {
    decode_with_emojis(s)
        .map(|t| match t {
            Token::Char(_) => 1,
            Token::Emoji(_) => EMOJI_COLUMNS,
        })
        .sum()
}

/// Pixel width of `s` rendered with [`draw_string`] — `columns × 7` plus
/// any per-emoji slack.
pub fn pixel_width(s: &str) -> i32 {
    let mut w = 0i32;
    for t in decode_with_emojis(s) {
        match t {
            Token::Char(_) => w += 7,
            Token::Emoji(_) => w += EMOJI_ADVANCE_PX,
        }
    }
    w
}

// ---------------------------------------------------------------------------
// Drawing
// ---------------------------------------------------------------------------

/// Draw one atlas glyph at `top_left` in `color`.  Pixels with the bit
/// set in the packed bitmap are drawn; others are left untouched.
pub fn draw_emoji<D>(
    display: &mut D,
    idx: u8,
    top_left: Point,
    color: D::Color,
) -> Result<(), D::Error>
where
    D: DrawTarget,
    D::Color: PixelColor,
{
    let bits = ATLAS[idx as usize];
    for y in 0..EMOJI_PX {
        for x in 0..EMOJI_PX {
            let byte = bits[y * ROW_BYTES + x / 8];
            if byte & (0x80 >> (x % 8)) != 0 {
                Pixel(top_left + Point::new(x as i32, y as i32), color).draw(display)?;
            }
        }
    }
    Ok(())
}

/// Vertical offset from the caller-supplied `position.y` to the top
/// row of an emoji glyph, in the chosen [`Baseline`].  Matches the
/// baseline math `embedded-graphics::text::Text` applies internally for
/// `FONT_7X13` (13 px tall, ascent 10).
fn emoji_top_offset(baseline: Baseline) -> i32 {
    match baseline {
        Baseline::Top         => 0,
        Baseline::Bottom      => -(EMOJI_PX as i32 - 1),
        Baseline::Middle      => -(EMOJI_PX as i32 / 2),
        // FONT_7X13's `baseline` (ascent) is 10 — keep emoji baseline
        // aligned to text baseline by offsetting top up by 10.
        Baseline::Alphabetic  => -10,
    }
}

/// Drop-in replacement for `Text::with_text_style(...).draw(d)` that
/// intercepts emoji codepoints from `s` and renders them via the atlas
/// instead of letting them fall back to the host MonoFont's
/// "missing glyph" indicator.
///
/// Plain-text runs between emojis go through the standard
/// `embedded-graphics::text::Text` renderer, so kerning, baseline
/// handling, font features etc. all match the rest of the UI.  Emojis
/// are drawn aligned to the text run's baseline (derived from
/// `text_style.baseline`) and advance the cursor by [`EMOJI_ADVANCE_PX`]
/// (= 2 character columns of `FONT_7X13`).
///
/// Returns the cursor position after the last drawn glyph — same
/// contract as `Text::draw`.
pub fn draw_string<D>(
    target:    &mut D,
    s:         &str,
    position:  Point,
    style:     MonoTextStyle<'_, D::Color>,
    text_style: TextStyle,
) -> Result<Point, D::Error>
where
    D: DrawTarget,
    D::Color: PixelColor,
{
    // Pull the active text color out of the style so emoji glyphs match
    // the surrounding text run.  `None` = transparent text — also skip
    // emoji draws to stay consistent.
    let Some(color) = style.text_color else {
        return Ok(position);
    };

    let emoji_dy = emoji_top_offset(text_style.baseline);

    let mut cursor = position;
    let mut text_start = 0usize;
    let mut iter = s.char_indices().peekable();

    while let Some(&(byte_idx, c)) = iter.peek() {
        let cp = c as u32;

        // Plain glyph — let the embedded-graphics renderer handle it
        // in a later batched run.
        let is_emoji = atlas_index(cp).is_some();
        let is_vs = cp == 0xFE0E || cp == 0xFE0F;
        if !is_emoji && !is_vs {
            iter.next();
            continue;
        }

        // Hit a special codepoint — flush any pending text run first.
        if byte_idx > text_start {
            cursor = Text::with_text_style(
                &s[text_start..byte_idx],
                cursor,
                style,
                text_style,
            )
            .draw(target)?;
        }

        // Consume the special codepoint.
        iter.next();
        text_start = byte_idx + c.len_utf8();

        if let Some(idx) = atlas_index(cp) {
            draw_emoji(target, idx, Point::new(cursor.x, cursor.y + emoji_dy), color)?;
            cursor.x += EMOJI_ADVANCE_PX;
            // Swallow an immediately-following variation selector.
            if let Some(&(_, peeked)) = iter.peek()
                && (peeked == '\u{FE0E}' || peeked == '\u{FE0F}')
            {
                iter.next();
                text_start += peeked.len_utf8();
            }
        }
        // Orphan variation selector: silently dropped (already consumed
        // above, `text_start` advanced past it).
    }

    // Flush any trailing text run.
    if text_start < s.len() {
        cursor = Text::with_text_style(&s[text_start..], cursor, style, text_style)
            .draw(target)?;
    }

    Ok(cursor)
}
