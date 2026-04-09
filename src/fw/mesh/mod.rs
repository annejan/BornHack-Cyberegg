//! MeshCore mesh networking stack.
//!
//! This module contains everything related to the LoRa mesh: radio driver,
//! BLE companion protocol, packet codec, contact/channel/bond/KV storage,
//! and the inter-task channels that connect them.
//!
//! Gated behind `#[cfg(feature = "mesh")]` in `fw/mod.rs`.

pub mod ble;
pub mod bonds;
pub mod channels;
pub mod contacts;
pub mod device_identity;
pub mod kv;
pub mod meshcore;
pub mod msg_queue;
pub mod settings;
pub mod storage;
pub mod sx1262;

// Re-export the meshcore listener entry point for embassy.rs.
pub use meshcore::run_meshcore_listener;

use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::signal::Signal;
use core::cell::RefCell;

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
    CriticalSectionRawMutex, AdvertBleNotif, 4,
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

/// Fired by the menu to request the BLE task to wipe and re-seed the channel store.
pub static CHANNEL_RESET_SIGNAL: Signal<CriticalSectionRawMutex, ()> = Signal::new();

/// Fired by the menu when the boost-RX toggle changes so the BLE task can persist it.
pub static BOOST_RX_CHANGED_SIGNAL: Signal<CriticalSectionRawMutex, ()> = Signal::new();

/// Fired by the menu to request the BLE task to clear all stored contacts.
pub static CONTACT_RESET_SIGNAL: Signal<CriticalSectionRawMutex, ()> = Signal::new();

/// Fired after a `SET_CHANNEL` or channel reset so the meshcore task reloads channels.
pub static CHANNELS_CHANGED_SIGNAL: Signal<CriticalSectionRawMutex, ()> = Signal::new();

/// Signals the meshcore task that tuning params changed; carries the new airtime_factor_x1000.
pub static TUNING_CHANGED_SIGNAL: Signal<CriticalSectionRawMutex, u32> = Signal::new();

/// Signals the meshcore task to transmit a self-advert.
pub static SEND_ADVERT_SIGNAL: Signal<CriticalSectionRawMutex, meshcore::AdvertMode> =
    Signal::new();

/// 16-byte transport key for region-scoped flood packets.
pub static FLOOD_SCOPE_KEY: Mutex<
    CriticalSectionRawMutex,
    core::cell::Cell<Option<[u8; 16]>>,
> = Mutex::new(core::cell::Cell::new(None));

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
        last_rssi:    0,
        last_snr_x4:  0,
        tx_air_secs:  0,
        rx_air_secs:  0,
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
// Control data packets
// ---------------------------------------------------------------------------

pub struct TxControlData {
    pub payload: heapless::Vec<u8, { ::meshcore::MAX_PAYLOAD_SIZE }>,
}

pub static TX_CONTROL_DATA_CHANNEL: embassy_sync::channel::Channel<
    CriticalSectionRawMutex, TxControlData, 2,
> = embassy_sync::channel::Channel::new();

pub struct ControlDataPkt {
    pub snr_x4: i8,
    pub rssi: i8,
    pub path_len: u8,
    pub payload: heapless::Vec<u8, { ::meshcore::MAX_PAYLOAD_SIZE }>,
}

pub static CONTROL_DATA_PKT_CHANNEL: embassy_sync::channel::Channel<
    CriticalSectionRawMutex, ControlDataPkt, 4,
> = embassy_sync::channel::Channel::new();

// ---------------------------------------------------------------------------
// TX channels (BLE → meshcore)
// ---------------------------------------------------------------------------

pub struct TxChannelMsg {
    pub channel_idx: u8,
    pub timestamp: u32,
    pub text: heapless::Vec<u8, { msg_queue::MAX_TEXT }>,
}

pub static TX_MSG_CHANNEL: embassy_sync::channel::Channel<
    CriticalSectionRawMutex, TxChannelMsg, 16,
> = embassy_sync::channel::Channel::new();

pub struct TxPrivateMsg {
    pub recipient_pub_key: [u8; ::meshcore::PUB_KEY_SIZE],
    pub timestamp: u32,
    pub text: heapless::Vec<u8, { msg_queue::MAX_TEXT }>,
    pub txt_type: u8,
    pub attempt: u8,
}

pub static TX_PM_CHANNEL: embassy_sync::channel::Channel<CriticalSectionRawMutex, TxPrivateMsg, 4> =
    embassy_sync::channel::Channel::new();

pub struct TxTracePath {
    pub tag: u32,
    pub auth: u32,
    pub flags: u8,
    pub path: heapless::Vec<u8, { ::meshcore::MAX_PATH_SIZE }>,
}

pub static TX_TRACE_CHANNEL: embassy_sync::channel::Channel<
    CriticalSectionRawMutex, TxTracePath, 2,
> = embassy_sync::channel::Channel::new();

pub struct TxLogin {
    pub pub_key: [u8; ::meshcore::PUB_KEY_SIZE],
    pub password: heapless::Vec<u8, 15>,
}

pub static TX_LOGIN_CHANNEL: embassy_sync::channel::Channel<CriticalSectionRawMutex, TxLogin, 2> =
    embassy_sync::channel::Channel::new();

pub struct TxStatusReq {
    pub pub_key: [u8; ::meshcore::PUB_KEY_SIZE],
}

pub static TX_STATUS_REQ_CHANNEL: embassy_sync::channel::Channel<CriticalSectionRawMutex, TxStatusReq, 2> =
    embassy_sync::channel::Channel::new();

pub struct TxTelemReq {
    pub pub_key: [u8; 32],
    pub tag: u32,
}

pub static TX_TELEM_REQ_CHANNEL: embassy_sync::channel::Channel<
    CriticalSectionRawMutex, TxTelemReq, 2,
> = embassy_sync::channel::Channel::new();

pub struct TxDiscoveryReq {
    pub pub_key: [u8; 32],
    pub tag: u32,
}

pub static TX_DISCOVERY_CHANNEL: embassy_sync::channel::Channel<
    CriticalSectionRawMutex, TxDiscoveryReq, 2,
> = embassy_sync::channel::Channel::new();

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
    CriticalSectionRawMutex, DiscoveryResult, 2,
> = embassy_sync::channel::Channel::new();

pub struct StatusResult {
    pub pub_key: [u8; ::meshcore::PUB_KEY_SIZE],
    pub uptime_secs: u32,
    pub battery_mv: u16,
}

pub static STATUS_RESULT_CHANNEL: embassy_sync::channel::Channel<
    CriticalSectionRawMutex, StatusResult, 2,
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
    CriticalSectionRawMutex, TraceResult, 2,
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
    CriticalSectionRawMutex, LoginResult, 2,
> = embassy_sync::channel::Channel::new();

pub struct TelemResult {
    pub pub_key: [u8; ::meshcore::PUB_KEY_SIZE],
    pub lpp: heapless::Vec<u8, 176>,
}

pub static TELEM_RESULT_CHANNEL: embassy_sync::channel::Channel<
    CriticalSectionRawMutex, TelemResult, 2,
> = embassy_sync::channel::Channel::new();

// ---------------------------------------------------------------------------
// ACK tracking
// ---------------------------------------------------------------------------

pub struct AckEvent {
    pub ack_crc: u32,
    pub trip_time_ms: u32,
}

pub static ACK_EVENT_CHANNEL: embassy_sync::channel::Channel<
    CriticalSectionRawMutex, AckEvent, 2,
> = embassy_sync::channel::Channel::new();

#[derive(Clone, Copy)]
pub struct PendingAck {
    pub ack_hash: u32,
    pub sent_at: embassy_time::Instant,
}

pub static PENDING_ACK: Mutex<
    CriticalSectionRawMutex,
    core::cell::Cell<Option<PendingAck>>,
> = Mutex::new(core::cell::Cell::new(None));

// ---------------------------------------------------------------------------
// Pending request tags
// ---------------------------------------------------------------------------

pub static PENDING_DISCOVERY_TAG: Mutex<
    CriticalSectionRawMutex,
    core::cell::Cell<Option<u32>>,
> = Mutex::new(core::cell::Cell::new(None));

pub static PENDING_STATUS_PUBKEY: Mutex<
    CriticalSectionRawMutex,
    core::cell::Cell<Option<[u8; ::meshcore::PUB_KEY_SIZE]>>,
> = Mutex::new(core::cell::Cell::new(None));

pub static PENDING_TELEM_TAG: Mutex<
    CriticalSectionRawMutex,
    core::cell::Cell<Option<u32>>,
> = Mutex::new(core::cell::Cell::new(None));
