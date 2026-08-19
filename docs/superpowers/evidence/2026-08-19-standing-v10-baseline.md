# Drift watch — the first standing baseline (assay 0.12.0, schema v10)

**Date:** 2026-08-19 (boots at 09:24 and 09:37 CDT; ~28 min wall across
two boots including model load).
**Context:** assay merged its v1.9 (scale-free overlap, schema v10) and
v1.10 (semantic-break registry) waves this morning — the live
`PYTHONPATH` pin tracks assay master, so the daemon's instrument moved
from 0.9.0/v8 to 0.12.0/v10 — and bloomery PR #14 landed first, so the
gate reads assay's exit `3` as its own `incomplete` verdict instead of
"undocumented exit" infrastructure. The 2026-08-17 acceptance
(`2026-08-17-drift-watch-live.md`) ran against **scratch** data_dirs, so
no standing baseline existed anywhere; these two boots establish it.
**bloomery:** master at `c2db7eb` (the PR #14 merge). Suite green before
the boots: 45 suites, **532 passed, 0 failed**
(`cargo test -p bloomery-core -p bloomery-daemon`).
**Box/tier:** `enthusiast-16gb`, `emulated = false` — RTX 5080 16 GB,
Vulkan. GPU 1581 MiB used (desktop) before boot 1; **1169 MiB** after
the last shutdown with `pgrep -x bloomery-daemon` empty — the
bloomery-attributable delta at close is zero.
**Model:** `/home/brice/flywheel2/qwen3-14b-flywheel2-Q4_K_M.gguf`
(flywheel2 merge), configured as `qwen3-14b-flywheel2`.

## The standing home

```
/home/brice/.local/share/bloomery/drift/
├── bloomery-drift.toml      # the config, byte-for-byte the acceptance's
│                            # values except data_dir (durable, not a
│                            # worktree's target/)
├── boot1.log  boot2.log     # daemon stdout+stderr per boot
└── data/
    ├── journal/boot-1787149620.jsonl   # boot 1 (556 rows)
    ├── journal/boot-1787150388.jsonl   # boot 2 (557 rows)
    └── profiles/qwen3-14b-flywheel2.{baseline,previous,}.json
```

This directory — not the repo, not a worktree — is where the drift
watch's references now live. The assay pin stays what the acceptance's
§6 established: `PYTHONPATH=/home/brice/workspace/assay/src` **on the
daemon process**, verified this run to import assay **0.12.0** before
boot 1.

## Verdict

Both boots read exactly what the spec pins, no retries.

| boot | data_dir | drift-step | drift-cumulative | diff spawned? |
|---|---|---|---|---|
| 1 | fresh | `unmeasured` (no previous) | `unmeasured` (no baseline) | no (`exit_code: null`) |
| 2 | boot 1's | **`within-noise`** | **`within-noise`** | yes, both (`exit_code: 0`) |

Boot 1 auto-blessed **after** both comparisons answered
(`provenance: "auto-first-profile"`), so its cumulative row honestly
reads `unmeasured` — the same ordering the acceptance pinned. Boot 2
journaled zero `Blessed` rows: the baseline stood.

The blessed baseline: `probe_version 0.12.0`, `assay_profile_version
10`, sha `f2a2cabc360a7423f5f963975103672cda04242c55fc04897ee0e2aa5a5b1b98`.
Boot 2's current profile is a genuinely different document
(sha `179b706e58f97f8d9e728dbefbabaf0aa8b837fb6798aedf67ca325148ee1c2a`)
that both diffs compared and found within noise — a real measurement
pair, not a byte-identical self-compare.

Probe walls, from each profile's own provenance: boot 1
`14:28:59Z → 14:38:45Z` (9m46s), boot 2 `14:41:48Z → 14:51:33Z` (9m45s).
Both probes ran assay's **quick mode** — that is what POST invokes — so
the `parallel` family is unmeasured and `verdict.parallel` reads
`unmeasured` on both sides. Honest, and it means future step/cumulative
diffs exercise assay v1.8's unmeasured-on-both-sides rule (the cell is
not `dropped`, exit 3 does not fire) rather than the parallel ladder.

## Boot 1's drift-relevant rows, verbatim

```json
{"event":"Boot","version":"0.1.0"}
{"event":"Degraded","reason":"provisional admission: qwen3-14b-flywheel2 has no profile yet; POST in progress"}
{"event":"Post","model":"qwen3-14b-flywheel2","outcome":"ok","profile_path":"/home/brice/.local/share/bloomery/drift/data/profiles/qwen3-14b-flywheel2.json"}
{"event":"Drift","model":"qwen3-14b-flywheel2","comparison":"step","outcome":"unmeasured: reference /home/brice/.local/share/bloomery/drift/data/profiles/qwen3-14b-flywheel2.previous.json: no such file","reference_path":"/home/brice/.local/share/bloomery/drift/data/profiles/qwen3-14b-flywheel2.previous.json","current_path":"/home/brice/.local/share/bloomery/drift/data/profiles/qwen3-14b-flywheel2.json","exit_code":null,"reference_sha":null,"current_sha":"f2a2cabc360a7423f5f963975103672cda04242c55fc04897ee0e2aa5a5b1b98"}
{"event":"Drift","model":"qwen3-14b-flywheel2","comparison":"cumulative","outcome":"unmeasured: reference /home/brice/.local/share/bloomery/drift/data/profiles/qwen3-14b-flywheel2.baseline.json: no such file","reference_path":"/home/brice/.local/share/bloomery/drift/data/profiles/qwen3-14b-flywheel2.baseline.json","current_path":"/home/brice/.local/share/bloomery/drift/data/profiles/qwen3-14b-flywheel2.json","exit_code":null,"reference_sha":null,"current_sha":"f2a2cabc360a7423f5f963975103672cda04242c55fc04897ee0e2aa5a5b1b98"}
{"event":"Blessed","model":"qwen3-14b-flywheel2","profile_path":"/home/brice/.local/share/bloomery/drift/data/profiles/qwen3-14b-flywheel2.baseline.json","sha":"f2a2cabc360a7423f5f963975103672cda04242c55fc04897ee0e2aa5a5b1b98","provenance":"auto-first-profile"}
```

## Boot 2's drift-relevant rows, verbatim

```json
{"event":"Boot","version":"0.1.0"}
{"event":"Degraded","reason":"provisional admission: qwen3-14b-flywheel2 has no profile yet; POST in progress"}
{"event":"Post","model":"qwen3-14b-flywheel2","outcome":"ok","profile_path":"/home/brice/.local/share/bloomery/drift/data/profiles/qwen3-14b-flywheel2.json"}
{"event":"Drift","model":"qwen3-14b-flywheel2","comparison":"step","outcome":"within-noise","reference_path":"/home/brice/.local/share/bloomery/drift/data/profiles/qwen3-14b-flywheel2.previous.json","current_path":"/home/brice/.local/share/bloomery/drift/data/profiles/qwen3-14b-flywheel2.json","exit_code":0,"reference_sha":"f2a2cabc360a7423f5f963975103672cda04242c55fc04897ee0e2aa5a5b1b98","current_sha":"179b706e58f97f8d9e728dbefbabaf0aa8b837fb6798aedf67ca325148ee1c2a"}
{"event":"Drift","model":"qwen3-14b-flywheel2","comparison":"cumulative","outcome":"within-noise","reference_path":"/home/brice/.local/share/bloomery/drift/data/profiles/qwen3-14b-flywheel2.baseline.json","current_path":"/home/brice/.local/share/bloomery/drift/data/profiles/qwen3-14b-flywheel2.json","exit_code":0,"reference_sha":"f2a2cabc360a7423f5f963975103672cda04242c55fc04897ee0e2aa5a5b1b98","current_sha":"179b706e58f97f8d9e728dbefbabaf0aa8b837fb6798aedf67ca325148ee1c2a"}
```

Boot 2's step reference sha equals boot 1's document — the rotation law
holding — and both rows carry `exit_code: 0` from a genuinely spawned
`assay diff --gate` under 0.12.0.

## Two build traps, named so they stop costing boots

Both cost a failed boot 1 attempt this run (the daemon refused at
startup with "built without the `llama` feature" — the honest guard
doing its job):

1. **`cargo build --features vulkan` from the workspace root no longer
   forwards the feature to `bloomery-daemon`.** It exits 0 and produces
   a featureless binary. Build the daemon as
   `cargo build -p bloomery-daemon --features vulkan`.
2. **A later `cargo test` silently overwrites the featured binary.**
   `cargo test -p bloomery-daemon` rebuilds `target/debug/bloomery-daemon`
   without features. Order of operations for a live run: test first,
   build the featured binary last, then boot.
