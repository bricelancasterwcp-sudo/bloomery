"""Tests for `tools.memory_battery.corpus_pg` (premise-gone-battery-v1
plan Task 1; design spec §3).

Covers: moved-on authoring on a hand-built fixture with a KNOWN defective
output (the literal is pinned by execution, so the test pins the pin);
the raising-defect and unpinnable-repr exclusions; the structural
hard-error shapes (not exactly one assertEqual, call rooted outside the
target module); and `generate_corpus_pg` end-to-end on a small n —
manifest schema, `pristine_p2/` target byte-identity, moved-on test
divergence, the moved-on test actually PASSING on the defective target
(executed, not trusted), and same-seed determinism modulo out_dir.
"""

from __future__ import annotations

import json
import unittest
from pathlib import Path
from tempfile import TemporaryDirectory

from tools.flywheel.factory import planted_test
from tools.memory_battery.corpus_pg import (
    INSTRUMENT_PG,
    MovedOnShapeError,
    author_moved_on_test,
    generate_corpus_pg,
)

SEED = 1
N = 3

DEFECTIVE_MODULE = "def double(x):\n    return x + x + x\n"
FIXED_MODULE = "def double(x):\n    return x + x\n"
PLANTED_TEST = (
    "import unittest\n"
    "\n"
    "import calcmod\n"
    "\n"
    "\n"
    "class TestCalcmod(unittest.TestCase):\n"
    "    def test_double(self):\n"
    "        self.assertEqual(calcmod.double(2), 4)\n"
    "\n"
    "\n"
    'if __name__ == "__main__":\n'
    "    unittest.main()\n"
)
FIXTURE_FILES = {"calcmod.py": DEFECTIVE_MODULE, "test_calcmod.py": PLANTED_TEST}


class AuthorMovedOnTest(unittest.TestCase):
    def test_pins_the_defective_output_and_changes_nothing_else(self):
        moved_on = author_moved_on_test(PLANTED_TEST, "calcmod", FIXTURE_FILES)
        # double(2) under the defect returns 6 — the expected literal moves
        # 4 -> 6 and every other byte stays put.
        self.assertEqual(moved_on, PLANTED_TEST.replace("calcmod.double(2), 4", "calcmod.double(2), 6"))

    def test_moved_on_test_passes_on_defective_and_fails_on_fixed(self):
        moved_on = author_moved_on_test(PLANTED_TEST, "calcmod", FIXTURE_FILES)
        argv = ("python3", "-m", "unittest", "test_calcmod.py")
        on_defective = planted_test.run_python(
            {"calcmod.py": DEFECTIVE_MODULE, "test_calcmod.py": moved_on}, argv
        )
        self.assertEqual(on_defective.returncode, 0, on_defective.stdout)
        on_fixed = planted_test.run_python(
            {"calcmod.py": FIXED_MODULE, "test_calcmod.py": moved_on}, argv
        )
        self.assertNotEqual(on_fixed.returncode, 0)

    def test_raising_defective_call_is_excluded_as_none(self):
        files = {"calcmod.py": "def double(x):\n    raise ValueError(str(x))\n", "test_calcmod.py": PLANTED_TEST}
        self.assertIsNone(author_moved_on_test(PLANTED_TEST, "calcmod", files))

    def test_unpinnable_repr_is_excluded_as_none(self):
        files = {
            "calcmod.py": "def double(x):\n    return object()\n",
            "test_calcmod.py": PLANTED_TEST,
        }
        self.assertIsNone(author_moved_on_test(PLANTED_TEST, "calcmod", files))

    def test_two_assert_equals_is_a_shape_error(self):
        doubled = PLANTED_TEST.replace(
            "        self.assertEqual(calcmod.double(2), 4)\n",
            "        self.assertEqual(calcmod.double(2), 4)\n"
            "        self.assertEqual(calcmod.double(3), 6)\n",
        )
        with self.assertRaises(MovedOnShapeError):
            author_moved_on_test(doubled, "calcmod", {"calcmod.py": DEFECTIVE_MODULE, "test_calcmod.py": doubled})

    def test_call_rooted_outside_the_target_module_is_a_shape_error(self):
        foreign = PLANTED_TEST.replace("calcmod.double(2)", "othermod.double(2)")
        with self.assertRaises(MovedOnShapeError):
            author_moved_on_test(foreign, "calcmod", FIXTURE_FILES)


class GenerateCorpusPgTest(unittest.TestCase):
    def test_end_to_end_schema_bytes_and_executed_moved_on(self):
        with TemporaryDirectory() as tmp:
            out_dir = Path(tmp) / "corpus"
            manifest = generate_corpus_pg(SEED, N, out_dir)

            self.assertEqual(manifest["instrument"], INSTRUMENT_PG)
            self.assertEqual(manifest["corpus_seed"], SEED)
            self.assertEqual(manifest["n"], N)
            self.assertEqual(len(manifest["tasks"]), N)
            on_disk = json.loads((out_dir / "manifest.json").read_text(encoding="utf-8"))
            self.assertEqual(on_disk, manifest)

            for entry in manifest["tasks"]:
                task_dir = out_dir / "tasks" / entry["name"]
                pristine = task_dir / "pristine"
                pristine_p2 = task_dir / "pristine_p2"
                self.assertEqual(entry["pristine_p2"], str(pristine_p2.relative_to(out_dir)))
                self.assertIn("workspace_p2_sha256", entry)

                target = entry["target"]
                test_file = entry["test_file"]
                # The fingerprint-match property: p2's target is the
                # defective bytes verbatim; the test file moved on.
                self.assertEqual(
                    (pristine_p2 / target).read_bytes(), (pristine / target).read_bytes()
                )
                self.assertNotEqual(
                    (pristine_p2 / test_file).read_bytes(), (pristine / test_file).read_bytes()
                )

                # Executed, not trusted: the moved-on test passes on the
                # defective target.
                p2_files = {
                    p.name: p.read_text(encoding="utf-8") for p in sorted(pristine_p2.iterdir())
                }
                result = planted_test.run_python(p2_files, entry["run_argv"])
                self.assertEqual(result.returncode, 0, f"{entry['name']}: {result.stdout}")

    def test_same_seed_regeneration_is_field_identical_modulo_out_dir(self):
        with TemporaryDirectory() as tmp_a, TemporaryDirectory() as tmp_b:
            a = generate_corpus_pg(SEED, N, Path(tmp_a) / "corpus")
            b = generate_corpus_pg(SEED, N, Path(tmp_b) / "corpus")
            for entry_a, entry_b in zip(a["tasks"], b["tasks"], strict=True):
                trimmed_a = {k: v for k, v in entry_a.items() if k != "grant"}
                trimmed_b = {k: v for k, v in entry_b.items() if k != "grant"}
                self.assertEqual(trimmed_a, trimmed_b)


if __name__ == "__main__":
    unittest.main()
