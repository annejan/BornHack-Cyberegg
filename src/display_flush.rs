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

use crate::menu::ButtonId;
use core::sync::atomic::{AtomicU8, Ordering};

const SEQUENCE: [ButtonId; 9] = [
    ButtonId::Down,
    ButtonId::Down,
    ButtonId::Up,
    ButtonId::Up,
    ButtonId::Right,
    ButtonId::Left,
    ButtonId::Right,
    ButtonId::Left,
    ButtonId::Fire,
];

/// How many correct presses in a row have been made so far.
static PROGRESS: AtomicU8 = AtomicU8::new(0);

/// Feed one button press into the sequence tracker.
///
/// Returns `true` the instant the full sequence completes — the caller
/// should trigger the flush and treat the triggering press as consumed.
/// Returns `false` otherwise, including every partial-progress press, so
/// the caller can fall through to its regular button handling.
pub fn feed(btn: ButtonId) -> bool {
    let progress = PROGRESS.load(Ordering::Relaxed) as usize;
    if btn == SEQUENCE[progress] {
        let next = progress + 1;
        if next == SEQUENCE.len() {
            PROGRESS.store(0, Ordering::Relaxed);
            return true;
        }
        PROGRESS.store(next as u8, Ordering::Relaxed);
    } else {
        // Wrong button — restart, but let this press count as step one
        // if it happens to match the sequence's first button, so
        // slightly-mistimed re-attempts don't have to fully pause first.
        let restart = if btn == SEQUENCE[0] { 1 } else { 0 };
        PROGRESS.store(restart, Ordering::Relaxed);
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_sequence_returns_true_once() {
        let mut result = false;
        for &btn in &SEQUENCE {
            result = feed(btn);
        }
        assert!(result, "the final press of the correct sequence should return true");
    }

    #[test]
    fn wrong_button_resets_progress() {
        assert!(!feed(ButtonId::Down));
        assert!(!feed(ButtonId::Down));
        // Wrong button here — breaks the sequence.
        assert!(!feed(ButtonId::Fire));
        // Now replay the whole thing; should still need all 9 presses.
        for &btn in &SEQUENCE[..SEQUENCE.len() - 1] {
            assert!(!feed(btn));
        }
        assert!(feed(*SEQUENCE.last().unwrap()));
    }

    #[test]
    fn partial_progress_never_returns_true_early() {
        for &btn in &SEQUENCE[..SEQUENCE.len() - 1] {
            assert!(!feed(btn));
        }
    }
}
