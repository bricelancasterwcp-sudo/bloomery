"""Hand-built fixtures for the `test_recompute_v2*` modules.

Split out of `test_recompute_v2.py` on 2026-09-01 (carried-debt slice D). The
leading underscore keeps pytest from collecting this as a test module.
"""

from __future__ import annotations

import json
import unittest
from pathlib import Path
from tempfile import TemporaryDirectory
from typing import Any

from tools.memory_battery.recompute_v2 import B_V2, SEED_V2, recompute_v2

TASKS = ["t0", "t1", "t2", "t3", "t4", "t5"]


def _write_manifest(corpus_dir: Path, names: list[str], corpus_seed: int = 20260828) -> None:
    manifest = {
        "instrument": "refalsify-battery-v2",
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
    agent_id: str,
    task_id: str,
    mode: str,
    episode_id: str | None = None,
    candidates_checked: int = 0,
    refalsify: str | None = None,
) -> dict[str, Any]:
    return {
        "event": "MemoryStamp",
        "id": agent_id,
        "task_id": task_id,
        "mode": mode,
        "episode_id": episode_id,
        "candidates_checked": candidates_checked,
        "refalsify": refalsify,
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


def _write_arm(
    arm_dir: Path,
    ledger_path: Path,
    ledger_arm_label: str,
    digest: tuple[str | None, str | None],
    names: list[str],
    p1_costs: dict[str, int],
    p2_costs: dict[str, int],
    p1_mode: str = "silent",
    p2_mode: str = "injected",
    p2_refalsify: dict[str, str | None] | None = None,
    p1_wall_ms: dict[str, int] | None = None,
    p2_wall_ms: dict[str, int] | None = None,
    skip_names_p2: set[str] | None = None,
    p1_minted: set[str] | None = None,
    p2_mode_by_task: dict[str, str] | None = None,
    p1_stepless: set[str] | None = None,
    p2_stepless: set[str] | None = None,
) -> None:
    """Writes one v2 arm's ledger + both journals. `p2_refalsify` overrides
    the auto-derived refalsify spelling (default: "premise_held" for an
    injected p2 stamp, None for silent -- design spec §3's happy-path
    prediction) per task name; `skip_names_p2` omits the ledger row
    entirely for those tasks in phase 2 (the "no ledger row" drop shape).
    `p1_minted` writes a MemoryMint row for those task names in phase 1.
    `p2_mode_by_task` overrides the uniform `p2_mode` for specific task
    names (G2 deficit/excess fixtures: one task's mode differs). `p1_stepless`/
    `p2_stepless` omit ONLY the `TaskStep` row for those task names (ledger
    row, MemoryStamp, and InferCompleted rows are all still written, so the
    task-half still joins normally) -- the "stepless but conducted" shape
    A1's none-vs-zero fix exists for: the task has a real cost and mode,
    but no wall measurement at all."""
    skip_names_p2 = skip_names_p2 or set()
    p1_minted = p1_minted or set()
    p2_mode_by_task = p2_mode_by_task or {}
    p1_stepless = p1_stepless or set()
    p2_stepless = p2_stepless or set()
    ledger_rows: list[dict[str, Any]] = list(_identity_rows(ledger_arm_label, digest[0], digest[1]))
    tasks_journal: list[dict[str, Any]] = []
    boot: list[dict[str, Any]] = []

    for name in names:
        task_id = f"{ledger_arm_label}-1-{name}-tid"
        agent_id = f"{ledger_arm_label}-1-{name}-agent"
        ledger_rows.append(_ledger_row(ledger_arm_label, 1, name, task_id))
        tasks_journal.append(_memory_stamp(agent_id, task_id, p1_mode, refalsify=None))
        if name not in p1_stepless:
            duration = (p1_wall_ms or {}).get(name, 1000)
            tasks_journal.append(_task_step_done(agent_id, duration_ms=duration))
        if name in p1_minted:
            tasks_journal.append(_memory_mint(agent_id, task_id, f"ep-{name}"))
        cost = p1_costs[name]
        boot.append(_infer_completed(agent_id, cost, cost + 1))

    for name in names:
        if name in skip_names_p2:
            continue
        task_id = f"{ledger_arm_label}-2-{name}-tid"
        agent_id = f"{ledger_arm_label}-2-{name}-agent"
        ledger_rows.append(_ledger_row(ledger_arm_label, 2, name, task_id))
        this_mode = p2_mode_by_task.get(name, p2_mode)
        refalsify: str | None
        if p2_refalsify is not None and name in p2_refalsify:
            refalsify = p2_refalsify[name]
        else:
            refalsify = "premise_held" if this_mode == "injected" else None
        episode_id = f"ep-{name}" if this_mode == "injected" else None
        tasks_journal.append(_memory_stamp(agent_id, task_id, this_mode, episode_id, refalsify=refalsify))
        if name not in p2_stepless:
            duration = (p2_wall_ms or {}).get(name, 1000)
            tasks_journal.append(_task_step_done(agent_id, duration_ms=duration))
        cost = p2_costs[name]
        boot.append(_infer_completed(agent_id, cost, cost + 1))

    _write_jsonl(ledger_path, ledger_rows)
    _write_jsonl(arm_dir / "journal" / "tasks.jsonl", tasks_journal)
    _write_jsonl(arm_dir / "journal" / "boot-0001.jsonl", boot)


def _build_fixture(
    tmp: Path,
    names: list[str],
    m_prime_p1_costs: dict[str, int],
    m_prime_p2_costs: dict[str, int],
    r_p1_costs: dict[str, int],
    r_p2_costs: dict[str, int],
    *,
    m_prime_p1_mode: str = "silent",
    m_prime_p2_mode: str = "injected",
    r_p1_mode: str = "silent",
    r_p2_mode: str = "injected",
    m_prime_p2_refalsify: dict[str, str | None] | None = None,
    r_p2_refalsify: dict[str, str | None] | None = None,
    m_prime_p1_wall_ms: dict[str, int] | None = None,
    m_prime_p2_wall_ms: dict[str, int] | None = None,
    r_p1_wall_ms: dict[str, int] | None = None,
    r_p2_wall_ms: dict[str, int] | None = None,
    m_prime_skip_p2: set[str] | None = None,
    r_skip_p2: set[str] | None = None,
    m_prime_minted: set[str] | None = None,
    r_minted: set[str] | None = None,
    m_prime_p2_mode_by_task: dict[str, str] | None = None,
    r_p2_mode_by_task: dict[str, str] | None = None,
    m_prime_p1_stepless: set[str] | None = None,
    m_prime_p2_stepless: set[str] | None = None,
    r_p1_stepless: set[str] | None = None,
    r_p2_stepless: set[str] | None = None,
    ledger_label_m_prime: str = "m_prime",
    ledger_label_r: str = "r",
    digest_m_prime: tuple[str | None, str | None] = ("digest-m-prime", "digest-m-prime"),
    digest_r: tuple[str | None, str | None] = ("digest-r", "digest-r"),
) -> dict[str, Path]:
    corpus_dir = tmp / "corpus"
    _write_manifest(corpus_dir, names)

    arm_m_prime_dir = tmp / "arm_m_prime"
    arm_r_dir = tmp / "arm_r"
    ledger_m_prime = tmp / "ledger_m_prime.jsonl"
    ledger_r = tmp / "ledger_r.jsonl"

    _write_arm(
        arm_m_prime_dir,
        ledger_m_prime,
        ledger_label_m_prime,
        digest_m_prime,
        names,
        m_prime_p1_costs,
        m_prime_p2_costs,
        p1_mode=m_prime_p1_mode,
        p2_mode=m_prime_p2_mode,
        p2_refalsify=m_prime_p2_refalsify,
        p1_wall_ms=m_prime_p1_wall_ms,
        p2_wall_ms=m_prime_p2_wall_ms,
        skip_names_p2=m_prime_skip_p2,
        p1_minted=m_prime_minted,
        p2_mode_by_task=m_prime_p2_mode_by_task,
        p1_stepless=m_prime_p1_stepless,
        p2_stepless=m_prime_p2_stepless,
    )
    _write_arm(
        arm_r_dir,
        ledger_r,
        ledger_label_r,
        digest_r,
        names,
        r_p1_costs,
        r_p2_costs,
        p1_mode=r_p1_mode,
        p2_mode=r_p2_mode,
        p2_refalsify=r_p2_refalsify,
        p1_wall_ms=r_p1_wall_ms,
        p2_wall_ms=r_p2_wall_ms,
        skip_names_p2=r_skip_p2,
        p1_minted=r_minted,
        p2_mode_by_task=r_p2_mode_by_task,
        p1_stepless=r_p1_stepless,
        p2_stepless=r_p2_stepless,
    )

    return {
        "corpus_dir": corpus_dir,
        "arm_m_prime_dir": arm_m_prime_dir,
        "arm_r_dir": arm_r_dir,
        "ledger_m_prime": ledger_m_prime,
        "ledger_r": ledger_r,
    }


CONSTANT_50 = {n: 50 for n in TASKS}


if __name__ == "__main__":
    unittest.main()
