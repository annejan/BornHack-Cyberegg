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
pub mod menu;
pub use menu::{DISPLAY_STATE, DisplayState, MenuItem, MenuItemKind, ScreenState, draw_menu};

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

/// Boosted RX gain toggle (0x96 vs 0x94 in register 0x08AC). Default: off.
pub static BOOSTED_RX_GAIN: AtomicBool = AtomicBool::new(false);

/// UTC offset in whole hours (-12..=+14). Default: 0 (UTC).
pub static TIMEZONE_OFFSET: core::sync::atomic::AtomicI8 = core::sync::atomic::AtomicI8::new(0);

/// Fired when `TIMEZONE_OFFSET` changes so the BLE task can persist it.
#[cfg(feature = "embassy")]
pub static TZ_CHANGED_SIGNAL: Signal<CriticalSectionRawMutex, ()> = Signal::new();

/// Last decoded LoRa group-text message, updated by the meshcore listener task.
#[cfg(feature = "embassy")]
pub struct LoraMessage {
    pub channel: heapless::String<32>,
    pub sender: heapless::String<32>,
    pub text: heapless::String<128>,
    pub timestamp: u32,
    pub rssi: i16,
    pub snr_x4: i8,
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
    pub snr_x4: i8,
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

/// Fired every minute by `minute_tick_task` so the display redraws the clock.
#[cfg(feature = "embassy")]
pub static MINUTE_TICK: Signal<CriticalSectionRawMutex, ()> = Signal::new();

// ---------------------------------------------------------------------------
// Wall clock
// ---------------------------------------------------------------------------

#[cfg(feature = "embassy")]
struct WallClock {
    unix_base:  u32,
    ticks_base: u64,
}

#[cfg(feature = "embassy")]
static WALL_CLOCK: Mutex<CriticalSectionRawMutex, RefCell<Option<WallClock>>> =
    Mutex::new(RefCell::new(None));

/// Called by the BLE task when `SET_DEVICE_TIME` (0x06) is received.
#[cfg(feature = "embassy")]
pub fn set_wall_clock(unix_secs: u32) {
    WALL_CLOCK.lock(|cell| {
        *cell.borrow_mut() = Some(WallClock {
            unix_base:  unix_secs,
            ticks_base: embassy_time::Instant::now().as_ticks(),
        });
    });
}

/// Current unix time in seconds, or `None` if the clock has never been synced.
#[cfg(feature = "embassy")]
pub fn unix_now() -> Option<u32> {
    WALL_CLOCK.lock(|cell| {
        cell.borrow().as_ref().map(|wc| {
            let elapsed = embassy_time::Instant::now().as_ticks()
                .saturating_sub(wc.ticks_base);
            wc.unix_base.saturating_add((elapsed / embassy_time::TICK_HZ) as u32)
        })
    })
}

/// MeshCore node name cached from KV for synchronous access by the display renderer.
/// Populated by the BLE task at startup (after reading from flash) and on every
/// SET_ADVERT_NAME update.  Empty until the BLE task has initialized.
#[cfg(feature = "embassy")]
pub static NODE_NAME: Mutex<CriticalSectionRawMutex, RefCell<heapless::String<31>>> =
    Mutex::new(RefCell::new(heapless::String::new()));

/// Store `name` (raw UTF-8 bytes) into [`NODE_NAME`].  Invalid UTF-8 is ignored.
#[cfg(feature = "embassy")]
pub fn update_node_name(name: &[u8]) {
    if let Ok(s) = core::str::from_utf8(name) {
        NODE_NAME.lock(|cell| {
            let mut stored = cell.borrow_mut();
            stored.clear();
            let _ = stored.push_str(&s[..s.len().min(31)]);
        });
    }
}

/// Fired by the menu to request the BLE task to wipe and re-seed the channel store.
#[cfg(feature = "embassy")]
pub static CHANNEL_RESET_SIGNAL: Signal<CriticalSectionRawMutex, ()> = Signal::new();

/// Fired by the menu when the boost-RX toggle changes so the BLE task can persist it.
#[cfg(feature = "embassy")]
pub static BOOST_RX_CHANGED_SIGNAL: Signal<CriticalSectionRawMutex, ()> = Signal::new();

/// Fired by the menu to request the BLE task to clear all stored contacts.
#[cfg(feature = "embassy")]
pub static CONTACT_RESET_SIGNAL: Signal<CriticalSectionRawMutex, ()> = Signal::new();

/// Fired by the meshcore task whenever a new message is pushed to `msg_queue`.
/// The BLE task listens for this to send an unsolicited `0x83` notification.
#[cfg(feature = "embassy")]
pub static MESSAGES_WAITING_SIGNAL: Signal<CriticalSectionRawMutex, ()> = Signal::new();

/// Fired by the BLE task after a `SET_CHANNEL` or channel reset so that the
/// meshcore task reloads its channel table from KV.
#[cfg(feature = "embassy")]
pub static CHANNELS_CHANGED_SIGNAL: Signal<CriticalSectionRawMutex, ()> = Signal::new();

/// Signals the meshcore task to transmit a self-advert.
pub static SEND_ADVERT_SIGNAL: Signal<CriticalSectionRawMutex, fw::meshcore::AdvertMode> =
    Signal::new();

/// A raw received LoRa packet to be forwarded to the BLE companion as `0x88`.
#[cfg(feature = "embassy")]
pub struct RawLoRaPkt {
    pub snr_x4: i8,
    pub rssi: i8,
    pub len: usize,
    pub data: [u8; meshcore::MAX_TRANS_UNIT],
}

/// Passes raw received LoRa packets from the meshcore task to the BLE task for
/// immediate `0x88` (PUSH_CODE_LOG_RX_DATA) notifications.
///
/// Depth 4: burst tolerance.  If the BLE task is slow the oldest raw packet is
/// dropped (send via `try_send`, ignore `Err`).
#[cfg(feature = "embassy")]
pub static RAW_PKT_CHANNEL: embassy_sync::channel::Channel<CriticalSectionRawMutex, RawLoRaPkt, 4> =
    embassy_sync::channel::Channel::new();

/// An outgoing channel message queued by the BLE task for the meshcore task to transmit.
#[cfg(feature = "embassy")]
pub struct TxChannelMsg {
    pub channel_idx: u8,
    pub timestamp: u32,
    pub text: heapless::Vec<u8, { fw::msg_queue::MAX_TEXT }>,
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

/// An outgoing private (P2P) message queued by the BLE task for the meshcore task to transmit.
#[cfg(feature = "embassy")]
pub struct TxPrivateMsg {
    /// Full 32-byte recipient public key.
    pub recipient_pub_key: [u8; meshcore::PUB_KEY_SIZE],
    pub timestamp: u32,
    pub text: heapless::Vec<u8, { fw::msg_queue::MAX_TEXT }>,
}

/// Queue from the BLE companion task to the meshcore task for outgoing private messages.
#[cfg(feature = "embassy")]
pub static TX_PM_CHANNEL: embassy_sync::channel::Channel<CriticalSectionRawMutex, TxPrivateMsg, 4> =
    embassy_sync::channel::Channel::new();

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
    let active = with_display_state!(|state: &Ref<'_, DisplayState<5>>| state.active_screen());
    match active {
        #[cfg(feature = "embassy")]
        0 => fw::game::draw_screen_game(display, fw::game::nav::get_nav()),
        1 => draw_screen_main(display, health_str, bat_prc),
        #[cfg(feature = "embassy")]
        2 => draw_screen_lora(display, bat_prc),
        #[cfg(feature = "embassy")]
        3 => draw_screen_advert(display, bat_prc),
        // Screen 4 (badgercorn) is rendered via blit() in embassy.rs — nothing to draw here.
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
#[cfg(feature = "embassy")]
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

    let centered = TextStyleBuilder::new()
        .baseline(Baseline::Middle)
        .alignment(Alignment::Center)
        .build();

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
    #[cfg(feature = "embassy")]
    NODE_NAME.lock(|cell| -> Result<(), D::Error> {
        let name = cell.borrow();
        let display_name = if name.is_empty() { "<Empty>" } else { name.as_str() };
        Text::with_text_style(
            display_name,
            Point::new(148, 33),
            text_style_inverted,
            TextStyleBuilder::new().baseline(Baseline::Bottom).alignment(Alignment::Right).build(),
        )
        .draw(display)
        .map(|_| ())
    })?;

    let (items, pos) = with_display_state!(|state: &Ref<'_, DisplayState<5>>| {
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

    #[cfg(feature = "embassy")]
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

                // RSSI and SNR at bottom
                let rssi_text = format!(24; "{} dBm / {} dB", msg.rssi, msg.snr_x4 / 4).unwrap();
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
