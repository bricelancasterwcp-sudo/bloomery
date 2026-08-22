# Task B — REAP-prune Qwen3.6-35B-A3B 48% with OUR pruner, on a rented A100

Date: 2026-08-22 (UTC). Operator: agent, under Brice's explicit go.
Tool under test: `tools/flywheel/prune` @ `af17588` (branch `reap-observer`, Task A).
Nothing was committed. Repo HEAD unchanged at `af17588`, working tree unchanged.

**Headline: our pruner works at full scale and produces a coherent, servable,
trainable REAP-48 checkpoint — but the smoke and the serve boot each caught a
real bug that Task A's tests did not. Three defects found, all in `prune.py` /
`save_pruned`. Total spend $6.3524 of the $8 cap. Pods remaining: 0.**

---

## 0. Cost and pod ledger

| | Pod 1 | Pod 2 |
|---|---|---|
| id | `z8vqwhvv9pzls1` | `lhwg92y00fzheh` |
| GPU | NVIDIA A100 80GB PCIe | NVIDIA A100-SXM4-80GB |
| price | $1.19/h | $1.39/h |
| containerDisk | **200 GB** | **150 GB** |
| created (UTC) | 06:39:15 | 06:46:04 |
| booted | **never** (`runtime:null` for 5m47s) | 06:51:00 (**4.93 min**) |
| terminated (UTC) | 06:45:38 | **11:10:39** |
| wall | 6.4 min | 264.6 min (4.41 h) |

Balance **$19.3171878066 → $12.9647986767**, spend **$6.352389**, `currentSpendPerHr` 0.
Zero pods verified by **both** REST `GET /pods` (`POD_COUNT 0`) and GraphQL
`myself { pods }` (`PODS 0`), checked after termination and again at the end.

**Second confirmation of the spike's disk lesson.** The spike lost a pod to a
260 GB container disk. The brief asked for ~200 GB; that pod *also* hung at
`runtime:null` and was killed at 5m47s ($0.13 wasted). Re-cut at the spike's
proven **150 GB**, which scheduled in 4.93 min. 150 GB is sufficient if the base
download is deleted before GGUF conversion (peak observed: 116 GB of 150 GB).
**Do not request >150 GB container disk on RunPod Community Cloud.**

---

## 1. B1/B2 — environment

- A100-SXM4-80GB, 81,920 MiB, driver 595.71.05
- image `runpod/pytorch:1.1.0-cu1290-torch291-ubuntu2404`, **torch 2.9.1+cu129**, Python 3.12.3
- 150 GB overlay, image ships **no numpy** (as the spike found); pip needs `--break-system-packages`

Versions vs `~/flywheel4/pip-freeze.txt`: transformers 5.5.0 ✓, peft 0.20.0 ✓,
accelerate 1.14.0 ✓, safetensors 0.8.0 ✓, numpy 2.5.2, gguf 0.19.0,
huggingface_hub 1.28.0, hf_transfer 0.1.9, flash-linear-attention 0.5.2.
torch differs (2.9.1+cu129 on pod vs 2.11.0 at home) — image-supplied, not overridden.

`causal-conv1d` **failed to build again**, identically to the spike (verbatim):

```
urllib.error.HTTPError: HTTP Error 404: Not Found
RuntimeError: ('The detected CUDA version (%s) mismatches the version that was used to compilePyTorch (%s). Please make sure to use the same CUDA versions.', '12.9', '13.0')
ERROR: Failed building wheel for causal-conv1d
```

So every forward pass below ran the **slow torch fallback** for the 36 Gated-DeltaNet
layers (`The fast path is not available ... Falling back to torch implementation`).
Calibration is forward-only, so this costs wall time, not correctness. All timings
here are conservative upper bounds.

Tool shipped with `git archive af17588 tools/flywheel | ssh ... tar -x`; corpus
scp'd and **md5-verified identical** (`aa8fb25d24289d21616933834fcebbe2`).

## 2. B3 — download

`hf download Qwen/Qwen3.6-35B-A3B` with `HF_HUB_ENABLE_HF_TRANSFER=1`:
**67 GiB in 108 s** (06:52:58 → 06:54:46), 39 files. Disk after: 70G used / 81G free.

---

## 3. B4 — SMOKE at full scale: **found bug #1**

The smoke did exactly what it was for. Task A never ran on CUDA, and the first
full-scale run **crashed** (verbatim):

```
File "/workspace/bloomery/tools/flywheel/prune/prune.py", line 100, in prune_model
  experts.gate_up_proj.data.index_select(0, index).clone(),
RuntimeError: Expected all tensors to be on the same device, but got index is on cpu,
different from other tensors on cuda:0 (when checking argument in method wrapper_CUDA__index_select)
```

### BUG #1 — `prune_model` builds its keep-index on CPU (`prune.py:95`)

```python
index = torch.tensor(sorted(keep_indices_per_layer[ref.layer_index]),
                     dtype=torch.long)          # <-- always CPU
```

`--device cuda` is unusable as shipped. Fixed **on the pod only** (repo untouched)
by building the index on each tensor's own device — a pure device-placement change
that alters no selection logic. **This must land in Task A.**

### Smoke result (after the fix)

| measurement | value |
|---|---|
| resolved class | `Qwen3_5MoeForCausalLM` (text-only; vision tower dropped) |
| experts | **256 → 133**, `plan: keep 133 of 256`, uniform across **all 40 layers** |
| wall | **89.469 s** |
| peak CUDA | **65.607 GiB** |
| peak RSS | 66,959 MB |
| calibration | 4 samples, seq_len 512, 1,923 tokens |
| saliency quantiles | min 0, p10 0.00459, p50 0.01771, p90 0.05386, max 0.51741 |

**133/256 is exactly crucible-labs' published keep count**, reached independently
by our tool from `--compression 0.48 --rounding ceil`.

### Ruling bA/R2 — renormalize-router-weights on vs off

Both rankings computed from the **same 4 samples** (1,923 tokens each pass),
in-memory, one model load:

| metric | value |
|---|---|
| Jaccard **min** (per layer) | **0.7616** |
| Jaccard **mean** | **0.8672** |
| Jaccard max | 0.9416 |
| layers with identical keep sets | **0 of 40** |

Renormalisation moves roughly **13% of the kept set** on average and **no layer
is unaffected**. This is not a cosmetic flag. Per the brief the real run kept the
**upstream default (off)**.

---

## 4. B5 — the real prune

### First attempt DESTROYED by environment mutation — my error, $1.70 and 73 min

The first 512-sample run reached sample 500/512 and then died at observer teardown
with a nonsensical traceback (garbled line numbers, and):

```
FileNotFoundError: [Errno 2] No such file or directory:
'/usr/local/lib/python3.12/dist-packages/transformers/models/qwen3_5_moe/modeling_qwen3_5_moe.py'
```

**Cause: I ran llama.cpp's `pip install -r requirements-convert_hf_to_gguf.txt`
concurrently with the calibration to "save time".** It silently:

- downgraded **transformers 5.5.0 → 4.57.6** (which has *no* `qwen3_5_moe` module)
- downgraded **numpy 2.5.2 → 1.26.4**
- replaced **torch 2.9.1+cu129 → torch 2.11.0+cpu** (CUDA gone entirely)

rewriting the package tree under a running process. Garbled tracebacks plus a
missing module file are the signature of exactly this.

**Rule for next time: never run any `pip install` while a long GPU job is running.
Build converter/tooling deps in an isolated venv, or before the run starts.**
Recovery: force-reinstalled torch 2.9.1+cu129 / transformers 5.5.0 / numpy, verified
`cuda_avail True` and the `qwen3_5_moe` module present *before* restarting. The
llama.cpp converter was then installed into a **dedicated venv**
(`/workspace/gguf-venv`, torch 2.11.0+cpu / transformers 4.57.6), and the system env
was re-verified intact afterwards.

### Second attempt — SUCCESS

Sampling rule (recorded verbatim): corpus `~/flywheel4/corpus.jsonl`, **4,561 rows**,
every row shaped `{prompt, completion, meta}`; text = `row["prompt"] + row["completion"]`;
selection = `random.Random(42).sample(range(4561), 512)` then **sorted ascending**
(first indices `[3, 4, 13, 17, 26, 29, 45, 48, 53, 58]`, last `[4541, 4543, 4552]`).
`--seq-len 2048` acts as a tokenizer `truncation`/`max_length` cap, i.e. natural
length capped at 2048.

| measurement | value |
|---|---|
| wall | **4,311.602 s = 71.9 min** (08:25:11 → 09:37:12) |
| peak CUDA | **65.607 GiB** |
| peak RSS | 66,969.9 MB |
| calibration | **512 samples, seq_len 2048, 233,159 tokens** |
| experts | **256 → 133**, uniform, **all 40 layers** (`kept_per_layer` distinct = {133}) |
| saliency quantiles | min **0.0**, p10 **0.00823**, p50 **0.01995**, p90 **0.05371**, max **0.44080**, mean **0.02813** |
| output | 36 GB, single `model.safetensors` = **38,349,435,696 bytes** |

Note `min = 0.0`: at least one expert received **zero** routed mass across 233k
calibration tokens — a genuinely dead expert, which is what makes 48% pruning safe.

### Provenance read back from `config.json` (`reap_pruning`), quoted

```json
{
  "tool": "tools.flywheel.prune",
  "method": "reap",
  "saliency_metric": "reap",
  "reap_upstream": "CerebrasResearch/reap@1970473c51ca3caeb98c10392f15b3a08a672974",
  "reap_formula": "CerebrasResearch/reap@1970473c src/reap/pruning_metrics.py:172-211",
  "keep_rule": "CerebrasResearch/reap@1970473c src/reap/prune.py:261 (rounding=ceil)",
  "compression": 0.48,
  "rounding": "ceil",
  "seed": 42,
  "renormalize_router_weights": false,
  "num_experts_before": 256,
  "num_experts_after": 133,
  "num_layers": 40,
  "calibration": {"requested_samples": 512, "samples": 512, "seq_len": 2048,
                  "source": "/workspace/corpus.jsonl", "tokens": 233159}
}
```

(`kept_per_layer` = 133 for every layer 0–39; full `kept_indices_per_layer` is in
the `reap_pruning.json` sidecar.)

### BUG #2 — `save_pruned` writes no tokenizer files

The output directory contained only `config.json`, `generation_config.json`,
`model.safetensors`, `reap_pruning.json`, `summary.json`. The checkpoint is **not
standalone**: `AutoTokenizer.from_pretrained(out_dir)` fails. Worked around by
copying `tokenizer.json`, `tokenizer_config.json`, `vocab.json`, `merges.txt`,
`chat_template.jinja` from the base (unchanged by expert pruning).

### COHERENCE — pure transformers, not our tool, bf16, 40 greedy tokens

Loaded from the pruned dir: `LOAD_S 13.0`, class `Qwen3_5MoeForCausalLM`,
`num_experts 133`. Verbatim:

> **Prompt:** Write a Python function called clamp(x, lo, hi) that returns x limited to the range [lo, hi].
> ```
> ```python
> def clamp(x, lo, hi):
>     if x < lo:
>         return lo
>     elif x > hi:
>         return hi
>     else:
> ```
> *(cut at the 40-token budget)*

> **Prompt:** What is the capital city of Japan?
> ```
> <think>
>
> </think>
>
> Japan does not have a single, officially designated "capital city" in the same way that some other countries or regions do. However, **Tokyo** is widely considered the
> ```

Correct code and a correct (if pedantically hedged) factual answer with 48% of
routed experts removed. **Coherence: PASS.** Base download deleted only after this.

---

## 5. B6 — GGUF

llama.cpp `8672290`, CMake CPU build, `llama-quantize` built while the (first)
prune ran. **The converter did NOT refuse the pruned expert count or the text-only
config** — it accepted `/workspace/reap48-ours` and wrote 733 tensors.

| step | time | result |
|---|---|---|
| `convert_hf_to_gguf.py --outtype bf16` | 6.57 min (09:49:00→09:55:34) | **38,382,368,928 B**, 733 tensors |
| `llama-quantize ... Q4_K_M` | 3.35 min (09:56:07→09:59:28) | **11,755,624,288 B** |

Quantizer reported `model size = 36593.80 MiB (16.01 BPW)` → `quant size =
11200.56 MiB (**4.90 BPW**)`. Tensor shapes confirm the prune, e.g.
`blk.39.ffn_up_exps.weight - [2048, 512, 133, 1]`.

sha256 as produced on the pod:

| artifact | sha256 |
|---|---|
| `qwen36-reap48-ours-Q4_K_M.gguf` | `22a719baf09fdfbaffddfaf8dd6181ab96927111b2f47f8efa534d5277e13781` |
| `reap48-ours-bf16.gguf` | `d6f9b2298a4fad586014eda61bf722a9c0a12910104ee13ecf29559a95265509` |
| `reap48-ours/model.safetensors` | `8027ca0a8277b540cd4c62eb7a5bdf6028875e84b33ddcf4f9cd4b0e9d63423b` |

---

## 6. B7 — transfer home

**Single-stream throughput was 4.0 MB/s, not the assumed ~30 MB/s**, and one scp
stalled completely (control channel fine, bulk frozen at a fixed byte count; host
load average 48 — a noisy Community-Cloud neighbour). A 6-way parallel chunked
pull (`dd ... skip/count` over 6 ssh streams) reached **17.2–19.8 MB/s — a 4.3–4.8x
speedup**, which is what brought the 38 GB bf16 checkpoint inside the cost cap.
Streams dropped mid-transfer (all six RC=255 at 55%), so the puller was made
**resumable** — each part re-requests only its missing byte range — and it
self-healed on attempt 2 both times.

Two further traps hit and worth recording:

- **`pkill -f "<string>"` matched my own `bash -c` command line and killed the
  shell** (standing box rule, confirmed again). Use PID-targeted kills.
- **The scratchpad is a size-limited tmpfs** — staging 11.7 GB of chunks there
  died with `Disk quota exceeded`. Stage large transfers on the real disk.

Landed and verified at home:

| artifact | bytes | sha256 verified |
|---|---|---|
| `~/models/gguf/qwen36-reap48-ours-Q4_K_M.gguf` | 11,755,624,288 | **exact match** `22a719ba…` |
| `~/models/hf/Qwen3.6-35B-A3B-REAP48-ours/model.safetensors` | 38,349,435,696 | **exact match** `8027ca0a…` |

plus `config.json`, `generation_config.json`, `reap_pruning.json`, `summary.json`,
`tokenizer.json`, `tokenizer_config.json`, `vocab.json`, `merges.txt`,
`chat_template.jinja`. **Both artifacts came home** — the bf16 HF dir is the
trainable one and did not have to be sacrificed.

---

## 7. B9 — local acceptance boot: **found bug #3 (the serious one)**

Preflight 11:12 UTC: no daemon running (`ps -eo pid,comm | grep -w bloomery-daemon`,
non-self-matching form); GPU 16,303 MiB total / **906 MiB used** by the desktop;
idle **`ollama serve` PID 3696348 — reported, not killed**; 143 GB free.
Featured binary mtime `1787348438` > last `crates/` change `1787348043` → **binary
newer, no rebuild**, and `cargo test` was not run. Dedicated scratch `data_dir`;
`~/.local/share/bloomery/drift/` never read or written.

### BUG #3 — the pruned checkpoint converts to an UNLOADABLE GGUF

First boot failed:

```
llama_model_load: error loading model: missing tensor 'blk.40.attn_norm.weight'
```

Root cause: the pruned `config.json` carries **`mtp_num_hidden_layers: 1`** inherited
from the base, but the text-only `Qwen3_5MoeForCausalLM` load **drops the MTP layer's
weights**. `convert_hf_to_gguf` therefore sized `block_count = 40 + 1 = 41` and emitted
`qwen35moe.nextn_predict_layers = 1`, while writing only 40 blocks of tensors.
**The checkpoint is internally inconsistent, and nothing before serve-time noticed.**

A full KV diff against crucible's GGUF isolated it exactly — **733 tensors in both,
every attention/expert KV identical** (`expert_count 133`, `expert_used_count 8`,
`key_length 256`, `head_count_kv 2`), differing only in:

| key | ours (as converted) | crucible |
|---|---|---|
| `qwen35moe.block_count` | **41** | 40 |
| `qwen35moe.nextn_predict_layers` | **1** | *(absent)* |
| `general.file_type` | 15 (Q4_K_M) | 11 (Q3_K) |

Fixed with two **same-length 4-byte u32 metadata edits** (`block_count 41→40`,
`nextn_predict_layers 1→0`), each asserted on the pre-value and re-read after; file
size unchanged. This is the "config-key mirror" case the brief permitted, and it
reproduces crucible's exact shape. `mtp_num_hidden_layers` was also set to 0 in the
local `config.json` so a future re-conversion is correct without patching.

**Post-patch on-disk sha256: `90e2181e8c3175c7f59f911ee70dfcc58cd068977fc657be3a4101d041f591a5`**
(the daemon's independently-computed `digest` agrees). The `22a719ba…` hash above is
the as-converted file and is what the transfer was verified against.

### Boot results (single boot, `g5_probe = true`, no `kv_per_token_bytes` override)

| measurement | value |
|---|---|
| `ModelLoaded.duration_ms` | **25,505** |
| `Post` outcome | **ok**; `Blessed` `auto-first-profile` sha `fa095213…` |
| `/status` `kv_per_token` | **81,920** (`declared: false`) — identical to crucible |
| `/status` `loaded_weights_bytes` | 11,755,624,288 |
| `/status` `free_vram_bytes` | 15,659,433,984 |
| `AgentCreated.window_tokens` | **29,631**, `bound_by: vram` (all **163** agents identical) |
| peak GPU observed | **12,682 MiB** vs 906 idle → **~11.5 GiB** attributable |
| `speed.decode_tps` | **116.74** |
| `speed.prefill_tps` | **4,415.33** |
| `ceiling` | `max_verified` 13,312, `first_failure` 14,336, `hard_error` |
| `Refusal` rows | **2** (`needed_tokens` 34,881 / 30,528 > window 29,631) |
| grant violations | **none found** |
| verb histogram (both legs) | `patch 47, read 44, done 47, find 25, run 3, ? 18` (`?` = 12 `NoAction`, 6 `unparseable after 2 re-asks`) |

**G4** — `CodecVerdict`, `codec-tasks-v1` @ envelope-v4, untrained:
`landed 20, n 20`, `interval95 [0.8388748419471806, 1.0]`, `provisional false`.

**G5** — `CodecVerdictMixed`, `codec-tasks-v4-mixed`:
`patch_landed 13 / 16` (`[0.5699, 0.9341]`, **provisional true**),
`refuse_landed 9 / 16` (`[0.3318, 0.7690]`, provisional false), `done_trust false`.

### Side by side with crucible's REAP-48 (the 2026-08-21 spike)

| metric | crucible REAP-48 (Q3K-mixed + imatrix) | **ours (Q4_K_M, no imatrix)** |
|---|---|---|
| file size | 9,427,632,224 | **11,755,624,288** (+2.33 GB) |
| BPW | ~3.4 (Q3_K) | **4.90** |
| `kv_per_token` | 81,920 | **81,920** (identical) |
| `window_tokens` | 54,631 | **29,631** |
| `ModelLoaded` | 1,421 ms | 25,505 ms |
| decode tps | 97.82 | **116.74 (+19.3%)** |
| prefill tps | 3,947.33 | **4,415.33 (+11.9%)** |
| ceiling | 16,384 (cap-bound) | 13,312 (window-bound) |
| `Refusal` rows | 0 | 2 |
| **G4 on v1 @v4** | **20/20** | **20/20** (identical Wilson interval) |
| **G5 patch** | 16/16 | **13/16** |
| **G5 refuse** | 5/16 | **9/16** |

**The window gap is fully explained by quantisation, not by pruning.** `kv_per_token`
is bit-identical (81,920) and both models have 40 layers / 133 experts; our GGUF is
simply 2.33 GB larger, and every byte of weights is a byte not available for KV
(`bound_by: vram`). Choosing Q4_K_M — as the brief specified — bought quality and
speed and cost window. A Q3_K_M build of our same checkpoint would land near 54k.

**Caveat on the comparison:** crucible quantised with an imatrix
(`entries_count 510`, `chunks_count 454`); we did not. So this table compares two
*pipelines*, not two prunes. That makes the G4/G5 agreement more impressive, not less.

---

## 8. RECOMMENDATION

### Is our pruned model a valid trainable stand-in for the served REAP-48?

**Yes — within noise on G4/G5, and it is the only *trainable* one in existence.**

- **G4: 20/20 vs 20/20**, bit-identical Wilson interval `[0.8389, 1.0]`. Our
  untrained pruned 35B matches what four flywheel turns bought on the 14B, exactly
  as crucible's did.
- **G5 patch: 13/16 vs 16/16.** Wilson `[0.5699, 0.9341]` vs `[0.8064, 1.0]` —
  **overlapping**; not distinguishable at 95% on n=16, and ours is flagged
  `provisional: true`.
- **G5 refuse: 9/16 vs 5/16** — ours is **better**, and `[0.3318, 0.7690]` vs
  `[0.1416, 0.5560]` again overlap. Crucible's REAP-48 was called out in the spike
  as *over-eager* (patching where it should refuse, and below stock-14B's 8/16);
  ours is the only REAP-48 variant measured so far that clears stock-14B on refusal.

No leg separates the two models at 95%. Both are 133/256, both `kv_per_token`
81,920, both G4 20/20. Independently reaching crucible's exact 133/256 keep count
from `--compression 0.48 --rounding ceil` is strong corroboration that our saliency
implementation tracks theirs.

**Decisive point: crucible published GGUF only, which cannot be LoRA-trained.
`~/models/hf/Qwen3.6-35B-A3B-REAP48-ours/` (bf16 safetensors, 38.3 GB, sha-verified)
is now the only trainable 48% checkpoint of this model that exists locally or
publicly.** That was the blocker the 2026-08-21 spike identified for turn 5, and it
is now cleared.

Two caveats to carry: our G5 patch leg is `provisional`, and the quantisation
differs from crucible's, so treat the head-to-head as pipeline-level.

### What turn 5's training step costs end to end on this pipeline

Measured inputs: 1.52 s/step unpacked and 2.12 s/step packed-to-4096 (spike, on the
*unpruned* model — our 133/256 model should be equal or faster); turn-4 shape of
4,340 pairs × 2 epochs = 8,680 steps; observed upload 19 MB/s; A100 SXM4 $1.39/h
(PCIe $1.19/h if it schedules — it did not, twice, at >150 GB disk).

| phase | time | $ @1.39/h |
|---|---|---|
| pod boot + env install | ~15 min | $0.35 |
| upload our bf16 checkpoint (38.3 GB @ 19 MB/s, 6 streams) | ~34 min | $0.79 |
| **train, turn-4 recipe unpacked** (8,680 × 1.52 s) | **3.66 h** | **$5.09** |
| GGUF convert + Q4_K_M quantize | ~10 min | $0.23 |
| download Q4_K_M (11.8 GB) | ~10 min | $0.23 |
| **total, as-is** | **~4.9 h** | **~$6.69** |
| **total, sequences packed to 4096** (975 × 2.12 s ≈ 0.57 h) | **~1.8 h** | **~$2.55** |

**Recommendations for turn 5, in priority order:**

1. **Fix the three bugs in Task A before spending again** — #1 (CUDA index) blocks
   `--device cuda` entirely; #3 (`mtp_num_hidden_layers`) silently produces an
   unloadable GGUF and is the dangerous one, because it survives every check until
   serve time. Add a regression test that asserts the emitted `block_count` equals
   the number of blocks actually written, and one that round-trips a tiny pruned
   checkpoint through `convert_hf_to_gguf`.
2. **Pack sequences to 4096.** It is a 2.6x end-to-end cost reduction ($6.69 → $2.55)
   and the single biggest lever, as the spike also concluded. It changes the
   pre-registered recipe, so it needs a recorded amendment.
3. **Avoid re-uploading 38 GB every run** — put the pruned checkpoint on a RunPod
   network volume, or publish it to HF. Re-pruning instead costs 72 min ($1.67),
   which is more than the $0.79 upload.
4. **Always transfer with parallel resumable chunks.** 4 MB/s → 19 MB/s is the
   difference between the bf16 checkpoint coming home and being abandoned.
5. **Never `pip install` alongside a running GPU job.** That mistake cost 73 minutes
   and ~$1.70 here. Use a dedicated venv (which then worked flawlessly).
6. If window matters more than quality for serving, ship a **Q3_K_M** build of this
   same checkpoint to recover the ~54k window; keep Q4_K_M for quality work.

### Honest limits

- Single boot, `n=16` per G5 leg — the patch leg is `provisional` and none of the
  G4/G5 comparisons can resolve differences smaller than ~20 points.
- `speed` is assay `--quick`: `n_decode 1`, `n_prefill 1`. Indicative, not a benchmark.
- All prune timings ran the slow DeltaNet torch fallback (`causal-conv1d` unbuildable).
- The GGUF was metadata-patched by hand after conversion; it was **not** re-converted
  from a corrected config, so the shipped file is "converted then fixed", not
  "converted correctly". The corrected `config.json` is in place for next time.
- Ours vs crucible differs in quantisation *and* imatrix, so the head-to-head is a
  pipeline comparison.
