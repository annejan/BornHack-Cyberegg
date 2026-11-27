#![cfg(feature = "simulator")]
extern crate embedded_graphics as eg;
extern crate embedded_graphics_simulator as simulator;

use bornhack_aegg::menu::ButtonId;
use bornhack_aegg::draw_graphics;
use bornhack_aegg::sim_support::{self, TriColorDisplay, install_pets_from_repo_cfg};
use eg::pixelcolor::Rgb888;
use eg::prelude::*;
use embedded_graphics_simulator::sdl2::Keycode;
use embedded_graphics_simulator::{OutputSettings, SimulatorDisplay, SimulatorEvent, Window};

/// Map SDL keycode to ButtonId.
///
/// Note: Escape is intercepted by `embedded-graphics-simulator` itself
/// and surfaces as `SimulatorEvent::Quit` — it never reaches this
/// match — so it isn't usable as an in-app key.  Use Backspace for
/// Cancel.
fn key_to_button(k: Keycode) -> Option<ButtonId> {
    match k {
        Keycode::Up => Some(ButtonId::Up),
        Keycode::Down => Some(ButtonId::Down),
        Keycode::Left => Some(ButtonId::Left),
        Keycode::Right => Some(ButtonId::Right),
        Keycode::Return => Some(ButtonId::Fire),
        Keycode::Space => Some(ButtonId::Fire),
        Keycode::Backspace => Some(ButtonId::Cancel),
        Keycode::E => Some(ButtonId::Execute),
        _ => None,
    }
}

fn main() -> Result<(), core::convert::Infallible> {
    use std::time::{Duration, Instant};

    #[cfg(feature = "game")]
    install_pets_from_repo_cfg();

    let mut display: SimulatorDisplay<Rgb888> = SimulatorDisplay::new(Size::new(152, 152));
    let mut window = Window::new("BornPets simulator", &OutputSettings::default());

    let health_str = "sim";
    let bat_prc: u8 = 85;

    // ~5 fps redraw cadence — fast enough for visible sprite animation
    // (firmware advances frames roughly every 1.5 s) but slow enough to
    // keep CPU use trivial.  The wall-clock used by `now_tick` ticks
    // independently of the redraw rate, so stat decay and hatching
    // progress at firmware speed regardless of this number.
    let frame_period = Duration::from_millis(200);
    let mut next_frame = Instant::now() + frame_period;
    let mut need_redraw = true;
    // F12 saves the live screen for the guide: how many have been saved.
    let mut manual_shots = 0u32;

    'running: loop {
        let now = Instant::now();
        if now >= next_frame {
            next_frame = now + frame_period;

            // Run one game cycle so engine timers (stat decay, action
            // cooldowns, hatching countdown, leaving timer) progress.
            // On hardware this is invoked from the embassy display loop
            // and from a few in-renderer call sites; here we drive it
            // ourselves at the sim frame rate.
            #[cfg(feature = "game")]
            let _ = bornhack_aegg::game::lifecycle::cycle();

            // Always redraw at frame rate so any time-driven animation
            // (sprite frame cycle, watch face minute roll, hatching
            // countdown text) stays current.
            need_redraw = true;
        }

        if need_redraw {
            display.clear(Rgb888::new(255, 255, 255)).unwrap();
            draw_graphics(&mut TriColorDisplay(&mut display), health_str, &bat_prc).unwrap();
            need_redraw = false;
        }

        window.update(&display);

        for event in window.events() {
            match event {
                SimulatorEvent::Quit => break 'running,
                SimulatorEvent::KeyDown {
                    keycode: Keycode::F12,
                    ..
                } => {
                    let path = std::path::PathBuf::from(concat!(
                        env!("CARGO_MANIFEST_DIR"),
                        "/guide/build/screens"
                    ))
                    .join(format!("manual-{manual_shots}.png"));
                    let saved = std::fs::create_dir_all(path.parent().unwrap())
                        .map_err(|e| e.to_string())
                        .and_then(|()| sim_support::save_png(&display, &path));
                    match saved {
                        Ok(()) => {
                            println!("saved {}", path.display());
                            manual_shots += 1;
                        }
                        Err(e) => eprintln!("F12: {e}"),
                    }
                }
                SimulatorEvent::KeyDown { keycode, .. } => {
                    if let Some(btn) = key_to_button(keycode) {
                        handle_button(btn);
                        need_redraw = true;
                    }
                }
                _ => {}
            }
        }

        // Yield CPU briefly so we don't busy-spin between events.
        std::thread::sleep(Duration::from_millis(16));
    }

    Ok(())
}

/// Route a button press: flush combo, game (if active), then menu.
fn handle_button(btn: ButtonId) {
    bornhack_aegg::menu::dispatch_press(btn);
}
