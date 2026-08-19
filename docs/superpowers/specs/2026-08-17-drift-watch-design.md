# Drift watch — the capability-vector seam, slice 1

**Date:** 2026-08-17
**Status:** Approved in conversation (first-slice choice: drift watch;
on-trip policy: confirm-then-alarm; reference policy: step + cumulative
with operator blessing).
**Lineage:** the POST module (assay probing this daemon's own `/v1` at
boot, law 5's measured admission); assay v1.7 (`assay diff --gate`,
schema v8, and the 2026-08 campaign whose diffs demonstrated the
instrument-changed confound this spec handles); the capability-vector
seam idea queued 2026-08-17 (bloomery consumes assay evidence as
capability vectors, not model names — this is its first slice).

## 1. What this builds and why

POST already produces a per-model assay profile every boot, and law 5
already refuses a model with *no* profile. Nothing yet reads what is
*in* the profile across time: a model (or the serving stack under it)
can degrade boot over boot and the daemon will keep admitting it with
identical confidence. This slice adds the **drift watch**: each boot's
POST profile is compared against two references through assay's own
noise discipline, a tripped gate is confirmed before it alarms, and
the outcome lands in the journal and in `ModelStatus` — observability
first, enforcement in a later slice.

Assay's founding finding binds the design: this daemon's own failures
can be state-transient (the 11.5k ceiling that vanished without a
restart). A single regression reading is therefore never an alarm; it
is a hypothesis the confirm re-probe tests.

## 2. The two comparisons

Per model, per boot, after POST writes this boot's profile:

- **drift-step** — this boot's profile vs the PREVIOUS boot's profile.
  Detects step changes. Alone it leaks the ratchet: a slow
  degradation, each step within noise, never flags.
- **drift-cumulative** — this boot's profile vs the **blessed
  baseline**. Catches the ratchet. Alone it goes stale the moment
  anything legitimately changes.

Both run; each journals its own named outcome. A missing reference
(first boot ever; baseline never blessed) journals `unmeasured` by
name for that comparison — never a silent skip, never a pass.

**Blessing** is an explicit operator action (API/CLI: "bless the
current profile of model M as baseline"), journaled with the profile's
identity. The first successful POST for a model auto-blesses, journaled
as `auto-blessed (first profile)` so the provenance of every baseline
is explicit. Re-blessing replaces the baseline and journals the old
identity beside the new.

> **Footnote, 2026-08-17 (as-built).** The auto-blessing provenance
> ships spelled **`auto-first-profile`**, not the `auto-blessed (first
> profile)` prose above — settled at `254ddb9`, which made provenance a
> *prefix family* (`auto-first-profile`, `operator`, `operator (replaced
> <sha256>)`) rather than a two-string set. The paragraph's requirement
> is unchanged and met: the first successful POST auto-blesses, and the
> provenance of every baseline is explicit in the `Blessed` row.
> Measured live in
> `docs/superpowers/evidence/2026-08-17-drift-watch-live.md` (boot 1).

## 3. Instrument-changed honesty

When the reference and current profiles disagree on `probe_version` or
`assay_profile_version`, the comparison reports **`instrument-changed`**
— never pass, never fail — and the cumulative gate stays in that state
until the operator re-blesses. Rationale is measured, not theoretical:
assay's 2026-08 campaign diffs showed 12 of 15 models "improving"
because the *ceiling cap* changed between probe versions, not the
models. A gate that scored that as drift would train the operator to
ignore it. The first boot after this slice's assay pin upgrade (§6)
exercises exactly this path on every configured model.

## 4. The gate mechanics

- The comparison is `assay diff <reference> <current> --gate` run as a
  **subprocess**, consuming its documented exit codes: `0` within
  noise, `1` drift beyond noise, `2` not comparable. Same decoupling
  POST already practices: the artifact and the exit code are the
  contract; bloomery never parses diff's prose output.

  > **Amendment (2026-08-19):** assay 0.10.0 (its v1.8 wave) added a
  > fourth documented code — `3`, an **incomplete comparison**: a cell
  > measured on exactly one side, outranking a measured drift
  > (precedence `2 > 3 > 1 > 0`). The gate reads it as its own settled
  > verdict, `incomplete` — never a pass, and no confirm run, since an
  > incomplete comparison asserts no drift to reproduce; a later boot
  > that measures the missing cell resolves it. A confirm re-diff
  > exiting `3` follows the existing rule for a re-diff that did not
  > answer: the first reading stands `unconfirmed`, naming
  > `incomplete`. Before this amendment the daemon journaled exit `3`
  > as infrastructure ("undocumented exit") — honest about not
  > understanding it, wrong about the contract: assay's exit-code
  > vocabulary grew under the live pin and this bullet's three-code
  > claim went stale.
- The journal row for every comparison records: model, comparison kind
  (step/cumulative), verdict, the exit code, and **both profile
  paths** — so any human or tool can re-run the identical diff. The
  row never copies numbers out of the profiles (a value that looks
  like a measurement, transcribed, is how transcription errors become
  evidence).
- A FIRST diff exiting `2` journals `not-comparable` with the exit
  code and no confirm run — there is no drift hypothesis to test.
  (§3's version pre-check catches the instrument-changed case before
  diff runs, by reading the two profiles' own version fields — the
  documented artifact, not diff prose; exit 2 here covers diff's other
  refusals, e.g. one-sided tier marking.)
- **Confirm-then-alarm.** On exit 1: re-run the identical POST probe
  invocation for that model to a FRESH file, then diff again against
  the same reference.
  - Second diff also exits 1 → journal **`Confirmed`**; surface in
    `ModelStatus` as the new `drift` field (per-comparison:
    step/cumulative, with the reference's identity).
  - Second diff exits 0 → journal **`Transient`** — itself a finding:
    the serving state moved between two probes of one boot. The
    transient profile is retained beside the row.
  - Second diff exits 2 → journal `not-comparable` with the exit code;
    this is an infrastructure-shaped outcome, not a drift verdict.
- The confirm probe is the same instrument as POST's probe (same mode,
  same flags) — a confirmation under a different instrument would be a
  different measurement, not a confirmation.
- The confirm probe is bounded by the same
  `assay.probe_timeout_secs` discipline POST uses; a wedged confirm
  journals as infrastructure and the first reading stands as
  `unconfirmed` (named), never silently upgraded to `Confirmed`.

## 5. Profile retention and identity

Per model, the daemon retains: the current boot's profile, the
previous boot's profile, the blessed baseline, and any transient
profiles from confirm runs (bounded: the latest N=4 transients, oldest
dropped and journaled). Files are content-addressed in naming
(sha256 prefix in the filename) so a journal row's path claim is
verifiable against bytes. POST's existing delete-before-probe rule
stands: a stale file can never be read as this boot's measurement —
rotation to "previous" happens on successful parse, before deletion.

> **Footnote, 2026-08-17 (as-built).** Content-addressed naming ships
> for **transients only** — `{model}.transient-{sha8}.json`. The three
> named documents keep plain, role-derived names
> (`{model}.json`, `{model}.previous.json`, `{model}.baseline.json`),
> because their role, not their content, is what a reader looks them up
> by, and a content-addressed current profile would change name every
> boot. The stated purpose — *a journal row's path claim is verifiable
> against bytes* — is fully met by a different mechanism: every `Drift`
> row carries `reference_sha`/`current_sha` and every `Blessed` row
> carries `sha`, all full-64-hex sha256 **of the bytes the daemon
> actually read**, so `sha256sum <path from the row>` checks the claim
> directly. Verified live against real rows in
> `docs/superpowers/evidence/2026-08-17-drift-watch-live.md` (boot 1's
> `Blessed` sha equals `sha256sum` of the file it names).

## 6. The assay pin upgrade

Part of this slice, not a side effect: bloomery's assay pin moves from
`74c5b71` to assay 0.9.0 (schema v8). The upgrade is what makes
`diff --gate` and the v8 families available to the seam. Consequences
handled by design: the first post-upgrade boot's comparisons read
`instrument-changed` (§3) on every model with an old-schema reference,
and the operator re-blesses per model. Nothing converts old profiles;
old references are superseded by blessing, never rewritten.

## 7. Authority precedence (law)

`done_trust` remains the sole property of the G4 codec gate and the G5
refusal gate. The drift watch never reads or writes it. Drift status is
its own field in `ModelStatus`, rendered beside `done_trust`, and the
two say different things by design: G4/G5 answer "does this model do
bloomery's task honestly"; drift answers "has what assay can measure
about this serving path changed". Admission stays existence-gated
(law 5 unchanged). Any enforcement from capability vectors — refusing
admission on a confirmed regression, verdict floors, swap
admissibility, routing — is a LATER slice with its own spec.

## 8. Testing posture

House rules apply (TDD, mutation checks under the pyc-equivalent
discipline for Rust: verify each load-bearing test fails when its
pinned line is broken). Specifically:

- Gate outcomes driven by a scripted `assay` substitute (the POST
  tests' subprocess-injection seam) covering exits 0/1/2, the
  confirm's three outcomes, and the wedged-confirm timeout path.
- The instrument-changed rule pinned with mixed-version profile
  fixtures — including the real pre-upgrade schema as committed bytes,
  so the first-boot-after-upgrade path is tested against the artifact
  it will actually meet.
- Journal completeness: every comparison outcome class appears in the
  journal exactly once per boot per model; a replay can reconstruct
  the full drift history from journal + retained files alone.
- None-vs-zero: missing reference is `unmeasured` by name; an
  unconfirmed first reading is `unconfirmed` by name; no default that
  looks like a verdict.
- `ModelStatus.drift` renders in the status surface with the same
  None-honesty as `done_trust` (absent ≠ clean).

## 9. Non-goals

Admission changes of any kind; task routing; swap admissibility;
parsing `assay diff` prose; touching G4/G5 or `done_trust`; re-probing
on a schedule or on demand (drift is measured at boot; both a timer
and an on-demand trigger are later policy questions); converting or rewriting
old profiles; any assay-side change (assay ships what this needs
as of 0.9.0).

## 10. Deliverable order

1. Assay pin upgrade + the instrument-changed rule (testable without
   any new gate: mixed-version fixtures).
2. Retention/rotation + blessing (API/CLI + journal rows).
3. The two comparisons + gate subprocess + journal rows.
4. Confirm-then-alarm + `ModelStatus.drift`.
5. Live acceptance: two real boots on this box — one clean (within
   noise end to end), one with a deliberately induced instrument
   change (pin flip on a scratch config) reading `instrument-changed`,
   both journaled; evidence committed.
