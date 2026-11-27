//! Hidden button sequence that forces a full black → white → redraw
//! e-paper flush.
//!
//! E-paper accumulates visible ghosting over many fast partial updates;
//! the normal fix is flipping screens or waiting for the automatic
//! full-refresh promotion (see `FULL_REFRESH_PENDING`). This gives anyone
//! a way to force a clean de-ghost immediately, on any screen — not just
//! the game screen, since ghosting isn't game-specific — without waiting
//! for that promotion or navigating into a menu.
//!
//! Sequence: Down, Down, Up, Up, Right, Left, Right, Left, Fire — the
//! debug-cheat combo (`crate::game::debug_cheats`) run in reverse, so the
//! two hidden combos are easy to keep straight without colliding.
//!
//! Same "watch every press, only intercept on full match" tracker shape
//! as `debug_cheats`, just fed from the global button task in
//! `fw::button` instead of the game screen's input dispatch, so it works
//! regardless of which screen is active.

use crate::button_seq::ButtonSequence;
use crate::menu::ButtonId;

static TRACKER: ButtonSequence = ButtonSequence::new([
    ButtonId::Down,
    ButtonId::Down,
    ButtonId::Up,
    ButtonId::Up,
    ButtonId::Right,
    ButtonId::Left,
    ButtonId::Right,
    ButtonId::Left,
    ButtonId::Fire,
]);

/// Feed one button press into the sequence tracker.
///
/// Returns `true` the instant the full sequence completes — the caller
/// should trigger the flush and treat the triggering press as consumed.
/// Returns `false` otherwise, including every partial-progress press, so
/// the caller can fall through to its regular button handling.
pub fn feed(btn: ButtonId) -> bool {
    TRACKER.feed(btn)
}
