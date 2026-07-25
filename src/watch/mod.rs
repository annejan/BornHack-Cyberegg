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

/// Load persisted watch settings (alarm + face choice + boot chime
/// + per-event notification sound preferences) from the `"watch"`
/// kv namespace.  Call once at boot, after `kv::init()`.  Silently
/// leaves defaults in place if a key is missing or invalid.
#[cfg(feature = "embassy-base")]
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
#[cfg(feature = "embassy-base")]
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

/// Read window used to walk `ALARMS.ICS`.  The file is parsed in chunks
/// rather than slurped, so its size is not a limit on how many events
/// can be imported — only [`alarm::N_ALARMS`] is.  8 KiB holds many
/// whole `VEVENT` blocks at a time while staying comfortable on the
/// stack during the brief import.
#[cfg(feature = "embassy-base")]
const ICS_READ_BUF_LEN: usize = 8 * 1024;

/// How many events the last import had to leave out because the slots
/// ran out.  Surfaced on the calendar screen so a truncated import says
/// so instead of quietly showing half a programme.
static EVENTS_DROPPED: core::sync::atomic::AtomicU16 = core::sync::atomic::AtomicU16::new(0);

/// Events the last import could not fit into the available slots.
pub fn events_dropped() -> u16 {
    EVENTS_DROPPED.load(core::sync::atomic::Ordering::Relaxed)
}

/// Populate alarm slots 1..N_ALARMS from `ALARMS.ICS` on the FAT12
/// partition, if the file is there.  Slot 0 is reserved for the user's
/// manual alarm and is left untouched.
///
/// Drop the file on the badge by mounting its USB mass-storage partition
/// (hold Execute on plug-in if you need DFU first) and copying any
/// iCalendar export — the schedule from <https://bornhack.dk/.../program/ics/>
/// works directly.  Times with a `Z` suffix are converted using
/// `TIMEZONE_OFFSET`; floating and `TZID=…:` values are taken at face
/// value, since the badge ships no tzdata.
///
/// Runs at boot and again whenever [`ics_reload_task`] sees the file
/// change, clearing the previous import first so a new file replaces the
/// schedule rather than merging into it.  The default melody (`ALARM`
/// beep-beep) is applied; the trigger auto-disables each one-shot slot
/// after firing, so old events stop alarming themselves at midnight.
#[cfg(feature = "embassy-base")]
pub async fn import_alarms_from_fat12() {
    use core::sync::atomic::Ordering;

    use crate::fw::fat12;
    use crate::fw::led::{self, LED_BLUE, LedState};

    EVENTS_DROPPED.store(0, Ordering::Relaxed);

    let Some(name) = fat12::to_8_3("ALARMS.ICS") else {
        return;
    };
    let Ok(file) = fat12::find_file(&name).await else {
        return; // not present — nothing to do
    };
    let Ok(mut reader) = fat12::FileReader::open(&file).await else {
        return; // corrupt chain — leave whatever is already loaded
    };

    // Drop the previous import first. On a re-import the new file may be
    // shorter than the old one, and stale events left in the tail slots
    // would show up on the calendar as entries no ICS file mentions.
    // Also clears anything added via Settings → Events → Quick test.
    alarm::clear_imported_alarms();

    // Visible "we're chewing on the calendar file" feedback — boot
    // import can take a noticeable second on a full festival ICS, and
    // the EPD takes its sweet time to refresh after, so without this
    // the user just sees a frozen pre-import frame.  Blue while we
    // read+parse, off when the slots are populated.
    led::set_led(&LED_BLUE, LedState::On);

    // Pull the wall-clock UTC offset once — applied to any event whose
    // DTSTART / DTEND carried a `Z` suffix.  The badge has no tzdata,
    // so non-Z timestamps (floating local time, `TZID=...:` values) are
    // taken at face value.
    let tz_offset = crate::TIMEZONE_OFFSET.load(Ordering::Relaxed);

    let mut buf = [0u8; ICS_READ_BUF_LEN];
    let mut slot = 1usize; // slot 0 stays reserved for the manual alarm
    let mut dropped = 0u16;
    // How much of `buf` is carried over from the previous window (the
    // tail of a `VEVENT` that straddled the boundary).
    let mut carry = 0usize;

    loop {
        let read = match reader.read(&mut buf[carry..]).await {
            Ok(n) => n,
            Err(_) => break,
        };
        let avail = carry + read;
        if avail == 0 {
            break;
        }

        let mut parser = ics::Parser::new(&buf[..avail]);
        for event in &mut parser {
            if slot >= alarm::N_ALARMS {
                dropped = dropped.saturating_add(1);
                continue;
            }
            store_event(slot, &event, tz_offset);
            slot += 1;
        }

        let used = parser.consumed();
        if read == 0 {
            break; // end of file, and the tail held no further event
        }
        if used == 0 && avail == buf.len() {
            // A single VEVENT longer than the whole window — drop the
            // window and move on rather than spinning on the same bytes.
            carry = 0;
            continue;
        }
        // Keep the unparsed tail and refill behind it.
        carry = avail - used;
        buf.copy_within(used..avail, 0);
    }

    let imported = slot - 1;
    EVENTS_DROPPED.store(dropped, Ordering::Relaxed);
    if dropped > 0 {
        defmt::warn!(
            "imported {} alarm(s) from ALARMS.ICS, {} dropped (only {} slots)",
            imported,
            dropped,
            alarm::N_ALARMS - 1
        );
    } else {
        defmt::info!("imported {} alarm(s) from ALARMS.ICS", imported);
    }

    // Done — drop the blue "working" indicator and (on success) flash a
    // single green pulse so the user knows events landed in slots.
    led::set_led(&LED_BLUE, LedState::Off);
    if imported > 0 {
        led::set_led(&crate::fw::led::LED_GREEN, LedState::Duty50Once);
    }
}

/// Set by **Settings → Events → Reload from ICS** to ask
/// [`ics_reload_task`] for an immediate re-import.  A signal rather than
/// a direct call because the import is async and menu actions are not.
#[cfg(feature = "embassy-base")]
pub static ICS_RELOAD_SIGNAL: embassy_sync::signal::Signal<
    embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex,
    (),
> = embassy_sync::signal::Signal::new();

/// Menu action: re-read `ALARMS.ICS` without a reboot.
#[cfg(feature = "embassy-base")]
pub fn request_ics_reload() {
    ICS_RELOAD_SIGNAL.signal(());
}

/// Re-import `ALARMS.ICS` whenever the file might have changed.
///
/// Two triggers:
///   * the host wrote to the USB mass-storage partition and then went
///     quiet — MSC gives no "copy finished" notification and hosts flush
///     lazily, so the settle window is the only reliable signal;
///   * the user picked **Settings → Events → Reload from ICS**.
///
/// Either way the import clears the old event slots first, so dropping a
/// new calendar on the badge replaces the schedule rather than merging
/// into it.
#[cfg(feature = "embassy-base")]
#[embassy_executor::task]
pub async fn ics_reload_task() {
    use embassy_time::{Duration, Timer};

    /// How long the host must stay quiet before we treat a copy as done.
    const SETTLE: Duration = Duration::from_secs(2);
    /// Poll interval while idle.  Cheap (one atomic load) and the
    /// latency it adds is invisible next to an EPD refresh.
    const POLL: Duration = Duration::from_millis(500);

    let mut seen = host_write_count();

    loop {
        // Wait for either trigger.
        let manual = loop {
            if ICS_RELOAD_SIGNAL.signaled() {
                ICS_RELOAD_SIGNAL.reset();
                break true;
            }
            if host_write_count() != seen {
                break false;
            }
            Timer::after(POLL).await;
        };

        if !manual {
            // Let the host finish: re-arm the settle window for as long
            // as blocks keep arriving.
            loop {
                let before = host_write_count();
                Timer::after(SETTLE).await;
                if host_write_count() == before {
                    break;
                }
            }
            defmt::info!("watch: USB write settled, re-reading ALARMS.ICS");
        } else {
            defmt::info!("watch: manual ALARMS.ICS reload");
        }

        import_alarms_from_fat12().await;
        // Nudge the display loop so a visible calendar picks the new
        // events up straight away instead of at the next minute tick.
        crate::TOAST_SIGNAL.signal(());
        seen = host_write_count();
    }
}

/// Blocks the USB host has written to the FAT partition.
///
/// Always zero in builds without `usb-storage`: there is no host write
/// path at all, so the auto-reload trigger never fires and
/// [`ics_reload_task`] runs on the manual signal alone.
#[cfg(feature = "embassy-base")]
fn host_write_count() -> u32 {
    #[cfg(feature = "usb-storage")]
    {
        crate::fw::usb_msc::host_write_count()
    }
    #[cfg(not(feature = "usb-storage"))]
    {
        0
    }
}

/// Write one parsed event into an alarm slot, converting UTC timestamps
/// to local time on the way in.
#[cfg(feature = "embassy-base")]
fn store_event(slot: usize, event: &ics::Event, tz_offset: i8) {
    // All-day events carry no meaningful clock time, so there is nothing
    // to shift — and shifting would push them onto the wrong day.
    let (sy, sm, sd, sh, smi) = if event.start_is_utc && !event.all_day {
        shift_utc_to_local(
            event.year,
            event.month,
            event.day,
            event.hour,
            event.minute,
            tz_offset,
        )
    } else {
        (event.year, event.month, event.day, event.hour, event.minute)
    };
    let (ey, em, ed, eh, emi) = if event.end_is_utc && !event.all_day {
        shift_utc_to_local(
            event.end_year,
            event.end_month,
            event.end_day,
            event.end_hour,
            event.end_minute,
            tz_offset,
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
    // An all-day entry belongs on the calendar but must not ring at
    // midnight — see `alarm::set_alarm_silent_n`.
    alarm::set_alarm_silent_n(slot, event.all_day);
    alarm::set_alarm_enabled_n(slot, true);
}

/// Shift a UTC `(Y, M, D, H, Mi)` to local time using the given hour
/// offset (-12..=+14).  Handles day rollover via fasttime's calendar
/// arithmetic.  Returns the input unchanged if the date is outside
/// fasttime's representable range (shouldn't happen for any realistic
/// value).
#[cfg(feature = "embassy-base")]
fn shift_utc_to_local(
    year: u16,
    month: u8,
    day: u8,
    hour: u8,
    minute: u8,
    tz_offset_hours: i8,
) -> (u16, u8, u8, u8, u8) {
    let total = hour as i32 + tz_offset_hours as i32;
    let day_delta = total.div_euclid(24);
    let new_hour = total.rem_euclid(24) as u8;
    if day_delta == 0 {
        return (year, month, day, new_hour, minute);
    }
    match fasttime::Date::from_ymd(year as i32, month, day)
        .ok()
        .and_then(|d| d.add_days(day_delta as i64).ok())
    {
        Some(d) => (d.year as u16, d.month, d.day, new_hour, minute),
        None => (year, month, day, new_hour, minute),
    }
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
