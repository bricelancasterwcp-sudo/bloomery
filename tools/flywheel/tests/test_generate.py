"""Tests for tools.flywheel.factory.generate — brief rules 3, 4, 5, 6.

Rule 3 (determinism): --seed N twice -> byte-identical corpus.
Rule 4 (verification): every task goes through flywheel-tool trajectory;
landed:false ABORTS generation with the task printed.
Rule 5 (dedup): normalized (goal, target_contents) uniqueness; drops
counted in the fingerprint.
Rule 6 (validation split): 5% of task_ids marked in the fingerprint.

All but one test use the canned stub_tool.py fixture (brief step 1); one
integration test drives the real built flywheel-tool binary end to end.

G5's extension (refuse-task generation via `--refusal-count`, design doc
§5 / task-4 brief) lives in `test_generate_refusal.py` — split out to keep
this file under the 400-line house cap, same reasoning turn 1 used for
`templates_python.py`/`templates_text.py`. It imports `STUB_TOOL` and
`run_generate` from this module.
"""

import json
import subprocess
import sys
import unittest
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[3]
STUB_TOOL = REPO_ROOT / "tools" / "flywheel" / "tests" / "fixtures" / "stub_tool.py"
FAIL_SECOND_TOOL = REPO_ROOT / "tools" / "flywheel" / "tests" / "fixtures" / "fail_second_request_tool.py"


def _find_real_tool():
    for candidate in ("target/release/flywheel-tool", "target/debug/flywheel-tool"):
        path = REPO_ROOT / candidate
        if path.exists():
            return path
    return None


REAL_TOOL = _find_real_tool()


def run_generate(args, cwd=REPO_ROOT):
    return subprocess.run(
        [sys.executable, "-m", "tools.flywheel.factory.generate", *args],
        cwd=cwd,
        capture_output=True,
        text=True,
    )


class DeterminismTest(unittest.TestCase):
    """Rule 3: --seed N twice -> byte-identical corpus."""

    def test_same_seed_produces_byte_identical_corpus_and_fingerprint(self):
        import tempfile

        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            out_a, report_a = tmp / "a.jsonl", tmp / "a.json"
            out_b, report_b = tmp / "b.jsonl", tmp / "b.json"

            result_a = run_generate(
                [
                    "--seed", "7", "--count", "6", "--tool", str(STUB_TOOL),
                    "--out", str(out_a), "--report", str(report_a),
                ]
            )
            result_b = run_generate(
                [
                    "--seed", "7", "--count", "6", "--tool", str(STUB_TOOL),
                    "--out", str(out_b), "--report", str(report_b),
                ]
            )

            self.assertEqual(result_a.returncode, 0, result_a.stderr)
            self.assertEqual(result_b.returncode, 0, result_b.stderr)

            self.assertEqual(out_a.read_bytes(), out_b.read_bytes(), "corpus bytes differ across identical seeds")

            fp_a = json.loads(report_a.read_text(encoding="utf-8"))
            fp_b = json.loads(report_b.read_text(encoding="utf-8"))
            self.assertEqual(fp_a, fp_b)

    def test_different_seeds_produce_different_corpora(self):
        import tempfile

        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            out_a, report_a = tmp / "a.jsonl", tmp / "a.json"
            out_b, report_b = tmp / "b.jsonl", tmp / "b.json"

            run_generate(["--seed", "1", "--count", "6", "--tool", str(STUB_TOOL), "--out", str(out_a), "--report", str(report_a)])
            run_generate(["--seed", "2", "--count", "6", "--tool", str(STUB_TOOL), "--out", str(out_b), "--report", str(report_b)])

            self.assertNotEqual(out_a.read_bytes(), out_b.read_bytes())


class VerificationAbortsOnLandingFailureTest(unittest.TestCase):
    """Rule 4: landed:false ABORTS generation with the task printed —
    never dropped silently. Uses fail_second_request_tool.py, a stub that
    deterministically fails the 2nd trajectory request regardless of
    content — the forcing logic lives in the test fixture, not in
    generate.py's production code path."""

    def test_a_landing_failure_aborts_the_whole_run_nonzero(self):
        import tempfile

        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            out, report = tmp / "out.jsonl", tmp / "report.json"
            result = run_generate(
                [
                    "--seed", "1", "--count", "6", "--tool", str(FAIL_SECOND_TOOL),
                    "--out", str(out), "--report", str(report),
                ]
            )
            self.assertNotEqual(result.returncode, 0)

    def test_forced_failure_prints_the_offending_task_and_does_not_write_the_corpus(self):
        import tempfile

        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            out, report = tmp / "out.jsonl", tmp / "report.json"
            result = run_generate(
                [
                    "--seed", "1", "--count", "6", "--tool", str(FAIL_SECOND_TOOL),
                    "--out", str(out), "--report", str(report),
                ]
            )
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("did not land", result.stderr.lower())
            # The aborting task's goal/target must be visible in the
            # failure output -- "never dropped silently" -- and no
            # corpus/report file should be left behind from a run that
            # never completed.
            self.assertIn("goal", result.stderr.lower())
            self.assertFalse(out.exists())
            self.assertFalse(report.exists())


class DedupUnitTest(unittest.TestCase):
    """Rule 5: normalized (goal, target_contents) uniqueness; drops
    counted. Tested directly against generate.dedup_tasks — a pure
    function — rather than trying to coax the CLI into producing a
    natural collision."""

    def test_exact_duplicate_tasks_are_dropped(self):
        from tools.flywheel.factory import generate, templates

        def make(goal, contents, target="a.py"):
            return templates.Task(
                name="fam", lens="python", target=target, files={target: contents},
                goal=goal, search=contents.splitlines()[0], replace="x", summary="s",
            )

        t1 = make("Fix a.py. Patch the file, then emit done.", "x = 1\ny = 2\nz = 3\na = 4\nb = 5\n")
        t2 = make("Fix a.py. Patch the file, then emit done.", "x = 1\ny = 2\nz = 3\na = 4\nb = 5\n")
        unique, dropped = generate.dedup_tasks([t1, t2])
        self.assertEqual(len(unique), 1)
        self.assertEqual(dropped, 1)

    def test_whitespace_and_case_normalized_duplicates_are_dropped(self):
        from tools.flywheel.factory import generate, templates

        def make(goal, contents, target="a.py"):
            return templates.Task(
                name="fam", lens="python", target=target, files={target: contents},
                goal=goal, search=contents.splitlines()[0], replace="x", summary="s",
            )

        t1 = make("Fix a.py. Patch the file, then emit done.", "x = 1\ny = 2\nz = 3\na = 4\nb = 5\n")
        t2 = make(
            "  FIX   A.PY.   Patch the file, then emit done.  ",
            "X = 1\n  Y = 2\nZ  = 3\na = 4\nb = 5\n",
        )
        unique, dropped = generate.dedup_tasks([t1, t2])
        self.assertEqual(len(unique), 1)
        self.assertEqual(dropped, 1)

    def test_distinct_tasks_are_all_kept(self):
        from tools.flywheel.factory import generate, templates

        def make(goal, contents, target="a.py"):
            return templates.Task(
                name="fam", lens="python", target=target, files={target: contents},
                goal=goal, search=contents.splitlines()[0], replace="x", summary="s",
            )

        t1 = make("Fix a.py. Patch the file, then emit done.", "x = 1\ny = 2\nz = 3\na = 4\nb = 5\n")
        t2 = make("Fix b.py. Patch the file, then emit done.", "p = 1\nq = 2\nr = 3\ns = 4\nt = 5\n", target="b.py")
        unique, dropped = generate.dedup_tasks([t1, t2])
        self.assertEqual(len(unique), 2)
        self.assertEqual(dropped, 0)

    def test_dedup_preserves_first_occurrence_order(self):
        from tools.flywheel.factory import generate, templates

        def make(idx, goal, contents, target="a.py"):
            return templates.Task(
                name=f"fam{idx}", lens="python", target=target, files={target: contents},
                goal=goal, search=contents.splitlines()[0], replace="x", summary="s",
            )

        t1 = make(1, "Fix a.py. Patch the file, then emit done.", "x = 1\ny = 2\nz = 3\na = 4\nb = 5\n")
        t2 = make(2, "Fix b.py. Patch the file, then emit done.", "p = 1\nq = 2\nr = 3\ns = 4\nt = 5\n", target="b.py")
        t3 = make(3, "Fix a.py. Patch the file, then emit done.", "x = 1\ny = 2\nz = 3\na = 4\nb = 5\n")
        unique, dropped = generate.dedup_tasks([t1, t2, t3])
        self.assertEqual([t.name for t in unique], ["fam1", "fam2"])
        self.assertEqual(dropped, 1)


class DedupIntegrationTest(unittest.TestCase):
    def test_normal_run_has_low_dedup_rate_at_moderate_count(self):
        import tempfile

        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            out, report = tmp / "out.jsonl", tmp / "report.json"
            result = run_generate(
                ["--seed", "123", "--count", "200", "--tool", str(STUB_TOOL), "--out", str(out), "--report", str(report)]
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            fp = json.loads(report.read_text(encoding="utf-8"))
            self.assertLess(fp["dedup_dropped"] / 200, 0.05)


class ValidationSplitTest(unittest.TestCase):
    """Rule 6: 5% of task_ids marked in the fingerprint, deterministic."""

    def test_validation_split_is_about_five_percent(self):
        import tempfile

        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            out, report = tmp / "out.jsonl", tmp / "report.json"
            result = run_generate(
                ["--seed", "11", "--count", "200", "--tool", str(STUB_TOOL), "--out", str(out), "--report", str(report)]
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            fp = json.loads(report.read_text(encoding="utf-8"))
            total_tasks = sum(fp["tasks_by_lens"].values())
            val_count = len(fp["val_split_ids"])
            self.assertGreater(val_count, 0)
            ratio = val_count / total_tasks
            self.assertTrue(0.02 <= ratio <= 0.08, f"val split ratio {ratio} not near 5%")

    def test_validation_split_ids_are_a_subset_of_real_task_ids_and_sorted(self):
        import tempfile

        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            out, report = tmp / "out.jsonl", tmp / "report.json"
            run_generate(["--seed", "11", "--count", "200", "--tool", str(STUB_TOOL), "--out", str(out), "--report", str(report)])
            fp = json.loads(report.read_text(encoding="utf-8"))
            all_task_ids = set()
            with out.open(encoding="utf-8") as f:
                for line in f:
                    all_task_ids.add(json.loads(line)["meta"]["task_id"])
            val_ids = fp["val_split_ids"]
            self.assertEqual(val_ids, sorted(val_ids))
            self.assertTrue(set(val_ids).issubset(all_task_ids))

    def test_validation_split_is_deterministic_across_identical_seeds(self):
        import tempfile

        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            report_a, report_b = tmp / "a.json", tmp / "b.json"
            run_generate(["--seed", "55", "--count", "200", "--tool", str(STUB_TOOL), "--out", str(tmp / "a.jsonl"), "--report", str(report_a)])
            run_generate(["--seed", "55", "--count", "200", "--tool", str(STUB_TOOL), "--out", str(tmp / "b.jsonl"), "--report", str(report_b)])
            fp_a = json.loads(report_a.read_text(encoding="utf-8"))
            fp_b = json.loads(report_b.read_text(encoding="utf-8"))
            self.assertEqual(fp_a["val_split_ids"], fp_b["val_split_ids"])


class FingerprintShapeTest(unittest.TestCase):
    def test_fingerprint_has_all_required_fields(self):
        import tempfile

        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            out, report = tmp / "out.jsonl", tmp / "report.json"
            result = run_generate(["--seed", "8", "--count", "6", "--tool", str(STUB_TOOL), "--out", str(out), "--report", str(report)])
            self.assertEqual(result.returncode, 0, result.stderr)
            fp = json.loads(report.read_text(encoding="utf-8"))
            for key in ("seed", "tasks_by_template", "tasks_by_lens", "pairs", "dedup_dropped", "corpus_sha256", "val_split_ids"):
                self.assertIn(key, fp)
            self.assertEqual(fp["seed"], 8)

    def test_corpus_sha256_matches_the_actual_written_file(self):
        import hashlib
        import tempfile

        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            out, report = tmp / "out.jsonl", tmp / "report.json"
            run_generate(["--seed", "8", "--count", "6", "--tool", str(STUB_TOOL), "--out", str(out), "--report", str(report)])
            fp = json.loads(report.read_text(encoding="utf-8"))
            self.assertEqual(fp["corpus_sha256"], hashlib.sha256(out.read_bytes()).hexdigest())

    def test_each_surviving_task_yields_exactly_three_pairs(self):
        import tempfile

        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            out, report = tmp / "out.jsonl", tmp / "report.json"
            run_generate(["--seed", "8", "--count", "6", "--tool", str(STUB_TOOL), "--out", str(out), "--report", str(report)])
            lines = out.read_text(encoding="utf-8").splitlines()
            fp = json.loads(report.read_text(encoding="utf-8"))
            self.assertEqual(len(lines), fp["pairs"])
            by_task = {}
            for line in lines:
                row = json.loads(line)
                by_task.setdefault(row["meta"]["task_id"], []).append(row["meta"]["pair"])
            for task_id, pairs in by_task.items():
                self.assertEqual(pairs, ["read", "patch", "done"], f"{task_id}: {pairs}")


@unittest.skipUnless(REAL_TOOL is not None, "flywheel-tool binary not built; run cargo build -p bloomery-daemon --bin flywheel-tool")
class RealToolIntegrationTest(unittest.TestCase):
    """The one integration test the brief requires: drives the real
    built flywheel-tool binary end to end, not the stub."""

    def test_small_corpus_via_the_real_binary(self):
        import tempfile

        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            out, report = tmp / "out.jsonl", tmp / "report.json"
            result = run_generate(
                ["--seed", "1", "--count", "6", "--tool", str(REAL_TOOL), "--out", str(out), "--report", str(report)]
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            fp = json.loads(report.read_text(encoding="utf-8"))
            self.assertGreater(fp["pairs"], 0)
            lines = [json.loads(line) for line in out.read_text(encoding="utf-8").splitlines()]
            self.assertGreater(len(lines), 0)
            for row in lines:
                self.assertIn("prompt", row)
                self.assertIn("completion", row)
                self.assertTrue(row["prompt"].strip())
                self.assertTrue(row["completion"].strip())
            # Pair 2 ("patch") completions must carry the real SearchReplace
            # conflict-marker grammar the real tool emits.
            patch_rows = [r for r in lines if r["meta"]["pair"] == "patch"]
            self.assertTrue(patch_rows)
            for row in patch_rows:
                self.assertIn("<<<<<<< SEARCH", row["completion"])
                self.assertIn(">>>>>>> REPLACE", row["completion"])


if __name__ == "__main__":
    unittest.main()
