//! Token screen — a simple full-screen display showing either the static text
//! "Token" or a token value received from a MeshCore message or NFC tag.
//!
//! The token value is shown for 2 minutes 30 seconds (150 seconds) after
//! which the display reverts to the placeholder text "Token".
//!
//! # Triggering
//!
//! * **MeshCore**: any received message whose plaintext starts with `"token:"`
//!   causes the substring after the colon to be stored and shown.
//! * **NFC**: any NDEF text record written to the badge whose text starts with
//!   `"token:"` does the same.
//!
//! There are no button interactions on this screen; it is navigated to/from
//! using the standard left/right screen-switching buttons.

#[cfg(feature = "embassy-base")]
use core::sync::atomic::Ordering;
use core::sync::atomic::AtomicU32;

use embedded_graphics::prelude::*;
use embedded_graphics::text::{Alignment, Baseline, Text, TextStyleBuilder};

use crate::{TriColor, draw_frame};
use core::cell::RefCell;

// ---------------------------------------------------------------------------
// Mutex — embassy vs simulator (mirrors NODE_NAME in lib.rs)
// ---------------------------------------------------------------------------

#[cfg(feature = "embassy-base")]
use embassy_sync::blocking_mutex::{Mutex, raw::CriticalSectionRawMutex};

/// Maximum byte length of the stored token string.
pub const TOKEN_MAX_LEN: usize = 64;

#[cfg(feature = "embassy-base")]
pub static TOKEN_VALUE: Mutex<CriticalSectionRawMutex, RefCell<heapless::String<TOKEN_MAX_LEN>>> =
    Mutex::new(RefCell::new(heapless::String::new()));

#[cfg(feature = "simulator")]
pub static TOKEN_VALUE: std::sync::Mutex<RefCell<heapless::String<TOKEN_MAX_LEN>>> =
    std::sync::Mutex::new(RefCell::new(heapless::String::new()));

// ---------------------------------------------------------------------------
// Token state
// ---------------------------------------------------------------------------

/// Embassy monotonic time in seconds at which the token was last set.
/// `u32::MAX` means "no token active".
pub static TOKEN_SET_AT: AtomicU32 = AtomicU32::new(u32::MAX);

/// How long a received token stays visible: 2 min 30 s = 150 seconds.
/// Embassy-only: the simulator's `token_is_active` is a stub that
/// always returns `false` and never consults this value.
#[cfg(feature = "embassy-base")]
const TOKEN_VISIBLE_SECS: u32 = 150;

// ---------------------------------------------------------------------------
// Public API called by MeshCore / NFC handlers
// ---------------------------------------------------------------------------

/// Store `value` as the active token and start the 150-second visibility
/// timer.  `value` is silently truncated to [`TOKEN_MAX_LEN`] bytes on a
/// UTF-8 char boundary.
#[cfg(feature = "embassy-base")]
pub fn set_token(value: &str) {
    TOKEN_VALUE.lock(|cell| {
        let mut stored = cell.borrow_mut();
        stored.clear();
        let _ = stored.push_str(crate::truncate_str(value, TOKEN_MAX_LEN));
    });
    let now_secs = (embassy_time::Instant::now().as_ticks() / embassy_time::TICK_HZ) as u32;
    TOKEN_SET_AT.store(now_secs, Ordering::Relaxed);
    crate::TOKEN_SIGNAL.signal(());
}

// ---------------------------------------------------------------------------
// Active-token check
// ---------------------------------------------------------------------------

#[cfg(feature = "embassy-base")]
fn token_is_active() -> bool {
    let set_at = TOKEN_SET_AT.load(Ordering::Relaxed);
    if set_at == u32::MAX {
        return false;
    }
    let now_secs = (embassy_time::Instant::now().as_ticks() / embassy_time::TICK_HZ) as u32;
    now_secs.saturating_sub(set_at) < TOKEN_VISIBLE_SECS
}

#[cfg(not(feature = "embassy-base"))]
fn token_is_active() -> bool {
    false
}

// ---------------------------------------------------------------------------
// Token value read — embassy vs simulator
// ---------------------------------------------------------------------------

/// Call `f` with the currently stored token string.
#[cfg(feature = "embassy-base")]
fn with_token_value<F, R>(f: F) -> R
where
    F: FnOnce(&str) -> R,
{
    TOKEN_VALUE.lock(|cell| f(cell.borrow().as_str()))
}

#[cfg(feature = "simulator")]
fn with_token_value<F, R>(f: F) -> R
where
    F: FnOnce(&str) -> R,
{
    let guard = TOKEN_VALUE.lock().unwrap();
    f(guard.borrow().as_str())
}

// ---------------------------------------------------------------------------
// Draw
// ---------------------------------------------------------------------------

pub fn draw<D>(display: &mut D) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    #[cfg(feature = "embassy-base")]
    let bat = crate::fw::battery::read_pct();
    #[cfg(not(feature = "embassy-base"))]
    let bat: u8 = 0;

    draw_frame(display, Some(("Token", &bat)), None)?;

    let centered = TextStyleBuilder::new()
        .baseline(Baseline::Middle)
        .alignment(Alignment::Center)
        .build();

    // Collect the token text (if active) into a local buffer to avoid
    // holding the mutex across the draw calls.
    let display_text: heapless::String<TOKEN_MAX_LEN> = if token_is_active() {
        with_token_value(|s| {
            let mut out = heapless::String::new();
            let _ = out.push_str(s);
            out
        })
    } else {
        heapless::String::new()
    };

    if display_text.is_empty() {
        // Static placeholder — no active token or empty value.
        Text::with_text_style(
            "Token",
            Point::new(76, 84),
            crate::ui::TEXT_BOLD_BLACK,
            centered,
        )
        .draw(display)?;
        return Ok(());
    }

    // Render up to two lines.  FONT_7X13_BOLD is 7 px wide on a 152 px
    // panel, so ~21 chars fit per line; cap at 20 to leave a small
    // margin and avoid touching the panel edges.  Single-word tokens
    // up to 20 chars now fit on one line; longer ones break to a
    // second line at the 20-char boundary instead of mid-word at 10.
    const CHARS_PER_LINE: usize = 20;
    let s = display_text.as_str();
    let line1_end = s
        .char_indices()
        .nth(CHARS_PER_LINE)
        .map(|(i, _)| i)
        .unwrap_or(s.len());
    let line1 = &s[..line1_end];
    let line2_full = &s[line1_end..];

    if line2_full.is_empty() {
        Text::with_text_style(line1, Point::new(76, 84), crate::ui::TEXT_BOLD_BLACK, centered)
            .draw(display)?;
    } else {
        let line2_end = line2_full
            .char_indices()
            .nth(CHARS_PER_LINE)
            .map(|(i, _)| i)
            .unwrap_or(line2_full.len());
        let line2 = &line2_full[..line2_end];
        Text::with_text_style(line1, Point::new(76, 76), crate::ui::TEXT_BOLD_BLACK, centered)
            .draw(display)?;
        Text::with_text_style(line2, Point::new(76, 92), crate::ui::TEXT_BOLD_BLACK, centered)
            .draw(display)?;
    }

    Ok(())
}
