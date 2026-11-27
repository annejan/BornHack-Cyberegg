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

    // ── Battle screen: owns its own input logic across its states. It is
    // ahead of the other views, so a challenge's Fire and Cancel reach it. ──
    if super::battle_view::is_active() {
        super::battle_view::handle_input(btn);
        return true;
    }

    // ── Rolled-stats view: any button closes it ────────────────────────
    if super::traits_view::is_active() {
        super::traits_view::close();
        return true;
    }

    // ── Diary: Left / Right flip the page, any other button closes ──────
    if super::diary_view::is_active() {
        match btn {
            ButtonId::Left | ButtonId::Right => super::diary_view::flip(),
            _ => super::diary_view::close(),
        }
        return true;
    }

    // ── Friends view: owns its own input logic across its two states ────
    if super::friends_view::is_active() {
        super::friends_view::handle_input(btn);
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

    // ── Not started: Fire or Execute opens pet selection ─────────────
    // Execute used to be swallowed here without doing anything, which
    // left the start screen looking frozen for anyone reaching for the
    // button the rest of the UI uses to confirm.
    if !lifecycle::is_started() {
        if btn == ButtonId::Fire || btn == ButtonId::Execute {
            super::pet_select::open_new_game();
            return true;
        }
        // Let Left/Right pass through to switch screens.
        return matches!(btn, ButtonId::Up | ButtonId::Down | ButtonId::Cancel);
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

    // ── BornJeweled mini-game ─────────────────────────────────────────
    if super::bornjeweled::is_active() {
        super::bornjeweled::dispatch(btn);
        return true;
    }

    // ── Lights Out mini-game ──────────────────────────────────────────
    if super::lightsout::is_active() {
        super::lightsout::dispatch(btn);
        return true;
    }

    // ── Black Hole mini-game ──────────────────────────────────────────
    if super::blackhole::is_active() {
        super::blackhole::dispatch(btn);
        return true;
    }

    // ── Tic-tac-toe mini-game ──────────────────────────────────────────
    if super::tictactoe::is_active() {
        super::tictactoe::dispatch(btn);
        return true;
    }

    // ── Active: modal or icon navigation ─────────────────────────────
    if modal::is_open() {
        match btn {
            // On the hint view Cancel steps back to the list; a second one
            // closes the modal, as on every popover view.
            ButtonId::Cancel => {
                if !modal::dismiss_hint() {
                    modal::close();
                }
            }
            ButtonId::Up => modal::cursor_up(),
            ButtonId::Down => modal::cursor_down(),
            ButtonId::Fire | ButtonId::Execute => modal::activate(),
            ButtonId::Left | ButtonId::Right => modal::flip_page(),
        }
        true
    } else {
        // Feed every idle-screen press to the hidden debug-cheat
        // sequence tracker. On a match, open the Debug menu and
        // consume the triggering press so it doesn't also move the nav
        // cursor or activate whatever icon happened to be focused.
        if super::debug_cheats::feed(btn) {
            modal::open(modal::ModalKind::Debug);
            return true;
        }
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
