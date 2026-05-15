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

use embassy_futures::join::{join, join4};

use super::{contacts, settings};
use crate::fw::kv;

/// Run the settings persister.  Spawn this once at boot; it loops
/// forever waiting on each menu signal and persisting changes.
///
/// Each individual signal has its own concurrent loop so they're
/// processed independently — a long flash write for one setting
/// can't delay another.
#[embassy_executor::task]
pub async fn run() -> ! {
    join4(
        join4(
            tz_loop(),
            boost_rx_loop(),
            lora_radio_loop(),
            other_params_loop(),
        ),
        join4(
            advert_loop(),
            path_hash_loop(),
            node_name_loop(),
            lora_disabled_loop(),
        ),
        join4(
            ble_disabled_loop(),
            factory_reset_loop(),
            contact_reset_loop(),
            clear_bonds_loop(),
        ),
        join(crate::fw::epd::epd_lut_speed_persist_loop(), core::future::pending::<()>()),
    )
    .await;
    // Unreachable — every inner loop is `loop {}`.  The signature
    // returns `!` so callers can spawn this without unwrapping.
    loop {
        core::future::pending::<()>().await;
    }
}

async fn tz_loop() -> ! {
    loop {
        crate::TZ_CHANGED_SIGNAL.wait().await;
        let offset = crate::TIMEZONE_OFFSET.load(core::sync::atomic::Ordering::Relaxed);
        match settings::set_timezone(offset).await {
            Ok(()) => defmt::debug!("settings: timezone={} persisted", offset),
            Err(e) => defmt::warn!("settings: timezone persist failed: {:?}", e),
        }
    }
}

async fn boost_rx_loop() -> ! {
    loop {
        crate::BOOST_RX_CHANGED_SIGNAL.wait().await;
        let enabled = crate::BOOSTED_RX_GAIN.load(core::sync::atomic::Ordering::Relaxed);
        match settings::set_boost_rx(enabled).await {
            Ok(()) => defmt::debug!("settings: boost_rx={} persisted", enabled),
            Err(e) => defmt::warn!("settings: boost_rx persist failed: {:?}", e),
        }
    }
}

async fn lora_radio_loop() -> ! {
    loop {
        crate::LORA_RADIO_CHANGED_SIGNAL.wait().await;
        use core::sync::atomic::Ordering::Relaxed;
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
}

async fn other_params_loop() -> ! {
    loop {
        crate::OTHER_PARAMS_CHANGED_SIGNAL.wait().await;
        use core::sync::atomic::Ordering::Relaxed;
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
}

async fn advert_loop() -> ! {
    loop {
        crate::ADVERT_CHANGED_SIGNAL.wait().await;
        use core::sync::atomic::Ordering::Relaxed;
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
}

async fn path_hash_loop() -> ! {
    loop {
        crate::PATH_HASH_CHANGED_SIGNAL.wait().await;
        let mode = crate::fw::mesh::PATH_HASH_MODE.load(core::sync::atomic::Ordering::Relaxed);
        match settings::set_path_hash_mode(mode).await {
            Ok(()) => defmt::debug!("settings: path_hash_mode={=u8} persisted", mode),
            Err(e) => defmt::warn!("settings: path_hash persist failed: {:?}", e),
        }
    }
}

async fn node_name_loop() -> ! {
    loop {
        crate::NODE_NAME_CHANGED_SIGNAL.wait().await;
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
}

async fn lora_disabled_loop() -> ! {
    loop {
        crate::LORA_DISABLED_CHANGED.wait().await;
        let enabled = !crate::LORA_DISABLED.load(core::sync::atomic::Ordering::Relaxed);
        match settings::set_lora_enabled(enabled).await {
            Ok(()) => defmt::debug!("settings: lora_enabled={=bool} persisted", enabled),
            Err(e) => defmt::warn!("settings: lora_enabled persist failed: {:?}", e),
        }
    }
}

async fn ble_disabled_loop() -> ! {
    loop {
        crate::BLE_DISABLED_CHANGED.wait().await;
        let enabled = !crate::BLE_DISABLED.load(core::sync::atomic::Ordering::Relaxed);
        match settings::set_ble_enabled(enabled).await {
            Ok(()) => defmt::debug!("settings: ble_enabled={=bool} persisted", enabled),
            Err(e) => defmt::warn!("settings: ble_enabled persist failed: {:?}", e),
        }
    }
}

async fn factory_reset_loop() -> ! {
    // Single-shot: `wipe_and_reset` reboots the MCU and never returns,
    // so the surrounding `loop {}` (clippy: never_loop) only ever ran
    // once.  Keeping the function name + signal-wait shape unchanged so
    // the spawn site in `bin/embassy.rs` doesn't move.
    crate::FACTORY_RESET_SIGNAL.wait().await;
    defmt::info!("settings: factory reset requested — wiping KV and rebooting");
    kv::wipe_and_reset().await
}

async fn contact_reset_loop() -> ! {
    loop {
        crate::CONTACT_RESET_SIGNAL.wait().await;
        defmt::info!("settings: clearing all contacts");
        contacts::ContactStore::new().clear_all().await;
    }
}

async fn clear_bonds_loop() -> ! {
    loop {
        crate::CLEAR_BONDS_SIGNAL.wait().await;
        let _ = super::bonds::BOND_CMD_CHANNEL.try_send(super::bonds::BondCmd::ClearAll);
        // bond_task will wipe the store and reboot — just wait.
        embassy_time::Timer::after_secs(5).await;
    }
}

