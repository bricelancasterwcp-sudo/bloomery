"""Tests for `tools.memory_battery.corpus_s5` (s5-weight-battery-v1 plan
Task 1; design spec §3).

Covers: the deterministic third-value type rules; moved-goal authoring on
a hand-built fixture with KNOWN defective/fixed outputs (the spliced test
executed against defective, fixed, and witness targets); the two-valued
exclusion; lane assignment (quotas, priority, per-lane p2 presence);
manifest schema (`lane`, `families_by_lane`); and same-seed determinism.
"""

from __future__ import annotations

import json
import unittest
from pathlib import Path
from tempfile import TemporaryDirectory

from tools.flywheel.factory import planted_test
from tools.memory_battery.corpus_s5 import (
    INSTRUMENT_S5,
    author_moved_goal_test,
    generate_corpus_s5,
    third_value,
)

DEFECTIVE_MODULE = "def double(x):\n    return x + x + x\n"
FIXED_MODULE = "def double(x):\n    return x + x\n"
SEARCH = "    return x + x + x"
REPLACE = "    return x + x"
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
ARGV = ("python3", "-m", "unittest", "test_calcmod.py")


class ThirdValueTest(unittest.TestCase):
    def test_numbers_use_max_plus_seven(self):
        self.assertEqual(third_value(6, 4), 13)
        self.assertEqual(third_value(4.0, 16.0), 23.0)

    def test_strings_append_the_rev_suffix(self):
        self.assertEqual(third_value("a=1, b=2", "a=3, b=5"), "a=3, b=5 (rev 2)")

    def test_tuples_reverse_then_fall_back_to_appending(self):
        self.assertEqual(third_value((7, 7), (7, 9)), (9, 7))
        # A palindromic fixed tuple reverses to itself -- the fallback
        # appends max+7 instead.
        self.assertEqual(third_value((7, 9), (9, 9)), (9, 9, 16))

    def test_two_valued_domains_are_excluded(self):
        self.assertIsNone(third_value(True, False))
        self.assertIsNone(third_value(False, True))


class AuthorMovedGoalTest(unittest.TestCase):
    def test_splices_the_third_value_and_emits_a_passing_witness(self):
        authored = author_moved_goal_test(PLANTED_TEST, "calcmod", FIXTURE_FILES, SEARCH, REPLACE)
        self.assertIsNotNone(authored)
        moved_goal, witness = authored
        # double(2): defective 6, fixed 4 -> third = max+7 = 13.
        self.assertEqual(moved_goal, PLANTED_TEST.replace("calcmod.double(2), 4", "calcmod.double(2), 13"))
        self.assertIn("def double(*args):\n    return 13", witness)

        on_defective = planted_test.run_python(
            {"calcmod.py": DEFECTIVE_MODULE, "test_calcmod.py": moved_goal}, ARGV
        )
        self.assertNotEqual(on_defective.returncode, 0)
        on_fixed = planted_test.run_python(
            {"calcmod.py": FIXED_MODULE, "test_calcmod.py": moved_goal}, ARGV
        )
        self.assertNotEqual(on_fixed.returncode, 0)
        on_witness = planted_test.run_python(
            {"calcmod.py": witness, "test_calcmod.py": moved_goal}, ARGV
        )
        self.assertEqual(on_witness.returncode, 0, on_witness.stdout)

    def test_boolean_output_is_excluded(self):
        files = {
            "calcmod.py": "def double(x):\n    return True\n",
            "test_calcmod.py": PLANTED_TEST.replace("calcmod.double(2), 4", "calcmod.double(2), False"),
        }
        self.assertIsNone(
            author_moved_goal_test(
                files["test_calcmod.py"], "calcmod", files, "    return True", "    return False"
            )
        )


class GenerateCorpusS5Test(unittest.TestCase):
    def test_end_to_end_lanes_schema_and_executed_properties(self):
        with TemporaryDirectory() as tmp:
            out_dir = Path(tmp) / "corpus"
            manifest = generate_corpus_s5(1, 2, out_dir)

            self.assertEqual(manifest["instrument"], INSTRUMENT_S5)
            self.assertEqual(manifest["n"], 6)
            self.assertEqual(manifest["n_per_lane"], 2)
            lanes = [entry["lane"] for entry in manifest["tasks"]]
            self.assertEqual(sorted(lanes).count("control"), 2)
            self.assertEqual(sorted(lanes).count("moot"), 2)
            self.assertEqual(sorted(lanes).count("stale"), 2)
            self.assertEqual(set(manifest["families_by_lane"]), {"control", "moot", "stale"})

            for entry in manifest["tasks"]:
                task_dir = out_dir / "tasks" / entry["name"]
                if entry["lane"] == "control":
                    self.assertNotIn("pristine_p2", entry)
                    self.assertFalse((task_dir / "pristine_p2").exists())
                    continue
                self.assertIn("workspace_p2_sha256", entry)
                p2 = task_dir / "pristine_p2"
                target, test_file = entry["target"], entry["test_file"]
                self.assertEqual(
                    (p2 / target).read_bytes(), (task_dir / "pristine" / target).read_bytes()
                )
                p2_files = {p.name: p.read_text(encoding="utf-8") for p in sorted(p2.iterdir())}
                result = planted_test.run_python(p2_files, entry["run_argv"])
                if entry["lane"] == "moot":
                    self.assertEqual(result.returncode, 0, f"{entry['name']}: {result.stdout}")
                else:  # stale: fails on defective, and the witness exists and passes
                    self.assertNotEqual(result.returncode, 0)
                    witness_path = task_dir / "witness" / target
                    self.assertTrue(witness_path.is_file())
                    witness_files = dict(p2_files)
                    witness_files[target] = witness_path.read_text(encoding="utf-8")
                    on_witness = planted_test.run_python(witness_files, entry["run_argv"])
                    self.assertEqual(on_witness.returncode, 0, f"{entry['name']}: {on_witness.stdout}")

    def test_same_seed_regeneration_is_field_identical_modulo_out_dir(self):
        with TemporaryDirectory() as tmp_a, TemporaryDirectory() as tmp_b:
            a = generate_corpus_s5(1, 2, Path(tmp_a) / "c")
            b = generate_corpus_s5(1, 2, Path(tmp_b) / "c")
            for entry_a, entry_b in zip(a["tasks"], b["tasks"], strict=True):
                trimmed_a = {k: v for k, v in entry_a.items() if k != "grant"}
                trimmed_b = {k: v for k, v in entry_b.items() if k != "grant"}
                self.assertEqual(trimmed_a, trimmed_b)

    def test_priority_sends_a_dual_qualified_draw_to_stale_while_moot_is_open(self):
        # Spec §3's priority: a dual-qualified task lands in STALE while
        # both lanes are unfilled. The factory's slot pattern makes draw 0
        # always the boolean family (stale-excluded -> moot), so the first
        # discriminating draw is draw 1: at seed 1 / n_per_lane=2 it is a
        # non-boolean family arriving while moot still has quota (1/2) --
        # correct priority sends it to stale; an inverted priority would
        # send it to moot. Both the pin AND its premises are asserted, so
        # a factory-order drift fails loudly rather than passing vacuously.
        with TemporaryDirectory() as tmp:
            manifest = generate_corpus_s5(1, 2, Path(tmp) / "c")
            tasks = sorted(manifest["tasks"], key=lambda entry: entry["name"].rsplit("-", 1)[1])
            # Premises: draw 0 is the boolean family in moot; draw 1 is a
            # non-boolean family that arrived with moot still open.
            self.assertEqual(tasks[0]["family"], "py_inverted_boolean_run_verified")
            self.assertEqual(tasks[0]["lane"], "moot")
            self.assertNotEqual(tasks[1]["family"], "py_inverted_boolean_run_verified")
            # The pin: priority sends it to stale.
            self.assertEqual(tasks[1]["lane"], "stale")


if __name__ == "__main__":
    unittest.main()
