#!/usr/bin/env python3
"""Tests for the state-frame flow of scripts/bornpets_ai_frames.py.

No real API call: subprocess.run is patched wherever call_api can be reached, and
the key is a fake token in a tmp dir outside the repo.
"""
import base64
import contextlib
import io
import json
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

os.environ["BORNPETS_PET"] = "panda"
os.environ.pop("BORNPETS_MODEL", None)
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from PIL import Image  # noqa: E402

import bornpets_ai_frames as afm  # noqa: E402
import bornpets_art_check as ac  # noqa: E402

REAL_RUN = subprocess.run          # kept for the per-pet subprocess check
TOKEN = "sk-test-UNIQUE-TOKEN-0123456789"
SCRIPTS = Path(__file__).resolve().parent


def canvas_png_b64() -> str:
    buf = io.BytesIO()
    Image.new("RGBA", afm.CANVAS, afm.KEY).save(buf, "PNG")
    return base64.b64encode(buf.getvalue()).decode()


class FakeRun:
    """Records every curl call and answers with a magenta CANVAS PNG."""

    def __init__(self):
        self.calls = []

    def __call__(self, cmd, **kw):
        self.calls.append((list(cmd), kw))
        out = json.dumps({"data": [{"b64_json": canvas_png_b64()}]}).encode()
        return subprocess.CompletedProcess(cmd, 0, out, b"")


def make_key_file(directory, mode=0o600, text=TOKEN):
    p = Path(directory) / "key"
    p.write_text(text + "\n")
    p.chmod(mode)
    return p


class Base(unittest.TestCase):
    def setUp(self):
        self._env = mock.patch.dict(os.environ)
        self._env.start()
        for k in ("OPENAI_API_KEY", "OPENAI_API_KEY_FILE"):
            os.environ.pop(k, None)
        self.addCleanup(self._env.stop)
        self._kf = mock.patch.object(afm, "KEY_FILE", None)
        self._kf.start()
        self.addCleanup(self._kf.stop)


class KeyHandling(Base):
    def test_key_never_reaches_argv_or_output(self):
        with tempfile.TemporaryDirectory() as t:
            kf = make_key_file(t)
            fake = FakeRun()
            out, err = io.StringIO(), io.StringIO()
            with mock.patch.object(subprocess, "run", fake), \
                    contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
                afm.KEY_FILE = str(kf)
                afm.call_api([], "prompt", "low", afm.CANVAS)
            cmd, kw = fake.calls[0]
            self.assertFalse([a for a in cmd if TOKEN in a])
            self.assertIn(TOKEN, kw["input"].decode())
            self.assertIn("-K", cmd)
            self.assertNotIn(TOKEN, out.getvalue() + err.getvalue())

    def test_key_file_inside_repo_is_refused(self):
        (afm.REPO / "target").mkdir(exist_ok=True)
        d = tempfile.mkdtemp(dir=afm.REPO / "target")
        self.addCleanup(shutil.rmtree, d, True)
        kf = make_key_file(d)
        with self.assertRaises(SystemExit) as cm:
            afm.api_key(str(kf))
        self.assertNotIn(TOKEN, str(cm.exception))
        self.assertIn(str(kf), str(cm.exception))

    def test_key_file_readable_by_others_is_refused(self):
        with tempfile.TemporaryDirectory() as t:
            kf = make_key_file(t, mode=0o644)
            with self.assertRaises(SystemExit) as cm:
                afm.api_key(str(kf))
            self.assertNotIn(TOKEN, str(cm.exception))

    def test_symlink_outside_repo_pointing_inside_is_refused(self):
        (afm.REPO / "target").mkdir(exist_ok=True)
        d = tempfile.mkdtemp(dir=afm.REPO / "target")
        self.addCleanup(shutil.rmtree, d, True)
        real = make_key_file(d)
        with tempfile.TemporaryDirectory() as t:
            link = Path(t, "link")
            link.symlink_to(real)
            with self.assertRaises(SystemExit) as cm:
                afm.api_key(str(link))
            self.assertNotIn(TOKEN, str(cm.exception))

    def test_key_with_odd_characters_is_refused_without_echo(self):
        with tempfile.TemporaryDirectory() as t:
            kf = make_key_file(t, text='abc"\nheader = x')
            with self.assertRaises(SystemExit) as cm:
                afm.api_key(str(kf))
            self.assertNotIn("abc", str(cm.exception))

    def test_curl_does_not_inherit_the_key_variable(self):
        fake = FakeRun()
        os.environ["OPENAI_API_KEY"] = TOKEN
        with mock.patch.object(subprocess, "run", fake):
            afm.call_api([], "p", "low", afm.CANVAS)
        self.assertNotIn("OPENAI_API_KEY", fake.calls[0][1]["env"])

    def test_key_source_order(self):
        with tempfile.TemporaryDirectory() as t:
            a = Path(t, "a")
            a.write_text("from-arg\n")
            a.chmod(0o600)
            g = Path(t, "g")
            g.write_text("from-global\n")
            g.chmod(0o600)
            e = Path(t, "e")
            e.write_text("from-envfile\n")
            e.chmod(0o600)
            os.environ["OPENAI_API_KEY"] = "from-env"
            os.environ["OPENAI_API_KEY_FILE"] = str(e)
            self.assertEqual(afm.api_key(str(a)), "from-arg")
            afm.KEY_FILE = str(g)
            self.assertEqual(afm.api_key(), "from-global")
            afm.KEY_FILE = None
            self.assertEqual(afm.api_key(), "from-envfile")
            del os.environ["OPENAI_API_KEY_FILE"]
            self.assertEqual(afm.api_key(), "from-env")

    def test_default_model_is_current(self):
        self.assertEqual(afm.MODEL, "gpt-image-2.6-sunburst")


class StateFlow(Base):
    def test_state_request_sends_idle_first_then_table_refs(self):
        with tempfile.TemporaryDirectory() as t:
            refs = afm.state_refs("06", "throwing_up", Path(t))
            self.assertEqual([r.stem for r in refs],
                             ["060100", "060300", "060C00", "062300"])
            fake = FakeRun()
            with mock.patch.object(subprocess, "run", fake):
                os.environ["OPENAI_API_KEY"] = TOKEN
                afm.call_api(refs, "p", "low", afm.CANVAS)
            cmd = fake.calls[0][0]
            imgs = [a for a in cmd if a.startswith("image[]=@")]
            self.assertEqual([Path(a.split("@", 1)[1].split(";")[0]).stem for a in imgs],
                             ["060100", "060300", "060C00", "062300"])
            self.assertIn("size=1536x1024", cmd)

    def test_every_state_has_a_brief_and_two_to_four_refs(self):
        self.assertEqual(len(ac.STATES), 13)
        for s in ac.STATES:
            self.assertIn(s, afm.STATE_BRIEFS)
            self.assertTrue(afm.STATE_BRIEFS[s].endswith("Nothing else in the frame."), s)
            self.assertGreaterEqual(len(afm.STATE_REFS[s]), 2)
            self.assertLessEqual(len(afm.STATE_REFS[s]), 4)
            for prefix, *_ in ac.ART_PETS.values():
                for aa in (0x01,) + tuple(afm.STATE_REFS[s]):
                    rows = afm.ref_rows(prefix, aa)       # to-badge, else the pet's sheet
                    self.assertEqual((len(rows), len(rows[0])), (75, 152), f"{prefix}{aa:02X}")
        self.assertEqual(set(afm.STATE_REFS), set(ac.STATES))
        self.assertEqual(set(afm.STATE_BRIEFS), set(ac.STATES))

    def test_ref_falls_back_to_the_sheet(self):
        with tempfile.TemporaryDirectory() as t:
            with mock.patch.object(afm, "TO_BADGE", Path(t)):
                rows = afm.ref_rows("05", 0x06)
                self.assertEqual((len(rows), len(rows[0])), (75, 152))
                self.assertTrue({v for r in rows for v in r} <= {0, 1, 2, 3})
                self.assertGreater(sum(v != 3 for r in rows for v in r), 100)
                self.assertEqual(afm.ref_rows("05", 0x07),
                                 ac.decode_pcx(ac.TO_BADGE / "050700.PCX")[2])
                p = Path(t) / "r.png"
                afm.ref_png("05", 0x06, p)
                self.assertEqual(Image.open(p).size, afm.CANVAS)
        # with the file present the file wins
        self.assertEqual(afm.ref_rows("05", 0x01), ac.decode_pcx(ac.TO_BADGE / "050100.PCX")[2])

    def test_every_prompt_asks_for_flat_fills(self):
        self.assertIn("FLAT FILLS, SMALL FILE", afm.FLAT_FILL)
        self.assertTrue(afm.FLAT_FILL.endswith("Solid white stays solid white."))
        for s in ac.STATES:
            self.assertIn(afm.FLAT_FILL, afm.state_prompt(s), s)
            note = "Owner words here."
            p = afm.state_prompt(s, note)
            self.assertLess(p.index(afm.FLAT_FILL), p.index(note), s)
            self.assertGreater(p.index(afm.FLAT_FILL), p.index(afm.style()), s)

    def test_new_state_briefs_follow_the_owner(self):
        self.assertIn("holding on", afm.STATE_BRIEFS["needs_rehab"])
        self.assertIn("starved", afm.STATE_BRIEFS["starving"])
        self.assertEqual(afm.STATE_REFS["needs_rehab"], (0x27, 0x04, 0x0B))
        self.assertEqual(afm.STATE_REFS["starving"], (0x05, 0x09, 0x0C))
        self.assertEqual(afm.STATE_REFS["washing"], (0x0E, 0x0F))
        self.assertIn("tub", afm.STATE_BRIEFS["washing"])

    def test_prompt_shape(self):
        p = afm.state_prompt("wired", "bigger can")
        self.assertIn(afm.IDENTITY, p)
        self.assertIn("IMAGE 1 is this panda in its idle pose", p)
        self.assertIn("IMAGES 2 to 4", p)
        self.assertIn(afm.STATE_BRIEFS["wired"].format(pet="panda"), p)
        self.assertTrue(p.endswith("Owner feedback on the previous attempt: bigger can"))
        self.assertNotIn("Owner feedback", afm.state_prompt("wired"))

    def test_ref_png_puts_the_frame_in_the_fixed_rectangle(self):
        with tempfile.TemporaryDirectory() as t:
            p = Path(t) / "r.png"
            afm.ref_png("05", 0x01, p)
            im = Image.open(p).convert("RGBA")
        self.assertEqual(im.size, afm.CANVAS)
        ox, oy = afm.REF_ORIGIN
        self.assertEqual((ox, oy), (312, 287))
        px = im.load()
        for y in range(im.height):
            for x in range(im.width):
                if not (ox <= x < ox + 912 and oy <= y < oy + 450):
                    self.assertEqual(px[x, y], afm.KEY)
        self.assertNotEqual(px[ox + 456, oy + 300], afm.KEY)   # the panda is inside

    @staticmethod
    def synthetic_raw(halo=True):
        """A whole pet on a magenta CANVAS: white body, black outline, red paws, a soft dark halo."""
        raw = Image.new("RGBA", afm.CANVAS, afm.KEY)
        px = raw.load()
        for y in range(180, 860):
            for x in range(500, 1100):
                edge = x < 508 or x >= 1092 or y < 188 or y >= 852
                px[x, y] = (0, 0, 0, 255) if edge else (255, 255, 255, 255)
        for y in range(780, 850):
            for x in range(540, 700):
                px[x, y] = (255, 0, 0, 255)
        if halo:
            for k in range(1, 40):      # a ring fading from black out into the key colour
                v = (k * 250 // 40, 0, k * 250 // 40, 255)
                for y in range(180 - k, 860 + k):
                    px[500 - k, y] = v
                    px[1099 + k, y] = v
                for x in range(500 - k, 1100 + k):
                    px[x, 180 - k] = v
                    px[x, 859 + k] = v
        return raw

    @staticmethod
    def art_box(img):
        return Image.eval(img.getchannel("A"), lambda a: 255 if a >= 128 else 0).getbbox()

    def test_cut_pet_fits_the_pet_inside_the_idle_framing(self):
        idle = (32, 1, 117, 74)                       # panda idle bbox, exclusive
        for halo in (False, True):
            out = afm.cut_pet(self.synthetic_raw(halo), idle)
            self.assertEqual(out.size, ac.FRAME)
            x0, y0, x1, y1 = self.art_box(out)
            self.assertGreater(x0, 0)
            self.assertLess(x1, ac.FRAME[0])
            self.assertGreater(y0, 0)
            self.assertLess(y1, ac.FRAME[1])
            self.assertLessEqual(y1 - y0, idle[3] - idle[1])
            self.assertEqual(y1, idle[3])             # bottom-aligned to idle
            self.assertLessEqual(abs((x0 + x1) - (idle[0] + idle[2])), 1)   # centred on idle
            self.assertLessEqual(set(out.getdata()),
                                 {afm.BLACK, afm.WHITE, afm.RED, afm.TRANSPARENT})

    def test_cut_pet_clamps_a_pet_whose_idle_sits_at_the_right_edge(self):
        idle = (100, 5, 152, 70)                      # like the slug: idle touches the right edge
        x0, y0, x1, y1 = self.art_box(afm.cut_pet(self.synthetic_raw(), idle))
        self.assertLessEqual(x1, 152)
        self.assertGreaterEqual(x0, 0)
        self.assertEqual(y1, 70)

    def test_cut_pet_halo_does_not_enlarge_the_pet(self):
        idle = (32, 1, 117, 74)
        plain = afm.cut_pet(self.synthetic_raw(False), idle)
        haloed = afm.cut_pet(self.synthetic_raw(True), idle)
        self.assertEqual(self.art_box(plain), self.art_box(haloed))

    def test_cut_pet_of_an_empty_raw_is_a_blank_frame(self):
        out = afm.cut_pet(Image.new("RGBA", afm.CANVAS, afm.KEY), (32, 1, 117, 74))
        self.assertEqual(set(out.getdata()), {afm.TRANSPARENT})

    # --- ink_resize: thin black lines must survive a ~10x shrink ---------------

    @staticmethod
    def blacks_per_row(img, rows, cols=None):
        px = img.load()
        cols = cols if cols is not None else range(img.width)
        return [[x for x in cols if px[x, y] == afm.BLACK] for y in rows]

    def test_ink_resize_keeps_a_thin_line_split_over_two_pixels(self):
        # 5 px line at 1/10 scale = half a badge pixel, split 0.3 / 0.2 over columns 49 and 50.
        src = Image.new("RGBA", (1000, 1000), (255, 255, 255, 255))
        src.paste((0, 0, 0, 255), (497, 0, 502, 1000))
        out = afm.ink_resize(src, (100, 100))
        # Rows 0 and 99 border the outside of the image, which counts as transparent
        # (silhouette), so only the inner rows show the plain line rule.
        for xs in self.blacks_per_row(out, range(1, 99)):
            self.assertEqual(len(xs), 1)
            self.assertIn(xs[0], (49, 50))

    def test_ink_resize_line_split_evenly_is_one_pixel_wide_not_two(self):
        src = Image.new("RGBA", (1000, 1000), (255, 255, 255, 255))
        src.paste((0, 0, 0, 255), (495, 0, 505, 1000))     # 1 badge px wide, 0.5 / 0.5
        out = afm.ink_resize(src, (100, 100))
        self.assertTrue(all(len(xs) == 1 for xs in self.blacks_per_row(out, range(1, 99))))
        src = src.transpose(Image.Transpose.ROTATE_90)       # same for a horizontal line
        out = afm.ink_resize(src, (100, 100)).transpose(Image.Transpose.ROTATE_270)
        self.assertTrue(all(len(xs) == 1 for xs in self.blacks_per_row(out, range(1, 99))))

    def test_ink_resize_keeps_a_thin_diagonal_line_connected(self):
        src = Image.new("RGBA", (1000, 1000), (255, 255, 255, 255))
        px = src.load()
        for y in range(1000):
            for x in range(max(0, y - 3), min(1000, y + 4)):   # ~7 px wide diagonal
                px[x, y] = (0, 0, 0, 255)
        out = afm.ink_resize(src, (100, 100))
        prev = None
        for y, xs in enumerate(self.blacks_per_row(out, range(2, 98))):
            self.assertTrue(xs, f"row {y + 2} lost the line")
            if prev is not None:
                self.assertLessEqual(min(abs(a - b) for a in xs for b in prev), 1)
            prev = xs

    def test_ink_resize_keeps_a_thin_outline_on_the_silhouette_closed(self):
        # White disc with a 5 px black rim on transparent: every opaque pixel that
        # touches transparency must be black, or the outline has a gap.
        src = Image.new("RGBA", (1000, 1000), (0, 0, 0, 0))
        px = src.load()
        for y in range(1000):
            for x in range(1000):
                d2 = (x - 500) ** 2 + (y - 500) ** 2
                if d2 < 400 ** 2:
                    px[x, y] = (0, 0, 0, 255) if d2 >= 395 ** 2 else (255, 255, 255, 255)
        out = afm.ink_resize(src, (100, 100))
        op = out.load()
        gaps = [(x, y) for y in range(1, 99) for x in range(1, 99)
                if op[x, y] not in (afm.TRANSPARENT, afm.BLACK)
                and any(op[x + dx, y + dy] == afm.TRANSPARENT
                        for dx, dy in ((1, 0), (-1, 0), (0, 1), (0, -1)))]
        self.assertEqual(gaps, [])

    def test_ink_resize_does_not_grow_black_fills_or_eat_colour(self):
        src = Image.new("RGBA", (1000, 1000), (0, 0, 0, 0))
        src.paste((255, 255, 255, 255), (100, 100, 900, 900))
        src.paste((0, 0, 0, 255), (200, 200, 400, 400))      # black block: 20x20 badge px
        src.paste((220, 20, 30, 255), (600, 600, 800, 800))  # red block
        out = afm.ink_resize(src, (100, 100))
        op = out.load()
        self.assertEqual(sum(op[x, y] == afm.BLACK for y in range(100) for x in range(100)), 400)
        self.assertEqual(sum(op[x, y] == afm.RED for y in range(100) for x in range(100)), 400)
        self.assertEqual(op[5, 5], afm.TRANSPARENT)
        self.assertEqual(op[50, 30], afm.WHITE)
        self.assertLessEqual(set(out.getdata()),
                             {afm.BLACK, afm.WHITE, afm.RED, afm.TRANSPARENT})

    def test_cut_pet_keeps_a_thin_outline_closed(self):
        raw = Image.new("RGBA", afm.CANVAS, afm.KEY)
        px = raw.load()
        for y in range(180, 860):
            for x in range(500, 1100):
                edge = x < 505 or x >= 1095 or y < 185 or y >= 855   # 5 px rim, ~0.5 badge px
                px[x, y] = (0, 0, 0, 255) if edge else (255, 255, 255, 255)
        self.assertEqual(self.outline_gaps(afm.cut_pet(raw, (32, 1, 117, 74))), [])

    @staticmethod
    def outline_gaps(out):
        """White pixels touching transparency or the frame edge: holes in the outline."""
        op = out.load()
        w, h = out.size
        return [(x, y) for y in range(h) for x in range(w)
                if op[x, y] == afm.WHITE
                and any(not (0 <= x + dx < w and 0 <= y + dy < h) or op[x + dx, y + dy] == afm.TRANSPARENT
                        for dx, dy in ((1, 0), (-1, 0), (0, 1), (0, -1)))]

    def test_quantize_one_keeps_a_thin_outline_closed(self):
        # The sheet-frame path (gen/strip -> quantize) shrinks the same way as cut_pet.
        raw = Image.new("RGBA", (1024, 1024), afm.KEY)
        px = raw.load()
        for y in range(100, 900):
            for x in range(100, 900):
                edge = x < 105 or x >= 895 or y < 105 or y >= 895   # 5 px rim, ~0.5 badge px
                px[x, y] = (0, 0, 0, 255) if edge else (255, 255, 255, 255)
        out = afm.quantize_one(raw, (75, 75))
        self.assertEqual(self.outline_gaps(out), [])
        self.assertIn(afm.WHITE, set(out.getdata()))

    def test_close_outline_blackens_only_white_on_the_silhouette(self):
        # Finished pixel art with no raw: a white square whose outline has a gap,
        # a red block on the edge, a white block touching the frame edge.
        img = Image.new("RGBA", (40, 30), afm.TRANSPARENT)
        img.paste(afm.BLACK, (5, 5, 25, 25))
        img.paste(afm.WHITE, (6, 6, 24, 24))
        img.paste(afm.WHITE, (10, 5, 14, 6))                 # 4 px gap in the top outline
        img.paste(afm.RED, (28, 10, 32, 14))                 # red art without outline stays red
        img.paste(afm.WHITE, (34, 20, 40, 30))               # white touching the frame edge
        out = afm.close_outline(img)
        op = out.load()
        self.assertEqual([op[x, 5] for x in range(10, 14)], [afm.BLACK] * 4)
        self.assertEqual(op[12, 6], afm.WHITE)               # interior unchanged
        self.assertEqual(op[30, 12], afm.RED)
        self.assertEqual(op[39, 29], afm.WHITE)              # frame edge is not silhouette
        self.assertEqual(op[34, 25], afm.BLACK)              # its side facing transparency is
        changed = sum(a != b for a, b in zip(img.getdata(), out.getdata()))
        self.assertEqual(changed, 4 + 6 + 10 - 1)            # gap + top + left of the edge block (shared corner)

    def test_idle_box_is_the_bbox_of_the_pets_own_idle_pcx(self):
        w, h, rows = ac.decode_pcx(ac.TO_BADGE / "050100.PCX")
        xs = [x for r in rows for x, v in enumerate(r) if v != 3]
        ys = [y for y, r in enumerate(rows) if any(v != 3 for v in r)]
        self.assertEqual(afm.idle_box("05"), (min(xs), min(ys), max(xs) + 1, max(ys) + 1))

    def test_rows_box_ignores_tiny_corner_marks(self):
        rows = [[3] * 152 for _ in range(75)]
        for x, y in [(0, 0), (1, 0), (2, 0), (149, 0), (150, 0), (151, 0), (150, 1), (151, 1)]:
            rows[y][x] = 0
        for y in range(30, 61):
            for x in range(70, 91):
                rows[y][x] = 0
        self.assertEqual(afm.rows_box(rows), (70, 30, 91, 61))

    def test_idle_box_of_the_cat_is_its_small_body_not_the_corner_marks(self):
        x0, y0, x1, y1 = afm.idle_box("01")
        self.assertGreater(x0, 10)
        self.assertGreater(y0, 10)
        self.assertLess(y1 - y0, 45)

    def test_merge_islands_merges_enclosed_specks(self):
        img = Image.new("RGBA", (12, 12), afm.TRANSPARENT)
        for y in range(2, 10):
            for x in range(2, 10):
                img.putpixel((x, y), afm.WHITE)
        img.putpixel((5, 5), afm.BLACK)
        img.putpixel((6, 5), afm.BLACK)
        out = afm.merge_islands(img, 3)
        self.assertEqual(out.getpixel((5, 5)), afm.WHITE)
        self.assertEqual(out.getpixel((6, 5)), afm.WHITE)

    def test_merge_islands_never_touches_the_outline(self):
        img = Image.new("RGBA", (12, 12), afm.TRANSPARENT)
        for y in range(2, 10):
            for x in range(2, 10):
                img.putpixel((x, y), afm.WHITE)
        img.putpixel((2, 5), afm.BLACK)     # on the silhouette edge, next to transparency
        img.putpixel((0, 0), afm.RED)       # on the frame edge
        out = afm.merge_islands(img, 3)
        self.assertEqual(out.getpixel((2, 5)), afm.BLACK)
        self.assertEqual(out.getpixel((0, 0)), afm.RED)
        self.assertEqual(out.getpixel((5, 11)), afm.TRANSPARENT)

    def test_merge_islands_keeps_a_diagonal_stroke(self):
        img = Image.new("RGBA", (14, 14), afm.TRANSPARENT)
        for y in range(2, 12):
            for x in range(2, 12):
                img.putpixel((x, y), afm.WHITE)
        for i in range(6):
            img.putpixel((4 + i, 4 + i), afm.BLACK)
        out = afm.merge_islands(img, 3)
        self.assertEqual([out.getpixel((4 + i, 4 + i)) for i in range(6)], [afm.BLACK] * 6)

    def test_merge_islands_keeps_a_plus_whole(self):
        img = Image.new("RGBA", (14, 14), afm.TRANSPARENT)
        for y in range(2, 12):
            for x in range(2, 12):
                img.putpixel((x, y), afm.WHITE)
        cells = [(6, 6), (5, 6), (7, 6), (6, 5), (6, 7)]
        for c in cells:
            img.putpixel(c, afm.BLACK)
        img.putpixel((6, 6), afm.RED)
        for m in (3, 5, 9):
            out = afm.merge_islands(img, m)
            self.assertEqual(list(out.getdata()), list(img.getdata()), m)

    def test_merge_islands_edge_rule_alone(self):
        img = Image.new("RGBA", (8, 8), afm.WHITE)      # no transparency anywhere
        img.putpixel((0, 4), afm.BLACK)                 # speck on the frame edge
        img.putpixel((4, 4), afm.BLACK)                 # speck inside
        out = afm.merge_islands(img, 3)
        self.assertEqual(out.getpixel((0, 4)), afm.BLACK)
        self.assertEqual(out.getpixel((4, 4)), afm.WHITE)

    def test_merge_islands_leaves_a_mixed_neighbourhood(self):
        img = Image.new("RGBA", (6, 3), afm.WHITE)
        for y in range(3):
            img.putpixel((3, y), afm.BLACK)       # a black wall right of x=2
        img.putpixel((2, 1), afm.RED)             # red speck touching white and black
        out = afm.merge_islands(img, 3)
        self.assertEqual(out.getpixel((2, 1)), afm.RED)


class StateCommand(Base):
    def run_state(self, work, note=None, extra=(), runner=None):
        argv = ["x", "--work", str(work), "state", "--quality", "low", *extra]
        if note is not None:
            nf = Path(work) / "notes.json"
            if not nf.exists() or json.loads(nf.read_text()) != note:
                nf.write_text(json.dumps(note))
            argv += ["--note-file", str(nf)]
        fake = runner or FakeRun()
        os.environ["OPENAI_API_KEY"] = TOKEN
        with mock.patch.object(subprocess, "run", fake), \
                mock.patch.object(sys, "argv", argv), \
                contextlib.redirect_stdout(io.StringIO()):
            afm.main()
        return fake

    def test_note_file_skips_ok_frames_and_adds_notes(self):
        with tempfile.TemporaryDirectory() as t:
            fake = self.run_state(t, {"panda_tipsy_0": "ok", "panda_wired_0": "bigger can",
                                      "floor_crumbs": "too dark"})
            self.assertEqual(len(fake.calls), 1)
            prompt = [a for a in fake.calls[0][0] if a.startswith("prompt=")][0]
            self.assertTrue(prompt.endswith("bigger can"))
            self.assertTrue(Path(t, "frames/panda_wired_0.png").is_file())
            self.assertFalse(Path(t, "frames/panda_tipsy_0.png").exists())
            self.assertFalse(Path(t, "frames/panda_sore_0.png").exists())   # absent: skipped

    def test_cached_raw_is_skipped_unless_forced(self):
        with tempfile.TemporaryDirectory() as t:
            self.assertEqual(len(self.run_state(t, extra=["--states", "tipsy,sore"]).calls), 2)
            self.assertEqual(len(self.run_state(t, extra=["--states", "tipsy,sore"]).calls), 0)
            self.assertEqual(len(self.run_state(t, extra=["--states", "tipsy", "--force"]).calls), 1)

    def test_rerun_never_undoes_simplify(self):
        with tempfile.TemporaryDirectory() as t:
            self.run_state(t, extra=["--states", "tipsy"])
            frame = Path(t, "frames/panda_tipsy_0.png")
            marked = Image.new("RGBA", ac.FRAME, afm.WHITE)
            marked.save(frame)
            self.assertEqual(len(self.run_state(t, extra=["--states", "tipsy"]).calls), 0)
            self.assertEqual(list(Image.open(frame).convert("RGBA").getdata()),
                             list(marked.getdata()))
            self.run_state(t, extra=["--states", "tipsy", "--force"])
            self.assertNotEqual(list(Image.open(frame).convert("RGBA").getdata()),
                                list(marked.getdata()))

    def test_missing_frame_is_cut_from_cached_raw_without_a_call(self):
        with tempfile.TemporaryDirectory() as t:
            self.run_state(t, extra=["--states", "tipsy"])
            Path(t, "frames/panda_tipsy_0.png").unlink()
            self.assertEqual(len(self.run_state(t, extra=["--states", "tipsy"]).calls), 0)
            self.assertTrue(Path(t, "frames/panda_tipsy_0.png").is_file())

    def test_same_note_file_twice_costs_nothing_the_second_time(self):
        with tempfile.TemporaryDirectory() as t:
            note = {"panda_wired_0": "bigger can"}
            self.assertEqual(len(self.run_state(t, note).calls), 1)
            self.assertEqual(len(self.run_state(t, note).calls), 0)

    def test_force_with_a_note_file_pays_once(self):
        with tempfile.TemporaryDirectory() as t:
            note = {"panda_wired_0": "bigger can"}
            self.assertEqual(len(self.run_state(t, note, extra=["--force"]).calls), 1)
            self.assertEqual(len(self.run_state(t, note, extra=["--force"]).calls), 0)

    def test_malformed_note_file_is_a_clean_exit(self):
        with tempfile.TemporaryDirectory() as t:
            Path(t, "notes.json").write_text("{nope")
            argv = ["x", "--work", t, "state", "--note-file", str(Path(t, "notes.json"))]
            with mock.patch.object(sys, "argv", argv), self.assertRaises(SystemExit) as cm:
                afm.main()
            self.assertIn("notes.json", str(cm.exception))

    def test_note_newer_than_cached_raw_redraws(self):
        with tempfile.TemporaryDirectory() as t:
            note = {"panda_wired_0": "bigger can"}
            self.run_state(t, note)
            future = Path(t, "raw/panda_wired_0.png").stat().st_mtime + 100
            nf = Path(t, "notes.json")
            os.utime(nf, (future, future))
            argv = ["x", "--work", t, "state", "--quality", "low", "--note-file", str(nf)]
            fake = FakeRun()
            with mock.patch.object(subprocess, "run", fake), mock.patch.object(sys, "argv", argv), \
                    contextlib.redirect_stdout(io.StringIO()):
                afm.main()
            self.assertEqual(len(fake.calls), 1)

    def test_ok_note_is_case_and_dot_insensitive(self):
        with tempfile.TemporaryDirectory() as t:
            fake = self.run_state(t, {"panda_wired_0": "OK.", "panda_sore_0": " ok "})
            self.assertEqual(len(fake.calls), 0)

    def test_one_failing_frame_does_not_stop_the_others_and_exits_1(self):
        good = FakeRun()

        def flaky(cmd, **kw):
            if any("prompt=Draw" in a and "Tipsy." in a for a in cmd):
                raise ValueError("bad json")
            return good(cmd, **kw)

        with tempfile.TemporaryDirectory() as t:
            with self.assertRaises(SystemExit) as cm:
                self.run_state(t, extra=["--states", "tipsy,sore"], runner=flaky)
            self.assertEqual(cm.exception.code, 1)
            self.assertEqual(len(good.calls), 1)
            self.assertTrue(Path(t, "frames/panda_sore_0.png").is_file())
            self.assertFalse(Path(t, "frames/panda_tipsy_0.png").exists())

    def test_simplify_rewrites_frames_in_place(self):
        with tempfile.TemporaryDirectory() as t:
            (Path(t) / "frames").mkdir()
            img = Image.new("RGBA", ac.FRAME, afm.TRANSPARENT)
            for y in range(10, 30):
                for x in range(10, 30):
                    img.putpixel((x, y), afm.WHITE)
            img.putpixel((20, 20), afm.BLACK)
            img.save(Path(t) / "frames/panda_tipsy_0.png")
            with mock.patch.object(sys, "argv", ["x", "--work", t, "simplify",
                                                 "--only", "panda_tipsy_0", "--min-island", "2"]), \
                    contextlib.redirect_stdout(io.StringIO()):
                afm.main()
            out = Image.open(Path(t) / "frames/panda_tipsy_0.png").convert("RGBA")
            self.assertEqual(out.getpixel((20, 20)), afm.WHITE)
            self.assertFalse((Path(t) / "overrides").exists())

    def test_list_states_prints_twelve_ids(self):
        buf = io.StringIO()
        with mock.patch.object(sys, "argv", ["x", "list", "--states"]), \
                contextlib.redirect_stdout(buf):
            afm.main()
        ids = buf.getvalue().strip().split(",")
        self.assertEqual(len(ids), 13)
        self.assertEqual(ids[0], "panda_tipsy_0")
        self.assertEqual(ids[-1], "panda_washing_0")


class V3StateRows(unittest.TestCase):
    def test_frame_index_has_the_states(self):
        for pet, (_prefix, cfg, _sheet, seq) in ac.ART_PETS.items():
            seqs = afm.load_sequences(ac.REPO / "assets" / cfg)
            self.assertEqual(len(seqs), 54, pet)
            for i, state in enumerate(ac.STATES):
                s = seqs[0x29 + i]
                self.assertEqual(
                    (s.name, s.frames, s.frame_width, s.frame_height),
                    (f"{seq}_{state}", 1, 152, 75), f"{pet} row {0x29 + i:#x}")
        # frame_index() reads the pet chosen at import (panda)
        idx = afm.frame_index()
        for state in ac.STATES:
            self.assertIn(f"panda_{state}_0", idx)


class Profiles(unittest.TestCase):
    def test_profiles_load_and_pair_only_when_anchored(self):
        code = (
            "import sys; sys.path.insert(0, %r)\n"
            "import bornpets_ai_frames as afm, bornpets_art_check as ac\n"
            "pet=sys.argv[1]\n"
            "idx=afm.frame_index()\n"
            "assert idx, 'no frames'\n"
            "paired=any('panda_id' in v for v in idx.values())\n"
            "assert paired == ('anchor_prompt' in afm.PROFILE), (pet, paired)\n"
            "assert any(k.startswith(ac.ART_PETS[pet][3] + '_') for k in idx) if pet in ac.ART_PETS else True\n"
            % str(SCRIPTS)
        )
        # the Jellyfish is gone (plan 7); the six art pets (the Slug since A4b) are loaded here.
        for pet in ("bartholomeus", "cat", "slug", "panda", "badger", "seastar"):
            env = {k: v for k, v in os.environ.items() if k != "BORNPETS_PET"}
            env["BORNPETS_PET"] = pet
            res = REAL_RUN([sys.executable, "-I", "-c", code, pet],
                           env=env, capture_output=True, text=True)
            self.assertEqual(res.returncode, 0, f"{pet}: {res.stdout}{res.stderr}")


if __name__ == "__main__":
    unittest.main()
