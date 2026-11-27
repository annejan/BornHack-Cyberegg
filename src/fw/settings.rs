//! Device-wide settings in the shared `"settings"` KV namespace.
//!
//! These are available in every firmware build (mesh or not): the namespace
//! accessor [`ns`], one-byte typed helpers ([`get_u8`] / [`set_u8`] /
//! [`get_flag`] / [`set_flag`]), and the settings that belong to the device
//! rather than to MeshCore: timezone and the EPD tuning overrides.
//! `fw::mesh::settings` re-exports these so mesh callers are unchanged.
//!
//! Namespace and key strings are persisted on existing badges — do not rename
//! them: `"settings"`, `"tzq"`, `"tz"`, `"epd_lut"`, `"epd_tb"`.

use crate::fw::kv;

/// The shared `"settings"` KV namespace.
pub fn ns() -> kv::KvNamespace {
    kv::namespace("settings")
}

/// Read a one-byte value, or `None` if the key is absent or not exactly 1 B.
pub async fn get_u8(key: &str) -> Option<u8> {
    let mut b = [0u8; 1];
    match ns().get(key, &mut b).await {
        Ok(1) => Some(b[0]),
        _ => None,
    }
}

/// Persist a one-byte value.
pub async fn set_u8(key: &str, v: u8) -> Result<(), kv::KvError> {
    ns().set(key, &[v], true).await
}

/// Read a boolean stored as one byte (`!= 0`), or `default` if absent.
pub async fn get_flag(key: &str, default: bool) -> bool {
    get_u8(key).await.map_or(default, |v| v != 0)
}

/// Persist a boolean as one byte.
pub async fn set_flag(key: &str, v: bool) -> Result<(), kv::KvError> {
    set_u8(key, v as u8).await
}

// ---------------------------------------------------------------------------
// Timezone offset  (menu stepper, persisted locally)
// ---------------------------------------------------------------------------

/// Read the persisted UTC offset in quarter hours (see `TZ_OFFSET_QH`).
/// Returns +2 h (8 qh, CEST / Europe/Copenhagen summer) when no value has been
/// stored yet — most badges ship into a Bornhack-style summer venue, so this
/// saves the user from immediately having to flip Settings → Timezone.
///
/// Firmware before quarter-hour support only wrote whole hours under `"tz"`;
/// that value is used, times four, until `"tzq"` exists.
pub async fn get_timezone() -> i8 {
    if let Some(b) = get_u8("tzq").await {
        return (b as i8).clamp(crate::TZ_OFFSET_QH_MIN, crate::TZ_OFFSET_QH_MAX);
    }
    match get_u8("tz").await {
        Some(b) => (b as i8).clamp(-12, 14) * 4,
        None => 8,
    }
}

/// Persist the UTC offset (quarter hours) to flash. The whole-hour part also
/// goes to the old `"tz"` key so older firmware still reads a sane offset.
pub async fn set_timezone(offset_qh: i8) -> Result<(), kv::KvError> {
    set_u8("tzq", offset_qh as u8).await?;
    set_u8("tz", (offset_qh / 4) as u8).await
}

// ---------------------------------------------------------------------------
// EPD tuning overrides
// ---------------------------------------------------------------------------

/// Read the persisted EPD LUT cycle-duration scale override, or `None` if
/// the user has never adjusted it (caller should fall back to the
/// per-variant default in the ssd1675 driver).
pub async fn get_epd_lut_speed() -> Option<u8> {
    get_u8("epd_lut").await
}

/// Persist the EPD LUT cycle-duration scale (`0..=200`, `100` = OEM speed,
/// `0` = no delay).
pub async fn set_epd_lut_speed(scale: u8) -> Result<(), kv::KvError> {
    set_u8("epd_lut", scale).await
}

/// Read the persisted EPD temperature-bias override (°C × 10, range
/// ±50), or `None` if the user has never adjusted it.
pub async fn get_epd_temp_bias_c10() -> Option<i8> {
    get_u8("epd_tb").await.map(|b| b as i8)
}

/// Persist the EPD temperature-bias override (°C × 10).
pub async fn set_epd_temp_bias_c10(bias: i8) -> Result<(), kv::KvError> {
    set_u8("epd_tb", bias as u8).await
}

// ---------------------------------------------------------------------------
// Persister for non-mesh builds
// ---------------------------------------------------------------------------

/// Saves timezone and EPD tuning changes made from the menu. Mesh builds do
/// this from `fw::mesh::persister::run` (which also owns the other mesh
/// settings); builds without `mesh` spawn this instead.
#[cfg(not(feature = "mesh"))]
#[embassy_executor::task]
pub async fn persister_task() -> ! {
    let tz = async {
        loop {
            crate::TZ_CHANGED_SIGNAL.wait().await;
            let offset = crate::TZ_OFFSET_QH.load(core::sync::atomic::Ordering::Relaxed);
            match set_timezone(offset).await {
                Ok(()) => defmt::debug!("settings: timezone={} qh persisted", offset),
                Err(e) => defmt::warn!("settings: timezone persist failed: {:?}", e),
            }
        }
    };
    // EPD LUT-speed / temperature-bias tuning is SSD1675-only.
    #[cfg(feature = "ssd1675-driver")]
    let epd = embassy_futures::join::join(
        crate::fw::epd::epd_lut_speed_persist_loop(),
        crate::fw::epd::epd_temp_bias_persist_loop(),
    );
    #[cfg(not(feature = "ssd1675-driver"))]
    let epd = core::future::pending::<()>();
    let _: ((), _) = embassy_futures::join::join(tz, epd).await;
    loop {
        core::future::pending::<()>().await;
    }
}
