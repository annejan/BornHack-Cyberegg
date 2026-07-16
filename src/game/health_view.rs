//! Health status view — weight/diabetes "modifiers" at a glance.
//!
//! A full-screen overlay opened from the Stats modal, same pattern as
//! `traits_view`. Any button closes it.

use core::sync::atomic::{AtomicBool, Ordering};

use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{PrimitiveStyle, Rectangle};
use embedded_graphics::text::{Baseline, Text, TextStyleBuilder};

use crate::ui::{self, TEXT_BOLD_BLACK};
use crate::{RED, TriColor, WHITE};

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
    ui::draw_title_bar(display, "Health Status", Point::zero(), 152)?;

    let left = TextStyleBuilder::new().baseline(Baseline::Top).build();

    let Some(stats) = super::lifecycle::cycle() else {
        ui::draw_centered_message(display, "No pet yet", Point::new(76, 85))?;
        return Ok(());
    };

    const ROW_X: i32 = 6;
    // 18px rows (down from the original 20px used for 2 rows) — needed
    // to fit a 3rd row (Alcoholic) plus the Fit bar and a footnote
    // inside the 152px screen without anything running off the bottom.
    const ROW_H: i32 = 18;
    const ROWS_Y: i32 = 28;

    // Each row: a label and a value, value in red when it's the "bad"
    // state (diabetic / overweight / alcoholic) so it stands out the
    // same way critical stat bars do elsewhere in the UI.
    let rows: [(&str, bool); 3] = [
        ("Diabetic", stats.diabetic),
        ("Overweight", stats.overweight),
        ("Alcoholic", stats.alcoholic),
    ];

    for (i, (label, is_bad)) in rows.iter().enumerate() {
        let y = ROWS_Y + i as i32 * ROW_H;
        Text::with_text_style(label, Point::new(ROW_X, y), TEXT_BOLD_BLACK, left).draw(display)?;
        let value_style = embedded_graphics::mono_font::MonoTextStyle::new(
            &embedded_graphics::mono_font::iso_8859_1::FONT_7X13_BOLD,
            if *is_bad { RED } else { crate::BLACK },
        );
        let value = if *is_bad { "Yes" } else { "No" };
        Text::with_text_style(
            value,
            Point::new(ROW_X + 100, y),
            value_style,
            TextStyleBuilder::new().baseline(Baseline::Top).build(),
        )
        .draw(display)?;
    }

    // Weight as a bar, same widget every other stat uses, so it reads
    // consistently with the main Stats view.
    let weight_y = ROWS_Y + rows.len() as i32 * ROW_H + 4;
    let fill_color = if stats.weight < 25 { RED } else { crate::BLACK };
    super::stat_bar::draw_stat_bar(
        display,
        "Fit",
        stats.weight,
        Point::new(ROW_X, weight_y + 2),
        Point::new(ROW_X + 40, weight_y),
        Size::new(84, 16),
        fill_color,
    )?;

    // Footnote — condensed to a single line now that a 3rd row shares
    // the same screen; the detailed per-condition breakdown lives in
    // GAME.md instead of trying to fit it here too.
    let note_y = weight_y + 22;
    Text::with_text_style(
        "Sustained neglect -> permanent.",
        Point::new(ROW_X, note_y),
        TEXT_BOLD_BLACK,
        left,
    )
    .draw(display)?;

    // Lifetime mesh Battle record — see `game::battle`. Plenty of
    // vertical room left below the footnote (screen is 152px tall).
    let mut record: heapless::String<24> = heapless::String::new();
    let _ = core::fmt::Write::write_fmt(
        &mut record,
        format_args!("Battles: {}W-{}L", stats.wins, stats.losses),
    );
    Text::with_text_style(
        record.as_str(),
        Point::new(ROW_X, note_y + 16),
        TEXT_BOLD_BLACK,
        left,
    )
    .draw(display)?;

    Ok(())
}
