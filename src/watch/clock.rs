//! Clock face — switchable Casio-style digital and analog faces, plus the
//! shared 7-segment digit primitives and the date / day-of-week strip.
//!
//! The 7-segment helpers ([`draw_digit`], [`draw_colon`], and the time-row
//! geometry constants) are also used by [`super::alarm::draw_edit`] for the
//! alarm-edit screen, so they're `pub(super)`.

use core::sync::atomic::{AtomicU8, Ordering};

use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::mono_font::ascii::{FONT_6X10, FONT_8X13_BOLD, FONT_9X18_BOLD};
use embedded_graphics::mono_font::iso_8859_1::FONT_7X13_BOLD;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{
    Circle, Line, PrimitiveStyle, PrimitiveStyleBuilder, Rectangle, Triangle,
};
use embedded_graphics::text::{Alignment, Baseline, Text, TextStyleBuilder};

use crate::menu::ButtonId;
use crate::{BLACK, RED, TriColor, WHITE};

// ── Face selection ───────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum WatchFace {
    Digital = 0,
    Analog = 1,
}

static WATCH_FACE: AtomicU8 = AtomicU8::new(WatchFace::Digital as u8);

fn current_face() -> WatchFace {
    match WATCH_FACE.load(Ordering::Relaxed) {
        0 => WatchFace::Digital,
        _ => WatchFace::Analog,
    }
}

fn toggle_face() {
    let next = match current_face() {
        WatchFace::Digital => WatchFace::Analog,
        WatchFace::Analog => WatchFace::Digital,
    };
    WATCH_FACE.store(next as u8, Ordering::Relaxed);
    super::signal_settings_dirty();
}

/// Handle a button press while in normal (non-edit) watch mode.
/// Returns `true` if the button was consumed.
pub(super) fn dispatch_normal(btn: ButtonId) -> bool {
    match btn {
        ButtonId::Up | ButtonId::Down => {
            toggle_face();
            true
        }
        ButtonId::Fire | ButtonId::Execute => {
            super::alarm::enter_edit();
            true
        }
        _ => false,
    }
}

// ── 7-segment digit geometry ─────────────────────────────────────────────────
//
// Each segment is a hex (lozenge) so adjacent segments meet at 45° miters,
// like a real LCD. Lengths are chosen so:
//   DIGIT_H = 3 * STROKE + 2 * VERT_LEN
// which keeps the middle segment exactly centred and the upper/lower halves
// symmetric.
pub(super) const DIGIT_W: i32 = 30;
const STROKE: i32 = 5; // segment thickness; must be odd so the tip apex sits on a single pixel
const HALF: i32 = STROKE / 2; // 2
const VERT_LEN: i32 = 25; // length of one vertical segment (top or bottom half)
pub(super) const DIGIT_H: i32 = 3 * STROKE + 2 * VERT_LEN; // 65
// Horizontal segments are inset from the digit edges, like a real Casio:
// the vertical segs are flush with the side, the horizontals sit between them.
const HORIZ_LEN: i32 = 24;
const HORIZ_INSET: i32 = (DIGIT_W - HORIZ_LEN) / 2;

// ── Time-row layout ──────────────────────────────────────────────────────────
pub(super) const DIGIT_Y: i32 = 30;
const DIGIT_PITCH: i32 = DIGIT_W + 4; // gap between digits within a pair
pub(super) const PAIR_W: i32 = DIGIT_PITCH + DIGIT_W; // 64 — width of "HH" or "MM"
const COLON_W: i32 = 6;
const COLON_GAP: i32 = 4;
const TIME_W: i32 = 2 * PAIR_W + 2 * COLON_GAP + COLON_W; // 142
const TIME_X: i32 = (152 - TIME_W) / 2; // 5
pub(super) const HH_TENS_X: i32 = TIME_X;
pub(super) const HH_ONES_X: i32 = TIME_X + DIGIT_PITCH;
const COLON_X: i32 = TIME_X + PAIR_W + COLON_GAP;
pub(super) const MM_TENS_X: i32 = COLON_X + COLON_W + COLON_GAP;
pub(super) const MM_ONES_X: i32 = MM_TENS_X + DIGIT_PITCH;

// Colon dots aligned with the inner blank rows between top/middle and
// middle/bottom.
const COLON_TOP_Y: i32 = DIGIT_Y + STROKE + VERT_LEN / 2 - COLON_W / 2;
const COLON_BOT_Y: i32 = DIGIT_Y + 2 * STROKE + VERT_LEN + VERT_LEN / 2 - COLON_W / 2;

// ── Analog face geometry ─────────────────────────────────────────────────────
//
// Day-Date layout, à la Rolex: the dial fills the entire body (no separate
// weekday strip — that's digital-only), with two red complications inside
// the dial — `TUE` at 12 o'clock and `27 APR` at 6 o'clock.
const ANALOG_CX: i32 = 76;
const ANALOG_CY: i32 = 84;
const ANALOG_R: i32 = 64;
const ANALOG_TICK_HOUR: i32 = 5;
const ANALOG_TICK_CARDINAL: i32 = 9;
const HOUR_HAND_LEN: i32 = 36;
const MINUTE_HAND_LEN: i32 = 55;
const HOUR_HAND_W: u32 = 4;
const MINUTE_HAND_W: u32 = 2;
const CENTER_DOT_R: u32 = 9; // diameter
/// Date complication (12 o'clock) — pulled toward centre so it sits well
/// clear of the 12 tick.
const DATE_COMPL_Y: i32 = ANALOG_CY - 40;
/// Day complication (6 o'clock) — pulled toward centre so it sits well
/// clear of the 6 tick.
const DAY_COMPL_Y: i32 = ANALOG_CY + 40;

// ── Date label ───────────────────────────────────────────────────────────────
const DATE_X: i32 = 76;
// Centred between the bottom of the digit pair (y=95) and the top of the
// day strip (y=136) so the date sits in the visual middle of the band.
const DATE_Y: i32 = 115;

// ── Day-of-week strip (bottom of screen) ─────────────────────────────────────
const DAY_NAMES: [&str; 7] = ["MON", "TUE", "WED", "THU", "FRI", "SAT", "SUN"];
const DAY_W: i32 = 20;
const DAY_H: i32 = 14;
const DAY_GAP: i32 = 1;
const DAY_Y: i32 = 152 - DAY_H - 2; // bottom-anchored with 2 px margin
const DAY_X_START: i32 = (152 - (7 * DAY_W + 6 * DAY_GAP)) / 2;

// 7-segment encoding using A,B,C,D,E,F,G order.
const SEGMENTS: [[bool; 7]; 10] = [
    [true, true, true, true, true, true, false],     // 0
    [false, true, true, false, false, false, false], // 1
    [true, true, false, true, true, false, true],    // 2
    [true, true, true, true, false, false, true],    // 3
    [false, true, true, false, false, true, true],   // 4
    [true, false, true, true, false, true, true],    // 5
    [true, false, true, true, true, true, true],     // 6
    [true, true, true, false, false, false, false],  // 7
    [true, true, true, true, true, true, true],      // 8
    [true, true, true, true, false, true, true],     // 9
];

// ── Sine table for analog hands (Q.14 fixed point, 0°..90°) ──────────────────
const SIN_Q14: [i16; 91] = [
    0, 286, 572, 857, 1143, 1428, 1713, 1997, 2280, 2563, 2845, 3126, 3406, 3686, 3964, 4240, 4516,
    4790, 5063, 5334, 5604, 5872, 6138, 6402, 6664, 6924, 7182, 7438, 7692, 7943, 8192, 8438, 8682,
    8923, 9162, 9397, 9630, 9860, 10087, 10311, 10531, 10749, 10963, 11174, 11381, 11585, 11786,
    11982, 12176, 12365, 12551, 12733, 12911, 13085, 13255, 13421, 13583, 13741, 13894, 14044,
    14189, 14330, 14466, 14598, 14726, 14849, 14968, 15082, 15191, 15296, 15396, 15491, 15582,
    15668, 15749, 15826, 15897, 15964, 16026, 16083, 16135, 16182, 16225, 16262, 16294, 16322,
    16344, 16362, 16374, 16382, 16384,
];

fn sin_deg(deg: i32) -> i32 {
    let d = deg.rem_euclid(360);
    let v = match d {
        0..=90 => SIN_Q14[d as usize] as i32,
        91..=180 => SIN_Q14[(180 - d) as usize] as i32,
        181..=270 => -(SIN_Q14[(d - 180) as usize] as i32),
        _ => -(SIN_Q14[(360 - d) as usize] as i32),
    };
    v
}

fn cos_deg(deg: i32) -> i32 {
    sin_deg(90 - deg)
}

// ── Clock source ─────────────────────────────────────────────────────────────

#[derive(Clone, Copy)]
pub(super) struct Clock {
    pub hour: u8,
    pub minute: u8,
    pub day: u8,
    pub month: u8,
    pub year: u16,
    pub weekday: u8, // 0 = MON
}

fn build_clock(unix_secs: u32, tz_offset_hours: i8) -> Option<Clock> {
    use fasttime::Date;

    let offset_secs = tz_offset_hours as i64 * 3600;
    let local = (unix_secs as i64).saturating_add(offset_secs).max(0) as u32;

    let minute = ((local % 3600) / 60) as u8;
    let hour = ((local % 86400) / 3600) as u8;
    let days = (local / 86400) as i64;
    let date = Date::from_days_since_unix_epoch(days).ok()?;
    let weekday = date.weekday().number_from_monday().saturating_sub(1);

    Some(Clock {
        hour,
        minute,
        day: date.day,
        month: date.month,
        year: date.year as u16,
        weekday,
    })
}

#[cfg(feature = "embassy-base")]
pub(super) fn wall_clock() -> Option<Clock> {
    let unix = crate::unix_now()?;
    let tz = crate::TIMEZONE_OFFSET.load(Ordering::Relaxed);
    build_clock(unix, tz)
}

#[cfg(all(feature = "simulator", not(feature = "embassy-base")))]
pub(super) fn wall_clock() -> Option<Clock> {
    use std::time::{SystemTime, UNIX_EPOCH};
    let unix = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs() as u32;
    let tz = crate::TIMEZONE_OFFSET.load(Ordering::Relaxed);
    build_clock(unix, tz)
}

#[cfg(not(any(feature = "embassy-base", feature = "simulator")))]
pub(super) fn wall_clock() -> Option<Clock> {
    None
}

#[cfg(feature = "embassy-base")]
pub(super) fn battery_pct() -> u8 {
    crate::fw::battery::read_pct()
}

#[cfg(not(feature = "embassy-base"))]
pub(super) fn battery_pct() -> u8 {
    100
}

// ── Hex (lozenge) segment primitives ─────────────────────────────────────────

/// Filled horizontal lozenge of width `HORIZ_LEN` and thickness `STROKE`,
/// with apexes at the left and right midline.
fn draw_seg_horiz<D>(display: &mut D, x: i32, y: i32) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let fill = PrimitiveStyle::with_fill(BLACK);
    let x = x + HORIZ_INSET;
    Rectangle::new(
        Point::new(x + HALF, y),
        Size::new((HORIZ_LEN - 2 * HALF) as u32, STROKE as u32),
    )
    .into_styled(fill)
    .draw(display)?;
    Triangle::new(
        Point::new(x, y + HALF),
        Point::new(x + HALF, y),
        Point::new(x + HALF, y + STROKE - 1),
    )
    .into_styled(fill)
    .draw(display)?;
    Triangle::new(
        Point::new(x + HORIZ_LEN - 1, y + HALF),
        Point::new(x + HORIZ_LEN - HALF - 1, y),
        Point::new(x + HORIZ_LEN - HALF - 1, y + STROKE - 1),
    )
    .into_styled(fill)
    .draw(display)?;
    Ok(())
}

/// Filled vertical lozenge of length `VERT_LEN` and thickness `STROKE`,
/// with apexes at the top and bottom midline.
fn draw_seg_vert<D>(display: &mut D, x: i32, y: i32) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let fill = PrimitiveStyle::with_fill(BLACK);
    Rectangle::new(
        Point::new(x, y + HALF),
        Size::new(STROKE as u32, (VERT_LEN - 2 * HALF) as u32),
    )
    .into_styled(fill)
    .draw(display)?;
    Triangle::new(
        Point::new(x + HALF, y),
        Point::new(x, y + HALF),
        Point::new(x + STROKE - 1, y + HALF),
    )
    .into_styled(fill)
    .draw(display)?;
    Triangle::new(
        Point::new(x + HALF, y + VERT_LEN - 1),
        Point::new(x, y + VERT_LEN - HALF - 1),
        Point::new(x + STROKE - 1, y + VERT_LEN - HALF - 1),
    )
    .into_styled(fill)
    .draw(display)?;
    Ok(())
}

pub(super) fn draw_digit<D>(display: &mut D, x: i32, y: i32, digit: u8) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let segs = SEGMENTS.get(digit as usize).copied().unwrap_or([false; 7]);

    if segs[0] {
        draw_seg_horiz(display, x, y)?;
    }
    if segs[1] {
        draw_seg_vert(display, x + DIGIT_W - STROKE, y + STROKE)?;
    }
    if segs[2] {
        draw_seg_vert(display, x + DIGIT_W - STROKE, y + 2 * STROKE + VERT_LEN)?;
    }
    if segs[3] {
        draw_seg_horiz(display, x, y + DIGIT_H - STROKE)?;
    }
    if segs[4] {
        draw_seg_vert(display, x, y + 2 * STROKE + VERT_LEN)?;
    }
    if segs[5] {
        draw_seg_vert(display, x, y + STROKE)?;
    }
    if segs[6] {
        draw_seg_horiz(display, x, y + STROKE + VERT_LEN)?;
    }
    Ok(())
}

/// Two stacked square dots between the HH and MM digits.  Shared with the
/// alarm-edit screen.
pub(super) fn draw_colon<D>(display: &mut D) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let dot = PrimitiveStyle::with_fill(BLACK);
    Rectangle::new(
        Point::new(COLON_X, COLON_TOP_Y),
        Size::new(COLON_W as u32, COLON_W as u32),
    )
    .into_styled(dot)
    .draw(display)?;
    Rectangle::new(
        Point::new(COLON_X, COLON_BOT_Y),
        Size::new(COLON_W as u32, COLON_W as u32),
    )
    .into_styled(dot)
    .draw(display)?;
    Ok(())
}

// ── Face renderers ───────────────────────────────────────────────────────────

fn draw_digital<D>(display: &mut D, clock: &Clock) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    draw_digit(display, HH_TENS_X, DIGIT_Y, clock.hour / 10)?;
    draw_digit(display, HH_ONES_X, DIGIT_Y, clock.hour % 10)?;
    draw_colon(display)?;
    draw_digit(display, MM_TENS_X, DIGIT_Y, clock.minute / 10)?;
    draw_digit(display, MM_ONES_X, DIGIT_Y, clock.minute % 10)?;
    Ok(())
}

/// Compute the endpoint of a hand of `length` rooted at `(cx, cy)` pointing at
/// `angle_deg`, where 0° is 12 o'clock and angles increase clockwise.
fn polar(cx: i32, cy: i32, length: i32, angle_deg: i32) -> Point {
    let dx = (length * sin_deg(angle_deg)) >> 14;
    let dy = -((length * cos_deg(angle_deg)) >> 14);
    Point::new(cx + dx, cy + dy)
}

fn draw_analog<D>(display: &mut D, clock: &Clock) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    // Outer face circle.
    Circle::with_center(Point::new(ANALOG_CX, ANALOG_CY), (ANALOG_R as u32) * 2)
        .into_styled(PrimitiveStyle::with_stroke(BLACK, 2))
        .draw(display)?;

    // 12 hour ticks.
    let tick_style = PrimitiveStyle::with_stroke(BLACK, 2);
    for i in 0..12i32 {
        let angle = i * 30;
        let outer = polar(ANALOG_CX, ANALOG_CY, ANALOG_R - 1, angle);
        let tick_len = if i % 3 == 0 {
            ANALOG_TICK_CARDINAL
        } else {
            ANALOG_TICK_HOUR
        };
        let inner = polar(ANALOG_CX, ANALOG_CY, ANALOG_R - 1 - tick_len, angle);
        Line::new(inner, outer)
            .into_styled(tick_style)
            .draw(display)?;
    }

    // Hands. Hour hand carries minute fraction so it advances smoothly.
    let hour_angle = (clock.hour as i32 % 12) * 30 + (clock.minute as i32) / 2;
    let minute_angle = (clock.minute as i32) * 6;

    let hour_style = PrimitiveStyleBuilder::new()
        .stroke_color(BLACK)
        .stroke_width(HOUR_HAND_W)
        .build();
    let minute_style = PrimitiveStyleBuilder::new()
        .stroke_color(BLACK)
        .stroke_width(MINUTE_HAND_W)
        .build();

    Line::new(
        Point::new(ANALOG_CX, ANALOG_CY),
        polar(ANALOG_CX, ANALOG_CY, HOUR_HAND_LEN, hour_angle),
    )
    .into_styled(hour_style)
    .draw(display)?;

    Line::new(
        Point::new(ANALOG_CX, ANALOG_CY),
        polar(ANALOG_CX, ANALOG_CY, MINUTE_HAND_LEN, minute_angle),
    )
    .into_styled(minute_style)
    .draw(display)?;

    // Day-Date complications — `DD MON` at 12 o'clock, `TUE` at 6 o'clock.
    // Drawn before the hands so the minute hand (which crosses each one
    // about once an hour) reads on top in black; the complications are
    // red, so the two planes coexist legibly.
    draw_analog_date_complication(display, clock)?;
    draw_analog_day_complication(display, clock)?;

    // Centre dot covers the hand pivot.
    Circle::with_center(Point::new(ANALOG_CX, ANALOG_CY), CENTER_DOT_R)
        .into_styled(PrimitiveStyle::with_fill(BLACK))
        .draw(display)?;

    Ok(())
}

const MONTH_ABBR: [&str; 12] = [
    "JAN", "FEB", "MAR", "APR", "MAY", "JUN", "JUL", "AUG", "SEP", "OCT", "NOV", "DEC",
];

fn draw_analog_day_complication<D>(display: &mut D, clock: &Clock) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let centered = TextStyleBuilder::new()
        .baseline(Baseline::Middle)
        .alignment(Alignment::Center)
        .build();

    let weekday_idx = (clock.weekday as usize).min(6);
    Text::with_text_style(
        DAY_NAMES[weekday_idx],
        Point::new(ANALOG_CX, DAY_COMPL_Y),
        MonoTextStyle::new(&FONT_8X13_BOLD, RED),
        centered,
    )
    .draw(display)?;
    Ok(())
}

fn draw_analog_date_complication<D>(display: &mut D, clock: &Clock) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let centered = TextStyleBuilder::new()
        .baseline(Baseline::Middle)
        .alignment(Alignment::Center)
        .build();

    let month_idx = (clock.month as usize).saturating_sub(1).min(11);
    let mon = MONTH_ABBR[month_idx];

    let mut buf: heapless::String<8> = heapless::String::new();
    let _ = core::fmt::write(&mut buf, format_args!("{:02} {}", clock.day, mon));

    Text::with_text_style(
        &buf,
        Point::new(ANALOG_CX, DATE_COMPL_Y),
        MonoTextStyle::new(&FONT_8X13_BOLD, RED),
        centered,
    )
    .draw(display)?;
    Ok(())
}

fn draw_date<D>(display: &mut D, clock: &Clock) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let centered = TextStyleBuilder::new()
        .baseline(Baseline::Middle)
        .alignment(Alignment::Center)
        .build();

    let mut date_buf: heapless::String<12> = heapless::String::new();
    let _ = core::fmt::write(
        &mut date_buf,
        format_args!("{:04}-{:02}-{:02}", clock.year, clock.month, clock.day),
    );
    Text::with_text_style(
        &date_buf,
        Point::new(DATE_X, DATE_Y),
        MonoTextStyle::new(&FONT_9X18_BOLD, BLACK),
        centered,
    )
    .draw(display)?;
    Ok(())
}

fn draw_day_strip<D>(display: &mut D, weekday: u8) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let centered = TextStyleBuilder::new()
        .baseline(Baseline::Middle)
        .alignment(Alignment::Center)
        .build();

    for (i, name) in DAY_NAMES.iter().enumerate() {
        let x = DAY_X_START + i as i32 * (DAY_W + DAY_GAP);
        let is_current = i == weekday as usize;
        let rect = Rectangle::new(Point::new(x, DAY_Y), Size::new(DAY_W as u32, DAY_H as u32));
        let fg = if is_current {
            rect.into_styled(PrimitiveStyle::with_fill(RED))
                .draw(display)?;
            WHITE
        } else {
            rect.into_styled(PrimitiveStyle::with_stroke(RED, 1))
                .draw(display)?;
            BLACK
        };
        Text::with_text_style(
            name,
            Point::new(x + DAY_W / 2, DAY_Y + DAY_H / 2),
            MonoTextStyle::new(&FONT_6X10, fg),
            centered,
        )
        .draw(display)?;
    }
    Ok(())
}

/// Draw the active face (digital or analog), the date, and the weekday strip.
/// If no wall-clock time is available yet, draws a placeholder message instead.
pub(super) fn draw_face<D>(display: &mut D) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let Some(clock) = wall_clock() else {
        let centered = TextStyleBuilder::new()
            .baseline(Baseline::Middle)
            .alignment(Alignment::Center)
            .build();
        Text::with_text_style(
            "Clock not set",
            Point::new(76, 80),
            MonoTextStyle::new(&FONT_7X13_BOLD, BLACK),
            centered,
        )
        .draw(display)?;
        return Ok(());
    };

    // Analog has its own Day-Date complications inside the dial (and fills
    // the body all the way down to the screen edge), so the digital-style
    // date row + bottom weekday strip apply to the digital face only.
    match current_face() {
        WatchFace::Digital => {
            draw_digital(display, &clock)?;
            draw_date(display, &clock)?;
            draw_day_strip(display, clock.weekday)?;
        }
        WatchFace::Analog => draw_analog(display, &clock)?,
    }
    Ok(())
}

// ── KV load / persist ───────────────────────────────────────────────────────

#[cfg(feature = "embassy-base")]
pub(super) async fn load_settings_from_kv(ns: &crate::fw::kv::KvNamespace) {
    let mut b = [0u8; 1];
    if let Ok(1) = ns.get("face", &mut b).await
        && b[0] <= 1
    {
        WATCH_FACE.store(b[0], Ordering::Relaxed);
    }
}

#[cfg(feature = "embassy-base")]
pub(super) async fn persist(ns: &crate::fw::kv::KvNamespace) {
    let _ = ns
        .set("face", &[WATCH_FACE.load(Ordering::Relaxed)], true)
        .await;
}
