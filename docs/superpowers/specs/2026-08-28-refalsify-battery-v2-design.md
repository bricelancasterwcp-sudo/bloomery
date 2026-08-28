# `refalsify-battery-v2` — cost-and-preservation instrument for the v2 probe

The battery slice re-registered against refalsify v2 (the v1 registration
was cancelled by the 2026-08-28 domain-of-validity erratum: every one of
its endpoints was entailed pre-boot). Under v2's premise verdicts the
outcomes are genuinely unknown again, and this instrument buys the
default-availability decision: what `[memory] refalsify = on` costs, and
whether it preserves the organ's measured repeat benefit.

Approved shape (Brice, 2026-08-28): M′-then-R same night; G1
token-equivalence + G2 injection-equality gates; stamp audit; wall cost
advisory; named absences; prereg-first sequence with a human launch gate.

## 1. The claim under test, and what may be said afterward

If the gates pass, the licensed sentence is: *"With refalsify on, the
memory organ's repeat-exposure benefit is preserved — injection and token
cost equivalent to refalsify-off within the pre-registered bands — at a
measured probe cost of X ms wall per probed retrieval (lens: this
battery)."* Nothing else. In particular this battery licenses NO sentence
about: the `premise_gone` lane (no corpus task starts goal-satisfied),
the staleness-benefit story (no staleness treatment exists here), or the
design-§5 passive-poisoning weight (the corpus's happy path re-verifies,
so §5 does not fire on it). Those are **named absences** — each needs its
own corpus treatment and its own registration. Battery-v1's claim
(memory-on beats memory-off on repeats) is settled evidence and is not
re-litigated; no number from this battery may be compared against v1's
run (different night, materially different daemon — window ladder, R9,
refalsify itself all landed since; incomparable, not wrong).

## 2. Instrument identity and lens

`refalsify-battery-v2`. Everything battery-v1 pinned carries forward
unchanged unless named here: the frozen corpus-v1 (same manifest sha,
re-asserted at lock), the served model identity asserted per phase, the
driver/watcher machinery, journal-bytes-only recompute discipline, fresh
agent per task at `window_cap 16384`, full workspace byte-reset +
`__pycache__` purge between phases, intent-to-treat cost accounting,
none-vs-zero, task-level `dropped` lists. The daemon build is the merged
refalsify-v2 master (`21a477c` or a successor recorded at lock) — the
lens names the exact commit and config per arm.

## 3. Arms and phases

Two boots, pre-registered order **M′ then R**, same night:

- **Arm M′**: `[memory] enabled = true`, `refalsify = false`. Store
  starts EMPTY in a fresh scratch `data_dir`. Phase 1 = tasks 1–50 in
  manifest order (mints); phase 2 = same tasks, same order (retrieves and
  injects). The v2-era baseline.
- **Arm R**: `[memory] enabled = true`, `refalsify = true`. Identical
  phases, its own fresh empty store. Phase 2's retrievals are probed;
  under the erratum's analysis every probe re-runs the planted
  `python3 -m unittest` on the reset (defective) workspace, exits
  nonzero, and stamps `premise_held` — which is precisely the prediction
  the stamp audit checks rather than assumes.

Probes cannot fire in either arm's phase 1 (empty store → nothing
retrieved → nothing probed), which makes the two phase-1s a cross-arm
instrument check (H2 below).

## 4. Endpoints, bars, kill criteria

`cost(task)` exactly as battery-v1 §4: summed `completion_tokens` over
the task's agent's `InferCompleted` journal rows; ITT; unparseable =
`None` + `dropped`, never zero. Wall per task from the journal's own
task rows (the v1 recompute's `wall_ms` view), never from driver
observations.

**Gate G1 — token preservation (equivalence):**

> `|median_R,p2 − median_M′,p2| ≤ 2 × SE_boot(median_R,p2 − median_M′,p2)`
> — seeded bootstrap B = 10,000, seed **20260828**, locked here as a
> formula and computed only at gate time; resampling unit = tasks, each
> arm's phase-2 tasks resampled independently; medians over non-`dropped`
> tasks. An equivalence bar, derived not chosen: refalsify-on must not
> move the token cost outside its own noise. (The probe itself never
> enters a prompt, so any token movement is downstream behavior change —
> exactly what this gate exists to catch.)

**Gate G2 — injection preservation (exact):**

> `injected_R,p2 = injected_M′,p2`, counted from `MemoryStamp`
> `mode:"injected"` rows over non-`dropped` tasks. The corpus is
> deterministic and byte-reset; a deficit in R means the probe silenced
> or poisoned an episode — the failure v2 exists to prevent — and FAILS.
> An excess is impossible by construction; observing one is an
> instrument alarm, not a pass.

**Stamp audit (gating, instrument honesty):** over R-p2's non-`dropped`
tasks, every `mode:"injected"` stamp carries `refalsify:"premise_held"`,
and the spellings `passed`/`failed` appear **nowhere** in either arm
(they are unreachable under v2 — one occurrence means the served build
is not the locked build). `premise_gone` expected count: 0 — any
occurrence means a workspace reset failed (the goal was already
satisfied) → investigate before reading gates; it is an instrument
alarm, not task data. `inconclusive` (probe timeout/spawn) and
`skipped_ungranted` expected 0; tolerated within H3's infra budget,
counted and named individually.

**A1 — the purchased number (advisory, never gates):** `median wall_R,p2
− median wall_M′,p2`, reported beside the per-probed-retrieval derivation
(that delta ÷ probed-retrieval count) and beside the no-probe control
`median wall_R,p1 − median wall_M′,p1` — the control bounds box drift:
a p1 wall gap of the same order as the p2 gap means the p2 number is
box noise, and the honest report says so instead of quoting a probe
cost. Per-task wall deltas also reported as a distribution summary.

**Hygiene (computed before any gate is read, in this order):**

- H2 — first-exposure equivalence: `|median_M′,p1 − median_R,p1|` within
  `2 × SE_boot` (tokens). No probe can fire in p1, both stores are
  empty, so a gap is instrument error → run **INVALID**.
- H3 — infra rate ≤ 5% per arm (task-level `Error`, daemon faults,
  driver protocol breaks — counted apart from task conduct, `dropped`
  for every endpoint). Above 5% → **infrastructure kill**: rerun from
  zero only if no gate number was read.
- H4 (advisory): mint rate in each arm's p1; retrieval rate in each
  arm's p2.

(v1's H1 — control-arm phase stability — has no analogue here: both
arms carry the treatment-relevant store, and cross-phase within-arm
deltas are the organ's intended effect, not a contamination check.)

**Kill criteria / discipline (v1 rules carried verbatim):** the point
estimate decides; no re-run, no extension, no corpus change after any
number is seen; an infrastructure kill with no numbers read may rerun
from zero; floor-saturation: if `median` differences sit under the
band's resolution because the cost distribution is floor-saturated,
the verdict is **UNMEASURABLE**, not PASS (equivalence must be earned
by resolution, not granted by noise).

## 5. Machinery deltas (all before the prereg locks, all mutation-tested)

- **Recompute**: extended (or wrapped — implementer's structural call,
  reviewed) to (a) name the arms honestly as `m_prime`/`r` — battery-v1's
  `c_/m_` slot names must not be reused for different semantics; (b)
  emit the per-arm×phase refalsify-spelling counts for the stamp audit;
  (c) emit G1/G2/A1/H2 exactly as §4 states them, with lenses. Each new
  computation mutation-tested before the corpus is touched (broken
  median, broken join, broken spelling count, swapped arms — each must
  fail a test).
- **Driver/scripts**: unchanged unless the per-arm daemon config
  (`[memory] refalsify`) needs a launch-side seam; if the arm boots are
  hand-configured (v1 pattern: one boot per arm), the prereg's
  operational checklist pins each arm's config file content and the
  `/status` assertion that verifies it live before task 1.
- **Prereg doc** (`docs/superpowers/evidence/2026-08-28-refalsify-battery-v2-preregistration.md`),
  committed BEFORE any GPU run, mirroring v1's: claim discipline quoted,
  lens pins (daemon commit, config per arm, model digest), corpus sha
  re-assertion, §4 by reference plus every locked number (seed, B,
  bars), machinery file shas at lock, operational preconditions (GPU
  hygiene: ollama models stopped; box quiet; watcher armed), amendment
  rule.

## 6. Sequence and the locks

1. This spec approved → plan → machinery deltas + tests (no corpus, no
   GPU).
2. Dry run: 3 corpus tasks through both arm configs on the live daemon
   (numbers discarded, marked DRY — instrument shakedown only; the
   capture-once rule does not attach to a dry run's numbers, and none
   may be quoted).
3. Prereg doc committed (the lock).
4. **Brice's launch gate** for the overnight detached run.
5. Run M′ then R; watcher; journals collected.
6. Recompute → findings doc quoting recompute output only → CARRIED-DEBT
   and memory updates.

## 7. Out of scope

- Any corpus treatment (staleness lanes, goal-satisfied starts) — the
  `premise_gone` and benefit stories are future registrations.
- Any change to refalsify v2 semantics, the organ, or the window law.
- Re-litigating battery-v1's memory-on claim; any cross-battery number
  comparison.
- Default-flipping `[memory] refalsify` — the findings inform that
  ruling; they do not make it.
