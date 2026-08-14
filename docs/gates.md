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

**Kill consequence:** v1 ships deterministic-only; LLM policy demoted to human-request granularity.

---

## G2 — agent switch latency (this plan)

**Commitment:** p95 **warm** agent switch (KV image in RAM, weights resident) ≤ 2000 ms; p95 **cold** switch (weights not resident, image on NVMe) ≤ 5000 ms.

**Note:** Warm and cold results are presented separately in the evidence report (recorded obligation from Task 1's prior-art pass, docs/priorart/2026-08-14-phase0-priorart.md). Page-cache caveat for cold switches must be stated in the evidence doc.

**Protocol:** ≥50 switches per class on the enthusiast-16GB tier (declared `--real-hardware`), model qwen2.5-coder:7b-instruct-q8_0, computed by `bloomery-bench report` from `PagerOp` journal events only.

**Kill consequence:** the process model is redesigned before anything is built on it.

**Clarification (2026-08-14, recorded at the final review, before any re-run):** G2's protocol line says 'computed ... from PagerOp journal events only'; the pre-registered sample definition (plan Task 17) and the shipped `bloomery-bench report` also consume `ModelLoaded` events for cold-class attribution. Both texts predate the instrument and the measurement; this note reconciles the wording — the sample definition governs. No number was affected.

---

## G3 — semantic view precision (future)

**Commitment:** semantic view beats grep/fd baseline by ≥15pp top-5 hit rate on a frozen task set.

**Kill consequence:** the view stays an app-level index and never gets syscall status.

---

## G4 — per-model codec landing rate (Phase 2)

**Commitment:** per-model codec landing (applies-and-parses lens) ≥80% under the OS envelope, else demotion.

**Kill consequence:** the model is demoted to a narrower verb set or refused for mutating roles.

---
