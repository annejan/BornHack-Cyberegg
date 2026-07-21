//! Screen lock.
//!
//! Holding the Cancel button for [`HOLD`] toggles a global lock (see
//! `crate::fw::button::run_buttons`). While locked, every button press is
//! swallowed upstream of both input sinks (game and menu), so the badge
//! ignores all input except the next Cancel hold, which unlocks it. A release
//! between lock and unlock is guaranteed because the hold detector waits for
//! Cancel to go high before returning.
//!
//! When locked, [`draw`] paints a red padlock in the centre of the screen. It
//! is drawn after the active screen but *before* the BLE PIN overlay, so the
//! pairing popup still takes priority over the lock icon.

use core::sync::atomic::{AtomicBool, Ordering};

use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::mono_font::iso_8859_1::FONT_7X13_BOLD;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{PrimitiveStyle, Rectangle};
use embedded_graphics::text::{Alignment, Baseline, Text, TextStyleBuilder};

use crate::{RED, TriColor, WHITE};

static LOCKED: AtomicBool = AtomicBool::new(false);

/// Is the screen currently locked?
pub fn is_active() -> bool {
    LOCKED.load(Ordering::Relaxed)
}

/// Flip the lock state (lock if unlocked, unlock if locked).
pub fn toggle() {
    LOCKED.fetch_xor(true, Ordering::Relaxed);
}

/// Draw a red padlock centred on the 152x152 display.
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
