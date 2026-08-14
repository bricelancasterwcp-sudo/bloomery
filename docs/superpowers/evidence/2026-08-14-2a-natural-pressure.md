# Phase 2a — eviction under a natural measured budget

**Date:** 2026-08-14
**What this is:** the acceptance run for Phase 2a's pager work — weights charged
to the reservation budget, and therefore residency pressure that arises from the
hardware instead of from a disabled probe.

> **This is not a gate reading.** G2 (`docs/gates.md`) is pinned, was read once
> on 2026-08-14, and **stands exactly as published**: p95 warm 32 ms, p95 cold
> 862 ms, n=56 per class. Nothing here re-reads it, amends it, or is computed
> against its ceilings. This page runs the *warm class only*, at a window 8×
> larger, under a different pressure mechanism, for a different question:
> **does the pager evict when the arithmetic — not a missing tool — says it
> must?** Latencies are reported because they were recorded, not because they
> are being judged. No number below may be substituted for a G2 number.

---

## The question, and why it needed a run

Phase 1's residency planner charged KV bytes only. On this box, with this
model, that made natural eviction impossible: forcing one needed
`2·kv > free_vram` while getting a non-zero window needed `kv ≤ free_vram −
weights`, and together those wanted more VRAM than the card has (G2 evidence,
§2). So G2 arranged pressure the other way — a `PATH` with no `nvidia-smi`, an
unmeasured probe, residency capped at one resident agent. Real switches, real
images, real GPU; an artificial reason for them to happen.

Phase 2a charges the weights to the same pool:

```
avail = budget − Σ loaded weights − Σ resident kv
```

That closes the gap on paper. This run is the check that it closes it on the
box: a measured `nvidia-smi` budget, no `PATH` games, no `overhead_mib`
trickery, and a workload sized so the arithmetic predicts evictions.

## Pressure arithmetic — pre-registered for attempt 1

Computed before the run from the card and the model, and computed again by the
driver at run time from the daemon's own `/status`. Both appear below; the
driver's numbers are the ones that count, and they are printed by the
instrument, not typed by hand.

| term | pre-run estimate |
|---|---|
| budget (boot free VRAM) | ≈ 14 300 MiB |
| weights | 8 098 525 696 B = 7723 MiB |
| remainder | ≈ 6 500 MiB |
| context at `--window 16384` | 57 344 B/token × 16 384 = 939 524 096 B = 896 MiB |
| capacity | ≈ 7 contexts alongside the weights |
| restores per lap | `agents + 1 − capacity` = 8 + 1 − 7 = **2** |
| run total | 8 laps × 2 = **16** switch samples |

The lap-opening reset agent is why the formula carries the `+ 1`: it evicts the
lowest-priority resident and then suspends, so a lap starts with the
`capacity − 1` highest-priority workers resident and every worker below them
must be restored as its turn comes. At `capacity = 1` — G2's regime — the same
formula gives `agents` restores per lap, which is exactly the 56 samples G2
expected from 8 agents × 7 laps. The rule generalises the old expectation
rather than replacing it.

**16 samples is an acceptance count, not a gate count.** G2 pre-registered ≥50
per class and got 56. Nothing here is read at a percentile that needs that many;
what this run has to show is that evictions happen at all, under the measured
budget, with the weights charged.

**This table is wrong, and that is the finding.** It is left exactly as
pre-registered because attempt 1 falsified it on the hardware: the row
"context at `--window 16384` = 896 MiB" counts the KV cache and nothing else,
and a llama.cpp context costs more than its KV cache. What that omission did,
and what replaced it, is [Attempt 1](#attempt-1--aborted-oom-and-the-accounting-gap-it-found) below.

## Box

| | |
|---|---|
| GPU | NVIDIA GeForce RTX 5080, 16303 MiB total |
| Driver | 595.84; substrate on the **Vulkan** backend (`--features llama,vulkan`) |
| Tier declared | `enthusiast-16gb`, `emulated = false` — **real hardware** |
| CPU | 16 cores |
| Host RAM | 30 649 MiB total |
| `data_dir` | `/home/brice/.cache/bloomery-2a` on `/dev/nvme0n1p2` (ext4) — **not** `/tmp`, which is tmpfs on this box and would have made the image store's NVMe tier RAM-backed |
| GGUF blob | `/mnt/extra/ollama-models/blobs/…` on `/dev/nvme1n1p1` (ext4) |

## Model

| | |
|---|---|
| Name | `qwen2.5-coder:7b-instruct-q8_0` |
| Blob | `/mnt/extra/ollama-models/blobs/sha256-24b532e5276503b147d0eea0e47cb1d2bcce7c9034edd657b624261862ca54a1` |
| Size | 8 098 525 696 bytes (7723 MiB) |
| Geometry from the GGUF | `kv_per_token` = 57 344 B, `training_ctx` = 32 768 |
| Daemon blob identity | `model_digest` = `24b532e5276503b147d0eea0e47cb1d2bcce7c9034edd657b624261862ca54a1` |

The blob path is the `FROM` line of `ollama show qwen2.5-coder:7b-instruct-q8_0
--modelfile`. Same blob as G2's, so the only deliberate differences from that
run are the window, the pressure mechanism, and the code.

**The digest is a check on Phase 2a's second work item, and it passes in the
open.** G2's daemon reported `a6c11e79e7d4…` — `sha256(first 1 MiB ‖ file_len)`,
bloomery's own prefix construction, a number nothing else in the world could
confirm. This daemon reports `24b532e527…`, which is *character for character*
the blob's file name — ollama's content address, i.e. the file's real SHA-256,
computed by an unrelated program on a different day. Full-file digest,
independently corroborated, without anyone having to trust this document.

## Code

| | |
|---|---|
| Branch | `feat/phase2a-hardening` |
| Daemon sources, attempt 1 | `d2e33e8` (`fix: pager — timeshare sufficiency check, last-use-at-placement ruling, determinism pins`) |
| Daemon sources, attempt 2 | `a79a804` (`fix: pager — per-context runtime reservation + overhead in placement`) — the exact tree both release binaries were built from, 15 minutes before the run |
| Instrument | `f227e9d` (`feat: bench — pressure arithmetic replaces the measured-budget refusal`), committed **before** attempt 1, as G2's was; `a79a804` extends its capacity formula with the two terms attempt 1 found |
| After the run | `284c6bb` moves the `/status` builder between files to stay under the 800-line ceiling. Pure code motion, committed separately so the tree named above stays the one that was measured. |
| Build | `cargo build --release --features llama,vulkan -p bloomery-daemon -p bloomery-bench` |

---

## The lens

### What one sample is

Unchanged from G2, and computed by the same pure code
(`crates/bloomery-bench/src/report.rs`): the sum of `duration_ms` over the
contiguous pager-op sequence serving one resume — `EvictSave` of the victim,
`ResumeLoad` of the target, plus `ModelLoaded` if the weights had to come back.
**warm** = RAM-tier image, no `ModelLoaded` in the sequence.

### The pressure mechanism — the whole point of this run

`nvidia-smi` is on `PATH`. The probe measures. `/status` reports a number, not
`null`, and the journal contains **no** `Degraded {"reason":"vram unmeasured…"}`
line. Every eviction below was planned against measured VRAM with the weights
subtracted from it.

### Priorities, and why time-sharing never fires here

Phase 2a also added equal-priority time-sharing: a refusal between equal peers
that has waited out `time_share_quantum_secs` is retried as an eviction of the
LRU peer. The bench does **not** exercise it, deliberately. Workers carry
strictly ascending priorities (10, 20, … 80) and are visited in that order, so
every step is an ordinary planner eviction; a bench leaning on the time-share
path would be measuring a 30-second quantum's clock rather than a switch. The
quantum is pinned in the config at its default (`time_share_quantum_secs = 30`)
so it is recorded rather than ambient, and it never elapses: no
`evict_timeshare` decision appears in the journal.

### What is different from G2, and what that should do to the numbers

| | G2 warm | here |
|---|---|---|
| window | 2048 tokens | **16 384 tokens** (8×) |
| VRAM per context | 112 MiB | **896 MiB** (8×) |
| pressure | unmeasured probe, cap of 1 resident | **measured budget, weights charged** |
| residents at a time | 1 | several |
| eviction victims per placement | always exactly 1 | whatever the planner names |

The KV *image* that moves is not 8× bigger: an image holds the tokens actually
used, not the window. Both runs prime with 6000 characters, so both move an
image of roughly the same size. What did change is the cost of *allocating* a
context — 896 MiB of VRAM per placement instead of 112 MiB — which sits inside
the substrate's context create/destroy on either side of a switch. Expect that
to show up; it is described below, not judged.

### Page cache

**Stated, not controlled.** This run makes no attempt to drop or warm any
cache, and the GGUF blob had been read by other processes on this box before it
started. Warm-class samples never touch the disk on the measured path (the
image is in the RAM tier), so the page cache is not on the critical path of the
numbers below — but the `SuspendSave` writes that spill reset-agent images to
the NVMe tier land in the page cache like any other write, and any restore that
came off the NVMe tier would have been served from it. G2's page-cache caveat
therefore still applies to anything cold, and the NVMe-media read cost remains
**unmeasured** (carried debt item 5).

---

## The run

### Attempt 1 — aborted (OOM), and the accounting gap it found

Fired automatically at **16:45:02** when the GPU cleared. It ran the
pre-registered invocation and **failed after 16 seconds**, on the sixth
context:

```text
boot-gate: free VRAM 13603 MiB, floor 13000 MiB
/status at boot: free_vram_bytes 14263779328, loaded_weights_bytes 0
created a1..a8  priority=10..80  window_tokens=16384
primed a1 (1132 prompt + 8 completion tokens, 281 ms)
primed a2 … a5   (268, 268, 267, 268 ms)
bloomery-bench: POST /agents/a6/infer -> 500 (wanted 200):
  {"error":"substrate_error","message":"model 1, n_ctx 16384: null reference from llama.cpp"}
bench exit 1
```

`bloomery-bench report` on that journal: **`warm n: 0`, `cold n: 0`, both
percentiles `null`.** The instrument did not report a fast daemon; it reported
nothing, loudly, with exit code 1 — which is the behaviour Task 5 built and the
one thing about attempt 1 that went right.

**Why it died.** `daemon.log`, verbatim, once per context:

```text
llama_prepare_model_devices: using device Vulkan0 (RTX 5080) - 13602 MiB free
load_tensors:      Vulkan0 model buffer size =  7165.44 MiB
sched_reserve:     Vulkan0 compute buffer size =   304.00 MiB
sched_reserve: Vulkan_Host compute buffer size =    30.01 MiB
```

and then, on the sixth:

```text
ggml_vulkan: Device memory allocation of size 939524096 failed.
ggml_vulkan: vk::Device::allocateMemory: ErrorOutOfDeviceMemory
alloc_tensor_range: failed to allocate Vulkan0 buffer of size 939524096
llama_init_from_model: failed to initialize the context: failed to allocate buffer for kv cache
```

A context on this box costs **896 MiB of KV cache + 304 MiB of `Vulkan0`
compute buffer + 30 MiB of host buffer**. The device arithmetic:
`13602 − 7165 = 6437 MiB` of headroom at `896 + 304 = 1200 MiB` of device
memory per context = **5 contexts**. Five allocated; the sixth did not.

The pager, meanwhile, charged the KV cache alone —
`(13603 − 7723) / 896 = 6` — and the last line of the aborted journal is that
belief, one instant before the device disagreed:

```json
{"event":"SchedulerDecision","id":"a6","decision":"fits","evicted":[]}
```

**This is the most valuable thing the run produced.** Phase 2a's headline was
that weights now enter the reservation budget; what this shows is that a
*second* term was missing from the same sum, and that no amount of test-suite
green could have found it — the gap only exists where bloomery's model of a
context meets llama.cpp's. Two lines below the OOM the daemon also proves the
instrument's other half was working: `SchedulerDecision "fits"` is a decision
the journal recorded, so the mistake is legible after the fact rather than
merely fatal.

**Recorded, not re-rolled.** No number was read off attempt 1 and none was
tuned. The journal is committed as
`2026-08-14-2a-aborted-oom-journal.jsonl` (28 events).

### The fix

Three changes, each pinned by tests before the rerun
(`crates/bloomery-daemon/tests/pager_reservation_test.rs`):

1. **Per-context reservation.** Every agent carries `reserved_bytes =
   kv_bytes + ctx_overhead_bytes`, and *that* is what residency plans
   against — placement demand, eviction sufficiency, the LRU tiebreak and
   `/status` all read it. The planner itself is untouched: its `kv_bytes`
   field means "bytes this residency holds, and bytes freed by evicting it",
   and the compute buffer satisfies both halves because llama.cpp frees it
   with the context.
2. **`ctx_overhead_mib`**, new config key, default **384** — above the 334
   MiB measured above, with the `daemon.log` lines quoted at the default's
   definition so the number's provenance travels with it.
3. **The daemon-level `overhead_mib` margin now enters placement too.** It
   had only ever been in the window law, which is why a 1 GiB "held back"
   margin was silently available for the pager to fill.

### Attempt 2 — the run

Same protocol, same flags, same box, 12 minutes later. The driver's own
pressure block, printed from `/status` before the laps:

```text
pressure:
  budget            14169407488 bytes (13513.0 MiB)
  daemon overhead   1073741824 bytes (1024.0 MiB)
  loaded weights    8098525696 bytes (7723.4 MiB)
  reserved / agent  1342177280 bytes (1280.0 MiB) — kv + per-context overhead
  capacity          3 contexts alongside the weights
  class             warm
  agents/rounds     8 / 8
  predicted         6 restores per lap, 48 for the run
  floor             5 restores per lap, 40 for the run
```

Invocation, verbatim (full driver: `scratchpad/run-2a.sh`, machine-local
paths, not committed):

```bash
target/release/bloomery-daemon --config bloomery-2a.toml   # nvidia-smi ON PATH
target/release/bloomery-bench switch \
  --daemon http://127.0.0.1:8181 --model qwen2.5-coder:7b-instruct-q8_0 \
  --agents 8 --rounds 8 --window 16384 --prime-chars 6000 --max-tokens 8 \
  --journal /home/brice/.cache/bloomery-2a/journal/boot-1786744644.jsonl
target/release/bloomery-bench report --journal <same file>
```

Config: `tier = enthusiast-16gb`, `emulated = false`, `assay.enabled = false`,
`allow_unprofiled = true`, `overhead_mib = 1024`, `ctx_overhead_mib = 384`,
`time_share_quantum_secs = 30`, `data_dir` on ext4 NVMe.

**Predicted 48 samples. Observed 48.** Exit 0.

```json
{
  "cold": { "n": 0, "p50_ms": null, "p95_ms": null },
  "warm": { "n": 48, "p50_ms": 32, "p95_ms": 34 }
}
```

`bloomery-bench report --journal
docs/superpowers/evidence/2026-08-14-2a-natural-pressure-journal.jsonl`
reproduces that block exactly, with no daemon and no GPU.

The prediction matching the observation to the unit is worth one sentence of
caution: it is not a second measurement, it is the same arithmetic the pager
used, checked against the behaviour it produced. What it rules out is a run
that switched by accident or not at all.

### Confirmations

| claim | evidence |
|---|---|
| the budget was **measured**, not the Phase 1 unmeasured cap | no `vram unmeasured` line anywhere in 353 events; the only two `Degraded` lines are `POST disabled by config` and the `allow_unprofiled` admission |
| evictions really happened | **53 `EvictSave`**, 48 `ResumeLoad`, 8 `SuspendSave`; 53 `SchedulerDecision "evict"` against 11 `"fits"` |
| nothing was refused or violated | zero `Refusal`, zero `ContractViolation` |
| weights were charged throughout | `/status` sampled every 10 s mid-run: `loaded_weights_bytes 8098525696`, `overhead_bytes 1073741824`, `ctx_overhead_bytes 402653184` |
| the capacity was real, not notional | the same samples show `resident_kv_bytes 4026531840` = exactly **3 × 1 342 177 280**, i.e. the predicted 3 residents, holding their whole reservation |
| time-sharing never fired | no `evict_timeshare` decision — workers carry ascending priorities, as designed |

### Component operations

| op | tier | n | ms min/median/max | bytes (median) |
|---|---|---|---|---|
| `EvictSave` | ram | 53 | 24 / 27 / 29 | 68 827 896 |
| `ResumeLoad` | ram | 48 | 5 / 5 / 6 | 68 397 726 |
| `SuspendSave` | nvme | 8 | 11 / 13 / 14 | 287 476 (reset agents) |
| `ModelLoaded` | — | 1 | 1047 | — |

Warm sample distribution (n=48):

```text
5 ms ×8 | 29 ms ×1 | 31 ms ×11 | 32 ms ×12 | 33 ms ×8 | 34 ms ×8
```

The eight 5 ms samples are the lap-opening restores, where the reset agent has
already vacated VRAM so the sequence is `ResumeLoad` alone — the same structural
artefact G2 saw (7 of its 56), and for the same reason.

## Qualitative comparison to G2 warm — description, not adjudication

**G2 is not re-read here and no verdict is being issued.** These two runs
differ in window, pressure mechanism, code and sample count; the table exists
because the brief asked what an 8× window does to the numbers that were
measured.

| | G2 warm | this run |
|---|---|---|
| window / VRAM per context | 2048 tokens / 112 MiB | **16 384 tokens / 896 MiB KV + 384 MiB reserved** |
| pressure | unmeasured probe, cap of 1 resident | **measured budget, weights + reservation charged** |
| residents at a time | 1 | **3** |
| n | 56 | 48 |
| p50 / p95 | 29 / 32 ms | **32 / 34 ms** |
| `EvictSave` (ram) median | 25 ms | **27 ms** |
| `ResumeLoad` (ram) median | 5 ms | **5 ms** |
| KV image moved (median) | 67 967 556 B | **68 397 726 B** |

Two things stand out, and both are consistent with the mechanism rather than
surprising:

* **The image barely grew** — 64.8 MiB → 65.2 MiB — even though the window is
  8× larger. A KV image holds the tokens actually used, not the window, and
  both runs prime with the same 6000 characters (1132 prompt tokens, identical
  in both). The window is a *reservation* size; the image is a *content* size.
* **The switch got ~2 ms slower at the median**, all of it on the save side
  (`EvictSave` 25 → 27 ms; `ResumeLoad` unchanged at 5 ms). An eviction now
  destroys a 1280 MiB reservation instead of a 112 MiB one, and a restore
  creates one — but the bytes copied are the same, so only the allocator work
  moved. Two milliseconds on an 8× larger context is a description of this
  box's allocator, measured once, not a law.

Nothing here is compared against G2's 2000 ms ceiling, because this is not a
G2 run.

## Page-cache status of this run

**Not controlled, and not on the critical path.** Every one of the 48 samples
is `EvictSave(ram) + ResumeLoad(ram)` or `ResumeLoad(ram)` alone: the image
never left host memory, so no disk read is inside any published number. The
eight `SuspendSave` writes (reset agents, 287 KB each) went to the ext4 NVMe
image tier and landed in the page cache like any other write; none is a sample.
The GGUF blob had been read by other processes before the run — the single
`ModelLoaded` of 1047 ms for a 7723 MiB file is page-cache speed, and it is not
in any warm sample either. G2's page-cache caveat stands untouched, and the cost
of reading a KV image off NVMe **media** remains unmeasured (carried debt 5).

## What this run does and does not establish

**Establishes.** On this box, with a measured `nvidia-smi` budget and no
`PATH` games, the pager plans residency against
`budget − overhead − Σ weights − Σ reserved`, evicts when that arithmetic says
it must, and does so 53 times in 20 seconds without a refusal, a contract
violation or an allocation failure. The Phase 1 configuration that made
switches possible — a disabled VRAM probe — is no longer needed.

**Does not establish.** That 384 MiB is the right per-context reservation
anywhere but here: it is a measured floor for *this* model at *this* window on
*this* Vulkan driver, and a different model, backend or `n_ctx` will have a
different compute buffer. bloomery does not measure that number, it is told it.
That is the honest limit this run leaves behind, and it is now the README's.


