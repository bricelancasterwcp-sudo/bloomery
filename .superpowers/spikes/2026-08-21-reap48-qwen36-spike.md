# SPIKE — `Qwen3.6-35B-A3B-REAP-48` Q3K-mixed on the 16 GB tier

**Date:** 2026-08-21. **Status:** in progress (scaffold written pre-boot).
**Scope:** a spike, not a measurement battery. Two boots, real GPU. Nothing
built here is kept in the repo; no commits were made.

**Repo state:** `master` @ `7571986aca7bcf715dde4f90afa4a9eee4efc425`.
Featured binary `target/release/bloomery-daemon` mtime `1787348438`
(2026-08-21 16:40:38 CDT) vs last source change under `crates/`
`1787342430` — **binary is newer, no rebuild performed**, and per the brief
`cargo test` was not run.

**Note on the report path.** `.superpowers/spikes/` is **untracked**, not
gitignored (`.gitignore` holds only `/target`, `__pycache__/`, `*.pyc`,
`.worktrees/`). `.gitignore` was deliberately left unmodified so this spike
leaves zero working-tree changes in the repo. Nothing here was committed.

---

## 1. Pre-boot prediction (written BEFORE any boot; not amended after)

### 1.1 The code path

The daemon derives KV-per-token in exactly one place, from GGUF metadata only:

- `crates/bloomery-core/src/gguf.rs:279` `parse_gguf_meta` reads, from the
  KV section: `general.architecture` -> `arch`; `{arch}.block_count` ->
  `layers`; `{arch}.attention.head_count_kv` -> `kv_heads`;
  `{arch}.attention.key_length` (falling back to
  `{arch}.embedding_length / {arch}.attention.head_count`) -> `head_dim`;
  `{arch}.context_length` -> `training_ctx`. `weights_bytes` is the **file
  size on disk**, not a tensor sum.
- `crates/bloomery-core/src/geometry.rs:21` `kv_bytes_per_token(m)` =
  `2 (K,V) * layers * kv_heads * head_dim * 2 (f16 bytes)`.
  **It has no notion of layer type.** Every `block_count` layer is charged as
  if it carried a KV cache.
- `crates/bloomery-daemon/src/pager.rs:527` stores that as
  `ModelEntry.kv_per_token` at `register_model`.
- `crates/bloomery-daemon/src/pager/tuning.rs:68`
  `effective_kv_per_token() = kv_per_token_bytes.unwrap_or(kv_per_token)` —
  the ONE place the override lands, read by both the window law
  (`pager.rs:586`) and the reservation charge (`tuning.rs:177`).
- Window law: `geometry.rs:76` `usable_window`, VRAM term =
  `(free_vram - weights - overhead - ctx_overhead) / kv_per_token`.

The override itself is `crates/bloomery-daemon/src/config.rs:186`
`kv_per_token_bytes`. **Its own doc comment already names this exact case**
(verbatim):

> declaring a SMALLER number than GGUF derives is the whole point
> (hybrid-DeltaNet architectures the pager's GGUF-derived formula overcounts
> ~4x, per the spec's measured qwen3.8-27b figure)

and the matching warning:

> **Declaring too small is the OOM direction**: the window law would then
> grant tokens whose real KV exceeds VRAM — this is a declared, measured-
> once-with-headroom number, not something this daemon ever verifies against
> the model's actual runtime KV footprint.

### 1.2 The prediction

`qwen35moe` is hybrid: 30 Gated-DeltaNet layers + 10 gated-attention layers.
`block_count` should be **40** (all blocks), but only 10 of them hold a KV
cache. So:

| quantity | predicted |
|---|---|
| `arch` | `qwen35moe` |
| `layers` (`block_count`) | 40 |
| `kv_heads` | 2 |
| `head_dim` (`key_length`) | 256 |
| **GGUF-derived `kv_per_token`** | `2*40*2*256*2` = **81,920 B/tok (80 KiB)** |
| **true architectural `kv_per_token`** | `2*10*2*256*2` = **20,480 B/tok (20 KiB)** |
| over-count factor | **4.00x** — matching the config doc's "~4x" |

Window prediction, calibrated against the fw4 boot-1 `/status`
(`free_vram_bytes` 14,838,398,976; `overhead_bytes` 1,073,741,824;
`ctx_overhead_bytes` 402,653,184 — those three reproduce fw4's journaled
`window_tokens` 26,612 exactly, so the arithmetic model is verified):

- remaining = `free_vram - weights(~9.4e9) - 1 GiB - 384 MiB` ~= **3.96 GB**
- Boot A (no override): `3.96e9 / 81,920` ~= **48,000 tokens**
- Boot B (override 20,480): `3.96e9 / 20,480` ~= **193,000 tokens**, unless
  `training_ctx` binds first.

**Predicted headline: >=15K context is met in BOTH boots.** Even the 4x
over-count leaves ~48K, because the REAP model's *true* KV is so small that
4x of it is still cheap. The override's real effect here is on the
**reservation charge** (how much VRAM one agent's window books), not on
clearing the 15K bar.

**Predicted risk:** the DeltaNet recurrent state is per-sequence constant and
is charged by **neither** number. If llama.cpp allocates it outside the KV
cache, real VRAM will exceed the daemon's model in a way nothing here
predicts. Watch `nvidia-smi` during serve against `loaded_weights_bytes +
resident_kv_bytes`.

---

## 2. Artifact

| item | value |
|---|---|
| repo | `crucible-labs/Qwen3.6-35B-A3B-REAP-48-Q3K-mixed-GGUF` |
| file | `/home/brice/models/gguf/qwen36-reap-48pct-mixed-q3k.gguf` |
| size | **9,427,632,224 bytes** (8.78 GiB) |
| sha256 | **`d04cc1d7606baf7c9e9c9a2b4a149d4df19da6719db26a304645b688f5c9cb4e`** |
| download | `hf download`, 22:42:53 -> 22:48:01 CDT = **308 s (5.13 min)**, **30.6 MB/s** average. Disk after: 189 GiB free |
| sha provenance | the value above **equals the sha256 embedded in HF's own `.incomplete` staging filename**, i.e. it matches the repo manifest, not just itself |
| license | Apache-2.0 (per the model card) |

### 2.1 GGUF metadata (read directly; the daemon's loader dump agrees byte-for-byte)

```
general.architecture              = qwen35moe
qwen35moe.block_count             = 40
qwen35moe.context_length          = 262144
qwen35moe.embedding_length        = 2048
qwen35moe.attention.head_count    = 16
qwen35moe.attention.head_count_kv = 2
qwen35moe.attention.key_length    = 256
qwen35moe.attention.value_length  = 256
qwen35moe.full_attention_interval = 4      <-- 40 / 4 = 10 attention layers
qwen35moe.expert_count            = 133    <-- 133/256 kept, as advertised
qwen35moe.expert_used_count       = 8
qwen35moe.ssm.conv_kernel         = 4
qwen35moe.ssm.state_size          = 128
qwen35moe.ssm.group_count         = 16
qwen35moe.ssm.inner_size          = 4096
qwen35moe.ssm.time_step_rank      = 32
```

llama.cpp's own load line: `file type = Q3_K - Small`, `file size = 8.77 GiB
(3.93 BPW)`, `model params = 19.17 B`, `model type = 35B.A3B`,
`n_ctx_train = 262144`, `n_embd_head_k = 256`, `n_embd_head_v = 256`,
`n_embd_k_gqa = 512`, `n_embd_v_gqa = 512`.

**`full_attention_interval = 4` is the key confirming datum** and it settles
the 10-vs-40 question from the GGUF itself: 40 blocks with a full-attention
layer every 4th one gives exactly **10 attention layers, 30 Gated-DeltaNet
layers**. The true KV charge is therefore
`2 (K,V) * 10 * 2 heads * 256 dim * 2 bytes` = **20,480 B/token**, which is
the number Boot B declares. Confirmed independently: `n_embd_k_gqa` =
`n_embd_v_gqa` = 512 = `kv_heads * head_dim`, so per attention layer
`(512 + 512) * 2 bytes` = 2,048 B/token, times 10 layers = 20,480 B/token.

**The prediction in §1.2 was exact on every line** — arch, 40 blocks, 2 KV
heads, 256 head_dim, 81,920 B/token derived, 20,480 B/token true, 4.00x.

---

## 3. Boot A — no override

**Config** (`target/reap-spike/a/bloomery-reap-a.toml`), verbatim:

```toml
port = 8399
data_dir = "/home/brice/workspace/bloomery/target/reap-spike/a/data"
tasks_enabled = true

[models."qwen36-reap48"]
path = "/home/brice/models/gguf/qwen36-reap-48pct-mixed-q3k.gguf"
envelope = "v4"

[tier]
name = "enthusiast-16gb"
emulated = false

[assay]
enabled = true
python = "python3"
probe_timeout_secs = 1800
```

`allow_unprofiled` is omitted -> default `false`, exactly as the fw4 battery
ran it (`crates/bloomery-daemon/tests/config_test.rs:43` asserts that
default). Launched detached, `setsid nohup env PYTHONPATH=/home/brice/workspace/assay/src`,
PID **2789473**, `readlink /proc/2789473/exe` asserted equal to
`/home/brice/workspace/bloomery/target/release/bloomery-daemon` before
anything was measured.

**Preflight 22:49:05 CDT:** no `bloomery-daemon` running (`ps -eo pid,comm |
grep -w bloomery-daemon`, the non-self-matching form); GPU 16,303 MiB total /
**1,173 MiB used** by the desktop / 14,667 MiB free; compute-apps ptyxis 31,
lact 49, gnome-text-editor 142; idle **`ollama serve` PID 3696348 holding
0 MiB — reported, not killed**, per the standing rule; 189 GiB free on `/`.
The standing drift home `~/.local/share/bloomery/drift/` was neither read nor
written (dedicated scratch `data_dir`).

### 3.1 Numbers

| measurement | value |
|---|---|
| **`ModelLoaded.duration_ms`** | **1,421 ms** (mmap load) |
| `/status` `loaded_weights_bytes` | 9,427,632,224 (= file size) |
| `/status` `free_vram_bytes` (boot-time one-shot) | 15,379,464,192 |
| `/status` `kv_per_token` | **81,920** |
| `/status` `kv_per_token_declared` | **false** |
| `/status` `training_ctx` | 262,144 |
| `/status` `digest` | `d04cc1d7…cb4e` — **equals the `sha256sum` above** |
| **`AgentCreated.window_tokens`** | **54,631**, `bound_by: "vram"` (all 26 agents identical) |
| VRAM during serve (`nvidia-smi`) | 11,310 MiB used / 4,530 MiB free (vs 1,173 idle -> **~10.1 GiB attributable to the daemon**) |

Window arithmetic, reproduced by hand from the committed `/status` terms:
`(15,379,464,192 - 9,427,632,224 - 1,073,741,824 - 402,653,184) / 81,920 =
4,475,436,960 / 81,920 = 54,631.3` -> **54,631**. Exactly the journaled
number.

**This is the headline of Boot A: even carrying the 4x over-count, the window
is 54,631 tokens — 3.6x the >=15K bar.** The over-count is real and worth
fixing, but it is not what decides serveability for this model.

### 3.2 llama.cpp proves the over-count at runtime — verbatim

The daemon's own stdout settles the hybrid question without any inference on
my part. **llama.cpp filters 30 of the 40 layers out of the KV cache**,
keeping exactly the 10 full-attention layers (3, 7, 11, 15, 19, 23, 27, 31,
35, 39 — every 4th, matching `full_attention_interval = 4`):

```
llama_kv_cache: layer   0: filtered
llama_kv_cache: layer   1: filtered
llama_kv_cache: layer   2: filtered
llama_kv_cache: layer   3: dev = Vulkan0
...
llama_kv_cache:    Vulkan0 KV buffer size =  1070.00 MiB
llama_kv_cache: size = 1070.00 MiB ( 54784 cells,  10 layers,  1/1 seqs), K (f16):  535.00 MiB, V (f16):  535.00 MiB
```

`1070.00 MiB / 54,784 cells` = **exactly 20,480 B/token**. The daemon charged
**81,920**. **Measured over-charge: exactly 4.00x**, and 20,480 is exactly the
value Boot B declares — so the override number is not a guess, it is
confirmed by the substrate's own allocator at the same boot.

In VRAM terms: the window law reserved `54,631 * 81,920` = **4.48 GB** of KV
for a context whose KV cache llama.cpp actually allocated at **1.07 GiB**.
**~3.4 GB of VRAM is booked and never used.**

### 3.3 The DeltaNet state IS allocated, and nothing in the daemon charges it

```
llama_memory_recurrent, layer   0: dev = Vulkan0
llama_memory_recurrent: layer   3: skipped
...
llama_memory_recurrent:    Vulkan0 RS buffer size =    62.81 MiB
llama_memory_recurrent: size =   62.81 MiB (     1 cells,  40 layers,  1 seqs  0 rs_seq), R (f32):    2.81 MiB, S (f32):   60.00 MiB
```

The mirror image of the KV filter: the recurrent store covers the 30 DeltaNet
layers and **skips** the 10 attention layers. **62.81 MiB, `1 cells` — a
per-sequence constant, independent of context length**, exactly as the brief
predicted. It is charged by *neither* `kv_per_token` nor the declared
override.

Full per-context VRAM beyond weights, from the same boot:

| term | size | scales with ctx? | charged by the daemon? |
|---|---|---|---|
| KV cache | 1,070.00 MiB @ 54,784 | yes, 20,480 B/tok | yes, but at 4x |
| recurrent (DeltaNet) state | **62.81 MiB** | **no — constant** | **no** |
| Vulkan0 compute buffer | 493.00 MiB | no (batch-bound) | via `ctx_overhead` |
| Vulkan_Host compute buffer | 61.52 MiB | — | host RAM, not VRAM |
| output buffer | 0.95 MiB | — | host RAM, not VRAM |

**Non-KV per-context VRAM = 62.81 + 493.00 = 555.81 MiB, against a
`ctx_overhead_bytes` of 384 MiB — a ~172 MiB per-context shortfall.** On this
boot it is invisible, because the 4x KV over-charge over-reserves by ~3.4 GB
and swamps it. **An override that removes the over-charge also removes the
thing that was masking this.** See §5 — it is the one reason the override is
not a free win.

One more conservatism worth naming, in the safe direction: the daemon charges
`weights_bytes` = **file size** (8,990.9 MiB), while llama.cpp's actual
`Vulkan0 model buffer size` is **8,465.10 MiB** — a 525.8 MiB over-charge
(GGUF metadata and the 248K-entry tokenizer are in the file but not in VRAM).

### 3.4 Load-time warnings: none

`grep -inE "warn|error|unsupported|missing|not supported|fail|MTP|deprecat"`
over the whole of `daemon.out`, excluding the routine
`control token ... is not marked as EOG` lines, returns **zero matches**.
No unsupported tensor, no missing-MTP complaint, no fallback notice.
`arch = qwen35moe` is fully supported by the pinned b10200 substrate; all 40
layers went to `Vulkan0`; **no expert offload and no CPU fallback occurred or
was needed** — `Vulkan0 model buffer size = 8465.10 MiB` is the whole model.

The only informational line of note is expected and benign:

```
llama_context: n_ctx_seq (54784) < n_ctx_train (262144) -- the full capacity of the model will not be utilized
```



### 3.5 POST, speed, and the assay ceiling

`Post` outcome **`ok`** at 22:53:36, **266 s** after boot (the 14B's fw4 POST
took 495 s). Profile blessed `auto-first-profile`, sha
`3ec1915bea753488d23d9c9c92b3dcccf126efb45765a1a38ece7d7c7179318d`.

| profile field | **REAP-48** | fw4 14B (anchor) |
|---|---|---|
| `speed.decode_tps` | **97.82** | 50.82 (**1.92x**) |
| `speed.prefill_tps` | **3,947.33** | 2,587.49 (**1.53x**) |
| `ceiling.max_verified` | **16,384** | 12,288 |
| `ceiling.first_failure` | **`null`** | 13,312 |
| `ceiling.failure_mode` | **`none_up_to_cap`** | `hard_error` |
| journaled `Refusal` rows | **0** | 3 |

`speed` evidence is `wall_clock_counts`, `n_decode: 1`, `n_prefill: 1` — one
sample each, assay's `--quick` shape. Treat as indicative, not a benchmark.

**The ceiling is cap-bound, not model-bound.** POST runs assay in `--quick`
mode, whose ladder cap is the constant `_QUICK_CEILING_CAP = 16384`
(`assay/src/assay/run.py:59`), and `ceiling_cap_for` does **not** clamp quick
by `training_ctx`. The ladder walked 1024 -> 2048 -> 4096 -> 8192 -> 16384 and
**every rung passed** (`failure_mode: none_up_to_cap`), so 16,384 is the
instrument's ceiling, not the model's. The 14B failed at 16,384 with
`HTTP 400` and bisected down to 12,288 — because its 26,913-token window
could not hold assay's 16K probe plus `max_tokens`. This model's 54,631-token
window swallows the whole ladder, which is also why it journals **zero**
`Refusal` rows against the 14B's three. **The two ceilings are therefore not
comparable as model properties** — one is cap-bound, one is window-bound.

`long_context` verdict: **`ready`** (`evidence: counts+canary`).

### 3.6 The G4 verdict — the surprise of this spike

```json
{"event":"CodecVerdict","model":"qwen36-reap48","fixture_set":"codec-tasks-v1",
 "codec":"search_replace","landed":20,"n":20,
 "interval95":[0.8388748419471806,1.0],"provisional":false,"mutating_verbs":true,
 "detail":"applies_and_parses under bloomery-task-envelope-v4; codec from profile"}
```
*(`"epoch_ms":1787370848036` elided; the scratch journal carries the unedited row.)*

**G4 = 20/20 @ envelope-v4, untrained.** Independently recomputed from the 20
committed `CodecFixture` rows by the validated script — 20/20, Wilson
`[0.8388748419471806, 1.0]`, **bit-identical** to the journaled interval.

Against the two v4 anchors (`2026-08-21-g5v4-baselines.md`):

| model | G4 on v1 @v4 |
|---|---|
| stock 14B (untrained) | 6/20 |
| **REAP-48 35B-A3B (untrained)** | **20/20** |
| flywheel4 14B (**trained**, 4 turns) | 20/20 |

**An untrained model matched what four flywheel turns bought on the 14B.**

Boot A verb histogram: `read 17, patch 23, done 20` — **zero `find`, zero
`run`** (codec-tasks-v1 grants neither, so this is expected and matches the
14B's v1 leg). `patch 23 > 20` means three fixtures needed a second patch
attempt.

**Peak VRAM across boot A: 11,390 MiB** (44 samples at 6 s), min 9,520,
against 1,173 idle -> **~10.0 GiB peak attributable to the daemon**, with
~4.8 GiB of headroom never touched. Final `/status` retained at
`target/reap-spike/a/status-boot-a-final.json`.

Daemon brought down by **verified PID 2789473** — `readlink /proc/2789473/exe`
re-asserted against the featured binary immediately before `kill`, then
polled to exit. No `pkill`. Nothing wrapped in `timeout`. GPU returned to
931 MiB.

---

## 4. Boot B — override + g5_probe

**Config** (`target/reap-spike/b/bloomery-reap-b.toml`), differing from boot A
only in `data_dir`, the two added keys, and the comment:

```toml
[models."qwen36-reap48"]
path = "/home/brice/models/gguf/qwen36-reap-48pct-mixed-q3k.gguf"
envelope = "v4"
kv_per_token_bytes = 20480
g5_probe = true
```

Launched detached the same way, PID **2798704**, exe verified against the
featured binary. Preflight 22:54:47: no daemon running, GPU 931 MiB used /
14,910 MiB free.

### 4.1 The window moves 4.23x, and the KV allocation confirms 20,480 again

| measurement | Boot A (no override) | **Boot B (override 20,480)** |
|---|---|---|
| `/status` `kv_per_token` | 81,920 | **20,480** |
| `/status` `kv_per_token_declared` | false | **true** |
| **`AgentCreated.window_tokens`** | 54,631 | **230,968** |
| `bound_by` | `vram` | **`vram`** (262,144 training_ctx never binds) |
| `ModelLoaded.duration_ms` | 1,421 | **1,373** |
| llama.cpp `n_ctx` | 54,784 | **231,168** |
| llama.cpp KV buffer | 1,070.00 MiB | **4,515.00 MiB** |
| KV B/token, measured | 20,480.0 | **20,480.0** |
| recurrent (RS) buffer | 62.81 MiB | **62.81 MiB** (unchanged — constant) |
| Vulkan0 compute buffer | 493.00 MiB | **493.00 MiB** (unchanged) |

```
llama_kv_cache: size = 4515.00 MiB (231168 cells,  10 layers,  1/1 seqs), K (f16): 2257.50 MiB, V (f16): 2257.50 MiB
llama_memory_recurrent: size =   62.81 MiB (     1 cells,  40 layers,  1 seqs  0 rs_seq), R (f32):    2.81 MiB, S (f32):   60.00 MiB
```

`4,515.00 MiB / 231,168 cells` = **20,480 B/token exactly**, at a 4.2x larger
context than boot A. The rate is confirmed twice, at two very different
context sizes. **The override is correct and the daemon honored it.**

The override did not just widen the window — it converted ~3.4 GB of phantom
reservation into 176,337 additional real tokens. **The model would serve a
230K context on a 16 GB card.**

### 4.2 Both boots: no expert offload, no CPU fallback, no warnings

Identical in both boots:

```
load_tensors: offloading output layer to GPU
load_tensors: offloading 39 repeating layers to GPU
load_tensors: offloaded 41/41 layers to GPU
load_tensors:   CPU_Mapped model buffer size =   515.31 MiB
load_tensors:      Vulkan0 model buffer size =  8465.10 MiB
```

**41/41 layers on GPU.** The 515.31 MiB `CPU_Mapped` buffer is the token
embedding left in host mmap — routine, not a spill. **No `--cpu-moe`-style
expert offload is needed or was used**, exactly as the brief expected: 8.78
GiB of weights fits a 16 GB card with room for a 230K context.
`grep -inE "warn|error|unsupported|not supported|fail|MTP|deprecat"` over both
`daemon.out` files returns nothing but those three informational offload
lines.


### 4.3 The cost of the override: throughput and headroom

Both are real and neither was predicted:

| | Boot A (54,631) | Boot B (230,968) | delta |
|---|---|---|---|
| `speed.decode_tps` | 97.82 | **78.20** | **-20.1%** |
| `speed.prefill_tps` | 3,947.33 | **3,012.95** | **-23.7%** |
| POST wall time | 266 s | 278 s | +4.5% |
| **peak VRAM (nvidia-smi)** | 11,390 MiB | **15,130 MiB** | **+3,740 MiB** |
| **free VRAM at peak** | ~4,913 MiB | **~1,173 MiB (7.2%)** | |
| sum of `InferCompleted.duration_ms`, first 84 | 138.5 s | 137.8 s | **-0.5%** |
| non-infer wall gap, first 84 | 13.4 s | 26.3 s | **+96%** |

Two separate effects, and it matters that they are separate:

1. **Raw generation work is unchanged** (137.8 s vs 138.5 s over the same
   first 84 infers). The `decode_tps`/`prefill_tps` drop is an *attention-
   over-a-larger-KV* effect, since llama.cpp sizes `n_ctx` to the **whole
   window** (231,168) rather than to the prompt. Both figures are `n=1`
   samples from assay `--quick` — indicative, not a benchmark — but decode
   and prefill move together and in the direction the mechanism predicts.
2. **Per-agent context churn roughly doubled** (13.4 s -> 26.3 s of non-infer
   wall time over 84 agents, ~0.15 s more each): every `create_agent`
   allocates a 4,515 MiB KV buffer instead of a 1,070 MiB one.

**The headroom number is the one to worry about.** Peak 15,130 MiB of 16,303
leaves **1,173 MiB — 7.2%**. The desktop session moved between 931 and 1,232
MiB during this session's own measurements; a browser window opening during a
230K-token serve is plausibly an OOM. The daemon's arithmetic is sound
(charged 14,910 MiB vs real 13,536 MiB, 1,374 MiB slack), but that slack is
supplied by two *accidental* conservatisms, not by design — see §5b.

### 4.4 Verdicts

```json
{"event":"CodecVerdict","model":"qwen36-reap48","fixture_set":"codec-tasks-v1",
 "codec":"search_replace","landed":20,"n":20,
 "interval95":[0.8388748419471806,1.0],"provisional":false,"mutating_verbs":true,
 "detail":"applies_and_parses under bloomery-task-envelope-v4; codec from profile"}

{"event":"CodecVerdictMixed","model":"qwen36-reap48","fixture_set":"codec-tasks-v4-mixed",
 "codec":"search_replace","envelope":"bloomery-task-envelope-v4",
 "patch_landed":16,"patch_n":16,"patch_interval95":[0.8063923194655636,1.0],"patch_provisional":false,
 "refuse_landed":5,"refuse_n":16,"refuse_interval95":[0.14164643854782036,0.5559564416525933],
 "refuse_provisional":false,"done_trust":false,"detail":"codec from profile"}
```
*(`epoch_ms` elided from both — `1787371210756` and `1787371295157`.)*

Both independently recomputed from the committed `CodecFixture` rows: 20/20,
16/16, 5/16, with Wilson bounds bit-identical to the journaled ones. **G4
reproduced boot A exactly (20/20), across a 4.2x different window** — the
override changed the geometry and nothing about the measurement.

### 4.5 Secondary endpoints — APPROXIMATE, and one of them is untrustworthy

Per the brief, this is the turn-4 recompute method at spike fidelity.
**Flagged honestly:** my script attributes `TaskStep` rows to fixtures by
epoch ordering, and for boot B that pairing is **provably wrong** — it reports
`find-usage = 12` against a denominator of **6**. A number above its own
denominator is not a number; do not quote the paired endpoints from this
boot. **The raw verb histogram is exact and needs no pairing**, so that is
what is reported:

| | **REAP-48 (untrained)** | fw4 (trained) | stock 14B |
|---|---|---|---|
| total `TaskStep` rows | **199** | 191 | — |
| `read` | 64 | 52 | — |
| `patch` | 49 | 36 | — |
| `done` | **45** (of 32 fixtures) | 52 | — |
| **`find`** | **33** | **6** | 6 |
| **`run`** | **8** | **5** | 0 |
| **grant-violation rows** | **5** (4 `read`, 1 `run`) | **0** | 38 |

**This model is far more verb-active than either anchor** — 33 `find` verbs
against fw4's 6, and 8 `run` verbs against 5 on a 5-fixture granted slice. It
explores, and it acts. The two grant-violation shapes, verbatim heads:
`grant violation: /home/brice/workspace/bloomery/target/reap-...` (4 `read`
rows reaching outside the slice) and `grant violation: command ["python3",
"-c", "import tympanche...` (1 `run` inventing a module name). Five is
between fw4's zero and stock's 38.

### 4.6 The refusal result, stated plainly

**Refuse 5/16 — a decided FAIL, and *below stock-14B's 8/16*.** Per-family
landed (approximate attribution, but the total 5 is exact and matches the
verdict): `absent 4, missing 0, mismatch 1`.

This is a coherent picture, not a contradiction with the 16/16 patch score.
The model is **over-eager**: it patches confidently and well, and when the
target does not exist or does not match, it patches *anyway* rather than
refusing. `done 45` on 32 fixtures — it declares completion more often than
there are fixtures. **Capability arrives maxed; refusal-honesty arrives worse
than the floor model.**

`done_trust: false`, correctly, and `/status` renders the `refusal_gate`
accordingly.

---

## 5. Recommendation

### (a) Is it serveable on the 16 GB tier with >=15K context resident?

**Yes, decisively, and the override is not required to clear the bar.**

| | window | >= 15K? | margin |
|---|---|---|---|
| **no override** (ship-as-is) | **54,631** | **yes** | **3.6x** |
| **override 20,480** | **230,968** | **yes** | **15.4x** |

Both boots served the real model on the real GPU with `bound_by: vram`, POST
`ok`, a `search_replace` codec resolved from the profile, and G4 20/20. Load
time 1.4 s. Weights 8.78 GiB, **41/41 layers on GPU, no expert offload, no
`--cpu-moe`, no CPU fallback** — exactly as the brief expected. Zero llama.cpp
warnings; `qwen35moe` is fully supported by the pinned b10200 substrate.

The model is also **faster than the incumbent 14B**: decode 97.8 tps vs 50.8
(1.92x), prefill 3,947 vs 2,587 (1.53x) at the no-override window — an A3B
MoE with ~3B active parameters.

**Recommendation: serve it with NO override.** 54,631 tokens is far past any
real workload, the throughput is 20% better, and peak VRAM sits at 11.4 GB
with ~4.9 GB of genuine headroom instead of 1.2 GB. **The override's 4.2x
window is real but buys context nothing needs at a cost in speed and safety
margin.** Reach for it only if someone actually wants a >54K context, and if
they do, raise `ctx_overhead_mib` first (see below).

### (b) What bloomery change is needed?

**1. Hybrid-aware KV derivation — the real fix, and it is small.**
`geometry.rs:21` multiplies by `m.layers` (`block_count`) with no notion of
layer type. For any hybrid arch this over-counts by exactly the
attention-layer fraction. **The GGUF already carries the answer**:
`qwen35moe.full_attention_interval = 4`, so
`attention_layers = block_count / full_attention_interval` = 10. Reading that
key when present (falling back to `block_count` when absent, i.e. every
dense model today behaves identically) would have produced **20,480 B/token
automatically** — the exact value llama.cpp allocated, confirmed at two
different context sizes. This is a ~10-line change in `gguf.rs` +
`geometry.rs` and it makes the override unnecessary for this whole model
family. **This is the recommended change.**

**2. Do NOT ship a default KV override.** The `kv_per_token_bytes` escape
hatch works exactly as documented and its doc comment already anticipated this
architecture ("hybrid-DeltaNet ... overcounts ~4x"). It should stay an
operator-declared, measured-once number. A shipped default would be a
per-model magic constant that nothing verifies — precisely the OOM direction
its own doc warns about.

**3. No `cpu-moe` flag is needed.** Nothing about this model asked for one.

**4. The one genuine defect this spike found — `ctx_overhead_bytes` does not
cover a hybrid model's recurrent state.** Measured per-context non-KV VRAM:

```
recurrent (DeltaNet) state   62.81 MiB   (constant, charged by NOTHING)
Vulkan0 compute buffer      493.00 MiB
                            ------
                            555.81 MiB   vs ctx_overhead_bytes = 384 MiB
```

**A 171.81 MiB shortfall per resident context.** It did not bite here for two
reasons, and **both are accidents rather than design**: the daemon charges
`weights_bytes` = *file size* (8,990.9 MiB) against a real `Vulkan0 model
buffer` of 8,465.10 MiB — a 525.8 MiB windfall — and the 1,024 MiB global
overhead is counted once. Cushion = 1,549.8 MiB, so **the shortfall is
absorbed for about 9 concurrent contexts and not beyond**. With one 230K agent
this is invisible; with a `user_cap` of ~16K and many resident agents it is
reachable. Either raise `ctx_overhead_mib` to ~640 for hybrid models, or —
better and consistent with fix 1 — teach the geometry to add a per-context
recurrent-state term derived from the `ssm.*` keys, which are all present in
the GGUF (`ssm.inner_size = 4096`, `ssm.state_size = 128`,
`ssm.conv_kernel = 4`, `ssm.group_count = 16`).

**5. `/status` exposes nothing wrong.** `digest` equals the file's real
sha256; `loaded_weights_bytes` equals the file size (conservative, as
designed); `kv_per_token` reported the derived 81,920 in boot A and the
declared 20,480 in boot B with `kv_per_token_declared` flipping `false` ->
`true` correctly; `training_ctx` 262,144 correct; `codec_gate`,
`refusal_gate`, and `done_trust: false` all rendered correctly. **The one
misleading field is `kv_per_token` in the no-override case — it is honestly
reporting a wrong derivation, which fix 1 addresses at the source.**

### (c) The untrained G4/G5 baseline vs stock-14B, and what a flywheel turn costs

| model | G4 on v1 @v4 | G5 patch | G5 refuse | `done_trust` |
|---|---|---|---|---|
| stock 14B (untrained) | 6/20 | 5/16 | 8/16 | false |
| **REAP-48 35B-A3B (untrained)** | **20/20** | **16/16** | **5/16** | **false** |
| flywheel4 14B (**trained**, 4 turns) | 20/20 | 16/16 | 16/16 | true |

**This is a genuinely surprising baseline and the most consequential finding
of the spike.** On both *capability* legs the untrained REAP model is already
**at the trained flywheel4 ceiling** — G4 20/20 and patch 16/16, versus stock
14B's 6/20 and 5/16. Four flywheel turns on the 14B bought what this model
arrives with.

**It is worse than stock on the one leg that is left.** Refuse **5/16** vs
stock's 8/16 — a decided fail, below the floor model. The mechanism is
visible in the verb histogram (§4.5/§4.6): it is over-eager, patching where
it should refuse, and `done 45` on 32 fixtures.

So the flywheel's job on this base is **exactly one leg, not four turns'
worth of work**. Two legs are already maxed and cannot improve; the entire
gradient is refusal-honesty. That is a much narrower and better-posed target
than the 14B's starting position — but also a harder one, because there is no
capability headroom left to trade against, and the two maxed legs must be held
while refusal is trained (a patch regression would be the first thing to
watch).

**Cost of adopting it as a new base: re-baselining everything.** Concretely:

- Every v4 anchor in `2026-08-21-g5v4-baselines.md` is a 14B number. A new
  base needs its own committed baseline pair, pre-registered, before any
  training step — the numbers in *this* file are a **spike, not a baseline**:
  one boot per leg, no pre-registration, `n=1` speed samples, and the
  secondary endpoints demonstrably mis-attributed (§4.5).
- The blessed profile, drift references, and the assay `0.13.0`/v10 pin all
  re-establish per model.
- **The two ceilings are not comparable** (§3.5): this model's 16,384 is
  cap-bound (`none_up_to_cap`), the 14B's 12,288 was window-bound. Any
  ceiling comparison across the two is invalid as written.
- Training-side cost is unmeasured here and is the real unknown: a 35B-A3B
  MoE with 133 experts is a different fine-tuning proposition from a dense
  14B, and nothing in this spike touched training.

**Suggested next step, if Brice wants to pursue it:** do fix 1
(hybrid-aware KV derivation) and fix 4 (recurrent-state overhead) as a small
normal change with tests, then — separately and pre-registered — a proper
two-boot baseline battery on this model at the **no-override** geometry, since
that is the configuration worth shipping.

---

## 6. Housekeeping

- **No repo commits.** `git status` shows zero modified tracked files; the only
  addition is the untracked `.superpowers/spikes/` holding this file.
  `.gitignore` was left unmodified.
- **Nothing was built.** The featured binary was already newer than the last
  `crates/` change, so no rebuild; `cargo test` was not run.
- Both daemons brought down by **verified PID** (2789473, 2798704), each
  `readlink /proc/<pid>/exe`-asserted against the featured binary immediately
  before `kill`, then polled to exit. **No `pkill`. Nothing wrapped in
  `timeout`.** GPU returned to 1,193 MiB with no `bloomery-daemon` running.
- **Standing drift home never touched** —
  `find ~/.local/share/bloomery -newermt "2026-08-21 22:40"` returns nothing,
  checked after both boots. Each boot used a dedicated scratch `data_dir`
  under `target/reap-spike/` and blessed its own first profile
  (`provenance: auto-first-profile`).
- Idle **`ollama serve` PID 3696348 holding 0 MiB** — reported, not killed.
- **GGUF left in place** at `/home/brice/models/gguf/qwen36-reap-48pct-mixed-q3k.gguf`.
- Retained scratch: `target/reap-spike/{a,b}/` — configs, `daemon.out`,
  final `/status` snapshots, journals, profiles. `target/` is gitignored.
- The recompute script was **validated against the committed fw4 baselines
  before touching spike data**: it reproduces G4 20/20, patch 16/16, refuse
  16/16, productive find 6, find-usage 6, run-before-done 5, productive run 5,
  per-family 6·5·5 — matching `2026-08-21-flywheel4-battery.md` §5.3 exactly
  (Wilson lower bound differs by one ULP from my clamp ordering).

