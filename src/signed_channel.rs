//! Ed25519-signed APDU verifier with challenge-response replay
//! protection.
//!
//! # Protocol
//!
//! Two APDUs per command:
//!
//! 1. **GET CHALLENGE** (`CLA=0x80 INS=0x02`) — PCB returns 16 fresh
//!    random bytes.  The PCB stores the challenge in RAM, scoped to
//!    the current ISO-DEP session.
//! 2. **SIGNED CMD** (`CLA=0x80 INS=0x01`) — data field:
//!
//!        [plaintext command, 1+ bytes] || [64 B Ed25519 signature]
//!
//!    Signature covers `challenge || plaintext` as one message.  The
//!    PCB consumes the challenge on the first verify attempt (success
//!    or failure), so each challenge buys exactly one chance to sign.
//!
//! # Why no counter
//!
//! The previous wire format used a monotonically increasing counter,
//! persisted on both sides.  That breaks down with multiple readers
//! sharing the same signing key: any reader running ahead of the
//! PCB's stored counter is fine; any running behind is rejected as
//! "replay" until manual resync.  The challenge-response form has
//! no per-reader state on the PCB, so any number of readers can
//! share a key without coordination.
//!
//! # Why challenge consumption is a `take`
//!
//! `verify_in_place` calls `Option::take` on the challenge before any
//! parse / signature work.  This guarantees that any single SIGNED
//! CMD APDU — valid or otherwise — burns the challenge, so an
//! attacker who learns the challenge cannot brute-force a signature
//! against it.  The only way to recover is to issue another GET
//! CHALLENGE, which yields a fresh random value.

use ed25519_dalek::{SIGNATURE_LENGTH, Signature, SignatureError, VerifyingKey};
use heapless::Vec as HVec;

/// Ed25519 public key the firmware accepts signatures from.  The
/// matching private key lives in the Android app and never touches
/// the PCB — the badge is open-source and holds no secret material.
pub const AUTHORIZED_PUBLIC_KEY: [u8; 32] = [
    0x18, 0xde, 0x9d, 0xb0, 0x65, 0xd8, 0xef, 0xed, 0xb6, 0x36, 0xfb, 0x88, 0xa2, 0x3f, 0x77, 0x9a,
    0xc9, 0x56, 0x0d, 0x31, 0x76, 0x2c, 0xa5, 0x1c, 0x2a, 0xc9, 0x94, 0x49, 0x8f, 0x8d, 0xb6, 0xef,
];

pub const CHALLENGE_LEN: usize = 16;
const SIG_LEN: usize = SIGNATURE_LENGTH; // 64
/// Smallest valid frame: 1B plaintext + 64B signature.
const MIN_FRAME_LEN: usize = 1 + SIG_LEN;
/// Upper bound on plaintext we'll hash for verification.  Matches the
/// 256-byte APDU buffer in `nfct.rs`; sized to comfortably hold any
/// station-command phrase.
const SIGNED_MSG_CAP: usize = CHALLENGE_LEN + 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignedError {
    /// Public key bytes do not form a valid Ed25519 point.
    InvalidPublicKey,
    /// Frame shorter than `MIN_FRAME_LEN`, or otherwise unparseable.
    MalformedFrame,
    /// Signature did not verify against the public key.
    BadSignature,
    /// No active challenge — caller must issue GET CHALLENGE first.
    NoChallenge,
}

pub struct Session {
    verifying_key: VerifyingKey,
    pending_challenge: Option<[u8; CHALLENGE_LEN]>,
}

impl Session {
    pub fn new(public_key: [u8; 32]) -> Result<Self, SignedError> {
        let verifying_key =
            VerifyingKey::from_bytes(&public_key).map_err(|_| SignedError::InvalidPublicKey)?;
        Ok(Self {
            verifying_key,
            pending_challenge: None,
        })
    }

    /// Stash a freshly generated challenge.  Overwrites any previous
    /// unconsumed challenge — only the most recent one is valid.
    pub fn arm(&mut self, challenge: [u8; CHALLENGE_LEN]) {
        self.pending_challenge = Some(challenge);
    }

    /// Drop any pending challenge.  Called on session deselect / reset
    /// so a stale challenge from a previous tap can't be used.
    pub fn clear(&mut self) {
        self.pending_challenge = None;
    }

    /// Verify a SIGNED CMD frame against the currently armed challenge.
    /// Consumes the challenge on entry — any subsequent verify call
    /// will return `NoChallenge` until `arm` is called again.
    pub fn verify_in_place<'a>(&mut self, frame: &'a [u8]) -> Result<&'a [u8], SignedError> {
        let challenge = self
            .pending_challenge
            .take()
            .ok_or(SignedError::NoChallenge)?;

        if frame.len() < MIN_FRAME_LEN {
            return Err(SignedError::MalformedFrame);
        }

        let plaintext_end = frame.len() - SIG_LEN;
        let plaintext = &frame[..plaintext_end]; // ≥ 1 B
        let sig_bytes = &frame[plaintext_end..]; // 64 B

        let signature = Signature::from_slice(sig_bytes)
            .map_err(|_: SignatureError| SignedError::BadSignature)?;

        let mut signed_msg: HVec<u8, SIGNED_MSG_CAP> = HVec::new();
        signed_msg
            .extend_from_slice(&challenge)
            .map_err(|_| SignedError::MalformedFrame)?;
        signed_msg
            .extend_from_slice(plaintext)
            .map_err(|_| SignedError::MalformedFrame)?;

        self.verifying_key
            .verify_strict(&signed_msg, &signature)
            .map_err(|_| SignedError::BadSignature)?;

        Ok(plaintext)
    }
}

// ---------------------------------------------------------------------------
// Hash-DRBG — generates 16-byte challenges from a TRNG seed.
// ---------------------------------------------------------------------------
//
// The nRF52840 hardware RNG peripheral is owned by the BLE softdevice
// for the lifetime of the program, so the NFC path can't draw bytes
// from it directly.  Instead `embassy.rs` calls `Csprng::seed` with a
// fresh 32-byte TRNG sample at boot (before BLE init takes the
// peripheral).  After that, `Csprng::next_challenge` produces 16-byte
// outputs by hashing the current state with SHA-256: the new state
// is the full 32-byte digest, the challenge is its first 16 bytes.
//
// This is a forward-secure deterministic CSPRNG — once seeded, an
// attacker observing all challenges still can't recover the state or
// predict future outputs without breaking SHA-256.
//
// Csprng is target-only because it relies on `embassy_sync` for
// interrupt-safe state and on the nRF52840 RNG registers for seeding.
// Host-side unit tests cover `Session` directly without touching it.

#[cfg(target_arch = "arm")]
mod csprng {
    use super::CHALLENGE_LEN;
    use core::sync::atomic::{AtomicBool, Ordering};
    use embassy_sync::blocking_mutex::{Mutex, raw::CriticalSectionRawMutex};
    use sha2::{Digest, Sha256};

    static SEEDED: AtomicBool = AtomicBool::new(false);
    static STATE: Mutex<CriticalSectionRawMutex, core::cell::RefCell<[u8; 32]>> =
        Mutex::new(core::cell::RefCell::new([0u8; 32]));

    pub struct Csprng;

    impl Csprng {
        /// Install initial entropy.  Must be called exactly once at boot,
        /// before any call to `next_challenge`, with bytes from the
        /// hardware TRNG.
        pub fn seed(seed: [u8; 32]) {
            STATE.lock(|s: &core::cell::RefCell<[u8; 32]>| *s.borrow_mut() = seed);
            SEEDED.store(true, Ordering::Release);
        }

        /// Convenience: seed directly from the nRF52840 on-chip hardware
        /// TRNG by direct register access.  MUST be called before
        /// `embassy_nrf` hands the RNG peripheral to anything else (e.g.
        /// the BLE softdevice), and exactly once at boot.
        ///
        /// Safety: register addresses are valid for the nRF52840, and
        /// startup is single-threaded so concurrent access is impossible.
        pub fn seed_from_hardware() {
            const RNG_BASE: u32 = 0x4000_D000;
            const TASKS_START: u32 = RNG_BASE;
            const TASKS_STOP: u32 = RNG_BASE + 0x004;
            const EVENTS_VALRDY: u32 = RNG_BASE + 0x100;
            const CONFIG: u32 = RNG_BASE + 0x504; // bit 0: DERCEN
            const VALUE: u32 = RNG_BASE + 0x508;

            let mut seed = [0u8; 32];
            unsafe {
                (CONFIG as *mut u32).write_volatile(1); // DERCEN = 1
                (TASKS_START as *mut u32).write_volatile(1);

                for byte in &mut seed {
                    while (EVENTS_VALRDY as *const u32).read_volatile() == 0 {}
                    *byte = (VALUE as *const u32).read_volatile() as u8;
                    (EVENTS_VALRDY as *mut u32).write_volatile(0);
                }

                (TASKS_STOP as *mut u32).write_volatile(1);
            }
            Self::seed(seed);
        }

        /// Draw a 16-byte challenge.  Panics if `seed` was never called —
        /// generating a zero-entropy challenge would silently strip all
        /// replay protection.
        pub fn next_challenge() -> [u8; CHALLENGE_LEN] {
            if !SEEDED.load(Ordering::Acquire) {
                defmt::panic!("Csprng::next_challenge called before seed");
            }
            STATE.lock(|s: &core::cell::RefCell<[u8; 32]>| {
                let mut state = s.borrow_mut();
                let digest = Sha256::new().chain_update(*state).finalize();
                *state = digest.into();
                let mut out = [0u8; CHALLENGE_LEN];
                out.copy_from_slice(&state[..CHALLENGE_LEN]);
                out
            })
        }
    }
}

#[cfg(target_arch = "arm")]
pub use csprng::Csprng;

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    use heapless::Vec;

    fn test_keypair() -> (SigningKey, [u8; 32]) {
        let seed: [u8; 32] = [
            0xde, 0xad, 0xbe, 0xef, 0xfe, 0xed, 0xfa, 0xce, 0x00, 0x11, 0x22, 0x33, 0x44, 0x55,
            0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff, 0x12, 0x34, 0x56, 0x78,
            0x9a, 0xbc, 0xde, 0xf0,
        ];
        let signing_key = SigningKey::from_bytes(&seed);
        let pk = signing_key.verifying_key().to_bytes();
        (signing_key, pk)
    }

    fn build_frame(
        signing_key: &SigningKey,
        challenge: &[u8; CHALLENGE_LEN],
        plaintext: &[u8],
    ) -> Vec<u8, 256> {
        let mut signed_msg: Vec<u8, 256> = Vec::new();
        signed_msg.extend_from_slice(challenge).unwrap();
        signed_msg.extend_from_slice(plaintext).unwrap();
        let sig = signing_key.sign(&signed_msg);

        let mut frame: Vec<u8, 256> = Vec::new();
        frame.extend_from_slice(plaintext).unwrap();
        frame.extend_from_slice(&sig.to_bytes()).unwrap();
        frame
    }

    #[test]
    fn round_trip_consumes_challenge() {
        let (sk, pk) = test_keypair();
        let mut session = Session::new(pk).unwrap();

        let challenge: [u8; CHALLENGE_LEN] = [0xab; CHALLENGE_LEN];
        session.arm(challenge);

        let pt: &[u8] = b"more food";
        let frame = build_frame(&sk, &challenge, pt);

        let plaintext = session.verify_in_place(&frame).unwrap();
        assert_eq!(plaintext, pt);

        // Replaying the same frame must fail — challenge already taken.
        assert_eq!(
            session.verify_in_place(&frame),
            Err(SignedError::NoChallenge)
        );
    }

    #[test]
    fn rejects_signature_under_wrong_challenge() {
        let (sk, pk) = test_keypair();
        let mut session = Session::new(pk).unwrap();

        let signed_under: [u8; CHALLENGE_LEN] = [0xab; CHALLENGE_LEN];
        let armed: [u8; CHALLENGE_LEN] = [0xcd; CHALLENGE_LEN];
        session.arm(armed);

        let frame = build_frame(&sk, &signed_under, b"more food");
        assert_eq!(
            session.verify_in_place(&frame),
            Err(SignedError::BadSignature)
        );

        // Failed verify still consumed the challenge.
        session.arm(armed);
        let good = build_frame(&sk, &armed, b"more food");
        assert!(session.verify_in_place(&good).is_ok());
    }

    #[test]
    fn rejects_tampered_signature() {
        let (sk, pk) = test_keypair();
        let mut session = Session::new(pk).unwrap();
        let challenge = [0x11; CHALLENGE_LEN];
        session.arm(challenge);

        let mut frame = build_frame(&sk, &challenge, b"more food");
        let last = frame.len() - 1;
        frame[last] ^= 0x01;
        assert_eq!(
            session.verify_in_place(&frame),
            Err(SignedError::BadSignature)
        );
    }

    #[test]
    fn rejects_too_short_frame() {
        let (_, pk) = test_keypair();
        let mut session = Session::new(pk).unwrap();
        session.arm([0; CHALLENGE_LEN]);

        // 64 bytes = signature only, no plaintext byte.
        let buf = [0u8; 64];
        assert_eq!(
            session.verify_in_place(&buf),
            Err(SignedError::MalformedFrame)
        );
    }

    #[test]
    fn no_challenge_means_no_verify() {
        let (sk, pk) = test_keypair();
        let mut session = Session::new(pk).unwrap();
        let frame = build_frame(&sk, &[0; CHALLENGE_LEN], b"more food");
        assert_eq!(
            session.verify_in_place(&frame),
            Err(SignedError::NoChallenge)
        );
    }
}
