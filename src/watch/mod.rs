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
    N_ALARMS, TONES, alarm_day_enabled, alarm_day_n, alarm_days_label, alarm_dec_hour,
    alarm_dec_melody, alarm_dec_minute, alarm_enabled_label, alarm_enabled_n, alarm_hour,
    alarm_hour_n, alarm_inc_hour, alarm_inc_melody, alarm_inc_minute, alarm_is_one_shot_n,
    alarm_minute, alarm_minute_n, alarm_month_n, alarm_toggle_day, alarm_toggle_enabled,
    alarm_tone_label, alarm_year_n, clear_imported_alarms, first_empty_event_slot,
};
#[cfg(feature = "embassy-core")]
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

#[cfg(feature = "embassy-core")]
pub static SETTINGS_DIRTY_SIGNAL: embassy_sync::signal::Signal<
    embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex,
    (),
> = embassy_sync::signal::Signal::new();

#[cfg(feature = "embassy-core")]
pub(crate) fn signal_settings_dirty() {
    SETTINGS_DIRTY_SIGNAL.signal(());
}

#[cfg(not(feature = "embassy-core"))]
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

/// Load persisted watch settings (alarm + face choice + boot chime
/// + per-event notification sound preferences) from the `"watch"`
/// kv namespace.  Call once at boot, after `kv::init()`.  Silently
/// leaves defaults in place if a key is missing or invalid.
#[cfg(feature = "embassy-core")]
pub async fn load_settings_from_kv() {
    use core::sync::atomic::Ordering;
    let ns = crate::fw::kv::namespace("watch");
    alarm::load_settings_from_kv(&ns).await;
    clock::load_settings_from_kv(&ns).await;
    let mut buf = [0u8; 1];
    if let Ok(1) = ns.get("boot_chime", &mut buf).await {
        crate::BOOT_CHIME_ENABLED.store(buf[0] != 0, Ordering::Relaxed);
    }
    #[cfg(feature = "mesh")]
    crate::fw::mesh::sounds::load_settings_from_kv(&ns).await;
}

/// Embassy task that persists watch settings (alarm + face + boot
/// chime + per-event sound preferences) whenever a setter signals
/// `SETTINGS_DIRTY_SIGNAL`.
#[cfg(feature = "embassy-core")]
#[embassy_executor::task]
pub async fn settings_persister_task() {
    use core::sync::atomic::Ordering;
    let ns = crate::fw::kv::namespace("watch");
    loop {
        SETTINGS_DIRTY_SIGNAL.wait().await;
        alarm::persist(&ns).await;
        clock::persist(&ns).await;
        let chime = [crate::BOOT_CHIME_ENABLED.load(Ordering::Relaxed) as u8];
        let _ = ns.set("boot_chime", &chime, true).await;
        #[cfg(feature = "mesh")]
        crate::fw::mesh::sounds::persist(&ns).await;
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
/// ICS is in UTC you'll be off by the `TZ_OFFSET_QH` offset.
///
/// Re-runs on every boot, overwriting whatever was in slots 1..N_ALARMS.
/// The default melody (`ALARM` beep-beep) is applied; the trigger
/// auto-disables each one-shot slot after firing, so old events stop
/// alarming themselves at midnight.
/// Read buffer used to slurp `ALARMS.ICS` at boot.  Larger than the
/// previous 4 KiB so a stripped Bornhack-programme dump (~100 events ×
/// ~250 B per event with DESCRIPTION/UID/etc removed) fits.  Lives on
/// the stack only during the brief boot import — released before any
/// user-facing task starts.
#[cfg(feature = "embassy-core")]
const ICS_READ_BUF_LEN: usize = 16 * 1024;

#[cfg(feature = "embassy-core")]
pub async fn import_alarms_from_fat12() {
    use crate::fw::fat12;
    use crate::fw::led::{self, Rgb};
    use embassy_time::Duration;

    let Some(name) = fat12::to_8_3("ALARMS.ICS") else {
        return;
    };
    let Ok(file) = fat12::find_file(&name).await else {
        return; // not present — nothing to do
    };

    // Visible "we're chewing on the calendar file" feedback — boot
    // import can take a noticeable second on a full festival ICS, and
    // the EPD takes its sweet time to refresh after, so without this
    // the user just sees a frozen pre-import frame.  Blue while we
    // read+parse, off when the slots are populated.
    led::set_color(Rgb::BLUE, led::FOREVER);

    // Boxed onto the heap-equivalent? No heap — keep on stack.  16 KiB
    // is fine on the main task's stack during this brief boot phase.
    let mut buf = [0u8; ICS_READ_BUF_LEN];
    let n = match fat12::read_file(&file, 0, &mut buf).await {
        Ok(n) => n,
        Err(_) => {
            led::off();
            return;
        }
    };

    // Pull the wall-clock UTC offset once — applied to any event whose
    // DTSTART / DTEND carried a `Z` suffix.  The badge has no tzdata,
    // so non-Z timestamps (floating local time, `TZID=...:` values) are
    // taken at face value.
    let tz_offset_min = (crate::tz_offset_secs() / 60) as i32;

    let mut slot = 1usize; // slot 0 stays reserved for the manual alarm
    for event in ics::Parser::new(&buf[..n]) {
        if slot >= alarm::N_ALARMS {
            break;
        }
        let (sy, sm, sd, sh, smi) = if event.start_is_utc {
            shift_utc_to_local(
                event.year,
                event.month,
                event.day,
                event.hour,
                event.minute,
                tz_offset_min,
            )
        } else {
            (event.year, event.month, event.day, event.hour, event.minute)
        };
        let (ey, em, ed, eh, emi) = if event.end_is_utc {
            shift_utc_to_local(
                event.end_year,
                event.end_month,
                event.end_day,
                event.end_hour,
                event.end_minute,
                tz_offset_min,
            )
        } else {
            (
                event.end_year,
                event.end_month,
                event.end_day,
                event.end_hour,
                event.end_minute,
            )
        };

        // Day-view assumes start and end are on the same day.  Multi-
        // day events get clamped to 23:59 of the start day so the
        // renderer doesn't have to reason about midnight crossings.
        let (final_eh, final_emi) = if (ey, em, ed) == (sy, sm, sd) {
            (eh, emi)
        } else {
            (23, 59)
        };

        alarm::set_alarm_time_n(slot, sh, smi);
        alarm::set_alarm_date_n(slot, sy, sm, sd);
        alarm::set_alarm_end_time_n(slot, final_eh, final_emi);
        alarm::set_alarm_summary_n(slot, &event.summary);
        alarm::set_alarm_enabled_n(slot, true);
        slot += 1;
    }
    let imported = slot - 1;
    defmt::info!("imported {} alarm(s) from ALARMS.ICS", imported);

    // Done — drop the blue "working" indicator and (on success) flash a
    // single green pulse so the user knows events landed in slots.
    if imported > 0 {
        led::set_color(Rgb::GREEN, Duration::from_millis(500));
    } else {
        led::off();
    }
}

/// Shift a UTC `(Y, M, D, H, Mi)` to local time using the given hour
/// offset (-12..=+14).  Handles day rollover via fasttime's calendar
/// arithmetic.  Returns the input unchanged if the date is outside
/// fasttime's representable range (shouldn't happen for any realistic
/// value).
#[cfg(feature = "embassy-core")]
fn shift_utc_to_local(
    year: u16,
    month: u8,
    day: u8,
    hour: u8,
    minute: u8,
    offset_min: i32,
) -> (u16, u8, u8, u8, u8) {
    let total = hour as i32 * 60 + minute as i32 + offset_min;
    let day_delta = total.div_euclid(24 * 60);
    let new_hour = (total.rem_euclid(24 * 60) / 60) as u8;
    let minute = (total.rem_euclid(60)) as u8;
    if day_delta == 0 {
        return (year, month, day, new_hour, minute);
    }
    let (y, mo, d) = calendar::add_days(year, month, day, day_delta as i64);
    (y, mo, d, new_hour, minute)
}

// ── Top-level draw ──────────────────────────────────────────────────────────

pub fn draw<D>(display: &mut D) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let bat = crate::platform::battery_pct();
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
