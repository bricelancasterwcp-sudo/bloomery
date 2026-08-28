# `refalsify-battery-v2` — pre-registration (committed BEFORE any GPU run)

**Date:** 2026-08-28. **Branch:** `refalsify-battery-v2`, worktree
`/home/brice/workspace/bloomery-batv2`, base `master` at `898867a` (per
`.superpowers/sdd/2026-08-28-refalsify-battery-v2/progress.md`'s own
pre-flight note). **Spec:**
`docs/superpowers/specs/2026-08-28-refalsify-battery-v2-design.md` —
binding; §4's formulas are cited below, never restated with different
words (v1's own rule, carried forward). **Amendment protocol:** identical
to `docs/gates.md`'s house rule and spec §6's own non-silent amendment
rule — all values below are frozen; changes require a recorded protocol
amendment executed before re-running, never tune-and-rerun (§"Amendment
rule" below, copied verbatim from v1's prereg).

This document locks Task 3 of
`docs/superpowers/plans/2026-08-28-refalsify-battery-v2.md`, plus one
controller-ruled preliminary code step folded into this task (§5's
`dry_manifest.py --real` extension, committed separately at `8548468`,
immediately before this document — see §6 for why the real run needs it
and §7 for its pinned invocation).

## 1. Claim discipline (spec §1, quoted verbatim)

> If the gates pass, the licensed sentence is: *"With refalsify on, the
> memory organ's repeat-exposure benefit is preserved — injection and token
> cost equivalent to refalsify-off within the pre-registered bands — at a
> measured probe cost of X ms wall per probed retrieval (lens: this
> battery)."* Nothing else. In particular this battery licenses NO sentence
> about: the `premise_gone` lane (no corpus task starts goal-satisfied),
> the staleness-benefit story (no staleness treatment exists here), or the
> design-§5 passive-poisoning weight (the corpus's happy path re-verifies,
> so §5 does not fire on it). Those are **named absences** — each needs its
> own corpus treatment and its own registration. Battery-v1's claim
> (memory-on beats memory-off on repeats) is settled evidence and is not
> re-litigated; no number from this battery may be compared against v1's
> run (different night, materially different daemon — window ladder, R9,
> refalsify itself all landed since; incomparable, not wrong).

Out-of-scope items (spec §7 — any corpus treatment beyond the frozen
happy-path corpus; any change to refalsify v2 semantics, the organ, or
the window law; re-litigating battery-v1's memory-on claim or any
cross-battery number comparison; default-flipping `[memory] refalsify` —
findings inform that ruling, they do not make it) are named so their
absence from this document is a decision, not an oversight.

## 2. Lens (spec §2/§3 pins)

| pin | value | source |
|---|---|---|
| **daemon commit** (crates/ tip serving this run) | `21a477c` (`21a477ce5c46e404f538f28a6fc2cd66b7a96a5f`) | design spec §2/§5; verified at lock: `git diff 21a477c HEAD -- crates/` and `git diff 21a477c master -- crates/` are both **empty** on this branch and on `master` (currently at `898867a`, two commits ahead of `21a477c` — both docs-only: `644482f`, `898867a` — zero `crates/` bytes changed). `21a477c` is therefore still the true crates/ tip at lock time, not a stale pin. **Living-document note for Task 4:** re-run `git diff 21a477c HEAD -- crates/` immediately before boot; if it is no longer empty, this pin is out of date and Task 4 must STOP and record the successor commit rather than boot on drifted `crates/` bytes (spec §2's own "or a successor recorded at lock" escape hatch). |
| **binary served** | `/home/brice/workspace/bloomery/target/debug/bloomery-daemon` — the **main checkout**'s featured debug build (`--features vulkan,llama`), built 2026-08-28 09:16:37 (`ls -la` mtime), i.e. after `21a477c`'s commit timestamp (09:02:08) and before any later commit — confirmed a build of exactly that tree. Per the controller ruling recorded in progress.md: "Task 2 boots the MAIN checkout's featured vulkan daemon binary — branch touches only `tools/` ... so the binary is code-identical and no cargo build is needed anywhere." Task 4's operational checklist (§7 below) requires `readlink /proc/<pid>/exe` immediately after boot and immediately before `kill`, both times re-confirming this exact path — per v1 prereg §7 / memory-organ-acceptance precedent, `$!` is never trusted as the real pid. |
| **boot-to-ready budget** (scheduling only, not evidence) | ~4–4.5 min per boot on this debug binary (GGUF digest hash + model load ~3–4 min before `/status` answers at all, then ~60–75s boot-POST, then ~15–20s G4 codec probe), **×2 boots** for the real run (M′, then R) | task-2 shakedown, `EVIDENCE-NOTES-DRY.md` §"Boot procedure quirks" — DRY numbers, scheduling ballpark only, never a cost claim. A release build would be faster; this pin assumes the debug binary above unless Task 4 records a different one. |
| **served-model digest** | `7020b925c07c5a3808e1155700ca707598cb4f6d6089bb6daff4147b4d6b00bd` | pinned by `memory-battery-v1`'s prereg §2 and the memory-organ slice-1 acceptance; **re-verified live twice** in the task-2 shakedown (`/status.models[0].digest` on both M′ and R boots, plus independently inside `recompute_v2`'s own `lens.identity` output — `agree: true, matches_expected: true` both arms, both phases). This is `--expected-digest`, checked against `/status`'s served identity, never against the API model name. |
| **model identity (API name)** | `qwen36-reap48-flywheel5` — the boot config's model-table stanza key (`[models."qwen36-reap48-flywheel5"]`), exact-key lookup, no alias/fallback (v1 prereg §2's own derivation, unchanged mechanism) | boot configs below |
| envelope | `v4` | boot config `[models.*].envelope` |
| `window_cap` | `16384` (every battery agent) | `driver.py` `WINDOW_CAP` constant (unchanged since v1; `driver.py`'s sha in §5 below is byte-identical to v1's frozen sha) |
| poll cadence | `5.0` s | `driver.py` `DEFAULT_POLL_INTERVAL_S` |
| per-task poll deadline | `600.0` s | `driver.py` `DEFAULT_TASK_DEADLINE_S` |
| **arm order** | **M′ then R** (fixed, same night — spec §3 status line) | spec §3 |
| corpus seed | `20260826` (v1's, carried forward unchanged — v2 registers no new corpus draw) | manifest `corpus_seed`, re-verified §3 below |
| **bootstrap seed** | `20260828` — deliberately different from v1's `20260826` | `recompute_v2.py` `SEED_V2 = 20260828` (module-level constant, own line, per the module's own mutation-check note #5: "seed drifts — any literal") |
| **bootstrap B** | `10,000` | `recompute_v2.py` `B_V2 = 10_000` |
| G1/H2 SE multiplier | `2` (`2 × SE_boot`) | `recompute_bootstrap.py` `HYGIENE_SE_MULTIPLIER = 2`, imported and reused by `recompute_v2.py` (both G1's band and H2's bar, lines 245/423) |
| H3 infra-rate ceiling | `0.05` (5%) | `recompute_bootstrap.py` `INFRA_RATE_CEILING = 0.05`, imported and reused by `recompute_v2.py` |
| arm labels | `m_prime` (M′), `r` (R) — v1's `C`/`M` unconditionally **forbidden** in v2 ledgers regardless of any `--expected-arm-labels` override | `recompute_v2.py` `ARM_LABEL_M_PRIME = "m_prime"`, `ARM_LABEL_R = "r"`, `FORBIDDEN_ARM_LABELS = frozenset({"C", "M"})` |

### Boot configs, VERBATIM (task-2 shakedown's pinned configs, `data_dir` only changed for the real run's evidence layout)

Both configs are byte-identical to the task-2 dry shakedown's own pinned
configs (`EVIDENCE-NOTES-DRY.md`, itself adapted from
`docs/superpowers/evidence/2026-08-26-memory-battery-preregistration.md`
§2's Arm M config) with exactly **one** field changed per arm from the
shakedown's version — `data_dir`, moved from the shakedown's `dry/`
subtree to a fresh `real/` subtree so the real run's store starts EMPTY
in its own scratch directory and never collides with the (already
torn-down) shakedown's data. `port`, `[memory]`, the model stanza,
`tier`, and `assay` are unchanged from the shakedown's pins.

**Arm M′** (`[memory] enabled = true, refalsify = false`, port `8497`):

```toml
port = 8497
data_dir = "/home/brice/workspace/bloomery/.superpowers/sdd/2026-08-28-refalsify-battery-v2/real/runs/arm-m-prime/data"
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

**Arm R** (`[memory] enabled = true, refalsify = true`, port `8498`):

```toml
port = 8498
data_dir = "/home/brice/workspace/bloomery/.superpowers/sdd/2026-08-28-refalsify-battery-v2/real/runs/arm-r/data"
tasks_enabled = true
ctx_overhead_mib = 512

[memory]
enabled = true
refalsify = true

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

Neither `real/runs/arm-m-prime/data` nor `real/runs/arm-r/data` exists
yet — both are created fresh at Task 4 boot time, per arm, matching
spec §3's "Store starts EMPTY in a fresh scratch `data_dir`" for both
arms (v2's design carries no memory-off control arm — both M′ and R are
treatment arms with their own empty store).

## 3. Corpus — sha re-assertion (spec §2: "same manifest sha, re-asserted at lock")

The frozen corpus is `memory-battery-v1`'s own `corpus-v1/` tree
(seed `20260826`, n=50, 8 run-verified python families — identical
composition to `memory-battery-v1`'s prereg §1/§3, reproduced here from
the live manifest: `py_inverted_boolean_run_verified` ×7,
`py_off_by_one_index_run_verified` ×7,
`py_off_by_one_range_bound_run_verified` ×6,
`py_wrong_comparison_operator_run_verified` ×6,
`py_wrong_constant_multiplier_run_verified` ×6,
`py_wrong_dict_key_run_verified` ×6, `py_wrong_fstring_field_run_verified`
×6, `py_wrong_variable_reference_run_verified` ×6). v2 registers **no new
corpus draw** — the design's own claim discipline (§1 above) licenses no
sentence about any corpus treatment beyond this frozen happy-path
instrument. Four independent checks, all executed at lock, all agreeing:

1. **`git hash-object` on the working-tree file:**
   `git hash-object tools/memory_battery/corpus-v1/manifest.json` →
   `404ae10e52cc4c1d51deeb8e6c663df067538448`.
2. **Git blob identity across the freeze commit and now:**
   `git rev-parse a514bf5:tools/memory_battery/corpus-v1/manifest.json`
   (the original `memory-battery-v1` freeze commit, "docs:
   memory-battery-v1 corpus freeze and preregistration") →
   `404ae10e52cc4c1d51deeb8e6c663df067538448` — **identical** to check 1.
   `git diff a514bf5 HEAD --stat -- tools/memory_battery/corpus-v1/`
   (the whole tree, not just the manifest) → **empty**, zero files
   changed across the entire corpus since the freeze commit.
3. **v1's own `freeze_sha256` method reproduced** (v1 prereg §3.2's exact
   procedure: sha256 over `manifest.json`'s bytes, then every file under
   `tasks/<name>/workspace/*` in sorted relative-path order, each path
   and its bytes NUL-separated):
   ```python
   import hashlib
   from pathlib import Path

   def freeze_sha256(corpus_dir: Path) -> str:
       hasher = hashlib.sha256()
       hasher.update((corpus_dir / "manifest.json").read_bytes())
       for path in sorted(corpus_dir.glob("tasks/*/workspace/*")):
           rel = path.relative_to(corpus_dir).as_posix()
           hasher.update(rel.encode("utf-8") + b"\0")
           hasher.update(path.read_bytes() + b"\0")
       return hasher.hexdigest()

   print(freeze_sha256(Path("tools/memory_battery/corpus-v1")))
   ```
   **Computed:** `d9df82e2f7ae95130fc8fa765b5b1faff7b15e93832f8adfd1980b07d797c9d5`.
   **v1 prereg §3.2's pinned value:** `d9df82e2f7ae95130fc8fa765b5b1faff7b15e93832f8adfd1980b07d797c9d5`.
   **Match: exact.**
4. **`recompute`'s own `lens.corpus_sha` reproduced** (v1 prereg §3.2's
   cross-check: `recompute.py`'s `_corpus_sha`, derived purely from the
   frozen manifest's per-task `workspace_sha256` values, no live
   filesystem re-read — imported unchanged by `recompute_v2.py`, line 81:
   `from tools.memory_battery.recompute import _corpus_sha`):
   ```python
   import json
   from tools.memory_battery.recompute import _corpus_sha
   manifest = json.loads(open("tools/memory_battery/corpus-v1/manifest.json").read())
   print(_corpus_sha(manifest))
   ```
   **Computed:** `778b1491aac67f9235ff2ae6ce74c0c767465fb30b2ab5053e17ce99ccc9a5ff`.
   **v1 prereg §3.2's pinned cross-check value:**
   `778b1491aac67f9235ff2ae6ce74c0c767465fb30b2ab5053e17ce99ccc9a5ff`.
   **Match: exact.**

**Four checks, four exact matches — the corpus is byte-identical to
`memory-battery-v1`'s frozen instrument, unchanged by this branch's work.**
The manifest's own `"instrument"` field still reads `"memory-battery-v1"`
(`corpus.py`'s `INSTRUMENT` constant at generation time) — expected, not a
defect: the bytes are frozen and carried forward, not regenerated under a
v2 name, so the self-identification field is a correctly-preserved v1
artifact.

## 4. Protocol, endpoints, bars — by reference (spec §4)

Every formula (`cost(task)`, ITT/none-vs-zero, G1's equivalence band, G2's
exact-equality bar, the stamp audit's spelling sets, A1's advisory
derivation, H2–H4, the kill criteria) is spec §4's own text, cited here,
never restated with different words. Every locked number, restated for
this document's own completeness (all sourced and cross-checked against
the live `recompute_v2.py` module in §2's table above):

- **Gate G1 (token preservation, equivalence):**
  `|median_R,p2 − median_M′,p2| ≤ 2 × SE_boot(median_R,p2 − median_M′,p2)`
  — seeded bootstrap `B = 10,000`, seed **`20260828`**, resampling unit =
  tasks, each arm's phase-2 tasks resampled independently, medians over
  non-`dropped` tasks.
- **Gate G2 (injection preservation, exact):**
  `injected_R,p2 = injected_M′,p2`, counted from `MemoryStamp`
  `mode:"injected"` rows over non-`dropped` tasks. Deficit fails; excess
  is an instrument alarm, not a pass.
- **Stamp audit (gating, instrument honesty):** every R-p2 non-`dropped`
  `mode:"injected"` stamp must carry `refalsify:"premise_held"`; the
  spellings `passed`/`failed` must appear **nowhere** in either arm
  (`recompute_v2.py` `FORBIDDEN_REFALSIFY_SPELLINGS = frozenset({"passed",
  "failed"})`); `premise_gone` expected count `0`; `inconclusive` and
  `skipped_ungranted` expected `0`, tolerated within H3's budget, counted
  and named individually.
- **A1 (advisory, never gates):** `median wall_R,p2 − median wall_M′,p2`,
  reported beside the per-probed-retrieval derivation and beside the
  no-probe control `median wall_R,p1 − median wall_M′,p1`.
- **H2 (first-exposure equivalence, gating):** `|median_M′,p1 −
  median_R,p1|` within `2 × SE_boot` (tokens) — a gap here means
  instrument error → run **INVALID**, since no probe can fire in either
  arm's phase 1 (both stores empty).
- **H3 (infra rate, gating):** `≤ 5%` per arm (task-level `Error`,
  daemon faults, driver protocol breaks) → above 5% is an
  **infrastructure kill**.
- **H4 (advisory):** mint rate in each arm's p1; retrieval rate in each
  arm's p2.
- **Kill criteria (carried verbatim from v1):** the point estimate
  decides; no re-run, no extension, no corpus change after any number is
  seen; an infrastructure kill with no numbers read may rerun from zero;
  floor-saturation → verdict **UNMEASURABLE**, never PASS.
- **Hygiene evaluation order** (RNG order, per `recompute_v2.py`'s own
  module docstring): one seeded `random.Random(seed)` instance created
  fresh inside `recompute_v2()`; **H2 first, G1 second** — "Hygiene ...
  computed before any gate is read" (spec §4). A1 touches no RNG (pure
  arithmetic, no SE/band).
- **v1's H1 has no analogue here** — both arms carry the
  treatment-relevant store; cross-phase within-arm deltas are the
  organ's intended effect, not a contamination check (spec §4, final
  paragraph).

## 5. Machinery — file shas at lock (`git hash-object`, `tools/memory_battery/`)

```
$ for f in <listed below>; do echo "$(git hash-object "$f")  $f"; done
```

| file | `git hash-object` sha | note |
|---|---|---|
| `recompute_v2.py` | `1595c1fb11e519d4219ee189e4cbf1d70098df3b` | v2-native; committed `6adab84`, fix round `f142ad9` (CLI enforcement + completeness/identity FATAL) |
| `dry_manifest.py` | `b675d00030a980b4ad9554f64d29544a11a3e62a` | **post-`--real`** — this lock's own preliminary commit `8548468`; superseded the shakedown-only version committed at `88d9985` |
| `driver.py` | `6e9ab8233e7f058a0f83a068f4fd3e2172ff96bf` | **unchanged since `memory-battery-v1`** — byte-identical to v1 prereg §5's own pinned sha for this file; zero edits on this branch |
| `run_battery.sh` | `57a14f3d88db88390a66ac9278340f13071d8c0a` | unchanged on this branch (design spec §5: "Driver/scripts: unchanged unless the per-arm daemon config needs a launch-side seam" — it did not; the daemon config seam is `bloomery.toml`'s `[memory].refalsify`, entirely config-side) |
| `watch_battery.sh` | `9f3743fbb8cc10ec2b7208330eabbb43e4af9503` | unchanged on this branch |
| `corpus-v1/manifest.json` | `404ae10e52cc4c1d51deeb8e6c663df067538448` | §3 above — 4/4 checks, exact match to the `memory-battery-v1` freeze |
| `recompute.py` | `d5e7d80f09cd0548cbe7b88102a88ed48cab9893` | **unchanged since `memory-battery-v1`** — byte-identical to v1's own pinned sha; `recompute_v2.py` imports `_corpus_sha` from it directly, no fork |
| `tests/test_recompute_v2.py` | `088f0a3a37b1dd6939e139aeb8b502ccb8b5fe58` | recompute_v2's primary mutation-tested suite (task-1 brief; spec §5: "each new computation mutation-tested before the corpus is touched") |
| `tests/test_recompute_v2_cli.py` | `aae3f4bc582e64b9312026e5a7256da6f8fbcb4d` | split from the file above to stay under the house 800-line ceiling; covers CLI-layer FATAL enforcement (digest mismatch, incomplete arm, arm-label defaults) |
| `tests/test_dry_manifest.py` | `5d1a4e738a507bbba6d118c908f7b5fe08f203e3` | new this session — the `--real` mode's own test file (§6 below); 11 tests, 5 targeted mutations individually caught at implementation time |

All ten shas were read from a **clean working tree**
(`git status --short` empty at lock, verified immediately before this
table was compiled) — nothing quoted here is a pre-commit or uncommitted
value. **118/118 package tests green**
(`PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s
tools/memory_battery/tests -t .`, run after the preliminary's commit,
exit `0`).

## 6. The preliminary: why `dry_manifest.py` needed a `--real` mode

The task-2 shakedown (`EVIDENCE-NOTES-DRY.md`, "Corpus grant-path gotcha"
and "Corpus-mutation incident" sections) found, live, that the frozen
`corpus-v1/manifest.json`'s `grant.read_roots`/`write_roots` still bake
in absolute paths under `memory-battery-v1`'s own deleted worktree
(`/home/brice/workspace/bloomery/.worktrees/memory-battery/...`) —
driving those grants verbatim from this worktree fails before the first
HTTP request. The first fix attempt re-derived grant paths straight at
`corpus_dir`'s own tracked `tasks/<name>/workspace` directories; a live
daemon task's `run`/patch actions then wrote into that tracked path,
patching 3 committed corpus files in place before being caught by `git
status` and restored via `git checkout --`. The corrected tool instead
copies each task's `workspace/` + `pristine/` pair into a scratch tree
and points grants at the scratch copy — proven, at the time, only for a
3-task dry subset.

progress.md's own ruling (2026-08-28, Task-2 completion note): "the real
50-task run needs the same scratch-copy manifest step ... Task 3 first
extends `dry_manifest.py` with a tested `--real` mode (n=50, no dry
stamp), then the prereg pins that tool sha + the exact real-run
invocation." That extension is this lock's preliminary, committed at
`8548468` immediately before this document:

- `generate_run_manifest(..., real=True)` drives **all** tasks in the
  frozen manifest's order (verified live against the actual `corpus-v1/`
  tree at prereg time, not just the test suite's small fixture: `n=50`,
  50 scratch task directories created, `"dry"` key absent,
  `"scratch_copy": true` present, every grant path rewritten onto the
  scratch tree — and `git status --short --
  tools/memory_battery/corpus-v1/` empty both immediately before and
  immediately after, plus a `git hash-object` spot-check on one tracked
  workspace file matching before and after).
- `--count` combined with `--real` is rejected at both the CLI
  (`argparse` usage error, exit 2) and the library level (`ValueError`)
  — real mode's task count is never a caller's choice.
- Both modes now stamp `"scratch_copy": true` unconditionally; `"dry":
  true` is stamped only in dry mode and is absent (not `false`) in real
  mode, so neither a real nor a dry output can ever be mistaken for the
  frozen manifest itself, and a real output is never mistaken for a dry
  one.
- `tests/test_dry_manifest.py` (new, 11 tests): dry-mode default
  behavior pinned; real-mode full count + missing `dry` key + scratch
  grants pinned; the `--count`+`--real` rejection pinned at both layers;
  the frozen tree's byte-identity proven before/after under both modes
  (sha-fingerprint of every file under a small real fixture corpus,
  built via `corpus.generate_corpus` — the same convention
  `test_corpus_check.py` already uses, never touching the tracked
  `corpus-v1/` tree during the test run itself) plus a repeated-call
  case and a mutate-the-scratch-copy-afterward case (proving a genuine
  copy, not the failure mode the shakedown actually hit). Five targeted
  mutations (omit the dry stamp's negation, make `scratch_copy`
  conditional, remove the count/real guard, keep real mode subset-limited,
  point grants at the source instead of the scratch copy) were each
  individually applied and confirmed to fail the suite before being
  reverted.

## 7. Real-run invocation — pinned exactly

Driven from this worktree's root (`/home/brice/workspace/bloomery-batv2`
— `driver.py` and `recompute_v2.py` both need `tools` importable, exactly
like the package's own test command). Evidence dir:
`/home/brice/workspace/bloomery/.superpowers/sdd/2026-08-28-refalsify-battery-v2/real/`
(sibling of the shakedown's already-torn-down `dry/`).

**Step 1 — scratch manifest (Task 4, before either boot):**
```
PYTHONDONTWRITEBYTECODE=1 python3 -m tools.memory_battery.dry_manifest \
  --corpus-dir tools/memory_battery/corpus-v1 \
  --out /home/brice/workspace/bloomery/.superpowers/sdd/2026-08-28-refalsify-battery-v2/real/scratch-corpus/manifest.json \
  --real
```
Expected output: `dry_manifest: wrote 50 task(s) (real mode) to ...`.
The driver consumes **this scratch manifest**, never the tracked
`corpus-v1/manifest.json` directly (§6 above — the grant-path defect is
unconditional, not shakedown-specific).

**Step 2 — Arm M′ (boot per §2's config, then):**
```
tools/memory_battery/run_battery.sh \
  /home/brice/workspace/bloomery/.superpowers/sdd/2026-08-28-refalsify-battery-v2/real/runs/arm-m-prime/out \
  --manifest /home/brice/workspace/bloomery/.superpowers/sdd/2026-08-28-refalsify-battery-v2/real/scratch-corpus/manifest.json \
  --base-url http://127.0.0.1:8497 \
  --arm m_prime \
  --expected-digest 7020b925c07c5a3808e1155700ca707598cb4f6d6089bb6daff4147b4d6b00bd \
  --ledger /home/brice/workspace/bloomery/.superpowers/sdd/2026-08-28-refalsify-battery-v2/real/runs/arm-m-prime/ledger.jsonl
```
Watched with `tools/memory_battery/watch_battery.sh
.../real/runs/arm-m-prime/out`. `--arm m_prime` **exactly** — any other
label makes `recompute_v2`'s arm-label check reject the ledger (`ARM_LABEL_M_PRIME
= "m_prime"`, checked unconditionally against `FORBIDDEN_ARM_LABELS =
{"C", "M"}` regardless of what `--expected-arm-labels` a later recompute
invocation supplies).

**Step 3 — Arm R (after M′'s full teardown, same scratch manifest):**
```
tools/memory_battery/run_battery.sh \
  /home/brice/workspace/bloomery/.superpowers/sdd/2026-08-28-refalsify-battery-v2/real/runs/arm-r/out \
  --manifest /home/brice/workspace/bloomery/.superpowers/sdd/2026-08-28-refalsify-battery-v2/real/scratch-corpus/manifest.json \
  --base-url http://127.0.0.1:8498 \
  --arm r \
  --expected-digest 7020b925c07c5a3808e1155700ca707598cb4f6d6089bb6daff4147b4d6b00bd \
  --ledger /home/brice/workspace/bloomery/.superpowers/sdd/2026-08-28-refalsify-battery-v2/real/runs/arm-r/ledger.jsonl
```
`--arm r` **exactly** (`ARM_LABEL_R = "r"`).

**Step 4 — recompute (Task 5, POST-GATE, not run now):**
```
PYTHONDONTWRITEBYTECODE=1 python3 -m tools.memory_battery.recompute_v2 \
  --corpus-dir tools/memory_battery/corpus-v1 \
  --arm-m-prime-dir /home/brice/workspace/bloomery/.superpowers/sdd/2026-08-28-refalsify-battery-v2/real/runs/arm-m-prime/data \
  --arm-r-dir /home/brice/workspace/bloomery/.superpowers/sdd/2026-08-28-refalsify-battery-v2/real/runs/arm-r/data \
  --ledger-m-prime /home/brice/workspace/bloomery/.superpowers/sdd/2026-08-28-refalsify-battery-v2/real/runs/arm-m-prime/ledger.jsonl \
  --ledger-r /home/brice/workspace/bloomery/.superpowers/sdd/2026-08-28-refalsify-battery-v2/real/runs/arm-r/ledger.jsonl \
  --expected-digest 7020b925c07c5a3808e1155700ca707598cb4f6d6089bb6daff4147b4d6b00bd
```
`--expected-arm-labels` is **omitted** — it defaults to
`("m_prime", "r")`, exactly the labels this run's ledgers carry (the flag
exists only for the dry shakedown's deliberately-distinct
`M_PRIME_DRY`/`R_DRY` labels, spec §5/task-1 brief). `--corpus-dir`
points at the **frozen** `tools/memory_battery/corpus-v1`, not the
scratch copy: `recompute_v2` reads only `manifest.json`'s per-task name
and `workspace_sha256` fields for `corpus_sha` — fields
`generate_run_manifest` copies byte-for-byte from the frozen manifest
(only `grant.read_roots`/`write_roots` are rewritten, and recompute never
reads those) — so pointing at the frozen tree ties the recomputed
`corpus_sha` unambiguously back to the instrument §3 above just verified,
rather than to a scratch artifact.

## 8. Operational preconditions (v1 §7 adapted + task-2's carried findings)

- **Scratch-copy manifest generation is mandatory, not optional (task-2
  carried finding 1).** Task 4 must run §7 Step 1 before either boot —
  driving the tracked `corpus-v1/manifest.json` directly fails before
  the first HTTP request (grant paths point at a deleted worktree).
- **Never grant a live daemon write access into the tracked `corpus-v1/`
  tree (task-2 carried finding 2).** Found the hard way on the
  shakedown's first attempt. `dry_manifest.py --real`'s scratch-copy
  mechanics are the only sanctioned path from a frozen manifest to a
  drivable one; no ad hoc grant-path rewrite that resolves against
  `corpus_dir`'s own tracked directories is safe, even if the rewriting
  code itself never writes there — the live daemon's own `run`/patch
  actions do.
- **Boot-to-ready budget (task-2 carried finding 3): ~4–4.5 minutes per
  boot on the debug binary, ×2 boots** — budget the overnight window
  accordingly; a release build would be faster if that ruling is made
  before Task 4, but this document pins the debug binary as-is (§2).
- **Boot exactly ONE model.** Both configs above declare exactly one
  `[models."qwen36-reap48-flywheel5"]` stanza (v1 prereg §7's own
  precondition, unchanged mechanism — `driver.py`'s `/status` identity
  assert reads `models[0]["digest"]` positionally).
- **GPU hygiene: `ollama ps` must be empty before every boot.** The
  shakedown observed clean hygiene throughout (empty before M′, empty
  before R, empty at final teardown, no coexisting `bloomery-daemon`
  process) — Task 4 must re-verify this live, not assume it from the
  shakedown's own (now-torn-down) session.
- **PID discipline.** Real pid found via `ps` after `setsid nohup`
  launch (`run_battery.sh`'s own `--pid-file` mechanism — never `$!`,
  which can name an already-exited intermediate process under a
  double-forking `setsid`), confirmed via `readlink /proc/<pid>/exe`
  pointing at the exact binary path (§2) both before driving tasks and
  immediately before `kill`.
- **Poll to fully-ready before submitting any task, not merely to
  `/status` answering.** The shakedown found `/status` answering does
  not mean the daemon is ready: `posting: true` and
  `models[0].mutating_verbs: false` persist while the boot-POST (assay)
  probe and the G4 codec probe run in the background. Poll until
  `posting: false` AND `models[0].codec_gate` is non-null AND
  `models[0].mutating_verbs: true` before the driver's first task
  request — submitting earlier is untested territory (grants include a
  `run`/patch action gated by `mutating_verbs`).
- **Drift-watch: expect `unmeasured`, expect no re-bless, on a fresh
  scratch `data_dir`.** `models[0].drift` reads `{"cumulative": {"status":
  "unmeasured", ...}}` on a first boot into an empty `data_dir` (the
  shakedown's own observation, matching
  `2026-08-26-memory-organ-acceptance.md`'s precedent) — this is the
  expected, intended shape, not a fault; no `POST /models/{name}/bless`
  call should be needed for either arm's first boot.
- **On wrapper-SIGKILL, the ledger is the authority — never eyeball it**
  (v1 prereg §7, unchanged mechanism). `run_battery.sh`'s trap cannot
  observe SIGKILL at all; a DIED-WITHOUT-MARKER run whose ledger shows
  the full `2n` task-half rows for its arm is still complete in every
  way `recompute_v2`'s `completeness` check can measure — that check,
  not a human reading a log tail, decides.
- **Invoke the driver as `--arm m_prime` and `--arm r`, exactly** (v2's
  analogue of v1's `--arm C`/`--arm M` precondition — §7 above). Any
  other label, or either of v1's forbidden `C`/`M` labels, makes
  `recompute_v2`'s arm-label check reject the ledger unconditionally.
- **The corpus grants are worktree-relative to the scratch copy, not the
  frozen tree — intentional, by construction (§6/§7 above).** Do not
  delete the scratch corpus directory (`.../real/scratch-corpus/`)
  before Task 4's both arms and Task 5's recompute complete; it holds
  the actual workspace/pristine bytes the driver reads and resets
  between phases.

## 9. DRY-numbers prohibition (restated)

**Nothing from the task-2 shakedown is evidence.** Every number
`EVIDENCE-NOTES-DRY.md` records — wall-clock ballparks, the G1
`UNMEASURABLE` verdict at n=3, per-task timings — is DRY: discarded,
quoted nowhere in this document as a cost claim, and not comparable to
anything the real run will produce. The shakedown's G1 `UNMEASURABLE`
result is **floor-saturation at n=3**, the code's declared-conservative
branch exercising correctly on a tiny, near-identical-cost sample — it is
an expected consequence of running only 3 tasks, not a prediction about
the real 50-task battery's outcome under either hypothesis. The only
things this document carries forward from the shakedown are: procedure
(boot configs, boot-order quirks, PID discipline), the served-identity
digest (independently re-verifiable at Task 4's own boot, not merely
trusted from the shakedown), and the three carried findings in §8 — never
a cost or gate number.

## Amendment rule

Any amendment to this pre-registration after this commit is a **separate
dated file** in `docs/superpowers/evidence/`, cross-linked from here by a
later commit, and **never** an in-place edit of this document — identical
in force to `docs/gates.md`'s house rule and spec §6's own non-silent
amendment rule. No endpoint, formula, seed, corpus byte, boot config
field, or digest pin changes after a gate number has been seen. The
corpus is bytes (§3 above, re-asserted at this lock); nothing in
`tools/memory_battery/corpus-v1/` is ever edited in place.

## Self-check (task-3 brief step 2)

Every number in this document traces to one of: (a) the binding spec's
own text, quoted or cited by reference, never restated with different
words; (b) a `git hash-object` / `git rev-parse` / `git diff --stat`
computation reproduced live at lock and shown in §3/§5 above; (c) a
literal read from committed source (`recompute_v2.py`'s module-level
constants, `driver.py`'s constants, `run_battery.sh`/`watch_battery.sh`'s
own usage contracts); or (d) a procedural observation carried forward
from the task-2 shakedown and explicitly labeled DRY/non-evidentiary
(§9). No bar, seed, band multiplier, ceiling, digest, or corpus sha was
chosen freehand at prereg time — each has a named, reproducible source
above. The one number this document does **not** and cannot pin is any
G1/G2/A1/H2–H4 gate value itself — those do not exist yet, by
construction, until Task 4's tasks actually run.

## Committed artifacts

- This document,
  `2026-08-28-refalsify-battery-v2-preregistration.md`.
- `tools/memory_battery/corpus-v1/` — unchanged since `memory-battery-v1`'s
  freeze commit `a514bf5`; re-asserted byte-identical at this lock, §3
  above (4/4 checks).
- `tools/memory_battery/recompute_v2.py` — committed `6adab84`, fix round
  `f142ad9`; sha pinned §5.
- `tools/memory_battery/dry_manifest.py` — **post-`--real`**, this lock's
  preliminary commit `8548468`; sha pinned §5.
- `tools/memory_battery/tests/test_dry_manifest.py` — new this session,
  committed alongside `dry_manifest.py`'s `--real` mode at `8548468`; sha
  pinned §5.
- `tools/memory_battery/{driver.py, run_battery.sh, watch_battery.sh,
  recompute.py}` — unchanged since `memory-battery-v1` (driver.py,
  recompute.py) or unchanged on this branch (the shell scripts); shas
  pinned §5.
- `tools/memory_battery/tests/{test_recompute_v2.py,
  test_recompute_v2_cli.py}` — committed `6adab84`..`f142ad9`; shas
  pinned §5.
