"""The memory-battery structural checker (design spec §3; task-2 brief).

"Structural check, before anything expensive" (spec §3, the black-oxide
rule): a frozen corpus does not get to assert its own soundness -- every
claim the corpus makes about itself is EXECUTED here, not read off the
manifest and trusted. `check_corpus(corpus_dir)` runs, per task:

1. **fails-before** -- the planted test, run in a throwaway copy of the
   PRISTINE workspace, must exit nonzero. Delegated verbatim to
   `tools.flywheel.factory.planted_test.fails_before_violations`: the
   factory's own fails-before rule (turn-4 spec §3) IS this check, and
   reusing it means check 1 runs the planted test under exactly the
   child-process shape the tool itself uses (`python3` off
   `PATH=/usr/bin:/bin`, `HOME=cwd`, `LANG=C`, combined stdout+stderr) --
   see that module's docstring for why that shape matters (a check run
   under this process's own environment could pass a test the tool then
   fails for an environmental reason).
2. **passes-after** -- `search` is required to occur EXACTLY ONCE in
   `target` (zero or more than one is itself a named corpus defect, never
   silently patched); the one occurrence is replaced with `replace` in
   another throwaway copy, and the same planted test must then exit 0.
   The rerun goes through `planted_test.run_python` -- the same public
   materialize-and-execute helper check 1's rule calls internally -- so
   both runs share the identical environment discipline without this
   module reimplementing it.
3. **workspace_sha256** -- recomputed for both `workspace/` and
   `pristine/` and required to equal the manifest's value (and each
   other). Per the controller's Task 1 carry-note (progress.md, task-1
   handoff: "the checker's sha recompute (check 3) must be an INDEPENDENT
   implementation of the pinned formula, not an import of corpus.py's
   helper -- cross-implementation agreement becomes the missing test
   vector"), `_independent_workspace_sha256` below is a deliberate,
   from-scratch duplicate of `corpus.py`'s `_workspace_sha256` -- same
   sorted-(path,bytes) formula, same NUL-separation on both sides -- and
   is NOT imported from `corpus.py`. The duplication is the point: it is
   what pins the formula the way a hand-computed test vector would.
4. **families** -- the manifest's declared `families` counts must equal
   the per-task `family` values counted directly from `manifest["tasks"]`
   -- a manifest edited independently of its own task list is caught
   here.

Every manifest task entry produces exactly one `TaskCheckResult` in the
returned `CheckReport`, even when a task cannot be run at all (missing
directory, unreadable file, a KeyError against a malformed entry): that
task is recorded as a named failure across all three per-task checks,
never dropped from the report and never silently skipped (task-2 brief).
"""

from __future__ import annotations

import argparse
import hashlib
import json
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Callable, Sequence

from tools.flywheel.factory.planted_test import fails_before_violations, run_python


@dataclass(frozen=True)
class TaskCheckResult:
    """One task's verdict across checks 1-3 (fails-before, passes-after,
    workspace_sha256). `ok` is the AND of all three: a task failing any
    one of them fails the corpus overall via `CheckReport.ok`."""

    name: str
    fails_before_ok: bool
    fails_before_detail: str
    passes_after_ok: bool
    passes_after_detail: str
    sha256_ok: bool
    sha256_detail: str

    @property
    def ok(self) -> bool:
        return self.fails_before_ok and self.passes_after_ok and self.sha256_ok


@dataclass(frozen=True)
class CheckReport:
    """The structural checker's full result: one `TaskCheckResult` per
    manifest task entry (check 1-3) plus the corpus-level family-count
    check (check 4). `ok` is the AND of everything -- the CLI's exit code
    is `0 if report.ok else 1`."""

    corpus_dir: Path
    task_results: list[TaskCheckResult]
    families_ok: bool
    families_detail: str

    @property
    def ok(self) -> bool:
        return self.families_ok and all(result.ok for result in self.task_results)


def _load_files(directory: Path) -> dict[str, str]:
    """Reads a flat task directory (`workspace/` or `pristine/`) into a
    `{filename: contents}` dict, walked in sorted order -- no filesystem-
    order dependence, mirroring `corpus.py`'s own discipline. Matches
    `corpus.py._write_workspace`'s shape: every file sits at the
    directory's own root, never nested."""
    return {
        path.name: path.read_text(encoding="utf-8")
        for path in sorted(directory.iterdir())
        if path.is_file()
    }


def _independent_workspace_sha256(files: dict[str, str]) -> str:
    """INDEPENDENT reimplementation of `corpus.py`'s `_workspace_sha256`
    formula: sha256 over the sorted `(relative_path, file_bytes)`
    sequence, each path/content pair NUL-separated on both sides.
    Deliberately duplicated rather than imported -- see this module's
    docstring (check 3) and the Task 1 controller carry-note
    (progress.md) it cites: cross-implementation agreement on this exact
    byte-pairing is what the checker is proving."""
    hasher = hashlib.sha256()
    for path in sorted(files):
        hasher.update(path.encode("utf-8"))
        hasher.update(b"\0")
        hasher.update(files[path].encode("utf-8"))
        hasher.update(b"\0")
    return hasher.hexdigest()


def _check_fails_before(pristine_files: dict[str, str], entry: dict[str, Any]) -> tuple[bool, str]:
    """Check 1: delegates to `planted_test.fails_before_violations`,
    which materializes `pristine_files` into its own throwaway temp
    directory and runs the planted test there -- see this module's
    docstring for why reusing that function (rather than re-running the
    subprocess here) matters."""
    violations = fails_before_violations(pristine_files, entry["test_file"], entry["run_argv"])
    return (not violations), "; ".join(violations)


def _check_passes_after(pristine_files: dict[str, str], entry: dict[str, Any]) -> tuple[bool, str]:
    """Check 2: search->replace `target` exactly once in a second
    throwaway copy of the pristine files (`run_python` materializes its
    own fresh temp directory per call, distinct from check 1's), then
    rerun the planted test and require exit 0."""
    target = entry["target"]
    search = entry["search"]
    replace = entry["replace"]

    if target not in pristine_files:
        return False, f"target {target!r} is not among the pristine workspace's files"

    occurrences = pristine_files[target].count(search)
    if occurrences != 1:
        return False, (
            f"search {search!r} occurs {occurrences} time(s) in {target!r} -- exact-once "
            f"occurrence is required (corpus defect)"
        )

    patched_files = dict(pristine_files)
    patched_files[target] = pristine_files[target].replace(search, replace, 1)

    result = run_python(patched_files, entry["run_argv"])
    if result.returncode != 0:
        return False, (
            f"planted test {entry['test_file']!r} exited {result.returncode} after the "
            f"search->replace patch (expected 0)\n{result.stdout}"
        )
    return True, ""


def _check_sha256(task_dir: Path, entry: dict[str, Any]) -> tuple[bool, str]:
    """Check 3: recomputes `workspace_sha256` independently (see
    `_independent_workspace_sha256`) for both `workspace/` and
    `pristine/`, and requires each to equal the manifest's value -- which
    also proves `workspace == pristine`, since both must equal the same
    single recorded hash."""
    workspace_files = _load_files(task_dir / "workspace")
    pristine_files = _load_files(task_dir / "pristine")

    workspace_sha = _independent_workspace_sha256(workspace_files)
    pristine_sha = _independent_workspace_sha256(pristine_files)
    expected = entry["workspace_sha256"]

    problems: list[str] = []
    if workspace_sha != expected:
        problems.append(f"workspace sha256 {workspace_sha} != manifest {expected}")
    if pristine_sha != expected:
        problems.append(f"pristine sha256 {pristine_sha} != manifest {expected}")
    if workspace_files != pristine_files:
        problems.append("workspace and pristine file contents differ")

    return (not problems), "; ".join(problems)


def _check_families(manifest: dict[str, Any]) -> tuple[bool, str]:
    """Check 4: the manifest's declared `families` counts must equal the
    per-task `family` values counted directly from `manifest["tasks"]` --
    catches a manifest doctored after the fact, independently of its own
    task list."""
    observed: dict[str, int] = {}
    for entry in manifest["tasks"]:
        family = entry["family"]
        observed[family] = observed.get(family, 0) + 1

    declared = manifest["families"]
    if observed != declared:
        return False, f"observed family counts {observed} != manifest['families'] {declared}"
    return True, ""


def _safe_check(fn: Callable[..., tuple[bool, str]], *args: Any) -> tuple[bool, str]:
    """Runs one per-task check function, turning any exception it raises
    into a failed verdict rather than letting it propagate -- an
    unrunnable task is a named failure, never a crash that silently drops
    every task after it from the report (task-2 brief)."""
    try:
        return fn(*args)
    except Exception as exc:  # noqa: BLE001 -- deliberately broad, see docstring
        return False, f"check raised {exc!r}"


def _check_task(corpus_dir: Path, entry: dict[str, Any]) -> TaskCheckResult:
    """Runs checks 1-3 for one manifest task entry. Loading the pristine
    workspace is done once, up front, and guarded the same way: a missing
    directory or unreadable file fails all three checks for this task
    rather than raising out of `check_corpus` and losing every task that
    would have run after it."""
    name = entry.get("name", "<unnamed task>")
    try:
        workspace_dir = corpus_dir / entry["workspace"]
        task_dir = workspace_dir.parent
        pristine_files = _load_files(task_dir / "pristine")
    except Exception as exc:  # noqa: BLE001 -- see docstring: any failure here must be recorded
        detail = f"could not load pristine workspace for checking: {exc!r}"
        return TaskCheckResult(
            name=name,
            fails_before_ok=False,
            fails_before_detail=detail,
            passes_after_ok=False,
            passes_after_detail=detail,
            sha256_ok=False,
            sha256_detail=detail,
        )

    fails_before_ok, fails_before_detail = _safe_check(_check_fails_before, pristine_files, entry)
    passes_after_ok, passes_after_detail = _safe_check(_check_passes_after, pristine_files, entry)
    sha256_ok, sha256_detail = _safe_check(_check_sha256, task_dir, entry)

    return TaskCheckResult(
        name=name,
        fails_before_ok=fails_before_ok,
        fails_before_detail=fails_before_detail,
        passes_after_ok=passes_after_ok,
        passes_after_detail=passes_after_detail,
        sha256_ok=sha256_ok,
        sha256_detail=sha256_detail,
    )


def check_corpus(corpus_dir: Path) -> CheckReport:
    """Runs all four checks against the corpus at `corpus_dir` (which
    must hold `manifest.json` and `tasks/<name>/{workspace,pristine}/`,
    Task 1's `generate_corpus` shape) and returns the full report. Every
    entry in `manifest["tasks"]` produces exactly one `TaskCheckResult` --
    iteration never skips or drops one, per this module's docstring."""
    corpus_dir = Path(corpus_dir)
    manifest = json.loads((corpus_dir / "manifest.json").read_text(encoding="utf-8"))

    task_results = [_check_task(corpus_dir, entry) for entry in manifest["tasks"]]
    families_ok, families_detail = _check_families(manifest)

    return CheckReport(
        corpus_dir=corpus_dir,
        task_results=task_results,
        families_ok=families_ok,
        families_detail=families_detail,
    )


def _verdict(ok: bool) -> str:
    return "PASS" if ok else "FAIL"


def format_report(report: CheckReport) -> str:
    """Renders `report` as the per-task verdict table the CLI prints:
    one row per task across the three per-task checks, a families row,
    failure detail beneath (only for what actually failed), and an
    OVERALL line."""
    name_width = max([len("task")] + [len(result.name) for result in report.task_results])
    header = f"{'task':<{name_width}}  {'fails_before':<12}  {'passes_after':<12}  {'sha256':<6}"
    rule = "-" * len(header)

    lines = [header, rule]
    for result in report.task_results:
        lines.append(
            f"{result.name:<{name_width}}  {_verdict(result.fails_before_ok):<12}  "
            f"{_verdict(result.passes_after_ok):<12}  {_verdict(result.sha256_ok):<6}"
        )
    lines.append(rule)
    lines.append(f"{'families':<{name_width}}  {_verdict(report.families_ok)}")

    detail_lines: list[str] = []
    if report.families_detail:
        detail_lines.append(f"  families: {report.families_detail}")
    for result in report.task_results:
        if result.fails_before_detail:
            detail_lines.append(f"  {result.name} fails_before: {result.fails_before_detail}")
        if result.passes_after_detail:
            detail_lines.append(f"  {result.name} passes_after: {result.passes_after_detail}")
        if result.sha256_detail:
            detail_lines.append(f"  {result.name} sha256: {result.sha256_detail}")
    if detail_lines:
        lines.append(rule)
        lines.extend(detail_lines)

    lines.append(rule)
    lines.append(f"OVERALL: {_verdict(report.ok)}")
    return "\n".join(lines)


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description=(
            "Structural check for a memory-battery corpus (design spec §3; task-2 brief): "
            "executes fails-before/passes-after per task, recomputes workspace_sha256 "
            "independently, and verifies family counts -- every claim run, never asserted."
        )
    )
    parser.add_argument(
        "corpus_dir",
        type=Path,
        help="Corpus directory holding manifest.json and tasks/<name>/{workspace,pristine}/.",
    )
    args = parser.parse_args(argv)

    report = check_corpus(args.corpus_dir)
    print(format_report(report))
    return 0 if report.ok else 1


if __name__ == "__main__":
    raise SystemExit(main())
