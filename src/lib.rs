#![cfg_attr(feature = "embassy", no_std)]
#![cfg_attr(feature = "embassy", no_main)]

#[derive(Debug, defmt::Format, PartialEq)]
pub enum ScreenError {
    NotFound,
    OutOfBounds,
    InvalidScreen,
}

#[cfg(feature = "embassy")]
pub mod fw;

use core::cell::{Ref, RefCell};

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
#[cfg(feature = "embassy")]
use fw::device_id::get_bytes as get_device_id;
#[cfg(feature = "simulator")]
fn get_device_id() -> [u8; 4] {
    *b"A3F7"
}
#[cfg(feature = "embassy")]
use heapless::format;
// Embassy: re-export Color from ssd1675 hardware driver
#[cfg(feature = "embassy")]
pub use ssd1675::graphics::Color;
#[cfg(feature = "embassy")]
pub use ssd1675::graphics::Color as TriColor;
#[cfg(feature = "embassy")]
pub const BLACK: Color = Color::Black;
#[cfg(feature = "embassy")]
pub const WHITE: Color = Color::White;
#[cfg(feature = "embassy")]
pub const RED: Color = Color::Red;

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
use embassy_sync::blocking_mutex::{
    Mutex,
    raw::{CriticalSectionRawMutex, ThreadModeRawMutex},
};
#[cfg(feature = "embassy")]
use embassy_sync::signal::Signal;

#[cfg(feature = "simulator")]
use std::sync::Mutex;

pub struct MenuItem {
    pub label: fn() -> &'static str,
    pub action: fn(),
}

/// Boosted RX gain toggle (0x96 vs 0x94 in register 0x08AC). Default: off.
pub static BOOSTED_RX_GAIN: AtomicBool = AtomicBool::new(false);

fn label_boost_rx() -> &'static str {
    if BOOSTED_RX_GAIN.load(Ordering::Relaxed) {
        "Boost RX: ON"
    } else {
        "Boost RX: OFF"
    }
}

fn action_boost_rx() {
    let current = BOOSTED_RX_GAIN.load(Ordering::Relaxed);
    BOOSTED_RX_GAIN.store(!current, Ordering::Relaxed);
}

pub struct ScreenState {
    pub items: &'static [MenuItem],
    pub menu_pos: u8,
}

impl ScreenState {
    pub const fn new(items: &'static [MenuItem]) -> Self {
        Self { items, menu_pos: 0 }
    }

    pub fn menu_up(&mut self) {
        if self.menu_pos > 0 {
            self.menu_pos -= 1;
        }
    }

    pub fn menu_down(&mut self) {
        if (self.menu_pos as usize) + 1 < self.items.len() {
            self.menu_pos += 1;
        }
    }

    pub fn current_item(&self) -> &MenuItem {
        &self.items[self.menu_pos as usize]
    }
}

/// Top-level display state: M screens each with their own item list and cursor position.
///
/// Up/down navigates items within the active screen.
/// Left/right switches between screens, preserving each screen's cursor position.
pub struct DisplayState<const M: usize> {
    active_screen: u8,
    screens: [ScreenState; M],
}

#[allow(dead_code)]
impl<const M: usize> DisplayState<M> {
    pub const fn new(screens: [ScreenState; M]) -> Self {
        Self {
            active_screen: 0,
            screens,
        }
    }

    pub fn screen_left(&mut self) {
        if self.active_screen > 0 {
            self.active_screen -= 1;
        }
    }

    pub fn screen_right(&mut self) {
        if (self.active_screen as usize) + 1 < M {
            self.active_screen += 1;
        }
    }

    pub fn active_screen(&self) -> u8 {
        self.active_screen
    }

    pub fn current_screen(&self) -> &ScreenState {
        &self.screens[self.active_screen as usize]
    }

    pub fn current_screen_mut(&mut self) -> &mut ScreenState {
        &mut self.screens[self.active_screen as usize]
    }

    pub fn menu_up(&mut self) {
        self.current_screen_mut().menu_up();
    }

    pub fn menu_down(&mut self) {
        self.current_screen_mut().menu_down();
    }

    pub fn fire(&self) {
        (self.current_screen().current_item().action)();
    }

    pub fn get_current_menu_item(&self) -> Option<&'static str> {
        Some((self.current_screen().current_item().label)())
    }

    pub fn get_menu_item(&self, index: usize) -> Option<&'static str> {
        self.current_screen().items.get(index).map(|i| (i.label)())
    }
}

static MAIN_ITEMS: [MenuItem; 4] = [
    MenuItem {
        label: || "Item 1",
        action: || {},
    },
    MenuItem {
        label: || "Item 2",
        action: || {},
    },
    MenuItem {
        label: label_boost_rx,
        action: action_boost_rx,
    },
    MenuItem {
        label: || "Reset channels",
        #[cfg(feature = "embassy")]
        action: || { CHANNEL_RESET_SIGNAL.signal(()); },
        #[cfg(not(feature = "embassy"))]
        action: || {},
    },
];

static LORA_ITEMS: [MenuItem; 1] = [MenuItem {
    label: || "LoRa",
    action: || {},
}];

static ADVERT_ITEMS: [MenuItem; 1] = [MenuItem {
    label: || "Adverts",
    action: || {},
}];

// Embassy version with ThreadModeRawMutex
#[cfg(feature = "embassy")]
pub static DISPLAY_STATE: Mutex<ThreadModeRawMutex, RefCell<DisplayState<3>>> =
    Mutex::new(RefCell::new(DisplayState::new([
        ScreenState::new(&MAIN_ITEMS),
        ScreenState::new(&LORA_ITEMS),
        ScreenState::new(&ADVERT_ITEMS),
    ])));

// Simulator version with std::sync::Mutex
#[cfg(feature = "simulator")]
pub static DISPLAY_STATE: Mutex<RefCell<DisplayState<3>>> =
    Mutex::new(RefCell::new(DisplayState::new([
        ScreenState::new(&MAIN_ITEMS),
        ScreenState::new(&LORA_ITEMS),
        ScreenState::new(&ADVERT_ITEMS),
    ])));

/// Last decoded LoRa group-text message, updated by the meshcore listener task.
#[cfg(feature = "embassy")]
pub struct LoraMessage {
    pub channel: heapless::String<32>,
    pub sender: heapless::String<32>,
    pub text: heapless::String<128>,
    pub timestamp: u32,
    pub rssi: i16,
}

#[cfg(feature = "embassy")]
pub static LAST_LORA_MSG: Mutex<CriticalSectionRawMutex, RefCell<Option<LoraMessage>>> =
    Mutex::new(RefCell::new(None));

/// Fired by the meshcore listener task whenever a new message is stored in LAST_LORA_MSG.
#[cfg(feature = "embassy")]
pub static LORA_MSG_SIGNAL: Signal<CriticalSectionRawMutex, ()> = Signal::new();

/// Last received MeshCore advert, updated by the meshcore listener task.
#[cfg(feature = "embassy")]
pub struct LastAdvert {
    /// Device name, or empty string if the advert carried no name.
    pub name: heapless::String<32>,
    /// First 8 bytes of the public key as lowercase hex (16 chars).
    pub pub_key_hex: heapless::String<16>,
    pub role: u8,
    pub sig_ok: bool,
    pub rssi: i16,
    /// GPS latitude in microdegrees (° × 1 000 000), 0 if not present.
    pub lat: i32,
    /// GPS longitude in microdegrees (° × 1 000 000), 0 if not present.
    pub lon: i32,
}

#[cfg(feature = "embassy")]
pub static LAST_ADVERT: Mutex<CriticalSectionRawMutex, RefCell<Option<LastAdvert>>> =
    Mutex::new(RefCell::new(None));

/// Fired by the meshcore listener task whenever a new advert is stored in LAST_ADVERT.
#[cfg(feature = "embassy")]
pub static ADVERT_SIGNAL: Signal<CriticalSectionRawMutex, ()> = Signal::new();

/// Advert data forwarded to the BLE task for push to the companion app (0x8A).
#[cfg(feature = "embassy")]
pub struct AdvertBleNotif {
    /// Full Ed25519 public key (32 bytes) of the advertising node.
    pub pub_key: [u8; 32],
    /// Node role (ADV_TYPE_*).
    pub adv_type: u8,
    /// RSSI in dBm (cast to i8).
    pub rssi: i8,
    /// Unix timestamp from the advert payload.
    pub timestamp: u32,
    /// Latitude × 1 000 000 (0 if not present).
    pub lat: i32,
    /// Longitude × 1 000 000 (0 if not present).
    pub lon: i32,
    /// Advertising node's display name (UTF-8 bytes).
    pub name: heapless::Vec<u8, 32>,
}

#[cfg(feature = "embassy")]
pub static ADVERT_BLE_CHANNEL: embassy_sync::channel::Channel<
    CriticalSectionRawMutex,
    AdvertBleNotif,
    4,
> = embassy_sync::channel::Channel::new();

/// Last received private message (TxtMsg), updated by the meshcore listener task.
#[cfg(feature = "embassy")]
pub struct LastPm {
    /// Sender's name from the contacts list, or hex pub-key prefix if unknown.
    pub sender_name: heapless::String<32>,
    pub text: heapless::String<{ meshcore::payload::txt_msg::MAX_TXT_TEXT_SIZE }>,
    pub timestamp: u32,
    pub rssi: i16,
}

#[cfg(feature = "embassy")]
pub static LAST_PM: Mutex<CriticalSectionRawMutex, RefCell<Option<LastPm>>> =
    Mutex::new(RefCell::new(None));

/// Fired by the meshcore listener task whenever a new PM is stored in LAST_PM.
#[cfg(feature = "embassy")]
pub static PM_SIGNAL: Signal<CriticalSectionRawMutex, ()> = Signal::new();

/// Active BLE pairing passkey (6-digit, 0–999999). `u32::MAX` means no pairing in progress.
pub static BLE_PASSKEY: AtomicU32 = AtomicU32::new(u32::MAX);

/// Fired by the BLE task whenever the pairing passkey changes (new passkey or cleared).
#[cfg(feature = "embassy")]
pub static BLE_PAIRING_SIGNAL: Signal<CriticalSectionRawMutex, ()> = Signal::new();

/// Fired by the menu to request the BLE task to wipe and re-seed the channel store.
#[cfg(feature = "embassy")]
pub static CHANNEL_RESET_SIGNAL: Signal<CriticalSectionRawMutex, ()> = Signal::new();

/// Fired by the meshcore task whenever a new message is pushed to `msg_queue`.
/// The BLE task listens for this to send an unsolicited `0x83` notification.
#[cfg(feature = "embassy")]
pub static MESSAGES_WAITING_SIGNAL: Signal<CriticalSectionRawMutex, ()> = Signal::new();

/// Fired by the BLE task after a `SET_CHANNEL` or channel reset so that the
/// meshcore task reloads its channel table from KV.
#[cfg(feature = "embassy")]
pub static CHANNELS_CHANGED_SIGNAL: Signal<CriticalSectionRawMutex, ()> = Signal::new();

/// A raw received LoRa packet to be forwarded to the BLE companion as `0x88`.
#[cfg(feature = "embassy")]
pub struct RawLoRaPkt {
    pub snr_x4: i8,
    pub rssi:   i8,
    pub len:    usize,
    pub data:   [u8; meshcore::MAX_TRANS_UNIT],
}

/// Passes raw received LoRa packets from the meshcore task to the BLE task for
/// immediate `0x88` (PUSH_CODE_LOG_RX_DATA) notifications.
///
/// Depth 4: burst tolerance.  If the BLE task is slow the oldest raw packet is
/// dropped (send via `try_send`, ignore `Err`).
#[cfg(feature = "embassy")]
pub static RAW_PKT_CHANNEL: embassy_sync::channel::Channel<
    CriticalSectionRawMutex,
    RawLoRaPkt,
    4,
> = embassy_sync::channel::Channel::new();

/// An outgoing channel message queued by the BLE task for the meshcore task to transmit.
#[cfg(feature = "embassy")]
pub struct TxChannelMsg {
    pub channel_idx: u8,
    pub timestamp:   u32,
    pub text:        heapless::Vec<u8, { fw::msg_queue::MAX_TEXT }>,
}

/// Queue from the BLE companion task to the meshcore task for outgoing channel messages.
///
/// Depth 16: at slow LoRa settings (SF12 / narrow BW) each packet takes several seconds,
/// so a burst of pasted messages must not overflow before they can be drained.
#[cfg(feature = "embassy")]
pub static TX_MSG_CHANNEL: embassy_sync::channel::Channel<
    CriticalSectionRawMutex,
    TxChannelMsg,
    16,
> = embassy_sync::channel::Channel::new();

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

/// Dispatch to the correct screen renderer based on the active screen.
pub fn draw_graphics<D>(display: &mut D, health_str: &str, bat_prc: &u8) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let active = with_display_state!(|state: &Ref<'_, DisplayState<3>>| state.active_screen());
    match active {
        0 => draw_screen_main(display, health_str, bat_prc),
        #[cfg(feature = "embassy")]
        1 => draw_screen_lora(display, bat_prc),
        #[cfg(feature = "embassy")]
        2 => draw_screen_advert(display, bat_prc),
        _ => draw_screen_main(display, health_str, bat_prc),
    }
}

fn draw_screen_main<D>(display: &mut D, health_str: &str, bat_prc: &u8) -> Result<(), D::Error>
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

    let text_style = MonoTextStyle::new(&FONT_7X13, WHITE);
    let text_style_inverted = MonoTextStyle::new(&FONT_10X20, BLACK);
    let bat_style = MonoTextStyle::new(&FONT_7X13, BLACK);
    let item_text = with_display_state!(|state: &Ref<'_, DisplayState<3>>| state
        .get_current_menu_item()
        .unwrap());

    let bat_text = format!(4; "{}%", bat_prc).unwrap();
    Text::with_text_style(
        &bat_text,
        Point::new(110, 16),
        bat_style,
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
    let passkey_val = BLE_PASSKEY.load(Ordering::Relaxed);
    if passkey_val != u32::MAX {
        let pin_label_style = MonoTextStyle::new(&FONT_7X13, WHITE);
        let pin_code_style = MonoTextStyle::new(&FONT_10X20, WHITE);
        let center = display.bounding_box().center();
        let code_str = format!(8; "{:06}", passkey_val).unwrap();
        Text::with_text_style(
            "BT PIN:",
            Point::new(center.x, center.y - 14),
            pin_label_style,
            centered,
        )
        .draw(display)?;
        Text::with_text_style(&code_str, center, pin_code_style, centered).draw(display)?;
    } else {
        Text::with_text_style(
            item_text,
            display.bounding_box().center(),
            text_style,
            centered,
        )
        .draw(display)?;
    }
    Text::with_text_style(
        health_str,
        Point::new(10, 128),
        text_style_inverted,
        TextStyleBuilder::new().baseline(Baseline::Bottom).build(),
    )
    .draw(display)?;

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

#[cfg(feature = "embassy")]
fn draw_screen_lora<D>(display: &mut D, bat_prc: &u8) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let style_bold = MonoTextStyle::new(&FONT_7X13_BOLD, BLACK);
    let style_msg = MonoTextStyle::new(&FONT_7X13, BLACK);
    let style_rssi = MonoTextStyle::new(&FONT_7X13, BLACK);
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

    LAST_LORA_MSG.lock(|cell| -> Result<(), D::Error> {
        match *cell.borrow() {
            None => {
                Text::with_text_style(
                    "No messages",
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
                // Row 1: channel name (bold)
                Text::with_text_style(msg.channel.as_str(), Point::new(4, 14), style_bold, bottom)
                    .draw(display)?;

                // Row 2: sender nickname (bold)
                Text::with_text_style(msg.sender.as_str(), Point::new(4, 28), style_bold, bottom)
                    .draw(display)?;

                // Divider
                Rectangle::new(Point::new(0, 30), Size::new(152, 1))
                    .into_styled(PrimitiveStyle::with_fill(BLACK))
                    .draw(display)?;

                // Rows 3+: message text wrapped at 21 chars, 14px per line
                draw_wrapped(display, msg.text.as_str(), 4, 44, 14, 21, style_msg)?;

                // RSSI at bottom
                let rssi_text = format!(12; "{} dBm", msg.rssi).unwrap();
                Text::with_text_style(&rssi_text, Point::new(4, 152), style_rssi, bottom)
                    .draw(display)?;
            }
        }
        Ok(())
    })
}

#[cfg(feature = "embassy")]
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
                    let lat_deg  = adv.lat / 1_000_000;
                    let lat_frac = (adv.lat.abs() % 1_000_000) as u32;
                    let lat_hem  = if adv.lat >= 0 { 'N' } else { 'S' };
                    let lon_deg  = adv.lon / 1_000_000;
                    let lon_frac = (adv.lon.abs() % 1_000_000) as u32;
                    let lon_hem  = if adv.lon >= 0 { 'E' } else { 'W' };
                    let lat_text = format!(18; "{}.{:06}{}", lat_deg.abs(), lat_frac, lat_hem).unwrap();
                    let lon_text = format!(19; "{}.{:06}{}", lon_deg.abs(), lon_frac, lon_hem).unwrap();
                    Text::with_text_style(&lat_text, Point::new(4, 88), style_small, bottom).draw(display)?;
                    Text::with_text_style(&lon_text, Point::new(4, 104), style_small, bottom).draw(display)?;
                } else {
                    Text::with_text_style("No GPS", Point::new(4, 88), style_small, bottom).draw(display)?;
                }

                // RSSI at bottom
                let rssi_text = format!(12; "{} dBm", adv.rssi).unwrap();
                Text::with_text_style(&rssi_text, Point::new(4, 152), style_small, bottom)
                    .draw(display)?;
            }
        }
        Ok(())
    })
}
