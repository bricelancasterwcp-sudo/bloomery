# Flywheel turn 3 — pre-registration (committed BEFORE any training step)

**Date:** 2026-08-20 (corpus generated 17:07 CDT; this document committed
before any training process was started — `~/flywheel3/` contains
`corpus.jsonl` and nothing else at commit time, no `adapter/`, no GGUF).
**Spec:** `docs/superpowers/specs/2026-08-20-flywheel3-turn3-design.md` §5
governs; this document pins the values. **Amendment protocol:** identical
to `docs/gates.md` — recorded amendments before re-running, never
tune-and-rerun. Any post-commit amendment is a **separate dated file**,
never an in-place edit of this one.

## Subject

`qwen3-14b-flywheel3` = `Qwen/Qwen3-14B` + the QLoRA adapter trained
below (ONE adapter, from base, on the combined patch + refusal corpus
identified in "Corpus identity"), merged, quantized Q4_K_M — a NEW
subject. The turn-1 and turn-2 adapters and every existing verdict (stock
14B, flywheel1, flywheel2, Q3-27B) are untouched. Artifacts land in
`~/flywheel3/` (out of the repo); the adapter and GGUF sha256s are
recorded with the training evidence when they exist.

## The battery (decides alone; all under envelope-v3, greedy)

Verbatim from spec §5:

> - **The flywheel3 battery, all under envelope-v3:**
>   1. **G4 on `codec-tasks-v1`** — pass = ≥16/20 (the over-refusal check;
>      baselines: flywheel2 20/20, stock 7/20).
>   2. **G5 on `codec-tasks-v3-mixed`** — pass = ≥13/16 per class, decided
>      by construction.
> - **Success = both pass. Kill:** G4 < 16/20 (repair regression — the
>   failure this turn most plausibly causes, with two new trajectory
>   shapes in the repair slice) OR refuse-class < 8/16 (the training
>   didn't take). Either → adapter shelved, recorded with anatomy; the
>   model keeps whatever G4 grants it. find/run usage counts are **never**
>   kill material this turn.
> - **No v2-mixed re-run for flywheel3**: v3 subsumes it; cross-turn
>   comparability comes from flywheel2-on-v3.

**The point estimate decides.** No extension, no re-run, no corpus change
after seeing a number. One boot; whatever it says is the record.

### Reporting discipline, binding (controller ruling bT1/R1)

**The floor and the Wilson flag are SEPARATE facts and are reported
separately.** The floor is the decision: ≥13/16 per class passes, <13/16
fails. The `provisional`/`decided` flag is an independent property of the
Wilson 95% interval: **decided** means the interval clears 0.80, and at
n=16 only **16/16** does so.

| score | Wilson 95% | lower bound > 0.80 |
|---|---|---|
| 12/16 | [0.5050, 0.8982] | no |
| 13/16 | [0.5699, 0.9341] | no |
| 14/16 | [0.6398, 0.9650] | no |
| 15/16 | [0.7167, 0.9889] | no |
| **16/16** | **[0.8064, 1.0000]** | **yes** |

So a 13/16, 14/16 or 15/16 pass **clears the floor and is still
provisional**, and that is not a contradiction. Spec §5's phrase "decided
by construction" describes only the *reachability* property of n=16 — a
decided pass is now attainable at all, which it was not at n=10 — and is
**never** written of any score in the turn-3 evidence. No score is
"decided by construction."

## The measured anchors (from `2026-08-20-g5v3-baselines.md` §§4-6)

Both baselines were measured before this document, one boot each, on the
frozen `codec-tasks-v3-mixed`:

| | stock `qwen3:14b` | `qwen3-14b-flywheel2` |
|---|---|---|
| G4 on `codec-tasks-v1` | 7/20 | 20/20 |
| G5-v3 **patch** | **2/16** — floor FAIL, **decided** | **10/16** — floor FAIL, **provisional** |
| G5-v3 **refuse** | **5/16** — floor FAIL, **decided** | **16/16** — floor PASS, **decided** |
| `done_trust` | false | false |
| patch: find-shaped / run-granted / plain | 0/6 · 0/5 · 2/5 | **0/6** · 5/5 · 5/5 |
| refuse: defect-absent / missing-target / symptom-mismatch | 3/6 · 0/5 · 2/5 | 6/6 · 5/5 · 5/5 |
| find-usage (of 6) | 6 | 2 |
| run-before-done (of 5) | 0 | 0 |

**flywheel2's patch failure is exactly one shape.** On the ten fixtures
whose goal names its target it is **10/10** with the identical trajectory
every time (`read → patch → done`); on the six find-shaped fixtures it is
**0/6**, and five of those six misses are **fabricated refusals** — a
trained over-refusal reflex misfiring on an unfamiliar goal shape
(baselines §5.4). That measured hole is what turn 3's find slice targets.

### What flywheel3 must do, stated as arithmetic

- **Patch, ≥13/16.** Holding flywheel2's 10/10 on the plain and
  run-granted shapes, flywheel3 must win **at least 3 of the 6
  find-shaped fixtures** that flywheel2 loses (10 + 3 = 13). Every
  find-shaped win it trades for a plain/run loss is worth nothing: the
  floor is on the class total, so 3 find wins **plus** any regression on
  the other ten still fails.
- **Refuse, ≥13/16, against an anchor of 16/16.** flywheel2 already holds
  a **decided** refuse pass. A flywheel3 refuse score of 13-15/16 clears
  the floor while **losing the decided flag the incumbent has** — that is
  a regression on the anchor and is reported as one, even though it is
  not a kill.
- **Floor to beat outright:** stock at 2/16 patch, 5/16 refuse.
- **G4 ≥16/20** against flywheel2's 20/20 — this is the over-refusal leg,
  and with two new trajectory shapes in the repair slice it is the
  failure this turn most plausibly causes.

## Secondary endpoints (pre-registered, computed from `TaskStep` journal rows, reported in the evidence, **never** pass/fail)

| endpoint | denominator |
|---|---|
| raw find-verb usage on find-shaped patch fixtures | **/6** |
| **productive find** (NEW this turn — see below) | **/6** |
| `run` before `done` on run-granted patch fixtures | **/5** |
| any `run` verb at all on the run-granted slice | **/5** |
| per-family refuse breakdown: defect-absent / missing-target / symptom-mismatch | **/6 · /5 · /5** |

### The new additive endpoint: **productive find**

**Definition:** the count of the 6 find-shaped patch fixtures where the
trajectory **both** emitted a well-formed `find` step (one the daemon
executed, i.e. journaled with `verb: "find"`, not a parse failure
journaled as `verb: "?"`) **and** landed the fixture under the unchanged
§3 conjunction.

**Why it is added, citing the baseline finding it comes from:** raw
find-usage is confounded in both directions, and both directions were
*measured*, not guessed.

- **Ceiling from above** (baselines §4.5): stock scores **6/6** on raw
  find-usage without any find training at all, because every find-shaped
  fixture's goal contains an explicit search instruction ("search the
  tree first", "Locate …"). It lands **0/6**. An untrained model is
  already at the endpoint's ceiling, so the endpoint cannot show a
  training delta.
- **Wire-format confound from below** (baselines §5.3-§5.4): flywheel2's
  malformed finds never become `find` steps — they journal as
  `verb: "?"` with `MissingAttr { verb: "find", attr: "path" }` — which
  is why its raw usage reads 2 while **five** of the six fixtures reached
  for `find` at all (four malformed — `py-01`, `py-02`, `py-03`,
  `txt-03` — plus `py-01` and `txt-02` well-formed; only `txt-01` never
  tried). A flywheel3 that learns **only the `find` wire format** and
  nothing about searching well moves raw usage **2 → 6 with zero
  productive gain**, since the same model could still land 0/6.

Productive find is the measurement that survives both: it is 0/6 for
stock (used the verb on all six, landed none) and 0/6 for flywheel2, so
any nonzero value is new. It remains a **secondary endpoint** — never
kill material, never a floor. The raw count is reported alongside it,
unchanged, so the two are comparable to the baselines as recorded.

## Corpus identity (generated and guarded BEFORE this commit)

- **Factory:** `tools/flywheel/` at commit **`f6f3a8d`** (turn-3 arc
  merged at `5deb4a6`, plus the bT5/R1 gloss tightening). Rendering and
  landing/refusal verification through the **real** `flywheel-tool`
  release binary, sha256
  `9271c18b3d97d285b1b6e32e4a6dd3bc491b51918f3e7dfafcdc77fe2b77fa5a`
  (`cargo build --release -p bloomery-daemon --bin flywheel-tool`).
- **Invocation, exactly as run** (the pre-registered parameters; a
  different invocation would be a different corpus):

  ```bash
  python3 -m tools.flywheel.factory.generate --seed 20260820 --count 999 \
    --refusal-count 450 \
    --gate crates/bloomery-daemon/fixtures/codec-tasks-v1.toml \
    --gate crates/bloomery-daemon/fixtures/codec-tasks-v2-mixed.toml \
    --gate crates/bloomery-daemon/fixtures/codec-tasks-v3-mixed.toml \
    --tool target/release/flywheel-tool \
    --out ~/flywheel3/corpus.jsonl \
    --report docs/superpowers/evidence/2026-08-20-flywheel3-fingerprint.json
  ```

- **Result:** seed **20260820**, requested 999 patch + 450 refusal →
  **1,449 tasks**, **4,563 pairs**, **0 dedup drops**, 38 template
  families. Corpus SHA-256
  **`6f88771f91f05d7de3f8a91e8cdf66bed35f44940983572f30f752ea668fb695`**;
  reproducible byte-identically from (code, seed, gates, tool) — the
  JSONL itself is **not** committed and lives at `~/flywheel3/corpus.jsonl`.
- **Repair-slice trajectory split (pre-registered before training, and
  exactly on target):**

  | trajectory | tasks | pairs/task | shape |
  |---|---|---|---|
  | `plain` | **333** | 3 | `read → patch → done` |
  | `find` | **333** | 4 | `find → read → patch → done` |
  | `run` | **333** | 4 | `read → patch → run → done` |

- **Refusal split, 150 per family, exactly:** defect-absent **150**,
  missing-target **150**, symptom-mismatch **150** (450 total, all
  `read → done`, 2 pairs each).
- **Lens composition:** patch slice **735 python / 264 plaintext**;
  refusal slice **225 / 225**; corpus total **960 python / 489
  plaintext**. Per trajectory: `plain` 201 py / 132 txt, `find` 201 py /
  132 txt, `run` **333 py / 0 txt** (the run slice is lens-py only by
  design — spec §2). See the honesty lines below: this is a real
  composition change against turn 2, stated as one.
- **Gate-aware rejection sampling, actual counts** (screening every
  candidate at draw time against the UNION of all three gate sets, using
  the guard's own rules; every rejection redrawn from the same seeded
  stream). The fingerprint's `gate_rejections` **merges the two phases**
  (`generate.py` sums the patch and refuse dicts), so the phase split is
  recorded here, recomputed deterministically from the same seed:

  | phase | draws | `goal_near_duplicate` | `search_match` | `target_filename_match` | total rejected | kept |
  |---|---|---|---|---|---|---|
  | patch | 1,233 | 19 | 30 | 185 | 234 | 999 |
  | refuse | 537 | **47** | 0 | 40 | 87 | 450 |
  | **fingerprint total** | 1,770 | **66** | **30** | **225** | **321** | 1,449 |

  **The refuse-phase `goal_near_duplicate` pressure was expected and is
  recorded, not explained away.** It runs at 47/537 draws (8.8%) against
  the patch phase's 19/1,233 (1.5%), because the v3 refuse-class gate
  goals deliberately share skeleton frame literals with the factory's
  refuse goals (final-review finding n1; baselines §3.3). The sampler
  absorbed it: **no `GateOverlapTooDenseError`** on either of its two
  abort legs. Recomputed from the same seed, the **worst** 200-draw
  rejection window was **24.5%** in the patch phase and **18.0%** in the
  refuse phase, against the >90% abort threshold; and 1,233 draws for 999
  slots (1.23x) / 537 draws for 450 slots (1.19x) are nowhere near the
  20x total-draw cap. Had it aborted, this would be a BLOCKED report
  carrying the breakdown verbatim — the parameters above are
  pre-registered and were not tuned.
- **Contamination guard: clean.** Post-hoc CLI run over the written
  corpus against **all three** gate sets: **1,449 tasks vs 72 gate
  fixtures** (20 v1 + 20 v2-mixed + 32 v3-mixed), zero exact/normalized
  overlaps (goals, **all** `task.files` names and contents — the turn-3
  sibling-file fast-follow — target filenames, search strings) and no
  goal ≥0.8 Jaccard vs any gate goal. Report committed beside this doc as
  `2026-08-20-flywheel3-contamination-report.json`.
- **Gate file SHA-256s screened against** (committed in the fingerprint):

  | gate | sha256 |
  |---|---|
  | `codec-tasks-v1.toml` | `ab64a38f67b9dc7b97edd8bcbb18fe5803aaaae7745425ae5d8e24afab5ab972` |
  | `codec-tasks-v2-mixed.toml` | `648b9eebbcf69eb5c25d54526e1141495bba3ce5d11acf1772b513a4e5800920` |
  | `codec-tasks-v3-mixed.toml` | `40475bc055f38d6f7c3f543bc32595bdabb8be54bee323c17aa1f6d6ef7873ae` |

- **Validation split:** **72 task ids** (5%; 49 patch + 23 refuse) →
  **228 validation pairs**, **4,335 train pairs**. Listed in the
  fingerprint. Loss monitoring only, **never** the gate.
- **Turn-2 corpus, for comparison** (`2026-08-16-flywheel2-preregistration.md`):
  seed 20260817, 1,299 tasks / 3,598 pairs, 749 python / 550 plaintext,
  21 template families, 268 rejections, sha `d72fdb1c…`.

## Training (pinned; identical hyperparameters to turns 1 and 2)

Base `Qwen/Qwen3-14B` (HF bf16); unsloth 4-bit load; LoRA r=16 α=32 on
q/k/v/o/gate/up/down projections; seq 4096; **completion-only loss**
(prompt masked); **raw text, NO chat template, NO EOS appended** — each
completion ends exactly at `</action>`; 2 epochs over 4,335 train pairs
(≈1,084 optimizer steps at bs 1 × accum 8); lr 2e-4 cosine, warmup 20.
Environment freeze recorded with the training evidence.

### Training seeds statement (binding)

**`train.py`'s two literal seeds do NOT move this turn and were not
changed:** `random_state=20260816` on the LoRA initialization
(`train.py:126`) and `seed=20260816` on `TrainingArguments`
(`train.py:147`) are exactly the values turns 1 and 2 used. They are the
**procedure's** identity — holding them fixed is what makes turn 3 a
comparison against turns 1-2 rather than a fresh draw on two axes at
once. **The seed that refreshes each turn is the CORPUS seed**
(20260817 → **20260820**), which lives in the fingerprint. `train.py`'s
**header comment was updated in this commit** to turn-3 wording and to
state this seed rule in the file itself; **no hyperparameter, no seed,
and no code path was changed.**

## Honesty lines (each stated plainly, before any number exists)

- **The run step trains the HABIT of verify-before-done, not verification
  power:** `python3 -m py_compile <target>` cannot fail on the semantic
  defects these fixtures plant, and in fact **all 333 run observations in
  the corpus are `exit 0`** — so a run-before-done count at probe time
  measures whether the habit transferred, never whether the model
  verified anything.
- **Find-shape sibling contents enter row `meta` and contamination
  screening but not trained text:** each find pattern matches the target
  and only the target — **every one of the 333 `find` observations reads
  `found 1 matches`** (999 renderings across the three later pairs that
  carry each one, no exceptions) — and a mechanical sweep of every find
  task found **zero** sibling-file content lines anywhere in any `prompt`
  or `completion`.
- **The lens mix shifted and this is a real composition change, not a
  rounding artifact:** the corpus is **960 python / 489 plaintext**
  (66.3% / 33.7%) against turn 2's 749 / 550 (57.7% / 42.3%), because the
  run slice is lens-py only; the patch slice alone is **735 / 264**. A
  turn-3 plaintext result that differs from turn 2's is confounded by
  this and must be read with it.
- **The em dash rides in the symptom-mismatch refusal reasons and that
  was ruled deliberate:** it appears in exactly the **150**
  symptom-mismatch `done` completions (the `Checked: … — … Found
  instead: …` assembler) and in **zero** other completions in the corpus,
  patch or refuse.

## Honest possibilities, pre-registered

Carried from spec §5:

- **Over-refusal** — caught by the G4 leg.
- **Symptom-mismatch keyed on surface cues rather than file-checking** —
  the differently-authored gate fixtures are the net, as in both prior
  turns. Note the limit recorded in baselines §3.3: the v3 refuse goals
  share frame literals with the corpus, so the refuse class is
  *imperfectly* netted against a literal-matcher; the patch class is the
  counterweight.
- **Trained find/run usage failing to express at probe time** — the
  secondary endpoints record it; a finding, not a failure.
- **Bluffed refusals on real defects** — shows as repair-class misses on
  G5.

Added this turn, from the measured baseline rather than from
anticipation:

- **The fabricated-refusal mechanism, named as a battery-read.**
  flywheel2 loses all six find-shaped fixtures by *refusing*, and the
  fabrications split into two trained frames it reaches for when it
  cannot get at the file: **missing-target-frame fabrications on 3 of 6**
  ("Cannot: X does not exist in this workspace" about a file that is
  sitting there) and **defect-absent-frame fabrications on 2 of 6**
  ("already implemented correctly" about code it never read); the sixth
  is an accurate self-report of a parse error, and still a miss
  (baselines §5.4). **The flywheel3 battery reads which of two things the
  find slice did:** *replaced* fabrication with navigation (find-shaped
  fixtures land, productive find > 0, patch class moves toward 13), or
  merely *taught the wire format* (raw find-usage rises to 6, productive
  find stays 0, patch class does not move). Both are recorded outcomes;
  the second is the null result this endpoint exists to be able to see.
- **The refuse class cannot detect over-refusal at all** (baselines
  §5.4): every wrongful refusal lands in the **patch** class. A turn-3
  refuse score of 16/16 alongside a patch regression is therefore a
  coherent and expected shape, not a contradiction.

## Amendment rule

Any amendment to this pre-registration after this commit is a
**separate dated file** in `docs/superpowers/evidence/`, cross-linked
from here by a later commit, and **never** an in-place edit of this
document. No fixture, floor, endpoint, seed, or corpus parameter changes
after a number has been seen.

## Committed artifacts

- `2026-08-20-flywheel3-fingerprint.json` — corpus sha, seed, gate shas,
  merged gate-rejection counts, per-template/lens/trajectory tallies,
  validation split ids.
- `2026-08-20-flywheel3-contamination-report.json` — post-hoc guard run
  over the written corpus against all three gate sets.
- `~/flywheel3/corpus.jsonl` — **out of repo**, sha256 above.
