//! Hidden button-sequence tracker shared by the cheat and flush combos.
//!
//! Each combo owns its own `static ButtonSequence` (see
//! `crate::game::debug_cheats` and `crate::display_flush`) so their
//! progress counters never interfere: the flush combo is fed from every
//! screen, the cheat combo only from the game screen.

use crate::menu::ButtonId;
use core::sync::atomic::{AtomicU8, Ordering};

/// Tracks progress through one fixed 9-press button sequence.
pub struct ButtonSequence {
    seq: [ButtonId; 9],
    /// How many correct presses in a row have been made so far.
    progress: AtomicU8,
}

impl ButtonSequence {
    pub const fn new(seq: [ButtonId; 9]) -> Self {
        Self { seq, progress: AtomicU8::new(0) }
    }

    /// Feed one button press into the sequence tracker.
    ///
    /// Returns `true` the instant the full sequence completes — the caller
    /// should act on it and treat the triggering press as consumed (not
    /// also forwarded to normal navigation). Returns `false` otherwise,
    /// including every partial-progress press, so the caller can fall
    /// through to its regular button handling.
    pub fn feed(&self, btn: ButtonId) -> bool {
        let progress = self.progress.load(Ordering::Relaxed) as usize;
        if btn == self.seq[progress] {
            let next = progress + 1;
            if next == self.seq.len() {
                self.progress.store(0, Ordering::Relaxed);
                return true;
            }
            self.progress.store(next as u8, Ordering::Relaxed);
        } else {
            // Wrong button — restart, but let this press count as step one
            // if it happens to match the sequence's first button, so
            // slightly-mistimed re-attempts don't have to fully pause first.
            let restart = if btn == self.seq[0] { 1 } else { 0 };
            self.progress.store(restart, Ordering::Relaxed);
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHEAT: [ButtonId; 9] = [
        ButtonId::Up,
        ButtonId::Up,
        ButtonId::Down,
        ButtonId::Down,
        ButtonId::Left,
        ButtonId::Right,
        ButtonId::Left,
        ButtonId::Right,
        ButtonId::Fire,
    ];
    const FLUSH: [ButtonId; 9] = [
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

    #[test]
    fn full_sequence_returns_true_once() {
        for seq in [CHEAT, FLUSH] {
            let tracker = ButtonSequence::new(seq);
            let mut result = false;
            for &btn in &seq {
                result = tracker.feed(btn);
            }
            assert!(result, "the final press of the correct sequence should return true");
        }
    }

    #[test]
    fn wrong_button_resets_progress() {
        for seq in [CHEAT, FLUSH] {
            let tracker = ButtonSequence::new(seq);
            assert!(!tracker.feed(seq[0]));
            assert!(!tracker.feed(seq[1]));
            // Wrong button here — breaks the sequence.
            assert!(!tracker.feed(ButtonId::Fire));
            // Now replay the whole thing; should still need all 9 presses.
            for &btn in &seq[..seq.len() - 1] {
                assert!(!tracker.feed(btn));
            }
            assert!(tracker.feed(seq[seq.len() - 1]));
        }
    }

    #[test]
    fn partial_progress_never_returns_true_early() {
        for seq in [CHEAT, FLUSH] {
            let tracker = ButtonSequence::new(seq);
            for &btn in &seq[..seq.len() - 1] {
                assert!(!tracker.feed(btn));
            }
        }
    }

    #[test]
    fn sequences_do_not_trigger_each_other() {
        let tracker = ButtonSequence::new(CHEAT);
        for &btn in &FLUSH {
            assert!(!tracker.feed(btn));
        }
    }
}
