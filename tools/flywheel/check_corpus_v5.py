"""The structural corpus check (turn-7 spec §2.4) — the black-oxide
lesson made executable ("falsifiable is not sufficient": endpoints can
pass over a drifted corpus). Runs over every row of a generated v5
corpus.jsonl in seconds, BEFORE the pre-registration commit, and is
quoted in it; any violation → exit 2, nothing quotable.

CLI: ``python3 -m tools.flywheel.check_corpus_v5 --corpus PATH
[--json OUT]``

Five rules, every violation collected (all counted, the first 20
reported verbatim):

1. every row's `meta.envelope` is `"v5"` — a mixed or v4 corpus fails;
2. only `pair == "done"` rows carry the declared ideal: every other
   pair's completion must NOT start with `<action verb="done"`;
3. every done completion matches `done_v5`'s EXACT emitted shape (this
   checks OUR generator's authored output, not model output): the
   declared opening line, ≥ 1 leading `evidence: ` line, non-empty
   prose, the bare closing tag, and an (outcome, reason) pairing valid
   per `task.DONE_V5_OUTCOMES`;
4. outcome matches the row's class: `expect == "patch"` ⇒
   `patched`/`fixed`; `expect == "refuse"` ⇒ `refused` with reason
   `generate_envelope_v5.FAMILY_TO_REASON[meta.family]` — a refuse row
   with no `family` key is itself a violation, never an inferred family
   (the same rule the declaration endpoint enforces for fixtures);
5. every evidence line classifies EXACTLY `grounded` under the shipped
   endpoint's own `endpoints._classify_evidence_line` — imported, never
   copied (one implementation; the underscore is convention, and the
   reuse is the spec's explicit instruction) — against `meta.files` for
   refuse rows and the POST-patch bytes for patch rows, recomputed as
   `files[target] = target_contents.replace(search, replace, 1)`; a
   patch row with no `meta.replace` is itself a violation.

The JSON summary (stdout, and `--json OUT`) carries row/task/done-row
counts, per-expect / per-refuse-family / per-patch-trajectory task
counts, the violation count, and the bounded violation list — corpus
counts in any document come from here and the generator's fingerprint,
never memory.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from collections import Counter
from typing import Iterator

from tools.evidence.endpoints import _classify_evidence_line
from tools.flywheel.factory.generate_envelope_v5 import FAMILY_TO_REASON
from tools.flywheel.factory.task import DONE_V5_OUTCOMES

V5 = "v5"
DONE_PREFIX = '<action verb="done"'
EVIDENCE_PREFIX = "evidence: "
CLOSING_TAG = "</action>"
MAX_REPORTED = 20

_OPEN_RE = re.compile(r'<action verb="done" outcome="([^"]*)" reason="([^"]*)">')


def _done_shape(completion: str) -> tuple[str, str, list[str], str]:
    """(outcome, reason, evidence lines, prose) of a completion in
    `done_v5`'s exact emitted shape, or ValueError naming the missed
    requirement — the rule-3 parse, strict on the whole string (no
    trailing bytes after the closing tag, no wrapping whitespace)."""
    lines = completion.split("\n")
    opened = _OPEN_RE.fullmatch(lines[0])
    if opened is None or lines[-1] != CLOSING_TAG:
        raise ValueError(
            "completion is not done_v5's emitted shape "
            '(`<action verb="done" outcome="..." reason="...">` ... `</action>`)'
        )
    outcome, reason = opened.group(1), opened.group(2)
    body = lines[1:-1]
    evidence = []
    for line in body:
        if not line.startswith(EVIDENCE_PREFIX):
            break
        evidence.append(line)
    if not evidence:
        raise ValueError("no leading 'evidence: ' line")
    prose = "\n".join(body[len(evidence):])
    if not prose.strip():
        raise ValueError("prose after the evidence lines is empty")
    if reason not in DONE_V5_OUTCOMES.get(outcome, ()):
        raise ValueError(
            f"(outcome, reason) = ({outcome!r}, {reason!r}) is not a valid "
            f"DONE_V5_OUTCOMES pairing"
        )
    return outcome, reason, evidence, prose


def _class_violations(meta: dict, outcome: str, reason: str) -> list[str]:
    """The rule-4 outcome⇔class violations for one done row."""
    expect = meta.get("expect")
    if expect == "patch":
        out = []
        if outcome != "patched":
            out.append(f'patch row declares outcome {outcome!r}, must be "patched"')
        if reason != "fixed":
            out.append(f'patch row declares reason {reason!r}, must be "fixed"')
        return out
    if expect == "refuse":
        out = []
        if outcome != "refused":
            out.append(f'refuse row declares outcome {outcome!r}, must be "refused"')
        if "family" not in meta:
            out.append(
                "refuse row has no meta.family key -- the family is never inferred "
                "from a template name"
            )
            return out
        family = meta["family"]
        expected = FAMILY_TO_REASON.get(family)
        if expected is None:
            out.append(
                f"refuse row carries unknown family {family!r} "
                f"(valid: {sorted(FAMILY_TO_REASON)})"
            )
        elif reason != expected:
            out.append(
                f"refuse row of family {family!r} declares reason {reason!r}, "
                f"must be {expected!r}"
            )
        return out
    return [f"expect is {expect!r}, must be 'patch' or 'refuse'"]


def _evidence_files(meta: dict) -> dict[str, str]:
    """The byte ground truth rule 5 classifies against: `meta.files` for a
    refuse row, the POST-patch bytes for a patch row (a truthful `fixed`
    quotes the patched file — the endpoint's own post-`reference` rule)."""
    if "files" not in meta:
        raise ValueError("row has no meta.files; evidence cannot be classified")
    files = dict(meta["files"])
    if meta.get("expect") != "patch":
        return files
    if "replace" not in meta:
        raise ValueError(
            "patch row has no meta.replace; the post-patch bytes cannot be recomputed"
        )
    try:
        target = meta["target"]
        files[target] = meta["target_contents"].replace(meta["search"], meta["replace"], 1)
    except KeyError as exc:
        raise ValueError(
            f"patch row has no meta.{exc.args[0]}; the post-patch bytes cannot be recomputed"
        )
    return files


def _check_done_row(label: str, meta: dict, completion: str, violations: list[str]) -> None:
    try:
        outcome, reason, evidence, _prose = _done_shape(completion)
    except ValueError as exc:
        violations.append(f"{label}: rule 3 -- {exc}")
        return
    violations.extend(f"{label}: rule 4 -- {text}" for text in _class_violations(meta, outcome, reason))
    try:
        files = _evidence_files(meta)
    except ValueError as exc:
        violations.append(f"{label}: rule 5 -- {exc}")
        return
    for line in evidence:
        verdict = _classify_evidence_line(line, files, reason)
        if verdict != "grounded":
            violations.append(
                f"{label}: rule 5 -- evidence line classifies {verdict!r}, "
                f"must be 'grounded': {line}"
            )


def _iter_rows(path: str, violations: list[str]) -> Iterator[tuple[int, dict]]:
    with open(path, encoding="utf-8") as handle:
        for number, line in enumerate(handle, 1):
            if not line.strip():
                continue
            try:
                yield number, json.loads(line)
            except json.JSONDecodeError as exc:
                violations.append(f"row {number}: not valid JSON ({exc})")


def _register_task(tasks: dict[str, dict], meta: dict) -> None:
    task_id = meta.get("task_id")
    if task_id is None:
        return
    info = tasks.setdefault(task_id, {})
    for key in ("expect", "family", "trajectory"):
        if key in meta and key not in info:
            info[key] = meta[key]


def _summary(rows: int, done_rows: int, tasks: dict[str, dict], violations: list[str]) -> dict:
    by_expect = Counter(info.get("expect", "(missing)") for info in tasks.values())
    refuse_by_family = Counter(
        info.get("family", "(missing)")
        for info in tasks.values()
        if info.get("expect") == "refuse"
    )
    patch_by_trajectory = Counter(
        info.get("trajectory", "(missing)")
        for info in tasks.values()
        if info.get("expect") == "patch"
    )
    return {
        "rows": rows,
        "tasks": len(tasks),
        "done_rows": done_rows,
        "tasks_by_expect": dict(sorted(by_expect.items())),
        "refuse_tasks_by_family": dict(sorted(refuse_by_family.items())),
        "patch_tasks_by_trajectory": dict(sorted(patch_by_trajectory.items())),
        "violations": len(violations),
        "violations_reported": violations[:MAX_REPORTED],
    }


def check_corpus(path: str) -> dict:
    """All five rules over every row of `path`, violations collected (never
    fail-fast: the report shows the corpus's whole failure surface, bounded
    to `MAX_REPORTED` verbatim entries, all counted)."""
    violations: list[str] = []
    tasks: dict[str, dict] = {}
    row_count = 0
    done_rows = 0
    for number, row in _iter_rows(path, violations):
        row_count += 1
        meta = row.get("meta")
        if not isinstance(meta, dict):
            violations.append(f"row {number}: no meta object")
            continue
        _register_task(tasks, meta)
        label = f"row {number} (task {meta.get('task_id')!r}, pair {meta.get('pair')!r})"
        if meta.get("envelope") != V5:
            violations.append(
                f"{label}: rule 1 -- meta.envelope is {meta.get('envelope')!r}, "
                f'every row of a v5 corpus must carry envelope "v5"'
            )
        completion = str(row.get("completion", ""))
        if meta.get("pair") != "done":
            if completion.startswith(DONE_PREFIX):
                violations.append(
                    f"{label}: rule 2 -- non-done pair completion starts with the "
                    f"declared done block"
                )
            continue
        done_rows += 1
        _check_done_row(label, meta, completion, violations)
    return _summary(row_count, done_rows, tasks, violations)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        prog="python3 -m tools.flywheel.check_corpus_v5",
        description="Structural check of a generated v5 corpus (turn-7 spec §2.4): "
        "exit 0 clean, exit 2 on any violation.",
    )
    parser.add_argument("--corpus", required=True, metavar="PATH", help="corpus.jsonl to check")
    parser.add_argument("--json", metavar="OUT", help="also write the JSON summary to this file")
    args = parser.parse_args(argv)
    summary = check_corpus(args.corpus)
    rendered = json.dumps(summary, indent=2, sort_keys=True)
    print(rendered)
    if args.json:
        with open(args.json, "w", encoding="utf-8") as handle:
            handle.write(rendered + "\n")
    return 0 if summary["violations"] == 0 else 2


if __name__ == "__main__":
    sys.exit(main())
