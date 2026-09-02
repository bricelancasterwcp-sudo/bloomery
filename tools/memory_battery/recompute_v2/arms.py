"""Per-arm task detail and the arm-label check.

Split out of `recompute_v2.py` on 2026-09-01 (carried-debt slice D). These are
the label-agnostic pieces `recompute_pg.py` and `recompute_s5.py` already cite
and reuse, which is what makes them the natural first module to carve out.
"""

from __future__ import annotations
import argparse
import json
import random
import statistics
import sys
from pathlib import Path
from typing import Any, Sequence
from tools.memory_battery.recompute import _corpus_sha
from tools.memory_battery.recompute_bootstrap import (
    HYGIENE_SE_MULTIPLIER,
    INFRA_RATE_CEILING,
    _bootstrap_diff_independent,
    _check_arm_completeness,
    _check_identity,
)
from tools.memory_battery.recompute_join import _load_arm
from tools.memory_battery.recompute_journal import (
    _index_memory_stamps,
    _read_ledger,
    _task_step_duration_by_agent,
)
from tools.memory_battery.recompute_v2.constants import (  # noqa: F401
    ARM_LABEL_M_PRIME,
    ARM_LABEL_R,
    B_V2,
    FORBIDDEN_ARM_LABELS,
    FORBIDDEN_REFALSIFY_SPELLINGS,
    NO_PROBE_SPELLING_KEY,
    SEED_V2,
    TOLERATED_NON_PREMISE_HELD_SPELLINGS,
)



def _median_or_none(values: Sequence[float]) -> float | None:
    """Local, NOT imported from `recompute_bootstrap.py` -- see module
    docstring: this keeps mutation check #1 ("median -> mean in G1")
    surgical to this file."""
    return statistics.median(values) if values else None



# Per-task detail join (refalsify spelling + wall_ms), name-keyed.


def _arm_task_details(
    arm_result: dict[str, Any], ledger_path: Path
) -> dict[int, dict[str, dict[str, Any]]]:
    """One arm's (phase, task name) -> {"refalsify", "wall_ms"} detail,
    restricted to exactly the non-dropped, joined task-halves `_load_arm`
    already decided are valid (`arm_result["view"]["modes"][phase]`'s own
    keys) -- never re-derives drop/join validity itself (`_measure_arm`'s
    job, in v1-immutable `recompute_join.py`). Fills a gap `_load_arm`'s
    view leaves open: `refalsify` isn't exposed at all, and `wall_ms` is
    only a flat unkeyed list -- re-reads the ledger + the arm's own
    `tasks_journal_rows` via `recompute_journal.py`'s reused row parsers.

    **Mutation check #2 lives here**: `stamp.get("id")` (the AGENT id,
    the correct key into `step_walls`) versus `task_id` (the ledger's
    task-half id, the WRONG field).

    **None-vs-zero (forward-looking instrument-honesty fix).** A joined
    task whose agent id never wrote a `TaskStep` row (no entry in
    `step_walls`) yields `wall_ms: None`, never a silent `0` -- a
    stepless-but-conducted task must not look like a zero-duration
    measurement and drag A1's wall medians toward zero (the named bug
    class: "a value that looks like a measurement but is not"). `_a1_wall`
    below excludes `None` walls from every median/delta/per-task
    computation and surfaces the exclusion count explicitly rather than
    ever letting it vanish silently."""
    ledger_task_halves, _identity_rows = _read_ledger(Path(ledger_path))
    stamps_by_task_id = _index_memory_stamps(arm_result["tasks_journal_rows"])
    step_walls = _task_step_duration_by_agent(arm_result["tasks_journal_rows"])

    details: dict[int, dict[str, dict[str, Any]]] = {1: {}, 2: {}}
    for phase in (1, 2):
        for name in arm_result["view"]["modes"][phase]:
            ledger_row = ledger_task_halves.get((phase, name))
            task_id = ledger_row.get("task_id") if ledger_row else None
            stamp = stamps_by_task_id.get(task_id) if task_id else None
            agent_id = stamp.get("id") if stamp else None
            details[phase][name] = {
                "refalsify": stamp.get("refalsify") if stamp else None,
                "wall_ms": step_walls.get(agent_id, None) if agent_id else None,
            }
    return details



# Arm-label honesty (design spec §5; task-1 brief's `expected_arm_labels`).


def _check_arm_labels(
    observed_m_prime: list[str], observed_r: list[str], expected_arm_labels: tuple[str, str]
) -> None:
    """Raises `ValueError` (a hard reject, not a soft verdict field -- a
    ledger that isn't even about the arm its slot claims produces no
    trustworthy number at all) when either slot's ledger `arm` label(s)
    don't match `expected_arm_labels`, OR when either slot carries a
    FORBIDDEN v1 label (`"C"`/`"M"`) -- the forbidden check is
    UNCONDITIONAL: it fires even if a caller's own `expected_arm_labels`
    tried to allow it (task-1 brief: "the label check still REJECTS v1's
    C/M unconditionally")."""
    expected_m_prime, expected_r = expected_arm_labels
    problems: list[str] = []
    if observed_m_prime != [expected_m_prime]:
        problems.append(
            f"m_prime slot: ledger carries arm label(s) {observed_m_prime}, expected exactly "
            f"['{expected_m_prime}']"
        )
    if observed_r != [expected_r]:
        problems.append(
            f"r slot: ledger carries arm label(s) {observed_r}, expected exactly ['{expected_r}']"
        )
    forbidden_m_prime = [label for label in observed_m_prime if label in FORBIDDEN_ARM_LABELS]
    forbidden_r = [label for label in observed_r if label in FORBIDDEN_ARM_LABELS]
    if forbidden_m_prime or forbidden_r:
        problems.append(
            f"forbidden v1 arm label(s) detected -- m_prime slot: {forbidden_m_prime}, r slot: "
            f"{forbidden_r} -- 'C'/'M' are v1-only labels, rejected unconditionally regardless of "
            f"expected_arm_labels"
        )
    if problems:
        raise ValueError("recompute_v2: arm-label check failed: " + "; ".join(problems))
