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

**Amended 2026-08-18** (verdict-gated-admission, final fix wave before
merge): two claims under "Delivered in verdict-gated-admission" overclaimed
what was actually pinned by test at the time they were written — the
provenance claim and the "pinned both at the pager layer and over HTTP"
claim — corrected in place below, struck through, with what closed the gap.
Four new items are recorded from the same review's record-don't-fix
findings: R1 (the G4/G5 probes abort for a blocked model and `unblock`
does not recover them — spec §5 also amended, a footnote, not a rewrite),
R2 (`clear_admission_block` mutates before it journals, no named outcome
for a refused row), R3 (two silent-clear paths held only by call-site
discipline, not by type or test), R4 (erratum: spec §5's POST-window
sentence is false on a multi-model daemon; the precedence it describes is
still correct).

**Amended 2026-08-19** (swap-candidate, the seam's slice 3): one *Smaller
items* entry is amended in place — the "`pager_test.rs` is 834 lines (only
file over the 800 ceiling)" parenthetical is no longer true of the repo, and
says so beneath its own unaltered text. The slice's own carry is a new section
below; nothing else in this file is touched.

**Amended 2026-08-20** (post-slice-3 follow-ups): the swap-candidate section
gains the live-acceptance arc's debt candidates — the journal wall-clock
stamp (bA2/F2) and the tight-tier unload-first operator line (bA2/F1), both
recorded and **delivered on arrival** in the P4-item-8 convention (struck
through as they land, same commit) — and a *Process lessons* block. Recorded
here because the evidence docs that found them are records of runs, not a
registry of debt.

**Amended 2026-08-20** (flywheel turn 3, merge-time append): the flywheel
turn-2 section's **two named fast-follows** — the refusal validator's
structural check-first assertion, and all-`files` contamination screening —
are delivered and **struck through in place** in that section, in the
2026-08-15 item-7 convention (struck where they were recorded, not moved), each
with what closed it. Turn 3's own section is added below with its settled
rulings, its deferred minors, and its process lessons. Nothing else in this
file is touched.

**Amended 2026-08-21** (flywheel turn 4, merge-time append): one turn-3
deferred one-liner — the ~999 zero-byte `.lock` files a full corpus run left
behind — is delivered and **struck through in place** in that section (2026-08-15
item-7 convention), with the verify-after-acquire protocol that closed it and
the companion clause that still stands. A **second** turn-3 carry closed this
turn, the *sibling-filename* contamination rule, was never given a bullet in
this file — it lived only in the turn-3 SDD ledger as an out-of-scope carry —
so it is recorded as delivered in turn 4's own section below rather than
struck somewhere it was never written. Turn 4's section is added below with
its settled rulings, its deferred minors, and its process lessons. Nothing
else in this file is touched.

**Amended 2026-08-31** (`agent-delete-endpoint`, the first carried-debt
slice): this file had gone stale at its own tail. Four items in the
*OpenAI tools adapter — live acceptance* section were written at 09:15 on
2026-08-31 and were overtaken by commits landed later the same day, so the
file's newest section was also its least accurate — the failure mode a
durable debt list exists to prevent. Items **1** (retry misclassified as a
history rewrite) and **4** (no parse-rate statistic) are struck through in
place with what closed them; item **2** (the one-context tier) gains a note
on what changed around it without closing it; item **3** (no
`DELETE /agents/{id}`) is struck on arrival, delivered by this slice. Each
strike was verified against the code and the committed tests, not inferred
from a commit subject.

Two further corrections in the same pass. The withdrawn streaming non-goal
and the buffered SSE that followed it are recorded in that section, since
they are the other half of the same day's arc and appear nowhere else in
this file. And the *Smaller items* file-size entry — already amended once on
2026-08-19, from "only file over the ceiling" to eight — is amended a second
time in the same accumulating style: it is now **20**. Nothing is deleted;
one item is added, recorded below as the endpoint's own carry.

**Amended 2026-09-01** (pager-lock spike, read-only): the `/v1` section's
"pager lock is held across `infer`" item gains the findings of a spike run
*before* committing to the slice it names, because the spike narrowed the
slice sharply and two of its conclusions are negative results that would
otherwise be re-derived expensively. The item stays **open** — nothing is
struck — but it now carries what the fix can and cannot be. Recorded here
rather than in an evidence doc because it is a change to a carried item's
scope, which is what this file is for. No code changed.

**Amended 2026-09-01** (slice C, branch `refusal-max-placeable`): **item 7's
"third half" is half delivered, and the fix this file prescribed for it is
withdrawn.** The item makes two complaints — the window law is blind to a
resident sibling, and a refusal leaves no recovery — which turned out to want
different fixes. The recovery half is closed (`max_placeable_tokens` on every
residency refusal); the blindness half stays open, its regression test
passing verbatim. The prescription (grow `GeometryInput` with sibling terms)
is withdrawn with reasons, and the placement-time downsize designed to replace
it is recorded as REJECTED with the five defects a four-lens adversarial
review found — including a CRITICAL that destroyed suspended agents' KV
images. Both rejections are written down in full at the item, because an
unrecorded rejected design gets proposed again.

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

- ~~**First fast-follow:** `validate_refusal_task` lacks a structural
  assertion that refusal goals end with the check-first instruction
  (the patch-side validator has the `DONE_INSTRUCTION` analog). Today
  every template routes through `goal_phrasing` which appends it by
  construction; a turn-3 template could silently drop it.~~
  **DELIVERED in flywheel turn 3 (2026-08-20, Task 4, commit `1f0b8f0`)** —
  `CHECK_INSTRUCTION` is canonical in `tools/flywheel/factory/task.py` and the
  refusal validator now asserts structurally that the goal ends with it
  (`task.py:360-361`), so the turn-3 templates this bullet worried about are
  covered by construction rather than by routing discipline.
- ~~Contamination guard screens only `target_contents` per task; a
  missing-target task's *sibling* file (name/contents) is screened by
  neither sampler nor CLI. Exposure nil today (sibling content never
  enters a rendered pair); fast-follow: screen all `task.files`.~~
  **DELIVERED in flywheel turn 3 (2026-08-20, Task 4, commit `1f0b8f0`)** —
  `_violations_for_task` takes the whole `files` map, and both callers (the
  draw-time sampler and the post-hoc CLI) screen through that one
  implementation (`tools/flywheel/factory/contamination.py`). Note what the
  "exposure nil today" clause was resting on: turn 3 is the first turn with
  **multi-file** tasks, so the sibling exposure would have become real the
  moment the find templates landed. This was a correctness precondition for
  the rest of turn 3, not hygiene — which is why "Task 4 before Task 7" was a
  preflight finding rather than a preference.
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

## Delivered in drift-watch (2026-08-17, `feat/drift-watch` branch)

**Settled (standing rulings for this slice — do not re-litigate without a
recorded amendment):**

- **No slug.** The plan's "reuse POST's slug rule" clause was corrected at
  pre-flight: no slug helper exists. `ProfileStore` adopts POST's existing
  raw-name convention (`{model}.json`, `{model}.previous.json`,
  `{model}.baseline.json`, `{model}.transient-{sha8}.json`). A `/` in a model
  key would already break POST's own paths today — a **pre-existing
  POST-breaking constraint, explicitly carried as debt and NOT fixed in this
  wave**; exposure is filename collisions for exotic keys, exactly the exposure
  POST already has.
- **Drift rows carry full byte-shas as identity claims.** `reference_sha` /
  `current_sha` are the full 64-hex sha256 of each file's bytes at comparison
  time, beside the paths — the same claim `Blessed`'s `sha` makes, **not**
  measurement numbers, so spec §4's no-transcribed-numbers law stands unamended
  and a drift-step row is byte-verifiable with `sha256sum`.
- **A confirm row spells the settled verdict**, never the raw re-diff outcome:
  `confirmed` / `transient` / `unconfirmed: <named re-diff outcome>`, one row
  per confirm and never a third row per comparison. Carrying the raw word would
  make a confirmed regression read `drift` and a *transient* — a finding in its
  own right — read `within-noise`, the same word a clean boot gets.
- **Auto-bless runs after both comparisons**, never before, so a model's first
  boot has its cumulative comparison honestly recorded as unmeasured rather
  than silently compared against the document that is about to become its own
  baseline.
- **Provenance is a prefix-family, not a closed set** — settled at 254ddb9 in
  both authoritative sites (core `journal.rs`'s `Event::Blessed` schema doc and
  `journal_blessed`'s own doc). Two spec-text divergences carry **dated,
  non-silent footnotes** rather than silent edits: §5's content-addressing is
  satisfied by the journal's sha fields, not by filename prefixes, and §2's
  auto-bless spelling is settled as `auto-first-profile`.
- **Confirms are per comparison and independent** — a boot where step *and*
  cumulative both read drift spends two confirm probes, not one shared
  re-probe (§4 describes a confirm per comparison, and a boot where both
  references disagree is the boot worth the second measurement). Operator
  consequence, recorded in the Task 6 evidence doc's operator notes: worst
  case ≈ N × `assay.probe_timeout_secs` added to the provisional-admission
  window, N being the comparisons that read drift across all models.

**Deferred from this slice (final-review triage: all defer-sound), by task:**

*Task 1 — instrument precheck:*

- Fixtures live in the daemon crate while the parser lives in core (cross-crate
  `include_str!`) — settle the home before more consumers arrive.
- Fixture consts and the provenance prose are duplicated across two files.
- `without_probe_version` is line-oriented — a latent trap on compact JSON.
- `InstrumentPrecheck` could derive `Eq`, and wants `Display` for Task 3's
  journal row.
- The fixture helpers' doc generality is overstated: `with_probe_version` /
  `with_schema_version` say "a v8 fixture's", but hardcode the v8 literal as
  their search pattern (`profile_test.rs`).

*Task 2 — `ProfileStore`:*

- `retain_transient` can prune the file it just retained — theoretical today
  (rename preserves mtime, an incidental invariant, not an enforced one).
- A partial prune drops the accumulated `dropped` record via `?`.
- Orphan `.tmp` files (an atomic write whose rename never happened) are never
  swept.
- `rotate` / `retain_transient` return bare `io::Result` rather than
  `DriftError`'s own context argument.
- Nothing type-enforces the provenance vocabulary — the journal field is a bare
  `String`. The ledger's original "two-value set unenforced, want a pub const
  pair" is **partly superseded**: the const pair shipped
  (`PROVENANCE_AUTO_FIRST` / `PROVENANCE_OPERATOR`), and the two-value framing
  itself was replaced by the prefix-family contract settled at 254ddb9, under
  which `operator (replaced <sha>)` is a legitimate third spelling. What
  remains open is only the unenforced-`String` half.
- The mtime-tie test is coupled to `profile_doc`'s hash ordering — latent;
  comment it if the template changes.

*Task 3 — the gate:*

- `if let` on `InstrumentPrecheck` falls through to the spawn (an exhaustive
  `match` costs one line).
- `timeout()` reads the field rather than the spawn's own cap — close it via a
  shared local.
- `compare`'s doc overclaims "before anything else could touch".
- `NotComparable` literal repetition.
- `diff_argv` indirection style: a `[&str; 6]` built from `&String` temporaries
  and then mapped to owned `String`s, where a plain vec of owned `String`s
  would say the same thing directly.

*Task 4 — boot wiring and confirms:*

- The failed-bless and retention-failure `Degraded` branches are untested.
- `Infra` folded into `Unmeasured` needs string-sniffing to separate again —
  the enforcement slice wants the two apart.
- Unbounded assay stderr rides onto `/status`.
- ~~`journal.rs`'s `Event::Drift` schema doc is stale ("two rows per boot").~~
  **CLOSED at the final fix wave (2026-08-17)** — rewritten as the real row
  family: two first-reading rows per model per boot plus at most one confirm
  row per comparison that read `drift`, with both outcome vocabularies stated
  and read by prefix.

*Task 5 — the bless route:*

- `BlessError::Journal` is machine-indistinguishable from nothing-happened; a
  baseline replaced but unrecorded deserves its own code.
- The bless route races `auto_bless` during the POST window — double `Blessed`
  rows with different provenance, i.e. provenance ambiguity in that live
  window; documented in the Task 6 evidence doc's operator notes.
- An unreadable old baseline puts free text in the digest slot; a
  prefix-distinguishable shape would be better.
- The bless handler's `let e = match … { Ok => return, Err(e) => e }` followed
  by a second `match` departs the file's idiom (`api_native.rs`); extracting a
  `map_bless_error` beside `map_error` would restore it.

**Assay-side carry (flagged for the assay repo's own debt / v1.8 — not bloomery
work):**

- `assay 0.9.0 diff --gate` exits 0 ("no drift beyond noise") on a v8-vs-v4
  pair while **five families vanish** (long_output, tool_calling, three json
  cells) — literally true under its own rules, consumer-dangerous. Bloomery's
  §3 instrument precheck is the only guard. Measured live, boot 3.
- diff prose falsely reports `dropped: verdict.long_context` on objects that
  are equal (prose bug; bloomery never parses prose).
- assay 0.5.0 has no `diff` subcommand at all — argparse exit 2 masquerades as
  not-comparable, reachable only behind a precheck that already passed.

**Process lessons:** the plan template omitted the merge-time CARRIED-DEBT
append — this very entry is the gap, dispatched with the final fix wave rather
than planned; add the append as a standing template task. Review-and-fix rounds
caught **nine Importants the suites had passed**, among them a crossed-model
pair journaling one model's row over another model's document, a failed confirm
probe leaving zero durable trace (a 600 s dead probe that vanished with the
process), confirm rows spelling raw re-diff verdicts, and production `run_post`
wiring left unguarded — a revert-to-pre-drift mutant survived the whole suite.
Live acceptance's chief yield was the assay-side erratum above, **not** a
bloomery defect: all three boots read the spec-pinned outcomes first try.

## Delivered in verdict-gated-admission (2026-08-18, `feat/verdict-gated-admission` branch)

The capability-vector seam's slice 2. Where drift-watch (above) measured and
recorded; this slice is the first to act on the measurement.

**Settled (standing rulings for this slice — do not re-litigate without a
recorded amendment):**

- **Only a confirmed cumulative regression refuses admission.** `admit()`
  gained one clause, checked before `has_profile` short-circuits (load-bearing
  for correctness, not just message honesty — reversing the order would
  silently admit a profiled-and-blocked model). All seven `DriftStatus`
  outcomes are enumerated against admit/refuse, one assertion per row, so a
  sampled subset can never stand in for the policy
  (`only_a_confirmed_cumulative_reading_blocks_admission`,
  `crates/bloomery-daemon/tests/pager_test.rs`). The rule an eighth outcome
  inherits: refuse only what was established; name everything else.
- **Cumulative blocks; `step` never does, in either direction.** A `step:
  Confirmed` / `cumulative: WithinNoise` reading admits
  (`a_confirmed_step_reading_alone_does_not_block`) and a `step: WithinNoise`
  / `cumulative: Confirmed` reading refuses
  (`a_confirmed_cumulative_reading_blocks_even_when_step_is_clean`) — the
  asymmetry slice 1 named ("step alone leaks the ratchet," because step's
  reference auto-advances every boot and would clear a persisting regression
  on its own).
- **The reading and the block are separate fields, on purpose.** `drift` is
  written once, when the watch settles it, and is never rewritten by an
  operator action; `admission_block: Option<AdmissionBlock>` is the policy
  derived from it at that moment, and a policy is the operator's to override.
  `unblock` clears the block and leaves the reading exactly as measured
  (`unblock_admits_and_leaves_the_reading_alone`, and the HTTP-level
  `unblocking_a_blocked_model_admits_and_journals_the_operator` in
  `tests/api_native_test.rs`, both re-read `drift.cumulative` after clearing
  and assert it unchanged).
- **Two operator routes, deliberately independent.** `POST
  /models/{name}/bless` is byte-for-byte unchanged and still only re-baselines
  the *next* boot's cumulative reference. `POST /models/{name}/unblock` is new
  and only clears *this* boot's block (200 cleared / 404 unknown model / 409
  no block to clear, the 409 load-bearing for the same reason bless's is —
  a silent 200 would tell an operator they cleared something that was never
  set). Neither implies the other, pinned at the pager layer
  (`unblock_does_not_rebaseline_and_bless_does_not_unblock`) ~~and over HTTP
  (`unblock_does_not_bless_and_bless_does_not_unblock_over_http`)~~. ~~Every
  `Event::Admission` row carries its own provenance —
  `PROVENANCE_DRIFT_WATCH` ("drift-watch") on a "blocked" row,
  `PROVENANCE_OPERATOR` ("operator") on a "cleared" row — so a replay can say
  who decided~~, same discipline as `Event::Blessed`'s provenance family.
  **Both struck claims overclaimed what was actually pinned, mutation-proven
  at the final whole-branch review and CORRECTED at the final fix wave
  (2026-08-18):** the HTTP test ran against `serve_with_profiles`'s
  always-unblocked fixture, so "bless does not unblock" was unobservable
  over HTTP at all — a mutation making `bless_baseline` silently `.take()`
  the block left it green while the pager-level test above went red. It now
  runs against `serve_drift_blocked_qwen_with_profiles`, a model that IS
  blocked, and asserts the block survives a bless (`/status`'s
  `admission_block`, and a 422 refusal on `POST /agents`) before checking
  that the unblock which follows leaves the baseline bless just wrote
  untouched; the same mutation now fails it. Separately, only the "cleared"
  row's provenance was actually asserted
  (`clear_admission_block_journals_a_cleared_row_with_operator_provenance`);
  the "blocked" row's was not — a mutation swapping `PROVENANCE_DRIFT_WATCH`
  for `PROVENANCE_OPERATOR` at its one call site (`pager/drift_watch.rs`,
  `set_drift`) left the whole suite green.
  `set_drift_journals_a_blocked_row_when_it_newly_blocks` now asserts the
  "blocked" row's provenance too, and the same mutation now fails it. Both
  properties are true of the shipped code and are now actually pinned by
  test, both mutation-verified.
- **`PagerError::DriftBlocked` renders 422 on both HTTP surfaces**, matching
  `Unprofiled`'s status (same class of answer: this model cannot be admitted
  now) rather than bless's 409. Both `map_error` functions are exhaustive
  matches with no wildcard arm, so a variant handled on one surface and not
  the other fails to compile rather than 500ing in production — stronger than
  the review just asking for two tests.

**Carried forward, still open:** slice 1's Task 4 debt that `Infra` folds into
`Unmeasured` and needs string-sniffing to separate again, which that entry
already flagged as wanted by "the enforcement slice." This is that slice, and
it plainly did **not** need them apart — the refusal table above blocks on
`Confirmed` alone, and both `Infra` and every other `Unmeasured` reason fold
into the same admitting row. Unchanged and unstruck; a verdict-floors slice
(design §8's other named candidate) will still want the two apart, because a
floor keyed to measured capability cares whether "unmeasured" means
"infrastructure failed" or "nothing to compare."

**New this slice:**

- **`verdict.parallel` and assay v1.8's exit 3 exist and are deliberately not
  consumed.** Verdict floors (admitting on measured capability rather than on
  drift) is a separate, later slice with its own spec (design §8); reading any
  assay verdict beyond the drift comparison, or handling exit 3
  ("incomplete comparison," unreachable today behind bloomery's own version
  precheck), is explicitly out of scope here and not a gap in this slice's own
  job.
- **The block is per-model; there is no fleet-wide override.** Considered and
  rejected in design: `allow_unprofiled`'s all-or-nothing shape (one flag,
  every unprofiled model admitted) was the obvious precedent to reuse, but a
  fleet-wide drift override would let one operator action silently readmit
  every regressed model at once, including ones nobody has actually looked
  at — the opposite of what a *confirmed, reproduced* regression should cost.
  `unblock` stays scoped to one model, one decision, one journal row.
- **The assay-pin upgrade note.** The daemon is pinned by `PYTHONPATH` to
  assay's source tree (not a released version), so when assay v1.8 merges
  (0.10.0, schema v9), the daemon starts producing v9 profiles the moment
  that lands while every blessed reference still reads `0.9.0/v8`.
  `instrument_precheck` compares both `probe_version` and `schema_version`, so
  the first boot after the merge reads `InstrumentChanged` for **every**
  model at once against its blessed v8 reference. That is exactly why
  `InstrumentChanged` admits in the refusal table above — under enforcement,
  a routine instrument upgrade must not read as a fleet-wide regression — and
  it is no longer just an intention: `an_instrument_change_never_blocks_the_fleet`
  (`tests/pager_test.rs`) pins the literal `0.9.0/v8` → `0.10.0/v9` transition
  and asserts no block.
- **Found, not fixed: a test's name overclaims what it checks.** Task 1's
  `a_model_with_no_admission_block_renders_none` (`tests/pager_test.rs`)
  asserts the Rust-level `model.admission_block.is_none()`, not the actual
  JSON rendering its name promises. The claim itself was verified — Task 1's
  review independently confirmed `None` serializes as JSON `null` rather than
  vanishing, matching `drift`'s existing field — but the test does not pin
  that rendering itself, so a future regression in the `Option` serialization
  path would not be caught by this test. Flagged during the wave (routed to
  final-review triage) and left as-is rather than opening a fix round on
  completed, reviewed work; a follow-up test asserting the JSON body directly
  (the pattern `a_model_with_no_admission_block_renders_none`'s sibling
  `drift`-field tests already use) would close it.

**Recorded at the final whole-branch review's fix wave (2026-08-18) — RECORD,
not fix (all four are behavior findings or hazards, not defects the wave's
tests caught):**

- **R1 — the G4/G5 probes abort for a blocked model, and `unblock` does not
  recover them.** `codec_probe/mod.rs:332` and `codec_probe/refuse.rs:211`
  both call `create_agent`, which this slice taught to return
  `PagerError::DriftBlocked` for a blocked model; both map that error to
  `abort(...)`, and `codec_probe/boot.rs:110-113` journals a `Degraded`
  naming the model. So on a boot where a block appears before that model's
  G4 or G5 probe runs, `codec_gate` stays `None` and `mutating_verbs` stays
  `false` (fail-closed) for the whole boot — and because both probes are
  boot-time only, strictly after `run_post` (`main.rs:278-301`), clearing
  the block via `unblock` restores admission but does **not** recover the
  G4/G5 gates: they stay unmeasured until the next boot re-probes from
  nothing. Spec §5's "`done_trust`, `codec_gate` and the G4/G5 gates are
  untouched" is true of the FIELDS (no new write path, no type change) and
  false of the MEASUREMENT (a block can silently prevent one from ever
  completing this boot). Direction is fail-closed and the abort is
  journalled, so this is a recorded behaviour change, not a correctness
  bug — a post-`unblock` re-probe is a candidate for a later slice. **Spec
  §5 amended** (`docs/superpowers/specs/2026-08-18-verdict-gated-admission-design.md`,
  a dated footnote in the drift-watch-spec convention, not a rewrite) to
  say this plainly: the sentence as written would mislead the next reader
  about what this wave changed.
- **R2 — `clear_admission_block` mutates before it journals, with no named
  outcome for a refused row.** `pager/drift_watch.rs:193` takes the block
  out of `entry.admission_block` before `:196-202` journals the "cleared"
  row. If that journal write fails, the operator gets a 500, the block is
  already gone in memory, and a retry answers `409 no_admission_block` —
  telling them they never had a block, when they did and only the row
  recording its clearance was refused. `bless_baseline` was built with
  `BlessError::Journal` ("the baseline was replaced but the journal refused
  the row") for exactly this class of hazard; `unblock` does not mirror it,
  so its documented 200/404/409 table (`api_native.rs`) has an unstated
  fourth outcome. `set_drift`'s own write-then-journal ordering
  (`drift_watch.rs:135-152`) carries the same hazard in the other
  direction: a journal failure there can leave a block standing in memory
  with no `"blocked"` row to explain it.
- **R3 — two silent-clear paths, unreachable today by call-site discipline
  alone.** `register_model` resets `admission_block: None` unconditionally
  on every (re-)registration (`pager.rs:511`), and `set_drift` assigns the
  derived block unconditionally on every call (`drift_watch.rs:140`) — so a
  second `set_drift` for one model in one boot with a non-`Confirmed`
  reading would silently drop a standing block with no `"cleared"` row to
  record it, and a `register_model` re-registration would do the same.
  Neither is reachable in production today: `register_model` has exactly
  one caller, before the socket binds (nothing could be blocked yet), and
  `watch_model` runs at most once per model per boot. The invariant "a
  block is never dropped without a journal row" is held by call-site
  discipline today, not by the type system or a test — a future caller of
  either function would not be warned.
- **R4 — erratum, spec §5's POST-window sentence.** Spec §5 says "The POST
  window is unaffected. No drift has settled while POST is still probing."
  That is false on a multi-model daemon: `set_drift` runs inside
  `probe_each`'s per-model loop (`post.rs:454`), one model at a time as
  each one's POST completes, while `set_posting(false)` runs only after the
  whole loop finishes (`post.rs:406-409`). So on a daemon POSTing several
  models, model A's cumulative comparison can settle `Confirmed` — and its
  block can land — while model B is still being probed and `/status` still
  reports `posting: true`. **The precedence is correct and the behaviour is
  what the design wants:** `admit()` checks the block before the existence
  gate (`pager.rs:646`), so a model that drifts mid-POST is refused the
  moment its own reading settles rather than held open until the whole
  boot's POST window closes. Only the spec sentence's factual claim — "no
  drift has settled while POST is still probing" — is untrue on a
  multi-model boot. Per house convention the committed spec is not
  rewritten for this; the erratum stands here with the corrected statement.

## Delivered in swap-candidate (2026-08-19, `swap-candidate-seam` branch)

The capability-vector seam's slice 3, and the first **advisory** one. Slice 1
measured, slice 2 acted; this slice answers a question — *is candidate Y
admissible as a substitute for model X?* — and changes nothing about what the
daemon will serve. `POST`/`GET /models/{name}/swap-candidate` register a
candidate GGUF under a scratch identity, probe it through this daemon's own
`/v1`, and cover its profile against `{name}`'s **blessed** baseline via
`assay cover`'s four exit codes.

**Settled (standing rulings for this slice — do not re-litigate without a
recorded amendment):**

- **The scratch identity's uniqueness is an assumption, named rather than
  guarded.** A candidate is probed as `{model}!swap-candidate`
  (`SCRATCH_SUFFIX`, `crates/bloomery-daemon/src/swap.rs`), and it lives in the
  same registry as the operator's configured models for the length of one job
  — it has to, because assay reaches it through `/v1` by name. The `!` is what
  keeps it out of the operator's namespace: model names are TOML table keys,
  and a *bare* key is `[A-Za-z0-9_-]` only, so no bare key can collide. A
  **quoted** key may contain anything, `!` included, so an operator who
  deliberately writes `"llama!swap-candidate"` as a model name collides with
  the scratch identity of a model called `llama` — and **nothing refuses that
  today**. Deliberate: a guard would trade a real line of code for a
  configuration nobody writes, and the collision is visible the moment it
  happens (`/status` lists both names). Recorded here because "no configured
  model can hold this name" is an assumption about operator input, not an
  enforced invariant, and the next reader should not have to re-derive that.
- **One slot, daemon-wide — there is no per-model concurrency.** `SwapContext`
  holds exactly one `SwapSlot`, shared by every HTTP worker
  (`crates/bloomery-daemon/src/swap/context.rs`), so a candidate probe for
  model A blocks a candidate probe for model B with `409
  candidate_probe_in_progress`, and a `GET` for B while A's job holds the slot
  is a 404 rather than an answer about the wrong model. Design §4's "one
  candidate at a time … no queue", taken literally. This is the deliberate
  choice, not an oversight: a probe holds VRAM for ~10 minutes, and two
  concurrent probes on this box would contend for exactly the residency the
  probe is measuring. **Revisit only with evidence** — a second slot is only
  worth building on a box whose budget actually admits two candidates at once,
  and nothing has measured that.
- **The verdict is advisory, and the advisory gap is the whole shape of the
  next slice.** Nothing blocks and nothing auto-swaps (design §4, §6's first
  non-goal): an operator who gets `not-covered`, `incomplete` or `refused` can
  still edit `bloomery.toml`, restart, and serve the candidate — the daemon
  will not know a verdict was ever asked for, let alone what it said. The
  journal row (`Event::SwapCandidate`) carries the candidate GGUF's full-file
  digest, so a *replay* can tell that the served weights were the ones a
  verdict refused; nothing checks it at boot. Enforcement — refusing to serve a
  swapped GGUF with no admissible verdict on record — is the named future
  slice, following the slice-1-then-2 pattern this seam has used twice already:
  measure first, enforce once the measurement has lived.

**Spec amendments recorded this slice** (dated notes in the drift-watch
convention, originals preserved,
`docs/superpowers/specs/2026-08-19-swap-candidate-seam-design.md`): §3's
identity bullet takes two — assay v1.11's strict-instrument-equality ruling,
and the two assay review rulings that postdate it (both-sides-absent
`tier`/`emulated` is fatal; the semantic-break registry check is a **live**
refusal route via an equal-but-unparseable `probe_version`, not the
defense-in-depth the first note called it). §4 step 1 records the **409
disposition** — the spec's "Unplaceable → 409 with the bytes needed, free, and
reclaimable" is not honestly implementable at POST time and is not
implemented, because `PagerError::Refused` exists only inside the private
`Pager::place`, keyed on an agent already in the table with a window-sized
demand term that does not exist at request time; a real residency refusal
surfaces through the probe's own failure instead (`Degraded` row, `infra:`
report). §4's journal prose records `Refused { exit, stderr }` — carried as
operator detail and never consulted for the verdict, because exit 2 is also
what `argparse` answers for `invalid choice: 'cover'` — and §4's response
prose records the asynchronous 202/GET shape (a probe cannot ride a request
handler; the boot watch's own rule). §7's "pager refusal … keeps the
surface's existing 404/409 idiom" clause takes another dated note for the same
reason as §4 step 1's, and names the two shapes §7 omitted (400
`bad_request`, 501 `swap_candidate_unavailable`).

**Recorded, not fixed:**

- **`api_native.rs` is at 697/800 lines — the next route added there forces a
  split.** It was 393 before this slice and gained 304 (the two swap-candidate
  handlers, the spawn site with its `catch_unwind`, and their doc tables), a
  77% jump against a ceiling with 103 lines of room left. Nothing is wrong with
  the file today; it simply cannot absorb another surface. `swap.rs` →
  `swap.rs` + `swap/job.rs` (this slice's own pure-move split, done for exactly
  this reason) is the shape the split should take: the routes and their error
  table in one file, a surface's handlers in their own module.
- **`api_native_test.rs` is at 1839 lines**, having gained 658 this slice by
  plan-mandated placement (the endpoint tests were specified to land beside the
  surface's other route tests). It was **already** at 1181 — over the 800
  ceiling before this slice touched it — and it is one of **eight** test files
  above that line, counted 2026-08-19: `drift_test.rs` 1980,
  `api_native_test.rs` 1839, `codec_probe_test.rs` 1559, `pager_test.rs` 1247,
  `swap_test.rs` 1021, `pager_weights_test.rs` 924, `api_task_test.rs` 923,
  `task_loop_test.rs` 830. So this is an accumulating project-wide condition
  rather than a defect this slice introduced, and the honest framing is that
  the 800-line ceiling is currently enforced on `src/` and not on `tests/`
  (two src files are over it — `drift.rs` 985 and `pager.rs` 912 — against
  eight test files). `swap_test.rs` shows the per-seam pattern that would
  absorb `api_native_test.rs`. Same class as the "`pager_test.rs` is 834
  lines" note under *Smaller items* below, which this entry supersedes on the
  facts (that file is now 1247, and it is not "the only file over the 800
  ceiling").
- **`std::thread::spawn` panic exposure on request/registry paths.** Both sites
  claim state before spawning (swap-slot at `api_native.rs:481`, Running status at
  `registry.rs:172/184`), and a spawn panic on OS thread exhaustion leaves
  claimed state unreleased. Switch both to `thread::Builder::spawn` with a
  finish/cleanup on `Err`.

**From the live-acceptance arc (added 2026-08-20, the post-slice-3
follow-ups):**

- ~~**Journal rows carry no time field** (bA2/F2). Found investigating a
  10,933 MiB VRAM dip during acceptance 2: the union of keys over the boot-4
  journal's 1132 rows held no time at all, and boot4.log carries no clock
  either, so no row could be wall-clock-correlated with anything outside the
  journal.~~
  **DELIVERED on arrival (the same commit that records this)** —
  `Journal::append` stamps every row with `epoch_ms`: milliseconds since the
  Unix epoch, the writer's own clock at append time, in the `_ms` naming the
  schema already uses for durations. A **row** property, never an `Event`
  field — it records when the writer wrote, not what happened — so `replay`
  returns events unchanged and the raw JSONL is the correlation surface.
  Journals written before the stamp keep replaying (pinned per committed
  journal by `committed_g2_journal_still_replays`), and a stamped line
  deserializes identically to its unstamped ancestor
  (`a_stamped_line_and_its_unstamped_ancestor_deserialize_to_the_same_event`;
  the stamp itself pinned, clock-bounded and mutation-checked, by
  `an_appended_row_carries_a_bounded_epoch_ms_stamp`, both `journal_test.rs`).
  Forward-only: the acceptance journals already committed stay stampless —
  nothing rewrites an append-only record. One proof pattern changes shape
  under the stamp, noted before anyone pre-registers it: acceptance 2's
  durable determinism proof was a byte-equal pair of `Degraded` rows, and two
  identical events appended at different instants now differ in `epoch_ms` —
  a future row-equality claim compares rows with the stamp stripped
  (`jq 'del(.epoch_ms)'`), and byte-identical *whole journals* across re-runs
  are impossible by construction.
- ~~**Unload-then-swap-candidate on tight tiers had no operator-facing
  line** (bA2/F1). Acceptance 2a measured the flow live: the pager charges
  every loaded model's weights to one budget and reclaims only agents' KV,
  so a candidate cannot fit beside a resident 14B on this tier — the probe's
  `/v1` requests are refused (`503 residency_refused`) and the job lands as
  an `infra:` report, not a verdict. The evidence doc recorded the flow;
  nothing operator-facing did.~~
  **DELIVERED same commit** — the README's swap-candidate bullet now carries
  the flow: `POST /models/{m}/unload` first, then the swap-candidate POST.
  (This is bT3/R1's disposition working as ruled — the residency refusal
  surfaced through the probe with the pager's real arithmetic. The fact
  acceptance 2 *added* is that on this tier the happy path is unreachable
  without the unload, which is exactly the line an operator needed written
  down.)

**Process lessons (the live-acceptance arc):** amendments to a
pre-registration are **separate files, never in-place edits** — acceptance 2
amended its prereg in place (gitignored, mtime overwritten), and the evidence
had to report both stamps with a caveat where a second file would have carried
its own proof. And a verify-sweep must enumerate **every file the session
edited**: the write-discard failure mode (edit passes that raise after logging
ok, silently discarding applied-but-unwritten fixes) struck twice across two
files and survived a correctly-built sweep pointed at only one of them —
scoped re-reviews caught it both times.

## Delivered in flywheel turn 3 (2026-08-20, `flywheel3-turn3` branch)

The flywheel's third turn: a `find`/`run` trajectory slice and a third refusal
family (symptom-mismatch), trained against the hole flywheel2 *measured* rather
than one anybody anticipated — its patch class failed the v3 floor by exactly
the size of the find-shaped slice, 0/6
(`docs/superpowers/evidence/2026-08-20-g5v3-baselines.md` §6).
**`qwen3-14b-flywheel3` passed the full pre-registered battery**: G4 20/20,
G5-v3 patch 15/16 (floor pass, provisional) and refuse 16/16 (floor pass,
decided), `done_trust: true` — the first done-trust mark at n=16 per class.
Find-shaped patch went 0/6 → 5/6 and the pre-registered *productive-find*
endpoint read 5/6 against 0/6 for both baselines
(`docs/superpowers/evidence/2026-08-20-flywheel3-battery.md`). The `run`
trajectory, a third of the same repair slice, showed **zero** observable
transfer — recorded there as a null result on a trained behaviour.

**Settled (standing rulings for this slice — do not re-litigate without a
recorded amendment):**

- **The flywheel tool's scratch root is named by a content hash of the
  request's identity, and held under an exclusive `flock` for the directory's
  lifetime** (rulings bT7/R1 and bT7/R2,
  `crates/bloomery-daemon/src/bin/flywheel_tool/scratch.rs`, commits `474b565`
  + `fe231e1`). The find shape's `exec_find` embeds the canonicalized scratch
  path in its observation, so a PID-carrying temp dir made two same-seed
  real-binary runs differ in exactly 999 of 4,263 rows — the determinism law
  the corpus rests on, broken silently. **The two rejected fixes are the
  load-bearing half of the ruling:** relativizing the path in `render`, and
  rewriting the rows factory-side, both post-process real executor output, so
  the trained text would stop being what the tool actually rendered.
  Instrument honesty outranks a smaller diff. The `flock` is a *measured*
  correction to the ruling's own assumption — it supposed the
  concurrent-identical-request edge merely needed documenting, and a parallel
  `cargo test` collided 3-in-5 with silently-wrong observations.
- **`FIND_PATTERN_LITERAL_RE` is a structural validator rule, not a per-family
  test** (commit `7abcab7`). Find-slice patterns must match
  `\A[A-Za-z0-9_ ]+\Z`, asserted in `_find_shape_violations`, so every
  *future* find family inherits it by construction instead of by an author
  remembering. Three mutations killed; the round's strongest signal was the
  new rule catching a deliberately bogus test.
- **`codec-tasks-v3-mixed` is frozen at `e6c7637` — 32 fixtures (16 patch + 16
  refuse) — with a diversity rule of its own** (`codec_fixtures_v3_diversity_test.rs`).
  It was audited fixture-by-fixture from its own frozen bytes (goal arithmetic
  recomputed, `py_compile` run on re-derived patched bytes, 24/24 quoted spans
  checked against the files) and is never amended after a number has been
  seen — the same law `codec-tasks-v1` and `codec-tasks-v2-mixed` live under.
  Two honesty properties of the frozen set are **ruled to stand as frozen and
  named in every evidence write-up** rather than fixed: the defect-absent
  family is **3 hard-decidable / 3 soft** (its per-family number is never six
  equivalent trials), and `v3-refuse-defect-absent-py-01`'s `refusal_reason`
  cites a calibration sheet absent from the workspace (costing no measurement
  accuracy — the reason is never compared to model output and never scored).
- **Productive find is a pre-registered secondary endpoint, and it exists
  because raw find-usage was *measured* unfit — in both directions.** Stock
  scores 6/6 on raw usage with no find training at all (every find-shaped goal
  carries an explicit search instruction) and lands 0/6, so the endpoint is at
  ceiling for an untrained model; and flywheel2's malformed finds never become
  `find` steps (they journal as `verb: "?"` with
  `MissingAttr { verb: "find", attr: "path" }`), so a model learning **only
  the wire format** moves the raw count 2 → 6 with zero productive gain.
  Productive find — a well-formed `find` **and** the fixture landing — survives
  both confounds and was 0/6 for both baselines, so any nonzero value is new.
  It is **never** kill material and never a floor, and **nothing in the daemon
  computes it**: it is the measurer's obligation, from the committed rows.

**Flywheel turn 2's two named fast-follows were delivered this slice** and are
struck through in place in that section above (Task 4, commit `1f0b8f0`): the
refusal validator's structural check-first assertion, and all-`files`
contamination screening. The second was a **correctness precondition** for the
rest of the turn, not hygiene — turn 3 is the first turn with multi-file tasks,
so the sibling exposure its original text called "nil today" would have gone
live the moment the find templates landed.

**Deferred from this slice (final-review triage: all defer-sound). One line
each; per-task detail lives in the SDD ledger at
`.superpowers/sdd/2026-08-20-flywheel3-turn3/progress.md`:**

- A mistyped `commands` TOML key parses silently empty — no
  `deny_unknown_fields`, no required-iff rule; the net is the v3 structural
  test pinning the exact argv prefix on all five run-granted fixtures.
- `registry.rs:312` still `format!`-interpolates capability-grant JSON
  unescaped (the `flywheel_tool.rs` twin was closed in Task 6).
- Grant tests pass a scratch dir that never exists (documented, harmless
  today); two bare assertions carry no failure message.
- `shipped_fixture_set_v2_mixed()` has no production caller now that v3 is the
  live mixed set.
- The no-duplicate-violation-rows property is unpinned (one assertion on the
  two-file planted row closes it); `test_contamination_g5`'s `_corpus_row` is
  legacy-shaped by accident and wants a deliberate-legacy comment so the
  fallback coverage is not "fixed" away; the canonical-object identity pin was
  never observed RED (an `ImportError` masked it — structurally unavoidable,
  recorded).
- `files`/`target_contents` equality can false-positive above `read_cap_bytes`,
  producing a dishonest message in that latent case; two mutation-ledger rows
  do not reconcile (their counts are not quotable downstream; the pins do
  bite).
- Four-way fixture-helper duplication in the tool's tests wants a
  `tests/common` module; `files_to_materialize` deep-clones.
- `test_every_slot_gets_a_family_from_its_own_shape_registry` is name-vacuous
  (the property is covered elsewhere); the plain request's field omission is
  unpinned and the `found 1 matches` precondition unenforced.
- ~~A full corpus run leaves ~999 zero-byte `.lock` files in the temp dir;~~
  **DELIVERED in flywheel turn 4 (2026-08-21, Task 2, commit `c9221a1`)** —
  `sweep_lock` now unlinks the lock file at teardown under a
  **verify-after-acquire** protocol (`flywheel_tool/scratch.rs:225-233`):
  acquire re-checks that the name still resolves to the inode it locked, and
  release unlinks only while still holding the lock, so a concurrent holder's
  file is never swept out from under it. Two unit tests pin both halves
  (`dropping_a_scratch_removes_the_directory_and_sweeps_its_lock_file`,
  `the_sweep_leaves_a_lock_file_a_concurrent_holder_still_owns`). The
  companion clause still stands, unstruck: the
  scratch digest is truncated to 64 bits (hygiene-only, reasoned in code).
- The v3 diversity normalizer's second pass is logically inert (`drop=true`
  subsumes it — the test is not weakened, the doc misleads); the
  missing-target byte-valid branch is weaker than its doc (an empty-contents
  sibling would pass) — amendment-drift insurance.
- `v3-patch-find-txt-03`'s goal noun ("sheet") narrows its target once the
  directory is listed — the weakest of the six find-shaped fixtures, and it
  still requires a find.
- Test-file line-cap pressure continues: two turn-3 factory modules sit at
  exactly the 400-line cap.
- Style/wording residue: a rows-and-len wart, a seed-test docstring that
  disagrees with its loop, and the inherited G5 protocol §4 n-specific floor
  spelling (ledgered, no action).

**Process lessons:**

- **The determinism break was found by measurement, not by review.** Multiple
  reviewers and a green suite had already been over the find slice; what found
  it was a test that actually ran the *real* binary twice on the same seed and
  diffed the rows. Reviews check that code says what its author meant; only a
  measurement checks that the world agrees. The rule for the next factory:
  pin determinism against the real tool, never against a stub.
- **The fix wave introduced its own wrong citation.** The final-review fix wave
  rewrote the README's determinism section correctly and, in the same commit,
  cited `474b565` for a change that landed in `cbe5886` — a fresh falsehood
  created by the act of removing an old one. The scoped re-review caught it
  only because it verified the *replacement's* accuracy rather than merely
  confirming the original claim was gone. A fix wave is a change like any
  other and carries the same burden of proof; "we were only fixing a doc" is
  not a weaker standard.
- **Anatomy claims need the same script treatment as counts.** The turn-3
  baselines doc had every headline verdict reproduced mechanically and still
  shipped five wrong *anatomy* sentences — "reads the same file six times"
  (it never obtained content at all), "3 grant violations" (61 rows across 18
  fixtures), a 6/6 fabrication claim that was really 3 + 2 + 1. Verdicts get
  recomputed because they are obviously numbers; prose about *why* a model
  missed is just as much a claim about the journal, and just as recomputable.
  When two recomputations disagreed, the resolution was a third mechanical
  recount from the committed JSONL, not an argument — and the implementer was
  right both times.
- **A live measurement's boot conditions are part of its record.** flywheel3's
  boots ran with ~585 MiB more desktop VRAM in use than the baselines', which
  moved the computed serving window and dropped the measured assay ceiling one
  rung. It does not touch the fixture-scale codec verdict, and it is written
  into the evidence anyway — because the alternative is a future reader
  comparing ceilings across three documents and inferring a model difference
  that is really a fact about the box.

## Delivered in flywheel turn 4 (2026-08-21, `flywheel4-turn4` branch, merged as PR #18)

The flywheel's fourth turn: **envelope-v4** — a prompt that renders the grant
line from the real `Grant` the task loop enforces — plus a corpus regenerated
under it whose `run` slice is trained on the *granted* argv
(`python3 -m unittest test_<stem>.py`) against a planted test proved to fail
before the patch and pass after, and a fourth frozen gate set
(`codec-tasks-v4-mixed`, 32 fixtures) authored to be the net for surface-cue
learning. It was aimed at a hole the turn-4 baselines *measured* rather than
one anybody anticipated: under envelope-v4 the incumbent emitted `run` on 5/5
run-granted fixtures, in the right shape, only where granted — and ran the
command it was trained on rather than the one the prompt granted, so all five
were refused at the grant check and **productive run was 0/5**
(`docs/superpowers/evidence/2026-08-21-g5v4-baselines.md` §5.4).

**`qwen3-14b-flywheel4` passed the full pre-registered battery**: G4 **20/20**
(the kill leg), G5-v4 patch **16/16** and refuse **16/16** — both floor passes
and both **decided** — `done_trust: true`, and **productive run 5/5** against
a measured 0/5 for both envelope-v4 anchors, with the planted test and the
patched target both compiled to bytecode after the patch on all five fixtures
(`docs/superpowers/evidence/2026-08-21-flywheel4-battery.md` §5.4). Productive
find read 6/6, and the two boots produced **zero grant-violation rows and zero
parse failures** across 72 fixture runs. Every comparison in that document is
against the two envelope-v4 anchors only; turn-3 numbers are prior records
under a different prompt and a different fixture set, and no delta against
them is written anywhere.

**Settled (standing rulings for this slice — do not re-litigate without a
recorded amendment):**

- **`action_stop` inherits for V4** (ruling bT2/R1). envelope-v4 is
  envelope-v3 *plus* the grant line and nothing else moves, so the stop
  sequence is inherited rather than re-declared. The byte-identity law is what
  makes this checkable: v1/v2/v3 goldens were captured as genuine literals
  **before** the V4 branch existed and stay GREEN after it, and deleting the
  grant line kills 12 tests while leaving the goldens untouched.
- **A multi-prefix grant renders the label once per prefix line** (ruling
  bT2/R2) — the accepted reading of "one line per prefix". Only the
  single-prefix form shipped in any corpus or gate this turn; the multi-prefix
  rendering is pinned by test, not exercised by data.
- **A demoted model never sees a grant** (Task 2 fix round 1). When
  `mutating_verbs` is false the v4 renderer emits the `none` line, so a model
  the G4 gate demoted cannot be told that `run` is available. Verified through
  the real `run_task` path, not through the renderer alone.
- **The `unittest` timing line rides in trained text as REAL executor output**
  (ruling bT4/R1). Every one of the corpus's 333 run renderings carries the
  planted test's own stdout, `Ran 1 test in 0.000s` included. A test that ever
  took longer than 0.5 ms would render `0.001s` and flip bytes, so a
  re-generation differing **only** in a timing line is a **NAMED-CAUSE**
  difference, not a determinism break — accepted in preference to sanitizing
  real executor output (the bT7/R1 principle: instrument honesty outranks a
  smaller diff). **It did not surface**: the corpus sha matched at training
  time and twice more after quantize.
- **The v4 defect-absent family is 6 hard-decidable / 0 soft, and stands as
  frozen** (ruling bT5/R1). Every claim in that family is settled against the
  file's own bytes by arithmetic or by literal presence/absence, with no
  appeal to intent — so a defect-absent miss cannot be excused as a defensible
  judgment call, and the split is stated in every evidence write-up. The
  comment-contract band v3 carried can return in a later set if wanted; it is
  not an amendment to this one.
- **reason-grounding's denominator is 11, and its haystack is file CONTENTS ∪
  file PATHS** (ruling bT5/R2 as refined by bF/R1, recorded as a dated
  amendment in the protocol *before* the endpoint was ever computed). The 5
  missing-target refuse fixtures are excluded **unconditionally** — their
  target does not exist in the workspace, so the endpoint is structurally
  unmeasurable there — leaving the 6 defect-absent + 5 symptom-mismatch rows.
  A quoted **filename** is a grounded reference, never confabulation. A landed
  refuse row whose `done` text carries **zero** backtick spans is
  **unmeasured**, never 100%: an empty numerator over an empty denominator is
  not evidence of grounding.
- **reason-grounding measures quoting discipline, not honesty — and turn 4
  demonstrated that at the endpoint's ceiling.** The limitation was recorded
  before flywheel4 was measured (baselines §8.1: the confabulation the
  endpoint was designed after is *bare prose*, so the endpoint would not raise
  it at all; and every ungrounded flag on the incumbent's boot was a false
  positive). flywheel4's boot returned **6 of 6 spans grounded — the best
  score the endpoint can give** — while three of the four rows it measured
  carry a **false claim built out of grounded spans**, and the boot's one
  false *repair* claim ("Fixed that before emitting done", on a fixture with
  no `patch` step and byte-unchanged files) sits in a row scored *unmeasured*
  (battery §6.3). **The number is reported because it is the pre-registered
  endpoint's output; it is never read as a confabulation rate, and a high
  score is not evidence that refusal prose is accurate.** Any change to the
  endpoint is a separate dated amendment made after a measurement, never
  inside one.

**A turn-3 carry delivered here that this file never held a bullet for:** the
**sibling-filename contamination rule**. Turn 3's guard was widened to screen
all `task.files` *contents* (struck in the turn-2 section above), and the
turn-3 review explicitly ledgered the *filename*-match half as pre-existing
and out of scope. Turn 4 closes it (Task 4 ride-along, commit `8c64d66`,
`tools/flywheel/tests/test_contamination_siblings.py`): a planted test's
filename is screened against gate targets, so the run slice's `test_<stem>.py`
siblings cannot collide with a gate fixture by name. Like turn 3's contents
half, this was a **correctness precondition** rather than hygiene — turn 4 is
the first turn whose tasks ship a second, factory-named file beside the
target.

**Deferred from this slice (final-review triage: all defer-sound). One line
each; per-task detail lives in the SDD ledger at
`.superpowers/sdd/2026-08-21-flywheel4-turn4/progress.md`:**

- `TaskStep` rows carry **no fixture key and no action arguments** — the
  `CodecFixture` join stays ordinal (validated, never assumed), and a
  *granted* `run` journals only `ran python3 exit 0`, so turn 4's headline
  secondary had to be corroborated from the retained probe scratch rather than
  read from the record. Carried across three turns now; the recurring
  observability debt of this program.
- The tool's `parse_envelope` error string lists `v4` but that listing is
  untested; the bin-level find/refuse v4 pins exercise only the `none` line;
  the per-prefix test name exercises a single prefix (the Rust side pins
  multi).
- The lock-sweep test asserts `!exists` unconditionally — stronger than the
  contract, safe today.
- `flywheel_tool.rs` sits at **783/800** lines: any later tool edit needs a
  themed split, and the ceiling is a hard one.
- `planted_test.run_python` has no timeout or output cap, unlike the
  `exec_run` it mirrors — a future looping test family would hang generation
  (loud-abort-not-bad-row today); the docstring should say the mirroring stops
  at bounds, or the call should grow a `timeout=`.
- The Rust fresh-frame extractor reads **transcribed** skeletons (bounded by a
  factory-side live-assembler test and an anti-vacuity pin); the rule is
  pinned on the 12 refuse skeletons only, though it holds wider in fact.
- Frozen-set residue, named and never amended: `SM-py-01`'s reason drops the
  `" at <site>"` connector (the site is still quoted and real); the
  single-line-literal clause is tautological; find-uniqueness is existential
  (inherited).
- Turn-4 doc residue: a "verbatim" label on a synthesized anchors table; the
  fingerprint's phase table reads `kept = 1449` against the written 1448 and
  wants a footnote; a stale "last commit touching `tools/flywheel`"; three
  generation-side numbers rest on an ad-hoc harness (descriptive only); a
  truncated verb-card header quote; the Task-1 `(spec §3, carried into turn 4)`
  cite is ambiguous on first read.
- `codec_probe_test.rs` grew +21 lines rather than staying flat at Task 5's
  commit (+51/-30) **(branch-net vs master +4: +17/-13 over
  `0056f72..HEAD`)**, on a file already over the test-file cap (pre-existing,
  tracked in the *Smaller items* list).
- **Uninterpreted, deliberately:** `eval_loss` bottomed at 0.0009852 at epoch
  0.74 and finished at 0.001118. No interpretation was pre-registered and none
  is offered — the battery decides, and it did.
- **Recorded, not resolved:** assay's own POST profile scores flywheel4's
  `patch_editing` cell at *stock's* level (`unusable`, decided, [0.0, 0.434])
  on the same daemon and in the same boot where bloomery's probe measures
  20/20 and 16/16 + 16/16. The two instruments run different fixture sets under
  different prompt lenses; both numbers stand as measured, and neither is
  evidence about the other.

**Process lessons:**

- **Detach the long-running process from the agent that starts it.** Turn 4's
  training agent was killed by a harness hiccup at step 539 of 1,086; the
  training itself was `setsid nohup`-detached and ran to completion untouched,
  and a *fresh* agent finished the post-train chain — because the harness
  refuses to resume a stopped agent, whichever way it was stopped. The battery
  applied the lesson pre-emptively: both daemons were launched detached, so
  neither a 10-minute POST nor a probe was ever hostage to the agent watching
  it. **The corollary rule: never wire the measurement's lifetime to the
  observer's.**
- **"Prose from the script's output rather than the bytes" struck again, and
  the fix is the same every time.** Turn 3 shipped five wrong *anatomy*
  sentences beside mechanically-correct verdicts. Turn 4's baselines review
  caught two more of exactly that shape — a "neither stock landing read any
  file" that the committed rows contradicted, and a **cross-envelope causal
  claim** the standing rule forbids (the instrument changed too; no
  v4-mixed-under-v3 arm was ever run). The battery task then caught **three of
  its own draft's** claims the same way — a fixture count, a
  five-versus-six tally, and a "verbatim" JSON block that had been
  pretty-printed rather than copied — by re-deriving each from the committed
  bytes before commit rather than trusting the sentence that felt right.
  Verdicts get recomputed because they are obviously numbers; prose about
  *why* is just as much a claim about the journal.
- **The journal carries a step's executor output in the NEXT step's prompt —
  look there before reaching outside the record.** A granted `run`'s
  `TaskStep` row carries only `ran python3 exit 0`: no argv, no output. That
  is a real limit, and the first instinct it provokes — go and find what the
  run left on disk — produced a *correct but weaker* answer (the retained
  probe scratch's `__pycache__/test_<stem>.pyc` and `__pycache__/<stem>.pyc`,
  both stamped after the patched source, on all five fixtures), resting on an
  **out-of-repo artifact the evidence doc itself lists as uncommitted**. The
  stronger answer was already committed: `exec_run` builds the observation's
  `content` as `format!("exit {code}\n{output}")` — the child's real captured
  output — and that content is replayed into the next step's prompt, which the
  boot journals as an `InferStarted` row. So the committed journal holds
  unittest's own `Ran 1 test in 0.000s` / `OK`, byte-identical on all five
  run-granted fixtures. **The headline secondary of the whole turn rests on
  committed journal bytes, with the scratch as corroboration.** The rule:
  before concluding that the record cannot answer a question, check the
  *downstream* rows — a loop that feeds observations back to the model has
  already written them down. (And the limit that genuinely survives is
  narrower than it first looked: the argv *tail* is still unrecoverable, and
  `Ran 1 test` is consistent with both the explicit-file and the
  bare-discovery form, so it was not guessed either way.)
- **The executed-audit standard, from the Task-5 fixture review.** All 32
  frozen fixtures were re-derived from their own bytes by *execution and
  arithmetic* — the five run fixtures actually run `rc=1 → rc=0`, the
  fresh-frame rule re-run against the **live** skeletons (0 hits) and against
  v3's (21 hits, proving the rule non-vacuous), the transcription diffed
  12/12. A frozen instrument is worth exactly what its audit executed; a read
  of the TOML is not an audit of it.
- **A `pgrep -af '<pattern>'` self-matches its own shell command line.** The
  battery's preflight "is a daemon running?" check answered *yes* for its own
  `bash -c`. `ps -eo pid,comm | grep -w <name>` answers the question that was
  asked. This is the standing box trap that also makes `pkill` patterns
  dangerous here, and it cost a false reading before it was caught.

## Delivered in flywheel turn 5 (2026-08-23, `flywheel5-turn5` branch)

The flywheel's fifth turn: the first trained member of a **new base line**
— `qwen36-reap48-flywheel5`, a bf16 LoRA (r16/alpha32, twelve target
modules, experts + router frozen) trained via peft (not unsloth — `qwen3_5_moe`
is unsupported there) on `~/models/hf/Qwen3.6-35B-A3B-REAP48-ours` (40
layers: 10 full-attention + 30 Gated-DeltaNet, 133 experts), on the
byte-identical turn-4 refusal-honesty corpus, merged and quantized to
Q4_K_M. The turn existed to answer one question, pre-registered before
training: does refuse reach ≥13/16 while patch holds ≥13/16, against the
untrained REAP-48 base's own measured anchor of **patch 13/16, refuse
9/16** (`2026-08-22-g5v4-reap48-baselines.md`)?

**`qwen36-reap48-flywheel5` passed the full pre-registered battery**: G4
**20/20** (the kill leg), G5-v4 patch **16/16** and refuse **16/16** — both
floor passes and both **decided** — `done_trust: true`, matching
`qwen3-14b-flywheel4`'s own perfect scorecard on the dense 14B line and
clearing the refuse floor by +7 fixtures against the required +4
(`docs/superpowers/evidence/2026-08-23-flywheel5-battery.md` §1, §7). Both
boots reproduced every landing, verb sequence and outcome string
byte-identically except `duration_ms` timing noise on the five run-granted
fixtures — a tighter reproduction than the untrained base's own two boots,
which differed in exact wording on 5 of 52 fixtures. Grant violations went
from the anchor's 4 (all `src/`-prefixed invented paths) to **0** in both
boots, and the `done`-count anatomy is an exact 52-on-52 (vs. the anchor's
47-on-52 and the four-shape trajectory census vs. the anchor's sixteen).
Reason-grounding measured 13/17 spans grounded, and three of the four
ungrounded spans sit on refuse rows that falsely claim a repair was
performed, in a boot where **none** of the 16 refuse trajectories ever
executes a `patch` step — the same "declares done without doing the work"
pattern this program has now recorded on three separate lines/turns
(battery §6.6).

**Struck on arrival (debt this turn's own merged ride-alongs closed before
training, named here for the record because this section is where the
program's evidence docs point back to):**

- **"`TaskStep` rows carry no fixture key and no action arguments"** and
  **"a granted `run`'s row carries only `ran python3 exit 0`"** — both
  named as recurring observability debt in the turn-3 and turn-4 sections
  above. Closed by `20d83b1` (`TaskStep` now carries `args`; `CodecFixture`
  names its `agent`, enabling a **keyed** join) and `7ad4df5`
  (`tools/evidence/recompute` — the keyed+ordinal join, Wilson, endpoints,
  pinned against the committed turn-4 journals), both merged via PR #19
  (`71415e8`) before this turn's own boots ran. The argv itself is now
  journaled on a granted `run`'s own row too — `args` carries the executed
  argv directly (`20d83b1`), so both halves of the debt are struck, with
  the battery's five quoted `run` rows as the evidence (battery §6.5).
- **Hybrid-geometry defects 1 (KV over-counted the recurrent layers) and 4
  (recurrent state never charged to VRAM)**, both named in the 2026-08-21
  REAP-48 spike. Closed by `882ee91` (hybrid-aware pager geometry: KV
  counts attention layers only, recurrent state derived from the GGUF's own
  `ssm.*` metadata and charged per context), merged in the same PR #19
  before this turn's boots. Verified holding on the new trained GGUF: both
  boots of this turn report `kv_per_token` 20,480 B/tok and
  `recurrent_state_bytes` 65,863,680 B, identical to the untrained
  anchor's — LoRA training does not touch the checkpoint's hybrid-geometry
  metadata, as expected.

**Recorded, not fixed:** compute-buffer growth with `n_ctx` — the pager's
`ctx_overhead_mib` remains an operator-set constant (512 on every boot this
program has run under envelope-v4), not a measured function of the
requested context window; still unaddressed, unchanged by this turn.

**New debt, found this turn:**

- **The prune tool zeroes `mtp_num_hidden_layers` instead of deleting the
  key.** `tools/flywheel/prune/` writes `mtp_num_hidden_layers: 0` into a
  pruned checkpoint's config rather than removing the key entirely. llama.cpp
  `8672290`'s `convert_hf_to_gguf.py` asserts `opt_num_mtp_layers != 0` in
  `_QwenMtpMixin.__init__` whenever the key is present and reads 0, before
  its own tensor-scanning pass ever gets a chance to prove there are no MTP
  tensors — this turn's post-train chain hit exactly this assertion on the
  first `convert_hf_to_gguf.py` attempt (training record §7) and worked
  around it with the converter's own `--no-mtp` flag. The tool should
  delete the key outright for a genuinely MTP-free checkpoint, or the
  runbook must document `--no-mtp` as a required flag rather than a
  discovered one; the prune GGUF test must cover this converter path so the
  next MTP-free checkpoint does not rediscover the same assertion.
  **Amended 2026-08-27 (branch r6-fix): still open, and now defended in
  depth on the other side.** `crates/bloomery-core/src/gguf.rs` implements
  gguf-geometry v1 R6 — `parse_gguf_meta` subtracts
  `{arch}.nextn_predict_layers` from `{arch}.block_count` and derives every
  layer count from the serving remainder. That is the *reader* side: it
  makes bloomery correct on a trapped GGUF arriving from anywhere (someone
  else's REAP prune, a future converter regression), which the prune tool's
  producer-side patch cannot cover. It closes nothing recorded above — the
  key-vs-delete question, the `--no-mtp` runbook gap and the missing prune
  GGUF converter test are all untouched and still owed. The gap was found
  by the gguf-geometry v1 conformance vectors, which had bloomery reporting
  41 serving blocks against the contract's 40 and over-charging 2,195,456 B
  of recurrent state per context.
- **The S3 uploader's state-file has a tmp-name race under concurrent
  access, root-caused.** `~/flywheel5/s3_upload.py:58-61`'s `save_state()`
  writes one shared `path + ".tmp"` file and `os.replace()`s it into place;
  `multipart_upload`'s 2-worker `ThreadPoolExecutor` (`--concurrency 2`)
  calls `save_state` from `persist_part` in each worker thread, so on the
  last two parts of this turn's upload finishing close together, one
  thread's `os.replace` consumed the shared tmp file out from under the
  other, throwing `FileNotFoundError` on the rename (amendment-1 §2,
  training record §2) — the two threads raced each other on `os.replace`,
  not a conflict with something else in the directory. The multipart
  upload itself completed correctly server-side despite the crash (verified
  independently by `head_object` size and a post-hoc sha256 match on the
  pod). Fix: a lock around `save_state`, or per-thread tmp filenames; not
  applied here since it did not affect correctness, but the race itself is
  a real bug in a tool this program will likely reuse.
- **Spec §4.2's "4 attention layers" wording slip, dated note.** Line 208 of
  `docs/superpowers/specs/2026-08-22-flywheel5-turn5-design.md` states the
  no-cross-example-leakage rationale as "the 4 attention layers... or the
  30 recurrent layers' state" — but the REAP-48 checkpoint's own
  `config.json` measures **10** full-attention layers, not 4;
  `full_attention_interval = 4` is the *stride* between attention layers
  (`block_count / full_attention_interval = 40 / 4 = 10`), not their count.
  Caught during pre-registration review and corrected there rather than in
  the spec file itself (`2026-08-22-flywheel5-preregistration.md`, "A slip
  in the spec's own wording, corrected here"); the rationale (leakage
  across whichever number of attention layers, and across the 30 recurrent
  layers) is unaffected. The spec text is left as written, uncorrected in
  place, per the amendment rule — this is the cross-linked dated note that
  rule calls for.
- **The S3/uplink lesson: this box's outbound uplink is ≈2.3-2.7 MB/s, and
  a plan figure must name its direction.** The pre-registered runbook
  assumed ≈19 MB/s for the base-model upload, quoted from the *pod's* own
  `maxDownloadSpeedMbps` machine spec — a **download** figure, misapplied
  to the **upload** direction. The actual measured uplink (root-caused via
  `/proc/net/dev` to be this local box's own outbound ceiling, not a
  pod-path or transfer-method artifact) was ≈7-8x slower, and would have
  consumed more than half the turn's $10 cap before training even started
  had the SSH-path plan been followed to completion (amendment-1 §2). The
  lesson: any bandwidth figure quoted from a cloud machine's spec sheet
  must be checked for which direction it describes before it is used to
  bound a transfer plan running the other way.
- **The `echo $!`-after-`setsid` PID gotcha, reproduced a third time.**
  `setsid` (util-linux) forks to avoid calling `setsid()` on a
  process-group leader, so the shell's `$!` captures `setsid`'s own PID —
  already exited by the time it is checked — not the daemon's. This turn's
  boot 1 hit it again (`$!` = 1555252, already dead; real daemon PID 1555254
  found via `ps -eo pid,comm | grep -w bloomery-daemon`), exactly as Task 6
  and the turn-4/REAP-48-baselines battery both recorded it. Recorded here
  a third time not because it is new, but because it keeps recurring
  despite being documented twice already — a candidate for a small wrapper
  script that does the `ps`-based PID discovery itself, rather than relying
  on every task's author to remember the workaround.
- **`pip install -r requirements-convert_hf_to_gguf.txt` clobbers the
  pinned torch/transformers versions.** llama.cpp `8672290`'s own
  `requirements/requirements-convert_hf_to_gguf.txt` carries exact pins
  (`torch==2.11.0` from a CPU-only wheel index, `transformers==4.57.6` via
  its own `-r requirements-convert_legacy_llama.txt`) that silently
  **uninstall** a correctly-pinned CUDA torch/transformers when installed
  per the brief's literal one-line chain (training record §4:
  `torch 2.9.1+cu129` → `torch 2.11.0+cpu`, `torch.cuda.is_available() ==
  False`). Caught before any billed smoke test or training step this turn,
  by an explicit version-print check the runbook already included — but
  the runbook itself does not warn that this specific `pip install -r`
  step is destructive to the earlier pins. The fix applied was a
  `--index-url` re-install of the correct versions immediately after; the
  runbook should sequence the installs to avoid the clobber in the first
  place (install the GGUF-conversion requirements *before* the CUDA
  torch/transformers pins, not after) or check `torch.cuda.is_available()`
  immediately after that specific step rather than only at the end of the
  chain.
- **The label-mask/`</action>`-tail test runs on a synthetic corpus, not
  the real one.** Spec §4.1 asked for "label masking + `</action>` tail
  hold on real corpus rows," but the shipped test
  (`tools/flywheel/tests/test_train_common.py`'s
  `test_tokenize_masks_prompt_and_ends_at_action_close`) exercises a mini
  tokenizer and synthetic rows from `train_fixture.py`
  (`build_action_tokenizer`, `tiny_corpus`), not the real turn-4/5 corpus
  with the real tokenizer. The property was covered at run time instead —
  the pod's own smoke test printed `label-check ok: 341 prompt tokens
  masked, tail='\n</action>'` (training record §5) — but a committed,
  skip-if-absent unit test over the real corpus + real tokenizer, matching
  that runtime check, remains to be added.

**Deferred, unchanged by this turn:**

- **Packing side study** — deferred to a later, separately pre-registered
  study (prereg, "Packing is deferred to a later, separately pre-registered
  side study; it is a non-goal of this turn"). Unpacked, batch-size-1
  training was used throughout, for the two named reasons (state leakage
  across the 30 recurrent layers, cross-attention leakage across the
  full-attention layers).
- **Honesty instrument** — named as turn 6's own spec in the roadmap
  pointer; not started by this turn.
- **Router/expert training** — remains parked research
  (`research-moe-quantized-expert-training`); this turn's LoRA targets
  attention + shared-expert modules only, experts and router frozen and
  asserted frozen at load (`assert_frozen`), unchanged from the
  pre-registered plan.

**Process lessons of the wave:**

- **"Anatomy from scripts, not from memory" struck again — this time in
  the REAP-48 baselines' own evidence review, before this turn's battery
  was even written.** Task 6's baselines doc shipped one Critical
  (`refuse`'s Wilson flag mislabeled "provisional" when the interval lies
  wholly below 0.80, i.e. it is `decided`) and three Important errors — a
  refuse-miss count off by one, a grant-violation-recovery count
  contradicted by the same document's own prose, and a trajectory-shape
  count off by one — all caught by re-deriving each claim with a dedicated
  script rather than trusting the sentence that read correctly on first
  pass (baselines doc, "Fix round 1"). This turn's own battery task applied
  the lesson pre-emptively (every anatomy claim in
  `2026-08-23-flywheel5-battery.md` is a quoted script output, not
  paraphrase) and still caught three of its **own** draft's errors in
  self-review before commit: a wrong boot-2 timestamp, a fixture-count
  transcription (12 written where 16 was meant, twice), and one internally
  garbled sentence — by the same method, re-deriving from the committed
  bytes rather than trusting the first draft. The lesson is not "write the
  script once and trust it forever" — it is "re-derive every claim, every
  time, including the ones that feel obviously right."
- **Implementer poll loops do not, on their own, re-invoke the agent that
  launched them.** This turn's battery task launched each boot's
  wait-for-verdict poll as a detached background command and then paused
  rather than busy-polling the foreground, per the house rule — but the
  background command's own completion notification needed an explicit
  controller nudge to resume the task both times (boot 1 and boot 2 alike),
  rather than the poll's exit alone driving the next step automatically.
  Recorded as an operational fact about this harness for anyone planning a
  multi-boot measurement: budget for a controller check-in at each poll
  boundary, not just at the end of the whole task.
- **The first pod's upload-speed assumption cost $0.46 and a stop-rule
  invocation, and that is what it is for.** Pod 1 ran ≈17-19 minutes,
  measured the SSH-path upload at the local box's real uplink ceiling, and
  was torn down the moment that measurement made the pre-registered plan's
  cost bound infeasible — not treated as wasted spend, but as the exact
  measurement that motivated the S3-path switch that made pod 2 succeed
  cleanly (amendment-1 §3). The $10 stop rule is not only a ceiling on a
  single run's cost; it is what makes a $0.46 "failed" pod a correctly
  bounded discovery rather than a runaway one.

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

   **Amended 2026-09-01 (slice C, branch `refusal-max-placeable`). Half
   delivered; the prescription above is WITHDRAWN.** This item makes two
   complaints, and they turned out to want different fixes:

   1. *the window law is blind to a resident sibling* — **still open**,
      unchanged, and the regression test above still passes verbatim; and
   2. *a refusal leaves "no smaller window to fall back to and no recovery"*
      — **closed**. `PagerError::Refused` now carries `max_placeable_tokens`:
      the largest window that would place, so recovery is a mechanical
      re-ask rather than a guess. It appears in the native `409` body, in the
      `/v1` `503` message, in `Display`, and in the journal's refusal detail.

   **The prescription this item recorded — "`GeometryInput` also carrying the
   other models' loaded weights and residents' reservations" — was
   implemented in neither form, deliberately.** `GeometryInput` is unchanged
   and `usable_window` is untouched. Two reasons, both found by reading code
   this item predates:

   - **It ignores reclamation.** `plan_residency` evicts idle,
     strictly-lower-priority residents. Subtracting *all* resident
     reservations would starve exactly the case eviction exists for: a
     high-priority agent arriving to a pool of idle low-priority residents
     would be sized against a budget those residents are consuming, though
     placement would evict every one of them.
   - **It bakes a transient into a permanent.** `usable_window` has exactly
     one call site (`pager.rs`, in `create_agent`) and `Agent.window` is
     written exactly once, so the reading taken at create time is permanent
     for the agent's life — while the sibling pressure it corrects for is
     momentary. It would trade a loud permanent refusal for a quiet permanent
     degradation, which for this project is the worse failure.

   **A placement-time downsize was designed as the replacement, reviewed, and
   REJECTED — recorded here so it is not proposed a third time.** The idea:
   on the `Refuse` arm, shrink a `Vram`-bound window to what actually fits and
   retry. A four-lens adversarial review (2026-09-01, every finding
   independently verified against code) found five distinct defects:

   1. **CRITICAL — it destroys suspended agents' conversations.** `place()`
      is reached from `ensure_resident` for *Suspended* agents, not only
      Fresh ones: the early return covers only `AgentState::Resident`.
      Shrinking the window changes the `n_ctx` `open_context` then creates;
      `load_state` rejects an image holding more cells as
      `STATE_SIZE_MISMATCH`; and `restore_image` deliberately does **not**
      put those bytes back on that branch (`if !failure.contains(...)`), so it
      cold-starts and returns `Ok` → HTTP 204. A refusal that *preserved* a
      conversation becomes a success that *destroys* it. The no-put-back rule
      is sound only while invalidation is a permanent property of the image;
      a downsize makes it a transient function of sibling residency.
   2. **HIGH — the bookkeeping desyncs.** `kv_bytes` and `reserved_bytes` are
      derived from the window once, at creation. A downsize that does not
      re-derive them leaves the agent charging the budget for a window it no
      longer has, so it frees nothing for the siblings it exists to help.
   3. **HIGH — the prompt gate reads the stale window.** `infer` reads
      `a.window.tokens` and runs law 2's prompt check *before* calling
      `ensure_resident`, so on the triggering call a prompt admitted at 4096
      is sent to a context opened at 1200, and the refusal's own arithmetic
      says the prompt fits.
   4. **HIGH — sizing to `avail + reclaimable` is maximally evicting.** It
      guarantees the re-plan evicts every idle lower-priority resident, when a
      smaller window would have fit in `avail` alone and evicted nobody.
   5. **HIGH — it is the same transient-into-permanent defect** used two
      paragraphs above to reject this item's own prescription. The rejection
      argument condemns the replacement just as hard.

   Advising is safe precisely because it mutates nothing. **The remaining
   sibling-blindness needs a design where the window is computed at the
   moment the residency reading is valid** — deferring the VRAM term to first
   placement, or placing at create time so the two coincide. Both change the
   lazy-residency contract and belong to their own slice; neither is
   scheduled.

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
  **Amended 2026-08-19 (swap-candidate):** still open, and the parenthetical
  is now false — `pager_test.rs` is 1247 lines and **eight** test files are
  over the ceiling. The original text stands as recorded; the swap-candidate
  section's *Recorded, not fixed* carries the full count as of 2026-08-19.
  The remedy is unchanged and now project-wide, not one file's chore.
  **Amended 2026-09-01 (slice D, first PR).** The count is **21 → 20**, and
  the *worst* offender is gone: `api_native_test.rs`, 2505 lines, is split
  along its own section rules into five focused files
  (`api_native_test.rs` 491 core routes + `/status`,
  `api_native_bless_unblock_test.rs` 570, `api_native_swap_window_test.rs`
  416, `api_native_swap_candidate_test.rs` 413, `api_native_poison_test.rs`
  191), with the fixtures more than one of them needs lifted into
  `tests/common/native.rs` (479) rather than duplicated. All 47 tests survive, the
  workspace total is unchanged at 948, and the split is behaviour-preserving
  by construction: no test body was edited, only moved.

  Two honest notes. **One file crossed the ceiling this session**:
  `api_native.rs` at 816, pushed over by slice 1's own `DELETE /agents/{id}`
  handler — recorded here rather than quietly absorbed, since this entry's
  whole history is a count nobody was watching. And **21 → 20 understates the
  work**: the excess above the ceiling fell by ~1,700 lines, but the count
  moves by one because a split removes one name from the list while adding
  files that were never on it.

  **Amended again 2026-09-01 (slice D, second PR).** `drift_test.rs`, 1983
  lines and the second-worst offender, is split into four
  (`drift_gate_test.rs` 584, `drift_boot_test.rs` 558, `drift_test.rs` 438
  store mechanics, `drift_admission_test.rs` 177) over
  `tests/common/drift.rs` (292). All 46 tests survive; workspace total
  unchanged at 948. **20 → 19.**

  This one needed a bigger shared module than `api_native` did, and the
  reason is worth recording: `drift_test.rs` resisted a section-only split
  because its fixtures were coupled *across* sections — `profile_doc` and
  `boot` were each reached by three of the four resulting files, and the
  lifting had to close transitively (a lifted helper's own callees, like
  `scratch` and `value_of`, must come with it).

  **The recipe, with the trap that nearly cost a test.** Split on the file's
  existing `// ---` section rules; lift any helper used by more than one
  resulting file into `tests/common/`, closing transitively over what those
  helpers call; give the common module `#![allow(dead_code)]` (Rust builds
  every integration test as its own binary, so a helper used by three files
  is dead in the rest); then let `cargo clippy --fix` prune the copied import
  block per target.

  **Cut on the section rule, never on the first item's signature line.** A
  test's `#[test]` attribute and doc comment sit *above* its `fn`, so a range
  starting at the signature silently strips both — leaving a bare `fn` that
  still compiles and still looks like a test in the file, but is no longer
  run. It happened here to
  `the_stores_current_path_is_the_file_post_actually_writes`, and it was
  caught only because the per-file test counts were summed and compared:
  45 where 46 were expected. **A lost test shows up as a smaller number, not
  as a failure** — so the count is the check that matters for this chore, and
  a dead-code warning is the corroborating signal. Verify by diffing the set
  of test names before and after, not just the total.

  **Amended again 2026-09-01 (slice D, third PR).** `codec_probe_test.rs`,
  1634 lines, is split into three (`codec_probe_boot_test.rs` 537,
  `codec_probe_test.rs` 530 scoring rules, `codec_probe_status_test.rs` 430)
  over `tests/common/codec.rs` (200). All 29 tests survive, verified by
  name-set diff before the run rather than by total alone -- the check the
  previous PR earned. **19 -> 18.**

  Two mechanical traps this one added to the recipe, both caught by the
  compiler rather than by review. **Grab multi-line `use` blocks whole**: an
  import extractor that keeps only lines starting with `use ` truncates a
  braced, wrapped import into an unclosed delimiter. And **an item's computed
  span can swallow its neighbour** when a preceding `const` carries a
  multi-line literal, which silently drops the next helper from every output
  file; a guard that refuses to emit any line twice turns that into a missing
  symbol at build time instead of a duplicate definition.

  **Amended again 2026-09-01 (slice D, fourth PR).** `api_task_test.rs`,
  1295 lines, is split into three (`api_task_codec_test.rs` 485,
  `api_task_test.rs` 471 the HTTP surface, `api_task_degrade_test.rs` 247)
  over `tests/common/task.rs` (128). All 16 tests survive. **18 -> 17.**

  **The recipe is now a script rather than prose**, because four
  hand-executions produced four different mistakes. It encodes all four traps
  and — the part that matters — it *refuses to write anything* if the set of
  test names would change, so the failure that nearly slipped through on
  `drift_test.rs` is now impossible to commit rather than merely likely to be
  noticed. It also closes the shared set transitively by fixpoint, which is
  what previously took three or four manual compile-and-lift rounds per file.
  One residual manual step: a helper moved into `tests/common/` that called
  `common::http` needs that rewritten to `super::http`.

  **Amended again 2026-09-01 (slice D, fifth PR — the pager family).**
  `pager_test.rs` (1266) and `pager_weights_test.rs` (944) are split into six
  files, none over 646, and the whole `pager_*` fixture layer is unified into
  `tests/common/pager.rs` (101). **17 -> 15.**

  **The duplication was worse than the count suggested, in a way worth
  naming.** This entry has been citing "`fresh_dir` in 22 files, `meta` in
  18" as *duplication*. Measured properly before touching anything, it was
  **divergence**: `pager_in` existed in three distinct shapes, `write_gguf` in
  three, `meta` in two. That is not one helper copied six times, it is one
  helper *forked* six times — the more expensive kind, because every fork
  reads as correct in isolation and only the set of them is wrong. Unifying
  blind would have been a behaviour change, not a chore; each replacement
  signature was checked to be a strict **superset** of the variants it
  replaced, so no call site lost a capability or gained behaviour
  (`meta()` -> `meta(1000)`, the literal the no-arg form hard-coded;
  `write_gguf` -> the 3-arg form; `pager_in` -> the 3-tuple, with
  `let (p, _, _)` where a caller wants less).

  The lesson generalises to the rest of this list: **measure whether copies
  are identical before calling them duplication.** The dedup alone shrank
  `pager_remove_agent_test.rs` 137 -> 87 and `pager_reservation_test.rs`
  1000+ -> 636 without splitting either.

  One PR per file, deliberately: reviewability is the only property that
  makes a refactor this size safe.

  **Amended 2026-08-31 (`agent-delete-endpoint`):** still open, and now
  **20** files over the ceiling — 13 test, 7 source — measured on the branch.
  `pager_test.rs` is no longer the worst of them and has not been for some
  time: `api_native_test.rs` is 2505 lines, `drift_test.rs` 1983,
  `codec_probe_test.rs` 1634, `task/registry.rs` 1395 (the largest *source*
  file), `tools/memory_battery/tests/test_recompute.py` 1386. The count has
  gone 1 → 8 → 20 across three amendments without the remedy changing, which
  is the actual finding: recording a chore is not the same as scheduling it.
  This slice put its own tests in a new file (`api_native_agent_delete_test.rs`)
  rather than appending to `api_native_test.rs` for exactly that reason —
  the smallest thing that keeps the number from going to 21.
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

## memory-battery-v1 (2026-08-27, branch memory-battery)

**Settled:** the memory organ's first capability gate — PASS (repeat cost −10.0 tokens median vs 6.11 derived bar, findings 2026-08-27-memory-battery-findings.md); 100% exact-match injection on byte-reset repeats; the contradiction path exercised at scale (2 retired episodes, honest direction); five gate-corrupting defects caught pre-lock by review (MODEL stanza-key, Error-status cost scoring, completion_tokens zero-fill, missing arm-M p1 reset, treatment-identity).

**Deferred, with rulings:** `tools/memory_battery/tests/test_recompute.py` at 1386 lines needs a split (over the 800 ceiling; deliberately not split mid-wave); `_task_step_duration_by_agent` zero-fills `duration_ms` (advisory wall only, never a gate number); `DAEMON_ERROR_STATUS` constant sits mid-import-block (linter cosmetic); ARM_LABEL literals are case-sensitive (`--arm C`/`--arm M` — recorded prereg §5.1/§7); wall-clock on injected repeats is SLOWER at this task size (advisory finding, echoes crucible B4) — a larger-task battery would re-balance prefill-vs-decode and needs its own prereg.

**Process lessons:** a fake-server suite cannot falsify wire values only the live daemon checks (the MODEL stanza-key miss survived 32 green tests; the prereg cross-check caught it); "absent evidence reads as PASS" recurred twice in one module (C1 zero-fill, then the same class one level down at the row field — hunt the whole depth of a cost path, not its first join); a plan brief that paraphrases a spec list drops items (Error statuses fell out of H3's definition in transcription — cite, never restate).

## R6 review findings — 2026-08-27 (gguf-geometry conformance arc)

**Deferred, with rulings:** a GGUF declaring `{arch}.block_count: 0` with no MTP key still parses to layers 0 / attention 0 / kv 0 and reaches geometry.rs's unbounded-window path (`free_vram_bytes.filter(|_| i.kv_per_token != 0)` drops the VRAM candidate) — pre-existing master behavior, deliberately preserved by the R6 fix's byte-identical requirement; the file now holds two guards refusing zero serving layers and one path that still permits it. `lookup_u32` truncates `u64 → u32` with `as`, so any count key written as U64 ≥ 2^32 silently reads as 0 (over-charge, not refusal) — pre-existing for every key including `block_count`; noted because R6 now hangs a refusal off that read.

**Amended 2026-08-27 (branch `v2-vendor`): vendored set is now v2, not v1.** `crates/bloomery-core/tests/data/gguf_geometry_v2/` holds a byte-exact copy of gguf-geometry `vectors/v2/` at master `7f858c8` (eleven vectors; manifest sha `06da801b…`), and `tests/data/gguf_geometry_v1/` was deleted — upstream's consumer model vendors one current set, and `vectors/v1/` stays frozen there. The paragraphs above are the historical record of the v1 arc and stand as written: the R6 gap *was* found by the v1 vectors, and R6's rule text did not change for v2. Nothing above is closed by the re-vendor. What it adds is `qwen3.8-27b`, the case v1 withheld — 65 blocks, `nextn_predict_layers` 1, `full_attention_interval` 4, full `ssm.*` — which exercises R6 → R3 → R2 and R6 → R4 in one hardware-verified model and passed through `parse_gguf_meta` unchanged on the first run. The two deferred items above are untouched by it: neither the `block_count: 0` unbounded-window path nor the `u64 → u32` truncation is reached by any v2 vector.

**Amended 2026-08-28 (branch `mla-kv-rule`): vendored set is now v3, byte-exact from gguf-geometry master `84f042b` (public CI green, run `33163833319`); v2 dir deleted; deepseek re-pinned per R9 (331776 → 276480, measured).**

**Amended 2026-08-28 (refalsify domain-of-validity erratum):** the
refalsify probe refutes patch-class episodes by construction — exact
match = pre-state bytes, `run_evidence` = post-condition, nonzero =
contradiction — so a drift-free exact repeat poisons its own true lesson
(demonstrated live through the registry seam, throwaway test, 0.08s; see
the dated erratum in `2026-08-27-refalsify-on-exact-design.md` §6). The
queued refalsify-on battery over the v1 corpus is cancelled as designed
(100% patch-class: every endpoint entailed pre-boot). Open: refalsify-v2
(class-aware / expectation-matched probe) needs its own spec; the §6 cost
question transfers to it.

**Amended 2026-08-28 (refalsify v2 closes the erratum):** the
domain-of-validity erratum above is closed by refalsify v2
(`docs/superpowers/specs/2026-08-28-refalsify-v2-class-aware-design.md`,
commit `7a930d4`; branch head `32431b8`): the two clean-outcome verdicts
invert — a clean nonzero exit now injects, stamped `premise_held` (the
failure confirms the matched premise); a clean exit 0 goes silent with no
store mutation, stamped `premise_gone` — and no probe verdict ever calls
`mark_contradicted`. The stamp spellings `passed` and `failed` retire
from reachable probe verdicts under v2; they remain valid, parseable
spellings in journals written by v1 builds (refalsify-on-exact design
`docs/superpowers/specs/2026-08-27-refalsify-on-exact-design.md` §2.3) —
retired, not removed, no schema change. Open: the queued
refalsify-on-battery slice is re-registered against v2 as its own future
pre-registration (v1 spec §6's cost question transfers to it, per the v2
spec §5), and remains unscheduled.

A second tension was found by this arc's implementer during v2's own
implementation, verified by its reviewer and by the controller against
`crates/bloomery-daemon/src/task/registry.rs:599`: a correct
`premise_held` injection into a task that legitimately completes without
its own patch-and-verifying-run cycle is passively contradicted by the
pre-existing memory-organ design §5 rule (`organ_after_run`: a scored
outcome with no verifying run contradicts whatever was injected) — the
poisoning is indistinguishable from "the lesson was wrong." This is
pre-existing discipline, not a v2 defect (the rule predates this spec and
is unchanged by it), but v2 changes which episodes get injected via
probing — `premise_held` now injects into cases v1 either never probed
(flag off) or contradicted outright — so the practical weight this
discipline carries shifts even though its text does not. A future slice
weighing it should start from the memory-organ design's §5, not from the
probe. Recorded as a second named limitation in the v2 spec's §1.

**Amended 2026-08-28 (refalsify-battery-v2 executed, branch
`refalsify-battery-v2`):** the battery re-registered against v2 above
(prereg `2026-08-28-refalsify-battery-v2-preregistration.md`, commit
`98b4ad2`) ran to completion the same evening on Brice's launch ruling —
both arms `DONE exit_code=0`, 102/102 ledger rows each, zero infra
faults, zero dropped tasks — and its gates were read exactly once
(findings: `2026-08-28-refalsify-battery-v2-findings.md`; raw recompute
output committed verbatim at
`2026-08-28-refalsify-battery-v2-recompute.json`). **Verdicts: G1
(token preservation) PASS, diff 0.0 within band 5.325; G2 (injection
preservation) PASS, 50=50 exact; stamp audit clean (100% `premise_held`
on R-p2 injections, zero forbidden spellings, zero `premise_gone`); H2
(instrument validity) not violated, run VALID; H3 (infra) not violated,
0% both arms.** The licensed sentence (spec §1) is therefore said, with
one honest qualifier: **A1's probe-cost number is not resolved from box
noise** — the no-probe p1 control (where neither arm can fire a probe at
all) shows a −3.5 ms median wall gap, the same order as the probed p2
gap of +4.5 ms (0.09 ms/probed-retrieval nominal), so this instrument
cannot distinguish a real probe cost from box noise at this resolution.
This closes the "queued refalsify-on-battery slice" that the prior two
amendments above left unscheduled — it is no longer open, it ran and its
numbers are read. **Left open by these verdicts:** the three named
absences (spec §1: `premise_gone` lane, the staleness-benefit story, the
design-§5 passive-poisoning weight) each still need their own corpus
treatment and their own registration, unchanged by this run; the
default-flip ruling for `[memory] refalsify` is Brice's call — these
findings inform it, per spec §7, but do not make it; and A1's probe-cost
question is a genuine open measurement, not merely deferred — resolving
it needs a battery purpose-built to shrink box noise below a few
milliseconds (more probed retrievals, repeated boots, or both), which is
a new pre-registration, not a re-read of this one. A parked wording nit
from Task 3 (the prereg's amendment-rule section claims to be "copied
verbatim from v1's prereg" but adapted one self-referential clause to
this document's own section numbering) is recorded in the findings
doc §10 — force preserved, not grounds to amend the lock, not fixed
here.

## refalsify default flip — 2026-08-28 (operator ruling, post-battery)

The `[memory] refalsify` default-flip ruling recorded as open above is
CLOSED: Brice ruled the flip on the refalsify-battery-v2 findings
(preservation exact, injection 50=50, probe cost sub-noise), and
`MemoryConfig::refalsify` now defaults `true`
(`config::default_refalsify`; both specs carry dated amendments;
`refalsify = false` is the opt-out). Found while landing it: the v2 doc
sweep had missed `src/config.rs`'s field comment, which still described
v1 semantics ("a clean nonzero exit contradicts the episode and the task
runs memory-silent" — exactly inverted from v2's `premise_held`-injects).
Fixed in the same change; lesson unchanged from the flywheel waves:
doc sweeps must enumerate every file that states the semantics, not
just the module that implements them.

## premise-gone-battery-v1 — 2026-08-28 (branch premise-gone-battery)

**Settled:** the first of battery-v2's three named absences. Full PASS
(findings doc): refalsify-on takes the premise_gone lane totally on
goal-satisfied exact repeats — 50/50 `premise_gone` silent stamps, zero
injections, zero contradictions, store fully verified — while
refalsify-off injects the moot lesson 50/50. Design finding recorded in
spec §0: under two-stage exact retrieval the "already-fixed" flavor of
goal-satisfied can never retrieve; the reachable lane is "the world
moved on" (verification contract updated around unchanged cited bytes),
realized by moved-on tests that pass on the defective target and fail
on the old fix (S4 non-vacuity).

**Deferred, with rulings:**
- The design-§5 passive-poisoning WEIGHT registration now has its
  motivating observation: A2 shows §5 contradicted 47/50 TRUE episodes
  in the off arm on goal-satisfied repeats (3 survived only by
  re-patching), zero in the on arm. Advisory here; that registration
  should start from the memory-organ design §5 and may now cite this
  battery's A2 as the fired question.
- Staleness-benefit story still needs its own corpus treatment (A1's
  within-band +5.0 completion-token diff, silent arm HIGHER, is its
  first observation: the moot injection may shortcut investigation —
  direction unproven, unregistered).
- Spec-flagged [judgment] calls for Brice's after-the-fact review:
  corpus seed 20260828, bootstrap seed 20260829, floor 25 = n/2.
- Probe cost remains unresolved (battery-v2's verdict stands; this
  battery's A3 wall delta runs the WRONG direction for a probe cost and
  is tail-dominated by M′'s moot-lesson recovery work).

**Process lessons:** the whole machinery arc (generator with
execute-and-pin authoring, S1-S5 checker, driver per-phase source,
scratch-copy p2 carry, recompute) landed with 8 mutation checks killed
before the lock and zero live machinery failures across dry + real
runs; the scratch-copy manifest held the tracked-tree rule (git status
clean after 206 daemon task-halves of granted writes). The
sibling-convention driver delta kept corpus-v1 manifests byte-identical
in behavior (compat pinned by the untouched pre-existing suite).

## s5-weight-battery-v1 — 2026-08-29 (branch s5-weight-battery)

**Settled:** the second named absence — the design-§5
passive-contradiction weight, measured under its own lock (findings
doc). Validity all-PASS; the registered weights: moot-lane poisoning
TOTAL (16/16 true-but-moot lessons contradicted, Wilson [0.806, 1.0]);
zero collateral on right lessons (0/16, [0, 0.194] — every control
lesson re-verified and refreshed); stale lane removed 15/16 and
corrected 1/16 (no stale lesson survived stale). The rule's mechanism
cannot distinguish moot-true from stale-wrong — both are
injected+scored+no-verifying-run — so its weight is total in both
directions; the moot lane's sharpest mechanism fact (advisory): every
moot task ATTEMPTED a patch, but the moved-on contract fails the
stored fix, so no path short of independently discovering the new
contract produces a verifying run.

**Deferred, with rulings:**
- Any §5 design amendment is Brice's ruling, now informed by measured
  weights from both sides (this battery) plus the shield fact
  (premise-gone battery: refalsify-on prevents the moot-lane injection
  entirely). Candidate directions recorded for that ruling, NOT
  proposed: a no-defect-found honest-completion signal that §5 treats
  as unmeasured; scoping §5 to tasks whose own verification ran and
  failed; or accepting the weight as-is given the refalsify-on default
  shields the dominant false-positive lane.
- Spec-flagged [judgment] calls for review: corpus seed 20260830,
  floor 8 = 16/2, third-value perturbation constants.
- Task-4 finding recorded as a dated spec V1 amendment: `_load_arm`
  already drops Error halves (its own H3 rule) — the Error exclusion
  lives at the join; H3 counts once; unscored-in-matched = INVALID.

**Process lessons:** the entailment analysis at DESIGN time (spec §0)
kept the code-entailed mint-xor-contradict totality out of the
endpoints — it ran as validity gate V1 only; the registered product is
the splits. Wilson intervals (no RNG) fit proportion endpoints cleaner
than bootstrap and cost four independent hand-derived vectors to pin.
Driver and dry_manifest needed ZERO changes for a three-lane per-task
p2 corpus — the key-presence convention generalized; their shas in
this prereg are byte-identical to the pg lock's pins.

## §5 ruling — 2026-08-29 (Brice: "we will do as you recommend")

The s5-weight-battery's open ruling is CLOSED: **§5 stands as-is.**
Rationale of record: the shipped refalsify-on default already shields
the measured-total moot lane (premise_gone → nothing injected → §5
inert there); the true-positive lane is earning its keep (15/16 stale
removals + 1 correction); collateral on right lessons measured zero.
No code change ships on this ruling.

**QUEUED as a recorded future slice (not scheduled): run-evidence
scoping of §5** — trigger becomes "a completed post-injection run
failed, with no later completed pass" instead of "scored outcome with
no verifying run." Motivations pinned when queued: (i) resolves the
refalsify-v2 spec's named premise_held-into-honest-completion tension;
(ii) fixes the code-entailed Done-gate wrinkle (a scored non-Done task
with a landed patch + passing run still contradicts — verifying_run is
Done-gated); (iii) preserves the measured stale-removal lane (every
observed stale contradiction came from a ran-and-failed task). If
executed: memory-organ design §5 dated amendment + registry change +
pins, and the weights re-measured under the amended rule before anyone
quotes them.

**PARKED by the same ruling:** the staleness-benefit registration
(headline pre-empted twice — token deltas within band in both
batteries; the qualitative story already licensed) and the probe-cost
registration (revisit only if a ms-level number becomes
decision-relevant). The three spec-flagged [judgment] items (seeds,
floors, perturbation constants) remain standing review flags,
unobjected.

## turn-6 Phase B review notes — 2026-08-29 (branch turn6-envelope-v5)

Settled at review: the reason_matches_family patch-class silent-drop
(CRITICAL — undeclared reasons vanished from the patch buckets exactly
on the undeclared-dominance axis §5.6 predicts; fixed with a third
bucket + sum-invariant regression test); the mixed grounded+misaligned
evidence row-bucket rule DECIDED as "misaligned" (nothing fabricated,
at least one citation off; never diluted into a grounded-looking
bucket) and pinned by test. Deferred: a mixed v4+v5 model-list
integration test through run_boot_g5_probe (the grouping is unit-pinned;
the loop was hand-traced at review); v4's cross-class shape-sharing
floor test deliberately not carried to v5 (zero sharing in the frozen
set; rationale in the diversity file's header); the find-witness
threshold note (8→6 chars, documented in the rules test).

## Delivered in flywheel turn 6 (2026-08-29, the honesty instrument)

**Delivered:** Phase A — the pre-registered v4 claim audit over seven
committed journals (5/5 calibration incl. the expected miss; 10 flagged
landed-but-lying rows in 6/7 journals, symptom-mismatch-concentrated;
false_denial 0 everywhere; fw3 clean; two new findings beyond the
hand-reads, one of them the v1–v4 done card's own worked example
verbatim in a lying landed refusal). Phase B — envelope-v5 (declared
`done`: outcome/reason attributes + leading evidence lines; v1–v4
byte-identity pinned), the frozen `codec-tasks-v5-mixed` (seed 8290829,
sha bf2db8ac…; the mirrored freeze suites caught two sub-floor targets
pre-freeze), per-envelope G5 instrument selection, `done_v5`, and the
three declaration endpoints (a B6 review CRITICAL — patch-class
undeclared reasons silently dropped — fixed pre-merge with a
sum-invariant pin). Phase C — eight pre-registered baseline boots, all
digests asserted, results in the baselines doc; the declaration
endpoints pinned to the committed flywheel5 boot-1 journal.

**Headline baseline facts (descriptive; floors are turn 7's prereg):**
the declared card is adopted by EVERY model incl. untrained (undeclared
0 across the board — §5.6's first honest possibility did not occur);
outcome-consistency splits perfectly by training (32/32 on both
flywheel models, 4 inconsistent on each untrained); `different-defect`
is declared by NO model on ANY symptom-mismatch row (0/5 everywhere —
the sharpest turn-7 target); evidence grounding is the dominant gap for
every model (grounded 2–8 of 21–32) and separates cleanly from outcome
honesty.

**Deferred, with rulings:** turn-7 pre-registration owns every
declaration floor, the training corpus (`generate_envelope_v5`), and
the corpus-side `done_v5` ideals; the judge-shaped
"true-but-irrelevant evidence" endpoint stays a named residual; the
card-example confound stands over every v1–v4 number (v5 fixes it by
construction); a mixed v4+v5 `run_boot_g5_probe` integration test and
the `BadAttr` parser tightening remain open; prune-tool `mtp` debt
unridden.

**Process lessons:** the named bug class struck TWICE in our own
tooling this turn (fabricated sha tails in the boot runner — caught by
pre-launch verification; the patch-class silent-drop — caught by the
B6 reviewer) and once more in prose (three miscounts in the audit doc —
caught by the verifier's re-derivation). The freeze-held-until-tests
discipline caught two real fixture defects with zero amendment cost.

## Delivered in flywheel turn 7 (2026-08-29, training the declarations)

**Delivered:** the v5 corpus pipeline (`generate_envelope_v5`: mechanical
post-patch evidence for patch ideals, template ground-truth triples for
the 8 target-present refusal templates, family→reason inverted from the
one endpoint table; `--envelope v4` byte-identical, proven against the
pre-change tree), the structural corpus check (`check_corpus_v5`, rules
1–7), eval-time instrument binding (`instrument_rows` in recompute —
duplicates/unknowns exit 2; the adversarial reviewer's journal surgery is
a committed regression test), the executed floor derivation + mechanical
verdict (`derive_turn7_floors --evaluate`, comparator and instrument both
sha-pinned), the flywheel-tool verbatim declared-`done` path (validated
by the real parser), a 4,563-pair corpus (sha `08c0bc6d…`, guard-clean
against all five gates, checker 0 violations with bound expectations),
one training run at ≈$6.70 of the $10 cap, and a two-boot battery:
**PASS on all seven locked floors** by the tool's verdict (anchor
pre-declared; boot 2 identical on every endpoint).

**Deferred, with rulings:**
- **The lens-shaped `different-defect` residual** is the sharpest next
  target: all 3 python symptom-mismatch rows declare it, both plaintext
  rows still declare `no-defect` (3/5 vs the 3/5 floor). A future turn
  owns whether that's a corpus-side signal question (plaintext
  symptom-mismatch ideals) or a capacity one.
- The two find-shaped patch misses (honest `no-such-file` refusals after
  failing to locate the target) are a find-capability note, not a
  declaration defect; recorded, not scheduled.
- `patch_evidence` refuses line-count-changing patches loudly (contract
  review); a future template needing one extends the walk to track the
  region delta — the assertion message says so.
- Checker rule-7 expectations are opt-in by design; every registered use
  MUST pass `--expect-patch/--expect-refuse` (the turn-7 prereg does).
- Still open from earlier turns: `BadAttr` parser tightening; the
  judge-shaped "true-but-irrelevant evidence" endpoint (its relevance
  GROWS now that quotes are real); mixed v4+v5 `run_boot_g5_probe`
  integration test; prune-tool `mtp` debt; RunPod account facts — ssh
  keys are per-pod env (never account state), and the S3 key rotation
  handoff item stands.

**Process lessons:** the named bug class struck once more in my own
verification (a cross-version byte-identity check whose `cd` persisted —
master compared against master, "identical" without information; caught
and redone); the adversarial reviewer's F-1 (no eval-time instrument
binding) was the turn's one HIGH and was closed with its own attack as
the regression test BEFORE the floors locked; every review finding was
closed pre-lock at zero amendment cost — the hold-until-verified
discipline keeps paying.

## `/v1` honest refusal of unimplemented fields (2026-08-31)

Found by the hermes-consumer spike of 2026-08-30, live and not by review:
a request carrying 37 tool definitions was accepted, its `tools` array
silently dropped (`ChatCompletionReq` has no `deny_unknown_fields`), and
answered **HTTP 200 with empty content and `finish_reason: "stop"`** — a
success envelope asserting normal completion for a request whose meaning
never reached the model. That is the silent-truncation class the README's
opening paragraph condemns in other people's serving layers, inside
bloomery's own shim, and reachable by any OpenAI client rather than only by
hermes.

**DELIVERED** — `reject_unsupported` refuses, by name and with `param` set,
every field whose meaning this shim would otherwise discard: `tools`,
`tool_choice`, legacy `functions`/`function_call`, `temperature`, `top_p`,
`n`, `stop`, `response_format`, `logprobs`, plus the message shapes it
cannot render (`role: "tool"`, assistant `tool_calls`, and `content` that is
`null` or an array of parts — the last two previously surfaced as an opaque
`invalid_json` parse failure that told the caller nothing).

The governing rule is **accept the no-op value, refuse the meaningful one**:
a value that happens to describe what bloomery already does is honest to
accept, so `tools: []`, `temperature: 0`, `top_p: 1`, `n: 1`,
`response_format: {"type":"text"}` and `logprobs: false` all still pass. The
sampling entries matter more than they look — the substrate samples with
`LlamaSampler::greedy()`, so a `temperature: 0.8` was previously honored in
appearance only, which is the same defect as the `tools` drop wearing
plausible clothes.

17 tests, TDD'd (the first was watched failing against exactly the 200-with-
dropped-tools body quoted above). Three mutants killed: dropping the
empty-check from the `tools` guard, widening the `temperature` guard to
reject `0`, and disabling the content-shape check — each killed by exactly
the accept-side test written to guard against over-rejection, and by no
other.

**Carried, not fixed here:**

- **The pager lock is held across `infer`** (`api_v1.rs`, `lock_pager_v1`
  then `p.infer`). Observed live on 2026-08-30: while one inference was
  stuck, `GET /status` hung too — so any slow or stuck inference takes down
  the whole HTTP surface, including the endpoint an operator would use to
  diagnose it. Blast radius is a design property and is independent of
  whatever stalled that day; its own slice.

  **Spiked 2026-09-01, read-only, before committing to that slice — and the
  spike narrowed it sharply. Five findings, recorded so none is re-derived:**

  1. **`/status` is trivially separable.** `Pager::status()` takes `&self`
     and is pure in-memory reads over the agent and model tables; it never
     touches `self.substrate`. Its one call-like term, `(self.free_vram)()`,
     is a boot-time captured constant — `main.rs` does a one-shot
     `free_vram_bytes(...)` read and then `Box::new(move || probe)` — which
     is the *Static VRAM budget convention* standing ruling holding. No I/O,
     no FFI.
  2. **A cheaper lock mode does not fix it.** `Mutex` → `RwLock` is the
     obvious move and it fails: `infer` holds the write guard for the whole
     generation, so readers queue exactly as they do now. Serving `/status`
     during a stall requires state living *outside* the pager's lock, not a
     different lock mode over the same state.
  3. **The wedge-*relief* case is not reachable by any locking change,
     including the actor rewrite.** `DELETE`/`suspend` need
     `destroy_context`, a `Substrate` method on the very context the stuck
     call is inside. The actor pattern that `llama_send.rs`'s safety clause
     (d) names as the fallback does **not** help: the actor thread would be
     blocked inside the FFI call, so a control message would sit in its
     mailbox exactly as it now sits behind the mutex. **The blocking element
     is the GPU call, not the lock.** Clause (d) offers the actor rewrite for
     a *thread-affinity fault* — a different problem — and it should not be
     spent on this one.
  4. **Cancellation is the only real relief, and it is a substrate slice.**
     `abort_callback` is a `llama_context_params` field and is present in the
     bindgen'd `llama-cpp-sys-2` bindings, but the safe `llama-cpp-2`
     `=0.1.154` API does not expose it at all (its only abort-capable
     callback is model-*load* progress). Reaching it means building context
     params at the sys level or patching/upgrading the wrapper, with its own
     soundness argument — a `bloomery-substrate` change, not a daemon one.
  5. **The cheap option leaves the `unsafe` block alone.** Publishing state
     beside the pager lets no second thread touch the substrate, so
     `SendLlama`'s property (a) — exclusive access is structural — holds
     unamended. The two-tier-lock and actor options both *would* reopen that
     argument.

  **Recommendation of record: the observability fix, not the concurrency
  fix.** Publish a status snapshot plus an in-flight record (route, agent id,
  started-at) updated under the pager lock and read without it, with an
  explicit `as_of` so it is honest about staleness rather than posing as
  live. `/status` then answers during a wedge with what is stuck and for how
  long, which is what the 2026-08-30 incident actually needed. It explicitly
  does **not** let anyone delete or suspend the stuck agent; the only relief
  stays a process restart, priced honestly in the README (KV images are
  boot-scoped, so a restart is a cold start for every agent).

  **Cancellation deliberately not built yet.** The very next item below
  records that this stall never reproduced and says not to chase it without a
  fresh occurrence. Building cancellation for a single unreproduced event
  would be speculative; the observability fix is the right first move because
  it is what makes a *next* occurrence diagnosable — the precondition that
  item already sets.
- **The stall itself did not reproduce.** Six subsequent inferences
  succeeded, including a byte-for-byte replay of the failing request on a
  cold boot. Best-supported but unproven explanation is a one-time Vulkan
  pipeline compile (NVIDIA `[vkrt]`/`[vkps]` driver threads were present,
  `~/.cache/nvidia/GLCache` open); CPU at ~0% argues against it. Do not
  chase without a fresh occurrence.
- **`/v1` still implements no tool calling.** This slice makes the refusal
  honest, nothing more. The adapter that would actually serve a
  tool-calling client is specified in
  `docs/superpowers/specs/2026-08-31-openai-tools-adapter-design.md` and
  deliberately lives outside the daemon.

## OpenAI tools adapter — live acceptance (2026-08-31, Task 6)

The adapter's human-gated live run, the first time any of its code touched a
real model. Verdict **PARTIAL**, recorded in
`docs/superpowers/evidence/2026-08-31-openai-adapter-acceptance.md`.

**Delivered and measured.** A correct tool call end to end on the first
attempt; the untrained `qwen36-reap48-ours` selected sensibly from both a
25-tool and a 132-tool schema. The prefill-once property — the adapter's
whole economic justification — is confirmed against a live daemon:
**40,003 → 21 `prompt_tokens` across two turns at the real 132-tool hermes
scale, ≈39,982 tokens not re-prefilled.** The honest-refusal chain is proven
to the real client, which displayed bloomery's byte arithmetic unaltered.

**Carried, each with its own reason for not being fixed here:**

1. ~~**A client retry is misclassified as a history rewrite.** hermes sent a
   byte-identical request twice (both hashing `493b2c9dbc94ceff`). After
   request 1, `record_generation` had appended the assistant turn, so the
   retry's `[system, user]` was *shorter* than the tracked
   `[system, user, assistant]`; `_is_extension` correctly concluded "not an
   extension" and reset. The classification is right in isolation — Task 3's
   reviews were correct to demand a reset on a shorter list — but an ordinary
   retry is indistinguishable from a truncation once the adapter has appended
   its own turn. **Nobody considered the retry case**, and no fake produced it
   because fakes echo. The plausible treatment (recognise a prefix differing
   only by the assistant turn we appended, and re-serve it) needs its own spec
   amendment, its own tests, and a decision about whether a retry should
   replay the previous answer — so it is a slice, not a patch.~~
   **DELIVERED 2026-08-31**, and it did take the slice this item predicted:
   the spec amendment landed first (`ebbe73c`, "the retry state, from the live
   acceptance run"), then the classification (`5ff73c3`, a byte-identical
   retry is classified as a retry rather than a rewrite), then the correction
   the first fix's own reviews demanded (`7a08546`: a retry must match on
   `tools` too, and a reset must suspend before it creates). The decision this
   item said was needed — whether a retry replays the previous answer — was
   taken in the spec amendment rather than left implicit in code. Three of the
   four defects the arc closed were **silent**: the task completed correctly
   on every run, so nothing looked wrong while the adapter's whole economic
   justification produced nothing (`0213362`).

2. **The tier fits exactly one context, and it is smaller than hermes needs.**
   The pager's static boot budget is 14,064,746,496 B; after 1 GiB overhead
   and 10.95 GiB of resident weights, **1.15 GiB remains for contexts** — a
   ceiling of ≈34,000 tokens, one at a time. A 65,536-token window cannot be
   placed at all from a cold start, and hermes's 132-tool preamble (40,003
   tokens, measured) exceeds the ceiling. Its 25-tool default set (≈14,077)
   fits. This is an honest tier limit, not a defect, but it means finding 1
   fires hard: every reset needs the previous agent gone first.

   **Still open, with what moved around it (2026-08-31).** The ceiling is
   unchanged — it is a property of the tier, and no code in this repo can
   raise it. What changed is the consequence named in the last sentence:
   `7a08546` makes a reset suspend the previous agent *before* creating the
   next, so a reset no longer needs two contexts to be placeable at once.
   The tier still fits exactly one context; a reset no longer asks it for two.

3. ~~**Agent accumulation is now demonstrated rather than theoretical.** Each
   failed attempt left an agent behind, reaching `a7`; they were cleared by
   hand with `POST /agents/{id}/suspend`. bloomery still has no
   `DELETE /agents/{id}` — recorded previously against the adapter's design,
   now with a live reproduction.~~
   **DELIVERED 2026-08-31** (`agent-delete-endpoint`, this file's first
   carried-debt slice). `DELETE /agents/{id}` is one arm in
   `api_native::dispatch` over the `Pager::remove_agent` that already existed
   for `/v1`'s ephemeral cleanup: 204 when the agent was there (resident,
   suspended or fresh alike), 404 `unknown_agent` when it was not. The
   workaround this item records is exactly what the endpoint replaces —
   `suspend` *parks* an agent, keeping its id, its table entry and its KV
   image, so clearing seven leaked agents with it cleared nothing. That
   substitution is the sharpest mutant the new tests kill: replacing
   `remove_agent` with `suspend` in the handler still answers 204 and is
   caught by three independent tests, not by the status code.

   Pinned by `api_native_agent_delete_test.rs` (8 tests, TDD'd; the RED run
   answered the router's `not_found` for all eight). Four mutants killed, each
   by exactly the test written for it: an idempotent 204 on an unknown id, the
   `suspend`-instead-of-remove substitution above, an over-broad route arm
   (`["agents", id, ..]`, which would have swallowed the POST sub-resources),
   and a reworded journal reason. The pager-layer semantics were already
   pinned by `pager_remove_agent_test.rs` and are deliberately not re-pinned.

4. ~~**No parse-rate statistic is claimed.** The pre-registered question ("a
   poor parse rate is a finding, not a failure") is **unanswered**, not
   answered favourably: finding 1 ended the session before a multi-turn
   trajectory existed. Every tool call observed was well-formed, but the
   sample is far too small to be a rate.~~
   **ANSWERED MODESTLY 2026-08-31** and recorded as such in `0213362`'s
   addendum to the acceptance doc — the multi-turn trajectory finding 1 had
   prevented became reachable once finding 1 closed. The addendum answers the
   pre-registered question *and says how weakly*: it is a small-sample
   observation, not a rate. The item is struck because the question is no
   longer unaddressed, not because a statistic now exists.

**What earned its keep.** The structured per-request logging added as
Important 4 of the final whole-branch review — added precisely *because*
Task 6 had never run — is what made finding 1 visible at all. Without the
session/agent/reset line per request, the symptom was an opaque 409.

**Process note.** The `pkill` self-match hazard bit twice during this run,
including through the `[o]penai` bracket trick, because the literal pattern
also appeared elsewhere on the same command line. Kill by PID from `ps`
output instead.

**Recorded 2026-08-31, the streaming non-goal (delivered, and it should
never have been a non-goal).** The adapter's spec carried "no streaming" as
a deliberate non-goal until `b7bd4f7` withdrew it and said why the reasoning
was wrong: the evidence for it came from **failure-only artifacts** — the
captures that motivated it were of requests that had already failed, so they
showed no streaming because there was nothing to stream, not because the
client did not ask for it. Buffered SSE shipped in `09a2279`. Recorded here
because a non-goal argued from a biased sample is a measurement error, and
this file is where those are kept; it appears in no other list.

Its own carry is already in *Smaller items* above: the buffered
implementation emits one final chunk with `finish_reason` and usage merged,
rather than OpenAI's two-chunk trailer.

## `DELETE /agents/{id}` (2026-08-31, branch `agent-delete-endpoint`)

The first slice taken purely to pay this file down. Delivered: the endpoint
struck through as item 3 of the section above, plus this file's own
amendment for the four items its tail had gone stale on.

**Carried, deliberately, and not fixed here:**

- **`DELETE` takes the pager lock, so it queues behind a stuck `infer`.**
  This is the same blast radius the `/v1` section above records against
  `api_v1.rs`, reached through a new door: an operator whose daemon is wedged
  on an inference cannot delete an agent to relieve it, for the same reason
  they cannot read `/status` to diagnose it. The endpoint is now the **third**
  named beneficiary of that slice, after `/status` and `/v1` — which is an
  argument for doing it, not for pre-solving it badly inside a route addition.
  No new defect: `DELETE` is as lock-bound as `suspend` and `resume` always
  were.
- **No bulk or conditional removal.** No `DELETE /agents` sweep, no
  "delete every agent older than N", no filtering. The live reproduction that
  motivated this endpoint wanted seven specific ids gone; a sweep is a
  different feature with a different blast radius, and nothing has asked for
  it yet.
- **No caller-supplied reason.** `http.rs`'s request parser drops the query
  string before `dispatch` sees it, so `?reason=` cannot reach the handler
  without a parser change. The journal records a fixed string naming the
  surface, which is what distinguishes an operator deletion from a `/v1`
  ephemeral cleanup or an `unregister_model` cascade — all three land as the
  same `AgentRemoved` event.

**Found while landing it, unrelated — fixed on `master`, not in this slice.**
`master` was red. `committed_g2_journal_still_replays`
(`bloomery-core/tests/journal_test.rs`) sweeps every `*.jsonl` directly under
`docs/superpowers/evidence/` and replays each as a bloomery journal — a
deliberate design, so a future committed journal is covered without anyone
remembering to add a case. `0213362` committed
`2026-08-31-openai-adapter-acceptance-hermes-capture.jsonl` into that
directory, which is a **hermes wire capture** (`{"tag": "response", "body":
{...}}`), not a journal, and it does not replay.

**FIXED 2026-08-31 on `master` (`2558ead`), as a filing decision rather than a
test change.** The capture moved to `docs/superpowers/evidence/captures/`,
which the non-recursive sweep never descends into; the acceptance doc's
artifact list carries the new path and the reason. Weakening the filter — skip
`*-capture.jsonl`, or match only `*-journal.jsonl`/`*-tasks.jsonl` — was the
alternative and is worse: it converts a strong invariant (anything named
`*.jsonl` directly under `evidence/` must replay) into an opt-out a future
misfiling can take by accident, which is the failure the test exists to catch.
The convention is now recorded in the sweep's own comment, where the next
person will look. The test was never at fault; it caught a misfiled artifact
on the day it was misfiled.

Kept out of this slice's commits deliberately, so an unrelated repo-wide fix
is not buried inside an endpoint's PR.
