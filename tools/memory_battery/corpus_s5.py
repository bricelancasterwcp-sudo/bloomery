"""The s5-weight-battery corpus generator (design spec
`docs/superpowers/specs/2026-08-28-s5-weight-battery-v1-design.md` §3;
plan Task 1).

Three ground-truth lanes over the same factory families:

- `control`: the plain byte-reset repeat (no `pristine_p2`) — lesson
  right and applicable.
- `moot`: the premise-gone treatment verbatim (`corpus_pg`'s
  execute-and-pin moved-on test) — lesson true but inapplicable.
- `stale`: the **moved-goal** treatment — the expected literal replaced
  by a THIRD value neither the defective nor the stored-fix output
  produces (spec §3's deterministic type rules), so the stored lesson is
  wrong and the goal text misdescribes the fix. Every stale task ships a
  `witness/` target (defective source + a `def <fn>(*args)` override
  returning the third value) proving by execution that the moved-goal
  test is satisfiable by patching the target alone; the witness lives
  outside the three run trees and never reaches the model.

Lane assignment is deterministic (spec §3): iterate the draw in order; a
task goes to the first unfilled lane in priority `stale → moot →
control` for which it qualifies. Exclusion-vs-block discipline is
`corpus_pg`'s: a raising/unpinnable probe or a two-valued domain
EXCLUDES the task from that lane (bounded overdraw), while a shape
violation or a failed generator-side sanity run BLOCKS generation.
"""

from __future__ import annotations

import ast
import json
from pathlib import Path
from typing import Any

from tools.flywheel.factory import planted_test
from tools.flywheel.factory.task import Task
from tools.memory_battery.corpus import (
    CorpusExhaustedError,
    _INITIAL_OVERDRAW_MULTIPLIER,
    _MAX_OVERDRAW_MULTIPLIER,
    _run_verified_python_tasks,
    _task_manifest_entry,
    _workspace_sha256,
    _write_workspace,
)
from tools.memory_battery.corpus_pg import (
    MovedOnShapeError,
    _PROBE_FILE,
    _call_root_name,
    _single_assert_equal,
    author_moved_on_test,
)

INSTRUMENT_S5 = "s5-weight-battery-v1"

LANE_CONTROL = "control"
LANE_MOOT = "moot"
LANE_STALE = "stale"
# Spec §3's assignment priority: stale is scarcest (two-valued domains
# excluded), so it claims qualified tasks first.
LANE_PRIORITY = (LANE_STALE, LANE_MOOT, LANE_CONTROL)


def third_value(defective: Any, fixed: Any) -> Any | None:
    """Spec §3's deterministic third-value synthesis: a value distinct
    from BOTH observed outputs, by type — or `None` when the domain is
    two-valued (booleans) or unhandled. The perturbation constants are
    arbitrary [judgment-flagged in the spec]; the registered property is
    the executed distinctness."""
    if isinstance(defective, bool) or isinstance(fixed, bool):
        return None
    if isinstance(defective, (int, float)) and isinstance(fixed, (int, float)):
        return max(defective, fixed) + 7
    if isinstance(defective, str) and isinstance(fixed, str):
        candidate = fixed + " (rev 2)"
        return candidate if candidate != defective else candidate + " (rev 2)"
    if isinstance(defective, tuple) and isinstance(fixed, tuple):
        candidate = tuple(reversed(fixed))
        if candidate not in (defective, fixed):
            return candidate
        if all(isinstance(x, (int, float)) and not isinstance(x, bool) for x in fixed) and fixed:
            appended = fixed + (max(fixed) + 7,)
            if appended not in (defective, fixed):
                return appended
        return None
    return None


def _probe_value(files: dict[str, str], target_stem: str, call_src: str) -> Any | None:
    """The observed value of `call_src` against `files`, or `None` when
    the call raises or its repr does not round-trip a literal — the same
    execute-and-pin discipline as `corpus_pg.author_moved_on_test`."""
    probe = f"import {target_stem}\nprint(repr({call_src}))\n"
    result = planted_test.run_python({**files, _PROBE_FILE: probe}, ("python3", _PROBE_FILE))
    if result.returncode != 0:
        return None
    observed_repr = result.stdout.strip().splitlines()[-1] if result.stdout.strip() else ""
    try:
        return ast.literal_eval(observed_repr)
    except (ValueError, SyntaxError):
        return None


def author_moved_goal_test(
    test_source: str, target_stem: str, files: dict[str, str], search: str, replace: str
) -> tuple[str, str] | None:
    """The stale lane's `(moved_goal_test_source, witness_target_source)`,
    or `None` when this task is EXCLUDED from the lane (raising or
    unpinnable probe on either version; no third value in the domain).
    Raises `MovedOnShapeError` on shape violations or when a
    generator-side sanity run fails (fails-on-defective,
    fails-on-stored-fix, passes-on-witness — the checker re-verifies all
    three independently)."""
    tree = ast.parse(test_source)
    assert_call = _single_assert_equal(tree)
    call_node, expected_node = assert_call.args
    if _call_root_name(call_node) != target_stem:
        raise MovedOnShapeError(
            f"assertEqual's call expression is not rooted at the target module {target_stem!r}"
        )
    if expected_node.lineno != expected_node.end_lineno:
        raise MovedOnShapeError("assertEqual's expected literal spans multiple lines")
    call_src = ast.get_source_segment(test_source, call_node)
    if call_src is None:
        raise MovedOnShapeError("could not read the call expression's source segment")
    fn_name = assert_call.args[0].func.attr if isinstance(call_node, ast.Call) else None
    if not isinstance(call_node, ast.Call) or not isinstance(call_node.func, ast.Attribute):
        raise MovedOnShapeError("assertEqual's first argument is not a module.fn(...) call")
    fn_name = call_node.func.attr

    target_file = f"{target_stem}.py"
    defective_source = files.get(target_file)
    if defective_source is None:
        raise MovedOnShapeError(f"target file {target_file!r} is not among the task's files")
    if defective_source.count(search) != 1:
        raise MovedOnShapeError(
            f"search does not occur exactly once in {target_file!r} -- cannot build the fixed target"
        )
    fixed_files = {**files, target_file: defective_source.replace(search, replace, 1)}

    defective = _probe_value(files, target_stem, call_src)
    if defective is None:
        return None
    fixed = _probe_value(fixed_files, target_stem, call_src)
    if fixed is None:
        return None

    third = third_value(defective, fixed)
    if third is None:
        return None
    if third in (defective, fixed):
        raise MovedOnShapeError(
            f"third-value rule produced a colliding value {third!r} (defective {defective!r}, "
            f"fixed {fixed!r}) -- the distinctness contract broke"
        )

    lines = test_source.splitlines(keepends=True)
    line_index = expected_node.lineno - 1
    line = lines[line_index]
    lines[line_index] = (
        line[: expected_node.col_offset] + repr(third) + line[expected_node.end_col_offset :]
    )
    moved_goal = "".join(lines)

    witness = defective_source + f"\n\ndef {fn_name}(*args):\n    return {third!r}\n"
    return moved_goal, witness


def _sanity_moved_goal(
    task: Task, moved_goal: str, witness: str
) -> None:
    """Generator-side executed guarantees (checker re-verifies): the
    moved-goal test FAILS on the defective target, FAILS on the
    stored-fix target, and PASSES on the witness."""
    defective_files = {**task.files, task.test_file: moved_goal}
    fixed_target = task.files[task.target].replace(task.search, task.replace, 1)
    argv = tuple(task.run_argv)
    for label, files, want_zero in (
        ("defective", defective_files, False),
        ("stored-fix", {**defective_files, task.target: fixed_target}, False),
        ("witness", {**defective_files, task.target: witness}, True),
    ):
        result = planted_test.run_python(files, argv)
        ok = (result.returncode == 0) == want_zero
        if not ok:
            raise MovedOnShapeError(
                f"moved-goal sanity failed on the {label} target "
                f"(exit {result.returncode}, wanted {'0' if want_zero else 'nonzero'}):\n{result.stdout}"
            )


def _draw_lanes(seed: int, n_per_lane: int) -> list[tuple[Task, str, dict[str, str] | None, str | None]]:
    """corpus.py's overdraw pattern with the lane-assignment rule: returns
    `(task, lane, p2_files_or_None, witness_or_None)` in draw order."""
    quotas = {LANE_STALE: n_per_lane, LANE_MOOT: n_per_lane, LANE_CONTROL: n_per_lane}
    multiplier = _INITIAL_OVERDRAW_MULTIPLIER
    while multiplier <= _MAX_OVERDRAW_MULTIPLIER:
        assigned: list[tuple[Task, str, dict[str, str] | None, str | None]] = []
        filled = {lane: 0 for lane in quotas}
        for task in _run_verified_python_tasks(seed, 3 * n_per_lane * multiplier):
            target_stem = Path(task.target).stem
            lane: str | None = None
            p2_files: dict[str, str] | None = None
            witness: str | None = None
            for candidate_lane in LANE_PRIORITY:
                if filled[candidate_lane] >= quotas[candidate_lane]:
                    continue
                if candidate_lane == LANE_STALE:
                    authored = author_moved_goal_test(
                        task.files[task.test_file], target_stem, task.files, task.search, task.replace
                    )
                    if authored is None:
                        continue
                    moved_goal, witness_source = authored
                    _sanity_moved_goal(task, moved_goal, witness_source)
                    lane = LANE_STALE
                    p2_files = {**task.files, task.test_file: moved_goal}
                    witness = witness_source
                elif candidate_lane == LANE_MOOT:
                    moved_on = author_moved_on_test(task.files[task.test_file], target_stem, task.files)
                    if moved_on is None:
                        continue
                    lane = LANE_MOOT
                    p2_files = {**task.files, task.test_file: moved_on}
                else:
                    lane = LANE_CONTROL
                break
            if lane is None:
                continue
            filled[lane] += 1
            assigned.append((task, lane, p2_files, witness))
            if all(filled[l] >= quotas[l] for l in quotas):
                return assigned
        multiplier *= 2
    raise CorpusExhaustedError(
        f"could not fill 3x{n_per_lane} lanes at seed {seed} within the draw budget"
    )


def generate_corpus_s5(seed: int, n_per_lane: int, out_dir: Path) -> dict[str, Any]:
    """Draws `3 * n_per_lane` lane-assigned tasks at `seed` and
    materializes `out_dir/tasks/<name>/` with `workspace/` + `pristine/`
    always, `pristine_p2/` for moot/stale, and `witness/<target>` for
    stale; writes `manifest.json` (pg schema + per-task `lane` +
    top-level `n_per_lane` and `families_by_lane`)."""
    out_dir = Path(out_dir)
    drawn = _draw_lanes(seed, n_per_lane)

    task_entries: list[dict[str, Any]] = []
    families: dict[str, int] = {}
    families_by_lane: dict[str, dict[str, int]] = {
        LANE_CONTROL: {},
        LANE_MOOT: {},
        LANE_STALE: {},
    }
    for index, (task, lane, p2_files, witness) in enumerate(drawn):
        name = f"{task.name}-{index:04d}"
        task_dir = out_dir / "tasks" / name
        _write_workspace(task_dir / "workspace", task.files)
        _write_workspace(task_dir / "pristine", task.files)
        entry = _task_manifest_entry(task, name, task_dir / "workspace", out_dir)
        entry["lane"] = lane
        if p2_files is not None:
            _write_workspace(task_dir / "pristine_p2", p2_files)
            entry["pristine_p2"] = str((task_dir / "pristine_p2").relative_to(out_dir))
            entry["workspace_p2_sha256"] = _workspace_sha256(p2_files)
        if witness is not None:
            _write_workspace(task_dir / "witness", {task.target: witness})
        task_entries.append(entry)
        families[task.name] = families.get(task.name, 0) + 1
        families_by_lane[lane][task.name] = families_by_lane[lane].get(task.name, 0) + 1

    manifest: dict[str, Any] = {
        "instrument": INSTRUMENT_S5,
        "corpus_seed": seed,
        "n": len(task_entries),
        "n_per_lane": n_per_lane,
        "families": dict(sorted(families.items())),
        "families_by_lane": {
            lane: dict(sorted(counts.items())) for lane, counts in families_by_lane.items()
        },
        "tasks": task_entries,
    }
    out_dir.mkdir(parents=True, exist_ok=True)
    (out_dir / "manifest.json").write_text(
        json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    return manifest
