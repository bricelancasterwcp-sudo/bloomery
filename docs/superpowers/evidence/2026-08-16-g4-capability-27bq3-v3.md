# G4 FIRST PASS — qwen3.8-27b Q3 under envelope-v3: 20/20

**Date:** 2026-08-16 (launched ~07:20, verdict ~07:45 local — ~25 min wall
INCLUDING the POST; the fixtures themselves took minutes)
**Status:** measured; the pre-registered decision applied — **the gate's
first PASS. `mutating_verbs: true`.**
**Protocol:** `2026-08-15-g4-protocol.md` incl. Amendments 1 (§9), 2 (§10),
3 (§11 — this run is Amendment 3's first exercise; the law-3
termination-not-constraint ruling is recorded there).
**Lens:** **`bloomery-task-envelope-v3`** = v2 (think pre-seed) + task-turn
generation terminating at the first `</action>`. Per §10/§11, per-(model,
envelope) verdicts — this pass licenses mutating verbs when the daemon is
configured with `envelope = "v3"` for this model, which the run config was.

## Verdict

**20 / 20 fixtures landed. Wilson 95% [0.839, 1.0] — the lower bound
clears the 0.80 floor: a NON-PROVISIONAL PASS.**

```json
{"event":"CodecVerdict","model":"qwen3.8-27b:q3","fixture_set":"codec-tasks-v1",
 "codec":"search_replace","landed":20,"n":20,
 "interval95":[0.8388748419471806,1.0],"provisional":false,
 "mutating_verbs":true,
 "detail":"applies_and_parses under bloomery-task-envelope-v3; codec from profile"}
```

Recomputation from the committed `CodecFixture` rows: 20 rows, 20 landed —
equals the verdict. `/status` after: `mutating_verbs: true`,
`kv_per_token: 83886` with `kv_per_token_declared: true`. This matches the
§5 sanity derivation exactly: 20/20 is the smallest perfect score whose
Wilson lower bound (n/(n+z²) = 20/23.84 = 0.8389) clears the floor
non-provisionally at N=20.

## The anatomy: nothing left to fail

- 19 fixtures completed in exactly the minimal **read → patch → done**;
  one in 2 steps (patch → done). 59 steps total.
- **Zero parse failures of any kind**: no `NoAction`, no
  `MultipleActions`, no re-asks, no `WindowExhausted`, no grant
  violations. Every patch landed first try; every task closed with an
  honest, specific `done` summary.
- Turn sizes collapsed: probe-tail completion tokens mean **83**, max
  512, **zero** 1024-cap hits (v1: 133/160 cap hits; v2: the ramble).
  The stop removed the void the model used to talk into.

## The envelope ladder that got here (same model, same blob, same offload, same fixtures, same sampler)

| lens | landed | Wilson 95% | verdict |
|---|---|---|---|
| v1 (raw) | 0/20 | [0.000, 0.161] | demoted — thinking filled the window; zero patch attempts |
| v2 (+think pre-seed) | 10/20 | [0.299, 0.701] | demoted — `MultipleActions` ramble exhausted windows |
| **v3 (+stop at `</action>`)** | **20/20** | **[0.839, 1.0]** | **PASS — mutating verbs earned** |

Each rung's residual failure mode was measured, a targeted amendment was
recorded BEFORE the instrument changed, and the next rung confirmed the
diagnosis by intervention. The model never changed; the envelope stopped
lying about it. (The declared KV override also widened the window
~11.7k → ~31k this run; given zero `WindowExhausted` at v3's tiny turns,
the stop was almost certainly sufficient on its own — the override's
effect is not separately isolated here, recorded honestly.)

## Subject / instrument facts

- `batiai/qwen3.8-27b:q3` (blob `sha256-9af7a683…`) at 50/65 layers,
  `weights_vram_mib = 9890` (declared), `ctx_overhead_mib = 640`,
  `kv_per_token_bytes = 83886` (declared, headroom over the measured
  0.064 MiB/token — spec §10 derivation in the Q3 derivation doc).
- bloomery @ `0d6ae6b` (branch `feat/envelope-v3-kv-override`, PR #9 —
  master + the opus-reviewed Amendment-3/KV change + the stop-scan
  UTF-8 fix). assay pinned `74c5b71`; greedy; `probe_timeout_secs =
  3600` (POST `ok`); preflight `offloaded 50/65` confirmed; no
  competing GPU tenants.

## What this means for the OS

For the first time a locally-served model, on consumer hardware, holds
G4's mutating verbs: `patch` and `run` are live for this (model,
envelope) under the same structural grants, journaling, and fail-closed
machinery as every demoted rung. The gate did what it was built to do in
both directions — refused four rungs honestly, then admitted one on
measured evidence.

## Caveats

- Boots-only; greedy; N=20 (a perfect score's lower bound is 0.839 —
  the interval is honest about how much 20/20 proves).
- Per-(model, envelope): under v1/v2 configs this model remains demoted.
- The v3 envelope limit (§11): a patch whose body must contain the
  literal `</action>` cannot be expressed; no fixture requires it.
- The stop-scan UTF-8 edge (reviewed, fixed, 6 GPU-free pins) means a
  non-UTF-8 accumulation degrades a turn toward v2 behavior at worst —
  none observed in this run (zero MultipleActions).

## Committed artifacts

- `2026-08-16-g4-capability-27bq3-v3-journal.jsonl` (581 events)
- `2026-08-16-g4-capability-27bq3-v3-tasks.jsonl` (59 steps)
