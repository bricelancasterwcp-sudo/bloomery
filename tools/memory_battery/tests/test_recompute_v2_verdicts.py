"""Recompute v2: the stamp audit, the H2/H3 endpoints, arm-label honesty and
the golden bootstrap.

Arm-label honesty is the load-bearing one: the labels are case-sensitive by
prereg (`--arm C` / `--arm M`), and a run that spells them otherwise must be
refused rather than silently scored against the wrong arm.

Split out of `test_recompute_v2.py` on 2026-09-01 (carried-debt slice D).
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
