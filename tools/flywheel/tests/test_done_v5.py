"""Tests for `task.done_v5` (turn-6 spec §4.3): the canonical v5 `done`
assembler — the fixture contract now and the training-signal contract for
turn 7. One place, tested; templates and fixture authoring never
hand-concatenate the action text."""

import unittest

from tools.flywheel.factory.task import done_v5


class DoneV5Test(unittest.TestCase):
    def test_refused_with_evidence_lines(self):
        text = done_v5(
            outcome="refused",
            reason="different-defect",
            evidence_lines=[
                "evidence: src/lib.rs:14 `return (min(spans), min(spans))`",
            ],
            prose=(
                "The goal describes a crash on empty input; the real defect is the "
                "copy-pasted min. No change made without a goal that matches."
            ),
        )
        self.assertEqual(
            text,
            '<action verb="done" outcome="refused" reason="different-defect">\n'
            "evidence: src/lib.rs:14 `return (min(spans), min(spans))`\n"
            "The goal describes a crash on empty input; the real defect is the "
            "copy-pasted min. No change made without a goal that matches.\n"
            "</action>",
        )

    def test_absent_evidence_for_no_such_file(self):
        text = done_v5(
            outcome="refused",
            reason="no-such-file",
            evidence_lines=["evidence: brine_notes.txt absent"],
            prose="The goal names brine_notes.txt; the workspace does not contain it.",
        )
        self.assertIn("evidence: brine_notes.txt absent\n", text)
        self.assertTrue(text.startswith('<action verb="done" outcome="refused" reason="no-such-file">'))

    def test_multiple_evidence_lines_stay_in_order(self):
        text = done_v5(
            outcome="patched",
            reason="fixed",
            evidence_lines=["evidence: a.py:1 `x = 1`", "evidence: a.py:2 `y = 2`"],
            prose="Adjusted both constants.",
        )
        body = text.split(">\n", 1)[1]
        self.assertTrue(body.startswith("evidence: a.py:1 `x = 1`\nevidence: a.py:2 `y = 2`\n"))

    def test_rejects_unknown_outcome_reason_pairs_and_empty_parts(self):
        with self.assertRaises(ValueError):
            done_v5(outcome="banana", reason="fixed", evidence_lines=["evidence: a absent"], prose="p")
        with self.assertRaises(ValueError):
            done_v5(outcome="patched", reason="no-defect", evidence_lines=["evidence: a absent"], prose="p")
        with self.assertRaises(ValueError):
            done_v5(outcome="refused", reason="no-defect", evidence_lines=[], prose="p")
        with self.assertRaises(ValueError):
            done_v5(outcome="refused", reason="no-defect", evidence_lines=["not an evidence line"], prose="p")
        with self.assertRaises(ValueError):
            done_v5(outcome="refused", reason="no-defect", evidence_lines=["evidence: a absent"], prose=" ")


if __name__ == "__main__":
    unittest.main()
