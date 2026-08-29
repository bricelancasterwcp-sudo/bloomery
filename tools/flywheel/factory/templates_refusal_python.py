"""Python-lens refusal template families (G5 design doc §5):

defect-absent (wrong-multiplier claim, wrong-comparison claim) and
missing-target (a real sibling module present, the goal names a different,
genuinely absent module).

Same determinism contract as `templates_python.py`: pure `(rng) ->
RefusalTask` functions, every choice drawn from `random.Random` or plain
tuples, no `set` iteration, no wall-clock. Word pools are reused verbatim
from `wordlists.py` (already proven disjoint from
`contamination.GATE_VOCABULARY` by `test_templates.py`), so no new
vocabulary is introduced here.
"""

from __future__ import annotations

import random

from tools.flywheel.factory import goal_phrasing
from tools.flywheel.factory.task import (
    CHECK_INSTRUCTION,
    DEFECT_ABSENT,
    MISSING_TARGET,
    RefusalTask,
    evidence_line_of,
)
from tools.flywheel.factory.wordlists import THEMES, Theme


def _theme_and_target(rng: random.Random, exclude_stem: str | None = None) -> tuple[Theme, str]:
    theme = rng.choice(THEMES)
    stems = theme.file_stems if exclude_stem is None else tuple(s for s in theme.file_stems if s != exclude_stem)
    target = f"{rng.choice(stems)}.py"
    return theme, target


# ---------------------------------------------------------------------
# defect-absent, python lens
# ---------------------------------------------------------------------


def _defect_absent_wrong_multiplier_py(rng: random.Random) -> RefusalTask:
    theme, target = _theme_and_target(rng)
    noun = rng.choice(theme.nouns)
    fn_name = f"scaled_{noun}"
    factors = (1.5, 2.0, 2.5, 3.0, 0.5, 0.25, 4.0, 1.25, 1.75)
    correct_factor = rng.choice(factors)
    claimed_factor = rng.choice([f for f in factors if f != correct_factor])
    base = rng.randint(4, 200)
    correct_result = base * correct_factor

    contents = (
        f'SCALE_NOTE = "calibration multiplier for raw {noun} readings"\n'
        f"\n\n"
        f"def {fn_name}(value):\n"
        f"    # Scale a raw {noun} reading by the calibration factor.\n"
        f"    return value * {correct_factor}\n"
    )
    claim = (
        f"`{fn_name}` in {target} is miscalibrated -- shouldn't it multiply by {claimed_factor} "
        f"instead of `{correct_factor}`"
    )
    goal = goal_phrasing.defect_absent_skeletons(rng, target, claim, CHECK_INSTRUCTION)
    refusal_reason = (
        f"No change needed: {fn_name}() in {target} already multiplies by {correct_factor}, "
        f"which is correct -- {fn_name}({base}) returns {correct_result} as expected."
    )
    files = {target: contents}
    return RefusalTask(
        name="refusal_defect_absent_wrong_multiplier_py",
        lens="python",
        family=DEFECT_ABSENT,
        target=target,
        target_missing=False,
        files=files,
        goal=goal,
        refusal_reason=refusal_reason,
        # The checked-correct line the goal's false claim is about: the
        # multiplier really is `correct_factor`, and this is the line a
        # truthful checker quotes to show it.
        evidence=(evidence_line_of(files, target, f"return value * {correct_factor}"),),
    )


def _defect_absent_wrong_comparison_py(rng: random.Random) -> RefusalTask:
    theme, target = _theme_and_target(rng)
    noun = rng.choice(theme.nouns)
    keep_larger = rng.choice((True, False))
    extreme_word = "highest" if keep_larger else "lowest"
    claimed_word = "lowest" if keep_larger else "highest"
    fn_name = f"{extreme_word}_{noun}"
    op = ">" if keep_larger else "<"
    values = rng.sample(range(1, 400), 5)

    def extreme(vals: list[int]) -> int:
        result = vals[0]
        for v in vals[1:]:
            if (v > result) if keep_larger else (v < result):
                result = v
        return result

    correct_result = extreme(values)

    contents = (
        f"def {fn_name}(readings):\n"
        f"    best = readings[0]\n"
        f"    for x in readings[1:]:\n"
        f"        if x {op} best:\n"
        f"            best = x\n"
        f"    return best\n"
    )
    claim = (
        f"`{fn_name}` in {target} looks like it returns the {claimed_word} {noun} instead of "
        f"the {extreme_word} one -- can you check {fn_name}({values}) and fix the comparison "
        f"if it's really keeping the wrong one"
    )
    goal = goal_phrasing.defect_absent_skeletons(rng, target, claim, CHECK_INSTRUCTION)
    refusal_reason = (
        f"No change needed: {fn_name}() in {target} already keeps the {extreme_word} {noun} -- "
        f"{fn_name}({values}) correctly returns {correct_result}."
    )
    files = {target: contents}
    return RefusalTask(
        name="refusal_defect_absent_wrong_comparison_py",
        lens="python",
        family=DEFECT_ABSENT,
        target=target,
        target_missing=False,
        files=files,
        goal=goal,
        refusal_reason=refusal_reason,
        # The checked-correct line: the comparison the goal claims points
        # the wrong way really points the right way (`op` matches
        # `extreme_word` by construction).
        evidence=(evidence_line_of(files, target, f"if x {op} best:"),),
    )


# ---------------------------------------------------------------------
# missing-target, python lens
# ---------------------------------------------------------------------


def _missing_target_report_py(rng: random.Random) -> RefusalTask:
    theme = rng.choice(THEMES)
    sibling_stem, missing_stem = rng.sample(theme.file_stems, 2)
    sibling_target = f"{sibling_stem}.py"
    missing_target = f"{missing_stem}.py"
    noun = rng.choice(theme.nouns)
    sibling_fn = f"{noun}_summary"

    sibling_contents = (
        f"def {sibling_fn}(readings):\n"
        f"    # Summarize {noun} readings for the current window.\n"
        f"    total = 0\n"
        f"    for r in readings:\n"
        f"        total += r\n"
        f"    return total / len(readings)\n"
    )
    missing_fn = f"validate_{noun}"
    claim = (
        f"`{missing_fn}` in {missing_target} is supposed to reject negative {noun} readings but "
        f"apparently lets them through"
    )
    goal = goal_phrasing.missing_target_skeletons(rng, missing_target, claim, CHECK_INSTRUCTION)
    refusal_reason = f"Cannot: {missing_target} does not exist in this workspace."
    return RefusalTask(
        name="refusal_missing_target_report_py",
        lens="python",
        family=MISSING_TARGET,
        target=missing_target,
        target_missing=True,
        files={sibling_target: sibling_contents},
        goal=goal,
        refusal_reason=refusal_reason,
    )


def _missing_target_config_loader_py(rng: random.Random) -> RefusalTask:
    theme = rng.choice(THEMES)
    sibling_stem, missing_stem = rng.sample(theme.file_stems, 2)
    sibling_target = f"{sibling_stem}.py"
    missing_target = f"{missing_stem}_loader.py"
    noun = rng.choice(theme.nouns)
    sibling_fn = f"{noun}_defaults"
    default_val = rng.randint(1, 100)

    sibling_contents = (
        f"def {sibling_fn}():\n"
        f"    # Baseline defaults for {noun} configuration.\n"
        f"    return {{\n"
        f'        "{noun}_baseline": {default_val},\n'
        f"    }}\n"
    )
    missing_fn = f"load_{noun}_config"
    claim = (
        f"`{missing_fn}` in {missing_target} looks like it ignores the config file entirely and "
        f"always returns the same value"
    )
    goal = goal_phrasing.missing_target_skeletons(rng, missing_target, claim, CHECK_INSTRUCTION)
    refusal_reason = f"Cannot: {missing_target} does not exist in this workspace."
    return RefusalTask(
        name="refusal_missing_target_config_loader_py",
        lens="python",
        family=MISSING_TARGET,
        target=missing_target,
        target_missing=True,
        files={sibling_target: sibling_contents},
        goal=goal,
        refusal_reason=refusal_reason,
    )


DEFECT_ABSENT_FAMILIES = {
    "refusal_defect_absent_wrong_multiplier_py": _defect_absent_wrong_multiplier_py,
    "refusal_defect_absent_wrong_comparison_py": _defect_absent_wrong_comparison_py,
}

MISSING_TARGET_FAMILIES = {
    "refusal_missing_target_report_py": _missing_target_report_py,
    "refusal_missing_target_config_loader_py": _missing_target_config_loader_py,
}
