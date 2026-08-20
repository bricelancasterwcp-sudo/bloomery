"""The wire-facing half of the corpus generator: turning a validated
`Task`/`RefusalTask` into the `flywheel-tool trajectory` request that
renders it, and turning the response's pairs back into corpus row `meta`.

Split out of `generate.py` when turn 3's three-shape repair slice
(task-7 brief) pushed that file past the 400-line house cap — the same
split `generate_refusal.py` and `gate_sampling.py` already made, along the
same seam: `generate.py` keeps the PIPELINE (draw, dedup, verify, split,
write) and this module owns the REQUEST/ROW format. Everything the wire
shape needs is here and nothing else is, so a future wire field has one
obvious home.

Turn 3's additive fields (`flywheel_tool.rs`'s `TrajectoryRequest`; names
exact): `files` (the whole workspace as `{"path", "contents"}` objects),
`find_pattern` (presence selects the find shape), `run_argv` (presence
selects the run shape) and `commands` (the grant that permits the run).
They are sent ONLY for the shape that owns them, so a plain task's request
is byte-identical to turn 1's — which is what keeps the tool's turn-1
golden green.
"""

from __future__ import annotations

from typing import Union

from tools.flywheel.factory import generate_refusal
from tools.flywheel.factory.task import (
    FIND_TRAJECTORY,
    RUN_TRAJECTORY,
    RefusalTask,
    Task,
)

ENVELOPE = "v3"
PATCH_CODEC = "search_replace"

AnyTask = Union[Task, RefusalTask]

# The pair sequence each shape renders, in order. These are the names the
# corpus rows carry and the lengths `generate.py` asserts the tool returned
# — one table, so "how many pairs does this shape have" and "what are they
# called" can never disagree.
PAIR_NAMES: dict[str, tuple[str, ...]] = {
    "plain": ("read", "patch", "done"),
    FIND_TRAJECTORY: ("find", "read", "patch", "done"),
    RUN_TRAJECTORY: ("read", "patch", "run", "done"),
}
REFUSAL_PAIR_NAMES: tuple[str, ...] = ("read", "done")


def _wire_files(task: Task) -> list[dict[str, str]]:
    """`files` as the tool deserializes it: a LIST of objects, sorted by
    path so the request bytes (and therefore the run) stay deterministic
    regardless of dict construction order."""
    return [{"path": path, "contents": task.files[path]} for path in sorted(task.files)]


def _patch_request(task: Task) -> dict:
    request = {
        "cmd": "trajectory",
        "goal": task.goal,
        "patch_codec": PATCH_CODEC,
        "envelope": ENVELOPE,
        "target": task.target,
        "target_contents": task.files[task.target],
        "search": task.search,
        "replace": task.replace,
        "summary": task.summary,
    }
    if task.trajectory == FIND_TRAJECTORY:
        # The find completion's path is hardcoded "." tool-side (fixture
        # dirs are flat), so the request never sends one.
        request["files"] = _wire_files(task)
        request["find_pattern"] = task.find_pattern
    elif task.trajectory == RUN_TRAJECTORY:
        request["files"] = _wire_files(task)
        request["run_argv"] = list(task.run_argv)
        request["commands"] = [list(prefix) for prefix in task.commands]
    return request


def build_trajectory_request(task: AnyTask) -> dict:
    if isinstance(task, RefusalTask):
        return generate_refusal.build_refusal_trajectory_request(task)
    return _patch_request(task)


def expected_pair_names(task: AnyTask) -> tuple[str, ...]:
    if isinstance(task, RefusalTask):
        return REFUSAL_PAIR_NAMES
    return PAIR_NAMES[task.trajectory]


def row_meta(task_id: str, task: AnyTask, pair_name: str) -> dict:
    """One corpus row's `meta`. `files` covers every file the task carries
    (not just the target) because the post-hoc contamination guard can only
    screen siblings through this key; `trajectory` is what makes the
    pre-registered slice counts (design doc §2/§5) readable straight off
    the corpus, and lets the per-shape pair pins say WHICH shape a row's
    sequence should match."""
    if isinstance(task, RefusalTask):
        return generate_refusal.refusal_row_meta(task_id, task, pair_name)
    meta = {
        "task_id": task_id,
        "template": task.name,
        "lens": task.lens,
        "pair": pair_name,
        "expect": "patch",
        "trajectory": task.trajectory,
        "goal": task.goal,
        "target": task.target,
        "target_contents": task.files[task.target],
        "files": dict(task.files),
        "search": task.search,
    }
    # Shape-specific keys, present only on the shape that owns them — the
    # meta mirrors the request, so a row records exactly what produced it.
    if task.trajectory == FIND_TRAJECTORY:
        meta["find_pattern"] = task.find_pattern
    elif task.trajectory == RUN_TRAJECTORY:
        meta["run_argv"] = list(task.run_argv)
    return meta
