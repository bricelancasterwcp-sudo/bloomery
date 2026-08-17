"""The contamination guard (design spec §3, brief rule 7; extended by G5
design doc §3: "the contamination guard runs against both gate sets from
now on").

Proves the generated corpus shares nothing with one or more frozen gate
sets: no shared goals, file contents, target filenames, or search strings
(exact or whitespace/case-normalized), and no near-duplicate goals (>=
0.8 Jaccard token-set similarity). This is the machine check the design
spec calls for: "a structural comparator proves zero overlap between the
corpus and codec-tasks-v1" — G5 widens the same comparator to cover
`codec-tasks-v2-mixed` too, rather than teaching it a second, parallel
rule set.

Two responsibilities live here:

1. ``GATE_VOCABULARY`` / ``load_gate_fixtures`` / ``GateFixture`` —
   gate-TOML parsing and the forbidden-word enumeration rule 1 needs.
   Split out into `gate_vocabulary.py` to keep this file under the
   400-line house cap (task 6a's `_violations_for_task`/
   `task_violates_gates` addition pushed it over budget); re-exported
   here so every existing `contamination.<name>` call site (this module's
   own CLI, `templates.py`, `gate_sampling.py`, and every test) keeps
   working unchanged. See `gate_vocabulary.py`'s own module docstring for
   the full rationale, including why it is deliberately scoped to
   `codec-tasks-v1` ONLY, never unioned with `codec-tasks-v2-mixed`.

2. ``check_corpus`` — the CLI's actual comparator, run over a generated
   corpus.jsonl against the union of every gate's parsed fixtures. This is
   the half the multi-``--gate`` CLI flag (below) parameterizes: unlike
   ``GATE_VOCABULARY``, `check_corpus` compares specific fixture CONTENT
   (goals/targets/contents/search), not raw vocabulary, so unioning
   `codec-tasks-v1` and `codec-tasks-v2-mixed` fixtures here has none of
   responsibility 1's paradox — it just means a generated corpus task can
   never coincide with either frozen set's actual authored content.

Both responsibility 2's rule set and its post-hoc `check_corpus` entry
point are unchanged by task 6a (gate-aware rejection sampling in the
generator, `gate_sampling.py`): the rule set itself now lives in a single
shared helper, ``_violations_for_task``, with ``check_corpus`` as one
caller (post-hoc, over every task in a finished corpus.jsonl) and the new
``task_violates_gates`` as the other (at generation time, over ONE
candidate before it is ever written to a corpus). This is factoring, not
new policy — the guard CLI's behavior, and every rule it enforces, is
untouched.

CLI: ``python3 -m tools.flywheel.factory.contamination --corpus
corpus.jsonl --gate crates/bloomery-daemon/fixtures/codec-tasks-v1.toml
--gate crates/bloomery-daemon/fixtures/codec-tasks-v2-mixed.toml --out
contamination-report.json`` — ``--gate`` is repeatable (checks run against
the union of every gate given); exits nonzero on ANY overlap with ANY of
them.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Iterable, Optional, Union

from tools.flywheel.factory.gate_vocabulary import (
    DEFAULT_GATE_PATH,
    GATE_VOCABULARY,
    GateFixture,
    load_gate_fixtures,
)
from tools.flywheel.factory.task import RefusalTask, Task

__all__ = [
    "DEFAULT_GATE_PATH",
    "GATE_VOCABULARY",
    "GateFixture",
    "load_gate_fixtures",
    "normalize",
    "token_set",
    "jaccard",
    "Report",
    "task_violates_gates",
    "check_corpus",
    "main",
]

_TOKEN_RE = re.compile(r"[a-z0-9]+")


def normalize(text: str) -> str:
    """Whitespace-collapsed, lowercased — the normalization rule 5's
    dedup key and rule 7's exact/normalized contamination checks share."""
    return " ".join(text.split()).lower()


def token_set(text: str) -> frozenset[str]:
    """Lowercased alphanumeric token set, for Jaccard similarity."""
    return frozenset(_TOKEN_RE.findall(text.lower()))


def jaccard(a: frozenset[str] | set[str], b: frozenset[str] | set[str]) -> float:
    """Token-set Jaccard similarity. Two empty sets are treated as
    identical (both contain nothing); one empty and one non-empty are
    treated as maximally dissimilar."""
    if not a and not b:
        return 1.0
    if not a or not b:
        return 0.0
    return len(a & b) / len(a | b)


@dataclass(frozen=True)
class Report:
    violations: list[dict]
    clean: bool
    corpus_tasks_checked: int
    gate_fixtures_checked: int


def _corpus_tasks_from_rows(rows: Iterable[dict]) -> dict[str, dict]:
    """Groups corpus.jsonl rows by task_id, keeping the first occurrence
    of each task's goal/target/target_contents/search (generate.py writes
    these on every one of a task's 3 pair rows; they are identical across
    the 3, so first-occurrence is sufficient and avoids relying on
    iteration order beyond "first seen wins" — the row order in the file
    is itself deterministic, written by generate.py in task order)."""
    tasks: dict[str, dict] = {}
    for row in rows:
        meta = row["meta"]
        task_id = meta["task_id"]
        if task_id not in tasks:
            tasks[task_id] = {
                "goal": meta["goal"],
                "target": meta["target"],
                "target_contents": meta["target_contents"],
                "search": meta["search"],
            }
    return tasks


def _violations_for_task(
    goal: str,
    target: str,
    target_contents: str,
    search: str,
    fixtures: Iterable[GateFixture],
    jaccard_threshold: float = 0.8,
) -> list[dict]:
    """The rule set itself (rule 7), extracted so it has exactly ONE
    implementation shared by two callers: `check_corpus` (post-hoc, over
    every task in a finished corpus.jsonl) and `task_violates_gates`
    (task 6a, at generation time, over ONE candidate before it is ever
    written to a corpus). Fails on any of: exact or normalized match of
    goals, file contents, target filenames, search strings; OR >=
    `jaccard_threshold` Jaccard token-set similarity between the goal and
    any gate goal. `task_id`/corpus-vs-candidate bookkeeping stays with
    each caller — this function only knows about the four scalar fields
    every rule compares, so a violation dict here never carries
    `task_id`."""
    violations: list[dict] = []
    goal_norm = normalize(goal)
    target_norm = normalize(target)
    search_norm = normalize(search)
    contents_norm = normalize(target_contents)
    goal_tokens = token_set(goal)

    for fixture in fixtures:
        if goal_norm == normalize(fixture.goal):
            violations.append(
                {
                    "rule": "goal_match",
                    "gate_fixture": fixture.name,
                    "detail": "corpus goal matches a gate goal (exact or normalized)",
                }
            )
        if target_norm == normalize(fixture.target):
            violations.append(
                {
                    "rule": "target_filename_match",
                    "gate_fixture": fixture.name,
                    "detail": f"corpus target {target!r} matches gate target {fixture.target!r}",
                }
            )
        if fixture.search is not None and search_norm == normalize(fixture.search):
            violations.append(
                {
                    "rule": "search_match",
                    "gate_fixture": fixture.name,
                    "detail": "corpus search string matches a gate reference search string",
                }
            )
        for gate_path, gate_contents in sorted(fixture.files.items()):
            if contents_norm == normalize(gate_contents):
                violations.append(
                    {
                        "rule": "file_contents_match",
                        "gate_fixture": fixture.name,
                        "gate_file": gate_path,
                        "detail": "corpus target file contents match a gate fixture file",
                    }
                )

        similarity = jaccard(goal_tokens, token_set(fixture.goal))
        if similarity >= jaccard_threshold:
            violations.append(
                {
                    "rule": "goal_near_duplicate",
                    "gate_fixture": fixture.name,
                    "jaccard": similarity,
                    "detail": f"corpus goal is a {similarity:.0%} token-set match to a gate goal",
                }
            )

    return violations


def task_violates_gates(
    task: Union[Task, RefusalTask], gates: list[GateFixture], jaccard_threshold: float = 0.8
) -> Optional[str]:
    """Screens ONE candidate task (`Task` or `RefusalTask`, task.py)
    against `gates` using the exact same rule set `check_corpus` applies
    to a finished corpus (`_violations_for_task`, above) — the generator's
    rejection sampler (task 6a, `gate_sampling.py`) calls this at DRAW
    time, before a candidate ever becomes a corpus row, so a colliding
    candidate can be dropped and redrawn rather than caught only after a
    full corpus is written. Returns the FIRST violated rule's name
    (`_violations_for_task` always checks fixtures/rules in the same
    order, so this is stable across identical inputs), or `None` if the
    candidate is clean against every gate. `gates=[]` always returns
    `None` — nothing to screen against, and no extra work done."""
    if not gates:
        return None

    if isinstance(task, RefusalTask):
        target_contents = "" if task.target_missing else task.files[task.target]
        search = ""
    else:
        target_contents = task.files[task.target]
        search = task.search

    violations = _violations_for_task(task.goal, task.target, target_contents, search, gates, jaccard_threshold)
    return violations[0]["rule"] if violations else None


def check_corpus(rows: Iterable[dict], fixtures: list[GateFixture], jaccard_threshold: float = 0.8) -> Report:
    """The comparator itself. Fails on any of: exact or normalized match
    of goals, file contents, target filenames, search strings; OR >=
    `jaccard_threshold` Jaccard token-set similarity between any corpus
    goal and any gate goal."""
    corpus_tasks = _corpus_tasks_from_rows(rows)
    violations: list[dict] = []

    for task_id in sorted(corpus_tasks):
        task = corpus_tasks[task_id]
        for violation in _violations_for_task(
            task["goal"], task["target"], task["target_contents"], task["search"], fixtures, jaccard_threshold
        ):
            violations.append({"task_id": task_id, **violation})

    return Report(
        violations=violations,
        clean=(len(violations) == 0),
        corpus_tasks_checked=len(corpus_tasks),
        gate_fixtures_checked=len(fixtures),
    )


def _read_jsonl(path: Path) -> list[dict]:
    rows = []
    with path.open(encoding="utf-8") as f:
        for line in f:
            line = line.strip()
            if line:
                rows.append(json.loads(line))
    return rows


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="Contamination guard: prove the generated corpus is disjoint from one or more gate sets."
    )
    parser.add_argument("--corpus", required=True, type=Path, help="Path to corpus.jsonl (generate.py's output).")
    parser.add_argument(
        "--gate",
        action="append",
        dest="gates",
        required=True,
        type=Path,
        metavar="PATH",
        help=(
            "Path to a gate TOML (e.g. codec-tasks-v1.toml). Repeatable — with the design doc "
            "§3 change ('the contamination guard runs against both gate sets from now on'), "
            "checks run against the UNION of every --gate given, so a plant in ANY one gate set "
            "is caught."
        ),
    )
    parser.add_argument("--out", required=True, type=Path, help="Path to write the contamination report JSON.")
    parser.add_argument("--jaccard-threshold", type=float, default=0.8, help="Near-duplicate goal similarity threshold (default 0.8).")
    args = parser.parse_args(argv)

    rows = _read_jsonl(args.corpus)
    fixtures: list[GateFixture] = []
    for gate_path in args.gates:
        fixtures.extend(load_gate_fixtures(gate_path))
    report = check_corpus(rows, fixtures, jaccard_threshold=args.jaccard_threshold)

    out_data = {
        "clean": report.clean,
        "corpus_tasks_checked": report.corpus_tasks_checked,
        "gate_fixtures_checked": report.gate_fixtures_checked,
        "violations": report.violations,
    }
    args.out.write_text(json.dumps(out_data, indent=2, sort_keys=True) + "\n", encoding="utf-8")

    gate_paths_str = ", ".join(str(g) for g in args.gates)
    if not report.clean:
        print(
            f"contamination guard: {len(report.violations)} violation(s) found against "
            f"[{gate_paths_str}] (see {args.out})",
            file=sys.stderr,
        )
        return 1
    print(
        f"contamination guard: clean — {report.corpus_tasks_checked} corpus task(s) checked against "
        f"{report.gate_fixtures_checked} gate fixture(s) across [{gate_paths_str}]",
        file=sys.stderr,
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
