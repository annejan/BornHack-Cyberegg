# Factory Flashing & Test Playbook

End-to-end workflow for flashing, testing, and provisioning CyberÆgg badges on a
production line.  Two stations, both Linux laptops with one shared firmware
checkout.  The work in [PR #47](https://codeberg.org/Ranzbak/bornhack-firmware-2026/pulls/47)
turns the previous "two-step `make flash-bl` + `make flash`" workflow (~107 s
per board) into:

* **SWD station**: ~3 s to install the bootloader.
* **DFU station**: ~33 s firmware push + ~10 s factory test + ~3 s asset copy
  = ~50 s end-to-end *per board*, with **parallelism via a USB hub** dropping
  the wall-clock cost per badge proportionally.

Visible bench signalling at every stage:

| State                          | E-paper                                  | LED                  | Audible (host)    |
| ------------------------------ | ---------------------------------------- | -------------------- | ----------------- |
| Test running                   | Factory-test grid painting               | Boot breadcrumb LEDs | —                 |
| All PASS, ship image visible   | `BORNHACK 2026 / Factory Tested / Ready` | **Green pulsing**    | —                 |
| Asset copy done                | Ship image (unchanged)                   | Green pulsing        | `\a` BEL          |
| Test FAIL — needs rework       | `NEEDS REWORK` footer + per-row results  | **Red pulsing**      | —                 |

---

## One-time host setup (per factory laptop)

### 1. udev rules — sudo-free DFU and USB-MSC access

```bash
sudo cp scripts/99-cyberaegg.rules /etc/udev/rules.d/
sudo udevadm control --reload-rules
sudo udevadm trigger
```

After this, neither `dfu-util` nor `udisksctl` needs `sudo` for badge ops on
the logged-in session.

### 2. probe-rs udev rules — sudo-free SWD access

Standard probe-rs rules (almost certainly already installed if you've ever
flashed via SWD on this laptop):

```bash
curl -o /tmp/69-probe-rs.rules https://probe.rs/files/69-probe-rs.rules
sudo cp /tmp/69-probe-rs.rules /etc/udev/rules.d/
sudo udevadm control --reload-rules
sudo udevadm trigger
```

### 3. Build the firmware artifacts once

```bash
# Custom bootloader (the one bl_factory.py flashes)
(cd bootloader && cargo bl)

# App ELF + raw .bin for DFU
make fw
arm-none-eabi-objcopy -O binary \
    target/thumbv7em-none-eabihf/debug/embassy \
    target/thumbv7em-none-eabihf/debug/embassy.bin
```

Verify:

```bash
ls scripts/{bl_factory,copy_assets}.py scripts/99-cyberaegg.rules
ls bootloader/target/thumbv7em-none-eabihf/release/nrf-aegg-bootloader
ls target/thumbv7em-none-eabihf/debug/embassy.bin
```

---

## Hardware setup

### Station 1 — SWD assembly bench

* J-Link / ST-Link / CMSIS-DAP probe on a USB port.
* Pogo-pin jig wired to the badge's SWD header (SWCLK, SWDIO, GND, VTref).
* USB-C cable to power the badge during flash (the probe alone doesn't power
  the target — VTref needs to read a real voltage).

### Station 2 — DFU + factory-test bench

* USB hub (4-port or more if you want parallel asset copy).
* USB-C cables, one per port.
* The laptop runs `scripts/copy_assets.py` in a persistent terminal all shift.
* Optional: a power-only USB-C port on a second hub for badges being
  factory-test'ed without the DFU laptop, e.g. while you wait for assets to
  finish.

---

## Per-board workflow

### Station 1 — install bootloader (~3 s per board)

Start the watcher once and leave it running all shift:

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
3. **Lift** the board off the jig.  Script prints `Board lifted — ready for next.`
4. Repeat with the next board.

State at end of station 1: bootloader at `0x00000000`, app region empty.

#### Options

| Flag           | Effect                                                                  |
| -------------- | ----------------------------------------------------------------------- |
| `--once`       | Exit after one successful flash (handy for first-board sanity check).   |
| `--probe ID`   | Pick a specific J-Link in a multi-probe bank.  Format: `VID:PID:SERIAL`. |
| `--with-app`   | Also SWD-flash the debug App.  Slower but skips needing DFU later.       |

### Station 2 — DFU app + factory test + asset copy

#### Once per shift: start the asset-copy watcher

In a terminal you'll leave open all day:

```bash
scripts/copy_assets.py -j 4
```

`-j N` sets the parallel-worker count.  Match it to the number of USB-hub
ports you'll use concurrently; default 8.

#### Per board

1. **Hold the Execute button** on the badge.
2. Plug USB-C into the board.  Keep Execute held for ~1 second after the
   cable is in, then release.  Bootloader is now in DFU mode (red LED blinks
   slowly).
3. From a second terminal:
   ```bash
   make dfu-flash
   ```
   ~33 s.  Bootloader auto-resets out of DFU into the App when done.
4. Watch the **e-paper** paint:
   ```
        FACTORY TEST
   HFXO  PASS    VBAT  PASS
   LFXO  PASS    TEMP  PASS
   SDA   PASS    BUZZ  PASS
   SCL   PASS    LORA  PASS

   ALL PASS - shipping
   ```
   then the ship image takes over.  Green LED starts pulsing.
5. In the **`copy_assets.py` terminal** you'll see:
   ```
   FRESH CYBRA3F7  (/dev/sdc1)
     [CYBRA3F7]   0E000000.PCX
     [CYBRA3F7]   0E000100.PCX
     …
     [CYBRA3F7] → 157 files, 642 KiB copied + flushed in 1.3s
   DONE  CYBRA3F7  ✓        ← + BEL beep
   ```
6. Once DONE has printed, **unplug** the board.  Ship image stays on the
   e-ink; assets are in QSPI; KV says factory-test-passed.  Pack.

### What if it FAILs?

* E-paper shows the test-row grid with one or more `FAIL` columns.
* Footer reads `NEEDS REWORK` instead of `ALL PASS - shipping`.
* **Red LED pulses** (bench signal: pull this one aside).
* No KV stamp, no ship image.  Next power-on re-runs the test from scratch
  so post-rework retesting needs zero extra steps.

---

## Parallel batches with a USB hub

The big throughput win.  `copy_assets.py -j N` dispatches each udev `add`
event to its own worker thread, so:

| Hub ports plugged in | Wall-clock for asset copy |
| -------------------- | ------------------------- |
| 1                    | ~1.3 s                    |
| 4                    | ~1.5 s (4 in parallel)    |
| 8                    | ~1.8 s                    |

(Limited by USB bus bandwidth + host filesystem I/O, not the script.)

The DFU step is still sequential per port — `dfu-util` only addresses one
device at a time, and the bootloader uses fixed USB descriptors so you can't
multiplex.  For maximum throughput **stagger the DFU flashes**: start
`make dfu-flash` for board 1, then while it's running plug board 2 into the
next port (hold Execute again), start `make dfu-flash` for board 2 once the
first finishes, etc.  Factory-test + asset-copy phases naturally overlap
across boards because they're driven by the badge + host script.

---

## Failure recovery

### "Bootloader present but App won't DFU"

Symptom: `dfu-util` says `Cannot open DFU device` or hangs.

* Make sure the udev rule is installed (`scripts/99-cyberaegg.rules` in
  `/etc/udev/rules.d/`).
* Re-run `sudo udevadm control --reload-rules && sudo udevadm trigger`.
* Replug the board with Execute held the whole time.

### "Badge boots straight into normal mode, no DFU"

You let go of Execute too early.  Unplug, hold Execute, replug, count to 2,
*then* release.

### "Factory test never starts, ship image already shown"

KV says the badge already passed.  Either:

* This is a re-test after a successful pass — expected, factory test is
  skipped by design.
* The badge needs a full QSPI wipe to re-test.  Hold **Execute + Cancel +
  Fire all three** while plugging in.  Bootloader formats QSPI (wipes
  `hwtest:passed` *and* sprite assets), then resets.  Release Cancel + Fire
  but keep Execute → bootloader drops into DFU mode → re-flash via
  `make dfu-flash`.

### "Asset copy never fires"

* Check the badge shows ship image *and* the green LED pulses.  If yes, the
  App is running USB-MSC.
* Check `lsblk` for a CYBR-labelled partition.  If missing, badge isn't
  enumerating — likely HFXO crystal isn't running (USB needs HFXO for the
  48 MHz domain).  Re-test via wipe + re-flash; if still missing, send for
  reflow.
* If `lsblk` shows it but `copy_assets.py` doesn't react: udev rule may not
  match — `udevadm info /dev/sdX1` should show `ID_FS_LABEL=CYBRxxxx`.

### "All tests PASS but I want to redo the asset copy"

Workflow doesn't currently support this on the same boot.  Wipe via
Execute + Cancel + Fire, re-flash, re-test.  Faster than nudging through
the firmware.

---

## What does the factory test actually check?

Eight explicit checks on the on-board hardware + two implicit (would have
hung earlier on failure):

| Code | Test                       | What it catches                                        |
| :--: | -------------------------- | ------------------------------------------------------ |
| 22   | HFXO 32 MHz crystal start  | Bad solder joint on the 32 MHz crystal (this badge's repaired failure mode) |
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
[`HWTEST.md`](HWTEST.md) so a tech who knows the beep codes from the
standalone hwtest firmware knows them here too.

---

## SWD-only fallback (no DFU station)

For very small batches or first-board bring-up you can skip the DFU station
entirely by passing `--with-app` to `bl_factory.py`:

```bash
scripts/bl_factory.py --with-app
```

Adds ~100 s per board for the SWD App flash, but you only need one machine.
The factory test + asset-copy steps from Station 2 still apply afterwards
(via USB hub for asset copy).

---

## Where to look when something's weird

| Symptom                    | First thing to look at                            |
| -------------------------- | ------------------------------------------------- |
| Factory test never runs    | KV stamp from prior boot — wipe + re-flash        |
| Ship image overwritten     | KV stamp from prior boot — same                   |
| LEDs all dark              | App isn't running — DFU flash didn't take         |
| Red LED pulsing            | Read the e-paper rows — one is `FAIL`             |
| Green LED pulsing, no copy | Host udev rule, `dmesg` for `usb-storage` errors  |
| `bl_factory.py` never sees a board | Probe is wedged — `udevadm trigger`; replug |

---

## Files referenced

| Path                                  | Purpose                                             |
| ------------------------------------- | --------------------------------------------------- |
| `scripts/bl_factory.py`               | Station 1: SWD bootloader auto-flasher              |
| `scripts/copy_assets.py`              | Station 2: asset bundle auto-copier                 |
| `scripts/99-cyberaegg.rules`          | Sudo-free dev access to DFU + MSC                   |
| `src/fw/factory_test.rs`              | The on-badge boot supervisor + visible test grid    |
| `src/bin/embassy.rs`                  | App entry, includes hoisted USB MSC spawn           |
| `bootloader/`                         | The custom DFU bootloader source                    |
| `assets/to-badge/`                    | Asset bundle the copy script reads from             |

---

## See also

* [HWTEST.md](HWTEST.md) — the standalone `bin/hwtest.rs` factory test
  firmware (single-flash dev tool); its `ERR_*` table is the source of truth
  for beep codes that the integrated factory_test in this PR mirrors.
* [README.md](README.md) — overall project overview, build/flash basics,
  badge architecture.
