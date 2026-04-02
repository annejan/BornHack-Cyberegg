//! BornPets game-screen input routing.
//!
//! [`dispatch`] is the single entry point for all button events while the game
//! screen (screen 0) is active.  It decides whether a modal is open or not,
//! and routes accordingly.  Returning `false` tells the caller that the event
//! was not consumed by the game layer and should be forwarded to the global
//! [`DisplayState`] (e.g. to switch screens).

use super::modal;
use super::nav::{get_nav, nav_down, nav_left, nav_right, nav_up, NavResult};

// ── Button enum ───────────────────────────────────────────────────────────────

/// Hardware button / joystick direction, abstracted away from GPIO indices.
pub enum GameBtn {
    Cancel,
    Execute,
    Up,
    Down,
    Left,
    Right,
    Fire,
}

// ── Dispatch ──────────────────────────────────────────────────────────────────

/// Route a button press on the game screen.
///
/// Returns `true` if the game layer consumed the event.
/// Returns `false` if the caller should forward to `DisplayState`
/// (only possible for `Right` at the grid edge, which advances to the next
/// screen, and `Cancel` / `Execute` while no modal is open).
pub fn dispatch(btn: GameBtn) -> bool {
    if modal::is_open() {
        // All input goes to the modal while it is active.
        match btn {
            GameBtn::Cancel             => modal::close(),
            GameBtn::Up                 => modal::cursor_up(),
            GameBtn::Down               => modal::cursor_down(),
            GameBtn::Fire | GameBtn::Execute => modal::activate(),
            GameBtn::Left | GameBtn::Right   => {}  // ignored in modal
        }
        true
    } else {
        match btn {
            GameBtn::Up    => { nav_up();   true }
            GameBtn::Down  => { nav_down(); true }
            GameBtn::Left  => { nav_left(); true }
            GameBtn::Right => {
                // At the right edge: signal the caller to move to the next screen.
                matches!(nav_right(), NavResult::Moved)
            }
            GameBtn::Fire | GameBtn::Execute => {
                let nav  = get_nav();
                let kind = modal::kind_for_icon(nav.row, nav.col);
                modal::open(kind);
                true
            }
            // Cancel with no modal open: nothing to do on the game screen.
            GameBtn::Cancel => true,
        }
    }
}
