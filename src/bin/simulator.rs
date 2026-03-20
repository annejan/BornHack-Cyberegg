#![cfg(feature = "simulator")]
extern crate embedded_graphics as eg;
extern crate embedded_graphics_simulator as simulator;

use hello_graphics::{
    DISPLAY_STATE, DisplayState, TriColor, draw_graphics, with_display_state_mut,
};

use eg::pixelcolor::Rgb888;
use eg::prelude::*;
use embedded_graphics_simulator::{
    OutputSettings, SimulatorDisplay, SimulatorEvent, Window, sdl2::Keycode,
};

/// Adapter that translates TriColor draw calls to an Rgb888 SimulatorDisplay
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

fn main() -> Result<(), core::convert::Infallible> {
    println!("Hello, world!");

    let mut display: SimulatorDisplay<Rgb888> = SimulatorDisplay::new(Size::new(152, 152));
    let mut window = Window::new("Hello Graphics", &OutputSettings::default());

    display.clear(Rgb888::new(255, 255, 255)).unwrap();
    draw_graphics(
        &mut TriColorDisplay(&mut display),
        "test123\ntest",
        String::from("100%").as_ref(),
    )
    .unwrap();

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
                SimulatorEvent::KeyDown { keycode, .. } => match keycode {
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
                    _ => {}
                },
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
            display.clear(Rgb888::new(255, 255, 255)).unwrap();
            draw_graphics(&mut TriColorDisplay(&mut display), "test123\ntest", 100)?;
            need_redraw = false;
        }
    }

    Ok(())
}
