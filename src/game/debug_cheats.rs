//! Hidden debug/cheat sequence.
//!
//! Lets a developer jump straight to the weight/diabetes states for
//! testing instead of waiting out the real multi-day arc. Entered by
//! pressing a fixed button sequence while idle on the main game screen
//! (no modal or mini-game open) — same "hidden combo" idea as the
//! existing boot-time combos (DFU, safe-LUT, factory reset), just
//! entered as a sequence during play instead of held at boot.
//!
//! Sequence: Up, Up, Down, Down, Left, Right, Left, Right, Fire — the
//! classic template, adapted to this badge's Up/Down/Left/Right/Fire
//! button set (no A/B/Start).

use crate::button_seq::ButtonSequence;
use crate::menu::ButtonId;

static TRACKER: ButtonSequence = ButtonSequence::new([
    ButtonId::Up,
    ButtonId::Up,
    ButtonId::Down,
    ButtonId::Down,
    ButtonId::Left,
    ButtonId::Right,
    ButtonId::Left,
    ButtonId::Right,
    ButtonId::Fire,
]);

/// Feed one button press into the sequence tracker.
///
/// Returns `true` the instant the full sequence completes — the caller
/// should open the debug menu and treat the triggering press as
/// consumed (not also forwarded to normal navigation). Returns `false`
/// otherwise, including every partial-progress press, so the caller can
/// fall through to its regular button handling.
pub fn feed(btn: ButtonId) -> bool {
    TRACKER.feed(btn)
}
