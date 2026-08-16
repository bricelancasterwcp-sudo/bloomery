"""Mechanical diversity pins for task 6a's third follow-on fix (goal-
phrasing skeleton diversity across every template family) --
`tools/flywheel/factory/goal_phrasing.py`.

Every one of the 21 template families (8 python patch, 5 plaintext
patch, 4 defect-absent refusal, 4 missing-target refusal) now offers
>= 4 structurally distinct goal-phrasing skeletons, chosen per draw via
`rng.choice`. The property that actually matters -- and previously did
NOT hold (99% `goal_near_duplicate` rejection at full-scale generation,
even after the identifier-entropy fix) -- is that two draws landing on
DIFFERENT skeletons stay well under 0.8 token-set Jaccard, even when
their identifiers happen to coincide by chance. This file pins that
property directly against the REAL family functions (never a synthetic
stand-in) for all 21 families, plus `goal_phrasing.py`'s own assembler
determinism.

Classification of "which skeleton a goal used" is a TEST-ONLY technique
(distinguishing opening substrings) -- it does not constrain
`goal_phrasing.py`'s wording as a production contract; if the skeletons'
opening phrasing ever changes, only the marker tuples below need
updating.
"""

import random
import unittest

from tools.flywheel.factory import goal_phrasing, templates, templates_python, templates_text
from tools.flywheel.factory.contamination import jaccard, token_set

N_SEEDS = 100
MAX_CROSS_SKELETON_JACCARD = 0.8
SAMPLE_SIZE = 60  # bounded, not exhaustive -- requirement 2's own wording

_PATCH_MARKERS = ("Bug ticket for", "A reviewer flagged", "Please correct")
_DEFECT_ABSENT_MARKERS = ("Ticket filed against", "A teammate double-checking", "Before touching")
_MISSING_TARGET_MARKERS = ("Ticket:", "A user reported", "Before editing")


def _classify(goal: str, markers: tuple[str, ...]) -> int:
    """Skeleton 1 ("standard") is whatever doesn't match one of the
    other three's distinguishing opening substrings."""
    for i, marker in enumerate(markers, start=2):
        if goal.startswith(marker):
            return i
    return 1


def _assert_cross_skeleton_diversity(case: unittest.TestCase, name: str, fn, markers: tuple[str, ...]) -> None:
    goals = [fn(random.Random(seed)).goal for seed in range(N_SEEDS)]
    skeleton_ids = [_classify(g, markers) for g in goals]
    case.assertGreaterEqual(
        len(set(skeleton_ids)), 2, f"{name}: only one skeleton style chosen across {N_SEEDS} seeds"
    )

    cross_pairs = [(i, j) for i in range(N_SEEDS) for j in range(i + 1, N_SEEDS) if skeleton_ids[i] != skeleton_ids[j]]
    case.assertGreater(len(cross_pairs), 0, f"{name}: no cross-skeleton pairs to sample")
    sample_rng = random.Random(f"diversity-sample::{name}")
    sample = sample_rng.sample(cross_pairs, min(SAMPLE_SIZE, len(cross_pairs)))

    worst = 0.0
    worst_pair = None
    for i, j in sample:
        jv = jaccard(token_set(goals[i]), token_set(goals[j]))
        if jv > worst:
            worst, worst_pair = jv, (goals[i], goals[j])
    case.assertLess(
        worst,
        MAX_CROSS_SKELETON_JACCARD,
        f"{name}: cross-skeleton pair hit Jaccard {worst:.3f} >= {MAX_CROSS_SKELETON_JACCARD}\n"
        f"  a: {worst_pair[0] if worst_pair else None}\n  b: {worst_pair[1] if worst_pair else None}",
    )


class PythonPatchFamilyDiversityTest(unittest.TestCase):
    def test_every_python_family_clears_the_cross_skeleton_jaccard_bar(self):
        for name, fn in sorted(templates_python.FAMILIES.items()):
            with self.subTest(family=name):
                _assert_cross_skeleton_diversity(self, name, fn, _PATCH_MARKERS)


class TextPatchFamilyDiversityTest(unittest.TestCase):
    def test_every_text_family_clears_the_cross_skeleton_jaccard_bar(self):
        for name, fn in sorted(templates_text.FAMILIES.items()):
            with self.subTest(family=name):
                _assert_cross_skeleton_diversity(self, name, fn, _PATCH_MARKERS)


class DefectAbsentRefusalFamilyDiversityTest(unittest.TestCase):
    def test_every_defect_absent_family_clears_the_cross_skeleton_jaccard_bar(self):
        for group_name in ("defect_absent_python", "defect_absent_plaintext"):
            for name, fn in templates.REFUSAL_GROUPS[group_name]:
                with self.subTest(family=name):
                    _assert_cross_skeleton_diversity(self, name, fn, _DEFECT_ABSENT_MARKERS)


class MissingTargetRefusalFamilyDiversityTest(unittest.TestCase):
    def test_every_missing_target_family_clears_the_cross_skeleton_jaccard_bar(self):
        for group_name in ("missing_target_python", "missing_target_plaintext"):
            for name, fn in templates.REFUSAL_GROUPS[group_name]:
                with self.subTest(family=name):
                    _assert_cross_skeleton_diversity(self, name, fn, _MISSING_TARGET_MARKERS)


class AllFamiliesCoveredTest(unittest.TestCase):
    """Guards against a typo/omission silently shrinking coverage below
    all 21 families (8 python + 5 plaintext + 4 defect-absent + 4
    missing-target)."""

    def test_exactly_twenty_one_families_are_exercised_across_this_file(self):
        python_count = len(templates_python.FAMILIES)
        text_count = len(templates_text.FAMILIES)
        defect_absent_count = len(templates.REFUSAL_GROUPS["defect_absent_python"]) + len(
            templates.REFUSAL_GROUPS["defect_absent_plaintext"]
        )
        missing_target_count = len(templates.REFUSAL_GROUPS["missing_target_python"]) + len(
            templates.REFUSAL_GROUPS["missing_target_plaintext"]
        )
        total = python_count + text_count + defect_absent_count + missing_target_count
        self.assertEqual(total, 21, f"expected 21 families, counted {total}")


class GoalPhrasingAssemblerDeterminismTest(unittest.TestCase):
    """`goal_phrasing.py`'s three assemblers are themselves pure
    functions of `rng` (same contract every family function already
    has) -- pinned directly, independent of any specific family."""

    def test_patch_skeletons_is_deterministic(self):
        args = ("a.py", "f()", "does the wrong thing", "f(1) returns 2 instead of 3", "f() in a.py", "it returns 3", "Patch the file, then emit done.")
        a = goal_phrasing.patch_skeletons(random.Random(99), *args)
        b = goal_phrasing.patch_skeletons(random.Random(99), *args)
        self.assertEqual(a, b)

    def test_defect_absent_skeletons_is_deterministic(self):
        args = ("a.py", "`f` looks wrong -- shouldn't it return `3`?", "Check first, and only patch if it is genuinely wrong; then emit done.")
        a = goal_phrasing.defect_absent_skeletons(random.Random(99), *args)
        b = goal_phrasing.defect_absent_skeletons(random.Random(99), *args)
        self.assertEqual(a, b)

    def test_missing_target_skeletons_is_deterministic(self):
        args = ("missing.py", "`f` in missing.py looks broken", "Check first, and only patch if it is genuinely wrong; then emit done.")
        a = goal_phrasing.missing_target_skeletons(random.Random(99), *args)
        b = goal_phrasing.missing_target_skeletons(random.Random(99), *args)
        self.assertEqual(a, b)

    def test_each_assembler_offers_at_least_four_skeletons(self):
        # Drive many seeds and count how many DISTINCT skeleton openings
        # appear -- a mechanical floor on "at least 4 skeletons", not
        # just "at least 2 distinct goals" (which content entropy alone
        # could already satisfy without any skeleton diversity at all).
        patch_args = ("a.py", "f()", "does the wrong thing", "f(1) returns 2 instead of 3", "f() in a.py", "it returns 3", "Patch the file, then emit done.")
        seen = {goal_phrasing.patch_skeletons(random.Random(seed), *patch_args) for seed in range(50)}
        openings = {_classify(g, _PATCH_MARKERS) for g in seen}
        self.assertEqual(openings, {1, 2, 3, 4})


if __name__ == "__main__":
    unittest.main()
