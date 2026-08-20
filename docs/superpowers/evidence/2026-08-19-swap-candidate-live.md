# Swap-candidate live acceptance — the fw1 candidate against the standing fw2 baseline

**Date:** 2026-08-19 (boot 3 of the standing lineage, 22:41 CDT →
23:03 CDT; all timestamps below are UTC, i.e. 2026-08-20T03:41Z →
04:03Z). ~22 min wall: one 10m18s boot probe and two 4-minute candidate
jobs.
**Context:** the live acceptance the swap-candidate seam design (§8,
"Live acceptance") pins: probe **flywheel1's GGUF** as a candidate
against the standing **flywheel2 baseline** on this box. Tasks 1–4 of
the SDD wave built and reviewed the endpoint against injected probes;
this is the first time it ran against real assay, a real GPU and the
real standing baseline.
**bloomery:** worktree `.worktrees/seam`, branch `swap-candidate-seam`
at `f4ebd57`. Suite green before the boot: 46 suites, **566 passed, 0
failed** (`cargo test -p bloomery-core -p bloomery-daemon`), featured
binary built **after** the tests (`cargo build -p bloomery-daemon
--features vulkan`, the standing-v10 doc's two traps).
**assay:** master at `bdb7f92` (the PR #6 merge, v1.11), **0.13.0**,
`cover` present in `--help`. Suite green: **1073 passed**
(`.venv/bin/python -m pytest -q`).
**Box/tier:** `enthusiast-16gb`, `emulated = false` — RTX 5080 16 GB,
Vulkan. GPU **1821 MiB** used (desktop) before the boot; **1709 MiB**
after shutdown with `pgrep -x bloomery-daemon` empty — the
bloomery-attributable delta at close is zero.
**Model:** `/home/brice/flywheel2/qwen3-14b-flywheel2-Q4_K_M.gguf`,
configured as `qwen3-14b-flywheel2`, standing config
`~/.local/share/bloomery/drift/bloomery-drift.toml` unchanged.
**Candidate:** `/home/brice/flywheel1/qwen3-14b-flywheel1-Q4_K_M.gguf`
(9,001,752,960 bytes; sha256
`80bc0ea9042aef7f5fa16ab64e641d5ed30da6f7c2faf6da1084a388d26ff293`).
The sibling `fw1-bf16.gguf` is the training artifact — a different
quant class from the baseline's Q4_K_M and too large for this GPU — and
was deliberately not used.

## Verdict

**The endpoint did not reach a coverage verdict, twice, for the same
deterministic reason.** The candidate was never measured. That is the
finding, and it is a defect in this slice, not a fact about flywheel1.

| # | route | HTTP | result |
|---|---|---|---|
| 1 | `POST /models/qwen3-14b-flywheel2/swap-candidate` | **202** | `state: running` |
| 2 | `GET` (polled to completion) | **200** | `state: done`, `outcome: infra: … HTTP 422 …`, `exit_code: null` |
| 3 | second `POST` during the probe window (control) | **409** | `candidate_probe_in_progress` ✅ |
| 4 | `POST` for an unknown model (control) | **404** | `unknown_model` ✅ |
| 5 | confirming repeat of 1–2, well clear of any POST window | **202 → 200** | byte-identical `outcome`, `exit_code: null` |

Both negative controls passed. The two candidate jobs produced the same
sentence, the same candidate digest and the same floor digest, and
their two journal rows are byte-equal — this is deterministic, not
flaky, so no third run was made.

| comparison | reference | current | outcome | exit code |
|---|---|---|---|---|
| boot 3 drift-step | `…previous.json` (0.12.0/v10) | boot 3's (0.13.0/v10) | **`instrument-changed`** | `null` (diff never spawned) |
| boot 3 drift-cumulative | `…baseline.json` (0.12.0/v10) | boot 3's (0.13.0/v10) | **`instrument-changed`** | `null` (diff never spawned) |
| swap-candidate coverage | `…baseline.json` | *never written* | **`infra:` — unmeasured** | `null` (cover never spawned) |

No `SwapCandidate` row was journaled on either run. That is correct and
by design: the verdict row is written when a comparison happened, and
the failure path journals `Degraded` instead (design §7, "every failure
is named and none is a verdict").

## What the endpoint answered, verbatim

Run 1, POST at `03:53:46Z`:

```
HTTP 202
{"candidate":"/home/brice/flywheel1/qwen3-14b-flywheel1-Q4_K_M.gguf","model":"qwen3-14b-flywheel2","state":"running"}
```

`GET` immediately after: `{"model":"qwen3-14b-flywheel2","state":"running"}` (200).

`GET` at `03:57:57Z` (4m11s after the POST) — the whole body:

```json
{"model":"qwen3-14b-flywheel2","report":{"candidate_gguf_sha":"80bc0ea9042aef7f5fa16ab64e641d5ed30da6f7c2faf6da1084a388d26ff293","candidate_profile_path":"/home/brice/.local/share/bloomery/drift/data/profiles/qwen3-14b-flywheel2!swap-candidate.confirm.json","exit_code":null,"floor_sha":"f2a2cabc360a7423f5f963975103672cda04242c55fc04897ee0e2aa5a5b1b98","notes":["done_trust/G4/G5 are unmeasured for this candidate until its first real boot with tasks enabled","on swap: edit config, restart; the next boot reads not-comparable against the old lineage's baseline until you POST /models/{name}/bless"],"outcome":"infra: the candidate probe for qwen3-14b-flywheel2 (registered as qwen3-14b-flywheel2!swap-candidate) failed: assay exited 4: assay: infrastructure failure: HTTP 422 from http://127.0.0.1:8399/v1/chat/completions; no coverage verdict was reached — this candidate is unmeasured, not refused"},"state":"done"}
```

Run 2 (POST `03:59:09Z`, done `04:03:29Z`, 4m20s) returned the same
`report` object, field for field. HTTP bodies are ephemeral, so the
durable form of that claim is in the journal, where both runs left a
row: lines **558 and 559** of `boot-1787197252.jsonl` are **byte-equal**
— re-checkable by anyone holding the file, no transcript required:

```
$ grep '"reason":"swap:' boot-1787197252.jsonl | sort -u | wc -l
1
$ grep '"reason":"swap:' boot-1787197252.jsonl | uniq -c
      2 {"event":"Degraded","reason":"swap: the candidate probe for …
```

**The two `notes` came back on both runs, whatever the outcome, exactly
as §4's amendment promises** — this is the operator handover §5 owes,
and it is here reproduced as the surface printed it:

> `done_trust/G4/G5 are unmeasured for this candidate until its first real boot with tasks enabled`

> `on swap: edit config, restart; the next boot reads not-comparable against the old lineage's baseline until you POST /models/{name}/bless`

Both negative controls, verbatim:

```
=== CONTROL 1 03:53:46Z: second POST while the first runs ===
HTTP 409
{"detail":"a candidate probe for qwen3-14b-flywheel2 (/home/brice/flywheel1/qwen3-14b-flywheel1-Q4_K_M.gguf) is already running; one at a time, no queue","error":"candidate_probe_in_progress","model":"qwen3-14b-flywheel2"}

=== CONTROL 2 03:53:46Z: POST for an unknown model ===
HTTP 404
{"error":"unknown_model","model":"qwen3-14b-nonexistent"}
```

## The journal rows, verbatim

Boot 3's drift-relevant rows
(`~/.local/share/bloomery/drift/data/journal/boot-1787197252.jsonl`,
559 rows):

```json
{"event":"Boot","version":"0.1.0"}
{"event":"Degraded","reason":"provisional admission: qwen3-14b-flywheel2 has no profile yet; POST in progress"}
{"event":"Post","model":"qwen3-14b-flywheel2","outcome":"ok","profile_path":"/home/brice/.local/share/bloomery/drift/data/profiles/qwen3-14b-flywheel2.json"}
{"event":"Drift","model":"qwen3-14b-flywheel2","comparison":"step","outcome":"instrument-changed (0.12.0/v10 -> 0.13.0/v10)","reference_path":"/home/brice/.local/share/bloomery/drift/data/profiles/qwen3-14b-flywheel2.previous.json","current_path":"/home/brice/.local/share/bloomery/drift/data/profiles/qwen3-14b-flywheel2.json","exit_code":null,"reference_sha":"179b706e58f97f8d9e728dbefbabaf0aa8b837fb6798aedf67ca325148ee1c2a","current_sha":"34cc643b1ef321b2f30b3a0a204bdd313b1c33ff724885013c0bf74cdcbc968c"}
{"event":"Drift","model":"qwen3-14b-flywheel2","comparison":"cumulative","outcome":"instrument-changed (0.12.0/v10 -> 0.13.0/v10)","reference_path":"/home/brice/.local/share/bloomery/drift/data/profiles/qwen3-14b-flywheel2.baseline.json","current_path":"/home/brice/.local/share/bloomery/drift/data/profiles/qwen3-14b-flywheel2.json","exit_code":null,"reference_sha":"f2a2cabc360a7423f5f963975103672cda04242c55fc04897ee0e2aa5a5b1b98","current_sha":"34cc643b1ef321b2f30b3a0a204bdd313b1c33ff724885013c0bf74cdcbc968c"}
```

The two candidate jobs, at lines 558 and 559 — byte-equal to each
other, printed once:

```json
{"event":"Degraded","reason":"swap: the candidate probe for qwen3-14b-flywheel2 (registered as qwen3-14b-flywheel2!swap-candidate) failed: assay exited 4: assay: infrastructure failure: HTTP 422 from http://127.0.0.1:8399/v1/chat/completions; no coverage verdict was reached — this candidate is unmeasured, not refused"}
```

## Pre-registration, and what the run did to it

Three predictions were written down **before** the POST (during boot
3's own probe), from reading the floor document and assay's gate:

1. The verdict will be **exit 2, `refused`**, on instrument grounds —
   the floor is `probe_version 0.12.0` and the live pin serves 0.13.0,
   and `cover_identity_gate` demands exact equality.
2. `model.name` stays informational: the scratch name
   `qwen3-14b-flywheel2!swap-candidate` appears in `identity_notes`,
   never as the fatal term.
3. Boot 3's drift rows will read **`within-noise`** — assay's `diff`
   gate does not require `probe_version` equality, and `SEMANTIC_BREAKS`
   registers only `verdict.parallel` at `(0, 11, 0)`, which neither
   0.12.0 nor 0.13.0 straddles.

**Prediction 3 was refuted, and the refutation is a fact about
bloomery, not assay.** The rows read `instrument-changed (0.12.0/v10 ->
0.13.0/v10)` with `exit_code: null`. The prediction reasoned about
assay's gate and forgot bloomery has one of its own: `DriftGate` runs
`instrument_precheck` **before** `diff_argv`
(`crates/bloomery-daemon/src/drift.rs`, whose `InstrumentChanged` doc
says outright "The diff is never spawned"), so assay was never asked.
The daemon is stricter than the tool it drives, and this boot is the
first time that rule fired in production.

**Predictions 1 and 2 were not tested at all** — the job failed before
the cover step, so no candidate profile and no cover document exist.
The brief asked for the run's `identity_notes` quoted verbatim; the
honest answer is **there are none, because `cover` never ran**. What
can be said about the instrument question is in the next-but-one
section, derived from documents this run really produced and labelled
as a derivation, not as the endpoint's verdict.

## Why the probe failed: the admission gate closes on the scratch identity

**Two different `PagerError`s render as 422 on this surface, and this
boot could plausibly have produced either.** `map_error`
(`crates/bloomery-daemon/src/api_native.rs`, at this run's HEAD
`a236de4`) maps both:

```rust
PagerError::Unprofiled(model) => (422, json!({"error": "unprofiled", "model": model})),
PagerError::DriftBlocked { model, reference } => (
    422,
    json!({"error": "drift_blocked", "model": model, "reference": reference}),
),
```

`DriftBlocked` is the rival explanation and it is not far-fetched: boot
3's drift rows read `instrument-changed`, and `Pager::admit` checks the
standing admission block **first**, before the profile gate. So the
snippet below starts at that branch rather than after it. Verbatim from
`crates/bloomery-daemon/src/pager.rs:664-688` **as it stood at
`a236de4`**, the commit this run's binary was built from (the file has
since moved — a later fix commit `dc0ae2d` changed `admit`; nothing
below describes the current tree):

```rust
    fn admit(&mut self, model: &str) -> Result<(), PagerError> {
        // Design §2. Checked before the existence gate so a blocked model
        // reports the reason that actually applies: it HAS a profile, and
        // that is precisely why a regression against it could be measured.
        if let Some(block) = self
            .models
            .get(model)
            .and_then(|e| e.admission_block.as_ref())
        {
            return Err(PagerError::DriftBlocked {
                model: model.to_string(),
                reference: block.reference.clone(),
            });
        }
        let has_profile = self
            .models
            .get(model)
            .is_some_and(|entry| entry.profile.is_some());
        if has_profile {
            return Ok(());
        }
        let posting = self.posting;
        if !posting && !self.allow_unprofiled {
            return Err(PagerError::Unprofiled(model.to_string()));
        }
```

**The `DriftBlocked` branch is excluded by observation, not by
argument.** `/status`, read immediately before run 1's POST, carried
`"admission_block": null` for `qwen3-14b-flywheel2` — no block stood,
so that first branch could not have fired. (It could not have fired for
the scratch identity either, which had just been registered and carried
no block, but the reading above settles it without needing that
reasoning.) `instrument-changed` is advisory: it is not one of the
outcomes that installs an admission block, which is why the row and the
null block coexist.

That leaves the second refusal, and its three terms are all pinned by
this run: the swap job's scratch identity has no profile — producing one
is the whole point of the probe; `allow_unprofiled` is not set in the
standing config; and `posting` is a **daemon-global** flag owned by the
boot-time POST window, which had closed ~37 seconds before run 1's POST
(the boot probe's own provenance finishes at `03:53:09Z`; the POST is at
`03:53:46Z`) — `/status` read `"posting": false` immediately before the
request, and again before run 2. So agent creation for
`qwen3-14b-flywheel2!swap-candidate` is refused 422 at the door, assay
gets a 422 from `/v1/chat/completions`, exits **4** (its own
infrastructure-failure code, not one of `cover`'s four), and the job
reports it as `infra:` — unmeasured, explicitly not a verdict.

**Design §4 step 2 says "probe it through the daemon's own `/v1`", and
on this daemon that is currently impossible outside the boot POST
window.** The endpoint's error handling is exactly right — it named the
probe's own words, invented no verdict, journaled `Degraded`, cleaned up
and released the slot — but the happy path has never been reachable in
production.

The near-miss is worth naming because it is worse than the failure: a
swap-candidate POST fired *during* the boot POST window would have been
admitted, by the global flag, for a reason that has nothing to do with
this endpoint. This slice would then have appeared to work, intermittently
and by accident of timing.

**What a fix needs** (named, not built — this task is acceptance): a
provisional admission scoped to the candidate job and its scratch
identity, journaled per model the way POST's already is, rather than the
daemon-global `posting` flag. That is new pager surface, and it belongs
to a follow-up slice with its own tests.

## The second blocker, standing behind the first

Even had the probe succeeded, this pair could not have produced a
coverage verdict today. The standing floor was written by assay
**0.12.0**; anything probed under the live pin is **0.13.0**; and
`cover_identity_gate` is strict. Run on the two real documents this
boot holds — the blessed floor and boot 3's own current fw2 profile,
both genuine, neither synthesised:

```
$ PYTHONPATH=/home/brice/workspace/assay/src python3 -m assay cover \
    qwen3-14b-flywheel2.baseline.json qwen3-14b-flywheel2.json --json …
not comparable:
  probe_version must match exactly: '0.12.0' -> '0.13.0'
EXIT=2
```

The `--json` document, all seven keys, nothing elided:

```json
{"comparable": false,
 "identity_notes": ["probe_version must match exactly: '0.12.0' -> '0.13.0'"],
 "uncovered": [], "covered": [], "incomplete": [], "ignored": [],
 "incomparable": []}
```

The four empty cell lists are `cover_profiles`' documented behaviour on
a refused pair — "A refused pair reports NOTHING beyond the notes" — not
a coverage result of zero.

**This is a derivation, not the endpoint's verdict** — the candidate
here is fw2's own current profile, not flywheel1, because flywheel1's
profile does not exist. It establishes only that pre-registered
prediction 1's *mechanism* is real and firing on this box: the exit-2
instrument refusal reproduces on a document pair produced 12 hours
apart across an assay release. Prediction 2 stays untested — with no
crossed-name pair to feed it, no `model.name (informational)` note was
observed, and none is quoted here.

Instrument identity of every document involved:

| document | probe_version / schema | mode | probe wall (from its own provenance) |
|---|---|---|---|
| `…baseline.json` (the floor) | 0.12.0 / v10 | quick | `2026-08-19T14:28:59Z → 14:38:45Z` (9m46s) |
| `…previous.json` | 0.12.0 / v10 | quick | `2026-08-19T14:41:48Z → 14:51:33Z` (9m45s) |
| `…json` (boot 3's current) | **0.13.0** / v10 | quick | `2026-08-20T03:42:51Z → 03:53:09Z` (**10m18s**) |
| the candidate's | — | — | **never written** |

All three carry `tier=enthusiast-16gb emulated=False`, so the hardware
half of the gate agrees; only the instrument half disagrees.

## What this means for the fw1-vs-fw2 pair

**Nothing.** No claim about flywheel1's admissibility as a substitute
for flywheel2 is supported by this run, in either direction. The
candidate's weights were read (digested, 9.0 GB) and never loaded: GPU
held steady at ~10,020 MiB through both candidate jobs — flywheel2's
resident weights and nothing else — where a real candidate probe would
have shown a second multi-GB load.

GPU readings are ephemeral, so the durable proof is the journal census
of `boot-1787197252.jsonl`, which anyone holding the file can repeat:

```
$ grep -c '"event"' boot-1787197252.jsonl                              # 559
$ grep -c '"event":"AgentCreated"' boot-1787197252.jsonl               # 111
$ grep -c '"event":"AgentCreated".*"model":"qwen3-14b-flywheel2"' …    # 111
$ grep -c '"event":"AgentCreated".*swap-candidate' …                   #   0
$ grep -c 'swap-candidate' boot-1787197252.jsonl                       #   2
```

**Every one of the 111 agents this boot created named
`qwen3-14b-flywheel2`; none named the scratch identity.** Only 2 of the
559 rows mention `swap-candidate` at all, and they are the byte-equal
`Degraded` pair above. The candidate never got an agent, so it never
got an inference, so it was never loaded — which is the same fact the
422 predicts, arrived at from the other side.

The question the acceptance was
designed to answer is still open, and answering it needs two fixes
first:

1. the admission gate above, so the candidate can be probed at all; and
2. a floor and a candidate measured by the same assay — which on this
   box means re-blessing `qwen3-14b-flywheel2` under 0.13.0 (a
   deliberate operator act, `POST /models/{name}/bless`, deliberately
   not performed here: nothing in this task's remit authorises moving
   the standing lineage's blessed baseline).

What the run *does* establish, and it is not nothing: the surface's
refusals, its asynchrony, its handover notes, its cleanup and its
failure discipline all behave in production exactly as Tasks 1–4 built
them, including on a path nobody scripted.

## GPU and shutdown

| moment | GPU used |
|---|---|
| before boot 3 | 1821 MiB |
| boot 3 probe running (fw2 loaded) | 14,319–14,480 MiB |
| between jobs, weights resident | 10,050 MiB |
| both candidate jobs, throughout | 9,986–10,023 MiB (**no second load**) |
| after shutdown | **1709 MiB** |

Shutdown was by verified PID, never by name:

```
$ readlink /proc/972344/exe
/home/brice/workspace/bloomery/.worktrees/seam/target/debug/bloomery-daemon
$ kill -TERM 972344
$ pgrep -x bloomery-daemon        # empty
$ ss -ltn | grep 8399             # not listening
```

The scratch identity did not outlive either request: `/status` after
run 1 listed `['qwen3-14b-flywheel2']` only, and no
`qwen3-14b-flywheel2!swap-candidate.confirm.json` was left behind in
the profiles directory — design §4's cleanup law held on the failure
path, which is the path it exists for.

## What boot 3 left in the standing home

Every artifact this document quotes is durable on the box — in the
standing home, not in the repo and not in a worktree's `target/`, the
same convention `2026-08-19-standing-v10-baseline.md` established:

```
/home/brice/.local/share/bloomery/drift/
├── bloomery-drift.toml                    # unchanged by this run
├── boot1.log  boot2.log                   # the standing-baseline boots
├── boot3.log                              # NEW — this run (557,942 bytes)
└── data/
    ├── journal/boot-1787149620.jsonl      # boot 1 (556 rows)
    ├── journal/boot-1787150388.jsonl      # boot 2 (557 rows)
    ├── journal/boot-1787197252.jsonl      # NEW — boot 3 (559 rows)
    └── profiles/qwen3-14b-flywheel2.{baseline,previous,}.json
```

Boot 3 added two files — `boot3.log` (557,942 bytes) and
`boot-1787197252.jsonl` (559 rows) — and moved the profile documents on
one rung, the rotation the drift watch runs every boot. State after
shutdown, read off the files:

| document | sha256 (first 8) | probe_version | its probe started |
|---|---|---|---|
| `…baseline.json` | `f2a2cabc` | 0.12.0 | `2026-08-19T14:28:59Z` |
| `…previous.json` | `179b706e` | 0.12.0 | `2026-08-19T14:41:48Z` |
| `…json` (current) | `34cc643b` | **0.13.0** | `2026-08-20T03:42:51Z` |

`previous.json` held `f2a2cabc` before this boot and holds `179b706e`
after it, so the rotation ran; its mtime is boot 2's, inherited because
rotation renames the file rather than rewriting it. **The blessed
baseline is byte-identical to what boot 1 blessed** — `f2a2cabc`, the
same digest the drift-**cumulative** row names as `reference_sha` and
both swap reports name as `floor_sha` (the drift-step row's
`reference_sha` is `179b706e`, the previous rung). Nothing in this run
moved it.

No `qwen3-14b-flywheel2!swap-candidate.confirm.json` exists: the
candidate profile named in both reports was never written, because the
probe that would have written it never produced one.

## What ran, verbatim

```bash
# preflight
PYTHONPATH=/home/brice/workspace/assay/src python3 -c "import assay; print(assay.__version__)"   # 0.13.0
cd /home/brice/workspace/assay && ./.venv/bin/python -m pytest -q                                # 1073 passed
cd /home/brice/workspace/bloomery/.worktrees/seam
cargo test -p bloomery-core -p bloomery-daemon                                                   # 46 suites, 566 passed
cargo build -p bloomery-daemon --features vulkan                                                 # featured binary LAST

# boot 3 of the standing lineage
PYTHONPATH=/home/brice/workspace/assay/src \
  ./target/debug/bloomery-daemon --config /home/brice/.local/share/bloomery/drift/bloomery-drift.toml \
  > /home/brice/.local/share/bloomery/drift/boot3.log 2>&1 &
tr '\0' '\n' < /proc/972344/environ | grep '^PYTHONPATH='   # the pin, read back off the live process

# the question, and both controls
curl -X POST :8399/models/qwen3-14b-flywheel2/swap-candidate \
  -d '{"gguf_path":"/home/brice/flywheel1/qwen3-14b-flywheel1-Q4_K_M.gguf"}'
curl -X POST :8399/models/qwen3-14b-flywheel2/swap-candidate  -d '…'   # control: 409
curl -X POST :8399/models/qwen3-14b-nonexistent/swap-candidate -d '…'  # control: 404
curl :8399/models/qwen3-14b-flywheel2/swap-candidate                   # polled to done

# shutdown by verified PID
readlink /proc/972344/exe && kill -TERM 972344
```

Nothing was wrapped in `timeout` (this box's `timeout` segfaults on
multithreaded children), and nothing was killed by bare name. The
standing config was not edited, the blessed baseline was not moved, and
the assay working tree was not touched.
