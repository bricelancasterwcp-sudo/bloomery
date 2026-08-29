"""Pins for the eval-time instrument-row binding (turn-7 adversarial
review F-1, 2026-08-29): every fixed-denominator numerator upstream of
the turn-7 floors counts journal ROWS, and before this binding a
duplicated fixture row inflated them while `recompute` exited 0. The
integration test performs exactly the review's demonstrated surgery —
one committed fixture's verdict row and steps re-keyed under a fresh
agent id and appended — and requires exit 2 with the duplicate named.
"""

import io
import json
import tempfile
import unittest
from contextlib import redirect_stdout
from pathlib import Path

from tools.evidence.endpoints import instrument_row_check
from tools.evidence.recompute import main as recompute_main

REPO = Path(__file__).resolve().parents[3]
EVIDENCE = REPO / "docs/superpowers/evidence"
JOURNAL = EVIDENCE / "2026-08-29-g5v5-flywheel5-boot1-journal.jsonl"
TASKS = EVIDENCE / "2026-08-29-g5v5-flywheel5-boot1-tasks.jsonl"
FIXTURES = REPO / "crates/bloomery-daemon/fixtures/codec-tasks-v5-mixed.toml"


class InstrumentRowCheckUnitTest(unittest.TestCase):
    EXPECTED = {"a", "b", "c"}

    def test_clean(self):
        check = instrument_row_check(["a", "b", "c"], self.EXPECTED)
        self.assertEqual(
            check,
            {"expected": 3, "seen": 3, "duplicates": [], "unknown": [], "missing": []},
        )

    def test_a_duplicate_is_named(self):
        check = instrument_row_check(["a", "b", "b", "c"], self.EXPECTED)
        self.assertEqual(check["duplicates"], ["b"])
        self.assertEqual(check["seen"], 4)

    def test_an_unknown_row_is_named(self):
        check = instrument_row_check(["a", "b", "c", "z"], self.EXPECTED)
        self.assertEqual(check["unknown"], ["z"])

    def test_a_missing_fixture_is_reported_not_fatal_shaped(self):
        check = instrument_row_check(["a", "b"], self.EXPECTED)
        self.assertEqual(check["missing"], ["c"])
        self.assertEqual(check["duplicates"], [])


class RecomputeInstrumentBindingTest(unittest.TestCase):
    def _recompute(self, journal, tasks):
        stdout = io.StringIO()
        with redirect_stdout(stdout):
            code = recompute_main(
                ["--journal", str(journal), "--tasks", str(tasks), "--g5-fixtures", str(FIXTURES)]
            )
        return code, json.loads(stdout.getvalue())

    def test_the_committed_boot_is_clean_and_carries_the_binding(self):
        code, report = self._recompute(JOURNAL, TASKS)
        self.assertEqual(code, 0)
        self.assertEqual(report["instrument_rows"]["expected"], 32)
        self.assertEqual(report["instrument_rows"]["seen"], 32)
        self.assertEqual(report["instrument_rows"]["duplicates"], [])
        self.assertEqual(report["instrument_rows"]["unknown"], [])
        self.assertEqual(report["instrument_rows"]["missing"], [])

    def test_a_duplicated_fixture_row_is_exit_2_with_the_duplicate_named(self):
        journal_rows = [json.loads(line) for line in JOURNAL.read_text().splitlines()]
        task_rows = [json.loads(line) for line in TASKS.read_text().splitlines()]
        victim = next(
            r for r in journal_rows
            if r.get("event") == "CodecFixture" and r.get("fixture_set") == "codec-tasks-v5-mixed"
        )
        fresh = "a999999"
        dup = dict(victim, agent=fresh)
        dup_steps = [dict(s, id=fresh) for s in task_rows
                     if s.get("event") == "TaskStep" and s.get("id") == victim["agent"]]
        self.assertTrue(dup_steps)
        with tempfile.TemporaryDirectory() as raw:
            tmp = Path(raw)
            forged_journal = tmp / "journal.jsonl"
            forged_tasks = tmp / "tasks.jsonl"
            forged_journal.write_text(
                "".join(json.dumps(r) + "\n" for r in journal_rows + [dup]), encoding="utf-8"
            )
            forged_tasks.write_text(
                "".join(json.dumps(r) + "\n" for r in task_rows + dup_steps), encoding="utf-8"
            )
            code, report = self._recompute(forged_journal, forged_tasks)
        self.assertEqual(code, 2)
        self.assertEqual(report["instrument_rows"]["duplicates"], [victim["fixture"]])
        self.assertTrue(any("instrument rows" in v for v in report["join"]["violations"]))


if __name__ == "__main__":
    unittest.main()
