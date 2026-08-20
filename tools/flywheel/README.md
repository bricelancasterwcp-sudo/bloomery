# The flywheel task factory (turn 1, design spec §3)

Generates a verified SFT corpus for the qwen3:14b flywheel and proves it
shares nothing with the frozen G4 gate set (`codec-tasks-v1`). Every
training pair is produced by shelling out to the real `flywheel-tool`
binary (Task 1) — this package never re-implements prompt rendering or
patch landing; see `docs/superpowers/specs/2026-08-16-flywheel-14b-design.md`
§2-§3.

## Requirements

- Python 3.11+ (stdlib only — no pip dependencies for the factory itself).
- `flywheel-tool`, built once:
  ```bash
  cargo build --release -p bloomery-daemon --bin flywheel-tool
  ```
- Tests use stdlib `unittest` (this box does not have `pytest` importable;
  if your box does, `python3 -m pytest tools/flywheel/tests -q` runs the
  same test modules unchanged — they are plain `unittest.TestCase`
  classes, which pytest collects natively).

## Generate a corpus

```bash
python3 -m tools.flywheel.factory.generate \
  --seed 20260816 --count 999 \
  --tool target/release/flywheel-tool \
  --out corpus.jsonl --report fingerprint.json
```

(`--count` a multiple of 3 splits the three trajectory shapes evenly —
see the table below.)

Each surviving task yields one JSONL line per pair, in the order its
TRAJECTORY SHAPE renders them (turn 3; `generate_request.PAIR_NAMES`):

| `meta.trajectory` | pairs | sequence |
| ----------------- | ----- | -------- |
| `plain`           | 3     | `read`, `patch`, `done` |
| `find`            | 4     | `find`, `read`, `patch`, `done` |
| `run`             | 4     | `read`, `patch`, `run`, `done` |

(plus 2 — `read`, `done` — for a refuse task, which carries no
`trajectory`). The repair slice cycles the three shapes by slot position,
333 of each at `--count 999`.

```json
{"prompt": "...", "completion": "...", "meta": {
  "task_id": "s20260816-000000", "template": "py_inverted_boolean",
  "lens": "python", "pair": "read", "trajectory": "plain",
  "goal": "...", "target": "...", "target_contents": "...",
  "files": {"<path>": "<contents>"}, "search": "..."
}}
```

A find-shaped row additionally carries `meta.find_pattern`, and a
run-verified row `meta.run_argv` — each present only on the shape that
owns it, mirroring the wire request that produced it.

`meta.goal`/`target`/`target_contents`/`files`/`search` are a superset of
the brief's required `meta` fields (`task_id`/`template`/`lens`/`pair`) —
`contamination.py` needs the raw pre-tool task data to compare against
the gate set without re-parsing rendered prompts (which would recreate
the exact prompt-drift risk the design spec's §2 rules out).

`meta.files` is EVERY file the task carries, not just the target, so the
post-hoc guard screens sibling files too; `target_contents` stays as the
target's own contents (`""` for a missing-target refusal, whose target is
by construction absent from `files`). A row written before `files`
existed is treated as a legacy row and falls back to the target alone.

`fingerprint.json`:

```json
{
  "seed": 20260816,
  "tasks_by_template": {"py_inverted_boolean": 26, "...": 0},
  "tasks_by_lens": {"python": 735, "plaintext": 264},
  "tasks_by_trajectory": {"find": 333, "plain": 333, "run": 333},
  "pairs": 3663,
  "dedup_dropped": 0,
  "corpus_sha256": "...",
  "val_split_ids": ["s20260816-000016", "..."]
}
```

Those are the real numbers a `--seed 20260816 --count 999` patch-only run
produces. `tasks_by_lens` is python-heavy (735:264, not turn 1's 3:2)
because the run-verified third of the slice is lens-py only — there is no
plaintext verification to run — so the plain and find slices carry the
whole plaintext share.

`val_split_ids` (~5% of surviving task_ids, deterministic from seed) is
loss-monitoring only — the training task must filter these task_ids out
of the train set and never let them influence the G4 gate decision
(design spec §3/§6).

## The determinism rule

The entire run — which template family generates each task slot, every
identifier/value/word choice inside a template, and which task_ids land
in the validation split — is driven by **exactly one `random.Random(seed)`
instance**, consumed in a fixed, position-determined order. No wall-clock
reads, no iteration over a plain `set` where order would leak into the
output (Python randomizes string-hash-derived `set` iteration order per
process by default — `random.Random.sample`/`.choice` over a `set` is
**not** reproducible across runs even with the same seed; every value
pool in `wordlists.py` is a `tuple`, never a `set`).

Consequence: `--seed N` with the same `--count` twice produces a
byte-identical `corpus.jsonl` and an identical `fingerprint.json`. This
is a machine-checked property (`tests/test_generate.py`'s
`DeterminismTest`), not just documentation.

### The determinism law holds without exception

The determinism law above extends end-to-end to every task shape,
including the find shape. Ruling bT7/R1 (2026-08-20) fixed the tool's
`Scratch::materialize` to name its scratch directory from a content hash
of the request identity, rather than the tool's PID. Identical requests
now materialize at identical paths, so two same-seed runs of the factory
produce byte-identical corpora with zero differing rows.

Find-shaped rows still carry real, unmodified absolute paths in their
observations — `exec_find` emits `{canonicalized absolute path}:{lineno}:
{line}` exactly as rendered by the executor. Those paths are deterministic
because the executor's inputs are reproducible; the factory never rewrites
observation text.

`tests/test_generate_trajectories.py`'s `RealToolDeterminismBoundaryTest`
enforces this against the real binary: two runs at the same seed must
produce byte-identical corpora (zero differing rows), and find rows must
still embed a real absolute scratch path. If anything ever becomes
nondeterministic, that test fails.

## Verify contamination

```bash
python3 -m tools.flywheel.factory.contamination \
  --corpus corpus.jsonl \
  --gate crates/bloomery-daemon/fixtures/codec-tasks-v1.toml \
  --out contamination-report.json
```

Exits nonzero if the corpus shares an exact/normalized goal, the contents
of ANY file a task carries (target or sibling), target filename, or
search string with any gate fixture, or if
any corpus goal is a >= 0.8 Jaccard token-set near-duplicate of a gate
goal. Must be run — and reported clean — before any training step (design
spec §6: this report is part of the pre-registration record).

## Package layout

```
factory/
  task.py             Task NamedTuple + structural validator (brief rule 2)
  wordlists.py         Domain vocabulary (10 unrelated themes + shared pools),
                        verified disjoint from the gate set's vocabulary
  templates_python.py  8 python-lens families + their run-verified wrappers
  templates_text.py    5 plaintext-lens template families
  templates_multifile_python.py  3 find-shaped, multi-file python families
  templates_multifile_text.py    2 find-shaped, multi-file text families
  templates.py         Re-exports + the family registries + the rule-1
                        disjointness assertion
  goal_phrasing.py      Shared goal-sentence skeletons, one set per shape
  contamination.py     GATE_VOCABULARY + the contamination comparator + CLI
  generate.py           The generator CLI (dedup, verification, fingerprint)
  generate_slices.py    Which family fills which slot (the shape/lens cycle)
  generate_request.py   The wire request + corpus row `meta` format
  toolclient.py         One long-lived flywheel-tool subprocess wrapper
tests/
  test_templates.py      rules 1-2 (registries, vocabulary, shared rules)
  test_task_validation.py      validate_task's per-shape branches
  test_templates_multifile.py  the find-shaped families + the run wrappers
  test_generate.py       rules 3-6 (+ one real-binary integration test)
  test_generate_trajectories.py  the three-shape slice cycle, end to end
  test_contamination.py  rule 7, including the planted-disguised-copy test
  fixtures/               canned stub tools used by test_generate.py
```

(Turn 2's refusal modules — `templates_refusal*.py`, `generate_refusal.py`
— and task 6a's `gate_sampling.py` are documented in their own module
docstrings.)

## Running the tests

```bash
python3 -m unittest discover -s tools/flywheel/tests -p "test_*.py" -v
```

The real-binary integration test in `test_generate.py`
(`RealToolIntegrationTest`) auto-skips if
`target/release/flywheel-tool`/`target/debug/flywheel-tool` isn't built
yet; build it first to exercise it:

```bash
cargo build --release -p bloomery-daemon --bin flywheel-tool
```
