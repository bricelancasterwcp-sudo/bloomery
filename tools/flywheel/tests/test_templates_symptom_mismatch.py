"""Tests for the symptom-mismatch refusal family (turn-3 design doc §2,
family 3; `.superpowers/sdd/2026-08-20-flywheel3-turn3/task-5-brief.md`
step 1).

The family's whole point is a goal that is WRONG about the file while the
file is genuinely broken: a real defect Y is planted, and the goal reports
a different defect X that is absent. `RefusalTask` carries no Y field (the
shape is defect-absent's: `target_missing=False`, target among `files`),
so the two proofs that make the family what it claims to be live HERE, not
in the validator:

- X is provably absent — re-derived from the generated TEXT (and, for the
  python lens, from EXECUTING the generated module), never from the
  template's internal variables, so a refactor that keeps the variables
  right but renders the file wrong is still caught. Same technique and
  same reason as `DefectAbsentClaimIsProvablyFalseTest`.
- Y is provably present — the defect relation actually holds in the
  generated file.

A new module, not more of `test_templates_refusal.py`: that file sits at 388 of the 400-line cap.
"""

import random
import re
import unittest

from tools.flywheel.factory import templates, templates_symptom_mismatch_python, templates_symptom_mismatch_text
from tools.flywheel.factory.task import (
    CHECK_INSTRUCTION,
    REFUSAL_FAMILIES,
    REFUSAL_QUOTED_RE,
    SYMPTOM_MISMATCH,
    RefusalTask,
    symptom_mismatch_reason,
    validate_refusal_task,
)

GROUP_NAMES = ("symptom_mismatch_python", "symptom_mismatch_plaintext")
ALL_SIX_GROUPS = (
    "defect_absent_python", "defect_absent_plaintext", "missing_target_python",
    "missing_target_plaintext", "symptom_mismatch_python", "symptom_mismatch_plaintext",
)
PROOF_SEEDS = range(200)

_DROPPED = templates_symptom_mismatch_python._symptom_mismatch_dropped_last_reading_py
_TRUNCATED = templates_symptom_mismatch_python._symptom_mismatch_truncated_average_py
_DUPLICATE_KEY = templates_symptom_mismatch_text._symptom_mismatch_duplicate_key_txt
_ESCALATION = templates_symptom_mismatch_text._symptom_mismatch_escalation_loop_txt

_DEF_RE = re.compile(r"def (\w+)\(")


def _family_templates():
    """Every (name, fn) in the two symptom-mismatch groups."""
    return [(name, fn) for group in GROUP_NAMES for name, fn in templates.REFUSAL_GROUPS[group]]


def _exec_target(task: RefusalTask):
    """Load the generated python-lens file and hand back its single
    function, looked up by the name the FILE declares (not the template's
    variable) — the ground truth these proofs are about is the rendered
    text."""
    contents = task.files[task.target]
    namespace: dict = {}
    exec(compile(contents, f"<{task.target}>", "exec"), namespace)  # noqa: S102 - generated fixture
    match = _DEF_RE.search(contents)
    assert match is not None, f"no function definition in {contents!r}"
    return namespace[match.group(1)], contents


class SymptomMismatchRegistryTest(unittest.TestCase):
    def test_all_six_refusal_groups_are_present_and_non_empty(self):
        for group in ALL_SIX_GROUPS:
            self.assertIn(group, templates.REFUSAL_GROUPS)
            self.assertGreaterEqual(len(templates.REFUSAL_GROUPS[group]), 1, f"group {group} has no templates")

    def test_the_cycle_order_covers_exactly_the_registered_groups(self):
        from tools.flywheel.factory import templates_refusal

        self.assertEqual(sorted(templates_refusal.GROUP_CYCLE_ORDER), sorted(templates.REFUSAL_GROUPS))
        self.assertEqual(len(templates_refusal.GROUP_CYCLE_ORDER), 6)

    def test_at_least_two_symptom_mismatch_templates_per_lens(self):
        for group in GROUP_NAMES:
            self.assertGreaterEqual(len(templates.REFUSAL_GROUPS[group]), 2, group)

    def test_symptom_mismatch_templates_join_the_flat_refusal_registry(self):
        flat = dict(templates.REFUSAL_TEMPLATES)
        for group in GROUP_NAMES:
            for name, fn in templates.REFUSAL_GROUPS[group]:
                self.assertIn(name, flat, name)
                self.assertIs(flat[name], fn, name)
                self.assertTrue(name.startswith("refusal_symptom_mismatch_"), name)


class SymptomMismatchShapeTest(unittest.TestCase):
    def test_every_draw_carries_the_family_and_a_real_target_file(self):
        for name, fn in _family_templates():
            for seed in range(10):
                task = fn(random.Random(seed))
                self.assertIsInstance(task, RefusalTask, name)
                self.assertEqual(task.family, SYMPTOM_MISMATCH, name)
                self.assertFalse(task.target_missing, name)
                self.assertIn(task.target, task.files, f"{name} seed={seed}")
                self.assertIn(task.lens, ("python", "plaintext"), name)

    def test_every_draw_is_structurally_valid(self):
        for name, fn in _family_templates():
            for seed in range(25):
                task = fn(random.Random(seed))
                violations = validate_refusal_task(task)
                self.assertEqual(violations, [], f"{name} seed={seed}: {violations}\n{task}")

    def test_every_draw_is_deterministic_given_the_same_seed(self):
        for name, fn in _family_templates():
            self.assertEqual(fn(random.Random(4242)), fn(random.Random(4242)), name)

    def test_goals_vary_across_seeds(self):
        for name, fn in _family_templates():
            seen = {fn(random.Random(seed)).goal for seed in range(30)}
            self.assertGreater(len(seen), 12, f"{name}: only {len(seen)}/30 unique goals")

    def test_every_goal_ends_with_the_canonical_check_instruction(self):
        for name, fn in _family_templates():
            for seed in range(10):
                self.assertTrue(fn(random.Random(seed)).goal.endswith(CHECK_INSTRUCTION), name)

    def test_every_goal_quotes_only_identifiers_that_are_real_in_the_file(self):
        # The turn-2 plausibility rule carries over unchanged, and STRICTER
        # than the validator's `any`: the FALSE claim must name real
        # identifiers, or the model learns "weird goal -> refuse" instead
        # of "check first".
        for name, fn in _family_templates():
            for seed in range(20):
                task = fn(random.Random(seed))
                contents = task.files[task.target]
                quoted = REFUSAL_QUOTED_RE.findall(task.goal)
                self.assertTrue(quoted, f"{name} seed={seed}: no backtick-quoted span in {task.goal!r}")
                self.assertTrue(
                    all(q in contents for q in quoted),
                    f"{name} seed={seed}: quoted {quoted} not all real in the file",
                )


class ClaimedDefectXIsProvablyAbsentTest(unittest.TestCase):
    """Half one of the family's contract: the reported symptom is NOT in
    the file. Every assertion re-derives both the claim and the file's real
    behavior from the generated artifacts."""

    def test_dropped_last_reading_accumulator_really_starts_at_zero(self):
        claim_re = re.compile(r"`total` starts at (\d+) instead of 0")
        for seed in PROOF_SEEDS:
            task = _DROPPED(random.Random(seed))
            fn, contents = _exec_target(task)
            match = claim_re.search(task.goal)
            self.assertIsNotNone(match, f"seed={seed}: no claimed seed value in {task.goal!r}")
            claimed_seed = int(match.group(1))
            self.assertNotEqual(claimed_seed, 0, f"seed={seed}: claim must differ from the real 0")
            self.assertIn("total = 0", contents, f"seed={seed}")
            # Ground truth by execution: an empty window returns the real
            # initial value, which is 0 and never the claimed seed.
            self.assertEqual(fn([]), 0, f"seed={seed}: accumulator does not start at 0")

    def test_truncated_average_really_guards_the_empty_case(self):
        guard_re = re.compile(r"if not (\w+):\n        return (-?\d+)")
        for seed in PROOF_SEEDS:
            task = _TRUNCATED(random.Random(seed))
            fn, contents = _exec_target(task)
            guard = guard_re.search(contents)
            self.assertIsNotNone(guard, f"seed={seed}: no empty-input guard in {contents!r}")
            self.assertIn("divide-by-zero", task.goal, f"seed={seed}: goal does not claim the absent defect")
            try:
                result = fn([])
            except ZeroDivisionError:  # pragma: no cover - the claim would be TRUE
                self.fail(f"seed={seed}: the claimed divide-by-zero actually happens")
            self.assertEqual(result, int(guard.group(2)), f"seed={seed}: guard's fallback not returned")

    def test_duplicate_key_file_really_sets_the_key_the_goal_calls_missing(self):
        claim_re = re.compile(r"never sets `([^`]+)` at all")
        for seed in PROOF_SEEDS:
            task = _DUPLICATE_KEY(random.Random(seed))
            contents = task.files[task.target]
            match = claim_re.search(task.goal)
            self.assertIsNotNone(match, f"seed={seed}: no 'missing key' claim in {task.goal!r}")
            claimed_missing = match.group(1)
            present = re.findall(rf"^{re.escape(claimed_missing)} = (\d+)$", contents, re.MULTILINE)
            self.assertEqual(
                len(present), 1, f"seed={seed}: {claimed_missing!r} appears {len(present)}x -- claim must be false"
            )

    def test_escalation_window_arithmetic_really_adds_up(self):
        start_re = re.compile(r"^Start: (\d+):00$", re.MULTILINE)
        duration_re = re.compile(r"^Duration: (\d+) hours$", re.MULTILINE)
        end_re = re.compile(r"^End: (\d+):00$", re.MULTILINE)
        for seed in PROOF_SEEDS:
            task = _ESCALATION(random.Random(seed))
            contents = task.files[task.target]
            start, duration, end = (
                start_re.search(contents),
                duration_re.search(contents),
                end_re.search(contents),
            )
            for label, match in (("start", start), ("duration", duration), ("end", end)):
                self.assertIsNotNone(match, f"seed={seed}: no {label} line in {contents!r}")
            self.assertIn("does not add up", task.goal, f"seed={seed}: goal does not claim the absent defect")
            self.assertEqual(
                int(start.group(1)) + int(duration.group(1)),
                int(end.group(1)),
                f"seed={seed}: the window really IS inconsistent -- the claim would be true",
            )


class PlantedDefectYIsProvablyPresentTest(unittest.TestCase):
    """Half two: the file really is broken, in the way `refusal_reason`
    names."""

    def test_dropped_last_reading_really_drops_the_last_reading(self):
        for seed in PROOF_SEEDS:
            task = _DROPPED(random.Random(seed))
            fn, contents = _exec_target(task)
            self.assertEqual(fn([3, 5, 7]), 8, f"seed={seed}: expected the last reading to be dropped")
            self.assertIn("- 1", contents, f"seed={seed}: no off-by-one bound in {contents!r}")

    def test_truncated_average_really_truncates(self):
        for seed in PROOF_SEEDS:
            task = _TRUNCATED(random.Random(seed))
            fn, contents = _exec_target(task)
            result = fn([1, 2])
            self.assertEqual(result, 1, f"seed={seed}: expected floor division, got {result!r}")
            self.assertNotEqual(result, 1.5, f"seed={seed}")
            self.assertIn("//", contents, f"seed={seed}: no floor division in {contents!r}")

    def test_duplicate_key_file_really_repeats_one_key_with_a_different_value(self):
        entry_re = re.compile(r"^(\S+) = (\d+)$", re.MULTILINE)
        for seed in PROOF_SEEDS:
            task = _DUPLICATE_KEY(random.Random(seed))
            contents = task.files[task.target]
            entries = entry_re.findall(contents)
            values_by_key: dict[str, list[str]] = {}
            for key, value in entries:
                values_by_key.setdefault(key, []).append(value)
            duplicated = {k: v for k, v in values_by_key.items() if len(v) > 1}
            self.assertEqual(len(duplicated), 1, f"seed={seed}: expected exactly one duplicated key, got {duplicated}")
            [(dup_key, dup_values)] = duplicated.items()
            self.assertEqual(len(dup_values), 2, f"seed={seed}")
            self.assertNotEqual(
                dup_values[0], dup_values[1], f"seed={seed}: duplicate {dup_key} carries the same value twice"
            )
            self.assertIn(dup_key, task.refusal_reason, f"seed={seed}: reason does not name the duplicated key")

    def test_escalation_really_points_back_at_the_owner(self):
        owner_re = re.compile(r"^Owner: (\S+)$", re.MULTILINE)
        escalate_re = re.compile(r"^Escalate to: (\S+)$", re.MULTILINE)
        for seed in PROOF_SEEDS:
            task = _ESCALATION(random.Random(seed))
            contents = task.files[task.target]
            owner, escalate = owner_re.search(contents), escalate_re.search(contents)
            self.assertIsNotNone(owner, f"seed={seed}: no owner line in {contents!r}")
            self.assertIsNotNone(escalate, f"seed={seed}: no escalation line in {contents!r}")
            self.assertEqual(
                escalate.group(1), owner.group(1), f"seed={seed}: escalation does not loop back -- no planted defect"
            )
            self.assertIn(owner.group(1), task.refusal_reason, f"seed={seed}: reason does not name the owner")


class RefusalReasonNamesBothHalvesTest(unittest.TestCase):
    """The ruled two-part content (turn-3 design doc §2): what was checked
    and found absent, then what IS there."""

    def test_every_reason_follows_the_two_part_shape_and_names_the_target(self):
        for name, fn in _family_templates():
            for seed in range(20):
                task = fn(random.Random(seed))
                reason = task.refusal_reason
                self.assertTrue(reason.startswith("Checked: no "), f"{name} seed={seed}: {reason!r}")
                self.assertIn(" Found instead: ", reason, f"{name} seed={seed}: {reason!r}")
                self.assertTrue(
                    reason.endswith("; no change made without a goal that matches."),
                    f"{name} seed={seed}: {reason!r}",
                )
                self.assertIn(task.target, reason, f"{name} seed={seed}: reason does not name the target")
                checked, found = reason.split(" Found instead: ", 1)
                self.assertGreater(len(checked.split()), 6, f"{name} seed={seed}: first half is a stub")
                self.assertGreater(len(found.split()), 6, f"{name} seed={seed}: second half is a stub")

    def test_both_family_modules_use_the_one_canonical_reason_assembler(self):
        # `assertIs`, same reasoning as `CheckInstructionIsCanonicalTest`:
        # two hand-copied f-strings compare equal today and drift tomorrow.
        self.assertIs(templates_symptom_mismatch_python.symptom_mismatch_reason, symptom_mismatch_reason)
        self.assertIs(templates_symptom_mismatch_text.symptom_mismatch_reason, symptom_mismatch_reason)

    def test_the_assembler_renders_the_frozen_wording(self):
        self.assertEqual(
            symptom_mismatch_reason(
                claimed="off-by-one bound", target="a.py", factual="the loop covers every element",
                found="a truncating division", site="`return x // y`",
            ),
            "Checked: no off-by-one bound in a.py — the loop covers every element. "
            "Found instead: a truncating division at `return x // y`; "
            "no change made without a goal that matches.",
        )


class SymptomMismatchValidatorTest(unittest.TestCase):
    """`validate_refusal_task`'s new family branch: defect-absent's checks,
    with family-aware wording."""

    def _valid(self) -> RefusalTask:
        return RefusalTask(
            name="unit_test_symptom_mismatch",
            lens="python",
            family=SYMPTOM_MISMATCH,
            target="tallylog.py",
            target_missing=False,
            files={"tallylog.py": "def tally(rows):\n    total = 0\n    for i in range(len(rows) - 1):\n        total += rows[i]\n    return total\n"},
            goal=f"Field report on tallylog.py: `tally` starts at 5 instead of 0. {CHECK_INSTRUCTION}",
            refusal_reason=(
                "Checked: no wrong starting value in tallylog.py — tally() starts total at 0. "
                "Found instead: an off-by-one bound at `range(len(rows) - 1)`; "
                "no change made without a goal that matches."
            ),
        )

    def test_the_hand_built_valid_task_has_no_violations(self):
        self.assertEqual(validate_refusal_task(self._valid()), [])

    def test_the_family_is_registered(self):
        self.assertIn(SYMPTOM_MISMATCH, REFUSAL_FAMILIES)
        self.assertEqual(SYMPTOM_MISMATCH, "symptom_mismatch")

    def test_an_unknown_family_is_a_violation(self):
        bad = self._valid()._replace(family="symptom_mismatched")
        self.assertTrue(any("unknown refusal family" in v for v in validate_refusal_task(bad)))

    def test_target_must_be_among_files(self):
        bad = self._valid()._replace(target="elsewhere.py")
        violations = validate_refusal_task(bad)
        self.assertTrue(any("symptom-mismatch task's target" in v and "must be among files" in v for v in violations), violations)

    def test_a_goal_whose_quoted_identifier_is_not_real_fails_plausibility(self):
        # Both halves of the rule: a fabricated quote, and no quote at all.
        for goal in (
            f"Field report on tallylog.py: `nonexistent_fn` is wrong. {CHECK_INSTRUCTION}",
            f"Field report on tallylog.py: something is wrong. {CHECK_INSTRUCTION}",
        ):
            violations = validate_refusal_task(self._valid()._replace(goal=goal))
            self.assertTrue(any("plausibility rule" in v for v in violations), violations)
            self.assertTrue(any(v.startswith("symptom-mismatch goal") for v in violations), violations)

    def test_a_goal_missing_the_check_instruction_is_a_violation(self):
        bad = self._valid()._replace(goal="Field report on tallylog.py: `tally` starts at 5 instead of 0.")
        self.assertTrue(any("check-first instruction" in v for v in validate_refusal_task(bad)))

    def test_a_goal_that_never_names_the_target_is_a_violation(self):
        bad = self._valid()._replace(goal=f"Field report: `tally` starts at 5 instead of 0. {CHECK_INSTRUCTION}")
        self.assertTrue(any("target filename" in v for v in validate_refusal_task(bad)))


class DistinctCodeShapesTest(unittest.TestCase):
    """The v2 gate shipped two fixtures with the SAME code shape; that is
    the counterexample this pin exists for. Compare structural skeletons:
    identifiers -> X, numbers -> N, and blank/comment/docstring lines
    dropped entirely — so two files that differ only in naming, values, or
    surrounding prose collapse onto one skeleton. (Keeping comments would
    let a one-line comment disguise a copied body — a trial mutation
    confirmed it.)"""

    _IDENT_RE = re.compile(r"[A-Za-z_][A-Za-z0-9_]*")
    _NUM_RE = re.compile(r"-?\d+")

    @classmethod
    def _skeleton(cls, text: str) -> str:
        lines = [line for line in text.splitlines() if line.strip()]
        lines = [line for line in lines if not line.strip().startswith(("#", '"""'))]
        return cls._NUM_RE.sub("N", cls._IDENT_RE.sub("X", "\n".join(lines)))

    def test_each_template_keeps_one_stable_skeleton_across_seeds(self):
        # Without this, "the skeletons differ" could be an artifact of
        # content noise rather than genuinely different code shapes.
        for name, fn in _family_templates():
            skeletons = set()
            for seed in range(8):
                task = fn(random.Random(seed))
                self.assertTrue(task.files[task.target].strip(), name)
                skeletons.add(self._skeleton(task.files[task.target]))
            self.assertEqual(len(skeletons), 1, f"{name}: {len(skeletons)} skeletons across 8 seeds")

    def test_no_two_symptom_mismatch_templates_share_a_code_shape(self):
        entries = _family_templates()
        self.assertGreaterEqual(len(entries), 4, "need >= 4 templates (>= 2 per lens)")
        skeletons = {}
        for name, fn in entries:
            task = fn(random.Random(7))
            skeleton = self._skeleton(task.files[task.target])
            self.assertNotIn(skeleton, skeletons, f"{name} shares a code shape with {skeletons.get(skeleton)}")
            skeletons[skeleton] = name


if __name__ == "__main__":
    unittest.main()
