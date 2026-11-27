#!/usr/bin/env python3
"""Regenerate src/game/realm_picture.rs data from assets/bornpets-unicorn-realm.png.

The PNG is the source of truth: 96 x 64, black / red / white only.
Usage: python3 scripts/realm_picture.py   (needs Pillow)
"""
import re
import sys
from pathlib import Path
from PIL import Image

ROOT = Path(__file__).resolve().parent.parent
PNG = ROOT / "assets" / "bornpets-unicorn-realm.png"
RS = ROOT / "src" / "game" / "realm_picture.rs"
BLACK, RED, WHITE = (0, 0, 0), (220, 0, 0), (255, 255, 255)


def planes():
    im = Image.open(PNG).convert("RGB")
    w, h = im.size
    assert (w, h) == (96, 64), im.size
    black, red = bytearray(), bytearray()
    for y in range(h):
        for x0 in range(0, w, 8):
            b = r = 0
            for dx in range(8):
                p = im.getpixel((x0 + dx, y))
                assert p in (BLACK, RED, WHITE), (x0 + dx, y, p)
                b = (b << 1) | (p == BLACK)
                r = (r << 1) | (p == RED)
            black.append(b)
            red.append(r)
    return black, red


def fmt(name, data):
    lines = [f"const {name}: [u8; {len(data)}] = ["]
    for i in range(0, len(data), 12):
        lines.append("    " + ", ".join(f"0x{v:02x}" for v in data[i:i + 12]) + ",")
    lines.append("];")
    return "\n".join(lines)


def main():
    black, red = planes()
    text = RS.read_text()
    for name, data in (("BLACK_PLANE", black), ("RED_PLANE", red)):
        pat = re.compile(rf"const {name}: \[u8; \d+\] = \[.*?\n\];", re.S)
        if not pat.search(text):
            sys.exit(f"{name} not found in {RS}")
        text = pat.sub(lambda _m: fmt(name, data), text, count=1)
    RS.write_text(text)


if __name__ == "__main__":
    main()
