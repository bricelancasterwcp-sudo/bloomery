"""G5's extension to the corpus generator (design doc §5 / task-4 brief),
split out of `test_generate.py` to keep that file under the 400-line house
cap (same reasoning turn 1 used for `templates_python.py`/`templates_text.py`):

`generate.py` now emits refuse tasks too (via `--refusal-count`), verified
through the tool with `expect="refuse"`. Omitting the flag must stay
byte-identical to turn-1 behavior — `test_generate.py`'s existing suite
(unmodified) is that regression pin; these tests only cover the NEW
additive path.
"""

import json
import unittest
from pathlib import Path

from tools.flywheel.tests.test_generate import STUB_TOOL, run_generate


class RefusalGenerationTest(unittest.TestCase):
    def test_refusal_count_adds_two_pair_refuse_rows_alongside_three_pair_patch_rows(self):
        import tempfile

        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            out, report = tmp / "out.jsonl", tmp / "report.json"
            result = run_generate(
                [
                    "--seed", "9", "--count", "6", "--refusal-count", "4", "--tool", str(STUB_TOOL),
                    "--out", str(out), "--report", str(report),
                ]
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            lines = [json.loads(line) for line in out.read_text(encoding="utf-8").splitlines()]
            by_task = {}
            for row in lines:
                by_task.setdefault(row["meta"]["task_id"], []).append(row["meta"])
            patch_tasks = {tid: metas for tid, metas in by_task.items() if metas[0]["expect"] == "patch"}
            refuse_tasks = {tid: metas for tid, metas in by_task.items() if metas[0]["expect"] == "refuse"}
            self.assertEqual(len(patch_tasks), 6)
            self.assertEqual(len(refuse_tasks), 4)
            # Turn 3: the patch slice cycles three trajectory shapes with
            # three pair sequences; the refuse shape's 2-pair sequence is
            # what this test actually pins, alongside "a patch task always
            # ends in `patch` -> `done`" regardless of shape.
            pairs_by_shape = {
                "plain": ["read", "patch", "done"],
                "find": ["find", "read", "patch", "done"],
                "run": ["read", "patch", "run", "done"],
            }
            for metas in patch_tasks.values():
                self.assertEqual([m["pair"] for m in metas], pairs_by_shape[metas[0]["trajectory"]])
            for metas in refuse_tasks.values():
                self.assertEqual([m["pair"] for m in metas], ["read", "done"])

    def test_refusal_tasks_cover_both_lenses(self):
        import tempfile

        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            out, report = tmp / "out.jsonl", tmp / "report.json"
            run_generate(
                [
                    "--seed", "9", "--count", "0", "--refusal-count", "8", "--tool", str(STUB_TOOL),
                    "--out", str(out), "--report", str(report),
                ]
            )
            lines = [json.loads(line) for line in out.read_text(encoding="utf-8").splitlines()]
            lenses = {row["meta"]["lens"] for row in lines}
            self.assertEqual(lenses, {"python", "plaintext"})

    def test_omitting_refusal_count_produces_no_refuse_rows(self):
        import tempfile

        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            out, report = tmp / "out.jsonl", tmp / "report.json"
            run_generate(
                ["--seed", "9", "--count", "6", "--tool", str(STUB_TOOL), "--out", str(out), "--report", str(report)]
            )
            lines = [json.loads(line) for line in out.read_text(encoding="utf-8").splitlines()]
            self.assertTrue(all(row["meta"]["expect"] == "patch" for row in lines))

    def test_refusal_generation_is_deterministic_across_identical_seeds(self):
        import tempfile

        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            out_a, report_a = tmp / "a.jsonl", tmp / "a.json"
            out_b, report_b = tmp / "b.jsonl", tmp / "b.json"
            run_generate(
                [
                    "--seed", "21", "--count", "6", "--refusal-count", "6", "--tool", str(STUB_TOOL),
                    "--out", str(out_a), "--report", str(report_a),
                ]
            )
            run_generate(
                [
                    "--seed", "21", "--count", "6", "--refusal-count", "6", "--tool", str(STUB_TOOL),
                    "--out", str(out_b), "--report", str(report_b),
                ]
            )
            self.assertEqual(out_a.read_bytes(), out_b.read_bytes())
            self.assertEqual(report_a.read_text(encoding="utf-8"), report_b.read_text(encoding="utf-8"))

    def test_validation_split_spans_both_patch_and_refuse_task_ids(self):
        import tempfile

        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            out, report = tmp / "out.jsonl", tmp / "report.json"
            run_generate(
                [
                    "--seed", "31", "--count", "100", "--refusal-count", "100", "--tool", str(STUB_TOOL),
                    "--out", str(out), "--report", str(report),
                ]
            )
            fp = json.loads(report.read_text(encoding="utf-8"))
            all_ids = set()
            with out.open(encoding="utf-8") as f:
                for line in f:
                    all_ids.add(json.loads(line)["meta"]["task_id"])
            val_ids = fp["val_split_ids"]
            self.assertTrue(set(val_ids).issubset(all_ids))
            # 100 patch + 100 refuse = 200 total tasks; 5% of 200 = 10, so
            # the split is large enough that BOTH id shapes should appear
            # if the split genuinely draws from the combined pool.
            self.assertTrue(any("refuse" in tid for tid in val_ids), val_ids)
            self.assertTrue(any("refuse" not in tid for tid in val_ids), val_ids)

    def test_fingerprint_counts_include_refusal_templates(self):
        import tempfile

        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            out, report = tmp / "out.jsonl", tmp / "report.json"
            run_generate(
                [
                    "--seed", "9", "--count", "0", "--refusal-count", "8", "--tool", str(STUB_TOOL),
                    "--out", str(out), "--report", str(report),
                ]
            )
            fp = json.loads(report.read_text(encoding="utf-8"))
            self.assertTrue(any(name.startswith("refusal_") for name in fp["tasks_by_template"]))


class RefusalVerificationUnitTest(unittest.TestCase):
    """`_verify_and_build_rows` must abort (never silently accept) when the
    tool's response does not confirm `verified: "refusal"` for a refuse
    task — the sanity check that stands in for a landing check (refuse
    tasks have no landing check at all, per the task-3 wire contract)."""

    def test_aborts_when_the_tool_does_not_confirm_a_refusal(self):
        from tools.flywheel.factory import generate
        from tools.flywheel.factory.task import DEFECT_ABSENT, RefusalTask

        task = RefusalTask(
            name="fam",
            lens="python",
            family=DEFECT_ABSENT,
            target="a.py",
            target_missing=False,
            files={"a.py": "def add(a, b):\n    return a + b\n\n\ndef sub(a, b):\n    return a - b\n"},
            goal="a.py's `add` looks wrong TRIGGER_VERIFIED_MISMATCH -- check first, then emit done.",
            refusal_reason="No change needed.",
        )
        with self.assertRaises(SystemExit):
            generate._verify_and_build_rows([("t1", task)], STUB_TOOL)


if __name__ == "__main__":
    unittest.main()
