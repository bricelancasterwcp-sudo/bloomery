# Plan — `premise-gone-battery-v1`

Spec: `docs/superpowers/specs/2026-08-28-premise-gone-battery-v1-design.md`
(binding; formulas cited, never restated). Branch `premise-gone-battery`
in the main checkout — every delta is `tools/` + `docs/`, zero `crates/`
bytes, so the featured binary stays code-identical to master `e3cad71`
(no rebuild dance). Test runner:
`PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s
tools/memory_battery/tests -t .` — read the `Ran N tests`/`OK` line, not
the fixture tables. Mutation checks: revert + touch, purge
`__pycache__`, per the pyc rule.

## Task 1 — `corpus_pg.py` (generator)

Invariants:
- Same factory draw discipline as `corpus.py` (one `random.Random(seed)`,
  overdraw-and-trim, run-verified python filter), seed 20260828, n=50.
- Per task: `pristine/` = factory files verbatim; `pristine_p2/` =
  same target bytes + moved-on test at the same filename; `workspace/`
  = `pristine/` copy.
- Moved-on authoring: AST-parse the planted test's single
  `assertEqual(call, literal)`; evaluate `call` against the DEFECTIVE
  module in a subprocess with `planted_test.run_python`'s env shape;
  replace ONLY the expected literal with the result's repr. A raising
  defective call excludes the task (redraw, bounded, BLOCKED-not-shrink).
- Manifest = corpus-v1 schema + `"pristine_p2"` + `"workspace_p2_sha256"`
  + `"instrument": "premise-gone-battery-v1"`.
- Determinism: same (seed, n) → byte-identical fields modulo out_dir.

Falsification tests: authored literal equals subprocess-observed
defective value (a hand-built fixture with known defective output);
raising-defect exclusion actually excludes (fixture whose defect
raises); determinism (two generations, field-identical); target bytes
`pristine/` == `pristine_p2/`; test file differs. Mutation: pin the
wrong literal → test fails; skip the exclusion → test fails.

## Task 2 — `corpus_check_pg.py` (S1–S5)

Invariants: spec §3's five checks, executed not trusted; independent
sha implementation (deliberate duplicate, corpus_check.py's rule);
every task yields one result row, corpus-level failures named, CLI
prints a verdict and exits nonzero on any failure.

Falsification tests: seeded tiny corpora each breaking exactly one of
S1–S5 (defect absent → S1 fails; search count ≠ 1 → S2; target bytes
drifted in p2 → S3; moved-on test passing on fixed target → S4; sha
mismatch → S5). Mutation: invert S4's expected exit → its test fails.

## Task 3 — generate + freeze `corpus-pg-v1`

Run the generator into `tools/memory_battery/corpus-pg-v1/`, run the
checker (all green), commit. Bytes thereafter — the amendment rule
attaches at this commit. Record the manifest sha256 in the ledger for
the prereg's cross-check.

## Task 4 — `driver.py` per-phase source

Invariant: `_reset_workspace` gains a source-dir parameter; `run_arm`'s
phase-2 reset uses the task's `pristine_p2` (resolved against the
manifest's own directory) when the key is present, else the sibling
`pristine/`. **Compat: key absent → byte-identical behavior to today.**

Falsification tests: fake-server arm run over a manifest WITH the key —
phase 2's workspaces contain the p2 bytes; over a manifest WITHOUT it —
phase 2 gets pristine bytes (the existing tests keep passing
unmodified is itself the compat pin). Mutation: make phase 1 use the
p2 source → test fails.

## Task 5 — `dry_manifest.py` p2 carry

Invariant: a task with `pristine_p2` gets it scratch-copied beside
`workspace/`+`pristine/`, and the output manifest's key points at the
scratch copy; tasks without the key are untouched; dry/real semantics
unchanged.

Falsification tests: scratch layout contains `pristine_p2/` with the
frozen bytes; grant paths never point into the frozen tree (existing
pin extended to the new key). Mutation: drop the p2 copy → test fails.

## Task 6 — `recompute_pg.py`

Invariants: spec §5 verbatim — PG1/PG2/PG3, floor 25, stamp audit,
H2 (seed 20260829, B=10,000, `HYGIENE_SE_MULTIPLIER`), H3
(`INFRA_RATE_CEILING`), H4, A1/A2/A3, completeness, identity
(`--expected-digest` FATAL on mismatch — CLI-enforced, the named bug
class), `dropped`, corpus sha. Arms `m_prime`/`r`; inputs = two data
dirs, two ledgers, two `episodes.jsonl` paths, corpus dir. Matched-set
definitions exactly spec §4's. None-vs-zero throughout.

Falsification tests: synthetic journals/ledgers/stores covering — a
premise_held stamp → PG1 alarm surfaces; an injected R-p2 row → PG1
FAIL; a contradicted episode row → PG2 FAIL; M′ injected below floor →
UNMEASURABLE; swapped-arm ledgers → identity/label FATAL; digest
mismatch → nonzero exit (the test asserts the exit code, not just the
message). Mutation: break the spelling counter, break the store-status
read, drift the seed literal → each fails a test.

## Task 7 — dry shakedown (3 tasks, both arms, live daemon)

`dry_manifest.py` (dry mode, count 3) → boot M′ config → run → tear
down → boot R config → run → tear down. Numbers discarded, notes
marked DRY. Purpose: probe fires premise_gone live on at least one
matched retrieval; driver p2 materialization verified on the real
seam; teardown clean; `git status` clean afterward (the tracked-tree
rule, checked explicitly).

## Task 8 — prereg lock, launch, run, findings

Prereg doc mirroring battery-v2's (lens pins incl. daemon commit
re-verified crates-empty-diff at lock; configs verbatim with fresh
scratch `data_dir`s; every locked number; machinery shas; operational
checklist incl. GPU hygiene and the `readlink /proc/<pid>/exe` rule;
amendment rule verbatim). Commit BEFORE any real boot. Launch under the
recorded delegation; M′ then R; watcher; recompute once; findings doc
quoting recompute output only; CARRIED-DEBT + memory; merge + push per
standing rulings.
