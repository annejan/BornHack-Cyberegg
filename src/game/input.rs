//! BornPets game-screen input routing.
//!
//! [`dispatch`] is the single entry point for all button events while the game
//! screen is active.  It handles the full lifecycle:
//! - **Not started**: only Fire starts a new game.
//! - **Hatching**: all input blocked (countdown running).
//! - **Gone**: only Execute starts a new generation.
//! - **Active**: icon navigation + modal interaction.

use super::engine::to_display::DisplayAnim;
use super::nav::{NavResult, get_nav, nav_down, nav_left, nav_right, nav_up};
use super::{lifecycle, modal};
use crate::menu::ButtonId;

/// Route a button press on the game screen.
///
/// Returns `true` if the game layer consumed the event.
/// Returns `false` if the caller should forward to the menu
/// (e.g. `Right` at the grid edge to advance to the next screen).
pub fn dispatch(btn: ButtonId) -> bool {
    // ── Text entry (pet naming): pass through to menu handler ────────
    if crate::text_entry::is_active() {
        return false;
    }

    // ── Rolled-stats view: any button closes it ────────────────────────
    if super::traits_view::is_active() {
        super::traits_view::close();
        return true;
    }

    // ── Pet selection screen ────────────────────────────────────────────
    // Cancel is intentionally ignored here — picking a pet is mandatory
    // for the game to start, so closing the screen without a confirmed
    // selection would leave the game in limbo.
    if super::pet_select::is_active() {
        match btn {
            ButtonId::Up => super::pet_select::cursor_up(),
            ButtonId::Down => super::pet_select::cursor_down(),
            ButtonId::Fire | ButtonId::Execute => super::pet_select::confirm(),
            _ => {}
        }
        return true;
    }

    // ── Not started: Fire opens pet selection ────────────────────────
    if !lifecycle::is_started() {
        if btn == ButtonId::Fire {
            super::pet_select::open_new_game();
            return true;
        }
        // Let Left/Right pass through to switch screens.
        return matches!(
            btn,
            ButtonId::Up | ButtonId::Down | ButtonId::Cancel | ButtonId::Execute
        );
    }

    // ── Hatching: block game input but allow screen navigation ─────
    let anim = lifecycle::display_anim();
    if matches!(anim, DisplayAnim::Hatching { .. }) {
        return matches!(
            btn,
            ButtonId::Up | ButtonId::Down | ButtonId::Execute | ButtonId::Fire | ButtonId::Cancel
        );
    }

    // ── Gone: Fire opens pet selection for new generation ────────────
    if anim == DisplayAnim::Gone {
        if btn == ButtonId::Fire || btn == ButtonId::Execute {
            super::pet_select::open_new_generation();
            return true;
        }
        // Let Left/Right pass through to switch screens.
        return matches!(btn, ButtonId::Up | ButtonId::Down | ButtonId::Cancel);
    }

    if super::maze::is_active() {
        match btn {
            ButtonId::Cancel => super::maze::close(),
            ButtonId::Up => super::maze::move_up(),
            ButtonId::Down => super::maze::move_down(),
            ButtonId::Left => super::maze::move_left(),
            ButtonId::Right => super::maze::move_right(),
            ButtonId::Fire | ButtonId::Execute => super::maze::activate(),
        }
        return true;
    }

    // ── Triple Born mini-game ─────────────────────────────────────────
    if super::tripleborn::is_active() {
        match btn {
            ButtonId::Cancel => super::tripleborn::close(),
            ButtonId::Up => super::tripleborn::cursor_up(),
            ButtonId::Down => super::tripleborn::cursor_down(),
            ButtonId::Left => super::tripleborn::cursor_left(),
            ButtonId::Right => super::tripleborn::cursor_right(),
            ButtonId::Fire => super::tripleborn::activate(),
            ButtonId::Execute => super::tripleborn::swap_stash(),
        }
        return true;
    }

    // ── Lights Out mini-game ──────────────────────────────────────────
    if super::lightsout::is_active() {
        match btn {
            ButtonId::Cancel => super::lightsout::close(),
            ButtonId::Up => super::lightsout::cursor_up(),
            ButtonId::Down => super::lightsout::cursor_down(),
            ButtonId::Left => super::lightsout::cursor_left(),
            ButtonId::Right => super::lightsout::cursor_right(),
            ButtonId::Fire | ButtonId::Execute => {
                super::lightsout::activate();
            }
        }
        return true;
    }

    // ── Black Hole mini-game ──────────────────────────────────────────
    if super::blackhole::is_active() {
        match btn {
            ButtonId::Cancel => super::blackhole::close(),
            ButtonId::Up => super::blackhole::cursor_up(),
            ButtonId::Down => super::blackhole::cursor_down(),
            ButtonId::Left => super::blackhole::cursor_left(),
            ButtonId::Right => super::blackhole::cursor_right(),
            ButtonId::Fire | ButtonId::Execute => {
                super::blackhole::activate();
            }
        }
        return true;
    }

    // ── Nim mini-game ──────────────────────────────────────────────────
    if super::nim::is_active() {
        match btn {
            ButtonId::Cancel => super::nim::close(),
            ButtonId::Up => super::nim::cursor_up(),
            ButtonId::Down => super::nim::cursor_down(),
            ButtonId::Left => super::nim::cursor_left(),
            ButtonId::Right => super::nim::cursor_right(),
            ButtonId::Fire | ButtonId::Execute => {
                super::nim::activate();
            }
        }
        return true;
    }

    // ── Tic-tac-toe mini-game ──────────────────────────────────────────
    if super::tictactoe::is_active() {
        match btn {
            ButtonId::Cancel => super::tictactoe::close(),
            ButtonId::Up => super::tictactoe::cursor_up(),
            ButtonId::Down => super::tictactoe::cursor_down(),
            ButtonId::Left => super::tictactoe::cursor_left(),
            ButtonId::Right => super::tictactoe::cursor_right(),
            ButtonId::Fire | ButtonId::Execute => {
                super::tictactoe::place();
            }
        }
        return true;
    }

    // ── Active: modal or icon navigation ─────────────────────────────
    if modal::is_open() {
        match btn {
            ButtonId::Cancel => modal::close(),
            ButtonId::Up => modal::cursor_up(),
            ButtonId::Down => modal::cursor_down(),
            ButtonId::Fire | ButtonId::Execute => modal::activate(),
            ButtonId::Left | ButtonId::Right => {}
        }
        true
    } else {
        match btn {
            ButtonId::Up => {
                nav_up();
                true
            }
            ButtonId::Down => {
                nav_down();
                true
            }
            ButtonId::Left => {
                nav_left();
                true
            }
            ButtonId::Right => matches!(nav_right(), NavResult::Moved),
            ButtonId::Fire | ButtonId::Execute => {
                let nav = get_nav();
                let kind = modal::kind_for_icon(nav.row, nav.col);
                if kind != modal::ModalKind::None {
                    modal::open(kind);
                }
                true
            }
            ButtonId::Cancel => true,
        }
    }
}
