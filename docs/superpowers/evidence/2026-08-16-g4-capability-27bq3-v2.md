# G4 capability window — qwen3.8-27b Q3 under envelope-v2 (think-preseeded)

**Date:** 2026-08-16 (launched ~04:05, verdict ~06:20 local — ~2.3 h wall
incl. a ~25 min POST; barely a third of the v1 run's wall time)
**Status:** measured; the pre-registered decision applied
**Protocol:** `2026-08-15-g4-protocol.md` incl. Amendments 1 (§9) and
2 (§10 — this run is Amendment 2's first exercise). Amendment 2 was
recorded and pushed BEFORE the instrument change was written.
**Lens:** **`bloomery-task-envelope-v2`** — identical to v1 except the
rendered prompt ends with a pre-closed `<think></think>` block. Per §10,
this rung is NOT comparable to v1 rungs as a single ladder — the v1↔v2
pair for the SAME subject is exactly the comparison it exists for.

## Verdict

**10 / 20 fixtures landed. Wilson 95% [0.299, 0.701]. Not provisional
(the interval sits entirely below the 0.80 floor). Demoted to read-only
under this lens too.**

```json
{"event":"CodecVerdict","model":"qwen3.8-27b:q3","fixture_set":"codec-tasks-v1",
 "codec":"search_replace","landed":10,"n":20,
 "interval95":[0.29929800819821234,0.7007019918017877],
 "provisional":false,"mutating_verbs":false,
 "detail":"applies_and_parses under bloomery-task-envelope-v2; codec from profile"}
```

Recomputation from the committed `CodecFixture` rows: 20 rows, 10 landed —
equals the verdict. The lens name in the verdict is the v2 name, produced
from the same flag read that drove the prompt append (the one-source
property the change's review traced end-to-end).

## The headline: the thinking tax, isolated

Same model, same blob, same offload (50/65), same declared values, same
fixtures, same sampler, same box — **only the lens changed**:

| lens | landed | Wilson 95% | patch attempts | byte-exact misses |
|---|---|---|---|---|
| envelope-v1 (thinking as-is) | 0/20 | [0.000, 0.161] | **0** | — |
| envelope-v2 (think-preseeded) | 10/20 | [0.299, 0.701] | 12 | **0** |

The pre-seed converted a model that never once proposed an edit into one
that lands half the fixtures with a genuine read→patch→done workflow:
16 `read` steps (v1: 5), every landing closed by an honest `done` summary
("Fixed find_max() to use the correct comparison", "Changed listen_port
to 8181", …), landings balanced across lenses (5 python + 5 plaintext),
several fixtures completed in the minimal 3 steps. **Zero SearchNotFound
misses** — when this model reads first, its patches are byte-exact.

## What still fails (all 10 misses = `WindowExhausted`)

The residual disease is the exactly-one-action discipline: 45
`MultipleActions` steps (the model emits a correct action, then keeps
generating hallucinated follow-on turns containing more action blocks) and
19 re-ask terminals. Those re-asks resend the growing transcript, and on
the harder half of the fixtures the ~11.7k window (see the v1 rung's lens
note — the pager's ~4× hybrid-KV overcount shapes it) fills before a
patch lands. Two grant violations were structurally refused (boundary
held, as every rung).

## Decision and honest reading

The §5 rule stands: 10/20 < 80%, interval decided, **demoted** — under
either lens, this model does not earn mutating verbs on this box today.
The pre-registered §10 possibility that v2 might *lower* landing did not
materialize; the opposite did, by ≥50 points. What the pair establishes:

1. For qwen3-family under the raw-completion lens, **thinking verbosity —
   not codec ability — was the binding constraint** (the v1 anatomy's
   conclusion, now confirmed by intervention).
2. The remaining gap to the gate is **turn discipline** (one action per
   turn) and **window headroom**, not byte-exactness. The recorded
   escalations that attack those: the fine-tune flywheel (turn
   discipline is precisely trainable), a bigger window (fewer offloaded
   layers, or closing the hybrid-KV overcount so the same VRAM buys a
   ~4× window), or the Q4-blob rung.

## Subject / instrument facts

- bloomery @ `7d012d1` (branch `feat/envelope-v2-think-preseed`, PR #8 —
  master + the reviewed Amendment-2 change only), assay pinned `74c5b71`,
  greedy sampler, `probe_timeout_secs = 3600` (POST `ok`), preflight
  `offloaded 50/65 layers` confirmed, no competing GPU tenants.
- Fresh data_dir; both journals committed beside this doc; per-fixture
  rows recompute to the verdict.

## Committed artifacts

- `2026-08-16-g4-capability-27bq3-v2-journal.jsonl` (681 events)
- `2026-08-16-g4-capability-27bq3-v2-tasks.jsonl` (102 steps)
