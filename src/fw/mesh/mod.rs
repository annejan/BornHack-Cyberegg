//! MeshCore mesh networking stack.
//!
//! This module contains everything related to the LoRa mesh: radio driver,
//! BLE companion protocol, packet codec, contact/channel/bond/KV storage,
//! and the inter-task channels that connect them.
//!
//! Gated behind `#[cfg(feature = "mesh")]` in `fw/mod.rs`.

pub mod ble;
pub mod bonds;
pub mod channel_browser;
pub mod channels;
pub mod contacts;
pub mod device_identity;
pub mod meshcore;
pub mod msg_queue;
pub mod persister;
pub mod repeater;
pub mod settings;
pub mod sx1262;

// Re-export the meshcore listener entry point for embassy.rs.
use core::cell::RefCell;

use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::signal::Signal;
pub use meshcore::run_meshcore_listener;

// ---------------------------------------------------------------------------
// Display data — written by meshcore task, read by display renderer
// ---------------------------------------------------------------------------

/// Last decoded LoRa group-text message.
pub struct LoraMessage {
    pub channel: heapless::String<32>,
    pub sender: heapless::String<32>,
    pub text: heapless::String<128>,
    pub timestamp: u32,
    pub rssi: i16,
    pub snr_x4: i8,
}

pub static LAST_LORA_MSG: Mutex<CriticalSectionRawMutex, RefCell<Option<LoraMessage>>> =
    Mutex::new(RefCell::new(None));

/// Fired whenever a new channel message is stored in `LAST_LORA_MSG`.
pub static LORA_MSG_SIGNAL: Signal<CriticalSectionRawMutex, ()> = Signal::new();

// ---------------------------------------------------------------------------
// Channel message ring buffer (for the on-device channel browser)
// ---------------------------------------------------------------------------

/// Maximum channel message text size in bytes.
pub const CHANNEL_MSG_TEXT_MAX: usize = 130;

/// A single channel message cached in RAM for the on-device browser.
pub struct ChannelMsgEntry {
    pub channel_idx: u8,
    pub sender: heapless::String<16>,
    pub text: heapless::String<CHANNEL_MSG_TEXT_MAX>,
    pub timestamp: u32,
    pub content_hash: u32,
    pub is_own: bool,
    pub repeat_count: u8,
}

pub const CHANNEL_MSG_RING_SIZE: usize = 32;

pub struct ChannelMsgRing {
    entries: [Option<ChannelMsgEntry>; CHANNEL_MSG_RING_SIZE],
    head: usize,
    len: usize,
}

impl ChannelMsgRing {
    const INIT: Option<ChannelMsgEntry> = None;

    pub const fn new() -> Self {
        Self {
            entries: [Self::INIT; CHANNEL_MSG_RING_SIZE],
            head: 0,
            len: 0,
        }
    }

    pub fn push(&mut self, entry: ChannelMsgEntry) {
        self.entries[self.head] = Some(entry);
        self.head = (self.head + 1) % CHANNEL_MSG_RING_SIZE;
        if self.len < CHANNEL_MSG_RING_SIZE {
            self.len += 1;
        }
    }

    /// Iterate entries from oldest to newest.
    pub fn iter(&self) -> impl Iterator<Item = &ChannelMsgEntry> {
        let start = if self.len < CHANNEL_MSG_RING_SIZE {
            0
        } else {
            self.head
        };
        (0..self.len).filter_map(move |i| {
            let idx = (start + i) % CHANNEL_MSG_RING_SIZE;
            self.entries[idx].as_ref()
        })
    }

    /// Find a mutable entry by content_hash (for repeat-count updates).
    pub fn find_by_hash_mut(&mut self, hash: u32) -> Option<&mut ChannelMsgEntry> {
        self.entries
            .iter_mut()
            .filter_map(|e| e.as_mut())
            .find(|e| e.content_hash == hash)
    }
}

pub static CHANNEL_MSG_RING: Mutex<CriticalSectionRawMutex, RefCell<ChannelMsgRing>> =
    Mutex::new(RefCell::new(ChannelMsgRing::new()));

/// Cached channel list for the on-device browser. Populated by the meshcore
/// task at boot and on `CHANNELS_CHANGED_SIGNAL`.
pub struct CachedChannel {
    pub slot_idx: u8,
    pub name: heapless::String<20>,
}

pub static CACHED_CHANNELS: Mutex<
    CriticalSectionRawMutex,
    RefCell<heapless::Vec<CachedChannel, { channels::NUM_CHANNELS }>>,
> = Mutex::new(RefCell::new(heapless::Vec::new()));

/// Last received MeshCore advert.
pub struct LastAdvert {
    pub name: heapless::String<32>,
    pub pub_key_hex: heapless::String<16>,
    pub role: u8,
    pub sig_ok: bool,
    pub rssi: i16,
    pub snr_x4: i8,
    pub lat: i32,
    pub lon: i32,
}

pub static LAST_ADVERT: Mutex<CriticalSectionRawMutex, RefCell<Option<LastAdvert>>> =
    Mutex::new(RefCell::new(None));

pub static ADVERT_SIGNAL: Signal<CriticalSectionRawMutex, ()> = Signal::new();

/// Advert data forwarded to the BLE task for push to the companion app (0x8A).
pub struct AdvertBleNotif {
    pub pub_key: [u8; 32],
    pub adv_type: u8,
    pub rssi: i8,
    pub timestamp: u32,
    pub lat: i32,
    pub lon: i32,
    pub name: heapless::Vec<u8, 32>,
}

pub static ADVERT_BLE_CHANNEL: embassy_sync::channel::Channel<
    CriticalSectionRawMutex,
    AdvertBleNotif,
    4,
> = embassy_sync::channel::Channel::new();

/// Last received private message (TxtMsg).
pub struct LastPm {
    pub sender_name: heapless::String<32>,
    pub text: heapless::String<{ ::meshcore::payload::txt_msg::MAX_TXT_TEXT_SIZE }>,
    pub timestamp: u32,
    pub rssi: i16,
}

pub static LAST_PM: Mutex<CriticalSectionRawMutex, RefCell<Option<LastPm>>> =
    Mutex::new(RefCell::new(None));

pub static PM_SIGNAL: Signal<CriticalSectionRawMutex, ()> = Signal::new();

// ---------------------------------------------------------------------------
// Inter-task signals
// ---------------------------------------------------------------------------

/// Fired whenever a new message is pushed to `msg_queue`.
pub static MESSAGES_WAITING_SIGNAL: Signal<CriticalSectionRawMutex, ()> = Signal::new();

/// Fired when an auto-add advert is refused because the contact store is
/// full and the `AUTO_ADD_OVERWRITE_OLDEST` bit in `autoadd_config` is not
/// set. The BLE task consumes this to emit `PUSH_CODE_CONTACTS_FULL` (0x90).
pub static CONTACTS_FULL_SIGNAL: Signal<CriticalSectionRawMutex, ()> = Signal::new();

/// Queue of pub_keys for which the firmware has just learned (or refreshed)
/// a routing path. The BLE task drains this to emit `PUSH_CODE_PATH_UPDATED`
/// (0x81) notifications so the companion app can re-fetch the contact and
/// keep its own DB in sync — matching the reference `companion_radio`
/// behaviour. Sized at 4 to absorb short bursts without dropping updates;
/// a dropped update just means the phone sees the old path for one extra
/// reconnect cycle.
pub static PATH_UPDATED_CHANNEL: embassy_sync::channel::Channel<
    CriticalSectionRawMutex,
    [u8; ::meshcore::PUB_KEY_SIZE],
    4,
> = embassy_sync::channel::Channel::new();

/// In-RAM cache of the persisted `path_hash_mode` setting
/// (CMD_SET_PATH_HASH_MODE, 0x3D).
///
/// Value semantics match the reference: 0 ⇒ 1-byte per-hop hashes,
/// 1 ⇒ 2-byte, 2 ⇒ 3-byte. Values ≥ 3 are reserved and rejected by the
/// setter. Read on the hot TX path to compose `path_len_byte` for every
/// freshly-originated flood packet; loaded from flash once at boot and
/// refreshed whenever `CMD_SET_PATH_HASH_MODE` is received.
pub static PATH_HASH_MODE: core::sync::atomic::AtomicU8 = core::sync::atomic::AtomicU8::new(0);

/// Fired by the menu to request the BLE task to wipe and re-seed the channel
/// store.
pub static CHANNEL_RESET_SIGNAL: Signal<CriticalSectionRawMutex, ()> = Signal::new();

/// Fired by the menu when the boost-RX toggle changes so the BLE task can
/// persist it.
pub static BOOST_RX_CHANGED_SIGNAL: Signal<CriticalSectionRawMutex, ()> = Signal::new();

/// Fired by the menu to request the BLE task to clear all stored contacts.
pub static CONTACT_RESET_SIGNAL: Signal<CriticalSectionRawMutex, ()> = Signal::new();

/// Fired after a `SET_CHANNEL` or channel reset so the meshcore task reloads
/// channels.
pub static CHANNELS_CHANGED_SIGNAL: Signal<CriticalSectionRawMutex, ()> = Signal::new();

/// Signals the meshcore task that tuning params changed; carries the new
/// airtime_factor_x1000.
pub static TUNING_CHANGED_SIGNAL: Signal<CriticalSectionRawMutex, u32> = Signal::new();

/// Wakes the meshcore task out of `lora.receive_packet()` whenever a new TX
/// request is enqueued on `TX_CHANNEL`.
///
/// Without this, `receive_packet`'s ~15 s timeout becomes the worst-case TX
/// latency for any channel that isn't directly part of the meshcore task's
/// `select` race. The top-of-loop drain handles routing per channel — this
/// signal only exists to break the receive call so the loop iterates.
pub static TX_WAKEUP: Signal<CriticalSectionRawMutex, ()> = Signal::new();

/// 16-byte transport key for region-scoped flood packets.
pub static FLOOD_SCOPE_KEY: Mutex<CriticalSectionRawMutex, core::cell::Cell<Option<[u8; 16]>>> =
    Mutex::new(core::cell::Cell::new(None));

// ---------------------------------------------------------------------------
// Radio stats
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct RadioStats {
    pub noise_floor: i16,
    pub last_rssi: i8,
    pub last_snr_x4: i8,
    pub tx_air_secs: u32,
    pub rx_air_secs: u32,
}

pub static RADIO_STATS: Mutex<CriticalSectionRawMutex, core::cell::Cell<RadioStats>> =
    Mutex::new(core::cell::Cell::new(RadioStats {
        noise_floor: -120,
        last_rssi: 0,
        last_snr_x4: 0,
        tx_air_secs: 0,
        rx_air_secs: 0,
    }));

// ---------------------------------------------------------------------------
// Raw packet forwarding (meshcore → BLE)
// ---------------------------------------------------------------------------

pub struct RawLoRaPkt {
    pub snr_x4: i8,
    pub rssi: i8,
    pub len: usize,
    pub data: [u8; ::meshcore::MAX_TRANS_UNIT],
}

pub static RAW_PKT_CHANNEL: embassy_sync::channel::Channel<CriticalSectionRawMutex, RawLoRaPkt, 4> =
    embassy_sync::channel::Channel::new();

// ---------------------------------------------------------------------------
// Control data packets (RX: meshcore → BLE)
// ---------------------------------------------------------------------------

pub struct ControlDataPkt {
    pub snr_x4: i8,
    pub rssi: i8,
    pub path_len: u8,
    pub payload: heapless::Vec<u8, { ::meshcore::MAX_PAYLOAD_SIZE }>,
}

pub static CONTROL_DATA_PKT_CHANNEL: embassy_sync::channel::Channel<
    CriticalSectionRawMutex,
    ControlDataPkt,
    4,
> = embassy_sync::channel::Channel::new();

// ---------------------------------------------------------------------------
// Unified TX channel (BLE / menu / advert ticker → meshcore)
// ---------------------------------------------------------------------------

pub struct TxChannelMsg {
    pub channel_idx: u8,
    pub timestamp: u32,
    pub text: heapless::Vec<u8, { msg_queue::MAX_TEXT }>,
}

pub struct TxPrivateMsg {
    pub recipient_pub_key: [u8; ::meshcore::PUB_KEY_SIZE],
    pub timestamp: u32,
    pub text: heapless::Vec<u8, { msg_queue::MAX_TEXT }>,
    pub txt_type: u8,
    pub attempt: u8,
}

pub struct TxTracePath {
    pub tag: u32,
    pub auth: u32,
    pub flags: u8,
    pub path: heapless::Vec<u8, { ::meshcore::MAX_PATH_SIZE }>,
}

pub struct TxLogin {
    pub pub_key: [u8; ::meshcore::PUB_KEY_SIZE],
    pub password: heapless::Vec<u8, 15>,
}

/// `PAYLOAD_TYPE_REQ` with `REQ_TYPE_GET_STATUS` (0x01) — authenticated
/// repeater-stats query sent after a successful login to a repeater.
pub struct TxAdminStatusReq {
    pub pub_key: [u8; ::meshcore::PUB_KEY_SIZE],
    pub tag: u32,
}

/// Maximum `req_data` payload for a `CMD_SEND_BINARY_REQ`.
pub const MAX_BINARY_REQ_PARAMS: usize = 24;

/// Generic `PAYLOAD_TYPE_REQ` with an opaque `[req_type:1][params...]` body.
pub struct TxBinaryReq {
    pub pub_key: [u8; ::meshcore::PUB_KEY_SIZE],
    pub tag: u32,
    pub req_data: heapless::Vec<u8, MAX_BINARY_REQ_PARAMS>,
}

pub struct TxTelemReq {
    pub pub_key: [u8; 32],
    pub tag: u32,
}

pub struct TxDiscoveryReq {
    pub pub_key: [u8; 32],
    pub tag: u32,
}

pub struct TxControlData {
    pub payload: heapless::Vec<u8, { ::meshcore::MAX_PAYLOAD_SIZE }>,
}

/// Every outgoing LoRa transmission flows through this single enum.
/// The meshcore task drains the channel and dispatches to the appropriate
/// send function (encryption, framing, serialization, `lora.send_message`).
pub enum TxRequest {
    ChannelMsg(TxChannelMsg),
    PrivateMsg(TxPrivateMsg),
    Trace(TxTracePath),
    Login(TxLogin),
    AdminStatusReq(TxAdminStatusReq),
    BinaryReq(TxBinaryReq),
    TelemReq(TxTelemReq),
    DiscoveryReq(TxDiscoveryReq),
    ControlData(TxControlData),
    Advert(meshcore::AdvertMode),
    /// Pre-serialized frame (for relay / repeat). Sent as-is via
    /// `lora.send_message()`.
    RawFrame {
        data: [u8; ::meshcore::MAX_TRANS_UNIT],
        len: usize,
    },
}

pub static TX_CHANNEL: embassy_sync::channel::Channel<CriticalSectionRawMutex, TxRequest, 16> =
    embassy_sync::channel::Channel::new();

/// Convenience: push a `TxRequest` and wake the meshcore task.
/// Returns `Err` if the channel is full.
pub fn tx_send(req: TxRequest) -> Result<(), TxRequest> {
    match TX_CHANNEL.try_send(req) {
        Ok(()) => {
            TX_WAKEUP.signal(());
            Ok(())
        }
        Err(embassy_sync::channel::TrySendError::Full(r)) => Err(r),
    }
}

// ---------------------------------------------------------------------------
// Result channels (meshcore → BLE)
// ---------------------------------------------------------------------------

pub struct DiscoveryResult {
    pub pub_key: [u8; ::meshcore::PUB_KEY_SIZE],
    pub out_path_len_byte: u8,
    pub out_path: heapless::Vec<u8, { ::meshcore::MAX_PATH_SIZE }>,
    pub in_path_len_byte: u8,
    pub in_path: heapless::Vec<u8, { ::meshcore::MAX_PATH_SIZE }>,
}

pub static DISCOVERY_RESULT_CHANNEL: embassy_sync::channel::Channel<
    CriticalSectionRawMutex,
    DiscoveryResult,
    2,
> = embassy_sync::channel::Channel::new();

/// Raw `RepeaterStats` blob from a `REQ_TYPE_GET_STATUS` reply, ready for the
/// companion protocol's `PUSH_CODE_STATUS_RESPONSE` (0x87) wire format.
///
/// The legacy anonymous status-ping path also produces this struct, with only
/// `batt_milli_volts` and `total_up_time_secs` populated and the rest zeroed.
pub struct StatusResult {
    pub pub_key: [u8; ::meshcore::PUB_KEY_SIZE],
    /// Raw 56-byte `RepeaterStats` C struct. Forwarded verbatim to the phone.
    pub stats: [u8; 56],
}

pub static STATUS_RESULT_CHANNEL: embassy_sync::channel::Channel<
    CriticalSectionRawMutex,
    StatusResult,
    2,
> = embassy_sync::channel::Channel::new();

/// Parsed fields of a `RepeaterStats` reply to `REQ_TYPE_GET_STATUS`.
///
/// C++ reference: `examples/simple_repeater/MyMesh.h` `struct RepeaterStats` —
/// 56 bytes of tightly-packed little-endian fields, no padding.
#[derive(Clone, Copy, Default)]
pub struct AdminStatusResult {
    pub pub_key: [u8; ::meshcore::PUB_KEY_SIZE],
    pub tag: u32,
    pub batt_milli_volts: u16,
    pub curr_tx_queue_len: u16,
    pub noise_floor: i16,
    pub last_rssi: i16,
    pub n_packets_recv: u32,
    pub n_packets_sent: u32,
    pub total_air_time_secs: u32,
    pub total_up_time_secs: u32,
    pub n_sent_flood: u32,
    pub n_sent_direct: u32,
    pub n_recv_flood: u32,
    pub n_recv_direct: u32,
    pub err_events: u16,
    pub last_snr_x4: i16,
    pub n_direct_dups: u16,
    pub n_flood_dups: u16,
    pub total_rx_air_time_secs: u32,
    pub n_recv_errors: u32,
}

pub static ADMIN_STATUS_RESULT_CHANNEL: embassy_sync::channel::Channel<
    CriticalSectionRawMutex,
    AdminStatusResult,
    2,
> = embassy_sync::channel::Channel::new();

/// Maximum `BinaryResult` body size. The longest reply is `GET_NEIGHBOURS`
/// which for 10 neighbours × 9 bytes + 4 header = 94 bytes, and
/// `GET_ACCESS_LIST` which is roughly bounded by the same.
pub const MAX_BINARY_RESP_BODY: usize = 176;

/// Result of a `TxBinaryReq`, echoed to the companion app via
/// `PUSH_CODE_BINARY_RESPONSE` (0x8C). `body` is the raw response bytes from
/// the repeater's `handleRequest()` starting after the echoed tag.
pub struct BinaryResult {
    pub pub_key: [u8; ::meshcore::PUB_KEY_SIZE],
    pub tag: u32,
    pub body: heapless::Vec<u8, MAX_BINARY_RESP_BODY>,
}

pub static BINARY_RESULT_CHANNEL: embassy_sync::channel::Channel<
    CriticalSectionRawMutex,
    BinaryResult,
    2,
> = embassy_sync::channel::Channel::new();

pub struct TraceResult {
    pub path_len: u8,
    pub flags: u8,
    pub tag: u32,
    pub auth_code: u32,
    pub path_hashes: heapless::Vec<u8, { ::meshcore::MAX_PATH_SIZE }>,
    pub path_snrs: heapless::Vec<u8, { ::meshcore::MAX_PATH_SIZE }>,
    pub final_snr: i8,
}

pub static TRACE_RESULT_CHANNEL: embassy_sync::channel::Channel<
    CriticalSectionRawMutex,
    TraceResult,
    2,
> = embassy_sync::channel::Channel::new();

pub struct LoginResult {
    pub success: bool,
    pub is_admin: u8,
    pub pub_key: [u8; ::meshcore::PUB_KEY_SIZE],
    pub tag: u32,
    pub acl_perms: u8,
    pub fw_ver_level: u8,
}

pub static LOGIN_RESULT_CHANNEL: embassy_sync::channel::Channel<
    CriticalSectionRawMutex,
    LoginResult,
    2,
> = embassy_sync::channel::Channel::new();

pub struct TelemResult {
    pub pub_key: [u8; ::meshcore::PUB_KEY_SIZE],
    pub lpp: heapless::Vec<u8, 176>,
}

pub static TELEM_RESULT_CHANNEL: embassy_sync::channel::Channel<
    CriticalSectionRawMutex,
    TelemResult,
    2,
> = embassy_sync::channel::Channel::new();

// ---------------------------------------------------------------------------
// ACK tracking
// ---------------------------------------------------------------------------

pub struct AckEvent {
    pub ack_crc: u32,
    pub trip_time_ms: u32,
}

pub static ACK_EVENT_CHANNEL: embassy_sync::channel::Channel<CriticalSectionRawMutex, AckEvent, 2> =
    embassy_sync::channel::Channel::new();

#[derive(Clone, Copy)]
pub struct PendingAck {
    pub ack_hash: u32,
    pub sent_at: embassy_time::Instant,
}

pub static PENDING_ACK: Mutex<CriticalSectionRawMutex, core::cell::Cell<Option<PendingAck>>> =
    Mutex::new(core::cell::Cell::new(None));

// ---------------------------------------------------------------------------
// Pending request tags
// ---------------------------------------------------------------------------

pub static PENDING_DISCOVERY_TAG: Mutex<CriticalSectionRawMutex, core::cell::Cell<Option<u32>>> =
    Mutex::new(core::cell::Cell::new(None));

pub static PENDING_STATUS_PUBKEY: Mutex<
    CriticalSectionRawMutex,
    core::cell::Cell<Option<[u8; ::meshcore::PUB_KEY_SIZE]>>,
> = Mutex::new(core::cell::Cell::new(None));

pub static PENDING_TELEM_TAG: Mutex<CriticalSectionRawMutex, core::cell::Cell<Option<u32>>> =
    Mutex::new(core::cell::Cell::new(None));

/// Tag of the in-flight `REQ_TYPE_GET_STATUS` request. The repeater echoes
/// this value as the first 4 bytes of its `PAYLOAD_TYPE_RESPONSE` plaintext,
/// and we match it here to route the parsed `RepeaterStats` to the result
/// channel.
pub static PENDING_ADMIN_STATUS_TAG: Mutex<CriticalSectionRawMutex, core::cell::Cell<Option<u32>>> =
    Mutex::new(core::cell::Cell::new(None));

/// Tag of the in-flight generic `CMD_SEND_BINARY_REQ` request. Tag-based
/// routing delivers the echoed-timestamp response to `BINARY_RESULT_CHANNEL`.
pub static PENDING_BINARY_REQ_TAG: Mutex<CriticalSectionRawMutex, core::cell::Cell<Option<u32>>> =
    Mutex::new(core::cell::Cell::new(None));

/// Fast-path hint for the contact-scan loops in the receive handlers.
///
/// Every outbound request handler (`send_login`, `send_admin_status_request`,
/// `send_telem_request`, ...) sets this to the target's pub_key. The receive
/// handlers try `find_by_key` on this hint first — a single O(1) KV lookup
/// against the prefix index — before falling back to the O(N) linear scan
/// over all contact slots. The scan is required only for unsolicited messages
/// from a peer we haven't recently talked to.
///
/// Without this hint, the ekv log scan costs ~50 ms per slot × 300 slots =
/// ~15 s per decrypt, which is what we were observing for login and status
/// responses.
pub static LAST_REQ_TARGET: Mutex<
    CriticalSectionRawMutex,
    core::cell::Cell<Option<[u8; ::meshcore::PUB_KEY_SIZE]>>,
> = Mutex::new(core::cell::Cell::new(None));
