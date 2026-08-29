"""s5-weight-battery recompute (design spec
`docs/superpowers/specs/2026-08-28-s5-weight-battery-v1-design.md` §5;
plan Task 4): the single-arm analysis instrument for the §5
passive-contradiction weight.

**Single arm** `s5_off` (`[memory] enabled = true, refalsify = false`);
v1's `C`/`M` labels rejected unconditionally. Lane classification comes
from the frozen manifest's per-task `lane` field. **No RNG anywhere**:
the intervals are Wilson score intervals (deterministic closed form,
z pinned below) — spec §5's choice for proportions.

**The entailment discipline (spec §0):** mint-xor-contradict for scored
injected tasks is code-entailed; V1 checks it LIVE as a validity gate
(the stamp-audit discipline — checked, never quoted as a result), and
the registered endpoints are the SPLITS per ground-truth lane, reported
with intervals and no pass/fail bar.

Evidence sources: journals + ledger (the template's), plus the arm's
`memory/episodes.jsonl` store file (last-writer-wins replay, reused from
`recompute_pg`) and `Degraded` rows for the oversize scan and the V1
named exception class. None-vs-zero throughout; a rate over an empty
denominator is `None`, never 0.
"""

from __future__ import annotations

import argparse
import json
import math
import statistics
import sys
from pathlib import Path
from typing import Any, Sequence

from tools.memory_battery.recompute import _corpus_sha
from tools.memory_battery.recompute_bootstrap import (
    INFRA_RATE_CEILING,
    _check_arm_completeness,
    _check_identity,
)
from tools.memory_battery.recompute_join import _load_arm
from tools.memory_battery.recompute_journal import _read_ledger
from tools.memory_battery.recompute_pg import _final_episode_statuses
from tools.memory_battery.recompute_v2 import (
    FORBIDDEN_REFALSIFY_SPELLINGS,
    _arm_task_details,
)

ARM_LABEL_S5 = "s5_off"
FORBIDDEN_ARM_LABELS = frozenset({"C", "M"})

# Spec §5's per-lane matched floor — 8 = 16/2, the flagged n/2 convention.
FLOOR_S5 = 8

# The two-sided 95% normal quantile, pinned as a literal so the Wilson
# formula has one unambiguous seed-like constant to mutation-check.
WILSON_Z = 1.959963984540054

# `organ_after_run`'s scored set (registry.rs `is_scored_outcome`; G4
# protocol §3 + Amendment 1): Error is the infra bucket, unmeasured.
SCORED_STATUSES = frozenset({"Done", "StepsExhausted", "BudgetExhausted", "WindowExhausted"})

OVERSIZE_REASON_MARKER = "injection bound"

LANES = ("control", "moot", "stale")


def wilson_interval(k: int, n: int) -> tuple[float, float]:
    """The 95% Wilson score interval for k successes in n trials —
    deterministic closed form, `WILSON_Z` pinned above. Callers guard
    n > 0 (a rate over nothing is `None`, not an interval)."""
    z = WILSON_Z
    p = k / n
    denom = 1 + z * z / n
    center = p + z * z / (2 * n)
    margin = z * math.sqrt(p * (1 - p) / n + z * z / (4 * n * n))
    return (center - margin) / denom, (center + margin) / denom


def _median_or_none(values: Sequence[float]) -> float | None:
    return statistics.median(values) if values else None


def _lane_by_name(manifest_tasks: list[dict[str, Any]]) -> dict[str, str]:
    return {entry["name"]: entry["lane"] for entry in manifest_tasks}


def _event_task_ids(rows: list[dict[str, Any]], event: str) -> set[str]:
    return {row["task_id"] for row in rows if row.get("event") == event and "task_id" in row}


def _p2_task_ids_by_name(ledger_path: Path) -> dict[str, str | None]:
    task_halves, _identity = _read_ledger(Path(ledger_path))
    return {
        name: row.get("task_id")
        for (phase, name), row in task_halves.items()
        if phase == 2
    }


def _p2_status_by_name(ledger_path: Path) -> dict[str, str]:
    task_halves, _identity = _read_ledger(Path(ledger_path))
    return {name: str(row.get("status")) for (phase, name), row in task_halves.items() if phase == 2}


def _v1_conformance(
    matched_by_name: dict[str, str],
    status_by_name: dict[str, str],
    mint_ids: set[str],
    contra_ids: set[str],
    task_id_by_name: dict[str, str | None],
    journal_rows: list[dict[str, Any]],
) -> dict[str, Any]:
    """Spec §5 V1 (as amended at Task 4): every matched injected p2 task
    carries exactly one of {MemoryMint, MemoryContradicted}; a `Degraded`
    mint/contradict I/O row citing the task is the one counted exception
    class for a missing event; `Error` halves never reach the matched set
    at all (`recompute_join._load_arm` drops them, its own H3 rule), so a
    non-scored status appearing HERE is a join-contract break; both
    events, an unexplained neither, or an unscored-in-matched → INVALID."""
    degraded_reasons = [
        str(row.get("reason", "")) for row in journal_rows if row.get("event") == "Degraded"
    ]
    both: list[str] = []
    neither: list[str] = []
    degraded_explained: list[str] = []
    unscored_in_matched: list[str] = []

    for name in sorted(matched_by_name):
        task_id = task_id_by_name.get(name)
        status = status_by_name.get(name, "")
        minted = task_id in mint_ids
        contradicted = task_id in contra_ids
        if status not in SCORED_STATUSES:
            # _load_arm drops Error halves before the view (its H3 rule),
            # so a non-scored status HERE is a join-contract break.
            unscored_in_matched.append(name)
            continue
        if minted and contradicted:
            both.append(name)
        elif not minted and not contradicted:
            if task_id and any(task_id in reason for reason in degraded_reasons):
                degraded_explained.append(name)
            else:
                neither.append(name)

    invalid = bool(both or neither or unscored_in_matched)
    return {
        "both_event_names": both,
        "neither_event_names": neither,
        "degraded_explained_names": degraded_explained,
        "unscored_in_matched_names": unscored_in_matched,
        "verdict": "INVALID" if invalid else "PASS",
        "reason": None
        if not invalid
        else (
            f"V1 INVALID: both-events on {both}; unexplained neither-event on {neither} -- a "
            f"live divergence from organ_after_run's entailment is a daemon-bug discovery, not "
            f"a measurement"
        ),
    }


def _v2_stamp_audit(
    details: dict[int, dict[str, dict[str, Any]]],
    view: dict[str, Any],
    journal_rows: list[dict[str, Any]],
) -> dict[str, Any]:
    spellings = {
        detail["refalsify"]
        for phase in (1, 2)
        for detail in details[phase].values()
        if detail["refalsify"] is not None
    }
    forbidden_hits = sorted(s for s in spellings if s in FORBIDDEN_REFALSIFY_SPELLINGS)
    injected_p1 = sum(1 for mode in view["modes"][1].values() if mode == "injected")
    oversize = sum(
        1
        for row in journal_rows
        if row.get("event") == "Degraded" and OVERSIZE_REASON_MARKER in str(row.get("reason", ""))
    )
    return {
        "refalsify_all_none": not spellings,
        "non_none_spellings": sorted(spellings),
        "forbidden_spelling_hits": forbidden_hits,
        "injected_p1_count": injected_p1,
        "oversize_degraded_count": oversize,
        "violated": bool(spellings or forbidden_hits or injected_p1 or oversize),
    }


def _weights(
    matched_by_name: dict[str, str],
    mint_ids: set[str],
    contra_ids: set[str],
    task_id_by_name: dict[str, str | None],
) -> dict[str, Any]:
    out: dict[str, Any] = {}
    for lane in LANES:
        names = sorted(name for name, l in matched_by_name.items() if l == lane)
        # Error-status halves are dropped at the join (never matched), so
        # every matched task is scored and the denominator is the matched
        # count itself.
        contradicted = sum(1 for n in names if task_id_by_name.get(n) in contra_ids)
        minted = sum(1 for n in names if task_id_by_name.get(n) in mint_ids)
        neither = len(names) - contradicted - minted
        denominator = len(names)
        lane_out: dict[str, Any] = {
            "matched": len(names),
            "denominator": denominator,
            "contradicted": contradicted,
            "minted": minted,
            "neither": neither,
            "rate_contradicted": (contradicted / denominator) if denominator else None,
            "rate_minted": (minted / denominator) if denominator else None,
            "wilson_contradicted": list(wilson_interval(contradicted, denominator))
            if denominator
            else None,
            "wilson_minted": list(wilson_interval(minted, denominator)) if denominator else None,
        }
        out[lane] = lane_out
    return out


def recompute_s5(
    corpus_dir: str | Path,
    arm_dir: str | Path,
    ledger_path: str | Path,
    *,
    expected_digest: str | None = None,
    floor: int = FLOOR_S5,
    expected_arm_label: str = ARM_LABEL_S5,
) -> dict[str, Any]:
    """The s5 battery's pinned entry point. Output keys: `v1_conformance`,
    `v2_stamp_audit`, `v3_floors`, `weights`, `h3_infra`, `a_advisory`,
    `completeness`, `dropped`, `corpus_sha`, `lens`. JSON-native; the
    library stays permissive (completeness/identity computed, enforced
    only by `main()`)."""
    corpus_dir = Path(corpus_dir)
    manifest = json.loads((corpus_dir / "manifest.json").read_text(encoding="utf-8"))
    manifest_tasks = manifest["tasks"]
    n = len(manifest_tasks)
    lane_by_name = _lane_by_name(manifest_tasks)

    arm = _load_arm(Path(arm_dir), Path(ledger_path), manifest_tasks)

    observed_labels = arm["ledger_arm_labels"]
    forbidden = [label for label in observed_labels if label in FORBIDDEN_ARM_LABELS]
    if observed_labels != [expected_arm_label] or forbidden:
        raise ValueError(
            f"recompute_s5: arm-label check failed: ledger carries {observed_labels}, expected "
            f"exactly ['{expected_arm_label}']; forbidden v1 labels: {forbidden}"
        )

    completeness = _check_arm_completeness(expected_arm_label, arm["ledger_task_half_count"], n)
    details = _arm_task_details(arm, Path(ledger_path))
    rows = arm["tasks_journal_rows"]

    task_id_by_name = _p2_task_ids_by_name(Path(ledger_path))
    status_by_name = _p2_status_by_name(Path(ledger_path))
    mint_ids = _event_task_ids(rows, "MemoryMint")
    contra_ids = _event_task_ids(rows, "MemoryContradicted")

    # Matched set (spec §4): p2 mode "injected", restricted to the
    # non-dropped joined names the view already decided are valid.
    matched_by_name = {
        name: lane_by_name[name]
        for name, mode in arm["view"]["modes"][2].items()
        if mode == "injected" and name in lane_by_name
    }

    v1 = _v1_conformance(matched_by_name, status_by_name, mint_ids, contra_ids, task_id_by_name, rows)
    v2 = _v2_stamp_audit(details, arm["view"], rows)
    weights = _weights(matched_by_name, mint_ids, contra_ids, task_id_by_name)

    lanes_under_floor = sorted(lane for lane in LANES if weights[lane]["matched"] < floor)
    v3 = {
        "floor": floor,
        "matched_by_lane": {lane: weights[lane]["matched"] for lane in LANES},
        "lanes_under_floor": lanes_under_floor,
        "verdict": "UNMEASURABLE" if lanes_under_floor else "PASS",
        "reason": None
        if not lanes_under_floor
        else f"V3: lanes {lanes_under_floor} under the matched floor {floor} -- their weights are UNMEASURABLE",
    }

    # H3: dropped halves + Error-status halves over 2n (Error is excluded
    # from the weights as unmeasured, so it must be counted as infra here
    # rather than vanishing).
    # `dropped` already contains every Error-status half (recompute_join's
    # H3 rule), so infra is counted exactly once from it.
    infra_count = len(arm["dropped"])
    infra_rate = infra_count / (2 * n) if n else None
    h3 = {
        "ceiling": INFRA_RATE_CEILING,
        "infra_count": infra_count,
        "dropped_halves": len(arm["dropped"]),
        "task_halves": 2 * n,
        "infra_rate": infra_rate,
        "violated": infra_rate is not None and infra_rate > INFRA_RATE_CEILING,
    }

    # Advisory: per-lane p2 medians, terminal statuses, mints, patch
    # attempts (TaskStep verb rows -- attempts, not successes), store.
    agent_by_p2_name: dict[str, str | None] = {}
    stamps_by_task_id = {
        row["task_id"]: row for row in rows if row.get("event") == "MemoryStamp"
    }
    for name, task_id in task_id_by_name.items():
        stamp = stamps_by_task_id.get(task_id) if task_id else None
        agent_by_p2_name[name] = stamp.get("id") if stamp else None
    patch_agents = {
        row["id"] for row in rows if row.get("event") == "TaskStep" and row.get("verb") == "patch"
    }
    mint_p1 = sum(
        1
        for row in rows
        if row.get("event") == "MemoryMint" and arm["task_id_to_phase"].get(row.get("task_id")) == 1
    )
    a_advisory: dict[str, Any] = {
        "mint_count_p1": mint_p1,
        "mint_rate_p1": mint_p1 / n if n else None,
        "store_final_status_counts": {},
        "by_lane": {},
    }
    store_statuses = _final_episode_statuses(Path(arm_dir) / "memory" / "episodes.jsonl")
    for status in store_statuses.values():
        a_advisory["store_final_status_counts"][status] = (
            a_advisory["store_final_status_counts"].get(status, 0) + 1
        )
    for lane in LANES:
        lane_names = [name for name, l in lane_by_name.items() if l == lane]
        p2_costs = [arm["view"]["costs"][2][n_] for n_ in lane_names if n_ in arm["view"]["costs"][2]]
        p2_walls = [
            details[2][n_]["wall_ms"]
            for n_ in lane_names
            if n_ in details[2] and details[2][n_]["wall_ms"] is not None
        ]
        statuses: dict[str, int] = {}
        for n_ in lane_names:
            s = status_by_name.get(n_)
            if s is not None:
                statuses[s] = statuses.get(s, 0) + 1
        a_advisory["by_lane"][lane] = {
            "p2_token_median": _median_or_none(p2_costs),
            "p2_wall_median_ms": _median_or_none(p2_walls),
            "p2_terminal_status_counts": dict(sorted(statuses.items())),
            "p2_patch_attempt_tasks": sum(
                1 for n_ in lane_names if agent_by_p2_name.get(n_) in patch_agents
            ),
        }

    identity = _check_identity(expected_arm_label, arm["identity_by_phase"], expected_digest)
    lens = {
        "instrument": "s5-weight-battery-v1",
        "arm_label": expected_arm_label,
        "floor": floor,
        "wilson_z": WILSON_Z,
        "n": n,
        "n_per_lane": manifest.get("n_per_lane"),
        "source_paths": {
            "corpus_dir": str(corpus_dir),
            "arm_dir": str(arm_dir),
            "ledger": str(ledger_path),
        },
        "expected_digest": expected_digest,
        "identity": identity,
    }

    return {
        "v1_conformance": v1,
        "v2_stamp_audit": v2,
        "v3_floors": v3,
        "weights": weights,
        "h3_infra": h3,
        "a_advisory": a_advisory,
        "completeness": completeness,
        "dropped": arm["dropped"],
        "corpus_sha": _corpus_sha(manifest),
        "lens": lens,
    }


def _cli_fatal_checks(result: dict[str, Any], expected_digest: str) -> list[str]:
    fatals: list[str] = []
    completeness = result["completeness"]
    if completeness["violated"]:
        fatals.append(
            f"memory_battery.recompute_s5: FATAL: arm incomplete -- "
            f"{completeness['actual_task_halves']} task-half row(s), expected "
            f"{completeness['expected_task_halves']} -- {completeness['reason']}"
        )
    identity = result["lens"]["identity"]
    if identity["violated"]:
        fatals.append(
            f"memory_battery.recompute_s5: FATAL: identity mismatch -- "
            f"phase1_digest={identity['phase1_digest']!r} "
            f"phase2_digest={identity['phase2_digest']!r} expected={expected_digest!r}"
        )
    return fatals


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description=(
            "s5-weight-battery recompute (design spec §5): derives V1/V2/V3, the per-lane "
            "weights with Wilson intervals, H3, and the advisories from journal, ledger, and "
            "store bytes plus the frozen manifest, single arm s5_off."
        )
    )
    parser.add_argument("--corpus-dir", type=Path, required=True)
    parser.add_argument("--arm-dir", type=Path, required=True)
    parser.add_argument("--ledger", type=Path, required=True)
    parser.add_argument("--expected-digest", required=True)
    parser.add_argument("--floor", type=int, default=FLOOR_S5)
    parser.add_argument("--expected-arm-label", default=ARM_LABEL_S5)
    args = parser.parse_args(argv)

    try:
        result = recompute_s5(
            args.corpus_dir,
            args.arm_dir,
            args.ledger,
            expected_digest=args.expected_digest,
            floor=args.floor,
            expected_arm_label=args.expected_arm_label,
        )
    except Exception as exc:  # noqa: BLE001 -- last-resort net, house pattern
        print(f"memory_battery.recompute_s5: FATAL: {exc!r}", file=sys.stderr)
        return 1

    fatals = _cli_fatal_checks(result, args.expected_digest)
    if fatals:
        for message in fatals:
            print(message, file=sys.stderr)
        return 1

    print(json.dumps(result, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
