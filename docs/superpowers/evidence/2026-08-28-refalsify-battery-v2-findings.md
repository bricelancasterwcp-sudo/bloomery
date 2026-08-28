# refalsify-battery-v2 — gate findings: **PASS** (probe-cost advisory: **not resolved from box noise**)

**Date:** 2026-08-28 (both arms run back-to-back the same evening; Brice's
launch ruling recorded in `progress.md`). **Lock:**
`docs/superpowers/evidence/2026-08-28-refalsify-battery-v2-preregistration.md`
at branch commit `98b4ad2`, committed BEFORE either boot. Endpoints
computed exactly as locked, by one `recompute_v2` invocation after both
arms completed; no number was read before both arms finished; nothing was
re-run, extended, or spliced. Every number below traces to the recompute
output committed verbatim at
`docs/superpowers/evidence/2026-08-28-refalsify-battery-v2-recompute.json`.

## 1. The licensed sentence

All gating conditions passed (G1, G2, the stamp audit, H2, H3 — §§3–4
below), so spec §1's sentence is licensed. Its `X ms` clause cannot be
filled honestly with the raw p2 number, per the prereg's own A1 honesty
rule (spec §4: "a p1 wall gap of the same order as the p2 gap means the
p2 number is box noise, and the honest report says so instead of quoting
a probe cost") — see §5 below for the check. The licensed sentence,
X-clause resolved honestly:

> **With refalsify on, the memory organ's repeat-exposure benefit is
> preserved — injection and token cost equivalent to refalsify-off
> within the pre-registered bands — at a probe cost not resolved from
> box noise (this battery cannot distinguish it from zero): the p2
> median wall delta was +4.5 ms/task (0.09 ms per probed retrieval), but
> the no-probe p1 control — where neither arm's phase 1 can fire a probe
> at all — shows a same-order gap of −3.5 ms, so the p2 number does not
> clear the box's own noise floor (lens: this battery).**

Nothing here speaks to novel tasks, other models, other task shapes,
accuracy, the `premise_gone` lane, staleness, or the design-§5
passive-poisoning weight — see the named absences, §8 below, restated
verbatim.

## 2. Runs

| Arm | Boot | Result |
|---|---|---|
| M′ (`m_prime`, refalsify off) | port 8497, fresh scratch `data_dir`, digest `7020b925…` asserted both phases, `/memory.refalsify = false` | driver exit 0; **100/100 task-halves `Done`**; 102/102 ledger rows; teardown clean |
| R (`r`, refalsify on) | port 8498, fresh scratch `data_dir`, digest `7020b925…` asserted both phases, `/memory.refalsify = true` | driver exit 0; **100/100 task-halves `Done`**; 102/102 ledger rows; teardown clean |

Daemon: `21a477c` (crates/ tip, re-verified empty-diff against both this
branch's HEAD and `master` immediately before Task 4's boots), served via
the main checkout's featured debug build, digest re-confirmed live at
every boot and inside `recompute_v2`'s own `lens.identity` block (below).
Arm order M′→R per the lock. Full procedure log: Task 4's
`real/RUN-NOTES.md` and `task-4-report.md`; the single recompute output at
`task-5-recompute-stdout.txt`, committed verbatim as
`2026-08-28-refalsify-battery-v2-recompute.json`.

**Recompute invocation (exactly the prereg's §7 Step 4, unmodified):**

```
PYTHONDONTWRITEBYTECODE=1 python3 -m tools.memory_battery.recompute_v2 \
  --corpus-dir tools/memory_battery/corpus-v1 \
  --arm-m-prime-dir .../real/runs/arm-m-prime/data \
  --arm-r-dir .../real/runs/arm-r/data \
  --ledger-m-prime .../real/runs/arm-m-prime/ledger.jsonl \
  --ledger-r .../real/runs/arm-r/ledger.jsonl \
  --expected-digest 7020b925c07c5a3808e1155700ca707598cb4f6d6089bb6daff4147b4d6b00bd
```

`exit=0`. `--expected-arm-labels` omitted (defaults to `("m_prime",
"r")`, exactly this run's labels, per prereg §7).

## 3. Gate G1 — token preservation (equivalence), verbatim from recompute

```json
"g1": {
  "band": 5.325166059382561,
  "diff": 0.0,
  "headroom_m_prime": 80.5,
  "headroom_r": 80.5,
  "median_m_prime_p2": 109.5,
  "median_r_p2": 109.5,
  "n_m_prime_p2": 50,
  "n_r_p2": 50,
  "se_boot": 2.6625830296912807,
  "verdict": "PASS"
}
```

`|109.5 − 109.5| = 0.0 ≤ 2 × 2.6625830296912807 = 5.325166059382561`
holds with wide headroom (median is 80.5 tokens off the observed floor in
both arms — resolution is not floor-saturated; the recompute's own
declared-conservative branch did not fire, and the printed verdict is
`PASS`, never `UNMEASURABLE`). Bootstrap: seed `20260828`, B `10,000`,
resampling unit = tasks, each arm's phase-2 tasks resampled
independently, per prereg §4/spec §4 formula, cited not restated.

## 4. Gate G2 — injection preservation (exact), verbatim from recompute

```json
"g2": {
  "injected_count_m_prime": 50,
  "injected_count_r": 50,
  "reason": null,
  "verdict": "PASS"
}
```

`injected_R,p2 = injected_M′,p2` → `50 = 50`, exact equality, no deficit,
no excess. **PASS.**

## 5. Stamp audit (gating, instrument honesty), verbatim from recompute

```json
"stamp_audit": {
  "counts": {
    "m_prime": {"1": {"none": 50}, "2": {"none": 50}},
    "r": {"1": {"none": 50}, "2": {"premise_held": 50}}
  },
  "forbidden_spelling_hits": [],
  "forbidden_spellings_absent": true,
  "inconclusive_count": 0,
  "offending_premise_held": [],
  "premise_gone_hits": [],
  "premise_gone_zero": true,
  "premise_held_complete": true,
  "skipped_ungranted_count": 0
}
```

Every R-p2 non-`dropped` `mode:"injected"` stamp (50/50) carries
`refalsify:"premise_held"` (`premise_held_complete: true`, matching G2's
`injected_count_r: 50` one-to-one). The forbidden spellings `passed`/
`failed` appear nowhere in either arm (`forbidden_spelling_hits: []`).
`premise_gone` count is 0 (`premise_gone_zero: true`) — no workspace
reset failed. `inconclusive_count` and `skipped_ungranted_count` are both
0 (not merely within H3's tolerance — zero occurrences). **Clean, all
gating conditions satisfied.** M′'s counts read `none` in both phases
(expected: M′ never refalsifies, so the stamp's `refalsify` field is not
applicable there).

## 6. Hygiene (computed before any gate is read, H2 first per prereg §4)

**H2 — first-exposure equivalence (gating):**

```json
"h2_p1_equivalence": {
  "band": 4.198194563142589,
  "diff": 0.0,
  "median_m_prime_p1": 121.5,
  "median_r_p1": 121.5,
  "n_m_prime_p1": 50,
  "n_r_p1": 50,
  "se_boot": 2.0990972815712947,
  "violated": false
}
```

`|121.5 − 121.5| = 0.0` within `2 × SE_boot = 4.198194563142589` →
`violated: false` — **run VALID**, no instrument error. (No probe can
fire in either arm's phase 1; both stores are empty; this is the
cross-arm instrument check.)

**H3 — infra rate (gating, ≤ 5% per arm):**

```json
"h3_infra": {
  "ceiling": 0.05,
  "m_prime_infra_count": 0,
  "m_prime_infra_rate": 0.0,
  "m_prime_task_halves": 100,
  "r_infra_count": 0,
  "r_infra_rate": 0.0,
  "r_task_halves": 100,
  "violated": false
}
```

0/100 both arms, `violated: false` — **no infrastructure kill.**

**H4 (advisory):**

```json
"h4_advisory": {
  "m_prime": {"mint_count_p1": 50, "mint_rate_p1": 1.0, "n": 50, "retrieval_count_p2": 50, "retrieval_rate_p2": 1.0},
  "r":       {"mint_count_p1": 50, "mint_rate_p1": 1.0, "n": 50, "retrieval_count_p2": 50, "retrieval_rate_p2": 1.0}
}
```

50/50 mint in phase 1, 50/50 retrieval in phase 2, both arms — the
deterministic byte-reset corpus hit every episode in both arms, matching
G2's exact counts.

(v1's H1 — control-arm phase stability — has no analogue here, per spec
§4's final paragraph: both arms carry the treatment-relevant store, so
cross-phase within-arm deltas are the organ's intended effect, not a
contamination check.)

## 7. A1 — wall cost (advisory, never gates) — the box-noise check

```json
"a1_wall": {
  "p1_control": {"delta": -3.5, "median_m_prime": 456.0, "median_r": 452.5},
  "p2":         {"delta": 4.5,  "median_m_prime": 483.0, "median_r": 487.5},
  "per_probed_retrieval_ms": 0.09,
  "probed_retrieval_count": 50,
  "per_task_wall_delta_p2": {"max": 126, "median": 2.0, "min": -219, "n": 50}
}
```

**The honesty check (spec §4/prereg §4, applied):** the no-probe p1
control — where zero probes can fire in either arm, both stores being
empty — shows a **−3.5 ms** median wall gap (R minus M′). The probed p2
delta is **+4.5 ms**. `|−3.5|` and `|4.5|` are the same order of
magnitude (ratio ≈1.3×, both single-digit milliseconds against ~450–490
ms task medians) — **the p2 gap does not clear the box's own no-probe
noise floor.** Per the prereg's own rule, the honest report says the
probe cost is not resolved from box noise rather than quoting `0.09 ms`
per probed retrieval as a measured cost. The per-task p2 delta
distribution underscores this: median +2.0 ms but ranging from −219 ms to
+126 ms across 50 tasks (one large negative outlier,
`py_wrong_comparison_operator_run_verified-0011`, at −219 ms) — noise of
that scale swamps a nominal 4.5 ms median difference. **This battery
cannot license a probe-cost number; it can only report that if a probe
cost exists, this instrument's resolution does not reach it.**

## 8. Completeness, identity, dropped — verbatim from recompute

```json
"completeness": {
  "m_prime": {"actual_task_halves": 100, "expected_task_halves": 100, "reason": null, "violated": false},
  "r":       {"actual_task_halves": 100, "expected_task_halves": 100, "reason": null, "violated": false},
  "violated": false
},
"dropped": {"m_prime": [], "r": []},
"corpus_sha": "778b1491aac67f9235ff2ae6ce74c0c767465fb30b2ab5053e17ce99ccc9a5ff"
```

Both arms complete (100/100 task-halves), zero dropped tasks either arm.
`corpus_sha` matches the prereg §3 pinned cross-check value exactly.
Identity, from `lens.identity`:

```json
"identity": {
  "m_prime": {"agree": true, "matches_expected": true, "phase1_digest": "7020b925…", "phase2_digest": "7020b925…", "violated": false},
  "r":       {"agree": true, "matches_expected": true, "phase1_digest": "7020b925…", "phase2_digest": "7020b925…", "violated": false},
  "violated": false
}
```

Served digest matches the pinned expected digest
(`7020b925c07c5a3808e1155700ca707598cb4f6d6089bb6daff4147b4d6b00bd`) in
both phases, both arms — the CLI's own FATAL identity+completeness
enforcement is why the recompute invocation above exited `0` rather than
nonzero.

## 9. Named absences (spec §1, restated verbatim)

> In particular this battery licenses NO sentence about: the
> `premise_gone` lane (no corpus task starts goal-satisfied), the
> staleness-benefit story (no staleness treatment exists here), or the
> design-§5 passive-poisoning weight (the corpus's happy path
> re-verifies, so §5 does not fire on it). Those are **named absences**
> — each needs its own corpus treatment and its own registration.
> Battery-v1's claim (memory-on beats memory-off on repeats) is settled
> evidence and is not re-litigated; no number from this battery may be
> compared against v1's run (different night, materially different
> daemon — window ladder, R9, refalsify itself all landed since;
> incomparable, not wrong).

This run's own stamp audit corroborates the `premise_gone` absence
empirically, not just by corpus design: `premise_gone_hits: []`,
`premise_gone_zero: true` — zero occurrences observed, consistent with
"no corpus task starts goal-satisfied."

## 10. Parked prereg nit (recorded, not amended)

`progress.md`'s Task 3 entry flagged, and this findings doc now records
per that ruling: the prereg's amendment-rule section claims to be
"copied verbatim from v1's prereg" (prereg document line 13 and its
Amendment-rule section), but one clause was adapted, not copied
byte-for-byte. v1's prereg reads *"The corpus is bytes after this commit
(§3.2 above); nothing in `tools/memory_battery/corpus-v1/` is ever
edited in place."* This prereg's own text reads *"The corpus is bytes
(§3 above, re-asserted at this lock); nothing in
`tools/memory_battery/corpus-v1/` is ever edited in place."* — the
self-reference was reworded to match this document's own section
numbering and its own "re-asserted at lock" framing (§3, not §3.2; "at
this lock" added). The binding **force** of the clause is unchanged
(same prohibition, same scope) — this is a precision-of-claims blemish
in the prereg's own self-description, not a substantive deviation, and
per the Task 3 ruling it is **not grounds to amend the lock**: the lock
stands as committed, and this note is the record.

## 11. DRY-numbers prohibition (confirmed observed)

No number in this findings document, or in the committed recompute JSON
it quotes, originates from the task-2 shakedown. Every G1/G2/stamp-audit/
H2/H3/H4/A1/completeness/identity/dropped/corpus_sha value above was
read from the single `recompute_v2` invocation over the real run's
journals (§2 above), the first and only time any of these numbers was
computed. `EVIDENCE-NOTES-DRY.md`'s own numbers (wall-clock ballparks,
the n=3 `UNMEASURABLE` floor-saturation result) are quoted nowhere here,
per prereg §9.

## 12. What this does and does not license

PASS licenses the §1 sentence, X-clause resolved honestly (§1 above):
refalsify preserves the organ's token-cost and injection behavior on
this corpus, this model, this box — and this battery could not resolve
whether refalsify has any wall-clock cost at all, because its own
no-probe control shows equal-sized noise. It does **not** license:
anything about the `premise_gone` lane, staleness, or the design-§5
passive-poisoning weight (§9 above — each needs its own registration);
any cross-battery comparison against `memory-battery-v1` (different
night, materially different daemon); a default-flip of `[memory]
refalsify` (this battery informs that ruling, per spec §7 out-of-scope;
it does not make it); or a resolved probe-cost number (§7 above — a
future battery with more probed retrievals or repeated boots, aimed
specifically at shrinking box noise below a few ms, would be needed to
resolve it, and that is a new registration, not a re-read of this one).
