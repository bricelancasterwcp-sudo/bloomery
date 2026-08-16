# Flywheel turn 1 — pre-registration (committed BEFORE any training step)

**Date:** 2026-08-16
**Spec:** `2026-08-16-flywheel-14b-design.md` §6 governs; this document
pins the values. **Amendment protocol:** identical to `docs/gates.md` —
recorded amendments before re-running, never tune-and-rerun.

## Subject

`qwen3-14b-flywheel1` = `Qwen/Qwen3-14B` + the QLoRA adapter trained
below, merged, quantized Q4_K_M — a NEW subject; the stock qwen3:14b's
verdicts are untouched.

## The gate (unchanged, decides alone)

The existing G4 gate: fixture set `codec-tasks-v1` (HELD OUT from
training — proof below), envelope-v3, greedy, the §3+Amendment-1 scoring
conjunction, the §5 decision rule `landed*5 >= n*4`.

- **Success:** landing ≥80% (≥16/20). Nothing softer counts.
- **Baseline:** stock qwen3:14b under v3 = 7/20, Wilson [0.181, 0.567]
  (`2026-08-16-g4-capability-14b-v3.md`).
- **Kill:** tuned < 7/20 → regression; the adapter is shelved and the
  result recorded (withdrawn-claims style). A second turn is a NEW
  pre-registered experiment with a regenerated corpus.
- **Intermediate** (7/20 ≤ result < 16/20): recorded honestly with
  anatomy (reads per run; SearchNotFound count — the trained-habit
  check); the model stays demoted.
- **The point estimate decides.** Wilson recorded; provisional flag per
  the protocol.

## Corpus identity (generated and guarded before this commit)

- Factory: `tools/flywheel/` at branch commit `5ddab0d`; rendering and
  landing verification through `flywheel-tool` (the serving code —
  anti-drift pins in `flywheel_tool_test.rs`).
- Seed **20260816**, requested 1000 → **999 tasks** (1 dedup drop),
  **2,997 pairs** (600 python / 399 plaintext tasks), 13 template
  families. Corpus SHA-256
  `0cb65af32e4d70d0784a7868625597997e0c223059492531a20ac62443c4d3ec`;
  reproducible byte-identically from (code, seed) — the JSONL itself is
  not committed.
- **Contamination guard: clean** — 999 tasks vs 20 gate fixtures, zero
  exact/normalized overlaps (goals, file contents, target filenames,
  search strings) and no goal ≥0.8 Jaccard vs any gate goal. Report
  committed beside this doc (`…-contamination-report.json`); fingerprint
  committed (`…-fingerprint.json`); validation-split task ids are in the
  fingerprint (5%, loss monitoring only, never the gate).

## Training (pinned; *chosen+sanity:* consumer-QLoRA convention)

Base `Qwen/Qwen3-14B` (HF bf16); unsloth 4-bit load; LoRA r=16 α=32 on
q/k/v/o/gate/up/down projections; seq 4096; **completion-only loss**
(prompt masked); **raw text, NO chat template, NO EOS appended** — each
completion ends exactly at `</action>` (v3's stop is the serving
terminator; an EOS habit would leak into v1/v2 behavior); 2 epochs over
~2,850 train pairs (≈700 optimizer steps at bs 1 × accum 8); lr 2e-4
cosine, warmup 20. Environment freeze recorded with the training
evidence.

## Honest possibilities, pre-registered

The read habit trains but byte-exactness doesn't; template-phrasing
overfit that the differently-authored gate set exposes; catastrophic
forgetting visible in the tuned model's own assay POST profile (the
boot probe runs regardless and its profile is part of the evidence).
