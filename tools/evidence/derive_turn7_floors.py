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
FIXTURE_COUNT = 32
FAMILY_COUNTS = {"defect-absent": 6, "missing-target": 5, "symptom-mismatch": 5}
PATCH_COUNT = 16


def improvement_floor(passes: int, n: int) -> dict:
    """Smallest k with k/n strictly above wilson95(passes, n)'s upper."""
    _, hi = wilson95(passes, n)
    k = next(k for k in range(n + 1) if k / n > hi)
    return {"baseline": [passes, n], "wilson95_upper": hi, "floor": k, "rule": "improvement"}


def hold_floor(passes: int, n: int) -> dict:
    """Smallest k with k/n at or above wilson95(passes, n)'s lower."""
    lo, _ = wilson95(passes, n)
    k = next(k for k in range(n + 1) if k / n >= lo)
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


def main(argv=None) -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--baseline", required=True, type=Path)
    ap.add_argument("--fixtures", required=True, type=Path)
    ap.add_argument("--json", type=Path, default=None)
    a = ap.parse_args(argv)
    report = derive(a.baseline, a.fixtures)
    text = json.dumps(report, indent=2, sort_keys=True)
    if a.json:
        a.json.write_text(text + "\n", encoding="utf-8")
    print(text)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
