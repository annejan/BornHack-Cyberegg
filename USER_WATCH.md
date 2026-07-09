# Watch, Alarm & Calendar — User Guide

Three apps share the "watch" carousel slots: **Clock**, **Alarm** (entered from Clock), and **Calendar**.

## Clock

Two switchable watch faces — digital and analog. A small bell icon in the header lights up if any alarm is armed.

**Open** — push Left/Right until you land on the **Clock** screen.

| Key                | Action                                  |
| ------------------ | --------------------------------------- |
| Up / Down          | toggle digital ↔ analog face            |
| EXE / Fire         | enter alarm edit (slot 0, see below)    |
| Left / Right       | next / previous carousel screen         |

### Setting the time

The badge has no backup battery for its RTC. The wall clock resets to **None** on every boot, and the display reads "Clock not set" until you set it. Two ways:

- **MeshCore app over BLE** — the phone pushes its time. This is the easy path.
- **Mesh time advert** — stand near a synced LoRa repeater; the badge picks the time up over the air.

Set the timezone once in **Main → Settings → Timezone**. That setting persists across reboots (default is `+2`, CEST for BornHack).

> BLE-set time overrides on-air refinement until next reboot. There is no seconds hand — e-paper refresh is too slow for that.

## Alarm

Push EXE / Fire while on the Clock screen to open the alarm editor for slot 0.

| Key                | Action                                            |
| ------------------ | ------------------------------------------------- |
| Up / Down          | move between fields (Hour → Minute → Days → Tone → Enabled) |
| EXE / Fire         | drill in / out of the field's edit mode           |
| CAN                | exit back to the watch face                       |

The **Days** field cycles: Daily · Weekdays · Weekends · None · Custom.

Ten built-in tones to pick from in the **Tone** field:

- Beep, Imperial March, Rickroll, Pink Panther, Sandstorm, Startup, Trololo, Daisy Bell, Nokia, Samsung

When an alarm fires the buzzer plays the chosen tone up to five times, 8 seconds apart. Any button press silences it. If you ignore it, the alarm stops itself after about 32 seconds.

> Alarms only fire when the clock is set. If the badge has rebooted and you haven't paired or heard a time advert, the alarm is dead — pair first.

## Calendar

Month grid with a per-day timeline of imported iCalendar events.

**Open** — Left/Right to the **Calendar** screen (right of Clock).

### Passive view

The month grid is shown, no cursor. Push **EXE / Fire** to enter active mode (cursor appears).

### Active mode

| Key                | Action                                         |
| ------------------ | ---------------------------------------------- |
| Up / Down          | move cursor ±7 days (jump a week)              |
| Left / Right       | move cursor ±1 day                             |
| EXE / Fire         | open the day-detail timeline                   |
| CAN                | back to passive view                           |

### Day detail (timeline)

Shows one day's events as a vertical strip.

| Key                | Action                                          |
| ------------------ | ----------------------------------------------- |
| Up / Down          | scroll ±1 hour                                  |
| Left / Right       | scroll long event titles horizontally           |
| EXE / Fire         | full day-list (all events as a list)            |
| CAN                | back to month view                              |

### Loading events

The badge imports iCalendar events at boot from a file called **`ALARMS.ICS`** in the root of the USB drive.

1. Plug USB-C cable into your computer.
2. Open the drive labelled `CYBR<4 hex>`.
3. Drop your `.ics` file (rename to `ALARMS.ICS`) in the root.
4. Eject the drive.
5. Reboot the badge (unplug USB or reset).

You can use the official BornHack programme `.ics` straight from `https://bornhack.dk/`.

> Cap: 31 events stored. Multi-day events get clamped to start day 23:59 (e-paper doesn't draw events spanning days). All events are RAM-only and re-imported on every boot from `ALARMS.ICS`.

### Import limits & quirks

The parser is deliberately minimal. If events are missing or look odd,
one of these is usually why:

- **File size: 16 KiB max.** The file is read in one go into a fixed
  buffer; anything past 16 KiB is silently cut off mid-event. A full
  conference programme easily exceeds this — trim it first with
  `scripts/strip_ics.py` (drops DESCRIPTION/UID/etc. and supports
  `--from` / `--to` / `--max` to select a range).
- **31 events max.** Import stops quietly at the cap; later events in
  the file never appear.
- **No recurrence.** `RRULE` is ignored — a repeating event imports as
  its first occurrence only. Export "expanded" / per-occurrence ICS
  instead (the BornHack programme already is).
- **No all-day events.** A DATE-only `DTSTART` (`;VALUE=DATE:YYYYMMDD`)
  is dropped without warning. Give the event a real start time.
- **ASCII only.** Non-ASCII characters in titles are stripped, not
  transliterated (`Æ`, accents, emoji simply vanish). Edit the SUMMARY
  to plain ASCII if the spelling matters.
- **Timezones.** Only `Z`-suffixed (UTC) timestamps are shifted to
  local time — and always by the built-in default of **UTC+2** (right
  for BornHack), because the import runs before your persisted timezone
  setting is applied. Floating times and `TZID=`-zoned times are taken
  as-is, zone discarded. When in doubt, export in UTC.
- **Fired events disappear from the Calendar until reboot.** Imported
  events are one-shot alarms: once one has fired, its slot is disabled
  and it no longer shows on the grid or day view. Rebooting re-imports
  everything.
- **Edits apply at boot only.** Replace `ALARMS.ICS`, eject the drive
  properly (so the write is flushed), then power-cycle the badge.
