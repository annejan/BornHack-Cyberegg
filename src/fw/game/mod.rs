//! BornPets — game screen renderer (mock / Phase 3 placeholder).
//!
//! Layout (152×152 EPD, black & white):
//!
//! ```text
//! ┌───────────────────────────────────────┐  y =  0
//! │  [fork]  [bulb]  [bat]  [syringe]     │  y = 0–34   top icon row
//! ├───────────────────────────────────────┤  y = 34
//! │                                       │
//! │            [egg / pet]                │  y = 35–110  pet area
//! │                                       │
//! ├───────────────────────────────────────┤  y = 111
//! │  [duck]  [meter] [face] [2faces]      │  y = 111–152 bottom icon row
//! └───────────────────────────────────────┘  y = 152
//! ```
//!
//! The focused icon is highlighted: black filled circle + icon drawn in white.
//! Navigation state lives in [`nav`].

pub mod input;
pub mod modal;
pub mod nav;
pub use nav::{GameNav, Row};

use embedded_graphics::{
    prelude::*,
    primitives::{Circle, Ellipse, PrimitiveStyle, Rectangle},
};

use crate::{BLACK, TriColor, WHITE};

// ── Layout constants ──────────────────────────────────────────────────────────

/// X centres of the four icon columns (evenly spaced across 152 px).
const ICON_CX: [i32; 4] = [19, 57, 95, 133];
/// Y centre of the top icon row.
const TOP_CY: i32 = 17;
/// Y centre of the bottom icon row.
const BOT_CY: i32 = 131;
/// Y of the separator below the top icon row.
const SEP_TOP: i32 = 34;
/// Y of the separator above the bottom icon row.
const SEP_BOT: i32 = 111;
/// Radius of the selection circle background (diameter = 26 px).
const SEL_RADIUS: i32 = 13;

// ── Selection highlight ───────────────────────────────────────────────────────

/// Draw a filled black disc that serves as the inversion background for the
/// focused icon.  Call this *before* drawing the icon itself in [`WHITE`].
fn draw_selection_bg<D>(display: &mut D, cx: i32, cy: i32) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    Circle::new(
        Point::new(cx - SEL_RADIUS, cy - SEL_RADIUS),
        (SEL_RADIUS * 2) as u32,
    )
    .into_styled(PrimitiveStyle::with_fill(BLACK))
    .draw(display)
}

// ── Icon drawing ──────────────────────────────────────────────────────────────
// Every icon accepts `color: TriColor` so it can be drawn in either BLACK
// (normal) or WHITE (selected, on top of the black selection disc).

/// Fork & knife — feed action (icon 0, top row).
fn icon_fork<D>(display: &mut D, cx: i32, cy: i32, color: TriColor) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let s = PrimitiveStyle::with_fill(color);
    // Three tines at top
    for dx in [-5i32, -1, 3] {
        Rectangle::new(Point::new(cx + dx, cy - 7), Size::new(2, 5))
            .into_styled(s)
            .draw(display)?;
    }
    // Crossbar
    Rectangle::new(Point::new(cx - 5, cy - 2), Size::new(10, 1))
        .into_styled(s)
        .draw(display)?;
    // Handle (middle)
    Rectangle::new(Point::new(cx - 1, cy - 1), Size::new(2, 8))
        .into_styled(s)
        .draw(display)?;
    Ok(())
}

/// Lightbulb — toggle day/night (icon 1, top row).
fn icon_bulb<D>(display: &mut D, cx: i32, cy: i32, color: TriColor) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let fill = PrimitiveStyle::with_fill(color);
    let stroke = PrimitiveStyle::with_stroke(color, 1);
    // Bulb outline
    Circle::new(Point::new(cx - 5, cy - 8), 10)
        .into_styled(stroke)
        .draw(display)?;
    // Base stripes
    Rectangle::new(Point::new(cx - 3, cy + 2), Size::new(6, 1))
        .into_styled(fill)
        .draw(display)?;
    Rectangle::new(Point::new(cx - 3, cy + 4), Size::new(6, 1))
        .into_styled(fill)
        .draw(display)?;
    Rectangle::new(Point::new(cx - 2, cy + 6), Size::new(4, 2))
        .into_styled(fill)
        .draw(display)?;
    Ok(())
}

/// Bat & ball — play sub-menu (icon 2, top row).
fn icon_bat<D>(display: &mut D, cx: i32, cy: i32, color: TriColor) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let s = PrimitiveStyle::with_fill(color);
    // Bat head (wide)
    Rectangle::new(Point::new(cx - 7, cy - 5), Size::new(6, 4))
        .into_styled(s)
        .draw(display)?;
    // Bat handle (narrow)
    Rectangle::new(Point::new(cx - 6, cy - 1), Size::new(3, 8))
        .into_styled(s)
        .draw(display)?;
    // Ball (top-right)
    Rectangle::new(Point::new(cx + 2, cy - 7), Size::new(4, 4))
        .into_styled(s)
        .draw(display)?;
    Ok(())
}

/// Syringe — medicine (icon 3, top row; `dimmed` when pet is healthy).
fn icon_syringe<D>(
    display: &mut D,
    cx: i32,
    cy: i32,
    dimmed: bool,
    color: TriColor,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let fill = PrimitiveStyle::with_fill(color);
    let stroke = PrimitiveStyle::with_stroke(color, 1);
    // Plunger cap
    Rectangle::new(Point::new(cx - 4, cy - 8), Size::new(8, 2))
        .into_styled(fill)
        .draw(display)?;
    // Barrel (outline only when dimmed)
    Rectangle::new(Point::new(cx - 3, cy - 6), Size::new(6, 9))
        .into_styled(if dimmed { stroke } else { fill })
        .draw(display)?;
    // Needle
    Rectangle::new(Point::new(cx - 1, cy + 3), Size::new(2, 4))
        .into_styled(fill)
        .draw(display)?;
    Ok(())
}

/// Duck — rest / poop cleanup (icon 0, bottom row).
fn icon_duck<D>(display: &mut D, cx: i32, cy: i32, color: TriColor) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let stroke = PrimitiveStyle::with_stroke(color, 1);
    // Body
    Circle::new(Point::new(cx - 5, cy - 3), 10)
        .into_styled(stroke)
        .draw(display)?;
    // Head
    Circle::new(Point::new(cx - 5, cy - 10), 6)
        .into_styled(stroke)
        .draw(display)?;
    // Beak
    Rectangle::new(Point::new(cx + 1, cy - 8), Size::new(3, 2))
        .into_styled(PrimitiveStyle::with_fill(color))
        .draw(display)?;
    Ok(())
}

/// Health meter (bar chart) — stats overview (icon 1, bottom row).
fn icon_meter<D>(display: &mut D, cx: i32, cy: i32, color: TriColor) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let stroke = PrimitiveStyle::with_stroke(color, 1);
    let bottom = cy + 7;
    for (i, h) in [(0i32, 4u32), (1, 8), (2, 12)] {
        Rectangle::new(Point::new(cx - 5 + i * 5, bottom - h as i32), Size::new(3, h))
            .into_styled(stroke)
            .draw(display)?;
    }
    Ok(())
}

/// Talking head — discipline (icon 2, bottom row).
fn icon_face<D>(display: &mut D, cx: i32, cy: i32, color: TriColor) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let fill = PrimitiveStyle::with_fill(color);
    let stroke = PrimitiveStyle::with_stroke(color, 1);
    // Face outline
    Circle::new(Point::new(cx - 6, cy - 6), 12)
        .into_styled(stroke)
        .draw(display)?;
    // Eyes
    Rectangle::new(Point::new(cx - 3, cy - 3), Size::new(2, 2))
        .into_styled(fill)
        .draw(display)?;
    Rectangle::new(Point::new(cx + 1, cy - 3), Size::new(2, 2))
        .into_styled(fill)
        .draw(display)?;
    // Mouth (open)
    Rectangle::new(Point::new(cx - 2, cy + 2), Size::new(4, 2))
        .into_styled(fill)
        .draw(display)?;
    Ok(())
}

/// Two faces — attention acknowledge (icon 3, bottom row; `dimmed` until attention needed).
fn icon_twofaces<D>(
    display: &mut D,
    cx: i32,
    cy: i32,
    dimmed: bool,
    color: TriColor,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let stroke = PrimitiveStyle::with_stroke(color, 1);
    if dimmed {
        // Placeholder outline only
        Rectangle::new(Point::new(cx - 8, cy - 5), Size::new(16, 10))
            .into_styled(stroke)
            .draw(display)?;
    } else {
        Circle::new(Point::new(cx - 9, cy - 4), 8).into_styled(stroke).draw(display)?;
        Circle::new(Point::new(cx + 1, cy - 4), 8).into_styled(stroke).draw(display)?;
    }
    Ok(())
}

// ── Egg sprite ────────────────────────────────────────────────────────────────

/// Draw the egg sprite centred at `center` (used while `is_egg == true`).
fn draw_egg<D>(display: &mut D, center: Point) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    Ellipse::new(Point::new(center.x - 16, center.y - 21), Size::new(32, 42))
        .into_styled(PrimitiveStyle::with_stroke(BLACK, 2))
        .draw(display)?;
    Ok(())
}

// ── Public entry point ────────────────────────────────────────────────────────

/// Render the BornPets game screen (screen 0).
///
/// `nav` controls which icon is highlighted (black disc + white icon).
/// Game state flags will be wired in later phases.
pub fn draw_screen_game<D>(display: &mut D, nav: GameNav) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    // TODO(Phase 3): derive icon visibility from live GameState.
    let syringe_active = false;
    let attention = false;

    // ── Top icon row ──────────────────────────────────────────────────────────
    for (i, &cx) in ICON_CX.iter().enumerate() {
        let selected = nav.row == Row::Top && nav.col == i as u8;
        let fg = if selected { WHITE } else { BLACK };
        if selected {
            draw_selection_bg(display, cx, TOP_CY)?;
        }
        match i {
            0 => icon_fork(display, cx, TOP_CY, fg)?,
            1 => icon_bulb(display, cx, TOP_CY, fg)?,
            2 => icon_bat(display, cx, TOP_CY, fg)?,
            _ => icon_syringe(display, cx, TOP_CY, !syringe_active, fg)?,
        }
    }

    // Separator
    Rectangle::new(Point::new(0, SEP_TOP), Size::new(152, 1))
        .into_styled(PrimitiveStyle::with_fill(BLACK))
        .draw(display)?;

    // ── Pet area ──────────────────────────────────────────────────────────────
    let pet_center = Point::new(76, (SEP_TOP + SEP_BOT) / 2);
    draw_egg(display, pet_center)?;

    // Separator
    Rectangle::new(Point::new(0, SEP_BOT), Size::new(152, 1))
        .into_styled(PrimitiveStyle::with_fill(BLACK))
        .draw(display)?;

    // ── Bottom icon row ───────────────────────────────────────────────────────
    for (i, &cx) in ICON_CX.iter().enumerate() {
        let selected = nav.row == Row::Bottom && nav.col == i as u8;
        let fg = if selected { WHITE } else { BLACK };
        if selected {
            draw_selection_bg(display, cx, BOT_CY)?;
        }
        match i {
            0 => icon_duck(display, cx, BOT_CY, fg)?,
            1 => icon_meter(display, cx, BOT_CY, fg)?,
            2 => icon_face(display, cx, BOT_CY, fg)?,
            _ => icon_twofaces(display, cx, BOT_CY, !attention, fg)?,
        }
    }

    // Modal overlay — drawn last so it sits on top of everything.
    modal::draw_modal(display)?;

    Ok(())
}
