use crate::update_health;

use super::iso14443::iso14443_3;
use super::iso14443::iso14443_4::{Card, IsoDep};
use defmt::{todo, *};
use embassy_nrf::nfct::NfcT;
use embassy_nrf::nfct::{Config as NfcConfig, NfcId};
use embassy_nrf::peripherals::NFCT;
use embassy_nrf::{Peri, bind_interrupts, nfct};
use {defmt_rtt as _, embassy_nrf as _, panic_probe as _};

bind_interrupts!(struct Irqs {
    NFCT => nfct::InterruptHandler;
});

const NDEF_URL: &[u8] = b"badge.team";
const NDEF_URL_PREFIX: u8 = 0x04; // https://
/// Default URL NDEF: NLEN(2) + record header(1) + type len(1) + payload len(1)
/// + type 'U'(1) + URI prefix(1) + URL.
const NDEF_URL_LEN: usize = 7 + NDEF_URL.len();

/// RAM buffer for the NDEF file.  Sized for headroom; the CC TLV
/// advertises a max NDEF size of 127 bytes so readers won't write
/// beyond that, but anything we receive that fits goes here.
const NDEF_BUF_LEN: usize = 256;

/// Initialise (or re-arm) the NDEF buffer to the default `badge.team`
/// URL record.  Returns the number of valid bytes (NLEN + message).
fn init_ndef_url(buf: &mut [u8; NDEF_BUF_LEN]) -> usize {
    // Zero out — anything past the message must read back as 0x00 so
    // a phone re-reading the tag sees a clean record.
    for b in buf.iter_mut() {
        *b = 0;
    }
    let msg_len = NDEF_URL_LEN - 2;
    buf[0] = (msg_len >> 8) as u8;
    buf[1] = msg_len as u8;
    buf[2] = 0xd1; // NDEF record header (MB, ME, SR, TNF=Well-known)
    buf[3] = 0x01; // type length = 1
    buf[4] = (1 + NDEF_URL.len()) as u8; // payload length = prefix + url
    buf[5] = 0x55; // type = 'U' (URI)
    buf[6] = NDEF_URL_PREFIX;
    buf[7..7 + NDEF_URL.len()].copy_from_slice(NDEF_URL);
    NDEF_URL_LEN
}

/// Which file the reader has currently selected.
#[derive(Clone, Copy)]
enum Selected {
    Cc,
    Ndef,
}

pub async fn run_nfct(nfct: Peri<'_, NFCT>) {
    dbg!("Setting up...");
    let config = NfcConfig {
        nfcid1: NfcId::DoubleSize([0x04, 0x68, 0x95, 0x71, 0xFA, 0x5C, 0x64]),
        sdd_pat: nfct::SddPat::SDD00100,
        plat_conf: 0b0000,
        protocol: nfct::SelResProtocol::Type4A,
    };

    let mut nfc = NfcT::new(nfct, Irqs, &config);

    let mut buf = [0u8; 256];

    // Capability Container.  The TLV's last two bytes (0x00, 0x00)
    // declare the NDEF file as both read-free and write-free, so phone
    // NFC writers can issue UPDATE BINARY without further auth.
    let cc = &[
        0x00, 0x0f, /* CCEN_HI, CCEN_LOW */
        0x20, /* VERSION */
        0x00, 0x7f, /* MLe_HI, MLe_LOW */
        0x00, 0x7f, /* MLc_HI, MLc_LOW */
        /* TLV */
        0x04, 0x06, 0xe1, 0x04, 0x00, 0x7f, 0x00, 0x00,
    ];

    let mut ndef_buf = [0u8; NDEF_BUF_LEN];
    init_ndef_url(&mut ndef_buf);

    let mut selected = Selected::Cc;

    loop {
        info!("activating");
        nfc.activate().await;
        info!("activated!");

        let mut nfc = IsoDep::new(iso14443_3::Logger(&mut nfc));

        loop {
            let n = match nfc.receive(&mut buf).await {
                Ok(n) => n,
                Err(e) => {
                    error!("rx error {}", e);
                    break;
                }
            };
            let req = &buf[..n];
            info!("iso-dep rx {:02x}", req);

            let Ok(apdu) = Apdu::parse(req) else {
                error!("apdu parse error");
                break;
            };

            info!("apdu: {:?}", apdu);

            // Compute the response into a small scratch slot so the
            // borrow checker doesn't have to reason about overlapping
            // borrows of `buf` and `apdu.data`.
            let mut ok = [0x90, 0x00];
            let resp: &[u8] = match (apdu.cla, apdu.ins, apdu.p1, apdu.p2) {
                (0, 0xa4, 4, 0) => {
                    info!("select app");
                    &ok
                }
                (0, 0xa4, 0, 12) => {
                    info!("select df");
                    match apdu.data {
                        [0xe1, 0x03] => {
                            selected = Selected::Cc;
                            &ok
                        }
                        [0xe1, 0x04] => {
                            selected = Selected::Ndef;
                            &ok
                        }
                        _ => todo!(), // return NOT FOUND
                    }
                }
                (0, 0xb0, p1, p2) => {
                    info!("read");
                    let offs = u16::from_be_bytes([p1 & 0x7f, p2]) as usize;
                    let len = if apdu.le == 0 {
                        usize::MAX
                    } else {
                        apdu.le as usize
                    };
                    let file: &[u8] = match selected {
                        Selected::Cc => cc,
                        Selected::Ndef => &ndef_buf[..],
                    };
                    let n = len.min(file.len() - offs);
                    buf[..n].copy_from_slice(&file[offs..][..n]);
                    buf[n..][..2].copy_from_slice(&[0x90, 0x00]);
                    &buf[..n + 2]
                }
                (0, 0xd6, p1, p2) => {
                    // UPDATE BINARY — phone is writing into the
                    // currently-selected file.  We only honour writes
                    // to the NDEF file; CC writes are silently
                    // accepted but discarded.
                    info!("update binary");
                    let offs = u16::from_be_bytes([p1, p2]) as usize;
                    if let Selected::Ndef = selected {
                        let end = offs + apdu.data.len();
                        if end <= ndef_buf.len() {
                            ndef_buf[offs..end].copy_from_slice(apdu.data);
                            try_apply_station(&mut ndef_buf);
                        }
                    }
                    &ok
                }
                _ => {
                    info!("Got unknown command!");
                    ok = [0xFF, 0xFF];
                    &ok
                }
            };

            info!("iso-dep tx {:02x}", resp);

            match nfc.transmit(resp).await {
                Ok(()) => {
                    update_health!(|h| h.nfc.set_ok("NFC transmit okay!"));
                }
                Err(e) => {
                    error!("tx error {}", e);
                    update_health!(|h| h.nfc.set_err("NFC transmit failed"));
                    break;
                }
            }
        }
    }
}

/// After every NDEF write, see whether the buffer now holds a
/// complete NDEF text record.  If it does and the text matches a
/// station phrase, apply the effect, show the toast, and re-arm the
/// buffer back to the default URL so the next phone-read shows the
/// `badge.team` URL again.
#[cfg(feature = "game")]
fn try_apply_station(ndef_buf: &mut [u8; NDEF_BUF_LEN]) {
    let nlen = u16::from_be_bytes([ndef_buf[0], ndef_buf[1]]) as usize;
    if nlen == 0 || 2 + nlen > ndef_buf.len() {
        return;
    }
    let msg = &ndef_buf[2..2 + nlen];
    let Some(text) = ndef::find_text_record(msg) else {
        return;
    };
    let Some(toast) = crate::game::station::apply(text) else {
        return;
    };
    crate::game::show_toast(toast);
    init_ndef_url(ndef_buf);
}

#[cfg(not(feature = "game"))]
fn try_apply_station(_ndef_buf: &mut [u8; NDEF_BUF_LEN]) {}

#[cfg(feature = "game")]
mod ndef {
    //! Minimal NDEF reader — just enough to pull a UTF-8 text payload
    //! out of the first text record in a message.

    /// Find the first NDEF Well-Known text record in `msg` and return
    /// its UTF-8 text bytes (with the language code stripped off), or
    /// `None` if no usable text record is present.  Tolerates
    /// well-formed messages with non-text records preceding the text
    /// one; bails on chunked records (CF=1) and anything malformed.
    pub fn find_text_record(msg: &[u8]) -> Option<&[u8]> {
        let mut i = 0;
        loop {
            if i >= msg.len() {
                return None;
            }
            let header = msg[i];
            i += 1;
            let me = header & 0x40 != 0;
            let cf = header & 0x20 != 0;
            let sr = header & 0x10 != 0;
            let il = header & 0x08 != 0;
            let tnf = header & 0x07;
            if cf {
                return None;
            }

            if i >= msg.len() {
                return None;
            }
            let type_len = msg[i] as usize;
            i += 1;

            let payload_len = if sr {
                if i >= msg.len() {
                    return None;
                }
                let pl = msg[i] as usize;
                i += 1;
                pl
            } else {
                if i + 4 > msg.len() {
                    return None;
                }
                let pl = u32::from_be_bytes([msg[i], msg[i + 1], msg[i + 2], msg[i + 3]]) as usize;
                i += 4;
                pl
            };

            let id_len = if il {
                if i >= msg.len() {
                    return None;
                }
                let il_byte = msg[i] as usize;
                i += 1;
                il_byte
            } else {
                0
            };

            if i + type_len > msg.len() {
                return None;
            }
            let type_bytes = &msg[i..i + type_len];
            i += type_len + id_len;

            if i + payload_len > msg.len() {
                return None;
            }
            let payload = &msg[i..i + payload_len];
            i += payload_len;

            if tnf == 0x01 && type_bytes == b"T" {
                // Text payload: [status][lang_code][utf8 text]
                if payload.is_empty() {
                    if me {
                        return None;
                    }
                    continue;
                }
                let status = payload[0];
                if status & 0x80 != 0 {
                    // UTF-16 — we don't decode it; skip.
                    if me {
                        return None;
                    }
                    continue;
                }
                let lang_len = (status & 0x3F) as usize;
                if 1 + lang_len > payload.len() {
                    if me {
                        return None;
                    }
                    continue;
                }
                return Some(&payload[1 + lang_len..]);
            }

            if me {
                return None;
            }
        }
    }
}

#[derive(Debug, Clone, defmt::Format)]
struct Apdu<'a> {
    pub cla: u8,
    pub ins: u8,
    pub p1: u8,
    pub p2: u8,
    pub data: &'a [u8],
    pub le: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, defmt::Format)]
struct ApduParseError;

impl<'a> Apdu<'a> {
    pub fn parse(apdu: &'a [u8]) -> Result<Self, ApduParseError> {
        if apdu.len() < 4 {
            return Err(ApduParseError);
        }

        let (data, le) = match apdu.len() - 4 {
            0 => (&[][..], 0),
            1 => (&[][..], apdu[4]),
            n if n == 1 + apdu[4] as usize && apdu[4] != 0 => (&apdu[5..][..apdu[4] as usize], 0),
            n if n == 2 + apdu[4] as usize && apdu[4] != 0 => {
                (&apdu[5..][..apdu[4] as usize], apdu[apdu.len() - 1])
            }
            _ => return Err(ApduParseError),
        };

        Ok(Apdu {
            cla: apdu[0],
            ins: apdu[1],
            p1: apdu[2],
            p2: apdu[3],
            data,
            le: le as _,
        })
    }
}
