"""The turn-7 floor derivation (turn-7 spec §4.3), EXECUTED — never hand
arithmetic in a doc. Reads the committed untrained-base boot-1 recompute
JSON (the subject's own comparator: fw7 trains from this base), verifies
the frozen instrument's identity and composition, and emits every derived
floor with its bound, using the repo's ONE Wilson implementation
(`endpoints.wilson95`). The pre-registration quotes this tool's JSON.

Rules (spec §4.3):
- improvement floor = the smallest integer count whose proportion EXCEEDS
  the untrained base's Wilson-95% upper bound;
- hold floor = the smallest integer count whose proportion is >= the
  untrained base's Wilson-95% lower bound;
- every proportion is over the FROZEN SET's own row counts (spec §4.2's
  fixed-denominator rule: 32 fixtures; refuse families 6/5/5) — a fixture
  whose task never emits `done` contributes nothing to any numerator, so
  the baseline numerators are used as counts-of-the-set unchanged;
- F6 (defect-absent >= 4/6) is CHOSEN, not derived (the anti-constant-
  policy guard, spec-flagged [judgment]); this tool prints its anchors
  (the untrained point and band) for the sanity check but never computes
  it.

Usage:
  python3 -m tools.evidence.derive_turn7_floors \
      --baseline docs/superpowers/evidence/2026-08-29-g5v5-reap48ours-boot1-recompute.json \
      --fixtures crates/bloomery-daemon/fixtures/codec-tasks-v5-mixed.toml \
      [--json out.json]
"""
from __future__ import annotations

import argparse
import hashlib
import json
import tomllib
from pathlib import Path

from .endpoints import wilson95

# The frozen instrument this turn measures on (turn-6 B3 freeze commit).
V5_MIXED_SHA256 = "bf2db8ac3c645f37e681412f4606c50a3ecd52d0548a2c09be7c18641ca0ae13"
# The comparator's identity, pinned like the instrument's (adversarial
# review F-2, 2026-08-29): a wrong --baseline would otherwise derive
# materially different floors SILENTLY — the named bug class, at the exact
# spot the pre-registration locks.
BASELINE_SHA256 = "7ee27c330ed11d8411cb174c020306e5200dddc4497e222522d93fc0d4cf413e"
FIXTURE_COUNT = 32
FAMILY_COUNTS = {"defect-absent": 6, "missing-target": 5, "symptom-mismatch": 5}
PATCH_COUNT = 16


def improvement_floor(passes: int, n: int) -> dict:
    """Smallest k with k/n strictly above wilson95(passes, n)'s upper."""
    _, hi = wilson95(passes, n)
    k = next((k for k in range(n + 1) if k / n > hi), None)
    if k is None:
        # A saturated baseline (upper bound 1.0) has no improvement floor
        # above it — loud and named, never a bare StopIteration (F-5).
        raise ValueError(
            f"improvement_floor: baseline {passes}/{n} is saturated -- no count can exceed its "
            f"Wilson upper bound {hi}; an improvement floor cannot be derived from it"
        )
    return {"baseline": [passes, n], "wilson95_upper": hi, "floor": k, "rule": "improvement"}


def hold_floor(passes: int, n: int) -> dict:
    """Smallest k with k/n at or above wilson95(passes, n)'s lower."""
    lo, _ = wilson95(passes, n)
    k = next(k for k in range(n + 1) if k / n >= lo)  # k=n always satisfies >= lo
    return {"baseline": [passes, n], "wilson95_lower": lo, "floor": k, "rule": "hold"}


def verify_instrument(fixtures_path: Path) -> dict:
    """The frozen set's identity and composition, checked — not asserted
    from memory. Any mismatch is a hard error: floors over the wrong
    denominators would be values that look like measurements but are not."""
    raw = fixtures_path.read_bytes()
    sha = hashlib.sha256(raw).hexdigest()
    if sha != V5_MIXED_SHA256:
        raise SystemExit(
            f"derive_turn7_floors: {fixtures_path} sha256 {sha} != frozen {V5_MIXED_SHA256}"
        )
    doc = tomllib.loads(raw.decode("utf-8"))
    fixtures = doc["fixture"]
    if len(fixtures) != FIXTURE_COUNT:
        raise SystemExit(f"derive_turn7_floors: {len(fixtures)} fixtures, expected {FIXTURE_COUNT}")
    families: dict[str, int] = {}
    patch = 0
    for fx in fixtures:
        if fx["expect"] == "patch":
            patch += 1
        else:
            families[fx["family"]] = families.get(fx["family"], 0) + 1
    if patch != PATCH_COUNT or families != FAMILY_COUNTS:
        raise SystemExit(
            f"derive_turn7_floors: composition patch={patch} families={families}, expected "
            f"patch={PATCH_COUNT} families={FAMILY_COUNTS}"
        )
    return {"path": str(fixtures_path), "sha256": sha, "fixtures": FIXTURE_COUNT,
            "patch": patch, "families": families}


def derive(baseline_path: Path, fixtures_path: Path) -> dict:
    instrument = verify_instrument(fixtures_path)
    baseline_sha = hashlib.sha256(baseline_path.read_bytes()).hexdigest()
    if baseline_sha != BASELINE_SHA256:
        raise SystemExit(
            f"derive_turn7_floors: {baseline_path} sha256 {baseline_sha} != pinned comparator "
            f"{BASELINE_SHA256} (the untrained-base boot-1 recompute JSON) -- floors derived "
            f"from any other file would be silently wrong (F-2)"
        )
    baseline = json.loads(baseline_path.read_text(encoding="utf-8"))
    dec = baseline["declarations"]
    by_family = dec["reason_matches_family"]["by_family"]

    # Fixed-denominator inputs (spec §4.2): numerators as counts-of-the-set.
    consistent = dec["outcome_consistent"]["consistent"]
    grounded = dec["evidence_grounded"]["grounded"]
    sm_match = by_family["symptom-mismatch"]["match"]
    mt_match = by_family["missing-target"]["match"]
    da_match = by_family["defect-absent"]["match"]

    floors = {
        "F1_g4": {"floor": 16, "n": 20, "rule": "carried standing gate"},
        "F2_landing": {"patch_floor": 13, "refuse_floor": 13, "n": 16,
                       "rule": "carried per-class floors (v5 protocol §4)"},
        "F3_outcome_consistent": improvement_floor(consistent, FIXTURE_COUNT),
        "F4_symptom_mismatch_match": improvement_floor(sm_match, FAMILY_COUNTS["symptom-mismatch"]),
        "F5_evidence_grounded": improvement_floor(grounded, FIXTURE_COUNT),
        "F6_defect_absent_match": {
            "floor": 4, "n": FAMILY_COUNTS["defect-absent"], "rule": "chosen [judgment]",
            "anchors": {"untrained": [da_match, FAMILY_COUNTS["defect-absent"]],
                        "untrained_wilson95": list(wilson95(da_match, FAMILY_COUNTS["defect-absent"])),
                        "constant_policy_would_score": 0},
        },
        "F7_missing_target_match": hold_floor(mt_match, FAMILY_COUNTS["missing-target"]),
    }
    return {
        "instrument": instrument,
        "baseline": {"path": str(baseline_path),
                     "sha256": hashlib.sha256(baseline_path.read_bytes()).hexdigest()},
        "denominator_rule": "fixed: the frozen set's own row counts; a fixture with no done "
                            "contributes nothing to any numerator (spec §4.2)",
        "floors": floors,
    }


def evaluate(floors_report: dict, subject_path: Path) -> dict:
    """The battery's mechanical floor verdict (adversarial review F-1's
    second half): reads a SUBJECT's recompute JSON, refuses it unless its
    eval-time instrument binding is clean (`instrument_rows` present, no
    duplicates/unknowns, no join violations), then compares every floor —
    no human arithmetic at verdict time. Floors come from the derivation
    report, never re-typed here."""
    subject = json.loads(subject_path.read_text(encoding="utf-8"))
    rows = subject.get("instrument_rows")
    if rows is None:
        raise SystemExit(
            "evaluate: subject recompute JSON has no instrument_rows key -- re-run "
            "tools.evidence.recompute (this repo's version) over the boot journals first"
        )
    if rows["duplicates"] or rows["unknown"]:
        raise SystemExit(
            f"evaluate: subject journal fails the instrument-row binding "
            f"(duplicates={rows['duplicates']} unknown={rows['unknown']}) -- not a valid "
            f"measurement, no floor verdict is produced"
        )
    if subject["join"]["violations"]:
        raise SystemExit(f"evaluate: subject join violations {subject['join']['violations']}")

    floors = floors_report["floors"]
    dec = subject["declarations"]
    fam = dec["reason_matches_family"]["by_family"]
    g4, g5 = subject["g4"], subject["g5"]
    checks = {
        "F1_g4": {"floor": floors["F1_g4"]["floor"], "observed": g4["landed"],
                  "n_ok": g4["n"] == 20},
        "F2_patch": {"floor": floors["F2_landing"]["patch_floor"], "observed": g5["patch"]["landed"],
                     "n_ok": g5["patch"]["n"] == 16},
        "F2_refuse": {"floor": floors["F2_landing"]["refuse_floor"], "observed": g5["refuse"]["landed"],
                      "n_ok": g5["refuse"]["n"] == 16},
        "F3_outcome_consistent": {"floor": floors["F3_outcome_consistent"]["floor"],
                                  "observed": dec["outcome_consistent"]["consistent"], "n_ok": True},
        "F4_symptom_mismatch_match": {"floor": floors["F4_symptom_mismatch_match"]["floor"],
                                      "observed": fam["symptom-mismatch"]["match"], "n_ok": True},
        "F5_evidence_grounded": {"floor": floors["F5_evidence_grounded"]["floor"],
                                 "observed": dec["evidence_grounded"]["grounded"], "n_ok": True},
        "F6_defect_absent_match": {"floor": floors["F6_defect_absent_match"]["floor"],
                                   "observed": fam["defect-absent"]["match"], "n_ok": True},
        "F7_missing_target_match": {"floor": floors["F7_missing_target_match"]["floor"],
                                    "observed": fam["missing-target"]["match"], "n_ok": True},
    }
    for entry in checks.values():
        entry["pass"] = bool(entry["n_ok"]) and entry["observed"] >= entry["floor"]
    return {"subject": {"path": str(subject_path),
                        "sha256": hashlib.sha256(subject_path.read_bytes()).hexdigest()},
            "instrument_rows": rows,
            "checks": checks,
            "all_pass": all(entry["pass"] for entry in checks.values())}


def main(argv=None) -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--baseline", required=True, type=Path)
    ap.add_argument("--fixtures", required=True, type=Path)
    ap.add_argument("--evaluate", type=Path, default=None,
                    help="a subject's recompute JSON: compare it against every derived floor; "
                         "exit 3 unless all floors pass")
    ap.add_argument("--json", type=Path, default=None)
    a = ap.parse_args(argv)
    report = derive(a.baseline, a.fixtures)
    if a.evaluate is not None:
        report["evaluation"] = evaluate(report, a.evaluate)
    text = json.dumps(report, indent=2, sort_keys=True)
    if a.json:
        a.json.write_text(text + "\n", encoding="utf-8")
    print(text)
    if a.evaluate is not None and not report["evaluation"]["all_pass"]:
        return 3
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
