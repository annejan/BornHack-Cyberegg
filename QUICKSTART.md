# CyberÆgg — Quickstart

Welcome to the BornHack 2026 CyberÆgg badge. This guide gets you from "unbox" to "actually playing with it" in a few minutes. For the one-page reference see [POCKET_CARD.md](POCKET_CARD.md).

## What's in the badge

- nRF52840 microcontroller (BLE + USB)
- 1.5" tri-colour e-paper display (black / red / white)
- LoRa SX1262 mesh radio (MeshCore network)
- NFC antenna on the back
- Piezo buzzer, RGB LED, 5-way joystick + 2 buttons
- Li-ion battery, USB-C for power + data

## First power-on

A fresh badge runs a built-in factory self-test on the very first boot. You see a `FACTORY TEST` screen with a small PASS/FAIL grid, then `ALL PASS — shipping`. After that the badge goes straight to the app on every boot (the test never runs again unless the firmware is wiped).

Sequence of LEDs at every boot:

1. Pulsing **orange** — hardware init
2. Pulsing **blue** — display + LoRa coming up (about 13 seconds)
3. Single **green** flash — ready

You land on the **Main** screen. Push **Left / Right** to flip through the top-level screens (see below).

## Controls

Two thumb buttons on the right and a 5-way joystick on the left:

- **EXE** (Execute) or **Fire** (joystick press) — select / activate
- **CAN** (Cancel) — back / dismiss
- **Up / Down** — move cursor inside the current screen
- **Left / Right** — switch to the next top-level screen

## Top-level screens

The badge is a carousel. Left/Right cycles through ten screens:

| Screen        | What it is                                            |
| ------------- | ----------------------------------------------------- |
| **Game**      | BornPets — virtual pet, mini-games, hatchery          |
| **Main**      | Root menu: Bornagotchi · Settings · About             |
| **PMs**       | Private mesh messages inbox                           |
| **Channel**   | Group / room mesh chat                                |
| **Adverts**   | Recently heard mesh adverts                           |
| **Tokens**    | Collected NFC tokens                                  |
| **Clock**     | Digital / analog watch face + alarm                   |
| **Calendar**  | Month grid + per-day timeline                         |
| **Name**      | Big conference-badge name view                        |
| **My QR**     | Your mesh identity QR (share with other badges)       |

Per-app user guides:

- [USER_WATCH.md](USER_WATCH.md) — watch face, alarms, calendar, time sync
- [USER_GAMES.md](USER_GAMES.md) — BornPets and the seven mini-games
- [USER_MESH.md](USER_MESH.md) — LoRa mesh, adverts, channels, PMs
- [USER_CONTACTS.md](USER_CONTACTS.md) — contacts list, saving, blocking
- [USER_NFC.md](USER_NFC.md) — NFC tap interactions with BadgeCtl

## Pair with the MeshCore app

The badge speaks the MeshCore companion protocol over Bluetooth. Install **MeshCore** on Android / iOS, or open `https://app.meshcore.nz/` in Chrome / Edge.

1. Power the badge with USB **un**plugged (BLE turns off while USB is connected).
2. In the app, scan for devices. Yours advertises as **`Cyber Ægg XXYY`** (XXYY is unique per badge — 4 hex characters of the chip ID).
3. The phone shows a passkey prompt. The badge shows a 6-digit passkey on its display. Type that number in the phone.
4. Bonded. The app can now set the wall clock, manage contacts, send / receive messages, change LoRa preset, etc.

## Set the time

The badge has no battery-backed RTC — the clock resets to "not set" on every reboot. There are two ways to set it:

- **MeshCore app** — connect via BLE, the app pushes its phone time to the badge.
- **Stand near a synced repeater** — a known-good mesh repeater advertises its time periodically; your badge picks it up automatically.

Set the timezone once in **Main → Settings → Timezone**. That setting persists.

## Charging

Plug any USB-C cable into the badge. Charge LED shows on the on-screen battery icon (the badge does not have a dedicated charge LED). USB-connected = BLE off; unplug when you want pairing back.

## USB drag-drop

When the badge is plugged in via USB-C it appears on your computer as a small drive labelled **`CYBR<4 hex>`**. You can drop these files in the root:

| File                          | Effect                                                 |
| ----------------------------- | ------------------------------------------------------ |
| `ALARMS.ICS`                  | iCalendar file: imports alarms and calendar events     |
| `030000.PCX` … `030009.PCX`   | Sponsor slides shown on the splash carousel            |
| `<6 hex>.PCX`                 | Game sprites (palette + size enforced by asset tool)   |
| `BORNPETS.CFG`                | Override BornPets balance — see [USER_GAMES.md](USER_GAMES.md) |

Reboot the badge after dropping files (re-plug or hold power if no power switch — pull the strap and replug USB).

## Firmware update

Hold **EXE** while plugging in USB. The badge enters bootloader DFU mode (red blink on the LED). Send the new image:

```
dfu-util -d 1915:521f -D cyber-aegg.bin
```

The LED goes solid blue while flashing, then solid green when done. Power-cycle.

If the badge appears blank (no display) on power-up it auto-enters DFU on its own — just send the firmware.

## Factory reset

Hold **EXE + CAN + Fire** all together while plugging in USB. Red LED blinks rapidly for about 40 seconds while the on-board flash is erased. The badge then reboots into DFU mode (because the app slot is now blank).

This wipes:

- All settings (timezone, mesh nickname, LoRa preset)
- Contacts cache and PM inbox
- BornPets pet save
- Imported calendar (`ALARMS.ICS`)
- Sponsor and sprite PCX files

The factory test runs again on the next boot.

## Troubleshooting

- **Badge won't wake / display stays blank.** Plug in USB. If the LED never blinks, hold EXE while plugging in and re-flash via `dfu-util`.
- **Clock keeps resetting.** Expected. The RTC has no battery; pair via MeshCore once per boot or wait for a mesh time advert.
- **BLE not visible.** USB is plugged in. BLE is disabled whenever USB is connected. Unplug.
- **Alarm never fires.** Clock hasn't been set yet this boot. See "Set the time" above.
- **No mesh peers showing up.** Walk around — LoRa range varies. Or set a wrong LoRa preset; see **Main → Settings → LoRa Radio**, must match the rest of the local mesh.

## Where to file bugs

`https://codeberg.org/Ranzbak/bornhack-firmware-2026/issues`
