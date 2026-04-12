//! BornPets game-screen input routing.
//!
//! [`dispatch`] is the single entry point for all button events while the game
//! screen is active.  It handles the full lifecycle:
//! - **Not started**: only Fire starts a new game.
//! - **Hatching**: all input blocked (countdown running).
//! - **Gone**: only Execute starts a new generation.
//! - **Active**: icon navigation + modal interaction.

use crate::menu::ButtonId;
use super::{lifecycle, modal};
use super::engine::to_display::DisplayAnim;
use super::nav::{get_nav, nav_down, nav_left, nav_right, nav_up, NavResult};

/// Route a button press on the game screen.
///
/// Returns `true` if the game layer consumed the event.
/// Returns `false` if the caller should forward to the menu
/// (e.g. `Right` at the grid edge to advance to the next screen).
pub fn dispatch(btn: ButtonId) -> bool {
    // ── Not started: only Fire starts the game ───────────────────────
    if !lifecycle::is_started() {
        if btn == ButtonId::Fire {
            lifecycle::start_new_game();
            return true;
        }
        // Let Left/Right pass through to switch screens.
        return matches!(btn, ButtonId::Up | ButtonId::Down | ButtonId::Cancel | ButtonId::Execute);
    }

    // ── Hatching: all input blocked ──────────────────────────────────
    let anim = lifecycle::display_anim();
    if matches!(anim, DisplayAnim::Hatching { .. }) {
        return true; // consume everything, do nothing
    }

    // ── Gone: Execute starts a new generation ────────────────────────
    if anim == DisplayAnim::Gone {
        if btn == ButtonId::Execute {
            lifecycle::new_generation();
            return true;
        }
        // Let Left/Right pass through to switch screens.
        return matches!(btn, ButtonId::Up | ButtonId::Down | ButtonId::Cancel | ButtonId::Fire);
    }

    // ── Active: modal or icon navigation ─────────────────────────────
    if modal::is_open() {
        match btn {
            ButtonId::Cancel             => modal::close(),
            ButtonId::Up                 => modal::cursor_up(),
            ButtonId::Down               => modal::cursor_down(),
            ButtonId::Fire | ButtonId::Execute => modal::activate(),
            ButtonId::Left | ButtonId::Right   => {}
        }
        true
    } else {
        match btn {
            ButtonId::Up    => { nav_up();   true }
            ButtonId::Down  => { nav_down(); true }
            ButtonId::Left  => { nav_left(); true }
            ButtonId::Right => matches!(nav_right(), NavResult::Moved),
            ButtonId::Fire | ButtonId::Execute => {
                let nav  = get_nav();
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
