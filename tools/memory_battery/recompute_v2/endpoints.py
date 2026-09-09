"""The registered endpoints: G1, G2, the stamp audit, H2, H3, H4 and the A1
wall.

Split out of `recompute_v2.py` on 2026-09-01 (carried-debt slice D). One
function per registered endpoint, so a prereg reading of "what was measured"
maps to a file rather than to a range of lines.
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
from tools.memory_battery.recompute_v2.arms import _arm_task_details, _median_or_none  # noqa: F401



# G1 -- token preservation (equivalence gate, design spec §4).


def _g1_token_preservation(
    rng: random.Random, view_m_prime: dict[str, Any], view_r: dict[str, Any], *, b: int
) -> dict[str, Any]:
    """Design spec §4, G1, quoted verbatim: "|median_R,p2 - median_M',p2|
    <= 2 x SE_boot(median_R,p2 - median_M',p2)" -- resampling unit = tasks,
    each arm's phase-2 tasks resampled independently, medians over
    non-dropped tasks. Floor-saturation clause: see module docstring's
    judgment-call note (headroom checked on BOTH arms, symmetric
    generalization of v1 E1's one-sided headroom formula)."""
    costs_m_prime_p2 = list(view_m_prime["costs"][2].values())
    costs_r_p2 = list(view_r["costs"][2].values())
    median_m_prime = _median_or_none(costs_m_prime_p2)
    median_r = _median_or_none(costs_r_p2)
    base = {
        "median_m_prime_p2": median_m_prime,
        "median_r_p2": median_r,
        "n_m_prime_p2": len(costs_m_prime_p2),
        "n_r_p2": len(costs_r_p2),
    }

    if median_m_prime is None or median_r is None:
        return {
            **base,
            "diff": None,
            "se_boot": None,
            "band": None,
            "headroom_m_prime": None,
            "headroom_r": None,
            "verdict": "UNMEASURABLE",
            "reason": "G1 unmeasurable: no non-dropped phase-2 tasks in one or both arms",
        }

    diff = median_r - median_m_prime
    se_boot = statistics.pstdev(_bootstrap_diff_independent(rng, costs_r_p2, costs_m_prime_p2, b=b))
    band = HYGIENE_SE_MULTIPLIER * se_boot
    headroom_m_prime = median_m_prime - min(costs_m_prime_p2)
    headroom_r = median_r - min(costs_r_p2)
    base.update(
        {"diff": diff, "se_boot": se_boot, "band": band, "headroom_m_prime": headroom_m_prime, "headroom_r": headroom_r}
    )

    if headroom_m_prime < band or headroom_r < band:
        return {
            **base,
            "verdict": "UNMEASURABLE",
            "reason": (
                f"G1 unmeasurable: floor-saturation headroom clause (module docstring judgment "
                f"call) -- headroom_m_prime={headroom_m_prime}, headroom_r={headroom_r}, both "
                f"must be >= band={band}"
            ),
        }

    verdict = "PASS" if abs(diff) <= band else "FAIL"
    reason = None if verdict == "PASS" else f"G1 FAIL: |median_R,p2 - median_M',p2| = {abs(diff)} > band = {band}"
    return {**base, "verdict": verdict, "reason": reason}



# G2 -- injection preservation (exact-count gate, design spec §4).


def _g2_injection_preservation(view_m_prime: dict[str, Any], view_r: dict[str, Any]) -> dict[str, Any]:
    """Design spec §4, G2, quoted verbatim: "injected_R,p2 = injected_M',p2
    ... A deficit in R FAILS. An excess is impossible by construction;
    observing one is an instrument alarm, not a pass." Counted from
    `MemoryStamp` `mode:"injected"` rows over non-dropped tasks (`view`'s
    keys are already exactly the non-dropped, joined set)."""
    injected_m_prime = sum(1 for mode in view_m_prime["modes"][2].values() if mode == "injected")
    injected_r = sum(1 for mode in view_r["modes"][2].values() if mode == "injected")

    if injected_r == injected_m_prime:
        verdict, reason = "PASS", None
    elif injected_r < injected_m_prime:
        verdict = "FAIL"
        reason = (
            f"G2 FAIL: deficit -- injected_R={injected_r} < injected_M'={injected_m_prime} "
            f"(the probe silenced or poisoned an episode)"
        )
    else:
        verdict = "ALARM"
        reason = (
            f"G2 ALARM: excess -- injected_R={injected_r} > injected_M'={injected_m_prime}, "
            f"impossible by construction under a deterministic byte-reset corpus; instrument "
            f"alarm, not a pass"
        )
    return {
        "injected_count_m_prime": injected_m_prime,
        "injected_count_r": injected_r,
        "verdict": verdict,
        "reason": reason,
    }



# Stamp audit (design spec §4, "gating, instrument honesty").


def _stamp_audit(
    details_m_prime: dict[int, dict[str, dict[str, Any]]],
    details_r: dict[int, dict[str, dict[str, Any]]],
    view_m_prime: dict[str, Any],
    view_r: dict[str, Any],
) -> dict[str, Any]:
    """Design spec §4, stamp audit, quoted verbatim: "over R-p2's
    non-dropped tasks, every mode:'injected' stamp carries refalsify:
    'premise_held'" (`premise_held_complete`); "the spellings passed/
    failed appear nowhere in either arm" (`forbidden_spellings_absent`);
    "premise_gone expected count: 0" (`premise_gone_zero`) -- any
    occurrence "is an instrument alarm, not task data," scoped here across
    BOTH arms/phases (a workspace-reset failure is not arm-specific by
    construction).

    **`premise_held_complete`'s exact rule (review finding IMPORTANT-2).**
    Spec §4's next sentence names `inconclusive`/`skipped_ungranted` as
    "tolerated ... counted and named individually" -- NOT as a
    `premise_held_complete` violation. An injected stamp whose refalsify
    is in `TOLERATED_NON_PREMISE_HELD_SPELLINGS` is excluded from
    `offending_premise_held`; both are still tallied into `counts`."""
    counts: dict[str, dict[int, dict[str, int]]] = {
        "m_prime": {1: {}, 2: {}},
        "r": {1: {}, 2: {}},
    }

    def _tally(counts_phase: dict[str, int], details_phase: dict[str, dict[str, Any]]) -> None:
        for info in details_phase.values():
            key = info["refalsify"] if info["refalsify"] is not None else NO_PROBE_SPELLING_KEY
            counts_phase[key] = counts_phase.get(key, 0) + 1

    for phase in (1, 2):
        _tally(counts["m_prime"][phase], details_m_prime[phase])
        _tally(counts["r"][phase], details_r[phase])

    offending_premise_held = [
        {"task": name, "mode": view_r["modes"][2].get(name), "refalsify": info["refalsify"]}
        for name, info in sorted(details_r[2].items())
        if view_r["modes"][2].get(name) == "injected"
        and info["refalsify"] != "premise_held"
        and info["refalsify"] not in TOLERATED_NON_PREMISE_HELD_SPELLINGS
    ]
    premise_held_complete = not offending_premise_held

    forbidden_hits = []
    for arm_label, details in (("m_prime", details_m_prime), ("r", details_r)):
        for phase in (1, 2):
            for name, info in sorted(details[phase].items()):
                if info["refalsify"] in FORBIDDEN_REFALSIFY_SPELLINGS:
                    forbidden_hits.append(
                        {"arm": arm_label, "phase": phase, "task": name, "refalsify": info["refalsify"]}
                    )
    forbidden_spellings_absent = not forbidden_hits

    premise_gone_hits = []
    for arm_label, details in (("m_prime", details_m_prime), ("r", details_r)):
        for phase in (1, 2):
            for name, info in sorted(details[phase].items()):
                if info["refalsify"] == "premise_gone":
                    premise_gone_hits.append({"arm": arm_label, "phase": phase, "task": name})
    premise_gone_zero = not premise_gone_hits

    inconclusive_count = sum(
        counts[arm][phase].get("inconclusive", 0) for arm in ("m_prime", "r") for phase in (1, 2)
    )
    skipped_ungranted_count = sum(
        counts[arm][phase].get("skipped_ungranted", 0) for arm in ("m_prime", "r") for phase in (1, 2)
    )

    return {
        "counts": counts,
        "premise_held_complete": premise_held_complete,
        "offending_premise_held": offending_premise_held,
        "forbidden_spellings_absent": forbidden_spellings_absent,
        "forbidden_spelling_hits": forbidden_hits,
        "premise_gone_zero": premise_gone_zero,
        "premise_gone_hits": premise_gone_hits,
        "inconclusive_count": inconclusive_count,
        "skipped_ungranted_count": skipped_ungranted_count,
    }



# H2 -- first-exposure equivalence (hygiene, design spec §4).


def _h2_p1_equivalence(
    rng: random.Random, view_m_prime: dict[str, Any], view_r: dict[str, Any], *, b: int
) -> dict[str, Any]:
    """Design spec §4, H2, quoted verbatim: "|median_M',p1 - median_R,p1|
    within 2 x SE_boot (tokens). No probe can fire in p1 ... so a gap is
    instrument error -> run INVALID." Same equivalence shape as G1, no
    floor-saturation clause (design spec §4's Hygiene section states only
    the band check for H2, not the kill-criteria's UNMEASURABLE rule,
    which is G1's own gate-specific clause)."""
    costs_m_prime_p1 = list(view_m_prime["costs"][1].values())
    costs_r_p1 = list(view_r["costs"][1].values())
    median_m_prime = _median_or_none(costs_m_prime_p1)
    median_r = _median_or_none(costs_r_p1)
    base = {
        "median_m_prime_p1": median_m_prime,
        "median_r_p1": median_r,
        "n_m_prime_p1": len(costs_m_prime_p1),
        "n_r_p1": len(costs_r_p1),
    }

    if median_m_prime is None or median_r is None:
        return {
            **base,
            "diff": None,
            "se_boot": None,
            "band": None,
            "violated": True,
            "reason": "H2: insufficient non-dropped phase-1 tasks in one or both arms",
        }

    diff = median_r - median_m_prime
    se_boot = statistics.pstdev(_bootstrap_diff_independent(rng, costs_r_p1, costs_m_prime_p1, b=b))
    band = HYGIENE_SE_MULTIPLIER * se_boot
    violated = abs(diff) > band
    reason = None
    if violated:
        reason = (
            f"H2 first-exposure-equivalence violation: |median_M',p1 - median_R,p1| = {abs(diff)} "
            f"> 2*SE_boot = {band}"
        )
    return {**base, "diff": diff, "se_boot": se_boot, "band": band, "violated": violated, "reason": reason}



# H3 -- infra rate (hygiene, design spec §4).


def _h3_infra(dropped_m_prime: list[dict[str, Any]], dropped_r: list[dict[str, Any]], n: int) -> dict[str, Any]:
    """Design spec §4, H3, quoted verbatim: "infra rate <= 5% per arm ...
    Above 5% -> infrastructure kill." Denominator is the fixed
    manifest-derived task-half count (n x 2), matching v1's own
    `_check_h3` discipline; re-implemented locally (not imported) purely
    to keep the reason string honestly labeled m_prime/r rather than
    v1's baked "C="/"M=" text."""
    task_halves = n * 2
    m_prime_infra = sum(1 for entry in dropped_m_prime if entry["infra"])
    r_infra = sum(1 for entry in dropped_r if entry["infra"])
    m_prime_rate = (m_prime_infra / task_halves) if task_halves else None
    r_rate = (r_infra / task_halves) if task_halves else None
    violated = (m_prime_rate is not None and m_prime_rate > INFRA_RATE_CEILING) or (
        r_rate is not None and r_rate > INFRA_RATE_CEILING
    )
    reason = None
    if violated:
        reason = f"H3 infra-rate kill: m_prime={m_prime_rate} r={r_rate} > ceiling {INFRA_RATE_CEILING}"
    return {
        "m_prime_infra_count": m_prime_infra,
        "m_prime_task_halves": task_halves,
        "m_prime_infra_rate": m_prime_rate,
        "r_infra_count": r_infra,
        "r_task_halves": task_halves,
        "r_infra_rate": r_rate,
        "violated": violated,
        "reason": reason,
        "ceiling": INFRA_RATE_CEILING,
    }



# H4 -- advisory mint/retrieval rates, per arm (design spec §4).


def _h4_advisory(
    view_m_prime: dict[str, Any],
    view_r: dict[str, Any],
    tasks_journal_m_prime: list[dict[str, Any]],
    task_id_to_phase_m_prime: dict[Any, Any],
    tasks_journal_r: list[dict[str, Any]],
    task_id_to_phase_r: dict[Any, Any],
    n: int,
) -> dict[str, Any]:
    """Design spec §4, H4 (advisory): "mint rate in each arm's p1;
    retrieval rate in each arm's p2" -- both arms mint/retrieve now (both
    are memory-on under v2), unlike v1's H4 which only had arm M to
    report. Denominator is manifest n (ITT), matching v1's own
    `_mint_rate_p1`/h4 discipline."""

    def _mint_rate(tasks_journal_rows: list[dict[str, Any]], task_id_to_phase: dict[Any, Any]) -> tuple[int, float | None]:
        count = sum(
            1
            for row in tasks_journal_rows
            if row.get("event") == "MemoryMint" and task_id_to_phase.get(row.get("task_id")) == 1
        )
        return count, (count / n if n else None)

    def _retrieval_rate(view: dict[str, Any]) -> tuple[int, float | None]:
        count = sum(1 for mode in view["modes"][2].values() if mode == "injected")
        return count, (count / n if n else None)

    mint_count_m_prime, mint_rate_m_prime = _mint_rate(tasks_journal_m_prime, task_id_to_phase_m_prime)
    mint_count_r, mint_rate_r = _mint_rate(tasks_journal_r, task_id_to_phase_r)
    retrieval_count_m_prime, retrieval_rate_m_prime = _retrieval_rate(view_m_prime)
    retrieval_count_r, retrieval_rate_r = _retrieval_rate(view_r)

    return {
        "m_prime": {
            "mint_count_p1": mint_count_m_prime,
            "mint_rate_p1": mint_rate_m_prime,
            "retrieval_count_p2": retrieval_count_m_prime,
            "retrieval_rate_p2": retrieval_rate_m_prime,
            "n": n,
        },
        "r": {
            "mint_count_p1": mint_count_r,
            "mint_rate_p1": mint_rate_r,
            "retrieval_count_p2": retrieval_count_r,
            "retrieval_rate_p2": retrieval_rate_r,
            "n": n,
        },
    }



# A1 -- the purchased number (advisory, never gates -- design spec §4).


def _a1_wall(
    details_m_prime: dict[int, dict[str, dict[str, Any]]], details_r: dict[int, dict[str, dict[str, Any]]]
) -> dict[str, Any]:
    """Design spec §4, A1, quoted verbatim: "median wall_R,p2 -
    median wall_M',p2 ... reported beside the per-probed-retrieval
    derivation (that delta / probed-retrieval count) and beside the
    no-probe control median wall_R,p1 - median wall_M',p1 ... Per-task
    wall deltas also reported as a distribution summary."

    `probed_retrieval_count` = R-p2 stamps whose `refalsify` is not `None`
    (a probe genuinely ran) -- NOT `g2`'s injected count, since a
    `premise_gone` retrieval is also probed but never injected (design
    spec §3: happy-path corpus predicts every probe resolves
    `premise_held`, so the two counts coincide on a clean run, but the
    formula itself must count probes, not injections).

    **None-vs-zero.** `_arm_task_details` yields `wall_ms: None` for a
    joined task whose agent wrote no `TaskStep` row (stepless but
    conducted) -- every wall list below excludes those `None` entries
    from medians/deltas/per-task deltas rather than letting a phantom `0`
    pull them down, and the excluded count is surfaced per arm x phase as
    `wall_unmeasured_count` so an exclusion is visible, never silent."""

    def _measured_walls(details_phase: dict[str, dict[str, Any]]) -> list[float]:
        return [info["wall_ms"] for info in details_phase.values() if info["wall_ms"] is not None]

    def _unmeasured_count(details_phase: dict[str, dict[str, Any]]) -> int:
        return sum(1 for info in details_phase.values() if info["wall_ms"] is None)

    wall_m_prime_p1 = _measured_walls(details_m_prime[1])
    wall_r_p1 = _measured_walls(details_r[1])
    wall_m_prime_p2 = _measured_walls(details_m_prime[2])
    wall_r_p2 = _measured_walls(details_r[2])

    wall_unmeasured_count = {
        "m_prime": {1: _unmeasured_count(details_m_prime[1]), 2: _unmeasured_count(details_m_prime[2])},
        "r": {1: _unmeasured_count(details_r[1]), 2: _unmeasured_count(details_r[2])},
    }

    median_m_prime_p1 = _median_or_none(wall_m_prime_p1)
    median_r_p1 = _median_or_none(wall_r_p1)
    median_m_prime_p2 = _median_or_none(wall_m_prime_p2)
    median_r_p2 = _median_or_none(wall_r_p2)

    delta_p1 = (
        median_r_p1 - median_m_prime_p1
        if median_r_p1 is not None and median_m_prime_p1 is not None
        else None
    )
    delta_p2 = (
        median_r_p2 - median_m_prime_p2
        if median_r_p2 is not None and median_m_prime_p2 is not None
        else None
    )

    probed_retrieval_count = sum(1 for info in details_r[2].values() if info["refalsify"] is not None)
    per_probed_retrieval_ms = (
        delta_p2 / probed_retrieval_count if delta_p2 is not None and probed_retrieval_count else None
    )

    common_names = sorted(set(details_m_prime[2]) & set(details_r[2]))
    per_task = [
        {"task": name, "delta": details_r[2][name]["wall_ms"] - details_m_prime[2][name]["wall_ms"]}
        for name in common_names
        if details_r[2][name]["wall_ms"] is not None and details_m_prime[2][name]["wall_ms"] is not None
    ]
    deltas_only = [entry["delta"] for entry in per_task]

    return {
        "p2": {"median_r": median_r_p2, "median_m_prime": median_m_prime_p2, "delta": delta_p2},
        "p1_control": {"median_r": median_r_p1, "median_m_prime": median_m_prime_p1, "delta": delta_p1},
        "probed_retrieval_count": probed_retrieval_count,
        "per_probed_retrieval_ms": per_probed_retrieval_ms,
        "per_task_wall_delta_p2": {
            "per_task": per_task,
            "n": len(per_task),
            "median": _median_or_none(deltas_only),
            "min": min(deltas_only) if deltas_only else None,
            "max": max(deltas_only) if deltas_only else None,
        },
        "wall_unmeasured_count": wall_unmeasured_count,
    }
