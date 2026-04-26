//! Client-repeat / relay logic.
//!
//! When `LORA_CLIENT_REPEAT` is enabled, flood packets are re-transmitted
//! with an incremented hop count and the node's own hash appended to the
//! path.  A content-based dedup ring prevents ping-pong loops.

use core::cell::RefCell;
use core::sync::atomic::Ordering::Relaxed;

use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use meshcore::dedup::{MsgHashRing, relay_hash};
use meshcore::packet::{Message, RouteType};

use super::device_identity::DeviceIdentity;

static RELAY_SEEN: Mutex<CriticalSectionRawMutex, RefCell<MsgHashRing<32>>> =
    Mutex::new(RefCell::new(MsgHashRing::new()));

/// Evaluate whether `msg` should be relayed and, if so, return a
/// `TxRequest::RawFrame` ready for the unified TX queue.
///
/// Returns `None` when:
/// - `LORA_CLIENT_REPEAT` is off
/// - Route is not Flood / TransportFlood
/// - Payload was already seen in the dedup ring
/// - Hop count has reached the maximum (63)
/// - Serialization fails
pub fn try_relay(msg: &Message, identity: &DeviceIdentity) -> Option<crate::TxRequest> {
    if !crate::LORA_CLIENT_REPEAT.load(Relaxed) {
        return None;
    }
    if !matches!(msg.route, RouteType::Flood | RouteType::TransportFlood) {
        return None;
    }

    let rh = relay_hash(msg.payload_type.to_u8(), &msg.payload);
    let is_new = RELAY_SEEN.lock(|cell| {
        let mut ring = cell.borrow_mut();
        if ring.contains(rh) {
            false
        } else {
            ring.insert(rh);
            true
        }
    });
    if !is_new {
        return None;
    }

    let hash_size_code = (msg.path_len_byte >> 6) as usize;
    let hash_count = (msg.path_len_byte & 0x3F) as usize;
    let hash_size = hash_size_code + 1;

    if hash_count >= 63 {
        return None;
    }

    let new_path_len_byte = ((hash_size_code as u8) << 6) | ((hash_count + 1) as u8);
    let mut new_path = msg.path.clone();
    let _ = new_path.extend_from_slice(&identity.pub_key[..hash_size]);

    let relay_msg = Message {
        payload_type: msg.payload_type,
        route: msg.route,
        version: msg.version,
        transport_code: msg.transport_code,
        path_len_byte: new_path_len_byte,
        path: new_path,
        payload: msg.payload.clone(),
    };

    let mut frame = [0u8; meshcore::MAX_TRANS_UNIT];
    let len = meshcore::packet::serialize(&relay_msg, &mut frame).ok()?;

    defmt::info!(
        "Relay: type={=u8} hops={=usize}->{=usize} len={=usize}B",
        msg.payload_type.to_u8(),
        hash_count,
        hash_count + 1,
        len,
    );

    let mut data = [0u8; meshcore::MAX_TRANS_UNIT];
    data[..len].copy_from_slice(&frame[..len]);
    Some(crate::TxRequest::RawFrame { data, len })
}
