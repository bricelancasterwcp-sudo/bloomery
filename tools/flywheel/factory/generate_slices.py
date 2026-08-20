"""Which template family fills which task slot — the corpus generator's
slot cycle, split out of `generate.py` when turn 3's three-shape repair
slice (task-7 brief) pushed that file past the 400-line house cap.

Two patterns, both PURELY POSITIONAL: which trajectory shape and which
lens a given slot uses is a function of the slot's index alone, never of
an rng draw. Determinism (brief rule 3) therefore never depends on draw
order here — only each family's *content* consumes rng, so a redraw after
a gate collision (`gate_sampling.py`) keeps the slot's family assignment
unchanged.

- `_TRAJECTORY_PATTERN` cycles the three repair shapes, so `--count 999`
  is exactly 333 plain / 333 find-shaped / 333 run-verified (turn-3 design
  doc §2's pre-registered slice split). `plain` leads the cycle on
  purpose: slot 0 stays the plain python family every prior turn's tests
  predict it to be.
- `_FAMILY_PATTERN` cycles the plain slice's 3:2 python:plaintext mix
  (design spec §3's ~600/~400 of ~1000), unchanged from turn 1 apart from
  now counting plain slots rather than all slots.

The find slice needs no lens pattern of its own: `templates.FIND_TEMPLATES`
is python-then-plaintext and is walked round-robin, so its 3 python + 2
plaintext families already produce the same 3:2 mix. The run slice is
python-only (there is no plaintext verification to run).
"""

from __future__ import annotations

import random
from typing import Callable

from tools.flywheel.factory import templates
from tools.flywheel.factory.task import FIND_TRAJECTORY, PLAIN_TRAJECTORY, RUN_TRAJECTORY, Task

TemplateFn = Callable[[random.Random], Task]

_TRAJECTORY_PATTERN: tuple[str, ...] = (PLAIN_TRAJECTORY, FIND_TRAJECTORY, RUN_TRAJECTORY)
_FAMILY_PATTERN: tuple[str, ...] = ("python", "python", "python", "plaintext", "plaintext")


def trajectory_for_slot(slot: int) -> str:
    """The shape task slot `slot` renders. Public because the shape split
    is a pre-registered property of a run, so it is asserted directly
    rather than inferred from a generated corpus."""
    return _TRAJECTORY_PATTERN[slot % len(_TRAJECTORY_PATTERN)]


def _plain_registry_and_index(
    plain_slot: int, lens_counters: dict[str, int]
) -> tuple[tuple[tuple[str, TemplateFn], ...], int]:
    """Turn 1's lens cycle, now counting PLAIN slots rather than all
    slots: each lens keeps its own running index into its own registry, so
    both lenses' families are still walked round-robin."""
    lens = _FAMILY_PATTERN[plain_slot % len(_FAMILY_PATTERN)]
    registry = templates.PYTHON_TEMPLATES if lens == "python" else templates.TEXT_TEMPLATES
    index = lens_counters[lens]
    lens_counters[lens] += 1
    return registry, index


def family_functions(count: int) -> list[TemplateFn]:
    """The ordered list of template functions a run will call, one per
    task slot: each slot's shape from `trajectory_for_slot`, then that
    shape's own registry cycled in its sorted order (and, for the plain
    slice, its lens from `_FAMILY_PATTERN`)."""
    shape_counters = {PLAIN_TRAJECTORY: 0, FIND_TRAJECTORY: 0, RUN_TRAJECTORY: 0}
    lens_counters = {"python": 0, "plaintext": 0}
    fns: list[TemplateFn] = []

    for slot in range(count):
        shape = trajectory_for_slot(slot)
        if shape == PLAIN_TRAJECTORY:
            registry, index = _plain_registry_and_index(shape_counters[shape], lens_counters)
        else:
            registry = templates.FIND_TEMPLATES if shape == FIND_TRAJECTORY else templates.RUN_TEMPLATES
            index = shape_counters[shape]
        shape_counters[shape] += 1
        _, fn = registry[index % len(registry)]
        fns.append(fn)

    return fns
