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


class DefectAbsentClaimIsProvablyFalseTest(unittest.TestCase):
    """Gate review round 1 (Critical, task-4 fix round 1): backtick-quoting
    a real identifier (`DefectAbsentPlausibilityTest`, above) is necessary
    but not SUFFICIENT — a goal can quote a real value and still make a
    claim that is mechanically GUARANTEED true by construction (the
    `_defect_absent_config_value_txt` bug: a claimed floor drawn from an
    independent range that could never be satisfied by the file's own
    value range). Each test below re-derives both the file's real
    number/direction and the goal's claimed one straight from the
    generated TEXT (never the template's internal variables, so a future
    refactor that keeps the variables right but the rendered text wrong
    would still be caught) and asserts the numeric/directional relation
    that makes the claim false, for every one of the four defect-absent
    template functions."""

    def test_config_value_claimed_floor_is_always_strictly_below_the_real_value(self):
        import re

        from tools.flywheel.factory.templates_refusal_text import _defect_absent_config_value_txt

        key_value_re = re.compile(r"^(\S+) = (\d+)$", re.MULTILINE)
        # No trailing punctuation anchor (task 6a's goal-phrasing skeleton
        # diversity means the claim is no longer always the goal's final
        # clause ending in "?" -- some skeletons end it with "." or splice
        # more text after it; the claimed-floor NUMBER itself is what this
        # test cares about, preserved verbatim by every skeleton).
        floor_re = re.compile(r"at least (\d+)")
        for seed in range(200):
            task = _defect_absent_config_value_txt(random.Random(seed))
            contents = task.files[task.target]
            key_match = key_value_re.search(contents)
            self.assertIsNotNone(key_match, f"seed={seed}: no key/value line found in {contents!r}")
            real_value = int(key_match.group(2))
            floor_match = floor_re.search(task.goal)
            self.assertIsNotNone(floor_match, f"seed={seed}: no claimed floor found in {task.goal!r}")
            claimed_floor = int(floor_match.group(1))
            self.assertLess(
                claimed_floor,
                real_value,
                f"seed={seed}: claimed floor {claimed_floor} is NOT below the real value "
                f"{real_value} -- the 'no defect' refusal_reason would be unfounded",
            )

    def test_version_string_claimed_tag_always_differs_from_the_real_one(self):
        import re

        from tools.flywheel.factory.templates_refusal_text import _defect_absent_version_string_txt

        heading_re = re.compile(r"## (\S+) - ")
        claim_re = re.compile(r"read `(\S+)` instead")
        for seed in range(200):
            task = _defect_absent_version_string_txt(random.Random(seed))
            contents = task.files[task.target]
            heading_match = heading_re.search(contents)
            self.assertIsNotNone(heading_match, f"seed={seed}: no version heading found in {contents!r}")
            real_version = heading_match.group(1)
            claim_match = claim_re.search(task.goal)
            self.assertIsNotNone(claim_match, f"seed={seed}: no claimed version found in {task.goal!r}")
            claimed_version = claim_match.group(1)
            self.assertNotEqual(
                claimed_version,
                real_version,
                f"seed={seed}: claimed version {claimed_version!r} equals the real one -- not a false claim",
            )

    def test_wrong_multiplier_claimed_factor_always_differs_from_the_real_one(self):
        import re

        from tools.flywheel.factory.templates_refusal_python import _defect_absent_wrong_multiplier_py

        return_line_re = re.compile(r"return value \* ([\d.]+)")
        claim_re = re.compile(r"multiply by ([\d.]+) instead of `([\d.]+)`")
        for seed in range(200):
            task = _defect_absent_wrong_multiplier_py(random.Random(seed))
            contents = task.files[task.target]
            return_match = return_line_re.search(contents)
            self.assertIsNotNone(return_match, f"seed={seed}: no return line found in {contents!r}")
            real_factor = return_match.group(1)
            claim_match = claim_re.search(task.goal)
            self.assertIsNotNone(claim_match, f"seed={seed}: no claimed/real factor found in {task.goal!r}")
            claimed_factor, stated_real_factor = claim_match.group(1), claim_match.group(2)
            self.assertEqual(
                stated_real_factor, real_factor, f"seed={seed}: goal's stated real factor doesn't match the file"
            )
            self.assertNotEqual(
                claimed_factor,
                real_factor,
                f"seed={seed}: claimed factor {claimed_factor!r} equals the real one -- not a false claim",
            )

    def test_wrong_comparison_claimed_direction_always_differs_from_the_real_one(self):
        import re

        from tools.flywheel.factory.templates_refusal_python import _defect_absent_wrong_comparison_py

        op_re = re.compile(r"if x ([<>]) best:")
        claim_re = re.compile(r"returns the (\w+) \w+ instead of the (\w+) one")
        for seed in range(200):
            task = _defect_absent_wrong_comparison_py(random.Random(seed))
            contents = task.files[task.target]
            op_match = op_re.search(contents)
            self.assertIsNotNone(op_match, f"seed={seed}: no comparison operator found in {contents!r}")
            real_direction = "highest" if op_match.group(1) == ">" else "lowest"
            claim_match = claim_re.search(task.goal)
            self.assertIsNotNone(claim_match, f"seed={seed}: no claimed/real direction found in {task.goal!r}")
            claimed_direction, stated_real_direction = claim_match.group(1), claim_match.group(2)
            self.assertEqual(
                stated_real_direction,
                real_direction,
                f"seed={seed}: goal's stated real direction doesn't match the file's actual comparison",
            )
            self.assertNotEqual(
                claimed_direction,
                real_direction,
                f"seed={seed}: claimed direction equals the real one -- not a false claim",
            )


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
