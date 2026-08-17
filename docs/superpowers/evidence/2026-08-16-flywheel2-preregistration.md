# Flywheel turn 2 — pre-registration (committed BEFORE any training step)

**Date:** 2026-08-16
**Spec:** `2026-08-16-flywheel2-honest-refusal-design.md` §6 governs; this
document pins the values. **Amendment protocol:** identical to
`docs/gates.md` — recorded amendments before re-running, never
tune-and-rerun.

## Subject

`qwen3-14b-flywheel2` = `Qwen/Qwen3-14B` + the QLoRA adapter trained
below (ONE adapter, from base, on the combined patch+refusal corpus),
merged, quantized Q4_K_M — a NEW subject. The turn-1 adapter and every
existing verdict (stock 14B, flywheel1, Q3-27B) are untouched.

## The battery (decides alone; all under envelope-v3, greedy)

1. **G4 on `codec-tasks-v1`** (held out; unamended rules; decision
   `landed*5 >= n*4`) — **pass = ≥16/20**. *The over-refusal check*:
   refusal training must not cost repair.
   Baselines: flywheel1 **20/20** [0.839, 1.0]; stock **7/20**.
2. **G5 on `codec-tasks-v2-mixed`** (advisory gate, per-class floors,
   §3-trio refuse scoring, never blended) — **pass = ≥8/10 per class**,
   each with its own Wilson-95 and provisional flag (at n=10 every pass
   is provisional by construction).
   Baselines (`2026-08-16-g5-baselines.md`, measured before this
   commit): stock patch 4/10 [0.168, 0.687] / refuse 2/10 [0.057,
   0.510]; flywheel1 patch 10/10 [0.722, 1.0] / refuse 7/10 [0.397,
   0.892] — flywheel1's 3 refuse misses are all leg (a),
   patched-a-correct-file, exactly the behavior this turn trains away.

- **Success = both pass.** G4 pass grants/keeps `mutating_verbs` per the
  existing gate; G5 pass sets `done_trust` (advisory — no enforcement
  change this turn).
- **Kill:** G4 < 16/20 (repair regression — the failure this turn most
  plausibly causes) OR refuse class < 5/10 (the training didn't take).
  Either → the adapter is shelved and the result recorded
  (withdrawn-claims style). A third turn is a NEW pre-registered
  experiment with a regenerated corpus.
- **Intermediate** (neither success nor kill): recorded honestly with
  anatomy (reads per run; which refuse-scoring leg failed; patch-class
  vs refuse-class split); the model keeps whatever G4 grants it.
- **The point estimate decides.** No extension, no re-run, no corpus
  change after seeing a number.

## Corpus identity (generated and guarded before this commit)

- Factory: `tools/flywheel/` at branch commit `b036edb` (gate-aware
  rejection sampling + template entropy + goal-phrasing skeleton
  diversity — task-6a arc, reviewed); rendering and landing/refusal
  verification through `flywheel-tool` (the serving code; anti-drift
  pins in `flywheel_tool_test.rs`, golden regression pin for the turn-1
  patch trajectory).
- Seed **20260817**, requested 1000 patch + 300 refusal → **1,299
  tasks** (1 dedup drop, a refusal task), **3,598 pairs** (749 python /
  550 plaintext tasks), 21 template families (13 patch + 8 refusal:
  4 defect-absent, 4 missing-target). Corpus SHA-256
  `d72fdb1c2467e64424b82f15e83933b76522304598a067c2b7b99a57209e4c62`;
  reproducible byte-identically from (code, seed, gates) — the JSONL
  itself is not committed.
- **Gate-aware rejection sampling** (new this turn): every candidate was
  screened at draw time against the UNION of both gate sets using the
  guard's own rules; **268 rejections** (70 goal_near_duplicate, 15
  search_match, 183 target_filename_match), all redrawn from the same
  seeded stream — counts and both gate files' SHA-256 are in the
  committed fingerprint (`…-fingerprint.json`).
- **Contamination guard: clean** — 1,299 tasks vs **40 gate fixtures
  across BOTH sets** (`codec-tasks-v1` + `codec-tasks-v2-mixed`), zero
  exact/normalized overlaps (goals, file contents, target filenames,
  search strings) and no goal ≥0.8 Jaccard vs any gate goal. Report
  committed beside this doc (`…-contamination-report.json`).
- Validation split: 65 task ids (5%; 52 patch + 13 refusal) in the
  fingerprint — loss monitoring only, never the gate.

## Training (pinned; identical hyperparameters to turn 1)

Base `Qwen/Qwen3-14B` (HF bf16); unsloth 4-bit load; LoRA r=16 α=32 on
q/k/v/o/gate/up/down projections; seq 4096; **completion-only loss**
(prompt masked); **raw text, NO chat template, NO EOS appended** — each
completion ends exactly at `</action>`; 2 epochs over ~3,416 train pairs
(≈850 optimizer steps at bs 1 × accum 8); lr 2e-4 cosine, warmup 20.
Environment freeze recorded with the training evidence.

## Honest possibilities, pre-registered

Over-refusal (caught by the G4 leg); refusal keyed on surface cues
rather than file-checking (the gate's differently-authored fixtures are
the net, as in turn 1); bluffed refusals on real defects (shows as
repair-class misses on G5); catastrophic forgetting visible in the tuned
model's own assay POST profile (the boot probe runs regardless and its
profile is part of the evidence).
