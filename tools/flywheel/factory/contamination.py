"""The contamination guard (design spec §3, brief rule 7).

Proves the generated corpus shares nothing with the frozen G4 gate set
(`crates/bloomery-daemon/fixtures/codec-tasks-v1.toml`, "codec-tasks-v1"):
no shared goals, file contents, target filenames, or search strings
(exact or whitespace/case-normalized), and no near-duplicate goals (>=
0.8 Jaccard token-set similarity). This is the machine check the design
spec calls for: "a structural comparator proves zero overlap between the
corpus and codec-tasks-v1."

Two responsibilities live here:

1. ``GATE_VOCABULARY`` — the forbidden-word enumeration rule 1 needs.
   ``templates.py`` imports it and a test in ``test_templates.py`` asserts
   every template word list is disjoint from it. It is built from the
   REAL gate TOML (target filenames, filename stems, and every ``def``
   name it defines — parsed mechanically, not hand-transcribed, so it
   cannot silently drift out of sync) unioned with a hand-curated set of
   the gate set's other distinctive nouns, compound identifiers, domains,
   and version/date strings (things a filename/function-name scan alone
   would not surface, e.g. "bloomery", "ada", "listen_port",
   "example.com"). ``test_contamination.py`` mechanically checks the
   auto-derived half for completeness against the live TOML.

2. ``check_corpus`` — the CLI's actual comparator, run over a generated
   corpus.jsonl against the parsed gate fixtures.

CLI: ``python3 -m tools.flywheel.factory.contamination --corpus
corpus.jsonl --gate crates/bloomery-daemon/fixtures/codec-tasks-v1.toml
--out contamination-report.json`` — exits nonzero on ANY overlap.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
import tomllib
from dataclasses import dataclass
from pathlib import Path
from typing import Iterable

_REPO_ROOT = Path(__file__).resolve().parents[3]
DEFAULT_GATE_PATH = _REPO_ROOT / "crates" / "bloomery-daemon" / "fixtures" / "codec-tasks-v1.toml"

_DEF_NAME_RE = re.compile(r"\bdef\s+([A-Za-z_]\w*)")
_TOKEN_RE = re.compile(r"[a-z0-9]+")


@dataclass(frozen=True)
class GateFixture:
    """One `[[fixture]]` entry from the gate TOML."""

    name: str
    lens: str
    target: str
    files: dict[str, str]
    goal: str
    search: str
    replace: str


def load_gate_fixtures(path: Path) -> list[GateFixture]:
    """Parses the gate TOML (stdlib `tomllib`, per the brief) into
    structured fixtures, in file order (deterministic — TOML arrays
    preserve order; never a `set`)."""
    with open(path, "rb") as f:
        data = tomllib.load(f)
    fixtures = []
    for fx in data["fixture"]:
        files = {file_entry["path"]: file_entry["contents"] for file_entry in fx["file"]}
        fixtures.append(
            GateFixture(
                name=fx["name"],
                lens=fx["lens"],
                target=fx["target"],
                files=files,
                goal=fx["goal"],
                search=fx["reference"]["search"],
                replace=fx["reference"]["replace"],
            )
        )
    return fixtures


def _extract_filenames_and_stems(fixtures: Iterable[GateFixture]) -> frozenset[str]:
    words: set[str] = set()
    for fx in fixtures:
        words.add(fx.target.lower())
        words.add(fx.target.split(".")[0].lower())
    return frozenset(words)


def _extract_function_names(fixtures: Iterable[GateFixture]) -> frozenset[str]:
    names: set[str] = set()
    for fx in fixtures:
        for contents in fx.files.values():
            names.update(m.group(1).lower() for m in _DEF_NAME_RE.finditer(contents))
    return frozenset(names)


# ---------------------------------------------------------------------------
# Hand-curated additions: the gate set's other distinctive vocabulary that
# a filename/function-name scan does not surface on its own — thematic
# nouns tied to a single fixture's premise, config/identifier keys that
# are not `def` names, domains/emails, and the specific version/date
# strings used in the changelog/release-notes fixtures. Each is banned as
# a whole token (an exact identifier or word), not a substring — e.g.
# "listen_port" is forbidden, but the generic word "port" that rule 1's
# own "port/host mismatch" family needs is not.
# ---------------------------------------------------------------------------

_EXTRA_DOMAIN_NOUNS = frozenset(
    {
        "cart", "billing", "shipping", "inventory", "restock", "reorder",
        "discontinued", "discount", "discounted", "pricing", "subtotal",
        "membership", "member", "password", "validator", "signup",
        "register", "username", "secret", "welcome", "acme", "corp",
        "support", "upstream", "proxy", "backend", "worker", "retries",
        "concurrency", "queue", "readme", "npm", "changelog", "settings",
        "health", "threshold", "tax", "stock", "valid", "free", "db",
        "release", "bloomery", "ada", "best",
    }
)

_EXTRA_COMPOUND_IDENTIFIERS = frozenset(
    {
        "listen_addr", "listen_port", "health_path", "db_host", "db_port",
        "db_name", "db_user", "health_check_url", "http_timeout_ms",
        "max_retries", "log_level", "upstream_url", "proxy_pass",
        "max_discount_pct",
    }
)

_EXTRA_DOMAINS_AND_EMAILS = frozenset(
    {
        "example.com", "example.org", "billing@example.org",
        "billing@example.com", "api.internal.example.com",
        "support.bloomery.example.com",
    }
)

_EXTRA_VERSION_AND_DATE_STRINGS = frozenset(
    {"2.4.0", "2.4.1", "3.1.0", "v3.1.0", "2026-02-30", "2026-02-28", "2026-07-18", "2026-08-02"}
)

_gate_fixtures_for_vocabulary = load_gate_fixtures(DEFAULT_GATE_PATH)

GATE_VOCABULARY: frozenset[str] = frozenset().union(
    _extract_filenames_and_stems(_gate_fixtures_for_vocabulary),
    _extract_function_names(_gate_fixtures_for_vocabulary),
    _EXTRA_DOMAIN_NOUNS,
    _EXTRA_COMPOUND_IDENTIFIERS,
    _EXTRA_DOMAINS_AND_EMAILS,
    _EXTRA_VERSION_AND_DATE_STRINGS,
)
"""Every target filename (and stem), function name, and distinctive
domain noun the gate set uses, lowercased. Rule 1's forbidden list."""


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


def check_corpus(rows: Iterable[dict], fixtures: list[GateFixture], jaccard_threshold: float = 0.8) -> Report:
    """The comparator itself. Fails on any of: exact or normalized match
    of goals, file contents, target filenames, search strings; OR >=
    `jaccard_threshold` Jaccard token-set similarity between any corpus
    goal and any gate goal."""
    corpus_tasks = _corpus_tasks_from_rows(rows)
    violations: list[dict] = []

    for task_id in sorted(corpus_tasks):
        task = corpus_tasks[task_id]
        goal_norm = normalize(task["goal"])
        target_norm = normalize(task["target"])
        search_norm = normalize(task["search"])
        contents_norm = normalize(task["target_contents"])
        goal_tokens = token_set(task["goal"])

        for fixture in fixtures:
            if goal_norm == normalize(fixture.goal):
                violations.append(
                    {
                        "rule": "goal_match",
                        "task_id": task_id,
                        "gate_fixture": fixture.name,
                        "detail": "corpus goal matches a gate goal (exact or normalized)",
                    }
                )
            if target_norm == normalize(fixture.target):
                violations.append(
                    {
                        "rule": "target_filename_match",
                        "task_id": task_id,
                        "gate_fixture": fixture.name,
                        "detail": f"corpus target {task['target']!r} matches gate target {fixture.target!r}",
                    }
                )
            if search_norm == normalize(fixture.search):
                violations.append(
                    {
                        "rule": "search_match",
                        "task_id": task_id,
                        "gate_fixture": fixture.name,
                        "detail": "corpus search string matches a gate reference search string",
                    }
                )
            for gate_path, gate_contents in sorted(fixture.files.items()):
                if contents_norm == normalize(gate_contents):
                    violations.append(
                        {
                            "rule": "file_contents_match",
                            "task_id": task_id,
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
                        "task_id": task_id,
                        "gate_fixture": fixture.name,
                        "jaccard": similarity,
                        "detail": f"corpus goal is a {similarity:.0%} token-set match to a gate goal",
                    }
                )

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
    parser = argparse.ArgumentParser(description="Contamination guard: prove the generated corpus is disjoint from codec-tasks-v1.")
    parser.add_argument("--corpus", required=True, type=Path, help="Path to corpus.jsonl (generate.py's output).")
    parser.add_argument("--gate", required=True, type=Path, help="Path to the gate TOML (codec-tasks-v1.toml).")
    parser.add_argument("--out", required=True, type=Path, help="Path to write the contamination report JSON.")
    parser.add_argument("--jaccard-threshold", type=float, default=0.8, help="Near-duplicate goal similarity threshold (default 0.8).")
    args = parser.parse_args(argv)

    rows = _read_jsonl(args.corpus)
    fixtures = load_gate_fixtures(args.gate)
    report = check_corpus(rows, fixtures, jaccard_threshold=args.jaccard_threshold)

    out_data = {
        "clean": report.clean,
        "corpus_tasks_checked": report.corpus_tasks_checked,
        "gate_fixtures_checked": report.gate_fixtures_checked,
        "violations": report.violations,
    }
    args.out.write_text(json.dumps(out_data, indent=2, sort_keys=True) + "\n", encoding="utf-8")

    if not report.clean:
        print(
            f"contamination guard: {len(report.violations)} violation(s) found against {args.gate} "
            f"(see {args.out})",
            file=sys.stderr,
        )
        return 1
    print(
        f"contamination guard: clean — {report.corpus_tasks_checked} corpus task(s) checked against "
        f"{report.gate_fixtures_checked} gate fixture(s)",
        file=sys.stderr,
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
