//! Sponsor logo slideshow — shown on demand from the Bornagotchi menu.
//!
//! Displays full-screen sponsor logos (152×152 PCX files) stored on
//! the FAT12 filesystem as `030000.PCX` through `030009.PCX`.
//! Missing files are silently skipped.
//!
//! Triggered by the "Badge sponsors" menu item via [`request_show`]; the
//! display loop polls [`run_if_requested`] and plays it inline (it owns the
//! display + button receiver).  Not shown automatically at boot.

use embassy_time::Timer;
use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::mono_font::iso_8859_1::FONT_7X13_BOLD;
use embedded_graphics::prelude::*;
use embedded_graphics::text::{Alignment, Baseline, Text, TextStyleBuilder};
use ssd1675::graphics::Color;

use super::epd::EpdGfx;
use super::fat12;

/// Maximum number of sponsor slides (filenames 030000–030009).
const MAX_SPONSORS: usize = 10;

/// Seconds to display each sponsor logo.
const SLIDE_DURATION_SECS: u64 = 10;

// ── Filename generation ──────────────────────────────────────────────────────

const HEX: &[u8; 16] = b"0123456789ABCDEF";

/// Build FAT12 8.3 filename for sponsor slide `index` (0–9).
/// Format: `0300FF  PCX` where FF is the hex frame index.
/// (Sponsors moved from prefix `02` to `03` when the slug pet was
/// added — slug now occupies the `02xx` range.)
fn sponsor_filename(index: u8) -> [u8; 11] {
    [
        b'0',
        b'3',
        b'0',
        b'0',
        HEX[(index >> 4) as usize],
        HEX[(index & 0xF) as usize],
        b' ',
        b' ',
        b'P',
        b'C',
        b'X',
    ]
}

// ── On-demand show request ─────────────────────────────────────────────────

/// Set by the "Badge sponsors" menu item; the display loop polls it via
/// [`run_if_requested`].  Sponsors are no longer shown automatically at boot.
static SHOW_REQUESTED: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

/// Request the sponsor slideshow (sync, for menu callbacks).
pub fn request_show() {
    SHOW_REQUESTED.store(true, core::sync::atomic::Ordering::Relaxed);
}

/// If a show was requested, play the slideshow now.  Call from the display
/// loop, which owns `display` + `button_rcvr`.  No-op when nothing was
/// requested or no sponsor slides are present.
pub async fn run_if_requested(
    display: &mut EpdGfx<'_>,
    button_rcvr: &mut embassy_sync::watch::Receiver<
        '_,
        embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex,
        u8,
        2,
    >,
) {
    if !SHOW_REQUESTED.swap(false, core::sync::atomic::Ordering::Relaxed) {
        return;
    }
    if !any_sponsor_file_present().await {
        defmt::info!("sponsors: no slides present — nothing to show");
        return;
    }
    show_slideshow(display, button_rcvr).await;
}

// ── Slideshow runner ─────────────────────────────────────────────────────────

/// Play the sponsor slideshow to completion (caller has already confirmed at
/// least one slide is present).  `button_rcvr` advances slides early.
async fn show_slideshow(
    display: &mut EpdGfx<'_>,
    button_rcvr: &mut embassy_sync::watch::Receiver<
        '_,
        embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex,
        u8,
        2,
    >,
) {
    // ── Intro screen ─────────────────────────────────────────────────
    display.clear(Color::White);

    let centered = TextStyleBuilder::new()
        .baseline(Baseline::Middle)
        .alignment(Alignment::Center)
        .build();
    let font = MonoTextStyle::new(&FONT_7X13_BOLD, Color::Black);

    let _ =
        Text::with_text_style("This badge has", Point::new(76, 50), font, centered).draw(display);
    let _ = Text::with_text_style("been made possible", Point::new(76, 66), font, centered)
        .draw(display);
    let _ =
        Text::with_text_style("by our awesome", Point::new(76, 82), font, centered).draw(display);
    let _ = Text::with_text_style("sponsors!", Point::new(76, 98), font, centered).draw(display);

    let _ = display.reset().await;
    let _ = display.update_tc(crate::fw::epd::current_lut_speed()).await;
    let _ = display.deep_sleep().await;

    wait_or_button(button_rcvr, SLIDE_DURATION_SECS).await;

    // ── Logo slides ──────────────────────────────────────────────────
    for i in 0..MAX_SPONSORS as u8 {
        let name = sponsor_filename(i);
        let Ok(file) = fat12::find_file(&name).await else {
            continue; // Skip missing slides.
        };

        display.clear(Color::White);

        #[cfg(feature = "game")]
        crate::game::sprite_loader::blit_file(display, &file, 0, 0).await;
        #[cfg(not(feature = "game"))]
        let _ = &file; // Suppress unused warning when game feature is off.

        let _ = display.reset().await;
        let _ = display.update_tc(crate::fw::epd::current_lut_speed()).await;
        let _ = display.deep_sleep().await;

        wait_or_button(button_rcvr, SLIDE_DURATION_SECS).await;
    }

    // ── Final white clear ────────────────────────────────────────────
    // Wipe the last sponsor logo so it doesn't linger — or ghost — into
    // the first carousel screen when the main loop takes over the panel.
    display.clear(Color::White);
    let _ = display.reset().await;
    let _ = display.update_tc(crate::fw::epd::current_lut_speed()).await;
    let _ = display.deep_sleep().await;

    defmt::info!("sponsors: slideshow complete");
}

/// Returns true if at least one sponsor PCX file (030000.PCX .. 0300NN.PCX)
/// exists on the FAT partition.
async fn any_sponsor_file_present() -> bool {
    for i in 0..MAX_SPONSORS as u8 {
        let name = sponsor_filename(i);
        if fat12::find_file(&name).await.is_ok() {
            return true;
        }
    }
    false
}

/// Wait for `secs` seconds, or until any button is pressed.
async fn wait_or_button(
    button_rcvr: &mut embassy_sync::watch::Receiver<
        '_,
        embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex,
        u8,
        2,
    >,
    secs: u64,
) {
    use embassy_futures::select::{Either, select};

    match select(Timer::after_secs(secs), button_rcvr.changed()).await {
        Either::First(_) => {}
        Either::Second(_) => {}
    }
}
