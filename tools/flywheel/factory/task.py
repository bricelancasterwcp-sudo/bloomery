"""`Task` and structural validation (brief rule 2) — split out of
`templates.py` so the template family modules (`templates_python.py`,
`templates_text.py`) can import `Task`/`DONE_INSTRUCTION` without a
circular import (`templates.py` imports the family modules to build its
registries).
"""

from __future__ import annotations

from typing import NamedTuple

DONE_INSTRUCTION = "Patch the file, then emit done."
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
