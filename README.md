# BornHack CyberÆgg Badge Firmware

Embassy-based async firmware for the BornHack CyberÆgg badge (nRF52840).

## Hardware

| Component  | Part              | Interface      |
| ---------- | ----------------- | -------------- |
| MCU        | nRF52840          | —              |
| Display    | SSD1675 e-paper   | SPI3           |
| LoRa radio | SX1262            | SPI2           |
| BLE        | nRF52840 built-in | nrf-sdc / MPSL |

## Architecture

The firmware runs three concurrent Embassy tasks:

- **BLE task** — GATT peripheral exposing Nordic UART Service (NUS). Speaks the MeshCore companion protocol; handles all commands from the companion app and pushes async notifications.
- **MeshCore task** — drives the SX1262 in continuous RX. Receives/transmits MeshCore packets (adverts, private messages, channel messages, trace-path, login). Forwards received packets to the BLE task via channels.
- **Display task** — renders UI screens to the SSD1675 e-paper display.

### Bootloader

`embassy-boot-nrf` replaces the factory Adafruit UF2 bootloader.\
The `bootloader/` directory is a standalone Cargo project (not in the workspace, not tracked in git).

Flash partition layout:

| Region       | Start        | End          | Size  |
| ------------ | ------------ | ------------ | ----- |
| Bootloader   | `0x00000000` | `0x0000BFFF` | 48 K  |
| State        | `0x0000C000` | `0x0000CFFF` | 4 K   |
| Active (app) | `0x0000D000` | `0x00084FFF` | 480 K |
| DFU          | `0x00085000` | `0x000FEFFF` | 480 K |

The main app's `memory.x` sets `FLASH ORIGIN = 0x0000D000`.

### Vendor libraries

| Library              | Location                     | Notes                                                                                                                 |
| -------------------- | ---------------------------- | --------------------------------------------------------------------------------------------------------------------- |
| `meshcore`           | `vendor/meshcore/`           | MeshCore packet codec (no_std)                                                                                        |
| `meshcore-companion` | `vendor/meshcore-companion/` | BLE companion protocol encoder/decoder                                                                                |
| `ssd1675`            | `vendor/ssd1675/`            | Async Embassy SSD1675 driver with OTP LUT readback, variant detection (A/B), `UpdateMode`, `BorderWaveform`, fast LUT |

## Connecting with MeshCore

The badge is compatible with the MeshCore companion app, available for Android/iOS and as a web app.

When the badge boots, it begins advertising over BLE. On first pairing a numeric passkey is shown on the e-paper display — enter this in the app to complete the bond.

Once connected, the MeshCore app gives full control of the LoRa mesh side of the firmware:

- View and message contacts discovered over LoRa
- Send and receive channel messages
- Manage stored contacts and routing paths
- Adjust radio parameters (frequency, bandwidth, spreading factor, TX power)
- Run path traces (ping) to nearby nodes
- Monitor incoming advertisements and ACKs in real time

The badge appears in the app as a standard MeshCore node. All mesh activity (received messages, adverts, ACKs) is pushed to the app as BLE notifications without polling.

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
git clone --recursive <your-repo-url>
cd cyberegg/embedded_graphics/hello-graphics
```

## Build

### Firmware (nRF52840)

The firmware can be built in three configurations:

| Variant                    | Game | Mesh (LoRa/BLE) | Use case                                             |
| -------------------------- | ---- | --------------- | ---------------------------------------------------- |
| Full (`make fw`)           | yes  | yes             | Production — everything enabled                      |
| Game only (`make fw-game`) | yes  | no              | Game development when full build exceeds flash       |
| Mesh only (`make fw-mesh`) | no   | yes             | Mesh/radio development when full build exceeds flash |

```bash
make fw              # Full debug build (game + mesh)
make fw-release      # Full release build (optimised for size)
make fw-game         # Game only (no mesh)
make fw-mesh         # Mesh only (no game)
```

All print flash and RAM usage after building. Release builds use full LTO and `opt-level = "z"` for minimum binary size.

### Flash firmware

Connect a debug probe (J-Link, ST-Link, or CMSIS-DAP) to the badge SWD header, then:

```bash
make flash              # Build + flash full debug firmware (SWD)
make flash-release      # Build + flash full release firmware (SWD)
make flash-game         # Build + flash game-only debug firmware (SWD)
make flash-mesh         # Build + flash mesh-only debug firmware (SWD)
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
| `make flash`             | Build + flash full debug firmware (SWD)   |
| `make flash-release`     | Build + flash full release firmware (SWD) |
| `make flash-game`        | Build + flash game-only firmware (SWD)    |
| `make flash-mesh`        | Build + flash mesh-only firmware (SWD)    |
| `make dfu-flash`         | Build + flash debug firmware (USB DFU)    |
| `make dfu-flash-release` | Build + flash release firmware (USB DFU)  |
| `make sim`               | Build and run the SDL2 simulator          |
| `make monitor`           | Attach RTT log monitor to running device  |
| `make bl`                | Build bootloader                          |
| `make bl-flash`          | Full-chip erase + flash bootloader        |

## Game engine

The CyberÆgg virtual pet game uses a **delta-T progression engine** that computes stat changes in one step over any time interval, rather than ticking every 10 seconds. The engine predicts the next boundary crossing (where a rate or modifier changes) and sleeps until then — saving battery on the badge while maintaining precise game state.

### Stats

Five primary stats (u16, 0 = best, 65535 = worst):

| Stat          | Fills in         | What makes it worse                             | What helps              |
| ------------- | ---------------- | ----------------------------------------------- | ----------------------- |
| **Hunger**    | ~20 hours        | Time, miserable boost                           | Feed action             |
| **Tired**     | ~13 hours        | Time, miserable boost                           | Sleep (tiered recovery) |
| **Drained**   | Interval-based   | Activity, miserable boost                       | Relax action, sleep     |
| **Sick**      | ~7.6 days (base) | Time + condition decay when other stats are bad | Heal action             |
| **Miserable** | Interval-based   | Multiple stats above 60%                        | Play action (zeroes it) |

Stats interact through feedback loops: high miserable boosts hunger/tired/drained decay rates, bad hunger/tired/drained trigger accelerated sick decay, and multiple bad stats increase miserable's growth rate.

### Traits

Each pet hatches with randomized traits (25%–75% range):

- **Vitality** — determines initial sick level (higher = healthier start)
- **Curiosity** — reduces play action costs (higher = cheaper to play)
- **Resilience** — reserved for future use

### Actions

| Action    | Duration     | Cooldown | Effect                                        |
| --------- | ------------ | -------- | --------------------------------------------- |
| **Feed**  | 2 ticks      | 12 ticks | Reduces hunger and drained                    |
| **Heal**  | 3 ticks      | 24 ticks | Reduces sick                                  |
| **Relax** | 2 ticks      | 24 ticks | Reduces drained (costs hunger)                |
| **Play**  | 4 ticks      | 48 ticks | Zeroes miserable (costs hunger/tired/drained) |
| **Sleep** | Until rested | —        | Tiered tired recovery, drained recovery       |

Actions are mutually exclusive. During an action and its cooldown, the corresponding stat's decay is suppressed.

### Lifecycle

1. **Hatching** — 30 ticks (5 minutes), then active
1. **Active** — stats decay, player manages with actions
1. **Leaving** — triggered when stats max out for too long (1–4 maxed stats = 20h–2h countdown)
1. **Gone** — pet has left, new egg starts

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

### Game Architecture

```text
src/game/
  engine/
    mod.rs           — GameState, delta-T update(), next_wake_tick()
    thresholds.rs    — all balance constants
  sprite_loader.rs   — PCX image loading from FAT12 flash
  input.rs           — button dispatch
  modal.rs           — in-game modal dialogs
  nav.rs             — icon grid navigation
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
