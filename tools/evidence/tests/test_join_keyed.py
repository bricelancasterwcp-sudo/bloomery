"""Unit tests for the keyed CodecFixture<->TaskStep join (turn-5 spec §3)."""
import json
import tempfile
import unittest
from pathlib import Path

from tools.evidence.journal import join, load_rows

SET = "codec-tasks-v4-mixed"


def _fixture(name, agent, steps, expect, epoch_ms):
    row = {"event": "CodecFixture", "fixture": name, "fixture_set": SET,
           "steps": steps, "expect": expect, "landed": True, "epoch_ms": epoch_ms}
    if agent is not None:
        row["agent"] = agent
    return row


def _step(id_, step, epoch_ms):
    return {"event": "TaskStep", "id": id_, "step": step, "verb": "read",
            "outcome": "read 1 bytes", "duration_ms": 0, "args": [], "epoch_ms": epoch_ms}


def _write(tmp, name, rows):
    path = Path(tmp) / name
    with path.open("w") as f:
        for r in rows:
            f.write(json.dumps(r) + "\n")
    return path


TASKS = [_step("a1", 1, 100), _step("a1", 2, 500), _step("a2", 1, 1500)]

# Two EQUAL-length groups (both 2 steps) so a swap can't be caught by the
# `len(steps) != fx.get("steps")` check alone — it must be caught by
# comparing the joined rows themselves (item 1).
SWAP_TASKS = [
    _step("a1", 1, 100), _step("a1", 2, 500),
    _step("a2", 1, 1500), _step("a2", 2, 1800),
]


def _join(fixtures, tasks=TASKS):
    with tempfile.TemporaryDirectory() as tmp:
        jpath = _write(tmp, "journal.jsonl", fixtures)
        tpath = _write(tmp, "tasks.jsonl", tasks)
        return join(load_rows(jpath), load_rows(tpath))


class KeyedJoin(unittest.TestCase):
    def test_agreeing_keys_join_clean(self):
        fixtures = [_fixture("v4-patch-find-1", "a1", 2, "patch", 1000),
                    _fixture("v4-refuse-defect-absent-1", "a2", 1, "refuse", 2000)]
        joined, report = _join(fixtures)
        self.assertEqual(report.mode, "keyed")
        self.assertIs(report.keyed_equals_ordinal, True)
        self.assertEqual(report.violations, [])
        for j, expected_len in zip(joined, (2, 1)):
            self.assertEqual(len(j.steps), expected_len)

    def test_swapped_keys_flag_keyed_ne_ordinal(self):
        # Both fixtures claim `steps=2`, matching BOTH groups' length (2
        # each) — the length check in `_keyed`/`_ordinal` cannot catch this
        # swap, only a row-level comparison can.
        fixtures = [_fixture("v4-patch-find-1", "a2", 2, "patch", 1000),
                    _fixture("v4-refuse-defect-absent-1", "a1", 2, "refuse", 2000)]
        _, report = _join(fixtures, tasks=SWAP_TASKS)
        self.assertIn("keyed != ordinal", report.violations)

    def test_old_style_journal_is_ordinal_only(self):
        fixtures = [_fixture("v4-patch-find-1", None, 2, "patch", 1000),
                    _fixture("v4-refuse-defect-absent-1", None, 1, "refuse", 2000)]
        _, report = _join(fixtures)
        self.assertEqual(report.mode, "ordinal")
        self.assertIsNone(report.keyed_equals_ordinal)


if __name__ == "__main__":
    unittest.main()
