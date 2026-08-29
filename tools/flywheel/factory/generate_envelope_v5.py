"""The v5 ideal assembler (turn-7 spec §2.1–§2.2) — pure functions that
turn a validated `Task`/`RefusalTask` into the same task with its `done`
ideal replaced by the full declared v5 block (`done_v5`), applied by
`generate.py` under `--envelope v5` AFTER validation, dedup, and gate
screening (all of which read goal/files and are envelope-independent),
immediately before the wire request is built.

Two contracts live here and nowhere else:

- **The family→reason mapping is the inversion of the ONE existing
  table** (`tools.evidence.endpoints.REASON_TO_FAMILY`, turn-6 spec §5.2
  — "the mapping lives in the recompute tool, in one place"). Imported
  and inverted, never copied: if the endpoint table ever changes, this
  module changes with it or the bijection assertion below fails at
  import.
- **Patch evidence is mechanical**: the quoted line is the first line of
  the replaced region in the POST-patch file that differs from its
  pre-patch counterpart (fallback: the region's first line), so a
  truthful `fixed` quotes the patched bytes — exactly the shipped
  endpoint's post-`reference` grounding rule
  (`endpoints._classify_evidence_line`), which `check_corpus_v5.py`
  re-proves for every generated row.
"""

from __future__ import annotations

from tools.evidence.endpoints import REASON_TO_FAMILY
from tools.flywheel.factory.task import (
    MISSING_TARGET,
    REFUSAL_FAMILIES,
    RefusalTask,
    Task,
    done_v5,
)

# endpoint families are hyphenated ("defect-absent"); factory families are
# underscored (DEFECT_ABSENT = "defect_absent"). One mechanical translation,
# asserted to be a bijection onto REFUSAL_FAMILIES at import time.
FAMILY_TO_REASON: dict[str, str] = {
    family.replace("-", "_"): reason for reason, family in REASON_TO_FAMILY.items()
}
if sorted(FAMILY_TO_REASON) != sorted(REFUSAL_FAMILIES):
    raise AssertionError(
        f"FAMILY_TO_REASON keys {sorted(FAMILY_TO_REASON)} are not the factory's "
        f"REFUSAL_FAMILIES {sorted(REFUSAL_FAMILIES)} -- the endpoint table and the "
        f"factory families have drifted"
    )


def format_evidence_line(path: str, line_no: int, quote: str) -> str:
    """The evidence-line grammar of the v5 `done` card (turn-6 spec §3.2),
    in the exact shape `endpoints._EVIDENCE_QUOTED_RE` reads back."""
    return f"evidence: {path}:{line_no} `{quote}`"


def patch_evidence(task: Task) -> tuple[str, int, str]:
    """(path, 1-based post-patch line, verbatim post-patch quote) for a
    patch task's ideal `done` — the §2.1 mechanical rule. `search` occurs
    exactly once (validated upstream), so the replaced region starts at a
    single well-defined line; everything above it is byte-identical pre
    and post, so the first differing line at-or-after it IS the patch."""
    contents = task.files[task.target]
    offset = contents.index(task.search)
    post = contents.replace(task.search, task.replace, 1)
    first_region_line = contents.count("\n", 0, offset)  # 0-based
    pre_lines = contents.splitlines()
    post_lines = post.splitlines()
    if not (first_region_line < len(post_lines)):
        raise ValueError(
            f"patch_evidence: replaced region of {task.target!r} starts past the end of the "
            f"post-patch file -- the reference patch deleted through EOF with nothing after it"
        )
    chosen = None
    for i in range(first_region_line, min(len(pre_lines), len(post_lines))):
        if pre_lines[i] != post_lines[i] and post_lines[i].strip():
            chosen = i
            break
    if chosen is None:
        chosen = first_region_line
    quote = post_lines[chosen]
    if not quote.strip():
        raise ValueError(
            f"patch_evidence: the patched line {chosen + 1} of {task.target!r} is blank -- a "
            f"blank quote is no evidence; this is a factory bug in the template's reference patch"
        )
    return (task.target, chosen + 1, quote)


def refusal_evidence_lines(task: RefusalTask) -> list[str]:
    """The refusal ideal's evidence lines: mechanical for missing-target
    (`evidence: <target> absent` — the grammar `_EVIDENCE_ABSENT_RE` reads
    back), template ground truth (`task.evidence`) for the two
    target-present families. An empty `evidence` on a target-present task
    is a factory bug: the template was never taught its ground truth."""
    if task.family == MISSING_TARGET:
        return [f"evidence: {task.target} absent"]
    if not task.evidence:
        raise ValueError(
            f"refusal_evidence_lines: target-present template {task.name!r} carries no evidence "
            f"triples -- populate them via evidence_line_of (turn-7 spec §2.2)"
        )
    return [format_evidence_line(path, line_no, quote) for path, line_no, quote in task.evidence]


def to_v5_task(task: Task | RefusalTask) -> Task | RefusalTask:
    """The same task with its `done` ideal replaced by the full declared
    v5 block. Prose is the existing `summary`/`refusal_reason`,
    byte-identical — the trained prose shape carries over; only the
    declaration and evidence lines are new. Everything else on the task
    (goal, files, shape, grant) is untouched, which is what lets this run
    AFTER validation/dedup/gate screening without re-running any of them."""
    if isinstance(task, RefusalTask):
        return task._replace(
            refusal_reason=done_v5(
                outcome="refused",
                reason=FAMILY_TO_REASON[task.family],
                evidence_lines=refusal_evidence_lines(task),
                prose=task.refusal_reason,
            )
        )
    path, line_no, quote = patch_evidence(task)
    return task._replace(
        summary=done_v5(
            outcome="patched",
            reason="fixed",
            evidence_lines=[format_evidence_line(path, line_no, quote)],
            prose=task.summary,
        )
    )
