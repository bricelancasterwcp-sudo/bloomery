# G4 capability window, rung 2 — qwen3.8-27b Q3 (partial offload)

**Date:** 2026-08-15 (launched 19:33, verdict ~23:05 local — ~3.5 h wall
incl. a ~30 min POST)
**Status:** measured; the pre-registered decision applied
**Protocol:** `2026-08-15-g4-protocol.md` incl. Amendment 1 (§9) — unamended
for this run.
**Derivation:** `2026-08-15-27b-q3-offload-derivation.txt` (the values used).

## Verdict

**0 / 20 fixtures landed. Wilson 95% [0.000, 0.161]. Not provisional.
qwen3.8-27b:q3 is demoted to read-only verbs.**

```json
{"event":"CodecVerdict","model":"qwen3.8-27b:q3","fixture_set":"codec-tasks-v1",
 "codec":"search_replace","landed":0,"n":20,
 "interval95":[0.0,0.16112515805281938],"provisional":false,
 "mutating_verbs":false,
 "detail":"applies_and_parses under bloomery-task-envelope-v1; codec from profile"}
```

Recomputation from the committed `CodecFixture` rows: 20 rows, 0 landed —
equals the verdict; the recomputability obligation holds. `/status` after
the verdict surfaced `mutating_verbs: false` with the same gate numbers.

## The one-line story

**All 20 fixtures ended `WindowExhausted`, at only 7–9 step-records each
(≈2–3 loop steps).** The model never emitted a single `patch` action —
not even a failing one. Amendment 1 scored every fixture instead of
aborting; this run is the amendment's first full-scale exercise.

## Subject and lens

- **Model:** `batiai/qwen3.8-27b:q3` (blob `sha256-9af7a683…`, 12,685 MiB,
  65 offload units) — a DIFFERENT subject from the Q4 blob (model × quant
  is the identity; the Q4 rung remains unmeasured/held).
- **Offload:** 50/65 layers, `weights_vram_mib = 9890` (declared),
  `ctx_overhead_mib = 640`. Preflight confirmed `offloaded 50/65 layers
  to GPU` verbatim.
- **Window:** ~11.7k tokens this boot (static budget − declared weights −
  ctx overhead, at the pager's 0.254 MiB/token charge — its known ~4×
  overcount for this hybrid arch; with truthful KV the same VRAM would
  have given a ~4× larger window. That overcount is part of this lens.)
- **bloomery:** branch `feat/probe-timeout-config` @ `dfb922f` (master
  `94aa4f5` + the reviewed `assay.probe_timeout_secs` change, PR #7 —
  required: the 14B's POST consumed ~600 s of inference and this subject
  is ~3.4× slower; the run set `probe_timeout_secs = 3600` and POST
  completed `ok` in ~30 min).
- **assay:** pinned `74c5b71`; **sampler:** greedy; **envelope-v1** (raw
  completion, no chat template, thinking measured as-is — Brice's
  recorded choice). A feasibility probe before this run showed
  `/no_think` does NOT work under the raw lens, while a pre-seeded
  closed `<think></think>` does suppress thinking — that variant is a
  different lens (envelope-v2), deliberately not used here.

## Step anatomy (155 journaled steps, committed)

| outcome class | count |
|---|---|
| `NoAction` (turn had no action block) | 56 |
| `MultipleActions` (several blocks in one turn) | 54 |
| terminal `unparseable after 2 re-asks` | 40 |
| clean `read` | 5 |
| `patch` attempts (any) | **0** |

133 of the probe's last 160 inference calls hit the 1024-token completion
cap exactly — the same thinking-overrun disease as the 14B, compounded by
a window less than half the 14B's: 2–3 turns of cap-length thinking
(re-asks resend the growing transcript) fill ~11.7k tokens, and the
fixture dies `WindowExhausted` before the model ever proposes an edit.

## The capability window after three rungs

| rung | landed | Wilson 95% | dominant mode |
|---|---|---|---|
| qwen2.5-coder 7B Q8 (full offload) | 0/20 | [0.000, 0.161] | no valid envelope at all |
| qwen3:14b (~Q4, full offload, ~25.6k window) | 1/20 | [0.009, 0.236] | real patches; byte-exact SearchNotFound |
| qwen3.8-27b Q3 (50/65, ~11.7k window) | 0/20 | [0.000, 0.161] | WindowExhausted by thinking; zero patches |

The ladder is NOT monotonic under this envelope, and the three rungs are
not a clean scale comparison — the Q3 rung differs in quant (Q3 vs the
14B's ~Q4), window (11.7k vs 25.6k — the cost of weights on a 16 GB
card under the conservative KV charge), and thinking intensity. What it
does establish: **under envelope-v1 on this box, no local model measured
so far earns mutating verbs**, and for the qwen3 family the binding
constraint is thinking-verbosity against the per-turn cap and the
window, before codec ability is even reached.

Recorded escalation paths, in evidence order: (a) **envelope-v2**
(pre-seeded closed think block — mechanism proven; a recorded amendment
+ its own lens name; would isolate the thinking tax directly);
(b) the **Q4-blob 27B rung** (bigger window at 30/66, no quant confound
vs 14B, ~13–20 h); (c) the **fine-tune flywheel** (the only path that
attacks byte-exact landing itself).

## Enforcement

Read-only verb card + structural dispatch refusal for this model, per
boot, as every demoted rung.

## Caveats

Boots-only; greedy; N=20; the window's boot-VRAM dependence; the pager's
hybrid-KV overcount shaping the window (recorded above, and in
CARRIED-DEBT as the conservative direction). The run binary was the
reviewed PR #7 branch, not yet merged at run time — the diff vs master
is exactly the probe-timeout change.

## Committed artifacts

- `2026-08-15-g4-capability-27bq3-journal.jsonl` (809 events)
- `2026-08-15-g4-capability-27bq3-tasks.jsonl` (155 steps)
