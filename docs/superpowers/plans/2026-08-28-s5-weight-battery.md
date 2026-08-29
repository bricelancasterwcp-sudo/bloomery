# Plan — `s5-weight-battery-v1`

Spec: `docs/superpowers/specs/2026-08-28-s5-weight-battery-v1-design.md`
(binding; formulas cited, never restated). Branch `s5-weight-battery` in
the main checkout — tools/ + docs/ only, zero `crates/` bytes, featured
binary stays code-identical to the `e3cad71` crates tip. Test runner and
mutation discipline as the pg plan (unittest discover; pyc purge).

## Task 1 — `corpus_s5.py` (generator)

Invariants:
- Factory draw at seed 20260830; deterministic lane assignment (spec §3
  priority rule `stale → moot → control`, quotas 16/16/16); manifest =
  pg schema + `"lane"` per task + top-level `families_by_lane`;
  `pristine_p2` only for moot/stale; `"instrument": "s5-weight-battery-v1"`.
- Moot authoring = `corpus_pg.author_moved_on_test` reused verbatim.
- Stale authoring `author_moved_goal_test`: observe defective AND fixed
  outputs by subprocess (fixed via search→replace applied exactly once);
  synthesize the third value by spec §3's type rules with an executed
  distinctness guarantee; splice; also emit the witness source
  (defective + `def <fn>(*args): return <third>` override). Exclusions
  (two-valued domain, unhandled type, raising calls) are redraws.
- Generator-side sanity: moved-goal test FAILS on defective, FAILS on
  fixed, PASSES on witness — hard error if not (the checker re-verifies
  independently).
- Witness materialized at `tasks/<name>/witness/<target>` — outside the
  three run trees.
- Determinism: same (seed, n) → byte-identical fields modulo out_dir.

Falsification tests: hand-built fixture with known defective/fixed
outputs → third value equals the type-rule result and the spliced test
fails-on-both/passes-on-witness (executed); boolean fixture → stale
exclusion; lane quotas and priority pinned on a real small draw
(composition test: with quotas (1,1,1), the first stale-qualified draw
lands in stale even when moot is also unfilled); determinism. Mutations:
splice the fixed output instead of the third → caught; skip the
exclusion → caught.

## Task 2 — `corpus_check_s5.py`

Invariants: spec §3's checker — S1/S2 all lanes; moot = pg S3/S4
verbatim (reuse `corpus_check_pg`'s check functions where exposed);
stale = B1/B2/B3/B4/B5; control = no-p2 assertion; independent shas;
per-task result rows; corpus-level instrument-name check; CLI verdict.

Falsification tests: seeded corpora each breaking one check (B1: p2
test passing on defective; B2: passing on stored fix; B3: broken
witness; B5: p2 test byte-equal; control task with a stray p2 key).
Mutation: invert B2's expected exit → caught.

## Task 3 — generate + freeze `corpus-s5-v1`

Generate into `tools/memory_battery/corpus-s5-v1/`, checker green,
commit (freeze; amendment rule attaches). Record manifest sha in the
ledger.

## Task 4 — `recompute_s5.py`

Invariants: spec §5 verbatim — single-arm loader (`_load_arm` once,
label `s5_off`, dry label `S5_OFF_DRY`; v1 `C`/`M` forbidden); lane
join from the manifest; V1 conformance (named exception classes,
`Error` exclusion), V2 stamp audit, V3 floors (8), H3, weights with
Wilson 95% intervals (formula: score interval, z = 1.959963985...,
pinned by a hand-computed test vector), advisories, completeness +
identity CLI-FATAL, dropped, corpus sha. None-vs-zero.

Falsification tests: synthetic single-arm fixtures — happy path (every
lane's counts land where constructed); a mint+contradict double on one
task → V1 INVALID; a Done-injected task with neither event → V1 named
class surfaces; an `Error` injected task → excluded + counted; a
non-None refalsify stamp → V2 fails; lane floor miss → UNMEASURABLE for
that lane only; digest mismatch → CLI exit 1; Wilson vector: k=47,
n=50 → interval ≈ (0.8299, 0.9752) asserted to 4 decimals (hand-derived
independently). Mutations: swap mint/contradict; break lane join; drop
the z²/n Wilson term; ignore Error exclusion — each caught.

## Task 5 — dry shakedown

`dry_manifest` (dry, 3 tasks — manifest order; lanes as they fall) →
boot the arm config → drive with `--arm S5_OFF_DRY` → teardown → git
status clean. Verify at least one injected p2 stamp and the
mint/contradict events flow; numbers discarded, DRY.

## Task 6 — prereg lock, launch, run, findings

Prereg mirroring the pg battery's (lens incl. crates-tip re-verify;
config verbatim; locked numbers — seed 20260830, floors 8, Wilson z;
machinery shas incl. UNTOUCHED driver/dry_manifest pins; operational
checklist; amendment rule). Commit BEFORE the real boot. GPU hygiene;
single arm; watcher; one recompute; findings; CARRIED-DEBT + memory;
merge + push per standing rulings.
