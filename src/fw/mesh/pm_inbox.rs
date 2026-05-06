//! On-device PM (private message) inbox + per-peer threads.
//!
//! Replaces the old single-record `SCREEN_PM` rendering — same redesign
//! pattern we used for adverts → Contacts.  A small RAM ring (`INBOX`,
//! cap [`MAX_ENTRIES`]) holds recent PMs in both directions.  The
//! Contacts-screen popup feeds outgoing entries via [`note_outgoing`]
//! and the meshcore RX path feeds incoming via [`note_incoming`].
//!
//! ## State machine
//!
//! - `Inbox` — list of distinct peers (by `pub_key`), sorted by most- recent
//!   message, each row showing peer name + latest message preview.  Up/Down
//!   scrolls; Fire opens the thread.
//! - `Thread { pub_key }` — chronological history with that peer. Up/Down
//!   scrolls within long threads; Fire opens the reply keyboard via the
//!   existing [`contacts_screen::start_pm_compose`] flow.  Cancel returns to
//!   the Inbox.
//!
//! Per-peer "last read" tracking is intentionally not persisted —
//! `(N)` unread badges reset on reboot, which matches the discovery-
//! first design semantics elsewhere in the firmware.

use core::cell::RefCell;

use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;

/// Maximum cached PMs (incoming + outgoing combined).  Each entry is
/// ~256 B (text + name buffers), so 32 = ~8 KiB total.
pub const MAX_ENTRIES: usize = 32;

/// Per-PM direction.  Drives left/right alignment in the thread view
/// and disambiguates "from me" vs "to me" when both are present.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Incoming,
    Outgoing,
}

/// One PM in the inbox ring.
#[derive(Clone)]
pub struct PmEntry {
    /// The *peer* — sender for incoming, recipient for outgoing.
    pub pub_key: [u8; 32],
    pub direction: Direction,
    /// Message text — sized to MeshCore's `MAX_TXT_TEXT_SIZE` (130 B).
    pub text: heapless::String<160>,
    /// Display name resolved at insert time, cached so the thread view
    /// can render without re-looking up.  Falls back to a hex prefix
    /// when no name is known.
    pub peer_name: heapless::String<32>,
    /// Seconds-since-boot when *we* observed this PM (sent or received).
    /// Source of truth for sort + "Last:" rendering — same model as
    /// the Contacts screen's observation table.
    pub observed_at_secs: u64,
}

static INBOX: Mutex<CriticalSectionRawMutex, RefCell<heapless::Vec<PmEntry, MAX_ENTRIES>>> =
    Mutex::new(RefCell::new(heapless::Vec::new()));

/// Per-peer "last read" cursor, in seconds-since-boot.  Lookups are
/// O(N) over a tiny table; eviction-by-LRU keeps it bounded.  Per-boot
/// — unread counts reset across reboots.
const READ_CURSORS_CAP: usize = 16;

#[derive(Clone, Copy)]
struct ReadCursor {
    pub_key: [u8; 32],
    last_read_secs: u64,
}

static READ_CURSORS: Mutex<
    CriticalSectionRawMutex,
    RefCell<heapless::Vec<ReadCursor, READ_CURSORS_CAP>>,
> = Mutex::new(RefCell::new(heapless::Vec::new()));

// ── Insertion helpers ──────────────────────────────────────────────────────

fn push_entry(entry: PmEntry) {
    INBOX.lock(|cell| {
        let mut list = cell.borrow_mut();
        if list.is_full() {
            // FIFO eviction by observation time.
            let oldest_idx = list
                .iter()
                .enumerate()
                .min_by_key(|(_, e)| e.observed_at_secs)
                .map(|(i, _)| i)
                .unwrap_or(0);
            let _ = list.swap_remove(oldest_idx);
        }
        let _ = list.push(entry);
    });
}

/// Resolve a peer's name from the Contacts-screen cache + discovery
/// cache.  Falls back to a 16-char hex prefix when no name is known.
fn resolve_peer_name(pub_key: &[u8; 32]) -> heapless::String<32> {
    // Fast path: check the rendered Contacts cache (covers both saved
    // and discovered entries).
    let from_cache = super::contacts_screen::CACHED_CONTACTS.lock(|c| {
        c.borrow()
            .iter()
            .find(|e| &e.pub_key == pub_key)
            .map(|e| e.name.clone())
    });
    if let Some(n) = from_cache
        && !n.is_empty()
    {
        return n;
    }
    // Last resort: hex prefix.
    let mut s: heapless::String<32> = heapless::String::new();
    for &b in pub_key.iter().take(8) {
        let hi = b >> 4;
        let lo = b & 0xF;
        let _ = s.push(if hi < 10 {
            (b'0' + hi) as char
        } else {
            (b'a' + hi - 10) as char
        });
        let _ = s.push(if lo < 10 {
            (b'0' + lo) as char
        } else {
            (b'a' + lo - 10) as char
        });
    }
    s
}

/// Record an incoming PM.  Called from `meshcore::log_advert`'s sibling
/// PM-handling path.
pub fn note_incoming(pub_key: &[u8; 32], peer_name: &str, text: &str) {
    let mut text_buf: heapless::String<160> = heapless::String::new();
    let _ = text_buf.push_str(text);
    let mut name_buf: heapless::String<32> = heapless::String::new();
    if peer_name.is_empty() {
        name_buf = resolve_peer_name(pub_key);
    } else {
        let _ = name_buf.push_str(peer_name);
    }
    push_entry(PmEntry {
        pub_key: *pub_key,
        direction: Direction::Incoming,
        text: text_buf,
        peer_name: name_buf,
        observed_at_secs: embassy_time::Instant::now().as_secs(),
    });
}

/// Record an outgoing PM.  Called from the Contacts-screen popup when
/// the user submits a compose, and from the BLE companion's
/// SEND_TXT_MSG path so phone-originated PMs also show up in the
/// on-device thread.
pub fn note_outgoing(pub_key: &[u8; 32], text: &str) {
    let mut text_buf: heapless::String<160> = heapless::String::new();
    let _ = text_buf.push_str(text);
    let name_buf = resolve_peer_name(pub_key);
    push_entry(PmEntry {
        pub_key: *pub_key,
        direction: Direction::Outgoing,
        text: text_buf,
        peer_name: name_buf,
        observed_at_secs: embassy_time::Instant::now().as_secs(),
    });
}

// ── Read access for the screen ──────────────────────────────────────────────

/// One peer's summary row for the inbox-list view.
#[derive(Clone)]
pub struct PeerSummary {
    pub pub_key: [u8; 32],
    pub peer_name: heapless::String<32>,
    /// Newest entry's text, truncated by the renderer.
    pub last_text: heapless::String<160>,
    /// Newest entry's direction — drives a small `→` / `←` glyph.
    pub last_direction: Direction,
    pub last_observed_at_secs: u64,
    /// Count of incoming messages newer than the per-peer read cursor.
    pub unread: u8,
}

/// Build the inbox peer list — one row per distinct `pub_key`, sorted
/// by `last_observed_at_secs` descending.  Walks the inbox ring twice
/// (once to enumerate distinct peers, once to compute unread); for a
/// 32-entry ring with ≤ 8 active threads this is trivial.
pub fn peer_list() -> heapless::Vec<PeerSummary, MAX_ENTRIES> {
    let mut summary: heapless::Vec<PeerSummary, MAX_ENTRIES> = heapless::Vec::new();
    INBOX.lock(|cell| {
        let list = cell.borrow();
        for entry in list.iter() {
            if let Some(s) = summary.iter_mut().find(|s| s.pub_key == entry.pub_key) {
                if entry.observed_at_secs > s.last_observed_at_secs {
                    s.last_text = entry.text.clone();
                    s.last_direction = entry.direction;
                    s.last_observed_at_secs = entry.observed_at_secs;
                }
            } else {
                let _ = summary.push(PeerSummary {
                    pub_key: entry.pub_key,
                    peer_name: entry.peer_name.clone(),
                    last_text: entry.text.clone(),
                    last_direction: entry.direction,
                    last_observed_at_secs: entry.observed_at_secs,
                    unread: 0,
                });
            }
        }
    });
    // Compute unread counts — incoming entries newer than the per-peer
    // read cursor.
    READ_CURSORS.lock(|cell| {
        let cursors = cell.borrow();
        for s in summary.iter_mut() {
            let cursor = cursors
                .iter()
                .find(|c| c.pub_key == s.pub_key)
                .map(|c| c.last_read_secs)
                .unwrap_or(0);
            INBOX.lock(|ic| {
                s.unread = ic
                    .borrow()
                    .iter()
                    .filter(|e| {
                        e.pub_key == s.pub_key
                            && e.direction == Direction::Incoming
                            && e.observed_at_secs > cursor
                    })
                    .count()
                    .min(255) as u8;
            });
        }
    });
    summary.sort_unstable_by(|a, b| b.last_observed_at_secs.cmp(&a.last_observed_at_secs));
    summary
}

/// Return all entries for `pub_key` in chronological order.
pub fn thread_for(pub_key: &[u8; 32]) -> heapless::Vec<PmEntry, MAX_ENTRIES> {
    let mut out: heapless::Vec<PmEntry, MAX_ENTRIES> = heapless::Vec::new();
    INBOX.lock(|cell| {
        for entry in cell.borrow().iter() {
            if &entry.pub_key == pub_key {
                let _ = out.push(entry.clone());
            }
        }
    });
    out.sort_unstable_by(|a, b| a.observed_at_secs.cmp(&b.observed_at_secs));
    out
}

/// Mark the user as having seen everything for `pub_key` up to now.
/// Resets the (N) unread badge for that peer.
pub fn mark_read(pub_key: &[u8; 32]) {
    let now = embassy_time::Instant::now().as_secs();
    READ_CURSORS.lock(|cell| {
        let mut list = cell.borrow_mut();
        if let Some(c) = list.iter_mut().find(|c| &c.pub_key == pub_key) {
            c.last_read_secs = now;
            return;
        }
        if list.is_full() {
            // Evict the oldest cursor.
            let oldest_idx = list
                .iter()
                .enumerate()
                .min_by_key(|(_, c)| c.last_read_secs)
                .map(|(i, _)| i)
                .unwrap_or(0);
            let _ = list.swap_remove(oldest_idx);
        }
        let _ = list.push(ReadCursor {
            pub_key: *pub_key,
            last_read_secs: now,
        });
    });
}

/// `true` when at least one incoming message exists newer than the
/// `pub_key`'s read cursor.  Cheap version of `peer_list().unread > 0`
/// for callers that only want a yes/no — currently unused but exposed
/// for the future passive-screen indicator.
#[allow(dead_code)]
pub fn has_unread(pub_key: &[u8; 32]) -> bool {
    let cursor = READ_CURSORS.lock(|cell| {
        cell.borrow()
            .iter()
            .find(|c| &c.pub_key == pub_key)
            .map(|c| c.last_read_secs)
            .unwrap_or(0)
    });
    INBOX.lock(|cell| {
        cell.borrow().iter().any(|e| {
            &e.pub_key == pub_key
                && e.direction == Direction::Incoming
                && e.observed_at_secs > cursor
        })
    })
}

// ── UI state machine ───────────────────────────────────────────────────────
//
// Mirrors the pattern from `contacts_screen` — a small `Mode` enum
// guarded by a Mutex<RefCell<…>>, sync `dispatch()` and `draw()`
// entry points called by the menu layer.

use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{PrimitiveStyle, Rectangle};
use embedded_graphics::text::{Alignment, Baseline, Text, TextStyleBuilder};

use crate::menu::ButtonId;
use crate::{BLACK, TriColor, WHITE, draw_header, ui};

#[derive(Clone, Copy)]
enum Mode {
    Inbox,
    Thread { pub_key: [u8; 32] },
}

pub struct InboxState {
    mode: Mode,
    /// Inbox cursor (index into the peer list).
    cursor: u8,
    /// Inbox scroll offset.
    scroll: u8,
    /// Thread scroll offset (rows from the top of the chronological
    /// thread that the renderer should skip).  Reset on enter.
    thread_scroll: u8,
}

impl InboxState {
    const fn new() -> Self {
        Self {
            mode: Mode::Inbox,
            cursor: 0,
            scroll: 0,
            thread_scroll: 0,
        }
    }
}

pub static BROWSER: Mutex<CriticalSectionRawMutex, RefCell<InboxState>> =
    Mutex::new(RefCell::new(InboxState::new()));

const ROW_H: i32 = 18;
const VISIBLE_ROWS: u8 = 7;
/// Max characters we render in a thread bubble before wrapping is left
/// to the renderer's natural break.  Each row is ~7 px/char × 144 px =
/// 20 chars.
const THREAD_LINE_CHARS: usize = 20;
const THREAD_VISIBLE_LINES: u8 = 7;

/// Handle a button press.  Returns `true` when Cancel should propagate
/// to the menu layer (i.e., leave the PM screen).
pub fn dispatch(btn: ButtonId) -> bool {
    BROWSER.lock(|cell| {
        let mut b = cell.borrow_mut();
        match b.mode {
            Mode::Inbox => {
                let count = INBOX.lock(|c| {
                    let list = c.borrow();
                    let mut seen: heapless::Vec<[u8; 32], MAX_ENTRIES> = heapless::Vec::new();
                    for e in list.iter() {
                        if !seen.contains(&e.pub_key) {
                            let _ = seen.push(e.pub_key);
                        }
                    }
                    seen.len() as u8
                });
                match btn {
                    ButtonId::Up => {
                        if b.cursor > 0 {
                            b.cursor -= 1;
                            if b.cursor < b.scroll {
                                b.scroll = b.cursor;
                            }
                        }
                        false
                    }
                    ButtonId::Down => {
                        if b.cursor + 1 < count {
                            b.cursor += 1;
                            if b.cursor >= b.scroll + VISIBLE_ROWS {
                                b.scroll = b.cursor + 1 - VISIBLE_ROWS;
                            }
                        }
                        false
                    }
                    ButtonId::Fire | ButtonId::Execute => {
                        if count > 0
                            && let Some(s) = peer_list().get(b.cursor as usize)
                        {
                            mark_read(&s.pub_key);
                            b.mode = Mode::Thread { pub_key: s.pub_key };
                            b.thread_scroll = 0;
                        }
                        false
                    }
                    ButtonId::Cancel => true,
                    // Left/Right propagate to the screen-swipe carousel.
                    ButtonId::Left | ButtonId::Right => true,
                }
            }
            Mode::Thread { pub_key } => match btn {
                ButtonId::Up => {
                    if b.thread_scroll > 0 {
                        b.thread_scroll -= 1;
                    }
                    false
                }
                ButtonId::Down => {
                    // Allow scrolling as long as more lines remain.
                    let total_lines = total_thread_lines(&pub_key);
                    let max_scroll = total_lines.saturating_sub(THREAD_VISIBLE_LINES as usize);
                    if (b.thread_scroll as usize) < max_scroll {
                        b.thread_scroll += 1;
                    }
                    false
                }
                ButtonId::Fire | ButtonId::Execute => {
                    // Open the keyboard for a reply.  Reuses the
                    // Contacts-screen compose plumbing — same recipient
                    // stash, same tx_send path, same on-submit callback.
                    super::contacts_screen::start_pm_compose(pub_key);
                    false
                }
                ButtonId::Cancel => {
                    b.mode = Mode::Inbox;
                    false
                }
                ButtonId::Left | ButtonId::Right => true,
            },
        }
    })
}

fn total_thread_lines(pub_key: &[u8; 32]) -> usize {
    // Each entry consumes one header line + ceil(text/THREAD_LINE_CHARS)
    // body lines.  Cheap walk over the at-most-32 entries.
    let entries = thread_for(pub_key);
    let mut lines = 0usize;
    for e in entries.iter() {
        lines += 1;
        let bytes = e.text.len();
        lines += if bytes == 0 {
            1
        } else {
            (bytes + THREAD_LINE_CHARS - 1) / THREAD_LINE_CHARS
        };
    }
    lines
}

pub fn draw<D>(display: &mut D, bat_prc: &u8) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let (mode, cursor, scroll, thread_scroll) = BROWSER.lock(|c| {
        let s = c.borrow();
        (s.mode, s.cursor, s.scroll, s.thread_scroll)
    });
    match mode {
        Mode::Inbox => draw_inbox(display, bat_prc, cursor, scroll),
        Mode::Thread { pub_key } => draw_thread(display, bat_prc, &pub_key, thread_scroll),
    }
}

fn draw_inbox<D>(display: &mut D, bat_prc: &u8, cursor: u8, scroll: u8) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    draw_header(display, "Messages", bat_prc)?;
    let summary = peer_list();
    if summary.is_empty() {
        ui::draw_centered_message(display, "No messages yet", Point::new(76, 80))?;
        return Ok(());
    }

    let list_top: i32 = ui::TITLE_BAR_H as i32 + 2;
    let bottom = TextStyleBuilder::new().baseline(Baseline::Bottom).build();
    let right = TextStyleBuilder::new()
        .baseline(Baseline::Bottom)
        .alignment(Alignment::Right)
        .build();
    let total = summary.len() as u8;
    let visible = VISIBLE_ROWS.min(total - scroll);
    for i in 0..visible {
        let idx = scroll + i;
        let s = &summary[idx as usize];
        let row_top = list_top + i as i32 * ROW_H;
        let row_mid = row_top + ROW_H / 2;
        let selected = idx == cursor;
        if selected {
            Rectangle::new(Point::new(0, row_top), Size::new(152, ROW_H as u32))
                .into_styled(PrimitiveStyle::with_fill(BLACK))
                .draw(display)?;
        }
        let txt = if selected {
            ui::TEXT_BOLD_WHITE
        } else {
            ui::TEXT_BLACK
        };
        let small = if selected {
            ui::TEXT_BOLD_WHITE
        } else {
            ui::TEXT_BLACK
        };

        // Row 1: peer name (with `(N)` unread suffix).
        let name = s.peer_name.as_str();
        let name = if name.is_empty() { "(unknown)" } else { name };
        let max_chars = 14usize;
        let name_short = if name.len() > max_chars {
            &name[..max_chars]
        } else {
            name
        };
        Text::with_text_style(name_short, Point::new(2, row_mid - 1), txt, bottom).draw(display)?;
        if s.unread > 0 {
            let mut badge: heapless::String<8> = heapless::String::new();
            let _ = core::fmt::Write::write_fmt(&mut badge, format_args!("({})", s.unread));
            Text::with_text_style(badge.as_str(), Point::new(150, row_mid - 1), txt, right)
                .draw(display)?;
        }

        // Row 2: latest-message preview, with a small direction arrow.
        let arrow = match s.last_direction {
            Direction::Incoming => "<",
            Direction::Outgoing => ">",
        };
        let preview_max = 20usize;
        let preview = s.last_text.as_str();
        let preview_short = if preview.len() > preview_max {
            &preview[..preview_max]
        } else {
            preview
        };
        let mut combined: heapless::String<32> = heapless::String::new();
        let _ = combined.push_str(arrow);
        let _ = combined.push(' ');
        let _ = combined.push_str(preview_short);
        Text::with_text_style(combined.as_str(), Point::new(2, row_mid + 8), small, bottom)
            .draw(display)?;
    }
    Ok(())
}

fn draw_thread<D>(
    display: &mut D,
    bat_prc: &u8,
    pub_key: &[u8; 32],
    scroll: u8,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    // Resolve a header label — peer name, falling back to hex prefix.
    let mut title_buf: heapless::String<24> = heapless::String::new();
    let _ = title_buf.push_str("PM: ");
    let name = resolve_peer_name(pub_key);
    let n = name.as_str();
    let n_short = if n.len() > 16 { &n[..16] } else { n };
    let _ = title_buf.push_str(n_short);
    draw_header(display, title_buf.as_str(), bat_prc)?;

    let entries = thread_for(pub_key);
    if entries.is_empty() {
        ui::draw_centered_message(display, "(empty thread)", Point::new(76, 80))?;
        return Ok(());
    }

    // Walk entries → produce a flat line list (header + body lines).
    // Then render the [scroll .. scroll + THREAD_VISIBLE_LINES) window.
    let bottom = TextStyleBuilder::new().baseline(Baseline::Bottom).build();
    let mut painted: u8 = 0;
    let mut skipped: u32 = 0;
    let body_top: i32 = ui::TITLE_BAR_H as i32 + 4;

    for entry in entries.iter() {
        // Header: a `>` for outgoing, `<` for incoming, plus rough
        // age or direction indicator.
        let header = match entry.direction {
            Direction::Incoming => "<",
            Direction::Outgoing => ">",
        };
        if skipped < scroll as u32 {
            skipped += 1;
        } else if painted < THREAD_VISIBLE_LINES {
            let row_y = body_top + painted as i32 * ROW_H + ROW_H - 4;
            Text::with_text_style(header, Point::new(2, row_y), ui::TEXT_BOLD_BLACK, bottom)
                .draw(display)?;
            painted += 1;
        }
        // Body — split into THREAD_LINE_CHARS-wide chunks.
        let bytes = entry.text.as_bytes();
        let total_chunks = if bytes.is_empty() {
            1
        } else {
            (bytes.len() + THREAD_LINE_CHARS - 1) / THREAD_LINE_CHARS
        };
        for chunk_i in 0..total_chunks {
            if skipped < scroll as u32 {
                skipped += 1;
                continue;
            }
            if painted >= THREAD_VISIBLE_LINES {
                break;
            }
            let start = chunk_i * THREAD_LINE_CHARS;
            let end = ((chunk_i + 1) * THREAD_LINE_CHARS).min(bytes.len());
            let slice = if start < bytes.len() {
                core::str::from_utf8(&bytes[start..end]).unwrap_or("")
            } else {
                ""
            };
            let row_y = body_top + painted as i32 * ROW_H + ROW_H - 4;
            // Indent body lines so they line up to the right of the arrow.
            Text::with_text_style(slice, Point::new(12, row_y), ui::TEXT_BLACK, bottom)
                .draw(display)?;
            painted += 1;
        }
        if painted >= THREAD_VISIBLE_LINES {
            break;
        }
    }

    // Footer hint.
    let hint = "Fire: Reply  Cancel: back";
    Text::with_text_style(hint, Point::new(2, 152), ui::TEXT_BLACK, bottom).draw(display)?;
    // Avoid unused-lint for the fallback empty path.
    let _ = WHITE;
    Ok(())
}
