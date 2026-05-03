//! First-boot sponsor logo slideshow.
//!
//! Displays full-screen sponsor logos (152×152 PCX files) stored on
//! the FAT12 filesystem as `020000.PCX` through `020009.PCX`.
//! Missing files are silently skipped.
//!
//! After the slideshow completes, a flag is written to ekv so the
//! slideshow is only shown once. A menu option can clear the flag
//! to replay it on the next boot.

use embassy_time::Timer;
use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::mono_font::iso_8859_1::FONT_7X13_BOLD;
use embedded_graphics::prelude::*;
use embedded_graphics::text::{Alignment, Baseline, Text, TextStyleBuilder};
use ssd1675::graphics::Color;

use super::epd::EpdGfx;
use super::fat12;

/// Maximum number of sponsor slides (filenames 020000–020009).
const MAX_SPONSORS: usize = 10;

/// Seconds to display each sponsor logo.
const SLIDE_DURATION_SECS: u64 = 10;

/// EKV key: presence means the slideshow has already been shown.
const KV_KEY: &str = "shown";
const KV_NAMESPACE: &str = "sponsors";

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

// ── Slideshow flag ───────────────────────────────────────────────────────────

/// Check if the slideshow has already been shown (flag exists in ekv).
pub async fn already_shown() -> bool {
    use super::kv;
    let ns = kv::namespace(KV_NAMESPACE);
    let mut buf = [0u8; 1];
    ns.get(KV_KEY, &mut buf).await.is_ok()
}

/// Mark the slideshow as shown (write flag to ekv).
async fn mark_shown() {
    use super::kv;
    let ns = kv::namespace(KV_NAMESPACE);
    let _ = ns.set(KV_KEY, &[1], true).await;
}

/// Clear the "already shown" flag so the slideshow replays on next boot.
pub async fn clear_flag() {
    use super::kv;
    let ns = kv::namespace(KV_NAMESPACE);
    let _ = ns.delete(KV_KEY).await;
}

/// Synchronous request to clear the flag (called from menu callback).
/// The actual async clear happens on the next save cycle.
static CLEAR_REQUESTED: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

/// Request that the slideshow flag be cleared (sync, for menu callbacks).
pub fn request_clear() {
    CLEAR_REQUESTED.store(true, core::sync::atomic::Ordering::Relaxed);
}

/// Poll and execute the pending clear request (call from an async context).
pub async fn process_clear_request() {
    if CLEAR_REQUESTED.swap(false, core::sync::atomic::Ordering::Relaxed) {
        clear_flag().await;
        defmt::info!("sponsors: flag cleared — slideshow will replay on next boot");
    }
}

// ── Slideshow runner ─────────────────────────────────────────────────────────

/// Run the sponsor slideshow. Blocks until all slides are shown.
///
/// `button_rcvr` is used to detect button presses to advance slides.
///
/// If no sponsor PCX files are present on the FAT partition, shows a
/// "No assets found in flash" screen and blocks forever — the operator
/// can then copy files in via USB mass storage (spawned separately) and
/// power-cycle the badge to proceed.
pub async fn run(
    display: &mut EpdGfx<'_>,
    button_rcvr: &mut embassy_sync::watch::Receiver<
        '_,
        embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex,
        u8,
        2,
    >,
) {
    // ── Pre-flight: check at least one sponsor slide exists ──────────
    if !any_sponsor_file_present().await {
        show_missing_assets_forever(display).await;
        // show_missing_assets_forever never returns.
    }

    // ── Intro screen ─────────────────────────────────────────────────
    let _ = display.clear(Color::White);

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
    let _ = display.update_tc().await;
    let _ = display.deep_sleep().await;

    wait_or_button(button_rcvr, SLIDE_DURATION_SECS).await;

    // ── Logo slides ──────────────────────────────────────────────────
    for i in 0..MAX_SPONSORS as u8 {
        let name = sponsor_filename(i);
        let Ok(file) = fat12::find_file(&name).await else {
            continue; // Skip missing slides.
        };

        let _ = display.clear(Color::White);

        #[cfg(feature = "game")]
        crate::game::sprite_loader::blit_file(display, &file, 0, 0).await;
        #[cfg(not(feature = "game"))]
        let _ = &file; // Suppress unused warning when game feature is off.

        let _ = display.reset().await;
        let _ = display.update_tc().await;
        let _ = display.deep_sleep().await;

        wait_or_button(button_rcvr, SLIDE_DURATION_SECS).await;
    }

    // ── Mark as shown ────────────────────────────────────────────────
    mark_shown().await;

    defmt::info!("sponsors: slideshow complete");
}

/// Returns true if at least one sponsor PCX file (020000.PCX .. 0200NN.PCX)
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

/// Draw a full-screen "No assets found" message, commit it to the display,
/// then block forever.  Intended for mass-flashing: the operator flashes the
/// firmware, waits for this message to appear, copies the asset PCX files in
/// via USB mass storage, then power-cycles the badge.
async fn show_missing_assets_forever(display: &mut EpdGfx<'_>) {
    defmt::info!("sponsors: no sponsor PCX files found — waiting for USB upload");

    let _ = display.clear(Color::White);

    let centered = TextStyleBuilder::new()
        .baseline(Baseline::Middle)
        .alignment(Alignment::Center)
        .build();
    let font = MonoTextStyle::new(&FONT_7X13_BOLD, Color::Black);

    let _ =
        Text::with_text_style("No assets found", Point::new(76, 60), font, centered).draw(display);
    let _ = Text::with_text_style("in flash", Point::new(76, 76), font, centered).draw(display);
    let _ =
        Text::with_text_style("Copy via USB,", Point::new(76, 100), font, centered).draw(display);
    let _ = Text::with_text_style("then power cycle", Point::new(76, 116), font, centered)
        .draw(display);

    let _ = display.reset().await;
    let _ = display.update_tc().await;
    let _ = display.deep_sleep().await;

    // Block forever.  USB mass storage is running on its own task and stays
    // live while we're parked here.  A power-cycle is the intended exit.
    core::future::pending::<()>().await
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
