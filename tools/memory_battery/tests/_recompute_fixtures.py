"""Hand-built journal, ledger and manifest fixtures for the `test_recompute*`
modules.

Split out of `test_recompute.py` on 2026-09-01 (carried-debt slice D). The
leading underscore keeps pytest from collecting this as a test module.
"""

from __future__ import annotations

import contextlib
import io
import json
import unittest
from pathlib import Path
from tempfile import TemporaryDirectory
from typing import Any

from tools.memory_battery.driver import WINDOW_CAP
from tools.memory_battery.recompute import main, recompute

TASKS = ["t0", "t1", "t2", "t3", "t4", "t5"]


def _write_manifest(corpus_dir: Path, names: list[str], corpus_seed: int = 20260826) -> None:
    manifest = {
        "instrument": "memory-battery-v1",
        "corpus_seed": corpus_seed,
        "n": len(names),
        "families": {},
        "tasks": [{"name": name, "workspace_sha256": f"sha-{name}"} for name in names],
    }
    corpus_dir.mkdir(parents=True, exist_ok=True)
    (corpus_dir / "manifest.json").write_text(json.dumps(manifest), encoding="utf-8")


def _write_jsonl(path: Path, rows: list[dict[str, Any]]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8") as handle:
        for row in rows:
            handle.write(json.dumps(row) + "\n")


def _identity_rows(arm: str, digest_p1: str | None, digest_p2: str | None) -> list[dict[str, Any]]:
    return [
        {"arm": arm, "phase": 1, "event": "identity", "digest": digest_p1, "ts": "t"},
        {"arm": arm, "phase": 2, "event": "identity", "digest": digest_p2, "ts": "t"},
    ]


def _ledger_row(
    arm: str, phase: int, task: str, task_id: str | None, status: str = "Done", wall_s: float = 1.0
) -> dict[str, Any]:
    return {
        "arm": arm,
        "phase": phase,
        "task": task,
        "agent_id": f"ignored-{task}-{phase}",
        "task_id": task_id,
        "status": status,
        "wall_s": wall_s,
        "suspend_ok": True,
        "ts": "t",
    }


def _memory_stamp(
    agent_id: str, task_id: str, mode: str, episode_id: str | None = None, candidates_checked: int = 0
) -> dict[str, Any]:
    return {
        "event": "MemoryStamp",
        "id": agent_id,
        "task_id": task_id,
        "mode": mode,
        "episode_id": episode_id,
        "candidates_checked": candidates_checked,
    }


def _task_step_done(agent_id: str, duration_ms: int = 1000, step: int = 1) -> dict[str, Any]:
    return {
        "event": "TaskStep",
        "id": agent_id,
        "step": step,
        "verb": "done",
        "outcome": "ok",
        "duration_ms": duration_ms,
        "args": [],
    }


def _infer_completed(
    agent_id: str, completion_tokens: int, prompt_tokens: int, duration_ms: int = 500
) -> dict[str, Any]:
    return {
        "event": "InferCompleted",
        "id": agent_id,
        "prompt_tokens": prompt_tokens,
        "completion_tokens": completion_tokens,
        "duration_ms": duration_ms,
    }


def _memory_mint(agent_id: str, task_id: str, episode_id: str) -> dict[str, Any]:
    return {"event": "MemoryMint", "id": agent_id, "task_id": task_id, "episode_id": episode_id}

# Arm C completion-token costs (task-4 brief's `cost(task)` formula): t1
# phase 1 gets TWO InferCompleted rows (40 + 15 = 55) -- the re-ask case.
C_P1_COSTS = {"t0": 100, "t1": (40, 15), "t2": 80, "t3": 60, "t4": 90, "t5": 70}
C_P2_COSTS = {"t0": 90, "t1": 50, "t3": 55, "t4": 85, "t5": 65}  # t2 dropped (driver-infra)

M_P1_COSTS = {"t0": 95, "t1": 52, "t2": 78, "t3": 58, "t4": 88, "t5": 68}
M_P2_COSTS = {"t0": 60, "t1": 53, "t2": 55, "t3": 50, "t4": 70}  # t5 dropped (no MemoryStamp)
M_P2_MODES = {"t0": "injected", "t1": "silent", "t2": "injected", "t3": "silent", "t4": "injected"}
M_P1_MINTED = {"t0", "t1", "t2", "t4", "t5"}  # t3 never mints in phase 1


def _build_arithmetic_fixture(tmp: Path) -> dict[str, Path]:
    corpus_dir = tmp / "corpus"
    _write_manifest(corpus_dir, TASKS)

    arm_c_dir = tmp / "arm_c"
    arm_m_dir = tmp / "arm_m"
    ledger_c = tmp / "ledger_c.jsonl"
    ledger_m = tmp / "ledger_m.jsonl"

    ledger_c_rows: list[dict[str, Any]] = list(_identity_rows("C", "digest-c", "digest-c"))
    tasks_journal_c: list[dict[str, Any]] = []
    boot_c: list[dict[str, Any]] = []

    for name in TASKS:
        task_id = f"c-p1-{name}-tid"
        agent_id = f"c-1-{name}-agent"
        ledger_c_rows.append(_ledger_row("C", 1, name, task_id))
        tasks_journal_c.append(_memory_stamp(agent_id, task_id, "off"))
        tasks_journal_c.append(_task_step_done(agent_id))
        cost = C_P1_COSTS[name]
        if isinstance(cost, tuple):
            boot_c.append(_infer_completed(agent_id, cost[0], cost[0] * 3))
            boot_c.append(_infer_completed(agent_id, cost[1], cost[1] * 3))
        else:
            boot_c.append(_infer_completed(agent_id, cost, cost * 3))

    for name in TASKS:
        if name == "t2":
            ledger_c_rows.append(_ledger_row("C", 2, name, None, status="driver-infra"))
            continue
        task_id = f"c-p2-{name}-tid"
        agent_id = f"c-2-{name}-agent"
        ledger_c_rows.append(_ledger_row("C", 2, name, task_id))
        tasks_journal_c.append(_memory_stamp(agent_id, task_id, "off"))
        tasks_journal_c.append(_task_step_done(agent_id))
        cost = C_P2_COSTS[name]
        boot_c.append(_infer_completed(agent_id, cost, cost * 3))

    _write_jsonl(ledger_c, ledger_c_rows)
    _write_jsonl(arm_c_dir / "journal" / "tasks.jsonl", tasks_journal_c)
    _write_jsonl(arm_c_dir / "journal" / "boot-0001.jsonl", boot_c)

    ledger_m_rows: list[dict[str, Any]] = list(_identity_rows("M", "digest-m", "digest-m"))
    tasks_journal_m: list[dict[str, Any]] = []
    boot_m: list[dict[str, Any]] = []

    for name in TASKS:
        task_id = f"m-p1-{name}-tid"
        agent_id = f"m-1-{name}-agent"
        ledger_m_rows.append(_ledger_row("M", 1, name, task_id))
        tasks_journal_m.append(_memory_stamp(agent_id, task_id, "silent", candidates_checked=0))
        tasks_journal_m.append(_task_step_done(agent_id))
        if name in M_P1_MINTED:
            tasks_journal_m.append(_memory_mint(agent_id, task_id, f"ep-{name}"))
        cost = M_P1_COSTS[name]
        boot_m.append(_infer_completed(agent_id, cost, cost * 3))

    for name in TASKS:
        if name == "t5":
            # Ledger row present with a real task_id, but NO MemoryStamp row
            # is ever written for it -- the "missing MemoryStamp" infra case.
            ledger_m_rows.append(_ledger_row("M", 2, name, "m-p2-t5-tid"))
            continue
        task_id = f"m-p2-{name}-tid"
        agent_id = f"m-2-{name}-agent"
        ledger_m_rows.append(_ledger_row("M", 2, name, task_id))
        mode = M_P2_MODES[name]
        episode_id = f"ep-{name}" if mode == "injected" else None
        tasks_journal_m.append(_memory_stamp(agent_id, task_id, mode, episode_id, candidates_checked=1))
        tasks_journal_m.append(_task_step_done(agent_id))
        cost = M_P2_COSTS[name]
        boot_m.append(_infer_completed(agent_id, cost, cost * 3))

    _write_jsonl(ledger_m, ledger_m_rows)
    _write_jsonl(arm_m_dir / "journal" / "tasks.jsonl", tasks_journal_m)
    _write_jsonl(arm_m_dir / "journal" / "boot-0001.jsonl", boot_m)

    return {
        "corpus_dir": corpus_dir,
        "arm_c_dir": arm_c_dir,
        "arm_m_dir": arm_m_dir,
        "ledger_c": ledger_c,
        "ledger_m": ledger_m,
    }


def _build_clean_fixture(
    tmp: Path,
    c_p1_costs: dict[str, int],
    c_p2_costs: dict[str, int],
    m_p1_costs: dict[str, int],
    m_p2_costs: dict[str, int],
    wall_s: float = 1.0,
    digest_c: tuple[str | None, str | None] = ("digest-c", "digest-c"),
    digest_m: tuple[str | None, str | None] = ("digest-m", "digest-m"),
    modes_c: tuple[str, str] = ("off", "off"),
    modes_m: tuple[str, str] = ("silent", "silent"),
    ledger_arm_c: str = "C",
    ledger_arm_m: str = "M",
) -> dict[str, Path]:
    """Each arm's phase-1 and phase-2 costs are four INDEPENDENT dicts
    (review finding I4: the earlier single shared `phase1_costs` parameter
    could not drive H2 to a violation on its own, since it forced arm M's
    phase-1 costs to always equal arm C's). A caller wanting H1
    (`median_C,p2 - median_C,p1`) and H2 (`median_M,p1 - median_C,p1`)
    both trivially at 0 passes the SAME dict for `c_p1_costs`/
    `c_p2_costs`/`m_p1_costs` -- every PASS/FAIL/UNMEASURABLE/determinism/
    golden/expected-digest test below does exactly this; a hygiene-
    violation test instead makes exactly one of the four diverge.

    `digest_c`/`digest_m` are `(phase1, phase2)` digest pairs -- default
    is a single consistent digest per arm (every existing caller's prior
    behavior); a caller can pass a differing pair to build an arm whose
    own two ledger identity rows disagree with each other.

    `modes_c`/`modes_m` are `(phase1, phase2)` `MemoryStamp` modes and
    `ledger_arm_c`/`ledger_arm_m` are the `arm` label written on every
    ledger row (branch-review finding I-2). Defaults are the only
    treatment-legal combination -- arm C memory-off, arm M memory-on,
    each ledger labelled with its own slot -- so every pre-existing caller
    stays hygiene-clean; the I-2 tests below make exactly one diverge."""
    names = list(c_p1_costs.keys())
    corpus_dir = tmp / "corpus"
    _write_manifest(corpus_dir, names)

    arm_c_dir = tmp / "arm_c"
    arm_m_dir = tmp / "arm_m"
    ledger_c = tmp / "ledger_c.jsonl"
    ledger_m = tmp / "ledger_m.jsonl"

    def _write_arm(
        arm_dir: Path,
        ledger_path: Path,
        arm: str,
        ledger_arm: str,
        digest: tuple[str | None, str | None],
        p1_costs: dict[str, int],
        p2_costs: dict[str, int],
        p1_mode: str,
        p2_mode: str,
    ) -> None:
        ledger_rows: list[dict[str, Any]] = list(_identity_rows(ledger_arm, digest[0], digest[1]))
        tasks_journal: list[dict[str, Any]] = []
        boot: list[dict[str, Any]] = []
        for phase, costs in ((1, p1_costs), (2, p2_costs)):
            for name in names:
                task_id = f"{arm}-{phase}-{name}-tid"
                agent_id = f"{arm}-{phase}-{name}-agent"
                ledger_rows.append(_ledger_row(ledger_arm, phase, name, task_id, wall_s=wall_s))
                mode = p1_mode if phase == 1 else p2_mode
                tasks_journal.append(_memory_stamp(agent_id, task_id, mode))
                tasks_journal.append(_task_step_done(agent_id))
                cost = costs[name]
                boot.append(_infer_completed(agent_id, cost, cost + 1))
        _write_jsonl(ledger_path, ledger_rows)
        _write_jsonl(arm_dir / "journal" / "tasks.jsonl", tasks_journal)
        _write_jsonl(arm_dir / "journal" / "boot-0001.jsonl", boot)

    _write_arm(
        arm_c_dir, ledger_c, "C", ledger_arm_c, digest_c, c_p1_costs, c_p2_costs, modes_c[0], modes_c[1]
    )
    _write_arm(
        arm_m_dir, ledger_m, "M", ledger_arm_m, digest_m, m_p1_costs, m_p2_costs, modes_m[0], modes_m[1]
    )

    return {
        "corpus_dir": corpus_dir,
        "arm_c_dir": arm_c_dir,
        "arm_m_dir": arm_m_dir,
        "ledger_c": ledger_c,
        "ledger_m": ledger_m,
    }


PHASE1 = {"t0": 40, "t1": 45, "t2": 50, "t3": 55, "t4": 60, "t5": 65}
CONSTANT_50 = {name: 50 for name in TASKS}


if __name__ == "__main__":
    unittest.main()
