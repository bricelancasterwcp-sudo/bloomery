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
- A full corpus run leaves ~999 zero-byte `.lock` files in the temp dir; the
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
  **Amended 2026-08-19 (swap-candidate):** still open, and the parenthetical
  is now false — `pager_test.rs` is 1247 lines and **eight** test files are
  over the ceiling. The original text stands as recorded; the swap-candidate
  section's *Recorded, not fixed* carries the full count as of 2026-08-19.
  The remedy is unchanged and now project-wide, not one file's chore.
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
