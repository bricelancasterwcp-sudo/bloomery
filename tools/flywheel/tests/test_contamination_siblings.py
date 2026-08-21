"""Sibling screening: the contamination guard must judge EVERY file a task
carries, never only the declared target.

Split out of `test_contamination.py` when turn 4's filename half pushed
that file past the 400-line house cap — the same split turn 1 used for
`templates_python.py`/`templates_text.py`. The seam is the rule, not the
file size: `test_contamination.py` keeps the rules that read one field of a
task (goal, search, target), and this file owns the two that read the whole
`files` map, because they were widened for the same reason and would rot
together.

The two widenings, in order:

- **contents** (turn 3, CARRIED-DEBT fast-follow): the guard compared only
  the declared target's contents, so a task whose SIBLING was a verbatim
  copy of a gate fixture file passed both the draw-time screen and the
  post-hoc CLI. Turn 3's multi-file repair tasks render siblings into real
  training pairs, which made this a correctness precondition rather than
  hygiene.
- **filenames** (turn 4, spec §3's ride-along — "the last gap in that
  rule"): the same argument, one field later. Turn 4's run slice plants a
  `test_<stem>.py` sibling into every run-verified task, so a rule looking
  only at the target was about to be blind to a file on the corpus's main
  path.

Every test here plants its collision as a SIBLING, never as the target, so
none of them can pass for the pre-widening reason.
"""

import unittest
from pathlib import Path

from tools.flywheel.factory import contamination
from tools.flywheel.factory.task import MISSING_TARGET, RefusalTask, Task
from tools.flywheel.tests.test_contamination import (
    CLEAN_CONTENTS,
    CLEAN_GOAL,
    CLEAN_SEARCH,
    CLEAN_TARGET,
    GATE_PATH,
    _corpus_row,
)

# A gate fixture whose target filename is worth colliding with. Read off
# the real TOML rather than transcribed, so a gate edit that renamed it
# would fail this file loudly instead of quietly making the filename tests
# vacuous.
GATE_FIXTURE = "py-mean-off-by-one"


class SiblingFileScreeningTest(unittest.TestCase):
    """The CONTENTS half: a gate fixture file planted as a sibling."""

    def setUp(self):
        self.fixtures = contamination.load_gate_fixtures(GATE_PATH)
        self.gate_contents = next(f for f in self.fixtures if f.name == GATE_FIXTURE).files["stats.py"]

    def _patch_task(self, files):
        return Task(
            name="unit_test_family",
            lens="python",
            target=CLEAN_TARGET,
            files=files,
            goal=CLEAN_GOAL,
            search=CLEAN_SEARCH,
            replace="    return total / len(readings)",
            summary="fix the divisor",
        )

    def test_the_same_task_without_the_plant_is_clean(self):
        # Pins that the tests below catch the PLANT and not something
        # incidental about the task's goal/target/search.
        clean = self._patch_task({CLEAN_TARGET: CLEAN_CONTENTS})
        self.assertIsNone(contamination.task_violates_gates(clean, self.fixtures))

    def test_draw_time_screen_rejects_a_planted_sibling(self):
        planted = self._patch_task({CLEAN_TARGET: CLEAN_CONTENTS, "sidecar.py": self.gate_contents})
        self.assertEqual(contamination.task_violates_gates(planted, self.fixtures), "file_contents_match")

    def test_draw_time_screen_rejects_a_planted_refusal_sibling(self):
        planted = RefusalTask(
            name="unit_test_family",
            lens="python",
            family=MISSING_TARGET,
            target=CLEAN_TARGET,
            target_missing=True,
            files={"sidecar.py": self.gate_contents},
            goal=CLEAN_GOAL,
            refusal_reason=f"Cannot: {CLEAN_TARGET} does not exist in this workspace.",
        )
        self.assertEqual(contamination.task_violates_gates(planted, self.fixtures), "file_contents_match")

    def test_post_hoc_guard_sees_a_planted_sibling_via_the_row_files_key(self):
        row = _corpus_row(
            "t1",
            CLEAN_GOAL,
            CLEAN_TARGET,
            CLEAN_CONTENTS,
            CLEAN_SEARCH,
            files={CLEAN_TARGET: CLEAN_CONTENTS, "sidecar.py": self.gate_contents},
        )
        report = contamination.check_corpus([row], self.fixtures)
        self.assertFalse(report.clean, "a gate fixture file planted as a sibling must be flagged")
        self.assertTrue(any(v["rule"] == "file_contents_match" for v in report.violations), report.violations)

    def test_a_legacy_row_without_a_files_key_still_has_its_target_checked(self):
        row = _corpus_row("t1", CLEAN_GOAL, CLEAN_TARGET, self.gate_contents, CLEAN_SEARCH)
        self.assertNotIn("files", row["meta"], "this row must be the legacy shape for the fallback to be exercised")
        report = contamination.check_corpus([row], self.fixtures)
        self.assertTrue(any(v["rule"] == "file_contents_match" for v in report.violations), report.violations)


class SiblingFilenameScreeningTest(unittest.TestCase):
    """The FILENAME half (turn-4 spec §3's ride-along). The declared target
    is deliberately clean in every plant below, so none of these can pass
    for the old, target-only reason."""

    def setUp(self):
        self.fixtures = contamination.load_gate_fixtures(GATE_PATH)
        self.gate_target = next(f for f in self.fixtures if f.name == GATE_FIXTURE).target

    def _row(self, files):
        return _corpus_row("t1", CLEAN_GOAL, CLEAN_TARGET, CLEAN_CONTENTS, CLEAN_SEARCH, files=files)

    def test_a_sibling_filename_reusing_a_gate_target_is_caught(self):
        report = contamination.check_corpus(
            [self._row({CLEAN_TARGET: CLEAN_CONTENTS, self.gate_target: "def helper(rows):\n    return list(rows)\n"})],
            self.fixtures,
        )
        self.assertFalse(report.clean)
        matches = [v for v in report.violations if v["rule"] == "target_filename_match"]
        self.assertTrue(matches, report.violations)
        self.assertTrue(any(v["corpus_file"] == self.gate_target for v in matches), matches)

    def test_the_draw_time_screen_catches_it_too(self):
        # Both callers go through the one shared rule set, and this is what
        # holds that claim: a candidate is rejected at DRAW time, before it
        # can become a corpus row at all.
        planted = Task(
            name="unit_test_family",
            lens="python",
            target=CLEAN_TARGET,
            files={CLEAN_TARGET: CLEAN_CONTENTS, self.gate_target: "def helper(rows):\n    return list(rows)\n"},
            goal=CLEAN_GOAL,
            search=CLEAN_SEARCH,
            replace="    return total / len(readings)",
            summary="fix the divisor",
        )
        self.assertEqual(
            contamination.task_violates_gates(planted, self.fixtures), "target_filename_match"
        )

    def test_a_task_whose_filenames_are_all_novel_still_passes(self):
        # Without this, the two tests above could pass because the widened
        # rule fires on every task carrying more than one file.
        report = contamination.check_corpus(
            [
                self._row(
                    {
                        CLEAN_TARGET: CLEAN_CONTENTS,
                        f"test_{Path(CLEAN_TARGET).stem}.py": "def helper(rows):\n    return list(rows)\n",
                    }
                )
            ],
            self.fixtures,
        )
        self.assertEqual(report.violations, [])


if __name__ == "__main__":
    unittest.main()
