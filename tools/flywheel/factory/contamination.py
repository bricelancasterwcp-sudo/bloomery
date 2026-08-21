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
candidate before it is ever written to a corpus). That was factoring, not
new policy.

Turn 3 widens the CONTENTS rule (both callers at once, since both go
through the one shared helper): every file a task carries is compared
against every gate fixture file, not just the declared target's contents.
A sibling file that is a verbatim copy of a gate fixture file is a
contamination too, and turn 3's multi-file repair tasks render siblings
into real training pairs. Corpus rows carry a `files` map for exactly
this reason; a row predating it falls back to target-only.

Turn 4 widens the FILENAME rule the same way, through the same shared
helper (turn-4 spec §3's ride-along — "the last gap in that rule"): every
name a task carries is screened against every gate target, not just the
declared one. Turn 4's run slice plants a `test_<stem>.py` sibling into
every run-verified task, so a rule that only ever looked at the target was
about to be blind to a file on the corpus's main path.

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
    of each task's goal/target/files/search (generate.py writes these on
    every one of a task's 3 pair rows; they are identical across the 3, so
    first-occurrence is sufficient and avoids relying on iteration order
    beyond "first seen wins" — the row order in the file is itself
    deterministic, written by generate.py in task order).

    `meta["files"]` (every file the task carries, not just the declared
    target) is what lets the post-hoc guard screen SIBLING files. A row
    written before that key existed is a legacy row: fall back to
    `{target: target_contents}` so an older corpus.jsonl still gets its
    target checked rather than silently skipping the contents rule."""
    tasks: dict[str, dict] = {}
    for row in rows:
        meta = row["meta"]
        task_id = meta["task_id"]
        if task_id not in tasks:
            files = meta.get("files")
            if files is None:
                files = {meta["target"]: meta["target_contents"]}
            tasks[task_id] = {
                "goal": meta["goal"],
                "target": meta["target"],
                "files": files,
                "search": meta["search"],
            }
    return tasks


def _violations_for_task(
    goal: str,
    target: str,
    files: dict[str, str],
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
    each caller — this function only knows about the fields every rule
    compares, so a violation dict here never carries `task_id`.

    Both the contents rule and (since turn 4) the FILENAME rule are full
    cross products over `files`; see `names_norm` below for why the
    filename rule was the last one still looking at the declared target
    alone.

    `files` is EVERY file the task carries, not just the declared
    target's contents: the gate side has always iterated `fixture.files`,
    and the corpus side now matches it, so the contents rule is a full
    cross product. A task whose sibling file is a verbatim copy of a gate
    fixture file is a contamination just as surely as one whose target
    is, and Task 7's multi-file repair tasks render siblings into real
    training pairs. An empty `files` is legal (nothing to compare) and
    leaves the goal/target/search rules running unchanged."""
    violations: list[dict] = []
    goal_norm = normalize(goal)
    search_norm = normalize(search)
    files_norm = {path: normalize(contents) for path, contents in sorted(files.items())}
    goal_tokens = token_set(goal)
    # Turn 4's ride-along (turn-4 spec §3: "the contamination guard screens
    # sibling FILENAMES against gate targets -- the last gap in that rule").
    # Every name the task carries is screened, not only the declared target:
    # a novel target does not make a sibling named after a gate fixture's
    # file any less of a collision, and turn 4's run slice plants a sibling
    # (`test_<stem>.py`) into every run-verified task, so the gap moved onto
    # the corpus's main path. The declared `target` stays in the set even
    # though it is normally a key of `files`, because for a missing-target
    # refusal task it is by construction NOT one -- dropping it there would
    # silently narrow the rule while widening it everywhere else.
    names_norm = {name: normalize(name) for name in sorted({target, *files})}

    for fixture in fixtures:
        if goal_norm == normalize(fixture.goal):
            violations.append(
                {
                    "rule": "goal_match",
                    "gate_fixture": fixture.name,
                    "detail": "corpus goal matches a gate goal (exact or normalized)",
                }
            )
        fixture_target_norm = normalize(fixture.target)
        for name, name_norm in names_norm.items():
            if name_norm == fixture_target_norm:
                violations.append(
                    {
                        "rule": "target_filename_match",
                        "gate_fixture": fixture.name,
                        "corpus_file": name,
                        "detail": f"corpus file {name!r} matches gate target {fixture.target!r}",
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
            gate_norm = normalize(gate_contents)
            for corpus_path, corpus_norm in files_norm.items():
                if corpus_norm == gate_norm:
                    violations.append(
                        {
                            "rule": "file_contents_match",
                            "gate_fixture": fixture.name,
                            "gate_file": gate_path,
                            "corpus_file": corpus_path,
                            "detail": f"corpus file {corpus_path!r} contents match a gate fixture file",
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
    `None` — nothing to screen against, and no extra work done.

    `task.files` goes in whole, so every file the candidate carries is
    screened, not just the declared target's contents. A missing-target
    `RefusalTask` therefore has its real SIBLING files screened (there is
    no target file to screen — `files` never contains the target); a
    multi-file repair task has its companion files screened too."""
    if not gates:
        return None

    search = "" if isinstance(task, RefusalTask) else task.search
    violations = _violations_for_task(task.goal, task.target, task.files, search, gates, jaccard_threshold)
    return violations[0]["rule"] if violations else None


def check_corpus(rows: Iterable[dict], fixtures: list[GateFixture], jaccard_threshold: float = 0.8) -> Report:
    """The comparator itself. Fails on any of: exact or normalized match
    of goals, the contents of ANY file a task carries (target or sibling),
    the NAME of any file a task carries against any gate target, search
    strings; OR >= `jaccard_threshold` Jaccard token-set similarity between
    any corpus goal and any gate goal."""
    corpus_tasks = _corpus_tasks_from_rows(rows)
    violations: list[dict] = []

    for task_id in sorted(corpus_tasks):
        task = corpus_tasks[task_id]
        for violation in _violations_for_task(
            task["goal"], task["target"], task["files"], task["search"], fixtures, jaccard_threshold
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
