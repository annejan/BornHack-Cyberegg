use core::cell::RefCell;

use embassy_nrf::{Peri, gpio::AnyPin, peripherals};
use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_time::Timer;

use super::device_identity::DeviceIdentity;
use super::health::SYSTEM_HEALTH;
use super::settings;
use super::sx1262::{MeshCoreConfig, SimpleLoRa};
use super::{channels, contacts, msg_queue};
use crate::{health_err, update_health};
use meshcore::channel::hash_from_key;
use meshcore::contacts::Contacts;
use meshcore::dedup::{MsgHashRing, msg_hash};

// ---------------------------------------------------------------------------
// Loaded channel table
// ---------------------------------------------------------------------------

struct LoadedChannel {
    slot_idx: u8,
    name:     [u8; 32],
    key:      [u8; 16],
    hash:     u8,
}

async fn load_channels() -> heapless::Vec<LoadedChannel, { channels::NUM_CHANNELS }> {
    let mut v = heapless::Vec::new();
    for i in 0..channels::NUM_CHANNELS as u8 {
        if let Some((name, key)) = channels::get(i).await {
            let hash = hash_from_key(&key);
            let name_str = core::str::from_utf8(&name).unwrap_or("?").trim_end_matches('\0');
            defmt::info!("  channel slot {=u8}: hash={=u8} name={=str}", i, hash, name_str);
            let _ = v.push(LoadedChannel { slot_idx: i, name, key, hash });
        }
    }
    v
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
        defmt::error!(
            "SX1262 failed to enter RX mode after 500ms — check crystal/wiring"
        );
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
    defmt::info!("MeshCore identity pub_key: {=[u8]:02x}", &identity.pub_key[..]);

    let mut loaded_channels = load_channels().await;
    defmt::info!("MeshCore: loaded {} channel(s) from KV", loaded_channels.len());

    let mut raw = [0u8; 255];

    loop {
        // Reload channel table if the BLE task updated a channel.
        if crate::CHANNELS_CHANGED_SIGNAL.signaled() {
            crate::CHANNELS_CHANGED_SIGNAL.reset();
            loaded_channels = load_channels().await;
            defmt::info!("MeshCore: reloaded {} channel(s) from KV", loaded_channels.len());
        }

        // Update TX duty-cycle budget if tuning params changed.
        if let Some(af_x1000) = crate::TUNING_CHANGED_SIGNAL.try_take() {
            lora.init_budget(af_x1000);
            defmt::info!("TX budget updated: af={=u32}.{=u32:03}", af_x1000 / 1000, af_x1000 % 1000);
        }

        // Drain any already-queued outgoing messages before entering RX.
        while let Ok(req) = crate::TX_MSG_CHANNEL.try_receive() {
            send_grp_txt(&mut lora, &loaded_channels, req).await;
        }
        while let Ok(req) = crate::TX_PM_CHANNEL.try_receive() {
            send_txt_msg(&mut lora, req, identity).await;
        }
        while let Ok(req) = crate::TX_TRACE_CHANNEL.try_receive() {
            send_trace(&mut lora, req).await;
        }
        while let Ok(req) = crate::TX_LOGIN_CHANNEL.try_receive() {
            send_login(&mut lora, req, identity).await;
        }
        while let Ok(req) = crate::TX_STATUS_REQ_CHANNEL.try_receive() {
            send_status_request(&mut lora, req, identity).await;
        }
        while let Ok(req) = crate::TX_TELEM_REQ_CHANNEL.try_receive() {
            send_telem_request(&mut lora, req, identity).await;
        }
        while let Ok(req) = crate::TX_DISCOVERY_CHANNEL.try_receive() {
            send_discovery_request(&mut lora, req, identity).await;
        }
        while let Ok(req) = crate::TX_CONTROL_DATA_CHANNEL.try_receive() {
            send_control_data(&mut lora, req).await;
        }
        if let Some(mode) = crate::SEND_ADVERT_SIGNAL.try_take() {
            let mut name_buf = [0u8; settings::MAX_NODE_NAME];
            let name_len = settings::get_node_name(&mut name_buf).await;
            send_advert(&mut lora, identity, &name_buf[..name_len], 0, mode).await;
        }

        // Race: receive the next LoRa packet OR a new TX request arrives.
        // This keeps TX latency to the radio air-time only, instead of up to
        // the full 15-second receive_packet timeout.
        use embassy_futures::select::{Either3, select3};
        let rx_result = match select3(
            lora.receive_packet(&mut raw),
            crate::TX_MSG_CHANNEL.receive(),
            crate::TX_PM_CHANNEL.receive(),
        ).await {
            Either3::Second(tx_req) => {
                send_grp_txt(&mut lora, &loaded_channels, tx_req).await;
                continue;
            }
            Either3::Third(pm_req) => {
                send_txt_msg(&mut lora, pm_req, identity).await;
                continue;
            }
            Either3::First(result) => result,
        };

        match rx_result {
            Ok(None) => { /* CRC error or non-data IRQ — already re-armed */ }

            Ok(Some((len, rssi, snr_x4))) => {
                // Update radio stats snapshot for CMD_GET_STATS / STATS_TYPE_RADIO.
                {
                    let noise_floor = (rssi as i32 - (snr_x4 as i32 / 4)).clamp(-128, 0) as i16;
                    crate::RADIO_STATS.lock(|cell| {
                        cell.set(crate::RadioStats {
                            noise_floor,
                            last_rssi:   rssi.clamp(-128, 0) as i8,
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
                        rssi:   rssi.clamp(-128, 0) as i8,
                        len,
                        data:   [0u8; meshcore::MAX_TRANS_UNIT],
                    };
                    pkt.data[..len].copy_from_slice(&raw[..len]);
                    let _ = crate::RAW_PKT_CHANNEL.try_send(pkt);
                }

                let frame = &raw[..len];

                match meshcore::packet::deserialize(frame) {
                    Err(_) => {
                        defmt::info!(
                            "MeshCore [raw {=usize}B {=i16}dBm]: {=[u8]}",
                            len,
                            rssi,
                            frame
                        );
                    }

                    Ok(msg) => {
                        update_health!(|h| h.lora.set_ok("Packet received."));
                        use meshcore::packet::{PayloadType, RouteType};
                        // Mirror the original firmware: flood routes carry the wire-encoded
                        // path_len_byte (hash_size_code<<6 | hop_count); direct routes
                        // signal 0xFF (no path built up by relays).
                        let path_len = match msg.route {
                            RouteType::Flood | RouteType::TransportFlood => msg.path_len_byte,
                            _ => 0xFF,
                        };
                        match msg.payload_type {
                            PayloadType::GrpTxt => push_grp_txt(&msg.payload, rssi, snr_x4, path_len, &loaded_channels).await,
                            PayloadType::TxtMsg => log_txt_msg(&mut lora, &msg.payload, rssi, path_len, &msg.path, identity).await,
                            PayloadType::Advert => log_advert(&msg.payload, rssi, snr_x4, path_len, &msg.path).await,
                            PayloadType::Ack => handle_ack_recv(&msg.payload, rssi),
                            PayloadType::Trace => handle_trace_recv(&msg.payload, &msg.path, snr_x4).await,
                            PayloadType::Response => handle_response_recv(&msg.payload, identity).await,
                            PayloadType::Path => handle_path_recv(&msg.payload, rssi, identity).await,
                            PayloadType::Unknown(0x0B) => {
                                // PAYLOAD_TYPE_CONTROL — forward to BLE as PUSH_CODE_CONTROL_DATA (0x8E).
                                defmt::info!(
                                    "MeshCore control [{=usize}B {=i16}dBm ctl={=u8:#04x}]: {=[u8]:x}",
                                    len,
                                    rssi,
                                    msg.payload.first().copied().unwrap_or(0),
                                    msg.payload.as_slice(),
                                );
                                let mut payload_vec: heapless::Vec<u8, { meshcore::MAX_PAYLOAD_SIZE }> = heapless::Vec::new();
                                let _ = payload_vec.extend_from_slice(&msg.payload);
                                let _ = crate::CONTROL_DATA_PKT_CHANNEL.try_send(crate::ControlDataPkt {
                                    snr_x4,
                                    rssi: rssi.clamp(-128, 0) as i8,
                                    path_len: msg.path_len_byte,
                                    payload: payload_vec,
                                });
                            }
                            other => {
                                defmt::info!(
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

            Err(e) => {
                defmt::error!("LoRa RX error: {:?}", e);
                health_err!(lora, "LoRa RX error");
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Per-type handlers
// ---------------------------------------------------------------------------

async fn push_grp_txt(payload: &[u8], rssi: i16, snr_x4: i8, path_len: u8, channels: &[LoadedChannel]) {
    use meshcore::payload::grp_txt;

    let grp = match grp_txt::deserialize(payload) {
        Ok(g) => g,
        Err(_) => {
            defmt::warn!("GrpTxt: failed to parse payload");
            return;
        }
    };

    let ch = match channels.iter().find(|c| c.hash == grp.channel_hash) {
        Some(c) => c,
        None => {
            defmt::info!(
                "MeshCore GrpTxt [hash={=u8} {=i16}dBm] no matching channel (have: {=[u8]})",
                grp.channel_hash,
                rssi,
                &channels.iter().map(|c| c.hash).collect::<heapless::Vec<u8, { channels::NUM_CHANNELS }>>()[..],
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

            let content_hash = msg_hash(grp.channel_hash, text.as_bytes(), dec.timestamp);
            let is_new = MSG_SEEN.lock(|cell| {
                let mut ring = cell.borrow_mut();
                if ring.contains(content_hash) { false } else { ring.insert(content_hash); true }
            });
            if !is_new {
                defmt::debug!("GrpTxt: duplicate suppressed (hash={=u32:#010x})", content_hash);
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

            // Push to the flash queue and notify any connected BLE companion.
            let mut queued_text: heapless::Vec<u8, { msg_queue::MAX_TEXT }> = heapless::Vec::new();
            let _ = queued_text.extend_from_slice(&dec.text[..dec.text.len().min(msg_queue::MAX_TEXT)]);
            msg_queue::push(&msg_queue::ReceivedMsg {
                kind:          msg_queue::MsgKind::Channel,
                sender_prefix: [0u8; 6],
                channel_idx:   ch.slot_idx,
                path_len,
                text_type:     dec.text_type,
                timestamp:     dec.timestamp,
                rssi,
                text:          queued_text,
            }).await;
            defmt::info!("msg_queue: {} message(s) waiting", msg_queue::count());
            crate::MESSAGES_WAITING_SIGNAL.signal(());
        }
        Err(_) => {
            defmt::warn!("GrpTxt: decryption failed on channel slot {=u8}", ch.slot_idx);
        }
    }
}

async fn log_advert(
    payload:       &[u8],
    rssi:          i16,
    snr_x4:        i8,
    path_len_byte: u8,
    path:          &heapless::Vec<u8, { meshcore::MAX_PATH_SIZE }>,
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

    if let Some(ref name) = a.name {
        defmt::info!(
            "MeshCore advert [{=i16}dBm] role={=u8} name={=[u8]} sig_ok={=bool}",
            rssi, a.role.to_u8(), &name[..], sig_ok,
        );
    } else {
        defmt::info!(
            "MeshCore advert [{=i16}dBm] role={=u8} key={=[u8]} sig_ok={=bool}",
            rssi, a.role.to_u8(), &a.pub_key[..8], sig_ok,
        );
    }

    // Build name string (used both for contacts and display).
    let mut name_str: heapless::String<32> = heapless::String::new();
    if let Some(ref n) = a.name {
        let _ = name_str.push_str(core::str::from_utf8(n).unwrap_or("?"));
    }

    // Upsert into contacts list so TxtMsg can resolve the sender's name.
    CONTACTS.lock(|cell| cell.borrow_mut().upsert(a.pub_key, name_str.clone()));

    let mut pub_key_hex: heapless::String<16> = heapless::String::new();
    for &b in &a.pub_key[..8] {
        let hi = b >> 4;
        let lo = b & 0xF;
        let _ = pub_key_hex.push(if hi < 10 { (b'0' + hi) as char } else { (b'a' + hi - 10) as char });
        let _ = pub_key_hex.push(if lo < 10 { (b'0' + lo) as char } else { (b'a' + lo - 10) as char });
    }

    let (lat, lon) = a.position.unwrap_or((0, 0));

    crate::LAST_ADVERT.lock(|cell| {
        *cell.borrow_mut() = Some(crate::LastAdvert {
            name: name_str.clone(),
            pub_key_hex,
            role: a.role.to_u8(),
            sig_ok,
            rssi,
            snr_x4,
            lat,
            lon,
        });
    });
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

    let store = contacts::ContactStore::new();

    // Update routing path for this contact if it arrived via flood.
    if path_len_byte != contacts::OUT_PATH_UNKNOWN && !path.is_empty() {
        let mut path_buf = [0u8; contacts::MAX_PATH_SIZE];
        let copy_len = path.len().min(contacts::MAX_PATH_SIZE);
        path_buf[..copy_len].copy_from_slice(&path[..copy_len]);
        if let Err(e) = store.update_path(&a.pub_key, path_len_byte, &path_buf).await {
            defmt::warn!("contacts: path update failed: {:?}", e);
        }
    }
}

// ---------------------------------------------------------------------------
// TxtMsg (private message) handler
// ---------------------------------------------------------------------------

async fn log_txt_msg(
    lora:          &mut SimpleLoRa<'_>,
    payload:       &[u8],
    rssi:          i16,
    path_len_byte: u8,
    path:          &heapless::Vec<u8, { meshcore::MAX_PATH_SIZE }>,
    identity:      &DeviceIdentity,
) {
    use meshcore::payload::txt_msg;

    let msg = match txt_msg::deserialize(payload) {
        Ok(m) => m,
        Err(e) => {
            let reason = match e {
                meshcore::Error::TooShort  => "too short",
                meshcore::Error::TooLong   => "too long",
                meshcore::Error::Overflow  => "overflow",
                _                          => "other",
            };
            defmt::warn!("TxtMsg: failed to parse payload ({=usize}B): {=str}", payload.len(), reason);
            return;
        }
    };

    // Only process messages addressed to us (dest_hash = first byte of our pub_key).
    if msg.dest_hash != identity.pub_key[0] {
        defmt::debug!("TxtMsg: dest_hash={=u8:#04x} not ours, ignoring", msg.dest_hash);
        return;
    }

    // Scan ContactStore for a contact whose hash matches src_hash and can decrypt.
    let store = contacts::ContactStore::new();
    let count = store.count().await;
    let mut found: Option<(contacts::Contact, meshcore::payload::txt_msg::DecryptedTxtMsg, u32)> = None;

    let mut found_count = 0u16;
    for idx in 0..contacts::MAX_CONTACTS {
        if found_count >= count { break; }
        if let Some(c) = store.read_slot(idx).await {
            found_count += 1;
            // Quick hash pre-filter.
            if c.pub_key[0] != msg.src_hash { continue; }
            if txt_msg::verify_mac(&identity.sec_key, &c.pub_key, &msg).is_ok() {
                if let Ok((dec, ack_hash)) = txt_msg::decrypt(&identity.sec_key, &c.pub_key, &msg) {
                    found = Some((c, dec, ack_hash));
                    break;
                }
            }
        }
    }

    match found {
        None => {
            defmt::warn!("TxtMsg: received but could not decrypt (sender unknown or MAC fail) [{=i16}dBm]", rssi);
        }
        Some((sender, dec, ack_hash)) => {
            let text = core::str::from_utf8(&dec.text).unwrap_or("<invalid utf-8>");
            defmt::info!(
                "TxtMsg from {=[u8]:02x} [{=i16}dBm ts={=u32} type={=u8}]: {=str}",
                &sender.pub_key[..6], rssi, dec.timestamp, dec.txt_type(), text,
            );

            // Update the stored routing path so replies can go direct.
            if path_len_byte != contacts::OUT_PATH_UNKNOWN && !path.is_empty() {
                let mut path_buf = [0u8; contacts::MAX_PATH_SIZE];
                let copy_len = path.len().min(contacts::MAX_PATH_SIZE);
                path_buf[..copy_len].copy_from_slice(&path[..copy_len]);
                if let Err(e) = store.update_path(&sender.pub_key, path_len_byte, &path_buf).await {
                    defmt::warn!("TxtMsg: path update failed: {:?}", e);
                }
            }

            // Only push plain-text messages to the UI / message queue.
            // CLI_DATA and signed types are not handled yet.
            if dec.txt_type() == txt_msg::TXT_TYPE_PLAIN {
                // Push to the message queue so SYNC_NEXT_MESSAGE delivers it.
                let mut text_bytes: heapless::Vec<u8, { msg_queue::MAX_TEXT }> = heapless::Vec::new();
                let _ = text_bytes.extend_from_slice(
                    &dec.text[..dec.text.len().min(msg_queue::MAX_TEXT)]
                );
                let mut sender_prefix = [0u8; 6];
                sender_prefix.copy_from_slice(&sender.pub_key[..6]);
                msg_queue::push(&msg_queue::ReceivedMsg {
                    kind:          msg_queue::MsgKind::Private,
                    sender_prefix,
                    channel_idx:   0,
                    path_len:      path_len_byte,
                    text_type:     dec.txt_type(),
                    timestamp:     dec.timestamp,
                    rssi,
                    text:          text_bytes,
                }).await;
                crate::MESSAGES_WAITING_SIGNAL.signal(());

                // Send ACK back to the sender.
                send_ack(lora, &sender.pub_key, path_len_byte, path, ack_hash).await;

                // Update the display.
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
                        sender_name: display_name,
                        text: text_str,
                        timestamp: dec.timestamp,
                        rssi,
                    });
                });
                crate::PM_SIGNAL.signal(());
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Channel message transmission
// ---------------------------------------------------------------------------

/// Encrypt and broadcast a group-text message on channel slot `req.channel_idx`.
///
/// The channel key is looked up first from the already-loaded in-RAM table,
/// with a direct KV fallback for channels set after the last reload.
async fn send_grp_txt(
    lora: &mut SimpleLoRa<'_>,
    loaded_channels: &[LoadedChannel],
    req: crate::TxChannelMsg,
) {
    use meshcore::payload::grp_txt;
    use meshcore::packet::{Message, PayloadType, RouteType};
    use meshcore::{MAX_PAYLOAD_SIZE, MAX_TRANS_UNIT};

    // Resolve the channel key from the in-RAM table (kept current via CHANNELS_CHANGED_SIGNAL).
    let Some(ch) = loaded_channels.iter().find(|c| c.slot_idx == req.channel_idx) else {
        defmt::warn!("send_grp_txt: channel slot {=u8} not in RAM table, dropping", req.channel_idx);
        return;
    };
    let (key, hash) = (ch.key, ch.hash);

    // MeshCore GrpTxt wire format embeds the sender as "Name: MessageText".
    // Use the persisted node name, falling back to the 4-byte device-ID hex if unset.
    let mut name_buf = [0u8; settings::MAX_NODE_NAME];
    let name_len = {
        let n = settings::get_node_name(&mut name_buf).await;
        if n == 0 {
            let id = super::device_id::get_bytes();
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
            defmt::warn!("send_grp_txt: encrypt failed: {:?}", defmt::Debug2Format(&e));
            return;
        }
    };

    let mut payload_buf = [0u8; MAX_PAYLOAD_SIZE];
    let mut payload_len = 0usize;
    if let Err(e) = grp_txt::serialize(&grp, &mut payload_buf, &mut payload_len) {
        defmt::warn!("send_grp_txt: serialize failed: {:?}", defmt::Debug2Format(&e));
        return;
    }

    let mut msg_payload: heapless::Vec<u8, MAX_PAYLOAD_SIZE> = heapless::Vec::new();
    let _ = msg_payload.extend_from_slice(&payload_buf[..payload_len]);

    let msg = Message {
        payload_type:   PayloadType::GrpTxt,
        route:          RouteType::Flood,
        version:        0,
        transport_code: 0,
        path_len_byte:  0,
        path:           heapless::Vec::new(),
        payload:        msg_payload,
    };

    let mut frame = [0u8; MAX_TRANS_UNIT];
    match meshcore::packet::serialize(&msg, &mut frame) {
        Ok(len) => {
            if let Err(e) = lora.send_message(&frame[..len]).await {
                defmt::warn!("send_grp_txt: TX failed: {:?}", e);
            } else {
                defmt::info!(
                    "GrpTxt sent: ch={=u8} ts={=u32} len={=usize}B",
                    req.channel_idx, req.timestamp, len
                );

                // Seed MSG_SEEN so relay-bounces of our own packet are
                // suppressed.  We do NOT push to the companion queue — the
                // companion app already knows it sent this message.
                let content_hash = msg_hash(hash, &wire_text, req.timestamp);
                MSG_SEEN.lock(|cell| cell.borrow_mut().insert(content_hash));
            }
        }
        Err(e) => {
            defmt::warn!("send_grp_txt: packet serialize failed: {:?}", defmt::Debug2Format(&e));
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
async fn send_txt_msg(lora: &mut SimpleLoRa<'_>, req: crate::TxPrivateMsg, identity: &DeviceIdentity) {
    use meshcore::payload::txt_msg;
    use meshcore::packet::{Message, PayloadType, RouteType};
    use meshcore::{MAX_PAYLOAD_SIZE, MAX_TRANS_UNIT};

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
        _ => (RouteType::Flood, 0u8, heapless::Vec::new()),
    };

    let (encrypted, _expected_ack) = match txt_msg::encrypt(
        &identity.sec_key,
        &identity.pub_key,
        &req.recipient_pub_key,
        req.timestamp,
        0, // txt_type = plain
        0, // attempt
        &req.text,
    ) {
        Ok(e) => e,
        Err(e) => {
            defmt::warn!("send_txt_msg: encrypt failed: {:?}", defmt::Debug2Format(&e));
            return;
        }
    };

    let mut payload_buf = [0u8; MAX_PAYLOAD_SIZE];
    let mut payload_len = 0usize;
    if let Err(e) = txt_msg::serialize(&encrypted, &mut payload_buf, &mut payload_len) {
        defmt::warn!("send_txt_msg: serialize failed: {:?}", defmt::Debug2Format(&e));
        return;
    }

    let mut msg_payload: heapless::Vec<u8, MAX_PAYLOAD_SIZE> = heapless::Vec::new();
    let _ = msg_payload.extend_from_slice(&payload_buf[..payload_len]);

    let msg = Message {
        payload_type:   PayloadType::TxtMsg,
        route,
        version:        0,
        transport_code: 0,
        path_len_byte,
        path:           path_bytes,
        payload:        msg_payload,
    };

    let mut frame = [0u8; MAX_TRANS_UNIT];
    match meshcore::packet::serialize(&msg, &mut frame) {
        Ok(len) => {
            if let Err(e) = lora.send_message(&frame[..len]).await {
                defmt::warn!("send_txt_msg: TX failed: {:?}", e);
            } else {
                defmt::info!(
                    "TxtMsg sent: to={=[u8]:02x} route={=str} len={=usize}B ack={=u32:#010x}",
                    &req.recipient_pub_key[..6],
                    if route == RouteType::Direct { "direct" } else { "flood" },
                    len, _expected_ack,
                );
                // Record pending ACK so we can compute round-trip time when the mesh ACKs back.
                crate::PENDING_ACK.lock(|cell| {
                    cell.set(Some(crate::PendingAck {
                        ack_hash: _expected_ack,
                        sent_at:  embassy_time::Instant::now(),
                    }));
                });
            }
        }
        Err(e) => {
            defmt::warn!("send_txt_msg: packet serialize failed: {:?}", defmt::Debug2Format(&e));
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
    use meshcore::payload::advert::{Advert, DeviceRole, serialize};
    use meshcore::packet::{Message, PayloadType, RouteType};
    use meshcore::{MAX_PAYLOAD_SIZE, MAX_TRANS_UNIT};

    let mut advert = Advert {
        pub_key:   identity.pub_key,
        timestamp,
        signature: [0u8; meshcore::SIGNATURE_SIZE],
        role:      DeviceRole::ChatNode,
        name:      {
            let mut v = heapless::Vec::new();
            let _ = v.extend_from_slice(&name[..name.len().min(32)]);
            if v.is_empty() { None } else { Some(v) }
        },
        position:  None,
        extra1:    None,
        extra2:    None,
    };

    if let Err(e) = meshcore::identity::sign_advert(&identity.sec_key, &mut advert) {
        defmt::warn!("send_advert: signing failed: {:?}", defmt::Debug2Format(&e));
        return;
    }

    let mut payload_buf = [0u8; MAX_PAYLOAD_SIZE];
    let mut payload_len = 0usize;
    if let Err(e) = serialize(&advert, &mut payload_buf, &mut payload_len) {
        defmt::warn!("send_advert: serialize failed: {:?}", defmt::Debug2Format(&e));
        return;
    }

    let mut msg_payload: heapless::Vec<u8, MAX_PAYLOAD_SIZE> = heapless::Vec::new();
    let _ = msg_payload.extend_from_slice(&payload_buf[..payload_len]);

    let route = match mode {
        AdvertMode::Flood   => RouteType::Flood,
        AdvertMode::ZeroHop => RouteType::Direct, // path_len=0 → zero-hop direct
    };

    let msg = Message {
        payload_type:   PayloadType::Advert,
        route,
        version:        0,
        transport_code: 0,
        path_len_byte:  0,
        path:           heapless::Vec::new(),
        payload:        msg_payload,
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
                    if mode == AdvertMode::Flood { "flood" } else { "zero-hop" },
                );
            }
        }
        Err(e) => {
            defmt::warn!("send_advert: packet serialize failed: {:?}", defmt::Debug2Format(&e));
        }
    }
}

// ---------------------------------------------------------------------------
// PM (TxtMsg) transmission
// ---------------------------------------------------------------------------

/// Encrypt and send a private message to `recipient_pk`.
///
/// The recipient must have previously broadcast an advert so their key is
/// known to the mesh.  `text` is plain UTF-8, max [`meshcore::payload::txt_msg::MAX_TXT_TEXT_SIZE`] bytes.
pub async fn send_pm(
    lora: &mut SimpleLoRa<'_>,
    identity: &DeviceIdentity,
    recipient_pk: &[u8; meshcore::PUB_KEY_SIZE],
    text: &[u8],
    timestamp: u32,
) {
    use meshcore::payload::txt_msg;
    use meshcore::packet::{Message, PayloadType, RouteType};
    use meshcore::{MAX_PAYLOAD_SIZE, MAX_TRANS_UNIT};

    let (msg, expected_ack) = match txt_msg::encrypt(
        &identity.sec_key, &identity.pub_key, recipient_pk,
        timestamp, txt_msg::TXT_TYPE_PLAIN, 0, text,
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
    let packet = Message {
        payload_type:   PayloadType::TxtMsg,
        route:          RouteType::Flood,
        version:        0,
        transport_code: 0,
        path_len_byte:  0,
        path:           heapless::Vec::new(),
        payload:        msg_payload,
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
            defmt::warn!("send_pm: packet serialize failed: {:?}", defmt::Debug2Format(&e));
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
    lora:          &mut SimpleLoRa<'_>,
    sender_pk:     &[u8; meshcore::PUB_KEY_SIZE],
    _recv_path_len: u8,
    _recv_path:    &heapless::Vec<u8, { meshcore::MAX_PATH_SIZE }>,
    ack_hash:      u32,
) {
    use meshcore::packet::{Message, PayloadType, RouteType};
    use meshcore::MAX_TRANS_UNIT;

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
        _ => (RouteType::Flood, 0u8, heapless::Vec::new()),
    };

    let msg = Message {
        payload_type:   PayloadType::Ack,
        route,
        version:        0,
        transport_code: 0,
        path_len_byte,
        path:           path_bytes,
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
        defmt::warn!("handle_ack_recv: payload too short ({=usize}B)", payload.len());
        return;
    }
    let ack_crc = u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
    defmt::info!("MeshCore Ack: ack_crc={=u32:#010x} [{=i16}dBm]", ack_crc, rssi);

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

    let _ = crate::ACK_EVENT_CHANNEL.try_send(crate::AckEvent { ack_crc, trip_time_ms });
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
        payload_type:   PayloadType::Trace,
        route:          RouteType::Direct,
        version:        0,
        transport_code: 0,
        path_len_byte:  0,
        path:           heapless::Vec::new(),
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
                    req.tag, req.path.len(), len,
                );
            }
        }
        Err(e) => {
            defmt::warn!("send_trace: packet serialize failed: {:?}", defmt::Debug2Format(&e));
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
/// This mirrors C++ `BaseChatMesh::sendLogin`: flood when `out_path_len == OUT_PATH_UNKNOWN`.
async fn send_login(lora: &mut SimpleLoRa<'_>, req: crate::TxLogin, identity: &DeviceIdentity) {
    use meshcore::packet::{Message, PayloadType, RouteType};
    use meshcore::MAX_TRANS_UNIT;

    let timestamp = crate::unix_now().unwrap_or(0);

    let payload = match meshcore::payload::anon_req::encrypt(
        &identity.sec_key,
        &identity.pub_key,
        &req.pub_key,
        timestamp,
        &req.password,
    ) {
        Ok(p) => p,
        Err(e) => {
            defmt::warn!("send_login: encrypt failed: {:?}", defmt::Debug2Format(&e));
            return;
        }
    };

    // Flood when no path is known; direct when a stored path exists.
    let contact = contacts::ContactStore::new().find_by_key(&req.pub_key).await;
    let (route, path_len_byte, path_bytes) = match contact {
        Some(ref c) if c.out_path_len != contacts::OUT_PATH_UNKNOWN => {
            let actual = c.path_actual_bytes();
            let mut pv: heapless::Vec<u8, { meshcore::MAX_PATH_SIZE }> = heapless::Vec::new();
            let _ = pv.extend_from_slice(&c.out_path[..actual]);
            (RouteType::Direct, c.out_path_len, pv)
        }
        _ => (RouteType::Flood, 0u8, heapless::Vec::new()),
    };

    let msg = Message {
        payload_type:   PayloadType::AnonReq,
        route,
        version:        0,
        transport_code: 0,
        path_len_byte,
        path:           path_bytes,
        payload,
    };

    let mut frame = [0u8; MAX_TRANS_UNIT];
    match meshcore::packet::serialize(&msg, &mut frame) {
        Ok(len) => {
            if let Err(e) = lora.send_message(&frame[..len]).await {
                defmt::warn!("send_login: TX failed: {:?}", e);
            } else {
                defmt::info!("Login sent to {=[u8]:02x} ({=usize}B)", &req.pub_key[..6], len);
            }
        }
        Err(e) => {
            defmt::warn!("send_login: packet serialize failed: {:?}", defmt::Debug2Format(&e));
        }
    }
}

// ---------------------------------------------------------------------------
// Status request transmission
// ---------------------------------------------------------------------------

/// Build and transmit a STATUS REQUEST — same `AnonReq` wire format as login
/// but with an empty password.  The server sees `data[4] == 0` (no password
/// byte) and responds with uptime + battery instead of a login result.
async fn send_status_request(
    lora: &mut SimpleLoRa<'_>,
    req: crate::TxStatusReq,
    identity: &DeviceIdentity,
) {
    use meshcore::packet::{Message, PayloadType, RouteType};
    use meshcore::MAX_TRANS_UNIT;

    let timestamp = crate::unix_now().unwrap_or(0);

    let payload = match meshcore::payload::anon_req::encrypt(
        &identity.sec_key,
        &identity.pub_key,
        &req.pub_key,
        timestamp,
        &[], // empty password = status/ping request
    ) {
        Ok(p) => p,
        Err(e) => {
            defmt::warn!("send_status_req: encrypt failed: {:?}", defmt::Debug2Format(&e));
            return;
        }
    };

    let contact = contacts::ContactStore::new().find_by_key(&req.pub_key).await;
    let (route, path_len_byte, path_bytes) = match contact {
        Some(ref c) if c.out_path_len != contacts::OUT_PATH_UNKNOWN => {
            let actual = c.path_actual_bytes();
            let mut pv: heapless::Vec<u8, { meshcore::MAX_PATH_SIZE }> = heapless::Vec::new();
            let _ = pv.extend_from_slice(&c.out_path[..actual]);
            (RouteType::Direct, c.out_path_len, pv)
        }
        _ => (RouteType::Flood, 0u8, heapless::Vec::new()),
    };

    let msg = Message {
        payload_type:   PayloadType::AnonReq,
        route,
        version:        0,
        transport_code: 0,
        path_len_byte,
        path:           path_bytes,
        payload,
    };

    let mut frame = [0u8; MAX_TRANS_UNIT];
    match meshcore::packet::serialize(&msg, &mut frame) {
        Ok(len) => {
            // Record as pending BEFORE transmitting so the response handler can match it.
            crate::PENDING_STATUS_PUBKEY.lock(|cell| cell.set(Some(req.pub_key)));
            if let Err(e) = lora.send_message(&frame[..len]).await {
                defmt::warn!("send_status_req: TX failed: {:?}", e);
                crate::PENDING_STATUS_PUBKEY.lock(|cell| cell.set(None));
            } else {
                defmt::info!("Status req sent to {=[u8]:02x} ({=usize}B)", &req.pub_key[..6], len);
            }
        }
        Err(e) => {
            defmt::warn!("send_status_req: packet serialize failed: {:?}", defmt::Debug2Format(&e));
        }
    }
}

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
    use meshcore::packet::{Message, PayloadType, RouteType};
    use meshcore::MAX_TRANS_UNIT;

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
        0,                   // txt_type = 0 → upper bits of flags = 0
        REQ_TYPE_TELEMETRY,  // attempt field → flags & 3 = 0x03
        &text,
    ) {
        Ok(r) => r,
        Err(e) => {
            defmt::warn!("send_telem_req: encrypt failed: {:?}", defmt::Debug2Format(&e));
            return;
        }
    };

    let mut payload_buf = [0u8; meshcore::MAX_PAYLOAD_SIZE];
    let mut payload_len = 0usize;
    if let Err(e) = meshcore::payload::txt_msg::serialize(&encrypted, &mut payload_buf, &mut payload_len) {
        defmt::warn!("send_telem_req: serialize failed: {:?}", defmt::Debug2Format(&e));
        return;
    }

    let contact = contacts::ContactStore::new().find_by_key(&req.pub_key).await;
    let (route, path_len_byte, path_bytes) = match contact {
        Some(ref c) if c.out_path_len != contacts::OUT_PATH_UNKNOWN => {
            let actual = c.path_actual_bytes();
            let mut pv: heapless::Vec<u8, { meshcore::MAX_PATH_SIZE }> = heapless::Vec::new();
            let _ = pv.extend_from_slice(&c.out_path[..actual]);
            (RouteType::Direct, c.out_path_len, pv)
        }
        _ => (RouteType::Flood, 0u8, heapless::Vec::new()),
    };

    let mut payload_vec: heapless::Vec<u8, { meshcore::MAX_PAYLOAD_SIZE }> = heapless::Vec::new();
    let _ = payload_vec.extend_from_slice(&payload_buf[..payload_len]);

    let msg = Message {
        payload_type:   PayloadType::Req,
        route,
        version:        0,
        transport_code: 0,
        path_len_byte,
        path:           path_bytes,
        payload:        payload_vec,
    };

    let mut frame = [0u8; MAX_TRANS_UNIT];
    match meshcore::packet::serialize(&msg, &mut frame) {
        Ok(len) => {
            crate::PENDING_TELEM_TAG.lock(|cell| cell.set(Some(req.tag)));
            if let Err(e) = lora.send_message(&frame[..len]).await {
                defmt::warn!("send_telem_req: TX failed: {:?}", e);
                crate::PENDING_TELEM_TAG.lock(|cell| cell.set(None));
            } else {
                defmt::info!("Telem req sent to {=[u8]:02x} tag={=u32:#010x} ({=usize}B)",
                    &req.pub_key[..6], req.tag, len);
            }
        }
        Err(e) => {
            defmt::warn!("send_telem_req: packet serialize failed: {:?}", defmt::Debug2Format(&e));
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
    use meshcore::packet::{Message, PayloadType, RouteType};
    use meshcore::MAX_TRANS_UNIT;

    // Discovery uses flags = 0x03 (REQ_TYPE_GET_TELEMETRY_DATA), same as telemetry,
    // but the text body starts with 0xFE (~TELEM_PERM_BASE) to signal discovery intent.
    // txt_msg::encrypt: flags = (attempt & 3) | (txt_type << 2) → attempt=3, txt_type=0.
    // text = [0xFE, 0, 0, 0, 0, 0, 0, 0] (perm byte + 7 padding/random zeros).
    const REQ_TYPE_DISCOVERY: u8 = 0x03;
    let text: [u8; 8] = [0xFE, 0, 0, 0, 0, 0, 0, 0];

    let (encrypted, _ack_hash) = match meshcore::payload::txt_msg::encrypt(
        &identity.sec_key,
        &identity.pub_key,
        &req.pub_key,
        req.tag,
        0,                    // txt_type = 0
        REQ_TYPE_DISCOVERY,   // attempt → flags & 3 = 0x03
        &text,
    ) {
        Ok(r) => r,
        Err(e) => {
            defmt::warn!("send_discovery_req: encrypt failed: {:?}", defmt::Debug2Format(&e));
            return;
        }
    };

    let mut payload_buf = [0u8; meshcore::MAX_PAYLOAD_SIZE];
    let mut payload_len = 0usize;
    if let Err(e) = meshcore::payload::txt_msg::serialize(&encrypted, &mut payload_buf, &mut payload_len) {
        defmt::warn!("send_discovery_req: serialize failed: {:?}", defmt::Debug2Format(&e));
        return;
    }

    let mut payload_vec: heapless::Vec<u8, { meshcore::MAX_PAYLOAD_SIZE }> = heapless::Vec::new();
    let _ = payload_vec.extend_from_slice(&payload_buf[..payload_len]);

    // Always flood for discovery — the whole point is to find new paths.
    let msg = Message {
        payload_type:   PayloadType::Req,
        route:          RouteType::Flood,
        version:        0,
        transport_code: 0,
        path_len_byte:  0,
        path:           heapless::Vec::new(),
        payload:        payload_vec,
    };

    let mut frame = [0u8; MAX_TRANS_UNIT];
    match meshcore::packet::serialize(&msg, &mut frame) {
        Ok(len) => {
            crate::PENDING_DISCOVERY_TAG.lock(|cell| cell.set(Some(req.tag)));
            if let Err(e) = lora.send_message(&frame[..len]).await {
                defmt::warn!("send_discovery_req: TX failed: {:?}", e);
                crate::PENDING_DISCOVERY_TAG.lock(|cell| cell.set(None));
            } else {
                defmt::info!("Discovery req sent to {=[u8]:02x} tag={=u32:#010x} ({=usize}B) [flood]",
                    &req.pub_key[..6], req.tag, len);
            }
        }
        Err(e) => {
            defmt::warn!("send_discovery_req: packet serialize failed: {:?}", defmt::Debug2Format(&e));
        }
    }
}

// ---------------------------------------------------------------------------
// Trace-path receive handler
// ---------------------------------------------------------------------------

/// Handle a received `PayloadType::Trace` packet.
///
/// When a TRACE packet is reflected back to us by the relay, the packet contains:
/// - `payload` = `[tag:4 LE][auth:4 LE][flags:1][route_hashes...]`
///   — the original route hashes embedded in the payload by `sendDirect`.
/// - `pkt_path` = `[snr_relay1, snr_relay2, ...]`
///   — per-hop SNRs appended by each relay that forwarded the packet.
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

    let tag       = u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
    let auth_code = u32::from_le_bytes([payload[4], payload[5], payload[6], payload[7]]);
    let flags = payload[8];

    // payload[9..] = route path hashes embedded by the sender.
    let route_hashes = &payload[9..];

    // path_len counts the route hashes in units of hash_size bytes.
    let path_sz   = (flags & 0x03) as usize; // 0=1B, 1=2B, 2=4B per hash entry
    let hash_size = path_sz + 1;
    let path_len  = (route_hashes.len() / hash_size.max(1)) as u8;

    // The per-hop relay SNRs come from the packet's path field (appended by each relay).
    let relay_snrs = pkt_path.as_slice();

    defmt::info!(
        "Trace recv: tag={=u32:#010x} auth={=u32:#010x} flags={=u8:#04x} path_len={=u8} relay_snrs={=usize}B final_snr={=i8}",
        tag, auth_code, flags, path_len, relay_snrs.len(), snr_x4,
    );

    let mut path_hashes: heapless::Vec<u8, { meshcore::MAX_PATH_SIZE }> = heapless::Vec::new();
    let mut path_snrs:   heapless::Vec<u8, { meshcore::MAX_PATH_SIZE }> = heapless::Vec::new();
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
        defmt::debug!("Response recv: dest_hash={=u8:#04x} not ours, ignoring", msg.dest_hash);
        return;
    }

    // Try each stored contact as the potential sender (server).
    let store = contacts::ContactStore::new();
    let count = store.count().await;
    let mut found_count = 0u16;
    for idx in 0..contacts::MAX_CONTACTS {
        if found_count >= count { break; }
        let Some(c) = store.read_slot(idx).await else { continue; };
        found_count += 1;

        if c.pub_key[0] != msg.src_hash { continue; }
        if txt_msg::verify_mac(&identity.sec_key, &c.pub_key, &msg).is_err() {
            continue;
        }
        let Ok((dec, _ack_hash)) = txt_msg::decrypt(&identity.sec_key, &c.pub_key, &msg) else {
            continue;
        };

        // Decrypted plaintext layout (same as C++ onContactResponse `data` arg):
        //   [0..4]  = tag / timestamp (u32 LE)      → dec.timestamp
        //   [4]     = resp_type                      → dec.text_type
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
        // In the Response payload, dec.flags = resp_type byte, dec.text = body after flags.
        let resp_type = dec.flags;
        let mut pub_key = [0u8; meshcore::PUB_KEY_SIZE];
        pub_key.copy_from_slice(&c.pub_key);

        // Check if this is a response to a pending status request.
        let pending_status = crate::PENDING_STATUS_PUBKEY.lock(|cell| cell.get());
        if let Some(pending_key) = pending_status {
            if pending_key == pub_key {
                crate::PENDING_STATUS_PUBKEY.lock(|cell| cell.set(None));

                // Status pong body: [uptime:4 LE][battery_mv:2 LE]
                let uptime_secs = if dec.text.len() >= 4 {
                    u32::from_le_bytes([
                        dec.text.get(0).copied().unwrap_or(0),
                        dec.text.get(1).copied().unwrap_or(0),
                        dec.text.get(2).copied().unwrap_or(0),
                        dec.text.get(3).copied().unwrap_or(0),
                    ])
                } else { 0 };
                let battery_mv = if dec.text.len() >= 6 {
                    u16::from_le_bytes([
                        dec.text.get(4).copied().unwrap_or(0),
                        dec.text.get(5).copied().unwrap_or(0),
                    ])
                } else { 0 };

                defmt::info!(
                    "Response recv: STATUS from {=[u8]:02x} resp_type={=u8} uptime={=u32}s batt={=u16}mV",
                    &c.pub_key[..6], resp_type, uptime_secs, battery_mv,
                );

                let _ = crate::STATUS_RESULT_CHANNEL.try_send(crate::StatusResult {
                    pub_key,
                    uptime_secs,
                    battery_mv,
                });
                return;
            }
        }

        // Check if this is a response to a pending telemetry request (tag-based match).
        let pending_telem = crate::PENDING_TELEM_TAG.lock(|cell| cell.get());
        if let Some(telem_tag) = pending_telem {
            if dec.timestamp == telem_tag {
                crate::PENDING_TELEM_TAG.lock(|cell| cell.set(None));

                // CayenneLPP = flags_byte (= data[4]) followed by dec.text (= data[5..]).
                let mut lpp: heapless::Vec<u8, 176> = heapless::Vec::new();
                let _ = lpp.push(dec.flags);
                let _ = lpp.extend_from_slice(&dec.text);

                defmt::info!(
                    "Response recv: TELEM from {=[u8]:02x} lpp={=usize}B",
                    &c.pub_key[..6], lpp.len(),
                );

                let _ = crate::TELEM_RESULT_CHANNEL.try_send(crate::TelemResult {
                    pub_key,
                    lpp,
                });
                return;
            }
        }

        // No pending status or telemetry request — treat as a login response.
        let success = resp_type == 0
            || (resp_type == b'O' && dec.text.first() == Some(&b'K'));

        defmt::info!(
            "Response recv: login {} from {=[u8]:02x} resp_type={=u8}",
            if success { "OK" } else { "FAIL" },
            &c.pub_key[..6],
            resp_type,
        );

        let tag = dec.timestamp;

        let is_admin      = if success { dec.text.get(1).copied().unwrap_or(0) } else { 0 };
        let acl_perms     = if success { dec.text.get(2).copied().unwrap_or(0) } else { 0 };
        let fw_ver_level  = if success { dec.text.get(7).copied().unwrap_or(0) } else { 0 };

        let _ = crate::LOGIN_RESULT_CHANNEL.try_send(crate::LoginResult {
            success,
            is_admin,
            pub_key,
            tag,
            acl_perms,
            fw_ver_level,
        });
        return; // Only process the first matching contact.
    }

    defmt::debug!("Response recv: could not decrypt with any known contact");
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
            defmt::warn!("Path recv: failed to parse payload ({=usize}B)", payload.len());
            return;
        }
    };

    // Quick check: is this addressed to us?
    if msg.dest_hash != identity.pub_key[0] {
        defmt::debug!(
            "Path recv: dest_hash={=u8:#04x} not ours ({=u8:#04x}), ignoring",
            msg.dest_hash, identity.pub_key[0],
        );
        return;
    }

    defmt::info!(
        "Path recv: dest={=u8:#04x} src={=u8:#04x} [{=i16}dBm]",
        msg.dest_hash, msg.src_hash, rssi,
    );

    // Scan contacts for a matching src_hash and try to decrypt.
    let store = contacts::ContactStore::new();
    let count = store.count().await;
    let mut found_count = 0u16;

    for idx in 0..contacts::MAX_CONTACTS {
        if found_count >= count { break; }
        let Some(c) = store.read_slot(idx).await else { continue; };
        found_count += 1;

        if c.pub_key[0] != msg.src_hash { continue; }

        let dec = match path_msg::verify_and_decrypt(&identity.sec_key, &c.pub_key, &msg) {
            Ok(d) => d,
            Err(meshcore::Error::MacMismatch) => continue, // wrong contact
            Err(e) => {
                defmt::warn!(
                    "Path recv: decrypt failed for {=[u8]:02x}: {:?}",
                    &c.pub_key[..6],
                    defmt::Debug2Format(&e),
                );
                continue;
            }
        };

        defmt::info!(
            "Path recv: decrypted from {=[u8]:02x} path_len={=u8} extra_type={=u8} extra_len={=usize}",
            &c.pub_key[..6], dec.path_len_byte, dec.extra_type, dec.extra.len(),
        );

        // Update the stored routing path for this contact.
        if dec.path_len_byte != contacts::OUT_PATH_UNKNOWN && !dec.path.is_empty() {
            let mut path_buf = [0u8; contacts::MAX_PATH_SIZE];
            let copy = dec.path.len().min(contacts::MAX_PATH_SIZE);
            path_buf[..copy].copy_from_slice(&dec.path[..copy]);
            if let Err(e) = store.update_path(&c.pub_key, dec.path_len_byte, &path_buf).await {
                defmt::warn!("Path recv: path update failed: {:?}", e);
            }
        }

        // PAYLOAD_TYPE_RESPONSE = 1 → either a discovery response (tag-based) or a login response.
        if dec.extra_type == 1 {
            // Check for a pending discovery request first (tag = extra[0..4]).
            let pending_disc = crate::PENDING_DISCOVERY_TAG.lock(|cell| cell.get());
            if let Some(disc_tag) = pending_disc {
                if dec.extra.len() >= 4 {
                    let resp_tag = u32::from_le_bytes([dec.extra[0], dec.extra[1], dec.extra[2], dec.extra[3]]);
                    if resp_tag == disc_tag {
                        crate::PENDING_DISCOVERY_TAG.lock(|cell| cell.set(None));

                        let mut out_path: heapless::Vec<u8, { meshcore::MAX_PATH_SIZE }> = heapless::Vec::new();
                        let _ = out_path.extend_from_slice(&dec.path);
                        let in_path: heapless::Vec<u8, { meshcore::MAX_PATH_SIZE }> = heapless::Vec::new();
                        // msg.path is not accessible here; we push empty (in_path not available in path_recv)
                        // The C++ comment says dec.path = out_path (return route).
                        // We store what we have: out_path from dec.path, in_path empty.

                        let mut pub_key = [0u8; meshcore::PUB_KEY_SIZE];
                        pub_key.copy_from_slice(&c.pub_key);

                        defmt::info!(
                            "Path recv: DISCOVERY response from {=[u8]:02x} tag={=u32:#010x} out_path_len={=u8}",
                            &c.pub_key[..6], disc_tag, dec.path_len_byte,
                        );

                        let _ = crate::DISCOVERY_RESULT_CHANNEL.try_send(crate::DiscoveryResult {
                            pub_key,
                            out_path_len_byte: dec.path_len_byte,
                            out_path,
                            in_path_len_byte: 0xFF,
                            in_path,
                        });
                        return;
                    }
                }
            }
            handle_path_login_response(&dec.extra, &c.pub_key);
        }

        return;
    }

    defmt::debug!("Path recv: no matching contact for src_hash={=u8:#04x}", msg.src_hash);
}

/// Extract and push a login result from path-packet extra bytes.
///
/// Extra byte layout (same as the plaintext seen by C++ `onContactResponse`):
///   `[timestamp:4 LE][resp_type:1][keep_alive:1][is_admin:1][acl_perms:1][random:4][fw_ver_level:1]`
fn handle_path_login_response(extra: &[u8], sender_pub_key: &[u8; meshcore::PUB_KEY_SIZE]) {
    if extra.len() < path_msg::LOGIN_RESPONSE_EXTRA_LEN {
        defmt::warn!(
            "Path login response: extra too short ({=usize}B, need {=usize}B)",
            extra.len(), path_msg::LOGIN_RESPONSE_EXTRA_LEN,
        );
        return;
    }

    use meshcore::payload::path_msg;

    let tag          = u32::from_le_bytes([extra[0], extra[1], extra[2], extra[3]]);
    let resp_type    = extra[4];
    // extra[5] = keep_alive / 16
    let is_admin     = extra[6];
    let acl_perms    = extra[7];
    // extra[8..12] = random
    let fw_ver_level = extra[12];

    // RESP_SERVER_LOGIN_OK = 0 (new format).
    // Legacy: resp_type == b'O' and extra[5] == b'K'.
    let success = resp_type == 0
        || (resp_type == b'O' && extra.get(5) == Some(&b'K'));

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
        is_admin:      if success { is_admin     } else { 0 },
        pub_key,
        tag,
        acl_perms:     if success { acl_perms    } else { 0 },
        fw_ver_level:  if success { fw_ver_level } else { 0 },
    });
}

// ---------------------------------------------------------------------------
// Control data transmission (CMD_SEND_CONTROL_DATA, 0x37)
// ---------------------------------------------------------------------------

/// Transmit a raw PAYLOAD_TYPE_CONTROL (0x0B) packet zero-hop (direct, path_len=0).
///
/// The `req.payload` slice starts with the `ctl_type` byte (e.g. 0x80 =
/// CTL_TYPE_NODE_DISCOVER_REQ) and may carry additional bytes.  This mirrors
/// `Mesh::createControlData` + `sendZeroHop` from the reference C firmware.
async fn send_control_data(lora: &mut SimpleLoRa<'_>, req: crate::TxControlData) {
    use meshcore::packet::{Message, PayloadType, RouteType};
    use meshcore::MAX_TRANS_UNIT;

    let msg = Message {
        payload_type:   PayloadType::Unknown(0x0B), // PAYLOAD_TYPE_CONTROL
        route:          RouteType::Direct,           // zero-hop (path_len_byte = 0)
        version:        0,
        transport_code: 0,
        path_len_byte:  0,
        path:           heapless::Vec::new(),
        payload:        req.payload,
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
