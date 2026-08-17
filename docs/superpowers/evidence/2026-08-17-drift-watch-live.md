# Drift watch — live acceptance: three real boots on this box

**Date:** 2026-08-17 (16:11 local at close; ~34 min wall across three boots)
**Spec:** `docs/superpowers/specs/2026-08-17-drift-watch-design.md`,
deliverable §10.5. Wave: `.superpowers/sdd/2026-08-17-drift-watch/`.
**bloomery:** worktree `.worktrees/drift` at `254ddb9` — the wave's last
code commit. **No code changed for this acceptance**; this document is
the only thing added on top. Suite green before and after: 45 suites,
**507 passed, 0 failed** (`cargo test -p bloomery-core -p bloomery-daemon`,
run at both ends).
**Box/tier:** `enthusiast-16gb`, `emulated = false` — RTX 5080 16 GB,
Vulkan, driver 595.84, CUDA 13.2.
**GPU, all three readings:** **774 MiB / 16303 MiB** before the first
boot; **776 MiB** immediately after the last shutdown; **839 MiB** at
the final check a few minutes later. The last reading is *higher than
the first* and that is not bloomery: `nvidia-smi`'s process table at
that moment listed only `ptyxis`, `lact` and a Chrome GPU process, and
`pgrep -x bloomery-daemon` was empty. The card is shared with a live
desktop (GNOME Shell, Xwayland, Firefox, Chrome), whose usage drifts by
tens of MiB on its own; the bloomery-attributable delta at close is
**zero**. Peak during a boot was ~14.4 GiB with the model resident.
**Model:** `/home/brice/flywheel2/qwen3-14b-flywheel2-Q4_K_M.gguf` —
bloomery's own flywheel2 merge, 8.38 GiB Q4_K_M, 40 layers, fully
resident (`llama_prepare_model_devices: using device Vulkan0 (NVIDIA
GeForce RTX 5080) - 15065 MiB free`). Configured under the model name
`qwen3-14b-flywheel2`.

## Verdict

All three boots read exactly what the spec pins, first try, with no code
changes and no retries.

| boot | data_dir | assay pin | drift-step | drift-cumulative | diff spawned? |
|---|---|---|---|---|---|
| 1 | fresh | 0.9.0 | `unmeasured` (no previous) | `unmeasured` (no baseline) | no (`exit_code: null`) |
| 2 | boot 1's | 0.9.0 | **`within-noise`** | **`within-noise`** | yes, both (`exit_code: 0`) |
| 3 | scratch **copy** of boot 2's | **0.5.0** (`74c5b71`) | **`instrument-changed (0.9.0/v8 -> 0.5.0/v4)`** | same | no (`exit_code: null`) |

Boot 1 auto-blessed (`provenance: "auto-first-profile"`) **after** both
comparisons answered, so cumulative honestly read `unmeasured` on the
boot that created its own baseline — the ordering Task 4 pinned. Boots 2
and 3 journaled **zero** `Blessed` rows: a baseline already stood.

## §6 — how the assay pin is set on this box (deployment requirement)

Spec §6 moves bloomery's pin from `74c5b71` to assay 0.9.0. On this box
**assay is not installed as a package** — `python3 -m pip show assay` →
`WARNING: Package(s) not found: assay`, and a bare `python3 -c "import
assay"` is `ModuleNotFoundError`. The pin is therefore an **environment
pin on the daemon process**:

```bash
PYTHONPATH=/home/brice/workspace/assay/src \
  target/debug/bloomery-daemon --config <config>.toml
```

```
$ PYTHONPATH=/home/brice/workspace/assay/src python3 -c "import assay; print(assay.__version__)"
0.9.0
```

**Why it has to be on the daemon, not on a shell.** The daemon spawns
both `python3 -m assay probe` (`post::PostRunner`) and `python3 -m assay
diff --gate` (`drift::DriftGate`) with `std::process::Command`, which
inherits the parent's environment. `config.assay.python` is the bare
string `"python3"` — it names an interpreter, not an assay install — so
the *only* thing that decides which assay the daemon measures and diffs
with is the `PYTHONPATH` the daemon itself was started under. Verified
per boot by reading it back out of the live process:

```
$ tr '\0' '\n' < /proc/4180593/environ | grep '^PYTHONPATH='
PYTHONPATH=/home/brice/workspace/assay/src
```

This is also what makes the boot-3 pin flip a *real* instrument change
rather than a doctored file: swapping that one variable swaps the
instrument for probe and diff together, which is precisely `DriftGate::new`'s
rule ("the gate's interpreter is the probe's interpreter").

**Operators deploying this must set `PYTHONPATH` (or install assay
0.9.0 into the interpreter `config.assay.python` names) in the daemon's
own environment — a login-shell export that the service manager does not
pass through is not the pin.**

## What ran, verbatim

```bash
cd /home/brice/workspace/bloomery/.worktrees/drift
cargo build --features vulkan            # a featureless build cannot load models
cargo test -p bloomery-core -p bloomery-daemon    # 507 passed, 0 failed

# boot 1 and boot 2 — identical command, identical config, same data_dir
PYTHONPATH=/home/brice/workspace/assay/src \
  ./target/debug/bloomery-daemon --config target/drift-live/bloomery-drift.toml

# between boots: clean shutdown by verified PID (never pkill by bare name)
P=$(pgrep -x bloomery-daemon); readlink /proc/$P/exe   # assert the full binary path
kill -TERM $P

# boot 3 — old pin, scratch COPY of the data_dir
mkdir -p <scratch>/assay-74c5b71
git -C /home/brice/workspace/assay archive 74c5b71 | tar -x -C <scratch>/assay-74c5b71
cp -a target/drift-live/data <scratch>/boot3-data
PYTHONPATH=<scratch>/assay-74c5b71/src \
  ./target/debug/bloomery-daemon --config target/drift-live/bloomery-drift-boot3.toml
```

`<scratch>` = the session scratchpad,
`/tmp/claude-1000/-home-brice/611a2bcd-4387-4f48-863a-b55015758633/scratchpad`.
`git archive` is read-only on the assay repo — its working tree was
verified untouched (`git -C /home/brice/workspace/assay status --short`
was empty after the extraction). The extracted rev resolves to
`assay 0.5.0`, schema v4.

Nothing was wrapped in `timeout` (this box's `timeout` binary segfaults
on multithreaded children).

### The config — every value verbatim

Not committed — it names local paths. **Values are byte-exact; the
on-disk file's comments are elided here** (a three-line header saying
what the file is, and a four-line rationale above `probe_timeout_secs`
— that rationale's content is the paragraph below the block). Boot 3's
config differs only in `data_dir` (pointing at the scratch copy) and in
its header comment, which says so.

```toml
port = 8399
data_dir = "/home/brice/workspace/bloomery/.worktrees/drift/target/drift-live/data"

[models]
"qwen3-14b-flywheel2" = "/home/brice/flywheel2/qwen3-14b-flywheel2-Q4_K_M.gguf"

[tier]
name = "enthusiast-16gb"
emulated = false

[assay]
enabled = true
python = "python3"
probe_timeout_secs = 1800
```

`probe_timeout_secs = 1800` (30 min) against a measured ~9m43s probe:
generous by ~3×, and generous on purpose — see the boot-window note
below, where a confirm probe would have cost a second one.
`tasks_enabled` left at its default `false`, so no G4/G5 codec probe ran
and the boots exercise the drift path only. `allow_unprofiled` left
`false`.

## Boot 1 — fresh data_dir

Journal `data/journal/boot-1786999045.jsonl` (boot at 15:37:25 CDT;
serving ~15:39; profile written 15:49:06). Probe wall **9m42s**
(`provenance.started 20:39:24Z → finished 20:49:06Z`). Traffic the probe
put through the daemon's own `/v1`: 111 `AgentCreated`, 109
`InferCompleted`, 2 `Refusal`. Vulkan backend init before the socket
bound took ~95 s of single-threaded shader work — that is llama.cpp's,
not bloomery's, and it happens before `serving on` prints.

This boot's drift-relevant rows, verbatim and untruncated. This is a
**selection, not the whole journal**: the omitted rows are the probe's
own per-call traffic and its bookkeeping — `AgentCreated` ×111,
`SchedulerDecision` ×109, `InferStarted` ×109, `InferCompleted` ×109,
`AgentRemoved` ×111, `ModelLoaded` ×1, and `Refusal` ×2 (those two are
quoted in full further down). Nothing drift-related is omitted: the
counts above are the complete event census of
`boot-1786999045.jsonl`, and every `Boot`/`Degraded`/`Post`/`Drift`/`Blessed`
row it contains is below.

```json
{"event":"Boot","version":"0.1.0"}
{"event":"Degraded","reason":"provisional admission: qwen3-14b-flywheel2 has no profile yet; POST in progress"}
{"event":"Post","model":"qwen3-14b-flywheel2","outcome":"ok","profile_path":"/home/brice/workspace/bloomery/.worktrees/drift/target/drift-live/data/profiles/qwen3-14b-flywheel2.json"}
{"event":"Drift","model":"qwen3-14b-flywheel2","comparison":"step","outcome":"unmeasured: reference /home/brice/workspace/bloomery/.worktrees/drift/target/drift-live/data/profiles/qwen3-14b-flywheel2.previous.json: no such file","reference_path":"/home/brice/workspace/bloomery/.worktrees/drift/target/drift-live/data/profiles/qwen3-14b-flywheel2.previous.json","current_path":"/home/brice/workspace/bloomery/.worktrees/drift/target/drift-live/data/profiles/qwen3-14b-flywheel2.json","exit_code":null,"reference_sha":null,"current_sha":"9dc4033982f061d7c023f4a290a23adf5e193d4e22c873f55671611f863e4eed"}
{"event":"Drift","model":"qwen3-14b-flywheel2","comparison":"cumulative","outcome":"unmeasured: reference /home/brice/workspace/bloomery/.worktrees/drift/target/drift-live/data/profiles/qwen3-14b-flywheel2.baseline.json: no such file","reference_path":"/home/brice/workspace/bloomery/.worktrees/drift/target/drift-live/data/profiles/qwen3-14b-flywheel2.baseline.json","current_path":"/home/brice/workspace/bloomery/.worktrees/drift/target/drift-live/data/profiles/qwen3-14b-flywheel2.json","exit_code":null,"reference_sha":null,"current_sha":"9dc4033982f061d7c023f4a290a23adf5e193d4e22c873f55671611f863e4eed"}
{"event":"Blessed","model":"qwen3-14b-flywheel2","profile_path":"/home/brice/workspace/bloomery/.worktrees/drift/target/drift-live/data/profiles/qwen3-14b-flywheel2.baseline.json","sha":"9dc4033982f061d7c023f4a290a23adf5e193d4e22c873f55671611f863e4eed","provenance":"auto-first-profile"}
```

Four things this pins:

1. **The auto-bless ordering.** The `Blessed` row is *after* both
   `Drift` rows, and cumulative reads `unmeasured` — not a manufactured
   `within-noise` against a baseline byte-identical to the current
   document. This is the ambiguity the plan resolved in-plan and Task 4
   pinned; it is now measured.
2. **`exit_code: null` on both.** No `assay diff` was spawned: with no
   reference there is nothing a subprocess could have decided.
3. **`reference_sha: null`, `current_sha` present.** The digest is of
   the bytes the gate actually read, and only the side that existed has
   one.
4. **The path claim is checkable.** `sha256sum` on the blessed file
   equals the row's `sha` exactly:
   `9dc4033982f061d7c023f4a290a23adf5e193d4e22c873f55671611f863e4eed`
   — and the baseline is a *copy*, so the current document carries the
   same digest.

`GET /status` (`models[0]`, after POST closed):

```json
{
  "posting": false,
  "name": "qwen3-14b-flywheel2",
  "profiled": true,
  "drift": {
    "step":       {"status": "unmeasured", "reason": "reference /home/brice/workspace/bloomery/.worktrees/drift/target/drift-live/data/profiles/qwen3-14b-flywheel2.previous.json: no such file"},
    "cumulative": {"status": "unmeasured", "reason": "reference /home/brice/workspace/bloomery/.worktrees/drift/target/drift-live/data/profiles/qwen3-14b-flywheel2.baseline.json: no such file"}
  },
  "done_trust": null,
  "codec_gate": null
}
```

`done_trust` and `codec_gate` stay `null` — design §7's separation
holds on a live boot: drift answers a different question and does not
touch them.

Instrument and headline measurement of this profile: `0.9.0/v8`,
`ceiling.max_verified = 14336`, `first_failure = 15360`,
`failure_mode: hard_error` — assay's ladder recording
`InfrastructureError: HTTP 400 from http://127.0.0.1:8399/v1/chat/completions`.
The daemon's own side of those two 400s is journaled, and it is the
window law doing its job rather than anything breaking:

```json
{"event":"Refusal","id":"a7","needed_tokens":35016,"window_tokens":32468,"detail":"prompt + max_tokens exceeds the computed window"}
{"event":"Refusal","id":"a10","needed_tokens":32833,"window_tokens":32468,"detail":"prompt + max_tokens exceeds the computed window"}
```

(assay's `est_tokens` ladder rung is an estimate of prompt size; the
daemon's `needed_tokens` is the real prompt-plus-`max_tokens` charge it
refused against a 32468-token window.)

## Boot 2 — same data_dir, real diff subprocesses

Journal `data/journal/boot-1786999799.jsonl` (boot 15:49:59; profile
16:01:40). Probe wall **9m43s** — within one second of boot 1's — and
the identical traffic shape: 111 `AgentCreated`, 109 `InferCompleted`,
2 `Refusal`.

Rotation happened first, as spec §5's law requires: boot 1's
`qwen3-14b-flywheel2.json` became `qwen3-14b-flywheel2.previous.json`
(same digest `9dc40339…`), the baseline untouched, and nothing was
journaled for it (a clean rotation adds no fact the step row does not
already carry).

Rows, verbatim and untruncated:

```json
{"event":"Post","model":"qwen3-14b-flywheel2","outcome":"ok","profile_path":"/home/brice/workspace/bloomery/.worktrees/drift/target/drift-live/data/profiles/qwen3-14b-flywheel2.json"}
{"event":"Drift","model":"qwen3-14b-flywheel2","comparison":"step","outcome":"within-noise","reference_path":"/home/brice/workspace/bloomery/.worktrees/drift/target/drift-live/data/profiles/qwen3-14b-flywheel2.previous.json","current_path":"/home/brice/workspace/bloomery/.worktrees/drift/target/drift-live/data/profiles/qwen3-14b-flywheel2.json","exit_code":0,"reference_sha":"9dc4033982f061d7c023f4a290a23adf5e193d4e22c873f55671611f863e4eed","current_sha":"ae86e4f5fcbd2fd5cc4386a7944b2d8cb4090c67397ab2e387fcaaea6debe301"}
{"event":"Drift","model":"qwen3-14b-flywheel2","comparison":"cumulative","outcome":"within-noise","reference_path":"/home/brice/workspace/bloomery/.worktrees/drift/target/drift-live/data/profiles/qwen3-14b-flywheel2.baseline.json","current_path":"/home/brice/workspace/bloomery/.worktrees/drift/target/drift-live/data/profiles/qwen3-14b-flywheel2.json","exit_code":0,"reference_sha":"9dc4033982f061d7c023f4a290a23adf5e193d4e22c873f55671611f863e4eed","current_sha":"ae86e4f5fcbd2fd5cc4386a7944b2d8cb4090c67397ab2e387fcaaea6debe301"}
```

`exit_code: 0` on both — two real `python3 -m assay diff … --gate`
subprocesses ran and answered. No confirm probe was earned (nothing read
`Drift`), no `Degraded` beyond the provisional-admission bracket, no
`Blessed`.

`GET /status` → `{"step": {"status":"within-noise"}, "cumulative":
{"status":"within-noise"}}`.

**The row is re-runnable, and it was re-run by hand:**

```
$ PYTHONPATH=/home/brice/workspace/assay/src python3 -m assay diff \
    /home/brice/workspace/bloomery/.worktrees/drift/target/drift-live/data/profiles/qwen3-14b-flywheel2.previous.json \
    /home/brice/workspace/bloomery/.worktrees/drift/target/drift-live/data/profiles/qwen3-14b-flywheel2.json --gate
no drift beyond noise
within noise: ceiling.max_verified, ceiling.failure_mode, verdict.agent_speed,
  verdict.chat_speed, verdict.long_output, verdict.long_output.provisional,
  verdict.loop_discipline, verdict.loop_discipline.provisional,
  verdict.patch_editing, verdict.patch_editing.provisional,
  verdict.structured_extraction, verdict.structured_extraction.provisional,
  verdict.tool_calling, verdict.tool_calling.provisional,
  codec.json_object.{tiny,small,medium,constrained,nested,tabular}.lands,
  codec.search_replace.{tiny,small,medium}.{lands,lands_applies},
  codec.whole_file.{tiny,small,medium}.{lands,lands_applies},
  speed.decode_tps, speed.prefill_tps
dropped: verdict.long_context
exit=0
```

(The `within noise:` list is assay's own single line; only the
line-wrapping and the `{a,b}` brace-folding of adjacent codec cells are
this document's, to keep it readable. Names and order are assay's.)

What actually moved between the two documents (a structural diff of the
JSON, outside assay). **Six paths changed in total; all six are here** —
the `_samples` pair is the single-element array each `_tps` scalar is
derived from, so the two speed measurements account for four of the six
lines:

```
/provenance/started      :: '2026-08-17T20:39:24Z'   -> '2026-08-17T20:51:57Z'
/provenance/finished     :: '2026-08-17T20:49:06Z'   -> '2026-08-17T21:01:40Z'
/speed/decode_tps        :: 50.14530573922124        -> 50.14145699767755
/speed/decode_samples    :: [50.14530573922124]      -> [50.14145699767755]
/speed/prefill_tps       :: 2496.547889464197        -> 2491.8657415398375
/speed/prefill_samples   :: [2496.547889464197]      -> [2491.8657415398375]
```

Two consecutive boots of a fully-resident Q4 on an idle card reproduced
every verdict, every ceiling value and every codec cell **exactly** —
two timestamps and two speed numbers are the whole of what changed in
the entire document. The speed numbers moved by 0.008 % and 0.19 %.
That is a real measurement of this seam's noise floor on this box, and
it is far tighter than the noise discipline the gate allows.

## Boot 3 — instrument-changed, on a scratch copy under the old pin

The whole boot ran against `<scratch>/boot3-data`, a `cp -a` of boot 2's
`data_dir`. **The committed-lineage originals were verified untouched**
by digest after boot 3 finished — `9dc40339…` / `ae86e4f5…` / `9dc40339…`
for baseline / current / previous, identical to before the copy.

Journal `<scratch>/boot3-data/journal/boot-1787000588.jsonl` (boot
16:03:08; profile 16:10:17). Probe wall **5m12s**; 76 `AgentCreated`,
74 `InferCompleted`, 2 `Refusal` — the 0.5.0 quick ladder is a third
smaller than 0.9.0's. (That size difference is a *symptom* of the two
instruments, not the thing the precheck looks at: the precheck compares
`probe_version` and `assay_profile_version` only.)

The daemon's own environment carried the old pin, read back live:

```
$ tr '\0' '\n' < /proc/20831/environ | grep '^PYTHONPATH='
PYTHONPATH=/tmp/claude-1000/-home-brice/611a2bcd-4387-4f48-863a-b55015758633/scratchpad/assay-74c5b71/src
```

Rows, verbatim and untruncated, with one substitution: `<scratch>` is
the literal scratchpad root
`/tmp/claude-1000/-home-brice/611a2bcd-4387-4f48-863a-b55015758633/scratchpad`,
folded only to keep the lines readable.

```json
{"event":"Degraded","reason":"provisional admission: qwen3-14b-flywheel2 has no profile yet; POST in progress"}
{"event":"Post","model":"qwen3-14b-flywheel2","outcome":"ok","profile_path":"<scratch>/boot3-data/profiles/qwen3-14b-flywheel2.json"}
{"event":"Drift","model":"qwen3-14b-flywheel2","comparison":"step","outcome":"instrument-changed (0.9.0/v8 -> 0.5.0/v4)","reference_path":"<scratch>/boot3-data/profiles/qwen3-14b-flywheel2.previous.json","current_path":"<scratch>/boot3-data/profiles/qwen3-14b-flywheel2.json","exit_code":null,"reference_sha":"ae86e4f5fcbd2fd5cc4386a7944b2d8cb4090c67397ab2e387fcaaea6debe301","current_sha":"31af8bf70dd0554c2d9dee7858f6216fb4e1a7e59961229827f4933d1fca361a"}
{"event":"Drift","model":"qwen3-14b-flywheel2","comparison":"cumulative","outcome":"instrument-changed (0.9.0/v8 -> 0.5.0/v4)","reference_path":"<scratch>/boot3-data/profiles/qwen3-14b-flywheel2.baseline.json","current_path":"<scratch>/boot3-data/profiles/qwen3-14b-flywheel2.json","exit_code":null,"reference_sha":"9dc4033982f061d7c023f4a290a23adf5e193d4e22c873f55671611f863e4eed","current_sha":"31af8bf70dd0554c2d9dee7858f6216fb4e1a7e59961229827f4933d1fca361a"}
```

`exit_code: null` on both: **the diff was never spawned.** Both
identities are named in the outcome (`0.9.0/v8 -> 0.5.0/v4`), both
digests are present because both documents were read — the precheck runs
*after* reading, which is what makes those digests honest. No `Blessed`
row: a baseline already stood, and an instrument change never
auto-re-blesses (spec §3: it stays that way until the operator
re-blesses).

`GET /status`:

```json
{"step":       {"status": "instrument-changed", "reference": "0.9.0/v8", "current": "0.5.0/v4"},
 "cumulative": {"status": "instrument-changed", "reference": "0.9.0/v8", "current": "0.5.0/v4"}}
```

### The falsification, and why §3 earns its place

The instrument precheck is not a formality here. Running by hand the
diff that boot 3 refused to spawn — `assay 0.9.0`, the pin the gate
would have used had the precheck let it through:

```
$ PYTHONPATH=/home/brice/workspace/assay/src python3 -m assay diff \
    <scratch>/boot3-data/profiles/qwen3-14b-flywheel2.previous.json \
    <scratch>/boot3-data/profiles/qwen3-14b-flywheel2.json --gate
no drift beyond noise
within noise: ceiling.max_verified, ceiling.failure_mode, verdict.agent_speed,
  verdict.chat_speed, verdict.loop_discipline, verdict.loop_discipline.provisional,
  verdict.patch_editing, verdict.patch_editing.provisional,
  verdict.structured_extraction, verdict.structured_extraction.provisional,
  codec.json_object.{tiny,small,medium}.lands,
  codec.search_replace.{tiny,small,medium}.{lands,lands_applies},
  codec.whole_file.{tiny,small,medium}.{lands,lands_applies},
  speed.decode_tps, speed.prefill_tps
dropped: verdict.long_context, verdict.long_output, verdict.tool_calling,
         codec.json_object.constrained.lands, codec.json_object.nested.lands,
         codec.json_object.tabular.lands
exit=0
```

(Complete — nothing elided. Same wrapping and brace-folding convention
as boot 2's block above. This `within noise:` list is **27 names against
boot 2's 34**, and the 7 that left are exactly: the five *real* dropped
entries — `verdict.long_output`, `verdict.tool_calling`,
`codec.json_object.{constrained,nested,tabular}.lands` — plus
`verdict.long_output.provisional` and `verdict.tool_calling.provisional`.
`verdict.long_context` appears in neither list's `within noise:` half,
in this run or boot 2's, which is consistent with it being the reporting
artifact of surprise 2 rather than a real drop. Seven names left the
comparison and the exit code still says 0.)

**Exit 0 — a clean pass — while five real measured families silently
vanished.** Comparing the two documents' key sets directly confirms the
v4 schema simply does not carry them:

```
v8 verdicts: agent_speed chat_speed long_context long_output loop_discipline
             patch_editing structured_extraction tool_calling
v4 verdicts: agent_speed chat_speed long_context           loop_discipline
             patch_editing structured_extraction
v8 codecs.json_object cells: tiny small medium constrained nested tabular
v4 codecs.json_object cells: tiny small medium
v8 top-level sections not present at all in v4: long_output, tools, parallel
```

(The sixth `dropped:` entry, `verdict.long_context`, is present in
*both* — it is the same reporting artifact as surprise 2 below, not a
real drop. Five are real.)

Had the gate spawned that diff, this daemon would have journaled
`within-noise` for a pair of documents that disagree about what a
profile even contains — `tool_calling` and `long_output` verdicts and
three json_object codec cells gone, reported as no change. Spec §3's
rationale ("assay's 2026-08 campaign diffs showed 12 of 15 models
'improving' because the ceiling cap moved between probe versions") is
now reproduced on this box, in this seam, against this daemon's own
profiles. The precheck refused first and named both instruments instead.

## Surprises — recorded, not tidied

1. **`assay 0.5.0` has no `diff` subcommand at all.**
   `python3 -m assay diff …` under the old pin is
   `argument command: invalid choice: 'diff' (choose from probe,
   geometry, ceiling, envelope, codecs, report)`, **exit 2**. This makes
   boot 3 a sharper test than intended — a leaked precheck could not
   have produced a quiet wrong answer under the old pin; it would have
   produced something visibly odd. But it also names an edge worth
   knowing: bloomery maps `assay diff --gate` exit 2 to
   `GateOutcome::NotComparable` ("diff itself refused the pair"), and
   argparse's rejection of an unknown subcommand *also* exits 2. An
   operator who pins an assay older than `diff` and whose two documents
   share an instrument (so the precheck passes) would get
   `not-comparable` rows — the right shape for the wrong reason. Not a
   defect in the shipped configuration: spec §6's pin is 0.9.0, which
   has `diff`. Recorded because the exit-code contract is only as good
   as the pin behind it.

2. **`assay diff` reports `dropped: verdict.long_context` for two
   byte-identical-in-that-field documents.** Both boot-1 and boot-2
   profiles carry `verdicts.long_context = {"verdict": "unmeasured",
   "lens": {"evidence": "counts+canary"}}`, and their whole `verdicts`
   objects compare *equal* in Python — yet the same-instrument diff
   prints it under `dropped:`. It is an assay-side prose artifact for a
   family that is `unmeasured` on both sides; it does **not** move the
   gate exit code (0). This is exactly why the design reads the exit
   code and never the prose ("design §4's contract is the exit code and
   the documents, never diff's prose output, which this daemon does not
   read at all") — and this boot is the first live case where reading
   the prose would have been misleading and reading the code was not.

3. **Vulkan backend init costs ~95 s of single-threaded work before the
   socket binds.** Unrelated to drift, but it is a minute and a half in
   which the daemon is alive at 100 % of one core, silent, and not yet
   answering — the process holds `/dev/nvidia0` and the shader cache
   open and prints nothing after ggml's two device lines. Anyone timing
   a boot should not read that as a hang; `serving on` is the first line
   that means anything. (It cost me one false diagnosis during this
   run.)

4. **The 14B was much faster than the historical figure.** The brief
   budgeted 10-25 min per probe from partial-offload history; fully
   resident Q4 measured **9m42s / 9m43s** for 0.9.0-quick. The
   `probe_timeout_secs = 1800` cap was never close to firing.

5. **No `Drift` reading occurred, so the confirm path did not execute on
   real hardware.** Boot 2 was clean end to end. Confirm-then-alarm
   remains covered by Task 4's suite (including the wedged-confirm
   `Unconfirmed` case) but is **not** exercised by this live acceptance.
   That is a gap in this document, not a claim it makes.

## Operator notes

**Confirm probes extend the provisional-admission window.** POST holds
`posting = true` — law 5 suspended, unprofiled models admitted — from
before the socket binds until `run_post` returns. Each comparison that
reads `Drift` earns its own confirm probe, and the two comparisons
confirm independently, so a single model can cost up to **three** full
probes in one boot (POST + step confirm + cumulative confirm). Worst
case the window stays open for roughly **N × `assay.probe_timeout_secs`**
per model, where N is 1 plus the number of comparisons that tripped. At
the 1800 s configured here that is up to 90 minutes for one model in the
pathological case; at the 600 s default, 30. An operator sizing
`probe_timeout_secs` is sizing the worst-case admission window, not just
one probe — the two are the same knob.

**Blessing during the POST window races auto-bless.** `POST
/models/{name}/bless` runs on a request thread and takes the pager lock;
boot's `auto_bless` runs on the POST thread and takes the same lock, but
only checks `baseline.exists()` before deciding. An operator who blesses
a model *while that model's boot POST is still running* can therefore
produce **two `Blessed` rows for one boot with different provenance**
(`operator` and `auto-first-profile`, in whichever order the lock
granted). Mild — the baseline on disk is a copy of the same current
document either way, so no measurement is lost — but the provenance of
that boot's baseline is ambiguous in the journal. Deferred from Task 5's
review; recorded here because the live window is minutes long, not
milliseconds. The safe habit is to bless after `/status` reports
`"posting": false`.

**Shutdown.** The daemon installs no signal handler; its main thread
parks forever. `Journal::append` flushes to the OS after every row, so
`kill -TERM <pid>` loses nothing already journaled — that is the
mechanism used between all three boots here. Find the PID with `pgrep -x
bloomery-daemon` and **verify `readlink /proc/<pid>/exe`** against the
binary you built before signalling; a `pkill -f` on a path fragment
matches the shell that is running the search on this box.

## Committed artifacts

This document, plus two dated as-built footnotes this run's findings
earned in `docs/superpowers/specs/2026-08-17-drift-watch-design.md` (§2
on the auto-bless provenance spelling, §5 on where content-addressing
actually lands and how the path-claim guarantee is met instead). The
spec's original text is left visible in both cases.

Nothing else is committed: the configs name local paths (their values
reproduced above as text), the `data_dir`s live under `target/` and the
session scratchpad, and the boot-3 tree is a deliberate throwaway copy.
Every journal row this acceptance rests on is quoted above verbatim, and
every quoted block that is a *selection* of a larger artifact says so at
the point of quoting; every digest quoted is checkable with `sha256sum`
against the path in the same row.
