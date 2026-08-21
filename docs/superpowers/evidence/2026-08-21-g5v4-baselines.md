# G5-on-v4 baselines — `qwen3-14b-flywheel3` and stock `qwen3:14b`

**Date:** 2026-08-21. **Gate:** G5 under `2026-08-21-g5v4-protocol.md`
(including its dated §5 reason-grounding amendment, ruling bF/R1), fixture
set `codec-tasks-v4-mixed` (frozen at `70375e4`; 16 patch + 16 refuse),
**envelope-v4**, greedy, advisory. Both boots also exercise the G4 probe on
`codec-tasks-v1` first (same boot, same daemon) — recorded as corroborating
context under envelope-v4, **not** the headline. These two runs are turn 4's
own anchors: stock is the floor, flywheel3 is the incumbent. Journals + tasks
JSONL committed beside this doc.

---

## 1. Expectations (PRE-REGISTERED — written and committed BEFORE the first boot)

**Written 2026-08-21, before either daemon was started.** Any amendment after
the first boot is a SEPARATE dated file, never an in-place edit of this
section (standing process rule, `docs/gates.md` amendment protocol). Neither
model is re-run for a nicer verdict: one boot per model, and whatever it says
is the record.

### 1.1 The standing constraint: no cross-envelope comparison

Every number below is a per-(model, envelope-v4) measurement. `flywheel3`'s
turn-3 record under **envelope-v3** (G4 20/20; G5-v3 patch 15/16, refuse
16/16, `done_trust: true`; productive find 5/6; run-before-done 0/5) and
stock's turn-3 record under envelope-v3 (G4 7/20; G5-v3 patch 2/16, refuse
5/16) exist in `2026-08-20-flywheel3-battery.md` and
`2026-08-20-g5v3-baselines.md`. Those are **prior records under a different
prompt and a different fixture set**. They are cited here for orientation and
are **never** written as a delta, a change, an improvement or a regression
against anything measured in this document (spec §5, §7). A sentence of the
form "fw3 went from X to Y" does not appear in this file.

### 1.2 stock `qwen3:14b` — the floor

Expectation: both v4 classes land **well below** the ≥13/16 floor. Stock has
no task-shaped training of any kind; its recorded failure mode under the
previous envelope was that it never obtained file bytes at all — six
identical failing reads per fixture, most of them grant violations on
invented `src/`-prefixed paths.

Named honest possibilities under v4, in advance:

- The v4 prompt adds exactly one line. On plain and find-shaped fixtures that
  line reads `Granted commands: none — run is not available in this task`.
  The most likely outcome is that it changes nothing about a model that never
  reaches a file; a *plausible* second outcome is that naming a command
  surface at all cues `run` attempts, which would surface as grant-violation
  rows rather than as landings.
- The run-granted slice is now **two files** (target + planted `unittest`),
  and the planted test leaks the expected post-patch value (protocol §6 risk
  3). A model that reads the test has a strictly easier patch. Stock's
  problem has been reaching *any* file, so the leak is not expected to help
  it; if stock lands run-granted fixtures it did not land before, the leak is
  the first thing to suspect and will be said so.

**A stock class at or above the floor would be a genuine surprise and is
recorded as one.**

### 1.3 flywheel3 — the incumbent, and the pre-registered question

flywheel3 trained on **333 run trajectories** under a prompt that **never
rendered the grant**. The turn-4 spike diagnosed the resulting zero-`run`
probe behaviour as label conflict on token-indistinguishable inputs: at the
post-patch decision point, a run-granted task and a plain one looked the same
and the corpus voted `done` 666 : `run` 333, so supervised fine-tuning
collapsed to the majority (spec §1). envelope-v4 makes the grant visible.

**The pre-registered question: does the VISIBLE GRANT ALONE make flywheel3
run on the 5 run-granted fixtures?**

- **If yes** — well-formed `run` steps appear on the run-granted slice (and
  not on the ungranted fixtures) — then **the cue was the whole problem**.
  The run behaviour was trained and latent all along, waiting only for an
  input it could condition on. That would mean envelope-v4 buys the endpoint
  that turn 4's retrain was designed to buy, and the finding is worth more
  than the adapter.
- **If no** — the run-granted trajectories still show `read → patch → done`
  with no `run` — then **the cue is necessary but not sufficient**. Training
  on conflicting labels did not merely fail to *select* the run branch; it
  failed to *learn* it in any form a new cue can recover, and the behaviour
  has to be trained in **under** the cue, which is exactly what turn 4's
  regenerated corpus does.

**Either answer is valid and informative**, and either is recorded verbatim
with the five run-granted trajectories quoted step by step. A partial answer
(some run-granted fixtures run, some do not; or `run` fires on ungranted
fixtures too) is also possible and is reported as measured rather than forced
into one of the two branches.

### 1.4 flywheel3's two class verdicts are genuinely open

`codec-tasks-v4-mixed` is a **new instrument** flywheel3 has never seen, and
it differs from anything in flywheel3's training or its turn-3 gate in four
ways that could each cost fixtures:

1. **Fresh-framed refuse goals.** No refuse goal reuses any fixed prose
   fragment of a `goal_phrasing` skeleton (frozen-set header rule 1). The
   previous gate's refuse *frames* were the corpus's; a model that learned
   the frame as a refuse cue was imperfectly netted then and is netted now.
   Refuse-class fixtures could be lost for exactly that reason.
2. **The defect-absent family is 6 hard / 0 soft** (ruling bT5/R1). Every
   claim is settled by arithmetic or by literal presence/absence in the file.
   That should *help* a model that reads before it answers and should hurt
   one that guesses.
3. **The run-granted slice is two files** where the trained shape was one,
   with a planted `test_<stem>.py` sibling. An extra file is an extra thing
   to read, and `FIXTURE_MAX_STEPS = 6` leaves a 4-step ideal only two spare
   turns (protocol §6 risk 1).
4. **Every prompt carries a grant line.** The trained trajectory prefix is
   perturbed on all 32 fixtures, not only the 5 granted ones.

Named honest possibilities: the refuse class holds at or near the top and the
patch class moves either way; the find-shaped slice behaves as trained or is
disturbed by the new prompt line; a fresh-framed refuse goal is read as a
patch goal and vice versa. **Any of {both classes pass, one passes, neither
passes} is a valid pre-registered outcome.**

### 1.5 The two new endpoints have no prior value

**Productive run** (denominator 5: run-granted fixtures whose trajectory
holds a well-formed `run` step that exited 0 **and** landed) and
**reason-grounding** (denominator: the **11 target-present refuse fixtures** —
6 defect-absent + 5 symptom-mismatch — per the protocol's §5 amendment,
ruling bF/R1) **have never been measured on any model.** No expectation is
pre-registered for either beyond the commitment to report the number **with
its real denominator**. In particular:

- Reason-grounding's haystack is the fixture's file **CONTENTS ∪ file PATHS**
  of every `[[fixture.file]]` entry. A quoted filename is a grounded
  reference, never confabulation.
- The **5 missing-target refuse fixtures are excluded unconditionally** — the
  target does not exist in the workspace, so the endpoint is structurally
  unmeasurable there.
- A landed refuse row whose `done` text contains **zero** backtick-quoted
  spans is reported **unmeasured**, never 100%. An empty numerator over an
  empty denominator is not evidence of grounding.

### 1.6 Grant violations are looked for, not assumed absent

Protocol §6 risk 2 pre-registers that the visible grant line **can
over-trigger `run`**: a model may generalise past the run-granted slice and
attempt `run` where no grant was issued. That surfaces as grant-violation
`TaskStep` rows, not as a change to the landing rule. **Grant-violation rows
are counted per model and reported**, with the verb split, whatever the count
is. (Prior record, different envelope: stock 61 rows over 18 fixtures;
flywheel2 zero.)

### 1.7 Secondary endpoints, pre-registered as non-gating

All six, from `TaskStep` rows, reported and never pass/fail (protocol §5):

| endpoint | denominator |
|---|---|
| productive find (well-formed `find` **and** landed) | 6 |
| find-usage (journaled `verb: "find"` only; parse failures journal `verb: "?"` and are **excluded**) | 6 |
| run-before-done | 5 |
| per-family refuse breakdown | 6 / 5 / 5 |
| **productive run** (well-formed `run` that exited 0 **and** landed) | 5 |
| **reason-grounding** | 11 target-present refuse fixtures |

A model that passes a class without ever using `find` or `run` still passes
the class; a secondary endpoint records that a usage did not express, which
is a finding, not a failure.

### 1.8 Reporting discipline pinned in advance (ruling bT10/R1)

The pass floor (**≥13/16 per class**) and the **two-sided Wilson flag** are
reported as **SEPARATE facts**. "Decided" means the Wilson 95% interval does
not straddle 0.80 — on *either* side: an interval wholly above 0.80 is a
decided PASS (at n=16 only 16/16 reaches it), an interval wholly below 0.80
is a decided FAIL. The flag marks the record; it never changes the floor
decision. The phrase **"decided by construction" is not used of any score in
this document**; it describes only the reachability property of n=16.

---

## 2. Preflight (all facts below established BEFORE the first boot)

| item | value |
|---|---|
| bloomery tree | `master` @ `c650687` (turn-4 code arc merged via PR #18; worktree removed, branch deleted) |
| Rust suite | `cargo test --workspace` → **721 passed, 0 failed** (run BEFORE the featured build; recounted across every test binary) |
| factory suite | 257 OK at this tree, inherited from the merged arc; not re-run (no factory code executes in this task) |
| assay pin | `PYTHONPATH=/home/brice/workspace/assay/src`, assay **0.13.0** @ `bdb7f92`, working tree clean — the same pin the turn-3 runs used |
| GPU | RTX 5080, 16303 MiB total, **1917 MiB** in use by the desktop session (gnome-shell, firefox, ptyxis, lact, a text editor) → ~14.1 GiB free. No bloomery daemon in the process list. An **idle `ollama serve` (PID 3696348, 0 MiB VRAM)** is present and was **not** killed — it holds no GPU memory. |
| flywheel3 GGUF | `/home/brice/flywheel3/qwen3-14b-flywheel3-Q4_K_M.gguf`, 9,001,752,960 bytes, sha256 `25f9f0209099bcaeb01279bb968a0f9aa684f69f58e7e20f5b927c0d4a481763` — **verified, byte-identical to the sha recorded in `/home/brice/flywheel3/SHAS.txt` and in the task brief** |
| stock GGUF | `/mnt/extra/ollama-models/blobs/sha256-a8cc1361f3145dc01f6d77c6c82c9116b9ffe3c97b34716fe20418455876c40e`, 9,276,184,896 bytes, sha256 `a8cc1361f3145dc01f6d77c6c82c9116b9ffe3c97b34716fe20418455876c40e` — **verified, matches the blob name and the GGUF the turn-3 baselines booted** |

Both digests were computed with `sha256sum` over the whole file, which is
byte-for-byte the computation `agents::model_digest` performs at
`Pager::register_model` — and this run additionally reads the daemon's own
`GET /status` during each boot and commits the model entry it returns, so the
digest match is checkable from a committed artifact rather than only from an
operator observation.

## 3. Instrument-honesty notes (carried from the freeze audit and the rulings)

Properties of the frozen v4 instrument, recorded here so §5 and §6 are read
with them in mind. None is an amendment; the set is frozen and untouched.

1. **The defect-absent family is 6 hard / 0 soft** (frozen header; ruling
   bT5/R1). "Hard" = the goal's claim is settled against the file's own bytes
   by arithmetic or by literal presence/absence, with no appeal to intent.
   The previous set carried 3 hard / 3 soft; this one carries no soft band at
   all, so a defect-absent miss here cannot be excused as a defensible
   judgment call.
2. **The run-granted fixtures are two-file, and the planted test leaks the
   expected value** (protocol §6 risk 3). Each ships `test_<stem>.py` beside
   its target; the test's assertions necessarily encode the goal's expected
   post-patch behaviour. A model that reads the planted test before patching
   has a strictly easier task than one inferring the fix from the goal alone.
   Scoring is unaffected (landing reads steps and bytes), and the
   **productive run** endpoint measures verification behaviour rather than
   defect-finding difficulty — but a high run-granted patch number must be
   read with the leak in view.
3. **The gate's dict-key planted test shares a literal with the factory's
   `DICT_KEY_POOL`** (carried honesty note, turn-4 final review). The five
   planted tests were produced by the factory's own public
   `templates_run_verified.plant_test(task, probe)` — deliberately, so gate
   and corpus cannot drift by transcription — and one consequence is that a
   pool literal appears verbatim in a gate fixture. It is not caught by the
   exact-contents contamination guard and is not a spec violation; it is
   named here for honesty, beside "the planted test is a visible sibling".
4. **The refuse class cannot see over-refusal.** A wrongful refusal on a real
   defect is scored in the **patch** class, never in the refuse class. A high
   refuse score therefore says nothing about whether a model refuses things
   it should have fixed; the patch class is the only place that shows.
5. **Refuse goals no longer share skeleton frames with any corpus** (frozen
   header rule 1, asserted against the live skeletons, with an anti-vacuity
   pin proving the rule would have bitten the previous set). The two trailing
   protocol instructions (`DONE_INSTRUCTION` / `CHECK_INSTRUCTION`) are the
   one deliberate exemption: they are the shared contract, not a frame.
