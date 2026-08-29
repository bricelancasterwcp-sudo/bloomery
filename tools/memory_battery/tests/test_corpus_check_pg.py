"""Tests for `tools.memory_battery.corpus_check_pg` (plan Task 2; design
spec §3's S1-S5).

Each test seeds a small REAL corpus via `generate_corpus_pg` and then
breaks exactly one structural property, asserting the checker names that
break and fails the corpus — the falsification-per-check discipline. The
clean corpus passes wholesale (which also exercises S1/S2 delegation and
the S5 sha agreement between the checker's independent formula and the
generator's).
"""

from __future__ import annotations

import json
import unittest
from pathlib import Path
from tempfile import TemporaryDirectory

from tools.memory_battery.corpus_check_pg import check_corpus_pg, format_report_pg
from tools.memory_battery.corpus_pg import generate_corpus_pg

SEED = 1
N = 2

VACUOUS_TEST = (
    "import unittest\n"
    "\n"
    "\n"
    "class TestVacuous(unittest.TestCase):\n"
    "    def test_nothing(self):\n"
    "        self.assertTrue(True)\n"
    "\n"
    "\n"
    'if __name__ == "__main__":\n'
    "    unittest.main()\n"
)


def _generate(tmp: str) -> tuple[Path, dict]:
    out_dir = Path(tmp) / "corpus"
    manifest = generate_corpus_pg(SEED, N, out_dir)
    return out_dir, manifest


class CheckCorpusPgTest(unittest.TestCase):
    def test_clean_corpus_passes_every_check(self):
        with TemporaryDirectory() as tmp:
            out_dir, _ = _generate(tmp)
            report = check_corpus_pg(out_dir)
            self.assertTrue(report.ok, format_report_pg(report))

    def test_s3_fails_when_p2_target_bytes_drift_from_pristine(self):
        with TemporaryDirectory() as tmp:
            out_dir, manifest = _generate(tmp)
            entry = manifest["tasks"][0]
            task_dir = out_dir / "tasks" / entry["name"]
            pristine_target = (task_dir / "pristine" / entry["target"]).read_text(encoding="utf-8")
            fixed = pristine_target.replace(entry["search"], entry["replace"], 1)
            (task_dir / "pristine_p2" / entry["target"]).write_text(fixed, encoding="utf-8")

            report = check_corpus_pg(out_dir)
            self.assertFalse(report.ok)
            broken = report.pg_results[0]
            self.assertFalse(broken.s3_ok)
            self.assertIn("target", broken.s3_detail)

    def test_s3_fails_when_p2_test_is_the_original_planted_test(self):
        with TemporaryDirectory() as tmp:
            out_dir, manifest = _generate(tmp)
            entry = manifest["tasks"][0]
            task_dir = out_dir / "tasks" / entry["name"]
            planted = (task_dir / "pristine" / entry["test_file"]).read_text(encoding="utf-8")
            (task_dir / "pristine_p2" / entry["test_file"]).write_text(planted, encoding="utf-8")

            report = check_corpus_pg(out_dir)
            self.assertFalse(report.ok)
            self.assertFalse(report.pg_results[0].s3_ok)

    def test_s4_fails_when_the_moved_on_test_is_vacuous(self):
        with TemporaryDirectory() as tmp:
            out_dir, manifest = _generate(tmp)
            entry = manifest["tasks"][0]
            task_dir = out_dir / "tasks" / entry["name"]
            (task_dir / "pristine_p2" / entry["test_file"]).write_text(VACUOUS_TEST, encoding="utf-8")

            report = check_corpus_pg(out_dir)
            self.assertFalse(report.ok)
            broken = report.pg_results[0]
            self.assertFalse(broken.s4_ok)
            self.assertIn("fixed", broken.s4_detail)

    def test_s5_fails_when_the_manifest_p2_sha_is_doctored(self):
        with TemporaryDirectory() as tmp:
            out_dir, manifest = _generate(tmp)
            manifest_path = out_dir / "manifest.json"
            doctored = json.loads(manifest_path.read_text(encoding="utf-8"))
            doctored["tasks"][0]["workspace_p2_sha256"] = "0" * 64
            manifest_path.write_text(json.dumps(doctored, indent=2, sort_keys=True) + "\n", encoding="utf-8")

            report = check_corpus_pg(out_dir)
            self.assertFalse(report.ok)
            self.assertFalse(report.pg_results[0].s5_ok)

    def test_wrong_instrument_is_a_corpus_level_failure(self):
        with TemporaryDirectory() as tmp:
            out_dir, _ = _generate(tmp)
            manifest_path = out_dir / "manifest.json"
            doctored = json.loads(manifest_path.read_text(encoding="utf-8"))
            doctored["instrument"] = "memory-battery-v1"
            manifest_path.write_text(json.dumps(doctored, indent=2, sort_keys=True) + "\n", encoding="utf-8")

            report = check_corpus_pg(out_dir)
            self.assertFalse(report.ok)
            self.assertTrue(any("instrument" in failure for failure in report.corpus_failures))


if __name__ == "__main__":
    unittest.main()
