"""Per-template evidence ground-truth proofs for the 8 target-present
refusal templates (turn-7 spec §2.1–§2.2).

`RefusalTask.evidence` is the v5 ideal's training-signal contract: for a
defect-absent task the one triple quotes the CHECKED-CORRECT line the
goal's false claim is about; for a symptom-mismatch task it quotes the
REAL defect Y's line — the same `site` ground truth the template spends
on `refusal_reason`. Every assertion below re-derives the pinned line
from the generated artifacts (file, goal, refusal_reason), never from the
template's internal variables — the same technique, and for the same
reason, as `DefectAbsentClaimIsProvablyFalseTest` and the Y-present
proofs in `test_templates_symptom_mismatch.py`.

A new module, not more of `test_templates_refusal.py` or
`test_templates_symptom_mismatch.py`: both sit at the 400-line house cap.
"""

import random
import re
import unittest

from tools.flywheel.factory import templates
from tools.flywheel.factory.task import validate_refusal_task
from tools.flywheel.factory.templates_refusal_python import (
    _defect_absent_wrong_comparison_py,
    _defect_absent_wrong_multiplier_py,
)
from tools.flywheel.factory.templates_refusal_text import (
    _defect_absent_config_value_txt,
    _defect_absent_version_string_txt,
)
from tools.flywheel.factory.templates_symptom_mismatch_python import (
    _symptom_mismatch_dropped_last_reading_py,
    _symptom_mismatch_truncated_average_py,
)
from tools.flywheel.factory.templates_symptom_mismatch_text import (
    _symptom_mismatch_duplicate_key_txt,
    _symptom_mismatch_escalation_loop_txt,
)

TARGET_PRESENT_GROUPS = (
    "defect_absent_python", "defect_absent_plaintext",
    "symptom_mismatch_python", "symptom_mismatch_plaintext",
)
MISSING_TARGET_GROUPS = ("missing_target_python", "missing_target_plaintext")
SEEDS = range(60)


def _evidence_and_lines(task):
    """The task's single triple plus the target's split lines — every pin
    below starts from these."""
    path, line_no, quote = task.evidence[0]
    return path, line_no, quote, task.files[task.target].splitlines()


class EvidenceStructuralContractTest(unittest.TestCase):
    """The shape every target-present draw must have: validates clean,
    exactly one triple, path is the target, and the quote sits on exactly
    the recorded 1-based line of the generated file (re-derived here, not
    trusted from the triple)."""

    def test_every_target_present_draw_carries_one_rederivable_triple(self):
        for group in TARGET_PRESENT_GROUPS:
            for name, fn in templates.REFUSAL_GROUPS[group]:
                for seed in SEEDS:
                    task = fn(random.Random(seed))
                    violations = validate_refusal_task(task)
                    self.assertEqual(violations, [], f"{name} seed={seed}: {violations}")
                    self.assertEqual(len(task.evidence), 1, f"{name} seed={seed}: {task.evidence}")
                    path, line_no, quote, lines = _evidence_and_lines(task)
                    self.assertEqual(path, task.target, f"{name} seed={seed}")
                    hits = [i + 1 for i, line in enumerate(lines) if quote in line]
                    self.assertEqual(
                        hits, [line_no],
                        f"{name} seed={seed}: quote {quote!r} occurs on line(s) {hits}, triple says {line_no}",
                    )

    def test_missing_target_draws_carry_no_evidence_and_still_validate(self):
        # The 4 missing-target templates are deliberately untouched: their
        # `evidence: <target> absent` line is mechanical (spec §2.1), so
        # the field stays at its default.
        for group in MISSING_TARGET_GROUPS:
            for name, fn in templates.REFUSAL_GROUPS[group]:
                for seed in SEEDS:
                    task = fn(random.Random(seed))
                    self.assertEqual(task.evidence, (), f"{name} seed={seed}")
                    self.assertEqual(validate_refusal_task(task), [], f"{name} seed={seed}")


class DefectAbsentEvidencePinTest(unittest.TestCase):
    """The semantic half for defect-absent: the quoted line is the
    checked-correct line — it holds the identifier/value the goal's false
    claim quotes, re-derived from the goal text."""

    def test_wrong_multiplier_quotes_the_line_with_the_real_factor(self):
        claim_re = re.compile(r"instead of `([\d.]+)`")
        for seed in SEEDS:
            task = _defect_absent_wrong_multiplier_py(random.Random(seed))
            _path, line_no, _quote, lines = _evidence_and_lines(task)
            match = claim_re.search(task.goal)
            self.assertIsNotNone(match, f"seed={seed}: no backticked real factor in {task.goal!r}")
            self.assertIn(
                f"return value * {match.group(1)}", lines[line_no - 1],
                f"seed={seed}: evidence line is not the checked-correct multiplier line",
            )

    def test_wrong_comparison_quotes_the_line_with_the_real_direction(self):
        claim_re = re.compile(r"instead of the (\w+) one")
        for seed in SEEDS:
            task = _defect_absent_wrong_comparison_py(random.Random(seed))
            _path, line_no, _quote, lines = _evidence_and_lines(task)
            match = claim_re.search(task.goal)
            self.assertIsNotNone(match, f"seed={seed}: no stated real direction in {task.goal!r}")
            expected_op = ">" if match.group(1) == "highest" else "<"
            op_match = re.search(r"if x ([<>]) best:", lines[line_no - 1])
            self.assertIsNotNone(op_match, f"seed={seed}: evidence line is not the comparison line")
            self.assertEqual(
                op_match.group(1), expected_op,
                f"seed={seed}: quoted comparison contradicts the goal's stated real direction",
            )

    def test_config_value_quotes_the_key_line_that_meets_the_claimed_floor(self):
        key_re = re.compile(r"`(\w+)` in")
        floor_re = re.compile(r"at least (\d+)")
        for seed in SEEDS:
            task = _defect_absent_config_value_txt(random.Random(seed))
            _path, line_no, _quote, lines = _evidence_and_lines(task)
            key, floor = key_re.search(task.goal), floor_re.search(task.goal)
            self.assertIsNotNone(key, f"seed={seed}: no backticked key in {task.goal!r}")
            self.assertIsNotNone(floor, f"seed={seed}: no claimed floor in {task.goal!r}")
            entry = re.fullmatch(rf"{re.escape(key.group(1))} = (\d+)", lines[line_no - 1])
            self.assertIsNotNone(entry, f"seed={seed}: evidence line is not the claimed key's line")
            self.assertGreater(
                int(entry.group(1)), int(floor.group(1)),
                f"seed={seed}: quoted value does not beat the claimed floor -- not the checked-correct line",
            )

    def test_version_string_quotes_the_heading_with_the_real_tag(self):
        claim_re = re.compile(r"tagged `([^`]+)`")
        for seed in SEEDS:
            task = _defect_absent_version_string_txt(random.Random(seed))
            _path, line_no, _quote, lines = _evidence_and_lines(task)
            match = claim_re.search(task.goal)
            self.assertIsNotNone(match, f"seed={seed}: no backticked real tag in {task.goal!r}")
            heading_lines = [i + 1 for i, line in enumerate(lines) if line.startswith("## ")]
            self.assertEqual(heading_lines, [line_no], f"seed={seed}: evidence line is not the heading")
            self.assertTrue(
                lines[line_no - 1].startswith(f"## {match.group(1)} "),
                f"seed={seed}: heading does not carry the goal's quoted real tag",
            )


class SymptomMismatchEvidencePinTest(unittest.TestCase):
    """The semantic half for symptom-mismatch: the quoted line is planted
    defect Y's line — the same ground truth the Y-present proofs in
    `test_templates_symptom_mismatch.py` execute, and the same `site` the
    refusal_reason names."""

    def test_dropped_last_reading_quotes_the_off_by_one_loop_line(self):
        site = "for i in range(len(readings) - 1)"
        for seed in SEEDS:
            task = _symptom_mismatch_dropped_last_reading_py(random.Random(seed))
            _path, line_no, _quote, lines = _evidence_and_lines(task)
            y_lines = [i + 1 for i, line in enumerate(lines) if site in line]
            self.assertEqual(y_lines, [line_no], f"seed={seed}: evidence line is not the off-by-one bound")
            self.assertIn(site, task.refusal_reason, f"seed={seed}: reason and evidence disagree on the site")

    def test_truncated_average_quotes_the_floor_division_line(self):
        site = "sum(samples) // len(samples)"
        for seed in SEEDS:
            task = _symptom_mismatch_truncated_average_py(random.Random(seed))
            _path, line_no, _quote, lines = _evidence_and_lines(task)
            y_lines = [i + 1 for i, line in enumerate(lines) if "//" in line]
            self.assertEqual(y_lines, [line_no], f"seed={seed}: evidence line is not the floor division")
            self.assertIn(site, task.refusal_reason, f"seed={seed}: reason and evidence disagree on the site")

    def test_duplicate_key_quotes_the_second_declaration_of_the_duplicated_key(self):
        entry_re = re.compile(r"^(\S+) = (\d+)$", re.MULTILINE)
        for seed in SEEDS:
            task = _symptom_mismatch_duplicate_key_txt(random.Random(seed))
            _path, line_no, quote, lines = _evidence_and_lines(task)
            values_by_key: dict[str, list[str]] = {}
            for key, value in entry_re.findall(task.files[task.target]):
                values_by_key.setdefault(key, []).append(value)
            duplicated = {k: v for k, v in values_by_key.items() if len(v) > 1}
            self.assertEqual(len(duplicated), 1, f"seed={seed}: expected exactly one duplicated key")
            [(dup_key, dup_values)] = duplicated.items()
            second_lines = [i + 1 for i, line in enumerate(lines) if line == f"{dup_key} = {dup_values[1]}"]
            self.assertEqual(
                second_lines, [line_no],
                f"seed={seed}: evidence line is not the duplicated key's second declaration",
            )
            self.assertEqual(quote, f"{dup_key} = {dup_values[1]}", f"seed={seed}")
            self.assertIn(dup_key, task.refusal_reason, f"seed={seed}")

    def test_escalation_loop_quotes_the_escalate_line_naming_the_owner(self):
        owner_re = re.compile(r"^Owner: (\S+)$", re.MULTILINE)
        for seed in SEEDS:
            task = _symptom_mismatch_escalation_loop_txt(random.Random(seed))
            _path, line_no, _quote, lines = _evidence_and_lines(task)
            owner = owner_re.search(task.files[task.target])
            self.assertIsNotNone(owner, f"seed={seed}: no owner line")
            escalate_lines = [i + 1 for i, line in enumerate(lines) if line.startswith("Escalate to: ")]
            self.assertEqual(escalate_lines, [line_no], f"seed={seed}: evidence line is not the escalation entry")
            self.assertEqual(
                lines[line_no - 1], f"Escalate to: {owner.group(1)}",
                f"seed={seed}: quoted escalation does not loop back to the owner -- not defect Y",
            )


class ValidatorTargetPresentEvidenceRuleTest(unittest.TestCase):
    """`validate_refusal_task`'s family-conditional rule (spec §2.2): a
    target-present task must carry >= 1 triple whose path IS the target.
    Exercised through real template draws so the rule is proven against
    the shapes the factory actually emits."""

    def test_stripping_evidence_from_a_target_present_draw_is_a_violation(self):
        for group in TARGET_PRESENT_GROUPS:
            for name, fn in templates.REFUSAL_GROUPS[group]:
                bad = fn(random.Random(7))._replace(evidence=())
                violations = validate_refusal_task(bad)
                self.assertTrue(
                    any("evidence_line_of" in v for v in violations),
                    f"{name}: no evidence-rule violation in {violations}",
                )

    def test_a_triple_for_a_sibling_path_does_not_satisfy_the_rule(self):
        # Per-triple valid (the sibling is a real file and the quote is on
        # the recorded line) but pointing at the WRONG file: the rule is
        # about the target's ground truth, not about having any triple.
        task = _symptom_mismatch_truncated_average_py(random.Random(7))
        files = dict(task.files, **{"aside.txt": "note: aside\n"})
        bad = task._replace(files=files, evidence=(("aside.txt", 1, "note: aside"),))
        violations = validate_refusal_task(bad)
        self.assertTrue(any("evidence_line_of" in v for v in violations), violations)

    def test_a_stale_triple_line_is_still_caught_by_the_per_triple_recheck(self):
        # The pre-existing re-check stays load-bearing beside the new
        # rule: a triple whose path IS the target but whose recorded line
        # no longer holds the quote must not survive validation.
        task = _symptom_mismatch_truncated_average_py(random.Random(7))
        path, line_no, quote = task.evidence[0]
        bad = task._replace(evidence=((path, line_no - 1, quote),))
        violations = validate_refusal_task(bad)
        self.assertTrue(any("never hand-counted" in v for v in violations), violations)


if __name__ == "__main__":
    unittest.main()
