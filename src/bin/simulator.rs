#![cfg(feature = "simulator")]
extern crate embedded_graphics as eg;
extern crate embedded_graphics_simulator as simulator;

use eg::pixelcolor::Rgb888;
use eg::prelude::*;
use embedded_graphics_simulator::sdl2::Keycode;
use embedded_graphics_simulator::{OutputSettings, SimulatorDisplay, SimulatorEvent, Window};
use hello_graphics::menu::ButtonId;
use hello_graphics::{DISPLAY_STATE, TriColor, draw_graphics, with_display_state_mut};

/// Adapter that translates TriColor draw calls to an Rgb888 SimulatorDisplay.
struct TriColorDisplay<'a>(&'a mut SimulatorDisplay<Rgb888>);

impl DrawTarget for TriColorDisplay<'_> {
    type Color = TriColor;
    type Error = core::convert::Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<TriColor>>,
    {
        self.0
            .draw_iter(
                pixels
                    .into_iter()
                    .map(|Pixel(p, c)| Pixel(p, Rgb888::from(c))),
            )
            .unwrap();
        Ok(())
    }
}

impl OriginDimensions for TriColorDisplay<'_> {
    fn size(&self) -> Size {
        self.0.size()
    }
}

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
            let _ = hello_graphics::game::lifecycle::cycle();

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

        window.update(&mut display);

        for event in window.events() {
            match event {
                SimulatorEvent::Quit => break 'running,
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

/// Route a button press: game first (if active), then menu.
fn handle_button(btn: ButtonId) {
    #[cfg(feature = "game")]
    {
        let on_game = hello_graphics::with_display_state!(|s| s.active_screen())
            == hello_graphics::SCREEN_GAME;
        if on_game && hello_graphics::game::input::dispatch(btn) {
            return;
        }
    }
    with_display_state_mut!(|s| s.dispatch_button(btn));
}
