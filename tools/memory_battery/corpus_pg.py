"""The premise-gone-battery corpus generator (design spec
`docs/superpowers/specs/2026-08-28-premise-gone-battery-v1-design.md` §3;
plan Task 1).

Same factory draw discipline as `corpus.py` (whose helpers this module
deliberately reuses — the generator side of the house has no independence
requirement; that belongs to the CHECKER, `corpus_check_pg.py`), plus the
one thing this corpus exists for: a per-task `pristine_p2/` phase-2
source holding the target at its defective, fingerprint-matching bytes
beside a **moved-on test** — same filename, same import, only the
expected literal rewritten to the value the DEFECTIVE code actually
produces (spec §0's "the world moved on" reading, the only
goal-satisfied flavor reachable under two-stage exact retrieval).

**Moved-on authoring is execute-and-pin.** The planted test's single
`self.assertEqual(<call>, <literal>)` is located by AST; `<call>` is
evaluated against the defective module in a subprocess under
`planted_test.run_python`'s own environment shape; the observed value's
`repr` replaces the literal, byte-for-byte in place, nothing else
touched. Two outcomes are EXCLUSIONS (the task is dropped and the
overdraw loop keeps going — a bounded redraw, never a silent shrink):
a defective call that RAISES (a crash is not "the world moved on"), and
a result whose `repr` does not round-trip `ast.literal_eval` (no honest
literal exists to pin). Everything else unexpected is a
`MovedOnShapeError` — a hard, named error, because the run-verified
python families are single-assert by construction (surveyed across all
50 corpus-v1 instances) and a shape drift should BLOCK the generation,
not silently thin the corpus.

Two generator-side sanity runs guard the authoring before the checker
ever sees the corpus (the checker re-verifies both independently):
the moved-on value must differ from the planted expectation (equal
values would contradict the factory's own fails-before rule), and the
moved-on test must exit 0 on the defective files.
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

INSTRUMENT_PG = "premise-gone-battery-v1"

_PROBE_FILE = "_pg_probe.py"


class MovedOnShapeError(RuntimeError):
    """The planted test does not have the surveyed single-assert shape
    (exactly one `self.assertEqual(<call>, <one-line literal>)` with the
    call rooted at the target module), or an authoring invariant broke
    (moved-on value equal to the planted expectation; moved-on test not
    passing on the defective files). Always a BLOCK, never an
    exclusion — see the module docstring."""


def _single_assert_equal(tree: ast.Module) -> ast.Call:
    """The planted test's one `self.assertEqual(...)` call, or raises."""
    found = [
        node
        for node in ast.walk(tree)
        if isinstance(node, ast.Call)
        and isinstance(node.func, ast.Attribute)
        and node.func.attr == "assertEqual"
        and isinstance(node.func.value, ast.Name)
        and node.func.value.id == "self"
    ]
    if len(found) != 1:
        raise MovedOnShapeError(
            f"expected exactly one self.assertEqual in the planted test, found {len(found)}"
        )
    call = found[0]
    if len(call.args) != 2 or call.keywords:
        raise MovedOnShapeError(
            "self.assertEqual must carry exactly two positional arguments"
        )
    return call


def _call_root_name(node: ast.AST) -> str | None:
    """The leftmost `Name` a call expression hangs off (e.g. `calcmod` in
    `calcmod.double(2)`), or None when the expression is rooted elsewhere."""
    while isinstance(node, (ast.Call, ast.Attribute, ast.Subscript)):
        node = node.func if isinstance(node, ast.Call) else node.value
    return node.id if isinstance(node, ast.Name) else None


def author_moved_on_test(test_source: str, target_stem: str, files: dict[str, str]) -> str | None:
    """The moved-on test's source, or `None` when this task is EXCLUDED
    (defective call raises, or its value has no round-tripping literal
    repr). Raises `MovedOnShapeError` on any shape violation — see the
    module docstring for the exclusion-vs-block rule."""
    tree = ast.parse(test_source)
    assert_call = _single_assert_equal(tree)
    call_node, expected_node = assert_call.args

    if _call_root_name(call_node) != target_stem:
        raise MovedOnShapeError(
            f"assertEqual's call expression is not rooted at the target module {target_stem!r}"
        )
    if expected_node.lineno != expected_node.end_lineno:
        raise MovedOnShapeError("assertEqual's expected literal spans multiple lines")
    try:
        planted_expected = ast.literal_eval(expected_node)
    except ValueError as error:
        raise MovedOnShapeError(
            "assertEqual's expected argument is not a literal"
        ) from error

    call_src = ast.get_source_segment(test_source, call_node)
    if call_src is None:
        raise MovedOnShapeError("could not read the call expression's source segment")

    # Execute-and-pin: the defective value, observed under the factory's
    # own child-process environment shape (see `planted_test.run_python`).
    probe = f"import {target_stem}\nprint(repr({call_src}))\n"
    result = planted_test.run_python({**files, _PROBE_FILE: probe}, ("python3", _PROBE_FILE))
    if result.returncode != 0:
        return None  # the defective call raises -- excluded, not moved-on
    observed_repr = result.stdout.strip().splitlines()[-1] if result.stdout.strip() else ""
    try:
        observed = ast.literal_eval(observed_repr)
    except (ValueError, SyntaxError):
        return None  # no honest literal to pin -- excluded

    if observed == planted_expected:
        raise MovedOnShapeError(
            "defective value equals the planted expectation -- the factory's fails-before "
            "rule says this cannot happen; refusing to author a no-op moved-on test"
        )

    # Splice the observed repr over the expected literal, in place.
    lines = test_source.splitlines(keepends=True)
    line_index = expected_node.lineno - 1
    line = lines[line_index]
    lines[line_index] = line[: expected_node.col_offset] + observed_repr + line[expected_node.end_col_offset :]
    return "".join(lines)


def _author_p2_files(task: Task) -> dict[str, str] | None:
    """The task's phase-2 file set (target bytes verbatim, moved-on test
    at the same name), or `None` when the task is excluded. Runs the
    generator-side sanity check: the moved-on test must exit 0 on the
    defective files (a failure here is an authoring bug, never an
    exclusion)."""
    target_stem = Path(task.target).stem
    moved_on = author_moved_on_test(task.files[task.test_file], target_stem, task.files)
    if moved_on is None:
        return None
    p2_files = {**task.files, task.test_file: moved_on}
    sanity = planted_test.run_python(p2_files, tuple(task.run_argv))
    if sanity.returncode != 0:
        raise MovedOnShapeError(
            f"authored moved-on test does not pass on the defective files "
            f"(exit {sanity.returncode}):\n{sanity.stdout}"
        )
    return p2_files


def _draw_authorable_tasks(seed: int, n: int) -> list[tuple[Task, dict[str, str]]]:
    """corpus.py's overdraw pattern with one extra filter: only tasks
    whose moved-on test is authorable (see `author_moved_on_test`'s
    exclusion rule) count toward `n`."""
    multiplier = _INITIAL_OVERDRAW_MULTIPLIER
    selected: list[tuple[Task, dict[str, str]]] = []
    while multiplier <= _MAX_OVERDRAW_MULTIPLIER:
        selected = []
        for task in _run_verified_python_tasks(seed, n * multiplier):
            p2_files = _author_p2_files(task)
            if p2_files is not None:
                selected.append((task, p2_files))
            if len(selected) == n:
                return selected
        multiplier *= 2
    raise CorpusExhaustedError(
        f"could not draw {n} authorable run-verified python task(s) at seed {seed} within "
        f"{n * _MAX_OVERDRAW_MULTIPLIER} candidate draws (found only {len(selected)})"
    )


def generate_corpus_pg(seed: int, n: int, out_dir: Path) -> dict[str, Any]:
    """Draws `n` authorable tasks at `seed` and materializes each into
    `out_dir/tasks/<name>/{workspace,pristine,pristine_p2}/`, writing
    `out_dir/manifest.json` — corpus-v1's schema plus the per-task
    `pristine_p2` path and `workspace_p2_sha256`, under this
    instrument's own name. Same determinism contract as
    `corpus.generate_corpus`: a same-`(seed, n)` regeneration reproduces
    every field byte-for-byte except the grant's absolute paths."""
    out_dir = Path(out_dir)
    drawn = _draw_authorable_tasks(seed, n)

    task_entries: list[dict[str, Any]] = []
    families: dict[str, int] = {}
    for index, (task, p2_files) in enumerate(drawn):
        name = f"{task.name}-{index:04d}"
        task_dir = out_dir / "tasks" / name
        _write_workspace(task_dir / "workspace", task.files)
        _write_workspace(task_dir / "pristine", task.files)
        _write_workspace(task_dir / "pristine_p2", p2_files)
        entry = _task_manifest_entry(task, name, task_dir / "workspace", out_dir)
        entry["pristine_p2"] = str((task_dir / "pristine_p2").relative_to(out_dir))
        entry["workspace_p2_sha256"] = _workspace_sha256(p2_files)
        task_entries.append(entry)
        families[task.name] = families.get(task.name, 0) + 1

    manifest: dict[str, Any] = {
        "instrument": INSTRUMENT_PG,
        "corpus_seed": seed,
        "n": n,
        "families": dict(sorted(families.items())),
        "tasks": task_entries,
    }
    out_dir.mkdir(parents=True, exist_ok=True)
    (out_dir / "manifest.json").write_text(
        json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    return manifest
