# Refalsify-Battery-v2 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build and lock the refalsify-battery-v2 instrument (recompute_v2 + dry-run shakedown + pre-registration), then — after Brice's launch gate — run the overnight M′/R battery and produce the findings.

**Architecture:** New `tools/memory_battery/recompute_v2.py` reusing the v1 loaders/bootstrap (v1's `recompute.py` is a locked historical instrument — never edited); a 3-task dry shakedown of both arm configs against the live daemon; the prereg doc as the lock; the run and findings gated behind the human launch ruling.

**Tech Stack:** Python (tools/memory_battery, its existing test style), bloomery daemon (Rust, already built — no Rust changes in this plan).

**Spec:** `docs/superpowers/specs/2026-08-28-refalsify-battery-v2-design.md` — binding authority; §4's endpoint formulas and §6's sequence are the requirements. Battery-v1's design + prereg (`2026-08-26-memory-battery-design.md`, `docs/superpowers/evidence/2026-08-26-memory-battery-preregistration.md`) are the house patterns to mirror.

## Global Constraints

- Work on a worktree branch off bloomery master `644482f`; merges/pushes/launches only on Brice's explicit rulings.
- **v1 instrument immutability**: `tools/memory_battery/recompute.py`, `driver.py`, `corpus.py`, `corpus_check.py`, the recompute_* helpers, and `corpus-v1/` are NOT edited. recompute_v2 imports/reuses; if a helper genuinely cannot be reused without modification, STOP and report (that is a design question, not a refactor call).
- Pre-registration discipline: no gate formula may be computed against real arm data before the prereg commit; the dry run's numbers are marked DRY and discarded (quoted nowhere).
- Locked numbers (spec §4): bootstrap B=10,000, seed **20260828**; G1 band = 2×SE_boot of the p2 median difference; H3 infra bar 5%; arm order M′ then R; corpus = frozen corpus-v1, manifest sha re-asserted.
- Python test runner: match `tools/memory_battery/tests`' existing invocation (read its README/test files; battery-v1 used pytest-style via python3 — mirror exactly). Mutation checks: revert-then-touch, purge `__pycache__`, PYTHONDONTWRITEBYTECODE=1.
- Full output to a file + `echo exit=$?` on every verification run; never pipe through tail/head without capturing exit.
- No cargo anything (daemon untouched; featured binary must stay newest).
- GPU hygiene before any live-daemon step: `ollama ps` empty (stop anything listed); the bloomery boot re-bless note stands (drift watch may flag hybrid geometry via PYTHONPATH-tracked assay — intended honesty, re-bless per procedure).

---

### Task 1: `recompute_v2.py` + tests (TDD + mutation)

**Files:**
- Create: `tools/memory_battery/recompute_v2.py`, `tools/memory_battery/tests/test_recompute_v2.py` (mirror the existing tests' fixture style — read `tools/memory_battery/tests/` first)
- Read-only reference: `recompute.py` (loaders `_load_arm`, wall/costs/modes views, completeness/identity checks), `recompute_bootstrap.py` (seeded bootstrap), `recompute_journal.py` (journal row parsing incl. MemoryStamp)

**Interfaces:**
- Consumes: `_load_arm`-style per-arm views (import from recompute/private helpers if importable; if the underscore-privacy makes importing them fragile, re-export via a tiny shared module WITHOUT editing recompute.py — a new `recompute_shared.py` that recompute.py does NOT import; state the choice in the report).
- Produces: `recompute_v2(corpus_dir, arm_m_prime_dir, arm_r_dir, ledger_m_prime, ledger_r, *, expected_digest=None, seed=20260828, b=10_000, expected_arm_labels=("m_prime", "r")) -> dict` — `expected_arm_labels` exists so the dry shakedown (ledger labels `M_PRIME_DRY`/`R_DRY`) parses without weakening the real run's label check; the CLI default stays `("m_prime", "r")` and the label check still REJECTS v1's C/M unconditionally and a CLI `python3 -m tools.memory_battery.recompute_v2 ...` mirroring v1's CLI shape (incl. the `--expected-digest` carry-note behavior from v1 prereg §6). Output dict keys (exact): `g1` (medians, diff, se_boot, band, verdict PASS/FAIL/UNMEASURABLE per the floor-saturation rule), `g2` (both injected counts, verdict), `stamp_audit` (per arm×phase spelling counts over non-dropped tasks + verdicts: premise_held_complete, forbidden_spellings_absent, premise_gone_zero, inconclusive/skipped counts), `a1_wall` (p2 medians+delta, p1 medians+delta control, per_probed_retrieval_ms, distribution summary), `h2_p1_equivalence`, `h3_infra`, `h4_advisory` (mint rate p1, retrieval rate p2, per arm), `dropped` (per arm), `corpus_sha`, `lens` (seed, b, arm labels, source paths).
- Arm labels everywhere: `m_prime` and `r` — never c/m.

- [ ] **Step 1: Write failing tests first.** Fixture style: synthetic journals/ledgers as the existing recompute tests build them (read and reuse their builders). Cover, minimum: G1 verdicts (equivalent-within-band; outside-band FAIL; floor-saturated UNMEASURABLE); G2 equality PASS / deficit FAIL / excess flagged as alarm-not-pass; stamp audit (all-premise_held PASS; one `failed` spelling → forbidden verdict; one premise_gone → alarm); A1 wall arithmetic incl. per-probe derivation and p1 control; H2 equivalence; H3 over-bar; none-vs-zero (unparseable task → None + dropped, never 0); arm-label honesty (output carries m_prime/r; the ledger arm-name check rejects a ledger labeled C or M).
- [ ] **Step 2: RED run** (runner per Global Constraints; quote failures).
- [ ] **Step 3: Implement** recompute_v2 per the Interfaces block. Bootstrap: reuse `recompute_bootstrap`'s seeded machinery with seed 20260828; the SE and band computed only inside `g1`/`h2` result assembly — no other superiority/inferiority endpoint exists in this module (E1 is v1's, not copied).
- [ ] **Step 4: GREEN**, full tools test dir green.
- [ ] **Step 5: Mutation checks** (each: mutate → named test FAILS quoted → revert + touch + pycache purge → green): (1) median → mean in G1; (2) cost join keyed to the wrong id field; (3) stamp spelling counter counts `premise_gone` as `premise_held`; (4) arm dirs swapped inside recompute_v2; (5) seed drifts (20260828 → any literal) — a fixture with asymmetric noise must produce a changed band that a test pins.
- [ ] **Step 6: Commit** `feat: recompute_v2 — refalsify-battery-v2 endpoints (G1/G2/stamp-audit/A1/H2-H4), m_prime/r arms`

### Task 2: Dry-run shakedown (live daemon, numbers discarded)

**Files:**
- Create: `tools/memory_battery/dry_manifest.py` (tiny: writes a 3-task manifest subset from the frozen manifest, stamping `"dry": true` into it), scratch outputs under the battery workspace (not committed), `EVIDENCE-NOTES-DRY.md` in the SDD workspace (not the repo) recording the shakedown observations.
- Read-only: v1 prereg §7 operational preconditions (`docs/superpowers/evidence/2026-08-26-memory-battery-preregistration.md`) — the boot/ops checklist to follow; the daemon config format for `[memory] enabled/refalsify` (find the config file the daemon boots with — grep the daemon docs/boot scripts; battery-v1's arm boots are the precedent).

**Interfaces:**
- Consumes: Task 1's recompute_v2 CLI (the dry journals must parse through it end-to-end).
- Produces: verified arm-config file contents for M′ and R (pinned verbatim into Task 3's prereg), the served-identity digest, and the shakedown verdict.

- [ ] **Step 1:** GPU hygiene + boot the daemon with the M′ config (memory on, refalsify off) per the v1 §7 checklist; `/status` digest recorded; handle the drift-watch re-bless if prompted (per procedure — it is intended honesty, not an error).
- [ ] **Step 2:** `dry_manifest.py` → 3-task manifest; drive those 3 tasks (phase-1-style mint + phase-2-style repeat with byte reset, mirroring the driver's per-phase reset procedure — use the real driver with the dry manifest, `--arm M_PRIME_DRY`).
- [ ] **Step 3:** Reboot with the R config (refalsify on); same 3 tasks both phases, `--arm R_DRY`. Verify in the R dry journal: p2 stamps carry `refalsify:"premise_held"` and mode `injected` (the v2 prediction, observed live); no `passed`/`failed` spellings anywhere.
- [ ] **Step 4:** Run recompute_v2 over the dry outputs — it must execute end-to-end and emit every §4 key (values DRY, discarded, quoted nowhere; assert only shape/parseability). Any parse gap = fix loop in Task 1's code before proceeding.
- [ ] **Step 5:** Tear down (daemon stopped); record in the SDD workspace notes: config file contents per arm, digest, boot procedure quirks, wall-clock ballpark per task (for scheduling only, marked DRY).
- [ ] **Step 6: Commit** (dry_manifest.py only) `feat: dry-manifest subset tool for battery shakedowns`

### Task 3: Pre-registration (the lock), then STOP for the launch gate

**Files:**
- Create: `docs/superpowers/evidence/2026-08-28-refalsify-battery-v2-preregistration.md`

- [ ] **Step 1:** Mirror v1's prereg structure section-for-section: claim discipline (spec §1 quoted verbatim incl. named absences); lens pins (daemon commit sha serving the run, config file content per arm verbatim from Task 2, served-identity digest, model identity); corpus sha re-assertion (recompute the manifest sha, compare against v1's frozen value — quote both); protocol/endpoints by reference to spec §4 plus every locked number restated (seed 20260828, B=10k, bars, arm order M′→R); machinery shas at lock (`git hash-object` on recompute_v2.py, driver.py, run_battery.sh, watch_battery.sh, dry_manifest.py, the corpus manifest); operational preconditions (v1 §7 adapted: GPU hygiene, box quiet, watcher armed, per-arm boot checklist from Task 2's notes); the amendment rule (v1's, verbatim).
- [ ] **Step 2:** Self-check: every number in the prereg traces to the spec or a computed sha — nothing chosen at prereg time.
- [ ] **Step 3: Commit** `docs: refalsify-battery-v2 pre-registration (locked before any measured run)` — **STOP: present the branch for Brice's merge ruling and the overnight-launch gate.** Nothing below runs before both.

### Task 4 (POST-GATE): The overnight run

- [ ] Arm M′ boot per pinned config → served-identity assert → `run_battery.sh` detached with watcher → completion verified (DONE marker + ledger completeness) → daemon down.
- [ ] Arm R boot → same → down. Same night. Any infrastructure kill follows the spec's rule (rerun from zero only if no gate number was read).
- [ ] Journals + ledgers collected into the evidence layout the prereg names. No numbers read beyond the watcher's liveness signals.

### Task 5 (POST-GATE): Recompute → findings → records

- [ ] `recompute_v2` over both arms (with `--expected-digest`); output committed verbatim as the findings doc's data section.
- [ ] Findings doc `docs/superpowers/evidence/2026-08-28-refalsify-battery-v2-findings.md`: verdicts G1/G2/stamp-audit, A1 wall numbers with the p1-control honesty check, hygiene, dropped lists, the licensed claim sentence (or the honest failure), named absences restated.
- [ ] CARRIED-DEBT + memory updates; STOP for the final merge/push ruling.
