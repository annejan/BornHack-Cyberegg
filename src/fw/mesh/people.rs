//! On-device "People" screen — discovery-sorted list of mesh contacts with a
//! popup menu for per-contact actions (PM, Info, …).
//!
//! Replaces the old single-record `SCREEN_ADVERT`.  The advert *is* a contact
//! event: every received advert updates `contacts.rs`'s slot for that
//! `pub_key` (creating it on first sight), and this screen renders the
//! contact store sorted by `last_advert_ts` descending so live nodes float
//! to the top.
//!
//! See `PEOPLE_SCREEN.md` at the repo root for the full design.
//!
//! ## State
//!
//! - `CACHED_PEOPLE` — heapless ring of summary rows for sync access from the
//!   draw path.  Refilled by `refresh_cache()` from the persistent
//!   `ContactStore` whenever a new advert lands (debounced).
//! - `BROWSER` — UI state machine: List ↔ Popup ↔ Detail.
//!
//! ## Rendering
//!
//! Top-level list shows up to `VISIBLE_ROWS` rows at a time with the cursor
//! row inverted.  The popup is drawn on top using `ui::draw_picker_menu`.

use core::cell::RefCell;

use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{PrimitiveStyle, Rectangle};
use embedded_graphics::text::{Alignment, Baseline, Text, TextStyleBuilder};
use heapless::format;

use super::contacts::{Contact, ContactStore, FLAG_FAVORITE, MAX_CONTACTS};
use crate::menu::ButtonId;
use crate::{BLACK, RED, TriColor, WHITE, draw_header, ui, unix_now};

// ── Tunables ────────────────────────────────────────────────────────────────

/// Maximum entries cached in RAM for the sync draw path.  Anything beyond
/// this falls off the bottom of the list — at a hacker camp 50 is more
/// than enough since the list is sorted by recency.  Each entry is small
/// (~80 B) so the cache fits in ≈ 4 KiB.
const CACHE_CAP: usize = 50;

/// Rows visible at once.  Header eats `TITLE_BAR_H` so we have ~134 px of
/// list area; each row is `ROW_H` px tall.
const ROW_H: i32 = 18;
const VISIBLE_ROWS: u8 = 7;

/// Live-dot threshold — a contact's advert is "live" when last seen within
/// this window (seconds).  Only used for the red dot indicator; sort is
/// strictly by `last_advert_ts` desc regardless.
const LIVE_WINDOW_SECS: u32 = 5 * 60;

// ── Cached entry ────────────────────────────────────────────────────────────

/// One contact's display-only summary.  Lighter than [`Contact`] so the
/// cache stays small and the draw path doesn't pull in routing/path data
/// it doesn't need.
#[derive(Clone)]
pub struct PeopleEntry {
    pub pub_key: [u8; 32],
    pub name: heapless::String<32>,
    pub node_type: u8,
    pub flags: u8,
    pub last_advert_ts: u32,
}

impl PeopleEntry {
    fn from_contact(c: &Contact) -> Self {
        let mut name: heapless::String<32> = heapless::String::new();
        let bytes = c.name_bytes();
        if let Ok(s) = core::str::from_utf8(bytes) {
            let _ = name.push_str(s);
        }
        Self {
            pub_key: c.pub_key,
            name,
            node_type: c.node_type,
            flags: c.flags,
            last_advert_ts: c.last_advert_ts,
        }
    }

    fn is_favorite(&self) -> bool {
        self.flags & FLAG_FAVORITE != 0
    }
}

pub static CACHED_PEOPLE: Mutex<
    CriticalSectionRawMutex,
    RefCell<heapless::Vec<PeopleEntry, CACHE_CAP>>,
> = Mutex::new(RefCell::new(heapless::Vec::new()));

// ── UI state ────────────────────────────────────────────────────────────────

#[derive(Clone, Copy)]
enum Mode {
    /// Top-level scrollable list.
    List,
    /// Popup menu over the list — `target` is the cached-list index of the
    /// selected contact, `pos` is the popup cursor.
    Popup { target: u8, pos: u8 },
    /// Per-contact info panel.
    Detail { target: u8 },
}

pub struct PeopleState {
    mode: Mode,
    /// Cursor row in the underlying cache (not screen offset).
    cursor: u8,
    /// First visible cache index.
    scroll: u8,
}

impl PeopleState {
    const fn new() -> Self {
        Self {
            mode: Mode::List,
            cursor: 0,
            scroll: 0,
        }
    }
}

pub static BROWSER: Mutex<CriticalSectionRawMutex, RefCell<PeopleState>> =
    Mutex::new(RefCell::new(PeopleState::new()));

/// Reset cursor/scroll to the top.  Called when the user navigates away.
pub fn reset() {
    BROWSER.lock(|cell| {
        *cell.borrow_mut() = PeopleState::new();
    });
}

// ── Cache refresh ──────────────────────────────────────────────────────────

/// Read the persistent contact store and rebuild the top-`CACHE_CAP`
/// in-RAM cache, sorted by `last_advert_ts` descending.
///
/// Implemented as an online insertion-sort to avoid allocating a
/// `MAX_CONTACTS`-sized scratch buffer (which would cost ≈ 24 KiB on the
/// refresh task's stack/future).  Cost: up to `MAX_CONTACTS` async kv
/// reads (≈ 300 ms on a full store) + `O(N · K)` insertion work where
/// `N = MAX_CONTACTS`, `K = CACHE_CAP`.  Call from a debounced refresh
/// task — not from the draw path.
pub async fn refresh_cache() {
    let store = ContactStore::new();
    let mut top: heapless::Vec<PeopleEntry, CACHE_CAP> = heapless::Vec::new();

    for idx in 0..MAX_CONTACTS {
        let Some(c) = store.read_slot(idx).await else {
            continue;
        };
        if c.is_deleted() {
            continue;
        }
        let e = PeopleEntry::from_contact(&c);

        // Find insertion position to keep `top` sorted by last_advert_ts
        // descending.  When the cache is full and the new entry is older
        // than every existing entry, skip it.
        let pos = top
            .iter()
            .position(|x| x.last_advert_ts < e.last_advert_ts)
            .unwrap_or(top.len());
        if pos >= CACHE_CAP {
            continue;
        }
        if top.len() == CACHE_CAP {
            let _ = top.pop();
        }
        let _ = top.insert(pos, e);
    }

    CACHED_PEOPLE.lock(|cell| {
        let mut list = cell.borrow_mut();
        list.clear();
        for e in top.iter() {
            let _ = list.push(e.clone());
        }
    });
}

/// Embassy task: rebuild the cache when adverts arrive.  Debounces bursts
/// (e.g. multiple adverts during a sync gust) by waiting for a quiet 1 s
/// window before each rebuild.
#[embassy_executor::task]
pub async fn refresh_cache_task() {
    use embassy_time::{Duration, Timer, with_timeout};
    // Initial population at boot.
    refresh_cache().await;
    loop {
        // Block until at least one advert (or other mutation) arrives.
        crate::ADVERT_SIGNAL.wait().await;
        // Coalesce: keep absorbing further signals as long as they keep
        // arriving within the debounce window.
        loop {
            match with_timeout(Duration::from_millis(1000), crate::ADVERT_SIGNAL.wait()).await {
                Ok(()) => continue, // got another, keep waiting
                Err(_) => break,    // quiet for the window — go rebuild
            }
        }
        refresh_cache().await;
        // Brief breath so we don't hammer kv in pathological burst.
        Timer::after(Duration::from_millis(50)).await;
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────────

fn live_now(ts: u32) -> bool {
    let Some(now) = unix_now() else {
        return false;
    };
    now.saturating_sub(ts) <= LIVE_WINDOW_SECS
}

/// Render a relative time delta as a short string fitting in ~3 chars
/// where possible: `now`, `3m`, `42m`, `5h`, `ydy`, `3d`, `2w`, `?`.
fn fmt_relative(ts: u32) -> heapless::String<5> {
    let mut s: heapless::String<5> = heapless::String::new();
    let Some(now) = unix_now() else {
        let _ = s.push('?');
        return s;
    };
    if ts > now.saturating_add(60) {
        // Future — clock skew.
        let _ = s.push('?');
        return s;
    }
    let delta = now.saturating_sub(ts);
    if delta < 60 {
        let _ = s.push_str("now");
    } else if delta < 60 * 60 {
        if let Ok(t) = format!(5; "{}m", delta / 60) {
            s = t;
        }
    } else if delta < 24 * 60 * 60 {
        if let Ok(t) = format!(5; "{}h", delta / 3600) {
            s = t;
        }
    } else if delta < 2 * 24 * 60 * 60 {
        let _ = s.push_str("ydy");
    } else if delta < 14 * 24 * 60 * 60 {
        if let Ok(t) = format!(5; "{}d", delta / 86400) {
            s = t;
        }
    } else {
        if let Ok(t) = format!(5; "{}w", delta / (7 * 86400)) {
            s = t;
        }
    }
    s
}

fn role_glyph(node_type: u8) -> Option<&'static str> {
    match node_type {
        // Chat node — no glyph (the common case stays visually quiet).
        1 => None,
        2 => Some("R"), // Repeater
        3 => Some("#"), // Room server
        4 => Some("S"), // Sensor
        _ => Some("?"),
    }
}

/// Role-aware popup item set.  Returned as a fixed-size array of
/// `Option<&str>`; `None` slots are not rendered.  Index 0 is always the
/// primary action and is preselected on entry.
fn popup_items(node_type: u8) -> heapless::Vec<&'static str, 4> {
    let mut v: heapless::Vec<&'static str, 4> = heapless::Vec::new();
    match node_type {
        1 => {
            // Chat Node
            let _ = v.push("PM");
            let _ = v.push("Info");
            let _ = v.push("< Cancel");
        }
        2 => {
            // Repeater
            let _ = v.push("Info");
            let _ = v.push("< Cancel");
        }
        3 => {
            // Room Server
            let _ = v.push("Info");
            let _ = v.push("< Cancel");
        }
        4 => {
            // Sensor
            let _ = v.push("Info");
            let _ = v.push("< Cancel");
        }
        _ => {
            let _ = v.push("Info");
            let _ = v.push("< Cancel");
        }
    }
    v
}

// ── Input dispatch ──────────────────────────────────────────────────────────

/// Handle a button press.  Returns `true` when Cancel should propagate to
/// the menu layer (i.e., we want to leave the People screen entirely).
pub fn dispatch(btn: ButtonId) -> bool {
    let count = CACHED_PEOPLE.lock(|c| c.borrow().len() as u8);

    BROWSER.lock(|cell| {
        let mut b = cell.borrow_mut();
        match b.mode {
            Mode::List => match btn {
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
                    if count > 0 {
                        b.mode = Mode::Popup {
                            target: b.cursor,
                            pos: 0,
                        };
                    }
                    false
                }
                ButtonId::Cancel => true,
                // Left/Right fall through to the global screen-swipe carousel.
                ButtonId::Left | ButtonId::Right => true,
            },

            Mode::Popup { target, pos } => {
                let node_type =
                    CACHED_PEOPLE.lock(|c| c.borrow().get(target as usize).map(|e| e.node_type));
                let items = node_type.map(popup_items).unwrap_or_default();
                let n = items.len() as u8;
                match btn {
                    ButtonId::Up => {
                        if pos > 0 {
                            b.mode = Mode::Popup {
                                target,
                                pos: pos - 1,
                            };
                        }
                        false
                    }
                    ButtonId::Down => {
                        if pos + 1 < n {
                            b.mode = Mode::Popup {
                                target,
                                pos: pos + 1,
                            };
                        }
                        false
                    }
                    ButtonId::Fire | ButtonId::Execute => {
                        let label = items.get(pos as usize).copied().unwrap_or("");
                        // Run the action.  `Cancel` and `Info` resolve here;
                        // `PM` switches the active screen so we drop our
                        // mode back to List first to avoid coming back into
                        // a stale popup if the user navigates back.
                        match label {
                            "PM" => {
                                b.mode = Mode::List;
                                drop(b);
                                // Switch to the existing PM screen.  The
                                // per-contact thread UX is a follow-up
                                // (see PEOPLE_SCREEN.md "Out of scope").
                                crate::DISPLAY_STATE
                                    .lock(|s| s.borrow_mut().set_active_screen(crate::SCREEN_PM));
                                return false;
                            }
                            "Info" => {
                                b.mode = Mode::Detail { target };
                            }
                            _ => {
                                // Cancel
                                b.mode = Mode::List;
                            }
                        }
                        false
                    }
                    ButtonId::Cancel => {
                        b.mode = Mode::List;
                        false
                    }
                    // Left/Right don't propagate from a popup — keep the
                    // user inside the modal until they confirm/cancel.
                    ButtonId::Left | ButtonId::Right => false,
                }
            }

            Mode::Detail { target } => match btn {
                ButtonId::Cancel | ButtonId::Left => {
                    b.mode = Mode::List;
                    false
                }
                ButtonId::Right => {
                    // Next contact (clamped).
                    if target + 1 < count {
                        b.mode = Mode::Detail { target: target + 1 };
                    }
                    false
                }
                ButtonId::Fire | ButtonId::Execute => {
                    // Open PM if this contact is a chat node.
                    let is_chat = CACHED_PEOPLE.lock(|c| {
                        c.borrow()
                            .get(target as usize)
                            .map(|e| e.node_type == 1)
                            .unwrap_or(false)
                    });
                    if is_chat {
                        b.mode = Mode::List;
                        drop(b);
                        crate::DISPLAY_STATE
                            .lock(|s| s.borrow_mut().set_active_screen(crate::SCREEN_PM));
                    }
                    false
                }
                _ => false,
            },
        }
    })
}

// ── Render ──────────────────────────────────────────────────────────────────

pub fn draw<D>(display: &mut D, bat_prc: &u8) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    draw_header(display, "People", bat_prc)?;

    let (cursor, scroll, mode) = BROWSER.lock(|c| {
        let s = c.borrow();
        (s.cursor, s.scroll, s.mode)
    });

    // Empty state.
    let empty = CACHED_PEOPLE.lock(|c| c.borrow().is_empty());
    if empty {
        ui::draw_centered_message(display, "Listening for adverts…", Point::new(76, 80))?;
        return Ok(());
    }

    draw_list(display, cursor, scroll)?;

    // Overlays — drawn after the list so they sit on top.
    match mode {
        Mode::List => {}
        Mode::Popup { target, pos } => draw_popup(display, target, pos)?,
        Mode::Detail { target } => draw_detail(display, target)?,
    }

    Ok(())
}

fn draw_list<D>(display: &mut D, cursor: u8, scroll: u8) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let list_top: i32 = ui::TITLE_BAR_H as i32 + 2;
    let bottom = TextStyleBuilder::new().baseline(Baseline::Bottom).build();
    let right = TextStyleBuilder::new()
        .baseline(Baseline::Bottom)
        .alignment(Alignment::Right)
        .build();

    CACHED_PEOPLE.lock(|c| -> Result<(), D::Error> {
        let list = c.borrow();
        for screen_row in 0..VISIBLE_ROWS {
            let cache_idx = scroll as usize + screen_row as usize;
            let Some(entry) = list.get(cache_idx) else {
                break;
            };
            let row_top = list_top + screen_row as i32 * ROW_H;
            let row_mid = row_top + ROW_H / 2;
            let selected = cache_idx as u8 == cursor;
            // Selected row inverted.
            if selected {
                Rectangle::new(Point::new(0, row_top), Size::new(152, ROW_H as u32))
                    .into_styled(PrimitiveStyle::with_fill(BLACK))
                    .draw(display)?;
            }
            let txt_style = if selected {
                ui::TEXT_BOLD_WHITE
            } else {
                ui::TEXT_BLACK
            };

            // Live dot — red filled circle, ~5 px diameter, only when fresh.
            // Selected row: keep the dot but recolor to white so it stays
            // visible on the black inverted bar.
            if live_now(entry.last_advert_ts) {
                let dot_color = if selected { WHITE } else { RED };
                Rectangle::new(Point::new(2, row_mid - 2), Size::new(5, 5))
                    .into_styled(PrimitiveStyle::with_fill(dot_color))
                    .draw(display)?;
            }

            // Role glyph (only for non-chat nodes).
            let mut name_x = 12;
            if let Some(g) = role_glyph(entry.node_type) {
                Text::with_text_style(g, Point::new(12, row_mid + 5), txt_style, bottom)
                    .draw(display)?;
                name_x = 22;
            }

            // Name (with optional ★ prefix for favorites).
            let name = entry.name.as_str();
            let display_name = if name.is_empty() { "(unknown)" } else { name };
            // Truncate to fit ~14 chars at 7 px/char before the right column.
            let max_chars = 14usize;
            let truncated = if display_name.len() > max_chars {
                &display_name[..max_chars]
            } else {
                display_name
            };
            if entry.is_favorite() {
                Text::with_text_style("*", Point::new(name_x, row_mid + 5), txt_style, bottom)
                    .draw(display)?;
                Text::with_text_style(
                    truncated,
                    Point::new(name_x + 8, row_mid + 5),
                    txt_style,
                    bottom,
                )
                .draw(display)?;
            } else {
                Text::with_text_style(
                    truncated,
                    Point::new(name_x, row_mid + 5),
                    txt_style,
                    bottom,
                )
                .draw(display)?;
            }

            // Last-seen, right-aligned.
            let rel = fmt_relative(entry.last_advert_ts);
            Text::with_text_style(rel.as_str(), Point::new(150, row_mid + 5), txt_style, right)
                .draw(display)?;
        }
        Ok(())
    })
}

fn draw_popup<D>(display: &mut D, target: u8, pos: u8) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    // Resolve title (contact name, truncated) and the role-aware items.
    let (title, items_owned) = CACHED_PEOPLE.lock(|c| {
        let list = c.borrow();
        let Some(entry) = list.get(target as usize) else {
            return (
                heapless::String::<16>::new(),
                heapless::Vec::<&'static str, 4>::new(),
            );
        };
        let mut t: heapless::String<16> = heapless::String::new();
        let n = entry.name.as_str();
        let n = if n.len() > 14 { &n[..14] } else { n };
        let _ = t.push_str(if n.is_empty() { "(unknown)" } else { n });
        (t, popup_items(entry.node_type))
    });

    // `ui::draw_picker_menu` wants `&[&str]` — convert.
    let items_ref: heapless::Vec<&str, 4> = items_owned.iter().copied().collect();
    ui::draw_picker_menu(display, title.as_str(), items_ref.as_slice(), pos as usize)?;
    Ok(())
}

fn draw_detail<D>(display: &mut D, target: u8) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    // Full-screen detail view drawn over the list.  White-fill the body
    // first so the underlying list is hidden.
    Rectangle::new(
        Point::new(0, ui::TITLE_BAR_H as i32),
        Size::new(152, 152 - ui::TITLE_BAR_H),
    )
    .into_styled(PrimitiveStyle::with_fill(WHITE))
    .draw(display)?;

    let bottom = TextStyleBuilder::new().baseline(Baseline::Bottom).build();
    let style_bold = ui::TEXT_BOLD_BLACK;
    let style_small = ui::TEXT_BLACK;

    let entry: Option<PeopleEntry> =
        CACHED_PEOPLE.lock(|c| c.borrow().get(target as usize).cloned());
    let Some(entry) = entry else {
        return Ok(());
    };

    // Name (bold)
    let name = entry.name.as_str();
    let name = if name.is_empty() { "(unknown)" } else { name };
    Text::with_text_style(name, Point::new(4, 32), style_bold, bottom).draw(display)?;

    // Role
    let role = match entry.node_type {
        1 => "Chat Node",
        2 => "Repeater",
        3 => "Room Server",
        4 => "Sensor",
        _ => "Unknown role",
    };
    Text::with_text_style(role, Point::new(4, 48), style_small, bottom).draw(display)?;

    // Divider
    Rectangle::new(Point::new(0, 50), Size::new(152, 1))
        .into_styled(PrimitiveStyle::with_fill(BLACK))
        .draw(display)?;

    // Last seen
    let rel = fmt_relative(entry.last_advert_ts);
    let line = format!(20; "Last: {}", rel.as_str()).unwrap_or_default();
    Text::with_text_style(line.as_str(), Point::new(4, 66), style_small, bottom).draw(display)?;

    // Key prefix (8 bytes hex)
    Text::with_text_style("Key:", Point::new(4, 84), style_small, bottom).draw(display)?;
    let mut hex: heapless::String<24> = heapless::String::new();
    for (i, &byte) in entry.pub_key.iter().take(8).enumerate() {
        if i == 4 {
            let _ = hex.push(' ');
        }
        let hi = byte >> 4;
        let lo = byte & 0xF;
        let _ = hex.push(if hi < 10 {
            (b'0' + hi) as char
        } else {
            (b'a' + hi - 10) as char
        });
        let _ = hex.push(if lo < 10 {
            (b'0' + lo) as char
        } else {
            (b'a' + lo - 10) as char
        });
    }
    Text::with_text_style(hex.as_str(), Point::new(4, 100), style_small, bottom).draw(display)?;

    // Footer hint
    Text::with_text_style("Cancel: back", Point::new(4, 148), style_small, bottom).draw(display)?;

    Ok(())
}
