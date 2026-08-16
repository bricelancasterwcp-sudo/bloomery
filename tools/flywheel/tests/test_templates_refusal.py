"""Tests for the G5 refusal template families (design doc §5;
`.superpowers/sdd/2026-08-16-flywheel2-honest-refusal/task-4-brief.md`
step 1).

Two families, both lenses:
- defect-absent: the generated file is genuinely CORRECT; the goal claims a
  plausible-but-FALSE defect. The plausibility rule is mechanically
  enforced: the goal must backtick-quote an identifier/value that is a
  literal substring of the generated file's real contents (design doc §5:
  "otherwise the model learns 'weird goal -> refuse' instead of 'check
  first'").
- missing-target: the goal names a file NOT among the fixture's files, with
  >= 1 real sibling file present so the directory is not suspiciously
  empty; `refusal_reason` names the missing file.

Same determinism contract as `test_templates.py`: pure `(rng) -> RefusalTask`
functions, byte-identical output for an identically-seeded `rng`.
"""

import random
import unittest

from tools.flywheel.factory import templates
from tools.flywheel.factory.contamination import GATE_VOCABULARY
from tools.flywheel.factory.task import (
    DEFECT_ABSENT,
    MISSING_TARGET,
    REFUSAL_QUOTED_RE,
    RefusalTask,
    validate_refusal_task,
)


class RefusalTemplateGroupsTest(unittest.TestCase):
    """The four (family, lens) groups each carry at least one template
    family, and `templates.REFUSAL_TEMPLATES` is their union."""

    def test_all_four_groups_present_and_non_empty(self):
        for group in (
            "defect_absent_python",
            "defect_absent_plaintext",
            "missing_target_python",
            "missing_target_plaintext",
        ):
            self.assertIn(group, templates.REFUSAL_GROUPS)
            self.assertGreaterEqual(
                len(templates.REFUSAL_GROUPS[group]), 1, f"group {group} has no templates"
            )

    def test_refusal_templates_is_the_union_of_all_groups(self):
        total = sum(len(v) for v in templates.REFUSAL_GROUPS.values())
        self.assertEqual(len(templates.REFUSAL_TEMPLATES), total)

    def test_refusal_template_names_are_unique(self):
        names = [name for name, _fn in templates.REFUSAL_TEMPLATES]
        self.assertEqual(len(names), len(set(names)))


class RefusalTemplatesAreDeterministicTest(unittest.TestCase):
    def test_every_refusal_family_is_a_pure_function_of_its_rng(self):
        for name, fn in templates.REFUSAL_TEMPLATES:
            task_a = fn(random.Random(4242))
            task_b = fn(random.Random(4242))
            self.assertEqual(task_a, task_b, f"{name} is not deterministic given the same rng seed")

    def test_families_produce_varied_output_across_seeds(self):
        for name, fn in templates.REFUSAL_TEMPLATES:
            seen = {fn(random.Random(seed)).goal for seed in range(30)}
            self.assertGreater(
                len(seen), 12, f"{name}'s value space looks too narrow ({len(seen)}/30 unique goals)"
            )


class RefusalTemplateShapeTest(unittest.TestCase):
    def test_every_template_returns_a_refusal_task_with_a_known_family_and_lens(self):
        for name, fn in templates.REFUSAL_TEMPLATES:
            for seed in range(10):
                task = fn(random.Random(seed))
                self.assertIsInstance(task, RefusalTask, name)
                self.assertIn(task.family, (DEFECT_ABSENT, MISSING_TARGET), name)
                self.assertIn(task.lens, ("python", "plaintext"), name)

    def test_every_generated_refusal_task_is_structurally_valid(self):
        for name, fn in templates.REFUSAL_TEMPLATES:
            for seed in range(25):
                task = fn(random.Random(seed))
                violations = validate_refusal_task(task)
                self.assertEqual(violations, [], f"{name} seed={seed}: {violations}\n{task}")


class DefectAbsentPlausibilityTest(unittest.TestCase):
    """The load-bearing mechanical check (design doc §5): a defect-absent
    goal's backtick-quoted identifier/value must be a real substring of the
    generated (correct) file's contents."""

    def test_defect_absent_templates_quote_a_real_identifier_present_in_the_file(self):
        for group_name in ("defect_absent_python", "defect_absent_plaintext"):
            for name, fn in templates.REFUSAL_GROUPS[group_name]:
                for seed in range(10):
                    task = fn(random.Random(seed))
                    self.assertEqual(task.family, DEFECT_ABSENT, name)
                    contents = task.files[task.target]
                    quoted = REFUSAL_QUOTED_RE.findall(task.goal)
                    self.assertTrue(quoted, f"{name} seed={seed}: goal has no backtick-quoted span: {task.goal!r}")
                    self.assertTrue(
                        any(q in contents for q in quoted),
                        f"{name} seed={seed}: none of {quoted} appear in the target's real contents",
                    )

    def test_the_plausibility_check_actually_fails_on_a_fabricated_goal(self):
        # Sanity check on the mechanism itself: a goal that quotes something
        # NOT present in the file must be caught by validate_refusal_task,
        # proving the assertion above isn't vacuously true.
        bad = RefusalTask(
            name="unit_test_bad",
            lens="python",
            family=DEFECT_ABSENT,
            target="a.py",
            target_missing=False,
            files={"a.py": "def add(a, b):\n    return a + b\n\n\ndef sub(a, b):\n    return a - b\n"},
            goal="a.py's `nonexistent_fn` looks wrong -- check a.py and fix if it really is wrong.",
            refusal_reason="No change needed.",
        )
        violations = validate_refusal_task(bad)
        self.assertTrue(any("plausibility rule" in v for v in violations), violations)


class MissingTargetShapeTest(unittest.TestCase):
    def test_missing_target_templates_never_include_the_target_among_files(self):
        for group_name in ("missing_target_python", "missing_target_plaintext"):
            for name, fn in templates.REFUSAL_GROUPS[group_name]:
                for seed in range(10):
                    task = fn(random.Random(seed))
                    self.assertEqual(task.family, MISSING_TARGET, name)
                    self.assertTrue(task.target_missing, name)
                    self.assertNotIn(task.target, task.files, f"{name} seed={seed}")

    def test_missing_target_templates_always_have_a_real_sibling_file(self):
        for group_name in ("missing_target_python", "missing_target_plaintext"):
            for name, fn in templates.REFUSAL_GROUPS[group_name]:
                for seed in range(10):
                    task = fn(random.Random(seed))
                    self.assertGreaterEqual(len(task.files), 1, f"{name} seed={seed}: no sibling file")

    def test_missing_target_refusal_reason_names_the_missing_file(self):
        for group_name in ("missing_target_python", "missing_target_plaintext"):
            for name, fn in templates.REFUSAL_GROUPS[group_name]:
                for seed in range(10):
                    task = fn(random.Random(seed))
                    self.assertIn(task.target, task.refusal_reason, f"{name} seed={seed}")


class RefusalValidationMechanicsTest(unittest.TestCase):
    """Direct unit coverage of `validate_refusal_task`'s own rules, mirroring
    `StructuralValidityTest` in `test_templates.py`."""

    def _valid_defect_absent(self):
        return RefusalTask(
            name="unit_test_family",
            lens="python",
            family=DEFECT_ABSENT,
            target="mathy.py",
            target_missing=False,
            files={"mathy.py": "def add(a, b):\n    return a + b\n\n\ndef sub(a, b):\n    return a - b\n"},
            goal="mathy.py's `add` looks miscalculated -- check mathy.py and fix if it really is wrong.",
            refusal_reason="No change needed: add() in mathy.py is already correct.",
        )

    def _valid_missing_target(self):
        return RefusalTask(
            name="unit_test_family",
            lens="python",
            family=MISSING_TARGET,
            target="ghost.py",
            target_missing=True,
            files={"sibling.py": "def noop():\n    return None\n\n\ndef other():\n    return 1\n"},
            goal="ghost.py's helper() looks broken -- check ghost.py and fix if it really is wrong.",
            refusal_reason="Cannot: ghost.py does not exist in this workspace.",
        )

    def test_valid_defect_absent_task_has_no_violations(self):
        self.assertEqual(validate_refusal_task(self._valid_defect_absent()), [])

    def test_valid_missing_target_task_has_no_violations(self):
        self.assertEqual(validate_refusal_task(self._valid_missing_target()), [])

    def test_defect_absent_target_must_be_among_files(self):
        bad = self._valid_defect_absent()._replace(target="other.py")
        violations = validate_refusal_task(bad)
        self.assertTrue(any("must be among files" in v for v in violations))

    def test_missing_target_target_must_not_be_among_files(self):
        bad = self._valid_missing_target()._replace(target="sibling.py", target_missing=False)
        violations = validate_refusal_task(bad)
        self.assertTrue(any("must NOT be among files" in v for v in violations))

    def test_missing_target_requires_at_least_one_sibling_file(self):
        bad = self._valid_missing_target()._replace(files={})
        violations = validate_refusal_task(bad)
        self.assertTrue(any("real sibling file" in v for v in violations))

    def test_goal_must_contain_target_filename(self):
        bad = self._valid_defect_absent()._replace(goal="Something is `add`. Check first.")
        violations = validate_refusal_task(bad)
        self.assertTrue(any("target filename" in v for v in violations))

    def test_empty_refusal_reason_is_a_violation(self):
        bad = self._valid_defect_absent()._replace(refusal_reason="   ")
        violations = validate_refusal_task(bad)
        self.assertTrue(any("refusal_reason is empty" in v for v in violations))

    def test_target_missing_flag_inconsistent_with_files_is_a_violation(self):
        bad = self._valid_defect_absent()._replace(target_missing=True)
        violations = validate_refusal_task(bad)
        self.assertTrue(any("inconsistent" in v for v in violations))


class RefusalVocabularyDisjointFromGateSetTest(unittest.TestCase):
    """Same rule-1 hygiene `test_templates.py` pins for repair templates,
    applied to refusal templates: no gate-set target filename leaks out."""

    def test_generated_refusal_targets_never_surface_gate_target_filenames(self):
        for name, fn in templates.REFUSAL_TEMPLATES:
            for seed in range(10):
                task = fn(random.Random(seed))
                self.assertNotIn(
                    task.target.lower(),
                    GATE_VOCABULARY,
                    f"{name} produced a gate-set target filename: {task.target!r}",
                )


if __name__ == "__main__":
    unittest.main()
