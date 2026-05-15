# Factory & dev tooling — `scripts/`

Host-side helpers that automate the badge's factory-floor lifecycle:
SWD bootloader install, DFU firmware push, asset bundle copy.  All three
are event-driven (udev) with polling fallbacks, no third-party Python
deps beyond stdlib.

| Script                | Station            | What it does                                                 |
| --------------------- | ------------------ | ------------------------------------------------------------ |
| `bl_factory.py`       | Assembly (SWD)     | Detects a board on the J-Link jig, erases + flashes bootloader |
| `dfu_factory.py`      | Production (DFU)   | Detects a board in DFU mode, pushes the release App `.bin`     |
| `copy_assets.py`      | Production (MSC)   | Detects a `CYBR*` USB-MSC volume, copies the sprite bundle     |
| `99-cyberaegg.rules`  | one-time install   | udev rules so DFU + USB-MSC + SWD work without sudo            |
| `strip_ics.py`        | dev                | Trims `ALARMS.ICS` calendar exports to fit the 4 KiB read buffer |

---

## One-time host setup (per factory laptop)

### 1. udev rules — sudo-free DFU + MSC access

```bash
sudo cp scripts/99-cyberaegg.rules /etc/udev/rules.d/
sudo udevadm control --reload-rules
sudo udevadm trigger
```

After this, neither `dfu-util` nor `udisksctl` needs `sudo` for badge ops.

### 2. probe-rs udev rules — sudo-free SWD access

Standard probe-rs rules (skip if you've ever flashed via SWD before on this
laptop):

```bash
curl -o /tmp/69-probe-rs.rules https://probe.rs/files/69-probe-rs.rules
sudo cp /tmp/69-probe-rs.rules /etc/udev/rules.d/
sudo udevadm control --reload-rules
sudo udevadm trigger
```

### 3. Build the firmware artifacts once

```bash
# Custom bootloader (small, ~64 KB)
(cd bootloader && cargo bl)

# Release App ELF + .bin (DFU pushes the .bin)
make fw-release
arm-none-eabi-objcopy -O binary \
    target/thumbv7em-none-eabihf/release/embassy \
    target/thumbv7em-none-eabihf/release/embassy.bin
```

Verify:

```bash
ls scripts/*.py scripts/99-cyberaegg.rules
ls bootloader/target/thumbv7em-none-eabihf/release/nrf-aegg-bootloader
ls target/thumbv7em-none-eabihf/release/embassy.bin
```

(`dfu_factory.py` will auto-run `make fw-release` + objcopy at startup if the
release `.bin` is missing.)

---

## Hardware setup

### Station 1 — SWD assembly bench

* J-Link / ST-Link / CMSIS-DAP probe on a USB port.
* Pogo-pin jig wired to the badge's SWD header (SWCLK, SWDIO, GND, VTref).
* USB-C cable to power the badge during flash — the probe alone doesn't
  power the target.

### Station 2 — DFU + factory-test + MSC bench

* USB hub (4-port or more if you want parallel asset copy).
* USB-C cables, one per port.
* Two terminals on the same laptop, both with the firmware checkout:
  one runs `dfu_factory.py`, the other runs `copy_assets.py -j 4`.

---

## Visible bench feedback (badge-side)

| Badge state                  | E-paper                                | LED              | Audible (host) |
| ---------------------------- | -------------------------------------- | ---------------- | -------------- |
| Test running                 | Factory-test grid painting             | Boot breadcrumbs | —              |
| All PASS, ship image visible | `BORNHACK 2026 / Factory Tested / Ready` | **Green pulsing** | —              |
| Asset copy completed         | Ship image (unchanged)                 | Green pulsing    | `\a` BEL       |
| Test FAIL — needs rework     | `NEEDS REWORK` footer + row results    | **Red pulsing**  | —              |

---

## Per-board workflow

### Station 1 — install bootloader (~3 s per board)

Start the watcher and leave it running:

```bash
scripts/bl_factory.py
```

It prints `Place a board on the SWD jig; lift after DONE.` and waits.

For each fresh board:

1. **Press** the board onto the jig (USB-C also plugged in for power).
2. Watch the script:
   ```
   BOARD detected.
     erase…       0.6s
     bootloader…  2.4s
     reset…       3.2s total ✓
   ```
3. **Lift** the board off the jig. Script prints `Board lifted — ready for next.`
4. Repeat with the next board.

End state: bootloader at `0x00000000`, app region empty.

#### Flags

| Flag           | Effect                                                                  |
| -------------- | ----------------------------------------------------------------------- |
| `--once`       | Exit after one successful flash (handy for first-board sanity check).   |
| `--probe ID`   | Pick a specific J-Link in a multi-probe bank (`VID:PID:SERIAL`).         |
| `--with-app`   | Also SWD-flash the App.  Defaults to release; pair with `--debug` for dev. |
| `--debug`      | Use the debug App ELF instead of release (with `--with-app`).            |

### Station 2 — DFU app + factory test + assets

#### Once per shift: start both watchers

Terminal A — the DFU flasher:

```bash
scripts/dfu_factory.py
```

Terminal B — the asset copier:

```bash
scripts/copy_assets.py -j 4
```

(Both terminals stay open all day.  `dfu_factory.py` defaults to the release
build; pass `--debug` if you want the debug build with RTT support.)

#### Per board

1. **Hold the Execute button** on the badge.
2. Plug USB-C into the board.  Keep Execute held for ~1 second, then release.
   Bootloader is now in DFU mode (red LED blinks slowly).
3. **Terminal A** (`dfu_factory.py`) detects the DFU device and runs
   `dfu-util` automatically.  Output:
   ```
   FRESH  DFU device  serial=0000XXXX
     → DFU flashed in 33.5s ✓
   ```
4. Bootloader auto-resets out of DFU; App boots.
5. **E-paper** paints `FACTORY TEST` header, then the 8-row 2-column grid
   (~8 s).  All PASS → `ALL PASS - shipping` footer → ship image takes over.
   Green LED starts pulsing.
6. **Terminal B** (`copy_assets.py`) prints:
   ```
   FRESH CYBRA3F7  (/dev/sdX1)
     [CYBRA3F7]   0E000000.PCX
     ...
     [CYBRA3F7] → 157 files, 642 KiB copied + flushed in 1.3s
   DONE  CYBRA3F7  ✓        ← + BEL beep
   ```
7. Once DONE has printed, **unplug** the board.  Ship image stays on the
   e-ink; assets are in QSPI; KV stamps factory-test-passed.  Pack.

### What if it FAILs?

* E-paper shows the test-row grid with one or more `FAIL` columns.
* Footer reads `NEEDS REWORK` instead of `ALL PASS - shipping`.
* **Red LED pulses** (bench signal: pull this one aside).
* No KV stamp, no ship image.  Next power-on re-runs the test from scratch
  so post-rework retesting needs zero extra steps.

---

## Parallel batches with a USB hub

`copy_assets.py -j N` dispatches each udev `add` event to its own worker
thread, so multiple badges plugged into a hub process concurrently:

| Hub ports plugged in | Wall-clock for asset copy |
| -------------------- | ------------------------- |
| 1                    | ~1.3 s                    |
| 4                    | ~1.5 s (4 in parallel)    |
| 8                    | ~1.8 s                    |

(Limited by USB bus bandwidth + host filesystem I/O, not the script.)

The DFU step is still sequential per port — `dfu-util` only addresses one
device at a time.  For maximum throughput **stagger the DFU flashes**: with
two free hands, plug board 2 with Execute held while board 1's
`dfu_factory.py` flash is still streaming.  Factory-test + asset-copy
phases overlap naturally across boards.

---

## Failure recovery

### "Bootloader present but App won't DFU"

Symptom: `dfu_factory.py` says `DFU FAILED`, or `dfu-util --list` is empty.

* Make sure the udev rule is installed (`scripts/99-cyberaegg.rules` in
  `/etc/udev/rules.d/`).
* Re-run `sudo udevadm control --reload-rules && sudo udevadm trigger`.
* Replug the board with Execute held the whole time.  Common error:
  releasing Execute too early — count to 2 with Execute held *after* the
  cable is in.

### "Factory test never starts, ship image already shown"

KV says the badge already passed.  Either:

* This is a re-test after a successful pass — expected, factory test is
  skipped by design.
* The badge needs a full QSPI wipe to re-test.  Hold **Execute + Cancel +
  Fire all three** while plugging in.  Bootloader formats QSPI (wipes
  `hwtest:passed` *and* sprite assets), then resets.  Release Cancel + Fire
  but keep Execute → bootloader drops into DFU mode → `dfu_factory.py`
  catches the event.

### "Asset copy never fires"

* Check the badge shows ship image *and* the green LED pulses.  If yes,
  the App is running USB-MSC.
* Check `lsblk` for a CYBR-labelled partition.  If missing, badge isn't
  enumerating — likely HFXO crystal isn't running (USB needs HFXO for the
  48 MHz domain).  Re-test via wipe + re-flash; if still missing, send for
  reflow.
* If `lsblk` shows it but `copy_assets.py` doesn't react: udev rule may
  not match — `udevadm info /dev/sdX1` should show `ID_FS_LABEL=CYBRxxxx`.

### "All tests PASS but I want to redo the asset copy"

Wipe via Execute + Cancel + Fire, re-DFU, re-test.  Faster than nudging
through firmware state.

---

## What the factory test actually checks

Eight explicit checks on the on-board hardware + two implicit (would have
hung earlier on failure):

| Code | Test                       | What it catches                                        |
| :--: | -------------------------- | ------------------------------------------------------ |
| 22   | HFXO 32 MHz crystal start  | Bad solder joint on the 32 MHz crystal                  |
| 21   | LFXO 32.768 kHz crystal    | Same for the slow-clock crystal                        |
| 9    | Battery voltage (SAADC)    | ADC sample of `vbat` divider in the 2.5–5.0 V range    |
| 20   | Buzzer pin pull-down       | P0_13 idles low via the PCB 1 MΩ pull-down             |
| 11   | QWIIC SDA pull-up          | I²C SDA reads high via external pull-up                |
| 12   | QWIIC SCL pull-up          | I²C SCL reads high via external pull-up                |
| —    | Die temperature            | Internal nRF52 TEMP sensor in 0–60 °C range            |
| 8    | LoRa SX1262 `GetStatus`    | SPI handshake to the radio, valid mode bits returned   |
| 10 (implicit) | QSPI flash JEDEC ID | `flash::init` panics on bad QSPI before reaching the gate |
| 13–18 (implicit) | EPD pins        | `init_epd` hangs / errors on RESET/DC/CSN/SCK/MOSI fail |

Each numeric code matches `bin/hwtest.rs::ERR_*` and the table in
[`../HWTEST.md`](../HWTEST.md) — a tech who knows the beep codes from the
standalone hwtest firmware knows them here too.

---

## SWD-only fallback (no DFU station)

For very small batches or first-board bring-up you can skip the DFU station
entirely by passing `--with-app` to `bl_factory.py`:

```bash
scripts/bl_factory.py --with-app
```

Adds ~75 s per board for the SWD App flash (release ELF), but only one
machine needed.  The factory test + asset-copy steps from Station 2 still
apply afterwards (via USB hub for asset copy).

---

## Where to look when something's weird

| Symptom                            | First thing to look at                            |
| ---------------------------------- | ------------------------------------------------- |
| Factory test never runs            | KV stamp from prior boot — wipe + re-flash        |
| Ship image overwritten             | KV stamp from prior boot — same                   |
| LEDs all dark                      | App isn't running — DFU flash didn't take         |
| Red LED pulsing                    | Read the e-paper rows — one is `FAIL`             |
| Green LED pulsing, no copy         | Host udev rule, `dmesg` for `usb-storage` errors  |
| `bl_factory.py` never sees a board | Probe is wedged — `udevadm trigger`; replug       |
| `dfu_factory.py` never sees DFU    | Execute not held long enough; replug holding it   |

---

## Cross-references

* [`../HWTEST.md`](../HWTEST.md) — standalone `bin/hwtest.rs` factory test
  firmware (single-flash dev tool); its `ERR_*` table is the source of
  truth for beep codes that the integrated factory_test mirrors.
* [`../README.md`](../README.md) — overall project overview, build/flash
  basics, badge architecture.
* [`../src/fw/factory_test.rs`](../src/fw/factory_test.rs) — the on-badge
  boot supervisor + visible test grid.
* [`../src/bin/embassy.rs`](../src/bin/embassy.rs) — App entry, includes
  the hoisted USB-MSC spawn that runs during factory_test's halt loop.
* [`../bootloader/`](../bootloader/) — the custom DFU bootloader source.
* [`../assets/to-badge/`](../assets/to-badge/) — asset bundle
  `copy_assets.py` reads from.
