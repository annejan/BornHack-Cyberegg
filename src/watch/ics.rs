//! Minimal iCalendar (RFC 5545) parser — extracts `DTSTART`, `DTEND`,
//! `RRULE` and `SUMMARY` from `BEGIN:VEVENT` / `END:VEVENT` blocks.
//!
//! What we actually need from an ICS file:
//!   * `DTSTART` — the event start, in any of: `DTSTART:YYYYMMDDTHHMMSS`
//!     (floating local time), `DTSTART:YYYYMMDDTHHMMSSZ`            (UTC),
//!     `DTSTART;TZID=Europe/Copenhagen:YYYYMMDDTHHMMSS`, or a date-only
//!     `DTSTART;VALUE=DATE:YYYYMMDD` (all-day).
//!   * `DTEND` — same shapes, optional.  When absent the event has zero
//!     duration (renders as a thin marker on the day-view).  For all-day
//!     events the ICS `DTEND` is *exclusive*, so it is pulled back to
//!     23:59 on the last day the event actually covers.
//!   * `RRULE` — `FREQ=DAILY|WEEKLY|MONTHLY|YEARLY` with `INTERVAL`,
//!     `COUNT`, `UNTIL` and `BYDAY`.  Expanded in-parser: the iterator
//!     yields one [`Event`] per occurrence, up to [`MAX_OCCURRENCES`].
//!   * `SUMMARY` — the event title, kept as the first 31 Latin-1
//!     characters.  The label is drawn with an ISO 8859-1 font, so `Æ`,
//!     `é` and `ø` render as themselves; only code points past Latin-1
//!     are transliterated (`Š` → `S`, `…` → `...`), and anything with no
//!     sensible spelling (emoji, CJK) becomes `?`.  RFC 5545 escapes
//!     (`\,` `\;` `\n` `\\`) are decoded.
//!
//! Timezone handling is split across the parser and its caller:
//!   * The parser detects the trailing `Z` and reports `is_utc` per timestamp.
//!     All time values are returned verbatim (no offset applied).
//!   * The caller (`watch::import_alarms_from_fat12`) applies
//!     `crate::TIMEZONE_OFFSET` to UTC timestamps before storing them in alarm
//!     slots.  TZID parameters are stripped at parse time and the accompanying
//!     value is treated as floating local — we don't ship a tzdata table.
//!
//! Out of scope: line folding (continuation lines starting with a space),
//! `EXDATE` / `RDATE`, `BYMONTHDAY` / `BYSETPOS` and friends, nested
//! VTIMEZONE blocks.  Bornhack ICS dumps don't fold lines around the
//! properties we care about; if we ever need richer parsing, swap this
//! for a real crate.

/// Maximum bytes kept from an event SUMMARY for the on-device label.
pub const SUMMARY_LEN: usize = 31;

/// Hard cap on how many occurrences a single `RRULE` may expand to.  A
/// rule with neither `COUNT` nor `UNTIL` repeats forever; the badge has a
/// fixed number of event slots, so runaway rules are cut off here rather
/// than being allowed to fill the calendar on their own.
pub const MAX_OCCURRENCES: u16 = 64;

/// Upper bound on how far a rule may search for its next occurrence
/// before giving up, in steps.  Guards against rules that can never match
/// again (`BYDAY` with no matching weekday, 29 February yearly, …).
const MAX_SEARCH_STEPS: u16 = 400;

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
    /// True when `DTSTART` was a date with no time (`VALUE=DATE`).  The
    /// event covers 00:00–23:59 of each day it spans; the importer marks
    /// these silent so an all-day entry doesn't ring at midnight.
    pub all_day: bool,
    /// First [`SUMMARY_LEN`] characters of the SUMMARY as Latin-1,
    /// NUL-padded — one byte per glyph, matching the ISO 8859-1 font.
    pub summary: [u8; SUMMARY_LEN],
}

impl Event {
    /// The label decoded from Latin-1 into a UTF-8 string.  Each stored
    /// byte is one character, so this never truncates mid-glyph.
    #[allow(dead_code)] // Only used in tests
    pub fn summary_str(&self) -> heapless::String<{ SUMMARY_LEN * 2 }> {
        let mut out = heapless::String::new();
        for &b in self.summary.iter() {
            if b == 0 {
                break;
            }
            let _ = out.push(b as char);
        }
        out
    }
}

/// Iterator over `VEVENT` blocks in an ICS byte slice.  Lines are split on
/// `\n` (a trailing `\r` from CRLF endings is tolerated).  Malformed events
/// (missing/bad DTSTART) are silently skipped.  An event carrying an
/// `RRULE` yields one item per occurrence.
pub struct Parser<'a> {
    bytes: &'a [u8],
    pos: usize,
    /// Byte offset just past the last `END:VEVENT` that produced an
    /// event.  Lets a caller feeding the parser in chunks know how much
    /// of its buffer is safe to discard — see [`Parser::consumed`].
    consumed: usize,
    /// In-flight recurrence expansion, if the current event has an RRULE.
    pending: Option<Pending>,
}

impl<'a> Parser<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            pos: 0,
            consumed: 0,
            pending: None,
        }
    }

    /// Bytes fully consumed, i.e. the offset just past the last complete
    /// `VEVENT` this parser has yielded from.  Everything from here on is
    /// either an unfinished event or trailing junk, so a chunked reader
    /// can drop the prefix and re-parse the remainder against more data.
    pub fn consumed(&self) -> usize {
        self.consumed
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

/// Internal: a parsed timestamp — `(year, month, day, hour, minute,
/// is_utc, is_date_only)`.
type ParsedDateTime = (u16, u8, u8, u8, u8, bool, bool);

// ── Recurrence ──────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Freq {
    Daily,
    Weekly,
    Monthly,
    Yearly,
}

/// The subset of `RRULE` we honour.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Recur {
    freq: Freq,
    /// `INTERVAL`, at least 1.
    interval: u16,
    /// `COUNT`, counting `DTSTART` itself as occurrence 1.
    count: Option<u16>,
    /// `UNTIL`, as a date — the time part is ignored, so an occurrence on
    /// the UNTIL day is kept.
    until: Option<(u16, u8, u8)>,
    /// `BYDAY` weekday mask, bit 0 = Monday .. bit 6 = Sunday.  Zero
    /// means unset (the rule keeps DTSTART's weekday).
    byday: u8,
}

/// In-flight expansion of one `RRULE`.
struct Pending {
    base: Event,
    rule: Recur,
    /// Occurrences yielded so far, including `DTSTART`.
    emitted: u16,
    /// Start date of the occurrence yielded last.
    cur: (u16, u8, u8),
    /// Days between the base event's start and end date, preserved
    /// across occurrences so multi-day events stay multi-day.
    span_days: i32,
}

impl Pending {
    /// `None` when the rule can't produce anything beyond `DTSTART`.
    fn new(base: Event, rule: Recur) -> Option<Self> {
        if rule.count == Some(1) {
            return None;
        }
        let start = days_from_civil(base.year, base.month, base.day);
        let end = days_from_civil(base.end_year, base.end_month, base.end_day);
        Some(Self {
            base,
            rule,
            emitted: 1,
            cur: (base.year, base.month, base.day),
            span_days: i32::try_from((end - start).max(0)).unwrap_or(0),
        })
    }

    fn next(&mut self) -> Option<Event> {
        if self.emitted >= MAX_OCCURRENCES {
            return None;
        }
        if let Some(count) = self.rule.count
            && self.emitted >= count
        {
            return None;
        }
        let next = self.advance()?;
        if let Some(until) = self.rule.until
            && days_from_civil(next.0, next.1, next.2) > days_from_civil(until.0, until.1, until.2)
        {
            return None;
        }
        self.cur = next;
        self.emitted += 1;

        let (ey, em, ed) = shift_date(next.0, next.1, next.2, self.span_days)?;
        Some(Event {
            year: next.0,
            month: next.1,
            day: next.2,
            end_year: ey,
            end_month: em,
            end_day: ed,
            ..self.base
        })
    }

    /// Start date of the occurrence after `self.cur`, or `None` when the
    /// rule has no further match within [`MAX_SEARCH_STEPS`].
    fn advance(&self) -> Option<(u16, u8, u8)> {
        let (y, m, d) = self.cur;
        let interval = self.rule.interval.max(1) as i32;

        match self.rule.freq {
            Freq::Daily => shift_date(y, m, d, interval),
            Freq::Weekly if self.rule.byday == 0 => shift_date(y, m, d, 7 * interval),
            Freq::Weekly => {
                // Week numbers are counted from the Monday of DTSTART's
                // week, so INTERVAL=2 means "every other week", not
                // "14 days after the previous hit".
                //
                // Finish the current week first, then jump straight to
                // the next selected week.  Walking day by day instead
                // made the search budget an implicit cap on INTERVAL:
                // at one step per day, INTERVAL=58 (406 days) exhausted
                // it and the rule silently collapsed to DTSTART alone —
                // while the same rule *without* BYDAY took the fast path
                // above and expanded fine.
                let base_week = week_index(self.base.year, self.base.month, self.base.day);
                let cur = days_from_civil(y, m, d);
                let cur_weekday = weekday(cur) as i64;

                // Remaining BYDAY weekdays in the week we're already in,
                // but only if this week is one the INTERVAL selects.
                let this_week = week_index(y, m, d);
                if (this_week - base_week).rem_euclid(interval as i64) == 0 {
                    for wd in (cur_weekday + 1)..7 {
                        if self.rule.byday & (1 << wd) != 0 {
                            return civil_from_days(cur + (wd - cur_weekday));
                        }
                    }
                }

                // Otherwise advance to the Monday of the next selected
                // week and take its first BYDAY weekday.
                let elapsed = (this_week - base_week).rem_euclid(interval as i64);
                let weeks_ahead = interval as i64 - elapsed;
                let next_monday = cur - cur_weekday + 7 * weeks_ahead;
                for wd in 0..7 {
                    if self.rule.byday & (1 << wd) != 0 {
                        return civil_from_days(next_monday + wd);
                    }
                }
                None
            }
            Freq::Monthly => {
                // Keep the day-of-month; months too short to hold it (a
                // 31st in February) are skipped, per RFC 5545.
                let mut months = (y as i32) * 12 + (m as i32 - 1);
                for _ in 0..MAX_SEARCH_STEPS {
                    months += interval;
                    // `as u16` would wrap here on a large INTERVAL and
                    // hand back a year in the past, which then reads as a
                    // perfectly valid occurrence.  Run off the end of the
                    // calendar instead.
                    let ny = u16::try_from(months / 12).ok()?;
                    let nm = (months % 12) as u8 + 1;
                    if d <= days_in_month(ny, nm) {
                        return Some((ny, nm, d));
                    }
                }
                None
            }
            Freq::Yearly => {
                // Same guard for 29 February in a non-leap year.
                let mut year = y as i32;
                for _ in 0..MAX_SEARCH_STEPS {
                    year += interval;
                    let ny = u16::try_from(year).ok()?;
                    if d <= days_in_month(ny, m) {
                        return Some((ny, m, d));
                    }
                }
                None
            }
        }
    }
}

/// Parse an `RRULE` value.  Unknown parts are ignored; an unsupported or
/// missing `FREQ` rejects the whole rule (the event then imports as a
/// single occurrence, which is what happened before RRULE was honoured).
fn parse_rrule(value: &[u8]) -> Option<Recur> {
    let mut rule = Recur {
        freq: Freq::Daily,
        interval: 1,
        count: None,
        until: None,
        byday: 0,
    };
    let mut have_freq = false;

    for part in value.split(|&b| b == b';') {
        let Some(eq) = part.iter().position(|&b| b == b'=') else {
            continue;
        };
        let (key, val) = (&part[..eq], &part[eq + 1..]);
        match key {
            b"FREQ" => {
                rule.freq = match val {
                    b"DAILY" => Freq::Daily,
                    b"WEEKLY" => Freq::Weekly,
                    b"MONTHLY" => Freq::Monthly,
                    b"YEARLY" => Freq::Yearly,
                    // SECONDLY / MINUTELY / HOURLY have no meaning for a
                    // day-granularity calendar screen.
                    _ => return None,
                };
                have_freq = true;
            }
            b"INTERVAL" => rule.interval = digits(val).unwrap_or(1).clamp(1, u16::MAX as u32) as u16,
            b"COUNT" => rule.count = digits(val).map(|n| n.min(u16::MAX as u32) as u16),
            // UNTIL is a full timestamp; only its date matters here.
            b"UNTIL" if val.len() >= 8 => {
                let y = digits(&val[0..4])? as u16;
                let mo = digits(&val[4..6])? as u8;
                let d = digits(&val[6..8])? as u8;
                if (1..=12).contains(&mo) && d >= 1 && d <= days_in_month(y, mo) {
                    rule.until = Some((y, mo, d));
                }
            }
            b"BYDAY" => {
                for day in val.split(|&b| b == b',') {
                    // Strip an ordinal prefix ("2MO", "-1FR") — we don't
                    // implement positional BYDAY, but the weekday still
                    // narrows the rule usefully.
                    let name = &day[day.len().saturating_sub(2)..];
                    let bit = match name {
                        b"MO" => 0,
                        b"TU" => 1,
                        b"WE" => 2,
                        b"TH" => 3,
                        b"FR" => 4,
                        b"SA" => 5,
                        b"SU" => 6,
                        _ => continue,
                    };
                    rule.byday |= 1 << bit;
                }
            }
            _ => {}
        }
    }

    have_freq.then_some(rule)
}

// ── Date arithmetic ─────────────────────────────────────────────────────────
//
// Plain civil-calendar maths (Howard Hinnant's days_from_civil), so
// recurrence expansion stays exact and allocation-free without pulling a
// date library into the parser.

/// Days since 1970-01-01 for a proleptic-Gregorian date.
pub(super) fn days_from_civil(y: u16, m: u8, d: u8) -> i64 {
    let y = y as i64 - if m <= 2 { 1 } else { 0 };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400; // [0, 399]
    let m = m as i64;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d as i64 - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    era * 146097 + doe - 719468
}

/// Inverse of [`days_from_civil`].  `None` if the result falls outside a
/// `u16` year.
pub(super) fn civil_from_days(z: i64) -> Option<(u16, u8, u8)> {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u8;
    let m = (mp + if mp < 10 { 3 } else { -9 }) as u8;
    let y = y + if m <= 2 { 1 } else { 0 };
    Some((u16::try_from(y).ok()?, m, d))
}

/// Move a date by `delta` days.
fn shift_date(y: u16, m: u8, d: u8, delta: i32) -> Option<(u16, u8, u8)> {
    civil_from_days(days_from_civil(y, m, d) + delta as i64)
}

/// Weekday of a day number, 0 = Monday .. 6 = Sunday.  Day 0
/// (1970-01-01) was a Thursday.
fn weekday(days: i64) -> u8 {
    (days + 3).rem_euclid(7) as u8
}

/// Index of the Monday-anchored week a date falls in.
fn week_index(y: u16, m: u8, d: u8) -> i64 {
    let days = days_from_civil(y, m, d);
    (days - weekday(days) as i64).div_euclid(7)
}

fn days_in_month(year: u16, month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            let leap =
                (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400);
            if leap { 29 } else { 28 }
        }
        _ => 0,
    }
}

impl Iterator for Parser<'_> {
    type Item = Event;

    fn next(&mut self) -> Option<Event> {
        // Finish the current recurrence before reading more of the file.
        if let Some(ev) = self.next_occurrence() {
            return Some(ev);
        }

        loop {
            // Find the next BEGIN:VEVENT.
            loop {
                let line = self.next_line()?;
                if line == b"BEGIN:VEVENT" {
                    break;
                }
            }

            // Collect DTSTART, DTEND, RRULE and SUMMARY until END:VEVENT.
            let mut dtstart: Option<ParsedDateTime> = None;
            let mut dtend: Option<ParsedDateTime> = None;
            let mut rrule: Option<Recur> = None;
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
                } else if let Some(value) = match_property(line, b"RRULE") {
                    rrule = parse_rrule(value);
                } else if let Some(value) = match_property(line, b"SUMMARY") {
                    copy_summary(&mut summary, value);
                }
            }
            self.consumed = self.pos;

            let Some(start) = dtstart else {
                continue; // dtstart-less event — skip to next.
            };
            let base = build_event(start, dtend, summary);

            if let Some(rule) = rrule {
                // DTSTART is always the rule's first occurrence; the
                // pending state produces the rest on later calls.
                self.pending = Pending::new(base, rule);
            }
            return Some(base);
        }
    }
}

impl Parser<'_> {
    /// Next occurrence of the in-flight `RRULE`, if any is left.
    fn next_occurrence(&mut self) -> Option<Event> {
        let pending = self.pending.as_mut()?;
        match pending.next() {
            Some(ev) => Some(ev),
            None => {
                self.pending = None;
                None
            }
        }
    }
}

/// Assemble an [`Event`] from the parsed `DTSTART` / `DTEND` pair.
fn build_event(
    start: ParsedDateTime,
    end: Option<ParsedDateTime>,
    summary: [u8; SUMMARY_LEN],
) -> Event {
    let (y, mo, d, h, mi, utc, all_day) = start;
    // Default end = start (zero-duration event when DTEND is missing).
    // Same UTC flag so the caller's timezone conversion is consistent
    // across both timestamps.
    let (ey, emo, ed, eh, emi, eutc, _) = end.unwrap_or((y, mo, d, h, mi, utc, all_day));

    if all_day {
        // An all-day DTEND is exclusive: `DTEND;VALUE=DATE:20250812` on a
        // 20250811 event means "the 11th only".  Pull it back a day and
        // run to 23:59 so the day-view shows the days actually covered.
        // A missing DTEND leaves the event on its start day.
        let (ly, lmo, ld) = match end {
            Some(_) => shift_date(ey, emo, ed, -1).unwrap_or((y, mo, d)),
            None => (y, mo, d),
        };
        // Guard against a DTEND at or before DTSTART.
        let (ly, lmo, ld) = if days_from_civil(ly, lmo, ld) < days_from_civil(y, mo, d) {
            (y, mo, d)
        } else {
            (ly, lmo, ld)
        };
        return Event {
            year: y,
            month: mo,
            day: d,
            hour: 0,
            minute: 0,
            start_is_utc: false,
            end_year: ly,
            end_month: lmo,
            end_day: ld,
            end_hour: 23,
            end_minute: 59,
            end_is_utc: false,
            all_day: true,
            summary,
        };
    }

    Event {
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
        all_day: false,
        summary,
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

/// Parse `YYYYMMDDTHHMMSS` (with an optional trailing `Z`) or a date-only
/// `YYYYMMDD` into `(year, month, day, hour, minute, is_utc, is_date_only)`.
/// Seconds are discarded.
fn parse_datetime(value: &[u8]) -> Option<ParsedDateTime> {
    // Need at least YYYYMMDD = 8 bytes.
    if value.len() < 8 {
        return None;
    }
    let year = digits(&value[0..4])? as u16;
    let month = digits(&value[4..6])? as u8;
    let day = digits(&value[6..8])? as u8;
    if month == 0 || month > 12 {
        return None;
    }
    // Validate the day against the actual month length (incl. leap years) so a
    // malformed date like 0230 / 0431 is rejected instead of producing a
    // phantom one-shot alarm that can never fire or auto-disable.
    if day == 0 || day > days_in_month(year, month) {
        return None;
    }

    // Date-only value (`VALUE=DATE`, all-day event).  Anything else must
    // carry a time, introduced by `T`.
    if value.len() == 8 {
        return Some((year, month, day, 0, 0, false, true));
    }
    if value[8] != b'T' || value.len() < 13 {
        return None;
    }
    let hour = digits(&value[9..11])? as u8;
    let minute = digits(&value[11..13])? as u8;
    if hour > 23 || minute > 59 {
        return None;
    }
    // `Z` may follow the seconds — accept either trailing position
    // (right after HHMM if seconds were stripped, or right after SS).
    let is_utc = matches!(value.last(), Some(&b'Z'));
    Some((year, month, day, hour, minute, is_utc, false))
}

/// Parse an all-digit byte slice.  `None` on a non-digit, on an empty
/// slice, or on a value too large for `u32` — `RRULE` parameters are
/// unbounded in the file, and silently wrapping a 20-digit `COUNT` into
/// a small number is worse than ignoring the rule.
fn digits(bytes: &[u8]) -> Option<u32> {
    if bytes.is_empty() {
        return None;
    }
    let mut n = 0u32;
    for &b in bytes {
        if !b.is_ascii_digit() {
            return None;
        }
        n = n.checked_mul(10)?.checked_add((b - b'0') as u32)?;
    }
    Some(n)
}

/// Copy a `SUMMARY` value into the fixed label buffer: RFC 5545 escapes
/// decoded, non-ASCII transliterated where a sensible ASCII spelling
/// exists, everything else dropped.
fn copy_summary(dst: &mut [u8; SUMMARY_LEN], src: &[u8]) {
    let mut i = 0;
    let mut src = src;

    while let Some((&b, rest)) = src.split_first() {
        if i >= dst.len() {
            break;
        }
        src = rest;

        // RFC 5545 escapes.  `\n` / `\N` is a line break inside a value;
        // the label is one line, so it becomes a space.
        if b == b'\\' {
            let Some((&esc, rest)) = src.split_first() else {
                break;
            };
            src = rest;
            let out = match esc {
                b'n' | b'N' => b' ',
                b',' | b';' | b'\\' => esc,
                // Unknown escape — keep the escaped character as-is.
                other => other,
            };
            if (0x20..=0x7e).contains(&out) {
                dst[i] = out;
                i += 1;
            }
            continue;
        }

        if b < 0x80 {
            // Printable ASCII passes through; control characters (stray
            // tabs from a folded line, say) become spaces.
            let out = if (0x20..=0x7e).contains(&b) { b } else { b' ' };
            dst[i] = out;
            i += 1;
            continue;
        }

        // Multi-byte UTF-8: decode, then keep or transliterate.
        let (cp, used) = decode_utf8(b, src);
        src = &src[used..];

        // The label is drawn with an ISO 8859-1 font, so every code point
        // up to U+00FF has a glyph — Æ, é and ø render as themselves and
        // only what Latin-1 can't hold needs an ASCII spelling.  The
        // buffer holds Latin-1, one byte per glyph, so an accented title
        // gets the same 31 characters as a plain one.
        if (0xa0..=0xff).contains(&cp) {
            dst[i] = cp as u8;
            i += 1;
            continue;
        }

        for &out in translit(cp) {
            if i >= dst.len() {
                break;
            }
            dst[i] = out;
            i += 1;
        }
    }

    // Zero-pad the rest.
    for slot in &mut dst[i..] {
        *slot = 0;
    }
}

/// Decode the remainder of a UTF-8 sequence whose leading byte is `lead`.
/// Returns the code point and how many *continuation* bytes were used.
/// Malformed input decodes to `0` (dropped by [`translit`]).
fn decode_utf8(lead: u8, rest: &[u8]) -> (u32, usize) {
    let (need, mut cp) = match lead {
        0xc0..=0xdf => (1, (lead & 0x1f) as u32),
        0xe0..=0xef => (2, (lead & 0x0f) as u32),
        0xf0..=0xf7 => (3, (lead & 0x07) as u32),
        // Stray continuation byte or invalid lead.
        _ => return (0, 0),
    };
    if rest.len() < need {
        return (0, rest.len());
    }
    for &b in &rest[..need] {
        if b & 0xc0 != 0x80 {
            return (0, 0);
        }
        cp = (cp << 6) | (b & 0x3f) as u32;
    }
    (cp, need)
}

/// ASCII spelling of a code point the ISO 8859-1 font can't draw, or
/// empty when there is no reasonable one (emoji, CJK, symbols).
///
/// Latin-1 itself never reaches here — [`copy_summary`] keeps those code
/// points as they are.  What's left is the Latin Extended-A letters that
/// turn up in European conference programmes and the punctuation word
/// processors substitute in (curly quotes, dashes, ellipsis).
fn translit(cp: u32) -> &'static [u8] {
    match cp {
        // Latin Extended-A: accented letters, by base letter.
        0x100..=0x105 => {
            if cp.is_multiple_of(2) {
                b"A"
            } else {
                b"a"
            }
        }
        0x106..=0x10d => {
            if cp.is_multiple_of(2) {
                b"C"
            } else {
                b"c"
            }
        }
        0x10e..=0x111 => {
            if cp.is_multiple_of(2) {
                b"D"
            } else {
                b"d"
            }
        }
        0x112..=0x11b => {
            if cp.is_multiple_of(2) {
                b"E"
            } else {
                b"e"
            }
        }
        0x11c..=0x123 => {
            if cp.is_multiple_of(2) {
                b"G"
            } else {
                b"g"
            }
        }
        0x124..=0x127 => {
            if cp.is_multiple_of(2) {
                b"H"
            } else {
                b"h"
            }
        }
        0x128..=0x12f => {
            if cp.is_multiple_of(2) {
                b"I"
            } else {
                b"i"
            }
        }
        0x130 => b"I", // dotted capital I
        0x131 => b"i", // dotless i
        0x132 => b"IJ",
        0x133 => b"ij",
        0x134 | 0x135 => {
            if cp.is_multiple_of(2) {
                b"J"
            } else {
                b"j"
            }
        }
        0x136 | 0x137 => {
            if cp.is_multiple_of(2) {
                b"K"
            } else {
                b"k"
            }
        }
        0x139..=0x142 => {
            if !cp.is_multiple_of(2) {
                b"L"
            } else {
                b"l"
            }
        }
        0x143..=0x148 => {
            if !cp.is_multiple_of(2) {
                b"N"
            } else {
                b"n"
            }
        }
        0x138 => b"k", // kra
        0x149 => b"n", // 'n
        0x14a => b"N",
        0x14b => b"n",
        0x152 => b"OE",
        0x153 => b"oe",
        0x14c..=0x151 => {
            if cp.is_multiple_of(2) {
                b"O"
            } else {
                b"o"
            }
        }
        0x154..=0x159 => {
            if cp.is_multiple_of(2) {
                b"R"
            } else {
                b"r"
            }
        }
        0x15a..=0x161 => {
            if cp.is_multiple_of(2) {
                b"S"
            } else {
                b"s"
            }
        }
        0x162..=0x167 => {
            if cp.is_multiple_of(2) {
                b"T"
            } else {
                b"t"
            }
        }
        0x168..=0x173 => {
            if cp.is_multiple_of(2) {
                b"U"
            } else {
                b"u"
            }
        }
        0x174 | 0x175 => {
            if cp.is_multiple_of(2) {
                b"W"
            } else {
                b"w"
            }
        }
        0x176..=0x178 => {
            if cp == 0x177 {
                b"y"
            } else {
                b"Y"
            }
        }
        0x179..=0x17e => {
            if !cp.is_multiple_of(2) {
                b"Z"
            } else {
                b"z"
            }
        }
        0x17f => b"s", // long s
        // Punctuation word processors love to substitute in.
        0x2010..=0x2015 => b"-",
        0x2018..=0x201b => b"'",
        0x201c..=0x201f => b"\"",
        0x2022 => b"*",
        0x2026 => b"...",
        0x20ac => b"EUR",
        0x2122 => b"(tm)",
        // Emoji, CJK, anything else with no Latin-1 glyph and no sensible
        // spelling.  Shown as `?` rather than dropped: a title that reads
        // "Party ? time" tells you something was there, where "Party
        // time" quietly lies about what the programme said.
        _ => b"?",
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

    /// Helper: parse a single-VEVENT document and return its summary.
    fn summary_of(summary: &str) -> std::string::String {
        let doc = std::format!("BEGIN:VEVENT\nSUMMARY:{summary}\nDTSTART:20250101T120000\nEND:VEVENT\n");
        let ev = Parser::new(doc.as_bytes()).next().unwrap();
        ev.summary_str().as_str().into()
    }

    #[test]
    fn keeps_latin1_letters_verbatim() {
        // The label is drawn with an ISO 8859-1 font, so these all have
        // real glyphs — no need to flatten them to ASCII.
        assert_eq!(summary_of("Café Talk"), "Café Talk");
        assert_eq!(summary_of("CyberÆgg"), "CyberÆgg");
        assert_eq!(summary_of("Smørrebrød"), "Smørrebrød");
        assert_eq!(summary_of("Straße"), "Straße");
        assert_eq!(summary_of("Ångström"), "Ångström");
    }

    #[test]
    fn transliterates_beyond_latin1() {
        // Latin Extended-A has no Latin-1 glyph, so it gets an ASCII
        // spelling rather than vanishing mid-word.
        assert_eq!(summary_of("Škoda"), "Skoda");
        assert_eq!(summary_of("Gdańsk"), "Gdansk");
        assert_eq!(summary_of("Łódź"), "Lódz");
        assert_eq!(summary_of("“quoted” — dash…"), "\"quoted\" - dash...");
    }

    #[test]
    fn marks_untranslatable_code_points() {
        // Emoji and CJK have no Latin-1 glyph and no ASCII spelling, so
        // they become `?` — silently dropping them would misreport what
        // the programme actually said.
        assert_eq!(summary_of("Party 🎉 time"), "Party ? time");
        assert_eq!(summary_of("東京 talk"), "?? talk");
    }

    #[test]
    fn decodes_rfc5545_escapes() {
        assert_eq!(summary_of(r"Rust\, part 2"), "Rust, part 2");
        assert_eq!(summary_of(r"A\;B"), "A;B");
        assert_eq!(summary_of(r"line\nbreak"), "line break");
        assert_eq!(summary_of(r"back\\slash"), r"back\slash");
    }

    #[test]
    fn accented_titles_get_the_full_label_length() {
        // Latin-1 storage is one byte per glyph, so an accented title is
        // allowed just as many characters as a plain one.
        let s = summary_of("ÆÆÆÆÆÆÆÆÆÆÆÆÆÆÆÆÆÆÆÆÆÆÆÆÆÆÆÆÆÆÆÆÆÆÆÆ");
        assert_eq!(s.chars().count(), SUMMARY_LEN);
    }

    // ── All-day events ──────────────────────────────────────────────────

    #[test]
    fn all_day_event_covers_the_whole_day() {
        let bytes = b"BEGIN:VEVENT\n\
SUMMARY:Camp build-up\n\
DTSTART;VALUE=DATE:20250811\n\
DTEND;VALUE=DATE:20250812\n\
END:VEVENT\n";
        let ev = Parser::new(bytes).next().unwrap();
        assert!(ev.all_day);
        assert_eq!((ev.year, ev.month, ev.day), (2025, 8, 11));
        assert_eq!((ev.hour, ev.minute), (0, 0));
        // DTEND is exclusive — the event still ends on the 11th.
        assert_eq!((ev.end_year, ev.end_month, ev.end_day), (2025, 8, 11));
        assert_eq!((ev.end_hour, ev.end_minute), (23, 59));
    }

    #[test]
    fn multi_day_all_day_event_keeps_its_last_day() {
        let bytes = b"BEGIN:VEVENT\n\
SUMMARY:Bornhack\n\
DTSTART;VALUE=DATE:20250811\n\
DTEND;VALUE=DATE:20250818\n\
END:VEVENT\n";
        let ev = Parser::new(bytes).next().unwrap();
        assert_eq!((ev.end_year, ev.end_month, ev.end_day), (2025, 8, 17));
    }

    #[test]
    fn all_day_event_without_dtend_stays_on_its_start_day() {
        let bytes = b"BEGIN:VEVENT\nSUMMARY:Holiday\nDTSTART;VALUE=DATE:20250101\nEND:VEVENT\n";
        let ev = Parser::new(bytes).next().unwrap();
        assert!(ev.all_day);
        assert_eq!((ev.end_year, ev.end_month, ev.end_day), (2025, 1, 1));
        assert_eq!((ev.end_hour, ev.end_minute), (23, 59));
    }

    #[test]
    fn timed_events_are_not_marked_all_day() {
        let ev = Parser::new(SAMPLE).next().unwrap();
        assert!(!ev.all_day);
    }

    // ── Recurrence ──────────────────────────────────────────────────────

    /// Collect `(month, day, hour)` for every occurrence, for compact
    /// assertions below.
    fn occurrences(doc: &[u8]) -> std::vec::Vec<(u8, u8, u8)> {
        Parser::new(doc)
            .map(|e| (e.month, e.day, e.hour))
            .collect()
    }

    #[test]
    fn daily_rrule_with_count_expands() {
        let doc = b"BEGIN:VEVENT\n\
SUMMARY:Standup\n\
DTSTART:20250811T090000\n\
DTEND:20250811T091500\n\
RRULE:FREQ=DAILY;COUNT=3\n\
END:VEVENT\n";
        assert_eq!(
            occurrences(doc),
            [(8, 11, 9), (8, 12, 9), (8, 13, 9)],
            "DTSTART counts as the first occurrence"
        );
    }

    #[test]
    fn daily_rrule_honours_interval_and_until() {
        let doc = b"BEGIN:VEVENT\n\
SUMMARY:Every other day\n\
DTSTART:20250811T090000\n\
RRULE:FREQ=DAILY;INTERVAL=2;UNTIL=20250816T235959Z\n\
END:VEVENT\n";
        assert_eq!(
            occurrences(doc),
            [(8, 11, 9), (8, 13, 9), (8, 15, 9)],
            "the 17th is past UNTIL"
        );
    }

    #[test]
    fn weekly_rrule_with_byday_picks_named_weekdays() {
        // 2025-08-11 is a Monday.
        let doc = b"BEGIN:VEVENT\n\
SUMMARY:Mon+Wed\n\
DTSTART:20250811T180000\n\
RRULE:FREQ=WEEKLY;BYDAY=MO,WE;COUNT=4\n\
END:VEVENT\n";
        assert_eq!(
            occurrences(doc),
            [(8, 11, 18), (8, 13, 18), (8, 18, 18), (8, 20, 18)]
        );
    }

    /// The BYDAY search used to walk one day per step against a 400-step
    /// budget, so `INTERVAL >= 58` (406 days) exhausted it and the rule
    /// collapsed to DTSTART alone — while the same rule without BYDAY
    /// took a different path and expanded fine.  A large INTERVAL must
    /// not be a cliff.
    #[test]
    fn weekly_byday_survives_a_large_interval() {
        for interval in [1u32, 2, 57, 58, 200, 1000] {
            let doc = std::format!(
                "BEGIN:VEVENT\nSUMMARY:X\nDTSTART:20260105T090000\n\
                 RRULE:FREQ=WEEKLY;INTERVAL={interval};BYDAY=MO;COUNT=4\nEND:VEVENT\n"
            );
            let evs: std::vec::Vec<_> = Parser::new(doc.as_bytes()).collect();
            assert_eq!(evs.len(), 4, "INTERVAL={interval} lost occurrences");

            // 2026-01-05 is a Monday; every occurrence must be one too,
            // spaced exactly `interval` weeks apart.
            let base = days_from_civil(2026, 1, 5);
            for (i, e) in evs.iter().enumerate() {
                let day = days_from_civil(e.year, e.month, e.day);
                assert_eq!(weekday(day), 0, "INTERVAL={interval}: not a Monday");
                assert_eq!(
                    day - base,
                    (i as i64) * 7 * interval as i64,
                    "INTERVAL={interval}: wrong spacing at {i}"
                );
            }
        }
    }

    /// Multiple BYDAY weekdays with an interval: all of the named days in
    /// each selected week, and no days from the weeks in between.
    #[test]
    fn weekly_byday_multiple_days_with_interval() {
        // 2026-01-05 is a Monday.
        let doc = b"BEGIN:VEVENT\nSUMMARY:X\nDTSTART:20260105T090000\n\
RRULE:FREQ=WEEKLY;INTERVAL=3;BYDAY=MO,WE,FR;COUNT=6\nEND:VEVENT\n";
        let got: std::vec::Vec<_> = Parser::new(doc)
            .map(|e| (e.month, e.day))
            .collect();
        assert_eq!(
            got,
            [(1, 5), (1, 7), (1, 9), (1, 26), (1, 28), (1, 30)],
            "three days in week 0, then a three-week jump"
        );
    }

    #[test]
    fn weekly_rrule_interval_skips_whole_weeks() {
        let doc = b"BEGIN:VEVENT\n\
SUMMARY:Fortnightly\n\
DTSTART:20250811T180000\n\
RRULE:FREQ=WEEKLY;INTERVAL=2;COUNT=3\n\
END:VEVENT\n";
        assert_eq!(occurrences(doc), [(8, 11, 18), (8, 25, 18), (9, 8, 18)]);
    }

    #[test]
    fn monthly_rrule_skips_months_too_short_for_the_day() {
        let doc = b"BEGIN:VEVENT\n\
SUMMARY:End of month\n\
DTSTART:20250131T120000\n\
RRULE:FREQ=MONTHLY;COUNT=3\n\
END:VEVENT\n";
        // February has no 31st, so the rule lands on March next.
        assert_eq!(occurrences(doc), [(1, 31, 12), (3, 31, 12), (5, 31, 12)]);
    }

    #[test]
    fn yearly_rrule_repeats_the_same_date() {
        let doc = b"BEGIN:VEVENT\n\
SUMMARY:Birthday\n\
DTSTART:20250811T000000\n\
RRULE:FREQ=YEARLY;COUNT=2\n\
END:VEVENT\n";
        let years: std::vec::Vec<_> = Parser::new(doc).map(|e| e.year).collect();
        assert_eq!(years, [2025, 2026]);
    }

    #[test]
    fn recurring_event_keeps_its_duration() {
        // A two-day event repeating weekly must stay two days long.
        let doc = b"BEGIN:VEVENT\n\
SUMMARY:Weekend\n\
DTSTART:20250808T180000\n\
DTEND:20250810T120000\n\
RRULE:FREQ=WEEKLY;COUNT=2\n\
END:VEVENT\n";
        let evs: std::vec::Vec<_> = Parser::new(doc).collect();
        assert_eq!((evs[1].month, evs[1].day), (8, 15));
        assert_eq!((evs[1].end_month, evs[1].end_day), (8, 17));
        assert_eq!((evs[1].end_hour, evs[1].end_minute), (12, 0));
    }

    #[test]
    fn unbounded_rrule_is_capped() {
        let doc = b"BEGIN:VEVENT\n\
SUMMARY:Forever\n\
DTSTART:20250101T090000\n\
RRULE:FREQ=DAILY\n\
END:VEVENT\n";
        assert_eq!(Parser::new(doc).count(), MAX_OCCURRENCES as usize);
    }

    /// `RRULE` values come from a host-writable file, so every numeric
    /// parameter is attacker-controlled.  None of these may wrap into a
    /// plausible-looking date or run away.
    #[test]
    fn hostile_rrule_parameters_are_rejected_not_wrapped() {
        // A huge INTERVAL used to wrap the computed year back into the
        // past via `as u16`, producing occurrences *before* DTSTART.
        let doc = b"BEGIN:VEVENT\n\
SUMMARY:Wrap\n\
DTSTART:20260101T090000\n\
RRULE:FREQ=MONTHLY;INTERVAL=65535;COUNT=5\n\
END:VEVENT\n";
        let evs: std::vec::Vec<_> = Parser::new(doc).collect();
        for e in &evs {
            assert!(
                (e.year, e.month, e.day) >= (2026, 1, 1),
                "occurrence before DTSTART: {:?}",
                (e.year, e.month, e.day)
            );
        }

        // Digit strings longer than u32 can hold must not wrap into a
        // small COUNT/INTERVAL.
        for rule in [
            &b"FREQ=DAILY;COUNT=99999999999999999999"[..],
            &b"FREQ=DAILY;INTERVAL=99999999999999999999"[..],
            &b"FREQ=DAILY;UNTIL=99999999999999999999"[..],
        ] {
            let doc = std::format!(
                "BEGIN:VEVENT\nSUMMARY:X\nDTSTART:20260101T090000\nRRULE:{}\nEND:VEVENT\n",
                core::str::from_utf8(rule).unwrap()
            );
            let n = Parser::new(doc.as_bytes()).count();
            assert!(
                n <= MAX_OCCURRENCES as usize,
                "rule {rule:?} expanded to {n}"
            );
        }
    }

    /// Every occurrence a rule yields must be a real date, whatever the
    /// file asked for.
    #[test]
    fn every_occurrence_is_a_valid_date() {
        for rule in [
            "FREQ=DAILY;INTERVAL=400",
            "FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR,SA,SU",
            "FREQ=MONTHLY;INTERVAL=7",
            "FREQ=YEARLY;INTERVAL=3",
        ] {
            let doc = std::format!(
                "BEGIN:VEVENT\nSUMMARY:X\nDTSTART:20260131T090000\nRRULE:{rule}\nEND:VEVENT\n"
            );
            let mut prev = None;
            for e in Parser::new(doc.as_bytes()) {
                assert!((1..=12).contains(&e.month), "{rule}: month {}", e.month);
                assert!(
                    e.day >= 1 && e.day <= days_in_month(e.year, e.month),
                    "{rule}: {}-{}-{} is not a real date",
                    e.year,
                    e.month,
                    e.day
                );
                let now = days_from_civil(e.year, e.month, e.day);
                if let Some(p) = prev {
                    assert!(now > p, "{rule}: occurrences must move forward");
                }
                prev = Some(now);
            }
        }
    }

    #[test]
    fn unsupported_freq_imports_as_a_single_event() {
        let doc = b"BEGIN:VEVENT\n\
SUMMARY:Too fine-grained\n\
DTSTART:20250101T090000\n\
RRULE:FREQ=HOURLY;COUNT=5\n\
END:VEVENT\n";
        assert_eq!(Parser::new(doc).count(), 1);
    }

    #[test]
    fn recurrence_does_not_swallow_the_next_event() {
        let doc = b"BEGIN:VEVENT\n\
SUMMARY:Repeating\n\
DTSTART:20250101T090000\n\
RRULE:FREQ=DAILY;COUNT=2\n\
END:VEVENT\n\
BEGIN:VEVENT\n\
SUMMARY:Single\n\
DTSTART:20250601T100000\n\
END:VEVENT\n";
        let evs: std::vec::Vec<_> = Parser::new(doc).collect();
        assert_eq!(evs.len(), 3);
        assert_eq!(evs[2].summary_str(), "Single");
        assert_eq!(evs[2].month, 6);
    }

    // ── Date arithmetic ─────────────────────────────────────────────────

    #[test]
    fn civil_day_conversion_round_trips() {
        for &(y, m, d) in &[
            (1970u16, 1u8, 1u8),
            (2000, 2, 29),
            (2025, 8, 11),
            (2026, 12, 31),
            (2100, 3, 1),
        ] {
            let days = days_from_civil(y, m, d);
            assert_eq!(civil_from_days(days), Some((y, m, d)));
        }
    }

    #[test]
    fn weekday_is_monday_zero() {
        // 2025-08-11 is a Monday, 2025-08-17 a Sunday.
        assert_eq!(weekday(days_from_civil(2025, 8, 11)), 0);
        assert_eq!(weekday(days_from_civil(2025, 8, 17)), 6);
        assert_eq!(weekday(days_from_civil(1970, 1, 1)), 3); // Thursday
    }

    #[test]
    fn week_index_is_stable_within_a_week() {
        let mon = week_index(2025, 8, 11);
        assert_eq!(week_index(2025, 8, 17), mon, "Sunday closes the same week");
        assert_eq!(week_index(2025, 8, 18), mon + 1);
    }

    // ── Chunked reading ─────────────────────────────────────────────────

    #[test]
    fn consumed_tracks_the_last_complete_event() {
        let mut p = Parser::new(SAMPLE);
        assert_eq!(p.consumed(), 0);
        p.next().unwrap();
        let after_first = p.consumed();
        assert!(after_first > 0);
        // The offset lands just past the first END:VEVENT, so re-parsing
        // from there yields exactly the remaining events.
        assert_eq!(Parser::new(&SAMPLE[after_first..]).count(), 2);
    }

    #[test]
    fn consumed_excludes_a_truncated_trailing_event() {
        let doc = b"BEGIN:VEVENT\n\
SUMMARY:Complete\n\
DTSTART:20250101T090000\n\
END:VEVENT\n\
BEGIN:VEVENT\n\
SUMMARY:Cut off mid-";
        let mut p = Parser::new(doc);
        assert_eq!(p.next().unwrap().summary_str(), "Complete");
        assert!(p.next().is_none());
        // Everything from `consumed` on is the incomplete event, which a
        // chunked reader must retry once it has more bytes.
        assert_eq!(&doc[p.consumed()..][..13], b"BEGIN:VEVENT\n");
    }

    // ── Shapes the real Bornhack export actually uses ───────────────────

    /// Trimmed from <https://bornhack.dk/bornhack-2026/program/ics/>: UTC
    /// timestamps with no TZID, folded `DESCRIPTION` continuation lines,
    /// `LOCATION` / `UID` / `DTSTAMP` we ignore, and titles carrying
    /// Latin-1 letters, an em dash and a curly apostrophe.
    const BORNHACK: &[u8] = b"BEGIN:VCALENDAR\r
VERSION:2.0\r
PRODID:-//BornHack Website iCal Generator//bornhack.dk//\r
NAME:BornHack 2026\r
BEGIN:VEVENT\r
SUMMARY:BornHack Cyber\xc3\x86gg\r
DTSTART:20260716T110000Z\r
DTEND:20260716T120000Z\r
DTSTAMP:20260725T223201Z\r
UID:17841168-0012-2880-29b0-084d339246f5\r
DESCRIPTION:URL: https://bornhack.dk/bornhack-2026/program/cyberaegg/\\n\\nSpe\r
 aker(s): BornHack\\n\\nRecorded: No\\n\\nWe meet in the star tent\\, and we'\r
 ll go over the badge.\r
LOCATION:Startent at Info Desk\r
END:VEVENT\r
BEGIN:VEVENT\r
SUMMARY:NIS2 \xe2\x80\x94 comparing notes\r
DTSTART:20260717T140000Z\r
DTEND:20260717T150000Z\r
LOCATION:Speakers Tent\r
END:VEVENT\r
BEGIN:VEVENT\r
SUMMARY:BornHack Radio: Jenny\xe2\x80\x99s Half Hour\r
DTSTART:20260718T090000Z\r
DTEND:20260718T093000Z\r
END:VEVENT\r
END:VCALENDAR\r
";

    #[test]
    fn parses_the_bornhack_export_shape() {
        let evs: std::vec::Vec<_> = Parser::new(BORNHACK).collect();
        assert_eq!(evs.len(), 3, "folded DESCRIPTION lines must not confuse it");

        assert_eq!(evs[0].summary_str().as_str(), "BornHack CyberÆgg");
        assert_eq!((evs[0].year, evs[0].month, evs[0].day), (2026, 7, 16));
        assert!(evs[0].start_is_utc, "no TZID, plain Z-suffixed UTC");
        assert_eq!((evs[0].hour, evs[0].end_hour), (11, 12));

        // Em dash and curly apostrophe have no Latin-1 glyph but do have
        // an obvious ASCII spelling.
        assert_eq!(evs[1].summary_str().as_str(), "NIS2 - comparing notes");
        assert_eq!(evs[2].summary_str().as_str(), "BornHack Radio: Jenny's Half Ho");
    }

    #[test]
    fn chunked_parse_matches_a_single_pass() {
        // What the boot importer does: walk the file in windows, keeping
        // the tail of any VEVENT that straddled the boundary.
        let one_pass = Parser::new(BORNHACK).count();

        // Windows comfortably larger than the biggest VEVENT.
        for window in [512usize, 1024, 4096] {
            assert_eq!(
                chunked_count(BORNHACK, window),
                one_pass,
                "window {window} lost an event"
            );
        }

        // A window too small to hold the first (long-DESCRIPTION) event
        // must skip it and carry on, not spin on the same bytes.
        let tiny = chunked_count(BORNHACK, 64);
        assert!(tiny < one_pass);
    }

    /// Walk `doc` in `window`-sized reads the way the boot importer does,
    /// returning how many events came out.
    fn chunked_count(doc: &[u8], window: usize) -> usize {
        let mut found = 0usize;
        let mut pos = 0usize;
        while pos < doc.len() {
            let end = (pos + window).min(doc.len());
            let mut p = Parser::new(&doc[pos..end]);
            found += (&mut p).count();
            let used = p.consumed();
            // A VEVENT longer than the window can't be recovered — the
            // importer skips it, and so does this.
            pos += if used == 0 { end - pos } else { used };
        }
        found
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
