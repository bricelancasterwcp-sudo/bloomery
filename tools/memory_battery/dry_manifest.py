"""Dry-manifest subset tool for battery shakedowns (refalsify-battery-v2
task-2 brief; design spec §6 step 2: "3 corpus tasks through both arm
configs on the live daemon (numbers discarded, marked DRY)").

Writes a SMALL, non-frozen manifest containing the first `count` tasks
(frozen manifest order -- deterministic, no draw, no shuffle) of a frozen
memory-battery corpus, with `"dry": true` stamped at the top level so no
downstream consumer (driver, recompute) can mistake this output for the
frozen instrument itself.

**Grant paths point at a SCRATCH COPY, never at the frozen `corpus-v1/`
tree -- found the hard way, not a design preference.** An earlier version
of this tool re-derived grant paths straight at `corpus_dir`'s own
tracked `tasks/<name>/workspace` directories (mirroring `corpus.py`'s
`_task_manifest_entry`: `str(workspace_dir.resolve())`). That is wrong: a
live daemon task's `run`/patch actions write into whatever `write_roots`
names, so a real dry-run boot (task-2 evidence notes,
`.superpowers/sdd/2026-08-28-refalsify-battery-v2/EVIDENCE-NOTES-DRY.md`)
mutated 3 committed corpus files in place -- caught only by `git status`
after the fact and restored via `git checkout --`. The corpus is bytes
after its freeze commit (`docs/superpowers/evidence/
2026-08-26-memory-battery-preregistration.md`'s own "Amendment rule"): a
tool that can silently corrupt it on a normal, successful run is not
safe to keep, regardless of how careful any one invocation happens to
be. This version instead copies each subset task's `workspace/` AND its
sibling `pristine/` (`driver.py`'s `_reset_workspace` resets FROM a
`pristine` sibling before every phase, so both must exist at the scratch
location) into `out_path.parent / "tasks" / <name> / {workspace,
pristine}/` -- the identical on-disk shape `corpus.py` itself uses, so
`driver.py` needs no changes to consume it -- and points the grant at the
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


def generate_dry_manifest(corpus_dir: Path, out_path: Path, count: int = DRY_TASK_COUNT) -> dict[str, Any]:
    """Reads `corpus_dir/manifest.json` (READ-ONLY -- never written back
    into), takes the first `count` tasks in manifest order, copies each
    subset task's `workspace/`+`pristine/` pair into a scratch tree beside
    `out_path` (`out_path.parent/"tasks"/<name>/{workspace,pristine}/`),
    re-derives that task's grant paths against the SCRATCH copy (never
    the frozen tree), stamps `"dry": true`, and writes the result to
    `out_path`. Raises `FileNotFoundError` if a subset task's frozen
    `workspace/` (or its `pristine/` sibling) does not exist on disk --
    fail loud, never a silent mismatch between the manifest and the
    filesystem it claims to describe. Safe to call repeatedly: each
    task's scratch `workspace/`/`pristine/` pair is fully replaced
    (`shutil.rmtree` + `copytree`), never appended to or merged."""
    corpus_dir = Path(corpus_dir)
    out_path = Path(out_path)
    manifest = json.loads((corpus_dir / "manifest.json").read_text(encoding="utf-8"))
    subset = manifest["tasks"][:count]

    scratch_tasks_root = out_path.parent / "tasks"

    dry_tasks: list[dict[str, Any]] = []
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
        dry_task = dict(task)
        dry_task["grant"] = dict(task["grant"])
        dry_task["grant"]["read_roots"] = [workspace_abs]
        dry_task["grant"]["write_roots"] = [workspace_abs]
        dry_tasks.append(dry_task)

    dry_manifest: dict[str, Any] = dict(manifest)
    dry_manifest["dry"] = True
    dry_manifest["n"] = len(dry_tasks)
    dry_manifest["tasks"] = dry_tasks

    out_path.parent.mkdir(parents=True, exist_ok=True)
    out_path.write_text(json.dumps(dry_manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    return dry_manifest


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--corpus-dir",
        type=Path,
        default=Path("tools/memory_battery/corpus-v1"),
        help="Frozen corpus directory (contains manifest.json). Never modified.",
    )
    parser.add_argument("--out", type=Path, required=True, help="Output path for the dry manifest JSON.")
    parser.add_argument("--count", type=int, default=DRY_TASK_COUNT, help="Number of tasks in the subset.")
    args = parser.parse_args(argv)

    dry_manifest = generate_dry_manifest(args.corpus_dir, args.out, args.count)
    print(f"dry_manifest: wrote {len(dry_manifest['tasks'])} task(s) to {args.out}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
