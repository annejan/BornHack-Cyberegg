//! Small reusable UI primitives shared across the game's overlays.
//!
//! Every full-screen and popover view in the game ends up drawing a
//! title bar, a white popover frame, or a centred "no data" message.
//! Before this module the same five-or-so lines lived inline in each
//! view (modal, pet_select, traits_view, realm_view, …); the helpers
//! here exist purely to keep those sites a one-liner with consistent
//! geometry and typography.

use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::mono_font::iso_8859_1::{FONT_7X13, FONT_7X13_BOLD};
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{PrimitiveStyle, Rectangle};
use embedded_graphics::text::{Alignment, Baseline, Text, TextStyleBuilder};

use crate::{BLACK, RED, TriColor, WHITE};

// ---------------------------------------------------------------------------
// Shared text styles
// ---------------------------------------------------------------------------
//
// These collapse the ~24 inline `MonoTextStyle::new(...)` invocations
// scattered across the game's render code.  Use the constant rather
// than re-creating the style at each call site.

/// 7×13 regular weight, drawn in `BLACK` — body text on white.
pub const TEXT_BLACK: MonoTextStyle<'static, TriColor> = MonoTextStyle::new(&FONT_7X13, BLACK);

/// 7×13 regular weight, drawn in `WHITE` — body text on a black fill.
pub const TEXT_WHITE: MonoTextStyle<'static, TriColor> = MonoTextStyle::new(&FONT_7X13, WHITE);

/// 7×13 bold, drawn in `BLACK` — emphasised body text / footers.
pub const TEXT_BOLD_BLACK: MonoTextStyle<'static, TriColor> =
    MonoTextStyle::new(&FONT_7X13_BOLD, BLACK);

/// 7×13 bold, drawn in `WHITE` — title-bar text on black.
pub const TEXT_BOLD_WHITE: MonoTextStyle<'static, TriColor> =
    MonoTextStyle::new(&FONT_7X13_BOLD, WHITE);

/// 7×13 regular weight, drawn in `RED` — used for invalid-signature
/// notices on the advert screen and any other "warning" body text.
pub const TEXT_RED: MonoTextStyle<'static, TriColor> = MonoTextStyle::new(&FONT_7X13, RED);

// ---------------------------------------------------------------------------
// Title bar
// ---------------------------------------------------------------------------

/// Standard title-bar height in pixels.
pub const TITLE_BAR_H: u32 = 18;

/// Draw a title bar — black-fill rectangle of `width × TITLE_BAR_H`
/// at `origin`, with `title` in centred bold white text.
///
/// Used at the top of every full-screen overlay (Pet Select, Rolled
/// Stats, Unicorn Realm, …) and inside the in-game modal popover.
pub fn draw_title_bar<D>(
    display: &mut D,
    title: &str,
    origin: Point,
    width: u32,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    Rectangle::new(origin, Size::new(width, TITLE_BAR_H))
        .into_styled(PrimitiveStyle::with_fill(BLACK))
        .draw(display)?;

    let style = TextStyleBuilder::new()
        .baseline(Baseline::Middle)
        .alignment(Alignment::Center)
        .build();
    Text::with_text_style(
        title,
        Point::new(
            origin.x + width as i32 / 2,
            origin.y + TITLE_BAR_H as i32 / 2,
        ),
        TEXT_BOLD_WHITE,
        style,
    )
    .draw(display)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Popover frame
// ---------------------------------------------------------------------------

/// Draw a popover frame: white-fill rectangle of `size` at `origin`,
/// surrounded by a `border_w`-pixel black stroke.  The in-game modal
/// uses this for both the menu and the stats sub-view; any future
/// dialog with the same look should reuse it rather than open-coding
/// the two draws.
pub fn draw_popover_frame<D>(
    display: &mut D,
    origin: Point,
    size: Size,
    border_w: u32,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    Rectangle::new(origin, size)
        .into_styled(PrimitiveStyle::with_fill(WHITE))
        .draw(display)?;
    Rectangle::new(origin, size)
        .into_styled(PrimitiveStyle::with_stroke(BLACK, border_w))
        .draw(display)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// "No data" placeholder
// ---------------------------------------------------------------------------

/// Draw a single-line message centred on `pos` with regular black
/// text.  Used by the empty-state placeholders ("No pet yet", "No
/// past pets yet", "No private messages", "No adverts", …).
pub fn draw_centered_message<D>(display: &mut D, text: &str, pos: Point) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let style = TextStyleBuilder::new()
        .baseline(Baseline::Middle)
        .alignment(Alignment::Center)
        .build();
    Text::with_text_style(text, pos, TEXT_BLACK, style).draw(display)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Picker overlay
// ---------------------------------------------------------------------------

/// Draw a centred popover with a title bar and a vertical list of
/// short labels — the selected row inverted.  Used by the mini-games
/// to show their pre-game difficulty picker; any other small
/// "choose-one" overlay should reuse this rather than open-coding it.
///
/// `pos` is the index of the currently-selected item.
pub fn draw_picker_menu<D>(
    display: &mut D,
    title: &str,
    items: &[&str],
    pos: usize,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    const MARGIN: i32 = 16;
    const W: u32 = 120; // 152 - 2 × MARGIN
    const TOP_Y: i32 = 36;
    const BORDER: u32 = 2;
    const ITEM_H: i32 = 18;
    const PADDING_BELOW_TITLE: i32 = 4;
    /// Maximum rows that fit on the 152-px display: list area is
    /// `152 - TOP_Y - 2·BORDER - TITLE_BAR_H - PADDING_BELOW_TITLE`
    /// = 152 - 36 - 4 - 18 - 4 = 90 px ÷ 18 px/row = 5 rows.
    const MAX_VISIBLE: usize = 5;

    // Cap visible rows so the popover never overflows the screen.
    let total = items.len();
    let visible = total.min(MAX_VISIBLE);

    // Slide the scroll window so `pos` is always visible.
    let scroll = if total <= MAX_VISIBLE {
        0
    } else if pos < MAX_VISIBLE {
        0
    } else {
        (pos + 1).saturating_sub(MAX_VISIBLE)
    };
    let end = (scroll + visible).min(total);

    let title_h = TITLE_BAR_H as i32;
    let total_h =
        BORDER as i32 + title_h + PADDING_BELOW_TITLE + ITEM_H * visible as i32 + BORDER as i32;

    draw_popover_frame(
        display,
        Point::new(MARGIN, TOP_Y),
        Size::new(W, total_h as u32),
        BORDER,
    )?;
    let inner_x = MARGIN + BORDER as i32;
    let inner_y = TOP_Y + BORDER as i32;
    let inner_w = W - BORDER * 2;
    draw_title_bar(display, title, Point::new(inner_x, inner_y), inner_w)?;

    let list_y = inner_y + title_h + PADDING_BELOW_TITLE;
    let left_style = TextStyleBuilder::new()
        .baseline(Baseline::Middle)
        .alignment(Alignment::Left)
        .build();

    for (row, label) in items[scroll..end].iter().enumerate() {
        let abs_idx = scroll + row;
        let row_top = list_y + row as i32 * ITEM_H;
        let row_mid = row_top + ITEM_H / 2;
        if abs_idx == pos {
            Rectangle::new(
                Point::new(inner_x, row_top),
                Size::new(inner_w, ITEM_H as u32),
            )
            .into_styled(PrimitiveStyle::with_fill(BLACK))
            .draw(display)?;
            Text::with_text_style(
                label,
                Point::new(inner_x + 6, row_mid),
                TEXT_WHITE,
                left_style,
            )
            .draw(display)?;
        } else {
            Text::with_text_style(
                label,
                Point::new(inner_x + 6, row_mid),
                TEXT_BLACK,
                left_style,
            )
            .draw(display)?;
        }
    }

    // Scroll indicators — small "·" chevrons in the right margin when
    // there are items above/below the current viewport.
    if scroll > 0 {
        Text::with_text_style(
            "^",
            Point::new(inner_x + inner_w as i32 - 8, list_y + 9),
            TEXT_BLACK,
            left_style,
        )
        .draw(display)?;
    }
    if end < total {
        let last_row_mid = list_y + (visible as i32 - 1) * ITEM_H + ITEM_H / 2;
        Text::with_text_style(
            "v",
            Point::new(inner_x + inner_w as i32 - 8, last_row_mid),
            TEXT_BLACK,
            left_style,
        )
        .draw(display)?;
    }
    Ok(())
}
