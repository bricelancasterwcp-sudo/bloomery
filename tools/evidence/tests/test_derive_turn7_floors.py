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

import json
import unittest
from pathlib import Path

from tools.evidence.derive_turn7_floors import V5_MIXED_SHA256, derive, hold_floor, improvement_floor

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


class BaselineIdentityTest(unittest.TestCase):
    """F-2 (adversarial review, 2026-08-29): the comparator's identity is
    pinned like the instrument's -- a wrong --baseline must be a hard
    error, never silently different floors."""

    def test_a_wrong_baseline_file_is_a_hard_error(self):
        wrong = REPO / "docs/superpowers/evidence/2026-08-29-g5v5-stock14b-boot1-recompute.json"
        with self.assertRaises(SystemExit):
            derive(wrong, FIXTURES)

    def test_a_saturated_baseline_is_a_named_error_not_a_bare_stopiteration(self):
        with self.assertRaises(ValueError) as ctx:
            improvement_floor(32, 32)
        self.assertIn("saturated", str(ctx.exception))


class EvaluateModeTest(unittest.TestCase):
    """The battery's mechanical floor verdict: floors read from the
    derivation report, subject refused unless its instrument-row binding
    is clean, no human arithmetic at verdict time."""

    @staticmethod
    def _subject(**mutations):
        subject = {
            "instrument_rows": {"expected": 32, "seen": 32, "duplicates": [], "unknown": [], "missing": []},
            "join": {"violations": []},
            "g4": {"landed": 20, "n": 20},
            "g5": {"set": "codec-tasks-v5-mixed", "fixtures_sha256": V5_MIXED_SHA256,
                   "patch": {"landed": 15, "n": 16}, "refuse": {"landed": 16, "n": 16}},
            "declarations": {
                "outcome_consistent": {"consistent": 31},
                "evidence_grounded": {"grounded": 20},
                "reason_matches_family": {"by_family": {
                    "symptom-mismatch": {"match": 4},
                    "defect-absent": {"match": 5},
                    "missing-target": {"match": 5},
                }},
            },
        }
        subject.update(mutations)
        return subject

    def _evaluate(self, subject):
        import tempfile

        from tools.evidence.derive_turn7_floors import evaluate

        report = derive(BASELINE, FIXTURES)
        with tempfile.TemporaryDirectory() as raw:
            path = Path(raw) / "subject.json"
            path.write_text(json.dumps(subject), encoding="utf-8")
            return evaluate(report, path)

    def test_a_passing_subject_passes_every_floor(self):
        evaluation = self._evaluate(self._subject())
        self.assertTrue(evaluation["all_pass"])
        self.assertEqual({k: v["pass"] for k, v in evaluation["checks"].items()},
                         {k: True for k in evaluation["checks"]})

    def test_one_floor_miss_fails_the_whole_verdict(self):
        subject = self._subject()
        subject["declarations"]["reason_matches_family"]["by_family"]["symptom-mismatch"]["match"] = 2
        evaluation = self._evaluate(subject)
        self.assertFalse(evaluation["all_pass"])
        self.assertFalse(evaluation["checks"]["F4_symptom_mismatch_match"]["pass"])
        self.assertTrue(evaluation["checks"]["F5_evidence_grounded"]["pass"])

    def test_a_wrong_leg_denominator_fails_its_floor(self):
        subject = self._subject(g4={"landed": 19, "n": 19})
        evaluation = self._evaluate(subject)
        self.assertFalse(evaluation["checks"]["F1_g4"]["pass"])

    def test_a_duplicated_instrument_row_refuses_the_verdict(self):
        subject = self._subject()
        subject["instrument_rows"] = {"expected": 32, "seen": 33,
                                      "duplicates": ["v5-patch-find-py-01"], "unknown": [], "missing": []}
        with self.assertRaises(SystemExit):
            self._evaluate(subject)

    def test_a_subject_without_the_binding_is_refused(self):
        subject = self._subject()
        del subject["instrument_rows"]
        with self.assertRaises(SystemExit):
            self._evaluate(subject)

    def test_a_subject_scored_against_other_fixture_bytes_is_refused(self):
        subject = self._subject()
        subject["g5"]["fixtures_sha256"] = "0" * 64
        with self.assertRaises(SystemExit):
            self._evaluate(subject)

    def test_a_structurally_incomplete_subject_is_a_named_refusal(self):
        subject = self._subject(g4=None)
        with self.assertRaises(SystemExit):
            self._evaluate(subject)


if __name__ == "__main__":
    unittest.main()
