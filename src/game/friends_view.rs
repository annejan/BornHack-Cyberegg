//! Friends screen — pets met on the mesh "SHDW" channel.
//!
//! Opened from the Stats modal. Two internal states, mirroring
//! `battle_view`:
//! - **List**: a cursor-based menu, one row per friend, word-wrapped so
//!   long names/kind combos never overflow the 152px width (the old
//!   single-line "name (kind) - since" layout could run off-screen).
//!   Up/Down moves the cursor, Fire opens the detail screen for the
//!   highlighted friend, Cancel/any other button closes the whole
//!   screen.
//! - **Detail**: full stats for one friend — kind, how long you've known
//!   them, their cached combat-stat snapshot, and your head-to-head
//!   Battle record against them. That record is kept in sync between
//!   both badges — see `battle::challenge` (challenger's side) and
//!   `battle::on_battle_result` (target's side), which both update
//!   `friends::record_battle_vs` for the same pair of device IDs. Any
//!   button returns to the list.

use core::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{PrimitiveStyle, Rectangle};
use embedded_graphics::text::{Alignment, Baseline, Text, TextStyleBuilder};

use crate::menu::ButtonId;
use crate::text_wrap::word_wrap;
use crate::ui::{self, TEXT_BLACK, TEXT_BOLD_BLACK, TEXT_BOLD_WHITE};
use crate::{BLACK, TriColor, WHITE};

const STATE_LIST: u8 = 0;
const STATE_DETAIL: u8 = 1;

/// Characters per line at `FONT_7X13`(_BOLD) — 7px/glyph — leaving a
/// few px of margin on the 152px-wide screen.
const CHARS_PER_LINE: usize = 19;

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

fn cursor_up() {
    let c = CURSOR.load(Ordering::Relaxed);
    if c > 0 {
        CURSOR.store(c - 1, Ordering::Relaxed);
    }
}

fn cursor_down() {
    let count = super::friends::count();
    let c = CURSOR.load(Ordering::Relaxed);
    if count > 0 && c + 1 < count {
        CURSOR.store(c + 1, Ordering::Relaxed);
    }
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
        ButtonId::Up => cursor_up(),
        ButtonId::Down => cursor_down(),
        ButtonId::Fire => {
            if super::friends::count() > 0 {
                STATE.store(STATE_DETAIL, Ordering::Relaxed);
            }
        }
        _ => close(),
    }
}

/// Format ticks-since-first-met as "Xd Xh" (1 tick = 10s, same convention
/// as `PetRecord::age_str`).
fn since_str(ticks: u32) -> heapless::String<12> {
    let hours = ticks / 360;
    let days = hours / 24;
    let mut s = heapless::String::new();
    let _ = core::fmt::Write::write_fmt(&mut s, format_args!("{}d {}h", days, hours % 24));
    s
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
    let left = TextStyleBuilder::new().baseline(Baseline::Top).build();
    let bytes = label.as_bytes();
    let mut cursor_y = y;
    for (s, e) in word_wrap(bytes, CHARS_PER_LINE).iter().take(2) {
        let slice = core::str::from_utf8(&bytes[*s as usize..*e as usize]).unwrap_or("");
        Text::with_text_style(slice, Point::new(x, cursor_y), style, left).draw(display)?;
        cursor_y += 13;
    }
    Ok(cursor_y)
}

fn friend_label(name: &str, kind_name: &str) -> heapless::String<32> {
    let mut label = heapless::String::new();
    if !name.is_empty() {
        let _ = core::fmt::Write::write_fmt(&mut label, format_args!("{} ({})", name, kind_name));
    } else {
        let _ = core::fmt::Write::write_fmt(&mut label, format_args!("{}", kind_name));
    }
    label
}

pub fn draw<D>(display: &mut D) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    Rectangle::new(Point::zero(), Size::new(152, 152))
        .into_styled(PrimitiveStyle::with_fill(WHITE))
        .draw(display)?;

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
        ui::draw_centered_message(display, "No friends met yet", Point::new(76, 85))?;
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

        let kind_name = super::engine::PetKind::from_u8(friend.pet_kind).name();
        let label = friend_label(friend.name_str(), kind_name);
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
        let right = TextStyleBuilder::new()
            .baseline(Baseline::Bottom)
            .alignment(Alignment::Right)
            .build();
        Text::with_text_style(indicator.as_str(), Point::new(148, 150), TEXT_BLACK, right)
            .draw(display)?;
    }

    let left_hint = TextStyleBuilder::new().baseline(Baseline::Bottom).build();
    Text::with_text_style(
        "Fire for details",
        Point::new(4, 150),
        TEXT_BLACK,
        left_hint,
    )
    .draw(display)?;

    Ok(())
}

fn draw_detail<D>(display: &mut D) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    ui::draw_title_bar(display, "Friend Details", Point::zero(), 152)?;

    let idx = CURSOR.load(Ordering::Relaxed) as usize;
    let Some(friend) = super::friends::get(idx) else {
        ui::draw_centered_message(display, "No friend selected", Point::new(76, 85))?;
        return Ok(());
    };

    let left = TextStyleBuilder::new().baseline(Baseline::Top).build();
    let kind_name = super::engine::PetKind::from_u8(friend.pet_kind).name();
    let label = friend_label(friend.name_str(), kind_name);

    let mut y = draw_wrapped(display, label.as_str(), 6, 22, TEXT_BOLD_BLACK)?;
    y += 4;

    let now = super::lifecycle::now_tick();
    let mut met: heapless::String<24> = heapless::String::new();
    let _ = core::fmt::Write::write_fmt(
        &mut met,
        format_args!(
            "Met {} ago",
            since_str(now.saturating_sub(friend.first_seen_tick))
        ),
    );
    Text::with_text_style(met.as_str(), Point::new(6, y), TEXT_BLACK, left).draw(display)?;
    y += 16;

    // Cached combat-stat snapshot — see `battle::CombatStats`. Attack/
    // Defense/Speed are already 1-100; HP (20-150) is normalized to the
    // same 0-100 scale purely for this bar, not a raw stat value.
    const BAR_H: u32 = 12;
    const BAR_X: i32 = 36;
    let bars: [(&str, u8); 4] = [
        ("Atk", friend.attack.min(100)),
        ("Def", friend.defense.min(100)),
        ("Spd", friend.speed.min(100)),
        (
            "HP",
            (((friend.max_hp.saturating_sub(20) as u32) * 100) / 130).min(100) as u8,
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
        y += 16;
    }
    y += 2;

    let mut record: heapless::String<24> = heapless::String::new();
    let _ = core::fmt::Write::write_fmt(
        &mut record,
        format_args!("Battles: {}W-{}L", friend.wins, friend.losses),
    );
    Text::with_text_style(record.as_str(), Point::new(6, y), TEXT_BOLD_BLACK, left)
        .draw(display)?;

    let hint = TextStyleBuilder::new()
        .baseline(Baseline::Bottom)
        .alignment(Alignment::Center)
        .build();
    Text::with_text_style(
        "Any button to go back",
        Point::new(76, 150),
        TEXT_BLACK,
        hint,
    )
    .draw(display)?;

    Ok(())
}
