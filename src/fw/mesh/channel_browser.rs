//! On-device channel message browser.
//!
//! Replaces the single-message channel screen with a two-level browser:
//! 1. Channel list — scrollable list of active channels
//! 2. Channel view — last 2-3 messages in the selected channel + Reply

use core::cell::RefCell;
use core::sync::atomic::Ordering::Relaxed;

use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::mono_font::iso_8859_1::FONT_7X13_BOLD;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{PrimitiveStyle, Rectangle};
use embedded_graphics::text::{Alignment, Baseline, Text, TextStyleBuilder};

use super::meshcore::truncate_bytes;
use crate::menu::ButtonId;
use crate::{BLACK, TriColor, WHITE};

// ── State ────────────────────────────────────────────────────────────────────

#[derive(Clone, Copy)]
enum BrowserState {
    ChannelList {
        cursor: u8,
    },
    /// Channel message browser.  `anchor` identifies which message sits
    /// at the bottom of the visible body:
    ///
    /// * `None` → pinned to the newest message.  New arrivals stay visible at
    ///   the bottom; older messages flow upward.  This is the default when the
    ///   view is opened.
    /// * `Some(content_hash)` → anchored on a specific message.  The anchor
    ///   survives eviction of *other* messages — the user's place is preserved.
    ///   If the anchor itself is evicted from the ring, the renderer falls back
    ///   to the oldest available message in the channel (the closest
    ///   still-known position to where the user was reading).
    ChannelView {
        channel_idx: u8,
        anchor: Option<u32>,
    },
}

pub struct ChannelBrowser {
    state: BrowserState,
}

pub static BROWSER: Mutex<CriticalSectionRawMutex, RefCell<ChannelBrowser>> =
    Mutex::new(RefCell::new(ChannelBrowser {
        state: BrowserState::ChannelList { cursor: 0 },
    }));

/// Reset the browser to the channel list (called when navigating to the
/// screen).
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
                    } else {
                        // Wrap to the last channel instead of doing
                        // nothing at the top.
                        let count = crate::CACHED_CHANNELS.lock(|c| c.borrow().len() as u8);
                        if count > 0 {
                            b.state = BrowserState::ChannelList { cursor: count - 1 };
                        }
                    }
                }
                ButtonId::Down => {
                    let count = crate::CACHED_CHANNELS.lock(|c| c.borrow().len() as u8);
                    if cursor + 1 < count {
                        b.state = BrowserState::ChannelList { cursor: cursor + 1 };
                    } else if count > 0 {
                        // Wrap to the top instead of doing nothing at the
                        // bottom.
                        b.state = BrowserState::ChannelList { cursor: 0 };
                    }
                }
                ButtonId::Execute | ButtonId::Fire => {
                    let ch_idx = crate::CACHED_CHANNELS
                        .lock(|c| c.borrow().get(cursor as usize).map(|ch| ch.slot_idx));
                    if let Some(idx) = ch_idx {
                        b.state = BrowserState::ChannelView {
                            channel_idx: idx,
                            anchor: None, // open at newest
                        };
                    }
                }
                ButtonId::Cancel | ButtonId::Left | ButtonId::Right => return true,
            },
            BrowserState::ChannelView {
                channel_idx,
                anchor,
            } => match btn {
                ButtonId::Up => {
                    // Up = older messages.  Resolve the current anchor
                    // to an index, step one back, and store the new
                    // neighbour's content_hash as the new anchor.
                    let (_total, anchor_idx) = resolve_anchor(channel_idx, anchor);
                    if anchor_idx > 0 {
                        let new_anchor = hash_at(channel_idx, anchor_idx - 1);
                        b.state = BrowserState::ChannelView {
                            channel_idx,
                            anchor: new_anchor,
                        };
                    }
                }
                ButtonId::Down => {
                    // Down = newer messages.  When the step lands on
                    // the newest message we re-pin to `None` so the
                    // view follows future arrivals.
                    let (total, anchor_idx) = resolve_anchor(channel_idx, anchor);
                    if total > 0 && anchor_idx + 1 < total {
                        let new_idx = anchor_idx + 1;
                        let new_anchor = if new_idx == total - 1 {
                            None
                        } else {
                            hash_at(channel_idx, new_idx)
                        };
                        b.state = BrowserState::ChannelView {
                            channel_idx,
                            anchor: new_anchor,
                        };
                    }
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
    static REPLY_CHANNEL_IDX: core::sync::atomic::AtomicU8 = core::sync::atomic::AtomicU8::new(0);
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

/// Resolve `anchor` to a position in the channel's oldest→newest
/// message list.  Returns `(total_messages, anchor_index)`.
///
/// The fallback rules let the view tolerate ring eviction:
/// * `None`                       → newest message (`total - 1`).
/// * `Some(h)` and hash present   → that message's index.
/// * `Some(h)` and hash *missing* → `0` (oldest still-available), i.e. the
///   closest known position to where the user was reading.
fn resolve_anchor(channel_idx: u8, anchor: Option<u32>) -> (usize, usize) {
    crate::CHANNEL_MSG_RING.lock(|cell| {
        let ring = cell.borrow();
        let mut total = 0usize;
        let mut found_at: Option<usize> = None;
        for entry in ring.iter() {
            if entry.channel_idx != channel_idx {
                continue;
            }
            if let Some(h) = anchor
                && found_at.is_none()
                && entry.content_hash == h
            {
                found_at = Some(total);
            }
            total += 1;
        }
        let anchor_idx = if total == 0 {
            0
        } else {
            match (anchor, found_at) {
                (None, _) => total - 1,
                (Some(_), Some(i)) => i,
                (Some(_), None) => 0,
            }
        };
        (total, anchor_idx)
    })
}

/// Look up the `content_hash` of the message at `index` in the channel's
/// oldest→newest list.  Returns `None` if `index` is out of range or
/// the ring changed under us.
fn hash_at(channel_idx: u8, index: usize) -> Option<u32> {
    crate::CHANNEL_MSG_RING.lock(|cell| {
        cell.borrow()
            .iter()
            .filter(|m| m.channel_idx == channel_idx)
            .nth(index)
            .map(|m| m.content_hash)
    })
}

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
                anchor,
            } => draw_channel_view(display, channel_idx, anchor),
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
            cell.borrow()
                .iter()
                .filter(|m| m.channel_idx == ch_idx)
                .count()
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
    anchor: Option<u32>,
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

    // Collect every message for this channel from oldest → newest,
    // including the content_hash so we can resolve the anchor below.
    type MsgEntry = (
        heapless::String<16>,
        heapless::String<{ crate::CHANNEL_MSG_TEXT_MAX }>,
        bool, // is_own
        u8,   // repeat_count
        u32,  // content_hash — used to locate the anchor message
    );
    let mut msgs: heapless::Vec<MsgEntry, { crate::CHANNEL_MSG_RING_SIZE }> = heapless::Vec::new();
    crate::CHANNEL_MSG_RING.lock(|cell| {
        for entry in cell.borrow().iter() {
            if entry.channel_idx == channel_idx {
                let _ = msgs.push((
                    entry.sender.clone(),
                    entry.text.clone(),
                    entry.is_own,
                    entry.repeat_count,
                    entry.content_hash,
                ));
            }
        }
    });
    let msg_total = msgs.len();

    // Header: channel name (capped at 16 chars) + message count
    let mut header: heapless::String<24> = heapless::String::new();
    let name_cap = truncate_bytes(ch_name.as_str(), 16);
    let _ = core::fmt::Write::write_fmt(&mut header, format_args!("{} ({})", name_cap, msg_total));
    Rectangle::new(Point::new(0, 0), Size::new(152, 16))
        .into_styled(PrimitiveStyle::with_fill(BLACK))
        .draw(display)?;
    Text::with_text_style(&header, Point::new(76, 2), FONT_INV, center).draw(display)?;

    let chars_per_line = 20usize;
    let max_lines = 8usize;
    // Body width is 148 px (152 minus a 4 px right margin reserved for
    // the scroll bar) — text wraps at chars_per_line so this isn't
    // currently used for layout, just kept as a visual guideline.

    if msgs.is_empty() {
        Text::with_text_style("No messages", Point::new(76, 60), FONT, center).draw(display)?;
    } else {
        // Resolve the anchor to an index in the freshly-collected list.
        // Eviction handling:
        //   * `None`                     → newest (msg_total - 1)
        //   * `Some(h)` and hash present → that message's position
        //   * `Some(h)` and hash missing → oldest available (0), the closest known
        //     position to where the user was reading
        let anchor_idx = match anchor {
            None => msg_total - 1,
            Some(h) => msgs.iter().position(|m| m.4 == h).unwrap_or(0),
        };

        // Lines required by each message: 1 (sender header) + the
        // wrapped-text line count.  Wrapping uses the shared
        // word-aware breaker so embedded `\n` produces real line
        // breaks and word boundaries are respected.
        let mut lines_per: heapless::Vec<usize, { crate::CHANNEL_MSG_RING_SIZE }> =
            heapless::Vec::new();
        for (_, text, ..) in msgs.iter() {
            let text_lines = crate::text_wrap::word_wrap(text.as_bytes(), chars_per_line).len();
            let _ = lines_per.push(1 + text_lines);
        }

        // Two-phase window expansion around the anchor.
        //
        // Phase 1 (backward): greedily prepend older messages above the
        // anchor — preserves the chat-style "anchor at the bottom"
        // feel for the common case.
        //
        // Phase 2 (forward): if the screen still has room (typically
        // when the anchor is the oldest available message), append
        // newer messages below the anchor to fill the body.  Without
        // this phase, scrolling all the way back leaves the screen
        // empty save for one message.
        //
        // The anchor itself is always counted first so even an
        // overflowing single message is rendered (clipped at the top).
        let mut first_visible = anchor_idx;
        let mut last_visible = anchor_idx;
        let mut total_lines = lines_per[anchor_idx];
        while first_visible > 0 {
            let needed = lines_per[first_visible - 1];
            if total_lines + needed > max_lines {
                break;
            }
            first_visible -= 1;
            total_lines += needed;
        }
        while last_visible + 1 < msg_total {
            let needed = lines_per[last_visible + 1];
            if total_lines + needed > max_lines {
                break;
            }
            last_visible += 1;
            total_lines += needed;
        }

        // Render first_visible → last_visible (oldest to newest in window).
        let mut y = 20i32;
        for i in first_visible..=last_visible {
            let (ref sender, ref text, is_own, repeat_count, _hash) = msgs[i];

            // Sender line — inverted (white on black)
            let nick: heapless::String<24> = if is_own {
                let mut lbl: heapless::String<24> = heapless::String::new();
                if repeat_count > 0 {
                    let digit = if repeat_count > 9 {
                        b'9'
                    } else {
                        b'0' + repeat_count
                    };
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
            let nick_w = (nick.len() as u32 * 7).min(144) + 4;
            Rectangle::new(Point::new(2, y), Size::new(nick_w, LH as u32))
                .into_styled(PrimitiveStyle::with_fill(BLACK))
                .draw(display)?;
            Text::with_text_style(&nick, Point::new(4, y + 1), FONT_INV, left).draw(display)?;
            y += LH;

            // Message text — word-aware wrap with newline support.  Renders
            // via `emoji::draw_string` so emoji codepoints in messages
            // come out as monochrome icons instead of the font's
            // missing-glyph indicator.  `text_wrap::word_wrap` already
            // counts emoji codepoints as 2 cells so line breaks land
            // on the right column.
            let text_bytes = text.as_bytes();
            for (s, e) in crate::text_wrap::word_wrap(text_bytes, chars_per_line) {
                let slice = core::str::from_utf8(&text_bytes[s as usize..e as usize]).unwrap_or("");
                crate::fw::emoji::draw_string(display, slice, Point::new(8, y), FONT, left)?;
                y += LH;
            }
        }

        // Scroll bar on the right edge of the body — only when there's
        // something off-screen (above or below).
        let visible = last_visible + 1 - first_visible;
        if visible < msg_total {
            const TRACK_X: i32 = 149;
            const TRACK_W: u32 = 2;
            const TRACK_Y: i32 = 20;
            const TRACK_H: i32 = 120; // body region: y=20..140
            // Track outline so the thumb has something to sit in.
            Rectangle::new(
                Point::new(TRACK_X, TRACK_Y),
                Size::new(TRACK_W, TRACK_H as u32),
            )
            .into_styled(PrimitiveStyle::with_stroke(BLACK, 1))
            .draw(display)?;
            // Thumb size proportional to visible window.
            let thumb_h = ((visible as i32 * TRACK_H) / msg_total as i32).max(8);
            // Thumb position: anchor at the newest message → thumb at
            // the bottom; older anchor → thumb moves up.  Equivalent to
            // the old (max_scroll - scroll) form, with the offset
            // recomputed from the anchor's position.
            let max_scroll = msg_total.saturating_sub(1) as i32;
            let scroll_i = max_scroll - anchor_idx as i32;
            let travel = TRACK_H - thumb_h;
            let thumb_y = if max_scroll == 0 {
                TRACK_Y
            } else {
                TRACK_Y + travel - (scroll_i * travel) / max_scroll
            };
            Rectangle::new(
                Point::new(TRACK_X, thumb_y),
                Size::new(TRACK_W, thumb_h as u32),
            )
            .into_styled(PrimitiveStyle::with_fill(BLACK))
            .draw(display)?;
        }
    }

    // Reply hint at bottom
    Rectangle::new(Point::new(0, 140), Size::new(152, 12))
        .into_styled(PrimitiveStyle::with_fill(BLACK))
        .draw(display)?;
    Text::with_text_style("Fire: Reply", Point::new(76, 140), FONT_INV, center).draw(display)?;

    Ok(())
}
