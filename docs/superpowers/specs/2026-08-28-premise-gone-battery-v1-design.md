# `premise-gone-battery-v1` — the goal-satisfied-repeat lane instrument

The first of refalsify-battery-v2's three named absences, registered on
its own corpus treatment (that battery's findings §9: "the `premise_gone`
lane (no corpus task starts goal-satisfied) ... needs its own corpus
treatment and its own registration"). Registered under Brice's 2026-08-28
delegation ("do the premise_gone lane"), shape inherited from the
battery-v2 template; judgment calls this spec makes on its own are marked
**[judgment]** for after-the-fact review.

## 0. What the lane IS under exact retrieval (design finding, recorded)

Two-stage exact retrieval (memory-organ design §3: goal hash, then every
cited file byte-identical to the episode's pre-first-touch fingerprint)
makes one reading of "goal already satisfied" unreachable: if the cited
defect was FIXED, the cited bytes differ, stage 2 misses, and the task
runs as a stranger — no probe, no stamp, definitionally memory-off
behavior. That flavor needs no battery.

The reachable premise_gone lane is the other reading: **the world moved
on.** The cited file still carries its old (once-defective) bytes — so
retrieval matches — but the verification contract changed around it, and
the stored verification now passes on the matched state: the behavior the
episode calls a defect is what the current contract expects. The lesson
is not false; the world no longer needs it (refalsify v2 spec §1), and
its stored patch is now actively wrong. This corpus constructs exactly
that: each phase-2 workspace holds the target at its defective
(fingerprint-matching) bytes beside a **moved-on test** — same filename,
same module import — that passes on the defective target and fails on
the old fix.

## 1. The claim under test, and what may be said afterward

If the gates pass, the licensed sentence is: *"On exact repeats whose
stored verification already passes at task start — cited bytes unchanged,
the verification contract moved on (this corpus's moved-on-test
construction) — refalsify-on takes the premise_gone lane totally: every
matched retrieval stamps `premise_gone` and stays silent, and no episode
is contradicted or store-mutated, while refalsify-off injects the moot
lesson on every matched retrieval (lens: this battery)."* Nothing else.

In particular this battery licenses NO sentence about: the
staleness-benefit story (what the suppressed injection is *worth* — the
token/wall deltas here are advisory observations, never a claim); the
design-§5 passive-poisoning weight (M′'s aftermath is observed and
reported for the future §5 registration, never gated, never quoted as a
capability number); a probe-cost number (battery-v2 §7 stands: box noise
unresolved); the already-fixed-start flavor (§0 — unreachable, no
sentence either way); novel tasks, other models, other task shapes, or
accuracy. No number from this battery may be compared against
memory-battery-v1 or refalsify-battery-v2 (different corpus, different
night — incomparable, not wrong).

## 2. Instrument identity and lens

`premise-gone-battery-v1`. Everything battery-v2 pinned carries forward
unchanged unless named here: served-identity assertion per phase, driver/
watcher machinery, journal-bytes-only recompute discipline (plus one
named addition: the store files — each arm's `memory/episodes.jsonl` —
are a quotable evidence source for PG2/A2, read by the recompute, never
by hand), fresh agent per task at `window_cap 16384`, full workspace
reset + `__pycache__` purge before every phase, intent-to-treat,
none-vs-zero, task-level `dropped` lists, scratch-copy manifests (a live
daemon's write grant NEVER points into the tracked tree — battery-v2's
hard-won rule). The daemon build is master `e3cad71` (or a successor
recorded at lock — the refalsify default flip landed there; inert here
because both arms set `refalsify` explicitly). The lens names the exact
commit, config per arm, and model digest at lock.

## 3. Corpus — `corpus-pg-v1` (new, frozen at its generation commit)

- Generator `tools/memory_battery/corpus_pg.py`: same factory draw as
  `corpus.py` (run-verified python tasks, flat two-file workspaces),
  **corpus seed 20260828** [judgment: date-convention seed, distinct
  from corpus-v1's 20260826], n = 50. Per task it materializes:
  - `pristine/` — the factory workspace verbatim (defective target +
    original planted test). Phase-1 source, byte-identical role to
    corpus-v1's.
  - `pristine_p2/` — the phase-2 source: **target byte-identical to
    `pristine/`'s target** (the fingerprint match is the whole point) +
    the moved-on test at the same `test_file` name.
- **Moved-on test authoring (execute-and-pin):** every planted test in
  the factory's 8 run-verified families is a single
  `self.assertEqual(module.fn(args...), expected)` (surveyed across all
  50 corpus-v1 instances). The generator parses the planted test's AST,
  extracts the call expression, evaluates it against the DEFECTIVE
  module in a subprocess (same env discipline as
  `planted_test.run_python`), and rewrites the expected literal to the
  defective result's repr — nothing else in the file changes. A task
  whose defective call RAISES (no value to pin) is excluded and redrawn,
  bounded by the corpus.py overdraw pattern — a crash is not "the world
  moved on".
- Manifest: corpus-v1's schema plus per-task `"pristine_p2"` (relative
  path) and `"workspace_p2_sha256"`; `"instrument":
  "premise-gone-battery-v1"`.
- **Structural checker** `corpus_check_pg.py` (the black-oxide rule —
  every corpus claim executed, run at freeze AND re-run before the real
  boots), per task:
  - S1 fails-before: planted test on the pristine workspace exits
    nonzero (factory rule, delegated as corpus_check.py does).
  - S2 passes-after: `search` occurs exactly once in the target; with
    `replace` applied, the planted test exits 0.
  - S3 goal-satisfied start: `pristine_p2/`'s target is byte-identical
    to `pristine/`'s target, its test file differs from the original,
    and the moved-on test on `pristine_p2/` exits **0**.
  - S4 non-vacuity (the moved-on test still discriminates): the
    moved-on test against the FIXED target (S2's patched bytes) exits
    **nonzero** — the world genuinely moved on; the stored patch is now
    wrong, not merely redundant. Kills the "test weakened to accept
    anything" failure mode.
  - S5 shas: `workspace_sha256` and `workspace_p2_sha256` recomputed
    independently (corpus_check.py's deliberate-duplicate rule) and
    matched against the manifest; `workspace/` == `pristine/`.

## 4. Arms and phases

Two boots, pre-registered order **M′ then R**, same session:

- **Arm M′**: `[memory] enabled = true, refalsify = false`, fresh empty
  store, fresh scratch `data_dir`. Phase 1 = tasks 1–50 in manifest
  order on `pristine/`-sourced workspaces (defective starts — mints,
  identical semantics to battery-v2's phase 1). Phase 2 = same tasks,
  same order, workspaces materialized from **`pristine_p2/`**
  (goal-satisfied starts). The off-arm comparator: a matched retrieval
  injects the moot lesson.
- **Arm R**: `[memory] enabled = true, refalsify = true`. Identical
  phases, its own fresh empty store. Phase 2's matched retrievals are
  probed; the probe re-runs the stored verification in the moved-on
  workspace, exits 0, and the premise_gone lane — silent, uninjected,
  store untouched — is precisely what the gates check rather than
  assume.

Probes cannot fire in either arm's phase 1 (empty stores), so the two
phase-1s remain the cross-arm instrument check (H2). Goal text is
byte-identical across phases per task (stage-1 hash); each phase-2 task
therefore receives a goal describing a defect the workspace no longer
exhibits — the honest shape of a moved-on world, and the symptom-mismatch
shape the flywheel line was trained to handle honestly.

**Matched set, per arm** (the gates' denominator): in R-p2, stamps with
`refalsify != None` (the probe fired or was skipped — a hit existed;
`premise_gone` stamps carry `episode_id: None` by design, so the
spelling field, not the id, marks the match). In M′-p2, stamps with
`mode: "injected"` (flag-off hits always inject), plus a required scan
for oversize `Degraded` rows (expected zero) since an oversize skip
stamps silent/None and would otherwise hide a match.

## 5. Endpoints, bars, kill criteria

`cost(task)` and wall exactly as battery-v2 §4 (journal
`InferCompleted` sums, ITT, `None`+`dropped` never zero; wall from
journal task rows).

**Gate PG1 — premise_gone totality (R):** over R-p2's non-`dropped`
tasks: `injected_R,p2 = 0` (exact), AND every matched stamp's spelling
is `premise_gone` with `mode: "silent"`. The spellings that would break
this are each diagnosed, not blended: `premise_held` means a phase-2
workspace failed to materialize goal-satisfied (instrument alarm —
investigate before reading any gate; the battery-v2 rule mirrored);
`skipped_ungranted` means the corpus's own grant failed to cover its
own `run_argv` (instrument alarm, run **INVALID**); `inconclusive`
(probe timeout/spawn) is probe infrastructure — that task is `dropped`
and counted toward H3, never scored.

**Gate PG2 — store preservation (R):** zero `MemoryContradicted` events
in arm R's entire journal, AND every episode row in R's final
`memory/episodes.jsonl` has `status: "verified"`. (Entailed by v2's "no
probe ever contradicts" plus "nothing injected → §5 cannot fire" — the
gate checks the entailment live rather than assuming it, exactly the
stamp-audit discipline.)

**Gate PG3 — moot-lesson injection (M′):** every matched M′-p2
retrieval injects: `injected_M′,p2 ≥ 25` (the floor below) with zero
oversize `Degraded` rows, and every M′ stamp's `refalsify` is `None`
(flag-off truth).

**Matched-count floor (both arms):** `matched_R,p2 ≥ 25` AND
`injected_M′,p2 ≥ 25`, else the verdict is **UNMEASURABLE**, not
FAIL — below half the corpus, the construction's premise (phase-1
episodes cite only the target file, so phase-2's moved-on test file
stays out of the match key) failed at scale and the corpus needs
redesign, not a verdict. **[judgment: 25 = n/2 is chosen, not derived
— flagged per the house rule on chosen thresholds.]** The per-arm
matched counts and the phase-1 cited-set behavior behind any misses are
reported either way.

**Stamp audit (gating, instrument honesty):** `passed`/`failed` appear
nowhere in either arm; `premise_held` count 0 (alarm semantics above);
every M′ stamp `refalsify: None`; every R-p1 stamp `refalsify: None`
(no probe can fire on an empty store).

**Hygiene (computed before any gate is read, in this order):**

- H2 — first-exposure equivalence: `|median_M′,p1 − median_R,p1|`
  within `2 × SE_boot` (tokens; seeded bootstrap **seed 20260829**, B =
  10,000, resampling unit = tasks, arms independent) [judgment:
  fresh seed, distinct from both corpus seeds and battery-v2's
  bootstrap seed]. Violation → run **INVALID**.
- H3 — infra rate ≤ 5% per arm (`driver-infra`, task `Error`,
  `inconclusive`-dropped tasks). Above → infrastructure kill; rerun
  from zero only if no gate number was read.
- H4 (advisory): p1 mint rate and p2 matched rate per arm; the
  cross-arm matched-count gap `|matched_R − injected_M′|` (arms mint
  independently — a large gap is phase-1 behavioral divergence, named).

**Advisory (never gates, no capability sentence):**

- A1 — p2 token medians per arm, delta, and `2 × SE_boot` band
  (bootstrap as H2). Expected direction R ≤ M′ (R carries no injected
  block); it belongs to the staleness-benefit story's future
  registration and is reported as an observation only.
- A2 — M′ aftermath, for the future §5 registration:
  `MemoryContradicted` count in M′'s journal (§5 poisonings of true
  episodes); per-arm p2 counts of tasks with ≥1 successful patch
  (moot-lesson-driven re-patching); terminal-status distributions per
  arm×phase; M′'s final episode statuses from its `episodes.jsonl`.
- A3 — wall: battery-v2 §4's A1 verbatim (p2 delta beside the no-probe
  p1 control; a p1 gap of the same order means box noise and the honest
  report says so).

**Kill criteria / discipline (carried verbatim):** the point estimate
decides; no re-run, no extension, no corpus change after any number is
seen; an infrastructure kill with no numbers read may rerun from zero;
equivalence-band verdicts obey the floor-saturation rule
(UNMEASURABLE, not PASS, when resolution is floor-granted).

## 6. Machinery deltas (all before the prereg locks, all mutation-tested)

- `corpus_pg.py` + `corpus_check_pg.py` (§3) — generator determinism
  pinned (same (seed, n) → byte-identical fields modulo out_dir);
  moved-on authoring mutation-tested (wrong literal, unpinned call,
  raising defective call each must fail a test).
- `driver.py`: per-phase workspace source — phase 2 resets from the
  manifest's per-task `pristine_p2` when present, else `pristine/`.
  **Compat pin: a manifest without the key behaves byte-identically to
  today** (corpus-v1 unaffected), with a test proving both branches and
  a mutation check (swap the phase-2 source → caught).
- `dry_manifest.py`: scratch-copies `pristine_p2/` beside
  `workspace/`+`pristine/` when the task has one, and carries the
  manifest key through with rewritten paths; dry/real modes unchanged
  otherwise.
- `recompute_pg.py`: arms `m_prime`/`r`; reads journals + ledgers +
  both arms' `episodes.jsonl`; emits PG1/PG2/PG3, floor, stamp audit,
  H2/H3/H4, A1/A2/A3, completeness, identity (digest FATAL on
  mismatch, battery-v2's CLI-enforcement rule), `dropped`, corpus sha.
  Mutation checks: swapped arms, broken spelling count, broken
  store-status read, broken matched-set definition, seed drift — each
  must fail a test.

## 7. Sequence and the locks

1. This spec committed → plan → machinery + tests (no corpus, no GPU).
2. Corpus generated, checker green, frozen at its commit (bytes
   thereafter; the amendment rule attaches).
3. Dry shakedown: 3 tasks through both arm configs on the live daemon —
   numbers discarded, marked DRY, never quoted.
4. Prereg doc committed (the lock): lens pins, configs verbatim, every
   locked number, machinery shas, operational checklist, amendment rule.
5. Launch under Brice's standing delegation (recorded in the ledger;
   GPU hygiene checked immediately before boot — free VRAM verified, no
   competing daemon; if the GPU is held, STOP and report instead).
6. Run M′ then R (detached, watcher, teardown verified); journals
   collected.
7. Recompute → findings quoting recompute output only → CARRIED-DEBT,
   memory, merge + push per standing rulings.

## 8. Out of scope

- The staleness-benefit and design-§5-weight registrations (their
  observables here are advisory feed-forward, nothing more).
- Any probe-cost resolution; any refalsify/organ/retrieval semantics
  change; any mint-bar change; the default-flip (already ruled and
  shipped, `e3cad71`).
- The already-fixed-start flavor (§0): unreachable under exact
  retrieval; its behavior (stranger-silence) is unit-pinned in the
  organ's own suite and licenses no sentence here.
- Cross-battery number comparisons of any kind.
