#!/usr/bin/env python3
"""Unit tests for scripts/bornpets_art_check.py (no API calls, no repo-local state)."""
import json
import os
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from PIL import Image  # noqa: E402

import bornpets_art_check as ac  # noqa: E402
from fix_badge_pcx import rle_encode_line  # noqa: E402


def write_pcx(path, rows):
    h, w = len(rows), len(rows[0])
    bpl = (w * 2 + 7) // 8
    hdr = bytearray(128)
    hdr[0], hdr[1], hdr[2], hdr[3] = 0x0A, 5, 1, 2
    hdr[8:10] = (w - 1).to_bytes(2, "little")
    hdr[10:12] = (h - 1).to_bytes(2, "little")
    hdr[65] = 1
    hdr[66:68] = bpl.to_bytes(2, "little")
    body = bytearray()
    for row in rows:
        line = bytearray()
        for x in range(0, w, 4):
            b = 0
            for k in range(4):
                if x + k < w:
                    b |= (row[x + k] & 3) << (6 - 2 * k)
            line.append(b)
        body += rle_encode_line(line)
    Path(path).write_bytes(bytes(hdr) + bytes(body))


def box(w=30, h=20, outline=2, ox=40, oy=20, fw=152, fh=75):
    """A white box with a black outline on a transparent frame."""
    rows = [[3] * fw for _ in range(fh)]
    for y in range(h):
        for x in range(w):
            edge = min(x, y, w - 1 - x, h - 1 - y) < outline
            rows[oy + y][ox + x] = 0 if edge else 2
    return rows


def pad_file(path, size):
    Path(path).write_bytes(b"\0" * size)


class Checker(unittest.TestCase):
    def test_decode_round_trips_a_written_pcx(self):
        rows = [[(x * 7 + y * 3 + x // 5) % 4 for x in range(152)] for y in range(75)]
        with tempfile.TemporaryDirectory() as t:
            p = Path(t) / "a.PCX"
            write_pcx(p, rows)
            self.assertEqual(ac.decode_pcx(p), (152, 75, rows))

    def test_clusters_at_the_boundary(self):
        self.assertEqual([ac.clusters(n) for n in (0, 1, 2048, 2049)], [0, 1, 1, 2])

    def test_states_are_thirteen_with_ids(self):
        self.assertEqual(len(ac.STATES), 13)
        self.assertEqual(ac.STATES[10], "needs_rehab")
        self.assertEqual(ac.STATES[11], "starving")
        self.assertEqual(ac.STATES[12], "washing")
        self.assertEqual(ac.FIRST_STATE_ANIM + 10, 0x33)
        self.assertEqual(len(ac.ART_PETS), 6)
        self.assertEqual(ac.ART_PETS["slug"],
                         ("02", "bornpets-sponsors-slug.json5", "bornpets-sprites-slug.png", "slug"))
        self.assertEqual(set(ac.INVENTORY_PETS), set(ac.ART_PETS))

    def test_new_files_are_seventy_eight(self):
        names = ac.new_files()
        self.assertEqual(len(names), 78)
        self.assertEqual(names, sorted(names))
        for n in ("022900.PCX", "023400.PCX", "003300.PCX", "073400.PCX", "003500.PCX", "073500.PCX"):
            self.assertIn(n, names)
        for n in names:
            self.assertIn(int(n[2:4], 16), range(0x29, 0x36))

    def test_existing_slug_files_stay_protected(self):
        for n in ("020100.PCX", "021500.PCX", "022800.PCX", "002000.PCX", "040600.PCX"):
            self.assertTrue(ac.is_protected(n), n)
        for n in ("022900.PCX", "023400.PCX"):
            self.assertFalse(ac.is_protected(n), n)

    def test_diff_fails_on_a_changed_protected_file(self):
        with tempfile.TemporaryDirectory() as o, tempfile.TemporaryDirectory() as n:
            Path(o, "020100.PCX").write_bytes(b"old")
            Path(n, "020100.PCX").write_bytes(b"new")
            res = ac.diff_exports(o, n)
            self.assertEqual(res["protected_changed"], ["020100.PCX"])
            self.assertEqual(ac.main(["diff", o, n]), 1)
        with tempfile.TemporaryDirectory() as o, tempfile.TemporaryDirectory() as n:
            Path(o, "020100.PCX").write_bytes(b"same")
            Path(n, "020100.PCX").write_bytes(b"same")
            Path(n, "022900.PCX").write_bytes(b"new")
            res = ac.diff_exports(o, n)
            self.assertEqual(res["protected_changed"], [])
            self.assertEqual(res["added"], ["022900.PCX"])
            self.assertEqual(ac.main(["diff", o, n]), 0)
        with tempfile.TemporaryDirectory() as o, tempfile.TemporaryDirectory() as n:
            Path(o, "021500.PCX").write_bytes(b"gone")      # in old, not in new
            self.assertEqual(ac.diff_exports(o, n)["protected_changed"], ["021500.PCX"])

    def test_start_screens_010000_and_020000_are_retired_000000_is_not(self):
        for n in ("010000.PCX", "020000.PCX"):
            self.assertIn(n, ac.RETIRED)
            self.assertFalse(ac.is_protected(n), n)
        self.assertNotIn("000000.PCX", ac.RETIRED)
        self.assertTrue(ac.is_protected("020100.PCX"))
        # a retired file that vanishes is "removed", not protected_changed
        with tempfile.TemporaryDirectory() as o, tempfile.TemporaryDirectory() as n:
            for name in ("010000.PCX", "020000.PCX"):
                Path(o, name).write_bytes(b"old")
            res = ac.diff_exports(o, n)
            self.assertEqual(res["removed"], ["010000.PCX", "020000.PCX"])
            self.assertEqual(res["protected_changed"], [])

    def test_retired_are_drained_and_third_relax_frames(self):
        want = ["000600.PCX", "000A00.PCX", "010600.PCX", "010601.PCX", "010A00.PCX", "010A01.PCX",
                "050600.PCX", "050601.PCX", "050A00.PCX", "050A01.PCX", "060600.PCX", "060601.PCX",
                "060A00.PCX", "060A01.PCX", "070600.PCX", "070601.PCX", "070A00.PCX", "070A01.PCX",
                "050E02.PCX", "060E02.PCX", "070E02.PCX", "010000.PCX", "020000.PCX"]
        self.assertEqual(list(ac.RETIRED), sorted(want))
        self.assertEqual(len(ac.RETIRED), 23)
        for n in ac.RETIRED:
            self.assertTrue(n == "020000.PCX" or not n.startswith("02"), n)
            self.assertFalse(ac.is_protected(n), n)
            self.assertNotIn(n, ac.new_files())

    def _volume(self, t, pcx_clusters=393, with_retired=False):
        """404 clusters in all: PCX + BPETS_V3.CFG (6) + MANIFEST.TXT (5)."""
        for i in range(pcx_clusters):
            pad_file(Path(t, f"09{i // 256:02X}{i % 256:02X}.PCX"), 100)
        pad_file(Path(t, "BPETS_V3.CFG"), 11483)
        pad_file(Path(t, "MANIFEST.TXT"), 8563)
        if with_retired:
            for n in ac.RETIRED:
                pad_file(Path(t, n), 100)

    def test_limit_from_the_measured_volume(self):
        with tempfile.TemporaryDirectory() as t:
            self._volume(t)
            res = ac.volume_limit(t)
            self.assertEqual(res["limit"], 80)
            self.assertEqual((res["used"], res["retired_present"], res["base"]), (404, 0, 404))
            self.assertEqual((res["cfg_growth"], res["manifest_growth"]), (1, 1))
        with tempfile.TemporaryDirectory() as t:
            self._volume(t, with_retired=True)
            res = ac.volume_limit(t)
            self.assertEqual(res["limit"], 80)
            self.assertEqual((res["used"], res["retired_present"], res["base"]), (427, 23, 404))

    def test_limit_counts_the_retired_manifest_lines(self):
        with tempfile.TemporaryDirectory() as t:
            self._volume(t, with_retired=True)
            lines = "".join(f"{n} X\n" for n in ac.RETIRED)
            Path(t, "MANIFEST.TXT").write_text(lines + "x" * (8563 - len(lines)))
            self.assertEqual(ac.volume_limit(t)["manifest_growth"], 1)

    def test_manifest_lines_for_the_new_files(self):
        lines = ac.manifest_lines()
        self.assertEqual(len(lines), 78)
        self.assertIn("022900.PCX SLUG_TIPSY_F1\n", lines)
        self.assertIn("003300.PCX BARTHO_NEEDS_REHAB_F1\n", lines)
        self.assertEqual(sum(len(l) for l in lines), 2209)
        self.assertEqual([l.split()[0] for l in lines], ac.new_files())

    def test_pcx_bytes_matches_exported_files(self):
        # Not written by asset-gen: the two start screens and the menu icons 0406xx / 0407xx.
        hand_made = ("000000", "010000", "0406", "0407")
        n = 0
        for f in sorted(ac.TO_BADGE.glob("0[0-7]????.PCX")):
            if f.name.startswith(hand_made):
                continue
            _, _, rows = ac.decode_pcx(f)
            self.assertEqual(ac.pcx_bytes(rows), f.stat().st_size, f.name)
            n += 1
        self.assertGreater(n, 300)

    def test_fit_notes_only_for_over_frames(self):
        with tempfile.TemporaryDirectory() as t:
            for name in ac.new_files():
                pad_file(Path(t, name), 1000)
            pad_file(Path(t, "063200.PCX"), 2300)
            res = ac.budget(t)
            self.assertEqual(res["over"], ["063200.PCX"])
            self.assertEqual(ac.fit_notes(res), {"badger_throwing_up_0": ac.FIT_NOTE.format(bytes=2300)})
            out = ac.fit_notes(res, {"badger_throwing_up_0": "Owner words.", "cat_tipsy_0": "unused"})
            self.assertEqual(out, {"badger_throwing_up_0": "Owner words. " + ac.FIT_NOTE.format(bytes=2300)})
            dest = Path(t, "sub", "notes.json")
            owner = Path(t, "owner.json")
            owner.write_text(json.dumps({"badger_throwing_up_0": "Owner words."}))
            self.assertEqual(ac.main(["fit-notes", t, "--owner-notes", str(owner), "-o", str(dest)]), 0)
            self.assertEqual(json.loads(dest.read_text()), out)

    def test_diff_flags_changed_removed_and_foreign_added(self):
        with tempfile.TemporaryDirectory() as o, tempfile.TemporaryDirectory() as n:
            for name in ("000100.PCX", "000200.PCX", "010100.PCX"):
                Path(o, name).write_bytes(b"abc")
            Path(n, "000100.PCX").write_bytes(b"abd")   # changed
            Path(n, "010100.PCX").write_bytes(b"abc")   # same; 000200 removed
            Path(n, "060E05.PCX").write_bytes(b"x")     # added, not a shipped name
            res = ac.diff_exports(o, n)
            self.assertEqual(res, {"changed": ["000100.PCX"], "removed": ["000200.PCX"],
                                   "added": ["060E05.PCX"], "protected": [],
                                   "protected_changed": []})
            self.assertEqual(ac.main(["diff", o, n]), 1)

    def test_diff_ok_when_only_shipped_names_are_added(self):
        with tempfile.TemporaryDirectory() as o, tempfile.TemporaryDirectory() as n:
            Path(o, "000100.PCX").write_bytes(b"abc")
            Path(n, "000100.PCX").write_bytes(b"abc")
            Path(n, "002900.PCX").write_bytes(b"new")
            self.assertEqual(ac.main(["diff", o, n]), 0)

    def test_diff_ignores_other_prefixes(self):
        with tempfile.TemporaryDirectory() as o, tempfile.TemporaryDirectory() as n:
            Path(o, "030000.PCX").write_bytes(b"a")
            Path(n, "030000.PCX").write_bytes(b"b")
            self.assertEqual(ac.diff_exports(o, n),
                             {"changed": [], "removed": [], "added": [], "protected": [],
                              "protected_changed": []})

    def test_diff_reports_slug_hand_added_and_icons_as_protected(self):
        names = ["021500.PCX", "002000.PCX", "012800.PCX", "022400.PCX", "040600.PCX", "040700.PCX"]
        with tempfile.TemporaryDirectory() as o, tempfile.TemporaryDirectory() as n:
            for name in names:
                Path(o, name).write_bytes(b"old")
                Path(n, name).write_bytes(b"old")
            Path(o, "002900.PCX").write_bytes(b"a")    # a new-state name is not protected
            Path(n, "002900.PCX").write_bytes(b"a")
            Path(o, "000900.PCX").write_bytes(b"a")
            Path(n, "000900.PCX").write_bytes(b"a")
            res = ac.diff_exports(o, n)
            self.assertEqual(res["changed"], [])
            self.assertEqual(res["removed"], [])
            self.assertEqual(res["added"], [])
            self.assertEqual(res["protected"], sorted(names))
            self.assertEqual(ac.main(["diff", o, n]), 0)
            os.remove(Path(n, "002000.PCX"))      # a vanished hand-added file is not "removed" ...
            res = ac.diff_exports(o, n)
            self.assertEqual(res["removed"], [])
            self.assertEqual(res["protected_changed"], ["002000.PCX"])   # ... but it is protected_changed
            self.assertEqual(ac.main(["diff", o, n]), 1)

    def test_is_protected_edges(self):
        for name in ("020100.PCX", "0020FF.PCX", "002000.PCX", "002800.PCX", "012100.PCX",
                     "040601.PCX", "040799.PCX"):
            self.assertTrue(ac.is_protected(name), name)
        for name in ("001F00.PCX", "002900.PCX", "052000.PCX", "000000.PCX", "040500.PCX",
                     "010000.PCX", "020000.PCX"):
            self.assertFalse(ac.is_protected(name), name)

    def test_budget_keeps_counting_protected_files(self):
        with tempfile.TemporaryDirectory() as t:
            for name in ac.new_files():
                pad_file(Path(t, name), 100)
            pad_file(Path(t, "021500.PCX"), 3000)    # 2 clusters
            pad_file(Path(t, "002000.PCX"), 100)     # 1 cluster
            res = ac.budget(t)
            self.assertEqual(res["total"], 78)
            self.assertEqual(res["protected_clusters"], 3)

    def test_used_clusters_counts_files_and_budget_reports_drift(self):
        with tempfile.TemporaryDirectory() as t, tempfile.TemporaryDirectory() as e:
            pad_file(Path(t, "a.PCX"), 1)
            pad_file(Path(t, "b.PCX"), 2049)
            Path(t, "sub").mkdir()
            self.assertEqual(ac.used_clusters(t), 3)
            res = ac.budget(e, t)
            self.assertEqual(res["measured_used"], 3)
            self.assertEqual(res["limit"], ac.volume_limit(t)["limit"])
            self.assertEqual(ac.budget(e)["measured_used"], ac.used_clusters(ac.TO_BADGE))

    def test_budget_on_a_missing_export_dir_is_a_clear_error(self):
        with tempfile.TemporaryDirectory() as t:
            gone = Path(t, "nope")
            with self.assertRaises(ValueError) as cm:
                ac.budget(gone)
            self.assertIn(str(gone), str(cm.exception))

    def test_budget_uses_the_computed_limit(self):
        with tempfile.TemporaryDirectory() as t:
            names = ac.new_files()
            for name in names[:77]:
                pad_file(Path(t, name), 1000)
            pad_file(Path(t, names[77]), 2049)
            res = ac.budget(t)
            self.assertEqual(res["total"], 79)
            self.assertEqual(res["over"], [names[77]])
            self.assertEqual(res["limit"], ac.volume_limit()["limit"])
            self.assertEqual(res["fits"], res["total"] <= res["limit"])
            self.assertNotIn("assumed_used", res)
            self.assertFalse(hasattr(ac, "NEW_PCX_BUDGET"))
            self.assertFalse(hasattr(ac, "ASSUMED_USED"))

    def test_budget_over_the_limit_does_not_fit(self):
        with tempfile.TemporaryDirectory() as t:
            for name in ac.new_files():
                pad_file(Path(t, name), 2500)
            res = ac.budget(t)
            self.assertEqual(res["total"], 156)
            self.assertFalse(res["fits"])
            self.assertEqual(ac.main(["budget", t]), 1)

    def test_budget_with_a_missing_file_fails_the_cli(self):
        with tempfile.TemporaryDirectory() as t:
            for name in ac.new_files()[1:]:
                pad_file(Path(t, name), 100)
            self.assertEqual(ac.budget(t)["missing"], [ac.new_files()[0]])
            self.assertEqual(ac.main(["budget", t]), 1)

    def test_drift_same_frame_has_no_flags(self):
        self.assertEqual(ac.drift(box(), box())["flags"], [])

    def test_drift_thicker_outline_is_flagged(self):
        d = ac.drift(box(outline=3), box(outline=2))
        self.assertIn("outline", d["flags"])
        self.assertEqual(d["outline"], (2, 3))

    def test_drift_gap_in_outline_is_flagged(self):
        new = box()
        for y in range(20):                       # whiten the outline on the top 40 %
            for x in range(30):
                if new[20 + y][40 + x] == 0 and (y < 8 or x < 12):
                    new[20 + y][40 + x] = 2
        self.assertIn("coverage", ac.drift(new, box())["flags"])

    def test_drift_bigger_pet_is_flagged(self):
        d = ac.drift(box(h=24), box(h=20))
        self.assertIn("size", d["flags"])
        self.assertEqual(d["height"], (20, 24))
        d = ac.drift(box(oy=30), box(oy=20))
        self.assertIn("position", d["flags"])
        self.assertEqual(d["dy"], 10)

    def test_drift_art_on_a_new_border_is_cut_off(self):
        new = box()
        new[0][10] = 0
        d = ac.drift(new, box())
        self.assertTrue(d["cut_off"])
        self.assertIn("cut_off", d["flags"])

    def test_drift_border_art_in_idle_is_not_cut_off(self):
        idle = box()
        idle[0][10] = 0
        new = box()
        new[0][11] = 0
        self.assertFalse(ac.drift(new, idle)["cut_off"])

    def _export(self, t):
        rows = box()
        for name in ac.new_files():
            write_pcx(Path(t, name), rows)

    def test_review_writes_manifest_and_files(self):
        with tempfile.TemporaryDirectory() as e, tempfile.TemporaryDirectory() as o, \
                tempfile.TemporaryDirectory() as lit:
            self._export(e)
            old = Path(o, "feedback-1.json")
            old.write_text("{}")
            mpath = ac.write_review(e, 1, out_dir=o, litter_dir=lit)
            self.assertEqual(mpath, Path(o) / "manifest.json")
            m = json.loads(mpath.read_text())
            self.assertEqual(m["round"], 1)
            self.assertEqual(m["budget"]["limit"], ac.volume_limit()["limit"])
            self.assertEqual(m["budget"]["total"], 78)
            self.assertEqual(len(m["frames"]), 78)
            self.assertTrue(Path(o, "slug/sheet.png").is_file())
            self.assertTrue(Path(o, "slug/idle.png").is_file())
            self.assertTrue(m["budget"]["fits"])
            self.assertTrue(old.exists())
            f0 = m["frames"][0]
            self.assertEqual((f0["pet"], f0["state"], f0["anim_id"], f0["frame_id"], f0["pcx"]),
                             ("bartholomeus", "tipsy", "0x29", "bartho_tipsy_0", "002900.PCX"))
            for f in m["frames"]:
                frame = Image.open(Path(o, f["file"]))
                self.assertEqual(frame.size, (152 * 4, 75 * 4))
                self.assertEqual(Image.open(Path(o, f["idle"])).size, (152 * 4, 75 * 4))
                self.assertTrue(Path(o, f["sheet"]).is_file())
                self.assertEqual(set(f["metrics"]), {"outline", "coverage", "height", "dy", "cut_off"})
                self.assertFalse(f["missing"])
                self.assertEqual(f["bytes"], Path(e, f["pcx"]).stat().st_size)
                self.assertEqual(f["clusters"], 1)
                self.assertEqual(f["metrics"]["outline"][1], 2)
                self.assertEqual(f["metrics"]["coverage"][1], 100)
                self.assertEqual(f["metrics"]["height"][1], 20)
                self.assertNotIn("frame_size", f["flags"])
            # colour map: black outline, white fill, transparent shows white, red stays red
            img = Image.open(Path(o, f0["file"])).convert("RGB")
            self.assertEqual(img.getpixel((40 * 4 + 1, 20 * 4 + 1)), (0, 0, 0))
            self.assertEqual(img.getpixel((50 * 4 + 1, 30 * 4 + 1)), (255, 255, 255))
            self.assertEqual(img.getpixel((1, 1)), (255, 255, 255))

    def test_review_maps_red_and_flags_a_wrong_frame_size(self):
        with tempfile.TemporaryDirectory() as e, tempfile.TemporaryDirectory() as o, \
                tempfile.TemporaryDirectory() as lit:
            self._export(e)
            rows = box()
            rows[5][5] = 1
            write_pcx(Path(e, "002900.PCX"), rows)
            write_pcx(Path(e, "002A00.PCX"), [r[:100] for r in box()])
            m = json.loads(ac.write_review(e, 1, out_dir=o, litter_dir=lit).read_text())
            by = {f["pcx"]: f for f in m["frames"]}
            self.assertEqual(Image.open(Path(o, "bartholomeus/tipsy.png")).convert("RGB")
                             .getpixel((5 * 4, 5 * 4)), (255, 0, 0))
            self.assertIn("frame_size", by["002A00.PCX"]["flags"])
            self.assertNotIn("frame_size", by["002900.PCX"]["flags"])

    def test_review_of_a_partial_export_is_marked_incomplete(self):
        with tempfile.TemporaryDirectory() as e, tempfile.TemporaryDirectory() as o, \
                tempfile.TemporaryDirectory() as lit:
            self._export(e)
            os.remove(Path(e, "063200.PCX"))
            m = json.loads(ac.write_review(e, 1, out_dir=o, litter_dir=lit).read_text())
            self.assertEqual(len(m["frames"]), 78)
            self.assertFalse(m["budget"]["fits"])
            self.assertEqual(m["budget"]["total"], 77)
            gone = [f for f in m["frames"] if f["missing"]]
            self.assertEqual([f["pcx"] for f in gone], ["063200.PCX"])
            self.assertIsNone(gone[0]["file"])
            self.assertIn("missing", gone[0]["flags"])
            self.assertFalse(Path(o, "badger/throwing_up.png").exists())

    def _litter(self, lit, skip=(), strip=True, plan7=True, metrics=True):
        lit = Path(lit)
        (lit / "items").mkdir()
        (lit / "plan7").mkdir()
        data = {}
        for item in ac.FLOOR_ITEMS:
            if item in skip:
                continue
            im = Image.new("RGBA", (12, 5), (0, 0, 0, 0))
            im.save(lit / "items" / f"{item}.png")
            if plan7:
                Image.new("RGBA", (152, 10), (0, 0, 0, 0)).save(lit / "plan7" / f"{item}.png")
            data[item] = {"slot": [2, 105, 12, 5], "bytes": 10, "flags": ["speck"],
                          "metrics": {"outline": 1, "coverage": 100, "bottom": True,
                                      "ink_pct": 41, "red": 0}}
        if strip:
            Image.new("RGBA", (152, 10), (0, 0, 0, 0)).save(lit / "strip.png")
        if metrics:
            (lit / "metrics.json").write_text(json.dumps(data))
        return lit

    def test_review_with_a_missing_floor_item_keeps_the_others(self):
        with tempfile.TemporaryDirectory() as e, tempfile.TemporaryDirectory() as o, \
                tempfile.TemporaryDirectory() as lit:
            self._export(e)
            self._litter(lit, skip=("sock",))
            m = json.loads(ac.write_review(e, 1, out_dir=o, litter_dir=lit).read_text())
            floor = {f["state"]: f for f in m["frames"] if f["pet"] == "floor"}
            self.assertEqual(list(floor), list(ac.FLOOR_ITEMS))
            self.assertTrue(floor["sock"]["missing"])
            self.assertIsNone(floor["sock"]["file"])
            self.assertFalse(m["budget"]["fits"])
            for item in ("crumbs", "peel", "can", "pizza_box"):
                self.assertFalse(floor[item]["missing"])
                self.assertEqual(floor[item]["flags"], ["speck"])
                self.assertEqual(floor[item]["metrics"]["ink_pct"], 41)
                self.assertTrue(Path(o, floor[item]["file"]).is_file())

    def test_floor_entries_reference_only_written_files(self):
        with tempfile.TemporaryDirectory() as e, tempfile.TemporaryDirectory() as o, \
                tempfile.TemporaryDirectory() as lit:
            self._export(e)
            self._litter(lit, strip=False, plan7=False, metrics=False)
            m = json.loads(ac.write_review(e, 1, out_dir=o, litter_dir=lit).read_text())
            for f in (f for f in m["frames"] if f["pet"] == "floor"):
                self.assertIsNone(f["idle"])
                self.assertIsNone(f["sheet"])
                self.assertEqual(f["flags"], ["no_metrics"])
                self.assertTrue(Path(o, f["file"]).is_file())
            self.assertFalse(Path(o, "floor/scene.png").exists())

    def test_floor_metrics_json_rejects_other_shapes(self):
        for bad in ({"items": {}}, {"crumbs": {"slot": [0, 0, 1, 1], "bytes": 1, "flags": [],
                                               "outline": 1}}):
            with tempfile.TemporaryDirectory() as e, tempfile.TemporaryDirectory() as o, \
                    tempfile.TemporaryDirectory() as lit:
                self._export(e)
                self._litter(lit)
                Path(lit, "metrics.json").write_text(json.dumps(bad))
                with self.assertRaises(ValueError):
                    ac.write_review(e, 1, out_dir=o, litter_dir=lit)

    def test_review_without_litter_has_no_floor(self):
        with tempfile.TemporaryDirectory() as e, tempfile.TemporaryDirectory() as o, \
                tempfile.TemporaryDirectory() as lit:
            self._export(e)
            m = json.loads(ac.write_review(e, 2, out_dir=o, litter_dir=lit).read_text())
            self.assertEqual(len(m["frames"]), 78)
            self.assertFalse(Path(o, "floor").exists())

    def test_review_lists_the_floor_items(self):
        with tempfile.TemporaryDirectory() as e, tempfile.TemporaryDirectory() as o, \
                tempfile.TemporaryDirectory() as lit:
            self._export(e)
            lit = Path(lit)
            (lit / "items").mkdir()
            (lit / "plan7").mkdir()
            metrics = {}
            for i, item in enumerate(ac.FLOOR_ITEMS):
                w, h = 12 + i, 5 + i
                im = Image.new("RGBA", (w, h), (0, 0, 0, 0))
                im.paste((0, 0, 0, 255), (0, h - 1, w, h))
                im.save(lit / "items" / f"{item}.png")
                Image.new("RGBA", (152, 10), (0, 0, 0, 0)).save(lit / "plan7" / f"{item}.png")
                metrics[item] = {"slot": [2, 105, w, h], "bytes": 10, "flags": [],
                                 "metrics": {"outline": 1, "coverage": 100, "bottom": True,
                                             "ink_pct": 41, "red": 0}}
            Image.new("RGBA", (152, 10), (0, 0, 0, 0)).save(lit / "strip.png")
            (lit / "metrics.json").write_text(json.dumps(metrics))
            m = json.loads(ac.write_review(e, 1, out_dir=o, litter_dir=lit).read_text())
            self.assertEqual(len(m["frames"]), 83)
            self.assertEqual(m["budget"]["total"], 78)
            floor = [f for f in m["frames"] if f["pet"] == "floor"]
            self.assertEqual([f["state"] for f in floor], list(ac.FLOOR_ITEMS))
            for f in floor:
                self.assertEqual(f["clusters"], 0)
                self.assertEqual(f["bytes"], 10)
                self.assertEqual(len(f["slot"]), 4)
                self.assertIsNone(f["pcx"])
                self.assertEqual(f["frame_id"], "floor_" + f["state"])
                for key in ("file", "idle", "sheet"):
                    self.assertTrue(Path(o, f[key]).is_file(), f[key])
            self.assertEqual(Image.open(Path(o, "floor/scene.png")).size, (152 * 4, 75 * 4))
            self.assertEqual(Image.open(Path(o, "floor/strip.png")).size, (152 * 4, 10 * 4))


class Inventory(unittest.TestCase):
    """M1: which status word / action shows which art, per pet (reads src/ and assets/to-badge)."""

    STATUS_RS = ac.REPO / "src/game/engine/status.rs"
    DISPLAY_RS = ac.REPO / "src/game/engine/to_display.rs"
    ANIM_RS = ac.REPO / "src/game/engine/anim_files.rs"
    PETS = ("00", "01", "02", "05", "06", "07")
    RETIRED_PREFIXES = ("00", "01", "05", "06", "07")   # Drained 06 / 0A go in A4b

    @staticmethod
    def snake(name):
        import re
        return re.sub(r"(?<!^)(?=[A-Z])", "_", name).lower()

    def status_words(self):
        import re
        text = self.STATUS_RS.read_text()
        m = re.search(r"pub const ALL: \[Word; (\d+)\] = \[(.*?)\];", text, re.S)
        names = [self.snake(n) for n in re.findall(r"Word::(\w+)", m.group(2))]
        self.assertEqual(int(m.group(1)), len(names))
        return names

    def test_inventory_covers_every_status_word(self):
        words = self.status_words()
        self.assertEqual(len(words), 29)
        self.assertGreaterEqual(set(ac.SHOWN), set(words))

    def test_inventory_covers_every_display_anim(self):
        import re
        enum = re.search(r"pub enum DisplayAnim \{(.*?)\n\}", self.DISPLAY_RS.read_text(), re.S).group(1)
        variants = re.findall(r"^    ([A-Z]\w*)", enum, re.M)
        ids = {v: int(i, 16) for v, i in
               re.findall(r"DisplayAnim::(\w+)(?: \{ \.\. \})? => (0x[0-9A-Fa-f]+)", self.ANIM_RS.read_text())}
        known = {i for row in ac.SHOWN.values() for i in list(row["stand_in"]) + [row["art"]] if i is not None}
        self.assertTrue(variants)
        for v in variants:
            self.assertIn(v, ids, v)
            self.assertIn(ids[v], known, f"{v} 0x{ids[v]:02X} is in no SHOWN row")

    def test_every_pet_has_the_shipped_rows(self):
        shipped = [a for a in list(range(0x01, 0x15)) + list(range(0x20, 0x29))]
        for pp in self.PETS:
            for a in shipped:
                if pp in self.RETIRED_PREFIXES and a in (0x06, 0x0A):
                    continue
                self.assertTrue((ac.TO_BADGE / f"{pp}{a:02X}00.PCX").is_file(), f"{pp}{a:02X}00")
        rows = {r["word"]: r for r in ac.inventory()}
        for word, row in rows.items():
            art = ac.SHOWN[word]["art"]
            if art is not None and art < 0x29:
                self.assertEqual(set(row["pets"].values()), {"real"}, word)
        self.assertEqual(set(rows["content"]["pets"]), set(ac.INVENTORY_PETS))
        self.assertEqual(len(ac.INVENTORY_PETS), 6)

    def test_new_states_are_new_for_every_pet(self):
        rows = {r["word"]: r for r in ac.inventory()}
        for i, st in enumerate(ac.STATES):
            self.assertEqual(ac.SHOWN[st]["art"], 0x29 + i, st)
            self.assertEqual(set(rows[st]["pets"].values()), {"new"}, st)

    def test_candidates_are_the_stand_ins_left(self):
        self.assertEqual(set(ac.CANDIDATES),
                         {w for w, r in ac.SHOWN.items() if r["art"] is None})
        for w in ("waterlogged", "wash", "no_insulin"):
            self.assertIn(w, ac.CANDIDATES)
        self.assertFalse(set(ac.CANDIDATES) & set(ac.STATES))
        for w in ac.CANDIDATES:
            self.assertTrue(ac.SHOWN[w]["stand_in"], w)


if __name__ == "__main__":
    unittest.main()
