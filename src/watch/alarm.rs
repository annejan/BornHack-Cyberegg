//! Alarm clock — armed-time + day mask + tone, plus the alarm-edit screen
//! UI and ring playback task.
//!
//! Splits cleanly from `clock.rs` because everything here is about the
//! *armed alarm*: deciding when it fires, persisting its settings, and
//! letting the user edit them on-device.  Clock-face rendering and the
//! 7-segment digit primitives live in [`super::clock`]; we reuse those
//! primitives for the alarm-edit `HH:MM` display.
//!
//! There are [`N_ALARMS`] independent alarm slots.  Slot 0 is the
//! "primary" alarm — it's the one the existing on-screen edit and
//! Settings → Alarm submenu mutate, and it persists under the original
//! `alarm_*` kv keys for backward compatibility.  Slots 1..7 are
//! reachable through the slot-aware `*_n(slot)` accessors and are
//! checked by the trigger every minute alongside slot 0; later commits
//! wire them into UI and persistence.
//!
//! Edit-mode buttons mirror the Settings-menu stepper pattern:
//!
//!   Row-nav (default after entering edit mode):
//!     * Up/Down       — move between fields (Hour → Minute → Days → Tone →
//!       Enabled)
//!     * Fire/Execute  — drill into the selected field, or just toggle Enabled
//!     * Cancel        — exit edit mode (changes are live, no save needed)
//!
//!   Field active (after Fire on a steppable field):
//!     * Up/Down       — increment / decrement the value
//!     * Fire/Execute  — exit field editing, back to row-nav
//!     * Cancel        — exit field editing, back to row-nav

use core::sync::atomic::{AtomicBool, AtomicU8, AtomicU16, Ordering};

use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::mono_font::ascii::FONT_6X10;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{PrimitiveStyle, Rectangle};
use embedded_graphics::text::{Alignment, Baseline, Text, TextStyleBuilder};

use super::clock;
use super::ics::SUMMARY_LEN;
use crate::menu::ButtonId;
use crate::{BLACK, RED, TriColor, WHITE};

/// Maximum number of independent alarm slots.  Slot 0 is the user-editable
/// "primary" alarm; slots 1..N_ALARMS-1 hold imported calendar events and
/// other automation.  At ~11 bytes of atomics per slot, 32 slots cost
/// ~352 bytes of RAM — comfortable for an unfiltered Bornhack day's worth
/// of events.
pub const N_ALARMS: usize = 32;

// ── Edit-mode state ─────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum WatchMode {
    Normal = 0,
    AlarmEdit = 1,
}

#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
enum EditField {
    Hour = 0,
    Minute = 1,
    Days = 2,
    Tone = 3,
    Enabled = 4,
}

static WATCH_MODE: AtomicU8 = AtomicU8::new(WatchMode::Normal as u8);
static EDIT_FIELD: AtomicU8 = AtomicU8::new(EditField::Hour as u8);
/// True while the user has drilled into the selected field with Fire.
/// Up/Down step the value; Fire or Cancel pops back to row-nav.
static EDIT_ACTIVE: AtomicBool = AtomicBool::new(false);

pub fn current_mode() -> WatchMode {
    match WATCH_MODE.load(Ordering::Relaxed) {
        1 => WatchMode::AlarmEdit,
        _ => WatchMode::Normal,
    }
}

fn current_field() -> EditField {
    match EDIT_FIELD.load(Ordering::Relaxed) {
        1 => EditField::Minute,
        2 => EditField::Days,
        3 => EditField::Tone,
        4 => EditField::Enabled,
        _ => EditField::Hour,
    }
}

pub(super) fn enter_edit() {
    WATCH_MODE.store(WatchMode::AlarmEdit as u8, Ordering::Relaxed);
    EDIT_FIELD.store(EditField::Hour as u8, Ordering::Relaxed);
    EDIT_ACTIVE.store(false, Ordering::Relaxed);
}

fn exit_edit() {
    WATCH_MODE.store(WatchMode::Normal as u8, Ordering::Relaxed);
    EDIT_ACTIVE.store(false, Ordering::Relaxed);
}

pub(super) fn is_edit_active() -> bool {
    EDIT_ACTIVE.load(Ordering::Relaxed)
}

/// Move the row-selection one step toward the next/prev field.  Stops at the
/// ends — no wraparound — so it matches `menu_up`/`menu_down`.
fn nav_field(down: bool) {
    let next = match (current_field(), down) {
        (EditField::Hour, true) => EditField::Minute,
        (EditField::Minute, true) => EditField::Days,
        (EditField::Days, true) => EditField::Tone,
        (EditField::Tone, true) => EditField::Enabled,
        (EditField::Enabled, true) => EditField::Enabled, // bottom — stop
        (EditField::Hour, false) => EditField::Hour,      // top — stop
        (EditField::Minute, false) => EditField::Hour,
        (EditField::Days, false) => EditField::Minute,
        (EditField::Tone, false) => EditField::Days,
        (EditField::Enabled, false) => EditField::Tone,
    };
    EDIT_FIELD.store(next as u8, Ordering::Relaxed);
}

fn step_current_field(up: bool) {
    match current_field() {
        EditField::Hour => {
            if up {
                alarm_inc_hour();
            } else {
                alarm_dec_hour();
            }
        }
        EditField::Minute => {
            if up {
                alarm_inc_minute();
            } else {
                alarm_dec_minute();
            }
        }
        EditField::Days => alarm_step_days_preset(up),
        EditField::Tone => {
            if up {
                alarm_inc_melody();
            } else {
                alarm_dec_melody();
            }
        }
        EditField::Enabled => alarm_toggle_enabled(),
    }
}

/// Handle a button press while in alarm-edit mode.  Always consumes
/// the event (returns `true`).
///
/// Two layers of state inside this mode:
///  * row-nav (default) — Up/Left moves to the previous field, Down/Right to
///    the next.  Fire/Execute drills into a steppable field (or just toggles
///    Enabled).  Cancel exits alarm-edit entirely.
///  * field active — Up/Right increment the value, Down/Left decrement.
///    Fire/Execute or Cancel pops back to row-nav.
pub(super) fn dispatch_edit(btn: ButtonId) -> bool {
    if EDIT_ACTIVE.load(Ordering::Relaxed) {
        match btn {
            ButtonId::Up | ButtonId::Right => step_current_field(true),
            ButtonId::Down | ButtonId::Left => step_current_field(false),
            ButtonId::Fire | ButtonId::Execute | ButtonId::Cancel => {
                EDIT_ACTIVE.store(false, Ordering::Relaxed);
            }
        }
        return true;
    }

    // Row-nav (default).
    match btn {
        ButtonId::Up | ButtonId::Left => nav_field(false),
        ButtonId::Down | ButtonId::Right => nav_field(true),
        ButtonId::Fire | ButtonId::Execute => match current_field() {
            // Enabled is a binary toggle — just flip it inline, no extra Fire.
            EditField::Enabled => alarm_toggle_enabled(),
            _ => EDIT_ACTIVE.store(true, Ordering::Relaxed),
        },
        ButtonId::Cancel => exit_edit(),
    }
    true
}

// ── Per-slot persisted state ────────────────────────────────────────────────
//
// Each field is an array indexed by slot (0..N_ALARMS).  Slot 0 mirrors the
// previous single-alarm state and uses the same kv keys, so existing badges
// keep their settings across the upgrade.

static ALARM_HOUR: [AtomicU8; N_ALARMS] = [const { AtomicU8::new(7) }; N_ALARMS];
static ALARM_MINUTE: [AtomicU8; N_ALARMS] = [const { AtomicU8::new(0) }; N_ALARMS];
static ALARM_ENABLED: [AtomicBool; N_ALARMS] = [const { AtomicBool::new(false) }; N_ALARMS];
/// Day-of-week mask: bit 0 = Mon .. bit 6 = Sun. Default = every day.
static ALARM_DAYS: [AtomicU8; N_ALARMS] = [const { AtomicU8::new(0b0111_1111) }; N_ALARMS];
/// Index into [`crate::fw::buzzer::MELODIES`] used as the alarm ringtone.
/// Default: the dedicated `ALARM` beep-beep pattern.
static ALARM_MELODY: [AtomicU8; N_ALARMS] =
    [const { AtomicU8::new(crate::ALARM_INDEX as u8) }; N_ALARMS];
/// Optional one-shot date.  When `year` is non-zero, the slot fires only on
/// the exact matching `year-month-day` (and then self-disables) — used for
/// calendar-event alarms.  `year == 0` means recurring per the day mask.
static ALARM_YEAR: [AtomicU16; N_ALARMS] = [const { AtomicU16::new(0) }; N_ALARMS];
static ALARM_MONTH: [AtomicU8; N_ALARMS] = [const { AtomicU8::new(0) }; N_ALARMS];
static ALARM_DAY: [AtomicU8; N_ALARMS] = [const { AtomicU8::new(0) }; N_ALARMS];
/// Event end time (hour, minute) per slot.  Used by the Calendar
/// day-view to render events as duration blocks.  When `DTEND` is
/// missing in the source ICS the importer mirrors the start time
/// (zero-duration event → renders as a thin marker).  Multi-day
/// events are clamped to 23:59 of the start day at import time so
/// the day-view doesn't have to handle midnight crossings.  These
/// fields are not consulted by `check_and_fire_alarm`; the alarm
/// fires at the start time only.
static ALARM_END_HOUR: [AtomicU8; N_ALARMS] = [const { AtomicU8::new(0) }; N_ALARMS];
static ALARM_END_MINUTE: [AtomicU8; N_ALARMS] = [const { AtomicU8::new(0) }; N_ALARMS];
/// Event SUMMARY (calendar title) per slot, NUL-padded ASCII.  Stored as
/// per-byte atomics to match the rest of the alarm state — no
/// synchronisation primitive needed and the byte-by-byte loads are
/// negligible compared to a screen redraw.  Empty for slot 0 (the
/// manual alarm has no title) and overwritten at boot from `ALARMS.ICS`.
static ALARM_SUMMARY: [[AtomicU8; SUMMARY_LEN]; N_ALARMS] =
    [const { [const { AtomicU8::new(0) }; SUMMARY_LEN] }; N_ALARMS];

/// Curated tone choices: (display name, melody index).  Shared between
/// the alarm-tone stepper (Settings → Alarm → Tone) and the per-event
/// notification-sound steppers in `fw::mesh::sounds` — both modules use
/// the same set of player-pickable songs.  Indices reference
/// [`crate::fw::buzzer::MELODIES`] via the named constants in `crate`
/// (player songs) and `crate::fw::buzzer` (system-only sounds like
/// `ALARM`).
///
/// Bare names — callers that want a "Tone: " prefix prepend it
/// themselves.
pub const TONES: &[(&str, u8)] = &[
    ("Beep", crate::ALARM_INDEX as u8),
    ("Imp. March", crate::SONG_IMPERIAL_MARCH_INDEX),
    ("Rickroll", crate::SONG_RICKROLL_INDEX),
    ("Pink Pant.", crate::SONG_PINK_PANTHER_INDEX),
    ("Sandstorm", crate::SONG_SANDSTORM_INDEX),
    ("Startup", crate::SONG_STARTUP_INDEX),
    ("Trololo", crate::SONG_TROLOLO_INDEX),
    ("Daisy Bell", crate::SONG_DAISY_BELL_INDEX),
    ("Nokia", crate::SONG_NOKIA_INDEX),
    ("Samsung", crate::SONG_OVER_THE_HORIZON_INDEX),
];

#[inline]
fn s(slot: usize) -> usize {
    slot.min(N_ALARMS - 1)
}

// ── Slot-aware accessors ────────────────────────────────────────────────────

pub fn alarm_hour_n(slot: usize) -> u8 {
    ALARM_HOUR[s(slot)].load(Ordering::Relaxed)
}
pub fn alarm_minute_n(slot: usize) -> u8 {
    ALARM_MINUTE[s(slot)].load(Ordering::Relaxed)
}
pub fn alarm_enabled_n(slot: usize) -> bool {
    ALARM_ENABLED[s(slot)].load(Ordering::Relaxed)
}
pub fn alarm_days_n(slot: usize) -> u8 {
    ALARM_DAYS[s(slot)].load(Ordering::Relaxed) & 0x7F
}
pub fn alarm_melody_n(slot: usize) -> u8 {
    ALARM_MELODY[s(slot)].load(Ordering::Relaxed)
}
pub fn alarm_year_n(slot: usize) -> u16 {
    ALARM_YEAR[s(slot)].load(Ordering::Relaxed)
}
pub fn alarm_month_n(slot: usize) -> u8 {
    ALARM_MONTH[s(slot)].load(Ordering::Relaxed)
}
pub fn alarm_day_n(slot: usize) -> u8 {
    ALARM_DAY[s(slot)].load(Ordering::Relaxed)
}
pub fn alarm_end_hour_n(slot: usize) -> u8 {
    ALARM_END_HOUR[s(slot)].load(Ordering::Relaxed)
}
pub fn alarm_end_minute_n(slot: usize) -> u8 {
    ALARM_END_MINUTE[s(slot)].load(Ordering::Relaxed)
}

/// Returns the slot's SUMMARY as a heapless string.  Empty if no
/// summary was set (e.g. slot 0, or pre-import).
pub fn alarm_summary_n(slot: usize) -> heapless::String<SUMMARY_LEN> {
    let i = s(slot);
    let mut out: heapless::String<SUMMARY_LEN> = heapless::String::new();
    for byte_atomic in ALARM_SUMMARY[i].iter() {
        let b = byte_atomic.load(Ordering::Relaxed);
        if b == 0 {
            break;
        }
        let _ = out.push(b as char);
    }
    out
}

/// `day` is 0 = Mon .. 6 = Sun.
pub fn alarm_day_enabled_n(slot: usize, day: u8) -> bool {
    day < 7 && (alarm_days_n(slot) >> day) & 1 != 0
}

/// Returns `true` if `slot` is a one-shot calendar alarm (year != 0) bound to
/// a specific date, rather than a recurring weekly alarm.
pub fn alarm_is_one_shot_n(slot: usize) -> bool {
    alarm_year_n(slot) != 0
}

/// Set or clear a slot's one-shot date.  Pass `(0, 0, 0)` to make the slot
/// recurring (governed by its day mask) again.
pub fn set_alarm_date_n(slot: usize, year: u16, month: u8, day: u8) {
    let i = s(slot);
    ALARM_YEAR[i].store(year, Ordering::Relaxed);
    ALARM_MONTH[i].store(month, Ordering::Relaxed);
    ALARM_DAY[i].store(day, Ordering::Relaxed);
    super::signal_settings_dirty();
}

pub fn set_alarm_time_n(slot: usize, hour: u8, minute: u8) {
    let i = s(slot);
    ALARM_HOUR[i].store(hour.min(23), Ordering::Relaxed);
    ALARM_MINUTE[i].store(minute.min(59), Ordering::Relaxed);
    super::signal_settings_dirty();
}

/// Set the slot's event end time.  Used by the ICS importer to record
/// the `DTEND` of each event so the day-view can render duration
/// blocks.  Defaults to the start time when `DTEND` is missing or
/// degenerate (zero-duration event renders as a thin marker).
pub fn set_alarm_end_time_n(slot: usize, hour: u8, minute: u8) {
    let i = s(slot);
    ALARM_END_HOUR[i].store(hour.min(23), Ordering::Relaxed);
    ALARM_END_MINUTE[i].store(minute.min(59), Ordering::Relaxed);
}

pub fn set_alarm_enabled_n(slot: usize, enabled: bool) {
    ALARM_ENABLED[s(slot)].store(enabled, Ordering::Relaxed);
    super::signal_settings_dirty();
}

/// Set the slot's SUMMARY (event title) from a NUL-padded byte buffer.
pub fn set_alarm_summary_n(slot: usize, src: &[u8; SUMMARY_LEN]) {
    let i = s(slot);
    for (j, b) in src.iter().enumerate() {
        ALARM_SUMMARY[i][j].store(*b, Ordering::Relaxed);
    }
}

/// Find the lowest empty event slot index (>= 1) suitable for a new
/// event.  Returns None if all event slots (1..N_ALARMS) are populated.
pub fn first_empty_event_slot() -> Option<usize> {
    (1..N_ALARMS).find(|&slot| !alarm_enabled_n(slot))
}

/// Add an event scheduled `minutes_ahead` minutes from the current wall
/// clock, with the given summary.  Picks the first empty event slot.
/// Returns the firing `(hour, minute)` on success, or `None` if the
/// wall clock isn't synced or all event slots are full.
#[cfg(feature = "embassy-base")]
pub fn add_quick_event(minutes_ahead: u16, summary: &[u8]) -> Option<(u8, u8)> {
    let c = clock::wall_clock()?;
    let slot = first_empty_event_slot()?;

    // Roll over hour/day boundaries via plain integer math, then ask
    // fasttime to handle the calendar arithmetic if we crossed midnight.
    let total_mins = c.hour as u32 * 60 + c.minute as u32 + minutes_ahead as u32;
    let day_offset = (total_mins / (24 * 60)) as i64;
    let mins_in_day = total_mins % (24 * 60);
    let target_hour = (mins_in_day / 60) as u8;
    let target_min = (mins_in_day % 60) as u8;

    let (year, month, day) = if day_offset == 0 {
        (c.year, c.month, c.day)
    } else {
        let d = fasttime::Date::from_ymd(c.year as i32, c.month, c.day).ok()?;
        let d2 = d.add_days(day_offset).ok()?;
        (d2.year as u16, d2.month, d2.day)
    };

    set_alarm_date_n(slot, year, month, day);
    set_alarm_time_n(slot, target_hour, target_min);
    let mut buf = [0u8; SUMMARY_LEN];
    let mut i = 0usize;
    for &b in summary {
        if i >= SUMMARY_LEN {
            break;
        }
        if (0x20..=0x7e).contains(&b) {
            buf[i] = b;
            i += 1;
        }
    }
    set_alarm_summary_n(slot, &buf);
    set_alarm_enabled_n(slot, true);
    Some((target_hour, target_min))
}

/// Clear all imported alarms — disable + zero-date slots 1..N_ALARMS.  Slot 0
/// (the user's manual alarm) is left alone.  Used by the Events menu to
/// undo an `ALARMS.ICS` import without rebooting; the next boot would
/// overwrite slots 1..7 again from the file anyway, so this is mostly for
/// "I changed my mind, take them off the Clock face *now*" flows.
pub fn clear_imported_alarms() {
    for slot in 1..N_ALARMS {
        ALARM_ENABLED[slot].store(false, Ordering::Relaxed);
        ALARM_YEAR[slot].store(0, Ordering::Relaxed);
        ALARM_MONTH[slot].store(0, Ordering::Relaxed);
        ALARM_DAY[slot].store(0, Ordering::Relaxed);
        ALARM_END_HOUR[slot].store(0, Ordering::Relaxed);
        ALARM_END_MINUTE[slot].store(0, Ordering::Relaxed);
        for byte_atomic in ALARM_SUMMARY[slot].iter() {
            byte_atomic.store(0, Ordering::Relaxed);
        }
    }
    super::signal_settings_dirty();
}

// ── Slot-0 (primary) accessors — backward-compatible thin wrappers ──────────

pub fn alarm_hour() -> u8 {
    alarm_hour_n(0)
}
pub fn alarm_minute() -> u8 {
    alarm_minute_n(0)
}
pub fn alarm_enabled() -> bool {
    alarm_enabled_n(0)
}
pub fn alarm_days() -> u8 {
    alarm_days_n(0)
}
pub fn alarm_melody() -> u8 {
    alarm_melody_n(0)
}
pub fn alarm_day_enabled(day: u8) -> bool {
    alarm_day_enabled_n(0, day)
}

// ── Slot-0 mutators (used by the existing menu/edit UI) ─────────────────────

pub fn alarm_inc_hour() {
    let h = ALARM_HOUR[0].load(Ordering::Relaxed);
    ALARM_HOUR[0].store((h + 1) % 24, Ordering::Relaxed);
    super::signal_settings_dirty();
}
pub fn alarm_dec_hour() {
    let h = ALARM_HOUR[0].load(Ordering::Relaxed);
    ALARM_HOUR[0].store(if h == 0 { 23 } else { h - 1 }, Ordering::Relaxed);
    super::signal_settings_dirty();
}
pub fn alarm_inc_minute() {
    let m = ALARM_MINUTE[0].load(Ordering::Relaxed);
    ALARM_MINUTE[0].store((m + 1) % 60, Ordering::Relaxed);
    super::signal_settings_dirty();
}
pub fn alarm_dec_minute() {
    let m = ALARM_MINUTE[0].load(Ordering::Relaxed);
    ALARM_MINUTE[0].store(if m == 0 { 59 } else { m - 1 }, Ordering::Relaxed);
    super::signal_settings_dirty();
}
pub fn alarm_toggle_enabled() {
    let v = ALARM_ENABLED[0].load(Ordering::Relaxed);
    ALARM_ENABLED[0].store(!v, Ordering::Relaxed);
    super::signal_settings_dirty();
}

pub fn alarm_toggle_day(day: u8) {
    if day >= 7 {
        return;
    }
    let v = ALARM_DAYS[0].load(Ordering::Relaxed);
    ALARM_DAYS[0].store((v ^ (1 << day)) & 0x7F, Ordering::Relaxed);
    super::signal_settings_dirty();
}

/// Cycle the day mask through preset modes:
/// Daily ↔ Weekdays ↔ Weekends ↔ None ↔ Daily.  Used by the on-screen
/// alarm-edit Days field.  A "Custom" mask (anything else) jumps to
/// Daily on either direction.
pub fn alarm_step_days_preset(forward: bool) {
    let cur = ALARM_DAYS[0].load(Ordering::Relaxed) & 0x7F;
    let next: u8 = match (cur, forward) {
        (0x7F, true) => 0x1F,
        (0x1F, true) => 0x60,
        (0x60, true) => 0x00,
        (0x00, true) => 0x7F,
        (0x7F, false) => 0x00,
        (0x00, false) => 0x60,
        (0x60, false) => 0x1F,
        (0x1F, false) => 0x7F,
        _ => 0x7F,
    };
    ALARM_DAYS[0].store(next, Ordering::Relaxed);
    super::signal_settings_dirty();
}

pub fn alarm_days_label() -> &'static str {
    match alarm_days() {
        0x7F => "Days: Daily",
        0x1F => "Days: Weekdays",
        0x60 => "Days: Weekends",
        0x00 => "Days: None",
        _ => "Days: Custom",
    }
}

pub fn alarm_enabled_label() -> &'static str {
    if alarm_enabled() {
        "Enabled: On"
    } else {
        "Enabled: Off"
    }
}

fn alarm_tone_position() -> usize {
    let m = alarm_melody();
    TONES.iter().position(|(_, idx)| *idx == m).unwrap_or(0)
}

/// `"Tone: <name>"` — built fresh on each call.  Used by both the
/// alarm-edit screen's row renderer and the Settings menu's
/// `ValueStepper` format callback.
pub fn alarm_tone_label() -> heapless::String<24> {
    let mut s = heapless::String::new();
    use core::fmt::Write;
    let _ = write!(s, "Tone: {}", TONES[alarm_tone_position()].0);
    s
}

fn step_melody(delta: i32) {
    let pos = alarm_tone_position() as i32;
    let len = TONES.len() as i32;
    let next = pos.rem_euclid(len).wrapping_add(delta).rem_euclid(len) as usize;
    let idx = TONES[next].1;
    ALARM_MELODY[0].store(idx, Ordering::Relaxed);
    super::signal_settings_dirty();
    #[cfg(feature = "embassy-base")]
    crate::fw::buzzer::play(idx as usize);
}

pub fn alarm_inc_melody() {
    step_melody(1);
}

pub fn alarm_dec_melody() {
    step_melody(-1);
}

// ── KV load / persist (called by the watch coordinator) ─────────────────────

#[cfg(feature = "embassy-base")]
pub(super) async fn load_settings_from_kv(ns: &crate::fw::kv::KvNamespace) {
    let mut b = [0u8; 1];
    if let Ok(1) = ns.get("alarm_h", &mut b).await
        && b[0] < 24
    {
        ALARM_HOUR[0].store(b[0], Ordering::Relaxed);
    }
    if let Ok(1) = ns.get("alarm_m", &mut b).await
        && b[0] < 60
    {
        ALARM_MINUTE[0].store(b[0], Ordering::Relaxed);
    }
    if let Ok(1) = ns.get("alarm_on", &mut b).await {
        ALARM_ENABLED[0].store(b[0] != 0, Ordering::Relaxed);
    }
    if let Ok(1) = ns.get("alarm_days", &mut b).await {
        ALARM_DAYS[0].store(b[0] & 0x7F, Ordering::Relaxed);
    }
    if let Ok(1) = ns.get("alarm_mel", &mut b).await
        && TONES.iter().any(|(_, idx)| *idx == b[0])
    {
        ALARM_MELODY[0].store(b[0], Ordering::Relaxed);
    }
}

#[cfg(feature = "embassy-base")]
pub(super) async fn persist(ns: &crate::fw::kv::KvNamespace) {
    let _ = ns.set("alarm_h", &[alarm_hour()], true).await;
    let _ = ns.set("alarm_m", &[alarm_minute()], true).await;
    let _ = ns.set("alarm_on", &[alarm_enabled() as u8], true).await;
    let _ = ns.set("alarm_days", &[alarm_days()], true).await;
    let _ = ns.set("alarm_mel", &[alarm_melody()], true).await;
}

// ── Ringing — fire / dismiss / repeat task ─────────────────────────────────

/// True while the alarm melody is playing and the user hasn't yet dismissed
/// it. Cleared by [`dismiss_alarm_if_ringing`] or after a short timeout.
#[cfg(feature = "embassy-base")]
static ALARM_RINGING: AtomicBool = AtomicBool::new(false);

#[cfg(feature = "embassy-base")]
static ALARM_RING_SIGNAL: embassy_sync::signal::Signal<
    embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex,
    (),
> = embassy_sync::signal::Signal::new();

/// Walks all `N_ALARMS` slots; for each enabled slot, fires the buzzer if
/// the local time matches and either:
///  * the slot is a one-shot calendar alarm (`year != 0`) whose date is today,
///    or
///  * the slot is a recurring weekly alarm (`year == 0`) whose day mask covers
///    today.
///
/// One-shot slots auto-disable after firing so they don't ring again on the
/// next reboot if the badge is rebooted while still on the same calendar day.
///
/// Only the *first* matching slot fires per minute boundary — running two
/// melodies on top of each other would just sound bad.  Slot order is the
/// natural numeric one (slot 0 wins ties).
#[cfg(feature = "embassy-base")]
pub fn check_and_fire_alarm() {
    let Some(c) = clock::wall_clock() else {
        return;
    };
    for slot in 0..N_ALARMS {
        if !alarm_enabled_n(slot) {
            continue;
        }
        // Date- vs day-mask gate.
        if alarm_is_one_shot_n(slot) {
            if alarm_year_n(slot) != c.year
                || alarm_month_n(slot) != c.month
                || alarm_day_n(slot) != c.day
            {
                continue;
            }
        } else if !alarm_day_enabled_n(slot, c.weekday) {
            continue;
        }
        if c.hour == alarm_hour_n(slot) && c.minute == alarm_minute_n(slot) {
            ALARM_RINGING.store(true, Ordering::Relaxed);
            ALARM_RING_SIGNAL.signal(());
            // One-shot (imported calendar) slots have no per-event tone
            // UI — they inherit slot 0's melody so the user's chosen
            // Settings → Alarm → Tone applies to every alarm consistently.
            let melody = if alarm_is_one_shot_n(slot) {
                alarm_melody_n(0)
            } else {
                alarm_melody_n(slot)
            };
            crate::fw::buzzer::play(melody as usize);
            // One-shot alarms auto-disable after firing.
            if alarm_is_one_shot_n(slot) {
                ALARM_ENABLED[slot].store(false, Ordering::Relaxed);
                super::signal_settings_dirty();
            }
            return;
        }
    }
}

/// Returns `true` if there was an active alarm to silence; in that case the
/// buzzer is stopped and the ringing flag cleared. Called by the menu dispatch
/// before any other button handling.
#[cfg(feature = "embassy-base")]
pub fn dismiss_alarm_if_ringing() -> bool {
    if ALARM_RINGING.swap(false, Ordering::Relaxed) {
        crate::fw::buzzer::stop();
        true
    } else {
        false
    }
}

/// Re-plays the alarm melody up to `ALARM_REPEATS` times (every
/// `ALARM_REPEAT_INTERVAL_SECS`) unless the user dismisses it, then clears
/// the ringing flag so an un-dismissed alarm stops eating button presses.
///
/// The first play is triggered by `check_and_fire_alarm`; this task handles
/// the repeats and the final cleanup.  The repeat melody is whichever slot's
/// tone is currently set on slot 0 — close enough; chaining the
/// originating-slot index through the ring task is overkill for now.
#[cfg(feature = "embassy-base")]
#[embassy_executor::task]
pub async fn alarm_ring_timeout_task() {
    const ALARM_REPEATS: u8 = 4; // total plays = 1 initial + 4 repeats
    const ALARM_REPEAT_INTERVAL_SECS: u64 = 8;
    loop {
        ALARM_RING_SIGNAL.wait().await;
        for _ in 0..ALARM_REPEATS {
            embassy_time::Timer::after(embassy_time::Duration::from_secs(
                ALARM_REPEAT_INTERVAL_SECS,
            ))
            .await;
            if !ALARM_RINGING.load(Ordering::Relaxed) {
                break;
            }
            crate::fw::buzzer::play(alarm_melody() as usize);
        }
        ALARM_RINGING.store(false, Ordering::Relaxed);
    }
}

// ── Drawing ─────────────────────────────────────────────────────────────────

/// Returns `true` if any slot has an enabled alarm.
fn any_alarm_enabled() -> bool {
    (0..N_ALARMS).any(alarm_enabled_n)
}

/// Find the soonest enabled alarm whose firing is still in the future *today*.
/// Recurring alarms count if the day mask covers today; one-shot alarms count
/// if their date matches today.  Returns `(hour, minute)` or `None`.
fn next_alarm_today(c: &super::clock::Clock) -> Option<(u8, u8)> {
    let mut earliest: Option<(u8, u8)> = None;
    for slot in 0..N_ALARMS {
        if !alarm_enabled_n(slot) {
            continue;
        }
        let active_today = if alarm_is_one_shot_n(slot) {
            alarm_year_n(slot) == c.year
                && alarm_month_n(slot) == c.month
                && alarm_day_n(slot) == c.day
        } else {
            alarm_day_enabled_n(slot, c.weekday)
        };
        if !active_today {
            continue;
        }
        let h = alarm_hour_n(slot);
        let m = alarm_minute_n(slot);
        // Only future-today: strictly after the current minute.
        if h < c.hour || (h == c.hour && m <= c.minute) {
            continue;
        }
        let better = match earliest {
            None => true,
            Some((eh, em)) => h < eh || (h == eh && m < em),
        };
        if better {
            earliest = Some((h, m));
        }
    }
    earliest
}

/// Render a 13×13 red bell, centred at `(cx, cy)`, blitting the shared
/// `fw::emoji::ATLAS_BELL` glyph in red instead of the default black.
///
/// Previously this function drew the bell from primitive shapes (circle
/// dome + rectangles).  Now it shares the bitmap with in-message emoji
/// rendering so a 🔔 typed in a PM and the watch-face indicator look
/// identical — only the colour plane differs (red here vs black in text).
fn draw_bell<D>(display: &mut D, cx: i32, cy: i32) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    crate::fw::emoji::draw_emoji(
        display,
        crate::fw::emoji::ATLAS_BELL,
        Point::new(cx - 6, cy - 6),
        RED,
    )
}

/// Render a 13×13 red envelope, centred at `(cx, cy)`, blitting the
/// shared `fw::emoji::ATLAS_ENVELOPE` glyph in red.  Same atlas bitmap
/// the in-message ✉ / 📧 / 📨 / 📩 codepoints use.
#[cfg(feature = "mesh")]
fn draw_envelope<D>(display: &mut D, cx: i32, cy: i32) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    crate::fw::emoji::draw_emoji(
        display,
        crate::fw::emoji::ATLAS_ENVELOPE,
        Point::new(cx - 6, cy - 6),
        RED,
    )
}

/// Header indicator: an optional red envelope (when there are unread PMs,
/// `feature = "mesh"` only), an optional red bell (when any alarm is
/// enabled), and an optional `HH:MM` for the next alarm firing today.
///
/// Visibility:
///   * Nothing → nothing rendered.
///   * Unread PMs → envelope, optionally followed by `+N`.
///   * Alarm(s) enabled but none firing later today → bell.
///   * Alarm enabled with a future firing today → bell + `HH:MM`.
///
/// The envelope and bell sit on the red plane, which only refreshes on a
/// full tri-color update; the `+N` and `HH:MM` are black and update on
/// every redraw.
pub(super) fn draw_indicator<D>(display: &mut D) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let bell_cx = 56i32;
    let bell_cy = 8i32;
    let left = TextStyleBuilder::new()
        .baseline(Baseline::Middle)
        .alignment(Alignment::Left)
        .build();

    // Bell + optional HH:MM keep their existing positions.  Title text
    // sits left-aligned at x=4..~40, so anything we draw past the bell
    // (x=56) is in the free zone between the title and the battery icon
    // at x=128.
    let mut alarm_time_end_x: i32 = bell_cx + 10;
    if any_alarm_enabled() {
        draw_bell(display, bell_cx, bell_cy)?;
        if let Some(c) = clock::wall_clock()
            && let Some((h, m)) = next_alarm_today(&c)
        {
            let mut buf: heapless::String<8> = heapless::String::new();
            let _ = core::fmt::write(&mut buf, format_args!("{:02}:{:02}", h, m));
            Text::with_text_style(
                &buf,
                Point::new(alarm_time_end_x, bell_cy),
                MonoTextStyle::new(&FONT_6X10, BLACK),
                left,
            )
            .draw(display)?;
            alarm_time_end_x += 32; // 5 chars × ~6 px + small gap
        }
    }

    // PM envelope — only when mesh is built in and at least one incoming
    // PM is unread.  Drawn last so it lands right of the bell + alarm
    // time, well clear of the title text on the left.
    #[cfg(feature = "mesh")]
    {
        let unread = crate::fw::mesh::pm_inbox::unread_total();
        if unread > 0 {
            let env_cx = alarm_time_end_x + 7;
            draw_envelope(display, env_cx, bell_cy)?;
            // The envelope alone says "you've got one" — only annotate
            // when a count adds information (≥ 2).
            if unread >= 2 {
                let mut buf: heapless::String<8> = heapless::String::new();
                let _ = core::fmt::write(&mut buf, format_args!("+{}", unread));
                Text::with_text_style(
                    &buf,
                    Point::new(env_cx + 8, bell_cy),
                    MonoTextStyle::new(&FONT_6X10, BLACK),
                    left,
                )
                .draw(display)?;
            }
        }
    }
    #[cfg(not(feature = "mesh"))]
    let _ = alarm_time_end_x;
    Ok(())
}

/// Render the alarm-edit screen: HH:MM in 7-seg digits, three info rows,
/// and a black underline beneath the currently-selected field.
pub(super) fn draw_edit<D>(display: &mut D) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let h = alarm_hour();
    let m = alarm_minute();
    let active = is_edit_active();
    let field = current_field();

    clock::draw_digit(display, clock::HH_TENS_X, clock::DIGIT_Y, h / 10)?;
    clock::draw_digit(display, clock::HH_ONES_X, clock::DIGIT_Y, h % 10)?;
    clock::draw_colon(display)?;
    clock::draw_digit(display, clock::MM_TENS_X, clock::DIGIT_Y, m / 10)?;
    clock::draw_digit(display, clock::MM_ONES_X, clock::DIGIT_Y, m % 10)?;

    let centered = TextStyleBuilder::new()
        .baseline(Baseline::Middle)
        .alignment(Alignment::Center)
        .build();

    // Three info rows below the digits.  FONT_6X10 keeps them tight enough
    // that there's room for an underline / inverted bar beneath each without
    // overlapping the next row's text.
    const ROW_DAYS_Y: i32 = 108;
    const ROW_TONE_Y: i32 = 124;
    const ROW_ENABLED_Y: i32 = 140;
    const ROW_BAR_X: i32 = 13;
    const ROW_BAR_W: u32 = 126;
    const ROW_BAR_H: u32 = 13; // covers the 10 px text + 1 px above/below
    const DIGIT_UL_THIN: u32 = 3;
    const DIGIT_UL_THICK: u32 = 6;
    const ROW_UL_THICK: u32 = 2;

    // Helper closure: render one info row, with optional inverted background
    // when the user has drilled into this row.
    let mut draw_row = |label: &str, y: i32, this_field: EditField| -> Result<(), D::Error> {
        let drilled = active && field == this_field;
        if drilled {
            Rectangle::new(
                Point::new(ROW_BAR_X, y - (ROW_BAR_H as i32) / 2),
                Size::new(ROW_BAR_W, ROW_BAR_H),
            )
            .into_styled(PrimitiveStyle::with_fill(BLACK))
            .draw(display)?;
        }
        let fg = if drilled { WHITE } else { BLACK };
        Text::with_text_style(
            label,
            Point::new(76, y),
            MonoTextStyle::new(&FONT_6X10, fg),
            centered,
        )
        .draw(display)?;
        Ok(())
    };

    draw_row(alarm_days_label(), ROW_DAYS_Y, EditField::Days)?;
    draw_row(&alarm_tone_label(), ROW_TONE_Y, EditField::Tone)?;
    draw_row(alarm_enabled_label(), ROW_ENABLED_Y, EditField::Enabled)?;

    // Underline marks the selected row.  For text rows the inverted bar
    // already says "drilled in", so the underline is only drawn while in
    // row-nav (selected but not active).  For digit rows the underline
    // thickens when drilled in.
    let digit_ul_thick = if active {
        DIGIT_UL_THICK
    } else {
        DIGIT_UL_THIN
    };
    let underline = match field {
        EditField::Hour => Some((
            clock::HH_TENS_X,
            clock::DIGIT_Y + clock::DIGIT_H + 2,
            clock::PAIR_W as u32,
            digit_ul_thick,
        )),
        EditField::Minute => Some((
            clock::MM_TENS_X,
            clock::DIGIT_Y + clock::DIGIT_H + 2,
            clock::PAIR_W as u32,
            digit_ul_thick,
        )),
        EditField::Days if !active => Some((ROW_BAR_X, ROW_DAYS_Y + 7, ROW_BAR_W, ROW_UL_THICK)),
        EditField::Tone if !active => Some((ROW_BAR_X, ROW_TONE_Y + 7, ROW_BAR_W, ROW_UL_THICK)),
        // Enabled is a binary toggle — no drill-in, just keep the underline.
        EditField::Enabled => Some((ROW_BAR_X, ROW_ENABLED_Y + 7, ROW_BAR_W, ROW_UL_THICK)),
        _ => None,
    };
    if let Some((x, y, w, h)) = underline {
        Rectangle::new(Point::new(x, y), Size::new(w, h))
            .into_styled(PrimitiveStyle::with_fill(BLACK))
            .draw(display)?;
    }

    Ok(())
}
