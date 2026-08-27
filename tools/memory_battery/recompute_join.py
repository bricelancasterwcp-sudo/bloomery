"""The task-half join for `tools.memory_battery.recompute` (design spec §4;
task-4 brief, pinned success/infra/cost formulas). Split out of
`recompute.py` to keep each file under the house 800-line ceiling
(`coding-style.md`); the public entry point stays
`tools.memory_battery.recompute.recompute`.

**The join** (task-4 brief, `bloomery-daemon/src/task/registry.rs` as
built): the driver ledger's task-half row names `task_id`; the task
journal's `MemoryStamp` row for that `task_id` names `id` -- the fresh
agent the daemon created for that one task-half (`registry.rs`:
`MemoryStamp { id: agent_id.clone(), task_id: worker_task_id.clone(),
... }`). Every `InferCompleted`/`TaskStep` row sharing that `id` belongs to
this task-half and only this one -- a fresh agent per task-half (design
spec §5) makes the join exact.
"""

from __future__ import annotations

from pathlib import Path
from typing import Any

from tools.memory_battery.driver import DRIVER_INFRA_STATUS
from tools.memory_battery.recompute_journal import (
    _completion_tokens_by_agent,
    _done_agent_ids,
    _index_memory_stamps,
    _read_jsonl,
    _read_ledger,
    _row_counts,
    _task_step_count_by_agent,
    _task_step_duration_by_agent,
)


def _measure_arm(
    manifest_tasks: list[dict[str, Any]],
    ledger_task_halves: dict[tuple[Any, Any], dict[str, Any]],
    stamps_by_task_id: dict[str, dict[str, Any]],
    done_agents: set[str],
    step_counts: dict[str, int],
    step_walls: dict[str, int],
    completion_by_agent: dict[str, int],
) -> tuple[dict[tuple[int, str], dict[str, Any]], list[dict[str, Any]]]:
    """One arm's full join, task-4 brief verbatim: "success := a TaskStep
    with verb=='done' exists for the task's agent; infra := ledger
    driver-infra status OR a task_id with no MemoryStamp row". Every
    manifest task, both phases, is visited in manifest order -- a task not
    joinable for any reason becomes exactly one named ``dropped`` entry
    (none-vs-zero: never a silent zero), and no OTHER reason ever appears
    in ``dropped`` beyond the four named below, so the ``infra`` flag on
    each entry reflects the brief's own two-clause infra definition exactly
    (a missing ledger row / missing task_id are unjoinable but NOT "infra"
    by that definition -- they cannot occur in a well-formed real run,
    since the driver always writes exactly one ledger row per task-half,
    but are handled here defensively rather than assumed away)."""
    measurements: dict[tuple[int, str], dict[str, Any]] = {}
    dropped: list[dict[str, Any]] = []
    for phase in (1, 2):
        for entry in manifest_tasks:
            name = entry["name"]
            ledger_row = ledger_task_halves.get((phase, name))
            if ledger_row is None:
                dropped.append(
                    {
                        "task": name,
                        "phase": phase,
                        "infra": False,
                        "reason": f"no ledger row for task {name!r} phase {phase}",
                    }
                )
                continue
            if ledger_row.get("status") == DRIVER_INFRA_STATUS:
                dropped.append(
                    {
                        "task": name,
                        "phase": phase,
                        "infra": True,
                        "reason": f"driver-infra status recorded for task {name!r} phase {phase}",
                    }
                )
                continue
            task_id = ledger_row.get("task_id")
            if not task_id:
                dropped.append(
                    {
                        "task": name,
                        "phase": phase,
                        "infra": False,
                        "reason": f"ledger row for task {name!r} phase {phase} carries no task_id",
                    }
                )
                continue
            stamp = stamps_by_task_id.get(task_id)
            if stamp is None:
                dropped.append(
                    {
                        "task": name,
                        "phase": phase,
                        "infra": True,
                        "reason": (
                            f"no MemoryStamp row for task_id {task_id!r} "
                            f"(task {name!r} phase {phase})"
                        ),
                    }
                )
                continue
            agent_id = stamp["id"]
            measurements[(phase, name)] = {
                "cost": completion_by_agent.get(agent_id, 0),
                "steps": step_counts.get(agent_id, 0),
                "wall_ms": step_walls.get(agent_id, 0),
                "success": agent_id in done_agents,
                "mode": stamp.get("mode"),
                "episode_id": stamp.get("episode_id"),
                "candidates_checked": stamp.get("candidates_checked"),
            }
    return measurements, dropped


def _build_arm_view(
    measurements: dict[tuple[int, str], dict[str, Any]], manifest_tasks: list[dict[str, Any]]
) -> dict[str, dict[int, Any]]:
    """Reshapes the (phase, task) -> measurement join into per-phase views
    -- ``costs``/``modes``/``successes`` keyed by task name (manifest
    order, so a dropped task is simply absent -- ITT: "every non-dropped
    task contributes", ``dropped``'s complement), plus flat ``steps``/
    ``wall_ms`` lists for the advisory medians."""
    names = [task["name"] for task in manifest_tasks]
    costs: dict[int, dict[str, int]] = {1: {}, 2: {}}
    modes: dict[int, dict[str, str | None]] = {1: {}, 2: {}}
    successes: dict[int, dict[str, bool]] = {1: {}, 2: {}}
    steps: dict[int, list[int]] = {1: [], 2: []}
    wall_ms: dict[int, list[int]] = {1: [], 2: []}
    for phase in (1, 2):
        for name in names:
            measurement = measurements.get((phase, name))
            if measurement is None:
                continue
            costs[phase][name] = measurement["cost"]
            modes[phase][name] = measurement["mode"]
            successes[phase][name] = measurement["success"]
            steps[phase].append(measurement["steps"])
            wall_ms[phase].append(measurement["wall_ms"])
    return {"costs": costs, "modes": modes, "successes": successes, "steps": steps, "wall_ms": wall_ms}


def _load_arm(arm_dir: Path, ledger_path: Path, manifest_tasks: list[dict[str, Any]]) -> dict[str, Any]:
    """One arm's full pipeline: read both journals + the ledger, join,
    reshape into the per-phase view. Returns everything `recompute()`
    needs to assemble that arm's slice of the output (view, dropped list,
    raw row counts, identity digests, and the task_id->phase map the H4
    mint-rate advisory needs)."""
    tasks_journal_rows = _read_jsonl(arm_dir / "journal" / "tasks.jsonl")
    boot_paths = sorted((arm_dir / "journal").glob("boot-*.jsonl"))
    boot_journal_rows: list[dict[str, Any]] = []
    for boot_path in boot_paths:
        boot_journal_rows.extend(_read_jsonl(boot_path))

    ledger_task_halves, ledger_identity_rows = _read_ledger(ledger_path)
    identity_by_phase = {row.get("phase"): row.get("digest") for row in ledger_identity_rows}

    stamps_by_task_id = _index_memory_stamps(tasks_journal_rows)
    done_agents = _done_agent_ids(tasks_journal_rows)
    step_counts = _task_step_count_by_agent(tasks_journal_rows)
    step_walls = _task_step_duration_by_agent(tasks_journal_rows)
    completion_by_agent = _completion_tokens_by_agent(boot_journal_rows)

    measurements, dropped = _measure_arm(
        manifest_tasks,
        ledger_task_halves,
        stamps_by_task_id,
        done_agents,
        step_counts,
        step_walls,
        completion_by_agent,
    )
    view = _build_arm_view(measurements, manifest_tasks)
    task_id_to_phase = {
        row.get("task_id"): row.get("phase") for row in ledger_task_halves.values() if row.get("task_id")
    }

    return {
        "identity_by_phase": identity_by_phase,
        "dropped": dropped,
        "view": view,
        "row_counts": _row_counts(tasks_journal_rows),
        "tasks_journal_rows": tasks_journal_rows,
        "task_id_to_phase": task_id_to_phase,
    }
