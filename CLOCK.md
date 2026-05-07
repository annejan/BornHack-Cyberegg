# Clock, Alarm & Calendar — CyberÆgg Watch App

> **See also:** [README.md](README.md) for project overview, [CONTACTS_SCREEN.md](CONTACTS_SCREEN.md) for the meshcore chat UI (contacts list, PM inbox + threads), [GAME.md](GAME.md) for player-facing game instructions, [GAMES.md](GAMES.md) for mini-game developer reference, [NFC_README.md](NFC_README.md) for NFC signed channel protocol, [HWTEST.md](HWTEST.md) for the factory hardware-test firmware, [License.md](License.md) for licensing.

The badge includes a full-featured watch application with two switchable faces, a 32-slot alarm system, and a calendar browser — all accessible from the **Clock** icon in the main icon grid.

---

## Clock Faces

The watch has two faces, toggled with **Up/Down** while on the watch screen:

### Digital Face

Casio-style 7-segment LCD digits with hex (lozenge) segments meeting at 45° miters. Displays:

- **Time**: large `HH:MM` centered on screen
- **Date**: ISO format (`YYYY-MM-DD`) below the digits
- **Weekday strip**: bottom-anchored row with days Mon–Sun; current day shown white-on-red, others outlined in red

### Analog Face

Circular dial with:
- 12 hour ticks (longer at 12/3/6/9)
- Thick hour hand + thin minute hand (hour hand carries minute fraction for smooth sweep)
- **Date complication** at 12 o'clock (`DD Mon`)
- **Weekday complication** at 6 o'clock (red text)
- No separate weekday strip (the dial fills the body)

### Time Source

The clock reads `unix_now()` (set via the MeshCore companion `SET_DEVICE_TIME` 0x06 command) and applies `TIMEZONE_OFFSET` (configured under **Settings → Timezone**).

When the clock has not been synced, the screen shows **"Clock not set"**.

The watch redraws on every minute boundary via the shared `MINUTE_TICK` signal — no second hand, as the e-paper refresh is too slow.

The current face survives reboots — it is persisted to the `"watch"` KV namespace alongside alarm settings.

---

## Alarm System

Up to **32 alarm slots** (`N_ALARMS = 32`):

| Slot range | Purpose |
| ----------- | ------- |
| **Slot 0** | Manual recurring alarm — set via the on-screen editor or **Settings → Alarm** |
| **Slots 1–31** | One-shot calendar events — populated from `ALARMS.ICS` at boot |

### Recurring Alarm (Slot 0)

A once-per-day alarm that fires on selected weekdays.

#### On-screen editor

Press **Fire/Execute** from the watch screen to enter alarm-edit mode. Two layers:

**Row-nav** (default):

| Button | Action |
| ------ | ------- |
| Up / Down | Move field: Hour → Minute → Days → Tone → Enabled |
| Fire / Execute | Drill into the selected field (or toggle Enabled inline) |
| Cancel | Exit edit mode (changes are live, no save needed) |

**Field active** (after Fire on a steppable field):

| Button | Action |
| ------ | ------- |
| Up / Down | Increment / decrement the active value |
| Fire / Execute | Exit field editing, back to row-nav |
| Cancel | Exit field editing, back to row-nav |

#### Settings → Alarm submenu

Provides steppers for Hour, Minute, Days, Tone, and Enabled toggle. The Days submenu lets you toggle individual weekdays; the parent label summarises the mask as `Daily`, `Weekdays`, `Weekends`, `Custom`, or `None`.

#### Day mask presets

The alarm fires only on enabled days (bit 0 = Mon … bit 6 = Sun). The Days field cycles through:

| Mask | Label |
| ----- | ----- |
| `0x7F` | Daily |
| `0x1F` | Weekdays (Mon–Fri) |
| `0x60` | Weekends (Sat–Sun) |
| `0x00` | None |
| other | Custom |

#### Tone

The alarm melody is a curated subset of `MELODIES`:

| Tone | Label |
| ---- | ----- |
| Beep (default) | Tone: Beep |
| Imperial March | Tone: Imp. March |
| Rickroll | Tone: Rickroll |
| Pink Panther | Tone: Pink Pant. |
| Sandstorm | Tone: Sandstorm |
| Startup | Tone: Startup |
| Trololo | Tone: Trololo |
| Daisy Bell | Tone: Daisy Bell |
| Nokia | Tone: Nokia |
| Samsung | Tone: Samsung |

### Calendar Events (Slots 1–31)

One-shot alarms bound to a specific date (`year-month-day`). These are **populated from `ALARMS.ICS`** at boot (see Calendar section below) or manually via the **Quick test +5min** action in **Settings → Events**.

One-shot slots auto-disable after firing so they don't re-alarm on reboot.

### Firing & Dismissal

When the wall clock matches an enabled alarm's time:

1. The buzzer plays the selected melody
2. The alarm repeats up to **4 times** every **8 seconds**
3. **Pressing any button** silences the buzzer (and consumes that button — a second press is needed to navigate)
4. After 5 seconds an un-dismissed alarm auto-clears

The Clock face header shows a small red bell when any alarm is enabled. If a future-firing alarm is scheduled for later today, its `HH:MM` appears next to the bell in black.

---

## Calendar

Reachable from the icon grid right after Clock. Browse imported events on a month grid and inspect them on a timeline.

### Three modes

| Mode | Entry | Description |
| ----- | ----- | ----------- |
| **Passive** | Default on entry | Month grid with today highlighted in red, days-with-events get a small red dot. All buttons fall through so you can scroll past Calendar with Left/Right. Fire/Execute enters Active. |
| **Active** | Fire/Execute from Passive | Cursor border visible. Up/Down moves ±7 days, Left/Right moves ±1 day (crosses month boundaries). Fire/Execute drills into Day-detail. Cancel returns to Passive. |
| **Day-detail** | Fire/Execute from Active | Timeline view of every event on the cursor day, scrollable with Up/Down (±1 hour). Left/Right scroll event titles in 3-char steps. Fire/Execute opens Day-list popup. Cancel returns to Active. |
| **Day-list** | Fire/Execute from Day-detail | Full-screen list of every event on the cursor day with full (untruncated) summaries. Up/Down scroll rows. Cancel returns to Day-detail. |

### Timeline view (Day-detail)

- Fixed 18-hour window (6 AM–12 AM) with 18 px/hour scaling
- Events render as filled black blocks (red if currently happening), height proportional to duration
- Zero-duration events (missing `DTEND`) render as thin 4px markers
- The event title shows inside the block if it is tall enough (≥13 px); titles longer than the block width scroll with Left/Right
- A red horizontal line marks the current time (only when viewing today)
- `^` / `v` arrows on the right edge indicate hidden events above/below the visible window

### Importing Events — `ALARMS.ICS`

Drop an iCalendar file named `ALARMS.ICS` onto the FAT12 partition (mount the badge as USB mass storage, hold Execute on plug-in if needed).

At boot, the firmware:
1. Reads `ALARMS.ICS` into a 16 KiB buffer
2. Parses each `BEGIN:VEVENT` block (extracting `DTSTART`, `DTEND`, `SUMMARY`)
3. Populates slots 1..31 with one-shot alarms
4. Slot 0 (manual recurring alarm) is left untouched

**Import notes:**
- Re-runs at every boot — edits while running don't take effect until reboot
- Caps at 31 events (slots 1..31) or ~15–25 events depending on `SUMMARY` length (4 KiB effective read)
- Multi-day events clamped to 23:59 of the start day
- Times with `Z` suffix (UTC) are converted using `TIMEZONE_OFFSET`; floating times and `TZID=...:` values are taken at face value
- The Bornhack programme export from <https://bornhack.dk/.../program/ics/> works directly — the parser handles `TZID=…:` parameters and CRLF line endings

An example file (Bornhack 2026 opening + closing) ships at `assets/to-badge/ALARMS.ICS`.

### Settings → Events

Lists every populated one-shot slot read-only (`<n>: HH:MM MM-DD`) plus two actions:

| Action | Description |
| ------ | ----------- |
| **Quick test +5min** | Drops a `Quick test` event 5 minutes from now in the first empty slot. Handy for verifying the alarm path without USB. Silently no-ops if the wall clock isn't synced or all slots are taken. |
| **Clear all** | Destructive — disables and zeros slots 1..31 immediately. |

Empty slots are auto-hidden — you only scroll past events that actually exist.

---

## ICS Parser (`watch/ics.rs`)

Minimal RFC 5545 parser — extracts only what the badge needs:

| Property | Parsed | Notes |
| -------- | ------ | ----- |
| `DTSTART` | `YYYYMMDDTHHMMSS` (floating), `YYYYMMDDTHHMMSSZ` (UTC), `TZID=…:YYYYMMDDTHHMMSS` | Seconds discarded; `Z` flag tracked per timestamp |
| `DTEND` | Same formats as DTSTART | Optional; when missing, event is zero-duration (start == end) |
| `SUMMARY` | First 31 ASCII bytes | Non-ASCII bytes dropped; NUL-padded |

**Not implemented:** line folding, `VALUE=` overrides, `RRULE` recurrence, escape sequences (`\,`, `\;`, `\n`), nested `VTIMEZONE` blocks, all-day `DATE` values. Bornhack ICS dumps don't use these features.

---

## Persistence

All watch state is persisted to the `"watch"` KV namespace in ekv (flash key-value store):

| Key | Value | What |
| --- | ----- | --- |
| `face` | 1 byte (0 = Digital, 1 = Analog) | Selected watch face |
| `alarm_h` | 1 byte (0–23) | Slot 0 alarm hour |
| `alarm_m` | 1 byte (0–59) | Slot 0 alarm minute |
| `alarm_on` | 1 byte (0/1) | Slot 0 enabled flag |
| `alarm_days` | 1 byte (bitmask) | Slot 0 day mask |
| `alarm_mel` | 1 byte (melody index) | Slot 0 tone selection |
| `boot_chime` | 1 byte (0/1) | Settings → Boot chime toggle (piggybacks on the watch persister so it lands in the same flash batch) |

The `SETTINGS_DIRTY_SIGNAL` is signalled on every setting change; the `settings_persister_task` waits on this signal and persists clock face, alarm state, and the boot-chime toggle in one batch.

Calendar event slots (1..31) are **RAM-only** — they're not persisted to flash. They are re-imported from `ALARMS.ICS` at each boot.

---

## Button Reference

### Watch screen (normal mode)

| Button | Action |
| ------ | ------- |
| Up / Down | Toggle between digital and analog face |
| Fire / Execute | Enter alarm-edit mode |
| Left / Right | Navigate to adjacent screens |

### Alarm-edit mode

See the [Alarm System](#alarm-system) section above for the full two-layer button table.

### Calendar — Passive mode

| Button | Action |
| ------ | ------- |
| Fire / Execute | Enter Active mode (cursor appears) |
| Left / Right | Navigate to adjacent screens (falls through) |
| Up / Down / Cancel | Fall through to menu layer |

### Calendar — Active mode

| Button | Action |
| ------ | ------- |
| Up / Down | Move cursor ±7 days |
| Left / Right | Move cursor ±1 day (crosses month boundaries) |
| Fire / Execute | Drill into Day-detail for cursor day |
| Cancel | Return to Passive mode |

### Calendar — Day-detail (timeline)

| Button | Action |
| ------ | ------- |
| Up / Down | Scroll timeline ±1 hour |
| Left / Right | Scroll event titles ±3 chars |
| Fire / Execute | Open Day-list popup (full summaries) |
| Cancel | Return to Active mode |

### Calendar — Day-list popup

| Button | Action |
| ------ | ------- |
| Up / Down | Scroll rows ±1 |
| Cancel | Return to Day-detail |
| Any other | Consumed (no screen-nav while popup is up) |
