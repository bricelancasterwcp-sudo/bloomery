"""Python-lens template families (brief rule 1: >= 8 families):

wrong comparison operator, off-by-one index, off-by-one range bound,
wrong constant/multiplier, inverted boolean, wrong variable reference,
wrong f-string field, wrong dict key.

Each family is a pure function `(rng) -> Task` — no wall-clock, no `set`
iteration, every choice drawn from `random.Random` or plain tuples so two
calls with an identically-seeded `rng` are byte-identical (brief rule 3).
The planted defect's exact before/after numeric example is computed by
mirroring the buggy vs. fixed logic in plain Python (not string
templating), so the goal's stated symptom is always literally true of the
generated file.
"""

from __future__ import annotations

import random

from tools.flywheel.factory import goal_phrasing
from tools.flywheel.factory.task import DONE_INSTRUCTION, Task
from tools.flywheel.factory.wordlists import (
    DICT_KEY_POOL,
    FLAG_NAMES,
    INDEX_VAR_NAMES,
    THEMES,
    VALUE_HOLDER_NAMES,
    Theme,
)


def _theme_and_target(rng: random.Random) -> tuple[Theme, str]:
    theme = rng.choice(THEMES)
    target = f"{rng.choice(theme.file_stems)}.py"
    return theme, target


def _family_wrong_comparison_operator(rng: random.Random) -> Task:
    theme, target = _theme_and_target(rng)
    noun = rng.choice(theme.nouns)
    keep_larger = rng.choice((True, False))
    extreme_word = "highest" if keep_larger else "lowest"
    wrong_word = "lowest" if keep_larger else "highest"
    fn_name = f"{extreme_word}_{noun}"
    holder = rng.choice(VALUE_HOLDER_NAMES)
    correct_op = ">" if keep_larger else "<"
    wrong_op = "<" if keep_larger else ">"
    values = rng.sample(range(1, 400), 5)

    def extreme(vals: list[int], op: str) -> int:
        result = vals[0]
        for v in vals[1:]:
            beats = (v > result) if op == ">" else (v < result)
            if beats:
                result = v
        return result

    correct_result = extreme(values, correct_op)
    buggy_result = extreme(values, wrong_op)

    contents = (
        f"def {fn_name}(readings):\n"
        f"    {holder} = readings[0]\n"
        f"    for x in readings[1:]:\n"
        f"        if x {wrong_op} {holder}:\n"
        f"            {holder} = x\n"
        f"    return {holder}\n"
    )
    search = f"        if x {wrong_op} {holder}:"
    replace = f"        if x {correct_op} {holder}:"
    goal = goal_phrasing.patch_skeletons(
        rng,
        target,
        subject=f"{fn_name}()",
        problem=f"keeps the {wrong_word} {noun} instead of the {extreme_word} one",
        evidence=f"{fn_name}({values}) returns {buggy_result} instead of {correct_result}",
        fix_target=f"the comparison in {fn_name}() in {target}",
        fix_goal=f"it keeps the {extreme_word} {noun}",
        instruction=DONE_INSTRUCTION,
    )
    summary = f"Fixed the comparison operator in {fn_name}() so it keeps the {extreme_word} {noun}."
    return Task("py_wrong_comparison_operator", "python", target, {target: contents}, goal, search, replace, summary)


def _family_off_by_one_index(rng: random.Random) -> Task:
    """The defect line (and hence `search`) used to be the hardcoded
    literal `"    last_reading = readings[0]"` -- byte-identical on EVERY
    draw regardless of seed. That's a real bug (task 6a's follow-on
    fix): a fixed search string can permanently collide with a frozen
    gate fixture generated from this same family, and no amount of
    rejection-sampling retries can ever clear a collision that has
    probability 1. `param_name` (from `noun`, ~120 reachable values) and
    `holder` (from `VALUE_HOLDER_NAMES`, independent draw, 10 values)
    together parameterize every identifier the defect line touches, the
    same way `_family_wrong_comparison_operator` parameterizes its own
    search line with `holder`/`wrong_op`."""
    theme, target = _theme_and_target(rng)
    noun = rng.choice(theme.nouns)
    holder = rng.choice(VALUE_HOLDER_NAMES)
    fn_name = f"first_and_last_{noun}"
    param_name = f"{noun}_values"
    first_name = f"first_{holder}"
    last_name = f"last_{holder}"
    values = rng.sample(range(1, 500), rng.choice((3, 4, 5)))

    contents = (
        f"def {fn_name}({param_name}):\n"
        f"    # Return the first and last {noun} reading from {param_name}.\n"
        f"    {first_name} = {param_name}[0]\n"
        f"    {last_name} = {param_name}[0]\n"
        f"    return ({first_name}, {last_name})\n"
    )
    search = f"    {last_name} = {param_name}[0]"
    replace = f"    {last_name} = {param_name}[-1]"
    goal = goal_phrasing.patch_skeletons(
        rng,
        target,
        subject=f"{fn_name}()",
        problem=f"returns the first {noun} reading twice instead of the first and last",
        evidence=f"{fn_name}({values}) returns ({values[0]}, {values[0]}) instead of ({values[0]}, {values[-1]})",
        fix_target=f"the last assignment in {fn_name}() in {target}",
        fix_goal=f"it takes the last {noun} reading",
        instruction=DONE_INSTRUCTION,
    )
    summary = f"Fixed {last_name} in {fn_name}() to index from the end."
    return Task("py_off_by_one_index", "python", target, {target: contents}, goal, search, replace, summary)


def _family_off_by_one_range_bound(rng: random.Random) -> Task:
    """Same fix as `_family_off_by_one_index`, above: the defect line
    used to be the hardcoded literal
    `"    for cycle in range(1, count):"`. `loop_var` (from
    `INDEX_VAR_NAMES`, 6 values -- previously unused by any family) and
    `bound_name` (from `noun`, ~120 reachable values, independent draw)
    together parameterize the loop header the defect line is built
    from."""
    theme, target = _theme_and_target(rng)
    noun = rng.choice(theme.nouns)
    loop_var = rng.choice(INDEX_VAR_NAMES)
    fn_name = f"{noun}_checkpoints"
    bound_name = f"{noun}_count"
    n = rng.randint(3, 12)

    contents = (
        f"def {fn_name}({bound_name}):\n"
        f"    markers = []\n"
        f"    for {loop_var} in range(1, {bound_name}):\n"
        f'        markers.append(f"cycle {{{loop_var}}}")\n'
        f"    return markers\n"
    )
    search = f"    for {loop_var} in range(1, {bound_name}):"
    replace = f"    for {loop_var} in range(1, {bound_name} + 1):"
    goal = goal_phrasing.patch_skeletons(
        rng,
        target,
        subject=f"{fn_name}()",
        problem="stops one short of producing a marker for every cycle from 1 through count",
        evidence=f"{fn_name}({n}) yields only {n - 1} markers and cycle {n} never appears",
        fix_target=f"the loop bound in {fn_name}() in {target}",
        fix_goal="the final cycle is included",
        instruction=DONE_INSTRUCTION,
    )
    summary = f"Widened the loop bound in {fn_name}() so the final cycle is included."
    return Task("py_off_by_one_range_bound", "python", target, {target: contents}, goal, search, replace, summary)


def _family_wrong_constant_multiplier(rng: random.Random) -> Task:
    theme, target = _theme_and_target(rng)
    noun = rng.choice(theme.nouns)
    fn_name = f"scaled_{noun}"
    factors = (1.5, 2.0, 2.5, 3.0, 0.5, 0.25, 4.0, 1.25, 1.75)
    correct_factor = rng.choice(factors)
    wrong_factor = rng.choice([f for f in factors if f != correct_factor])
    base = rng.randint(4, 200)
    correct_result = base * correct_factor
    buggy_result = base * wrong_factor

    contents = (
        f'SCALE_NOTE = "calibration multiplier for raw {noun} readings"\n'
        f"\n"
        f"\n"
        f"def {fn_name}(value):\n"
        f"    # Scale a raw {noun} reading by the calibration factor.\n"
        f"    return value * {wrong_factor}\n"
    )
    search = f"    return value * {wrong_factor}"
    replace = f"    return value * {correct_factor}"
    goal = goal_phrasing.patch_skeletons(
        rng,
        target,
        subject=f"{fn_name}()",
        problem="scales by the wrong calibration factor",
        evidence=f"{fn_name}({base}) returns {buggy_result} instead of {correct_result}",
        fix_target=f"{fn_name}() in {target}",
        fix_goal=f"it multiplies by {correct_factor}",
        instruction=DONE_INSTRUCTION,
    )
    summary = f"Fixed the calibration factor in {fn_name}() from {wrong_factor} to {correct_factor}."
    return Task("py_wrong_constant_multiplier", "python", target, {target: contents}, goal, search, replace, summary)


def _family_inverted_boolean(rng: random.Random) -> Task:
    theme, target = _theme_and_target(rng)
    noun_a, noun_b = rng.sample(theme.nouns, 2)
    fn_name = rng.choice(FLAG_NAMES)
    threshold = rng.randint(10, 90)
    wrong_conn = rng.choice(("and", "or"))
    correct_conn = "or" if wrong_conn == "and" else "and"

    def check(a_val: int, b_val: bool, conn: str) -> bool:
        cond1 = a_val >= threshold
        return (cond1 and b_val) if conn == "and" else (cond1 or b_val)

    a_val = rng.randint(0, threshold - 1)  # cond1 always False -> connector alone decides
    b_val = True
    buggy_result = check(a_val, b_val, wrong_conn)
    correct_result = check(a_val, b_val, correct_conn)

    contents = (
        f"def {fn_name}({noun_a}, {noun_b}_ready):\n"
        f"    # Return True when the {noun_a} threshold or {noun_b}_ready qualifies.\n"
        f"    if {noun_a} >= {threshold} {wrong_conn} {noun_b}_ready:\n"
        f"        return True\n"
        f"    return False\n"
    )
    search = f"    if {noun_a} >= {threshold} {wrong_conn} {noun_b}_ready:"
    replace = f"    if {noun_a} >= {threshold} {correct_conn} {noun_b}_ready:"
    goal = goal_phrasing.patch_skeletons(
        rng,
        target,
        subject=f"{fn_name}()",
        problem="requires both conditions instead of either one (or vice versa)",
        evidence=f"{fn_name}({a_val}, True) returns {buggy_result} instead of {correct_result}",
        fix_target=f"the condition in {fn_name}() in {target}",
        fix_goal="the connector is correct",
        instruction=DONE_INSTRUCTION,
    )
    summary = f"Fixed the boolean connector in {fn_name}() from '{wrong_conn}' to '{correct_conn}'."
    return Task("py_inverted_boolean", "python", target, {target: contents}, goal, search, replace, summary)


def _family_wrong_variable_reference(rng: random.Random) -> Task:
    theme, target = _theme_and_target(rng)
    noun_a, noun_b = rng.sample(theme.nouns, 2)
    fn_name = f"combined_{noun_a}_{noun_b}"
    a_val = rng.randint(1, 200)
    b_val = rng.randint(1, 200)
    buggy_result = (a_val * 2) + (a_val * 2)
    correct_result = (a_val * 2) + (b_val * 2)

    contents = (
        f"def {fn_name}({noun_a}, {noun_b}):\n"
        f"    # Combine adjusted {noun_a} and {noun_b} readings.\n"
        f"    adjusted_{noun_a} = {noun_a} * 2\n"
        f"    adjusted_{noun_b} = {noun_b} * 2\n"
        f"    return adjusted_{noun_a} + adjusted_{noun_a}\n"
    )
    search = f"    return adjusted_{noun_a} + adjusted_{noun_a}"
    replace = f"    return adjusted_{noun_a} + adjusted_{noun_b}"
    goal = goal_phrasing.patch_skeletons(
        rng,
        target,
        subject=f"{fn_name}()",
        problem=f"adds the adjusted {noun_a} in twice instead of combining {noun_a} and {noun_b}",
        evidence=f"{fn_name}({a_val}, {b_val}) returns {buggy_result} instead of {correct_result}",
        fix_target=f"the return in {fn_name}() in {target}",
        fix_goal=f"it adds adjusted_{noun_b} instead of repeating adjusted_{noun_a}",
        instruction=DONE_INSTRUCTION,
    )
    summary = f"Fixed {fn_name}() to add adjusted_{noun_b} instead of repeating adjusted_{noun_a}."
    return Task("py_wrong_variable_reference", "python", target, {target: contents}, goal, search, replace, summary)


def _family_wrong_fstring_field(rng: random.Random) -> Task:
    theme, target = _theme_and_target(rng)
    noun_a, noun_b = rng.sample(theme.nouns, 2)
    fn_name = f"describe_{noun_a}"
    val_a = rng.randint(1, 300)
    val_b = rng.randint(1, 300)

    buggy_line = '    return f"' + noun_a + "={" + noun_a + "}, " + noun_b + "={" + noun_a + '}"'
    fixed_line = '    return f"' + noun_a + "={" + noun_a + "}, " + noun_b + "={" + noun_b + '}"'
    buggy_output = f"{noun_a}={val_a}, {noun_b}={val_a}"
    correct_output = f"{noun_a}={val_a}, {noun_b}={val_b}"

    contents = (
        f"def {fn_name}({noun_a}, {noun_b}):\n"
        f"    # Return a summary mentioning both {noun_a} and {noun_b}.\n"
        f"{buggy_line}\n"
        f"\n"
        f"\n"
        f"def {fn_name}_for(entry):\n"
        f'    return {fn_name}(entry["{noun_a}"], entry["{noun_b}"])\n'
    )
    search = buggy_line
    replace = fixed_line
    goal = goal_phrasing.patch_skeletons(
        rng,
        target,
        subject=f"{fn_name}()",
        problem=f"reports {noun_a} where {noun_b} should appear",
        evidence=f"{fn_name}({val_a}, {val_b}) returns '{buggy_output}' instead of '{correct_output}'",
        fix_target=f"the f-string in {fn_name}() in {target}",
        fix_goal=f"it reports {noun_b}, not {noun_a}",
        instruction=DONE_INSTRUCTION,
    )
    summary = f"Fixed the f-string in {fn_name}() to report {noun_b} instead of repeating {noun_a}."
    return Task("py_wrong_fstring_field", "python", target, {target: contents}, goal, search, replace, summary)


def _family_wrong_dict_key(rng: random.Random) -> Task:
    theme, target = _theme_and_target(rng)
    noun = rng.choice(theme.nouns)
    fn_name = f"{noun}_value"
    key_correct, key_wrong = rng.sample(DICT_KEY_POOL, 2)
    val_correct = rng.randint(1, 500)
    val_wrong = rng.randint(1, 500)
    while val_wrong == val_correct:
        val_wrong = rng.randint(1, 500)
    entry_repr = repr({key_correct: val_correct, key_wrong: val_wrong})

    contents = (
        f"def {fn_name}(entry):\n"
        f'    # Return the "{key_correct}" {noun} reading from entry.\n'
        f'    return entry["{key_wrong}"]\n'
        f"\n"
        f"\n"
        f"def {fn_name}_or_default(entry, fallback):\n"
        f"    value = {fn_name}(entry)\n"
        f"    return value if value is not None else fallback\n"
    )
    search = f'    return entry["{key_wrong}"]'
    replace = f'    return entry["{key_correct}"]'
    goal = goal_phrasing.patch_skeletons(
        rng,
        target,
        subject=f"{fn_name}()",
        problem=f'reads the "{key_wrong}" entry instead of "{key_correct}"',
        evidence=f"{fn_name}({entry_repr}) returns {val_wrong} instead of {val_correct}",
        fix_target=f"the dict lookup in {fn_name}() in {target}",
        fix_goal=f'it reads "{key_correct}"',
        instruction=DONE_INSTRUCTION,
    )
    summary = f'Fixed {fn_name}() to read the "{key_correct}" key instead of "{key_wrong}".'
    return Task("py_wrong_dict_key", "python", target, {target: contents}, goal, search, replace, summary)


FAMILIES = {
    "py_wrong_comparison_operator": _family_wrong_comparison_operator,
    "py_off_by_one_index": _family_off_by_one_index,
    "py_off_by_one_range_bound": _family_off_by_one_range_bound,
    "py_wrong_constant_multiplier": _family_wrong_constant_multiplier,
    "py_inverted_boolean": _family_inverted_boolean,
    "py_wrong_variable_reference": _family_wrong_variable_reference,
    "py_wrong_fstring_field": _family_wrong_fstring_field,
    "py_wrong_dict_key": _family_wrong_dict_key,
}

# The run-verified twin of every family above lives in
# `templates_run_verified.py` (turn-4 spec §3): each is a WRAPPER over the
# entry here, adding a planted `unittest` and the grant that runs it,
# never a second copy of the defect. Plaintext has no run-verified slice
# for the mirror-image reason: there is no verification to run.
