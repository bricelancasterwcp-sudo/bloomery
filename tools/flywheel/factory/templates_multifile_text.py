"""Plaintext-lens find-shaped template families (turn-3 design doc §2's
multi-file repair slice; task-7 brief).

`templates_multifile_python.py`'s module doc carries the full reasoning
this file shares: target plus 2-4 plausible siblings, a goal that names
the SYMPTOM and never a filename, and a `find_pattern` made unique to the
target by construction — every target marker is
`<target verb>_<noun>_<family suffix>` and every sibling marker is
`<sibling verb>_<noun>_<sibling suffix>`, drawn from the two DISJOINT
pools in `wordlists.py`, so no sibling can contain a target's pattern
whatever nouns a draw picks.

Same determinism contract as `templates_text.py`: pure `(rng) -> Task`
functions, every choice from `random.Random` or plain tuples, no `set`
iteration, no wall-clock. The two families deliberately have different
shapes (a two-section INI and a runbook table), and neither shares a shape
with the three python families — the v3 diversity rule, pinned by
`test_templates_multifile.py`'s `DistinctCodeShapesTest`.
"""

from __future__ import annotations

import random

from tools.flywheel.factory import goal_phrasing
from tools.flywheel.factory.task import DONE_INSTRUCTION, FIND_TRAJECTORY, Task
from tools.flywheel.factory.wordlists import (
    CONFIG_KEY_BASES,
    DOC_URL_PATHS,
    FAKE_DOMAIN_BASES,
    FAKE_DOMAIN_TLDS,
    MULTIFILE_SIBLING_VERBS,
    MULTIFILE_TARGET_VERBS,
    PERSON_NAMES,
    THEMES,
    Theme,
)

MIN_SIBLINGS = 2
MAX_SIBLINGS = 4

# Sibling-side marker suffixes, and the target-side qualifiers whose only
# job is defect-line entropy (see `templates_multifile_python.py`'s note on
# why a byte-identical defect line collides with a frozen gate forever).
_SIBLING_SUFFIXES: tuple[str, ...] = ("log", "sheet", "roster", "digest")
_LIMIT_SUFFIXES: tuple[str, ...] = ("limit", "floor", "quota", "budget")
_ALERT_SUFFIXES: tuple[str, ...] = ("alert", "page", "escalation", "signal")


def _draw_workspace(rng: random.Random) -> tuple[Theme, str, str, str, list[str]]:
    """One draw's shared skeleton — the plaintext mirror of
    `templates_multifile_python._draw_workspace`, including the single
    `rng.sample` that keeps every sibling's noun distinct from the
    target's."""
    theme = rng.choice(THEMES)
    target_stem = rng.choice(theme.file_stems)
    verb = rng.choice(MULTIFILE_TARGET_VERBS)
    sibling_count = rng.randint(MIN_SIBLINGS, MAX_SIBLINGS)
    nouns = rng.sample(theme.nouns, sibling_count + 1)
    return theme, target_stem, verb, nouns[0], nouns[1:]


def _sibling_document(theme: Theme, marker: str, owner: str, shape: int) -> str:
    """One neighbour document. Three bodies, cycled by position, for the
    same reason `templates_multifile_python._sibling_module` has three: a
    directory of byte-identical non-target files would make the `find`
    separable on shape rather than on the pattern."""
    if shape == 0:
        return (
            f"{theme.id} operations notes\n"
            f"\n"
            f"{marker}: reviewed at the start of every shift\n"
            f"owner: {owner}\n"
        )
    if shape == 1:
        return (
            f"# {theme.id} handover\n"
            f"{marker} = pending\n"
            f"reviewer = {owner}\n"
        )
    return (
        f"{marker}\n"
        f"----\n"
        f"Standing item for the {theme.id} desk; {owner} signs it off weekly.\n"
    )


def _sibling_files(rng: random.Random, theme: Theme, target_stem: str, nouns: list[str]) -> dict[str, str]:
    """Plausible neighbour documents for the same operations directory."""
    stems = [stem for stem in theme.file_stems if stem != target_stem]
    files: dict[str, str] = {}
    for i, noun in enumerate(nouns):
        stem = stems[i % len(stems)]
        name = f"{stem}_notes.txt" if i < len(stems) else f"{stem}_shift.txt"
        marker = f"{rng.choice(MULTIFILE_SIBLING_VERBS)}_{noun}_{_SIBLING_SUFFIXES[i % len(_SIBLING_SUFFIXES)]}"
        files[name] = _sibling_document(theme, marker, rng.choice(PERSON_NAMES), i % 3)
    return files


def _family_intake_limit_ini(rng: random.Random) -> Task:
    """Two-section INI whose intake floor is set below what operations
    requires."""
    theme, target_stem, verb, noun, sibling_nouns = _draw_workspace(rng)
    target = f"{target_stem}.ini"
    key = f"{verb}_{noun}_{rng.choice(_LIMIT_SUFFIXES)}"
    wrong_val = rng.randint(1, 40)
    correct_val = rng.randint(wrong_val + 5, wrong_val + 55)
    filler_a, filler_b, filler_c = rng.sample(CONFIG_KEY_BASES, 3)

    contents = (
        f"[{theme.id}_intake]\n"
        f"# floor agreed with operations for the {noun} pipeline\n"
        f"{key} = {wrong_val}\n"
        f"{filler_a} = {rng.randint(1, 500)}\n"
        f"\n"
        f"[{theme.id}_drain]\n"
        f"{filler_b} = {rng.randint(1, 500)}\n"
        f"{filler_c} = {rng.randint(1, 500)}\n"
    )
    search = f"{key} = {wrong_val}"
    replace = f"{key} = {correct_val}"
    goal = goal_phrasing.find_skeletons(
        rng,
        subject=f"the {key} setting",
        problem=f"is pinned at {wrong_val}",
        evidence=f"operations signed off on a floor of {correct_val}, and anything lower sheds {noun} load",
        fix_goal=f"{key} reads {correct_val}",
        instruction=DONE_INSTRUCTION,
    )
    files = {target: contents}
    files.update(_sibling_files(rng, theme, target_stem, sibling_nouns))
    return Task(
        name="mf_txt_intake_limit_ini",
        lens="plaintext",
        target=target,
        files=files,
        goal=goal,
        search=search,
        replace=replace,
        summary=f"Raised {key} from {wrong_val} to {correct_val}.",
        trajectory=FIND_TRAJECTORY,
        find_pattern=key,
    )


def _family_runbook_table(rng: random.Random) -> Task:
    """Runbook table whose escalation row links the wrong documentation
    path."""
    theme, target_stem, verb, noun, sibling_nouns = _draw_workspace(rng)
    target = f"{target_stem}_runbook.txt"
    row_id = f"{verb}_{noun}_{rng.choice(_ALERT_SUFFIXES)}"
    other_id = f"{rng.choice(MULTIFILE_SIBLING_VERBS)}_{sibling_nouns[0]}_handoff"
    owner_a, owner_b = rng.sample(PERSON_NAMES, 2)
    domain = f"{rng.choice(FAKE_DOMAIN_BASES)}.{rng.choice(FAKE_DOMAIN_TLDS)}"
    correct_path, wrong_path = rng.sample(DOC_URL_PATHS, 2)

    # Every line's WORD COUNT is fixed across draws on purpose: the v3
    # diversity pin compares structural skeletons (identifiers -> X), so a
    # heading built from a multi-word product name would make this family's
    # own skeleton wobble seed to seed and the pin vacuous.
    contents = (
        f"{theme.id} on-call runbook\n"
        f"purpose: escalation routing for the {noun} pipeline\n"
        f"\n"
        f"| step | owner | reference |\n"
        f"| ---- | ----- | --------- |\n"
        f"| {row_id} | {owner_a} | https://{domain}{wrong_path} |\n"
        f"| {other_id} | {owner_b} | https://{domain}{correct_path} |\n"
    )
    search = f"| {row_id} | {owner_a} | https://{domain}{wrong_path} |"
    replace = f"| {row_id} | {owner_a} | https://{domain}{correct_path} |"
    goal = goal_phrasing.find_skeletons(
        rng,
        subject=f"the {row_id} row of an on-call runbook",
        problem=f"sends whoever is paged to {wrong_path}",
        evidence=f"the {noun} escalation procedure actually lives at {correct_path}, so the link 404s",
        fix_goal=f"the {row_id} reference points at {correct_path}",
        instruction=DONE_INSTRUCTION,
    )
    files = {target: contents}
    files.update(_sibling_files(rng, theme, target_stem, sibling_nouns))
    return Task(
        name="mf_txt_runbook_table",
        lens="plaintext",
        target=target,
        files=files,
        goal=goal,
        search=search,
        replace=replace,
        summary=f"Repointed the {row_id} runbook reference at {correct_path}.",
        trajectory=FIND_TRAJECTORY,
        find_pattern=row_id,
    )


FAMILIES = {
    "mf_txt_intake_limit_ini": _family_intake_limit_ini,
    "mf_txt_runbook_table": _family_runbook_table,
}
