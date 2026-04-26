//! Small reusable UI primitives shared across the game's overlays.
//!
//! Every full-screen and popover view in the game ends up drawing a
//! title bar, a white popover frame, or a centred "no data" message.
//! Before this module the same five-or-so lines lived inline in each
//! view (modal, pet_select, traits_view, realm_view, …); the helpers
//! here exist purely to keep those sites a one-liner with consistent
//! geometry and typography.

use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::mono_font::ascii::{FONT_7X13, FONT_7X13_BOLD};
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
