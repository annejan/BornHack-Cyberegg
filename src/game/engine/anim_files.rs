//! Animation-to-filename lookup table.
//!
//! Maps each [`DisplayAnim`] variant + frame index to an 8.3 filename
//! on the FAT12 filesystem.  The sprite loader uses this to know which
//! PCX file to load for a given animation state.
//!
//! During development, undrawn animations point to `11111111.PCX`
//! (the "not implemented" placeholder).  Once artwork is ready, update
//! the entries here or load them from `MANIFEST.TXT` at runtime.
//!
//! # Usage
//!
//! ```rust,ignore
//! let name = anim_filename(DisplayAnim::Hatching { ticks_remaining: 0 }, 2);
//! // → b"15000002PCX" (8.3 format, no dot)
//! ```

use super::to_display::DisplayAnim;

/// Not-implemented placeholder filename (8.3 format).
const NI: [u8; 11] = *b"1E000000PCX";

/// Maximum frames per animation sequence.
pub const MAX_FRAMES: u8 = 5;

/// Lookup table entry: up to 5 filenames per animation.
struct AnimEntry {
    filenames: [[u8; 11]; MAX_FRAMES as usize],
    count: u8,
}

impl AnimEntry {
    const fn single(f0: [u8; 11]) -> Self {
        Self {
            filenames: [f0, NI, NI, NI, NI],
            count: 1,
        }
    }

    const fn new(count: u8, f: [[u8; 11]; MAX_FRAMES as usize]) -> Self {
        Self {
            filenames: f,
            count,
        }
    }

    // const fn placeholder(count: u8) -> Self {
    //     Self { filenames: [NI; MAX_FRAMES as usize], count }
    // }
}

// ---------------------------------------------------------------------------
// Lookup table
// ---------------------------------------------------------------------------

// Group 6: idle / happy.
const IDLE_NEUTRAL: AnimEntry = AnimEntry::single(*b"01000000PCX");
const HAPPY: AnimEntry = AnimEntry::new(2, [*b"02000000PCX", *b"02000001PCX", NI, NI, NI]);

// Group 4: critical stats.
const CRITICAL_SICK: AnimEntry = AnimEntry::single(*b"03000000PCX");
const CRITICAL_TIRED: AnimEntry = AnimEntry::single(*b"04000000PCX");
const CRITICAL_HUNGRY: AnimEntry = AnimEntry::single(*b"05000000PCX");
const CRITICAL_DRAINED: AnimEntry = AnimEntry::single(*b"06000000PCX");

// Group 5: warning stats.
const WARNING_SICK: AnimEntry = AnimEntry::single(*b"07000000PCX");
const WARNING_TIRED: AnimEntry = AnimEntry::single(*b"08000000PCX");
const WARNING_HUNGRY: AnimEntry = AnimEntry::single(*b"09000000PCX");
const WARNING_DRAINED: AnimEntry = AnimEntry::single(*b"0A000000PCX");
const WARNING_MISERABLE: AnimEntry = AnimEntry::single(*b"0B000000PCX");

// Group 2: active actions.
const FEEDING: AnimEntry = AnimEntry::new(2, [*b"0C000000PCX", *b"0C000001PCX", NI, NI, NI]);
const HEALING: AnimEntry = AnimEntry::new(2, [*b"0D000000PCX", *b"0D000001PCX", NI, NI, NI]);
const RELAXING: AnimEntry = AnimEntry::single(*b"0E000000PCX");
const PLAYING: AnimEntry = AnimEntry::single(*b"0F000000PCX");
const SLEEPING: AnimEntry = AnimEntry::new(2, [*b"10000000PCX", *b"10000001PCX", NI, NI, NI]);

// Group 3: leaving.
const LEAVING: AnimEntry = AnimEntry::single(*b"11000000PCX");

// Group 1: terminal / blocking.
const GONE: AnimEntry = AnimEntry::single(*b"12000000PCX");
const HIBERNATING: AnimEntry = AnimEntry::single(*b"13000000PCX");

const HATCHING: AnimEntry = AnimEntry::new(
    4,
    [
        *b"14000000PCX",
        *b"14000001PCX",
        *b"14000002PCX",
        *b"14000003PCX",
        NI,
    ],
);

/// Get the entry for a given animation state.
fn entry_for(anim: DisplayAnim) -> &'static AnimEntry {
    match anim {
        DisplayAnim::Idle => &IDLE_NEUTRAL,
        DisplayAnim::Happy => &HAPPY,

        DisplayAnim::CriticalSick => &CRITICAL_SICK,
        DisplayAnim::CriticalTired => &CRITICAL_TIRED,
        DisplayAnim::CriticalHungry => &CRITICAL_HUNGRY,
        DisplayAnim::CriticalDrained => &CRITICAL_DRAINED,

        DisplayAnim::WarningSick => &WARNING_SICK,
        DisplayAnim::WarningTired => &WARNING_TIRED,
        DisplayAnim::WarningHungry => &WARNING_HUNGRY,
        DisplayAnim::WarningDrained => &WARNING_DRAINED,
        DisplayAnim::WarningMiserable => &WARNING_MISERABLE,

        DisplayAnim::Feeding => &FEEDING,
        DisplayAnim::Healing => &HEALING,
        DisplayAnim::Relaxing => &RELAXING,
        DisplayAnim::Playing => &PLAYING,
        DisplayAnim::Sleeping => &SLEEPING,

        DisplayAnim::Leaving { .. } => &LEAVING,
        DisplayAnim::Gone => &GONE,
        DisplayAnim::Hibernating => &HIBERNATING,
        DisplayAnim::Hatching { .. } => &HATCHING,
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Get the 8.3 filename for animation `anim` at frame `frame_index`.
///
/// Returns the filename in FAT12 8.3 format (11 bytes, no dot).
/// If the frame index exceeds the animation's frame count, wraps around.
/// Use [`frame_count`] to query how many frames an animation has.
pub fn anim_filename(anim: DisplayAnim, frame_index: u8) -> &'static [u8; 11] {
    let e = entry_for(anim);
    let idx = if e.count > 0 {
        frame_index % e.count
    } else {
        0
    };
    &e.filenames[idx as usize]
}

/// Number of frames available for the given animation.
pub fn frame_count(anim: DisplayAnim) -> u8 {
    entry_for(anim).count
}
