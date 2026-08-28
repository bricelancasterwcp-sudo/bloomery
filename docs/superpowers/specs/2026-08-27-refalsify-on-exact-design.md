# Refalsify-on-exact — a task-scoped verification probe before injection

**Date:** 2026-08-27
**Status:** Approved in conversation (rulings: execution locus = in-task,
under the incoming task's grant, at the worker's retrieval moment, through
the task loop's own `exec_run` — never daemon-spontaneous, no operator
sweep in this slice; ungranted = inject anyway, stamped `skipped_ungranted`
— refalsification upgrades where possible, never shrinks reach below the
battery-passing behavior; outcomes = clean nonzero exit → `mark_contradicted`
citing the incoming task + silence, exit 0 → inject, no record-schema
change; activation = `[memory] refalsify`, default `false`. Judgment calls
presented and accepted with the design: an inconclusive probe — timeout or
spawn failure — injects and stamps `inconclusive`, never contradicts; a
demoted/read-only task never probes and takes the ungranted-class skip.)
**Lineage:** the memory organ
(`docs/superpowers/specs/2026-08-26-memory-organ-design.md`), whose §5
declared falsification passive-only, named crucible's active
re-falsification "a possible later, operator-triggered slice," and banned
spontaneous daemon-initiated execution — this slice IS that later slice,
with the execution boundary drawn at a task's granted capability instead of
an operator trigger. Crucible S3's falsification scheduler
(`crucible/docs/superpowers/specs/2026-08-24-crucible-s3-memory-design.md`
§4): `refalsify(item)` re-runs the lesson's cited verification; pass keeps
the item, fail removes it from retrieval permanently — the shape ported
here, minus the sleep cadence bloomery does not have. Crucible GATE-B
(verdict **GO_B**: store-only exact-class retrieval lifts second exposures
0.780 → 0.905 on a 14B; the exact-only probe carries Δ_second +15.0 pp)
and GATE-C (verdict **NO-GO**: symptom-conditioned non-exact transfer fails
for want of material) — together the standing prohibition this slice
honors: THE MATCH IS NEVER LOOSENED; refalsification layers trust on the
exact path, never reach beyond it. The memory battery
(`docs/superpowers/evidence/2026-08-27-memory-battery-findings.md`): GATE
PASS under inject-without-refalsify (both arms 100/100, repeat cost
121.5 → 111.5 tokens); its §5 — the two contradictions fired honestly, but
only AFTER a receiving task had already failed — is the wasted run this
slice moves detection ahead of; its §4 advisory — injected repeats ~7%
SLOWER by wall (prefill > decode at this size) — is the cost story §6
below must extend honestly.

## 1. What this builds and why

Today the organ trusts an exact match completely: goal hash and cited-file
fingerprints line up, the episode injects. The fingerprint gate catches
workspace drift on cited bytes — but nothing catches an episode whose
verification no longer holds for reasons the citation set cannot see:
uncited dependencies, environment drift, an external service, a moved
interpreter. Those episodes today inject stale guidance, and the organ
learns the truth only passively — after the receiving task fails, which is
exactly one wasted task per stale episode (battery findings §5 measured
this path firing live).

This slice adds crucible's missing half: at retrieval, before injection,
the worker re-runs the episode's own stored verification command
(`EpisodeRecord::run_evidence.argv` — already in every record, no schema
change) under the incoming task's granted capability. An episode that
still proves itself injects; one that cleanly fails is contradicted on the
spot and the task proceeds memory-silent, exactly as a stranger would.

**Claim discipline.** This slice claims mechanism only: the probe runs
where and when specified, the four outcomes stamp and act as specified,
and flag-off behavior is indistinguishable from today's. It claims nothing
about task success rates, wall-clock economics, or whether earlier
contradiction detection pays for its execution cost — that is a future
battery slice with its own pre-registration. No number from this slice's
tests may appear in a capability sentence. GATE-C's standing prohibition
is untouched: no non-exact retrieval mode exists, before or after this
slice.

## 2. The probe

At the worker's existing seam (`task/registry.rs`: retrieve → stamp →
inject → run → mint-or-contradict), after the two-stage exact gate passes
and before rendering or stamping, with `[memory] refalsify = true`:

1. **Coverage pre-check.** The episode's `run_evidence.argv` is checked
   against the incoming task's `Grant` command prefixes — the same
   coverage rule `exec_run` enforces — BEFORE any execution attempt. The
   pre-check exists because an `exec_run` grant refusal is an
   `Observation` shaped like a failed run, and a refusal must never be
   mistakable for evidence. A task whose grant does not cover the argv —
   and every demoted/read-only task (`mutating_verbs == false`), which
   may not have commands executed at its moment regardless of what its
   grant says — skips the probe entirely: the episode injects as today,
   and the stamp says `skipped_ungranted`. The demotion boundary outranks
   refalsification.
2. **Execution.** A covered probe runs through the task loop's own
   `exec_run`, with the incoming task's `Grant`, `cwd`, and `ExecBounds`
   — the identical executor, capability check, output cap, and
   `run_timeout_secs` a task's own `run` verb gets. One command, bounded;
   the worst wall cost is one capped execution.
3. **Verdicts.**
   - **Exit 0** → inject, stamp `passed`. No record change, no
     re-append: retrieval is exact single-hit with no ranking to feed,
     and the stamp is the durable evidence (YAGNI ruled against a
     crucible-style `last_verified_at`).
   - **Clean nonzero exit** → `MemoryStore::mark_contradicted(episode_id,
     incoming_task_id)` — the same method, status transition, and
     `Event::MemoryContradicted` row passive contradiction already uses —
     and NO injection: the task runs memory-silent, byte-identical to a
     stranger's prompt. Stamp `failed`. A command that fails on a
     workspace whose cited bytes are fingerprint-identical to the mint is
     real evidence, and the store's honesty story keeps it.
   - **Timeout or spawn failure** → inject, stamp `inconclusive`. Not
     clean evidence the lesson is wrong — environmental, not semantic —
     and the organ's own law ("total failure must be indistinguishable
     from memory-off; never a failed task") forbids the probe's
     infrastructure from costing a task its injection. Only a genuine
     nonzero exit ever contradicts. The fail/inconclusive distinction
     must come from the executor's own outcome classes: if `exec_run`'s
     `Observation` distinguishes timeout and spawn failure only by its
     pinned outcome strings, matching those exact pinned constants is
     acceptable (they are load-bearing constants in this codebase, not
     free prose); if it cannot distinguish them at all, the plan must add
     the distinction at the executor seam rather than guess from text.

The match semantics, single-injection cap, retrieval ordering, mint bar,
passive-contradiction path, and rendered memory block are all untouched.
A probe never appears in the transcript, never renders into any prompt,
and never journals a `TaskStep` — it is not a model action.

## 3. The law, revised

Organ spec §5's "the organ never executes anything" is revised to: **the
organ never initiates execution.** Refalsification is a task-scoped probe
the worker performs with the incoming task's own granted capability, at
that task's moment, through the same executor the task's own verbs use.
Daemon-spontaneous execution stays banned; the operator-triggered sweep
variant named by §5 stays out of this slice (no workspace-of-record to run
it in, and nobody has asked). Everything else the organ does remains
read-outcomes-only.

## 4. The ledger

`Event::MemoryStamp` gains one additive field:
`refalsify: Option<String>` — `None` when the flag is off, when memory is
off, or when nothing was retrieved (absent-key serde default, so every
existing journal row replays unchanged — the house additive pattern);
`Some("passed" | "failed" | "skipped_ungranted" | "inconclusive")` when a
retrieval hit was probed or skipped. A `failed` stamp is always
accompanied by the ordinary `Event::MemoryContradicted` row citing the
incoming task, so the journal walks in both directions exactly as §4 of
the organ spec promises. No new counters: `/status`'s `contradicted`
already moves when a refalsification fails, and `GET /memory` already
shows the status flip.

## 5. Activation

`[memory]` gains `refalsify` (bool, default `false`), read only when
`enabled` is true. Flag-off behavior is bit-identical to today — the
battery's GATE PASS was measured under inject-without-refalsify, and an
enabled organ keeps that exact behavior until the operator opts in. Every
frozen instrument runs memory-off and is doubly untouched. A future
repeat-exposure battery slice may pre-register refalsify-on as its own
lens; this spec deliberately leaves that instrument undesigned.

## 6. Cost honesty

Battery findings §4: injected repeats are already ~7% slower by wall
(prefill > decode at this size); the token saving, not a wall saving, is
the measured benefit. Refalsify-on adds one bounded command execution to
every probed retrieval on top of that. This spec makes no argument that
the trade is favorable — it makes the trade available, gated off, and
measurable later. The one cost asymmetry worth naming: passive
contradiction spends a full failed task to learn what a probe learns in
one command.

## 7. Error handling

- Journal write failures on the stamp stay exactly as the worker treats
  them today (the organ's failure is never the task's failure).
- A store append failure while marking contradicted follows the store's
  existing failure handling; the injection decision (silence) stands
  regardless — the task must not receive guidance the probe just refuted,
  even if recording that refutation failed.
- The probe honors `ExecBounds` verbatim; there is no refalsify-specific
  timeout knob (YAGNI — the task's own bounds are the task's tolerance).

## 8. Testing

All GPU-free: real store, real journal, real subprocess probes via
trivially-true/false argv (`["true"]` / `["false"]` or equivalent), the
existing memory task-test fixtures:

1. **Flag-off identity** — `refalsify = false` (and absent): behavior and
   stamps byte-identical to today; existing memory suites pass untouched.
2. **Pass path** — granted `["true"]` probe: injected, stamp `passed`,
   store untouched.
3. **Fail path** — granted `["false"]` probe: NOT injected (prompt
   byte-identical to memory-silent), episode `contradicted` with
   `contradicted_by` = the incoming task's id, `Event::MemoryContradicted`
   journaled, stamp `failed`; a subsequent identical task retrieves
   silence.
4. **Ungranted skip** — argv outside the task's grant: injected, stamp
   `skipped_ungranted`, no execution attempted (asserted via the probe
   seam, not inference).
5. **Demoted skip** — `mutating_verbs == false` with a covering grant:
   same as 4.
6. **Inconclusive** — a probe that exceeds `run_timeout_secs` (e.g.
   `["sleep", "..."]` with a 1s bound): injected, stamp `inconclusive`,
   episode stays `verified`.
7. **Stamp compat** — journal rows without the field replay unchanged;
   a stamped row round-trips.
8. **Config parse** — `[memory] refalsify` parses; absent → false.
9. **Mutation spot-checks** — the skip-vs-fail boundary (a swapped
   pre-check must fail test 4), the flag gate (ignoring the flag must
   fail test 1), and the contradiction citation (wrong task id must fail
   test 3).

Acceptance: full workspace `cargo test` green, zero ignored; featured
binary rebuilt after the last test run (box rule).
