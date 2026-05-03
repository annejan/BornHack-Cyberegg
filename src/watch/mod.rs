//! Watch app — switchable Casio-style digital face and analog face, plus an
//! on-device alarm clock.
//!
//! This module is now a thin coordinator: state and rendering live in two
//! sibling submodules, `alarm` and `clock`.  External callers keep using
//! the same `crate::watch::*` paths; the items they need are re-exported
//! below.
//!
//! Normal mode buttons:
//!   * Up/Down       — toggle digital ↔ analog face
//!   * Fire/Execute  — enter alarm-edit mode
//!
//! Alarm-edit mirrors the Settings-menu stepper pattern: Up/Down moves the
//! selection between fields (Hour, Minute, Days, Tone, Enabled), Fire drills
//! into a field (Up/Down then steps the value, Fire or Cancel pops back), and
//! Cancel from row-nav exits the edit screen entirely.  See `alarm` for the
//! full button table.
//!
//! The current weekday is highlighted in red (white-on-red) for visual punch.
//! Note: the red plane only updates on a full tri-color refresh; on the fast
//! B&W minute-tick refresh the red pixels won't redraw, so the current-day
//! highlight may look stale until the next full refresh.

mod alarm;
pub mod calendar;
mod clock;
mod ics;

// ── Public re-exports — keep external paths stable ──────────────────────────
//
// `crate::watch::*` already exposes these; menu.rs and embassy.rs reference
// them by their unqualified names.  The submodules are kept private so the
// only entry points are the ones below.
pub use alarm::{
    N_ALARMS, alarm_day_enabled, alarm_day_n, alarm_days_label, alarm_dec_hour, alarm_dec_melody,
    alarm_dec_minute, alarm_enabled_label, alarm_enabled_n, alarm_hour, alarm_hour_n,
    alarm_inc_hour, alarm_inc_melody, alarm_inc_minute, alarm_is_one_shot_n, alarm_minute,
    alarm_minute_n, alarm_month_n, alarm_toggle_day, alarm_toggle_enabled, alarm_tone_label,
    alarm_year_n, clear_imported_alarms, first_empty_event_slot,
};
#[cfg(feature = "embassy-base")]
pub use alarm::{
    add_quick_event, alarm_ring_timeout_task, check_and_fire_alarm, dismiss_alarm_if_ringing,
};
use embedded_graphics::prelude::*;

use crate::menu::ButtonId;
use crate::{TriColor, draw_frame};

// ── Settings-dirty signalling ───────────────────────────────────────────────
//
// Both the alarm submodule and the clock submodule call this when a setter
// has updated their state.  The `settings_persister_task` below waits on the
// signal and persists both submodules' state to the shared `"watch"` KV
// namespace.

#[cfg(feature = "embassy-base")]
pub static SETTINGS_DIRTY_SIGNAL: embassy_sync::signal::Signal<
    embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex,
    (),
> = embassy_sync::signal::Signal::new();

#[cfg(feature = "embassy-base")]
pub(crate) fn signal_settings_dirty() {
    SETTINGS_DIRTY_SIGNAL.signal(());
}

#[cfg(not(feature = "embassy-base"))]
pub(crate) fn signal_settings_dirty() {}

// ── Button dispatch ─────────────────────────────────────────────────────────

/// Returns `true` if the button was consumed by the watch screen.
pub fn dispatch(btn: ButtonId) -> bool {
    use alarm::WatchMode;
    match alarm::current_mode() {
        WatchMode::AlarmEdit => alarm::dispatch_edit(btn),
        WatchMode::Normal => clock::dispatch_normal(btn),
    }
}

// ── KV load / persist ───────────────────────────────────────────────────────

/// Load persisted watch settings (alarm + face choice) from the `"watch"` kv
/// namespace. Call once at boot, after `kv::init()`. Silently leaves defaults
/// in place if a key is missing or invalid.
#[cfg(feature = "embassy-base")]
pub async fn load_settings_from_kv() {
    let ns = crate::fw::kv::namespace("watch");
    alarm::load_settings_from_kv(&ns).await;
    clock::load_settings_from_kv(&ns).await;
}

/// Embassy task that persists watch settings (alarm + face) whenever a setter
/// signals `SETTINGS_DIRTY_SIGNAL`.
#[cfg(feature = "embassy-base")]
#[embassy_executor::task]
pub async fn settings_persister_task() {
    let ns = crate::fw::kv::namespace("watch");
    loop {
        SETTINGS_DIRTY_SIGNAL.wait().await;
        alarm::persist(&ns).await;
        clock::persist(&ns).await;
    }
}

/// At boot: if a file named `ALARMS.ICS` exists on the FAT12 partition,
/// parse its `VEVENT` blocks and populate alarm slots 1..N_ALARMS with
/// one-shot date alarms.  Slot 0 is reserved for the user's manual alarm
/// and is left untouched.
///
/// Drop the file on the badge by mounting its USB mass-storage partition
/// (hold Execute on plug-in if you need DFU first) and copying any
/// iCalendar export — the schedule from <https://bornhack.dk/.../program/ics/>
/// works directly.  Times are taken at face value as local time; if your
/// ICS is in UTC you'll be off by `TIMEZONE_OFFSET` hours.
///
/// Re-runs on every boot, overwriting whatever was in slots 1..N_ALARMS.
/// The default melody (`ALARM` beep-beep) is applied; the trigger
/// auto-disables each one-shot slot after firing, so old events stop
/// alarming themselves at midnight.
#[cfg(feature = "embassy-base")]
pub async fn import_alarms_from_fat12() {
    use crate::fw::fat12;

    let Some(name) = fat12::to_8_3("ALARMS.ICS") else {
        return;
    };
    let Ok(file) = fat12::find_file(&name).await else {
        return; // not present — nothing to do
    };

    let mut buf = [0u8; 4096];
    let n = match fat12::read_file(&file, 0, &mut buf).await {
        Ok(n) => n,
        Err(_) => return,
    };

    let mut slot = 1usize; // slot 0 stays reserved for the manual alarm
    for event in ics::Parser::new(&buf[..n]) {
        if slot >= alarm::N_ALARMS {
            break;
        }
        alarm::set_alarm_time_n(slot, event.hour, event.minute);
        alarm::set_alarm_date_n(slot, event.year, event.month, event.day);
        alarm::set_alarm_summary_n(slot, &event.summary);
        alarm::set_alarm_enabled_n(slot, true);
        slot += 1;
    }
    defmt::info!("imported {} alarm(s) from ALARMS.ICS", slot - 1);
}

// ── Top-level draw ──────────────────────────────────────────────────────────

pub fn draw<D>(display: &mut D) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let bat = clock::battery_pct();
    let title = match alarm::current_mode() {
        alarm::WatchMode::AlarmEdit => "Edit Alarm",
        alarm::WatchMode::Normal => "Clock",
    };
    draw_frame(display, Some((title, &bat)), None)?;

    if matches!(alarm::current_mode(), alarm::WatchMode::AlarmEdit) {
        return alarm::draw_edit(display);
    }

    alarm::draw_indicator(display)?;
    clock::draw_face(display)
}
