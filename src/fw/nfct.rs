use defmt::*;
use defmt_rtt as _;
use embassy_nrf as _;
use embassy_nrf::nfct::{Config as NfcConfig, NfcId, NfcT};
use embassy_nrf::peripherals::NFCT;
use embassy_nrf::{Peri, bind_interrupts, nfct};
use heapless::Vec as HVec;
use panic_probe as _;

use embassy_time::{Duration, Instant};

use super::iso14443::iso14443_3;
use super::iso14443::iso14443_4::{Card, IsoDep};
#[cfg(feature = "signed-channel")]
use crate::signed_channel::{AUTHORIZED_PUBLIC_KEY, Csprng, Session, SignedError};
use crate::update_health;

bind_interrupts!(struct Irqs {
    NFCT => nfct::InterruptHandler;
});

const NDEF_URL: &[u8] = b"badge.team/docs/badges/bornhack-2026/";
const NDEF_URL_PREFIX: u8 = 0x04; // https://
/// Default URL NDEF: NLEN(2) + record header(1) + type len(1) + payload len(1)
/// + type 'U'(1) + URI prefix(1) + URL.
const NDEF_URL_LEN: usize = 7 + NDEF_URL.len();

/// KV namespace + key for the user's persisted broadcast NDEF (the
/// vCard / vanity URL served by default). Stored as the full
/// `[NLEN(2) || message]` region, ready to copy straight into the file
/// buffer. Absent ⇒ fall back to the built-in Bornhack 2026 docs URL.
const KV_NS: &str = "nfc";
const KV_PROFILE_KEY: &str = "profile";

/// After a transient NFC write (a pushed `token:`, a station command, or
/// junk) the badge keeps broadcasting the written bytes for this long,
/// then reverts to the user's persisted profile.
const REVERT_SECS: u64 = 10;

/// What a completed UPDATE BINARY write turned out to be. Drives whether
/// the broadcast reverts (transient) or sticks and is persisted (profile).
#[derive(Clone, Copy, PartialEq, Eq)]
enum WriteOutcome {
    /// Not a completed NDEF write (SELECT/READ, or an incomplete message).
    None,
    /// A station command — reverts to the persisted profile after
    /// [`REVERT_SECS`].  Only constructed with the `nfc-plaintext-station`
    /// feature; the match arm stays so the revert path compiles either way.
    #[cfg_attr(
        not(all(feature = "game", feature = "nfc-plaintext-station")),
        allow(dead_code)
    )]
    Transient,
    /// A `set:<url>` text record or a vCard record — becomes the new
    /// persisted default broadcast.
    SetProfile,
}

/// RAM buffer for the NDEF file.  Sized for headroom; the CC TLV
/// advertises a max NDEF size of 127 bytes so readers won't write
/// beyond that, but anything we receive that fits goes here.
const NDEF_BUF_LEN: usize = 256;

/// Initialise (or re-arm) the NDEF buffer to the default Bornhack 2026
/// docs URL record.  Returns the number of valid bytes (NLEN + message).
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

/// Re-arm `buf` to the broadcast NDEF: the user's persisted `profile`
/// (stored as `[NLEN || message]`) if present, else the built-in URL.
fn arm_broadcast(buf: &mut [u8; NDEF_BUF_LEN], profile: &[u8]) -> usize {
    if profile.len() >= 2 && profile.len() <= NDEF_BUF_LEN {
        for b in buf.iter_mut() {
            *b = 0;
        }
        buf[..profile.len()].copy_from_slice(profile);
        profile.len()
    } else {
        init_ndef_url(buf)
    }
}

/// Load the persisted broadcast profile from KV. Returns an empty vec
/// when unset or unreadable (caller falls back to the built-in URL).
async fn load_profile() -> HVec<u8, NDEF_BUF_LEN> {
    let mut out: HVec<u8, NDEF_BUF_LEN> = HVec::new();
    let mut buf = [0u8; NDEF_BUF_LEN];
    if let Ok(n) = crate::fw::kv::namespace(KV_NS).get(KV_PROFILE_KEY, &mut buf).await
        && (2..=NDEF_BUF_LEN).contains(&n)
    {
        let _ = out.extend_from_slice(&buf[..n]);
    }
    out
}

/// Persist the broadcast profile to KV. Best-effort; logs on failure.
async fn save_profile(profile: &[u8]) {
    if let Err(e) = crate::fw::kv::namespace(KV_NS)
        .set(KV_PROFILE_KEY, profile, true)
        .await
    {
        error!("nfc: failed to persist profile: {}", e);
    }
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

    // User's persisted broadcast NDEF (vCard / vanity URL), stored as
    // `[NLEN || message]`. Empty ⇒ fall back to the built-in URL.
    let mut profile = load_profile().await;
    let mut ndef_buf = [0u8; NDEF_BUF_LEN];
    arm_broadcast(&mut ndef_buf, &profile);

    // When a transient write (token/station/junk) has dirtied the
    // broadcast buffer, this holds the instant at which it should revert
    // to `profile`. The revert is applied lazily on the next APDU.
    let mut revert_at: Option<Instant> = None;

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

            // Revert the broadcast to the persisted profile once the
            // post-write grace window has elapsed, so this APDU (e.g. a
            // READ BINARY) already sees the reverted content.
            if let Some(t) = revert_at
                && Instant::now() >= t
            {
                arm_broadcast(&mut ndef_buf, &profile);
                revert_at = None;
            }

            let mut resp_vec: HVec<u8, 256> = HVec::new();
            let outcome;
            #[cfg(feature = "signed-channel")]
            {
                outcome = match (apdu.cla, apdu.ins) {
                    (0x80, 0x01) => {
                        handle_signed(&mut session, apdu.data, &mut resp_vec);
                        WriteOutcome::None
                    }
                    (0x80, 0x02) => {
                        handle_get_challenge(&mut session, &mut resp_vec);
                        WriteOutcome::None
                    }
                    _ => dispatch_plain(&apdu, cc, &mut ndef_buf, &mut selected, &mut resp_vec),
                };
            }
            #[cfg(not(feature = "signed-channel"))]
            {
                outcome = dispatch_plain(&apdu, cc, &mut ndef_buf, &mut selected, &mut resp_vec);
            }

            // A profile-set persists and stays; a transient write arms the
            // revert timer. Persisting is deferred until after we respond
            // so the phone isn't kept waiting on a flash write.
            let mut persist_pending = false;
            match outcome {
                WriteOutcome::SetProfile => {
                    let nlen = u16::from_be_bytes([ndef_buf[0], ndef_buf[1]]) as usize;
                    let total = (2 + nlen).min(NDEF_BUF_LEN);
                    profile.clear();
                    let _ = profile.extend_from_slice(&ndef_buf[..total]);
                    revert_at = None;
                    persist_pending = true;
                }
                WriteOutcome::Transient => {
                    revert_at = Some(Instant::now() + Duration::from_secs(REVERT_SECS));
                }
                WriteOutcome::None => {}
            }

            let resp: &[u8] = &resp_vec;
            info!("iso-dep tx {:02x}", resp);

            let tx_result = nfc.transmit(resp).await;

            if persist_pending {
                save_profile(&profile).await;
            }

            match tx_result {
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
) -> WriteOutcome {
    out.clear();
    let ok: &[u8] = &[0x90, 0x00];
    let mut outcome = WriteOutcome::None;
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
                // Any other (or malformed-length) file ID: answer "file not
                // found" instead of panicking. A non-standard reader or NFC
                // fuzzer within range must not be able to reset the badge with
                // a stray SELECT.
                _ => {
                    let _ = out.extend_from_slice(&[0x6a, 0x82]);
                }
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
                    outcome = handle_ndef_write(ndef_buf);
                }
            }
            let _ = out.extend_from_slice(ok);
        }
        _ => {
            info!("Got unknown command!");
            let _ = out.extend_from_slice(&[0xFF, 0xFF]);
        }
    }
    outcome
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

/// Classify a completed NDEF write and apply its side effects.
///
/// The rule is: **only `token:` (and, when opted in, station phrases) are
/// transient — everything else you write becomes your persisted broadcast
/// profile.** So a URL tag, a vCard business card, a Wi-Fi record, or any
/// other NDEF message sticks (stored verbatim and re-broadcast until
/// changed), while a pushed token just lands on the token screen and the
/// broadcast reverts after [`REVERT_SECS`].
///
/// Special case: a Well-Known text record `set:<url>` is rewritten into a
/// clean URI record before persisting, so you can set a vanity URL from a
/// plain text writer too.
///
/// Returns [`WriteOutcome::None`] for an incomplete / cleared buffer.
fn handle_ndef_write(ndef_buf: &mut [u8; NDEF_BUF_LEN]) -> WriteOutcome {
    let nlen = u16::from_be_bytes([ndef_buf[0], ndef_buf[1]]) as usize;
    if nlen == 0 || 2 + nlen > ndef_buf.len() {
        return WriteOutcome::None;
    }

    // `set:<url>` text record — copy the URL out (ending the borrow on
    // ndef_buf) before rewriting the buffer as a clean generated URI
    // record, then persist that.
    let set_url: Option<HVec<u8, NDEF_BUF_LEN>> =
        crate::nfc_ndef::find_text_record(&ndef_buf[2..2 + nlen])
            .and_then(|t| t.strip_prefix(b"set:".as_slice()))
            .map(|url| {
                let mut v: HVec<u8, NDEF_BUF_LEN> = HVec::new();
                let _ = v.extend_from_slice(url);
                v
            });
    if let Some(url) = set_url {
        crate::nfc_ndef::build_uri_record(ndef_buf, &url);
        return WriteOutcome::SetProfile;
    }

    // Transient writes: an (opt-in, unsigned) station phrase. These do NOT
    // persist — the broadcast reverts to the profile.
    #[cfg(all(feature = "game", feature = "nfc-plaintext-station"))]
    if let Some(text) = crate::nfc_ndef::find_text_record(&ndef_buf[2..2 + nlen]) {
        if let Some(toast) = crate::game::station::apply(text) {
            crate::game::show_toast(toast);
            return WriteOutcome::Transient;
        }
    }

    // Everything else — a URL tag, vCard, Wi-Fi config, any other record —
    // becomes the persisted broadcast profile, stored verbatim.
    WriteOutcome::SetProfile
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
