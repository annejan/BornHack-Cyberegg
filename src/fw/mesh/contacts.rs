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
//! Offsets verified against `MyMesh::updateContactFromFrame()` in the MeshCore
//! reference firmware.
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

/// `true` when [`ContactStore`] has been mutated since an observer
/// last cleared it.  Currently observed by
/// `contacts_screen::refresh_cache` so it can skip the 300-slot kv
/// rescan when only the in-RAM advert/observation tables changed.
///
/// Set automatically by every mutation method on `ContactStore`
/// (`add_or_update`, `set_favorite`, `delete`, `update_path`,
/// `update_sync_since`, `clear_all`) when the operation actually
/// changes stored bytes.  Starts `true` so the first read after
/// boot always does a full rescan.
pub static STORE_DIRTY: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(true);

#[inline]
fn mark_dirty() {
    STORE_DIRTY.store(true, core::sync::atomic::Ordering::Relaxed);
    // Wake the Contacts-screen cache so any UI consumer reflects the
    // mutation promptly — covers BLE-companion-driven writes that
    // wouldn't otherwise produce an `ADVERT_SIGNAL`.
    super::contacts_screen::REBUILD_SIGNAL.signal(());
}

// ---------------------------------------------------------------------------
// On-flash record schema versioning
// ---------------------------------------------------------------------------
//
// STORE-LOCAL versioning for the on-flash [`Contact`] record layout. This
// number has nothing to do with MeshCore protocol versions, advert versions,
// or any other value that travels over the wire — it tracks how this
// firmware serialises contacts to flash, and nothing else.
//
// Layout: byte 0 of every record is the record version, followed by the
// serialised body. When this firmware starts and finds a slot whose first
// byte is not [`CURRENT_RECORD_VERSION`], it assumes the whole on-flash
// contact store was written by an older firmware with a different layout
// and wipes the contact-related namespaces (contacts / ci / hi) before
// continuing. Alpha-phase policy: no gradual migration, just start fresh.
//
// Bump [`CURRENT_RECORD_VERSION`] whenever the serialised body changes.

/// Current on-flash record version written by this firmware build.
///
/// Version history:
/// - `1`: first versioned layout. 149 bytes. No `sync_since` field.
/// - `2`: adds `sync_since: u32` at offset 149. 153 bytes. Required for room
///   server posts so we don't replay the full mailbox on every reboot.
pub(crate) const CURRENT_RECORD_VERSION: u8 = 2;

// ---------------------------------------------------------------------------
// Serialised sizes — defined explicitly, not derived from struct layout,
// to avoid any compiler-inserted padding changing the on-flash format.
// ---------------------------------------------------------------------------

/// Serialised record size: 1-byte version header + the 152-byte body.
const CONTACT_SIZE: usize = 153; // 1 (version) + 32+1+1+1+1+64+32+4+4+4+4+4
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
    /// Routing path bytes — zero-filled when `out_path_len ==
    /// OUT_PATH_UNKNOWN`. Always stored as `MAX_PATH_SIZE` (64 B) on the
    /// wire, matching MeshCore.
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
    /// Last post timestamp we have successfully ACKed for this peer.
    ///
    /// Meaningful only for room servers (`node_type == 3`): the firmware
    /// sends this value as the `sync_since` header in the login plaintext
    /// so the room can resume pushing posts from where we left off across
    /// reboots. Non-room contacts keep it at 0; `send_login` only reads it
    /// when the target is a room.
    ///
    /// Advances monotonically — see [`ContactStore::update_sync_since`].
    pub sync_since: u32,
}

impl Contact {
    /// Number of bytes in `out_path` that are valid, given `out_path_len`.
    ///
    /// MeshCore path_len_byte encoding: bits 7-6 = hash_size_code (0→1B, 1→2B,
    /// 2→3B), bits 5-0 = hop_count.  Actual bytes = hop_count ×
    /// (hash_size_code + 1). Returns 0 when `out_path_len` is
    /// [`OUT_PATH_UNKNOWN`] or invalid.
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
            sync_since: 0,
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
            sync_since: 0,
        }
    }

    // --- Serialisation ----------------------------------------------------------

    /// Serialise to the on-flash record layout.
    ///
    /// Byte 0 is the [`CURRENT_RECORD_VERSION`] store-local version marker.
    /// Bytes 1..153 are the record body (v2 = v1 + 4-byte `sync_since` tail).
    fn to_bytes(&self) -> [u8; CONTACT_SIZE] {
        let mut b = [0u8; CONTACT_SIZE];
        // [0] store-local record version
        b[0] = CURRENT_RECORD_VERSION;
        // [1..153] body — v1 layout + `sync_since` appended at the end.
        let mut p = 1usize;
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
        b[p..p + 4].copy_from_slice(&self.sync_since.to_le_bytes());
        p += 4;
        debug_assert_eq!(p, CONTACT_SIZE);
        b
    }

    /// Parse the on-flash record layout.
    ///
    /// Returns `None` if byte 0 is not [`CURRENT_RECORD_VERSION`] — the
    /// caller should treat the slot as legacy or corrupt and trigger a
    /// wipe via [`ContactStore::init`].
    fn from_bytes(b: &[u8; CONTACT_SIZE]) -> Option<Self> {
        if b[0] != CURRENT_RECORD_VERSION {
            return None;
        }
        // Body offsets (v2):
        //   version(0)
        //   pub_key(1-32) type(33) flags(34) path_len(35) pad(36)
        //   out_path(37-100) name(101-132) ts(133-136)
        //   lat(137-140) lon(141-144) lastmod(145-148)
        //   sync_since(149-152)
        let pub_key: [u8; 32] = b[1..33].try_into().unwrap();
        let node_type = b[33];
        let flags = b[34];
        let out_path_len = b[35];
        let _pad = b[36];
        let out_path: [u8; MAX_PATH_SIZE] = b[37..101].try_into().unwrap();
        let name: [u8; 32] = b[101..133].try_into().unwrap();
        let last_advert_ts = u32::from_le_bytes(b[133..137].try_into().unwrap());
        let gps_lat = i32::from_le_bytes(b[137..141].try_into().unwrap());
        let gps_lon = i32::from_le_bytes(b[141..145].try_into().unwrap());
        let lastmod = u32::from_le_bytes(b[145..149].try_into().unwrap());
        let sync_since = u32::from_le_bytes(b[149..153].try_into().unwrap());
        Some(Self {
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
            sync_since,
        })
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
    /// On-flash record schema version in use at the time this meta was
    /// written. Checked on boot via [`ContactStore::init`] — a mismatch
    /// triggers a wipe. Historical meta records wrote `_pad = 0` here,
    /// which correctly decodes as "legacy, needs migration".
    schema_version: u8,
    _reserved: u8,
}

impl Meta {
    fn to_bytes(self) -> [u8; META_SIZE] {
        let mut b = [0u8; META_SIZE];
        b[0..2].copy_from_slice(&self.capacity.to_le_bytes());
        b[2..4].copy_from_slice(&self.head.to_le_bytes());
        b[4..6].copy_from_slice(&self.count.to_le_bytes());
        b[6] = self.schema_version;
        b[7] = self._reserved;
        b
    }

    fn from_bytes(b: &[u8; META_SIZE]) -> Self {
        Self {
            capacity: u16::from_le_bytes([b[0], b[1]]),
            head: u16::from_le_bytes([b[2], b[3]]),
            count: u16::from_le_bytes([b[4], b[5]]),
            schema_version: b[6],
            _reserved: b[7],
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

/// Key format for the `hi` (hash-byte) namespace: 2-char lowercase hex of the
/// first byte of a contact's pub_key. Matches [`index_key`]'s encoding.
fn hi_key(src_hash: u8) -> heapless::String<2> {
    let mut s = heapless::String::new();
    let hi = src_hash >> 4;
    let lo = src_hash & 0xF;
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
    s
}

// ---------------------------------------------------------------------------
// ContactStore
// ---------------------------------------------------------------------------

/// Maximum number of slot indices that a single `hi` hash-bucket can hold.
///
/// Hard limit: when a bucket is full, [`ContactStore::add_or_update`] rejects
/// new contacts whose `pub_key[0]` hashes into that bucket, logging a
/// `defmt::warn!` line. Deletes of existing contacts in a full bucket free
/// space normally.
///
/// Sizing rationale (birthday paradox): with uniformly-distributed Ed25519
/// public keys and `MAX_CONTACTS = 300`, λ ≈ 1.17 → `P(bucket ≥ 16) ≈ 0`.
/// At `MAX_CONTACTS = 1024`, λ = 4 → `P(bucket ≥ 16) ≈ 1e-6`. Beyond
/// `MAX_CONTACTS ≈ 2000` the rejection probability starts mattering and
/// this limit may need to grow.
///
/// **Limitation**: increasing the bucket cap grows the on-flash value size
/// (`1 + 2 × MAX_SLOTS_PER_BUCKET` bytes) and the RAM read buffer in
/// [`ContactStore::hash_index_lookup`]. Keep it small.
pub const MAX_SLOTS_PER_BUCKET: usize = 16;

/// Namespaced handle to the on-flash contact store.
///
/// Cheap to create — holds only a pointer to a static namespace string.
/// Create one with [`ContactStore::new()`] whenever needed; there is no
/// global singleton.
pub struct ContactStore {
    kv: kv::KvNamespace,
    /// Secondary index: pub_key[0..6] hex → slot index (2 bytes LE u16).
    /// Used by `find_by_key` / `find_by_prefix` — O(1) lookup keyed on the
    /// 6-byte prefix the companion protocol uses to address contacts.
    ci: kv::KvNamespace,
    /// Tertiary index: `pub_key[0]` hex byte → list of slot indices that
    /// share that first byte.
    ///
    /// Answers the "all contacts with `pub_key[0] == src_hash`" query that
    /// the MeshCore receive handlers need: every datagram on the wire carries
    /// only a 1-byte `src_hash` (the first byte of the sender's pub_key), so
    /// an incoming packet needs to enumerate all contacts matching that byte
    /// to find the decrypting shared secret. Without this index the receive
    /// path would linear-scan every slot (~50 ms per flash read × N slots).
    ///
    /// Key format: 2-char lowercase hex of the byte (e.g. `"00"`, `"a5"`).
    /// Value format: `[count:1][slot_le:2]×count`, capped at
    /// [`MAX_SLOTS_PER_BUCKET`]. Deletes of the last slot in a bucket remove
    /// the KV key entirely.
    hi: kv::KvNamespace,
}

impl Default for ContactStore {
    fn default() -> Self {
        Self::new()
    }
}

impl ContactStore {
    /// Create a new handle to the contact store.
    pub fn new() -> Self {
        Self {
            kv: kv::namespace("contacts"),
            ci: kv::namespace("ci"),
            hi: kv::namespace("hi"),
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
        let c = Contact::from_bytes(cbuf[..CONTACT_SIZE].try_into().unwrap())?;
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
    // Hash-byte index (hi): first-byte-of-pub_key → list of slot indices
    // -----------------------------------------------------------------------

    /// Read a hash bucket into `out`. Returns the number of slot indices
    /// copied (0 if the bucket does not exist).
    ///
    /// O(1): a single `kv.get` on a tiny (≤ 33 byte) value.
    pub async fn hash_index_lookup(
        &self,
        src_hash: u8,
        out: &mut [u16; MAX_SLOTS_PER_BUCKET],
    ) -> usize {
        let k = hi_key(src_hash);
        let mut buf = [0u8; 1 + 2 * MAX_SLOTS_PER_BUCKET];
        let n = match self.hi.get(k.as_str(), &mut buf).await {
            Ok(n) => n,
            Err(_) => return 0,
        };
        if n < 1 {
            return 0;
        }
        let stored_count = buf[0] as usize;
        let avail_count = (n - 1) / 2;
        let count = stored_count.min(avail_count).min(MAX_SLOTS_PER_BUCKET);
        for (i, slot) in out.iter_mut().take(count).enumerate() {
            let off = 1 + i * 2;
            *slot = u16::from_le_bytes([buf[off], buf[off + 1]]);
        }
        count
    }

    /// `true` if the bucket for `src_hash` has at least one free entry.
    async fn hash_index_has_room(&self, src_hash: u8) -> bool {
        let k = hi_key(src_hash);
        let mut buf = [0u8; 1 + 2 * MAX_SLOTS_PER_BUCKET];
        match self.hi.get(k.as_str(), &mut buf).await {
            Ok(n) if n >= 1 => {
                let count = (buf[0] as usize).min((n - 1) / 2);
                count < MAX_SLOTS_PER_BUCKET
            }
            _ => true, // bucket doesn't exist yet — plenty of room
        }
    }

    /// Insert `slot` into the bucket for `src_hash`. Idempotent: if `slot`
    /// is already present the bucket is left unchanged.
    ///
    /// Returns `Err(KvError::StoreFull)` if the bucket is at
    /// [`MAX_SLOTS_PER_BUCKET`] and `slot` is not already present. The caller
    /// should reject the mutation and log a warning.
    async fn hash_index_insert(&self, src_hash: u8, slot: u16) -> Result<(), kv::KvError> {
        let k = hi_key(src_hash);
        let mut buf = [0u8; 1 + 2 * MAX_SLOTS_PER_BUCKET];
        let n = match self.hi.get(k.as_str(), &mut buf).await {
            Ok(n) => n,
            Err(kv::KvError::NotFound) => 0,
            Err(e) => return Err(e),
        };
        let mut count = if n >= 1 {
            (buf[0] as usize).min((n - 1) / 2)
        } else {
            0
        };

        // Idempotent: no-op if slot already present.
        for i in 0..count {
            let off = 1 + i * 2;
            let s = u16::from_le_bytes([buf[off], buf[off + 1]]);
            if s == slot {
                return Ok(());
            }
        }

        if count >= MAX_SLOTS_PER_BUCKET {
            return Err(kv::KvError::StoreFull);
        }

        let off = 1 + count * 2;
        let le = slot.to_le_bytes();
        buf[off] = le[0];
        buf[off + 1] = le[1];
        count += 1;
        buf[0] = count as u8;

        self.hi.set(k.as_str(), &buf[..1 + count * 2], true).await
    }

    /// Remove `slot` from the bucket for `src_hash`. No-op if absent.
    ///
    /// Deletes the KV key entirely when the last slot leaves the bucket, so
    /// empty buckets don't linger in flash.
    async fn hash_index_remove(&self, src_hash: u8, slot: u16) -> Result<(), kv::KvError> {
        let k = hi_key(src_hash);
        let mut buf = [0u8; 1 + 2 * MAX_SLOTS_PER_BUCKET];
        let n = match self.hi.get(k.as_str(), &mut buf).await {
            Ok(n) => n,
            Err(kv::KvError::NotFound) => return Ok(()),
            Err(e) => return Err(e),
        };
        if n < 1 {
            return Ok(());
        }
        let mut count = (buf[0] as usize).min((n - 1) / 2);

        // Find and drop the matching entry (shift tail left).
        let mut found_at: Option<usize> = None;
        for i in 0..count {
            let off = 1 + i * 2;
            let s = u16::from_le_bytes([buf[off], buf[off + 1]]);
            if s == slot {
                found_at = Some(i);
                break;
            }
        }
        let Some(pos) = found_at else { return Ok(()) };

        for j in pos..(count - 1) {
            let dst = 1 + j * 2;
            let src = 1 + (j + 1) * 2;
            buf[dst] = buf[src];
            buf[dst + 1] = buf[src + 1];
        }
        count -= 1;

        if count == 0 {
            self.hi.delete(k.as_str()).await
        } else {
            buf[0] = count as u8;
            self.hi.set(k.as_str(), &buf[..1 + count * 2], true).await
        }
    }

    // -----------------------------------------------------------------------
    // Initialisation & migration
    // -----------------------------------------------------------------------

    /// Decide whether the on-flash contact store is in a layout this firmware
    /// can read, based on a single `meta` read.
    ///
    /// The `meta` record carries the `schema_version` of the records that
    /// were live at the time it was last written. Comparing that one byte
    /// against [`CURRENT_RECORD_VERSION`] tells us whether the slot records
    /// still match our `Contact` layout:
    ///
    /// - `meta` missing → first boot / brand-new KV store → nothing to wipe.
    /// - `meta.schema_version == CURRENT_RECORD_VERSION` → store is current.
    /// - `meta.schema_version == 0` → historical meta written before this field
    ///   existed; legacy slots, wipe.
    /// - `meta.schema_version != CURRENT_RECORD_VERSION` → stale layout, wipe.
    ///
    /// Exactly **one** flash read per boot. The previous implementation
    /// walked up to `MAX_CONTACTS` slots to find a non-empty record, and
    /// ekv treats every `NotFound` lookup as a full log scan (~50 ms), so
    /// that cost ~15 s per boot on a sparse or empty store.
    async fn detect_legacy_records(&self) -> bool {
        let mut buf = [0u8; META_SIZE];
        match self.kv.get("meta", &mut buf).await {
            Ok(n) if n == META_SIZE => {
                let meta = Meta::from_bytes(buf[..META_SIZE].try_into().unwrap());
                if meta.schema_version == CURRENT_RECORD_VERSION {
                    false
                } else {
                    defmt::warn!(
                        "contacts: meta schema_version={=u8} expected {=u8} — wiping",
                        meta.schema_version,
                        CURRENT_RECORD_VERSION,
                    );
                    true
                }
            }
            _ => false, // no meta → fresh store, nothing to wipe
        }
    }

    /// Wipe the contact-related KV namespaces (`contacts`, `hi`) after a
    /// legacy-format store is detected.
    ///
    /// Deletes every slot key and every hash-bucket key, plus the `meta`
    /// sentinel, so the subsequent `init` flow writes a fresh capacity
    /// record and an empty hash index. The `ci` namespace is left alone;
    /// stale entries there self-heal on the next `add_or_update` because
    /// the update path verifies the slot contents before reusing the index.
    ///
    /// Slow (~several seconds of flash churn) — only runs once per
    /// format bump. The caller is responsible for turning on visual
    /// feedback (e.g. the green LED) before calling this.
    async fn wipe_contact_store(&self) {
        defmt::warn!("contacts: wiping store");

        for idx in 0..MAX_CONTACTS {
            let _ = self.kv.delete(slot_key(idx).as_str()).await;
            embassy_futures::yield_now().await;
        }
        let _ = self.kv.delete("meta").await;

        for b in 0u8..=255u8 {
            let _ = self.hi.delete(hi_key(b).as_str()).await;
            embassy_futures::yield_now().await;
        }

        defmt::info!("contacts: store wiped");
    }

    /// Initialise the contact store.
    ///
    /// 1. If the on-flash records are in a layout this firmware does not
    ///    understand (legacy `CONTACT_SIZE` or version byte mismatch), wipe the
    ///    contact-related namespaces.
    /// 2. Read stored metadata; if [`MAX_CONTACTS`] differs from the value on
    ///    flash, perform the capacity migration.
    ///
    /// Call once from the main task after [`kv::init`] succeeds, before
    /// spawning any task that reads or writes contacts.
    pub async fn init(&self) {
        // --- 1. Detect and wipe legacy records. ---
        if self.detect_legacy_records().await {
            defmt::warn!(
                "contacts: record format mismatch — wiping store (blink LED, alpha-phase policy)"
            );
            crate::fw::led::set_led(&crate::fw::led::LED_GREEN, crate::fw::led::LedState::Duty50);
            self.wipe_contact_store().await;
            crate::fw::led::set_led(&crate::fw::led::LED_GREEN, crate::fw::led::LedState::Off);
        }

        // --- 2. Read/write meta. ---
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
                    schema_version: CURRENT_RECORD_VERSION,
                    _reserved: 0,
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
                schema_version: CURRENT_RECORD_VERSION,
                _reserved: 0,
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
                defmt::debug!("contacts: deleting orphaned slot {} {}", idx, key.as_str());
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
                if let Ok(n) = self.kv.get(key.as_str(), &mut cbuf).await
                    && n == CONTACT_SIZE
                    && let Some(c) = Contact::from_bytes(cbuf[..CONTACT_SIZE].try_into().unwrap())
                    && !c.is_deleted()
                {
                    count += 1;
                }
                embassy_futures::yield_now().await;
            }

            let new_meta = Meta {
                capacity: MAX_CONTACTS as u16,
                head: old.head.min((MAX_CONTACTS as u16).saturating_sub(1)),
                count,
                schema_version: CURRENT_RECORD_VERSION,
                _reserved: 0,
            };
            if let Err(e) = self.kv.set("meta", &new_meta.to_bytes(), true).await {
                defmt::warn!("contacts: migrate(shrink) meta write failed: {:?}", e);
            }
        }
        defmt::debug!("contacts: migration complete");
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
                let c = Contact::from_bytes(buf[..CONTACT_SIZE].try_into().unwrap())?;
                if c.is_deleted() { None } else { Some(c) }
            }
            _ => None,
        }
    }

    /// Advance the per-room `sync_since` cursor for a contact.
    ///
    /// Writes `timestamp` into the contact's `sync_since` field only if the
    /// new value is strictly greater than the stored one — matching the
    /// reference `BaseChatMesh.cpp:242`:
    /// ```text
    /// if (timestamp > from.sync_since) { from.sync_since = timestamp; }
    /// ```
    /// This keeps the cursor monotonic even if posts arrive out of order.
    ///
    /// Returns `Ok(true)` when the flash record was rewritten,
    /// `Ok(false)` when the contact was not found or the stored value was
    /// already >= `timestamp`. Meant for room-server post ACK handling —
    /// see `try_handle_txt_msg` in meshcore.rs.
    pub async fn update_sync_since(
        &self,
        pub_key: &[u8; 32],
        timestamp: u32,
    ) -> Result<bool, kv::KvError> {
        let Some(slot) = self.index_lookup(pub_key).await else {
            return Ok(false);
        };
        let key = slot_key(slot);
        let mut buf = [0u8; CONTACT_SIZE];
        if self.kv.get(key.as_str(), &mut buf).await.ok() != Some(CONTACT_SIZE) {
            return Ok(false);
        }
        let Some(mut c) = Contact::from_bytes(buf[..CONTACT_SIZE].try_into().unwrap()) else {
            return Ok(false);
        };
        if timestamp <= c.sync_since {
            return Ok(false); // no advance
        }
        c.sync_since = timestamp;
        self.kv.set(key.as_str(), &c.to_bytes(), true).await?;
        mark_dirty();
        Ok(true)
    }

    /// Update the routing path for a contact identified by `pub_key`.
    ///
    /// Only writes to flash if the contact exists and the path actually
    /// changed. Silently does nothing if the contact is not found.
    ///
    /// Returns:
    /// - `Ok(true)` — the flash record was rewritten because the new path
    ///   differs from the stored one. Callers should fire
    ///   [`crate::PATH_UPDATED_CHANNEL`] so the BLE task notifies the phone.
    /// - `Ok(false)` — contact not found, or the stored path already matches.
    ///   No write happened, no notification should be pushed.
    /// - `Err(_)` — flash write failed.
    pub async fn update_path(
        &self,
        pub_key: &[u8; 32],
        out_path_len: u8,
        out_path: &[u8; MAX_PATH_SIZE],
    ) -> Result<bool, kv::KvError> {
        let Some(slot) = self.index_lookup(pub_key).await else {
            return Ok(false);
        };
        let key = slot_key(slot);
        let mut buf = [0u8; CONTACT_SIZE];
        if self.kv.get(key.as_str(), &mut buf).await.ok() != Some(CONTACT_SIZE) {
            return Ok(false);
        }
        let Some(mut c) = Contact::from_bytes(buf[..CONTACT_SIZE].try_into().unwrap()) else {
            return Ok(false);
        };
        if c.out_path_len == out_path_len && c.out_path == *out_path {
            return Ok(false); // nothing changed
        }
        c.out_path_len = out_path_len;
        c.out_path = *out_path;
        self.kv.set(key.as_str(), &c.to_bytes(), true).await?;
        mark_dirty();
        Ok(true)
    }

    /// Set or clear the [`FLAG_FAVORITE`] bit on the contact identified
    /// by `pub_key`.  Returns `Ok(true)` when the stored flag changed,
    /// `Ok(false)` when the contact wasn't found or the flag already
    /// matched the requested state.
    pub async fn set_favorite(
        &self,
        pub_key: &[u8; 32],
        favorite: bool,
    ) -> Result<bool, kv::KvError> {
        let Some(slot) = self.index_lookup(pub_key).await else {
            return Ok(false);
        };
        let key = slot_key(slot);
        let mut buf = [0u8; CONTACT_SIZE];
        if self.kv.get(key.as_str(), &mut buf).await.ok() != Some(CONTACT_SIZE) {
            return Ok(false);
        }
        let Some(mut c) = Contact::from_bytes(buf[..CONTACT_SIZE].try_into().unwrap()) else {
            return Ok(false);
        };
        let was = c.is_favorite();
        if was == favorite {
            return Ok(false); // already in the requested state
        }
        if favorite {
            c.flags |= FLAG_FAVORITE;
        } else {
            c.flags &= !FLAG_FAVORITE;
        }
        self.kv.set(key.as_str(), &c.to_bytes(), true).await?;
        mark_dirty();
        Ok(true)
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
    /// - If a contact with the same `pub_key` already exists it is updated; the
    ///   stored favourite flag is **preserved** even when the incoming entry
    ///   clears it.
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
            if let Ok(n) = self.kv.get(slot_key(slot).as_str(), &mut cbuf).await
                && n == CONTACT_SIZE
                && let Some(existing) =
                    Contact::from_bytes(cbuf[..CONTACT_SIZE].try_into().unwrap())
                && !existing.is_deleted()
                && existing.pub_key == contact.pub_key
            {
                let mut updated = contact.clone();
                updated.flags |= existing.flags & FLAG_FAVORITE;
                if updated.to_bytes() == cbuf[..CONTACT_SIZE] {
                    return Ok(AddResult::Updated);
                }
                self.kv
                    .set(slot_key(slot).as_str(), &updated.to_bytes(), true)
                    .await?;
                mark_dirty();
                return Ok(AddResult::Updated);
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
                schema_version: CURRENT_RECORD_VERSION,
                _reserved: 0,
            },
        };
        let capacity = (meta.capacity as usize).clamp(1, MAX_CONTACTS);
        let contact_hash = contact.pub_key[0];

        // Select the destination slot per the documented policy in a single
        // pass: prefer a free/deleted slot; otherwise evict the oldest
        // non-favourite by `lastmod`; only when every live slot is a favourite
        // do we evict the oldest favourite. (Previously this blindly took
        // `head % capacity`, which could silently overwrite a favourite or a
        // live contact while deleted holes sat unused.)
        let mut free_slot: Option<usize> = None;
        let mut oldest_nonfav: Option<(usize, u32)> = None;
        let mut oldest_fav: Option<(usize, u32)> = None;
        for slot in 0..capacity {
            let mut sb = [0u8; CONTACT_SIZE];
            let live = match self.kv.get(slot_key(slot).as_str(), &mut sb).await {
                Ok(n) if n == CONTACT_SIZE => {
                    Contact::from_bytes(sb[..CONTACT_SIZE].try_into().unwrap())
                        .filter(|c| !c.is_deleted())
                }
                _ => None,
            };
            match live {
                None => {
                    if free_slot.is_none() {
                        free_slot = Some(slot);
                    }
                }
                Some(c) if c.is_favorite() => {
                    if oldest_fav.is_none_or(|(_, lm)| c.lastmod < lm) {
                        oldest_fav = Some((slot, c.lastmod));
                    }
                }
                Some(c) => {
                    if oldest_nonfav.is_none_or(|(_, lm)| c.lastmod < lm) {
                        oldest_nonfav = Some((slot, c.lastmod));
                    }
                }
            }
        }
        let target = free_slot
            .or(oldest_nonfav.map(|(s, _)| s))
            .or(oldest_fav.map(|(s, _)| s))
            .unwrap_or(0);

        // Read the incumbent (if any) so we can unlink its index entries
        // before overwriting the slot.
        let mut slot_buf = [0u8; CONTACT_SIZE];
        let incumbent: Option<Contact> =
            match self.kv.get(slot_key(target).as_str(), &mut slot_buf).await {
                Ok(n) if n == CONTACT_SIZE => {
                    match Contact::from_bytes(slot_buf[..CONTACT_SIZE].try_into().unwrap()) {
                        Some(c) if !c.is_deleted() => Some(c),
                        _ => None,
                    }
                }
                _ => None,
            };
        let evicted = incumbent.is_some();

        // Pre-check the hash bucket BEFORE any writes so a rejection leaves
        // the store untouched. If the eviction target shares the same first
        // byte with the new contact the bucket loses and gains one entry;
        // room is unchanged in that case.
        let bucket_room_required = incumbent
            .as_ref()
            .is_none_or(|inc| inc.pub_key[0] != contact_hash);
        if bucket_room_required && !self.hash_index_has_room(contact_hash).await {
            defmt::warn!(
                "contacts: hash bucket {=u8:#04x} full ({=usize} slots) — rejecting new contact. Bump MAX_SLOTS_PER_BUCKET if this happens often.",
                contact_hash,
                MAX_SLOTS_PER_BUCKET,
            );
            return Err(kv::KvError::StoreFull);
        }

        // Unlink the incumbent from both secondary indices BEFORE overwriting
        // the slot, in the order hi-index then ci-index, so a crash leaves no
        // dangling lookups that would return this slot with mismatched data.
        if let Some(inc) = incumbent.as_ref() {
            let _ = self.hash_index_remove(inc.pub_key[0], target as u16).await;
            self.index_delete(&inc.pub_key).await;
        }

        if !evicted {
            meta.count = meta.count.saturating_add(1);
        }
        meta.head = ((target + 1) % capacity) as u16;

        self.kv
            .set(slot_key(target).as_str(), &contact.to_bytes(), true)
            .await?;
        self.index_write(&contact.pub_key, target).await?;
        // The pre-check guarantees there is room, so `hash_index_insert`
        // should not return `StoreFull` here. Any other error leaves a stale
        // ci entry without hi — the receive path still finds the contact via
        // the legacy scan fallback, so the store remains functionally correct.
        let _ = self.hash_index_insert(contact_hash, target as u16).await;
        self.kv.set("meta", &meta.to_bytes(), true).await?;
        mark_dirty();
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

        // Remove all dependent index entries BEFORE zeroing the slot, in the
        // order hi-bucket → ci-prefix → slot. The goal is that if a crash
        // interrupts the delete we never leave a lookup path (either ci or
        // hi) that points at a slot whose pub_key doesn't match it — which
        // would cause dangling entries to "bounce around" during receive.
        let _ = self.hash_index_remove(pub_key[0], idx as u16).await;
        self.index_delete(pub_key).await;

        // Zero the slot.
        let zeroed = [0u8; CONTACT_SIZE];
        self.kv.set(slot_key(idx).as_str(), &zeroed, true).await?;
        meta.count = meta.count.saturating_sub(1);
        self.kv.set("meta", &meta.to_bytes(), true).await?;
        mark_dirty();
        Ok(true)
    }

    /// Delete all contacts by iterating every slot and calling [`Self::delete`]
    /// on each.  `delete` already marks the store dirty per slot, so the
    /// observer sees a single dirty signal for the wipe.
    pub async fn clear_all(&self) {
        for idx in 0..MAX_CONTACTS {
            if let Some(contact) = self.read_slot(idx).await {
                let _ = self.delete(&contact.pub_key).await;
            }
        }
    }
}
