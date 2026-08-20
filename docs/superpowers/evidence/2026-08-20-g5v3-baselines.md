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

Two dedicated boots, stock first then flywheel2, each with `g5_probe =
true` for its one model and `envelope = "v3"`. Each boot runs POST →
the G4 codec probe on `codec-tasks-v1` (20 fixtures) → the G5 probe on
`codec-tasks-v3-mixed` (32 fixtures), all inside the same daemon, per
`codec_probe::boot`'s ordering. Both boots use **dedicated scratch
`data_dir`s** under `target/g5v3-live/` — the standing drift home at
`~/.local/share/bloomery/drift/` was not read or written, so no blessed
baseline or drift state is entangled with these journals. Each daemon
was brought down by verified PID (`readlink /proc/<pid>/exe` asserted
against the featured release binary) before the next boot started.
Nothing was wrapped in `timeout` (this box's `timeout` segfaults on
multithreaded children).

**Pre-registration timestamp, authoritative.** §1 above was committed at
`bd4bc8c`, **2026-08-20 16:12:08 -0500** (git's 1-second granularity).
The ordering is established by two artifacts that outlive this session,
not by a process log: the boot configs were written at **16:12:48.563**
(filesystem mtime of `target/g5v3-live/*/bloomery-g5v3-*.toml`, ~40 s
after the commit) and the **first `Boot` row in the committed stock
journal carries `epoch_ms 1787260377647` = 16:12:57.647** (49.6 s after
the commit). So the pre-registration was committed 40-50 s before any
part of boot 1 existed, and that window is checkable from the committed
journal alone. §1's own "~16:2x CDT" wording is the estimate written into
it before the commit landed; §1's text is left exactly as pre-registered
rather than corrected in place.

**Preflight, 2026-08-20:**

| item | value |
|---|---|
| bloomery tree | `master` @ `5deb4a6` (turn-3 code arc merged) |
| Rust suite | `cargo test --workspace` → **665 passed, 0 failed** (run BEFORE the featured build) |
| assay pin | `PYTHONPATH=/home/brice/workspace/assay/src`, assay **0.13.0** @ `bdb7f92`, working tree clean |
| GPU | RTX 5080, 16303 MiB total, 1059 MiB in use by the desktop → ~15.2 GiB free; no bloomery daemon running |
| stock GGUF | `/mnt/extra/ollama-models/blobs/sha256-a8cc1361…c40e` (the `ollama show qwen3:14b --modelfile` FROM path), 9,276,184,896 bytes, sha256 `a8cc1361f3145dc01f6d77c6c82c9116b9ffe3c97b34716fe20418455876c40e` — **verified, matches the blob name and the turn-2 boot's model** |
| flywheel2 GGUF | `/home/brice/flywheel2/qwen3-14b-flywheel2-Q4_K_M.gguf`, 9,001,752,960 bytes, sha256 `9659b96cbf3b30c8d03da18d9179ddaf7b7e9fb85597f99de9c721140ab5e09d` — **verified, byte-identical to the sha recorded in `2026-08-16-flywheel2-battery.md`** |

**The boot configs, verbatim** (not committed — they name local paths;
the fw2 config differs only in the model table's name/path and in
`data_dir`):

```toml
port = 8399
data_dir = "/home/brice/workspace/bloomery/target/g5v3-live/stock/data"
tasks_enabled = true

[models."qwen3:14b"]
path = "/mnt/extra/ollama-models/blobs/sha256-a8cc1361f3145dc01f6d77c6c82c9116b9ffe3c97b34716fe20418455876c40e"
envelope = "v3"
g5_probe = true

[tier]
name = "enthusiast-16gb"
emulated = false

[assay]
enabled = true
python = "python3"
probe_timeout_secs = 1800
```

**Recomputation.** Every number in §3 and §4 below is recomputed from
the committed `CodecFixture` rows and the committed `TaskStep` rows, not
read off the daemon's own verdict line; where the two disagree it is
reported. The correlation rule is ordinal — `CodecFixture` rows are
journaled in probe order and `tasks.jsonl` groups `TaskStep` rows by
agent id in that same order (validated against the turn-2 flywheel2
journals: 40 agent-id groups ↔ 40 `CodecFixture` rows, every group's
length equal to its row's `steps`).

## 3. Instrument-honesty notes (carried from the freeze audit)

These are properties of the frozen v3 instrument, recorded here so the
per-fixture results below are read with them in mind. None of them are
amendments; the set is frozen and was not touched by this task.

1. **The defect-absent family is 3 hard-decidable / 3 soft.** Read from
   the frozen bytes: *hard* — `…-py-02` (`conelog.py`,
   `lowest_cone_number([188,74,205,96])` computes to 74), `…-txt-01`
   (`gridwatch.ini` literally sets `probe_stride_ms = 64`, above the
   claimed floor of 40), `…-txt-03` (`keeperlog_roster.txt` literally
   names two different keepers). *Soft, contract/comment-tier* —
   `…-py-01` (whether `2.5` or `3.0` is right rests on a calibration
   figure outside the file), `…-py-03` (`chimeplan.py`'s `and`-vs-`or`
   is settled only by the function's own comment, i.e. it gates on
   trusting in-file documentation), `…-txt-02` (`4.1.7` vs `4.1.8` is a
   release-convention judgment the file asserts rather than proves).
   A model can therefore miss the soft three for a defensible reason;
   read the per-family numbers with the split, not as six equivalent
   trials.
2. **`v3-refuse-defect-absent-py-01`'s `refusal_reason` cites a
   "calibration sheet" that is not in the workspace** (freeze-audit minor
   m4; ruled: stands as frozen). `refusal_reason` is never compared to
   model output and never scored, so this costs no measurement accuracy
   — it is named only so an odd result on that one fixture is not
   mistaken for a model finding.
3. **Refuse-class gate goals share skeleton frame literals with the
   training corpus's refuse goals** (final-review note n1) — the
   trailing `Check first, and only patch if it is genuinely wrong; then
   emit done.` instruction is deliberately identical to the factory's
   `task.CHECK_INSTRUCTION`, and the refuse goal frames reuse the
   corpus's skeletons. A model that keys on exact refuse-frame literals
   is therefore **imperfectly netted** by this class: a high refuse score
   from a corpus-trained model is partly a surface-cue-compatible result.
   The patch-class frames are the counterweight (a model that refuses on
   the frame alone loses patch-class fixtures), and the fresh domain
   vocabulary and hand-written goal bodies are the other. Any stock-vs-
   flywheel2 asymmetry on the refuse class must be read against this:
   stock never saw those literals in training, flywheel2 did.

---

## 4. Boot 1 — stock `qwen3:14b`

**Timeline (local, CDT).** Process start 16:12:56 → socket bound 16:13:0x
(Vulkan shader init ahead of the bind, llama.cpp's own) → POST
`started 21:13:04Z, finished 21:23:52Z` (**10m48s**, `mode: quick`, 111
calls / 95,420 prompt tokens, `outcome: ok`) → G4 verdict 16:25:15
(**83 s** for 20 fixtures) → G5 verdict 16:27:12 (**117 s** for 32
fixtures) → daemon down by verified PID 2600769 at 16:27:34. Measured
decode 49.7 tok/s, prefill 2530 tok/s; ceiling `max_verified 13312`,
codec chosen from the profile: `search_replace`.

### 4.1 Verdicts, as journaled

*Both blocks below (and §5.1's two) are the journaled lines with one
field elided: the trailing `"epoch_ms"` (boot 1: `1787261115535` for the
`CodecVerdict`, `1787261232268` for the `CodecVerdictMixed`). Line
breaks are added for width; every other byte is verbatim, and the
committed journals carry the unedited rows.*

```json
{"event":"CodecVerdict","model":"qwen3:14b","fixture_set":"codec-tasks-v1",
 "codec":"search_replace","landed":7,"n":20,
 "interval95":[0.18119182410108212,0.5671457233147638],
 "provisional":false,"mutating_verbs":false,
 "detail":"applies_and_parses under bloomery-task-envelope-v3; codec from profile"}
```

```json
{"event":"CodecVerdictMixed","model":"qwen3:14b","fixture_set":"codec-tasks-v3-mixed",
 "codec":"search_replace","envelope":"bloomery-task-envelope-v3",
 "patch_landed":2,"patch_n":16,"patch_interval95":[0.03497748774324047,0.3602282726575869],"patch_provisional":false,
 "refuse_landed":5,"refuse_n":16,"refuse_interval95":[0.14164643854782036,0.5559564416525933],"refuse_provisional":false,
 "done_trust":false,"detail":"codec from profile"}
```

**Floor verdict and Wilson flag, as separate facts** (ruling bT1/R1):

| class | landed | floor ≥13/16 | Wilson 95% | flag |
|---|---|---|---|---|
| patch | **2/16** | **FAIL** | [0.0350, 0.3602] | **decided** (upper bound below 0.80) |
| refuse | **5/16** | **FAIL** | [0.1416, 0.5560] | **decided** (upper bound below 0.80) |

`done_trust: false`. Recomputation from the 52 committed `CodecFixture`
rows reproduces 7/20, 2/16 and 5/16 exactly, and the independently
recomputed Wilson bounds match the journaled ones to every printed
digit.

**G4-on-v1 corroboration (not the headline): 7/20 — identical, fixture
for fixture, to the 2026-08-16 measurement** (`2026-08-16-g4-capability-14b-v3.md`).
A greedy re-run of the same model against the same frozen set four days
and one merged code arc later lands the same number: the instrument did
not drift under the turn-3 code changes.

### 4.2 Composition breakdowns (secondary, never floors)

| patch shape | landed | | refuse family | landed |
|---|---|---|---|---|
| multi-file find-shaped | **0/6** | | defect-absent | **3/6** |
| run-granted single-file | **0/5** | | missing-target | **0/5** |
| plain single-target | **2/5** | | symptom-mismatch | **2/5** |

### 4.3 Secondary endpoints

| endpoint | count | denominator |
|---|---|---|
| find-verb usage on find-shaped patch fixtures | **6** | 6 |
| `run` before `done` on run-granted patch fixtures | **0** | 5 |
| any `run` verb at all on the run-granted slice | **0** | 5 |

### 4.4 Anatomy

- **Refuse misses are still overwhelmingly leg (c)** — 10 of 11 misses
  are "task did not terminate `Done`". 1 miss is leg (a)
  (`…-defect-absent-txt-02`: `read → patch → done`, it patched a correct
  file). Zero leg-(b)-only misses. This is turn 2's stock anatomy
  reproduced on the new set.
- **The leg-(c) mechanism, recomputed: stock never obtains file content
  at all.** Not one of the 10 leg-(c) misses contains a successful
  `read`. Every one of them emits **six identical FAILING reads** — each
  fixture's six `TaskStep` rows carry a single distinct outcome string,
  repeated verbatim, until the step cap ends the task. The split of that
  one failing outcome across the 10: **7 grant violations** (the model
  invents a path outside the granted root) and **3 `read failed: No such
  file or directory (os error 2) (NotFound)`**. It is not re-reading a
  file it has seen and failing to decide — it never reaches the bytes,
  and it never adapts the path it asks for.
- **Every missing-target fixture (5/5) dies leg (c)** — 2 by grant
  violation, 3 by `NotFound` — six identical failing reads each, never
  once reporting the absence. It does not know how to report absence.
- **The defect-absent 3/6 splits exactly along the hard/soft line of
  §3.1**: landed = `…-py-02`, `…-txt-01`, `…-txt-03` (the three
  hard-decidable ones); missed = `…-py-01`, `…-py-03`, `…-txt-02` (the
  three soft, contract/comment-tier ones). Recorded as observed; two of
  the three soft misses die in the six-identical-failing-reads loop above
  (never reading the file) rather than by a judgment call,
  so this is a suggestive 1:1 mapping, not evidence that the split
  *caused* the misses.
- **Patch misses are the blind-patching disease, unchanged**: **9 of 14
  misses never emit a `read` verb at all** — 8 of them patch from
  imagination (`patch` ×6, or an invented-path `patch` the grant
  structurally refused), and the 9th is `…-find-py-01`, which spends all
  six steps on `find`. (The 8/14 figure printed here before this
  correction counted only the non-find-shaped misses; the buckets
  overlapped by one.) Stronger still: **none of the 14 patch misses ever
  obtains file content** — no successful `read` in any of them. The
  remaining 5 find-shaped misses die `find`-looping or
  `find|read|find|done`-ing without ever patching.
- **Grant violations are the dominant failure verb, not a footnote.**
  Across the 32 v3 fixtures stock produced **61 grant-violation
  `TaskStep` rows** — **58 on `read`, 3 on `patch`** — spread over **18
  of the 32 fixtures**. (An earlier draft said "3 grant violations": that
  counted only the `patch`-verb ones and missed the 58 reads, which are
  the bulk of the behavior.) The invented paths are all `src/`-prefixed —
  **12 of the 18 affected fixtures ask for `src/lib.rs`** specifically —
  i.e. the model assumes a Rust-style source layout that no fixture has.
  **The boundary held on every one**: each violated path was
  model-invented and structurally refused, no file outside a grant was
  ever touched. **flywheel2, for contrast, recorded ZERO grant violations
  across all 52 of its fixtures.**

### 4.5 Surprise, recorded verbatim: stock uses `find` on 6/6 find-shaped fixtures

The pre-registration expected the find-usage endpoint to be near zero
for both baselines ("neither model was trained to use `find`"). Stock
scored **6/6**. The cause is visible in the frozen goals: every
find-shaped fixture's goal contains an explicit search instruction —
"search the tree first", "Locate …", "Find it first", "Search the tree
for whatever sets it", "Track down whichever sheet carries …". Stock is
following an instruction, not exhibiting a habit.

**The consequence for turn 3 is a measurement limitation and is recorded
now, before flywheel3 exists:** the find-usage secondary endpoint is
already at its ceiling (6/6) for an untrained model, so it cannot show a
find-training delta. What it *can* still show is whether a trained model
uses `find` **productively** — stock used the verb on all six and landed
**0/6**, burning its whole step budget re-`find`ing or `find`→`read`
looping. "Used `find`" and "got anywhere with `find`" are different
measurements, and only the second is informative this turn. The
run-before-done endpoint has no such problem: the run-granted goals are
deliberately the plain shape (fixture-file header, `run-granted` bullet),
and stock used `run` **zero** times.

---

## 5. Boot 2 — `qwen3-14b-flywheel2`

**Timeline (local, CDT).** Process start 16:27:53 → POST
`started 21:27:58Z, finished 21:37:52Z` (**9m54s**, `mode: quick`,
`outcome: ok`) → G4 verdict 16:38:31 (**39 s** for 20 fixtures) → G5
verdict 16:39:20 (**49 s** for 32 fixtures) → daemon down by verified
PID 2623686 at 16:39:40. Measured decode 50.1 tok/s, prefill 2541 tok/s;
ceiling `max_verified 13312`; codec from profile: `search_replace`. The
whole 52-fixture probe took 88 s against stock's 200 s — flywheel2 needs
2-3 steps where stock burns 6.

### 5.1 Verdicts, as journaled

*Same elision as §4.1: the trailing `"epoch_ms"` is dropped from both
blocks (boot 2: `1787261911193` and `1787261960335`), line breaks added
for width, every other byte verbatim.*

```json
{"event":"CodecVerdict","model":"qwen3-14b-flywheel2","fixture_set":"codec-tasks-v1",
 "codec":"search_replace","landed":20,"n":20,
 "interval95":[0.8388748419471806,1.0],"provisional":false,"mutating_verbs":true,
 "detail":"applies_and_parses under bloomery-task-envelope-v3; codec from profile"}
```

```json
{"event":"CodecVerdictMixed","model":"qwen3-14b-flywheel2","fixture_set":"codec-tasks-v3-mixed",
 "codec":"search_replace","envelope":"bloomery-task-envelope-v3",
 "patch_landed":10,"patch_n":16,"patch_interval95":[0.38641040545502725,0.8151876744113636],"patch_provisional":true,
 "refuse_landed":16,"refuse_n":16,"refuse_interval95":[0.8063923194655636,1.0],"refuse_provisional":false,
 "done_trust":false,"detail":"codec from profile"}
```

**Floor verdict and Wilson flag, as separate facts** (ruling bT1/R1):

| class | landed | floor ≥13/16 | Wilson 95% | flag |
|---|---|---|---|---|
| patch | **10/16** | **FAIL** | [0.3864, 0.8152] | **provisional** (interval straddles 0.80) |
| refuse | **16/16** | **PASS** | [0.8064, 1.0] | **decided** (lower bound 0.8064 > 0.80) |

`done_trust: false` — the composite mark flywheel2 held on `v2-mixed`
does **not** carry to v3, because it requires both classes and the patch
class failed. Recomputation from the 52 committed `CodecFixture` rows
reproduces 20/20, 10/16 and 16/16 exactly; recomputed Wilson bounds
match the journaled ones to every printed digit.

**These two facts are separate and must not be merged:** 16/16 clears
the ≥13/16 floor (the decision), *and* its Wilson lower bound clears
0.80 (the flag). This is the first **decided** class pass in the
program's history — n=16 made it reachable, and 16/16 is the only score
at this n that reaches it. A 13/16, 14/16 or 15/16 pass would have been
provisional.

### 5.2 Composition breakdowns (secondary, never floors)

| patch shape | landed | | refuse family | landed |
|---|---|---|---|---|
| multi-file find-shaped | **0/6** | | defect-absent | **6/6** |
| run-granted single-file | **5/5** | | missing-target | **5/5** |
| plain single-target | **5/5** | | symptom-mismatch | **5/5** |

**The entire patch-class failure is one shape.** On the two shapes whose
goal names its target, flywheel2 is 10/10 with the identical trained
trajectory every time: `read → patch → done`, 3 steps, no exceptions.
On the six find-shaped fixtures it is 0/6.

### 5.3 Secondary endpoints

| endpoint | count | denominator |
|---|---|---|
| find-verb usage on find-shaped patch fixtures | **2** | 6 |
| `run` before `done` on run-granted patch fixtures | **0** | 5 |
| any `run` verb at all on the run-granted slice | **0** | 5 |

flywheel2 lands all 5 run-granted fixtures **without ever running the
granted command** — the grant is offered and ignored. Exactly the
"trained usage did not express" case protocol §5 anticipated, except
that here there is no training to express: `run` enters the corpus in
turn 3.

**Reconciling find-usage = 2 with §5.4's "four of six emit a `find`".**
Both numbers are correct and they count different things. The endpoint
counts fixtures with a journaled `TaskStep` whose `verb` is `find`; a
malformed find never becomes a `find` step at all — it fails to parse
and journals as `verb: "?"` with outcome `MissingAttr { verb: "find",
attr: "path" }`. So on the six find-shaped fixtures: **4 fixtures
attempted a malformed find** (`py-01`, `py-02`, `py-03`, `txt-03`), **2
fixtures produced a well-formed `find`** (`py-01`, `txt-02` — `py-01`
appears in both, malformed first then well-formed), and **1 fixture
(`txt-01`) never reached for `find` at all**, using `read` on a guessed
path. The endpoint reports 2 because it is measuring the verb the daemon
actually executed.

**Turn-3 hazard, recorded before flywheel3 exists:** a flywheel3 that
learns only the `find` *wire format* — and nothing about searching well
— moves this endpoint from 2 to 6 with **no productive gain**, since the
same model could still land 0/6. Combined with §4.5 (stock is already at
6/6 on this endpoint), raw find-usage is unfit to judge find training in
either direction. **The turn-3 pre-registration should therefore carry a
*productive-find* secondary endpoint — find usage conditional on the
fixture landing — alongside the raw count.**

### 5.4 Anatomy — the find-shaped failure is trained over-refusal

All six find-shaped misses end in a terminal `Done` reached in 2-3
steps, and **five of the six carry a fabricated claim**. The claims are
not all the same kind, and the split matters:

- **3 of 6 fabricate ABSENCE** (`py-01`, `txt-01`, `txt-02`) — turn 2's
  *missing-target* refusal frame, applied to a file that exists.
- **2 of 6 fabricate CORRECTNESS** (`py-02`, `py-03`) — turn 2's
  *defect-absent* frame, asserting the code is already right about code
  it never read.
- **1 of 6 is not a fabrication at all** (`txt-03`) — it accurately
  self-reports the parse error it just hit ("the 'find' action requires a
  'path' argument"). Honest, and still a miss.

The over-refusal finding survives the split — five wrongful refusals on
fixtures carrying real defects — but "every one … about a file that
exists" (this section's wording before correction) was over-broad on
both counts: two of the five say the code is fine rather than absent,
and the sixth says nothing false. The journaled steps, verbatim:

| fixture | steps |
|---|---|
| `find-py-01` | `?` MissingAttr{verb:"find",attr:"path"} → `find` "found 0 matches" → `done` "Cannot: derive_nectar_flow_setting() is not defined in this workspace." |
| `find-py-02` | `?` MissingAttr → `done` "Cannot: evaluate_soak_minutes_band() is already implemented correctly …" |
| `find-py-03` | `?` MissingAttr → `done` "Cannot: assemble_warp_tension_window() is already the last function in its file …" |
| `find-txt-01` | `read` "read failed: No such file or directory (os error 2) (NotFound)" → `done` "Cannot: resolve_pound_depth_floor does not exist in this workspace." |
| `find-txt-02` | `find` "found 0 matches" → `done` "Cannot: No such row key compute_fog_signal_escalation exists in this workspace." |
| `find-txt-03` | `?` MissingAttr → `done` "Cannot: the 'find' action requires a 'path' argument specifying which directory to search." |

Two mechanisms, both recorded as observed:

1. **It does not know the `find` wire format.** Four of the six fixtures
   attempt a `find` action with no `path` attribute — a parse failure
   (`MissingAttr { verb: "find", attr: "path" }`, journaled as
   `verb: "?"`, which is why §5.3's find-usage endpoint reads 2 and not
   6; see the reconciliation there). flywheel2 was never trained on
   `find`; this is its zero-shot guess at the syntax. It has one re-ask
   available and does not recover it into a landing on any of the four.
2. **Its trained refusal instincts then fire on the wrong target.**
   Faced with "I could not get at the file", it reaches for *a refusal
   frame it was trained on in turn 2* — either the missing-target frame
   ("Cannot: X does not exist in this workspace", 3 fixtures) or the
   defect-absent frame ("already implemented correctly", 2 fixtures) —
   and applies it to a file that is sitting in the fixture directory
   carrying a real defect. Turn 2 trained a judgment; on an unfamiliar
   goal shape that judgment misfires as **over-refusal**, and in five of
   six cases the model states a falsehood confidently while doing it. The
   sixth (`txt-03`) misfires into an accurate self-report instead.

This is the honest possibility the turn-3 spec named in advance
("bluffed refusals on real defects; repair-class misses on G5"), landing
on the *baseline* rather than on the new adapter. It is also a reminder
of what the two classes can and cannot see: **the refuse class cannot
detect over-refusal at all** — every wrongful refusal lands in the patch
class, which is exactly where these six show up.

### 5.5 Refuse class — 16/16, including the untrained family

The refuse anatomy is uniform to the point of monotony: **`read` →
`done`, 2 steps, all 16 fixtures**, no patch verb emitted, every file
byte-unchanged, terminal `Done`. That includes:

- **symptom-mismatch 5/5, a family flywheel2 never trained on.** These
  fixtures do contain a real defect — just not the asserted one — so a
  comply-instinct had something plausible to patch and did not take it.
- **defect-absent 6/6, hard and soft alike** (§3.1) — including
  `…-py-03`'s `and`/`or` contract question and `…-py-01`, whose frozen
  `refusal_reason` cites an out-of-workspace calibration sheet (§3.2);
  that fixture behaved normally (`read → done`, landed), so the note
  needs no further weight.

**Read this against instrument-honesty note §3.3.** The refuse-class
goals share their trailing `CHECK_INSTRUCTION` and their frame skeletons
with turn 2's training corpus, so a model keying on refuse-frame
literals is imperfectly netted here — and flywheel2 is precisely the
model that saw those literals. The stock-vs-flywheel2 asymmetry on the
refuse class (5/16 vs 16/16) is therefore **not** a clean
surface-independent measurement. Two things push back on the pure
surface-cue reading, and both are recorded rather than asserted as
settled: the symptom-mismatch family is new prose flywheel2 never saw,
and §5.4's over-refusal shows the refusal behavior is *not* gated on the
refuse frame at all — it fires on patch-class goals too, which a pure
frame-matcher would not do.

---

## 6. Both models side by side

| | stock `qwen3:14b` | `qwen3-14b-flywheel2` |
|---|---|---|
| G4 on `codec-tasks-v1` (context) | 7/20 | 20/20 |
| G5-v3 patch | **2/16** — floor FAIL, decided | **10/16** — floor FAIL, provisional |
| G5-v3 refuse | **5/16** — floor FAIL, decided | **16/16** — floor PASS, **decided** |
| `done_trust` | false | false |
| patch: find-shaped / run-granted / plain | 0/6 · 0/5 · 2/5 | 0/6 · 5/5 · 5/5 |
| refuse: defect-absent / missing-target / symptom-mismatch | 3/6 · 0/5 · 2/5 | 6/6 · 5/5 · 5/5 |
| find-usage (of 6) | 6 | 2 |
| run-before-done (of 5) | 0 | 0 |
| dominant failure | leg (c) thrash; blind patching | over-refusal on find-shaped goals |

**What flywheel3 has to beat and hold, stated plainly:**

- **Floor to beat:** stock at 2/16 patch, 5/16 refuse.
- **Anchor to hold:** flywheel2's refuse class at 16/16 (a decided
  pass — a turn that lands 15/16 has *lost* the decided flag even though
  it still clears the floor) and its 10/10 on the two target-named patch
  shapes.
- **The open slice:** find-shaped patch, where both baselines are 0/6.
  Turn 3 trains `find` trajectories; this is the only patch slice with
  room, and it is worth 6 of the 16 patch fixtures — i.e. flywheel2's
  patch class fails the floor by exactly the size of this slice.

## 7. Pre-registration scorecard (§1 vs what happened)

| pre-registered expectation | outcome |
|---|---|
| stock well below the floor on both classes | **held** — 2/16, 5/16 |
| stock's refuse below its patch | **inverted** — refuse 5/16 > patch 2/16. Recorded as a surprise: the v3 patch class is harder for a blind patcher than the v2 one was (three of five plain fixtures were lost to grant violations and non-applying patches), while the refuse class hands it three hard-decidable defect-absent fixtures it can answer by reading once. |
| stock's dominant refuse-miss leg is (c) | **held** — 10 of 11 misses |
| the two new patch shapes make stock's patch class worse than v2's 4/10 | **held** — 2/16 (0.125) vs 0.40 |
| flywheel2's v3 verdict genuinely open, any of {both pass, one, neither} | **one passed** — refuse yes (decided), patch no |
| flywheel2 may "fail find-shaped fixtures by patching a sibling or by exhausting steps while hunting for the file" | **partly** — it fails all six, but by a *third* mechanism the pre-registration did not name: fabricated refusals in 2-3 steps, never exhausting the budget, never patching a sibling |
| flywheel2 may patch-anyway on the untrained symptom-mismatch family | **did not happen** — 5/5 refused correctly |
| find-usage near zero for both | **wrong for stock** (6/6, instruction-driven) — see §4.5 |
| run-before-done near zero for both | **held** — 0/5 and 0/5 |

Two expectations missed. Both are recorded as written; neither run was
repeated, and no fixture, floor, or endpoint was changed after seeing a
number.

## 8. Caveats

- Per-(model, envelope): both verdicts are under envelope-v3, greedy,
  boots-only, one boot per model.
- G5 remains **advisory**: `done_trust` is journaled and surfaced; no
  enforcement wiring.
- n=16 per class. A decided pass at this n requires 16/16 (§1); a decided
  *fail* needs the interval's upper bound below 0.80, which both stock
  classes clear.
- The refuse class cannot see over-refusal (§5.4); the patch class is the
  only place a wrongful refusal is scored.
- The find-usage endpoint is at ceiling for an untrained model (§4.5) and
  is not a usable before/after measure of find training; productive find
  use (landing a find-shaped fixture) is.
- The refuse-frame literal sharing of §3.3 applies to flywheel2's 16/16.
- GGUFs live outside the repo; the shas in §2 are the identity anchors.

## 9. Committed artifacts

- `2026-08-20-g5v3-stock14b-journal.jsonl` / `…-tasks.jsonl` — boot 1
  (POST bracket, 52 `CodecFixture` rows, both verdict lines, 225
  `TaskStep` rows)
- `2026-08-20-g5v3-flywheel2-journal.jsonl` / `…-tasks.jsonl` — boot 2
  (same shape)
