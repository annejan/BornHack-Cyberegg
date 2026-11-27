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
use embedded_graphics::text::{Alignment, Baseline, Text, TextStyle, TextStyleBuilder};

use crate::{BLACK, RED, TriColor, WHITE};

// ---------------------------------------------------------------------------
// Shared text styles
// ---------------------------------------------------------------------------
//
// These collapse the ~24 inline `MonoTextStyle::new(...)` invocations
// scattered across the game's render code.  Use the constant rather
// than re-creating the style at each call site.

// Alignment / baseline pairs.  Named `<horizontal>_<vertical>` for the
// centred variants and `<vertical>_<horizontal>` for the edge-anchored
// ones, matching how the call sites read ("centred, top baseline").

/// Centred on both axes (`Alignment::Center` + `Baseline::Middle`).
pub const CENTERED: TextStyle = TextStyleBuilder::new()
    .baseline(Baseline::Middle)
    .alignment(Alignment::Center)
    .build();

/// Horizontally centred, `Baseline::Top`.
pub const CENTERED_TOP: TextStyle = TextStyleBuilder::new()
    .baseline(Baseline::Top)
    .alignment(Alignment::Center)
    .build();

/// Horizontally centred, `Baseline::Bottom`.
pub const CENTERED_BOTTOM: TextStyle = TextStyleBuilder::new()
    .baseline(Baseline::Bottom)
    .alignment(Alignment::Center)
    .build();

/// Left-aligned, `Baseline::Top`.
pub const TOP_LEFT: TextStyle = TextStyleBuilder::new().baseline(Baseline::Top).build();

/// Right-aligned, `Baseline::Top`.
pub const TOP_RIGHT: TextStyle = TextStyleBuilder::new()
    .baseline(Baseline::Top)
    .alignment(Alignment::Right)
    .build();

/// Right-aligned, `Baseline::Middle`.
pub const MIDDLE_RIGHT: TextStyle = TextStyleBuilder::new()
    .baseline(Baseline::Middle)
    .alignment(Alignment::Right)
    .build();

/// Left-aligned, `Baseline::Middle`.
pub const MIDDLE_LEFT: TextStyle = TextStyleBuilder::new()
    .baseline(Baseline::Middle)
    .alignment(Alignment::Left)
    .build();

/// Left-aligned, `Baseline::Bottom`.
pub const BOTTOM_LEFT: TextStyle = TextStyleBuilder::new().baseline(Baseline::Bottom).build();

/// Right-aligned, `Baseline::Bottom`.
pub const BOTTOM_RIGHT: TextStyle = TextStyleBuilder::new()
    .baseline(Baseline::Bottom)
    .alignment(Alignment::Right)
    .build();

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

/// 7×13 bold, drawn in `RED` — warnings / emphasis on white.
pub const TEXT_BOLD_RED: MonoTextStyle<'static, TriColor> = MonoTextStyle::new(&FONT_7X13_BOLD, RED);

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

    draw_text(
        display,
        title,
        Point::new(
            origin.x + width as i32 / 2,
            origin.y + TITLE_BAR_H as i32 / 2,
        ),
        TEXT_BOLD_WHITE,
        CENTERED,
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Popover frame
// ---------------------------------------------------------------------------

/// Dithered (50 % checkerboard) black/white border of `thick` pixels just
/// inside `rect`. Pure B/W so it survives the fast LUT refresh. The dither
/// phase is `(x + y) & 1 == 0 -> BLACK` in absolute screen coordinates.
pub fn draw_dither_ring<D>(display: &mut D, rect: Rectangle, thick: i32) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let (x, y) = (rect.top_left.x, rect.top_left.y);
    let (w, h) = (rect.size.width as i32, rect.size.height as i32);
    let pixels = (0..h).flat_map(move |dy| {
        (0..w).filter_map(move |dx| {
            let on_border = dx < thick || dy < thick || dx >= w - thick || dy >= h - thick;
            if !on_border {
                return None;
            }
            let px = x + dx;
            let py = y + dy;
            let color = if (px + py) & 1 == 0 { BLACK } else { WHITE };
            Some(Pixel(Point::new(px, py), color))
        })
    });
    display.draw_iter(pixels)
}

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

/// Full-screen double border: a 2 px black outer frame with a red inner frame
/// of `inner_red_w` px just inside it. Used by the About / LoRa-info /
/// confirmation screens.
pub fn draw_screen_border<D>(display: &mut D, inner_red_w: u32) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    Rectangle::new(Point::new(0, 0), Size::new(152, 152))
        .into_styled(PrimitiveStyle::with_stroke(BLACK, 2))
        .draw(display)?;
    Rectangle::new(Point::new(2, 2), Size::new(148, 148))
        .into_styled(PrimitiveStyle::with_stroke(RED, inner_red_w))
        .draw(display)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Full-screen clear
// ---------------------------------------------------------------------------

/// Wipe the whole 152×152 panel to white.  Every screen that owns the
/// full display starts with this so the previous frame doesn't bleed
/// through.  `DrawTarget::clear` fills the target's bounding box, which
/// is exactly 152×152 on every panel adapter and the simulator.
pub fn clear_screen<D>(display: &mut D) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    display.clear(WHITE)
}

// ---------------------------------------------------------------------------
// Fake-bold text
// ---------------------------------------------------------------------------

/// Draw `text` with a fake-bold effect for fonts that have no bold
/// variant (e.g. the 6×10 face — embedded-graphics' `iso_8859_1`
/// catalogue stops at `FONT_6X13_BOLD`, and moving up to 6×13 would
/// disturb the layouts built around the 6×10 cell).  The text is drawn
/// twice, the second pass shifted 1 px right, which doubles the vertical
/// stroke width without changing layout — the difference between a 1 px
/// and a 2 px stroke on the e-ink panel.
pub fn draw_text_fake_bold<D>(
    display: &mut D,
    text: &str,
    position: Point,
    character_style: MonoTextStyle<'_, TriColor>,
    text_style: TextStyle,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    Text::with_text_style(text, position, character_style, text_style).draw(display)?;
    Text::with_text_style(
        text,
        Point::new(position.x + 1, position.y),
        character_style,
        text_style,
    )
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
    draw_text(display, text, pos, TEXT_BLACK, CENTERED)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Replacement-aware text
// ---------------------------------------------------------------------------

/// Drop-in for `Text::with_text_style(text, position, style, text_style)
/// .draw(display)` that renders any codepoint the ISO-8859-1 fonts cannot
/// display as the U+FFFD `�` diamond (and known emojis via the atlas), instead
/// of the font's default missing-glyph box.  Use this for every dynamic or
/// untrusted string.  Honours `Left`/`Center`/`Right` alignment and returns the
/// end cursor, exactly like `Text::draw`.
pub fn draw_text<D>(
    display: &mut D,
    text: &str,
    position: Point,
    style: MonoTextStyle<'_, TriColor>,
    text_style: TextStyle,
) -> Result<Point, D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    crate::emoji::draw_string(display, text, position, style, text_style)
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
    let scroll = if total <= MAX_VISIBLE || pos < MAX_VISIBLE {
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
    let left_style = MIDDLE_LEFT;

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
            draw_text(
                display,
                label,
                Point::new(inner_x + 6, row_mid),
                TEXT_WHITE,
                left_style,
            )?;
        } else {
            draw_text(
                display,
                label,
                Point::new(inner_x + 6, row_mid),
                TEXT_BLACK,
                left_style,
            )?;
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

// ---------------------------------------------------------------------------
// Wrapping list cursor
// ---------------------------------------------------------------------------

/// Step a wrapping list cursor up: `pos - 1`, or `count - 1` when already
/// at the top. With `count == 0` the cursor stays put. A stale `pos >= count`
/// simply steps down by one.
pub fn wrap_prev(pos: u8, count: u8) -> u8 {
    if pos > 0 {
        pos - 1
    } else {
        count.saturating_sub(1)
    }
}

/// Step a wrapping list cursor down: `pos + 1`, or `0` when at (or past) the
/// last item. With `count == 0` the cursor stays put.
pub fn wrap_next(pos: u8, count: u8) -> u8 {
    if count == 0 {
        pos
    } else if pos.saturating_add(1) >= count {
        0
    } else {
        pos + 1
    }
}

#[cfg(test)]
mod wrap_tests {
    use super::{wrap_next, wrap_prev};

    #[test]
    fn prev_wraps_to_last() {
        assert_eq!(wrap_prev(0, 5), 4);
        assert_eq!(wrap_prev(3, 5), 2);
        assert_eq!(wrap_prev(0, 1), 0);
    }

    #[test]
    fn next_wraps_to_first() {
        assert_eq!(wrap_next(4, 5), 0);
        assert_eq!(wrap_next(0, 5), 1);
        assert_eq!(wrap_next(0, 1), 0);
    }

    #[test]
    fn empty_list_keeps_position() {
        assert_eq!(wrap_prev(0, 0), 0);
        assert_eq!(wrap_next(0, 0), 0);
        assert_eq!(wrap_next(7, 0), 7);
    }

    #[test]
    fn stale_position_past_end() {
        assert_eq!(wrap_next(9, 5), 0);
        assert_eq!(wrap_prev(9, 5), 8);
        assert_eq!(wrap_next(255, 5), 0);
    }

    #[test]
    fn full_lap_returns_to_start() {
        let n = 4;
        let mut p = 2;
        for _ in 0..n {
            p = wrap_next(p, n);
        }
        assert_eq!(p, 2);
        for _ in 0..n {
            p = wrap_prev(p, n);
        }
        assert_eq!(p, 2);
    }
}
