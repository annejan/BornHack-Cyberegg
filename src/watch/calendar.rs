//! Calendar screen — month-grid view of the whole `ALARMS.ICS` file,
//! with a movable cursor and a per-day detail mode.  Sits in the icon
//! grid right after the Clock screen.
//!
//! Nothing here reads alarm slots.  The grid's has-events dots come from
//! the one-bit-per-day index (`super::day_has_events`) and the day views
//! from the single-day cache (`super::with_day_cache`, refilled by
//! `super::request_day` when the cursor moves), both of which cover
//! every event in the file however big it is.  The alarm slots hold only
//! the near future, because ringing is all they are for.
//!
//! Three modes — same shape as the Clock face's "consume arrows only
//! when needed" pattern, so the user can scroll past Calendar with
//! Left/Right without it grabbing the input:
//!
//!   * **Passive** (default on entry): the grid is rendered but the cursor
//!     border is hidden.  Up/Down/Left/Right/Cancel fall through to the menu
//!     layer so screen-nav works.  Fire/Execute is the only consumed button —
//!     it transitions into Active.
//!
//!   * **Active**: cursor border becomes visible.  Up/Down/Left/Right move the
//!     cursor one cell (crossing month boundaries via fasttime arithmetic).
//!     Fire/Execute drills into Day-detail. Cancel returns to Passive.
//!
//!   * **Day detail**: full-screen list of every event on the cursor day,
//!     scrollable, reusing the same alarm-slot state.  Cancel returns to
//!     Active.
//!
//! Today's cell gets a red fill with the day number in white.  Days
//! with one or more events get a small red dot above the day number.
//! The cursor cell (in Active) gets a 1 px black border drawn around
//! everything else.  The bottom strip previews the cursor day's first
//! event in Passive *and* Active so you see today's plan at a glance
//! the moment you land on the screen.

use core::sync::atomic::{AtomicU8, AtomicU16, Ordering};

use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::mono_font::iso_8859_1::FONT_6X10;
use embedded_graphics::mono_font::iso_8859_1::{FONT_6X13_BOLD, FONT_7X13_BOLD};
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{Circle, PrimitiveStyle, Rectangle};
use embedded_graphics::text::{Alignment, Baseline, Text, TextStyle, TextStyleBuilder};

// ───────────────────────────────────────────────────────────────────────────
// Fake-bold helper
// ───────────────────────────────────────────────────────────────────────────
//
// embedded-graphics' iso_8859_1 catalogue stops at FONT_6X13_BOLD — there is
// no FONT_6X10_BOLD.  Going up to 6×13 would force the calendar grid layout
// to grow by 3 px per row; bumping COL_W is similarly disruptive elsewhere.
// Instead, draw every 6×10 glyph twice — once at the requested point, once
// at point.x + 1 — which doubles the stroke width without changing layout.
// On the e-ink panel that's the difference between a 1-pixel-thin stroke and
// a 2-pixel one, which is the readability win we're after.
fn draw_bold<D>(
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

/// Centre of the unread-PM envelope in the frame header.  The header is
/// the title at x=4 (7 px/char, so "Calendar" runs to x=60) and the
/// battery icon at x=128; the envelope is 13 px wide and its optional
/// `+N` suffix another ~18, so this sits clear of both.  `PM_BADGE_CY`
/// matches the title's vertical centre.
const PM_BADGE_CX: i32 = 76;
const PM_BADGE_CY: i32 = 8;

/// Buffer size for an event row: `HH:MM-HH:MM ` plus the label.
///
/// Labels are stored as Latin-1, one byte per character, but a heapless
/// `String` holds UTF-8 — so a 31-character title can be 62 bytes once
/// decoded.  Sizing this for 31 *bytes* silently dropped the whole title
/// on any accented event, because `push_str` is all-or-nothing.
const ROW_BUF: usize = super::ics::SUMMARY_LEN * 2 + 16;

const MONTH_LABEL_Y: i32 = 25; // baseline middle
const WEEKDAY_STRIP_Y: i32 = 39;

const GRID_LEFT_X: i32 = 4;
const GRID_TOP_Y: i32 = 46;
const COL_W: i32 = 21; // 7 × 21 = 147 → fits with 4 left + 1 right margin
const ROW_H: i32 = 13; // 6 × 13 = 78
const N_ROWS: i32 = 6;

const FOOTER_Y: i32 = 130; // baseline middle of the first footer line
const FOOTER_Y_2: i32 = 144;

/// Baseline of the day-list "+N more today" note.  `Baseline::Middle` on
/// a 10 px font puts the glyph rows at `y - 4 ..= y + 5`, so on a
/// 152-row panel (0..=151) this is the lowest value that isn't clipped.
const OVERFLOW_NOTE_Y: i32 = 146;

const DAY_NAMES_SHORT: [&str; 7] = ["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"];
const DAY_NAMES_LONG: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
const MONTH_ABBR: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// Drop the first `n` *characters* of `s`.  Empty once `n` runs past the
/// end.  Used for the day-view title scroll, where counting bytes would
/// split a multi-byte character in an accented title.
fn scroll_chars(s: &str, n: usize) -> &str {
    match s.char_indices().nth(n) {
        Some((byte, _)) => &s[byte..],
        None => "",
    }
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
/// then the first day the calendar has anything on, then a
/// Bornhack-2026 fallback.
fn ensure_cursor() -> (u16, u8, u8) {
    let y = CURSOR_YEAR.load(Ordering::Relaxed);
    if y != 0 {
        return (
            y,
            CURSOR_MONTH.load(Ordering::Relaxed),
            CURSOR_DAY.load(Ordering::Relaxed),
        );
    }
    let init = today()
        .or_else(super::first_indexed_day)
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
/// screen-nav, dismiss alarms, etc. — the same shape as the Clock face.
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

/// The timeline's current top hour, or `None` while there is nothing to
/// scroll yet.
///
/// `DAY_VIEW_TOP_HOUR` holds `0xFF` until the renderer has resolved where
/// to anchor the view, and it only resolves once the day cache actually
/// holds the cursor day. Stepping from an unresolved sentinel used to
/// substitute hour 0, which turned the next Up or Down into a jump to
/// midnight — and, worse, destroyed the sentinel, so the view never
/// auto-scrolled to the day's events when they finally arrived.
///
/// Scroll presses during that window are swallowed instead: there is no
/// rendered position to step from, and the day is about to place itself.
fn scrollable_top_hour() -> Option<u8> {
    let top = DAY_VIEW_TOP_HOUR.load(Ordering::Relaxed);
    if top == 0xFF { None } else { Some(top) }
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
            if let Some(cur_top) = scrollable_top_hour() {
                DAY_VIEW_TOP_HOUR.store(cur_top.saturating_sub(1), Ordering::Relaxed);
            }
            true
        }
        ButtonId::Down => {
            if let Some(cur_top) = scrollable_top_hour() {
                // Cap at 23 so the user can't scroll past the end of the day.
                DAY_VIEW_TOP_HOUR.store(cur_top.saturating_add(1).min(23), Ordering::Relaxed);
            }
            true
        }
        ButtonId::Right => {
            let cur = DAY_VIEW_TITLE_SCROLL.load(Ordering::Relaxed);
            let next = cur.saturating_add(TITLE_SCROLL_STEP).min(TITLE_SCROLL_MAX);
            DAY_VIEW_TITLE_SCROLL.store(next, Ordering::Relaxed);
            true
        }
        ButtonId::Left => {
            let cur = DAY_VIEW_TITLE_SCROLL.load(Ordering::Relaxed);
            DAY_VIEW_TITLE_SCROLL.store(cur.saturating_sub(TITLE_SCROLL_STEP), Ordering::Relaxed);
            true
        }
        ButtonId::Fire | ButtonId::Execute => {
            DAY_LIST_SCROLL.store(0, Ordering::Relaxed);
            MODE.store(MODE_DAY_LIST, Ordering::Relaxed);
            true
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
            // end of the day's events.  `DAY_CACHE_MAX` is the most the
            // day cache holds, so no day can list more than that.
            DAY_LIST_SCROLL.store(
                cur.saturating_add(1).min(super::DAY_CACHE_MAX as u8),
                Ordering::Relaxed,
            );
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

#[cfg(feature = "embassy-core")]
fn battery_pct() -> u8 {
    crate::fw::battery::read_pct()
}

#[cfg(not(feature = "embassy-core"))]
fn battery_pct() -> u8 {
    100
}

pub fn draw<D>(display: &mut D) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let bat = battery_pct();
    draw_frame(display, Some(("Calendar", &bat)), None)?;

    // Unread-PM envelope, same as the clock face carries.  The header
    // strip between the "Calendar" title and the battery icon is
    // otherwise empty, and the calendar is the screen the organizer
    // edition boots on — mail shouldn't need a trip to the clock to
    // notice.
    #[cfg(feature = "mesh")]
    super::alarm::draw_unread_badge(display, PM_BADGE_CX, PM_BADGE_CY)?;

    // Everything below reads the cursor day out of the shared cache,
    // which the ICS task refills from the file whenever the cursor
    // moves.  Nothing here walks a list of every event in the calendar,
    // which is what lets the file be arbitrarily large.
    let cursor = ensure_cursor();
    super::request_day(cursor.0, cursor.1, cursor.2);

    super::with_day_cache(|cache| {
        // A cache miss is transient — the task is already loading the
        // day and will signal a redraw.  Render the empty day rather
        // than a spinner; on a fast day-load the placeholder is never
        // even seen.  `loaded` distinguishes that from a real day that
        // simply has nothing on it, which the views must not confuse.
        let loaded = cache.date == cursor;
        let day: &[super::CachedEvent] = if loaded { cache.valid() } else { &[] };
        let overflow = if loaded { cache.overflow } else { 0 };

        match MODE.load(Ordering::Relaxed) {
            MODE_DAY_LIST => draw_day_list(display, cursor, day, overflow),
            MODE_DAY_DETAIL => draw_day_detail(display, cursor, day, loaded),
            MODE_ACTIVE => draw_grid(display, cursor, day, true),
            _ => draw_grid(display, cursor, day, false),
        }
    })
}

fn draw_grid<D>(
    display: &mut D,
    cursor: (u16, u8, u8),
    day: &[super::CachedEvent],
    active: bool,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
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
        draw_bold(
            display,
            name,
            Point::new(cx, WEEKDAY_STRIP_Y),
            weekday_style,
            centered,
        )?;
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
                draw_bold(
                    display,
                    &nbuf,
                    Point::new(cell_x + COL_W / 2, cell_y + ROW_H / 2 + 1),
                    MonoTextStyle::new(&FONT_6X10, fg),
                    centered,
                )?;

                // Has-events dot in the top-right corner.
                if super::day_has_events(cell_date.0, cell_date.1, cell_date.2) {
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
    let cursor_evs = day;

    let left = TextStyleBuilder::new()
        .baseline(Baseline::Middle)
        .alignment(Alignment::Left)
        .build();
    // Every event in the file shows on this grid, however big the file
    // is — but only the first N_ALARMS of them get an alarm slot, and
    // the rest can't ring.  Say so in red rather than letting a silent
    // alarm look like a working one.
    let dropped = super::events_dropped();
    if dropped > 0 {
        // Kept short on purpose: at 6 px/char the 152 px panel takes 25
        // characters, and `dropped` is a u16 — spelling out the total as
        // well would run off the edge on a big calendar.  The boot log
        // carries the full "N events, M armed, K beyond the slots".
        let mut warn: heapless::String<24> = heapless::String::new();
        let _ = core::fmt::write(&mut warn, format_args!("! {dropped} can't ring"));
        draw_bold(
            display,
            &warn,
            Point::new(76, FOOTER_Y_2),
            MonoTextStyle::new(&FONT_6X10, RED),
            centered,
        )?;
    }

    if cursor_evs.is_empty() {
        draw_bold(
            display,
            "(no events)",
            Point::new(76, FOOTER_Y),
            MonoTextStyle::new(&FONT_6X10, BLACK),
            centered,
        )?;
    } else {
        let ev0 = cursor_evs[0];
        let summary = ev0.summary();
        let mut row: heapless::String<ROW_BUF> = heapless::String::new();
        let _ = core::fmt::write(
            &mut row,
            format_args!("{:02}:{:02} {}", ev0.hour, ev0.minute, summary.as_str()),
        );
        draw_bold(
            display,
            &row,
            Point::new(4, FOOTER_Y),
            MonoTextStyle::new(&FONT_6X10, BLACK),
            left,
        )?;

        // Both want the second footer line; the missing-events warning
        // wins, since "+ 3 more" is recoverable by opening the day and
        // "20 events not loaded" isn't.
        if cursor_evs.len() > 1 && dropped == 0 {
            let mut more: heapless::String<24> = heapless::String::new();
            let _ = core::fmt::write(&mut more, format_args!("+ {} more", cursor_evs.len() - 1));
            draw_bold(
                display,
                &more,
                Point::new(4, FOOTER_Y_2),
                MonoTextStyle::new(&FONT_6X10, BLACK),
                left,
            )?;
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
fn draw_day_detail<D>(
    display: &mut D,
    cursor: (u16, u8, u8),
    day_evs: &[super::CachedEvent],
    loaded: bool,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
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
        // Only commit the resolved position once the cache actually
        // holds this day.  The first frame after the cursor moves can
        // render before the day cache has been refilled; storing then
        // would pin the view to a default hour and never auto-scroll to
        // the events once they arrive.
        //
        // The test is "is this day loaded", not "does it have events" —
        // an empty day is loaded, and leaving its sentinel unresolved
        // made the next Up/Down read 0xFF as hour 0 and jump the
        // timeline instead of stepping it.
        if loaded {
            DAY_VIEW_TOP_HOUR.store(top_hour, Ordering::Relaxed);
        }
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
    for ev in day_evs {
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
            let summary = ev.summary();
            // Apply the global title scroll offset.  Counted in
            // characters, not bytes: an accented title is UTF-8 here, so
            // a byte offset would land mid-sequence and blank the row.
            // Past the end the title renders as the bare time prefix,
            // which still tells the user what's where and is the cue to
            // press Execute back.
            let scroll = DAY_VIEW_TITLE_SCROLL.load(Ordering::Relaxed) as usize;
            let scrolled = scroll_chars(summary.as_str(), scroll);
            let mut row: heapless::String<ROW_BUF> = heapless::String::new();
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
    let below = day_evs
        .iter()
        .any(|ev| (ev.hour as i32 * 60 + ev.minute as i32) >= (tl_start_hour + HOURS_VISIBLE) * 60);
    if above {
        draw_bold(display, "^", Point::new(146, TL_TOP_Y + 4), arrow_style, centered)?;
    }
    if below {
        draw_bold(display, "v", Point::new(146, TL_BOT_Y - 4), arrow_style, centered)?;
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
fn draw_day_list<D>(
    display: &mut D,
    cursor: (u16, u8, u8),
    day_evs: &[super::CachedEvent],
    overflow: u8,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
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
    // The cache holds exactly the cursor day, already sorted by start
    // time.

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
        let summary = ev.summary();
        let mut row: heapless::String<ROW_BUF> = heapless::String::new();
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
        draw_bold(display, "^", Point::new(146, ROW_TOP_Y + 4), arrow_style, centered)?;
    }
    if (scroll + ROWS_VISIBLE) < day_evs.len() as i32 {
        draw_bold(display, "v", Point::new(146, ROW_BOT_Y - 4), arrow_style, centered)?;
    }

    // A day busier than the cache can hold says so rather than silently
    // showing a subset. Red, like the truncated-import warning on the
    // grid — both mean "there is more than this".
    if overflow > 0 {
        let mut warn: heapless::String<24> = heapless::String::new();
        let _ = core::fmt::write(&mut warn, format_args!("+{overflow} more today"));
        draw_bold(
            display,
            &warn,
            // Centred FONT_6X10 spans 4 px above and 5 below its anchor,
            // so 146 is the lowest baseline that stays inside the 152-row
            // panel.  Anything lower clipped the message that exists to
            // say events are hidden.
            Point::new(76, OVERFLOW_NOTE_Y),
            MonoTextStyle::new(&FONT_6X10, RED),
            centered,
        )?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The header strip is shared by the title, the unread-PM envelope
    /// and the battery icon, all positioned by hand.  Pin the gaps so a
    /// later layout tweak can't quietly overlap them.
    #[test]
    fn pm_badge_clears_the_title_and_battery() {
        // `draw_frame` puts the title at x=4 in a 7 px/char font and the
        // battery icon at x=128.
        const TITLE_X: i32 = 4;
        const TITLE_END: i32 = TITLE_X + 7 * "Calendar".len() as i32;
        const BATTERY_X: i32 = 128;
        /// Envelope glyph, drawn centred.
        const BADGE_W: i32 = 13;
        /// Widest `+N` suffix: 2 chars at 6 px, offset 8 px from centre.
        const SUFFIX_END: i32 = 8 + 2 * 6;

        assert!(
            PM_BADGE_CX - BADGE_W / 2 > TITLE_END,
            "envelope overlaps the title"
        );
        assert!(
            PM_BADGE_CX + SUFFIX_END < BATTERY_X,
            "unread count overlaps the battery icon"
        );
        // Title baseline is y=14 in a 13 px font, so the header band is
        // roughly y=1..15; the 13 px envelope has to sit inside it.
        assert!(PM_BADGE_CY - BADGE_W / 2 >= 1);
        assert!(PM_BADGE_CY + BADGE_W / 2 <= 15);
    }

    /// An event row is `HH:MM-HH:MM ` plus the label.  Labels are stored
    /// as Latin-1 (one byte per character) but rendered from UTF-8, so a
    /// full-length accented title doubles in size — and `heapless`
    /// `push_str` is all-or-nothing, so a row buffer one byte too small
    /// drops the entire title rather than clipping it.
    #[test]
    fn row_buffer_holds_a_full_length_accented_title() {
        use core::fmt::Write;

        let worst = super::super::CachedEvent {
            hour: 23,
            minute: 59,
            end_hour: 23,
            end_minute: 59,
            summary: [0xc6; super::super::ics::SUMMARY_LEN], // 31 x 'Æ'
        };
        let label = worst.summary();
        assert_eq!(label.chars().count(), super::super::ics::SUMMARY_LEN);
        assert_eq!(label.len(), super::super::ics::SUMMARY_LEN * 2, "two bytes each");

        let mut row: heapless::String<ROW_BUF> = heapless::String::new();
        write!(
            row,
            "{:02}:{:02}-{:02}:{:02} {}",
            worst.hour,
            worst.minute,
            worst.end_hour,
            worst.end_minute,
            label.as_str()
        )
        .expect("row buffer must hold the widest event row");
        assert!(row.ends_with('Æ'), "title survived: {row:?}");
    }

    /// Every button in every mode, in every order, from every starting
    /// state — asserting the invariants the renderers rely on after each
    /// press.
    ///
    /// The renderers index arrays and compute coordinates from this
    /// state with no clipping assertions, and `panic = "abort"`, so a
    /// state the dispatcher can reach but a renderer can't handle takes
    /// the badge out. A person holding a direction down generates
    /// exactly this: long unbroken runs of one button with no redraw in
    /// between.
    #[test]
    fn button_walk_never_leaves_an_invalid_state() {
        const BUTTONS: [ButtonId; 7] = [
            ButtonId::Cancel,
            ButtonId::Execute,
            ButtonId::Up,
            ButtonId::Down,
            ButtonId::Left,
            ButtonId::Right,
            ButtonId::Fire,
        ];

        fn check(seq: &str) {
            let mode = MODE.load(Ordering::Relaxed);
            assert!(
                matches!(
                    mode,
                    MODE_PASSIVE | MODE_ACTIVE | MODE_DAY_DETAIL | MODE_DAY_LIST
                ),
                "{seq}: mode {mode} is not one of the four"
            );

            // The cursor is fed to add_days, weekday_for and
            // days_from_civil, and rendered as "Mon 15 Jul 2026".
            let (y, m, d) = (
                CURSOR_YEAR.load(Ordering::Relaxed),
                CURSOR_MONTH.load(Ordering::Relaxed),
                CURSOR_DAY.load(Ordering::Relaxed),
            );
            if y != 0 {
                assert!((1..=12).contains(&m), "{seq}: month {m}");
                assert!(d >= 1 && d <= 31, "{seq}: day {d}");
                // Must be a real date — the month-grid header indexes
                // MONTH_ABBR[m-1] and DAY_NAMES_LONG[weekday].
                assert!(
                    fasttime::Date::from_ymd(y as i32, m, d).is_ok(),
                    "{seq}: {y}-{m}-{d} is not a real date"
                );
                assert!(
                    (weekday_for(y, m, d) as usize) < DAY_NAMES_LONG.len(),
                    "{seq}: weekday out of range"
                );
            }

            // Scroll offsets are used as slice indices and loop bounds.
            let top = DAY_VIEW_TOP_HOUR.load(Ordering::Relaxed);
            assert!(top <= 23 || top == 0xFF, "{seq}: top hour {top}");
            assert!(
                DAY_VIEW_TITLE_SCROLL.load(Ordering::Relaxed) <= TITLE_SCROLL_MAX,
                "{seq}: title scroll past its cap"
            );
            assert!(
                DAY_LIST_SCROLL.load(Ordering::Relaxed) as usize <= crate::watch::DAY_CACHE_MAX,
                "{seq}: day-list scroll past the cache"
            );
        }

        // Exhaustive over every ordering up to length 4 (2801 sequences),
        // from a known date near a month end so rollovers get hit.
        fn walk(depth: usize, seq: &mut std::string::String) {
            if depth == 0 {
                return;
            }
            for (name, btn) in [
                ("C", ButtonId::Cancel),
                ("E", ButtonId::Execute),
                ("U", ButtonId::Up),
                ("D", ButtonId::Down),
                ("L", ButtonId::Left),
                ("R", ButtonId::Right),
                ("F", ButtonId::Fire),
            ] {
                let saved = (
                    MODE.load(Ordering::Relaxed),
                    CURSOR_YEAR.load(Ordering::Relaxed),
                    CURSOR_MONTH.load(Ordering::Relaxed),
                    CURSOR_DAY.load(Ordering::Relaxed),
                    DAY_VIEW_TOP_HOUR.load(Ordering::Relaxed),
                    DAY_VIEW_TITLE_SCROLL.load(Ordering::Relaxed),
                    DAY_LIST_SCROLL.load(Ordering::Relaxed),
                );

                seq.push_str(name);
                dispatch(btn);
                check(seq);
                walk(depth - 1, seq);
                seq.pop();

                MODE.store(saved.0, Ordering::Relaxed);
                CURSOR_YEAR.store(saved.1, Ordering::Relaxed);
                CURSOR_MONTH.store(saved.2, Ordering::Relaxed);
                CURSOR_DAY.store(saved.3, Ordering::Relaxed);
                DAY_VIEW_TOP_HOUR.store(saved.4, Ordering::Relaxed);
                DAY_VIEW_TITLE_SCROLL.store(saved.5, Ordering::Relaxed);
                DAY_LIST_SCROLL.store(saved.6, Ordering::Relaxed);
            }
        }

        // Dates worth starting from: a month end, a leap day, a year end,
        // and the far end of February.
        for start in [
            (2026u16, 7u8, 31u8),
            (2028, 2, 29),
            (2026, 12, 31),
            (2027, 2, 28),
            (2026, 1, 1),
        ] {
            for mode in [MODE_PASSIVE, MODE_ACTIVE, MODE_DAY_DETAIL, MODE_DAY_LIST] {
                MODE.store(mode, Ordering::Relaxed);
                set_cursor(start);
                DAY_VIEW_TOP_HOUR.store(0xFF, Ordering::Relaxed);
                DAY_VIEW_TITLE_SCROLL.store(0, Ordering::Relaxed);
                DAY_LIST_SCROLL.store(0, Ordering::Relaxed);
                walk(4, &mut std::string::String::new());
            }
        }

        // And long unbroken runs, which is what holding a key produces.
        for btn in BUTTONS {
            for start in [(2026u16, 12u8, 31u8), (2028, 2, 29)] {
                for mode in [MODE_PASSIVE, MODE_ACTIVE, MODE_DAY_DETAIL, MODE_DAY_LIST] {
                    MODE.store(mode, Ordering::Relaxed);
                    set_cursor(start);
                    for i in 0..500 {
                        dispatch(btn);
                        check(&std::format!("hold x{i}"));
                    }
                }
            }
        }
    }

    /// The timeline scroll must never step from the unresolved sentinel.
    /// Doing so read `0xFF` as hour 0 — turning the next press into a
    /// jump to midnight — and overwrote the sentinel, so the view never
    /// auto-anchored on the day's events once they loaded.
    #[test]
    fn scroll_does_nothing_until_the_view_has_a_position() {
        DAY_VIEW_TOP_HOUR.store(0xFF, Ordering::Relaxed);
        assert_eq!(scrollable_top_hour(), None, "sentinel is not a position");

        // Up and Down must both leave the sentinel intact.
        for (name, btn) in [("Up", ButtonId::Up), ("Down", ButtonId::Down)] {
            assert!(dispatch_day_detail(btn), "the press is still consumed");
            assert_eq!(
                DAY_VIEW_TOP_HOUR.load(Ordering::Relaxed),
                0xFF,
                "{name} clobbered the sentinel"
            );
        }

        // Once the renderer has anchored the view, stepping works.
        DAY_VIEW_TOP_HOUR.store(9, Ordering::Relaxed);
        assert_eq!(scrollable_top_hour(), Some(9));
        dispatch_day_detail(ButtonId::Down);
        assert_eq!(DAY_VIEW_TOP_HOUR.load(Ordering::Relaxed), 10);
        dispatch_day_detail(ButtonId::Up);
        assert_eq!(DAY_VIEW_TOP_HOUR.load(Ordering::Relaxed), 9);

        // And it still clamps at both ends of the day.
        DAY_VIEW_TOP_HOUR.store(0, Ordering::Relaxed);
        dispatch_day_detail(ButtonId::Up);
        assert_eq!(DAY_VIEW_TOP_HOUR.load(Ordering::Relaxed), 0);
        DAY_VIEW_TOP_HOUR.store(23, Ordering::Relaxed);
        dispatch_day_detail(ButtonId::Down);
        assert_eq!(DAY_VIEW_TOP_HOUR.load(Ordering::Relaxed), 23);
    }

    /// Text is anchored by its vertical middle, so a 10 px font reaches
    /// 4 px above its baseline and 5 below.  Every such anchor has to
    /// leave both ends inside the 152-row panel — the day-list overflow
    /// note used to sit 2 px too low and lose its bottom rows, which is
    /// a poor look for the one message that exists to say something is
    /// hidden.
    #[test]
    fn bottom_anchored_text_stays_on_the_panel() {
        const PANEL_H: i32 = 152;
        const FONT_H: i32 = 10;
        // embedded-graphics: baseline_offset(Middle) = (height - 1) / 2.
        const ABOVE: i32 = (FONT_H - 1) / 2;
        const BELOW: i32 = FONT_H - 1 - ABOVE;

        for (name, y) in [
            ("overflow note", OVERFLOW_NOTE_Y),
            ("footer line 1", FOOTER_Y),
            ("footer line 2", FOOTER_Y_2),
        ] {
            assert!(y - ABOVE >= 0, "{name} clipped at the top");
            assert!(
                y + BELOW <= PANEL_H - 1,
                "{name} clipped at the bottom: reaches row {}",
                y + BELOW
            );
        }
    }

    /// Footer strings are drawn at 6 px/char on a 152 px panel, so 25
    /// characters is the hard limit.  Both of these interpolate counts
    /// that can grow, which is exactly how text has run off this panel
    /// before.
    #[test]
    fn footer_warnings_fit_the_panel() {
        use core::fmt::Write;
        const MAX_CHARS: usize = 152 / 6;

        let mut warn: heapless::String<24> = heapless::String::new();
        let _ = write!(warn, "! {} can't ring", u16::MAX);
        assert!(
            warn.chars().count() <= MAX_CHARS,
            "ring warning too wide: {warn:?}"
        );

        let mut more: heapless::String<24> = heapless::String::new();
        let _ = write!(more, "+{} more today", u8::MAX);
        assert!(
            more.chars().count() <= MAX_CHARS,
            "overflow note too wide: {more:?}"
        );
    }

    #[test]
    fn scroll_chars_counts_characters_not_bytes() {
        assert_eq!(scroll_chars("Bornhack", 4), "hack");
        // Latin-1 letters are two UTF-8 bytes; a byte offset would land
        // mid-sequence here and lose the rest of the title.
        assert_eq!(scroll_chars("CyberÆgg", 5), "Ægg");
        assert_eq!(scroll_chars("SKÅL", 2), "ÅL");
        assert_eq!(scroll_chars("short", 99), "");
    }
}
