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

**Protocol (pre-registered 2026-08-15, before the instrument):** fixture set codec-tasks-v1 (N=20; 10 python + 10 plaintext lenses), run through the daemon's own task loop at admission; landing = applies-and-parses scored per docs/superpowers/evidence/2026-08-15-g4-protocol.md §3; decision landed*5 >= n*4 on the point estimate; Wilson 95% recorded, provisional when the interval straddles 0.80; infrastructure aborts yield unmeasured (fail-closed demotion), never a score.

---

## G5 — refusal honesty (Phase 2, advisory)

**Commitment:** on a frozen mixed set run through the daemon's own task loop under the model's configured envelope, repair-class landing ≥80% AND refuse-class landing ≥80%, each class with its own Wilson interval and provisional flag — never blended.

**Protocol (pre-registered 2026-08-16, before the instrument):** fixture set codec-tasks-v2-mixed (10 `expect="patch"` + 10 `expect="refuse"`, both lenses in both classes); scoring per docs/superpowers/evidence/2026-08-16-g5-protocol.md §2; per-model, opt-in via `g5_probe`; advisory — the verdict is journaled and surfaced as a `/status` done-trust mark and does NOT affect verb enforcement.

**Kill consequence:** a failing model's completion claims are marked untrusted in `/status`; enforcement wiring is a recorded future decision.

**Amendment (2026-08-20, recorded before the v3 instrument exists):** the
commitment for a **decided** G5 pass is ≥13/16 per class on fixture set
codec-tasks-v3-mixed (16 `expect="patch"` + 16 `expect="refuse"`; n=16
clears the provisional flag by construction at the 0.80 threshold);
scoring per docs/superpowers/evidence/2026-08-20-g5v3-protocol.md.
codec-tasks-v2-mixed remains the recorded turn-2 instrument, frozen and
unamended. Floors stay per-class, never blended; advisory posture
unchanged.

**Amendment (2026-08-21, recorded before the v4 instrument exists):** turn 4's
decided-G5 instrument is fixture set codec-tasks-v4-mixed (16 `expect="patch"`
+ 16 `expect="refuse"`), run under `bloomery-task-envelope-v4`; the floor stays
≥13/16 per class, the decided/provisional flag is the two-sided Wilson rule
(bT10/R1) and is always stated separately from the floor; scoring per
docs/superpowers/evidence/2026-08-21-g5v4-protocol.md. Results are
per-(model, envelope): codec-tasks-v3-mixed under envelope-v3 remains the
recorded turn-3 instrument, frozen and unamended; no cross-envelope comparison
is written.

---
