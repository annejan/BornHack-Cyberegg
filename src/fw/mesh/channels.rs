//! MeshCore channel slot management — per-slot KV access.
//!
//! Eight channel slots (indices 0–7) are stored one-per-key in the `ch`
//! KV namespace under keys `"0"` through `"7"`.  Nothing is buffered in
//! RAM between operations; every get/set/delete goes directly to flash.
//!
//! A slot is **empty** when all 48 bytes (name + key) are zero.
//! On first boot (no KV keys found) slot 0 is seeded with the well-known
//! public channel.
//!
//! # Slot layout (48 bytes)
//! ```text
//! [name: 32 bytes, zero-padded UTF-8][key: 16 bytes AES-128]
//! ```

use crate::fw::kv;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Number of channel slots supported by this device.
///
/// Used both here and in the `PACKET_DEVICE_INFO` `max_channels` field so
/// the companion app knows how many `GET_CHANNEL` requests to issue.
pub const NUM_CHANNELS: usize = 40;

const NAME_LEN: usize = 32;
const KEY_LEN: usize = 16;
const SLOT_LEN: usize = NAME_LEN + KEY_LEN; // 48 bytes

/// Format a slot index as a zero-padded 4-digit decimal string.
fn slot_key(idx: usize) -> heapless::String<4> {
    use core::fmt::Write;
    let mut s = heapless::String::new();
    let _ = write!(s, "{:02}", idx);
    s
}

/// Well-known public channel AES-128 key (publicly documented constant).
const PUBLIC_CHANNEL_KEY: [u8; KEY_LEN] = [
    0x8b, 0x33, 0x87, 0xe9, 0xc5, 0xcd, 0xea, 0x6a, 0xc9, 0xe5, 0xed, 0xba, 0xa1, 0x15, 0xcd, 0x72,
];

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn is_empty(slot: &[u8; SLOT_LEN]) -> bool {
    slot.iter().all(|&b| b == 0)
}

async fn kv_write(i: usize, slot: &[u8; SLOT_LEN]) {
    if let Err(e) = kv::namespace("ch")
        .set(slot_key(i).as_str(), slot, true)
        .await
    {
        defmt::warn!("channels: write slot {} failed: {:?}", i, e);
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Ensure all 8 KV keys exist.
///
/// Any missing key is created as an all-zero (empty) slot.
/// If no keys were found at all (first boot), slot 0 is seeded with the
/// public channel (`"public"` / well-known key
/// `8b3387e9c5cdea6ac9e5edbaa115cd72`).
pub async fn init() {
    let kv = kv::namespace("ch");
    let mut found = 0usize;

    for i in 0..NUM_CHANNELS {
        let mut buf = [0u8; SLOT_LEN];
        match kv.get(slot_key(i).as_str(), &mut buf).await {
            Ok(n) if n == SLOT_LEN => found += 1,
            _ => kv_write(i, &[0u8; SLOT_LEN]).await,
        }
    }

    if found == 0 {
        seed_defaults().await;
        defmt::info!("channels: first boot, seeded default channels");
    }
}

async fn seed_defaults() {
    let defaults: [(&[u8], &[u8; KEY_LEN]); 3] = [
        (b"public", &PUBLIC_CHANNEL_KEY),
        (
            b"#bornhack",
            &meshcore::channel::key_from_hashtag("#bornhack"),
        ),
        (b"#test", &meshcore::channel::key_from_hashtag("#test")),
    ];
    for (i, (name, key)) in defaults.iter().enumerate() {
        let mut slot = [0u8; SLOT_LEN];
        let n = name.len().min(NAME_LEN);
        slot[..n].copy_from_slice(&name[..n]);
        slot[NAME_LEN..].copy_from_slice(*key);
        kv_write(i, &slot).await;
    }
}

/// Read slot `idx` from KV.
///
/// Returns `Some((name, key))` if the slot is non-empty, `None` if it is
/// empty or `idx` is out of range.
pub async fn get(idx: u8) -> Option<([u8; NAME_LEN], [u8; KEY_LEN])> {
    if idx as usize >= NUM_CHANNELS {
        return None;
    }
    let mut buf = [0u8; SLOT_LEN];
    match kv::namespace("ch")
        .get(slot_key(idx as usize).as_str(), &mut buf)
        .await
    {
        Ok(n) if n == SLOT_LEN && !is_empty(&buf) => {
            let name: [u8; NAME_LEN] = buf[..NAME_LEN].try_into().unwrap();
            let key: [u8; KEY_LEN] = buf[NAME_LEN..].try_into().unwrap();
            Some((name, key))
        }
        _ => None,
    }
}

/// Write `name` and `key` into slot `idx` and persist to KV.
///
/// Passing an all-zero `name` **and** all-zero `key` is treated as a delete
/// — the slot is written as all-zero bytes.
///
/// Returns `false` if `idx` is out of range.
pub async fn set(idx: u8, name: &[u8; NAME_LEN], key: &[u8; KEY_LEN]) -> bool {
    let i = idx as usize;
    if i >= NUM_CHANNELS {
        return false;
    }
    let mut slot = [0u8; SLOT_LEN];
    slot[..NAME_LEN].copy_from_slice(name);
    slot[NAME_LEN..].copy_from_slice(key);
    kv_write(i, &slot).await;
    true
}

/// Zero out slot `idx` in KV (slot key is kept; value becomes all-zero).
///
/// Returns `false` if `idx` is out of range.
pub async fn delete(idx: u8) -> bool {
    let i = idx as usize;
    if i >= NUM_CHANNELS {
        return false;
    }
    kv_write(i, &[0u8; SLOT_LEN]).await;
    true
}

/// Reset all slots to factory defaults.
///
/// All slots are written as all-zero, then slot 0 is re-seeded with the
/// public channel.
pub async fn reset() {
    for i in 0..NUM_CHANNELS {
        kv_write(i, &[0u8; SLOT_LEN]).await;
    }
    seed_defaults().await;
    defmt::info!("channels: reset to defaults");
}

/// Count non-empty slots by reading all of them from KV.
pub async fn count_active() -> u8 {
    let kv = kv::namespace("ch");
    let mut count = 0u8;
    for i in 0..NUM_CHANNELS {
        let mut buf = [0u8; SLOT_LEN];
        if let Ok(n) = kv.get(slot_key(i).as_str(), &mut buf).await
            && n == SLOT_LEN && !is_empty(&buf) {
                count += 1;
            }
    }
    count
}
