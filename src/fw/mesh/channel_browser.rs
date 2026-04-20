//! On-device channel message browser.
//!
//! Replaces the single-message channel screen with a two-level browser:
//! 1. Channel list — scrollable list of active channels
//! 2. Channel view — last 2-3 messages in the selected channel + Reply

use core::cell::RefCell;
use core::sync::atomic::Ordering::Relaxed;

use super::meshcore::truncate_bytes;

use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;

use embedded_graphics::{
    mono_font::{MonoTextStyle, ascii::FONT_7X13_BOLD},
    prelude::*,
    primitives::{PrimitiveStyle, Rectangle},
    text::{Alignment, Baseline, Text, TextStyleBuilder},
};

use crate::menu::ButtonId;
use crate::{BLACK, TriColor, WHITE};

// ── State ────────────────────────────────────────────────────────────────────

#[derive(Clone, Copy)]
enum BrowserState {
    ChannelList { cursor: u8 },
    ChannelView { channel_idx: u8, scroll: u8 },
}

pub struct ChannelBrowser {
    state: BrowserState,
}

pub static BROWSER: Mutex<CriticalSectionRawMutex, RefCell<ChannelBrowser>> =
    Mutex::new(RefCell::new(ChannelBrowser {
        state: BrowserState::ChannelList { cursor: 0 },
    }));

/// Reset the browser to the channel list (called when navigating to the screen).
pub fn reset() {
    BROWSER.lock(|cell| {
        cell.borrow_mut().state = BrowserState::ChannelList { cursor: 0 };
    });
}

// ── Input dispatch ───────────────────────────────────────────────────────────

/// Handle a button press. Returns `true` if Cancel should propagate to the
/// menu layer (i.e., leave the channel screen entirely).
pub fn dispatch(btn: ButtonId) -> bool {
    // When BLE is connected, only allow navigation away from this screen.
    if crate::BLE_CONNECTED.load(Relaxed) {
        return matches!(btn, ButtonId::Cancel | ButtonId::Left | ButtonId::Right);
    }

    BROWSER.lock(|cell| {
        let mut b = cell.borrow_mut();
        match b.state {
            BrowserState::ChannelList { cursor } => match btn {
                ButtonId::Up => {
                    if cursor > 0 {
                        b.state = BrowserState::ChannelList { cursor: cursor - 1 };
                    }
                }
                ButtonId::Down => {
                    let count = crate::CACHED_CHANNELS.lock(|c| c.borrow().len() as u8);
                    if cursor + 1 < count {
                        b.state = BrowserState::ChannelList { cursor: cursor + 1 };
                    }
                }
                ButtonId::Execute | ButtonId::Fire => {
                    let ch_idx = crate::CACHED_CHANNELS.lock(|c| {
                        c.borrow().get(cursor as usize).map(|ch| ch.slot_idx)
                    });
                    if let Some(idx) = ch_idx {
                        b.state = BrowserState::ChannelView {
                            channel_idx: idx,
                            scroll: 0,
                        };
                    }
                }
                ButtonId::Cancel | ButtonId::Left | ButtonId::Right => return true,
            },
            BrowserState::ChannelView {
                channel_idx,
                scroll,
            } => match btn {
                ButtonId::Up => {
                    if scroll > 0 {
                        b.state = BrowserState::ChannelView {
                            channel_idx,
                            scroll: scroll - 1,
                        };
                    }
                }
                ButtonId::Down => {
                    b.state = BrowserState::ChannelView {
                        channel_idx,
                        scroll: scroll + 1,
                    };
                }
                ButtonId::Execute | ButtonId::Fire => {
                    let ci = channel_idx;
                    // Drop the borrow before calling text_entry (which may lock other statics).
                    drop(b);
                    start_reply(ci);
                    return false;
                }
                ButtonId::Cancel => {
                    b.state = BrowserState::ChannelList { cursor: 0 };
                }
                _ => {}
            },
        }
        false
    })
}

fn start_reply(channel_idx: u8) {
    static REPLY_CHANNEL_IDX: core::sync::atomic::AtomicU8 =
        core::sync::atomic::AtomicU8::new(0);
    REPLY_CHANNEL_IDX.store(channel_idx, Relaxed);

    fn on_reply_complete(text: &[u8]) {
        let ch = REPLY_CHANNEL_IDX.load(Relaxed);
        let ts = crate::unix_now().unwrap_or(0);
        let mut v: heapless::Vec<u8, { super::msg_queue::MAX_TEXT }> = heapless::Vec::new();
        let _ = v.extend_from_slice(&text[..text.len().min(super::msg_queue::MAX_TEXT)]);
        let _ = crate::tx_send(crate::TxRequest::ChannelMsg(crate::TxChannelMsg {
            channel_idx: ch,
            timestamp: ts,
            text: v,
        }));
    }

    crate::text_entry::begin(&[], 160, on_reply_complete, "Reply");
}

// ── Rendering ────────────────────────────────────────────────────────────────

const FONT: MonoTextStyle<'static, TriColor> = MonoTextStyle::new(&FONT_7X13_BOLD, BLACK);
const FONT_INV: MonoTextStyle<'static, TriColor> = MonoTextStyle::new(&FONT_7X13_BOLD, WHITE);
const LH: i32 = 14;

pub fn draw<D>(display: &mut D, bat_prc: &u8) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    if crate::BLE_CONNECTED.load(Relaxed) {
        return draw_ble_connected(display);
    }

    BROWSER.lock(|cell| {
        let b = cell.borrow();
        match b.state {
            BrowserState::ChannelList { cursor } => draw_channel_list(display, cursor, bat_prc),
            BrowserState::ChannelView {
                channel_idx,
                scroll,
            } => draw_channel_view(display, channel_idx, scroll),
        }
    })
}

fn draw_ble_connected<D>(display: &mut D) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let center = TextStyleBuilder::new()
        .baseline(Baseline::Middle)
        .alignment(Alignment::Center)
        .build();
    Text::with_text_style("Messages unavailable", Point::new(76, 60), FONT, center)
        .draw(display)?;
    Text::with_text_style("BLE client connected", Point::new(76, 80), FONT, center)
        .draw(display)?;
    Ok(())
}

fn draw_channel_list<D>(display: &mut D, cursor: u8, bat_prc: &u8) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let center = TextStyleBuilder::new()
        .baseline(Baseline::Top)
        .alignment(Alignment::Center)
        .build();
    let left = TextStyleBuilder::new()
        .baseline(Baseline::Top)
        .alignment(Alignment::Left)
        .build();

    crate::draw_header(display, "Channels", bat_prc)?;

    // Channel list — scrolling window of 9 visible rows
    let visible_rows = 9usize;
    let channels = crate::CACHED_CHANNELS.lock(|c| {
        let list = c.borrow();
        let mut out: heapless::Vec<(u8, heapless::String<20>), 40> = heapless::Vec::new();
        for ch in list.iter() {
            let _ = out.push((ch.slot_idx, ch.name.clone()));
        }
        out
    });

    if channels.is_empty() {
        Text::with_text_style("No channels", Point::new(76, 60), FONT, center).draw(display)?;
        return Ok(());
    }

    let scroll_start = if (cursor as usize) >= visible_rows {
        cursor as usize - visible_rows + 1
    } else {
        0
    };

    for i in 0..visible_rows {
        let idx = scroll_start + i;
        if idx >= channels.len() {
            break;
        }
        let y = 20 + i as i32 * LH;
        let is_sel = idx == cursor as usize;

        if is_sel {
            Rectangle::new(Point::new(0, y), Size::new(152, LH as u32))
                .into_styled(PrimitiveStyle::with_fill(BLACK))
                .draw(display)?;
        }

        // Show unread indicator: count messages in ring for this channel
        let ch_idx = channels[idx].0;
        let msg_count = crate::CHANNEL_MSG_RING.lock(|cell| {
            cell.borrow().iter().filter(|m| m.channel_idx == ch_idx).count()
        });

        let mut label: heapless::String<24> = heapless::String::new();
        let name = &channels[idx].1;
        let name_cap = truncate_bytes(name.as_str(), 16);
        let _ = label.push_str(name_cap);
        if msg_count > 0 {
            let _ = core::fmt::Write::write_fmt(&mut label, format_args!(" ({})", msg_count));
        }

        let font = if is_sel { FONT_INV } else { FONT };
        Text::with_text_style(&label, Point::new(4, y + 1), font, left).draw(display)?;
    }

    Ok(())
}

fn draw_channel_view<D>(
    display: &mut D,
    channel_idx: u8,
    _scroll: u8,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let center = TextStyleBuilder::new()
        .baseline(Baseline::Top)
        .alignment(Alignment::Center)
        .build();
    let left = TextStyleBuilder::new()
        .baseline(Baseline::Top)
        .alignment(Alignment::Left)
        .build();

    // Channel name header
    let ch_name = crate::CACHED_CHANNELS.lock(|c| {
        let list = c.borrow();
        list.iter()
            .find(|ch| ch.slot_idx == channel_idx)
            .map(|ch| ch.name.clone())
            .unwrap_or_else(|| {
                let mut s = heapless::String::new();
                let _ = s.push_str("???");
                s
            })
    });

    // Collect the last 16 messages for this channel.
    let mut msgs: heapless::Vec<(heapless::String<16>, heapless::String<{ crate::CHANNEL_MSG_TEXT_MAX }>, bool, u8), 16> =
        heapless::Vec::new();
    let mut msg_total: usize = 0;
    crate::CHANNEL_MSG_RING.lock(|cell| {
        for entry in cell.borrow().iter() {
            if entry.channel_idx == channel_idx {
                msg_total += 1;
                if msgs.is_full() {
                    msgs.remove(0);
                }
                let _ = msgs.push((
                    entry.sender.clone(),
                    entry.text.clone(),
                    entry.is_own,
                    entry.repeat_count,
                ));
            }
        }
    });

    // Header: channel name (capped at 16 chars) + message count
    let mut header: heapless::String<24> = heapless::String::new();
    let name_cap = truncate_bytes(ch_name.as_str(), 16);
    let _ = core::fmt::Write::write_fmt(
        &mut header,
        format_args!("{} ({})", name_cap, msg_total),
    );
    Rectangle::new(Point::new(0, 0), Size::new(152, 16))
        .into_styled(PrimitiveStyle::with_fill(BLACK))
        .draw(display)?;
    Text::with_text_style(&header, Point::new(76, 2), FONT_INV, center).draw(display)?;

    let chars_per_line = 20usize;
    let max_lines = 8usize;

    if msgs.is_empty() {
        Text::with_text_style("No messages", Point::new(76, 60), FONT, center).draw(display)?;
    } else {
        // Calculate lines per message (1 header + ceil(text_len / chars_per_line))
        // working backwards from newest, fitting as many as possible in max_lines.
        let candidate_count = msgs.len().min(4);
        let candidates = &msgs[msgs.len() - candidate_count..];

        // Count lines per candidate
        let mut lines_per: heapless::Vec<usize, 4> = heapless::Vec::new();
        for (_, text, _, _) in candidates.iter() {
            let text_lines = if text.is_empty() {
                0
            } else {
                (text.len() + chars_per_line - 1) / chars_per_line
            };
            let _ = lines_per.push(1 + text_lines); // 1 for header
        }

        // Walk backwards from newest, accumulating lines until we exceed max_lines.
        let mut total_lines = 0usize;
        let mut first_visible = candidates.len();
        for i in (0..candidates.len()).rev() {
            let needed = lines_per[i];
            if total_lines + needed > max_lines {
                break;
            }
            total_lines += needed;
            first_visible = i;
        }

        // Render from first_visible to end (oldest visible → newest).
        let mut y = 20i32;
        for i in first_visible..candidates.len() {
            let (ref sender, ref text, is_own, repeat_count) = candidates[i];

            // Sender line — inverted (white on black)
            let nick: heapless::String<24> = if is_own {
                let mut lbl: heapless::String<24> = heapless::String::new();
                if repeat_count > 0 {
                    let digit = if repeat_count > 9 { b'9' } else { b'0' + repeat_count };
                    let _ = core::fmt::Write::write_fmt(
                        &mut lbl,
                        format_args!("{} > You", digit as char),
                    );
                } else {
                    let _ = lbl.push_str("> You");
                }
                lbl
            } else {
                let mut lbl: heapless::String<24> = heapless::String::new();
                let s = sender.as_str();
                let _ = lbl.push_str(truncate_bytes(s, 16));
                lbl
            };
            let nick_w = (nick.len() as u32 * 7).min(148) + 4;
            Rectangle::new(Point::new(2, y), Size::new(nick_w, LH as u32))
                .into_styled(PrimitiveStyle::with_fill(BLACK))
                .draw(display)?;
            Text::with_text_style(&nick, Point::new(4, y + 1), FONT_INV, left)
                .draw(display)?;
            y += LH;

            // Message text — wrap on char boundaries
            let text_str = text.as_str();
            let mut offset = 0usize;
            while offset < text_str.len() {
                let mut end = text_str.len().min(offset + chars_per_line);
                while end > offset && !text_str.is_char_boundary(end) {
                    end -= 1;
                }
                if end == offset {
                    break;
                }
                Text::with_text_style(&text_str[offset..end], Point::new(8, y), FONT, left)
                    .draw(display)?;
                y += LH;
                offset = end;
            }
        }
    }

    // Reply hint at bottom
    Rectangle::new(Point::new(0, 140), Size::new(152, 12))
        .into_styled(PrimitiveStyle::with_fill(BLACK))
        .draw(display)?;
    Text::with_text_style("Fire: Reply", Point::new(76, 140), FONT_INV, center)
        .draw(display)?;

    Ok(())
}
