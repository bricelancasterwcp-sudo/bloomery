"""memory-battery recompute (design spec §4/§5; task-4 brief): the single
source of every number the battery's gate and findings doc will ever quote.

Per design spec §5: "journal bytes are the only source any quoted number
may have." This module reads BOTH arms' journals directly --
``<arm_dir>/journal/tasks.jsonl`` (``MemoryStamp``, ``TaskStep``,
``MemoryMint``, ``MemoryContradicted`` rows -- ``crates/bloomery-daemon/src/
task/registry.rs``'s own task journal, per ``main.rs``'s
``set_task_journal_path``) and ``<arm_dir>/journal/boot-*.jsonl`` (the
daemon's boot journal, ``InferCompleted`` rows -- ``crates/bloomery-daemon/
src/main.rs``'s ``journal_path``) -- and the driver's own ledger
(``driver.py``'s ``Ledger``), used ONLY for two things per the task-4 brief:
mapping a driver task name to the daemon's ``task_id`` for a given
``(arm, phase)``, and the ``driver-infra`` status flag. Every other quoted
number is computed here from journal bytes or the frozen manifest.

**The join** (task-4 brief, ``registry.rs`` as built): the driver ledger's
task-half row names ``task_id``; ``tasks.jsonl``'s ``MemoryStamp`` row for
that ``task_id`` names ``id`` -- the fresh agent the daemon created for that
one task-half. Every ``InferCompleted``/``TaskStep`` row sharing that ``id``
belongs to this task-half and only this one -- a fresh agent per task-half
(design spec §5) makes the join exact. See ``recompute_join.py``'s
docstring for the join implementation itself.

**Formulas are quoted from design spec §4 by reference, never restated**:
this module's doc comments cite the section; the code (here and in
``recompute_bootstrap.py``) computes exactly the cited formula. ``cost
(task)``, ``success``, and ``infra`` are pinned verbatim in the task-4
brief and quoted in the functions that implement them.

**R-PF-B1 (amended), binding this task**: ``lens`` digests come from the
DRIVER LEDGER's identity rows (``driver.py``'s ``_assert_identity``; two
rows per arm, ``{"arm","phase","event":"identity","digest","ts"}``) -- not
from any boot-journal event, because no ``Event`` variant in
``bloomery-core/src/journal.rs`` carries a served-identity digest at all
(``driver.py``'s own docstring: "no journal Event carries a GGUF digest; it
lives only in ``/status``"). The two ledger identity rows within one arm
must agree with each other; a mismatch is a hygiene violation that
short-circuits E1 to INVALID, exactly like H1/H2/H3. This is the ONE
ledger-derived fact this module treats as load-bearing evidence rather than
pure observation -- the ledger-independence invariant (a wrong ``wall_s``
never changes any output number) explicitly EXEMPTS identity rows and
``driver-infra`` status flags (controller ruling).

**Judgment call (flagged for the controller, task-3 precedent):** the
task-4 brief pins ``recompute``'s signature to exactly five positional
parameters, but R-PF-B1 (amended) also says the two ledger identity rows
must agree "with the expected digest passed in" -- a value the pinned
signature has no parameter for. Resolved additively, matching
``driver.py``'s own MODEL/WINDOW_CAP judgment-call precedent: an OPTIONAL
keyword-only ``expected_digest`` parameter, defaulting to ``None`` (pure
within-arm self-consistency check only). Every caller in this task's own
test suite calls ``recompute`` positionally exactly as pinned; a caller
that also has the prereg's pinned digest handy (design spec §2) may pass it
to get the full check R-PF-B1 describes.

**File split (house rule, `coding-style.md`: 800-line ceiling).** This
module's join logic lives in ``recompute_join.py``, its journal/ledger I/O
in ``recompute_journal.py``, and its seeded bootstrap + hygiene H1-H3 + E1
in ``recompute_bootstrap.py`` -- all private, `tools/memory_battery/`-local
modules; the ONE public entry point stays ``tools.memory_battery.recompute
.recompute``, exactly as the task-4 brief pins it, and its five-argument
signature and pinned output schema are unchanged by the split.

Python 3 stdlib only (``json``, ``random``, ``statistics``, ``hashlib``,
``argparse``); no GPU access, no clock reads -- every number derives from
journal/ledger/manifest bytes already on disk.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import random
import statistics
import sys
from pathlib import Path
from typing import Any, Sequence

from tools.memory_battery.driver import WINDOW_CAP
from tools.memory_battery.recompute_bootstrap import (
    ARM_C_ALLOWED_MODES,
    ARM_LABEL_C,
    ARM_LABEL_M,
    ARM_M_ALLOWED_MODES,
    BOOTSTRAP_B,
    BOOTSTRAP_SEED,
    _bootstrap_diff_paired,
    _check_arm_completeness,
    _check_e1,
    _check_h1,
    _check_h2,
    _check_h3,
    _check_identity,
    _check_treatment_identity,
    _median_or_none,
)
from tools.memory_battery.recompute_join import _load_arm

# Design spec §2: "envelope v4". No `Event` variant in
# `bloomery-core/src/journal.rs` carries an envelope identity (unlike
# `window_cap`, which IS observable on every `AgentCreated` row and so is
# imported from `driver.py`'s own module constant instead of duplicated
# here) -- pinned as an instrument constant, the same role `driver.py`'s
# `MODEL`/`WINDOW_CAP` play for values nothing in the journal records.
ENVELOPE = "v4"

# Design spec §1: success-rate lift is not the gate here; this note is
# attached to the advisory success-rate block, cited rather than restated.
SATURATION_NOTE = (
    "Advisory only, under design spec §1's pre-declared saturation note: "
    "every gate-passing bloomery model sits at patch ceiling on factory "
    "tasks, so success-rate lift is not this battery's gate -- E1's cost "
    "endpoint is."
)


# ---------------------------------------------------------------------------
# Advisory endpoints (design spec §4: "reported, never gating").
# ---------------------------------------------------------------------------


def _success_rate(successes_by_task: dict[str, bool]) -> dict[str, Any]:
    n = len(successes_by_task)
    if n == 0:
        return {"rate": None, "successes": 0, "n": 0}
    successes = sum(1 for value in successes_by_task.values() if value)
    return {"rate": successes / n, "successes": successes, "n": n}


def _paired_deltas_m(
    rng: random.Random, view_m: dict[str, Any], manifest_tasks: list[dict[str, Any]]
) -> dict[str, Any]:
    """Design spec §4 advisory: "per-task paired phase-2-phase-1 deltas
    within M." Only tasks non-dropped in BOTH M phases can pair.

    **Review finding I2 fix.** Spec §4 names this endpoint as a
    within-arm, PAIRED-bootstrap consumer in the same breath as H1
    ("for the within-arm differences (H1, and M's advisory paired
    deltas) tasks resample as p1/p2 PAIRS") -- an earlier revision
    reported `median_delta` with no SE at all. `se_boot` here is exactly
    H1's own bootstrap primitive (`_bootstrap_diff_paired`) applied to
    THESE pairs (M's own phase-1/phase-2 values) instead of arm C's.
    `None` only when there are zero pairs to resample (none-vs-zero:
    never a fabricated 0).

    **RNG consumption order (`recompute.py`'s `recompute()`, documented
    once here since this is where the order's tail end lives):** identity
    -> H1 -> H2 -> [E1, only if hygiene clean] -> this function, always,
    last. Advisory, so it runs regardless of hygiene/gate outcome; it
    runs AFTER E1's own bootstrap so a hygiene-clean run's `e1.delta_min`
    is fully computed and returned before this call ever touches `rng`
    again -- this function's own draws can never retroactively affect an
    already-returned `e1` value."""
    names = [task["name"] for task in manifest_tasks]
    pairs_with_names = [
        (name, view_m["costs"][1][name], view_m["costs"][2][name])
        for name in names
        if name in view_m["costs"][1] and name in view_m["costs"][2]
    ]
    per_task = [{"task": name, "delta": p2 - p1} for name, p1, p2 in pairs_with_names]
    deltas = [entry["delta"] for entry in per_task]
    se_boot = None
    if pairs_with_names:
        pairs = [(p1, p2) for _, p1, p2 in pairs_with_names]
        se_boot = statistics.pstdev(_bootstrap_diff_paired(rng, pairs))
    return {
        "per_task": per_task,
        "median_delta": _median_or_none(deltas),
        "se_boot": se_boot,
        "n_pairs": len(per_task),
    }


def _mint_rate_p1(
    tasks_journal_rows: list[dict[str, Any]], task_id_to_phase: dict[str, Any], n: int
) -> tuple[int, float | None]:
    """Design spec §4, H4 advisory: "mint rate in M-p1." A `MemoryMint` row
    carries no phase of its own (`registry.rs`'s `Event::MemoryMint` has no
    such field) -- its `task_id` is looked up in the arm's own ledger-
    derived task_id -> phase map to tell a phase-1 mint from a phase-2
    refresh (`registry.rs`'s own doc comment: "A repeat that verifies
    re-mints the same episode_id")."""
    count = sum(
        1
        for row in tasks_journal_rows
        if row.get("event") == "MemoryMint" and task_id_to_phase.get(row.get("task_id")) == 1
    )
    return count, (count / n if n else None)


# ---------------------------------------------------------------------------
# Lens (design spec §2 pins; corpus_sha is manifest-derived, not a live
# filesystem re-read -- see docstring below).
# ---------------------------------------------------------------------------


def _corpus_sha(manifest: dict[str, Any]) -> str:
    """A manifest-level aggregate digest: sha256 over the sorted
    ``(task name, workspace_sha256)`` pairs, each pair NUL-separated on
    both sides -- the same sorted-pairs hashing convention ``corpus.py``'s
    ``_workspace_sha256`` and ``corpus_check.py``'s independent
    reimplementation both use. This is NOT Task 5's separate freeze-time
    sha (design spec §6 step 2's own "sha256 over the sorted manifest +
    workspace bytes" procedure, computed once at freeze and recorded in
    the prereg) -- recompute has no access to live workspace bytes at gate
    time, and does not need them: each task's ``workspace_sha256`` is
    ALREADY the frozen manifest's own claim about those bytes (task-1
    brief), so hashing the manifest's own per-task hashes derives a corpus
    identity purely from frozen manifest bytes -- consistent with this
    module's own invariant that every lens field derives from journal
    bytes or the frozen manifest, never a live filesystem re-read."""
    hasher = hashlib.sha256()
    for entry in sorted(manifest.get("tasks", []), key=lambda task: task["name"]):
        hasher.update(entry["name"].encode("utf-8"))
        hasher.update(b"\0")
        hasher.update(entry["workspace_sha256"].encode("utf-8"))
        hasher.update(b"\0")
    return hasher.hexdigest()


# ---------------------------------------------------------------------------
# recompute -- the pinned entry point (task-4 brief).
# ---------------------------------------------------------------------------


def recompute(
    corpus_dir: str | Path,
    arm_c_dir: str | Path,
    arm_m_dir: str | Path,
    ledger_c: str | Path,
    ledger_m: str | Path,
    *,
    expected_digest: str | None = None,
) -> dict[str, Any]:
    """Task-4 brief's pinned entry point: five positional parameters
    (``corpus_dir``, ``arm_c_dir``, ``arm_m_dir``, ``ledger_c``,
    ``ledger_m``) -- ``expected_digest`` is an ADDITIVE, optional
    keyword-only parameter (see this module's docstring judgment call).

    **The fixed hygiene evaluation order** (design spec §4's own H1 -> H2
    -> H3 sequence, with three guards the spec does not name slotted ahead
    of it -- each one is a question about whether the numbers are even
    ABOUT what they claim, which is logically prior to comparing medians):

        1. arm completeness   (review finding C2 -- did the arm finish?)
        2. identity           (R-PF-B1 -- was it the pinned model?)
        3. treatment identity (finding I-2 -- was it the pinned ARM?)
        4. H1 control stability
        5. H2 first-exposure equivalence
        6. H3 infra rate

    Only steps 4 and 5 consume the seeded RNG; 1-3 are pure comparisons,
    which is what lets them be inserted without disturbing the bootstrap's
    pinned draw order. All six are evaluated UNCONDITIONALLY (never
    skipped; every hygiene finding is reported even when an earlier one
    already failed, since that is strictly more informative for a findings
    doc than stopping at the first violation).
    E1's bootstrap and PASS/FAIL/UNMEASURABLE decision is the only thing
    short-circuited to INVALID when any hygiene check is violated (design
    spec §4: "any INVALID short-circuits E1's verdict to INVALID"; §6:
    "no gate number was read" licenses a from-zero rerun) -- E1's raw
    phase-2 medians are still reported even then, since they are
    descriptive statistics, not the gate's own derived bootstrap number.

    Returns the pinned top-level schema: ``{"verdict", "e1", "hygiene",
    "advisory", "lens", "dropped"}``. Every value in the return is a plain
    JSON-native type (str/int/float/bool/None/list/dict) -- no dataclasses,
    no sets, no tuples reach the return boundary -- so the result
    round-trips through ``json.dumps``/``json.loads`` unchanged."""
    corpus_dir = Path(corpus_dir)
    manifest = json.loads((corpus_dir / "manifest.json").read_text(encoding="utf-8"))
    manifest_tasks = manifest["tasks"]
    n = len(manifest_tasks)

    arm_c = _load_arm(Path(arm_c_dir), Path(ledger_c), manifest_tasks)
    arm_m = _load_arm(Path(arm_m_dir), Path(ledger_m), manifest_tasks)

    # One seeded RNG instance, consumed in a FIXED program order (H1 -> H2 ->
    # [E1 if hygiene clean] -> M's advisory paired-deltas SE, always last --
    # see `_paired_deltas_m`'s docstring) regardless of the data -- see
    # `recompute_bootstrap.py`'s BOOTSTRAP_SEED comment: this is what makes
    # `delta_min` reproducible run-to-run for identical inputs (mutation
    # check #3).
    rng = random.Random(BOOTSTRAP_SEED)

    # Review finding C2: arm completeness is checked FIRST, ahead of
    # identity/H1/H2/H3 -- whether a run is even complete enough to trust
    # is logically prior to comparing digests or medians on it.
    completeness_c = _check_arm_completeness("C", arm_c["ledger_task_half_count"], n)
    completeness_m = _check_arm_completeness("M", arm_m["ledger_task_half_count"], n)
    completeness_violated = completeness_c["violated"] or completeness_m["violated"]

    identity_c = _check_identity(ARM_LABEL_C, arm_c["identity_by_phase"], expected_digest)
    identity_m = _check_identity(ARM_LABEL_M, arm_m["identity_by_phase"], expected_digest)
    identity_violated = identity_c["violated"] or identity_m["violated"]

    # Branch-review finding I-2: treatment identity slots THIRD -- after
    # arm-completeness and served-model identity, BEFORE H1 (see this
    # function's docstring for the full fixed order). It consumes no RNG,
    # so H1/H2/E1's pinned bootstrap draw order is untouched.
    treatment_c = _check_treatment_identity(
        ARM_LABEL_C, ARM_C_ALLOWED_MODES, arm_c["view"], arm_c["ledger_arm_labels"]
    )
    treatment_m = _check_treatment_identity(
        ARM_LABEL_M, ARM_M_ALLOWED_MODES, arm_m["view"], arm_m["ledger_arm_labels"]
    )
    treatment_violated = treatment_c["violated"] or treatment_m["violated"]

    h1 = _check_h1(rng, arm_c["view"], manifest_tasks)
    h2 = _check_h2(rng, arm_c["view"], arm_m["view"], manifest_tasks)
    h3 = _check_h3(arm_c["dropped"], arm_m["dropped"], n)

    hygiene_violated = (
        completeness_violated
        or identity_violated
        or treatment_violated
        or h1["violated"]
        or h2["violated"]
        or h3["violated"]
    )
    hygiene_reasons = [
        reason
        for reason in (
            completeness_c["reason"],
            completeness_m["reason"],
            identity_c["reason"],
            identity_m["reason"],
            treatment_c["reason"],
            treatment_m["reason"],
            h1["reason"],
            h2["reason"],
            h3["reason"],
        )
        if reason
    ]

    if hygiene_violated:
        costs_c_p2 = list(arm_c["view"]["costs"][2].values())
        costs_m_p2 = list(arm_m["view"]["costs"][2].values())
        e1 = {
            "verdict": "INVALID",
            "median_c_p2": _median_or_none(costs_c_p2),
            "median_m_p2": _median_or_none(costs_m_p2),
            "min_c_p2": min(costs_c_p2) if costs_c_p2 else None,
            "headroom": None,
            "delta_min": None,
            "se_boot": None,
            "n_c_p2": len(costs_c_p2),
            "n_m_p2": len(costs_m_p2),
            "reason": "hygiene INVALID before E1 was read: " + "; ".join(hygiene_reasons),
        }
    else:
        e1 = _check_e1(rng, arm_c["view"], arm_m["view"])

    injected_count = sum(1 for mode in arm_m["view"]["modes"][2].values() if mode == "injected")
    injection_rate = (injected_count / n) if n else None
    mint_count_p1, mint_rate_p1 = _mint_rate_p1(
        arm_m["tasks_journal_rows"], arm_m["task_id_to_phase"], n
    )

    advisory = {
        "saturation_note": SATURATION_NOTE,
        "h4": {
            "m_p2_injection_rate": injection_rate,
            "m_p2_injected_count": injected_count,
            "m_p1_mint_rate": mint_rate_p1,
            "m_p1_mint_count": mint_count_p1,
            "n": n,
        },
        "success_rates": {
            "c_p1": _success_rate(arm_c["view"]["successes"][1]),
            "c_p2": _success_rate(arm_c["view"]["successes"][2]),
            "m_p1": _success_rate(arm_m["view"]["successes"][1]),
            "m_p2": _success_rate(arm_m["view"]["successes"][2]),
        },
        "steps_median": {
            "c_p1": _median_or_none(arm_c["view"]["steps"][1]),
            "c_p2": _median_or_none(arm_c["view"]["steps"][2]),
            "m_p1": _median_or_none(arm_m["view"]["steps"][1]),
            "m_p2": _median_or_none(arm_m["view"]["steps"][2]),
        },
        "wall_ms_median": {
            "c_p1": _median_or_none(arm_c["view"]["wall_ms"][1]),
            "c_p2": _median_or_none(arm_c["view"]["wall_ms"][2]),
            "m_p1": _median_or_none(arm_m["view"]["wall_ms"][1]),
            "m_p2": _median_or_none(arm_m["view"]["wall_ms"][2]),
        },
        "paired_deltas_m": _paired_deltas_m(rng, arm_m["view"], manifest_tasks),
        "row_counts": {
            "c_stamp": arm_c["row_counts"]["stamp"],
            "c_mint": arm_c["row_counts"]["mint"],
            "c_contradicted": arm_c["row_counts"]["contradicted"],
            "m_stamp": arm_m["row_counts"]["stamp"],
            "m_mint": arm_m["row_counts"]["mint"],
            "m_contradicted": arm_m["row_counts"]["contradicted"],
        },
        "costs": {
            "c": {"p1": arm_c["view"]["costs"][1], "p2": arm_c["view"]["costs"][2]},
            "m": {"p1": arm_m["view"]["costs"][1], "p2": arm_m["view"]["costs"][2]},
        },
        "modes_m": {"p1": arm_m["view"]["modes"][1], "p2": arm_m["view"]["modes"][2]},
        "successes": {
            "c": {"p1": arm_c["view"]["successes"][1], "p2": arm_c["view"]["successes"][2]},
            "m": {"p1": arm_m["view"]["successes"][1], "p2": arm_m["view"]["successes"][2]},
        },
    }

    lens = {
        "instrument": manifest.get("instrument"),
        "corpus_seed": manifest.get("corpus_seed"),
        "n": n,
        "corpus_sha": _corpus_sha(manifest),
        "envelope": ENVELOPE,
        "window_cap": WINDOW_CAP,
        "bootstrap_seed": BOOTSTRAP_SEED,
        "bootstrap_b": BOOTSTRAP_B,
        "expected_digest": expected_digest,
        "digest_c": {"phase1": arm_c["identity_by_phase"].get(1), "phase2": arm_c["identity_by_phase"].get(2)},
        "digest_m": {"phase1": arm_m["identity_by_phase"].get(1), "phase2": arm_m["identity_by_phase"].get(2)},
    }

    hygiene = {
        "violated": hygiene_violated,
        "reasons": hygiene_reasons,
        "arm_completeness": {"c": completeness_c, "m": completeness_m, "violated": completeness_violated},
        "identity": {"c": identity_c, "m": identity_m, "violated": identity_violated},
        "treatment_identity": {"c": treatment_c, "m": treatment_m, "violated": treatment_violated},
        "h1_control_stability": h1,
        "h2_first_exposure_equivalence": h2,
        "h3_infra_rate": h3,
    }

    dropped = {"C": arm_c["dropped"], "M": arm_m["dropped"]}

    return {
        "verdict": e1["verdict"],
        "e1": e1,
        "hygiene": hygiene,
        "advisory": advisory,
        "lens": lens,
        "dropped": dropped,
    }


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description=(
            "memory-battery recompute (design spec §4/§5; task-4 brief): derives every "
            "gate/advisory number from journal bytes and the frozen manifest, prints the "
            "pinned JSON schema."
        )
    )
    parser.add_argument("--corpus-dir", type=Path, required=True, help="Frozen corpus directory (manifest.json).")
    parser.add_argument("--arm-c-dir", type=Path, required=True, help="Arm C's data_dir (journal/ subdirectory).")
    parser.add_argument("--arm-m-dir", type=Path, required=True, help="Arm M's data_dir (journal/ subdirectory).")
    parser.add_argument("--ledger-c", type=Path, required=True, help="Arm C's driver ledger JSONL path.")
    parser.add_argument("--ledger-m", type=Path, required=True, help="Arm M's driver ledger JSONL path.")
    parser.add_argument(
        "--expected-digest",
        required=True,
        help=(
            "REQUIRED: the prereg-pinned served-identity digest (design spec §2). Checked "
            "against both arms' ledger identity rows -- a mismatch is a named hygiene "
            "violation, verdict INVALID (review finding I3: the CLI enforces this because a "
            "real gate run must never silently skip it; the library `recompute()` kwarg "
            "stays optional-None so fixtures/tests that don't care about identity can omit it)."
        ),
    )
    args = parser.parse_args(argv)

    try:
        result = recompute(
            args.corpus_dir,
            args.arm_c_dir,
            args.arm_m_dir,
            args.ledger_c,
            args.ledger_m,
            expected_digest=args.expected_digest,
        )
    except Exception as exc:  # noqa: BLE001 -- last-resort net, house pattern (corpus_check.py/driver.py)
        print(f"memory_battery.recompute: FATAL: {exc!r}", file=sys.stderr)
        return 1

    print(json.dumps(result, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
