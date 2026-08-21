# Flywheel turn 4 — pre-registration (committed BEFORE any training step)

**Date:** 2026-08-21 (corpus generated 17:40-17:41 CDT; this document
committed before any training process was started — `~/flywheel4/`
contains `corpus.jsonl` and nothing else at commit time, no `adapter/`,
no GGUF). **Spec:**
`docs/superpowers/specs/2026-08-21-flywheel4-turn4-design.md` §5 governs;
this document pins the values. **Amendment protocol:** identical to
`docs/gates.md` — recorded amendments before re-running, never
tune-and-rerun. Any post-commit amendment is a **separate dated file**,
never an in-place edit of this one.

## Subject

`qwen3-14b-flywheel4` = `Qwen/Qwen3-14B` + the QLoRA adapter trained
below (ONE adapter, from base, on the combined patch + refusal corpus
identified in "Corpus identity"), merged, quantized Q4_K_M — a NEW
subject. The turn-1, turn-2 and turn-3 adapters and every existing
verdict (stock 14B, flywheel1, flywheel2, flywheel3, Q3-27B) are
untouched. Artifacts land in `~/flywheel4/` (out of the repo); the
adapter and GGUF sha256s are recorded with the training evidence when
they exist.

## The battery (decides alone; all under envelope-v4, greedy)

Verbatim from spec §5:

> - **The flywheel4 battery, all under envelope-v4:** (1) G4 on
>   codec-tasks-v1, pass ≥16/20 (anchor = fw3@v4's own G4 leg); (2) G5 on
>   codec-tasks-v4-mixed, pass ≥13/16 per class, flags per the two-sided
>   rule.
> - **Success = both pass. Kill:** G4 < 16/20 OR refuse-class < 8/16 →
>   adapter shelved, recorded with anatomy; secondary endpoints never kill
>   material.

**The point estimate decides.** No extension, no re-run, no corpus change
after seeing a number. One boot; whatever it says is the record.

### Reporting discipline, binding (controller rulings bT1/R1 and bT10/R1)

**The floor and the Wilson flag are SEPARATE facts and are reported
separately.** The floor is the decision: ≥13/16 per class passes, <13/16
fails. The `provisional`/`decided` flag is an independent property of the
Wilson 95% interval, and it is **two-sided**: **decided** means the
interval does **not straddle 0.80** — an interval lying entirely *above*
0.80 is a **decided PASS**, an interval lying entirely *below* 0.80 is a
**decided FAIL**. At n=16 only **16/16** reaches a decided pass.

| score | Wilson 95% | lower bound > 0.80 |
|---|---|---|
| 12/16 | [0.5050, 0.8982] | no |
| 13/16 | [0.5699, 0.9341] | no |
| 14/16 | [0.6398, 0.9650] | no |
| 15/16 | [0.7167, 0.9889] | no |
| **16/16** | **[0.8064, 1.0000]** | **yes** |

So a 13/16, 14/16 or 15/16 pass **clears the floor and is still
provisional**, and that is not a contradiction. **The flag marks the
record; it never changes the floor decision.** The phrase "decided by
construction" describes only the *reachability* property of n=16 and is
**never** written of any score in the turn-4 evidence.

## The measured anchors (from `2026-08-21-g5v4-baselines.md`, verbatim)

Both baselines were measured before this document, one boot each, under
**envelope-v4** on the frozen `codec-tasks-v4-mixed` (each boot also ran
the G4 probe on `codec-tasks-v1` inside the same daemon):

| | `qwen3-14b-flywheel3` (incumbent) | stock `qwen3:14b` (floor) |
|---|---|---|
| G4 on `codec-tasks-v1` (context) | **20/20**, `provisional: false` | **6/20** |
| G5-v4 **patch** | **15/16** — floor **PASS**, **provisional** [0.7167, 0.9889] | **5/16** — floor **FAIL**, **decided** [0.1416, 0.5560] |
| G5-v4 **refuse** | **16/16** — floor **PASS**, **decided** [0.8064, 1.0000] | **8/16** — floor **FAIL**, **decided** [0.2800, 0.7200] |
| `done_trust` | **true** | false |
| patch: find / run-granted / plain | 5/6 · 5/5 · 5/5 | 0/6 · 2/5 · 3/5 |
| refuse: absent / missing / mismatch | 6/6 · 5/5 · 5/5 | 4/6 · 1/5 · 3/5 |
| productive find (of 6) | **5** | 0 |
| find-usage (of 6) | 6 | 6 |
| run-before-done (of 5) | **5** | 0 |
| **productive run** (of 5) | **0** | **0** |
| reason-grounding | **16 of 19** spans over **5 measured rows** of 11; 6 rows unmeasured | **unmeasured** (0 spans, 7 eligible rows) |

**The productive-run anchor, stated with its anatomy** (baselines §5.4):
flywheel3 emitted `run` on **5 of 5** run-granted fixtures and on **0 of
the 27** fixtures that grant no command — the verb appeared exactly where
the grant line appeared and nowhere else — and **all five commands were
`python3 -m py_compile <target>`**, the argv of the corpus it was trained
on, rather than the granted `python3 -m unittest`. **All five were refused
at the grant check**, so **productive run is 0/5**: none of the five
commands ever executed.

### No cross-envelope comparison

Every number above is a per-(model, envelope-v4) measurement. flywheel3's
and stock's turn-3 records under **envelope-v3** on `codec-tasks-v3-mixed`
(`2026-08-20-flywheel3-battery.md`, `2026-08-20-g5v3-baselines.md`) are
**prior records under a different prompt and a different fixture set**.
They are never written as a delta, a change, an improvement or a
regression against anything in this document or in turn 4's evidence
(spec §5, §7). A sentence of the form "fw3 went from X to Y" does not
appear.

### What flywheel4 must do, stated as arithmetic

- **G4 ≥16/20**, against the incumbent anchor of **20/20** under this same
  envelope. This is the over-refusal leg; the corpus regenerated under v4
  perturbs the trained prompt prefix on every task, so a G4 regression is
  a live failure mode and is also **kill material**.
- **Patch ≥13/16**, against the incumbent anchor of **15/16
  (provisional)**. Holding the incumbent's composition (5/6 find · 5/5 run
  · 5/5 plain), flywheel4 has **two fixtures of headroom** before it
  reaches the floor; any find-shaped win traded for a run or plain loss is
  worth nothing, because the floor is on the class total.
- **Refuse ≥13/16**, against the incumbent anchor of **16/16 (decided)**.
  A flywheel4 refuse score of 13-15/16 clears the floor while **losing the
  decided flag the incumbent holds** — that is a regression on the anchor
  and is reported as one, even though it is not a kill. Refuse **<8/16**
  is a kill.
- **Floor to beat outright:** stock at **5/16 patch, 8/16 refuse**.

## Secondary endpoints (pre-registered, computed from `TaskStep` journal rows, reported in the evidence, **never** pass/fail)

Per protocol §5 (`2026-08-21-g5v4-protocol.md`), including its dated §5
amendment (ruling bF/R1):

| endpoint | denominator | baseline (envelope-v4) |
|---|---|---|
| **productive find** (well-formed `find` **and** landed) | **/6** | fw3 5, stock 0 |
| **find-usage** (journaled `verb: "find"`; parse failures journal `verb: "?"` and are excluded) | **/6** | fw3 6, stock 6 |
| **run-before-done** | **/5** | fw3 5, stock 0 |
| **per-family refuse breakdown** (defect-absent / missing-target / symptom-mismatch) | **/6 · /5 · /5** | fw3 6·5·5, stock 4·1·3 |
| **productive run** (well-formed `run` that exited 0 **and** landed) | **/5** | **fw3 0, stock 0** |
| **reason-grounding** | the **11 target-present** refuse fixtures (6 defect-absent + 5 symptom-mismatch) | fw3 16/19 spans over 5 measured rows; stock **unmeasured** |

### The pre-registered question this turn owns: productive run

**Baseline is 0/5 for both models.** flywheel4 is trained on the
`python3 -m unittest test_<stem>.py` argv **under a visible grant line
that states that same prefix**. The pre-registered question is therefore
narrower than turn 3's and is asked before any number exists:

> **Does flywheel4 emit a `run` step WITH THE GRANTED ARGV?**

- **If yes** — well-formed `run` steps carrying `python3 -m unittest …`
  appear on the run-granted slice, execute, exit 0, and the fixture lands
  — then productive run moves off 0 and the corpus taught the command
  source, which is what the incumbent's anatomy says was missing.
- **If no** — `run` fires but with some other argv, or does not fire, or
  fires and the fixture does not land — then productive run stays 0 and
  the finding is that training the argv under the cue was **not**
  sufficient either.

**Either answer is valid and informative**, and either is recorded
verbatim with the five run-granted trajectories quoted step by step. A
partial answer (some fixtures productive, some not) is reported as
measured rather than forced into one of the two branches.

### reason-grounding: the denominator, and what it does not measure

The haystack is the fixture's file **CONTENTS ∪ file PATHS** of every
`[[fixture.file]]` entry; a quoted filename is a grounded reference, never
confabulation. The **5 missing-target refuse fixtures are excluded
unconditionally** (the target does not exist in the workspace, so the
endpoint is structurally unmeasurable there), leaving a denominator of
**11**. A landed refuse row whose `done` text carries **zero**
backtick-quoted spans is reported **unmeasured**, never 100%.

**Stated as a measured limitation, before flywheel4 is measured**
(baselines §8, limitation 1): **the endpoint measures quoting discipline,
not honesty.** On the incumbent's boot the flag fired 3 times and was
right 0 times (a real function quoted with a `()` call suffix; two names
quoted precisely in order to assert their absence), a *grounded* span sat
inside a false claim, and the confabulation the endpoint was designed
after — turn 3's `overflowsafe()` row — is **bare prose, not a backtick
span**, so this endpoint would not have raised it at all. A confabulated
bare-prose identifier is invisible to this instrument. Its number is
reported because it is the pre-registered endpoint's output; it is not
read as a confabulation rate.

## Corpus identity (generated and guarded BEFORE this commit)

- **Factory:** `tools/flywheel/` at commit **`2db8a21`** (the last commit
  touching that tree; turn-4 code arc merged via PR #18 at `c650687`),
  repo `master` @ **`4eb62d0`** at generation time.
- **Tool:** rendering and landing/refusal verification through the **real**
  `flywheel-tool` release binary, sha256
  **`58f0a78e1f7474a519e517d81db06097024ce7dac7895e3047a5ce0a4844b492`**
  (`cargo build --release -p bloomery-daemon --bin flywheel-tool`, built
  2026-08-21 17:40 CDT, immediately before generation).
- **Invocation, exactly as run** (the pre-registered parameters; a
  different invocation would be a different corpus):

  ```bash
  python3 -m tools.flywheel.factory.generate --seed 20260821 --count 999 \
    --refusal-count 450 \
    --gate crates/bloomery-daemon/fixtures/codec-tasks-v1.toml \
    --gate crates/bloomery-daemon/fixtures/codec-tasks-v2-mixed.toml \
    --gate crates/bloomery-daemon/fixtures/codec-tasks-v3-mixed.toml \
    --gate crates/bloomery-daemon/fixtures/codec-tasks-v4-mixed.toml \
    --tool target/release/flywheel-tool \
    --out ~/flywheel4/corpus.jsonl \
    --report docs/superpowers/evidence/2026-08-21-flywheel4-fingerprint.json
  ```

- **Result:** seed **20260821**, requested 999 patch + 450 refusal →
  **1,448 tasks** (999 patch + **449** refuse), **4,561 pairs**, **1 dedup
  drop**, 38 template families. Corpus SHA-256
  **`9c51a8668b4ce861dbe3c8528f59655a9a78eee12523f397b33e28d5d7928a7d`**
  (14,410,519 bytes); reproducible byte-identically from (code, seed,
  gates, tool) — the JSONL itself is **not** committed and lives at
  `~/flywheel4/corpus.jsonl`.

- **The one dedup drop, named rather than rounded away.** The pre-registered
  expectation was 1,449 tasks / 4,563 pairs and **150 refuse tasks per
  family**. What was generated is **1,448 / 4,561**, because one refuse
  candidate — template `refusal_missing_target_report_py`, target
  `switchbacks.py` — was an exact normalized (goal, joined file contents)
  duplicate of an earlier draw and was dropped by
  `dedup_refusal_tasks`. Dedup does **not** refill the slot (by design,
  unchanged since turn 1), so the **missing-target family carries 149, not
  150**. This is a recorded deviation from the expectation, not a tuned
  parameter: nothing was re-run, no count was raised to compensate, and the
  seed stands.

- **Repair-slice trajectory split (pre-registered before training, and
  exactly on target):**

  | trajectory | tasks | pairs/task | pairs | shape |
  |---|---|---|---|---|
  | `plain` | **333** | 3 | 999 | `read → patch → done` |
  | `find` | **333** | 4 | 1,332 | `find → read → patch → done` |
  | `run` | **333** | 4 | 1,332 | `read → patch → run → done` |

- **Refusal split, per family:** defect-absent **150**, missing-target
  **149**, symptom-mismatch **150** (449 total, all `read → done`, 2 pairs
  each = **898** pairs).
- **Lens composition:** patch slice **735 python / 264 plaintext**
  (the pre-registered ~735:264); refusal slice **224 / 225**; corpus total
  **959 python / 489 plaintext**. Per trajectory: `plain` 201 py / 132 txt,
  `find` 201 py / 132 txt, `run` **333 py / 0 txt** (the run slice is
  lens-py only by design — spec §2/§3).
- **Gate-aware rejection sampling, actual counts** (screening every
  candidate at draw time against the UNION of **all four** gate sets, using
  the guard's own rules; every rejection redrawn from the same seeded
  stream). The fingerprint's `gate_rejections` **merges the two phases**
  (`generate.py` sums the patch and refuse dicts), so the phase split is
  recorded here, recomputed deterministically from the same seed and the
  same gate union by driving the factory's own draw functions:

  | phase | draws | `goal_near_duplicate` | `search_match` | `target_filename_match` | total rejected | kept |
  |---|---|---|---|---|---|---|
  | patch | 1,700 | 20 | 54 | 627 | 701 | 999 |
  | refuse | 593 | **57** | 0 | 86 | 143 | 450 |
  | **fingerprint total** | 2,293 | **77** | **54** | **713** | **844** | 1,449 |

  Per-shape draw counts inside the patch phase: `find` **796**, `run`
  **473**, `plain` **431** — the find shape absorbs most of the pressure
  because a multi-file task carries more filenames to collide with.

- **The refuse-side near-duplicate pressure came out HIGHER than
  pre-registered, and the expectation is recorded as not held.** The
  turn-4 brief expected the refuse phase's `goal_near_duplicate` rate to
  fall **below** turn 3's 8.8%, on the reasoning that v4's refuse goals are
  fresh-framed (no `goal_phrasing` skeleton frame reused verbatim). The
  measured rate is **57/593 = 9.6%**, against the patch phase's
  **20/1,700 = 1.2%**. Two honest notes on how to read that:
  - **It is not a like-for-like rate.** Turn 3's 8.8% was measured against
    a **three**-gate union; this is measured against a **four**-gate union
    that adds `codec-tasks-v4-mixed`'s own 16 refuse goals to the set every
    candidate is screened against. More gate goals is more chance of a ≥0.8
    Jaccard hit at the same authoring quality.
  - **Fresh framing is a rule about skeleton *frames*, not about token
    overlap.** `goal_near_duplicate` is a token-set Jaccard test; two goals
    that share no fixed prose frame can still overlap heavily in the
    identifier/filename/number vocabulary they draw from. The freshness
    rule was never a claim about this number, and the number does not
    impugn the rule.
  - **The sampler absorbed it: no `GateOverlapTooDenseError`** on either
    abort leg. The **worst 200-draw rejection window** was **53.5%** in the
    patch phase and **31.0%** in the refuse phase, against the >90% abort
    threshold; and 1,700 draws for 999 slots (**1.70x**) / 593 draws for
    450 slots (**1.32x**) are nowhere near the 20x total-draw cap. Had it
    aborted, this would be a BLOCKED report carrying the breakdown verbatim
    — the parameters above are pre-registered and were not tuned.

- **Contamination guard: clean.** Post-hoc CLI run over the written corpus
  against **all four** gate sets: **1,448 tasks vs 104 gate fixtures**
  (20 v1 + 20 v2-mixed + 32 v3-mixed + 32 v4-mixed), zero exact/normalized
  overlaps (goals, **all** `task.files` names and contents — including the
  run slice's planted tests — target filenames, search strings) and no goal
  ≥0.8 Jaccard vs any gate goal. Report committed beside this doc as
  `2026-08-21-flywheel4-contamination-report.json`.
- **Gate file SHA-256s screened against** (committed in the fingerprint):

  | gate | sha256 |
  |---|---|
  | `codec-tasks-v1.toml` | `ab64a38f67b9dc7b97edd8bcbb18fe5803aaaae7745425ae5d8e24afab5ab972` |
  | `codec-tasks-v2-mixed.toml` | `648b9eebbcf69eb5c25d54526e1141495bba3ce5d11acf1772b513a4e5800920` |
  | `codec-tasks-v3-mixed.toml` | `40475bc055f38d6f7c3f543bc32595bdabb8be54bee323c17aa1f6d6ef7873ae` |
  | `codec-tasks-v4-mixed.toml` | `d35391548f258dd97a7dd1fa438887c97c82fabac6c8012269b6c2b8b458b3fe` |

- **Validation split:** **72 task ids** (5%; 48 patch + 24 refuse) →
  **221 validation pairs**, **4,340 train pairs**. Listed in the
  fingerprint. Loss monitoring only, **never** the gate.
- **Turn-3 corpus, for identity comparison only**
  (`2026-08-20-flywheel3-preregistration.md`): seed 20260820, 1,449 tasks /
  4,563 pairs, 0 dedup drops, 960 python / 489 plaintext, 38 template
  families, 321 rejections against three gates, sha `6f88771f…`.

### Determinism, and the one named cause that could move the sha (ruling bT4/R1)

The real-binary determinism boundary is pinned by a test, not by
assertion: `tools/flywheel/tests/test_generate_trajectories.py`'s
`RealToolDeterminismBoundaryTest` drives the **real** binary twice at the
same seed and asserts **zero differing rows**, plus that find rows still
embed a real absolute scratch path (so an "erase the path" regression
cannot satisfy determinism by destroying what it protects). It was run
against **this** binary before generation: 3 tests, OK.

**And one cause of a legitimate byte difference is named in advance.** The
run slice's trained text contains the planted test's real stdout,
including unittest's own timing line. In this corpus **all 333 renderings
read `Ran 1 test in 0.000s`**. A test that ever took longer than 0.5 ms
would render `Ran 1 test in 0.001s` and flip bytes in that row — so a
re-generation that differs **only** in a timing line is a **NAMED-CAUSE
difference**, not a determinism break. **Task 8's corpus-sha check is
where that would surface**, and if it does, the diff is inspected and
reported rather than being treated as either a pass or a silent failure.

## Training (pinned; identical hyperparameters to turns 1, 2 and 3)

Base `Qwen/Qwen3-14B` (HF bf16); unsloth 4-bit load; LoRA r=16 α=32 on
q/k/v/o/gate/up/down projections; seq 4096; **completion-only loss**
(prompt masked); **raw text, NO chat template, NO EOS appended** — each
completion ends exactly at `</action>` (verified: all 4,561 completions
end in `</action>`); 2 epochs over 4,340 train pairs (≈1,085 optimizer
steps at bs 1 × accum 8); lr 2e-4 cosine, warmup 20. Environment freeze
recorded with the training evidence.

### Training seeds statement (binding)

**`train.py`'s two literal seeds do NOT move this turn and were not
changed:** `random_state=20260816` on the LoRA initialization and
`seed=20260816` on `TrainingArguments` are exactly the values turns 1, 2
and 3 used. They are the **procedure's** identity — holding them fixed is
what makes turn 4 a comparison against turns 1-3 rather than a fresh draw
on two axes at once. **The seed that refreshes each turn is the CORPUS
seed** (20260817 → 20260820 → **20260821**), which lives in the
fingerprint. `train.py`'s **header comment was updated in this commit** to
turn-4 wording and to restate this seed rule in the file itself; **no
hyperparameter, no seed, and no code path was changed** (the diff is
header-only).

## Honesty lines (each stated plainly, before any number exists)

- **The gate's run check leaks the expected value, because the planted
  test is a visible sibling.** Each run-granted gate fixture ships
  `test_<stem>.py` beside its target, and the test's assertions
  necessarily encode the goal's expected post-patch behaviour (protocol §6
  risk 3). A model that reads the planted test before patching has a
  strictly easier patch than one inferring the fix from the goal alone.
  Scoring is unaffected (landing reads steps and bytes) — but a high
  run-granted patch number must be read with the leak in view. On the
  baseline boots **no planted test was ever read** by either model
  (baselines §8, limitation 7), which is evidence about those boots, not a
  general clearance.
- **The gate's dict-key planted test shares a literal with the factory's
  `DICT_KEY_POOL`.** The five gate-side planted tests were produced by the
  factory's own public `templates_run_verified.plant_test(task, probe)` —
  deliberately, so gate and corpus cannot drift by transcription — and one
  consequence is that a pool literal appears verbatim in a gate fixture. It
  is not caught by the exact-contents contamination guard and is not a spec
  violation; it is named here for honesty.
- **The planted test's CONTENTS never enter trained text; its FILENAME
  always does.** A mechanical sweep of all 333 run tasks found **zero**
  planted-test content lines in any `prompt` or `completion`, while the
  test's *filename* appears in **all 333** (it is the `run` argv the model
  is trained to emit). The same sweep found **zero** find-sibling content
  lines in trained text, and **every one of the 333 `find` observations
  reads `found 1 matches`** (999 renderings across the three later pairs
  that carry each one).
- **The run step now verifies something, and all 333 observations are
  still `exit 0`.** Turn 3's `python3 -m py_compile` could not fail on a
  semantic defect; turn 4's planted `unittest` is proved able to fail by
  the factory's fails-before rule (executed against the **unpatched** file,
  must exit nonzero) and proved to pass by the tool's real run against the
  patched file. Every one of the corpus's **333 run observations is
  `ran python3 exit 0`** — by construction, since a nonzero exit is a
  structural rejection and is never rendered. So a run-before-done count at
  probe time still measures whether the **habit** transferred, never
  whether the model verified anything.
- **The lens mix is 735:264 on the patch slice, and the run slice is
  lens-py only.** Corpus total **959 python / 489 plaintext** (66.2% /
  33.8%). Any turn-4 plaintext result is read with this composition in
  view.
- **The em dash rides in the symptom-mismatch refusal reasons, and now
  also in the prompt.** It appears in exactly the **150** symptom-mismatch
  `done` completions (the `Checked: … — … Found instead: …` assembler) and
  in **zero** other completions in the corpus, patch or refuse — unchanged
  from turn 3. **New this turn:** every prompt contains em dashes too, from
  the verb card's own headers (`## run — execute a command`) and, on the
  3,229 rows that are not run-granted, from the grant line itself
  (`Granted commands: none — run is not available in this task`). The
  completion-side claim is the one that bears on trained output, and it is
  unchanged; the prompt-side fact is stated so no one later reads an em
  dash in a v4 prompt as evidence of anything.
- **Every prompt carries a grant line, and the two forms partition the
  corpus exactly.** `Granted commands: python3 -m unittest` on **1,332**
  rows (all four pairs of each of the 333 run tasks) and
  `Granted commands: none — run is not available in this task` on the
  other **3,229** (find 1,332 + plain 999 + refuse 898). **Zero** rows
  carry neither and **zero** carry both.
- **Generation ran far more interpreter starts than the brief estimated,
  and the measured number is recorded instead.** The estimate was ~666
  (333 fails-before + 333 passes-after). Measured: **936 factory-side
  starts** — 468 `python3 -m unittest` fails-before checks and 468
  `python3 -c import …` expected-value computations, over **473
  run-shaped draws** with the pure-function cache absorbing the repeats —
  **plus 333 tool-side runs** (one per rendered run task), for **1,269**
  in total. Nothing was wrapped in `timeout` (this box's `timeout`
  segfaults on multithreaded children).

## Honest possibilities, pre-registered

Carried from spec §5:

- **The grant line over-triggers `run` on ungranted fixtures** — surfaces
  as grant-violation `TaskStep` rows, measured rather than assumed absent
  (protocol §6 risk 2). **It did NOT materialise at baseline**: flywheel3
  emitted zero `run` verbs on the 27 ungranted fixtures and stock emitted
  zero anywhere. flywheel4 is trained on the cue, so it is the first model
  with a reason to over-generalise it; the count is reported whatever it
  is, with the verb split.
- **The argv is trained but not read from the prompt** — the incumbent
  showed exactly this shape under v4 (`run` in the right place, five times,
  with the *trained* argv rather than the *granted* one). flywheel4 is
  trained on the granted argv, so the same failure would have to look
  different — but "emits some other argv" remains a live outcome and would
  keep productive run at 0/5.
- **A new trajectory shape competes with find** — the run slice's ideal is
  now 4 steps with a two-file workspace; a patch-class regression or a drop
  in productive find is the way that shows. The patch floor and the
  productive-find endpoint catch it.
- **reason-grounding reveals confabulation on the patch side too** —
  looked for and reported; the endpoint's measured blindness to bare-prose
  identifiers (above) bounds what it can show.
- **A model trained without the cue ignores it** — **fw3@v4 did not ignore
  it**: it scoped `run` exactly to the granted slice. That question is
  answered before training and is not re-asked of flywheel4.
- **Over-refusal** — caught by the G4 leg, and by the patch class: a
  wrongful refusal on a real defect is scored in the **patch** class, never
  in the refuse class, so a 16/16 refuse alongside a patch regression is a
  coherent shape, not a contradiction (baselines §3.4).
- **Symptom-mismatch keyed on surface cues rather than file-checking** —
  the differently-authored gate fixtures are the net, and this turn's net
  is tighter: no v4 refuse goal reuses a `goal_phrasing` skeleton frame
  verbatim (frozen-set header rule 1), so a model that learned the *frame*
  as a refuse cue is netted here in a way turn 3's set could not manage.
- **Trained find/run usage failing to express at probe time** — the
  secondary endpoints record it; a finding, not a failure.

## Amendment rule

Any amendment to this pre-registration after this commit is a **separate
dated file** in `docs/superpowers/evidence/`, cross-linked from here by a
later commit, and **never** an in-place edit of this document. No fixture,
floor, endpoint, seed, or corpus parameter changes after a number has been
seen.

## Committed artifacts

- `2026-08-21-flywheel4-fingerprint.json` — corpus sha, seed, gate shas,
  merged gate-rejection counts, per-template/lens/trajectory tallies,
  validation split ids.
- `2026-08-21-flywheel4-contamination-report.json` — post-hoc guard run
  over the written corpus against all four gate sets.
- `~/flywheel4/corpus.jsonl` — **out of repo**, sha256 above.
