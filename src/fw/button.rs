use embassy_nrf::gpio::Input;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::watch::{Sender, Watch};

use crate::menu::ButtonId;
use crate::{DISPLAY_STATE, update_health};

macro_rules! update_button_health {
    ($pin:expr, $field:ident) => {
        if $pin.is_low() {
            update_health!(|f| f.buttons.$field.seen_low = true);
        } else {
            update_health!(|f| f.buttons.$field.seen_high = true);
        }
    };
}

pub static BTN_WATCH: Watch<CriticalSectionRawMutex, u8, 2> = Watch::new();

pub async fn run_buttons(
    mut btn_can: Input<'_>,
    mut btn_exe: Input<'_>,
    mut joy_up: Input<'_>,
    mut joy_down: Input<'_>,
    mut joy_left: Input<'_>,
    mut joy_right: Input<'_>,
    mut joy_fire: Input<'_>,
) {
    let btn_sender: Sender<CriticalSectionRawMutex, u8, 2> = BTN_WATCH.sender();
    loop {
        let (_btn, index) = embassy_futures::select::select_array([
            btn_can.wait_for_any_edge(),
            btn_exe.wait_for_any_edge(),
            joy_up.wait_for_any_edge(),
            joy_down.wait_for_any_edge(),
            joy_left.wait_for_any_edge(),
            joy_right.wait_for_any_edge(),
            joy_fire.wait_for_any_edge(),
        ])
        .await;

        let Some(btn) = ButtonId::from_index(index) else {
            continue;
        };

        // Only act on button-down (active low).
        let is_low = match btn {
            ButtonId::Cancel => btn_can.is_low(),
            ButtonId::Execute => btn_exe.is_low(),
            ButtonId::Up => joy_up.is_low(),
            ButtonId::Down => joy_down.is_low(),
            ButtonId::Left => joy_left.is_low(),
            ButtonId::Right => joy_right.is_low(),
            ButtonId::Fire => joy_fire.is_low(),
        };

        // Update health diagnostics on every edge.
        update_health_for(
            btn, &btn_can, &btn_exe, &joy_up, &joy_down, &joy_left, &joy_right, &joy_fire,
        );

        if is_low {
            // Hidden display-flush combo watches every press globally (not
            // just on the game screen, since ghosting isn't game-specific)
            // and only intercepts the triggering press on a full match —
            // same "background watcher" contract as the game's debug-cheat
            // sequence.
            if crate::display_flush::feed(btn) {
                crate::FORCE_FLUSH_PENDING.store(true, core::sync::atomic::Ordering::Relaxed);
            } else {
                // Let the game handle the button first when its screen is active.
                #[cfg(feature = "game")]
                let consumed = {
                    let on_game =
                        DISPLAY_STATE.lock(|f| f.borrow().active_screen()) == crate::SCREEN_GAME;
                    on_game && crate::game::input::dispatch(btn)
                };
                #[cfg(not(feature = "game"))]
                let consumed = false;

                if !consumed {
                    DISPLAY_STATE.lock(|f| f.borrow_mut().dispatch_button(btn));
                }
            }
        }

        btn_sender.send(index as u8);
    }
}

fn update_health_for(
    btn: ButtonId,
    btn_can: &Input<'_>,
    btn_exe: &Input<'_>,
    joy_up: &Input<'_>,
    joy_down: &Input<'_>,
    joy_left: &Input<'_>,
    joy_right: &Input<'_>,
    joy_fire: &Input<'_>,
) {
    match btn {
        ButtonId::Cancel => update_button_health!(btn_can, cancel),
        ButtonId::Execute => update_button_health!(btn_exe, execute),
        ButtonId::Up => update_button_health!(joy_up, up),
        ButtonId::Down => update_button_health!(joy_down, down),
        ButtonId::Left => update_button_health!(joy_left, left),
        ButtonId::Right => update_button_health!(joy_right, right),
        ButtonId::Fire => update_button_health!(joy_fire, fire),
    }
}
