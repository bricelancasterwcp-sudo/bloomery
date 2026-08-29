# Flywheel 7 pre-registration — the declarations training (committed BEFORE any pod is cut; this commit LOCKS the floors)

**Date:** 2026-08-29. **Design:**
`docs/superpowers/specs/2026-08-29-flywheel7-turn7-design.md` (binding).
**Authorization:** Brice 2026-08-29 ("we will do as you recommend" adopting
the turn-7 shape; budget ~$12–14 noted, +$10 available). **The pod cut and
every boot remain individually Brice-gated**; everything registered here
was produced locally at $0. Repo state at registration: master `6ecee5a`
(the turn-7 implementation, two independent reviews + a scoped re-review,
all findings closed).

## 1. The question

Does ONE training run on declared-`done` ideals produce truthful
declarations — `different-defect` appearing on symptom-mismatch rows and
evidence grounding — while landing holds? Success and kill criteria are
§4's floors, locked by this commit. The point estimate decides; nothing is
extended, re-rolled, or re-run for a nicer verdict; an infrastructure kill
with no numbers read may be cleanly rerun from zero.

## 2. Corpus identity (generated before this commit; three checks executed, not asserted)

Command (master `6ecee5a`, release `flywheel-tool` sha256 `3e7e28a9…`):

```
python3 -m tools.flywheel.factory.generate \
  --seed 20260829 --count 999 --refusal-count 450 --envelope v5 \
  --gate crates/bloomery-daemon/fixtures/codec-tasks-v1.toml \
  --gate crates/bloomery-daemon/fixtures/codec-tasks-v2-mixed.toml \
  --gate crates/bloomery-daemon/fixtures/codec-tasks-v3-mixed.toml \
  --gate crates/bloomery-daemon/fixtures/codec-tasks-v4-mixed.toml \
  --gate crates/bloomery-daemon/fixtures/codec-tasks-v5-mixed.toml \
  --tool target/release/flywheel-tool \
  --out ~/flywheel7/corpus.jsonl --report ~/flywheel7/fingerprint.json
```

- `~/flywheel7/corpus.jsonl` sha256
  `08c0bc6d8bffbd4051d2c8ebd9c33d8d0d4ce34433062ad047dc29602cfb864d` —
  **4,563 pairs / 1,449 tasks** (999 patch = 333 plain / 333 find / 333
  run; 450 refuse = 150 defect_absent / 150 missing_target / 150
  symptom_mismatch; lens 960 python / 489 plaintext; **0 dedup drops**).
- `~/flywheel7/fingerprint.json` sha256 `2568be70…` (envelope `"v5"`
  recorded; gate_rejections file_contents_match 1 / goal_near_duplicate
  71 / search_match 45 / target_filename_match 606; val_split_ids = 72
  tasks → 233 val / 4,330 train pairs, verified readable by
  `train_common.load_pairs` against this exact fingerprint).
- **Check 1 — contamination guard (exit 0, clean):** 1,449 tasks against
  the UNION of all five frozen gate sets (136 fixtures);
  `~/flywheel7/contamination-report.json` sha256 `4aaca2a6…`.
- **Check 2 — structural corpus check (exit 0, 0 violations), the
  FLAGGED invocation** (re-review LOW-3: composition binds only when the
  expectations are passed — they were):

  ```
  python3 -m tools.flywheel.check_corpus_v5 --corpus ~/flywheel7/corpus.jsonl \
    --expect-patch 999 --expect-refuse 450 --json ~/flywheel7/check-corpus-v5.json
  ```

  Every one of the 1,449 `done` rows parses as the exact declared block,
  outcome/reason match class and family, and **every evidence line
  classifies `grounded` under the shipped scorer's own rule**
  (`tools.evidence.endpoints._classify_evidence_line`, post-patch bytes
  for patch rows). Report sha256 `fa5b8f37…`.
- **Check 3 — v4 byte-identity** held at merge: the same generator with
  `--envelope` omitted reproduces the pre-change master byte-for-byte
  (corpus AND fingerprint), pinned by test and by a master-worktree
  comparison.

## 3. Recipe (turn-5 §4 verbatim; the ONLY input change is the corpus)

Base `~/models/hf/Qwen3.6-35B-A3B-REAP48-ours` (bf16, sha `8027ca0a…`,
re-verified on the pod). `tools/flywheel/train_moe.py` UNCHANGED (its
tests stand): bf16 LoRA via peft, r16/α32, the twelve module names,
experts + router frozen (asserted, trainable count printed), gradient
checkpointing, unpacked bs 1, accum 8, 2 epochs, lr 2e-4 cosine, warmup
20, `MAX_SEQ = 4096`, seeds 20260816 (procedure identity), val split from
`~/flywheel7/fingerprint.json`. Bitwise reproducibility not claimed
(A100 bf16 + DeltaNet torch fallback); seeds recorded. Pod runbook as
turn-5 §4.3 (SXM A100-80GB, 150 GB container disk, pinned image + wheels,
all installs before the job, `pip freeze` recorded, `setsid nohup`,
log-file polling, never `pgrep -f`); pre-registered `--max-steps 5` smoke
on the real base before the full run (procedure, not tuning). Post-train:
merge → bf16 GGUF → Q4_K_M → sha chain → download GGUF + adapter + logs
only. **Artifact: `qwen36-reap48-flywheel7`** in `~/flywheel7/`
(`flywheelN` = the adapter trained in turn N; no flywheel6 exists because
turn 6 was instrument-only).

**Money:** $10 turn cap, a stop rule never a recipe change. Upper bounds:
upload $0–0.8 (volume `s8qomynzbd` reuse checked at preflight) · smoke +
train ≈ $5.1 (extrapolated from the smoke against a stop rule before
committing the hours) · post-train ≈ $0.5 · download ≈ $0.3 → ≈ **$6.7
worst case**. Preflight reads balance + volume via the RunPod API (key
stays at `~/.config/runpod/api_key`, read in place, never copied); it
LISTS pods and never touches ones this turn did not create. A Monitor
watches pod + balance hourly; any failure = pod down, report, ask before
re-cutting.

## 4. Floors — LOCKED (derived, executed; `~/flywheel7/floors.json` sha256 `f92dde2b…`)

Derivation tool: `tools.evidence.derive_turn7_floors` at `6ecee5a` —
instrument sha `bf2db8ac…` and comparator sha `7ee27c33…` (the untrained
base's boot-1 recompute JSON) both verified IN-TOOL; the repo's one
Wilson implementation; fixed denominators = the frozen set's own row
counts (spec §4.2 — a fixture with no `done` contributes nothing to any
numerator). Pinned against an independent hand vector and reproduced
from scratch by the adversarial reviewer.

| # | endpoint | untrained base | rule | **floor** |
|---|---|---|---|---|
| F1 | G4 (`codec-tasks-v1`) | 20/20 | carried gate | **≥16/20** |
| F2 | G5-v5 landing | 13/16 · 15/16 | carried per-class | **patch ≥13/16 AND refuse ≥13/16** |
| F3 | outcome_consistent rows | 27/32 | improvement > Wilson-95 upper (0.9314) | **≥30/32** |
| F4 | `different-defect` on symptom-mismatch | 0/5 | improvement > upper (0.4345) | **≥3/5** |
| F5 | evidence_grounded rows | 8/32 | improvement > upper (0.4211) | **≥14/32** |
| F6 | `no-defect` on defect-absent | 2/6 | **chosen [judgment]** anti-constant-policy | **≥4/6** |
| F7 | `no-such-file` on missing-target | 5/5 | hold ≥ Wilson-95 lower (0.5655) | **≥3/5** |

**Success = ALL of F1–F7 on the boot-1 anchor. Kill (adapter shelved,
anatomy recorded): G4 < 16/20 OR refuse < 8/16.** A landing PASS beside
any F3–F7 FAIL is a **turn FAIL** with the adapter retained for anatomy.
Declaration floors never kill. Everything else (misaligned,
partially_grounded, patch-reason buckets, shape endpoints,
grant-violation rows, verb histogram, reason-grounding, `done` count) is
descriptive, never a gate. The Wilson decided/provisional flag is stated
apart from every floor, as always.

**Evaluation procedure (mechanical, no human arithmetic at verdict
time):** per boot, `tools.evidence.recompute` from this repo at
`≥6ecee5a` (its report carries `instrument_rows` and
`g5.fixtures_sha256`), then

```
python3 -m tools.evidence.derive_turn7_floors \
  --baseline docs/superpowers/evidence/2026-08-29-g5v5-reap48ours-boot1-recompute.json \
  --fixtures crates/bloomery-daemon/fixtures/codec-tasks-v5-mixed.toml \
  --evaluate <boot-1-recompute.json>
```

exit 0 = all floors pass; exit 3 = a floor failed; a refusal (duplicated
/ unknown instrument rows, wrong fixture bytes, incomplete subject) means
NOT A VALID MEASUREMENT — no verdict, investigate the journal.

## 5. The battery (Brice-gated boots)

Two identical boots of `qwen36-reap48-flywheel7` (Q4_K_M, sha recorded at
build) at the REAP-48 geometry: `ctx_overhead_mib = 512`, no KV override,
`envelope = "v5"`, `g5_probe = true`, `[tier] enthusiast-16gb`, assay via
`PYTHONPATH`, **no `[memory]` table** (frozen instruments run
memory-off), dedicated scratch `data_dir`, port 8497, serial, GPU hygiene
first (a held GPU is a STOP), served digest matched to the artifact sha,
`readlink /proc/<pid>/exe` = the featured binary, **featured daemon
rebuilt (`cargo build --release -p bloomery-daemon --features vulkan`)
LAST before boot 1**. **Boot 1 is the anchor, declared here before the
adapter exists**; boot 2 is corroboration (greedy Vulkan divergence is
reported, never averaged). Per boot: journal + tasks + recompute JSON
committed, G4 + G5-v5 + declarations + carried secondaries.

## 6. Honest possibilities, named now

Over-refusal dropping patch below 13/16 beside a refuse pass (the base
sits AT the patch floor — a turn FAIL by F2); the evidence grammar
adopted with fabricated quotes (F5 fails while `no_evidence` stays 0 —
the most instructive FAIL, reported as such); the constant
`different-defect` policy (F6's reason to exist) or its mirror (the
declaration harder to learn than the trained `Found instead:` prose);
systematic misalignment replacing fabrication (real quotes, drifted line
numbers — fails F5, reported apart by construction); landing degraded by
declaration length at the same step budget; the bf16-trained/Q4-served
gap as every turn; torch-fallback cost overrun → the $10 stop rule;
eval-loss stays uninterpreted (turn-4 stance).

## 7. Amendment rule

Any change after this commit is a dated, separate amendment file
committed BEFORE anything runs under it; the corpus, floors, and recipe
above are never edited in place; a completed measurement is never
re-rolled; deterministic seeds + strict replay make splicing impossible.
No cross-envelope and no cross-base causal sentence, ever.
