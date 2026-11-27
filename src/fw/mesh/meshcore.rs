use core::cell::RefCell;

use embassy_nrf::gpio::AnyPin;
use embassy_nrf::{Peri, peripherals};
use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_time::Timer;
use meshcore::channel::hash_from_key;
use meshcore::contacts::Contacts;
use meshcore::dedup::{MsgHashRing, msg_hash, relay_hash};

use super::device_identity::DeviceIdentity;
use super::sx1262::{MeshCoreConfig, SimpleLoRa};
use super::{channels, contacts, msg_queue, settings};
use crate::fw::health::SYSTEM_HEALTH;
use crate::{health_err, update_health};

// ---------------------------------------------------------------------------
// Loaded channel table
// ---------------------------------------------------------------------------

struct LoadedChannel {
    slot_idx: u8,
    name: [u8; 32],
    key: [u8; 16],
    hash: u8,
    /// Region override (see `channels::get_region`); empty = global scope.
    region: heapless::String<{ channels::REGION_NAME_MAX }>,
}

impl LoadedChannel {
    /// Effective flood-scope key: the channel's region override, else the
    /// global `DEFAULT_SCOPE_KEY` (def_scope / dk-bornhack). `None` = unscoped.
    fn scope_key(&self) -> Option<[u8; 16]> {
        if self.region.is_empty() {
            crate::DEFAULT_SCOPE_KEY.lock(|c| c.get())
        } else {
            Some(meshcore::packet::region_key(&self.region))
        }
    }

    /// Region filter for received channel messages. With the filter on, a
    /// message is shown only when it was flooded with this channel's
    /// effective region (`scope_code` = the non-zero TransportFlood code, or
    /// `None` for unscoped/direct packets). Filter off, no region configured,
    /// or the internal SHDW game channel (zero-hop beacons) → always shown.
    fn region_ok(&self, scope_code: Option<u16>, payload_type: u8, payload: &[u8]) -> bool {
        if !crate::REGION_FILTER.load(core::sync::atomic::Ordering::Relaxed)
            || self.slot_idx == channels::SHDW_SLOT
        {
            return true;
        }
        let Some(key) = self.scope_key() else {
            return true;
        };
        let ok = scope_code
            == Some(meshcore::packet::calc_transport_code(
                &key,
                payload_type,
                payload,
            ));
        if !ok {
            defmt::info!(
                "channel slot {=u8}: region mismatch (tc={=u16:#06x}) — hidden",
                self.slot_idx,
                scope_code.unwrap_or(0),
            );
        }
        ok
    }
}

async fn load_channels() -> heapless::Vec<LoadedChannel, { channels::NUM_CHANNELS }> {
    let mut v = heapless::Vec::new();
    for i in 0..channels::NUM_CHANNELS as u8 {
        if let Some((name, key)) = channels::get(i).await {
            let hash = hash_from_key(&key);
            let name_str = core::str::from_utf8(&name)
                .unwrap_or("?")
                .trim_end_matches('\0');
            defmt::debug!(
                "  channel slot {=u8}: hash={=u8} name={=str}",
                i,
                hash,
                name_str
            );
            let _ = v.push(LoadedChannel {
                slot_idx: i,
                name,
                key,
                hash,
                region: channels::get_region(i).await.unwrap_or_default(),
            });
        }
    }
    v
}

pub(super) use crate::truncate_str;

fn update_cached_channels(loaded: &heapless::Vec<LoadedChannel, { channels::NUM_CHANNELS }>) {
    crate::CACHED_CHANNELS.lock(|cell| {
        let mut list = cell.borrow_mut();
        list.clear();
        for ch in loaded {
            // Hide the private "SHDW" game channel from the on-screen channel
            // browser — it carries only BornPets friend beacons / battle
            // results (see `crate::game::friends` / `crate::game::battle`),
            // never human chat, so it has no place in the user-facing list.
            // It stays fully live for TX/RX: crypto uses `loaded_channels`,
            // not this UI mirror. Same "present but not shown" idea as the
            // `#blinkme` easter-egg channel, which is never a stored slot.
            if ch.slot_idx == channels::SHDW_SLOT {
                continue;
            }
            let name_str = core::str::from_utf8(&ch.name)
                .unwrap_or("?")
                .trim_end_matches('\0');
            let mut name = heapless::String::new();
            let _ = name.push_str(truncate_str(name_str, 20));
            let _ = list.push(crate::CachedChannel {
                slot_idx: ch.slot_idx,
                name,
                region: ch.region.clone(),
            });
        }
    });
}

static CONTACTS: Mutex<CriticalSectionRawMutex, RefCell<Contacts<20>>> =
    Mutex::new(RefCell::new(Contacts::new()));

/// Record `hash` in a dedup ring unless it is already there. Returns `true`
/// when it was new. (The ring type lives in the `meshcore` submodule, so this
/// is a free function rather than a `MsgHashRing` method.)
pub(super) fn insert_if_new<const N: usize>(ring: &mut MsgHashRing<N>, hash: u32) -> bool {
    if ring.contains(hash) {
        false
    } else {
        ring.insert(hash);
        true
    }
}

static MSG_SEEN: Mutex<CriticalSectionRawMutex, RefCell<MsgHashRing<50>>> =
    Mutex::new(RefCell::new(MsgHashRing::new()));

/// Dedup ring for `GrpData` — the same "arrived more than once via
/// different mesh paths" problem `MSG_SEEN` solves for `GrpTxt`, just
/// keyed on the raw (pre-decrypt, hop-stable) payload via `relay_hash`
/// instead of `msg_hash` since `GrpData` carries no timestamp to hash.
/// Without this, a single logical beacon or Battle-result broadcast
/// could be processed multiple times (each a fresh friend-boost or
/// win/loss increment) if it reached a receiver by more than one path —
/// the likely cause of mismatched Battle records between badges.
static GRP_DATA_SEEN: Mutex<CriticalSectionRawMutex, RefCell<MsgHashRing<50>>> =
    Mutex::new(RefCell::new(MsgHashRing::new()));

// ---------------------------------------------------------------------------
// Advert mode
// ---------------------------------------------------------------------------

/// How to route a self-advert transmission.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum AdvertMode {
    /// Flood — relayed across the full mesh.
    Flood,
    /// Zero-hop (direct, path_len=0) — reaches only immediate radio neighbors.
    ZeroHop,
}

// ---------------------------------------------------------------------------
// MeshCore listener task
// ---------------------------------------------------------------------------

/// Listen for MeshCore packets on the SX1262 and store decoded messages.
///
/// Loads LoRa radio parameters from [`settings`] (falling back to
/// [`MeshCoreConfig::UK_NARROW_BAND`] on first boot) and enters a continuous
/// receive loop.  Every received packet is parsed with the `meshcore` vendor
/// crate.  Group-text messages (`GrpTxt`) are decoded, deduplicated, and
/// stored in `LAST_LORA_MSG`; node advertisements are logged; all other types
/// are logged as raw hex.
///
/// Radio parameters (frequency, bandwidth, SF, CR, TX power) and the boost-RX
/// toggle, changed from the menu, the companion app or its CLI, are applied
/// live: the persister signals `LORA_RADIO_APPLY_SIGNAL` and the loop below
/// reprograms the SX1262 without a reboot.
pub async fn run_meshcore_listener<'a>(
    spi: Peri<'a, peripherals::SPI2>,
    sck_pin: Peri<'a, AnyPin>,
    mosi_pin: Peri<'a, AnyPin>,
    miso_pin: Peri<'a, AnyPin>,
    nrst_pin: Peri<'a, AnyPin>,
    nss_pin: Peri<'a, AnyPin>,
    busy_pin: Peri<'a, AnyPin>,
    dio1_pin: Peri<'a, AnyPin>,
    ant_pin: Peri<'a, AnyPin>,
    identity: &DeviceIdentity,
) -> ! {
    update_health!(|h| h.lora.set_ok("Ok when started."));

    let radio = settings::get_radio_params_or_default().await;
    let lora_cfg = MeshCoreConfig::from_radio_params(&radio);
    let config = &lora_cfg;

    let mut lora = match SimpleLoRa::new(
        spi, sck_pin, mosi_pin, miso_pin, nrst_pin, nss_pin, busy_pin, dio1_pin, ant_pin, config,
    ) {
        Ok(l) => {
            SYSTEM_HEALTH.lock(|cell| {
                cell.borrow_mut().lora.set_ok("SX1262 init OK");
            });
            l
        }
        Err(e) => {
            health_err!(lora, "LoRa init failed");
            defmt::error!("LoRa init failed: {:?}", e);
            loop {
                Timer::after_millis(60_000).await;
            }
        }
    };

    if !lora.ensure_rx().await {
        defmt::error!("SX1262 failed to enter RX mode after 500ms — check crystal/wiring");
    }

    // Initialise TX duty-cycle budget from persisted tuning params.
    let tuning = settings::get_tuning_params().await;
    lora.init_budget(tuning.airtime_factor_x1000);
    defmt::info!(
        "TX budget: af={=u32}.{=u32:03} ({}%), window=1h",
        tuning.airtime_factor_x1000 / 1000,
        tuning.airtime_factor_x1000 % 1000,
        1000 / (1 + tuning.airtime_factor_x1000 / 1000).max(1),
    );

    defmt::info!(
        "MeshCore listener ready — freq={=u32}Hz bw={=u32}Hz SF={=u8} CR={=u8} sync={=u16:#06x} preamble={=u16}",
        config.frequency_hz,
        radio.bw_hz,
        radio.sf,
        radio.cr,
        config.sync_word,
        config.preamble_len,
    );
    {
        let on = crate::RX_POWER_SAVE.load(core::sync::atomic::Ordering::Relaxed);
        match crate::lora_timing::rx_power_save_periods(radio.sf, radio.bw_hz).filter(|_| on) {
            Some(p) => defmt::info!(
                "MeshCore RX power save on: rx={=u32} sleep={=u32} (x15.625us)",
                p.rx,
                p.sleep
            ),
            None => defmt::info!("MeshCore RX power save {=bool}: continuous RX", on),
        }
    }
    defmt::info!(
        "MeshCore identity pub_key: {=[u8]:02x}",
        &identity.pub_key[..]
    );

    let mut loaded_channels = load_channels().await;
    update_cached_channels(&loaded_channels);
    defmt::info!(
        "MeshCore: loaded {} channel(s) from KV",
        loaded_channels.len()
    );

    // Load the persisted path hash mode into the RAM cache so every flood TX
    // below picks up the user's preference without paying a flash read.
    let path_mode = settings::get_path_hash_mode().await.min(2);
    crate::PATH_HASH_MODE.store(path_mode, core::sync::atomic::Ordering::Relaxed);
    defmt::info!(
        "MeshCore: path_hash_mode={=u8} ({=u8}-byte per-hop hashes)",
        path_mode,
        path_mode + 1,
    );

    let mut raw = [0u8; 255];

    loop {
        // When LoRa is disabled, put the radio into standby and wait.
        if crate::LORA_DISABLED.load(core::sync::atomic::Ordering::Relaxed) {
            defmt::info!("MeshCore: LoRa disabled — entering standby");
            lora.standby();
            loop {
                crate::LORA_DISABLED_CHANGED.wait().await;
                crate::LORA_DISABLED_CHANGED.reset();
                if !crate::LORA_DISABLED.load(core::sync::atomic::Ordering::Relaxed) {
                    defmt::info!("MeshCore: LoRa re-enabled — resuming RX");
                    lora.resume_rx();
                    break;
                }
            }
        }

        // Reset channels and reboot when requested from the menu.
        if crate::CHANNEL_RESET_SIGNAL.signaled() {
            crate::CHANNEL_RESET_SIGNAL.reset();
            channels::reset().await;
            defmt::info!("channels: reset complete — rebooting");
            Timer::after_millis(200).await;
            cortex_m::peripheral::SCB::sys_reset();
        }

        // Persist a region override set from the on-device channel browser,
        // then reload the table below so TX/RX pick it up.
        if let Some((idx, region)) = crate::CHANNEL_REGION_SIGNAL.try_take() {
            channels::set_region(idx, &region).await;
            crate::CHANNELS_CHANGED_SIGNAL.signal(());
        }

        // Reload channel table if the BLE task updated a channel.
        if crate::CHANNELS_CHANGED_SIGNAL.signaled() {
            crate::CHANNELS_CHANGED_SIGNAL.reset();
            loaded_channels = load_channels().await;
            update_cached_channels(&loaded_channels);
            defmt::debug!(
                "MeshCore: reloaded {} channel(s) from KV",
                loaded_channels.len()
            );
        }

        // Update TX duty-cycle budget if tuning params changed.
        if let Some(af_x1000) = crate::TUNING_CHANGED_SIGNAL.try_take() {
            lora.init_budget(af_x1000);
            defmt::debug!(
                "TX budget updated: af={=u32}.{=u32:03}",
                af_x1000 / 1000,
                af_x1000 % 1000
            );
        }

        // Apply radio-param changes (freq, BW, SF, CR, TX power) from the
        // menu or BLE companion without rebooting.  Fired by the
        // persister AFTER the flash write succeeds, so the live radio
        // never disagrees with what's persisted.
        if crate::LORA_RADIO_APPLY_SIGNAL.signaled() {
            crate::LORA_RADIO_APPLY_SIGNAL.reset();
            let rp = crate::RadioParams::load_live();
            let cfg = MeshCoreConfig::from_radio_params(&rp);
            lora.reconfigure_radio(&cfg);
            defmt::info!(
                "MeshCore: radio reprogrammed live — freq={=u32}Hz bw={=u32}Hz SF={=u8} CR={=u8} TX={=i8}dBm",
                rp.freq_hz,
                rp.bw_hz,
                rp.sf,
                rp.cr,
                rp.tx_power,
            );
        }

        // Drain all queued TX requests before entering RX.
        while let Ok(req) = crate::TX_CHANNEL.try_receive() {
            dispatch_tx(&mut lora, &loaded_channels, identity, req).await;
        }

        // Race: receive the next LoRa packet, OR a TX request arrives,
        // OR a radio-param change comes in.  TX_WAKEUP breaks the
        // receive_packet wait so the drain above picks up the new
        // request immediately; LORA_RADIO_APPLY_SIGNAL breaks it so
        // the reprogramming check at the top of the next iter runs
        // without waiting for the next inbound packet.
        use embassy_futures::select::{Either3, select3};
        let rx_result = match select3(
            lora.receive_packet(&mut raw),
            crate::TX_WAKEUP.wait(),
            crate::LORA_RADIO_APPLY_SIGNAL.wait(),
        )
        .await
        {
            Either3::Second(()) => {
                crate::TX_WAKEUP.reset();
                continue;
            }
            Either3::Third(()) => {
                // Re-arm so the top-of-loop handler sees it; `wait()`
                // consumed the pending state.
                crate::LORA_RADIO_APPLY_SIGNAL.signal(());
                continue;
            }
            Either3::First(result) => result,
        };

        match rx_result {
            Ok(None) => { /* channel idle or CRC error — next CAD cycle on next loop */ }
            Err(e) => {
                defmt::warn!("receive_packet error: {:?}", e);
            }

            Ok(Some((len, rssi, snr_x4))) => {
                if len > 0 {
                    crate::PACKET_STATS.count_recv(raw[0]);
                }
                // Update radio stats snapshot for CMD_GET_STATS / STATS_TYPE_RADIO.
                {
                    let noise_floor = (rssi as i32 - (snr_x4 as i32 / 4)).clamp(-128, 0) as i16;
                    crate::RADIO_STATS.lock(|cell| {
                        cell.set(crate::RadioStats {
                            noise_floor,
                            last_rssi: rssi.clamp(-128, 0) as i8,
                            last_snr_x4: snr_x4,
                            tx_air_secs: lora.tx_air_ms / 1000,
                            rx_air_secs: lora.rx_air_ms / 1000,
                        });
                    });
                }

                // Push raw bytes to the BLE task immediately (before dedup/decrypt)
                // so the client can do its own decryption and relay-repeat tracking.
                {
                    let mut pkt = crate::RawLoRaPkt {
                        snr_x4,
                        rssi: rssi.clamp(-128, 0) as i8,
                        len,
                        data: [0u8; meshcore::MAX_TRANS_UNIT],
                    };
                    pkt.data[..len].copy_from_slice(&raw[..len]);
                    let _ = crate::RAW_PKT_CHANNEL.try_send(pkt);
                }

                let frame = &raw[..len];

                defmt::debug!("RX [{=usize}B {=i16}dBm]: {=[u8]:02x}", len, rssi, frame,);

                match meshcore::packet::deserialize(frame) {
                    Err(_) => {
                        defmt::debug!(
                            "MeshCore [raw {=usize}B {=i16}dBm]: {=[u8]}",
                            len,
                            rssi,
                            frame
                        );
                    }

                    Ok(msg) => {
                        update_health!(|h| h.lora.set_ok("Packet received."));
                        use meshcore::packet::{PayloadType, RouteType};

                        defmt::debug!(
                            "RX: route={=u8} type={=u8} tc={=u16:#06x} payload={=usize}B",
                            msg.route.to_u8(),
                            msg.payload_type.to_u8(),
                            msg.transport_code,
                            msg.payload.len(),
                        );

                        // Region: a non-zero TransportFlood code is the sender's
                        // region scope. Code 0 is unscoped (MeshCore 1.18, #3568):
                        // the reference `calc_transport_code` never yields 0. Every
                        // packet is still handled here; only channel messages are
                        // region-filtered (per channel, see `LoadedChannel::region_ok`).
                        // A code that doesn't match our global flood scope marks the
                        // packet foreign so client-repeat doesn't carry other
                        // regions' floods into ours.
                        let scope_code = (matches!(msg.route, RouteType::TransportFlood)
                            && msg.transport_code != 0)
                            .then_some(msg.transport_code);
                        let foreign_region =
                            match (scope_code, crate::DEFAULT_SCOPE_KEY.lock(|c| c.get())) {
                                (Some(tc), Some(key)) => {
                                    tc != meshcore::packet::calc_transport_code(
                                        &key,
                                        msg.payload_type.to_u8(),
                                        &msg.payload,
                                    )
                                }
                                // Unscoped, or no key set (wildcard).
                                _ => false,
                            };
                        if foreign_region {
                            defmt::debug!(
                                "TransportFlood foreign region (tc={=u16:#06x}) — not relayed",
                                msg.transport_code,
                            );
                        }

                        // Client-repeat relay (if enabled) — own region / unscoped only.
                        if !foreign_region {
                            if let Some(relay_req) = super::repeater::try_relay(&msg, identity) {
                                let _ = crate::tx_send(relay_req);
                            }
                        }

                        // Mirror the original firmware: flood routes carry the wire-encoded
                        // path_len_byte (hash_size_code<<6 | hop_count); direct routes
                        // signal 0xFF (no path built up by relays).
                        let path_len = match msg.route {
                            RouteType::Flood | RouteType::TransportFlood => msg.path_len_byte,
                            _ => 0xFF,
                        };
                        match msg.payload_type {
                            PayloadType::GrpTxt => {
                                push_grp_txt(
                                    &msg.payload,
                                    rssi,
                                    snr_x4,
                                    path_len,
                                    scope_code,
                                    &loaded_channels,
                                )
                                .await
                            }
                            PayloadType::GrpData => {
                                push_grp_data(
                                    &msg.payload,
                                    rssi,
                                    snr_x4,
                                    path_len,
                                    scope_code,
                                    &loaded_channels,
                                )
                                .await
                            }
                            PayloadType::TxtMsg => {
                                log_txt_msg(
                                    &mut lora,
                                    &msg.payload,
                                    rssi,
                                    path_len,
                                    &msg.path,
                                    identity,
                                )
                                .await
                            }
                            PayloadType::Advert => {
                                log_advert(&msg.payload, rssi, path_len, &msg.path).await
                            }
                            PayloadType::Ack => handle_ack_recv(&msg.payload, rssi),
                            PayloadType::Trace => {
                                handle_trace_recv(&msg.payload, &msg.path, snr_x4).await
                            }
                            PayloadType::Response => {
                                handle_response_recv(&msg.payload, identity).await
                            }
                            PayloadType::Req => {
                                handle_req_recv(&mut lora, &msg.payload, identity).await
                            }
                            PayloadType::Path => {
                                handle_path_recv(&msg.payload, rssi, identity).await
                            }
                            PayloadType::Unknown(0x0B) => {
                                // PAYLOAD_TYPE_CONTROL — forward to BLE as PUSH_CODE_CONTROL_DATA
                                // (0x8E).
                                defmt::debug!(
                                    "MeshCore control [{=usize}B {=i16}dBm ctl={=u8:#04x}]: {=[u8]:x}",
                                    len,
                                    rssi,
                                    msg.payload.first().copied().unwrap_or(0),
                                    msg.payload.as_slice(),
                                );
                                let mut payload_vec: heapless::Vec<
                                    u8,
                                    { meshcore::MAX_PAYLOAD_SIZE },
                                > = heapless::Vec::new();
                                let _ = payload_vec.extend_from_slice(&msg.payload);
                                let _ = crate::CONTROL_DATA_PKT_CHANNEL.try_send(
                                    crate::ControlDataPkt {
                                        snr_x4,
                                        rssi: rssi.clamp(-128, 0) as i8,
                                        path_len: msg.path_len_byte,
                                        payload: payload_vec,
                                    },
                                );
                            }
                            other => {
                                defmt::debug!(
                                    "MeshCore type={=u8} [{=usize}B {=i16}dBm]: {=[u8]:x}",
                                    other.to_u8(),
                                    len,
                                    rssi,
                                    frame
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Per-type handlers
// ---------------------------------------------------------------------------

async fn push_grp_txt(
    payload: &[u8],
    rssi: i16,
    _snr_x4: i8,
    path_len: u8,
    scope_code: Option<u16>,
    channels: &[LoadedChannel],
) {
    use meshcore::payload::grp_txt;

    let grp = match grp_txt::deserialize(payload) {
        Ok(g) => g,
        Err(_) => {
            defmt::warn!("GrpTxt: failed to parse payload");
            return;
        }
    };

    // Easter egg: #blinkme channel — blink LED on 'r', 'g', 'b'.
    // Works even when the channel isn't subscribed.
    {
        use meshcore::channel::key_from_hashtag;
        let blink_key = key_from_hashtag("#blinkme");
        let blink_hash = meshcore::channel::hash_from_key(&blink_key);
        if grp.channel_hash == blink_hash
            && !crate::IGNORE_BLINK.load(core::sync::atomic::Ordering::Relaxed)
            && grp_txt::verify_mac(&blink_key, &grp).is_ok()
            && let Ok(dec) = grp_txt::decrypt(&blink_key, &grp)
        {
            let text = core::str::from_utf8(&dec.text).unwrap_or("");
            // The command is the last char after "sender: " prefix.
            let cmd = text
                .rsplit(": ")
                .next()
                .unwrap_or("")
                .trim()
                .as_bytes()
                .first()
                .copied();
            match cmd {
                Some(b'r') | Some(b'R') => {
                    crate::fw::led::set_color(
                        crate::fw::led::Rgb::RED,
                        embassy_time::Duration::from_millis(50),
                    );
                }
                Some(b'g') | Some(b'G') => {
                    crate::fw::led::set_color(
                        crate::fw::led::Rgb::GREEN,
                        embassy_time::Duration::from_millis(50),
                    );
                }
                Some(b'b') | Some(b'B') => {
                    crate::fw::led::set_color(
                        crate::fw::led::Rgb::BLUE,
                        embassy_time::Duration::from_millis(50),
                    );
                }
                _ => {}
            }
            defmt::info!("blinkme: cmd={=u8:#04x}", cmd.unwrap_or(0));
        }
    }

    // The channel id is a single hash byte and can collide: like upstream
    // Mesh::onRecvPacket, try (up to 4) channels with this hash in slot order
    // and take the first whose key verifies the MAC.
    let ch = match meshcore::payload::find_channel(
        channels,
        grp.channel_hash,
        |c| c.hash,
        |c| grp_txt::verify_mac(&c.key, &grp).is_ok(),
    ) {
        Some(c) => c,
        None => {
            defmt::debug!(
                "MeshCore GrpTxt [hash={=u8} {=i16}dBm] no channel verifies (have: {=[u8]})",
                grp.channel_hash,
                rssi,
                &channels
                    .iter()
                    .map(|c| c.hash)
                    .collect::<heapless::Vec<u8, { channels::NUM_CHANNELS }>>()[..],
            );
            return;
        }
    };

    // Region filter — before dedup, so a hidden copy can't suppress a later
    // one flooded with the right region.
    if !ch.region_ok(
        scope_code,
        meshcore::packet::PayloadType::GrpTxt.to_u8(),
        payload,
    ) {
        return;
    }

    match grp_txt::decrypt(&ch.key, &grp) {
        Ok(dec) => {
            let text = core::str::from_utf8(&dec.text).unwrap_or("<invalid utf-8>");

            // Feed the channel-message timestamp into the wall-clock
            // seeder.  MAC verification just above proves the sender
            // had the channel key.  `path_len` already encodes the
            // hop count in its low 6 bits (0 for direct).
            super::repeater_time::observe_timestamp(dec.timestamp, path_len & 0x3F);

            let content_hash = msg_hash(grp.channel_hash, text.as_bytes(), dec.timestamp);
            let is_new =
                MSG_SEEN.lock(|cell| insert_if_new(&mut cell.borrow_mut(), content_hash));
            if !is_new {
                // Check if this is our own message echoed back by a repeater.
                crate::CHANNEL_MSG_RING.lock(|cell| {
                    if let Some(entry) = cell.borrow_mut().find_by_hash_mut(content_hash)
                        && entry.is_own
                        && entry.repeat_count < 9
                    {
                        entry.repeat_count += 1;
                        crate::LORA_MSG_SIGNAL.signal(());
                    }
                });
                defmt::debug!(
                    "GrpTxt: duplicate suppressed (hash={=u32:#010x})",
                    content_hash
                );
                return;
            }

            // Channel name as an owned string for logging and display.
            let ch_name_str = core::str::from_utf8(&ch.name)
                .unwrap_or("")
                .trim_end_matches('\0');
            let mut ch_name: heapless::String<32> = heapless::String::new();
            let _ = ch_name.push_str(ch_name_str);

            defmt::info!(
                "MeshCore GrpTxt [{=str} ts={=u32} {=i16}dBm path={=u8}]: {=str}",
                ch_name.as_str(),
                dec.timestamp,
                rssi,
                path_len,
                text,
            );

            // Update the on-screen LoRa message display.
            let (sender_str, msg_str) = match text.find(": ") {
                Some(i) => (&text[..i], &text[i + 2..]),
                None => ("", text),
            };

            crate::LORA_MSG_SIGNAL.signal(());

            // Add to the channel message ring for the on-device browser.
            {
                let mut s: heapless::String<16> = heapless::String::new();
                let _ = s.push_str(truncate_str(sender_str, 16));
                let mut t: heapless::String<{ crate::CHANNEL_MSG_TEXT_MAX }> =
                    heapless::String::new();
                let _ = t.push_str(truncate_str(msg_str, crate::CHANNEL_MSG_TEXT_MAX));
                crate::CHANNEL_MSG_RING.lock(|cell| {
                    cell.borrow_mut().push(crate::ChannelMsgEntry {
                        channel_idx: ch.slot_idx,
                        sender: s,
                        text: t,
                        timestamp: dec.timestamp,
                        content_hash,
                        is_own: false,
                        repeat_count: 0,
                    });
                });
                super::sounds::play(super::sounds::SoundEvent::ChannelMsg);
            }

            // Push to the flash queue and notify any connected BLE companion.
            let mut queued_text: heapless::Vec<u8, { msg_queue::MAX_TEXT }> = heapless::Vec::new();
            let _ =
                queued_text.extend_from_slice(&dec.text[..dec.text.len().min(msg_queue::MAX_TEXT)]);
            msg_queue::push(&msg_queue::ReceivedMsg {
                kind: msg_queue::MsgKind::Channel,
                sender_prefix: [0u8; 6],
                channel_idx: ch.slot_idx,
                path_len,
                text_type: dec.text_type,
                timestamp: dec.timestamp,
                rssi,
                text: queued_text,
                data_type: 0,
                snr_x4: 0,
            })
            .await;
            defmt::debug!("msg_queue: {} message(s) waiting", msg_queue::count());
            crate::MESSAGES_WAITING_SIGNAL.signal(());
        }
        Err(_) => {
            defmt::warn!(
                "GrpTxt: decryption failed on channel slot {=u8}",
                ch.slot_idx
            );
        }
    }
}

/// Receive handler for `PAYLOAD_TYPE_GRP_DATA` (MeshCore 1.15).
///
/// Looks up the channel by hash, verifies MAC, decrypts, and queues the
/// binary blob as a `MsgKind::ChannelData` record so the companion app can
/// retrieve it via `CMD_SYNC_NEXT_MESSAGE` (delivered as
/// `RESP_CODE_CHANNEL_DATA_RECV` 0x1B).
async fn push_grp_data(
    payload: &[u8],
    rssi: i16,
    snr_x4: i8,
    path_len: u8,
    scope_code: Option<u16>,
    channels: &[LoadedChannel],
) {
    use meshcore::packet::PayloadType;
    use meshcore::payload::grp_data;

    let grp = match grp_data::deserialize(payload) {
        Ok(g) => g,
        Err(_) => {
            defmt::warn!("GrpData: failed to parse payload");
            return;
        }
    };

    // Colliding channel hashes: first of up to 4 matches whose MAC verifies
    // (upstream Mesh::onRecvPacket / searchChannelsByHash).
    let Some(ch) = meshcore::payload::find_channel(
        channels,
        grp.channel_hash,
        |c| c.hash,
        |c| grp_data::verify_mac(&c.key, &grp).is_ok(),
    ) else {
        defmt::debug!(
            "GrpData: no channel verifies hash={=u8:#04x}",
            grp.channel_hash
        );
        return;
    };

    // Region filter — before dedup, so a hidden copy can't suppress a later
    // one flooded with the right region.
    if !ch.region_ok(scope_code, PayloadType::GrpData.to_u8(), payload) {
        return;
    }

    // Suppress exact duplicates of the same broadcast arriving via more
    // than one mesh path (direct + one or more repeater hops) — see
    // `GRP_DATA_SEEN`'s doc comment. Must happen before any of the
    // stateful handlers below (beacon boost, Battle result) run.
    let dh = relay_hash(PayloadType::GrpData.to_u8(), payload);
    let is_new = GRP_DATA_SEEN.lock(|cell| insert_if_new(&mut cell.borrow_mut(), dh));
    if !is_new {
        defmt::debug!("GrpData: duplicate suppressed (hash={=u32:#010x})", dh);
        return;
    }

    let dec = match grp_data::decrypt(&ch.key, &grp) {
        Ok(d) => d,
        Err(_) => {
            defmt::warn!(
                "GrpData: decryption failed on channel slot {=u8}",
                ch.slot_idx
            );
            return;
        }
    };

    defmt::info!(
        "MeshCore GrpData [ch={=u8} type={=u16:#06x} {=usize}B {=i16}dBm path={=u8}]",
        ch.slot_idx,
        dec.data_type,
        dec.data.len(),
        rssi,
        path_len,
    );

    // BornPets friend-discovery beacon on the private SHDW channel — handed
    // straight to the game layer, not queued for the companion app (it's an
    // internal pet-presence ping, not something a human needs to see).
    #[cfg(feature = "game")]
    if ch.slot_idx == channels::SHDW_SLOT && dec.data_type == crate::game::friends::PET_BEACON_TYPE
    {
        crate::game::friends::on_pet_beacon(&dec.data).await;
        return;
    }

    // Mesh Battle packets (challenge, accept, decline, result): same
    // private/SHDW-only treatment as the beacon above, handed to the game.
    #[cfg(feature = "game")]
    if ch.slot_idx == channels::SHDW_SLOT {
        use crate::game::battle::{
            PET_ACCEPT_TYPE, PET_BATTLE_TYPE, PET_CHALLENGE_TYPE, PET_DECLINE_TYPE,
        };
        match dec.data_type {
            PET_CHALLENGE_TYPE => {
                crate::game::battle_link::on_challenge(&dec.data).await;
                return;
            }
            PET_ACCEPT_TYPE => {
                crate::game::battle_link::on_accept(&dec.data).await;
                return;
            }
            PET_DECLINE_TYPE => {
                crate::game::battle_link::on_decline(&dec.data).await;
                return;
            }
            PET_BATTLE_TYPE => {
                crate::game::battle_link::on_result(&dec.data).await;
                return;
            }
            _ => {}
        }
    }

    // Queue the binary blob for the companion app.
    let mut blob: heapless::Vec<u8, { msg_queue::MAX_TEXT }> = heapless::Vec::new();
    let _ = blob.extend_from_slice(&dec.data[..dec.data.len().min(msg_queue::MAX_TEXT)]);
    msg_queue::push(&msg_queue::ReceivedMsg {
        kind: msg_queue::MsgKind::ChannelData,
        sender_prefix: [0u8; 6],
        channel_idx: ch.slot_idx,
        path_len,
        text_type: 0,
        timestamp: 0, // GRP_DATA carries no timestamp in 1.15
        rssi,
        text: blob,
        data_type: dec.data_type,
        snr_x4,
    })
    .await;
    crate::MESSAGES_WAITING_SIGNAL.signal(());
}

async fn log_advert(
    payload: &[u8],
    rssi: i16,
    path_len_byte: u8,
    path: &heapless::Vec<u8, { meshcore::MAX_PATH_SIZE }>,
) {
    use meshcore::payload::advert;

    let a = match advert::deserialize(payload) {
        Ok(a) => a,
        Err(_) => {
            defmt::warn!("Advert: failed to parse payload");
            return;
        }
    };

    let sig_ok = meshcore::identity::verify_advert(&a, payload).is_ok();

    // Feed companion (role=1, Chat) and repeater (role=2) advert
    // timestamps into the wall-clock seeder.  Only trust
    // signature-verified adverts; the seeder applies its own
    // year-2026 floor, ±10 min delta filter, and hop-count refinement.
    if sig_ok
        && matches!(
        a.role,
        advert::DeviceRole::ChatNode | advert::DeviceRole::Repeater
    )
    {
        let hops = if path_len_byte == contacts::OUT_PATH_UNKNOWN {
            0u8
        } else {
            path_len_byte & 0x3F
        };
        super::repeater_time::observe_timestamp(a.timestamp, hops);
    }

    if let Some(ref name) = a.name {
        defmt::info!(
            "MeshCore advert [{=i16}dBm] role={=u8} name={=[u8]} sig_ok={=bool}",
            rssi,
            a.role.to_u8(),
            &name[..],
            sig_ok,
        );
    } else {
        defmt::info!(
            "MeshCore advert [{=i16}dBm] role={=u8} key={=[u8]} sig_ok={=bool}",
            rssi,
            a.role.to_u8(),
            &a.pub_key[..8],
            sig_ok,
        );
    }

    // Build name string (used both for contacts and display).
    let mut name_str: heapless::String<32> = heapless::String::new();
    if let Some(ref n) = a.name {
        let _ = name_str.push_str(core::str::from_utf8(n).unwrap_or("?"));
    }

    // Upsert into contacts list so TxtMsg can resolve the sender's name.
    CONTACTS.lock(|cell| cell.borrow_mut().upsert(a.pub_key, name_str.clone()));

    let (lat, lon) = a.position.unwrap_or((0, 0));

    // Stamp the local-observation table so the Contacts screen can
    // render an accurate "Last:" relative time and live-dot — the
    // advert's own timestamp is unreliable (most badges advertise
    // `timestamp=0` until their wall clock is set).
    super::contacts_screen::note_observed(&a.pub_key);

    // Record the full advert metadata so the Contacts screen can show
    // discovery rows (heard but not yet in the persistent store) and
    // the popup's "Add" action can promote them later.  Always called
    // — the screen's cache rebuild dedupes against `ContactStore`.
    let is_new_discovery = super::discovery::note(
        &a.pub_key,
        name_str.as_str(),
        a.role.to_u8(),
        lat,
        lon,
        a.timestamp,
    );
    if is_new_discovery {
        super::sounds::play(super::sounds::SoundEvent::ContactDiscovered);
    }

    // Wake the UI redraw loop (`ADVERT_SIGNAL`) and the Contacts
    // screen's cache-refresh task (`REBUILD_SIGNAL`).  Two distinct
    // signals because `embassy_sync::Signal` is single-waiter —
    // sharing one between the redraw loop and the refresh task
    // would silently drop wakes for whichever side registered its
    // waker last.
    crate::ADVERT_SIGNAL.signal(());
    super::contacts_screen::REBUILD_SIGNAL.signal(());

    let mut ble_name: heapless::Vec<u8, 32> = heapless::Vec::new();
    if let Some(ref n) = a.name {
        let _ = ble_name.extend_from_slice(n);
    }
    let _ = crate::ADVERT_BLE_CHANNEL.try_send(crate::AdvertBleNotif {
        pub_key: a.pub_key,
        adv_type: a.role.to_u8(),
        rssi: rssi.clamp(-128, 0) as i8,
        timestamp: a.timestamp,
        lat,
        lon,
        name: ble_name.clone(),
    });

    // Auto-add policy — matches the reference `companion_radio`:
    //
    //   if (manual_add_contacts & 1) == 0 {
    //       auto-add ALL advert types;
    //   } else if autoadd_config & (bit for this type) {
    //       auto-add THIS type (subject to max_hops);
    //   } else {
    //       manual-add only — just push PUSH_CODE_NEW_ADVERT and return.
    //   }
    //
    // `manual_add_contacts` comes from `settings::OtherParams` (CMD 0x26).
    // `autoadd_config` / `autoadd_max_hops` come from settings via CMD 0x3A.
    //
    // Defaults when neither setting has been persisted yet: manual-add-only
    // everywhere. This keeps the `MAX_SLOTS_PER_BUCKET` hash index from
    // filling up in dense neighbourhoods on a fresh device; the user opts
    // into auto-add explicitly via the companion app.
    //
    // The `update_path` call below runs regardless of auto-add policy —
    // it's a no-op for pub_keys that aren't in the store, so unknown senders
    // cost nothing and already-added contacts get their routing path refreshed.
    const AUTO_ADD_OVERWRITE_OLDEST: u8 = 1 << 0; // 0x01 — evict on full
    const AUTO_ADD_CHAT: u8 = 1 << 1; // ADV_TYPE_CHAT     (1)
    const AUTO_ADD_REPEATER: u8 = 1 << 2; // ADV_TYPE_REPEATER (2)
    const AUTO_ADD_ROOM: u8 = 1 << 3; // ADV_TYPE_ROOM     (3)
    const AUTO_ADD_SENSOR: u8 = 1 << 4; // ADV_TYPE_SENSOR   (4)

    let store = contacts::ContactStore::new();

    let manual_add_only = settings::get_other_params()
        .await
        .map(|p| (p.manual_add_contacts & 1) != 0)
        .unwrap_or(true); // default: manual add only

    let (autoadd_config, autoadd_max_hops) = settings::get_autoadd_config().await;

    // Decide whether this advert type is permitted to auto-add.
    let type_allowed = if !manual_add_only {
        true // manual mode off → reference auto-adds all types
    } else {
        let type_bit = match a.role {
            advert::DeviceRole::ChatNode => AUTO_ADD_CHAT,
            advert::DeviceRole::Repeater => AUTO_ADD_REPEATER,
            advert::DeviceRole::RoomServer => AUTO_ADD_ROOM,
            advert::DeviceRole::Sensor => AUTO_ADD_SENSOR,
            advert::DeviceRole::Unknown => 0, // unknown type — never auto-add
        };
        type_bit != 0 && (autoadd_config & type_bit) != 0
    };

    // Hop-count limit: only applied when a limit is set (0 = no limit).
    // Reference semantics (see BaseChatMesh.cpp `onAdvertRecv`):
    //   "0 = no limit, 1 = direct (0 hops), N = up to N-1 hops"
    // i.e. reject when `hops >= max_hops`. Direct packets have
    // `path_len_byte == OUT_PATH_UNKNOWN` and count as 0 hops.
    let hops = if path_len_byte == contacts::OUT_PATH_UNKNOWN {
        0
    } else {
        (path_len_byte & 0x3F) as usize
    };
    let hops_allowed = autoadd_max_hops == 0 || hops < autoadd_max_hops as usize;

    if type_allowed && hops_allowed {
        // Honour the `AUTO_ADD_OVERWRITE_OLDEST` bit: if the store is already
        // full and the bit is *not* set, refuse the add rather than evicting
        // an existing contact. `add_or_update` would otherwise overwrite the
        // oldest non-favourite slot unconditionally (ring-buffer behaviour).
        //
        // The existence check only matters when adding a brand-new contact;
        // if `add_or_update` would have taken the update path for an existing
        // entry the full-store condition doesn't apply. We detect "would
        // update" cheaply via `find_by_key` before the full-store check.
        let overwrite_ok = (autoadd_config & AUTO_ADD_OVERWRITE_OLDEST) != 0;
        let already_known = store.find_by_key(&a.pub_key).await.is_some();
        let store_full = !already_known
            && !overwrite_ok
            && store.count().await as usize >= contacts::MAX_CONTACTS;

        if store_full {
            defmt::info!(
                "contacts: store full, AUTO_ADD_OVERWRITE_OLDEST not set — skipping auto-add of {=[u8]:02x}",
                &a.pub_key[..6],
            );
            crate::CONTACTS_FULL_SIGNAL.signal(());
        } else {
            let contact = contacts::Contact::from_advert(
                a.pub_key,
                name_str.as_bytes(),
                a.role.to_u8(),
                a.timestamp,
                lat,
                lon,
            );
            match store.add_or_update(&contact).await {
                Ok(_) => {
                    // `add_or_update` already marks the store dirty
                    // when bytes actually changed.
                }
                Err(crate::fw::kv::KvError::StoreFull) => {
                    // Hash bucket full (`MAX_SLOTS_PER_BUCKET` rejected the insert).
                    // Matches the spirit of `PUSH_CODE_CONTACTS_FULL` even though
                    // the trigger is different from the reference's slot-count cap.
                    crate::CONTACTS_FULL_SIGNAL.signal(());
                }
                Err(e) => {
                    defmt::warn!(
                        "contacts: add_or_update failed: {:?}",
                        defmt::Debug2Format(&e)
                    );
                }
            }
        }
    }

    if path_len_byte != contacts::OUT_PATH_UNKNOWN {
        let mut path_buf = [0u8; contacts::MAX_PATH_SIZE];
        let copy_len = path.len().min(contacts::MAX_PATH_SIZE);
        path_buf[..copy_len].copy_from_slice(&path[..copy_len]);
        match store
            .update_path(&a.pub_key, path_len_byte, &path_buf)
            .await
        {
            Ok(true) => {
                let _ = crate::PATH_UPDATED_CHANNEL.try_send(a.pub_key);
            }
            Ok(false) => {}
            Err(e) => defmt::warn!("contacts: path update failed: {:?}", e),
        }
    }
}

// ---------------------------------------------------------------------------
// TxtMsg (private message) handler
// ---------------------------------------------------------------------------

async fn log_txt_msg(
    lora: &mut SimpleLoRa<'_>,
    payload: &[u8],
    rssi: i16,
    path_len_byte: u8,
    path: &heapless::Vec<u8, { meshcore::MAX_PATH_SIZE }>,
    identity: &DeviceIdentity,
) {
    use meshcore::payload::txt_msg;

    let msg = match txt_msg::deserialize(payload) {
        Ok(m) => m,
        Err(e) => {
            let reason = match e {
                meshcore::Error::TooShort => "too short",
                meshcore::Error::TooLong => "too long",
                meshcore::Error::Overflow => "overflow",
                _ => "other",
            };
            defmt::warn!(
                "TxtMsg: failed to parse payload ({=usize}B): {=str}",
                payload.len(),
                reason
            );
            return;
        }
    };

    // Only process messages addressed to us (dest_hash = first byte of our
    // pub_key).
    if msg.dest_hash != identity.pub_key[0] {
        return;
    }

    // Fast path: try the most recently hinted target contact (set by any
    // outbound request function) via the O(1) prefix index, skipping the
    // 300-slot linear scan. This is the common case for CLI replies right
    // after we've sent a command to the same peer.
    let store = contacts::ContactStore::new();
    let mut candidates = SenderCandidates::new(msg.src_hash);
    while let Some(c) = candidates.next_contact(&store).await {
        if try_handle_txt_msg(lora, &c, &msg, rssi, path_len_byte, path, identity, &store)
            .await
            .is_ok()
        {
            return;
        }
    }

    defmt::warn!(
        "TxtMsg: received but could not decrypt (sender unknown or MAC fail) [{=i16}dBm]",
        rssi,
    );
}

/// Verify MAC, decrypt and dispatch a `PayloadType::TxtMsg` under the given
/// contact's shared secret.
///
/// Returns `Ok(())` if this contact is the right peer (MAC verified and the
/// message has been pushed to the queue / ACKed / displayed as appropriate).
/// Returns `Err(meshcore::Error::MacMismatch)` when the contact is not the
/// sender — the caller should try the next candidate.
async fn try_handle_txt_msg(
    lora: &mut SimpleLoRa<'_>,
    sender: &contacts::Contact,
    msg: &meshcore::payload::txt_msg::TxtMsg,
    rssi: i16,
    path_len_byte: u8,
    path: &heapless::Vec<u8, { meshcore::MAX_PATH_SIZE }>,
    identity: &DeviceIdentity,
    store: &contacts::ContactStore,
) -> Result<(), meshcore::Error> {
    use meshcore::payload::txt_msg;

    txt_msg::verify_mac(&identity.sec_key, &sender.pub_key, msg)?;
    let (dec, ack_hash) = txt_msg::decrypt(&identity.sec_key, &sender.pub_key, msg)?;

    // Check if this is our own message echoed back by a flood relay. The
    // shared secret is symmetric, so our own ciphertext decrypts successfully
    // against the contact we sent it to. Detect via pending outgoing ACK.
    let is_own_echo = crate::PENDING_ACK.lock(|cell| {
        cell.get()
            .is_some_and(|pending| pending.ack_hash == ack_hash)
    });
    if is_own_echo {
        defmt::debug!(
            "TxtMsg: ack_hash={=u32:#010x} matches PENDING_ACK — ignoring own echo",
            ack_hash,
        );
        return Ok(());
    }

    let text = core::str::from_utf8(&dec.text).unwrap_or("<invalid utf-8>");
    defmt::info!(
        "TxtMsg from {=[u8]:02x} [{=i16}dBm ts={=u32} type={=u8}]: {=str}",
        &sender.pub_key[..6],
        rssi,
        dec.timestamp,
        dec.txt_type(),
        text,
    );

    // Update the stored routing path so replies can go direct.
    if path_len_byte != contacts::OUT_PATH_UNKNOWN {
        let mut path_buf = [0u8; contacts::MAX_PATH_SIZE];
        let copy_len = path.len().min(contacts::MAX_PATH_SIZE);
        path_buf[..copy_len].copy_from_slice(&path[..copy_len]);
        match store
            .update_path(&sender.pub_key, path_len_byte, &path_buf)
            .await
        {
            Ok(true) => {
                let _ = crate::PATH_UPDATED_CHANNEL.try_send(sender.pub_key);
            }
            Ok(false) => {}
            Err(e) => defmt::warn!("TxtMsg: path update failed: {:?}", e),
        }
    }

    // Push plain, CLI, and room-signed messages to the queue so the companion
    // app picks them up via SYNC_NEXT_MESSAGE.
    //
    // Layout of the decrypted plaintext by txt_type (see reference
    // `BaseChatMesh::onPeerDataRecv` at src/helpers/BaseChatMesh.cpp:218):
    //   TXT_TYPE_PLAIN (0):
    //     [ts:4][flags:1][text:N]                      → text = dec.text
    //   TXT_TYPE_CLI_DATA (1):
    //     [ts:4][flags:1][text:N]                      → same layout
    //   TXT_TYPE_SIGNED_PLAIN (2): room-server post push
    //     [ts:4][flags:1][author_prefix:4][text:N]    → dec.text = author_prefix(4)
    // || text
    //
    // For signed messages we store the full `dec.text` verbatim (including the
    // 4-byte author prefix) into msg_queue; the BLE task splits it back out
    // into `ContactMsg.signature` when forwarding to the companion app.
    let is_plain = dec.txt_type() == txt_msg::TXT_TYPE_PLAIN;
    // CLI_COMMAND (1.18): a remote node runs a CLI command on us. Like the
    // reference companion without remote-CLI permission, hand it to the app.
    let is_cli = dec.txt_type() == txt_msg::TXT_TYPE_CLI_DATA
        || dec.txt_type() == txt_msg::TXT_TYPE_CLI_COMMAND;
    let is_signed = dec.txt_type() == txt_msg::TXT_TYPE_SIGNED;

    if is_signed && dec.text.len() < 4 {
        defmt::warn!(
            "TxtMsg signed: payload too short for author prefix ({=usize}B), dropping",
            dec.text.len(),
        );
        return Ok(());
    }

    if is_plain || is_cli || is_signed {
        let mut text_bytes: heapless::Vec<u8, { msg_queue::MAX_TEXT }> = heapless::Vec::new();
        let _ = text_bytes.extend_from_slice(&dec.text[..dec.text.len().min(msg_queue::MAX_TEXT)]);
        let mut sender_prefix = [0u8; 6];
        sender_prefix.copy_from_slice(&sender.pub_key[..6]);
        msg_queue::push(&msg_queue::ReceivedMsg {
            kind: msg_queue::MsgKind::Private,
            sender_prefix,
            channel_idx: 0,
            path_len: path_len_byte,
            text_type: dec.txt_type(),
            timestamp: dec.timestamp,
            rssi,
            text: text_bytes,
            data_type: 0,
            snr_x4: 0,
        })
        .await;
        crate::MESSAGES_WAITING_SIGNAL.signal(());
    }

    // ACK handling differs by type:
    //
    // - PLAIN:  ACK hash from `txt_msg::decrypt` is correct (hashed with the
    //   sender's pub_key, per `BaseChatMesh.cpp:222`). Send it back.
    // - SIGNED: the room expects an ACK whose hash is computed with OUR pub_key
    //   instead of the sender's (per `BaseChatMesh.cpp:249`), over
    //   `[ts:4][flags:1][author_prefix:4][text:N]`. Recompute. Without this ACK the
    //   room retries up to 3 times and then evicts our session, which is the
    //   "previous messages never show up" symptom you saw.
    // - CLI:    no ACK (the repeater doesn't expect one).
    if is_signed {
        // Reassemble the plaintext prefix that the reference hashes over.
        // `dec.text` already holds `[author_prefix:4][text:N]`, so we just
        // need to prepend the 4-byte timestamp and 1-byte flags.
        let mut prefix = [0u8; 5 + meshcore::payload::txt_msg::MAX_TXT_TEXT_SIZE];
        prefix[0..4].copy_from_slice(&dec.timestamp.to_le_bytes());
        prefix[4] = dec.flags;
        let n = dec.text.len().min(prefix.len() - 5);
        prefix[5..5 + n].copy_from_slice(&dec.text[..n]);
        let signed_ack =
            meshcore::payload::txt_msg::compute_ack_hash(&prefix[..5 + n], &identity.pub_key);
        defmt::info!(
            "TxtMsg signed: author={=[u8]:02x} text_len={=usize} ack={=u32:#010x}",
            &dec.text[..4.min(dec.text.len())],
            dec.text.len().saturating_sub(4),
            signed_ack,
        );
        // Upstream still sends the 4-byte form for SIGNED messages
        // (`BaseChatMesh::onPeerDataRecv`, TXT_TYPE_SIGNED_PLAIN branch).
        send_ack(lora, &sender.pub_key, path_len_byte, path, &signed_ack.to_le_bytes()).await;

        // Advance the room's sync_since cursor so the next login resumes
        // after this post instead of replaying the whole backlog. Matches
        // the reference `BaseChatMesh.cpp:242` monotonic advance.
        match store
            .update_sync_since(&sender.pub_key, dec.timestamp)
            .await
        {
            Ok(true) => defmt::info!(
                "TxtMsg signed: sync_since advanced to {=u32} for {=[u8]:02x}",
                dec.timestamp,
                &sender.pub_key[..6],
            ),
            Ok(false) => {} // cursor already >= this timestamp, no write
            Err(e) => defmt::warn!(
                "TxtMsg signed: sync_since persist failed: {:?}",
                defmt::Debug2Format(&e),
            ),
        }
    }

    // Only PLAIN messages get an ACK, a display update, and an LED blink.
    // CLI replies from a repeater are consumed by the companion app only —
    // the repeater does not ACK our requests and does not expect one back.
    if is_plain {
        // MeshCore 1.16+ (f6e6fdaa, 717142ab, 2eb747d5, a130a95a) sends a
        // 6-byte ACK for PLAIN messages: the 4-byte hash, the byte after the
        // text's NUL (the extended attempt counter) and one random byte, so
        // that retries of the same text produce distinct ACK packets and are
        // not dropped as duplicates by repeaters.
        let random = crate::signed_channel::Csprng::next_challenge()[0];
        let ack = meshcore::payload::txt_msg::plain_ack_payload(ack_hash, dec.ack_tail, random);
        send_ack(lora, &sender.pub_key, path_len_byte, path, &ack).await;

        let display_name = {
            let name_end = sender.name.iter().position(|&b| b == 0).unwrap_or(32);
            let name_str = core::str::from_utf8(&sender.name[..name_end]).unwrap_or("");
            if name_str.is_empty() {
                let mut hex: heapless::String<32> = heapless::String::new();
                for &b in &sender.pub_key[..4] {
                    let _ = hex.push(char::from_digit((b >> 4) as u32, 16).unwrap_or('?'));
                    let _ = hex.push(char::from_digit((b & 0xF) as u32, 16).unwrap_or('?'));
                }
                hex
            } else {
                let mut s: heapless::String<32> = heapless::String::new();
                let _ = s.push_str(name_str);
                s
            }
        };
        super::pm_inbox::note_incoming(&sender.pub_key, display_name.as_str(), text);
        super::sounds::play(super::sounds::SoundEvent::PmReceived);
        crate::PM_SIGNAL.signal(());
        crate::PM_UNREAD.store(true, core::sync::atomic::Ordering::Relaxed);

        if !crate::BLE_CONNECTED.load(core::sync::atomic::Ordering::Relaxed) {
            super::pm_inbox::PM_LED.signal(());
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Flood-scope helpers
// ---------------------------------------------------------------------------

/// Return the route type and transport code to use for an outgoing flood
/// packet.
///
/// When a flood-scope key is set (via `SetFloodScope` / 0x36), returns
/// `(TransportFlood, hmac_code)` so that regional repeaters can verify and
/// forward only packets belonging to their region.
///
/// When no key is set, returns `(Flood, 0)` — unscoped, accepted by all nodes.
fn flood_route(payload_type_u8: u8, payload: &[u8]) -> (meshcore::packet::RouteType, u16) {
    scoped_flood_route(
        crate::FLOOD_SCOPE_KEY.lock(|cell| cell.get()),
        payload_type_u8,
        payload,
    )
}

/// [`flood_route`] with an explicit scope key (e.g. a channel's region).
fn scoped_flood_route(
    key: Option<[u8; 16]>,
    payload_type_u8: u8,
    payload: &[u8],
) -> (meshcore::packet::RouteType, u16) {
    use meshcore::packet::RouteType;
    match key {
        Some(key) => {
            let code = meshcore::packet::calc_transport_code(&key, payload_type_u8, payload);
            (RouteType::TransportFlood, code)
        }
        None => (RouteType::Flood, 0),
    }
}

/// `path_len_byte` to use for a freshly-originated flood packet (0 hops).
///
/// Encodes the currently configured `path_hash_mode` as the upper two bits
/// (`hash_size_code`), with `hash_count = 0`. Relays see this and append
/// their hop hashes in the declared byte width. Modes:
///
/// - 0 → 1-byte per-hop hashes (legacy, `path_len_byte = 0x00`)
/// - 1 → 2-byte per-hop hashes (`path_len_byte = 0x40`)
/// - 2 → 3-byte per-hop hashes (`path_len_byte = 0x80`)
///
/// The value comes from the [`crate::PATH_HASH_MODE`] atomic, which is
/// populated at boot from the persisted setting and refreshed by the BLE
/// `CMD_SET_PATH_HASH_MODE` handler. Reading an atomic on every TX is free.
fn flood_path_len_byte() -> u8 {
    let mode = crate::PATH_HASH_MODE.load(core::sync::atomic::Ordering::Relaxed);
    // Defensive clamp: mode 3 is reserved, treat as 2 (3-byte hashes) to avoid
    // emitting a packet with a path_len_byte that the decoder will reject.
    let code = mode.min(2) & 0x03;
    code << 6
}

/// Everything the packet header needs to know about how a packet leaves us:
/// route type, the `path_len_byte` and path bytes, and the transport code.
///
/// The transport code depends on the final payload (it is an HMAC over it),
/// so build a `Route` only after the payload is complete.
struct Route {
    route: meshcore::packet::RouteType,
    path_len_byte: u8,
    path: heapless::Vec<u8, { meshcore::MAX_PATH_SIZE }>,
    transport_code: u16,
}

impl Route {
    /// Flood with the app-controlled flood scope (`TransportFlood` when one is
    /// set, plain `Flood` otherwise), empty path, 0 hops.
    fn flood(ptype: meshcore::packet::PayloadType, payload: &[u8]) -> Self {
        let (route, transport_code) = flood_route(ptype.to_u8(), payload);
        Self {
            route,
            path_len_byte: flood_path_len_byte(),
            path: heapless::Vec::new(),
            transport_code,
        }
    }

    /// Flood scoped to an explicit region key (a channel's region), empty
    /// path, 0 hops.
    fn flood_scoped(
        key: Option<[u8; 16]>,
        ptype: meshcore::packet::PayloadType,
        payload: &[u8],
    ) -> Self {
        let (route, transport_code) = scoped_flood_route(key, ptype.to_u8(), payload);
        Self {
            route,
            path_len_byte: flood_path_len_byte(),
            path: heapless::Vec::new(),
            transport_code,
        }
    }

    /// Direct along an explicit, already-encoded path; no transport code.
    fn direct(path_len_byte: u8, path: &[u8]) -> Self {
        let mut pv: heapless::Vec<u8, { meshcore::MAX_PATH_SIZE }> = heapless::Vec::new();
        let _ = pv.extend_from_slice(path);
        Self {
            route: meshcore::packet::RouteType::Direct,
            path_len_byte,
            path: pv,
            transport_code: 0,
        }
    }

    /// Direct along `c`'s stored out-path. Only valid when the path is known
    /// (`out_path_len != OUT_PATH_UNKNOWN`).
    fn direct_to(c: &contacts::Contact) -> Self {
        Self::direct(c.out_path_len, &c.out_path[..c.path_actual_bytes()])
    }
}

/// Route to a contact: direct along its stored path when one is known,
/// otherwise flood. Run it after `payload` is final (see [`Route`]).
fn route_for_contact(
    contact: Option<&contacts::Contact>,
    ptype: meshcore::packet::PayloadType,
    payload: &[u8],
) -> Route {
    match contact {
        Some(c) if c.out_path_len != contacts::OUT_PATH_UNKNOWN => Route::direct_to(c),
        _ => Route::flood(ptype, payload),
    }
}

/// A serialized packet ready for the radio: the 255-byte frame and its length.
struct Frame {
    buf: [u8; meshcore::MAX_TRANS_UNIT],
    len: usize,
}

impl Frame {
    fn bytes(&self) -> &[u8] {
        &self.buf[..self.len]
    }
}

/// Assemble the header (`version` 0, no second transport code) and serialize
/// the packet. On failure logs `who: packet serialize failed` and returns
/// `None`. Split from [`send_frame`] so callers can set a `PENDING_*` tag
/// after a successful serialize and before the TX.
fn build_frame(
    who: &'static str,
    payload_type: meshcore::packet::PayloadType,
    route: Route,
    payload: heapless::Vec<u8, { meshcore::MAX_PAYLOAD_SIZE }>,
) -> Option<Frame> {
    let msg = meshcore::packet::Message {
        payload_type,
        route: route.route,
        version: 0,
        transport_code: route.transport_code,
        transport_code_2: 0,
        path_len_byte: route.path_len_byte,
        path: route.path,
        payload,
    };
    let mut buf = [0u8; meshcore::MAX_TRANS_UNIT];
    match meshcore::packet::serialize(&msg, &mut buf) {
        Ok(len) => Some(Frame { buf, len }),
        Err(e) => {
            defmt::warn!(
                "{=str}: packet serialize failed: {:?}",
                who,
                defmt::Debug2Format(&e)
            );
            None
        }
    }
}

/// Transmit a built frame. Logs `who: TX failed` and returns `false` on error.
async fn send_frame(lora: &mut SimpleLoRa<'_>, who: &'static str, frame: &Frame) -> bool {
    if let Err(e) = lora.send_message(frame.bytes()).await {
        defmt::warn!("{=str}: TX failed: {:?}", who, e);
        false
    } else {
        true
    }
}

/// [`build_frame`] then [`send_frame`]. Returns the frame on success so the
/// caller can log its length or bytes; `None` if either step failed (already
/// logged).
async fn tx_packet(
    lora: &mut SimpleLoRa<'_>,
    who: &'static str,
    payload_type: meshcore::packet::PayloadType,
    route: Route,
    payload: heapless::Vec<u8, { meshcore::MAX_PAYLOAD_SIZE }>,
) -> Option<Frame> {
    let frame = build_frame(who, payload_type, route, payload)?;
    if send_frame(lora, who, &frame).await {
        Some(frame)
    } else {
        None
    }
}

/// Encrypt `text` into a `PAYLOAD_TYPE_TXT_MSG`-shaped body for `dest` and
/// serialize it. This is the envelope for plain/CLI messages and for every
/// REQ (`txt_type`/`attempt` carry the request type in the flags byte).
/// Returns the payload and the expected ack hash; on failure logs
/// `who: encrypt failed` / `who: serialize failed` and returns `None`.
fn encrypt_txt_payload(
    who: &'static str,
    identity: &DeviceIdentity,
    dest: &[u8; meshcore::PUB_KEY_SIZE],
    timestamp: u32,
    txt_type: u8,
    attempt: u8,
    text: &[u8],
) -> Option<(heapless::Vec<u8, { meshcore::MAX_PAYLOAD_SIZE }>, u32)> {
    use meshcore::payload::txt_msg;

    let (encrypted, ack_hash) = match txt_msg::encrypt(
        &identity.sec_key,
        &identity.pub_key,
        dest,
        timestamp,
        txt_type,
        attempt,
        text,
    ) {
        Ok(r) => r,
        Err(e) => {
            defmt::warn!("{=str}: encrypt failed: {:?}", who, defmt::Debug2Format(&e));
            return None;
        }
    };

    let mut payload_buf = [0u8; meshcore::MAX_PAYLOAD_SIZE];
    let mut payload_len = 0usize;
    if let Err(e) = txt_msg::serialize(&encrypted, &mut payload_buf, &mut payload_len) {
        defmt::warn!(
            "{=str}: serialize failed: {:?}",
            who,
            defmt::Debug2Format(&e)
        );
        return None;
    }
    let payload = heapless::Vec::from_slice(&payload_buf[..payload_len]).ok()?;
    Some((payload, ack_hash))
}

/// Candidate senders for an incoming request/response packet that only
/// carries the first byte of the sender's pub_key (`src_hash`).
///
/// Yields, in order: the `LAST_REQ_TARGET` hint if its first byte matches
/// (the common case right after we sent that peer a request; no contact
/// lookup needed for [`next_pub_key`](Self::next_pub_key), so it works for
/// peers that are not in the contact store), then every stored contact in the
/// `src_hash` bucket of the hash-byte index whose first byte really matches,
/// skipping the hint (already tried). The bucket is read lazily, after the
/// hint failed: one flash `get` plus up to `MAX_SLOTS_PER_BUCKET` slot reads
/// instead of a full-table scan. The caller stops at the first `Ok`.
struct SenderCandidates {
    src_hash: u8,
    hint: Option<[u8; meshcore::PUB_KEY_SIZE]>,
    hint_pending: bool,
    slots: [u16; contacts::MAX_SLOTS_PER_BUCKET],
    n: usize,
    next_slot: usize,
    bucket_read: bool,
}

impl SenderCandidates {
    /// Candidates for `src_hash`, trying the `LAST_REQ_TARGET` hint first.
    fn new(src_hash: u8) -> Self {
        let hint = crate::LAST_REQ_TARGET.lock(|cell| cell.get());
        Self {
            hint,
            hint_pending: hint.is_some_and(|pk| pk[0] == src_hash),
            ..Self::without_hint(src_hash)
        }
    }

    /// Candidates for `src_hash` from the contact store only.
    fn without_hint(src_hash: u8) -> Self {
        Self {
            src_hash,
            hint: None,
            hint_pending: false,
            slots: [0u16; contacts::MAX_SLOTS_PER_BUCKET],
            n: 0,
            next_slot: 0,
            bucket_read: false,
        }
    }

    /// The next candidate's pub_key; the hint is returned as-is, without
    /// checking that it is a stored contact.
    async fn next_pub_key(
        &mut self,
        store: &contacts::ContactStore,
    ) -> Option<[u8; meshcore::PUB_KEY_SIZE]> {
        if self.hint_pending {
            self.hint_pending = false;
            return self.hint;
        }
        self.next_stored(store).await.map(|c| c.pub_key)
    }

    /// The next candidate as a stored contact; a hint that is not in the
    /// store is skipped.
    async fn next_contact(&mut self, store: &contacts::ContactStore) -> Option<contacts::Contact> {
        if self.hint_pending {
            self.hint_pending = false;
            if let Some(hint_pk) = self.hint
                && let Some(c) = store.find_by_key(&hint_pk).await
            {
                return Some(c);
            }
        }
        self.next_stored(store).await
    }

    /// Next contact from the hash bucket, skipping the hint.
    async fn next_stored(&mut self, store: &contacts::ContactStore) -> Option<contacts::Contact> {
        if !self.bucket_read {
            self.bucket_read = true;
            self.n = store
                .hash_index_lookup(self.src_hash, &mut self.slots)
                .await;
        }
        while self.next_slot < self.n {
            let slot = self.slots[self.next_slot];
            self.next_slot += 1;
            let Some(c) = store.read_slot(slot as usize).await else {
                continue;
            };
            if c.pub_key[0] != self.src_hash {
                continue;
            }
            if self.hint == Some(c.pub_key) {
                continue;
            }
            return Some(c);
        }
        None
    }
}

// ---------------------------------------------------------------------------
// Channel message transmission
// ---------------------------------------------------------------------------

/// Encrypt and broadcast a group-text message on channel slot
/// `req.channel_idx`.
///
/// The channel key is looked up first from the already-loaded in-RAM table,
/// with a direct KV fallback for channels set after the last reload.
async fn dispatch_tx(
    lora: &mut SimpleLoRa<'_>,
    loaded_channels: &heapless::Vec<LoadedChannel, { channels::NUM_CHANNELS }>,
    identity: &DeviceIdentity,
    req: crate::TxRequest,
) {
    match req {
        crate::TxRequest::ChannelMsg(msg) => send_grp_txt(lora, loaded_channels, msg).await,
        crate::TxRequest::ChannelData(msg) => send_grp_data(lora, loaded_channels, msg).await,
        crate::TxRequest::PrivateMsg(msg) => send_txt_msg(lora, msg, identity).await,
        crate::TxRequest::Trace(msg) => send_trace(lora, msg).await,
        crate::TxRequest::Login(msg) => send_login(lora, msg, identity).await,
        crate::TxRequest::AdminStatusReq(msg) => {
            send_admin_status_request(lora, msg, identity).await
        }
        crate::TxRequest::BinaryReq(msg) => send_binary_request(lora, msg, identity).await,
        crate::TxRequest::AnonReq(msg) => send_anon_request(lora, msg, identity).await,
        crate::TxRequest::TelemReq(msg) => send_telem_request(lora, msg, identity).await,
        crate::TxRequest::DiscoveryReq(msg) => send_discovery_request(lora, msg, identity).await,
        crate::TxRequest::ControlData(msg) => send_control_data(lora, msg).await,
        crate::TxRequest::Advert(mode) => {
            let mut name_buf = [0u8; settings::MAX_NODE_NAME];
            let name_len = settings::get_node_name(&mut name_buf).await;
            // Upstream nodes drop an advert whose timestamp is not newer than
            // the last one they kept from us (replay check), so it must move.
            let timestamp = crate::unix_now_unique().unwrap_or(0);
            send_advert(lora, identity, &name_buf[..name_len], timestamp, mode).await;
        }
        crate::TxRequest::RawFrame { data, len } => {
            if let Err(e) = lora.send_message(&data[..len]).await {
                defmt::warn!("relay TX failed: {:?}", e);
            }
        }
    }
}

async fn send_grp_txt(
    lora: &mut SimpleLoRa<'_>,
    loaded_channels: &[LoadedChannel],
    req: crate::TxChannelMsg,
) {
    use meshcore::packet::PayloadType;
    use meshcore::payload::grp_txt;
    use meshcore::MAX_PAYLOAD_SIZE;

    // Resolve the channel key from the in-RAM table (kept current via
    // CHANNELS_CHANGED_SIGNAL).
    let Some(ch) = loaded_channels
        .iter()
        .find(|c| c.slot_idx == req.channel_idx)
    else {
        defmt::warn!(
            "send_grp_txt: channel slot {=u8} not in RAM table, dropping",
            req.channel_idx
        );
        return;
    };
    let (key, hash) = (ch.key, ch.hash);
    // Badge-originated sends use the channel's effective region; companion
    // sends keep the app-controlled flood scope.
    let scope = if req.channel_region {
        ch.scope_key()
    } else {
        crate::FLOOD_SCOPE_KEY.lock(|c| c.get())
    };

    // MeshCore GrpTxt wire format embeds the sender as "Name: MessageText".
    // Use the persisted node name, falling back to the 4-byte device-ID hex if
    // unset.
    let mut name_buf = [0u8; settings::MAX_NODE_NAME];
    let name_len = {
        let n = settings::get_node_name(&mut name_buf).await;
        if n == 0 {
            let id = crate::fw::device_id::get_bytes();
            name_buf[..id.len()].copy_from_slice(&id);
            id.len()
        } else {
            n
        }
    };
    let sender = &name_buf[..name_len];

    let mut wire_text: heapless::Vec<u8, { meshcore::MAX_GRP_DATA_SIZE }> = heapless::Vec::new();
    let _ = wire_text.extend_from_slice(sender);
    let _ = wire_text.extend_from_slice(b": ");
    let body_max = meshcore::MAX_GRP_DATA_SIZE.saturating_sub(name_len + 2);
    let _ = wire_text.extend_from_slice(&req.text[..req.text.len().min(body_max)]);

    let grp = match grp_txt::encrypt(&key, hash, req.timestamp, 0, &wire_text) {
        Ok(g) => g,
        Err(e) => {
            defmt::warn!(
                "send_grp_txt: encrypt failed: {:?}",
                defmt::Debug2Format(&e)
            );
            return;
        }
    };

    let mut payload_buf = [0u8; MAX_PAYLOAD_SIZE];
    let mut payload_len = 0usize;
    if let Err(e) = grp_txt::serialize(&grp, &mut payload_buf, &mut payload_len) {
        defmt::warn!(
            "send_grp_txt: serialize failed: {:?}",
            defmt::Debug2Format(&e)
        );
        return;
    }

    let mut msg_payload: heapless::Vec<u8, MAX_PAYLOAD_SIZE> = heapless::Vec::new();
    let _ = msg_payload.extend_from_slice(&payload_buf[..payload_len]);

    let route = Route::flood_scoped(scope, PayloadType::GrpTxt, &msg_payload);
    let Some(frame) = tx_packet(
        lora,
        "send_grp_txt",
        PayloadType::GrpTxt,
        route,
        msg_payload,
    )
    .await
    else {
        return;
    };
    let len = frame.len;
    defmt::info!(
        "GrpTxt sent: ch={=u8} ts={=u32} len={=usize}B frame={=[u8]:02x}",
        req.channel_idx,
        req.timestamp,
        len,
        frame.bytes(),
    );

    // Seed MSG_SEEN so relay-bounces of our own packet are
    // suppressed.  We do NOT push to the companion queue — the
    // companion app already knows it sent this message.
    let content_hash = msg_hash(hash, &wire_text, req.timestamp);
    MSG_SEEN.lock(|cell| cell.borrow_mut().insert(content_hash));

    // Add to the channel message ring so the on-device browser
    // shows our own sent messages.
    {
        let body = core::str::from_utf8(&req.text).unwrap_or("");
        let mut t: heapless::String<{ crate::CHANNEL_MSG_TEXT_MAX }> =
            heapless::String::new();
        let _ = t.push_str(truncate_str(body, crate::CHANNEL_MSG_TEXT_MAX));
        crate::CHANNEL_MSG_RING.lock(|cell| {
            cell.borrow_mut().push(crate::ChannelMsgEntry {
                channel_idx: req.channel_idx,
                sender: heapless::String::new(),
                text: t,
                timestamp: req.timestamp,
                content_hash,
                is_own: true,
                repeat_count: 0,
            });
        });
        crate::LORA_MSG_SIGNAL.signal(());
    }
}

// ---------------------------------------------------------------------------
// Channel binary-data transmission (PAYLOAD_TYPE_GRP_DATA, MeshCore 1.15)
// ---------------------------------------------------------------------------

/// Encrypt and broadcast an application-defined binary blob on a channel
/// using the MeshCore 1.15 `PAYLOAD_TYPE_GRP_DATA` envelope.
///
/// When `req.path_len == OUT_PATH_UNKNOWN` the packet is flood-routed.
/// Otherwise it is sent `RouteType::Direct` along the supplied path
/// (mirrors upstream `BaseChatMesh::sendGroupData` ed326255).
async fn send_grp_data(
    lora: &mut SimpleLoRa<'_>,
    loaded_channels: &[LoadedChannel],
    req: crate::TxChannelData,
) {
    use meshcore::packet::{PayloadType, RouteType};
    use meshcore::payload::grp_data;
    use meshcore::MAX_PAYLOAD_SIZE;

    let Some(ch) = loaded_channels
        .iter()
        .find(|c| c.slot_idx == req.channel_idx)
    else {
        defmt::warn!(
            "send_grp_data: channel slot {=u8} not in RAM table, dropping",
            req.channel_idx
        );
        return;
    };
    let (key, hash) = (ch.key, ch.hash);

    let grp = match grp_data::encrypt(&key, hash, req.data_type, &req.data) {
        Ok(g) => g,
        Err(e) => {
            defmt::warn!(
                "send_grp_data: encrypt failed: {:?}",
                defmt::Debug2Format(&e)
            );
            return;
        }
    };

    let mut payload_buf = [0u8; MAX_PAYLOAD_SIZE];
    let mut payload_len = 0usize;
    if let Err(e) = grp_data::serialize(&grp, &mut payload_buf, &mut payload_len) {
        defmt::warn!(
            "send_grp_data: serialize failed: {:?}",
            defmt::Debug2Format(&e)
        );
        return;
    }

    let mut msg_payload: heapless::Vec<u8, MAX_PAYLOAD_SIZE> = heapless::Vec::new();
    let _ = msg_payload.extend_from_slice(&payload_buf[..payload_len]);

    let route = if req.path_len == contacts::OUT_PATH_UNKNOWN {
        Route::flood(PayloadType::GrpData, &msg_payload)
    } else {
        // `path_len` is the encoded byte ((hash_size-1)<<6 | hops), not a
        // byte count; the parser already cut `path` to hops * hash_size.
        Route::direct(req.path_len, &req.path)
    };

    let direct = route.route == RouteType::Direct;
    let Some(frame) = tx_packet(
        lora,
        "send_grp_data",
        PayloadType::GrpData,
        route,
        msg_payload,
    )
    .await
    else {
        return;
    };
    let len = frame.len;
    defmt::info!(
        "GrpData sent: ch={=u8} type={=u16:#06x} blob={=usize}B route={=str} len={=usize}B",
        req.channel_idx,
        req.data_type,
        req.data.len(),
        if direct {
            "direct"
        } else {
            "flood"
        },
        len,
    );
}

// ---------------------------------------------------------------------------
// Private message transmission
// ---------------------------------------------------------------------------

/// Encrypt and send a private (TxtMsg) message to a contact.
///
/// Uses `RouteType::Direct` with the stored path when one is known, falling
/// back to `RouteType::Flood` when `out_path_len == OUT_PATH_UNKNOWN`.
async fn send_txt_msg(
    lora: &mut SimpleLoRa<'_>,
    req: crate::TxPrivateMsg,
    identity: &DeviceIdentity,
) {
    use meshcore::packet::{PayloadType, RouteType};
    use meshcore::payload::txt_msg;

    // Hint for the RX fast path so the CLI reply (or plain-chat reply) lands
    // on a single O(1) find_by_key instead of a 300-slot linear scan.
    crate::LAST_REQ_TARGET.lock(|cell| cell.set(Some(req.recipient_pub_key)));

    // Look up the recipient contact for their stored path.
    let contact = contacts::ContactStore::new()
        .find_by_key(&req.recipient_pub_key)
        .await;

    let Some((msg_payload, expected_ack)) = encrypt_txt_payload(
        "send_txt_msg",
        identity,
        &req.recipient_pub_key,
        req.timestamp,
        req.txt_type,
        req.attempt,
        &req.text,
    ) else {
        return;
    };


    let route = route_for_contact(contact.as_ref(), PayloadType::TxtMsg, &msg_payload);
    let direct = route.route == RouteType::Direct;
    let Some(frame) = tx_packet(
        lora,
        "send_txt_msg",
        PayloadType::TxtMsg,
        route,
        msg_payload,
    )
    .await
    else {
        return;
    };
    let len = frame.len;
    defmt::info!(
        "TxtMsg sent: to={=[u8]:02x} attempt={=u8} route={=str} len={=usize}B ack={=u32:#010x}",
        &req.recipient_pub_key[..6],
        req.attempt,
        if direct {
            "direct"
        } else {
            "flood"
        },
        len,
        expected_ack,
    );
    // Record pending ACK so we can compute round-trip time when the
    // mesh ACKs back. CLI commands are not ACKed by the receiver
    // (matches C++ `MyMesh::onPeerDataRecv` which only ACKs TXT_TYPE_PLAIN),
    // so we skip the pending-ACK slot for them.
    if req.txt_type == txt_msg::TXT_TYPE_PLAIN {
        crate::PENDING_ACK.lock(|cell| {
            cell.set(Some(crate::PendingAck {
                ack_hash: expected_ack,
                sent_at: embassy_time::Instant::now(),
            }));
        });
        // Stamp the matching outgoing PmEntry so the
        // thread view's delivery marker can flip from
        // "in-flight" to "delivered" when the ACK comes
        // back via `handle_ack_recv`.
        super::pm_inbox::stamp_outgoing_ack(&req.recipient_pub_key, expected_ack);
    }
}

// ---------------------------------------------------------------------------
// Advert transmission
// ---------------------------------------------------------------------------

/// Build and broadcast a signed advert packet for this device.
///
/// `name` is the device name shown to other MeshCore nodes (max 32 bytes).
/// `timestamp` should be a monotonic counter or wall-clock seconds.
pub async fn send_advert(
    lora: &mut SimpleLoRa<'_>,
    identity: &DeviceIdentity,
    name: &[u8],
    timestamp: u32,
    mode: AdvertMode,
) {
    use meshcore::packet::{PayloadType, RouteType};

    let Some(msg_payload) = self_advert_payload(&identity.sec_key, name, timestamp) else {
        return;
    };

    // Both modes carry `flood_path_len_byte()` and an empty path (0 hops);
    // zero-hop only differs in the route type and having no transport code.
    let route = match mode {
        AdvertMode::Flood => Route::flood(PayloadType::Advert, &msg_payload),
        AdvertMode::ZeroHop => Route {
            route: RouteType::Direct, // path_len=0 → zero-hop direct
            path_len_byte: flood_path_len_byte(),
            path: heapless::Vec::new(),
            transport_code: 0,
        },
    };

    let Some(frame) = tx_packet(
        lora,
        "send_advert",
        PayloadType::Advert,
        route,
        msg_payload,
    )
    .await
    else {
        return;
    };
    let len = frame.len;
    defmt::info!(
        "MeshCore advert sent ({=usize}B, {=str})",
        len,
        if mode == AdvertMode::Flood {
            "flood"
        } else {
            "zero-hop"
        },
    );
}

/// Our signed advert payload: pub_key, timestamp, signature and app data
/// (chat-node role and `name`, at most 32 bytes). `None` if signing or
/// serialising fails, which is logged.
fn self_advert_payload(
    sec_key: &[u8; meshcore::PRV_KEY_SIZE],
    name: &[u8],
    timestamp: u32,
) -> Option<heapless::Vec<u8, { meshcore::MAX_PAYLOAD_SIZE }>> {
    use meshcore::MAX_PAYLOAD_SIZE;
    use meshcore::payload::advert::{Advert, DeviceRole, serialize};

    let mut advert = Advert {
        // Filled in from the secret key by `sign_advert`.
        pub_key: [0u8; meshcore::PUB_KEY_SIZE],
        timestamp,
        signature: [0u8; meshcore::SIGNATURE_SIZE],
        role: DeviceRole::ChatNode,
        name: {
            let mut v = heapless::Vec::new();
            let _ = v.extend_from_slice(&name[..name.len().min(32)]);
            if v.is_empty() { None } else { Some(v) }
        },
        position: None,
        extra1: None,
        extra2: None,
    };

    if let Err(e) = meshcore::identity::sign_advert(sec_key, &mut advert) {
        defmt::warn!("advert: signing failed: {:?}", defmt::Debug2Format(&e));
        return None;
    }

    let mut payload_buf = [0u8; MAX_PAYLOAD_SIZE];
    let mut payload_len = 0usize;
    if let Err(e) = serialize(&advert, &mut payload_buf, &mut payload_len) {
        defmt::warn!("advert: serialize failed: {:?}", defmt::Debug2Format(&e));
        return None;
    }
    heapless::Vec::from_slice(&payload_buf[..payload_len]).ok()
}

/// Our advert as a whole packet, the form `CMD_EXPORT_CONTACT` answers with
/// (upstream `createSelfAdvert` with `ROUTE_TYPE_FLOOD`, never sent, so no
/// transport codes and an empty path). Returns the length written to `out`.
pub fn export_self_advert(
    sec_key: &[u8; meshcore::PRV_KEY_SIZE],
    name: &[u8],
    timestamp: u32,
    out: &mut [u8; meshcore::MAX_TRANS_UNIT],
) -> Option<usize> {
    use meshcore::packet::{Message, PayloadType, RouteType};

    let msg = Message {
        payload_type: PayloadType::Advert,
        route: RouteType::Flood,
        version: 0,
        transport_code: 0,
        transport_code_2: 0,
        path_len_byte: 0,
        path: heapless::Vec::new(),
        payload: self_advert_payload(sec_key, name, timestamp)?,
    };
    meshcore::packet::serialize(&msg, out).ok()
}

// ---------------------------------------------------------------------------
// PM (TxtMsg) transmission
// ---------------------------------------------------------------------------

/// Encrypt and send a private message to `recipient_pk`.
///
/// The recipient must have previously broadcast an advert so their key is
/// known to the mesh.  `text` is plain UTF-8, max
/// [`meshcore::payload::txt_msg::MAX_TXT_TEXT_SIZE`] bytes.
pub async fn send_pm(
    lora: &mut SimpleLoRa<'_>,
    identity: &DeviceIdentity,
    recipient_pk: &[u8; meshcore::PUB_KEY_SIZE],
    text: &[u8],
    timestamp: u32,
) {
    use meshcore::packet::PayloadType;
    use meshcore::payload::txt_msg;

    let Some((msg_payload, expected_ack)) = encrypt_txt_payload(
        "send_pm",
        identity,
        recipient_pk,
        timestamp,
        txt_msg::TXT_TYPE_PLAIN,
        0,
        text,
    ) else {
        return;
    };

    defmt::info!("send_pm: expected_ack={=u32:#010x}", expected_ack);

    // TxtMsg uses Direct route so the full path to the recipient is embedded.
    // For now we send as Flood — the recipient will filter on dest_pub_key.
    let route = Route::flood(PayloadType::TxtMsg, &msg_payload);
    let Some(frame) = tx_packet(
        lora,
        "send_pm",
        PayloadType::TxtMsg,
        route,
        msg_payload,
    )
    .await
    else {
        return;
    };
    let len = frame.len;
    defmt::info!("PM sent ({=usize}B)", len);
}

// ---------------------------------------------------------------------------
// ACK transmission
// ---------------------------------------------------------------------------

/// Send an ACK for a received TxtMsg back toward the sender.
///
/// If we have a stored path for the sender we send Direct; otherwise Flood.
/// `ack` is the full ACK payload (4 bytes for SIGNED, 6 for PLAIN); it must
/// be at least 4 bytes long.
async fn send_ack(
    lora: &mut SimpleLoRa<'_>,
    sender_pk: &[u8; meshcore::PUB_KEY_SIZE],
    _recv_path_len: u8,
    _recv_path: &heapless::Vec<u8, { meshcore::MAX_PATH_SIZE }>,
    ack: &[u8],
) {
    use meshcore::packet::PayloadType;

    // The first four bytes are the ACK hash; receivers match on those only.
    let ack_hash = u32::from_le_bytes([ack[0], ack[1], ack[2], ack[3]]);

    let mut payload: heapless::Vec<u8, { meshcore::MAX_PAYLOAD_SIZE }> = heapless::Vec::new();
    let _ = payload.extend_from_slice(ack);

    // Route: Direct if we have a stored path, Flood otherwise.
    let contact = contacts::ContactStore::new().find_by_key(sender_pk).await;
    let route = route_for_contact(contact.as_ref(), PayloadType::Ack, &payload);

    let Some(frame) = tx_packet(lora, "send_ack", PayloadType::Ack, route, payload).await else {
        return;
    };
    let len = frame.len;
    defmt::info!("ACK sent ack={=u32:#010x} ({=usize}B)", ack_hash, len);
}

// ---------------------------------------------------------------------------
// ACK reception — notify BLE client
// ---------------------------------------------------------------------------

/// Handle a received `PayloadType::Ack` packet from the mesh.
///
/// Parses the 4-byte ack_crc from `payload`, matches it against the pending
/// sent message stored in `PENDING_ACK`, then pushes a `PACKET_ACK` (0x82)
/// event to the BLE task via `ACK_EVENT_CHANNEL`.
fn handle_ack_recv(payload: &[u8], rssi: i16) {
    if payload.len() < 4 {
        defmt::warn!(
            "handle_ack_recv: payload too short ({=usize}B)",
            payload.len()
        );
        return;
    }
    let ack_crc = u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
    defmt::info!(
        "MeshCore Ack: ack_crc={=u32:#010x} [{=i16}dBm]",
        ack_crc,
        rssi
    );

    let trip_time_ms = crate::PENDING_ACK.lock(|cell| {
        let pending = cell.take();
        if let Some(p) = pending {
            if p.ack_hash == ack_crc {
                let elapsed = p.sent_at.elapsed().as_millis() as u32;
                return elapsed;
            }
            // Not our ACK — put it back.
            cell.set(Some(p));
        }
        0u32
    });

    // Flip any matching outgoing PmEntry's delivery state so the
    // thread view's marker updates.  No-op if the ACK is for a
    // BLE-companion-originated PM that we never tracked locally.
    super::pm_inbox::handle_ack(ack_crc);

    let _ = crate::ACK_EVENT_CHANNEL.try_send(crate::AckEvent {
        ack_crc,
        trip_time_ms,
    });
}

// ---------------------------------------------------------------------------
// Trace-path transmission
// ---------------------------------------------------------------------------

/// Build and transmit a TRACE packet.
///
/// TRACE packets use `RouteType::Direct` with the routing path embedded in
/// the payload (not the packet path field).  Each relay along the path appends
/// its SNR and rebroadcasts; the final node calls `onTraceRecv` and (on
/// MeshCore firmware) pushes a `0x89 PUSH_CODE_TRACE_DATA` response to its
/// companion app.
///
/// Wire payload: `[tag:4 LE][auth:4 LE][flags:1][path_hashes...]`
async fn send_trace(lora: &mut SimpleLoRa<'_>, req: crate::TxTracePath) {
    use meshcore::packet::PayloadType;
    use meshcore::MAX_PAYLOAD_SIZE;

    let mut payload: heapless::Vec<u8, MAX_PAYLOAD_SIZE> = heapless::Vec::new();
    let _ = payload.extend_from_slice(&req.tag.to_le_bytes());
    let _ = payload.extend_from_slice(&req.auth.to_le_bytes());
    let _ = payload.push(req.flags);
    let _ = payload.extend_from_slice(&req.path);

    let Some(frame) = tx_packet(
        lora,
        "send_trace",
        PayloadType::Trace,
        Route::direct(0, &[]),
        payload,
    ).await else {
        return;
    };
    let len = frame.len;
    defmt::info!(
        "Trace sent: tag={=u32:#010x} path_len={=usize}B len={=usize}B",
        req.tag,
        req.path.len(),
        len,
    );
}

// ---------------------------------------------------------------------------
// Login (ANON_REQ) transmission
// ---------------------------------------------------------------------------

/// Build and transmit an ANON_REQ login packet.
///
/// Uses `RouteType::Flood` when no path to the target is known (the common
/// case for first login), and `RouteType::Direct` when a stored path exists.
/// This mirrors C++ `BaseChatMesh::sendLogin`: flood when `out_path_len ==
/// OUT_PATH_UNKNOWN`.
async fn send_login(lora: &mut SimpleLoRa<'_>, req: crate::TxLogin, identity: &DeviceIdentity) {
    use meshcore::packet::PayloadType;

    // Hint for the RX fast path so we skip the 300-slot contact scan.
    crate::LAST_REQ_TARGET.lock(|cell| cell.set(Some(req.pub_key)));

    // Bail out if the wall clock isn't set. Sending `timestamp = 0` is
    // guaranteed to be rejected by the server as a replay attack:
    //
    //     client = acl.putClient(...);           // fresh entry, last_timestamp = 0
    //     if (sender_timestamp <= last_timestamp) // 0 <= 0 → true → silent drop
    //
    // The phone must issue CMD_SET_DEVICE_TIME (0x06) before login can work.
    let Some(timestamp) = crate::unix_now() else {
        defmt::warn!(
            "send_login: wall clock not set — refusing to send login (would be silently dropped by server as replay). Phone must issue SET_DEVICE_TIME first."
        );
        return;
    };

    // Look up the contact once, up front, so we can branch on:
    //   (a) its `node_type` to decide the login plaintext format (rooms
    //       include a 4-byte `sync_since` header, repeaters do not), and
    //   (b) its `out_path_len` to pick Flood vs Direct routing below.
    let contact = contacts::ContactStore::new()
        .find_by_key(&req.pub_key)
        .await;

    // When the target is a room server we include the persisted
    // `Contact.sync_since` — the last post timestamp we successfully ACKed.
    // The room uses this to resume pushing posts from where we left off
    // instead of replaying the whole backlog. Non-room targets omit the
    // sync_since header entirely (repeater-shaped login plaintext).
    let is_room = contact
        .as_ref()
        .map(|c| {
            meshcore::payload::advert::DeviceRole::from(c.node_type)
                == meshcore::payload::advert::DeviceRole::RoomServer
        })
        .unwrap_or(false);
    let sync_since = if is_room {
        Some(contact.as_ref().map(|c| c.sync_since).unwrap_or(0))
    } else {
        None
    };

    let payload = match meshcore::payload::anon_req::encrypt(
        &identity.sec_key,
        &identity.pub_key,
        &req.pub_key,
        timestamp,
        sync_since,
        &req.password,
    ) {
        Ok(p) => p,
        Err(e) => {
            defmt::warn!("send_login: encrypt failed: {:?}", defmt::Debug2Format(&e));
            return;
        }
    };

    // Flood when no path is known; direct when a stored path exists.
    let route = route_for_contact(contact.as_ref(), PayloadType::AnonReq, &payload);
    let Some(frame) = build_frame("send_login", PayloadType::AnonReq, route, payload) else {
        return;
    };
    crate::PENDING_LOGIN.set(crate::login_tag(&req.pub_key));
    if send_frame(lora, "send_login", &frame).await {
        let len = frame.len;
        defmt::info!(
            "Login sent to {=[u8]:02x} node_type={=u8} is_room={=bool} ts={=u32} sync_since={=u32} ({=usize}B) frame={=[u8]:02x}",
            &req.pub_key[..6],
            contact.as_ref().map(|c| c.node_type).unwrap_or(0xff),
            is_room,
            timestamp,
            sync_since.unwrap_or(0),
            len,
            frame.bytes(),
        );
    } else {
        crate::PENDING_LOGIN.clear();
    }
}

// ---------------------------------------------------------------------------
// Status request transmission
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Telemetry request transmission
// ---------------------------------------------------------------------------

/// Build and transmit a `PAYLOAD_TYPE_REQ` telemetry request.
///
/// Plaintext layout (13 bytes, same as C++ `sendRequest` with `req_type`):
/// ```text
/// [tag:4 LE][req_type:1 = 0x03][reserved:4 zeros][random:4]
/// ```
/// Uses `txt_msg::encrypt` with `attempt = 0x03` (= flags byte) and
/// `text = [0;4] ++ [random;4]` to produce the correct plaintext.
async fn send_telem_request(
    lora: &mut SimpleLoRa<'_>,
    req: crate::TxTelemReq,
    identity: &DeviceIdentity,
) {
    use meshcore::packet::PayloadType;

    crate::LAST_REQ_TARGET.lock(|cell| cell.set(Some(req.pub_key)));

    // REQ_TYPE_GET_TELEMETRY_DATA = 0x03.
    // txt_msg::encrypt encodes flags = (attempt & 3) | (txt_type << 2).
    // To get flags = 0x03: attempt = 3, txt_type = 0.
    // text = [reserved:4][random:4] — we use zeros for both (no RNG needed).
    const REQ_TYPE_TELEMETRY: u8 = 0x03;
    // attempt = REQ_TYPE_TELEMETRY so that flags byte = 0x03 in the AES plaintext.
    let text: [u8; 8] = [0u8; 8]; // reserved(4) + random(4) — zeros acceptable

    let Some((payload_vec, _)) = encrypt_txt_payload(
        "send_telem_req",
        identity,
        &req.pub_key,
        req.tag,
        0,
        REQ_TYPE_TELEMETRY,
        &text,
    ) else {
        return;
    };

    let contact = contacts::ContactStore::new()
        .find_by_key(&req.pub_key)
        .await;

    let route = route_for_contact(contact.as_ref(), PayloadType::Req, &payload_vec);
    let Some(frame) = build_frame("send_telem_req", PayloadType::Req, route, payload_vec) else {
        return;
    };
    crate::PENDING_TELEM_TAG.set(req.tag);
    if send_frame(lora, "send_telem_req", &frame).await {
        let len = frame.len;
        defmt::info!(
            "Telem req sent to {=[u8]:02x} tag={=u32:#010x} ({=usize}B)",
            &req.pub_key[..6],
            req.tag,
            len
        );
    } else {
        crate::PENDING_TELEM_TAG.clear();
    }
}

// ---------------------------------------------------------------------------
// RepeaterStats parser (shared by Response and Path receive handlers)
// ---------------------------------------------------------------------------

/// Parse the 56-byte `RepeaterStats` blob returned by `REQ_TYPE_GET_STATUS`.
///
/// `blob` must be at least 56 bytes; shorter inputs are tolerated by
/// reading from a zero-padded local buffer (the tail fields become 0).
///
/// C++ reference: `examples/simple_repeater/MyMesh.h` `struct RepeaterStats`.
fn parse_repeater_stats(
    blob: &[u8],
    pub_key: [u8; meshcore::PUB_KEY_SIZE],
    tag: u32,
) -> crate::AdminStatusResult {
    let mut s = [0u8; 56];
    let n = blob.len().min(56);
    s[..n].copy_from_slice(&blob[..n]);

    let rd_u16 = |o: usize| u16::from_le_bytes([s[o], s[o + 1]]);
    let rd_i16 = |o: usize| i16::from_le_bytes([s[o], s[o + 1]]);
    let rd_u32 = |o: usize| u32::from_le_bytes([s[o], s[o + 1], s[o + 2], s[o + 3]]);

    crate::AdminStatusResult {
        pub_key,
        tag,
        batt_milli_volts: rd_u16(0),
        curr_tx_queue_len: rd_u16(2),
        noise_floor: rd_i16(4),
        last_rssi: rd_i16(6),
        n_packets_recv: rd_u32(8),
        n_packets_sent: rd_u32(12),
        total_air_time_secs: rd_u32(16),
        total_up_time_secs: rd_u32(20),
        n_sent_flood: rd_u32(24),
        n_sent_direct: rd_u32(28),
        n_recv_flood: rd_u32(32),
        n_recv_direct: rd_u32(36),
        err_events: rd_u16(40),
        last_snr_x4: rd_i16(42),
        n_direct_dups: rd_u16(44),
        n_flood_dups: rd_u16(46),
        total_rx_air_time_secs: rd_u32(48),
        n_recv_errors: rd_u32(52),
    }
}

// ---------------------------------------------------------------------------
// Admin status request transmission
// ---------------------------------------------------------------------------

/// Build and transmit a `PAYLOAD_TYPE_REQ` with `REQ_TYPE_GET_STATUS` (0x01).
///
/// This is the **authenticated** status query — the caller must already be
/// logged in to the repeater (the repeater decrypts using the ACL-stored
/// shared secret it cached during `handleLoginReq`). Guests also receive a
/// reply in the current C++ reference (`simple_repeater/MyMesh.cpp`
/// around line 219: "guests can also access this now"), so this works
/// regardless of admin bit.
///
/// Plaintext layout produced here (13 bytes, matches C++ `sendRequest`):
/// ```text
/// [tag:4 LE][req_type:1 = 0x01][reserved:4 zeros][random:4 zeros]
/// ```
///
/// `txt_msg::encrypt` encodes the flags byte as `(attempt & 3) | (txt_type <<
/// 2)`. To get flags = `0x01`, we pass `attempt = 1`, `txt_type = 0`. The rest
/// of the plaintext is the 8-byte `text` field (reserved + random, zeros
/// acceptable).
async fn send_admin_status_request(
    lora: &mut SimpleLoRa<'_>,
    req: crate::TxAdminStatusReq,
    identity: &DeviceIdentity,
) {
    use meshcore::packet::PayloadType;

    const REQ_TYPE_GET_STATUS: u8 = 0x01;
    let text: [u8; 8] = [0u8; 8];

    crate::LAST_REQ_TARGET.lock(|cell| cell.set(Some(req.pub_key)));

    let Some((payload_vec, _)) = encrypt_txt_payload(
        "send_admin_status_req",
        identity,
        &req.pub_key,
        req.tag,
        0,
        REQ_TYPE_GET_STATUS,
        &text,
    ) else {
        return;
    };

    let contact = contacts::ContactStore::new()
        .find_by_key(&req.pub_key)
        .await;

    let route = route_for_contact(contact.as_ref(), PayloadType::Req, &payload_vec);
    let Some(frame) = build_frame(
        "send_admin_status_req",
        PayloadType::Req,
        route,
        payload_vec,
    ) else {
        return;
    };
    crate::PENDING_ADMIN_STATUS_TAG.set(req.tag);
    if send_frame(lora, "send_admin_status_req", &frame).await {
        let len = frame.len;
        defmt::info!(
            "Admin status req sent to {=[u8]:02x} tag={=u32:#010x} ({=usize}B) frame={=[u8]:02x}",
            &req.pub_key[..6],
            req.tag,
            len,
            frame.bytes(),
        );
    } else {
        crate::PENDING_ADMIN_STATUS_TAG.clear();
    }
}

// ---------------------------------------------------------------------------
// Generic binary request transmission
// ---------------------------------------------------------------------------

/// Build and transmit an unauthenticated `PAYLOAD_TYPE_ANON_REQ` —
/// the companion protocol's `SEND_ANON_REQ` (0x39 / 57) pipe, which the app
/// uses for **Discover Regions** (`ANON_REQ_TYPE_REGIONS = 0x01`).
///
/// `req.data[0]` is the `ANON_REQ_TYPE_*` discriminant. Plaintext layout:
/// ```text
/// [tag:4 LE][anon_req_type:1][params...]
/// ```
/// matching C++ `BaseChatMesh::sendAnonReq()`, whose `createAnonDatagram` body
/// is `[tag:4][data...]`. No login is required — the repeater derives the
/// shared secret from the sender identity carried in the packet.
async fn send_anon_request(
    lora: &mut SimpleLoRa<'_>,
    req: crate::TxAnonReq,
    identity: &DeviceIdentity,
) {
    use meshcore::packet::{PayloadType, RouteType};

    if req.data.is_empty() {
        defmt::warn!("send_anon_req: empty data — dropping");
        return;
    }

    crate::LAST_REQ_TARGET.lock(|cell| cell.set(Some(req.pub_key)));

    // ANON_REQ payload: `[dest_hash:1][sender_pk:32][mac:2][AES(tag:4 ‖ body)]`;
    // the repeater reads the tag as its `timestamp` and switches on the first
    // body byte. Built here rather than through `anon_req::encrypt`, whose
    // plaintext slot only fits a 15-byte login password — a request carrying
    // a multi-hop reply path needs up to `MAX_ANON_REQ_DATA`.
    let secret = match meshcore::identity::derive_shared_secret(&identity.sec_key, &req.pub_key) {
        Ok(s) => s,
        Err(e) => {
            defmt::warn!(
                "send_anon_req: key exchange failed: {:?}",
                defmt::Debug2Format(&e)
            );
            return;
        }
    };
    let mut aes_key = [0u8; meshcore::CIPHER_KEY_SIZE];
    aes_key.copy_from_slice(&secret[..meshcore::CIPHER_KEY_SIZE]);
    let plain_len = 4 + req.data.len();
    let mut plain = [0u8; 4 + crate::MAX_ANON_REQ_DATA];
    plain[..4].copy_from_slice(&req.tag.to_le_bytes());
    plain[4..plain_len].copy_from_slice(&req.data);
    let mut cipher = [0u8; 4 + crate::MAX_ANON_REQ_DATA];
    let (cipher_len, mac) = match meshcore::crypto::encrypt_then_mac(
        &aes_key,
        &secret,
        &plain[..plain_len],
        &mut cipher,
    ) {
        Ok(r) => r,
        Err(e) => {
            defmt::warn!(
                "send_anon_req: encrypt failed: {:?}",
                defmt::Debug2Format(&e)
            );
            return;
        }
    };

    let mut payload_vec: heapless::Vec<u8, { meshcore::MAX_PAYLOAD_SIZE }> = heapless::Vec::new();
    let _ = payload_vec.push(req.pub_key[0]);
    let _ = payload_vec.extend_from_slice(&identity.pub_key);
    let _ = payload_vec.extend_from_slice(&mac);
    let _ = payload_vec.extend_from_slice(&cipher[..cipher_len]);

    // Route like the reference: a stored path goes direct, a contact without
    // one floods, and a non-contact (FIRMWARE_VER_CODE 13 transient contact,
    // `out_path_len = 0`) goes zero-hop direct. (This is the one request that
    // does not flood for an unknown contact.)
    let contact = contacts::ContactStore::new().find_by_key(&req.pub_key).await;
    let route = match contact {
        None => Route::direct(0, &[]),
        Some(ref c) => route_for_contact(Some(c), PayloadType::AnonReq, &payload_vec),
    };

    // The repeater gates ANON_REQ_TYPE_REGIONS (and OWNER/BASIC) on
    // `packet->isRouteDirect()` — a flooded request is silently ignored, which
    // looks identical to "discovery returns nothing". Say so rather than let it
    // fail quietly; the fix is to establish a path to the repeater first.
    if route.route != RouteType::Direct {
        defmt::warn!(
            "send_anon_req: no path to {=[u8]:02x} — sending FLOOD, but the repeater \
             only answers anon type {=u8:#04x} on a DIRECT route; expect no reply",
            &req.pub_key[..6],
            req.data[0],
        );
    }

    let direct = route.route == RouteType::Direct;
    let Some(frame) = build_frame("send_anon_req", PayloadType::AnonReq, route, payload_vec) else {
        return;
    };
    // The reply comes back as a plain RESPONSE echoing our tag, which is
    // the same shape a binary request gets — so reuse that pending slot
    // and the app sees the reference's PUSH_CODE_BINARY_RESPONSE (0x8C).
    crate::PENDING_BINARY_REQ_TAG.set(req.tag);
    if send_frame(lora, "send_anon_req", &frame).await {
        let len = frame.len;
        defmt::info!(
            "Anon req sent to {=[u8]:02x} tag={=u32:#010x} anon_type={=u8:#04x} direct={=bool} ({=usize}B)",
            &req.pub_key[..6],
            req.tag,
            req.data[0],
            direct,
            len,
        );
    } else {
        crate::PENDING_BINARY_REQ_TAG.clear();
    }
}

/// Build and transmit a generic `PAYLOAD_TYPE_REQ` with an opaque body —
/// used by the companion protocol's `SEND_BINARY_REQ` (0x32) pipe for
/// `REQ_TYPE_GET_NEIGHBOURS`, `REQ_TYPE_GET_ACCESS_LIST`,
/// `REQ_TYPE_GET_OWNER_INFO`, and any future admin request type.
///
/// `req.req_data[0]` is the `REQ_TYPE_*` discriminant; the remaining bytes
/// are request-type-specific parameters. The plaintext layout produced is:
/// ```text
/// [tag:4 LE][req_type:1][params...]
/// ```
/// which matches the C++ `BaseChatMesh::sendRequest(req_data, req_data_len)`
/// path. `req_type` is stuffed into the txt_msg `flags` byte by decomposing
/// it into `(attempt, txt_type)` such that `(attempt & 3) | (txt_type << 2) ==
/// req_type`.
///
/// Unlike [`send_anon_request`] this requires the caller to be logged in to the
/// target, so the shared secret is already in its ACL cache.
async fn send_binary_request(
    lora: &mut SimpleLoRa<'_>,
    req: crate::TxBinaryReq,
    identity: &DeviceIdentity,
) {
    use meshcore::packet::PayloadType;

    if req.req_data.is_empty() {
        defmt::warn!("send_binary_req: empty req_data — dropping");
        return;
    }

    crate::LAST_REQ_TARGET.lock(|cell| cell.set(Some(req.pub_key)));

    let req_type = req.req_data[0];
    let params = &req.req_data[1..];
    let attempt = req_type & 0x03;
    let txt_type = req_type >> 2;

    let Some((payload_vec, _)) = encrypt_txt_payload(
        "send_binary_req",
        identity,
        &req.pub_key,
        req.tag,
        txt_type,
        attempt,
        params,
    ) else {
        return;
    };

    let contact = contacts::ContactStore::new()
        .find_by_key(&req.pub_key)
        .await;

    let route = route_for_contact(contact.as_ref(), PayloadType::Req, &payload_vec);
    let Some(frame) = build_frame("send_binary_req", PayloadType::Req, route, payload_vec) else {
        return;
    };
    crate::PENDING_BINARY_REQ_TAG.set(req.tag);
    if send_frame(lora, "send_binary_req", &frame).await {
        let len = frame.len;
        defmt::info!(
            "Binary req sent to {=[u8]:02x} tag={=u32:#010x} req_type={=u8:#04x} params={=usize}B ({=usize}B) frame={=[u8]:02x}",
            &req.pub_key[..6],
            req.tag,
            req_type,
            params.len(),
            len,
            frame.bytes(),
        );
    } else {
        crate::PENDING_BINARY_REQ_TAG.clear();
    }
}

// ---------------------------------------------------------------------------
// Path-discovery request transmission
// ---------------------------------------------------------------------------

/// Build and transmit a `PAYLOAD_TYPE_REQ` path-discovery request.
///
/// This forces a flood regardless of any stored path so the mesh network can
/// discover repeaters between us and the target.  The C++ equivalent is
/// `BaseChatMesh::sendRequest` called with `REQ_TYPE_GET_TELEMETRY_DATA` (0x03)
/// and permission `~TELEM_PERM_BASE` (0xFE) to signal a path-discovery instead
/// of a telemetry pull.
///
/// Plaintext layout (same as `sendRequest` multi-byte form):
/// ```text
/// [tag:4 LE][req_type:1 = 0x03][~perm:1 = 0xFE][0:3 reserved][random:4 zeros]
/// ```
async fn send_discovery_request(
    lora: &mut SimpleLoRa<'_>,
    req: crate::TxDiscoveryReq,
    identity: &DeviceIdentity,
) {
    use meshcore::packet::PayloadType;

    crate::LAST_REQ_TARGET.lock(|cell| cell.set(Some(req.pub_key)));

    // Discovery uses flags = 0x03 (REQ_TYPE_GET_TELEMETRY_DATA), same as telemetry,
    // but the text body starts with 0xFE (~TELEM_PERM_BASE) to signal discovery
    // intent. txt_msg::encrypt: flags = (attempt & 3) | (txt_type << 2) →
    // attempt=3, txt_type=0. text = [0xFE, 0, 0, 0, 0, 0, 0, 0] (perm byte + 7
    // padding/random zeros).
    const REQ_TYPE_DISCOVERY: u8 = 0x03;
    let text: [u8; 8] = [0xFE, 0, 0, 0, 0, 0, 0, 0];

    let Some((payload_vec, _)) = encrypt_txt_payload(
        "send_discovery_req",
        identity,
        &req.pub_key,
        req.tag,
        0,
        REQ_TYPE_DISCOVERY,
        &text,
    ) else {
        return;
    };


    // Always flood for discovery — the whole point is to find new paths.
    let route = Route::flood(PayloadType::Req, &payload_vec);
    let Some(frame) = build_frame("send_discovery_req", PayloadType::Req, route, payload_vec) else {
        return;
    };
    crate::PENDING_DISCOVERY_TAG.set(req.tag);
    if send_frame(lora, "send_discovery_req", &frame).await {
        let len = frame.len;
        defmt::info!(
            "Discovery req sent to {=[u8]:02x} tag={=u32:#010x} ({=usize}B) [flood]",
            &req.pub_key[..6],
            req.tag,
            len
        );
    } else {
        crate::PENDING_DISCOVERY_TAG.clear();
    }
}

// ---------------------------------------------------------------------------
// Trace-path receive handler
// ---------------------------------------------------------------------------

/// Handle a received `PayloadType::Trace` packet.
///
/// When a TRACE packet is reflected back to us by the relay, the packet
/// contains:
/// - `payload` = `[tag:4 LE][auth:4 LE][flags:1][route_hashes...]` — the
///   original route hashes embedded in the payload by `sendDirect`.
/// - `pkt_path` = `[snr_relay1, snr_relay2, ...]` — per-hop SNRs appended by
///   each relay that forwarded the packet.
/// - `snr_x4` = our receive SNR × 4 (the final hop SNR at our radio).
///
/// Pushes a [`crate::TraceResult`] to [`crate::TRACE_RESULT_CHANNEL`] so the
/// BLE task can send a `0x89 PUSH_CODE_TRACE_DATA` notification.
async fn handle_trace_recv(
    payload: &[u8],
    pkt_path: &heapless::Vec<u8, { meshcore::MAX_PATH_SIZE }>,
    snr_x4: i8,
) {
    if payload.len() < 9 {
        defmt::warn!("Trace recv: payload too short ({=usize}B)", payload.len());
        return;
    }

    let tag = u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
    let auth_code = u32::from_le_bytes([payload[4], payload[5], payload[6], payload[7]]);
    let flags = payload[8];

    // payload[9..] = route path hashes embedded by the sender.
    let route_hashes = &payload[9..];

    // path_len counts the route hashes in units of hash_size bytes.
    let path_sz = (flags & 0x03) as usize; // 0=1B, 1=2B, 2=4B per hash entry
    let hash_size = path_sz + 1;
    let path_len = (route_hashes.len() / hash_size.max(1)) as u8;

    // The per-hop relay SNRs come from the packet's path field (appended by each
    // relay).
    let relay_snrs = pkt_path.as_slice();

    defmt::info!(
        "Trace recv: tag={=u32:#010x} auth={=u32:#010x} flags={=u8:#04x} path_len={=u8} relay_snrs={=usize}B final_snr={=i8}",
        tag,
        auth_code,
        flags,
        path_len,
        relay_snrs.len(),
        snr_x4,
    );

    let mut path_hashes: heapless::Vec<u8, { meshcore::MAX_PATH_SIZE }> = heapless::Vec::new();
    let mut path_snrs: heapless::Vec<u8, { meshcore::MAX_PATH_SIZE }> = heapless::Vec::new();
    let _ = path_hashes.extend_from_slice(route_hashes);
    let _ = path_snrs.extend_from_slice(relay_snrs);

    let _ = crate::TRACE_RESULT_CHANNEL.try_send(crate::TraceResult {
        path_len,
        flags,
        tag,
        auth_code,
        path_hashes,
        path_snrs,
        final_snr: snr_x4,
    });
}

// ---------------------------------------------------------------------------
// Login response receive handler
// ---------------------------------------------------------------------------

/// Handle a received `PayloadType::Response` packet.
///
/// Attempts to decrypt the response using each stored contact's public key.
/// A successful decryption where `decrypted[4]` matches `NODE_TYPE_RESP_OK`
/// is treated as a login success and pushes a [`crate::LoginResult`] with
/// `success = true`; otherwise `success = false` is pushed.
///
/// Wire format (same as TxtMsg):
/// `[dest_hash:1][src_hash:1][mac:2][aes_ciphertext]`
///
/// Decrypted plaintext: `[timestamp:4][resp_type:1][keep_alive_secs:2][...]`
async fn handle_response_recv(payload: &[u8], identity: &DeviceIdentity) {
    use meshcore::payload::txt_msg;

    let msg = match txt_msg::deserialize(payload) {
        Ok(m) => m,
        Err(_) => {
            defmt::debug!("Response recv: payload deserialize failed");
            return;
        }
    };

    // Only process responses addressed to us.
    if msg.dest_hash != identity.pub_key[0] {
        defmt::debug!(
            "Response recv: dest_hash={=u8:#04x} not ours, ignoring",
            msg.dest_hash
        );
        return;
    }

    // Fast path: decrypt directly using the most recently hinted target's
    // pub_key — no Contact lookup required. This mirrors the reference
    // client's "pending request target" slot so login / status / telemetry
    // responses decrypt even when the target isn't (yet) in the contact
    // store, e.g. first login after a wipe or before the first advert.
    // The hint is tried without a contact lookup; the rest of the hash bucket
    // covers unsolicited responses from peers we didn't just request anything
    // from.
    let store = contacts::ContactStore::new();
    let mut candidates = SenderCandidates::new(msg.src_hash);
    while let Some(pk) = candidates.next_pub_key(&store).await {
        if try_dispatch_response_by_pk(&pk, &msg, identity).await.is_ok() {
            return;
        }
    }

    defmt::debug!("Response recv: could not decrypt with any known contact");
}

/// Verify MAC, decrypt and dispatch a `PayloadType::Response` packet using
/// only the sender's 32-byte pub_key — no stored Contact required.
///
/// Mirrors the reference client's "pending request target" fast path: when we
/// just sent an ANON_REQ / login / status / telemetry request we already know
/// the target's full pub_key, and the shared secret is derived purely from
/// X25519(our_sk, sender_pk). Gating decryption on a persisted Contact would
/// drop login responses whenever the repeater isn't yet in the contact store
/// (e.g. first login after a wipe, or before the first advert arrives).
///
/// Returns `Ok(())` if the MAC verifies and the body is dispatched to one of
/// the pending-request branches (login included) or, when nothing is pending,
/// forwarded to the app as an unsolicited binary response. Returns
/// `Err(meshcore::Error::MacMismatch)` if this isn't the right peer so the
/// fallback scan can try the next candidate.
async fn try_dispatch_response_by_pk(
    sender_pk: &[u8; meshcore::PUB_KEY_SIZE],
    msg: &meshcore::payload::txt_msg::TxtMsg,
    identity: &DeviceIdentity,
) -> Result<(), meshcore::Error> {
    use meshcore::payload::txt_msg;

    txt_msg::verify_mac(&identity.sec_key, sender_pk, msg)?;
    let (dec, _ack_hash) = txt_msg::decrypt(&identity.sec_key, sender_pk, msg)?;

    // Decrypted plaintext layout (same as C++ onContactResponse `data` arg):
    //   [0..4]  = tag / timestamp (u32 LE)      → dec.timestamp
    //   [4]     = resp_type                      → dec.flags
    //
    // For a LOGIN response:
    //   [5]     = keep_alive_secs / 16           → dec.text[0]
    //   [6]     = is_admin                       → dec.text[1]
    //   [7]     = acl_perms                      → dec.text[2]
    //   [12]    = fw_ver_level                   → dec.text[7]
    //   RESP_SERVER_LOGIN_OK = 0 (new), legacy = b'O'/b'K'
    //
    // For a STATUS PONG response (AnonReq with empty password):
    //   The server's resp_type value for status is not firmly documented;
    //   we distinguish via the PENDING_STATUS_PUBKEY flag set before TX.
    //   dec.text[0..4] = uptime_secs (u32 LE), dec.text[4..6] = battery_mv (u16 LE)
    // In the Response payload, dec.flags = resp_type byte, dec.text = body after
    // flags.
    let resp_type = dec.flags;
    let mut pub_key = [0u8; meshcore::PUB_KEY_SIZE];
    pub_key.copy_from_slice(sender_pk);

    // Pending anonymous status ping (legacy ANON_REQ-with-empty-password path).
    let pending_status = crate::PENDING_STATUS_PUBKEY.lock(|cell| cell.get());
    if let Some(pending_key) = pending_status
        && pending_key == pub_key
    {
        crate::PENDING_STATUS_PUBKEY.lock(|cell| cell.set(None));

        // Status pong body: [uptime:4 LE][battery_mv:2 LE]
        let uptime_secs = if dec.text.len() >= 4 {
            u32::from_le_bytes([
                dec.text.first().copied().unwrap_or(0),
                dec.text.get(1).copied().unwrap_or(0),
                dec.text.get(2).copied().unwrap_or(0),
                dec.text.get(3).copied().unwrap_or(0),
            ])
        } else {
            0
        };
        let battery_mv = if dec.text.len() >= 6 {
            u16::from_le_bytes([
                dec.text.get(4).copied().unwrap_or(0),
                dec.text.get(5).copied().unwrap_or(0),
            ])
        } else {
            0
        };

        defmt::info!(
            "Response recv: STATUS from {=[u8]:02x} resp_type={=u8} uptime={=u32}s batt={=u16}mV",
            &sender_pk[..6],
            resp_type,
            uptime_secs,
            battery_mv,
        );

        // Build a synthetic RepeaterStats blob with only the two fields the
        // anonymous ping carries; the phone parses the same wire format as
        // for the authenticated GET_STATUS reply.
        let mut stats = [0u8; 56];
        stats[0..2].copy_from_slice(&battery_mv.to_le_bytes()); // batt_milli_volts
        stats[20..24].copy_from_slice(&uptime_secs.to_le_bytes()); // total_up_time_secs

        let _ = crate::STATUS_RESULT_CHANNEL.try_send(crate::StatusResult { pub_key, stats });
        return Ok(());
    }

    // Everything below is the pending-request ladder shared with the
    // Path-wrapped form (see `dispatch_response_plain`). The plaintext body
    // starts at plaintext[4]; `txt_msg::decrypt` splits it into `dec.flags`
    // (first body byte) and `dec.text` (the rest, trailing zeros stripped).
    //
    // `trimmed` is that stripped view (flags ++ text): admin status, telemetry
    // and login read it. The generic binary response needs the body up to the
    // AES block boundary, so neighbours/ACL entries that happen to end in zero
    // bytes aren't truncated: msg.data is the raw ciphertext, a multiple of
    // the AES block size, and body length = total plaintext - timestamp(4).
    let body_len = msg.data.len().saturating_sub(4);
    let mut trimmed: heapless::Vec<u8, { crate::MAX_BINARY_RESP_BODY }> = heapless::Vec::new();
    let _ = trimmed.push(dec.flags);
    let _ = trimmed.extend_from_slice(&dec.text);
    let mut body: heapless::Vec<u8, { crate::MAX_BINARY_RESP_BODY }> = heapless::Vec::new();
    if body_len > 0 {
        let _ = body.push(dec.flags);
        let tail_need = body_len - 1;
        let tail_copy = dec.text.len().min(tail_need);
        let _ = body.extend_from_slice(&dec.text[..tail_copy]);
        // Zero-pad the rest so the block-aligned length is preserved.
        while body.len() < body_len.min(crate::MAX_BINARY_RESP_BODY) {
            let _ = body.push(0);
        }
    }

    dispatch_response_plain(sender_pk, dec.timestamp, &body, &trimmed, None, "Response recv");
    Ok(())
}

/// Path-specific context for [`dispatch_response_plain`]: a Path-wrapped
/// response also carries the route the server used to reach us.
struct PathCtx<'a> {
    path_len_byte: u8,
    path: &'a [u8],
}

/// Match a decrypted response (`tag` + `body`) against the pending requests
/// and forward the result to the matching channel. Shared by plain
/// `Response` packets and `Path`-wrapped responses (`extra_type == 1`).
///
/// Ladder, in order: admin status, telemetry, path discovery (Path form
/// only), login, then everything else as a generic binary response (solicited
/// when `PENDING_BINARY_REQ_TAG` matches, unsolicited otherwise).
///
/// `body` is the response body verbatim (the Response form re-pads it to the
/// AES block boundary, the Path form already keeps its trailing zeros).
/// `trimmed` is the same body without trailing zero bytes, which is what the
/// Response form always had on hand for admin status, telemetry and login;
/// the Path form passes `body` for both. `src` prefixes the log lines.
fn dispatch_response_plain(
    sender_pk: &[u8; meshcore::PUB_KEY_SIZE],
    tag: u32,
    body: &[u8],
    trimmed: &[u8],
    path: Option<PathCtx<'_>>,
    src: &'static str,
) {
    let mut pub_key = [0u8; meshcore::PUB_KEY_SIZE];
    pub_key.copy_from_slice(sender_pk);

    // Pending admin status request (REQ_TYPE_GET_STATUS, tag-based match).
    // Body is the 56-byte RepeaterStats blob, zero-padded or truncated to fit.
    if crate::PENDING_ADMIN_STATUS_TAG.take_if_match(tag) {
        let mut stats = [0u8; 56];
        let n = trimmed.len().min(56);
        stats[..n].copy_from_slice(&trimmed[..n]);

        let result = parse_repeater_stats(&stats, pub_key, tag);

        defmt::info!(
            "{=str}: ADMIN_STATUS from {=[u8]:02x} tag={=u32:#010x} up={=u32}s batt={=u16}mV queue={=u16} rssi={=i16} snrX4={=i16} recv={=u32} sent={=u32}",
            src,
            &sender_pk[..6],
            result.tag,
            result.total_up_time_secs,
            result.batt_milli_volts,
            result.curr_tx_queue_len,
            result.last_rssi,
            result.last_snr_x4,
            result.n_packets_recv,
            result.n_packets_sent,
        );

        let _ = crate::ADMIN_STATUS_RESULT_CHANNEL.try_send(result);

        // Forward the raw stats blob to BLE for PUSH_CODE_STATUS_RESPONSE.
        let _ = crate::STATUS_RESULT_CHANNEL.try_send(crate::StatusResult { pub_key, stats });
        return;
    }

    // Pending telemetry request (tag-based match). Body is CayenneLPP.
    if crate::PENDING_TELEM_TAG.take_if_match(tag) {
        let mut lpp: heapless::Vec<u8, 176> = heapless::Vec::new();
        let n = trimmed.len().min(176);
        let _ = lpp.extend_from_slice(&trimmed[..n]);

        defmt::info!(
            "{=str}: TELEM from {=[u8]:02x} tag={=u32:#010x} lpp={=usize}B",
            src,
            &sender_pk[..6],
            tag,
            lpp.len(),
        );

        let _ = crate::TELEM_RESULT_CHANNEL.try_send(crate::TelemResult { pub_key, lpp });
        return;
    }

    // Pending path-discovery request (tag-based match). Only a Path-wrapped
    // response carries the discovered route.
    if let Some(ctx) = &path
        && crate::PENDING_DISCOVERY_TAG.take_if_match(tag)
    {
        let mut out_path: heapless::Vec<u8, { meshcore::MAX_PATH_SIZE }> = heapless::Vec::new();
        let _ = out_path.extend_from_slice(ctx.path);
        let in_path: heapless::Vec<u8, { meshcore::MAX_PATH_SIZE }> = heapless::Vec::new();

        defmt::info!(
            "{=str}: DISCOVERY response from {=[u8]:02x} tag={=u32:#010x} out_path_len={=u8}",
            src,
            &sender_pk[..6],
            tag,
            ctx.path_len_byte,
        );

        let _ = crate::DISCOVERY_RESULT_CHANNEL.try_send(crate::DiscoveryResult {
            pub_key,
            out_path_len_byte: ctx.path_len_byte,
            out_path,
            in_path_len_byte: 0xFF,
            in_path,
        });
        return;
    }

    // Pending login to this contact (upstream `pending_login`, matched on the
    // pub-key prefix). The MAC verified, so this contact is the right peer.
    if crate::PENDING_LOGIN.take_if_match(crate::login_tag(&pub_key)) {
        // Path extras keep their trailing zeros, so a short one is malformed.
        // Response bodies are stripped and are read leniently below.
        if path.is_some() && body.len() + 4 < meshcore::payload::path_msg::LOGIN_RESPONSE_EXTRA_LEN {
            warn_login_too_short(body.len() + 4);
            return;
        }

        // Body layout (same as the plaintext seen by C++ `onContactResponse`
        // after the 4-byte tag):
        //   [0] resp_type  [1] keep_alive/16 (b'K' in the legacy reply)
        //   [2] is_admin   [3] acl_perms     [4..8] random   [8] fw_ver_level
        let at = |i: usize| trimmed.get(i).copied().unwrap_or(0);
        let resp_type = at(0);
        // RESP_SERVER_LOGIN_OK = 0 (new format); legacy is b'O' followed by b'K'.
        let success = resp_type == 0 || (resp_type == b'O' && trimmed.get(1) == Some(&b'K'));

        defmt::info!(
            "{=str}: login {} from {=[u8]:02x} resp_type={=u8} tag={=u32:#010x}",
            src,
            if success { "OK" } else { "FAIL" },
            &sender_pk[..6],
            resp_type,
            tag,
        );

        let _ = crate::LOGIN_RESULT_CHANNEL.try_send(crate::LoginResult {
            success,
            is_admin: if success { at(2) } else { 0 },
            pub_key,
            tag,
            acl_perms: if success { at(3) } else { 0 },
            fw_ver_level: if success { at(8) } else { 0 },
        });
        return;
    }

    // Pending generic binary request (CMD_SEND_BINARY_REQ), or any response
    // nobody is waiting for (e.g. a sensor's unsolicited telemetry push):
    // forward it to the app as PUSH_CODE_BINARY_RESPONSE, like upstream 1.18
    // `onContactResponse`, instead of mistaking it for a login reply.
    let solicited = crate::PENDING_BINARY_REQ_TAG.take_if_match(tag);
    let mut out: heapless::Vec<u8, { crate::MAX_BINARY_RESP_BODY }> = heapless::Vec::new();
    let n = body.len().min(crate::MAX_BINARY_RESP_BODY);
    let _ = out.extend_from_slice(&body[..n]);

    defmt::info!(
        "{=str}: BINARY from {=[u8]:02x} tag={=u32:#010x} body={=usize}B solicited={=bool}",
        src,
        &sender_pk[..6],
        tag,
        out.len(),
        solicited,
    );

    let _ = crate::BINARY_RESULT_CHANNEL.try_send(crate::BinaryResult {
        pub_key,
        tag,
        body: out,
    });
}

/// A Path-wrapped login response whose extra bytes are too short to read.
fn warn_login_too_short(len: usize) {
    defmt::warn!(
        "Path login response: extra too short ({=usize}B, need {=usize}B)",
        len,
        meshcore::payload::path_msg::LOGIN_RESPONSE_EXTRA_LEN,
    );
}

// ---------------------------------------------------------------------------
// Path receive handler
// ---------------------------------------------------------------------------

/// Handle a received `PayloadType::Path` packet (type=8).
///
/// These arrive when a server sends its login response via `createPathReturn`
/// (typically flood-routed).  The packet wraps an extra payload whose type
/// indicates what is inside; `extra_type == 1` (PAYLOAD_TYPE_RESPONSE) means
/// it is a login response.
///
/// Wire format: `[dest_hash:1][src_hash:1][mac:2][AES-128-ECB ciphertext]`
///
/// Decrypted data: `[path_len_byte:1][path_bytes:N][extra_type:1][extra...]`
async fn handle_path_recv(payload: &[u8], rssi: i16, identity: &DeviceIdentity) {
    use meshcore::payload::path_msg;

    let msg = match path_msg::deserialize(payload) {
        Ok(m) => m,
        Err(_) => {
            defmt::warn!(
                "Path recv: failed to parse payload ({=usize}B)",
                payload.len()
            );
            return;
        }
    };

    // Quick check: is this addressed to us?
    if msg.dest_hash != identity.pub_key[0] {
        defmt::debug!(
            "Path recv: dest_hash={=u8:#04x} not ours ({=u8:#04x}), ignoring",
            msg.dest_hash,
            identity.pub_key[0],
        );
        return;
    }

    defmt::info!(
        "Path recv: dest={=u8:#04x} src={=u8:#04x} [{=i16}dBm]",
        msg.dest_hash,
        msg.src_hash,
        rssi,
    );

    // Fast path: decrypt directly using the most recently hinted target's
    // pub_key — no Contact lookup required. Lets Path-wrapped login /
    // admin-status / discovery responses decrypt even when the target isn't
    // (yet) in the contact store.
    // The rest of the hash bucket covers unsolicited Path returns from peers
    // we didn't just request anything from.
    let store = contacts::ContactStore::new();
    let mut candidates = SenderCandidates::new(msg.src_hash);
    while let Some(pk) = candidates.next_pub_key(&store).await {
        if try_dispatch_path_by_pk(&pk, &msg, rssi, &store, identity)
            .await
            .is_ok()
        {
            return;
        }
    }

    defmt::debug!(
        "Path recv: no matching contact for src_hash={=u8:#04x}",
        msg.src_hash
    );
}

/// Verify, decrypt and dispatch a `PayloadType::Path` packet using only the
/// sender's 32-byte pub_key — no stored Contact required.
///
/// Mirrors the reference client's "pending request target" fast path so that
/// login / admin-status / discovery / telemetry Path responses decrypt even
/// when the target isn't (yet) in the contact store. The embedded path update
/// is still attempted via [`ContactStore::update_path`], which is a no-op
/// when the contact doesn't exist — so zero-hop-known peers still get their
/// path cached here, and unknown peers silently skip the update.
async fn try_dispatch_path_by_pk(
    sender_pk: &[u8; meshcore::PUB_KEY_SIZE],
    msg: &meshcore::payload::path_msg::PathMsg,
    rssi: i16,
    store: &contacts::ContactStore,
    identity: &DeviceIdentity,
) -> Result<(), meshcore::Error> {
    use meshcore::payload::path_msg;

    let dec = path_msg::verify_and_decrypt(&identity.sec_key, sender_pk, msg)?;

    defmt::info!(
        "Path recv: decrypted from {=[u8]:02x} path_len={=u8} extra_type={=u8} extra_len={=usize}",
        &sender_pk[..6],
        dec.path_len_byte,
        dec.extra_type,
        dec.extra.len(),
    );

    // Update the stored routing path for this contact. Accept zero-hop paths
    // (direct neighbour, `dec.path_len_byte == 0`) as well — the previous
    // `!dec.path.is_empty()` gate incorrectly skipped those, leaving the
    // contact's out_path at `OUT_PATH_UNKNOWN` forever and forcing every
    // subsequent TX to flood. When the peer isn't in the contact store
    // `update_path` is a no-op (returns `Ok(false)`), which is the desired
    // behaviour for the pub-key-only fast path.
    if dec.path_len_byte != contacts::OUT_PATH_UNKNOWN {
        let mut path_buf = [0u8; contacts::MAX_PATH_SIZE];
        let copy = dec.path.len().min(contacts::MAX_PATH_SIZE);
        path_buf[..copy].copy_from_slice(&dec.path[..copy]);
        match store
            .update_path(sender_pk, dec.path_len_byte, &path_buf)
            .await
        {
            Ok(true) => {
                let _ = crate::PATH_UPDATED_CHANNEL.try_send(*sender_pk);
            }
            Ok(false) => {}
            Err(e) => defmt::warn!("Path recv: path update failed: {:?}", e),
        }
    }

    // PAYLOAD_TYPE_ACK = 3 → ACK piggybacked on a Path response.
    if dec.extra_type == 3 && dec.extra.len() >= 4 {
        let ack_crc = u32::from_le_bytes([dec.extra[0], dec.extra[1], dec.extra[2], dec.extra[3]]);
        defmt::info!(
            "Path recv: ACK from {=[u8]:02x} ack_crc={=u32:#010x}",
            &sender_pk[..6],
            ack_crc,
        );
        handle_ack_recv(&dec.extra[..4], rssi);
        return Ok(());
    }

    // PAYLOAD_TYPE_RESPONSE = 1 → admin-status / telemetry / discovery / login
    // / other binary response. Extra layout: `[tag:4 LE][body...]`; path
    // extras preserve trailing zero bytes, so the body is used verbatim.
    if dec.extra_type == 1 {
        if dec.extra.len() >= 4 {
            let tag = u32::from_le_bytes([dec.extra[0], dec.extra[1], dec.extra[2], dec.extra[3]]);
            let body = &dec.extra[4..];
            dispatch_response_plain(
                sender_pk,
                tag,
                body,
                body,
                Some(PathCtx {
                    path_len_byte: dec.path_len_byte,
                    path: &dec.path,
                }),
                "Path recv",
            );
        } else if crate::PENDING_LOGIN.take_if_match(crate::login_tag(sender_pk)) {
            // Too short for a tag, so only a (malformed) login can match.
            warn_login_too_short(dec.extra.len());
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// PAYLOAD_TYPE_REQ receive: on-air requests from peers
// ---------------------------------------------------------------------------
//
// Incoming `Req` packets are AES-encrypted under a per-contact shared
// secret (X25519 ECDH between this node's secret key and the sender's
// public key).  After verify_mac + decrypt the plaintext is:
//
//     [timestamp:4 LE][req_type:1][req_data: variable]
//
// Currently only `REQ_TYPE_GET_TELEMETRY_DATA` is honoured; other
// request types (get_status, neighbours, access_list, …) are repeater
// / room-server features and not relevant to the badge.

/// CayenneLPP voltage type discriminator: 2-byte big-endian, value × 100.
const LPP_VOLTAGE: u8 = 116;
/// CayenneLPP channel used for the badge's own telemetry (matches upstream
/// `TELEM_CHANNEL_SELF`).
const TELEM_CHANNEL_SELF: u8 = 1;
/// Permission bit covering "base" telemetry (battery + uptime); upstream
/// `TELEM_PERM_BASE`.
const TELEM_PERM_BASE: u8 = 0x01;
/// Bit in `Contact.flags` (after `>> 1`) that grants this contact the
/// `TELEM_PERM_BASE` permission when `telemetry_mode_base == ALLOW_FLAGS`.
const CONTACT_FLAG_TELEM_BASE: u8 = 0x02;

async fn handle_req_recv(
    lora: &mut SimpleLoRa<'_>,
    payload: &[u8],
    identity: &DeviceIdentity,
) {
    use meshcore::payload::req;

    let msg = match req::deserialize(payload) {
        Ok(m) => m,
        Err(_) => {
            defmt::debug!("Req recv: failed to parse envelope");
            return;
        }
    };

    if msg.dest_hash != identity.pub_key[0] {
        // Not addressed to us — drop quietly (no MAC computation cost).
        return;
    }

    let store = contacts::ContactStore::new();
    // No hint here: a request is never an answer to something we sent.
    let mut candidates = SenderCandidates::without_hint(msg.src_hash);
    while let Some(c) = candidates.next_contact(&store).await {
        if req::verify_mac(&identity.sec_key, &c.pub_key, &msg).is_err() {
            continue;
        }
        let dec = match req::decrypt(&identity.sec_key, &c.pub_key, &msg) {
            Ok(d) => d,
            Err(_) => return,
        };
        match dec.req_type {
            req::REQ_TYPE_GET_TELEMETRY_DATA => {
                handle_telem_request_recv(
                    lora, identity, &c, dec.timestamp, &dec.data,
                )
                .await;
            }
            other => {
                defmt::debug!(
                    "Req recv: unhandled type 0x{=u8:02x} from {=[u8]:02x}",
                    other,
                    &c.pub_key[..6],
                );
            }
        }
        return;
    }

    defmt::debug!(
        "Req recv: no contact match for src_hash 0x{=u8:02x}",
        msg.src_hash,
    );
}

async fn handle_telem_request_recv(
    lora: &mut SimpleLoRa<'_>,
    identity: &DeviceIdentity,
    sender: &contacts::Contact,
    sender_timestamp: u32,
    req_data: &[u8],
) {
    use meshcore::MAX_PAYLOAD_SIZE;
    use meshcore::packet::{PayloadType, RouteType};
    use meshcore::payload::response;

    // Permission check — matches upstream `MyMesh::onContactRequest`.
    let mode = crate::TELEMETRY_MODE_BASE.load(core::sync::atomic::Ordering::Relaxed);
    let base_granted = match mode {
        0 => false,                                        // TELEM_MODE_DENY
        1 => (sender.flags & CONTACT_FLAG_TELEM_BASE) != 0, // ALLOW_FLAGS — bit 1
        _ => true,                                         // ALLOW_ALL
    };
    let inverse_mask = req_data.first().copied().unwrap_or(0);
    let permitted = base_granted && ((!inverse_mask) & TELEM_PERM_BASE) != 0;
    if !permitted {
        defmt::debug!(
            "Telem req: denied for {=[u8]:02x} (mode={=u8} mask=0x{=u8:02x})",
            &sender.pub_key[..6],
            mode,
            inverse_mask,
        );
        return;
    }

    // Build CayenneLPP voltage frame: [chan=1][type=0x74][value × 100 :u16 BE].
    let battery_mv = crate::fw::battery::read_mv() as u32;
    let v_x100 = ((battery_mv * 100) / 1000).min(u16::MAX as u32) as u16;
    let lpp = [
        TELEM_CHANNEL_SELF,
        LPP_VOLTAGE,
        (v_x100 >> 8) as u8,
        (v_x100 & 0xFF) as u8,
    ];

    let resp = match response::encrypt(
        &identity.sec_key,
        &identity.pub_key,
        &sender.pub_key,
        sender_timestamp,
        &lpp,
    ) {
        Ok(r) => r,
        Err(e) => {
            defmt::warn!(
                "Telem resp encrypt failed: {:?}",
                defmt::Debug2Format(&e),
            );
            return;
        }
    };

    let mut payload_buf = [0u8; MAX_PAYLOAD_SIZE];
    let mut payload_len = 0usize;
    if response::serialize(&resp, &mut payload_buf, &mut payload_len).is_err() {
        return;
    }

    let mut msg_payload: heapless::Vec<u8, MAX_PAYLOAD_SIZE> = heapless::Vec::new();
    let _ = msg_payload.extend_from_slice(&payload_buf[..payload_len]);

    // Route: prefer Direct with the sender's stored out_path; else flood.
    let route = route_for_contact(Some(sender), PayloadType::Response, &msg_payload);

    let direct = route.route == RouteType::Direct;
    let Some(_) = tx_packet(
        lora,
        "Telem resp",
        PayloadType::Response,
        route,
        msg_payload,
    )
    .await
    else {
        return;
    };
    defmt::info!(
        "Telem resp sent: {=u32}mV to {=[u8]:02x} via {=str}",
        battery_mv,
        &sender.pub_key[..6],
        if direct { "direct" } else { "flood" },
    );
}

// ---------------------------------------------------------------------------
// Control data transmission (CMD_SEND_CONTROL_DATA, 0x37)
// ---------------------------------------------------------------------------

/// Transmit a raw PAYLOAD_TYPE_CONTROL (0x0B) packet zero-hop (direct,
/// path_len=0).
///
/// The `req.payload` slice starts with the `ctl_type` byte (e.g. 0x80 =
/// CTL_TYPE_NODE_DISCOVER_REQ) and may carry additional bytes.  This mirrors
/// `Mesh::createControlData` + `sendZeroHop` from the reference C firmware.
async fn send_control_data(lora: &mut SimpleLoRa<'_>, req: crate::TxControlData) {
    use meshcore::packet::PayloadType;

    // The companion handler only queues non-empty payloads.
    let ctl_type = req.payload[0];
    let Some(frame) = tx_packet(
        lora,
        "send_control_data",
        PayloadType::Unknown(0x0B), // PAYLOAD_TYPE_CONTROL
        // Zero-hop: Direct with path_len_byte = 0.
        Route::direct(0, &[]),
        req.payload,
    ).await else {
        return;
    };
    let len = frame.len;
    defmt::info!(
        "send_control_data: sent {=usize}B ctl_type={=u8:#04x}",
        len,
        ctl_type,
    );
}
