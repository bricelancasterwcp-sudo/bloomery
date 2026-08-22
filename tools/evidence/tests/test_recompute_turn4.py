"""The recompute tool must reproduce the committed turn-4 evidence exactly."""
import json
import unittest
from pathlib import Path

from tools.evidence.recompute import recompute

ROOT = Path(__file__).resolve().parents[3]
EV = ROOT / "docs/superpowers/evidence"
FIX = ROOT / "crates/bloomery-daemon/fixtures"
V4 = FIX / "codec-tasks-v4-mixed.toml"


def run(tag):
    return recompute(journal=EV / f"2026-08-21-{tag}-journal.jsonl",
                     tasks=EV / f"2026-08-21-{tag}-tasks.jsonl",
                     g5_fixtures=V4, g4_set="codec-tasks-v1")


class Flywheel4Battery(unittest.TestCase):
    def test_boot1_g4_only(self):
        r = run("flywheel4-g4")
        self.assertEqual((r["g4"]["landed"], r["g4"]["n"]), (20, 20))
        self.assertAlmostEqual(r["g4"]["wilson95"][0], 0.8388748419471806, places=12)
        self.assertFalse(r["g4"]["provisional"])
        self.assertTrue(r["g4"]["journaled_verdict_matches"])
        self.assertEqual((r["join"]["fixtures"], r["join"]["groups"]), (20, 20))
        self.assertEqual(r["join"]["violations"], [])
        self.assertIsNone(r["g5"])
        self.assertEqual(r["verb_histogram"], {"done": 20, "patch": 20, "read": 20})

    def test_boot2_g5(self):
        r = run("flywheel4-g5")
        self.assertEqual(r["join"]["mode"], "ordinal")
        self.assertEqual(r["join"]["violations"], [])
        self.assertEqual((r["g4"]["landed"], r["g4"]["n"]), (20, 20))
        self.assertEqual((r["g5"]["patch"]["landed"], r["g5"]["refuse"]["landed"]), (16, 16))
        self.assertFalse(r["g5"]["patch"]["provisional"]); self.assertFalse(r["g5"]["refuse"]["provisional"])
        self.assertAlmostEqual(r["g5"]["patch"]["wilson95"][0], 0.8063923194655636, places=12)
        self.assertTrue(r["g5"]["journaled_verdict_matches"])
        self.assertEqual(r["composition"], {"find": [6, 6], "run": [5, 5], "plain": [5, 5],
                                            "defect-absent": [6, 6], "missing-target": [5, 5], "symptom-mismatch": [5, 5]})
        e = r["endpoints"]
        self.assertEqual(e["productive_find"], [6, 6]); self.assertEqual(e["find_usage"], [6, 6])
        self.assertEqual(e["malformed_find"], [0, 6]); self.assertEqual(e["run_before_done"], [5, 5])
        self.assertEqual(e["any_run"], [5, 5]); self.assertEqual(e["productive_run"], [5, 5])
        self.assertEqual(e["reason_grounding"], {"eligible": 11, "landed_eligible": 11, "measured_rows": 4,
                                                 "unmeasured_rows": 7, "grounded": 6, "spans": 6})
        self.assertEqual(r["grant_violation_rows"], 0)
        self.assertEqual(r["verb_histogram"], {"done": 52, "find": 6, "patch": 36, "read": 52, "run": 5})


class G5v4Baselines(unittest.TestCase):
    def test_flywheel3_at_v4(self):
        r = run("g5v4-flywheel3")
        self.assertEqual((r["g4"]["landed"], r["g5"]["patch"]["landed"], r["g5"]["refuse"]["landed"]), (20, 15, 16))
        self.assertTrue(r["g5"]["patch"]["provisional"]); self.assertFalse(r["g5"]["refuse"]["provisional"])
        self.assertEqual(r["composition"], {"find": [5, 6], "run": [5, 5], "plain": [5, 5],
                                            "defect-absent": [6, 6], "missing-target": [5, 5], "symptom-mismatch": [5, 5]})
        e = r["endpoints"]
        self.assertEqual(e["productive_find"], [5, 6]); self.assertEqual(e["find_usage"], [6, 6])
        self.assertEqual(e["malformed_find"], [0, 6]); self.assertEqual(e["run_before_done"], [5, 5])
        self.assertEqual(e["any_run"], [5, 5]); self.assertEqual(e["productive_run"], [0, 5])
        self.assertEqual(e["reason_grounding"], {"eligible": 11, "landed_eligible": 11, "measured_rows": 5,
                                                 "unmeasured_rows": 6, "grounded": 16, "spans": 19})
        self.assertEqual(r["grant_violation_rows"], 5)
        self.assertEqual(r["verb_histogram"], {"done": 52, "find": 6, "patch": 35, "read": 51, "run": 5})

    def test_stock14b_at_v4(self):
        r = run("g5v4-stock14b")
        self.assertEqual((r["g4"]["landed"], r["g5"]["patch"]["landed"], r["g5"]["refuse"]["landed"]), (6, 5, 8))
        self.assertFalse(r["g4"]["pass"]); self.assertFalse(r["g5"]["patch"]["pass"]); self.assertFalse(r["g5"]["refuse"]["pass"])
        self.assertEqual(r["composition"], {"find": [0, 6], "run": [2, 5], "plain": [3, 5],
                                            "defect-absent": [4, 6], "missing-target": [1, 5], "symptom-mismatch": [3, 5]})
        e = r["endpoints"]
        self.assertEqual(e["productive_find"], [0, 6]); self.assertEqual(e["find_usage"], [6, 6])
        self.assertEqual(e["run_before_done"], [0, 5]); self.assertEqual(e["productive_run"], [0, 5])
        self.assertEqual(e["reason_grounding"], {"eligible": 11, "landed_eligible": 7, "measured_rows": 0,
                                                 "unmeasured_rows": 7, "grounded": 0, "spans": 0})
        self.assertEqual(r["grant_violation_rows"], 42)
        self.assertEqual(r["verb_histogram"], {"done": 32, "find": 9, "patch": 94, "read": 68})


if __name__ == "__main__":
    unittest.main()
