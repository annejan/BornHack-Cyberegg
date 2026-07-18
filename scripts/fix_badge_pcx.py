#!/usr/bin/env python3
"""Re-pack a 1-plane PCX to the CyberAegg badge format (2bpp, 1-plane, RLE).

Fixes PCX files the badge rejects because they're the wrong colour depth
(e.g. 4bpp/16-colour or 8bpp/256-colour exported by GIMP/other tools).
Pillow can't even open 4bpp PCX ("unknown PCX mode"), so this decodes PCX
itself — no PIL dependency.

Palette indices are re-mapped to the badge's 4 fixed slots by nearest header
palette RGB:  0=black  1=red  2=white  3=magenta→transparent(skip).
Dimensions are PRESERVED (unlike png_to_badge_pcx.py which forces 152x152).

Usage:  fix_badge_pcx.py <in.pcx> <out.PCX>
"""
import struct
import sys

# Badge palette slots (index -> RGB) the decoder hard-maps on the device:
# 0 black, 1 red, 2 white, 3 magenta = transparent/skip.
BADGE = {0: (0, 0, 0), 1: (255, 0, 0), 2: (255, 255, 255), 3: (255, 0, 255)}


def nearest_badge(rgb):
    return min(BADGE, key=lambda i: sum((a - c) ** 2 for a, c in zip(rgb, BADGE[i])))


def rle_decode_line(data, pos, nbytes):
    """Decode one PCX RLE scanline of exactly `nbytes` bytes. Returns (row, new_pos)."""
    out = bytearray()
    while len(out) < nbytes and pos < len(data):
        b = data[pos]
        pos += 1
        if (b & 0xC0) == 0xC0:
            run = b & 0x3F
            if pos >= len(data):
                break
            val = data[pos]
            pos += 1
            out.extend([val] * run)
        else:
            out.append(b)
    while len(out) < nbytes:  # pad short final line
        out.append(0)
    return out[:nbytes], pos


def rle_encode_line(row):
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


def main():
    if len(sys.argv) != 3:
        print(__doc__)
        sys.exit(2)
    raw = open(sys.argv[1], "rb").read()
    if len(raw) < 128:
        sys.exit("not a PCX (too small)")
    hdr = bytearray(raw[:128])
    if hdr[0] != 0x0A:
        sys.exit("not a PCX (bad manufacturer byte)")
    bpp = hdr[3]
    planes = hdr[65]
    if planes != 1:
        sys.exit(f"unsupported: {planes} planes (need 1-plane source)")
    xmin, ymin, xmax, ymax = struct.unpack("<HHHH", hdr[4:12])
    w, h = xmax - xmin + 1, ymax - ymin + 1
    src_bpl = struct.unpack("<H", hdr[66:68])[0]
    # Header palette: 16-colour EGA block lives at bytes 16..64.
    pal16 = [(hdr[16 + i * 3], hdr[17 + i * 3], hdr[18 + i * 3]) for i in range(16)]
    # 8bpp uses the 256-colour trailer (0x0C + 768 bytes) instead.
    pal256 = None
    if bpp == 8 and len(raw) >= 769 and raw[-769] == 0x0C:
        t = raw[-768:]
        pal256 = [(t[i * 3], t[i * 3 + 1], t[i * 3 + 2]) for i in range(256)]

    # index -> badge slot lookup, via whichever palette applies.
    def remap(idx):
        pal = pal256 if pal256 else pal16
        rgb = pal[idx] if idx < len(pal) else (0, 0, 0)
        return nearest_badge(rgb)

    # Decode all scanlines to per-pixel badge indices.
    pos = 128
    rows = []
    for _ in range(h):
        line, pos = rle_decode_line(raw, pos, src_bpl)
        px = []
        if bpp == 4:
            for byte in line:
                px.append(remap((byte >> 4) & 0xF))
                px.append(remap(byte & 0xF))
        elif bpp == 8:
            px = [remap(b) for b in line]
        elif bpp == 2:
            for byte in line:
                for k in range(4):
                    px.append(remap((byte >> (6 - 2 * k)) & 3))
        elif bpp == 1:
            for byte in line:
                for k in range(8):
                    px.append(remap((byte >> (7 - k)) & 1))
        else:
            sys.exit(f"unsupported source bpp={bpp}")
        rows.append(px[:w])

    # Re-pack 2bpp (4 px/byte, MSB-first) -> new bpl.
    out_bpl = (w * 2 + 7) // 8
    body = bytearray()
    for px in rows:
        line = bytearray()
        for x in range(0, w, 4):
            b = 0
            for k in range(4):
                b |= (px[x + k] & 3) << (6 - 2 * k) if x + k < w else 0
            line.append(b)
        assert len(line) == out_bpl
        body += rle_encode_line(line)

    # Patch header: 2bpp, 1 plane, new bpl, badge palette in the EGA block.
    hdr[3] = 2
    hdr[65] = 1
    hdr[66:68] = struct.pack("<H", out_bpl)
    for i in range(16):
        r, g, b = BADGE.get(i, (0, 0, 0))
        hdr[16 + i * 3], hdr[17 + i * 3], hdr[18 + i * 3] = r, g, b

    open(sys.argv[2], "wb").write(bytes(hdr) + bytes(body))
    print(f"ok: {w}x{h}  {bpp}bpp -> 2bpp  bpl {src_bpl} -> {out_bpl}  "
          f"({len(hdr) + len(body)} bytes)  -> {sys.argv[2]}")


if __name__ == "__main__":
    main()
