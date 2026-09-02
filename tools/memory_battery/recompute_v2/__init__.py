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

# Split 2026-09-01 (carried-debt slice D): this module was 850 lines. Constants
# moved to `constants.py`, the arm helpers to `arms.py`, the registered
# endpoints to `endpoints.py`. Everything is re-exported here so every existing
# import keeps working unchanged -- `recompute_pg.py`, `recompute_s5.py` and
# the tests all reach for `tools.memory_battery.recompute_v2`.

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
from tools.memory_battery.recompute_v2.arms import (  # noqa: F401
    _arm_task_details,
    _check_arm_labels,
    _median_or_none,
)
from tools.memory_battery.recompute_v2.endpoints import (  # noqa: F401
    _a1_wall,
    _g1_token_preservation,
    _g2_injection_preservation,
    _h2_p1_equivalence,
    _h3_infra,
    _h4_advisory,
    _stamp_audit,
)



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

    details_m_prime = _arm_task_details(arm_m_prime, Path(ledger_m_prime))
    details_r = _arm_task_details(arm_r, Path(ledger_r))

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
    parser.add_argument(
        "--ledger-m-prime", type=Path, required=True, help="Arm M-prime's driver ledger JSONL path."
    )
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
