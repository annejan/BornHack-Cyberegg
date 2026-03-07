#![cfg_attr(feature = "embassy", no_std)]
#![cfg_attr(feature = "embassy", no_main)]

#[cfg(feature = "embassy")]
pub mod fw;

use core::cell::{Ref, RefCell};

use core::result::{Result, Result::Ok};
use embedded_graphics::{
    mono_font::{MonoTextStyle, ascii::FONT_10X20},
    pixelcolor::BinaryColor,
    prelude::*,
    primitives::{Circle, PrimitiveStyle, Rectangle},
    text::{Alignment, Baseline, Text, TextStyleBuilder},
};

// Conditional imports based on feature
#[cfg(feature = "embassy")]
use embassy_sync::blocking_mutex::{Mutex, raw::ThreadModeRawMutex};

#[cfg(feature = "simulator")]
use std::sync::Mutex;

pub const FOREGROUND_COLOR: BinaryColor = BinaryColor::Off;
pub const BACKGROUND_COLOR: BinaryColor = BinaryColor::On;

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

/// Draw your graphics to any display that implements DrawTarget
pub fn draw_graphics<D>(display: &mut D, health_str: &str) -> Result<(), D::Error>
where
    D: DrawTarget<Color = BinaryColor>,
{
    // Clear the display, all white
    let _ = display.clear(BinaryColor::On);
    let centered = TextStyleBuilder::new()
        .baseline(Baseline::Middle)
        .alignment(Alignment::Center)
        .build();

    let position = Point::new(76, 76);
    Circle::with_center(position, 125)
        .into_styled(PrimitiveStyle::with_fill(FOREGROUND_COLOR))
        .draw(display)?;

    // Bottom 20 pixels of the screen white using rectangle
    Rectangle::new(Point::new(0, 108), Size::new(152, 44))
        .into_styled(PrimitiveStyle::with_fill(BACKGROUND_COLOR))
        .draw(display)?;
    Rectangle::new(Point::new(0, 108), Size::new(152, 44))
        .into_styled(PrimitiveStyle::with_stroke(FOREGROUND_COLOR, 2))
        .draw(display)?;

    // Put text "HELLO GRAPHICS" on the display, centered in white
    let text_style = MonoTextStyle::new(&FONT_10X20, BinaryColor::On);
    let text_style_inverted = MonoTextStyle::new(&FONT_10X20, BinaryColor::Off);
    let item_text =
        // DISPLAY_STATE.lock(|f| -> &'static str { f.borrow().get_current_menu_item().unwrap() });
        with_display_state!(| state: &Ref<'_, DisplayState<3>> | state.get_current_menu_item().unwrap());
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
