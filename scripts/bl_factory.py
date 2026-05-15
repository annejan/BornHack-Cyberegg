#!/usr/bin/env python3
"""Factory-assembly station: auto-flash the custom bootloader to a
fresh CyberÆgg board the moment it lands on the J-Link SWD jig.

Pairs with ``scripts/copy_assets.py``:

  - **This script (`bl_factory.py`)** runs at the assembly station.
    Worker places a fresh board onto an SWD jig; script polls the
    J-Link's target voltage, detects rising-edge (no board → board),
    runs ``probe-rs erase`` + bootloader download + reset.  Reports
    success; waits for the board to be lifted (falling edge) before
    arming for the next.
  - **`copy_assets.py`** then runs at the production station once
    each board has bootloader + factory_test App + has passed.

After this script runs, the board has the custom bootloader at
``0x00000000`` only — no App.  The App is loaded via USB-DFU at the
production station (~5–10 s per board) instead of via SWD (~100 s
per board), so the J-Link bank only has to push the small 64 KB
bootloader, parallelised across however many J-Links you have.

Per-board on this script: ~3 s (chip erase + 2 s bootloader write
+ verify + reset).  Versus a full ``make flash-bl`` + ``make flash``
cycle: ~107 s.

Usage
-----

    scripts/bl_factory.py              # watch forever, flash each new board
    scripts/bl_factory.py --once       # flash one board, exit
    scripts/bl_factory.py --probe ID   # pick a specific J-Link if multiple
    scripts/bl_factory.py --with-app   # also flash the debug App (slower,
                                       # but skips needing DFU later)

Requirements
------------

- ``probe-rs`` CLI in PATH (``probe-rs-tools`` cargo install).
- A J-Link / ST-Link / CMSIS-DAP on the host with udev rules.
- The bootloader release ELF, pre-built at:
    ``./bootloader/target/thumbv7em-none-eabihf/release/nrf-aegg-bootloader``
  Script auto-runs ``cargo bl`` (from the ``bootloader/`` subdir) on
  startup if the ELF is missing.

Behaviour notes
---------------

- Target detection uses ``probe-rs read 0x10000000 1`` (4 bytes off
  the start of the FICR INFO page).  Returncode 0 = chip is alive
  and reachable.  Returncode != 0 = no board, or VTref is 0 V.
- "Armed" state machine: after a successful flash, the script
  refuses to re-flash until ``probe-rs read`` fails (worker has
  lifted the board off the jig).  Prevents continuous re-flashing
  the same board in a tight loop if it stays on the jig.
- On flash error, the script logs and stays armed — worker can
  reseat the board and trigger a retry.

Stdout is the only feedback channel; no GUI, no sound.  Pipe to
``logger -t bl-factory`` if you want syslog capture.
"""

import argparse
import subprocess
import sys
import time
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
BOOTLOADER_DIR = REPO_ROOT / "bootloader"
BOOTLOADER_ELF = REPO_ROOT / "bootloader/target/thumbv7em-none-eabihf/release/nrf-aegg-bootloader"
APP_ELF_RELEASE = REPO_ROOT / "target/thumbv7em-none-eabihf/release/embassy"
APP_ELF_DEBUG = REPO_ROOT / "target/thumbv7em-none-eabihf/debug/embassy"
CHIP = "nRF52840_xxAA"
POLL_INTERVAL_SEC = 1.0
# 0x10000000 is the start of the nRF52 FICR — always readable on a
# live chip, returns 0V/no-target error if the SWD bus is dead.
PROBE_ADDR = "0x10000000"


def run(cmd, **kw):
    return subprocess.run(cmd, capture_output=True, text=True, **kw)


def _with_probe(args, probe_id):
    cmd = ["probe-rs"] + args
    if probe_id:
        cmd[2:2] = ["--probe", probe_id]
    return cmd


def probe_target(probe_id):
    """Return True if a board is connected and powered on the J-Link."""
    cmd = _with_probe(["read", "--chip", CHIP, "b32", PROBE_ADDR, "1"], probe_id)
    try:
        r = run(cmd, timeout=5)
    except subprocess.TimeoutExpired:
        return False
    return r.returncode == 0 and r.stdout.strip() != ""


def build_bootloader():
    print("Building bootloader (cargo bl)…", flush=True)
    r = subprocess.run(["cargo", "bl"], cwd=BOOTLOADER_DIR)
    if r.returncode != 0:
        print(f"ERROR: bootloader build failed (exit {r.returncode})", file=sys.stderr)
        sys.exit(1)


def flash_one(probe_id, *, with_app, app_elf):
    """Erase + bootloader write + (optional) app write + reset."""
    t0 = time.monotonic()

    print("  erase…", end=" ", flush=True)
    r = run(_with_probe(["erase", "--chip", CHIP], probe_id))
    if r.returncode != 0:
        print(f"FAILED\n    {r.stderr.strip()}")
        return False
    print(f"{time.monotonic() - t0:.1f}s")

    print("  bootloader…", end=" ", flush=True)
    t1 = time.monotonic()
    r = run(_with_probe(["download", "--chip", CHIP, "--verify", str(BOOTLOADER_ELF)], probe_id))
    if r.returncode != 0:
        print(f"FAILED\n    {r.stderr.strip()}")
        return False
    print(f"{time.monotonic() - t1:.1f}s")

    if with_app:
        if not app_elf.is_file():
            print(f"  WARN  app ELF missing ({app_elf}) — run 'make fw-release' first "
                  f"(or --debug + 'make fw')")
        else:
            print("  app…", end=" ", flush=True)
            t2 = time.monotonic()
            r = run(_with_probe(["download", "--chip", CHIP, "--verify", str(app_elf)], probe_id))
            if r.returncode != 0:
                print(f"FAILED\n    {r.stderr.strip()}")
                return False
            print(f"{time.monotonic() - t2:.1f}s")

    print("  reset…", end=" ", flush=True)
    run(_with_probe(["reset", "--chip", CHIP], probe_id))
    print(f"{time.monotonic() - t0:.1f}s total ✓\n")
    return True


def main():
    parser = argparse.ArgumentParser(description="Auto-flash bootloader to fresh boards on SWD.")
    parser.add_argument("--once", action="store_true",
                        help="Exit after the first successful flash.")
    parser.add_argument("--probe", metavar="ID",
                        help="J-Link probe selector (VID:PID or serial).  "
                             "Default: probe-rs picks the only attached one.")
    parser.add_argument("--with-app", action="store_true",
                        help="Also flash the App ELF via SWD (slower).  "
                             "Default: bootloader only; load App via DFU later.")
    parser.add_argument("--debug", action="store_true",
                        help="Use the debug App ELF instead of release (dev only).")
    args = parser.parse_args()
    app_elf = APP_ELF_DEBUG if args.debug else APP_ELF_RELEASE

    if not BOOTLOADER_ELF.is_file():
        build_bootloader()
    if not BOOTLOADER_ELF.is_file():
        print(f"ERROR: bootloader ELF still missing at {BOOTLOADER_ELF}",
              file=sys.stderr)
        sys.exit(1)

    variant = "debug" if args.debug else "release"
    label = "bootloader" + (f" + app ({variant})" if args.with_app else "")
    if args.with_app and not app_elf.is_file():
        print(f"ERROR: app ELF missing at {app_elf}", file=sys.stderr)
        print(f"       run 'make fw-release' first (or --debug + 'make fw')",
              file=sys.stderr)
        sys.exit(1)
    print(f"Factory SWD station — auto-flashing {label} on every fresh board.")
    print(f"Probe: {args.probe or 'auto'}   chip: {CHIP}")
    print("Place a board on the SWD jig; lift after DONE.  Ctrl-C to exit.\n")

    armed = True
    try:
        while True:
            present = probe_target(args.probe)
            if armed and present:
                print("BOARD detected.")
                if flash_one(args.probe, with_app=args.with_app, app_elf=app_elf):
                    armed = False
                    if args.once:
                        return
            elif not armed and not present:
                print("Board lifted — ready for next.\n")
                armed = True
            time.sleep(POLL_INTERVAL_SEC)
    except KeyboardInterrupt:
        print("\nBye.")


if __name__ == "__main__":
    main()
