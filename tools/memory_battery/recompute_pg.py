"""premise-gone-battery recompute (design spec
`docs/superpowers/specs/2026-08-28-premise-gone-battery-v1-design.md` §5;
plan Task 6): the analysis instrument for the goal-satisfied-repeat lane.

**Arms**: `m_prime` (`[memory] refalsify = false` — injects the moot
lesson on every matched retrieval) and `r` (`refalsify = true` — probes
every matched retrieval; the premise_gone lane under test). Label
honesty, reuse discipline, and the CLI/library split are all
`recompute_v2.py`'s, cited not restated: shared label-agnostic math and
row readers are imported; label-baked or semantics-baked presentation
layers are re-implemented thinly here.

**Two evidence sources beyond the journals** (spec §2's named addition):
each arm's `memory/episodes.jsonl` store file — full-record rows,
last-writer-wins by `episode_id` (`crates/bloomery-daemon/src/memory/
store.rs`'s own replay rule) — read for PG2/A2's final episode statuses;
and `Degraded` journal rows scanned for the oversize-skip marker
(`registry.rs`'s pinned "injection bound" reason substring), because an
oversize skip stamps silent/None in M′ and would otherwise be
indistinguishable from a retrieval miss (spec §4's matched-set note).

**Matched sets** (spec §4, verbatim semantics): in R-p2, stamps whose
`refalsify` is non-None (the spelling field, not `episode_id`, marks the
match — a `premise_gone` stamp carries `episode_id: None` by design); in
M′-p2, `mode: "injected"` stamps plus the required zero-oversize scan.

**PG1's spelling diagnoses** (spec §5): `premise_held` → ALARM (a
phase-2 workspace failed to materialize goal-satisfied — investigate
before reading any gate); `skipped_ungranted` → INVALID (the corpus's
own grant failed to cover its own `run_argv` — instrument
misconfiguration); `inconclusive` → probe infrastructure, the task is
excluded from PG1's totality accounting and counted toward H3's infra
budget, never scored. A `mode:"injected"` R-p2 stamp outside those
diagnosed spellings FAILS PG1 outright.

Python 3 stdlib only; every number derives from journal/ledger/store/
manifest bytes already on disk. None-vs-zero throughout.
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
from tools.memory_battery.recompute_journal import _read_jsonl
from tools.memory_battery.recompute_v2 import (
    ARM_LABEL_M_PRIME,
    ARM_LABEL_R,
    FORBIDDEN_REFALSIFY_SPELLINGS,
    NO_PROBE_SPELLING_KEY,
    _a1_wall,
    _arm_task_details,
    _check_arm_labels,
    _h2_p1_equivalence,
)

# Design spec §5 (locked numbers): bootstrap seed 20260829 — deliberately
# distinct from every prior lock (corpus-v1's 20260826, this corpus's
# 20260828, battery-v2's bootstrap 20260828). Module-level constants so
# the seed-drift mutation check has one unambiguous line.
SEED_PG = 20260829
B_PG = 10_000

# Spec §5's matched-count floor — 25 = n/2, a CHOSEN threshold, flagged
# [judgment] in the spec: below it the corpus's cited-set construction
# premise failed at scale and the verdict is UNMEASURABLE, never FAIL.
MATCHED_FLOOR_PG = 25

# registry.rs's oversize degrade reason (pinned substring): "... over the
# {MEMORY_BLOCK_MAX_BYTES}-byte injection bound; task ... runs memory-off".
OVERSIZE_REASON_MARKER = "injection bound"


def _median_or_none(values: Sequence[float]) -> float | None:
    return statistics.median(values) if values else None


def _p2_spellings(details: dict[int, dict[str, dict[str, Any]]]) -> dict[str, list[str]]:
    """Phase-2 task names grouped by refalsify spelling (None under the
    JSON-safe `NO_PROBE_SPELLING_KEY`), names sorted for determinism."""
    groups: dict[str, list[str]] = {}
    for name, detail in details[2].items():
        key = detail["refalsify"] if detail["refalsify"] is not None else NO_PROBE_SPELLING_KEY
        groups.setdefault(key, []).append(name)
    return {key: sorted(names) for key, names in groups.items()}


def _pg1_premise_gone_totality(
    details_r: dict[int, dict[str, dict[str, Any]]], view_r: dict[str, Any]
) -> dict[str, Any]:
    """Gate PG1 (spec §5): over R-p2's non-dropped tasks, zero
    non-infra injections and every matched stamp `premise_gone` +
    `mode:"silent"` — with each breaking spelling diagnosed, not
    blended (see module docstring)."""
    spellings = _p2_spellings(details_r)
    premise_gone_names = spellings.get("premise_gone", [])
    premise_held_names = spellings.get("premise_held", [])
    inconclusive_names = spellings.get("inconclusive", [])
    skipped_names = spellings.get("skipped_ungranted", [])

    modes = view_r["modes"][2]
    injected_names = sorted(name for name, mode in modes.items() if mode == "injected")
    hard_injected_names = [name for name in injected_names if name not in inconclusive_names]
    gone_not_silent = sorted(
        name for name in premise_gone_names if modes.get(name) != "silent"
    )

    base = {
        "premise_gone_count": len(premise_gone_names),
        "premise_held_names": premise_held_names,
        "inconclusive_names": inconclusive_names,
        "skipped_ungranted_names": skipped_names,
        "injected_names_r_p2": injected_names,
        "premise_gone_not_silent_names": gone_not_silent,
    }

    if skipped_names:
        verdict = "INVALID"
        reason = (
            f"PG1 INVALID: skipped_ungranted on {skipped_names} -- the corpus's own grant "
            f"failed to cover its own run_argv; instrument misconfiguration"
        )
    elif premise_held_names:
        verdict = "ALARM"
        reason = (
            f"PG1 ALARM: premise_held on {premise_held_names} -- a phase-2 workspace failed "
            f"to materialize goal-satisfied; investigate before reading any gate"
        )
    elif hard_injected_names or gone_not_silent:
        verdict = "FAIL"
        reason = (
            f"PG1 FAIL: hard-injected R-p2 tasks {hard_injected_names}; premise_gone-but-not-"
            f"silent tasks {gone_not_silent}"
        )
    else:
        verdict, reason = "PASS", None
    return {**base, "verdict": verdict, "reason": reason}


def _final_episode_statuses(store_path: Path) -> dict[str, str]:
    """`episode_id` -> final `status`, replaying the store file's rows in
    order with last-writer-wins per id — `MemoryStore::load`'s own rule.
    A row without both keys is counted nowhere (the daemon's own reader
    skips-and-counts corrupt lines; this reader has no /status surface,
    so it simply excludes them from the map — PG2 reads what replays)."""
    statuses: dict[str, str] = {}
    for row in _read_jsonl(store_path):
        episode_id = row.get("episode_id")
        status = row.get("status")
        if isinstance(episode_id, str) and isinstance(status, str):
            statuses[episode_id] = status
    return statuses


def _pg2_store_preservation(
    store_statuses_r: dict[str, str], journal_rows_r: list[dict[str, Any]]
) -> dict[str, Any]:
    """Gate PG2 (spec §5): zero `MemoryContradicted` events in arm R's
    entire journal AND every episode's final store status `verified` —
    the live check of v2's "no probe ever contradicts" plus "nothing
    injected → §5 cannot fire" entailment."""
    contradicted_count = sum(1 for row in journal_rows_r if row.get("event") == "MemoryContradicted")
    non_verified = sorted(
        episode_id for episode_id, status in store_statuses_r.items() if status != "verified"
    )
    ok = contradicted_count == 0 and not non_verified
    return {
        "memory_contradicted_count": contradicted_count,
        "episode_count": len(store_statuses_r),
        "non_verified_episode_ids": non_verified,
        "verdict": "PASS" if ok else "FAIL",
        "reason": None
        if ok
        else (
            f"PG2 FAIL: {contradicted_count} MemoryContradicted event(s); non-verified "
            f"episodes {non_verified}"
        ),
    }


def _pg3_moot_lesson_injection(
    view_m_prime: dict[str, Any], journal_rows_m_prime: list[dict[str, Any]], floor: int
) -> dict[str, Any]:
    """Gate PG3 (spec §5): every matched M′-p2 retrieval injects —
    `injected_M′,p2 ≥ floor` with zero oversize `Degraded` rows (an
    oversize skip stamps silent/None and would hide a match)."""
    injected = sum(1 for mode in view_m_prime["modes"][2].values() if mode == "injected")
    oversize = sum(
        1
        for row in journal_rows_m_prime
        if row.get("event") == "Degraded" and OVERSIZE_REASON_MARKER in str(row.get("reason", ""))
    )
    if oversize:
        verdict = "ALARM"
        reason = (
            f"PG3 ALARM: {oversize} oversize Degraded row(s) -- a matched retrieval was "
            f"silently skipped; the matched set is not what the modes say"
        )
    elif injected < floor:
        verdict = "UNMEASURABLE"
        reason = f"PG3 UNMEASURABLE: injected_M'={injected} < floor={floor}"
    else:
        verdict, reason = "PASS", None
    return {
        "injected_count_m_prime_p2": injected,
        "oversize_degraded_count": oversize,
        "floor": floor,
        "verdict": verdict,
        "reason": reason,
    }


def _floor_check(matched_r: int, injected_m_prime: int, floor: int) -> dict[str, Any]:
    """Spec §5's matched-count floor, both arms — UNMEASURABLE below it,
    never FAIL (the construction failed, not the mechanism)."""
    ok = matched_r >= floor and injected_m_prime >= floor
    return {
        "floor": floor,
        "matched_r_p2": matched_r,
        "injected_m_prime_p2": injected_m_prime,
        "verdict": "PASS" if ok else "UNMEASURABLE",
        "reason": None
        if ok
        else (
            f"floor UNMEASURABLE: matched_R={matched_r}, injected_M'={injected_m_prime}, both "
            f"must be >= {floor} -- the cited-set construction premise failed at scale; "
            f"redesign the corpus, do not read a verdict"
        ),
    }


def _stamp_audit_pg(
    details_m_prime: dict[int, dict[str, dict[str, Any]]],
    details_r: dict[int, dict[str, dict[str, Any]]],
) -> dict[str, Any]:
    """Spec §5's stamp audit: retired spellings nowhere; `premise_held`
    zero (PG1 diagnoses it; this audit corroborates); every M′ stamp and
    every R-p1 stamp `refalsify: None` (flag-off and empty-store truth)."""

    def _counts(details: dict[int, dict[str, dict[str, Any]]]) -> dict[str, dict[str, int]]:
        out: dict[str, dict[str, int]] = {}
        for phase in (1, 2):
            phase_counts: dict[str, int] = {}
            for detail in details[phase].values():
                key = detail["refalsify"] if detail["refalsify"] is not None else NO_PROBE_SPELLING_KEY
                phase_counts[key] = phase_counts.get(key, 0) + 1
            out[str(phase)] = dict(sorted(phase_counts.items()))
        return out

    counts = {"m_prime": _counts(details_m_prime), "r": _counts(details_r)}
    forbidden_hits = [
        (arm, phase, spelling)
        for arm, arm_counts in counts.items()
        for phase, phase_counts in arm_counts.items()
        for spelling in phase_counts
        if spelling in FORBIDDEN_REFALSIFY_SPELLINGS
    ]
    m_prime_all_none = all(
        detail["refalsify"] is None for phase in (1, 2) for detail in details_m_prime[phase].values()
    )
    r_p1_all_none = all(detail["refalsify"] is None for detail in details_r[1].values())
    premise_held_total = sum(
        counts[arm][phase].get("premise_held", 0) for arm in counts for phase in counts[arm]
    )
    return {
        "counts": counts,
        "forbidden_spelling_hits": [list(hit) for hit in forbidden_hits],
        "forbidden_spellings_absent": not forbidden_hits,
        "m_prime_refalsify_all_none": m_prime_all_none,
        "r_p1_refalsify_all_none": r_p1_all_none,
        "premise_held_total": premise_held_total,
    }


def _h3_pg(
    dropped_m_prime: list[dict[str, Any]],
    dropped_r: list[dict[str, Any]],
    inconclusive_m_prime: int,
    inconclusive_r: int,
    n: int,
) -> dict[str, Any]:
    """H3 (spec §5): infra ≤ 5% per arm — `dropped` task-halves plus
    `inconclusive`-stamped tasks (probe infrastructure, excluded from
    PG1's accounting, counted here instead). Local thin layer over the
    shared ceiling constant, the v2 label-honesty precedent."""
    task_halves = 2 * n
    m_count = len(dropped_m_prime) + inconclusive_m_prime
    r_count = len(dropped_r) + inconclusive_r
    m_rate = (m_count / task_halves) if task_halves else None
    r_rate = (r_count / task_halves) if task_halves else None
    violated = any(rate is not None and rate > INFRA_RATE_CEILING for rate in (m_rate, r_rate))
    return {
        "ceiling": INFRA_RATE_CEILING,
        "m_prime_infra_count": m_count,
        "m_prime_infra_rate": m_rate,
        "m_prime_task_halves": task_halves,
        "r_infra_count": r_count,
        "r_infra_rate": r_rate,
        "r_task_halves": task_halves,
        "violated": violated,
    }


def _h4_pg(
    matched_r: int,
    injected_m_prime: int,
    journal_rows_m_prime: list[dict[str, Any]],
    task_id_to_phase_m_prime: dict[Any, Any],
    journal_rows_r: list[dict[str, Any]],
    task_id_to_phase_r: dict[Any, Any],
    n: int,
) -> dict[str, Any]:
    """H4 (advisory, spec §5): p1 mint rates, p2 matched rates, and the
    cross-arm matched gap (arms mint independently — a large gap is
    phase-1 behavioral divergence, named, never gated)."""

    def _mint_count_p1(rows: list[dict[str, Any]], task_id_to_phase: dict[Any, Any]) -> int:
        return sum(
            1
            for row in rows
            if row.get("event") == "MemoryMint" and task_id_to_phase.get(row.get("task_id")) == 1
        )

    mint_m = _mint_count_p1(journal_rows_m_prime, task_id_to_phase_m_prime)
    mint_r = _mint_count_p1(journal_rows_r, task_id_to_phase_r)
    return {
        "m_prime": {
            "mint_count_p1": mint_m,
            "mint_rate_p1": mint_m / n if n else None,
            "matched_count_p2": injected_m_prime,
            "matched_rate_p2": injected_m_prime / n if n else None,
            "n": n,
        },
        "r": {
            "mint_count_p1": mint_r,
            "mint_rate_p1": mint_r / n if n else None,
            "matched_count_p2": matched_r,
            "matched_rate_p2": matched_r / n if n else None,
            "n": n,
        },
        "cross_arm_matched_gap": abs(matched_r - injected_m_prime),
    }


def _a1_tokens(
    rng: random.Random, view_m_prime: dict[str, Any], view_r: dict[str, Any], *, b: int
) -> dict[str, Any]:
    """A1 (advisory, never gates, no capability sentence — spec §5): p2
    token medians, delta, and the same `2 × SE_boot` band shape as H2 —
    reported for the staleness-benefit story's FUTURE registration. No
    verdict field, deliberately: an advisory block that carried one
    would read as a gate."""
    costs_m = list(view_m_prime["costs"][2].values())
    costs_r = list(view_r["costs"][2].values())
    median_m = _median_or_none(costs_m)
    median_r = _median_or_none(costs_r)
    if median_m is None or median_r is None:
        return {
            "median_m_prime_p2": median_m,
            "median_r_p2": median_r,
            "diff": None,
            "se_boot": None,
            "band": None,
            "n_m_prime_p2": len(costs_m),
            "n_r_p2": len(costs_r),
        }
    se_boot = statistics.pstdev(_bootstrap_diff_independent(rng, costs_r, costs_m, b=b))
    return {
        "median_m_prime_p2": median_m,
        "median_r_p2": median_r,
        "diff": median_r - median_m,
        "se_boot": se_boot,
        "band": HYGIENE_SE_MULTIPLIER * se_boot,
        "n_m_prime_p2": len(costs_m),
        "n_r_p2": len(costs_r),
    }


def _a2_aftermath(
    journal_rows_m_prime: list[dict[str, Any]],
    task_id_to_phase_m_prime: dict[Any, Any],
    store_statuses_m_prime: dict[str, str],
    journal_rows_r: list[dict[str, Any]],
    task_id_to_phase_r: dict[Any, Any],
    store_statuses_r: dict[str, str],
    ledger_m_prime: Path,
    ledger_r: Path,
) -> dict[str, Any]:
    """A2 (advisory, spec §5): the M′ aftermath the future design-§5
    registration will want — §5 poisonings, phase-2 re-mints (a p2 mint
    implies a landed patch + verifying run, the mint bar's own exact
    signal), final store statuses, and terminal-status distributions.
    Observed and reported; never gated, never quoted as capability."""

    def _arm_block(
        rows: list[dict[str, Any]],
        task_id_to_phase: dict[Any, Any],
        store_statuses: dict[str, str],
        ledger_path: Path,
    ) -> dict[str, Any]:
        contradicted = sum(1 for row in rows if row.get("event") == "MemoryContradicted")
        mint_p2 = sum(
            1
            for row in rows
            if row.get("event") == "MemoryMint" and task_id_to_phase.get(row.get("task_id")) == 2
        )
        status_counts: dict[str, dict[str, int]] = {"1": {}, "2": {}}
        for row in _read_jsonl(ledger_path):
            if row.get("event") == "identity":
                continue
            phase = str(row.get("phase"))
            if phase in status_counts:
                status = str(row.get("status"))
                status_counts[phase][status] = status_counts[phase].get(status, 0) + 1
        store_counts: dict[str, int] = {}
        for status in store_statuses.values():
            store_counts[status] = store_counts.get(status, 0) + 1
        return {
            "memory_contradicted_count": contradicted,
            "mint_count_p2": mint_p2,
            "terminal_status_counts": {k: dict(sorted(v.items())) for k, v in status_counts.items()},
            "final_episode_status_counts": dict(sorted(store_counts.items())),
        }

    return {
        "m_prime": _arm_block(
            journal_rows_m_prime, task_id_to_phase_m_prime, store_statuses_m_prime, ledger_m_prime
        ),
        "r": _arm_block(journal_rows_r, task_id_to_phase_r, store_statuses_r, ledger_r),
    }


def recompute_pg(
    corpus_dir: str | Path,
    arm_m_prime_dir: str | Path,
    arm_r_dir: str | Path,
    ledger_m_prime: str | Path,
    ledger_r: str | Path,
    *,
    expected_digest: str | None = None,
    seed: int = SEED_PG,
    b: int = B_PG,
    floor: int = MATCHED_FLOOR_PG,
    expected_arm_labels: tuple[str, str] = (ARM_LABEL_M_PRIME, ARM_LABEL_R),
) -> dict[str, Any]:
    """The pg battery's pinned entry point. Output keys: `pg1`, `pg2`,
    `pg3`, `floor`, `stamp_audit`, `a1_tokens`, `a2_aftermath`,
    `a3_wall`, `h2_p1_equivalence`, `h3_infra`, `h4_advisory`,
    `completeness`, `dropped`, `corpus_sha`, `lens`. JSON-native
    throughout; the library stays permissive (completeness/identity
    computed, never enforced — `main()`'s `_cli_fatal_checks` is the
    enforcement layer, the v2 CLI/library split verbatim)."""
    corpus_dir = Path(corpus_dir)
    manifest = json.loads((corpus_dir / "manifest.json").read_text(encoding="utf-8"))
    manifest_tasks = manifest["tasks"]
    n = len(manifest_tasks)

    arm_m_prime = _load_arm(Path(arm_m_prime_dir), Path(ledger_m_prime), manifest_tasks)
    arm_r = _load_arm(Path(arm_r_dir), Path(ledger_r), manifest_tasks)

    completeness_m_prime = _check_arm_completeness(
        ARM_LABEL_M_PRIME, arm_m_prime["ledger_task_half_count"], n
    )
    completeness_r = _check_arm_completeness(ARM_LABEL_R, arm_r["ledger_task_half_count"], n)

    _check_arm_labels(arm_m_prime["ledger_arm_labels"], arm_r["ledger_arm_labels"], expected_arm_labels)

    details_m_prime = _arm_task_details(arm_m_prime, Path(ledger_m_prime))
    details_r = _arm_task_details(arm_r, Path(ledger_r))

    store_statuses_m_prime = _final_episode_statuses(Path(arm_m_prime_dir) / "memory" / "episodes.jsonl")
    store_statuses_r = _final_episode_statuses(Path(arm_r_dir) / "memory" / "episodes.jsonl")

    # Fixed RNG order: H2 first, A1-tokens second (hygiene before anything
    # else touches the stream; both gates here are exact counts and draw
    # nothing).
    rng = random.Random(seed)
    h2 = _h2_p1_equivalence(rng, arm_m_prime["view"], arm_r["view"], b=b)
    a1_tokens = _a1_tokens(rng, arm_m_prime["view"], arm_r["view"], b=b)

    pg1 = _pg1_premise_gone_totality(details_r, arm_r["view"])
    pg2 = _pg2_store_preservation(store_statuses_r, arm_r["tasks_journal_rows"])
    pg3 = _pg3_moot_lesson_injection(arm_m_prime["view"], arm_m_prime["tasks_journal_rows"], floor)

    matched_r = sum(1 for detail in details_r[2].values() if detail["refalsify"] is not None)
    floor_result = _floor_check(matched_r, pg3["injected_count_m_prime_p2"], floor)

    stamp_audit = _stamp_audit_pg(details_m_prime, details_r)
    inconclusive_m = sum(
        1 for detail in details_m_prime[2].values() if detail["refalsify"] == "inconclusive"
    )
    h3 = _h3_pg(
        arm_m_prime["dropped"], arm_r["dropped"], inconclusive_m, len(pg1["inconclusive_names"]), n
    )
    h4 = _h4_pg(
        matched_r,
        pg3["injected_count_m_prime_p2"],
        arm_m_prime["tasks_journal_rows"],
        arm_m_prime["task_id_to_phase"],
        arm_r["tasks_journal_rows"],
        arm_r["task_id_to_phase"],
        n,
    )
    a2 = _a2_aftermath(
        arm_m_prime["tasks_journal_rows"],
        arm_m_prime["task_id_to_phase"],
        store_statuses_m_prime,
        arm_r["tasks_journal_rows"],
        arm_r["task_id_to_phase"],
        store_statuses_r,
        Path(ledger_m_prime),
        Path(ledger_r),
    )
    a3_wall = _a1_wall(details_m_prime, details_r)

    identity_m_prime = _check_identity(ARM_LABEL_M_PRIME, arm_m_prime["identity_by_phase"], expected_digest)
    identity_r = _check_identity(ARM_LABEL_R, arm_r["identity_by_phase"], expected_digest)

    lens = {
        "instrument": "premise-gone-battery-v1",
        "seed": seed,
        "b": b,
        "n": n,
        "floor": floor,
        "arm_labels": {"m_prime": expected_arm_labels[0], "r": expected_arm_labels[1]},
        "source_paths": {
            "corpus_dir": str(corpus_dir),
            "arm_m_prime_dir": str(arm_m_prime_dir),
            "arm_r_dir": str(arm_r_dir),
            "ledger_m_prime": str(ledger_m_prime),
            "ledger_r": str(ledger_r),
        },
        "expected_digest": expected_digest,
        "identity": {
            "m_prime": identity_m_prime,
            "r": identity_r,
            "violated": identity_m_prime["violated"] or identity_r["violated"],
        },
    }

    return {
        "pg1": pg1,
        "pg2": pg2,
        "pg3": pg3,
        "floor": floor_result,
        "stamp_audit": stamp_audit,
        "a1_tokens": a1_tokens,
        "a2_aftermath": a2,
        "a3_wall": a3_wall,
        "h2_p1_equivalence": h2,
        "h3_infra": h3,
        "h4_advisory": h4,
        "completeness": {
            "m_prime": completeness_m_prime,
            "r": completeness_r,
            "violated": completeness_m_prime["violated"] or completeness_r["violated"],
        },
        "dropped": {"m_prime": arm_m_prime["dropped"], "r": arm_r["dropped"]},
        "corpus_sha": _corpus_sha(manifest),
        "lens": lens,
    }


def _cli_fatal_checks(result: dict[str, Any], expected_digest: str) -> list[str]:
    """v2's CLI enforcement layer verbatim (completeness first, then
    identity, both arms both checks)."""
    fatals: list[str] = []
    completeness = result["completeness"]
    for arm_label in (ARM_LABEL_M_PRIME, ARM_LABEL_R):
        arm_completeness = completeness[arm_label]
        if arm_completeness["violated"]:
            fatals.append(
                f"memory_battery.recompute_pg: FATAL: arm {arm_label!r} is incomplete -- "
                f"{arm_completeness['actual_task_halves']} task-half row(s), expected "
                f"{arm_completeness['expected_task_halves']} -- {arm_completeness['reason']}"
            )
    identity = result["lens"]["identity"]
    for arm_label in (ARM_LABEL_M_PRIME, ARM_LABEL_R):
        arm_identity = identity[arm_label]
        if arm_identity["violated"]:
            fatals.append(
                f"memory_battery.recompute_pg: FATAL: identity mismatch on arm {arm_label!r} -- "
                f"phase1_digest={arm_identity['phase1_digest']!r} "
                f"phase2_digest={arm_identity['phase2_digest']!r} expected={expected_digest!r}"
            )
    return fatals


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description=(
            "premise-gone-battery recompute (design spec §5): derives every PG1/PG2/PG3/floor/"
            "stamp-audit/A1-A3/H2-H4 number from journal, ledger, and store bytes plus the "
            "frozen manifest for the m_prime/r arms, prints the pinned JSON schema."
        )
    )
    parser.add_argument("--corpus-dir", type=Path, required=True, help="Frozen corpus directory (manifest.json).")
    parser.add_argument("--arm-m-prime-dir", type=Path, required=True, help="Arm M' (refalsify=false)'s data_dir.")
    parser.add_argument("--arm-r-dir", type=Path, required=True, help="Arm R (refalsify=true)'s data_dir.")
    parser.add_argument("--ledger-m-prime", type=Path, required=True, help="Arm M-prime's driver ledger JSONL path.")
    parser.add_argument("--ledger-r", type=Path, required=True, help="Arm R's driver ledger JSONL path.")
    parser.add_argument(
        "--expected-digest",
        required=True,
        help="REQUIRED: the prereg-pinned served-identity digest; a mismatch is FATAL (nonzero exit).",
    )
    parser.add_argument(
        "--floor",
        type=int,
        default=MATCHED_FLOOR_PG,
        help=f"Matched-count floor (default {MATCHED_FLOOR_PG}, the prereg lock; tests pass smaller).",
    )
    parser.add_argument(
        "--expected-arm-labels",
        nargs=2,
        metavar=("M_PRIME_LABEL", "R_LABEL"),
        default=(ARM_LABEL_M_PRIME, ARM_LABEL_R),
        help="Ledger arm labels to require, (m_prime, r) order; dry shakedowns pass their DRY labels.",
    )
    args = parser.parse_args(argv)

    try:
        result = recompute_pg(
            args.corpus_dir,
            args.arm_m_prime_dir,
            args.arm_r_dir,
            args.ledger_m_prime,
            args.ledger_r,
            expected_digest=args.expected_digest,
            floor=args.floor,
            expected_arm_labels=tuple(args.expected_arm_labels),
        )
    except Exception as exc:  # noqa: BLE001 -- last-resort net, house pattern
        print(f"memory_battery.recompute_pg: FATAL: {exc!r}", file=sys.stderr)
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
