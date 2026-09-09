"""The memory-battery recompute: the arithmetic fixture and the clean gate.

Split 2026-09-01 (carried-debt slice D): this module was 1386 lines. The four
gate/completeness suites are in `test_recompute_gates.py`, and the hygiene,
infra and treatment-identity suites in `test_recompute_hygiene.py`; the
hand-built journal, ledger and manifest fixtures all three share are in
`_recompute_fixtures.py`.
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

from tools.memory_battery.tests._recompute_fixtures import (  # noqa: F401
    CONSTANT_50,
    C_P1_COSTS,
    C_P2_COSTS,
    M_P1_COSTS,
    M_P1_MINTED,
    M_P2_COSTS,
    M_P2_MODES,
    PHASE1,
    TASKS,
    _build_arithmetic_fixture,
    _build_clean_fixture,
    _identity_rows,
    _infer_completed,
    _ledger_row,
    _memory_mint,
    _memory_stamp,
    _task_step_done,
    _write_jsonl,
    _write_manifest,
)


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
                    "violated", "reasons", "arm_completeness", "identity", "treatment_identity",
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
                set(result["hygiene"]["treatment_identity"].keys()), {"c", "m", "violated"}
            )
            self.assertEqual(
                set(result["hygiene"]["treatment_identity"]["c"].keys()),
                {
                    "expected_arm_label", "observed_arm_labels", "allowed_modes",
                    "offending_stamps", "violated", "reason",
                },
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


if __name__ == "__main__":
    unittest.main()
