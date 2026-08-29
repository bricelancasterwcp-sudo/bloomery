# `premise-gone-battery-v1` — pre-registration (committed BEFORE any real run)

**Date:** 2026-08-28. **Branch:** `premise-gone-battery` in the main
checkout, base `master` at `e3cad71` (the refalsify default-flip commit —
inert here, both arms set the flag explicitly). **Spec:**
`docs/superpowers/specs/2026-08-28-premise-gone-battery-v1-design.md` —
binding; §5's formulas are cited below, never restated with different
words. **Amendment protocol:** identical to `docs/gates.md`'s house rule
and the battery-v2 prereg's — all values below are frozen; changes
require a recorded protocol amendment executed before re-running, never
tune-and-rerun (§"Amendment rule" below).

Registered under Brice's 2026-08-28 delegation ("do the premise_gone
lane"); the delegation is read as the launch authorization (spec §7 step
5), with the GPU-hygiene check mandatory immediately before each boot and
a held GPU a STOP. Spec-marked [judgment] calls (corpus seed 20260828;
bootstrap seed 20260829; floor 25 = n/2) stand for Brice's
after-the-fact review; none is tunable after this lock.

## 1. Claim discipline (spec §1, quoted verbatim)

> If the gates pass, the licensed sentence is: *"On exact repeats whose
> stored verification already passes at task start — cited bytes unchanged,
> the verification contract moved on (this corpus's moved-on-test
> construction) — refalsify-on takes the premise_gone lane totally: every
> matched retrieval stamps `premise_gone` and stays silent, and no episode
> is contradicted or store-mutated, while refalsify-off injects the moot
> lesson on every matched retrieval (lens: this battery)."* Nothing else.

Named absences (spec §1): the staleness-benefit story (A1 tokens is an
advisory observation, never a claim); the design-§5 passive-poisoning
weight (A2 aftermath is observed feed-forward for that future
registration, never gated, never quoted as capability); any probe-cost
number; the already-fixed-start flavor (spec §0 — unreachable under
exact retrieval); novel tasks/models/shapes/accuracy; every
cross-battery comparison (memory-battery-v1 AND refalsify-battery-v2 —
different corpus, different night; incomparable, not wrong).

## 2. Lens

| pin | value | source |
|---|---|---|
| **daemon commit** (crates/ tip) | `e3cad71` — verified at lock: `git diff e3cad71 HEAD -- crates/` is EMPTY on this branch (all 6 branch commits are tools/+docs). Living-document note: re-verify immediately before each boot; non-empty → STOP and record the successor. | ledger R1; verified this session |
| **binary served** | `/home/brice/workspace/bloomery/target/debug/bloomery-daemon` — the main checkout's featured vulkan debug build, rebuilt this session after `e3cad71` (mtime 2026-08-28 22:00), i.e. a build of exactly the pinned crates tip. `readlink /proc/<pid>/exe` re-confirmed after each boot and before each kill; pid via `ps`, never `$!`. | box rule + battery-v2 prereg §2 precedent |
| **served-model digest** | `7020b925c07c5a3808e1155700ca707598cb4f6d6089bb6daff4147b4d6b00bd` | battery-v1/v2 preregs; asserted per phase by the driver (`--expected-digest`) and FATAL-checked again inside `recompute_pg`'s CLI |
| model identity (API name) | `qwen36-reap48-flywheel5` (boot-config stanza key; exact-key lookup) | `driver.py` `MODEL` |
| envelope | `v4` | boot configs below |
| `window_cap` | `16384` | `driver.py` `WINDOW_CAP` |
| poll cadence / task deadline | `5.0` s / `600.0` s | `driver.py` defaults |
| **arm order** | **M′ then R**, same session | spec §4 |
| **corpus** | `tools/memory_battery/corpus-pg-v1`, seed `20260828`, n = `50`, FROZEN at commit `42ca7f0`; manifest sha256 `642c087332427c69790c6ab113791e8003e25d55c508ea2599ce175cf6c9c21d`; checker `corpus_check_pg` OVERALL: PASS at freeze AND re-run required immediately before the real boots | spec §3; ledger Task 3 |
| **bootstrap seed / B** | `20260829` / `10,000` | `recompute_pg.py` `SEED_PG` / `B_PG` (test-pinned literals) |
| **matched-count floor** | `25` (= n/2, [judgment] flagged) | `recompute_pg.py` `MATCHED_FLOOR_PG` (test-pinned) |
| SE multiplier / infra ceiling | `2` / `0.05` | `recompute_bootstrap.py` constants, imported |
| arm labels | `m_prime` / `r` (real run); dry shakedown used `M_PRIME_DRY` / `R_DRY`; v1's `C`/`M` forbidden unconditionally | `recompute_v2.py` constants, reused |

### Boot configs, VERBATIM (real run; the dry shakedown used byte-identical configs with `data_dir` under `dry/`)

**Arm M′** (`refalsify = false`, port `8497`):

```toml
port = 8497
data_dir = "/home/brice/workspace/bloomery/.superpowers/sdd/2026-08-28-premise-gone-battery/real/runs/arm-m-prime/data"
tasks_enabled = true
ctx_overhead_mib = 512

[memory]
enabled = true
refalsify = false

[models."qwen36-reap48-flywheel5"]
path = "/home/brice/flywheel5/qwen36-reap48-flywheel5-Q4_K_M.gguf"
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

**Arm R** (`refalsify = true`, port `8498`): byte-identical except
`port = 8498`, `refalsify = true`, and `data_dir` ending
`arm-r/data`.

Note the explicit `refalsify = false` in M′ is now an OPT-OUT (the
default flipped at `e3cad71`) — explicitly written in both configs
precisely so the flip cannot silently change an arm's semantics.

## 3. Corpus (bytes; re-asserted at this lock)

The corpus is bytes (§2's freeze row); nothing in
`tools/memory_battery/corpus-pg-v1/` is ever edited in place. The real
run consumes a scratch-copy manifest (`dry_manifest.py --real`) whose
grants point INTO the scratch tree — a live daemon's write grant never
points at the tracked tree (battery-v2's hard-won rule; the p2 trees are
scratch-copied by the same mechanism, Task 5).

## 4. Endpoints, computed exactly as spec §5 states them

By reference (spec §5, binding): gates **PG1** (premise_gone totality,
R — with the diagnosed spellings: `skipped_ungranted` → INVALID,
`premise_held` → ALARM, `inconclusive` → infra-dropped into H3),
**PG2** (store preservation, R — zero `MemoryContradicted` + every
episode `verified`), **PG3** (moot-lesson injection, M′ — ≥ floor, zero
oversize Degraded), the **floor** (both arms ≥ 25 else UNMEASURABLE,
never FAIL), the **stamp audit** (retired spellings nowhere; M′ and
R-p1 all `None`); hygiene **H2** (p1 token equivalence, 2×SE_boot,
gating: violation → run INVALID), **H3** (infra ≤ 5%/arm incl.
inconclusive-dropped tasks), **H4** (advisory rates + cross-arm matched
gap); advisory **A1** (p2 token medians + band — the
staleness-benefit story's territory, no sentence), **A2** (M′
aftermath: §5 poisonings, p2 re-mints, terminal statuses, final store
statuses — §5-registration feed-forward, no sentence), **A3** (wall,
battery-v2's honesty rule verbatim: a p1 control gap of the same order
as p2's means box noise and the honest report says so).

One `recompute_pg` invocation after both arms complete; no number read
before both arms finish; the point estimate decides; no re-run, no
extension, no corpus change after any number is seen; an infrastructure
kill with no numbers read may rerun from zero; hygiene computed before
any gate is read.

## 5. Machinery shas at lock (all committed on the branch)

```
tools/memory_battery/corpus_pg.py          b1e4a92443c95d33a836fb7d24fcda329eceac9860eadd323538b231107ff2f9
tools/memory_battery/corpus_check_pg.py    7b50c18c83a51a57c4f40f23841bcb8c92d1e8befdaffb2c38420bf7a3c59fc7
tools/memory_battery/driver.py             6f153bcb61d97eb038e33c8892061235aece0472a0f7e6019161d3b26c4da6d9
tools/memory_battery/dry_manifest.py       5363c42c2e95960c504361f6b3cc381e9caef40b978f8c44da882ebfe69e373d
tools/memory_battery/recompute_pg.py       47855b463ece285609c571ee34780ab9e0047bfd4aed542606c7eb5aa4cd4bce
tools/memory_battery/recompute_v2.py       fe8c72641b1c88ce1246138c8ecec6bc5b9cc7ee9938dd57a41683394982ad51
tools/memory_battery/recompute_join.py     28e068ba9d583102f8695693f89a58b281eb02056b3bdc673f552bdc1a45a17e
tools/memory_battery/recompute_journal.py  d98025aac3b8396bf441eb7e3ca12fb89332cb670c64d2453ea73135ecb43b36
tools/memory_battery/recompute_bootstrap.py 31ff0b04b73df611980caf472bff1a3440c92aa15e59c0228d0ba9dda1cef888
tools/memory_battery/recompute.py          837a435aac417766cdf386b70f30d928570f11d6cd0bb972b4207a2ba52e96e5
```

All pg additions mutation-tested before this lock (ledger Task log: 8
mutants across generator/checker/driver/dry-manifest/recompute, each
killed; suite 151 OK).

## 6. Operational checklist (real run)

1. `git status --porcelain` clean of tracked-file modifications; re-run
   `corpus_check_pg` over the frozen corpus (must be OVERALL: PASS);
   re-verify `git diff e3cad71 HEAD -- crates/` empty.
2. GPU hygiene: `nvidia-smi` shows desktop-only usage; `ollama ps`
   empty; ports 8497/8498 free; no bloomery-daemon process.
3. `dry_manifest.py --real --corpus-dir tools/memory_battery/corpus-pg-v1
   --out .../real/manifest.json` (scratch-copy, grants into scratch).
4. Boot M′ (config above, detached via setsid nohup with
   `PYTHONPATH=$HOME/workspace/assay/src`); poll `/status` to ready
   (posting False, mutating_verbs true, model listed); confirm digest;
   `readlink /proc/<pid>/exe`.
5. `run_battery.sh <real/runs/arm-m-prime> --manifest <real/manifest.json>
   --base-url http://127.0.0.1:8497 --arm m_prime --expected-digest
   7020b925…` ; `watch_battery.sh` to DONE marker numeric `0`; ledger
   2n+2 = 102 rows; teardown = SIGTERM, wait port-down, verify process
   gone + VRAM released.
6. Same for R on 8498 with `--arm r`.
7. One `recompute_pg` invocation (corpus dir, both data dirs, both
   ledgers, `--expected-digest 7020b925…`; default floor/seed/labels);
   stdout to a file + `echo exit=$?`; commit the JSON verbatim.
8. `git status` re-checked: the frozen corpus byte-identical (the
   tracked-tree rule, verified not assumed).

## 7. DRY-numbers prohibition

The Task-7 shakedown's numbers (3 tasks/arm, labels `M_PRIME_DRY`/
`R_DRY`) are instrument shakedown only — none may be quoted in the
findings; the capture-once rule does not attach to them.

## Amendment rule

All values in this document are frozen at commit time. Any change
requires a recorded protocol amendment (a dated section added here
naming what changed and why) executed BEFORE re-running any affected
step — never tune-and-rerun. The corpus is bytes (§3 above, re-asserted
at this lock); nothing in `tools/memory_battery/corpus-pg-v1/` is ever
edited in place.
