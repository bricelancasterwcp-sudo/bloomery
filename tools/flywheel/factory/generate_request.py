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
`find_pattern` (presence selects the find shape) and `run_argv` (presence
selects the run shape). Each is sent ONLY for the shape that owns it, so
presence keeps meaning "this shape", never "this field happens to be
empty".

Turn 4 (spec §2) makes `commands` the exception, and deliberately: under
envelope-v4 the tool RENDERS the grant into every prompt, so `commands` is
no longer a run-shape input — it is what every task says about itself,
including the tasks that say "nothing". A plain, find-shaped or refuse
request therefore sends `commands: []` explicitly and gets
`Granted commands: none — run is not available in this task`; only a
run-verified request sends a prefix. The empty list is sent rather than the
key omitted because the two are indistinguishable to the tool's
deserializer but not to a reader: the explicit form says the factory
decided, which is the property that matters for a field whose value becomes
prompt text the model acts on. `envelope` and `commands` are both stamped
in `build_trajectory_request`, the one function that builds EVERY request
of either class, so there is exactly one place either can be wrong.
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

# envelope-v4: the rendered prompt carries the grant line (turn-4 spec §2).
# A prompt change is a new envelope under the lens-travels-with-verdict
# rule, so every turn-4 measurement is per-(model, v4) — which is exactly
# why this is a single constant stamped onto every request rather than a
# per-shape default. Turn 7 makes the stamp a PARAMETER with this constant
# as its default (still one stamp site): omitting `--envelope` stays
# byte-identical to every prior turn, and `--envelope v5` renders the
# declared-`done` envelope (turn-7 spec §2.2).
ENVELOPE = "v4"
V5_ENVELOPE = "v5"
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
        # `files` carries the planted test alongside the target -- an
        # ordinary sibling, materialized by the same code path as any
        # other, which is what lets the tool's real run find it.
        request["files"] = _wire_files(task)
        request["run_argv"] = list(task.run_argv)
    return request


def build_trajectory_request(task: AnyTask, envelope: str = ENVELOPE) -> dict:
    """The one function that builds every request of either class, and
    therefore the one place `envelope` and `commands` are stamped (see this
    module's docstring). A refuse task has no `commands` field at all —
    `RefusalTask` grants nothing by construction — so it renders the `none`
    line for a structural reason, not a defaulted one."""
    if isinstance(task, RefusalTask):
        request = generate_refusal.build_refusal_trajectory_request(task)
        commands: tuple[tuple[str, ...], ...] = ()
    else:
        request = _patch_request(task)
        commands = task.commands
    request["envelope"] = envelope
    request["commands"] = [list(prefix) for prefix in commands]
    return request


def expected_pair_names(task: AnyTask) -> tuple[str, ...]:
    if isinstance(task, RefusalTask):
        return REFUSAL_PAIR_NAMES
    return PAIR_NAMES[task.trajectory]


def row_meta(task_id: str, task: AnyTask, pair_name: str, envelope: str = ENVELOPE) -> dict:
    """One corpus row's `meta`. `files` covers every file the task carries
    (not just the target) because the post-hoc contamination guard can only
    screen siblings through this key; `trajectory` is what makes the
    pre-registered slice counts (design doc §2/§5) readable straight off
    the corpus, and lets the per-shape pair pins say WHICH shape a row's
    sequence should match.

    Under v5 ONLY (turn-7 spec §2.2; v4 meta stays byte-identical), three
    additive keys for `check_corpus_v5.py`: `envelope`, `replace` on patch
    rows (the checker's post-patch bytes), and `family` on refuse rows
    (the checker never infers family from a template name — the same rule
    the declaration endpoint enforces for fixtures)."""
    if isinstance(task, RefusalTask):
        meta = generate_refusal.refusal_row_meta(task_id, task, pair_name)
        if envelope == V5_ENVELOPE:
            meta["envelope"] = envelope
            meta["family"] = task.family
        return meta
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
    if envelope == V5_ENVELOPE:
        meta["envelope"] = envelope
        meta["replace"] = task.replace
    return meta
