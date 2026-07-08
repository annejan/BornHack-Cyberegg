use defmt::{todo, *};
use defmt_rtt as _;
use embassy_nrf as _;
use embassy_nrf::nfct::{Config as NfcConfig, NfcId, NfcT};
use embassy_nrf::peripherals::NFCT;
use embassy_nrf::{Peri, bind_interrupts, nfct};
use heapless::Vec as HVec;
use panic_probe as _;

use super::iso14443::iso14443_3;
use super::iso14443::iso14443_4::{Card, IsoDep};
#[cfg(feature = "signed-channel")]
use crate::signed_channel::{AUTHORIZED_PUBLIC_KEY, Csprng, Session, SignedError};
use crate::update_health;

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
    #[cfg(feature = "signed-channel")]
    let mut session = match Session::new(AUTHORIZED_PUBLIC_KEY) {
        Ok(s) => s,
        Err(_) => {
            defmt::panic!("AUTHORIZED_PUBLIC_KEY in signed_channel.rs is not a valid Ed25519 point")
        }
    };
    #[cfg(feature = "signed-channel")]
    info!("signed channel: challenge-response mode");

    loop {
        info!("activating");
        nfc.activate().await;
        info!("activated!");

        // Each ISO-DEP session starts with no armed challenge — any
        // leftover from a previous tap is invalidated here so it can't
        // span sessions.
        #[cfg(feature = "signed-channel")]
        session.clear();

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

            let mut resp_vec: HVec<u8, 256> = HVec::new();
            #[cfg(feature = "signed-channel")]
            {
                match (apdu.cla, apdu.ins) {
                    (0x80, 0x01) => handle_signed(&mut session, apdu.data, &mut resp_vec),
                    (0x80, 0x02) => handle_get_challenge(&mut session, &mut resp_vec),
                    _ => dispatch_plain(&apdu, cc, &mut ndef_buf, &mut selected, &mut resp_vec),
                }
            }
            #[cfg(not(feature = "signed-channel"))]
            dispatch_plain(&apdu, cc, &mut ndef_buf, &mut selected, &mut resp_vec);

            let resp: &[u8] = &resp_vec;
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

/// Plaintext APDU dispatcher.  Pulled out of the rx loop so the
/// encrypted path (CLA 0x80 / INS 0x01) can dispatch the inner
/// sub-APDU through the same logic.  Behaviour matches the original
/// inline matcher.
fn dispatch_plain(
    apdu: &Apdu<'_>,
    cc: &[u8],
    ndef_buf: &mut [u8; NDEF_BUF_LEN],
    selected: &mut Selected,
    out: &mut HVec<u8, 256>,
) {
    out.clear();
    let ok: &[u8] = &[0x90, 0x00];
    match (apdu.cla, apdu.ins, apdu.p1, apdu.p2) {
        (0, 0xa4, 4, 0) => {
            info!("select app");
            let _ = out.extend_from_slice(ok);
        }
        (0, 0xa4, 0, 12) => {
            info!("select df");
            match apdu.data {
                [0xe1, 0x03] => {
                    *selected = Selected::Cc;
                    let _ = out.extend_from_slice(ok);
                }
                [0xe1, 0x04] => {
                    *selected = Selected::Ndef;
                    let _ = out.extend_from_slice(ok);
                }
                _ => todo!(),
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
            let file: &[u8] = match *selected {
                Selected::Cc => cc,
                Selected::Ndef => &ndef_buf[..],
            };
            // `offs` is attacker-controlled (up to 0x7FFF) and can exceed the
            // file length; clamp so `file.len() - offs` can't underflow and
            // `file[offs..]` can't slice out of bounds.
            let offs = offs.min(file.len());
            let n = len.min(file.len() - offs).min(out.capacity() - 2);
            let _ = out.extend_from_slice(&file[offs..][..n]);
            let _ = out.extend_from_slice(ok);
        }
        (0, 0xd6, p1, p2) => {
            info!("update binary");
            let offs = u16::from_be_bytes([p1, p2]) as usize;
            if let Selected::Ndef = *selected {
                let end = offs + apdu.data.len();
                if end <= ndef_buf.len() {
                    ndef_buf[offs..end].copy_from_slice(apdu.data);
                    try_apply_station(ndef_buf);
                }
            }
            let _ = out.extend_from_slice(ok);
        }
        _ => {
            info!("Got unknown command!");
            let _ = out.extend_from_slice(&[0xFF, 0xFF]);
        }
    }
}

/// GET CHALLENGE: draw a fresh 16-byte challenge from the CSPRNG, arm
/// the Session with it, and return it to the reader.  Each call
/// supersedes any previous unconsumed challenge.
#[cfg(feature = "signed-channel")]
fn handle_get_challenge(session: &mut Session, out: &mut HVec<u8, 256>) {
    out.clear();
    let challenge = Csprng::next_challenge();
    session.arm(challenge);
    info!("signed: GET CHALLENGE → {:02x}", challenge);
    let _ = out.extend_from_slice(&challenge);
    let _ = out.extend_from_slice(&[0x90, 0x00]);
}

/// Signed APDU path.  Verifies the Ed25519 signature against the
/// currently armed challenge, then dispatches the inner plaintext
/// as a station command.  The Session consumes the challenge on
/// entry, so any subsequent SIGNED CMD without a fresh GET CHALLENGE
/// returns 6A 88 (referenced data not found).
#[cfg(feature = "signed-channel")]
fn handle_signed(session: &mut Session, body: &[u8], out: &mut HVec<u8, 256>) {
    out.clear();

    let plaintext = match session.verify_in_place(body) {
        Ok(p) => p,
        Err(SignedError::MalformedFrame) => {
            error!("signed: malformed frame, body len={=usize}", body.len());
            let _ = out.extend_from_slice(&[0x67, 0x00]);
            return;
        }
        Err(SignedError::BadSignature) => {
            error!("signed: bad signature");
            let _ = out.extend_from_slice(&[0x69, 0x82]);
            return;
        }
        Err(SignedError::NoChallenge) => {
            error!("signed: no challenge armed — issue GET CHALLENGE first");
            let _ = out.extend_from_slice(&[0x6A, 0x88]);
            return;
        }
        Err(SignedError::InvalidPublicKey) => {
            // Cannot happen at runtime — checked at boot.
            let _ = out.extend_from_slice(&[0x69, 0x82]);
            return;
        }
    };

    info!("signed: verify ok, plaintext len={=usize}", plaintext.len());
    // The signed APDU's only payload kind today is a station command
    // dispatched into the game lifecycle.  In an `embassy-mesh`-only
    // build (signed-channel: yes, game: no) there's nothing to apply
    // it to, so we acknowledge the verify and respond 6A 82 (referenced
    // data not found) rather than hard-failing to compile.
    #[cfg(feature = "game")]
    {
        match crate::game::station::apply(plaintext) {
            Some(toast) => {
                // Pull the user back to the game screen so the bonus toast
                // is actually visible — they may have been on Watch /
                // Channels / etc. when they tapped.  `show_toast` itself
                // wakes the display loop via TOAST_SIGNAL.
                crate::DISPLAY_STATE.lock(|cell| {
                    cell.borrow_mut().set_active_screen(crate::SCREEN_GAME);
                });
                crate::game::show_toast(toast);
                let _ = out.extend_from_slice(&[0x90, 0x00]);
            }
            None => {
                let can_use = crate::game::lifecycle::can_use_station();
                info!(
                    "signed: station command rejected (can_use_station={=bool}), plaintext={:02x}",
                    can_use, plaintext
                );
                let _ = out.extend_from_slice(&[0x6A, 0x82]);
            }
        }
    }
    #[cfg(not(feature = "game"))]
    {
        let _ = plaintext; // silence unused-binding warning
        let _ = out.extend_from_slice(&[0x6A, 0x82]);
    }
}

/// After every NDEF write, see whether the buffer now holds a
/// complete NDEF text record.  Two effects can apply on the same
/// payload:
///   * If `text` starts with `"token:"`, forward the value to the token screen
///     (always, regardless of features).
///   * If both the `game` and `nfc-plaintext-station` features are enabled and
///     the text matches a station phrase, apply the effect, show the toast, and
///     re-arm the buffer back to the default URL so the next phone-read shows
///     the `badge.team` URL again.
///
/// Plaintext station dispatch is OFF by default: an UNSIGNED NDEF write is
/// enough to drive it, so it only benefits self-buffs on the tapped badge but
/// bypasses the signed channel. Stations are signed-channel only unless a build
/// opts in via `nfc-plaintext-station` (physical event-station tags).
fn try_apply_station(ndef_buf: &mut [u8; NDEF_BUF_LEN]) {
    let nlen = u16::from_be_bytes([ndef_buf[0], ndef_buf[1]]) as usize;
    if nlen == 0 || 2 + nlen > ndef_buf.len() {
        return;
    }
    let msg = &ndef_buf[2..2 + nlen];
    let Some(text) = ndef::find_text_record(msg) else {
        return;
    };
    try_apply_token(text);
    #[cfg(all(feature = "game", feature = "nfc-plaintext-station"))]
    if let Some(toast) = crate::game::station::apply(text) {
        crate::game::show_toast(toast);
        init_ndef_url(ndef_buf);
    }
}

/// If `text` starts with `"token:"`, forward the suffix to the token
/// screen.  Silently no-ops on non-token text or invalid UTF-8.
fn try_apply_token(text: &[u8]) {
    const PREFIX: &[u8] = b"token:";
    if let Some(value_bytes) = text.strip_prefix(PREFIX)
        && let Ok(value) = core::str::from_utf8(value_bytes)
    {
        crate::token::set_token(value);
    }
}

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
