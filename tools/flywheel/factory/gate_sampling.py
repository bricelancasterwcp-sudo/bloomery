"""Gate-aware rejection sampling (task 6a): a frozen gate set (e.g.
`codec-tasks-v2-mixed.toml`) and a factory-generated corpus must never
collide. The two-gate contamination guard (`contamination.check_corpus`)
already proves this AFTER a full corpus is written -- but by then a
collision means throwing away the run: Task 6's first attempt generated
1299 tasks and the guard found 729 `goal_near_duplicate` violations
against `codec-tasks-v2-mixed`'s own template-family structure (same
families, same shape, different rng draws).

This module screens each candidate at DRAW time instead, reusing
`contamination.task_violates_gates` (itself built on the guard's own rule
set -- no rule is duplicated here, only the draw/retry loop around it): a
colliding candidate is silently dropped, and the SAME rng stream draws
again for the SAME task slot, until an accepted candidate is found or the
termination guard (below) fires. The frozen gate never yields; the
corpus does.

Shared by `generate.py` (patch-task candidates) and `generate_refusal.py`
(refuse-task candidates) so there is exactly one rejection-sampling loop,
not two nearly-identical copies.
"""

from __future__ import annotations

import random
from dataclasses import dataclass, field
from typing import Callable, TypeVar

from tools.flywheel.factory.contamination import GateFixture, task_violates_gates

T = TypeVar("T")

# Termination guard: never spin forever, never silently under-fill. Two
# independent trips, whichever fires first. Both are sized so a healthy
# run (even one with a substantial-but-recoverable rejection rate) never
# false-trips: REJECTION_WINDOW is large enough that ordinary variance in
# a ~50-70% reject rate cannot itself push a 200-draw window over 90%,
# while a genuinely near-total collision (the "gate collides with
# everything" case) still fills the window with rejections almost
# immediately and aborts fast rather than waiting for MAX_DRAW_MULTIPLE.
MAX_DRAW_MULTIPLE = 20
REJECTION_WINDOW = 200
REJECTION_RATE_THRESHOLD = 0.9


class GateOverlapTooDenseError(RuntimeError):
    """Named abort: gate-aware rejection sampling could not fill the
    requested candidate count because too large a fraction of draws
    collide with a gate fixture. Always means the template/gate overlap
    is too dense for this (seed, gates) combination -- never a silent
    under-fill, never an infinite loop. Raised from `RejectionSampler`
    itself (not caught there), so a direct caller (tests; anything that
    wants the raw signal) sees it as-is; `generate.py`'s CLI `main()` is
    the one place that catches it and reroutes through `fail()` for a
    consistent operator-facing message."""


@dataclass
class RejectionSampler:
    """One rejection-sampling run for ONE task class (patch or refuse).
    `gates=[]` is the degenerate case: every candidate is accepted on its
    first draw, with zero extra rng consumption -- byte-identical to the
    pre-task-6a code path (generate.py's rule 3, determinism)."""

    gates: list[GateFixture]
    requested: int
    fail: Callable[[str], None]

    gate_rejections: dict[str, int] = field(default_factory=dict)
    total_draws: int = 0
    _recent: list[bool] = field(default_factory=list)  # True = accepted; oldest first

    def draw(
        self,
        rng: random.Random,
        draw_fn: Callable[[random.Random], T],
        validate_fn: Callable[[T], list[str]],
    ) -> T:
        """Draws one candidate for one task slot, redrawing from the SAME
        `rng` on a gate collision. `validate_fn`'s result is a
        STRUCTURAL check (`templates.validate_task` /
        `validate_refusal_task`) -- a violation there is always a
        factory bug (`self.fail`, aborts immediately, never retried);
        only a GATE collision triggers a redraw."""
        while True:
            self._check_termination()
            task = draw_fn(rng)
            self.total_draws += 1

            violations = validate_fn(task)
            if violations:
                self.fail(
                    f"template {task.name!r} produced a structurally invalid task: "
                    f"{violations}\ngoal: {task.goal}"
                )

            rule = task_violates_gates(task, self.gates)
            if rule is None:
                self._record(accepted=True)
                return task

            self.gate_rejections[rule] = self.gate_rejections.get(rule, 0) + 1
            self._record(accepted=False)

    def _record(self, accepted: bool) -> None:
        self._recent.append(accepted)
        if len(self._recent) > REJECTION_WINDOW:
            self._recent.pop(0)

    def _check_termination(self) -> None:
        if self.total_draws >= self.requested * MAX_DRAW_MULTIPLE:
            raise GateOverlapTooDenseError(
                f"gate-aware rejection sampling drew {self.total_draws} candidate(s) trying to "
                f"fill {self.requested} slot(s) ({MAX_DRAW_MULTIPLE}x the requested count) "
                f"without success -- the template/gate overlap is too dense for this seed/gate "
                f"combination. Rejections so far: {dict(sorted(self.gate_rejections.items()))}"
            )
        if len(self._recent) >= REJECTION_WINDOW:
            rejected = sum(1 for accepted in self._recent if not accepted)
            rate = rejected / len(self._recent)
            if rate > REJECTION_RATE_THRESHOLD:
                raise GateOverlapTooDenseError(
                    f"gate-aware rejection sampling: {rate:.0%} of the last {REJECTION_WINDOW} "
                    f"candidate draws collided with a gate fixture -- the template/gate overlap "
                    f"is too dense for this seed/gate combination. Rejections so far: "
                    f"{dict(sorted(self.gate_rejections.items()))}"
                )


def draw_all(
    rng: random.Random,
    fns: list[Callable[[random.Random], T]],
    validate_fn: Callable[[T], list[str]],
    gates: list[GateFixture],
    fail: Callable[[str], None],
) -> tuple[list[T], dict[str, int], int]:
    """Runs one `RejectionSampler` across `fns` (one family function per
    requested task slot, in order -- `generate._family_functions` /
    `generate_refusal.refusal_family_functions`; the slot's family
    assignment never changes on a redraw, only its rng-derived content
    does). Returns (accepted tasks in slot order, gate_rejections by
    rule, total candidate draws)."""
    sampler = RejectionSampler(gates=gates, requested=len(fns), fail=fail)
    tasks = [sampler.draw(rng, fn, validate_fn) for fn in fns]
    return tasks, sampler.gate_rejections, sampler.total_draws
