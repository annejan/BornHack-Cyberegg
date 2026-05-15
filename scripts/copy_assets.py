#!/usr/bin/env python3
"""Factory-floor asset-copy tool for the CyberÆgg badge.

Listens to udev block-device events; when a fresh CyberÆgg USB-MSC
volume appears (FAT12 label ``CYBRxxxx`` where ``xxxx`` is the
device-ID hex), the script:

    1. Mounts it via ``udisksctl mount`` (no-op when the desktop env
       has already auto-mounted).
    2. Verifies the volume is empty (skip if already provisioned).
    3. Copies every file from ``assets/to-badge/`` into the volume
       root.
    4. ``sync(1)`` to flush dirty buffers.
    5. ``udisksctl unmount`` so the host shows the "safe to remove"
       indicator and the worker can unplug cleanly.

Designed to slot into the factory workflow as the asset-load step
that follows the first-boot factory test passing + worker power-cycle.

Usage
-----

    scripts/copy_assets.py            # watch forever
    scripts/copy_assets.py --once     # exit after the first success

Requirements
------------

- Linux with ``udev`` (``udevadm`` in PATH).
- ``udisks2`` package providing ``udisksctl`` — handles mount/unmount
  via polkit, no root required.
- ``util-linux`` providing ``findmnt`` for the "already mounted by
  desktop env" path.

All deps are present on standard desktop installs.  No third-party
Python packages: the only non-stdlib calls are subprocesses to the
above tools.

Behaviour notes
---------------

- Detection is *device-level*: the script catches a badge the moment
  the kernel discovers the partition, even if no desktop env is
  automounting.  Falls back to ``inotify`` on automount directories
  if ``udevadm monitor`` isn't available.
- Already-mounted volumes (auto-mounted before the script started)
  are handled by an initial ``findmnt``-based sweep at startup.
- ``udisksctl mount`` is idempotent — if the desktop already mounted
  the volume, the existing mount point is reported and we just use it.
"""

import argparse
import ctypes
import ctypes.util
import os
import re
import shutil
import struct
import subprocess
import sys
import time
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
ASSETS_DIR = REPO_ROOT / "assets" / "to-badge"
VOLUME_PATTERN = re.compile(r"^CYBR[0-9A-F]{4}$")
IGNORED_ENTRIES = {".Trash-1000", ".trash", "System Volume Information"}


# ---------------------------------------------------------------------------
# Mount / copy / unmount primitives
# ---------------------------------------------------------------------------

def _run(cmd, **kw):
    """subprocess.run wrapper that captures text output by default."""
    return subprocess.run(cmd, capture_output=True, text=True, **kw)


def find_mount_point(devnode):
    """Return the current mount path for ``devnode``, or None if not mounted."""
    r = _run(["findmnt", "-no", "TARGET", devnode])
    if r.returncode != 0:
        return None
    target = r.stdout.strip()
    return Path(target) if target else None


def mount_via_udisks(devnode):
    """Mount the block device via udisksctl; return its mount path."""
    existing = find_mount_point(devnode)
    if existing is not None:
        return existing
    r = _run(["udisksctl", "mount", "-b", devnode])
    if r.returncode != 0:
        # `udisksctl` prints to stderr on failure; "already mounted" can
        # also race with the desktop env auto-mounting between our find
        # and our call, so retry the lookup.
        retry = find_mount_point(devnode)
        if retry is not None:
            return retry
        print(f"  ERR  udisksctl mount {devnode}: {r.stderr.strip()}",
              file=sys.stderr)
        return None
    # Parse "Mounted /dev/sdX1 at /run/media/.../CYBRA3F7."
    m = re.search(r"at\s+(\S+?)\.?\s*$", r.stdout)
    if not m:
        # Some udisksctl versions append a period; some don't.  Either
        # way, fall back to findmnt as the truth source.
        retry = find_mount_point(devnode)
        if retry is not None:
            return retry
        print(f"  ERR  cannot parse mount path: {r.stdout.strip()!r}",
              file=sys.stderr)
        return None
    return Path(m.group(1))


def unmount_via_udisks(devnode):
    r = _run(["udisksctl", "unmount", "-b", devnode])
    if r.returncode == 0:
        return True
    print(f"  WARN udisksctl unmount {devnode}: {r.stderr.strip()}",
          file=sys.stderr)
    return False


def is_volume_empty(mount_point):
    try:
        for entry in mount_point.iterdir():
            if entry.name in IGNORED_ENTRIES:
                continue
            return False
    except (PermissionError, OSError):
        return False
    return True


def copy_assets_to(mount_point, *, quiet):
    files_copied = 0
    total_bytes = 0
    for src in sorted(ASSETS_DIR.iterdir()):
        if not src.is_file():
            continue
        dst = mount_point / src.name
        if not quiet:
            print(f"    {src.name}")
        shutil.copy2(src, dst)
        files_copied += 1
        total_bytes += src.stat().st_size
    subprocess.run(["sync"], check=False)
    print(f"  → {files_copied} files, {total_bytes // 1024} KiB copied + flushed")
    return files_copied > 0


def process_device(devnode, label, *, quiet):
    """Full mount → copy → sync → unmount cycle for one block device."""
    print(f"FRESH {label}  ({devnode})")
    mount_point = mount_via_udisks(devnode)
    if mount_point is None:
        return False
    if not is_volume_empty(mount_point):
        print(f"SKIP  {mount_point}  (not empty — already provisioned?)")
        unmount_via_udisks(devnode)
        return False
    if not copy_assets_to(mount_point, quiet=quiet):
        unmount_via_udisks(devnode)
        return False
    unmount_via_udisks(devnode)
    print(f"DONE  {label}  ✓\n")
    return True


# ---------------------------------------------------------------------------
# Event source: udev monitor (preferred — device-level detection)
# ---------------------------------------------------------------------------

def initial_sweep(quiet, once):
    """Handle any CYBR* volumes already plugged in at startup.

    Uses ``lsblk`` to find every block partition; for any whose
    FAT label matches, we run the full process_device.
    """
    r = _run(["lsblk", "-rno", "NAME,LABEL,TYPE"])
    if r.returncode != 0:
        return False
    for line in r.stdout.splitlines():
        parts = line.split(None, 2)
        if len(parts) < 3:
            continue
        name, label, kind = parts[0], parts[1], parts[2]
        if kind != "part":
            continue
        if not VOLUME_PATTERN.match(label):
            continue
        devnode = f"/dev/{name}"
        if process_device(devnode, label, quiet=quiet) and once:
            return True
    return False


def watch_udev(quiet, once):
    """Block on udev block-device add events; process each CYBR*."""
    cmd = [
        "udevadm", "monitor", "--udev",
        "--subsystem-match=block", "--property",
    ]
    try:
        proc = subprocess.Popen(
            cmd, stdout=subprocess.PIPE, text=True, bufsize=1,
        )
    except FileNotFoundError:
        raise RuntimeError("udevadm not in PATH — install systemd or eudev")

    print(f"Watching (udev) for CYBR* USB-MSC volumes — press Ctrl-C to exit.\n")

    # Handle pre-mounted volumes first.
    if initial_sweep(quiet=quiet, once=once) and once:
        proc.terminate()
        return

    seen = set()
    event = {}
    for line in proc.stdout:
        line = line.rstrip("\n")
        if not line:
            # End-of-record: dispatch if it's an ADD on a CYBR* partition.
            if event.get("ACTION") == "add":
                label = event.get("ID_FS_LABEL", "")
                devnode = event.get("DEVNAME", "")
                if VOLUME_PATTERN.match(label) and devnode and devnode not in seen:
                    seen.add(devnode)
                    if process_device(devnode, label, quiet=quiet) and once:
                        proc.terminate()
                        return
            event = {}
        elif "=" in line:
            k, v = line.split("=", 1)
            event[k] = v
        # Header lines like "UDEV  [123.456] add /devices/..." carry no
        # key=value, but the property dump that follows them does.


# ---------------------------------------------------------------------------
# Event source: inotify (fallback for when udevadm is unavailable)
# ---------------------------------------------------------------------------

IN_CREATE = 0x00000100
IN_MOVED_TO = 0x00000080
_EVENT_HEADER_FMT = "iIII"
_EVENT_HEADER_SIZE = struct.calcsize(_EVENT_HEADER_FMT)


def candidate_mount_roots():
    user = os.environ.get("USER", "")
    if user:
        yield Path(f"/run/media/{user}")
        yield Path(f"/media/{user}")
    yield Path("/media")


def watch_inotify_fallback(quiet, once):
    """If udev monitoring fails, fall back to watching mount-point dirs.

    This catches volumes the desktop env auto-mounts, but won't see
    devices on headless setups with no automount.  Each detected mount
    is still processed through ``process_device`` for the explicit
    mount/unmount cycle (idempotent — already-mounted volumes are
    found by ``findmnt``).
    """
    libc_path = ctypes.util.find_library("c") or "libc.so.6"
    libc = ctypes.CDLL(libc_path, use_errno=True)
    libc.inotify_init1.argtypes = [ctypes.c_int]
    libc.inotify_init1.restype = ctypes.c_int
    libc.inotify_add_watch.argtypes = [ctypes.c_int, ctypes.c_char_p, ctypes.c_uint32]
    libc.inotify_add_watch.restype = ctypes.c_int

    fd = libc.inotify_init1(0)
    if fd < 0:
        raise OSError(ctypes.get_errno(), "inotify_init1 failed")
    wd_to_path = {}
    for root in candidate_mount_roots():
        if not root.is_dir():
            continue
        wd = libc.inotify_add_watch(fd, str(root).encode(), IN_CREATE | IN_MOVED_TO)
        if wd >= 0:
            wd_to_path[wd] = root
    if not wd_to_path:
        raise FileNotFoundError("no mount roots exist to watch")

    print(f"Watching (inotify) {', '.join(str(p) for p in wd_to_path.values())}\n")

    # Initial sweep handles anything already mounted.
    if initial_sweep(quiet=quiet, once=once) and once:
        return

    seen = set()
    while True:
        buf = os.read(fd, 4096)
        offset = 0
        while offset < len(buf):
            wd, _mask, _cookie, name_len = struct.unpack_from(
                _EVENT_HEADER_FMT, buf, offset
            )
            name = (
                buf[offset + _EVENT_HEADER_SIZE : offset + _EVENT_HEADER_SIZE + name_len]
                .rstrip(b"\x00")
                .decode(errors="replace")
            )
            offset += _EVENT_HEADER_SIZE + name_len
            if not VOLUME_PATTERN.match(name):
                continue
            # The new directory is a mount point; resolve its backing device.
            root = wd_to_path.get(wd)
            if root is None:
                continue
            mount_point = root / name
            if mount_point in seen:
                continue
            seen.add(mount_point)
            r = _run(["findmnt", "-no", "SOURCE", str(mount_point)])
            if r.returncode != 0 or not r.stdout.strip():
                continue
            devnode = r.stdout.strip()
            if process_device(devnode, name, quiet=quiet) and once:
                return


# ---------------------------------------------------------------------------
# Entry point
# ---------------------------------------------------------------------------

def main():
    parser = argparse.ArgumentParser(description="Auto-copy assets to fresh CyberÆgg badges.")
    parser.add_argument("--once", action="store_true",
                        help="Exit after the first successful copy.")
    parser.add_argument("--quiet", action="store_true",
                        help="Suppress per-file progress output.")
    parser.add_argument("--fallback", action="store_true",
                        help="Force the inotify mount-point fallback (debug).")
    args = parser.parse_args()

    if not ASSETS_DIR.is_dir():
        print(f"ERROR: assets directory {ASSETS_DIR} not found", file=sys.stderr)
        sys.exit(1)
    if not any(p.suffix.lower() == ".pcx" for p in ASSETS_DIR.iterdir() if p.is_file()):
        print(f"WARNING: {ASSETS_DIR} has no .PCX files — did the asset bundle build?",
              file=sys.stderr)

    print(f"Asset bundle: {ASSETS_DIR}/")
    try:
        if args.fallback:
            watch_inotify_fallback(quiet=args.quiet, once=args.once)
        else:
            try:
                watch_udev(quiet=args.quiet, once=args.once)
            except (RuntimeError, FileNotFoundError) as e:
                print(f"WARN  udev unavailable: {e}", file=sys.stderr)
                watch_inotify_fallback(quiet=args.quiet, once=args.once)
    except KeyboardInterrupt:
        print("\nBye.")


if __name__ == "__main__":
    main()
