use crate::{DISPLAY_STATE, update_health};
use crate::fw::game::input::{GameBtn, dispatch};
use embassy_nrf::gpio::Input;
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, watch::Sender, watch::Watch};

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

        // Only act on button-down (active low).
        let is_low = match index {
            0 => btn_can.is_low(),
            1 => btn_exe.is_low(),
            2 => joy_up.is_low(),
            3 => joy_down.is_low(),
            4 => joy_left.is_low(),
            5 => joy_right.is_low(),
            6 => joy_fire.is_low(),
            _ => false,
        };

        if !is_low {
            // Update health diagnostics on any edge, then skip the action.
            match index {
                0 => update_button_health!(btn_can, cancel),
                1 => update_button_health!(btn_exe, execute),
                2 => update_button_health!(joy_up, up),
                3 => update_button_health!(joy_down, down),
                4 => update_button_health!(joy_left, left),
                5 => update_button_health!(joy_right, right),
                6 => update_button_health!(joy_fire, fire),
                _ => {}
            }
            continue;
        }

        let on_game = DISPLAY_STATE.lock(|f| f.borrow().active_screen()) == 0;

        match index {
            0 => {
                defmt::info!("Cancel");
                if on_game {
                    dispatch(GameBtn::Cancel);
                } else {
                    DISPLAY_STATE.lock(|f| f.borrow_mut().on_cancel());
                }
                btn_sender.send(0);
                update_button_health!(btn_can, cancel);
            }
            1 => {
                defmt::info!("Execute");
                if on_game {
                    dispatch(GameBtn::Execute);
                }
                // Execute has no role in the non-game menu currently.
                btn_sender.send(1);
                update_button_health!(btn_exe, execute);
            }
            2 => {
                defmt::info!("Up");
                if on_game {
                    dispatch(GameBtn::Up);
                } else {
                    DISPLAY_STATE.lock(|f| f.borrow_mut().menu_up());
                }
                btn_sender.send(2);
                update_button_health!(joy_up, up);
            }
            3 => {
                defmt::info!("Down");
                if on_game {
                    dispatch(GameBtn::Down);
                } else {
                    DISPLAY_STATE.lock(|f| f.borrow_mut().menu_down());
                }
                btn_sender.send(3);
                update_button_health!(joy_down, down);
            }
            4 => {
                defmt::info!("Left");
                if on_game {
                    dispatch(GameBtn::Left);
                } else {
                    DISPLAY_STATE.lock(|f| f.borrow_mut().screen_left());
                }
                btn_sender.send(4);
                update_button_health!(joy_left, left);
            }
            5 => {
                defmt::info!("Right");
                if on_game {
                    // dispatch returns false when the cursor is at the grid edge.
                    let consumed = dispatch(GameBtn::Right);
                    if !consumed {
                        DISPLAY_STATE.lock(|f| f.borrow_mut().screen_right());
                    }
                } else {
                    DISPLAY_STATE.lock(|f| f.borrow_mut().screen_right());
                }
                btn_sender.send(5);
                update_button_health!(joy_right, right);
            }
            6 => {
                defmt::info!("Fire");
                if on_game {
                    dispatch(GameBtn::Fire);
                } else {
                    DISPLAY_STATE.lock(|f| f.borrow_mut().fire());
                }
                btn_sender.send(6);
                update_button_health!(joy_fire, fire);
            }
            _ => unreachable!(),
        }
    }
}
