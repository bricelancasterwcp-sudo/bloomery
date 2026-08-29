"""Recompute a boot's G4/G5 verdicts and secondary endpoints from its committed
journals (turn-5 spec §3). It REPORTS; the daemon DECIDES — this tool is never
on the gate path. Usage:

  python3 -m tools.evidence.recompute --journal J.jsonl --tasks T.jsonl \
      --g5-fixtures crates/bloomery-daemon/fixtures/codec-tasks-v4-mixed.toml \
      [--g4-set codec-tasks-v1] [--json out.json]
"""
from __future__ import annotations

import argparse
import json
from pathlib import Path

from . import endpoints as ep
from .journal import join, load_rows


def _journaled_g4(journal, set_name):
    for r in journal:
        if r.get("event") == "CodecVerdict" and r.get("fixture_set") == set_name:
            return r
    return None


def _journaled_g5(journal, set_name):
    for r in journal:
        if r.get("event") == "CodecVerdictMixed" and r.get("fixture_set") == set_name:
            return r
    return None


def recompute(journal: Path, tasks: Path, g5_fixtures: Path | None, g4_set: str = "codec-tasks-v1") -> dict:
    jrows, trows = load_rows(journal), load_rows(tasks)
    joined, jr = join(jrows, trows)
    report = {"join": {"mode": jr.mode, "keyed_equals_ordinal": jr.keyed_equals_ordinal,
                       "fixtures": jr.fixtures, "groups": jr.groups, "violations": jr.violations,
                       "ordinal_violations": jr.ordinal_violations}}

    g4_rows = [j for j in joined if j.fixture["fixture_set"] == g4_set]
    if g4_rows:
        g4 = ep.leg(sum(bool(j.fixture["landed"]) for j in g4_rows), len(g4_rows))
        jv = _journaled_g4(jrows, g4_set)
        g4["journaled_verdict_matches"] = bool(jv) and (jv["landed"], jv["n"], jv["provisional"]) == (g4["landed"], g4["n"], g4["provisional"])
        report["g4"] = {"set": g4_set, **g4}
    else:
        report["g4"] = None

    g5_set = Path(g5_fixtures).stem if g5_fixtures else None
    g5_rows = [j for j in joined if g5_set and j.fixture["fixture_set"] == g5_set]
    if g5_rows:
        fx = ep.load_fixture_files(g5_fixtures)
        # Eval-time instrument binding (turn-7 adversarial review F-1): a
        # duplicated or unknown fixture row inflates every row-counting
        # endpoint below while the keyed join stays green, so both are
        # folded into `join.violations` (exit 2); missing rows only shrink
        # numerators and are reported.
        rows_check = ep.instrument_row_check([j.fixture["fixture"] for j in g5_rows], set(fx))
        report["instrument_rows"] = rows_check
        if rows_check["duplicates"] or rows_check["unknown"]:
            report["join"]["violations"] = list(report["join"]["violations"]) + [
                f"instrument rows: duplicates={rows_check['duplicates']} "
                f"unknown={rows_check['unknown']} for {g5_set}"
            ]
        patch = [j for j in g5_rows if j.fixture.get("expect") == "patch"]
        refuse = [j for j in g5_rows if j.fixture.get("expect") == "refuse"]
        g5 = {"set": g5_set,
              "patch": ep.leg(sum(bool(j.fixture["landed"]) for j in patch), len(patch)),
              "refuse": ep.leg(sum(bool(j.fixture["landed"]) for j in refuse), len(refuse))}
        jv = _journaled_g5(jrows, g5_set)
        g5["journaled_verdict_matches"] = bool(jv) and (
            (jv["patch_landed"], jv["patch_n"], jv["refuse_landed"], jv["refuse_n"],
             jv["patch_provisional"], jv["refuse_provisional"]) ==
            (g5["patch"]["landed"], g5["patch"]["n"], g5["refuse"]["landed"], g5["refuse"]["n"],
             g5["patch"]["provisional"], g5["refuse"]["provisional"]))
        report["g5"] = g5
        report["composition"] = ep.composition(g5_rows)
        report["endpoints"] = ep.endpoints(g5_rows, fx)
        # The v4 claim audit (turn-6 spec §2): pre-registered for
        # codec-tasks-v4-mixed ONLY -- its patterns and family inference
        # are that audit's, never generalized to another set here.
        if g5_set == "codec-tasks-v4-mixed":
            report["claim_audit"] = ep.claim_audit(g5_rows)
        # The three v5 declaration endpoints (turn-6 spec §5.2): computed
        # for the v5-mixed set only -- the family key and declared-done
        # contract are that set's.
        if g5_set == "codec-tasks-v5-mixed":
            report["declarations"] = ep.declarations(g5_rows, fx)
    else:
        report["g5"] = None
    report["grant_violation_rows"] = ep.grant_violation_rows(trows)
    report["verb_histogram"] = ep.verb_histogram(trows)
    return report


def main(argv=None) -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--journal", required=True, type=Path)
    ap.add_argument("--tasks", required=True, type=Path)
    ap.add_argument("--g5-fixtures", type=Path, default=None)
    ap.add_argument("--g4-set", default="codec-tasks-v1")
    ap.add_argument("--json", type=Path, default=None)
    a = ap.parse_args(argv)
    report = recompute(a.journal, a.tasks, a.g5_fixtures, a.g4_set)
    text = json.dumps(report, indent=2)
    if a.json:
        a.json.write_text(text + "\n")
    print(text)
    return 0 if not report["join"]["violations"] else 2


if __name__ == "__main__":
    raise SystemExit(main())
