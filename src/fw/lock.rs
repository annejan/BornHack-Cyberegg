//! Screen lock.
//!
//! Holding the Cancel button for 3 s toggles a global lock (see
//! `crate::fw::button::run_buttons`). While locked, every button press is
//! swallowed upstream of both input sinks (game and menu), so the badge
//! ignores all input except the next Cancel hold, which unlocks it. A release
//! between lock and unlock is guaranteed because the hold detector waits for
//! Cancel to go high before returning.
//!
//! The padlock overlay is *transient*: it shows for [`OVERLAY_SECS`] then hides
//! itself so the underlying screen stays visible even though the keys remain
//! locked. Any key touch while locked re-shows it for another window. The
//! [`overlay_task`] owns this timer and every redraw wake; `run_buttons` only
//! calls [`poke`] to nudge it (on lock, on unlock, and on each swallowed key).
//!
//! When visible, [`draw`] paints the padlock after the active screen but
//! *before* the BLE PIN overlay, so the pairing popup keeps priority.

use core::sync::atomic::{AtomicBool, Ordering};

use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::mono_font::iso_8859_1::FONT_7X13_BOLD;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{PrimitiveStyle, Rectangle};
use embedded_graphics::text::{Alignment, Baseline, Text, TextStyleBuilder};

use crate::{RED, TriColor, WHITE};

/// How long the padlock stays on screen after each key touch.
#[cfg(feature = "embassy-base")]
const OVERLAY_SECS: u64 = 5;

/// True while input is locked (persists until the next unlock hold).
static LOCKED: AtomicBool = AtomicBool::new(false);
/// True while the padlock is currently drawn (auto-hides after `OVERLAY_SECS`).
static OVERLAY: AtomicBool = AtomicBool::new(false);

/// Is input currently locked?
pub fn is_active() -> bool {
    LOCKED.load(Ordering::Relaxed)
}

/// Should the padlock be drawn this frame?
pub fn overlay_visible() -> bool {
    OVERLAY.load(Ordering::Relaxed)
}

/// Flip the lock state, returning the new "is locked" value.
pub fn toggle() -> bool {
    !LOCKED.fetch_xor(true, Ordering::Relaxed)
}

/// Draw the red padlock and unlock hint centred on the 152x152 display.
///
/// Draws only the icon (no full-screen clear) so it sits on top of whatever
/// the active screen already rendered.
pub fn draw<D>(display: &mut D) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    // Shackle: an open-bottom red loop above the body.
    Rectangle::new(Point::new(66, 58), Size::new(20, 24))
        .into_styled(PrimitiveStyle::with_stroke(RED, 3))
        .draw(display)?;
    // Body: filled red block; overlaps the shackle's lower legs.
    Rectangle::new(Point::new(58, 74), Size::new(36, 28))
        .into_styled(PrimitiveStyle::with_fill(RED))
        .draw(display)?;
    // Keyhole: small punched-out mark so it reads as a lock.
    Rectangle::new(Point::new(74, 84), Size::new(4, 10))
        .into_styled(PrimitiveStyle::with_fill(WHITE))
        .draw(display)?;

    // Unlock hint below the padlock, red + bold. Fits one centred line at
    // 7 px/char within the 152 px panel.
    let bold_red = MonoTextStyle::new(&FONT_7X13_BOLD, RED);
    let centered = TextStyleBuilder::new()
        .baseline(Baseline::Middle)
        .alignment(Alignment::Center)
        .build();
    Text::with_text_style("Hold cancel to unlock", Point::new(76, 122), bold_red, centered)
        .draw(display)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Firmware-only overlay timer
// ---------------------------------------------------------------------------

/// Fired by `run_buttons` on every lock-relevant event (lock, unlock, or a key
/// touched while locked). The [`overlay_task`] wakes on it and re-evaluates the
/// overlay from [`is_active`].
#[cfg(feature = "embassy-base")]
static POKE: embassy_sync::signal::Signal<
    embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex,
    (),
> = embassy_sync::signal::Signal::new();

/// Nudge the overlay task: show/refresh the padlock while locked, or hide it on
/// unlock. Cheap and non-blocking — safe to call from the button task.
#[cfg(feature = "embassy-base")]
pub fn poke() {
    POKE.signal(());
}

/// Owns the padlock's visibility timer and every redraw wake.
///
/// Sleeps until [`poke`]d. While locked it shows the padlock and keeps it up
/// for `OVERLAY_SECS`, restarting that window on each further poke; after the
/// window elapses it hides the padlock but leaves the keys locked. A poke that
/// arrives while unlocked simply hides the padlock at once.
#[cfg(feature = "embassy-base")]
#[embassy_executor::task]
pub async fn overlay_task() {
    use embassy_futures::select::{Either, select};
    use embassy_time::{Duration, Timer};

    // Dedicated BTN_WATCH sender to wake the render loop. Values only need to
    // *change* to wake `button_rcvr.changed()`, so use a rolling counter kept
    // clear of the real 0..=6 button indices.
    let sender = crate::fw::button::BTN_WATCH.sender();
    let mut tick: u8 = 64;
    let mut wake = || {
        tick = tick.wrapping_add(1);
        sender.send(tick);
    };

    loop {
        POKE.wait().await;
        if !is_active() {
            // Poked while unlocked → clear the padlock.
            OVERLAY.store(false, Ordering::Relaxed);
            wake();
            continue;
        }
        // Locked: show the padlock and hold it for the window, refreshing on
        // each further poke until it times out or we get unlocked.
        loop {
            OVERLAY.store(true, Ordering::Relaxed);
            wake();
            match select(Timer::after(Duration::from_secs(OVERLAY_SECS)), POKE.wait()).await {
                Either::First(_) => break, // window elapsed → hide, stay locked
                Either::Second(_) => {
                    if !is_active() {
                        break; // unlocked mid-window → hide now
                    }
                    // else: another key touch, restart the window
                }
            }
        }
        OVERLAY.store(false, Ordering::Relaxed);
        wake();
    }
}
