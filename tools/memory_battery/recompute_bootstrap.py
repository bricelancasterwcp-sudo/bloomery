"""Seeded bootstrap primitives + hygiene H1-H3 + E1 for
`tools.memory_battery.recompute` (design spec §4; task-4 brief). Split out
of `recompute.py` to keep each file under the house 800-line ceiling
(`coding-style.md`); the public entry point stays
`tools.memory_battery.recompute.recompute`.

Every formula here is QUOTED from design spec §4 verbatim in the function
that implements it -- never restated with different words (task-4 brief).
"""

from __future__ import annotations

import random
import statistics
from typing import Any, Sequence

# Design spec §4/§6, task-4 brief: "Bootstrap exactly as §4 locks it:
# random.Random(20260826), B=10,000". `recompute()` creates exactly one
# seeded `random.Random(BOOTSTRAP_SEED)` instance fresh on every call (never
# module-level, never reseeded mid-call) so that identical inputs always
# consume the RNG in the identical program order -- the determinism
# invariant mutation check #3 pins (same inputs twice -> byte-identical
# `delta_min`).
BOOTSTRAP_SEED = 20260826
BOOTSTRAP_B = 10_000

# Design spec §4: "within 2 x SE_boot" (H1, H2) and "Delta_min = 2 x
# SE_boot(...)" (E1) -- one multiplier, quoted verbatim in both formulas.
HYGIENE_SE_MULTIPLIER = 2

# Design spec §4, H3: "infra rate <= 5% per arm ... Above 5% -> infrastructure
# kill".
INFRA_RATE_CEILING = 0.05


def _median_or_none(values: Sequence[float]) -> float | None:
    return statistics.median(values) if values else None


def _bootstrap_diff_independent(
    rng: random.Random, sample_first: list[int], sample_second: list[int], b: int = BOOTSTRAP_B
) -> list[float]:
    """Design spec §4: "for the cross-arm difference each arm's phase-2
    tasks resample independently" -- generalized here to any cross-ARM
    comparison (E1 is M,p2 vs C,p2; H2 is M,p1 vs C,p1; both compare
    different arms at the same phase, so both are "the cross-arm
    difference" in the spec's sense). Each of ``b`` draws independently
    resamples both samples with replacement at their own original sizes,
    and records ``median(resampled_first) - median(resampled_second)`` --
    callers pass ``(M, C)`` so every returned diff is "M minus C", matching
    the spec's own ``median_M - median_C`` ordering in both E1 and H2's
    formulas."""
    diffs: list[float] = []
    n_first, n_second = len(sample_first), len(sample_second)
    for _ in range(b):
        resampled_first = [sample_first[rng.randrange(n_first)] for _ in range(n_first)]
        resampled_second = [sample_second[rng.randrange(n_second)] for _ in range(n_second)]
        diffs.append(statistics.median(resampled_first) - statistics.median(resampled_second))
    return diffs


def _bootstrap_diff_paired(
    rng: random.Random, pairs: list[tuple[int, int]], b: int = BOOTSTRAP_B
) -> list[float]:
    """Design spec §4: "for the within-arm differences (H1, and M's
    advisory paired deltas) tasks resample as p1/p2 PAIRS." Each of ``b``
    draws resamples PAIR INDICES with replacement (preserving each drawn
    task's own p1/p2 pairing), then records
    ``median(after) - median(before)`` over that draw -- callers pass
    ``(p1_value, p2_value)`` pairs so this is always "p2 minus p1",
    matching H1's own ``median_C,p2 - median_C,p1`` ordering."""
    diffs: list[float] = []
    n = len(pairs)
    for _ in range(b):
        drawn = [pairs[rng.randrange(n)] for _ in range(n)]
        before = [pair[0] for pair in drawn]
        after = [pair[1] for pair in drawn]
        diffs.append(statistics.median(after) - statistics.median(before))
    return diffs


def _check_identity(
    arm_label: str, identity_by_phase: dict[Any, str | None], expected_digest: str | None
) -> dict[str, Any]:
    """R-PF-B1 (amended): the two ledger identity rows within one arm must
    agree with each other, and (when the caller supplied one -- see
    `recompute.py`'s docstring judgment call) with ``expected_digest``."""
    phase1_digest = identity_by_phase.get(1)
    phase2_digest = identity_by_phase.get(2)
    agree = phase1_digest is not None and phase1_digest == phase2_digest
    matches_expected: bool | None = None
    if expected_digest is not None:
        matches_expected = phase1_digest == expected_digest and phase2_digest == expected_digest
    violated = (not agree) or (expected_digest is not None and not matches_expected)

    reason = None
    if violated:
        parts = []
        if not agree:
            parts.append(
                f"{arm_label}: ledger identity rows disagree or are missing across phases "
                f"(phase1={phase1_digest!r}, phase2={phase2_digest!r})"
            )
        if expected_digest is not None and not matches_expected:
            parts.append(
                f"{arm_label}: served digest(s) phase1={phase1_digest!r} phase2={phase2_digest!r} "
                f"!= expected {expected_digest!r}"
            )
        reason = "; ".join(parts)
    return {
        "phase1_digest": phase1_digest,
        "phase2_digest": phase2_digest,
        "agree": agree,
        "matches_expected": matches_expected,
        "violated": violated,
        "reason": reason,
    }


def _check_h1(rng: random.Random, view_c: dict[str, Any], manifest_tasks: list[dict[str, Any]]) -> dict[str, Any]:
    """Design spec §4, H1 (control stability), quoted verbatim: "|median_C,
    p2 - median_C,p1| within 2 x SE_boot of that difference. A violation
    means ordering/warmup contaminates phase 2 -> run INVALID." Medians use
    each phase's own full non-dropped set (ITT); the paired bootstrap that
    estimates SE_boot uses the intersection of tasks non-dropped in BOTH
    phases (a paired resample is only defined where both values exist)."""
    names = [task["name"] for task in manifest_tasks]
    pairs = [
        (view_c["costs"][1][name], view_c["costs"][2][name])
        for name in names
        if name in view_c["costs"][1] and name in view_c["costs"][2]
    ]
    median_p1 = _median_or_none(list(view_c["costs"][1].values()))
    median_p2 = _median_or_none(list(view_c["costs"][2].values()))

    if median_p1 is None or median_p2 is None or not pairs:
        return {
            "diff": None,
            "se_boot": None,
            "bound": None,
            "violated": True,
            "reason": "H1: insufficient non-dropped/paired arm-C phase-1/phase-2 tasks to evaluate control stability",
            "n_pairs": len(pairs),
        }

    diff = median_p2 - median_p1
    se_boot = statistics.pstdev(_bootstrap_diff_paired(rng, pairs))
    bound = HYGIENE_SE_MULTIPLIER * se_boot
    violated = abs(diff) > bound
    reason = None
    if violated:
        reason = (
            f"H1 control-stability violation: |median_C,p2 - median_C,p1| = {abs(diff)} "
            f"> 2*SE_boot = {bound}"
        )
    return {
        "diff": diff,
        "se_boot": se_boot,
        "bound": bound,
        "violated": violated,
        "reason": reason,
        "n_pairs": len(pairs),
    }


def _check_h2(
    rng: random.Random,
    view_c: dict[str, Any],
    view_m: dict[str, Any],
    manifest_tasks: list[dict[str, Any]],
) -> dict[str, Any]:
    """Design spec §4, H2 (first-exposure equivalence), quoted verbatim:
    "|median_M,p1 - median_C,p1| within 2 x SE_boot. Injection cannot fire
    on an empty store, so a phase-1 gap is instrument error -> INVALID."
    Cross-arm: both phase-1 cost lists resample independently (no pairing
    needed -- different arms, same phase)."""
    costs_c_p1 = list(view_c["costs"][1].values())
    costs_m_p1 = list(view_m["costs"][1].values())
    median_c = _median_or_none(costs_c_p1)
    median_m = _median_or_none(costs_m_p1)

    if median_c is None or median_m is None:
        return {
            "diff": None,
            "se_boot": None,
            "bound": None,
            "violated": True,
            "reason": "H2: insufficient non-dropped phase-1 tasks in one or both arms to evaluate first-exposure equivalence",
            "n_c_p1": len(costs_c_p1),
            "n_m_p1": len(costs_m_p1),
        }

    diff = median_m - median_c
    se_boot = statistics.pstdev(_bootstrap_diff_independent(rng, costs_m_p1, costs_c_p1))
    bound = HYGIENE_SE_MULTIPLIER * se_boot
    violated = abs(diff) > bound
    reason = None
    if violated:
        reason = (
            f"H2 first-exposure-equivalence violation: |median_M,p1 - median_C,p1| = {abs(diff)} "
            f"> 2*SE_boot = {bound}"
        )
    return {
        "diff": diff,
        "se_boot": se_boot,
        "bound": bound,
        "violated": violated,
        "reason": reason,
        "n_c_p1": len(costs_c_p1),
        "n_m_p1": len(costs_m_p1),
    }


def _check_h3(dropped_c: list[dict[str, Any]], dropped_m: list[dict[str, Any]], n: int) -> dict[str, Any]:
    """Design spec §4, H3, quoted verbatim: "infra rate <= 5% per arm
    (task-level: Error statuses, daemon faults, driver-detected protocol
    breaks ...). Above 5% -> infrastructure kill." The denominator is the
    FIXED manifest-derived task-half count (``n`` tasks x 2 phases), not
    however many ledger/journal rows happened to exist -- a rate independent
    of how badly a run degraded. Only entries flagged ``infra`` (task-4
    brief's exact two-clause definition -- driver-infra status OR missing
    MemoryStamp) count toward the numerator; the two other, pathological
    ``dropped`` reasons (no ledger row / missing task_id -- defensive-only,
    see ``recompute_join._measure_arm``) do not, since they are not what H3
    names."""
    task_halves = n * 2
    c_infra_count = sum(1 for entry in dropped_c if entry["infra"])
    m_infra_count = sum(1 for entry in dropped_m if entry["infra"])
    c_infra_rate = (c_infra_count / task_halves) if task_halves else None
    m_infra_rate = (m_infra_count / task_halves) if task_halves else None
    violated = (c_infra_rate is not None and c_infra_rate > INFRA_RATE_CEILING) or (
        m_infra_rate is not None and m_infra_rate > INFRA_RATE_CEILING
    )
    reason = None
    if violated:
        reason = (
            f"H3 infra-rate kill: C={c_infra_rate} M={m_infra_rate} > ceiling {INFRA_RATE_CEILING}"
        )
    return {
        "c_infra_count": c_infra_count,
        "c_task_halves": task_halves,
        "c_infra_rate": c_infra_rate,
        "m_infra_count": m_infra_count,
        "m_task_halves": task_halves,
        "m_infra_rate": m_infra_rate,
        "violated": violated,
        "reason": reason,
        "ceiling": INFRA_RATE_CEILING,
    }


def _check_e1(rng: random.Random, view_c: dict[str, Any], view_m: dict[str, Any]) -> dict[str, Any]:
    """Design spec §4, E1, quoted verbatim: "median_M,p2 <= median_C,p2 -
    Delta_min, where Delta_min = 2 x SE_boot(median_M,p2 - median_C,p2) ...
    Medians are computed over the non-dropped tasks." Plus the headroom
    clause, quoted verbatim: "if median_C,p2 - min_C,p2 < Delta_min the
    cost distribution is floor-saturated and the verdict is UNMEASURABLE,
    not FAIL." Only called when hygiene is clean (`recompute()`'s
    short-circuit) -- "no gate number was read" otherwise."""
    costs_c_p2 = list(view_c["costs"][2].values())
    costs_m_p2 = list(view_m["costs"][2].values())
    n_c_p2, n_m_p2 = len(costs_c_p2), len(costs_m_p2)
    median_c = _median_or_none(costs_c_p2)
    median_m = _median_or_none(costs_m_p2)
    min_c = min(costs_c_p2) if costs_c_p2 else None

    if median_c is None or median_m is None:
        return {
            "verdict": "UNMEASURABLE",
            "median_c_p2": median_c,
            "median_m_p2": median_m,
            "min_c_p2": min_c,
            "headroom": None,
            "delta_min": None,
            "se_boot": None,
            "n_c_p2": n_c_p2,
            "n_m_p2": n_m_p2,
            "reason": "E1 unmeasurable: no non-dropped phase-2 tasks in one or both arms",
        }

    se_boot = statistics.pstdev(_bootstrap_diff_independent(rng, costs_m_p2, costs_c_p2))
    delta_min = HYGIENE_SE_MULTIPLIER * se_boot
    headroom = median_c - min_c

    if headroom < delta_min:
        return {
            "verdict": "UNMEASURABLE",
            "median_c_p2": median_c,
            "median_m_p2": median_m,
            "min_c_p2": min_c,
            "headroom": headroom,
            "delta_min": delta_min,
            "se_boot": se_boot,
            "n_c_p2": n_c_p2,
            "n_m_p2": n_m_p2,
            "reason": (
                f"E1 unmeasurable: headroom clause (design spec §4) -- "
                f"median_C,p2 - min_C,p2 = {headroom} < delta_min = {delta_min}"
            ),
        }

    verdict = "PASS" if median_m <= median_c - delta_min else "FAIL"
    reason = None
    if verdict == "FAIL":
        reason = (
            f"E1 FAIL: median_M,p2 = {median_m} > median_C,p2 - delta_min = {median_c - delta_min}"
        )
    return {
        "verdict": verdict,
        "median_c_p2": median_c,
        "median_m_p2": median_m,
        "min_c_p2": min_c,
        "headroom": headroom,
        "delta_min": delta_min,
        "se_boot": se_boot,
        "n_c_p2": n_c_p2,
        "n_m_p2": n_m_p2,
        "reason": reason,
    }
