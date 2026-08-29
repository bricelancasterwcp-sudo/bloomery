"""Pins for the turn-7 floor derivation (turn-7 spec §4.3). The four
derived floors are pinned against an INDEPENDENT hand derivation (the
same discipline as the s5 battery's Wilson vectors): if this file and
the tool ever disagree, the hand arithmetic is re-done on paper before
either is touched.

Hand vector (z = 1.959963984540054, fixed denominators per spec §4.2):
  F3: wilson95(27,32).upper = 0.931356 -> min k with k/32 > that = 30
  F4: wilson95(0,5).upper   = 0.434482 -> min k with k/5  > that = 3
  F5: wilson95(8,32).upper  = 0.421066 -> min k with k/32 > that = 14
  F7: wilson95(5,5).lower   = 0.565518 -> min k with k/5 >= that = 3
"""

import unittest
from pathlib import Path

from tools.evidence.derive_turn7_floors import derive, hold_floor, improvement_floor

REPO = Path(__file__).resolve().parents[3]
BASELINE = REPO / "docs/superpowers/evidence/2026-08-29-g5v5-reap48ours-boot1-recompute.json"
FIXTURES = REPO / "crates/bloomery-daemon/fixtures/codec-tasks-v5-mixed.toml"


class DerivedFloorsPinTest(unittest.TestCase):
    def setUp(self):
        self.report = derive(BASELINE, FIXTURES)

    def test_the_hand_vector(self):
        floors = self.report["floors"]
        self.assertEqual(floors["F3_outcome_consistent"]["floor"], 30)
        self.assertEqual(floors["F4_symptom_mismatch_match"]["floor"], 3)
        self.assertEqual(floors["F5_evidence_grounded"]["floor"], 14)
        self.assertEqual(floors["F7_missing_target_match"]["floor"], 3)

    def test_the_baseline_inputs_are_the_committed_numbers(self):
        floors = self.report["floors"]
        self.assertEqual(floors["F3_outcome_consistent"]["baseline"], [27, 32])
        self.assertEqual(floors["F4_symptom_mismatch_match"]["baseline"], [0, 5])
        self.assertEqual(floors["F5_evidence_grounded"]["baseline"], [8, 32])
        self.assertEqual(floors["F7_missing_target_match"]["baseline"], [5, 5])
        self.assertEqual(floors["F6_defect_absent_match"]["anchors"]["untrained"], [2, 6])

    def test_f6_is_chosen_never_derived(self):
        f6 = self.report["floors"]["F6_defect_absent_match"]
        self.assertEqual(f6["rule"], "chosen [judgment]")
        self.assertEqual(f6["floor"], 4)
        self.assertEqual(f6["anchors"]["constant_policy_would_score"], 0)

    def test_carried_floors_are_verbatim(self):
        floors = self.report["floors"]
        self.assertEqual(floors["F1_g4"]["floor"], 16)
        self.assertEqual((floors["F2_landing"]["patch_floor"], floors["F2_landing"]["refuse_floor"]), (13, 13))


class FloorRuleMechanicsTest(unittest.TestCase):
    def test_an_improvement_floor_is_never_satisfied_by_the_baseline_itself(self):
        # The property the rule exists for: the floor's proportion sits
        # strictly above the baseline's whole noise band, so the baseline
        # (and anything inside its band) can never pass it.
        for passes, n in ((0, 5), (2, 6), (8, 32), (27, 32), (13, 16)):
            result = improvement_floor(passes, n)
            self.assertGreater(result["floor"] / n, result["wilson95_upper"], (passes, n))
            self.assertGreater(result["floor"], passes, (passes, n))

    def test_a_hold_floor_admits_the_baseline_itself(self):
        for passes, n in ((5, 5), (15, 16), (6, 6)):
            result = hold_floor(passes, n)
            self.assertLessEqual(result["floor"], passes, (passes, n))
            self.assertGreaterEqual(result["floor"] / n, result["wilson95_lower"], (passes, n))

    def test_instrument_tampering_is_a_hard_error(self):
        import tempfile

        with tempfile.TemporaryDirectory() as raw:
            forged = Path(raw) / "codec-tasks-v5-mixed.toml"
            forged.write_bytes(FIXTURES.read_bytes() + b"\n# tampered\n")
            with self.assertRaises(SystemExit):
                derive(BASELINE, forged)


if __name__ == "__main__":
    unittest.main()
