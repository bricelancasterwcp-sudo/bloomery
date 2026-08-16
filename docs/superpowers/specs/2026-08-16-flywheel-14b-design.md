# The fine-tune flywheel, turn 1 — qwen3:14b learns read-before-patch

**Date:** 2026-08-16
**Status:** Approved in conversation (design presented, Brice approved).
**Lineage:** the recorded G4 escalation (umbrella §8; black-oxide SPEC
§32.4's flywheel concept). The black-oxide data-factory spec
(`~/workspace/oxide/docs/superpowers/specs/2026-08-11-finetune-data-factory-design.md`)
contributes the governing lesson: **train/eval contamination voided by
design** — the corpus is new tasks; the gate set is held out and a
machine check proves it.

## 1. What this builds and why

Under envelope-v3 every envelope disease is gone and the models'
failures are purely behavioral: qwen3.8-27b-Q3 reads-then-patches and
passed 20/20; **qwen3:14b patches from imagination** (2 reads in 76
steps, 51 `SearchNotFound`, only guessable plaintext lands → 7/20,
evidence `2026-08-16-g4-capability-14b-v3.md`). The 14B is fast and
fully offloaded — if it learns the read-first habit, a small model earns
mutating verbs, which is the local-AI mission's real prize.

Turn 1 of the flywheel: generate a verified training corpus, QLoRA-tune
the 14B on ideal trajectories, convert, and let **the existing G4 gate,
unamended, decide**.

## 2. The load-bearing principle: training artifacts run through the serving code

Two failure classes this spec refuses up front:

- **Prompt drift**: if the training prompts are rendered by a Python
  re-implementation of the envelope, the trained distribution silently
  diverges from what serving renders. Therefore corpus prompts are
  produced by the REAL `render_prompt` + `verb_card_for` + think-preseed
  + v3 semantics, exposed through a tiny workspace binary.
- **Verifier drift**: a Python re-implementation of the applier would
  bless patches the real `land()` rejects. Therefore every training
  patch is verified through the REAL applier, same binary.

**`flywheel-tool`** — a small Rust bin target in the bloomery workspace
(`crates/bloomery-daemon/src/bin/flywheel_tool.rs` or a dedicated tools
crate; the plan decides placement) with two subcommands, JSON on
stdin/stdout:

- `render`: given `{goal, patch_codec, envelope, transcript}` → the
  exact prompt string serving would produce at that step (v3 = preseed
  included; the stop is a generation-time property and does not alter
  the prompt).
- `land`: given `{file_contents, search, replace, lens}` → the real
  `Landing` outcome (reusing `bloomery_core::action::lens::land` and the
  daemon's Python/plaintext lenses).

No second implementation of either, anywhere in the factory.

## 3. The task factory (`tools/flywheel/`, Python, in this repo)

- Parametric templates generate **~1,000 NEW single-defect tasks**
  (~600 `python` lens, ~400 `plaintext`), codec-tasks-v1 SHAPE: 1–2
  files, 5–60 line targets, one planted contiguous defect, a goal that
  states the symptom, names the target, ends with the patch-then-done
  instruction. Template axes: domain vocabulary, defect class (wrong
  operator/index/constant/boolean/name, config value mismatches, prose
  facts), indentation depth, distractor files.
- Because the defect is planted, the **ideal trajectory is derived
  mechanically**: `read(target)` → `patch` with the exact search/replace
  (search copied byte-for-byte from the generated file) → `done` with a
  one-line factual summary. Every patch is `land`-verified through
  `flywheel-tool`; a task whose reference fails to land is a factory bug
  and aborts generation (never silently dropped).
- **Each task yields 3 SFT pairs** — (rendered v3 prompt at step k,
  ideal turn k) for k = read, patch, done — with the transcript at each
  step containing the real prior observations (the read observation =
  what the executor would return: the file contents; the plan pins the
  exact observation format against `exec_read`'s real output).
- Dedup on normalized content; ~5% held-back validation split (loss
  monitoring only — never the gate).
- **Contamination guard (machine-checked, output committed):** a
  structural comparator proves zero overlap between the corpus and
  `codec-tasks-v1` — no shared goals, file contents, target filenames,
  search strings, or normalized near-duplicates thereof. Runs before
  training; its output is part of the pre-registration record.
- Corpus fingerprint committed before training: task count per template
  and lens, dedup stats, pair count, SHA-256 of the corpus JSONL.

## 4. Training

- Base: `Qwen/Qwen3-14B` (HF bf16, ~30 GB download to `/` — 327 GB
  free). Method: **QLoRA via unsloth** (consumer-standard on a 16 GB
  card): 4-bit base, LoRA r=16 α=32 on attention+MLP projections,
  seq len 4096, **completion-only loss** (the prompt is masked; the
  model learns to emit the turn), 2–3 epochs over ~3k pairs, lr 2e-4
  cosine, bs 1 × grad-accum ~8. Estimated 1–2 h on the RTX 5080 —
  OS-detached with pid file + completion marker (the >2 h discipline
  applies at projection time).
- Environment: a dedicated venv (`~/flywheel-venv`); pinned package
  versions recorded in the evidence. GPU hygiene rules apply (no
  competing tenants; never kill in-flight runs).
- Training telemetry (loss curves) recorded; validation loss is
  informational only.

## 5. Convert, serve, re-gate

- Merge adapter → bf16 → convert with llama.cpp's `convert_hf_to_gguf`
  → quantize **Q4_K_M** (~9 GB, full offload). llama.cpp checkout/built
  tools are part of the plan's environment phase.
- Register in bloomery as a **new subject**: model name
  `qwen3-14b-flywheel1` (model × quant × adapter = identity; the stock
  14B's verdicts are untouched).
- **The existing G4 gate decides**: boot with `envelope = "v3"`,
  `tasks_enabled`, assay POST (its own profile — the tuned model is a
  new subject for assay too), fixture set `codec-tasks-v1` (held out
  from training by §3's guard), scoring/decision rules unamended.
  Its own evidence doc + committed journals, as every rung.

## 6. Pre-registration (committed BEFORE any training step runs)

- **Success = the existing gate**: ≥80% landing (16/20) under
  envelope-v3 on codec-tasks-v1. Nothing softer counts as success.
- **Baseline**: stock qwen3:14b under v3 = 7/20 (Wilson [0.181, 0.567]).
- **Kill criterion**: tuned < 7/20 → regression; the adapter is
  shelved, the result recorded (a withdrawn-claims-style entry), no
  tune-and-rerun — a second flywheel turn is a new pre-registered
  experiment with a regenerated corpus.
- **Intermediate outcome** (7/20 ≤ result < 16/20): recorded honestly
  with anatomy (did reads appear? did SearchNotFound fall?); informs
  turn 2; the model stays demoted.
- Honest possibilities, pre-registered: the habit trains but
  byte-exactness doesn't; template-phrasing overfit that the
  differently-authored gate set exposes; catastrophic forgetting
  visible in the assay POST profile (the boot probe runs regardless and
  its profile is part of the evidence).

## 7. Testing posture

- `flywheel-tool` subcommands: GPU-free Rust tests pinning render
  byte-equality against `render_prompt`'s real output and `land`
  pass-through (no logic of its own to test beyond IO framing).
- Factory: Python tests — template output validates against the same
  structural rules as codec-tasks-v1's validator (search
  exactly-once, target among files, goal names target); the
  contamination comparator has its own tests (a planted duplicate must
  be caught — mutation-style).
- The training/convert phases are operational, verified by their
  artifacts (loss curve, GGUF loads, POST profiles) rather than unit
  tests.

## 8. Non-goals (turn 1)

- No RL / rejection sampling / teacher distillation (factory-only, by
  decision).
- No training on the 27B or other models; no multi-turn beyond the
  3-step ideal shape; no new gate, no new fixtures for the gate.
- No serving-stack changes: bloomery is untouched except `flywheel-tool`
  (additive bin) and the tools/ Python package.
- No automatic flywheel loop — turn 2, if any, is its own decision.

## 9. Deliverable order

1. `flywheel-tool` (render + land passthrough bins) + tests.
2. Factory + corpus + contamination guard + fingerprint (committed).
3. Pre-registration doc (gates + kill criteria, §6) — before training.
4. Environment (venv, HF download, llama.cpp tools) + detached QLoRA
   run.
5. Merge → GGUF → register → **G4 re-gate** → evidence doc.
