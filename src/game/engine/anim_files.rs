//! Animation-to-filename lookup — generates FAT12 8.3 filenames from
//! (PetKind, DisplayAnim, frame_index).
//!
//! # Filename format
//!
//! Each sprite file is named `PPAAFF.PCX` where:
//!   - `PP` = pet kind prefix (hex): `00` = Snail, `01` = Cat, ...
//!   - `AA` = animation ID (hex): `01` = Idle, `02` = Happy, ...
//!   - `FF` = frame number (hex): `00`, `01`, `02`, ...
//!
//! In FAT12 8.3 format (11 bytes, no dot, space-padded):
//!   `b"PPAAFF  PCX"` — 6 hex chars + 2 spaces + PCX extension.
//!
//! The shared start screen uses `PP=00, AA=00, FF=00` (`000000  PCX`).
//! The hatching egg animation uses `AA=14` with the pet-specific prefix
//! (e.g. `001400  PCX` for snail egg, `011400  PCX` for cat egg).

use super::PetKind;
use super::to_display::DisplayAnim;

// ── Animation IDs (AA byte) ──────────────────────────────────────────────────

/// Map a DisplayAnim to its animation ID byte.
fn anim_id(anim: DisplayAnim) -> u8 {
    match anim {
        DisplayAnim::Idle              => 0x01,
        DisplayAnim::Happy             => 0x02,
        DisplayAnim::CriticalSick      => 0x03,
        DisplayAnim::CriticalTired     => 0x04,
        DisplayAnim::CriticalHungry    => 0x05,
        DisplayAnim::CriticalDrained   => 0x06,
        DisplayAnim::WarningSick       => 0x07,
        DisplayAnim::WarningTired      => 0x08,
        DisplayAnim::WarningHungry     => 0x09,
        DisplayAnim::WarningDrained    => 0x0A,
        DisplayAnim::WarningMiserable  => 0x0B,
        DisplayAnim::Feeding           => 0x0C,
        DisplayAnim::Healing           => 0x0D,
        DisplayAnim::Relaxing          => 0x0E,
        DisplayAnim::Playing           => 0x0F,
        DisplayAnim::Sleeping          => 0x10,
        DisplayAnim::Leaving { .. }    => 0x11,
        DisplayAnim::Gone              => 0x12,
        DisplayAnim::Hibernating       => 0x13,
        DisplayAnim::Hatching { .. }   => 0x14,
    }
}

// ── Frame counts per animation ───────────────────────────────────────────────

/// Frame counts for snail animations.
const SNAIL_FRAMES: [u8; 21] = [
    0,  // 0x00: start screen (not used here)
    1,  // 0x01: idle
    2,  // 0x02: happy
    1,  // 0x03: critical sick
    1,  // 0x04: critical tired
    1,  // 0x05: critical hungry
    1,  // 0x06: critical drained
    1,  // 0x07: warning sick
    1,  // 0x08: warning tired
    1,  // 0x09: warning hungry
    1,  // 0x0A: warning drained
    1,  // 0x0B: warning miserable
    2,  // 0x0C: feeding
    2,  // 0x0D: healing
    1,  // 0x0E: relaxing
    1,  // 0x0F: playing
    2,  // 0x10: sleeping
    1,  // 0x11: leaving
    1,  // 0x12: gone
    1,  // 0x13: hibernating
    4,  // 0x14: hatching
];

/// Frame counts for cat animations.
const CAT_FRAMES: [u8; 21] = [
    0,  // 0x00: start screen
    2,  // 0x01: idle
    2,  // 0x02: happy
    1,  // 0x03: critical sick
    2,  // 0x04: critical tired
    2,  // 0x05: critical hungry
    2,  // 0x06: critical drained
    1,  // 0x07: warning sick
    2,  // 0x08: warning tired
    2,  // 0x09: warning hungry
    2,  // 0x0A: warning drained
    2,  // 0x0B: warning miserable
    2,  // 0x0C: feeding
    2,  // 0x0D: healing
    2,  // 0x0E: relaxing
    2,  // 0x0F: playing
    1,  // 0x10: sleeping
    1,  // 0x11: leaving
    1,  // 0x12: gone
    1,  // 0x13: hibernating
    4,  // 0x14: hatching
];

fn frames_for(kind: PetKind) -> &'static [u8; 21] {
    match kind {
        PetKind::Snail => &SNAIL_FRAMES,
        PetKind::Cat   => &CAT_FRAMES,
    }
}

// ── Filename generation ──────────────────────────────────────────────────────

const HEX: &[u8; 16] = b"0123456789ABCDEF";

/// Build the FAT12 8.3 filename for a given pet, animation, and frame.
///
/// Returns an 11-byte array: `"PPAAFF  PCX"`.
pub fn build_filename(kind: PetKind, anim: DisplayAnim, frame: u8) -> [u8; 11] {
    let pp = kind as u8;
    let aa = anim_id(anim);
    let ff = frame;
    [
        HEX[(pp >> 4) as usize], HEX[(pp & 0xF) as usize],
        HEX[(aa >> 4) as usize], HEX[(aa & 0xF) as usize],
        HEX[(ff >> 4) as usize], HEX[(ff & 0xF) as usize],
        b' ', b' ',
        b'P', b'C', b'X',
    ]
}

/// Build the FAT12 8.3 filename for the start screen.
pub fn start_screen_filename() -> [u8; 11] {
    *b"000000  PCX"
}

// ── Public API (compatible with old interface) ───────────────────────────────

/// Maximum frames per animation sequence.
pub const MAX_FRAMES: u8 = 5;

/// Number of frames available for the given pet kind and animation.
pub fn frame_count(kind: PetKind, anim: DisplayAnim) -> u8 {
    let id = anim_id(anim) as usize;
    let table = frames_for(kind);
    if id < table.len() { table[id] } else { 1 }
}

/// Get the 8.3 filename for animation `anim` at frame `frame_index`.
///
/// If the frame index exceeds the animation's frame count, wraps around.
pub fn anim_filename(kind: PetKind, anim: DisplayAnim, frame_index: u8) -> [u8; 11] {
    let count = frame_count(kind, anim);
    let frame = if count > 0 { frame_index % count } else { 0 };
    build_filename(kind, anim, frame)
}
