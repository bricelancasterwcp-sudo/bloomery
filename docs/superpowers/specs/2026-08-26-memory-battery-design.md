# The memory battery — `memory-battery-v1`: pre-registered repeat-exposure efficiency instrument

**Date:** 2026-08-26 (evening; same day as slice 1's merge)
**Status:** Approved in conversation (Brice's rulings: gating endpoint =
EFFICIENCY on repeats — per-task total completion tokens, median, intent-to-
treat — success rate advisory under a saturation note; arm structure = two
arms, cross-arm gate, C before M; N = 50 tasks; model = fw5 only. Sub-rulings
presented and accepted with the design: C-before-M order fixed to kill the
degree of freedom; every GPU run HUMAN-GATED; the battery is its own
instrument under the memory-organ spec §4's carve-out).
**Lineage:** the memory organ, slice 1 (`2026-08-26-memory-organ-design.md`,
SHIPPED at master `6d8b433`; §1's claim discipline names this battery as the
next slice and forbids capability sentences until it gates; §4 reserves the
memory-on lens for "a future memory battery [that] is its own instrument");
the slice-1 live acceptance (`2026-08-26-memory-organ-acceptance.md`:
mechanism 4/4, the `window_cap 16384` + suspend-after-task residency
protocol); crucible's findings arc (github.com/bricelancasterwcp-sudo/
crucible: ABLATIONS-A — retrieval-only carried the repeat gain; B4 —
retrieval *slowed* the 14B +10% wall, so efficiency is a live question, not a
foregone conclusion); crucible's E3b saturated-bar lesson (the headroom
clause below); the house measurement discipline (`rigorous-experiments`:
derived bars, ITT, none-vs-zero, infra-kill ≠ re-roll, structural corpus
checks); the flywheel factory's run-verified machinery
(`tools/flywheel/factory/planted_test.py`, `templates_run_verified.py` — the
turn-4 fails-before-passes-after validator shape).

## 1. The claim under test, and what may be said afterward

**Pre-registered question:** does exact-repeat injection reduce the cost of
second exposures on bloomery tasks, on this box, for `qwen36-reap48-
flywheel5`? Success-rate lift — crucible's headline — is NOT the gate here:
every gate-passing bloomery model sits at patch ceiling on factory tasks
(fw5 16/16, fw3 15/16), so an accuracy endpoint is born saturated. The
honest translation of the store's value at ceiling is cost.

The gate's verdict licenses exactly one sentence per outcome: PASS — "on
`memory-battery-v1`, exact-repeat injection reduced median second-exposure
completion-token cost by the measured amount"; FAIL — the same sentence with
"did not reduce ... beyond the derived bar"; UNMEASURABLE / INVALID — the
named reason and no cost claim at all. Nothing about novel tasks, other
models, other task shapes, or accuracy. Per the house rule, the point
estimate decides: no extension, no re-run, no corpus change after any gate
number is read. A run killed by infrastructure with **no gate number read**
may be rerun in full from zero; partial data is never spliced.

## 2. Instrument identity and lens

`memory-battery-v1` is its own frozen instrument — the G4/G5 sets are
untouched and stay memory-off. Its declared lens, all pinned in the prereg
before any battery GPU run:

- model: `qwen36-reap48-flywheel5-Q4_K_M.gguf`, digest `7020b925c07c…`,
  served identity asserted from `/status` before every phase;
- envelope v4; `window_cap: 16384` on every battery agent; suspend-after-
  task (the acceptance's residency protocol — one live agent at a time);
- corpus: sha over the frozen corpus tree; corpus seed **20260826**;
- bootstrap seed and B (§4); task order = the frozen manifest order, both
  phases, both arms;
- daemon: the merged master tip's featured build (commit recorded), boot
  config per arm verbatim in the prereg, scratch `data_dir` per arm (the
  production drift baseline untouched);
- the memory-on arm's injection lens is exactly slice 1's mechanism as
  shipped — no organ code changes ride along with this battery.

## 3. Corpus (authored → structurally checked → frozen)

50 planted-defect patch-class tasks from the factory's run-verified shape:
each task = its own workspace directory (a defective module + a planted
failing `unittest`), a goal naming the symptom, grant =
`{read_roots: [ws], write_roots: [ws], commands: [["python3","-m","unittest"]]}`.
Workspaces are content-addressed; a byte-snapshot of every workspace is
taken at freeze and is the reset source for phase 2.

**Structural check, before anything expensive** (the black-oxide rule —
falsifiable endpoints are not sufficient for authored corpora): executed,
not asserted — for every task, `python3 -m unittest` FAILS on the frozen
workspace and PASSES after the factory's own ideal patch is applied to a
throwaway copy (the fails-before validator rule); plus template-family
counts against the declared distribution. The check is minutes of CPU; the
corpus does not freeze until it passes, and the freeze sha goes in the
prereg. After freeze, the corpus is bytes, not code.

## 4. Protocol, endpoints, bars, kill criteria

**Arms and phases.** Two boots, pre-registered order **C then M**, same
night where the box allows:

- Arm C: `[memory] enabled = false`. Phase 1 = tasks 1–50 in manifest
  order, fresh agent per task (`window_cap 16384`), suspend after each;
  full workspace byte-reset (+ `__pycache__` purge — the pyc rule); phase 2
  = same tasks, same order, fresh agents.
- Arm M: `[memory] enabled = true`, store starts EMPTY in a fresh scratch
  `data_dir`. Identical two phases. Phase 1 mints; phase 2 retrieves and
  injects; the store is the treatment and is never touched between phases.

**Cost.** `cost(task)` = the sum of `completion_tokens` over the
`InferCompleted` rows carrying that task's agent id (one fresh agent per
task makes the join exact; recomputed from journal bytes, never from driver
observations). Every task contributes — intent-to-treat: a failed repeat, a
non-injected repeat, and a re-asked step all pay their real cost. A task
whose rows cannot be parsed is `None`, named in a `dropped` list, and never
a zero (none-vs-zero).

**Gating endpoint E1** (the only gate):

> `median_M,p2 ≤ median_C,p2 − Δ_min`, where
> `Δ_min = 2 × SE_boot(median_M,p2 − median_C,p2)`,
> the seeded bootstrap (B = 10,000, seed **20260826**) locked here as a
> formula and computed only at gate time. Resampling unit = tasks: for the
> cross-arm difference each arm's phase-2 tasks resample independently; for
> the within-arm differences (H1, and M's advisory paired deltas) tasks
> resample as p1/p2 PAIRS. Medians are computed over the non-`dropped`
> tasks (§H3 — infra is not a measurement; task conduct is, per the ITT
> rule above). Derived, not chosen; no other number decides.

**Headroom clause** (pre-declared; crucible E3b): if
`median_C,p2 − min_C,p2 < Δ_min` the cost distribution is floor-saturated
and the verdict is **UNMEASURABLE**, not FAIL.

**Hygiene endpoints** (computed before E1 is read, in this order):

- H1 — control stability: `|median_C,p2 − median_C,p1|` within
  `2 × SE_boot` of that difference. A violation means ordering/warmup
  contaminates phase 2 → run **INVALID**.
- H2 — first-exposure equivalence: `|median_M,p1 − median_C,p1|` within
  `2 × SE_boot`. Injection cannot fire on an empty store, so a phase-1 gap
  is instrument error → **INVALID**.
- H3 — infra rate ≤ 5% per arm (task-level: `Error` statuses, daemon
  faults, driver-detected protocol breaks — always counted separately from
  task conduct, never scored as cost data; those tasks are `dropped` for
  E1). Above 5% → **infrastructure kill**: the whole battery may be rerun
  from zero only if no gate number was read.
- H4 (advisory, never gates): injection rate in M-p2 (`MemoryStamp`
  `mode:"injected"` count / 50) — ITT dilution is conservative, so a low
  rate weakens PASS but cannot manufacture one; mint rate in M-p1.

**Advisory endpoints:** per-arm step and wall medians; success rates
(all four phase×arm cells) under a pre-declared saturation note; per-task
paired phase-2−phase-1 deltas within M; the stamp/mint/contradiction row
counts. Reported, never gating.

## 5. Machinery

- **Driver** (`tools/memory_battery/`, Python, GPU-free logic): reads the
  frozen manifest; per task — create agent (`window_cap 16384`) → POST task
  → poll to terminal → suspend agent; per phase — served-identity assertion
  (`/status` digest) before the first task, workspace resets between
  phases. OS-detached for the run (`setsid nohup` + pid file + `.DONE`
  marker; a watcher distinguishes silence from success — every terminal
  state covered). The driver's own ledger is observational; **journal bytes
  are the only source any quoted number may have.**
- **Recompute** (`tools/memory_battery/recompute.py`, house pattern): reads
  both arms' `tasks.jsonl` + `pager.jsonl`, emits every §4 endpoint with
  its lens, `None` + `dropped` for the unparseable. Mutation-tested before
  the battery runs (the cost join, the median, the bootstrap seeding, the
  ITT inclusion rule, none-vs-zero — each broken implementation must fail a
  test). The evidence doc quotes recompute output only.
- Factory glue for corpus generation + the structural checker (executed
  fails-before/passes-after), reusing `planted_test.py` /
  `templates_run_verified.py`.

## 6. Sequence and the lock

1. Plan (writing-plans) → SDD build of generator glue, driver, recompute —
   all GPU-free, TDD, mutation checks as named above.
2. Corpus generation (seed 20260826) → structural check → freeze (sha).
3. **Prereg commit** — endpoints, formulas, seeds, corpus sha, model
   digest, boot configs, arm order — committed and PUSHED before any
   battery GPU run. Amendments after lock are dated footnotes, never
   edits (non-silent amendment rule).
4. **HUMAN GATE: Brice's explicit go**, then arm C, then arm M (each boot's
   preflight: GPU idle, featured build last, served identity).
5. Recompute → hygiene in order → gate → findings doc
   (`docs/superpowers/evidence/` house style), CARRIED-DEBT append at
   merge. If the verdict disappoints, it ships anyway — that is what the
   lock is for.

## 7. Out of scope (named so absence is a decision)

Accuracy on hardened task families (a follow-up battery with its own
prereg, only if this one shows signal — Brice's sequencing ruling); any
second model; any organ code change; non-exact retrieval (the standing
Phase-C prohibition); per-task paired toggling; wall-clock as a gate;
floors for turn-style pass/fail on success rates.

## 8. Delegated to the plan

Exact factory-glue interfaces and the manifest schema; the driver's poll
cadence and terminal-state table; recompute's output schema (with the
completeness test that no schema field can be silently dropped); the
watcher's terminal-state coverage; the corpus's template-family mix (the
plan declares it; the structural check enforces it).
