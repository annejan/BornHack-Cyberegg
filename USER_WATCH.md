# Watch, Alarm & Calendar — User Guide

Three apps share the "watch" carousel slots: **Clock**, **Alarm** (entered from Clock), and **Calendar**.

## Clock

Two switchable watch faces — digital and analog. A small bell icon in the header lights up if any alarm is set that will actually ring. Imported all-day events are held silently: they show on the calendar but never ring, and they do not light the bell.

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

**Open** — Left/Right to the **Calendar** screen (right of Clock in the standard edition; on an organizer badge Calendar is the screen you boot onto, with Clock to its right).

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

The badge reads iCalendar events from a file called **`ALARMS.ICS`** in the root of the USB drive.

1. Plug USB-C cable into your computer.
2. Open the drive labelled `CYBR<4 hex>`.
3. Drop your `.ics` file (rename to `ALARMS.ICS`) in the root.
4. Eject the drive.

No reboot. A couple of seconds after the copy finishes the badge notices,
blinks the blue LED while it reads the file, and the calendar has the new
schedule. **Settings → Events → Reload from ICS** forces a re-read if you
would rather not wait.

You can use the official BornHack programme `.ics` straight from
`https://bornhack.dk/` — no trimming, no size limit.

> The badge does not store your events; it reads them back out of the file
> whenever it needs them. That is why the file size doesn't matter, and why
> replacing the file replaces the schedule.

### What the parser handles

| | |
| --- | --- |
| **File size** | No limit. The file is read through a sliding window, so a whole year of events is fine |
| **Events shown** | Every event in the file appears on the calendar |
| **Events that ring** | The nearest 159 upcoming. Anything beyond that shows but cannot ring, and the grid says so in red |
| **Repeating events** | `RRULE` with `FREQ=DAILY`/`WEEKLY`/`MONTHLY`/`YEARLY`, plus `INTERVAL`, `COUNT`, `UNTIL` and `BYDAY`. Capped at 64 occurrences per rule |
| **All-day events** | Supported. They fill the day on the calendar and stay silent — no alarm at midnight |
| **Accents** | `Æ`, `é`, `ø` and the rest of Latin-1 render as written. Beyond that (`Š`, `—`, `…`) gets an ASCII spelling; anything with no sensible spelling shows as `?` |
| **Titles** | First 31 characters |
| **Timezones** | `Z`-suffixed (UTC) timestamps are shifted using your configured offset. Floating and `TZID=`-zoned times are taken as-is, zone discarded — the badge ships no timezone database. When in doubt, export in UTC |

### Remaining quirks

- **Only 159 events can ring in total**, not per day — it is one budget
  shared across the whole upcoming schedule, so a busy tomorrow eats into
  what is left for the days after it. Everything past that still *shows*
  on the calendar; the grid warns in red (`! N can't ring`). Not a limit
  you will meet with a conference programme.
- **A day shows at most 24 events.** Busier days list the first 24 and add
  `+N more today`.
- **Multi-day events** appear on every day they cover, but each day shows
  them as running the whole day — the timeline has no "continues tomorrow".
- **Alarms need the clock set.** If the badge has rebooted and you haven't
  paired or heard a time advert, nothing rings — pair first.
- **Not handled:** `EXDATE` / `RDATE` exceptions, positional `BYDAY`
  (`2MO`, `-1FR`), and line folding of the properties the badge reads.
  Ordinary exports don't use these.
