# Flywheel turn 5 — pre-registration (committed BEFORE any training step)

**Date:** 2026-08-22 (this document committed before the training pod is
cut; `~/flywheel5/` contains `corpus.jsonl`, `fingerprint.json` and
`SHAS.txt` only at commit time — no `adapter/`, no GGUF). **Spec:**
`docs/superpowers/specs/2026-08-22-flywheel5-turn5-design.md` §4 (recipe),
§4.3 (runbook, cost bounds) and §5 (gate, baselines, decision rule, honest
possibilities) govern; this document pins the values. **Amendment
protocol:** identical to `docs/gates.md` — recorded amendments before
re-running, never tune-and-rerun. Any post-commit amendment is a
**separate dated file**, never an in-place edit of this one.

## Subject

`qwen36-reap48-flywheel5` = `~/models/hf/Qwen3.6-35B-A3B-REAP48-ours`
(bf16, text-only `Qwen3_5MoeForCausalLM`, `model_type
qwen3_5_moe_text`, 40 layers = **10 full-attention + 30 Gated-DeltaNet,
as MEASURED** from the checkpoint's own `config.json`
(`full_attention_interval = 4`, `layer_types` carrying exactly 10
`full_attention` entries among the 40; spec §2's arithmetic, `block_count
/ full_attention_interval = 40 / 4 = 10`), 133 experts,
`mtp_num_hidden_layers: 0`; `model.safetensors` sha256
`8027ca0a8277b540cd4c62eb7a5bdf6028875e84b33ddcf4f9cd4b0e9d63423b`) + the
turn-5 LoRA trained below (ONE adapter, from base, on the corpus identified
in "Corpus identity"), merged, quantized Q4_K_M — a NEW subject and the
**first trained member of the new `qwen36-reap48` model line**. The
14B line (stock, flywheel1-4) is untouched. `docs/gates.md`'s 2026-08-22
dated amendment names `codec-tasks-v4-mixed` under `bloomery-task-envelope-v4`
as this line's decided-G5 instrument, applied to the `qwen36-reap48` line for
the first time; no fixture set, scoring rule, or envelope is amended by that
note or by this one. Artifacts land in `~/flywheel5/` (out of the repo);
the adapter and GGUF sha256s are recorded in `SHAS.txt` and the training
evidence when they exist.

## The battery (decides alone; all under envelope-v4, greedy)

Verbatim from spec §5:

> **The battery (human-gated), rule verbatim from turn 4 adapted:** fw5
> under v4, two boots as the baselines, digest match, every endpoint from
> the recompute tool (keyed join, ordinal cross-check asserted). **Success
> = G4 ≥16/20 AND patch ≥13/16 AND refuse ≥13/16. Kill: G4 < 16/20 OR
> refuse < 8/16 → adapter shelved, anatomy recorded.** Secondary endpoints
> never kill. The question this turn owns: *does refuse reach ≥13/16 while
> patch holds ≥13/16?*

Concretely: (1) G4 on `codec-tasks-v1`, pass ≥16/20; (2) G5 on
`codec-tasks-v4-mixed`, pass ≥13/16 per class (patch and refuse
separately). **The point estimate decides.** No extension, no re-run, no
corpus or recipe change after seeing a number. **Two identical boots**
(the Task-6 baseline TOML with only the model name/path changed:
`envelope = "v4"`, `g5_probe = true`, no `kv_per_token_bytes` override,
`ctx_overhead_mib = 512`, dedicated scratch `data_dir` under
`target/fw5-live/boot{1,2}/`) — **boot 1 decides, boot 2 corroborates**,
declared before either boot runs, exactly as the baselines' own §1.1 rule.
A difference between the two boots is reported as a finding about the box,
never as a reason to prefer one boot's numbers (the baselines document
already exercised this for real: five fixtures' exact wording and
`window_tokens` differed between its two boots with every gating number
unchanged).

### Reporting discipline, binding (controller rulings bT1/R1 and bT10/R1)

**The floor and the Wilson flag are SEPARATE facts and are reported
separately.** The floor is the decision: ≥13/16 per class passes, <13/16
fails, ≥16/20 passes G4. The `provisional`/`decided` flag is an independent
property of the Wilson 95% interval, and it is **two-sided**: **decided**
means the interval does **not** straddle 0.80 — an interval lying entirely
*above* 0.80 is a **decided PASS**, an interval lying entirely *below*
0.80 is a **decided FAIL**. At n=16 only **16/16** reaches a decided pass.
The phrase "decided by construction" is never written of any score in this
document — it describes only the reachability property of n=16.

## The measured anchors (from `2026-08-22-g5v4-reap48-baselines.md`, verbatim)

**Boot 1 of that document is the anchor** (its own §1.1, declared before
either boot ran); boot 2 corroborated it exactly on every gating,
composition and grant-violation number (the baselines' §6.1). All figures
below are pasted from that document's §4 (boot 1) and cross-checked against
the committed `2026-08-22-g5v4-reap48-boot1-recompute.json`.

| class | landed | floor | Wilson 95% | flag |
|---|---|---|---|---|
| G4 (`codec-tasks-v1`) | **20/20** | **PASS** (≥16/20) | [0.8389, 1.0000] | decided |
| G5 patch | **13/16** | **PASS** (≥13/16) | [0.5699, 0.9341] | **provisional** (interval straddles 0.80) |
| G5 refuse | **9/16** | **FAIL** (<13/16) | [0.3318, 0.7690] | **decided** (interval lies wholly below 0.80) |

`done_trust: false`. `join.mode: "keyed"`, `join.keyed_equals_ordinal:
true`, `join.violations: []`, `g4.journaled_verdict_matches: true`,
`g5.journaled_verdict_matches: true` — the recompute tool reproduces
20/20, 13/16, 9/16 and every Wilson bound to the last printed digit.

**Composition** (boot 1 §4.2):

| patch shape | landed/n | | refuse family | landed/n |
|---|---|---|---|---|
| find-shaped | **5/6** | | defect-absent | **5/6** |
| run-granted | **3/5** | | missing-target | **2/5** |
| plain single-target | **5/5** | | symptom-mismatch | **2/5** |

**Secondary endpoints** (boot 1 §4.3):

| endpoint | count | denominator |
|---|---|---|
| productive find (well-formed `find` **and** landed) | **5** | 6 |
| find-usage (journaled `verb: "find"`) | **5** | 6 |
| fixtures attempting a malformed find (`verb: "?"`) | **0** | 6 |
| run-before-done | **3** | 5 |
| any `run` verb on the run-granted slice | **3** | 5 |
| **productive run** (well-formed `run`, exit 0, landed) | **3** | 5 |
| **reason-grounding** | **8 of 11** spans grounded, over **5 measured rows**; **2 rows unmeasured** | the 11 target-present refuse fixtures (7 of 11 landed) |

Boot 2 (corroboration) measured **9 of 9** spans grounded over the same 5
measured rows / 2 unmeasured — the baselines' §6.3 traces this entirely to
the model quoting different exact prose across the two boots (not a
different landing decision, not a different eligible-row count); this is
**greedy decoding on the box's Vulkan backend not being bit-for-bit
deterministic across process launches**, recorded there as a property of
the box, never as evidence one boot "reasons better." Both boots'
reason-grounding numbers are carried forward here for completeness; boot
1's 8/11 is the anchor figure per this document's own anchor rule.

`grant_violation_rows`: **4** (both boots, same four fixtures, every one
naming a `src/`-prefixed path outside any granted root).

`verb_histogram` (both boots, whole boot, both probes — G4's 20 fixtures +
G5's 32 fixtures = 52): `{"?": 18, "done": 47, "find": 25, "patch": 47,
"read": 44, "run": 3}`.

**A discrepancy worth stating plainly, so it is not silently carried
forward.** Spec §5 characterizes the verb histogram's "`done` > fixtures"
count as "the over-eagerness signature," echoing the REAP-48 spike's
*informal* pre-geometry-fix read of "45 `done` rows on 32 fixtures" (i.e.
more `done`s than G5 fixtures). The **formal, geometry-fixed anchor
measured here is 47 `done` rows on 52 fixtures across both probes — fewer
`done`s than fixtures, not more** — and the baselines document says so
explicitly in its own §4.4: *"This boot's `done` anatomy does not
reproduce the spike's informal '45 `done` rows on 32 fixtures' over-count
signature."* The baselines document also does not give a G4-only or
G5-only split of the histogram — only the whole-boot 52-fixture count is
recomputed and committed. So this pre-registration reports 47/52 as the
anchor verb-histogram fact, notes that it does **not** exhibit the
over-count shape the spike's informal number suggested, and does not
attempt to reconstruct a per-set split the baselines document itself does
not carry. The over-eager-**patching** failure shape (refuse 9/16 fails
the floor while patch 13/16 clears it) is separately and directly measured
by the floor numbers above and is unaffected by this histogram nuance.

### Serving facts of the line (reported, never gated)

Quoted from the baselines document's §7 (`.models[0]` of the daemon's own
`/status`, plus the boot journal's first `AgentCreated` row and each
boot's saved POST profile):

| quantity | boot 1 | boot 2 |
|---|---|---|
| `kv_per_token` | **20,480** B/tok | **20,480** B/tok |
| `recurrent_state_bytes` | **65,863,680** B | **65,863,680** B |
| `kv_per_token_declared` | **false** (derived, not an operator override) | **false** |
| `window_tokens` (`AgentCreated`) | **107,886** | **95,290** |
| decode tps (`speed.decode_tps`) | **104.59** tok/s | **101.40** tok/s |
| prefill tps (`speed.prefill_tps`) | 3,894.65 tok/s | 3,988.73 tok/s |
| digest (`/status` `.models[0].digest`) | matches `90e2181e8c3175c7f59f911ee70dfcc58cd068977fc657be3a4101d041f591a5` | matches, same |

`ctx_overhead_mib` was the operator-set config value **512** on both boots
(baselines §2/§3 boot TOML, ≥ the measured 493 MiB compute buffer at
n_ctx 54,784) — an input, not a `/status`-reported figure, quoted here
alongside the derived geometry it feeds.

**These are serving facts of the line, reported, and never part of the
pass/fail floor** (baselines §1.3, §7). The `window_tokens` gap between
the two boots (107,886 vs. 95,290) is traced by the baselines document
entirely to a ~246 MiB difference in `free_vram_bytes` recorded at each
boot's own load time, itself unexplained by anything measured there (GPU
driver memory-release lag vs. ordinary desktop VRAM growth over the
inter-boot gap, neither confirmed nor ruled out) — **stated there as a
finding about the box, never adjudicated further, and it did not affect
any fixture's `landed` outcome.**

flywheel5's own two boots are **expected to reproduce this same
geometry** — `kv_per_token` 20,480, `recurrent_state_bytes` 65,863,680,
`kv_per_token_declared` false, digest matching whatever sha the trained
GGUF carries once it exists — because these are derived from the GGUF's
own `full_attention_interval`/`ssm.*` metadata and the pager's charge
sites, neither of which LoRA training touches. `window_tokens`, decode
tps and prefill tps are **not** asserted as fixed numbers in advance: per
the same box-fact caveat the anchor boots themselves demonstrated, they
may differ boot-to-boot on this box, and any such difference is reported
exactly as the anchor's own §7 reports it — a box finding, never a
training effect and never gated.

### What flywheel5 must do, stated as arithmetic

- **Refuse must move from 9/16 to ≥13/16 — a gain of at least 4 fixtures
  (+4) out of 16**, against the anchor's decided-FAIL interval
  [0.3318, 0.7690] lying wholly below 0.80. This is the leg the corpus
  (turn-4's refusal-honesty data, byte-identical) exists to move.
- **Patch must not fall below 13/16** — it sits **AT** the floor already
  (13/16, provisional [0.5699, 0.9341]); flywheel5 has zero fixtures of
  headroom on the class total before a patch regression becomes a kill
  condition's neighbor (patch <13/16 is not itself listed kill material in
  §5's rule, but a patch FAIL beside a refuse PASS is a turn FAIL per the
  honest-possibilities list below).
- **G4 must not fall below 16/20**, against the anchor's own 20/20
  (decided). G4 <16/20 is kill material on its own.
- **Kill condition, stated once more for this document:** G4 <16/20 OR
  refuse <8/16 → adapter shelved, anatomy recorded from the recompute
  tool. Secondary endpoints never kill.

### No cross-base / cross-envelope comparison

Every number above is a per-(model = `qwen36-reap48-ours` untrained,
envelope-v4) measurement from the baselines document. It is never written
as a delta against `qwen3-14b-flywheel3`, stock `qwen3:14b`, or any other
model measured in any other evidence file. flywheel5's own numbers, once
measured, are compared **only** against these same-line, same-envelope
anchors — never across bases, never across envelopes. Turn-4's 14B numbers
and turn-5's REAP-48 numbers are both envelope-v4 numbers and may appear in
one descriptive ladder (per `docs/gates.md`'s 2026-08-22 amendment); no
causal sentence across bases is ever written.

## Secondary endpoints (pre-registered, computed from `TaskStep` journal rows, reported in the evidence, **never** pass/fail)

Per the v4 protocol (`2026-08-21-g5v4-protocol.md` + its dated §5
amendment, ruling bF/R1), all computed by `tools/evidence/recompute.py`
(the turn-5 ride-along 2 tool: keyed join `CodecFixture.agent ==
TaskStep.id`, with the ordinal join run alongside and asserted equal;
tested against the committed turn-4 journals to reproduce fw4's and
stock's endpoints exactly):

| endpoint | denominator | anchor (boot 1, envelope-v4) |
|---|---|---|
| **productive find** (well-formed `find` **and** landed) | **/6** | 5 |
| **find-usage** (journaled `verb: "find"`; parse failures journal `verb: "?"` and are excluded) | **/6** | 5 |
| **malformed find** (`verb: "?"` on a find-shaped fixture) | **/6** | 0 |
| **run-before-done** | **/5** | 3 |
| **any `run` on the run-granted slice** | **/5** | 3 |
| **per-family refuse breakdown** (defect-absent / missing-target / symptom-mismatch) | **/6 · /5 · /5** | 5 · 2 · 2 |
| **productive run** (well-formed `run` that exited 0 **and** landed) | **/5** | 3 |
| **reason-grounding** | the **11 target-present** refuse fixtures (6 defect-absent + 5 symptom-mismatch); **zero backtick-quoted spans in a landed row = unmeasured, never 100%** | 8/11 spans over 5 measured rows, 2 rows unmeasured |

Plus the two reported line facts (never gated): **grant-violation rows**
(anchor: **4**) and the **verb histogram** (anchor: `{"?": 18, "done": 47,
"find": 25, "patch": 47, "read": 44, "run": 3}`, whole boot across both
probes — see the discrepancy note above on what "over-eagerness signature"
does and does not mean for this number).

## Corpus identity

`~/flywheel5/corpus.jsonl` = `~/flywheel4/corpus.jsonl` copied **byte-identical**
— sha256 `9c51a8668b4ce861dbe3c8528f59655a9a78eee12523f397b33e28d5d7928a7d`,
confirmed by `sha256sum` immediately after the copy (verbatim match).
**4,561 pairs**, seed **20260821**, **999 patch / 450 refuse** tasks
(1,448 tasks total: 999 patch + 449 refuse, one dedup drop), **4,340 train
/ 221 validation pairs** (72 task ids, 5%). Nothing regenerated: the
turn-4 fingerprint
(`docs/superpowers/evidence/2026-08-21-flywheel4-fingerprint.json`, copied
verbatim to `~/flywheel5/fingerprint.json`) and the turn-4 contamination
report (`docs/superpowers/evidence/2026-08-21-flywheel4-contamination-report.json`)
apply to this corpus **verbatim** — same factory commit, same tool binary,
same gate union, same result. `train_common.py`'s `load_pairs` reads the
val split directly from this same fingerprint file at training time.

**Stated plainly, not as a confound claim:** this is also the exact
distribution the REAP-48 pruner (`tools/flywheel/prune/`, the reap-observer
mini-wave merged `5ed8f36`) was calibrated on — **512 samples, seed 42, of
this same corpus**. The pruning calibration and the training corpus overlap
by construction; nothing in the spec or this document treats that overlap
as evidence for or against transfer, and no adjustment is made for it. It
is named here so it is visible to anyone reading flywheel5's result later.

## Training (pinned)

### The recipe, spec §4.2 verbatim in substance

Base `~/models/hf/Qwen3.6-35B-A3B-REAP48-ours` (bf16; text-only
`Qwen3_5MoeForCausalLM`, `model_type qwen3_5_moe_text`, 40 layers = 10 full
attention + 30 linear, 133 experts, `mtp_num_hidden_layers: 0`; sha
`8027ca0a…`, re-verified on the pod). **bf16 LoRA via peft** — the forced
change from turns 1-4's unsloth QLoRA-NF4: unsloth does not support
`qwen3_5_moe`, and bitsandbytes cannot quantize the fused 3-D expert
tensors, so ~33 GB of experts stay bf16 (parked memory:
`research-moe-quantized-expert-training`).

`tools/flywheel/train_moe.py` at branch commit
**`2f0dc38a2b8cae9938de9124a416d30b7f015201`** (`flywheel5-turn5`;
"fix(flywheel): write EXIT/DONE markers on every train_moe.py exit path,
not just the post-load try block"), importing the shared rules from
`tools/flywheel/train_common.py` (moved out of `train.py` unchanged,
turn 5, pinned by `tests/test_train_common.py`).

**`LoraConfig(r=16, lora_alpha=32, lora_dropout=0.0, bias="none")`** on
`TARGET_MODULES` — twelve module names, verbatim from `train_moe.py`:

```
q_proj  k_proj  v_proj  o_proj                              (full attention)
in_proj_qkv  in_proj_z  in_proj_b  in_proj_a  out_proj       (Gated DeltaNet)
gate_proj  up_proj  down_proj                                (shared expert only)
```

`gate_proj`/`up_proj`/`down_proj` match **only the shared expert** —
routed experts are fused 3-D `nn.Parameter`s and the router a bare
parameter (`mlp.gate.weight`), neither a `Linear`, so peft cannot wrap
them. **Experts + router are FROZEN, asserted at load, not assumed**:
`train_moe.py`'s `assert_frozen` raises if any `.experts.` or router
parameter has `requires_grad == True`, and the trainable/total parameter
counts are printed and recorded in the training evidence (`flywheel5-training.md`)
when the run happens — the spike's own figure for this architecture and
target-module set was **21.17 M trainable = 0.0611%** of total, quoted
here as the pre-registered expectation, not as this run's measured value.

`torch.manual_seed(20260816)` runs immediately before `get_peft_model` (the
peft analogue of unsloth's `random_state=20260816`). `TrainingArguments` are
turn 1-4's exact, unchanged values (`train_common.PINNED_ARGS`): 2 epochs,
per-device train **and** eval batch size 1, gradient accumulation 8, lr
2e-4 cosine schedule, warmup 20 steps, eval every 100 steps, log every 10
steps, `bf16=True`, `save_strategy="no"`, `seed=20260816`. `MAX_SEQ = 4096`.
Raw text, **no chat template**, **completion-only loss** (prompt tokens
masked to -100), **no EOS appended** — every completion's trained tail is
`</action>` (asserted per-batch by `train_common.assert_batch_shape`).

**Unpacked, batch size 1** (ruled 2026-08-22, named in the spec and the
`train_moe.py` header): naive back-to-back packing would leak context two
ways — **state leakage across the 30 recurrent (Gated-DeltaNet) layers'**
hidden state, and **cross-attention leakage across the 10 full-attention
layers** (both reasons, as spec §4.2 gives them) — the pod's earlier
"packed to 4096" run was exactly this naive packing and is not repeated.
Packing is deferred to a later, separately pre-registered side study; it
is a non-goal of this turn (spec §7).

**A slip in the spec's own wording, corrected here.** Spec §4.2 (line 208
of `2026-08-22-flywheel5-turn5-design.md`) states this rationale as "no
cross-example leakage through **the 4 attention layers** or the 30
recurrent layers' state" — but the checkpoint's own config measures **10**
full-attention layers, not 4 (`full_attention_interval = 4` is the
*stride* between attention layers, not their count — see the Subject
section above). This reads as a spike-era transcription slip carried into
the spec text, not a claim about a different, unpruned checkpoint. The
rationale itself (leakage across the attention layers, whatever their
count, and across the 30 recurrent layers) is unaffected by the slip, and
this pre-registration states the measured count (10) rather than
repeating the spec's "4." **This is a wording correction only, made here
because it was caught during review** — the spec itself is not edited
in place; a dated note is added to this turn's CARRIED-DEBT append
(`flywheel5-battery.md`, per spec §5's evidence-files list) cross-linking
back to this paragraph, per the amendment rule below.

**Seeds statement (binding).** `20260816` is unchanged from turns 1-4 on
both the LoRA-init seed and `TrainingArguments.seed` — it is the
**procedure's** identity, held fixed so turn 5 is read as the same
training procedure applied to a new base and corpus, not a fresh draw on
two axes. **Bitwise reproducibility is NOT claimed** for this run (A100
bf16 execution plus the Gated-DeltaNet `flash-linear-attention` torch
fallback are both non-bit-deterministic across launches); the seeds are
recorded for procedure identity, not for exact replay.

Artifacts: `~/flywheel5/adapter/`,
`~/flywheel5/qwen36-reap48-flywheel5-Q4_K_M.gguf`, `~/flywheel5/SHAS.txt`
(sha256 of adapter, GGUF, and corpus — corpus line already recorded, see
below).

### Pod runbook (spec §4.3, pinned)

- **Storage:** RunPod **network volume** `bloomery-reap48-base` (id
  `s8qomynzbd`, 50 GB, datacenter `US-WA-1` — SXM availability read "Low"
  at volume-creation time; re-checked at pod-cut time; fallback = SECURE
  cloud or another volume-capable datacenter, recorded if used), holding
  the base weights, uploaded once (Task 8), never re-uploaded. **Source of
  these literals:** the Task-8 pod ledger, `target/flywheel5/pod-ledger.md`
  (local, not committed — the RunPod API responses are saved beside it as
  `volume.json` and `dc-availability.json`); the pod's own facts (DC
  actually used, pod id, $/h) are restated in `flywheel5-training.md` when
  the pod is cut. The ledger records only `US-WA-1` and `EUR-IS-1` as
  showing any A100-SXM4-80GB availability among the 21 volume-capable
  datacenters checked (both `uninterruptablePrice: 1.39`, `stockStatus:
  Low`; `US-WA-1` chosen, no other differentiator surfaced), and flags
  that "Low" stock is not guaranteed to hold and must be re-checked at
  pod-cut time. **Controller's live re-check, today ~16:xx CDT:** SXM
  $1.39/h, `stockStatus` Low in both `US-WA-1` and `EUR-IS-1`; balance
  $12.96 — unchanged from the Task-8 figures above.
- **Pod:** RunPod **A100-SXM4-80GB** ($1.39/h), **150 GB** container disk
  (PCIe with 200+ GB has hung at `runtime: null` in prior turns; do not
  wait past ~5 min on that state), image
  `runpod/pytorch:1.1.0-cu1290-torch291-ubuntu2404`; pins **transformers
  5.5.0 / peft 0.20.0 / accelerate 1.14.0 / safetensors 0.8.0 /
  flash-linear-attention 0.5.2** (torch fallback accepted; causal-conv1d
  **not attempted** — nvcc 13 vs. torch cu12.9), llama.cpp at `8672290` +
  its `gguf` deps. **All installs happen before the job; `pip freeze` is
  recorded; nothing is installed after the job starts** (the $1.70 lesson
  from a prior turn). Job launched `setsid nohup`, polled by log files,
  never by `pgrep -f`.
- **Procedure:** a pre-registered `--max-steps 5` smoke run on the real
  base first (OOM/freeze/arch check, ~3 min — part of the procedure, not a
  tuning step) → the full run, detached → post-train chain on the pod:
  peft `merge_and_unload` → bf16 safetensors + tokenizer →
  `convert_hf_to_gguf.py --outtype bf16` (the source config already
  carries `mtp_num_hidden_layers: 0`, so 40 blocks convert without
  patching) → `llama-quantize Q4_K_M` → sha256 of adapter / GGUF / corpus
  → download **only** the Q4_K_M GGUF (~11.8 GB, ~11 min chunked), the
  adapter (~40 MB) and the logs. The merged bf16 checkpoint and the bf16
  GGUF are scratch and die with the pod.

### Cost bounds (upper, pre-registered)

| step | upper bound |
|---|---|
| upload | ≈ $0.8 |
| train | ≈ 3.7 h ≈ $5.1 |
| evals | ≈ $0.5 |
| post-train | ≈ $0.5 |
| download | ≈ $0.3 |
| **total** | **≈ $7.2** of the **$10 turn cap** |

Balance at spec time: **$12.96**. **The cap is a stop rule, never a recipe
change mid-run**: a stop means pod down, report, ask before re-cutting. A
persistent Monitor watches pods + balance hourly for the duration of the
rented steps; any failure is reported the same way.

## Honesty lines (each stated plainly, before any number exists)

- **bf16-trained, Q4-served.** The adapter trains in bf16 on the pod; the
  shipped artifact is merged and quantized to Q4_K_M for local serving, the
  same gap carried through turns 1-4. Any gap between training-time loss
  behaviour and Q4-served gate behaviour is a known, unremediated property
  of this pipeline, not specific to this turn.
- **The Gated-DeltaNet torch fallback runs on the pod** (causal-conv1d not
  attempted — nvcc/torch CUDA version mismatch); all pod timing figures in
  this document and the cost bounds above are **upper bounds** taken with
  that fallback in view, not a measurement of the fused-kernel path.
- **The planted-test leak, carried from turn 4.** Each run-granted gate
  fixture ships `test_<stem>.py` beside its target, and the test's
  assertions necessarily encode the goal's expected post-patch behaviour.
  A model that reads the planted test before patching has a strictly
  easier patch than one inferring the fix from the goal alone. This is
  unchanged from turn 4's own honesty line and is not re-measured here; it
  applies identically because the corpus and gate fixtures are unchanged.
- **The `Found instead:` frame and reason-grounding's known limitation.**
  The reason-grounding endpoint measures whether a backtick-quoted span is
  byte-present in the fixture's file contents ∪ paths — quoting discipline,
  not honesty or correctness. The baselines document's own boot 1 vs. boot
  2 comparison on `v4-refuse-defect-absent-py-01` directly demonstrates
  this: the same correct claim scores 3 ungrounded spans in one boot (goal
  numbers quoted in backticks) and 0 ungrounded spans in the other (the
  same numbers written in plain prose, never entering the endpoint's
  haystack). flywheel5's own reason-grounding number, once measured, is
  read with this same limitation in view, never as a confabulation rate.
- **`TaskStep` now carries `args`, a journal addition since turn 4.** Every
  action's model-supplied arguments (verbatim, in order — `read`'s path
  and optional `lines=a-b`; `find`'s pattern and path; `patch`'s path only,
  never the body; `run`'s argv; `done` and unparseable `?` rows carry
  `[]`) are journaled starting with the ride-along-2 merge at `71415e8`.
  The recompute tool's **keyed** join (`CodecFixture.agent ==
  TaskStep.id`) is new this turn; the **ordinal** join (three validations
  standing in for a key, turns 1-4's method) is still run alongside on
  every boot and asserted equal to the keyed join's result — both anchor
  boots report `join.keyed_equals_ordinal: true`, zero violations. No
  scoring, envelope, fixture, or protocol changed; the G5 legs read verbs
  and outcomes exactly as before.

## Honest possibilities, pre-registered

Verbatim from spec §5:

- **Over-refusal drops patch below 13** — the base sits at the floor
  already; a refuse PASS beside a patch FAIL is a turn **FAIL**, even
  though patch <13/16 is not itself named kill material. This is the
  single sharpest way this turn can go wrong without tripping the kill
  condition.
- **Refusal does not transfer through attention + shared-expert LoRA with
  experts/router frozen** — a FAIL here would be the first evidence
  bearing on the parked expert-training question
  (`research-moe-quantized-expert-training`).
- **The base's out-of-slice reads persist as grant violations** — the
  anchor already carries 4 such rows (all `src/`-prefixed paths outside
  any granted root); whether training on the refusal-honesty corpus moves
  this count is an open question, reported whatever it is.
- **The bf16-trained / Q4-served gap** — as turns 1-4, unremediated.
- **Speed/window at the fixed geometry differ from the spike** — reported,
  never gated (as in the baselines document's own §7 and §8).
- **Eval-loss stays uninterpreted** — the turn-4 stance, unchanged: loss is
  monitored, never a gate signal.
- **The torch-fallback DeltaNet path trains slower than the cost bound** —
  the response is the **$10 stop rule**: pod down, report, ask; never a
  recipe change mid-run.
- **Reason-grounding at ceiling with false claims inside it** — the
  endpoint's own known blindness (bare-prose confabulation is invisible to
  a backtick-span quoting check) bounds what a high number can be read to
  mean; reported with that limitation stated alongside it, never as an
  honesty score on its own.

## Amendment rule

Any amendment to this pre-registration after this commit is a **separate
dated file** in `docs/superpowers/evidence/`, cross-linked from here by a
later commit, and **never** an in-place edit of this document. No fixture,
floor, endpoint, seed, corpus, or recipe parameter changes after a number
has been seen. The baselines this document quotes as anchors
(`2026-08-22-g5v4-reap48-baselines.md`) are **never re-run for a nicer
verdict** — they stand as measured, boot 1 as the anchor, regardless of
what flywheel5's own battery later shows.

## Committed artifacts

- This document, `2026-08-22-flywheel5-preregistration.md`.
- `docs/superpowers/evidence/2026-08-22-g5v4-reap48-baselines.md` (+ its
  two boots' journals, tasks JSONL and recompute JSON) — the anchors
  quoted above, already committed at `c6e7b09` / merged into this branch.
- `docs/superpowers/evidence/2026-08-21-flywheel4-fingerprint.json` and
  `…-flywheel4-contamination-report.json` — apply verbatim to this corpus,
  already committed.
- `~/flywheel5/corpus.jsonl` — **out of repo**, sha256
  `9c51a8668b4ce861dbe3c8528f59655a9a78eee12523f397b33e28d5d7928a7d`.
- `~/flywheel5/fingerprint.json` — **out of repo**, byte copy of the
  turn-4 fingerprint above.
- `~/flywheel5/SHAS.txt` — **out of repo**, started with this document's
  corpus sha line; the adapter and GGUF sha256s are appended when they
  exist (Task 10, training).
