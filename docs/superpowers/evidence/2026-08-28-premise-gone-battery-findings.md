# premise-gone-battery-v1 — gate findings: **PASS** (all four gates + audits)

**Date:** 2026-08-28 (both arms run back-to-back the same session; run
under Brice's recorded delegation — prereg header). **Lock:**
`docs/superpowers/evidence/2026-08-28-premise-gone-battery-preregistration.md`
at branch commit `4dcbdc5`, committed BEFORE either real boot. Endpoints
computed exactly as locked, by one `recompute_pg` invocation after both
arms completed (`exit=0`); no number was read before both arms finished;
nothing was re-run, extended, or spliced. Every number below traces to
the recompute output committed verbatim at
`docs/superpowers/evidence/2026-08-28-premise-gone-battery-recompute.json`.

## 1. The licensed sentence

All gating conditions passed (PG1, PG2, PG3, the floor, the stamp audit,
H2, H3 — §§3–6 below), so spec §1's sentence is licensed, in full:

> **On exact repeats whose stored verification already passes at task
> start — cited bytes unchanged, the verification contract moved on
> (this corpus's moved-on-test construction) — refalsify-on takes the
> premise_gone lane totally: every matched retrieval stamps
> `premise_gone` and stays silent, and no episode is contradicted or
> store-mutated, while refalsify-off injects the moot lesson on every
> matched retrieval (lens: this battery).**

Nothing here speaks to the staleness-benefit story, the design-§5
passive-poisoning *weight*, probe cost, already-fixed starts, novel
tasks, other models, or accuracy — the named absences (§8), restated
from the spec and prereg.

## 2. Runs

| Arm | Boot | Result |
|---|---|---|
| M′ (`m_prime`, refalsify off) | port 8497, fresh scratch `data_dir`, digest `7020b925…` asserted both phases, ready at poll 27 | driver exit 0; **100/100 task-halves `Done`**; 102/102 ledger rows; teardown clean |
| R (`r`, refalsify on) | port 8498, fresh scratch `data_dir`, digest `7020b925…` asserted both phases, ready at poll 29 | driver exit 0; **100/100 task-halves `Done`**; 102/102 ledger rows; teardown clean |

Daemon: master `e3cad71` crates tip (re-verified empty `crates/` diff at
lock and again at pre-flight), served via the main checkout's featured
vulkan debug build; `readlink /proc/<pid>/exe` confirmed the binary at
boot for both arms; pid via `ps`, never `$!`. Arm order M′→R per the
lock. Pre-flight re-ran `corpus_check_pg` over the frozen corpus
(OVERALL: PASS) and verified GPU hygiene (desktop-only VRAM, `ollama ps`
empty, ports free). `git status` showed ZERO tracked-file modifications
after the full run — the scratch-copy manifest held the tracked-tree
rule by construction.

**Recompute invocation (prereg §6 step 7, unmodified):**

```
PYTHONDONTWRITEBYTECODE=1 python3 -m tools.memory_battery.recompute_pg \
  --corpus-dir tools/memory_battery/corpus-pg-v1 \
  --arm-m-prime-dir .../real/runs/arm-m-prime/data \
  --arm-r-dir .../real/runs/arm-r/data \
  --ledger-m-prime .../real/runs/arm-m-prime/ledger.jsonl \
  --ledger-r .../real/runs/arm-r/ledger.jsonl \
  --expected-digest 7020b925c07c5a3808e1155700ca707598cb4f6d6089bb6daff4147b4d6b00bd
```

`exit=0` (the CLI's own FATAL identity+completeness enforcement passed).
Default floor (25), seed (20260829), B (10,000), labels (`m_prime`/`r`).

## 3. Gates, verbatim from recompute

**PG1 — premise_gone totality (R): PASS.**

```json
"pg1": {"inconclusive_names": [], "injected_names_r_p2": [],
        "premise_gone_count": 50, "premise_gone_not_silent_names": [],
        "premise_held_names": [], "skipped_ungranted_names": [],
        "verdict": "PASS"}
```

All 50 R-p2 matched retrievals probed to `premise_gone`, all silent,
zero injections, zero alarms of any diagnosed class.

**PG2 — store preservation (R): PASS.**

```json
"pg2": {"episode_count": 51, "memory_contradicted_count": 0,
        "non_verified_episode_ids": [], "verdict": "PASS"}
```

Zero `MemoryContradicted` events in R's entire journal; every one of
the 51 episode ids in R's final store is `verified` (50 phase-1 mints;
R-p2 re-minted 6 times — refreshes plus one distinctly-cited episode,
hence 51 ids — all verified).

**PG3 — moot-lesson injection (M′): PASS.**

```json
"pg3": {"floor": 25, "injected_count_m_prime_p2": 50,
        "oversize_degraded_count": 0, "verdict": "PASS"}
```

**Floor: PASS** — `matched_r_p2 = 50`, `injected_m_prime_p2 = 50`,
both ≥ 25. The cited-set construction premise held at 100%: every
phase-1 episode in both arms cited only files whose phase-2 bytes
matched (H4: mint 50/50 p1, matched 50/50 p2, cross-arm gap 0).

## 4. Stamp audit (gating): clean

```json
"stamp_audit": {"counts": {"m_prime": {"1": {"none": 50}, "2": {"none": 50}},
                           "r": {"1": {"none": 50}, "2": {"premise_gone": 50}}},
                "forbidden_spelling_hits": [], "forbidden_spellings_absent": true,
                "m_prime_refalsify_all_none": true, "premise_held_total": 0,
                "r_p1_refalsify_all_none": true}
```

## 5. Hygiene (computed before any gate)

**H2** — p1 token equivalence: `|121.5 − 121.5| = 0.0` within band
`3.891` → `violated: false`, run VALID. **H3** — infra 0/100 both arms,
zero dropped, zero inconclusive. **H4** (advisory) — above.

## 6. Advisory observations (no capability sentence licensed)

- **A1 tokens**: M′-p2 median 118.5 vs R-p2 123.5 (diff +5.0, band
  6.397 — within band). Direction worth recording honestly: the
  *silent* arm spent slightly MORE completion tokens at the median than
  the injected arm — cost here is completion tokens, and M′'s injected
  lesson (prompt-side) plausibly shortcuts the model's own
  investigation on a goal-satisfied workspace. Within its band, owned
  by the staleness-benefit story's future registration.
- **A2 aftermath (the design-§5 feed-forward observation):** in M′,
  **47 of 50** injected episodes ended `contradicted`
  (`memory_contradicted_count: 47`; final store 47 contradicted / 3
  verified — the 3 survivors are the 3 tasks that re-patched and
  re-verified, `mint_count_p2: 3`). Every one of those 47 is a TRUE
  lesson passively poisoned by design-§5's "scored outcome + no
  verifying run → contradict" on a task that legitimately had nothing
  to patch. In R: zero contradictions, all 51 episodes verified,
  6 p2 re-mints. This is the strongest live evidence yet that the §5
  rule's practical weight on goal-satisfied repeats is severe under
  refalsify-off — and that the premise_gone lane is what shields the
  store from it. It licenses NO sentence (named absence); it sharpens
  the future §5 registration's question.
- **A3 wall**: p2 delta −18.0 ms (R faster: 508.5 vs 526.5), p1
  control +4.0 ms. The p2 delta exceeds the control this time, but the
  per-task distribution is heavy-tailed (min −362, max +264; five
  tasks near −300 ms, concentrated in the dict_key and
  variable_reference families) — consistent with M′ tasks doing extra
  moot-lesson-driven work (re-apply the now-wrong patch, watch it
  fail, recover), not with a probe cost, which would push the delta
  the other way. Advisory, never gates, no number quoted as a cost.

## 7. Completeness, identity, dropped

Both arms complete (100/100), `dropped` empty both arms, served digest
`7020b925…` matched in both phases of both arms, corpus sha
`ea0147e22725b0c2cb7668bdcb9d8b7282d382487c11ec051290de910b1cb256`
(the recompute's manifest-derived sha; the manifest FILE's sha256 at
freeze, `642c0873…`, is the prereg §2 pin — two different formulas over
the same frozen bytes, both recorded).

## 8. Named absences (restated)

This battery licenses NO sentence about: the staleness-benefit story
(A1 is an observation); the design-§5 passive-poisoning **weight** (A2
is feed-forward for that registration — it fires the question, not the
answer); any probe-cost number; the already-fixed-start flavor (spec
§0: unreachable under exact retrieval); novel tasks, other models,
other shapes, accuracy; and every cross-battery comparison
(memory-battery-v1, refalsify-battery-v2 — different corpus, different
night; incomparable, not wrong).

## 9. DRY-numbers prohibition (confirmed observed)

No number here originates from the Task-7 shakedown (labels
`M_PRIME_DRY`/`R_DRY`, 3 tasks/arm). Every value above was read from
the single `recompute_pg` invocation over the real run's journals,
ledgers, and stores — the first and only time any of these numbers was
computed.
