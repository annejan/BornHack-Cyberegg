#![cfg_attr(feature = "embassy", no_std)]
#![cfg_attr(feature = "embassy", no_main)]

#[cfg(feature = "embassy")]
pub mod fw;

use core::cell::{Ref, RefCell};

use core::result::{Result, Result::Ok};
use core::sync::atomic::{AtomicU32, Ordering};
use embedded_graphics::{
    mono_font::{MonoTextStyle, ascii::FONT_10X20},
    prelude::*,
    primitives::{Circle, PrimitiveStyle, Rectangle},
    text::{Alignment, Baseline, Text, TextStyleBuilder},
};
#[cfg(feature = "embassy")]
use fw::device_id::get_bytes as get_device_id;
#[cfg(feature = "simulator")]
fn get_device_id() -> [u8; 4] {
    *b"A3F7"
}
#[cfg(feature = "embassy")]
use heapless::format;
// Embassy: re-export TriColor from ssd1680 hardware driver
#[cfg(feature = "embassy")]
pub use ssd1680::graphics::{BLACK, RED, TriColor, WHITE};

// Simulator: define TriColor locally
#[cfg(feature = "simulator")]
mod tricolor {
    use embedded_graphics::pixelcolor::{Rgb888, raw::RawU2};
    use embedded_graphics::prelude::PixelColor;

    #[derive(Clone, Copy, PartialEq, Eq)]
    pub enum TriColor {
        Black,
        White,
        Chromatic,
    }

    impl PixelColor for TriColor {
        type Raw = RawU2;
    }

    pub const WHITE: TriColor = TriColor::White;
    pub const BLACK: TriColor = TriColor::Black;
    pub const RED: TriColor = TriColor::Chromatic;

    impl From<TriColor> for Rgb888 {
        fn from(c: TriColor) -> Self {
            match c {
                TriColor::White => Rgb888::new(255, 255, 255),
                TriColor::Black => Rgb888::new(0, 0, 0),
                TriColor::Chromatic => Rgb888::new(255, 0, 0),
            }
        }
    }
}

#[cfg(feature = "simulator")]
pub use tricolor::{BLACK, RED, TriColor, WHITE};

// Conditional imports based on feature
#[cfg(feature = "embassy")]
use embassy_sync::blocking_mutex::{Mutex, raw::ThreadModeRawMutex};
// #[cfg(feature = "embassy")]
// use trouble_host::prelude::ad_types::DEVICE_ID;

#[cfg(feature = "simulator")]
use std::sync::Mutex;

// Have a struct here that tracks the state of the display
// this struct needs to be async safe
#[derive()]
pub struct DisplayState<const N: usize> {
    // Add fields here to track the state of the display
    // e.g., button states, current screen, etc.
    menu_items: [&'static str; N],
    menu_pos: u8,
    fire_button: bool,
}

// Dead code allowed in this block
#[allow(dead_code)]
impl<const N: usize> DisplayState<N> {
    pub fn set_menu_pos(&mut self, pos: u8) {
        self.menu_pos = pos;
    }

    pub fn get_menu_pos(&self) -> u8 {
        self.menu_pos
    }

    pub fn menu_up(&mut self) {
        if self.menu_pos > 0 {
            self.menu_pos -= 1;
        }
    }

    pub fn menu_down(&mut self) {
        if self.menu_pos + 1 < N as u8 {
            self.menu_pos += 1;
        }
    }

    pub fn set_fire_button(&mut self, fire: bool) {
        self.fire_button = fire;
    }

    pub fn get_fire_button(&self) -> bool {
        self.fire_button
    }

    pub fn get_current_menu_item(&self) -> Option<&'static str> {
        self.menu_items.get(self.menu_pos as usize).map(|&s| s)
    }

    pub fn get_menu_item(&self, index: usize) -> Option<&'static str> {
        self.menu_items.get(index).map(|&s| s)
    }
}

// Embassy version with ThreadModeRawMutex
#[cfg(feature = "embassy")]
pub static DISPLAY_STATE: Mutex<ThreadModeRawMutex, RefCell<DisplayState<3>>> =
    Mutex::new(RefCell::new(DisplayState {
        menu_items: ["Item 1", "Item 2", "Item 3"],
        menu_pos: 0,
        fire_button: false,
    }));

// Simulator version with std::sync::Mutex
#[cfg(feature = "simulator")]
pub static DISPLAY_STATE: Mutex<RefCell<DisplayState<3>>> =
    Mutex::new(RefCell::new(DisplayState {
        menu_items: ["Item 1", "Item 2", "Item 3"],
        menu_pos: 0,
        fire_button: false,
    }));

// Macro for embassy - immutable access
#[cfg(feature = "embassy")]
#[macro_export]
macro_rules! with_display_state {
    ($f:expr) => {
        DISPLAY_STATE.lock(|cell| $f(&cell.borrow()))
    };
}

// Macro for embassy - mutable access
#[cfg(feature = "embassy")]
#[macro_export]
macro_rules! with_display_state_mut {
    ($f:expr) => {
        DISPLAY_STATE.lock(|cell| $f(&mut cell.borrow_mut()))
    };
}

// Macro for simulator - immutable access
#[cfg(feature = "simulator")]
#[macro_export]
macro_rules! with_display_state {
    ($f:expr) => {{
        let guard = DISPLAY_STATE.lock().unwrap();
        $f(&guard.borrow())
    }};
}

// Macro for simulator - mutable access
#[cfg(feature = "simulator")]
#[macro_export]
macro_rules! with_display_state_mut {
    ($f:expr) => {{
        let guard = DISPLAY_STATE.lock().unwrap();
        $f(&mut guard.borrow_mut())
    }};
}

// Position of the animated circle
static CIRCLE_POS: AtomicU32 = AtomicU32::new(0);

/// Draw your graphics to any display that implements DrawTarget
pub fn draw_graphics<D>(display: &mut D, health_str: &str, bat_prc: &u8) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let circle_post = CIRCLE_POS.load(Ordering::Relaxed);
    CIRCLE_POS.store(circle_post.wrapping_add(1) % 4, Ordering::Relaxed);

    let centered = TextStyleBuilder::new()
        .baseline(Baseline::Middle)
        .alignment(Alignment::Center)
        .build();

    // Animated red dot
    let dot_pos = Point::new(((circle_post * 20) + 15) as i32, 7);
    Circle::with_center(dot_pos, 10)
        .into_styled(PrimitiveStyle::with_fill(BLACK))
        .draw(display)?;

    let position = Point::new(76, 76);
    Circle::with_center(position, 110)
        .into_styled(PrimitiveStyle::with_fill(BLACK))
        .draw(display)?;

    // Bottom banner
    Rectangle::new(Point::new(0, 108), Size::new(152, 44))
        .into_styled(PrimitiveStyle::with_fill(WHITE))
        .draw(display)?;
    Rectangle::new(Point::new(0, 108), Size::new(152, 44))
        .into_styled(PrimitiveStyle::with_stroke(RED, 2))
        .draw(display)?;

    // Text: menu item centered, health status bottom-left
    let text_style = MonoTextStyle::new(&FONT_10X20, WHITE);
    let text_style_inverted = MonoTextStyle::new(&FONT_10X20, BLACK);
    let item_text =
        // DISPLAY_STATE.lock(|f| -> &'static str { f.borrow().get_current_menu_item().unwrap() });
        with_display_state!(| state: &Ref<'_, DisplayState<3>> | state.get_current_menu_item().unwrap());

    let bat_text = format!(4; "{}%", bat_prc).unwrap();
    Text::with_text_style(
        &bat_text,
        Point::new(110, 16),
        text_style_inverted,
        TextStyleBuilder::new().baseline(Baseline::Bottom).build(),
    )
    .draw(display)?;
    let id_text = get_device_id();
    Text::with_text_style(
        unsafe { core::str::from_utf8_unchecked(&id_text) },
        Point::new(110, 30),
        text_style_inverted,
        TextStyleBuilder::new().baseline(Baseline::Bottom).build(),
    )
    .draw(display)?;
    let text = Text::with_text_style(
        item_text,
        display.bounding_box().center(),
        text_style,
        centered,
    );
    text.draw(display)?;
    // Print health status string and print it in the bottom left corner
    let health = Text::with_text_style(
        health_str,
        Point::new(10, 128),
        text_style_inverted,
        TextStyleBuilder::new().baseline(Baseline::Bottom).build(),
    );
    health.draw(display)?;

    Ok(())
}
