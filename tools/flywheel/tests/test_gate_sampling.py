"""Unit tests for tools.flywheel.factory.gate_sampling (task 6a): the
rejection-sampling loop shared by generate.py (patch tasks) and
generate_refusal.py (refuse tasks).

These exercise `RejectionSampler`/`GateOverlapTooDenseError`/`draw_all`
directly against small hand-built `GateFixture`s and controllable
`draw_fn` stubs, rather than real templates -- so the "collides with
everything" abort path is deterministic and fast regardless of the real
template value space. End-to-end coverage against a REAL template's
first-draw output and the real `generate.py` CLI lives in
`test_generate_gates.py`.
"""

import random
import unittest

from tools.flywheel.factory import gate_sampling
from tools.flywheel.factory.gate_vocabulary import GateFixture
from tools.flywheel.factory.task import Task


def _gate(goal, target="unused.py"):
    return GateFixture(
        name="planted",
        lens="python",
        target=target,
        files={target: "x = 1\n"},
        goal=goal,
        expect="patch",
        search=None,
        replace=None,
        refusal_reason=None,
    )


def _task(goal, target="a.py", contents="x = 1\ny = 2\n"):
    return Task(
        name="fam",
        lens="python",
        target=target,
        files={target: contents},
        goal=goal,
        search="x = 1",
        replace="x = 2",
        summary="s",
    )


def _no_structural_violations(_task):
    return []


class RejectionSamplerNoGatesTest(unittest.TestCase):
    """`gates=[]` must be the exact byte-identical-to-pre-task-6a code
    path: one draw per slot, zero extra rng consumption, zero rejections
    recorded."""

    def test_zero_gates_accepts_the_first_draw_with_no_extra_rng_consumption(self):
        calls = []

        def draw_fn(rng):
            calls.append(rng.random())
            return _task(f"goal {calls[-1]}")

        rng_gated = random.Random(1)
        rng_plain = random.Random(1)

        sampler = gate_sampling.RejectionSampler(gates=[], requested=1, fail=self.fail)
        task = sampler.draw(rng_gated, draw_fn, _no_structural_violations)

        expected_first = rng_plain.random()
        self.assertEqual(calls, [expected_first])
        self.assertEqual(sampler.total_draws, 1)
        self.assertEqual(sampler.gate_rejections, {})
        self.assertIn(f"goal {expected_first}", task.goal)


class RejectionSamplerDropsAndRedrawsTest(unittest.TestCase):
    def test_a_colliding_candidate_is_dropped_and_the_next_draw_is_kept(self):
        gate = _gate(goal="fix a.py's alpha thing right away. Patch the file, then emit done.")
        outputs = iter(
            [
                _task("fix a.py's alpha thing right away. Patch the file, then emit done."),  # collides
                _task(
                    "fix b.py's beta thing entirely different topic today. Patch the file, then emit done.",
                    target="b.py",
                ),
            ]
        )

        def draw_fn(rng):
            return next(outputs)

        sampler = gate_sampling.RejectionSampler(gates=[gate], requested=1, fail=self.fail)
        task = sampler.draw(random.Random(1), draw_fn, _no_structural_violations)

        self.assertEqual(task.target, "b.py")
        self.assertEqual(sampler.total_draws, 2)
        self.assertEqual(sampler.gate_rejections, {"goal_match": 1})

    def test_rejections_are_counted_per_rule_across_multiple_gates(self):
        gate_goal_collision = _gate(goal="goal collision text right here today. Patch the file, then emit done.")
        gate_target_collision = _gate(
            goal="a totally unrelated premise about something else. Patch the file, then emit done.",
            target="c.py",
        )
        outputs = iter(
            [
                _task("goal collision text right here today. Patch the file, then emit done."),
                _task(
                    "some other content about a different topic now. Patch the file, then emit done.",
                    target="c.py",
                ),
                _task(
                    "finally an accepted task with fresh unrelated words. Patch the file, then emit done.",
                    target="z.py",
                ),
            ]
        )

        def draw_fn(rng):
            return next(outputs)

        sampler = gate_sampling.RejectionSampler(
            gates=[gate_goal_collision, gate_target_collision], requested=1, fail=self.fail
        )
        task = sampler.draw(random.Random(1), draw_fn, _no_structural_violations)

        self.assertEqual(task.target, "z.py")
        self.assertEqual(sampler.total_draws, 3)
        self.assertEqual(sampler.gate_rejections, {"goal_match": 1, "target_filename_match": 1})


class StructuralViolationsAreNeverRetriedTest(unittest.TestCase):
    """A structural violation is always a factory bug (rule 2, unchanged
    by task 6a) -- never dropped-and-redrawn like a gate collision."""

    def test_a_structural_violation_aborts_immediately_even_with_gates_configured(self):
        gate = _gate(goal="irrelevant to this test. Patch the file, then emit done.")
        draws = []

        def draw_fn(rng):
            draws.append(1)
            return _task("some goal here today. Patch the file, then emit done.")

        def always_invalid(_task):
            return ["some structural problem"]

        sampler = gate_sampling.RejectionSampler(gates=[gate], requested=1, fail=self.fail)
        with self.assertRaises(AssertionError):  # self.fail raises AssertionError
            sampler.draw(random.Random(1), draw_fn, always_invalid)
        self.assertEqual(len(draws), 1, "a structural violation must never be retried")


class TerminationGuardTest(unittest.TestCase):
    """Requirement 2: never spin forever, never silently under-fill."""

    def test_a_gate_that_collides_with_every_candidate_aborts_with_a_named_error(self):
        gate = _gate(goal="collide with every single candidate no matter what. Patch the file, then emit done.")

        def always_colliding_draw_fn(rng):
            return _task("collide with every single candidate no matter what. Patch the file, then emit done.")

        sampler = gate_sampling.RejectionSampler(gates=[gate], requested=2, fail=self.fail)
        with self.assertRaises(gate_sampling.GateOverlapTooDenseError) as ctx:
            sampler.draw(random.Random(1), always_colliding_draw_fn, _no_structural_violations)

        self.assertIn("too dense", str(ctx.exception))
        self.assertLessEqual(sampler.total_draws, 2 * gate_sampling.MAX_DRAW_MULTIPLE)
        self.assertEqual(sampler.gate_rejections, {"goal_match": sampler.total_draws})

    def test_the_abort_is_a_distinct_named_exception_type_not_a_bare_runtimeerror_lookalike(self):
        self.assertTrue(issubclass(gate_sampling.GateOverlapTooDenseError, RuntimeError))
        self.assertNotEqual(gate_sampling.GateOverlapTooDenseError, RuntimeError)


class DrawAllTest(unittest.TestCase):
    def test_draw_all_returns_one_task_per_fn_in_slot_order(self):
        def make_fn(target):
            def fn(rng):
                return _task(f"fix {target} entirely, unique wording here. Patch the file, then emit done.", target=target)

            return fn

        fns = [make_fn("a.py"), make_fn("b.py"), make_fn("c.py")]
        tasks, rejections, draws = gate_sampling.draw_all(random.Random(1), fns, _no_structural_violations, [], self.fail)

        self.assertEqual([t.target for t in tasks], ["a.py", "b.py", "c.py"])
        self.assertEqual(rejections, {})
        self.assertEqual(draws, 3)


if __name__ == "__main__":
    unittest.main()
