"""Python-lens find-shaped template families (turn-3 design doc §2's
multi-file repair slice; task-7 brief).

Each family plants a TARGET carrying a real defect plus 2-4 plausible
sibling modules, and writes a goal that names the SYMPTOM and never a
filename. The ideal trajectory therefore opens with `find(find_pattern)`
to locate the file before reading it — which is only a real step if the
pattern singles the target out.

**Why the pattern is unique to the target by construction.** Every target
identifier is `<target verb>_<noun>_<family suffix>` and every sibling
identifier is `<sibling verb>_<noun>_<sibling suffix>`, drawn from the two
DISJOINT pools in `wordlists.py` (`MULTIFILE_TARGET_VERBS` /
`MULTIFILE_SIBLING_VERBS`). A sibling can therefore never contain the
pattern, whatever nouns a draw picks — matching on the noun alone would
NOT be safe, since one theme noun can be a prefix of another
(`water_temp` / `water_temp_c`), which is also why the pattern always
carries a trailing suffix rather than ending at the noun. The trailing
suffix additionally keeps a family's pattern from matching a sibling
family's target if the two ever share a workspace.

The pattern is also kept regex-literal (identifier characters and spaces
only): `exec_find` compiles it with `Regex::new`, so a metacharacter would
either fail to compile or match something other than the literal text
`validate_task` checked for. `test_templates_multifile.py` pins both
properties across every draw.

Same determinism contract as `templates_python.py`: pure `(rng) -> Task`
functions, every choice from `random.Random` or plain tuples, no `set`
iteration, no wall-clock. The three families deliberately have DIFFERENT
code shapes (a guard-clause classifier, a module-level table plus a
lookup, and a class with a window method) — the v3 diversity rule, pinned
by `DistinctCodeShapesTest`.
"""

from __future__ import annotations

import random

from tools.flywheel.factory import goal_phrasing
from tools.flywheel.factory.task import DONE_INSTRUCTION, FIND_TRAJECTORY, Task
from tools.flywheel.factory.wordlists import (
    DICT_KEY_POOL,
    MULTIFILE_SIBLING_VERBS,
    MULTIFILE_TARGET_VERBS,
    THEMES,
    Theme,
)

# Sibling-side identifier suffixes. Purely English glue (the same call
# `templates_python.py` already makes for "readings"/"markers"): the
# uniqueness argument rests on the VERB pools, not on these.
_SIBLING_SUFFIXES: tuple[str, ...] = ("records", "manifest", "batch", "roster")
MIN_SIBLINGS = 2
MAX_SIBLINGS = 4

# Parameter/constant-name pools whose only job is search-string entropy.
# A family whose defect LINE is byte-identical on every draw collides with
# a frozen gate fixture generated from that same family with probability
# 1, and no amount of rejection-sampling retries can clear it -- the exact
# bug task 6a's follow-on fixed in `templates_python.py`'s two off-by-one
# families. Each pool multiplies the (theme, noun) space the defect line
# already varies over.
_LIMIT_QUALIFIERS: tuple[str, ...] = ("LIMIT", "CEILING", "THRESHOLD", "CUTOFF")
_TABLE_QUALIFIERS: tuple[str, ...] = ("SETTINGS", "DEFAULTS", "PROFILE", "OPTIONS")
_READING_PARAMS: tuple[str, ...] = ("reading", "sample", "measurement", "datapoint")
_WINDOW_PARAMS: tuple[str, ...] = ("span", "size", "width", "depth")


def _draw_workspace(rng: random.Random) -> tuple[Theme, str, str, str, list[str]]:
    """One draw's shared skeleton: a theme, the target's stem and marker
    verb, and distinct nouns for the target and each sibling (drawn in ONE
    `rng.sample` so no sibling can accidentally reuse the target's noun)."""
    theme = rng.choice(THEMES)
    target_stem = rng.choice(theme.file_stems)
    verb = rng.choice(MULTIFILE_TARGET_VERBS)
    sibling_count = rng.randint(MIN_SIBLINGS, MAX_SIBLINGS)
    nouns = rng.sample(theme.nouns, sibling_count + 1)
    return theme, target_stem, verb, nouns[0], nouns[1:]


def _sibling_module(theme: Theme, marker: str, shape: int) -> str:
    """One neighbour module. Three bodies, cycled by position: a directory
    where every non-target file is byte-identical apart from one
    identifier would make the `find` trivially separable on shape rather
    than on the pattern, and would hand the contamination guard a fixed
    file body to collide on."""
    if shape == 0:
        return (
            f"def {marker}(rows):\n"
            f"    # Neighbour helper in the {theme.id} package.\n"
            f"    return [row for row in rows if row is not None]\n"
        )
    if shape == 1:
        return (
            f"def {marker}(rows):\n"
            f'    """Index the {theme.id} rows by their id."""\n'
            f"    return {{row['id']: row for row in rows}}\n"
        )
    return (
        f"{marker.upper()}_DEFAULT = 0\n"
        f"\n"
        f"\n"
        f"def {marker}(rows):\n"
        f"    return sum(len(row) for row in rows) + {marker.upper()}_DEFAULT\n"
    )


def _sibling_files(rng: random.Random, theme: Theme, target_stem: str, nouns: list[str]) -> dict[str, str]:
    """Plausible neighbour modules for the same package: one per sibling
    noun. A theme carries 4 stems, so the fourth sibling reuses a stem
    under a distinct `_util` filename rather than capping the draw at
    three."""
    stems = [stem for stem in theme.file_stems if stem != target_stem]
    files: dict[str, str] = {}
    for i, noun in enumerate(nouns):
        stem = stems[i % len(stems)]
        name = f"{stem}.py" if i < len(stems) else f"{stem}_util.py"
        marker = f"{rng.choice(MULTIFILE_SIBLING_VERBS)}_{noun}_{_SIBLING_SUFFIXES[i % len(_SIBLING_SUFFIXES)]}"
        files[name] = _sibling_module(theme, marker, i % 3)
    return files


def _family_threshold_band(rng: random.Random) -> Task:
    """Guard-clause classifier with an inverted comparison: readings above
    the limit come back labelled as below it."""
    theme, target_stem, verb, noun, sibling_nouns = _draw_workspace(rng)
    target = f"{target_stem}.py"
    fn_name = f"{verb}_{noun}_band"
    const = f"{noun.upper()}_{rng.choice(_LIMIT_QUALIFIERS)}"
    param = rng.choice(_READING_PARAMS)
    limit = rng.randint(10, 90)
    sample = limit + rng.randint(1, 20)

    contents = (
        f"{const} = {limit}\n"
        f"\n"
        f"\n"
        f"def {fn_name}({param}):\n"
        f"    if {param} < {const}:\n"
        f'        return "over"\n'
        f'    return "under"\n'
    )
    search = f"    if {param} < {const}:"
    replace = f"    if {param} > {const}:"
    goal = goal_phrasing.find_skeletons(
        rng,
        subject=f"{fn_name}()",
        problem=f'labels every {noun} reading above the limit as "under"',
        evidence=f'{fn_name}({sample}) returns "under" against a limit of {limit}',
        fix_goal=f'readings above {const} come back as "over"',
        instruction=DONE_INSTRUCTION,
    )
    files = {target: contents}
    files.update(_sibling_files(rng, theme, target_stem, sibling_nouns))
    return Task(
        name="mf_py_threshold_band",
        lens="python",
        target=target,
        files=files,
        goal=goal,
        search=search,
        replace=replace,
        summary=f"Flipped the comparison in {fn_name}() so readings above {const} classify as over.",
        trajectory=FIND_TRAJECTORY,
        find_pattern=f"def {fn_name}",
    )


def _family_settings_table(rng: random.Random) -> Task:
    """Module-level settings table plus a lookup that reads the wrong key
    out of it."""
    theme, target_stem, verb, noun, sibling_nouns = _draw_workspace(rng)
    target = f"{target_stem}.py"
    fn_name = f"{verb}_{noun}_setting"
    table = f"{noun.upper()}_{rng.choice(_TABLE_QUALIFIERS)}"
    key_correct, key_wrong = rng.sample(DICT_KEY_POOL, 2)
    val_correct = rng.randint(1, 400)
    val_wrong = rng.randint(401, 900)

    contents = (
        f"{table} = {{\n"
        f'    "{key_correct}": {val_correct},\n'
        f'    "{key_wrong}": {val_wrong},\n'
        f"}}\n"
        f"\n"
        f"\n"
        f"def {fn_name}():\n"
        f'    # Return the "{key_correct}" {noun} setting for the active profile.\n'
        f'    return {table}["{key_wrong}"]\n'
    )
    search = f'    return {table}["{key_wrong}"]'
    replace = f'    return {table}["{key_correct}"]'
    goal = goal_phrasing.find_skeletons(
        rng,
        subject=f"{fn_name}()",
        problem=f'hands back the "{key_wrong}" {noun} setting instead of the "{key_correct}" one',
        evidence=f"it returns {val_wrong} where every caller expects {val_correct}",
        fix_goal=f'the lookup reads "{key_correct}"',
        instruction=DONE_INSTRUCTION,
    )
    files = {target: contents}
    files.update(_sibling_files(rng, theme, target_stem, sibling_nouns))
    return Task(
        name="mf_py_settings_table",
        lens="python",
        target=target,
        files=files,
        goal=goal,
        search=search,
        replace=replace,
        summary=f'Fixed {fn_name}() to read the "{key_correct}" key instead of "{key_wrong}".',
        trajectory=FIND_TRAJECTORY,
        find_pattern=f"def {fn_name}",
    )


def _family_window_class(rng: random.Random) -> Task:
    """A small class whose window method slices one element short."""
    theme, target_stem, verb, noun, sibling_nouns = _draw_workspace(rng)
    target = f"{target_stem}.py"
    fn_name = f"{verb}_{noun}_window"
    class_name = "".join(part.title() for part in noun.split("_")) + "Feed"
    attr = f"{noun}_readings"
    param = rng.choice(_WINDOW_PARAMS)
    size = rng.randint(3, 12)

    contents = (
        f"class {class_name}:\n"
        f'    """Recent {noun} readings for the {theme.id} feed."""\n'
        f"\n"
        f"    def __init__(self, {attr}):\n"
        f"        self.{attr} = {attr}\n"
        f"\n"
        f"    def {fn_name}(self, {param}):\n"
        f"        return self.{attr}[:{param} - 1]\n"
    )
    search = f"        return self.{attr}[:{param} - 1]"
    replace = f"        return self.{attr}[:{param}]"
    goal = goal_phrasing.find_skeletons(
        rng,
        subject=f"the {noun} feed's {fn_name}() method",
        problem="always comes back one reading short of the window it was asked for",
        evidence=f"asking for {size} readings yields only {size - 1}",
        fix_goal="a window of N readings really contains N",
        instruction=DONE_INSTRUCTION,
    )
    files = {target: contents}
    files.update(_sibling_files(rng, theme, target_stem, sibling_nouns))
    return Task(
        name="mf_py_window_class",
        lens="python",
        target=target,
        files=files,
        goal=goal,
        search=search,
        replace=replace,
        summary=f"Widened {class_name}.{fn_name}()'s slice so it returns the full window.",
        trajectory=FIND_TRAJECTORY,
        find_pattern=f"def {fn_name}",
    )


FAMILIES = {
    "mf_py_threshold_band": _family_threshold_band,
    "mf_py_settings_table": _family_settings_table,
    "mf_py_window_class": _family_window_class,
}
