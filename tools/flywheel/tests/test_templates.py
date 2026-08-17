"""Tests for tools.flywheel.factory.templates — brief rules 1 and 2.

Rule 1: >= 8 python template families, >= 5 plaintext template families,
each a pure function (rng) -> Task drawing from word lists that are
disjoint (at build time) from the gate set's own vocabulary.

Rule 2: structural validity mirrors codec-tasks-v1's own validator:
search appears exactly once in the target's contents; target is a key in
files; goal contains the target filename and ends with the exact phrase
"Patch the file, then emit done."; target is 5-60 lines; search != replace.
"""

import random
import unittest

from tools.flywheel.factory import templates
from tools.flywheel.factory.contamination import GATE_VOCABULARY


class TemplateFamilyCountTest(unittest.TestCase):
    def test_at_least_eight_python_template_families(self):
        self.assertGreaterEqual(len(templates.PYTHON_TEMPLATES), 8)

    def test_at_least_five_plaintext_template_families(self):
        self.assertGreaterEqual(len(templates.TEXT_TEMPLATES), 5)

    def test_python_template_names_are_unique(self):
        names = [name for name, _fn in templates.PYTHON_TEMPLATES]
        self.assertEqual(len(names), len(set(names)))

    def test_text_template_names_are_unique(self):
        names = [name for name, _fn in templates.TEXT_TEMPLATES]
        self.assertEqual(len(names), len(set(names)))


class TemplateFunctionsAreDeterministicPureFunctionsTest(unittest.TestCase):
    def test_every_python_family_is_a_pure_function_of_its_rng(self):
        for name, fn in templates.PYTHON_TEMPLATES:
            task_a = fn(random.Random(12345))
            task_b = fn(random.Random(12345))
            self.assertEqual(task_a, task_b, f"{name} is not deterministic given the same rng seed")

    def test_every_text_family_is_a_pure_function_of_its_rng(self):
        for name, fn in templates.TEXT_TEMPLATES:
            task_a = fn(random.Random(54321))
            task_b = fn(random.Random(54321))
            self.assertEqual(task_a, task_b, f"{name} is not deterministic given the same rng seed")

    def test_families_produce_varied_output_across_seeds(self):
        # A template drawing from a rich enough value space should not
        # collapse to the same task for many different seeds.
        for name, fn in templates.PYTHON_TEMPLATES + templates.TEXT_TEMPLATES:
            seen = {fn(random.Random(seed)).goal for seed in range(30)}
            self.assertGreater(
                len(seen), 15, f"{name}'s value space looks too narrow ({len(seen)}/30 unique goals)"
            )


class VocabularyDisjointFromGateSetTest(unittest.TestCase):
    """Rule 1: word lists must not contain any target filename, function
    name, or domain noun used in the gate set."""

    def test_all_template_words_are_disjoint_from_gate_vocabulary(self):
        overlap = templates.ALL_TEMPLATE_WORDS & GATE_VOCABULARY
        self.assertEqual(
            overlap, set(), f"template word lists reuse gate-set vocabulary: {sorted(overlap)}"
        )

    def test_generated_tasks_never_surface_gate_target_filenames(self):
        for name, fn in templates.PYTHON_TEMPLATES + templates.TEXT_TEMPLATES:
            for seed in range(10):
                task = fn(random.Random(seed))
                self.assertNotIn(
                    task.target.lower(),
                    GATE_VOCABULARY,
                    f"{name} produced a gate-set target filename: {task.target!r}",
                )


class OffByOneFamilySearchStringDiversityTest(unittest.TestCase):
    """Task 6a follow-on fix: `py_off_by_one_index` and
    `py_off_by_one_range_bound` used to emit a hardcoded, non-randomized
    `search` string on EVERY draw -- a defect line so fixed it collided
    with probability 1.0 against a frozen gate fixture generated from the
    same family (`codec-tasks-v2-mixed`'s own v2-patch-py-02/03), which
    no amount of gate-aware rejection-sampling retries could ever clear
    (see `gate_sampling.RejectionSampler`'s termination guard, which
    correctly and quickly refused to spin on it). This is the mechanical
    diversity property that was missing: >= 90/100 distinct search
    strings AND >= 90/100 distinct goals per family over a fixed run of
    seeds, mirroring the entropy every other python family already has
    (`test_families_produce_varied_output_across_seeds`, above, uses a
    looser 15/30 bar across ALL families; this pin targets exactly the
    two families and the exact property -- search-string entropy -- that
    was actually missing)."""

    FAMILIES_UNDER_TEST = ("py_off_by_one_index", "py_off_by_one_range_bound")
    N_SEEDS = 100
    MIN_DISTINCT = 90

    def test_both_families_under_test_are_actually_registered(self):
        # A typo in FAMILIES_UNDER_TEST would make the diversity test
        # below silently skip everything and pass vacuously -- guard
        # against that mechanically rather than trusting the string match.
        registered = {name for name, _fn in templates.PYTHON_TEMPLATES}
        for name in self.FAMILIES_UNDER_TEST:
            self.assertIn(name, registered)

    def test_off_by_one_families_produce_diverse_search_strings_and_goals(self):
        checked = 0
        for name, fn in templates.PYTHON_TEMPLATES:
            if name not in self.FAMILIES_UNDER_TEST:
                continue
            checked += 1
            searches = {fn(random.Random(seed)).search for seed in range(self.N_SEEDS)}
            goals = {fn(random.Random(seed)).goal for seed in range(self.N_SEEDS)}
            self.assertGreaterEqual(
                len(searches),
                self.MIN_DISTINCT,
                f"{name}: only {len(searches)}/{self.N_SEEDS} distinct search strings",
            )
            self.assertGreaterEqual(
                len(goals),
                self.MIN_DISTINCT,
                f"{name}: only {len(goals)}/{self.N_SEEDS} distinct goals",
            )
        self.assertEqual(checked, len(self.FAMILIES_UNDER_TEST))


class StructuralValidityTest(unittest.TestCase):
    """Rule 2, mirroring the gate set's own validator."""

    def test_every_generated_python_task_is_structurally_valid(self):
        for name, fn in templates.PYTHON_TEMPLATES:
            for seed in range(25):
                task = fn(random.Random(seed))
                violations = templates.validate_task(task)
                self.assertEqual(violations, [], f"{name} seed={seed}: {violations}\n{task}")

    def test_every_generated_text_task_is_structurally_valid(self):
        for name, fn in templates.TEXT_TEMPLATES:
            for seed in range(25):
                task = fn(random.Random(seed))
                violations = templates.validate_task(task)
                self.assertEqual(violations, [], f"{name} seed={seed}: {violations}\n{task}")

    def _make_valid_task(self):
        contents = "def add(a, b):\n    return a + b\n\n\ndef sub(a, b):\n    return a - b\n"
        return templates.Task(
            name="unit_test_family",
            lens="python",
            target="mathy.py",
            files={"mathy.py": contents},
            goal="mathy.py's add() is broken. Patch the file, then emit done.",
            search="    return a + b",
            replace="    return a + b  # ok",
            summary="Fixed add().",
        )

    def test_valid_task_has_no_violations(self):
        self.assertEqual(templates.validate_task(self._make_valid_task()), [])

    def test_search_must_appear_exactly_once(self):
        task = self._make_valid_task()
        bad = task._replace(search="    return a - b\nsomething", replace="x")
        violations = templates.validate_task(bad)
        self.assertTrue(any("exactly once" in v for v in violations))

    def test_search_appearing_zero_times_is_a_violation(self):
        task = self._make_valid_task()
        bad = task._replace(search="not present anywhere")
        violations = templates.validate_task(bad)
        self.assertTrue(any("exactly once" in v for v in violations))

    def test_target_must_be_among_files(self):
        task = self._make_valid_task()
        bad = task._replace(target="other.py")
        violations = templates.validate_task(bad)
        self.assertTrue(any("target" in v and "files" in v for v in violations))

    def test_goal_must_contain_target_filename(self):
        task = self._make_valid_task()
        bad = task._replace(goal="Something is broken. Patch the file, then emit done.")
        violations = templates.validate_task(bad)
        self.assertTrue(any("target filename" in v for v in violations))

    def test_goal_must_end_with_the_exact_instruction(self):
        task = self._make_valid_task()
        bad = task._replace(goal="mathy.py's add() is broken. Please fix it.")
        violations = templates.validate_task(bad)
        self.assertTrue(any("Patch the file, then emit done." in v for v in violations))

    def test_target_below_five_lines_is_a_violation(self):
        task = self._make_valid_task()
        bad = task._replace(files={"mathy.py": "x = 1\ny = 2\n"})
        violations = templates.validate_task(bad)
        self.assertTrue(any("5" in v and "60" in v for v in violations))

    def test_target_above_sixty_lines_is_a_violation(self):
        task = self._make_valid_task()
        long_contents = "\n".join(f"line_{i} = {i}" for i in range(61)) + "\n"
        bad = task._replace(files={"mathy.py": long_contents}, search="line_0 = 0")
        violations = templates.validate_task(bad)
        self.assertTrue(any("5" in v and "60" in v for v in violations))

    def test_search_equal_replace_is_a_violation(self):
        task = self._make_valid_task()
        bad = task._replace(replace=task.search)
        violations = templates.validate_task(bad)
        self.assertTrue(any("search" in v and "replace" in v for v in violations))


if __name__ == "__main__":
    unittest.main()
