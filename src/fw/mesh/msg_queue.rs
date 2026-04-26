//! KV-backed persistent circular message queue for received messages.
//!
//! Stores both channel (group) messages and private (P2P) messages in a single
//! ring buffer, distinguished by a `MsgKind` discriminator.
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
//! # Record layout (up to MAX_RECORD bytes)
//! ```text
//! [kind        : 1]  0x01 = channel, 0x02 = private
//! [sender_pfx  : 6]  pub_key[0..6] of sender (zeros for channel messages)
//! [channel_idx : 1]  channel slot (0 for private messages)
//! [path_len    : 1]  MeshCore path_len_byte encoding
//! [text_type   : 1]
//! [timestamp   : 4 LE]
//! [rssi        : 2 LE signed]
//! [text_len    : 1]
//! [text        : 0..=MAX_TEXT]
//! ```
//! Total header: 17 bytes + text_len bytes.

use core::cell::RefCell;

use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;

use crate::fw::kv;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum text payload length (mirrors `meshcore::MAX_GRP_DATA_SIZE`).
pub const MAX_TEXT: usize = 181;

const HEADER: usize = 17;

/// Maximum serialised record size in bytes.
const MAX_RECORD: usize = HEADER + MAX_TEXT;

/// Total number of KV slots.  Capacity = NUM_SLOTS − 1 = 255 messages.
const NUM_SLOTS: u16 = 256;

// ---------------------------------------------------------------------------
// Record types
// ---------------------------------------------------------------------------

/// Whether this queued message is a channel (group) or private (P2P) message.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum MsgKind {
    Channel,
    Private,
}

/// A single dequeued message, either channel or private.
pub struct ReceivedMsg {
    pub kind: MsgKind,
    /// First 6 bytes of the sender's pub_key (zeros for channel messages).
    pub sender_prefix: [u8; 6],
    pub channel_idx: u8,
    pub path_len: u8,
    pub text_type: u8,
    pub timestamp: u32,
    pub rssi: i16,
    pub text: heapless::Vec<u8, MAX_TEXT>,
}

// ---------------------------------------------------------------------------
// In-RAM state (indices only)
// ---------------------------------------------------------------------------

struct QueueState {
    /// Next slot to write into.  Always in `[0, NUM_SLOTS)`.
    write_idx: u16,
    /// Next slot to read from.  Always in `[0, NUM_SLOTS)`.
    read_idx: u16,
}

static STATE: Mutex<CriticalSectionRawMutex, RefCell<QueueState>> =
    Mutex::new(RefCell::new(QueueState {
        write_idx: 0,
        read_idx: 0,
    }));

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn advance(idx: u16) -> u16 {
    (idx + 1) % NUM_SLOTS
}

/// Format a slot index as a 2-character lowercase hex string.
fn slot_key(idx: u16) -> heapless::String<2> {
    let slot = idx as u8; // idx is always < 256
    let nibble = |n: u8| -> char { (if n < 10 { b'0' + n } else { b'a' + n - 10 }) as char };
    let mut s = heapless::String::<2>::new();
    s.push(nibble(slot >> 4)).ok();
    s.push(nibble(slot & 0xf)).ok();
    s
}

// ---------------------------------------------------------------------------
// Serialisation / deserialisation
// ---------------------------------------------------------------------------

fn serialize(msg: &ReceivedMsg, buf: &mut [u8; MAX_RECORD]) -> usize {
    buf[0] = match msg.kind {
        MsgKind::Channel => 0x01,
        MsgKind::Private => 0x02,
    };
    buf[1..7].copy_from_slice(&msg.sender_prefix);
    buf[7] = msg.channel_idx;
    buf[8] = msg.path_len;
    buf[9] = msg.text_type;
    buf[10..14].copy_from_slice(&msg.timestamp.to_le_bytes());
    buf[14..16].copy_from_slice(&msg.rssi.to_le_bytes());
    let text_len = msg.text.len().min(MAX_TEXT) as u8;
    buf[16] = text_len;
    buf[HEADER..HEADER + text_len as usize].copy_from_slice(&msg.text[..text_len as usize]);
    HEADER + text_len as usize
}

fn deserialize(buf: &[u8]) -> Option<ReceivedMsg> {
    if buf.len() < HEADER {
        return None;
    }
    let kind = match buf[0] {
        0x01 => MsgKind::Channel,
        0x02 => MsgKind::Private,
        _ => return None,
    };
    let sender_prefix: [u8; 6] = buf[1..7].try_into().ok()?;
    let channel_idx = buf[7];
    let path_len = buf[8];
    let text_type = buf[9];
    let timestamp = u32::from_le_bytes(buf[10..14].try_into().ok()?);
    let rssi = i16::from_le_bytes(buf[14..16].try_into().ok()?);
    let text_len = buf[16] as usize;
    if buf.len() < HEADER + text_len || text_len > MAX_TEXT {
        return None;
    }
    let mut text = heapless::Vec::<u8, MAX_TEXT>::new();
    text.extend_from_slice(&buf[HEADER..HEADER + text_len])
        .ok()?;
    Some(ReceivedMsg {
        kind,
        sender_prefix,
        channel_idx,
        path_len,
        text_type,
        timestamp,
        rssi,
        text,
    })
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
/// If the queue is full, the consumer pointer is advanced by one, silently
/// dropping the oldest message.
pub async fn push(msg: &ReceivedMsg) {
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

    if let Err(e) = kv::namespace("mq")
        .set(key.as_str(), &buf[..len], true)
        .await
    {
        defmt::warn!("msg_queue: push KV write failed: {:?}", e);
    }
}

/// Pop the oldest message from the queue.
///
/// Returns `None` if the queue is empty.
pub async fn pop() -> Option<ReceivedMsg> {
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
            defmt::warn!(
                "msg_queue: KV read failed for slot {:?}: {:?}",
                key.as_str(),
                e
            );
            None
        }
    }
}
