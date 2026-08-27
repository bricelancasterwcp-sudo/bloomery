"""Journal/ledger readers for `tools.memory_battery.recompute` (design spec
§4/§5; task-4 brief). Pure I/O and row-shape indexing only -- no cross-arm
join logic (that lives in `recompute_join.py`) and no statistics (that
lives in `recompute_bootstrap.py`). Split out of `recompute.py` to keep
each file under the house 800-line ceiling (`coding-style.md`); the public
entry point stays `tools.memory_battery.recompute.recompute`.

Per design spec §5: "journal bytes are the only source any quoted number
may have." Every function here reads exactly the row shapes
`bloomery-core/src/journal.rs`'s `Event` enum and `driver.py`'s `Ledger`
define -- see `recompute.py`'s module docstring for the full citation of
where each journal/ledger file and row shape comes from.
"""

from __future__ import annotations

import json
from pathlib import Path
from typing import Any


def _read_jsonl(path: Path) -> list[dict[str, Any]]:
    """Reads a JSONL file into a list of row dicts. A MISSING file (an arm
    directory that never wrote a given journal) is treated as an empty
    file -- callers see zero rows, never an exception, since an absent
    ``tasks.jsonl``/``boot-*.jsonl`` is itself informative (everything for
    that arm ends up in ``dropped``, not a crash). A file that EXISTS but
    contains an unparseable line is a hard error, matching
    ``bloomery-core/src/journal.rs``'s own ``replay`` contract: "a corrupt
    journal must fail loudly rather than silently skip events" (project law
    7) -- recompute inherits that discipline rather than quietly dropping a
    bad row from a cost sum."""
    if not path.exists():
        return []
    rows: list[dict[str, Any]] = []
    with path.open("r", encoding="utf-8") as handle:
        for line_number, raw_line in enumerate(handle, start=1):
            line = raw_line.strip()
            if not line:
                continue
            try:
                rows.append(json.loads(line))
            except json.JSONDecodeError as exc:
                raise ValueError(f"{path}: unparseable JSONL at line {line_number}: {exc}") from exc
    return rows


def _read_ledger(path: Path) -> tuple[dict[tuple[Any, Any], dict[str, Any]], list[dict[str, Any]]]:
    """Splits one arm's ledger into its two row shapes (``driver.py``'s
    docstring: "distinguished by the presence of `event`"): task-half rows,
    indexed by ``(phase, task)`` for the task_id join, and identity rows
    (R-PF-B1). A duplicate ``(phase, task)`` key (never produced by
    ``driver.py``'s own append-only, never-retried protocol) has its LAST
    occurrence win, matching append-only "the newest row is the ledger's
    current word on this key" semantics."""
    task_halves: dict[tuple[Any, Any], dict[str, Any]] = {}
    identity_rows: list[dict[str, Any]] = []
    for row in _read_jsonl(path):
        if row.get("event") == "identity":
            identity_rows.append(row)
        else:
            task_halves[(row.get("phase"), row.get("task"))] = row
    return task_halves, identity_rows


def _index_memory_stamps(tasks_journal_rows: list[dict[str, Any]]) -> dict[str, dict[str, Any]]:
    """``task_id`` -> its one ``MemoryStamp`` row (``registry.rs``: written
    exactly once per spawned task, "including tasks that ran with the organ
    off" -- design spec §4's lens-travels-with-verdict rule)."""
    return {row["task_id"]: row for row in tasks_journal_rows if row.get("event") == "MemoryStamp"}


def _done_agent_ids(tasks_journal_rows: list[dict[str, Any]]) -> set[str]:
    """Success (task-4 brief, verbatim): "a TaskStep with verb=='done'
    exists" for the agent id."""
    return {
        row["id"]
        for row in tasks_journal_rows
        if row.get("event") == "TaskStep" and row.get("verb") == "done"
    }


def _task_step_count_by_agent(tasks_journal_rows: list[dict[str, Any]]) -> dict[str, int]:
    counts: dict[str, int] = {}
    for row in tasks_journal_rows:
        if row.get("event") == "TaskStep":
            counts[row["id"]] = counts.get(row["id"], 0) + 1
    return counts


def _task_step_duration_by_agent(tasks_journal_rows: list[dict[str, Any]]) -> dict[str, int]:
    """Journal-derived "wall" for the advisory step/wall medians (design
    spec §4): the sum of ``TaskStep.duration_ms`` per agent. Deliberately
    NOT the ledger's ``wall_s`` -- ``driver.py``'s own docstring: "No number
    in this file is ever the number a findings doc cites" -- so the
    advisory wall endpoint is sourced from the journal's own per-step
    durations instead, keeping it inside the ledger-independence
    invariant."""
    totals: dict[str, int] = {}
    for row in tasks_journal_rows:
        if row.get("event") == "TaskStep":
            totals[row["id"]] = totals.get(row["id"], 0) + row.get("duration_ms", 0)
    return totals


def _completion_tokens_by_agent(
    boot_journal_rows: list[dict[str, Any]], source: Path | str
) -> dict[str, int]:
    """cost(task) join (task-4 brief, verbatim): "sum(completion_tokens
    over the task's agent's InferCompleted rows)" -- summed here across
    EVERY ``InferCompleted`` row for a given agent id, which is what makes
    a re-ask (two ``InferCompleted`` rows for the same agent) pay its real,
    summed cost rather than only its first reply's. ``source`` is the boot
    journal these rows were read from, carried only so a malformed row can
    name its own file.

    **Branch-review finding C-2 fix.** The old ``row.get(
    "completion_tokens", 0)`` priced a malformed ``InferCompleted`` row at
    ZERO instead of failing -- the same manufactured-cost shape finding C1
    already killed at the arm level, but one row deep and therefore
    invisible in every ``dropped`` list: a uniform serialization drift
    (every row losing the field) reads as "every task cost 0" in BOTH
    arms, which is a verdict PASS at ``delta_min`` 0.0 (probe-proven), not
    a named failure. This file's own contract for a corrupt journal is
    fail-loud (see ``_read_jsonl``: "a corrupt journal must fail loudly
    rather than silently skip events" -- project law 7), and a cost row
    that cannot be read is exactly that."""
    totals: dict[str, int] = {}
    for row_number, row in enumerate(boot_journal_rows, start=1):
        if row.get("event") != "InferCompleted":
            continue
        if "completion_tokens" not in row:
            raise ValueError(
                f"{source}: InferCompleted row {row_number} (agent id {row.get('id')!r}) "
                f"carries no 'completion_tokens' field -- a cost row that cannot be read "
                f"is a hard failure, never a silent 0 (review finding C-2). Row: {row!r}"
            )
        totals[row["id"]] = totals.get(row["id"], 0) + row["completion_tokens"]
    return totals


def _row_counts(tasks_journal_rows: list[dict[str, Any]]) -> dict[str, int]:
    """Design spec §4 advisory: "the stamp/mint/contradiction row counts" --
    raw per-arm totals, unfiltered by join success or phase."""
    return {
        "stamp": sum(1 for row in tasks_journal_rows if row.get("event") == "MemoryStamp"),
        "mint": sum(1 for row in tasks_journal_rows if row.get("event") == "MemoryMint"),
        "contradicted": sum(
            1 for row in tasks_journal_rows if row.get("event") == "MemoryContradicted"
        ),
    }
