"""Tests for `tools.memory_battery.recompute_s5` (s5-weight-battery-v1
design spec §5; plan Task 4).

Single-arm fixtures, hand-checkable arithmetic, label `s5_off`. The
Wilson vectors are INDEPENDENT hand-derivations of the score-interval
formula (recorded in the test, not computed by the module under test).
"""

from __future__ import annotations

import json
import unittest
from pathlib import Path
from tempfile import TemporaryDirectory
from typing import Any

from tools.memory_battery.recompute_s5 import (
    FLOOR_S5,
    main,
    recompute_s5,
    wilson_interval,
)

DIGEST = "d1"
# 4 tasks per lane keeps fixtures small; floor overridden to 2 in tests.
LANES = {
    "control": ["c0", "c1", "c2", "c3"],
    "moot": ["a0", "a1", "a2", "a3"],
    "stale": ["b0", "b1", "b2", "b3"],
}
ALL_NAMES = [name for names in LANES.values() for name in names]


def _write_jsonl(path: Path, rows: list[dict[str, Any]]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8") as handle:
        for row in rows:
            handle.write(json.dumps(row) + "\n")


def _write_manifest(corpus_dir: Path) -> None:
    tasks = []
    for lane, names in LANES.items():
        for name in names:
            tasks.append({"name": name, "lane": lane, "workspace_sha256": f"sha-{name}"})
    manifest = {
        "instrument": "s5-weight-battery-v1",
        "corpus_seed": 20260830,
        "n": len(tasks),
        "n_per_lane": 4,
        "families": {},
        "families_by_lane": {},
        "tasks": tasks,
    }
    corpus_dir.mkdir(parents=True, exist_ok=True)
    (corpus_dir / "manifest.json").write_text(json.dumps(manifest), encoding="utf-8")


def _write_arm(
    tmp: Path,
    *,
    p2_event_by_task: dict[str, str],
    p2_status_by_task: dict[str, str] | None = None,
    store_rows: list[dict[str, Any]] | None = None,
    extra_journal_rows: list[dict[str, Any]] | None = None,
    label: str = "s5_off",
) -> dict[str, Path]:
    """One arm: phase 1 silent+mint for every task; phase 2 injected for
    every task, with `p2_event_by_task[name]` in {"mint", "contradict",
    "neither"} controlling the after-run event and `p2_status_by_task`
    the ledger terminal status (default Done)."""
    corpus_dir = tmp / "corpus"
    _write_manifest(corpus_dir)
    arm_dir = tmp / "arm"
    ledger_path = tmp / "ledger.jsonl"
    p2_status_by_task = p2_status_by_task or {}

    ledger_rows: list[dict[str, Any]] = [
        {"arm": label, "phase": 1, "event": "identity", "digest": DIGEST, "ts": "t"},
        {"arm": label, "phase": 2, "event": "identity", "digest": DIGEST, "ts": "t"},
    ]
    tasks_journal: list[dict[str, Any]] = list(extra_journal_rows or [])
    boot: list[dict[str, Any]] = []

    for phase in (1, 2):
        for name in ALL_NAMES:
            task_id = f"{label}-{phase}-{name}-tid"
            agent_id = f"{label}-{phase}-{name}-agent"
            status = "Done" if phase == 1 else p2_status_by_task.get(name, "Done")
            ledger_rows.append(
                {
                    "arm": label, "phase": phase, "task": name, "agent_id": agent_id,
                    "task_id": task_id, "status": status, "wall_s": 1.0,
                    "suspend_ok": True, "ts": "t",
                }
            )
            mode = "silent" if phase == 1 else "injected"
            tasks_journal.append(
                {
                    "event": "MemoryStamp", "id": agent_id, "task_id": task_id,
                    "mode": mode, "episode_id": f"ep-{name}" if mode == "injected" else None,
                    "candidates_checked": 1 if phase == 2 else 0, "refalsify": None,
                }
            )
            tasks_journal.append(
                {
                    "event": "TaskStep", "id": agent_id, "step": 1, "verb": "done",
                    "outcome": "ok", "duration_ms": 1000, "args": [],
                }
            )
            if phase == 1:
                tasks_journal.append(
                    {"event": "MemoryMint", "id": agent_id, "task_id": task_id, "episode_id": f"ep-{name}"}
                )
            else:
                event = p2_event_by_task.get(name, "mint")
                if event == "mint":
                    tasks_journal.append(
                        {"event": "MemoryMint", "id": agent_id, "task_id": task_id, "episode_id": f"ep-{name}"}
                    )
                elif event == "contradict":
                    tasks_journal.append(
                        {"event": "MemoryContradicted", "id": agent_id, "task_id": task_id, "episode_id": f"ep-{name}"}
                    )
                # "neither": no after-run event row.
            boot.append(
                {
                    "event": "InferCompleted", "id": agent_id, "prompt_tokens": 101,
                    "completion_tokens": 100, "duration_ms": 500,
                }
            )

    _write_jsonl(ledger_path, ledger_rows)
    _write_jsonl(arm_dir / "journal" / "tasks.jsonl", tasks_journal)
    _write_jsonl(arm_dir / "journal" / "boot-0001.jsonl", boot)
    _write_jsonl(
        arm_dir / "memory" / "episodes.jsonl",
        store_rows if store_rows is not None else [
            {"episode_id": f"ep-{name}", "status": "verified"} for name in ALL_NAMES
        ],
    )
    return {"corpus": corpus_dir, "arm": arm_dir, "ledger": ledger_path}


def _run(paths: dict[str, Path], floor: int = 2, label: str = "s5_off") -> dict[str, Any]:
    return recompute_s5(
        paths["corpus"], paths["arm"], paths["ledger"],
        expected_digest=DIGEST, floor=floor, expected_arm_label=label,
    )


# The construction below mirrors the expected real shape: control mostly
# mints (one contradiction = collateral), moot mostly contradicts, stale
# splits between contradiction (removal) and mint (correction).
HAPPY_EVENTS = {
    "c0": "mint", "c1": "mint", "c2": "mint", "c3": "contradict",
    "a0": "contradict", "a1": "contradict", "a2": "contradict", "a3": "mint",
    "b0": "contradict", "b1": "contradict", "b2": "mint", "b3": "mint",
}


class WilsonTest(unittest.TestCase):
    def test_hand_derived_vectors(self):
        # Independent hand-derivation of the score interval (standard
        # closed form), pinned to 6 decimals.
        for k, n, lo, hi in (
            (47, 50, 0.837829, 0.979385),
            (0, 16, 0.0, 0.193608),
            (16, 16, 0.806392, 1.0),
            (8, 16, 0.279996, 0.720004),
        ):
            got_lo, got_hi = wilson_interval(k, n)
            self.assertAlmostEqual(got_lo, lo, places=6, msg=f"k={k} n={n}")
            self.assertAlmostEqual(got_hi, hi, places=6, msg=f"k={k} n={n}")


class RecomputeS5Test(unittest.TestCase):
    def test_happy_path_weights_and_validity(self):
        with TemporaryDirectory() as tmp:
            result = _run(_write_arm(Path(tmp), p2_event_by_task=HAPPY_EVENTS))
            self.assertEqual(result["v1_conformance"]["verdict"], "PASS", result["v1_conformance"])
            self.assertTrue(result["v2_stamp_audit"]["refalsify_all_none"])
            self.assertEqual(result["v3_floors"]["verdict"], "PASS")
            w = result["weights"]
            self.assertEqual(w["control"]["contradicted"], 1)
            self.assertEqual(w["control"]["minted"], 3)
            self.assertEqual(w["control"]["rate_contradicted"], 0.25)
            self.assertEqual(w["moot"]["contradicted"], 3)
            self.assertEqual(w["moot"]["rate_contradicted"], 0.75)
            self.assertEqual(w["stale"]["contradicted"], 2)
            self.assertEqual(w["stale"]["minted"], 2)
            self.assertEqual(w["stale"]["rate_minted"], 0.5)
            lo, hi = wilson_interval(3, 4)
            self.assertEqual(w["moot"]["wilson_contradicted"], [lo, hi])
            self.assertFalse(result["h3_infra"]["violated"])
            self.assertFalse(result["completeness"]["violated"])

    def test_double_event_is_invalid(self):
        with TemporaryDirectory() as tmp:
            extra = [
                {"event": "MemoryContradicted", "id": "s5_off-2-c0-agent",
                 "task_id": "s5_off-2-c0-tid", "episode_id": "ep-c0"}
            ]
            result = _run(_write_arm(Path(tmp), p2_event_by_task=HAPPY_EVENTS, extra_journal_rows=extra))
            self.assertEqual(result["v1_conformance"]["verdict"], "INVALID")
            self.assertIn("c0", str(result["v1_conformance"]["both_event_names"]))

    def test_neither_event_on_a_done_task_is_a_named_class(self):
        with TemporaryDirectory() as tmp:
            events = dict(HAPPY_EVENTS)
            events["c1"] = "neither"
            result = _run(_write_arm(Path(tmp), p2_event_by_task=events))
            self.assertEqual(result["v1_conformance"]["verdict"], "INVALID")
            self.assertIn("c1", str(result["v1_conformance"]["neither_event_names"]))

    def test_error_status_is_dropped_at_join_and_counted_once_by_h3(self):
        # `_load_arm` already drops Error-status task-halves into
        # `dropped` (recompute_join's design-spec-§4 H3 rule), so an
        # Error task never reaches the matched set -- the spec's "Error
        # excluded as unmeasured" lives THERE, and H3 must count it
        # exactly once (via dropped_halves, never a second time).
        with TemporaryDirectory() as tmp:
            events = dict(HAPPY_EVENTS)
            events["b0"] = "neither"  # Error task: episode stands, no event
            result = _run(
                _write_arm(Path(tmp), p2_event_by_task=events, p2_status_by_task={"b0": "Error"})
            )
            self.assertEqual(result["v1_conformance"]["verdict"], "PASS", result["v1_conformance"])
            self.assertEqual(result["weights"]["stale"]["matched"], 3)
            self.assertEqual(result["weights"]["stale"]["denominator"], 3)
            self.assertAlmostEqual(result["weights"]["stale"]["rate_contradicted"], 1 / 3)
            self.assertEqual(result["h3_infra"]["infra_count"], 1)
            self.assertEqual(len(result["dropped"]), 1)

    def test_scored_non_done_contradiction_counts_normally(self):
        with TemporaryDirectory() as tmp:
            result = _run(
                _write_arm(
                    Path(tmp),
                    p2_event_by_task=HAPPY_EVENTS,
                    p2_status_by_task={"a0": "StepsExhausted"},
                )
            )
            self.assertEqual(result["v1_conformance"]["verdict"], "PASS")
            self.assertEqual(result["weights"]["moot"]["contradicted"], 3)

    def test_non_none_refalsify_breaks_the_stamp_audit(self):
        with TemporaryDirectory() as tmp:
            paths = _write_arm(Path(tmp), p2_event_by_task=HAPPY_EVENTS)
            tasks_path = paths["arm"] / "journal" / "tasks.jsonl"
            rows = [json.loads(line) for line in tasks_path.read_text(encoding="utf-8").splitlines()]
            for row in rows:
                if row.get("event") == "MemoryStamp" and row["task_id"] == "s5_off-2-a0-tid":
                    row["refalsify"] = "premise_held"
            _write_jsonl(tasks_path, rows)
            result = _run(paths)
            self.assertFalse(result["v2_stamp_audit"]["refalsify_all_none"])

    def test_lane_floor_miss_is_unmeasurable_for_that_lane_only(self):
        with TemporaryDirectory() as tmp:
            result = _run(_write_arm(Path(tmp), p2_event_by_task=HAPPY_EVENTS), floor=5)
            self.assertEqual(result["v3_floors"]["verdict"], "UNMEASURABLE")
            self.assertEqual(
                sorted(result["v3_floors"]["lanes_under_floor"]), ["control", "moot", "stale"]
            )

    def test_default_floor_constant_is_the_locked_literal(self):
        self.assertEqual(FLOOR_S5, 8)


class CliFatalTest(unittest.TestCase):
    def test_digest_mismatch_exits_nonzero(self):
        with TemporaryDirectory() as tmp:
            paths = _write_arm(Path(tmp), p2_event_by_task=HAPPY_EVENTS)
            code = main(
                [
                    "--corpus-dir", str(paths["corpus"]),
                    "--arm-dir", str(paths["arm"]),
                    "--ledger", str(paths["ledger"]),
                    "--expected-digest", "not-the-served-digest",
                    "--floor", "2",
                ]
            )
            self.assertEqual(code, 1)


if __name__ == "__main__":
    unittest.main()
