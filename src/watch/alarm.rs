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

use core::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::mono_font::ascii::FONT_6X10;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{PrimitiveStyle, Rectangle};
use embedded_graphics::text::{Alignment, Baseline, Text, TextStyleBuilder};

use super::clock;
use crate::menu::ButtonId;
use crate::{BLACK, TriColor, WHITE};

/// Maximum number of independent alarm slots.  Slot 0 is the user-editable
/// "primary" alarm; slots 1..7 are reserved for things like calendar imports.
pub const N_ALARMS: usize = 8;

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
/// Default: 8 = the dedicated `ALARM` beep-beep pattern.
static ALARM_MELODY: [AtomicU8; N_ALARMS] = [const { AtomicU8::new(8) }; N_ALARMS];

/// Curated alarm-tone choices: (menu label, melody index).
/// Order is the cycle order in the Settings → Alarm → Tone stepper.
const ALARM_TONES: &[(&str, u8)] = &[
    ("Tone: Beep", 8),
    ("Tone: Imp. March", 2),
    ("Tone: Rickroll", 1),
    ("Tone: Pink Pant.", 4),
    ("Tone: Sandstorm", 3),
    ("Tone: Startup", 0),
    ("Tone: Trololo", 5),
    ("Tone: Daisy Bell", 9),
    ("Tone: Nokia", 10),
    ("Tone: Samsung", 11),
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

/// `day` is 0 = Mon .. 6 = Sun.
pub fn alarm_day_enabled_n(slot: usize, day: u8) -> bool {
    day < 7 && (alarm_days_n(slot) >> day) & 1 != 0
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
    ALARM_TONES
        .iter()
        .position(|(_, idx)| *idx == m)
        .unwrap_or(0)
}

pub fn alarm_tone_label() -> &'static str {
    ALARM_TONES[alarm_tone_position()].0
}

pub fn alarm_inc_melody() {
    let pos = alarm_tone_position();
    let next = (pos + 1) % ALARM_TONES.len();
    let idx = ALARM_TONES[next].1;
    ALARM_MELODY[0].store(idx, Ordering::Relaxed);
    super::signal_settings_dirty();
    #[cfg(feature = "embassy-base")]
    crate::fw::buzzer::play(idx as usize);
}

pub fn alarm_dec_melody() {
    let pos = alarm_tone_position();
    let prev = if pos == 0 {
        ALARM_TONES.len() - 1
    } else {
        pos - 1
    };
    let idx = ALARM_TONES[prev].1;
    ALARM_MELODY[0].store(idx, Ordering::Relaxed);
    super::signal_settings_dirty();
    #[cfg(feature = "embassy-base")]
    crate::fw::buzzer::play(idx as usize);
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
        && ALARM_TONES.iter().any(|(_, idx)| *idx == b[0])
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

/// Walks all `N_ALARMS` slots; for each enabled slot whose day mask covers
/// today, if the local time matches `HH:MM` the buzzer is fired with that
/// slot's melody.  The alarm-ring task then repeats the melody until the
/// user dismisses or the repeat budget is exhausted.
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
        if !alarm_day_enabled_n(slot, c.weekday) {
            continue;
        }
        if c.hour == alarm_hour_n(slot) && c.minute == alarm_minute_n(slot) {
            ALARM_RINGING.store(true, Ordering::Relaxed);
            ALARM_RING_SIGNAL.signal(());
            crate::fw::buzzer::play(alarm_melody_n(slot) as usize);
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

/// Black box with white text in the header showing `ALM HH:MM` when an alarm
/// is armed. Uses pure B&W so it survives the fast minute-tick refresh.
pub(super) fn draw_indicator<D>(display: &mut D) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    if !alarm_enabled() {
        return Ok(());
    }
    let mut buf: heapless::String<12> = heapless::String::new();
    let _ = core::fmt::write(
        &mut buf,
        format_args!("ALM {:02}:{:02}", alarm_hour(), alarm_minute()),
    );

    let box_x = 44i32;
    let box_y = 1i32;
    let box_w = 62u32;
    let box_h = 14u32;
    Rectangle::new(Point::new(box_x, box_y), Size::new(box_w, box_h))
        .into_styled(PrimitiveStyle::with_fill(BLACK))
        .draw(display)?;
    let centered = TextStyleBuilder::new()
        .baseline(Baseline::Middle)
        .alignment(Alignment::Center)
        .build();
    Text::with_text_style(
        &buf,
        Point::new(box_x + box_w as i32 / 2, box_y + box_h as i32 / 2),
        MonoTextStyle::new(&FONT_6X10, WHITE),
        centered,
    )
    .draw(display)?;
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
    draw_row(alarm_tone_label(), ROW_TONE_Y, EditField::Tone)?;
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
