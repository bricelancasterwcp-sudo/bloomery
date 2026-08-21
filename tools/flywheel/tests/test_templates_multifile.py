"""Tests for turn 3's two new repair-trajectory template registries
(task-7 brief; turn-3 design doc §2's "find/run enter through repair
ideals"):

- `templates.FIND_TEMPLATES` — the find-shaped, multi-file families in
  `templates_multifile_python.py`/`templates_multifile_text.py`: a target
  plus 2-4 plausible siblings, a goal that names the SYMPTOM and never the
  filename, and a `find_pattern` that occurs in the target and in NO
  sibling.
- `templates.RUN_TEMPLATES` — the run-verified wrappers over the existing
  python families (`templates_run_verified.RUN_FAMILIES`): same defect
  bodies, same goals, plus a PLANTED `unittest` sibling, the
  `python3 -m unittest test_<stem>.py` argv that runs it, and the
  `python3 -m unittest` grant that permits it (turn-4 spec §3).

Split out of `test_templates.py` (which stays the home of the registry
counts and the word-list disjointness proof) to keep both files under the
400-line house cap — the same reasoning turn 1 used for
`templates_python.py`/`templates_text.py`. The validator's own per-shape
mutation pins are a third file, `test_task_validation.py`; this one proves
the shipped families SATISFY those rules, seed after seed.
"""

import random
import re
import unittest

from tools.flywheel.factory import planted_test
from tools.flywheel.factory import task as task_mod
from tools.flywheel.factory import templates, templates_python, templates_run_verified
from tools.flywheel.factory.contamination import GATE_VOCABULARY
from tools.flywheel.factory.wordlists import MULTIFILE_SIBLING_VERBS, MULTIFILE_TARGET_VERBS

# The regex-literal alphabet is the VALIDATOR's rule
# (`task.FIND_PATTERN_LITERAL_RE`, enforced in `_find_shape_violations` and
# mutation-pinned in `test_task_validation.py`), reused here rather than
# re-declared: a second copy could drift from the rule the factory
# actually applies, and then this per-family test would be proving
# something no longer enforced.
_REGEX_LITERAL_RE = task_mod.FIND_PATTERN_LITERAL_RE


def _siblings(task):
    return {path: contents for path, contents in task.files.items() if path != task.target}


class FindTemplateRegistryTest(unittest.TestCase):
    def test_registry_covers_both_lenses(self):
        lenses = {fn(random.Random(3)).lens for _name, fn in templates.FIND_TEMPLATES}
        self.assertEqual(lenses, {"python", "plaintext"})

    def test_at_least_three_python_and_two_plaintext_find_families(self):
        by_lens = {"python": 0, "plaintext": 0}
        for _name, fn in templates.FIND_TEMPLATES:
            by_lens[fn(random.Random(3)).lens] += 1
        self.assertGreaterEqual(by_lens["python"], 3)
        self.assertGreaterEqual(by_lens["plaintext"], 2)

    def test_find_template_names_are_unique(self):
        names = [name for name, _fn in templates.FIND_TEMPLATES]
        self.assertEqual(len(names), len(set(names)))

    def test_find_template_names_do_not_collide_with_the_plain_registries(self):
        plain = {name for name, _fn in templates.PYTHON_TEMPLATES + templates.TEXT_TEMPLATES}
        find = {name for name, _fn in templates.FIND_TEMPLATES}
        self.assertEqual(plain & find, set())

    def test_every_find_family_is_a_pure_function_of_its_rng(self):
        for name, fn in templates.FIND_TEMPLATES:
            self.assertEqual(
                fn(random.Random(9876)), fn(random.Random(9876)), f"{name} is not deterministic"
            )

    def test_find_families_produce_varied_output_across_seeds(self):
        for name, fn in templates.FIND_TEMPLATES:
            goals = {fn(random.Random(seed)).goal for seed in range(30)}
            self.assertGreater(len(goals), 15, f"{name}: only {len(goals)}/30 unique goals")


class FindShapedFamilyPropertiesTest(unittest.TestCase):
    """The four properties that make a find-shaped task trainable at all,
    proven per family across a fixed run of seeds rather than once."""

    N_SEEDS = 100

    def test_every_find_task_carries_the_find_trajectory_and_no_run_fields(self):
        for name, fn in templates.FIND_TEMPLATES:
            task = fn(random.Random(1))
            self.assertEqual(task.trajectory, task_mod.FIND_TRAJECTORY, name)
            self.assertEqual(task.run_argv, (), name)
            self.assertEqual(task.commands, (), name)

    def test_every_find_family_plants_at_least_two_siblings(self):
        for name, fn in templates.FIND_TEMPLATES:
            for seed in range(self.N_SEEDS):
                task = fn(random.Random(seed))
                siblings = _siblings(task)
                self.assertGreaterEqual(
                    len(siblings), 2, f"{name} seed={seed}: only {len(siblings)} sibling(s)"
                )
                self.assertLessEqual(
                    len(siblings), 4, f"{name} seed={seed}: {len(siblings)} siblings (spec caps at 4)"
                )
                self.assertTrue(all(contents.strip() for contents in siblings.values()), name)

    def test_find_pattern_occurs_in_the_target_and_in_no_sibling_on_every_draw(self):
        for name, fn in templates.FIND_TEMPLATES:
            for seed in range(self.N_SEEDS):
                task = fn(random.Random(seed))
                self.assertIn(task.find_pattern, task.files[task.target], f"{name} seed={seed}")
                for path, contents in _siblings(task).items():
                    self.assertNotIn(
                        task.find_pattern,
                        contents,
                        f"{name} seed={seed}: pattern {task.find_pattern!r} also in sibling {path}",
                    )

    def test_find_pattern_is_a_single_line_regex_literal(self):
        # `exec_find` matches line by line, so a pattern spanning a newline
        # could never match anything the real tool walks.
        for name, fn in templates.FIND_TEMPLATES:
            for seed in range(20):
                pattern = fn(random.Random(seed)).find_pattern
                self.assertRegex(pattern, _REGEX_LITERAL_RE, f"{name} seed={seed}: {pattern!r}")

    def test_the_goal_never_names_the_target_or_any_sibling_filename(self):
        for name, fn in templates.FIND_TEMPLATES:
            for seed in range(self.N_SEEDS):
                task = fn(random.Random(seed))
                self.assertNotIn(task.target, task.goal, f"{name} seed={seed}")
                for path in _siblings(task):
                    self.assertNotIn(path, task.goal, f"{name} seed={seed}: goal names sibling {path}")

    def test_every_find_task_is_structurally_valid(self):
        for name, fn in templates.FIND_TEMPLATES:
            for seed in range(25):
                task = fn(random.Random(seed))
                violations = templates.validate_task(task)
                self.assertEqual(violations, [], f"{name} seed={seed}: {violations}\n{task}")

    def test_the_target_and_sibling_verb_pools_are_disjoint(self):
        """The uniqueness argument's mechanical core, pinned directly
        rather than only through its consequence: every target marker
        starts with a target verb and every sibling marker with a sibling
        verb, so overlapping pools would make sibling collisions possible
        for SOME draw long before any single test seed happened to hit
        one."""
        self.assertEqual(set(MULTIFILE_TARGET_VERBS) & set(MULTIFILE_SIBLING_VERBS), set())

    def test_no_sibling_file_carries_a_target_marker_verb_at_all(self):
        # Stronger than "the pattern is absent": this holds even if a
        # family later changes what its pattern is built from, as long as
        # it keeps building it out of a target verb.
        for name, fn in templates.FIND_TEMPLATES:
            for seed in range(20):
                task = fn(random.Random(seed))
                for path, contents in _siblings(task).items():
                    for verb in MULTIFILE_TARGET_VERBS:
                        self.assertNotIn(
                            f"{verb}_", contents, f"{name} seed={seed}: {path} carries {verb!r}"
                        )

    def test_the_defect_line_is_not_byte_identical_across_draws(self):
        """Task 6a's follow-on lesson, applied up front: a family whose
        `search` string never varies collides with a frozen gate fixture
        generated from that same family with probability 1, and gate-aware
        rejection sampling can never clear a collision it cannot redraw
        away from. Measured 93-100/100 distinct at the time of writing;
        the bar is set below that so ordinary variance is not a failure,
        but a family that regressed to a hardcoded defect line (1/100)
        would be caught immediately."""
        for name, fn in templates.FIND_TEMPLATES:
            searches = {fn(random.Random(seed)).search for seed in range(self.N_SEEDS)}
            self.assertGreaterEqual(
                len(searches), 85, f"{name}: only {len(searches)}/{self.N_SEEDS} distinct search strings"
            )

    def test_no_generated_filename_reuses_gate_vocabulary(self):
        for name, fn in templates.FIND_TEMPLATES:
            for seed in range(20):
                task = fn(random.Random(seed))
                for path in task.files:
                    self.assertNotIn(path.lower(), GATE_VOCABULARY, f"{name} seed={seed}: {path}")


class DistinctCodeShapesTest(unittest.TestCase):
    """The v3 diversity rule (design doc §3), applied to the find-shaped
    families: no two share a code shape, so a model cannot pattern-match
    the shape instead of reading the file. Same skeleton comparison
    `test_templates_symptom_mismatch.py` uses — identifiers -> X, numbers
    -> N, blank/comment lines dropped."""

    _IDENT_RE = re.compile(r"[A-Za-z_][A-Za-z0-9_]*")
    _NUM_RE = re.compile(r"-?\d+")

    @classmethod
    def _skeleton(cls, text: str) -> str:
        lines = [line for line in text.splitlines() if line.strip()]
        lines = [line for line in lines if not line.strip().startswith(("#", '"""'))]
        return cls._NUM_RE.sub("N", cls._IDENT_RE.sub("X", "\n".join(lines)))

    def test_each_find_template_keeps_one_stable_skeleton_across_seeds(self):
        for name, fn in templates.FIND_TEMPLATES:
            skeletons = {self._skeleton(fn(random.Random(seed)).files[fn(random.Random(seed)).target]) for seed in range(8)}
            self.assertEqual(len(skeletons), 1, f"{name}: {len(skeletons)} skeletons across 8 seeds")

    def test_no_two_find_templates_share_a_code_shape(self):
        seen = {}
        for name, fn in templates.FIND_TEMPLATES:
            task = fn(random.Random(7))
            skeleton = self._skeleton(task.files[task.target])
            self.assertNotIn(skeleton, seen, f"{name} shares a code shape with {seen.get(skeleton)}")
            seen[skeleton] = name


class RunVerifiedWrapperTest(unittest.TestCase):
    """Run-verified families are WRAPPERS over the plain python families,
    never copies: the same rng seed must reproduce the base family's exact
    body, goal, search and replace, with only the trajectory fields added.
    A copy-paste variant would drift the moment a base family is edited,
    and this is the test that would catch it."""

    def test_there_is_one_run_wrapper_per_python_family(self):
        self.assertEqual(len(templates.RUN_TEMPLATES), len(templates.PYTHON_TEMPLATES))
        names = [name for name, _fn in templates.RUN_TEMPLATES]
        self.assertEqual(len(names), len(set(names)))

    def test_each_wrapper_reproduces_its_base_family_body_verbatim(self):
        base_by_name = dict(templates.PYTHON_TEMPLATES)
        wrapped = 0
        for name, fn in templates.RUN_TEMPLATES:
            base_name = name.removesuffix(templates.RUN_FAMILY_SUFFIX)
            self.assertIn(base_name, base_by_name, f"{name} does not wrap a known python family")
            base_fn = base_by_name[base_name]
            for seed in (0, 1, 17):
                base = base_fn(random.Random(seed))
                run = fn(random.Random(seed))
                # The planted test is the ONLY thing the wrapper adds to
                # `files`; the base family's own file must survive verbatim.
                self.assertEqual(
                    {path: run.files[path] for path in base.files}, base.files, f"{name} seed={seed}"
                )
                self.assertEqual(
                    set(run.files) - set(base.files), {run.test_file}, f"{name} seed={seed}"
                )
                self.assertEqual(run.goal, base.goal, f"{name} seed={seed}")
                self.assertEqual(run.target, base.target, f"{name} seed={seed}")
                self.assertEqual(run.search, base.search, f"{name} seed={seed}")
                self.assertEqual(run.replace, base.replace, f"{name} seed={seed}")
                self.assertEqual(run.summary, base.summary, f"{name} seed={seed}")
            wrapped += 1
        self.assertEqual(wrapped, len(templates.PYTHON_TEMPLATES))

    def test_every_run_task_plants_a_unittest_it_is_granted_to_run(self):
        for name, fn in templates.RUN_TEMPLATES:
            for seed in range(20):
                task = fn(random.Random(seed))
                stem = task.target.removesuffix(".py")
                self.assertEqual(task.trajectory, task_mod.RUN_TRAJECTORY, name)
                self.assertEqual(task.lens, "python", name)
                self.assertEqual(task.test_file, f"test_{stem}.py", f"{name} seed={seed}")
                self.assertIn(task.test_file, task.files, f"{name} seed={seed}")
                self.assertEqual(
                    task.run_argv,
                    templates.UNITTEST_PREFIX + (task.test_file,),
                    f"{name} seed={seed}",
                )
                self.assertEqual(task.commands, (templates.UNITTEST_PREFIX,), name)

    def test_the_planted_test_imports_the_target_and_calls_its_probed_function(self):
        for name, fn in templates.RUN_TEMPLATES:
            base_name = name.removesuffix(templates.RUN_FAMILY_SUFFIX)
            probe = templates_run_verified.PROBE[base_name]
            for seed in (0, 3, 11):
                task = fn(random.Random(seed))
                stem = task.target.removesuffix(".py")
                source = task.files[task.test_file]
                func = templates_run_verified.probed_function(task.files[task.target], probe)
                self.assertIn(f"import {stem}\n", source, f"{name} seed={seed}")
                self.assertIn(f"{stem}.{func}({probe.args})", source, f"{name} seed={seed}")
                self.assertIn("unittest.TestCase", source, f"{name} seed={seed}")

    def test_there_is_exactly_one_probe_per_python_family(self):
        # A family without a probe could not plant a test at all; a probe
        # without a family is a stale entry nothing exercises. Both are
        # silent, so they are asserted mechanically.
        self.assertEqual(
            sorted(templates_run_verified.PROBE), sorted(templates_python.FAMILIES)
        )

    def test_run_tasks_carry_no_find_fields(self):
        for name, fn in templates.RUN_TEMPLATES:
            task = fn(random.Random(4))
            self.assertEqual(task.find_pattern, "", name)

    def test_every_run_task_is_structurally_valid(self):
        # Includes the fails-before rule (turn-4 spec §3): validating a
        # run-shaped task EXECUTES its planted test against the unpatched
        # workspace and requires a nonzero exit.
        for name, fn in templates.RUN_TEMPLATES:
            for seed in range(25):
                task = fn(random.Random(seed))
                violations = templates.validate_task(task)
                self.assertEqual(violations, [], f"{name} seed={seed}: {violations}\n{task}")

    def test_the_planted_test_fails_before_the_patch_and_passes_after(self):
        """Both halves of the verification contract in one place. The
        factory's validator only owns the FAILS-BEFORE half (the tool's own
        real run owns passes-after, and refuses to render a trajectory
        otherwise); asserting both here makes a family whose expected value
        drifted fail in the factory suite rather than only against the
        built binary."""
        for name, fn in templates.RUN_TEMPLATES:
            for seed in (0, 3, 11):
                task = fn(random.Random(seed))
                before = planted_test.run_python(task.files, task.run_argv)
                self.assertNotEqual(before.returncode, 0, f"{name} seed={seed}: {before.stdout}")

                patched = dict(task.files)
                patched[task.target] = task.files[task.target].replace(task.search, task.replace, 1)
                after = planted_test.run_python(patched, task.run_argv)
                self.assertEqual(after.returncode, 0, f"{name} seed={seed}: {after.stdout}")

    def test_run_targets_are_syntactically_valid_python_before_and_after_the_patch(self):
        # A family whose target (or patched target) did not compile would
        # abort the whole factory run at render time. Compiling both here
        # is the GPU-free, tool-free version of that check.
        for name, fn in templates.RUN_TEMPLATES:
            for seed in range(20):
                task = fn(random.Random(seed))
                before = task.files[task.target]
                after = before.replace(task.search, task.replace, 1)
                compile(before, task.target, "exec")
                compile(after, task.target, "exec")
                compile(task.files[task.test_file], task.test_file, "exec")
                self.assertNotEqual(before, after, f"{name} seed={seed}")


if __name__ == "__main__":
    unittest.main()
