//! Calendar screen — month-grid view of every enabled one-shot alarm
//! slot, with a movable cursor and a per-day detail mode.  Sits in the
//! icon grid right after the Watch screen; reads the same alarm-slot
//! state that fires the buzzer, so any event you load via `ALARMS.ICS`
//! automatically shows up here.
//!
//! Two visual modes:
//!
//!   * **Grid** (default): a 6-week, 7-column grid of the cursor's month.
//!     Out-of-month cells are blank.  Today's cell gets a red fill with
//!     the day number in white.  Days with one or more events get a
//!     small red dot above the day number.  The cursor cell gets a 1 px
//!     black border (drawn around the red fill if today is also the
//!     cursor).  The bottom strip previews the cursor day's first event
//!     plus a `+N more` line when applicable.
//!
//!   * **Day detail**: full-screen list of every event on the cursor
//!     day, scrollable.  Reuses the same alarm-slot state.
//!
//! Buttons in **grid mode**:
//!   * Up / Down / Left / Right — move cursor one cell (crosses month
//!     boundaries automatically via fasttime arithmetic)
//!   * Fire / Execute            — enter day-detail mode
//!   * Cancel                    — falls through (lets the menu layer
//!                                  navigate to the next/previous
//!                                  screen)
//!
//! Buttons in **day-detail mode**:
//!   * Up / Down                 — scroll
//!   * Cancel                    — back to grid mode (consumed)
//!   * Left / Right / Fire / Execute — consumed, no-op (avoids
//!                                       accidental screen-nav)

use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::mono_font::ascii::{FONT_6X10, FONT_7X13_BOLD};
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{Circle, PrimitiveStyle, Rectangle};
use embedded_graphics::text::{Alignment, Baseline, Text, TextStyleBuilder};

use core::sync::atomic::{AtomicU8, AtomicU16, Ordering};

use super::alarm::{
    N_ALARMS, alarm_day_n, alarm_enabled_n, alarm_hour_n, alarm_is_one_shot_n, alarm_minute_n,
    alarm_month_n, alarm_summary_n, alarm_year_n,
};
use crate::menu::ButtonId;
use crate::{BLACK, RED, TriColor, WHITE, draw_frame};

// ── State ───────────────────────────────────────────────────────────────────

const MODE_GRID: u8 = 0;
const MODE_DAY_DETAIL: u8 = 1;

static MODE: AtomicU8 = AtomicU8::new(MODE_GRID);

/// Cursor (selected) date.  `(0, 0, 0)` is the sentinel meaning
/// "uninitialised — pick a sensible default on the next draw".
static CURSOR_YEAR: AtomicU16 = AtomicU16::new(0);
static CURSOR_MONTH: AtomicU8 = AtomicU8::new(0);
static CURSOR_DAY: AtomicU8 = AtomicU8::new(0);

/// Scroll offset within the day-detail event list.
static DETAIL_SCROLL: AtomicU8 = AtomicU8::new(0);

const MAX_EVENTS: usize = N_ALARMS;

// ── Layout ──────────────────────────────────────────────────────────────────

const MONTH_LABEL_Y: i32 = 25; // baseline middle
const WEEKDAY_STRIP_Y: i32 = 39;

const GRID_LEFT_X: i32 = 4;
const GRID_TOP_Y: i32 = 46;
const COL_W: i32 = 21; // 7 × 21 = 147 → fits with 4 left + 1 right margin
const ROW_H: i32 = 13; // 6 × 13 = 78
const N_ROWS: i32 = 6;

const FOOTER_Y: i32 = 130; // baseline middle of the first footer line
const FOOTER_Y_2: i32 = 144;

const DAY_NAMES_SHORT: [&str; 7] = ["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"];
const DAY_NAMES_LONG: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
const MONTH_ABBR: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

// ── Event collection ────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
struct EventRow {
    year: u16,
    month: u8,
    day: u8,
    hour: u8,
    minute: u8,
    /// Back-reference to the alarm slot — used to look up the summary.
    slot: u8,
}

fn collect_sorted(out: &mut [EventRow; MAX_EVENTS]) -> usize {
    let mut n = 0usize;
    for slot in 0..N_ALARMS {
        if !alarm_enabled_n(slot) || !alarm_is_one_shot_n(slot) {
            continue;
        }
        if n >= out.len() {
            break;
        }
        out[n] = EventRow {
            year: alarm_year_n(slot),
            month: alarm_month_n(slot),
            day: alarm_day_n(slot),
            hour: alarm_hour_n(slot),
            minute: alarm_minute_n(slot),
            slot: slot as u8,
        };
        n += 1;
    }
    for i in 1..n {
        let key = out[i];
        let mut j = i;
        while j > 0 && cmp_event(&out[j - 1], &key) == core::cmp::Ordering::Greater {
            out[j] = out[j - 1];
            j -= 1;
        }
        out[j] = key;
    }
    n
}

fn cmp_event(a: &EventRow, b: &EventRow) -> core::cmp::Ordering {
    (a.year, a.month, a.day, a.hour, a.minute).cmp(&(b.year, b.month, b.day, b.hour, b.minute))
}

fn day_has_events(events: &[EventRow], y: u16, m: u8, d: u8) -> bool {
    events
        .iter()
        .any(|ev| ev.year == y && ev.month == m && ev.day == d)
}

// ── Date helpers ────────────────────────────────────────────────────────────

fn today() -> Option<(u16, u8, u8)> {
    let c = super::clock::wall_clock()?;
    Some((c.year, c.month, c.day))
}

/// Weekday for an arbitrary date, `0..=6` (Mon..Sun).  Falls back to 0
/// if the date is outside fasttime's representable range.
fn weekday_for(year: u16, month: u8, day: u8) -> u8 {
    fasttime::Date::from_ymd(year as i32, month, day)
        .map(|d| d.weekday().number_from_monday().saturating_sub(1))
        .unwrap_or(0)
}

/// Add `delta_days` to `(year, month, day)` and return the resulting
/// date.  Returns the input unchanged on out-of-range arithmetic.
fn add_days(year: u16, month: u8, day: u8, delta_days: i64) -> (u16, u8, u8) {
    let Ok(d) = fasttime::Date::from_ymd(year as i32, month, day) else {
        return (year, month, day);
    };
    let Ok(d2) = d.add_days(delta_days) else {
        return (year, month, day);
    };
    (d2.year as u16, d2.month, d2.day)
}

/// Cursor-date getter (sentinel-aware): if uninitialised, pick today,
/// then first event, then a Bornhack-2026 fallback.
fn ensure_cursor(events: &[EventRow]) -> (u16, u8, u8) {
    let y = CURSOR_YEAR.load(Ordering::Relaxed);
    if y != 0 {
        return (
            y,
            CURSOR_MONTH.load(Ordering::Relaxed),
            CURSOR_DAY.load(Ordering::Relaxed),
        );
    }
    let init = today()
        .or_else(|| events.first().map(|e| (e.year, e.month, e.day)))
        .unwrap_or((2026, 7, 15));
    set_cursor(init);
    init
}

fn set_cursor(ymd: (u16, u8, u8)) {
    CURSOR_YEAR.store(ymd.0, Ordering::Relaxed);
    CURSOR_MONTH.store(ymd.1, Ordering::Relaxed);
    CURSOR_DAY.store(ymd.2, Ordering::Relaxed);
}

// ── Button dispatch ─────────────────────────────────────────────────────────

pub fn dispatch(btn: ButtonId) -> bool {
    match MODE.load(Ordering::Relaxed) {
        MODE_DAY_DETAIL => dispatch_day_detail(btn),
        _ => dispatch_grid(btn),
    }
}

fn dispatch_grid(btn: ButtonId) -> bool {
    // We can't easily collect events here without re-doing the alarm walk;
    // skip event-aware logic in the dispatcher and just do date arithmetic.
    let cur = (
        CURSOR_YEAR.load(Ordering::Relaxed),
        CURSOR_MONTH.load(Ordering::Relaxed),
        CURSOR_DAY.load(Ordering::Relaxed),
    );
    // Sentinel — ignore until the renderer initialises it.
    if cur.0 == 0 {
        return matches!(
            btn,
            ButtonId::Up | ButtonId::Down | ButtonId::Left | ButtonId::Right
        );
    }
    let next = match btn {
        ButtonId::Up => add_days(cur.0, cur.1, cur.2, -7),
        ButtonId::Down => add_days(cur.0, cur.1, cur.2, 7),
        ButtonId::Left => add_days(cur.0, cur.1, cur.2, -1),
        ButtonId::Right => add_days(cur.0, cur.1, cur.2, 1),
        ButtonId::Fire | ButtonId::Execute => {
            DETAIL_SCROLL.store(0, Ordering::Relaxed);
            MODE.store(MODE_DAY_DETAIL, Ordering::Relaxed);
            return true;
        }
        ButtonId::Cancel => return false, // fall through to screen-nav
    };
    set_cursor(next);
    true
}

fn dispatch_day_detail(btn: ButtonId) -> bool {
    let cur = DETAIL_SCROLL.load(Ordering::Relaxed);
    match btn {
        ButtonId::Up => DETAIL_SCROLL.store(cur.saturating_sub(1), Ordering::Relaxed),
        ButtonId::Down => DETAIL_SCROLL.store(cur.saturating_add(1), Ordering::Relaxed),
        ButtonId::Cancel => MODE.store(MODE_GRID, Ordering::Relaxed),
        // Swallow other buttons so they don't accidentally screen-nav.
        ButtonId::Left | ButtonId::Right | ButtonId::Fire | ButtonId::Execute => {}
    }
    true
}

// ── Drawing ─────────────────────────────────────────────────────────────────

#[cfg(feature = "embassy-base")]
fn battery_pct() -> u8 {
    crate::fw::battery::read_pct()
}

#[cfg(not(feature = "embassy-base"))]
fn battery_pct() -> u8 {
    100
}

pub fn draw<D>(display: &mut D) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let bat = battery_pct();
    draw_frame(display, Some(("Calendar", &bat)), None)?;

    let mut events_buf = [EventRow {
        year: 0,
        month: 0,
        day: 0,
        hour: 0,
        minute: 0,
        slot: 0,
    }; MAX_EVENTS];
    let n = collect_sorted(&mut events_buf);
    let events = &events_buf[..n];

    match MODE.load(Ordering::Relaxed) {
        MODE_DAY_DETAIL => draw_day_detail(display, events),
        _ => draw_grid(display, events),
    }
}

fn draw_grid<D>(display: &mut D, events: &[EventRow]) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let cursor = ensure_cursor(events);
    let today_ymd = today();

    // ── Month label ────────────────────────────────────────────────────────
    let mon_idx = (cursor.1 as usize).saturating_sub(1).min(11);
    let mut buf: heapless::String<16> = heapless::String::new();
    let _ = core::fmt::write(
        &mut buf,
        format_args!("{} {}", MONTH_ABBR[mon_idx], cursor.0),
    );
    let centered = TextStyleBuilder::new()
        .baseline(Baseline::Middle)
        .alignment(Alignment::Center)
        .build();
    Text::with_text_style(
        &buf,
        Point::new(76, MONTH_LABEL_Y),
        MonoTextStyle::new(&FONT_7X13_BOLD, BLACK),
        centered,
    )
    .draw(display)?;

    // ── Weekday strip ──────────────────────────────────────────────────────
    let weekday_style = MonoTextStyle::new(&FONT_6X10, BLACK);
    for (col, name) in DAY_NAMES_SHORT.iter().enumerate() {
        let cx = GRID_LEFT_X + col as i32 * COL_W + COL_W / 2;
        Text::with_text_style(name, Point::new(cx, WEEKDAY_STRIP_Y), weekday_style, centered)
            .draw(display)?;
    }

    // ── Grid cells ─────────────────────────────────────────────────────────
    // Find the Monday at-or-before day 1 of the cursor's month.
    let first_weekday = weekday_for(cursor.0, cursor.1, 1) as i64;
    let start_date = add_days(cursor.0, cursor.1, 1, -first_weekday);

    for row in 0..N_ROWS {
        for col in 0..7i32 {
            let cell_date = add_days(
                start_date.0,
                start_date.1,
                start_date.2,
                row as i64 * 7 + col as i64,
            );
            let in_month = cell_date.1 == cursor.1 && cell_date.0 == cursor.0;

            let cell_x = GRID_LEFT_X + col * COL_W;
            let cell_y = GRID_TOP_Y + row * ROW_H;

            // Today fill — only when in-month so we don't paint the
            // out-of-month tail of the prior month.
            let is_today = matches!(today_ymd, Some(t) if t == cell_date);
            let is_cursor = cell_date == cursor;

            if is_today && in_month {
                Rectangle::new(
                    Point::new(cell_x, cell_y),
                    Size::new(COL_W as u32, ROW_H as u32),
                )
                .into_styled(PrimitiveStyle::with_fill(RED))
                .draw(display)?;
            }

            if in_month {
                // Day number — white if on today's red fill, black otherwise.
                let fg = if is_today { WHITE } else { BLACK };
                let mut nbuf: heapless::String<3> = heapless::String::new();
                let _ = core::fmt::write(&mut nbuf, format_args!("{}", cell_date.2));
                Text::with_text_style(
                    &nbuf,
                    Point::new(cell_x + COL_W / 2, cell_y + ROW_H / 2 + 1),
                    MonoTextStyle::new(&FONT_6X10, fg),
                    centered,
                )
                .draw(display)?;

                // Has-events dot in the top-right corner.
                if day_has_events(events, cell_date.0, cell_date.1, cell_date.2) {
                    Circle::new(Point::new(cell_x + COL_W - 5, cell_y + 1), 3)
                        .into_styled(PrimitiveStyle::with_fill(RED))
                        .draw(display)?;
                }
            }

            // Cursor border — drawn last so it sits on top of everything.
            // We draw it for in-month cells only; if the cursor would be
            // off-month we'd never get here because the cursor itself is
            // always one of the in-month cells (set_cursor is fed only
            // from add_days starting from a real date).
            if is_cursor && in_month {
                Rectangle::new(
                    Point::new(cell_x, cell_y),
                    Size::new(COL_W as u32, ROW_H as u32),
                )
                .into_styled(PrimitiveStyle::with_stroke(BLACK, 1))
                .draw(display)?;
            }
        }
    }

    // ── Footer: cursor day's first event + "+N more" ───────────────────────
    let cursor_evs: heapless::Vec<&EventRow, MAX_EVENTS> = events
        .iter()
        .filter(|ev| ev.year == cursor.0 && ev.month == cursor.1 && ev.day == cursor.2)
        .collect();

    let left = TextStyleBuilder::new()
        .baseline(Baseline::Middle)
        .alignment(Alignment::Left)
        .build();
    if cursor_evs.is_empty() {
        Text::with_text_style(
            "(no events)",
            Point::new(76, FOOTER_Y),
            MonoTextStyle::new(&FONT_6X10, BLACK),
            centered,
        )
        .draw(display)?;
    } else {
        let ev0 = cursor_evs[0];
        let summary = alarm_summary_n(ev0.slot as usize);
        let mut row: heapless::String<48> = heapless::String::new();
        let _ = core::fmt::write(
            &mut row,
            format_args!(
                "{:02}:{:02} {}",
                ev0.hour,
                ev0.minute,
                summary.as_str()
            ),
        );
        Text::with_text_style(
            &row,
            Point::new(4, FOOTER_Y),
            MonoTextStyle::new(&FONT_6X10, BLACK),
            left,
        )
        .draw(display)?;

        if cursor_evs.len() > 1 {
            let mut more: heapless::String<24> = heapless::String::new();
            let _ = core::fmt::write(
                &mut more,
                format_args!("+ {} more", cursor_evs.len() - 1),
            );
            Text::with_text_style(
                &more,
                Point::new(4, FOOTER_Y_2),
                MonoTextStyle::new(&FONT_6X10, BLACK),
                left,
            )
            .draw(display)?;
        }
    }

    Ok(())
}

fn draw_day_detail<D>(display: &mut D, events: &[EventRow]) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let cursor = ensure_cursor(events);
    let today_ymd = today();

    // ── Big date header ────────────────────────────────────────────────────
    let weekday = weekday_for(cursor.0, cursor.1, cursor.2) as usize;
    let mon_idx = (cursor.1 as usize).saturating_sub(1).min(11);
    let mut buf: heapless::String<24> = heapless::String::new();
    let _ = core::fmt::write(
        &mut buf,
        format_args!(
            "{} {} {} {}",
            DAY_NAMES_LONG[weekday], cursor.2, MONTH_ABBR[mon_idx], cursor.0
        ),
    );

    let is_today = matches!(today_ymd, Some(t) if t == cursor);
    if is_today {
        Rectangle::new(Point::new(0, 20), Size::new(152, 16))
            .into_styled(PrimitiveStyle::with_fill(RED))
            .draw(display)?;
    }
    let centered = TextStyleBuilder::new()
        .baseline(Baseline::Middle)
        .alignment(Alignment::Center)
        .build();
    let header_fg = if is_today { WHITE } else { RED };
    Text::with_text_style(
        &buf,
        Point::new(76, 28),
        MonoTextStyle::new(&FONT_7X13_BOLD, header_fg),
        centered,
    )
    .draw(display)?;

    // ── Event list ─────────────────────────────────────────────────────────
    let day_evs: heapless::Vec<&EventRow, MAX_EVENTS> = events
        .iter()
        .filter(|ev| ev.year == cursor.0 && ev.month == cursor.1 && ev.day == cursor.2)
        .collect();

    let left = TextStyleBuilder::new()
        .baseline(Baseline::Middle)
        .alignment(Alignment::Left)
        .build();

    if day_evs.is_empty() {
        Text::with_text_style(
            "(no events)",
            Point::new(76, 80),
            MonoTextStyle::new(&FONT_7X13_BOLD, BLACK),
            centered,
        )
        .draw(display)?;
        return Ok(());
    }

    const ROW_TOP_Y: i32 = 50;
    const ROW_STEP_Y: i32 = 14;
    const ROWS_VISIBLE: usize = 7; // (150 - 50) / 14 ≈ 7

    let raw_scroll = DETAIL_SCROLL.load(Ordering::Relaxed) as usize;
    let max_scroll = day_evs.len().saturating_sub(ROWS_VISIBLE);
    let scroll = raw_scroll.min(max_scroll);
    if raw_scroll != scroll {
        DETAIL_SCROLL.store(scroll as u8, Ordering::Relaxed);
    }

    for i in 0..ROWS_VISIBLE.min(day_evs.len() - scroll) {
        let ev = day_evs[scroll + i];
        let summary = alarm_summary_n(ev.slot as usize);
        let mut row: heapless::String<48> = heapless::String::new();
        let _ = core::fmt::write(
            &mut row,
            format_args!("{:02}:{:02} {}", ev.hour, ev.minute, summary.as_str()),
        );
        Text::with_text_style(
            &row,
            Point::new(4, ROW_TOP_Y + i as i32 * ROW_STEP_Y),
            MonoTextStyle::new(&FONT_7X13_BOLD, BLACK),
            left,
        )
        .draw(display)?;
    }

    // Scroll arrows.
    let arrow_style = MonoTextStyle::new(&FONT_6X10, BLACK);
    if scroll > 0 {
        Text::with_text_style("^", Point::new(146, ROW_TOP_Y), arrow_style, centered)
            .draw(display)?;
    }
    if scroll + ROWS_VISIBLE < day_evs.len() {
        Text::with_text_style(
            "v",
            Point::new(146, ROW_TOP_Y + (ROWS_VISIBLE as i32 - 1) * ROW_STEP_Y),
            arrow_style,
            centered,
        )
        .draw(display)?;
    }

    Ok(())
}
