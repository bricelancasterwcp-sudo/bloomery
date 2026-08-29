# `s5-weight-battery-v1` — the passive-contradiction weight instrument

The second of the named absences (refalsify-battery-v2 findings §9;
premise-gone-battery findings §8): the design-§5 passive-poisoning
**weight**. Per the refalsify-v2 spec's second named limitation, this
registration starts from the memory-organ design's §5 — the rule itself —
not from the probe. Registered under Brice's 2026-08-28 delegation ("do
the §5-weight registration"), shape inherited from the battery template;
judgment calls marked **[judgment]**.

The motivating observation is on record and licenses nothing:
premise-gone-battery A2 saw §5 contradict 47/50 true episodes in a
refalsify-off arm on goal-satisfied repeats. That fired the question;
this battery is the registered answer.

## 0. What §5 mechanically is (code-cited entailments, NOT findings)

`organ_after_run` (`task/registry.rs`): an injected task contradicts its
episode ⟺ `is_scored_outcome(status)` ∧ `verifying_run(result).is_none()`.
Scored = `Done`/`StepsExhausted`/`BudgetExhausted`/`WindowExhausted`;
`Error` is the infra bucket — unmeasured, the episode STANDS.
`verifying_run` (mint.rs) requires `Done` AND a successful patch AND the
last completed post-patch run exiting 0; `build_episode` additionally
refuses on any `PreTouch::Uncomputable`.

Entailed, therefore never reportable as findings:

- Every **scored** injected task ends in exactly one of
  {`MemoryMint` (refresh), `MemoryContradicted`} — except the named
  `Uncomputable` refusal (no mint, and if it verified, no contradiction
  either: verify ⟹ no-§5 but mint refused) and mint/contradict I/O
  degrades. The recompute checks this LIVE as a validity gate
  (conformance, the stamp-audit discipline), never quotes it as a result.
- A scored **non-Done** injected task always contradicts (verifying_run
  is Done-gated), whatever ran.
- What is NOT entailed — the measurement — is the **split**: which arm of
  the entailment each lane's tasks actually take is model×rule behavior
  on construction-certified ground truth.

Also code-cited: the injected block is rendered BEFORE any probe runs
(`organ_before_run` computes `block` prior to the refalsify branch), so
the injected prompt bytes are refalsify-verdict-independent.

## 1. The claim under test, and what may be said afterward

If the validity gates hold, the licensed sentence is: *"On exact repeats
under refalsify-off, design-§5's measured weight on this corpus and
model is: it contradicted W_A of matched true-but-moot lessons and W_C
of matched right lessons (collateral, via model non-verification), while
on stale lessons it corrected W_B_mint (refresh with a landed re-verify)
and removed W_B_contra (contradiction) of matched retrievals — each with
its 95% Wilson interval (lens: this battery)."* The three lanes'
splits ARE the registered endpoints; there is no pass/fail bar on the
rates themselves — the weight is the number, reported whatever it is.

This battery licenses NO sentence about: any §5 design amendment (the
numbers inform Brice's future ruling; they do not make it — explicitly
out of scope); refalsify-on behavior on these lanes (single-arm design,
§4); the premise_gone shield (premise-gone-battery's settled claim, not
re-litigated); probe cost; novel tasks/models/shapes/accuracy; and every
cross-battery number comparison (the 47/50 above is cited as motivation
only, never compared against this run's numbers — different corpus,
different lock).

## 2. Instrument identity and lens

`s5-weight-battery-v1`. Everything the battery template pinned carries
forward unchanged unless named here: served-identity assertion per
phase, driver/watcher machinery (BOTH untouched this arc — the
per-phase-source and scratch-copy p2 mechanics are key-presence-generic
already), journal-bytes-only recompute plus the store file as a named
quotable source, fresh agent per task at `window_cap 16384`, ITT,
none-vs-zero, task-level `dropped`, scratch-copy manifests (tracked-tree
rule). Daemon build = master `efa8e6a`'s crates tip = `e3cad71` (no
crates change since; re-verified at lock). The lens names the exact
commit, config, and model digest.

## 3. Corpus — `corpus-s5-v1` (new, frozen at its generation commit)

Generator `tools/memory_battery/corpus_s5.py`: same factory draw
discipline (run-verified python tasks, flat two-file workspaces),
**corpus seed 20260830** [judgment: date-convention, distinct from every
prior lock], **n = 48, three lanes × 16**, manifest per-task
`"lane": "control" | "moot" | "stale"`.

- **Lane `control`** (ground truth: lesson right + applicable): no
  `pristine_p2` — phase 2 is the plain byte-reset repeat. §5 firing here
  = collateral poisoning triggered by model non-verification.
- **Lane `moot`** (lesson true-but-inapplicable): the
  premise-gone-battery treatment verbatim — `pristine_p2` with the
  target at defective bytes + the moved-on test (passes on defective,
  fails on the stored fix; `corpus_pg.author_moved_on_test` reused).
  §5 firing here = moot poisoning.
- **Lane `stale`** (lesson wrong): `pristine_p2` with the target at
  defective bytes + a **moved-goal test**: the expected literal replaced
  by a THIRD value that neither the defective output nor the
  stored-fix output produces — the contract moved somewhere new, the
  original goal text now misdescribes the fix, and the stored patch is
  wrong. Contradiction here = correct removal; a phase-2 mint = correct
  refresh (re-verified against the new contract).
- **Third-value synthesis (deterministic, execute-and-pin):** both the
  defective and the fixed outputs are observed by subprocess (the fixed
  via the manifest's own `search`→`replace`); the third value is
  synthesized by type — numbers: `max(defective, fixed) + 7`; strings:
  `fixed + " (rev 2)"`; tuples: `reversed(fixed)`, falling back to
  appending `max(fixed) + 7` — first candidate distinct from both wins;
  two-valued domains (booleans) and unhandled types are EXCLUDED from
  the stale lane (bounded redraw, never a silent shrink). **[judgment:
  the perturbation constants are arbitrary; the registered thing is the
  rule and the executed distinctness, not the constants.]**
- **Witness (satisfiability, executed not asserted):** for every stale
  task the generator emits `witness/<target>` — the defective source
  plus an appended `def <fn>(*args): return <third>` override — proving
  by execution that the moved-goal test is satisfiable by patching the
  target alone. The witness lives OUTSIDE `workspace/`, `pristine/`,
  and `pristine_p2/`, is never scratch-copied, and never reaches the
  model.
- **Lane assignment (deterministic, recorded):** iterate the draw in
  order; a task goes to the first unfilled lane in the priority
  `stale → moot → control` for which it qualifies (stale needs
  third-value authorability; moot needs moved-on authorability;
  control accepts any); tasks qualifying for no unfilled lane are
  skipped. Priority order exists because stale is the scarcest
  (boolean families are excluded from it); the per-lane family mix is
  whatever this deterministic rule yields and is recorded in the
  manifest's `families_by_lane`.
- **Structural checker** `corpus_check_s5.py` (run at freeze AND before
  the real boot): S1/S2 (fails-before, passes-after on the ORIGINAL
  test) for all 48; lane `moot`: the pg checker's S3/S4 verbatim; lane
  `stale`: B1 the moved-goal test FAILS on the defective target, B2 it
  FAILS on the stored-fix target, B3 the witness PASSES it, B4 p2
  target bytes = pristine target bytes, B5 the p2 test differs from the
  original; lane `control`: NO `pristine_p2` key or directory; shas
  (workspace/pristine, and `workspace_p2_sha256` where a p2 exists)
  recomputed independently.

## 4. Arm and phases

**One boot** — arm `s5_off`: `[memory] enabled = true, refalsify =
false` (explicit opt-out of the shipped default, so the flip cannot
silently change the arm's semantics; single-arm because the subject is
§5 under injection, and refalsify-on would shield the moot lane while
leaving the other lanes' injected prompts byte-identical per §0 — a
second on-arm measures nearly nothing new here and is out of scope).
Fresh empty store, fresh scratch `data_dir`, port 8497. Phase 1 = all
48 tasks in manifest order on `pristine/` (defective starts — mints);
phase 2 = same tasks, same order, per-lane workspace source (`control`
resets to `pristine/`; `moot`/`stale` materialize `pristine_p2/` via
the driver's existing key-presence rule).

Goal text is byte-identical across phases per task. **Matched set, per
lane**: phase-2 `MemoryStamp` rows with `mode: "injected"` (flag-off
hits always inject), plus the required zero-oversize `Degraded` scan.
With no second arm there is no H2 analogue; the cross-arm instrument
check's role is taken by the conformance gate V1 and the per-lane
floors.

## 5. Endpoints

`cost(task)`/wall as the template (journal sums, ITT, None+dropped).

**Validity gates (all must hold; violation → run INVALID, no weight is
read):**

- **V1 — §5 conformance, live:** for every phase-2 injected task with a
  scored terminal status: exactly one of {`MemoryMint`,
  `MemoryContradicted`} cites its task_id — with two NAMED exception
  classes counted and expected 0 (a `Degraded` mint/contradict I/O row
  citing the task; a verified-but-Uncomputable mint refusal, visible as
  neither-event-on-a-Done-task); `Error`-status injected tasks are
  excluded as unmeasured and counted. Any unexplained neither/both →
  INVALID (that is a daemon-bug discovery, not a measurement).
- **V2 — stamp audit:** every stamp in both phases carries
  `refalsify: None` (the flag is off); `passed`/`failed` nowhere;
  injected count per lane = matched count per lane; zero oversize
  `Degraded` rows.
- **V3 — per-lane matched floor:** matched ≥ **8** per lane
  **[judgment: 8 = 16/2, the flagged n/2 convention]**, else that
  lane's weight is UNMEASURABLE (reported; the other lanes stand).
- **H3 — infra ≤ 5%** over 96 task-halves (dropped + `Error`).
- Completeness (96/96 halves) and served-identity: CLI-FATAL.

**Registered endpoints (the product; no pass/fail bar):** per lane,
over matched phase-2 tasks: `contradicted` count, `minted` count,
`error_unmeasured` count, `neither` count (V1's named classes), and the
rates W_C (control contradiction), W_A (moot contradiction), W_B_mint /
W_B_contra (stale correction/removal) — each with a **95% Wilson score
interval** (deterministic; no bootstrap, no RNG anywhere in this
instrument).

**Advisory (never gates, no sentence):** per-lane terminal-status
distributions; per-lane p2 token and wall medians (observational, no
bands); p1 mint rate; final store status counts; per-lane counts of
p2 tasks with ≥1 patch step attempted (journal `TaskStep` verb rows —
attempts, not successes, honestly labeled).

**Kill criteria / discipline (carried verbatim):** the point estimate
decides (here: the reported weights ARE the deliverable); no re-run, no
extension, no corpus change after any number is seen; an infrastructure
kill with no numbers read may rerun from zero.

## 6. Machinery deltas (all before the prereg locks, all mutation-tested)

- `corpus_s5.py` + `corpus_check_s5.py` (§3). Mutation checks: wrong
  third value spliced → caught; stale-authorability exclusion skipped →
  caught; witness generation broken → checker B3 fails; lane-assignment
  priority inverted → caught by a lane-composition pin.
- `recompute_s5.py`: single-arm inputs (one data dir, one ledger, the
  corpus dir); lane classification from the manifest; V1/V2/V3/H3,
  weights with Wilson intervals, advisories, completeness, identity
  (CLI-FATAL), dropped, corpus sha. Mutation checks: swap
  mint/contradict classification → caught; break the lane join →
  caught; break the Wilson formula (drop the z²/n term) → caught by a
  hand-computed interval pin; conformance check that ignores `Error`
  exclusion → caught.
- `driver.py` / `dry_manifest.py`: UNTOUCHED (verified by sha in the
  prereg).

## 7. Sequence and the locks

1. This spec committed → plan → machinery + tests (no corpus, no GPU).
2. Corpus generated, checker green, frozen at its commit.
3. Dry shakedown: 3 tasks (one per lane if the first three draws allow,
   else the first 3 in manifest order) through the arm config on the
   live daemon — numbers discarded, marked DRY, label `S5_OFF_DRY`.
4. Prereg committed (the lock).
5. Launch under the recorded delegation; GPU hygiene immediately before
   boot; a held GPU is a STOP.
6. Run the single arm; watcher; teardown verified; journals collected.
7. One `recompute_s5` invocation → findings quoting recompute output
   only → CARRIED-DEBT, memory, merge + push per standing rulings.

## 8. Out of scope

- Any §5 design change (amendment proposals, threshold changes,
  no-patch-needed signals) — Brice's ruling, informed by these weights.
- Refalsify-on lanes; the premise_gone shield; probe cost.
- Battery-v1's memory-on claim; every cross-battery comparison.
- Mint-bar or retrieval-semantics changes.
