# s5-weight-battery-v1 — findings: validity **PASS**; the weights are read

**Date:** 2026-08-29 (machinery + dry shakedown the prior evening, same
session; run under Brice's recorded delegation — prereg header).
**Lock:**
`docs/superpowers/evidence/2026-08-29-s5-weight-battery-preregistration.md`
at branch commit `ab7627b`, committed BEFORE the real boot. Endpoints
computed exactly as locked, by one `recompute_s5` invocation after the
arm completed (`exit=0`); no number read before it finished; nothing
re-run, extended, or spliced. Every number below traces to the recompute
output committed verbatim at
`docs/superpowers/evidence/2026-08-29-s5-weight-battery-recompute.json`.

## 1. The licensed sentence, numbers filled

All validity gates held (V1, V2, V3, H3, completeness, identity — §§3–4
below), so spec §1's sentence is licensed with its registered numbers:

> **On exact repeats under refalsify-off, design-§5's measured weight on
> this corpus and model is: it contradicted 16/16 matched true-but-moot
> lessons (W_A = 1.0, Wilson 95% [0.8064, 1.0000]) and 0/16 matched
> right lessons (W_C = 0.0, [0.0000, 0.1936] — zero collateral; every
> control lesson re-verified and refreshed), while on stale lessons it
> corrected 1/16 (W_B_mint = 0.0625, [0.0111, 0.2833]) and removed
> 15/16 (W_B_contra = 0.9375, [0.7167, 0.9889]) of matched retrievals
> (lens: this battery).**

Read together, per the lanes' construction-certified ground truth: §5's
true-positive lane works — no stale lesson survived stale (15 removed,
1 corrected) — and its false-positive weight on goal-satisfied repeats
is **total**: every true-but-moot lesson was destroyed. The rule cannot
distinguish moot-true from stale-wrong; both present identically to it
(injected, scored, no verifying run). Zero collateral on right lessons
means the weight is concentrated exactly where the premise-gone lane's
shield operates. No aggregate "precision" number is quoted — any such
number is a function of the constructed lane mix, not of the rule.

Per spec §1 this licenses NO §5 design change (that is Brice's ruling,
informed by these weights), nothing about refalsify-on lanes, the
premise_gone shield, probe cost, novel tasks/models, or any
cross-battery comparison (the motivating 47/50 stays motivation).

## 2. Run

| Arm | Boot | Result |
|---|---|---|
| `s5_off` (refalsify off, port 8497) | fresh scratch `data_dir`, digest `7020b925…` asserted both phases, ready at poll 29 | driver exit 0; **96/96 task-halves `Done`**; 98/98 ledger rows; teardown clean |

Daemon: crates tip `e3cad71` (empty `crates/` diff re-verified at lock
and pre-flight), the main checkout's featured vulkan build;
`readlink /proc/<pid>/exe` confirmed at boot; pid via `ps`. Pre-flight
re-ran `corpus_check_s5` (OVERALL: PASS) and GPU hygiene; `git status`
showed ZERO tracked-file modifications after the run; the scratch-copy
manifest carried no `witness/` directories (verified — witnesses never
reached the run tree).

**Recompute invocation (prereg §6 step 6, unmodified):**

```
PYTHONDONTWRITEBYTECODE=1 python3 -m tools.memory_battery.recompute_s5 \
  --corpus-dir tools/memory_battery/corpus-s5-v1 \
  --arm-dir .../real/runs/arm/data \
  --ledger .../real/runs/arm/ledger.jsonl \
  --expected-digest 7020b925c07c5a3808e1155700ca707598cb4f6d6089bb6daff4147b4d6b00bd
```

`exit=0`; default floor (8) and label (`s5_off`).

## 3. Validity gates, verbatim from recompute

```json
"v1_conformance": {"both_event_names": [], "degraded_explained_names": [],
                   "neither_event_names": [], "unscored_in_matched_names": [],
                   "verdict": "PASS"}
"v2_stamp_audit": {"forbidden_spelling_hits": [], "injected_p1_count": 0,
                   "non_none_spellings": [], "oversize_degraded_count": 0,
                   "refalsify_all_none": true, "violated": false}
"v3_floors": {"floor": 8, "lanes_under_floor": [],
              "matched_by_lane": {"control": 16, "moot": 16, "stale": 16},
              "verdict": "PASS"}
"h3_infra": {"ceiling": 0.05, "dropped_halves": 0, "infra_count": 0,
             "infra_rate": 0.0, "task_halves": 96, "violated": false}
```

V1's live conformance check of the code-entailed mint-xor-contradict
totality held on all 48 matched tasks (spec §0's discipline: checked,
never quoted as a result). Retrieval matched 16/16 in every lane — the
cited-set construction premise held at 100% for the third battery
running. Completeness 96/96; identity matched both phases; `dropped`
empty. Corpus provenance: the recompute's manifest-derived `corpus_sha`
is `f7c75e1c6c66b433ee13d6ff9a64a3107f5be0c0f608b2c641b2e96872cb2d30`;
the manifest FILE's sha256 at freeze, `f5d415ff…`, is the prereg §2
pin — two formulas over the same frozen bytes, both recorded (the pg
findings' §7 convention).

## 4. The weights, verbatim from recompute

```json
"control": {"matched": 16, "denominator": 16, "contradicted": 0, "minted": 16,
            "neither": 0, "rate_contradicted": 0.0, "rate_minted": 1.0,
            "wilson_contradicted": [0.0, 0.1936076805344365]}
"moot":    {"matched": 16, "denominator": 16, "contradicted": 16, "minted": 0,
            "neither": 0, "rate_contradicted": 1.0,
            "wilson_contradicted": [0.8063923194655636, 1.0000000000000002]}
"stale":   {"matched": 16, "denominator": 16, "contradicted": 15, "minted": 1,
            "neither": 0, "rate_contradicted": 0.9375, "rate_minted": 0.0625,
            "wilson_contradicted": [0.7167126242970107, 0.9888806552353575],
            "wilson_minted": [0.011119344764642517, 0.2832873757029894]}
```

## 5. Advisory observations (no sentence licensed)

- Every phase-2 task in every lane attempted at least one patch
  (`p2_patch_attempt_tasks: 16/16/16` — attempts, not successes). In
  the moot lane that is the sharpest reading of the poisoning
  mechanism: the goal drives a patch, the moved-on contract makes the
  stored fix FAIL its own verification, so even a model that patches
  cannot produce a verifying run — §5's conjuncts close on every path
  a goal-satisfied repeat can take short of independently discovering
  the new contract.
- The one stale-lane correction (`minted: 1`) is the only phase-2 task
  that discovered the moved goal and landed a re-verified fix; the
  other 15 stale lessons were removed by contradiction. All 96
  task-halves ended `Done`.
- Final store: 31 contradicted (16 moot + 15 stale) / 17 verified
  (16 control refreshes + 1 stale correction). p1 mint rate 48/48.
- Token/wall medians per lane (p2): control 108.5 / 494.0 ms; moot
  112.0 / 516.5 ms; stale 109.0 / 523.5 ms. Observational only.

## 6. Named absences (restated)

No §5 amendment is proposed or licensed here; refalsify-on behavior on
these lanes, the premise_gone shield, and probe cost are out of scope;
no number here may be compared against memory-battery-v1,
refalsify-battery-v2, or premise-gone-battery-v1 (different corpora,
different locks; the 47/50 that motivated this registration is cited as
the fired question only, never as a comparand).

## 7. DRY-numbers prohibition (confirmed observed)

No number here originates from the shakedown (label `S5_OFF_DRY`, 3
tasks). Every value above was read from the single `recompute_s5`
invocation over the real run's journal, ledger, and store — the first
and only time any of these numbers was computed.
