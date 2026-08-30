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
and is **not** a re-read of G2, which stands exactly as published. **Phase
2b/2c (edit-codec syscall ABI, capability grants, the task loop that wires
both into a real HTTP surface, and the G4 codec-landing gate that measures
each model's patch codec at boot) has since landed**, dark by default
(`tasks_enabled = false`) — see [Task loop](#task-loop-phase-2b2c-p3) and
[Codec gate](#codec-gate-phase-2b2c-p4) below. Phase 4 (policy plane,
semantic store, appliance boot) is not built. See
[Honest limits](#honest-limits) before believing anything else.

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
* **A swap candidate is answered, not guessed.** `POST
  /models/{m}/swap-candidate` takes `{"gguf_path": …}` and asks whether that
  candidate **covers** what `{m}`'s blessed baseline says `{m}` was relied on
  for — a one-directional comparison of the candidate's freshly probed profile
  against the floor, run as `assay cover` and read through its four exit codes
  (`0` covered, `1` not covered, `2` refused, `3` incomplete; an unmeasured
  floor cell is never a pass). The probe holds VRAM for ~10 minutes, so the
  POST answers `202` and `GET /models/{m}/swap-candidate` carries the verdict,
  the exit code, both digests and the retained profile path. One candidate at a
  time, no queue. **Advisory**: nothing blocks and nothing auto-swaps — the
  verdict is evidence, journaled, and config stays the operator's. The whole
  flow is driven in the suite through injected probe and cover seams (no
  python, no assay, no GPU), and **live-verified end-to-end 2026-08-20**: a
  real candidate reached a real `covered` verdict — weak by construction
  (34/34 quick-mode cells within noise, silent on G4/G5) — against the
  standing baseline:
  [the evidence](docs/superpowers/evidence/2026-08-20-swap-candidate-live-2.md).
  **On a tier where the candidate cannot fit beside the resident model,
  unload first.** The pager charges every loaded model's weights to one
  budget and reclaims only agents' KV, so the candidate's probe requests are
  refused (`503 residency_refused` on `/v1`) and the job lands as an `infra:`
  report rather than a verdict. `POST /models/{m}/unload`, then the
  swap-candidate POST, is the operator flow — measured live, same evidence
  doc.
* **A memory organ (config-off by default).** An exact-repeat episodic store:
  a task that ends `done` with a landed patch and a passing granted run mints
  an episode; a byte-identical repeat (same goal, same pre-touch file bytes,
  grant-gated) gets the prior evidence injected into its prompt; strangers and
  drifted workspaces get silence; every outcome is journal-stamped, and every
  frozen instrument runs memory-off. `[memory] enabled = true` to turn on;
  operator surface `GET /memory` + `DELETE /memory/{id}` + a `/status` block.
  Design: `docs/superpowers/specs/2026-08-26-memory-organ-design.md`; live
  acceptance: `docs/superpowers/evidence/2026-08-26-memory-organ-acceptance.md`.
* **Two HTTP surfaces.** A native API (`/agents`, `/agents/{id}/infer`,
  `/suspend`, `/resume`, `/models/{m}/unload`, `/models/{m}/bless`,
  `/models/{m}/unblock`, `POST`/`GET /models/{m}/swap-candidate`,
  `GET`/`DELETE /memory`, `/status`) and an OpenAI-compatible shim
  (`GET /v1/models`, `POST /v1/chat/completions`).
* **A journal you can replay.** Every boot writes `boot-<ts>.jsonl`; every
  admission, decision, paging op, refusal and degradation is a line in it, and
  every row carries its writer's wall-clock stamp (`epoch_ms`) so a row can be
  correlated with clocks *outside* the journal — GPU sample logs, daemon
  stderr, an operator's notes. The stamp is the append instant (a row with a
  `duration_ms` spans roughly `epoch_ms − duration_ms` to `epoch_ms`) and is
  wall clock, not monotonic — file order is the row order. Rows written
  before 2026-08-20 predate the stamp; they replay unchanged. The G2 numbers
  above are computed from nothing else — see
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
* **The per-model weights charge is derived once, then declared with headroom.**
  The `weights_vram_mib` field (spec §2–§5) declares an upper bound on a model's
  VRAM charge; the effective charge is `min(declared, measured_weights_bytes)` —
  declared absent means the file's full measured weight. The declared value is
  derived once per model according to spec §5's procedure: (1) Load the model once
  at the chosen `n_gpu_layers` (a scratch data_dir with `allow_unprofiled` is
  fine); (2) read llama.cpp's buffer-size log lines and the nvidia-smi delta;
  (3) declare `weights_vram_mib` with headroom above the observed number;
  (4) commit the log excerpt as evidence. The **active value is configured, not
  measured per run**, and bloomery never reads it back from the substrate, so a
  different model, hardware, or offload strategy can need a different number.
  Like `ctx_overhead_mib`, the declared value, when present, is used by placement
  budget and the window law's VRAM term (one value, both places, per spec §3).
  Setting `weights_vram_mib` too low is an OOM, not a refusal: the pager budgets
  against it, but admission will fail at the substrate's own memory limit if the
  declared value underestimates.
* **KV images stay fully charged to VRAM under partial offload.** llama.cpp
  places KV for CPU-resident layers in host RAM, so charging the full KV cache
  to the VRAM budget overcounts need — a conservative direction (smaller windows,
  earlier refusals). This never causes an OOM; it only makes admission stricter.
  Recorded here as a known honest limit, not changed in this slice.
* **Hybrid (attention + recurrent) models need a larger `ctx_overhead_mib`
  than the dense-model default.** The first trained member of a hybrid MoE
  line — `qwen36-reap48-flywheel5` (Gated-DeltaNet + full-attention, 133
  experts, LoRA on attention + shared-expert modules, experts/router frozen)
  — inherits this from the 2026-08-21 spike's measurement on the untrained
  base: a 493 MiB Vulkan compute buffer at n_ctx 54,784
  ([turn-5 design spec](docs/superpowers/specs/2026-08-22-flywheel5-turn5-design.md)
  §2). The buffer grows with `n_ctx` (a named residual, not modeled);
  `ctx_overhead_mib = 512` is the operator setting this turn's boots use,
  chosen to cover the measured 493 MiB rather than the 384 default.
  `kv_per_token` and `recurrent_state_bytes` are still derived from the
  GGUF's own `full_attention_interval`/`ssm.*` metadata, unchanged by
  training. The line's battery (G4 20/20, G5-v4 patch 16/16 and refuse
  16/16, both **decided** PASS, `done_trust: true`) is
  [recorded here](docs/superpowers/evidence/2026-08-23-flywheel5-battery.md).
- **Turn 6 (the honesty instrument):** envelope-v5's declared `done`
  (outcome/reason + evidence lines), the frozen `codec-tasks-v5-mixed`,
  three exact declaration endpoints, a pre-registered v4 claim audit,
  and four-model baselines —
  [audit](docs/superpowers/evidence/2026-08-29-v4-claim-audit.md) ·
  [protocol](docs/superpowers/evidence/2026-08-29-g5v5-protocol.md) ·
  [baselines](docs/superpowers/evidence/2026-08-29-g5v5-baselines.md).
- **Turn 7 (training the declarations):** `qwen36-reap48-flywheel7`,
  trained on the first corpus of declared-`done` ideals
  (`generate_envelope_v5`, seed 20260829, every evidence quote proven
  grounded under the shipped scorer's own rule before training) against
  seven pre-registered floors locked before the pod was cut — **PASS on
  all seven** (outcome consistency 32/32, evidence grounding 28/32 from
  an untrained 8/32, `different-defect` present where it was universally
  absent), verdict produced by the repo's own `derive --evaluate`, never
  by prose arithmetic —
  [pre-registration](docs/superpowers/evidence/2026-08-29-flywheel7-preregistration.md) ·
  [training](docs/superpowers/evidence/2026-08-29-flywheel7-training.md) ·
  [battery](docs/superpowers/evidence/2026-08-29-flywheel7-battery.md).
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
  not survive two. Phase 2b/2c P3's task loop makes this coarser still: a
  running task holds that same lock for its *whole* duration (see "Task loop"
  above), so a long `run` step can block every other agent's inference for up
  to `run_timeout_secs`, not just its own task's.
* **`/v1` streaming is buffered.** `stream: true` returns real SSE framing, but
  the whole completion is generated first and then written — the shape is
  compatible, the latency benefit is not there.
* **POST costs ~110 s per model per boot**, sequentially, and the daemon
  provisionally admits unprofiled models for that whole window (journaled).
  Set `assay.enabled = false` to skip it, and `allow_unprofiled = true` if you
  accept serving a model whose ceiling and codecs nobody measured.
* **The G4 codec probe costs real GPU minutes per model per boot, stated
  plainly.** With `tasks_enabled = true` and `assay.enabled = true`, boot runs
  the codec probe against the frozen `codec-tasks-v1` set (N=20) for every
  configured model, strictly after POST finishes — up to 20 fixtures × 6
  steps (`FIXTURE_MAX_STEPS`) = **up to 120 steps (≤3 inference calls each —
  a strict ceiling of ~360 calls, bounded in practice by the 30k
  per-fixture budget) per model** before that model's boot probe completes,
  each fixture holding the same coarse whole-task pager lock the task loop
  already does (see "One lock, held for a whole task" above), so this is
  minutes of GPU-busy, daemon-wide-blocking time per model, not seconds —
  and, like POST, this is a **per-boot** measurement only: there is no
  continuous re-probing (same honest limit as the POST line above). Turn
  `tasks_enabled` off, or `assay.enabled` off, to skip it — either one
  leaves every model's mutating verbs (`patch`/`run`) refused (see
  [Codec gate](#codec-gate-phase-2b2c-p4) below).
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

### Capability grants (Phase 2c P2)

* **Four fields, JSON, immutable:** `read_roots`, `write_roots` (absolute
  paths), `commands` (argv-prefix allowlists), `network` (`false` only —
  refused, not sandboxed; a granted command is *trusted* non-networking).
* **Canonical-path escape defense.** Real `std::fs::canonicalize`
  (symlinks followed, `..` collapsed), compared component-wise — no
  string-prefix match (`/root-evil` never passes a `/root` grant).
* **Argv-prefix allowlist, no shell.** `run`'s argv must start with a
  granted prefix element-wise; exec'd directly, never `sh -c`.
* **Structural, not persuadable.** The check takes a path/argv and a
  `Grant`, never file content — a red-team suite proves an injection-laced
  file can be read but its payload (exfil, `curl`, `bash -c`) is refused
  regardless of what it says.
* **Type + checks only.** P3 wires these into the task-loop's executors;
  `tasks_enabled` (default `false`) gates the whole task surface.

### Task loop (Phase 2b/2c P3)

* **propose → validate → execute, journaled every step.** `run_task` prompts
  the model with the goal and the P1 verb card, decodes its reply through
  `parse_action_with_codec`, dispatches the validated action to its executor,
  and journals a `TaskStep` — win, loss, or unparseable — before repeating. An
  unparseable turn is re-asked up to twice before the step is recorded
  failed; a grant violation is a failed step the model can see and recover
  from, not a task abort. The task ends on the model's own `done`, on
  `max_steps` running out, on the pager's own budget or window refusal, or on
  a substrate/journal failure — never on a stuck step.
* **Five verbs, four bounded executors.** `read` and `find` (capped by
  `read_cap_bytes` / `find_result_cap`), `patch` (atomic write, fsync'd, then
  P1's landing lens checks it both applies and parses), and `run` (no shell,
  a scrubbed `PATH`/`HOME`/`LANG` environment, `run_output_cap_bytes` /
  `run_timeout_secs`, its whole process group killed on timeout). `done` ends
  the task; it has no executor. **A granted command's arguments are not
  path-scoped:** the `commands` allowlist checks only the program and its
  argv prefix, never what paths the arguments name — a grant for `cat` lets
  the model run `cat /etc/passwd` regardless of `read_roots`. Operators
  choosing command grants must treat each granted program as fully trusted
  with whatever arguments the model supplies, not as implicitly confined to
  the grant's roots.
* **Every filesystem open is grant-checked, then `O_NOFOLLOW`.** Each
  executor opens the *canonical* path P2's `Grant::check_read`/`check_write`
  returned — never a path re-derived from the model's own string — with
  `O_NOFOLLOW` on the final component, so even a same-instant symlink swap of
  the checked path's last segment is refused (`ELOOP`) rather than followed.
  **Named v1 limit:** `O_NOFOLLOW` only protects the final path component; a
  TOCTOU race against a *mid-path* directory (swapped for a symlink between
  the grant's canonicalization and the open) is not closed by this call —
  that needs Linux's `openat2(2)` with `RESOLVE_NO_SYMLINKS`, not yet wired.
* **One lock, held for a whole task.** `run_task` takes `&mut Pager` for its
  entire call, so `TaskRegistry::spawn_task`'s background worker locks the
  shared `Arc<Mutex<Pager>>` once and holds it for the task's full duration —
  including the time an `exec_run` subprocess spends running and every
  executor's file I/O — rather than only across each step's `infer` call.
  Defensible for v1 because one GPU already serializes every `infer` daemon-
  wide (see "One coarse lock" above); the cost it adds is that a long `run`
  step (bounded by `run_timeout_secs`, default 120s) now blocks every *other*
  agent's inference for up to that long too, not just its own task. Revisiting
  this means threading a lock-per-`infer` shape through `run_task` itself,
  deferred past P3.
* **`tasks_enabled` (default `false`) gates the whole surface.** With it
  unset, `POST /agents/{id}/task` answers `501 {"error":"tasks_disabled"}`
  regardless of the request body. Enabled, `POST /agents/{id}/task
  {goal, grants, budget_tokens?, max_steps?}` returns `202 {"task_id"}` (a
  background worker started), `422 {"error":"invalid_grant", detail}` when
  `grants` fails P2's `Grant::from_json` validation (or has neither a
  `write_roots` nor a `read_roots` entry to run in), or `404` for an unknown
  agent; `GET /agents/{id}/task/{task_id}` polls a snapshot — `200
  {status, steps, summary}` or `404`. `steps` always lists every recorded
  `TaskStep`, in order — a re-asked step can produce more than one record
  sharing a step number, so nothing here assumes one record per step.
* **Codec choice and mutating-verb gating are P4's, not P3's.** A task now
  resolves its per-model `patch` codec and whether mutating verbs
  (`patch`/`run`) are even available from the pager's G4 verdict —
  see [Codec gate](#codec-gate-phase-2b2c-p4) below. Otherwise still
  local-only and buffered like the rest of this daemon: no remote execution,
  no streaming of a task's progress beyond polling `GET`.

### Codec gate (Phase 2b/2c P4)

* **Fail-closed, unmeasured is never permission.** Every model's mutating
  verbs (`patch`/`run`) are refused by default. They are enabled
  **only** for a model that has a *stored* G4 verdict recorded on the pager
  **and** whose verdict itself says keep (`landed * 5 >= n * 4`, no float
  edge) — a model that was never probed, is still probing, or whose probe
  aborted mid-run reads exactly like a demoted model, never like a
  permissive default. `read`/`find`/`done` are never gated; only the
  mutating two are.
* **What the gate measures.** The frozen `codec-tasks-v1` fixture set (N=20
  single-defect repair tasks, embedded in the daemon binary) run through the
  *real* `run_task` loop — real prompts, real envelope decoding, real
  executors, real grants — against each model's profile-selected patch codec
  (`SearchReplace` or `WholeFile`; `SearchReplace` when unprofiled or
  unmeasured). A fixture lands iff a `patch` step succeeded **and** the
  declared target file's bytes actually changed — either alone would score a
  non-repair as a repair. Any infrastructure failure (a substrate error, a
  refused agent creation, a poisoned lock, an unwritable journal, a
  panicking task) aborts that model's *whole* probe: no verdict, no partial
  score, the model stays unmeasured rather than a confident zero.
* **Runs once per boot, strictly after POST.** Wired into the same POST
  thread, after `run_post` returns `Ok` (profiles attached, `posting`
  cleared) — see the boot-cost line in [Honest limits](#honest-limits)
  above. `assay.enabled = false` **or** `tasks_enabled = false` skips it
  entirely (mutating verbs stay refused either way); with both `true` it
  runs for every configured model, in the same order POST probed them, and
  one model's abort never stops another's.
* **Demotion is per-boot state, never persisted.** The stored verdict lives
  in memory on the pager, exactly like every other piece of boot-time
  measurement here (VRAM budget, POST profiles). A restart re-measures from
  nothing — there is no notion of a demotion "sticking" across boots, and no
  notion of merging an old verdict with a new one.
* **`/status` renders it per model.** `mutating_verbs` (bool, the enforced
  decision) and `codec_gate` (the stored verdict's `fixture_set`, `codec`,
  `landed`, `n`, `interval95`, `provisional` — or JSON `null` when the model
  has never completed a probe, never a confident zero) sit beside the
  existing `patch_codec` field on every entry in `models`.
* **`CodecFixture` rows are a rate only under a matching `CodecVerdict`.** A
  mid-set abort leaves the fixture rows that already ran permanently in the
  journal (append-only, nothing retracts them) — diagnostic records of what
  ran, not a partial measurement. Reading a landing rate from them requires
  bounding by a `CodecVerdict` for that exact model and set; rows with no
  matching verdict are orphans and must never be hand-summed into a score.

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
tasks_enabled = false              # true = expose POST/GET /agents/{id}/task
read_cap_bytes = 262144            # task loop: max bytes a single `read` returns
find_result_cap = 100              # task loop: max matches a single `find` returns
run_output_cap_bytes = 65536       # task loop: max bytes a `run` step's output returns
run_timeout_secs = 120             # task loop: wall-clock cap on a `run` step's subprocess

# Per-model tuning (spec §2–§5): Each entry can be either a bare path string (today's shape)
# or a table with `path`, optional `n_gpu_layers`, and optional `weights_vram_mib`.
# `n_gpu_layers` overrides the global default, enabling partial offload.
# `weights_vram_mib` declares the model's VRAM charge as a ceiling, clamped to the file's
# measured weights (effective_weights = min(declared, weights_bytes)). Both fields optional;
# omitting both is byte-for-byte today's behavior. Declared weights charge feeds placement
# budget and the window law's VRAM term. Setting declared too low is an OOM, not a refusal.
# KV images stay fully charged to VRAM under partial offload — a conservative (smaller
# windows, earlier refusals) overcount that never breaks admission; recorded as honest limit.

[models]
# today's shape — unchanged, still valid
"qwen3:14b" = "/mnt/extra/ollama-models/blobs/sha256-…"

# new shape — per-model tuning
[models."qwen3.8:27b"]
path = "/mnt/extra/ollama-models/blobs/sha256-f5f1dd89…"
n_gpu_layers = 28          # optional; omitted = full offload
weights_vram_mib = 11264   # optional; omitted = charge full weights

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
* [G4 codec-landing protocol](docs/superpowers/evidence/2026-08-15-g4-protocol.md) — pre-registered before the codec-gate instrument existed
* [G2 evidence](docs/superpowers/evidence/2026-08-14-g2-agent-switch.md) — the switch-latency run, its lens, and its caveats
* [2a natural-pressure evidence](docs/superpowers/evidence/2026-08-14-2a-natural-pressure.md) — eviction under a measured budget with the weights charged; an acceptance run, not a gate re-read
* [Carried debt](docs/CARRIED-DEBT.md) — what each slice settled, what it deferred, and what has since been delivered
* [Phase 0 prior art](docs/priorart/2026-08-14-phase0-priorart.md) — what already exists and what was decided against it
* [Phase 1 plan](docs/superpowers/plans/2026-08-14-phase1-pager-daemon.md)

Sibling repos whose measurements this project is built on:
[assay](https://github.com/bricelancasterwcp-sudo/assay) (capability profiles),
[robigo](https://github.com/bricelancasterwcp-sudo/robigo) (the VRAM-budget
coding agent and its 1.06% null result).

## License

bloomery is **dual-licensed**.

* **Code** (`crates/`, `tools/`) — [GNU AGPL v3.0](LICENSE)
  (`AGPL-3.0-only`), free for any purpose including commercial use, on the
  AGPL's terms. bloomery is a network service, so **section 13 applies**: if
  you modify it and let users reach it over a network, those users must be
  offered your modified source.
* **Documents** (`docs/`) — [CC BY 4.0](docs/LICENSE). The specs,
  pre-registrations, evidence and findings are meant to be quoted, cited and
  argued with. Attribution is the only condition.
* **A commercial license** is available for embedding bloomery in a closed
  product, running a modified instance as a service without section 13, or
  when AGPL is barred by policy — and it is the only way to get warranty,
  indemnity or a support commitment. The AGPL grant carries none of those.

Read [LICENSING.md](LICENSING.md) for what each option requires and how to
enquire; [CLA.md](CLA.md) if you want to contribute. Every dependency is
permissive (MIT / Apache-2.0), llama.cpp included — the copyleft here is a
choice, not something inherited.

Copyright © 2026 Brice Lancaster.
