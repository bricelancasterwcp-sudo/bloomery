# Flywheel turn 5 — the REAP-48 line: refusal honesty on a rented GPU, hybrid-aware geometry, and a keyed journal

**Date:** 2026-08-22
**Status:** Approved in conversation (rulings: scope = refusal turn on
envelope-v4 with two ride-alongs, honesty instrument deferred to its own
spec; corpus = turn-4 corpus byte-identical; recipe = unpacked bs 1, bf16
LoRA; rental = RunPod network volume, turn cap $10; baselines = REAP-48-ours
untrained, two identical boots; packaging = two branches, ride-alongs first;
`tools/evidence/recompute.py` IN).
**Lineage:** flywheel turn 4 (`2026-08-21-flywheel4-turn4-design.md`, PASS —
G4 20/20, G5 16/16 + 16/16 decided, productive run 5/5); the REAP-48 spike
(`docs/superpowers/evidence/2026-08-21-reap48-qwen36-spike.md`); the RunPod
training-fit spike (`…/2026-08-21-runpod-reap-train-spike.md`); the
REAP-48-ours prune + acceptance record
(`…/2026-08-22-reap48-ours-prune-and-acceptance.md`); the reap-observer
mini-wave (`tools/flywheel/prune/`, merged `5ed8f36`).

## 1. What this builds and why

Turn 5 is the **first training of a new line**. The base is the REAP-48
pruned Qwen3.6-35B-A3B hybrid MoE we produced ourselves
(`~/models/hf/Qwen3.6-35B-A3B-REAP48-ours`, bf16, `model.safetensors` sha
`8027ca0a…`; served as `~/models/gguf/qwen36-reap48-ours-Q4_K_M.gguf`, sha
`90e2181e…`, 11,755,624,288 B) — the only trainable 48% checkpoint of this
model that exists. It serves **resident** on the 16 GB tier and arrives with
capability maxed and refusal broken: untrained under envelope-v4 it read G4
**20/20**, G5 patch **13/16** (provisional), refuse **9/16** (crucible-labs'
Q3K build: 20/20 · 16/16 · 5/16). The failure shape is over-eager patching —
45 `done` rows on 32 fixtures, 5 grant-violation rows — so the flywheel's
job on this base is exactly one leg: **refusal honesty**, with patch held
(it sits *at* the floor).

The training step cannot run on this box (fused 3-D expert tensors cannot be
4-bit quantized; ~33 GB of experts stay bf16 — memory
`research-moe-quantized-expert-training`, parked), so turn 5 trains on a
rented A100-80GB and serves locally. That forces a recipe change (bf16 LoRA
via peft instead of unsloth QLoRA-NF4) and is the reason the recipe is
re-pinned in this spec rather than inherited.

Turn 5 therefore:

- **Holds the instrument still.** Envelope-v4, `codec-tasks-v1`,
  `codec-tasks-v4-mixed`, the v4 protocol — all frozen, none amended. The
  only new variables are the base and the recipe. The honesty instrument the
  turn-4 battery asked for (reason-grounding measured quoting discipline, not
  honesty: 6-of-6 grounded spans beside three false claims and one
  "Fixed that before emitting done" with no patch step) is **turn 6's spec**,
  not a clause here.
- **Trains on the turn-4 corpus byte-identical** (`~/flywheel4/corpus.jsonl`,
  sha `9c51a866…`, 4,561 pairs, seed 20260821, 999 patch / 450 refuse) — the
  data that took a 14B from refuse 8/16 to 16/16 while holding patch 16/16;
  nothing in the factory or envelope changes; it is also the distribution the
  pruner was calibrated on (512 samples, seed 42), stated plainly.
- **Rides along two bounded prerequisites**, merged before any boot so the
  baseline geometry is the one worth shipping: the pager's hybrid-geometry
  fixes (§2) and the `TaskStep` observability debt carried three turns (§3).
- **Pre-registers the new line's own baselines** (two identical boots of the
  untrained base at the fixed geometry) before any training step; the spike
  numbers are superseded as anchors.

## 2. Ride-along 1 — hybrid-aware geometry

The spike proved two accounting defects on this model: `kv_bytes_per_token`
charges all 40 blocks (81,920 B/tok) where llama.cpp allocates KV for the 10
full-attention layers only (20,480 B/tok — `1070.00 MiB / 54,784 cells`,
exactly 4.00×), and the Gated-DeltaNet recurrent state (`RS buffer 62.81
MiB`, per sequence, independent of window) is charged by nothing, leaving
the per-context non-KV footprint (62.81 + 493.00 MiB compute = 555.81 MiB)
171.81 MiB above `ctx_overhead_bytes` (384 MiB).

`GgufMeta` gains two **derived** fields, each with a non-hybrid fallback so
every existing GGUF and every test helper parses unchanged:

- `attention_layers: u32` = `block_count / full_attention_interval` when
  `{arch}.full_attention_interval` is present (integer division — llama.cpp's
  rule "layer *i* is full attention iff (*i*+1) % interval == 0"; an interval
  of 0 is `InvalidData`, like `head_count == 0`); else `block_count`.
  `kv_bytes_per_token` uses it: qwen35moe → `2·10·2·256·2 = 20,480`.
- `recurrent_state_bytes: u64` = over the `block_count − attention_layers`
  recurrent layers, `[(conv_kernel−1)·(inner_size + 2·group_count·state_size)
  + state_size·inner_size] · 4` bytes (llama.cpp's `n_embd_r + n_embd_s`,
  f32), from `{arch}.ssm.{conv_kernel,state_size,group_count,inner_size}`;
  0 when the keys are absent. For this GGUF: `30·(3·8192 + 524,288)·4 =
  65,863,680 B = 62.8125 MiB` — the measured buffer to the byte. A
  per-context constant.

**Charge sites** (the two the spike named): the window law's per-context
term becomes `ctx_overhead_bytes + recurrent_state_bytes`, and
`Agent::reserved_bytes = kv_bytes + ctx_overhead_bytes +
recurrent_state_bytes`. `/status` `ModelStatus` gains `recurrent_state_bytes`
(additive) and `kv_per_token` now reports the honest derived figure. The
`kv_per_token_bytes` override stays exactly as is (declared, unclamped); its
doc-comments in `config.rs` and `pager/tuning.rs` lose the "the formula
overcounts hybrids ~4×" rationale (now false) in favour of "a measured
override for geometries the formula does not model". `ctx_overhead_mib`
stays an operator knob; **turn-5 boot configs set it to 512** (≥ the measured
493 MiB compute buffer at n_ctx 54,784) and **omit the KV override**.

**Named residual, not modeled:** llama.cpp's compute buffer grows with n_ctx
(the spike's 231k boot implies ~900 MiB by subtraction). It stays covered by
the standing cushions — the file-size weights charge (≈ +0.5 GiB over the
real model buffer) and the once-counted 1 GiB global overhead — and is
recorded, not fixed, this turn.

**Consequence, pre-registered:** at no override the base's window becomes
≈ 108.7k tokens (`(15,659,433,984 − 11,755,624,288 − 1,073,741,824 −
602,734,592) / 20,480`, the last term being 512 MiB + 65,863,680 B),
vram-bound; decode tps is expected *below* the
spike's 116.7 (the 231k boot lost 20%). Reported as serving facts of the
line, never gated.

Rejected alternatives: a default KV override for the arch (hides the bug; the
spike said do not ship it); `ctx_overhead_mib ≈ 640` alone (charges the
recurrent state as an anonymous fudge instead of a derived, testable term).

## 3. Ride-along 2 — `TaskStep` observability, and the recompute tool

**The debt:** the `CodecFixture` ↔ `TaskStep` join is ordinal (three
validations stand in for a key), and a step journals only `ran python3 exit
0` — the argv that made turn 4's `py_compile` diagnosis possible survived
only inside a grant-violation string.

**The key — on `CodecFixture`.** Every `TaskStep` already carries the agent
`id`; the missing half is the verdict row's. `Event::CodecFixture` gains
`agent: Option<AgentId>` (`#[serde(default)]`; the `expect` precedent — old
rows replay with `None`). The probe knows `agent.id` when it journals the
row, so the join is `CodecFixture.agent == TaskStep.id`: exact, no `TaskSpec`
change, nothing rendered to the model, no field on `TaskStep` that is
meaningless for API tasks. *Rejected:* a `fixture`/`label` on `TaskSpec`
journaled on every step — a generic-loop change for a probe-side problem.

**The arguments — on `TaskStep`.** `Event::TaskStep` gains
`#[serde(default)] args: Vec<String>`: the action's model-supplied arguments,
verbatim and in order — `read` → `[path]` plus `"lines=a-b"` when given;
`find` → `[pattern, path]`; `patch` → `[path]` (**never the body** — landing
is re-derivable from the frozen fixture and scratch, and the body would
bloat the journal); `run` → the argv element by element; `done` and
unparseable (`?`) rows → `[]`. Grant-violation and demoted-verb rows carry
the args too (the action is parsed before it is refused). `StepReport` and
`TaskStepRecord` carry the same field: one `record_step`, two sinks, as
today. Old rows replay with `args = []`; the `journal_test.rs` compat pin
gains both fields. **No scoring, envelope, fixture, or protocol change**: the
G5 legs read verbs and outcomes as before; the prompt is byte-identical;
v1–v4 goldens stay pinned.

**`tools/evidence/recompute.py` (ruled IN).** A repo-owned recompute over a
task journal + the frozen fixture TOML: the keyed join **with the ordinal
join run alongside and asserted equal**; per-class counts, Wilson 95%,
floor and flag as separate facts; composition; the six secondary endpoints
(productive find /6, run-before-done /5, find-usage /6, per-family refuse
6/5/5, productive run /5, reason-grounding over the 11 target-present refuse
fixtures with "zero spans = unmeasured, never 100%"); grant-violation count;
verb histogram. **Tested against the committed turn-4 journals**: it must
reproduce fw4's 20/20 · 16/16 · 16/16 · 5/5 · 6/6 · 6-of-6 grounded over 4
measured rows, fw3's 16-of-19 over 5, and stock's unmeasured rows exactly, or
the tests fail. It reports; the daemon decides — it is never on the gate
path.

## 4. The training path

### 4.1 Files

- `tools/flywheel/train_common.py` — the binding rules lifted out of
  `train.py` unchanged: `load_pairs` (val split from the fingerprint),
  `PairDataset`, `collate_single` (bs 1 asserted), `tokenize_fn` (BOS on the
  prompt, completion-only labels, no EOS, `MAX_SEQ = 4096`),
  `assert_batch_shape` (tail `</action>`), the constants `LORA_R = 16`,
  `LORA_ALPHA = 32`, `PROCEDURE_SEED = 20260816`, and one
  `training_args(out, max_steps)` builder returning turn 4's exact
  `TrainingArguments` (2 epochs, bs 1 train and eval, accum 8, lr 2e-4
  cosine, warmup 20, eval every 100, log 10, bf16, `seed = 20260816`, no
  saves). `train.py` imports them; a test pins `tokenize_fn`'s output and the
  args dict to today's values; its header gets a dated note ("turn 5:
  functions moved to `train_common`, behaviour pinned by test; no
  hyperparameter or seed changed"). The 14B line is not trained this turn, so
  no measurement rests on the refactor.
- `tools/flywheel/train_moe.py` — the new line's recipe, importing the above.
  `AutoModelForCausalLM` in bf16 (asserts `Qwen3_5MoeForCausalLM`; prints
  `num_experts`); `torch.manual_seed(20260816)` immediately before
  `get_peft_model` (the peft analogue of unsloth's `random_state`);
  `LoraConfig(r=16, lora_alpha=32, lora_dropout=0.0, bias="none")` on the
  spike's twelve module names — `q_proj k_proj v_proj o_proj` (full
  attention), `in_proj_qkv in_proj_z in_proj_b in_proj_a out_proj` (Gated
  DeltaNet), `gate_proj up_proj down_proj` (which by construction match only
  the **shared expert**: routed experts are fused 3-D `nn.Parameter`s and the
  router a bare parameter — neither is a `Linear`, peft cannot touch them).
  **Freeze is asserted, not assumed**: every `.experts.` and router parameter
  has `requires_grad == False`, and the trainable count is printed and
  recorded (spike: 21.17 M = 0.0611%). Gradient checkpointing +
  `enable_input_require_grads`. Same data path, same args, same outputs as
  turn 4 (`adapter/`, `train.log`, `trainer_state.json`, `pip-freeze.txt`,
  `DONE`/`EXIT`); `--max-steps` for smoke.
- **Tests, local, CPU, under `~/flywheel-venv`** (skip cleanly under stdlib
  python, as the prune tests do): on a tiny random-init `Qwen3_5MoeConfig`
  (the prune tests' builder) — the resolved LoRA target set is exactly the
  expected module names and contains no `.experts.`/router entry; all
  expert/router params frozen; two inits under the same seed are bitwise
  equal; label masking + `</action>` tail hold on real corpus rows; a
  `--max-steps 2` run finishes with finite loss and writes the markers.

### 4.2 The recipe, as the pre-registration will state it

Base `~/models/hf/Qwen3.6-35B-A3B-REAP48-ours` (bf16; text-only
`Qwen3_5MoeForCausalLM`, `model_type qwen3_5_moe_text`, 40 layers = 10 full
attention + 30 linear, 133 experts, `mtp_num_hidden_layers: 0`; sha
`8027ca0a…`, re-verified on the pod); **bf16 LoRA via peft** (the forced
change from unsloth QLoRA-NF4); experts + router frozen; **unpacked, bs 1**
(ruled: no cross-example leakage through the 4 attention layers or the 30
recurrent layers' state — the pod's "packed to 4096" run was naive
back-to-back packing; packing is a later, separately pre-registered side
study); corpus `~/flywheel4/corpus.jsonl` copied to `~/flywheel5/corpus.jsonl`,
sha `9c51a866…` verified before and after; turn-4 fingerprint's val split
(221 pairs); seeds 20260816 (procedure identity); **bitwise reproducibility
not claimed** (A100 bf16 + the DeltaNet torch fallback), seeds recorded.
Artifact: `qwen36-reap48-flywheel5` — adapter + Q4_K_M GGUF in
`~/flywheel5/`, sha-anchored in `SHAS.txt` and the training evidence.

### 4.3 Pod runbook (the evidence carries the wrappers verbatim, turn-4 style)

- **Storage:** a RunPod **network volume** (50 GB) in the datacenter where
  A100-SXM-80GB ($1.39/h) is available — volumes are datacenter-bound, a
  named gotcha — holding the base; one-time chunked upload (6-way, ~35 min,
  sha-verified on the pod). Never re-uploaded.
- **Pod:** SXM, **150 GB** container disk (PCIe with 200+ GB hung at
  `runtime: null`; do not wait past ~5 min on `runtime: null`), image
  `runpod/pytorch:1.1.0-cu1290-torch291-ubuntu2404`; pins transformers
  5.5.0 / peft 0.20.0 / accelerate 1.14.0 / safetensors 0.8.0 / fla 0.5.2
  (torch fallback accepted; causal-conv1d **not attempted** — nvcc 13 vs
  torch cu12.9), llama.cpp at `8672290` + its `gguf` deps; **all installs
  before the job, `pip freeze` recorded, nothing installed after** (the
  $1.70 lesson); the job `setsid nohup`, polled by log files, never by
  `pgrep -f`.
- **Procedure:** a pre-registered `--max-steps 5` smoke on the real base
  first (OOM/freeze/arch check, ~3 min; part of the procedure, not a tuning
  step) → the full run detached → post-train chain on the pod: peft
  `merge_and_unload` → bf16 safetensors + tokenizer → `convert_hf_to_gguf.py
  --outtype bf16` (the source config already carries `mtp_num_hidden_layers:
  0`, so 40 blocks without patching) → `llama-quantize Q4_K_M` → sha256 of
  adapter / GGUF / corpus → download **only** the Q4_K_M GGUF (~11.8 GB,
  ~11 min chunked), the adapter (~40 MB) and the logs. Merged bf16 and the
  bf16 GGUF are scratch and die with the pod.
- **Money (upper bounds, pre-registered):** upload ≈ $0.8 · train ≈ 3.7 h
  ≈ $5.1 · evals ≈ $0.5 · post-train ≈ $0.5 · download ≈ $0.3 → ≈ $7.2 of
  the **$10 turn cap** (balance $12.96 at spec time). A persistent Monitor
  watches pods + balance hourly; any failure = pod down, report, ask before
  re-cutting; the cap is a stop rule, never a recipe change mid-run.

## 5. Gate, baselines, pre-registration, decision rule

- **Instrument — unchanged and frozen.** G4 on `codec-tasks-v1`; G5 on
  `codec-tasks-v4-mixed` under `bloomery-task-envelope-v4`; scoring per
  `2026-08-21-g5v4-protocol.md` (+ its §5 amendment); floors ≥16/20 and
  ≥13/16 per class; decided/provisional by the two-sided Wilson rule
  (bT10/R1), always stated apart from the floor. `gates.md` takes a dated
  amendment naming v4-mixed@v4 as turn 5's decided-G5 instrument *for the
  `qwen36-reap48` line*; same set and envelope as turn 4, so fw4 and fw5
  numbers are both v4 numbers and sit in one descriptive ladder (different
  bases; no causal sentence). No set, scoring, or envelope change; the only
  journal change is the additive pair of fields in §3.
- **Baselines (human-gated; after both ride-alongs are merged, featured
  build last).** REAP-48-ours **untrained**, **two identical boots**:
  `envelope = "v4"`, `g5_probe = true`, **no KV override**, `ctx_overhead_mib
  = 512`, dedicated scratch `data_dir` (never the standing drift home),
  assay 0.13.0 via `PYTHONPATH`, daemon digest must equal `90e2181e…`. Each
  boot runs both legs, so every number is measured twice. **Pre-declared
  before the first boot: boot 1 is the anchor; boot 2 is corroboration** —
  greedy says identical; a difference is a finding about the box, never a
  reason to pick. Expectations written first (spike: 20/20 · 13/16 · 9/16 —
  either answer valid); the spike's numbers are superseded as anchors; the
  window (~108.7k), `kv_per_token` 20,480, `recurrent_state_bytes`
  65,863,680 and decode tps at the fixed geometry are recorded as the line's
  serving facts.
- **Pre-registration** (committed BEFORE the training pod is cut; the volume
  upload is infrastructure and may precede it): corpus identity (`9c51a866…`;
  the turn-4 fingerprint and contamination report apply verbatim — nothing
  regenerated; the calibration overlap stated); the §4 recipe verbatim with
  its named forced changes; the baseline anchors verbatim and "what fw5 must
  do, as arithmetic"; the six secondary endpoints with denominators, plus
  two reported line facts — **grant-violation rows** (base: 5) and the **verb
  histogram** (`done` > fixtures was the over-eagerness signature); honesty
  lines; honest possibilities; cost bounds; amendment rule (separate dated
  files; nothing re-run; baselines never re-run for a nicer verdict).
- **The battery (human-gated), rule verbatim from turn 4 adapted:** fw5
  under v4, two boots as the baselines, digest match, every endpoint from the
  recompute tool (keyed join, ordinal cross-check asserted). **Success = G4
  ≥16/20 AND patch ≥13/16 AND refuse ≥13/16. Kill: G4 < 16/20 OR refuse <
  8/16 → adapter shelved, anatomy recorded.** Secondary endpoints never kill.
  The question this turn owns: *does refuse reach ≥13/16 while patch holds
  ≥13/16?*
- **Honest possibilities, pre-registered:** over-refusal drops patch below
  13 (the base sits at the floor — a refuse PASS beside a patch FAIL is a turn
  FAIL); refusal does not transfer through attention + shared-expert LoRA
  with experts/router frozen (a FAIL here is the first evidence on the parked
  expert-training question); the base's out-of-slice reads persist as grant
  violations; the bf16-trained / Q4-served gap (as turns 1–4); speed/window
  at the fixed geometry differ from the spike (reported); eval-loss stays
  uninterpreted (the turn-4 stance); torch-fallback training slower than the
  bound → the $10 stop rule; reason-grounding at ceiling with false claims
  inside it — reported with its known limitation.
- **Evidence files** (each dated the day it is produced, in
  `docs/superpowers/evidence/`): `g5v4-reap48-baselines.md` (+ journals and
  tasks JSONL, both boots), `flywheel5-preregistration.md`,
  `flywheel5-training.md` (the rental record: pod ids, costs, timings,
  losses, `pip freeze`, the sha chain — the turn's `SHAS.txt` as evidence),
  `flywheel5-battery.md` (+ JSONL), the CARRIED-DEBT append, a README line
  for the new line.

## 6. Testing posture

TDD throughout; both branches in worktrees, never the shared checkout. Rust
pins: a dropped recurrent charge fails the reservation test; a wrong
`attention_layers` fails the 20,480 golden; interval-0 and keys-absent paths
tested; `args` asserted per verb (read / find / patch / run / done / `?` /
grant-violation / demoted); `CodecFixture.agent` equals the created agent on
the FakeSubstrate probe; journal compat pins for both new fields; v1–v4
render goldens untouched. Python: `train_common` value pins; `train_moe`
tiny-model tests under the venv (skip under stdlib); the recompute tool
reproduces the committed turn-4 battery and baselines exactly and asserts
keyed == ordinal on those journals. Both suites green at every task;
`cargo test` first, featured build (`cargo build --release -p
bloomery-daemon --features vulkan`) last; boots by verified PID; no
`timeout`, no `pkill`; every GPU/pod step human-gated; evidence reviewed with
independent recomputation, anatomy from the tool, never from memory; pod
jobs detached, nothing pip-installed beside them.

## 7. Non-goals

No envelope-v5 and no honesty instrument (turn 6's spec); no new gate set;
no scoring/protocol/enforcement change (`done_trust` stays advisory); no
packing (a later, separately pre-registered side study); no router or expert
training (parked research); no default KV override, no `cpu-moe`, no
window-cap config, no modeling of the compute buffer's growth with n_ctx; no
HF publication of the bf16 checkpoint; no 14B-line training; no amendment
to any frozen set or prior evidence; no cross-envelope sentence anywhere.

## 8. Deliverable order

*Branch 1 — `turn5-ride-alongs` (PR, merged before any boot)*

1. Docs first: `gates.md` dated amendment + the baseline protocol note (two
   identical boots; boot 1 pre-declared anchor).
2. Hybrid geometry: `gguf.rs` + `geometry.rs` + pager charge sites +
   `/status` + config/tuning comment amendments + tests + a skip-if-absent
   test that parses `~/models/gguf/qwen36-reap48-ours-Q4_K_M.gguf` and
   asserts 20,480 and 65,863,680.
3. `TaskStep.args` + `CodecFixture.agent` + compat pins + probe test.
4. `tools/evidence/recompute.py` + tests against the turn-4 journals.
5. Final whole-branch review → PR → merge → featured build.

*Baselines — on master, HUMAN-GATED*

6. Two identical boots of REAP-48-ours untrained at the fixed geometry →
   `g5v4-reap48-baselines.md` + JSONL, recomputed by the tool; commit.

*Branch 2 — `flywheel5-turn5` (PR at the end)*

7. `train_common.py` refactor + `train_moe.py` + tests (venv deps checked:
   peft present or added and recorded).
8. Network volume + one-time base upload, sha-verified (HUMAN-GATED; ~$1;
   recorded in the training evidence).
9. `~/flywheel5/` (corpus copy + sha) + `flywheel5-preregistration.md`;
   committed BEFORE the training pod is cut.
10. Training pod (HUMAN-GATED): pins, `pip freeze`, the `--max-steps 5`
    smoke, the full run detached, the post-train chain, download, shas →
    `flywheel5-training.md`.
11. Battery (HUMAN-GATED): two boots of fw5 → `flywheel5-battery.md` +
    JSONL, CARRIED-DEBT append, README line; final review → PR → merge.
