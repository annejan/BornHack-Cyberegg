#!/usr/bin/env python3
"""Render the README's screenshots into screenshots/ with the simulator.

  readme_shots.py        build the host `guide` bin, then write every picture

Game screens are the player's guide shots (`guide shot NAME`), so each one is
checked to show what its name says. The badge's other apps come from
`guide screen KEYS` (buttons pressed on a fresh badge at noon). pets.png puts
every roster pet's idle frame side by side (`guide sprites`).
"""
import json
import os
import subprocess
import sys
import tempfile

from PIL import Image, ImageDraw, ImageFont

OUT_DIR = "screenshots"
TARGET_DIR = os.environ.get("CARGO_TARGET_DIR", "target")
GUIDE_BIN = f"{TARGET_DIR}/x86_64-unknown-linux-gnu/debug/guide"
FONT = "guide/fonts/PressStart2P-Regular.ttf"

# Guide shots shown in the README (see src/guide/shots.rs for each one).
SHOTS = (
    "egg", "naming", "hatched",
    "day", "night", "floor",
    "stats-needs", "diary", "cause-toast",
    "risk-marker", "refusal-water", "menu-feed-hard",
    "hung-over", "injured", "starving",
    "demo-note", "pre-battle", "to-the-realm",
)

# Other screens: file name -> buttons pressed from boot (U D L R F C X).
SCREENS = {
    "boot": "",
    "clock": "RRRRR",
    "calendar": "RRRRRR",
    "name-tag": "RRRRRRR",
}

# pets.png: one cell per pet, 1:1 pixels before SCALE.
CELL_W, CELL_H, LABEL_H, COLS, SCALE = 160, 80, 20, 3, 3


def build_guide_bin():
    jobs = os.environ.get("CARGO_BUILD_JOBS", "2")
    subprocess.run(["cargo", "build", "--features", "simulator", "--bin", "guide",
                    "-j", jobs], check=True)
    if not os.path.exists(GUIDE_BIN):
        sys.exit(f"readme_shots: {GUIDE_BIN} was not built")


def pets_picture(out):
    with tempfile.TemporaryDirectory() as tmp:
        subprocess.run([GUIDE_BIN, "sprites", "--out", tmp], check=True,
                       stdout=subprocess.DEVNULL)
        with open(os.path.join(tmp, "roster.json")) as f:
            pets = json.load(f)["pets"]
        rows = (len(pets) + COLS - 1) // COLS
        sheet = Image.new("RGB", (COLS * CELL_W, rows * (CELL_H + LABEL_H)), "white")
        draw = ImageDraw.Draw(sheet)
        font = ImageFont.truetype(FONT, 8)
        for i, pet in enumerate(pets):
            x0 = (i % COLS) * CELL_W
            y0 = (i // COLS) * (CELL_H + LABEL_H)
            sprite = Image.open(os.path.join(tmp, f"{pet['prefix']}-idle-0.png")).convert("RGBA")
            sx = x0 + (CELL_W - sprite.width) // 2
            sy = y0 + CELL_H - sprite.height
            sheet.paste(sprite, (sx, sy), sprite)
            w = draw.textlength(pet["name"], font=font)
            draw.text((x0 + (CELL_W - w) / 2, y0 + CELL_H + 6), pet["name"],
                      font=font, fill="black")
        sheet = sheet.resize((sheet.width * SCALE, sheet.height * SCALE), Image.NEAREST)
        sheet.save(out, optimize=True)


def main():
    build_guide_bin()
    os.makedirs(OUT_DIR, exist_ok=True)
    for name in SHOTS:
        subprocess.run([GUIDE_BIN, "shot", name, "--out", OUT_DIR], check=True)
    for name, keys in SCREENS.items():
        subprocess.run([GUIDE_BIN, "screen", keys, "--out", f"{OUT_DIR}/{name}.png"],
                       check=True)
    pets_picture(f"{OUT_DIR}/pets.png")
    print("pets.png")


if __name__ == "__main__":
    main()
