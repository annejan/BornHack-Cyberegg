#!/usr/bin/env python3
"""Factory-floor asset-copy tool for the CyberÆgg badge.

Polls the OS's automount roots for a freshly-mounted CyberÆgg USB-MSC
volume (label ``CYBRxxxx`` where ``xxxx`` is the device-ID hex) and,
when it finds an *empty* one, copies every file from
``assets/to-badge/`` into the volume's root.  Designed to slot into the
factory workflow as the step that follows ``factory_test`` passing:

    1. Worker plugs badge into USB
    2. First-boot factory test runs to completion (auto-marks KV pass,
       renders the ship-image, halts).
    3. Worker power-cycles the badge — KV pass means it skips the test
       and boots normally.  The App's USB-MSC task enumerates the FAT12
       partition under e.g. ``/run/media/$USER/CYBRA3F7/``.
    4. This script (running continuously on the factory laptop) sees
       the new mount, confirms it's empty, copies the asset bundle,
       and reports success.
    5. Worker unplugs + packs the badge.

Usage
-----

    scripts/copy_assets.py            # watch forever, copy every fresh badge
    scripts/copy_assets.py --once     # copy the first fresh badge, then exit
    scripts/copy_assets.py --quiet    # don't print per-file progress

Detection
---------

Volume label regex: ``^CYBR[0-9A-F]{4}$`` (matches ``fw::fat12::format``
in the firmware).  Both ``/run/media/$USER/`` and ``/media/$USER/`` are
scanned, plus the legacy bare ``/media/`` for distros that mount there.

A volume is considered "empty" when its root directory contains no
non-system entries (``.Trash-*``, ``System Volume Information`` are
ignored).  Anything else → script SKIPs it as "already provisioned".

Cleanup
-------

After a successful copy the script calls ``sync(1)`` so the assets are
flushed before the worker unplugs.  No automatic unmount: the host
desktop environment will release the volume when the cable is pulled.
"""

import argparse
import os
import re
import shutil
import subprocess
import sys
import time
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
ASSETS_DIR = REPO_ROOT / "assets" / "to-badge"
VOLUME_PATTERN = re.compile(r"^CYBR[0-9A-F]{4}$")
IGNORED_ENTRIES = {".Trash-1000", ".trash", "System Volume Information"}
POLL_INTERVAL_SEC = 1.0


def candidate_mount_roots():
    user = os.environ.get("USER", "")
    if user:
        yield Path(f"/run/media/{user}")
        yield Path(f"/media/{user}")
    yield Path("/media")


def find_unprocessed_volumes(seen):
    for root in candidate_mount_roots():
        if not root.is_dir():
            continue
        for entry in root.iterdir():
            if not VOLUME_PATTERN.match(entry.name):
                continue
            if not entry.is_dir():
                continue
            if entry in seen:
                continue
            yield entry


def is_volume_empty(volume):
    try:
        for entry in volume.iterdir():
            if entry.name in IGNORED_ENTRIES:
                continue
            return False
    except PermissionError:
        return False
    return True


def copy_assets_to(volume, *, quiet):
    files_copied = 0
    total_bytes = 0
    for src in sorted(ASSETS_DIR.iterdir()):
        if not src.is_file():
            continue
        dst = volume / src.name
        if not quiet:
            print(f"    {src.name}")
        shutil.copy2(src, dst)
        files_copied += 1
        total_bytes += src.stat().st_size
    subprocess.run(["sync"], check=False)
    print(f"  → {files_copied} files, {total_bytes // 1024} KiB copied + flushed")
    return files_copied > 0


def main():
    parser = argparse.ArgumentParser(description="Auto-copy assets to fresh CyberÆgg badges.")
    parser.add_argument("--once", action="store_true",
                        help="Exit after the first successful copy.")
    parser.add_argument("--quiet", action="store_true",
                        help="Suppress per-file progress output.")
    args = parser.parse_args()

    if not ASSETS_DIR.is_dir():
        print(f"ERROR: assets directory {ASSETS_DIR} not found", file=sys.stderr)
        sys.exit(1)
    if not any(p.suffix.lower() == ".pcx" for p in ASSETS_DIR.iterdir() if p.is_file()):
        print(f"WARNING: {ASSETS_DIR} has no .PCX files — did the asset bundle build?",
              file=sys.stderr)

    user = os.environ.get("USER", "?")
    print(f"Watching for fresh CyberÆgg badges (label CYBRxxxx) under")
    print(f"  /run/media/{user}/  and  /media/{user}/")
    print(f"Will copy {ASSETS_DIR}/ contents into the volume root.")
    print("Ctrl-C to exit.\n")

    seen = set()
    try:
        while True:
            for volume in find_unprocessed_volumes(seen):
                seen.add(volume)
                if not is_volume_empty(volume):
                    print(f"SKIP {volume}  (not empty — already provisioned?)")
                    continue
                print(f"FRESH {volume}")
                if copy_assets_to(volume, quiet=args.quiet):
                    print(f"DONE  {volume}  ✓\n")
                    if args.once:
                        return
            time.sleep(POLL_INTERVAL_SEC)
    except KeyboardInterrupt:
        print("\nBye.")


if __name__ == "__main__":
    main()
