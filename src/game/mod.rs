//! BornPets — game screen renderer.
//!
//! Layout (152×152 EPD, black & white + red):
//!
//! ```text
//! ┌───────────────────────────────────────┐  y =  0
//! │  [Stats] [Hibernate]  (empty)(empty)  │  y = 0–34   top icon row
//! ├───────────────────────────────────────┤  y = 34
//! │                                       │
//! │            [pet / egg]                │  y = 35–110  pet area
//! │                                       │
//! ├───────────────────────────────────────┤  y = 111
//! │  [Feed]  [Heal]  [Play]  [Rest]       │  y = 111–152 bottom icon row
//! └───────────────────────────────────────┘  y = 152
//! ```

pub mod engine;
pub mod input;
pub mod lifecycle;
pub mod modal;
pub mod nav;
pub mod sprite_loader;
pub mod lightsout;
pub mod pet_select;
pub mod realm_view;
pub mod stat_bar;
pub mod station;
pub mod tictactoe;
pub mod traits_view;
pub use nav::{GameNav, Row};

use embedded_graphics::{
    prelude::*,
    primitives::{Circle, PrimitiveStyle, Rectangle},
};

use crate::{BLACK, TriColor, WHITE};

// ── Action feedback toast ────────────────────────────────────────────────────

use core::sync::atomic::{AtomicU8, Ordering};

/// Action feedback shown briefly after an action.
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Toast {
    None          = 0,
    Feed          = 1,
    Heal          = 2,
    Sleep         = 3,
    Relax         = 4,
    Play          = 5,
    Inspired      = 6,
    Hibernate     = 7,
    Wake          = 8,
    StationFood   = 9,
    StationDrugs  = 10,
    StationInspire = 11,
    StationRest   = 12,
}

impl Toast {
    fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::Feed, 2 => Self::Heal, 3 => Self::Sleep,
            4 => Self::Relax, 5 => Self::Play, 6 => Self::Inspired,
            7 => Self::Hibernate, 8 => Self::Wake,
            9 => Self::StationFood, 10 => Self::StationDrugs,
            11 => Self::StationInspire, 12 => Self::StationRest,
            _ => Self::None,
        }
    }

    fn message(self) -> &'static str {
        match self {
            Toast::None           => "",
            Toast::Feed           => "-hunger",
            Toast::Heal           => "-sick",
            Toast::Sleep          => "-tired",
            Toast::Relax          => "-drained",
            Toast::Play           => "-miserable",
            Toast::Inspired       => "+inspired",
            Toast::Hibernate      => "hibernating",
            Toast::Wake           => "waking up",
            Toast::StationFood    => "station: fed!",
            Toast::StationDrugs   => "station: healed!",
            Toast::StationInspire => "station: inspired!",
            Toast::StationRest    => "station: rested!",
        }
    }
}

/// Toast message index.
static TOAST_MSG: AtomicU8 = AtomicU8::new(0);
/// Remaining draw cycles before the toast disappears.
static TOAST_TTL: AtomicU8 = AtomicU8::new(0);

/// Number of display refreshes to show the toast (~2–3 sec per refresh on e-ink).
const TOAST_DRAWS: u8 = 2;

/// Show a feedback toast for the next few display refreshes.
pub fn show_toast(toast: Toast) {
    TOAST_MSG.store(toast as u8, Ordering::Relaxed);
    TOAST_TTL.store(TOAST_DRAWS, Ordering::Relaxed);
}

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

/// First display row of the pet/sprite area.
pub const PET_AREA_TOP: usize = SEP_TOP as usize + 1;

// ── Selection highlight ───────────────────────────────────────────────────────

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

// ── Icon drawing functions ────────────────────────────────────────────────────

fn icon_fork<D>(display: &mut D, cx: i32, cy: i32, color: TriColor) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let fill = PrimitiveStyle::with_fill(color);
    Rectangle::new(Point::new(cx - 1, cy - 8), Size::new(2, 16))
        .into_styled(fill)
        .draw(display)?;
    Rectangle::new(Point::new(cx - 4, cy - 8), Size::new(2, 8))
        .into_styled(fill)
        .draw(display)?;
    Rectangle::new(Point::new(cx + 2, cy - 8), Size::new(2, 8))
        .into_styled(fill)
        .draw(display)?;
    Ok(())
}

fn icon_bulb<D>(display: &mut D, cx: i32, cy: i32, color: TriColor) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let stroke = PrimitiveStyle::with_stroke(color, 1);
    let fill = PrimitiveStyle::with_fill(color);
    Circle::new(Point::new(cx - 5, cy - 7), 10)
        .into_styled(stroke)
        .draw(display)?;
    Rectangle::new(Point::new(cx - 2, cy + 3), Size::new(4, 4))
        .into_styled(fill)
        .draw(display)?;
    Ok(())
}

fn icon_bat<D>(display: &mut D, cx: i32, cy: i32, color: TriColor) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let fill = PrimitiveStyle::with_fill(color);
    Rectangle::new(Point::new(cx - 1, cy - 8), Size::new(2, 16))
        .into_styled(fill)
        .draw(display)?;
    Rectangle::new(Point::new(cx - 6, cy + 2), Size::new(12, 3))
        .into_styled(fill)
        .draw(display)?;
    Circle::new(Point::new(cx - 3, cy - 5), 6)
        .into_styled(PrimitiveStyle::with_stroke(color, 1))
        .draw(display)?;
    Ok(())
}

fn icon_syringe<D>(
    display: &mut D,
    cx: i32,
    cy: i32,
    _active: bool,
    color: TriColor,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let fill = PrimitiveStyle::with_fill(color);
    let stroke = PrimitiveStyle::with_stroke(color, 1);
    Rectangle::new(Point::new(cx - 2, cy - 8), Size::new(4, 12))
        .into_styled(stroke)
        .draw(display)?;
    Rectangle::new(Point::new(cx - 1, cy - 10), Size::new(2, 3))
        .into_styled(fill)
        .draw(display)?;
    Rectangle::new(Point::new(cx - 1, cy + 4), Size::new(2, 4))
        .into_styled(fill)
        .draw(display)?;
    Ok(())
}

fn icon_meter<D>(display: &mut D, cx: i32, cy: i32, color: TriColor) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let stroke = PrimitiveStyle::with_stroke(color, 1);
    let fill = PrimitiveStyle::with_fill(color);
    Circle::new(Point::new(cx - 7, cy - 7), 14)
        .into_styled(stroke)
        .draw(display)?;
    Rectangle::new(Point::new(cx - 1, cy - 5), Size::new(2, 6))
        .into_styled(fill)
        .draw(display)?;
    Rectangle::new(Point::new(cx - 1, cy - 5), Size::new(5, 2))
        .into_styled(fill)
        .draw(display)?;
    Ok(())
}

fn icon_duck<D>(display: &mut D, cx: i32, cy: i32, color: TriColor) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let stroke = PrimitiveStyle::with_stroke(color, 1);
    let fill = PrimitiveStyle::with_fill(color);
    Circle::new(Point::new(cx - 3, cy - 7), 8)
        .into_styled(stroke)
        .draw(display)?;
    Circle::new(Point::new(cx - 6, cy), 12)
        .into_styled(stroke)
        .draw(display)?;
    Rectangle::new(Point::new(cx + 2, cy - 4), Size::new(4, 2))
        .into_styled(fill)
        .draw(display)?;
    Ok(())
}

// ── Public entry point ────────────────────────────────────────────────────────

/// Render the BornPets game screen.
///
/// Handles four states:
/// - **Not started**: "Press Fire to start" — no icons.
/// - **Hatching**: egg animation + countdown — no icons.
/// - **Gone**: farewell + "Press Execute for new egg" — no icons.
/// - **Active**: icons + pet animation + modal overlay.
pub fn draw_screen_game<D>(display: &mut D, nav: GameNav) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    use embedded_graphics::mono_font::{MonoTextStyle, ascii::FONT_7X13_BOLD};
    use embedded_graphics::text::{Alignment, Baseline, Text, TextStyleBuilder};
    use engine::to_display::DisplayAnim;

    let centered = TextStyleBuilder::new()
        .baseline(Baseline::Middle)
        .alignment(Alignment::Center)
        .build();
    let font = MonoTextStyle::new(&FONT_7X13_BOLD, BLACK);

    // ── Full-screen takeover screens ───────────────────────────────────
    if pet_select::is_active() {
        return pet_select::draw(display);
    }
    if tictactoe::is_active() {
        return tictactoe::draw(display);
    }
    if lightsout::is_active() {
        return lightsout::draw(display);
    }

    // Battery icon — top-right.
    #[cfg(feature = "embassy-base")]
    {
        let pct = crate::fw::battery::read_pct();
        crate::draw_battery_icon(display, 128, 2, pct)?;
    }

    // ── Not started ──────────────────────────────────────────────────────
    if !lifecycle::is_started() {
        // Start screen graphic is blitted by render(); only battery shown here.
        return Ok(());
    }

    let anim = lifecycle::display_anim();

    // ── Hatching ─────────────────────────────────────────────────────────
    if let DisplayAnim::Hatching { ticks_remaining } = anim {
        // Egg animation is blitted by embassy.rs.  The countdown timer
        // below acts as the sole hatching indicator.
        let secs = ticks_remaining as u32 * 10;
        let mut time_str: heapless::String<16> = heapless::String::new();
        let _ = core::fmt::Write::write_fmt(
            &mut time_str,
            format_args!("{}:{:02}", secs / 60, secs % 60),
        );
        Text::with_text_style(time_str.as_str(), Point::new(76, 100), font, centered)
            .draw(display)?;
        return Ok(());
    }

    // ── Gone ─────────────────────────────────────────────────────────────
    if anim == DisplayAnim::Gone {
        // Farewell animation blitted by embassy.rs if available.
        if sprite_loader::frame_count() == 0 {
            Text::with_text_style("Your pet has left", Point::new(76, 50), font, centered)
                .draw(display)?;
        }
        Text::with_text_style("Press Fire", Point::new(76, 90), font, centered).draw(display)?;
        Text::with_text_style("for a new egg", Point::new(76, 106), font, centered)
            .draw(display)?;
        return Ok(());
    }

    // ── Active game ──────────────────────────────────────────────────────

    // Top icon row: Stats, Hibernate.
    for (i, &cx) in ICON_CX.iter().enumerate() {
        let selected = nav.row == Row::Top && nav.col == i as u8;
        let fg = if selected { WHITE } else { BLACK };
        if selected {
            draw_selection_bg(display, cx, TOP_CY)?;
        }
        match i {
            0 => icon_meter(display, cx, TOP_CY, fg)?,
            1 => icon_bulb(display, cx, TOP_CY, fg)?,
            _ => {}
        }
    }

    Rectangle::new(Point::new(0, SEP_TOP), Size::new(152, 1))
        .into_styled(PrimitiveStyle::with_fill(BLACK))
        .draw(display)?;

    // Pet area: sprite blitted by embassy.rs, or fallback.
    if sprite_loader::frame_count() == 0 {
        Text::with_text_style(
            "No sprites on flash",
            Point::new(76, (SEP_TOP + SEP_BOT) / 2),
            font,
            centered,
        )
        .draw(display)?;
    }

    Rectangle::new(Point::new(0, SEP_BOT), Size::new(152, 1))
        .into_styled(PrimitiveStyle::with_fill(BLACK))
        .draw(display)?;

    // Bottom icon row: Feed, Heal, Play, Rest.
    for (i, &cx) in ICON_CX.iter().enumerate() {
        let selected = nav.row == Row::Bottom && nav.col == i as u8;
        let fg = if selected { WHITE } else { BLACK };
        if selected {
            draw_selection_bg(display, cx, BOT_CY)?;
        }
        match i {
            0 => icon_fork(display, cx, BOT_CY, fg)?,
            1 => icon_syringe(display, cx, BOT_CY, true, fg)?,
            2 => icon_bat(display, cx, BOT_CY, fg)?,
            _ => icon_duck(display, cx, BOT_CY, fg)?,
        }
    }

    // Action feedback toast — shown briefly after an action.
    let ttl = TOAST_TTL.load(Ordering::Relaxed);
    if ttl > 0 {
        let toast = Toast::from_u8(TOAST_MSG.load(Ordering::Relaxed));
        let msg = toast.message();
        if !msg.is_empty() {
            use embedded_graphics::text::{Text, TextStyleBuilder, Baseline};
            use embedded_graphics::mono_font::{MonoTextStyle, ascii::FONT_7X13_BOLD};
            let style = TextStyleBuilder::new()
                .baseline(Baseline::Top)
                .build();
            Text::with_text_style(
                msg,
                Point::new(2, SEP_TOP + 2),
                MonoTextStyle::new(&FONT_7X13_BOLD, BLACK),
                style,
            )
            .draw(display)?;
        }
        TOAST_TTL.store(ttl - 1, Ordering::Relaxed);
    }

    modal::draw_modal(display)?;

    Ok(())
}

// ── Async render — called from the display loop ──────────────────────────────

/// Full game render cycle: engine update, sprite blit, debug overlay, save.
///
/// Handles the start screen (full 152×152 blit of `00000000.PCX`),
/// in-game animation blitting, and the debug animation name overlay
/// when no artwork is loaded.
#[cfg(feature = "embassy-base")]
pub async fn render(display: &mut crate::fw::epd::EpdGfx<'_>, sprite_frame: u8) {
    use crate::fw::fat12;
    use engine::anim_files;
    use engine::to_display::DisplayAnim;

    if lifecycle::is_started() {
        lifecycle::cycle();

        // After hatching completes, prompt the player to name their pet.
        if lifecycle::take_naming_pending() {
            let seed = lifecycle::now_tick();
            let default = lifecycle::random_default_name(seed);
            crate::text_entry::begin(default.as_bytes(), 12, on_pet_named, "Name your Pet");
        }
    }

    // Blit sprite from flash.
    let mut has_sprite = false;

    if !lifecycle::is_started() {
        // Start screen: full 152×152 graphic at origin.
        let start_name = anim_files::start_screen_filename();
        if let Ok(file) = fat12::find_file(&start_name).await {
            sprite_loader::blit_file(display, &file, 0, 0).await;
            has_sprite = true;
        }
    } else {
        // In-game animation in the pet area.
        let kind = lifecycle::pet_kind();
        let anim = lifecycle::display_anim();
        let frame_count = anim_files::frame_count(kind, anim);
        if frame_count > 0 {
            let name = anim_files::anim_filename(kind, anim, sprite_frame);
            if let Ok(file) = fat12::find_file(&name).await {
                sprite_loader::blit_file(display, &file, 0, PET_AREA_TOP as i32).await;
                has_sprite = true;
            }
        }
    }

    // Debug: show animation name when no artwork loaded.
    if !has_sprite && lifecycle::is_started() {
        use embedded_graphics::mono_font::{MonoTextStyle, ascii::FONT_7X13};
        use embedded_graphics::text::{Alignment, Baseline, Text, TextStyleBuilder};
        use ssd1675::graphics::Color;

        let anim = lifecycle::display_anim();
        let anim_name: &str = match anim {
            DisplayAnim::Gone => "GONE",
            DisplayAnim::Hibernating => "HIBERNATE",
            DisplayAnim::Hatching { .. } => "HATCHING",
            DisplayAnim::Feeding => "FEEDING",
            DisplayAnim::Healing => "HEALING",
            DisplayAnim::Relaxing => "RELAXING",
            DisplayAnim::Playing => "PLAYING",
            DisplayAnim::Sleeping => "SLEEPING",
            DisplayAnim::Leaving { .. } => "LEAVING",
            DisplayAnim::CriticalSick => "CRIT:SICK",
            DisplayAnim::CriticalTired => "CRIT:TIRED",
            DisplayAnim::CriticalHungry => "CRIT:HUNGRY",
            DisplayAnim::CriticalDrained => "CRIT:DRAINED",
            DisplayAnim::WarningSick => "WARN:SICK",
            DisplayAnim::WarningTired => "WARN:TIRED",
            DisplayAnim::WarningHungry => "WARN:HUNGRY",
            DisplayAnim::WarningDrained => "WARN:DRAINED",
            DisplayAnim::WarningMiserable => "WARN:MISER",
            DisplayAnim::Happy => "HAPPY",
            DisplayAnim::Idle => "IDLE",
        };
        let style = TextStyleBuilder::new()
            .baseline(Baseline::Top)
            .alignment(Alignment::Right)
            .build();
        let _ = Text::with_text_style(
            anim_name,
            Point::new(150, 36),
            MonoTextStyle::new(&FONT_7X13, Color::Black),
            style,
        )
        .draw(display);
    }

    if lifecycle::is_started() {
        lifecycle::save_if_needed().await;
    }
}

/// Text entry callback: player has submitted a pet name.
#[cfg(feature = "embassy-base")]
fn on_pet_named(name: &[u8]) {
    lifecycle::set_pet_name(name);
}
