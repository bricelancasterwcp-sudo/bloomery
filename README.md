# bloomery

An operating layer for local LLMs that treats the model endpoint as
**unreliable hardware** — something to be probed, budgeted, and
admission-controlled before it is trusted with work. The thesis comes from
measurements in three sibling repos, not from taste: serving layers silently
front-truncate oversized prompts and answer confidently about whatever
survived; a daemon can return HTTP 200s that break its own protocol, and that
failure can be gone two days later on the same unrestarted process; "context
length" is geometry, not configuration — KV cost per token spans **14×** across
real 7–8B blobs, so a 32k window costs 1.75 GB on one model and 25 GB on
another. Every one of those failures lives in the serving daemon and the agent
harness. None live in the kernel, the drivers, ext4, or TCP.

So bloomery rewrites the layers that were measured lying and rents the ones
that never did: a Rust system that owns the whole path from GPU to agent —
serving substrate, pager, scheduler, budgets, contracts, journal — on top of a
Linux kernel treated as firmware. It is **not** a wrapper over an existing
serving daemon, and it is not an orchestration product. Those assume the
endpoint is honest and capable. bloomery assumes neither and measures both.
The full design, its nine laws and the failure each one was bought with are in
[docs/superpowers/specs/2026-08-14-bloomery-design.md](docs/superpowers/specs/2026-08-14-bloomery-design.md).

**Status: Phase 1 live-proven on one box; Phase 2a hardening landed on top.**
The daemon boots, loads a real
GGUF through llama.cpp, pages agents in and out of VRAM, and journals every
decision. The pre-registered kill gate for the process model, **G2, passed**:
p95 warm agent switch **32 ms** against a 2000 ms ceiling and p95 cold switch
**862 ms** against 5000 ms, 56 samples per class on real hardware — with a
page-cache caveat that matters and is quantified in the evidence. Those
switches happened because the run supplied residency pressure the *other* way:
the pager's unmeasured-VRAM mode, which caps residency at one resident agent.
With a measured budget the Phase 1 planner would have placed every agent and
evicted none — an accounting gap rather than a tuning choice, and the first
thing **Phase 2a** closed. Weights now spend from the reservation budget
alongside KV, and the pager has been driven to evict under a natural measured
budget on this box; that run is
[recorded separately](docs/superpowers/evidence/2026-08-14-2a-natural-pressure.md)
and is **not** a re-read of G2, which stands exactly as published. Phases 2b–4
(edit-codec syscall ABI, capability grants, policy plane, semantic store,
appliance boot) are not built. See [Honest limits](#honest-limits) before
believing anything else.

## What works

* **Computed windows, never configured ones.** `usable = min(training_ctx,
  (free_vram − weights − overhead) / kv_per_token, user_cap)`, and every agent
  is told which term bound it. GGUF geometry is parsed from the file.
* **A real pager.** Deterministic residency planning against a measured VRAM
  budget *before* anything is allocated — `avail = budget − overhead −
  Σ loaded weights − Σ resident reservations`, where a context's reservation is
  its KV cache *plus* the runtime buffers llama.cpp allocates beside it, so
  everything that occupies VRAM spends from one pool — or, when the
  probe reports unmeasured, against a documented cap of one resident agent,
  journaled as a degradation; priority eviction with the arithmetic
  printed on refusal; KV images that round-trip through a RAM tier and an NVMe
  spill tier, tagged with a full-file model digest so a changed model
  invalidates them into a cold start instead of a corrupt restore.
* **Equal-priority peers time-share instead of starving.** Eviction still
  requires a *strictly* lower-priority idle resident — the planner is
  unchanged and deterministic — but a refusal between equal-priority peers
  that has waited out `time_share_quantum_secs` (default 30 s) is retried as
  an eviction of the least-recently-used peer, journaled as
  `evict_timeshare(waited_Nms)`.
* **A substrate that cannot lie about token counts.** llama.cpp via
  `llama-cpp-2`, per-sequence `state_seq_*_ext` save/restore, prompt and
  completion counts derived from vectors this process built. A reply without
  stats is a `ContractViolation`, charged to nobody's budget.
* **Refusals, never truncation.** Oversized prompts come back as `413` with
  the token arithmetic; unplaceable agents as `409` with bytes needed, free and
  reclaimable; exhausted budgets as `402`.
* **Boot-time POST.** The daemon probes *itself* with [assay](https://github.com/bricelancasterwcp-sudo/assay)
  and attaches the resulting capability profile, so admission can be gated on a
  measured verdict rather than a promise.
* **Two HTTP surfaces.** A native API (`/agents`, `/agents/{id}/infer`,
  `/suspend`, `/resume`, `/models/{m}/unload`, `/status`) and an
  OpenAI-compatible shim (`GET /v1/models`, `POST /v1/chat/completions`).
* **A journal you can replay.** Every boot writes `boot-<ts>.jsonl`; every
  admission, decision, paging op, refusal and degradation is a line in it. The
  G2 numbers above are computed from nothing else — see
  [the evidence](docs/superpowers/evidence/2026-08-14-g2-agent-switch.md), whose
  journals are committed beside it.

## Honest limits

Limits after Phase 2a, all known and none hidden:

* **The VRAM budget is a static boot-time read.** The pager subtracts its own
  weights and contexts from that number (reservation accounting) and never
  re-reads the driver, so VRAM another process allocates after boot is
  invisible to it. Deliberate — a live read already excludes what the pager
  allocated, and subtracting residents from it would double-count.
* **Weights are charged but never evicted.** They spend from the same budget
  KV does, so residency planning is honest about them; nothing, however,
  unloads a model automatically to make room. `POST /models/{m}/unload` is the
  only thing that credits weights back.
* **The per-context runtime reservation is configured, not measured.** A
  llama.cpp context costs more than its KV cache — on this box, at
  `n_ctx = 16384`, a 896 MiB KV cache came with a 304 MiB Vulkan compute buffer
  and a 30 MiB host buffer. The **default** `ctx_overhead_mib = 384` is derived
  from that measured floor ([excerpt committed](docs/superpowers/evidence/2026-08-14-2a-daemon-log-excerpt.txt));
  the **active value is configured, not measured per run**, and bloomery never
  reads it back from the substrate, so a different model, backend or window can
  need a different number. Setting it too low is how the
  [2a natural-pressure run](docs/superpowers/evidence/2026-08-14-2a-natural-pressure.md#attempt-1--aborted-oom-and-the-accounting-gap-it-found)
  OOM'd a GPU the planner believed had room.
* **A VRAM-bound window is un-placeable by exactly that reservation.** The
  window law subtracts `weights` and `overhead_mib` from free VRAM, but not
  `ctx_overhead_mib`; placement charges it. So an agent whose window is bound
  by VRAM (rather than by `window_cap`, the training context, or a measured
  ceiling) is sized to fill the budget and then reserves more than it — a
  permanent, safe refusal with nothing allocated, and no recovery but a smaller
  window or a smaller reservation. Fixing it properly means changing the window
  law itself; it is [carried debt](docs/CARRIED-DEBT.md), not a surprise.
* **The VRAM probe is `nvidia-smi`-only.** Anywhere else it reports
  *unmeasured* (`None`, never zero) and residency falls back to a cap of one
  resident agent, journaled as a degradation.
* **One coarse lock.** Four HTTP workers share one `Mutex<Pager>`, so
  inference is serialized daemon-wide. That is deliberate for one GPU and will
  not survive two.
* **`/v1` streaming is buffered.** `stream: true` returns real SSE framing, but
  the whole completion is generated first and then written — the shape is
  compatible, the latency benefit is not there.
* **POST costs ~110 s per model per boot**, sequentially, and the daemon
  provisionally admits unprofiled models for that whole window (journaled).
  Set `assay.enabled = false` to skip it, and `allow_unprofiled = true` if you
  accept serving a model whose ceiling and codecs nobody measured.
* **KV images are boot-scoped.** The image store's index lives in memory, so
  spilled images from a previous boot are unreachable litter. A restart is a
  cold start for every agent.
* **Localhost only, no auth, no read timeout.** It binds `127.0.0.1` and
  nothing else, and a client that opens a socket and stops talking stalls the
  worker that accepted it. Both are fine on loopback and unacceptable off it.
* **`vulkan` was the offload path used for the G2 run**; the CUDA path is not
  exercised by any committed evidence.

### Action codec (Phase 2b P1)

* **Five verbs, one per turn.** `read`, `find`, `patch`, `run`, `done` decode
  from a single `<action verb="...">...</action>` block — envelope-constrained,
  never grammar-forced; zero or multiple blocks come back as typed
  `ActionError`s instead of being silently coerced.
* **Two patch codecs, one landing lens.** `SearchReplace` (conflict-marker,
  unique-match-required) or `WholeFile`, selected per call; the landing lens
  checks whether a patch both applies and parses — `PlainText` ships now,
  language-specific lenses are P3's job.
* **Codec only — nothing live yet.** No daemon wiring, no executors, no
  capability grants. P3 wires executors and capability grants into the task
  loop; P4 gates per-model codec landing against G4 (≥80% applies-and-parses,
  else demotion).

## Quick start

Needs a Rust toolchain, a llama.cpp-capable GPU stack, and a `.gguf` on disk.
Everything except the `llama` feature builds and tests GPU-free.

```toml
# bloomery.toml
port = 8181
data_dir = "/var/lib/bloomery"     # journal/, images/, profiles/ — put it on NVMe
overhead_mib = 1024                # VRAM held back for allocator and compute buffers
default_priority = 100
default_budget_tokens = 200000
allow_unprofiled = false           # true = serve models nobody measured, journaled
time_share_quantum_secs = 30       # how long an equal-priority refusal waits
                                   # before the LRU peer is evicted anyway
ctx_overhead_mib = 384             # VRAM each resident context reserves beyond
                                   # its KV cache (llama.cpp compute buffers)

[models]
"qwen2.5-coder:7b-instruct-q8_0" = "/path/to/model.gguf"

[tier]
name = "enthusiast-16gb"
emulated = false                   # false = these numbers came off real hardware

[assay]
enabled = true                     # boot-time capability probe (~110 s per model)
python = "python3"
```

```bash
cargo build --release --features llama,vulkan -p bloomery-daemon
target/release/bloomery-daemon --config bloomery.toml

curl -s localhost:8181/status
curl -s localhost:8181/agents -d '{"model":"qwen2.5-coder:7b-instruct-q8_0","window_cap":2048}'
curl -s localhost:8181/agents/a1/infer -d '{"prompt":"hello","max_tokens":32}'
```

Reproducing the gate — the invocations verbatim, including the pressure setup
without which the warm run produces no samples at all:

```bash
cargo build --release -p bloomery-bench

# Both classes run against their own daemon boot, so each gets its own journal.
# PATH holds no nvidia-smi: the VRAM probe then reports unmeasured and the pager
# caps residency at one resident agent, which is what makes a switch a switch.
mkdir -p /tmp/no-tools
env PATH=/tmp/no-tools target/release/bloomery-daemon --config bloomery.toml

J=<data_dir>/journal/boot-<ts>.jsonl   # the boot this run is driving

# warm class
target/release/bloomery-bench switch --journal "$J" \
  --daemon http://127.0.0.1:8181 --model qwen2.5-coder:7b-instruct-q8_0 \
  --agents 8 --rounds 7 --window 2048 --prime-chars 6000 --max-tokens 8

# cold class — restart the daemon first for a fresh journal
target/release/bloomery-bench switch --cold --journal "$J" \
  --daemon http://127.0.0.1:8181 --model qwen2.5-coder:7b-instruct-q8_0 \
  --agents 8 --rounds 7 --window 2048 --prime-chars 6000 --max-tokens 8

target/release/bloomery-bench report --journal "$J"
```

`--journal` is not optional. The driver reads that file before and after its
laps, predicts from the daemon's own `/status` numbers how many restores the
workload must force, and fails the run if it did not deliver them — a bench
that switched nothing exits zero otherwise, and `report` will print its `n: 0`
without comment. The Phase 1 driver instead refused up front to run the warm
class against a *measured* VRAM budget, because the planner then charged KV
bytes only and would have evicted nobody; since Phase 2a charges the weights,
that refusal is gone and a measured budget is the natural way to run this — see
[the natural-pressure evidence](docs/superpowers/evidence/2026-08-14-2a-natural-pressure.md),
which is a 2a acceptance run and **not** a re-read of G2.

`report` is pure: point it at either committed journal in
`docs/superpowers/evidence/` and it recomputes the published numbers with no
daemon and no GPU.

## Layout

| crate | what it owns |
|---|---|
| `bloomery-core` | GGUF parsing, window law, residency planner, budgets, profiles, journal |
| `bloomery-substrate` | the `Substrate` trait, a fake for tests, and the llama.cpp backend |
| `bloomery-daemon` | pager, agent table, KV image store, config, HTTP surfaces, boot POST |
| `bloomery-bench` | the G2 instrument: switch driver + pure report |

```bash
cargo test --workspace       # GPU-free, no llama.cpp toolchain needed
cargo clippy --workspace --all-targets -- -D warnings
```

## Documents

* [Design spec](docs/superpowers/specs/2026-08-14-bloomery-design.md) — mission, laws, architecture, phases
* [Kill gates G1–G4](docs/gates.md) — pre-registered, frozen before any instrument existed
* [G2 evidence](docs/superpowers/evidence/2026-08-14-g2-agent-switch.md) — the switch-latency run, its lens, and its caveats
* [2a natural-pressure evidence](docs/superpowers/evidence/2026-08-14-2a-natural-pressure.md) — eviction under a measured budget with the weights charged; an acceptance run, not a gate re-read
* [Carried debt](docs/CARRIED-DEBT.md) — what each slice settled, what it deferred, and what has since been delivered
* [Phase 0 prior art](docs/priorart/2026-08-14-phase0-priorart.md) — what already exists and what was decided against it
* [Phase 1 plan](docs/superpowers/plans/2026-08-14-phase1-pager-daemon.md)

Sibling repos whose measurements this project is built on:
[assay](https://github.com/bricelancasterwcp-sudo/assay) (capability profiles),
[robigo](https://github.com/bricelancasterwcp-sudo/robigo) (the VRAM-budget
coding agent and its 1.06% null result).
