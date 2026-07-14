#!/usr/bin/env python3
"""Convert a PNG logo to a CyberAegg badge PCX (152x152, 2bpp, 1-plane, RLE).

Palette: 0=black 1=red 2=white 3=transparent. The image is scaled to fit
(preserving aspect), centred on a white 152x152 canvas, composited over white,
and each pixel quantised to nearest of black / red / white.

Usage:  png_to_badge_pcx.py <in.png> <out.PCX> [--pad N] [--bg white|transparent]
"""
import argparse
import os
from PIL import Image

W = H = 152
REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
HEADER_SRC = os.path.join(REPO, "assets/to-badge/030000.PCX")  # reuse 128-byte header

PAL = {0: (0, 0, 0), 1: (255, 0, 0), 2: (255, 255, 255)}  # quantise targets


def quantize_index(r, g, b):
    return min(PAL, key=lambda i: sum((a - c) ** 2 for a, c in zip((r, g, b), PAL[i])))


def pack_row(idx):
    out = bytearray()
    for x in range(0, W, 4):
        b = 0
        for k in range(4):
            b |= (idx[x + k] & 3) << (6 - 2 * k)
        out.append(b)
    return out  # 38 bytes


def rle(row):
    out = bytearray()
    i, n = 0, len(row)
    while i < n:
        b = row[i]
        run = 1
        while i + run < n and row[i + run] == b and run < 63:
            run += 1
        if run > 1 or (b & 0xC0) == 0xC0:
            out += bytes((0xC0 | run, b))
        else:
            out.append(b)
        i += run
    return out


def convert(in_png, out_pcx, pad, bg_transparent):
    src = Image.open(in_png).convert("RGBA")
    max_wh = W - 2 * pad
    src.thumbnail((max_wh, max_wh), Image.LANCZOS)
    # White canvas, paste logo centred using its own alpha as the mask.
    canvas = Image.new("RGB", (W, H), (255, 255, 255))
    ox, oy = (W - src.width) // 2, (H - src.height) // 2
    canvas.paste(src, (ox, oy), src)
    px = canvas.load()
    alpha = src.split()[3]

    header = open(HEADER_SRC, "rb").read(128)
    data = bytearray(header)
    for y in range(H):
        row = []
        for x in range(W):
            # Optionally keep the outside-logo area transparent (index 3).
            inside = ox <= x < ox + src.width and oy <= y < oy + src.height
            if bg_transparent and (not inside or alpha.getpixel((x - ox, y - oy)) < 8):
                row.append(3)
            else:
                row.append(quantize_index(*px[x, y]))
        data += rle(pack_row(row))
    open(out_pcx, "wb").write(data)
    print(f"{os.path.basename(out_pcx)}  {src.width}x{src.height} centred  {len(data)} bytes")


if __name__ == "__main__":
    ap = argparse.ArgumentParser()
    ap.add_argument("in_png")
    ap.add_argument("out_pcx")
    ap.add_argument("--pad", type=int, default=6)
    ap.add_argument("--bg", choices=["white", "transparent"], default="white")
    a = ap.parse_args()
    convert(a.in_png, a.out_pcx, a.pad, a.bg == "transparent")
