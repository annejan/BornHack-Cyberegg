use core::cell::RefCell;

use embassy_nrf::gpio::AnyPin;
use embassy_nrf::{Peri, peripherals};
use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_time::Timer;
use meshcore::channel::hash_from_key;
use meshcore::contacts::Contacts;
use meshcore::dedup::{MsgHashRing, msg_hash};

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
            });
        }
    }
    v
}

pub(super) use crate::truncate_str as truncate_bytes;

fn update_cached_channels(loaded: &heapless::Vec<LoadedChannel, { channels::NUM_CHANNELS }>) {
    crate::CACHED_CHANNELS.lock(|cell| {
        let mut list = cell.borrow_mut();
        list.clear();
        for ch in loaded {
            let name_str = core::str::from_utf8(&ch.name)
                .unwrap_or("?")
                .trim_end_matches('\0');
            let mut name = heapless::String::new();
            let _ = name.push_str(truncate_bytes(name_str, 20));
            let _ = list.push(crate::CachedChannel {
                slot_idx: ch.slot_idx,
                name,
            });
        }
    });
}

static CONTACTS: Mutex<CriticalSectionRawMutex, RefCell<Contacts<20>>> =
    Mutex::new(RefCell::new(Contacts::new()));

static MSG_SEEN: Mutex<CriticalSectionRawMutex, RefCell<MsgHashRing<50>>> =
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
/// Radio parameters updated via the companion app take effect on the **next
/// reboot** — reinitialisation while in RX is not yet implemented.
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

        // Drain all queued TX requests before entering RX.
        while let Ok(req) = crate::TX_CHANNEL.try_receive() {
            dispatch_tx(&mut lora, &loaded_channels, identity, req).await;
        }

        // Race: receive the next LoRa packet OR a TX request arrives.
        // TX_WAKEUP breaks the receive_packet wait so the drain above
        // picks up the new request immediately.
        use embassy_futures::select::{Either, select};
        let rx_result = match select(lora.receive_packet(&mut raw), crate::TX_WAKEUP.wait()).await {
            Either::Second(()) => {
                crate::TX_WAKEUP.reset();
                continue;
            }
            Either::First(result) => result,
        };

        match rx_result {
            Ok(None) => { /* channel idle or CRC error — next CAD cycle on next loop */ }
            Err(e) => {
                defmt::warn!("receive_packet error: {:?}", e);
            }

            Ok(Some((len, rssi, snr_x4))) => {
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

                defmt::info!("RX [{=usize}B {=i16}dBm]: {=[u8]:02x}", len, rssi, frame,);

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

                        // Region filter: if we have a flood-scope key and this is a
                        // TransportFlood packet, verify the transport code.  A mismatch
                        // means the packet belongs to a different region — drop it silently.
                        if matches!(msg.route, RouteType::TransportFlood) {
                            let key = crate::FLOOD_SCOPE_KEY.lock(|c| c.get());
                            defmt::debug!(
                                "TransportFlood pkt: type={=u8} tc={=u16:#06x} key_set={=bool}",
                                msg.payload_type.to_u8(),
                                msg.transport_code,
                                key.is_some(),
                            );
                            if let Some(key) = key {
                                let expected = meshcore::packet::calc_transport_code(
                                    &key,
                                    msg.payload_type.to_u8(),
                                    &msg.payload,
                                );
                                defmt::debug!(
                                    "TransportFlood region check: got={=u16:#06x} expected={=u16:#06x}",
                                    msg.transport_code,
                                    expected,
                                );
                                if msg.transport_code != expected {
                                    defmt::info!(
                                        "TransportFlood region mismatch (got {=u16:#06x}, expected {=u16:#06x}) — dropped",
                                        msg.transport_code,
                                        expected
                                    );
                                    continue;
                                }
                            }
                            // No key set → wildcard: accept all TransportFlood
                            // packets.
                        }

                        // Client-repeat relay (if enabled).
                        if let Some(relay_req) = super::repeater::try_relay(&msg, identity) {
                            let _ = crate::tx_send(relay_req);
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
                                push_grp_txt(&msg.payload, rssi, snr_x4, path_len, &loaded_channels)
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
    snr_x4: i8,
    path_len: u8,
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
                    crate::fw::led::set_led(
                        &crate::fw::led::LED_RED,
                        crate::fw::led::LedState::BlinkOnce,
                    );
                }
                Some(b'g') | Some(b'G') => {
                    crate::fw::led::set_led(
                        &crate::fw::led::LED_GREEN,
                        crate::fw::led::LedState::BlinkOnce,
                    );
                }
                Some(b'b') | Some(b'B') => {
                    crate::fw::led::set_led(
                        &crate::fw::led::LED_BLUE,
                        crate::fw::led::LedState::BlinkOnce,
                    );
                }
                _ => {}
            }
            defmt::info!("blinkme: cmd={=u8:#04x}", cmd.unwrap_or(0));
        }
    }

    let ch = match channels.iter().find(|c| c.hash == grp.channel_hash) {
        Some(c) => c,
        None => {
            defmt::debug!(
                "MeshCore GrpTxt [hash={=u8} {=i16}dBm] no matching channel (have: {=[u8]})",
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

    if grp_txt::verify_mac(&ch.key, &grp).is_err() {
        defmt::warn!(
            "MeshCore GrpTxt [channel={=u8}] MAC mismatch on channel hash={=u8}",
            grp.channel_hash,
            ch.slot_idx,
        );
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
            let is_new = MSG_SEEN.lock(|cell| {
                let mut ring = cell.borrow_mut();
                if ring.contains(content_hash) {
                    false
                } else {
                    ring.insert(content_hash);
                    true
                }
            });
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

            // Token screen: if the message text starts with "token:" (after
            // stripping any "sender: " prefix), store the value.
            if let Some(token_val) = msg_str.strip_prefix("token:") {
                crate::token::set_token(token_val);
            }
            let mut sender: heapless::String<32> = heapless::String::new();
            let _ = sender.push_str(sender_str);
            let mut text_str: heapless::String<128> = heapless::String::new();
            let _ = text_str.push_str(msg_str);
            crate::LAST_LORA_MSG.lock(|cell| {
                *cell.borrow_mut() = Some(crate::LoraMessage {
                    channel: ch_name,
                    sender,
                    text: text_str,
                    timestamp: dec.timestamp,
                    rssi,
                    snr_x4,
                });
            });
            crate::LORA_MSG_SIGNAL.signal(());

            // Add to the channel message ring for the on-device browser.
            {
                let mut s: heapless::String<16> = heapless::String::new();
                let _ = s.push_str(truncate_bytes(sender_str, 16));
                let mut t: heapless::String<{ crate::CHANNEL_MSG_TEXT_MAX }> =
                    heapless::String::new();
                let _ = t.push_str(truncate_bytes(msg_str, crate::CHANNEL_MSG_TEXT_MAX));
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
    if sig_ok && matches!(a.role.to_u8(), 1 | 2) {
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
    super::discovery::note(
        &a.pub_key,
        name_str.as_str(),
        a.role.to_u8(),
        lat,
        lon,
        a.timestamp,
    );

    // Wake the Contacts-screen cache so it can rebuild from the
    // persisted contact store now that this advert has landed in it.
    crate::ADVERT_SIGNAL.signal(());

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
        let type_bit = match a.role.to_u8() {
            1 => AUTO_ADD_CHAT,
            2 => AUTO_ADD_REPEATER,
            3 => AUTO_ADD_ROOM,
            4 => AUTO_ADD_SENSOR,
            _ => 0, // unknown type — never auto-add
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
                Ok(_) => {}
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
    let hint = crate::LAST_REQ_TARGET.lock(|cell| cell.get());
    if let Some(hint_pk) = hint
        && hint_pk[0] == msg.src_hash
        && let Some(c) = store.find_by_key(&hint_pk).await
        && try_handle_txt_msg(lora, &c, &msg, rssi, path_len_byte, path, identity, &store)
            .await
            .is_ok()
    {
        return;
    }

    // Fallback: look up candidate slots via the `hi` hash-byte index. One
    // flash `get` for the bucket + up to `MAX_SLOTS_PER_BUCKET` slot reads.
    // For uncached contacts (the hint didn't match), this replaces what was
    // previously a full-table linear scan.
    let mut slots = [0u16; contacts::MAX_SLOTS_PER_BUCKET];
    let n = store.hash_index_lookup(msg.src_hash, &mut slots).await;
    for &slot in &slots[..n] {
        let Some(c) = store.read_slot(slot as usize).await else {
            continue;
        };
        if c.pub_key[0] != msg.src_hash {
            continue;
        }
        if hint == Some(c.pub_key) {
            continue;
        }

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
    let is_cli = dec.txt_type() == txt_msg::TXT_TYPE_CLI_DATA;
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
        send_ack(lora, &sender.pub_key, path_len_byte, path, signed_ack).await;

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
        send_ack(lora, &sender.pub_key, path_len_byte, path, ack_hash).await;

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
        let mut text_str: heapless::String<{ meshcore::payload::txt_msg::MAX_TXT_TEXT_SIZE }> =
            heapless::String::new();
        let _ = text_str.push_str(text);
        crate::LAST_PM.lock(|cell| {
            *cell.borrow_mut() = Some(crate::LastPm {
                sender_name: display_name.clone(),
                text: text_str.clone(),
                timestamp: dec.timestamp,
                rssi,
            });
        });
        super::pm_inbox::note_incoming(&sender.pub_key, display_name.as_str(), text_str.as_str());
        crate::PM_SIGNAL.signal(());
        crate::PM_UNREAD.store(true, core::sync::atomic::Ordering::Relaxed);

        // Token screen: direct messages starting with "token:" also work.
        if let Some(token_val) = text.strip_prefix("token:") {
            crate::token::set_token(token_val);
        }

        if !crate::BLE_CONNECTED.load(core::sync::atomic::Ordering::Relaxed) {
            crate::fw::led::set_led(&crate::fw::led::LED_BLUE, crate::fw::led::LedState::Blink);
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
    use meshcore::packet::RouteType;
    crate::FLOOD_SCOPE_KEY.lock(|cell| match cell.get() {
        Some(key) => {
            let code = meshcore::packet::calc_transport_code(&key, payload_type_u8, payload);
            (RouteType::TransportFlood, code)
        }
        None => (RouteType::Flood, 0),
    })
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
        crate::TxRequest::PrivateMsg(msg) => send_txt_msg(lora, msg, identity).await,
        crate::TxRequest::Trace(msg) => send_trace(lora, msg).await,
        crate::TxRequest::Login(msg) => send_login(lora, msg, identity).await,
        crate::TxRequest::AdminStatusReq(msg) => {
            send_admin_status_request(lora, msg, identity).await
        }
        crate::TxRequest::BinaryReq(msg) => send_binary_request(lora, msg, identity).await,
        crate::TxRequest::TelemReq(msg) => send_telem_request(lora, msg, identity).await,
        crate::TxRequest::DiscoveryReq(msg) => send_discovery_request(lora, msg, identity).await,
        crate::TxRequest::ControlData(msg) => send_control_data(lora, msg).await,
        crate::TxRequest::Advert(mode) => {
            let mut name_buf = [0u8; settings::MAX_NODE_NAME];
            let name_len = settings::get_node_name(&mut name_buf).await;
            send_advert(lora, identity, &name_buf[..name_len], 0, mode).await;
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
    use meshcore::packet::{Message, PayloadType};
    use meshcore::payload::grp_txt;
    use meshcore::{MAX_PAYLOAD_SIZE, MAX_TRANS_UNIT};

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

    let (route, transport_code) = flood_route(PayloadType::GrpTxt.to_u8(), &msg_payload);
    let msg = Message {
        payload_type: PayloadType::GrpTxt,
        route,
        version: 0,
        transport_code,
        path_len_byte: flood_path_len_byte(),
        path: heapless::Vec::new(),
        payload: msg_payload,
    };

    let mut frame = [0u8; MAX_TRANS_UNIT];
    match meshcore::packet::serialize(&msg, &mut frame) {
        Ok(len) => {
            if let Err(e) = lora.send_message(&frame[..len]).await {
                defmt::warn!("send_grp_txt: TX failed: {:?}", e);
            } else {
                defmt::info!(
                    "GrpTxt sent: ch={=u8} ts={=u32} len={=usize}B frame={=[u8]:02x}",
                    req.channel_idx,
                    req.timestamp,
                    len,
                    &frame[..len],
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
                    let _ = t.push_str(truncate_bytes(body, crate::CHANNEL_MSG_TEXT_MAX));
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
        }
        Err(e) => {
            defmt::warn!(
                "send_grp_txt: packet serialize failed: {:?}",
                defmt::Debug2Format(&e)
            );
        }
    }
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
    use meshcore::packet::{Message, PayloadType, RouteType};
    use meshcore::payload::txt_msg;
    use meshcore::{MAX_PAYLOAD_SIZE, MAX_TRANS_UNIT};

    // Hint for the RX fast path so the CLI reply (or plain-chat reply) lands
    // on a single O(1) find_by_key instead of a 300-slot linear scan.
    crate::LAST_REQ_TARGET.lock(|cell| cell.set(Some(req.recipient_pub_key)));

    // Look up the recipient contact for their stored path.
    let contact = contacts::ContactStore::new()
        .find_by_key(&req.recipient_pub_key)
        .await;

    let (route, path_len_byte, path_bytes) = match contact {
        Some(ref c) if c.out_path_len != contacts::OUT_PATH_UNKNOWN => {
            let actual = c.path_actual_bytes();
            let mut pv: heapless::Vec<u8, { meshcore::MAX_PATH_SIZE }> = heapless::Vec::new();
            let _ = pv.extend_from_slice(&c.out_path[..actual]);
            (RouteType::Direct, c.out_path_len, pv)
        }
        _ => (
            RouteType::Flood,
            flood_path_len_byte(),
            heapless::Vec::new(),
        ),
    };

    let (encrypted, expected_ack) = match txt_msg::encrypt(
        &identity.sec_key,
        &identity.pub_key,
        &req.recipient_pub_key,
        req.timestamp,
        req.txt_type,
        req.attempt,
        &req.text,
    ) {
        Ok(e) => e,
        Err(e) => {
            defmt::warn!(
                "send_txt_msg: encrypt failed: {:?}",
                defmt::Debug2Format(&e)
            );
            return;
        }
    };

    let mut payload_buf = [0u8; MAX_PAYLOAD_SIZE];
    let mut payload_len = 0usize;
    if let Err(e) = txt_msg::serialize(&encrypted, &mut payload_buf, &mut payload_len) {
        defmt::warn!(
            "send_txt_msg: serialize failed: {:?}",
            defmt::Debug2Format(&e)
        );
        return;
    }

    let mut msg_payload: heapless::Vec<u8, MAX_PAYLOAD_SIZE> = heapless::Vec::new();
    let _ = msg_payload.extend_from_slice(&payload_buf[..payload_len]);

    let (route, transport_code) = match route {
        RouteType::Flood => flood_route(PayloadType::TxtMsg.to_u8(), &msg_payload),
        r => (r, 0),
    };
    let msg = Message {
        payload_type: PayloadType::TxtMsg,
        route,
        version: 0,
        transport_code,
        path_len_byte,
        path: path_bytes,
        payload: msg_payload,
    };

    let mut frame = [0u8; MAX_TRANS_UNIT];
    match meshcore::packet::serialize(&msg, &mut frame) {
        Ok(len) => {
            if let Err(e) = lora.send_message(&frame[..len]).await {
                defmt::warn!("send_txt_msg: TX failed: {:?}", e);
            } else {
                defmt::info!(
                    "TxtMsg sent: to={=[u8]:02x} attempt={=u8} route={=str} len={=usize}B ack={=u32:#010x}",
                    &req.recipient_pub_key[..6],
                    req.attempt,
                    if route == RouteType::Direct {
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
                }
            }
        }
        Err(e) => {
            defmt::warn!(
                "send_txt_msg: packet serialize failed: {:?}",
                defmt::Debug2Format(&e)
            );
        }
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
    use meshcore::packet::{Message, PayloadType, RouteType};
    use meshcore::payload::advert::{Advert, DeviceRole, serialize};
    use meshcore::{MAX_PAYLOAD_SIZE, MAX_TRANS_UNIT};

    let mut advert = Advert {
        pub_key: identity.pub_key,
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

    if let Err(e) = meshcore::identity::sign_advert(&identity.sec_key, &mut advert) {
        defmt::warn!("send_advert: signing failed: {:?}", defmt::Debug2Format(&e));
        return;
    }

    let mut payload_buf = [0u8; MAX_PAYLOAD_SIZE];
    let mut payload_len = 0usize;
    if let Err(e) = serialize(&advert, &mut payload_buf, &mut payload_len) {
        defmt::warn!(
            "send_advert: serialize failed: {:?}",
            defmt::Debug2Format(&e)
        );
        return;
    }

    let mut msg_payload: heapless::Vec<u8, MAX_PAYLOAD_SIZE> = heapless::Vec::new();
    let _ = msg_payload.extend_from_slice(&payload_buf[..payload_len]);

    let (route, transport_code) = match mode {
        AdvertMode::Flood => flood_route(PayloadType::Advert.to_u8(), &msg_payload),
        AdvertMode::ZeroHop => (RouteType::Direct, 0u16), // path_len=0 → zero-hop direct
    };

    let msg = Message {
        payload_type: PayloadType::Advert,
        route,
        version: 0,
        transport_code,
        path_len_byte: flood_path_len_byte(),
        path: heapless::Vec::new(),
        payload: msg_payload,
    };

    let mut frame = [0u8; MAX_TRANS_UNIT];
    match meshcore::packet::serialize(&msg, &mut frame) {
        Ok(len) => {
            if let Err(e) = lora.send_message(&frame[..len]).await {
                defmt::warn!("send_advert: TX failed: {:?}", e);
            } else {
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
        }
        Err(e) => {
            defmt::warn!(
                "send_advert: packet serialize failed: {:?}",
                defmt::Debug2Format(&e)
            );
        }
    }
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
    use meshcore::packet::{Message, PayloadType};
    use meshcore::payload::txt_msg;
    use meshcore::{MAX_PAYLOAD_SIZE, MAX_TRANS_UNIT};

    let (msg, expected_ack) = match txt_msg::encrypt(
        &identity.sec_key,
        &identity.pub_key,
        recipient_pk,
        timestamp,
        txt_msg::TXT_TYPE_PLAIN,
        0,
        text,
    ) {
        Ok(m) => m,
        Err(e) => {
            defmt::warn!("send_pm: encrypt failed: {:?}", defmt::Debug2Format(&e));
            return;
        }
    };

    defmt::info!("send_pm: expected_ack={=u32:#010x}", expected_ack);

    let mut payload_buf = [0u8; MAX_PAYLOAD_SIZE];
    let mut payload_len = 0usize;
    if let Err(e) = txt_msg::serialize(&msg, &mut payload_buf, &mut payload_len) {
        defmt::warn!("send_pm: serialize failed: {:?}", defmt::Debug2Format(&e));
        return;
    }

    let mut msg_payload: heapless::Vec<u8, MAX_PAYLOAD_SIZE> = heapless::Vec::new();
    let _ = msg_payload.extend_from_slice(&payload_buf[..payload_len]);

    // TxtMsg uses Direct route so the full path to the recipient is embedded.
    // For now we send as Flood — the recipient will filter on dest_pub_key.
    let (route, transport_code) = flood_route(PayloadType::TxtMsg.to_u8(), &msg_payload);
    let packet = Message {
        payload_type: PayloadType::TxtMsg,
        route,
        version: 0,
        transport_code,
        path_len_byte: flood_path_len_byte(),
        path: heapless::Vec::new(),
        payload: msg_payload,
    };

    let mut frame = [0u8; MAX_TRANS_UNIT];
    match meshcore::packet::serialize(&packet, &mut frame) {
        Ok(len) => {
            if let Err(e) = lora.send_message(&frame[..len]).await {
                defmt::warn!("send_pm: TX failed: {:?}", e);
            } else {
                defmt::info!("PM sent ({=usize}B)", len);
            }
        }
        Err(e) => {
            defmt::warn!(
                "send_pm: packet serialize failed: {:?}",
                defmt::Debug2Format(&e)
            );
        }
    }
}

// ---------------------------------------------------------------------------
// ACK transmission
// ---------------------------------------------------------------------------

/// Send an ACK for a received TxtMsg back toward the sender.
///
/// If we have a stored path for the sender we send Direct; otherwise Flood.
async fn send_ack(
    lora: &mut SimpleLoRa<'_>,
    sender_pk: &[u8; meshcore::PUB_KEY_SIZE],
    _recv_path_len: u8,
    _recv_path: &heapless::Vec<u8, { meshcore::MAX_PATH_SIZE }>,
    ack_hash: u32,
) {
    use meshcore::MAX_TRANS_UNIT;
    use meshcore::packet::{Message, PayloadType, RouteType};

    let ack_bytes = ack_hash.to_le_bytes();

    let mut payload: heapless::Vec<u8, { meshcore::MAX_PAYLOAD_SIZE }> = heapless::Vec::new();
    let _ = payload.extend_from_slice(&ack_bytes);

    // Route: Direct if we have a stored path, Flood otherwise.
    let contact = contacts::ContactStore::new().find_by_key(sender_pk).await;
    let (route, path_len_byte, path_bytes) = match contact {
        Some(ref c) if c.out_path_len != contacts::OUT_PATH_UNKNOWN => {
            let actual = c.path_actual_bytes();
            let mut pv: heapless::Vec<u8, { meshcore::MAX_PATH_SIZE }> = heapless::Vec::new();
            let _ = pv.extend_from_slice(&c.out_path[..actual]);
            (RouteType::Direct, c.out_path_len, pv)
        }
        _ => (
            RouteType::Flood,
            flood_path_len_byte(),
            heapless::Vec::new(),
        ),
    };
    let (route, transport_code) = match route {
        RouteType::Flood => flood_route(PayloadType::Ack.to_u8(), &payload),
        r => (r, 0),
    };

    let msg = Message {
        payload_type: PayloadType::Ack,
        route,
        version: 0,
        transport_code,
        path_len_byte,
        path: path_bytes,
        payload,
    };

    let mut frame = [0u8; MAX_TRANS_UNIT];
    match meshcore::packet::serialize(&msg, &mut frame) {
        Ok(len) => {
            if let Err(e) = lora.send_message(&frame[..len]).await {
                defmt::warn!("send_ack: TX failed: {:?}", e);
            } else {
                defmt::info!("ACK sent ack={=u32:#010x} ({=usize}B)", ack_hash, len);
            }
        }
        Err(e) => {
            defmt::warn!("send_ack: serialize failed: {:?}", defmt::Debug2Format(&e));
        }
    }
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
    use meshcore::packet::{Message, PayloadType, RouteType};
    use meshcore::{MAX_PAYLOAD_SIZE, MAX_TRANS_UNIT};

    let mut payload: heapless::Vec<u8, MAX_PAYLOAD_SIZE> = heapless::Vec::new();
    let _ = payload.extend_from_slice(&req.tag.to_le_bytes());
    let _ = payload.extend_from_slice(&req.auth.to_le_bytes());
    let _ = payload.push(req.flags);
    let _ = payload.extend_from_slice(&req.path);

    let msg = Message {
        payload_type: PayloadType::Trace,
        route: RouteType::Direct,
        version: 0,
        transport_code: 0,
        path_len_byte: 0,
        path: heapless::Vec::new(),
        payload,
    };

    let mut frame = [0u8; MAX_TRANS_UNIT];
    match meshcore::packet::serialize(&msg, &mut frame) {
        Ok(len) => {
            if let Err(e) = lora.send_message(&frame[..len]).await {
                defmt::warn!("send_trace: TX failed: {:?}", e);
            } else {
                defmt::info!(
                    "Trace sent: tag={=u32:#010x} path_len={=usize}B len={=usize}B",
                    req.tag,
                    req.path.len(),
                    len,
                );
            }
        }
        Err(e) => {
            defmt::warn!(
                "send_trace: packet serialize failed: {:?}",
                defmt::Debug2Format(&e)
            );
        }
    }
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
    use meshcore::MAX_TRANS_UNIT;
    use meshcore::packet::{Message, PayloadType, RouteType};

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

    // ADV_TYPE_ROOM = 3 (see
    // `vendor/meshcore/src/payload/advert.rs::DeviceRole::RoomServer`).
    // When the target is a room server we include the persisted
    // `Contact.sync_since` — the last post timestamp we successfully ACKed.
    // The room uses this to resume pushing posts from where we left off
    // instead of replaying the whole backlog. Non-room targets omit the
    // sync_since header entirely (repeater-shaped login plaintext).
    const ADV_TYPE_ROOM: u8 = 3;
    let is_room = contact
        .as_ref()
        .map(|c| c.node_type == ADV_TYPE_ROOM)
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
    let (route, path_len_byte, path_bytes) = match contact {
        Some(ref c) if c.out_path_len != contacts::OUT_PATH_UNKNOWN => {
            let actual = c.path_actual_bytes();
            let mut pv: heapless::Vec<u8, { meshcore::MAX_PATH_SIZE }> = heapless::Vec::new();
            let _ = pv.extend_from_slice(&c.out_path[..actual]);
            (RouteType::Direct, c.out_path_len, pv)
        }
        _ => (
            RouteType::Flood,
            flood_path_len_byte(),
            heapless::Vec::new(),
        ),
    };

    let (route, transport_code) = match route {
        RouteType::Flood => flood_route(PayloadType::AnonReq.to_u8(), &payload),
        r => (r, 0),
    };
    let msg = Message {
        payload_type: PayloadType::AnonReq,
        route,
        version: 0,
        transport_code,
        path_len_byte,
        path: path_bytes,
        payload,
    };

    let mut frame = [0u8; MAX_TRANS_UNIT];
    match meshcore::packet::serialize(&msg, &mut frame) {
        Ok(len) => {
            if let Err(e) = lora.send_message(&frame[..len]).await {
                defmt::warn!("send_login: TX failed: {:?}", e);
            } else {
                defmt::info!(
                    "Login sent to {=[u8]:02x} node_type={=u8} is_room={=bool} ts={=u32} sync_since={=u32} ({=usize}B) frame={=[u8]:02x}",
                    &req.pub_key[..6],
                    contact.as_ref().map(|c| c.node_type).unwrap_or(0xff),
                    is_room,
                    timestamp,
                    sync_since.unwrap_or(0),
                    len,
                    &frame[..len],
                );
            }
        }
        Err(e) => {
            defmt::warn!(
                "send_login: packet serialize failed: {:?}",
                defmt::Debug2Format(&e)
            );
        }
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
    use meshcore::MAX_TRANS_UNIT;
    use meshcore::packet::{Message, PayloadType, RouteType};

    crate::LAST_REQ_TARGET.lock(|cell| cell.set(Some(req.pub_key)));

    // REQ_TYPE_GET_TELEMETRY_DATA = 0x03.
    // txt_msg::encrypt encodes flags = (attempt & 3) | (txt_type << 2).
    // To get flags = 0x03: attempt = 3, txt_type = 0.
    // text = [reserved:4][random:4] — we use zeros for both (no RNG needed).
    const REQ_TYPE_TELEMETRY: u8 = 0x03;
    // attempt = REQ_TYPE_TELEMETRY so that flags byte = 0x03 in the AES plaintext.
    let text: [u8; 8] = [0u8; 8]; // reserved(4) + random(4) — zeros acceptable

    let (encrypted, _ack_hash) = match meshcore::payload::txt_msg::encrypt(
        &identity.sec_key,
        &identity.pub_key,
        &req.pub_key,
        req.tag,
        0,                  // txt_type = 0 → upper bits of flags = 0
        REQ_TYPE_TELEMETRY, // attempt field → flags & 3 = 0x03
        &text,
    ) {
        Ok(r) => r,
        Err(e) => {
            defmt::warn!(
                "send_telem_req: encrypt failed: {:?}",
                defmt::Debug2Format(&e)
            );
            return;
        }
    };

    let mut payload_buf = [0u8; meshcore::MAX_PAYLOAD_SIZE];
    let mut payload_len = 0usize;
    if let Err(e) =
        meshcore::payload::txt_msg::serialize(&encrypted, &mut payload_buf, &mut payload_len)
    {
        defmt::warn!(
            "send_telem_req: serialize failed: {:?}",
            defmt::Debug2Format(&e)
        );
        return;
    }

    let contact = contacts::ContactStore::new()
        .find_by_key(&req.pub_key)
        .await;
    let (route, path_len_byte, path_bytes) = match contact {
        Some(ref c) if c.out_path_len != contacts::OUT_PATH_UNKNOWN => {
            let actual = c.path_actual_bytes();
            let mut pv: heapless::Vec<u8, { meshcore::MAX_PATH_SIZE }> = heapless::Vec::new();
            let _ = pv.extend_from_slice(&c.out_path[..actual]);
            (RouteType::Direct, c.out_path_len, pv)
        }
        _ => (
            RouteType::Flood,
            flood_path_len_byte(),
            heapless::Vec::new(),
        ),
    };

    let mut payload_vec: heapless::Vec<u8, { meshcore::MAX_PAYLOAD_SIZE }> = heapless::Vec::new();
    let _ = payload_vec.extend_from_slice(&payload_buf[..payload_len]);

    let (route, transport_code) = match route {
        RouteType::Flood => flood_route(PayloadType::Req.to_u8(), &payload_vec),
        r => (r, 0),
    };
    let msg = Message {
        payload_type: PayloadType::Req,
        route,
        version: 0,
        transport_code,
        path_len_byte,
        path: path_bytes,
        payload: payload_vec,
    };

    let mut frame = [0u8; MAX_TRANS_UNIT];
    match meshcore::packet::serialize(&msg, &mut frame) {
        Ok(len) => {
            crate::PENDING_TELEM_TAG.lock(|cell| cell.set(Some(req.tag)));
            if let Err(e) = lora.send_message(&frame[..len]).await {
                defmt::warn!("send_telem_req: TX failed: {:?}", e);
                crate::PENDING_TELEM_TAG.lock(|cell| cell.set(None));
            } else {
                defmt::info!(
                    "Telem req sent to {=[u8]:02x} tag={=u32:#010x} ({=usize}B)",
                    &req.pub_key[..6],
                    req.tag,
                    len
                );
            }
        }
        Err(e) => {
            defmt::warn!(
                "send_telem_req: packet serialize failed: {:?}",
                defmt::Debug2Format(&e)
            );
        }
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
    use meshcore::MAX_TRANS_UNIT;
    use meshcore::packet::{Message, PayloadType, RouteType};
    use meshcore::payload::txt_msg;

    const REQ_TYPE_GET_STATUS: u8 = 0x01;
    let text: [u8; 8] = [0u8; 8];

    crate::LAST_REQ_TARGET.lock(|cell| cell.set(Some(req.pub_key)));

    let (encrypted, _ack_hash) = match txt_msg::encrypt(
        &identity.sec_key,
        &identity.pub_key,
        &req.pub_key,
        req.tag,
        0,                   // txt_type = 0
        REQ_TYPE_GET_STATUS, // attempt → flags & 3 = 0x01
        &text,
    ) {
        Ok(r) => r,
        Err(e) => {
            defmt::warn!(
                "send_admin_status_req: encrypt failed: {:?}",
                defmt::Debug2Format(&e)
            );
            return;
        }
    };

    let mut payload_buf = [0u8; meshcore::MAX_PAYLOAD_SIZE];
    let mut payload_len = 0usize;
    if let Err(e) = txt_msg::serialize(&encrypted, &mut payload_buf, &mut payload_len) {
        defmt::warn!(
            "send_admin_status_req: serialize failed: {:?}",
            defmt::Debug2Format(&e)
        );
        return;
    }

    let contact = contacts::ContactStore::new()
        .find_by_key(&req.pub_key)
        .await;
    let (route, path_len_byte, path_bytes) = match contact {
        Some(ref c) if c.out_path_len != contacts::OUT_PATH_UNKNOWN => {
            let actual = c.path_actual_bytes();
            let mut pv: heapless::Vec<u8, { meshcore::MAX_PATH_SIZE }> = heapless::Vec::new();
            let _ = pv.extend_from_slice(&c.out_path[..actual]);
            (RouteType::Direct, c.out_path_len, pv)
        }
        _ => (
            RouteType::Flood,
            flood_path_len_byte(),
            heapless::Vec::new(),
        ),
    };

    let mut payload_vec: heapless::Vec<u8, { meshcore::MAX_PAYLOAD_SIZE }> = heapless::Vec::new();
    let _ = payload_vec.extend_from_slice(&payload_buf[..payload_len]);

    let (route, transport_code) = match route {
        RouteType::Flood => flood_route(PayloadType::Req.to_u8(), &payload_vec),
        r => (r, 0),
    };
    let msg = Message {
        payload_type: PayloadType::Req,
        route,
        version: 0,
        transport_code,
        path_len_byte,
        path: path_bytes,
        payload: payload_vec,
    };

    let mut frame = [0u8; MAX_TRANS_UNIT];
    match meshcore::packet::serialize(&msg, &mut frame) {
        Ok(len) => {
            crate::PENDING_ADMIN_STATUS_TAG.lock(|cell| cell.set(Some(req.tag)));
            if let Err(e) = lora.send_message(&frame[..len]).await {
                defmt::warn!("send_admin_status_req: TX failed: {:?}", e);
                crate::PENDING_ADMIN_STATUS_TAG.lock(|cell| cell.set(None));
            } else {
                defmt::info!(
                    "Admin status req sent to {=[u8]:02x} tag={=u32:#010x} ({=usize}B) frame={=[u8]:02x}",
                    &req.pub_key[..6],
                    req.tag,
                    len,
                    &frame[..len],
                );
            }
        }
        Err(e) => {
            defmt::warn!(
                "send_admin_status_req: packet serialize failed: {:?}",
                defmt::Debug2Format(&e)
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Generic binary request transmission
// ---------------------------------------------------------------------------

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
async fn send_binary_request(
    lora: &mut SimpleLoRa<'_>,
    req: crate::TxBinaryReq,
    identity: &DeviceIdentity,
) {
    use meshcore::MAX_TRANS_UNIT;
    use meshcore::packet::{Message, PayloadType, RouteType};
    use meshcore::payload::txt_msg;

    if req.req_data.is_empty() {
        defmt::warn!("send_binary_req: empty req_data — dropping");
        return;
    }

    crate::LAST_REQ_TARGET.lock(|cell| cell.set(Some(req.pub_key)));

    let req_type = req.req_data[0];
    let params = &req.req_data[1..];
    let attempt = req_type & 0x03;
    let txt_type = req_type >> 2;

    let (encrypted, _ack_hash) = match txt_msg::encrypt(
        &identity.sec_key,
        &identity.pub_key,
        &req.pub_key,
        req.tag,
        txt_type,
        attempt,
        params,
    ) {
        Ok(r) => r,
        Err(e) => {
            defmt::warn!(
                "send_binary_req: encrypt failed: {:?}",
                defmt::Debug2Format(&e)
            );
            return;
        }
    };

    let mut payload_buf = [0u8; meshcore::MAX_PAYLOAD_SIZE];
    let mut payload_len = 0usize;
    if let Err(e) = txt_msg::serialize(&encrypted, &mut payload_buf, &mut payload_len) {
        defmt::warn!(
            "send_binary_req: serialize failed: {:?}",
            defmt::Debug2Format(&e)
        );
        return;
    }

    let contact = contacts::ContactStore::new()
        .find_by_key(&req.pub_key)
        .await;
    let (route, path_len_byte, path_bytes) = match contact {
        Some(ref c) if c.out_path_len != contacts::OUT_PATH_UNKNOWN => {
            let actual = c.path_actual_bytes();
            let mut pv: heapless::Vec<u8, { meshcore::MAX_PATH_SIZE }> = heapless::Vec::new();
            let _ = pv.extend_from_slice(&c.out_path[..actual]);
            (RouteType::Direct, c.out_path_len, pv)
        }
        _ => (
            RouteType::Flood,
            flood_path_len_byte(),
            heapless::Vec::new(),
        ),
    };

    let mut payload_vec: heapless::Vec<u8, { meshcore::MAX_PAYLOAD_SIZE }> = heapless::Vec::new();
    let _ = payload_vec.extend_from_slice(&payload_buf[..payload_len]);

    let (route, transport_code) = match route {
        RouteType::Flood => flood_route(PayloadType::Req.to_u8(), &payload_vec),
        r => (r, 0),
    };
    let msg = Message {
        payload_type: PayloadType::Req,
        route,
        version: 0,
        transport_code,
        path_len_byte,
        path: path_bytes,
        payload: payload_vec,
    };

    let mut frame = [0u8; MAX_TRANS_UNIT];
    match meshcore::packet::serialize(&msg, &mut frame) {
        Ok(len) => {
            crate::PENDING_BINARY_REQ_TAG.lock(|cell| cell.set(Some(req.tag)));
            if let Err(e) = lora.send_message(&frame[..len]).await {
                defmt::warn!("send_binary_req: TX failed: {:?}", e);
                crate::PENDING_BINARY_REQ_TAG.lock(|cell| cell.set(None));
            } else {
                defmt::info!(
                    "Binary req sent to {=[u8]:02x} tag={=u32:#010x} req_type={=u8:#04x} params={=usize}B ({=usize}B) frame={=[u8]:02x}",
                    &req.pub_key[..6],
                    req.tag,
                    req_type,
                    params.len(),
                    len,
                    &frame[..len],
                );
            }
        }
        Err(e) => {
            defmt::warn!(
                "send_binary_req: packet serialize failed: {:?}",
                defmt::Debug2Format(&e)
            );
        }
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
    use meshcore::MAX_TRANS_UNIT;
    use meshcore::packet::{Message, PayloadType};

    crate::LAST_REQ_TARGET.lock(|cell| cell.set(Some(req.pub_key)));

    // Discovery uses flags = 0x03 (REQ_TYPE_GET_TELEMETRY_DATA), same as telemetry,
    // but the text body starts with 0xFE (~TELEM_PERM_BASE) to signal discovery
    // intent. txt_msg::encrypt: flags = (attempt & 3) | (txt_type << 2) →
    // attempt=3, txt_type=0. text = [0xFE, 0, 0, 0, 0, 0, 0, 0] (perm byte + 7
    // padding/random zeros).
    const REQ_TYPE_DISCOVERY: u8 = 0x03;
    let text: [u8; 8] = [0xFE, 0, 0, 0, 0, 0, 0, 0];

    let (encrypted, _ack_hash) = match meshcore::payload::txt_msg::encrypt(
        &identity.sec_key,
        &identity.pub_key,
        &req.pub_key,
        req.tag,
        0,                  // txt_type = 0
        REQ_TYPE_DISCOVERY, // attempt → flags & 3 = 0x03
        &text,
    ) {
        Ok(r) => r,
        Err(e) => {
            defmt::warn!(
                "send_discovery_req: encrypt failed: {:?}",
                defmt::Debug2Format(&e)
            );
            return;
        }
    };

    let mut payload_buf = [0u8; meshcore::MAX_PAYLOAD_SIZE];
    let mut payload_len = 0usize;
    if let Err(e) =
        meshcore::payload::txt_msg::serialize(&encrypted, &mut payload_buf, &mut payload_len)
    {
        defmt::warn!(
            "send_discovery_req: serialize failed: {:?}",
            defmt::Debug2Format(&e)
        );
        return;
    }

    let mut payload_vec: heapless::Vec<u8, { meshcore::MAX_PAYLOAD_SIZE }> = heapless::Vec::new();
    let _ = payload_vec.extend_from_slice(&payload_buf[..payload_len]);

    // Always flood for discovery — the whole point is to find new paths.
    let (route, transport_code) = flood_route(PayloadType::Req.to_u8(), &payload_vec);
    let msg = Message {
        payload_type: PayloadType::Req,
        route,
        version: 0,
        transport_code,
        path_len_byte: flood_path_len_byte(),
        path: heapless::Vec::new(),
        payload: payload_vec,
    };

    let mut frame = [0u8; MAX_TRANS_UNIT];
    match meshcore::packet::serialize(&msg, &mut frame) {
        Ok(len) => {
            crate::PENDING_DISCOVERY_TAG.lock(|cell| cell.set(Some(req.tag)));
            if let Err(e) = lora.send_message(&frame[..len]).await {
                defmt::warn!("send_discovery_req: TX failed: {:?}", e);
                crate::PENDING_DISCOVERY_TAG.lock(|cell| cell.set(None));
            } else {
                defmt::info!(
                    "Discovery req sent to {=[u8]:02x} tag={=u32:#010x} ({=usize}B) [flood]",
                    &req.pub_key[..6],
                    req.tag,
                    len
                );
            }
        }
        Err(e) => {
            defmt::warn!(
                "send_discovery_req: packet serialize failed: {:?}",
                defmt::Debug2Format(&e)
            );
        }
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
    let store = contacts::ContactStore::new();
    let hint = crate::LAST_REQ_TARGET.lock(|cell| cell.get());
    if let Some(hint_pk) = hint
        && hint_pk[0] == msg.src_hash
        && try_dispatch_response_by_pk(&hint_pk, &msg, identity)
            .await
            .is_ok()
    {
        return;
    }

    // Fallback: look up candidate slots via the `hi` hash-byte index instead
    // of a 300-slot linear scan. Covers unsolicited responses from peers we
    // didn't just request anything from.
    let mut slots = [0u16; contacts::MAX_SLOTS_PER_BUCKET];
    let n = store.hash_index_lookup(msg.src_hash, &mut slots).await;
    for &slot in &slots[..n] {
        let Some(c) = store.read_slot(slot as usize).await else {
            continue;
        };
        if c.pub_key[0] != msg.src_hash {
            continue;
        }
        if hint == Some(c.pub_key) {
            continue;
        }

        if try_dispatch_response(&c, &msg, identity).await.is_ok() {
            return;
        }
    }

    defmt::debug!("Response recv: could not decrypt with any known contact");
}

/// Verify MAC, decrypt and dispatch a `PayloadType::Response` packet.
///
/// Thin wrapper around [`try_dispatch_response_by_pk`] that accepts a stored
/// [`contacts::Contact`] — kept so the store-fallback scan in
/// [`handle_response_recv`] can stay unchanged.
async fn try_dispatch_response(
    c: &contacts::Contact,
    msg: &meshcore::payload::txt_msg::TxtMsg,
    identity: &DeviceIdentity,
) -> Result<(), meshcore::Error> {
    try_dispatch_response_by_pk(&c.pub_key, msg, identity).await
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
/// the pending-request branches or treated as a login response. Returns
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

    // Pending admin status request (REQ_TYPE_GET_STATUS, tag-based match).
    // Plaintext layout: [ts:4 LE][RepeaterStats:56]. After txt_msg::decrypt
    // that becomes dec.timestamp = ts, dec.flags = stats[0], dec.text =
    // stats[1..] (with trailing zero bytes stripped). We reassemble the full
    // 56-byte blob and parse it manually.
    let pending_admin_status = crate::PENDING_ADMIN_STATUS_TAG.lock(|cell| cell.get());
    if let Some(admin_tag) = pending_admin_status
        && dec.timestamp == admin_tag
    {
        crate::PENDING_ADMIN_STATUS_TAG.lock(|cell| cell.set(None));

        let mut stats = [0u8; 56];
        stats[0] = dec.flags;
        let tail_len = dec.text.len().min(55);
        stats[1..1 + tail_len].copy_from_slice(&dec.text[..tail_len]);

        let result = parse_repeater_stats(&stats, pub_key, dec.timestamp);

        defmt::info!(
            "Response recv: ADMIN_STATUS from {=[u8]:02x} tag={=u32:#010x} up={=u32}s batt={=u16}mV queue={=u16} rssi={=i16} snrX4={=i16} recv={=u32} sent={=u32}",
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
        return Ok(());
    }

    // Pending generic binary request (CMD_SEND_BINARY_REQ /
    // PUSH_CODE_BINARY_RESPONSE). Response body starts at plaintext[4]; in our
    // txt_msg::decrypt view that's dec.flags (first body byte) followed by
    // dec.text (rest, trailing zeros stripped). We reassemble the padded body
    // up to the AES block boundary so neighbours/ACL entries that happen to end
    // in zero bytes aren't truncated.
    let pending_binary = crate::PENDING_BINARY_REQ_TAG.lock(|cell| cell.get());
    if let Some(binary_tag) = pending_binary
        && dec.timestamp == binary_tag
    {
        crate::PENDING_BINARY_REQ_TAG.lock(|cell| cell.set(None));

        // msg.data is the raw ciphertext; length is a multiple of AES block size.
        // Body length = total plaintext - timestamp(4) = msg.data.len() - 4.
        let body_len = msg.data.len().saturating_sub(4);
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

        defmt::info!(
            "Response recv: BINARY from {=[u8]:02x} tag={=u32:#010x} body={=usize}B",
            &sender_pk[..6],
            binary_tag,
            body.len(),
        );

        let _ = crate::BINARY_RESULT_CHANNEL.try_send(crate::BinaryResult {
            pub_key,
            tag: binary_tag,
            body,
        });
        return Ok(());
    }

    // Pending telemetry request (tag-based match).
    let pending_telem = crate::PENDING_TELEM_TAG.lock(|cell| cell.get());
    if let Some(telem_tag) = pending_telem
        && dec.timestamp == telem_tag
    {
        crate::PENDING_TELEM_TAG.lock(|cell| cell.set(None));

        // CayenneLPP = flags_byte (= data[4]) followed by dec.text (= data[5..]).
        let mut lpp: heapless::Vec<u8, 176> = heapless::Vec::new();
        let _ = lpp.push(dec.flags);
        let _ = lpp.extend_from_slice(&dec.text);

        defmt::info!(
            "Response recv: TELEM from {=[u8]:02x} lpp={=usize}B",
            &sender_pk[..6],
            lpp.len(),
        );

        let _ = crate::TELEM_RESULT_CHANNEL.try_send(crate::TelemResult { pub_key, lpp });
        return Ok(());
    }

    // No pending status / admin-status / telemetry request — treat as a
    // login response. The MAC verified, so this contact is the right peer.
    let success = resp_type == 0 || (resp_type == b'O' && dec.text.first() == Some(&b'K'));

    defmt::info!(
        "Response recv: login {} from {=[u8]:02x} resp_type={=u8}",
        if success { "OK" } else { "FAIL" },
        &sender_pk[..6],
        resp_type,
    );

    let tag = dec.timestamp;

    let is_admin = if success {
        dec.text.get(1).copied().unwrap_or(0)
    } else {
        0
    };
    let acl_perms = if success {
        dec.text.get(2).copied().unwrap_or(0)
    } else {
        0
    };
    let fw_ver_level = if success {
        dec.text.get(7).copied().unwrap_or(0)
    } else {
        0
    };

    let _ = crate::LOGIN_RESULT_CHANNEL.try_send(crate::LoginResult {
        success,
        is_admin,
        pub_key,
        tag,
        acl_perms,
        fw_ver_level,
    });
    Ok(())
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
    let store = contacts::ContactStore::new();
    let hint = crate::LAST_REQ_TARGET.lock(|cell| cell.get());
    if let Some(hint_pk) = hint
        && hint_pk[0] == msg.src_hash
        && try_dispatch_path_by_pk(&hint_pk, &msg, rssi, &store, identity)
            .await
            .is_ok()
    {
        return;
    }

    // Fallback: look up candidate slots via the `hi` hash-byte index instead
    // of a 300-slot linear scan. Covers unsolicited Path returns from peers
    // we didn't just request anything from.
    let mut slots = [0u16; contacts::MAX_SLOTS_PER_BUCKET];
    let n = store.hash_index_lookup(msg.src_hash, &mut slots).await;
    for &slot in &slots[..n] {
        let Some(c) = store.read_slot(slot as usize).await else {
            continue;
        };
        if c.pub_key[0] != msg.src_hash {
            continue;
        }
        if hint == Some(c.pub_key) {
            continue;
        }

        if try_dispatch_path(&c, &msg, rssi, &store, identity)
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

/// Verify, decrypt and dispatch a `PayloadType::Path` packet.
///
/// Thin wrapper around [`try_dispatch_path_by_pk`] that accepts a stored
/// [`contacts::Contact`] — kept so the store-fallback scan in
/// [`handle_path_recv`] can stay unchanged.
async fn try_dispatch_path(
    c: &contacts::Contact,
    msg: &meshcore::payload::path_msg::PathMsg,
    rssi: i16,
    store: &contacts::ContactStore,
    identity: &DeviceIdentity,
) -> Result<(), meshcore::Error> {
    try_dispatch_path_by_pk(&c.pub_key, msg, rssi, store, identity).await
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

    // PAYLOAD_TYPE_RESPONSE = 1 → admin-status / discovery / login response.
    if dec.extra_type == 1 {
        // Pending admin status request (tag = extra[0..4]). Path-decrypted
        // extras preserve trailing zero bytes, so we can parse stats directly.
        let pending_admin_status = crate::PENDING_ADMIN_STATUS_TAG.lock(|cell| cell.get());
        if let (Some(admin_tag), true) = (pending_admin_status, dec.extra.len() >= 4) {
            let resp_tag =
                u32::from_le_bytes([dec.extra[0], dec.extra[1], dec.extra[2], dec.extra[3]]);
            if resp_tag == admin_tag {
                crate::PENDING_ADMIN_STATUS_TAG.lock(|cell| cell.set(None));

                let mut pub_key = [0u8; meshcore::PUB_KEY_SIZE];
                pub_key.copy_from_slice(sender_pk);

                // Copy the raw stats blob (truncate or zero-pad to exactly 56B).
                let mut stats = [0u8; 56];
                let n = (dec.extra.len() - 4).min(56);
                stats[..n].copy_from_slice(&dec.extra[4..4 + n]);

                let result = parse_repeater_stats(&stats, pub_key, admin_tag);

                defmt::info!(
                    "Path recv: ADMIN_STATUS from {=[u8]:02x} tag={=u32:#010x} up={=u32}s batt={=u16}mV queue={=u16} rssi={=i16} snrX4={=i16} recv={=u32} sent={=u32}",
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
                let _ =
                    crate::STATUS_RESULT_CHANNEL.try_send(crate::StatusResult { pub_key, stats });
                return Ok(());
            }
        }

        // Pending generic binary request (CMD_SEND_BINARY_REQ). Path-decrypted
        // extras preserve trailing zero bytes, so we can forward the body verbatim.
        let pending_binary = crate::PENDING_BINARY_REQ_TAG.lock(|cell| cell.get());
        if let (Some(binary_tag), true) = (pending_binary, dec.extra.len() >= 4) {
            let resp_tag =
                u32::from_le_bytes([dec.extra[0], dec.extra[1], dec.extra[2], dec.extra[3]]);
            if resp_tag == binary_tag {
                crate::PENDING_BINARY_REQ_TAG.lock(|cell| cell.set(None));

                let mut pub_key = [0u8; meshcore::PUB_KEY_SIZE];
                pub_key.copy_from_slice(sender_pk);

                let mut body: heapless::Vec<u8, { crate::MAX_BINARY_RESP_BODY }> =
                    heapless::Vec::new();
                let n = (dec.extra.len() - 4).min(crate::MAX_BINARY_RESP_BODY);
                let _ = body.extend_from_slice(&dec.extra[4..4 + n]);

                defmt::info!(
                    "Path recv: BINARY from {=[u8]:02x} tag={=u32:#010x} body={=usize}B",
                    &sender_pk[..6],
                    binary_tag,
                    body.len(),
                );

                let _ = crate::BINARY_RESULT_CHANNEL.try_send(crate::BinaryResult {
                    pub_key,
                    tag: binary_tag,
                    body,
                });
                return Ok(());
            }
        }

        // Pending telemetry request (tag = extra[0..4]). Body is CayenneLPP.
        let pending_telem = crate::PENDING_TELEM_TAG.lock(|cell| cell.get());
        if let (Some(telem_tag), true) = (pending_telem, dec.extra.len() >= 4) {
            let resp_tag =
                u32::from_le_bytes([dec.extra[0], dec.extra[1], dec.extra[2], dec.extra[3]]);
            if resp_tag == telem_tag {
                crate::PENDING_TELEM_TAG.lock(|cell| cell.set(None));

                let mut pub_key = [0u8; meshcore::PUB_KEY_SIZE];
                pub_key.copy_from_slice(sender_pk);

                let mut lpp: heapless::Vec<u8, 176> = heapless::Vec::new();
                let n = (dec.extra.len() - 4).min(176);
                let _ = lpp.extend_from_slice(&dec.extra[4..4 + n]);

                defmt::info!(
                    "Path recv: TELEM from {=[u8]:02x} tag={=u32:#010x} lpp={=usize}B",
                    &sender_pk[..6],
                    telem_tag,
                    lpp.len(),
                );

                let _ = crate::TELEM_RESULT_CHANNEL.try_send(crate::TelemResult { pub_key, lpp });
                return Ok(());
            }
        }

        // Pending discovery request (tag = extra[0..4]).
        let pending_disc = crate::PENDING_DISCOVERY_TAG.lock(|cell| cell.get());
        if let Some(disc_tag) = pending_disc
            && dec.extra.len() >= 4
        {
            let resp_tag =
                u32::from_le_bytes([dec.extra[0], dec.extra[1], dec.extra[2], dec.extra[3]]);
            if resp_tag == disc_tag {
                crate::PENDING_DISCOVERY_TAG.lock(|cell| cell.set(None));

                let mut out_path: heapless::Vec<u8, { meshcore::MAX_PATH_SIZE }> =
                    heapless::Vec::new();
                let _ = out_path.extend_from_slice(&dec.path);
                let in_path: heapless::Vec<u8, { meshcore::MAX_PATH_SIZE }> = heapless::Vec::new();

                let mut pub_key = [0u8; meshcore::PUB_KEY_SIZE];
                pub_key.copy_from_slice(sender_pk);

                defmt::info!(
                    "Path recv: DISCOVERY response from {=[u8]:02x} tag={=u32:#010x} out_path_len={=u8}",
                    &sender_pk[..6],
                    disc_tag,
                    dec.path_len_byte,
                );

                let _ = crate::DISCOVERY_RESULT_CHANNEL.try_send(crate::DiscoveryResult {
                    pub_key,
                    out_path_len_byte: dec.path_len_byte,
                    out_path,
                    in_path_len_byte: 0xFF,
                    in_path,
                });
                return Ok(());
            }
        }

        // Default: treat as a login response.
        handle_path_login_response(&dec.extra, sender_pk);
    }

    Ok(())
}

/// Extract and push a login result from path-packet extra bytes.
///
/// Extra byte layout (same as the plaintext seen by C++ `onContactResponse`):
///   `[timestamp:4
/// LE][resp_type:1][keep_alive:1][is_admin:1][acl_perms:1][random:
/// 4][fw_ver_level:1]`
fn handle_path_login_response(extra: &[u8], sender_pub_key: &[u8; meshcore::PUB_KEY_SIZE]) {
    if extra.len() < path_msg::LOGIN_RESPONSE_EXTRA_LEN {
        defmt::warn!(
            "Path login response: extra too short ({=usize}B, need {=usize}B)",
            extra.len(),
            path_msg::LOGIN_RESPONSE_EXTRA_LEN,
        );
        return;
    }

    use meshcore::payload::path_msg;

    let tag = u32::from_le_bytes([extra[0], extra[1], extra[2], extra[3]]);
    let resp_type = extra[4];
    // extra[5] = keep_alive / 16
    let is_admin = extra[6];
    let acl_perms = extra[7];
    // extra[8..12] = random
    let fw_ver_level = extra[12];

    // RESP_SERVER_LOGIN_OK = 0 (new format).
    // Legacy: resp_type == b'O' and extra[5] == b'K'.
    let success = resp_type == 0 || (resp_type == b'O' && extra.get(5) == Some(&b'K'));

    defmt::info!(
        "Path login response: {} from {=[u8]:02x} resp_type={=u8} tag={=u32:#010x}",
        if success { "OK" } else { "FAIL" },
        &sender_pub_key[..6],
        resp_type,
        tag,
    );

    let mut pub_key = [0u8; meshcore::PUB_KEY_SIZE];
    pub_key.copy_from_slice(sender_pub_key);

    let _ = crate::LOGIN_RESULT_CHANNEL.try_send(crate::LoginResult {
        success,
        is_admin: if success { is_admin } else { 0 },
        pub_key,
        tag,
        acl_perms: if success { acl_perms } else { 0 },
        fw_ver_level: if success { fw_ver_level } else { 0 },
    });
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
    use meshcore::MAX_TRANS_UNIT;
    use meshcore::packet::{Message, PayloadType, RouteType};

    let msg = Message {
        payload_type: PayloadType::Unknown(0x0B), // PAYLOAD_TYPE_CONTROL
        route: RouteType::Direct,                 // zero-hop (path_len_byte = 0)
        version: 0,
        transport_code: 0,
        path_len_byte: 0,
        path: heapless::Vec::new(),
        payload: req.payload,
    };

    let mut frame = [0u8; MAX_TRANS_UNIT];
    match meshcore::packet::serialize(&msg, &mut frame) {
        Ok(len) => {
            if let Err(e) = lora.send_message(&frame[..len]).await {
                defmt::warn!("send_control_data: TX failed: {:?}", e);
            } else {
                defmt::info!(
                    "send_control_data: sent {=usize}B ctl_type={=u8:#04x}",
                    len,
                    msg.payload[0],
                );
            }
        }
        Err(e) => {
            defmt::warn!(
                "send_control_data: serialize failed: {:?}",
                defmt::Debug2Format(&e)
            );
        }
    }
}
