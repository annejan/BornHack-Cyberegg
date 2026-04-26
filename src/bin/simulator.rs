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
    let mut display: SimulatorDisplay<Rgb888> = SimulatorDisplay::new(Size::new(152, 152));
    let mut window = Window::new("BornPets simulator", &OutputSettings::default());

    let health_str = "sim";
    let bat_prc: u8 = 85;

    let mut need_redraw = true;

    'running: loop {
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
                    if keycode == Keycode::Escape {
                        break 'running;
                    }
                    if let Some(btn) = key_to_button(keycode) {
                        handle_button(btn);
                        need_redraw = true;
                    }
                }
                _ => {}
            }
        }
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
