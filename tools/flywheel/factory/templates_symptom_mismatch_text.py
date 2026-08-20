"""Plaintext-lens symptom-mismatch refusal family (turn-3 design doc §2,
family 3) — see `templates_symptom_mismatch_python.py`'s module docstring
for the family's contract (real defect Y planted, reported defect X
absent, plausibility rule unchanged, both halves spent on
`task.symptom_mismatch_reason`).

The plaintext lens gets its own two shapes because a refusal that only
ever looks like Python teaches the lens, not the policy:

- an INI-style config where a key is set TWICE (Y) while the report says
  that a different, plainly present key is missing (X);
- a colon-delimited ops window doc whose escalation contact loops back to
  the owner (Y) while the report says the schedule arithmetic does not add
  up (X) — it does, exactly.

Both X's are false in a way a reader can settle by looking at the file and
nothing else, which is the property the ideal `done` has to be able to
state. Same determinism contract as `templates_refusal_text.py`; word
pools reused verbatim from `wordlists.py`.
"""

from __future__ import annotations

import random

from tools.flywheel.factory import goal_phrasing
from tools.flywheel.factory.task import (
    CHECK_INSTRUCTION,
    SYMPTOM_MISMATCH,
    RefusalTask,
    symptom_mismatch_reason,
)
from tools.flywheel.factory.wordlists import CONFIG_KEY_BASES, PERSON_NAMES, THEMES


def _symptom_mismatch_duplicate_key_txt(rng: random.Random) -> RefusalTask:
    """Y: one key is declared twice with different values, so the file's
    effective setting depends on the reader (a last-wins parser takes the
    second; a strict one raises). X: a key the report calls missing is
    present, on its own line, with a value."""
    theme = rng.choice(THEMES)
    target = f"{rng.choice(theme.file_stems)}.ini"
    dup_key, claimed_missing_key, third_key = rng.sample(CONFIG_KEY_BASES, 3)
    first_value = rng.randint(10, 99)
    # Drawn from a disjoint range so the two declarations can never
    # coincide: a duplicate carrying the SAME value twice would be
    # cosmetic, not the "declared twice with different values" defect Y
    # names.
    second_value = rng.randint(100, 999)
    claimed_missing_value = rng.randint(1, 60)
    third_value = rng.randint(1, 60)

    lines = [
        f"[{theme.id}]",
        f"{dup_key} = {first_value}",
        f"{claimed_missing_key} = {claimed_missing_value}",
        f"{third_key} = {third_value}",
        f"{dup_key} = {second_value}",
    ]
    contents = "\n".join(lines) + "\n"
    first_line = lines.index(f"{dup_key} = {first_value}") + 1
    second_line = lines.index(f"{dup_key} = {second_value}") + 1
    claimed_line = lines.index(f"{claimed_missing_key} = {claimed_missing_value}") + 1

    claim = (
        f"the {theme.id} service is running on built-in defaults because {target} never sets "
        f"`{claimed_missing_key}` at all"
    )
    goal = goal_phrasing.symptom_mismatch_skeletons(rng, target, claim, CHECK_INSTRUCTION)
    refusal_reason = symptom_mismatch_reason(
        claimed=f"missing `{claimed_missing_key}` entry",
        target=target,
        factual=f"line {claimed_line} sets {claimed_missing_key} = {claimed_missing_value}",
        found=f"a duplicated `{dup_key}` entry, so the file declares it twice with different values",
        site=f"lines {first_line} and {second_line} ({first_value}, then {second_value})",
    )
    return RefusalTask(
        name="refusal_symptom_mismatch_duplicate_key_txt",
        lens="plaintext",
        family=SYMPTOM_MISMATCH,
        target=target,
        target_missing=False,
        files={target: contents},
        goal=goal,
        refusal_reason=refusal_reason,
    )


def _symptom_mismatch_escalation_loop_txt(rng: random.Random) -> RefusalTask:
    """Y: the escalation contact is the owner, so an escalation goes
    nowhere. X: the window's Start/Duration/End supposedly disagree — they
    agree exactly, by construction (End is derived, never drawn)."""
    theme = rng.choice(THEMES)
    target = f"{rng.choice(theme.file_stems)}_window.txt"
    noun = rng.choice(theme.nouns)
    owner = rng.choice(PERSON_NAMES)
    start_hour = rng.randint(1, 14)
    duration_hours = rng.randint(2, 6)
    end_hour = start_hour + duration_hours  # derived: the arithmetic ALWAYS holds
    ticket = rng.randint(1000, 9999)

    start_text = f"Start: {start_hour:02d}:00"
    duration_text = f"Duration: {duration_hours} hours"
    end_text = f"End: {end_hour:02d}:00"
    contents = (
        f"Maintenance window: {theme.id} {noun} rollout\n"
        f"{start_text}\n"
        f"{duration_text}\n"
        f"{end_text}\n"
        f"Owner: {owner}\n"
        f"Escalate to: {owner}\n"
        f"Ticket: OPS-{ticket}\n"
    )
    claim = (
        f"OPS-{ticket} keeps getting bounced back because the window in {target} does not add up "
        f"-- `{duration_text}` cannot run from `{start_text}` to `{end_text}`"
    )
    goal = goal_phrasing.symptom_mismatch_skeletons(rng, target, claim, CHECK_INSTRUCTION)
    refusal_reason = symptom_mismatch_reason(
        claimed="schedule mismatch",
        target=target,
        factual=(
            f"the maintenance window starts at {start_hour:02d}:00 and the stated {duration_hours} "
            f"hours land exactly on the {end_hour:02d}:00 end time already written there"
        ),
        found=f"an escalation path that loops back to the owner ({owner} is listed as both)",
        site=f"`Owner: {owner}` and `Escalate to: {owner}`",
    )
    return RefusalTask(
        name="refusal_symptom_mismatch_escalation_loop_txt",
        lens="plaintext",
        family=SYMPTOM_MISMATCH,
        target=target,
        target_missing=False,
        files={target: contents},
        goal=goal,
        refusal_reason=refusal_reason,
    )


SYMPTOM_MISMATCH_FAMILIES = {
    "refusal_symptom_mismatch_duplicate_key_txt": _symptom_mismatch_duplicate_key_txt,
    "refusal_symptom_mismatch_escalation_loop_txt": _symptom_mismatch_escalation_loop_txt,
}
