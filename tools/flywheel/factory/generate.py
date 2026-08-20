"""The corpus generator (design spec §3, brief rules 3-6; G5 design doc §5
extends this additively with refusal tasks; task 6a extends it additively
with gate-aware rejection sampling).

CLI: ``python3 -m tools.flywheel.factory.generate --seed N --count 1000
[--refusal-count 300] [--gate gate.toml ...] --tool <path to
flywheel-tool> --out corpus.jsonl --report fingerprint.json``

Pipeline, all driven by ONE `random.Random(seed)` instance (rule 3 —
determinism depends on a single deterministic sequence of draws):

1. Generate `count` candidate PATCH tasks, one per slot, with the slot's
   family chosen by `generate_slices.family_functions` — turn 3 (design
   doc §2) makes that a THREE-shape cycle (plain / find-shaped / run-
   verified, 333 each at count=999) on top of turn 1's 3:2 python:
   plaintext mix, all still purely position-derived. Each family draws
   from `rng`. Every candidate is structurally validated immediately
   (rule 2) — a violation is a factory bug and aborts the run.
1b. Generate `refusal_count` candidate REFUSE tasks (G5 design doc §5),
    continuing the SAME `rng` stream, cycling the six (family, lens)
    groups (`templates_refusal.GROUP_CYCLE_ORDER`) so all three families
    and both lenses stay represented — same "fail on structural violation"
    posture as patch tasks. `refusal_count` defaults to 0: omitting the
    flag reproduces turn-1 behavior byte-for-byte (no refusal generation
    code runs at all, so the rng stream driving the validation split
    below is untouched).
2. Dedup each class separately: patch tasks on normalized (goal,
   target_contents) (rule 5, unchanged); refuse tasks on normalized
   (goal, joined file contents) — `target_contents` alone doesn't exist
   for a missing-target refuse task, so the key spans every file the
   task carries instead. Drops counted per class, summed into one
   fingerprint total.
3. Every surviving task (both classes) is verified through the real
   `flywheel-tool trajectory` subprocess (design spec §2/§3 — "training
   artifacts run through the serving code"), kept alive as ONE
   long-lived process for the WHOLE run, patch and refuse tasks alike
   (Task 1's report). A patch task's `landed:false` ABORTS the entire
   run with the failing task printed to stderr (rule 4, unchanged); a
   refuse task has no landing check (task-3's wire contract: `landed` is
   trivially `true`) — instead, a response that does not carry
   `verified: "refusal"` ABORTS the run the same way, printing the task.
   Nothing is written to `--out`/`--report` on abort, either class.
4. A deterministic 5% validation split is drawn from the SAME `rng`,
   continuing its stream after ALL task generation, patch and refuse
   combined (rule 6).
5. corpus.jsonl (one pair-row per pair, in the order the task's SHAPE
   renders them — `generate_request.PAIR_NAMES`: 3 for a plain patch task,
   4 for a find-shaped or run-verified one, 2 for a refuse task) and the
   fingerprint JSON are written together at the end. The wire request and
   row `meta` formats both live in `generate_request.py`.

Task 6a's addition lives INSIDE step 1/1b, not as a separate pass: when
one or more `--gate` paths are given, every candidate (patch and refuse
alike) is screened at draw time via `gate_sampling.draw_all` /
`contamination.task_violates_gates` -- the SAME rule set the two-gate
contamination guard (`contamination.py`) applies post-hoc. A colliding
candidate is dropped and the SAME rng stream draws again for the SAME
slot until an accepted candidate is found, so `count`/`refusal_count`
candidates are still what step 2's dedup receives. Omitting `--gate`
reproduces every prior turn's behavior byte-for-byte: zero screening,
zero extra rng draws (rule 3's determinism guarantee is unaffected;
`--gate` just adds one more input the deterministic sequence depends on).
Per-rule rejection counts, the gate paths, and each gate file's sha256
are recorded in the fingerprint (`gate_rejections`, `gate_paths`,
`gates_sha256`) so it records exactly what a run was screened against. A
gate set so dense that rejection sampling cannot productively continue
aborts via `gate_sampling.GateOverlapTooDenseError` (never spins forever,
never silently under-fills) -- caught in `main()` and routed through
`fail()` for the same consistent stderr-and-nonzero-exit contract every
other factory-bug abort in this file already uses.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import random
import sys
from pathlib import Path

from tools.flywheel.factory import (
    gate_sampling,
    generate_refusal,
    generate_request,
    generate_slices,
    templates,
)
from tools.flywheel.factory.contamination import GateFixture, load_gate_fixtures, normalize
# `AnyTask` lives with the request/row format it exists to describe, so
# there is exactly one definition of "a task of either class".
from tools.flywheel.factory.generate_request import AnyTask
from tools.flywheel.factory.task import RefusalTask, Task
from tools.flywheel.factory.toolclient import ToolClient

VALIDATION_SPLIT_FRACTION = 0.05


def fail(message: str) -> None:
    """A factory bug: printed to stderr, run aborted nonzero. Called
    before any output file is opened, so a failed run leaves no
    corpus/report behind — never a silently-dropped or partial result
    (rule 4)."""
    print(f"flywheel factory: FATAL: {message}", file=sys.stderr)
    raise SystemExit(1)


def generate_candidate_tasks(
    rng: random.Random, count: int, gates: list[GateFixture]
) -> tuple[list[Task], dict[str, int], int]:
    """Calls each slot's template family in order, validating every
    result immediately (rule 2 -- a structurally invalid task is always a
    factory bug, never dropped silently) and, when `gates` is non-empty,
    screening it against them (task 6a) via `gate_sampling.draw_all`: a
    colliding candidate is dropped and the SAME slot redraws from the
    SAME rng stream. Returns (accepted tasks, gate_rejections by rule,
    total candidate draws). `gates=[]` is byte-identical to the
    pre-task-6a code path -- one draw per slot, no extra rng consumption."""
    return gate_sampling.draw_all(
        rng, generate_slices.family_functions(count), templates.validate_task, gates, fail
    )


def dedup_tasks(tasks: list[Task]) -> tuple[list[Task], int]:
    """Rule 5: normalized (goal, target_contents) uniqueness, first
    occurrence wins, input order preserved. `seen` is a `set` used only
    for O(1) membership testing (never iterated), so this stays
    deterministic despite Python's randomized string hashing."""
    seen: set[tuple[str, str]] = set()
    unique: list[Task] = []
    dropped = 0
    for task in tasks:
        target_contents = task.files[task.target]
        key = (normalize(task.goal), normalize(target_contents))
        if key in seen:
            dropped += 1
            continue
        seen.add(key)
        unique.append(task)
    return unique, dropped


def _verify_and_build_rows(assigned: list[tuple[str, AnyTask]], tool_path: Path) -> tuple[list[dict], int]:
    """Rule 4, extended additively for refuse tasks: every task goes
    through flywheel-tool `trajectory`, patch and refuse alike, over ONE
    long-lived subprocess for the whole run (Task 1's report). A patch
    task's `landed:false` ABORTS generation with the task printed
    (unchanged). A refuse task has no landing check (task-3's wire
    contract: `landed` is trivially `true` for refuse) — instead, a
    response missing `verified: "refusal"` ABORTS the same way, since that
    is the only signal that the tool actually exercised the refuse path
    rather than a vacuous success."""
    rows: list[dict] = []
    total_pairs = 0
    with ToolClient(tool_path) as client:
        for task_id, task in assigned:
            request = generate_request.build_trajectory_request(task)
            response = client.trajectory(request)

            if "error" in response:
                fail(f"flywheel-tool error for task {task_id} ({task.name}): {response['error']}\ngoal: {task.goal}")

            if isinstance(task, RefusalTask):
                generate_refusal.verify_refusal_response(task_id, task, response, fail)
            elif not response.get("landed", False):
                fail(
                    f"task {task_id} ({task.name}) did not land -- reference patch failed to "
                    f"apply through the real tool. This is always a factory bug, never dropped "
                    f"silently.\n"
                    f"goal: {task.goal}\n"
                    f"target: {task.target}\n"
                    f"search: {task.search!r}\n"
                    f"replace: {task.replace!r}\n"
                    f"landing_detail: {response.get('landing_detail')}"
                )

            pairs = response["pairs"]
            expected_pair_names = generate_request.expected_pair_names(task)
            if len(pairs) != len(expected_pair_names):
                fail(
                    f"task {task_id} ({task.name}) returned {len(pairs)} pairs, expected "
                    f"{len(expected_pair_names)} ({'/'.join(expected_pair_names)})"
                )

            for pair_name, pair in zip(expected_pair_names, pairs):
                rows.append(
                    {
                        "prompt": pair["prompt"],
                        "completion": pair["completion"],
                        "meta": generate_request.row_meta(task_id, task, pair_name),
                    }
                )
                total_pairs += 1
    return rows, total_pairs


def _validation_split(rng: random.Random, sorted_task_ids: list[str]) -> list[str]:
    """Rule 6: 5% of task_ids, deterministic from seed. Draws from the
    SAME rng that generated the tasks (continuing its stream, not a fresh
    one), over a sorted LIST (never a `set`) so the draw is reproducible."""
    val_count = round(len(sorted_task_ids) * VALIDATION_SPLIT_FRACTION)
    if val_count <= 0:
        return []
    return sorted(rng.sample(sorted_task_ids, val_count))


def parse_args(argv: list[str] | None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Generate the flywheel training corpus (design spec §3; G5 design doc §5).")
    parser.add_argument("--seed", type=int, required=True)
    parser.add_argument("--count", type=int, required=True, help="Number of PATCH (repair) tasks to generate.")
    parser.add_argument(
        "--refusal-count",
        type=int,
        default=0,
        help=(
            "Number of REFUSE (honest-refusal) tasks to generate, continuing the same rng "
            "stream after the patch tasks (G5 design doc §5). Defaults to 0: omitting this "
            "flag reproduces turn-1 behavior byte-for-byte."
        ),
    )
    parser.add_argument(
        "--gate",
        action="append",
        dest="gates",
        type=Path,
        metavar="PATH",
        help=(
            "Path to a gate TOML (e.g. codec-tasks-v2-mixed.toml). Repeatable (task 6a): every "
            "candidate task -- patch and refuse alike -- is screened at draw time against the "
            "UNION of every --gate given, using the same rules the two-gate contamination guard "
            "(contamination.py) applies post-hoc. A colliding candidate is dropped and redrawn "
            "from the same seeded stream, never silently kept and never silently under-filling "
            "the requested count. Omitting this flag reproduces prior-turn behavior byte-for-"
            "byte: zero screening, zero extra rng draws."
        ),
    )
    parser.add_argument("--tool", type=Path, required=True, help="Path to the flywheel-tool binary (or a stub).")
    parser.add_argument("--out", type=Path, required=True, help="Output corpus.jsonl path.")
    parser.add_argument("--report", type=Path, required=True, help="Output fingerprint JSON path.")
    return parser.parse_args(argv)


def _load_gates(gate_paths: list[Path]) -> list[GateFixture]:
    """Loads and unions every `--gate` TOML's fixtures (task 6a) -- the
    same parser the contamination guard itself uses, so a candidate is
    screened against exactly the fixtures a post-hoc guard run would
    see."""
    gates: list[GateFixture] = []
    for gate_path in gate_paths:
        gates.extend(load_gate_fixtures(gate_path))
    return gates


def _gates_sha256(gate_paths: list[Path]) -> dict[str, str]:
    """The fingerprint's `gates_sha256` field: each gate FILE's sha256
    (raw bytes, not its parsed fixtures), so the fingerprint records
    exactly what was screened against -- a later edit to the gate TOML
    changes this hash even if `check_corpus`'s comparator would not
    notice (e.g. reordering fixtures)."""
    return {str(path): hashlib.sha256(path.read_bytes()).hexdigest() for path in gate_paths}


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv)
    gate_paths: list[Path] = args.gates or []
    gates = _load_gates(gate_paths)

    rng = random.Random(args.seed)
    try:
        candidate_tasks, patch_gate_rejections, _patch_draws = generate_candidate_tasks(rng, args.count, gates)
    except gate_sampling.GateOverlapTooDenseError as exc:
        fail(str(exc))
    unique_tasks, dropped = dedup_tasks(candidate_tasks)
    assigned: list[tuple[str, AnyTask]] = [
        (f"s{args.seed}-{i:06d}", task) for i, task in enumerate(unique_tasks)
    ]

    refusal_dropped = 0
    refusal_gate_rejections: dict[str, int] = {}
    if args.refusal_count > 0:
        try:
            candidate_refusal_tasks, refusal_gate_rejections, _refusal_draws = (
                generate_refusal.generate_candidate_refusal_tasks(rng, args.refusal_count, gates, fail)
            )
        except gate_sampling.GateOverlapTooDenseError as exc:
            fail(str(exc))
        unique_refusal_tasks, refusal_dropped = generate_refusal.dedup_refusal_tasks(candidate_refusal_tasks)
        assigned.extend(
            (f"s{args.seed}-refuse-{i:06d}", task) for i, task in enumerate(unique_refusal_tasks)
        )
    total_dropped = dropped + refusal_dropped

    gate_rejections: dict[str, int] = {}
    for rejections in (patch_gate_rejections, refusal_gate_rejections):
        for rule, n in rejections.items():
            gate_rejections[rule] = gate_rejections.get(rule, 0) + n

    rows, total_pairs = _verify_and_build_rows(assigned, args.tool)

    sorted_ids = sorted(task_id for task_id, _task in assigned)
    val_split_ids = _validation_split(rng, sorted_ids)

    with args.out.open("w", encoding="utf-8", newline="\n") as f:
        for row in rows:
            f.write(json.dumps(row, sort_keys=True) + "\n")
    corpus_sha256 = hashlib.sha256(args.out.read_bytes()).hexdigest()

    tasks_by_template: dict[str, int] = {}
    tasks_by_lens: dict[str, int] = {}
    # The repair slice's shape split (turn-3 design doc §2/§5: "exact slice
    # counts pre-registered before training"), recorded so a run's actual
    # split is readable off the fingerprint rather than recomputed from the
    # corpus. Refuse tasks carry no trajectory and are not counted here.
    tasks_by_trajectory: dict[str, int] = {}
    for _task_id, task in assigned:
        tasks_by_template[task.name] = tasks_by_template.get(task.name, 0) + 1
        tasks_by_lens[task.lens] = tasks_by_lens.get(task.lens, 0) + 1
        if isinstance(task, Task):
            tasks_by_trajectory[task.trajectory] = tasks_by_trajectory.get(task.trajectory, 0) + 1

    fingerprint = {
        "seed": args.seed,
        "tasks_by_template": dict(sorted(tasks_by_template.items())),
        "tasks_by_lens": dict(sorted(tasks_by_lens.items())),
        "tasks_by_trajectory": dict(sorted(tasks_by_trajectory.items())),
        "pairs": total_pairs,
        "dedup_dropped": total_dropped,
        "corpus_sha256": corpus_sha256,
        "val_split_ids": val_split_ids,
        "gate_paths": [str(path) for path in gate_paths],
        "gates_sha256": _gates_sha256(gate_paths),
        "gate_rejections": dict(sorted(gate_rejections.items())),
    }
    args.report.write_text(json.dumps(fingerprint, indent=2, sort_keys=True) + "\n", encoding="utf-8")

    print(
        f"flywheel factory: wrote {len(assigned)} task(s) / {total_pairs} pair(s) to {args.out} "
        f"({total_dropped} dedup drop(s)); fingerprint at {args.report}",
        file=sys.stderr,
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
