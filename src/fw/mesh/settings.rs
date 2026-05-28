//! Device settings — the single API for all persistent device configuration.
//!
//! All user-configurable settings **and** the device identity seed are stored
//! in the `"settings"` KV namespace, making this the one authoritative place
//! for every piece of device config data.
//!
//! > **Note for existing devices**: the identity seed previously lived under
//! > the `"meshcore"` namespace.  Moving it here means a device that already
//! > has a seed in `"meshcore:identity_seed"` will generate a **new** Ed25519
//! > keypair on the first boot with this firmware.  This is intentional for
//! > development builds.
//!
//! # Stored keys
//!
//! | Key              | Contents                                       | Bytes |
//! |------------------|------------------------------------------------|-------|
//! | `"identity_seed"`| Ed25519 seed — managed by [`device_identity`]  | 32    |
//! | `"name"`         | Node name — UTF-8, max [`MAX_NODE_NAME`] B     | 1–31  |
//! | `"radio"`        | LoRa radio params (freq, bw, sf, cr, pwr, rep) | 12    |
//! | `"other"`        | manual_add, telemetry, advert_loc, multi_acks  | 6     |
//! | `"autoadd"`      | autoadd_config + autoadd_max_hops              | 2     |
//! | `"path_hash"`    | path_hash_mode (0/1/2)                         | 1     |
//! | `"epd_lut"`      | EPD LUT cycle-duration scale override          | 1     |
//!
//! # Companion protocol mapping
//!
//! | Command byte | Name                  | Key(s) written   |
//! |--------------|-----------------------|------------------|
//! | `0x08`       | SET_ADVERT_NAME       | `"name"`         |
//! | `0x0B`       | SET_RADIO_PARAMS      | `"radio"`        |
//! | `0x0C`       | SET_RADIO_TX_POWER    | `"radio"`        |
//! | `0x26`       | SET_OTHER_PARAMS      | `"other"`        |
//! | `0x3A`       | SET_AUTOADD_CONFIG    | `"autoadd"`      |
//! | `0x3D`       | SET_PATH_HASH_MODE    | `"path_hash"`    |

pub use device_identity::DeviceIdentity;

use super::device_identity;
use crate::fw::kv;

// ---------------------------------------------------------------------------
// Shared KV namespace
// ---------------------------------------------------------------------------

fn ns() -> kv::KvNamespace {
    kv::namespace("settings")
}

// ---------------------------------------------------------------------------
// Device identity  (Ed25519 keypair)
// ---------------------------------------------------------------------------

/// Load the Ed25519 device identity from the settings namespace, or generate
/// and persist a new one on first boot.
pub async fn load_or_create_identity() -> DeviceIdentity {
    device_identity::load_or_create(ns()).await
}

/// Generate a brand-new random identity and overwrite the stored seed.
///
/// Use this when the user requests a key rotation via the menu.
pub async fn regenerate_identity() -> DeviceIdentity {
    device_identity::regenerate(ns()).await
}

/// Delete the stored identity seed.  The next call to
/// [`load_or_create_identity`] will generate a fresh keypair.
pub async fn delete_identity() {
    device_identity::delete(ns()).await
}

// ---------------------------------------------------------------------------
// Node name  (CMD_SET_ADVERT_NAME  0x08)
// ---------------------------------------------------------------------------

/// Maximum node name length in bytes (matches MeshCore `node_name[32] - 1`).
pub const MAX_NODE_NAME: usize = 31;

/// Read the persisted node name into `buf`.
///
/// Returns the number of valid bytes written.  Returns 0 if no name has been
/// stored yet (first boot); the caller should fall back to the hardware ID.
pub async fn get_node_name(buf: &mut [u8; MAX_NODE_NAME]) -> usize {
    match ns().get("name", buf).await {
        Ok(n) if n > 0 && n <= MAX_NODE_NAME => n,
        _ => 0,
    }
}

/// Persist `name` (UTF-8, max [`MAX_NODE_NAME`] bytes) to flash.
pub async fn set_node_name(name: &[u8]) -> Result<(), kv::KvError> {
    let len = name.len().min(MAX_NODE_NAME);
    ns().set("name", &name[..len], true).await
}

// ---------------------------------------------------------------------------
// Radio parameters  (CMD_SET_RADIO_PARAMS 0x0B / CMD_SET_RADIO_TX_POWER 0x0C)
// ---------------------------------------------------------------------------

/// LoRa radio parameters stored on-device.
#[derive(Clone, Copy, Debug, defmt::Format)]
pub struct RadioParams {
    /// Carrier frequency in Hz (e.g. 869_618_000).
    pub freq_hz: u32,
    /// Bandwidth in Hz (e.g. 62_500).
    pub bw_hz: u32,
    /// Spreading factor (5–12).
    pub sf: u8,
    /// Coding rate — 5 = 4/5, 6 = 4/6, 7 = 4/7, 8 = 4/8.
    pub cr: u8,
    /// TX power in dBm.
    pub tx_power: i8,
    /// Client-repeat mode enabled.
    pub client_repeat: bool,
}

impl RadioParams {
    fn to_bytes(self) -> [u8; 12] {
        let mut b = [0u8; 12];
        b[0..4].copy_from_slice(&self.freq_hz.to_le_bytes());
        b[4..8].copy_from_slice(&self.bw_hz.to_le_bytes());
        b[8] = self.sf;
        b[9] = self.cr;
        b[10] = self.tx_power as u8;
        b[11] = self.client_repeat as u8;
        b
    }

    fn from_bytes(b: &[u8; 12]) -> Self {
        Self {
            freq_hz: u32::from_le_bytes([b[0], b[1], b[2], b[3]]),
            bw_hz: u32::from_le_bytes([b[4], b[5], b[6], b[7]]),
            sf: b[8],
            cr: b[9],
            tx_power: b[10] as i8,
            client_repeat: b[11] != 0,
        }
    }
}

/// Default radio parameters — **BornHack Turbo (ETSI g4)**, the badge's
/// out-of-the-box preset.
///
/// 869.85 MHz · BW 250 kHz · SF8 · CR 4/5 · 14 dBm TX
///
/// Spectrum-separated from the standard EU/UK Narrow channel at 869.618 MHz,
/// so badges on this preset don't share airtime with stock MeshCore badges.
/// TX power is kept at the legacy 14 dBm default; for fully compliant ETSI
/// g4 100 % duty-cycle operation, drop it to +7 dBm via the Power menu.
///
/// Coding rate uses **MeshCore protocol encoding**: 5 = CR 4/5, 6 = CR 4/6,
/// etc. (distinct from the sx126x hardware register encoding where CR4_5 = 1).
pub const DEFAULT_RADIO: RadioParams = RadioParams {
    freq_hz: 869_850_000,
    bw_hz: 250_000,
    sf: 8,
    cr: 5, // CR 4/5 in MeshCore protocol encoding
    tx_power: 14,
    client_repeat: false,
};

/// Read the persisted radio parameters.  Returns `None` if not yet stored.
pub async fn get_radio_params() -> Option<RadioParams> {
    let mut b = [0u8; 12];
    match ns().get("radio", &mut b).await {
        Ok(12) => Some(RadioParams::from_bytes(&b)),
        _ => None,
    }
}

/// Read the persisted radio parameters, or return [`DEFAULT_RADIO`] if not
/// stored.
pub async fn get_radio_params_or_default() -> RadioParams {
    get_radio_params().await.unwrap_or(DEFAULT_RADIO)
}

/// Persist radio parameters to flash.
pub async fn set_radio_params(p: RadioParams) -> Result<(), kv::KvError> {
    ns().set("radio", &p.to_bytes(), true).await
}

// ---------------------------------------------------------------------------
// Position  (CMD_SET_ADVERT_LATLON 0x0E)
// ---------------------------------------------------------------------------

/// Advertised GPS position stored on-device.
///
/// Values are in **microdegrees** (integer × 1 000 000), matching the
/// MeshCore wire format.  `(0, 0)` is the default and means "no position".
#[derive(Clone, Copy, Debug, defmt::Format)]
pub struct Position {
    /// Latitude in microdegrees (e.g. 55_670_000 for 55.67° N).
    pub lat: i32,
    /// Longitude in microdegrees (e.g. 12_590_000 for 12.59° E).
    pub lon: i32,
}

/// Default position — (0, 0), meaning "no position set".
pub const DEFAULT_POSITION: Position = Position { lat: 0, lon: 0 };

/// Read the persisted position.  Returns `None` if not yet stored.
pub async fn get_position() -> Option<Position> {
    let mut b = [0u8; 8];
    match ns().get("pos", &mut b).await {
        Ok(8) => Some(Position {
            lat: i32::from_le_bytes([b[0], b[1], b[2], b[3]]),
            lon: i32::from_le_bytes([b[4], b[5], b[6], b[7]]),
        }),
        _ => None,
    }
}

/// Read the persisted position, or return [`DEFAULT_POSITION`] if not stored.
pub async fn get_position_or_default() -> Position {
    get_position().await.unwrap_or(DEFAULT_POSITION)
}

/// Persist a position to flash.
pub async fn set_position(p: Position) -> Result<(), kv::KvError> {
    let mut b = [0u8; 8];
    b[0..4].copy_from_slice(&p.lat.to_le_bytes());
    b[4..8].copy_from_slice(&p.lon.to_le_bytes());
    ns().set("pos", &b, true).await
}

// ---------------------------------------------------------------------------
// Ignore blink  (menu only)
// ---------------------------------------------------------------------------

pub async fn get_ignore_blink() -> bool {
    let mut b = [0u8; 1];
    match ns().get("no_blink", &mut b).await {
        Ok(1) => b[0] != 0,
        _ => false,
    }
}

pub async fn set_ignore_blink(ignore: bool) -> Result<(), kv::KvError> {
    ns().set("no_blink", &[ignore as u8], true).await
}

// ---------------------------------------------------------------------------
// LoRa enabled  (menu only — not part of the companion protocol)
// ---------------------------------------------------------------------------

/// Read the persisted LoRa enabled flag.  Returns `true` (enabled) if not
/// stored.
pub async fn get_lora_enabled() -> bool {
    let mut b = [0u8; 1];
    match ns().get("lora_en", &mut b).await {
        Ok(1) => b[0] != 0,
        _ => true,
    }
}

/// Persist the LoRa enabled flag to flash.
pub async fn set_lora_enabled(enabled: bool) -> Result<(), kv::KvError> {
    ns().set("lora_en", &[enabled as u8], true).await
}

// ---------------------------------------------------------------------------
// BLE enabled  (menu only — not part of the companion protocol)
// ---------------------------------------------------------------------------

/// Read the persisted BLE enabled flag.  Returns `true` (enabled) if not
/// stored.
pub async fn get_ble_enabled() -> bool {
    let mut b = [0u8; 1];
    match ns().get("ble_en", &mut b).await {
        Ok(1) => b[0] != 0,
        _ => true,
    }
}

/// Persist the BLE enabled flag to flash.
pub async fn set_ble_enabled(enabled: bool) -> Result<(), kv::KvError> {
    ns().set("ble_en", &[enabled as u8], true).await
}

// ---------------------------------------------------------------------------
// Advert scheduling  (menu only — not part of the companion protocol)
// ---------------------------------------------------------------------------

/// Periodic self-advert scheduling. Not a MeshCore companion command — this
/// lives in flash so the setting survives reboots, but it is only written
/// from the on-device menu.
#[derive(Clone, Copy, Debug, defmt::Format)]
pub struct AdvertConfig {
    /// When false, the advert ticker task never fires.
    pub enabled: bool,
    /// Interval in whole hours between periodic flood adverts.
    /// Menu only exposes the values 2, 4, 8, 16, 32, 64, 96.
    pub interval_hours: u8,
}

/// Default: adverts enabled, 4-hour interval.
pub const DEFAULT_ADVERT: AdvertConfig = AdvertConfig {
    enabled: true,
    interval_hours: 16,
};

pub async fn get_advert_config_or_default() -> AdvertConfig {
    let mut b = [0u8; 2];
    match ns().get("advert", &mut b).await {
        Ok(2) => AdvertConfig {
            enabled: b[0] != 0,
            interval_hours: b[1],
        },
        _ => DEFAULT_ADVERT,
    }
}

pub async fn set_advert_config(cfg: AdvertConfig) -> Result<(), kv::KvError> {
    ns().set("advert", &[cfg.enabled as u8, cfg.interval_hours], true)
        .await
}

// ---------------------------------------------------------------------------
// Other parameters  (CMD_SET_OTHER_PARAMS 0x26)
// ---------------------------------------------------------------------------

/// Miscellaneous node behaviour settings.
///
/// `telemetry_mode_loc` and `telemetry_mode_env` are not present here —
/// this device has no GPS or environment sensors, so those wire-protocol
/// bits are always emitted as zero.  The KV layout still reserves them
/// in the packed telemetry byte for forward compatibility.
#[derive(Clone, Copy, Debug, defmt::Format)]
pub struct OtherParams {
    /// 0 = auto-add contacts, 1 = manual approval required.
    pub manual_add_contacts: u8,
    /// Telemetry mode — base sensors (0 = deny, 1 = use contact.flags, 2 =
    /// allow all).
    pub telemetry_mode_base: u8,
    /// Location broadcast policy: 0 = off, 1 = share.
    pub advert_loc_policy: u8,
    /// Multi-ACK aggregation: 0 = off, 1 = on.
    pub multi_acks: u8,
}

impl OtherParams {
    fn to_bytes(self) -> [u8; 6] {
        [
            self.manual_add_contacts,
            self.telemetry_mode_base, // upper bits (loc/env) intentionally 0
            self.advert_loc_policy,
            self.multi_acks,
            0, // reserved
            0, // reserved
        ]
    }

    fn from_bytes(b: &[u8; 6]) -> Self {
        Self {
            manual_add_contacts: b[0],
            telemetry_mode_base: b[1] & 0x03,
            advert_loc_policy: b[2],
            multi_acks: b[3],
        }
    }
}

/// Read the persisted other-params.  Returns `None` if not yet stored.
pub async fn get_other_params() -> Option<OtherParams> {
    let mut b = [0u8; 6];
    match ns().get("other", &mut b).await {
        Ok(6) => Some(OtherParams::from_bytes(&b)),
        _ => None,
    }
}

/// Persist other-params to flash.
pub async fn set_other_params(p: OtherParams) -> Result<(), kv::KvError> {
    ns().set("other", &p.to_bytes(), true).await
}

// ---------------------------------------------------------------------------
// Auto-add contact config  (CMD_SET_AUTOADD_CONFIG 0x3A)
// ---------------------------------------------------------------------------

/// Read `(autoadd_config, autoadd_max_hops)`.  Returns `(0, 0)` if not stored.
pub async fn get_autoadd_config() -> (u8, u8) {
    let mut b = [0u8; 2];
    match ns().get("autoadd", &mut b).await {
        Ok(2) => (b[0], b[1]),
        _ => (0, 0),
    }
}

/// Persist `(autoadd_config, autoadd_max_hops)` to flash.
pub async fn set_autoadd_config(config: u8, max_hops: u8) -> Result<(), kv::KvError> {
    ns().set("autoadd", &[config, max_hops], true).await
}

// ---------------------------------------------------------------------------
// Boost RX gain  (menu toggle, persisted locally)
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Timezone offset  (menu stepper, persisted locally)
// ---------------------------------------------------------------------------

/// Read the persisted UTC hour offset (-12..=+14).  Returns +2 (CEST,
/// Europe/Copenhagen summer) when no value has been stored yet — most
/// badges ship into a Bornhack-style summer venue, so this saves the
/// user from immediately having to flip Settings → Timezone before the
/// clock face shows the right time.
pub async fn get_timezone() -> i8 {
    let mut b = [0u8; 1];
    match ns().get("tz", &mut b).await {
        Ok(1) => b[0] as i8,
        _ => 2,
    }
}

/// Persist the UTC hour offset to flash.
pub async fn set_timezone(offset: i8) -> Result<(), kv::KvError> {
    ns().set("tz", &[offset as u8], true).await
}

/// Read the persisted boost-RX flag.
///
/// Returns `true` on first boot (no KV value yet) to match MeshCore 1.15.0's
/// `radio.rxgain = true` default (upstream commit `ff5aad71`).  Once the
/// user toggles the menu setting at least once, the persisted value wins
/// — even an explicit `false` sticks across reboots.
pub async fn get_boost_rx() -> bool {
    let mut b = [0u8; 1];
    match ns().get("boost_rx", &mut b).await {
        Ok(1) => b[0] != 0,
        _ => true,
    }
}

/// Persist the boost-RX flag to flash.
pub async fn set_boost_rx(enabled: bool) -> Result<(), kv::KvError> {
    ns().set("boost_rx", &[enabled as u8], true).await
}

// ---------------------------------------------------------------------------
// Flood scope key  (region-scoped flood — applies to all originated flood TX)
// ---------------------------------------------------------------------------
//
// Mirrors the reference C++ MeshCore implementation's single global
// `send_scope` (see `examples/companion_radio/MyMesh.cpp::sendFloodScoped`).
// `Some(key)` ⇒ outgoing flood packets carry the matching transport code so
// regional repeaters that hold the same key forward them; foreign repeaters
// silently drop them.  `None` ⇒ unscoped flood, accepted everywhere.
//
// On first boot we seed the persisted value with the dk-bornhack region key
// so badges shipped to BornHack route through event repeaters by default.
// The companion `SET_FLOOD_SCOPE` (0x36) command lets ops re-key or clear
// the scope at runtime, and the change is persisted so it survives reboots.

/// First-boot default flood-scope key: SHA-256-truncated from `"dk-bornhack"`.
/// Derived on demand — no flash write at first boot.  Used only when neither
/// the MeshCore-1.15 `def_scope` slot nor a phone-set scope is present.
pub fn dk_bornhack_default_scope() -> [u8; 16] {
    meshcore::channel::key_from_hashtag("dk-bornhack")
}

// ---------------------------------------------------------------------------
// Default flood scope  (CMD_GET_DEFAULT_FLOOD_SCOPE 0x40 /
// CMD_SET_DEFAULT_FLOOD_SCOPE 0x3F)
// ---------------------------------------------------------------------------

/// MeshCore 1.15 default flood scope: a named region with a 16-byte transport
/// key. Persisted as a 47-byte blob `[name:31][key:16]`. `name[0] == 0` means
/// cleared.
#[derive(Clone, Copy, Debug)]
pub struct DefaultFloodScope {
    /// 31-byte NUL-padded ASCII name.
    pub name: [u8; 31],
    /// 16-byte transport key.
    pub key: [u8; 16],
}

const DEFAULT_SCOPE_BLOB_LEN: usize = 31 + 16;

pub async fn get_default_flood_scope() -> Option<DefaultFloodScope> {
    let mut b = [0u8; DEFAULT_SCOPE_BLOB_LEN];
    match ns().get("def_scope", &mut b).await {
        Ok(DEFAULT_SCOPE_BLOB_LEN) if b[0] != 0 => {
            let mut name = [0u8; 31];
            name.copy_from_slice(&b[..31]);
            let mut key = [0u8; 16];
            key.copy_from_slice(&b[31..]);
            Some(DefaultFloodScope { name, key })
        }
        _ => None,
    }
}

pub async fn set_default_flood_scope(value: Option<DefaultFloodScope>) -> Result<(), kv::KvError> {
    let mut bytes = [0u8; DEFAULT_SCOPE_BLOB_LEN];
    if let Some(v) = value {
        bytes[..31].copy_from_slice(&v.name);
        bytes[31..].copy_from_slice(&v.key);
    }
    ns().set("def_scope", &bytes, true).await
}

// ---------------------------------------------------------------------------
// Tuning parameters  (CMD_SET_TUNING_PARAMS 0x15 / CMD_GET_TUNING_PARAMS 0x2B)
// ---------------------------------------------------------------------------

/// Tuning parameters for TX duty-cycle enforcement and relay timing.
#[derive(Clone, Copy, Debug, defmt::Format)]
pub struct TuningParams {
    /// RX relay delay base, encoded as `delay_secs * 1000` (e.g. 1000 = 1.0 s).
    /// Not yet acted on by the firmware; stored for round-trip fidelity.
    pub rx_delay_base_x1000: u32,
    /// Airtime factor encoded as `factor * 1000` (e.g. 9000 = factor 9.0).
    /// Duty cycle = 1 / (1 + factor): factor 9.0 → 10%, factor 0.0 → 100%.
    /// Default: 9000 (10% duty cycle — EU 869 MHz compliant).
    pub airtime_factor_x1000: u32,
}

impl TuningParams {
    fn to_bytes(self) -> [u8; 8] {
        let mut b = [0u8; 8];
        b[0..4].copy_from_slice(&self.rx_delay_base_x1000.to_le_bytes());
        b[4..8].copy_from_slice(&self.airtime_factor_x1000.to_le_bytes());
        b
    }

    fn from_bytes(b: &[u8; 8]) -> Self {
        Self {
            rx_delay_base_x1000: u32::from_le_bytes([b[0], b[1], b[2], b[3]]),
            airtime_factor_x1000: u32::from_le_bytes([b[4], b[5], b[6], b[7]]),
        }
    }
}

/// Default tuning parameters.
///
/// `rx_delay_base = 0` (no relay delay), `airtime_factor = 9.0` (10% duty
/// cycle).
pub const DEFAULT_TUNING: TuningParams = TuningParams {
    rx_delay_base_x1000: 0,
    airtime_factor_x1000: 9_000,
};

/// Read the persisted tuning params, or return [`DEFAULT_TUNING`] if not
/// stored.
pub async fn get_tuning_params() -> TuningParams {
    let mut b = [0u8; 8];
    match ns().get("tuning", &mut b).await {
        Ok(8) => TuningParams::from_bytes(&b),
        _ => DEFAULT_TUNING,
    }
}

/// Persist tuning params to flash.
pub async fn set_tuning_params(p: TuningParams) -> Result<(), kv::KvError> {
    ns().set("tuning", &p.to_bytes(), true).await
}

// ---------------------------------------------------------------------------
// Path hash mode  (CMD_SET_PATH_HASH_MODE 0x3D)
// ---------------------------------------------------------------------------

/// Read the path hash mode (0 = 1-byte hashes, 1/2 = extended).  Returns 0 if
/// not stored.
pub async fn get_path_hash_mode() -> u8 {
    let mut b = [0u8; 1];
    match ns().get("path_hash", &mut b).await {
        Ok(1) => b[0],
        _ => 0,
    }
}

/// Persist the path hash mode to flash.
pub async fn set_path_hash_mode(mode: u8) -> Result<(), kv::KvError> {
    ns().set("path_hash", &[mode], true).await
}

// ---------------------------------------------------------------------------
// EPD LUT cycle-duration scale (per-refresh patching of SSD1675/B OTP LUT)
// ---------------------------------------------------------------------------

/// Read the persisted EPD LUT cycle-duration scale override, or `None` if
/// the user has never adjusted it (caller should fall back to the
/// per-variant default in the ssd1675 driver).
pub async fn get_epd_lut_speed() -> Option<u8> {
    let mut b = [0u8; 1];
    match ns().get("epd_lut", &mut b).await {
        Ok(1) => Some(b[0]),
        _ => None,
    }
}

/// Persist the EPD LUT cycle-duration scale (`0..=200`, `100` = OEM speed,
/// `0` = no delay).
pub async fn set_epd_lut_speed(scale: u8) -> Result<(), kv::KvError> {
    ns().set("epd_lut", &[scale], true).await
}

/// Read the persisted EPD temperature-bias override (°C × 10, range
/// ±50), or `None` if the user has never adjusted it.
pub async fn get_epd_temp_bias_c10() -> Option<i8> {
    let mut b = [0u8; 1];
    match ns().get("epd_tb", &mut b).await {
        Ok(1) => Some(b[0] as i8),
        _ => None,
    }
}

/// Persist the EPD temperature-bias override (°C × 10).
pub async fn set_epd_temp_bias_c10(bias: i8) -> Result<(), kv::KvError> {
    ns().set("epd_tb", &[bias as u8], true).await
}
