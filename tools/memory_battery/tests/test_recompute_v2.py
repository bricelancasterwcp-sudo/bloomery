"""Recompute v2: the arithmetic fixture, the A1 wall, and the G1/G2 verdicts.

Split 2026-09-01 (carried-debt slice D): this module was 875 lines. The stamp
audit, the H2/H3 endpoints, arm-label honesty and the golden bootstrap are in
`test_recompute_v2_verdicts.py`; the fixtures both share are in
`_recompute_v2_fixtures.py`.

The fixture names `test_recompute_v2_cli.py` imports from this module
(`CONSTANT_50`, `TASKS`, `_build_fixture`) stay importable from here, since
this module re-exports them from the shared fixtures.
"""

from __future__ import annotations
import json
import unittest
from pathlib import Path
from tempfile import TemporaryDirectory
from typing import Any
from tools.memory_battery.recompute_v2 import B_V2, SEED_V2, recompute_v2

from tools.memory_battery.tests._recompute_v2_fixtures import (  # noqa: F401
    CONSTANT_50,
    TASKS,
    _build_fixture,
    _identity_rows,
    _infer_completed,
    _ledger_row,
    _memory_mint,
    _memory_stamp,
    _task_step_done,
    _write_arm,
    _write_jsonl,
    _write_manifest,
)


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


if __name__ == "__main__":
    unittest.main()
