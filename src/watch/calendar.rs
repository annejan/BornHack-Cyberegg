//! Calendar screen — a chronologically-sorted list view of every enabled
//! one-shot alarm slot.  Sits in the icon grid right after the Watch
//! screen; reads the same alarm-slot state that fires the buzzer, so any
//! event you load via `ALARMS.ICS` automatically shows up here.
//!
//! No editing — this is purely "what's coming up?".  The Settings → Alarm
//! and Settings → Events submenus stay the place to tweak slot 0 / clear
//! imports.
//!
//! Buttons:
//!   * Up   — scroll up the list
//!   * Down — scroll down the list
//!   * Left / Right / Cancel / Fire / Execute — fall through (so the menu
//!     layer can do screen-nav etc.)

use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::mono_font::ascii::{FONT_6X10, FONT_7X13_BOLD};
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{PrimitiveStyle, Rectangle};
use embedded_graphics::text::{Alignment, Baseline, Text, TextStyleBuilder};

use super::alarm::{
    N_ALARMS, alarm_day_n, alarm_enabled_n, alarm_hour_n, alarm_is_one_shot_n, alarm_minute_n,
    alarm_month_n, alarm_year_n,
};
use crate::menu::ButtonId;
use crate::{BLACK, RED, TriColor, WHITE, draw_frame};

/// Scroll offset — index into the sorted-events list of the first row
/// rendered.  Wraps around at the list end so the screen never shows a
/// blank tail; if there are fewer events than rows, scrolling is a no-op.
static SCROLL: core::sync::atomic::AtomicU8 = core::sync::atomic::AtomicU8::new(0);

/// Maximum events the calendar display walks across.
const MAX_EVENTS: usize = N_ALARMS;
/// Rows of `FONT_7X13_BOLD` text we can fit between the header and the
/// bottom edge of the screen.  Header takes y=0..17, leaving ~134 px;
/// at 14 px per row that's 9 rows comfortably.
const ROWS_VISIBLE: usize = 9;
/// y-baseline of the top row.
const ROW_TOP_Y: i32 = 28;
/// Vertical step between consecutive rows.
const ROW_STEP_Y: i32 = 14;

#[derive(Clone, Copy, PartialEq, Eq)]
struct EventRow {
    year: u16,
    month: u8,
    day: u8,
    hour: u8,
    minute: u8,
}

/// Collect every enabled one-shot alarm slot into a fixed-size buffer and
/// sort by (year, month, day, hour, minute).  Returns the count.
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
        };
        n += 1;
    }
    // Insertion sort — small N, stable, avoids pulling in any sort code from
    // core that we don't already use.
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

/// Returns `(year, month, day)` from the wall clock if synced, else
/// `None`.  Used to highlight today's events on the list.
fn today() -> Option<(u16, u8, u8)> {
    let c = super::clock::wall_clock()?;
    Some((c.year, c.month, c.day))
}

/// Handle a button press while the Calendar screen is active.  Returns
/// `true` if consumed.  Only Up/Down are consumed (for list scrolling) —
/// Left/Right have to fall through so the menu layer can navigate to the
/// next/previous screen.  Otherwise Calendar becomes a dead end.
pub fn dispatch(btn: ButtonId) -> bool {
    use core::sync::atomic::Ordering;
    let cur = SCROLL.load(Ordering::Relaxed);
    let next = match btn {
        ButtonId::Up => cur.saturating_sub(1),
        ButtonId::Down => cur.saturating_add(1),
        _ => return false,
    };
    // Cap to avoid scrolling past the end (computed lazily — can't easily
    // know the live count from inside dispatch without re-collecting).
    // Renderer clamps anyway; keep `next` bounded by MAX_EVENTS just to
    // stop unbounded growth.
    SCROLL.store(next.min(MAX_EVENTS as u8), Ordering::Relaxed);
    true
}

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

    let mut events = [EventRow {
        year: 0,
        month: 0,
        day: 0,
        hour: 0,
        minute: 0,
    }; MAX_EVENTS];
    let n = collect_sorted(&mut events);

    if n == 0 {
        let centered = TextStyleBuilder::new()
            .baseline(Baseline::Middle)
            .alignment(Alignment::Center)
            .build();
        Text::with_text_style(
            "(no events)",
            Point::new(76, 80),
            MonoTextStyle::new(&FONT_7X13_BOLD, BLACK),
            centered,
        )
        .draw(display)?;
        return Ok(());
    }

    use core::sync::atomic::Ordering;
    // Clamp the scroll offset so we never render past the end of the list.
    let raw_scroll = SCROLL.load(Ordering::Relaxed) as usize;
    let max_scroll = n.saturating_sub(ROWS_VISIBLE);
    let scroll = raw_scroll.min(max_scroll);
    if raw_scroll != scroll {
        SCROLL.store(scroll as u8, Ordering::Relaxed);
    }

    let today_ymd = today();
    let row_style = MonoTextStyle::new(&FONT_7X13_BOLD, BLACK);
    let row_inv = MonoTextStyle::new(&FONT_7X13_BOLD, WHITE);
    let left = TextStyleBuilder::new()
        .baseline(Baseline::Middle)
        .alignment(Alignment::Left)
        .build();

    for i in 0..ROWS_VISIBLE.min(n - scroll) {
        let ev = events[scroll + i];
        let y = ROW_TOP_Y + (i as i32) * ROW_STEP_Y;

        let is_today = matches!(today_ymd, Some((ty, tm, td)) if ev.year == ty && ev.month == tm && ev.day == td);

        // Today's events get a red background bar with white text — same
        // treatment the watch face uses for the current weekday in the
        // bottom strip, so the visual idiom stays consistent.
        if is_today {
            Rectangle::new(Point::new(2, y - 7), Size::new(148, 13))
                .into_styled(PrimitiveStyle::with_fill(RED))
                .draw(display)?;
        }

        let mut buf: heapless::String<24> = heapless::String::new();
        let _ = core::fmt::write(
            &mut buf,
            format_args!(
                "{:02}-{:02}  {:02}:{:02}",
                ev.month, ev.day, ev.hour, ev.minute
            ),
        );

        Text::with_text_style(
            &buf,
            Point::new(6, y),
            if is_today { row_inv } else { row_style },
            left,
        )
        .draw(display)?;
    }

    // Scroll indicators on the right edge if there's content beyond what's
    // visible.
    let arrow_style = MonoTextStyle::new(&FONT_6X10, BLACK);
    let center = TextStyleBuilder::new()
        .baseline(Baseline::Middle)
        .alignment(Alignment::Center)
        .build();
    if scroll > 0 {
        Text::with_text_style("^", Point::new(146, ROW_TOP_Y), arrow_style, center)
            .draw(display)?;
    }
    if scroll + ROWS_VISIBLE < n {
        Text::with_text_style(
            "v",
            Point::new(146, ROW_TOP_Y + (ROWS_VISIBLE as i32 - 1) * ROW_STEP_Y),
            arrow_style,
            center,
        )
        .draw(display)?;
    }

    Ok(())
}
