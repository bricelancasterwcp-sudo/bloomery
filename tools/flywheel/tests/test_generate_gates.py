"""Tests for tools.flywheel.factory.generate's task 6a extension:
gate-aware rejection sampling via repeated `--gate` arguments.

`test_gate_sampling.py` covers the `RejectionSampler`/
`GateOverlapTooDenseError` machinery directly against hand-built
fixtures; this file covers the CLI wiring end to end -- real templates,
the real `generate.py` subprocess, the real `STUB_TOOL`. Split out of
`test_generate.py` to keep that file under the 400-line house cap (same
reasoning turn 1 used for `templates_python.py`/`templates_text.py`).
"""

import hashlib
import json
import random
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

from tools.flywheel.factory import contamination, templates
from tools.flywheel.factory.wordlists import THEMES
from tools.flywheel.tests.test_generate import REPO_ROOT, STUB_TOOL, run_generate


def _toml_string(s: str) -> str:
    """Escapes `s` for embedding as a TOML basic (`"..."`) string value."""
    escaped = s.replace("\\", "\\\\").replace('"', '\\"').replace("\n", "\\n")
    return f'"{escaped}"'


def _write_gate_toml(path: Path, name: str, goal: str, target: str = "unused.py") -> Path:
    """A minimal, syntactically valid gate TOML with ONE `expect="patch"`
    fixture -- everything `gate_vocabulary.load_gate_fixtures` needs and
    nothing more. `[fixture.reference]` is omitted (the parser tolerates
    that; `search`/`replace` come back `None`, which the guard's
    `search_match` rule already treats as "nothing to compare")."""
    toml_text = (
        f'set = {_toml_string("test-gate")}\n\n'
        "[[fixture]]\n"
        f"name = {_toml_string(name)}\n"
        'lens = "python"\n'
        f"target = {_toml_string(target)}\n"
        f"goal = {_toml_string(goal)}\n\n"
        "[[fixture.file]]\n"
        f"path = {_toml_string(target)}\n"
        'contents = "x = 1\\n"\n'
    )
    path.write_text(toml_text, encoding="utf-8")
    return path


def _write_universal_python_target_collision_gate_toml(path: Path) -> Path:
    """A gate engineered to collide with EVERY possible python-lens
    candidate via `target_filename_match` alone -- a rule that compares
    only the target filename, never the goal text, so it is completely
    independent of goal-phrasing skeleton diversity (unlike planting a
    single goal string, which -- now that every family offers >= 4
    genuinely different skeletons -- no longer collides with every
    redraw; that used to be exactly how this abort path was pinned,
    before the skeleton-diversity fix made single-goal planting an
    unreliable way to force a collision). Every python family draws its
    target as `f"{stem}.py"` from `wordlists.THEMES`'s `file_stems`
    (`_theme_and_target`, `templates_python.py`) -- a small, fully
    enumerable set (40 filenames). One fixture per filename guarantees
    every draw, regardless of family, identifiers, or skeleton, matches
    some fixture's target exactly."""
    all_python_targets = sorted({f"{stem}.py" for theme in THEMES for stem in theme.file_stems})
    lines = [f'set = {_toml_string("universal-target-collision")}', ""]
    for i, target in enumerate(all_python_targets):
        lines.extend(
            [
                "[[fixture]]",
                f"name = {_toml_string(f'planted-{i:03d}')}",
                'lens = "python"',
                f"target = {_toml_string(target)}",
                f"goal = {_toml_string('irrelevant -- this fixture collides on target filename alone')}",
                "",
                "[[fixture.file]]",
                f"path = {_toml_string(target)}",
                'contents = "x = 1\\n"',
                "",
            ]
        )
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")
    return path


def _harmless_gate_toml(path: Path) -> Path:
    """A gate distinctive enough that no factory template's word-list-
    driven output should ever collide with it -- for tests that only
    care that `--gate` PLUMBING works (determinism, fingerprint shape),
    not that a collision occurs."""
    return _write_gate_toml(
        path,
        name="harmless-unrelated-fixture",
        goal="reticulate the xenoglot spline registry urgently before lunch. Patch the file, then emit done.",
        target="xenoglot_spline_registry_zzz.py",
    )


def _expected_accepted_candidate(seed: int, gate_fixtures, max_draws: int = 50):
    """Replays the REAL slot-0 family function (`templates.PYTHON_TEMPLATES[0]`
    -- the family `--count 1` uses for its single slot) against a fresh
    rng seeded the same way generate.py's own would be, screening each
    draw with the REAL `contamination.task_violates_gates` -- the exact
    function the rejection sampler calls. Returns (accepted_task,
    n_rejected_before_it), so a test can assert the CLI's actual output
    matches whatever the real rule set would keep, without hard-coding
    an assumption about how many redraws a given seed/gate needs."""
    _, fn = templates.PYTHON_TEMPLATES[0]
    rng = random.Random(seed)
    for i in range(max_draws):
        task = fn(rng)
        if contamination.task_violates_gates(task, gate_fixtures) is None:
            return task, i
    raise AssertionError(f"no accepted candidate found for seed={seed} within {max_draws} draws")


class DeterminismWithGatesTest(unittest.TestCase):
    def test_same_seed_and_gates_produce_byte_identical_corpus_and_fingerprint(self):
        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            gate_path = _harmless_gate_toml(tmp / "gate.toml")
            out_a, report_a = tmp / "a.jsonl", tmp / "a.json"
            out_b, report_b = tmp / "b.jsonl", tmp / "b.json"

            args = ["--seed", "42", "--count", "10", "--gate", str(gate_path), "--tool", str(STUB_TOOL)]
            result_a = run_generate(args + ["--out", str(out_a), "--report", str(report_a)])
            result_b = run_generate(args + ["--out", str(out_b), "--report", str(report_b)])

            self.assertEqual(result_a.returncode, 0, result_a.stderr)
            self.assertEqual(result_b.returncode, 0, result_b.stderr)
            self.assertEqual(out_a.read_bytes(), out_b.read_bytes())
            self.assertEqual(report_a.read_text(encoding="utf-8"), report_b.read_text(encoding="utf-8"))


class CollidingCandidateIsDroppedAndReplacedTest(unittest.TestCase):
    """A candidate colliding with a gate goal is dropped and the SAME rng
    stream draws the next candidate for that slot instead -- proven by
    planting a gate whose goal matches a REAL template's known
    first-draw output at a fixed seed, then confirming the generated
    corpus carries whatever LATER draw the real rule set would accept,
    never the colliding first draw."""

    def test_a_first_draw_collision_is_replaced_by_a_later_draw_from_the_same_slot(self):
        seed = 100
        _, fn = templates.PYTHON_TEMPLATES[0]
        first_draw = fn(random.Random(seed))

        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            gate_path = _write_gate_toml(tmp / "gate.toml", "planted-first-draw-collision", first_draw.goal)
            gate_fixtures = contamination.load_gate_fixtures(gate_path)
            expected_task, n_rejected = _expected_accepted_candidate(seed, gate_fixtures)
            self.assertGreaterEqual(n_rejected, 1, "test setup didn't actually force a rejection")

            out, report = tmp / "out.jsonl", tmp / "report.json"
            result = run_generate(
                [
                    "--seed", str(seed), "--count", "1", "--gate", str(gate_path),
                    "--tool", str(STUB_TOOL), "--out", str(out), "--report", str(report),
                ]
            )
            self.assertEqual(result.returncode, 0, result.stderr)

            rows = [json.loads(line) for line in out.read_text(encoding="utf-8").splitlines()]
            goals = {row["meta"]["goal"] for row in rows}
            self.assertEqual(goals, {expected_task.goal})
            self.assertNotIn(first_draw.goal, goals)

            fp = json.loads(report.read_text(encoding="utf-8"))
            self.assertEqual(sum(fp["gate_rejections"].values()), n_rejected)
            self.assertIn("goal_match", fp["gate_rejections"])
            self.assertEqual(fp["gate_paths"], [str(gate_path)])
            self.assertEqual(
                fp["gates_sha256"][str(gate_path)], hashlib.sha256(gate_path.read_bytes()).hexdigest()
            )


class AbortsWhenGateOverlapIsTooDenseTest(unittest.TestCase):
    """Requirement 2's termination guard, exercised through the real
    CLI: a gate engineered to collide with EVERY possible python-lens
    candidate via `target_filename_match` (see
    `_write_universal_python_target_collision_gate_toml` -- goal-
    independent by construction, unlike planting a single colliding
    goal, which the goal-phrasing skeleton-diversity fix makes an
    unreliable way to force a permanent collision) must abort nonzero
    with the named error's message, and must leave no corpus or report
    file behind (same "never a partial result" contract every other
    factory-bug abort in generate.py already has)."""

    def test_cli_aborts_nonzero_and_leaves_no_output_when_a_slot_cannot_clear_the_gate(self):
        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            gate_path = _write_universal_python_target_collision_gate_toml(tmp / "gate.toml")
            out, report = tmp / "out.jsonl", tmp / "report.json"

            result = run_generate(
                [
                    "--seed", "7", "--count", "1", "--gate", str(gate_path),
                    "--tool", str(STUB_TOOL), "--out", str(out), "--report", str(report),
                ]
            )
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("too dense", result.stderr.lower())
            self.assertFalse(out.exists())
            self.assertFalse(report.exists())


class ContaminationGuardStillCatchesPlantedViolationTest(unittest.TestCase):
    """Regression: task 6a's refactor of contamination.py (extracting
    `_violations_for_task`/`task_violates_gates` so the rejection sampler
    can reuse the SAME rule set) must not weaken or break the standalone
    post-hoc guard CLI -- it remains a fully independent safety net even
    when a corpus was written without ever being screened against a
    given gate."""

    def test_standalone_guard_cli_still_catches_a_planted_violation(self):
        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            gate_path = _write_gate_toml(
                tmp / "gate.toml",
                "planted-fixture",
                "fix z.txt urgently before the deadline passes today. Patch the file, then emit done.",
                target="z.txt",
            )
            corpus_path = tmp / "corpus.jsonl"
            row = {
                "prompt": "p",
                "completion": "c",
                "meta": {
                    "task_id": "t1",
                    "template": "unit",
                    "lens": "plaintext",
                    "pair": "read",
                    "goal": "fix z.txt urgently before the deadline passes today. Patch the file, then emit done.",
                    "target": "z.txt",
                    "target_contents": "irrelevant\n",
                    "search": "irrelevant",
                },
            }
            corpus_path.write_text(json.dumps(row) + "\n", encoding="utf-8")
            report_path = tmp / "report.json"

            result = subprocess.run(
                [
                    sys.executable, "-m", "tools.flywheel.factory.contamination",
                    "--corpus", str(corpus_path), "--gate", str(gate_path), "--out", str(report_path),
                ],
                cwd=REPO_ROOT,
                capture_output=True,
                text=True,
            )
            self.assertNotEqual(result.returncode, 0)
            report = json.loads(report_path.read_text(encoding="utf-8"))
            self.assertFalse(report["clean"])
            self.assertTrue(any(v["rule"] == "goal_match" for v in report["violations"]))


class FingerprintGateFieldsTest(unittest.TestCase):
    def test_gate_fields_present_and_populated_when_a_gate_is_given(self):
        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            gate_path = _harmless_gate_toml(tmp / "gate.toml")
            out, report = tmp / "out.jsonl", tmp / "report.json"
            result = run_generate(
                [
                    "--seed", "9", "--count", "5", "--gate", str(gate_path),
                    "--tool", str(STUB_TOOL), "--out", str(out), "--report", str(report),
                ]
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            fp = json.loads(report.read_text(encoding="utf-8"))
            for key in ("gate_paths", "gates_sha256", "gate_rejections"):
                self.assertIn(key, fp)
            self.assertEqual(fp["gate_paths"], [str(gate_path)])
            self.assertEqual(
                fp["gates_sha256"], {str(gate_path): hashlib.sha256(gate_path.read_bytes()).hexdigest()}
            )
            self.assertIsInstance(fp["gate_rejections"], dict)

    def test_gate_fields_present_but_empty_when_no_gate_is_given(self):
        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            out, report = tmp / "out.jsonl", tmp / "report.json"
            result = run_generate(
                ["--seed", "9", "--count", "5", "--tool", str(STUB_TOOL), "--out", str(out), "--report", str(report)]
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            fp = json.loads(report.read_text(encoding="utf-8"))
            self.assertEqual(fp["gate_paths"], [])
            self.assertEqual(fp["gates_sha256"], {})
            self.assertEqual(fp["gate_rejections"], {})


if __name__ == "__main__":
    unittest.main()
