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

Everything is rendered under **envelope-v4** (turn-4 spec §2): the prompt
carries a grant line above the verb card, rendered by the tool from the
request's own `commands`. A run-verified task sends
`[["python3","-m","unittest"]]` and reads
`Granted commands: python3 -m unittest`; every other shape and both refusal
classes send `[]` and read
`Granted commands: none — run is not available in this task`. That line is
the whole reason turn 4 exists — through v3 the post-patch decision point
was token-identical between a run-granted task and a plain one, and the
model trained on it emitted zero `run` verbs.

The **run slice verifies with a planted `unittest`** (turn-4 spec §3), not
turn 3's `py_compile`, which could not fail on a semantic defect. Each
run-verified task ships `test_<stem>.py` beside its target as an ordinary
sibling in `files`, and the proof that the verification means something is
split in two: the factory executes the test against the UNPATCHED workspace
and requires a nonzero exit (`planted_test.py`'s fails-before rule, part of
`validate_task`), and the tool executes the same argv against the PATCHED
file and refuses to render a trajectory on any nonzero exit. Either failing
is a structural rejection, never a rendered row.

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

`meta.files` is EVERY file the task carries, not just the target — a
find-shaped task's siblings and a run-verified task's planted test
included — so the post-hoc guard screens both their CONTENTS and (since
turn 4) their FILENAMES against every gate target. `target_contents` stays
as the target's own contents (`""` for a missing-target refusal, whose
target is by construction absent from `files`). A row written before
`files` existed is treated as a legacy row and falls back to the target
alone.

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
produce byte-identical corpora with zero differing rows. Concurrent
identical requests are serialized by an exclusive flock held for the
scratch directory's lifetime (see `scratch.rs`), preventing tool processes
from corrupting each other's observations.

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
  templates_python.py  8 python-lens families (the defects themselves)
  templates_run_verified.py  their run-verified wrappers: the per-family
                        PROBE table, the planted unittest, the grant
  planted_test.py      Running a planted test the way exec_run will, and
                        the fails-before rule built on it
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
  test_generate_envelope_v4.py   the grant line, on every shape and class
  test_contamination.py  rule 7, including the planted-disguised-copy test
  test_contamination_siblings.py  the two rules that read the whole
                        `files` map: sibling contents and sibling filenames
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

## `prune/` — REAP expert pruning for `qwen3_5_moe`

`tools/flywheel/prune/` is a self-contained, REAP-compatible expert pruner
for Qwen3.5/3.6 MoE (hybrid Gated-DeltaNet + MoE). Upstream
[CerebrasResearch/reap](https://github.com/CerebrasResearch/reap) cannot
load, hook or slice this architecture — five verified blockers, recorded in
`.superpowers/spikes/2026-08-21-runpod-reap-train-spike.md` §S4. We keep
REAP's **saliency math** (cited by file:line in `saliency.py`) and replace
the observer and the pruned-config/save path.

```bash
~/flywheel-venv/bin/python -m tools.flywheel.prune.cli \
    --model ~/models/hf/Qwen3.6-35B-A3B \
    --calib ~/flywheel4/corpus.jsonl \
    --samples 512 --seq-len 4096 \
    --compression 0.48 --seed 42 \
    --out ~/models/hf/Qwen3.6-35B-A3B-reap48 \
    --device cuda --dtype bf16
```

The keep-count rule is pinned in `saliency.py`:
`n_prune = floor(E * compression)` (upstream's `int()` truncation,
`reap/prune.py:261`), so **c=0.48 keeps 5 of 8 and 134 of 256**.
`--rounding ceil` prunes `ceil(E * c)` instead and keeps **133 of 256** —
crucible-labs' published REAP-48 count. No single rule gives both 5-of-8
and 133-of-256; pick deliberately.

`--metric`, `--renormalize-router-weights`, `--rounding`, `--seed` and the
calibration description are all written into the output checkpoint
(`config.json` → `reap_pruning`, plus the full kept-index lists in
`reap_pruning.json`).

A compression that would leave a layer with fewer experts than
`num_experts_per_tok` is refused with `PruneConfigurationError` and exit 2
**before** the model is calibrated or anything is written — the router
selects top-k for every token, so such a checkpoint could not route.

### Running the prune tests

They need torch + transformers, so use the venv interpreter; under the
stdlib `python3` all four modules skip cleanly and the rest of the suite
still runs.

```bash
~/flywheel-venv/bin/python -m unittest discover -s tools/flywheel/tests -t .
```

### Validated at full scale

Task B ran this pruner on `Qwen3.6-35B-A3B` on a rented A100 80 GB:
**256 → 133 experts uniformly across all 40 layers** (`--compression 0.48
--rounding ceil`, 512 calibration samples at seq-len 2048, 233,159 tokens,
seed 42), 71.9 min wall, 65.6 GiB peak CUDA. The result is coherent in pure
transformers and boots as a GGUF at parity with crucible-labs' published
REAP-48. The run also found three bugs the unit tests had missed — CUDA
index placement, missing tokenizer files, and the MTP block-count
mismatch — all now fixed and covered. See
`.superpowers/sdd/2026-08-22-reap-observer/task-B-report.md`.

Notably `saliency min = 0.0`: at least one expert took **zero** routed mass
across 233k calibration tokens. Genuinely dead experts are what make 48%
pruning survivable.

### Operational rule: never `pip install` beside a running calibration

On the first full-scale attempt, running llama.cpp's
`pip install -r requirements-convert_hf_to_gguf.txt` concurrently with a
512-sample calibration **rewrote the package tree under the running
process**: transformers 5.5.0 → 4.57.6 (no `qwen3_5_moe` module at all),
numpy 2.5.2 → 1.26.4, torch 2.9.1+cu129 → 2.11.0+cpu. The job died at
sample 500/512 with garbled line numbers and

```
FileNotFoundError: .../transformers/models/qwen3_5_moe/modeling_qwen3_5_moe.py
```

Cost: 73 minutes and $1.70 of GPU time. **Garbled tracebacks plus a missing
module file are the signature of a package tree mutating under a live
process.** Build converter and tooling dependencies into a separate venv,
or before the run starts — never alongside it. Re-verify `cuda_avail` and
the `qwen3_5_moe` import before restarting.

## train_moe.py — turn 5's bf16-LoRA recipe for qwen3_5_moe

Turns 1-4 trained a dense 14B model with unsloth QLoRA (`train.py`). Turn 5
trains the REAP-48-pruned `Qwen3.6-35B-A3B` hybrid MoE
(`Qwen3_5MoeForCausalLM`) instead, and neither turn-1-4 mechanism applies:
unsloth has no support for `qwen3_5_moe`, and bitsandbytes cannot quantize
the architecture's fused 3-D expert tensors (`experts.gate_up_proj`,
`experts.down_proj`). `train_moe.py` is the bf16 LoRA-via-peft recipe
forced by that constraint. The binding rules every flywheel recipe shares
— raw text with no chat template, completion-only loss, no EOS appended
(the sample tail is `</action>`), the fingerprint's `val_split_ids` held
out for eval only, `TrainingArguments` verbatim, and the procedure seed
`20260816` — now live in `train_common.py`, imported unchanged by both
`train.py` and `train_moe.py`.

**LoRA targets (12 module names).** Attention (`q_proj`, `k_proj`,
`v_proj`, `o_proj`), the Gated-DeltaNet linear-attention projections
(`in_proj_qkv`, `in_proj_z`, `in_proj_b`, `in_proj_a`, `out_proj`), and the
SHARED expert only (`gate_proj`, `up_proj`, `down_proj` — the only Linear
modules with those names; the routed experts are fused, non-Linear
`nn.Parameter`s peft cannot wrap). r=16, alpha=32, dropout=0, same as
turns 1-4.

**Frozen and asserted.** Routed experts (`mlp.experts.gate_up_proj`,
`mlp.experts.down_proj`) and the router (`mlp.gate.weight`, a bare
parameter) are never LoRA targets and stay frozen; `assert_frozen(model)`
walks every parameter and raises if any `.experts.` or router parameter is
trainable, returning `{"trainable": N, "total": M}` so the run log records
the real percentage — **0.1103%** measured on this turn's own 133-expert
REAP-48 checkpoint (21,166,080 / 19,194,718,848, training record §5); the
pre-registration's 0.0611% figure was the 2026-08-21 spike's measurement
against the unpruned 256-expert total, kept there for context only, not
this run's measured value. The mini-model test only checks trainable/total
< 20%, since a 4-layer/8-expert toy has a much higher LoRA-to-total ratio
than the real 40-layer/133-expert checkpoint.

**Unpacked, batch size 1** — ruled 2026-08-22: naive example-packing would
let one packed sequence's state leak across the Gated-DeltaNet layers'
recurrent state into the next example, so `train_moe.py` keeps the same
bs-1, no-packing shape `train.py` already used.

**Wrong-checkpoint refusal.** `main()` loads the checkpoint, then checks
`type(model).__name__ == "Qwen3_5MoeForCausalLM"` before doing anything
else; a mismatch prints to stderr, writes `EXIT 2` / `DONE failed`, and
returns exit code 2 without touching LoRA, the corpus, or the trainer.

**Markers.** `main()` wraps everything after `--out` is created (model
load, LoRA, freeze assertion, data, trainer, save) in a single
try/except, and writes `EXIT`/`DONE` on **every** exit path — the
wrong-checkpoint refusal (`EXIT 2`/`DONE failed`), any other unexpected
exception (`EXIT 1`/`DONE failed`, with `TRAINING FAILED: {e!r}` and the
full traceback on stderr), and success (`EXIT 0`/`DONE ok`) — so the
pod's wrapper can always read the outcome from `--out` instead of parsing
training logs.

Usage (turn 5, on the pod):

```bash
python -m tools.flywheel.train_moe --corpus /workspace/flywheel5/corpus.jsonl \
    --fingerprint /workspace/flywheel5/fingerprint.json \
    --base /workspace/Qwen3.6-35B-A3B-REAP48-ours --out /workspace/flywheel5/adapter \
    [--max-steps N] [--device cuda|cpu] [--dtype bfloat16|float32]
```

`--device cpu --dtype float32` is the CPU-smoke path exercised by
`tests/test_train_moe.py::CpuSmoke` — it is not how the pod runs the real
job. peft and accelerate must be **installed on the pod before the job
starts**, the same rule as the prune run above: never `pip install`
alongside a live process.

### Running the trainer tests

Same interpreter split as the prune tests: the venv for real assertions,
stdlib for a clean skip.

```bash
~/flywheel-venv/bin/python -m unittest discover -s tools/flywheel/tests -t .
```
