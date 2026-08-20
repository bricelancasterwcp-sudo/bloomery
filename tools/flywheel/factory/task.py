"""`Task`/`RefusalTask` and structural validation (brief rule 2; G5 design
doc §5 for the refusal shape) — split out of `templates.py` so the
template family modules (`templates_python.py`, `templates_text.py`,
`templates_refusal_python.py`, `templates_refusal_text.py`) can import
`Task`/`RefusalTask`/`DONE_INSTRUCTION`/`CHECK_INSTRUCTION` without a
circular import (`templates.py` imports the family modules to build its
registries).
"""

from __future__ import annotations

import re
from typing import NamedTuple

DONE_INSTRUCTION = "Patch the file, then emit done."
# The refusal shape's counterpart to DONE_INSTRUCTION: every refusal goal
# ends with it, and `validate_refusal_task` asserts that. Canonical HERE,
# beside its repair-shape sibling, rather than declared twice in
# `templates_refusal_python.py`/`templates_refusal_text.py` — two verbatim
# copies with nothing pinning them together is exactly the drift this
# module's single `DONE_INSTRUCTION` already prevents for repair goals.
CHECK_INSTRUCTION = "Check first, and only patch if it is genuinely wrong; then emit done."
MIN_TARGET_LINES = 5
MAX_TARGET_LINES = 60


class Task(NamedTuple):
    """One generated task. `files` always contains at least `target`;
    template families that plant a companion (non-target) file for
    realism add it here too. Immutable — `_replace(...)` builds a
    modified copy rather than mutating in place."""

    name: str
    lens: str  # "python" | "plaintext"
    target: str
    files: dict[str, str]
    goal: str
    search: str
    replace: str
    summary: str


def validate_task(task: Task) -> list[str]:
    """Mirrors codec-tasks-v1's own validator (brief rule 2). Returns a
    list of human-readable violations; empty means the task is
    structurally valid."""
    violations: list[str] = []

    if task.target not in task.files:
        violations.append(f"target {task.target!r} is not among files {sorted(task.files)}")
        return violations  # nothing further can be checked without it

    contents = task.files[task.target]
    occurrences = contents.count(task.search)
    if occurrences != 1:
        violations.append(f"search must appear exactly once in target, found {occurrences} time(s)")

    if task.target not in task.goal:
        violations.append(f"goal does not contain the target filename {task.target!r}")

    if not task.goal.endswith(DONE_INSTRUCTION):
        violations.append(f"goal does not end with the exact instruction {DONE_INSTRUCTION!r}")

    line_count = len(contents.rstrip("\n").split("\n"))
    if not (MIN_TARGET_LINES <= line_count <= MAX_TARGET_LINES):
        violations.append(f"target must be {MIN_TARGET_LINES}-{MAX_TARGET_LINES} lines, got {line_count}")

    if task.search == task.replace:
        violations.append("search must not equal replace")

    return violations


# ---------------------------------------------------------------------------
# G5 refusal families (design doc §5): defect-absent and missing-target;
# turn 3's design doc §2 adds symptom-mismatch.
# ---------------------------------------------------------------------------

DEFECT_ABSENT = "defect_absent"
MISSING_TARGET = "missing_target"
# Turn 3, design doc §2 family 3: the file carries a REAL defect Y and the
# goal reports a DIFFERENT, absent defect X. Structurally this is
# defect-absent's shape (`target_missing=False`, target among `files`) —
# the NamedTuple deliberately gains no Y field, because Y is ground truth
# the TEMPLATE holds while writing `refusal_reason`, not a value any
# consumer of a task needs at runtime. The X-is-false and Y-is-present
# proofs therefore live in `test_templates_symptom_mismatch.py`, asserted
# against each template's generated file.
SYMPTOM_MISMATCH = "symptom_mismatch"
REFUSAL_FAMILIES = (DEFECT_ABSENT, MISSING_TARGET, SYMPTOM_MISMATCH)

# The families whose `target` is a real file among `files` (as opposed to
# missing-target's absent one). They share one validation branch below
# because they share one structural shape — a second copy of the same three
# checks is exactly the drift `CHECK_INSTRUCTION`'s single home prevents
# for the goal instruction.
TARGET_PRESENT_FAMILIES = (DEFECT_ABSENT, SYMPTOM_MISMATCH)

# The plausibility rule's mechanical marker (design doc §5): a goal from a
# TARGET_PRESENT_FAMILIES family (defect-absent, and turn 3's
# symptom-mismatch — both make a claim ABOUT a file that is really there)
# must backtick-quote at least one identifier/value that is a real,
# literal substring of the generated file's contents -- otherwise a model
# could learn "any weird-sounding goal -> refuse" instead of "check the file
# before deciding". Every `templates_refusal_*.py` family wraps its claimed
# identifier/value in backticks for exactly this reason. Public (not a
# leading-underscore private) so tests can reuse the exact same pattern
# rather than re-declaring a second copy that could silently drift.
REFUSAL_QUOTED_RE = re.compile(r"`([^`]+)`")


def symptom_mismatch_reason(*, claimed: str, target: str, factual: str, found: str, site: str) -> str:
    """The ruled two-part `refusal_reason` content for the
    symptom-mismatch family (turn-3 design doc §2, verbatim): what was
    checked and found ABSENT, then what IS actually there.

    Canonical here, beside `CHECK_INSTRUCTION`, for the same reason: two
    verbatim copies of this sentence shape (one per lens module) with
    nothing pinning them together drift the moment one is edited, and the
    wording is a training-signal contract, not decoration — it is the
    literal `done` text the ideal trajectory emits.

    Every argument is ground truth the template holds: `claimed` names the
    absent defect X as a noun phrase, `factual` says why X is not there in
    terms of the file's real content, `found` names the planted defect Y,
    and `site` points at where Y lives."""
    return (
        f"Checked: no {claimed} in {target} — {factual}. "
        f"Found instead: {found} at {site}; no change made without a goal that matches."
    )


class RefusalTask(NamedTuple):
    """One generated refusal task (G5 design doc §5; turn-3 design doc §2
    for the third family). `target_missing` separates the missing-target
    family from the two target-present ones: `False` means `target` is a
    real key in `files` (defect-absent — the file is genuinely correct and
    the goal's claimed defect is false; or symptom-mismatch — the file
    really IS broken, but in a different way than the goal reports);
    `True` means `target` is NOT in `files` (missing-target — `files`
    still holds >= 1 real sibling so the directory is not suspiciously
    empty). Immutable, same reasoning as `Task`."""

    name: str
    lens: str  # "python" | "plaintext"
    family: str  # DEFECT_ABSENT | MISSING_TARGET | SYMPTOM_MISMATCH
    target: str
    target_missing: bool
    files: dict[str, str]
    goal: str
    refusal_reason: str


def validate_refusal_task(task: RefusalTask) -> list[str]:
    """Mirrors `validate_task`'s role for the refusal shape: returns a list
    of human-readable violations; empty means the task is structurally
    valid. Branches on `task.family` for the family-specific rules
    (target-present + plausibility for defect-absent and symptom-mismatch,
    which share one branch because they share one structural shape;
    absence + non-empty dir for missing-target) and applies the shared
    rules (goal names the target, goal ends with `CHECK_INSTRUCTION`,
    non-empty refusal_reason, family/target_missing consistency)
    unconditionally.

    Symptom-mismatch deliberately gets NO extra structural rule: whether
    the claimed defect X is really absent and the planted defect Y really
    present is a property of generated CONTENT, provable only against
    ground truth the template holds — `test_templates_symptom_mismatch.py`
    proves both, per template, by re-deriving them from the rendered file.
    A validator check there would be either vacuous or a second, weaker
    copy of that proof."""
    violations: list[str] = []

    if task.family not in REFUSAL_FAMILIES:
        violations.append(f"unknown refusal family {task.family!r}, expected one of {REFUSAL_FAMILIES}")

    target_in_files = task.target in task.files
    if task.target_missing == target_in_files:
        violations.append(
            f"target_missing={task.target_missing} is inconsistent with target "
            f"{'present in' if target_in_files else 'absent from'} files"
        )

    if task.family in TARGET_PRESENT_FAMILIES:
        # "defect_absent" -> "defect-absent", "symptom_mismatch" ->
        # "symptom-mismatch": the messages stay family-specific (a reader
        # of a violation list should not have to guess which family
        # produced it) without a second hand-maintained label table.
        label = task.family.replace("_", "-")
        if not target_in_files:
            violations.append(f"{label} task's target {task.target!r} must be among files")
        else:
            contents = task.files[task.target]
            quoted = REFUSAL_QUOTED_RE.findall(task.goal)
            if not quoted:
                violations.append(f"{label} goal has no backtick-quoted identifier/value (plausibility rule)")
            elif not any(q in contents for q in quoted):
                violations.append(
                    f"{label} goal's quoted identifier(s) {quoted} do not appear in the "
                    f"target's real contents (plausibility rule: the claimed defect must name a "
                    f"real identifier from the generated file)"
                )
    elif task.family == MISSING_TARGET:
        if target_in_files:
            violations.append(f"missing-target task's target {task.target!r} must NOT be among files")
        if not task.files:
            violations.append("missing-target task must have >= 1 real sibling file present")
        if task.target not in task.refusal_reason:
            violations.append(f"missing-target refusal_reason does not name the missing file {task.target!r}")

    if task.target not in task.goal:
        violations.append(f"goal does not contain the target filename {task.target!r}")

    # The refusal-shape mirror of `validate_task`'s DONE_INSTRUCTION rule.
    # Load-bearing, not cosmetic: the trailing instruction is the ONLY part
    # of a refusal goal that tells the model to look before it leaps, so a
    # family that renders it wrong (or drops it) would teach "weird goal ->
    # refuse" — the exact failure the plausibility rule above also guards.
    if not task.goal.endswith(CHECK_INSTRUCTION):
        violations.append(f"goal does not end with the check-first instruction {CHECK_INSTRUCTION!r}")

    if not task.refusal_reason.strip():
        violations.append("refusal_reason is empty")

    return violations
