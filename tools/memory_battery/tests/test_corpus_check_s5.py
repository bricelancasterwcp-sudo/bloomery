"""Tests for `tools.memory_battery.corpus_check_s5` (s5 plan Task 2;
design spec §3's per-lane checks). Each test seeds a small REAL corpus
via `generate_corpus_s5` and breaks exactly one structural property.
"""

from __future__ import annotations

import json
import unittest
from pathlib import Path
from tempfile import TemporaryDirectory

from tools.memory_battery.corpus_check_s5 import check_corpus_s5, format_report_s5
from tools.memory_battery.corpus_s5 import generate_corpus_s5

SEED = 1
N_PER_LANE = 2


def _generate(tmp: str) -> tuple[Path, dict]:
    out_dir = Path(tmp) / "corpus"
    manifest = generate_corpus_s5(SEED, N_PER_LANE, out_dir)
    return out_dir, manifest


def _lane_entry(manifest: dict, lane: str) -> dict:
    return next(entry for entry in manifest["tasks"] if entry["lane"] == lane)


class CheckCorpusS5Test(unittest.TestCase):
    def test_clean_corpus_passes_every_check(self):
        with TemporaryDirectory() as tmp:
            out_dir, _ = _generate(tmp)
            report = check_corpus_s5(out_dir)
            self.assertTrue(report.ok, format_report_s5(report))

    def test_b1_fails_when_the_moved_goal_test_passes_on_defective(self):
        with TemporaryDirectory() as tmp:
            out_dir, manifest = _generate(tmp)
            entry = _lane_entry(manifest, "stale")
            task_dir = out_dir / "tasks" / entry["name"]
            # Replace the moved-goal test with the MOVED-ON construction
            # (passes on defective) -- exactly the wrong lane's artifact.
            from tools.memory_battery.corpus_pg import author_moved_on_test

            pristine = task_dir / "pristine"
            files = {p.name: p.read_text(encoding="utf-8") for p in sorted(pristine.iterdir())}
            moved_on = author_moved_on_test(files[entry["test_file"]], Path(entry["target"]).stem, files)
            (task_dir / "pristine_p2" / entry["test_file"]).write_text(moved_on, encoding="utf-8")

            report = check_corpus_s5(out_dir)
            self.assertFalse(report.ok)
            broken = next(r for r in report.lane_results if r.name == entry["name"])
            self.assertFalse(broken.b1_ok)

    def test_b2_fails_when_the_moved_goal_test_passes_on_the_stored_fix(self):
        with TemporaryDirectory() as tmp:
            out_dir, manifest = _generate(tmp)
            entry = _lane_entry(manifest, "stale")
            task_dir = out_dir / "tasks" / entry["name"]
            # Restore the ORIGINAL planted test (passes on the stored fix).
            planted = (task_dir / "pristine" / entry["test_file"]).read_text(encoding="utf-8")
            (task_dir / "pristine_p2" / entry["test_file"]).write_text(planted, encoding="utf-8")

            report = check_corpus_s5(out_dir)
            self.assertFalse(report.ok)
            broken = next(r for r in report.lane_results if r.name == entry["name"])
            self.assertFalse(broken.b2_ok)

    def test_b3_fails_when_the_witness_is_broken(self):
        with TemporaryDirectory() as tmp:
            out_dir, manifest = _generate(tmp)
            entry = _lane_entry(manifest, "stale")
            witness_path = out_dir / "tasks" / entry["name"] / "witness" / entry["target"]
            witness_path.write_text("def broken(:\n", encoding="utf-8")

            report = check_corpus_s5(out_dir)
            self.assertFalse(report.ok)
            broken = next(r for r in report.lane_results if r.name == entry["name"])
            self.assertFalse(broken.b3_ok)

    def test_control_task_with_a_stray_p2_key_fails(self):
        with TemporaryDirectory() as tmp:
            out_dir, manifest = _generate(tmp)
            entry = _lane_entry(manifest, "control")
            manifest_path = out_dir / "manifest.json"
            doctored = json.loads(manifest_path.read_text(encoding="utf-8"))
            for task in doctored["tasks"]:
                if task["name"] == entry["name"]:
                    task["pristine_p2"] = f"tasks/{entry['name']}/pristine_p2"
            manifest_path.write_text(json.dumps(doctored, indent=2, sort_keys=True) + "\n", encoding="utf-8")

            report = check_corpus_s5(out_dir)
            self.assertFalse(report.ok)
            broken = next(r for r in report.lane_results if r.name == entry["name"])
            self.assertFalse(broken.ok)

    def test_wrong_instrument_is_a_corpus_level_failure(self):
        with TemporaryDirectory() as tmp:
            out_dir, _ = _generate(tmp)
            manifest_path = out_dir / "manifest.json"
            doctored = json.loads(manifest_path.read_text(encoding="utf-8"))
            doctored["instrument"] = "premise-gone-battery-v1"
            manifest_path.write_text(json.dumps(doctored, indent=2, sort_keys=True) + "\n", encoding="utf-8")

            report = check_corpus_s5(out_dir)
            self.assertFalse(report.ok)
            self.assertTrue(any("instrument" in failure for failure in report.corpus_failures))


if __name__ == "__main__":
    unittest.main()
