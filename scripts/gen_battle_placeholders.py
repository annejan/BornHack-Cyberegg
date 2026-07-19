#!/usr/bin/env python3
"""Generate placeholder battle-pose PCX sprites for every pet.

The battle animation (see docs/superpowers/specs/2026-07-19-battle-animation-
design.md) draws two pets side by side. Each pet needs three left-facing poses;
the right combatant is the same art mirrored in software. Real art is made by
the graphics designers — this fills in text-labelled placeholders so the flow
works end to end.

Output: 72x72, 2bpp, RLE PCX with a **transparent** background (palette index
3) and black text (index 0), so two sprites can overlap/mirror cleanly. Matches
the palette `sprite_loader` assumes: 00=black, 01=red, 10=white, 11=transparent.

Files: `PPAA00.PCX` where PP = pet prefix, AA = battle pose code:
  0x20 STAND, 0x21 WON, 0x22 LOST.

Also appends one line per file to assets/to-badge/MANIFEST.TXT.

Usage:  python3 scripts/gen_battle_placeholders.py
"""
import os

from PIL import Image, ImageDraw, ImageFont

W, H = 72, 72
BPL = W // 4  # 2bpp, 4 px per byte -> 18 bytes/line (even, valid PCX)

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ASSET_DIR = os.path.join(REPO, "assets/to-badge")
MANIFEST = os.path.join(ASSET_DIR, "MANIFEST.TXT")

# pet prefix -> short label
PETS = {
    0x00: "BART",
    0x01: "CAT",
    0x02: "SLUG",
    0x05: "PANDA",
    0xFE: "???",
}
# pose code -> label
POSES = {
    0x20: "STAND",
    0x21: "WON",
    0x22: "LOST",
}

BG = 3     # transparent
BLACK = 0  # text


def render_indices(lines):
    """Render centred text -> a W*H array of palette indices (BG/BLACK)."""
    img = Image.new("L", (W, H), 255)  # white canvas for text raster
    draw = ImageDraw.Draw(img)
    font = ImageFont.load_default()

    boxes = [draw.textbbox((0, 0), s, font=font) for s in lines]
    hs = [b[3] - b[1] for b in boxes]
    total_h = sum(hs) + 2 * (len(lines) - 1)
    y = (H - total_h) // 2
    for s, b, h in zip(lines, boxes, hs):
        w = b[2] - b[0]
        x = (W - w) // 2
        draw.text((x - b[0], y - b[1]), s, fill=0, font=font)
        y += h + 2

    px = img.load()
    idx = bytearray(W * H)
    for yy in range(H):
        for xx in range(W):
            idx[yy * W + xx] = BLACK if px[xx, yy] < 128 else BG
    return idx


def pack_row(idx, row):
    out = bytearray()
    base = row * W
    for x in range(0, W, 4):
        b = 0
        for k in range(4):
            b |= (idx[base + x + k] & 3) << (6 - 2 * k)
        out.append(b)
    return out  # BPL bytes


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


def header():
    h = bytearray(128)
    h[0] = 0x0A          # manufacturer (ZSoft)
    h[1] = 5             # version
    h[2] = 1             # RLE encoding
    h[3] = 2             # bits per pixel (per plane)
    # window: xmin,ymin,xmax,ymax (LE u16)
    h[4:6] = (0).to_bytes(2, "little")
    h[6:8] = (0).to_bytes(2, "little")
    h[8:10] = (W - 1).to_bytes(2, "little")
    h[10:12] = (H - 1).to_bytes(2, "little")
    h[12:14] = (72).to_bytes(2, "little")  # hdpi
    h[14:16] = (72).to_bytes(2, "little")  # vdpi
    h[65] = 1            # planes
    h[66:68] = BPL.to_bytes(2, "little")   # bytes per line
    h[68:70] = (1).to_bytes(2, "little")   # palette type
    return h


def write_pcx(path, idx):
    data = header()
    for row in range(H):
        data += rle(pack_row(idx, row))
    with open(path, "wb") as f:
        f.write(data)


def main():
    lines_out = []
    for pp, pet in PETS.items():
        for aa, pose in POSES.items():
            name = f"{pp:02X}{aa:02X}00.PCX"
            idx = render_indices([pet, pose])
            write_pcx(os.path.join(ASSET_DIR, name), idx)
            lines_out.append(f"{name} BATTLE_{pet.replace('?', 'X')}_{pose}")
            print("wrote", name)

    # Append manifest lines that aren't already present.
    existing = ""
    if os.path.exists(MANIFEST):
        with open(MANIFEST) as f:
            existing = f.read()
    with open(MANIFEST, "a") as f:
        if existing and not existing.endswith("\n"):
            f.write("\n")
        for line in lines_out:
            if line.split()[0] not in existing:
                f.write(line + "\n")
    print(f"done: {len(lines_out)} placeholder sprites")


if __name__ == "__main__":
    main()
