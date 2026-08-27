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

# Branch-review finding I-2 (controller-ruled as code): the treatment
# itself must be verifiable from the data, not assumed from the command
# line. Design spec §4 pins each arm's configuration -- "Arm C: `[memory]
# enabled = false`" / "Arm M: `[memory] enabled = true` ... Phase 1 mints;
# phase 2 retrieves and injects" -- and the daemon stamps the realized
# mode on EVERY spawned task (`registry.rs`'s `Event::MemoryStamp`, written
# "including tasks that ran with the organ off"). So a memory-off arm can
# only ever produce `mode: "off"` stamps, and a memory-on arm only
# `"silent"` (nothing retrieved) or `"injected"` (something was).
#
# ARM_LABEL_C/ARM_LABEL_M are the `arm` strings `driver.py` writes verbatim
# onto every ledger row from its `--arm` argument (`run_arm`'s `arm_name`);
# Tasks 6/7 must therefore invoke the driver as `--arm C` and `--arm M`.
ARM_LABEL_C = "C"
ARM_LABEL_M = "M"
ARM_C_ALLOWED_MODES = ("off",)
ARM_M_ALLOWED_MODES = ("silent", "injected")


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


def _check_arm_completeness(arm_label: str, actual_task_halves: int, n: int) -> dict[str, Any]:
    """Review finding C2: an arm-level guard, NOT named in design spec §4,
    added because H3's infra-rate ceiling alone is blind to a driver that
    dies partway through an arm and simply never appends any more ledger
    rows -- a 40%-complete arm has plenty of infra-flagged drops (good,
    fixed by C2's other half) but a 5%-style RATE ceiling can still miss a
    catastrophic but partial run in edge cases, and a reader of the gate's
    output deserves an explicit, named "this arm never finished" fact
    rather than inferring it from an infra-rate number alone. Every arm
    must carry exactly ``2 * n`` task-half ledger rows (one per task, per
    phase) -- anything else means the driver did not run to completion,
    and the run is INVALID regardless of what the infra rate says.

    Evaluated FIRST, before identity/H1/H2/H3 (`recompute.py`'s hygiene
    order): whether the run is even complete enough to trust its other
    numbers is logically prior to comparing digests or medians on it."""
    expected = 2 * n
    violated = actual_task_halves != expected
    reason = None
    if violated:
        reason = (
            f"{arm_label}: ledger carries {actual_task_halves} task-half row(s), expected "
            f"{expected} (2 x n={n}) -- truncated/incomplete arm, verdict INVALID regardless "
            f"of infra rate"
        )
    return {
        "expected_task_halves": expected,
        "actual_task_halves": actual_task_halves,
        "violated": violated,
        "reason": reason,
    }


def _check_treatment_identity(
    expected_arm_label: str,
    allowed_modes: tuple[str, ...],
    view: dict[str, Any],
    ledger_arm_labels: list[str],
) -> dict[str, Any]:
    """Branch-review finding I-2 (controller-ruled as code), NOT named in
    design spec §4: does the data in this slot actually come from the arm
    the slot claims? Two independent facts, either of which alone makes
    the run INVALID:

    (a) **Realized treatment mode.** Every JOINED ``MemoryStamp`` in the C
    slot must carry ``mode == "off"``; every one in the M slot must carry
    ``"silent"`` or ``"injected"``. Nothing else in the hygiene chain can
    see a C/M transposition or a mis-configured arm C (an arm C booted
    with ``[memory] enabled = true`` still produces perfectly well-formed
    journals, passes identity, H1, H2, H3 -- and silently INVERTS E1,
    since the "control" would then be the treated arm). A cheap
    configuration slip therefore has to be a named INVALID, not a number.

    (b) **Ledger arm label.** Every task-half ledger row in this slot must
    carry exactly ``expected_arm_label`` (``driver.py`` writes ``--arm``
    verbatim on every row). Ledgers passed in the wrong slots are the
    other half of the same transposition, and are otherwise indetectable
    because the join itself succeeds.

    Consumes NO RNG -- deliberately, so slotting this check into the fixed
    hygiene order (arm-completeness -> identity -> THIS -> H1 -> H2 -> H3)
    cannot disturb the bootstrap's pinned draw order."""
    offending_stamps = [
        {"task": name, "phase": phase, "mode": mode}
        for phase in (1, 2)
        for name, mode in sorted(view["modes"][phase].items())
        if mode not in allowed_modes
    ]
    observed_arm_labels = list(ledger_arm_labels)
    label_violated = observed_arm_labels != [expected_arm_label]

    parts = []
    if label_violated:
        parts.append(
            f"{expected_arm_label}: task-half ledger rows carry arm label(s) "
            f"{observed_arm_labels} -- expected exactly ['{expected_arm_label}'] "
            f"(arm slots transposed, or the driver was invoked with the wrong --arm)"
        )
    if offending_stamps:
        parts.append(
            f"{expected_arm_label}: {len(offending_stamps)} joined MemoryStamp row(s) carry a "
            f"mode outside {list(allowed_modes)} -- first {offending_stamps[0]} "
            f"(treatment identity: arm C must be memory-off, arm M memory-on -- design spec §4)"
        )
    return {
        "expected_arm_label": expected_arm_label,
        "observed_arm_labels": observed_arm_labels,
        "allowed_modes": list(allowed_modes),
        "offending_stamps": offending_stamps,
        "violated": bool(label_violated or offending_stamps),
        "reason": "; ".join(parts) if parts else None,
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
    of how badly a run degraded.

    Every ``dropped`` entry flagged ``infra: True`` counts toward the
    numerator (review finding C2: ``recompute_join._measure_arm`` flags
    ALL FIVE of its drop reasons -- including "no ledger row" and "no
    task_id", the exact shape a driver that dies mid-arm leaves behind --
    as infra; a truncated arm must show up here, not read as clean)."""
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
