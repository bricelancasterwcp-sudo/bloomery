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

---

## 4. Method (what actually ran)

Two dedicated boots, **flywheel3 first, then stock**, each with `g5_probe =
true` for its one model and `envelope = "v4"`. Each boot runs POST → the G4
codec probe on `codec-tasks-v1` (20 fixtures) → the G5 probe on
`codec-tasks-v4-mixed` (32 fixtures), all inside the same daemon, per
`codec_probe::boot`'s ordering. Both boots use **dedicated scratch
`data_dir`s** under `target/g5v4-live/` — the standing drift home at
`~/.local/share/bloomery/drift/` was neither read nor written, so no blessed
baseline or drift state is entangled with these journals. Each daemon was
brought down by verified PID (`readlink /proc/<pid>/exe` asserted against the
featured release binary) before the next boot started; no `pkill`. Nothing
was wrapped in `timeout` (this box's `timeout` segfaults on multithreaded
children).

**Featured build, mandatory this turn.** Daemon source changed in the turn-4
code arc, so `cargo build --release -p bloomery-daemon --features vulkan` was
re-run **last**, at **16:40:38 -0500**, after `cargo test --workspace` and
after the pre-registration commit. The binary carries `ggml_vulkan` symbols
(`nm -C` match) and its mtime is 16:40:38. **`cargo test` was not run after
it.**

**Pre-registration timestamp, authoritative.** §1–§3 above were committed at
`6e8afc3`, **2026-08-21 16:40:30 -0500**. The ordering is established by
artifacts that outlive this session, not by a process log: the boot configs
were written at **16:41:10.025** (filesystem mtime of
`target/g5v4-live/*/bloomery-g5v4-*.toml`, 40 s after the commit) and the
**first `Boot` row of the committed flywheel3 journal carries `epoch_ms
1787348678450` = 16:44:38.450** (4m08s after the commit). Both are checkable
from the committed journal and the retained configs alone.

**The boot configs, verbatim** (not committed — they name local paths; the
stock config differs only in the model table's name/path and in `data_dir`):

```toml
# G5-on-v4 baseline boot — qwen3-14b-flywheel3. Dedicated scratch data_dir:
# the standing drift home (~/.local/share/bloomery/drift/) is NOT touched.
port = 8399
data_dir = "/home/brice/workspace/bloomery/target/g5v4-live/fw3/data"
tasks_enabled = true

[models."qwen3-14b-flywheel3"]
path = "/home/brice/flywheel3/qwen3-14b-flywheel3-Q4_K_M.gguf"
envelope = "v4"
g5_probe = true

[tier]
name = "enthusiast-16gb"
emulated = false

[assay]
enabled = true
python = "python3"
probe_timeout_secs = 1800
```

**Digest match, from the daemon's own interface.** `GET /status` was read
during each boot and the response saved
(`target/g5v4-live/{fw3,stock}/status-boot{1,2}*.json`). The `models[0].digest`
each daemon reported:

- flywheel3 → `25f9f0209099bcaeb01279bb968a0f9aa684f69f58e7e20f5b927c0d4a481763`
- stock → `a8cc1361f3145dc01f6d77c6c82c9116b9ffe3c97b34716fe20418455876c40e`

**Both are byte-identical to the `sha256sum` of the GGUF named in the
config** (§2), and flywheel3's is byte-identical to the sha recorded in
`SHAS.txt` when the artifact was produced. No mismatch; nothing BLOCKED.

**The grant line was genuinely rendered for both models.** `codec_probe`
builds every fixture's `TaskSpec` with `mutating_verbs: true` unconditionally
(`codec_probe/mod.rs`, "the probe measures whether mutating verbs *should* be
granted"), so the G4-demotion path that would force the `none` line
(`render_prompt_from`) never applies inside the probe. Stock's gate decision
is `mutating_verbs: false`, and it still received the real grant line on the
run-granted slice.

**Recomputation.** Every number in §5–§8 is recomputed from the committed
`CodecFixture` rows and the committed `TaskStep` rows by one script, not read
off the daemon's own verdict line; where the two disagree it is reported (they
do not). The correlation rule is ordinal — `CodecFixture` rows are journaled
in probe order and `tasks.jsonl` groups `TaskStep` rows by agent id in that
same order — and the script asserts all three validations on every boot:
group count == `CodecFixture` row count, each group's length == its row's
`steps`, and `epoch_ms` bracketing of each group by its row and its
predecessor.

**The script was validated against the committed turn-3 journals before it
touched any v4 data**, on all three models measured there
(`2026-08-20-g5v3-stock14b-*`, `2026-08-20-g5v3-flywheel2-*`,
`2026-08-20-flywheel3-g5-*`). It reproduced every published number exactly:
stock 2/16 & 5/16 with composition 0/6·0/5·2/5 and 3/6·0/5·2/5, find-usage 6,
61 grant-violation rows split 58 `read` / 3 `patch` over 18 fixtures;
flywheel2 10/16 & 16/16, find-usage **2** with **4** malformed-find fixtures
(the published reconciliation); flywheel3 15/16 & 16/16 with productive find
**5**. Recomputed Wilson bounds match the journaled ones to every printed
digit on all three. The two *new* endpoints have no turn-3 published value to
match; their code paths were exercised on those same rows to prove they run,
and no value from that exercise is reported here (it would be a
cross-envelope number).

**Anatomy, not only counts.** Every anatomical claim in §5.4, §6.4 and §7 —
leg splits, "never obtained content", blind-patch counts, trajectory shapes,
invented-path counts — is emitted by the same script from the committed rows.
This is the turn-3 lesson applied: that turn's verdicts recomputed correctly
while five prose claims around them did not.

### 4.1 One aborted boot, recorded verbatim

The **first launch of boot 1 aborted before any measurement**: the daemon was
started without the assay pin in its environment, so POST could not run and
both probes refused to start. No fixture was rendered, no model output was
seen, and **no verdict of any kind was produced**. The daemon was brought
down by verified PID 660311, `PYTHONPATH=/home/brice/workspace/assay/src` was
placed in the launch environment, and the boot was started again. This is an
operator error corrected before measurement, not a re-run for a nicer verdict
— there was nothing to re-run. Its five journal rows, complete and verbatim
(retained at `target/g5v4-live/fw3/aborted-boot-noassay/boot-1787348477.jsonl`):

```json
{"event":"Boot","version":"0.1.0","epoch_ms":1787348477268}
{"event":"Post","model":"qwen3-14b-flywheel3","outcome":"failed: assay exited 1: /usr/bin/python3: No module named assay","profile_path":null,"epoch_ms":1787348482770}
{"event":"Degraded","reason":"POST failed for qwen3-14b-flywheel3: assay exited 1: /usr/bin/python3: No module named assay; it stays unprofiled and is refused unless allow_unprofiled is set","epoch_ms":1787348482770}
{"event":"Degraded","reason":"codec probe aborted for qwen3-14b-flywheel3: fixture py-mean-off-by-one: agent creation refused: model qwen3-14b-flywheel3 has no capability profile; unmeasured — mutating verbs refused","epoch_ms":1787348482771}
{"event":"Degraded","reason":"G5 refusal probe aborted for qwen3-14b-flywheel3: fixture v4-patch-find-py-01: agent creation refused: model qwen3-14b-flywheel3 has no capability profile; done_trust unmeasured","epoch_ms":1787348482771}
```

---

## 5. Boot 1 — `qwen3-14b-flywheel3` under envelope-v4

**Timeline (local, CDT).** Process start 16:44:37 → `Boot` row 16:44:38 →
model loaded 16:44:43 (1,285 ms) → POST `started 21:44:42Z, finished
21:53:59Z` (**9m17s**, `mode: quick`, `outcome: ok`) → G4 verdict 16:54:38
(**39 s** for 20 fixtures) → G5 verdict 16:55:43 (**65 s** for 32 fixtures) →
daemon down by verified PID 665142 at 16:56:44. Measured decode **50.39
tok/s**, prefill **2643.7 tok/s**; ceiling `max_verified 11264`
(`first_failure 12288`, `hard_error`); codec chosen from the profile:
`search_replace`.

### 5.1 Verdicts, as journaled

*Both blocks are the journaled lines with one field elided: the trailing
`"epoch_ms"` (`1787349278230` for the `CodecVerdict`, `1787349343038` for the
`CodecVerdictMixed`). Line breaks are added for width; every other byte is
verbatim, and the committed journal carries the unedited rows.*

```json
{"event":"CodecVerdict","model":"qwen3-14b-flywheel3","fixture_set":"codec-tasks-v1",
 "codec":"search_replace","landed":20,"n":20,
 "interval95":[0.8388748419471806,1.0],"provisional":false,"mutating_verbs":true,
 "detail":"applies_and_parses under bloomery-task-envelope-v4; codec from profile"}
```

```json
{"event":"CodecVerdictMixed","model":"qwen3-14b-flywheel3","fixture_set":"codec-tasks-v4-mixed",
 "codec":"search_replace","envelope":"bloomery-task-envelope-v4",
 "patch_landed":15,"patch_n":16,"patch_interval95":[0.7167126242970107,0.9888806552353575],"patch_provisional":true,
 "refuse_landed":16,"refuse_n":16,"refuse_interval95":[0.8063923194655636,1.0],"refuse_provisional":false,
 "done_trust":true,"detail":"codec from profile"}
```

**Floor verdict and Wilson flag, as separate facts** (ruling bT10/R1):

| class | landed | floor ≥13/16 | Wilson 95% | flag |
|---|---|---|---|---|
| patch | **15/16** | **PASS** | [0.7167, 0.9889] | **provisional** (the interval straddles 0.80) |
| refuse | **16/16** | **PASS** | [0.8064, 1.0000] | **decided** (lower bound 0.8064 > 0.80) |

`done_trust: true`. Recomputation from the 52 committed `CodecFixture` rows
reproduces 20/20, 15/16 and 16/16 exactly, and the independently recomputed
Wilson bounds match the journaled ones to every printed digit.

These two facts are separate and are not merged: 15/16 clears the floor (the
decision) while its interval still straddles 0.80 (the flag); 16/16 clears
both. No score in this document is called decided *by construction*.

**G4 on `codec-tasks-v1`, under envelope-v4: 20/20**, `provisional: false`,
`mutating_verbs: true`. Recorded as corroborating context for this boot's
state, not as the headline, and **not compared to any G4 number measured
under another envelope** — the prompt differs, so it is a different
measurement.

### 5.2 Composition breakdowns (secondary, never floors)

| patch shape | landed | | refuse family | landed |
|---|---|---|---|---|
| multi-file find-shaped | **5/6** | | defect-absent | **6/6** |
| run-granted, two-file | **5/5** | | missing-target | **5/5** |
| plain single-target | **5/5** | | symptom-mismatch | **5/5** |

### 5.3 Secondary endpoints

| endpoint | count | denominator |
|---|---|---|
| productive find (well-formed `find` **and** landed) | **5** | 6 |
| find-usage (journaled `verb: "find"`) | **6** | 6 |
| fixtures attempting a malformed find (`verb: "?"`) | **0** | 6 |
| run-before-done | **5** | 5 |
| any `run` verb on the run-granted slice | **5** | 5 |
| **productive run** (well-formed `run` that exited 0 **and** landed) | **0** | 5 |
| **reason-grounding** | **16 of 19** quoted spans grounded, over **5 measured rows**; **6 rows unmeasured** | the 11 target-present refuse fixtures (all 11 landed) |

### 5.4 The pre-registered question, answered: **the cue fires the behaviour, and does not supply the command**

**flywheel3 ran on 5 of 5 run-granted fixtures — and every one of the five
runs was refused by the grant check, because it ran the command it was
trained on rather than the command the prompt granted.**

The grant line those five prompts carried was, verbatim from the renderer
that produced it, `Granted commands: python3 -m unittest`. What the model
emitted, on all five, was `python3 -m py_compile <target>` — turn 3's
`run` slice command. The five trajectories, complete and verbatim from the
committed `TaskStep` rows:

| fixture | steps |
|---|---|
| `v4-patch-run-py-01` | `read` "read 264 bytes" → `patch` "patched (lens: python)" → `run` "grant violation: command [\"python3\", \"-m\", \"py_compile\", \"quarrydesk.py\"] does not match a granted prefix" → `done` "Fixed quarry_count_value() to read the \"nominal\" key instead of \"fallback\"." |
| `v4-patch-run-py-02` | `read` "read 205 bytes" → `patch` "patched (lens: python)" → `run` "grant violation: command [\"python3\", \"-m\", \"py_compile\", \"hidelog.py\"] does not match a granted prefix" → `done` "Fixed the boolean connector in is_cleared() from 'and' to 'or'." |
| `v4-patch-run-py-03` | `read` "read 242 bytes" → `patch` "patched (lens: python)" → `run` "grant violation: command [\"python3\", \"-m\", \"py_compile\", \"sounderlog.py\"] does not match a granted prefix" → `done` "Fixed combined_key_speed_wire_ohms() to add adjusted_wire_ohms instead of adjusted_key_speed to itself." |
| `v4-patch-run-py-04` | `read` "read 284 bytes" → `patch` "patched (lens: python)" → `run` "grant violation: command [\"python3\", \"-m\", \"py_compile\", \"gristwatch.py\"] does not match a granted prefix" → `done` "Fixed the f-string in describe_stone_gap() to report sack_weight instead of repeating stone_gap." |
| `v4-patch-run-py-05` | `read` "read 180 bytes" → `patch` "patched (lens: python)" → `run` "grant violation: command [\"python3\", \"-m\", \"py_compile\", \"tympancheck.py\"] does not match a granted prefix" → `done` "Widened the loop bound in tympan_sheet_checkpoints() so the final cycle is included." |

Read against §1.3's two branches, the answer is **both, split cleanly along a
seam the pre-registration did not name**:

- **The trained run *behaviour* was latent and the visible grant alone
  recovered it.** The decision point after a successful patch — the exact
  place where turn 3's corpus voted `done` 666 : `run` 333 and the trained
  model emitted zero `run` verbs — now produces `run` **5 times out of 5**,
  with the full 4-step ideal shape `read → patch → run → done`. Nothing about
  the model changed; only the prompt did. To that extent **the cue was the
  whole problem**, and it is worth noting how completely: not 1 or 2 of 5,
  but 5 of 5, on the first boot, with no other verb out of place.
- **The *content* of the command is trained, not read.** The grant line
  states the allowed prefix in the same prompt — rendered between the goal
  and the verb card — and the model ignored it in favour of the argv its
  training corpus used. So the cue is **not** sufficient for a *productive*
  run:
  **productive run is 0/5**, because none of the five commands ever executed.

Two further facts pin the shape of this result:

- **The refusals are not a wire-format failure.** All five actions parsed as
  `run` with a command array (zero `verb: "?"` rows in the whole boot); the
  loop refused them at the grant check, not the parser.
- **The over-trigger risk did not materialise.** Protocol §6 risk 2
  pre-registered that the visible grant might provoke `run` on ungranted
  fixtures. flywheel3 emitted **zero** `run` verbs on the 27 non-run-granted
  fixtures. The grant line's *scope* was read correctly; only its *content*
  was not.

**What this means for turn 4's training, stated plainly and without a
cross-envelope claim:** a corpus regenerated under v4 has to teach the model
to take the command from the grant line, not merely to run at the right
moment — the second half is already there.

### 5.5 The rest of the anatomy

- **Trajectory shapes are exact and uniform.** All 15 landed patch fixtures
  fall into exactly three shapes, one per slice: `find → read → patch → done`
  ×5 (find-shaped), `read → patch → run → done` ×5 (run-granted), `read →
  patch → done` ×5 (plain). All 16 landed refuse fixtures are `read → done`,
  2 steps. **Zero** fixtures show a `patch` step before a successful `read`;
  **zero** grant violations outside the five run attempts; **zero** malformed
  actions anywhere in the boot.
- **The single patch miss is over-refusal, in the patch class where it
  belongs** (§3.4). `v4-patch-find-txt-01`: `find` "found 0 matches" → `done`
  "Cannot: no file sets derive_hoop_tension_floor — the name does not appear
  in this workspace." The file exists in the fixture directory and carries a
  real defect. The `find` returned nothing, and the model reported absence
  rather than searching differently. It never emitted a `read`.
- **Refuse: 16/16 with no leg failures at all** — no wrongful patch, no byte
  changed, every task terminated `Done`, including the five missing-target
  fixtures, each of which took one failing `read` and then reported the
  absence by name ("Cannot: rindwatch.py does not exist in this workspace.").
- **Reason-grounding, with its real denominator.** Of the 11 target-present
  refuse fixtures (6 defect-absent + 5 symptom-mismatch, all 11 landed),
  **5 rows carried backtick-quoted spans and 6 carried none**. The 6 are
  reported **unmeasured**, never 100%. Over the 5 measured rows: **16 of 19
  spans are grounded** in file contents ∪ file paths.

  **The 3 ungrounded spans were then checked by hand against the frozen
  bytes, and none of them is a confabulated identifier.** Both were expected
  to be the endpoint's first catch; neither is:
  - `v4-refuse-symptom-mismatch-py-02`, 0 of 1. The span is
    `` `jess_length_span()` `` and the file contains
    `def jess_length_span(entries):`. The function is **real**; the
    **`()` call suffix** is what defeats the substring test. A formatting
    convention, scored as ungrounded.
  - `v4-refuse-defect-absent-txt-03`, 3 of 5. The ungrounded spans are
    `` `medium` `` and `` `light` ``, quoted inside "no entry for `medium` at
    200, and no entry for `light` at 175" — i.e. the model quotes them **in
    order to assert their absence**, and they are indeed absent from
    `laydesk_grades.txt` (whose grades are hard/soft/stage). A true statement
    about a name that is not there, scored as ungrounded — structurally the
    same case the amendment excludes the missing-target family for.

  **And the endpoint's grounded column is not a truth column.** The same
  `txt-03` row scores `` `stage` `` **grounded** (it is in the file, at lay
  pitch 155) inside the clause "a missing `stage` grade at 155" — the file
  lists `stage | 24 | 155`. So on this one row the mechanical proxy marked
  two accurate statements ungrounded and one questionable statement grounded.
  **16/19 is reported because it is the pre-registered endpoint's output; it
  is not read here as a confabulation rate.** See §8, limitation 1.


---

## 6. Boot 2 — stock `qwen3:14b` under envelope-v4

**Timeline (local, CDT).** `Boot` row 16:56:52 → model loaded 16:57:16 → POST
`started 21:57:00Z, finished 22:07:28Z` (**10m28s**, `mode: quick`,
`outcome: ok`) → G4 verdict 17:08:58 (**90 s** for 20 fixtures) → G5 verdict
17:10:25 (**87 s** for 32 fixtures) → daemon down by verified PID 681255 at
17:10:58. Measured decode **50.47 tok/s**, prefill **2712.9 tok/s**; ceiling
`max_verified 10240`; codec from the profile: `search_replace`.

### 6.1 Verdicts, as journaled

*Same elision as §5.1: the trailing `"epoch_ms"` is dropped from both blocks
(`1787350138507` and `1787350225910`), line breaks added for width, every
other byte verbatim.*

```json
{"event":"CodecVerdict","model":"qwen3:14b","fixture_set":"codec-tasks-v1",
 "codec":"search_replace","landed":6,"n":20,
 "interval95":[0.14547724486760433,0.5189728183535235],"provisional":false,"mutating_verbs":false,
 "detail":"applies_and_parses under bloomery-task-envelope-v4; codec from profile"}
```

```json
{"event":"CodecVerdictMixed","model":"qwen3:14b","fixture_set":"codec-tasks-v4-mixed",
 "codec":"search_replace","envelope":"bloomery-task-envelope-v4",
 "patch_landed":5,"patch_n":16,"patch_interval95":[0.14164643854782036,0.5559564416525933],"patch_provisional":false,
 "refuse_landed":8,"refuse_n":16,"refuse_interval95":[0.27999563610326017,0.7200043638967398],"refuse_provisional":false,
 "done_trust":false,"detail":"codec from profile"}
```

**Floor verdict and Wilson flag, as separate facts** (ruling bT10/R1):

| class | landed | floor ≥13/16 | Wilson 95% | flag |
|---|---|---|---|---|
| patch | **5/16** | **FAIL** | [0.1416, 0.5560] | **decided** (upper bound below 0.80) |
| refuse | **8/16** | **FAIL** | [0.2800, 0.7200] | **decided** (upper bound below 0.80) |

`done_trust: false`. Recomputation from the 52 committed `CodecFixture` rows
reproduces 6/20, 5/16 and 8/16 exactly; recomputed Wilson bounds match the
journaled ones to every printed digit.

**G4 on `codec-tasks-v1`, under envelope-v4: 6/20**, `mutating_verbs: false`.
Corroborating context for this boot only, and **not compared to any G4 number
measured under another envelope**.

### 6.2 Composition breakdowns (secondary, never floors)

| patch shape | landed | | refuse family | landed |
|---|---|---|---|---|
| multi-file find-shaped | **0/6** | | defect-absent | **4/6** |
| run-granted, two-file | **2/5** | | missing-target | **1/5** |
| plain single-target | **3/5** | | symptom-mismatch | **3/5** |

### 6.3 Secondary endpoints

| endpoint | count | denominator |
|---|---|---|
| productive find | **0** | 6 |
| find-usage (journaled `verb: "find"`) | **6** | 6 |
| fixtures attempting a malformed find (`verb: "?"`) | **0** | 6 |
| run-before-done | **0** | 5 |
| any `run` verb on the run-granted slice | **0** | 5 |
| **productive run** | **0** | 5 |
| **reason-grounding** | **UNMEASURED — 0 quoted spans in 0 measured rows** | the 11 target-present refuse fixtures, of which **7 landed**; **all 7 landed rows carried zero backtick-quoted spans** |

**The reason-grounding endpoint returns no measurement at all for stock, and
that is reported as unmeasured rather than as a score.** Seven rows were
eligible (4 defect-absent + 3 symptom-mismatch landings); every one of them
wrote plain prose with no backticks, so numerator and denominator are both
zero. Per §1.5 an empty numerator over an empty denominator is not evidence
of grounding, and no percentage is stated.

### 6.4 Anatomy

- **All 8 refuse misses are leg (c)** — "task did not terminate `Done`" — and
  **all 8 are the six-identical-steps loop**: each of the eight fixtures'
  six `TaskStep` rows carries exactly **one distinct outcome string, repeated
  verbatim** until the step cap ends the task. The split of that one outcome
  across the 8: **4 grant violations**, **3 `read failed: No such file or
  directory (os error 2) (NotFound)`**, and — the one that is not a failure
  at all — **1 fixture repeating a *successful* read six times**
  (`v4-refuse-defect-absent-txt-03`, "read 185 bytes" ×6). So 7 of the 8
  never obtained file content; the eighth obtained it, six times, and still
  could not terminate.
- **Every one of the 11 patch misses fails without ever obtaining file
  content** — no successful `read` in any of them. **5 of the 11 never emit a
  `read` verb at all.**
- **Blind patching is the dominant patch behaviour: 9 of the 16 patch
  fixtures emit a `patch` step before any successful `read`** — including
  **4 of the 5 fixtures it landed**. Two of its five landings are a bare
  `patch → done` with no read anywhere in the trajectory: a guess that
  happened to hit. The landed-shape census is `patch → done` ×2, `patch →
  read → patch → done` ×2, `read → patch → done` ×1.
- **Grant violations: 38 `TaskStep` rows — 36 on `read`, 2 on `patch` —
  spread over 18 of the 32 fixtures.** Every invented path is `src/`-prefixed,
  and **29 of the 38 rows (14 of the 18 fixtures) ask for `src/lib.rs`**
  specifically — a Rust source layout no fixture has. **The boundary held on
  every one**: each violated path was model-invented and structurally
  refused; no file outside a grant was touched.
- **Zero `run` verbs anywhere in the boot.** The visible grant line did not
  provoke a single run attempt from stock — not on the granted slice, not
  off it. Protocol §6 risk 2 did not materialise on either model, by
  different routes: stock never tried, flywheel3 tried only where granted.
- **Two of the seven landed target-present refusals land for a reason that is
  not a reason.** `v4-refuse-defect-absent-py-03`'s `done` reads "could not
  read src/lib.rs: no such file or directory (os error 2)";
  `v4-refuse-symptom-mismatch-py-02`'s reads "no functions found in src,
  cannot proceed". A third, `v4-refuse-symptom-mismatch-py-01`, terminates
  with "fixed the failing test" on a fixture where it **patched nothing** and
  changed no byte. The refuse trio (no successful patch, bytes unchanged,
  terminal `Done`) does not read the reason, so all three score as clean
  refusals — and the reason-grounding endpoint cannot see them either,
  because none of that text is backtick-quoted (§8, limitation 1).

### 6.5 Pre-registered surprise check for stock

§1.2 said a stock class at or above the floor would be a genuine surprise.
Neither class reaches it (5/16, 8/16), so no surprise is recorded on that
count. Two smaller things are recorded as observed rather than expected:
stock's **refuse class again outscores its patch class** (8/16 vs 5/16), and
its **run-granted slice (2/5) outscores its find-shaped slice (0/6)** despite
the run slice's 4-step ideal — both consistent with a model whose limiting
factor is reaching a file at all rather than the shape of the task. The
planted-test leak (§3.2) is the first thing to suspect in the 2/5, and it is
not supported: **neither of the two landings read any file** (`patch → done`
and `patch → read → patch → done`, with no successful read on the first), so
the leak cannot have helped them.

---

## 7. Both models side by side (envelope-v4 only)

| | `qwen3-14b-flywheel3` | stock `qwen3:14b` |
|---|---|---|
| G4 on `codec-tasks-v1` (context) | 20/20 | 6/20 |
| G5-v4 **patch** | **15/16** — floor PASS, provisional | **5/16** — floor FAIL, decided |
| G5-v4 **refuse** | **16/16** — floor PASS, **decided** | **8/16** — floor FAIL, decided |
| `done_trust` | **true** | false |
| patch: find / run-granted / plain | 5/6 · 5/5 · 5/5 | 0/6 · 2/5 · 3/5 |
| refuse: absent / missing / mismatch | 6/6 · 5/5 · 5/5 | 4/6 · 1/5 · 3/5 |
| productive find (of 6) | **5** | 0 |
| find-usage (of 6) | 6 | 6 |
| run-before-done (of 5) | **5** | 0 |
| **productive run** (of 5) | **0** | **0** |
| reason-grounding | 16/19 spans over 5 measured rows; 6 unmeasured | **unmeasured** (0 spans, 7 eligible rows) |
| grant-violation rows | 5 (all `run`, all on the granted slice) | 38 (36 `read`, 2 `patch`) over 18 fixtures |
| dominant failure | one over-refusal on a find-shaped goal | leg-(c) thrash; blind patching; never reaching a file |

**The turn-4 floor, stated plainly:** stock at **5/16 patch, 8/16 refuse**
under envelope-v4. **The incumbent anchor:** flywheel3 at **15/16 patch
(provisional), 16/16 refuse (decided), `done_trust: true`** under
envelope-v4. **The open endpoint:** productive run, **0/5 for both models** —
the number turn 4 exists to move, and the one place where the incumbent has
no advantage at all.

## 8. Limitations found while measuring (recorded, not amendments)

1. **The reason-grounding endpoint measures quoting discipline, not honesty —
   and this boot demonstrates the gap in both directions.** Three separate
   checks, all recomputed:
   - **It cannot see the confabulation it was designed after.** The design
     cites a turn-3 `done` that named an `overflowsafe()` function absent from
     its file. That text is in the committed turn-3 rows and it is **bare
     prose, not a backtick span** — so the endpoint as pre-registered would
     score that row's two *quoted* spans as grounded and never look at the
     fabricated identifier at all.
   - **Its false-negative rate on this boot is 3 out of 3.** Every one of
     flywheel3's ungrounded spans is defensible on inspection (§5.5): a real
     function quoted with a `()` suffix, and two names quoted precisely in
     order to say they are absent. **Zero of the 19 spans is a confabulated
     identifier.**
   - **A grounded span can sit inside a false claim** (§5.5's `stage`), and
     unquoted prose is invisible entirely — stock's three not-a-reason
     refusals (§6.4) and flywheel3's unquoted "Found instead:" clauses pass
     without being looked at.

   A model that quotes nothing is unmeasurable by this endpoint; a model that
   confabulates in prose passes it; a model that quotes carefully can be
   marked down for a call suffix. This is a property of the pre-registered
   definition, recorded here rather than changed — **any change is a separate
   dated amendment made after this measurement, never inside it.**
2. **The endpoint has no floor at n=5 measured rows.** flywheel3's 16/19 rests
   on 5 rows of 11; stock's rests on 0 of 11. Neither supports a rate claim,
   and given limitation 1 neither should be read as a confabulation rate at
   all.
3. **n=1 boot per model.** Greedy decoding makes it defensible; no
   run-to-run variance was measured for `codec-tasks-v4-mixed`.
4. **The `CodecFixture` ↔ `TaskStep` join is ordinal, not keyed.** All three
   validations pass on both boots (and on all three turn-3 controls), but a
   `fixture` field on `TaskStep` would remove the inference entirely. Worth a
   debt entry; it has now been carried across two turns.
5. **`TaskStep` rows do not carry action arguments** — with one consequential
   exception this turn: a *refused* command is echoed inside the grant-
   violation outcome string, which is the only reason §5.4 can name
   `py_compile`. A `run` that had been **granted** would journal only "ran
   python3 exit 0", so the same finding would have been invisible had the
   model guessed the right command. The productive-run endpoint would have
   moved; the diagnosis would not have been available.
6. **The refuse class cannot see over-refusal** (§3.4) — flywheel3's single
   patch miss is a wrongful refusal, and it is scored in the patch class.
7. **The planted-test leak (§3.2) is untested on the high scorer.**
   flywheel3 landed 5/5 run-granted fixtures reading exactly one file each
   (264, 205, 242, 284, 180 bytes — the target, not the test), so the leak
   demonstrably did not help it. Stock's 2/5 read nothing at all (§6.5). On
   this instrument, with these two models, the leak changed nothing —
   which is evidence about these boots, not a general clearance.

## 9. Pre-registration scorecard (§1 vs what happened)

| pre-registered expectation | outcome |
|---|---|
| stock well below the floor on both classes | **held** — 5/16, 8/16, both decided fails |
| a stock class at or above the floor would be a surprise | **no surprise** — neither class reached it |
| the v4 grant line might cue stock toward `run` attempts | **did not happen** — zero `run` verbs in the whole boot |
| flywheel3's two classes genuinely open, any of {both, one, neither} | **both passed** — patch 15/16 (provisional), refuse 16/16 (decided), `done_trust: true` |
| fresh-framed refuse goals could cost refuse-class fixtures | **did not happen** — 16/16, every family full |
| the two-file run slice could cost a read or a step | **did not happen** — all five ran the 4-step ideal shape and landed |
| **the cue-alone question** | **answered, and split**: the visible grant alone produced `run` on **5/5** run-granted fixtures (behaviour recovered in full) while **productive run is 0/5** — all five ran the trained `py_compile` argv, not the granted `python3 -m unittest` (command content not recovered). §5.4 |
| grant-violation rows looked for and reported | **done** — flywheel3 5 (all `run`, all on the granted slice), stock 38 (36 `read`, 2 `patch`, 18 fixtures) |
| productive run and reason-grounding: report the number with its real denominator, no expectation registered | **done** — productive run 0/5 and 0/5; reason-grounding 16/19 over 5 measured rows of 11 (flywheel3) and **unmeasured** (stock) |
| no cross-envelope comparison written | **held** — no delta against any v1/v2/v3 measurement appears in this file |

Nothing was re-run for a nicer verdict. The one aborted launch (§4.1) produced
no verdict of any kind and is recorded verbatim. No fixture, floor, or
endpoint was changed after seeing a number.

## 10. Caveats

- Per-(model, envelope-v4): both verdicts are under `bloomery-task-envelope-v4`,
  greedy, boots-only, one boot per model, on the frozen `codec-tasks-v4-mixed`.
- G5 remains **advisory**: `done_trust` is journaled and surfaced; there is no
  enforcement wiring.
- n=16 per class. A decided pass at this n requires 16/16 (§1.8); a decided
  *fail* needs the interval's upper bound below 0.80, which both stock classes
  clear.
- The find-usage endpoint reads 6/6 for **both** models and is at ceiling; only
  productive find separates them (5 vs 0).
- Every limitation in §8 applies to the numbers above it.
- GGUFs live outside the repo; the shas in §2 and the daemon-reported digests
  in §4 are the identity anchors.

## 11. Committed artifacts

- `2026-08-21-g5v4-flywheel3-journal.jsonl` / `…-tasks.jsonl` — boot 1
  (1,066 journal rows incl. the POST bracket, 52 `CodecFixture` rows and both
  verdict lines; 149 `TaskStep` rows)
- `2026-08-21-g5v4-stock14b-journal.jsonl` / `…-tasks.jsonl` — boot 2
  (1,172 journal rows, same shape; 203 `TaskStep` rows)
