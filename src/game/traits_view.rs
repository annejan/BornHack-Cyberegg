//! Rolled-stats view — display the current pet's vitality, curiosity
//! and resilience as bars with percentages.
//!
//! A full-screen overlay opened from the Stats modal.  Any button closes it.

use core::sync::atomic::{AtomicBool, Ordering};

use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{PrimitiveStyle, Rectangle};
use embedded_graphics::text::{Baseline, Text, TextStyleBuilder};

use super::stat_bar::draw_stat_bar;
use crate::ui::{self, TEXT_BOLD_BLACK};
use crate::{BLACK, TriColor, WHITE};

static ACTIVE: AtomicBool = AtomicBool::new(false);

pub fn is_active() -> bool {
    ACTIVE.load(Ordering::Relaxed)
}

pub fn open() {
    ACTIVE.store(true, Ordering::Relaxed);
}

pub fn close() {
    ACTIVE.store(false, Ordering::Relaxed);
}

pub fn draw<D>(display: &mut D) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    // Background.
    Rectangle::new(Point::zero(), Size::new(152, 152))
        .into_styled(PrimitiveStyle::with_fill(WHITE))
        .draw(display)?;

    // Title bar.
    ui::draw_title_bar(display, "Rolled Stats", Point::zero(), 152)?;

    let left = TextStyleBuilder::new().baseline(Baseline::Top).build();

    // Header: name (if any), kind, generation.
    let name = super::lifecycle::pet_name();
    let kind_name = super::lifecycle::pet_kind().name();
    let generation = super::lifecycle::pet_generation();
    let mut header: heapless::String<28> = heapless::String::new();
    if !name.is_empty() {
        let _ = core::fmt::Write::write_fmt(&mut header, format_args!("{} ({})", name, kind_name));
    } else {
        let _ = core::fmt::Write::write_fmt(
            &mut header,
            format_args!("{} Gen {}", kind_name, generation),
        );
    }
    Text::with_text_style(header.as_str(), Point::new(4, 24), TEXT_BOLD_BLACK, left)
        .draw(display)?;

    // Fetch traits — if no game is active, show a placeholder.
    let Some((vit, cur, res)) = super::lifecycle::pet_traits() else {
        ui::draw_centered_message(display, "No pet yet", Point::new(76, 85))?;
        return Ok(());
    };

    // Trait bars — geometry chosen so the longest label ("Resilience",
    // 10 chars × 7 px = 70 px) clears the bar, and the bar extends close
    // to the right edge of the display now that the percentage lives
    // inside the bar rather than in a separate column.
    const BAR_X: i32 = 78;
    const BAR_RIGHT: i32 = 148;
    const BAR_W: u32 = (BAR_RIGHT - BAR_X) as u32; // 70
    const BAR_H: u32 = 16;
    const LABEL_X: i32 = 4;
    const ROW_H: i32 = 22;
    const ROWS_Y: i32 = 50;

    let bars: [(&str, u16); 3] = [("Vitality", vit), ("Curiosity", cur), ("Resilience", res)];

    for (i, (label, value)) in bars.iter().enumerate() {
        let y = ROWS_Y + i as i32 * ROW_H;
        // Label vertically centred against the taller bar.
        let label_y = y + (BAR_H as i32 - 13) / 2;
        let pct = (*value as u32 * 100 / 65535) as u8;

        draw_stat_bar(
            display,
            label,
            pct,
            Point::new(LABEL_X, label_y),
            Point::new(BAR_X, y),
            Size::new(BAR_W, BAR_H),
            BLACK,
        )?;
    }

    Ok(())
}
