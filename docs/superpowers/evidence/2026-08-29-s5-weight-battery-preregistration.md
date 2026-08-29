# `s5-weight-battery-v1` — pre-registration (committed BEFORE any real run)

**Date:** 2026-08-29 (spec/plan/machinery 2026-08-28 night, same
session). **Branch:** `s5-weight-battery` in the main checkout, base
`master` `efa8e6a` (crates tip `e3cad71`). **Spec:**
`docs/superpowers/specs/2026-08-28-s5-weight-battery-v1-design.md` —
binding, including its dated V1 amendment (Task-4 finding: `Error`
halves drop at the join; mechanism not intent). **Amendment protocol:**
the house rule verbatim — all values below are frozen; changes require a
recorded protocol amendment executed before re-running, never
tune-and-rerun.

Registered under Brice's 2026-08-28 delegation ("do the §5-weight
registration"); read as covering the launch, GPU hygiene mandatory
immediately before boot, a held GPU a STOP. Spec-marked [judgment]
calls (corpus seed 20260830; floor 8 = 16/2; third-value perturbation
constants) stand for after-the-fact review; none is tunable after this
lock.

## 1. Claim discipline (spec §1, quoted verbatim)

> If the validity gates hold, the licensed sentence is: *"On exact
> repeats under refalsify-off, design-§5's measured weight on this corpus
> and model is: it contradicted W_A of matched true-but-moot lessons and
> W_C of matched right lessons (collateral, via model non-verification),
> while on stale lessons it corrected W_B_mint (refresh with a landed
> re-verify) and removed W_B_contra (contradiction) of matched
> retrievals — each with its 95% Wilson interval (lens: this
> battery)."* The three lanes' splits ARE the registered endpoints;
> there is no pass/fail bar on the rates themselves — the weight is the
> number, reported whatever it is.

Named absences (spec §1): any §5 design amendment (Brice's future
ruling — these numbers inform it, they do not make it); refalsify-on
behavior on these lanes; the premise_gone shield (settled, not
re-litigated); probe cost; novel tasks/models/shapes/accuracy; every
cross-battery number comparison (the motivating 47/50 is cited as the
fired question only). The entailment discipline (spec §0) binds the
findings doc: mint-xor-contradict totality for scored injected tasks is
code-entailed and appears only as the V1 validity check, never as a
result.

## 2. Lens

| pin | value | source |
|---|---|---|
| **daemon commit** (crates/ tip) | `e3cad71` — verified at lock: `git diff e3cad71 HEAD -- crates/` EMPTY on this branch (all commits tools/+docs). Re-verify immediately before boot; non-empty → STOP. | this session |
| **binary served** | `/home/brice/workspace/bloomery/target/debug/bloomery-daemon` (featured vulkan build of the `e3cad71` tree, unchanged since the flip arc). `readlink /proc/<pid>/exe` after boot and before kill; pid via `ps`, never `$!`. | box rule |
| **served-model digest** | `7020b925c07c5a3808e1155700ca707598cb4f6d6089bb6daff4147b4d6b00bd` | prior battery preregs; driver-asserted per phase; recompute CLI-FATAL |
| model identity (API name) / envelope / window_cap | `qwen36-reap48-flywheel5` / `v4` / `16384` | boot config below; `driver.py` constants |
| poll cadence / task deadline | `5.0` s / `600.0` s | `driver.py` defaults |
| **arm** | single: `s5_off` (`[memory] enabled = true, refalsify = false` — explicit opt-out of the shipped default), port `8497`, fresh empty store, fresh scratch `data_dir`; dry label `S5_OFF_DRY` | spec §4 |
| **corpus** | `tools/memory_battery/corpus-s5-v1`, seed `20260830`, n = `48` (16/lane), FROZEN at commit `5025410`; manifest sha256 `f5d415ff75c590f4f2c49189d4dd0f7140c66f6c9606114fd0204b3224ff08c4`; `corpus_check_s5` OVERALL: PASS at freeze AND re-run before the real boot | spec §3; ledger Task 3 |
| **intervals** | Wilson score, `WILSON_Z = 1.959963984540054`, NO RNG anywhere in the instrument | `recompute_s5.py` (vector-pinned by 4 independent hand derivations) |
| **per-lane matched floor** | `8` | `recompute_s5.py` `FLOOR_S5` (test-pinned) |
| infra ceiling | `0.05` | `recompute_bootstrap.py`, imported |

### Boot config, VERBATIM

```toml
port = 8497
data_dir = "/home/brice/workspace/bloomery/.superpowers/sdd/2026-08-28-s5-weight-battery/real/runs/arm/data"
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

## 3. Corpus (bytes; re-asserted at this lock)

The corpus is bytes (§2's freeze row); nothing in
`tools/memory_battery/corpus-s5-v1/` is ever edited in place. The real
run consumes a scratch-copy manifest (`dry_manifest.py --real`) whose
grants point INTO the scratch tree; the `witness/` directories are not
copied and never reach any run tree.

## 4. Endpoints, computed exactly as spec §5 states them

By reference (spec §5 incl. its dated V1 amendment, binding): validity
gates **V1** (§5 conformance, live — both-events / unexplained-neither /
unscored-in-matched → INVALID; the `Degraded`-explained class counted,
expected 0), **V2** (stamp audit: refalsify all `None`, retired
spellings nowhere, zero p1 injections, zero oversize `Degraded`),
**V3** (per-lane matched floor 8 → that lane UNMEASURABLE), **H3**
(infra ≤ 5% over 96 halves, counted once from `dropped`),
completeness (96/96) + identity CLI-FATAL. Registered endpoints: the
per-lane splits (contradicted / minted / neither counts, rates over the
matched denominator, Wilson 95% intervals). Advisory (no sentence):
per-lane p2 token/wall medians, terminal statuses, patch-attempt
counts, p1 mint rate, final store statuses.

One `recompute_s5` invocation after the arm completes; no number read
before it finishes; the point estimate stands; no re-run, no extension,
no corpus change after any number is seen; an infrastructure kill with
no numbers read may rerun from zero.

## 5. Machinery shas at lock (all committed on the branch)

```
tools/memory_battery/corpus_s5.py          f5b1d7a373228a52387c11a80d64330850c6f0768b1211836190dd3bbafc360f
tools/memory_battery/corpus_check_s5.py    e9de026dda219c0ebffb276554b409123e27821f7cb9ea90a20c0d01ae3b9b10
tools/memory_battery/recompute_s5.py       7091d2cce50de0a5f0c94c7bc19f5d51b6b3d90ad6a3139dfe940ec8bd75194d
tools/memory_battery/corpus_pg.py          b1e4a92443c95d33a836fb7d24fcda329eceac9860eadd323538b231107ff2f9  (reused, unchanged from the pg lock)
tools/memory_battery/corpus_check_pg.py    7b50c18c83a51a57c4f40f23841bcb8c92d1e8befdaffb2c38420bf7a3c59fc7  (reused, unchanged)
tools/memory_battery/recompute_pg.py       47855b463ece285609c571ee34780ab9e0047bfd4aed542606c7eb5aa4cd4bce  (reused: _final_episode_statuses)
tools/memory_battery/driver.py             6f153bcb61d97eb038e33c8892061235aece0472a0f7e6019161d3b26c4da6d9  (UNTOUCHED — byte-identical to the pg lock's pin)
tools/memory_battery/dry_manifest.py       5363c42c2e95960c504361f6b3cc381e9caef40b978f8c44da882ebfe69e373d  (UNTOUCHED — byte-identical to the pg lock's pin)
tools/memory_battery/recompute_v2.py       fe8c72641b1c88ce1246138c8ecec6bc5b9cc7ee9938dd57a41683394982ad51  (reused, unchanged)
tools/memory_battery/recompute_join.py     28e068ba9d583102f8695693f89a58b281eb02056b3bdc673f552bdc1a45a17e  (reused, unchanged)
tools/memory_battery/recompute_journal.py  d98025aac3b8396bf441eb7e3ca12fb89332cb670c64d2453ea73135ecb43b36  (reused, unchanged)
tools/memory_battery/recompute_bootstrap.py 31ff0b04b73df611980caf472bff1a3440c92aa15e59c0228d0ba9dda1cef888  (reused, unchanged)
tools/memory_battery/recompute.py          837a435aac417766cdf386b70f30d928570f11d6cd0bb972b4207a2ba52e96e5  (reused, unchanged)
```

All s5 additions mutation-tested before this lock (ledger: 7 mutants
across generator/checker/recompute, each killed; battery suite 176 OK).

## 6. Operational checklist (real run)

1. `git status --porcelain` clean of tracked modifications; re-run
   `corpus_check_s5` (OVERALL: PASS); re-verify `git diff e3cad71 HEAD
   -- crates/` empty.
2. GPU hygiene: desktop-only VRAM; `ollama ps` empty; port 8497 free;
   no bloomery-daemon process.
3. `dry_manifest.py --real --corpus-dir tools/memory_battery/corpus-s5-v1
   --out .../real/manifest.json`.
4. Boot (config above, `PYTHONPATH=$HOME/workspace/assay/src`, setsid
   nohup); poll `/status` ready; digest; `readlink /proc/<pid>/exe`.
5. `run_battery.sh <real/runs/arm> --manifest <real/manifest.json>
   --base-url http://127.0.0.1:8497 --arm s5_off --expected-digest
   7020b925…`; watcher to DONE numeric 0; ledger 2n+2 = 98 rows;
   teardown = SIGTERM, port-down, VRAM released.
6. One `recompute_s5` invocation (`--corpus-dir --arm-dir --ledger
   --expected-digest`; default floor/label); stdout to a file +
   `echo exit=$?`; commit the JSON verbatim.
7. `git status` re-checked: frozen corpus byte-identical.

## 7. DRY-numbers prohibition

The dry shakedown (3 tasks — lanes moot×2 + stale×1 as the first three
manifest entries fall; label `S5_OFF_DRY`) is instrument shakedown only;
none of its numbers may be quoted in the findings.

## Amendment rule

All values in this document are frozen at commit time. Any change
requires a recorded protocol amendment (a dated section added here
naming what changed and why) executed BEFORE re-running any affected
step — never tune-and-rerun. The corpus is bytes (§3 above, re-asserted
at this lock); nothing in `tools/memory_battery/corpus-s5-v1/` is ever
edited in place.
