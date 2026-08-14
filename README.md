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

**Status: Phase 1, live-proven on one box.** The daemon boots, loads a real
GGUF through llama.cpp, pages agents in and out of VRAM, and journals every
decision. The pre-registered kill gate for the process model, **G2, passed**:
p95 warm agent switch **32 ms** against a 2000 ms ceiling and p95 cold switch
**862 ms** against 5000 ms, 56 samples per class on real hardware — with a
page-cache caveat that matters and is quantified in the evidence. Those
switches happened because the run supplied residency pressure the *other* way:
the pager's unmeasured-VRAM mode, which caps residency at one resident agent.
With a measured budget the planner would have placed every agent and evicted
none, for reasons that are an accounting gap rather than a tuning choice — see
the evidence doc's [pressure configuration](docs/superpowers/evidence/2026-08-14-g2-agent-switch.md#2-the-pressure-configuration--and-why-it-had-to-be-this)
section and the first bullet under [Honest limits](#honest-limits). Phases 2–4
(capability plane, edit-codec syscall ABI, semantic store, appliance boot) are
not built. See [Honest limits](#honest-limits) before believing anything else.

## What works

* **Computed windows, never configured ones.** `usable = min(training_ctx,
  (free_vram − weights − overhead) / kv_per_token, user_cap)`, and every agent
  is told which term bound it. GGUF geometry is parsed from the file.
* **A real pager.** Deterministic residency planning against a measured VRAM
  budget *before* anything is allocated — or, when the probe reports
  unmeasured, against a documented cap of one resident agent, journaled as a
  degradation; priority eviction with the arithmetic
  printed on refusal; KV images that round-trip through a RAM tier and an NVMe
  spill tier, digest-tagged so a changed model invalidates them into a cold
  start instead of a corrupt restore.
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

Phase 1 limits, all known and none hidden:

* **Model weights are not charged against the reservation budget.** The
  residency planner tracks KV bytes only. It will plan residency for agents
  whose contexts cannot physically fit alongside the weights.
* **Equal-priority agents are never evicted.** Eviction requires a *strictly*
  lower-priority idle resident, so under memory pressure a same-priority peer
  is refused rather than time-shared. bloomery cannot yet round-robin equal
  peers under pressure.
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
without which the warm run produces no samples at all (and which the bench
refuses to proceed without):

```bash
cargo build --release -p bloomery-bench

# Both classes run against their own daemon boot, so each gets its own journal.
# PATH holds no nvidia-smi: the VRAM probe then reports unmeasured and the pager
# caps residency at one resident agent, which is what makes a switch a switch.
mkdir -p /tmp/no-tools
env PATH=/tmp/no-tools target/release/bloomery-daemon --config bloomery.toml

# warm class
target/release/bloomery-bench switch \
  --daemon http://127.0.0.1:8181 --model qwen2.5-coder:7b-instruct-q8_0 \
  --agents 8 --rounds 7 --window 2048 --prime-chars 6000 --max-tokens 8

# cold class — restart the daemon first for a fresh journal
target/release/bloomery-bench switch --cold \
  --daemon http://127.0.0.1:8181 --model qwen2.5-coder:7b-instruct-q8_0 \
  --agents 8 --rounds 7 --window 2048 --prime-chars 6000 --max-tokens 8

target/release/bloomery-bench report --journal <data_dir>/journal/boot-<ts>.jsonl
```

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
* [Phase 0 prior art](docs/priorart/2026-08-14-phase0-priorart.md) — what already exists and what was decided against it
* [Phase 1 plan](docs/superpowers/plans/2026-08-14-phase1-pager-daemon.md)

Sibling repos whose measurements this project is built on:
[assay](https://github.com/bricelancasterwcp-sudo/assay) (capability profiles),
[robigo](https://github.com/bricelancasterwcp-sudo/robigo) (the VRAM-budget
coding agent and its 1.06% null result).
