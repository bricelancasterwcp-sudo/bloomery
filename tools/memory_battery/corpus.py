"""The memory-battery corpus generator (design spec §3; task-1 brief).

`generate_corpus` draws `n` distinct run-verified python tasks from the
flywheel factory at a given seed, materializes each into its own
`workspace/` directory plus a byte-identical `pristine/` snapshot, and
writes the frozen `manifest.json` that every later task in this plan
(the structural checker, the driver, recompute) consumes verbatim.

Every random draw funnels through exactly ONE `random.Random(seed)`
instance, handed straight to the factory's own
`generate.generate_candidate_tasks` (the factory's rule 3 determinism
guarantee) -- this module introduces no randomness, no clock, and no
filesystem-order dependence of its own (every directory this module
iterates is walked in sorted order).

**Family vs. name.** `Task.name` (`tools/flywheel/factory/task.py`'s
NamedTuple field) is a TEMPLATE FAMILY identifier -- e.g.
``"py_wrong_comparison_operator_run_verified"`` -- shared by every
instance the factory draws from that family (`templates_python.py` /
`templates_run_verified.py` set it to a literal, not a per-draw value).
The manifest schema pins two separate per-task fields, ``"family"`` and
``"name"``, precisely because a family repeats across a battery this
size (8 python run-verified families, up to `n=50` tasks) while the
manifest's ``"name"`` must be unique -- it names the task's own on-disk
directory. ``"family"`` therefore carries `Task.name` verbatim (so
``families: {family: count}`` is the observed distribution, computed
here rather than declared independently), and ``"name"`` is minted as
``f"{task.name}-{index:04d}"`` over the task's position in the final,
seed-ordered, trimmed list -- unique, and stable across a same-seed
regeneration since draw order (and therefore `index`) never changes for
a fixed `(seed, n)`.
"""

from __future__ import annotations

import hashlib
import json
import random
from pathlib import Path
from typing import Any

from tools.flywheel.factory import generate
from tools.flywheel.factory.task import RUN_TRAJECTORY, Task

INSTRUMENT = "memory-battery-v1"
PYTHON_LENS = "python"

# `generate_slices._TRAJECTORY_PATTERN` cycles (plain, find, run), so only
# 1 candidate slot in 3 is run-verified -- the rest (plain-shaped,
# find-shaped, or plaintext-lensed) are drawn by the factory and discarded
# here. Starting the over-draw at 6x covers that 3x ratio with headroom
# for the rare, content-random dedup drop; doubling from there gives a
# bounded BLOCKED-not-silent-shrink budget instead of an unbounded retry
# (task-1 brief step 3).
_INITIAL_OVERDRAW_MULTIPLIER = 6
_MAX_OVERDRAW_MULTIPLIER = 96

Manifest = dict[str, Any]


class CorpusExhaustedError(RuntimeError):
    """The factory could not yield `n` distinct run-verified python tasks
    at this seed within the draw budget below. This is always reported
    upstream as BLOCKED -- `generate_corpus` never silently returns fewer
    than `n` tasks (task-1 brief step 3)."""


def _run_verified_python_tasks(seed: int, count: int) -> list[Task]:
    """One (seed, count) draw: candidates -> dedup -> filter to the
    run-verified / python subset, preserving draw order throughout
    (brief: "select ONLY tasks whose trajectory is the run-verified shape
    and whose lens is python").

    A *larger* `count` at the SAME seed always reproduces the smaller
    draw's prefix byte-for-byte: `generate_slices.family_functions` picks
    each slot's family purely from the slot's index (never from an rng
    draw), and an empty `gates` list costs `gate_sampling.draw_all` zero
    extra rng consumption. Calling this again with a bigger `count` is
    therefore a safe, deterministic re-draw from a fresh `random.Random
    (seed)` -- never a continuation that could desync from a stashed rng
    state."""
    rng = random.Random(seed)
    candidates, _rejections, _draws = generate.generate_candidate_tasks(rng, count, [])
    unique, _dropped = generate.dedup_tasks(candidates)
    return [task for task in unique if task.trajectory == RUN_TRAJECTORY and task.lens == PYTHON_LENS]


def _draw_n_run_verified_python_tasks(seed: int, n: int) -> list[Task]:
    """Doubles the candidate draw count until `n` distinct run-verified
    python tasks are found, or raises `CorpusExhaustedError` once the
    draw budget is exhausted."""
    multiplier = _INITIAL_OVERDRAW_MULTIPLIER
    selected: list[Task] = []
    while multiplier <= _MAX_OVERDRAW_MULTIPLIER:
        selected = _run_verified_python_tasks(seed, n * multiplier)
        if len(selected) >= n:
            return selected[:n]
        multiplier *= 2
    raise CorpusExhaustedError(
        f"could not draw {n} distinct run-verified python task(s) at seed {seed} within "
        f"{n * _MAX_OVERDRAW_MULTIPLIER} candidate draws (found only {len(selected)})"
    )


def _workspace_sha256(files: dict[str, str]) -> str:
    """sha256 over the sorted `(relative_path, file_bytes)` sequence
    (brief-pinned, and later reused by Task 2's structural checker and
    Task 4's completeness test): each path/content pair is NUL-separated
    on both sides so no concatenation of adjacent pairs can collide
    across a different split of the same total bytes."""
    hasher = hashlib.sha256()
    for path in sorted(files):
        hasher.update(path.encode("utf-8"))
        hasher.update(b"\0")
        hasher.update(files[path].encode("utf-8"))
        hasher.update(b"\0")
    return hasher.hexdigest()


def _write_workspace(directory: Path, files: dict[str, str]) -> None:
    """Materializes `files` FLAT at `directory`'s root -- the factory's
    own workspace shape (`tools/flywheel/factory/planted_test.py`'s
    `run_python`: "every file is written at the workspace root: the
    factory's workspaces are flat")."""
    directory.mkdir(parents=True, exist_ok=True)
    for name, contents in files.items():
        (directory / name).write_text(contents, encoding="utf-8")


def _task_manifest_entry(task: Task, name: str, workspace_dir: Path, out_dir: Path) -> dict[str, Any]:
    """The pinned per-task manifest fields (task-1 brief). `grant.commands`
    comes verbatim from `task.commands` (only tuple -> list for JSON);
    `read_roots`/`write_roots` are both the task's own absolute workspace
    path (design spec §3: `grant = {read_roots: [ws], write_roots: [ws],
    commands: [...]}`)."""
    workspace_abs = str(workspace_dir.resolve())
    return {
        "name": name,
        "family": task.name,
        "workspace": str(workspace_dir.relative_to(out_dir)),
        "goal": task.goal,
        "grant": {
            "read_roots": [workspace_abs],
            "write_roots": [workspace_abs],
            "commands": [list(prefix) for prefix in task.commands],
        },
        "run_argv": list(task.run_argv),
        "search": task.search,
        "replace": task.replace,
        "target": task.target,
        "test_file": task.test_file,
        "workspace_sha256": _workspace_sha256(task.files),
    }


def generate_corpus(seed: int, n: int, out_dir: Path) -> Manifest:
    """Draws `n` distinct run-verified python tasks at `seed`, materializes
    each into its own `workspace/` + byte-identical `pristine/` snapshot
    under ``out_dir/tasks/<name>/``, writes ``out_dir/manifest.json``, and
    returns the same manifest dict that was written (task-1 brief's
    pinned schema).

    Regenerating with the same `(seed, n)` -- even into a different
    `out_dir` -- reproduces every field byte-for-byte except the grant's
    absolute-path fields, which derive from `out_dir` by construction. A
    different seed draws a different task list."""
    out_dir = Path(out_dir)
    tasks = _draw_n_run_verified_python_tasks(seed, n)

    task_entries: list[dict[str, Any]] = []
    families: dict[str, int] = {}
    for index, task in enumerate(tasks):
        name = f"{task.name}-{index:04d}"
        task_dir = out_dir / "tasks" / name
        _write_workspace(task_dir / "workspace", task.files)
        _write_workspace(task_dir / "pristine", task.files)
        task_entries.append(_task_manifest_entry(task, name, task_dir / "workspace", out_dir))
        families[task.name] = families.get(task.name, 0) + 1

    manifest: Manifest = {
        "instrument": INSTRUMENT,
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
