"""G5-specific extensions to the contamination guard (task-4 brief), split
out of `test_contamination.py` to keep that file under the 400-line house
cap (same reasoning turn 1 used for `templates_python.py`/`templates_text.py`):

1. `load_gate_fixtures` must handle `expect = "refuse"` fixtures (no
   `[fixture.reference]`) without crashing, and `check_corpus`'s
   `search_match` rule must not false-positive against one.
2. The CLI accepts MULTIPLE `--gate` arguments; checks run against the
   union of all given gates, so a plant in EITHER set is caught.
3. `codec-tasks-v2-mixed` (factory-authored, frozen) must be disjoint from
   `codec-tasks-v1` — the CRITICAL disjointness requirement, checked via
   the same contamination guard with v2-mixed exported as a pseudo-corpus.
"""

import json
import unittest
from pathlib import Path

from tools.flywheel.factory import contamination

REPO_ROOT = Path(__file__).resolve().parents[3]
GATE_PATH = REPO_ROOT / "crates" / "bloomery-daemon" / "fixtures" / "codec-tasks-v1.toml"
V2_MIXED_GATE_PATH = REPO_ROOT / "crates" / "bloomery-daemon" / "fixtures" / "codec-tasks-v2-mixed.toml"


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


class LoadGateFixturesHandlesRefuseTest(unittest.TestCase):
    """G5 design doc §2: a gate TOML may now contain `expect = "refuse"`
    fixtures, which carry no `[fixture.reference]`. `load_gate_fixtures`
    must not KeyError on that shape, and must record `search`/`replace` as
    `None` (nothing to compare a corpus's `search` string against) rather
    than fabricating empty strings that could accidentally exact-match a
    genuinely search-less corpus row."""

    def _write_mixed_toml(self, tmp_path):
        toml_text = (
            'set = "test-mixed"\n\n'
            "[[fixture]]\n"
            'name = "p1"\n'
            'lens = "plaintext"\n'
            'target = "a.txt"\n'
            'goal = "fix a.txt"\n\n'
            "[[fixture.file]]\n"
            'path = "a.txt"\n'
            'contents = "broken\\n"\n\n'
            "[fixture.reference]\n"
            'search = "broken"\n'
            'replace = "fixed"\n\n'
            "[[fixture]]\n"
            'name = "r1"\n'
            'lens = "plaintext"\n'
            'target = "b.txt"\n'
            'goal = "b.txt looks miscalculated -- check first"\n'
            'expect = "refuse"\n'
            'refusal_reason = "No change needed: b.txt is already correct."\n\n'
            "[[fixture.file]]\n"
            'path = "b.txt"\n'
            'contents = "totals: 4\\n"\n'
        )
        path = tmp_path / "mixed.toml"
        path.write_text(toml_text, encoding="utf-8")
        return path

    def test_a_refuse_fixture_with_no_reference_does_not_crash(self):
        import tempfile

        with tempfile.TemporaryDirectory() as tmp:
            path = self._write_mixed_toml(Path(tmp))
            fixtures = contamination.load_gate_fixtures(path)
            self.assertEqual(len(fixtures), 2)
            refuse_fx = next(f for f in fixtures if f.name == "r1")
            self.assertIsNone(refuse_fx.search)
            self.assertIsNone(refuse_fx.replace)
            self.assertEqual(refuse_fx.expect, "refuse")
            self.assertEqual(refuse_fx.refusal_reason, "No change needed: b.txt is already correct.")

    def test_a_patch_fixture_still_carries_its_search_and_replace(self):
        import tempfile

        with tempfile.TemporaryDirectory() as tmp:
            path = self._write_mixed_toml(Path(tmp))
            fixtures = contamination.load_gate_fixtures(path)
            patch_fx = next(f for f in fixtures if f.name == "p1")
            self.assertEqual(patch_fx.search, "broken")
            self.assertEqual(patch_fx.replace, "fixed")
            self.assertEqual(patch_fx.expect, "patch")

    def test_check_corpus_does_not_false_positive_search_match_against_a_refuse_fixture(self):
        import tempfile

        with tempfile.TemporaryDirectory() as tmp:
            path = self._write_mixed_toml(Path(tmp))
            fixtures = contamination.load_gate_fixtures(path)
            rows = [
                _corpus_row(
                    "t1",
                    "Something unrelated about a telescope calibration routine at dawn. "
                    "Patch the file, then emit done.",
                    "harmless.py",
                    "x = 1\ny = 2\nz = 3\na = 4\nb = 5\n",
                    "",
                )
            ]
            report = contamination.check_corpus(rows, fixtures)
            self.assertTrue(report.clean, report.violations)


class MultiGateCliTest(unittest.TestCase):
    """Task 4 brief: the CLI accepts MULTIPLE `--gate` arguments; checks run
    against the union of all given gates, so a plant in EITHER set is
    caught."""

    def _write_gate(self, tmp_path, filename, fixture_name, goal, target, contents, search, replace):
        toml_text = (
            f'set = "{filename}"\n\n'
            "[[fixture]]\n"
            f'name = "{fixture_name}"\n'
            'lens = "plaintext"\n'
            f'target = "{target}"\n'
            f'goal = "{goal}"\n\n'
            "[[fixture.file]]\n"
            f'path = "{target}"\n'
            f'contents = "{contents}\\n"\n\n'
            "[fixture.reference]\n"
            f'search = "{search}"\n'
            f'replace = "{replace}"\n'
        )
        path = tmp_path / filename
        path.write_text(toml_text, encoding="utf-8")
        return path

    def test_a_plant_in_the_first_gate_is_caught_when_multiple_gates_are_given(self):
        import tempfile

        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            gate_a = self._write_gate(
                tmp, "gate_a.toml", "fx-a", "fix a.txt's alpha line", "a.txt", "alpha-broken", "broken", "fixed"
            )
            gate_b = self._write_gate(
                tmp, "gate_b.toml", "fx-b", "fix b.txt's beta line", "b.txt", "beta-broken", "broken", "fixed"
            )
            fixtures = contamination.load_gate_fixtures(gate_a) + contamination.load_gate_fixtures(gate_b)
            rows = [_corpus_row("t1", "fix a.txt's alpha line", "harmless.py", "x = 1\ny = 2\nz = 3\na = 4\nb = 5\n", "x = 1")]
            report = contamination.check_corpus(rows, fixtures)
            self.assertFalse(report.clean)
            self.assertTrue(any(v["gate_fixture"] == "fx-a" for v in report.violations))

    def test_a_plant_in_the_second_gate_is_caught_when_multiple_gates_are_given(self):
        import tempfile

        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            gate_a = self._write_gate(
                tmp, "gate_a.toml", "fx-a", "fix a.txt's alpha line", "a.txt", "alpha-broken", "broken", "fixed"
            )
            gate_b = self._write_gate(
                tmp, "gate_b.toml", "fx-b", "fix b.txt's beta line", "b.txt", "beta-broken", "broken", "fixed"
            )
            fixtures = contamination.load_gate_fixtures(gate_a) + contamination.load_gate_fixtures(gate_b)
            rows = [_corpus_row("t1", "fix b.txt's beta line", "harmless.py", "x = 1\ny = 2\nz = 3\na = 4\nb = 5\n", "x = 1")]
            report = contamination.check_corpus(rows, fixtures)
            self.assertFalse(report.clean)
            self.assertTrue(any(v["gate_fixture"] == "fx-b" for v in report.violations))

    def test_cli_accepts_multiple_gate_flags_and_catches_a_plant_in_either(self):
        import subprocess
        import sys
        import tempfile

        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            gate_a = self._write_gate(
                tmp, "gate_a.toml", "fx-a", "fix a.txt's alpha line", "a.txt", "alpha-broken", "broken", "fixed"
            )
            gate_b = self._write_gate(
                tmp, "gate_b.toml", "fx-b", "fix b.txt's beta line", "b.txt", "beta-broken", "broken", "fixed"
            )
            corpus_path = tmp / "corpus.jsonl"
            report_path = tmp / "report.json"
            row = _corpus_row(
                "t1", "fix b.txt's beta line", "harmless.py", "x = 1\ny = 2\nz = 3\na = 4\nb = 5\n", "x = 1"
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
                    str(gate_a),
                    "--gate",
                    str(gate_b),
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
            self.assertEqual(report["gate_fixtures_checked"], 2)

    def test_cli_still_accepts_a_single_gate_flag_backward_compatibly(self):
        import subprocess
        import sys
        import tempfile

        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            corpus_path = tmp / "corpus.jsonl"
            report_path = tmp / "report.json"
            row = _corpus_row(
                "t1",
                "orbitwatch.py's exposure_minutes() divides by the wrong count, so "
                "exposure_minutes([4, 6]) returns 2.5 instead of 5.0. Fix exposure_minutes() in "
                "orbitwatch.py so it divides by the reading count. Patch the file, then emit done.",
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
            self.assertEqual(report["gate_fixtures_checked"], 20)


class V2MixedDisjointFromV1GuardTest(unittest.TestCase):
    """Task 4 CRITICAL disjointness requirement: `codec-tasks-v2-mixed`
    (factory-authored, frozen) must be disjoint from `codec-tasks-v1`
    (names, goals, contents). `codec-tasks-v2-mixed` is itself
    FACTORY-MADE, not a training corpus, so the guard's normal direction
    ("corpus vs gate") inverts here — the mechanism chosen (documented in
    the frozen TOML's own header comment): export every v2-mixed FILE (not
    just its declared target — every file a fixture carries, so a
    missing-target fixture's real sibling content is checked too) as a
    pseudo-corpus row and run the SAME `check_corpus` comparator with v1
    as the gate. This reuses the exact machine check rule 7 already
    proves for real corpora, rather than inventing a second, parallel
    comparator for gate-vs-gate disjointness."""

    def setUp(self):
        self.v1_fixtures = contamination.load_gate_fixtures(GATE_PATH)
        self.v2_fixtures = contamination.load_gate_fixtures(V2_MIXED_GATE_PATH)

    def _v2_mixed_as_pseudo_corpus_rows(self):
        rows = []
        for fx in self.v2_fixtures:
            for path, contents in sorted(fx.files.items()):
                rows.append(
                    _corpus_row(
                        f"{fx.name}::{path}",
                        fx.goal,
                        path,
                        contents,
                        fx.search or "",
                    )
                )
        return rows

    def test_v2_mixed_has_the_frozen_shape(self):
        self.assertEqual(len(self.v2_fixtures), 20)
        patch = [f for f in self.v2_fixtures if f.expect == "patch"]
        refuse = [f for f in self.v2_fixtures if f.expect == "refuse"]
        self.assertEqual(len(patch), 10)
        self.assertEqual(len(refuse), 10)

    def test_v2_mixed_fixture_names_are_disjoint_from_v1(self):
        # check_corpus compares goal/target/contents/search, never fixture
        # `name` -- names are checked directly here as a complementary,
        # cheap structural assertion (the brief's "names, goals, contents").
        v1_names = {f.name for f in self.v1_fixtures}
        v2_names = {f.name for f in self.v2_fixtures}
        overlap = v1_names & v2_names
        self.assertEqual(overlap, set(), f"fixture names shared between v1 and v2-mixed: {overlap}")

    def test_v2_mixed_is_disjoint_from_v1_via_the_contamination_guard(self):
        rows = self._v2_mixed_as_pseudo_corpus_rows()
        report = contamination.check_corpus(rows, self.v1_fixtures)
        self.assertTrue(
            report.clean,
            f"codec-tasks-v2-mixed is NOT disjoint from codec-tasks-v1: {report.violations}",
        )

    def test_v2_mixed_generating_seed_differs_from_the_turn1_corpus_seed(self):
        # Recorded in the TOML's own header comment (task-4 brief: "the
        # gate-set generation seed must differ from every corpus seed").
        # Turn 1's corpus seed is 20260816
        # (docs/superpowers/evidence/2026-08-16-g4-flywheel1.md).
        header = V2_MIXED_GATE_PATH.read_text(encoding="utf-8")
        self.assertIn("FROZEN", header)
        self.assertIn("dedicated generating seed = 8160816", header)
        self.assertNotIn("generating seed = 20260816", header)


if __name__ == "__main__":
    unittest.main()
