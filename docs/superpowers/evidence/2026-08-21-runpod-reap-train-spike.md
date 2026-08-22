# RunPod REAP + MoE-training rental spike — Qwen3.6-35B-A3B

Date: 2026-08-22 (UTC; launched evening of 2026-08-21 local)
Operator: agent, under Brice's explicit go ($20 RunPod credit, $10 hard cap for this spike)
Nothing from this spike is kept in the repo. This file is the only output.

**Headline: pruning is BLOCKED (REAP cannot touch this architecture without real
surgery). Training is PROVEN: bf16 LoRA on the UNPRUNED 35B-A3B fits on one
80 GB A100 and steps cleanly at 2.12 s/step at seq 4096.**

Total spend: **$0.6828** of the $10 cap (6.8%). Pods remaining: **0**.

---

## Cost and pod ledger

| | Pod 1 | Pod 2 |
|---|---|---|
| id | `7uqx7spz1mtgb3` | `4kxq7cqcgvsbrr` |
| GPU | NVIDIA A100 80GB PCIe | NVIDIA A100-SXM4-80GB |
| price | $1.19/h | $1.39/h |
| container disk | 260 GB | 150 GB |
| created (UTC) | 04:58:43 | 05:12:40 |
| terminated (UTC) | 05:12:25 | 05:30:00 |
| wall | 13.7 min | 17.3 min |
| outcome | **never booted** — `runtime: null` for 13.5 min, no port mapping, proxy SSH refused. Killed and re-cut. | full success |

Balance: **$20.000000 → $19.317188**, spend **$0.682812** (settled a few minutes
after termination; the reading immediately at teardown was $0.570558).
Verified zero pods remaining via **both** REST `GET /pods` (`POD_COUNT 0`) and
GraphQL `myself { pods }` (`PODS 0`), checked twice.

**Ops lesson (pod 1):** a Community-Cloud pod that reports `desiredStatus:
RUNNING` with `runtime: null` and no `portMappings` is *not* booting — it is
stuck. It bills the whole time. The likely cause was `containerDiskInGb: 260`
being unsatisfiable on the community host. Dropping to 150 GB (legitimate once
pruning was ruled out) scheduled in 4.75 min. **Do not wait more than ~5 minutes
on `runtime: null`; kill and re-cut.**

---

## S1 — Environment (pod 2)

- GPU: NVIDIA A100-SXM4-80GB, 81920 MiB
- Driver 595.71.05, driver CUDA 13.2
- Image: `runpod/pytorch:1.1.0-cu1290-torch291-ubuntu2404`
- torch **2.9.1+cu129**, Python 3.12, 150 GB overlay disk
- Image ships **no numpy**; pip is PEP-668 externally-managed, so every install
  needed `--break-system-packages` (fine on a disposable pod; a venv would have
  meant re-downloading torch).

Version deltas vs `~/flywheel4/pip-freeze.txt`:

| package | flywheel4 (home box) | this pod | note |
|---|---|---|---|
| torch | 2.11.0 | **2.9.1+cu129** | image-supplied, not overridden |
| transformers | 5.5.0 | 5.5.0 | matched |
| peft | 0.20.0 | 0.20.0 | matched |
| accelerate | 1.14.0 | 1.14.0 | matched |
| safetensors | 0.8.0 | 0.8.0 | matched |
| flash-linear-attention | — | 0.5.2 | installed, imports OK |
| causal-conv1d | — | **FAILED TO BUILD** | see below |

### causal-conv1d failure (verbatim)

```
urllib.error.HTTPError: HTTP Error 404: Not Found
RuntimeError: ('The detected CUDA version (%s) mismatches the version that was used to compilePyTorch (%s). Please make sure to use the same CUDA versions.', '12.9', '13.0')
ERROR: Failed building wheel for causal-conv1d
```

Two causes stacked: no prebuilt wheel for this torch/CUDA combo (404), and the
image's **nvcc is CUDA 13.0 while torch was built against 12.9**, so the source
build refuses. Consequence, printed by transformers at load:

```
The fast path is not available because one of the required library is not installed.
Falling back to torch implementation.
```

**Every timing below is therefore the SLOW torch fallback for the Gated-DeltaNet
linear-attention layers (36 of 40 layers). Real numbers with the fast path will
be equal or better — treat these as conservative upper bounds.**

Fix for next time: pick an image whose nvcc matches torch's CUDA, or
`pip install nvidia-cuda-nvcc-cu12==12.9.*` before building, or find a prebuilt
`causal-conv1d` wheel for torch 2.9/cu129.

---

## S3 — Download

`hf download Qwen/Qwen3.6-35B-A3B --local-dir /workspace/models/qwen36`
with `HF_HUB_ENABLE_HF_TRANSFER=1`.

- **67 GiB in 79 s ≈ 868 MiB/s** (first file 05:18:13 → last shard 05:19:32)
- 26 safetensors shards, 40 files total
- unauthenticated (HF warned about rate limits; it did not bite)

The repo name in the brief was correct: `Qwen/Qwen3.6-35B-A3B` exists,
created 2026-04-15, 5.57M downloads.

**Gotcha:** `pgrep -f "hf download"` self-matches the `bash -c` wrapper carrying
that string, so it reports RUNNING forever. The download had actually finished
(`✓ Downloaded` in the log). Same trap bit the fit-test poll. Match on the log,
not on pgrep.

---

## S4 — REAP prune 48%: **BLOCKED**

Determined entirely by desk check against the public repo and the HF config
(**$0 of pod time** — this was settled before the GPU was rented).

### The model

`Qwen/Qwen3.6-35B-A3B` `config.json`:
- `architectures: ["Qwen3_5MoeForConditionalGeneration"]`, `model_type: qwen3_5_moe`
- multimodal wrapper (has `image_token_id`, `video_token_id`, `preprocessor_config.json`)
- text: 40 layers, **256 routed experts**, top-k 8, `moe_intermediate_size` 512,
  `shared_expert_intermediate_size` 512
- hybrid attention: `layer_types` is 36 × `linear_attention` (Gated DeltaNet) +
  4 × `full_attention` (every 4th layer)

### Why REAP cannot prune it

REAP was last pushed 2026-04-17 and predates this architecture. Five independent
blockers, each verified against source:

1. **Dependency pins are incompatible.** `pyproject.toml` pins
   `transformers==4.55.0`, `torch==2.7.1`, `vllm==0.10.0`. `qwen3_5_moe` does not
   exist in transformers 4.55.0 — loading the model needs 5.x. Running REAP under
   transformers 5.5.0 is itself untested and spans a major-version break.

2. **No architecture entry.** `src/reap/model_util.py::MODEL_ATTRS` is keyed on
   `model.__class__.__name__` and has no `Qwen3_5MoeForConditionalGeneration`
   (only `Qwen3MoeForCausalLM`, `Llama4ForCausalLM`, `MixtralForCausalLM`,
   `DeepseekV2ForCausalLM`, `Ernie4_5_MoEForCausalLM`, `Glm4MoeForCausalLM`).
   `src/reap/observer.py::OBSERVER_CONFIGS` has the same gap.

3. **Wrong module path.** `model_util.get_moe()` does
   `model.model.layers[layer]`. This architecture nests the text stack under the
   multimodal wrapper — `_tied_weights_keys = {"lm_head.weight":
   "model.language_model.embed_tokens.weight"}` confirms the layers live at
   `model.model.language_model.layers[...]`. Straight `AttributeError`.

4. **The observer's fused-expert path is written to Llama4's API and does not
   fit.** In `observer.py::_hook_factory`, the `fused_experts` branch does:
   - `_, router_scores = output` — but `Qwen3_5MoeSparseMoeBlock.forward`
     returns a **single Tensor**, not a tuple, and **never returns router
     logits at all**;
   - `module.router(flat_input)` — this block names it **`self.gate`**, not
     `.router`;
   - `module.experts(routed_in)` with one argument — but
     `Qwen3_5MoeExperts.forward(hidden_states, top_k_index, top_k_weights)`
     **requires three**.

   REAP's whole saliency metric is router-weighted expert activation. To get it
   you must **patch the MoE block's forward to emit router logits** and write a
   **new fused-expert observer** matching this signature.

5. **Expert tensor layout differs from every supported fused model.**
   `Qwen3_5MoeExperts` stores raw `nn.Parameter`s —
   `gate_up_proj [256, 1024, 2048]` (gate and up **interleaved in one tensor**)
   and `down_proj [256, 2048, 512]`. Llama4's fused entry maps both `gate_proj`
   and `up_proj` to one name, but the slicing/merging code in `prune.py` and
   `merge.py` would still need rewriting for this packing.

**Verdict: this is real surgery, not "a clear mirror of the existing qwen3_moe
entry".** Per the pre-registration this is recorded as BLOCKED-for-pruning and
the spike continued to S5 on the **unpruned** model. Nothing was patched.

Rough scope if Brice wants it later: bump REAP off its pins onto transformers 5.x
and fix the fallout; add `MODEL_ATTRS` + `OBSERVER_CONFIGS` entries; generalise
`get_moe` for the multimodal nesting; subclass the observer for this expert
signature; patch `Qwen3_5MoeSparseMoeBlock.forward` to return router logits;
rewrite fused slicing for the interleaved `gate_up_proj`. Call it a multi-day
task with real correctness risk, not an afternoon.

### S4 addendum — others have already done this surgery (and it is narrower than a full port)

This matters, and it changes the recommendation. HF search shows **REAP-pruned
Qwen3.6-35B-A3B checkpoints in `safetensors` already exist**, so the blocker is
"upstream REAP doesn't support it", not "it can't be done":

| repo | ratio | format | note |
|---|---|---|---|
| `crucible-labs/Qwen3.6-35B-A3B-REAP-48-Q3K-mixed-GGUF` | **48%** | GGUF only | the model the prior bloomery spike booted |
| `DJLougen/Qwen3.6-35B-A3B-REAP-90pct` | 90% | **safetensors** | very aggressive |
| `groxaxo/…-Heretic-REAP-30-bf16` | 30% | **safetensors bf16** | abliterated variant |
| `anik-jha/Qwen3.6-35B-A3B-coding-reap50-GGUF` | 50% | GGUF only | coding-calibrated |

Two facts read straight off the model cards:

- **DJLougen** states the saliency math is *"verified bit-identical to upstream
  `reap.pruning_metrics.update_pruning_state`"*. That is precisely the shape of
  the fix: **reuse upstream's metric, write your own observer/plumbing.**
  `pruning_metrics.py` is architecture-agnostic; only the observer and model
  plumbing are not. That is a far smaller job than porting all of REAP.
- **crucible-labs'** card carries a comparison column labelled **"REAP-48 · bf16"**,
  i.e. they produced a bf16 REAP-48 intermediate and published only the
  quantized GGUF. Their recipe matches the brief exactly: **512-sample
  agentic-coding calibration, seed 42, 123 of 256 routed experts pruned leaving
  133** — the 133/256 the pre-registration predicted.

**Consequence for turn 5:** the fastest route to a *trainable* REAP-48 model is
not to port REAP. It is (a) ask crucible-labs for the unpublished bf16 REAP-48
intermediate, or (b) reimplement only the observer around upstream's saliency
metric. **No trainable 48% checkpoint is public today** — the crucible-labs
artefact is GGUF, which is inference-only and cannot be LoRA-trained with peft.

---

## S5 — bf16 LoRA fit test on the UNPRUNED model: **SUCCESS**

Loaded via `AutoModelForCausalLM` → resolved to **`Qwen3_5MoeForCausalLM`**, the
**text-only** class. This silently drops the vision tower, which is exactly what
we want for text training and saves memory.

- load: **17.1 s** from local disk; **34.66 B params**; **64.56 GiB** weights resident
- LoRA r=16 α=32, dropout 0, bias none
- target modules: `q_proj k_proj v_proj o_proj` (full-attn),
  `in_proj_qkv in_proj_z in_proj_b in_proj_a out_proj` (Gated DeltaNet),
  `gate_proj up_proj down_proj` (**shared expert only** — the routed experts are
  fused `nn.Parameter`s, so these names cannot collide with them)
- **trainable: 21.17 M (0.0611%)**; `experts frozen: True` asserted by scanning
  every parameter named `.experts.` for `requires_grad`
- gradient checkpointing on, `enable_input_require_grads()`
- label masking ported verbatim from `tools/flywheel/train.py`: raw text, no chat
  template, prompt tokens `-100`, no EOS appended. Check passed:
  `341/357 prompt tokens masked, tail='\n</action>'`

### Finding: the router is NOT LoRA-targetable

The brief asked for LoRA on "router/gate". **Not possible with stock peft.**
`Qwen3_5MoeTopKRouter` holds a bare `nn.Parameter` (`self.weight`), not an
`nn.Linear`, and peft's LoRA only wraps supported module types. The router was
therefore left frozen. If router adaptation is wanted, it needs either a custom
peft module type or promoting the router to `nn.Linear` — flag for turn 5 design.

### Run A — real corpus pairs (as `train.py` feeds them, unpacked)

16 pairs from `~/flywheel4/corpus.jsonl`, bs 1, 10 optimizer steps.
Sequence lengths came out **357–584 tokens** — the corpus pairs are short, so
this run never exercised seq 4096.

| metric | value |
|---|---|
| peak VRAM allocated | **66.76 GiB** |
| peak VRAM reserved | 72.15 GiB |
| s/step (mean, steps 3-10) | **1.52 s** |
| step 1 (warmup) | 112.48 s |
| losses | 0.1254, 0.0307, 0.7480, 0.6807, 0.0694, 0.0022, 0.4525, 0.0809, 0.0011, 0.8071 |
| all finite | **yes** |

### Run B — the pre-registered bs1 × seq 4096 condition

Because run A never reached 4096, real corpus pairs were **packed back-to-back to
exactly 4096 tokens** (prompt spans still masked; 93% of tokens masked in seq 0).

| metric | value |
|---|---|
| peak VRAM allocated | **76.84 GiB** |
| peak VRAM reserved | **77.24 GiB of 80 GiB (96.5%)** |
| s/step (mean, steps 3-10) | **2.12 s** |
| step 1 (warmup) | 30.01 s |
| losses | 0.1714, 0.4053, 0.4827, 0.2603, 0.3826, 0.1857, 0.1970, 0.0939, 0.3599, 0.1846 |
| all finite | **yes** |

**It fits — with about 3 GiB to spare.** That is a real constraint, not comfort:
any increase in batch size, sequence length, or a concurrent eval pass will OOM.
Turn 4's recipe (`per_device_train_batch_size=1`,
`per_device_eval_batch_size=1`, `gradient_accumulation_steps=8`) stays inside it,
since grad-accum costs no extra memory.

### Expert execution path

`Qwen3_5MoeExperts.forward` is a **Python loop over hit experts** with
`nn.functional.linear` per expert (`@use_experts_implementation` decorated, but
no grouped-MM kernel engaged in this env). Combined with the DeltaNet torch
fallback, both hot paths ran unoptimised. Again: these timings are upper bounds.

### Coherence check (base model, pre-LoRA, 40 tokens greedy)

Prompt: *"Write a Python function called clamp(x, lo, hi) that returns x limited
to the range [lo, hi]."* Output began:

```
Here's a thinking process:

1.  **Understand the User Request:**
   - **Function Name:** `clamp(x, lo, hi)`
   - **Purpose:**
```

Coherent, on-topic, correctly parsing the signature. It is a reasoning model and
opens with a thinking block. The weights load and run correctly.

---

## S6 — GGUF: NOT RUN (gate not met), but arch support confirmed for $0

The pre-registration gates S6 on "S4+S5 succeeded". S4 is BLOCKED, so the gate
was **not met and S6 was deliberately skipped** rather than quietly substituted.
Spend was only ~$0.5 at that point, so this was a discipline call, not a budget one.

The one genuinely unknown piece — whether llama.cpp can convert this
architecture at all — was answered by desk check instead, free:

- `src/models/qwen35moe.cpp` — inference graph for Qwen3.5/3.6 MoE **exists**
- `tests/snapshots/qwen3.6-27b.schema` and `qwen3.5-397b-a17b.schema` — **Qwen3.6
  is a tested target**
- `conversion/qwen.py`, `models/ggml-vocab-qwen35.gguf` present

**So GGUF Q4_K_M is expected to work**, but this is a desk check — *it was not
executed on the 35B*. Treat as "supported upstream, unverified here".

For scale, flywheel4's own 14B post-train chain: merge 2.6 min → bf16 GGUF
1.9 min → Q4_K_M 1.3 min. The 35B is ~2.5× larger; budget ~15 min for the chain.
Nothing was copied home (no artifact was worth the transfer).

---

## RECOMMENDATION

### Can turn 5's training step run on a ~$1.19/h A100 within ~$5/run?

**Yes — but only just, and only on the unpruned model.**

Projection from the measured **1.52 s/step** (unpacked, matching `train.py`'s
binding bs1-no-packing rule), using turn 4's shape (4,340 train pairs, 2 epochs
= 8,680 forward/backward passes):

| scenario | steps | time | @ $1.19/h | @ $1.39/h |
|---|---|---|---|---|
| unpacked (turn-4 recipe, as-is) | 8,680 | **3.66 h** | **$4.36** | $5.09 |
| + setup, download, load, evals | — | ~3.9 h | **~$4.65** | ~$5.42 |
| packed to seq 4096 (recipe change) | ~975 | **0.57 h** | **$0.68** | $0.80 |

**The turn-4 recipe lands at roughly $4.65/run on an A100 80GB PCIe** — inside
$5, but with almost no margin. Two things protect that number and one threatens it:

- *Protecting:* the DeltaNet fast path was **off** (causal-conv1d unbuildable) and
  the expert forward is an unfused Python loop. Fixing either makes it cheaper.
- *Protecting:* PCIe at $1.19/h was available; this spike only got SXM4 at $1.39/h
  because of the disk-size retry.
- *Threatening:* pod-boot waste is real. Pod 1 burned $0.27 producing nothing.

**Packing sequences to 4096 is a ~6.4× cost reduction** ($4.36 → $0.68) and is by
far the biggest lever available. It changes the pre-registered recipe, so it needs
a recorded amendment — but for a 35B model it is close to free money and I'd
raise it before turn 5 rather than after.

### Exact pipeline that works today

```
pod: A100 80GB PCIe, Community, containerDiskInGb 150, ports 22/tcp, PUBLIC_KEY
image: a CUDA-matched pytorch image (nvcc must match torch's CUDA — see below)
pip install --break-system-packages numpy huggingface_hub hf_transfer \
    transformers==5.5.0 peft==0.20.0 accelerate==1.14.0 safetensors==0.8.0 \
    flash-linear-attention causal-conv1d
HF_HUB_ENABLE_HF_TRANSFER=1 hf download Qwen/Qwen3.6-35B-A3B --local-dir ...   # ~80 s
AutoModelForCausalLM.from_pretrained(..., dtype=bfloat16, device_map="cuda:0")  # 17 s, text-only
LoRA r16 a32 on: q/k/v/o_proj, in_proj_{qkv,z,b,a}, out_proj, shared-expert gate/up/down
gradient_checkpointing_enable() + enable_input_require_grads()
train bs1, grad_accum 8, completion-only masking            # 1.52 s/step real, 2.12 s/step @4096
llama.cpp convert_hf_to_gguf → llama-quantize Q4_K_M        # supported, unverified at 35B
```

### What failed / what to change

1. **Do not try to run stock REAP on this architecture.** It cannot load, hook,
   or slice this model (five independent blockers, §S4). But **do not drop
   pruning from turn 5 either** — the S4 addendum shows the surgery is already
   solved by others and is narrower than a full port (reuse upstream's saliency
   metric, replace only the observer/plumbing). In priority order:
   **(a)** ask crucible-labs for their unpublished **bf16 REAP-48** intermediate —
   cheapest path by far, and their recipe already matches the brief
   (512-sample agentic-coding calibration, seed 42, 133/256 experts kept);
   **(b)** reimplement the observer around upstream `pruning_metrics.py`;
   **(c)** fall back to a model REAP supports natively (`Qwen3-30B-A3B`).
   Note that **no trainable 48% checkpoint is public** — the existing artefact
   is GGUF and cannot be LoRA-trained.
2. **Fix the CUDA/nvcc mismatch before renting again.** `causal-conv1d` refused to
   build (nvcc 13.0 vs torch cu12.9). This silently costs speed on 36 of 40 layers.
   Verify `is_fla_available()` and the fast path *before* starting a paid run.
3. **The router cannot take a LoRA.** Stock peft can't wrap
   `Qwen3_5MoeTopKRouter`'s bare `nn.Parameter`. Decide whether turn 5 needs
   router adaptation; if so it needs custom work.
4. **Memory margin is ~3 GiB at seq 4096.** Do not raise batch size or sequence
   length on an 80 GB card. If more headroom is wanted, 4-bit base weights would
   drop the resident 64.56 GiB to roughly 18 GiB.
5. **Kill any pod still showing `runtime: null` after ~5 minutes.** Request
   150 GB container disk, not 260 GB, on Community Cloud.
6. **`pgrep -f` self-matches through `ssh bash -c`.** Poll logs, not process lists.

### Honest limits of this spike

- Pruning was never executed; the BLOCKED verdict is a source-level desk check,
  not a runtime failure. It is well-evidenced but it is a reading of the code.
- The seq-4096 numbers come from **packed** real pairs, not from naturally-4096
  samples; the flywheel corpus has none that long.
- 10 steps is a fit test, not a training run. It proves memory, speed and finite
  losses. It proves nothing about convergence or output quality.
- GGUF conversion at 35B was not executed.
- All timings are on the slow kernel path.
