"""Tests for `tools.memory_battery.recompute` (task-4 brief). Every fixture
below is hand-built (no `corpus.py`/`driver.py` machinery involved) so every
number recompute reports has a known, hand-checkable arithmetic origin --
per the task-4 brief: "tests synthesize small journal files by hand (5-8
tasks) with known arithmetic ... assert exact medians, exact ITT inclusion,
exact None handling."

Fixture families:

- `_build_arithmetic_fixture`: 6 tasks (t0-t5), ONE re-ask (t1, arm C phase
  1: two `InferCompleted` rows for the same agent), ONE dropped task per
  arm (t2 in C via a `driver-infra` ledger status; t5 in M via a present
  ledger task_id with no `MemoryStamp` row), and non-injected M repeats
  (t1, t3 phase 2: mode `"silent"`, still ITT-included). With 1 infra
  drop per arm out of 12 task-halves (8.33% > H3's 5% ceiling), H3 --
  and therefore overall hygiene -- is INTENTIONALLY violated here: this
  fixture exercises the dropped/ITT/none-vs-zero/H3-kill path, not a
  clean E1 gate read.
- `_build_clean_fixture`: each arm's phase-1 and phase-2 costs are
  supplied as four INDEPENDENT dicts (`c_p1_costs`, `c_p2_costs`,
  `m_p1_costs`, `m_p2_costs`, review finding I4) -- a caller wanting H1/H2
  both trivially at 0 passes the SAME dict to the three phase-1/matching
  slots (as every PASS/FAIL/UNMEASURABLE/determinism/golden test below
  does); a caller wanting a HYGIENE violation makes exactly one dict
  diverge. `digest_c`/`digest_m` (also new) let a caller build an arm
  whose own two identity rows disagree with each other.

Review-fix additions (2026-08-26 fix round, 2 Critical + 4 Important):
`CostJournalFailureTests` (C1), `ArmCompletenessTests` (C2),
`GoldenBootstrapTests` (I1 + I2, one hand-derived fixture covering both),
`ExpectedDigestTests` (I3), `HygieneViolationTests` (I4).
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


# ---------------------------------------------------------------------------
# Shared fixture-writing helpers.
# ---------------------------------------------------------------------------


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


# ---------------------------------------------------------------------------
# Fixture 1: known arithmetic, ITT, dropped, none-vs-zero, H3 kill.
# ---------------------------------------------------------------------------

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


class ArithmeticFixtureTests(unittest.TestCase):
    """Fixture 1: exact medians, ITT, dropped, none-vs-zero, H3 kill."""

    def setUp(self) -> None:
        self._tmp_ctx = TemporaryDirectory()
        self.tmp = Path(self._tmp_ctx.name)
        self.addCleanup(self._tmp_ctx.cleanup)
        self.paths = _build_arithmetic_fixture(self.tmp)
        self.result = recompute(
            self.paths["corpus_dir"],
            self.paths["arm_c_dir"],
            self.paths["arm_m_dir"],
            self.paths["ledger_c"],
            self.paths["ledger_m"],
        )

    def test_arm_c_costs_exact_including_re_ask_sum(self) -> None:
        # t1 phase 1: 40 + 15 = 55 (the re-ask sums both InferCompleted rows).
        self.assertEqual(
            self.result["advisory"]["costs"]["c"]["p1"],
            {"t0": 100, "t1": 55, "t2": 80, "t3": 60, "t4": 90, "t5": 70},
        )
        # t2 is ABSENT (dropped), never a zero -- none-vs-zero.
        self.assertEqual(
            self.result["advisory"]["costs"]["c"]["p2"],
            {"t0": 90, "t1": 50, "t3": 55, "t4": 85, "t5": 65},
        )
        self.assertNotIn("t2", self.result["advisory"]["costs"]["c"]["p2"])

    def test_arm_m_costs_exact_and_non_injected_repeat_included_itt(self) -> None:
        self.assertEqual(
            self.result["advisory"]["costs"]["m"]["p1"],
            {"t0": 95, "t1": 52, "t2": 78, "t3": 58, "t4": 88, "t5": 68},
        )
        self.assertEqual(
            self.result["advisory"]["costs"]["m"]["p2"],
            {"t0": 60, "t1": 53, "t2": 55, "t3": 50, "t4": 70},
        )
        self.assertNotIn("t5", self.result["advisory"]["costs"]["m"]["p2"])
        # t1's phase-2 repeat is non-injected (mode "silent") but its real
        # cost (53) IS present in the p2 cost dict -- ITT: every non-dropped
        # task contributes, injected or not.
        self.assertEqual(self.result["advisory"]["modes_m"]["p2"]["t1"], "silent")
        self.assertIn("t1", self.result["advisory"]["costs"]["m"]["p2"])

    def test_exact_medians(self) -> None:
        # sorted([55,60,70,80,90,100]) -> (70+80)/2
        self.assertEqual(self.result["hygiene"]["h1_control_stability"]["diff"], 65.0 - 75.0)
        # sorted([50,55,65,85,90]) -> 65 (middle of 5)
        self.assertEqual(self.result["e1"]["median_c_p2"], 65.0)
        # sorted([50,53,55,60,70]) -> 55 (middle of 5)
        self.assertEqual(self.result["e1"]["median_m_p2"], 55.0)
        self.assertEqual(self.result["e1"]["min_c_p2"], 50)
        # H2: median_M,p1 (73.0) - median_C,p1 (75.0)
        self.assertEqual(self.result["hygiene"]["h2_first_exposure_equivalence"]["diff"], 73.0 - 75.0)

    def test_none_vs_zero_dropped_lists(self) -> None:
        dropped_c = self.result["dropped"]["C"]
        dropped_m = self.result["dropped"]["M"]
        self.assertEqual(len(dropped_c), 1)
        self.assertEqual(dropped_c[0]["task"], "t2")
        self.assertEqual(dropped_c[0]["phase"], 2)
        self.assertTrue(dropped_c[0]["infra"])
        self.assertIn("driver-infra", dropped_c[0]["reason"])

        self.assertEqual(len(dropped_m), 1)
        self.assertEqual(dropped_m[0]["task"], "t5")
        self.assertEqual(dropped_m[0]["phase"], 2)
        self.assertTrue(dropped_m[0]["infra"])
        self.assertIn("no MemoryStamp row", dropped_m[0]["reason"])

    def test_h3_infra_kill_short_circuits_e1_to_invalid(self) -> None:
        h3 = self.result["hygiene"]["h3_infra_rate"]
        self.assertEqual(h3["c_infra_count"], 1)
        self.assertEqual(h3["m_infra_count"], 1)
        self.assertEqual(h3["c_task_halves"], 12)
        self.assertAlmostEqual(h3["c_infra_rate"], 1 / 12)
        self.assertTrue(h3["violated"])  # 8.33% > 5% ceiling
        self.assertTrue(self.result["hygiene"]["violated"])
        self.assertEqual(self.result["e1"]["verdict"], "INVALID")
        self.assertEqual(self.result["verdict"], "INVALID")
        self.assertIsNone(self.result["e1"]["delta_min"])  # no gate number read

    def test_h4_advisory_rates_use_manifest_n_denominator(self) -> None:
        h4 = self.result["advisory"]["h4"]
        # 3 injected (t0,t2,t4) out of n=6 -- NOT out of 5 measured.
        self.assertEqual(h4["m_p2_injected_count"], 3)
        self.assertEqual(h4["m_p2_injection_rate"], 3 / 6)
        # 5 minted (all but t3) out of n=6.
        self.assertEqual(h4["m_p1_mint_count"], 5)
        self.assertEqual(h4["m_p1_mint_rate"], 5 / 6)
        self.assertEqual(h4["n"], 6)

    def test_success_rates_all_measured_tasks_done(self) -> None:
        rates = self.result["advisory"]["success_rates"]
        self.assertEqual(rates["c_p1"], {"rate": 1.0, "successes": 6, "n": 6})
        self.assertEqual(rates["c_p2"], {"rate": 1.0, "successes": 5, "n": 5})
        self.assertEqual(rates["m_p1"], {"rate": 1.0, "successes": 6, "n": 6})
        self.assertEqual(rates["m_p2"], {"rate": 1.0, "successes": 5, "n": 5})

    def test_row_counts(self) -> None:
        rc = self.result["advisory"]["row_counts"]
        self.assertEqual(rc["c_stamp"], 11)  # 12 task-halves minus t2/p2 (never stamped)
        self.assertEqual(rc["c_mint"], 0)  # arm C never mints (memory off)
        self.assertEqual(rc["c_contradicted"], 0)
        self.assertEqual(rc["m_stamp"], 11)  # 12 minus t5/p2 (never stamped)
        self.assertEqual(rc["m_mint"], 5)
        self.assertEqual(rc["m_contradicted"], 0)

    def test_paired_deltas_m_exact(self) -> None:
        paired = self.result["advisory"]["paired_deltas_m"]
        by_task = {entry["task"]: entry["delta"] for entry in paired["per_task"]}
        self.assertEqual(
            by_task, {"t0": -35, "t1": 1, "t2": -23, "t3": -8, "t4": -18}
        )
        self.assertNotIn("t5", by_task)  # t5 dropped in p2, cannot pair
        # sorted([-35,-23,-18,-8,1]) -> middle = -18
        self.assertEqual(paired["median_delta"], -18.0)
        self.assertEqual(paired["n_pairs"], 5)
        # Review finding I2: paired_deltas_m now carries a paired-bootstrap
        # SE alongside median_delta -- these 5 deltas have real spread, so
        # se_boot must be a genuine positive float, never None/0 by default.
        self.assertIsInstance(paired["se_boot"], float)
        self.assertGreater(paired["se_boot"], 0.0)

    def test_lens_block(self) -> None:
        lens = self.result["lens"]
        self.assertEqual(lens["instrument"], "memory-battery-v1")
        self.assertEqual(lens["corpus_seed"], 20260826)
        self.assertEqual(lens["n"], 6)
        self.assertEqual(lens["envelope"], "v4")
        self.assertEqual(lens["window_cap"], WINDOW_CAP)
        self.assertEqual(lens["bootstrap_seed"], 20260826)
        self.assertEqual(lens["bootstrap_b"], 10_000)
        self.assertEqual(lens["digest_c"], {"phase1": "digest-c", "phase2": "digest-c"})
        self.assertEqual(lens["digest_m"], {"phase1": "digest-m", "phase2": "digest-m"})
        self.assertIsNone(lens["expected_digest"])
        # Independent re-derivation of corpus_sha (mirrors corpus_check.py's
        # own "independent reimplementation" discipline for check 3).
        import hashlib

        hasher = hashlib.sha256()
        for name in sorted(TASKS):
            hasher.update(name.encode("utf-8"))
            hasher.update(b"\0")
            hasher.update(f"sha-{name}".encode("utf-8"))
            hasher.update(b"\0")
        self.assertEqual(lens["corpus_sha"], hasher.hexdigest())

    def test_step_and_wall_medians(self) -> None:
        adv = self.result["advisory"]
        self.assertEqual(adv["steps_median"]["c_p1"], 1)
        self.assertEqual(adv["wall_ms_median"]["c_p1"], 1000)
        self.assertEqual(adv["wall_ms_median"]["m_p2"], 1000)

    def test_identity_agrees_no_violation(self) -> None:
        identity = self.result["hygiene"]["identity"]
        self.assertFalse(identity["c"]["violated"])
        self.assertFalse(identity["m"]["violated"])
        self.assertTrue(identity["c"]["agree"])
        self.assertTrue(identity["m"]["agree"])

    def test_arm_completeness_clean_both_arms_full(self) -> None:
        # Every task-half in this fixture DOES get a ledger row (even the
        # driver-infra / missing-stamp ones) -- 12 == 2*6 for both arms, so
        # arm-completeness itself is clean; H3 is the check that fires here.
        completeness = self.result["hygiene"]["arm_completeness"]
        self.assertFalse(completeness["c"]["violated"])
        self.assertFalse(completeness["m"]["violated"])
        self.assertEqual(completeness["c"]["actual_task_halves"], 12)
        self.assertEqual(completeness["c"]["expected_task_halves"], 12)


# ---------------------------------------------------------------------------
# Fixture 2: hygiene-clean-by-default (H1/H2 diffs exactly 0 when a caller
# passes matching dicts), used for E1 PASS/FAIL/UNMEASURABLE, determinism,
# round-trip, completeness, the ledger-independence invariant, the golden
# bootstrap value, expected-digest checks, and hygiene-violation tests.
# ---------------------------------------------------------------------------


def _build_clean_fixture(
    tmp: Path,
    c_p1_costs: dict[str, int],
    c_p2_costs: dict[str, int],
    m_p1_costs: dict[str, int],
    m_p2_costs: dict[str, int],
    wall_s: float = 1.0,
    digest_c: tuple[str | None, str | None] = ("digest-c", "digest-c"),
    digest_m: tuple[str | None, str | None] = ("digest-m", "digest-m"),
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
    own two ledger identity rows disagree with each other."""
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
        digest: tuple[str | None, str | None],
        p1_costs: dict[str, int],
        p2_costs: dict[str, int],
        p1_mode: str,
        p2_mode: str,
    ) -> None:
        ledger_rows: list[dict[str, Any]] = list(_identity_rows(arm, digest[0], digest[1]))
        tasks_journal: list[dict[str, Any]] = []
        boot: list[dict[str, Any]] = []
        for phase, costs in ((1, p1_costs), (2, p2_costs)):
            for name in names:
                task_id = f"{arm}-{phase}-{name}-tid"
                agent_id = f"{arm}-{phase}-{name}-agent"
                ledger_rows.append(_ledger_row(arm, phase, name, task_id, wall_s=wall_s))
                mode = p1_mode if phase == 1 else p2_mode
                tasks_journal.append(_memory_stamp(agent_id, task_id, mode))
                tasks_journal.append(_task_step_done(agent_id))
                cost = costs[name]
                boot.append(_infer_completed(agent_id, cost, cost + 1))
        _write_jsonl(ledger_path, ledger_rows)
        _write_jsonl(arm_dir / "journal" / "tasks.jsonl", tasks_journal)
        _write_jsonl(arm_dir / "journal" / "boot-0001.jsonl", boot)

    _write_arm(arm_c_dir, ledger_c, "C", digest_c, c_p1_costs, c_p2_costs, "off", "off")
    _write_arm(arm_m_dir, ledger_m, "M", digest_m, m_p1_costs, m_p2_costs, "silent", "silent")

    return {
        "corpus_dir": corpus_dir,
        "arm_c_dir": arm_c_dir,
        "arm_m_dir": arm_m_dir,
        "ledger_c": ledger_c,
        "ledger_m": ledger_m,
    }


PHASE1 = {"t0": 40, "t1": 45, "t2": 50, "t3": 55, "t4": 60, "t5": 65}
CONSTANT_50 = {name: 50 for name in TASKS}


class CleanGateTests(unittest.TestCase):
    def test_pass_when_m_p2_constant_gap_below_c_p2(self) -> None:
        with TemporaryDirectory() as tmp_str:
            tmp = Path(tmp_str)
            paths = _build_clean_fixture(
                tmp, CONSTANT_50, CONSTANT_50, CONSTANT_50, {n: 20 for n in TASKS}
            )
            result = recompute(**{k: v for k, v in paths.items()})

            self.assertFalse(result["hygiene"]["violated"])
            e1 = result["e1"]
            # Both arms constant in phase 2 -> every bootstrap resample's
            # median equals the constant exactly -> SE_boot = 0 exactly ->
            # delta_min = 0 exactly (hand-derivable, not just "reproducible").
            self.assertEqual(e1["delta_min"], 0.0)
            self.assertEqual(e1["se_boot"], 0.0)
            self.assertEqual(e1["median_c_p2"], 50.0)
            self.assertEqual(e1["median_m_p2"], 20.0)
            self.assertEqual(e1["headroom"], 0.0)
            self.assertEqual(e1["verdict"], "PASS")
            self.assertEqual(result["verdict"], "PASS")

    def test_fail_when_m_p2_constant_gap_above_c_p2(self) -> None:
        with TemporaryDirectory() as tmp_str:
            tmp = Path(tmp_str)
            paths = _build_clean_fixture(
                tmp, CONSTANT_50, CONSTANT_50, CONSTANT_50, {n: 80 for n in TASKS}
            )
            result = recompute(**{k: v for k, v in paths.items()})

            self.assertFalse(result["hygiene"]["violated"])
            e1 = result["e1"]
            self.assertEqual(e1["delta_min"], 0.0)
            self.assertEqual(e1["verdict"], "FAIL")
            self.assertIsNotNone(e1["reason"])
            self.assertEqual(result["verdict"], "FAIL")

    def test_unmeasurable_headroom_clause_when_c_p2_floor_saturated(self) -> None:
        with TemporaryDirectory() as tmp_str:
            tmp = Path(tmp_str)
            # C,p2 constant (headroom exactly 0); M,p2 has real spread, so
            # SE_boot(and thus delta_min) is strictly > 0 = headroom.
            m_p2 = {"t0": 10, "t1": 10, "t2": 10, "t3": 90, "t4": 90, "t5": 90}
            paths = _build_clean_fixture(tmp, CONSTANT_50, CONSTANT_50, CONSTANT_50, m_p2)
            result = recompute(**{k: v for k, v in paths.items()})

            self.assertFalse(result["hygiene"]["violated"])
            e1 = result["e1"]
            self.assertEqual(e1["headroom"], 0.0)
            self.assertGreater(e1["delta_min"], 0.0)
            self.assertEqual(e1["verdict"], "UNMEASURABLE")
            self.assertIn("headroom", e1["reason"])
            self.assertEqual(result["verdict"], "UNMEASURABLE")

    def test_e1_verdict_is_self_consistent_with_reported_delta_min(self) -> None:
        """Whatever delta_min the bootstrap produced, PASS/FAIL must follow
        the pinned formula (design spec §4) applied to that exact number --
        this does not require hand-deriving the bootstrap's own output."""
        with TemporaryDirectory() as tmp_str:
            tmp = Path(tmp_str)
            m_p2 = {"t0": 15, "t1": 22, "t2": 18, "t3": 25, "t4": 20, "t5": 30}
            c_p2 = {"t0": 48, "t1": 52, "t2": 50, "t3": 55, "t4": 49, "t5": 53}
            paths = _build_clean_fixture(tmp, PHASE1, c_p2, PHASE1, m_p2)
            result = recompute(**{k: v for k, v in paths.items()})
            e1 = result["e1"]
            if e1["verdict"] == "UNMEASURABLE":
                self.assertLess(e1["headroom"], e1["delta_min"])
            else:
                expected = "PASS" if e1["median_m_p2"] <= e1["median_c_p2"] - e1["delta_min"] else "FAIL"
                self.assertEqual(e1["verdict"], expected)

    def test_determinism_same_inputs_twice_identical_delta_min(self) -> None:
        """Mutation check #3's target: a seeded bootstrap must reproduce the
        exact same `delta_min` (and full result) across two independent
        calls over byte-identical inputs. Uses varied (non-constant)
        phase-2 data so `delta_min` is genuinely bootstrap-derived, not
        trivially 0 regardless of seeding."""
        with TemporaryDirectory() as tmp_str:
            tmp = Path(tmp_str)
            m_p2 = {"t0": 15, "t1": 22, "t2": 18, "t3": 25, "t4": 20, "t5": 30}
            c_p2 = {"t0": 48, "t1": 52, "t2": 50, "t3": 55, "t4": 49, "t5": 53}
            paths = _build_clean_fixture(tmp, PHASE1, c_p2, PHASE1, m_p2)
            kwargs = {k: v for k, v in paths.items()}
            result1 = recompute(**kwargs)
            result2 = recompute(**kwargs)
            self.assertEqual(result1["e1"]["delta_min"], result2["e1"]["delta_min"])
            self.assertIsNotNone(result1["e1"]["delta_min"])
            self.assertEqual(result1, result2)

    def test_completeness_pinned_top_level_schema(self) -> None:
        with TemporaryDirectory() as tmp_str:
            tmp = Path(tmp_str)
            paths = _build_clean_fixture(
                tmp, CONSTANT_50, CONSTANT_50, CONSTANT_50, {n: 20 for n in TASKS}
            )
            result = recompute(**{k: v for k, v in paths.items()})

            self.assertEqual(set(result.keys()), {"verdict", "e1", "hygiene", "advisory", "lens", "dropped"})
            self.assertEqual(
                set(result["e1"].keys()),
                {
                    "verdict", "median_c_p2", "median_m_p2", "min_c_p2", "headroom",
                    "delta_min", "se_boot", "n_c_p2", "n_m_p2", "reason",
                },
            )
            self.assertEqual(
                set(result["hygiene"].keys()),
                {
                    "violated", "reasons", "arm_completeness", "identity",
                    "h1_control_stability", "h2_first_exposure_equivalence", "h3_infra_rate",
                },
            )
            self.assertEqual(
                set(result["hygiene"]["arm_completeness"].keys()), {"c", "m", "violated"}
            )
            self.assertEqual(
                set(result["hygiene"]["arm_completeness"]["c"].keys()),
                {"expected_task_halves", "actual_task_halves", "violated", "reason"},
            )
            self.assertEqual(
                set(result["advisory"].keys()),
                {
                    "saturation_note", "h4", "success_rates", "steps_median", "wall_ms_median",
                    "paired_deltas_m", "row_counts", "costs", "modes_m", "successes",
                },
            )
            self.assertEqual(
                set(result["advisory"]["paired_deltas_m"].keys()),
                {"per_task", "median_delta", "se_boot", "n_pairs"},
            )
            self.assertEqual(
                set(result["lens"].keys()),
                {
                    "instrument", "corpus_seed", "n", "corpus_sha", "envelope", "window_cap",
                    "bootstrap_seed", "bootstrap_b", "expected_digest", "digest_c", "digest_m",
                },
            )
            self.assertEqual(set(result["dropped"].keys()), {"C", "M"})

    def test_serialization_round_trips(self) -> None:
        with TemporaryDirectory() as tmp_str:
            tmp = Path(tmp_str)
            paths = _build_clean_fixture(
                tmp, CONSTANT_50, CONSTANT_50, CONSTANT_50, {n: 20 for n in TASKS}
            )
            result = recompute(**{k: v for k, v in paths.items()})
            round_tripped = json.loads(json.dumps(result))
            self.assertEqual(result, round_tripped)

    def test_ledger_independence_wrong_wall_s_never_changes_output(self) -> None:
        """The task-4 brief's INVARIANT: "a test feeds a ledger with a
        WRONG wall_s and asserts the output is unchanged." The ledger's
        only load-bearing contributions are identity rows, driver-infra
        status flags, and task->task_id join pairs (controller ruling) --
        `wall_s` itself must never move any output number."""
        with TemporaryDirectory() as tmp_a:
            paths_a = _build_clean_fixture(
                Path(tmp_a), PHASE1, {n: 50 for n in TASKS}, PHASE1, {n: 20 for n in TASKS}, wall_s=1.0
            )
            result_correct = recompute(**{k: v for k, v in paths_a.items()})
        with TemporaryDirectory() as tmp_b:
            paths_b = _build_clean_fixture(
                Path(tmp_b), PHASE1, {n: 50 for n in TASKS}, PHASE1, {n: 20 for n in TASKS}, wall_s=99999.0
            )
            result_wrong_wall = recompute(**{k: v for k, v in paths_b.items()})

        self.assertEqual(result_correct, result_wrong_wall)


# ---------------------------------------------------------------------------
# Review finding C1: missing/empty cost journals must never zero-fill.
# ---------------------------------------------------------------------------


class CostJournalFailureTests(unittest.TestCase):
    """A deleted boot journal, or a task-half whose agent has zero
    InferCompleted rows, must never read as "this task cost 0" -- the
    reviewer's probe turned the old `completion_by_agent.get(agent_id, 0)`
    fallback into a manufactured PASS (medians 0 vs 0, delta_min 0.0)."""

    def test_deleted_boot_journal_raises_hard_error(self) -> None:
        with TemporaryDirectory() as tmp_str:
            tmp = Path(tmp_str)
            paths = _build_clean_fixture(
                tmp, CONSTANT_50, CONSTANT_50, CONSTANT_50, {n: 20 for n in TASKS}
            )
            boot_path = paths["arm_c_dir"] / "journal" / "boot-0001.jsonl"
            self.assertTrue(boot_path.exists())
            boot_path.unlink()  # simulates a deleted/never-written boot journal

            with self.assertRaises(ValueError) as ctx:
                recompute(**{k: v for k, v in paths.items()})
            self.assertIn("boot-*.jsonl", str(ctx.exception))

    def test_agent_with_zero_infercompleted_rows_is_dropped_never_cost_zero(self) -> None:
        """A task-half that got a real agent/MemoryStamp/TaskStep but NO
        InferCompleted row at all (e.g. the daemon crashed before its
        first inference reply landed) must be dropped, not silently cost
        0 -- `agent_id not in completion_by_agent` is the exact signal."""
        with TemporaryDirectory() as tmp_str:
            tmp = Path(tmp_str)
            names = ["z0", "z1", "z2", "z3"]
            corpus_dir = tmp / "corpus"
            _write_manifest(corpus_dir, names)
            arm_c_dir = tmp / "arm_c"
            arm_m_dir = tmp / "arm_m"
            ledger_c = tmp / "ledger_c.jsonl"
            ledger_m = tmp / "ledger_m.jsonl"

            def write_arm(arm_dir: Path, ledger_path: Path, arm: str, digest: str, cost: int) -> None:
                ledger_rows = list(_identity_rows(arm, digest, digest))
                tasks_journal: list[dict[str, Any]] = []
                boot: list[dict[str, Any]] = []
                for phase in (1, 2):
                    for name in names:
                        task_id = f"{arm}-{phase}-{name}-tid"
                        agent_id = f"{arm}-{phase}-{name}-agent"
                        ledger_rows.append(_ledger_row(arm, phase, name, task_id))
                        tasks_journal.append(_memory_stamp(agent_id, task_id, "off"))
                        tasks_journal.append(_task_step_done(agent_id))
                        # z1/C/phase2's agent gets everything EXCEPT an
                        # InferCompleted row -- the case under test.
                        if not (arm == "C" and phase == 2 and name == "z1"):
                            boot.append(_infer_completed(agent_id, cost, cost + 1))
                _write_jsonl(ledger_path, ledger_rows)
                _write_jsonl(arm_dir / "journal" / "tasks.jsonl", tasks_journal)
                _write_jsonl(arm_dir / "journal" / "boot-0001.jsonl", boot)

            write_arm(arm_c_dir, ledger_c, "C", "digest-c", 50)
            write_arm(arm_m_dir, ledger_m, "M", "digest-m", 20)

            result = recompute(corpus_dir, arm_c_dir, arm_m_dir, ledger_c, ledger_m)

            self.assertNotIn("z1", result["advisory"]["costs"]["c"]["p2"])
            dropped_c = result["dropped"]["C"]
            matching = [e for e in dropped_c if e["task"] == "z1" and e["phase"] == 2]
            self.assertEqual(len(matching), 1)
            self.assertTrue(matching[0]["infra"])
            self.assertIn("no InferCompleted rows", matching[0]["reason"])


# ---------------------------------------------------------------------------
# Review finding C2: H3 must not be blind to a truncated/dead-driver arm.
# ---------------------------------------------------------------------------


class ArmCompletenessTests(unittest.TestCase):
    """A ledger that never got rows for part of an arm (driver died
    mid-run, leaving no trace at all for the un-run task-halves) must be
    caught by an explicit arm-completeness check -- H3's RATE alone is not
    a sufficient substitute for "this arm never finished" as a named fact
    -- and every missing-ledger-row drop must show up as infra:True."""

    def test_truncated_arm_yields_invalid_with_named_completeness_reason(self) -> None:
        with TemporaryDirectory() as tmp_str:
            tmp = Path(tmp_str)
            names = ["u0", "u1", "u2", "u3", "u4"]
            corpus_dir = tmp / "corpus"
            _write_manifest(corpus_dir, names)

            arm_c_dir = tmp / "arm_c"
            arm_m_dir = tmp / "arm_m"
            ledger_c = tmp / "ledger_c.jsonl"
            ledger_m = tmp / "ledger_m.jsonl"

            # Arm C: phase 1 complete (5/5); phase 2 only 2 of 5 task-halves
            # ever got a ledger row -- the driver died after u0/u1 and never
            # wrote rows for u2/u3/u4 at all (a real crash leaves NO trace,
            # unlike an explicit driver-infra status row).
            ledger_c_rows = list(_identity_rows("C", "digest-c", "digest-c"))
            tasks_journal_c: list[dict[str, Any]] = []
            boot_c: list[dict[str, Any]] = []
            for name in names:
                task_id = f"c-p1-{name}-tid"
                agent_id = f"c-1-{name}-agent"
                ledger_c_rows.append(_ledger_row("C", 1, name, task_id))
                tasks_journal_c.append(_memory_stamp(agent_id, task_id, "off"))
                tasks_journal_c.append(_task_step_done(agent_id))
                boot_c.append(_infer_completed(agent_id, 50, 51))
            for name in names[:2]:  # only u0, u1 have a phase-2 ledger row
                task_id = f"c-p2-{name}-tid"
                agent_id = f"c-2-{name}-agent"
                ledger_c_rows.append(_ledger_row("C", 2, name, task_id))
                tasks_journal_c.append(_memory_stamp(agent_id, task_id, "off"))
                tasks_journal_c.append(_task_step_done(agent_id))
                boot_c.append(_infer_completed(agent_id, 50, 51))
            _write_jsonl(ledger_c, ledger_c_rows)
            _write_jsonl(arm_c_dir / "journal" / "tasks.jsonl", tasks_journal_c)
            _write_jsonl(arm_c_dir / "journal" / "boot-0001.jsonl", boot_c)

            # Arm M: fully complete, both phases, all 5 tasks.
            ledger_m_rows = list(_identity_rows("M", "digest-m", "digest-m"))
            tasks_journal_m: list[dict[str, Any]] = []
            boot_m: list[dict[str, Any]] = []
            for phase in (1, 2):
                for name in names:
                    task_id = f"m-{phase}-{name}-tid"
                    agent_id = f"m-{phase}-{name}-agent"
                    ledger_m_rows.append(_ledger_row("M", phase, name, task_id))
                    tasks_journal_m.append(_memory_stamp(agent_id, task_id, "silent"))
                    tasks_journal_m.append(_task_step_done(agent_id))
                    boot_m.append(_infer_completed(agent_id, 20, 21))
            _write_jsonl(ledger_m, ledger_m_rows)
            _write_jsonl(arm_m_dir / "journal" / "tasks.jsonl", tasks_journal_m)
            _write_jsonl(arm_m_dir / "journal" / "boot-0001.jsonl", boot_m)

            result = recompute(corpus_dir, arm_c_dir, arm_m_dir, ledger_c, ledger_m)

            completeness = result["hygiene"]["arm_completeness"]
            self.assertTrue(completeness["c"]["violated"])
            self.assertEqual(completeness["c"]["expected_task_halves"], 10)
            self.assertEqual(completeness["c"]["actual_task_halves"], 7)
            self.assertIsNotNone(completeness["c"]["reason"])
            self.assertFalse(completeness["m"]["violated"])
            self.assertTrue(result["hygiene"]["violated"])
            self.assertEqual(result["verdict"], "INVALID")
            self.assertIsNone(result["e1"]["delta_min"])

            # The 3 never-ledgered phase-2 tasks (u2,u3,u4) are dropped with
            # infra:True -- "no ledger row" now counts toward H3 (C2's other
            # half).
            dropped_c = result["dropped"]["C"]
            missing = {"u2", "u3", "u4"}
            found = {entry["task"] for entry in dropped_c if entry["phase"] == 2 and entry["task"] in missing}
            self.assertEqual(found, missing)
            for entry in dropped_c:
                if entry["task"] in missing and entry["phase"] == 2:
                    self.assertTrue(entry["infra"])
                    self.assertIn("no ledger row", entry["reason"])

            h3 = result["hygiene"]["h3_infra_rate"]
            self.assertEqual(h3["c_infra_count"], 3)
            self.assertGreater(h3["c_infra_rate"], 0.05)
            self.assertTrue(h3["violated"])


# ---------------------------------------------------------------------------
# Review finding I1 + I2: a golden, hand-derived bootstrap value covering
# BOTH E1's delta_min (I1) and M's paired-deltas SE (I2) on one fixture.
# ---------------------------------------------------------------------------


class GoldenBootstrapTests(unittest.TestCase):
    """Review finding I1: the resample-median calls INSIDE the bootstrap
    loops (`recompute_bootstrap.py`'s `_bootstrap_diff_independent`/
    `_bootstrap_diff_paired`) were not covered by any exact-value
    assertion -- only `_median_or_none`'s own OUTER median call was pinned
    (mutation check #2). `EXPECTED_DELTA_MIN`/`EXPECTED_SE_BOOT` (E1) and
    `EXPECTED_M_PAIRED_SE_BOOT`/`EXPECTED_M_MEDIAN_DELTA` (review finding
    I2, M's advisory paired-deltas SE) below were each computed ONCE via
    an independent from-scratch reimplementation of the bootstrap
    algorithm (seed 20260826, B=10,000, the SAME H1 -> H2 -> E1 -> M's
    paired-deltas RNG consumption order `recompute()` uses) run in a
    standalone script, then hard-coded here. Any drift in either bootstrap
    loop's own median call (e.g. swapped for mean) changes these numbers
    and fails this test, independent of the outer `_median_or_none`
    mutation check.

    Derivation (run once, independently, NOT importing
    `recompute_bootstrap.py`):

        import random, statistics

        def bootstrap_paired(rng, pairs, b=10000):
            diffs = []
            n = len(pairs)
            for _ in range(b):
                drawn = [pairs[rng.randrange(n)] for _ in range(n)]
                before = [p[0] for p in drawn]
                after = [p[1] for p in drawn]
                diffs.append(statistics.median(after) - statistics.median(before))
            return diffs

        def bootstrap_independent(rng, first, second, b=10000):
            diffs = []
            n1, n2 = len(first), len(second)
            for _ in range(b):
                r1 = [first[rng.randrange(n1)] for _ in range(n1)]
                r2 = [second[rng.randrange(n2)] for _ in range(n2)]
                diffs.append(statistics.median(r1) - statistics.median(r2))
            return diffs

        rng = random.Random(20260826)
        c_p1 = c_p2 = m_p1 = [50, 52, 54, 56]
        m_p2 = [10, 14, 18, 22]

        bootstrap_paired(rng, list(zip(c_p1, c_p2)))        # H1, consumed 1st
        bootstrap_independent(rng, m_p1, c_p1)              # H2, consumed 2nd
        e1_diffs = bootstrap_independent(rng, m_p2, c_p2)   # E1, consumed 3rd
        # EXPECTED_SE_BOOT   = statistics.pstdev(e1_diffs)
        # EXPECTED_DELTA_MIN = 2 * EXPECTED_SE_BOOT

        m_pairs = list(zip(m_p1, m_p2))
        m_diffs = bootstrap_paired(rng, m_pairs)            # M paired-deltas, 4th/last
        # EXPECTED_M_PAIRED_SE_BOOT = statistics.pstdev(m_diffs)
        # EXPECTED_M_MEDIAN_DELTA   = statistics.median([p2 - p1 for p1, p2 in m_pairs])
    """

    GOLDEN_C_P1 = {"g0": 50, "g1": 52, "g2": 54, "g3": 56}
    GOLDEN_C_P2 = {"g0": 50, "g1": 52, "g2": 54, "g3": 56}
    GOLDEN_M_P1 = {"g0": 50, "g1": 52, "g2": 54, "g3": 56}
    GOLDEN_M_P2 = {"g0": 10, "g1": 14, "g2": 18, "g3": 22}

    EXPECTED_SE_BOOT = 3.3896804333152115
    EXPECTED_DELTA_MIN = 6.779360866630423
    EXPECTED_M_PAIRED_SE_BOOT = 1.4935798070407889
    EXPECTED_M_MEDIAN_DELTA = -37.0

    def test_delta_min_and_paired_delta_se_match_hand_derived_golden_values(self) -> None:
        with TemporaryDirectory() as tmp_str:
            tmp = Path(tmp_str)
            paths = _build_clean_fixture(
                tmp, self.GOLDEN_C_P1, self.GOLDEN_C_P2, self.GOLDEN_M_P1, self.GOLDEN_M_P2
            )
            result = recompute(**{k: v for k, v in paths.items()})

            self.assertFalse(result["hygiene"]["violated"])
            e1 = result["e1"]
            self.assertEqual(e1["median_c_p2"], 53.0)
            self.assertEqual(e1["median_m_p2"], 16.0)
            self.assertEqual(e1["min_c_p2"], 50)
            self.assertEqual(e1["se_boot"], self.EXPECTED_SE_BOOT)
            self.assertEqual(e1["delta_min"], self.EXPECTED_DELTA_MIN)

            paired = result["advisory"]["paired_deltas_m"]
            self.assertEqual(paired["median_delta"], self.EXPECTED_M_MEDIAN_DELTA)
            self.assertEqual(paired["se_boot"], self.EXPECTED_M_PAIRED_SE_BOOT)


# ---------------------------------------------------------------------------
# Review finding I3: expected_digest must be live, not theoretical.
# ---------------------------------------------------------------------------


class ExpectedDigestTests(unittest.TestCase):
    """The library kwarg stays optional (fixtures don't need it), but a
    real mismatch must show up as a named INVALID, and the CLI must
    REQUIRE --expected-digest so a real gate run can never silently skip
    the check."""

    def test_expected_digest_mismatch_yields_invalid_with_named_reason(self) -> None:
        with TemporaryDirectory() as tmp_str:
            tmp = Path(tmp_str)
            paths = _build_clean_fixture(
                tmp, CONSTANT_50, CONSTANT_50, CONSTANT_50, {n: 20 for n in TASKS}
            )
            result = recompute(
                **{k: v for k, v in paths.items()}, expected_digest="totally-different-digest"
            )
            self.assertEqual(result["verdict"], "INVALID")
            identity = result["hygiene"]["identity"]
            self.assertTrue(identity["c"]["violated"])
            self.assertTrue(identity["m"]["violated"])
            self.assertFalse(identity["c"]["matches_expected"])
            self.assertIn("expected", identity["c"]["reason"])
            self.assertIn("totally-different-digest", identity["c"]["reason"])
            self.assertIsNone(result["e1"]["delta_min"])

    def test_expected_digest_match_does_not_violate_identity(self) -> None:
        with TemporaryDirectory() as tmp_str:
            tmp = Path(tmp_str)
            paths = _build_clean_fixture(
                tmp, CONSTANT_50, CONSTANT_50, CONSTANT_50, {n: 20 for n in TASKS}
            )
            # _build_clean_fixture's default digests are "digest-c"/"digest-m".
            result = recompute(**{k: v for k, v in paths.items()}, expected_digest="digest-c")
            self.assertFalse(result["hygiene"]["identity"]["c"]["violated"])
            self.assertTrue(result["hygiene"]["identity"]["c"]["matches_expected"])

    def test_cli_requires_expected_digest(self) -> None:
        with TemporaryDirectory() as tmp_str:
            tmp = Path(tmp_str)
            paths = _build_clean_fixture(
                tmp, CONSTANT_50, CONSTANT_50, CONSTANT_50, {n: 20 for n in TASKS}
            )
            argv = [
                "--corpus-dir", str(paths["corpus_dir"]),
                "--arm-c-dir", str(paths["arm_c_dir"]),
                "--arm-m-dir", str(paths["arm_m_dir"]),
                "--ledger-c", str(paths["ledger_c"]),
                "--ledger-m", str(paths["ledger_m"]),
            ]
            with self.assertRaises(SystemExit):
                main(argv)

    def test_cli_accepts_expected_digest_and_reports_mismatch(self) -> None:
        with TemporaryDirectory() as tmp_str:
            tmp = Path(tmp_str)
            paths = _build_clean_fixture(
                tmp, CONSTANT_50, CONSTANT_50, CONSTANT_50, {n: 20 for n in TASKS}
            )
            argv = [
                "--corpus-dir", str(paths["corpus_dir"]),
                "--arm-c-dir", str(paths["arm_c_dir"]),
                "--arm-m-dir", str(paths["arm_m_dir"]),
                "--ledger-c", str(paths["ledger_c"]),
                "--ledger-m", str(paths["ledger_m"]),
                "--expected-digest", "totally-different-digest",
            ]
            stdout = io.StringIO()
            with contextlib.redirect_stdout(stdout):
                exit_code = main(argv)
            self.assertEqual(exit_code, 0)  # a clean recompute run that just reports INVALID
            printed = json.loads(stdout.getvalue())
            self.assertEqual(printed["verdict"], "INVALID")


# ---------------------------------------------------------------------------
# Review finding I4: H1/H2/identity violations must each be independently
# exercised.
# ---------------------------------------------------------------------------


class HygieneViolationTests(unittest.TestCase):
    """Each of H1, H2, and identity self-disagreement is driven to
    INVALID in isolation (the other checks stay clean by construction),
    asserting verdict, the named hygiene reason, and null e1 fields."""

    def test_h1_violation_yields_invalid(self) -> None:
        with TemporaryDirectory() as tmp_str:
            tmp = Path(tmp_str)
            huge_c_p2 = {n: 500 for n in TASKS}  # C jumps 50 -> 500 between phases
            paths = _build_clean_fixture(
                tmp, CONSTANT_50, huge_c_p2, CONSTANT_50, {n: 20 for n in TASKS}
            )
            result = recompute(**{k: v for k, v in paths.items()})

            h1 = result["hygiene"]["h1_control_stability"]
            self.assertTrue(h1["violated"])
            self.assertEqual(h1["diff"], 450.0)
            self.assertIsNotNone(h1["reason"])
            self.assertIn("H1", h1["reason"])
            self.assertTrue(result["hygiene"]["violated"])
            self.assertEqual(result["verdict"], "INVALID")
            self.assertIsNone(result["e1"]["delta_min"])
            self.assertIsNone(result["e1"]["se_boot"])
            self.assertIsNone(result["e1"]["headroom"])

    def test_h2_violation_yields_invalid(self) -> None:
        with TemporaryDirectory() as tmp_str:
            tmp = Path(tmp_str)
            huge_m_p1 = {n: 500 for n in TASKS}  # M's phase 1 jumps far above C's
            paths = _build_clean_fixture(
                tmp, CONSTANT_50, CONSTANT_50, huge_m_p1, {n: 20 for n in TASKS}
            )
            result = recompute(**{k: v for k, v in paths.items()})

            h2 = result["hygiene"]["h2_first_exposure_equivalence"]
            self.assertTrue(h2["violated"])
            self.assertEqual(h2["diff"], 450.0)
            self.assertIsNotNone(h2["reason"])
            self.assertIn("H2", h2["reason"])
            self.assertTrue(result["hygiene"]["violated"])
            self.assertEqual(result["verdict"], "INVALID")
            self.assertIsNone(result["e1"]["delta_min"])
            self.assertIsNone(result["e1"]["se_boot"])
            self.assertIsNone(result["e1"]["headroom"])

    def test_identity_self_disagreement_yields_invalid(self) -> None:
        with TemporaryDirectory() as tmp_str:
            tmp = Path(tmp_str)
            paths = _build_clean_fixture(
                tmp,
                CONSTANT_50,
                CONSTANT_50,
                CONSTANT_50,
                {n: 20 for n in TASKS},
                digest_c=("digest-c-phase1", "digest-c-phase2"),
            )
            result = recompute(**{k: v for k, v in paths.items()})

            identity = result["hygiene"]["identity"]
            self.assertTrue(identity["c"]["violated"])
            self.assertFalse(identity["c"]["agree"])
            self.assertIn("disagree", identity["c"]["reason"])
            self.assertFalse(identity["m"]["violated"])  # unaffected
            self.assertTrue(result["hygiene"]["violated"])
            self.assertEqual(result["verdict"], "INVALID")
            self.assertIsNone(result["e1"]["delta_min"])
            self.assertIsNone(result["e1"]["se_boot"])
            self.assertIsNone(result["e1"]["headroom"])


if __name__ == "__main__":
    unittest.main()
