# BornHack CyberÆgg Badge Firmware

Embassy-based async firmware for the BornHack CyberÆgg badge (nRF52840).

## First use (badge holders)

If you just got a badge, start here:

| Document | Description |
| -------- | ----------- |
| **[QUICKSTART.md](QUICKSTART.md)** | Power on, pair, set the time, charge, recover, USB drag-drop |
| **[POCKET_CARD.md](POCKET_CARD.md)** | One-page printable reference — buttons, LEDs, combos |
| **[FAQ.md](FAQ.md)** | Why the red LED blinks, which screen saves battery, inverted/ghosted screen, DFU |
| **[USER_WATCH.md](USER_WATCH.md)** | Watch face, alarms, calendar, time sync |
| **[USER_GAMES.md](USER_GAMES.md)** | BornPets virtual pet + seven mini-games |
| **[USER_MESH.md](USER_MESH.md)** | LoRa mesh: adverts, PMs, channels, identity QR |
| **[USER_CONTACTS.md](USER_CONTACTS.md)** | Contacts list, filters, saving, blocking |
| **[USER_NFC.md](USER_NFC.md)** | NFC station taps via the BadgeCtl app |

The rest of this README is developer-facing.

## Hardware

| Component  | Part              | Interface      |
| ---------- | ----------------- | -------------- |
| MCU        | nRF52840          | —              |
| Display    | SSD1675 e-paper   | SPI3           |
| LoRa radio | SX1262            | SPI2           |
| BLE        | nRF52840 built-in | nrf-sdc / MPSL |

## Default LoRa radio settings

Out of the box the badge joins **EU/UK Narrow**, the stock MeshCore channel, so it
shares airtime with unmodified MeshCore nodes:

| Parameter        | Default     |
| ---------------- | ----------- |
| Frequency        | 869.618 MHz |
| Bandwidth        | 62.5 kHz    |
| Spreading factor | SF8         |
| Coding rate      | 4/5         |
| TX power         | 22 dBm      |
| Client repeat    | off         |

All of these are overridable from the on-device Settings menu or the companion app,
and the chosen values persist to flash. TX power defaults to the SX1262 maximum of
22 dBm — the badge antenna is not fully efficient, so radiated power stays within the
band limit; trim it via the Power menu if your local rules require it.

Single source of truth: `DEFAULT_RADIO` in [src/lib.rs](src/lib.rs). Change it there
and both the boot-time defaults and the persisted-settings fallback follow.

## Documentation

All project documentation is in markdown files at the repository root and in `vendor/` subdirectories:

| Document | Description |
| -------- | ----------- |
| **[README.md](README.md)** | This file — project overview, hardware, build instructions, known issues |
| **[GAME.md](GAME.md)** | Player-facing game instructions, controls, stats, mini-games |
| **[GAMES.md](GAMES.md)** | Developer reference for all seven mini-games, controls, scoring |
| **[CONTACTS_SCREEN.md](CONTACTS_SCREEN.md)** | On-device meshcore chat: contacts list, popup actions, PM inbox + threads, discovery cache |
| **[CLOCK.md](CLOCK.md)** | Watch faces, alarm system (32 slots), calendar browser, ICS parser |
| **[NFC_README.md](NFC_README.md)** | NFC signed channel protocol spec, reader implementation guide |
| **[HWTEST.md](HWTEST.md)** | Hardware test firmware — factory diagnostics, beep codes |
| **[SUBMODULES.md](SUBMODULES.md)** | How `vendor/` git submodules work — cloning, updating pins, hacking on vendor libs, sibling read-only checkouts |
| **[License.md](License.md)** | Apache 2.0 + Empty File License |

Vendor library documentation:

| Document | Description |
| -------- | ----------- |
| **[vendor/ssd1675/README.md](vendor/ssd1675/README.md)** | SSD1675/SSD1675B ePaper display driver |
| **[vendor/meshcore/README.md](vendor/meshcore/README.md)** | MeshCore LoRa packet protocol — `no_std` Rust port |
| **[vendor/meshcore-companion/README.md](vendor/meshcore-companion/README.md)** | MeshCore companion protocol — BLE NUS commands/responses |

## Architecture

The firmware runs several concurrent Embassy tasks:

- **BLE task** — GATT peripheral exposing Nordic UART Service (NUS). Speaks the MeshCore companion protocol; handles all commands from the companion app and pushes async notifications.
- **MeshCore task** — drives the SX1262 in continuous RX. Receives/transmits MeshCore packets (adverts, private messages, channel messages, trace-path, login). Forwards received packets to the BLE task via channels.
- **Display task** — renders UI screens to the SSD1675 e-paper display.
- **Buzzer task** — plays melodies on the piezo buzzer via PWM, triggered by signal from any task.
- **Battery task** — periodic ADC reads of battery voltage, caches percentage for display.
- **Minute tick / advert ticker** — periodic timers for game updates and self-advertisement scheduling.

### Bootloader

A custom USB-DFU bootloader (`nrf-aegg-bootloader`) replaces the factory Adafruit UF2 bootloader.\
The `bootloader/` directory is a standalone Cargo project (not in the workspace, not tracked in git).

Flash partition layout — single app region, no DFU staging:

| Region     | Start        | End          | Size  |
| ---------- | ------------ | ------------ | ----- |
| Bootloader | `0x00000000` | `0x0000FFFF` | 64 K  |
| App        | `0x00010000` | `0x000FFFFF` | 960 K |

The main app's `memory-fw.x` sets `FLASH ORIGIN = 0x00010000` and `LENGTH = 960K`.\
The bootloader exports `APP_START = 0x00010000` for the post-boot jump.

### Vendor libraries

| Library              | Location                     | Notes                                                                                                                 |
| -------------------- | ---------------------------- | --------------------------------------------------------------------------------------------------------------------- |
| `meshcore`           | `vendor/meshcore/`           | MeshCore packet codec (no_std)                                                                                        |
| `meshcore-companion` | `vendor/meshcore-companion/` | BLE companion protocol encoder/decoder (no_std)                                                                       |
| `ssd1675`            | `vendor/ssd1675/`            | Async Embassy SSD1675 driver with OTP LUT readback, variant detection (A/B), `UpdateMode`, `BorderWaveform`, fast LUT |

## Connecting with MeshCore

The badge is compatible with the MeshCore companion app:

- **Android / iOS** — install the **MeshCore** app from the Google Play Store / App Store.
- **Browser** — open <https://app.meshcore.nz/> (works in any browser that supports Web Bluetooth, e.g. Chrome / Edge on desktop or Android).

When the badge boots, it begins advertising over BLE. On first pairing a numeric passkey is shown on the e-paper display — enter this in the app to complete the bond.

Once connected, the MeshCore app gives full control of the LoRa mesh side of the firmware:

- View and message contacts discovered over LoRa
- Send and receive channel messages
- Manage stored contacts and routing paths
- Adjust radio parameters (frequency, bandwidth, spreading factor, TX power)
- Run path traces (ping) to nearby nodes
- Monitor incoming advertisements and ACKs in real time

The badge appears in the app as a standard MeshCore node. All mesh activity (received messages, adverts, ACKs) is pushed to the app as BLE notifications without polling.

## NFC station commands

The badge emulates an ISO 14443-A Type 4 tag and accepts authenticated
"station" commands (feed, heal, inspire, sleep) over NFC. Authentication
uses Ed25519 signatures with a challenge-response handshake — the badge
holds only the public verifying key, while the matching private key
lives in the reader app.

The reference reader is the [BadgeCtl Android app](../../android_nfc/BadgeCtl).

For the full protocol spec, wire format, status words, and a step-by-step
guide for implementing your own reader (Kotlin / Python / Rust examples),
see **[NFC_README.md](NFC_README.md)**.

The NFC tag also serves a plaintext, user-settable NDEF broadcast profile
for phone-side tag readers — a vanity URL, vCard, Wi-Fi record, or any
NDEF message you write to it, defaulting to `https://badge.team`. Writing
a `token:` record instead collects a token on the Tokens screen. See
[NFC_README.md](NFC_README.md) §5.

## MeshCore Companion Protocol

The firmware implements the [MeshCore companion protocol](https://docs.meshcore.io/companion_protocol/) over BLE NUS.

### Commands handled (companion → device)

| Code   | Name                   | Description                                     |
| ------ | ---------------------- | ----------------------------------------------- |
| `0x01` | `APP_START`            | Returns `SELF_INFO`                             |
| `0x02` | `SEND_TXT_MSG`         | Send private message to contact                 |
| `0x03` | `SEND_CHANNEL_MSG`     | Send message to a channel                       |
| `0x04` | `GET_CONTACTS`         | Stream all contacts (`CONTACT_START/.../END`)   |
| `0x06` | `SET_DEVICE_TIME`      | Set RTC clock                                   |
| `0x07` | `SEND_SELF_ADVERT`     | Flood or zero-hop self-advertisement            |
| `0x08` | `SET_ADVERT_NAME`      | Set advertised node name                        |
| `0x09` | `ADD_UPDATE_CONTACT`   | Add or update a contact                         |
| `0x0A` | `SYNC_NEXT_MESSAGE`    | Dequeue next pending message                    |
| `0x0B` | `SET_RADIO_PARAMS`     | Change LoRa frequency/BW/SF/CR                  |
| `0x0C` | `SET_RADIO_TX_POWER`   | Change TX power                                 |
| `0x0D` | `RESET_PATH`           | Clear routing path for a contact (set to flood) |
| `0x0E` | `SET_ADVERT_LATLON`    | Set advertised GPS position                     |
| `0x0F` | `REMOVE_CONTACT`       | Delete a contact                                |
| `0x13` | `REBOOT`               | Reboot device                                   |
| `0x14` | `GET_BATT_AND_STORAGE` | Returns battery voltage and storage stats       |
| `0x16` | `DEVICE_QUERY`         | Returns `DEVICE_INFO`                           |
| `0x1A` | `SEND_LOGIN`           | Login to a room/repeater server                 |
| `0x1E` | `GET_CONTACT_BY_KEY`   | Fetch full contact record by public key         |
| `0x1F` | `GET_CHANNEL`          | Fetch stored channel name and key               |
| `0x20` | `SET_CHANNEL`          | Store a channel name and key                    |
| `0x24` | `SEND_TRACE_PATH`      | Send trace packet (ping); returns `0x89` async  |
| `0x26` | `SET_OTHER_PARAMS`     | Set telemetry mode, location policy, etc.       |
| `0x2A` | `GET_ADVERT_PATH`      | Return last-seen advert path for a contact      |
| `0x36` | `SET_FLOOD_SCOPE`      | Set or clear transport-key flood scope          |

### Push notifications (device → companion)

| Code   | Name               | Trigger                                |
| ------ | ------------------ | -------------------------------------- |
| `0x80` | `ADVERTISEMENT`    | Node advert received over LoRa         |
| `0x82` | `ACK`              | Message ACK received                   |
| `0x83` | `MESSAGES_WAITING` | Incoming message queued                |
| `0x85` | `LOGIN_SUCCESS`    | Login accepted by remote node          |
| `0x86` | `LOGIN_FAIL`       | Login rejected                         |
| `0x88` | `LOG_RX_DATA`      | Raw received packet log                |
| `0x89` | `TRACE_DATA`       | Trace-path result (response to `0x24`) |
| `0x8A` | `NEW_ADVERT`       | Newly discovered or updated contact    |

## On-device UI

The badge has a 152x152 e-paper display and a 4-direction joystick with Fire, Execute, and Cancel buttons. Several features are accessible directly from the badge without needing the companion app.

### Navigation

| Button         | Action                                                     |
| -------------- | ---------------------------------------------------------- |
| Left / Right   | Switch between screens (Main, Channels, PM, Adverts, etc.) |
| Up / Down      | Scroll within a menu or list                               |
| Fire / Execute | Select / confirm                                           |
| Cancel         | Go back / dismiss                                          |

### Watch

> **Full documentation:** [CLOCK.md](CLOCK.md) — watch faces, alarm system, calendar browser, ICS parser.

A standalone watch screen — accessible from the main icon grid via Left/Right — with two switchable faces:

- **Digital** — Casio-style 7-segment LCD with hex (lozenge) segments that meet at 45° miters. Big `HH:MM` digits, ISO date underneath, and a bottom-anchored weekday strip with the current day shown white-on-red and the others outlined in red.
- **Analog** — circular dial with 12 hour ticks (longer at 12/3/6/9), thick hour hand and thin minute hand. The hour hand carries the minute fraction so it advances smoothly between hour marks. Same date and weekday strip below the dial.

The screen redraws on every minute boundary via the shared `MINUTE_TICK` signal — no second hand and no per-second redraws, since the e-paper refresh is too slow for that anyway.

The watch reads `unix_now()` (set via the MeshCore companion `SET_DEVICE_TIME` 0x06 command) and applies `TIMEZONE_OFFSET` (configured under Settings → Timezone). When the clock has not yet been synced the screen shows "Clock not set".

#### Watch buttons

In normal viewing mode:

| Button         | Action                                            |
| -------------- | ------------------------------------------------- |
| Up / Down      | Toggle between digital and analog face            |
| Fire / Execute | Enter alarm-edit mode (see below)                 |
| Left / Right   | Navigate to adjacent screens                      |

The current face survives reboots — it's persisted to the `"watch"` kv namespace alongside the alarm settings.

#### Alarm

The watch screen has a built-in once-per-day alarm. When any alarm slot is armed, the Clock face header shows a small red bell — and if a future-firing alarm is scheduled for later today, the next firing time appears as `HH:MM` (black) next to the bell.

There are two ways to set it:

1. **On the watch screen** — press Fire/Execute to enter edit mode. The header changes to `Edit Alarm`, the digits show the alarm time (not the wall clock), and a `[ On ]` / `[ Off ]` indicator + tone label appear below. A thick black bar marks the active field. The edit screen has two layers — row-nav (default) and field-active (after Fire on a steppable field):

   | Button         | Row-nav (default)                                                          | Field active (after Fire)                  |
   | -------------- | -------------------------------------------------------------------------- | ------------------------------------------ |
   | Up / Down      | Move field cursor: Hour → Minute → Days → Tone → Enabled                   | Increment / decrement the active value     |
   | Fire / Execute | Drill into the selected field (or just toggle the `Enabled` field inline) | Exit field editing, back to row-nav        |
   | Cancel         | Exit edit mode entirely (changes are live, no save needed)                 | Exit field editing, back to row-nav        |

2. **From Settings** — Main → Settings → Alarm. The submenu has Hour and Minute steppers, a Days submenu, a Tone stepper, and an Enabled toggle. The Days submenu lets you toggle individual weekdays; the parent label summarises the mask as `Daily`, `Weekdays`, `Weekends`, `Custom`, or `None`. The Tone stepper cycles through a curated subset of `MELODIES` (Beep, Imp. March, Rickroll, Pink Pant., Sandstorm, Startup, Trololo).

When the wall clock matches the armed time on a selected day, the buzzer plays a short "beep beep" pattern and repeats up to 4 times every 8 s. **Pressing any button anywhere in the menu silences the buzzer** (and consumes that button — a second press is needed to actually navigate). After 5 s an un-dismissed alarm auto-clears so it stops eating button presses.

All alarm state — hour, minute, day mask, and enabled flag — is persisted to flash and survives reboots.

#### Calendar events

Beyond the single recurring alarm in slot 0, the watch carries up to **31 one-shot calendar event slots** (slots 1..31 of `N_ALARMS = 32`). Each event has a date (year/month/day), a time, an enabled flag, a 31-byte ASCII summary, and shares slot 0's currently-selected ringtone when it fires.

**Calendar screen.** Reachable from the icon grid right after Clock. Three modes:

- **Passive** (default on entry): month grid with today highlighted in red, days-with-events get a small red dot, no cursor visible. All buttons fall through so you can scroll past Calendar with Left/Right just like any other screen. Fire/Execute enters Active.
- **Active**: cursor border becomes visible. Up/Down/Left/Right move the cursor a cell (crosses month boundaries automatically). Fire/Execute drills into Day-detail. Cancel returns to Passive.
- **Day-detail**: full-screen list of every event on the cursor day, scrollable. Cancel returns to Active.

**Clock-face indicator.** Whenever any alarm slot is enabled, a small red bell appears in the Clock face's header. If a future-firing event is scheduled for later today, its `HH:MM` is drawn in black next to the bell.

**Populate event slots by dropping `ALARMS.ICS` onto the FAT12 partition.** At boot the firmware reads the file, parses each `BEGIN:VEVENT` block (extracting `DTSTART`, `DTEND` and `SUMMARY`), and populates slots 1..N. Slot 0 (the manual recurring alarm) is left untouched. The Bornhack programme export from <https://bornhack.dk/.../program/ics/> works directly — the parser handles `TZID=…:` parameters and CRLF line endings. Import limits: a 4 KiB read buffer (≈15–25 events depending on `SUMMARY` length) and the 31-slot cap, whichever hits first. Re-runs only at boot — edits while running don't take effect until next reboot. An example file (Bornhack 2026 opening + closing) ships at [`assets/to-badge/ALARMS.ICS`](assets/to-badge/ALARMS.ICS).

Imported events live in RAM only — they're not persisted to flash, so a reboot re-imports from the FAT12 partition. Settings → Events lists every populated slot read-only (`<n>: HH:MM MM-DD`) plus two actions: **Quick test +5min** (drops a `Quick test` event 5 minutes from now in the first empty slot — handy for verifying the alarm path without USB; silently no-ops if the wall clock isn't synced or all slots are taken) and a destructive **Clear all** that disables and zeros slots 1..31 immediately. Empty slots are auto-hidden — you only scroll past the events that actually exist.

When the wall clock matches an event's date+time, the buzzer plays the user's currently-selected ringtone (the same Settings → Alarm → Tone choice that recurring slot 0 uses), and the slot auto-disables to prevent re-firing.

### Contacts (Adverts)

> **Full documentation:** [CONTACTS_SCREEN.md](CONTACTS_SCREEN.md) — list view, popup actions, discovery cache, filter picker, detail view.

The **Adverts** screen surfaces every contact you've heard from the mesh in one discovery-sorted list. Saved contacts (in the persistent `ContactStore`) and recently-heard adverts not yet promoted (the in-RAM discovery cache) appear side by side. Live nodes float to the top; a red dot marks contacts heard within the last 5 minutes.

- **Up at row 0** opens a Filter picker (All / Favorites / People / Repeaters / Rooms / Sensors).
- **Fire** opens a per-contact popup with role-aware actions: PM, Info, Save / Unsave (toggles `FLAG_FAVORITE`), Forget (deletes), Add (promotes a discovery row to a saved contact).
- **Info** drills into a per-contact detail view with name, role, last-seen, hop count, key prefix, and GPS (when broadcast).

Visual prefixes: `*` for saved + favorite, `+` for unsaved (discovery) rows.

### Private Messages

> **Full documentation:** [CONTACTS_SCREEN.md](CONTACTS_SCREEN.md#2-pm-inbox--per-peer-threads-fwmeshpm_inbox) — PM inbox + thread layout.

The **Messages** screen is a per-peer inbox with chronological threads. Incoming PMs land in a 32-entry RAM ring (per-boot); outgoing PMs are mirrored alongside replies so threads are two-sided.

- **Inbox view** — one row per distinct peer, sorted by most-recent activity, with `(N)` unread badges.
- **Thread view** — chronological history with `< 3m  …` direction-and-time prefix on the first body line; word-aware wrapping breaks on space boundaries. Up/Down scrolls long threads, Fire opens the on-screen keyboard for a reply.

PM compose is also reachable from the Contacts screen popup → PM action; the same keyboard plumbing serves both entry points.

### Channel browser

Navigate to the **Channels** screen (left/right from the main screen). When no BLE client is connected, the on-device channel browser is shown.

**Channel list** — shows all configured channels with a message count badge (e.g. `#bornhack (3)`). Use Up/Down to scroll, Fire to open a channel. Left/Right switches to adjacent screens.

**Channel view** — shows the channel name and message count in a header bar, followed by the most recent messages (newest at the bottom, filling up to 8 lines). Each message shows the sender name (inverted white-on-black) and the message text wrapped across multiple lines.

Own sent messages are prefixed with `> You`. When a repeater relays your message back, a repeat counter (inverted digit 1-9) appears before the sender name, confirming the message reached the mesh.

**Reply** — press Fire in the channel view to compose a reply using the on-screen keyboard. The message is sent to the currently viewed channel.

When a BLE companion is connected, the channel browser shows "Messages unavailable — BLE client connected" since the companion app handles messaging.

### On-screen keyboard

The text entry system uses a hierarchical joystick-driven keyboard. The screen splits: the top half shows entered text, the bottom half shows the current navigation state.

**Root level** — a center dot with four directional options:

| Direction | Action                                         |
| --------- | ---------------------------------------------- |
| Left      | Letters A-I                                    |
| Up        | Letters J-R                                    |
| Right     | Letters S-Z                                    |
| Down      | Commands (space, backspace, specials, numbers) |
| Fire      | Submit text                                    |
| Cancel    | Discard and exit                               |

**Letter groups** — after choosing a letter range, three sub-groups are available (the opposite direction returns to root). For example, A-I:

| Direction | Letters      |
| --------- | ------------ |
| Down      | a b c        |
| Left      | d e f        |
| Up        | g h i        |
| Right     | Back to root |

**Letter selection** — the group shows 2-3 characters with the middle one highlighted. Use Left/Right to move the highlight, Fire to insert the character.

**Commands** (Down from root):

| Direction | Action                                        |
| --------- | --------------------------------------------- |
| Up        | Back to root                                  |
| Left      | Special characters (`_ . , ( ) * / + - ? #`)  |
| Down      | Space / Backspace (space selected by default) |
| Right     | More options                                  |

**More options** (Right from Commands):

| Direction | Action                                                  |
| --------- | ------------------------------------------------------- |
| Up        | Toggle Shift (types one uppercase letter, then reverts) |
| Down      | Clear all text                                          |
| Right     | Number picker (0-9)                                     |
| Left      | Back to Commands                                        |

When Shift is active, an inverted "S" indicator appears in the keyboard area. The next letter entered will be uppercase, then shift automatically deactivates.

### Emoji rendering

PM threads and the channel browser render a curated set of common MeshCore emoji as **13×13 monochrome bitmaps**. The badge's `FONT_7X13` / `FONT_7X13_BOLD` only cover ISO-8859-1, so without this codepoints like ❤, 👍 or 😂 would otherwise come out as the font's missing-glyph indicator.

Hand-drawn 1-bit bitmaps live in [`src/fw/emoji.rs`](src/fw/emoji.rs) and map ~70 codepoints onto **50 visual archetypes** — close variants alias to the same glyph (e.g. ❤/♥ share one heart, 😂/🤣 share one laugh) since at 13×13 mono there's no perceptible difference. Each emoji claims **2 character cells (14 px advance)** in the monospaced grid, and `text_wrap::word_wrap` walks codepoints — counting emoji as 2 columns and variation selectors as 0 — so soft-wrap boundaries still line up. Unicode variation selectors `U+FE0E` (text style) / `U+FE0F` (emoji style) immediately following a known emoji are silently consumed.

Adding a glyph is a single entry: a 13-row ASCII stencil (`#` = on) packed by the `pack_glyph` const-fn, plus a codepoint → atlas-index row in `EMOJI_LOOKUP`.

### Settings menu

The main menu contains a **Settings** submenu with:

- **Bluetooth** — view BLE device name, enable/disable BLE, clear stored pairings (with confirmation)
- **LoRa** — enable/disable LoRa radio, boost RX gain, radio preset picker (community presets), TX power
- **MeshCore** — set node name, client repeat (with confirmation), adverts (on/off, interval), telemetry sharing (3-state: **No** / **Contacts** / **Yes** — mirrors the MeshCore companion app's "Allow Telemetry Requests" setting; `Contacts` only responds to peers with `Contact.flags` bit 1 set), multi-ACK, path hash length, reset channels, reset contacts
- **Timezone** — UTC offset stepper
- **Factory reset** — wipes all settings and reboots (with confirmation)

Destructive actions (reset channels, reset contacts, factory reset, clear pairings) show a full-screen "Are you sure?" confirmation dialog. The client-repeat toggle also shows a warning when enabling.

## Setup

### Prerequisites

You need:

- **Rust** (stable) with the embedded target
- **probe-rs** for SWD flashing and RTT logging
- **arm-none-eabi-gcc** toolchain (for `arm-none-eabi-size` and `arm-none-eabi-objcopy`)
- **make**
- **dfu-util** (optional, for USB DFU flashing)

### Linux (Ubuntu / Debian)

```bash
# System packages
sudo apt install build-essential gcc-arm-none-eabi libsdl2-dev dfu-util libudev-dev pkg-config

# Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
rustup target add thumbv7em-none-eabihf

# probe-rs
cargo install probe-rs-tools

# udev rules for SWD debug probes (J-Link, ST-Link, CMSIS-DAP, etc.)
curl -o /tmp/69-probe-rs.rules https://probe.rs/files/69-probe-rs.rules
sudo cp /tmp/69-probe-rs.rules /etc/udev/rules.d/
sudo udevadm control --reload-rules && sudo udevadm trigger
```

### Linux (Arch)

```bash
sudo pacman -S base-devel arm-none-eabi-gcc sdl2 dfu-util
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
rustup target add thumbv7em-none-eabihf
cargo install probe-rs-tools
```

### macOS

```bash
# Xcode Command Line Tools — provides make, clang, and the libclang that bindgen needs
xcode-select --install

# System packages (Homebrew); sdl2 is only needed for the simulator
brew install arm-none-eabi-binutils sdl2 dfu-util

# Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
rustup target add thumbv7em-none-eabihf

# probe-rs
cargo install probe-rs-tools
```

> If you already have Rust via Homebrew (`brew install rust`), uninstall it first
> (`brew uninstall rust`). It ships only the host standard library and can't add the
> `thumbv7em-none-eabihf` target, and it conflicts with rustup on PATH — the build fails
> with `error[E0463]: can't find crate for 'core'` if the wrong `cargo` wins.

### Windows

Install the following:

1. **Rust**: download from <https://rustup.rs> and run the installer. Then:

   ```powershell
   rustup target add thumbv7em-none-eabihf
   ```

1. **ARM toolchain**: download the GNU Arm Embedded Toolchain from <https://developer.arm.com/downloads/-/gnu-rm> and add it to your PATH.

1. **probe-rs**:

   ```powershell
   cargo install probe-rs-tools
   ```

1. **make**: install via [chocolatey](https://chocolatey.org/) (`choco install make`) or use the make bundled with Git for Windows.

1. **SDL2** (simulator only): download SDL2 development libraries from <https://github.com/libsdl-org/SDL/releases> and set the `LIBRARY_PATH` environment variable.

1. **WinUSB driver**: probe-rs needs WinUSB for your debug probe. Use [Zadig](https://zadig.akeo.ie/) to install the WinUSB driver for your probe (J-Link, ST-Link, CMSIS-DAP).

### Clone

```bash
git clone --recursive https://codeberg.org/Ranzbak/bornhack-firmware-2026.git
cd bornhack-firmware-2026
```

## Build

### Firmware (nRF52840)

The firmware can be built in four configurations:

| Variant                      | Game | Mesh (LoRa/BLE) | Watch | Use case                                                |
| ---------------------------- | ---- | --------------- | ----- | ------------------------------------------------------- |
| Full (`make fw`)             | yes  | yes             | yes   | Production — everything enabled                         |
| Game only (`make fw-game`)   | yes  | no              | yes   | Game development when full build exceeds flash          |
| Mesh only (`make fw-mesh`)   | no   | yes             | yes   | Mesh/radio development when full build exceeds flash    |
| Watch only (`make fw-watch`) | no   | no              | yes   | Minimal ~130 KiB build for watch-face / clock work only |

The watch face is part of `embassy-base` and is therefore present in every variant. The watch-only build (`embassy-watch` feature) is the smallest configuration that still drives the EPD.

```bash
make fw              # Full debug build (game + mesh + watch)
make fw-release      # Full release build (optimised for size)
make fw-game         # Game + watch (no mesh)
make fw-mesh         # Mesh + watch (no game)
make fw-watch        # Watch only (no game, no mesh)
```

All print flash and RAM usage after building. Release builds use full LTO and `opt-level = "z"` for minimum binary size.

### Flash firmware

Connect a debug probe (J-Link, ST-Link, or CMSIS-DAP) to the badge SWD header, then:

```bash
make flash              # Build + flash full debug firmware (SWD)
make flash-release      # Build + flash full release firmware (SWD)
make flash-game         # Build + flash game-only debug firmware (SWD)
make flash-mesh         # Build + flash mesh-only debug firmware (SWD)
make flash-watch        # Build + flash watch-only debug firmware (SWD)
```

For USB DFU flashing (hold the execute button while powering on to enter DFU mode):

```bash
make dfu-flash           # Debug
make dfu-flash-release   # Release
```

### Debug (VS Code + probe-rs)

Open the project in VS Code and press **F5**. The `cargo fw` build runs automatically before flashing. RTT log output appears in the terminal.

To attach a log monitor to an already-running device:

```bash
make monitor
```

### Simulator

The simulator requires SDL2 (see platform setup above). Build and run:

```bash
make sim
```

![Simulator screenshot](assets/simulator.png)

The simulator renders the full badge UI in a desktop window using SDL2, mirroring the SSD1675 e-paper layout and icon grid.

#### Key bindings

| Key        | Badge button   | Action                         |
| ---------- | -------------- | ------------------------------ |
| Arrow keys | Joystick       | Navigate menus and icon grid   |
| Space      | Joystick fire  | Fire / select highlighted item |
| Enter      | Execute button | Execute / confirm action       |
| Backspace  | Cancel button  | Cancel / close modal           |
| Escape     | —              | Quit simulator                 |

### All make targets

| Command                  | Description                               |
| ------------------------ | ----------------------------------------- |
| `make fw`                | Build full debug firmware (game + mesh)   |
| `make fw-release`        | Build full release firmware               |
| `make fw-game`           | Build game-only debug firmware            |
| `make fw-game-release`   | Build game-only release firmware          |
| `make fw-mesh`           | Build mesh-only debug firmware            |
| `make fw-mesh-release`   | Build mesh-only release firmware          |
| `make fw-watch`          | Build watch-only debug firmware           |
| `make fw-watch-release`  | Build watch-only release firmware         |
| `make flash`             | Build + flash full debug firmware (SWD)   |
| `make flash-release`     | Build + flash full release firmware (SWD) |
| `make flash-game`        | Build + flash game-only firmware (SWD)    |
| `make flash-mesh`        | Build + flash mesh-only firmware (SWD)    |
| `make flash-watch`       | Build + flash watch-only firmware (SWD)   |
| `make dfu-flash`         | Build + flash debug firmware (USB DFU)    |
| `make dfu-flash-release` | Build + flash release firmware (USB DFU)  |
| `make sim`               | Build and run the SDL2 simulator          |
| `make monitor`           | Attach RTT log monitor to running device  |
| `make bl`                | Build bootloader                          |
| `make flash-bl`          | Full-chip erase + flash bootloader        |

## Game engine

> **Looking for how to play?** See [GAME.md](GAME.md) for player-facing
> instructions, controls, and mini-game rules.

The CyberÆgg virtual pet game uses a **delta-T progression engine** that computes stat changes in one step over any time interval, rather than ticking every 10 seconds. The engine predicts the next boundary crossing (where a rate or modifier changes) and sleeps until then — saving battery on the badge while maintaining precise game state.

For stats, actions, traits, lifecycle, and mini-game details, see [GAME.md](GAME.md).

### Tuning

All game balance constants (rates, cooldowns, thresholds) are in a single file:
[`src/game/engine/thresholds.rs`](src/game/engine/thresholds.rs)

The game team can adjust values there without touching engine logic.

### Simulation

Two simulation tools are available for balance testing:

**Rust simulator** (delta-T engine, fast):

```bash
make simulate-game
```

Runs all player profiles (perfect, attentive, casual, absent, night owl, etc.) against the Rust engine and outputs a summary table with lifetime, final stats, and action counts. A 60-day simulation runs in milliseconds thanks to boundary-based scheduling.

**Python simulator** (tick-by-tick reference):

See [`simulation_py/README.md`](simulation_py/README.md) for the original Python balance simulator. The Python and Rust engines should produce similar results for the same player profiles — discrepancies indicate policy or engine bugs.

### Game assets

The badge stores sprite artwork on an external QSPI flash chip formatted as FAT12. When connected via USB, the badge appears as a removable drive where you can drag-and-drop PCX sprite files.

The ready-to-use asset files are in [`assets/to-badge/`](assets/to-badge/). Copy all `.PCX` files from that directory to the badge's USB drive. The [`MANIFEST.TXT`](assets/to-badge/MANIFEST.TXT) in that directory documents the mapping between filenames and animations.

#### Asset file format

Sprites use the **PCX** image format (2 bits per pixel, RLE compressed). The 4-colour palette is:

| Index | Colour      |
| ----- | ----------- |
| 0     | Black       |
| 1     | Red         |
| 2     | White       |
| 3     | Transparent |

PCX files can be opened and edited in **GIMP** (File > Open) or any other image editor that supports the PCX format. When saving, keep the format as 2bpp PCX with the palette above.

Missing animation files fall back to the placeholder sprite (`1E000000.PCX`).

#### Generating assets

The PCX sprite files are generated from PNG sprite sheets using the `aegg-asset-assistant` tool, a sibling repository at <https://codeberg.org/Ranzbak/aegg-asset-assistant>. Each JSON5 config file in `assets/` describes a sprite sheet layout.

To regenerate all PCX files into `assets/to-badge/`:

```bash
cd ../aegg-asset-assistant
cargo run -- export \
    ../bornhack-firmware-2026/assets/bornpets-bartholomeus.json5 \
    ../bornhack-firmware-2026/assets/bornpets-sponsors-cat.json5 \
    ../bornhack-firmware-2026/assets/bornpets-sponsors-slug.json5 \
    ../bornhack-firmware-2026/assets/sponsors.json5 \
    ../bornhack-firmware-2026/assets/bornpets-menu-icons.json5 \
    --output-dir ../bornhack-firmware-2026/assets/to-badge \
    --format pcx
```

This reads the source PNGs referenced in each JSON5 config, slices them into individual frames, and encodes them as 2bpp PCX files with the correct palette. The output filenames match the `PPAAFF.PCX` convention expected by the firmware.

Each JSON5 file contributes a separate `PP` prefix range:

| Config | Prefix | Purpose |
| --- | --- | --- |
| `bornpets-bartholomeus.json5` | `00xx` | Bartholomeus pet animations (formerly "Snail") + shared icons / placeholders |
| `bornpets-sponsors-cat.json5` | `01xx` | Cat pet animations |
| `bornpets-sponsors-slug.json5` | `02xx` | Slug pet animations |
| `sponsors.json5` | `03xx` | First-boot sponsor slideshow images |
| `bornpets-menu-icons.json5` | `04xx` | On-screen menu icons (top + bottom rows, normal + selected) |

After generating, copy all `.PCX` files from `assets/to-badge/` to the badge's USB drive.

#### Changing or adding assets

The firmware maps animation states to filenames in code. If you add new animations, change frame counts, reorder assets, or add a new pet kind, the following source files must be updated to match:

| What changed               | File to update                                                                       |
| -------------------------- | ------------------------------------------------------------------------------------ |
| Frame counts per animation | [`anim_files.rs`](src/game/engine/anim_files.rs) — `SNAIL_FRAMES`, `CAT_FRAMES`      |
| New pet kind               | [`mod.rs`](src/game/engine/mod.rs) — `PetKind` enum + frame table in `anim_files.rs` |
| Animation ID assignment    | [`anim_files.rs`](src/game/engine/anim_files.rs) — `anim_id()` function              |
| Sponsor slide filenames    | [`sponsors.rs`](src/fw/sponsors.rs) — `sponsor_filename()` and `MAX_SPONSORS`        |
| Start screen filename      | [`anim_files.rs`](src/game/engine/anim_files.rs) — `start_screen_filename()`         |
| Menu icons (top/bottom row)| [`anim_files.rs`](src/game/engine/anim_files.rs) — `menu_icon_filename()`, `MENU_ICON_COUNT` |

The filename convention is `PPAAFF.PCX` where `PP` = pet prefix (hex), `AA` = animation ID (hex), `FF` = frame number (hex). This is encoded in `anim_files.rs::build_filename()`.

### Game Architecture

```text
src/game/
  engine/
    mod.rs           — GameState, delta-T update(), next_wake_tick()
    thresholds.rs    — all balance constants
    anim_files.rs    — animation-to-filename lookup table
    to_display.rs    — DisplayAnim enum and state mapping
  lifecycle.rs       — save/restore, game cycle, player actions
  sprite_loader.rs   — PCX image loading from FAT12 flash
  input.rs           — button dispatch
  modal.rs           — in-game modal dialogs
  nav.rs             — icon grid navigation
  tictactoe.rs       — Tic Tac Toe mini-game
  lightsout.rs       — Lights Out mini-game
```

The engine has no dependencies on embassy or hardware — it's pure `no_std` Rust that runs identically on the badge and in host-side tests.

## Recent fixes

- **P2P receive**: incoming private messages now work. Fixed a `src_hash` collision that caused messages from contacts sharing the same `pub_key[0]` to be dropped as flood echoes.
- **P2P echo suppression**: own outgoing messages reflected by flood relays are no longer shown as new incoming messages. Detection uses `PENDING_ACK` hash matching.
- **Path-embedded ACKs**: ACKs piggybacked inside Path packets (`extra_type=3`) are now extracted and processed, matching the reference firmware behavior.
- **Flash error handling**: the ekv flash trait implementation now propagates QSPI errors instead of panicking, allowing ekv to recover from partial writes after power loss.
- **Dependency deduplication**: eliminated duplicate `embassy-time` (0.4 + 0.5) by bumping vendor/ssd1675 to embassy-time 0.5.

## Known issues

### TODO

- Investigate more space efficient SD-card storage method
- Check if sx126x vendored crate is really needed, using another crate might be better
- Add block sender (p2p and channel)
- Check messages accepted when not in contact list

### Meshcore missing / broken features

- Connecting a blank client to the companion, the Contact list is not synchronized
- Administration of repeaters does not work
- Login on repeaters is flaky
- Get repeater status not working
- Login on room servers is not working
- Room server support is absent (not support?)
- Discover routes are not reported in the frontend with contacts when adverts are received
- P2P ACK display in the companion app is unreliable (ACKs are received and processed but not always shown)

### Untested

- 2/3byte path support in messages
