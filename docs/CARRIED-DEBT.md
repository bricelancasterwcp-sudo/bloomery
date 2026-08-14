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
   `budget − Σ loaded weights − Σ resident kv`, a cold model's weights
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
   attempt). Measuring it at model-load time, or reading it back from
   llama.cpp, is the honest fix.

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
