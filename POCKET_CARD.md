# CyberÆgg — Pocket Reference

One-page badge cheat sheet. Print and tuck under the strap.

```
        ┌─────────────────────────────┐
        │      e-paper display        │
        ├─────────────────────────────┤
        │                             │
        │   [UP]                      │
        │ [L][F][R]      [CAN] [EXE]  │
        │   [DN]                      │
        │                             │
        └─────────────────────────────┘
            5-way joystick   2 buttons
            (F = press in)
```

## Buttons

| Button         | Anywhere in the menu       |
| -------------- | -------------------------- |
| **EXE** / Fire | select · open · confirm    |
| **CAN**        | back · cancel · close      |
| **Up / Down**  | move cursor within screen — wraps top ↔ bottom |
| **Left / Right** | switch top-level screen  |

## Top-level screens (Left / Right cycles)

`Game → Main → PMs → Channel → Adverts → Tokens → Clock → Calendar → Name → My QR`

## LED meanings

| Colour                   | Meaning                                     |
| ------------------------ | ------------------------------------------- |
| Pulsing orange           | boot, hardware init                         |
| Pulsing blue             | display + LoRa coming up (~13 s)            |
| Single green flash       | boot done                                   |
| Red flicker              | screen refreshing                           |
| Blue flicker             | USB drive write                             |
| Blinking green           | contacts wipe in progress                   |
| R / G / B one-shot       | someone pinged you via mesh (`blinkme`)     |

## Power-on combos (hold while resetting / connecting USB)

| Hold                                | Result                          |
| ----------------------------------- | ------------------------------- |
| **EXE**                             | USB firmware update (DFU mode)  |
| **EXE + CAN + Fire**                | Factory reset (~40 s, wipes data + settings) |

If app slot is blank the badge auto-enters DFU on its own.

## USB drag-drop (mount the badge)

Plug USB-C. Badge appears as **`CYBR<4 hex>`** drive.

| File you drop                       | What it does                    |
| ----------------------------------- | ------------------------------- |
| `ALARMS.ICS`                        | imports alarms + calendar events |
| `030000.PCX` … `030009.PCX`         | sponsor slides                  |
| `<6 hex>.PCX`                       | game sprite asset               |
| `BORNPETS.CFG`                      | custom pet balance (KEY=VALUE)  |
| `PETS.CFG`                          | add / rename pets (PREFIX=NAME) |
| `LUT.CFG`                           | custom e-paper waveform         |

Reboot the badge after dropping files.

## Firmware update (DFU mode)

```
dfu-util -d 1915:521f -D cyber-aegg.bin
```

Bootloader LEDs in DFU: red blink (idle) → solid blue (flashing) → solid green (done — power-cycle).

## Charging

USB-C in any port = charge. No separate charge LED — battery icon on the watch / status bar shows level. Unplug USB to re-enable BLE pairing.
