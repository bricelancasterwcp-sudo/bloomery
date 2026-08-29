"""Scratch-copy run-manifest tool for the memory-battery driver (refalsify-
battery-v2 task-2 brief; extended by the task-3 preliminary per the
task-2 shakedown's carried findings). Two mutually exclusive modes:

- **Dry mode (default)**: writes a SMALL, non-frozen manifest containing
  the first `count` tasks (frozen manifest order -- deterministic, no
  draw, no shuffle; default `DRY_TASK_COUNT = 3`) of a frozen
  memory-battery corpus, with `"dry": true` stamped at the top level so no
  downstream consumer (driver, recompute) can mistake this output for the
  frozen instrument itself. This is the design spec §6 step 2 shakedown
  tool: "3 corpus tasks through both arm configs on the live daemon
  (numbers discarded, marked DRY)".
- **Real mode (`--real` / `real=True`)**: writes a manifest containing
  ALL tasks in the frozen manifest's own order -- the manifest the real
  overnight battery run's driver invocation actually consumes. `--count`
  is rejected when combined with `--real` (real mode's count is always
  "every task in the manifest," never a caller-chosen subset) --
  enforced both at the CLI (`argparse` usage error) and in the library
  function (`ValueError`), so a direct caller gets the same guarantee a
  CLI invocation does. Real mode does **not** stamp `"dry": true` -- the
  real run's manifest is not a dry-run artifact and must not be logged,
  greppable, or mistaken for one.

**Both modes stamp `"scratch_copy": true`, unconditionally.** This is the
one property dry and real outputs always share: neither is ever the
frozen manifest itself, and no consumer of either output may confuse it
for `corpus-v1/manifest.json` proper. `"dry"` is the mode-specific
marker layered on top (present+true in dry mode, absent in real mode);
`"scratch_copy"` is the mode-independent one.

**Grant paths point at a SCRATCH COPY, never at the frozen `corpus-v1/`
tree, in EITHER mode -- found the hard way, not a design preference, and
confirmed to matter for a full 50-task real run, not just a 3-task dry
shakedown.** An earlier version of this tool re-derived grant paths
straight at `corpus_dir`'s own tracked `tasks/<name>/workspace`
directories (mirroring `corpus.py`'s `_task_manifest_entry`:
`str(workspace_dir.resolve())`). That is wrong: a live daemon task's
`run`/patch actions write into whatever `write_roots` names, so a real
dry-run boot (task-2 evidence notes,
`.superpowers/sdd/2026-08-28-refalsify-battery-v2/EVIDENCE-NOTES-DRY.md`)
mutated 3 committed corpus files in place -- caught only by `git status`
after the fact and restored via `git checkout --`. The same evidence
notes flag this as a real finding for the real run too, not just a
dry-run artifact: the real 50-task run driving the frozen manifest
directly from this worktree hits the identical problem (the frozen
manifest's grant paths still point at `memory-battery-v1`'s deleted
`.worktrees/memory-battery`), so real mode reuses the exact same
scratch-copy mechanics as dry mode rather than a separate, unaudited
path. The corpus is bytes after its freeze commit (`docs/superpowers/
evidence/2026-08-26-memory-battery-preregistration.md`'s own "Amendment
rule"): a tool that can silently corrupt it on a normal, successful run
is not safe to keep, regardless of how careful any one invocation
happens to be, and regardless of whether that invocation is a 3-task
shakedown or the full 50-task evidence run. This version copies each
subset task's `workspace/` AND its sibling `pristine/` (`driver.py`'s
`_reset_workspace` resets FROM a `pristine` sibling before every phase,
so both must exist at the scratch location) into
`out_path.parent / "tasks" / <name> / {workspace, pristine}/` -- the
identical on-disk shape `corpus.py` itself uses, so `driver.py` needs no
changes to consume either mode's output -- and points the grant at the
scratch `workspace/` copy. The frozen `corpus-v1/` tree is opened
READ-ONLY by this tool (`shutil.copytree` never writes back into its
source), and every other manifest field stays byte-identical to the
frozen source.

**Separately, why the grant paths are rewritten at all (not merely
relocated verbatim from the manifest).** `corpus.py`'s own
`generate_corpus` docstring names this precisely: "Regenerating with the
same (seed, n) -- even into a different `out_dir` -- reproduces every
field byte-for-byte except the grant's absolute-path fields, which
derive from `out_dir` by construction." The frozen
`corpus-v1/manifest.json` in THIS branch still carries
`memory-battery-v1`'s original grant paths
(`/home/brice/workspace/bloomery/.worktrees/memory-battery/...`), a
worktree that no longer exists (deleted after that battery closed out) --
driving those grants verbatim fails before the first HTTP request.
"""

from __future__ import annotations

import argparse
import json
import shutil
from pathlib import Path
from typing import Any

DRY_TASK_COUNT = 3


def generate_run_manifest(
    corpus_dir: Path,
    out_path: Path,
    count: int | None = None,
    real: bool = False,
) -> dict[str, Any]:
    """Reads `corpus_dir/manifest.json` (READ-ONLY -- never written back
    into), takes either the first `count` tasks in manifest order (dry
    mode, `real=False`, `count` defaulting to `DRY_TASK_COUNT` when
    omitted) or ALL tasks in manifest order (`real=True`; `count` must be
    `None` in this mode -- an explicit `count` alongside `real=True`
    raises `ValueError`, mirroring the CLI's usage-error rejection of
    `--count` combined with `--real`), copies each subset task's
    `workspace/` AND its sibling `pristine/` into a scratch tree beside
    `out_path` (`out_path.parent/"tasks"/<name>/{workspace,pristine}/`),
    re-derives that task's grant paths against the SCRATCH copy (never
    the frozen tree), stamps `"scratch_copy": true` unconditionally and
    `"dry": true` only when `real` is falsy, and writes the result to
    `out_path`. Raises `FileNotFoundError` if a subset task's frozen
    `workspace/` (or its `pristine/` sibling) does not exist on disk --
    fail loud, never a silent mismatch between the manifest and the
    filesystem it claims to describe. Safe to call repeatedly: each
    task's scratch `workspace/`/`pristine/` pair is fully replaced
    (`shutil.rmtree` + `copytree`), never appended to or merged."""
    corpus_dir = Path(corpus_dir)
    out_path = Path(out_path)
    manifest = json.loads((corpus_dir / "manifest.json").read_text(encoding="utf-8"))

    if real:
        if count is not None:
            raise ValueError(
                "dry_manifest: --count cannot be combined with --real "
                "(real mode always drives the manifest's full task list)"
            )
        subset = manifest["tasks"]
    else:
        effective_count = DRY_TASK_COUNT if count is None else count
        subset = manifest["tasks"][:effective_count]

    scratch_tasks_root = out_path.parent / "tasks"

    run_tasks: list[dict[str, Any]] = []
    for task in subset:
        name = task["name"]
        src_workspace = corpus_dir / "tasks" / name / "workspace"
        src_pristine = corpus_dir / "tasks" / name / "pristine"
        if not src_workspace.is_dir():
            raise FileNotFoundError(f"dry_manifest: {src_workspace} does not exist for task {name!r}")
        if not src_pristine.is_dir():
            raise FileNotFoundError(f"dry_manifest: {src_pristine} does not exist for task {name!r}")

        dst_workspace = scratch_tasks_root / name / "workspace"
        dst_pristine = scratch_tasks_root / name / "pristine"
        if dst_workspace.exists():
            shutil.rmtree(dst_workspace)
        if dst_pristine.exists():
            shutil.rmtree(dst_pristine)
        shutil.copytree(src_workspace, dst_workspace)
        shutil.copytree(src_pristine, dst_pristine)

        workspace_abs = str(dst_workspace.resolve())
        run_task = dict(task)
        run_task["grant"] = dict(task["grant"])
        run_task["grant"]["read_roots"] = [workspace_abs]
        run_task["grant"]["write_roots"] = [workspace_abs]

        # premise-gone corpora (plan Task 5): a task carrying a
        # `pristine_p2` phase-2 source gets it scratch-copied beside
        # `workspace/`/`pristine/` under the SAME sibling convention the
        # driver resolves by, and the key rewritten to the scratch copy --
        # the frozen tree stays untouchable by construction, exactly as
        # for the other two trees. Fail-loud on a manifest/filesystem
        # mismatch, same as above.
        if "pristine_p2" in task:
            src_p2 = corpus_dir / "tasks" / name / "pristine_p2"
            if not src_p2.is_dir():
                raise FileNotFoundError(f"dry_manifest: {src_p2} does not exist for task {name!r}")
            dst_p2 = scratch_tasks_root / name / "pristine_p2"
            if dst_p2.exists():
                shutil.rmtree(dst_p2)
            shutil.copytree(src_p2, dst_p2)
            run_task["pristine_p2"] = str(dst_p2.relative_to(out_path.parent))

        run_tasks.append(run_task)

    run_manifest: dict[str, Any] = dict(manifest)
    run_manifest.pop("dry", None)
    run_manifest["scratch_copy"] = True
    if not real:
        run_manifest["dry"] = True
    run_manifest["n"] = len(run_tasks)
    run_manifest["tasks"] = run_tasks

    out_path.parent.mkdir(parents=True, exist_ok=True)
    out_path.write_text(json.dumps(run_manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    return run_manifest


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--corpus-dir",
        type=Path,
        default=Path("tools/memory_battery/corpus-v1"),
        help="Frozen corpus directory (contains manifest.json). Never modified.",
    )
    parser.add_argument("--out", type=Path, required=True, help="Output path for the run manifest JSON.")
    parser.add_argument(
        "--count",
        type=int,
        default=None,
        help=f"Number of tasks in the subset (dry mode only; default {DRY_TASK_COUNT}). "
        "Rejected together with --real.",
    )
    parser.add_argument(
        "--real",
        action="store_true",
        help="Use ALL tasks in the frozen manifest's order (the real overnight run's manifest) "
        'instead of a dry subset. Does not stamp "dry": true. Mutually exclusive with --count.',
    )
    args = parser.parse_args(argv)

    if args.real and args.count is not None:
        parser.error("--count cannot be combined with --real")

    run_manifest = generate_run_manifest(args.corpus_dir, args.out, args.count, args.real)
    mode = "real" if args.real else "dry"
    print(f"dry_manifest: wrote {len(run_manifest['tasks'])} task(s) ({mode} mode) to {args.out}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
