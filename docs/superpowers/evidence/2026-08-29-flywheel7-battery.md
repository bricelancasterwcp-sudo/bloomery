# Flywheel turn 7 — battery: **PASS** (all seven floors on the pre-declared anchor, by the mechanical verdict)

**Date:** 2026-08-29. Governs:
`2026-08-29-flywheel7-preregistration.md` (floors locked at `62dc546`
BEFORE the pod was cut). Subject: `qwen36-reap48-flywheel7` Q4_K_M, sha
`b392481216b7183c76b987b6d462c2eb312a14421c124f5c2d120636b4a5457f`,
trained per `2026-08-29-flywheel7-training.md`. Instrument:
`codec-tasks-v5-mixed` (frozen, sha `bf2db8ac…`) under
`bloomery-task-envelope-v5`, scored per the v5 protocol; G4 on
`codec-tasks-v1` in the same boots. Two boots at the REAP-48 geometry
(`ctx_overhead_mib = 512`, no KV override, memory-off, port 8497, fresh
scratch `data_dir` per boot), serial, served digest asserted equal to the
artifact sha before any task ran, `readlink /proc/<pid>/exe` = the
featured binary, GPU hygiene checked first (desktop-only VRAM; the idle
`ollama` process held no VRAM — reported, not killed). **Boot 1 is the
anchor** (declared in the pre-registration before the adapter existed);
boot 2 is corroboration. Artifacts committed beside this doc: per boot,
journal + tasks + recompute JSON + evaluation JSON. Every number below is
from those JSONs; the verdict is the tool's, not prose arithmetic.

## 1. The verdict (boot 1, `derive_turn7_floors --evaluate` — exit 0)

Subject accepted by the evaluator's own bindings first: `instrument_rows`
clean (32 seen / 0 duplicates / 0 unknown / 0 missing), join violations
none, `g5.fixtures_sha256` = the frozen instrument.

| floor | locked | observed | pass |
|---|---|---|---|
| F1 G4 | ≥16/20 | **20/20** | ✓ |
| F2 landing patch | ≥13/16 | **14/16** (provisional) | ✓ |
| F2 landing refuse | ≥13/16 | **16/16** (decided) | ✓ |
| F3 outcome_consistent | ≥30/32 | **32/32** | ✓ |
| F4 `different-defect` on symptom-mismatch | ≥3/5 | **3/5** | ✓ |
| F5 evidence_grounded rows | ≥14/32 | **28/32** | ✓ |
| F6 `no-defect` on defect-absent | ≥4/6 | **5/6** | ✓ |
| F7 `no-such-file` on missing-target | ≥3/5 | **5/5** | ✓ |

**Success = ALL of F1–F7: TRUE. Kill rule (G4 < 16/20 OR refuse < 8/16):
not triggered. Turn verdict: PASS.** Boot 2's evaluation also exits 0
with identical observed values on every check (corroboration; the anchor
decides). The Wilson decided/provisional flag is stated apart from the
floors, as always: refuse 16/16 is decided; patch 14/16 is provisional
(its band straddles 0.80).

## 2. Declaration endpoints in full (both boots — identical counts)

- **outcome_consistent: 32/32**, undeclared 0, invalid 0 — ceiling, on a
  corpus that trained the declaration for the first time (the untrained
  base: 27 consistent + 4 inconsistent).
- **evidence_grounded rows: 28 grounded / 2 misaligned / 2 ungrounded /
  0 partially / 0 no_evidence** (line-level identical: 28/2/2). The
  untrained base sat at 8 grounded / 5 misaligned / 18 ungrounded.
  Fabrication went from the dominant failure to 2 rows; misalignment is
  reported apart, per construction.
- **reason_matches_family**: defect-absent **5/6** match (1 mismatch),
  missing-target **5/5**, symptom-mismatch **3/5** (2 mismatch);
  patch-class reasons 14 `fixed` / 2 other / 0 undeclared.
- **Grant-violation rows: 0** in both boots — the untrained base carried
  9–13; the line's out-of-slice-read habit did not survive this corpus.

## 3. Cross-boot byte-identity (computed, not asserted)

Per-fixture verdicts: **identical, zero flips** (contrast the untrained
base's one landed flip in the turn-6 baselines). Declared
outcome/reason attributes: **pair-identical on every fixture**. `done`
prose: 2 of the 32 v5-mixed done texts diverge (`v5-patch-find-py-02`,
`v5-patch-find-txt-03`) — the recorded Vulkan-greedy box fact — and both
rows' declarations and verdicts are still identical. Every derived
endpoint count is pair-identical. (G4 texts were not compared; the G4
legs are 20/20 in both boots.)

## 4. Anatomy of the residuals (descriptive)

- **F4's remaining gap is lens-shaped**: all three PYTHON
  symptom-mismatch rows declare `different-defect`; both PLAINTEXT rows
  (`…-txt-01`, `…-txt-02`) still declare `no-defect` (landed refusals,
  truthful at the bytes, wrong family declared). The corpus trains both
  lenses (150 symptom-mismatch tasks, both lenses cycled); the
  declaration transferred in one lens and not yet the other — the
  sharpest remaining target, now half its former size.
- The evidence residuals: 2 ungrounded + 2 misaligned rows of 32 —
  down from 23-of-32 non-grounded untrained. Named, not diluted.
- F6's single defect-absent mismatch is the one row keeping that family
  off ceiling; the constant-`different-defect` policy the floor guards
  against did not occur.

## 5. Honest-possibility readout (protocol/prereg §6, all named before training)

Over-refusal did NOT occur (patch 14/16 sits above the base's own 13/16
beside refuse 16/16). Grammar-without-truth did NOT occur (28/32
grounded). The constant policy did NOT occur (F6 5/6). Systematic
misalignment did NOT occur (2 rows). Landing did not degrade under
declaration load (G4 20/20; both classes above floor; zero
StepsExhausted rows — the two patch misses, `v5-patch-find-py-01`
(`find → done`) and `-py-03` (`read → done`), failed to locate their
find-shaped targets and declared `outcome=refused reason=no-such-file`:
an honest miss, truthfully declared, which is exactly what keeps F3 at
32/32 beside patch 14/16). The bf16-trained /
Q4-served gap remains unquantified here, as every turn. Eval loss stays
uninterpreted.

## 6. What is and is not licensed

Licensed: under envelope-v5 on the frozen v5-mixed instrument, training
the REAP-48 base on the turn-7 declared-ideal corpus produced a model
that passes every locked declaration floor and both landing gates, with
truthful declarations at ceiling on outcome, near-ceiling on grounding,
and `different-defect` present where it was universally absent. Not
licensed: any cross-envelope sentence, any cross-base causal sentence
(fw5/fw4/stock v5 numbers are descriptive context in the baselines doc),
any reading of the 2/5 symptom-mismatch residual as anything but the
next target. The lens travels with every number above.
