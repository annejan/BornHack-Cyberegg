//! Animation-to-filename lookup — generates FAT12 8.3 filenames from
//! (PetKind, DisplayAnim, frame_index).
//!
//! # Filename format
//!
//! Each sprite file is named `PPAAFF.PCX` where:
//!   - `PP` = pet kind prefix (hex): which prefixes are pets is declared by
//!     `PETS.CFG`, not by this code — `03` and `04` are reserved (sponsors,
//!     menu icons), everything else below `MAX_PET_PREFIX` is available.
//!   - `AA` = animation ID (hex): `01` = Idle, `02` = Happy, ...
//!   - `FF` = frame number (hex): `00`, `01`, `02`, ...
//!
//! In FAT12 8.3 format (11 bytes, no dot, space-padded):
//!   `b"PPAAFF  PCX"` — 6 hex chars + 2 spaces + PCX extension.
//!
//! The shared start screen uses `PP=00, AA=00, FF=00` (`000000  PCX`).
//! The hatching egg animation uses `AA=14` with the pet-specific prefix
//! (e.g. `001400  PCX` for pet 0's egg, `051400  PCX` for pet 5's egg).

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
        DisplayAnim::WarningSick => 0x07,
        DisplayAnim::WarningTired => 0x08,
        DisplayAnim::WarningHungry => 0x09,
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
        // 0x15-0x1F are reserved by assets/to-badge/MANIFEST.TXT for
        // planned-but-unbuilt features (ENV_FOOD_ITEM, ICON_HUNGER,
        // UI_ATTENTION, PLACEHOLDER_NOT_IMPLEMENTED, ...) across one or
        // more pets — picking IDs in that range collided with those
        // reserved (if currently uncoded) slots. 0x20+ is confirmed free
        // for every pet.
        DisplayAnim::Exercising => 0x23,
        DisplayAnim::Medicating => 0x24,
        DisplayAnim::Drinking => 0x25,
        DisplayAnim::Ozempic => 0x26,
        DisplayAnim::Rehab => 0x27,
        DisplayAnim::OnlyPets => 0x28,
        DisplayAnim::Tipsy => 0x29,
        DisplayAnim::Wasted => 0x2A,
        DisplayAnim::HungOver => 0x2B,
        DisplayAnim::Sore => 0x2C,
        DisplayAnim::Injured => 0x2D,
        DisplayAnim::Groggy => 0x2E,
        DisplayAnim::Stuffed => 0x2F,
        DisplayAnim::Thirsty => 0x30,
        DisplayAnim::Wired => 0x31,
        DisplayAnim::ThrowingUp => 0x32,
        DisplayAnim::NeedsRehab => 0x33,
        DisplayAnim::Starving => 0x34,
        DisplayAnim::Washing => 0x35,
    }
}

// ── Frame counts per animation ───────────────────────────────────────────────

// Frame counts are now derived from the runtime catalogue — see
// [`frame_count`] below.  Firmware queries the FAT12 directory list
// populated at boot; the simulator probes `assets/to-badge/`.

// ── Filename generation ──────────────────────────────────────────────────────

/// The one 8.3 PCX file-name builder: `"PPAAFF  PCX"`, each of the three
/// fields as two uppercase hex digits.
pub const fn pcx_name(pp: u8, aa: u8, ff: u8) -> [u8; 11] {
    let [p0, p1] = crate::hex::byte_upper(pp);
    let [a0, a1] = crate::hex::byte_upper(aa);
    let [f0, f1] = crate::hex::byte_upper(ff);
    [p0, p1, a0, a1, f0, f1, b' ', b' ', b'P', b'C', b'X']
}

/// Build the FAT12 8.3 filename for a given pet, animation, and frame.
///
/// Returns an 11-byte array: `"PPAAFF  PCX"`.
pub fn build_filename(kind: PetKind, anim: DisplayAnim, frame: u8) -> [u8; 11] {
    let pp = kind.0;
    let aa = anim_id(anim);
    pcx_name(pp, aa, frame)
}

/// Build the FAT12 8.3 filename for the start screen.
pub fn start_screen_filename() -> [u8; 11] {
    *b"000000  PCX"
}

/// Battle-pose animation codes (single frame `00` each). See the
/// battle-animation spec — only left-facing art exists; the right combatant is
/// the same file mirrored in software.
pub const BATTLE_AA_STAND: u8 = 0x20;
pub const BATTLE_AA_WON: u8 = 0x21;
pub const BATTLE_AA_LOST: u8 = 0x22;

/// Build the FAT12 filename for a battle pose: `"PPAA00  PCX"`. `pp` is a raw
/// species prefix (may be the generic-unknown placeholder) and `aa` one of the
/// `BATTLE_AA_*` codes. Bypasses the `PetKind`/`DisplayAnim` mapping because
/// battle poses are fixed single frames addressed by raw prefix.
pub fn battle_filename(pp: u8, aa: u8) -> [u8; 11] {
    pcx_name(pp, aa, 0)
}

/// Eight menu icons live under prefix `0x04`, one sequence per icon:
///
/// | slot | sequence | name           | row, col   |
/// |------|----------|----------------|------------|
/// |   0  | `0x00`   | `MENU_STATS`   | top, 0     |
/// |   1  | `0x01`   | `MENU_HIBERNATE` | top, 1   |
/// |   2  | `0x02`   | `MENU_FEED`    | bottom, 0  |
/// |   3  | `0x03`   | `MENU_HEAL`    | bottom, 1  |
/// |   4  | `0x04`   | `MENU_PLAY`    | bottom, 2  |
/// |   5  | `0x05`   | `MENU_REST`    | bottom, 3  |
/// |   6  | `0x06`   | `MENU_EXERCISE`| top, 2     |
/// |   7  | `0x07`   | `MENU_DRINK`   | top, 3     |
///
/// `selected` selects between frame 0 (normal, drawn over the white
/// background) and frame 1 (selected — replaces the firmware-drawn
/// black selection circle entirely).
///
/// Menu icons moved from prefix `03` to `04` when the slug pet
/// (`02xx`) and sponsors (`03xx`) shifted up. Slots 6-7 (Exercise,
/// Drink) were added after slots 0-5 already shipped, so they keep
/// their own slot numbers rather than renumbering the existing five
/// icons' art.
pub fn menu_icon_filename(slot: u8, selected: bool) -> [u8; 11] {
    let aa = slot;
    let ff: u8 = if selected { 1 } else { 0 };
    pcx_name(0x04, aa, ff)
}

/// Number of menu-icon slots (top-row 4 + bottom-row 4).
pub const MENU_ICON_COUNT: u8 = 8;

/// Where menu icon `slot` sits: `(top_row, column)`. Slots 0-5 shipped first
/// (0, 1 on the top row, 2-5 on the bottom row); Exercise (6) and Drink (7)
/// were added later and fill the top row's columns 2 and 3.
pub fn menu_icon_pos(slot: u8) -> (bool, u8) {
    match slot {
        0 | 1 => (true, slot),
        6 => (true, 2),
        7 => (true, 3),
        _ => (false, slot - 2),
    }
}

/// Sprite frame for a raw step count: hatching clamps to the last frame (the
/// egg stays cracked), every other animation wraps. `count` must be > 0.
pub fn clamp_or_wrap(raw: u32, count: u8, anim: DisplayAnim) -> u8 {
    if matches!(anim, DisplayAnim::Hatching { .. }) {
        raw.min(count.saturating_sub(1) as u32) as u8
    } else {
        (raw % count as u32) as u8
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
    let pp = kind.0;
    let aa = anim_id(anim);
    super::super::sprite_loader::count_anim_frames(pp, aa)
}

/// Get the 8.3 filename for animation `anim` at frame `frame_index`.
///
/// If the frame index exceeds the animation's frame count, wraps around.
pub fn anim_filename(kind: PetKind, anim: DisplayAnim, frame_index: u8) -> [u8; 11] {
    let count = frame_count(kind, anim);
    let frame = if count > 0 { frame_index % count } else { 0 };
    build_filename(kind, anim, frame)
}

#[cfg(test)]
mod pcx_name_tests {
    use super::*;

    #[test]
    fn builds_uppercase_8_3_names() {
        assert_eq!(pcx_name(0x05, 0x1A, 0x0F), *b"051A0F  PCX");
        assert_eq!(pcx_name(0, 0, 0), *b"000000  PCX");
        assert_eq!(start_screen_filename(), pcx_name(0, 0, 0));
    }

    #[test]
    fn v3_anim_ids_match_the_art_table() {
        use DisplayAnim::*;
        for (a, id) in [
            (Relaxing, 0x0E),
            (Tipsy, 0x29),
            (Wasted, 0x2A),
            (HungOver, 0x2B),
            (Sore, 0x2C),
            (Injured, 0x2D),
            (Groggy, 0x2E),
            (Stuffed, 0x2F),
            (Thirsty, 0x30),
            (Wired, 0x31),
            (ThrowingUp, 0x32),
            (NeedsRehab, 0x33),
            (Starving, 0x34),
            (Washing, 0x35),
        ] {
            assert_eq!(anim_id(a), id, "{a:?}");
        }
        assert_eq!(build_filename(PetKind(1), Washing, 0), *b"013500  PCX");
    }

    #[test]
    fn wrappers_match() {
        assert_eq!(battle_filename(0x05, BATTLE_AA_WON), *b"052100  PCX");
        assert_eq!(menu_icon_filename(7, true), *b"040701  PCX");
        assert_eq!(menu_icon_filename(2, false), *b"040200  PCX");
    }
}
