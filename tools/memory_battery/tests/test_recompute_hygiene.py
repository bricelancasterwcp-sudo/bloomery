"""Recompute hygiene: violations, Error-status infra rows, malformed cost rows,
and treatment identity.

These are the paths where a row must NOT be scored: an infra failure is never
a model's poor result, and a malformed row is named rather than quietly
skipped.

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



class ErrorStatusInfraTests(unittest.TestCase):
    """Design spec §4 H3, verbatim: "`Error` statuses, daemon faults,
    driver-detected protocol breaks -- always counted separately from task
    conduct, never scored as cost data; those tasks are `dropped` for E1."
    An errored task-half still writes perfectly real journal rows (its
    agent got as far as some inference before dying), so nothing but the
    ledger's own status distinguishes it -- and joining its cost anyway
    lets a cheap crash masquerade as a cheap success in the median."""

    NAMES = ["e0", "e1", "e2", "e3", "e4"]
    # Arm C, phase 2: e0 ends "Error" with a huge REAL cost; e1 ends
    # "StepsExhausted" (task CONDUCT -- ITT-included, pays its real cost).
    C_P2 = {"e0": 999, "e1": 10, "e2": 20, "e3": 30, "e4": 40}
    C_P2_STATUS = {"e0": "Error", "e1": "StepsExhausted"}

    def _build(self, tmp: Path) -> dict[str, Path]:
        corpus_dir = tmp / "corpus"
        _write_manifest(corpus_dir, self.NAMES)
        arm_c_dir, arm_m_dir = tmp / "arm_c", tmp / "arm_m"
        ledger_c, ledger_m = tmp / "ledger_c.jsonl", tmp / "ledger_m.jsonl"

        ledger_c_rows = list(_identity_rows("C", "digest-c", "digest-c"))
        journal_c: list[dict[str, Any]] = []
        boot_c: list[dict[str, Any]] = []
        for phase in (1, 2):
            for name in self.NAMES:
                task_id = f"c-{phase}-{name}-tid"
                agent_id = f"c-{phase}-{name}-agent"
                status = self.C_P2_STATUS.get(name, "Done") if phase == 2 else "Done"
                ledger_c_rows.append(_ledger_row("C", phase, name, task_id, status=status))
                # Every task-half, INCLUDING the errored one, gets a full
                # set of real journal rows -- that is the whole point: only
                # the ledger status tells them apart.
                journal_c.append(_memory_stamp(agent_id, task_id, "off"))
                journal_c.append(_task_step_done(agent_id))
                cost = 50 if phase == 1 else self.C_P2[name]
                boot_c.append(_infer_completed(agent_id, cost, cost + 1))
        _write_jsonl(ledger_c, ledger_c_rows)
        _write_jsonl(arm_c_dir / "journal" / "tasks.jsonl", journal_c)
        _write_jsonl(arm_c_dir / "journal" / "boot-0001.jsonl", boot_c)

        ledger_m_rows = list(_identity_rows("M", "digest-m", "digest-m"))
        journal_m: list[dict[str, Any]] = []
        boot_m: list[dict[str, Any]] = []
        for phase in (1, 2):
            for name in self.NAMES:
                task_id = f"m-{phase}-{name}-tid"
                agent_id = f"m-{phase}-{name}-agent"
                ledger_m_rows.append(_ledger_row("M", phase, name, task_id))
                journal_m.append(_memory_stamp(agent_id, task_id, "silent"))
                journal_m.append(_task_step_done(agent_id))
                boot_m.append(_infer_completed(agent_id, 50 if phase == 1 else 20, 51))
        _write_jsonl(ledger_m, ledger_m_rows)
        _write_jsonl(arm_m_dir / "journal" / "tasks.jsonl", journal_m)
        _write_jsonl(arm_m_dir / "journal" / "boot-0001.jsonl", boot_m)

        return {
            "corpus_dir": corpus_dir,
            "arm_c_dir": arm_c_dir,
            "arm_m_dir": arm_m_dir,
            "ledger_c": ledger_c,
            "ledger_m": ledger_m,
        }

    def test_error_status_half_is_dropped_counts_as_infra_and_leaves_the_median(self) -> None:
        with TemporaryDirectory() as tmp_str:
            result = recompute(**self._build(Path(tmp_str)))

            # (1) Never in `measurements` -- its real 999 cost is absent.
            costs_c_p2 = result["advisory"]["costs"]["c"]["p2"]
            self.assertNotIn("e0", costs_c_p2)
            self.assertEqual(costs_c_p2, {"e1": 10, "e2": 20, "e3": 30, "e4": 40})

            # (2) Dropped with a named, Error-specific reason, infra:True.
            matching = [e for e in result["dropped"]["C"] if e["task"] == "e0" and e["phase"] == 2]
            self.assertEqual(len(matching), 1)
            self.assertTrue(matching[0]["infra"])
            self.assertIn("Error status", matching[0]["reason"])

            # (3) Counts toward H3 like every other drop.
            h3 = result["hygiene"]["h3_infra_rate"]
            self.assertEqual(h3["c_infra_count"], 1)
            self.assertEqual(h3["m_infra_count"], 0)
            self.assertGreater(h3["c_infra_rate"], 0.05)  # 1/10 -> H3 kill
            self.assertTrue(h3["violated"])
            self.assertEqual(result["verdict"], "INVALID")

            # (4) E1's median EXCLUDES it: median([10,20,30,40]) == 25.0,
            # not median([10,20,30,40,999]) == 30.
            self.assertEqual(result["e1"]["median_c_p2"], 25.0)

    def test_other_terminal_statuses_are_conduct_and_still_pay_their_cost(self) -> None:
        """Only `Error` is infra. `StepsExhausted` (and `Done`,
        `BudgetExhausted`, `WindowExhausted`) are task CONDUCT: intent-to-
        treat says they contribute their real cost."""
        with TemporaryDirectory() as tmp_str:
            result = recompute(**self._build(Path(tmp_str)))
            self.assertIn("e1", result["advisory"]["costs"]["c"]["p2"])
            self.assertEqual(result["advisory"]["costs"]["c"]["p2"]["e1"], 10)
            dropped_names = {e["task"] for e in result["dropped"]["C"]}
            self.assertEqual(dropped_names, {"e0"})



class MalformedCostRowTests(unittest.TestCase):
    """`row.get("completion_tokens", 0)` silently priced an unreadable
    `InferCompleted` row at zero. Under a UNIFORM drift (a serialization
    change dropping the field everywhere) both arms read as "every task
    cost 0" -- medians 0 vs 0, `delta_min` 0.0, verdict PASS. Fail loud
    instead, matching `_read_jsonl`'s own corrupt-journal contract."""

    def test_infercompleted_row_missing_completion_tokens_raises_naming_journal_and_row(self) -> None:
        with TemporaryDirectory() as tmp_str:
            tmp = Path(tmp_str)
            paths = _build_clean_fixture(
                tmp, CONSTANT_50, CONSTANT_50, CONSTANT_50, {n: 20 for n in TASKS}
            )
            boot_path = paths["arm_c_dir"] / "journal" / "boot-0001.jsonl"
            rows = [json.loads(line) for line in boot_path.read_text(encoding="utf-8").splitlines()]
            self.assertIn("completion_tokens", rows[0])
            del rows[0]["completion_tokens"]  # the malformed row
            _write_jsonl(boot_path, rows)

            with self.assertRaises(ValueError) as ctx:
                recompute(**{k: v for k, v in paths.items()})

            message = str(ctx.exception)
            self.assertIn("boot-0001.jsonl", message)  # names the journal
            self.assertIn("completion_tokens", message)  # names the field
            self.assertIn(rows[0]["id"], message)  # names the row's agent

    def test_uniform_drift_no_longer_manufactures_a_pass(self) -> None:
        """The probe shape the finding named: EVERY `InferCompleted` row in
        BOTH arms loses the field. Previously: medians 0 vs 0, delta_min
        0.0, verdict PASS. Now: a hard, named failure."""
        with TemporaryDirectory() as tmp_str:
            tmp = Path(tmp_str)
            paths = _build_clean_fixture(
                tmp, CONSTANT_50, CONSTANT_50, CONSTANT_50, {n: 20 for n in TASKS}
            )
            for arm_dir in (paths["arm_c_dir"], paths["arm_m_dir"]):
                boot_path = arm_dir / "journal" / "boot-0001.jsonl"
                rows = [json.loads(line) for line in boot_path.read_text(encoding="utf-8").splitlines()]
                for row in rows:
                    row.pop("completion_tokens", None)
                _write_jsonl(boot_path, rows)

            with self.assertRaises(ValueError):
                recompute(**{k: v for k, v in paths.items()})



class TreatmentIdentityTests(unittest.TestCase):
    """Nothing else in the hygiene chain can see a C/M transposition or a
    mis-configured arm C: the journals are well-formed, the digests agree,
    H1/H2/H3 are all clean -- and E1 is silently INVERTED (the "control"
    was the treated arm) or ERASED (both arms memory-off). Checked from the
    data itself: the realized `MemoryStamp` mode per arm, and the `arm`
    label the driver wrote on every ledger row."""

    M_P2 = {n: 20 for n in TASKS}

    def test_clean_fixture_treatment_identity_not_violated(self) -> None:
        with TemporaryDirectory() as tmp_str:
            paths = _build_clean_fixture(
                Path(tmp_str), CONSTANT_50, CONSTANT_50, CONSTANT_50, self.M_P2
            )
            result = recompute(**{k: v for k, v in paths.items()})
            treatment = result["hygiene"]["treatment_identity"]
            self.assertFalse(treatment["violated"])
            self.assertFalse(treatment["c"]["violated"])
            self.assertFalse(treatment["m"]["violated"])
            self.assertEqual(treatment["c"]["observed_arm_labels"], ["C"])
            self.assertEqual(treatment["m"]["observed_arm_labels"], ["M"])
            self.assertEqual(treatment["c"]["allowed_modes"], ["off"])
            self.assertEqual(treatment["m"]["allowed_modes"], ["silent", "injected"])
            self.assertEqual(treatment["c"]["offending_stamps"], [])
            self.assertEqual(result["verdict"], "PASS")  # unchanged by the new check

    def test_transposed_arms_yield_invalid(self) -> None:
        """The whole battery run passed into the wrong slots: M's journals
        and ledger in the C slot and vice versa. The join still succeeds
        (each ledger matches its own journals), digests still self-agree,
        H1/H2/H3 all pass -- only treatment identity catches it."""
        with TemporaryDirectory() as tmp_str:
            paths = _build_clean_fixture(
                Path(tmp_str), CONSTANT_50, CONSTANT_50, CONSTANT_50, self.M_P2
            )
            result = recompute(
                paths["corpus_dir"],
                paths["arm_m_dir"],  # M's data in the C slot
                paths["arm_c_dir"],  # C's data in the M slot
                paths["ledger_m"],
                paths["ledger_c"],
            )

            treatment = result["hygiene"]["treatment_identity"]
            self.assertTrue(treatment["violated"])
            self.assertTrue(treatment["c"]["violated"])
            self.assertTrue(treatment["m"]["violated"])
            self.assertEqual(treatment["c"]["observed_arm_labels"], ["M"])
            self.assertEqual(treatment["m"]["observed_arm_labels"], ["C"])
            self.assertIn("transposed", treatment["c"]["reason"])
            self.assertEqual(
                {entry["mode"] for entry in treatment["c"]["offending_stamps"]}, {"silent"}
            )
            self.assertEqual({entry["mode"] for entry in treatment["m"]["offending_stamps"]}, {"off"})
            self.assertTrue(result["hygiene"]["violated"])
            self.assertEqual(result["verdict"], "INVALID")
            self.assertIsNone(result["e1"]["delta_min"])

    def test_arm_c_with_injected_stamps_yields_invalid(self) -> None:
        """A mis-configured arm C -- booted `[memory] enabled = true`, so
        its phase-2 stamps read `injected`. Everything else is clean; the
        ledger labels are correct, so ONLY the mode half of the check
        fires."""
        with TemporaryDirectory() as tmp_str:
            paths = _build_clean_fixture(
                Path(tmp_str),
                CONSTANT_50,
                CONSTANT_50,
                CONSTANT_50,
                self.M_P2,
                modes_c=("off", "injected"),
            )
            result = recompute(**{k: v for k, v in paths.items()})

            treatment = result["hygiene"]["treatment_identity"]
            self.assertTrue(treatment["c"]["violated"])
            self.assertFalse(treatment["m"]["violated"])
            self.assertEqual(treatment["c"]["observed_arm_labels"], ["C"])  # labels fine
            offending = treatment["c"]["offending_stamps"]
            self.assertEqual(len(offending), len(TASKS))  # every phase-2 stamp
            self.assertEqual({entry["phase"] for entry in offending}, {2})
            self.assertEqual({entry["mode"] for entry in offending}, {"injected"})
            self.assertIn("treatment identity", treatment["c"]["reason"])
            self.assertEqual(result["verdict"], "INVALID")
            self.assertIsNone(result["e1"]["delta_min"])

    def test_wrong_ledger_arm_label_alone_yields_invalid(self) -> None:
        """Only the ledger's `arm` label is wrong (modes are treatment-
        legal) -- isolating the label half of the check."""
        with TemporaryDirectory() as tmp_str:
            paths = _build_clean_fixture(
                Path(tmp_str),
                CONSTANT_50,
                CONSTANT_50,
                CONSTANT_50,
                self.M_P2,
                ledger_arm_c="M",
            )
            result = recompute(**{k: v for k, v in paths.items()})

            treatment = result["hygiene"]["treatment_identity"]
            self.assertTrue(treatment["c"]["violated"])
            self.assertFalse(treatment["m"]["violated"])
            self.assertEqual(treatment["c"]["observed_arm_labels"], ["M"])
            self.assertEqual(treatment["c"]["offending_stamps"], [])  # modes fine
            self.assertIn("expected exactly ['C']", treatment["c"]["reason"])
            self.assertEqual(result["verdict"], "INVALID")
            self.assertIsNone(result["e1"]["delta_min"])


if __name__ == "__main__":
    unittest.main()
