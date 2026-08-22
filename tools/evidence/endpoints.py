"""Gate arithmetic and the secondary endpoints (g5v4 protocol §4-§5).

Wilson 95% is a verbatim port of crates/bloomery-core/src/stats.rs;
`is_provisional` is scoring.rs's strict two-sided straddle; `gate_decision`
is `landed*5 >= n*4`.
"""
from __future__ import annotations

import math
import re
import tomllib
from collections import Counter
from pathlib import Path

from .journal import Joined

Z = 1.959963984540054
THRESHOLD = 0.80
SPAN = re.compile(r"`([^`]+)`")
RAN_EXIT0 = re.compile(r"^ran .* exit 0$")

PATCH_SHAPES = ("find", "run", "plain")
REFUSE_FAMILIES = ("defect-absent", "missing-target", "symptom-mismatch")


def wilson95(passes: int, n: int) -> tuple[float, float]:
    if n == 0:
        return (0.0, 1.0)
    phat = passes / n
    denom = 1.0 + Z * Z / n
    centre = phat + Z * Z / (2.0 * n)
    margin = Z * math.sqrt((phat * (1.0 - phat) + Z * Z / (4.0 * n)) / n)
    return (max((centre - margin) / denom, 0.0), min((centre + margin) / denom, 1.0))


def is_provisional(lo: float, hi: float) -> bool:
    return lo < THRESHOLD < hi


def gate_decision(landed: int, n: int) -> bool:
    return landed * 5 >= n * 4


def leg(landed: int, n: int) -> dict:
    lo, hi = wilson95(landed, n)
    return {"landed": landed, "n": n, "wilson95": [lo, hi],
            "provisional": is_provisional(lo, hi), "pass": gate_decision(landed, n)}


def shape_of(name: str) -> str | None:
    for s in PATCH_SHAPES:
        if f"-patch-{s}-" in name:
            return s
    for fam in REFUSE_FAMILIES:
        if fam in name:
            return fam
    return None


def composition(rows: list[Joined]) -> dict[str, list[int]]:
    out = {k: [0, 0] for k in PATCH_SHAPES + REFUSE_FAMILIES}
    for j in rows:
        s = shape_of(j.fixture["fixture"])
        if s is None:
            continue
        out[s][1] += 1
        out[s][0] += int(bool(j.fixture["landed"]))
    return out


def verbs(j: Joined) -> list[str]:
    return [s["verb"] for s in j.steps]


def load_fixture_files(toml_path: Path) -> dict[str, dict]:
    doc = tomllib.loads(Path(toml_path).read_text())
    return {fx["name"]: fx for fx in doc["fixture"]}


def reason_grounding(rows: list[Joined], fixtures: dict[str, dict]) -> dict:
    eligible = [j for j in rows if j.fixture.get("expect") == "refuse"
                and "missing-target" not in j.fixture["fixture"]]
    landed = [j for j in eligible if j.fixture["landed"]]
    measured = unmeasured = grounded = spans = 0
    for j in landed:
        done = [s for s in j.steps if s["verb"] == "done"]
        text = done[-1]["outcome"] if done else ""
        found = SPAN.findall(text)
        if not found:
            unmeasured += 1
            continue
        measured += 1
        fx = fixtures[j.fixture["fixture"]]
        contents = [f.get("contents", "") for f in fx.get("file", [])]
        paths = [f.get("path", "") for f in fx.get("file", [])]
        for span in found:
            spans += 1
            if any(span in c for c in contents) or any(span in p for p in paths):
                grounded += 1
    return {"eligible": len(eligible), "landed_eligible": len(landed), "measured_rows": measured,
            "unmeasured_rows": unmeasured, "grounded": grounded, "spans": spans}


def endpoints(rows: list[Joined], fixtures: dict[str, dict]) -> dict:
    find_rows = [j for j in rows if shape_of(j.fixture["fixture"]) == "find"]
    run_rows = [j for j in rows if shape_of(j.fixture["fixture"]) == "run"]

    def productive_find(j): return "find" in verbs(j) and bool(j.fixture["landed"])
    def find_usage(j): return "find" in verbs(j)
    def malformed(j): return "?" in verbs(j)
    def run_before_done(j):
        v = verbs(j)
        return "run" in v and "done" in v and v.index("run") < len(v) - 1 - v[::-1].index("done")
    def any_run(j): return "run" in verbs(j)
    def productive_run(j):
        return bool(j.fixture["landed"]) and any(s["verb"] == "run" and RAN_EXIT0.match(s["outcome"]) for s in j.steps)

    return {
        "productive_find": [sum(map(productive_find, find_rows)), len(find_rows)],
        "find_usage": [sum(map(find_usage, find_rows)), len(find_rows)],
        "malformed_find": [sum(map(malformed, find_rows)), len(find_rows)],
        "run_before_done": [sum(map(run_before_done, run_rows)), len(run_rows)],
        "any_run": [sum(map(any_run, run_rows)), len(run_rows)],
        "productive_run": [sum(map(productive_run, run_rows)), len(run_rows)],
        "reason_grounding": reason_grounding(rows, fixtures),
    }


def grant_violation_rows(tasks: list[dict]) -> int:
    return sum(1 for r in tasks if r.get("event") == "TaskStep" and str(r.get("outcome", "")).startswith("grant violation"))


def verb_histogram(tasks: list[dict]) -> dict[str, int]:
    return dict(sorted(Counter(r["verb"] for r in tasks if r.get("event") == "TaskStep").items()))
