//! Rolled-stats view — display the current pet's vitality, curiosity
//! and resilience as bars with percentages.
//!
//! A full-screen overlay opened from the Stats modal.  Any button closes it.

use core::sync::atomic::{AtomicBool, Ordering};

use embedded_graphics::{
    mono_font::{ascii::{FONT_7X13, FONT_7X13_BOLD}, MonoTextStyle},
    prelude::*,
    primitives::{PrimitiveStyle, Rectangle},
    text::{Alignment, Baseline, Text, TextStyleBuilder},
};

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
    Rectangle::new(Point::zero(), Size::new(152, 18))
        .into_styled(PrimitiveStyle::with_fill(BLACK))
        .draw(display)?;
    let title_style = TextStyleBuilder::new()
        .baseline(Baseline::Middle)
        .alignment(Alignment::Center)
        .build();
    Text::with_text_style(
        "Rolled Stats",
        Point::new(76, 9),
        MonoTextStyle::new(&FONT_7X13_BOLD, WHITE),
        title_style,
    )
    .draw(display)?;

    let font = MonoTextStyle::new(&FONT_7X13, BLACK);
    let font_bold = MonoTextStyle::new(&FONT_7X13_BOLD, BLACK);
    let left = TextStyleBuilder::new().baseline(Baseline::Top).build();

    // Header: name (if any), kind, generation.
    let name = super::lifecycle::pet_name();
    let kind_name = super::lifecycle::pet_kind().name();
    let generation = super::lifecycle::pet_generation();
    let mut header: heapless::String<28> = heapless::String::new();
    if !name.is_empty() {
        let _ = core::fmt::Write::write_fmt(
            &mut header,
            format_args!("{} ({})", name, kind_name),
        );
    } else {
        let _ = core::fmt::Write::write_fmt(
            &mut header,
            format_args!("{} Gen {}", kind_name, generation),
        );
    }
    Text::with_text_style(header.as_str(), Point::new(4, 24), font_bold, left)
        .draw(display)?;

    // Fetch traits — if no game is active, show a placeholder.
    let Some((vit, cur, res)) = super::lifecycle::pet_traits() else {
        let centered = TextStyleBuilder::new()
            .baseline(Baseline::Middle)
            .alignment(Alignment::Center)
            .build();
        Text::with_text_style(
            "No pet yet",
            Point::new(76, 85),
            font,
            centered,
        )
        .draw(display)?;
        return Ok(());
    };

    // Trait bars.
    const BAR_MAX_W: u32 = 80;
    const BAR_H:     u32 = 10;
    const BAR_X:     i32 = 60;
    const LABEL_X:   i32 = 4;
    const ROW_H:     i32 = 22;
    const ROWS_Y:    i32 = 50;

    let bars: [(&str, u16); 3] = [
        ("Vitality",   vit),
        ("Curiosity",  cur),
        ("Resilience", res),
    ];

    let right = TextStyleBuilder::new()
        .baseline(Baseline::Top)
        .alignment(Alignment::Right)
        .build();

    for (i, (label, value)) in bars.iter().enumerate() {
        let y = ROWS_Y + i as i32 * ROW_H;

        // Label.
        Text::with_text_style(label, Point::new(LABEL_X, y), font, left)
            .draw(display)?;

        // Bar outline.
        Rectangle::new(Point::new(BAR_X, y), Size::new(BAR_MAX_W, BAR_H))
            .into_styled(PrimitiveStyle::with_stroke(BLACK, 1))
            .draw(display)?;

        // Bar fill — value is u16 (0..=65535), scale to BAR_MAX_W - 2.
        let pct = *value as u32 * 100 / 65535;
        let fill_w = (*value as u32 * (BAR_MAX_W - 2)) / 65535;
        if fill_w > 0 {
            Rectangle::new(
                Point::new(BAR_X + 1, y + 1),
                Size::new(fill_w, BAR_H - 2),
            )
            .into_styled(PrimitiveStyle::with_fill(BLACK))
            .draw(display)?;
        }

        // Percentage to the right of the bar.
        let mut pct_str: heapless::String<8> = heapless::String::new();
        let _ = core::fmt::Write::write_fmt(&mut pct_str, format_args!("{}%", pct));
        Text::with_text_style(
            pct_str.as_str(),
            Point::new(148, y),
            font,
            right,
        )
        .draw(display)?;
    }

    Ok(())
}
