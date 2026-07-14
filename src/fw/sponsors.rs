//! Sponsor logo slideshows — shown on demand from the root menu.
//!
//! Two groups of full-screen 152×152 PCX logos on the FAT12 filesystem:
//! * "Badge sponsors" → `030000.PCX`..`03000F.PCX` (hardware sponsors).
//! * "Sponsors"       → `030100.PCX`..`03010F.PCX` (BornHack event sponsors).
//! Missing files are silently skipped.
//!
//! Played once at boot ([`run_boot_slideshow`]: event sponsors then badge
//! sponsors, 2s/slide) and on demand from the root menu (via
//! [`request_show_badge`] / [`request_show_event`], drained by
//! [`run_if_requested`] in the display loop, 5s/slide).

use embassy_time::Timer;
use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::mono_font::iso_8859_1::FONT_7X13_BOLD;
use embedded_graphics::prelude::*;
use embedded_graphics::text::{Alignment, Baseline, Text, TextStyleBuilder};
use ssd1675::graphics::Color;

use super::epd::EpdGfx;
use super::fat12;

/// Maximum number of sponsor slides per group (indices 00..0F).
const MAX_SPONSORS: usize = 16;

/// Seconds to display each sponsor logo.
const SLIDE_DURATION_SECS: u64 = 5;

/// EKV namespace + key: presence means the boot slideshow has already run.
const KV_NAMESPACE: &str = "sponsors";
const KV_KEY: &str = "shown";

/// True once the first-boot slideshow has run (flag present in ekv).
async fn already_shown() -> bool {
    let mut buf = [0u8; 1];
    crate::fw::kv::namespace(KV_NAMESPACE)
        .get(KV_KEY, &mut buf)
        .await
        .is_ok()
}

/// Mark the boot slideshow as shown so it never replays automatically.
async fn mark_shown() {
    let _ = crate::fw::kv::namespace(KV_NAMESPACE)
        .set(KV_KEY, &[1], true)
        .await;
}

// ── Filename generation ──────────────────────────────────────────────────────

const HEX: &[u8; 16] = b"0123456789ABCDEF";

/// Build FAT12 8.3 filename for sponsor slide `index` of `group`.
/// Format: `030G FF  PCX` — `030` prefix, `G` = group hex, `FF` = index hex.
/// Group 0 (`0300xx`) = badge hardware sponsors; group 1 (`0301xx`) =
/// BornHack event sponsors.  (Sponsors use prefix `03`; the slug pet owns
/// the `02xx` range.)
fn slide_filename(group: u8, index: u8) -> [u8; 11] {
    [
        b'0',
        b'3',
        b'0',
        HEX[(group & 0xF) as usize],
        HEX[(index >> 4) as usize],
        HEX[(index & 0xF) as usize],
        b' ',
        b' ',
        b'P',
        b'C',
        b'X',
    ]
}

/// Slide groups.
const GROUP_BADGE: u8 = 0;
const GROUP_EVENT: u8 = 1;

// ── On-demand show request ─────────────────────────────────────────────────

use core::sync::atomic::{AtomicBool, Ordering};

/// Set by the "Badge sponsors" / "Sponsors" menu items; drained by
/// [`run_if_requested`] in the display loop.  Nothing is shown at boot.
static SHOW_BADGE: AtomicBool = AtomicBool::new(false);
static SHOW_EVENT: AtomicBool = AtomicBool::new(false);

/// Request the "Badge sponsors" (hardware) slideshow (sync, for menu callbacks).
pub fn request_show_badge() {
    SHOW_BADGE.store(true, Ordering::Relaxed);
}

/// Request the "Sponsors" (BornHack event) slideshow (sync, for menu callbacks).
pub fn request_show_event() {
    SHOW_EVENT.store(true, Ordering::Relaxed);
}

/// If a slideshow was requested, play it now.  Call from the display loop,
/// which owns `display` + `button_rcvr`.  No-op when nothing was requested or
/// the requested group has no slides on flash.
pub async fn run_if_requested(
    display: &mut EpdGfx<'_>,
    button_rcvr: &mut embassy_sync::watch::Receiver<
        '_,
        embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex,
        u8,
        2,
    >,
) {
    if SHOW_BADGE.swap(false, Ordering::Relaxed) {
        show_slideshow(display, button_rcvr, GROUP_BADGE, SLIDE_DURATION_SECS).await;
    }
    if SHOW_EVENT.swap(false, Ordering::Relaxed) {
        show_slideshow(display, button_rcvr, GROUP_EVENT, SLIDE_DURATION_SECS).await;
    }
}

/// Seconds per slide for the first-boot slideshow (faster than the on-demand
/// menu view).
const BOOT_SLIDE_SECS: u64 = 2;

/// First-boot slideshow: all event sponsors, then the badge hardware sponsors.
/// Runs once per badge (guarded by the ekv `shown` flag), before the main
/// display loop takes the panel.  Replay any time from the root menu.
pub async fn run_boot_slideshow(
    display: &mut EpdGfx<'_>,
    button_rcvr: &mut embassy_sync::watch::Receiver<
        '_,
        embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex,
        u8,
        2,
    >,
) {
    if already_shown().await {
        return;
    }
    show_slideshow(display, button_rcvr, GROUP_EVENT, BOOT_SLIDE_SECS).await;
    show_slideshow(display, button_rcvr, GROUP_BADGE, BOOT_SLIDE_SECS).await;
    mark_shown().await;
}

// ── Slideshow runner ─────────────────────────────────────────────────────────

/// Play a group's logo slideshow to completion.  `button_rcvr` advances
/// slides early.  No-op (bar the intro is skipped too) when the group has no
/// slides on flash.
async fn show_slideshow(
    display: &mut EpdGfx<'_>,
    button_rcvr: &mut embassy_sync::watch::Receiver<
        '_,
        embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex,
        u8,
        2,
    >,
    group: u8,
    secs: u64,
) {
    if !any_slide_present(group).await {
        defmt::info!("sponsors: group {} has no slides — nothing to show", group);
        return;
    }

    // ── Intro screen ─────────────────────────────────────────────────
    display.clear(Color::White);

    let centered = TextStyleBuilder::new()
        .baseline(Baseline::Middle)
        .alignment(Alignment::Center)
        .build();
    let font = MonoTextStyle::new(&FONT_7X13_BOLD, Color::Black);

    let intro: &[&str] = if group == GROUP_EVENT {
        &["BornHack 2026", "thanks its", "sponsors!"]
    } else {
        &["This badge has", "been made possible", "by our awesome", "sponsors!"]
    };
    let n = intro.len() as i32;
    let mut y = 76 - (n - 1) * 8;
    for line in intro {
        let _ = Text::with_text_style(line, Point::new(76, y), font, centered).draw(display);
        y += 16;
    }

    let _ = display.reset().await;
    let _ = display.update_tc(crate::fw::epd::current_lut_speed()).await;
    let _ = display.deep_sleep().await;

    wait_or_button(button_rcvr, secs).await;

    // ── Logo slides ──────────────────────────────────────────────────
    for i in 0..MAX_SPONSORS as u8 {
        let name = slide_filename(group, i);
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

        wait_or_button(button_rcvr, secs).await;
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

/// Returns true if at least one slide of `group` exists on the FAT partition.
async fn any_slide_present(group: u8) -> bool {
    for i in 0..MAX_SPONSORS as u8 {
        let name = slide_filename(group, i);
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
