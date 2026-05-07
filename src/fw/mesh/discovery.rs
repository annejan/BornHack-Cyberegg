//! Recent-adverts cache for the on-device Contacts screen.
//!
//! Holds adverts heard during this boot regardless of the auto-add policy.
//! With auto-add off (the firmware default) advertisements would otherwise
//! evaporate after [`meshcore::log_advert`] processes them — this cache
//! keeps the metadata around so the user can see them on the Contacts
//! screen and **Add** them to the persistent [`ContactStore`].
//!
//! This complements the lighter [`OBSERVATIONS`] ring inside
//! [`contacts_screen`] — that one is keyed by `pub_key` only and serves
//! the "Last:" relative-time/live-dot rendering.  Here we keep the full
//! advert metadata (name, role, GPS) needed to *promote* an entry into
//! the contact store.
//!
//! Bounded ring (`CAP`) of recent observations; oldest entry is evicted
//! when full.  Per-boot — clears on reboot.  Entries that successfully
//! land in `ContactStore` are intentionally kept here too; the Contacts
//! screen merges by `pub_key` and prefers the saved record, so the
//! duplication is harmless and avoids an extra "remove from discovery"
//! round-trip.
//!
//! [`OBSERVATIONS`]: super::contacts_screen
//! [`ContactStore`]: super::contacts::ContactStore

use core::cell::RefCell;

use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;

const CAP: usize = 32;

/// One advert's display-and-add metadata.  Sized for the Contacts-screen
/// row + Add action — enough to build a `Contact` via
/// [`Contact::from_advert`].
#[derive(Clone)]
pub struct DiscoveryEntry {
    pub pub_key: [u8; 32],
    pub name: heapless::String<32>,
    /// `ADV_TYPE_*` (1=ChatNode, 2=Repeater, 3=RoomServer, 4=Sensor).
    pub node_type: u8,
    /// GPS latitude in microdegrees (1e-6°).  `0` = unset.
    pub gps_lat: i32,
    /// GPS longitude in microdegrees.  `0` = unset.
    pub gps_lon: i32,
    /// Sender's claimed timestamp (advert payload).  Forwarded straight
    /// to `Contact::from_advert` if the user later promotes this entry.
    pub advert_ts: u32,
    /// Seconds-since-boot when *we* last heard this advert.  Source of
    /// truth for sort order on the Contacts screen.
    pub observed_at_secs: u64,
}

static CACHE: Mutex<CriticalSectionRawMutex, RefCell<heapless::Vec<DiscoveryEntry, CAP>>> =
    Mutex::new(RefCell::new(heapless::Vec::new()));

/// Record (or refresh) a recently-heard advert.  Called from
/// `meshcore::log_advert` for every received advert — the Contacts
/// screen handles deduplication against the persistent store at render
/// time.
pub fn note(
    pub_key: &[u8; 32],
    name: &str,
    node_type: u8,
    gps_lat: i32,
    gps_lon: i32,
    advert_ts: u32,
) {
    let observed_at_secs = embassy_time::Instant::now().as_secs();
    CACHE.lock(|cell| {
        let mut list = cell.borrow_mut();
        // Same key already present: just refresh observation time + any
        // metadata that may have changed (name, GPS).
        if let Some(e) = list.iter_mut().find(|e| &e.pub_key == pub_key) {
            e.observed_at_secs = observed_at_secs;
            e.node_type = node_type;
            e.gps_lat = gps_lat;
            e.gps_lon = gps_lon;
            e.advert_ts = advert_ts;
            e.name.clear();
            let _ = e.name.push_str(name);
            return;
        }
        // New key — append, evicting the oldest by observed time.
        if list.is_full() {
            let oldest_idx = list
                .iter()
                .enumerate()
                .min_by_key(|(_, e)| e.observed_at_secs)
                .map(|(i, _)| i)
                .unwrap_or(0);
            let _ = list.swap_remove(oldest_idx);
        }
        let mut name_buf: heapless::String<32> = heapless::String::new();
        let _ = name_buf.push_str(name);
        let _ = list.push(DiscoveryEntry {
            pub_key: *pub_key,
            name: name_buf,
            node_type,
            gps_lat,
            gps_lon,
            advert_ts,
            observed_at_secs,
        });
    });
}

/// Look up a discovery entry by `pub_key`.  Returns a clone so the
/// caller can release the lock before doing async work.
pub fn get(pub_key: &[u8; 32]) -> Option<DiscoveryEntry> {
    CACHE.lock(|cell| {
        cell.borrow()
            .iter()
            .find(|e| &e.pub_key == pub_key)
            .cloned()
    })
}

/// Run `visit` on each entry currently in the cache, holding the lock
/// for the whole call.  Used by the Contacts-screen cache rebuild.
pub fn for_each<F: FnMut(&DiscoveryEntry)>(mut visit: F) {
    CACHE.lock(|cell| {
        for e in cell.borrow().iter() {
            visit(e);
        }
    });
}
