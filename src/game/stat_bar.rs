//! Shared "labelled stat bar with inline percentage" widget.
//!
//! Used by both the rolled-stats view (`game::traits_view`) and the
//! live stats modal (`game::modal::draw_stats_view`, through
//! [`draw_band_bar`]).
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
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{PrimitiveStyle, Rectangle};
use embedded_graphics::text::{Text};

use super::stats_pages::BarRow;
use crate::{BLACK, RED, TriColor, WHITE};

/// Draw a labelled bar with `pct%` (0–100) centred inside it.
///
/// * `label_pos` — top-left of the label text (baseline `Top`).
/// * `bar_origin` — top-left of the bar including the 1-px border.
/// * `bar_size` — outer dimensions of the bar (border included).
/// * `fill_color` — colour of the filled portion (`BLACK`, or `RED` for a
///   value that is out of its band, see [`draw_band_bar`]).
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
    draw_bar_body(display, label, pct, label_pos, bar_origin, bar_size, fill_color)?;
    draw_bar_pct(display, pct, bar_origin, bar_size)
}

/// A bar `w` px wide and 6 px high with a 1 px outline, filled `pct` from the
/// left, or from the right when `from_right`. No label, no number: the
/// pre-battle screen draws two of them facing each other.
pub fn draw_mirrored_bar<D>(
    d: &mut D,
    x: i32,
    y: i32,
    w: u32,
    pct: u8,
    from_right: bool,
    color: TriColor,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    const H: u32 = 6;
    Rectangle::new(Point::new(x, y), Size::new(w, H))
        .into_styled(PrimitiveStyle::with_stroke(BLACK, 1))
        .draw(d)?;
    let inner = w.saturating_sub(2);
    let fill = pct.min(100) as u32 * inner / 100;
    if fill > 0 {
        let fx = if from_right {
            x + 1 + (inner - fill) as i32
        } else {
            x + 1
        };
        Rectangle::new(Point::new(fx, y + 1), Size::new(fill, H - 2))
            .into_styled(PrimitiveStyle::with_fill(color))
            .draw(d)?;
    }
    Ok(())
}

/// The label, the outline and the fill of a bar (everything but the inline
/// percentage).
fn draw_bar_body<D>(
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
    // Label, left of the bar.
    Text::with_text_style(label, label_pos, crate::ui::TEXT_BLACK, crate::ui::TOP_LEFT)
        .draw(display)?;

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
    Ok(())
}

/// The inline percentage, drawn over the fill and the unfilled part.
fn draw_bar_pct<D>(
    display: &mut D,
    pct: u8,
    bar_origin: Point,
    bar_size: Size,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    // Inline percentage uses the bold variant of FONT_7X13 — bolder
    // glyphs survive the half-and-half clipping at the fill boundary
    // with better legibility than the regular weight.
    let pct_font_black = crate::ui::TEXT_BOLD_BLACK;
    let pct_font_white = crate::ui::TEXT_BOLD_WHITE;
    let centred = crate::ui::CENTERED;
    let pct = pct.min(100);
    let inner_w = bar_size.width.saturating_sub(2);
    let fill_w = (pct as u32 * inner_w) / 100;

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

/// Length of the black stub a tick leaves at the top and bottom of the bar.
const NOTCH_LEN: u32 = 3;

/// X of the tick for `pct` on a bar at `bar_x` that is `bar_w` wide (border
/// included): the inner width is mapped 0..=100 onto `bar_x + 1 ..`.
pub fn tick_x(bar_x: i32, bar_w: u32, pct: u8) -> i32 {
    bar_x + 1 + (pct.min(100) as u32 * bar_w.saturating_sub(2) / 100) as i32
}

/// [`draw_stat_bar`] for a [`BarRow`]: red fill when the row is out of its
/// band, plus one tick for each of the band edges and the cap. The ticks are
/// drawn before the inline percentage.
pub fn draw_band_bar<D>(
    display: &mut D,
    row: &BarRow,
    label_pos: Point,
    bar_origin: Point,
    bar_size: Size,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let fill = if row.red { RED } else { BLACK };
    draw_bar_body(display, row.label, row.pct, label_pos, bar_origin, bar_size, fill)?;

    let inner_w = bar_size.width.saturating_sub(2);
    let fill_end = bar_origin.x + 1 + (row.pct.min(100) as u32 * inner_w / 100) as i32;
    let inner_h = bar_size.height.saturating_sub(2);
    let bottom = bar_origin.y + bar_size.height as i32 - 1;
    for pct in [row.lo, row.hi, row.cap].into_iter().flatten() {
        let x = tick_x(bar_origin.x, bar_size.width, pct);
        let line_color = if x < fill_end { WHITE } else { BLACK };
        let black = PrimitiveStyle::with_fill(BLACK);
        // The line across the inner height, then a black stub at each end.
        Rectangle::new(Point::new(x, bar_origin.y + 1), Size::new(1, inner_h))
            .into_styled(PrimitiveStyle::with_fill(line_color))
            .draw(display)?;
        Rectangle::new(Point::new(x, bar_origin.y), Size::new(1, NOTCH_LEN))
            .into_styled(black)
            .draw(display)?;
        Rectangle::new(Point::new(x, bottom + 1 - NOTCH_LEN as i32), Size::new(1, NOTCH_LEN))
            .into_styled(black)
            .draw(display)?;
    }
    // The percentage goes last, so no tick cuts through its glyphs.
    draw_bar_pct(display, row.pct, bar_origin, bar_size)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::test_canvas::Canvas;

    #[test]
    fn tick_x_maps_percent_to_the_inner_bar() {
        assert_eq!(tick_x(76, 60, 0), 77);
        assert_eq!(tick_x(76, 60, 100), 135);
        assert_eq!(tick_x(76, 60, 50), 106);
    }

    #[test]
    fn band_bar_draws_ticks_and_red_fill() {
        let row = BarRow { label: "Weight", pct: 70, lo: Some(38), hi: Some(63), cap: None, red: true };
        let origin = Point::new(76, 40);
        let size = Size::new(60, 15);
        let mut c = Canvas::new();
        draw_band_bar(&mut c, &row, Point::new(16, 41), origin, size).unwrap();
        // The out-of-band fill is red, not black (row 2 below the top is
        // clear of the text, left of the first tick).
        assert_eq!(c.get(origin.x + 3, origin.y + 6), RED);
        // The tick for hi: black notch on the top and bottom border rows
        // and 2 px inside, white line over the red fill in between.
        let hx = tick_x(origin.x, size.width, 63);
        for dy in [0, 1, 2, 12, 13, 14] {
            assert_eq!(c.get(hx, origin.y + dy), BLACK, "dy {dy}");
        }
        assert_eq!(c.get(hx, origin.y + 4), WHITE);
        // The lo tick is inside the fill as well.
        let lx = tick_x(origin.x, size.width, 38);
        assert_eq!(c.get(lx, origin.y + 4), WHITE);
        assert_eq!(c.get(lx, origin.y), BLACK);
        // A black row stays black fill and ticks stay white; a cap past the
        // fill is a black line over the empty part.
        let row = BarRow { label: "Health", pct: 20, lo: Some(10), hi: None, cap: Some(80), red: false };
        let mut c = Canvas::new();
        draw_band_bar(&mut c, &row, Point::new(16, 41), origin, size).unwrap();
        assert_eq!(c.get(origin.x + 3, origin.y + 6), BLACK);
        let cx = tick_x(origin.x, size.width, 80);
        assert_eq!(c.get(cx, origin.y + 4), BLACK);
        assert_eq!(c.get(cx, origin.y + 12), BLACK);
        let lx = tick_x(origin.x, size.width, 10);
        assert_eq!(c.get(lx, origin.y + 4), WHITE);
    }

    /// A canvas that logs the order its pixels are drawn in.
    struct Logged {
        canvas: Canvas,
        order: std::vec::Vec<(i32, i32)>,
    }

    impl OriginDimensions for Logged {
        fn size(&self) -> Size {
            self.canvas.size()
        }
    }

    impl DrawTarget for Logged {
        type Color = TriColor;
        type Error = core::convert::Infallible;

        fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
        where
            I: IntoIterator<Item = Pixel<TriColor>>,
        {
            for Pixel(p, c) in pixels {
                self.order.push((p.x, p.y));
                self.canvas.draw_iter(core::iter::once(Pixel(p, c)))?;
            }
            Ok(())
        }
    }

    #[test]
    fn the_percentage_is_drawn_after_the_ticks() {
        // A tick at 50 % lands on the centre of "50%". Colours cannot tell the
        // tick from a glyph of the same colour, so the draw order is asserted:
        // the tick's top stub is written before the last pixel of the text.
        let row = BarRow { label: "Fun", pct: 50, lo: Some(50), hi: None, cap: None, red: false };
        let origin = Point::new(76, 40);
        let size = Size::new(60, 15);
        let mut d = Logged { canvas: Canvas::new(), order: std::vec::Vec::new() };
        draw_band_bar(&mut d, &row, Point::new(16, 41), origin, size).unwrap();
        let tx = tick_x(origin.x, size.width, 50);
        let stub = d.order.iter().rposition(|&p| p == (tx, origin.y)).unwrap();
        let in_text = |&(x, y): &(i32, i32)| (tx - 9..tx + 10).contains(&x) && (43..58).contains(&y);
        let last_text = d.order.iter().rposition(in_text).unwrap();
        assert!(stub < last_text, "stub {stub} text {last_text}");
        // The glyph pixels are still there.
        let area = Rectangle::new(Point::new(tx - 9, 43), Size::new(19, 14));
        assert!(d.canvas.any(area, WHITE) && d.canvas.any(area, BLACK));
    }

    #[test]
    fn mirrored_bar_fills_from_the_chosen_side() {
        let mut c = Canvas::new();
        draw_mirrored_bar(&mut c, 2, 10, 60, 50, false, BLACK).unwrap();
        assert_eq!(c.get(5, 13), BLACK, "left half filled");
        assert_eq!(c.get(55, 13), WHITE, "right half empty");
        let mut c = Canvas::new();
        draw_mirrored_bar(&mut c, 90, 10, 60, 50, true, BLACK).unwrap();
        assert_eq!(c.get(146, 13), BLACK, "right half filled");
        assert_eq!(c.get(95, 13), WHITE, "left half empty");
        // Never outside its 60 x 6 box.
        assert_eq!(c.get(89, 13), WHITE);
        assert_eq!(c.get(150, 13), WHITE);
        assert_eq!(c.get(120, 16), WHITE);
        // 0 % and 100 %.
        let mut c = Canvas::new();
        draw_mirrored_bar(&mut c, 2, 10, 60, 100, false, RED).unwrap();
        assert_eq!(c.get(60, 13), RED);
        let mut c = Canvas::new();
        draw_mirrored_bar(&mut c, 2, 10, 60, 0, false, RED).unwrap();
        assert_eq!(c.get(30, 13), WHITE);
    }
}
