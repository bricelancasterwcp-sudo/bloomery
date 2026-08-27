# Memory Battery (memory-battery-v1) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build and run the pre-registered repeat-exposure efficiency battery: 50 frozen run-verified tasks, two arms (memory-off, memory-on), gate on median phase-2 completion-token cost with a derived bar.

**Architecture:** A new Python package `tools/memory_battery/` — corpus generator reusing the flywheel factory's run-verified machinery, an executed structural checker, an HTTP driver that runs the four phase-halves against the live daemon, and a recompute tool that derives every quoted number from journal bytes. Tasks 1–4 are GPU-free TDD; Tasks 5–8 are the lock, the two human-gated GPU runs, and the gate.

**Tech Stack:** Python 3 stdlib only (`unittest`, `urllib.request`, `json`, `random`, `hashlib`, `statistics`) — no new dependencies, matching `tools/flywheel`'s conventions. Rust untouched: **no crate source file changes anywhere in this plan.**

**Spec:** `docs/superpowers/specs/2026-08-26-memory-battery-design.md` — the binding authority; §4's formulas are quoted there once and are never restated with different words anywhere (this plan cites, it does not paraphrase numbers).

## Global Constraints

- No organ or daemon code changes ride along (spec §2). The daemon binary for the runs is the featured build of the merged master tip.
- Seeds: corpus **20260826**, bootstrap **20260826**, B = **10,000** (spec §4). `Date.now`-style ambient values never enter generation or recompute.
- Instrument honesty: `None` + a named `dropped` list for anything unmeasured; no field may default to something that looks like a measurement; infra is never scored as cost (spec §4 H3).
- Python tests: stdlib `unittest`, files under `tools/memory_battery/tests/`, run with `python3 -m unittest discover -s tools/memory_battery/tests` (mirror `tools/flywheel/tests`). The pyc rule: every mutation-check step purges `__pycache__` and sets `PYTHONDONTWRITEBYTECODE=1`.
- Rust suite untouched; before any commit run the Python tests plus `git diff --stat` to confirm no `crates/` file moved.
- Branch `memory-battery` in worktree `.worktrees/memory-battery`; commit format `<type>: <description>`, no attribution trailers.
- GPU steps (Tasks 6–7) are HUMAN-GATED: Brice's explicit go per run, GPU-idle preflight, featured build LAST before boots (a `cargo test` after it clobbers the binary — rebuild `-p bloomery-daemon --features vulkan` if in doubt).

---

### Task 1: Corpus generator and manifest (`tools/memory_battery/corpus.py`)

**Files:**
- Create: `tools/memory_battery/__init__.py` (empty), `tools/memory_battery/corpus.py`
- Test: `tools/memory_battery/tests/test_corpus.py` (+ empty `tests/__init__.py`)

**Interfaces:**
- Consumes (read these before writing anything): `tools/flywheel/factory/task.py:66` `Task` NamedTuple — the battery uses `name`, `lens`, `target`, `files` (dict name→contents, includes `test_file`), `goal`, `search`, `replace`, `trajectory`, `run_argv`, `commands`, `test_file`; `tools/flywheel/factory/generate.py:108` `generate_candidate_tasks(...)` and `:124` `dedup_tasks(...)` (read their signatures and the family registry they draw from — select ONLY tasks whose `trajectory` is the run-verified shape and whose `lens` is `"python"`; the exact trajectory constant lives in `task.py`'s `TRAJECTORIES`).
- Produces:
  - `generate_corpus(seed: int, n: int, out_dir: Path) -> Manifest` — draws with `random.Random(seed)`, dedups, takes the first `n` run-verified tasks in draw order, materializes each task's `files` into `out_dir/tasks/<task.name>/workspace/` AND a byte-identical `out_dir/tasks/<task.name>/pristine/`, and writes `out_dir/manifest.json`.
  - Manifest schema (pinned; the completeness test in Task 4 covers it): top level `{"instrument": "memory-battery-v1", "corpus_seed": int, "n": int, "families": {family: count}, "tasks": [...]}`; per task `{"name", "family", "workspace" (relative path), "goal", "grant": {"read_roots": [abs ws], "write_roots": [abs ws], "commands": [[...]]}, "run_argv": [...], "search", "replace", "target", "test_file", "workspace_sha256"}` where `workspace_sha256` is the sha256 over the sorted `(relative_path, file_bytes)` sequence and `grant.commands` comes verbatim from `Task.commands`.
  - INVARIANT (falsification test): regenerating with the same seed yields byte-identical workspaces and manifest (minus absolute-path fields, which derive from `out_dir`); a different seed yields a different task list.
  - INVARIANT: `families` counts equal the observed per-task `family` values — computed, never declared independently.

- [ ] **Step 1: Write the failing tests** — `generate_corpus(seed=1, n=6, tmp)` produces 6 task dirs each containing the target and the planted `test_file`, pristine == workspace byte-for-byte, manifest parses with every pinned field, determinism (same seed twice → identical `workspace_sha256` list), seed sensitivity, and every task's `trajectory` was run-verified (assert `run_argv` non-empty and `test_file` non-empty for all).
- [ ] **Step 2:** `python3 -m unittest discover -s tools/memory_battery/tests` — expected: import error.
- [ ] **Step 3:** Implement. Draw more candidates than `n` (the factory over-draws; filter then trim). If the factory cannot yield `n=50` distinct run-verified python tasks at the real seed, that is a BLOCKED report, not a silent shrink.
- [ ] **Step 4:** Tests green.
- [ ] **Step 5: Commit** `feat: memory-battery corpus generator and manifest`.

---

### Task 2: Structural checker (`tools/memory_battery/corpus_check.py`)

**Files:**
- Create: `tools/memory_battery/corpus_check.py`
- Test: `tools/memory_battery/tests/test_corpus_check.py`

**Interfaces:**
- Consumes: Task 1's manifest + workspaces; `tools/flywheel/factory/planted_test.py`'s fails-before machinery (reuse its child-environment discipline — `python3` off `PATH=/usr/bin:/bin`, `HOME=cwd`, `LANG=C` — by CALLING its public function if one fits, else by mirroring the documented env exactly and saying so in a comment citing the module).
- Produces: `check_corpus(corpus_dir: Path) -> CheckReport` and a `__main__` CLI exiting nonzero on any failure. Checks, all EXECUTED (spec §3):
  1. fails-before: for every task, the planted test run in a throwaway copy of the pristine workspace exits nonzero;
  2. passes-after: apply `search`→`replace` to `target` in another throwaway copy (exact-once occurrence required — zero or >1 is a corpus defect), rerun; exit 0 required;
  3. `workspace_sha256` recomputed == manifest value for every task; pristine == workspace;
  4. family counts == manifest `families`.
  `CheckReport` lists per-task verdicts; any `dropped`/unrunnable task is a named failure, never skipped.
- INVARIANTS (falsification tests, real corpora built via Task 1 in tmp): a corpus with the defect pre-fixed fails check 1; a corpus whose `replace` breaks the test fails check 2; a flipped byte in one workspace fails check 3; a doctored manifest count fails check 4.

- [ ] **Step 1:** failing tests per the four invariants. **Step 2:** RED. **Step 3:** implement. **Step 4:** GREEN.
- [ ] **Step 5: Mutation checks** (purge `__pycache__`, `PYTHONDONTWRITEBYTECODE=1`): (a) invert fails-before (accept exit 0) → invariant-1 test FAILS; (b) skip passes-after → invariant-2 test FAILS. Restore, re-run green.
- [ ] **Step 6: Commit** `feat: memory-battery structural checker — executed fails-before and passes-after`.

---

### Task 3: Driver (`tools/memory_battery/driver.py` + detach wrapper)

**Files:**
- Create: `tools/memory_battery/driver.py`, `tools/memory_battery/run_battery.sh`, `tools/memory_battery/watch_battery.sh`
- Test: `tools/memory_battery/tests/test_driver.py`

**Interfaces:**
- Consumes: the manifest; the daemon HTTP surface exactly as the slice-1 acceptance used it — `POST /agents` `{"model", "window_cap": 16384}` → `{id}`; `POST /agents/{id}/task` `{goal, grants}` → 202 `{task_id}`; `GET /agents/{id}/task/{task_id}` → `{status, steps, summary}`; `POST /agents/{id}/suspend`; `GET /status` (served-identity: `models[0].digest`).
- Produces: `run_arm(manifest, base_url, arm_name, expected_digest, ledger_path) -> None` executing: identity assert → phase 1 (manifest order: create agent → submit → poll 5 s cadence, 600 s per-task deadline → suspend) → reset every workspace from pristine (byte-copy + `__pycache__` purge — the pyc rule) → identity assert → phase 2 (same order, fresh agents). Ledger: one JSONL row per task-half `{arm, phase, task, agent_id, task_id, status, wall_s, ts}` — observational only; a doc comment states journals are the only quotable source.
- Terminal-state table (pinned): poll ends on any non-`Running` status; a poll deadline or HTTP failure records `status: "driver-infra"` and CONTINUES to the next task (H3 counts it); the driver never retries a task and never reorders.
- `run_battery.sh`: `setsid nohup python3 -m tools.memory_battery.driver ... & `-style detach writing `<out>/driver.pid` and, on any exit, `<out>/driver.DONE` containing the exit code (trap-based — cover every terminal state). `watch_battery.sh`: poll marker-or-pid-death; silence must be distinguishable from success.
- INVARIANTS (tests drive `run_arm` against a stdlib `http.server` fake scripted per-request; no GPU): manifest order preserved both phases; suspend called after every task incl. failed ones; identity mismatch aborts the arm BEFORE any task (asserting no task requests were made); a scripted non-Running status ends that task's polling; a scripted 500 records `driver-infra` and the next task still runs; resets restore pristine bytes and remove `__pycache__`.

- [ ] **Step 1:** failing tests per invariants. **Step 2:** RED. **Step 3:** implement. **Step 4:** GREEN.
- [ ] **Step 5: Mutation checks:** (a) drop the suspend call → suspend-count test FAILS; (b) skip the identity assert → mismatch test FAILS. Restore, green.
- [ ] **Step 6: Commit** `feat: memory-battery driver — phased runner, identity asserts, detach wrapper`.

---

### Task 4: Recompute (`tools/memory_battery/recompute.py`)

**Files:**
- Create: `tools/memory_battery/recompute.py`
- Test: `tools/memory_battery/tests/test_recompute.py`

**Interfaces:**
- Consumes per arm: `data_dir/journal/tasks.jsonl` (the `MemoryStamp` row every spawned task gets — mode `off`/`silent`/`injected` — is the task_id→agent_id join; `TaskStep` rows give steps and the done-verb presence; `MemoryMint` rows give mint counts) and `data_dir/journal/boot-*.jsonl` (`InferCompleted{id, completion_tokens}` rows give cost), plus the driver ledger (ONLY for mapping driver task names to daemon task_ids via its recorded pairs, and for `driver-infra` flags).
- Produces: `recompute(corpus_dir, arm_c_dir, arm_m_dir, ledger_c, ledger_m) -> dict` emitting EXACTLY the spec-§4 endpoints — E1 with `delta_min` and verdict ∈ {PASS, FAIL, UNMEASURABLE, INVALID}; hygiene H1–H3 evaluated in spec order BEFORE E1 (any INVALID short-circuits E1 to INVALID); H4 + all advisories; a `lens` block (model digest read from the boot journal's identity rows, envelope, window_cap, corpus sha, seeds, B); per-arm `dropped` lists with reasons. Bootstrap exactly as §4 locks it: `random.Random(20260826)`, B=10,000, cross-arm independent resampling / within-arm paired resampling. `cost(task) = sum(completion_tokens over the task's agent's InferCompleted rows)`; success := a `TaskStep` with `verb == "done"` exists; infra := `driver-infra` flag OR a task_id with no `MemoryStamp` row.
- INVARIANT: every quoted number in the output derives from journal bytes or the frozen manifest — the ledger contributes only join pairs and infra flags (a test feeds a ledger with a WRONG wall_s and asserts the output is unchanged).
- Completeness test: every key in the pinned output schema present in the emitted JSON (a new field cannot be silently dropped); serialization round-trips.
- Fixture strategy: tests synthesize small journal files by hand (5–8 tasks) with known arithmetic — including one task with a re-ask (two `InferCompleted` rows), one `dropped`, one non-injected repeat — and assert exact medians, exact ITT inclusion, exact `None` handling.

- [ ] **Step 1:** failing tests (known-arithmetic fixtures + completeness + ledger-independence). **Step 2:** RED. **Step 3:** implement. **Step 4:** GREEN.
- [ ] **Step 5: The five named mutation checks** (spec §5; pyc discipline each time): break the cost join (sum prompt_tokens instead) → arithmetic test FAILS; break the median (mean) → FAILS; unseed the bootstrap → determinism test FAILS (same inputs twice must emit identical `delta_min`); break ITT (exclude non-injected repeats) → ITT test FAILS; zero-fill a dropped task → none-vs-zero test FAILS. Restore, green.
- [ ] **Step 6: Commit** `feat: memory-battery recompute — journal-derived endpoints, seeded bootstrap, ITT`.

---

### Task 5: Corpus generation, freeze, and the prereg lock (CPU only)

**Files:**
- Create: `tools/memory_battery/corpus-v1/` (committed — the frozen instrument), `docs/superpowers/evidence/2026-08-26-memory-battery-preregistration.md`

- [ ] **Step 1:** `generate_corpus(seed=20260826, n=50, tools/memory_battery/corpus-v1/)`; run `corpus_check` — must pass 4/4 checks on all 50; record the report.
- [ ] **Step 2:** Freeze sha: sha256 over the sorted manifest + workspace bytes; record.
- [ ] **Step 3:** Write the prereg doc: spec-§4 formulas BY REFERENCE (cite the spec section, restate nothing with different words), plus the concrete pins — corpus sha, per-task `workspace_sha256` table, model digest `7020b925…`, envelope v4, window_cap 16384, arm order C→M, boot configs VERBATIM (two scratch data_dirs, `[memory]` off/on, port per arm), seeds, B, the daemon commit to be built, driver/recompute file shas at lock. State the licensing sentences (spec §1) and the no-extension/no-reroll/no-splice rules.
- [ ] **Step 4:** Commit corpus + prereg (`docs: memory-battery-v1 corpus freeze and preregistration`), push the branch, **verify origin sync** — the lock is public before any GPU number exists.

---

### Task 6: HUMAN GATE — arm C (memory-off) run

**STOP: Brice's explicit go required.**

- [ ] Preflight (runbook style): GPU idle; no daemon; merged-master featured build (`cargo build --release -p bloomery-daemon --features vulkan` LAST, `nm | grep -c ggml_vulkan` ≥ 1); assay pin clean; scratch data_dir fresh.
- [ ] Boot with the prereg's arm-C config; wait for the codec gate (G4 must pass — a demotion is an infrastructure stop, not a datum); record `/status`.
- [ ] Launch `run_battery.sh` (detached) for arm C; watcher up; on `.DONE`, verify exit 0, archive `data_dir/journal/*` + ledger into the run root; kill the daemon (pid via `ps`+`readlink`, never `$!`).
- [ ] NO recompute yet — no number is read before both arms complete (spec §6).

### Task 7: HUMAN GATE — arm M (memory-on) run

- [ ] Same preflight; fresh scratch data_dir (empty store); boot with the arm-M config; run; archive; shutdown. Workspaces are regenerated from pristine before the arm starts (the driver does it; verify one `workspace_sha256` by hand).

### Task 8: The gate and the findings doc

- [ ] Run `recompute` once over both archives. The output IS the verdict: hygiene in order, then E1 (or UNMEASURABLE/INVALID). No re-runs, no extensions, whatever it says.
- [ ] Write `docs/superpowers/evidence/2026-08-2X-memory-battery-findings.md`: recompute output quoted verbatim, the licensed sentence (spec §1) and NOTHING stronger, per-arm run narratives, dropped lists, the lens block, deviations-if-any as dated footnotes. Append CARRIED-DEBT (settled / deferred / lessons). Commit; merge/push is Brice's call (finishing-a-development-branch).

---

## Self-Review (performed while writing)

- **Spec coverage:** §1 licensing → Tasks 5/8; §2 lens pins → Tasks 5 prereg + 6/7 preflights; §3 corpus + structural check → Tasks 1/2/5; §4 protocol/endpoints/bars/kill criteria → Tasks 3 (protocol), 4 (endpoints, in-order hygiene, ITT, none-vs-zero), 6/7 (arm order, human gates), 8 (gate discipline); §5 machinery incl. the five mutation checks → Tasks 3/4; §6 sequence + lock-before-GPU → Task 5 step 4 explicitly pushes before runs; §7 out-of-scope honored (no Rust changes, one model, no organ edits); §8 delegations all pinned here (manifest schema T1, terminal-state table T3, output schema + completeness T4, watcher coverage T3, family mix declared-by-computation T1).
- **Type consistency:** `generate_corpus`/`check_corpus`/`run_arm`/`recompute` names and argument shapes match across tasks; the manifest fields Task 3 reads (`goal`, `grant`, `workspace`) and Task 4 reads (`workspace_sha256`, task names) are all in Task 1's pinned schema.
- **Placeholder scan:** no TBDs; the two deliberate read-the-source directives (factory family registry, planted_test's public surface) name exact files/lines and the invariant the implementer must satisfy, per the plans-state-invariants rule.
