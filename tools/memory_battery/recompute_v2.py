"""memory-battery recompute_v2 (design spec
`docs/superpowers/specs/2026-08-28-refalsify-battery-v2-design.md` §4/§5;
task-1 brief `.superpowers/sdd/2026-08-28-refalsify-battery-v2/task-1-brief.md`):
the analysis instrument for the refalsify-v2 cost-and-preservation battery.

**Two arms, honest labels.** `m_prime` (`[memory] refalsify = false`) and
`r` (`[memory] refalsify = true`) -- design spec §5: "battery-v1's `c_/m_`
slot names must not be reused for different semantics." No output key,
reason string, or fixture here ever spells an arm `C`/`M`; a ledger
carrying either label is REJECTED unconditionally by `_check_arm_labels`
regardless of `expected_arm_labels` (which exists only so the
dry-shakedown's `M_PRIME_DRY`/`R_DRY` ledgers parse).

**Reuse-mechanism choice: direct underscore imports, no
`recompute_shared.py`.** `_load_arm` (`recompute_join.py`), the ledger/
journal row readers (`recompute_journal.py`), and the bootstrap/rate
primitives + `_check_identity`/`_check_arm_completeness`
(`recompute_bootstrap.py`) are imported straight from their modules --
exactly how `recompute.py` itself already imports across these same
boundaries. Leading-underscore names are a *convention*, not an import
barrier; nothing here required editing a v1 file, so the
`recompute_shared.py` escape hatch was not needed. NONE of `recompute.py`/
`recompute_join.py`/`recompute_journal.py`/`recompute_bootstrap.py`/
`driver.py`/`corpus.py` is edited by this module.

**Deliberately NOT reused (label-honesty, spec §5):** `_check_h1` (no v2
analogue -- both arms carry the treatment-relevant store); `_check_h2`/
`_check_h3`/`_check_e1`/`_check_treatment_identity`/`_paired_deltas_m` all
bake literal `"C"`/`"M"` (or `c_`/`m_`-prefixed keys) into their reason
strings or return shape. Their FORMULAS are the same shape, so this module
re-implements the thin presentation layer locally while calling the
shared, label-agnostic PURE MATH primitives (`_bootstrap_diff_independent`,
`HYGIENE_SE_MULTIPLIER`, `INFRA_RATE_CEILING`). `_check_identity` and
`_check_arm_completeness` ARE reused directly -- both take `arm_label` as
a caller-supplied parameter rather than a hardcoded `"C"`/`"M"`, so passing
`"m_prime"`/`"r"` produces honestly-labeled output for free.

**No superiority/inferiority endpoint here.** G1 is an EQUIVALENCE gate
(`|diff| <= band`), not E1's one-sided `median_M <= median_C - delta_min`
-- the task-1 brief is explicit that "E1 is v1's, not copied." This module
owns its own `_median_or_none` (a local one-liner, not imported) so
mutation check #1 ("median -> mean in G1") is a surgical, single-line edit
here, never touching the shared v1 helper `recompute.py`'s own H1/H2/E1
still depend on.

**G1's floor-saturation clause is a judgment call.** Design spec §4's
kill-criteria states the floor-saturation PRINCIPLE without restating v1
E1's one-sided `headroom = median_C,p2 - min_C,p2 < delta_min` formula,
because E1's formula is directional (checks only the reference arm) and
G1 is a SYMMETRIC equivalence test with no reference arm. Resolved by
generalizing headroom to BOTH arms: `headroom_x = median_x,p2 - min_x,p2`
for x in {m_prime, r}; if EITHER is under the band, the verdict is
UNMEASURABLE rather than a PASS granted by compression instead of earned
by resolution.

**RNG order**: one seeded `random.Random(seed)` instance, created fresh
inside `recompute_v2()`. H2 first, G1 second (design spec §4: "Hygiene ...
computed before any gate is read"). Nothing else touches it -- A1 is
advisory arithmetic only, no SE/band.

**CLI enforces, library stays permissive** (review findings CRITICAL +
IMPORTANT-1, v1's I3 precedent): `recompute_v2()` always COMPUTES
`completeness` and `lens["identity"]` but never raises or forces a verdict
on their violation -- `main()`'s `_cli_fatal_checks` is the layer that
turns either violation into a hard, pre-JSON, nonzero-exit failure.

Python 3 stdlib only; no GPU access, no clock reads -- every number
derives from journal/ledger/manifest bytes already on disk.
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

# Design spec §6/§4 (locked numbers, task-1 brief): seed 20260828, B=10,000
# -- DELIBERATELY DIFFERENT from `recompute_bootstrap.BOOTSTRAP_SEED`
# (20260826, v1's own lock). Module-level constants so mutation check #5
# ("seed drifts -- any literal") has one unambiguous line to mutate.
SEED_V2 = 20260828
B_V2 = 10_000

# Design spec §5: honest v2 arm labels -- never v1's "C"/"M".
ARM_LABEL_M_PRIME = "m_prime"
ARM_LABEL_R = "r"

# v1's arm labels, unconditionally forbidden in v2's ledgers regardless of
# what `expected_arm_labels` a caller passes (see `_check_arm_labels`).
FORBIDDEN_ARM_LABELS = frozenset({"C", "M"})

# Refalsify spellings design spec §4 names by name: the two LIVE v2
# spellings ("premise_held", "premise_gone"), the two named-zero/tolerated
# ones ("inconclusive", "skipped_ungranted"), and the two RETIRED v1
# spellings that must appear nowhere under a v2 build ("passed", "failed").
FORBIDDEN_REFALSIFY_SPELLINGS = frozenset({"passed", "failed"})
NO_PROBE_SPELLING_KEY = "none"  # JSON-safe stand-in for a `None` refalsify.

# Design spec §4, stamp audit, quoted verbatim: "inconclusive (probe
# timeout/spawn) and skipped_ungranted expected 0; tolerated within H3's
# infra budget, counted and named individually." Review finding
# IMPORTANT-2: these two spellings are TOLERATED on an injected stamp --
# counted, but never themselves an `premise_held_complete` violation (only
# a genuinely offending spelling -- premise_gone, a forbidden v1 spelling,
# or an unexpected None -- counts as offending).
TOLERATED_NON_PREMISE_HELD_SPELLINGS = frozenset({"inconclusive", "skipped_ungranted"})


def _median_or_none(values: Sequence[float]) -> float | None:
    """Local, NOT imported from `recompute_bootstrap.py` -- see module
    docstring: this keeps mutation check #1 ("median -> mean in G1")
    surgical to this file."""
    return statistics.median(values) if values else None


# Per-task detail join (refalsify spelling + wall_ms), name-keyed.


def _arm_task_details(
    arm_result: dict[str, Any], ledger_path: Path, manifest_tasks: list[dict[str, Any]]
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
    task-half id, the WRONG field)."""
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
                "wall_ms": step_walls.get(agent_id, 0) if agent_id else 0,
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
    formula itself must count probes, not injections)."""
    wall_m_prime_p1 = [info["wall_ms"] for info in details_m_prime[1].values()]
    wall_r_p1 = [info["wall_ms"] for info in details_r[1].values()]
    wall_m_prime_p2 = [info["wall_ms"] for info in details_m_prime[2].values()]
    wall_r_p2 = [info["wall_ms"] for info in details_r[2].values()]

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
    }


# recompute_v2 -- the pinned entry point (task-1 brief).


def recompute_v2(
    corpus_dir: str | Path,
    arm_m_prime_dir: str | Path,
    arm_r_dir: str | Path,
    ledger_m_prime: str | Path,
    ledger_r: str | Path,
    *,
    expected_digest: str | None = None,
    seed: int = SEED_V2,
    b: int = B_V2,
    expected_arm_labels: tuple[str, str] = (ARM_LABEL_M_PRIME, ARM_LABEL_R),
) -> dict[str, Any]:
    """Task-1 brief's pinned entry point. Output dict keys: the brief's own
    exact list (`g1`, `g2`, `stamp_audit`, `a1_wall`, `h2_p1_equivalence`,
    `h3_infra`, `h4_advisory`, `dropped`, `corpus_sha`, `lens`) PLUS
    `completeness` (review finding IMPORTANT-1 -- v2 analogue of v1's
    `_check_arm_completeness`/C2; additive since the brief didn't name it,
    but without it a truncated arm has no explicit "never finished" fact,
    only an inference from `h3_infra`'s rate).

    Every value in the return is a plain JSON-native type -- round-trips
    through `json.dumps`/`json.loads` unchanged, same invariant
    `recompute.py`'s own entry point holds itself to.

    **The library stays permissive** (review finding CRITICAL, v1's I3
    precedent): `completeness`/`lens["identity"]` are always computed and
    returned, never enforced here (no exception, no verdict forced to
    INVALID) -- enforcement is `main()`'s job, mirroring v1's CLI/library
    split (`_cli_fatal_checks`, below)."""
    corpus_dir = Path(corpus_dir)
    manifest = json.loads((corpus_dir / "manifest.json").read_text(encoding="utf-8"))
    manifest_tasks = manifest["tasks"]
    n = len(manifest_tasks)

    arm_m_prime = _load_arm(Path(arm_m_prime_dir), Path(ledger_m_prime), manifest_tasks)
    arm_r = _load_arm(Path(arm_r_dir), Path(ledger_r), manifest_tasks)

    # Review finding IMPORTANT-1 (v1 C2 port): checked first -- whether a
    # run is even complete enough to trust is logically prior to anything
    # else. Label-agnostic (`arm_label` is caller-supplied text), same
    # reuse precedent as `_check_identity` above.
    completeness_m_prime = _check_arm_completeness(ARM_LABEL_M_PRIME, arm_m_prime["ledger_task_half_count"], n)
    completeness_r = _check_arm_completeness(ARM_LABEL_R, arm_r["ledger_task_half_count"], n)

    _check_arm_labels(arm_m_prime["ledger_arm_labels"], arm_r["ledger_arm_labels"], expected_arm_labels)

    details_m_prime = _arm_task_details(arm_m_prime, Path(ledger_m_prime), manifest_tasks)
    details_r = _arm_task_details(arm_r, Path(ledger_r), manifest_tasks)

    # Fixed RNG consumption order (module docstring): H2 first, G1 second.
    rng = random.Random(seed)
    h2 = _h2_p1_equivalence(rng, arm_m_prime["view"], arm_r["view"], b=b)
    g1 = _g1_token_preservation(rng, arm_m_prime["view"], arm_r["view"], b=b)

    g2 = _g2_injection_preservation(arm_m_prime["view"], arm_r["view"])
    stamp_audit = _stamp_audit(details_m_prime, details_r, arm_m_prime["view"], arm_r["view"])
    a1_wall = _a1_wall(details_m_prime, details_r)
    h3 = _h3_infra(arm_m_prime["dropped"], arm_r["dropped"], n)
    h4 = _h4_advisory(
        arm_m_prime["view"],
        arm_r["view"],
        arm_m_prime["tasks_journal_rows"],
        arm_m_prime["task_id_to_phase"],
        arm_r["tasks_journal_rows"],
        arm_r["task_id_to_phase"],
        n,
    )

    corpus_sha = _corpus_sha(manifest)

    identity_m_prime = _check_identity(ARM_LABEL_M_PRIME, arm_m_prime["identity_by_phase"], expected_digest)
    identity_r = _check_identity(ARM_LABEL_R, arm_r["identity_by_phase"], expected_digest)

    lens = {
        "seed": seed,
        "b": b,
        "n": n,
        "arm_labels": {"m_prime": expected_arm_labels[0], "r": expected_arm_labels[1]},
        "source_paths": {
            "corpus_dir": str(corpus_dir),
            "arm_m_prime_dir": str(arm_m_prime_dir),
            "arm_r_dir": str(arm_r_dir),
            "ledger_m_prime": str(ledger_m_prime),
            "ledger_r": str(ledger_r),
        },
        "expected_digest": expected_digest,
        "digest_m_prime": {
            "phase1": arm_m_prime["identity_by_phase"].get(1),
            "phase2": arm_m_prime["identity_by_phase"].get(2),
        },
        "digest_r": {
            "phase1": arm_r["identity_by_phase"].get(1),
            "phase2": arm_r["identity_by_phase"].get(2),
        },
        "identity": {
            "m_prime": identity_m_prime,
            "r": identity_r,
            "violated": identity_m_prime["violated"] or identity_r["violated"],
        },
    }

    return {
        "g1": g1,
        "g2": g2,
        "stamp_audit": stamp_audit,
        "a1_wall": a1_wall,
        "h2_p1_equivalence": h2,
        "h3_infra": h3,
        "h4_advisory": h4,
        "completeness": {
            "m_prime": completeness_m_prime,
            "r": completeness_r,
            "violated": completeness_m_prime["violated"] or completeness_r["violated"],
        },
        "dropped": {"m_prime": arm_m_prime["dropped"], "r": arm_r["dropped"]},
        "corpus_sha": corpus_sha,
        "lens": lens,
    }


def _cli_fatal_checks(result: dict[str, Any], expected_digest: str) -> list[str]:
    """Review findings CRITICAL + IMPORTANT-1: CLI-layer enforcement only
    (library stays permissive) -- a real gate run must never silently pass
    a truncated arm or a served-identity mismatch (v1 I3 CLI/library
    split; this module's own `_check_arm_labels` hard-fail precedent).
    Checked in v1's priority order: completeness first (a truncated arm's
    numbers aren't trustworthy enough to check identity on), then per-arm
    identity -- both arms checked for both, since either arm's violation
    invalidates the whole run (v1 prereg §6: "either arm's mismatch makes
    the whole run INVALID"). Returns FATAL message(s) for stderr; empty =
    nothing to enforce."""
    fatals: list[str] = []

    completeness = result["completeness"]
    for arm_label in (ARM_LABEL_M_PRIME, ARM_LABEL_R):
        arm_completeness = completeness[arm_label]
        if arm_completeness["violated"]:
            fatals.append(
                f"memory_battery.recompute_v2: FATAL: arm {arm_label!r} is incomplete -- "
                f"{arm_completeness['actual_task_halves']} task-half row(s), expected "
                f"{arm_completeness['expected_task_halves']} -- {arm_completeness['reason']}"
            )

    identity = result["lens"]["identity"]
    for arm_label in (ARM_LABEL_M_PRIME, ARM_LABEL_R):
        arm_identity = identity[arm_label]
        if arm_identity["violated"]:
            fatals.append(
                f"memory_battery.recompute_v2: FATAL: identity mismatch on arm {arm_label!r} -- "
                f"phase1_digest={arm_identity['phase1_digest']!r} "
                f"phase2_digest={arm_identity['phase2_digest']!r} expected={expected_digest!r}"
            )

    return fatals


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description=(
            "memory-battery recompute_v2 (design spec §4/§5; task-1 brief): derives every "
            "G1/G2/stamp-audit/A1/H2-H4 number from journal bytes and the frozen manifest for "
            "the refalsify-battery-v2 m_prime/r arms, prints the pinned JSON schema."
        )
    )
    parser.add_argument("--corpus-dir", type=Path, required=True, help="Frozen corpus directory (manifest.json).")
    parser.add_argument("--arm-m-prime-dir", type=Path, required=True, help="Arm M' (refalsify=false)'s data_dir.")
    parser.add_argument("--arm-r-dir", type=Path, required=True, help="Arm R (refalsify=true)'s data_dir.")
    parser.add_argument("--ledger-m-prime", type=Path, required=True, help="Arm M's driver ledger JSONL path.")
    parser.add_argument("--ledger-r", type=Path, required=True, help="Arm R's driver ledger JSONL path.")
    parser.add_argument(
        "--expected-digest",
        required=True,
        help=(
            "REQUIRED: the prereg-pinned served-identity digest (v1 prereg §6 carry-note "
            "behavior). Checked against both arms' ledger identity rows; the library "
            "`recompute_v2()` kwarg stays optional-None so fixtures/tests that don't care about "
            "identity can omit it, matching v1's `recompute()` CLI/library split."
        ),
    )
    parser.add_argument(
        "--expected-arm-labels",
        nargs=2,
        metavar=("M_PRIME_LABEL", "R_LABEL"),
        default=(ARM_LABEL_M_PRIME, ARM_LABEL_R),
        help=(
            "Two literal ledger `--arm` labels this run's ledgers must carry, in "
            "(m_prime, r) order. Defaults to the real-run labels ('m_prime', 'r') -- a real "
            "gate invocation never needs this flag. Task-2 brief's dry shakedown drives the "
            "daemon with `--arm M_PRIME_DRY`/`--arm R_DRY` (so a DRY ledger can never be "
            "mistaken for a real one at a glance); this flag is what lets the CLI check "
            "against those DRY labels instead of the default -- the library `recompute_v2()` "
            "kwarg of the same name already supported this (see "
            "`test_dry_shakedown_labels_parse_via_expected_arm_labels`); this CLI plumbing was "
            "the missing wire-up. `_check_arm_labels`'s v1-label rejection (FORBIDDEN_ARM_LABELS) "
            "applies unconditionally regardless of what is passed here."
        ),
    )
    args = parser.parse_args(argv)

    try:
        result = recompute_v2(
            args.corpus_dir,
            args.arm_m_prime_dir,
            args.arm_r_dir,
            args.ledger_m_prime,
            args.ledger_r,
            expected_digest=args.expected_digest,
            expected_arm_labels=tuple(args.expected_arm_labels),
        )
    except Exception as exc:  # noqa: BLE001 -- last-resort net, house pattern (recompute.py/driver.py)
        print(f"memory_battery.recompute_v2: FATAL: {exc!r}", file=sys.stderr)
        return 1

    # Review findings CRITICAL + IMPORTANT-1: enforced HERE, at the CLI
    # layer, BEFORE any JSON is printed -- a real gate run must never
    # silently pass a truncated arm or a served-identity mismatch.
    fatals = _cli_fatal_checks(result, args.expected_digest)
    if fatals:
        for message in fatals:
            print(message, file=sys.stderr)
        return 1

    print(json.dumps(result, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
