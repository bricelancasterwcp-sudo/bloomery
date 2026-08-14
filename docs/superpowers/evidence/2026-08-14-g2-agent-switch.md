# G2 — agent switch latency, measured

**Date:** 2026-08-14
**Gate:** `docs/gates.md` G2, pre-registered 2026-08-14 before this instrument existed.
**Verdict: PASS on both classes, with a page-cache caveat that is quantified below and is not decorative.**

| class | commitment | measured (n=56) | margin |
|---|---|---|---|
| warm | p95 ≤ **2000 ms** | **p95 32 ms**, p50 29 ms | 62× |
| cold | p95 ≤ **5000 ms** | **p95 862 ms**, p50 834 ms | 5.8× |

The cold class measures a switch whose weights *and* KV image are out of VRAM
but still in the OS page cache. An auxiliary probe with the page cache dropped
(n=5, **not** a gate reading) measures **p95 5588 ms, p50 5435 ms** — *above*
the 5000 ms ceiling — but it moved the weights side only; the cost of reading a
KV image off NVMe media remains **unmeasured**. See
[Page-cache caveat](#page-cache-caveat).

---

## Box

| | |
|---|---|
| GPU | NVIDIA GeForce RTX 5080, 16303 MiB total, 14689 MiB free at run start |
| Driver | 595.84 (CUDA 13.2 runtime present; the substrate used the **Vulkan** backend) |
| Tier declared | `enthusiast-16gb`, `emulated = false` — **real hardware**, not an emulated tier |
| Host RAM | 30649 MiB total; 19482 MiB used, 14931 MiB buff/cache at run start |
| Filesystems | daemon `data_dir` (journal + NVMe image tier) on `/dev/nvme0n1p2`; GGUF blob on `/dev/nvme1n1p1` |
| CPU | 16 cores |

Both the journal and the NVMe spill tier are on real NVMe, never tmpfs. A
`data_dir` under `/tmp` would have made the cold class's "NVMe" images
RAM-backed and the whole reading meaningless.

## Model

| | |
|---|---|
| Name | `qwen2.5-coder:7b-instruct-q8_0` (the gate's pinned model) |
| Blob | `/mnt/extra/ollama-models/blobs/sha256-24b532e5276503b147d0eea0e47cb1d2bcce7c9034edd657b624261862ca54a1` |
| Size | 8 098 525 696 bytes (7723 MiB) |
| Daemon blob identity | `model_digest` = `a6c11e79e7d40ea7117377a8b62637870cdd6a153b7666c3b219b3728a06a74a` (bloomery's own `sha256(first 1 MiB ‖ file_len)`, not the file's full sha256) |
| Geometry read from the GGUF | `kv_per_token` = 57 344 B (56 KiB/token), `training_ctx` = 32 768 |

The blob path is the `FROM` line of `ollama show qwen2.5-coder:7b-instruct-q8_0
--modelfile`; the file name is ollama's content digest, so the pinned model's
identity is checkable without trusting this document.

## Code

| | |
|---|---|
| Daemon sources | `34a5469` (`fix: post — subprocess timeout, stale profile guard, honest window docs`) — unchanged by the bench commit |
| Instrument | `3479ec2` (`feat: G2 bench instrument`), committed **before** the run |
| Build | `cargo build --release --features llama,vulkan -p bloomery-daemon`; `cargo build --release -p bloomery-bench` |

---

## The lens

A verdict without its instrument is not a property of the subject. Everything
in this section changes what the numbers mean.

### 1. What one sample is

A **switch sample** is the sum of `duration_ms` over the *contiguous* pager-op
sequence serving one resume: `EvictSave` of the victim + `ResumeLoad` of the
target, plus `ModelLoaded` when the weights had to come back. Contiguity is
enforced — any non-paging journal event ends the sequence — so an orphan
`ModelLoaded` (a cold start with no image to restore after it) can never be
carried across an intervening request and charged to the next switch.

**warm** = RAM-tier image and no `ModelLoaded` in the sequence.
**cold** = `image_tier == "nvme"` **or** a `ModelLoaded` in the sequence.

`p95 = sorted[ceil(0.95·n) − 1]`, integer arithmetic. At n=56 that is index 53,
the 3rd largest sample. Pinned by `crates/bloomery-bench/tests/report_test.rs`
(nine tests, every one mutation-checked: each was run against a deliberately
broken implementation and observed to fail).

Every duration was recorded by the pager itself, inside the daemon, around the
operation it names. The bench takes no timings at all and cannot flatter a
number it never measures.

### 2. The pressure configuration — and why it had to be this

Four 2048-token contexts are 112 MiB each; a 16 GB card with a measured budget
holds all of them trivially, so **no eviction would occur naturally** and the
warm class would have zero samples.

The obvious lever — raise `overhead_mib` until the pool is tight — **does not
work, and the arithmetic says why.** Two different subtractions are at play:

* the *window law* (`usable_window`) computes `free_vram − weights − overhead`;
* the *residency planner* (`Pager::place`) computes `free_vram − Σ resident
  kv_bytes` — it subtracts neither the weights nor the overhead.

So forcing an eviction with `w` = 7723 MiB of weights needs
`2·kv > free_vram`, while getting a non-zero window at all needs
`kv ≤ free_vram − w`. Together those require `free_vram > 2w ≈ 15 446 MiB`,
and this card reports 14 689 MiB free. Shrinking `free_vram` instead (by
occupying VRAM before boot) drives `free_vram − w` to zero and the computed
window with it. **There is no configuration of this daemon, on this box, with
this pinned model, in which the planner evicts before the GPU physically
OOMs.** That is the weights-not-in-reservation gap, stated again below.

The one pressure mechanism that exists is the one the pager already ships:
when the VRAM probe reports **unmeasured**, residency is capped at one
resident agent. So both gate daemons were started with a `PATH` containing no
`nvidia-smi`:

```
env PATH=<empty dir> target/release/bloomery-daemon --config bench-warm.toml
```

Nothing is faked — the probe simply cannot find its tool and reports `None`
(unmeasured, never zero), exactly as on a machine without it. The
configuration is self-documenting *inside the artifact*: both journals open
with

```json
{"event":"Degraded","reason":"vram unmeasured; residency capped at 1 agent"}
```

and `/status` reported `free_vram_bytes: null` in the bench's own preflight
line. The bench **refuses** to start a warm run against a daemon reporting a
measured budget, because such a run would finish clean and report `n: 0`.

This changes the *decision*, not the *mechanics*: every switch below saved and
restored a real 2048-token-window KV image on the real GPU with the real
weights loaded. `overhead_mib` was set to `0` and had no effect either way —
with an unmeasured probe the window law skips its VRAM term entirely.

### 3. The switch protocol

The residency planner evicts only **strictly lower-priority** idle residents,
so a round-robin over same-priority agents does not switch — it is refused
with `409 {"error":"refused","free":0,"needed":117440512,"reclaimable":0}`
(observed during pilot). Workers therefore carry ascending priorities
(10, 20, … 80) and are always visited in that order.

A RAM-tier image can only be produced by an *eviction*: `Pager::suspend`
always spills to NVMe. So a lap cannot wrap around, and each warm lap opens
with a single-use **reset agent** (priority 250, freshly created) that evicts
the incumbent worker — parking that worker's image in RAM — and then suspends
itself to hand the VRAM back. Being fresh, it has no image to restore and
contributes no sample.

Each worker was primed with a 6000-character prompt (**1132 prompt tokens**,
measured, reported identically by all 8 workers) so that a switch moves a KV
image worth moving: **67 967 556 bytes ≈ 64.8 MiB median**, not a handful of
cells. Priming can only make the numbers worse, never better.

### 4. What each class does and does not include

* **warm** — the sequence is `EvictSave(victim, ram) + ResumeLoad(target,
  ram)`. 49 of the 56 samples are exactly that (29–33 ms). The remaining 7 are
  the first switch of each lap, where the reset agent has already vacated VRAM,
  so the sequence is `ResumeLoad` alone (5 ms). Those 7 deflate p50; p95 is
  read off the tail, which is entirely the full evict+restore population.
* **cold** — the sequence is `ModelLoaded + ResumeLoad(target, nvme)`. The
  outgoing agent's page-out happened inside the operator's `unload` request, a
  separate operation, so it is **not** in the sample. Measured separately in
  the same journal: `SuspendSave` (nvme, ~65 MiB) ran 32–86 ms, median 50 ms.
  Adding it back would put cold p95 near 950 ms — still 5× inside the gate.

**Scope, twice over.** (a) Residency was capped at one resident agent, so every
switch here evicted exactly one victim. **Multi-victim eviction sequences — a
placement whose `SchedulerDecision` names several agents at once — are
unexercised by this run**, and nothing here says what one costs. (b) Both
classes are *repeated measures*, not a survey: 56 = 8 agents × 7 laps, one
model, one 2048-token window, one ~64.8 MiB image size, one box. It is 56
repetitions of a single operation, which is what makes the tight spread (warm
29–33 ms, cold 816–888 ms) unsurprising rather than reassuring. It is not 56
scenarios, and it says nothing about other models, window sizes, image sizes or
machines.

### 5. Resolution

The journal records whole milliseconds. Warm samples of 5 ms and 29–33 ms sit
comfortably above that floor, but individual `ResumeLoad` ops measured 5 ms and
individual `EvictSave` ops 23–28 ms, so warm numbers are quantised at roughly
±1 ms per op. Nothing here is reported at the resolution floor.

---

## The run

Pre-registered before execution, executed once, no protocol change afterwards.
Full driver: `.superpowers/sdd/2026-08-14-phase1-pager-daemon/g2run/run-gate.sh`
(scratch, not committed — it names machine-local paths). The two invocations
verbatim:

```bash
# both daemons: env PATH=<empty dir> target/release/bloomery-daemon --config bench-<class>.toml
# warm class
target/release/bloomery-bench switch \
  --daemon http://127.0.0.1:8181 --model qwen2.5-coder:7b-instruct-q8_0 \
  --agents 8 --rounds 7 --window 2048 --prime-chars 6000 --max-tokens 8

# cold class
target/release/bloomery-bench switch --cold \
  --daemon http://127.0.0.1:8181 --model qwen2.5-coder:7b-instruct-q8_0 \
  --agents 8 --rounds 7 --window 2048 --prime-chars 6000 --max-tokens 8

# reports
target/release/bloomery-bench report --journal <data_dir>/journal/boot-<ts>.jsonl
```

Config common to both (`tier = enthusiast-16gb`, `emulated = false`,
`assay.enabled = false`, `allow_unprofiled = true`, `overhead_mib = 0`). POST
was off: it costs ~110 s per model per boot and measures the capability
ceiling, which G2 does not read; its absence is journaled as a degraded boot,
and `allow_unprofiled` is what admits an unprofiled model in its place — both
`Degraded` lines are in the journals.

One daemon boot per class, so neither journal can contaminate the other. The
GGUF page cache was warmed (`cat blob > /dev/null`) before the run to pin that
ambient variable; it was already resident from the pilot, and `buff/cache` did
not move.

### Reports, verbatim

`docs/superpowers/evidence/2026-08-14-g2-warm-journal.jsonl` (359 events):

```json
{
  "cold": {
    "n": 0,
    "p50_ms": null,
    "p95_ms": null
  },
  "warm": {
    "n": 56,
    "p50_ms": 29,
    "p95_ms": 32
  }
}
```

`docs/superpowers/evidence/2026-08-14-g2-cold-journal.jsonl`:

```json
{
  "cold": {
    "n": 56,
    "p50_ms": 834,
    "p95_ms": 862
  },
  "warm": {
    "n": 0,
    "p50_ms": null,
    "p95_ms": null
  }
}
```

Both journals are committed next to this document. `bloomery-bench report
--journal <file>` reproduces both blocks exactly; that is the intended way to
check this page. `n: 0` on the class a run did not exercise carries `null`
percentiles, never `0` — an unmeasured class must not read as an instantaneous
switch.

### Sample distributions

```
warm (n=56):  5 ms ×7 | 29 ms ×21 | 30 ms ×19 | 31 ms ×6 | 32 ms ×2 | 33 ms ×1
cold (n=56):  816…888 ms, median 834 — ModelLoaded 811–884 plus ResumeLoad 5–6
```

Component ops, from the same journals:

| op | class | n | ms min/median/max | bytes (median) |
|---|---|---|---|---|
| `EvictSave` (ram) | warm | 63 | 23 / 25 / 28 | 67 967 556 |
| `ResumeLoad` (ram) | warm | 56 | 5 / 5 / 5 | 67 967 556 |
| `SuspendSave` (nvme) | warm | 7 | 12 / 13 / 15 | 287 476 (reset agents) |
| `ModelLoaded` | cold | 57 | 811 / 830 / 884 | — |
| `ResumeLoad` (nvme) | cold | 56 | 5 / 5 / 6 | 67 967 556 |
| `SuspendSave` (nvme) | cold | 63 | 32 / 50 / 86 | 67 967 556 |

No `Refusal` and no `ContractViolation` appears in either journal. Everything
the protocol asked for was served: **135 inferences** (71 in the warm journal —
8 primes, 56 lap steps, 7 reset agents; 64 in the cold journal — 8 primes and
56 lap steps), **63 `EvictSave`** (all of them in the warm journal) and **70
`SuspendSave`** (7 warm reset-agent page-outs, 63 cold page-outs inside
`unload`).

---

## Page-cache caveat

**The cold class above measures a switch in which *both* the weights and the KV
image were out of VRAM but still in the OS page cache.** Both halves show it:
`ModelLoaded` ran 811–884 ms for a 7723 MiB blob (~9 GB/s) and `ResumeLoad`
read a 64.8 MiB image off "NVMe" in 5 ms (~13 GB/s). Those are host memory
bandwidth, not NVMe media. Nothing in the pinned protocol controls the page
cache; the gate anticipated exactly this and required the caveat be stated. It
is stated here with numbers rather than a shrug — and, below, with an explicit
note of which half is still unmeasured.

**Auxiliary probe (n=5, not a gate reading).** Same daemon, same agent, same
65 MiB image; before each switch the model was unloaded and
`POSIX_FADV_DONTNEED` was applied to both the GGUF blob and the spilled KV
image (`sync` first — a dirty page cannot be dropped). Journal committed as
`2026-08-14-g2-coldcache-probe-journal.jsonl`; driver
`g2run/probe_coldcache.py`.

```json
{
  "cold": {
    "n": 5,
    "p50_ms": 5435,
    "p95_ms": 5588
  },
  "warm": {
    "n": 0,
    "p50_ms": null,
    "p95_ms": null
  }
}
```

`ModelLoaded` in that regime: 5416, 5426, 5430, 5431, 5583 ms — a 6.5×
increase, and **above the gate's 5000 ms cold ceiling.** A third datapoint from
the pilot run, the first load of the session on a genuinely cold cache
(including first-touch Vulkan warm-up), measured **11 317 ms**.

**The probe moved the weights side only.** `ResumeLoad` in all five iterations
measured **5 ms** — identical to the page-cache-warm cold run — so the
`POSIX_FADV_DONTNEED` applied to the spilled `.kvimg` produced no measurable
change and the image was still being served from memory. Whatever the cause
(the pages were re-faulted before the read, or the advice was declined), the
consequence is what matters: **the cost of reading a KV image off NVMe media
is UNMEASURED by this run.** It is not "5 ms" and it is not "the same as
warm" — it is a number nobody here has taken. This probe quantifies the
weights-side page-cache penalty and nothing else, and any later claim about
NVMe-tier restore cost needs its own instrument.

So, plainly: a bloomery daemon that has been switching this model recently
serves a cold switch in ~0.86 s and passes G2. The *first* cold switch after a
boot, or after the file has aged out of a busy machine's page cache, costs
5.4–11.3 s and would not. n=5 is an observation, not a claim; what it is
enough for is to say the pinned reading depends on an ambient variable the
gate does not control, and to size that dependence.

---

## Accounting gaps this run exposes

Recorded because they bound what the numbers mean, not as future work:

1. **Model weights are not charged against the reservation budget.** The
   planner tracks KV bytes only. It will therefore plan residency for agents
   whose contexts cannot physically fit alongside the weights — on this box, at
   full 32 768-token windows, it would happily admit seven residents needing
   ~13 GB of KV on top of 7.7 GB of weights on a 16 GB card. G2 never reached
   that state (residency was capped at one), so this run neither confirms nor
   denies what happens when it does. It is the reason the pressure
   configuration above had to exist at all.
2. **Equal-priority agents are never evicted.** `plan_residency` requires a
   *strictly* lower priority, so under memory pressure a second agent at the
   same priority is refused (`409 refused`, `reclaimable: 0`) rather than
   time-shared. Bloomery as built cannot round-robin peers of equal priority
   under pressure. Found by building this bench; recorded, not fixed here.
3. **The VRAM probe is nvidia-smi-only**, so any non-NVIDIA box lands in the
   unmeasured/cap-at-one mode by default — which is what this run deliberately
   used.

## Corrections

**Shape of the run.** The task brief's example invocation was `--agents 4
--rounds 30`; this run used `--agents 8 --rounds 7`. Same total (56 ≥ 50
samples per class), different split, and the reason is a property of the
protocol rather than of the numbers: one sample per lap — the lap-opening
resume, which finds VRAM already vacated by the reset agent — contains no
`EvictSave` and measures the restore half only. That fraction is exactly
`1/agents`: 12.5% here (7 of 56), against 25% at `--agents 4` whatever the lap
count. The change was made while designing the protocol, before the gate run,
and moves the reading *toward* the more expensive population.

The task brief's own pinned test asserted `p95 = 129` for the sample
110…129 while its inline comment said "index 18", which is 128. The *formula*
(`sorted[ceil(0.95·n) − 1]`) is the pre-registered commitment and appears three
times; the literal appears once and contradicts its own comment, so it was the
transcription error. `report_test.rs` pins **128**, and the correction is
recorded here rather than made silently.

## Consequence

G2's kill criterion — "the process model is redesigned before anything is built
on it" — is **not** triggered. Warm switches at 32 ms p95 and page-cache-warm
cold switches at 862 ms p95 leave the process model standing. The cold-cache
result is not a gate failure under the pinned protocol, but it is the honest
answer to "what does a cold switch cost on a machine that has not touched this
model lately", and anything that later depends on cold-switch latency should
read 5.4 s, not 0.86 s.
