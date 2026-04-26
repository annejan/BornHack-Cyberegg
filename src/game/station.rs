//! Station NFC text-record dispatcher.
//!
//! Event-side stations (food / medic / inspiration / rest) hand out
//! buffs to a player's badge by NFC-writing a fixed text record onto
//! the tag.  When the firmware receives a write, the NDEF text payload
//! is handed here; if it matches one of the four phrases the
//! corresponding stat is restored to full and a station toast is
//! shown.  Anything else is silently ignored.

use super::Toast;
use super::lifecycle::with_state;

/// Try to interpret `text` as a station command.  Returns the toast to
/// display on success, or `None` if the text is not a recognised
/// station phrase.  Matching is exact after trimming ASCII whitespace
/// and lowercasing; empty / non-ASCII / oversized payloads are
/// rejected silently.
pub fn apply(text: &[u8]) -> Option<Toast> {
    let key = normalize(text)?;

    match key.as_slice() {
        b"more food" => {
            with_state(|s| { s.hunger = 0; true });
            Some(Toast::StationFood)
        }
        b"more drugs" => {
            with_state(|s| { s.sick = 0; true });
            Some(Toast::StationDrugs)
        }
        b"more inspiration" => {
            with_state(|s| { s.drained = 0; true });
            Some(Toast::StationInspire)
        }
        b"sleep like a bear" => {
            with_state(|s| { s.tired = 0; true });
            Some(Toast::StationRest)
        }
        _ => None,
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
