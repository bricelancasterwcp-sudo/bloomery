# bloomery — an AI-native operating system for consumer hardware

**Date:** 2026-08-14
**Status:** Design approved in conversation; awaiting written-spec review.
**Working name:** bloomery (the first furnace that smelted iron; same shelf as
assay, black-oxide, robigo). Rename is cheap until Phase 1 ships.

## 1. Mission and thesis

Make local LLMs and agents genuinely usable on consumer hardware
(mid-gamer and office-worker tiers) by building the operating layer they
actually need: one that treats the model endpoint as unreliable hardware
which must be **probed, budgeted, and admission-controlled** before it is
trusted with work.

The thesis is grounded in measured findings from three sibling repos
(all same author, all reproducible from committed artifacts):

- Serving layers silently front-truncate oversized prompts and answer
  confidently about whatever survived (robigo; assay canary probe).
- A daemon can return HTTP 200s that break their own protocol — no token
  stats — and the failure is *state-transient*: present 40/40 on
  2026-08-10, gone on 2026-08-12 on the same unrestarted process
  (assay live validation). Capability is a point-in-time property of a
  serving state, so it must be probed, never assumed.
- "Context length" is geometry, not configuration: KV cost per token
  spans **14×** across real 7–8B blobs (56 → 800 KiB/token). A 32k
  window costs 1.75 GB on one model and 25 GB on another (robigo).
- Edit landing is instrument-defined: the same model measured 0% and
  100% under two lenses on the same daemon (robigo stage 2 vs assay v1).
  A landing rate without its lens is not a model property.
- Grammar-constrained decoding deforms rather than rejects
  (`mut acc` → `mutacc`; `=` for `==` in statement position) — 44% of an
  entire error class was the instrument (black-oxide SPEC §54, §56).
- Small local models cannot do general autonomous agency: **1.06%**
  strict repair (940 attempts, pre-registered gate, robigo 2026-08-12).
  They *can* repair single defects under a fail-closed verifier with a
  specific diagnostic (black-oxide ownership probe), and they hold
  measured narrow capabilities (assay verdicts).

**Design consequence:** every failure above lives in the serving daemon
and the agent harness. None live in the Linux kernel, drivers, ext4, or
TCP. "From the ground up" therefore means: **rewrite every layer that
was measured lying; rent the layers that never lied.**

## 2. What bloomery is and is not

**Is:** a bare-metal-feeling appliance OS — Linux kernel vendored and
treated as firmware (drivers only, no conventional userland), with a
Rust system as PID 1 that owns the entire path from GPU to agent:
serving substrate, pager, scheduler, budgets, contracts, capabilities,
journal, storage, and the model-facing syscall ABI.

**Is not:**
- A userspace framework or shell wrapper over an existing serving
  daemon (the AIOS/MemGPT shape). bloomery owns the serving path.
- A bare-metal kernel in v1. Without the Linux DRM/driver stack there
  is no CUDA/Vulkan, and CPU-only 7B inference (~5–15 tok/s) violates
  the mission's "usable speed." Bare metal is the **Phase 4 endgame
  flag**, revisited only if a driver path stops being a decade-class
  project.
- An orchestration/identity product (the 2026 "Agent OS" category —
  AIOS scheduling, OpenClaw personal agents, Windows agent workspaces).
  Those all assume the endpoint is honest and capable; bloomery's
  differentiation is that it assumes neither and measures both.

## 3. Design laws

Each law was bought with a measured failure; the source is named.

1. **The window is computed, never read.**
   `usable = min(training_ctx, (free_vram − weights − overhead) / kv_per_token, user_cap)`,
   always reporting which term bound it. (robigo)
2. **Never send a prompt that does not fit.** Refuse with the arithmetic
   printed; truncation is dishonest. Degradation follows a fixed,
   recorded ladder chosen by rendering-and-measuring, not estimation —
   token estimates are non-additive and under-count precisely at the
   edge. (robigo; the 1097-of-1100 run)
3. **Constrain the envelope, never the payload. Never grammar-force.**
   Structure is enforced around model output by a deterministic
   validator that rejects and re-asks; a constrained decoder steers to
   the nearest valid string and deforms the measurement and the output.
   (robigo design rule; black-oxide §54/§56)
4. **Contract enforcement at the model boundary.** Every inference reply
   is validated: token stats present, canary survival on large prompts,
   envelope parses. `ContractViolation` is a first-class error, and
   infrastructure failure is never recorded as model failure.
   (assay; robigo; already applied in Castle VTT `classify.py`)
5. **Admission by measured verdict.** No model gets a role — workload or
   policy — without a tier-marked capability profile supporting it.
   Verdicts name their lens. Unmeasured ≠ failed: `None` with a named
   reason, never a zero. (assay: verdicts, lenses, None-vs-zero)
6. **No inference without a budget.** Budgets are explicit, charged per
   call, accounted spent-vs-granted per agent, and exhaustion is a
   scheduler-visible signal. No silent defaults. (assay `Budget`)
7. **Say what happened.** Every call, state transition, and scheduling
   decision is journaled and replayable. A run that degraded and one
   that did not leave different records. (robigo run records; the
   8,009-call replayable gate)
8. **Mechanism is deterministic; the LLM is policy, fail-closed.**
   A deterministic heuristic always exists underneath any LLM policy
   decision and takes over on contract violation, timeout, or budget
   exhaustion. (robigo gate: 1.06%, verbatim-repeat 29.8%, false `done`)
9. **Pre-register the gates.** Every research-shaped question in this
   project carries a kill criterion written before the instrument that
   measures it, per the house `rigorous-experiments` method. The 1.06%
   null shipping same-day is the standing proof of value.

## 4. Architecture

```
agents (VTT AI, coding agent, importers, …)      untrusted, budgeted, capability-scoped
──────────────────────────────────────────────
policy plane: tiny LLM policy server(s)          advisory, fail-closed, verdict-admitted
──────────────────────────────────────────────
syscall ABI: edit-codec state transitions        per-model codec, envelope-constrained
──────────────────────────────────────────────
store: content-addressed truth + semantic view   view gated by G3
──────────────────────────────────────────────
deterministic core (Rust): pager · scheduler ·
budgets · contracts · capabilities · journal     the kernel
──────────────────────────────────────────────
serving substrate: Rust-owned daemon             honest by construction; /v1 shim
(wrap llama.cpp Vulkan kernels first —
 own the daemon, rent the kernels)
──────────────────────────────────────────────
Linux kernel as firmware                         drivers only, vendored, no userland
```

### 4.1 Serving substrate

A Rust-owned inference daemon. v1 wraps llama.cpp's Vulkan path over
FFI (the same kernels the black-oxide amp runs used) — **own the daemon,
rent the kernels**; replacing kernels with pure Rust (candle-class) is
optional later purity, not a requirement. Properties that are design
requirements, not aspirations, because the daemon is ours:

- Real token counts on every reply; a reply without stats cannot be
  constructed.
- `num_ctx`/window honesty: the daemon serves the computed window and
  refuses oversized prompts with arithmetic instead of truncating.
- Exposes true geometry (KV per token, weights bytes, free VRAM) as an
  API, so law 1 reads from the source instead of reverse-engineering.
- **`/v1` OpenAI-compatible shim** for adoption ("the daemon that
  doesn't lie"): drop-in for existing tooling, with honest refusals
  surfaced as structured errors rather than silent degradation.

### 4.2 Deterministic core (the kernel)

**Pager.** Treats model weights and KV caches as pageable objects across
VRAM → RAM → NVMe, priority-driven. The unit is the **KV image**: an
agent's KV cache is its core image, so suspend / resume / snapshot /
migrate are first-class operations. Feasibility arithmetic (enthusiast
tier, PCIe 4.0 x16 ≈ 25 GB/s practical, NVMe Gen4 ≈ 7 GB/s):
7B-Q8 weights ≈ 8 GB ≈ 320 ms from RAM, ≈ 1.2 s from NVMe; an 8k-token
KV image at qwen geometry (56 KiB/tok) ≈ 450 MB ≈ 20 ms from RAM.
Agent switching is therefore a seconds-granularity operation — the
design point is 1980s process switching, and the scheduler quantum is
set accordingly. Mechanics:

- KV images tagged with model blob digest + quant + RoPE config;
  invalidated on mismatch (the blob-identity pattern).
- Same-model agents share a system-prompt prefix cache (radix-style);
  on 8–16 GB tiers prefix reuse is real VRAM recovered.
- Eviction policy is deterministic by default; the policy plane may
  advise (see 4.6) but never bypasses mechanism.

**Scheduler.** Deterministic mechanism: priorities, deadlines, budgets,
quanta at seconds granularity; preemption = KV image eviction. Policy
input from the policy plane is advisory and fail-closed (law 8).

**Contracts.** Law 4 enforced at the boundary between core and serving
substrate — even though we own the substrate. A canary rides every
large prompt; missing canary or missing stats is `ContractViolation`
with the serving state snapshotted for the journal.

**Budgets.** Law 6. Per-agent accounts for tokens and GPU-seconds;
admission-time reservation for scheduled work; exhaustion signals.

**Capabilities.** Model output is untrusted input, which makes prompt
injection a syscall-level threat class. Agents hold explicit capability
handles (seL4-flavored); the codec applier checks every state
transition against the emitting agent's grants; there is no ambient
authority. Worst-case successful injection spends the compromised
agent's own budget inside its own grants. Agents are isolated from each
other's state, images, and journals by default.

**Journal.** Law 7. Append-only, replayable records of every inference
call (exact prompt, reply, stats), every state transition, and every
scheduling decision, with rung/window/outcome named. The journal is
also the corpus source for the fine-tune flywheel (§7).

### 4.3 Memory hierarchy

| Level | Role | Managed by |
|---|---|---|
| Context window | Register file / working set — tiny, expensive (56–800 KiB/token), quality degrades before protocol does | Degradation ladder (spill policy) |
| KV images (VRAM/RAM/NVMe) | Suspended process cores | Pager |
| Content-addressed store | Long-term memory | Store + semantic view |

The context window is explicitly **not** treated as primary RAM: it is
the scarcest and least reliable memory in the system, spilled
deliberately, with every spill recorded.

### 4.4 Store and semantic view

Ground truth is content-addressed and hierarchical — names that do not
drift, so locks, atomicity, and references keep stable referents. The
semantic/vector-graph index is a first-class **view**, not the
namespace: resolution returns ranked candidates with scores, never a
pretend-deterministic `open()`. Embedding-model upgrades reindex a
view instead of renaming the world. The view earns syscall status only
by passing gate G3; until then it is an app-level index.

### 4.5 Syscall ABI: the edit-codec

Agents and models mutate system state only through structured
state-transition verbs (the robigo action-surface shape: read / find /
patch / run / done, generalized), applied by a deterministic Rust
applier. Properties:

- **Text, not binary.** Models emit text tokens; format discipline is a
  measured per-model capability (granite 0% vs qwen ≥90% on identical
  prompts), so the kernel selects each model's codec from its assay
  profile at admission time.
- **Envelope-constrained, validate-and-reask** (law 3). Never
  grammar-forced.
- **Diagnostics designed for the repair loop.** Every rejection carries
  a specific, typed diagnostic naming both ends of the defect —
  black-oxide's measured lesson that repair (+10pp from semantics,
  more from ergonomics) is where small models gain, and that
  diagnostic ergonomics dominate language elegance.
- Transitions are checked against the emitting agent's capability
  grants before application (§4.2).

### 4.6 Policy plane

Tiny (0.5–3B), role-specialized policy models fed compact state
summaries. A 7B consulted on 8k tokens of raw state costs ~2–3 s per
decision at measured rates (prefill 3.7–7.7k tok/s + ~50 tokens decoded
at 66–76 tok/s); a tiny model on a compact summary targets
~100–500 ms — fast enough to consult every scheduling quantum. Duties: eviction advice, priority advice,
degradation-rung advice, task routing. Constraints:

- Admitted by measured verdict like any other model (law 5).
- Fail-closed with a deterministic heuristic underneath (law 8).
- Budgeted and journaled like any other inference (laws 6–7).

Large models are workloads, never OS reflexes. This is orthodox
microkernel doctrine — policy out of the kernel — applied to the LLM.

### 4.7 Boot story: assay is the POST

Boot sequence: probe hardware → probe each configured model's *serving
state* (assay embedded: geometry, ceiling, envelope, codecs, speed) →
publish tier-marked capability profiles → admit workloads against
verdicts. Re-probe on drift signals (the 11.5k ceiling that appeared
and vanished on one unrestarted daemon is why probing is continuous,
not install-time). Missing or unmeasured capability boots **degraded
and says so**: affected task classes are refused with the arithmetic
printed, robigo-style. Profiles use assay's declared hardware tiers;
the primary development target is the enthusiast-16GB tier already
profiled; mid-gamer (8–12 GB) is the second target, per the mission.

## 5. Phases

- **Phase 0 — spec, prior art, gate pre-registration.** Prior-art pass
  (vLLM/SGLang prefix caching and PagedAttention, LMCache, llama.cpp
  slot save/restore, AIOS, mistral.rs, candle) per the
  research-and-reuse rule. Pin final gate numbers (provisional values
  in §6) before any gate's instrument exists.
- **Phase 1 — the pager daemon** on stock Linux, current box:
  multi-agent priority paging of weights + KV images, embedded assay
  POST, journal, `/v1` shim. Independently useful ("the serving daemon
  that doesn't lie") and independently publishable. Gate **G2** applies.
- **Phase 2 — the OS surface:** edit-codec ABI, capability grants,
  budgets, policy plane. Castle VTT becomes consumer #1 via the `/v1`
  shim, which fixes its audited AI defects (front-truncation, missing
  `max_tokens`, no context visibility) at the substrate. Gates **G1**
  and **G4** apply.
- **Phase 3 — the appliance image:** vendored Linux kernel as firmware,
  Rust PID 1, no conventional userland, console interface. This is the
  point where bloomery becomes an operating system rather than a
  daemon.
- **Phase 4 (conditional) — bare-metal exploration.** Only if a
  GPU-driver path stops being decade-class. Not scheduled.

Each phase gets its own spec → plan → implementation cycle; this
document is the umbrella architecture. Phase 1 is the first
sub-project to be planned.

## 6. Pre-registered gates (provisional numbers; pinned in Phase 0)

Provisional values below are declared now to prevent drift; Phase 0
finalizes them **before** any gate's instrument is built, and the
pinned versions supersede these.

- **G1 — policy value.** On a frozen replay benchmark of recorded
  workloads, tiny-model policy must beat the deterministic heuristic on
  useful-work-per-GPU-second by ≥10% with a contract-violation rate
  ≤5% and per-decision latency ≤500 ms. ("Useful work" gets a pinned,
  mechanical definition alongside the numbers in Phase 0 — e.g.
  completed-task tokens weighted by priority; the endpoint must be
  computable from the journal alone.) **Kill:** v1 ships
  deterministic-only; LLM policy demoted to human-request granularity.
- **G2 — pager feasibility.** p95 warm agent switch (KV image in RAM,
  weights resident) ≤2 s on the enthusiast-16GB tier; p95 cold switch
  (weights from NVMe) ≤5 s. **Kill:** the process model is redesigned
  before anything is built on it.
- **G3 — semantic view value.** On a frozen retrieval task set drawn
  from real agent runs, semantic resolution must beat a grep/fd
  baseline by ≥15pp top-5 hit rate. **Kill:** the view stays an
  app-level index and never gets syscall status.
- **G4 — codec landing.** Per model, landing under the OS's real
  envelope (applies-and-parses lens) ≥80% for the codec the profile
  selected. **Kill:** the model is demoted to a narrower verb set or
  refused for mutating roles.

## 7. The fine-tune flywheel

The journal yields verified decision/repair corpora → the black-oxide
fine-tune track (SPEC §32.4; corpus factory already gate-cleared for
training use) produces policy- and repair-tuned small models → the same
OS re-measures whether they widen the capability window, against the
same gates. The robigo 1.06% null is the effect window: this is the
only path by which the LLM's seat in the OS grows on evidence rather
than hope.

## 8. Non-goals (v1 cut list)

- Bare metal (Phase 4 flag only).
- Binary codec or any grammar-forced decoding.
- Vector-primary namespace.
- Distributed / multi-node anything.
- GUI (headless appliance; console/SSH + `/v1` + native API).
- Non-NVIDIA targets (though the llama.cpp Vulkan path keeps the AMD
  door open for free).
- Paid cloud endpoints as OS-managed resources (assay's rule: against a
  metered API those tokens are money; cloud models may appear later as
  explicitly-budgeted foreign resources, not v1).

## 9. Risks and open questions

- **KV image portability** across llama.cpp versions/builds: images may
  be invalidated by upgrades more often than by design intent.
  Mitigation: digest-tagged images; treat invalidation as a cold start,
  never an error.
- **Vulkan vs CUDA performance** on the target tiers; measured, not
  assumed, during Phase 1.
- **Multi-model floor on 8–12 GB tiers:** a policy model + workload
  model + embedding model may not co-reside; the pager must make
  time-sharing acceptable there, or the mid-gamer tier demotes the
  policy plane to heuristics (which law 8 makes a supported
  configuration, not a failure).
- **Policy-model quality at 0.5–3B** for eviction/priority advice is
  unproven — exactly what G1 exists to answer.
- **Prior-art overlap:** if the Phase 0 pass finds an existing system
  covering the pager's core (multi-agent priority paging of weights +
  KV on one consumer GPU), adopt or wrap it per the reuse rule rather
  than rebuilding.

## 10. Relationship to the sibling repos

| Repo | Contribution to bloomery |
|---|---|
| **assay** | Embedded as the POST and the admission-control instrument; tier system; None-vs-zero; Budget; lens discipline |
| **robigo** | Geometry law, degradation ladder, refusal-with-arithmetic, action surface, run records; its frozen benchmark measures future models for the flywheel |
| **black-oxide** | Codec/diagnostic design lessons (repair loop, envelope-not-payload, deformation hazard); the fine-tune track and corpus factory |
| **Castle VTT** | Consumer #1 of the `/v1` shim; its AI audit findings are the substrate's first acceptance list |

## 11. References

- robigo findings: `docs/findings/2026-08-12-the-instrument-fails-first.md`
  (KV 14×, transient ceiling, instrument-defined landing, the 1.06% null)
- robigo gate: `docs/superpowers/evidence/2026-08-12-stage4-gate.md`
- assay live validation:
  `docs/superpowers/evidence/2026-08-12-live-validation.md`; tier
  profiles under `docs/superpowers/evidence/tier-enthusiast/`
- black-oxide findings:
  `docs/findings/2026-08-12-constrained-decoding-deforms.md`,
  `docs/findings/2026-08-12-ergonomics-beat-ownership.md`; SPEC §32.4, §53–57
- Landscape: AIOS (arXiv 2403.16971); the 2026 "Agent OS" product
  category (OpenClaw, Windows agent workspaces) — orchestration-layer
  competitors that assume an honest, capable endpoint.
