//! Watch app — switchable Casio-style digital face and analog face.
//!
//! Normal mode buttons:
//!   * Up/Down       — toggle digital ↔ analog face
//!   * Fire/Execute  — enter alarm-edit mode
//!
//! Alarm-edit mode buttons:
//!   * Left/Right    — cycle selected field
//!                     (Hour → Minute → Days → Tone → Enabled → Hour)
//!   * Up/Down       — increment / decrement the selected field
//!                     (steppers, day-mask presets, tone preview, toggle)
//!   * Fire/Cancel   — exit edit mode (changes are live, no save needed)
//!
//! The current weekday is highlighted in red (white-on-red) for visual punch.
//! Note: the red plane only updates on a full tri-color refresh; on the fast
//! B&W minute-tick refresh the red pixels won't redraw, so the current-day
//! highlight may look stale until the next full refresh.

use core::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use embedded_graphics::{
    mono_font::{MonoTextStyle, ascii::FONT_6X10, ascii::FONT_7X13_BOLD},
    prelude::*,
    primitives::{Circle, Line, PrimitiveStyle, PrimitiveStyleBuilder, Rectangle, Triangle},
    text::{Alignment, Baseline, Text, TextStyleBuilder},
};

use crate::{BLACK, RED, TriColor, WHITE, draw_frame, menu::ButtonId};

// ── Face selection ───────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum WatchFace {
    Digital = 0,
    Analog = 1,
}

static WATCH_FACE: AtomicU8 = AtomicU8::new(WatchFace::Digital as u8);

fn current_face() -> WatchFace {
    match WATCH_FACE.load(Ordering::Relaxed) {
        0 => WatchFace::Digital,
        _ => WatchFace::Analog,
    }
}

fn toggle_face() {
    let next = match current_face() {
        WatchFace::Digital => WatchFace::Analog,
        WatchFace::Analog => WatchFace::Digital,
    };
    WATCH_FACE.store(next as u8, Ordering::Relaxed);
    signal_settings_dirty();
}

// ── Edit-mode state ─────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
enum WatchMode {
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

fn current_mode() -> WatchMode {
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

fn enter_edit() {
    WATCH_MODE.store(WatchMode::AlarmEdit as u8, Ordering::Relaxed);
    EDIT_FIELD.store(EditField::Hour as u8, Ordering::Relaxed);
}

fn exit_edit() {
    WATCH_MODE.store(WatchMode::Normal as u8, Ordering::Relaxed);
}

fn cycle_field(forward: bool) {
    let next = match (current_field(), forward) {
        (EditField::Hour, true) => EditField::Minute,
        (EditField::Minute, true) => EditField::Days,
        (EditField::Days, true) => EditField::Tone,
        (EditField::Tone, true) => EditField::Enabled,
        (EditField::Enabled, true) => EditField::Hour,
        (EditField::Hour, false) => EditField::Enabled,
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

/// Returns `true` if the button was consumed by the watch screen.
pub fn dispatch(btn: ButtonId) -> bool {
    match current_mode() {
        WatchMode::AlarmEdit => {
            match btn {
                ButtonId::Up => step_current_field(true),
                ButtonId::Down => step_current_field(false),
                ButtonId::Left => cycle_field(false),
                ButtonId::Right => cycle_field(true),
                ButtonId::Fire | ButtonId::Execute | ButtonId::Cancel => exit_edit(),
            }
            true
        }
        WatchMode::Normal => match btn {
            ButtonId::Up | ButtonId::Down => {
                toggle_face();
                true
            }
            ButtonId::Fire | ButtonId::Execute => {
                enter_edit();
                true
            }
            _ => false,
        },
    }
}

// ── Persisted state (alarm + face choice) ───────────────────────────────────
//
// Saved to the `kv` namespace `"watch"`. Loaded once at boot
// (`load_settings_from_kv`) and re-saved by `settings_persister_task` whenever
// a setter signals `SETTINGS_DIRTY_SIGNAL`.
static ALARM_HOUR: AtomicU8 = AtomicU8::new(7);
static ALARM_MINUTE: AtomicU8 = AtomicU8::new(0);
static ALARM_ENABLED: AtomicBool = AtomicBool::new(false);
/// Day-of-week mask: bit 0 = Mon .. bit 6 = Sun. Default = every day.
static ALARM_DAYS: AtomicU8 = AtomicU8::new(0b0111_1111);
/// Index into [`crate::fw::buzzer::MELODIES`] used as the alarm ringtone.
/// Default: 8 = the dedicated `ALARM` beep-beep pattern.
static ALARM_MELODY: AtomicU8 = AtomicU8::new(8);

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
];

#[cfg(feature = "embassy-base")]
pub static SETTINGS_DIRTY_SIGNAL: embassy_sync::signal::Signal<
    embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex,
    (),
> = embassy_sync::signal::Signal::new();

#[cfg(feature = "embassy-base")]
fn signal_settings_dirty() {
    SETTINGS_DIRTY_SIGNAL.signal(());
}

#[cfg(not(feature = "embassy-base"))]
fn signal_settings_dirty() {}

pub fn alarm_hour() -> u8 {
    ALARM_HOUR.load(Ordering::Relaxed)
}
pub fn alarm_minute() -> u8 {
    ALARM_MINUTE.load(Ordering::Relaxed)
}
pub fn alarm_enabled() -> bool {
    ALARM_ENABLED.load(Ordering::Relaxed)
}

pub fn alarm_inc_hour() {
    let h = ALARM_HOUR.load(Ordering::Relaxed);
    ALARM_HOUR.store((h + 1) % 24, Ordering::Relaxed);
    signal_settings_dirty();
}
pub fn alarm_dec_hour() {
    let h = ALARM_HOUR.load(Ordering::Relaxed);
    ALARM_HOUR.store(if h == 0 { 23 } else { h - 1 }, Ordering::Relaxed);
    signal_settings_dirty();
}
pub fn alarm_inc_minute() {
    let m = ALARM_MINUTE.load(Ordering::Relaxed);
    ALARM_MINUTE.store((m + 1) % 60, Ordering::Relaxed);
    signal_settings_dirty();
}
pub fn alarm_dec_minute() {
    let m = ALARM_MINUTE.load(Ordering::Relaxed);
    ALARM_MINUTE.store(if m == 0 { 59 } else { m - 1 }, Ordering::Relaxed);
    signal_settings_dirty();
}
pub fn alarm_toggle_enabled() {
    let v = ALARM_ENABLED.load(Ordering::Relaxed);
    ALARM_ENABLED.store(!v, Ordering::Relaxed);
    signal_settings_dirty();
}

pub fn alarm_days() -> u8 {
    ALARM_DAYS.load(Ordering::Relaxed) & 0x7F
}

/// `day` is 0 = Mon .. 6 = Sun.
pub fn alarm_day_enabled(day: u8) -> bool {
    day < 7 && (alarm_days() >> day) & 1 != 0
}

pub fn alarm_toggle_day(day: u8) {
    if day >= 7 {
        return;
    }
    let v = ALARM_DAYS.load(Ordering::Relaxed);
    ALARM_DAYS.store((v ^ (1 << day)) & 0x7F, Ordering::Relaxed);
    signal_settings_dirty();
}

/// Cycle the day mask through preset modes:
/// Daily ↔ Weekdays ↔ Weekends ↔ None ↔ Daily.  Used by the on-screen
/// alarm-edit Days field.  A "Custom" mask (anything else) jumps to
/// Daily on either direction.
pub fn alarm_step_days_preset(forward: bool) {
    let cur = ALARM_DAYS.load(Ordering::Relaxed) & 0x7F;
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
    ALARM_DAYS.store(next, Ordering::Relaxed);
    signal_settings_dirty();
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

pub fn alarm_melody() -> u8 {
    ALARM_MELODY.load(Ordering::Relaxed)
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
    ALARM_MELODY.store(idx, Ordering::Relaxed);
    signal_settings_dirty();
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
    ALARM_MELODY.store(idx, Ordering::Relaxed);
    signal_settings_dirty();
    #[cfg(feature = "embassy-base")]
    crate::fw::buzzer::play(idx as usize);
}

/// Load persisted watch settings (alarm + face choice) from the `"watch"` kv
/// namespace. Call once at boot, after `kv::init()`. Silently leaves defaults
/// in place if a key is missing or invalid.
#[cfg(feature = "embassy-base")]
pub async fn load_settings_from_kv() {
    let ns = crate::fw::kv::namespace("watch");
    let mut b = [0u8; 1];
    if let Ok(1) = ns.get("alarm_h", &mut b).await
        && b[0] < 24
    {
        ALARM_HOUR.store(b[0], Ordering::Relaxed);
    }
    if let Ok(1) = ns.get("alarm_m", &mut b).await
        && b[0] < 60
    {
        ALARM_MINUTE.store(b[0], Ordering::Relaxed);
    }
    if let Ok(1) = ns.get("alarm_on", &mut b).await {
        ALARM_ENABLED.store(b[0] != 0, Ordering::Relaxed);
    }
    if let Ok(1) = ns.get("alarm_days", &mut b).await {
        ALARM_DAYS.store(b[0] & 0x7F, Ordering::Relaxed);
    }
    if let Ok(1) = ns.get("alarm_mel", &mut b).await
        && ALARM_TONES.iter().any(|(_, idx)| *idx == b[0])
    {
        ALARM_MELODY.store(b[0], Ordering::Relaxed);
    }
    if let Ok(1) = ns.get("face", &mut b).await
        && b[0] <= 1
    {
        WATCH_FACE.store(b[0], Ordering::Relaxed);
    }
}

/// Embassy task that persists watch settings (alarm + face) whenever a setter
/// signals `SETTINGS_DIRTY_SIGNAL`.
#[cfg(feature = "embassy-base")]
#[embassy_executor::task]
pub async fn settings_persister_task() {
    let ns = crate::fw::kv::namespace("watch");
    loop {
        SETTINGS_DIRTY_SIGNAL.wait().await;
        let _ = ns.set("alarm_h", &[alarm_hour()], true).await;
        let _ = ns.set("alarm_m", &[alarm_minute()], true).await;
        let _ = ns.set("alarm_on", &[alarm_enabled() as u8], true).await;
        let _ = ns.set("alarm_days", &[alarm_days()], true).await;
        let _ = ns.set("alarm_mel", &[alarm_melody()], true).await;
        let _ = ns
            .set("face", &[WATCH_FACE.load(Ordering::Relaxed)], true)
            .await;
    }
}

/// True while the alarm melody is playing and the user hasn't yet dismissed
/// it. Cleared by [`dismiss_alarm_if_ringing`] or after a short timeout.
#[cfg(feature = "embassy-base")]
static ALARM_RINGING: AtomicBool = AtomicBool::new(false);

#[cfg(feature = "embassy-base")]
static ALARM_RING_SIGNAL: embassy_sync::signal::Signal<
    embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex,
    (),
> = embassy_sync::signal::Signal::new();

/// Called from the minute-tick task: if the alarm is enabled, today is in the
/// day mask, and the local time matches `HH:MM`, fire the buzzer. The
/// alarm-ring task then repeats the melody up to a few times unless dismissed.
#[cfg(feature = "embassy-base")]
pub fn check_and_fire_alarm() {
    if !alarm_enabled() {
        return;
    }
    let Some(clock) = wall_clock() else {
        return;
    };
    if !alarm_day_enabled(clock.weekday) {
        return;
    }
    if clock.hour == alarm_hour() && clock.minute == alarm_minute() {
        ALARM_RINGING.store(true, Ordering::Relaxed);
        ALARM_RING_SIGNAL.signal(());
        crate::fw::buzzer::play(alarm_melody() as usize);
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
/// the repeats and the final cleanup.
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

// ── 7-segment digit geometry ─────────────────────────────────────────────────
//
// Each segment is a hex (lozenge) so adjacent segments meet at 45° miters,
// like a real LCD. Lengths are chosen so:
//   DIGIT_H = 3 * STROKE + 2 * VERT_LEN
// which keeps the middle segment exactly centred and the upper/lower halves
// symmetric.
const DIGIT_W: i32 = 30;
const STROKE: i32 = 5; // segment thickness; must be odd so the tip apex sits on a single pixel
const HALF: i32 = STROKE / 2; // 2
const VERT_LEN: i32 = 25; // length of one vertical segment (top or bottom half)
const DIGIT_H: i32 = 3 * STROKE + 2 * VERT_LEN; // 65
// Horizontal segments are inset from the digit edges, like a real Casio:
// the vertical segs are flush with the side, the horizontals sit between them.
const HORIZ_LEN: i32 = 24;
const HORIZ_INSET: i32 = (DIGIT_W - HORIZ_LEN) / 2;

// ── Time-row layout ──────────────────────────────────────────────────────────
const DIGIT_Y: i32 = 30;
const DIGIT_PITCH: i32 = DIGIT_W + 4; // gap between digits within a pair
const PAIR_W: i32 = DIGIT_PITCH + DIGIT_W; // 64 — width of "HH" or "MM"
const COLON_W: i32 = 6;
const COLON_GAP: i32 = 4;
const TIME_W: i32 = 2 * PAIR_W + 2 * COLON_GAP + COLON_W; // 142
const TIME_X: i32 = (152 - TIME_W) / 2; // 5
const HH_TENS_X: i32 = TIME_X;
const HH_ONES_X: i32 = TIME_X + DIGIT_PITCH;
const COLON_X: i32 = TIME_X + PAIR_W + COLON_GAP;
const MM_TENS_X: i32 = COLON_X + COLON_W + COLON_GAP;
const MM_ONES_X: i32 = MM_TENS_X + DIGIT_PITCH;

// Colon dots aligned with the inner blank rows between top/middle and middle/bottom.
const COLON_TOP_Y: i32 = DIGIT_Y + STROKE + VERT_LEN / 2 - COLON_W / 2;
const COLON_BOT_Y: i32 = DIGIT_Y + 2 * STROKE + VERT_LEN + VERT_LEN / 2 - COLON_W / 2;

// ── Analog face geometry ─────────────────────────────────────────────────────
const ANALOG_CX: i32 = 76;
const ANALOG_CY: i32 = 65;
const ANALOG_R: i32 = 44;
const ANALOG_TICK_HOUR: i32 = 4;
const ANALOG_TICK_CARDINAL: i32 = 7;
const HOUR_HAND_LEN: i32 = 25;
const MINUTE_HAND_LEN: i32 = 38;
const HOUR_HAND_W: u32 = 4;
const MINUTE_HAND_W: u32 = 2;
const CENTER_DOT_R: u32 = 7; // diameter

// ── Date label ───────────────────────────────────────────────────────────────
const DATE_X: i32 = 76;
const DATE_Y: i32 = 122;

// ── Day-of-week strip (bottom of screen) ─────────────────────────────────────
const DAY_NAMES: [&str; 7] = ["MON", "TUE", "WED", "THU", "FRI", "SAT", "SUN"];
const DAY_W: i32 = 20;
const DAY_H: i32 = 14;
const DAY_GAP: i32 = 1;
const DAY_Y: i32 = 152 - DAY_H - 2; // bottom-anchored with 2 px margin
const DAY_X_START: i32 = (152 - (7 * DAY_W + 6 * DAY_GAP)) / 2;

// 7-segment encoding using A,B,C,D,E,F,G order.
const SEGMENTS: [[bool; 7]; 10] = [
    [true, true, true, true, true, true, false],     // 0
    [false, true, true, false, false, false, false], // 1
    [true, true, false, true, true, false, true],    // 2
    [true, true, true, true, false, false, true],    // 3
    [false, true, true, false, false, true, true],   // 4
    [true, false, true, true, false, true, true],    // 5
    [true, false, true, true, true, true, true],     // 6
    [true, true, true, false, false, false, false],  // 7
    [true, true, true, true, true, true, true],      // 8
    [true, true, true, true, false, true, true],     // 9
];

// ── Sine table for analog hands (Q.14 fixed point, 0°..90°) ──────────────────
const SIN_Q14: [i16; 91] = [
    0, 286, 572, 857, 1143, 1428, 1713, 1997, 2280, 2563, 2845, 3126, 3406, 3686, 3964, 4240, 4516,
    4790, 5063, 5334, 5604, 5872, 6138, 6402, 6664, 6924, 7182, 7438, 7692, 7943, 8192, 8438, 8682,
    8923, 9162, 9397, 9630, 9860, 10087, 10311, 10531, 10749, 10963, 11174, 11381, 11585, 11786,
    11982, 12176, 12365, 12551, 12733, 12911, 13085, 13255, 13421, 13583, 13741, 13894, 14044,
    14189, 14330, 14466, 14598, 14726, 14849, 14968, 15082, 15191, 15296, 15396, 15491, 15582,
    15668, 15749, 15826, 15897, 15964, 16026, 16083, 16135, 16182, 16225, 16262, 16294, 16322,
    16344, 16362, 16374, 16382, 16384,
];

fn sin_deg(deg: i32) -> i32 {
    let d = deg.rem_euclid(360);
    let v = match d {
        0..=90 => SIN_Q14[d as usize] as i32,
        91..=180 => SIN_Q14[(180 - d) as usize] as i32,
        181..=270 => -(SIN_Q14[(d - 180) as usize] as i32),
        _ => -(SIN_Q14[(360 - d) as usize] as i32),
    };
    v
}

fn cos_deg(deg: i32) -> i32 {
    sin_deg(90 - deg)
}

// ── Clock source ─────────────────────────────────────────────────────────────

#[derive(Clone, Copy)]
struct Clock {
    hour: u8,
    minute: u8,
    day: u8,
    month: u8,
    year: u16,
    weekday: u8, // 0 = MON
}

fn build_clock(unix_secs: u32, tz_offset_hours: i8) -> Option<Clock> {
    use fasttime::Date;

    let offset_secs = tz_offset_hours as i64 * 3600;
    let local = (unix_secs as i64).saturating_add(offset_secs).max(0) as u32;

    let minute = ((local % 3600) / 60) as u8;
    let hour = ((local % 86400) / 3600) as u8;
    let days = (local / 86400) as i64;
    let date = Date::from_days_since_unix_epoch(days).ok()?;
    let weekday = date.weekday().number_from_monday().saturating_sub(1);

    Some(Clock {
        hour,
        minute,
        day: date.day,
        month: date.month,
        year: date.year as u16,
        weekday,
    })
}

#[cfg(feature = "embassy-base")]
fn wall_clock() -> Option<Clock> {
    let unix = crate::unix_now()?;
    let tz = crate::TIMEZONE_OFFSET.load(Ordering::Relaxed);
    build_clock(unix, tz)
}

#[cfg(all(feature = "simulator", not(feature = "embassy-base")))]
fn wall_clock() -> Option<Clock> {
    use std::time::{SystemTime, UNIX_EPOCH};
    let unix = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs() as u32;
    build_clock(unix, 0)
}

#[cfg(not(any(feature = "embassy-base", feature = "simulator")))]
fn wall_clock() -> Option<Clock> {
    None
}

#[cfg(feature = "embassy-base")]
fn battery_pct() -> u8 {
    crate::fw::battery::read_pct()
}

#[cfg(not(feature = "embassy-base"))]
fn battery_pct() -> u8 {
    100
}

// ── Hex (lozenge) segment primitives ─────────────────────────────────────────

/// Filled horizontal lozenge of width `HORIZ_LEN` and thickness `STROKE`,
/// with apexes at the left and right midline.
fn draw_seg_horiz<D>(display: &mut D, x: i32, y: i32) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let fill = PrimitiveStyle::with_fill(BLACK);
    let x = x + HORIZ_INSET;
    Rectangle::new(
        Point::new(x + HALF, y),
        Size::new((HORIZ_LEN - 2 * HALF) as u32, STROKE as u32),
    )
    .into_styled(fill)
    .draw(display)?;
    Triangle::new(
        Point::new(x, y + HALF),
        Point::new(x + HALF, y),
        Point::new(x + HALF, y + STROKE - 1),
    )
    .into_styled(fill)
    .draw(display)?;
    Triangle::new(
        Point::new(x + HORIZ_LEN - 1, y + HALF),
        Point::new(x + HORIZ_LEN - HALF - 1, y),
        Point::new(x + HORIZ_LEN - HALF - 1, y + STROKE - 1),
    )
    .into_styled(fill)
    .draw(display)?;
    Ok(())
}

/// Filled vertical lozenge of length `VERT_LEN` and thickness `STROKE`,
/// with apexes at the top and bottom midline.
fn draw_seg_vert<D>(display: &mut D, x: i32, y: i32) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let fill = PrimitiveStyle::with_fill(BLACK);
    Rectangle::new(
        Point::new(x, y + HALF),
        Size::new(STROKE as u32, (VERT_LEN - 2 * HALF) as u32),
    )
    .into_styled(fill)
    .draw(display)?;
    Triangle::new(
        Point::new(x + HALF, y),
        Point::new(x, y + HALF),
        Point::new(x + STROKE - 1, y + HALF),
    )
    .into_styled(fill)
    .draw(display)?;
    Triangle::new(
        Point::new(x + HALF, y + VERT_LEN - 1),
        Point::new(x, y + VERT_LEN - HALF - 1),
        Point::new(x + STROKE - 1, y + VERT_LEN - HALF - 1),
    )
    .into_styled(fill)
    .draw(display)?;
    Ok(())
}

fn draw_digit<D>(display: &mut D, x: i32, y: i32, digit: u8) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let segs = SEGMENTS.get(digit as usize).copied().unwrap_or([false; 7]);

    if segs[0] {
        draw_seg_horiz(display, x, y)?;
    }
    if segs[1] {
        draw_seg_vert(display, x + DIGIT_W - STROKE, y + STROKE)?;
    }
    if segs[2] {
        draw_seg_vert(display, x + DIGIT_W - STROKE, y + 2 * STROKE + VERT_LEN)?;
    }
    if segs[3] {
        draw_seg_horiz(display, x, y + DIGIT_H - STROKE)?;
    }
    if segs[4] {
        draw_seg_vert(display, x, y + 2 * STROKE + VERT_LEN)?;
    }
    if segs[5] {
        draw_seg_vert(display, x, y + STROKE)?;
    }
    if segs[6] {
        draw_seg_horiz(display, x, y + STROKE + VERT_LEN)?;
    }
    Ok(())
}

// ── Face renderers ───────────────────────────────────────────────────────────

fn draw_digital<D>(display: &mut D, clock: &Clock) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    draw_digit(display, HH_TENS_X, DIGIT_Y, clock.hour / 10)?;
    draw_digit(display, HH_ONES_X, DIGIT_Y, clock.hour % 10)?;

    let dot = PrimitiveStyle::with_fill(BLACK);
    Rectangle::new(
        Point::new(COLON_X, COLON_TOP_Y),
        Size::new(COLON_W as u32, COLON_W as u32),
    )
    .into_styled(dot)
    .draw(display)?;
    Rectangle::new(
        Point::new(COLON_X, COLON_BOT_Y),
        Size::new(COLON_W as u32, COLON_W as u32),
    )
    .into_styled(dot)
    .draw(display)?;

    draw_digit(display, MM_TENS_X, DIGIT_Y, clock.minute / 10)?;
    draw_digit(display, MM_ONES_X, DIGIT_Y, clock.minute % 10)?;
    Ok(())
}

/// Compute the endpoint of a hand of `length` rooted at `(cx, cy)` pointing at `angle_deg`,
/// where 0° is 12 o'clock and angles increase clockwise.
fn polar(cx: i32, cy: i32, length: i32, angle_deg: i32) -> Point {
    let dx = (length * sin_deg(angle_deg)) >> 14;
    let dy = -((length * cos_deg(angle_deg)) >> 14);
    Point::new(cx + dx, cy + dy)
}

fn draw_analog<D>(display: &mut D, clock: &Clock) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    // Outer face circle.
    Circle::with_center(Point::new(ANALOG_CX, ANALOG_CY), (ANALOG_R as u32) * 2)
        .into_styled(PrimitiveStyle::with_stroke(BLACK, 2))
        .draw(display)?;

    // 12 hour ticks.
    let tick_style = PrimitiveStyle::with_stroke(BLACK, 2);
    for i in 0..12i32 {
        let angle = i * 30;
        let outer = polar(ANALOG_CX, ANALOG_CY, ANALOG_R - 1, angle);
        let tick_len = if i % 3 == 0 {
            ANALOG_TICK_CARDINAL
        } else {
            ANALOG_TICK_HOUR
        };
        let inner = polar(ANALOG_CX, ANALOG_CY, ANALOG_R - 1 - tick_len, angle);
        Line::new(inner, outer)
            .into_styled(tick_style)
            .draw(display)?;
    }

    // Hands. Hour hand carries minute fraction so it advances smoothly.
    let hour_angle = (clock.hour as i32 % 12) * 30 + (clock.minute as i32) / 2;
    let minute_angle = (clock.minute as i32) * 6;

    let hour_style = PrimitiveStyleBuilder::new()
        .stroke_color(BLACK)
        .stroke_width(HOUR_HAND_W)
        .build();
    let minute_style = PrimitiveStyleBuilder::new()
        .stroke_color(BLACK)
        .stroke_width(MINUTE_HAND_W)
        .build();

    Line::new(
        Point::new(ANALOG_CX, ANALOG_CY),
        polar(ANALOG_CX, ANALOG_CY, HOUR_HAND_LEN, hour_angle),
    )
    .into_styled(hour_style)
    .draw(display)?;

    Line::new(
        Point::new(ANALOG_CX, ANALOG_CY),
        polar(ANALOG_CX, ANALOG_CY, MINUTE_HAND_LEN, minute_angle),
    )
    .into_styled(minute_style)
    .draw(display)?;

    // Centre dot covers the hand pivot.
    Circle::with_center(Point::new(ANALOG_CX, ANALOG_CY), CENTER_DOT_R)
        .into_styled(PrimitiveStyle::with_fill(BLACK))
        .draw(display)?;

    Ok(())
}

fn draw_date<D>(display: &mut D, clock: &Clock) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let centered = TextStyleBuilder::new()
        .baseline(Baseline::Middle)
        .alignment(Alignment::Center)
        .build();

    let mut date_buf: heapless::String<12> = heapless::String::new();
    let _ = core::fmt::write(
        &mut date_buf,
        format_args!("{:04}-{:02}-{:02}", clock.year, clock.month, clock.day),
    );
    Text::with_text_style(
        &date_buf,
        Point::new(DATE_X, DATE_Y),
        MonoTextStyle::new(&FONT_7X13_BOLD, BLACK),
        centered,
    )
    .draw(display)?;
    Ok(())
}

fn draw_day_strip<D>(display: &mut D, weekday: u8) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let centered = TextStyleBuilder::new()
        .baseline(Baseline::Middle)
        .alignment(Alignment::Center)
        .build();

    for (i, name) in DAY_NAMES.iter().enumerate() {
        let x = DAY_X_START + i as i32 * (DAY_W + DAY_GAP);
        let is_current = i == weekday as usize;
        let rect = Rectangle::new(Point::new(x, DAY_Y), Size::new(DAY_W as u32, DAY_H as u32));
        let fg = if is_current {
            rect.into_styled(PrimitiveStyle::with_fill(RED))
                .draw(display)?;
            WHITE
        } else {
            rect.into_styled(PrimitiveStyle::with_stroke(RED, 1))
                .draw(display)?;
            BLACK
        };
        Text::with_text_style(
            name,
            Point::new(x + DAY_W / 2, DAY_Y + DAY_H / 2),
            MonoTextStyle::new(&FONT_6X10, fg),
            centered,
        )
        .draw(display)?;
    }
    Ok(())
}

/// Black box with white text in the header showing `ALM HH:MM` when an alarm
/// is armed. Uses pure B&W so it survives the fast minute-tick refresh.
fn draw_alarm_indicator<D>(display: &mut D) -> Result<(), D::Error>
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

/// Render the alarm-edit screen: HH:MM in 7-seg digits, an `[On]/[Off]` toggle
/// below, and a black underline beneath the currently-selected field.
fn draw_alarm_edit<D>(display: &mut D) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let h = alarm_hour();
    let m = alarm_minute();

    draw_digit(display, HH_TENS_X, DIGIT_Y, h / 10)?;
    draw_digit(display, HH_ONES_X, DIGIT_Y, h % 10)?;

    let dot = PrimitiveStyle::with_fill(BLACK);
    Rectangle::new(
        Point::new(COLON_X, COLON_TOP_Y),
        Size::new(COLON_W as u32, COLON_W as u32),
    )
    .into_styled(dot)
    .draw(display)?;
    Rectangle::new(
        Point::new(COLON_X, COLON_BOT_Y),
        Size::new(COLON_W as u32, COLON_W as u32),
    )
    .into_styled(dot)
    .draw(display)?;

    draw_digit(display, MM_TENS_X, DIGIT_Y, m / 10)?;
    draw_digit(display, MM_ONES_X, DIGIT_Y, m % 10)?;

    let centered = TextStyleBuilder::new()
        .baseline(Baseline::Middle)
        .alignment(Alignment::Center)
        .build();

    // Three info rows: Days, Tone, Enabled. FONT_6X10 keeps them tight so
    // there's room for an underline beneath each without overlapping the
    // next row's text.
    const ROW_DAYS_Y: i32 = 108;
    const ROW_TONE_Y: i32 = 124;
    const ROW_ENABLED_Y: i32 = 140;
    const ROW_UL_X: i32 = 26;
    const ROW_UL_W: u32 = 100;
    const DIGIT_UL_THICK: u32 = 3;
    const ROW_UL_THICK: u32 = 2;

    let row_style = MonoTextStyle::new(&FONT_6X10, BLACK);
    Text::with_text_style(
        alarm_days_label(),
        Point::new(76, ROW_DAYS_Y),
        row_style,
        centered,
    )
    .draw(display)?;
    Text::with_text_style(
        alarm_tone_label(),
        Point::new(76, ROW_TONE_Y),
        row_style,
        centered,
    )
    .draw(display)?;
    Text::with_text_style(
        alarm_enabled_label(),
        Point::new(76, ROW_ENABLED_Y),
        row_style,
        centered,
    )
    .draw(display)?;

    // Underline beneath the active field.
    let (ul_x, ul_y, ul_w, ul_h) = match current_field() {
        EditField::Hour => (
            HH_TENS_X,
            DIGIT_Y + DIGIT_H + 2,
            PAIR_W as u32,
            DIGIT_UL_THICK,
        ),
        EditField::Minute => (
            MM_TENS_X,
            DIGIT_Y + DIGIT_H + 2,
            PAIR_W as u32,
            DIGIT_UL_THICK,
        ),
        EditField::Days => (ROW_UL_X, ROW_DAYS_Y + 7, ROW_UL_W, ROW_UL_THICK),
        EditField::Tone => (ROW_UL_X, ROW_TONE_Y + 7, ROW_UL_W, ROW_UL_THICK),
        EditField::Enabled => (ROW_UL_X, ROW_ENABLED_Y + 7, ROW_UL_W, ROW_UL_THICK),
    };
    Rectangle::new(Point::new(ul_x, ul_y), Size::new(ul_w, ul_h))
        .into_styled(PrimitiveStyle::with_fill(BLACK))
        .draw(display)?;

    Ok(())
}

pub fn draw<D>(display: &mut D) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let bat = battery_pct();
    let title = match current_mode() {
        WatchMode::AlarmEdit => "Edit Alarm",
        WatchMode::Normal => "Watch",
    };
    draw_frame(display, Some((title, &bat)), None)?;

    if matches!(current_mode(), WatchMode::AlarmEdit) {
        return draw_alarm_edit(display);
    }

    draw_alarm_indicator(display)?;

    let Some(clock) = wall_clock() else {
        let centered = TextStyleBuilder::new()
            .baseline(Baseline::Middle)
            .alignment(Alignment::Center)
            .build();
        Text::with_text_style(
            "Clock not set",
            Point::new(76, 80),
            MonoTextStyle::new(&FONT_7X13_BOLD, BLACK),
            centered,
        )
        .draw(display)?;
        return Ok(());
    };

    match current_face() {
        WatchFace::Digital => draw_digital(display, &clock)?,
        WatchFace::Analog => draw_analog(display, &clock)?,
    }

    draw_date(display, &clock)?;
    draw_day_strip(display, clock.weekday)?;
    Ok(())
}
