"""Recompute gates: cost-journal failure, arm completeness, the golden
bootstrap and the expected-digest pin.

Split out of `test_recompute.py` on 2026-09-01 (carried-debt slice D).
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

            def write_arm(
                arm_dir: Path, ledger_path: Path, arm: str, digest: str, cost: int, mode: str
            ) -> None:
                ledger_rows = list(_identity_rows(arm, digest, digest))
                tasks_journal: list[dict[str, Any]] = []
                boot: list[dict[str, Any]] = []
                for phase in (1, 2):
                    for name in names:
                        task_id = f"{arm}-{phase}-{name}-tid"
                        agent_id = f"{arm}-{phase}-{name}-agent"
                        ledger_rows.append(_ledger_row(arm, phase, name, task_id))
                        tasks_journal.append(_memory_stamp(agent_id, task_id, mode))
                        tasks_journal.append(_task_step_done(agent_id))
                        # z1/C/phase2's agent gets everything EXCEPT an
                        # InferCompleted row -- the case under test.
                        if not (arm == "C" and phase == 2 and name == "z1"):
                            boot.append(_infer_completed(agent_id, cost, cost + 1))
                _write_jsonl(ledger_path, ledger_rows)
                _write_jsonl(arm_dir / "journal" / "tasks.jsonl", tasks_journal)
                _write_jsonl(arm_dir / "journal" / "boot-0001.jsonl", boot)

            # Treatment-legal modes per arm (branch-review finding I-2):
            # arm C memory-off, arm M memory-on.
            write_arm(arm_c_dir, ledger_c, "C", "digest-c", 50, "off")
            write_arm(arm_m_dir, ledger_m, "M", "digest-m", 20, "silent")

            result = recompute(corpus_dir, arm_c_dir, arm_m_dir, ledger_c, ledger_m)

            self.assertNotIn("z1", result["advisory"]["costs"]["c"]["p2"])
            dropped_c = result["dropped"]["C"]
            matching = [e for e in dropped_c if e["task"] == "z1" and e["phase"] == 2]
            self.assertEqual(len(matching), 1)
            self.assertTrue(matching[0]["infra"])
            self.assertIn("no InferCompleted rows", matching[0]["reason"])



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


if __name__ == "__main__":
    unittest.main()
