//! Minimal iCalendar (RFC 5545) parser — extracts `DTSTART`, `DTEND` and
//! `SUMMARY` from `BEGIN:VEVENT` / `END:VEVENT` blocks.
//!
//! What we actually need from an ICS file:
//!   * `DTSTART` — the event start, in any of:
//!     `DTSTART:YYYYMMDDTHHMMSS`             (floating local time),
//!     `DTSTART:YYYYMMDDTHHMMSSZ`            (UTC),
//!     `DTSTART;TZID=Europe/Copenhagen:YYYYMMDDTHHMMSS`.
//!   * `DTEND` — same shapes, optional.  When absent the event has zero
//!     duration (renders as a thin marker on the day-view).
//!   * `SUMMARY` — the event title.  We keep up to the first 31 ASCII
//!     bytes for the on-device label; non-ASCII bytes are dropped.
//!
//! Timezone handling is split across the parser and its caller:
//!   * The parser detects the trailing `Z` and reports `is_utc` per
//!     timestamp.  All time values are returned verbatim (no offset
//!     applied).
//!   * The caller (`watch::import_alarms_from_fat12`) applies
//!     `crate::TIMEZONE_OFFSET` to UTC timestamps before storing them in
//!     alarm slots.  TZID parameters are stripped at parse time and the
//!     accompanying value is treated as floating local — we don't ship
//!     a tzdata table.
//!
//! Out of scope: line folding (continuation lines starting with a space),
//! VALUE= overrides, RRULE recurrence, escape-sequence decoding (`\,`,
//! `\;`, `\n`, `\\`), nested VTIMEZONE blocks, all-day DATE values
//! (we require T-prefixed times).  Bornhack ICS dumps don't fold lines
//! around the SUMMARY/DTSTART/DTEND we care about; if we ever need
//! richer parsing, swap this for a real crate.

/// Maximum bytes kept from an event SUMMARY for the on-device label.
pub const SUMMARY_LEN: usize = 31;

/// One parsed `VEVENT`.  When `DTEND` is missing in the source, the end
/// fields equal the start fields (zero-duration event).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Event {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    /// True when the source `DTSTART` carried a trailing `Z` (UTC).
    pub start_is_utc: bool,
    pub end_year: u16,
    pub end_month: u8,
    pub end_day: u8,
    pub end_hour: u8,
    pub end_minute: u8,
    /// True when the source `DTEND` carried a trailing `Z` (UTC).  Only
    /// meaningful when `DTEND` was present; if it wasn't, this mirrors
    /// `start_is_utc`.
    pub end_is_utc: bool,
    /// First [`SUMMARY_LEN`] ASCII bytes of the SUMMARY, NUL-padded.
    pub summary: [u8; SUMMARY_LEN],
}

impl Event {
    #[allow(dead_code)] // Only used in tests
    pub fn summary_str(&self) -> &str {
        let n = self
            .summary
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(self.summary.len());
        // safe: we only ever push ASCII bytes into the buffer below.
        unsafe { core::str::from_utf8_unchecked(&self.summary[..n]) }
    }
}

/// Iterator over `VEVENT` blocks in an ICS byte slice.  Lines are split on
/// `\n` (a trailing `\r` from CRLF endings is tolerated).  Malformed events
/// (missing/bad DTSTART) are silently skipped.
pub struct Parser<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Parser<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0 }
    }

    fn next_line(&mut self) -> Option<&'a [u8]> {
        if self.pos >= self.bytes.len() {
            return None;
        }
        let start = self.pos;
        let rest = &self.bytes[start..];
        let nl = rest.iter().position(|&b| b == b'\n').unwrap_or(rest.len());
        let mut end = start + nl;
        if end < self.bytes.len() {
            self.pos = end + 1;
        } else {
            self.pos = self.bytes.len();
        }
        // Strip a trailing `\r` from CRLF endings.
        if end > start && self.bytes[end - 1] == b'\r' {
            end -= 1;
        }
        Some(&self.bytes[start..end])
    }
}

/// Internal: a parsed timestamp, including whether it was UTC-flagged.
type ParsedDateTime = (u16, u8, u8, u8, u8, bool);

impl Iterator for Parser<'_> {
    type Item = Event;

    fn next(&mut self) -> Option<Event> {
        loop {
            // Find the next BEGIN:VEVENT.
            loop {
                let line = self.next_line()?;
                if line == b"BEGIN:VEVENT" {
                    break;
                }
            }

            // Collect DTSTART, DTEND and SUMMARY until END:VEVENT.
            let mut dtstart: Option<ParsedDateTime> = None;
            let mut dtend: Option<ParsedDateTime> = None;
            let mut summary = [0u8; SUMMARY_LEN];
            loop {
                let Some(line) = self.next_line() else {
                    return None; // truncated event
                };
                if line == b"END:VEVENT" {
                    break;
                }
                if let Some(value) = match_property(line, b"DTSTART") {
                    dtstart = parse_datetime(value);
                } else if let Some(value) = match_property(line, b"DTEND") {
                    dtend = parse_datetime(value);
                } else if let Some(value) = match_property(line, b"SUMMARY") {
                    copy_ascii(&mut summary, value);
                }
            }

            if let Some((y, mo, d, h, mi, utc)) = dtstart {
                // Default end = start (zero-duration event when DTEND is
                // missing).  Same UTC flag so the caller's timezone
                // conversion is consistent across both timestamps.
                let (ey, emo, ed, eh, emi, eutc) =
                    dtend.unwrap_or((y, mo, d, h, mi, utc));
                return Some(Event {
                    year: y,
                    month: mo,
                    day: d,
                    hour: h,
                    minute: mi,
                    start_is_utc: utc,
                    end_year: ey,
                    end_month: emo,
                    end_day: ed,
                    end_hour: eh,
                    end_minute: emi,
                    end_is_utc: eutc,
                    summary,
                });
            }
            // dtstart-less event — skip to next.
        }
    }
}

/// Returns the property value if `line` matches `<name>` either bare
/// (`NAME:value`) or with parameters (`NAME;TZID=…:value`).  Otherwise
/// returns `None`.
fn match_property<'a>(line: &'a [u8], name: &[u8]) -> Option<&'a [u8]> {
    if !line.starts_with(name) {
        return None;
    }
    let rest = &line[name.len()..];
    match rest.first()? {
        b':' => Some(&rest[1..]),
        b';' => {
            // Skip parameters up to the first ':'.
            let colon = rest.iter().position(|&b| b == b':')?;
            Some(&rest[colon + 1..])
        }
        _ => None,
    }
}

/// Parse `YYYYMMDDTHHMMSS` (with an optional trailing `Z`) into
/// `(year, month, day, hour, minute, is_utc)`.  Seconds are discarded.
fn parse_datetime(value: &[u8]) -> Option<ParsedDateTime> {
    // Need at least YYYYMMDDTHHMM = 13 bytes.
    if value.len() < 13 {
        return None;
    }
    let year = digits(&value[0..4])? as u16;
    let month = digits(&value[4..6])? as u8;
    let day = digits(&value[6..8])? as u8;
    if value[8] != b'T' {
        return None;
    }
    let hour = digits(&value[9..11])? as u8;
    let minute = digits(&value[11..13])? as u8;
    if month == 0 || month > 12 || day == 0 || day > 31 || hour > 23 || minute > 59 {
        return None;
    }
    // `Z` may follow the seconds — accept either trailing position
    // (right after HHMM if seconds were stripped, or right after SS).
    let is_utc = matches!(value.last(), Some(&b'Z'));
    Some((year, month, day, hour, minute, is_utc))
}

fn digits(bytes: &[u8]) -> Option<u32> {
    let mut n = 0u32;
    for &b in bytes {
        if !b.is_ascii_digit() {
            return None;
        }
        n = n * 10 + (b - b'0') as u32;
    }
    Some(n)
}

fn copy_ascii(dst: &mut [u8; SUMMARY_LEN], src: &[u8]) {
    let mut i = 0;
    for &b in src {
        if i >= dst.len() {
            break;
        }
        // Only keep printable ASCII; everything else is dropped (including
        // multi-byte UTF-8 sequences, control chars, escape sequences).
        if (0x20..=0x7e).contains(&b) {
            dst[i] = b;
            i += 1;
        }
    }
    // Zero-pad the rest.
    for slot in &mut dst[i..] {
        *slot = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &[u8] = b"\
BEGIN:VCALENDAR\r
VERSION:2.0\r
BEGIN:VEVENT\r
SUMMARY:Opening Ceremony\r
DTSTART:20250811T140000\r
DTEND:20250811T150000\r
END:VEVENT\r
BEGIN:VEVENT\r
SUMMARY:Talk about Rust\r
DTSTART;TZID=Europe/Copenhagen:20250816T143000\r
END:VEVENT\r
BEGIN:VEVENT\r
SUMMARY:UTC event\r
DTSTART:20250819T120000Z\r
DTEND:20250819T130000Z\r
END:VEVENT\r
END:VCALENDAR\r
";

    #[test]
    fn parses_events() {
        let events: std::vec::Vec<_> = Parser::new(SAMPLE).collect();
        assert_eq!(events.len(), 3);

        // Event 0: floating local time, has DTEND.
        assert_eq!(events[0].year, 2025);
        assert_eq!(events[0].month, 8);
        assert_eq!(events[0].day, 11);
        assert_eq!(events[0].hour, 14);
        assert_eq!(events[0].minute, 0);
        assert!(!events[0].start_is_utc);
        assert_eq!(events[0].end_hour, 15);
        assert_eq!(events[0].end_minute, 0);
        assert!(!events[0].end_is_utc);
        assert_eq!(events[0].summary_str(), "Opening Ceremony");

        // Event 1: TZID=local, no DTEND → end mirrors start.
        assert_eq!(events[1].year, 2025);
        assert_eq!(events[1].month, 8);
        assert_eq!(events[1].day, 16);
        assert_eq!(events[1].hour, 14);
        assert_eq!(events[1].minute, 30);
        assert_eq!(events[1].end_hour, 14);
        assert_eq!(events[1].end_minute, 30);
        assert!(!events[1].start_is_utc);
        assert_eq!(events[1].summary_str(), "Talk about Rust");

        // Event 2: UTC, has DTEND.
        assert!(events[2].start_is_utc);
        assert!(events[2].end_is_utc);
        assert_eq!(events[2].hour, 12);
        assert_eq!(events[2].end_hour, 13);
        assert_eq!(events[2].summary_str(), "UTC event");
    }

    #[test]
    fn skips_event_without_dtstart() {
        let bytes = b"BEGIN:VEVENT\nSUMMARY:no time\nEND:VEVENT\n";
        assert!(Parser::new(bytes).next().is_none());
    }

    #[test]
    fn truncates_long_summary() {
        let bytes = b"BEGIN:VEVENT\n\
SUMMARY:0123456789abcdef0123456789abcdef0123456789\n\
DTSTART:20250101T000000\n\
END:VEVENT\n";
        let ev = Parser::new(bytes).next().unwrap();
        assert_eq!(ev.summary_str().len(), SUMMARY_LEN);
        assert_eq!(ev.summary_str(), "0123456789abcdef0123456789abcde");
    }

    #[test]
    fn drops_non_ascii_bytes() {
        let bytes =
            "BEGIN:VEVENT\nSUMMARY:Café Talk\nDTSTART:20250101T120000\nEND:VEVENT\n".as_bytes();
        let ev = Parser::new(bytes).next().unwrap();
        // The non-ASCII `é` (0xc3 0xa9 in UTF-8) is dropped.
        assert_eq!(ev.summary_str(), "Caf Talk");
    }

    #[test]
    fn missing_dtend_mirrors_start() {
        let bytes: &[u8] = b"BEGIN:VEVENT\nSUMMARY:Point\nDTSTART:20250101T120000\nEND:VEVENT\n";
        let ev = Parser::new(bytes).next().unwrap();
        assert_eq!(ev.hour, 12);
        assert_eq!(ev.end_hour, 12);
        assert_eq!(ev.minute, 0);
        assert_eq!(ev.end_minute, 0);
    }
}
