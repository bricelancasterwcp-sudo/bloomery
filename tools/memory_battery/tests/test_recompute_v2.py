"""Tests for `tools.memory_battery.recompute_v2` (design spec
`docs/superpowers/specs/2026-08-28-refalsify-battery-v2-design.md` §4/§5;
task-1 brief `.superpowers/sdd/2026-08-28-refalsify-battery-v2/task-1-brief.md`).

Fixture style mirrors `test_recompute.py` (hand-built journals/ledgers, no
`corpus.py`/`driver.py` machinery) but with HONEST v2 arm labels
(`m_prime`/`r`) everywhere -- never `C`/`M` (design spec §5: "battery-v1's
`c_/m_` slot names must not be reused for different semantics").

Every fixture's arithmetic is hand-checkable: costs, wall_ms, and refalsify
spellings are supplied as explicit per-task dicts and asserted exactly.
"""

from __future__ import annotations

import json
import unittest
from pathlib import Path
from tempfile import TemporaryDirectory
from typing import Any

from tools.memory_battery.recompute_v2 import B_V2, SEED_V2, recompute_v2

TASKS = ["t0", "t1", "t2", "t3", "t4", "t5"]


# ---------------------------------------------------------------------------
# Shared fixture-writing helpers (mirrors test_recompute.py's helper shapes,
# re-implemented locally so this file stays self-contained).
# ---------------------------------------------------------------------------


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


# ---------------------------------------------------------------------------
# Arithmetic, ITT, dropped, none-vs-zero, wall arithmetic.
# ---------------------------------------------------------------------------


class ArithmeticFixtureTests(unittest.TestCase):
    """Known arithmetic: exact costs/wall per task, one dropped R-p2 task
    (no ledger row -- none-vs-zero), mint rows, exact a1_wall medians."""

    M_PRIME_P1 = {"t0": 40, "t1": 42, "t2": 44, "t3": 46, "t4": 48, "t5": 50}
    M_PRIME_P2 = {"t0": 60, "t1": 62, "t2": 64, "t3": 66, "t4": 68, "t5": 70}
    R_P1 = {"t0": 41, "t1": 43, "t2": 45, "t3": 47, "t4": 49, "t5": 51}
    R_P2 = {"t0": 61, "t1": 63, "t2": 65, "t3": 67, "t4": 69}  # t5 dropped, no ledger row

    M_PRIME_P1_WALL = {"t0": 100, "t1": 200, "t2": 300, "t3": 400, "t4": 500, "t5": 600}
    M_PRIME_P2_WALL = {"t0": 1000, "t1": 1100, "t2": 1200, "t3": 1300, "t4": 1400, "t5": 1500}
    R_P1_WALL = {"t0": 150, "t1": 250, "t2": 350, "t3": 450, "t4": 550, "t5": 650}
    R_P2_WALL = {"t0": 1050, "t1": 1150, "t2": 1250, "t3": 1350, "t4": 1450}

    def setUp(self) -> None:
        self._tmp_ctx = TemporaryDirectory()
        self.tmp = Path(self._tmp_ctx.name)
        self.addCleanup(self._tmp_ctx.cleanup)
        self.paths = _build_fixture(
            self.tmp,
            TASKS,
            self.M_PRIME_P1,
            self.M_PRIME_P2,
            self.R_P1,
            self.R_P2,
            m_prime_p1_wall_ms=self.M_PRIME_P1_WALL,
            m_prime_p2_wall_ms=self.M_PRIME_P2_WALL,
            r_p1_wall_ms=self.R_P1_WALL,
            r_p2_wall_ms=self.R_P2_WALL,
            r_skip_p2={"t5"},
            m_prime_minted={"t0", "t1", "t2"},
            r_minted={"t0", "t3"},
        )
        self.result = recompute_v2(**self.paths)

    def test_dropped_task_is_absent_never_a_phantom_zero(self) -> None:
        dropped_r = self.result["dropped"]["r"]
        self.assertEqual(len(dropped_r), 1)
        self.assertEqual(dropped_r[0]["task"], "t5")
        self.assertEqual(dropped_r[0]["phase"], 2)
        self.assertTrue(dropped_r[0]["infra"])
        self.assertIn("no ledger row", dropped_r[0]["reason"])
        self.assertEqual(self.result["dropped"]["m_prime"], [])
        # t5 must be ABSENT from G1's r,p2 cost count, never present as 0.
        self.assertEqual(self.result["g1"]["n_r_p2"], 5)
        self.assertEqual(self.result["g1"]["n_m_prime_p2"], 6)

    def test_g1_medians_exact(self) -> None:
        g1 = self.result["g1"]
        # sorted M' p2: [60,62,64,66,68,70] -> (64+66)/2 = 65.0
        self.assertEqual(g1["median_m_prime_p2"], 65.0)
        # sorted R p2 (t5 dropped): [61,63,65,67,69] -> 65
        self.assertEqual(g1["median_r_p2"], 65.0)
        self.assertEqual(g1["diff"], 0.0)

    def test_a1_wall_p2_and_p1_control_medians_exact(self) -> None:
        a1 = self.result["a1_wall"]
        # M' p2 wall sorted: [1000,1100,1200,1300,1400,1500] -> 1250.0
        self.assertEqual(a1["p2"]["median_m_prime"], 1250.0)
        # R p2 wall (t5 dropped) sorted: [1050,1150,1250,1350,1450] -> 1250
        self.assertEqual(a1["p2"]["median_r"], 1250)
        self.assertEqual(a1["p2"]["delta"], 0.0)
        # M' p1 wall sorted: [100,200,300,400,500,600] -> 350.0
        self.assertEqual(a1["p1_control"]["median_m_prime"], 350.0)
        # R p1 wall sorted: [150,250,350,450,550,650] -> 400.0
        self.assertEqual(a1["p1_control"]["median_r"], 400.0)
        self.assertEqual(a1["p1_control"]["delta"], 50.0)

    def test_a1_per_task_wall_delta_exact(self) -> None:
        by_task = {e["task"]: e["delta"] for e in self.result["a1_wall"]["per_task_wall_delta_p2"]["per_task"]}
        self.assertEqual(
            by_task,
            {"t0": 50, "t1": 50, "t2": 50, "t3": 50, "t4": 50},
        )
        self.assertNotIn("t5", by_task)
        self.assertEqual(self.result["a1_wall"]["per_task_wall_delta_p2"]["median"], 50.0)
        self.assertEqual(self.result["a1_wall"]["per_task_wall_delta_p2"]["n"], 5)

    def test_a1_probed_retrieval_count_and_per_probed_ms(self) -> None:
        a1 = self.result["a1_wall"]
        # R p2: all 5 non-dropped tasks are mode "injected" -> refalsify
        # "premise_held" (auto-derived) -> all 5 are probed retrievals.
        self.assertEqual(a1["probed_retrieval_count"], 5)
        self.assertEqual(a1["per_probed_retrieval_ms"], a1["p2"]["delta"] / 5)

    def test_h4_advisory_mint_and_retrieval_rates_per_arm(self) -> None:
        h4 = self.result["h4_advisory"]
        self.assertEqual(h4["m_prime"]["mint_count_p1"], 3)
        self.assertEqual(h4["m_prime"]["mint_rate_p1"], 3 / 6)
        self.assertEqual(h4["r"]["mint_count_p1"], 2)
        self.assertEqual(h4["r"]["mint_rate_p1"], 2 / 6)
        # retrieval (injection) rate p2 uses manifest n=6 as denominator (ITT).
        self.assertEqual(h4["m_prime"]["retrieval_count_p2"], 6)
        self.assertEqual(h4["m_prime"]["retrieval_rate_p2"], 6 / 6)
        self.assertEqual(h4["r"]["retrieval_count_p2"], 5)
        self.assertEqual(h4["r"]["retrieval_rate_p2"], 5 / 6)

    def test_corpus_sha_and_lens(self) -> None:
        self.assertEqual(len(self.result["corpus_sha"]), 64)
        lens = self.result["lens"]
        self.assertEqual(lens["seed"], SEED_V2)
        self.assertEqual(lens["b"], B_V2)
        self.assertEqual(lens["arm_labels"], {"m_prime": "m_prime", "r": "r"})
        self.assertEqual(lens["n"], 6)
        self.assertIn("source_paths", lens)

    def test_wall_unmeasured_count_all_zero_when_every_task_has_steps(self) -> None:
        # Every task-half in this fixture writes a normal TaskStep row --
        # the none-vs-zero exclusion counter must read 0 everywhere, and
        # (per the arithmetic tests above) every median is unaffected.
        counts = self.result["a1_wall"]["wall_unmeasured_count"]
        self.assertEqual(counts, {"m_prime": {1: 0, 2: 0}, "r": {1: 0, 2: 0}})


# ---------------------------------------------------------------------------
# A1 wall none-vs-zero (fix 1): a stepless-but-conducted task's wall_ms must
# be EXCLUDED from every A1 median/delta/per-task computation, never a
# silent phantom 0 -- the named bug class "a value that looks like a
# measurement but is not".
# ---------------------------------------------------------------------------


class A1WallNoneVsZeroTests(unittest.TestCase):
    """Fixture: m_prime's phase-2 task "s0" joins normally (ledger row +
    MemoryStamp + InferCompleted, all present) but writes NO `TaskStep` row
    at all -- "stepless but conducted". Every other task-half (both arms,
    both phases) has a normal TaskStep row. Hand-derived expectations below
    treat s0's m_prime-p2 wall as ABSENT, never as 0."""

    NAMES = ["s0", "s1", "s2", "s3"]
    COSTS = {n: 50 for n in NAMES}
    M_PRIME_P2_WALL = {"s0": 1000, "s1": 1100, "s2": 1200, "s3": 1300}
    R_P2_WALL = {"s0": 2000, "s1": 2100, "s2": 2200, "s3": 2300}

    def setUp(self) -> None:
        self._tmp_ctx = TemporaryDirectory()
        self.tmp = Path(self._tmp_ctx.name)
        self.addCleanup(self._tmp_ctx.cleanup)
        self.paths = _build_fixture(
            self.tmp,
            self.NAMES,
            self.COSTS,
            self.COSTS,
            self.COSTS,
            self.COSTS,
            m_prime_p2_wall_ms=self.M_PRIME_P2_WALL,
            r_p2_wall_ms=self.R_P2_WALL,
            m_prime_p2_stepless={"s0"},
        )
        self.result = recompute_v2(**self.paths)

    def test_stepless_task_excluded_from_p2_medians(self) -> None:
        a1 = self.result["a1_wall"]
        # m_prime p2 measured walls: [1100, 1200, 1300] (s0 excluded, NOT a
        # phantom 0 in the list) -> median 1200, hand-derived over the
        # remaining tasks only.
        self.assertEqual(a1["p2"]["median_m_prime"], 1200)
        # r p2 walls (no stepless task there): [2000, 2100, 2200, 2300] -> 2150.0
        self.assertEqual(a1["p2"]["median_r"], 2150.0)
        self.assertEqual(a1["p2"]["delta"], 950.0)

    def test_wall_unmeasured_count_names_the_one_exclusion(self) -> None:
        counts = self.result["a1_wall"]["wall_unmeasured_count"]
        self.assertEqual(counts["m_prime"][2], 1)
        self.assertEqual(counts["m_prime"][1], 0)
        self.assertEqual(counts["r"][1], 0)
        self.assertEqual(counts["r"][2], 0)

    def test_per_task_wall_delta_excludes_stepless_task(self) -> None:
        per_task = self.result["a1_wall"]["per_task_wall_delta_p2"]
        by_task = {entry["task"]: entry["delta"] for entry in per_task["per_task"]}
        self.assertNotIn("s0", by_task)
        self.assertEqual(by_task, {"s1": 1000, "s2": 1000, "s3": 1000})
        self.assertEqual(per_task["n"], 3)
        self.assertEqual(per_task["median"], 1000)
        self.assertEqual(per_task["min"], 1000)
        self.assertEqual(per_task["max"], 1000)


# ---------------------------------------------------------------------------
# G1 verdicts.
# ---------------------------------------------------------------------------


class G1VerdictTests(unittest.TestCase):
    def test_g1_pass_within_band(self) -> None:
        # Golden fixture (see GoldenBootstrapV2Tests): diff=1.0 inside its
        # own band, headroom well above band -> real PASS.
        with TemporaryDirectory() as tmp_str:
            tmp = Path(tmp_str)
            names = [f"g{i}" for i in range(8)]
            m_prime_p1 = {n: v for n, v in zip(names, [40, 42, 44, 46, 48, 50, 52, 54])}
            r_p1 = {n: v for n, v in zip(names, [41, 43, 45, 47, 49, 51, 53, 55])}
            m_prime_p2 = {n: v for n, v in zip(names, [30, 58, 60, 62, 64, 66, 68, 95])}
            r_p2 = {n: v for n, v in zip(names, [32, 59, 61, 63, 65, 67, 69, 90])}
            paths = _build_fixture(tmp, names, m_prime_p1, m_prime_p2, r_p1, r_p2)
            result = recompute_v2(**paths)
            self.assertEqual(result["g1"]["verdict"], "PASS")
            self.assertIsNone(result["g1"]["reason"])

    def test_g1_fail_outside_band(self) -> None:
        with TemporaryDirectory() as tmp_str:
            tmp = Path(tmp_str)
            m_prime_p2 = dict(CONSTANT_50)
            r_p2 = {n: 400 for n in TASKS}
            paths = _build_fixture(tmp, TASKS, CONSTANT_50, m_prime_p2, CONSTANT_50, r_p2)
            result = recompute_v2(**paths)
            g1 = result["g1"]
            self.assertEqual(g1["verdict"], "FAIL")
            self.assertEqual(g1["se_boot"], 0.0)
            self.assertEqual(g1["band"], 0.0)
            self.assertEqual(g1["diff"], 350.0)
            self.assertIsNotNone(g1["reason"])

    def test_g1_unmeasurable_floor_saturated(self) -> None:
        # Hand-derived (see module docstring's judgment-call note): headroom
        # (median - min) on BOTH arms sits under the bootstrap band.
        with TemporaryDirectory() as tmp_str:
            tmp = Path(tmp_str)
            m_prime_p1 = {"t0": 40, "t1": 45, "t2": 50, "t3": 55, "t4": 60, "t5": 65}
            r_p1 = {"t0": 41, "t1": 44, "t2": 52, "t3": 53, "t4": 62, "t5": 63}
            m_prime_p2 = {"t0": 50, "t1": 55, "t2": 58, "t3": 61, "t4": 70, "t5": 90}
            r_p2 = {"t0": 52, "t1": 54, "t2": 60, "t3": 63, "t4": 65, "t5": 68}
            paths = _build_fixture(tmp, TASKS, m_prime_p1, m_prime_p2, r_p1, r_p2)
            result = recompute_v2(**paths)
            g1 = result["g1"]
            self.assertEqual(g1["verdict"], "UNMEASURABLE")
            self.assertEqual(g1["headroom_m_prime"], 9.5)
            self.assertEqual(g1["headroom_r"], 9.5)
            self.assertLess(g1["headroom_m_prime"], g1["band"])
            self.assertIn("floor-saturat", g1["reason"])


# ---------------------------------------------------------------------------
# G2 verdicts.
# ---------------------------------------------------------------------------


class G2VerdictTests(unittest.TestCase):
    """Reuses `_build_fixture`'s per-task `*_p2_mode_by_task` override
    (uniform `p2_mode="injected"` everywhere except the one task named)."""

    def _run(self, m_prime_p2_mode_by_task: dict[str, str], r_p2_mode_by_task: dict[str, str]) -> dict[str, Any]:
        with TemporaryDirectory() as tmp_str:
            tmp = Path(tmp_str)
            paths = _build_fixture(
                tmp,
                TASKS,
                CONSTANT_50,
                CONSTANT_50,
                CONSTANT_50,
                CONSTANT_50,
                m_prime_p2_mode_by_task=m_prime_p2_mode_by_task,
                r_p2_mode_by_task=r_p2_mode_by_task,
            )
            return recompute_v2(**paths)

    def test_g2_pass_equal_counts(self) -> None:
        result = self._run({}, {})
        g2 = result["g2"]
        self.assertEqual(g2["injected_count_m_prime"], 6)
        self.assertEqual(g2["injected_count_r"], 6)
        self.assertEqual(g2["verdict"], "PASS")

    def test_g2_fail_deficit(self) -> None:
        # One fewer injection in R than M' -> deficit.
        result = self._run({}, {"t0": "silent"})
        g2 = result["g2"]
        self.assertEqual(g2["injected_count_m_prime"], 6)
        self.assertEqual(g2["injected_count_r"], 5)
        self.assertEqual(g2["verdict"], "FAIL")
        self.assertIn("deficit", g2["reason"])

    def test_g2_alarm_excess(self) -> None:
        # R has strictly MORE injections than M' -> impossible-by-construction alarm.
        result = self._run({"t0": "silent"}, {})
        g2 = result["g2"]
        self.assertEqual(g2["injected_count_m_prime"], 5)
        self.assertEqual(g2["injected_count_r"], 6)
        self.assertEqual(g2["verdict"], "ALARM")
        self.assertIn("excess", g2["reason"])


# ---------------------------------------------------------------------------
# Stamp audit.
# ---------------------------------------------------------------------------


class StampAuditTests(unittest.TestCase):
    def test_all_premise_held_is_complete(self) -> None:
        with TemporaryDirectory() as tmp_str:
            tmp = Path(tmp_str)
            paths = _build_fixture(tmp, TASKS, CONSTANT_50, CONSTANT_50, CONSTANT_50, CONSTANT_50)
            result = recompute_v2(**paths)
            audit = result["stamp_audit"]
            self.assertTrue(audit["premise_held_complete"])
            self.assertTrue(audit["forbidden_spellings_absent"])
            self.assertTrue(audit["premise_gone_zero"])
            self.assertEqual(audit["offending_premise_held"], [])
            self.assertEqual(audit["counts"]["r"][2]["premise_held"], 6)

    def test_one_failed_spelling_marks_forbidden_present(self) -> None:
        with TemporaryDirectory() as tmp_str:
            tmp = Path(tmp_str)
            paths = _build_fixture(
                tmp,
                TASKS,
                CONSTANT_50,
                CONSTANT_50,
                CONSTANT_50,
                CONSTANT_50,
                r_p2_refalsify={"t0": "failed"},
            )
            result = recompute_v2(**paths)
            audit = result["stamp_audit"]
            self.assertFalse(audit["forbidden_spellings_absent"])
            self.assertEqual(len(audit["forbidden_spelling_hits"]), 1)
            self.assertEqual(audit["forbidden_spelling_hits"][0]["refalsify"], "failed")
            self.assertEqual(audit["forbidden_spelling_hits"][0]["arm"], "r")

    def test_one_premise_gone_marks_alarm(self) -> None:
        with TemporaryDirectory() as tmp_str:
            tmp = Path(tmp_str)
            paths = _build_fixture(
                tmp,
                TASKS,
                CONSTANT_50,
                CONSTANT_50,
                CONSTANT_50,
                CONSTANT_50,
                r_p2_refalsify={"t0": "premise_gone"},
            )
            result = recompute_v2(**paths)
            audit = result["stamp_audit"]
            self.assertFalse(audit["premise_gone_zero"])
            self.assertEqual(len(audit["premise_gone_hits"]), 1)
            self.assertEqual(audit["premise_gone_hits"][0]["task"], "t0")
            # t0's injected R-p2 stamp no longer carries premise_held either.
            self.assertFalse(audit["premise_held_complete"])
            # Spelling counter (mutation check #3 target): the ONE premise_gone
            # tally must land under its own key, never folded into
            # premise_held's count -- t0 is premise_gone, t1-t5 are
            # premise_held, so counts must read exactly 1 and 5, not 0 and 6.
            self.assertEqual(audit["counts"]["r"][2].get("premise_gone", 0), 1)
            self.assertEqual(audit["counts"]["r"][2].get("premise_held", 0), 5)

    def test_inconclusive_and_skipped_ungranted_are_tolerated_and_counted(self) -> None:
        """Review finding IMPORTANT-2: one R-p2 task stamps refalsify
        'inconclusive' (mode injected) and one stamps 'skipped_ungranted'
        (mode injected). Spec §4's own wording: these are "tolerated ...
        counted and named individually" -- NOT `premise_held_complete`
        violations, and (being neither `passed`/`failed`) never trip the
        forbidden-spellings verdict either."""
        with TemporaryDirectory() as tmp_str:
            tmp = Path(tmp_str)
            paths = _build_fixture(
                tmp,
                TASKS,
                CONSTANT_50,
                CONSTANT_50,
                CONSTANT_50,
                CONSTANT_50,
                r_p2_refalsify={"t0": "inconclusive", "t1": "skipped_ungranted"},
            )
            result = recompute_v2(**paths)
            audit = result["stamp_audit"]
            self.assertEqual(audit["counts"]["r"][2].get("inconclusive", 0), 1)
            self.assertEqual(audit["counts"]["r"][2].get("skipped_ungranted", 0), 1)
            self.assertEqual(audit["inconclusive_count"], 1)
            self.assertEqual(audit["skipped_ungranted_count"], 1)
            # Tolerated, not offending: premise_held_complete stays True and
            # neither task appears in offending_premise_held.
            self.assertTrue(audit["premise_held_complete"])
            self.assertEqual(audit["offending_premise_held"], [])
            # Neither spelling is a forbidden v1 spelling.
            self.assertTrue(audit["forbidden_spellings_absent"])
            self.assertEqual(audit["forbidden_spelling_hits"], [])


# ---------------------------------------------------------------------------
# H2 first-exposure equivalence.
# ---------------------------------------------------------------------------


class H2EquivalenceTests(unittest.TestCase):
    def test_h2_not_violated_when_p1_costs_close(self) -> None:
        with TemporaryDirectory() as tmp_str:
            tmp = Path(tmp_str)
            paths = _build_fixture(tmp, TASKS, CONSTANT_50, CONSTANT_50, CONSTANT_50, CONSTANT_50)
            result = recompute_v2(**paths)
            h2 = result["h2_p1_equivalence"]
            self.assertFalse(h2["violated"])
            self.assertEqual(h2["diff"], 0.0)

    def test_h2_violated_when_p1_costs_diverge(self) -> None:
        with TemporaryDirectory() as tmp_str:
            tmp = Path(tmp_str)
            r_p1 = {n: 500 for n in TASKS}
            paths = _build_fixture(tmp, TASKS, CONSTANT_50, CONSTANT_50, r_p1, CONSTANT_50)
            result = recompute_v2(**paths)
            h2 = result["h2_p1_equivalence"]
            self.assertTrue(h2["violated"])
            self.assertIsNotNone(h2["reason"])


# ---------------------------------------------------------------------------
# H3 infra rate.
# ---------------------------------------------------------------------------


class H3InfraTests(unittest.TestCase):
    def test_h3_not_violated_within_ceiling(self) -> None:
        with TemporaryDirectory() as tmp_str:
            tmp = Path(tmp_str)
            paths = _build_fixture(tmp, TASKS, CONSTANT_50, CONSTANT_50, CONSTANT_50, CONSTANT_50)
            result = recompute_v2(**paths)
            h3 = result["h3_infra"]
            self.assertFalse(h3["violated"])
            self.assertEqual(h3["m_prime_infra_count"], 0)
            self.assertEqual(h3["r_infra_count"], 0)

    def test_h3_violated_above_ceiling(self) -> None:
        with TemporaryDirectory() as tmp_str:
            tmp = Path(tmp_str)
            # 12 task-halves per arm; drop 1 in R (>5% ceiling: 1/12=8.33%).
            paths = _build_fixture(
                tmp, TASKS, CONSTANT_50, CONSTANT_50, CONSTANT_50, CONSTANT_50, r_skip_p2={"t0"}
            )
            result = recompute_v2(**paths)
            h3 = result["h3_infra"]
            self.assertTrue(h3["violated"])
            self.assertEqual(h3["r_infra_count"], 1)
            self.assertAlmostEqual(h3["r_infra_rate"], 1 / 12)


# ---------------------------------------------------------------------------
# Arm-label honesty.
# ---------------------------------------------------------------------------


class ArmLabelHonestyTests(unittest.TestCase):
    def test_default_labels_round_trip_cleanly(self) -> None:
        with TemporaryDirectory() as tmp_str:
            tmp = Path(tmp_str)
            paths = _build_fixture(tmp, TASKS, CONSTANT_50, CONSTANT_50, CONSTANT_50, CONSTANT_50)
            result = recompute_v2(**paths)
            self.assertEqual(result["lens"]["arm_labels"], {"m_prime": "m_prime", "r": "r"})

    def test_ledger_labeled_c_or_m_is_rejected(self) -> None:
        with TemporaryDirectory() as tmp_str:
            tmp = Path(tmp_str)
            paths = _build_fixture(
                tmp,
                TASKS,
                CONSTANT_50,
                CONSTANT_50,
                CONSTANT_50,
                CONSTANT_50,
                ledger_label_m_prime="C",
                ledger_label_r="M",
            )
            with self.assertRaises(ValueError) as ctx:
                recompute_v2(**paths)
            self.assertIn("forbidden", str(ctx.exception).lower())

    def test_c_or_m_rejected_even_when_expected_arm_labels_overridden(self) -> None:
        """The reject is UNCONDITIONAL: even a caller who (mis)configures
        `expected_arm_labels=("C", "M")` still gets rejected -- v1's labels
        are never valid, regardless of what the caller expects."""
        with TemporaryDirectory() as tmp_str:
            tmp = Path(tmp_str)
            paths = _build_fixture(
                tmp,
                TASKS,
                CONSTANT_50,
                CONSTANT_50,
                CONSTANT_50,
                CONSTANT_50,
                ledger_label_m_prime="C",
                ledger_label_r="M",
            )
            with self.assertRaises(ValueError):
                recompute_v2(**paths, expected_arm_labels=("C", "M"))

    def test_dry_shakedown_labels_parse_via_expected_arm_labels(self) -> None:
        with TemporaryDirectory() as tmp_str:
            tmp = Path(tmp_str)
            paths = _build_fixture(
                tmp,
                TASKS,
                CONSTANT_50,
                CONSTANT_50,
                CONSTANT_50,
                CONSTANT_50,
                ledger_label_m_prime="M_PRIME_DRY",
                ledger_label_r="R_DRY",
            )
            result = recompute_v2(**paths, expected_arm_labels=("M_PRIME_DRY", "R_DRY"))
            self.assertEqual(result["lens"]["arm_labels"], {"m_prime": "M_PRIME_DRY", "r": "R_DRY"})


# ---------------------------------------------------------------------------
# Golden, hand-derived bootstrap values (seed 20260828) -- also the pin for
# mutation check #5 (seed drift must change these).
# ---------------------------------------------------------------------------


class GoldenBootstrapV2Tests(unittest.TestCase):
    """Golden values independently derived (standalone script, NOT importing
    recompute_v2/recompute_bootstrap) with `random.Random(20260828)`,
    B=10,000, RNG consumption order H2 (first) -> G1 (second) -- the only
    two endpoints in this module that touch the seeded RNG. Both diffs use
    the `_bootstrap_diff_independent`-style convention "R minus M'" (first
    arg R, second arg M').

    Derivation::

        def bootstrap_independent(rng, first, second, b=10000):
            diffs = []
            n1, n2 = len(first), len(second)
            for _ in range(b):
                r1 = [first[rng.randrange(n1)] for _ in range(n1)]
                r2 = [second[rng.randrange(n2)] for _ in range(n2)]
                diffs.append(statistics.median(r1) - statistics.median(r2))
            return diffs

        rng = random.Random(20260828)
        m_prime_p1 = [40,42,44,46,48,50,52,54]
        r_p1       = [41,43,45,47,49,51,53,55]
        m_prime_p2 = [30,58,60,62,64,66,68,95]
        r_p2       = [32,59,61,63,65,67,69,90]

        h2_diffs = bootstrap_independent(rng, r_p1, m_prime_p1)   # H2, 1st
        g1_diffs = bootstrap_independent(rng, r_p2, m_prime_p2)   # G1, 2nd
        # EXPECTED_H2_SE = statistics.pstdev(h2_diffs)
        # EXPECTED_G1_SE = statistics.pstdev(g1_diffs)

    A drifted seed (e.g. `random.Random(1)`) run through the identical
    program produces `g1_se_boot = 5.255611979351215` -- DIFFERENT from the
    pinned `EXPECTED_G1_SE_BOOT` below, which is exactly the "band changes
    when the seed drifts" property mutation check #5 verifies.
    """

    NAMES = [f"g{i}" for i in range(8)]
    M_PRIME_P1 = {n: v for n, v in zip(NAMES, [40, 42, 44, 46, 48, 50, 52, 54])}
    R_P1 = {n: v for n, v in zip(NAMES, [41, 43, 45, 47, 49, 51, 53, 55])}
    M_PRIME_P2 = {n: v for n, v in zip(NAMES, [30, 58, 60, 62, 64, 66, 68, 95])}
    R_P2 = {n: v for n, v in zip(NAMES, [32, 59, 61, 63, 65, 67, 69, 90])}

    EXPECTED_H2_DIFF = 1.0
    EXPECTED_H2_SE_BOOT = 3.394617903387655
    EXPECTED_H2_BAND = 6.78923580677531

    EXPECTED_G1_DIFF = 1.0
    EXPECTED_G1_SE_BOOT = 5.183476765405628
    EXPECTED_G1_BAND = 10.366953530811257

    def test_h2_and_g1_match_hand_derived_golden_values(self) -> None:
        with TemporaryDirectory() as tmp_str:
            tmp = Path(tmp_str)
            paths = _build_fixture(tmp, self.NAMES, self.M_PRIME_P1, self.M_PRIME_P2, self.R_P1, self.R_P2)
            result = recompute_v2(**paths)

            h2 = result["h2_p1_equivalence"]
            self.assertEqual(h2["diff"], self.EXPECTED_H2_DIFF)
            self.assertEqual(h2["se_boot"], self.EXPECTED_H2_SE_BOOT)
            self.assertEqual(h2["band"], self.EXPECTED_H2_BAND)
            self.assertFalse(h2["violated"])

            g1 = result["g1"]
            self.assertEqual(g1["diff"], self.EXPECTED_G1_DIFF)
            self.assertEqual(g1["se_boot"], self.EXPECTED_G1_SE_BOOT)
            self.assertEqual(g1["band"], self.EXPECTED_G1_BAND)
            self.assertEqual(g1["verdict"], "PASS")


if __name__ == "__main__":
    unittest.main()
