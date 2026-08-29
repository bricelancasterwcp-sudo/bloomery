"""Tests for `tools.memory_battery.recompute_pg` (premise-gone-battery-v1
design spec §5; plan Task 6).

Fixture style mirrors `test_recompute_v2.py` (hand-built journals/ledgers,
honest `m_prime`/`r` labels) plus the two pg-only evidence sources: each
arm's `memory/episodes.jsonl` store file (full-record rows,
last-writer-wins by `episode_id`) and `Degraded` journal rows for the
oversize scan. Every fixture's arithmetic is hand-checkable.
"""

from __future__ import annotations

import json
import unittest
from pathlib import Path
from tempfile import TemporaryDirectory
from typing import Any

from tools.memory_battery.recompute_pg import (
    MATCHED_FLOOR_PG,
    SEED_PG,
    main,
    recompute_pg,
)

NAMES = ["t0", "t1", "t2", "t3", "t4", "t5"]
DIGEST = "d1"


def _write_jsonl(path: Path, rows: list[dict[str, Any]]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8") as handle:
        for row in rows:
            handle.write(json.dumps(row) + "\n")


def _write_manifest(corpus_dir: Path, names: list[str]) -> None:
    manifest = {
        "instrument": "premise-gone-battery-v1",
        "corpus_seed": 20260828,
        "n": len(names),
        "families": {},
        "tasks": [{"name": name, "workspace_sha256": f"sha-{name}"} for name in names],
    }
    corpus_dir.mkdir(parents=True, exist_ok=True)
    (corpus_dir / "manifest.json").write_text(json.dumps(manifest), encoding="utf-8")


def _write_arm_pg(
    arm_dir: Path,
    ledger_path: Path,
    label: str,
    names: list[str],
    *,
    p2_mode_by_task: dict[str, str],
    p2_refalsify_by_task: dict[str, str | None],
    store_rows: list[dict[str, Any]],
    extra_journal_rows: list[dict[str, Any]] | None = None,
    p1_cost: int = 100,
    p2_cost: int = 90,
) -> None:
    """One pg arm: phase 1 all silent/mint (the defective-start mint
    phase), phase 2 per-task mode + refalsify spelling. Costs uniform
    unless a test needs otherwise -- the token gates here are advisory,
    so the fixtures keep the arithmetic flat."""
    ledger_rows: list[dict[str, Any]] = [
        {"arm": label, "phase": 1, "event": "identity", "digest": DIGEST, "ts": "t"},
        {"arm": label, "phase": 2, "event": "identity", "digest": DIGEST, "ts": "t"},
    ]
    tasks_journal: list[dict[str, Any]] = list(extra_journal_rows or [])
    boot: list[dict[str, Any]] = []

    for phase in (1, 2):
        for name in names:
            task_id = f"{label}-{phase}-{name}-tid"
            agent_id = f"{label}-{phase}-{name}-agent"
            ledger_rows.append(
                {
                    "arm": label,
                    "phase": phase,
                    "task": name,
                    "agent_id": agent_id,
                    "task_id": task_id,
                    "status": "Done",
                    "wall_s": 1.0,
                    "suspend_ok": True,
                    "ts": "t",
                }
            )
            if phase == 1:
                mode, refalsify = "silent", None
            else:
                mode = p2_mode_by_task.get(name, "silent")
                refalsify = p2_refalsify_by_task.get(name)
            tasks_journal.append(
                {
                    "event": "MemoryStamp",
                    "id": agent_id,
                    "task_id": task_id,
                    "mode": mode,
                    "episode_id": f"ep-{name}" if mode == "injected" else None,
                    "candidates_checked": 1,
                    "refalsify": refalsify,
                }
            )
            tasks_journal.append(
                {
                    "event": "TaskStep",
                    "id": agent_id,
                    "step": 1,
                    "verb": "done",
                    "outcome": "ok",
                    "duration_ms": 1000,
                    "args": [],
                }
            )
            if phase == 1:
                tasks_journal.append(
                    {"event": "MemoryMint", "id": agent_id, "task_id": task_id, "episode_id": f"ep-{name}"}
                )
            cost = p1_cost if phase == 1 else p2_cost
            boot.append(
                {
                    "event": "InferCompleted",
                    "id": agent_id,
                    "prompt_tokens": cost + 1,
                    "completion_tokens": cost,
                    "duration_ms": 500,
                }
            )

    _write_jsonl(ledger_path, ledger_rows)
    _write_jsonl(arm_dir / "journal" / "tasks.jsonl", tasks_journal)
    _write_jsonl(arm_dir / "journal" / "boot-0001.jsonl", boot)
    _write_jsonl(arm_dir / "memory" / "episodes.jsonl", store_rows)


def _verified_store(names: list[str]) -> list[dict[str, Any]]:
    return [{"episode_id": f"ep-{name}", "status": "verified"} for name in names]


def _build(
    tmp: Path,
    *,
    r_p2_mode_by_task: dict[str, str] | None = None,
    r_p2_refalsify_by_task: dict[str, str | None] | None = None,
    m_p2_mode_by_task: dict[str, str] | None = None,
    r_store_rows: list[dict[str, Any]] | None = None,
    m_store_rows: list[dict[str, Any]] | None = None,
    r_extra_rows: list[dict[str, Any]] | None = None,
    m_extra_rows: list[dict[str, Any]] | None = None,
) -> dict[str, Path]:
    corpus_dir = tmp / "corpus"
    _write_manifest(corpus_dir, NAMES)
    paths = {
        "corpus": corpus_dir,
        "m_dir": tmp / "arm-m-prime",
        "r_dir": tmp / "arm-r",
        "m_ledger": tmp / "m.jsonl",
        "r_ledger": tmp / "r.jsonl",
    }
    # The pg happy path: M' injects the moot lesson on every match; R
    # probes every match to premise_gone and stays silent.
    _write_arm_pg(
        paths["m_dir"],
        paths["m_ledger"],
        "m_prime",
        NAMES,
        p2_mode_by_task=m_p2_mode_by_task or {name: "injected" for name in NAMES},
        p2_refalsify_by_task={name: None for name in NAMES},
        store_rows=m_store_rows if m_store_rows is not None else _verified_store(NAMES),
        extra_journal_rows=m_extra_rows,
    )
    _write_arm_pg(
        paths["r_dir"],
        paths["r_ledger"],
        "r",
        NAMES,
        p2_mode_by_task=r_p2_mode_by_task or {name: "silent" for name in NAMES},
        p2_refalsify_by_task=r_p2_refalsify_by_task or {name: "premise_gone" for name in NAMES},
        store_rows=r_store_rows if r_store_rows is not None else _verified_store(NAMES),
        extra_journal_rows=r_extra_rows,
    )
    return paths


def _run(paths: dict[str, Path], floor: int = 3) -> dict[str, Any]:
    return recompute_pg(
        paths["corpus"],
        paths["m_dir"],
        paths["r_dir"],
        paths["m_ledger"],
        paths["r_ledger"],
        expected_digest=DIGEST,
        floor=floor,
    )


class RecomputePgTest(unittest.TestCase):
    def test_happy_path_passes_every_gate(self):
        with TemporaryDirectory() as tmp:
            result = _run(_build(Path(tmp)))
            self.assertEqual(result["pg1"]["verdict"], "PASS", result["pg1"])
            self.assertEqual(result["pg1"]["premise_gone_count"], len(NAMES))
            self.assertEqual(result["pg2"]["verdict"], "PASS", result["pg2"])
            self.assertEqual(result["pg3"]["verdict"], "PASS", result["pg3"])
            self.assertEqual(result["floor"]["verdict"], "PASS", result["floor"])
            self.assertEqual(result["floor"]["matched_r_p2"], len(NAMES))
            self.assertEqual(result["floor"]["injected_m_prime_p2"], len(NAMES))
            self.assertTrue(result["stamp_audit"]["forbidden_spellings_absent"])
            self.assertTrue(result["stamp_audit"]["m_prime_refalsify_all_none"])
            self.assertFalse(result["h2_p1_equivalence"]["violated"])
            self.assertFalse(result["h3_infra"]["violated"])
            self.assertEqual(result["a2_aftermath"]["m_prime"]["memory_contradicted_count"], 0)

    def test_premise_held_in_r_p2_is_an_alarm(self):
        with TemporaryDirectory() as tmp:
            paths = _build(
                Path(tmp),
                r_p2_mode_by_task={**{n: "silent" for n in NAMES}, "t2": "injected"},
                r_p2_refalsify_by_task={**{n: "premise_gone" for n in NAMES}, "t2": "premise_held"},
            )
            result = _run(paths)
            self.assertEqual(result["pg1"]["verdict"], "ALARM")
            self.assertIn("t2", str(result["pg1"]["premise_held_names"]))

    def test_uninstrumented_injection_in_r_p2_fails_pg1(self):
        with TemporaryDirectory() as tmp:
            paths = _build(
                Path(tmp),
                r_p2_mode_by_task={**{n: "silent" for n in NAMES}, "t3": "injected"},
                r_p2_refalsify_by_task={**{n: "premise_gone" for n in NAMES}, "t3": None},
            )
            result = _run(paths)
            self.assertEqual(result["pg1"]["verdict"], "FAIL")

    def test_skipped_ungranted_in_r_p2_is_invalid(self):
        with TemporaryDirectory() as tmp:
            paths = _build(
                Path(tmp),
                r_p2_mode_by_task={**{n: "silent" for n in NAMES}, "t1": "injected"},
                r_p2_refalsify_by_task={**{n: "premise_gone" for n in NAMES}, "t1": "skipped_ungranted"},
            )
            result = _run(paths)
            self.assertEqual(result["pg1"]["verdict"], "INVALID")

    def test_inconclusive_in_r_p2_is_infra_not_a_pg1_failure(self):
        with TemporaryDirectory() as tmp:
            paths = _build(
                Path(tmp),
                r_p2_mode_by_task={**{n: "silent" for n in NAMES}, "t4": "injected"},
                r_p2_refalsify_by_task={**{n: "premise_gone" for n in NAMES}, "t4": "inconclusive"},
            )
            result = _run(paths)
            self.assertEqual(result["pg1"]["verdict"], "PASS", result["pg1"])
            self.assertEqual(result["pg1"]["inconclusive_names"], ["t4"])
            self.assertEqual(result["h3_infra"]["r_infra_count"], 1)

    def test_contradicted_episode_in_r_store_fails_pg2(self):
        with TemporaryDirectory() as tmp:
            store = _verified_store(NAMES) + [{"episode_id": "ep-t0", "status": "contradicted"}]
            result = _run(_build(Path(tmp), r_store_rows=store))
            self.assertEqual(result["pg2"]["verdict"], "FAIL")
            self.assertIn("ep-t0", result["pg2"]["non_verified_episode_ids"])

    def test_memory_contradicted_event_in_r_journal_fails_pg2(self):
        with TemporaryDirectory() as tmp:
            contradiction = [
                {"event": "MemoryContradicted", "id": "r-2-t0-agent", "task_id": "r-2-t0-tid", "episode_id": "ep-t0"}
            ]
            result = _run(_build(Path(tmp), r_extra_rows=contradiction))
            self.assertEqual(result["pg2"]["verdict"], "FAIL")
            self.assertEqual(result["pg2"]["memory_contradicted_count"], 1)

    def test_injected_below_floor_is_unmeasurable(self):
        with TemporaryDirectory() as tmp:
            modes = {name: "injected" for name in NAMES}
            modes["t5"] = "silent"
            result = _run(_build(Path(tmp), m_p2_mode_by_task=modes), floor=6)
            self.assertEqual(result["floor"]["verdict"], "UNMEASURABLE")
            self.assertEqual(result["pg3"]["verdict"], "UNMEASURABLE")

    def test_floor_is_inclusive_at_the_boundary(self):
        with TemporaryDirectory() as tmp:
            result = _run(_build(Path(tmp)), floor=len(NAMES))
            self.assertEqual(result["floor"]["verdict"], "PASS")
            self.assertEqual(result["pg3"]["verdict"], "PASS")

    def test_oversize_degraded_row_in_m_prime_is_a_pg3_alarm(self):
        with TemporaryDirectory() as tmp:
            degrade = [
                {
                    "event": "Degraded",
                    "reason": "memory organ: episode ep-t0 rendered 99999 bytes, over the "
                    "16384-byte injection bound; task x runs memory-off",
                }
            ]
            result = _run(_build(Path(tmp), m_extra_rows=degrade))
            self.assertEqual(result["pg3"]["verdict"], "ALARM")
            self.assertEqual(result["pg3"]["oversize_degraded_count"], 1)

    def test_non_none_refalsify_in_m_prime_breaks_the_stamp_audit(self):
        with TemporaryDirectory() as tmp:
            paths = _build(Path(tmp))
            # Doctor one M' p2 stamp to carry a spelling -- flag-off truth
            # violated (the served build is not the locked build).
            tasks_path = paths["m_dir"] / "journal" / "tasks.jsonl"
            rows = [json.loads(line) for line in tasks_path.read_text(encoding="utf-8").splitlines()]
            for row in rows:
                if row.get("event") == "MemoryStamp" and row["task_id"] == "m_prime-2-t0-tid":
                    row["refalsify"] = "premise_gone"
            _write_jsonl(tasks_path, rows)
            result = _run(paths)
            self.assertFalse(result["stamp_audit"]["m_prime_refalsify_all_none"])

    def test_seed_constant_is_the_locked_literal(self):
        self.assertEqual(SEED_PG, 20260829)
        self.assertEqual(MATCHED_FLOOR_PG, 25)


class CliFatalTest(unittest.TestCase):
    def test_digest_mismatch_exits_nonzero(self):
        with TemporaryDirectory() as tmp:
            paths = _build(Path(tmp))
            code = main(
                [
                    "--corpus-dir", str(paths["corpus"]),
                    "--arm-m-prime-dir", str(paths["m_dir"]),
                    "--arm-r-dir", str(paths["r_dir"]),
                    "--ledger-m-prime", str(paths["m_ledger"]),
                    "--ledger-r", str(paths["r_ledger"]),
                    "--expected-digest", "not-the-served-digest",
                    "--floor", "3",
                ]
            )
            self.assertEqual(code, 1)


if __name__ == "__main__":
    unittest.main()
