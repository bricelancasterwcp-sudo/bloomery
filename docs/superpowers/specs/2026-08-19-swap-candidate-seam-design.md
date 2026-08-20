# Swap-candidate admissibility — the capability-vector seam, slice 3

**Date:** 2026-08-19
**Status:** Approved in conversation (scope: swap admissibility first;
evidence: daemon-probed only; coverage: every measured floor cell;
consequence: advisory; handover: manual re-bless; G4/G5: out, named).
**Lineage:** the drift watch (slice 1, PR #12) and verdict-gated
admission (slice 2, PR #13), whose exit-code discipline and journal
idiom this slice extends; assay v1.8–v1.10 (exit 3, the parallel
verdict, the scale-free overlap rule, the semantic-break registry),
whose vocabulary the new comparison must honour; PR #14, which taught
the gate assay's full four-code vocabulary; the standing v10 baseline
(evidence `2026-08-19-standing-v10-baseline.md`), which is the floor
this slice compares against.

## 1. What this builds and why

Slices 1 and 2 answer "is the model I am serving still the model I
blessed?" Nothing answers the question that precedes every swap on this
box: **is candidate Y admissible as a substitute for model X?** Today
that decision is made on gut feel and a G4 run after the fact; the
operator swaps a GGUF in config and finds out what they lost at the
next boot.

The obvious mechanism — `assay diff` X-vs-Y — is refused by assay
itself, correctly: `model.name`, quant, and `weights_bytes` are
identity-fatal, because a drift comparison between two different models
is a statement about neither. That refusal shapes this slice. A swap
does not ask "did Y drift from X"; it asks **"does Y cover what X was
relied on for?"** — a one-directional coverage check of Y's measured
profile against the requirement vector X's blessed baseline induces.
This is the capability-vector idea landing concretely: the vector is
not a new artifact to invent, it is the baseline assay already writes,
read as a floor.

## 2. Two waves, one contract

The comparison logic belongs in assay, not here. Coverage needs the
verdict ladder, the provisional/Wilson discipline, the speed noise
bands, and the v1.10 semantic-break registry — bloomery has never
parsed a profile for verdicts, and duplicating any of that in Rust
recreates the divergence this seam exists to kill.

1. **assay v1.11 — the coverage mode** (`assay cover`, working name),
   specced and shipped in the assay repo, deriving from §3 here.
2. **bloomery — the swap-candidate endpoint** (§4), consuming exit
   codes through the same subprocess discipline as the drift gate.

The contract between them is the exit-code vocabulary and two file
paths. Nothing else crosses the seam — no prose parsing, no
transcribed numbers. Sequencing is strict: the assay wave merges
first; the bloomery wave's live acceptance needs the real subcommand.

## 3. The assay side: what `cover <floor.json> <candidate.json>` means

This section is the contract the assay-repo spec derives from; that
spec owns the final command name, flags, and internal design.

- **Identity gate, inverted on purpose.** `model.name`, quant, and
  `weights_bytes` MAY differ — that is the point of the command. Still
  fatal (exit 2): `tier`/`emulated` disagreement (a floor measured on
  a different hardware class is not a floor for this box), and
  **instrument disagreement** — `probe_version`/schema mismatch, or a
  pair straddling a registered semantic break. A floor and a candidate
  measured under two different rules is the v1.10 lesson, applied here
  from day one.

  > **Amendment (2026-08-19, assay v1.11 spec ruling):** instrument
  > equality is STRICT — `probe_version` and schema exactly equal,
  > absence fatal — which subsumes the straddle clause (equal versions
  > cannot straddle a registered break). Strictness is the honest
  > choice: v1.10's own record states the semantic-break registry is
  > not a complete inventory, so a version-tolerant cover would trust
  > an incomplete table. The registry check survives in `cover` as
  > defense-in-depth should the gate ever loosen.

  > **Amendment (2026-08-19, assay v1.11 review rulings).** Two rulings
  > postdate the note above, both recorded as dated amendments in assay's
  > own spec
  > (`assay:docs/superpowers/specs/2026-08-19-assay-v1.11-cover-design.md`,
  > §1 and §3). The second of them corrects the note above's last
  > sentence.
  >
  > 1. **`tier`/`emulated` absent on BOTH sides is also fatal** (assay §1's
  >    task-1 review amendment) — a deliberate deviation from `diff`'s
  >    gate, which passes that pair. The bullet above says
  >    "disagreement"; the shipped rule refuses disagreement, one-sided
  >    absence, *and* two-sided absence, so only a declared, matching pair
  >    passes. A coverage claim "for this box" with no box declared on
  >    either side is exactly the silent pass the gate exists to refuse,
  >    and cover's own instrument loop already holds that undeclared is
  >    unknown. `diff` is unchanged.
  > 2. **The registry check is a LIVE refusal route, not defense-in-depth**
  >    (assay §3's amendment, which supersedes the note above's closing
  >    sentence — an earlier wording called the branch unreachable and a
  >    review probe disproved it). Strict equality does not close one case:
  >    a `probe_version` equal on both sides but *unparseable* (not three
  >    decimal components) passes the equality check, and assay's
  >    `_straddles` fail-safes an unparseable version to straddling for
  >    registered cells. `cover` refuses that pair — exit 2, naming the
  >    cells, the fail-safe direction — on a path assay now pins by test.
  >    Nothing changes on this side of the seam (it is still exit 2, still
  >    read as a refusal and never a pass); what changes is that the seam
  >    spec must not leave a reader believing the registry matters only
  >    hypothetically.
- **Per-cell rule.** For every cell the floor **measured**, the
  candidate's verdict must rank greater-or-equal on assay's own
  verdict ladder; numeric floors (speed, geometry-derived windows)
  compare under assay's existing noise discipline, not raw
  subtraction. A floor cell the candidate did not measure is
  **incomplete — never a pass** (the exit-3 discipline: the unmeasured
  cell may hide exactly the regression the check exists to catch).
- **One-directional.** Candidate cells the floor lacks are ignored.
  Coverage asks whether Y provides what X provided; what Y adds is not
  evidence either way.
- **Exit codes, mirroring `diff --gate`:** `0` covered, `1` not
  covered, `2` refused (identity/instrument), `3` incomplete, with
  precedence `2 > 3 > 1 > 0`. Bloomery's post-PR-#14 four-code reading
  maps over without a new vocabulary.

## 4. The bloomery side: `POST /models/{name}/swap-candidate`

Body: `{"gguf_path": "/abs/path/to/candidate.gguf"}`. `{name}` is the
configured model whose role the candidate would take.

**Preconditions.** The name must be configured (404, the surface's one
`unknown_model` shape). A **blessed baseline** must exist for it (409
`no_baseline`): the floor is the operator-endorsed capability
statement, never the merely-latest profile.

**Flow.**

1. Register the candidate under a scratch identity (so `/v1` can
   address it) and admit its weights through the pager's normal
   reservation arithmetic. Unplaceable → 409 with the bytes needed,
   free, and reclaimable — the existing refusal shape. The scratch
   identity never outlives the request.

   > **Amendment (2026-08-19, ruling bT3/R1 — the 409 disposition).**
   > "Unplaceable → 409 with the bytes needed, free, and reclaimable" is
   > **not honestly implementable at POST time, and is not implemented.**
   > The pager exposes no pre-probe reservation check.
   > `PagerError::Refused` — the only source of those three numbers — is
   > produced in exactly one place, the private `Pager::place`
   > (`crates/bloomery-daemon/src/pager/paging.rs`), which is keyed on an
   > agent **already in the table** and whose demand term is that agent's
   > own window-sized reservation plus, for a cold model, its weights.
   > Neither call this step actually makes commits anything:
   > `register_model` inserts a registry entry and runs no residency
   > arithmetic, and `create_agent` commits no VRAM either (an agent
   > starts `Fresh` and becomes resident only at its first inference).
   > At POST time there is no agent and no window, so a 409 here would
   > have to invent the demand term and print `needed`/`free`/
   > `reclaimable` figures no real refusal ever produced — fabricated
   > arithmetic wearing the existing refusal shape, which is worse than
   > no refusal at all.
   >
   > **Ruled disposition:** the reservation refusal surfaces through the
   > probe's own failure, where it is real. The candidate is probed
   > through this daemon's own `/v1`, which renders `PagerError::Refused`
   > as `503 residency_refused` carrying the arithmetic in its message;
   > the probe then fails, the worker journals `Degraded`, and the report
   > and its `GET` carry `infra: the candidate probe for {model} …
   > failed: …` with the probe's own words. Unmeasured, explicitly not a
   > verdict — §7's rule, unchanged. A POST-time 409 would need a real
   > pager dry-run (a public "would this place?" that charges nothing),
   > which is new pager surface this advisory slice deliberately does not
   > build; named here as what a later enforcement slice would want, not
   > as a gap in this one. Step 5's "unload the candidate, crediting its
   > weights back" is unaffected and ships as written.
2. Probe the candidate through the daemon's own `/v1` with the
   identical POST invocation (same mode, same flags, same
   `probe_timeout_secs` cap). The daemon's environment supplies the
   instrument — the gate's interpreter is the probe's interpreter.
   Probe mode matching matters and self-corrects: a floor blessed
   from a deeper-mode profile than the candidate's probe leaves floor
   cells unmeasured on the candidate, and §3's incomplete rule
   refuses the pass rather than shrinking the floor.
3. Retain the profile content-named beside the drift transients,
   subject to the same retention bound.
4. Spawn `assay cover <baseline> <candidate-profile>` and read the
   exit code exactly as the drift gate reads `diff --gate`.
5. Unload the candidate, crediting its weights back.

**Journal row** (`SwapCandidate`): model name, candidate GGUF
full-file digest, both profile paths and shas, the exit code, and the
outcome word. Identity and prose, never a transcribed measurement —
anyone can re-run the identical `cover` from the row alone.

> **Amendment (2026-08-19, ruling bT1/R2 — as-built).** The outcome word
> for a *refusal* is not bare. `CoverOutcome::Refused` carries
> `{ exit, stderr }` (`crates/bloomery-daemon/src/swap.rs`), and both the
> row and the report's `outcome` spell it `refused: <assay's trimmed
> stderr>` when there are words to carry and `refused` when there are
> not — one spelling, so the row and the operator's answer can never
> disagree. The stderr is **operator detail, never consulted for the
> verdict**: the verdict is the exit code and nothing else. It rides
> along because exit 2 is also what `argparse` answers for `invalid
> choice: 'cover'` — an assay too old to have the subcommand (anything
> before 0.13.0, under the daemon's `PYTHONPATH` pin) refuses in a way
> that is indistinguishable *by code alone* from a considered refusal
> about the candidate, and discarding the one sentence that says "this
> tool has no cover" would let a stale install masquerade as a verdict.
> This is the discipline §7 already states for `Infra` ("carrying the
> code and stderr") extended to the one *reading* that shares an exit
> code with a missing tool, and it does not touch this paragraph's
> no-transcribed-measurements law, which is about numbers copied out of
> profiles. `Incomplete` (exit 3) deliberately carries nothing extra: it
> has one meaning and no ambiguity to resolve, and *which* floor cells
> went unmeasured is a measurement that lives in assay's own render of
> the pair, re-derivable from the row's two paths.

**Response.** The verdict and evidence paths, plus two named gaps:
done_trust/G4/G5 are unmeasured for the candidate until its first real
boot with tasks enabled; and the handover (§5) is the operator's next
step, spelled out.

> **Amendment (2026-08-19, as-built).** This paragraph describes the
> *verdict's content*, and none of it comes back from the POST. A
> candidate probe holds VRAM for ~10 minutes (this section's own "One
> candidate at a time"), so a handler that waited for it would hold one
> of four HTTP workers for the whole run — the boot watch's own rule,
> that a probe never rides a request handler, applies here unchanged.
> The shipped shape is therefore asynchronous, and it is two routes:
>
> - `POST /models/{name}/swap-candidate` answers **202**
>   `{model, candidate, state: "running"}` once it has run the cheap
>   preconditions, claimed the one slot and spawned the worker. The
>   refusals stay exactly as this section names them (404 unknown model,
>   400 `bad_request` for an unparseable body or unreadable candidate
>   weights, 409 `no_baseline`, 409 `candidate_probe_in_progress`).
>   Everything above the slot claim is a read — the model's existence,
>   the body, the candidate's bytes, the floor's existence — and the
>   claim comes last, so a request that was going to be refused anyway
>   never takes the slot from a job that would have run, and before the
>   spawn, so two workers can never both be registering the same scratch
>   identity.
> - `GET /models/{name}/swap-candidate` is where the answer appears:
>   **200** `{model, state: "running"}` while the job runs, **200**
>   `{model, state: "done", report: {…}}` once it has. The report is the
>   whole of this paragraph's requirement — `outcome` (the verdict word),
>   `exit_code`, `candidate_gguf_sha`, `floor_sha`,
>   `candidate_profile_path`, and both fixed `notes`, carried whatever
>   the verdict, because both gaps are true of every candidate. A `GET`
>   for a model whose job never ran — or while a *different* model's job
>   holds the one slot — is **404** `no_swap_candidate`: that job says
>   nothing about this name.
>
> A daemon served without a swap-candidate context (no interpreter, no
> profile store, no self-port) answers **501**
> `swap_candidate_unavailable` on both routes rather than pretending to
> a refusal about the candidate.

**One candidate at a time.** A probe holds VRAM for ~10 minutes. A
second request while one runs gets 409 `candidate_probe_in_progress` —
no queue.

**Advisory.** Nothing blocks, nothing auto-swaps. Config remains
operator domain; the verdict is evidence, journaled.

## 5. The handover, named

After an admissible verdict the operator edits config and restarts.
The next boot's drift watch reads **not-comparable** against the old
lineage's baseline — honest, the lineage did change — and the operator
re-blesses deliberately via the existing `POST /models/{m}/bless` once
satisfied. No new machinery: the swap verdict's response spells this
sequence so the not-comparable boot is expected, not alarming.

## 6. Non-goals, each with its reason

- **Enforcement** (refusing to serve a swapped GGUF with no admissible
  verdict on record) — deferred to a later slice, the slice-1-then-2
  pattern: measure first, enforce once the measurement has lived.
- **G4/G5 in the candidate probe** — the codec gate stays boot-time
  authority; coupling it to this endpoint lengthens every probe and
  duplicates a path that already runs at first boot.
- **Capability-addressed agent creation** ("agents ask by requirement
  vector, not name") — the end state this slice's vocabulary feeds,
  its own future slice.
- **Operator-supplied candidate profiles** — rejected for this slice:
  probe-only evidence keeps provenance airtight (same instrument, same
  box, journaled). Staleness policy for external evidence is a design
  problem this slice refuses to inherit.
- **bless-on-admissible / auto-bless** — rejected: blessing is a
  deliberate operator act in the drift watch's design, and a candidate
  that has never served a boot under its own name has not earned a
  baseline.

## 7. Error handling

Every failure is named and none is a verdict: a probe spawn failure or
timeout journals as degraded and returns an error naming the probe's
own words — no verdict invented; `cover`'s four codes are readings and
anything else is `Infra` carrying the code and stderr (the PR #14
discipline); pager refusal, unknown model, missing baseline, and busy
each keep the surface's existing 404/409 idiom.

> **Amendment (2026-08-19, as-built) — the "pager refusal" clause.** Three
> of that last list ship exactly as written: unknown model 404, missing
> baseline 409 `no_baseline`, busy 409 `candidate_probe_in_progress`. The
> fourth, **pager refusal, has no POST-time shape at all** — see §4 step
> 1's amendment for why the numbers a 409 would have to print do not exist
> before an agent and a window do. A real residency refusal reaches the
> operator through the first clause of this very sentence instead: the
> probe fails, it journals `Degraded`, and the report names the probe's own
> words as `infra: …`. Also as-built, and not in the list above: a body
> this route cannot use (unparseable JSON, no `gguf_path`, or weights that
> cannot be read) is a 400 `bad_request`, answered synchronously before the
> slot is claimed; and a daemon served with no swap-candidate context
> answers 501 `swap_candidate_unavailable` on both routes.

## 8. Testing posture

House rules (TDD; mutation-check load-bearing tests) on both sides.

- **assay v1.11:** fixtures driving each exit code; the
  one-directional pin (a candidate-only cell changes nothing); the
  incomplete pin (a floor cell unmeasured on the candidate is `3`,
  never `0`, even with every measured cell covered); refusal pins for
  tier mismatch and instrument mismatch including a registered-break
  straddle; ladder-order pins against the existing verdict ordering.
- **bloomery:** `drift_test`-style scripted probe and gate fixtures;
  journal-row pins (digest, shas, exit, outcome word); endpoint status
  table (404/409s); an explicit pin that the admission policy table is
  untouched — this slice is advisory.
- **Live acceptance:** probe **flywheel1's GGUF** as a candidate
  against the standing **flywheel2 baseline** on this box. A real
  question with a meaningful answer either way; evidence committed
  beside the prior acceptance docs.
