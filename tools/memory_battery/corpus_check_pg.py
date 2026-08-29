"""The premise-gone-battery structural checker (design spec
`docs/superpowers/specs/2026-08-28-premise-gone-battery-v1-design.md` §3,
S1-S5; plan Task 2).

The black-oxide rule, applied to THIS corpus's own claims: nothing the
manifest asserts about `pristine_p2/` is trusted — every property is
executed. S1/S2 (fails-before, passes-after) and the base sha/family
checks are `corpus_check.check_corpus`'s own, run verbatim over the same
directory shape (checker-side reuse is legitimate independence: that
module's sha formula is already the deliberate from-scratch duplicate of
the generator's — the independence requirement binds checker-vs-
GENERATOR, not checker-vs-checker). This module adds the pg-only checks:

- **S3 goal-satisfied start**: `pristine_p2/`'s target byte-identical to
  `pristine/`'s (the fingerprint-match property the whole lane rests
  on); its test file NOT byte-identical (the world actually moved); and
  the moved-on test exits **0** on the p2 files — executed via
  `planted_test.run_python`, the same child-process environment shape
  every other executed check uses.
- **S4 non-vacuity**: the moved-on test against the FIXED target
  (pristine target with `search`->`replace` applied exactly once) exits
  **nonzero** — the world genuinely moved on, the stored patch is now
  wrong, and a test weakened into accepting anything is caught here.
- **S5 p2 sha**: `workspace_p2_sha256` recomputed with the checker-side
  independent formula and required to equal the manifest's value.

Corpus-level: the manifest must name THIS instrument
(`premise-gone-battery-v1`) — a pg checker silently passing a corpus-v1
manifest (which has no `pristine_p2` at all) would report vacuous
success, so the instrument mismatch is a named corpus failure, not a
skip. Guard discipline (per-task `_safe_check`, corpus-level named
failures, CLI verdict-never-traceback) mirrors `corpus_check.py`.
"""

from __future__ import annotations

import argparse
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Sequence

from tools.flywheel.factory.planted_test import run_python
from tools.memory_battery.corpus_check import (
    CheckReport,
    _independent_workspace_sha256,
    _load_files,
    _load_manifest,
    _safe_check,
    check_corpus,
    format_report,
)
from tools.memory_battery.corpus_pg import INSTRUMENT_PG


@dataclass(frozen=True)
class PgTaskCheckResult:
    """One task's verdict across the pg-only checks S3-S5."""

    name: str
    s3_ok: bool
    s3_detail: str
    s4_ok: bool
    s4_detail: str
    s5_ok: bool
    s5_detail: str

    @property
    def ok(self) -> bool:
        return self.s3_ok and self.s4_ok and self.s5_ok


@dataclass(frozen=True)
class PgCheckReport:
    """The full pg verdict: the base checker's report (S1/S2/base-sha/
    families, verbatim) plus one `PgTaskCheckResult` per task and any
    pg-specific corpus-level failures. `ok` ANDs everything."""

    base: CheckReport
    pg_results: list[PgTaskCheckResult]
    corpus_failures: list[str]

    @property
    def ok(self) -> bool:
        return (
            self.base.ok
            and not self.corpus_failures
            and all(result.ok for result in self.pg_results)
        )


def _check_s3(task_dir: Path, entry: dict[str, Any]) -> tuple[bool, str]:
    pristine_files = _load_files(task_dir / "pristine")
    p2_files = _load_files(task_dir / "pristine_p2")
    target = entry["target"]
    test_file = entry["test_file"]

    problems: list[str] = []
    if p2_files.get(target) != pristine_files.get(target):
        problems.append(f"pristine_p2 target {target!r} is not byte-identical to pristine's")
    if p2_files.get(test_file) == pristine_files.get(test_file):
        problems.append(f"pristine_p2 test {test_file!r} is byte-identical to pristine's -- nothing moved on")
    if problems:
        return False, "; ".join(problems)

    result = run_python(p2_files, entry["run_argv"])
    if result.returncode != 0:
        return False, (
            f"moved-on test exited {result.returncode} on the pristine_p2 files (expected 0 -- "
            f"the goal-satisfied start is the corpus's whole premise)\n{result.stdout}"
        )
    return True, ""


def _check_s4(task_dir: Path, entry: dict[str, Any]) -> tuple[bool, str]:
    pristine_files = _load_files(task_dir / "pristine")
    p2_files = _load_files(task_dir / "pristine_p2")
    target = entry["target"]
    search = entry["search"]

    if pristine_files.get(target, "").count(search) != 1:
        return False, f"search does not occur exactly once in pristine {target!r} -- cannot build the fixed target"

    fixed_files = dict(p2_files)
    fixed_files[target] = pristine_files[target].replace(search, entry["replace"], 1)
    result = run_python(fixed_files, entry["run_argv"])
    if result.returncode == 0:
        return False, (
            "moved-on test still passes on the fixed target (expected nonzero) -- the test no "
            "longer discriminates; the world did not genuinely move on"
        )
    return True, ""


def _check_s5(task_dir: Path, entry: dict[str, Any]) -> tuple[bool, str]:
    p2_files = _load_files(task_dir / "pristine_p2")
    recomputed = _independent_workspace_sha256(p2_files)
    expected = entry["workspace_p2_sha256"]
    if recomputed != expected:
        return False, f"pristine_p2 sha256 {recomputed} != manifest workspace_p2_sha256 {expected}"
    return True, ""


def _check_pg_task(corpus_dir: Path, entry: dict[str, Any]) -> PgTaskCheckResult:
    name = entry.get("name", "<unnamed task>")
    try:
        task_dir = (corpus_dir / entry["workspace"]).parent
    except Exception as exc:  # noqa: BLE001 -- any failure here must be recorded, never raised
        detail = f"could not resolve task directory: {exc!r}"
        return PgTaskCheckResult(name, False, detail, False, detail, False, detail)

    s3_ok, s3_detail = _safe_check(_check_s3, task_dir, entry)
    s4_ok, s4_detail = _safe_check(_check_s4, task_dir, entry)
    s5_ok, s5_detail = _safe_check(_check_s5, task_dir, entry)
    return PgTaskCheckResult(name, s3_ok, s3_detail, s4_ok, s4_detail, s5_ok, s5_detail)


def check_corpus_pg(corpus_dir: Path) -> PgCheckReport:
    """Runs the base checks plus S3-S5 against the corpus at
    `corpus_dir`. Never raises; every failure is a named entry somewhere
    in the returned report (the base checker's own guard discipline,
    extended)."""
    corpus_dir = Path(corpus_dir)
    base = check_corpus(corpus_dir)

    corpus_failures: list[str] = []
    pg_results: list[PgTaskCheckResult] = []

    manifest, load_error = _load_manifest(corpus_dir)
    if not load_error and manifest is not None:
        instrument = manifest.get("instrument")
        if instrument != INSTRUMENT_PG:
            corpus_failures.append(
                f"manifest instrument {instrument!r} != {INSTRUMENT_PG!r} -- this checker's "
                f"S3-S5 are meaningless against another instrument's corpus"
            )
        tasks = manifest.get("tasks")
        if isinstance(tasks, list):
            pg_results = [_check_pg_task(corpus_dir, entry) for entry in tasks]
    # A missing/unreadable manifest is already a named failure in
    # `base.corpus_failures`; repeating it here would double-report.

    return PgCheckReport(base=base, pg_results=pg_results, corpus_failures=corpus_failures)


def _verdict(ok: bool) -> str:
    return "PASS" if ok else "FAIL"


def format_report_pg(report: PgCheckReport) -> str:
    """The base report's table, then the pg table (S3-S5 per task), any
    pg corpus-level failures, and a pg OVERALL line."""
    lines: list[str] = [format_report(report.base), ""]
    if report.corpus_failures:
        lines.append("PG CORPUS: FAIL")
        for failure in report.corpus_failures:
            lines.append(f"  {failure}")
        lines.append("")

    name_width = max([len("task")] + [len(result.name) for result in report.pg_results])
    header = f"{'task':<{name_width}}  {'s3_satisfied':<12}  {'s4_nonvacuous':<13}  {'s5_p2_sha':<9}"
    rule = "-" * len(header)
    lines.append(header)
    lines.append(rule)
    for result in report.pg_results:
        lines.append(
            f"{result.name:<{name_width}}  {_verdict(result.s3_ok):<12}  "
            f"{_verdict(result.s4_ok):<13}  {_verdict(result.s5_ok):<9}"
        )

    detail_lines: list[str] = []
    for result in report.pg_results:
        for label, detail in (("s3", result.s3_detail), ("s4", result.s4_detail), ("s5", result.s5_detail)):
            if detail:
                detail_lines.append(f"  {result.name} {label}: {detail}")
    if detail_lines:
        lines.append(rule)
        lines.extend(detail_lines)

    lines.append(rule)
    lines.append(f"OVERALL: {_verdict(report.ok)}")
    return "\n".join(lines)


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description=(
            "Structural check for a premise-gone-battery corpus (spec §3, S1-S5): the base "
            "memory-battery checks plus the goal-satisfied-start, non-vacuity, and p2-sha "
            "checks -- every claim run, never asserted."
        )
    )
    parser.add_argument(
        "corpus_dir",
        type=Path,
        help="Corpus directory holding manifest.json and tasks/<name>/{workspace,pristine,pristine_p2}/.",
    )
    args = parser.parse_args(argv)

    try:
        report = check_corpus_pg(args.corpus_dir)
        print(format_report_pg(report))
    except Exception as exc:  # noqa: BLE001 -- last-resort net; verdict, never a traceback
        print(f"corpus_check_pg: FATAL: {exc!r}")
        return 1

    return 0 if report.ok else 1


if __name__ == "__main__":
    raise SystemExit(main())
