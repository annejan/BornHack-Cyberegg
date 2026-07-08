//! Station NFC text-record dispatcher.
//!
//! Event-side stations (food / medic / inspiration / rest) hand out
//! buffs to a player's badge.  Two callers reach this dispatcher:
//!   * the signed NFC channel (`nfct::handle_signed`) — authenticated,
//!     always on; the phrase is the verified plaintext of a signed APDU.
//!   * an unsigned plaintext NDEF write (`nfct::try_apply_station`) —
//!     off by default, gated behind the `nfc-plaintext-station` feature
//!     for physical event-station tags.
//! Either way, if the phrase matches one of the four below the
//! corresponding stat is restored to full and a station toast is
//! shown.  Anything else is silently ignored.
//!
//! # Gating
//!
//! Station effects are only applied while a pet is present and not
//! gone (see [`lifecycle::can_use_station`]); a fresh badge with no
//! egg yet, or a badge whose pet has already left, ignores all
//! station writes.
//!
//! # Cooldown
//!
//! Each of the four effects has its own 5-minute cooldown so a player
//! can walk between stations without delay but can't farm the same
//! station twice in quick succession.  Cooldowns live in RAM only and
//! reset across reboots, which is acceptable for the event use case.

use core::sync::atomic::{AtomicU32, Ordering};

use super::Toast;
use super::lifecycle::{self, with_state};

/// Cooldown between repeat applications of the *same* station effect.
/// 30 ticks × 10 s/tick = 5 minutes.
const COOLDOWN_TICKS: u32 = 30;

/// Last-applied tick per effect.  Sentinel `u32::MAX` means "never
/// applied this boot" — `try_consume` checks for it explicitly and
/// skips the cooldown gate, so the first tap always succeeds.
static LAST_FOOD: AtomicU32 = AtomicU32::new(u32::MAX);
static LAST_DRUGS: AtomicU32 = AtomicU32::new(u32::MAX);
static LAST_INSPIRE: AtomicU32 = AtomicU32::new(u32::MAX);
static LAST_REST: AtomicU32 = AtomicU32::new(u32::MAX);

/// Try to claim the cooldown slot.  Returns `Ok(())` and stamps the
/// slot if the cooldown has elapsed; otherwise returns the remaining
/// time in *seconds* so the caller can surface it to the user.
fn try_consume(slot: &AtomicU32) -> Result<(), u16> {
    let now = lifecycle::now_tick();
    let last = slot.load(Ordering::Relaxed);
    if last != u32::MAX {
        let elapsed = now.wrapping_sub(last);
        if elapsed < COOLDOWN_TICKS {
            let remaining_ticks = COOLDOWN_TICKS - elapsed;
            // 1 tick = 10 seconds; cap at u16::MAX defensively even
            // though our 5-min cooldown only ever produces ≤ 300.
            return Err((remaining_ticks * 10).min(u16::MAX as u32) as u16);
        }
    }
    slot.store(now, Ordering::Relaxed);
    Ok(())
}

/// Try to interpret `text` as a station command.  Returns the toast to
/// display on success, or `None` if the text is not a recognised
/// station phrase, the game isn't ready to receive a buff, or the
/// matched effect is on cooldown.  Matching is exact after trimming
/// ASCII whitespace and lowercasing; empty / non-ASCII / oversized
/// payloads are rejected silently.
pub fn apply(text: &[u8]) -> Option<Toast> {
    if !lifecycle::can_use_station() {
        return None;
    }

    let key = normalize(text)?;

    match key.as_slice() {
        b"more food" => station_step(&LAST_FOOD, Toast::StationFood, |s| s.hunger = 0),
        b"more drugs" => station_step(&LAST_DRUGS, Toast::StationDrugs, |s| s.sick = 0),
        b"more inspiration" => {
            station_step(&LAST_INSPIRE, Toast::StationInspire, |s| s.drained = 0)
        }
        b"sleep like a bear" => station_step(&LAST_REST, Toast::StationRest, |s| s.tired = 0),
        _ => None,
    }
}

/// Apply a station effect, or surface the cooldown via a
/// [`Toast::StationCooldown`] with remaining seconds.
fn station_step(
    slot: &AtomicU32,
    success_toast: Toast,
    mutate: impl FnOnce(&mut super::engine::GameState),
) -> Option<Toast> {
    match try_consume(slot) {
        Ok(()) => {
            with_state(|s| {
                mutate(s);
                true
            });
            Some(success_toast)
        }
        Err(secs) => {
            super::show_station_cooldown(secs);
            // Returning `None` so the caller (`nfct.rs`) does not also
            // call `show_toast` — the cooldown toast was already
            // pushed by `show_station_cooldown` above.
            None
        }
    }
}

/// Trim ASCII whitespace and lowercase into a fixed-size buffer.
/// Returns `None` if the trimmed text is empty, non-ASCII, or longer
/// than 32 bytes (the longest station phrase is 17 bytes).
fn normalize(text: &[u8]) -> Option<heapless::Vec<u8, 32>> {
    let trimmed = trim_ascii(text);
    if trimmed.is_empty() || trimmed.len() > 32 {
        return None;
    }
    let mut out: heapless::Vec<u8, 32> = heapless::Vec::new();
    for &b in trimmed {
        if !b.is_ascii() {
            return None;
        }
        let lower = if b.is_ascii_uppercase() { b + 32 } else { b };
        out.push(lower).ok()?;
    }
    Some(out)
}

fn trim_ascii(text: &[u8]) -> &[u8] {
    let mut start = 0;
    let mut end = text.len();
    while start < end && text[start].is_ascii_whitespace() {
        start += 1;
    }
    while end > start && text[end - 1].is_ascii_whitespace() {
        end -= 1;
    }
    &text[start..end]
}
