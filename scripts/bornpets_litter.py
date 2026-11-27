#!/usr/bin/env python3
"""BornPets floor litter in the pets' sprite style, via the OpenAI images edit endpoint.

  generate [--items a,b] [--quality q] [--force] [--note-file F]
        sketch (plan 7's bitmap) + three pet frames -> raw/<item>.png -> items/<item>.png
        (the item cut to its slot size, 4 badge colours), plan7/<item>.png, metrics.json.
        --note-file is the gallery feedback JSON: only `floor_<item>` keys whose value is
        not "ok" are redrawn; absent keys are skipped and other keys are ignored. A raw
        newer than the note file already answers it (the same file costs no call).
  assemble       items/*.png -> strip.png (LITTER_DIR) and SOURCE (working tree)
  rust           SOURCE -> RUST_OUT; refuses unless SOURCE is tracked and clean in git
  check          exit 1 unless RUST_OUT equals rust_source(SOURCE)

Working files live in LITTER_DIR (bornpets_art_check.py, which never imports this module,
reads items/, plan7/, strip.png and metrics.json for the owner gallery). The API key
handling is bornpets_ai_frames.py's (--key-file, key only on curl's stdin).
"""
from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
os.environ.setdefault("BORNPETS_PET", "panda")

from PIL import Image  # noqa: E402

import bornpets_ai_frames as afm  # noqa: E402
from bornpets_ai_frames import CANVAS, KEY, dekey, despeckle  # noqa: E402
from bornpets_art_check import FLOOR_ITEMS, LITTER_DIR, TO_BADGE, decode_pcx, edge_runs  # noqa: E402
from bornpets_sheet import BLACK, TRANSPARENT, WHITE  # noqa: E402

REPO = Path(__file__).resolve().parent.parent

ITEMS = FLOOR_ITEMS   # ("crumbs", "peel", "can", "sock", "pizza_box"), plan 7 slot order
SLOTS = {"crumbs": (2, 105, 12, 5), "peel": (116, 101, 16, 9), "can": (16, 100, 12, 10),
         "sock": (134, 100, 14, 10), "pizza_box": (30, 102, 24, 8)}   # x, y, w, h on the screen
OLD_SLOTS = {"crumbs": (4, 107, 7, 3), "peel": (120, 104, 11, 6), "can": (16, 104, 10, 6),
             "sock": (136, 102, 9, 8), "pizza_box": (29, 105, 16, 5)}  # plan 7 at 7fdea40
STRIP_TOP, STRIP = 100, (152, 10)
SOURCE = REPO / "assets/bornpets-litter.png"
RUST_OUT = REPO / "src/game/litter_art.rs"
LITTER_MIN_BLOB = 2   # drops only isolated 1-cell specks; the default 12 would erase crumbs
FIT = (1400, 900)     # the area an item or a reference may fill on the CANVAS

# Plan 7's bitmaps (src/game/background.rs at 7fdea40), one string per row, left column first.
PLAN7_SKETCH: dict[str, tuple[str, ...]] = {
    "crumbs": ("0100010", "1010100", "0010001"),
    "peel": ("00000110000", "00011001001", "01100000101", "10010001010", "10001110010", "01110000000"),
    "can": ("0011111110", "0100000001", "1010011100", "1010011100", "0100000001", "0011111110"),
    "sock": ("011111000", "010001000", "010001000", "010001000", "010001110", "010000010",
             "010000001", "001111111"),
    "pizza_box": ("1111111111111111", "1000000000000001", "1000110011000001",
                  "1000000000000001", "1111111111111111"),
}
# Three shipped frames per item (pet prefix + anim id + frame, as PCX names) for line weight.
LITTER_REFS = {"crumbs": ("001500", "050C00", "021500"), "peel": ("001500", "010C00", "021500"),
               "can": ("052500", "062500", "001600"), "sock": ("001700", "021700", "051000"),
               "pizza_box": ("001500", "060C00", "021500")}
LITTER_NOUNS = {"crumbs": "few bread crumbs", "peel": "banana peel", "can": "drink can",
                "sock": "sock", "pizza_box": "pizza box"}
LITTER_BRIEFS = {
    "crumbs": "A few scattered bread crumbs, three or four small lumps.",
    "peel": "A banana peel lying flat, three floppy peel arms.",
    "can": "An empty drink can lying on its side, a dent in the middle.",
    "sock": "One dirty sock, an L shape, a hole at the toe.",
    "pizza_box": "A flat closed pizza box, a grease stain on the lid.",
}


def item_scale(item: str) -> int:
    w, h = SLOTS[item][2:]
    return min(FIT[0] // w, FIT[1] // h)


def item_box(item: str) -> tuple[int, int, int, int]:
    w, h = SLOTS[item][2:]
    s = item_scale(item)
    return ((CANVAS[0] - w * s) // 2, (CANVAS[1] - h * s) // 2, w * s, h * s)


def _plan7_cells(item: str, w: int, h: int) -> list[list[bool]]:
    rows = PLAN7_SKETCH[item]
    sh, sw = len(rows), len(rows[0])
    return [[rows[y * sh // h][x * sw // w] == "1" for x in range(w)] for y in range(h)]


def sketch_png(item: str, path: Path) -> None:
    """Plan 7's bitmap stretched onto the new slot, in the fixed box of a KEY canvas."""
    w, h = SLOTS[item][2:]
    s = item_scale(item)
    x0, y0, bw, bh = item_box(item)
    canvas = Image.new("RGBA", CANVAS, KEY)
    canvas.paste(WHITE, (x0, y0, x0 + bw, y0 + bh))
    for cy, row in enumerate(_plan7_cells(item, w, h)):
        for cx, on in enumerate(row):
            if on:
                canvas.paste(BLACK, (x0 + cx * s, y0 + cy * s, x0 + (cx + 1) * s, y0 + (cy + 1) * s))
    canvas.save(path)


def ref_canvas(name: str, path: Path) -> None:
    """A shipped frame at the largest integer scale that fits FIT, centred on a KEY canvas."""
    w, h, rows = decode_pcx(TO_BADGE / f"{name}.PCX")
    s = max(1, min(FIT[0] // w, FIT[1] // h))
    origin = ((CANVAS[0] - w * s) // 2, (CANVAS[1] - h * s) // 2)
    afm.key_canvas(w, h, rows, s, origin).save(path)


def litter_prompt(item: str, note: str | None = None) -> str:
    w, h = SLOTS[item][2:]
    text = (
        f"IMAGE 1 is a rough sketch of a {LITTER_NOUNS[item]} lying on the floor, inside the box "
        "it must fill exactly. IMAGES 2 to 4 are frames from the same game: match their black "
        "line weight and flat fills, not their subjects.\n\n"
        f"{LITTER_BRIEFS[item]}\n\n"
        f"Chunky pixel art on a {w} x {h} grid, each grid cell one flat colour. A 1 pixel black "
        "outline all round. Black and white only, no red, no grey, no shading. Nothing outside the box."
    )
    if note:
        text += f"\n\nOwner feedback on the previous attempt: {note}"
    return text


def _is_red(r: int, g: int, b: int) -> bool:
    return r > 140 and g < 110 and b < 110


def _is_dark(r: int, g: int, b: int) -> bool:
    return (r * 299 + g * 587 + b * 114) // 1000 < 96 or _is_red(r, g, b)


def cut_item(raw: Image.Image, item: str) -> tuple[Image.Image, int]:
    """Cut the item box out of a CANVAS result into a w x h image in the badge colours.

    Masks are built at full size (dark: luma < 96 or red, on opaque pixels; opaque; red)
    and BOX-reduced by exactly the scale: a cell is black when >= 25 % of it was dark,
    else white when at least half of it was opaque, else transparent. Red cells count and
    become black.
    """
    assert raw.size == CANVAS, f"expected {CANVAS}, got {raw.size}"
    w, h = SLOTS[item][2:]
    x0, y0, bw, bh = item_box(item)
    crop = dekey(raw.convert("RGBA").crop((x0, y0, x0 + bw, y0 + bh)))
    opaque, dark, red = bytearray(), bytearray(), bytearray()
    for r, g, b, a in crop.getdata():
        on = a >= 128
        opaque.append(255 if on else 0)
        dark.append(255 if on and _is_dark(r, g, b) else 0)
        red.append(255 if on and _is_red(r, g, b) else 0)

    def reduce(mask: bytearray) -> list[int]:
        im = Image.frombytes("L", (bw, bh), bytes(mask))
        return list(im.resize((w, h), Image.BOX).getdata())

    opq, drk, rd = reduce(opaque), reduce(dark), reduce(red)
    out = Image.new("RGBA", (w, h), TRANSPARENT)
    px = out.load()
    nred = 0
    for i in range(w * h):
        x, y = i % w, i // w
        if rd[i] * 4 >= 255:
            nred += 1
        if drk[i] * 4 >= 255:
            px[x, y] = BLACK
        elif opq[i] * 2 >= 255:
            px[x, y] = WHITE
    return despeckle(out, min_blob=LITTER_MIN_BLOB), nred


def _rows(img: Image.Image) -> list[list[int]]:
    """0 black, 2 white, 3 transparent: the shape edge_runs reads."""
    w, h = img.size
    px = img.load()
    out = []
    for y in range(h):
        row = []
        for x in range(w):
            p = px[x, y]
            row.append(3 if p[3] < 128 else 0 if p[:3] == (0, 0, 0) else 2)
        out.append(row)
    return out


def item_drift(img: Image.Image, red: int) -> dict:
    rows = _rows(img)
    w, h = img.size
    outline, coverage = edge_runs(rows)
    blacks = sum(v == 0 for r in rows for v in r)
    bottom = 0 in rows[-1]
    ink_pct = (100 * blacks) // (w * h)
    flags = []
    if outline != 1:
        flags.append("outline")
    if coverage < 90:
        flags.append("coverage")
    if not bottom:
        flags.append("floating")
    if blacks * 100 < 15 * w * h:
        flags.append("speck")
    if red > 0:
        flags.append("red")
    return {"outline": outline, "coverage": coverage, "bottom": bottom,
            "ink_pct": ink_pct, "red": red, "flags": flags}


def strip(items: dict[str, Image.Image]) -> Image.Image:
    out = Image.new("RGBA", STRIP, TRANSPARENT)
    for item, img in items.items():
        x, y = SLOTS[item][:2]
        out.paste(img, (x, y - STRIP_TOP), img)
    return out


def ink_rows(img: Image.Image) -> bytes:
    w, h = img.size
    px = img.load()
    data = bytearray()
    for y in range(h):
        for bx in range((w + 7) // 8):
            byte = 0
            for k in range(8):
                x = bx * 8 + k
                if x < w and px[x, y][3] >= 128 and px[x, y][:3] == (0, 0, 0):
                    byte |= 0x80 >> k
            data.append(byte)
    return bytes(data)


def rust_source(strip_img: Image.Image) -> str:
    lines = [
        "//! Generated by scripts/bornpets_litter.py from assets/bornpets-litter.png. Do not edit.",
        "use embedded_graphics::prelude::*;",
        "use embedded_graphics::primitives::Rectangle;",
        "",
        "use super::engine::scene::FLOOR_ITEMS;",
        "",
        "/// Slot boxes in slot order: crumbs, peel, can, sock, pizza box (bottom row y 109).",
        "pub const FLOOR_SLOTS: [Rectangle; FLOOR_ITEMS] = [",
    ]
    for x, y, w, h in SLOTS.values():
        lines.append(f"    Rectangle::new(Point::new({x}, {y}), Size::new({w}, {h})),")
    lines += [
        "];",
        "/// Black pixels of each item, MSB first, `(width + 7) / 8` bytes a row.",
        "pub const FLOOR_INK: [&[u8]; FLOOR_ITEMS] = [",
    ]
    for item, (x, y, w, h) in SLOTS.items():
        cell = strip_img.crop((x, y - STRIP_TOP, x + w, y - STRIP_TOP + h))
        data = ink_rows(cell)
        chunks = [", ".join(f"0x{b:02X}" for b in data[i:i + 16]) for i in range(0, len(data), 16)]
        if len(chunks) == 1:
            lines.append(f"    &[{chunks[0]}],")
        else:
            lines.append("    &[")
            lines += [f"        {c}," for c in chunks]
            lines.append("    ],")
    lines += ["];", ""]
    return "\n".join(lines)


# ---------------------------------------------------------------- CLI

def plan7_png(item: str) -> Image.Image:
    """Plan 7's old bitmap in its old slot, 1x, on a strip-sized transparent image."""
    x, y, w, h = OLD_SLOTS[item]
    out = Image.new("RGBA", STRIP, TRANSPARENT)
    for cy, row in enumerate(PLAN7_SKETCH[item]):
        for cx, bit in enumerate(row):
            if bit == "1":
                out.putpixel((x + cx, y - STRIP_TOP + cy), BLACK)
    return out


def _metrics_entry(item: str, img: Image.Image, red: int) -> dict:
    d = item_drift(img, red)
    flags = d.pop("flags")
    return {"slot": list(SLOTS[item]), "bytes": len(ink_rows(img)), "flags": flags, "metrics": d}


def _load_metrics(work: Path) -> dict:
    try:
        data = json.loads((work / "metrics.json").read_text())
    except (OSError, ValueError):
        return {}
    return data if isinstance(data, dict) else {}


def cmd_generate(args: argparse.Namespace) -> None:
    work = Path(LITTER_DIR)
    wanted = [i.strip() for i in args.items.split(",") if i.strip()] if args.items else list(ITEMS)
    unknown = [i for i in wanted if i not in ITEMS]
    if unknown:
        sys.exit(f"unknown items: {unknown}; known: {', '.join(ITEMS)}")
    notes = afm.load_notes(args.note_file) if args.note_file else None
    for sub in ("raw", "items", "plan7", "ref"):
        (work / sub).mkdir(parents=True, exist_ok=True)
    metrics = _load_metrics(work)
    failed: list[str] = []
    for n, item in enumerate(wanted, 1):
        key, note = f"floor_{item}", None
        raw_path = work / "raw" / f"{item}.png"
        if notes is not None:
            if key not in notes or afm._is_ok(notes[key]):
                print(f"[{n}/{len(wanted)}] {item}: skipped")
                continue
            note = notes[key].strip()
        generate = afm.needs_call(raw_path, args.note_file, note, args.force)
        try:
            if generate:
                print(f"[{n}/{len(wanted)}] {item}: generating...", flush=True)
                sketch = work / "ref" / f"{item}_sketch.png"
                sketch_png(item, sketch)
                refs = [sketch]
                for name in LITTER_REFS[item]:
                    ref_canvas(name, work / "ref" / f"{name}.png")
                    refs.append(work / "ref" / f"{name}.png")
                raw_path.write_bytes(afm.call_api(refs, litter_prompt(item, note), args.quality, CANVAS))
            else:
                print(f"[{n}/{len(wanted)}] {item}: cutting cached raw")
            img, red = cut_item(Image.open(raw_path).convert("RGBA"), item)
            img.save(work / "items" / f"{item}.png")
            plan7_png(item).save(work / "plan7" / f"{item}.png")
            metrics[item] = _metrics_entry(item, img, red)
        except Exception as exc:   # one bad item must not abandon the others
            failed.append(item)
            print(f"[{n}/{len(wanted)}] {item}: FAILED - {type(exc).__name__}: {str(exc)[:300]}",
                  flush=True)
    ordered = {i: metrics[i] for i in ITEMS if i in metrics}
    (work / "metrics.json").write_text(json.dumps(ordered, indent=1) + "\n")
    if failed:
        print(f"\n{len(failed)} item(s) failed: {', '.join(failed)}")
        sys.exit(1)


def cmd_assemble(args: argparse.Namespace) -> None:
    work = Path(LITTER_DIR)
    missing = [i for i in ITEMS if not (work / "items" / f"{i}.png").is_file()]
    if missing:
        sys.exit(f"no items/<item>.png for: {', '.join(missing)}; run generate first")
    items = {i: Image.open(work / "items" / f"{i}.png").convert("RGBA") for i in ITEMS}
    img = strip(items)
    (work / "plan7").mkdir(parents=True, exist_ok=True)
    for i in ITEMS:
        plan7_png(i).save(work / "plan7" / f"{i}.png")
    img.save(work / "strip.png")
    Path(SOURCE).parent.mkdir(parents=True, exist_ok=True)
    img.save(SOURCE)
    print(f"wrote {work / 'strip.png'} and {SOURCE}")


def source_is_committed() -> bool:
    """True when SOURCE is tracked and has no change against HEAD (staged or not)."""
    src = Path(SOURCE)
    cwd = src.parent
    try:
        tracked = subprocess.run(["git", "ls-files", "--error-unmatch", src.name], cwd=cwd,
                                 capture_output=True)
        if tracked.returncode != 0:
            return False
        clean = subprocess.run(["git", "diff", "--quiet", "HEAD", "--", src.name], cwd=cwd,
                               capture_output=True)
    except OSError:
        return False   # no git: the strip cannot be verified, so it is refused
    return clean.returncode == 0


def cmd_rust(args: argparse.Namespace) -> None:
    if not source_is_committed():
        sys.exit(f"{SOURCE} is not tracked and clean in git; get the strip approved and "
                 "committed before generating firmware source")
    Path(RUST_OUT).write_text(rust_source(Image.open(SOURCE).convert("RGBA")))
    print(f"wrote {RUST_OUT}")


def cmd_check(args: argparse.Namespace) -> None:
    want = rust_source(Image.open(SOURCE).convert("RGBA"))
    try:
        have = Path(RUST_OUT).read_text()
    except OSError:
        have = None
    if have != want:
        sys.exit(f"{RUST_OUT} is out of date; run `bornpets_litter.py rust`")
    print("litter_art.rs is current")


def main(argv=None) -> None:
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--key-file", help="file holding the API key (outside the repo, chmod 600)")
    sub = p.add_subparsers(dest="cmd", required=True)
    g = sub.add_parser("generate")
    g.add_argument("--items", help="comma-separated items (default: all five)")
    g.add_argument("--quality", default="medium", choices=["low", "medium", "high", "max"])
    g.add_argument("--force", action="store_true", help="regenerate cached raws")
    g.add_argument("--note-file", help="JSON {floor_<item>: note}; only non-ok entries are redrawn")
    g.set_defaults(func=cmd_generate)
    sub.add_parser("assemble").set_defaults(func=cmd_assemble)
    sub.add_parser("rust").set_defaults(func=cmd_rust)
    sub.add_parser("check").set_defaults(func=cmd_check)
    args = p.parse_args(argv)
    if args.key_file:
        afm.KEY_FILE = args.key_file
    args.func(args)


if __name__ == "__main__":
    main()
