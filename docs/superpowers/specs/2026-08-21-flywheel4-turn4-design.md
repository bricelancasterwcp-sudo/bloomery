# Flywheel turn 4 — envelope-v4 (the visible grant), verified runs, and the reason-grounding endpoint

**Date:** 2026-08-21
**Status:** Approved in conversation (rulings: scope = full turn; run check =
planted `unittest` + grant `python3 -m unittest`; the grant line is rendered
from the real `Grant`, making it envelope-v4; gate = v4-mixed 16+16 with
fresh-framed refuse goals; two new secondary endpoints — productive run and
reason-grounding — reported, never gated).
**Lineage:** flywheel turn 3 (`2026-08-20-flywheel3-turn3-design.md`, PASS —
`done_trust` at n=16; find-shaped navigation 5/6) and its battery's named
open questions; the 2026-08-21 run-transfer spike (memory + this doc §1).

## 1. What this builds and why

Turn 3 trained three things and two took. The `run` slice — 333 trajectories
teaching verify-before-done — produced **zero** `run` verbs at probe time.
The spike found the cause, and it is not weak training: the task prompt is
`goal + verb card + transcript` and **never renders the grant**, so a
run-granted task and a plain task are token-indistinguishable at the
decision point (after a successful patch observation), where the corpus
voted `done` 666 : `run` 333. Supervised fine-tuning on conflicting labels
for indistinguishable inputs collapses to the majority; flywheel3 did exactly
that. The find slice took because its cue (a goal naming no file) is visible
at step 1.

Turn 4 therefore:

- **Makes the grant visible — envelope-v4.** The prompt gains a grant line
  rendered from the real `Grant` the loop enforces. A prompt change is a new
  envelope under the lens-travels-with-verdict rule, so every turn-4
  measurement is per-(model, v4), with fresh baselines under v4.
- **Rebuilds the run slice around a check that can fail.** `py_compile`
  cannot fail on semantic defects (turn 3 trained a habit, not
  verification). Run-verified tasks and gate fixtures now plant a
  `unittest` that fails before the patch and passes after.
- **Adds the reason-grounding endpoint** — a mechanical proxy for the
  confabulation turn 3's battery review found inside a *correct* refusal
  (a `done` that cited an `overflowsafe()` function absent from the file).
- **Freezes `codec-tasks-v4-mixed`** with fresh-framed refuse goals, closing
  the frame-literal sharing the v3 audit named.

## 2. envelope-v4 — the visible grant

`render_prompt` under v4 is `goal + grant line + verb card + transcript`.
The grant line is rendered **from the `Grant` the task loop will enforce**,
never from task text:

- granted: `Granted commands: python3 -m unittest` — one line per argv
  prefix, space-joined;
- none: `Granted commands: none — run is not available in this task`.

Same source of truth as enforcement, so the model can never be told
something the loop refuses. v1/v2/v3 render byte-identically to today (the
line is v4-only; `think_preseed` as v3). Footprint, named now:
`EnvelopeLens::V4`, lens name `bloomery-task-envelope-v4`, parse `"v4"` in
config and the tool, `ENVELOPE_LENS_V4` in the probe, the render branch, and
the grant-line renderer beside `verb_card_for`. The tool's rendered prompt
must match the loop's under v4 (anti-drift pins, as for every envelope).

Rejected alternative: a grant sentence in the fixture/task *goal* text — no
envelope bump, but the model would learn its permissions from the task author
rather than the system, and two authoring surfaces would have to agree.

## 3. The corpus (regenerated under envelope-v4)

Everything regenerates under v4 so every pair carries the cue. Corpus seed
**20260821**; same scale and slices as turn 3 (999 patch = 333 find / 333 run
/ 333 plain; 450 refuse = 150 per family) — the slices are known-good and only
the cue should move. Lens mix stays ~735:264 (run is lens-py only), stated in
the prereg.

**The run slice, rebuilt:** each run-verified task plants `test_<target>.py`
beside the target (a small `unittest` asserting the goal's expected behavior)
and carries the grant `[["python3","-m","unittest"]]`. Ideal: `read(target)`
→ `patch` → `run ["python3","-m","unittest","test_<target>.py"]` → `done`.
**Fails-before / passes-after is a validator rule**: the factory executes the
test against the unpatched file (must exit nonzero); the tool's real run
executes it against the patched file (must exit 0, the turn-3 rule). Either
failing = structural rejection, never rendered. Plain and find tasks render
the `none` grant line, so the post-patch decision point now has
distinguishable inputs; the 2:1 conflict is dissolved by the cue, not by
rebalancing.

The find slice, the plain slice and the three refusal families are unchanged
in design; their ground-truth reasons already backtick-quote real
identifiers, which is what makes §4's reason-grounding endpoint measurable.

**Ride-alongs** from the turn-3 ledger: the contamination guard screens
sibling **filenames** against gate targets (the last gap in that rule); the
scratch `.lock` residue is swept at teardown. Nothing else from the deferred
list enters the corpus path.

## 4. Gate: `codec-tasks-v4-mixed`, the endpoints, and the G5 amendment

- **The set**: 16 + 16, composition as v3 for comparability — patch 6
  find-shaped / 5 run-granted / 5 plain; refuse 6 defect-absent / 5
  missing-target / 5 symptom-mismatch; both lenses in both classes; a fresh
  held-out gate seed; frozen on first commit. v1, v2-mixed, v3-mixed stay
  byte-frozen.
- **Authoring rules** (v3's diversity and quoting rules, plus): **no refuse
  goal may reuse a `goal_phrasing` skeleton frame verbatim** (asserted at
  freeze against the skeleton templates' fixed prose); **run-granted fixtures
  carry a planted `unittest` + the `python3 -m unittest` grant**, and the
  structural test executes each one against the shipped file (fails) and the
  reference-patched file (passes). `FIXTURE_MAX_STEPS = 6` leaves a 4-step
  ideal two spare turns — pre-registered as in v3.
- **G5 floors unchanged**: ≥13/16 per class; decided/provisional by the
  two-sided Wilson rule (bT10/R1), always stated separately from the floor.
  `gates.md` takes a dated amendment naming `v4-mixed` under
  `bloomery-task-envelope-v4` as turn 4's decided-G5 instrument; v3-mixed
  remains turn 3's. No scoring change; no journal schema change.
- **Secondary endpoints** (pre-registered; reported, never gated): the
  turn-3 four (productive find /6, run-before-done /5, find-usage /6,
  per-family refuse 6/5/5) plus two new:
  - **productive run** — run-granted fixtures (of 5) whose trajectory holds a
    well-formed `run` that exited 0 AND landed. The number the turn exists to
    move.
  - **reason-grounding** — over landed refuse fixtures, the fraction of
    backtick-quoted spans in the `done` text that are substrings of the
    fixture's files; rows with zero quoted spans are reported as unmeasured,
    never as 100%. Computed post-hoc from the committed `done` rows and the
    frozen TOML.
- **Instrument deltas, named**: `ENVELOPE_LENS_V4`; `shipped_fixture_set_v4_mixed()`
  + boot swap with the placeholder-era pattern (skip wording is already
  era-independent); planted tests are ordinary `[[fixture.file]]` entries and
  `commands` already exists — no parser change.

## 5. Pre-registration (committed BEFORE training)

- **Baselines first, under v4** — one boot each (G4-on-v1 + G5-on-v4-mixed):
  **flywheel3** (the incumbent; also answers a pre-registered question — does
  the visible grant alone make a model trained without the cue run? either
  answer is valid and informative) and **stock-14B** (the floor). flywheel2
  skipped. **No cross-envelope comparison is ever written**: fw3@v3 stays in
  turn 3's record; fw3@v4 is turn 4's anchor.
- **The flywheel4 battery, all under envelope-v4:** (1) G4 on codec-tasks-v1,
  pass ≥16/20 (anchor = fw3@v4's own G4 leg); (2) G5 on codec-tasks-v4-mixed,
  pass ≥13/16 per class, flags per the two-sided rule.
- **Success = both pass. Kill:** G4 < 16/20 OR refuse-class < 8/16 → adapter
  shelved, recorded with anatomy; secondary endpoints never kill material.
- **Training:** same pinned recipe and training seeds (20260816, procedure
  identity); corpus seed 20260821 → `qwen3-14b-flywheel4`; artifacts in
  `~/flywheel4/`, sha-anchored. Turn-1/2/3 artifacts untouched.
- **Honest possibilities:** the grant line over-triggers `run` on ungranted
  fixtures (surfaces as grant-violation rows — measured); the planted test
  leaks the expected value and eases run-granted patch fixtures (scoring is
  landing, the endpoint is verification — a caveat, not a confound); a new
  trajectory shape competes with find (patch floor + productive find catch
  it); reason-grounding reveals confabulation on the patch side too (looked
  for and reported); a model trained without the cue ignores it (fw3@v4 tells
  us before training).

## 6. Testing posture

TDD throughout; mutation pins on the grant-line renderer (a dropped or wrong
line fails the tool-vs-loop anti-drift pin under v4) and on the fails-before
validator rule; v1–v3 rendering pinned byte-identical; structural suite for
v4 mirroring v3's (composition, prefixes, executed tests, diversity,
fresh-frame, names unique across four sets, disjointness across three older
gates); both suites green at every task; every GPU step human-gated,
featured-build-last; evidence reviews with independent recomputation.

## 7. Non-goals

No enforcement wiring — `done_trust` stays advisory; wiring it into admission
is its own future spec. No verb-use floors; no scoring or journal-schema
change; no amendment to any frozen set; no multi-defect tasks; no
cross-envelope comparisons; reason-grounding is reported, never a floor.

## 8. Deliverable order

1. `gates.md` dated amendment + `g5v4-protocol.md` — before any code.
2. envelope-v4 (config + tool + probe + renderer + pins).
3. v4 set plumbing (shipped fn + boot swap + placeholder era).
4. Factory: run slice rebuilt; requests under v4; ride-alongs.
5. Author + freeze `codec-tasks-v4-mixed`.
6. Baselines live — fw3 and stock under v4 (human-gated).
7. Corpus + prereg (seed 20260821; anchors baked in).
8. Training → `qwen3-14b-flywheel4` (human-gated).
9. Battery + evidence + CARRIED-DEBT append (human-gated).
