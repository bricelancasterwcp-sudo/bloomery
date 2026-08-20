# G5-on-v3 baselines — stock qwen3:14b and qwen3-14b-flywheel2

**Date:** 2026-08-20. **Gate:** G5 under `2026-08-20-g5v3-protocol.md`,
fixture set `codec-tasks-v3-mixed` (frozen at `e6c7637`; 16 patch + 16
refuse), envelope-v3, greedy, advisory. Both boots also exercise the G4
probe on `codec-tasks-v1` first (same boot, same daemon) — recorded as
corroborating context, **not** the headline: the v1 baselines already
exist (`2026-08-16-g4-capability-14b-v3.md`, `2026-08-16-flywheel2-battery.md`).
These two runs anchor what flywheel3 must beat (stock) and must hold
(flywheel2). Journals + tasks JSONL committed beside this doc.

---

## 1. Expectations (PRE-REGISTERED — written and committed BEFORE the first boot)

**Written 2026-08-20 ~16:2x CDT, before either daemon was started.** Any
amendment after the first boot is a SEPARATE dated file, never an
in-place edit of this section (standing process rule, `docs/gates.md`
amendment protocol).

**stock qwen3:14b — the floor.** Turn-2 measured it on `v2-mixed` at
patch **4/10**, refuse **2/10**, `done_trust: false`, and on
`codec-tasks-v1` at 7/20 (blind patching: 2 `read` steps across 76).
Expectation: both v3 classes land well below the ≥13/16 floor, refuse
lower than patch, and the dominant refuse-miss leg is (c) — no terminal
`Done` — as in turn 2. The two new patch shapes (find-shaped multi-file,
run-granted) plausibly make its patch class *worse* than the v2 rate,
because a 4-step ideal has only 2 spare turns inside
`FIXTURE_MAX_STEPS = 6` (protocol §6, risk 1) and this model spends
turns guessing. A stock score at or above the floor on either class
would be a genuine surprise and would be recorded as one.

**flywheel2 — the ceiling, and genuinely open.** Turn-2 measured it on
`v2-mixed` at patch **10/10**, refuse **10/10**, `done_trust: true`
(both provisional at n=10 by the pinned property). v3 contains material
flywheel2 **never trained on**:

- the **symptom-mismatch** refusal family (5 fixtures) — turn 2 trained
  only defect-absent and missing-target;
- **find-shaped multi-file** patch fixtures (6) whose goal never names
  the target file;
- **run-granted** patch fixtures (5) with a `py_compile` grant.

So flywheel2's v3 verdict is **not** a foregone 16/16 in either class.
Named honest possibilities, in advance: it may refuse correctly on the
two trained families and patch-anyway on symptom-mismatch (the turn-1 →
turn-2 leg-(a) failure re-appearing on the untrained family); it may
fail find-shaped fixtures by patching a sibling or by exhausting steps
while hunting for the file; run-granted fixtures may cost it a turn it
does not have. Any of {both classes pass, one passes, neither passes} is
a valid pre-registered outcome. **Neither model is re-run for a nicer
verdict.** A single boot per model; whatever it says is the record.

**Secondary endpoints, pre-registered as non-gating** (protocol §5, and
the controller ruling that they are never pass/fail): find-verb usage on
the 6 find-shaped patch fixtures; run-before-done on the 5 run-granted
patch fixtures; per-family refuse breakdown against denominators 6
defect-absent / 5 missing-target / 5 symptom-mismatch. **Neither model
was trained to use `find` or `run`** (both verbs enter the corpus in
turn 3, which does not exist yet), so low counts here are the expected,
uninteresting result — they are recorded as the *before* half of turn
3's find/run comparison, not as a deficiency.

**Reporting discipline pinned in advance** (controller ruling bT1/R1):
the pass floor (≥13/16 per class) and the Wilson decided/provisional
flag are reported as SEPARATE facts. n=16 makes a decided pass
*reachable* (16/16 → lower bound ≈ 0.806 > 0.80); it does not make every
pass decided (13/16 → lower ≈ 0.570 → provisional). The phrase "decided
by construction" (spec §5) is **not** used of any score in this
document; it describes only the reachability property of n=16.

---

## 2. Method

(filled in as the runs execute — preflight facts below were established
before the pre-registration above was committed)

**Preflight, 2026-08-20:**

| item | value |
|---|---|
| bloomery tree | `master` @ `5deb4a6` (turn-3 code arc merged) |
| Rust suite | `cargo test --workspace` → **665 passed, 0 failed** (run BEFORE the featured build) |
| assay pin | `PYTHONPATH=/home/brice/workspace/assay/src`, assay **0.13.0** @ `bdb7f92`, working tree clean |
| GPU | RTX 5080, 16303 MiB total, 1059 MiB in use by the desktop → ~15.2 GiB free; no bloomery daemon running |
| stock GGUF | `/mnt/extra/ollama-models/blobs/sha256-a8cc1361…c40e` (the `ollama show qwen3:14b --modelfile` FROM path), 9,276,184,896 bytes, sha256 `a8cc1361f3145dc01f6d77c6c82c9116b9ffe3c97b34716fe20418455876c40e` — **verified, matches the blob name and the turn-2 boot's model** |
| flywheel2 GGUF | `/home/brice/flywheel2/qwen3-14b-flywheel2-Q4_K_M.gguf`, 9,001,752,960 bytes, sha256 `9659b96cbf3b30c8d03da18d9179ddaf7b7e9fb85597f99de9c721140ab5e09d` — **verified, byte-identical to the sha recorded in `2026-08-16-flywheel2-battery.md`** |
