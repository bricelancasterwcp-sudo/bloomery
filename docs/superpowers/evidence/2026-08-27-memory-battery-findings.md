# memory-battery-v1 — gate findings: **PASS**

**Date:** 2026-08-27 (both arms run back-to-back the same night; Brice's go:
"go, gpu is free"). **Lock:** `docs/superpowers/evidence/2026-08-26-memory-battery-preregistration.md`
at branch commit `7493080`, pushed to origin BEFORE either boot. Endpoints
computed exactly as locked, by one `recompute` invocation after both arms
completed; no number was read before both arms finished; nothing was re-run,
extended, or spliced.

## 1. The licensed sentence (spec §1, PASS form)

**On `memory-battery-v1`, exact-repeat injection reduced median
second-exposure completion-token cost by 10.0 tokens (121.5 → 111.5) against
a derived bar of 6.11.** Nothing here speaks to novel tasks, other models,
other task shapes, or accuracy.

## 2. Runs

| Arm | Boot | Result |
|---|---|---|
| C (memory off) | port 8396, fresh scratch `data_dir`, G4 **20/20 non-provisional**, digest `7020b925…` asserted both phases, `/status.memory.enabled = false` | driver exit 0; **100/100 task-halves `Done`**; 2 identity rows ok; 0 suspend failures |
| M (memory on, store empty) | port 8395, fresh scratch `data_dir`, G4 **20/20 non-provisional**, digest asserted both phases, memory enabled | driver exit 0; **100/100 `Done`**; 2 identity rows ok; 0 suspend failures |

Daemon: master's featured build at commit **`a5879b7`** (the prereg's
deliberately-unpinned lens variable, recorded here as it required), Vulkan
symbol verified, booted from `/home/brice/workspace/bloomery`; tools and
frozen corpus from the worktree at `7493080`. Arm order C→M per the lock.
Archives (journals, ledgers, store file, daemon logs) under
`.superpowers/sdd/2026-08-26-memory-battery/runs/arm-{c,m}/archive/`;
the single gate output at `runs/gate-output.json`. The frozen corpus
workspaces were `git restore`d to their locked bytes after arm M
(48 of 50 carried landed patches — see §5).

## 3. The gate, verbatim from recompute

```json
"verdict": "PASS",
"e1": {
  "delta_min": 6.108895071942225,
  "headroom": 15.5,
  "median_c_p2": 121.5,
  "median_m_p2": 111.5,
  "min_c_p2": 106,
  "n_c_p2": 50, "n_m_p2": 50,
  "se_boot": 3.0544475359711125,
  "verdict": "PASS"
}
```

`111.5 ≤ 121.5 − 6.108…` holds. The headroom clause did not fire
(`121.5 − 106 = 15.5 ≥ Δ_min`). Hygiene, evaluated in the locked order, all
clean: arm completeness 100/100 both arms; identity and treatment-identity
(every C stamp `mode:"off"`, every M stamp `silent`/`injected`, arm labels
exact); H1 control stability diff 0.0; H2 first-exposure equivalence diff
0.0 (bound 4.26); H3 infra 0/100 both arms. `dropped`: C 0, M 0.

## 4. Advisory endpoints (reported, never gating)

- **Mechanism saturation in the good direction:** mint rate M-p1 **50/50**,
  injection rate M-p2 **50/50** — the two-stage exact match (goal hash +
  pre-first-touch fingerprints) hit every byte-reset repeat.
- **Steps median:** M-p2 **3.0** vs 4.0 in every other cell — the injected
  repeat typically skips one step of its trajectory.
- **Within-M paired deltas:** median −16.0 tokens (n=50 pairs) — the paired
  form of the same effect E1 measures cross-arm.
- **Wall medians (ms):** c_p1 453.0, c_p2 445.5, m_p1 442.5, **m_p2 476.5**
  — injected repeats are ~7% *slower* by wall despite fewer tokens: the
  injected block lengthens the prompt, and prefill outweighs the decode
  saving at this task size. An honest echo of crucible's B4 (retrieval
  slowed its 14B +10% wall); the token saving, not a wall saving, is what
  this gate licensed.
- **Success rates:** 1.0 in all four cells, under the lock's pre-declared
  saturation note (ceiling models; success was never this battery's gate).

## 5. The two contradictions — the organ's honesty path, live at scale

`row_counts`: M minted 50 (phase 1) + 48 refreshes (phase 2) and journaled
**2 `MemoryContradicted`** rows; the store ended `episodes: 50, verified:
48, contradicted: 2`. The two contradicted tasks' phase-2 runs ended `Done`
WITHOUT a landed patch (their workspace files stayed byte-identical to the
frozen corpus — the post-run `git restore` touched 48 files, not 50), so
the scored-outcome contradiction rule retired the episode each had been
shown. Both tasks still count in E1's median at their full cost (ITT).
That is slice 1's §5 semantics doing exactly what it was specified to do
on the first night it met scale.

## 6. Lens (from recompute output)

Bootstrap B=10,000 seed 20260826; corpus seed 20260826, corpus sha
`778b1491…` (manifest), freeze sha `d9df82e2…` (prereg §3.2); digests
`7020b925…` on all four phase asserts; envelope v4; window_cap 16384;
model name `qwen36-reap48-flywheel5`; driver constants 5.0 s poll /
600 s deadline. Deviations from the lock: **none** — no amendment was
needed after the pre-lock §5.1 correction wave.

## 7. What this does and does not license

PASS licenses the §1 sentence: repeats get cheaper in tokens, on this
corpus, this model, this box. It does not license: token savings at other
task sizes (the ~8% here is one corpus's number), wall-clock savings (the
advisory says the opposite at this size), accuracy claims (ceiling), or
anything about non-exact retrieval (the Phase-C prohibition stands). The
natural next questions — larger/harder tasks where the prefill-vs-decode
balance shifts, and the hardened-accuracy battery — are new
pre-registrations per the sequencing ruling.
