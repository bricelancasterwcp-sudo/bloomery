"""The refusal-task half of the corpus generator pipeline (G5 design doc
§5), split out of `generate.py` to keep that file under the 400-line house
cap (same reasoning as turn 1's `templates_python.py`/`templates_text.py`
split): candidate generation, dedup, and the refuse-mode wire request, all
mirroring `generate.py`'s own patch-task functions one-for-one so the two
files read as parallel halves of one pipeline. Task 6a's gate-aware
rejection sampling is likewise mirrored one-for-one via the SAME
`gate_sampling.draw_all` helper `generate.py` uses -- one rejection-
sampling loop, shared, not two nearly-identical copies.
"""

from __future__ import annotations

import random

from tools.flywheel.factory import gate_sampling, templates, templates_refusal
from tools.flywheel.factory.contamination import GateFixture, normalize
from tools.flywheel.factory.task import RefusalTask

ENVELOPE = "v3"
PATCH_CODEC = "search_replace"


def refusal_family_functions(count: int) -> list:
    """The refusal analog of `generate._family_functions`: round-robins
    the four (family, lens) groups (`templates_refusal.GROUP_CYCLE_ORDER`)
    so a run of N refusal tasks splits as evenly as possible across both
    families and both lenses, cycling each group's own template variants
    in their sorted order."""
    group_indices = {name: 0 for name in templates_refusal.GROUP_CYCLE_ORDER}
    fns = []
    for i in range(count):
        group_name = templates_refusal.GROUP_CYCLE_ORDER[i % len(templates_refusal.GROUP_CYCLE_ORDER)]
        group = templates.REFUSAL_GROUPS[group_name]
        idx = group_indices[group_name]
        _, fn = group[idx % len(group)]
        group_indices[group_name] += 1
        fns.append(fn)
    return fns


def generate_candidate_refusal_tasks(
    rng: random.Random, count: int, gates: list[GateFixture], fail
) -> tuple[list[RefusalTask], dict[str, int], int]:
    """The refusal analog of `generate.generate_candidate_tasks` (G5
    design doc §5; task 6a for the `gates` parameter): continues the SAME
    `rng` stream (never a fresh one — rule 3's determinism contract),
    validating every result immediately via
    `templates.validate_refusal_task` and, when `gates` is non-empty,
    screening it via `gate_sampling.draw_all` — the SAME rejection-
    sampling loop `generate.py`'s patch path uses. `fail` is
    `generate.fail`, threaded in rather than imported, so both files
    share exactly one "print to stderr and raise SystemExit(1)"
    implementation. Returns (accepted tasks, gate_rejections by rule,
    total candidate draws)."""
    return gate_sampling.draw_all(
        rng, refusal_family_functions(count), templates.validate_refusal_task, gates, fail
    )


def dedup_refusal_tasks(tasks: list[RefusalTask]) -> tuple[list[RefusalTask], int]:
    """The refusal analog of `generate.dedup_tasks`. Keyed on normalized
    (goal, joined file contents) rather than (goal, target_contents): a
    missing-target task has no target contents at all, so the key spans
    every file the task carries (sorted by path, for a deterministic
    join) instead of one declared target."""
    seen: set[tuple[str, str]] = set()
    unique: list[RefusalTask] = []
    dropped = 0
    for task in tasks:
        joined_contents = "\x00".join(f"{path}={task.files[path]}" for path in sorted(task.files))
        key = (normalize(task.goal), normalize(joined_contents))
        if key in seen:
            dropped += 1
            continue
        seen.add(key)
        unique.append(task)
    return unique, dropped


def build_refusal_trajectory_request(task: RefusalTask) -> dict:
    """The refuse-mode wire shape (task-3 report's wire section): `expect`,
    `refusal_reason`, `target_missing`; no `search`/`replace`/`summary`.
    `target_contents` is `""` for the missing-target family (by
    convention, per `flywheel_tool.rs`'s own doc comment — never read when
    `target_missing` is true) and the real content for defect-absent."""
    target_contents = "" if task.target_missing else task.files[task.target]
    return {
        "cmd": "trajectory",
        "goal": task.goal,
        "patch_codec": PATCH_CODEC,
        "envelope": ENVELOPE,
        "target": task.target,
        "target_contents": target_contents,
        "expect": "refuse",
        "refusal_reason": task.refusal_reason,
        "target_missing": task.target_missing,
    }


def refusal_row_meta(task_id: str, task: RefusalTask, pair_name: str) -> dict:
    target_contents = "" if task.target_missing else task.files[task.target]
    return {
        "task_id": task_id,
        "template": task.name,
        "lens": task.lens,
        "pair": pair_name,
        "expect": "refuse",
        "goal": task.goal,
        "target": task.target,
        "target_contents": target_contents,
        # Same reason as `generate._row_meta`'s `files`: the post-hoc guard
        # screens every file. For a missing-target refusal this is the ONLY
        # way its real sibling files reach the guard at all — `target` is
        # by construction absent from `files`, so `target_contents` is "".
        "files": dict(task.files),
        "search": "",
    }


def verify_refusal_response(task_id: str, task: RefusalTask, response: dict, fail) -> None:
    """The refuse-class sanity check `generate._verify_and_build_rows`
    calls in place of a landing check: a refuse task has none (task-3's
    wire contract — `landed` is trivially `true` for refuse), so
    `verified: "refusal"` is the only signal the tool actually exercised
    the refuse path rather than a vacuous success."""
    if response.get("verified") != "refusal":
        fail(
            f"task {task_id} ({task.name}) did not come back verified as a refusal "
            f"(got verified={response.get('verified')!r}) -- the tool did not "
            f"exercise the refuse path. This is always a factory bug, never dropped "
            f"silently.\n"
            f"goal: {task.goal}\n"
            f"target: {task.target}\n"
            f"target_missing: {task.target_missing}"
        )
