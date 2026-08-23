# Flywheel turn 5 — training record (rental run, S3 upload, sha chain, costs)

Governs: `docs/superpowers/evidence/2026-08-22-flywheel5-preregistration.md`
(commit `84e5a57`). This document records what actually happened when the
pre-registered recipe was executed on rented hardware. **No fixture, floor,
endpoint, seed, corpus, or recipe parameter was changed after a number was
seen.** All deviations below are infrastructure/tooling facts, recorded
verbatim, per the prereg's own amendment rule. A separate, second dated file
(`2026-08-23-flywheel5-preregistration-amendment-1.md`) records the cost/cloud
assumptions that changed; this file is the factual run record.

Turn cap: **$10**. Total spend this turn: **$0.4605308916 (pod 1, terminated)
+ $5.8584331859 (pod 2, this run) = $6.3189640775 (≈$6.32)** — well within
cap. Balance: prereg-time $12.9647986767 → final $6.5490071799.

---

## 1. Pod ledger (both pods)

| | Pod 1 (terminated) | Pod 2 (this run) |
|---|---|---|
| id | `7al24l12yuhaqs` | `dh3v0u3byzajpf` |
| GPU | NVIDIA A100-SXM4-80GB | NVIDIA A100-SXM4-80GB |
| datacenter | `US-WA-1` | `US-WA-1` |
| cloud type | SECURE (COMMUNITY: "no instances currently available") | SECURE (COMMUNITY: "no instances currently available") |
| `costPerHr` | $1.59 | $1.59 |
| container disk | 150 GB | 150 GB |
| network volume | `s8qomynzbd` (50 GB) | `s8qomynzbd` (50 GB) |
| cut (UTC) | 2026-08-22T23:42:59.639Z | 2026-08-23T05:07:25.77Z |
| terminated/torn-down (UTC) | ≈2026-08-22T23:59Z, confirmed empty 00:02:18Z | confirmed empty 2026-08-23T08:45:53Z |
| elapsed | ≈17-19 min (≈0.29h) | ≈3h38m (≈3.63h) |
| balance before | $12.9453542323 | $12.4074403658 |
| balance after | $12.4848233407 | $6.5490071799 |
| spend | **$0.4605308916** | **$5.8584331859** |
| outcome | **BLOCKED**: base-model upload over the pod's own SSH path measured ≈2.665 MB/s aggregate (≈21.3 Mbps) — the local box's outbound uplink ceiling, root-caused via `/proc/net/dev`, not a competing process or fixable transfer-method choice. Projected full upload ≈4h/≈$6.36 — would have consumed the whole cap before training started. Stop rule invoked; pod terminated at discovery. | **Full success**: base+corpus already on the volume via the out-of-band S3 upload (§2 below); smoke, full training, post-train chain, download, and teardown all completed. |

Both `costPerHr` figures deviate from the pre-registration's assumed
$1.39/h COMMUNITY rate (COMMUNITY had zero A100-SXM4-80GB availability at
both cut attempts, ≈9 hours apart, in the only two volume-capable datacenters
that ever showed any stock). This is a datacenter-availability fact, not a
recipe change — recorded formally in the amendment file.

Pod 2's `machineId`: `2kbys5tpjs02`. `memoryInGb: 250` (host RAM, ample for
the CPU merge step later). 32 vCPU. Public IP `195.26.233.70`, SSH port
`48640` (appeared on the 7th poll of a 25s cadence, ≈3 min, within the 5-min
budget).

---

## 2. The S3 API base upload (out of band, between the two pod attempts)

Pod 1's own SSH-path upload was infeasible at the local box's measured
uplink (≈21 Mbps). Rather than accept a much larger cost cap, the base model
was uploaded directly to RunPod's S3-compatible object API — **no pod
running during the upload**, so it cost $0 of pod time regardless of how
long it took.

- **Tool**: `~/flywheel5/s3_upload.py` (local, not committed to the repo;
  credentials via `~/.aws` profile `runpods3`, never CLI args, never logged
  — matches the security rule against embedding secrets). Resumable
  multipart upload via `boto3`, state persisted to
  `~/flywheel5/s3-upload.state.json` after every part so a crash can resume
  without re-uploading completed parts.
- **Endpoint**: `https://s3api-us-wa-1.runpod.io/`, region `us-wa-1`.
- **Method**: 4,572 parts × 8 MiB each (`--part-mb 8`), **2 concurrent
  threads** (`--concurrency 2`) — well below pod 1's 6-way parallelism,
  because this transfer ran unattended for hours and didn't need to race a
  billed clock; the constraint was total wall time tolerance, not peak
  throughput.
- **Measured rate**: **≈2.3 MB/s average** (from the upload's own per-part
  log, `~/flywheel5/s3-upload.log`) — essentially the same uplink ceiling
  pod 1's SSH-path measurement found (≈2.665 MB/s), confirming this is a
  genuine local-box uplink limit, not a pod-path-specific bottleneck.
- **Timeline**: started ≈2026-08-22T19:36 CDT, last part (4572/4572)
  completed 2026-08-22T23:57:40 CDT — **≈4h22m wall time**, entirely
  off the RunPod billing clock.
- **Client-side crash on the last part, recorded honestly**: the uploader's
  `save_state()` writes a temp file and `os.replace()`s it into place after
  every part; on the very last part this raced with something else touching
  the same directory and threw
  `FileNotFoundError: [Errno 2] No such file or directory:
  '/home/brice/flywheel5/s3-upload.state.json.tmp' ->
  '/home/brice/flywheel5/s3-upload.state.json'` — a real bug in the
  uploader's state-file handling under concurrent access, not investigated
  further since it didn't affect correctness (below).
- **The multipart upload itself completed successfully server-side** despite
  the client crash: `~/flywheel5/s3-upload.DONE` records
  `38349435696 (HEAD OK 2026-08-23T00:05:14 CDT; completed server-side
  after the client-side timeout; upload session closed)` — a subsequent
  `head_object` call confirmed the object existed at exactly the expected
  size before this task began.
- **Verified independently on the pod** (§3): `ls -la` size and `sha256sum`
  both matched exactly, so the crash-adjacent completion is confirmed
  correct, not merely claimed.

---

## 3. Base + corpus verification on the pod

Per the task's binding instruction, the base sha was **re-verified on the
pod before anything else**, run detached (`setsid nohup sha256sum … &`,
polled via the output file, never `pgrep -f`).

- `ls -la /workspace/Qwen3.6-35B-A3B-REAP48-ours/`: all 9 expected sidecar
  files present (`config.json generation_config.json generation_config.json
  tokenizer.json tokenizer_config.json vocab.json merges.txt
  chat_template.jinja reap_pruning.json summary.json`) plus
  `model.safetensors` at exactly **38,349,435,696 bytes** — matches the
  S3 `head_object` size and the pre-registered expectation exactly.
- `sha256sum model.safetensors` → **`8027ca0a8277b540cd4c62eb7a5bdf6028875e84b33ddcf4f9cd4b0e9d63423b`**
  — **exact match** to the pre-registered value. Ran ≈3 min (uninterruptible-sleep-bound on the mfs read).
- `sha256sum corpus.jsonl` → **`9c51a8668b4ce861dbe3c8528f59655a9a78eee12523f397b33e28d5d7928a7d`**
  — **exact match**.
- Both matched exactly; training proceeded per the binding rule ("if it does
  NOT match: STOP, terminate the pod, report BLOCKED" — not triggered).
- `df -h /workspace /`: `/workspace` (mfs network volume backend) 615T/415T
  used/200T avail (shared cluster backend, not the 50GB volume's own quota
  — see §6 for why this distinction mattered); `/` (container disk, overlay)
  150G/3.9G used/147G avail. `free -g`: 2003 GB total host RAM.

---

## 4. Environment — a deviation found and corrected before any job ran

Followed the brief's Step 3 pip/clone/build chain verbatim: `pip install
transformers==5.5.0 peft==0.20.0 accelerate==1.14.0 safetensors==0.8.0
flash-linear-attention==0.5.2 numpy` → clone `llama.cpp` @ `8672290` →
`pip install -r requirements/requirements-convert_hf_to_gguf.txt` → cmake
build `llama-quantize` → clone `bloomery` @ `flywheel5-turn5` → `pip freeze`
→ version print.

The first `pip install` correctly preserved the image's pre-installed
`torch 2.9.1+cu129` (log: `Requirement already satisfied: torch>=1.13.0 ...
(2.9.1+cu129)`) and installed `transformers 5.5.0` as pinned.

**Deviation found: `requirements/requirements-convert_hf_to_gguf.txt` at
commit `8672290` carries *exact* pins `torch==2.11.0` (from
`--extra-index-url https://download.pytorch.org/whl/cpu`, a CPU-only wheel)
and, via its own `-r requirements-convert_legacy_llama.txt`,
`transformers==4.57.6`.** Installing it per the brief's literal one-line
chain **uninstalled** the correct `torch 2.9.1+cu129` and `transformers
5.5.0` and replaced them with `torch 2.11.0+cpu` (`torch.cuda.is_available()
== False`) and `transformers 4.57.6` — verbatim from `env-setup.log`:

```
Attempting uninstall: torch
  Found existing installation: torch 2.9.1+cu129
  Uninstalling torch-2.9.1+cu129:
    Successfully uninstalled torch-2.9.1+cu129
...
Successfully installed ... torch-2.11.0+cpu
...
Attempting uninstall: transformers
  Found existing installation: transformers 5.5.0
  Uninstalling transformers-5.5.0:
    Successfully uninstalled transformers-5.5.0
...
Successfully installed ... transformers-4.57.6
```

First version-check printed `2.11.0+cpu 4.57.6 0.20.0 False` — a smoke test
or training run in this state would have burned billed A100 time on
CPU-only torch, a total waste of the rental.

**Caught before the smoke test or any job started** — pure environment
setup, no training number seen yet, not a mid-run recipe change.
**Corrective action**: `pip install --break-system-packages "torch==2.9.1"
--index-url https://download.pytorch.org/whl/cu129` then `pip install
--break-system-packages "transformers==5.5.0"`. `pip freeze` re-captured
(overwriting the polluted one), versions re-printed:

**`2.9.1+cu129 5.5.0 0.20.0 True`** — exact match to the brief's expected
string. `torchaudio 2.9.1+cu129` (already present, untouched by either pip
call) confirms this is the same torch build the image originally shipped,
not a different/incompatible one.

`pip freeze` (corrected) recorded at `~/flywheel5/pip-freeze.txt`. No
further `pip install` after this point.

---

## 5. Smoke test (`--max-steps 5`) — PASS

Ran 2026-08-23T05:21:47Z → 05:25:35Z (≈3m48s wall; includes weight load + 1
full eval pass over 221 val rows + 5 train micro-steps).

- `model class Qwen3_5MoeForCausalLM; num_experts=133; layers=40` — matches.
- `trainable 21166080 / total 19194718848 (0.1103%) — experts+router
  frozen: asserted`. Trainable count (21,166,080 ≈ 21.17M) matches the
  spike's quoted figure almost exactly. The **percentage** (0.1103%) differs
  from the brief's quoted spike figure (0.0611%) because the total parameter
  count differs by base (REAP-48-pruned total 19.19B vs whatever the spike's
  figure was computed against) — the brief explicitly names 0.0611% "the
  pre-registered expectation, not this run's measured value," so this is the
  real measurement superseding a placeholder, not a discrepancy.
- `pairs: train=4340 val=221` — exact match to the prereg's stated split.
- `label-check ok: 341 prompt tokens masked, tail='\n</action>'` — matches.
- 5 steps completed, `rc=0`. `eval_loss 0.4175` (56.85s, 3.887 it/s over 221
  rows), `train_loss 0.7223` (5-step summary only, not interpreted).
- **Peak VRAM: 51,715 MiB** (~50.5 GB of 80 GB), sampled via `nvidia-smi
  --query-gpu=memory.used` every ~15s during the run.

---

## 6. Training run — COMPLETE

`train.DONE` + `train.EXIT` = **0**, launched via `setsid nohup
train-wrapper.sh` (byte-identical to the brief's Step 5 script) at
`2026-08-23T05:26:xxZ`, done `08:17:13Z`.

**`train_moe.py`'s live tqdm stream did not print intermediate `{'loss':
...}` dicts during the full run** — a property of this HF Trainer /
`report_to=[]` combination observed live (the smoke run's *final* summary
dict did print, since that fires unconditionally at `state.global_step ==
max_steps` regardless of the periodic schedule). Not investigated further:
`trainer_state.json`'s `log_history` is saved unconditionally on the success
path and carries the complete record regardless of what printed live.

### Final summary (`trainer_state.json`)

```
epoch 2.0, step 1086, train_loss 0.015204, train_runtime 10222.1039 s
(2.840 h), train_samples_per_second 0.849, train_steps_per_second 0.106,
total_flos 4.473e17
```

Total optimizer steps: **1086** (4340 train pairs × 2 epochs / grad-accum 8
≈ 1085, +1 rounding) — matches the prereg's `PINNED_ARGS`. Measured pace
over the first 10 steps: ≈9.2 s/optimizer-step (≈1.15 s/micro-step) —
**faster than the brief's ≤1.52 s/micro-step upper bound**, holding roughly
steady (7-10 s/optimizer-step) for the whole run.

### Train loss curve (`logging_steps=10`, selected points; full 108-point curve in `trainer_state.json`)

| step | loss | lr |
|---|---|---|
| 10 | 0.5633 | 9.0e-5 |
| 20 | 0.2786 | 1.9e-4 |
| 30 | 0.2998 | 2.00e-4 |
| 40 | 0.0951 | 2.00e-4 |
| 50 | 0.0699 | 2.00e-4 |
| … | (decaying, noisy at small scale) | cosine schedule |
| 990 | 0.00364 | 4.06e-6 |
| 1000 | 4.0e-5 | 3.27e-6 |
| 1080 | 4.0e-5 | 2.1e-8 |

### Eval loss curve (`eval_steps=100`, all 11 points, full run)

| step | eval_loss | epoch |
|---|---|---|
| 100 | 0.013030 | 0.184 |
| 200 | 0.002692 | 0.369 |
| 300 | 0.011117 | 0.553 |
| 400 | 0.001480 | 0.737 |
| 500 | 0.001421 | 0.922 |
| 600 | 0.001210 | 1.105 |
| 700 | 0.000972 | 1.289 |
| 800 | 0.001023 | 1.474 |
| 900 | 0.000992 | 1.658 |
| 1000 | 0.000987 | 1.842 |
| **1086 (final)** | **0.000985** | 2.000 |

Monotonic-ish decline with a step-300 bump; converges to ≈0.001 by step
≈600 and stays flat — consistent with a completion-only-loss objective on a
structured, low-entropy action-grammar corpus. **Per the prereg's honesty
line, eval loss is monitored, never a gate signal** — reported here for the
record only; the battery (G4/G5, human-gated) is the sole decision
instrument.

### Adapter artifacts

`/workspace/flywheel5/adapter/`: `adapter_model.safetensors`
(**84,751,528 bytes**), `adapter_config.json`, `tokenizer.json`,
`tokenizer_config.json`, `chat_template.jinja`, `README.md`,
`trainer_state.json`. Adapter size ≈84.75 MB bf16 — roughly double the
brief's "~40MB" pre-registered estimate (peft saves both LoRA A/B matrices
at full bf16 without additional compression); a size note, not a recipe
deviation.

---

## 7. Post-train chain

### Merge — succeeded

`peft.PeftModel.merge_and_unload()` on CPU (per `free -g` ≥80GB check —
host has 2003GB), bf16. `=== merge === 2026-08-23T08:17:59Z` → `merged ok`.
Saved to `/root/flywheel5-scratch/merged/` — see the storage-location
deviation below for why not `/workspace`.

### First `convert_hf_to_gguf.py --outtype bf16` attempt — FAILED

At `2026-08-23T08:28:30Z`, `rc=1`:

```
File "/workspace/llama.cpp/conversion/qwen.py", line 303, in __init__
    assert self.opt_num_mtp_layers != 0
AssertionError
```

**Root cause** (read from `conversion/qwen.py`'s `_QwenMtpMixin.__init__`):
when `hparams.get("mtp_num_hidden_layers", 0) == 0` (true for this
REAP-pruned, genuinely MTP-free checkpoint) and the converter's
MTP-inclusion path is active by default, it asserts `opt_num_mtp_layers !=
0` — a class attribute only ever populated by `filter_tensors()` scanning
actual `mtp.*` tensors during indexing, which runs *after* `__init__`. On a
checkpoint that truly has no MTP tensors, this assertion fails immediately,
before indexing ever has a chance to prove there's nothing to count. A
genuine tooling gap for MTP-free checkpoints in this converter revision —
not a bug in the training recipe or corpus.

**Fix**: `convert_hf_to_gguf.py` defines `--no-nextn` / `--no-mtp` (`dest
no_mtp`) as its own documented flag: *"Exclude NextN speculative draft
tensors from the converted GGUF."* Setting it makes `model_class.no_mtp =
True`, which skips the MTP-layer-counting branch in `_QwenMtpMixin.__init__`
entirely, leaving `block_count = hparams["num_hidden_layers"]` (40) exactly
as expected, no assertion touched. **This is the conversion tool's own
supported mechanism for a checkpoint property (`mtp_num_hidden_layers: 0`)
that was already known and pre-registered before training started — not a
metadata patch, not a recipe change.** The merge output was preserved (not
re-run); the chain resumed from the convert step via a new
`posttrain2.log`/`.EXIT`/`.DONE` sequence with `--no-mtp` added (the failed
`posttrain.log` kept as evidence, not deleted).

### Second convert attempt (`--no-mtp`) — succeeded

`2026-08-23T08:31:15Z` → wrote `/root/flywheel5-scratch/fw5-bf16.gguf`,
**36,626,527,552 bytes**, exporting all tensors normally (`output.weight`,
`token_embd.weight`, `blk.0.*` including `ssm_a`, `ssm_alpha`, `ssm_beta`,
`attn_qkv` — consistent with the hybrid attention/Gated-DeltaNet
architecture). `llama_model_quantize_impl: model size = 36593.80 MiB (16.01
BPW)` at the quantize step confirms this bf16 GGUF is the expected size.

### Quantize Q4_K_M — succeeded

`/workspace/llama.cpp/build/bin/llama-quantize ... Q4_K_M`:
`quant size = 11200.56 MiB (4.90 BPW)`, `quantize time = 128831.85 ms`
(≈2.15 min). Output `qwen36-reap48-flywheel5-Q4_K_M.gguf`, exactly
**11,755,624,192 bytes**.

### `block_count` check — PASS, no STOP

**`block_count 40`** — exact match to the prereg's expectation. The
brief's binding rule ("a 41 here is a STOP — do not metadata-patch, report")
was not triggered.

### `nextn`/`mtp` metadata — confirmed absent

Explicitly checked via `gguf.GGUFReader` over all 42 kv fields in the final
Q4_K_M GGUF: **no key matched `nextn` or `mtp`** — confirming `--no-mtp`
produced a clean, architecturally correct MTP-free GGUF, not merely an
omitted-but-implied field.

### sha256 (computed on the pod)

```
abfcf6596db2c072d840e33b6e86907c51f2f062a2e8e233890079c173c5a6b6  adapter_model.safetensors
7020b925c07c5a3808e1155700ca707598cb4f6d6089bb6daff4147b4d6b00bd  qwen36-reap48-flywheel5-Q4_K_M.gguf
9c51a8668b4ce861dbe3c8528f59655a9a78eee12523f397b33e28d5d7928a7d  corpus.jsonl (re-verified, unchanged)
```

### Storage-location deviation (infrastructure, decided ahead of the step)

The network volume `s8qomynzbd` (`/workspace`) is nominally **50 GB**; the
base model alone already occupies 38.35 GB of it, leaving only ≈11.6 GB of
headroom. The brief's post-train wrapper writes the merged bf16 checkpoint
(~38 GB) and the bf16 GGUF (~38 GB) — both explicitly scratch, "die with the
pod" per the brief's own text — which would not fit in that headroom. The
container disk (`/`, overlay) has 150 GB total / 147 GB free and is not
subject to the volume's quota. **Deviation**: `merged/`, `fw5-bf16.gguf`,
and the final Q4_K_M GGUF were written to `/root/flywheel5-scratch/`
instead of `/workspace/flywheel5/...`. The adapter (already written by the
training run to `/workspace/flywheel5/adapter/`, ~85 MB, fits the volume's
headroom fine) was left where the training job put it. Storage-location fix
only — no training hyperparameter, seed, corpus, or recipe value changed;
decided before the post-train step ran, not after seeing any number from
it.

### CARRIED-DEBT candidate (recorded per controller ruling)

"the prune tool zeroes `mtp_num_hidden_layers` for absent MTP weights;
llama.cpp `8672290`'s converter asserts on 0 unless `--no-mtp` is passed —
the tool should delete the key (or the runbook must pass `--no-mtp`), and
the prune GGUF test must cover this converter path."

---

## 8. Bring it home

- Small artifacts (adapter files, `trainer_state.json`, `train.log`,
  `pip-freeze.txt`, `smoke.log`, `posttrain2.log`, the failed
  `posttrain.log` [kept], `train-wrapper.out`, `base.sha`, `corpus.sha`,
  `env-setup.log`, `env-fix.log`) — plain `scp`.
- **Q4_K_M GGUF** (11,755,624,192 bytes): **6 parallel byte-range `dd`
  streams** (`bs=1M skip=$((i*1869)) count=1869`, `i=0..5`) over ssh,
  redirected to local part files, `cat`'d together in order — adapted from
  the brief's split+scp method to ssh+dd since the source now lives on the
  container disk (`/root/flywheel5-scratch/`) rather than a directory scp's
  split-file convention addresses directly. **Local size exactly matches**
  (11,755,624,192 bytes). **sha256 of the reassembled local file:
  `7020b925c07c5a3808e1155700ca707598cb4f6d6089bb6daff4147b4d6b00bd` —
  MATCHES the pod's value exactly.**
- Adapter sha256 at home: `abfcf6596db2c072d840e33b6e86907c51f2f062a2e8e233890079c173c5a6b6` — matches.
- Corpus sha256 at home: `9c51a8668b4ce861dbe3c8528f59655a9a78eee12523f397b33e28d5d7928a7d` — matches.
- `~/flywheel5/SHAS.txt` updated with the full chain (base / corpus
  before+after / adapter / Q4_K_M pod+home) plus the `block_count`/`nextn`
  facts.
- **No local boot attempted** — Task 11's job, per instructions.

---

## 9. Teardown

`DELETE /v1/pods/dh3v0u3byzajpf` → **HTTP 204**. Verified via **both** REST
`GET /v1/pods` → `[]` and GraphQL `myself { pods { id } }` → `[]`, at
`2026-08-23T08:45:53Z`. **Balance after: $6.5490071799.** Network volume
`s8qomynzbd` left intact (holds the base + corpus for any future turn;
Brice decides later whether to delete it).

---

## 10. Cost summary

| phase | pod | cost |
|---|---|---|
| pod 1 (blocked, terminated) | `7al24l12yuhaqs` | $0.4605308916 |
| pod 2: cut → smoke → train → post-train → download → teardown | `dh3v0u3byzajpf` | $5.8584331859 |
| **turn total** | | **$6.3189640775 (≈$6.32) of the $10 cap** |

Balance: prereg-time $12.9647986767 → final $6.5490071799.

---

## 11. Every deviation from the runbook, listed together

1. **Cloud type**: COMMUNITY unavailable both cut attempts (≈9h apart, only
   volume-capable DC pair with any A100-SXM4-80GB stock) → SECURE used both
   times, $1.59/h vs the prereg's assumed $1.39/h COMMUNITY. See the
   amendment file for the cost-bound recomputation.
2. **Upload path**: pod-SSH-path upload infeasible at the local box's
   measured ≈21-27 Mbps uplink → base uploaded via RunPod's S3 API
   out-of-band (no pod running), 8 MiB parts, 2 threads, ≈4h22m, $0 pod
   cost. See the amendment file.
3. **Environment**: `pip install -r requirements-convert_hf_to_gguf.txt`
   clobbered the pinned `torch 2.9.1+cu129`/`transformers 5.5.0` with
   `torch 2.11.0+cpu`/`transformers 4.57.6` (exact pins in that
   requirements file) — caught and corrected before the smoke test (§4).
4. **Storage**: merge/bf16-GGUF/Q4_K_M scratch redirected from the 50GB
   network volume (headroom exhausted by the base model) to the 150GB
   container disk (§7).
5. **Conversion flag**: `convert_hf_to_gguf.py` invoked with `--no-mtp`
   (not in the brief's literal command) after the first attempt failed on
   an MTP-layer assertion for this genuinely MTP-free checkpoint (§7).

No fixture, floor, endpoint, seed, corpus, or recipe parameter was changed
at any point. The battery (G4/G5, human-gated, per the pre-registration) is
unaffected by any of the above and remains the sole decision instrument for
this turn's outcome.
