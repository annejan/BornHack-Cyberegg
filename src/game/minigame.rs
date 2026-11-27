//! Boilerplate shared by the mini-games.
//!
//! [`DifficultyPicker`] is the two-option "pick a difficulty" overlay shown at
//! the start of a game. Each game owns one `static` instance; all state is in
//! relaxed atomics because it is read from synchronous draw code.

use core::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use embedded_graphics::prelude::*;

use crate::{TriColor, ui};

/// Pay a finished mini-game by its result (`engine::GRADE_*`): HAX, fun, the
/// game's cooldown and a toast. Each game calls this once, from `close()`.
pub fn pay_result(game: super::engine::MiniGame, grade: u8) {
    super::lifecycle::award_inspiration(game, grade);
    super::show_toast(if grade == super::engine::GRADE_LOSS {
        super::Toast::MinigameTry
    } else {
        super::Toast::MinigameWin
    });
}

/// Difficulty-picker overlay state: open flag, highlighted row, and the
/// difficulty confirmed for the current game.
pub struct DifficultyPicker {
    open: AtomicBool,
    pos: AtomicU8,
    chosen: AtomicU8,
    default: u8,
}

impl DifficultyPicker {
    /// `default` is both the initially chosen difficulty and the row
    /// highlighted each time the picker is shown.
    pub const fn new(default: u8) -> Self {
        Self {
            open: AtomicBool::new(false),
            pos: AtomicU8::new(default),
            chosen: AtomicU8::new(default),
            default,
        }
    }

    /// Open the overlay with the default row highlighted.
    pub fn show(&self) {
        self.pos.store(self.default, Ordering::Relaxed);
        self.open.store(true, Ordering::Relaxed);
    }

    pub fn is_open(&self) -> bool {
        self.open.load(Ordering::Relaxed)
    }

    /// Move the highlight up (clamped at the first row).
    pub fn up(&self) {
        let p = self.pos.load(Ordering::Relaxed);
        if p > 0 {
            self.pos.store(p - 1, Ordering::Relaxed);
        }
    }

    /// Move the highlight down (clamped at the last of `n` rows).
    pub fn down(&self, n: u8) {
        let p = self.pos.load(Ordering::Relaxed);
        if p + 1 < n {
            self.pos.store(p + 1, Ordering::Relaxed);
        }
    }

    /// Accept the highlighted row, close the overlay and return it.
    pub fn confirm(&self) -> u8 {
        let p = self.pos.load(Ordering::Relaxed);
        self.chosen.store(p, Ordering::Relaxed);
        self.open.store(false, Ordering::Relaxed);
        p
    }

    /// The difficulty confirmed for the current game.
    pub fn chosen(&self) -> u8 {
        self.chosen.load(Ordering::Relaxed)
    }

    /// Draw the overlay if it is open.
    pub fn draw<D>(&self, display: &mut D, title: &str, items: &[&str]) -> Result<(), D::Error>
    where
        D: DrawTarget<Color = TriColor>,
    {
        if self.is_open() {
            ui::draw_picker_menu(display, title, items, self.pos.load(Ordering::Relaxed) as usize)?;
        }
        Ok(())
    }
}
