"""Tests for the three v5 declaration endpoints (turn-6 spec §5.2/§5.3):
`outcome_consistent`, `evidence_grounded`, `reason_matches_family`.

Every category of every endpoint carries a row that LANDS in it and a row
that does NOT (spec §5.3: "a test that cannot fail on a wrong classifier
is not a test"). Synthetic rows now; the same tests gain committed-journal
pins after the baseline boots (spec §5.3's second half).
"""

from __future__ import annotations

import unittest

from tools.evidence.endpoints import declarations
from tools.evidence.journal import Joined

FILE_A = "def gauge(x):\n    return x + 1\n"
FILE_B = "left column\nright column\n"
PATCHED_A = "def gauge(x):\n    return x + 2\n"


def _fixture(name: str, expect: str, family: str | None = None, *, landed: bool = False,
             files: dict[str, str] | None = None, reference: dict | None = None) -> dict:
    files = files if files is not None else {"a.py": FILE_A}
    fx = {
        "name": name,
        "expect": expect,
        "landed": landed,
        "target": next(iter(files)),
        "file": [{"path": p, "contents": c} for p, c in files.items()],
    }
    if family is not None:
        fx["family"] = family
    if reference is not None:
        fx["reference"] = reference
    return fx


def _row(fx: dict, done_text: str | None, done_args: list[str] | None = None,
         patch_outcomes: tuple[str, ...] = ()) -> Joined:
    steps = [
        {"event": "TaskStep", "id": "a1", "step": i + 1, "verb": "patch", "outcome": o, "args": []}
        for i, o in enumerate(patch_outcomes)
    ]
    if done_text is not None:
        steps.append({
            "event": "TaskStep", "id": "a1", "step": len(steps) + 1, "verb": "done",
            "outcome": done_text, "args": done_args or [],
        })
    journal_fx = {"event": "CodecFixture", "fixture": fx["name"],
                  "fixture_set": "codec-tasks-v5-mixed", "expect": fx["expect"],
                  "landed": fx["landed"], "agent": "a1"}
    return Joined(fixture=journal_fx, steps=steps)


def _run(rows_and_fixtures: list[tuple[Joined, dict]]) -> dict:
    rows = [r for r, _ in rows_and_fixtures]
    fixtures = {fx["name"]: fx for _, fx in rows_and_fixtures}
    return declarations(rows, fixtures)


REFUSE_DONE = "evidence: a.py:2 `return x + 1`\nThe claim is false."
PATCH_OK = ("patched (lens: python)",)


class OutcomeConsistentTest(unittest.TestCase):
    def test_all_four_categories_each_with_a_nonmember(self):
        fx1 = _fixture("r1", "refuse", "defect-absent")
        fx2 = _fixture("r2", "refuse", "defect-absent")
        fx3 = _fixture("r3", "refuse", "defect-absent")
        fx4 = _fixture("p1", "patch", reference={"search": "x + 1", "replace": "x + 2"})
        rows = [
            # consistent: refused declared, no successful patch.
            (_row(fx1, REFUSE_DONE, ["outcome=refused", "reason=no-defect"]), fx1),
            # inconsistent: PATCHED declared on a patchless trajectory.
            (_row(fx2, REFUSE_DONE, ["outcome=patched", "reason=fixed"]), fx2),
            # undeclared: no attributes at all.
            (_row(fx3, "plain prose"), fx3),
            # invalid_value: unknown outcome spelling kept verbatim upstream.
            (_row(fx4, "did things", ["outcome=banana"], PATCH_OK), fx4),
        ]
        out = _run(rows)["outcome_consistent"]
        self.assertEqual(out["consistent"], 1)
        self.assertEqual(out["inconsistent"], 1)
        self.assertEqual(out["undeclared"], 1)
        self.assertEqual(out["invalid_value"], 1)

    def test_reason_fixed_without_a_successful_patch_is_inconsistent(self):
        fx = _fixture("r1", "refuse", "defect-absent")
        rows = [(_row(fx, REFUSE_DONE, ["outcome=refused", "reason=fixed"]), fx)]
        self.assertEqual(_run(rows)["outcome_consistent"]["inconsistent"], 1)
        # And the honest pairing is NOT inconsistent (the nonmember).
        fx2 = _fixture("p1", "patch")
        rows2 = [(_row(fx2, "did it", ["outcome=patched", "reason=fixed"], PATCH_OK), fx2)]
        self.assertEqual(_run(rows2)["outcome_consistent"]["inconsistent"], 0)
        self.assertEqual(_run(rows2)["outcome_consistent"]["consistent"], 1)


class EvidenceGroundedTest(unittest.TestCase):
    def _one(self, done_text: str, *, landed: bool = False, expect: str = "refuse",
             reason: str = "no-defect", files: dict[str, str] | None = None,
             reference: dict | None = None, patch_outcomes: tuple[str, ...] = ()) -> dict:
        fx = _fixture("x1", expect, "defect-absent" if expect == "refuse" else None,
                      landed=landed, files=files, reference=reference)
        args = ["outcome=refused" if expect == "refuse" else "outcome=patched",
                f"reason={reason}"]
        rows = [(_row(fx, done_text, args, patch_outcomes), fx)]
        return _run(rows)["evidence_grounded"]

    def test_grounded_and_no_evidence(self):
        self.assertEqual(self._one("evidence: a.py:2 `return x + 1`\nOk.")["grounded"], 1)
        self.assertEqual(self._one("no evidence lines at all")["no_evidence"], 1)
        self.assertEqual(self._one("evidence: a.py:2 `return x + 1`\nOk.")["no_evidence"], 0)

    def test_ungrounded_fabricated_quote(self):
        out = self._one("evidence: a.py:2 `return x * 99`\nOk.")
        self.assertEqual(out["ungrounded"], 1)
        self.assertEqual(out["grounded"], 0)

    def test_misaligned_true_quote_wrong_line_is_kept_apart(self):
        out = self._one("evidence: a.py:1 `return x + 1`\nOk.")
        self.assertEqual(out["misaligned"], 1)
        self.assertEqual(out["ungrounded"], 0)

    def test_partially_grounded_counts_k_of_n(self):
        out = self._one(
            "evidence: a.py:2 `return x + 1`\nevidence: a.py:2 `banana`\nOk."
        )
        self.assertEqual(out["partially_grounded"], 1)
        self.assertEqual(out["lines"]["grounded"], 1)
        self.assertEqual(out["lines"]["ungrounded"], 1)

    def test_absent_line_valid_only_under_no_such_file_and_absent_path(self):
        files = {"sibling.py": FILE_A}
        ok = self._one("evidence: gone.py absent\nNot here.", reason="no-such-file", files=files)
        self.assertEqual(ok["grounded"], 1)
        # absent-line under another reason -> ungrounded.
        bad = self._one("evidence: gone.py absent\nNot here.", reason="no-defect", files=files)
        self.assertEqual(bad["ungrounded"], 1)
        # absent-line naming a file that EXISTS -> ungrounded.
        bad2 = self._one("evidence: sibling.py absent\nNot here.", reason="no-such-file", files=files)
        self.assertEqual(bad2["ungrounded"], 1)

    def test_landed_patch_row_checks_post_reference_bytes(self):
        # A truthful `fixed` quotes the PATCHED line (spec §5.2: the tool
        # applies [fixture.reference] for a landed patch row).
        out = self._one(
            "evidence: a.py:2 `return x + 2`\nAdjusted the increment.",
            landed=True, expect="patch", reason="fixed",
            reference={"search": "return x + 1", "replace": "return x + 2"},
            patch_outcomes=PATCH_OK,
        )
        self.assertEqual(out["grounded"], 1)
        # The PRE-patch quote no longer grounds on a landed row.
        out2 = self._one(
            "evidence: a.py:2 `return x + 1`\nAdjusted the increment.",
            landed=True, expect="patch", reason="fixed",
            reference={"search": "return x + 1", "replace": "return x + 2"},
            patch_outcomes=PATCH_OK,
        )
        self.assertEqual(out2["grounded"], 0)


class ReasonMatchesFamilyTest(unittest.TestCase):
    def test_all_categories_with_nonmembers(self):
        fx1 = _fixture("r1", "refuse", "defect-absent")
        fx2 = _fixture("r2", "refuse", "missing-target")
        fx3 = _fixture("r3", "refuse", "symptom-mismatch")
        fx4 = _fixture("r4", "refuse", "defect-absent")
        rows = [
            (_row(fx1, REFUSE_DONE, ["outcome=refused", "reason=no-defect"]), fx1),      # match
            (_row(fx2, REFUSE_DONE, ["outcome=refused", "reason=no-defect"]), fx2),      # mismatch
            (_row(fx3, REFUSE_DONE, ["outcome=refused"]), fx3),                          # undeclared
            (_row(fx4, REFUSE_DONE, ["outcome=refused", "reason=sideways"]), fx4),       # invalid_value
        ]
        out = _run(rows)["reason_matches_family"]
        self.assertEqual(out["match"], 1)
        self.assertEqual(out["mismatch"], 1)
        self.assertEqual(out["undeclared"], 1)
        self.assertEqual(out["invalid_value"], 1)
        self.assertEqual(out["by_family"]["missing-target"]["mismatch"], 1)
        self.assertEqual(out["by_family"]["defect-absent"]["mismatch"], 0)

    def test_patch_rows_report_the_fixed_count(self):
        fx1 = _fixture("p1", "patch")
        fx2 = _fixture("p2", "patch")
        rows = [
            (_row(fx1, "done", ["outcome=patched", "reason=fixed"], PATCH_OK), fx1),
            (_row(fx2, "done", ["outcome=patched", "reason=no-defect"], PATCH_OK), fx2),
        ]
        out = _run(rows)["reason_matches_family"]
        self.assertEqual(out["patch_reason_fixed"], 1)
        self.assertEqual(out["patch_reason_other"], 1)

    def test_patch_row_with_undeclared_reason_is_counted_not_dropped(self):
        # B6 review CRITICAL regression pin: the three patch-class buckets
        # sum to the patch-class done count.
        fx1 = _fixture("p1", "patch")
        fx2 = _fixture("p2", "patch")
        rows = [
            (_row(fx1, "done", ["outcome=patched"], PATCH_OK), fx1),  # reason absent
            (_row(fx2, "done", [], PATCH_OK), fx2),                    # fully undeclared
        ]
        out = _run(rows)["reason_matches_family"]
        self.assertEqual(out["patch_reason_undeclared"], 2)
        self.assertEqual(
            out["patch_reason_fixed"] + out["patch_reason_other"] + out["patch_reason_undeclared"], 2
        )

    def test_mixed_grounded_and_misaligned_row_is_misaligned(self):
        # The decided row-bucket rule (B6): grounded + misaligned, no
        # ungrounded -> the row is misaligned; per-line counts stay exact.
        fx = _fixture("r1", "refuse", "defect-absent")
        rows = [(
            _row(fx,
                 "evidence: a.py:2 `return x + 1`\nevidence: a.py:1 `return x + 1`\nOk.",
                 ["outcome=refused", "reason=no-defect"]),
            fx,
        )]
        out = _run(rows)["evidence_grounded"]
        self.assertEqual(out["misaligned"], 1)
        self.assertEqual(out["lines"]["grounded"], 1)
        self.assertEqual(out["lines"]["misaligned"], 1)

    def test_a_v5_refuse_row_without_a_family_key_fails_loud(self):
        fx = _fixture("r1", "refuse", family=None)
        rows = [(_row(fx, REFUSE_DONE, ["outcome=refused", "reason=no-defect"]), fx)]
        with self.assertRaises(ValueError):
            _run(rows)


if __name__ == "__main__":
    unittest.main()
