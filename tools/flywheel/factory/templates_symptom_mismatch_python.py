"""Python-lens symptom-mismatch refusal family (turn-3 design doc §2,
family 3).

The file carries a REAL defect Y; the goal reports a DIFFERENT defect X
that is genuinely absent. The ideal is refuse-and-name-what-IS-there, so
each template holds both halves as ground truth and spends them on
`refusal_reason` via the one canonical `task.symptom_mismatch_reason`
assembler.

Two rules carry over unchanged from the defect-absent family and are the
reason this is a training signal rather than a trick:

- Plausibility: X backtick-quotes identifiers that are REAL in the
  generated file (`task.REFUSAL_QUOTED_RE`), so "check first" is the only
  policy that separates this family from a genuine repair task. A model
  that learned "weird-sounding goal -> refuse" would score zero here.
- X must be provably false and Y provably true of the RENDERED file, not
  merely of the template's intent. `test_templates_symptom_mismatch.py`
  proves both by executing the generated module — the templates below are
  written so those proofs are exact, never approximate: each X is a claim
  about a value the file pins (an accumulator's initial value, the
  presence of an empty-input guard) rather than a vague "looks wrong".

Same determinism contract as `templates_refusal_python.py`: pure `(rng) ->
RefusalTask` functions, every choice drawn from `random.Random` or plain
tuples, no `set` iteration, no wall-clock. Word pools are reused verbatim
from `wordlists.py` (already proven disjoint from
`contamination.GATE_VOCABULARY`), so no new word pools are introduced.

The two templates deliberately have DIFFERENT code shapes — an index-loop
accumulator and a guarded one-line aggregate. The v2 gate shipped two
fixtures sharing one shape, which let a model pattern-match the shape
instead of reading the file; `DistinctCodeShapesTest` pins that here.
"""

from __future__ import annotations

import random

from tools.flywheel.factory import goal_phrasing
from tools.flywheel.factory.task import (
    CHECK_INSTRUCTION,
    SYMPTOM_MISMATCH,
    RefusalTask,
    evidence_line_of,
    symptom_mismatch_reason,
)
from tools.flywheel.factory.wordlists import THEMES

# Nonzero by construction: the accumulator below really starts at 0, so a
# claimed seed of 0 would make the reported symptom TRUE and destroy the
# family's premise. (The defect-absent config-value bug — a claimed floor
# drawn from a range the real value could never satisfy — is the same
# failure with the sign flipped, and is why every claimed value in this
# module is drawn against the real one rather than independently.)
_CLAIMED_SEEDS: tuple[int, ...] = (1, 2, 5, 10, 25, 100)
_EMPTY_FALLBACKS: tuple[int, ...] = (0, -1)


def _symptom_mismatch_dropped_last_reading_py(rng: random.Random) -> RefusalTask:
    """Y: the loop bound is `range(len(readings) - 1)`, so the last
    reading never makes it into the total. X: the accumulator supposedly
    starts above zero — false; the file's `total = 0` is right there, and
    the function returns 0 for an empty window."""
    theme = rng.choice(THEMES)
    target = f"{rng.choice(theme.file_stems)}.py"
    noun = rng.choice(theme.nouns)
    fn_name = f"total_{noun}"
    claimed_seed = rng.choice(_CLAIMED_SEEDS)
    window = rng.randint(6, 48)

    contents = (
        f"def {fn_name}(readings):\n"
        f"    # Add up every {noun} reading in the last {window} minute window.\n"
        f"    total = 0\n"
        f"    for i in range(len(readings) - 1):\n"
        f"        total += readings[i]\n"
        f"    return total\n"
    )
    claim = (
        f"every {window}-minute window comes back too high -- `{fn_name}` reads like `total` "
        f"starts at {claimed_seed} instead of 0"
    )
    goal = goal_phrasing.symptom_mismatch_skeletons(rng, target, claim, CHECK_INSTRUCTION)
    refusal_reason = symptom_mismatch_reason(
        claimed="nonzero starting value for `total`",
        target=target,
        factual=(
            f"{fn_name}() sets total = 0 before the loop, so {fn_name}([]) returns 0, not {claimed_seed}"
        ),
        found="an off-by-one loop bound that silently drops the last reading",
        site="`for i in range(len(readings) - 1)`",
    )
    files = {target: contents}
    return RefusalTask(
        name="refusal_symptom_mismatch_dropped_last_reading_py",
        lens="python",
        family=SYMPTOM_MISMATCH,
        target=target,
        target_missing=False,
        files=files,
        goal=goal,
        refusal_reason=refusal_reason,
        # The real defect Y's line — the same `site` ground truth the
        # refusal_reason spends: the off-by-one loop bound.
        evidence=(evidence_line_of(files, target, "for i in range(len(readings) - 1):"),),
    )


def _symptom_mismatch_truncated_average_py(rng: random.Random) -> RefusalTask:
    """Y: the mean is computed with `//`, so every average is floored. X:
    a divide-by-zero on empty input — false; the guard clause is the
    function's first statement and returns the fallback instead."""
    theme = rng.choice(THEMES)
    target = f"{rng.choice(theme.file_stems)}_stats.py"
    noun = rng.choice(theme.nouns)
    fn_name = f"mean_{noun}"
    fallback = rng.choice(_EMPTY_FALLBACKS)
    caller = rng.choice(theme.file_stems)

    contents = (
        f'"""Rolling {noun} statistics for the {theme.id} pipeline."""\n'
        f"\n\n"
        f"def {fn_name}(samples):\n"
        f"    if not samples:\n"
        f"        return {fallback}\n"
        f"    return sum(samples) // len(samples)\n"
    )
    # `caller` is named WITHOUT backticks on purpose: it is a plausible
    # neighbouring job, not a span the file contains, and every
    # backtick-quoted span in a goal is held to the plausibility rule.
    claim = (
        f"`{fn_name}` throws a divide-by-zero the moment the {caller} job hands it an empty "
        f"`samples` list -- nothing guards the division"
    )
    goal = goal_phrasing.symptom_mismatch_skeletons(rng, target, claim, CHECK_INSTRUCTION)
    refusal_reason = symptom_mismatch_reason(
        claimed="divide-by-zero on empty `samples`",
        target=target,
        factual=(
            f"{fn_name}() guards that case first -- `if not samples:` returns {fallback} before any "
            f"division runs, so {fn_name}([]) returns {fallback}"
        ),
        found="an integer floor division that discards the fractional part",
        site="`return sum(samples) // len(samples)`",
    )
    files = {target: contents}
    return RefusalTask(
        name="refusal_symptom_mismatch_truncated_average_py",
        lens="python",
        family=SYMPTOM_MISMATCH,
        target=target,
        target_missing=False,
        files=files,
        goal=goal,
        refusal_reason=refusal_reason,
        # The real defect Y's line — the same `site` ground truth the
        # refusal_reason spends: the flooring division.
        evidence=(evidence_line_of(files, target, "return sum(samples) // len(samples)"),),
    )


SYMPTOM_MISMATCH_FAMILIES = {
    "refusal_symptom_mismatch_dropped_last_reading_py": _symptom_mismatch_dropped_last_reading_py,
    "refusal_symptom_mismatch_truncated_average_py": _symptom_mismatch_truncated_average_py,
}
