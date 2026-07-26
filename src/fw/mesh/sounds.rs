//! Per-event notification sounds — user-configurable tone choices.
//!
//! Each event (incoming PM, incoming channel message, newly-heard
//! contact) has its own [`AtomicU8`] index.  Index `0` is "Off";
//! indices `1..=watch::TONES.len()` map to a tone in
//! [`crate::watch::TONES`] (shared with the alarm-tone stepper).
//!
//! Defaults: incoming-PM = Nokia (the recognisable jingle), all
//! others off.  Persisted in the `"watch"` kv namespace alongside
//! alarm state and the boot-chime toggle — piggybacking on the
//! existing `watch::settings_persister_task` keeps the persister
//! count down (no extra task future).
//!
//! Suppressing sound when `BLE_CONNECTED` would be reasonable
//! (the companion app already handles notifications), but the user
//! has explicitly opted in via Settings, so we honour the choice.

use core::sync::atomic::{AtomicU8, Ordering};

use crate::watch::TONES;

/// Number of selectable values: "Off" plus every entry in [`TONES`].
const N_CHOICES: usize = TONES.len() + 1;

/// Default index for the PM-received tone — "Nokia".  Computed from
/// `TONES` so reordering the shared table doesn't silently shift the
/// default.  Falls back to "Off" if Nokia is ever removed.
pub const DEFAULT_PM_TONE: u8 = {
    let mut i = 0;
    let mut found = 0u8;
    while i < TONES.len() {
        if TONES[i].1 == crate::SONG_NOKIA_INDEX {
            found = (i + 1) as u8;
            break;
        }
        i += 1;
    }
    found
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SoundEvent {
    /// Plain TxtMsg arrived (decrypted, MAC-verified).
    PmReceived,
    /// New channel message (`PayloadType::GrpTxt`) arrived.
    ChannelMsg,
    /// Brand-new contact heard via advert (not a refresh of an
    /// existing entry).
    ContactDiscovered,
}

pub static PM_TONE: AtomicU8 = AtomicU8::new(DEFAULT_PM_TONE);
pub static CHANNEL_TONE: AtomicU8 = AtomicU8::new(0);
pub static CONTACT_TONE: AtomicU8 = AtomicU8::new(0);

fn atomic_for(event: SoundEvent) -> &'static AtomicU8 {
    match event {
        SoundEvent::PmReceived => &PM_TONE,
        SoundEvent::ChannelMsg => &CHANNEL_TONE,
        SoundEvent::ContactDiscovered => &CONTACT_TONE,
    }
}

/// Play the configured tone for `event`, or do nothing when set to
/// "Off" / out of range.  Cheap to call from radio-RX hot paths —
/// one atomic load plus a buzzer enqueue.
pub fn play(event: SoundEvent) {
    play_idx(atomic_for(event).load(Ordering::Relaxed) as usize);
}

fn play_idx(idx: usize) {
    if idx == 0 || idx > TONES.len() {
        return;
    }
    let song = TONES[idx - 1].1;
    #[cfg(feature = "embassy-core")]
    crate::fw::buzzer::play(song as usize);
    #[cfg(not(feature = "embassy-core"))]
    let _ = song;
}

// ── Settings stepper helpers ───────────────────────────────────────────────
//
// Exposed to the menu so each event has its own row that cycles
// through "Off" → TONES.  The setter previews the chosen tone via
// `fw::buzzer::play` so the user hears the choice immediately
// (skipping "Off"), and signals `watch::SETTINGS_DIRTY_SIGNAL` so the
// existing watch-namespace persister flushes the new index.

/// Returns the human label for the tone currently configured for `event`.
pub fn tone_label(event: SoundEvent) -> &'static str {
    let idx = atomic_for(event).load(Ordering::Relaxed) as usize;
    if idx == 0 || idx > TONES.len() {
        return "Off";
    }
    TONES[idx - 1].0
}

/// Step `event`'s tone selection by `delta` (`+1` / `-1`), wrapping.
pub fn tone_step(event: SoundEvent, delta: i8) {
    let atomic = atomic_for(event);
    let len = N_CHOICES as i32;
    let cur = atomic.load(Ordering::Relaxed) as i32;
    let next = (cur + delta as i32).rem_euclid(len) as usize;
    atomic.store(next as u8, Ordering::Relaxed);
    play_idx(next);
    crate::watch::signal_settings_dirty();
}

// ── KV persistence ─────────────────────────────────────────────────────────
//
// Three 1-byte keys in the `"watch"` namespace, alongside `face`,
// `alarm_*`, and `boot_chime`.  Same load-on-boot, persist-on-signal
// pattern.

#[cfg(feature = "embassy-core")]
const KV_KEYS: [(&str, &AtomicU8); 3] = [
    ("pm_tone", &PM_TONE),
    ("ch_tone", &CHANNEL_TONE),
    ("disc_tone", &CONTACT_TONE),
];

#[cfg(feature = "embassy-core")]
pub async fn load_settings_from_kv(ns: &crate::fw::kv::KvNamespace) {
    for (key, atomic) in KV_KEYS {
        let mut buf = [0u8; 1];
        if let Ok(1) = ns.get(key, &mut buf).await {
            // Sanity-clamp so a corrupted byte can't permanently
            // disable sounds (or point past the table).
            let idx = if (buf[0] as usize) < N_CHOICES {
                buf[0]
            } else {
                0
            };
            atomic.store(idx, Ordering::Relaxed);
        }
    }
}

#[cfg(feature = "embassy-core")]
pub async fn persist(ns: &crate::fw::kv::KvNamespace) {
    for (key, atomic) in KV_KEYS {
        let _ = ns.set(key, &[atomic.load(Ordering::Relaxed)], true).await;
    }
}
