#!/usr/bin/env python3
"""Tests for scripts/bornpets_litter.py. No real API call: subprocess.run is patched
wherever call_api can be reached; the key is a fake token in a tmp dir outside the repo."""
import base64
import contextlib
import io
import json
import os
import re
import subprocess
import sys
import tempfile
import time
import unittest
from pathlib import Path
from unittest import mock

os.environ["BORNPETS_PET"] = "panda"
os.environ.pop("BORNPETS_MODEL", None)
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from PIL import Image  # noqa: E402

import bornpets_ai_frames as afm  # noqa: E402
import bornpets_litter as lit  # noqa: E402

REAL_RUN = subprocess.run
TOKEN = "sk-test-UNIQUE-TOKEN-0123456789"
BLACK, WHITE, CLEAR, RED = (0, 0, 0, 255), (255, 255, 255, 255), (0, 0, 0, 0), (255, 0, 0, 255)


def canvas_png_b64() -> str:
    buf = io.BytesIO()
    Image.new("RGBA", lit.CANVAS, lit.KEY).save(buf, "PNG")
    return base64.b64encode(buf.getvalue()).decode()


class FakeRun:
    def __init__(self, fail=False):
        self.calls = []
        self.fail = fail

    def __call__(self, cmd, **kw):
        self.calls.append((list(cmd), kw))
        if self.fail:
            return subprocess.CompletedProcess(cmd, 1, b"", b"boom")
        out = json.dumps({"data": [{"b64_json": canvas_png_b64()}]}).encode()
        return subprocess.CompletedProcess(cmd, 0, out, b"")


def images_of(cmd):
    return [Path(a.split("@", 1)[1].split(";")[0]).name for a in cmd if a.startswith("image[]=@")]


def prompt_of(cmd):
    return next(a for a in cmd if a.startswith("prompt="))


def make_key_file(directory, mode=0o600):
    p = Path(directory) / "key"
    p.write_text(TOKEN + "\n")
    p.chmod(mode)
    return p


def boxed(item, fn):
    """A KEY canvas; fn(img, x0, y0, S, w, h) draws into the item box."""
    w, h = lit.SLOTS[item][2:]
    s = lit.item_scale(item)
    x0, y0, _, _ = lit.item_box(item)
    canvas = Image.new("RGBA", lit.CANVAS, lit.KEY)
    fn(canvas, x0, y0, s, w, h)
    return canvas


def fill(img, x, y, w, h, colour):
    img.paste(colour, (x, y, x + w, y + h))


class Base(unittest.TestCase):
    def setUp(self):
        env = mock.patch.dict(os.environ)
        env.start()
        self.addCleanup(env.stop)
        for k in ("OPENAI_API_KEY", "OPENAI_API_KEY_FILE"):
            os.environ.pop(k, None)
        kf = mock.patch.object(afm, "KEY_FILE", None)
        kf.start()
        self.addCleanup(kf.stop)
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.dir = Path(self.tmp.name)
        patch = mock.patch.object(lit, "LITTER_DIR", self.dir / "litter")
        patch.start()
        self.addCleanup(patch.stop)
        self.key = make_key_file(self.dir)

    def run_cli(self, *args, fake=None):
        fake = fake or FakeRun()
        out, err = io.StringIO(), io.StringIO()
        code = 0
        with mock.patch.object(subprocess, "run", fake), contextlib.redirect_stdout(out), \
                contextlib.redirect_stderr(err):
            try:
                lit.main(["--key-file", str(self.key), *args])
            except SystemExit as e:
                code = e.code if isinstance(e.code, int) else 1
        return code, fake, out.getvalue() + err.getvalue()


class Geometry(unittest.TestCase):
    def test_slots_fit_the_strip_and_never_meet(self):
        self.assertEqual(list(lit.SLOTS), list(lit.OLD_SLOTS))
        self.assertEqual(len(lit.SLOTS), 5)
        boxes = list(lit.SLOTS.values())
        for x, y, w, h in boxes:
            self.assertTrue(0 <= x and x + w <= 152, (x, w))
            self.assertTrue(100 <= y and y + h <= 110, (y, h))
        for i, a in enumerate(boxes):
            for b in boxes[i + 1:]:
                apart = a[0] + a[2] <= b[0] or b[0] + b[2] <= a[0] or a[1] + a[3] <= b[1] or b[1] + b[3] <= a[1]
                self.assertTrue(apart, (a, b))

    def test_default_slots_total_92(self):
        self.assertEqual(sum(h * ((w + 7) // 8) for _, _, w, h in lit.SLOTS.values()), 92)

    def test_scales(self):
        self.assertEqual({i: lit.item_scale(i) for i in lit.ITEMS},
                         {"crumbs": 116, "peel": 87, "can": 90, "sock": 90, "pizza_box": 58})

    def test_sketch_is_plan_7_in_the_fixed_box(self):
        with tempfile.TemporaryDirectory() as t:
            p = Path(t, "s.png")
            lit.sketch_png("can", p)
            im = Image.open(p).convert("RGBA")
            self.assertEqual(im.size, lit.CANVAS)
            x0, y0, bw, bh = lit.item_box("can")
            s = lit.item_scale("can")
            rows = lit.PLAN7_SKETCH["can"]
            w, h = lit.SLOTS["can"][2:]
            px = im.load()
            for y in range(lit.CANVAS[1]):
                for x in range(0, lit.CANVAS[0], 7):
                    if not (x0 <= x < x0 + bw and y0 <= y < y0 + bh):
                        self.assertEqual(px[x, y], lit.KEY)
            for cy in range(h):
                for cx in range(w):
                    bit = rows[cy * len(rows) // h][cx * len(rows[0]) // w] == "1"
                    if bit:
                        self.assertEqual(px[x0 + cx * s + s // 2, y0 + cy * s + s // 2], BLACK)

    def test_plan7_sketch_matches_old_slots(self):
        for item, (x, y, w, h) in lit.OLD_SLOTS.items():
            rows = lit.PLAN7_SKETCH[item]
            self.assertEqual((len(rows[0]), len(rows)), (w, h))
            self.assertTrue(all(len(r) == w for r in rows))

    def test_ref_canvas_fits_the_largest_scale(self):
        with tempfile.TemporaryDirectory() as t:
            p = Path(t, "r.png")
            lit.ref_canvas("001700", p)
            im = Image.open(p)
            self.assertEqual(im.size, lit.CANVAS)


class Prompts(unittest.TestCase):
    def test_prompt_has_the_pieces(self):
        txt = lit.litter_prompt("sock", "a red stripe")
        self.assertTrue(txt.startswith("IMAGE 1 is a rough sketch of a "))
        self.assertIn(lit.LITTER_BRIEFS["sock"], txt)
        self.assertIn("14 x 10 grid", txt)
        self.assertTrue(txt.endswith("Owner feedback on the previous attempt: a red stripe"))
        self.assertNotIn("Owner feedback", lit.litter_prompt("sock"))


class Cutting(unittest.TestCase):
    def test_cut_item_keeps_a_thin_outline(self):
        def draw(img, x0, y0, s, w, h):
            fill(img, x0, y0, w * s, h * s, WHITE)
            t = s // 3
            fill(img, x0, y0, w * s, t, BLACK)
            fill(img, x0, y0 + h * s - t, w * s, t, BLACK)
            fill(img, x0, y0, t, h * s, BLACK)
            fill(img, x0 + w * s - t, y0, t, h * s, BLACK)

        out, red = lit.cut_item(boxed("pizza_box", draw), "pizza_box")
        w, h = lit.SLOTS["pizza_box"][2:]
        self.assertEqual(out.size, (w, h))
        self.assertEqual(red, 0)
        px = out.load()
        for y in range(h):
            for x in range(w):
                edge = x in (0, w - 1) or y in (0, h - 1)
                self.assertEqual(px[x, y], BLACK if edge else WHITE, (x, y))

    def test_cut_item_ignores_a_line_under_the_threshold(self):
        def draw(img, x0, y0, s, w, h):
            fill(img, x0, y0, w * s, h * s, WHITE)
            fill(img, x0, y0, w * s, s // 5, BLACK)

        out, _ = lit.cut_item(boxed("pizza_box", draw), "pizza_box")
        self.assertFalse([p for p in out.getdata() if p == BLACK])

    def test_cut_item_threshold_is_a_quarter(self):
        def line(frac):
            def draw(img, x0, y0, s, w, h):
                fill(img, x0, y0, w * s, h * s, WHITE)
                fill(img, x0, y0, w * s, round(s * frac), BLACK)
            return draw

        under, _ = lit.cut_item(boxed("pizza_box", line(0.22)), "pizza_box")
        over, _ = lit.cut_item(boxed("pizza_box", line(0.28)), "pizza_box")
        self.assertFalse([p for p in under.getdata() if p == BLACK])
        self.assertTrue(all(over.getpixel((x, 0)) == BLACK for x in range(24)))

    def test_cut_item_snaps_red_and_counts_it(self):
        def draw(img, x0, y0, s, w, h):
            fill(img, x0, y0, w * s, h * s, WHITE)
            for cx in (2, 3, 7):
                fill(img, x0 + cx * s, y0 + 2 * s, s, s, RED)

        out, red = lit.cut_item(boxed("pizza_box", draw), "pizza_box")
        self.assertEqual(red, 3)
        for cx in (2, 3, 7):
            self.assertEqual(out.getpixel((cx, 2)), BLACK)

    def test_cut_item_keeps_small_crumbs(self):
        def draw(img, x0, y0, s, w, h):
            fill(img, x0, y0, w * s, h * s, CLEAR)
            for a in (0, 4, 8):
                fill(img, x0 + a * s, y0 + 2 * s, 3 * s, 3 * s, WHITE)
                fill(img, x0 + (a + 1) * s, y0 + 3 * s, 2 * s, 2 * s, BLACK)
            fill(img, x0 + 11 * s, y0, s, s, BLACK)       # a lone speck

        out, _ = lit.cut_item(boxed("crumbs", draw), "crumbs")
        for a in (0, 4, 8):
            self.assertEqual(out.getpixel((a + 1, 3)), BLACK)
        self.assertEqual(out.getpixel((11, 0))[3], 0)


def solid(w, h, black):
    """w x h image: white fill, black cells listed as (x, y)."""
    im = Image.new("RGBA", (w, h), WHITE)
    for xy in black:
        im.putpixel(xy, BLACK)
    return im


def outlined(w, h, t=1, bottom_gap=0):
    im = Image.new("RGBA", (w, h), CLEAR)
    bh = h - bottom_gap
    im.paste(WHITE, (0, 0, w, bh))
    for i in range(t):
        for x in range(w):
            im.putpixel((x, i), BLACK)
            im.putpixel((x, bh - 1 - i), BLACK)
        for y in range(bh):
            im.putpixel((i, y), BLACK)
            im.putpixel((w - 1 - i, y), BLACK)
    return im


class Drift(unittest.TestCase):
    def test_drift_flags_speck_floating_and_thick(self):
        self.assertEqual(lit.item_drift(outlined(12, 8), 0)["flags"], [])
        im = Image.new("RGBA", (12, 8), CLEAR)
        im.putpixel((3, 7), BLACK)
        self.assertIn("speck", lit.item_drift(im, 0)["flags"])
        self.assertIn("floating", lit.item_drift(outlined(12, 8, bottom_gap=1), 0)["flags"])
        self.assertIn("outline", lit.item_drift(outlined(12, 10, t=2), 0)["flags"])
        d = lit.item_drift(outlined(12, 8), 2)
        self.assertEqual(d["flags"], ["red"])
        self.assertEqual(set(d), {"outline", "coverage", "bottom", "ink_pct", "red", "flags"})


class Packing(unittest.TestCase):
    def test_ink_rows_pad_to_bytes(self):
        data = lit.ink_rows(Image.new("RGBA", (12, 5), BLACK))
        self.assertEqual(data, bytes([0xFF, 0xF0] * 5))

    def test_strip_places_items(self):
        items = {i: Image.new("RGBA", lit.SLOTS[i][2:], BLACK) for i in lit.ITEMS}
        s = lit.strip(items)
        self.assertEqual(s.size, (152, 10))
        x, y, w, h = lit.SLOTS["can"]
        self.assertEqual(s.getpixel((x, y - 100)), BLACK)
        self.assertEqual(s.getpixel((x + w, y - 100))[3], 0)

    def _items(self):
        items = {}
        for n, i in enumerate(lit.ITEMS):
            w, h = lit.SLOTS[i][2:]
            im = Image.new("RGBA", (w, h), CLEAR)
            for y in range(h):
                for x in range(w):
                    if (x * 3 + y * 5 + n) % 3 == 0:
                        im.putpixel((x, y), BLACK)
                    elif (x + y) % 2:
                        im.putpixel((x, y), WHITE)
            items[i] = im
        return items

    def test_rust_source_round_trips(self):
        items = self._items()
        text = lit.rust_source(lit.strip(items))
        self.assertTrue(text.startswith("//! Generated by scripts/bornpets_litter.py"))
        rects = re.findall(r"Rectangle::new\(Point::new\((\d+), (\d+)\), Size::new\((\d+), (\d+)\)\)", text)
        self.assertEqual([tuple(map(int, r)) for r in rects], list(lit.SLOTS.values()))
        ink = text.split("FLOOR_ITEMS] = [")[2]
        blobs = re.findall(r"&\[(.*?)\]", ink, re.S)
        total = 0
        for item, blob in zip(lit.ITEMS, blobs):
            got = bytes(int(b, 16) for b in re.findall(r"0x([0-9A-Fa-f]{2})", blob))
            self.assertEqual(got, lit.ink_rows(items[item]))
            total += len(got)
        self.assertEqual(len(blobs), 5)
        self.assertEqual(total, sum(h * ((w + 7) // 8) for _, _, w, h in lit.SLOTS.values()))


class Cli(Base):
    def test_generate_sends_the_sketch_first_then_refs(self):
        code, fake, _ = self.run_cli("generate", "--items", "sock")
        self.assertEqual(code, 0)
        self.assertEqual(len(fake.calls), 1)
        self.assertEqual(images_of(fake.calls[0][0]),
                         ["sock_sketch.png", "001700.png", "021700.png", "051000.png"])
        self.assertTrue((lit.LITTER_DIR / "raw/sock.png").is_file())
        self.assertTrue((lit.LITTER_DIR / "items/sock.png").is_file())
        m = json.loads((lit.LITTER_DIR / "metrics.json").read_text())
        self.assertEqual(set(m), {"sock"})
        self.assertEqual(set(m["sock"]), {"slot", "bytes", "flags", "metrics"})
        self.assertEqual(m["sock"]["slot"], list(lit.SLOTS["sock"]))
        self.assertEqual(m["sock"]["bytes"], 20)
        self.assertTrue((lit.LITTER_DIR / "plan7/sock.png").is_file())
        # cached: no second call
        _, fake2, _ = self.run_cli("generate", "--items", "sock")
        self.assertEqual(len(fake2.calls), 0)

    def test_litter_uses_the_same_key_rules(self):
        self.key.chmod(0o644)
        code, fake, out = self.run_cli("generate", "--items", "sock")
        self.assertNotEqual(code, 0)
        self.assertEqual(fake.calls, [])
        self.assertNotIn(TOKEN, out)
        self.key.chmod(0o600)
        code, fake, out = self.run_cli("generate", "--items", "sock")
        cmd, kw = fake.calls[0]
        self.assertFalse([a for a in cmd if TOKEN in a])
        self.assertIn(TOKEN, kw["input"].decode())
        self.assertNotIn(TOKEN, out)

    def test_note_file_skips_ok_items(self):
        note = self.dir / "note.json"
        note.write_text(json.dumps({"floor_can": "ok", "floor_sock": "a red stripe",
                                    "panda_tipsy_0": "bigger"}))
        code, fake, _ = self.run_cli("generate", "--note-file", str(note))
        self.assertEqual(code, 0)
        self.assertEqual(len(fake.calls), 1)
        self.assertEqual(images_of(fake.calls[0][0])[0], "sock_sketch.png")
        self.assertIn("a red stripe", prompt_of(fake.calls[0][0]))
        # the raw is newer than the note: same note file costs nothing
        _, fake2, _ = self.run_cli("generate", "--note-file", str(note))
        self.assertEqual(len(fake2.calls), 0)
        # a newer note redraws
        time.sleep(0.05)
        note.write_text(json.dumps({"floor_sock": "smaller"}))
        os.utime(note, (time.time() + 5, time.time() + 5))
        _, fake3, _ = self.run_cli("generate", "--note-file", str(note))
        self.assertEqual(len(fake3.calls), 1)

    def test_force_with_a_note_file_pays_once(self):
        note = self.dir / "note.json"
        note.write_text(json.dumps({"floor_sock": "a red stripe"}))
        _, fake, _ = self.run_cli("generate", "--force", "--note-file", str(note))
        self.assertEqual(len(fake.calls), 1)
        _, fake2, _ = self.run_cli("generate", "--force", "--note-file", str(note))
        self.assertEqual(len(fake2.calls), 0)

    def test_malformed_note_file_is_a_clean_exit(self):
        note = self.dir / "note.json"
        note.write_text("{nope")
        code, fake, out = self.run_cli("generate", "--note-file", str(note))
        self.assertEqual(code, 1)
        self.assertEqual(fake.calls, [])

    def test_rust_without_git_refuses(self):
        with mock.patch.object(lit, "SOURCE", self.dir / "strip.png"), \
                mock.patch.object(lit, "RUST_OUT", self.dir / "o.rs"):
            def nogit(*a, **k):
                raise FileNotFoundError("git")
            code = self.run_cli("rust", fake=nogit)[0]
        self.assertEqual(code, 1)
        self.assertFalse((self.dir / "o.rs").exists())

    def test_a_failed_item_exits_1_after_the_others(self):
        code, fake, out = self.run_cli("generate", "--items", "sock,can", fake=FakeRun(fail=True))
        self.assertEqual(code, 1)
        self.assertEqual(len(fake.calls), 2)
        self.assertNotIn(TOKEN, out)

    def test_assemble_writes_strip_and_source(self):
        self.run_cli("generate")
        src = self.dir / "strip.png"
        with mock.patch.object(lit, "SOURCE", src):
            code, _, _ = self.run_cli("assemble")
        self.assertEqual(code, 0)
        self.assertEqual(Image.open(src).size, (152, 10))
        self.assertEqual(Image.open(lit.LITTER_DIR / "strip.png").size, (152, 10))

    def _git(self, repo, *args):
        env = {**os.environ, "GIT_AUTHOR_NAME": "t", "GIT_AUTHOR_EMAIL": "t@t",
               "GIT_COMMITTER_NAME": "t", "GIT_COMMITTER_EMAIL": "t@t"}
        REAL_RUN(["git", "-C", str(repo), *args], check=True, capture_output=True, env=env)

    def test_rust_refuses_an_uncommitted_strip(self):
        repo = self.dir / "repo"
        repo.mkdir()
        self._git(repo, "init", "-q")
        src, out = repo / "strip.png", repo / "litter_art.rs"
        lit.strip({i: Image.new("RGBA", lit.SLOTS[i][2:], BLACK) for i in lit.ITEMS}).save(src)
        with mock.patch.object(lit, "SOURCE", src), mock.patch.object(lit, "RUST_OUT", out):
            code = self.run_cli("rust", fake=REAL_RUN)[0]
            self.assertEqual(code, 1)                      # untracked
            self.assertFalse(out.exists())
            self._git(repo, "add", "strip.png")
            self.assertEqual(self.run_cli("rust", fake=REAL_RUN)[0], 1)   # staged, not committed
            self._git(repo, "commit", "-q", "-m", "x")
            self.assertEqual(self.run_cli("rust", fake=REAL_RUN)[0], 0)   # clean
            self.assertTrue(out.is_file())
            self.assertEqual(self.run_cli("check", fake=REAL_RUN)[0], 0)
            out.write_text("stale")
            self.assertEqual(self.run_cli("check", fake=REAL_RUN)[0], 1)
            Image.new("RGBA", (152, 10), CLEAR).save(src)
            out.unlink()
            self.assertEqual(self.run_cli("rust", fake=REAL_RUN)[0], 1)   # modified after commit
            self.assertFalse(out.exists())


if __name__ == "__main__":
    unittest.main()
