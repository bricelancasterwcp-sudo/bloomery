"""Tests for `task.validate_task`'s per-shape branches (turn-3 task-7
brief): the rules that decide whether a find-shaped or run-verified repair
task is structurally sound before a byte of it is rendered.

Split out of `test_templates.py` when the turn-3 branch pins pushed that
file past the 400-line house cap — the same split
`test_templates_multifile.py` made for the template families themselves.
The seam: `test_templates.py` keeps rules 1-2 (the registries, the
word-list disjointness proof, and the SHARED validator rules every shape
takes); this file owns what each shape adds or inverts.

Every test here pairs a valid task with the ONE mutation that flips
exactly the rule under test — a rule nothing can flip is a rule that is
not enforced.
"""

import re
import unittest

from tools.flywheel.factory import task as task_mod
from tools.flywheel.factory import templates


class TrajectoryFieldDefaultsTest(unittest.TestCase):
    """The new fields are defaulted, so every pre-turn-3 construction site
    (eight positional args) still builds a plain task -- the additive
    contract the whole turn rests on."""

    def test_eight_positional_args_still_build_a_plain_task(self):
        contents = "x = 1\ny = 2\nz = 3\na = 4\nb = 5\n"
        task = templates.Task(
            "fam", "python", "a.py", {"a.py": contents},
            "Fix a.py. Patch the file, then emit done.", "x = 1", "x = 9", "s",
        )
        self.assertEqual(task.trajectory, task_mod.PLAIN_TRAJECTORY)
        self.assertEqual(task.find_pattern, "")
        self.assertEqual(task.run_argv, ())
        self.assertEqual(task.commands, ())
        self.assertEqual(templates.validate_task(task), [])

    def test_an_unknown_trajectory_is_a_violation(self):
        contents = "x = 1\ny = 2\nz = 3\na = 4\nb = 5\n"
        task = templates.Task(
            "fam", "python", "a.py", {"a.py": contents},
            "Fix a.py. Patch the file, then emit done.", "x = 1", "x = 9", "s",
        )._replace(trajectory="teleport")
        violations = templates.validate_task(task)
        self.assertTrue(any("trajectory" in v for v in violations), violations)


class FindShapedValidationTest(unittest.TestCase):
    """Find-shaped branch: the goal must NOT name the target filename
    (the plain rule inverted -- the model has to search for the file), the
    `find_pattern` must occur in the target and in NO sibling, and the goal
    still ends with DONE_INSTRUCTION."""

    TARGET = "reeflog.py"
    SIBLING = "brinecheck.py"
    CONTENTS = (
        "SALINITY_LIMIT = 34\n"
        "\n"
        "\n"
        "def resolve_salinity_band(reading):\n"
        "    if reading < SALINITY_LIMIT:\n"
        '        return "over"\n'
        '    return "under"\n'
    )
    SIBLING_CONTENTS = "def mirror_coral_count_records(rows):\n    return list(rows)\n"

    def _make_valid_task(self):
        return templates.Task(
            name="unit_find_family",
            lens="python",
            target=self.TARGET,
            files={self.TARGET: self.CONTENTS, self.SIBLING: self.SIBLING_CONTENTS},
            goal=(
                "resolve_salinity_band() labels readings under the limit as \"over\". "
                "Patch the file, then emit done."
            ),
            search="    if reading < SALINITY_LIMIT:",
            replace="    if reading > SALINITY_LIMIT:",
            summary="Fixed the comparison in resolve_salinity_band().",
            trajectory=task_mod.FIND_TRAJECTORY,
            find_pattern="def resolve_salinity_band",
        )

    def test_valid_find_task_has_no_violations(self):
        self.assertEqual(templates.validate_task(self._make_valid_task()), [])

    def test_a_goal_that_names_the_target_filename_is_a_violation(self):
        task = self._make_valid_task()
        bad = task._replace(goal=f"In {self.TARGET}: {task.goal}")
        violations = templates.validate_task(bad)
        self.assertTrue(any("names the target filename" in v for v in violations), violations)

    def test_a_find_pattern_absent_from_the_target_is_a_violation(self):
        task = self._make_valid_task()
        bad = task._replace(find_pattern="def resolve_nothing_at_all")
        violations = templates.validate_task(bad)
        self.assertTrue(any("does not occur in the target" in v for v in violations), violations)

    def test_a_find_pattern_that_also_occurs_in_a_sibling_is_a_violation(self):
        task = self._make_valid_task()
        bad = task._replace(
            files={
                self.TARGET: self.CONTENTS,
                self.SIBLING: self.SIBLING_CONTENTS + "\n\ndef resolve_salinity_band(x):\n    return x\n",
            }
        )
        violations = templates.validate_task(bad)
        self.assertTrue(any("sibling" in v for v in violations), violations)

    def test_an_empty_find_pattern_is_a_violation(self):
        task = self._make_valid_task()
        violations = templates.validate_task(task._replace(find_pattern=""))
        self.assertTrue(any("find_pattern" in v for v in violations), violations)

    def test_a_find_pattern_carrying_a_regex_metacharacter_is_a_violation(self):
        """The precondition that makes the two uniqueness rules above mean
        anything: they are SUBSTRING checks, but `exec_find` compiles the
        pattern as a REGEX and the two only coincide for a literal.

        `ops.team` is literally in the target and literally NOT in the
        sibling, so both uniqueness rules pass — but the compiled regex's
        `.` matches the sibling's `opsxteam`, and the tool rejects only a
        ZERO-match find. Without this rule that task renders a wrong-file
        hit into trained text with nothing failing anywhere. The setup
        asserts each half of that, so the test cannot rot into passing for
        some other reason."""
        task = self._make_valid_task()
        contents = self.CONTENTS + "\n\n# owner: ops.team\n"
        sibling = self.SIBLING_CONTENTS + "\n# owner: opsxteam\n"
        bad = task._replace(
            files={self.TARGET: contents, self.SIBLING: sibling},
            find_pattern="ops.team",
        )
        # Invisible to the other two rules...
        self.assertIn(bad.find_pattern, contents)
        self.assertNotIn(bad.find_pattern, sibling)
        # ...but the regex the tool would actually run hits the sibling.
        self.assertIsNotNone(re.search(bad.find_pattern, sibling))

        violations = templates.validate_task(bad)
        self.assertTrue(any("regex literal" in v for v in violations), violations)

    def test_a_multi_line_find_pattern_is_a_violation(self):
        # The other half of the same precondition: `exec_find` matches line
        # by line, so a pattern spanning a newline can never match anything
        # -- even though plain substring containment says it is right there.
        task = self._make_valid_task()
        bad = task._replace(find_pattern="def resolve_salinity_band(reading):\n    if reading")
        violations = templates.validate_task(bad)
        self.assertTrue(any("regex literal" in v for v in violations), violations)

    def test_a_find_goal_must_still_end_with_the_done_instruction(self):
        task = self._make_valid_task()
        bad = task._replace(goal="resolve_salinity_band() mislabels readings. Please fix it.")
        violations = templates.validate_task(bad)
        self.assertTrue(any("Patch the file, then emit done." in v for v in violations), violations)

    def test_the_plain_search_rule_still_applies_to_a_find_task(self):
        task = self._make_valid_task()
        bad = task._replace(search="not present anywhere")
        violations = templates.validate_task(bad)
        self.assertTrue(any("exactly once" in v for v in violations), violations)


class RunVerifiedValidationTest(unittest.TestCase):
    """Run-verified branch: every plain rule still holds (the goal DOES
    name the target), plus a non-empty `run_argv` that starts with one of
    the granted `commands` prefixes -- the same element-wise prefix match
    the real `Grant` applies, so a task the tool would refuse is caught in
    the factory instead of at render time.

    Turn 4 adds the **fails-before rule** (spec §3). Turn 3's verification
    was `py_compile`, which cannot fail on a semantic defect: the run step
    trained the HABIT of verifying but verified nothing. The rebuilt slice
    plants a `unittest` beside the target, and the factory proves that test
    can actually fail by executing it against the UNPATCHED workspace and
    requiring a nonzero exit. (The tool's own real run is the other half:
    it executes the same test against the PATCHED file and refuses to
    render a trajectory unless it exits 0.) A planted test that passes
    before the patch would render an ideal whose `run` step proves nothing
    -- exactly turn 3's failure, one layer deeper -- so it is a named
    structural violation here."""

    CONTENTS = (
        "def add(a, b):\n"
        "    # Return the sum of a and b.\n"
        "    return a - b\n"
        "\n"
        "\n"
        "def double(a):\n"
        "    return a * 2\n"
    )

    def _planted_test(self, expected):
        return (
            "import unittest\n"
            "\n"
            "import mathy\n"
            "\n"
            "\n"
            "class TestMathy(unittest.TestCase):\n"
            "    def test_add(self):\n"
            f"        self.assertEqual(mathy.add(3, 5), {expected})\n"
            "\n"
            "\n"
            'if __name__ == "__main__":\n'
            "    unittest.main()\n"
        )

    def _make_valid_task(self, expected=8):
        return templates.Task(
            name="unit_run_family",
            lens="python",
            target="mathy.py",
            files={"mathy.py": self.CONTENTS, "test_mathy.py": self._planted_test(expected)},
            goal="mathy.py's add() subtracts. Patch the file, then emit done.",
            search="    return a - b",
            replace="    return a + b",
            summary="Fixed add().",
            trajectory=task_mod.RUN_TRAJECTORY,
            run_argv=("python3", "-m", "unittest", "test_mathy.py"),
            commands=(("python3", "-m", "unittest"),),
            test_file="test_mathy.py",
        )

    def test_valid_run_task_has_no_violations(self):
        self.assertEqual(templates.validate_task(self._make_valid_task()), [])

    def test_an_empty_run_argv_is_a_violation(self):
        task = self._make_valid_task()
        violations = templates.validate_task(task._replace(run_argv=()))
        self.assertTrue(any("run_argv" in v for v in violations), violations)

    def test_a_run_argv_outside_every_granted_prefix_is_a_violation(self):
        task = self._make_valid_task()
        bad = task._replace(commands=(("cargo", "test"),))
        violations = templates.validate_task(bad)
        self.assertTrue(any("granted command prefix" in v for v in violations), violations)

    def test_an_empty_granted_prefix_does_not_vacuously_grant_everything(self):
        # `Grant`'s own wire parser rejects an empty prefix; a validator
        # that accepted one would grant every argv by accident, which is
        # the one way this rule could be silently vacuous.
        task = self._make_valid_task()
        bad = task._replace(commands=((),))
        violations = templates.validate_task(bad)
        self.assertTrue(any("granted command prefix" in v for v in violations), violations)

    def test_a_run_goal_must_still_name_the_target_filename(self):
        task = self._make_valid_task()
        bad = task._replace(goal="add() subtracts. Patch the file, then emit done.")
        violations = templates.validate_task(bad)
        self.assertTrue(any("target filename" in v for v in violations), violations)

    # --- the fails-before rule -------------------------------------------------

    def test_a_planted_test_that_already_passes_before_the_patch_is_a_violation(self):
        """The mutation that proves the rule is enforced rather than
        decorative: assert the BUGGY behavior instead of the fixed one and
        the planted test passes against the unpatched file, which makes the
        ideal's `run` step vacuous."""
        task = self._make_valid_task(expected=-2)
        # The mutation really is a pre-patch pass, not a broken test:
        # add(3, 5) is 3 - 5 == -2 before the patch.
        violations = templates.validate_task(task)
        self.assertTrue(any("passes against the unpatched" in v for v in violations), violations)

    def test_a_run_shaped_task_with_no_test_file_is_a_violation(self):
        task = self._make_valid_task()
        violations = templates.validate_task(task._replace(test_file=""))
        self.assertTrue(any("test_file" in v for v in violations), violations)

    def test_a_test_file_that_is_not_among_the_task_files_is_a_violation(self):
        task = self._make_valid_task()
        bad = task._replace(test_file="test_absent.py")
        violations = templates.validate_task(bad)
        self.assertTrue(any("not among" in v for v in violations), violations)

    def test_a_run_argv_that_does_not_name_the_planted_test_is_a_violation(self):
        # Still inside the grant, so the prefix rule passes -- but the run
        # would not execute the test the fails-before rule just cleared,
        # and the two halves of the proof would be about different commands.
        task = self._make_valid_task()
        bad = task._replace(run_argv=("python3", "-m", "unittest", "discover"))
        violations = templates.validate_task(bad)
        self.assertTrue(any("does not name" in v for v in violations), violations)

    def test_the_fails_before_rule_does_not_fire_for_a_plain_task(self):
        # The expensive branch must belong to the run shape alone: a plain
        # task carries no test_file and must still validate clean.
        task = self._make_valid_task()._replace(
            trajectory=task_mod.PLAIN_TRAJECTORY, run_argv=(), commands=(), test_file=""
        )
        self.assertEqual(templates.validate_task(task), [])


if __name__ == "__main__":
    unittest.main()
