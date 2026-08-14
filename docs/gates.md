# Phase 1 — pinned kill gates (G1–G4)

**Date:** 2026-08-14  
**Status:** pre-registered before any instrument exists  
**Amendment protocol:** all values below are frozen. Changes require a recorded protocol amendment executed before re-running any measurement, never tune-and-rerun.

Adopted from the spec's provisional values; Task 1's prior-art pass found no feasibility reason to change any number.

---

## G1 — tiny-model policy (runs Phase 2, pinned now)

**Commitment:** tiny-model policy beats the deterministic heuristic on useful-work-per-GPU-second by ≥10%, contract-violation rate ≤5%, per-decision latency ≤500 ms.

**Pinned metric:** `useful_work = Σ over completed infer calls of completion_tokens × (priority + 1) / 256`, divided by wall-clock GPU-seconds of the run.

**Protocol:** computable from `InferCompleted` + `AgentCreated` journal events alone.

**Kill consequence:** If G1 fails, Phase 2 does not run. The policy comparison must be reproducible from the journal; no live instrumentation is added mid-stream.

---

## G2 — agent switch latency (this plan)

**Commitment:** p95 **warm** agent switch (KV image in RAM, weights resident) ≤ 2000 ms; p95 **cold** switch (weights not resident, image on NVMe) ≤ 5000 ms.

**Note:** Evidence report must present warm and cold results separately. Page-cache caveat for cold switches must be stated in the evidence doc.

**Protocol:** ≥50 switches per class on the enthusiast-16GB tier (declared `--real-hardware`), model qwen2.5-coder:7b-instruct-q8_0, computed by `bloomery-bench report` from `PagerOp` journal events only.

**Kill consequence:** If G2 fails on either warm or cold, this plan's core pacing assumption is invalidated. The pager is not deployed.

---

## G3 — semantic view precision (future)

**Commitment:** semantic view beats grep/fd baseline by ≥15pp top-5 hit rate on a frozen task set.

**Kill consequence:** If G3 fails, the semantic view feature does not ship until the gap is closed.

---

## G4 — per-model codec landing rate (Phase 2)

**Commitment:** per-model codec landing (applies-and-parses lens) ≥80% under the OS envelope, else demotion.

**Kill consequence:** If G4 fails, the codec is demoted: further Phase 2 work treats it as best-effort, never as a hard requirement.

---
