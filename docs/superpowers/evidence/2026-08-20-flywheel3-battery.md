# Flywheel turn 3 — the battery: `qwen3-14b-flywheel3` PASSES both legs

**Date:** 2026-08-20 (G4 verdict 19:46:33 CDT, G5 verdict 19:58:52 CDT).
**Status:** measured; the pre-registered decision applied — **SUCCESS on both
legs.** G4 **20/20**; G5 **patch 15/16** (floor PASS, provisional) and
**refuse 16/16** (floor PASS, decided); **`done_trust: true`** — the first
done-trust mark earned at n=16 per class, and the first time the program has
measured a model that both repairs a defect it has to *find* and declines one
that is not there.
**Pre-registration:** `2026-08-20-flywheel3-preregistration.md` (committed
before training, with its dated bT10/R1 addendum; unamended after any number
was seen). Envelope-v3, greedy, one boot per leg, nothing re-run.

The measured hole this turn targeted — flywheel2's **0/6 on the find-shaped
patch slice**, the entire reason its patch class failed the v3 floor — closed
to **5/6**, by navigation rather than by wire-format mimicry. The
pre-registered **productive-find** endpoint, which exists precisely to tell
those two apart, reads **5/6** against a measured **0/6 for both baselines**.

---

## 1. Verdicts

**Leg 1 — G4 on `codec-tasks-v1`: 20/20.** Pass floor was ≥16/20. Wilson 95
[0.8389, 1.0], `provisional: false` — a decided pass. This was the
pre-registered kill leg ("the failure this turn most plausibly causes, with
two new trajectory shapes in the repair slice"); it cost **zero** repair.

*The two verdict blocks below are the journaled lines with one field elided —
the trailing `"epoch_ms"` (`1787273193795` for boot 1's `CodecVerdict`,
`1787273932294` for boot 2's `CodecVerdictMixed`). Line breaks are added for
width; every other byte is verbatim, and the committed journals carry the
unedited rows.*

```json
{"event":"CodecVerdict","model":"qwen3-14b-flywheel3","fixture_set":"codec-tasks-v1",
 "codec":"search_replace","landed":20,"n":20,
 "interval95":[0.8388748419471806,1.0],"provisional":false,"mutating_verbs":true,
 "detail":"applies_and_parses under bloomery-task-envelope-v3; codec from profile"}
```

**Leg 2 — G5 on `codec-tasks-v3-mixed`: patch 15/16, refuse 16/16.** Pass
floor was ≥13/16 per class. Both classes clear it. `done_trust: true`.

```json
{"event":"CodecVerdictMixed","model":"qwen3-14b-flywheel3","fixture_set":"codec-tasks-v3-mixed",
 "codec":"search_replace","envelope":"bloomery-task-envelope-v3",
 "patch_landed":15,"patch_n":16,"patch_interval95":[0.7167126242970107,0.9888806552353575],"patch_provisional":true,
 "refuse_landed":16,"refuse_n":16,"refuse_interval95":[0.8063923194655636,1.0],"refuse_provisional":false,
 "done_trust":true,"detail":"codec from profile"}
```

### Floor verdict and Wilson flag, as separate facts (rulings bT1/R1, bT10/R1)

The floor is the decision. The flag is an independent property of the
interval. They are reported apart, and no score here is called decided *by
construction*.

| class | landed | floor ≥13/16 | Wilson 95% | flag |
|---|---|---|---|---|
| patch | **15/16** | **PASS** | [0.7167, 0.9889] | **provisional** (interval straddles 0.80) |
| refuse | **16/16** | **PASS** | [0.8064, 1.0] | **decided** (lower bound 0.8064 > 0.80) |

A 15/16 pass clears the floor **and is still provisional** — that is not a
contradiction, it is the pinned n=16 property: only 16/16 reaches a decided
pass at this n. And the refuse class **holds the decided flag flywheel2
earned** (16/16 → 16/16): the pre-registration named a 13-15/16 refuse score
as a regression on the incumbent anchor even though it would not be a kill;
that regression did not happen.

`done_trust: true` because both class decisions cleared — the composite mark
flywheel2 held on `v2-mixed` at n=10 and **lost** on v3, now earned on v3 at
n=16.

**Recomputation.** Every number in this document is recomputed from the
committed `CodecFixture` rows and the committed `TaskStep` rows, not read off
the daemon's own verdict line. The recomputation reproduces 20/20, 15/16 and
16/16 exactly, and independently recomputed Wilson bounds match the journaled
ones to every printed digit.

One apparent exception is not one, and the cause is worth stating exactly: the
recomputation's 16/16 **upper** bound prints `1.0000000000000002` against the
journaled `1.0`. That is **not** a float-ordering difference between two
computations of the same quantity — `wilson95`
(`crates/bloomery-core/src/stats.rs:34`) **clamps the upper bound with
`.min(1.0)`**, so the daemon computes the identical number and then clamps it.
The journaled `1.0` is the clamped value of the recomputation's own result, and
the two agree exactly. (The lower bound, which is the only one the flag rests
on, is `0.8063923194655636` in both.)

---

## 2. Identity chain

Everything the verdict rests on, with the check that was actually run.

| artifact | value | check |
|---|---|---|
| bloomery tree | `master` @ **`9575b21`** (the bT10/R1 step-0 commit, committed before training started) | `git log -1` |
| featured binary | `target/release/bloomery-daemon`, built 2026-08-20 16:12 | `cargo build --release -p bloomery-daemon --features vulkan` re-run 19:34 → `Finished` in 0.47 s, i.e. the fingerprint was already current and **no rebuild was needed**; the last commit touching `crates/`/`Cargo.*` is `e1d5e38` at 15:19:54, before the build. `cargo test` was **not** run after it. |
| daemon PID, boot 1 | 2899457 | `readlink /proc/2899457/exe` = the featured binary, asserted before the kill |
| daemon PID, boot 2 | 2915928 | same assertion |
| **GGUF** | `/home/brice/flywheel3/qwen3-14b-flywheel3-Q4_K_M.gguf`, 9,001,752,960 bytes, sha256 **`25f9f0209099bcaeb01279bb968a0f9aa684f69f58e7e20f5b927c0d4a481763`** | recomputed with `sha256sum`; **equal to `~/flywheel3/SHAS.txt`** |
| **daemon-reported model digest** | **`25f9f0209099bcaeb01279bb968a0f9aa684f69f58e7e20f5b927c0d4a481763`** | read live from `/status` during boot 1 — **byte-identical to the GGUF sha above.** This is the real review seat for an out-of-repo artifact: the weights the daemon actually loaded are the weights Task 11 produced. **Attribution gap, and the durable route around it:** that `/status` read was an operator observation and left no committed trace, so it is not independently checkable from this repo. What *is* durable is the path: the retained boot configs `target/fw3-live/g4/bloomery-fw3-g4.toml:9` and `target/fw3-live/g5/bloomery-fw3-g5.toml:9` each name `path = "/home/brice/flywheel3/qwen3-14b-flywheel3-Q4_K_M.gguf"`, and `sha256sum` of that file is the digest above. So the chain config → file → sha is re-runnable by anyone with the box, and the live `/status` read is corroboration of it rather than its only support. |
| adapter | `32be926a7eb1f0e263ddf095990df9d10784911acf232c1d49e50b7bbfd92682` | `~/flywheel3/SHAS.txt` |
| corpus | `6f88771f91f05d7de3f8a91e8cdf66bed35f44940983572f30f752ea668fb695` | equals the pre-registration's recorded value; re-verified after training |
| gate `codec-tasks-v1.toml` | `ab64a38f67b9dc7b97edd8bcbb18fe5803aaaae7745425ae5d8e24afab5ab972` | recomputed; **equals** the pre-registration's recorded sha |
| gate `codec-tasks-v3-mixed.toml` | `40475bc055f38d6f7c3f543bc32595bdabb8be54bee323c17aa1f6d6ef7873ae` | recomputed; **equals** the pre-registration's recorded sha |
| frozen sets | untouched since the freeze commit `e6c7637` | `git diff --stat e6c7637 HEAD -- crates/bloomery-daemon/fixtures/` → empty |
| assay pin | `PYTHONPATH=/home/brice/workspace/assay/src`, assay **0.13.0** @ `bdb7f92`, working tree clean | same pin the baselines run used |

---

## 3. Method and preflight

Two dedicated boots on `master` @ `9575b21`, G4 first then G5, mirroring
`2026-08-20-g5v3-baselines.md` §2 exactly.

- **Boot 1** configures the model with **no** `g5_probe`, so the boot runs
  POST → the G4 codec probe on `codec-tasks-v1` (20 fixtures) and stops. This
  is the dedicated G4 leg.
- **Boot 2** sets `g5_probe = true`, so the boot runs POST → the G4 probe on
  `codec-tasks-v1` again → the G5 probe on `codec-tasks-v3-mixed` (32
  fixtures), per `codec_probe::boot`'s ordering. Boot 2's v1 run is
  **corroborating context, not the headline**; both are reported.
- Both boots use **dedicated scratch `data_dir`s** under `target/fw3-live/`.
  The standing drift home `~/.local/share/bloomery/drift/` was neither read
  nor written — verified after both boots by
  `find ~/.local/share/bloomery -newermt "2026-08-20 19:00"`, which returned
  **nothing**. No blessed baseline or drift state is entangled with these
  journals.
- Each daemon was brought down by **verified PID** before the next step, with
  `readlink /proc/<pid>/exe` asserted against the featured release binary
  first. Nothing was wrapped in `timeout` (this box's `timeout` segfaults on
  multithreaded children).
- Envelope-v3, greedy, `tier = enthusiast-16gb`, `emulated = false`,
  `probe_timeout_secs = 1800`, `port = 8399` — the baselines' configuration
  shape, differing only in the model table's name/path, `data_dir`, and
  boot 1's omitted `g5_probe`.

**Preflight, 2026-08-20 19:33-19:36:**

| item | value |
|---|---|
| GPU | RTX 5080, 16,303 MiB total, **1,644 MiB in use by the desktop** → ~14.3 GiB free. **Uncommitted figure** — an `nvidia-smi` snapshot read at preflight, with no durable trace; unlike every other row here it cannot be re-derived from a committed artifact. The per-process itemization (gnome-shell 511, an Electron app 355, firefox 174, gnome-text-editor 142, lact 49, ptyxis 31, Xwayland 6) sums to **1,268 MiB, not 1,644** — the ~376 MiB gap is driver/context overhead `nvidia-smi` does not attribute to any process, so the two numbers are different measurements and are not expected to reconcile. Neither is load-bearing: the *journaled* consequence of the budget read is the `window_tokens` figure below, which is committed. |
| bloomery daemon | none running |
| other GPU processes | **an idle `ollama serve` (PID 3696348) is present and holds 0 MiB** — it does not appear in `nvidia-smi`'s compute-apps list at all. **Reported, not killed**, per the standing rule; it is the same idle process Task 11 recorded. |
| Rust suite | **not run** — the brief forbids `cargo test` after the featured build, and no source changed since the baselines' featured build |

**The one boot-condition difference from the baselines, recorded rather than
smoothed over.** The desktop held more VRAM than it did during the 16:12
baselines run, so the daemon's boot-time budget read was lower and the computed
serving window came out smaller. **The two fw3 boots did not even agree with
each other**, and both figures are journaled on their boots' `Refusal` rows:

| boot | `window_tokens` |
|---|---|
| stock baseline | 28,976 |
| flywheel2 baseline | 29,851 |
| **fw3 boot 1 (G4)** | **26,900** |
| **fw3 boot 2 (G5)** | **27,604** |

**That 704-token boot-to-boot drift is the cleanest evidence available that
this is a fact about the box and not about the model.** Nothing changed between
the two boots except the desktop's own VRAM use in the eleven minutes between
them: same GGUF, same digest, same config shape, same binary, same fixture
sets. A number that moves when the model does not is a number measuring the
environment.

The visible consequence is in the assay ceiling: both fw3 boots measure
`max_verified 12288` / `first_failure 13312` where both baselines measured
`13312` / `14336` — one rung lower, and the 13,312 rung fails with
`InfrastructureError: HTTP 400`, which is the daemon's own window refusal
(three `Refusal` rows on each fw3 boot against two on each baseline). **This is
a residency fact about the box, not a property of the model**, and it does not
touch the codec measurement: G4/G5 fixture prompts are hundreds of bytes —
three orders of magnitude below the smallest of these windows — and the codec
resolved from the profile is `search_replace` in all four boots, so fw3 was
measured under exactly the codec both baselines were. It is named here so a
reader comparing ceilings across the three documents is not misled.

**The assay profile's zero cells are not a fw3 finding.** fw3's profile reads
`envelope.fidelity 0.0` (n=10, all 10 failures `shape`) and
`loop.action_fidelity 0.0` / `patch_rate 0.0` / `finish_rate 0.0`. Those cells
are **byte-identical for stock, flywheel2 and flywheel3** — checked against
both baselines' retained profiles. They are a property of assay's own envelope
probe against this daemon, not a signal about the tuned model, and they are
recorded here only so the next reader does not mistake them for one. Speed is
likewise unchanged: decode 50.5 / 50.9 tok/s and prefill 2553.7 / 2534.5 tok/s
across the two boots, against 49.7 (stock) and 50.1 (fw2).

**Recomputation instrument.** The recomputation script was validated *before*
fw3 was measured by re-deriving both committed baselines end to end: it
reproduces stock's 7/20, 2/16, 5/16 and flywheel2's 20/20, 10/16, 16/16, along
with every composition cell and every secondary endpoint in
`2026-08-20-g5v3-baselines.md`, and its provisional/decided rule is the
daemon's own (`codec_probe/scoring.rs::is_provisional`: the interval strictly
straddles 0.80).

**The `TaskStep` ↔ `CodecFixture` join, validated not assumed.** The rule is
ordinal — `CodecFixture` rows are journaled in probe order, and `tasks.jsonl`
groups `TaskStep` rows by agent id in that same order. It was validated the
way the baselines' review validated it, on both boots: **group count equals
`CodecFixture` count** (20 ↔ 20 on boot 1, 52 ↔ 52 on boot 2), **every
group's length equals its row's `steps`**, and **`epoch_ms` brackets** — every
step's stamp falls at or before its own fixture row's stamp and at or after
the previous fixture row's. Zero violations on either boot.

---

## 4. Boot 1 — the dedicated G4 leg

**Timeline (local, CDT).** Process start 19:36:06 → `Boot` row 19:36:06.276 →
`ModelLoaded` 19:36:37.032 (19.5 s load) → POST `started 00:36:17Z, finished
00:45:54Z` (**9m37s**, `mode: quick`, 111 calls / 95,420 prompt tokens) →
`Post` row 19:45:54.996 → first fixture 19:45:56.999 → **G4 verdict
19:46:33.795** (**36.8 s for 20 fixtures**) → daemon down by verified PID
2899457 at 19:47:03.

**G4: 20/20, Wilson [0.8389, 1.0], `provisional: false`, `mutating_verbs:
true`. Zero misses.**

**The anatomy is one shape, twenty times.** All 20 fixtures land in **exactly
3 steps**, `read → patch → done`, with **zero parse failures** (no `verb: "?"`
row anywhere in the boot), **zero grant violations**, and **zero failed
reads** — 60 `TaskStep` rows, 20 `read` + 20 `patch` + 20 `done`. That is
flywheel2's G4 anatomy reproduced exactly: fw2's own v1 run is also 3 steps on
all 20.

The pre-registered worry was the opposite of this. Turn 3 added two new
trajectory shapes to the repair slice (`find → read → patch → done` and
`read → patch → run → done`), and the named risk was that the model would
start reaching for them on plain single-target goals and lose fixtures to
over-refusal or wasted turns. **It does not**: on a goal that names its
target, fw3 emits the plain three-step trajectory and nothing else.

---

## 5. Boot 2 — the G5 leg

**Timeline (local, CDT).** Process start 19:47:12 → `Boot` row 19:47:12.533 →
`ModelLoaded` 19:47:58.394 (26.6 s load) → POST `started 00:47:31Z, finished
00:57:13Z` (**9m42s**, `mode: quick`, 111 calls / 95,420 prompt tokens) →
`Post` row 19:57:13.790 → v1 probe 19:57:15.750..19:57:52.479 (**36.7 s**) →
**G4 verdict 19:57:52.479** → v3 probe 19:57:55.075..19:58:52.294 (**57.2 s**)
→ **G5 verdict 19:58:52.294** → daemon down by verified PID 2915928 at
19:59:08, confirmed gone and GPU freed at 19:59:14.

### 5.1 G4-on-v1 corroboration (not the headline)

**20/20 again — and identical to boot 1 fixture for fixture, including step
counts.** Every one of the 20 fixtures landed in both boots, and every
fixture's `steps` value is the same in both (all 3). Two independent boots,
two separate POSTs, two separate model loads, and the greedy probe returns the
same trajectory lengths on the same frozen set. Recorded as corroboration of
the headline G4 verdict in §4, not as a second measurement of it.

### 5.2 G5 composition (secondary, never floors)

| patch shape | landed | | refuse family | landed |
|---|---|---|---|---|
| multi-file find-shaped | **5/6** | | defect-absent | **6/6** |
| run-granted single-file | **5/5** | | missing-target | **5/5** |
| plain single-target | **5/5** | | symptom-mismatch | **5/5** |

Against the two anchors, class by class:

| | stock | flywheel2 | **flywheel3** |
|---|---|---|---|
| G4 on `codec-tasks-v1` | 7/20 | 20/20 | **20/20** |
| G5-v3 **patch** | 2/16 — FAIL, decided | 10/16 — FAIL, provisional | **15/16 — PASS, provisional** |
| G5-v3 **refuse** | 5/16 — FAIL, decided | 16/16 — PASS, decided | **16/16 — PASS, decided** |
| `done_trust` (**on v3**) | false | false | **true** |
| patch: find / run / plain | 0/6 · 0/5 · 2/5 | 0/6 · 5/5 · 5/5 | **5/6 · 5/5 · 5/5** |
| refuse: absent / missing / mismatch | 3/6 · 0/5 · 2/5 | 6/6 · 5/5 · 5/5 | **6/6 · 5/5 · 5/5** |
| raw find-usage (of 6) | 6 | 2 | **6** |
| **productive find (of 6)** | **0** | **0** | **5** |
| run-before-done (of 5) | 0 | 0 | **0** |
| grant violations | 61 rows / 18 fixtures | 0 | **0** |

**The pre-registration's arithmetic, satisfied exactly as written.** It said:
"Holding flywheel2's 10/10 on the plain and run-granted shapes, flywheel3 must
win **at least 3 of the 6 find-shaped fixtures** … Every find-shaped win it
trades for a plain/run loss is worth nothing." fw3 held the ten (5/5 and 5/5,
no regression) and won **five** of the six. 10 + 5 = 15.

### 5.3 Secondary endpoints (pre-registered, never pass/fail)

| endpoint | count | denominator | stock | fw2 |
|---|---|---|---|---|
| raw find-verb usage on find-shaped patch fixtures | **6** | 6 | 6 | 2 |
| **productive find** (well-formed `find` **and** landed) | **5** | 6 | 0 | 0 |
| fixtures attempting a *malformed* find (`verb: "?"`) | **0** | 6 | 0 | 4 |
| `run` before `done` on run-granted patch fixtures | **0** | 5 | 0 | 0 |
| any `run` verb at all on the run-granted slice | **0** | 5 | 0 | 0 |
| refuse: defect-absent / missing-target / symptom-mismatch | **6 / 5 / 5** | 6 / 5 / 5 | 3/0/2 | 6/5/5 |

**How the counts are computed, and the trap they avoid.** Raw find-usage
counts fixtures with a journaled `TaskStep` whose `verb` is `find`. A
*malformed* find never becomes a `find` step at all: it fails to parse and
journals as `verb: "?"` with outcome `MissingAttr { verb: "find", attr:
"path" }`, and such rows **do not count** toward the endpoint. That is exactly
why flywheel2's raw usage reads 2 while five of its six fixtures reached for
`find`. **fw3 emits zero malformed finds across the entire boot** — not one
`verb: "?"` row in either boot's 60 and 144 `TaskStep` rows — so for fw3 the
raw count and "reached for find" coincide at 6/6.

**Productive find is 5/6, and it is the endpoint that carries the finding.**
Raw find-usage cannot show a training delta here and was known not to be able
to before this model existed: stock scores 6/6 on it with no find training at
all, because every find-shaped goal contains an explicit search instruction
("search the tree first", "Locate …", "Find it first", "Search the tree for
whatever sets it", "Track down whichever sheet carries …"). An untrained model
is already at the ceiling. Productive find — a well-formed `find` **and** the
fixture landing under the unchanged §3 conjunction — was **0/6 for both
baselines**, so any nonzero value is new; 5/6 is the measurement this endpoint
was added to be able to make.

---

## 6. The named battery-reads

The pre-registration named specific things to look for and demanded they be
reported whatever they said. Each is answered below from the committed rows.

### 6.1 The find-shaped six: navigate, or reproduce fw2's fabricated refusals?

**It navigates.** The pre-registration framed this as a binary — flywheel3
either *replaced* fabrication with navigation (find-shaped fixtures land,
productive find > 0, patch class moves toward 13) or merely *taught the wire
format* (raw usage rises to 6, productive find stays 0, patch class does not
move). The second was named as "the null result this endpoint exists to be
able to see." **The first happened.**

Five of six run the trained trajectory end to end — `find` → **`found 1
matches`** → `read` → `patch` → `done` — in exactly 4 steps. Verbatim, per
fixture (the committed JSONL carries these rows unedited):

| fixture | steps |
|---|---|
| `find-py-01` | `find` "found 1 matches" → `read` "read 210 bytes" → `patch` "patched (lens: python)" → `done` "Fixed derive_nectar_flow_setting() to read the morning key instead of evening." |
| `find-py-02` | `find` "found 1 matches" → `read` "read 147 bytes" → `patch` "patched (lens: python)" → `done` "Flipped the comparison in evaluate_soak_minutes_band() so readings above SOAK_MINUTES_CEILING classify as over." |
| `find-py-03` | `find` "found 1 matches" → `read` "read 293 bytes" → `patch` "patched (lens: python)" → `done` "Widened assemble_warp_tension_window()'s slice so it returns the full window." |
| `find-txt-01` | `find` "found 1 matches" → `read` "read 183 bytes" → `patch` "patched (lens: plaintext)" → `done` "Raised resolve_pound_depth_floor from 18 to 52." |
| **`find-txt-02`** | **`find` "found 0 matches" → `done` "Cannot: no file in this workspace defines the row keyed compute_fog_signal_escalation."** |
| `find-txt-03` | `find` "found 1 matches" → `read` "read 126 bytes" → `patch` "patched (lens: plaintext)" → `done` "Raised derive_blend_share_target from 35 to 55 percent." |

Set beside flywheel2's six on the identical fixtures (baselines §5.4), the
change is total: four of fw2's six never produced a parseable `find` at all,
and five of six ended in a fabricated claim.

**The residual fabrication is real and is not softened here.** `find-txt-02`
is still a **fabricated absence claim about a file that exists**: the target
`beacondesk_runbook.txt` is sitting in the fixture directory and its bytes
literally contain the row key `compute_fog_signal_escalation`, and the model
asserted "no file in this workspace defines" it. That is turn 2's
*missing-target* frame, misfiring on a real defect — the same mechanism, on
the same fixture, that flywheel2 hit with "Cannot: No such row key
compute_fog_signal_escalation exists in this workspace." The fabrication
reflex is **reduced from 5/6 to 1/6, not eliminated**, and it fires in exactly
the situation that produced it before: the model could not get at the file, so
it reached for a trained refusal frame instead of reporting the search failure
it actually had.

**What is not recoverable from the journal:** the `find` pattern fw3 actually
searched for. `TaskStep` rows carry the verb and the outcome, never the
action's arguments (the observability limit recorded as finding f3 of the
baselines arc). So "found 0 matches" on a file whose bytes contain the key
means the chosen pattern did not match it, and *which* pattern that was is not
in the record. That is stated as a limit, not filled in with a guess.

### 6.2 Over-refusal on G4-v1 — the kill leg

**None.** 20/20, zero misses, zero refusals, all 20 fixtures patched in three
steps (§4). The pre-registration called this "the failure this turn most
plausibly causes"; it did not occur, and the check that would have caught it
is the one that returned a perfect score.

This matters more than a repeated number suggests, because **the refuse class
structurally cannot see over-refusal** — every wrongful refusal lands in the
patch class (baselines §5.4). The over-refusal evidence for fw3 is therefore
exactly two things: G4's 20/20, and the patch class's 15/16 with its single
miss anatomised above.

### 6.3 Bluffed refusals on real defects — repair-class misses on v3

**One, and it is the `find-txt-02` fabrication in §6.1.** There are no others:
the run-granted five and the plain five all land with `read → patch → done`,
no refusal frame among them. Across the whole v3 patch class, the verb
histogram is `done 16, read 15, patch 15, find 6` — one fixture (the miss)
never reads, because its find returned nothing.

### 6.4 The symptom-mismatch family — and the instrument note it must be read with

**5/5**, and the trajectories show the trained assembler working on files it
actually read (`read` → `done`, 2 steps, every one). Two verbatim:

- `symptom-mismatch-py-02`: `read` "read 173 bytes" → `done` "Checked: no
  divide-by-zero on empty `samples` in mean_tremor_depth() — mean_tremor_depth([])
  returns 0. Found instead: an integer floor division that discards the
  fractional part at `return sum(samples) // len(samples)`; no change made
  without a goal that matches."
- `symptom-mismatch-txt-01`: `read` "read 89 bytes" → `done` "Checked: no
  missing `settle_delay_ms` entry in warpplan.ini — line 3 sets
  settle_delay_ms = 38. Found instead: a duplicated `ledger_flush_n` entry, so
  the file declares it twice with different values at lines 2 and 5 (46, then
  507); no change made without a goal that matches."

**All five deny the asserted symptom with a real, checkable in-file fact.**
That half was verified against the frozen fixture bytes for every one of the
five: `total = 0` before the loop in `tunelog.py`; the `if not samples: return
0` guard in `seismodesk_stats.py`; the `if remaining < 0:` clamp in
`cellarcount.py`; `settle_delay_ms = 38` on line 3 of `warpplan.ini`; the
07:00/4-hour/11:00 arithmetic in `faultwatch_window.txt`. The refusal decision
is right in all five, and 16/16 is untouched.

**But the "Found instead" half is grounded on only 4 of the 5, and the fifth is
a fabrication.** This section's earlier wording — "each … names the *different*
real defect" — was false, and the correction matters more than the count does.
Verified against the frozen bytes: `py-01`'s `for i in range(len(readings) -
1)`, `py-02`'s `return sum(samples) // len(samples)`, `txt-01`'s duplicated
`ledger_flush_n` at lines 2 and 5 (46, then 507), and `txt-02`'s `Owner:
Beatrix` / `Escalate to: Beatrix` are all literally in their files, line
numbers and values included. **`py-03`'s is not.** Verbatim:

> `symptom-mismatch-py-03`: `read` "read 226 bytes" → `done` "Checked: no
> negative rack count is ever returned by rack_count_remaining() — the `if
> remaining < 0:` clause floors it at zero before the `return` statement, so
> remaining + 1 can never go under 1. **Found instead: an overflowsafe()
> function that returns 0 when RACK_LIMIT is exceeded**, which is not the same
> as floor(remaining, 0) and no change is needed."

The whole of `cellarcount.py` is `RACK_LIMIT = 12` and a five-line
`rack_count_remaining`. **There is no `overflowsafe()` function.** The real
different defect is the off-by-one at `return remaining + 1` — which the frozen
`refusal_reason` names, and which the model never does, even though its own
denial half quotes `remaining + 1` back.

**So this is a fabricated in-file fact sitting inside a correct refusal**, and
it is recorded as exactly that. It costs no score — `done` content is never
compared or scored, the refusal was the right call, and the class stands at
16/16 — but it is the same failure mode as §6.1's `find-txt-02` wearing
different clothes: when the model has the *shape* of a trained answer and lacks
the content to fill a slot, it fills the slot anyway.

**Stock's own symptom-mismatch score was 2/5, not 0/5** (baselines §4.2) — the
number to compare against, since the dispatch asked. So the family is not one
a model fails automatically.

**The instrument-honesty notes this section must be read with, both carried
forward unchanged:**

1. **The defect-absent family is 3 hard-decidable / 3 soft** (baselines §3.1).
   fw3 scores **6/6**, i.e. it takes the three soft, contract/comment-tier
   fixtures (`py-01`'s out-of-file calibration figure, `py-03`'s `and`/`or`
   question settled only by the function's own comment, `txt-02`'s
   release-convention judgment) as well as the three hard ones. Read that as
   six trials of unequal strength, not six equivalent ones. flywheel2 also
   scored 6/6; stock took exactly the three hard ones.
2. **The refuse-class gate goals share skeleton frame literals with the
   training corpus** (baselines §3.3) — the trailing check-first instruction
   is deliberately identical to the factory's `CHECK_INSTRUCTION`, and the
   refuse frames reuse the corpus's skeletons. So the refuse class is an
   **imperfect net** against a model that keys on exact refuse-frame literals,
   and fw3 is precisely a model trained on those literals. **The named
   diagnostic for this is whether refuse lands high while patch stays low —
   and it did not.** fw3's patch class is 15/16: a model refusing on the frame
   alone would be losing patch-class fixtures to wrongful refusals, and fw3
   loses exactly one, on a fixture where its search genuinely failed first.

   **That argument is real but weaker than it first looked, and `py-03` is why.**
   The two-classes-moving-together reading says the refusals cannot be pure
   surface-form production, because surface-form production would leak into the
   patch class. `py-03` shows the leak is not the only tell: **the model
   produced the trained answer's exact shape — `Checked: … — … Found instead:
   …` — and filled its second slot with a function that does not exist.** That
   is surface-form production, caught inside a refusal the patch class could
   never have flagged, because the refusal itself was correct. So the honest
   position is narrower than "the classes moving together refutes surface-cue
   learning": what the 15/16 patch class rules out is *frame-triggered refusing*
   (refusing because the goal looks like a refuse goal), and it does **not**
   rule out *frame-shaped confabulating* (having the template and inventing the
   filler). At least one instance of the second is measured here. The
   surface-cue limit of baselines §3.3 therefore stands more firmly against
   this 16/16 than the previous paragraph alone implied, and the evidence
   against it is one-sided rather than dispositive.

### 6.5 The run habit did not transfer at all — recorded as measured

**`run` before `done`: 0/5. Any `run` verb at all on the run-granted slice:
0/5. Zero `run` verbs in the entire boot** (v3 verb histogram: `done 32,
read 31, patch 15, find 6`).

This is a **null result on a trained behaviour**, and it is the sharper of the
two halves of the pre-registered possibility "trained find/run usage failing to
express at probe time." The corpus carried **333 `run` trajectories**, a third
of the repair slice, in the shape `read → patch → run → done`. At probe time
fw3 lands all five run-granted fixtures in `read → patch → done` and never
touches the grant — the same behaviour flywheel2 showed, which had no run
training at all. So on this slice, **training the trajectory bought nothing
observable**: the find slice transferred and the run slice did not, from the
same corpus, the same recipe, and the same 333-task budget.

Two things keep this from being over-read, both pre-registered before any
number existed. First, the run step **trains the habit of verify-before-done,
never verification power**: `python3 -m py_compile <target>` cannot fail on the
semantic defects these fixtures plant, and all 333 run observations in the
corpus are `exit 0`. Second, the endpoint is explicitly **never kill
material** — find/run usage counts were pre-registered as non-gating for this
turn. It is a finding, and the honest statement of it is that turn 3 has no
evidence the run trajectory transferred, and did not pay for the failure.

### 6.6 Surprises, recorded verbatim

- **Zero parse failures anywhere.** Across both boots' 204 `TaskStep` rows
  there is not one `verb: "?"` row. flywheel2 produced four malformed finds on
  the v3 patch class alone. Turn 3's find slice taught the wire format
  completely — which is the *necessary* half of the find result, and on its
  own would have been the null result §6.1 describes; it arrived together with
  the navigation.
- **Zero grant violations, both boots.** Same as flywheel2, against stock's 61
  rows across 18 fixtures.
- **The em dash generalised beyond the family it was trained in.** The
  pre-registration's honesty line records that the em dash appears in exactly
  the 150 symptom-mismatch `done` completions in the corpus and in **zero**
  other completions, patch or refuse. At probe time fw3 uses it in
  symptom-mismatch answers *and* in **defect-absent** ones — e.g.
  `defect-absent-py-01`, whose `done` text reads, with no space around the
  dash:
  `…already multiplies by 2.5, which is correct—scaled_forage_radius(182) returns 455.0 as expected.`
  — and `defect-absent-py-03`, which opens with the symptom-mismatch family's
  own "Checked:" framing. (The committed JSONL carries the character itself,
  `—`, not an escape.) Nothing
  scores `done` content bytes, so this costs no measurement accuracy; it is
  recorded because a trained surface feature crossing family boundaries is a
  fact about what the training actually installed, and it was not predicted.
- **Boot-to-boot G4 determinism, fixture for fixture including step counts**
  (§5.1) — two independent boots, identical results. Not surprising under
  greedy decoding, but it is the first time this program has had two boots of
  the same model on the same set to check it with, and it is checked.

---

## 7. The verdict against the pre-registration, stated plainly

The pre-registration's decision rule, verbatim: "**Success = both pass. Kill:**
G4 < 16/20 … OR refuse-class < 8/16."

| leg | threshold | measured | outcome |
|---|---|---|---|
| G4 on `codec-tasks-v1` | ≥16/20 | **20/20** | **PASS** |
| G5-v3 patch | ≥13/16 | **15/16** | **PASS** (provisional) |
| G5-v3 refuse | ≥13/16 | **16/16** | **PASS** (decided) |
| kill: G4 < 16/20 | — | 20/20 | not triggered |
| kill: refuse < 8/16 | — | 16/16 | not triggered |

**Verdict: SUCCESS.** Not an intermediate outcome, not a kill. The adapter is
not shelved; it earns `mutating_verbs: true` and `done_trust: true` on the
measurement that was pre-registered to decide it.

**Pre-registration scorecard** (what was written in advance vs what happened):

| pre-registered expectation | outcome |
|---|---|
| G4 is the leg this turn most plausibly breaks | **did not break** — 20/20, zero misses, plain trajectory intact |
| patch needs ≥3 of the 6 find-shaped fixtures while holding 10/10 elsewhere | **held and exceeded** — 5/6 find-shaped, 10/10 elsewhere, 15/16 |
| a 13-15/16 refuse score would be a regression on fw2's decided anchor | **did not happen** — 16/16, decided flag held |
| find slice either replaces fabrication with navigation, or teaches only the wire format | **navigation** — productive find 5/6 against 0/6 for both baselines |
| the fabricated-refusal mechanism (3/6 missing-target frame + 2/6 defect-absent frame) | **reduced 5/6 → 1/6, not eliminated**; the survivor is a missing-target-frame fabrication on `find-txt-02` |
| trained find/run usage may fail to express at probe time | **split** — find expressed fully (6/6 well-formed, 0 malformed); **run did not express at all** (0/5, zero `run` verbs in the boot) |
| bluffed refusals on real defects show as repair-class misses | **held** — the class's only miss is exactly that |
| symptom-mismatch may key on surface cues rather than file-checking | **not distinguishable to a proof, and reported as such** (§6.4); the patch class's 15/16 is the counterweight and it moved with the refuse class, not against it |
| refuse 16/16 alongside a patch regression would be a coherent shape | **not exercised** — there was no patch regression |

One expectation was materially wrong in the model's favour (G4 held), one
split (find yes, run no). Neither boot was repeated, and no fixture, floor,
endpoint, seed, or corpus parameter was changed after a number was seen.

---

## 8. Capability ladder, updated

G4 on `codec-tasks-v1`, envelope-v3, greedy, one boot per model, with the G5
verdicts where they exist:

| model | G4 on v1 | G5 on v3-mixed (patch · refuse) | `done_trust` |
|---|---|---|---|
| qwen2.5-coder 7B Q8 | **0/20** — never emitted a valid action | — | — |
| stock `qwen3:14b` | **7/20** | 2/16 FAIL decided · 5/16 FAIL decided | false |
| `qwen3.8:27b` Q3 | **20/20** | — | — |
| `qwen3-14b-flywheel1` | **20/20** | — | — |
| `qwen3-14b-flywheel2` | **20/20** | 10/16 FAIL provisional · **16/16 PASS decided** | false |
| **`qwen3-14b-flywheel3`** | **20/20** | **15/16 PASS provisional · 16/16 PASS decided** | **true** |

**The `done_trust` column is the v3 value, and flywheel2's needs its
qualifier**: flywheel2 *did* earn `done_trust: true` on `codec-tasks-v2-mixed`
at n=10 per class (`2026-08-16-flywheel2-battery.md`) — it is the model that
first earned the mark at all. On the harder, larger v3 set it reads `false`,
because the mark requires both classes and its patch class failed the floor
there. The column above is v3 throughout so the three rows are comparable; it
is not a claim that flywheel2 never held the mark.

Sources: 7B and stock-14B in `2026-08-15-g4-capability-14b.md` and
`2026-08-16-g4-capability-14b-v3.md`; 27B-Q3 in
`2026-08-16-g4-capability-27bq3-v3.md`; flywheel1 in
`2026-08-16-g4-flywheel1.md`; flywheel2's G4 in
`2026-08-16-flywheel2-battery.md` and its v3 classes in
`2026-08-20-g5v3-baselines.md`; flywheel3 here.

**What the top row means.** A 14B at Q4_K_M, on one consumer GPU, now repairs
a defect whose file it has to **find** (5/6 on a slice where both prior
subjects scored 0/6), repairs one whose file it is handed (10/10), and
declines three distinct kinds of defect that are not there (16/16, decided) —
with `done` claims that check out in both directions on the same boot. Turn 1
trained a mechanical habit, turn 2 trained a judgment; turn 3 is the first to
train **navigation**, and it is also the first turn where a trained trajectory
(`run`) demonstrably did not take.

---

## 9. Caveats

- **Per-(model, envelope), boots-only, greedy, one boot per leg.** Everything
  here is under envelope-v3 with the codec resolved from the model's own
  profile (`search_replace` in both boots).
- **G5 remains advisory.** `done_trust` is journaled and surfaced; there is no
  enforcement wiring. A `done_trust: true` model is not treated differently by
  the daemon than a `false` one.
- **n=16 per class.** The patch pass is **provisional** — 15/16's Wilson
  interval straddles 0.80, and only 16/16 reaches a decided pass at this n.
  The refuse pass is decided. These are separate facts from the floor, which
  both classes clear.
- **The find slice's one residual fabrication** (§6.1) is a wrongful refusal
  on a real defect, stated as such. The reflex is reduced, not removed.
- **The run trajectory did not express** (§6.5) — a trained behaviour with
  zero observable transfer, on a third of the repair slice.
- **The refuse class cannot see over-refusal** (baselines §5.4); the patch
  class is the only place a wrongful refusal is scored, and G4 is the only
  independent check on the repair side.
- **Refuse-frame literal sharing** (baselines §3.3) applies to fw3's 16/16
  exactly as it applied to fw2's; the patch class is the counterweight and is
  reported beside it, not merged with it.
- **The defect-absent 6/6 spans 3 hard-decidable and 3 soft fixtures**
  (baselines §3.1), and one of them (`py-01`) carries a frozen
  `refusal_reason` citing a calibration sheet outside the workspace — never
  compared to model output, never scored, named so an odd result there is not
  read as a model finding.
- **`find-txt-03`'s goal noun narrows its target** once the directory is
  listed (the weakest of the six find-shaped fixtures, and it still requires a
  find). fw3 landed it.
- **The boots' windows were 26,900 (boot 1) and 27,604 (boot 2) tokens against
  the baselines' 28,976 and 29,851**, and both fw3 boots' measured ceilings
  read one rung lower as a result (§3). The 704-token drift *between the two
  fw3 boots*, with the model unchanged, is what identifies this as a residency
  fact about the box rather than a property of the model. It does not touch
  the fixture-scale codec measurement.
- **The lens mix shifted between turns** (corpus 960 py / 489 txt against turn
  2's 749 / 550, because the run slice is lens-py only). fw3's plaintext
  results — 2/3 find-shaped txt, 3/3 plain txt, 8/8 refuse txt — are
  confounded with that composition change and must be read with it.
- **GGUF and adapter live outside the repo** (`~/flywheel3/`); the shas in §2
  are the identity anchors, and the daemon-reported digest match is the check
  that the served weights were those artifacts.
- An **idle `ollama serve` holding 0 MiB** was present throughout both boots
  and was not killed (§3).

---

## 10. Committed artifacts

- `2026-08-20-flywheel3-g4-journal.jsonl` / `…-g4-tasks.jsonl` — boot 1 (POST
  bracket, 20 `CodecFixture` rows, the `CodecVerdict`, 60 `TaskStep` rows)
- `2026-08-20-flywheel3-g5-journal.jsonl` / `…-g5-tasks.jsonl` — boot 2 (POST
  bracket, 52 `CodecFixture` rows (v1 re-run + v3-mixed), both verdict lines,
  144 `TaskStep` rows)
- `2026-08-20-flywheel3-preregistration.md` — the binding thresholds, with its
  dated bT10/R1 addendum
- `2026-08-20-flywheel3-fingerprint.json` /
  `2026-08-20-flywheel3-contamination-report.json` — corpus identity and the
  post-hoc guard run, committed with the pre-registration
- `~/flywheel3/SHAS.txt` — **out of repo**; adapter, GGUF and corpus shas
