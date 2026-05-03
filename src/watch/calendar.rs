//! Calendar screen — month-grid view of every enabled one-shot alarm
//! slot, with a movable cursor and a per-day detail mode.  Sits in the
//! icon grid right after the Watch screen; reads the same alarm-slot
//! state that fires the buzzer, so any event you load via `ALARMS.ICS`
//! automatically shows up here.
//!
//! Three modes — same shape as the Watch face's "consume arrows only
//! when needed" pattern, so the user can scroll past Calendar with
//! Left/Right without it grabbing the input:
//!
//!   * **Passive** (default on entry): the grid is rendered but the
//!     cursor border is hidden.  Up/Down/Left/Right/Cancel fall through
//!     to the menu layer so screen-nav works.  Fire/Execute is the only
//!     consumed button — it transitions into Active.
//!
//!   * **Active**: cursor border becomes visible.  Up/Down/Left/Right
//!     move the cursor one cell (crossing month boundaries via
//!     fasttime arithmetic).  Fire/Execute drills into Day-detail.
//!     Cancel returns to Passive.
//!
//!   * **Day detail**: full-screen list of every event on the cursor
//!     day, scrollable, reusing the same alarm-slot state.  Cancel
//!     returns to Active.
//!
//! Today's cell gets a red fill with the day number in white.  Days
//! with one or more events get a small red dot above the day number.
//! The cursor cell (in Active) gets a 1 px black border drawn around
//! everything else.  The bottom strip previews the cursor day's first
//! event in Passive *and* Active so you see today's plan at a glance
//! the moment you land on the screen.

use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::mono_font::ascii::{FONT_6X10, FONT_6X13_BOLD, FONT_7X13_BOLD};
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{Circle, PrimitiveStyle, Rectangle};
use embedded_graphics::text::{Alignment, Baseline, Text, TextStyleBuilder};

use core::sync::atomic::{AtomicU8, AtomicU16, Ordering};

use super::alarm::{
    N_ALARMS, alarm_day_n, alarm_enabled_n, alarm_end_hour_n, alarm_end_minute_n, alarm_hour_n,
    alarm_is_one_shot_n, alarm_minute_n, alarm_month_n, alarm_summary_n, alarm_year_n,
};
use crate::menu::ButtonId;
use crate::{BLACK, RED, TriColor, WHITE, draw_frame};

// ── State ───────────────────────────────────────────────────────────────────

const MODE_PASSIVE: u8 = 0;
const MODE_ACTIVE: u8 = 1;
const MODE_DAY_DETAIL: u8 = 2;
/// Full-screen agenda list of every event on the cursor day, with full
/// (untruncated) summaries.  Fire from day-detail enters; Cancel
/// returns.  Lets users see short events whose blocks were too small
/// to show their title inline on the timeline.
const MODE_DAY_LIST: u8 = 3;

static MODE: AtomicU8 = AtomicU8::new(MODE_PASSIVE);

/// Cursor (selected) date.  `(0, 0, 0)` is the sentinel meaning
/// "uninitialised — pick a sensible default on the next draw".
static CURSOR_YEAR: AtomicU16 = AtomicU16::new(0);
static CURSOR_MONTH: AtomicU8 = AtomicU8::new(0);
static CURSOR_DAY: AtomicU8 = AtomicU8::new(0);

const MAX_EVENTS: usize = N_ALARMS;

/// First hour visible at the top of the day-detail timeline (0..=23).
/// Sentinel `0xFF` means "auto-position on next render" — set when the
/// user enters day-detail, then resolved to the first event's hour
/// (or the current hour for today) and replaced in-place.
static DAY_VIEW_TOP_HOUR: AtomicU8 = AtomicU8::new(0xFF);

/// Horizontal scroll offset (in chars) applied to every event title in
/// day-detail.  The "HH:MM " prefix stays pinned; only the summary text
/// scrolls so the user can still tell which event is which.  Stepped by
/// Right (forward) / Left (back) in 3-char increments, capped at 24.
/// Reset to 0 on Cancel.
static DAY_VIEW_TITLE_SCROLL: AtomicU8 = AtomicU8::new(0);
const TITLE_SCROLL_STEP: u8 = 3;
const TITLE_SCROLL_MAX: u8 = 24;

/// Top-of-window row offset for the day-list popup.  Stepped by Up
/// (back) / Down (forward) one row at a time.  Cleared on entry so the
/// popup always opens at the first event of the day.
static DAY_LIST_SCROLL: AtomicU8 = AtomicU8::new(0);

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
    /// Event end time on the same day.  Mirrors `(hour, minute)` for
    /// zero-duration events (DTEND missing in the source ICS).
    end_hour: u8,
    end_minute: u8,
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
            end_hour: alarm_end_hour_n(slot),
            end_minute: alarm_end_minute_n(slot),
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
        MODE_DAY_LIST => dispatch_day_list(btn),
        MODE_DAY_DETAIL => dispatch_day_detail(btn),
        MODE_ACTIVE => dispatch_active(btn),
        _ => dispatch_passive(btn),
    }
}

/// Passive: the only button we consume is Fire/Execute (transitions to
/// Active).  Everything else falls through so the menu can do
/// screen-nav, dismiss alarms, etc. — the same shape as the Watch face.
fn dispatch_passive(btn: ButtonId) -> bool {
    match btn {
        ButtonId::Fire | ButtonId::Execute => {
            MODE.store(MODE_ACTIVE, Ordering::Relaxed);
            true
        }
        _ => false,
    }
}

fn dispatch_active(btn: ButtonId) -> bool {
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
            // Re-arm the day-view auto-scroll for the day we just dropped
            // into.  The next render computes the right top-hour based on
            // events / now and stores it.
            DAY_VIEW_TOP_HOUR.store(0xFF, Ordering::Relaxed);
            MODE.store(MODE_DAY_DETAIL, Ordering::Relaxed);
            return true;
        }
        ButtonId::Cancel => {
            MODE.store(MODE_PASSIVE, Ordering::Relaxed);
            return true;
        }
    };
    set_cursor(next);
    true
}

fn dispatch_day_detail(btn: ButtonId) -> bool {
    // Up/Down:        scroll the timeline by an hour.
    // Left/Right:     scroll all event titles left/right in 3-char steps so
    //                 long titles like "Daily Volunteer Meeting" can be
    //                 read past their truncation point.  Day switching
    //                 isn't bound here — it's an uncommon action; Cancel
    //                 back to the grid, arrow to a different day, Fire to
    //                 enter again.
    // Fire / Execute: open the full-screen event-list popup so short
    //                 events whose blocks were too small to fit a title
    //                 inline can still be inspected.
    // Cancel:         back to the grid (title scroll resets to 0).
    match btn {
        ButtonId::Up => {
            let cur_top = DAY_VIEW_TOP_HOUR.load(Ordering::Relaxed);
            // Treat sentinel as 0 for the bounds-check; renderer will
            // resolve the new value into the visible range.
            let resolved = if cur_top == 0xFF { 0 } else { cur_top };
            DAY_VIEW_TOP_HOUR.store(resolved.saturating_sub(1), Ordering::Relaxed);
            return true;
        }
        ButtonId::Down => {
            let cur_top = DAY_VIEW_TOP_HOUR.load(Ordering::Relaxed);
            let resolved = if cur_top == 0xFF { 0 } else { cur_top };
            // Cap at 23 so the user can't scroll past the end of the day.
            DAY_VIEW_TOP_HOUR.store(resolved.saturating_add(1).min(23), Ordering::Relaxed);
            return true;
        }
        ButtonId::Right => {
            let cur = DAY_VIEW_TITLE_SCROLL.load(Ordering::Relaxed);
            let next = cur.saturating_add(TITLE_SCROLL_STEP).min(TITLE_SCROLL_MAX);
            DAY_VIEW_TITLE_SCROLL.store(next, Ordering::Relaxed);
            return true;
        }
        ButtonId::Left => {
            let cur = DAY_VIEW_TITLE_SCROLL.load(Ordering::Relaxed);
            DAY_VIEW_TITLE_SCROLL.store(cur.saturating_sub(TITLE_SCROLL_STEP), Ordering::Relaxed);
            return true;
        }
        ButtonId::Fire | ButtonId::Execute => {
            DAY_LIST_SCROLL.store(0, Ordering::Relaxed);
            MODE.store(MODE_DAY_LIST, Ordering::Relaxed);
            return true;
        }
        ButtonId::Cancel => {
            DAY_VIEW_TITLE_SCROLL.store(0, Ordering::Relaxed);
            MODE.store(MODE_ACTIVE, Ordering::Relaxed);
            true
        }
    }
}

/// Day-list popup — full-screen scrollable agenda for the cursor day,
/// with full (untruncated) event summaries.  Up / Down scroll one row
/// at a time; Cancel returns to the timeline.  Other buttons are
/// swallowed so the screen-nav doesn't take over while the popup is up.
fn dispatch_day_list(btn: ButtonId) -> bool {
    match btn {
        ButtonId::Up => {
            let cur = DAY_LIST_SCROLL.load(Ordering::Relaxed);
            DAY_LIST_SCROLL.store(cur.saturating_sub(1), Ordering::Relaxed);
            true
        }
        ButtonId::Down => {
            let cur = DAY_LIST_SCROLL.load(Ordering::Relaxed);
            // Loose cap — the renderer just leaves rows blank past the
            // end of the day's events.  N_ALARMS is the absolute upper
            // bound on events ever importable.
            DAY_LIST_SCROLL.store(cur.saturating_add(1).min(N_ALARMS as u8), Ordering::Relaxed);
            true
        }
        ButtonId::Cancel => {
            MODE.store(MODE_DAY_DETAIL, Ordering::Relaxed);
            true
        }
        _ => true,
    }
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
        end_hour: 0,
        end_minute: 0,
        slot: 0,
    }; MAX_EVENTS];
    let n = collect_sorted(&mut events_buf);
    let events = &events_buf[..n];

    match MODE.load(Ordering::Relaxed) {
        MODE_DAY_LIST => draw_day_list(display, events),
        MODE_DAY_DETAIL => draw_day_detail(display, events),
        MODE_ACTIVE => draw_grid(display, events, true),
        _ => draw_grid(display, events, false),
    }
}

fn draw_grid<D>(display: &mut D, events: &[EventRow], active: bool) -> Result<(), D::Error>
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

            // Cursor border — drawn last so it sits on top of everything,
            // and only in Active mode (Passive hides it so the user knows
            // arrows will fall through to screen-nav).  In-month cells only;
            // the cursor itself is always one of the in-month cells.
            if active && is_cursor && in_month {
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

/// Day-detail view: an agenda timeline.
///
/// Layout (152×152, with the frame header drawn earlier in `draw`):
///
/// ```text
///        ┌─────────────────────────────┐  y=0..17  Calendar / [bat]
///        │  Wed 15 Jul 2026 (red bar)  │  y=20..36 day header
///        ├─────────────────────────────┤
///   06   │                             │  y=40..148 timeline
///        │░░░░ 09:30 Workshop          │
///   09   │░░░░░░                       │  ← block height ∝ duration
///        │                             │
///   12   │█ 12:00 Lunch                │  ← short event = thin marker
///        │░░ 13:00 Talk                │
///   15   │░░                           │
///        │                             │
///   18   │── ←─ red "now" line if today│
///        │░ 17:30 Demo                 │
///   21   │                             │
///        └─────────────────────────────┘
/// ```
///
/// Timeline shows the fixed 06:00–24:00 window (18 h × 6 px = 108 px).
/// Events render as filled black blocks with white title text inside;
/// blocks shorter than ~10 px omit the title.  Today's "now" position
/// is marked with a red horizontal line.  Empty days show
/// `(no events)` over the timeline.
fn draw_day_detail<D>(display: &mut D, events: &[EventRow]) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let cursor = ensure_cursor(events);
    let today_ymd = today();

    // ── Day header (compact red bar) ─────────────────────────────────────
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

    // ── Timeline ─────────────────────────────────────────────────────────
    // Zoom is fixed; the visible window scrolls.  At 12 px/hour the
    // 108 px timeline fits exactly 9 hours — a useful "fit one Bornhack
    // session-block" range.  Up/Down in dispatch_day_detail scrolls in
    // 1-hour increments.
    const TL_TOP_Y: i32 = 40;
    const TL_BOT_Y: i32 = 148;
    const PX_PER_HOUR: i32 = 18;
    const HOURS_VISIBLE: i32 = (TL_BOT_Y - TL_TOP_Y) / PX_PER_HOUR; // = 6
    const TL_AXIS_X: i32 = 20;
    const TL_LEFT_X: i32 = 22;
    const TL_RIGHT_X: i32 = 148;

    // Filter events to the cursor day (collected before resolving the
    // scroll position so we can auto-scroll to the first event).
    let day_evs: heapless::Vec<&EventRow, MAX_EVENTS> = events
        .iter()
        .filter(|ev| ev.year == cursor.0 && ev.month == cursor.1 && ev.day == cursor.2)
        .collect();

    // Resolve the scroll sentinel to a sensible top-hour: first event
    // hour, or current hour if today, or 06:00.  Clamp so the visible
    // window always stays inside 0..24.
    let mut top_hour = DAY_VIEW_TOP_HOUR.load(Ordering::Relaxed);
    if top_hour == 0xFF {
        let chosen = if let Some(c) = super::clock::wall_clock() {
            if (c.year, c.month, c.day) == cursor {
                c.hour as i32
            } else {
                day_evs.first().map(|e| e.hour as i32).unwrap_or(6)
            }
        } else {
            day_evs.first().map(|e| e.hour as i32).unwrap_or(6)
        };
        // Land one hour above the anchor so it isn't pinned to the
        // very top edge.
        let anchored = (chosen - 1).clamp(0, 24 - HOURS_VISIBLE);
        top_hour = anchored as u8;
        DAY_VIEW_TOP_HOUR.store(top_hour, Ordering::Relaxed);
    } else if (top_hour as i32) > 24 - HOURS_VISIBLE {
        top_hour = (24 - HOURS_VISIBLE) as u8;
        DAY_VIEW_TOP_HOUR.store(top_hour, Ordering::Relaxed);
    }
    let tl_start_hour = top_hour as i32;

    // Convert (hour, minute) to a y-pixel inside the timeline,
    // clamped to the visible window.
    let y_for_time = |h: u8, m: u8| -> i32 {
        let total_min = h as i32 * 60 + m as i32 - tl_start_hour * 60;
        let clamped = total_min.clamp(0, HOURS_VISIBLE * 60);
        TL_TOP_Y + clamped * PX_PER_HOUR / 60
    };

    // Vertical axis line.
    Rectangle::new(
        Point::new(TL_AXIS_X, TL_TOP_Y),
        Size::new(1, (TL_BOT_Y - TL_TOP_Y) as u32),
    )
    .into_styled(PrimitiveStyle::with_fill(BLACK))
    .draw(display)?;

    // Hour labels + tick marks every hour (18 px between labels — fits
    // a FONT_6X13_BOLD line with comfortable headroom; the bold weight
    // prints noticeably crisper on the e-paper's grey-ish whites).
    let label_style = MonoTextStyle::new(&FONT_6X13_BOLD, BLACK);
    let right_align = TextStyleBuilder::new()
        .baseline(Baseline::Middle)
        .alignment(Alignment::Right)
        .build();
    let mut h = tl_start_hour;
    while h <= tl_start_hour + HOURS_VISIBLE {
        let label_y = TL_TOP_Y + (h - tl_start_hour) * PX_PER_HOUR + 6;
        let mut s: heapless::String<3> = heapless::String::new();
        let _ = core::fmt::write(&mut s, format_args!("{:02}", h));
        Text::with_text_style(
            &s,
            Point::new(TL_AXIS_X - 2, label_y),
            label_style,
            right_align,
        )
        .draw(display)?;
        Rectangle::new(Point::new(TL_AXIS_X, label_y - 1), Size::new(3, 1))
            .into_styled(PrimitiveStyle::with_fill(BLACK))
            .draw(display)?;
        h += 1;
    }

    let inside_left = TextStyleBuilder::new()
        .baseline(Baseline::Middle)
        .alignment(Alignment::Left)
        .build();

    // Wall-clock-now in day-minutes, but only when the cursor day is
    // actually today — otherwise there's nothing to highlight as
    // "current".  Reused below for both the per-block colour and the
    // horizontal "now" line so we only hit `wall_clock()` once.
    let now_min_today: Option<i32> = if is_today {
        super::clock::wall_clock().map(|c| c.hour as i32 * 60 + c.minute as i32)
    } else {
        None
    };

    // Event blocks — only those that intersect the visible window.
    for ev in &day_evs {
        let ev_start_min = ev.hour as i32 * 60 + ev.minute as i32;
        let ev_end_min = ev.end_hour as i32 * 60 + ev.end_minute as i32;
        let win_start_min = tl_start_hour * 60;
        let win_end_min = win_start_min + HOURS_VISIBLE * 60;
        if ev_end_min < win_start_min || ev_start_min > win_end_min {
            continue;
        }

        let start_y = y_for_time(ev.hour, ev.minute);
        let end_y = y_for_time(ev.end_hour, ev.end_minute);
        // Min 4 px tall so zero-duration events are still visible.
        let height = (end_y - start_y).max(4) as u32;
        // Carve 1 px off the bottom of every block so back-to-back
        // events (one ending at the same minute the next starts) don't
        // fuse into a single tall rectangle — the gap reads as a
        // hairline divider.  Standalone blocks lose nothing visible
        // since the gap blends into the white timeline background.
        let block_h = height.saturating_sub(1).max(1);
        let block_w = (TL_RIGHT_X - TL_LEFT_X) as u32;

        // Currently-happening events render in red — start-inclusive,
        // end-exclusive ([start, end)) so a 13:00–14:00 block is
        // highlighted from 13:00:00 up to but not including 14:00:00,
        // matching the standard calendar convention.  Zero-duration
        // markers (start == end) never highlight as current.
        let is_now = matches!(
            now_min_today,
            Some(now) if ev_start_min <= now && now < ev_end_min,
        );
        let fill = if is_now { RED } else { BLACK };

        Rectangle::new(Point::new(TL_LEFT_X, start_y), Size::new(block_w, block_h))
            .into_styled(PrimitiveStyle::with_fill(fill))
            .draw(display)?;

        // Title fits inside if the block (post-divider) is at least one
        // text-line tall — FONT_6X13_BOLD needs the full 13 px or its
        // bottom row would land in the divider gap.  At 18 px/hour
        // that means 60-min events get titles; 30-min and 45-min events
        // render as bare time markers.
        if block_h >= 13 {
            let summary = alarm_summary_n(ev.slot as usize);
            // Apply the global title scroll offset.  `get(N..)` returns
            // None if N is past the end of the (NUL-trimmed) summary —
            // fine, the title row just renders as the bare time prefix
            // for that event, which still tells the user what's where
            // and is the cue to press Execute back.
            let scroll = DAY_VIEW_TITLE_SCROLL.load(Ordering::Relaxed) as usize;
            let scrolled = summary.as_str().get(scroll..).unwrap_or("");
            let mut row: heapless::String<48> = heapless::String::new();
            let _ = core::fmt::write(
                &mut row,
                format_args!("{:02}:{:02} {}", ev.hour, ev.minute, scrolled),
            );
            Text::with_text_style(
                &row,
                Point::new(TL_LEFT_X + 2, start_y + 8),
                MonoTextStyle::new(&FONT_6X13_BOLD, WHITE),
                inside_left,
            )
            .draw(display)?;
        }
    }

    // "Now" indicator — red horizontal line across the events area.
    // Mostly invisible inside a currently-happening (red) block, but
    // still useful as a marker during gap time between events.
    if let Some(now_min) = now_min_today {
        let win_start_min = tl_start_hour * 60;
        let win_end_min = win_start_min + HOURS_VISIBLE * 60;
        if (win_start_min..=win_end_min).contains(&now_min) {
            let now_h = (now_min / 60) as u8;
            let now_m = (now_min % 60) as u8;
            let now_y = y_for_time(now_h, now_m);
            Rectangle::new(
                Point::new(TL_AXIS_X, now_y),
                Size::new((TL_RIGHT_X - TL_AXIS_X) as u32, 1),
            )
            .into_styled(PrimitiveStyle::with_fill(RED))
            .draw(display)?;
        }
    }

    // Scroll indicators on the right edge: ↑ if events exist before
    // the visible window, ↓ if events exist after.
    let arrow_style = MonoTextStyle::new(&FONT_6X10, BLACK);
    let above = day_evs
        .iter()
        .any(|ev| (ev.hour as i32 * 60 + ev.minute as i32) < tl_start_hour * 60);
    let below = day_evs.iter().any(|ev| {
        (ev.hour as i32 * 60 + ev.minute as i32) >= (tl_start_hour + HOURS_VISIBLE) * 60
    });
    if above {
        Text::with_text_style("^", Point::new(146, TL_TOP_Y + 4), arrow_style, centered)
            .draw(display)?;
    }
    if below {
        Text::with_text_style("v", Point::new(146, TL_BOT_Y - 4), arrow_style, centered)
            .draw(display)?;
    }

    if day_evs.is_empty() {
        // Soft "(no events)" overlay so the timeline doesn't look broken.
        Text::with_text_style(
            "(no events)",
            Point::new(85, 90),
            MonoTextStyle::new(&FONT_7X13_BOLD, BLACK),
            centered,
        )
        .draw(display)?;
    }

    Ok(())
}

/// Day-list popup — full-screen scrollable list of every event on the
/// cursor day with full (untruncated) summaries.  Reached from
/// day-detail by Fire / Execute; see `MODE_DAY_LIST`.
fn draw_day_list<D>(display: &mut D, events: &[EventRow]) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let cursor = ensure_cursor(events);
    let today_ymd = today();

    // ── Day header (same red bar / FONT_7X13_BOLD as day-detail) ─────────
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

    // ── Event rows ───────────────────────────────────────────────────────
    // Filter to cursor day; events arrive already sorted by start time.
    let day_evs: heapless::Vec<&EventRow, MAX_EVENTS> = events
        .iter()
        .filter(|ev| ev.year == cursor.0 && ev.month == cursor.1 && ev.day == cursor.2)
        .collect();

    if day_evs.is_empty() {
        Text::with_text_style(
            "(no events)",
            Point::new(76, 90),
            MonoTextStyle::new(&FONT_7X13_BOLD, BLACK),
            centered,
        )
        .draw(display)?;
        return Ok(());
    }

    const ROW_TOP_Y: i32 = 42;
    const ROW_BOT_Y: i32 = 148;
    const ROW_H: i32 = 14; // FONT_6X13_BOLD = 13 px + 1 px gap
    const ROWS_VISIBLE: i32 = (ROW_BOT_Y - ROW_TOP_Y) / ROW_H; // = 7
    const ROW_LEFT_X: i32 = 2;

    // Clamp the stored scroll to a sensible window so the user can't
    // wedge themselves on a fully-blank screen by spamming Down.
    let scroll = DAY_LIST_SCROLL.load(Ordering::Relaxed) as i32;
    let max_scroll = (day_evs.len() as i32 - ROWS_VISIBLE).max(0);
    let scroll = scroll.min(max_scroll);
    DAY_LIST_SCROLL.store(scroll as u8, Ordering::Relaxed);

    let row_style = MonoTextStyle::new(&FONT_6X13_BOLD, BLACK);
    let left_align = TextStyleBuilder::new()
        .baseline(Baseline::Middle)
        .alignment(Alignment::Left)
        .build();

    for r in 0..ROWS_VISIBLE {
        let idx = scroll + r;
        if (idx as usize) >= day_evs.len() {
            break;
        }
        let ev = day_evs[idx as usize];
        let summary = alarm_summary_n(ev.slot as usize);
        let mut row: heapless::String<48> = heapless::String::new();
        let _ = core::fmt::write(
            &mut row,
            format_args!(
                "{:02}:{:02}-{:02}:{:02} {}",
                ev.hour,
                ev.minute,
                ev.end_hour,
                ev.end_minute,
                summary.as_str()
            ),
        );
        let y = ROW_TOP_Y + r * ROW_H + ROW_H / 2;
        Text::with_text_style(&row, Point::new(ROW_LEFT_X, y), row_style, left_align)
            .draw(display)?;
    }

    // Scroll indicators on the right edge: ^ if rows hidden above,
    // v if rows hidden below.
    let arrow_style = MonoTextStyle::new(&FONT_6X10, BLACK);
    if scroll > 0 {
        Text::with_text_style("^", Point::new(146, ROW_TOP_Y + 4), arrow_style, centered)
            .draw(display)?;
    }
    if (scroll + ROWS_VISIBLE) < day_evs.len() as i32 {
        Text::with_text_style("v", Point::new(146, ROW_BOT_Y - 4), arrow_style, centered)
            .draw(display)?;
    }

    Ok(())
}
