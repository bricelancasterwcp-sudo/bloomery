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
    the factory instead of at render time."""

    def _make_valid_task(self):
        contents = "def add(a, b):\n    return a + b\n\n\ndef sub(a, b):\n    return a - b\n"
        return templates.Task(
            name="unit_run_family",
            lens="python",
            target="mathy.py",
            files={"mathy.py": contents},
            goal="mathy.py's add() is broken. Patch the file, then emit done.",
            search="    return a + b",
            replace="    return a + b  # ok",
            summary="Fixed add().",
            trajectory=task_mod.RUN_TRAJECTORY,
            run_argv=("python3", "-m", "py_compile", "mathy.py"),
            commands=(("python3", "-m", "py_compile"),),
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
        bad = task._replace(goal="add() is broken. Patch the file, then emit done.")
        violations = templates.validate_task(bad)
        self.assertTrue(any("target filename" in v for v in violations), violations)


if __name__ == "__main__":
    unittest.main()
