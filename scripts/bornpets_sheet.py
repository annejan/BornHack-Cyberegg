#!/usr/bin/env python3
"""Frame geometry for asset-gen bornpets sprite sheets.

Mirrors the layout in ../asset-gen/src/template.rs so Python tooling can read
and write individual frames of a `bornpets-sprites-*.png` sheet.

Layout per `sequence` asset, stacked top to bottom:

    y            +-- LABEL_HEIGHT (22px) label band --------------+
    y+22         | [border] frame 0 [border] | [border] frame 1 ...|

Each frame occupies a slot of `frame_width + 2*BORDER` pixels, and the frame
content starts at `slot_x + BORDER, block_y + LABEL_HEIGHT + BORDER`.
"""

from __future__ import annotations

import re
from dataclasses import dataclass
from pathlib import Path

LABEL_HEIGHT = 22
BORDER = 1

# asset-gen palette (RGBA) after classification in template.rs::sample_frame
TRANSPARENT = (0, 0, 0, 0)
BLACK = (0, 0, 0, 255)
WHITE = (255, 255, 255, 255)
RED = (255, 0, 0, 255)


@dataclass
class Sequence:
    name: str
    frames: int
    frame_width: int
    frame_height: int
    block_y: int

    @property
    def slot_w(self) -> int:
        # text_width for the frame-number label never exceeds frame_width+2 for
        # the sizes used here, so the box width always wins.
        return self.frame_width + 2 * BORDER

    def frame_rect(self, index: int) -> tuple[int, int, int, int]:
        """(x, y, w, h) of frame `index`'s content area in sheet pixels."""
        x = index * self.slot_w + BORDER
        y = self.block_y + LABEL_HEIGHT + BORDER
        return (x, y, self.frame_width, self.frame_height)


_ENTRY = re.compile(
    r"\{\s*type:\s*\"sequence\",\s*"
    r"name:\s*\"(?P<name>[^\"]+)\",\s*"
    r"(?:comment:\s*\"[^\"]*\",\s*)?"
    r"frames:\s*(?P<frames>\d+),\s*"
    r"frame_width:\s*(?P<w>\d+),\s*"
    r"frame_height:\s*(?P<h>\d+),\s*\}",
    re.S,
)


_LINE_COMMENT = re.compile(r"^\s*//.*$", re.M)


def load_sequences(config_path: str | Path) -> list[Sequence]:
    """Parse the `assets` list of a bornpets json5 config, in sheet order."""
    # Strip `//` comments first. The entry regex matches the fields in a fixed
    # order, so a comment line between two of them used to drop the whole
    # sequence — and a missing sequence silently shifts the block_y of every
    # later row, which reads as artwork landing in the wrong place rather than
    # as a parse error.
    text = _LINE_COMMENT.sub("", Path(config_path).read_text(encoding="utf-8"))
    seqs: list[Sequence] = []
    y = 0
    for m in _ENTRY.finditer(text):
        w, h = int(m["w"]), int(m["h"])
        seqs.append(
            Sequence(
                name=m["name"],
                frames=int(m["frames"]),
                frame_width=w,
                frame_height=h,
                block_y=y,
            )
        )
        y += LABEL_HEIGHT + h + 2 * BORDER

    declared = len(re.findall(r'type:\s*"sequence"', text))
    if declared != len(seqs):
        raise ValueError(
            f"{config_path}: parsed {len(seqs)} of {declared} sequences — an "
            "entry does not match the expected field order "
            "(name, comment, frames, frame_width, frame_height)"
        )
    return seqs


def canvas_size(seqs: list[Sequence]) -> tuple[int, int]:
    w = max(s.frames * s.slot_w for s in seqs)
    h = sum(LABEL_HEIGHT + s.frame_height + 2 * BORDER for s in seqs)
    return (w, h)


if __name__ == "__main__":
    import sys

    seqs = load_sequences(sys.argv[1])
    for s in seqs:
        print(f"{s.name:28} y={s.block_y:5} {s.frames}x {s.frame_rect(0)}")
    print("canvas:", canvas_size(seqs))
