//! Persistent contact store backed by the KV flash store.
//!
//! Contacts are stored in up to [`MAX_CONTACTS`] fixed-size slots under the
//! `"contacts"` KV namespace.  A metadata record tracks the ring-buffer write
//! head and the current non-deleted contact count.  If the firmware is flashed
//! with a different [`MAX_CONTACTS`] value, a one-time migration runs on the
//! first boot to bring the on-flash state in line with the new constant.
//!
//! ## Slot key format
//!
//! Each slot is stored as `"contacts:NNNN"` where NNNN is a zero-padded
//! decimal index (e.g. `"0000"` … `"0699"`).  Keys are hashed by the KV layer
//! so the exact string format only matters for human readability.
//!
//! ## Slot layout (148 bytes each)
//!
//! Path size matches MeshCore `MAX_PATH_SIZE` = 64 B.
//! Offsets verified against `MyMesh::updateContactFromFrame()` in the MeshCore reference firmware.
//!
//! | Offset | Bytes | Field           | Notes                         |
//! |--------|-------|-----------------|-------------------------------|
//! |   0    |  32   | `pub_key`       | All-zeros = deleted slot      |
//! |  32    |   1   | `node_type`     | ADV_TYPE_* (1–4)              |
//! |  33    |   1   | `flags`         | Bit 0 = [`FLAG_FAVORITE`]     |
//! |  34    |   1   | `out_path_len`  | [`OUT_PATH_UNKNOWN`] = 0xFF   |
//! |  35    |   1   | _(pad)_         |                               |
//! |  36    |  64   | `out_path`      | Zero-filled when unknown      |
//! | 100    |  32   | `name`          | UTF-8, zero-padded            |
//! | 132    |   4   | `last_advert_ts`| LE u32 — contact's clock     |
//! | 136    |   4   | `gps_lat`       | LE i32, microdegrees          |
//! | 140    |   4   | `gps_lon`       | LE i32, microdegrees          |
//! | 144    |   4   | `lastmod`       | LE u32 — used for eviction    |
//!
//! ## Metadata (`"contacts:meta"`, 8 bytes)
//!
//! | Offset | Bytes | Field      | Notes                              |
//! |--------|-------|------------|------------------------------------|
//! |   0    |   2   | `capacity` | [`MAX_CONTACTS`] at last write     |
//! |   2    |   2   | `head`     | Ring-buffer next-write index       |
//! |   4    |   2   | `count`    | Non-deleted contacts currently held|
//! |   6    |   2   | _(pad)_    |                                    |

use crate::fw::kv;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Maximum number of contacts stored on-device.
///
/// Change this constant to resize the store.  The first boot after a change
/// will automatically migrate existing data: growing keeps all contacts,
/// shrinking discards slots beyond the new limit.
pub const MAX_CONTACTS: usize = 300;

const _: () = assert!(
    MAX_CONTACTS <= 9999,
    "MAX_CONTACTS exceeds the 4-digit slot key format"
);

/// Routing path size in bytes — matches MeshCore `MAX_PATH_SIZE`.
pub const MAX_PATH_SIZE: usize = 64;

/// Bit 0 of the `flags` field — contact is marked as favourite.
///
/// Favourite contacts are never evicted until all non-favourite slots are
/// exhausted.  If the store is entirely favourites the oldest favourite is
/// overwritten as a last resort.
pub const FLAG_FAVORITE: u8 = 0x01;

/// Sentinel `out_path_len` meaning "no routing path established yet".
pub const OUT_PATH_UNKNOWN: u8 = 0xFF;

// ---------------------------------------------------------------------------
// Serialised sizes — defined explicitly, not derived from struct layout,
// to avoid any compiler-inserted padding changing the on-flash format.
// ---------------------------------------------------------------------------

const CONTACT_SIZE: usize = 148; // 32+1+1+1+1+64+32+4+4+4+4
const META_SIZE: usize = 8;

// ---------------------------------------------------------------------------
// Contact
// ---------------------------------------------------------------------------

/// One stored contact entry.
///
/// When `pub_key` is all-zeros the slot has been deleted and its storage
/// may be reused by the next [`ContactStore::add_or_update`] call.
#[derive(Clone)]
pub struct Contact {
    /// Ed25519 public key.  All-zeros = deleted slot.
    pub pub_key: [u8; 32],
    /// Node role: 1 = ChatNode, 2 = Repeater, 3 = RoomServer, 4 = Sensor.
    pub node_type: u8,
    /// Contact flags — bit 0 is [`FLAG_FAVORITE`].
    pub flags: u8,
    /// Routing path length.  [`OUT_PATH_UNKNOWN`] (0xFF) = not yet established.
    pub out_path_len: u8,
    _pad: u8,
    /// Routing path bytes — zero-filled when `out_path_len == OUT_PATH_UNKNOWN`.
    /// Always stored as `MAX_PATH_SIZE` (64 B) on the wire, matching MeshCore.
    pub out_path: [u8; MAX_PATH_SIZE],
    /// Display name, UTF-8, zero-padded to 32 bytes.
    pub name: [u8; 32],
    /// Timestamp from the contact's last advertisement (contact's own clock).
    pub last_advert_ts: u32,
    /// GPS latitude in microdegrees (0 = not set).
    pub gps_lat: i32,
    /// GPS longitude in microdegrees (0 = not set).
    pub gps_lon: i32,
    /// Last-modified timestamp on our device clock.
    ///
    /// Used as the eviction key: the contact with the smallest `lastmod`
    /// (among non-favourites) is overwritten when the store is full.
    pub lastmod: u32,
}

impl Contact {
    /// Number of bytes in `out_path` that are valid, given `out_path_len`.
    ///
    /// MeshCore path_len_byte encoding: bits 7-6 = hash_size_code (0→1B, 1→2B, 2→3B),
    /// bits 5-0 = hop_count.  Actual bytes = hop_count × (hash_size_code + 1).
    /// Returns 0 when `out_path_len` is [`OUT_PATH_UNKNOWN`] or invalid.
    pub fn path_actual_bytes(&self) -> usize {
        if self.out_path_len == OUT_PATH_UNKNOWN {
            return 0;
        }
        let hop_count = (self.out_path_len & 0x3F) as usize;
        let hash_size = ((self.out_path_len >> 6) as usize) + 1;
        if hash_size == 4 {
            return 0;
        } // reserved
        (hop_count * hash_size).min(MAX_PATH_SIZE)
    }

    /// `true` when this slot holds no valid contact (pub_key all-zeros).
    pub fn is_deleted(&self) -> bool {
        self.pub_key == [0u8; 32]
    }

    /// `true` when [`FLAG_FAVORITE`] is set.
    pub fn is_favorite(&self) -> bool {
        self.flags & FLAG_FAVORITE != 0
    }

    /// Name bytes with trailing zero padding stripped.
    pub fn name_bytes(&self) -> &[u8] {
        let end = self
            .name
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(self.name.len());
        &self.name[..end]
    }

    /// Parse from the raw `AddUpdateContact` (0x09) BLE payload.
    ///
    /// `buf` is the full write buffer starting with the 0x09 command byte.
    /// Returns `None` if the buffer is shorter than the 136-byte minimum.
    ///
    /// Wire layout (MeshCore `MAX_PATH_SIZE` = 64 B):
    /// ```text
    /// d[0..32]   pub_key
    /// d[32]      node_type
    /// d[33]      flags
    /// d[34]      out_path_len
    /// d[35..99]  out_path   (64 B, always present)
    /// d[99..131] name       (32 B)
    /// d[131..135] last_advert_ts  (LE u32)
    /// d[135..139] gps_lat         (LE i32, optional)
    /// d[139..143] gps_lon         (LE i32, optional)
    /// d[143..147] lastmod         (LE u32, optional)
    /// ```
    pub fn from_add_update_payload(buf: &[u8]) -> Option<Self> {
        // Minimum: 1 (cmd) + 32 + 1 + 1 + 1 + 64 + 32 + 4 = 136
        if buf.len() < 136 {
            return None;
        }
        let d = &buf[1..]; // skip command byte; d.len() >= 135
        let pub_key: [u8; 32] = d[0..32].try_into().ok()?;
        let node_type = d[32];
        let flags = d[33];
        let out_path_len = d[34];
        let out_path: [u8; MAX_PATH_SIZE] = d[35..99].try_into().ok()?;
        let name: [u8; 32] = d[99..131].try_into().ok()?;
        let last_advert_ts = u32::from_le_bytes([d[131], d[132], d[133], d[134]]);
        // MeshCore sends lat+lon together when frame.len >= 144 (d.len >= 143).
        let gps_lat = if d.len() >= 143 {
            i32::from_le_bytes([d[135], d[136], d[137], d[138]])
        } else {
            0
        };
        let gps_lon = if d.len() >= 143 {
            i32::from_le_bytes([d[139], d[140], d[141], d[142]])
        } else {
            0
        };
        // lastmod present when frame.len >= 148 (d.len >= 147).
        let lastmod = if d.len() >= 147 {
            u32::from_le_bytes([d[143], d[144], d[145], d[146]])
        } else {
            0
        };
        Some(Self {
            pub_key,
            node_type,
            flags,
            out_path_len,
            _pad: 0,
            out_path,
            name,
            last_advert_ts,
            gps_lat,
            gps_lon,
            lastmod,
        })
    }

    /// Build a minimal contact from a received advert (for auto-add on RX).
    ///
    /// Sets `out_path_len = OUT_PATH_UNKNOWN` and zeroes the path.
    /// Uses `timestamp` as both `last_advert_ts` and `lastmod` (best
    /// approximation without a real-time clock).
    pub fn from_advert(
        pub_key: [u8; 32],
        name: &[u8],
        adv_type: u8,
        timestamp: u32,
        lat: i32,
        lon: i32,
    ) -> Self {
        let mut name_buf = [0u8; 32];
        let len = name.len().min(32);
        name_buf[..len].copy_from_slice(&name[..len]);
        Self {
            pub_key,
            node_type: adv_type,
            flags: 0,
            out_path_len: OUT_PATH_UNKNOWN,
            _pad: 0,
            out_path: [0u8; MAX_PATH_SIZE],
            name: name_buf,
            last_advert_ts: timestamp,
            gps_lat: lat,
            gps_lon: lon,
            lastmod: timestamp,
        }
    }

    // --- Serialisation ----------------------------------------------------------

    fn to_bytes(&self) -> [u8; CONTACT_SIZE] {
        let mut b = [0u8; CONTACT_SIZE];
        let mut p = 0usize;
        b[p..p + 32].copy_from_slice(&self.pub_key);
        p += 32;
        b[p] = self.node_type;
        p += 1;
        b[p] = self.flags;
        p += 1;
        b[p] = self.out_path_len;
        p += 1;
        b[p] = self._pad;
        p += 1;
        b[p..p + MAX_PATH_SIZE].copy_from_slice(&self.out_path);
        p += MAX_PATH_SIZE;
        b[p..p + 32].copy_from_slice(&self.name);
        p += 32;
        b[p..p + 4].copy_from_slice(&self.last_advert_ts.to_le_bytes());
        p += 4;
        b[p..p + 4].copy_from_slice(&self.gps_lat.to_le_bytes());
        p += 4;
        b[p..p + 4].copy_from_slice(&self.gps_lon.to_le_bytes());
        p += 4;
        b[p..p + 4].copy_from_slice(&self.lastmod.to_le_bytes());
        p += 4;
        debug_assert_eq!(p, CONTACT_SIZE);
        b
    }

    fn from_bytes(b: &[u8; CONTACT_SIZE]) -> Self {
        // Offsets: pub_key(0-31) type(32) flags(33) path_len(34) pad(35)
        //          out_path(36-99) name(100-131) ts(132-135)
        //          lat(136-139) lon(140-143) lastmod(144-147)
        let pub_key: [u8; 32] = b[0..32].try_into().unwrap();
        let node_type = b[32];
        let flags = b[33];
        let out_path_len = b[34];
        let _pad = b[35];
        let out_path: [u8; MAX_PATH_SIZE] = b[36..100].try_into().unwrap();
        let name: [u8; 32] = b[100..132].try_into().unwrap();
        let last_advert_ts = u32::from_le_bytes(b[132..136].try_into().unwrap());
        let gps_lat = i32::from_le_bytes(b[136..140].try_into().unwrap());
        let gps_lon = i32::from_le_bytes(b[140..144].try_into().unwrap());
        let lastmod = u32::from_le_bytes(b[144..148].try_into().unwrap());
        Self {
            pub_key,
            node_type,
            flags,
            out_path_len,
            _pad,
            out_path,
            name,
            last_advert_ts,
            gps_lat,
            gps_lon,
            lastmod,
        }
    }
}

// ---------------------------------------------------------------------------
// Metadata
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Default)]
struct Meta {
    /// [`MAX_CONTACTS`] value when this record was last written.
    capacity: u16,
    /// Ring-buffer next-write index (0 ≤ head < capacity).
    head: u16,
    /// Number of non-deleted contacts currently in the store.
    count: u16,
    _pad: u16,
}

impl Meta {
    fn to_bytes(self) -> [u8; META_SIZE] {
        let mut b = [0u8; META_SIZE];
        b[0..2].copy_from_slice(&self.capacity.to_le_bytes());
        b[2..4].copy_from_slice(&self.head.to_le_bytes());
        b[4..6].copy_from_slice(&self.count.to_le_bytes());
        b
    }

    fn from_bytes(b: &[u8; META_SIZE]) -> Self {
        Self {
            capacity: u16::from_le_bytes([b[0], b[1]]),
            head: u16::from_le_bytes([b[2], b[3]]),
            count: u16::from_le_bytes([b[4], b[5]]),
            _pad: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// AddResult
// ---------------------------------------------------------------------------

/// Result returned by [`ContactStore::add_or_update`].
#[derive(defmt::Format)]
pub enum AddResult {
    /// A new contact was stored in a free or previously-deleted slot.
    Added,
    /// An existing contact with the same public key was updated in place.
    Updated,
    /// The store was full; an old non-favourite (or favourite if necessary)
    /// was overwritten.
    Evicted,
}

// ---------------------------------------------------------------------------
// Key helpers
// ---------------------------------------------------------------------------

/// Format a slot index as a zero-padded 4-digit decimal string.
fn slot_key(idx: usize) -> heapless::String<4> {
    use core::fmt::Write;
    let mut s = heapless::String::new();
    let _ = write!(s, "{:04}", idx);
    s
}

/// Format the first 6 bytes of `pub_key` as a 12-char lowercase hex string.
///
/// 6 bytes (48 bits) gives a collision probability of < 10⁻⁹ among 700
/// contacts.  Using 6 bytes lets `find_by_prefix` reuse the same index since
/// the companion protocol identifies contacts by their first 6 pub_key bytes.
fn index_key(pub_key: &[u8]) -> heapless::String<12> {
    let mut s = heapless::String::new();
    for &b in &pub_key[..6.min(pub_key.len())] {
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

// ---------------------------------------------------------------------------
// ContactStore
// ---------------------------------------------------------------------------

/// Namespaced handle to the on-flash contact store.
///
/// Cheap to create — holds only a pointer to a static namespace string.
/// Create one with [`ContactStore::new()`] whenever needed; there is no
/// global singleton.
pub struct ContactStore {
    kv: kv::KvNamespace,
    /// Secondary index: pub_key[0..6] hex → slot index (2 bytes LE u16).
    /// Using 6 bytes lets find_by_prefix() (companion sends 6-byte prefix) share
    /// the same index as find_by_key() and add_or_update().
    ci: kv::KvNamespace,
}

impl ContactStore {
    /// Create a new handle to the contact store.
    pub fn new() -> Self {
        Self {
            kv: kv::namespace("contacts"),
            ci: kv::namespace("ci"),
        }
    }

    /// Look up a slot by the first 6 bytes of pub_key.
    ///
    /// Returns the slot index and full Contact if the slot is live and its
    /// pub_key starts with `prefix`.  Used by both full-key and prefix lookups.
    async fn index_lookup_prefix(&self, prefix: &[u8; 6]) -> Option<(usize, Contact)> {
        let ikey = index_key(prefix);
        let mut buf = [0u8; 2];
        if self.ci.get(ikey.as_str(), &mut buf).await.ok()? != 2 {
            return None;
        }
        let slot = u16::from_le_bytes(buf) as usize;
        if slot >= MAX_CONTACTS {
            return None;
        }
        let mut cbuf = [0u8; CONTACT_SIZE];
        if self.kv.get(slot_key(slot).as_str(), &mut cbuf).await.ok()? != CONTACT_SIZE {
            return None;
        }
        let c = Contact::from_bytes(cbuf[..CONTACT_SIZE].try_into().unwrap());
        if !c.is_deleted() && c.pub_key[..6] == *prefix {
            Some((slot, c))
        } else {
            None
        }
    }

    /// Look up the slot index for a full pub_key.
    async fn index_lookup(&self, pub_key: &[u8; 32]) -> Option<usize> {
        let prefix: &[u8; 6] = pub_key[..6].try_into().unwrap();
        self.index_lookup_prefix(prefix)
            .await
            .and_then(|(slot, c)| {
                if c.pub_key == *pub_key {
                    Some(slot)
                } else {
                    None
                }
            })
    }

    /// Write an index entry: pub_key[0..6] → slot index.
    async fn index_write(&self, pub_key: &[u8; 32], slot: usize) -> Result<(), kv::KvError> {
        let ikey = index_key(&pub_key[..6]);
        let slot_bytes = (slot as u16).to_le_bytes();
        self.ci.set(ikey.as_str(), &slot_bytes, true).await
    }

    /// Delete the index entry for pub_key.
    /// Must be called BEFORE zeroing the slot so the pub_key is still known.
    async fn index_delete(&self, pub_key: &[u8; 32]) {
        let ikey = index_key(&pub_key[..6]);
        let _ = self.ci.delete(ikey.as_str()).await;
    }

    // -----------------------------------------------------------------------
    // Initialisation & migration
    // -----------------------------------------------------------------------

    /// Initialise the contact store.
    ///
    /// Reads stored metadata and performs a one-time migration when
    /// [`MAX_CONTACTS`] differs from the value stored on flash.
    ///
    /// Call once from the main task after [`kv::init`] succeeds, before
    /// spawning any task that reads or writes contacts.
    pub async fn init(&self) {
        let mut buf = [0u8; META_SIZE];
        match self.kv.get("meta", &mut buf).await {
            Ok(n) if n == META_SIZE => {
                let meta = Meta::from_bytes(buf[..META_SIZE].try_into().unwrap());
                if meta.capacity as usize != MAX_CONTACTS {
                    self.migrate(meta).await;
                } else {
                    defmt::info!(
                        "contacts: {} slot(s) in use (capacity {})",
                        meta.count,
                        meta.capacity
                    );
                }
            }
            _ => {
                // First boot or corrupted metadata — write a fresh record.
                let meta = Meta {
                    capacity: MAX_CONTACTS as u16,
                    head: 0,
                    count: 0,
                    _pad: 0,
                };
                match self.kv.set("meta", &meta.to_bytes(), true).await {
                    Ok(()) => defmt::info!("contacts: initialised (capacity {})", MAX_CONTACTS),
                    Err(e) => {
                        defmt::error!(
                            "contacts: failed to write initial metadata: {:?} — wiping KV store",
                            e
                        );
                        crate::fw::kv::wipe_and_reset().await;
                    }
                }
            }
        }
    }

    async fn migrate(&self, old: Meta) {
        let old_cap = old.capacity as usize;
        defmt::info!(
            "contacts: capacity changed {} → {} — migrating",
            old_cap,
            MAX_CONTACTS
        );

        if MAX_CONTACTS >= old_cap {
            // Growing: existing slots remain valid; just update the capacity field.
            let new_meta = Meta {
                capacity: MAX_CONTACTS as u16,
                head: old.head.min((MAX_CONTACTS as u16).saturating_sub(1)),
                count: old.count,
                _pad: 0,
            };
            if let Err(e) = self.kv.set("meta", &new_meta.to_bytes(), true).await {
                defmt::warn!("contacts: migrate(grow) meta write failed: {:?}", e);
            }
        } else {
            // Shrinking: delete orphaned slots then rescan to get an accurate count.
            // Yield every iteration so the watchdog task gets to run — deleting
            // hundreds of slots takes several seconds and would otherwise starve it.
            for idx in MAX_CONTACTS..old_cap {
                let key = slot_key(idx);
                defmt::info!("contacts: deleting orphaned slot {} {}", idx, key.as_str());
                match self.kv.delete(key.as_str()).await {
                    Ok(()) => {}
                    Err(e) => {
                        defmt::warn!("contacts: failed to delete orphaned slot {}: {:?}", idx, e)
                    }
                }
                embassy_futures::yield_now().await;
            }

            // Rescan retained slots to get an accurate count.
            let mut count: u16 = 0;
            let mut cbuf = [0u8; CONTACT_SIZE];
            for idx in 0..MAX_CONTACTS {
                let key = slot_key(idx);
                if let Ok(n) = self.kv.get(key.as_str(), &mut cbuf).await {
                    if n == CONTACT_SIZE {
                        let c = Contact::from_bytes(cbuf[..CONTACT_SIZE].try_into().unwrap());
                        if !c.is_deleted() {
                            count += 1;
                        }
                    }
                }
                embassy_futures::yield_now().await;
            }

            let new_meta = Meta {
                capacity: MAX_CONTACTS as u16,
                head: old.head.min((MAX_CONTACTS as u16).saturating_sub(1)),
                count,
                _pad: 0,
            };
            if let Err(e) = self.kv.set("meta", &new_meta.to_bytes(), true).await {
                defmt::warn!("contacts: migrate(shrink) meta write failed: {:?}", e);
            }
        }
        defmt::info!("contacts: migration complete");
    }

    // -----------------------------------------------------------------------
    // Queries
    // -----------------------------------------------------------------------

    /// Returns the number of non-deleted contacts currently stored.
    pub async fn count(&self) -> u16 {
        let mut buf = [0u8; META_SIZE];
        match self.kv.get("meta", &mut buf).await {
            Ok(n) if n == META_SIZE => Meta::from_bytes(buf[..META_SIZE].try_into().unwrap()).count,
            _ => 0,
        }
    }

    /// Read the contact at slot `idx`.
    ///
    /// Returns `None` if the slot is out of range, has never been written, or
    /// has been deleted (pub_key all-zeros).
    pub async fn read_slot(&self, idx: usize) -> Option<Contact> {
        if idx >= MAX_CONTACTS {
            return None;
        }
        let key = slot_key(idx);
        let mut buf = [0u8; CONTACT_SIZE];
        match self.kv.get(key.as_str(), &mut buf).await {
            Ok(n) if n == CONTACT_SIZE => {
                let c = Contact::from_bytes(buf[..CONTACT_SIZE].try_into().unwrap());
                if c.is_deleted() { None } else { Some(c) }
            }
            _ => None,
        }
    }

    /// Update the routing path for a contact identified by `pub_key`.
    ///
    /// Only writes to flash if the contact exists and the path actually changed.
    /// Silently does nothing if the contact is not found.
    pub async fn update_path(
        &self,
        pub_key: &[u8; 32],
        out_path_len: u8,
        out_path: &[u8; MAX_PATH_SIZE],
    ) -> Result<(), kv::KvError> {
        let Some(slot) = self.index_lookup(pub_key).await else {
            return Ok(());
        };
        let key = slot_key(slot);
        let mut buf = [0u8; CONTACT_SIZE];
        if self.kv.get(key.as_str(), &mut buf).await.ok() != Some(CONTACT_SIZE) {
            return Ok(());
        }
        let mut c = Contact::from_bytes(buf[..CONTACT_SIZE].try_into().unwrap());
        if c.out_path_len == out_path_len && c.out_path == *out_path {
            return Ok(()); // nothing changed
        }
        c.out_path_len = out_path_len;
        c.out_path = *out_path;
        self.kv.set(key.as_str(), &c.to_bytes(), true).await
    }

    /// Find a contact whose pub_key starts with `prefix` (6 bytes).
    pub async fn find_by_prefix(&self, prefix: &[u8; 6]) -> Option<Contact> {
        self.index_lookup_prefix(prefix).await.map(|(_, c)| c)
    }

    /// Find a contact by exact pub_key match.
    pub async fn find_by_key(&self, pub_key: &[u8; 32]) -> Option<Contact> {
        let prefix: &[u8; 6] = pub_key[..6].try_into().unwrap();
        self.index_lookup_prefix(prefix)
            .await
            .and_then(|(_, c)| if c.pub_key == *pub_key { Some(c) } else { None })
    }

    // -----------------------------------------------------------------------
    // Mutations
    // -----------------------------------------------------------------------

    /// Add a new contact or update an existing one.
    ///
    /// Behaviour:
    /// - If a contact with the same `pub_key` already exists it is updated;
    ///   the stored favourite flag is **preserved** even when the incoming
    ///   entry clears it.
    /// - If a free (never-written) or deleted slot is available it is used.
    /// - When the store is full the oldest non-favourite contact (by `lastmod`)
    ///   is evicted.  If every contact is a favourite the oldest favourite is
    ///   overwritten instead.
    ///
    /// The entire scan is a single pass over all slots.
    pub async fn add_or_update(&self, contact: &Contact) -> Result<AddResult, kv::KvError> {
        // --- Update path: contact already known via index ---
        if let Some(slot) = self.index_lookup(&contact.pub_key).await {
            let mut cbuf = [0u8; CONTACT_SIZE];
            if let Ok(n) = self.kv.get(slot_key(slot).as_str(), &mut cbuf).await {
                if n == CONTACT_SIZE {
                    let existing = Contact::from_bytes(cbuf[..CONTACT_SIZE].try_into().unwrap());
                    if !existing.is_deleted() && existing.pub_key == contact.pub_key {
                        let mut updated = contact.clone();
                        updated.flags |= existing.flags & FLAG_FAVORITE;
                        if updated.to_bytes() == cbuf[..CONTACT_SIZE] {
                            return Ok(AddResult::Updated);
                        }
                        self.kv
                            .set(slot_key(slot).as_str(), &updated.to_bytes(), true)
                            .await?;
                        return Ok(AddResult::Updated);
                    }
                }
            }
            // Index pointed at a stale/deleted slot — fall through to add.
        }

        // --- Add path: write to the ring-buffer head slot ---
        let mut meta_buf = [0u8; META_SIZE];
        let mut meta = match self.kv.get("meta", &mut meta_buf).await {
            Ok(n) if n == META_SIZE => Meta::from_bytes(meta_buf[..META_SIZE].try_into().unwrap()),
            _ => Meta {
                capacity: MAX_CONTACTS as u16,
                head: 0,
                count: 0,
                _pad: 0,
            },
        };
        let capacity = (meta.capacity as usize).min(MAX_CONTACTS).max(1);
        let target = meta.head as usize % capacity;

        // Check if we are overwriting a live contact (eviction) and remove its
        // index entry first so the index never points at a stale slot.
        let mut evicted = false;
        let mut slot_buf = [0u8; CONTACT_SIZE];
        if let Ok(n) = self.kv.get(slot_key(target).as_str(), &mut slot_buf).await {
            if n == CONTACT_SIZE {
                let incumbent = Contact::from_bytes(slot_buf[..CONTACT_SIZE].try_into().unwrap());
                if !incumbent.is_deleted() {
                    self.index_delete(&incumbent.pub_key).await;
                    evicted = true;
                }
            }
        }

        if !evicted {
            meta.count = meta.count.saturating_add(1);
        }
        meta.head = ((target + 1) % capacity) as u16;

        self.kv
            .set(slot_key(target).as_str(), &contact.to_bytes(), true)
            .await?;
        self.index_write(&contact.pub_key, target).await?;
        self.kv.set("meta", &meta.to_bytes(), true).await?;
        Ok(if evicted {
            AddResult::Evicted
        } else {
            AddResult::Added
        })
    }

    /// Delete a contact by public key.
    ///
    /// The slot is zeroed (pub_key overwritten with all-zeros) so the storage
    /// can be reused.  Returns `true` if the contact was found and deleted,
    /// `false` if no contact with that key exists.
    pub async fn delete(&self, pub_key: &[u8; 32]) -> Result<bool, kv::KvError> {
        // Look up the slot via the index.
        let Some(idx) = self.index_lookup(pub_key).await else {
            return Ok(false);
        };

        let mut meta_buf = [0u8; META_SIZE];
        let mut meta = match self.kv.get("meta", &mut meta_buf).await {
            Ok(n) if n == META_SIZE => Meta::from_bytes(meta_buf[..META_SIZE].try_into().unwrap()),
            _ => return Ok(false),
        };

        // Zero the index entry first (while pub_key is still in scope).
        self.index_delete(pub_key).await;

        // Zero the slot.
        let zeroed = [0u8; CONTACT_SIZE];
        self.kv.set(slot_key(idx).as_str(), &zeroed, true).await?;
        meta.count = meta.count.saturating_sub(1);
        self.kv.set("meta", &meta.to_bytes(), true).await?;
        Ok(true)
    }

    /// Delete all contacts by iterating every slot and calling [`delete`] on each.
    pub async fn clear_all(&self) {
        for idx in 0..MAX_CONTACTS {
            if let Some(contact) = self.read_slot(idx).await {
                let _ = self.delete(&contact.pub_key).await;
            }
        }
    }
}
