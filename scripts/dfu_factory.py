#!/usr/bin/env python3
"""Factory production-line DFU flasher.

Watches udev for USB ADD events matching the CyberÆgg DFU bootloader
(VID 1915 / PID 521f, set by ``bootloader/src/dfu.rs``), then runs
``dfu-util`` to push the release App ``.bin``.  Continues watching
after each successful flash so a worker can roll badge after badge
through the station without restarting the script.

Trio of factory tools:

  - ``scripts/bl_factory.py``  — assembly station (SWD bootloader)
  - ``scripts/dfu_factory.py`` — production station (DFU App flash)  ←
  - ``scripts/copy_assets.py`` — production station (asset bundle copy)

Per-board workflow at this station:

  1. Worker holds **Execute** on a board, plugs USB.
  2. Bootloader enters DFU mode (red LED blinking, USB enumerates as
     1915:521f).
  3. This script catches the udev ADD event.
  4. ``dfu-util -D embassy.bin`` runs against that device.
  5. Bootloader auto-resets out of DFU into the App on completion.
  6. App's factory_test runs, ship image renders on PASS, USB-MSC
     enumerates as CYBR* — ``copy_assets.py`` (already running)
     lands the sprite bundle.
  7. Worker unplugs, packs.

Usage
-----

    scripts/dfu_factory.py                 # watch forever, release App
    scripts/dfu_factory.py --once          # exit after first flash
    scripts/dfu_factory.py --debug         # use debug App (dev only)
    scripts/dfu_factory.py --bin path/to/embassy.bin

Requirements
------------

- Linux with ``udev`` (``udevadm`` in PATH).
- ``dfu-util`` in PATH and our udev rule installed
  (``scripts/99-cyberaegg.rules``) so it works without sudo.
- ``arm-none-eabi-objcopy`` (or a pre-built ``.bin``).  The script auto-
  runs ``make fw-release && objcopy`` on startup if the release .bin
  is missing.

Implementation notes
--------------------

Each USB device that enumerates produces multiple udev events (one
per interface plus the device-level event).  We filter to
``DEVTYPE=usb_device`` so we only fire once per badge.  Identical
flashes are deduped by ``ID_SERIAL_SHORT`` for ~5 s to defend against
duplicate add events from interface-add settling.

dfu-util is invoked with the device address (``--device VID:PID``) and
the matched serial via ``--serial`` so multiple DFU devices on the
same hub don't collide.  No ``-w`` (wait-for-device) — if the badge
vanished between the udev event and dfu-util fork, we fail fast and
catch the next ADD event.
"""

import argparse
import os
import re
import subprocess
import sys
import time
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
RELEASE_BIN = REPO_ROOT / "target/thumbv7em-none-eabihf/release/embassy.bin"
DEBUG_BIN = REPO_ROOT / "target/thumbv7em-none-eabihf/debug/embassy.bin"
RELEASE_ELF = REPO_ROOT / "target/thumbv7em-none-eabihf/release/embassy"

DFU_VID = "1915"
DFU_PID = "521f"
DEDUPE_WINDOW_SEC = 5.0
POLL_INTERVAL_SEC = 1.0
# Time to let the kernel finish enumerating the DFU interface after a
# udev ADD event before we call dfu-util.  Without this, dfu-util races
# the kernel and gets LIBUSB_ERROR_BUSY trying to claim the interface.
USB_SETTLE_SEC = 1.0


def run(cmd, **kw):
    return subprocess.run(cmd, capture_output=True, text=True, **kw)


# ---------------------------------------------------------------------------
# .bin housekeeping
# ---------------------------------------------------------------------------

def ensure_release_bin():
    """If the release .bin is missing, build the ELF and objcopy it.

    Idempotent: no-op when both files exist and the .bin is newer than
    the ELF (the typical state after a build).
    """
    if RELEASE_BIN.is_file():
        return
    print("Release .bin missing — building…", flush=True)
    r = subprocess.run(["make", "fw-release"], cwd=REPO_ROOT)
    if r.returncode != 0:
        print("ERROR: 'make fw-release' failed", file=sys.stderr)
        sys.exit(1)
    r = subprocess.run(
        ["arm-none-eabi-objcopy", "-O", "binary", str(RELEASE_ELF), str(RELEASE_BIN)],
        cwd=REPO_ROOT,
    )
    if r.returncode != 0:
        print("ERROR: objcopy failed", file=sys.stderr)
        sys.exit(1)


# ---------------------------------------------------------------------------
# DFU flash
# ---------------------------------------------------------------------------

def dfu_flash(bin_path, *, serial=None):
    """Run ``dfu-util`` against the specified DFU device serial."""
    cmd = ["dfu-util", "-d", f"{DFU_VID}:{DFU_PID}", "-D", str(bin_path)]
    if serial:
        cmd[3:3] = ["-S", serial]
    t0 = time.monotonic()
    r = run(cmd)
    elapsed = time.monotonic() - t0
    if r.returncode == 0:
        print(f"  → DFU flashed in {elapsed:.1f}s ✓\n")
        return True
    # Common failure: badge already exited DFU between detection and run.
    print(f"  → DFU FAILED (exit {r.returncode}) after {elapsed:.1f}s")
    # dfu-util writes progress + most error context to stdout; only the
    # generic suffix warning goes to stderr.  Print both, stdout first,
    # so the real cause is visible.
    for stream_name, stream in (("stdout", r.stdout), ("stderr", r.stderr)):
        text = stream.strip()
        if not text:
            continue
        print(f"    -- {stream_name} --")
        for line in text.splitlines()[-12:]:
            print(f"    {line}")
    return False


# ---------------------------------------------------------------------------
# Event source: udev monitor (preferred)
# ---------------------------------------------------------------------------

def watch_udev(bin_path, *, once):
    cmd = ["udevadm", "monitor", "--udev", "--subsystem-match=usb", "--property"]
    try:
        proc = subprocess.Popen(cmd, stdout=subprocess.PIPE, text=True, bufsize=1)
    except FileNotFoundError:
        raise RuntimeError("udevadm not in PATH — install systemd or eudev")

    bin_kb = bin_path.stat().st_size // 1024
    print(f"DFU production station — auto-flashing {bin_path.name} ({bin_kb} KiB)")
    print(f"Watching (udev) for VID {DFU_VID}:{DFU_PID} — Ctrl-C to exit.\n")

    # Initial sweep: any DFU device already in DFU when the script starts.
    if initial_sweep(bin_path, once):
        proc.terminate()
        return

    last_serial = {}
    event = {}
    for line in proc.stdout:
        line = line.rstrip("\n")
        if not line:
            if event.get("ACTION") == "add" and event.get("DEVTYPE") == "usb_device":
                vid = event.get("ID_VENDOR_ID", "").lower()
                pid = event.get("ID_MODEL_ID", "").lower()
                if vid == DFU_VID and pid == DFU_PID:
                    serial = event.get("ID_SERIAL_SHORT", "")
                    # Dedup duplicate add events within DEDUPE_WINDOW_SEC.
                    now = time.monotonic()
                    if serial in last_serial and now - last_serial[serial] < DEDUPE_WINDOW_SEC:
                        event = {}
                        continue
                    last_serial[serial] = now
                    print(f"FRESH  DFU device  serial={serial or '(none)'}")
                    # Let the kernel finish claiming/releasing the
                    # interface; dfu-util otherwise races and gets
                    # LIBUSB_ERROR_BUSY.
                    time.sleep(USB_SETTLE_SEC)
                    if dfu_flash(bin_path, serial=serial):
                        if once:
                            proc.terminate()
                            return
            event = {}
        elif "=" in line:
            k, v = line.split("=", 1)
            event[k] = v


def initial_sweep(bin_path, once):
    """If a DFU device was already plugged in when the script started,
    flash it without waiting for the next udev event."""
    r = run(["dfu-util", "-d", f"{DFU_VID}:{DFU_PID}", "-l"])
    if r.returncode != 0:
        return False
    # Look for "[1915:521f] ..." lines in --list output.
    if f"[{DFU_VID}:{DFU_PID}]" not in r.stdout:
        return False
    print("DFU device already in DFU at startup — flashing immediately.")
    # Same settle window as the udev path: covers the case where the
    # worker plugged the badge then immediately started the script.
    time.sleep(USB_SETTLE_SEC)
    # We don't easily extract the serial from --list parsing in older
    # dfu-util versions; pass None and dfu-util picks the first match.
    if dfu_flash(bin_path, serial=None) and once:
        return True
    return False


# ---------------------------------------------------------------------------
# Event source: polling fallback
# ---------------------------------------------------------------------------

def watch_polling(bin_path, *, once):
    print("Falling back to 1 s polling (udev unavailable).\n")
    seen = set()
    while True:
        r = run(["dfu-util", "-d", f"{DFU_VID}:{DFU_PID}", "-l"])
        if r.returncode == 0 and f"[{DFU_VID}:{DFU_PID}]" in r.stdout:
            # Extract serial via the "serial=..." substring in dfu-util output.
            m = re.search(r'serial="([^"]+)"', r.stdout)
            serial = m.group(1) if m else None
            if serial not in seen:
                seen.add(serial)
                print(f"FRESH  DFU device  serial={serial or '(none)'}")
                if dfu_flash(bin_path, serial=serial) and once:
                    return
        else:
            # Reset seen when no device — allows re-flash of same serial later.
            seen.clear()
        time.sleep(POLL_INTERVAL_SEC)


# ---------------------------------------------------------------------------
# Entry point
# ---------------------------------------------------------------------------

def main():
    parser = argparse.ArgumentParser(description="Auto-flash CyberÆgg App via DFU.")
    parser.add_argument("--once", action="store_true",
                        help="Exit after the first successful flash.")
    parser.add_argument("--debug", action="store_true",
                        help="Use the debug .bin instead of release (dev only).")
    parser.add_argument("--bin", metavar="PATH",
                        help="Custom .bin path; overrides --debug / default release.")
    parser.add_argument("--poll", action="store_true",
                        help="Force polling instead of udev (debug).")
    args = parser.parse_args()

    if args.bin:
        bin_path = Path(args.bin)
    elif args.debug:
        bin_path = DEBUG_BIN
    else:
        bin_path = RELEASE_BIN
        ensure_release_bin()

    if not bin_path.is_file():
        print(f"ERROR: .bin not found at {bin_path}", file=sys.stderr)
        print("       try 'make fw-release' (or 'make fw' + --debug)", file=sys.stderr)
        sys.exit(1)

    try:
        if args.poll:
            watch_polling(bin_path, once=args.once)
        else:
            try:
                watch_udev(bin_path, once=args.once)
            except (RuntimeError, FileNotFoundError) as e:
                print(f"WARN  udev unavailable: {e}", file=sys.stderr)
                watch_polling(bin_path, once=args.once)
    except KeyboardInterrupt:
        print("\nBye.")


if __name__ == "__main__":
    main()
