//! Animation-to-filename lookup — generates FAT12 8.3 filenames from
//! (PetKind, DisplayAnim, frame_index).
//!
//! # Filename format
//!
//! Each sprite file is named `PPAAFF.PCX` where:
//!   - `PP` = pet kind prefix (hex): `00` = Bartholomeus
//!     (formerly "Snail"), `01` = Cat, `02` = Slug, ...
//!   - `AA` = animation ID (hex): `01` = Idle, `02` = Happy, ...
//!   - `FF` = frame number (hex): `00`, `01`, `02`, ...
//!
//! In FAT12 8.3 format (11 bytes, no dot, space-padded):
//!   `b"PPAAFF  PCX"` — 6 hex chars + 2 spaces + PCX extension.
//!
//! The shared start screen uses `PP=00, AA=00, FF=00` (`000000  PCX`).
//! The hatching egg animation uses `AA=14` with the pet-specific prefix
//! (e.g. `001400  PCX` for Bartholomeus egg, `011400  PCX` for cat
//! egg, `021400  PCX` for slug egg).

use super::PetKind;
use super::to_display::DisplayAnim;

// ── Animation IDs (AA byte) ──────────────────────────────────────────────────

/// Map a DisplayAnim to its animation ID byte.
fn anim_id(anim: DisplayAnim) -> u8 {
    match anim {
        DisplayAnim::Idle => 0x01,
        DisplayAnim::Happy => 0x02,
        DisplayAnim::CriticalSick => 0x03,
        DisplayAnim::CriticalTired => 0x04,
        DisplayAnim::CriticalHungry => 0x05,
        DisplayAnim::CriticalDrained => 0x06,
        DisplayAnim::WarningSick => 0x07,
        DisplayAnim::WarningTired => 0x08,
        DisplayAnim::WarningHungry => 0x09,
        DisplayAnim::WarningDrained => 0x0A,
        DisplayAnim::WarningMiserable => 0x0B,
        DisplayAnim::Feeding => 0x0C,
        DisplayAnim::Healing => 0x0D,
        DisplayAnim::Relaxing => 0x0E,
        DisplayAnim::Playing => 0x0F,
        DisplayAnim::Sleeping => 0x10,
        DisplayAnim::Leaving { .. } => 0x11,
        DisplayAnim::Gone => 0x12,
        DisplayAnim::Hibernating => 0x13,
        DisplayAnim::Hatching { .. } => 0x14,
    }
}

// ── Frame counts per animation ───────────────────────────────────────────────

// Frame counts are now derived from the runtime catalogue — see
// [`frame_count`] below.  Firmware queries the FAT12 directory list
// populated at boot; the simulator probes `assets/to-badge/`.

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
        HEX[(pp >> 4) as usize],
        HEX[(pp & 0xF) as usize],
        HEX[(aa >> 4) as usize],
        HEX[(aa & 0xF) as usize],
        HEX[(ff >> 4) as usize],
        HEX[(ff & 0xF) as usize],
        b' ',
        b' ',
        b'P',
        b'C',
        b'X',
    ]
}

/// Build the FAT12 8.3 filename for the start screen.
pub fn start_screen_filename() -> [u8; 11] {
    *b"000000  PCX"
}

/// Six menu icons live under prefix `0x04`, one sequence per icon:
///
/// | slot | sequence | name           | row, col   |
/// |------|----------|----------------|------------|
/// |   0  | `0x00`   | `MENU_STATS`   | top, 0     |
/// |   1  | `0x01`   | `MENU_HIBERNATE` | top, 1   |
/// |   2  | `0x02`   | `MENU_FEED`    | bottom, 0  |
/// |   3  | `0x03`   | `MENU_HEAL`    | bottom, 1  |
/// |   4  | `0x04`   | `MENU_PLAY`    | bottom, 2  |
/// |   5  | `0x05`   | `MENU_REST`    | bottom, 3  |
///
/// `selected` selects between frame 0 (normal, drawn over the white
/// background) and frame 1 (selected — replaces the firmware-drawn
/// black selection circle entirely).
///
/// Menu icons moved from prefix `03` to `04` when the slug pet
/// (`02xx`) and sponsors (`03xx`) shifted up.
pub fn menu_icon_filename(slot: u8, selected: bool) -> [u8; 11] {
    let aa = slot;
    let ff: u8 = if selected { 1 } else { 0 };
    [
        b'0',
        b'4',
        HEX[(aa >> 4) as usize],
        HEX[(aa & 0xF) as usize],
        HEX[(ff >> 4) as usize],
        HEX[(ff & 0xF) as usize],
        b' ',
        b' ',
        b'P',
        b'C',
        b'X',
    ]
}

/// Number of menu-icon slots (top-row 2 + bottom-row 4).
pub const MENU_ICON_COUNT: u8 = 6;

/// Map (row=Top, col 0..1) and (row=Bottom, col 0..3) to the
/// `slot` argument of [`menu_icon_filename`].  Returns `None` for
/// the empty top-row cells (cols 2, 3).
pub fn menu_icon_slot(top_row: bool, col: u8) -> Option<u8> {
    if top_row {
        if col < 2 { Some(col) } else { None }
    } else if col < 4 {
        Some(2 + col)
    } else {
        None
    }
}

/// Public accessor for the animation-id byte used in the FAT12
/// filename.  Callers track the last id seen to detect animation
/// changes (so they can reset their frame counter).
pub fn anim_id_for(anim: DisplayAnim) -> u8 {
    anim_id(anim)
}

// ── Public API ───────────────────────────────────────────────────────────────

/// Number of frames available for the given pet kind and animation.
///
/// Counts the contiguous run of `PPAAFF.PCX` files starting at frame
/// `00` in the runtime sprite catalogue (FAT12 directory in firmware,
/// `assets/to-badge/` on the simulator).  No frame `00` → returns 0
/// → callers treat as "no animation available".
pub fn frame_count(kind: PetKind, anim: DisplayAnim) -> u8 {
    let pp = kind as u8;
    let aa = anim_id(anim);
    let prefix = [
        HEX[(pp >> 4) as usize],
        HEX[(pp & 0xF) as usize],
        HEX[(aa >> 4) as usize],
        HEX[(aa & 0xF) as usize],
    ];
    super::super::sprite_loader::count_anim_frames(&prefix)
}

/// Get the 8.3 filename for animation `anim` at frame `frame_index`.
///
/// If the frame index exceeds the animation's frame count, wraps around.
pub fn anim_filename(kind: PetKind, anim: DisplayAnim, frame_index: u8) -> [u8; 11] {
    let count = frame_count(kind, anim);
    let frame = if count > 0 { frame_index % count } else { 0 };
    build_filename(kind, anim, frame)
}
