//! Settings persistence task — saves menu changes to flash *immediately*.
//!
//! When the user adjusts a setting in the menu (timezone, boost-RX,
//! LoRa params, etc.) the menu task updates an in-RAM atomic and
//! fires the matching `*_CHANGED_SIGNAL`.  This task waits on those
//! signals and writes the new value to ekv as soon as it sees one,
//! so a power loss or reset right after the menu interaction doesn't
//! lose the change.
//!
//! Previously this same persistence logic lived inside
//! `nus_peripheral_loop`'s outer loop, where it only ran between BLE
//! connections — which meant a TZ change made while no phone had
//! connected (or while a phone stayed connected) wasn't persisted.
//!
//! The dispatch is deliberately **serial**: we `select` across all the
//! menu signal-waits (each a tiny future) and then perform exactly one
//! flash write at a time.  An earlier version `join`ed a dozen
//! independent `loop {}` handlers, which forced the task's future to
//! reserve the async state of *every* handler's write concurrently
//! (~15 KiB of `.bss`).  Settings are user-driven and change one at a
//! time, so serial dispatch costs nothing in practice and reclaims that
//! RAM. The two EPD persist loops stay joined alongside (different
//! module, small).

use embassy_futures::join::join;
use embassy_futures::select::{Either, select, select4};

use super::{contacts, settings};
use crate::fw::kv;

/// Which menu setting fired — the shared result type of every signal-wait
/// so they can be `select`ed together and dispatched from one `match`.
#[derive(Clone, Copy)]
enum Which {
    Tz,
    BoostRx,
    LoraRadio,
    OtherParams,
    Advert,
    PathHash,
    NodeName,
    LoraDisabled,
    BleDisabled,
    FactoryReset,
    ContactReset,
    ClearBonds,
}

/// Run the settings persister.  Spawn this once at boot; it loops forever,
/// waiting on any menu signal and persisting that change before waiting again.
#[embassy_executor::task]
pub async fn run() -> ! {
    join(
        settings_dispatch_loop(),
        join(
            crate::fw::epd::epd_lut_speed_persist_loop(),
            crate::fw::epd::epd_temp_bias_persist_loop(),
        ),
    )
    .await;
    // Unreachable — every inner loop is `loop {}`.  The signature
    // returns `!` so callers can spawn this without unwrapping.
    loop {
        core::future::pending::<()>().await;
    }
}

/// Wait for any menu signal (cheap — just the signal futures), then perform
/// the single matching flash write. One write is ever in flight at a time.
async fn settings_dispatch_loop() -> ! {
    loop {
        // Only the tiny signal-wait futures coexist here; the write below runs
        // afterwards, so the task never holds two write states at once.
        let group_a = select4(w_tz(), w_boost_rx(), w_lora_radio(), w_other_params());
        let group_b = select4(w_advert(), w_path_hash(), w_node_name(), w_lora_disabled());
        let group_c = select4(w_ble_disabled(), w_factory_reset(), w_contact_reset(), w_clear_bonds());
        let which = match select(select(group_a, group_b), group_c).await {
            Either::First(Either::First(e)) => flatten(e),
            Either::First(Either::Second(e)) => flatten(e),
            Either::Second(e) => flatten(e),
        };
        dispatch(which).await;
    }
}

/// Collapse a `select4` result (every arm carries a `Which`) to the `Which`.
fn flatten(e: embassy_futures::select::Either4<Which, Which, Which, Which>) -> Which {
    use embassy_futures::select::Either4;
    match e {
        Either4::First(w) | Either4::Second(w) | Either4::Third(w) | Either4::Fourth(w) => w,
    }
}

// ── Signal waits — each resolves to its `Which` tag ────────────────────────

async fn w_tz() -> Which {
    crate::TZ_CHANGED_SIGNAL.wait().await;
    Which::Tz
}
async fn w_boost_rx() -> Which {
    crate::BOOST_RX_CHANGED_SIGNAL.wait().await;
    Which::BoostRx
}
async fn w_lora_radio() -> Which {
    crate::LORA_RADIO_CHANGED_SIGNAL.wait().await;
    Which::LoraRadio
}
async fn w_other_params() -> Which {
    crate::OTHER_PARAMS_CHANGED_SIGNAL.wait().await;
    Which::OtherParams
}
async fn w_advert() -> Which {
    crate::ADVERT_CHANGED_SIGNAL.wait().await;
    Which::Advert
}
async fn w_path_hash() -> Which {
    crate::PATH_HASH_CHANGED_SIGNAL.wait().await;
    Which::PathHash
}
async fn w_node_name() -> Which {
    crate::NODE_NAME_CHANGED_SIGNAL.wait().await;
    Which::NodeName
}
async fn w_lora_disabled() -> Which {
    crate::LORA_DISABLED_CHANGED.wait().await;
    Which::LoraDisabled
}
async fn w_ble_disabled() -> Which {
    crate::BLE_DISABLED_CHANGED.wait().await;
    Which::BleDisabled
}
async fn w_factory_reset() -> Which {
    crate::FACTORY_RESET_SIGNAL.wait().await;
    Which::FactoryReset
}
async fn w_contact_reset() -> Which {
    crate::CONTACT_RESET_SIGNAL.wait().await;
    Which::ContactReset
}
async fn w_clear_bonds() -> Which {
    crate::CLEAR_BONDS_SIGNAL.wait().await;
    Which::ClearBonds
}

// ── Writes — one at a time, dispatched after a signal fires ────────────────

async fn dispatch(which: Which) {
    use core::sync::atomic::Ordering::Relaxed;
    match which {
        Which::Tz => {
            let offset = crate::TIMEZONE_OFFSET.load(Relaxed);
            match settings::set_timezone(offset).await {
                Ok(()) => defmt::debug!("settings: timezone={} persisted", offset),
                Err(e) => defmt::warn!("settings: timezone persist failed: {:?}", e),
            }
        }
        Which::BoostRx => {
            let enabled = crate::BOOSTED_RX_GAIN.load(Relaxed);
            match settings::set_boost_rx(enabled).await {
                Ok(()) => defmt::debug!("settings: boost_rx={} persisted", enabled),
                Err(e) => defmt::warn!("settings: boost_rx persist failed: {:?}", e),
            }
        }
        Which::LoraRadio => {
            let mut rp = settings::get_radio_params_or_default().await;
            rp.freq_hz = crate::LORA_FREQ_HZ.load(Relaxed);
            rp.bw_hz = crate::LORA_BW_HZ.load(Relaxed);
            rp.sf = crate::LORA_SF.load(Relaxed);
            rp.cr = crate::LORA_CR.load(Relaxed);
            rp.tx_power = crate::LORA_TX_POWER.load(Relaxed);
            rp.client_repeat = crate::LORA_CLIENT_REPEAT.load(Relaxed);
            match settings::set_radio_params(rp).await {
                Ok(()) => defmt::debug!("settings: radio params persisted"),
                Err(e) => defmt::warn!("settings: radio params persist failed: {:?}", e),
            }
            // Fan out to the listener so the SX1262 is reprogrammed live.
            // Done after the flash write so a power-loss between signal and
            // apply still leaves the persisted params authoritative.
            crate::LORA_RADIO_APPLY_SIGNAL.signal(());
        }
        Which::OtherParams => {
            let mut op = settings::get_other_params()
                .await
                .unwrap_or(settings::OtherParams {
                    manual_add_contacts: 0,
                    telemetry_mode_base: 0,
                    advert_loc_policy: 0,
                    multi_acks: 0,
                });
            op.advert_loc_policy = crate::ADVERT_LOC_POLICY.load(Relaxed) as u8;
            op.multi_acks = crate::MULTI_ACKS.load(Relaxed);
            op.telemetry_mode_base = crate::TELEMETRY_MODE_BASE.load(Relaxed);
            match settings::set_other_params(op).await {
                Ok(()) => defmt::debug!("settings: other_params persisted"),
                Err(e) => defmt::warn!("settings: other_params persist failed: {:?}", e),
            }
            let ignore = crate::IGNORE_BLINK.load(Relaxed);
            match settings::set_ignore_blink(ignore).await {
                Ok(()) => defmt::debug!("settings: ignore_blink={=bool} persisted", ignore),
                Err(e) => defmt::warn!("settings: ignore_blink persist failed: {:?}", e),
            }
        }
        Which::Advert => {
            let cfg = settings::AdvertConfig {
                enabled: crate::ADVERT_ENABLED.load(Relaxed),
                interval_hours: crate::ADVERT_INTERVAL_HOURS.load(Relaxed),
            };
            match settings::set_advert_config(cfg).await {
                Ok(()) => defmt::debug!(
                    "settings: advert_config persisted (enabled={=bool} interval={=u8}h)",
                    cfg.enabled,
                    cfg.interval_hours
                ),
                Err(e) => defmt::warn!("settings: advert_config persist failed: {:?}", e),
            }
        }
        Which::PathHash => {
            let mode = crate::fw::mesh::PATH_HASH_MODE.load(Relaxed);
            match settings::set_path_hash_mode(mode).await {
                Ok(()) => defmt::debug!("settings: path_hash_mode={=u8} persisted", mode),
                Err(e) => defmt::warn!("settings: path_hash persist failed: {:?}", e),
            }
        }
        Which::NodeName => {
            let (buf, n) = crate::NODE_NAME.lock(|cell| {
                let s = cell.borrow();
                let mut buf = [0u8; 31];
                let n = s.len().min(31);
                buf[..n].copy_from_slice(s.as_bytes().get(..n).unwrap_or(&[]));
                (buf, n)
            });
            match settings::set_node_name(&buf[..n]).await {
                Ok(()) => defmt::debug!("settings: node_name persisted"),
                Err(e) => defmt::warn!("settings: node_name persist failed: {:?}", e),
            }
        }
        Which::LoraDisabled => {
            let enabled = !crate::LORA_DISABLED.load(Relaxed);
            match settings::set_lora_enabled(enabled).await {
                Ok(()) => defmt::debug!("settings: lora_enabled={=bool} persisted", enabled),
                Err(e) => defmt::warn!("settings: lora_enabled persist failed: {:?}", e),
            }
        }
        Which::BleDisabled => {
            let enabled = !crate::BLE_DISABLED.load(Relaxed);
            match settings::set_ble_enabled(enabled).await {
                Ok(()) => defmt::debug!("settings: ble_enabled={=bool} persisted", enabled),
                Err(e) => defmt::warn!("settings: ble_enabled persist failed: {:?}", e),
            }
        }
        Which::FactoryReset => {
            defmt::info!("settings: factory reset requested — wiping KV and rebooting");
            kv::wipe_and_reset().await // reboots; never returns
        }
        Which::ContactReset => {
            defmt::info!("settings: clearing all contacts");
            contacts::ContactStore::new().clear_all().await;
        }
        Which::ClearBonds => {
            let _ = super::bonds::BOND_CMD_CHANNEL.try_send(super::bonds::BondCmd::ClearAll);
            // bond_task will wipe the store and reboot — just wait.
            embassy_time::Timer::after_secs(5).await;
        }
    }
}
