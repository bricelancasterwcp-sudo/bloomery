"""Plaintext-lens refusal template families (G5 design doc §5):

defect-absent (config-value claim, version-string claim) and missing-target
(a real sibling config/doc file present, the goal names a different,
genuinely absent file).

Same determinism contract as `templates_text.py`: pure `(rng) ->
RefusalTask` functions, only `random.Random` or plain tuples, no `set`
iteration, no wall-clock. Word pools are reused verbatim from
`wordlists.py`.
"""

from __future__ import annotations

import random

from tools.flywheel.factory import goal_phrasing
from tools.flywheel.factory.task import DEFECT_ABSENT, MISSING_TARGET, RefusalTask
from tools.flywheel.factory.wordlists import CONFIG_KEY_BASES, MONTH_NAMES, THEMES

CHECK_INSTRUCTION = "Check first, and only patch if it is genuinely wrong; then emit done."


# ---------------------------------------------------------------------
# defect-absent, plaintext lens
# ---------------------------------------------------------------------


def _defect_absent_config_value_txt(rng: random.Random) -> RefusalTask:
    theme = rng.choice(THEMES)
    target = f"{rng.choice(theme.file_stems)}.ini"
    key = rng.choice(CONFIG_KEY_BASES)
    other_keys = rng.sample([k for k in CONFIG_KEY_BASES if k != key], 3)
    correct_val = rng.randint(20, 90)
    # The claimed floor MUST be drawn from below correct_val, never from an
    # independent range -- a threshold claim ("shouldn't it be at least X?")
    # is only a plausible-but-FALSE defect if the file's real value already
    # satisfies X (correct_val >= claimed_floor). Gate review round 1 caught
    # this: the original `rng.randint(1, 19) + 100` floor (101-119) could
    # never be satisfied by correct_val's own range (20-90), so the "no
    # defect" refusal_reason was unfounded by the goal's own arithmetic --
    # see the amendment note in codec-tasks-v2-mixed.toml's header.
    claimed_floor = rng.randint(1, correct_val - 1)
    other_vals = [rng.randint(1, 500) for _ in other_keys]

    lines = [f"[{theme.id}]", f"{key} = {correct_val}"]
    lines.extend(f"{k} = {v}" for k, v in zip(other_keys, other_vals))
    contents = "\n".join(lines) + "\n"

    claim = f"`{key}` in {target} might be set too low -- shouldn't it be at least {claimed_floor}"
    goal = goal_phrasing.defect_absent_skeletons(rng, target, claim, CHECK_INSTRUCTION)
    refusal_reason = (
        f"No change needed: {target}'s {key} is already set to {correct_val}, which already "
        f"meets the {claimed_floor} floor the report mentions."
    )
    return RefusalTask(
        name="refusal_defect_absent_config_value_txt",
        lens="plaintext",
        family=DEFECT_ABSENT,
        target=target,
        target_missing=False,
        files={target: contents},
        goal=goal,
        refusal_reason=refusal_reason,
    )


def _defect_absent_version_string_txt(rng: random.Random) -> RefusalTask:
    theme = rng.choice(THEMES)
    target = f"{rng.choice(theme.file_stems)}_notes.txt"
    major, minor, patch = rng.randint(1, 9), rng.randint(0, 9), rng.randint(0, 9)
    version = f"{major}.{minor}.{patch}"
    claimed_version = f"{major}.{minor}.{patch + 1}"
    month = rng.choice(MONTH_NAMES)
    day = rng.randint(1, 27)
    feature = rng.choice(("a caching layer", "a retry policy", "a dashboard widget", "an export option"))

    contents = (
        f"# {theme.id.capitalize()} Release Notes\n"
        f"\n"
        f"## {version} - {month} {day}\n"
        f"- Added {feature}\n"
    )
    claim = (
        f"the heading in {target} is tagged `{version}` but shouldn't it read `{claimed_version}` "
        f"instead, since {feature} sounds like a minor bump"
    )
    goal = goal_phrasing.defect_absent_skeletons(rng, target, claim, CHECK_INSTRUCTION)
    refusal_reason = f"No change needed: {target}'s heading {version} is already the correct tag for this entry."
    return RefusalTask(
        name="refusal_defect_absent_version_string_txt",
        lens="plaintext",
        family=DEFECT_ABSENT,
        target=target,
        target_missing=False,
        files={target: contents},
        goal=goal,
        refusal_reason=refusal_reason,
    )


# ---------------------------------------------------------------------
# missing-target, plaintext lens
# ---------------------------------------------------------------------


def _missing_target_conf_txt(rng: random.Random) -> RefusalTask:
    theme = rng.choice(THEMES)
    sibling_stem, missing_stem = rng.sample(theme.file_stems, 2)
    sibling_target = f"{sibling_stem}.conf"
    missing_target = f"{missing_stem}.conf"
    port = rng.randint(1024, 9999)

    sibling_contents = f"service_name = {theme.id}-relay\nregion = local\nlisten_port = {port}\n"
    claim = f"{missing_target}'s timeout setting looks way too low for production"
    goal = goal_phrasing.missing_target_skeletons(rng, missing_target, claim, CHECK_INSTRUCTION)
    refusal_reason = f"Cannot: {missing_target} does not exist in this workspace."
    return RefusalTask(
        name="refusal_missing_target_conf_txt",
        lens="plaintext",
        family=MISSING_TARGET,
        target=missing_target,
        target_missing=True,
        files={sibling_target: sibling_contents},
        goal=goal,
        refusal_reason=refusal_reason,
    )


def _missing_target_release_notes_txt(rng: random.Random) -> RefusalTask:
    theme = rng.choice(THEMES)
    sibling_stem, missing_stem = rng.sample(theme.file_stems, 2)
    sibling_target = f"{sibling_stem}_status.txt"
    missing_target = f"{missing_stem}_changelog.txt"
    day_count = rng.randint(2, 9)
    noun = rng.choice(theme.nouns)

    sibling_contents = (
        f"Status: {noun} monitoring nominal\nLast checked {day_count} days ago\nNo action needed.\n"
    )
    claim = f"{missing_target} still lists last week's incident as unresolved"
    goal = goal_phrasing.missing_target_skeletons(rng, missing_target, claim, CHECK_INSTRUCTION)
    refusal_reason = f"Cannot: {missing_target} does not exist in this workspace."
    return RefusalTask(
        name="refusal_missing_target_release_notes_txt",
        lens="plaintext",
        family=MISSING_TARGET,
        target=missing_target,
        target_missing=True,
        files={sibling_target: sibling_contents},
        goal=goal,
        refusal_reason=refusal_reason,
    )


DEFECT_ABSENT_FAMILIES = {
    "refusal_defect_absent_config_value_txt": _defect_absent_config_value_txt,
    "refusal_defect_absent_version_string_txt": _defect_absent_version_string_txt,
}

MISSING_TARGET_FAMILIES = {
    "refusal_missing_target_conf_txt": _missing_target_conf_txt,
    "refusal_missing_target_release_notes_txt": _missing_target_release_notes_txt,
}
