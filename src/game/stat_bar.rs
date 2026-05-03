//! Shared "labelled stat bar with inline percentage" widget.
//!
//! Used by both the rolled-stats view (`game::traits_view`) and the
//! live stats modal (`game::modal::draw_stats_view`).
//!
//! Layout:
//!
//! ```text
//!     Hunger    ┌──────────────────┐
//!               │       73%        │  ← 13-px font centred inside bar
//!               └──────────────────┘
//! ```
//!
//! The percentage is rendered twice — white-on-filled and
//! black-on-unfilled — using `display.clipped(...)`.  The split is
//! exactly where the fill ends, so a glyph straddling the boundary
//! shows half-and-half but stays readable on both halves.

use embedded_graphics::draw_target::DrawTargetExt;
use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::mono_font::iso_8859_1::{FONT_7X13, FONT_7X13_BOLD};
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{PrimitiveStyle, Rectangle};
use embedded_graphics::text::{Alignment, Baseline, Text, TextStyleBuilder};

use crate::{BLACK, TriColor, WHITE};

/// Draw a labelled bar with `pct%` (0–100) centred inside it.
///
/// * `label_pos` — top-left of the label text (baseline `Top`).
/// * `bar_origin` — top-left of the bar including the 1-px border.
/// * `bar_size` — outer dimensions of the bar (border included).
/// * `fill_color` — colour of the filled portion (usually `BLACK`, but the live
///   stats view passes `RED` for values below 25 % so critical stats stand
///   out).
///
/// The bar must be at least 4 × 4 px or the inner fill region
/// degenerates; the percentage label assumes the inner height is at
/// least the font height (13 px) for the glyphs to fit cleanly, so
/// `bar_size.height` should be ≥ 15.
pub fn draw_stat_bar<D>(
    display: &mut D,
    label: &str,
    pct: u8,
    label_pos: Point,
    bar_origin: Point,
    bar_size: Size,
    fill_color: TriColor,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    // Inline percentage uses the bold variant of FONT_7X13 — bolder
    // glyphs survive the half-and-half clipping at the fill boundary
    // with better legibility than the regular weight.
    let label_font = MonoTextStyle::new(&FONT_7X13, BLACK);
    let pct_font_black = MonoTextStyle::new(&FONT_7X13_BOLD, BLACK);
    let pct_font_white = MonoTextStyle::new(&FONT_7X13_BOLD, WHITE);
    let left_top = TextStyleBuilder::new().baseline(Baseline::Top).build();
    let centred = TextStyleBuilder::new()
        .baseline(Baseline::Middle)
        .alignment(Alignment::Center)
        .build();

    // Label, left of the bar.
    Text::with_text_style(label, label_pos, label_font, left_top).draw(display)?;

    // Bar outline.
    Rectangle::new(bar_origin, bar_size)
        .into_styled(PrimitiveStyle::with_stroke(BLACK, 1))
        .draw(display)?;

    // Filled portion — proportional to pct.
    let pct = pct.min(100);
    let inner_w = bar_size.width.saturating_sub(2);
    let fill_w = (pct as u32 * inner_w) / 100;
    if fill_w > 0 {
        Rectangle::new(
            Point::new(bar_origin.x + 1, bar_origin.y + 1),
            Size::new(fill_w, bar_size.height.saturating_sub(2)),
        )
        .into_styled(PrimitiveStyle::with_fill(fill_color))
        .draw(display)?;
    }

    // Percentage text — drawn twice with mutually-exclusive clip rects.
    let mut pct_str: heapless::String<8> = heapless::String::new();
    let _ = core::fmt::Write::write_fmt(&mut pct_str, format_args!("{}%", pct));
    let text_pos = Point::new(
        bar_origin.x + (bar_size.width / 2) as i32,
        bar_origin.y + (bar_size.height / 2) as i32,
    );
    let bar_right = bar_origin.x + bar_size.width as i32;
    let split_x = bar_origin.x + 1 + fill_w as i32;

    // White copy clipped to the filled (black/red) region.
    if split_x > bar_origin.x {
        let filled = Rectangle::new(
            bar_origin,
            Size::new((split_x - bar_origin.x) as u32, bar_size.height),
        );
        let mut clipped = display.clipped(&filled);
        Text::with_text_style(pct_str.as_str(), text_pos, pct_font_white, centred)
            .draw(&mut clipped)?;
    }
    // Black copy clipped to the unfilled (white) region.
    if split_x < bar_right {
        let unfilled = Rectangle::new(
            Point::new(split_x, bar_origin.y),
            Size::new((bar_right - split_x) as u32, bar_size.height),
        );
        let mut clipped = display.clipped(&unfilled);
        Text::with_text_style(pct_str.as_str(), text_pos, pct_font_black, centred)
            .draw(&mut clipped)?;
    }

    Ok(())
}
