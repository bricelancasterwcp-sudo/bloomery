# G5-on-v4 baselines — REAP-48-ours untrained base (`qwen36-reap48-ours`)

**Date:** 2026-08-22. **Gate:** G5 under `2026-08-21-g5v4-protocol.md`
(including its dated §5 reason-grounding amendment, ruling bF/R1), fixture
set `codec-tasks-v4-mixed` (frozen at `70375e4`; 16 patch + 16 refuse),
**envelope-v4**, greedy, advisory. Both boots also exercise the G4 probe on
`codec-tasks-v1` first (same boot, same daemon) — recorded as corroborating
context under envelope-v4, **not** the headline. These two runs are the
**new `qwen36-reap48` line's own anchors** (`docs/gates.md` dated amendment):
boot 1 is the anchor, boot 2 is corroboration. Journals + tasks JSONL
committed beside this doc.

This document is **not** a comparison to `2026-08-21-g5v4-baselines.md`
(`qwen3-14b-flywheel3` / stock `qwen3:14b`). Different base, different
parameter count, different architecture (hybrid MoE vs dense 14B), different
serving geometry. No cross-base sentence appears anywhere in this file.

---

## 1. Expectations (PRE-REGISTERED — written and committed BEFORE the first boot)

**Written 2026-08-22, before either daemon was started.** Any amendment
after the first boot is a SEPARATE dated file, never an in-place edit of
this section (standing process rule, `docs/gates.md` amendment protocol).
Neither boot is re-run for a nicer verdict: two boots, both counted, and
whatever they say is the record.

### 1.1 Boot 1 is the anchor; boot 2 is corroboration — declared before either runs

**This is the load-bearing rule of this document, stated once and binding
for everything below it.** Boot 1's numbers are the anchor value for this
line's untrained-base record. Boot 2 is run identically — same config
byte-for-byte except `port` and `data_dir` — to corroborate boot 1, not to
choose between two candidate answers. Greedy decoding at temperature-less
sampling says the two boots should be identical. **If they are not
identical, the difference is reported as a finding about this box (thermal
throttling, scheduler nondeterminism in the attention/SSM kernels, a
transient), never as a reason to prefer one boot's numbers over the other's.**
Both are printed, side by side, in §6, and the anchor (boot 1) is the number
carried forward into any later comparison this line makes against its own
future (trained) numbers.

### 1.2 The line's floor, quoted from the spike, superseded on measurement

The REAP-48-ours spike (`2026-08-21-reap48-qwen36-spike.md`) measured this
exact GGUF once, informally, before the geometry fix:

- G4 on `codec-tasks-v1`: **20/20**
- G5-v4 patch: **13/16** (provisional)
- G5-v4 refuse: **9/16**
- `done` rows: **45 on 32 fixtures** (over-eager — some fixtures produced
  more than one `done`-shaped step before terminating, or the anatomy is
  otherwise not 1:1; recorded here as the spike's own summary number)
- grant-violation rows: **5**

**These numbers are expectations, not results, and they are superseded the
moment boot 1's `CodecVerdict`/`CodecVerdictMixed` rows land.** They are
quoted here only so §8 (the scorecard) has something pre-registered to check
itself against. The spike's numbers are **not** repeated anywhere in §4–§7 as
if they were this run's data, and no sentence in this document reads "the
spike had X, this boot has Y" as a delta — the relationship is expectation
→ measurement, not measurement → measurement.

**Named honest possibilities, in advance:**

- The failure shape the spike diagnosed is over-eager patching: a capable,
  undertrained-on-refusal base that reaches files, patches them, and rarely
  declines. If that shape holds at the fixed geometry, refuse lands well
  below the ≥13/16 floor while patch sits at or near it — consistent with
  the spike's 13/16 · 9/16.
- The geometry fix (ride-along 1) changes VRAM accounting and the context
  window, not model weights or the prompt. **No behavioral change is
  expected from the geometry fix alone** — a large swing in either class
  between the spike's informal read and this boot's formal one would itself
  be a finding, not an expected consequence of fixing `kv_per_token`.
- `done` count exceeding fixture count (over-eager termination signature) may
  or may not reproduce at n=1 per class-question; it is reported as measured,
  not assumed to reproduce.

### 1.3 The fixed-geometry consequences to be recorded, not gated

Ride-along 1 (merged at `71415e8`) fixed two accounting defects for this
hybrid GGUF. The following are **serving facts of the line**, recorded in §7
from the daemon's own `/status` and the journal, and are **never** part of
the pass/fail floor:

| quantity | expected value | source |
|---|---|---|
| `kv_per_token` | **20,480** bytes/token | `/status` `.models[0].kv_per_token` |
| `recurrent_state_bytes` | **65,863,680** bytes | `/status` `.models[0].recurrent_state_bytes` |
| `kv_per_token_declared` | **false** (derived, not an operator override) | `/status` `.models[0].kv_per_token_declared` |
| window (`window_tokens`) | **≈ 108,700** tokens, vram-bound, no override | journal `AgentCreated.window_tokens` |
| decode tps | expected **below** the spike's 116.7 tok/s (the spike's 231k-token boot lost ~20% at longer context; this boot's window is shorter, ~108.7k, so the direction is reported, not asserted as a specific number in advance) | assay POST profile, if present in the journal/status, or measured directly by the codec probe's own throughput accounting — location noted in §7 |

Whatever the daemon actually reports for `window_tokens` is recorded
verbatim in §7 even if it differs from ≈108.7k — it is a serving fact, not a
gate, and the spec's own arithmetic (§2) is quoted here as the
pre-registered expectation, not as a value this task is permitted to force.

### 1.4 Reporting discipline pinned in advance (ruling bT10/R1, carried from the turn-4 baselines doc)

The pass floor (**≥13/16 per class**, and **≥16/20** for G4) and the
**two-sided Wilson flag** are reported as **SEPARATE facts**. "Decided"
means the Wilson 95% interval does not straddle 0.80 — on *either* side: an
interval wholly above 0.80 is a decided PASS (at n=16 only 16/16 reaches
it), an interval wholly below 0.80 is a decided FAIL. The flag marks the
record; it never changes the floor decision. The phrase **"decided by
construction" is not used of any score in this document** — it describes
only the reachability property of n=16.

**No cross-envelope comparison and no causal sentence across bases.** Every
number in §4–§7 is a per-(model = `qwen36-reap48-ours` untrained,
envelope-v4) measurement. It is never written as a delta against
`qwen3-14b-flywheel3`, stock `qwen3:14b`, or any other model measured in any
other evidence file, and never against the informal spike figures except as
"expectation vs. measurement" per §1.2.

**Every count, composition, endpoint, grant-violation number and verb
histogram in §4–§7 is pasted from the recompute JSON produced by
`tools/evidence/recompute.py`; every anatomy claim (trajectory shapes per
class, `done` count vs. fixtures, out-of-slice reads, refuse-class
per-family row reads) is emitted by a small script over the committed JSONL
whose output is quoted — never written from memory.**

Nothing is ever re-run for a nicer verdict. If the recompute tool's exit
code is nonzero, or `join.mode != "keyed"`, or `join.keyed_equals_ordinal !=
true`, or `join.violations != []`, or either `journaled_verdict_matches !=
true` — that is recorded verbatim and the task reports
`DONE_WITH_CONCERNS`, with nothing edited to make it pass.

---

## 2. Preflight (all facts below established BEFORE the first boot)

| item | value |
|---|---|
| bloomery tree | `master` @ `71415e8` (branch 1 / ride-along fixes for turn 5 merged: hybrid-aware pager geometry, `TaskStep.args` + `CodecFixture.agent`, `tools/evidence/recompute.py`) |
| Rust test suite | **not run this task** — the featured release binary is already built (`nm -C` confirms `ggml_vulkan` present) and `cargo test` is forbidden in this checkout for the duration of this task (it overwrites the featured binary featureless); the binary's provenance is taken on the standing house rule, not re-verified by a fresh `cargo test --workspace` |
| featured binary | `target/release/bloomery-daemon`, mtime **2026-08-22 14:12:01 -0500**, size 47,152,560 bytes, `nm -C target/release/bloomery-daemon \| grep -c ggml_vulkan` → **1** |
| no-op build confirmation | `cargo build --release -p bloomery-daemon --features vulkan` → `Finished \`release\` profile [optimized] target(s) in 0.05s` (real 0m0.058s) — no recompilation; binary mtime unchanged after (still 14:12:01 -0500), `nm -C` count still 1 |
| assay pin | `PYTHONPATH=/home/brice/workspace/assay/src`, assay **0.13.0** @ `bdb7f92`, working tree clean — the same pin the turn-4 and prior baseline runs used |
| GPU | RTX 5080, 16,303 MiB total, **923 MiB** in use (ptyxis 31 MiB, lact 49 MiB, gnome-text-editor 142 MiB, plus desktop-session overhead not attributed per-process by `nvidia-smi`) → ~14.9 GiB free. **No bloomery daemon in the process list** (`ps -eo pid,comm \| grep -w bloomery-daemon` → no match, exit 1). An **idle `ollama serve`** (PID 3696348, listed by `ps` but **0 MiB** in `nvidia-smi --query-compute-apps`) is present and was **not** killed — it holds no GPU memory, reported per house rule. |
| disk | `/` (holds both the repo and `~/models`): 915G total, 727G used, **143G available**, 84% used |
| daemon digest anchor | `sha256sum ~/models/gguf/qwen36-reap48-ours-Q4_K_M.gguf` → `90e2181e8c3175c7f59f911ee70dfcc58cd068977fc657be3a4101d041f591a5` — **matches** the digest named in the task brief and the flywheel5 spec; boot digest is asserted against this value in §4/§5, BLOCKED if it does not match |
| boot configs | `target/reap48-base-live/boot{1,2}/bloomery.toml`, written 2026-08-22 (not committed — local paths); verbatim in §3 |

Both the featured-build mtime and the `nm -C` count were re-checked
immediately after the no-op confirmation build to establish that the build
step performed no work and left the pre-existing featured binary untouched.

---
