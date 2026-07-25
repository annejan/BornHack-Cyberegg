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
const ICS_READ_BUF_LEN: usize = 8 * 1024;

/// Events the last import couldn't give an alarm slot to.  They still
/// show on the calendar — that reads the file — but they can't ring, and
/// the calendar says so rather than letting a silent alarm pass for a
/// working one.
static EVENTS_DROPPED: core::sync::atomic::AtomicU16 = core::sync::atomic::AtomicU16::new(0);

/// Events the last import could not give an alarm slot.
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

    use crate::fw::led::{self, LED_BLUE, LedState};

    EVENTS_DROPPED.store(0, Ordering::Relaxed);

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

    // Only events from `horizon` onward go into the alarm slots: with a
    // year-long calendar the slots can't hold everything, and what they
    // are *for* is ringing, so the near future is what belongs in them.
    // The month grid and day views don't read slots at all — they use
    // the day index and the on-demand day cache, both of which cover the
    // whole file. Without a synced wall clock, take the file from the
    // top rather than importing nothing.
    let horizon = today_day_number().unwrap_or(i64::MIN);

    index_clear();
    let mut slot = 1usize; // slot 0 stays reserved for the manual alarm
    let mut dropped = 0u16;
    let mut total = 0u16;

    let found = scan_ics(|event| {
        total = total.saturating_add(1);
        index_mark(event);
        // Multi-day events keep ringing until their last day is behind
        // us, so compare against the end date.
        let end = ics::days_from_civil(event.end_year, event.end_month, event.end_day);
        if end < horizon {
            return core::ops::ControlFlow::Continue(());
        }
        if slot >= alarm::N_ALARMS {
            dropped = dropped.saturating_add(1);
            return core::ops::ControlFlow::Continue(());
        }
        store_event(slot, event, tz_offset);
        slot += 1;
        core::ops::ControlFlow::Continue(())
    })
    .await;

    if !found {
        led::set_led(&LED_BLUE, LedState::Off);
        return;
    }
    EVENTS_TOTAL.store(total, Ordering::Relaxed);

    let imported = slot - 1;
    EVENTS_DROPPED.store(dropped, Ordering::Relaxed);
    if dropped > 0 {
        defmt::warn!(
            "ALARMS.ICS: {} events, {} armed, {} beyond the {} alarm slots",
            events_total(),
            imported,
            dropped,
            alarm::N_ALARMS - 1
        );
    } else {
        defmt::info!(
            "ALARMS.ICS: {} events, {} armed",
            events_total(),
            imported
        );
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
        // Wait for a trigger: a day the calendar wants cached, a manual
        // reload, or the host having written to the FAT partition.
        let manual = loop {
            if ICS_RELOAD_SIGNAL.signaled() {
                ICS_RELOAD_SIGNAL.reset();
                // A pending day request is the cheap case — serve it and
                // go back to waiting rather than re-importing.
                let req = DAY_REQUEST.swap(0, core::sync::atomic::Ordering::Relaxed);
                if req != 0 {
                    let (y, m, d) = (
                        (req >> 16) as u16,
                        ((req >> 8) & 0xff) as u8,
                        (req & 0xff) as u8,
                    );
                    load_day(y, m, d).await;
                    crate::TOAST_SIGNAL.signal(());
                    continue;
                }
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
        // The cached day is from the old file — reload it so the screen
        // doesn't keep showing events the new calendar dropped.
        let stale = with_day_cache(|c| c.date);
        if stale != (0, 0, 0) {
            load_day(stale.0, stale.1, stale.2).await;
        }
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

// ── Whole-file scanning ─────────────────────────────────────────────────────
//
// A festival programme fits in the alarm slots; a personal calendar
// export does not. Rather than growing the slot array until it does, the
// file on flash stays the source of truth and RAM holds only what a
// screen can show:
//
//   * the alarm slots — near-future events only, because ringing is all
//     they are for;
//   * a one-bit-per-day index of which days have anything at all, which
//     is everything the month grid needs;
//   * a cache of the single day the cursor is on, refilled by rescanning
//     the file whenever the cursor moves.
//
// That makes the number of events the badge can hold a property of the
// filesystem, not of RAM.

/// Walk every event in `ALARMS.ICS`, handing each to `visit`.
///
/// The file is read through an 8 KiB sliding window, keeping the tail of
/// any `VEVENT` that straddles a boundary, so file size is not a limit.
/// Returns `false` if the file is missing or unreadable — distinct from
/// "read fine and contained no events".
#[cfg(feature = "embassy-base")]
async fn scan_ics<F>(mut visit: F) -> bool
where
    F: FnMut(&ics::Event) -> core::ops::ControlFlow<()>,
{
    use crate::fw::fat12;

    let Some(name) = fat12::to_8_3("ALARMS.ICS") else {
        return false;
    };
    let Ok(file) = fat12::find_file(&name).await else {
        return false;
    };
    let Ok(mut reader) = fat12::FileReader::open(&file).await else {
        return false;
    };

    let mut buf = [0u8; ICS_READ_BUF_LEN];
    // How much of `buf` is carried over from the previous window.
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
            if visit(&event).is_break() {
                return true;
            }
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

    true
}

// ── Day index ───────────────────────────────────────────────────────────────

/// Days covered by the has-events index — a little over two years, which
/// is as far as anyone is going to scroll a badge calendar.
const INDEX_DAYS: usize = 768;
const INDEX_BYTES: usize = INDEX_DAYS / 8;

/// Day number ([`ics::days_from_civil`]) of bit 0 of [`INDEX_BITS`].
/// `i32::MIN` means "no index built yet".  32-bit because the target has
/// no 64-bit atomics, which is fine — a day number needs 16 bits for any
/// date a badge will see.
static INDEX_BASE: core::sync::atomic::AtomicI32 =
    core::sync::atomic::AtomicI32::new(i32::MIN);

/// One bit per day: set when at least one event covers that day. This is
/// all the month grid needs, and at 96 bytes it covers the whole file no
/// matter how many events are in it.
static INDEX_BITS: [core::sync::atomic::AtomicU8; INDEX_BYTES] =
    [const { core::sync::atomic::AtomicU8::new(0) }; INDEX_BYTES];

/// Total events seen by the last import, across the whole file.
static EVENTS_TOTAL: core::sync::atomic::AtomicU16 = core::sync::atomic::AtomicU16::new(0);

/// Events in the file, including those too far out for an alarm slot.
pub fn events_total() -> u16 {
    EVENTS_TOTAL.load(core::sync::atomic::Ordering::Relaxed)
}

fn index_clear() {
    use core::sync::atomic::Ordering;
    INDEX_BASE.store(i32::MIN, Ordering::Relaxed);
    for byte in INDEX_BITS.iter() {
        byte.store(0, Ordering::Relaxed);
    }
}

/// Mark every day `event` covers, extending the index window to fit it.
///
/// The window anchors on the first event seen and only ever moves
/// earlier, so an out-of-order file still indexes correctly as long as
/// its events span less than [`INDEX_DAYS`].
fn index_mark(event: &ics::Event) {
    use core::sync::atomic::Ordering;

    let start = ics::days_from_civil(event.year, event.month, event.day);
    let end = ics::days_from_civil(event.end_year, event.end_month, event.end_day).max(start);

    let Ok(start_i32) = i32::try_from(start) else {
        return;
    };
    let base = INDEX_BASE.load(Ordering::Relaxed);
    let base = if base == i32::MIN {
        INDEX_BASE.store(start_i32, Ordering::Relaxed);
        start
    } else if start_i32 < base {
        // An earlier event than anything seen so far: slide the window
        // back, dropping whatever falls off the far end. Rare — exports
        // are chronological — and losing a dot beyond a two-year span is
        // better than losing the near-term ones.
        index_shift(base as i64 - start);
        INDEX_BASE.store(start_i32, Ordering::Relaxed);
        start
    } else {
        base as i64
    };

    for day in start..=end {
        let Ok(bit) = usize::try_from(day - base) else {
            continue;
        };
        if bit >= INDEX_DAYS {
            break;
        }
        let byte = &INDEX_BITS[bit / 8];
        byte.store(byte.load(Ordering::Relaxed) | 1 << (bit % 8), Ordering::Relaxed);
    }
}

/// Shift every marked day `by` places later, i.e. move the window's base
/// `by` days earlier.
fn index_shift(by: i64) {
    use core::sync::atomic::Ordering;

    let Ok(by) = usize::try_from(by) else {
        return;
    };
    if by >= INDEX_DAYS {
        for byte in INDEX_BITS.iter() {
            byte.store(0, Ordering::Relaxed);
        }
        return;
    }
    // Walk downward so a bit is read before the slot it moves into is
    // overwritten.
    for bit in (0..INDEX_DAYS).rev() {
        let set = if bit >= by {
            let src = bit - by;
            INDEX_BITS[src / 8].load(Ordering::Relaxed) & 1 << (src % 8) != 0
        } else {
            false
        };
        let byte = &INDEX_BITS[bit / 8];
        let mask = 1u8 << (bit % 8);
        let cur = byte.load(Ordering::Relaxed);
        byte.store(if set { cur | mask } else { cur & !mask }, Ordering::Relaxed);
    }
}

/// Whether any event covers `(year, month, day)`.
///
/// Answers from the index, so it is correct for the whole file rather
/// than just the events currently in alarm slots.
pub fn day_has_events(year: u16, month: u8, day: u8) -> bool {
    use core::sync::atomic::Ordering;

    let base = INDEX_BASE.load(Ordering::Relaxed);
    if base == i32::MIN {
        return false;
    }
    let Ok(bit) = usize::try_from(ics::days_from_civil(year, month, day) - base as i64) else {
        return false;
    };
    bit < INDEX_DAYS && INDEX_BITS[bit / 8].load(Ordering::Relaxed) & 1 << (bit % 8) != 0
}

// ── Day cache ───────────────────────────────────────────────────────────────

/// Most events one day of the cache will hold.  Well past what the
/// day-detail timeline and the day list can show on a 152 px panel.
pub const DAY_CACHE_MAX: usize = 24;

/// One event as the day views need it: times plus the label, so the
/// renderer never has to reach back into an alarm slot.
#[derive(Clone, Copy)]
pub struct CachedEvent {
    pub hour: u8,
    pub minute: u8,
    pub end_hour: u8,
    pub end_minute: u8,
    pub summary: [u8; ics::SUMMARY_LEN],
}

impl CachedEvent {
    const EMPTY: Self = Self {
        hour: 0,
        minute: 0,
        end_hour: 0,
        end_minute: 0,
        summary: [0; ics::SUMMARY_LEN],
    };

    /// The label decoded from Latin-1, ready to draw.
    pub fn summary(&self) -> heapless::String<{ ics::SUMMARY_LEN * 2 }> {
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

/// Every event on one day, refilled from the file whenever the calendar
/// cursor lands on a different date.
pub struct CachedDay {
    /// The day this cache holds, or `(0, 0, 0)` when nothing is loaded.
    pub date: (u16, u8, u8),
    /// Events on `date`, sorted by start time.
    pub events: [CachedEvent; DAY_CACHE_MAX],
    /// How many of `events` are valid.
    pub len: u8,
    /// Events on this day that didn't fit in `events`.
    pub overflow: u8,
}

impl CachedDay {
    pub fn valid(&self) -> &[CachedEvent] {
        &self.events[..self.len as usize]
    }
}

#[cfg(feature = "embassy-base")]
type DayCacheMutex = embassy_sync::blocking_mutex::Mutex<
    embassy_sync::blocking_mutex::raw::ThreadModeRawMutex,
    core::cell::RefCell<CachedDay>,
>;
#[cfg(feature = "simulator")]
type DayCacheMutex = std::sync::Mutex<core::cell::RefCell<CachedDay>>;

/// The one day the calendar screen currently needs.
///
/// Written by [`ics_reload_task`] after a rescan, read by the calendar
/// renderer. Holding a single day rather than the whole file is what
/// lets the badge show a calendar with more events than fit in RAM.
#[cfg(any(feature = "embassy-base", feature = "simulator"))]
pub static DAY_CACHE: DayCacheMutex = DayCacheMutex::new(core::cell::RefCell::new(CachedDay {
    date: (0, 0, 0),
    events: [CachedEvent::EMPTY; DAY_CACHE_MAX],
    len: 0,
    overflow: 0,
}));

/// Run `f` against the day cache.
#[cfg(feature = "embassy-base")]
pub fn with_day_cache<R>(f: impl FnOnce(&CachedDay) -> R) -> R {
    DAY_CACHE.lock(|cell| f(&cell.borrow()))
}
#[cfg(all(feature = "simulator", not(feature = "embassy-base")))]
pub fn with_day_cache<R>(f: impl FnOnce(&CachedDay) -> R) -> R {
    let guard = DAY_CACHE.lock().unwrap_or_else(|e| e.into_inner());
    f(&guard.borrow())
}

/// Date the calendar wants cached, packed as `y << 16 | m << 8 | d`.
/// Zero means "nothing requested".
#[cfg(feature = "embassy-base")]
static DAY_REQUEST: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);

/// Ask for `(year, month, day)` to be loaded into [`DAY_CACHE`].
///
/// Called from the calendar's button handling, which is synchronous —
/// the actual rescan happens in [`ics_reload_task`]. A no-op when the
/// day is already cached, so holding an arrow key doesn't queue work.
#[cfg(feature = "embassy-base")]
pub fn request_day(year: u16, month: u8, day: u8) {
    use core::sync::atomic::Ordering;

    if with_day_cache(|c| c.date == (year, month, day)) {
        return;
    }
    let packed = (year as u32) << 16 | (month as u32) << 8 | day as u32;
    DAY_REQUEST.store(packed, Ordering::Relaxed);
    ICS_RELOAD_SIGNAL.signal(());
}

/// The simulator has no filesystem to read, so the calendar renders
/// against an empty cache and an empty index — enough for laying screens
/// out.  The index accessors work either way; only the loading does not.
#[cfg(not(feature = "embassy-base"))]
pub fn request_day(_year: u16, _month: u8, _day: u8) {}

/// Rescan the file for one day and publish it to [`DAY_CACHE`].
#[cfg(feature = "embassy-base")]
async fn load_day(year: u16, month: u8, day: u8) {
    use core::sync::atomic::Ordering;

    let tz_offset = crate::TIMEZONE_OFFSET.load(Ordering::Relaxed);
    let want = ics::days_from_civil(year, month, day);

    let mut events = [CachedEvent::EMPTY; DAY_CACHE_MAX];
    let mut len = 0usize;
    let mut overflow = 0u8;

    scan_ics(|event| {
        let (sy, sm, sd, sh, smi, eh, emi) = local_times(event, tz_offset);
        let start = ics::days_from_civil(sy, sm, sd);
        // A multi-day event belongs to every day it covers, but the
        // day view has no concept of "continues tomorrow", so it shows
        // on each as a full day.
        let end_day = ics::days_from_civil(
            event.end_year,
            event.end_month,
            event.end_day,
        )
        .max(start);
        if want < start || want > end_day {
            return core::ops::ControlFlow::Continue(());
        }

        // Clip to the requested day: a run-on event starts at 00:00 on
        // any day but its first, and ends at 23:59 on any but its last.
        let (h, mi) = if want == start { (sh, smi) } else { (0, 0) };
        let (eh, emi) = if want == end_day { (eh, emi) } else { (23, 59) };

        if len >= DAY_CACHE_MAX {
            overflow = overflow.saturating_add(1);
            return core::ops::ControlFlow::Continue(());
        }
        events[len] = CachedEvent {
            hour: h,
            minute: mi,
            end_hour: eh,
            end_minute: emi,
            summary: event.summary,
        };
        len += 1;
        core::ops::ControlFlow::Continue(())
    })
    .await;

    // Insertion sort by start time — `len` is capped at DAY_CACHE_MAX,
    // so this stays trivial no matter how big the file is.
    for i in 1..len {
        let key = events[i];
        let mut j = i;
        while j > 0 && (events[j - 1].hour, events[j - 1].minute) > (key.hour, key.minute) {
            events[j] = events[j - 1];
            j -= 1;
        }
        events[j] = key;
    }

    DAY_CACHE.lock(|cell| {
        let mut c = cell.borrow_mut();
        c.date = (year, month, day);
        c.events = events;
        c.len = len as u8;
        c.overflow = overflow;
    });
}

/// Local start/end of `event`, applying the UTC offset where the source
/// asked for it.  Shared by the slot importer and the day cache so the
/// two can't disagree about what time an event happens.
#[cfg(feature = "embassy-base")]
fn local_times(event: &ics::Event, tz_offset: i8) -> (u16, u8, u8, u8, u8, u8, u8) {
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
    // Day-view assumes start and end are on the same day; multi-day
    // events get clamped to 23:59 of the start day by the caller.
    let (final_eh, final_emi) = if (ey, em, ed) == (sy, sm, sd) {
        (eh, emi)
    } else {
        (23, 59)
    };
    (sy, sm, sd, sh, smi, final_eh, final_emi)
}

/// First day the index has anything on — the calendar's fallback cursor
/// when the wall clock hasn't synced yet, so a fresh badge opens on the
/// start of the programme rather than an arbitrary month.
pub fn first_indexed_day() -> Option<(u16, u8, u8)> {
    use core::sync::atomic::Ordering;

    let base = INDEX_BASE.load(Ordering::Relaxed);
    if base == i32::MIN {
        return None;
    }
    let bit = (0..INDEX_DAYS)
        .find(|&b| INDEX_BITS[b / 8].load(Ordering::Relaxed) & 1 << (b % 8) != 0)?;
    ics::civil_from_days(base as i64 + bit as i64)
}

/// Today as a day number, or `None` when the wall clock isn't synced.
#[cfg(feature = "embassy-base")]
fn today_day_number() -> Option<i64> {
    let c = clock::wall_clock()?;
    Some(ics::days_from_civil(c.year, c.month, c.day))
}

/// Write one parsed event into an alarm slot, converting UTC timestamps
/// to local time on the way in.
#[cfg(feature = "embassy-base")]
fn store_event(slot: usize, event: &ics::Event, tz_offset: i8) {
    let (sy, sm, sd, sh, smi, _, _) = local_times(event, tz_offset);
    alarm::set_alarm_time_n(slot, sh, smi);
    alarm::set_alarm_date_n(slot, sy, sm, sd);
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Build an event covering `start..=end`, the way the index sees it.
    fn event(start: (u16, u8, u8), end: (u16, u8, u8)) -> ics::Event {
        ics::Event {
            year: start.0,
            month: start.1,
            day: start.2,
            hour: 0,
            minute: 0,
            start_is_utc: false,
            end_year: end.0,
            end_month: end.1,
            end_day: end.2,
            end_hour: 23,
            end_minute: 59,
            end_is_utc: false,
            all_day: true,
            summary: [0; ics::SUMMARY_LEN],
        }
    }

    /// The index is global state; these tests mutate it, so they must not
    /// run concurrently.
    static INDEX_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn exclusive() -> std::sync::MutexGuard<'static, ()> {
        let guard = INDEX_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        index_clear();
        guard
    }

    #[test]
    fn index_marks_every_day_an_event_covers() {
        let _guard = exclusive();
        index_mark(&event((2026, 7, 15), (2026, 7, 18)));

        assert!(!day_has_events(2026, 7, 14));
        for day in 15..=18 {
            assert!(day_has_events(2026, 7, day), "missing day {day}");
        }
        assert!(!day_has_events(2026, 7, 19));
    }

    #[test]
    fn index_handles_an_out_of_order_earlier_event() {
        let _guard = exclusive();
        // Anchored on July, then something in June turns up — exports are
        // chronological, but nothing guarantees it.
        index_mark(&event((2026, 7, 15), (2026, 7, 15)));
        index_mark(&event((2026, 6, 1), (2026, 6, 1)));

        assert!(day_has_events(2026, 6, 1), "earlier event lost the window");
        assert!(day_has_events(2026, 7, 15), "later event fell out");
    }

    #[test]
    fn index_covers_a_full_year_of_events() {
        let _guard = exclusive();
        // The whole point: event count is bounded by the file, not RAM.
        // 400 daily events is already well past `N_ALARMS`.
        let mut date = (2026u16, 1u8, 1u8);
        for _ in 0..400 {
            index_mark(&event(date, date));
            let days = ics::days_from_civil(date.0, date.1, date.2) + 1;
            date = ics::civil_from_days(days).unwrap();
        }
        assert!(day_has_events(2026, 1, 1));
        assert!(day_has_events(2026, 12, 31));
        assert!(day_has_events(2027, 2, 4)); // day 400
        assert!(!day_has_events(2027, 2, 5));
    }

    #[test]
    fn first_indexed_day_is_the_earliest_marked() {
        let _guard = exclusive();
        assert_eq!(first_indexed_day(), None, "empty index has no first day");

        index_mark(&event((2026, 7, 15), (2026, 7, 15)));
        index_mark(&event((2026, 7, 20), (2026, 7, 20)));
        assert_eq!(first_indexed_day(), Some((2026, 7, 15)));

        index_mark(&event((2026, 7, 1), (2026, 7, 1)));
        assert_eq!(first_indexed_day(), Some((2026, 7, 1)));
    }

    /// A year of events, larger than the alarm slots and larger than the
    /// read window — the case the file-backed design exists for.
    fn year_long_ics() -> std::string::String {
        use core::fmt::Write;
        let mut out = std::string::String::from("BEGIN:VCALENDAR\r\nVERSION:2.0\r\n");
        let base = ics::days_from_civil(2026, 1, 1);
        for day in 0..365i64 {
            let (y, m, d) = ics::civil_from_days(base + day).unwrap();
            // Two or three events most days, none on every seventh.
            let n = if day % 7 == 6 { 0 } else { 2 + (day % 2) };
            for k in 0..n {
                let h = 8 + k * 2;
                let _ = write!(
                    out,
                    "BEGIN:VEVENT\r\nSUMMARY:Event {day}-{k}\r\n\
                     DTSTART:{y:04}{m:02}{d:02}T{h:02}0000\r\n\
                     DTEND:{y:04}{m:02}{d:02}T{:02}0000\r\n\
                     DESCRIPTION:{}\r\nEND:VEVENT\r\n",
                    h + 1,
                    "x".repeat(200),
                );
            }
        }
        out.push_str("END:VCALENDAR\r\n");
        out
    }

    /// Walk `doc` in 8 KiB windows the way `scan_ics` reads the file,
    /// building the index and filling alarm slots as the importer would.
    fn simulate_import(doc: &[u8]) -> (usize, usize) {
        let (mut total, mut armed) = (0usize, 0usize);
        let mut pos = 0usize;
        while pos < doc.len() {
            let end = (pos + ICS_READ_BUF_LEN).min(doc.len());
            let mut p = ics::Parser::new(&doc[pos..end]);
            for ev in &mut p {
                total += 1;
                index_mark(&ev);
                if armed < alarm::N_ALARMS - 1 {
                    armed += 1;
                }
            }
            let used = p.consumed();
            pos += if used == 0 { end - pos } else { used };
        }
        (total, armed)
    }

    #[test]
    fn a_year_of_events_is_fully_navigable() {
        let _guard = exclusive();
        let doc = year_long_ics();
        assert!(doc.len() > 200_000, "fixture should dwarf the read window");

        let expected: usize = (0..365i64)
            .map(|day| if day % 7 == 6 { 0 } else { 2 + (day % 2) as usize })
            .sum();
        let (total, armed) = simulate_import(doc.as_bytes());
        assert_eq!(
            total, expected,
            "every event survives the sliding window, {expected} of them"
        );
        assert!(total > 4 * alarm::N_ALARMS, "fixture must dwarf the slots");
        assert_eq!(
            armed,
            alarm::N_ALARMS - 1,
            "alarm slots fill up and stop — that's the point"
        );

        // Every day the fixture put an event on stays reachable, long
        // past where the alarm slots ran out.
        assert_eq!(first_indexed_day(), Some((2026, 1, 1)));
        let base = ics::days_from_civil(2026, 1, 1);
        for day in 0..365i64 {
            let (y, m, d) = ics::civil_from_days(base + day).unwrap();
            let expected = day % 7 != 6;
            assert_eq!(
                day_has_events(y, m, d),
                expected,
                "day {day} ({y}-{m}-{d})"
            );
        }
    }

    #[test]
    fn days_beyond_the_index_window_are_not_claimed() {
        let _guard = exclusive();
        index_mark(&event((2026, 1, 1), (2026, 1, 1)));
        // INDEX_DAYS is a little over two years from the anchor.
        let past_end = ics::civil_from_days(
            ics::days_from_civil(2026, 1, 1) + INDEX_DAYS as i64 + 1,
        )
        .unwrap();
        assert!(!day_has_events(past_end.0, past_end.1, past_end.2));
    }
}
