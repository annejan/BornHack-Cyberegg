//! Friends screen — pets met on the mesh "SHDW" channel.
//!
//! Opened from the Stats modal. Two internal states, mirroring
//! `battle_view`:
//! - **List**: a cursor-based menu, one row per friend, word-wrapped so
//!   long names/kind combos never overflow the 152px width (the old
//!   single-line "name (kind) - since" layout could run off-screen).
//!   Ordered most-recently-*seen*-first — `friends::FriendsList` moves a
//!   friend to the front every time a fresh beacon comes in (see
//!   `friends::on_pet_beacon`), so index 0 here is always whoever you
//!   last heard from. Up/Down moves the cursor, Fire opens the detail
//!   screen for the highlighted friend, Cancel/any other button closes
//!   the whole screen.
//! - **Detail**: full stats for one friend — kind, how long you've known
//!   them, how long since their last beacon, their cached combat-stat
//!   snapshot, and your head-to-head Battle record against them. That
//!   record is kept in sync between both badges — `battle_link` updates
//!   it through `friends::note_fight` on both sides for the same pair of
//!   device IDs. Any button returns to the list.

use core::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{PrimitiveStyle, Rectangle};
use embedded_graphics::text::{Text};

use crate::menu::ButtonId;
use crate::text_wrap::word_wrap;
use crate::ui::{self, TEXT_BLACK, TEXT_BOLD_BLACK, TEXT_BOLD_WHITE};
use crate::{BLACK, TriColor};

const STATE_LIST: u8 = 0;
const STATE_DETAIL: u8 = 1;

/// Characters per line at `FONT_7X13`(_BOLD) — 7px/glyph — leaving a
/// few px of margin on the 152px-wide screen.
const CHARS_PER_LINE: usize = 19;

const NO_FRIENDS: &str = "No friends met yet";
const DETAIL_TITLE: &str = "Friend Details";
const DETAIL_HINT: &str = "Fire for details";
const NO_SELECTION: &str = "No friend selected";
const BAR_LABELS: [&str; 4] = ["Pow", "Grd", "Spd", "HP"];

/// Fixed texts of the Friends screens (host-only, for the guide's word list).
#[cfg(any(test, feature = "simulator"))]
pub fn words() -> [&'static str; 13] {
    [
        NO_FRIENDS,
        DETAIL_TITLE,
        DETAIL_HINT,
        BAR_LABELS[0],
        BAR_LABELS[1],
        BAR_LABELS[2],
        BAR_LABELS[3],
        "Met <d>d <h>h ago",
        "Last seen <d>d <h>h ago",
        "Battles: <w>W-<l>L",
        "Back",
        NO_SELECTION,
        "<n>/<m>",
    ]
}

static ACTIVE: AtomicBool = AtomicBool::new(false);
static STATE: AtomicU8 = AtomicU8::new(STATE_LIST);
static CURSOR: AtomicU8 = AtomicU8::new(0);

pub fn is_active() -> bool {
    ACTIVE.load(Ordering::Relaxed)
}

pub fn open() {
    STATE.store(STATE_LIST, Ordering::Relaxed);
    CURSOR.store(0, Ordering::Relaxed);
    ACTIVE.store(true, Ordering::Relaxed);
}

pub fn close() {
    ACTIVE.store(false, Ordering::Relaxed);
}

/// Route a button press while the Friends screen is active. Owns its
/// own input logic across the two sub-states (mirrors `battle_view`).
pub fn handle_input(btn: ButtonId) {
    if STATE.load(Ordering::Relaxed) == STATE_DETAIL {
        // Any button steps back to the list, not all the way out.
        STATE.store(STATE_LIST, Ordering::Relaxed);
        return;
    }

    match btn {
        ButtonId::Up => super::friends::cursor_prev(&CURSOR),
        ButtonId::Down => super::friends::cursor_next(&CURSOR),
        // Fire or Execute opens the detail view — accept both so Execute
        // doesn't silently close the list instead of confirming.
        ButtonId::Fire | ButtonId::Execute => {
            if super::friends::count() > 0 {
                STATE.store(STATE_DETAIL, Ordering::Relaxed);
            }
        }
        _ => close(),
    }
}

/// Draw up to 2 word-wrapped lines of `label` starting at `(x, y)`,
/// 13px apart. Returns the y just past the last line drawn.
fn draw_wrapped<D>(
    display: &mut D,
    label: &str,
    x: i32,
    y: i32,
    style: embedded_graphics::mono_font::MonoTextStyle<'static, TriColor>,
) -> Result<i32, D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let left = crate::ui::TOP_LEFT;
    let bytes = label.as_bytes();
    let mut cursor_y = y;
    for (s, e) in word_wrap(bytes, CHARS_PER_LINE).iter().take(2) {
        let slice = core::str::from_utf8(&bytes[*s as usize..*e as usize]).unwrap_or("");
        crate::ui::draw_text(display, slice, Point::new(x, cursor_y), style, left)?;
        cursor_y += 13;
    }
    Ok(cursor_y)
}

pub fn draw<D>(display: &mut D) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    crate::ui::clear_screen(display)?;

    if STATE.load(Ordering::Relaxed) == STATE_DETAIL {
        draw_detail(display)
    } else {
        draw_list(display)
    }
}

fn draw_list<D>(display: &mut D) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    ui::draw_title_bar(display, "Friends", Point::zero(), 152)?;

    let count = super::friends::count();
    if count == 0 {
        ui::draw_centered_message(display, NO_FRIENDS, Point::new(76, 85))?;
        return Ok(());
    }

    let cursor = CURSOR.load(Ordering::Relaxed) as usize;

    // Fixed 3-row page window — each row reserves space for up to 2
    // word-wrapped text lines, so long name/kind combos never overflow.
    const PAGE: usize = 3;
    const ROW_H: i32 = 30;
    let viewport_start = (cursor / PAGE) * PAGE;
    let visible = PAGE.min(count as usize - viewport_start);

    for i in 0..visible {
        let idx = viewport_start + i;
        let Some(friend) = super::friends::get(idx) else {
            break;
        };
        let is_selected = idx == cursor;
        let y = 22 + i as i32 * ROW_H;

        if is_selected {
            Rectangle::new(Point::new(2, y - 1), Size::new(148, ROW_H as u32 - 2))
                .into_styled(PrimitiveStyle::with_fill(BLACK))
                .draw(display)?;
        }

        let label = friend.label();
        let style = if is_selected {
            TEXT_BOLD_WHITE
        } else {
            TEXT_BOLD_BLACK
        };
        draw_wrapped(display, label.as_str(), 6, y + 2, style)?;
    }

    // Scroll indicator (bottom-right) + hint (bottom-left).
    if count as usize > PAGE {
        let mut indicator: heapless::String<8> = heapless::String::new();
        let _ =
            core::fmt::Write::write_fmt(&mut indicator, format_args!("{}/{}", cursor + 1, count));
        let right = crate::ui::BOTTOM_RIGHT;
        Text::with_text_style(indicator.as_str(), Point::new(148, 150), TEXT_BLACK, right)
            .draw(display)?;
    }

    let left_hint = crate::ui::BOTTOM_LEFT;
    Text::with_text_style(
        DETAIL_HINT,
        Point::new(4, 150),
        TEXT_BLACK,
        left_hint,
    )
    .draw(display)?;

    Ok(())
}

/// The HP bar's fill: the wire range `WIRE_HP` (15..=120) mapped to 0..=100.
fn hp_bar_pct(hp: u8) -> u8 {
    let (lo, hi) = super::engine::combat::WIRE_HP;
    let span = (hi - lo) as u32;
    ((hp.saturating_sub(lo) as u32 * 100) / span).min(100) as u8
}

fn draw_detail<D>(display: &mut D) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    ui::draw_title_bar(display, DETAIL_TITLE, Point::zero(), 152)?;

    let idx = CURSOR.load(Ordering::Relaxed) as usize;
    let Some(friend) = super::friends::get(idx) else {
        ui::draw_centered_message(display, NO_SELECTION, Point::new(76, 85))?;
        return Ok(());
    };

    let left = crate::ui::TOP_LEFT;
    let label = friend.label();

    let mut y = draw_wrapped(display, label.as_str(), 6, 22, TEXT_BOLD_BLACK)?;
    y += 2;

    let now = super::lifecycle::now_tick();
    let mut met: heapless::String<24> = heapless::String::new();
    let _ = core::fmt::Write::write_fmt(
        &mut met,
        format_args!(
            "Met {} ago",
            super::engine::fmt_days_hours(now.saturating_sub(friend.first_seen_tick))
        ),
    );
    Text::with_text_style(met.as_str(), Point::new(6, y), TEXT_BLACK, left).draw(display)?;
    y += 13;

    let mut seen: heapless::String<24> = heapless::String::new();
    let _ = core::fmt::Write::write_fmt(
        &mut seen,
        format_args!(
            "Last seen {} ago",
            super::engine::fmt_days_hours(now.saturating_sub(friend.last_seen_tick))
        ),
    );
    Text::with_text_style(seen.as_str(), Point::new(6, y), TEXT_BLACK, left).draw(display)?;
    y += 13 + 2;

    // Cached combat numbers (power, guard, speed, hp) from the friend's
    // beacon. Power/guard/speed are shown as stored; hp (wire range
    // 15-120) is normalized to the same 0-100 scale purely for this bar,
    // not a raw stat value.
    const BAR_H: u32 = 12;
    const BAR_X: i32 = 36;
    let bars: [(&str, u8); 4] = [
        (BAR_LABELS[0], friend.power.min(100)),
        (BAR_LABELS[1], friend.guard.min(100)),
        (BAR_LABELS[2], friend.speed.min(100)),
        (
            BAR_LABELS[3],
            hp_bar_pct(friend.hp),
        ),
    ];
    for (label, pct) in bars {
        super::stat_bar::draw_stat_bar(
            display,
            label,
            pct,
            Point::new(6, y + 1),
            Point::new(BAR_X, y),
            Size::new(106, BAR_H),
            BLACK,
        )?;
        y += 14;
    }

    // Bottom row: Battle record (left) + close hint (right), sharing one
    // line like the list screen's indicator+hint row — keeps this screen
    // from needing yet another line of vertical space.
    let mut record: heapless::String<24> = heapless::String::new();
    let _ = core::fmt::Write::write_fmt(
        &mut record,
        format_args!("Battles: {}W-{}L", friend.wins, friend.losses),
    );
    let bottom_left = crate::ui::BOTTOM_LEFT;
    Text::with_text_style(record.as_str(), Point::new(6, 150), TEXT_BOLD_BLACK, bottom_left)
        .draw(display)?;

    let bottom_right = crate::ui::BOTTOM_RIGHT;
    Text::with_text_style("Back", Point::new(148, 150), TEXT_BLACK, bottom_right).draw(display)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hp_bar_spans_the_wire_range() {
        assert_eq!(hp_bar_pct(15), 0);
        assert_eq!(hp_bar_pct(120), 100);
        assert_eq!(hp_bar_pct(255), 100);
        assert_eq!(hp_bar_pct(67), 49);
    }

    #[test]
    fn bar_labels_fit_and_name_the_snapshot_fields() {
        assert_eq!(BAR_LABELS, ["Pow", "Grd", "Spd", "HP"]);
        assert!(BAR_LABELS.iter().all(|l| l.len() <= 8));
    }
}
