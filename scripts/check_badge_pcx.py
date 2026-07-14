#!/usr/bin/env python3
"""Validate that every PCX in assets/to-badge/ is in the format the badge
decoder reads.

Universal (all sprites — pets, icons, logos): PCX manufacturer 10, RLE
encoding, 2 bits/pixel, 1 plane, and NO 256-colour palette trailer. The
usual failure is a GIMP re-export at 4bpp (bpl doubles) — the badge only
reads 2bpp, so those render as white/garbled slides.

Sponsor logos (filenames 0300xx / 0301xx) additionally must be full-screen
152x152. Pet frames and menu icons are their own smaller sizes, so size is
NOT checked for them.

Usage:  scripts/check_badge_pcx.py [dir]   (default: assets/to-badge)
Exit 0 = all good, 1 = one or more bad files.
"""
import glob
import os
import struct
import sys

UNIVERSAL = dict(mfr=10, enc=1, bpp=2, planes=1)  # every badge PCX


def is_sponsor(name):
    b = os.path.basename(name).upper()
    return b.startswith("0300") or b.startswith("0301")


def check(path):
    d = open(path, "rb").read()
    if len(d) < 128:
        return ["file too small / not a PCX"]
    xmax, ymax = struct.unpack("<HH", d[8:12])
    got = dict(
        mfr=d[0], enc=d[2], bpp=d[3], planes=d[65],
        w=xmax + 1, h=ymax + 1, bpl=struct.unpack("<H", d[66:68])[0],
    )
    errs = [f"{k}={got[k]} (want {v})" for k, v in UNIVERSAL.items() if got[k] != v]
    if is_sponsor(path):  # full-screen logos only
        for k, v in dict(w=152, h=152, bpl=38).items():
            if got[k] != v:
                errs.append(f"{k}={got[k]} (want {v})")
    if len(d) >= 769 and d[-769] == 0x0C:
        errs.append("has 256-colour palette trailer (unsupported)")
    return errs


def main():
    root = sys.argv[1] if len(sys.argv) > 1 else os.path.join(
        os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "assets/to-badge"
    )
    files = sorted(glob.glob(os.path.join(root, "*.PCX")) + glob.glob(os.path.join(root, "*.pcx")))
    if not files:
        print(f"no PCX files in {root}")
        return 1
    bad = 0
    for f in files:
        errs = check(f)
        if errs:
            bad += 1
            print(f"BAD  {os.path.basename(f)}: {'; '.join(errs)}")
    print(f"\n{len(files) - bad}/{len(files)} OK" + (f", {bad} BAD" if bad else ""))
    if bad:
        print("Re-encode bad files: flatten to PNG, then "
              "scripts/png_to_badge_pcx.py <png> <out.PCX> --asis")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
