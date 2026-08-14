# Carried debt — Phase 1 (recorded 2026-08-14 at the final whole-branch review)

Known gaps deliberately carried out of Phase 1, with the final review's
triage. None blocks the merge; several are named Phase 2 work items.
Deferred-minor detail lives in the task-review history; this is the
durable list.

**Amended 2026-08-14** (Phase 2a hardening): work items 1–4 are
delivered and moved to the section below, struck through with the text
they were recorded under. Nothing is deleted — the record of what was
carried, and for how long, is the point of this file.

## Delivered in Phase 2a (2026-08-14)

Struck through, never deleted: the original text stands as recorded, with
what closed it. Branch `feat/phase2a-hardening`.

1. ~~**Weights are not charged to the reservation budget.** The planner
   accounts KV bytes only, and the VRAM budget is a static boot-time
   read taken before any model loads. This is the loudest honest limit
   (README), the reason the G2 pressure configuration exists, and the
   first pager work item for Phase 2. The evidence doc's pressure
   arithmetic (§2) is effectively the design note.~~
   **DELIVERED** — `Pager::place` plans against
   `budget − Σ loaded weights − Σ resident kv` *(superseded — the shipped
   formula is two terms wider; see the end of this item and item 7)*, a cold
   model's weights
   join the demand side of its own admission, refusals print the whole
   arithmetic, and `/status` carries `loaded_weights_bytes`. Live
   evidence of eviction under a natural measured budget (48 warm
   switches, 53 evictions, measured budget, no `vram unmeasured`
   degradation):
   `docs/superpowers/evidence/2026-08-14-2a-natural-pressure.md`.
   The budget remains a *static* boot read by standing ruling below.
   **The first live attempt OOM'd the device and found a second missing
   term** — a context reserves llama.cpp compute buffers beyond its KV
   cache — so the same accounting now charges
   `budget − overhead − Σ weights − Σ (kv + ctx_overhead)`. See item 7.
2. ~~**`model_digest` is `sha256(first 1 MiB ‖ file_len)`.** Latent
   collision risk (two fine-tunes sharing prefix + length). Harmless
   while KV images are boot-scoped; MUST be strengthened (full hash or
   multi-offset sampling) before images become restart-survivable —
   otherwise a silent wrong-weights KV restore becomes possible.~~
   **DELIVERED** — the digest is now a streamed full-file SHA-256, so
   restart-survivable images are no longer blocked on this. (Images are
   still boot-scoped, for the unrelated reason that the image store's
   index lives in memory — README, honest limits.)
3. ~~**Add an `AgentRemoved` journal event** at the next journal schema
   change — ephemeral `/v1` agents currently leave phantom
   `AgentCreated` entries in a replay (G2's committed journals are
   unaffected; POST was off).~~
   **DELIVERED** — `AgentRemoved` is journaled on removal (and a
   `TaskStep` variant was added to the schema at the same change, for
   2b). The committed Phase 1 journals still replay unchanged.
4. ~~**Equal-priority peers refuse rather than time-share** (planner
   requires a strictly-lower-priority victim). Correct per the pinned
   semantics; a fairness/time-slicing policy is a Phase 2 design
   question (surfaced by the G2 bench protocol shaping).~~
   **DELIVERED** — the planner is unchanged (still strictly-lower-
   priority, still deterministic); the pager retries a *qualifying*
   equal-priority refusal as an LRU eviction once it has waited out
   `time_share_quantum_secs` (default 30 s), journaled as
   `evict_timeshare(waited_Nms)`. All time reads go through one
   injectable clock so the rule is testable without sleeping.

## Phase 2 work items (in recommended order)

5. **NVMe-media KV image read is unmeasured** — every recorded
   `ResumeLoad` (gate runs and the cache-dropped probe) was served at
   page-cache speed. Measure before anything depends on cold-image
   latency.
6. **No drift re-probe**: assay POST runs at boot only (~110 s GPU per
   model, sequential). Spec §4.7's continuous probing is knowingly
   boots-only in Phase 1.
7. **The per-context runtime reservation is configured, not measured.**
   `ctx_overhead_mib` (default 384) is a measured floor for
   qwen2.5-coder-7b-q8_0 at `n_ctx = 16384` on this box's Vulkan driver
   — 304 MiB compute buffer + 30 MiB host buffer — not a property
   bloomery reads from the substrate. Another model, backend or window
   will want another number, and setting it too low is an OOM rather
   than a refusal (recorded 2026-08-14 by the aborted natural-pressure
   attempt). The default is derived from a measured floor — excerpt
   committed as
   `docs/superpowers/evidence/2026-08-14-2a-daemon-log-excerpt.txt` —
   but the *active* value is configured, never measured per run.
   Measuring it at model-load time, or reading it back from llama.cpp,
   is the honest fix.

   **Window/placement asymmetry (same item, second half).**
   `usable_window`'s VRAM term subtracts `weights` and `overhead_bytes`
   but **not** `ctx_overhead_bytes`, while placement charges
   `kv + ctx_overhead`. An agent whose window comes out `BoundBy::Vram`
   is therefore sized to consume the whole remaining budget and then
   reserves exactly `ctx_overhead_bytes` more than exists: permanently
   un-placeable, refused safely (law 1, pre-checked, nothing allocated)
   but with no smaller window to fall back to and no recovery short of
   lowering the window cap or `ctx_overhead_mib`. Not hit by the
   2026-08-14 run — all 16 agents were `user_cap`-bound, verified from
   the committed journal — and pinned in
   `pager_reservation_test.rs`'s refusal scenario, which is the same
   end state. **The real fix is a core geometry change**: the window law
   must subtract the per-context reservation it is sizing, which means
   `GeometryInput` grows a term and every window-law test moves with
   it. Deliberately deferred rather than bolted on beside a live-run
   fix.

   **Multi-model window/placement divergence (same item, third half;
   added 2026-08-14 in the final Phase 2a review, flagged as M-3 in
   Task 3's review and routed here).** The ctx_overhead half above is
   only one side of the same asymmetry. `create_agent` sizes a
   candidate's window from `budget − <this agent's own model's>
   weights_bytes − overhead_bytes` (`usable_window`'s `Vram` term,
   `crates/bloomery-core/src/geometry.rs`) — it reads only the one
   model the agent is being created against. `Pager::place`'s
   admission arithmetic charges the fuller picture: `budget − overhead
   − Σ ALL loaded models' weights − Σ ALL resident contexts'
   reservations`. On a single-model daemon these coincide — there is
   only one model to be "all loaded weights." On a multi-model daemon
   they diverge: an agent windowed while a *different* model is also
   loaded gets a window sized against a budget that never subtracted
   that other model's weights, so a `Vram`-bound window can be
   over-optimistic by up to that other model's entire `weights_bytes`
   — and `place()` then refuses it permanently, for the same reason
   the ctx_overhead half never recovers: the window law doesn't know
   the term admission will actually charge. Same refuse-safely-
   never-recovers class as the ctx_overhead half above, but larger in
   magnitude (a whole model's weights vs. a few hundred MiB of context
   overhead). The real fix is the same deferred core geometry change
   already named above: the window law must subtract what admission
   will actually charge, which for a multi-model daemon means
   `GeometryInput` also carrying the other models' loaded weights and
   residents' reservations — not a second, separate fix.

## Smaller items (fine as-is; fix opportunistically)

- Test scratch accumulates in /tmp (pid-suffixed `bloomery-*` dirs from
  tests that bind `_handle` without `shutdown()`); a RAII guard in test
  fixtures would end it.
- `pager_test.rs` is 834 lines (only file over the 800 ceiling); move
  `FailingSubstrate` to a helper module.
- `/v1` 429/503/422 extension rows lack dedicated tests.
- `probe_each`: an `attach_profile` failure aborts remaining probes and
  is reported as a journal failure (unreachable today).
- Substrate-level `vulkan` feature does not imply `llama`
  (daemon-level does; real build path covered).
- assay child is killed directly on probe timeout, not as a process
  group; an assay grandchild would be orphaned.
- Buffered SSE emits one final chunk (finish_reason + usage merged)
  rather than OpenAI's two-chunk trailer.
- `classify_infer_error` is a substring contract on shared consts
  (`WINDOW_EXCEEDED`, `STATE_SIZE_MISMATCH`) rather than typed — safe
  today, traced.

## Standing rulings (do not re-litigate without a recorded amendment)

- **Static VRAM budget convention**: the pager's `free_vram` closure is
  a boot-time budget, never a live driver read (live free already
  excludes allocated contexts → double-count).
- **Anti-ratchet**: self-measured (POST) profiles never clamp geometry
  via `measured_ceiling`; external profiles do.
- **`SendLlama`**: the `unsafe impl Send` lives in the daemon beside
  the mutex that discharges it; Send only, never Sync; actor-thread
  fallback documented at the impl.
- **Poisoned pager lock** → sticky named 500s; never `into_inner`.
- **State restore failures**: size-mismatch/corrupt/stale-digest are
  cold starts, never errors; transient failures re-insert the image
  for retry.
