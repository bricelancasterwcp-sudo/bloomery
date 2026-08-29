"""The s5-weight-battery structural checker (design spec
`docs/superpowers/specs/2026-08-28-s5-weight-battery-v1-design.md` §3;
plan Task 2).

Base checks (S1 fails-before, S2 passes-after, base shas, families) are
`corpus_check.check_corpus`'s, run verbatim. Per-lane additions:

- lane `moot`: `corpus_check_pg`'s S3 (goal-satisfied start) and S4
  (non-vacuity) verbatim — the same executed guarantees the pg battery
  froze under.
- lane `stale`: B1 the moved-goal test FAILS on the defective target;
  B2 it FAILS on the stored-fix target (search→replace applied exactly
  once); B3 the `witness/<target>` PASSES it (satisfiability, executed);
  B4 `pristine_p2/`'s target byte-identical to `pristine/`'s; B5 the p2
  test differs from the original; plus the independent
  `workspace_p2_sha256` recompute (shared with moot via S5).
- lane `control`: NO `pristine_p2` key and no such directory — a
  control task that acquired a p2 tree is a lane-contamination defect.

Corpus-level: the manifest must name THIS instrument, and every task
must carry a lane from the fixed set with the declared `n_per_lane`
quotas. Guard discipline mirrors `corpus_check.py`.
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
from tools.memory_battery.corpus_check_pg import _check_s3, _check_s4
from tools.memory_battery.corpus_s5 import (
    INSTRUMENT_S5,
    LANE_CONTROL,
    LANE_MOOT,
    LANE_STALE,
)


@dataclass(frozen=True)
class LaneCheckResult:
    """One task's verdict across its lane's checks. Fields not applicable
    to the task's lane hold `True` with empty detail (a per-lane check
    that never ran cannot fail a task it does not apply to)."""

    name: str
    lane: str
    b1_ok: bool
    b1_detail: str
    b2_ok: bool
    b2_detail: str
    b3_ok: bool
    b3_detail: str
    b4_ok: bool
    b4_detail: str
    b5_ok: bool
    b5_detail: str
    sha_ok: bool
    sha_detail: str

    @property
    def ok(self) -> bool:
        return (
            self.b1_ok
            and self.b2_ok
            and self.b3_ok
            and self.b4_ok
            and self.b5_ok
            and self.sha_ok
        )


@dataclass(frozen=True)
class S5CheckReport:
    base: CheckReport
    lane_results: list[LaneCheckResult]
    corpus_failures: list[str]

    @property
    def ok(self) -> bool:
        return (
            self.base.ok
            and not self.corpus_failures
            and all(result.ok for result in self.lane_results)
        )


_OK = (True, "")


def _check_b1_fails_on_defective(task_dir: Path, entry: dict[str, Any]) -> tuple[bool, str]:
    p2_files = _load_files(task_dir / "pristine_p2")
    result = run_python(p2_files, entry["run_argv"])
    if result.returncode == 0:
        return False, "moved-goal test passes on the defective target (expected nonzero)"
    return _OK


def _check_b2_fails_on_stored_fix(task_dir: Path, entry: dict[str, Any]) -> tuple[bool, str]:
    pristine_files = _load_files(task_dir / "pristine")
    p2_files = _load_files(task_dir / "pristine_p2")
    target, search = entry["target"], entry["search"]
    if pristine_files.get(target, "").count(search) != 1:
        return False, f"search does not occur exactly once in pristine {target!r}"
    fixed = dict(p2_files)
    fixed[target] = pristine_files[target].replace(search, entry["replace"], 1)
    result = run_python(fixed, entry["run_argv"])
    if result.returncode == 0:
        return False, (
            "moved-goal test passes on the stored-fix target (expected nonzero) -- the stored "
            "lesson is not actually stale under this test"
        )
    return _OK


def _check_b3_witness_passes(task_dir: Path, entry: dict[str, Any]) -> tuple[bool, str]:
    target = entry["target"]
    witness_path = task_dir / "witness" / target
    if not witness_path.is_file():
        return False, f"witness/{target} does not exist"
    p2_files = _load_files(task_dir / "pristine_p2")
    witness_files = dict(p2_files)
    witness_files[target] = witness_path.read_text(encoding="utf-8")
    result = run_python(witness_files, entry["run_argv"])
    if result.returncode != 0:
        return False, (
            f"witness target does not satisfy the moved-goal test (exit {result.returncode}) -- "
            f"satisfiability unproven\n{result.stdout}"
        )
    return _OK


def _check_b4_target_bytes(task_dir: Path, entry: dict[str, Any]) -> tuple[bool, str]:
    pristine_files = _load_files(task_dir / "pristine")
    p2_files = _load_files(task_dir / "pristine_p2")
    target = entry["target"]
    if p2_files.get(target) != pristine_files.get(target):
        return False, f"pristine_p2 target {target!r} is not byte-identical to pristine's"
    return _OK


def _check_b5_test_differs(task_dir: Path, entry: dict[str, Any]) -> tuple[bool, str]:
    pristine_files = _load_files(task_dir / "pristine")
    p2_files = _load_files(task_dir / "pristine_p2")
    test_file = entry["test_file"]
    if p2_files.get(test_file) == pristine_files.get(test_file):
        return False, f"pristine_p2 test {test_file!r} is byte-identical to pristine's"
    return _OK


def _check_p2_sha(task_dir: Path, entry: dict[str, Any]) -> tuple[bool, str]:
    p2_files = _load_files(task_dir / "pristine_p2")
    recomputed = _independent_workspace_sha256(p2_files)
    if recomputed != entry.get("workspace_p2_sha256"):
        return False, (
            f"pristine_p2 sha256 {recomputed} != manifest workspace_p2_sha256 "
            f"{entry.get('workspace_p2_sha256')}"
        )
    return _OK


def _check_control_has_no_p2(task_dir: Path, entry: dict[str, Any]) -> tuple[bool, str]:
    problems: list[str] = []
    if "pristine_p2" in entry:
        problems.append("control task carries a pristine_p2 manifest key")
    if (task_dir / "pristine_p2").exists():
        problems.append("control task has a pristine_p2 directory on disk")
    return (not problems), "; ".join(problems)


def _check_lane_task(corpus_dir: Path, entry: dict[str, Any]) -> LaneCheckResult:
    name = entry.get("name", "<unnamed task>")
    lane = entry.get("lane", "<no lane>")
    try:
        task_dir = (corpus_dir / entry["workspace"]).parent
    except Exception as exc:  # noqa: BLE001 -- recorded, never raised
        detail = f"could not resolve task directory: {exc!r}"
        return LaneCheckResult(name, lane, False, detail, False, detail, False, detail, False, detail, False, detail, False, detail)

    ok_pairs = {key: _OK for key in ("b1", "b2", "b3", "b4", "b5", "sha")}
    if lane == LANE_STALE:
        ok_pairs["b1"] = _safe_check(_check_b1_fails_on_defective, task_dir, entry)
        ok_pairs["b2"] = _safe_check(_check_b2_fails_on_stored_fix, task_dir, entry)
        ok_pairs["b3"] = _safe_check(_check_b3_witness_passes, task_dir, entry)
        ok_pairs["b4"] = _safe_check(_check_b4_target_bytes, task_dir, entry)
        ok_pairs["b5"] = _safe_check(_check_b5_test_differs, task_dir, entry)
        ok_pairs["sha"] = _safe_check(_check_p2_sha, task_dir, entry)
    elif lane == LANE_MOOT:
        # The pg battery's own executed guarantees, verbatim: S3
        # (goal-satisfied start incl. target byte-identity + moved-on
        # passes on defective) reported under b1, S4 (non-vacuity: fails
        # on the stored fix) under b2.
        ok_pairs["b1"] = _safe_check(_check_s3, task_dir, entry)
        ok_pairs["b2"] = _safe_check(_check_s4, task_dir, entry)
        ok_pairs["sha"] = _safe_check(_check_p2_sha, task_dir, entry)
    elif lane == LANE_CONTROL:
        ok_pairs["b1"] = _safe_check(_check_control_has_no_p2, task_dir, entry)
    else:
        ok_pairs["b1"] = (False, f"unknown lane {lane!r}")

    return LaneCheckResult(
        name,
        lane,
        *ok_pairs["b1"],
        *ok_pairs["b2"],
        *ok_pairs["b3"],
        *ok_pairs["b4"],
        *ok_pairs["b5"],
        *ok_pairs["sha"],
    )


def check_corpus_s5(corpus_dir: Path) -> S5CheckReport:
    """Base checks plus the per-lane checks. Never raises."""
    corpus_dir = Path(corpus_dir)
    base = check_corpus(corpus_dir)

    corpus_failures: list[str] = []
    lane_results: list[LaneCheckResult] = []

    manifest, load_error = _load_manifest(corpus_dir)
    if not load_error and manifest is not None:
        instrument = manifest.get("instrument")
        if instrument != INSTRUMENT_S5:
            corpus_failures.append(
                f"manifest instrument {instrument!r} != {INSTRUMENT_S5!r}"
            )
        tasks = manifest.get("tasks")
        if isinstance(tasks, list):
            lane_results = [_check_lane_task(corpus_dir, entry) for entry in tasks]
            declared = manifest.get("n_per_lane")
            observed = {
                lane: sum(1 for entry in tasks if entry.get("lane") == lane)
                for lane in (LANE_CONTROL, LANE_MOOT, LANE_STALE)
            }
            if any(count != declared for count in observed.values()):
                corpus_failures.append(
                    f"lane quotas {observed} do not all equal n_per_lane={declared!r}"
                )

    return S5CheckReport(base=base, lane_results=lane_results, corpus_failures=corpus_failures)


def _verdict(ok: bool) -> str:
    return "PASS" if ok else "FAIL"


def format_report_s5(report: S5CheckReport) -> str:
    lines: list[str] = [format_report(report.base), ""]
    if report.corpus_failures:
        lines.append("S5 CORPUS: FAIL")
        for failure in report.corpus_failures:
            lines.append(f"  {failure}")
        lines.append("")

    name_width = max([len("task")] + [len(result.name) for result in report.lane_results])
    header = f"{'task':<{name_width}}  {'lane':<8}  {'lane_checks':<11}"
    rule = "-" * len(header)
    lines.append(header)
    lines.append(rule)
    for result in report.lane_results:
        lines.append(f"{result.name:<{name_width}}  {result.lane:<8}  {_verdict(result.ok):<11}")

    detail_lines: list[str] = []
    for result in report.lane_results:
        for label in ("b1", "b2", "b3", "b4", "b5", "sha"):
            detail = getattr(result, f"{label}_detail")
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
            "Structural check for an s5-weight-battery corpus (spec §3): base memory-battery "
            "checks plus per-lane executed guarantees (moot S3/S4; stale B1-B5 incl. the "
            "witness; control lane-purity)."
        )
    )
    parser.add_argument("corpus_dir", type=Path)
    args = parser.parse_args(argv)
    try:
        report = check_corpus_s5(args.corpus_dir)
        print(format_report_s5(report))
    except Exception as exc:  # noqa: BLE001 -- verdict, never a traceback
        print(f"corpus_check_s5: FATAL: {exc!r}")
        return 1
    return 0 if report.ok else 1


if __name__ == "__main__":
    raise SystemExit(main())
