#![cfg_attr(feature = "embassy-base", no_std)]
#![cfg_attr(feature = "embassy-base", no_main)]

#[derive(Debug, PartialEq)]
#[cfg_attr(feature = "embassy-base", derive(defmt::Format))]
pub enum ScreenError {
    NotFound,
    OutOfBounds,
    InvalidScreen,
}

#[cfg(feature = "embassy-base")]
pub mod fw;
#[cfg(feature = "game")]
pub mod game;
pub mod menu;
use core::cell::RefCell;
pub use menu::{DISPLAY_STATE, DisplayState, MenuItem, MenuItemKind, ScreenState, draw_menu};

use core::result::{Result, Result::Ok};
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use embedded_graphics::{
    mono_font::{
        MonoTextStyle,
        ascii::{FONT_7X13, FONT_7X13_BOLD, FONT_10X20},
    },
    prelude::*,
    primitives::{Circle, PrimitiveStyle, Rectangle},
    text::{Alignment, Baseline, Text, TextStyleBuilder},
};
#[cfg(feature = "embassy-base")]
#[cfg(feature = "simulator")]
fn get_device_id() -> [u8; 4] {
    *b"A3F7"
}
use heapless::format;
// Embassy: re-export Color from ssd1675 hardware driver
#[cfg(feature = "embassy-base")]
mod embassy_colors {
    pub use ssd1675::graphics::Color;
    pub use ssd1675::graphics::Color as TriColor;
    pub const BLACK: Color = Color::Black;
    pub const WHITE: Color = Color::White;
    pub const RED: Color = Color::Red;
}
#[cfg(feature = "embassy-base")]
pub use embassy_colors::*;

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
#[cfg(feature = "embassy-base")]
use embassy_sync::blocking_mutex::{
    Mutex,
    raw::CriticalSectionRawMutex,
};
#[cfg(feature = "embassy-base")]
use embassy_sync::signal::Signal;

#[cfg(feature = "simulator")]
use std::sync::Mutex;

/// Boosted RX gain toggle (0x96 vs 0x94 in register 0x08AC). Default: off.
pub static BOOSTED_RX_GAIN: AtomicBool = AtomicBool::new(false);

/// UTC offset in whole hours (-12..=+14). Default: 0 (UTC).
pub static TIMEZONE_OFFSET: core::sync::atomic::AtomicI8 = core::sync::atomic::AtomicI8::new(0);

/// Fired when `TIMEZONE_OFFSET` changes so the BLE task can persist it.
#[cfg(feature = "embassy-base")]
pub static TZ_CHANGED_SIGNAL: Signal<CriticalSectionRawMutex, ()> = Signal::new();

// Re-export mesh types and statics so existing `crate::SomeType` paths keep working.
#[cfg(feature = "mesh")]
pub use fw::mesh::*;

/// Active BLE pairing passkey (6-digit, 0–999999). `u32::MAX` means no pairing in progress.
pub static BLE_PASSKEY: AtomicU32 = AtomicU32::new(u32::MAX);

/// Set to `true` while a BLE companion is connected, `false` on disconnect.
pub static BLE_CONNECTED: AtomicBool = AtomicBool::new(false);

/// Set to `true` when an unread PM arrives; cleared when the PM screen is viewed.
pub static PM_UNREAD: AtomicBool = AtomicBool::new(false);

/// Fired by the BLE task whenever the pairing passkey changes (new passkey or cleared).
#[cfg(feature = "embassy-base")]
pub static BLE_PAIRING_SIGNAL: Signal<CriticalSectionRawMutex, ()> = Signal::new();

/// Fired every minute by `minute_tick_task` so the display redraws the clock.
#[cfg(feature = "embassy-base")]
pub static MINUTE_TICK: Signal<CriticalSectionRawMutex, ()> = Signal::new();

// ---------------------------------------------------------------------------
// Wall clock
// ---------------------------------------------------------------------------

#[cfg(feature = "embassy-base")]
struct WallClock {
    unix_base: u32,
    ticks_base: u64,
}

#[cfg(feature = "embassy-base")]
static WALL_CLOCK: Mutex<CriticalSectionRawMutex, RefCell<Option<WallClock>>> =
    Mutex::new(RefCell::new(None));

/// Called by the BLE task when `SET_DEVICE_TIME` (0x06) is received.
#[cfg(feature = "embassy-base")]
pub fn set_wall_clock(unix_secs: u32) {
    WALL_CLOCK.lock(|cell| {
        *cell.borrow_mut() = Some(WallClock {
            unix_base: unix_secs,
            ticks_base: embassy_time::Instant::now().as_ticks(),
        });
    });
}

/// Current unix time in seconds, or `None` if the clock has never been synced.
#[cfg(feature = "embassy-base")]
pub fn unix_now() -> Option<u32> {
    WALL_CLOCK.lock(|cell| {
        cell.borrow().as_ref().map(|wc| {
            let elapsed = embassy_time::Instant::now()
                .as_ticks()
                .saturating_sub(wc.ticks_base);
            wc.unix_base
                .saturating_add((elapsed / embassy_time::TICK_HZ) as u32)
        })
    })
}

/// MeshCore node name cached from KV for synchronous access by the display renderer.
/// Populated by the BLE task at startup (after reading from flash) and on every
/// SET_ADVERT_NAME update.  Empty until the BLE task has initialized.
#[cfg(feature = "embassy-base")]
pub static NODE_NAME: Mutex<CriticalSectionRawMutex, RefCell<heapless::String<31>>> =
    Mutex::new(RefCell::new(heapless::String::new()));

/// Store `name` (raw UTF-8 bytes) into [`NODE_NAME`].  Invalid UTF-8 is ignored.
#[cfg(feature = "embassy-base")]
pub fn update_node_name(name: &[u8]) {
    if let Ok(s) = core::str::from_utf8(name) {
        NODE_NAME.lock(|cell| {
            let mut stored = cell.borrow_mut();
            stored.clear();
            let _ = stored.push_str(&s[..s.len().min(31)]);
        });
    }
}


// (mesh statics moved to fw/mesh/mod.rs)


// Macro for embassy - immutable access
/// Access the shared `DisplayState` immutably.
/// Usage: `with_display_state!(|s| s.active_screen())`
#[cfg(feature = "embassy-base")]
#[macro_export]
macro_rules! with_display_state {
    ($f:expr) => {
        DISPLAY_STATE.lock(|cell| {
            let state = cell.borrow();
            let f: &dyn Fn(&$crate::menu::DisplayState<{ $crate::menu::SCREEN_COUNT }>) -> _ = &$f;
            f(&state)
        })
    };
}

/// Access the shared `DisplayState` mutably.
/// Usage: `with_display_state_mut!(|s| s.dispatch_button(btn))`
#[cfg(feature = "embassy-base")]
#[macro_export]
macro_rules! with_display_state_mut {
    ($f:expr) => {
        DISPLAY_STATE.lock(|cell| {
            let mut state = cell.borrow_mut();
            let f: &dyn Fn(&mut $crate::menu::DisplayState<{ $crate::menu::SCREEN_COUNT }>) -> _ = &$f;
            f(&mut state)
        })
    };
}

#[cfg(feature = "simulator")]
#[macro_export]
macro_rules! with_display_state {
    ($f:expr) => {{
        let guard = DISPLAY_STATE.lock().unwrap();
        let state = guard.borrow();
        let f: &dyn Fn(&$crate::menu::DisplayState<{ $crate::menu::SCREEN_COUNT }>) -> _ = &$f;
        f(&state)
    }};
}

#[cfg(feature = "simulator")]
#[macro_export]
macro_rules! with_display_state_mut {
    ($f:expr) => {{
        let guard = DISPLAY_STATE.lock().unwrap();
        let mut state = guard.borrow_mut();
        let f: &dyn Fn(&mut $crate::menu::DisplayState<{ $crate::menu::SCREEN_COUNT }>) -> _ = &$f;
        f(&mut state)
    }};
}

// Position of the animated circle
static CIRCLE_POS: AtomicU32 = AtomicU32::new(0);

// Re-export screen indices from ScreenId for convenience.
// The game screen is always at index 0 but disabled when the game feature is off.
// Navigation automatically skips disabled screens.
pub use menu::ScreenId;
pub const SCREEN_GAME:       u8 = ScreenId::Game       as u8;
pub const SCREEN_MAIN:       u8 = ScreenId::Main       as u8;
pub const SCREEN_PM:         u8 = ScreenId::Pm         as u8;
pub const SCREEN_CHANNEL:    u8 = ScreenId::Channel    as u8;
pub const SCREEN_ADVERT:     u8 = ScreenId::Advert     as u8;
pub const SCREEN_BADGERCORN: u8 = ScreenId::Badgercorn as u8;

/// Dispatch to the correct screen renderer based on the active screen.
pub fn draw_graphics<D>(display: &mut D, health_str: &str, bat_prc: &u8) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let active = with_display_state!(|state| state.active_screen());
    match active {
        #[cfg(feature = "game")]
        SCREEN_GAME => game::draw_screen_game(display, game::nav::get_nav()),
        SCREEN_MAIN => draw_screen_main(display, health_str, bat_prc),
        #[cfg(feature = "mesh")]
        SCREEN_PM => draw_screen_pm(display, bat_prc),
        #[cfg(feature = "mesh")]
        SCREEN_CHANNEL => draw_screen_lora(display, bat_prc),
        #[cfg(feature = "mesh")]
        SCREEN_ADVERT => draw_screen_advert(display, bat_prc),
        // SCREEN_BADGERCORN is rendered via blit() in embassy.rs — nothing to draw here.
        _ => draw_screen_main(display, health_str, bat_prc),
    }?;

    // BLE pairing PIN overlay — drawn last so it appears on every screen,
    // including over the game screen and any in-game modal.
    draw_ble_pin_overlay(display)
}

/// Draw the BLE passkey PIN dialog centred on screen.
///
/// Does nothing when no pairing is in progress (`BLE_PASSKEY == u32::MAX`).
/// The double-border box signals urgency and renders on top of all other content.
#[cfg(feature = "embassy-base")]
fn draw_ble_pin_overlay<D>(display: &mut D) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let passkey_val = BLE_PASSKEY.load(Ordering::Relaxed);
    if passkey_val == u32::MAX {
        return Ok(());
    }
    let centered = TextStyleBuilder::new()
        .baseline(Baseline::Middle)
        .alignment(Alignment::Center)
        .build();
    Rectangle::new(Point::new(20, 48), Size::new(112, 62))
        .into_styled(PrimitiveStyle::with_fill(WHITE))
        .draw(display)?;
    Rectangle::new(Point::new(20, 48), Size::new(112, 62))
        .into_styled(PrimitiveStyle::with_stroke(BLACK, 1))
        .draw(display)?;
    Rectangle::new(Point::new(24, 52), Size::new(104, 54))
        .into_styled(PrimitiveStyle::with_stroke(BLACK, 1))
        .draw(display)?;
    Text::with_text_style(
        "BT PIN:",
        Point::new(76, 66),
        MonoTextStyle::new(&FONT_7X13, BLACK),
        centered,
    )
    .draw(display)?;
    let code_str = format!(8; "{:06}", passkey_val).unwrap();
    Text::with_text_style(
        &code_str,
        Point::new(76, 86),
        MonoTextStyle::new(&FONT_10X20, BLACK),
        centered,
    )
    .draw(display)
    .map(|_| ())
}

#[cfg(feature = "simulator")]
fn draw_ble_pin_overlay<D>(_display: &mut D) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    Ok(())
}

fn draw_screen_main<D>(display: &mut D, health_str: &str, bat_prc: &u8) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let circle_post = CIRCLE_POS.load(Ordering::Relaxed);
    CIRCLE_POS.store(circle_post.wrapping_add(1) % 4, Ordering::Relaxed);

    // let centered = TextStyleBuilder::new()
    //     .baseline(Baseline::Middle)
    //     .alignment(Alignment::Center)
    //     .build();

    // Animated red dot
    let dot_pos = Point::new(((circle_post * 20) + 15) as i32, 7);
    Circle::with_center(dot_pos, 10)
        .into_styled(PrimitiveStyle::with_fill(BLACK))
        .draw(display)?;

    // Bottom banner
    Rectangle::new(Point::new(0, 108), Size::new(152, 44))
        .into_styled(PrimitiveStyle::with_fill(WHITE))
        .draw(display)?;
    Rectangle::new(Point::new(0, 108), Size::new(152, 44))
        .into_styled(PrimitiveStyle::with_stroke(RED, 2))
        .draw(display)?;

    let text_style_inverted = MonoTextStyle::new(&FONT_10X20, BLACK);
    let bat_style = MonoTextStyle::new(&FONT_7X13, BLACK);

    let bat_text = format!(4; "{}%", bat_prc).unwrap();
    Text::with_text_style(
        &bat_text,
        Point::new(110, 16),
        bat_style,
        TextStyleBuilder::new().baseline(Baseline::Bottom).build(),
    )
    .draw(display)?;
    #[cfg(feature = "embassy-base")]
    NODE_NAME.lock(|cell| -> Result<(), D::Error> {
        let name = cell.borrow();
        let display_name = if name.is_empty() {
            "<Empty>"
        } else {
            name.as_str()
        };
        Text::with_text_style(
            display_name,
            Point::new(148, 33),
            text_style_inverted,
            TextStyleBuilder::new()
                .baseline(Baseline::Bottom)
                .alignment(Alignment::Right)
                .build(),
        )
        .draw(display)
        .map(|_| ())
    })?;

    let (items, pos) = with_display_state!(|state| {
        let screen = state.current_screen();
        (screen.current_items(), screen.current_pos())
    });
    menu::draw_menu(display, items, pos)?;

    Text::with_text_style(
        health_str,
        Point::new(10, 128),
        text_style_inverted,
        TextStyleBuilder::new().baseline(Baseline::Bottom).build(),
    )
    .draw(display)?;

    #[cfg(feature = "embassy-base")]
    if let Some(unix) = unix_now() {
        let offset_secs = TIMEZONE_OFFSET.load(Ordering::Relaxed) as i64 * 3600;
        let local = (unix as i64 + offset_secs) as u32;
        let h = (local % 86400) / 3600;
        let m = (local % 3600) / 60;
        let time_str = format!(5; "{:02}:{:02}", h, m).unwrap();
        Text::with_text_style(
            &time_str,
            Point::new(148, 148),
            MonoTextStyle::new(&FONT_7X13, BLACK),
            TextStyleBuilder::new()
                .baseline(Baseline::Bottom)
                .alignment(Alignment::Right)
                .build(),
        )
        .draw(display)?;
    }

    Ok(())
}

/// Draw `text` line by line, wrapping at `chars_per_line` characters.
fn draw_wrapped<D>(
    display: &mut D,
    text: &str,
    x: i32,
    y_start: i32,
    line_height: i32,
    chars_per_line: usize,
    style: MonoTextStyle<'_, TriColor>,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let bottom = TextStyleBuilder::new().baseline(Baseline::Bottom).build();
    let mut remaining = text;
    let mut y = y_start;
    while !remaining.is_empty() {
        let split = remaining
            .char_indices()
            .nth(chars_per_line)
            .map(|(i, _)| i)
            .unwrap_or(remaining.len());
        let (line, rest) = remaining.split_at(split);
        Text::with_text_style(line, Point::new(x, y), style, bottom).draw(display)?;
        y += line_height;
        remaining = rest;
    }
    Ok(())
}

#[cfg(feature = "mesh")]
fn draw_screen_pm<D>(display: &mut D, bat_prc: &u8) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let style_bold = MonoTextStyle::new(&FONT_7X13_BOLD, BLACK);
    let style_msg = MonoTextStyle::new(&FONT_7X13, BLACK);
    let style_rssi = MonoTextStyle::new(&FONT_7X13, BLACK);
    let bottom = TextStyleBuilder::new().baseline(Baseline::Bottom).build();

    // Header bar: "Direct Message" + battery
    Text::with_text_style("Direct Message", Point::new(4, 14), style_bold, bottom)
        .draw(display)?;
    let bat_text = format!(4; "{}%", bat_prc).unwrap();
    Text::with_text_style(
        &bat_text,
        Point::new(148, 14),
        style_msg,
        TextStyleBuilder::new()
            .baseline(Baseline::Bottom)
            .alignment(Alignment::Right)
            .build(),
    )
    .draw(display)?;
    Rectangle::new(Point::new(0, 16), Size::new(152, 1))
        .into_styled(PrimitiveStyle::with_fill(BLACK))
        .draw(display)?;

    LAST_PM.lock(|cell| -> Result<(), D::Error> {
        match *cell.borrow() {
            None => {
                Text::with_text_style(
                    "No private messages",
                    display.bounding_box().center(),
                    style_msg,
                    TextStyleBuilder::new()
                        .baseline(Baseline::Middle)
                        .alignment(Alignment::Center)
                        .build(),
                )
                .draw(display)?;
            }
            Some(ref msg) => {
                // Sender name (bold)
                Text::with_text_style(msg.sender_name.as_str(), Point::new(4, 30), style_bold, bottom)
                    .draw(display)?;

                // Divider
                Rectangle::new(Point::new(0, 32), Size::new(152, 1))
                    .into_styled(PrimitiveStyle::with_fill(BLACK))
                    .draw(display)?;

                // Message text wrapped
                draw_wrapped(display, msg.text.as_str(), 4, 46, 14, 21, style_msg)?;

                // RSSI at bottom
                let rssi_text = format!(16; "{} dBm", msg.rssi).unwrap();
                Text::with_text_style(&rssi_text, Point::new(4, 152), style_rssi, bottom)
                    .draw(display)?;
            }
        }
        Ok(())
    })
}

#[cfg(feature = "mesh")]
fn draw_screen_lora<D>(display: &mut D, bat_prc: &u8) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let style_bold = MonoTextStyle::new(&FONT_7X13_BOLD, BLACK);
    let style_msg = MonoTextStyle::new(&FONT_7X13, BLACK);
    let style_rssi = MonoTextStyle::new(&FONT_7X13, BLACK);
    let bottom = TextStyleBuilder::new().baseline(Baseline::Bottom).build();

    // Header bar: "Channel" + battery
    Text::with_text_style("Channel", Point::new(4, 14), style_bold, bottom)
        .draw(display)?;
    let bat_text = format!(4; "{}%", bat_prc).unwrap();
    Text::with_text_style(
        &bat_text,
        Point::new(148, 14),
        style_msg,
        TextStyleBuilder::new()
            .baseline(Baseline::Bottom)
            .alignment(Alignment::Right)
            .build(),
    )
    .draw(display)?;
    Rectangle::new(Point::new(0, 16), Size::new(152, 1))
        .into_styled(PrimitiveStyle::with_fill(BLACK))
        .draw(display)?;

    LAST_LORA_MSG.lock(|cell| -> Result<(), D::Error> {
        match *cell.borrow() {
            None => {
                Text::with_text_style(
                    "No channel messages",
                    display.bounding_box().center(),
                    style_msg,
                    TextStyleBuilder::new()
                        .baseline(Baseline::Middle)
                        .alignment(Alignment::Center)
                        .build(),
                )
                .draw(display)?;
            }
            Some(ref msg) => {
                // Channel name (bold)
                Text::with_text_style(msg.channel.as_str(), Point::new(4, 30), style_bold, bottom)
                    .draw(display)?;

                // Sender nickname (bold)
                Text::with_text_style(msg.sender.as_str(), Point::new(4, 44), style_bold, bottom)
                    .draw(display)?;

                // Divider
                Rectangle::new(Point::new(0, 46), Size::new(152, 1))
                    .into_styled(PrimitiveStyle::with_fill(BLACK))
                    .draw(display)?;

                // Message text wrapped
                draw_wrapped(display, msg.text.as_str(), 4, 60, 14, 21, style_msg)?;

                // RSSI and SNR at bottom
                let rssi_text = format!(24; "{} dBm / {} dB", msg.rssi, msg.snr_x4 / 4).unwrap();
                Text::with_text_style(&rssi_text, Point::new(4, 152), style_rssi, bottom)
                    .draw(display)?;
            }
        }
        Ok(())
    })
}

#[cfg(feature = "mesh")]
fn draw_screen_advert<D>(display: &mut D, bat_prc: &u8) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let style_bold = MonoTextStyle::new(&FONT_7X13_BOLD, BLACK);
    let style_msg = MonoTextStyle::new(&FONT_7X13, BLACK);
    let style_small = MonoTextStyle::new(&FONT_7X13, BLACK);
    let bottom = TextStyleBuilder::new().baseline(Baseline::Bottom).build();

    let bat_text = format!(4; "{}%", bat_prc).unwrap();
    Text::with_text_style(
        &bat_text,
        Point::new(148, 14),
        style_msg,
        TextStyleBuilder::new()
            .baseline(Baseline::Bottom)
            .alignment(Alignment::Right)
            .build(),
    )
    .draw(display)?;

    LAST_ADVERT.lock(|cell| -> Result<(), D::Error> {
        match *cell.borrow() {
            None => {
                Text::with_text_style(
                    "No adverts",
                    display.bounding_box().center(),
                    style_msg,
                    TextStyleBuilder::new()
                        .baseline(Baseline::Middle)
                        .alignment(Alignment::Center)
                        .build(),
                )
                .draw(display)?;
            }
            Some(ref adv) => {
                // Row 1: device name (bold) or "Unknown"
                let name = if adv.name.is_empty() {
                    "Unknown"
                } else {
                    adv.name.as_str()
                };
                Text::with_text_style(name, Point::new(4, 14), style_bold, bottom).draw(display)?;

                // Row 2: role
                let role = match adv.role {
                    1 => "Chat Node",
                    2 => "Repeater",
                    3 => "Room Server",
                    4 => "Sensor",
                    _ => "Unknown role",
                };
                Text::with_text_style(role, Point::new(4, 28), style_msg, bottom).draw(display)?;

                // Divider
                Rectangle::new(Point::new(0, 30), Size::new(152, 1))
                    .into_styled(PrimitiveStyle::with_fill(BLACK))
                    .draw(display)?;

                // Key prefix (16 hex chars = 8 bytes)
                Text::with_text_style("Key:", Point::new(4, 44), style_small, bottom)
                    .draw(display)?;
                Text::with_text_style(
                    adv.pub_key_hex.as_str(),
                    Point::new(4, 56),
                    style_small,
                    bottom,
                )
                .draw(display)?;

                // Signature validity
                let sig_text = if adv.sig_ok {
                    "Sig: OK"
                } else {
                    "Sig: INVALID"
                };
                let sig_style = if adv.sig_ok {
                    MonoTextStyle::new(&FONT_7X13, BLACK)
                } else {
                    MonoTextStyle::new(&FONT_7X13, RED)
                };
                Text::with_text_style(sig_text, Point::new(4, 72), sig_style, bottom)
                    .draw(display)?;

                // GPS coordinates (if present)
                if adv.lat != 0 || adv.lon != 0 {
                    let lat_deg = adv.lat / 1_000_000;
                    let lat_frac = (adv.lat.abs() % 1_000_000) as u32;
                    let lat_hem = if adv.lat >= 0 { 'N' } else { 'S' };
                    let lon_deg = adv.lon / 1_000_000;
                    let lon_frac = (adv.lon.abs() % 1_000_000) as u32;
                    let lon_hem = if adv.lon >= 0 { 'E' } else { 'W' };
                    let lat_text =
                        format!(18; "{}.{:06}{}", lat_deg.abs(), lat_frac, lat_hem).unwrap();
                    let lon_text =
                        format!(19; "{}.{:06}{}", lon_deg.abs(), lon_frac, lon_hem).unwrap();
                    Text::with_text_style(&lat_text, Point::new(4, 88), style_small, bottom)
                        .draw(display)?;
                    Text::with_text_style(&lon_text, Point::new(4, 104), style_small, bottom)
                        .draw(display)?;
                } else {
                    Text::with_text_style("No GPS", Point::new(4, 88), style_small, bottom)
                        .draw(display)?;
                }

                // RSSI and SNR at bottom
                let rssi_text = format!(24; "{} dBm / {} dB", adv.rssi, adv.snr_x4 / 4).unwrap();
                Text::with_text_style(&rssi_text, Point::new(4, 152), style_small, bottom)
                    .draw(display)?;
            }
        }
        Ok(())
    })
}
