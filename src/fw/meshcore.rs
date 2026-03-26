use core::cell::RefCell;

use embassy_nrf::{Peri, gpio::AnyPin, peripherals};
use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_time::Timer;

use super::device_identity::DeviceIdentity;
use super::health::SYSTEM_HEALTH;
use super::sx1262::{MeshCoreConfig, SimpleLoRa};
use super::{channels, msg_queue};
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
// MeshCore listener task
// ---------------------------------------------------------------------------

/// Listen for MeshCore packets on the SX1262 and store decoded messages.
///
/// Configures the SX1262 using [`MeshCoreConfig::UK_NARROW_BAND`] and enters
/// a continuous receive loop.  Every received packet is parsed with the
/// `meshcore` vendor crate.  Group-text messages (`GrpTxt`) are decoded,
/// deduplicated, and stored in `LAST_LORA_MSG`; node advertisements are
/// logged; all other types are logged as raw hex.
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

    let config = &MeshCoreConfig::UK_NARROW_BAND;

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

    defmt::info!(
        "MeshCore listener ready — freq={=u32}Hz BW=62.5kHz SF=8 CR=4/5 sync={=u16:#06x} preamble={=u16}",
        config.frequency_hz,
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

        // Drain any already-queued outgoing messages before entering RX.
        while let Ok(req) = crate::TX_MSG_CHANNEL.try_receive() {
            send_grp_txt(&mut lora, &loaded_channels, req).await;
        }

        // Race: receive the next LoRa packet OR a new TX request arrives.
        // This keeps TX latency to the radio air-time only, instead of up to
        // the full 15-second receive_packet timeout.
        use embassy_futures::select::{Either, select};
        let rx_result = match select(
            lora.receive_packet(&mut raw),
            crate::TX_MSG_CHANNEL.receive(),
        ).await {
            Either::Second(tx_req) => {
                // A TX request interrupted RX — send it immediately and loop.
                send_grp_txt(&mut lora, &loaded_channels, tx_req).await;
                continue;
            }
            Either::First(result) => result,
        };

        match rx_result {
            Ok(None) => { /* timeout or CRC error — already re-armed */ }

            Ok(Some((len, rssi))) => {
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
                        use meshcore::packet::PayloadType;
                        let path_len = msg.path.len() as u8;
                        match msg.payload_type {
                            PayloadType::GrpTxt => push_grp_txt(&msg.payload, rssi, path_len, &loaded_channels).await,
                            PayloadType::TxtMsg => log_txt_msg(&msg.payload, rssi, identity),
                            PayloadType::Advert => log_advert(&msg.payload, rssi),
                            PayloadType::Ack => defmt::info!("MeshCore Ack [{=i16}dBm]", rssi),
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

async fn push_grp_txt(payload: &[u8], rssi: i16, path_len: u8, channels: &[LoadedChannel]) {
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
                &channels.iter().map(|c| c.hash).collect::<heapless::Vec<u8, 8>>()[..],
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

            // Deduplicate: hash channel_hash + decrypted text + timestamp.
            let hash = msg_hash(grp.channel_hash, text.as_bytes(), dec.timestamp);
            let already_seen = MSG_SEEN.lock(|cell| {
                let mut ring = cell.borrow_mut();
                if ring.contains(hash) { true } else { ring.insert(hash); false }
            });
            if already_seen {
                defmt::debug!("GrpTxt: duplicate suppressed (hash={=u32:#010x})", hash);
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
                });
            });
            crate::LORA_MSG_SIGNAL.signal(());

            // Push to the flash queue and notify any connected BLE companion.
            let mut queued_text: heapless::Vec<u8, { msg_queue::MAX_TEXT }> = heapless::Vec::new();
            let _ = queued_text.extend_from_slice(&dec.text[..dec.text.len().min(msg_queue::MAX_TEXT)]);
            msg_queue::push(&msg_queue::ReceivedChannelMsg {
                channel_idx: ch.slot_idx,
                path_len,
                text_type: dec.text_type,
                timestamp: dec.timestamp,
                rssi,
                text: queued_text,
            }).await;
            defmt::info!("msg_queue: {} message(s) waiting", msg_queue::count());
            crate::MESSAGES_WAITING_SIGNAL.signal(());
        }
        Err(_) => {
            defmt::warn!("GrpTxt: decryption failed on channel slot {=u8}", ch.slot_idx);
        }
    }
}

fn log_advert(payload: &[u8], rssi: i16) {
    use meshcore::payload::advert;

    let a = match advert::deserialize(payload) {
        Ok(a) => a,
        Err(_) => {
            defmt::warn!("Advert: failed to parse payload");
            return;
        }
    };

    let sig_ok = meshcore::identity::verify_advert(&a).is_ok();

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

    crate::LAST_ADVERT.lock(|cell| {
        *cell.borrow_mut() = Some(crate::LastAdvert {
            name: name_str,
            pub_key_hex,
            role: a.role.to_u8(),
            sig_ok,
            rssi,
        });
    });
    crate::ADVERT_SIGNAL.signal(());
}

// ---------------------------------------------------------------------------
// TxtMsg (private message) handler
// ---------------------------------------------------------------------------

fn log_txt_msg(payload: &[u8], rssi: i16, identity: &DeviceIdentity) {
    use meshcore::payload::txt_msg;

    let msg = match txt_msg::deserialize(payload) {
        Ok(m) => m,
        Err(_) => {
            defmt::warn!("TxtMsg: failed to parse payload");
            return;
        }
    };

    // Only process messages addressed to us.
    if msg.dest_pub_key != identity.pub_key {
        defmt::debug!("TxtMsg: not for us, ignoring");
        return;
    }

    // Try to decrypt using each known contact as the potential sender.
    type DecResult = Option<(heapless::String<32>, [u8; meshcore::PUB_KEY_SIZE], meshcore::payload::txt_msg::DecryptedTxtMsg)>;
    let result: DecResult = CONTACTS.lock(|cell| {
        let contacts = cell.borrow();
        for contact in contacts.iter() {
            if txt_msg::verify_mac(&identity.sec_key, &contact.pub_key, &msg).is_ok() {
                if let Ok(dec) = txt_msg::decrypt(&identity.sec_key, &contact.pub_key, &msg) {
                    return Some((contact.name.clone(), contact.pub_key, dec));
                }
            }
        }
        None
    });

    match result {
        None => {
            defmt::warn!("TxtMsg: received but could not decrypt (sender unknown or MAC fail) [{=i16}dBm]", rssi);
        }
        Some((sender_name, sender_pk, dec)) => {
            let text = core::str::from_utf8(&dec.text).unwrap_or("<invalid utf-8>");
            defmt::info!(
                "TxtMsg from {=str} [{=i16}dBm ts={=u32}]: {=str}",
                sender_name.as_str(),
                rssi,
                dec.timestamp,
                text,
            );

            // Fallback name: first 8 bytes of pub_key as hex.
            let display_name = if sender_name.is_empty() {
                let mut hex: heapless::String<32> = heapless::String::new();
                for &b in &sender_pk[..4] {
                    let _ = hex.push(char::from_digit((b >> 4) as u32, 16).unwrap_or('?'));
                    let _ = hex.push(char::from_digit((b & 0xF) as u32, 16).unwrap_or('?'));
                }
                hex
            } else {
                sender_name
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

    // Resolve the channel key — prefer the in-RAM table, fall back to KV.
    let (key, hash) = if let Some(ch) = loaded_channels.iter().find(|c| c.slot_idx == req.channel_idx) {
        (ch.key, ch.hash)
    } else if let Some((_, key)) = channels::get(req.channel_idx).await {
        let hash = hash_from_key(&key);
        (key, hash)
    } else {
        defmt::warn!("send_grp_txt: channel slot {=u8} not found, dropping", req.channel_idx);
        return;
    };

    // MeshCore GrpTxt wire format embeds the sender as "Name: MessageText".
    // Build the prefixed wire text: "{device_id}: {message}".
    // MAX_GRP_DATA_SIZE = 181 bytes; prefix is 6 bytes ("XXYY: "), leaving 175 for the body.
    let device_id = super::device_id::get_bytes();
    let mut wire_text: heapless::Vec<u8, { meshcore::MAX_GRP_DATA_SIZE }> = heapless::Vec::new();
    let _ = wire_text.extend_from_slice(&device_id);
    let _ = wire_text.extend_from_slice(b": ");
    let body_max = meshcore::MAX_GRP_DATA_SIZE.saturating_sub(device_id.len() + 2);
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
            }
        }
        Err(e) => {
            defmt::warn!("send_grp_txt: packet serialize failed: {:?}", defmt::Debug2Format(&e));
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

    let msg = Message {
        payload_type:   PayloadType::Advert,
        route:          RouteType::Flood,
        version:        0,
        transport_code: 0,
        path:           heapless::Vec::new(),
        payload:        msg_payload,
    };

    let mut frame = [0u8; MAX_TRANS_UNIT];
    match meshcore::packet::serialize(&msg, &mut frame) {
        Ok(len) => {
            if let Err(e) = lora.send_message(&frame[..len]).await {
                defmt::warn!("send_advert: TX failed: {:?}", e);
            } else {
                defmt::info!("MeshCore advert sent ({=usize}B)", len);
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

    let msg = match txt_msg::encrypt(&identity.sec_key, recipient_pk, timestamp, 0, text) {
        Ok(m) => m,
        Err(e) => {
            defmt::warn!("send_pm: encrypt failed: {:?}", defmt::Debug2Format(&e));
            return;
        }
    };

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
