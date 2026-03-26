//! KV-backed persistent circular message queue for received channel messages.
//!
//! Classic circular buffer with a producer (`write_idx`) and consumer
//! (`read_idx`).  Both indices are bounded to `[0, NUM_SLOTS)` — they wrap via
//! modulo and never grow large, so there is no integer-overflow concern.
//!
//! ```text
//! Empty : write_idx == read_idx
//! Full  : (write_idx + 1) % NUM_SLOTS == read_idx
//! Count : (write_idx + NUM_SLOTS - read_idx) % NUM_SLOTS
//! ```
//!
//! Capacity is `NUM_SLOTS − 1` (one slot is sacrificed to distinguish full
//! from empty).  When full and a new message arrives, the consumer pointer is
//! advanced by one, silently dropping the oldest message.
//!
//! KV namespace `"mq"`, keys `"00"`–`"ff"` (slot index as 2-digit hex).
//!
//! # Record layout (up to 190 bytes)
//! ```text
//! [channel_idx : 1]
//! [path_len    : 1]
//! [timestamp   : 4 LE]
//! [rssi        : 2 LE signed]
//! [text_len    : 1]
//! [text        : 0..=181]
//! ```
//! Total: 10 + text_len bytes (max 191).

use core::cell::RefCell;

use embassy_sync::blocking_mutex::{Mutex, raw::CriticalSectionRawMutex};

use crate::fw::kv;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum text payload length (mirrors `meshcore::MAX_GRP_DATA_SIZE`).
pub const MAX_TEXT: usize = 181;

/// Maximum serialised record size in bytes.
const MAX_RECORD: usize = 10 + MAX_TEXT; // 191

/// Total number of KV slots.  Capacity = NUM_SLOTS − 1 = 255 messages.
const NUM_SLOTS: u16 = 256;

// ---------------------------------------------------------------------------
// Record type
// ---------------------------------------------------------------------------

/// A single received channel message, dequeued from the flash queue.
pub struct ReceivedChannelMsg {
    pub channel_idx: u8,
    pub path_len:    u8,
    pub text_type:   u8,
    pub timestamp:   u32,
    pub rssi:        i16,
    pub text:        heapless::Vec<u8, MAX_TEXT>,
}

// ---------------------------------------------------------------------------
// In-RAM state (indices only)
// ---------------------------------------------------------------------------

struct QueueState {
    /// Next slot to write into.  Always in `[0, NUM_SLOTS)`.
    write_idx: u16,
    /// Next slot to read from.  Always in `[0, NUM_SLOTS)`.
    read_idx:  u16,
}

static STATE: Mutex<CriticalSectionRawMutex, RefCell<QueueState>> =
    Mutex::new(RefCell::new(QueueState { write_idx: 0, read_idx: 0 }));

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn advance(idx: u16) -> u16 {
    (idx + 1) % NUM_SLOTS
}

/// Format a slot index as a 2-character lowercase hex string.
fn slot_key(idx: u16) -> heapless::String<2> {
    let slot = idx as u8; // idx is always < 256
    let nibble = |n: u8| -> char {
        (if n < 10 { b'0' + n } else { b'a' + n - 10 }) as char
    };
    let mut s = heapless::String::<2>::new();
    s.push(nibble(slot >> 4)).ok();
    s.push(nibble(slot & 0xf)).ok();
    s
}

// ---------------------------------------------------------------------------
// Serialisation / deserialisation
// ---------------------------------------------------------------------------

fn serialize(msg: &ReceivedChannelMsg, buf: &mut [u8; MAX_RECORD]) -> usize {
    buf[0] = msg.channel_idx;
    buf[1] = msg.path_len;
    buf[2] = msg.text_type;
    buf[3..7].copy_from_slice(&msg.timestamp.to_le_bytes());
    buf[7..9].copy_from_slice(&msg.rssi.to_le_bytes());
    let text_len = msg.text.len().min(MAX_TEXT) as u8;
    buf[9] = text_len;
    buf[10..10 + text_len as usize].copy_from_slice(&msg.text[..text_len as usize]);
    10 + text_len as usize
}

fn deserialize(buf: &[u8]) -> Option<ReceivedChannelMsg> {
    if buf.len() < 10 {
        return None;
    }
    let channel_idx = buf[0];
    let path_len    = buf[1];
    let text_type   = buf[2];
    let timestamp   = u32::from_le_bytes(buf[3..7].try_into().ok()?);
    let rssi        = i16::from_le_bytes(buf[7..9].try_into().ok()?);
    let text_len    = buf[9] as usize;
    if buf.len() < 10 + text_len || text_len > MAX_TEXT {
        return None;
    }
    let mut text = heapless::Vec::<u8, MAX_TEXT>::new();
    text.extend_from_slice(&buf[10..10 + text_len]).ok()?;
    Some(ReceivedChannelMsg { channel_idx, path_len, text_type, timestamp, rssi, text })
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Returns `true` when there are no messages available to pop.
pub fn is_empty() -> bool {
    STATE.lock(|cell| {
        let s = cell.borrow();
        s.write_idx == s.read_idx
    })
}

/// Returns the number of messages currently in the queue.
pub fn count() -> u16 {
    STATE.lock(|cell| {
        let s = cell.borrow();
        (s.write_idx + NUM_SLOTS - s.read_idx) % NUM_SLOTS
    })
}

/// Push a received message onto the queue.
///
/// If the queue is full (`(write_idx + 1) % NUM_SLOTS == read_idx`), the
/// consumer pointer is advanced by one, silently dropping the oldest message.
pub async fn push(msg: &ReceivedChannelMsg) {
    // Claim the write slot and possibly advance the consumer — all inside a
    // critical section so no await occurs while holding the lock.
    let (slot_idx, dropped) = STATE.lock(|cell| {
        let mut s = cell.borrow_mut();
        let full = advance(s.write_idx) == s.read_idx;
        if full {
            s.read_idx = advance(s.read_idx);
        }
        let slot = s.write_idx;
        s.write_idx = advance(s.write_idx);
        (slot, full)
    });

    if dropped {
        defmt::warn!("msg_queue: full, dropping oldest message");
    }

    let key = slot_key(slot_idx);
    let mut buf = [0u8; MAX_RECORD];
    let len = serialize(msg, &mut buf);

    if let Err(e) = kv::namespace("mq").set(key.as_str(), &buf[..len], true).await {
        defmt::warn!("msg_queue: push KV write failed: {:?}", e);
    }
}

/// Pop the oldest message from the queue.
///
/// Returns `None` if the queue is empty.
pub async fn pop() -> Option<ReceivedChannelMsg> {
    // Claim the read slot inside a critical section.
    let slot_idx = STATE.lock(|cell| {
        let mut s = cell.borrow_mut();
        if s.write_idx == s.read_idx {
            return None;
        }
        let slot = s.read_idx;
        s.read_idx = advance(s.read_idx);
        Some(slot)
    })?;

    let key = slot_key(slot_idx);
    let mut buf = [0u8; MAX_RECORD];
    match kv::namespace("mq").get(key.as_str(), &mut buf).await {
        Ok(n) => {
            if let Some(msg) = deserialize(&buf[..n]) {
                Some(msg)
            } else {
                defmt::warn!("msg_queue: deserialize failed for slot {:?}", key.as_str());
                None
            }
        }
        Err(e) => {
            defmt::warn!("msg_queue: KV read failed for slot {:?}: {:?}", key.as_str(), e);
            None
        }
    }
}
