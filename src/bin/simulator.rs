#![cfg(feature = "simulator")]
extern crate embedded_graphics as eg;
extern crate embedded_graphics_simulator as simulator;

use hello_graphics::{DISPLAY_STATE, DisplayState, draw_graphics, with_display_state_mut};

use eg::{pixelcolor::BinaryColor, prelude::*};
use embedded_graphics_simulator::{
    OutputSettings, SimulatorDisplay, SimulatorEvent, Window, sdl2::Keycode,
};

fn main() -> Result<(), core::convert::Infallible> {
    println!("Hello, world!");

    let mut display: SimulatorDisplay<BinaryColor> = SimulatorDisplay::new(Size::new(152, 152));
    let mut window = Window::new("Hello Graphics", &OutputSettings::default());

    draw_graphics(&mut display, "test123\ntest").unwrap();

    with_display_state_mut!(|state: &mut DisplayState<3>| {
        state.set_menu_pos(0);
    });

    let mut need_redraw: bool = false;

    'running: loop {
        // Handle window events
        window.update(&mut display);

        for event in window.events() {
            need_redraw = true;
            match event {
                SimulatorEvent::Quit => break 'running,
                SimulatorEvent::KeyDown { keycode, .. } => {
                    match keycode {
                        Keycode::Escape => break 'running,
                        Keycode::Up => {
                            with_display_state_mut!(|state: &mut DisplayState<3>| {
                                println!("Key up");
                                state.menu_up();
                            });
                        }
                        Keycode::Down => {
                            with_display_state_mut!(|state: &mut DisplayState<3>| {
                                println!("Key down");
                                state.menu_down();
                            });
                        }
                        Keycode::Return => {
                            with_display_state_mut!(|state: &mut DisplayState<3>| {
                                state.set_fire_button(true);
                            });
                        }
                        _ => {
                            // Handle other key presses
                            // e.g., change menu position, execute button, etc.
                        }
                    }
                }
                SimulatorEvent::KeyUp { keycode, .. } => match keycode {
                    Keycode::Return => {
                        with_display_state_mut!(|state: &mut DisplayState<3>| {
                            state.set_fire_button(false);
                        });
                    }
                    _ => {}
                },
                _ => {}
            }
        }
        if need_redraw {
            draw_graphics(&mut display, "test123\ntest")?;
            need_redraw = false;
        }
    }

    // All done nothing to see.
    Ok(())
}
