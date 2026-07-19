//! On-device "Contacts" screen — discovery-sorted list of mesh contacts
//! (chat nodes, repeaters, room servers, sensors) with a popup menu for
//! per-contact actions (PM, Info, …).
//!
//! Replaces the old single-record `SCREEN_ADVERT`.  The advert *is* a contact
//! event: every received advert updates `contacts.rs`'s slot for that
//! `pub_key` (creating it on first sight), and this screen renders the
//! contact store sorted by `last_advert_ts` descending so live nodes float
//! to the top.
//!
//! See `CONTACTS_SCREEN.md` at the repo root for the full design.
//!
//! ## State
//!
//! - `CACHED_CONTACTS` — heapless ring of summary rows for sync access from the
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

use super::contacts::{Contact, ContactStore, FLAG_FAVORITE, MAX_CONTACTS, OUT_PATH_UNKNOWN};
use super::{TxPrivateMsg, TxRequest, msg_queue, tx_send};
use crate::menu::ButtonId;
use crate::{BLACK, RED, TriColor, WHITE, draw_header, ui};

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
const LIVE_WINDOW_SECS: u64 = 5 * 60;

// ── Cached entry ────────────────────────────────────────────────────────────

/// One contact's display-only summary.  Lighter than [`Contact`] so the
/// cache stays small and the draw path doesn't pull in routing/path data
/// it doesn't need.
#[derive(Clone)]
pub struct ContactRow {
    pub pub_key: [u8; 32],
    pub name: heapless::String<32>,
    pub node_type: u8,
    pub flags: u8,
    /// Sender's claimed timestamp from their advert.  Persisted in the kv
    /// store; useful as a sort hint but unreliable for "last seen" since
    /// most badges advertise `timestamp=0` until their wall clock is set.
    pub last_advert_ts: u32,
    /// Routing-path length byte.  `OUT_PATH_UNKNOWN` (0xFF) = no path
    /// established (we have no DM route — only flood works).  Otherwise
    /// MeshCore encoding: bits 5-0 = hop count, bits 7-6 = hash size.
    pub out_path_len: u8,
    /// GPS latitude in microdegrees (1e-6°).  `0` = unset; the advert
    /// originator has either no GPS lock or chose not to broadcast.
    pub gps_lat: i32,
    /// GPS longitude in microdegrees.  `0` = unset (see `gps_lat`).
    pub gps_lon: i32,
    /// Seconds-since-boot when *we* last heard an advert from this
    /// pub_key during this session.  `None` if not heard since boot.
    /// This is the source of truth for the "Last:" column and the live
    /// dot — local time base, works regardless of inter-badge clock sync.
    pub observed_at_secs: Option<u64>,
    /// `true` when this row is backed by a persistent `ContactStore`
    /// slot.  `false` when it's a discovery-only entry (heard via advert
    /// but not yet promoted).  Drives the popup item set: saved rows
    /// get Save / Forget; discovery rows get Add.
    pub is_saved: bool,
}

impl ContactRow {
    fn from_contact(c: &Contact, observed_at_secs: Option<u64>) -> Self {
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
            out_path_len: c.out_path_len,
            gps_lat: c.gps_lat,
            gps_lon: c.gps_lon,
            observed_at_secs,
            is_saved: true,
        }
    }

    /// Build a row from a discovery-cache entry — these aren't in the
    /// persistent store yet, so `flags = 0` (no favorite) and
    /// `out_path_len = OUT_PATH_UNKNOWN` (no known route).
    fn from_discovery(e: &super::discovery::DiscoveryEntry) -> Self {
        Self {
            pub_key: e.pub_key,
            name: e.name.clone(),
            node_type: e.node_type,
            flags: 0,
            last_advert_ts: e.advert_ts,
            out_path_len: OUT_PATH_UNKNOWN,
            gps_lat: e.gps_lat,
            gps_lon: e.gps_lon,
            observed_at_secs: Some(e.observed_at_secs),
            is_saved: false,
        }
    }

    fn is_favorite(&self) -> bool {
        self.flags & FLAG_FAVORITE != 0
    }
}

pub static CACHED_CONTACTS: Mutex<
    CriticalSectionRawMutex,
    RefCell<heapless::Vec<ContactRow, CACHE_CAP>>,
> = Mutex::new(RefCell::new(heapless::Vec::new()));

/// Wakes the `refresh_cache_task`.  Separate from `ADVERT_SIGNAL`
/// because `embassy_sync::Signal` is single-waiter — sharing one
/// signal between the UI redraw loop (`bin/embassy.rs`) and this
/// task silently drops wakes for whichever side registered its
/// waker first.  `meshcore::log_advert` and `mutation_persister_task`
/// signal both.
pub static REBUILD_SIGNAL: embassy_sync::signal::Signal<CriticalSectionRawMutex, ()> =
    embassy_sync::signal::Signal::new();

/// Re-export so `refresh_cache` can read + clear the flag without
/// reaching across modules.  See [`super::contacts::STORE_DIRTY`]
/// for ownership and the set-on-mutation contract.
pub use super::contacts::STORE_DIRTY;

// ── Local-observation table ────────────────────────────────────────────────
//
// Tracks "I (this badge) heard from `pub_key` at this many seconds since
// boot."  The persistent contact store's `last_advert_ts` is the
// *sender's* clock — and most badges ship with no wall clock and
// advertise `timestamp=0`, so it's useless for "Last: 3m" rendering.
//
// Bounded ring (`OBSERVATIONS_CAP`) of `(pub_key, secs)`; oldest entry
// gets evicted on insert when full.  Per-boot — clears on reboot, which
// matches the discovery semantic ("I haven't heard them this session").

const OBSERVATIONS_CAP: usize = 64;

#[derive(Clone, Copy)]
struct Observation {
    pub_key: [u8; 32],
    secs: u64,
}

static OBSERVATIONS: Mutex<
    CriticalSectionRawMutex,
    RefCell<heapless::Vec<Observation, OBSERVATIONS_CAP>>,
> = Mutex::new(RefCell::new(heapless::Vec::new()));

/// Stamp `pub_key` as "heard right now."  Called from `meshcore::log_advert`
/// after the persistent store has been updated.  Cheap — bounded ring,
/// FIFO eviction.
pub fn note_observed(pub_key: &[u8; 32]) {
    let secs = embassy_time::Instant::now().as_secs();
    OBSERVATIONS.lock(|cell| {
        let mut list = cell.borrow_mut();
        // If we already have an entry for this key, just update it.
        if let Some(e) = list.iter_mut().find(|e| &e.pub_key == pub_key) {
            e.secs = secs;
            return;
        }
        // New key — append, evicting the oldest if full.
        if list.is_full() {
            // Find min-secs index and remove.
            let oldest_idx = list
                .iter()
                .enumerate()
                .min_by_key(|(_, e)| e.secs)
                .map(|(i, _)| i)
                .unwrap_or(0);
            let _ = list.swap_remove(oldest_idx);
        }
        let _ = list.push(Observation {
            pub_key: *pub_key,
            secs,
        });
    });
}

fn lookup_observation(pub_key: &[u8; 32]) -> Option<u64> {
    OBSERVATIONS.lock(|cell| {
        cell.borrow()
            .iter()
            .find(|e| &e.pub_key == pub_key)
            .map(|e| e.secs)
    })
}

// ── PM compose ─────────────────────────────────────────────────────────────
//
// `text_entry::begin` takes a `fn(&[u8])` (function pointer, not closure),
// so the recipient pub_key has to live somewhere statically reachable.
// We stash it here when the popup's PM action opens the keyboard, and
// `on_pm_compose_done` reads it back when the user submits.

static PM_COMPOSE_TARGET: Mutex<CriticalSectionRawMutex, RefCell<Option<[u8; 32]>>> =
    Mutex::new(RefCell::new(None));

/// Callback handed to `text_entry::begin` for the PM-compose flow.
/// Reads the target pub_key set by `start_pm_compose`, builds a
/// `TxPrivateMsg`, and pushes it onto the unified TX queue.  No-op
/// when the text is empty or the target is missing (e.g. the user
/// dismissed the keyboard without sending).
fn on_pm_compose_done(text: &[u8]) {
    let pub_key = match PM_COMPOSE_TARGET.lock(|c| c.borrow_mut().take()) {
        Some(k) => k,
        None => {
            defmt::info!("pm-compose: no target stashed — skipping");
            return;
        }
    };
    if text.is_empty() {
        defmt::info!(
            "pm-compose: empty text → no send (target {=[u8]:02x})",
            &pub_key[..6]
        );
        return;
    }
    let mut payload: heapless::Vec<u8, { msg_queue::MAX_TEXT }> = heapless::Vec::new();
    let n = text.len().min(payload.capacity());
    let _ = payload.extend_from_slice(&text[..n]);
    let timestamp = crate::unix_now().unwrap_or(0);
    if let Ok(text_str) = core::str::from_utf8(&payload) {
        super::pm_inbox::note_outgoing(&pub_key, text_str);
    }
    defmt::info!(
        "pm-compose: tx_send PM to={=[u8]:02x} bytes={=usize} ts={=u32}",
        &pub_key[..6],
        n,
        timestamp,
    );
    match tx_send(TxRequest::PrivateMsg(TxPrivateMsg {
        recipient_pub_key: pub_key,
        timestamp,
        text: payload,
        txt_type: 0,
        attempt: 0,
    })) {
        Ok(()) => defmt::info!("pm-compose: queued"),
        Err(_) => defmt::warn!("pm-compose: TX queue full — dropped"),
    }
}

/// Open the text-entry keyboard primed for sending a PM to `pub_key`.
/// Stash the recipient first so `on_pm_compose_done` can read it on
/// submit.  Cancelling the keyboard leaves the target stashed; the
/// next submit-or-dismiss clears it.
///
/// `pub` so the inbox-thread Reply action can reuse this flow.
pub fn start_pm_compose(pub_key: [u8; 32]) {
    PM_COMPOSE_TARGET.lock(|c| {
        *c.borrow_mut() = Some(pub_key);
    });
    // 130-byte limit matches MeshCore's `MAX_TXT_TEXT_SIZE` after
    // accounting for the 5-byte header (`timestamp[4] | flags[1]`).
    crate::text_entry::begin(b"", 130, on_pm_compose_done, "PM", false);
}

// ── Contact-store mutation queue ───────────────────────────────────────────
//
// The popup's Save / Unsave / Forget actions need to write to the
// persistent ContactStore — but `dispatch()` is sync and the kv ops are
// async.  Push the requested change here; `mutation_persister_task`
// drains the channel and applies them.  After each successful write
// it marks the store dirty and nudges `REBUILD_SIGNAL` so the cache
// rebuild picks up the change.

/// Pending mutation against the persistent contact store.
pub enum Mutation {
    /// Set or clear the FAVORITE bit on `pub_key`.
    SetFavorite([u8; 32], bool),
    /// Remove the contact slot for `pub_key`.
    Forget([u8; 32]),
    /// Promote a discovery-cache entry into a persistent contact slot.
    /// The persister looks up the entry by `pub_key` and calls
    /// `add_or_update`; if the discovery entry has been evicted by the
    /// time the persister runs, the request is silently dropped.
    Add([u8; 32]),
}

pub static MUTATION_QUEUE: embassy_sync::channel::Channel<CriticalSectionRawMutex, Mutation, 4> =
    embassy_sync::channel::Channel::new();

/// Embassy task: serialise contact-store mutations from the Contacts
/// screen popup actions, then trigger a cache rebuild.
#[embassy_executor::task]
pub async fn mutation_persister_task() {
    loop {
        let req = MUTATION_QUEUE.receive().await;
        let store = ContactStore::new();
        match req {
            Mutation::SetFavorite(pk, fav) => {
                let _ = store.set_favorite(&pk, fav).await;
            }
            Mutation::Forget(pk) => {
                let _ = store.delete(&pk).await;
            }
            Mutation::Add(pk) => {
                if let Some(d) = super::discovery::get(&pk) {
                    let contact = super::contacts::Contact::from_advert(
                        d.pub_key,
                        d.name.as_bytes(),
                        d.node_type,
                        d.advert_ts,
                        d.gps_lat,
                        d.gps_lon,
                    );
                    let _ = store.add_or_update(&contact).await;
                }
            }
        }
        // The mutation methods already marked the store dirty; just
        // wake the refresh task.
        REBUILD_SIGNAL.signal(());
    }
}

/// Apply an in-place edit to the cached row matching `pub_key` so the
/// UI reflects the mutation instantly, before the persister task has
/// finished writing to flash.  Cheap — small heapless Vec scan.  No-op
/// when the key isn't present.
fn cached_with<F: FnOnce(&mut ContactRow)>(pub_key: &[u8; 32], f: F) {
    CACHED_CONTACTS.lock(|c| {
        if let Some(e) = c.borrow_mut().iter_mut().find(|e| &e.pub_key == pub_key) {
            f(e);
        }
    });
}

/// Remove the cached row for `pub_key` (used by the Forget action).
fn cached_remove(pub_key: &[u8; 32]) {
    CACHED_CONTACTS.lock(|c| {
        let mut list = c.borrow_mut();
        if let Some(pos) = list.iter().position(|e| &e.pub_key == pub_key) {
            list.remove(pos);
        }
    });
}

/// Look up a peer's display name from the cache.  `pm_inbox` calls
/// this so it doesn't need to reach into `CACHED_CONTACTS` directly.
pub fn lookup_peer_name(pub_key: &[u8; 32]) -> Option<heapless::String<32>> {
    CACHED_CONTACTS.lock(|c| {
        c.borrow()
            .iter()
            .find(|e| &e.pub_key == pub_key)
            .map(|e| e.name.clone())
    })
}

// ── Filtering ───────────────────────────────────────────────────────────────

/// Contact-list filter applied at render and dispatch time.  The cache
/// itself is unfiltered; we just walk it skipping non-matching rows.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Filter {
    All,
    Favorites,
    Chat,
    Repeaters,
    Rooms,
    Sensors,
}

impl Filter {
    /// Short name shown in the picker and (when not `All`) appended to
    /// the screen header — `Contacts · Repeaters`.
    fn label(self) -> &'static str {
        match self {
            Filter::All => "All",
            Filter::Favorites => "Favorites",
            Filter::Chat => "People",
            Filter::Repeaters => "Repeaters",
            Filter::Rooms => "Rooms",
            Filter::Sensors => "Sensors",
        }
    }

    /// Empty-state message when the filter has no matches yet.
    fn empty_msg(self) -> &'static str {
        match self {
            Filter::All => "Listening for adverts…",
            Filter::Favorites => "No favorites yet",
            Filter::Chat => "No people heard yet",
            Filter::Repeaters => "No repeaters heard",
            Filter::Rooms => "No rooms heard",
            Filter::Sensors => "No sensors heard",
        }
    }

    fn matches(self, e: &ContactRow) -> bool {
        match self {
            Filter::All => true,
            Filter::Favorites => e.is_favorite(),
            Filter::Chat => e.node_type == 1,
            Filter::Repeaters => e.node_type == 2,
            Filter::Rooms => e.node_type == 3,
            Filter::Sensors => e.node_type == 4,
        }
    }
}

/// All filter variants in the order they appear in the picker.
const FILTERS: [Filter; 6] = [
    Filter::All,
    Filter::Favorites,
    Filter::Chat,
    Filter::Repeaters,
    Filter::Rooms,
    Filter::Sensors,
];

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
    /// Filter picker overlay — `pos` is the highlighted filter option.
    FilterPicker { pos: u8 },
}

pub struct ContactsState {
    mode: Mode,
    /// Active list filter.  Applied on top of `CACHED_CONTACTS` at
    /// render/dispatch time.  Resets on screen exit (per design — the
    /// default is discovery-first "All").
    filter: Filter,
    /// Cursor row within the *filtered* view (not the underlying cache).
    cursor: u8,
    /// First visible filtered-row index.
    scroll: u8,
}

impl ContactsState {
    const fn new() -> Self {
        Self {
            mode: Mode::List,
            filter: Filter::All,
            cursor: 0,
            scroll: 0,
        }
    }
}

pub static BROWSER: Mutex<CriticalSectionRawMutex, RefCell<ContactsState>> =
    Mutex::new(RefCell::new(ContactsState::new()));

/// Reset cursor/scroll to the top.  Called when the user navigates away.
pub fn reset() {
    BROWSER.lock(|cell| {
        *cell.borrow_mut() = ContactsState::new();
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
    use core::sync::atomic::Ordering;

    // Fast path — when the persistent store hasn't been mutated since
    // the last rebuild, skip the 300-slot kv rescan entirely.  Adverts
    // populate `OBSERVATIONS` and the discovery cache directly (both
    // pure RAM); the saved rows already in `CACHED_CONTACTS` only need
    // their `observed_at_secs` refreshed and the discovery overlay
    // re-merged.  Saves ≈ 300 ms QSPI + CPU per advert burst, which is
    // the dominant new energy actor in the Contacts screen.
    if !STORE_DIRTY.swap(false, Ordering::Relaxed) {
        refresh_cache_fast();
        return;
    }

    // Slow path — full rebuild.  Used after a contact-store mutation
    // (Save/Forget/Add via the popup, or `meshcore::log_advert`'s
    // auto-add succeeding).
    let store = ContactStore::new();
    let mut top: heapless::Vec<ContactRow, CACHE_CAP> = heapless::Vec::new();

    // Pass 1: persistent ContactStore.
    for idx in 0..MAX_CONTACTS {
        let Some(c) = store.read_slot(idx).await else {
            continue;
        };
        if c.is_deleted() {
            continue;
        }
        let observed = lookup_observation(&c.pub_key);
        let e = ContactRow::from_contact(&c, observed);
        insert_sorted(&mut top, e);
    }

    // Pass 2: discovery-cache entries that aren't already persisted.
    super::discovery::for_each(|d| {
        if top.iter().any(|x| x.pub_key == d.pub_key) {
            return;
        }
        insert_sorted(&mut top, ContactRow::from_discovery(d));
    });

    CACHED_CONTACTS.lock(|cell| {
        let mut list = cell.borrow_mut();
        list.clear();
        for e in top.iter() {
            let _ = list.push(e.clone());
        }
    });
}

/// In-RAM-only cache refresh: keep the existing saved rows, refresh
/// their `observed_at_secs` from `OBSERVATIONS`, and re-merge the
/// discovery overlay.  No flash I/O.
fn refresh_cache_fast() {
    let mut top: heapless::Vec<ContactRow, CACHE_CAP> = heapless::Vec::new();

    // Carry over saved rows from the existing cache, refreshing each
    // row's observed_at_secs.  Drop discovery rows — they're rebuilt
    // below from the (possibly-aged) discovery snapshot.
    CACHED_CONTACTS.lock(|cell| {
        for row in cell.borrow().iter().filter(|r| r.is_saved) {
            let mut r = row.clone();
            r.observed_at_secs = lookup_observation(&r.pub_key);
            insert_sorted(&mut top, r);
        }
    });

    // Re-merge discovery entries that aren't shadowed by a saved row.
    super::discovery::for_each(|d| {
        if top.iter().any(|x| x.pub_key == d.pub_key) {
            return;
        }
        insert_sorted(&mut top, ContactRow::from_discovery(d));
    });

    CACHED_CONTACTS.lock(|cell| {
        let mut list = cell.borrow_mut();
        list.clear();
        for e in top.iter() {
            let _ = list.push(e.clone());
        }
    });
}

/// Insert `e` into a sorted-desc-by-recency cache, dropping the worst
/// entry when the cache is full.  Insertion-sort: cheap because
/// `CACHE_CAP` is tiny.
fn insert_sorted(top: &mut heapless::Vec<ContactRow, CACHE_CAP>, e: ContactRow) {
    let e_key = sort_key(&e);
    let pos = top
        .iter()
        .position(|x| sort_key(x) < e_key)
        .unwrap_or(top.len());
    if pos >= CACHE_CAP {
        return;
    }
    if top.len() == CACHE_CAP {
        let _ = top.pop();
    }
    let _ = top.insert(pos, e);
}

/// Sort key for the discovery list.  Entries observed this session win
/// on the high u64 bit; their relative order uses the local observation
/// time.  Everything else falls back to the (possibly-zero) advert
/// timestamp.
fn sort_key(e: &ContactRow) -> u64 {
    match e.observed_at_secs {
        Some(s) => (1u64 << 63) | s,
        None => e.last_advert_ts as u64,
    }
}

/// Embassy task: rebuild the cache when an advert arrives or a popup
/// action mutates the persistent store.  Debounces bursts by waiting
/// for a quiet 1 s window before each rebuild.
///
/// Waits on its own [`REBUILD_SIGNAL`] rather than `ADVERT_SIGNAL`
/// because `embassy_sync::Signal` is single-waiter — sharing one
/// signal with the UI redraw loop in `bin/embassy.rs` causes wakes
/// to be silently dropped on whichever side last registered its
/// waker.  `meshcore::log_advert` and `mutation_persister_task`
/// signal both.
#[embassy_executor::task]
pub async fn refresh_cache_task() {
    use embassy_time::{Duration, Timer, with_timeout};
    // Initial population at boot — full rescan since the cache is
    // empty.  STORE_DIRTY starts true so this naturally takes the
    // slow path.
    refresh_cache().await;
    loop {
        REBUILD_SIGNAL.wait().await;
        loop {
            match with_timeout(Duration::from_millis(1000), REBUILD_SIGNAL.wait()).await {
                Ok(()) => continue,
                Err(_) => break,
            }
        }
        refresh_cache().await;
        Timer::after(Duration::from_millis(50)).await;
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────────

fn live_now(observed_at_secs: Option<u64>) -> bool {
    let Some(seen) = observed_at_secs else {
        return false;
    };
    let now = embassy_time::Instant::now().as_secs();
    now.saturating_sub(seen) <= LIVE_WINDOW_SECS
}

/// Render a relative time delta as a short string: `now`, `3m`, `42m`,
/// `5h`, `ydy`, `3d`, `2w`.  Empty when we haven't heard this contact
/// during the current boot — the list is sorted by recency so ordering
/// already conveys "newer above older."
fn fmt_relative(observed_at_secs: Option<u64>) -> heapless::String<8> {
    let Some(seen) = observed_at_secs else {
        return heapless::String::new();
    };
    let now = embassy_time::Instant::now().as_secs();
    super::time_fmt::fmt_relative_secs(now.saturating_sub(seen))
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

/// Per-contact popup action.  Picked from a vec returned by
/// `popup_items` and dispatched by the popup `Fire` arm.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PopupAction {
    Pm,
    Info,
    Save,
    Unsave,
    Forget,
    Add,
    Cancel,
}

impl PopupAction {
    fn label(self) -> &'static str {
        match self {
            PopupAction::Pm => "PM",
            PopupAction::Info => "Info",
            PopupAction::Save => "Save",
            PopupAction::Unsave => "Unsave",
            PopupAction::Forget => "Forget",
            PopupAction::Add => "Add",
            PopupAction::Cancel => "< Cancel",
        }
    }
}

/// Maximum popup item count — chat-node saved row has 5 (PM, Info,
/// Save/Unsave, Forget, Cancel); reserve one slot of headroom.
const MAX_POPUP_ITEMS: usize = 6;

/// Role- and saved-state-aware popup item set.  Index 0 is the primary
/// action and is preselected.  Discovery rows (`is_saved == false`)
/// expose **Add** as the primary action; saved rows expose Save / Forget
/// curation.
fn popup_items(
    node_type: u8,
    is_saved: bool,
    is_favorite: bool,
) -> heapless::Vec<PopupAction, MAX_POPUP_ITEMS> {
    let mut v: heapless::Vec<PopupAction, MAX_POPUP_ITEMS> = heapless::Vec::new();
    if !is_saved {
        // Heard-but-not-saved.  Add is primary; PM still works (we
        // have the pub_key) for chat nodes even before Adding.
        let _ = v.push(PopupAction::Add);
        if node_type == 1 {
            let _ = v.push(PopupAction::Pm);
        }
        let _ = v.push(PopupAction::Info);
        let _ = v.push(PopupAction::Cancel);
        return v;
    }
    let fav = if is_favorite {
        PopupAction::Unsave
    } else {
        PopupAction::Save
    };
    if node_type == 1 {
        // Chat Node — PM is the most common action.
        let _ = v.push(PopupAction::Pm);
        let _ = v.push(PopupAction::Info);
        let _ = v.push(fav);
        let _ = v.push(PopupAction::Forget);
        let _ = v.push(PopupAction::Cancel);
    } else {
        // Repeater / Room Server / Sensor / Unknown — no DM action.
        let _ = v.push(PopupAction::Info);
        let _ = v.push(fav);
        let _ = v.push(PopupAction::Forget);
        let _ = v.push(PopupAction::Cancel);
    }
    v
}

// ── Filtered-view helpers ──────────────────────────────────────────────────

/// Number of cached entries matching `filter`.
fn filtered_count(filter: Filter) -> u8 {
    CACHED_CONTACTS.lock(|c| {
        c.borrow()
            .iter()
            .filter(|e| filter.matches(e))
            .count()
            .min(255) as u8
    })
}

/// Clone the `idx`-th filtered entry (0-indexed within the filtered
/// view).  Returns `None` when out of bounds.
fn filtered_get(filter: Filter, idx: u8) -> Option<ContactRow> {
    CACHED_CONTACTS.lock(|c| {
        c.borrow()
            .iter()
            .filter(|e| filter.matches(e))
            .nth(idx as usize)
            .cloned()
    })
}

// ── Input dispatch ──────────────────────────────────────────────────────────

/// Handle a button press.  Returns `true` when Cancel should propagate to
/// the menu layer (i.e., we want to leave the Contacts screen entirely).
pub fn dispatch(btn: ButtonId) -> bool {
    BROWSER.lock(|cell| {
        let mut b = cell.borrow_mut();
        let filter = b.filter;
        let count = filtered_count(filter);
        match b.mode {
            Mode::List => match btn {
                ButtonId::Up => {
                    if b.cursor > 0 {
                        b.cursor -= 1;
                        if b.cursor < b.scroll {
                            b.scroll = b.cursor;
                        }
                    } else {
                        // Already at the top — overflow into the filter
                        // picker.  Pressing Up at row 0 was a no-op
                        // before, so this is a free gesture without
                        // burning a button.
                        let pos = FILTERS.iter().position(|f| *f == filter).unwrap_or(0) as u8;
                        b.mode = Mode::FilterPicker { pos };
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
                let entry_meta = filtered_get(filter, target)
                    .map(|e| (e.pub_key, e.node_type, e.is_saved, e.is_favorite()));
                let items = entry_meta
                    .map(|(_, nt, saved, fav)| popup_items(nt, saved, fav))
                    .unwrap_or_default();
                let n = items.len() as u8;
                match btn {
                    ButtonId::Up => {
                        b.mode = Mode::Popup {
                            target,
                            // Wrap to the last item instead of doing
                            // nothing at the top.
                            pos: if pos > 0 { pos - 1 } else { n.saturating_sub(1) },
                        };
                        false
                    }
                    ButtonId::Down => {
                        b.mode = Mode::Popup {
                            target,
                            // Wrap to the top instead of doing nothing at
                            // the bottom.
                            pos: if pos + 1 < n { pos + 1 } else { 0 },
                        };
                        false
                    }
                    ButtonId::Fire | ButtonId::Execute => {
                        let action = items.get(pos as usize).copied();
                        match (action, entry_meta) {
                            (Some(PopupAction::Pm), Some((pk, ..))) => {
                                defmt::info!("popup: PM → start compose to {=[u8]:02x}", &pk[..6]);
                                b.mode = Mode::List;
                                start_pm_compose(pk);
                            }
                            (Some(PopupAction::Info), _) => {
                                b.mode = Mode::Detail { target };
                            }
                            (Some(PopupAction::Save), Some((pk, ..))) => {
                                cached_with(&pk, |e| e.flags |= FLAG_FAVORITE);
                                let _ = MUTATION_QUEUE.try_send(Mutation::SetFavorite(pk, true));
                                b.mode = Mode::List;
                            }
                            (Some(PopupAction::Unsave), Some((pk, ..))) => {
                                cached_with(&pk, |e| e.flags &= !FLAG_FAVORITE);
                                let _ = MUTATION_QUEUE.try_send(Mutation::SetFavorite(pk, false));
                                b.mode = Mode::List;
                            }
                            (Some(PopupAction::Forget), Some((pk, ..))) => {
                                cached_remove(&pk);
                                let _ = MUTATION_QUEUE.try_send(Mutation::Forget(pk));
                                let new_count = filtered_count(filter);
                                if b.cursor >= new_count {
                                    b.cursor = new_count.saturating_sub(1);
                                }
                                if b.scroll > b.cursor {
                                    b.scroll = b.cursor;
                                }
                                b.mode = Mode::List;
                            }
                            (Some(PopupAction::Add), Some((pk, ..))) => {
                                cached_with(&pk, |e| e.is_saved = true);
                                let _ = MUTATION_QUEUE.try_send(Mutation::Add(pk));
                                b.mode = Mode::List;
                            }
                            _ => {
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
                    // Next contact within the filtered view (clamped).
                    if target + 1 < count {
                        b.mode = Mode::Detail { target: target + 1 };
                    }
                    false
                }
                ButtonId::Fire | ButtonId::Execute => {
                    // Open PM compose if this contact is a chat node.
                    if let Some(entry) = filtered_get(filter, target)
                        && entry.node_type == 1
                    {
                        b.mode = Mode::List;
                        start_pm_compose(entry.pub_key);
                    }
                    false
                }
                _ => false,
            },

            Mode::FilterPicker { pos } => {
                let n = FILTERS.len() as u8;
                match btn {
                    ButtonId::Up => {
                        // Wrap to the last filter instead of doing nothing
                        // at the top.
                        b.mode = Mode::FilterPicker {
                            pos: if pos > 0 { pos - 1 } else { n.saturating_sub(1) },
                        };
                        false
                    }
                    ButtonId::Down => {
                        // Wrap to the top instead of doing nothing at the
                        // bottom.
                        b.mode = Mode::FilterPicker {
                            pos: if pos + 1 < n { pos + 1 } else { 0 },
                        };
                        false
                    }
                    ButtonId::Fire | ButtonId::Execute => {
                        // Commit the picked filter and reset cursor/scroll
                        // since the visible row count changes.
                        b.filter = FILTERS[pos as usize];
                        b.cursor = 0;
                        b.scroll = 0;
                        b.mode = Mode::List;
                        false
                    }
                    ButtonId::Cancel => {
                        b.mode = Mode::List;
                        false
                    }
                    ButtonId::Left | ButtonId::Right => false,
                }
            }
        }
    })
}

// ── Render ──────────────────────────────────────────────────────────────────

pub fn draw<D>(display: &mut D, bat_prc: &u8) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let (cursor, scroll, mode, filter) = BROWSER.lock(|c| {
        let s = c.borrow();
        (s.cursor, s.scroll, s.mode, s.filter)
    });

    // Header — append filter name when not All.  20-char budget at 7×13
    // bold (152 / 7 ≈ 21) is just enough; the longest combo is
    // "Contacts · Repeaters" = 20.
    if filter == Filter::All {
        draw_header(display, "Contacts", bat_prc)?;
    } else {
        let mut title: heapless::String<24> = heapless::String::new();
        let _ = title.push_str("Contacts · ");
        let _ = title.push_str(filter.label());
        draw_header(display, title.as_str(), bat_prc)?;
    }

    // Empty state — use the filter-specific message so the user knows
    // *why* the list is blank.
    if filtered_count(filter) == 0 {
        ui::draw_centered_message(display, filter.empty_msg(), Point::new(76, 80))?;
        // A FilterPicker overlay can still be active even when the list
        // is empty, so render it on top.
        if let Mode::FilterPicker { pos } = mode {
            draw_filter_picker(display, pos)?;
        }
        return Ok(());
    }

    draw_list(display, cursor, scroll, filter)?;

    // Overlays — drawn after the list so they sit on top.
    match mode {
        Mode::List => {}
        Mode::Popup { target, pos } => draw_popup(display, target, pos, filter)?,
        Mode::Detail { target } => draw_detail(display, target, filter)?,
        Mode::FilterPicker { pos } => draw_filter_picker(display, pos)?,
    }

    Ok(())
}

fn draw_list<D>(display: &mut D, cursor: u8, scroll: u8, filter: Filter) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let list_top: i32 = ui::TITLE_BAR_H as i32 + 2;
    let bottom = TextStyleBuilder::new().baseline(Baseline::Bottom).build();
    let right = TextStyleBuilder::new()
        .baseline(Baseline::Bottom)
        .alignment(Alignment::Right)
        .build();

    CACHED_CONTACTS.lock(|c| -> Result<(), D::Error> {
        let list = c.borrow();
        // Walk the filtered view: keep a counter of matched-and-skipped
        // entries and only render rows in the [scroll .. scroll +
        // VISIBLE_ROWS) window.
        let mut filtered_idx: u8 = 0;
        let mut painted: u8 = 0;
        for entry in list.iter() {
            if !filter.matches(entry) {
                continue;
            }
            if filtered_idx < scroll {
                filtered_idx += 1;
                continue;
            }
            if painted >= VISIBLE_ROWS {
                break;
            }
            let screen_row = painted;
            let row_top = list_top + screen_row as i32 * ROW_H;
            let row_mid = row_top + ROW_H / 2;
            let selected = filtered_idx == cursor;
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
            if live_now(entry.observed_at_secs) {
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

            // Name with optional prefix glyph.  Mutually exclusive:
            //  * saved + favorite          → "*" prefix
            //  + unsaved (discovery row)   → "+" prefix (Add to use)
            //  (none)                       → just the name
            let name = entry.name.as_str();
            let display_name = if name.is_empty() { "(unknown)" } else { name };
            let truncated = crate::truncate_str(display_name, 14);
            let prefix: Option<&'static str> = if entry.is_favorite() {
                Some("*")
            } else if !entry.is_saved {
                Some("+")
            } else {
                None
            };
            let name_offset = if prefix.is_some() { 8 } else { 0 };
            if let Some(g) = prefix {
                Text::with_text_style(g, Point::new(name_x, row_mid + 5), txt_style, bottom)
                    .draw(display)?;
            }
            Text::with_text_style(
                truncated,
                Point::new(name_x + name_offset, row_mid + 5),
                txt_style,
                bottom,
            )
            .draw(display)?;

            // Last-seen, right-aligned.
            let rel = fmt_relative(entry.observed_at_secs);
            Text::with_text_style(rel.as_str(), Point::new(150, row_mid + 5), txt_style, right)
                .draw(display)?;

            painted += 1;
            filtered_idx += 1;
        }
        Ok(())
    })
}

fn draw_popup<D>(display: &mut D, target: u8, pos: u8, filter: Filter) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    // Resolve title (contact name, truncated) and the role-aware items.
    let (title, items_owned) = match filtered_get(filter, target) {
        Some(entry) => {
            let mut t: heapless::String<16> = heapless::String::new();
            let n = crate::truncate_str(entry.name.as_str(), 14);
            let _ = t.push_str(if n.is_empty() { "(unknown)" } else { n });
            (
                t,
                popup_items(entry.node_type, entry.is_saved, entry.is_favorite()),
            )
        }
        None => (
            heapless::String::<16>::new(),
            heapless::Vec::<PopupAction, MAX_POPUP_ITEMS>::new(),
        ),
    };

    let labels: heapless::Vec<&str, MAX_POPUP_ITEMS> =
        items_owned.iter().map(|a| a.label()).collect();
    ui::draw_picker_menu(display, title.as_str(), labels.as_slice(), pos as usize)?;
    Ok(())
}

fn draw_detail<D>(display: &mut D, target: u8, filter: Filter) -> Result<(), D::Error>
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

    let Some(entry) = filtered_get(filter, target) else {
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
    let rel = fmt_relative(entry.observed_at_secs);
    let line = format!(20; "Last: {}", rel.as_str()).unwrap_or_default();
    Text::with_text_style(line.as_str(), Point::new(4, 66), style_small, bottom).draw(display)?;

    // Hops — `out_path_len` encodes hop count in bits 5-0.
    // `OUT_PATH_UNKNOWN` (0xFF) means we don't know a route yet (only
    // flood works to reach this contact).  0 = direct neighbour.
    let hops_line = if entry.out_path_len == OUT_PATH_UNKNOWN {
        let mut s: heapless::String<20> = heapless::String::new();
        let _ = s.push_str("Hops: ?");
        s
    } else {
        let n = entry.out_path_len & 0x3F;
        if n == 0 {
            let mut s: heapless::String<20> = heapless::String::new();
            let _ = s.push_str("Hops: 0 (direct)");
            s
        } else {
            format!(20; "Hops: {}", n).unwrap_or_default()
        }
    };
    Text::with_text_style(hops_line.as_str(), Point::new(4, 82), style_small, bottom)
        .draw(display)?;

    // Key prefix (8 bytes hex) on a single line — `"Key: " + 16 hex
    // chars` = 21 chars × 7 px = 147 px, fits the 152-px display.
    let mut key_line: heapless::String<24> = heapless::String::new();
    let _ = key_line.push_str("Key: ");
    let _ = key_line.push_str(crate::hex_prefix(&entry.pub_key, 8).as_str());
    Text::with_text_style(key_line.as_str(), Point::new(4, 100), style_small, bottom)
        .draw(display)?;

    // GPS — only shown when broadcast.  3-decimal precision (~100 m)
    // keeps `GPS: 55.612N 12.999E` to 20 chars × 7 px = 140 px on a
    // 152-px display.
    if entry.gps_lat != 0 || entry.gps_lon != 0 {
        // unsigned_abs: gps_lat/lon come from untrusted adverts; i32::MIN.abs()
        // would overflow-panic.
        let lat_deg = entry.gps_lat.unsigned_abs() / 1_000_000;
        let lat_frac = (entry.gps_lat.unsigned_abs() % 1_000_000) / 1000;
        let lat_hem = if entry.gps_lat >= 0 { 'N' } else { 'S' };
        let lon_deg = entry.gps_lon.unsigned_abs() / 1_000_000;
        let lon_frac = (entry.gps_lon.unsigned_abs() % 1_000_000) / 1000;
        let lon_hem = if entry.gps_lon >= 0 { 'E' } else { 'W' };
        let gps = format!(24;
            "GPS: {}.{:03}{} {}.{:03}{}",
            lat_deg, lat_frac, lat_hem, lon_deg, lon_frac, lon_hem
        )
        .unwrap_or_default();
        Text::with_text_style(gps.as_str(), Point::new(4, 116), style_small, bottom)
            .draw(display)?;
    }

    // Footer hint
    Text::with_text_style("Cancel: back", Point::new(4, 148), style_small, bottom).draw(display)?;

    Ok(())
}

fn draw_filter_picker<D>(display: &mut D, pos: u8) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    // Build the label list once, in the same order as `FILTERS` so the
    // picker cursor aligns with the dispatch logic.
    let labels: heapless::Vec<&'static str, 6> = FILTERS.iter().map(|f| f.label()).collect();
    ui::draw_picker_menu(display, "Filter", labels.as_slice(), pos as usize)?;
    Ok(())
}
