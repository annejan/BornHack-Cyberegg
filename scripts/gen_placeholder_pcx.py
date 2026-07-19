#!/usr/bin/env python3
"""Generate a placeholder 152x75 badge PCX with centred black text on white.

Stopgap for pet-frame art that doesn't exist yet (e.g. the "Only pets"
dance animation) — renders one or more lines of text with PIL's default
font onto a white canvas, quantises to black/white, and packs it exactly
like a real pet frame.

Reuses the pack_row()/rle() approach from png_to_badge_pcx.py, but at
152x75 (a pet action frame, not the full 152x152 screen) and borrows the
128-byte header from an existing 152x75 pet frame (012500.PCX), which
already has the correct xmax=151, ymax=74, bpl=38.

Usage:  gen_placeholder_pcx.py <out.PCX> <line1> [line2] ...
"""
import os
import sys

from PIL import Image, ImageDraw, ImageFont

W, H = 152, 75
REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
HEADER_SRC = os.path.join(REPO, "assets/to-badge/012500.PCX")


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


def render(lines):
    """Render centred text lines onto a 152x75 white 'L' (grayscale) canvas."""
    img = Image.new("L", (W, H), 255)
    draw = ImageDraw.Draw(img)
    font = ImageFont.load_default()

    boxes = [draw.textbbox((0, 0), line, font=font) for line in lines]
    heights = [b[3] - b[1] for b in boxes]
    widths = [b[2] - b[0] for b in boxes]
    spacing = 3
    total_h = sum(heights) + spacing * (len(lines) - 1)
    y = (H - total_h) // 2
    for line, box, w, h in zip(lines, boxes, widths, heights):
        x = (W - w) // 2 - box[0]
        draw.text((x, y - box[1]), line, fill=0, font=font)
        y += h + spacing
    return img


def quantize(img):
    """0 = black, 2 = white — the badge palette (1 = red is unused here)."""
    px = img.load()
    rows = []
    for y in range(H):
        rows.append([0 if px[x, y] < 128 else 2 for x in range(W)])
    return rows


def main():
    if len(sys.argv) < 3:
        print(__doc__)
        return 1
    out_path = sys.argv[1]
    lines = sys.argv[2:]

    img = render(lines)
    rows = quantize(img)

    header = open(HEADER_SRC, "rb").read(128)
    data = bytearray(header)
    for row in rows:
        data += rle(pack_row(row))

    with open(out_path, "wb") as f:
        f.write(data)
    print(f"{os.path.basename(out_path)}  {W}x{H}  {len(data)} bytes")
    return 0


if __name__ == "__main__":
    sys.exit(main())
