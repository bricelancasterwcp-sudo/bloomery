# Carried debt — Phase 1 (recorded 2026-08-14 at the final whole-branch review)

Known gaps deliberately carried out of Phase 1, with the final review's
triage. None blocks the merge; several are named Phase 2 work items.
Deferred-minor detail lives in the task-review history; this is the
durable list.

**Amended 2026-08-14** (Phase 2a hardening): work items 1–4 are
delivered and moved to the section below, struck through with the text
they were recorded under. Nothing is deleted — the record of what was
carried, and for how long, is the point of this file.

**Amended 2026-08-15** (Phase 2b/2c P4, the G4 codec-landing gate): the
"Profile has NO codec field" ruling — recorded informally in the README's
task-surface prose (Phase 2b/2c P3) and in this same P4 sub-phase's own
implementation plan, never previously given a numbered entry in this file —
is delivered and recorded below as item 8, struck through on arrival. Four
new items are also recorded: (a) extends item 6 in place (the codec probe
is boots-only too); (b)–(d) are new items 9–11. P4 closes one debt and
opens others, same as every slice before it.

**Amended 2026-08-15** (partial-offload + G4 capability-window task 1):
item 7's "Window/placement asymmetry" half — found live by the first 14B
capability-window attempt refusing at exactly the asymmetry this file
predicted — is delivered and struck through in place, below, without
moving the compound item out of this section. Item 7's first paragraph
(configured, not measured) and its "Multi-model window/placement
divergence" third half remain open, unchanged and unstruck.

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

## Delivered in Phase 2b/2c P4 (2026-08-15)

Struck through, never deleted, same convention as the Phase 2a section
above. Branch `feat/phase2bc-p4-codec-gate`.

8. ~~**Profile has NO codec field.** `bloomery_core::profile::Profile`
   carried no per-model patch-codec selection, so Phase 2b/2c P3's task
   loop always ran every model under the fixed default codec
   (`SearchReplace`), regardless of what a model's own assay profile might
   measure landing better.~~
   **DELIVERED** — P4 added a `codecs` grid to `Profile` and
   `preferred_patch_codec` to select from it (protocol §4); `Pager`'s
   `model_patch_codec`/`model_codec_from_profile` resolve the pair for any
   model (profile-measured selection when one exists, `SearchReplace`
   otherwise — `crates/bloomery-daemon/src/pager/codec_gate.rs`); task
   creation now builds every `TaskSpec` from that resolved codec instead of
   the old literal (`crates/bloomery-daemon/src/api_task.rs`); the G4
   probe measures landing under exactly that resolved codec
   (`run_codec_probe`, Task 9); and Task 10 wires the whole thing into
   boot. The debt this closes was never a numbered item in this file — it
   lived in the README's task-surface prose and in this sub-phase's own
   plan doc — recorded here retroactively, delivered on arrival.

## Delivered in flywheel turn 2 (2026-08-16, honest-refusal branch)

**Settled:** Gate G5 (refusal honesty, advisory, per-class floors) pinned
and instrumented; frozen `codec-tasks-v2-mixed` gate set; two refusal
template families; gate-aware rejection sampling in the factory (shared
rule source with the contamination guard); `qwen3-14b-flywheel2` passed
the full pre-registered battery (G4 20/20 non-provisional; G5 10/10
patch + 10/10 refuse, both provisional at n=10) — first `done_trust`.

**Deferred from this slice (final-review triage: all defer-sound):**

- **First fast-follow:** `validate_refusal_task` lacks a structural
  assertion that refusal goals end with the check-first instruction
  (the patch-side validator has the `DONE_INSTRUCTION` analog). Today
  every template routes through `goal_phrasing` which appends it by
  construction; a turn-3 template could silently drop it.
- Contamination guard screens only `target_contents` per task; a
  missing-target task's *sibling* file (name/contents) is screened by
  neither sampler nor CLI. Exposure nil today (sibling content never
  enters a rendered pair); fast-follow: screen all `task.files`.
- `codec_probe/mod.rs` dormancy doc overstates: G4 scoring is
  unchanged, but `CodecFixture` journal rows now carry `expect` —
  "byte-comparable" is true of scores, not journal bytes. Doc tighten.
- `flywheel_tool.rs` `real_missing_target_read` interpolates a path
  into grant JSON unescaped (Linux-only tooling today; fragile if
  ported).
- `pager.rs` at 808/800 — extract `ModelEntry` (mechanical).
- gates.md G5 field ordering cosmetic; unlabeled illustrative Wilson
  literals in gate tests; per-request tempdir churn in flywheel-tool;
  dead `target_contents` field on missing-target requests (documented
  wire convention); two py defect-absent gate fixtures share a code
  shape (note for gate-set v2 — never amend the frozen, measured set);
  `codec_probe_test.rs` over the test-file cap (pre-existing).
- G5 at n=10/class: every pass provisional by construction; a decided
  pass needs the gate grown to n≥16 per class (a future, separately
  frozen set).

**Process lessons:** `cargo build --release -p bloomery-daemon` without
`--features vulkan` silently replaces the served binary with a
featureless one (boots, refuses to load models — fail-closed caught it
at smoke); always rebuild with the feature. Gate-screened generation
(rejection sampling at draw time) beats post-hoc guard-and-regenerate:
the 729-violation first attempt cost a full generation cycle the
sampler now prevents by construction.

## Phase 2 work items (in recommended order)

5. **NVMe-media KV image read is unmeasured** — every recorded
   `ResumeLoad` (gate runs and the cache-dropped probe) was served at
   page-cache speed. Measure before anything depends on cold-image
   latency.
6. **No drift re-probe**: assay POST runs at boot only (~110 s GPU per
   model, sequential). Spec §4.7's continuous probing is knowingly
   boots-only in Phase 1.

   **Extended 2026-08-15 (Phase 2b/2c P4, recorded item (a) below).** The
   G4 codec probe (`run_codec_probe`, wired into boot by Task 10) inherits
   this exact limit: it runs once per boot, inside the same POST thread,
   strictly after POST itself — no continuous re-probing here either. A
   model's codec-gate verdict, and any demotion it carries, is exactly as
   stale between boots as its capability profile is (see item 11 below for
   the demotion half of this).
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

   ~~**Window/placement asymmetry (same item, second half).**
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
   fix.~~
   **DELIVERED 2026-08-15** — `GeometryInput` gained `ctx_overhead_bytes:
   u64` and `usable_window`'s VRAM term now subtracts it too, alongside
   `weights_bytes` and `overhead_bytes`
   (`crates/bloomery-core/src/geometry.rs`); `Pager::create_agent` passes
   its own `self.ctx_overhead_bytes` in
   (`crates/bloomery-daemon/src/pager.rs`). The window law and placement
   now charge the same four terms, so a `Vram`-bound window is placeable
   by construction for a single agent — pinned by
   `pager_reservation_test.rs`'s
   `a_vram_bound_window_is_placeable_item_7_regression` and
   `geometry_test.rs`'s `vram_term_charges_ctx_overhead` /
   `ctx_overhead_larger_than_remainder_saturates_to_zero_without_panicking`.
   Found live, not in review: the first 14B capability-window attempt
   (2026-08-15, journal `~/.cache/bloomery-g4-14b/journal/`) refused at
   exactly this asymmetry, journaling:
   ```
   residency: weights 9276184896 B + reserved 4975919104 B (kv 4573265920 B + ctx overhead 402653184 B) vs budget 14923333632 B − overhead 1073741824 B − loaded 0 B − resident 0 B (needed 14252104000 B, free 13849591808 B, reclaimable 0 B)
   ```
   `needed − free = 402,512,192 B`, ≈ the 384 MiB configured
   `ctx_overhead_mib` (the small delta from 402,653,184 is kv rounding) —
   the exact signature this item predicted. See
   `docs/superpowers/specs/2026-08-15-partial-offload-capability-window-design.md`
   §3b for the amendment recorded before the fix landed. The "third half"
   below (multi-model divergence) is a separate, still-open half of the
   same item, **not** closed by this delivery.

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

   **Still open as of the 2026-08-15 delivery above.** The ctx_overhead
   half and this half were always named as needing the same
   `GeometryInput` growth, but only the single-model half of that growth
   shipped this round — `create_agent` still sizes a window from only
   its own model's `weights_bytes`, blind to any other loaded model or
   resident sibling. Pinned directly (fixed on review 2026-08-15: an
   earlier version of this note pointed at
   `a_second_agents_reservation_not_just_its_kv_is_what_refuses_it`, but
   that test gives both agents an explicit `window_cap` small enough that
   `UserCap` binds regardless of what the window law knows about
   residency — it exercises placement's already-correct whole-reservation
   subtraction across two agents, not the window law's sibling-blindness,
   and its doc comment now says so) —
   `pager_reservation_test.rs`'s
   `a_sibling_blind_automatic_window_still_refuses_item_7_third_half`
   gives **both** agents no `window_cap` and asserts `a2.window_tokens`
   and `a2.bound_by` directly: a2's automatic window law runs after a1 is
   already resident and computes the *identical* oversized, `Vram`-bound
   window a1 got, as if a2 were alone — the sibling-blindness itself,
   asserted, not merely inferred from a refusal that could have another
   cause. Placement's separate, already-correct whole-reservation
   subtraction then finds nothing left (`avail = 0`) and refuses. A first,
   otherwise-alone agent's reservation can no longer overflow its own
   budget (that's what closed); a second agent's window, sized blind to a
   resident sibling, still can be wrong (that's what's left).

9. **Recorded scoring edges (protocol §3), item (b) of the 2026-08-15
   batch.** Two landing-score edge cases are pinned by test, not by
   accident: an identity patch (search matches, replace is byte-identical
   to it) *applies* but does not count as landed — the target's bytes
   never actually changed (`an_identity_patch_applies_but_does_not_land`,
   `codec_probe_test.rs`). A patch that lands only on a scratch file the
   model wrote itself, leaving the fixture's declared `target` untouched,
   also does not count (`a_patch_landing_only_on_a_scratch_file_does_not_land`).
   Both are the scoring conjunction (§3: a `patch` step succeeded **and**
   the *declared target's* bytes changed) working as specified, not a bug
   — recorded here so a future reader who sees a model's patch "succeed"
   without moving the rate does not mistake it for a defect.
10. **The probe extends the ratified whole-task pager lock, item (c) of
    the 2026-08-15 batch.** `run_codec_probe` holds the pager lock across
    `create_agent` + `run_task` + `remove_agent` + the journal write, per
    *fixture* — the same whole-task lock `task::registry` ratified for the
    task loop generally (README, "One lock, held for a whole task"; this
    file's item 6 extension above covers the boots-only half of the same
    boot cost). A boot with `tasks_enabled` and the codec probe running
    therefore serializes daemon-wide inference for however long that
    model's fixtures take — up to 120 steps per model (20 fixtures × up to
    `FIXTURE_MAX_STEPS` = 6 steps), each step up to `MAX_PARSE_ATTEMPTS` = 3
    inference calls on a re-ask, a strict ceiling of ~360 inference calls
    per model, bounded in practice by the 30k `FIXTURE_BUDGET_TOKENS`
    per-fixture budget — which is strictly longer than POST's own ~110 s
    per model. See the "G4 codec probe costs real GPU minutes" line in the
    README's Honest limits.
11. **Demotion is per-boot state, never persisted, item (d) of the
    2026-08-15 batch.** A completed G4 verdict lives only in the pager's
    in-memory `codec_gate` field (`crates/bloomery-daemon/src/pager/codec_gate.rs`);
    nothing writes it to disk outside the append-only journal record of
    how it was reached. A restart re-measures from nothing — there is no
    notion of a demotion "sticking" across boots, and `set_codec_gate`
    replaces any previous verdict wholesale rather than merging with it.
    Matches item 6's extension above and the "No drift re-probe" reasoning
    exactly: this is a *consequence* of boots-only measurement, recorded
    separately because "unmeasured on this boot" and "demoted forever" are
    easy to conflate and the two must never be.
12. **KV is fully charged to VRAM under partial offload, recorded 2026-08-15
    (partial-offload + G4 capability-window task 4).** When `n_gpu_layers` is
    tuned to offload some model layers to CPU, llama.cpp places KV cache for
    those layers in host RAM, not VRAM. The pager conservatively charges the
    full KV cache against the VRAM budget anyway — the safe direction
    (overcount begets smaller windows, earlier refusals) and never an OOM.
    A measured read would require per-layer KV tracking; charging conservatively
    is simpler and recorded here as an honest limit, not deferred. The
    `weights_vram_mib` declared-charge field (Task 3) enables partial offload
    (smaller windows work with smaller declared weights); KV's full charge
    is the companion honest limit showing where the bounds come from.

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
