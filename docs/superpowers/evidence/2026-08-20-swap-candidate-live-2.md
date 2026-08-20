# Swap-candidate live acceptance #2 — the endpoint reaches a verdict

**Date:** 2026-08-20 (boot 4 of the standing lineage, 2026-08-19 23:58 CDT →
2026-08-20 00:37 CDT; all timestamps below are UTC, i.e. `2026-08-20T04:58Z →
05:37Z`). ~39 min wall: one 10m30s boot probe, one delegated bless, two
candidate jobs (≤4m42s and ≤13m09s, bounded by the polls that observed them
done), and shutdown.
**Context:** the second live acceptance the swap-candidate seam design (§8)
pins. Run 1
(`docs/superpowers/evidence/2026-08-19-swap-candidate-live.md`) could not
reach a verdict: the probe was refused admission `422` outside the boot POST
window, and the floor was `0.12.0` against a `0.13.0` pin. This run carries
the fix for the first (commits `dc0ae2d` + `8c6acd6`, code that had **never
met a real GPU**) and clears the second by an **operator bless delegated to
this run by Brice**.
**bloomery:** worktree `.worktrees/seam`, branch `swap-candidate-seam` at
`8c6acd6`. Suite green before the boot: 46 suites, **573 passed, 0 failed**
(`cargo test -p bloomery-core -p bloomery-daemon`), featured binary built
**after** the tests (`cargo build -p bloomery-daemon --features vulkan`,
verified to link `libvulkan.so.1`) — the standing-v10 doc's two traps.
**assay:** master at `bdb7f92` (PR #6 merge, v1.11), **0.13.0**, tree clean,
`cover` present in `--help`. Suite green: **1073 passed**.
**Box/tier:** `enthusiast-16gb`, `emulated = false` — RTX 5080 16 GB, Vulkan.
GPU **1714 MiB** (desktop) before the boot; **1713 MiB** after shutdown with
`pgrep -x bloomery-daemon` empty.
**Model:** `/home/brice/flywheel2/qwen3-14b-flywheel2-Q4_K_M.gguf`, configured
as `qwen3-14b-flywheel2`, standing config
`~/.local/share/bloomery/drift/bloomery-drift.toml` **unchanged by this run**.
**Candidate:** `/home/brice/flywheel1/qwen3-14b-flywheel1-Q4_K_M.gguf`
(9,001,752,960 bytes; sha256
`80bc0ea9042aef7f5fa16ab64e641d5ed30da6f7c2faf6da1084a388d26ff293`) — the same
candidate run 1 named, digest unchanged.
**Pre-registration:**
`.superpowers/sdd/2026-08-19-swap-candidate-endpoint/prereg-2.md`. Written and
stamped **`2026-08-19 23:58:28 -0500`**, before boot 4 — but the run-2b
amendment was appended **in place**, so the file now stats
**`2026-08-20 00:21:44 -0500`** and the earlier mtime survives only in this
document and in the task report. `.superpowers/sdd/` is gitignored, so there
is no committed copy to check either against. **The pre-boot mtime is
therefore an attested reading, not a durable artifact** — stated plainly
rather than left for a reader to trip over. *Process fix for future runs:
write each amendment as its own file, so every stage carries its own immutable
mtime and no timestamp is overwritten by a later append.*

---

## Verdict

**`covered`. Exit 0.** The endpoint reached a coverage verdict for the first
time. As a contiguous string this is the **journal row's** spelling — in the
`GET` body the two fields are separated by `floor_sha` and `notes`, so the
durable artifact is the one quoted here:

```
"exit_code":0,"outcome":"covered"
```

**What it means, stated inside what the POST profile actually measures:** on
all **34** cells fw2's blessed floor measured, fw1 did **not** rank below,
under each cell's own noise discipline. Every one of the 34 landed in
`within_noise` — **zero** cells changed in either direction. §"What `covered`
rests on" below is the load-bearing caveat and must be read with this line:
much of that floor is `unusable`, and at `n=5` the codec cells are nearly
incapable of reporting a regression at all.

| # | route | HTTP | result |
|---|---|---|---|
| 1 | boot 4's POST probe | — | `ok`, 0.13.0/v10, 10m30s |
| 2 | `POST /models/qwen3-14b-flywheel2/bless` | **200** | baseline `f2a2cabc` → `fe05f3b3` |
| 3 | `POST …/swap-candidate` (**run 2a**) | **202 → 200** | `infra: … HTTP 503 …`, `exit_code: null` — **a new, third blocker** |
| 4 | `POST /models/qwen3-14b-flywheel2/unload` | **204** | operator act; `loaded_weights_bytes: 0` |
| 5 | `POST …/swap-candidate` (**run 2b**) | **202 → 200** | **`covered`, `exit_code: 0`** |

**One provenance caveat on run 2a.** A failed job journals `Degraded`, not
`SwapCandidate`, and `boot4.log` carries no HTTP logging — so **run 2a's
`floor_sha` rests on the observed `GET` body alone**, with no durable artifact
behind it. Run 2b's `floor_sha` is in the `SwapCandidate` row and is durable.

**Both runs are reported.** Run 2a is not a discarded attempt: it is a live
finding in its own right, and the re-run is licensed by the brief's one
exception — *"a re-run is legitimate only for a named infra failure"*. The
failure was named by the daemon itself (`residency_refused`, HTTP 503), the
remedy used only existing shipped surface, and the amendment recording all of
it was written **before** run 2b, not after.

| comparison | reference | current | outcome | exit code |
|---|---|---|---|---|
| boot 4 drift-**step** | `…previous.json` `34cc643b` (0.13.0/v10) | boot 4's `fe05f3b3` (0.13.0/v10) | **`within-noise`** | **0** — the diff really ran |
| boot 4 drift-**cumulative** | `…baseline.json` `f2a2cabc` (0.12.0/v10) | boot 4's (0.13.0/v10) | **`instrument-changed`** | `null` |
| swap coverage, run 2a | `…baseline.json` `fe05f3b3` | *never written* | **`infra:` — unmeasured** | `null` |
| swap coverage, **run 2b** | `…baseline.json` `fe05f3b3` | `…transient-f8b7c60b.json` | **`covered`** | **0** |

## The delegated bless — this run's durable state change

**The one thing this run did that outlives the daemon.** Delegated by Brice
("do as recommended"); no task before this one was authorised to move the
standing lineage's blessed baseline.

```
$ curl -X POST :8399/models/qwen3-14b-flywheel2/bless        # 2026-08-20T05:14:15Z
HTTP 200
{"model":"qwen3-14b-flywheel2","path":"/home/brice/.local/share/bloomery/drift/data/profiles/qwen3-14b-flywheel2.baseline.json","sha":"fe05f3b348abf8fc79b191cda7c408143a827e17176ee383d212fb4d7f0f7373"}
```

The journal row, verbatim — and it names the digest it replaced, so the
rotation is durable in the ledger and not only in this document:

```json
{"event":"Blessed","model":"qwen3-14b-flywheel2","profile_path":"/home/brice/.local/share/bloomery/drift/data/profiles/qwen3-14b-flywheel2.baseline.json","sha":"fe05f3b348abf8fc79b191cda7c408143a827e17176ee383d212fb4d7f0f7373","provenance":"operator (replaced f2a2cabc360a7423f5f963975103672cda04242c55fc04897ee0e2aa5a5b1b98)"}
```

`sha256sum` of the standing home's profile documents, before and after the
one request — repeatable by anyone holding the files:

| document | **pre-bless** | **post-bless** |
|---|---|---|
| `…baseline.json` | `f2a2cabc` (0.12.0/v10) | **`fe05f3b3`** (0.13.0/v10) |
| `…previous.json` | `34cc643b` (0.13.0/v10) | `34cc643b` — **untouched** |
| `…json` (current) | `fe05f3b3` (0.13.0/v10) | `fe05f3b3` — **untouched** |

Full digests, post-bless:

```
fe05f3b348abf8fc79b191cda7c408143a827e17176ee383d212fb4d7f0f7373  qwen3-14b-flywheel2.baseline.json
fe05f3b348abf8fc79b191cda7c408143a827e17176ee383d212fb4d7f0f7373  qwen3-14b-flywheel2.json
34cc643b1ef321b2f30b3a0a204bdd313b1c33ff724885013c0bf74cdcbc968c  qwen3-14b-flywheel2.previous.json
```

**Baseline and current are now byte-identical**, which is `ProfileStore::bless`
doing exactly what its doc says — a *copy*, not a move
(`crates/bloomery-daemon/src/drift.rs:334-358` at `8c6acd6`), so the current
document survives for the drift-step machinery. **`f2a2cabc` no longer exists
anywhere in the standing home**: boot 4's rotation overwrote `previous.json`
(which had held it until boot 3) and the bless overwrote `baseline.json`.
**From this run forward the standing home is a 0.13.0 home**, and the next
boot's cumulative comparison will be the first since boot 2 that can spawn a
diff against the blessed baseline — and the first ever under 0.13.0.

The bless took effect **immediately for the swap job**, not at the next boot:
both candidate jobs report `floor_sha: fe05f3b3…`, never `f2a2cabc`. That is
`swap/job.rs` reading `store.paths(model).baseline` at job time, and it
confirms the pre-registration's reading of the bless docstring's "takes effect
at the NEXT boot" as applying to the *drift* reading only.

## What the endpoint answered, verbatim

### Run 2b — the verdict

`POST` at `05:22:00Z`:

```
HTTP 202
{"candidate":"/home/brice/flywheel1/qwen3-14b-flywheel1-Q4_K_M.gguf","model":"qwen3-14b-flywheel2","state":"running"}
```

`GET` at `05:35:16Z`; the report was already `done` at the `05:35:09Z` poll, so
≤13m09s after the POST. The whole body, nothing elided:

```json
{"model":"qwen3-14b-flywheel2","report":{"candidate_gguf_sha":"80bc0ea9042aef7f5fa16ab64e641d5ed30da6f7c2faf6da1084a388d26ff293","candidate_profile_path":"/home/brice/.local/share/bloomery/drift/data/profiles/qwen3-14b-flywheel2!swap-candidate.transient-f8b7c60b.json","exit_code":0,"floor_sha":"fe05f3b348abf8fc79b191cda7c408143a827e17176ee383d212fb4d7f0f7373","notes":["done_trust/G4/G5 are unmeasured for this candidate until its first real boot with tasks enabled","on swap: edit config, restart; the next boot reads not-comparable against the old lineage's baseline until you POST /models/{name}/bless"],"outcome":"covered"},"state":"done"}
```

**Both `notes` came back on the happy path too**, exactly as §4's amendment
promises and exactly as they came back on run 1's failure path — the operator
handover §5 owes, unchanged by the verdict:

> `done_trust/G4/G5 are unmeasured for this candidate until its first real boot with tasks enabled`

> `on swap: edit config, restart; the next boot reads not-comparable against the old lineage's baseline until you POST /models/{name}/bless`

### Run 2a — the new blocker, in the endpoint's own words

`POST` at `05:14:34Z`; the terminal journal row was present by the `05:19:16Z`
poll, so ≤4m42s. The `outcome`, verbatim:

```
infra: the candidate probe for qwen3-14b-flywheel2 (registered as qwen3-14b-flywheel2!swap-candidate) failed: assay exited 4: assay: infrastructure failure: HTTP 503 from http://127.0.0.1:8399/v1/chat/completions; no coverage verdict was reached — this candidate is unmeasured, not refused
```

Run 1's sentence was **byte-identical except for three characters** — the
status code, `422` there and `503` here. Checked, not asserted: the two full
`outcome` strings are both 289 characters and differ **only at offsets 162,
163 and 164** (offsets into the whole `infra: …` sentence; within the inner
`assay: infrastructure failure: …` substring, which begins at offset 126, the
same three characters sit at 36–38). The endpoint's failure
discipline behaved the same on
a failure nobody had scripted — it named the probe's own words, invented no
verdict, journaled `Degraded`, cleaned up and released the slot.

## The journal rows, verbatim

All from `~/.local/share/bloomery/drift/data/journal/boot-1787201923.jsonl`
(**1132 rows**). Boot 4's drift-relevant rows, at lines 1, 2, 553, 554, 555:

```json
{"event":"Boot","version":"0.1.0"}
{"event":"Degraded","reason":"provisional admission: qwen3-14b-flywheel2 has no profile yet; POST in progress"}
{"event":"Post","model":"qwen3-14b-flywheel2","outcome":"ok","profile_path":"/home/brice/.local/share/bloomery/drift/data/profiles/qwen3-14b-flywheel2.json"}
{"event":"Drift","model":"qwen3-14b-flywheel2","comparison":"step","outcome":"within-noise","reference_path":"/home/brice/.local/share/bloomery/drift/data/profiles/qwen3-14b-flywheel2.previous.json","current_path":"/home/brice/.local/share/bloomery/drift/data/profiles/qwen3-14b-flywheel2.json","exit_code":0,"reference_sha":"34cc643b1ef321b2f30b3a0a204bdd313b1c33ff724885013c0bf74cdcbc968c","current_sha":"fe05f3b348abf8fc79b191cda7c408143a827e17176ee383d212fb4d7f0f7373"}
{"event":"Drift","model":"qwen3-14b-flywheel2","comparison":"cumulative","outcome":"instrument-changed (0.12.0/v10 -> 0.13.0/v10)","reference_path":"/home/brice/.local/share/bloomery/drift/data/profiles/qwen3-14b-flywheel2.baseline.json","current_path":"/home/brice/.local/share/bloomery/drift/data/profiles/qwen3-14b-flywheel2.json","exit_code":null,"reference_sha":"f2a2cabc360a7423f5f963975103672cda04242c55fc04897ee0e2aa5a5b1b98","current_sha":"fe05f3b348abf8fc79b191cda7c408143a827e17176ee383d212fb4d7f0f7373"}
```

**The step row is the first comparison ever run under 0.13.0 documents**, and
the first `assay diff --gate` this lineage has spawned since the instrument
moved. It exited 0.

**A correction, and the pre-registration is where it comes from.** The prereg
asserted that boot 4 would be "the first boot in the standing lineage where
`assay diff --gate` actually runs — every previous step and cumulative row in
boots 1–3 was either bootstrap or `instrument-changed`". **That sentence is
false, and the standing home refutes it**; it was written without checking
boots 1 and 2, and the durable artifacts are what caught it:

```
$ grep -o '"comparison":"[^"]*","outcome":"[^"]*"' boot-1787150388.jsonl      # boot 2
"comparison":"step","outcome":"within-noise"
"comparison":"cumulative","outcome":"within-noise"
```

Boot 2 ran **both** comparisons to `exit_code: 0` on 0.12.0 documents. Boot 1
was bootstrap (`unmeasured: … no such file`), boot 3 was `instrument-changed`
on both. The **prediction** P1 — that boot 4's step row would carry a non-null
`exit_code` — was right; the *rationale* around it overstated the novelty, and
saying so here is cheaper than leaving a wrong sentence in the record.

**The `SwapCandidate` row**, line 1131 — the row run 1 never got:

```json
{"event":"SwapCandidate","model":"qwen3-14b-flywheel2","candidate_gguf_sha":"80bc0ea9042aef7f5fa16ab64e641d5ed30da6f7c2faf6da1084a388d26ff293","floor_path":"/home/brice/.local/share/bloomery/drift/data/profiles/qwen3-14b-flywheel2.baseline.json","floor_sha":"fe05f3b348abf8fc79b191cda7c408143a827e17176ee383d212fb4d7f0f7373","candidate_profile_path":"/home/brice/.local/share/bloomery/drift/data/profiles/qwen3-14b-flywheel2!swap-candidate.transient-f8b7c60b.json","candidate_profile_sha":"f8b7c60bb4eb4a8636b424b1d7a59e21e24cfb32402f62e818031375917b3f68","exit_code":0,"outcome":"covered"}
```

Both profile shas are there; the model named is the **configured** model, never
the scratch identity; `floor_sha` is the **post-bless** digest.

The four `Degraded` rows, at lines 2, 557, 578, 580 — the whole degraded
history of this boot, printed in full:

```json
{"event":"Degraded","reason":"provisional admission: qwen3-14b-flywheel2 has no profile yet; POST in progress"}
{"event":"Degraded","reason":"provisional admission: qwen3-14b-flywheel2!swap-candidate has no profile yet; a candidate probe is measuring it"}
{"event":"Degraded","reason":"swap: the candidate probe for qwen3-14b-flywheel2 (registered as qwen3-14b-flywheel2!swap-candidate) failed: assay exited 4: assay: infrastructure failure: HTTP 503 from http://127.0.0.1:8399/v1/chat/completions; no coverage verdict was reached — this candidate is unmeasured, not refused"}
{"event":"Degraded","reason":"provisional admission: qwen3-14b-flywheel2!swap-candidate has no profile yet; a candidate probe is measuring it"}
```

**Rows 557 and 580 are `dc0ae2d`'s new sentence, live.** Run 1's journal
contained neither. Exactly one per job, across 5 admissions on run 2a and 111 on run 2b —
`provisional_logged` doing its job. The two jobs admitted **5** (run 2a, all
refused by the scheduler) and **111** (run 2b) — the split census below — so
without dedup the boot would carry **116** scratch provisional rows, or 227
counting fw2's own 111.

The weight movements, all four, at lines 5, 579, 583, 1132:

```json
{"event":"ModelLoaded","model":"qwen3-14b-flywheel2","duration_ms":30400}
{"event":"ModelUnloaded","model":"qwen3-14b-flywheel2"}
{"event":"ModelLoaded","model":"qwen3-14b-flywheel2!swap-candidate","duration_ms":22436}
{"event":"ModelUnloaded","model":"qwen3-14b-flywheel2!swap-candidate"}
```

**Line 583 is the fact run 1 explicitly could not produce.** Run 1 proved fw1
was never loaded; here fw1's weights were loaded in 22.4 s under the scratch
identity, and unloaded again at step 7. Line 579 is the operator `unload` —
journaled, so the one deviation this run made is in the ledger, not only in
this document.

One `Refusal` row from run 2a, printed once — the five are identical **apart
from the agent id** (`a112`–`a116`; stripping `id` leaves exactly one distinct
body):

```json
{"event":"Refusal","id":"a112","needed_tokens":26478,"window_tokens":26478,"detail":"residency: weights 9001752960 B + reserved 4740808704 B (kv 4338155520 B + ctx overhead 402653184 B) vs budget 14816378880 B − overhead 1073741824 B − loaded 9001752960 B − resident 0 B (needed 13742561664 B, free 4740884096 B, reclaimable 0 B)"}
```

## Durable post-conditions

HTTP bodies and GPU readings are ephemeral. Everything below is a command
anyone holding the standing home can re-run, against
`boot-1787201923.jsonl` and the profiles directory.

```
$ grep -c '"event"' boot-1787201923.jsonl                                    # 1132
$ grep -c '"event":"AgentCreated"' boot-1787201923.jsonl                     #  227
$ grep -c '"event":"AgentCreated".*"model":"qwen3-14b-flywheel2"' …          #  111   (fw2, boot probe)
$ grep -c '"event":"AgentCreated".*swap-candidate' …                         #  116   (5 + 111)
$ grep -c '"event":"AgentRemoved"' …                                         #  227
$ grep -c '"event":"AgentRemoved".*ephemeral cleanup' …                      #  227
$ grep -c 'cannot outlive the registration' …                                #    0
$ grep -c '"event":"SwapCandidate"' …                                        #    1
$ grep -c '"event":"Blessed"' …                                              #    1
$ grep -c '"decision":"fits"' …    /    '"decision":"refuse"' …              #  216 / 5
$ grep -c '"event":"Refusal"' …                                              #   11
$ grep -c '"event":"Refusal".*residency:' …                                  #    5
$ grep -c '"event":"Refusal".*exceeds the computed window' …                 #    6
$ grep -c 'swap-candidate' …                                                 #  122
```

**The 11 `Refusal` rows split 5 + 6, and the six are worth a sentence.** Five
are run 2a's residency refusals (lines 560, 564, 568, 572, 576). The other six
read `prompt + max_tokens exceeds the computed window` and fall **three in
fw2's boot probe** (lines 35, 43, 46) and **three in fw1's candidate probe**
(lines 613, 621, 624). The same count on each side is what one expects if the
same fixture set met the same context window under the same instrument — **a
small independent corroboration, from the daemon's ledger rather than from the
profiles, of the "same instrument on both sides" the coverage verdict rests
on.**

They also reconcile the two artifacts exactly: `boot4.log` constructs **216**
contexts, and `227 AgentCreated − 11 Refusal = 216` — every agent that was not
refused got a context, and none was built for one that was.

Split by job (run 2a is lines ≤578, run 2b is ≥580):

```
$ sed -n '1,578p'  … | grep -c '"event":"AgentCreated".*swap-candidate'      #    5   (all refused)
$ sed -n '580,$p'  … | grep -c '"event":"AgentCreated".*swap-candidate'      #  111   (all placed)
```

**111 is exactly the count fw2's own boot probe produced, and exactly the
`calls: 111` the candidate's profile records in its own provenance.** Run 1's
count for the scratch identity was **0**, and that zero was how run 1 knew the
candidate had never been inferred. This 111 is the same proof from the other
side.

**`/status` after `done`** — no scratch model, no scratch agents:

```json
"agents": [], "models": [{ … "name": "qwen3-14b-flywheel2" … }], "loaded_weights_bytes": 0
```

**The retained candidate profile exists beside the drift transients, and its
sha matches the journal row:**

```
$ sha256sum 'qwen3-14b-flywheel2!swap-candidate.transient-f8b7c60b.json'
f8b7c60bb4eb4a8636b424b1d7a59e21e24cfb32402f62e818031375917b3f68
```

Byte-equal to the row's `candidate_profile_sha`, and its first 8 hex
characters are the `transient-f8b7c60b` in its own file name — the
content-addressed name and the ledger agree. No
`…!swap-candidate.confirm.json` staging file was left behind: retention
*moved* it, as `ProfileStore::retain_transient` documents. Its identity:

| field | value |
|---|---|
| `model.name` | `qwen3-14b-flywheel2!swap-candidate` |
| `probe_version` / schema | **0.13.0 / v10** |
| mode / tier / emulated | `quick` / `enthusiast-16gb` / `false` |
| probe wall | `2026-08-20T05:26:03Z → 05:34:37Z` (8m34s) |
| spent | `calls: 111, prompt_tokens: 95420` |

### `identity_notes`, the question run 1 could not answer

**This is a re-derivation, and it is labelled one.** `cover_argv`
(`crates/bloomery-daemon/src/swap.rs:35-46` at `8c6acd6`) invokes `assay
cover <floor> <candidate>` with **no `--json`**, and `CoverGate::check` reads
the **exit code only** — so the daemon writes no cover document and there is
nothing to quote from the job itself. This caveat was registered in advance
(prereg §3a). The command below is the daemon's own argv with `--json`
appended, run on the **two durable documents this run produced**, and `cover`
is pure, so it reproduces for anyone:

```
$ PYTHONPATH=/home/brice/workspace/assay/src python3 -m assay cover \
    qwen3-14b-flywheel2.baseline.json \
    'qwen3-14b-flywheel2!swap-candidate.transient-f8b7c60b.json' --json …
cover: covered
  note: model.name (informational): 'qwen3-14b-flywheel2' -> 'qwen3-14b-flywheel2!swap-candidate'
  covered: 34 cell(s)
EXIT=0
```

The `--json` document's own account — **exactly one note, and it is
informational**:

```json
{"comparable": true,
 "identity_notes": ["model.name (informational): 'qwen3-14b-flywheel2' -> 'qwen3-14b-flywheel2!swap-candidate'"],
 "uncovered": [], "incomplete": [], "ignored": [], "incomparable": [],
 "covered": [ … 34 cells, listed in full below … ]}
```

**Run 1's pre-registered prediction 2 is now tested and holds:** the scratch
name appears in `identity_notes` as informational and **never as the fatal
term**. The endpoint's premise — that `cover` is the crossed-pair comparison
that *is* supported — is confirmed against a real crossed pair.

## Run 2a's finding: a third blocker, in the pager's residency arithmetic

**The `dc0ae2d`/`8c6acd6` fix worked on first contact with a real GPU.** Run
1's 422 is gone: the probe was *admitted*, and the journal proves it (a
provisional-admission row for the scratch identity and five `AgentCreated`
rows under it, where run 1 had zero of each). The window opened, was scoped to
the scratch identity, and closed.

**A residency refusal took its place — and the spec had already ruled its
shape.** The seam design's §4 step-1 amendment (`ruling bT3/R1 — the 409
disposition`, 2026-08-19,
`docs/superpowers/specs/2026-08-19-swap-candidate-seam-design.md:139-171`)
routes an unplaceable candidate to *precisely* this outcome, and run 2a
matched it clause for clause:

> **Ruled disposition:** the reservation refusal surfaces through the
> probe's own failure, where it is real. The candidate is probed
> through this daemon's own `/v1`, which renders `PagerError::Refused`
> as `503 residency_refused` carrying the arithmetic in its message;
> the probe then fails, the worker journals `Degraded`, and the report
> and its `GET` carry `infra: the candidate probe for {model} …
> failed: …` with the probe's own words.

**So the refusal shape is not a discovery — it is the spec working**, and an
earlier draft of this document wrongly called it unanticipated. The same
amendment also fixes the demand term as "that agent's own window-sized
reservation plus, for a cold model, its weights", which is exactly what the
`Refusal` detail below prints. **What IS new is the frequency: on this tier
the happy path is not one edge case away, it is unreachable without an
unload** — a 14B Q4_K_M standing model leaves no room for any candidate at
all, so the ruled failure path is the *default* path here rather than a
corner. The mechanism, pinned to `8c6acd6`:

`Pager::loaded_weights_bytes` (`crates/bloomery-daemon/src/pager/paging.rs:47-57`)
sums **every** model holding a substrate handle:

```rust
    pub(super) fn loaded_weights_bytes(&self) -> u64 {
        self.models
            .values()
            .filter(|m| m.handle.is_some())
            .fold(0u64, |acc, m| {
                // … three comment lines elided …
                acc.saturating_add(m.effective_weights_bytes())
            })
    }
```

and `place` charges that whole sum against the budget before planning
(`paging.rs:203-209`):

```rust
        let avail = match budget {
            Some(budget) => budget
                .saturating_sub(overhead)
                .saturating_sub(loaded_weights)
                .saturating_sub(resident_reserved),
            None => 0,
        };
```

while `plan_residency` (`crates/bloomery-core/src/scheduler.rs:89-91`) computes
`reclaimable` from **agents' KV bytes** — it evicts agents, never another
model's weights:

```rust
    let reclaimable: u64 = evictable
        .iter()
        .fold(0u64, |acc, r| acc.saturating_add(r.kv_bytes));
```

So with fw2's 9.0 GB resident from its own boot probe and **no agents at all**
(`reclaimable 0 B`), the candidate had to find its own 9.0 GB plus 4.74 GB of
reserve inside 4.74 GB of headroom. The refusal's own arithmetic, restated:

| term | bytes |
|---|---|
| budget | 14,816,378,880 |
| − overhead | 1,073,741,824 |
| − loaded (fw2's weights) | 9,001,752,960 |
| − resident KV | 0 |
| = **available** | **4,740,884,096** |
| **needed** (candidate weights 9,001,752,960 + reserved 4,740,808,704) | **13,742,561,664** |

**This is arithmetic on constants, not a race** — it refused five times
identically, so repeating run 2a unchanged was guaranteed to reproduce it.

**The finding, stated generally:** *design §4 step 2's happy path requires
headroom for **two** models' weights at once, and the pager has no mechanism
to make room by unloading a different model — it evicts KV, not weights.* On
`enthusiast-16gb` with a 14B Q4_K_M standing model, that headroom does not
exist. This is a design gap, not a bug in `dc0ae2d`/`8c6acd6`: the admission
window is necessary for the probe and, on this tier, not sufficient.

### The remedy, and why it is a deviation worth naming

`POST /models/{name}/unload` (`api_native.rs:86` → `Pager::unload_model`; `:85`
is the `resume` arm) is
**existing shipped surface** — an operator route of the same kind as the
delegated bless. It changed nothing about the comparison: the blessed floor
document was already on disk (`fe05f3b3`), the candidate was unchanged, the
instrument was unchanged. It changed only whether the candidate could be
probed at all.

Arithmetic computed **before** the attempt and recorded in the amendment:

| state | available | needed | fits |
|---|---|---|---|
| fw2 loaded (run 2a) | 4,740,884,096 | 13,742,561,664 | **no** |
| fw2 unloaded (run 2b, first call) | 13,742,637,056 | 13,742,561,664 | **yes, by 75,392 B** |
| fw1 loaded, subsequent calls | 4,740,884,096 | 4,740,808,704 | **yes, by 75,392 B** |

**The margin is 75,392 bytes — 0.0005% of the budget.** It is the same margin
fw2's own boot probe ran on for 111 consecutive calls, so it is not new; it is
worth naming because it means this tier has no slack at all, and a single
additional resident byte would have refused.

**This deviation is the run's one unplanned act**, it used no code change and
no config edit, it is journaled (`ModelUnloaded`, line 579), and its only
lasting effect was that fw2's weights were off the card at shutdown — a state
the next boot reverses by itself.

## What `covered` rests on — the honesty section

**A coverage verdict is only as strong as the floor it clears, and this floor
is low.** Stating that here is not hedging; omitting it would make the verdict
line misleading. Every number below is read off the two durable documents.

**All 34 covered cells were `within_noise`. Zero cells changed.** `covered`
here does not mean fw1 improved anywhere — it means nothing moved.

**This is a re-derivation, labelled like the others.** `cover` does not expose
the partition (its `covered` list merges cells that held still with cells that
improved), and `diff` refuses this crossed pair outright, so neither shipped
command prints it. It comes from calling `cover`'s own internal walker on the
two durable documents — reproducible, and pure:

```
$ python3 -c "from assay.diff import _families; pair = _families(floor, candidate); \
              print(len(pair.within_noise), len(pair.changes), len(pair.dropped))"
34 0 0
```

34 within-noise, **0 changes**, 0 dropped — which is also why `uncovered`,
`incomplete` and `ignored` are all empty in the `--json` document above.

**Five of the seven measured capability verdicts are `unusable` or degraded,
identically on both sides:**

| verdict cell | floor (fw2) | candidate (fw1) |
|---|---|---|
| `structured_extraction` | **unusable** | **unusable** |
| `patch_editing` | **unusable** (provisional) | **unusable** (provisional) |
| `loop_discipline` | **unusable** | **unusable** |
| `tool_calling` | **unusable** (`call_rate 0.0`) | **unusable** (`call_rate 0.0`) |
| `long_output` | **degrades-at-4096** | **degrades-at-4096** |
| `chat_speed` | ready | ready |
| `agent_speed` | ready | ready |

**Twelve of the eighteen codec cells are `0.0` on both sides** — covering a
zero is trivial. Six come from `json_object` and six from `whole_file`:

```
codec.json_object.{tiny,small,medium,constrained,nested,tabular}   0.0 -> 0.0   (.lands only)
codec.whole_file.{tiny,small,medium}                               0.0 -> 0.0   (both lenses)
codec.search_replace.tiny.lands / .lands_applies                   0.8 -> 1.0
codec.search_replace.small.lands                                   0.0 -> 0.2
codec.search_replace.small.lands_applies                           0.4 -> 0.2   <-- DOWN
codec.search_replace.medium.lands                                  0.4 -> 0.6
codec.search_replace.medium.lands_applies                          0.4 -> 0.8
```

**`json_object` contributes six cells, not twelve**, because its two lenses
are one measurement, not two — `_lenses_for`'s own docstring
(`assay/diff.py:588-600`): *"validation IS the application there …  the probe
writes both columns from one count and they move together always … One cell,
one Change."* That is why only `.lands` appears for it in the 34. And
`codec.search_replace.small.lands` is **not** a both-zero cell: it is one of
the six movers listed above (0.0 → 0.2). Counted off the two documents:

```
$ python3 -c "…floor/candidate loaded…; both-zero over the 18 codec cells"
codec cells in the 34: 18
0.0 on BOTH sides:     12
not both-zero:          6
```

**One cell moved down** — `search_replace.small.lands_applies`, 0.4 → 0.2 —
and it was still `covered`. That is not the gate failing; it is the gate's
noise discipline working as documented. But the width of that discipline at
`n=5` is the caveat that matters most:

```
n=5 Wilson95 intervals, the rule governing every codec cell
  0/5 = 0.0  ->  [0.000, 0.434]
  1/5 = 0.2  ->  [0.036, 0.624]
  2/5 = 0.4  ->  [0.118, 0.769]
  3/5 = 0.6  ->  [0.231, 0.882]
  4/5 = 0.8  ->  [0.376, 0.964]
  5/5 = 1.0  ->  [0.566, 1.000]
```

`_diff_codec_cell` flags a change only when the two intervals are **disjoint**
(`assay/diff.py:568-585`, the test at `:580`, basis `disjoint-intervals`). At `n=5` the **only**
pair of rates that can ever be disjoint is **0/5 versus 5/5** — even 0/5 vs
4/5 overlaps on `[0.376, 0.434]`. So **the 18 codec cells in this verdict can
report a regression only in the single most extreme case possible**, and the
0.4 → 0.2 move was never going to be flagged.

The two speed cells are governed by a rule that says so on its own page: both
sides recorded `n_decode: 1, n_prefill: 1`, so the basis is
`threshold-20pct-assumed` — a 20% rule of thumb, not a measurement.
`decode_tps` 49.45 → 49.26 (−0.4%) and `prefill_tps` 2518.7 → 2526.1 (+0.3%)
are far inside it. `ceiling.max_verified` is 12288 on both sides,
`failure_mode` `hard_error` on both.

The 34 cells, in full, exactly as the `--json` document lists them:

```
ceiling.max_verified, ceiling.failure_mode,
verdict.agent_speed, verdict.chat_speed, verdict.long_output,
verdict.long_output.provisional, verdict.loop_discipline,
verdict.loop_discipline.provisional, verdict.patch_editing,
verdict.patch_editing.provisional, verdict.structured_extraction,
verdict.structured_extraction.provisional, verdict.tool_calling,
verdict.tool_calling.provisional,
codec.json_object.{tiny,small,medium,constrained,nested,tabular}.lands,
codec.search_replace.{tiny,small,medium}.{lands,lands_applies},
codec.whole_file.{tiny,small,medium}.{lands,lands_applies},
speed.decode_tps, speed.prefill_tps
```

## What this means for the fw1-vs-fw2 pair

**Supported, and no more than this:** *fw1 does not regress against the
capability floor fw2 established, on the 34 cells assay measures quick-mode on
this box under 0.13.0, at the sample sizes those cells were measured with.*

**Not supported, and the distinction is the whole point:**

- **This is not a statement about the flywheel.** fw1 is flywheel turn 1's
  model; fw2 is turn 2's, and turn 2 was accepted on **G4 20/20** and **G5
  10/10 + 10/10** — honest refusal and codec gates, run through bloomery's own
  task harness with tasks enabled. **None of that is in a probe profile.** The
  endpoint says so itself, in the note it returns with every report: *"done_trust/G4/G5
  are unmeasured for this candidate until its first real boot with tasks
  enabled."* Reading `covered` as "fw1 is as good as fw2" would conflate two
  different instruments, and the report's own first sentence forbids it.
- **It is not a swap recommendation.** The properties fw2 was selected for are
  not among the 34 cells.
- **It is a weak `covered`, by construction.** Twelve of eighteen codec
  cells are zero-vs-zero, five of seven capability verdicts are `unusable` on
  both sides, and at `n=5` only a 5/5→0/5 collapse could have produced a
  regression. A floor this low is easy to clear, and clearing it is
  correspondingly little evidence.

**What the run does establish, and it is not nothing:** the swap-candidate
endpoint's happy path works end to end in production — scratch registration,
per-identity admission, a real 111-call probe against real weights on a real
GPU, content-addressed retention, a real `assay cover` invocation, one journal
verdict row, and complete cleanup. Every part of design §4 has now executed
against real assay, and the two fix commits held on first contact.

## Pre-registration, and what the run did to it

Twelve predictions (P1–P12) were written before boot 4; seven more (Q1–Q7)
before run 2b, in an amendment carrying its own later timestamp.

| # | prediction | outcome |
|---|---|---|
| P1 | boot 4's **step** row spawns the diff, `exit_code` non-null | **held** — `within-noise`, exit 0 |
| P2 | cumulative reads `instrument-changed (0.12.0/v10 -> 0.13.0/v10)`, `exit_code: null` | **held**, verbatim |
| P3 | bless → 200; baseline sha becomes current's; `f2a2cabc` leaves the home | **held** |
| P4 | swap's `floor_sha` is the post-bless digest | **held** — `fe05f3b3` on both jobs |
| P5 | ~111 (100–120) `AgentCreated` rows for the scratch identity | **held on 2b** (111); **refuted on 2a** (5) |
| P6 | `AgentRemoved` evictions equal the `AgentCreated` count | **mis-specified** — see below |
| P7 | exactly one `Degraded` provisional row per job | **held** — one per job, across **5** admissions (2a) and **111** (2b); without dedup the boot would carry 116 |
| P8 | exactly one `SwapCandidate` row with both shas and an outcome word | **held on 2b**; **refuted on 2a** (a `Degraded` row instead, by design) |
| P9 | `identity_notes` = exactly one `model.name (informational)` note | **held, word for word as pre-registered** |
| P10 | after `done`, no scratch model and no scratch agents in `/status` | **held on both jobs** |
| P11 | retained content-named profile exists, sha matches the row | **held** |
| P12 | GPU shows a second multi-GB load | **refuted on 2a** (flat at 9,928 MiB); **held on 2b** (14,482 MiB) |
| Q1 | unload → 204, `loaded_weights_bytes: 0` | **held**; GPU 9,928 → 1,766 MiB |
| Q2 | `SchedulerDecision` reads `fits`, not `refuse` | **held** — 111 `fits` |
| Q3 | ~111 `AgentCreated`, matched by `AgentRemoved` | **held** — 111 and 111 |
| Q4 | GPU shows fw1's ~9 GB load | **held** — `ModelLoaded … duration_ms: 22436` |
| Q5 | one `SwapCandidate` row | **held** |
| Q6 | retained transient exists, sha matches | **held** |
| Q7 | step-7 eviction rows, count = surviving scratch agents | **held at zero** — see below |

**The pre-registration's most useful moment was disagreeing with the brief.**
The brief expected "instrument-changed again" at boot 4; the pre-registration
predicted that of the *cumulative* row and **the opposite** of the *step* row,
reasoning from `instrument_precheck`
(`crates/bloomery-core/src/profile.rs:266`) — `InstrumentChanged` iff
`probe_version` *or* `assay_profile_version` disagree, and boot 3's document
was already 0.13.0/v10. The step row came back `within-noise`, exit 0. A
prediction that had simply agreed with the brief would have called that a
surprise.

**The same pre-registration also got a checkable fact wrong**, and the
correction is recorded above rather than quietly dropped: it claimed no boot
in this lineage had ever spawned `assay diff --gate`, when boot 2's journal
shows both comparisons at `exit_code: 0`. The prediction survived; the
supporting sentence did not.

**P6 was mis-specified, and the amendment corrected it before run 2b rather
than after.** It predicted step-7 evictions equal to the `AgentCreated` count.
Run 2a revealed why that is wrong: assay's agents are **ephemeral** — all 227
`AgentRemoved` rows in this boot read `"reason":"ephemeral cleanup"`, created
and removed per call — so by step 7 there is normally **nothing left to
evict**. The correct reading, written down before 2b confirmed it: **zero
eviction rows on a clean run is a pass.** `8c6acd6`'s eviction path guards
against a *third-party* caller minting an agent through the open window, a
hazard that is not on assay's path — and this run therefore **did not exercise
it**. That is a real gap in this acceptance, named rather than papered over:
`8c6acd6` is still pinned only by its unit rows.

**P5, P8 and P12 read as refuted on run 2a and held on run 2b.** Both readings
are reported. Run 2a's refutations are what discovered the residency blocker.

## GPU and shutdown

Sampled every 15 s throughout; the full series is durable at
`~/.local/share/bloomery/drift/boot4-gpu-samples.log` (**144 samples**, 4,393
bytes). Every range below is the min and max of the samples falling inside
that phase's own wall-clock window — recomputed from the log, not from
readings taken during the run:

| moment | window (UTC) | n | GPU used |
|---|---|---|---|
| before boot 4 | — | — | **1714 MiB** |
| boot 4 POST probe (fw2 loaded) | 05:00:55–05:11:21 | 40 | 14,398–**14,481** MiB |
| after the probe, fw2 weights resident | 05:11:22–05:14:33 | 12 | 9,925–9,926 MiB |
| **run 2a, throughout** | 05:14:34–05:19:29 | 20 | 9,925–9,928 MiB (**no second load** — run 1's signature) |
| run 2b, candidate digest phase | 05:22:00–05:26:02 | 16 | 1766–**1838** MiB |
| **run 2b probe (fw1 loaded)** | 05:26:03–05:34:37 | 35 | **1833 → 14,483 MiB**, see below |
| after step 7 unloaded the scratch | 05:34:38–05:37:40 | 11 | 1767 MiB (flat) |
| **after shutdown** | — | — | **1713 MiB** |

**Two corrections against the first draft of this document, both caught by
re-reading the log rather than by memory.** The `14,483` upper bound belongs
to **run 2b**, not to the boot probe — the boot probe's maximum is `14,481`
at `05:01:33Z`; and the digest phase reached `1838` at `05:24:19Z`, not
`1833`.

**The run-2b probe row is a range only in the loosest sense, and the shape
matters.** Of its 35 samples, 32 sit between 14,402 and 14,483 MiB — full
residency. Three do not:

| sample | reading | |
|---|---|---|
| `05:26:05Z` | 1833 MiB | before the load began (`ModelLoaded` reports 22,436 ms) |
| `05:26:20Z` | 9997 MiB | the load in progress |
| **`05:28:50Z`** | **10,933 MiB** | **~2.7 min into the probe, at full residency — a ~3.5 GB dip** |

**The dip is reported because it happened, and it is not explained here.**
14,418 → 10,933 MiB is a drop of **3,485 MiB**, against a per-agent reserve of
4,521 MiB (`kv 4,338,155,520 B + ctx overhead 402,653,184 B`) — so it is
neither a full context teardown nor a weights unload, and no arithmetic in the
artifacts accounts for 3,485.

**It cannot be correlated to any row, and the reason is itself a finding:
neither durable artifact carries a wall clock.** The journal's complete key set
across all 1132 rows contains no time field —

```
$ python3 -c "…union of keys over every row…"
['bound_by','budget_granted','candidate_gguf_sha','candidate_profile_path',
 'candidate_profile_sha','comparison','completion_tokens','current_path',
 'current_sha','decision','detail','duration_ms','event','evicted','exit_code',
 'floor_path','floor_sha','id','model','needed_tokens','outcome','priority',
 'profile_path','prompt','prompt_sha256','prompt_tokens','provenance','reason',
 'reference_path','reference_sha','sha','version','window_tokens']
```

— and `boot4.log` has none either (its only two clock-shaped matches are the
PCI address `0000:01:00.0`). So **no journal row and no log line can be matched
to `05:28:50Z`**, and this document does not guess which of the probe's 111
calls was in flight.

What *can* be said from the artifacts, and no more: the probe's agents are
ephemeral, so the daemon builds and tears down a context per call, and
`boot4.log` shows **216 contexts constructed** across the boot — which is
exactly `227 AgentCreated − 11 Refusal`, the eleven refused agents never
having reached a context. A sampler tick landing inside one of those gaps is
*consistent* with a reading below full residency. **That is a hypothesis the
artifacts do not establish**, the 3,485 MiB does not match a full teardown,
and it is written here as a hypothesis rather than as the explanation.

The bloomery-attributable delta at close is **−1 MiB** against the pre-boot
reading. Shutdown was by verified PID, never by name:

```
$ readlink /proc/1121202/exe
/home/brice/workspace/bloomery/.worktrees/seam/target/debug/bloomery-daemon
$ kill -TERM 1121202
$ pgrep -x bloomery-daemon        # (empty)
$ ss -ltn | grep 8399             # (not listening)
$ nvidia-smi                      # 1713 MiB
```

## What boot 4 left in the standing home

```
/home/brice/.local/share/bloomery/drift/
├── bloomery-drift.toml                    # unchanged by this run
├── boot1.log  boot2.log  boot3.log        # earlier boots
├── boot4.log                              # NEW — 1,105,928 bytes
├── boot4-gpu-samples.log                  # NEW — 4,393 bytes
└── data/
    ├── journal/boot-1787149620.jsonl      # boot 1 (556 rows)
    ├── journal/boot-1787150388.jsonl      # boot 2 (557 rows)
    ├── journal/boot-1787197252.jsonl      # boot 3 (559 rows)
    ├── journal/boot-1787201923.jsonl      # NEW — boot 4 (1132 rows)
    └── profiles/
        ├── qwen3-14b-flywheel2.{baseline,previous,}.json
        └── qwen3-14b-flywheel2!swap-candidate.transient-f8b7c60b.json   # NEW
```

State after shutdown, read off the files:

| document | sha256 (first 8) | probe_version | its probe started |
|---|---|---|---|
| `…baseline.json` | **`fe05f3b3`** | **0.13.0** | `2026-08-20T05:00:51Z` |
| `…previous.json` | `34cc643b` | 0.13.0 | `2026-08-20T03:42:51Z` |
| `…json` (current) | `fe05f3b3` | 0.13.0 | `2026-08-20T05:00:51Z` |
| `…!swap-candidate.transient-f8b7c60b.json` | `f8b7c60b` | 0.13.0 | `2026-08-20T05:26:03Z` |

**Every document in the standing home is now 0.13.0/v10.** `f2a2cabc` and
`179b706e`, the two 0.12.0 documents boot 3 left, are both gone — the first
overwritten by the bless, the second by boot 4's rotation. That was
pre-registered as a consequence, not discovered afterwards.

## What ran, verbatim

```bash
# preflight
PYTHONPATH=/home/brice/workspace/assay/src python3 -c "import assay; print(assay.__version__)"   # 0.13.0
cd /home/brice/workspace/assay && ./.venv/bin/python -m pytest -q                                # 1073 passed
cd /home/brice/workspace/bloomery/.worktrees/seam
cargo test -p bloomery-core -p bloomery-daemon                                                   # 46 suites, 573 passed
cargo build -p bloomery-daemon --features vulkan                                                 # featured binary LAST

# pre-registration, BEFORE any state change
# .superpowers/sdd/2026-08-19-swap-candidate-endpoint/prereg-2.md   mtime 2026-08-19 23:58:28 -0500

# boot 4 of the standing lineage
PYTHONPATH=/home/brice/workspace/assay/src nohup ./target/debug/bloomery-daemon \
  --config /home/brice/.local/share/bloomery/drift/bloomery-drift.toml \
  > /home/brice/.local/share/bloomery/drift/boot4.log 2>&1 &
readlink /proc/1121202/exe                                   # the featured binary
tr '\0' '\n' < /proc/1121202/environ | grep '^PYTHONPATH='   # the pin, read off the live process

# the delegated operator bless
curl -X POST :8399/models/qwen3-14b-flywheel2/bless                     # 200

# the question, run 2a
curl -X POST :8399/models/qwen3-14b-flywheel2/swap-candidate \
  -d '{"gguf_path":"/home/brice/flywheel1/qwen3-14b-flywheel1-Q4_K_M.gguf"}'   # 202 -> infra: HTTP 503

# the named-infra remedy, existing surface only — then run 2b
curl -X POST :8399/models/qwen3-14b-flywheel2/unload                    # 204
curl -X POST :8399/models/qwen3-14b-flywheel2/swap-candidate -d '…'     # 202
curl :8399/models/qwen3-14b-flywheel2/swap-candidate                    # polled to done -> covered

# shutdown by verified PID
readlink /proc/1121202/exe && kill -TERM 1121202
```

Nothing was wrapped in `timeout` (this box's `timeout` segfaults on
multithreaded children), and nothing was killed by bare name. The standing
config was not edited and no code was changed mid-run. **The blessed baseline
*was* moved — deliberately, by delegation, and it is this run's one durable
state change.**

## Open, for the controller

1. **The residency blocker is unfixed.** `POST /models/{name}/unload` before a
   candidate probe is an operator workaround, not a design. Whether the swap
   job should unload the standing model itself — and what that means for a
   daemon still serving agents on it — is a design decision above this task.
2. **`8c6acd6`'s eviction path was not exercised live.** assay's agents are
   ephemeral, so step 7 had nothing to evict. It remains pinned only by unit
   rows.
3. **The verdict is weak by construction**, for the reasons in "What `covered`
   rests on". If a stronger fw1-vs-fw2 answer is wanted, it needs more samples
   per codec cell (`n=5` can only separate 0/5 from 5/5) and the G4/G5 gates,
   which no probe profile contains.
4. **The standing home is now 0.13.0 throughout.** The next boot's cumulative
   comparison will be the first since boot 2 that can spawn a diff against the
   blessed baseline — and the first ever under 0.13.0.
5. **One pre-registered sentence was factually wrong** (see the correction in
   "The journal rows"). The prediction it supported held, but the error is
   recorded rather than edited out.
