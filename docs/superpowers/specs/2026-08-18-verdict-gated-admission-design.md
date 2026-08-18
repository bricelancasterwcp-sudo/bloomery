# Verdict-gated admission — the capability-vector seam, slice 2

**Date:** 2026-08-18
**Status:** Approved in conversation (scope: refuse on confirmed
regression; refusal set: `Confirmed` only; recovery: the reading and
the block are separate, the block is operator-clearable in-boot).
**Lineage:** the drift watch (slice 1, PR #12, live-accepted
2026-08-17) which built the comparison, the confirm, and the seven-word
outcome vocabulary this slice reads; law 5's measured admission, whose
current form is existence-gated; assay v1.8 (exit 3, `verdict.parallel`,
schema v9), whose upgrade lands against every blessed v8 reference and
is the hazard §6 exists to defuse.

## 1. What this builds and why

Slice 1 measures drift and says so. Nothing acts on it: `Pager`'s
`drift` field carries the comment "Never read for enforcement," and
`admit()` still asks one question — does a profile exist? A model whose
serving path has demonstrably regressed is admitted with exactly the
confidence of one that has not.

This slice makes the seam load-bearing in the narrowest way that is
honest: **a confirmed regression refuses new admission, and nothing
else does.** Six of the seven drift outcomes continue to admit, each
with its reason named and rendered, because only one of them asserts
that a regression was established.

The design's whole difficulty is recovery. Slice 1 fixed, correctly,
that a reading is immutable — "a comparison nobody re-ran must never
acquire a new verdict" — and that blessing takes effect at the *next*
boot. Enforcement built naively on top of that would give an operator
no way to admit a model they have judged benign without restarting the
daemon. §3 resolves this by separating the measurement from the policy
derived from it.

## 2. The refusal set

`admit()` gains one clause. It currently reads: a profile exists →
admitted; otherwise the POST window or `allow_unprofiled` or
`PagerError::Unprofiled`. It becomes: a profile exists **and no
admission block stands** → admitted; the rest unchanged.

**Which of the two comparisons blocks.** `ModelDrift` carries two
statuses — `step` (this boot vs the previous boot) and `cumulative`
(this boot vs the blessed baseline). **The block is set on
`cumulative == Confirmed`, regardless of what `step` reads.** Three
reasons, and they agree:

- Cumulative is measured against the baseline an operator accepted.
  "This is now measurably worse than the state you blessed" is the
  claim that should hold a model out; "it moved since last boot but is
  still within noise of what you accepted" is not.
- Bless re-baselines the cumulative reference, which is what makes
  bless the coherent "this is the new normal" recovery in §4. A block
  keyed to a reference bless does not touch would be unrecoverable by
  the route built to recover it.
- Step's reference auto-advances: next boot compares against *this*
  boot, so a persisting regression reads `WithinNoise` on step the very
  next boot. A step-keyed block would therefore clear itself after one
  boot whether or not the regression went away — a block more transient
  than the fault it names. Slice 1 says it plainly: step "alone leaks
  the ratchet," and a ratcheting degradation is exactly what
  enforcement exists to catch.

`step` is still measured, journaled and rendered exactly as slice 1
ships it. It simply never blocks.

The block is set when, and only when, that cumulative comparison
settles `DriftStatus::Confirmed`. Stated as the full table, because a
policy over a seven-word vocabulary must be enumerated rather than
described:

| `DriftStatus` | admits? | why |
| --- | --- | --- |
| `WithinNoise` | yes | nothing moved beyond assay's noise discipline |
| **`Confirmed { reference }`** | **no** | a regression was measured and reproduced — the one established fact in the vocabulary |
| `Transient` | yes | the serving state moved between two probes and did not reproduce; a finding, but not an established regression, and assay's founding finding is that this daemon's failures can be state-transient |
| `Unconfirmed { reason }` | yes | the confirm could not be made; the instrument declined to conclude, so admission must not conclude for it |
| `NotComparable` | yes | assay's exit 2 — infrastructure-shaped, no drift hypothesis was ever tested |
| `InstrumentChanged { .. }` | yes | slice 1 §3: "never a pass, never a fail" — and see §6 |
| `Unmeasured { reason }` | yes | there was nothing to compare, including first boot ever; refusing here would mean a fresh install admits nothing |

The principle behind the table, which is the rule to apply if an eighth
outcome is ever added: **refuse only what was established; name
everything else.** This extends slice 1's "absence is never a verdict"
to "absence is never a refusal." An outcome that declines to conclude
must not be laundered into a conclusion by the admission path.

## 3. The reading and the block are separate fields

`Pager`'s per-model entry gains `admission_block: Option<AdmissionBlock>`
beside the existing `drift`. `AdmissionBlock` carries what refused and
the reference identity that refused it, so the 422 can name its cause
without re-deriving it.

This separation is the slice's central design decision and it is not
incidental:

- **The reading stays a measurement.** `drift` is written once, when
  the watch settles it, and is never rewritten by an operator action.
  Slice 1's bless route already established that rule and the reason
  for it.
- **The block is a policy derived from the reading** at the moment it
  settles. A policy may be overridden by the operator who owns the
  machine; a measurement may not.
- It mirrors the separation design §7 already keeps between
  `done_trust` and `drift` — different questions, different fields,
  neither borrowing the other's authority. `done_trust` remains the
  sole property of the G4 codec gate and the G5 refusal gate; this
  slice does not read or write it.

An operator reading `/status` sees both: what was measured, and whether
it is currently holding the model out.

## 4. Two operator actions, deliberately distinct

`POST /models/{name}/bless` keeps its documented contract **exactly** —
it replaces the reference the next boot's cumulative gate reads,
journaled with the profile's identity, and it does not recompute this
boot's reading. Nothing in this slice changes that route's behaviour or
its 200/404/409/500 table.

`POST /models/{name}/unblock` is new. It clears this boot's admission
block, journals the clearing with operator provenance
(`PROVENANCE_OPERATOR`, which slice 1 already defines), and touches
neither the reading nor the baseline.

| outcome | status | body |
| --- | --- | --- |
| cleared | 200 | `{model, cleared: <what was blocking>}` |
| no such model | 404 | the surface's one `unknown_model` shape |
| no block to clear | 409 | `{error: "no_admission_block", model, detail}` |

The 409 is load-bearing for the same reason bless's is: answering 200
where nothing was blocking would tell an operator they had cleared
something when nothing was written.

The two routes answer different questions — *"this is the new normal"*
versus *"I know, let it run anyway"* — and neither implies the other.
The consequences are intended and worth stating: unblocking without
blessing means next boot's confirmed regression blocks again, because
the regression is still real and still unaccepted; blessing without
unblocking leaves this boot blocked and the next one clean.

## 5. Refusal shape and what does not change

A blocked model refuses at agent creation with a new
`PagerError::DriftBlocked { model, reference }`, rendered as **422** —
matching the existing `Unprofiled` refusal rather than bless's 409,
because it is the same class of answer (this model cannot be admitted
now) on the same path clients already handle. The body names drift as
the cause, so the two refusals are distinguishable without a new status
code.

**The variant must be mapped on BOTH surfaces**, which is where a
half-done job would hide: `api_native.rs` renders `Unprofiled` as
`{error: "unprofiled", model}`, and `api_v1.rs` renders it through
`error_envelope` as `invalid_request_error` / `model_unprofiled` with a
sentence and a `"model"` param. `DriftBlocked` needs both, in each
surface's own idiom — `drift_blocked` and `model_drift_blocked`
respectively — and the v1 sentence should name the reference the block
carries, so an operator reading an OpenAI-shaped error still learns
which baseline refused them. A `PagerError` variant handled in one
surface and not the other is a 500 waiting for whichever client hits
the unmapped path.

Unchanged, and each for a reason already settled:

- **The gate is at agent creation, never per inference.** An agent
  admitted before a block appeared keeps working. Cutting a live
  conversation mid-turn because the watch settled would be its own
  dishonesty — the same argument that already governs the POST window.
- **The POST window is unaffected.** No drift has settled while POST is
  still probing, so nothing blocks, exactly as today.
- **`allow_unprofiled` stays orthogonal.** Unprofiled (no profile) and
  drift-blocked (a profile, and a reproduced regression against it) are
  different refusals with different reasons; neither flag or route
  answers the other's case.
- **`done_trust`, `codec_gate` and the G4/G5 gates are untouched.**

`drift_watch.rs`'s header comment — "Nothing here touches `done_trust`,
`codec_gate` or admission. Design §7 is …" — is precisely what this
slice repeals, and it must be rewritten rather than left to rot: the
module now touches admission and only admission, and `done_trust`
remains elsewhere's property.

## 6. The assay pin upgrade, and why it is now dangerous

Slice 1 moved the pin from `74c5b71` to assay 0.9.0 / schema v8 and
handled the consequence in its §6: the first post-upgrade boot reads
`instrument-changed` on every model with an old-schema reference, and
the operator re-blesses per model.

assay v1.8 (0.10.0, schema v9) is merging now. The daemon is pinned by
`PYTHONPATH` to the assay **source tree**, so it begins producing
`0.10.0/v9` profiles the moment that lands, while every blessed
reference reads `0.9.0/v8`. `instrument_precheck` compares both
`probe_version` and `schema_version`, so **the first boot after the
merge reads `InstrumentChanged` for every model at once.**

Under slice 1 that was cosmetic. Under enforcement it is the failure
mode that would take the entire fleet out on a routine instrument
upgrade. §2's table therefore admits on `InstrumentChanged`, and §7
makes that a test rather than an intention.

Nothing converts old profiles; old references are superseded by
blessing, never rewritten — slice 1's rule, restated because this slice
depends on it.

Two further v1.8 facts are recorded here as **not consumed by this
slice**, so a later reader does not assume they were: assay's new exit
**3** ("incomplete comparison") is unreachable behind bloomery's
existing version precheck, which refuses to run the diff at all on a
mismatched pair; and `verdict.parallel` is a new verdict this slice
does not read, because verdict floors are a different slice.

## 7. Testing posture

House rules apply (TDD; mutation checks under the pyc-equivalent
discipline for Rust — verify each load-bearing test fails when its
pinned line is broken). Specifically:

- **The refusal table, enumerated.** All seven `DriftStatus` values on
  `cumulative` against admit/refuse. Sampling three of them would not
  pin a policy whose whole content is which words mean what.
- **`step` never blocks**: a `step: Confirmed` / `cumulative:
  WithinNoise` reading admits, and a `step: WithinNoise` /
  `cumulative: Confirmed` reading refuses. Two tests, because the
  asymmetry is the one thing about this policy a reader would guess
  wrong.
- **`InstrumentChanged` never blocks**, pinned with the mixed-version
  fixtures slice 1 §8 established, including the real pre-upgrade
  schema as committed bytes — so the first-boot-after-upgrade path is
  tested against the artifact it will actually meet. This is the single
  most important test in the slice.
- **An agent created before the block survives it**, and new work on
  the same model afterwards is refused.
- **`unblock` admits, and does not alter the reading**: after clearing,
  `ModelStatus.drift` still reads `Confirmed` with the same reference.
- **`bless` does not clear the block; `unblock` does not re-baseline.**
  One test each, because the two routes' independence is exactly what
  a future reader would assume away.
- **Both error surfaces render `DriftBlocked`**, neither falling
  through to a 500 — one test per surface, because the defect this
  catches is invisible from the other one.
- **Journal completeness**: a block set and a block cleared each appear
  exactly once, with provenance, and a replay can reconstruct which
  models were held out and why.

## 8. Non-goals

Verdict floors (admitting on measured capability rather than on
drift), swap admissibility, and routing — the other three enforcement
candidates slice 1 §7 named. Each is a separate slice with its own
spec.

Reading `verdict.parallel` or any other assay verdict. Consuming exit
3. Separating `Infra` from `Unmeasured` (carried debt from slice 1's
Task 4, which the debt file notes "the enforcement slice wants apart")
— this slice does not need them apart, because it refuses on
`Confirmed` alone and both fold into an outcome that admits. It stays
open, and a verdict-floors slice will want it.

Any change to `done_trust`, the G4 codec gate, the G5 refusal gate, or
law 5's existence requirement, which remains in force underneath the
new clause.
