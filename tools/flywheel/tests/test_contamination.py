"""Tests for tools.flywheel.factory.contamination — brief rule 7.

The comparator must fail on any of: exact or normalized match of goals,
file contents, target filenames, search strings; OR >= 0.8 Jaccard
token-set similarity between any corpus goal and any gate goal. Its own
test plants a disguised copy of a gate fixture and must catch it.
"""

import json
import unittest
from pathlib import Path

from tools.flywheel.factory import contamination

REPO_ROOT = Path(__file__).resolve().parents[3]
GATE_PATH = REPO_ROOT / "crates" / "bloomery-daemon" / "fixtures" / "codec-tasks-v1.toml"


def _corpus_row(task_id, goal, target, target_contents, search, pair="read"):
    return {
        "prompt": "irrelevant for this test",
        "completion": "irrelevant for this test",
        "meta": {
            "task_id": task_id,
            "template": "unit_test_family",
            "lens": "python",
            "pair": pair,
            "goal": goal,
            "target": target,
            "target_contents": target_contents,
            "search": search,
        },
    }


class GateVocabularyIsComplete(unittest.TestCase):
    """Mechanical completeness: GATE_VOCABULARY must actually cover every
    target filename and every function name the real gate TOML defines —
    parsed from the real file, not trusted from a hand transcription."""

    def test_every_gate_target_filename_is_in_gate_vocabulary(self):
        fixtures = contamination.load_gate_fixtures(GATE_PATH)
        missing = sorted(
            f.target.lower() for f in fixtures if f.target.lower() not in contamination.GATE_VOCABULARY
        )
        self.assertEqual(missing, [], f"target filenames missing from GATE_VOCABULARY: {missing}")

    def test_every_gate_function_name_is_in_gate_vocabulary(self):
        fixtures = contamination.load_gate_fixtures(GATE_PATH)
        missing = sorted(name for name in fixtures_function_names(fixtures) if name not in contamination.GATE_VOCABULARY)
        self.assertEqual(missing, [], f"function names missing from GATE_VOCABULARY: {missing}")


def fixtures_function_names(fixtures):
    import re

    names = set()
    for fx in fixtures:
        for contents in fx.files.values():
            names.update(m.group(1).lower() for m in re.finditer(r"\bdef\s+([A-Za-z_]\w*)", contents))
    return names


class LoadGateFixturesTest(unittest.TestCase):
    def test_loads_all_fixtures_from_the_real_toml(self):
        fixtures = contamination.load_gate_fixtures(GATE_PATH)
        self.assertEqual(len(fixtures), 20)
        names = {f.name for f in fixtures}
        self.assertIn("py-mean-off-by-one", names)
        self.assertIn("txt-listen-port-mismatch", names)


class CleanCorpusPassesTest(unittest.TestCase):
    def test_a_genuinely_novel_corpus_has_no_violations(self):
        fixtures = contamination.load_gate_fixtures(GATE_PATH)
        rows = [
            _corpus_row(
                "t000001",
                "orbitwatch.py's exposure_minutes() divides the wrong total when averaging "
                "a night's readings. Fix exposure_minutes() in orbitwatch.py so it uses the "
                "readings count. Patch the file, then emit done.",
                "orbitwatch.py",
                "def exposure_minutes(readings):\n    total = 0\n    for r in readings:\n"
                "        total += r\n    return total / (len(readings) + 3)\n",
                "    return total / (len(readings) + 3)",
            )
        ]
        report = contamination.check_corpus(rows, fixtures)
        self.assertEqual(report.violations, [])
        self.assertTrue(report.clean)


class ExactAndNormalizedMatchTest(unittest.TestCase):
    def setUp(self):
        self.fixtures = contamination.load_gate_fixtures(GATE_PATH)

    def test_exact_goal_match_is_caught(self):
        gate_goal = next(f for f in self.fixtures if f.name == "py-mean-off-by-one").goal
        rows = [_corpus_row("t1", gate_goal, "harmless.py", "x = 1\ny = 2\nz = 3\na = 4\nb = 5\n", "x = 1")]
        report = contamination.check_corpus(rows, self.fixtures)
        self.assertFalse(report.clean)
        self.assertTrue(any(v["rule"] == "goal_match" for v in report.violations))

    def test_normalized_whitespace_and_case_goal_match_is_caught(self):
        gate_goal = next(f for f in self.fixtures if f.name == "py-mean-off-by-one").goal
        mangled = "   ".join(gate_goal.upper().split())
        rows = [_corpus_row("t1", mangled, "harmless.py", "x = 1\ny = 2\nz = 3\na = 4\nb = 5\n", "x = 1")]
        report = contamination.check_corpus(rows, self.fixtures)
        self.assertFalse(report.clean)
        self.assertTrue(any(v["rule"] == "goal_match" for v in report.violations))

    def test_target_filename_reuse_is_caught(self):
        rows = [
            _corpus_row(
                "t1",
                "Something unrelated entirely about a telescope calibration routine going "
                "sideways at dawn. Patch the file, then emit done.",
                "stats.py",
                "x = 1\ny = 2\nz = 3\na = 4\nb = 5\n",
                "x = 1",
            )
        ]
        report = contamination.check_corpus(rows, self.fixtures)
        self.assertFalse(report.clean)
        self.assertTrue(any(v["rule"] == "target_filename_match" for v in report.violations))

    def test_search_string_reuse_is_caught(self):
        rows = [
            _corpus_row(
                "t1",
                "Something unrelated entirely about a telescope calibration routine going "
                "sideways at dawn. Patch the file, then emit done.",
                "harmless.py",
                "x = 1\ny = 2\nz = 3\na = 4\nb = 5\n",
                "    return total / (len(values) + 1)",
            )
        ]
        report = contamination.check_corpus(rows, self.fixtures)
        self.assertFalse(report.clean)
        self.assertTrue(any(v["rule"] == "search_match" for v in report.violations))

    def test_file_contents_reuse_is_caught(self):
        gate_contents = (
            next(f for f in self.fixtures if f.name == "py-mean-off-by-one").files["stats.py"]
        )
        rows = [
            _corpus_row(
                "t1",
                "Something unrelated entirely about a telescope calibration routine going "
                "sideways at dawn. Patch the file, then emit done.",
                "harmless.py",
                gate_contents,
                "x = 1",
            )
        ]
        report = contamination.check_corpus(rows, self.fixtures)
        self.assertFalse(report.clean)
        self.assertTrue(any(v["rule"] == "file_contents_match" for v in report.violations))


class DisguisedCopyIsCaughtTest(unittest.TestCase):
    """The load-bearing test: a lightly reworded (disguised) copy of a real
    gate goal — not byte-identical — must still be caught by the Jaccard
    near-duplicate net, not just the exact/normalized checks."""

    def setUp(self):
        self.fixtures = contamination.load_gate_fixtures(GATE_PATH)
        gate_fixture = next(f for f in self.fixtures if f.name == "py-discount-wrong-operator")
        self.gate_goal = gate_fixture.goal
        # Disguise: cosmetic reformatting (case, whitespace) plus two
        # synonym swaps ("Fix" -> "Repair", "returns" -> "yields"). Same
        # content, >85% token overlap, different surface bytes -> an
        # exact/normalized string check alone would miss this; only the
        # Jaccard near-duplicate net catches it.
        self.disguised_goal = (
            "PRICING.PY'S   discounted_price()  increases the price instead of "
            "discounting it   -- discounted_price(100, 10) yields 110.0 instead of "
            "90.0. Repair discounted_price() in pricing.py so it subtracts the "
            "discount. Patch the file, then emit done."
        )

    def test_disguised_goal_is_caught_by_the_near_duplicate_net(self):
        rows = [
            _corpus_row(
                "t1",
                self.disguised_goal,
                "totally-different-name.py",
                "a = 1\nb = 2\nc = 3\nd = 4\ne = 5\n",
                "a = 1",
            )
        ]
        report = contamination.check_corpus(rows, self.fixtures)
        self.assertFalse(report.clean, "a disguised near-duplicate of a gate goal must be flagged")
        near_dup = [v for v in report.violations if v["rule"] == "goal_near_duplicate"]
        self.assertTrue(near_dup, f"expected a goal_near_duplicate violation, got: {report.violations}")
        self.assertGreaterEqual(near_dup[0]["jaccard"], 0.8)

    def test_the_disguise_really_is_not_an_exact_or_normalized_match(self):
        # Sanity check on the fixture itself: if this assertion ever fails,
        # the test above would be catching the disguise via the wrong
        # mechanism (exact match) and would no longer exercise the Jaccard
        # net at all.
        self.assertNotEqual(contamination.normalize(self.disguised_goal), contamination.normalize(self.gate_goal))

    def test_jaccard_similarity_of_the_disguise_is_genuinely_high(self):
        similarity = contamination.jaccard(
            contamination.token_set(self.disguised_goal), contamination.token_set(self.gate_goal)
        )
        self.assertGreaterEqual(similarity, 0.8)


class JaccardHelperTest(unittest.TestCase):
    def test_identical_sets_have_similarity_one(self):
        self.assertEqual(contamination.jaccard({"a", "b"}, {"a", "b"}), 1.0)

    def test_disjoint_sets_have_similarity_zero(self):
        self.assertEqual(contamination.jaccard({"a"}, {"b"}), 0.0)

    def test_both_empty_is_treated_as_identical(self):
        self.assertEqual(contamination.jaccard(set(), set()), 1.0)

    def test_partial_overlap(self):
        self.assertAlmostEqual(contamination.jaccard({"a", "b", "c"}, {"b", "c", "d"}), 2 / 4)


class MainCliTest(unittest.TestCase):
    def test_clean_corpus_exits_zero_and_writes_report(self, tmp_path=None):
        import subprocess
        import sys
        import tempfile

        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            corpus_path = tmp / "corpus.jsonl"
            report_path = tmp / "report.json"
            row = _corpus_row(
                "t1",
                "orbitwatch.py's exposure_minutes() divides by the wrong count when "
                "averaging a night's readings, so exposure_minutes([4, 6]) returns 2.5 "
                "instead of 5.0. Fix exposure_minutes() in orbitwatch.py so it divides by "
                "the reading count. Patch the file, then emit done.",
                "orbitwatch.py",
                "def exposure_minutes(readings):\n    total = 0\n    for r in readings:\n"
                "        total += r\n    return total / (len(readings) + 3)\n",
                "    return total / (len(readings) + 3)",
            )
            corpus_path.write_text(json.dumps(row) + "\n", encoding="utf-8")
            result = subprocess.run(
                [
                    sys.executable,
                    "-m",
                    "tools.flywheel.factory.contamination",
                    "--corpus",
                    str(corpus_path),
                    "--gate",
                    str(GATE_PATH),
                    "--out",
                    str(report_path),
                ],
                cwd=REPO_ROOT,
                capture_output=True,
                text=True,
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            report = json.loads(report_path.read_text(encoding="utf-8"))
            self.assertTrue(report["clean"])

    def test_contaminated_corpus_exits_nonzero(self):
        import subprocess
        import sys
        import tempfile

        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            corpus_path = tmp / "corpus.jsonl"
            report_path = tmp / "report.json"
            fixtures = contamination.load_gate_fixtures(GATE_PATH)
            gate_goal = next(f for f in fixtures if f.name == "py-mean-off-by-one").goal
            row = _corpus_row("t1", gate_goal, "harmless.py", "x = 1\ny = 2\nz = 3\na = 4\nb = 5\n", "x = 1")
            corpus_path.write_text(json.dumps(row) + "\n", encoding="utf-8")
            result = subprocess.run(
                [
                    sys.executable,
                    "-m",
                    "tools.flywheel.factory.contamination",
                    "--corpus",
                    str(corpus_path),
                    "--gate",
                    str(GATE_PATH),
                    "--out",
                    str(report_path),
                ],
                cwd=REPO_ROOT,
                capture_output=True,
                text=True,
            )
            self.assertNotEqual(result.returncode, 0)
            report = json.loads(report_path.read_text(encoding="utf-8"))
            self.assertFalse(report["clean"])


if __name__ == "__main__":
    unittest.main()
