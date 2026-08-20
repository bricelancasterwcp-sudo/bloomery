# Swap-candidate endpoint Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `POST /models/{name}/swap-candidate` — an advisory, journaled verdict on whether a candidate GGUF covers what the named model's blessed baseline measured, evidenced by a daemon-probed profile and `assay cover`.

**Architecture:** A new `swap.rs` module holds a `CoverGate` (subprocess wrapper for `assay cover`, mirroring `DriftGate`'s runner-injected design and the post-PR-#14 four-code reading) and a one-slot job state machine. The HTTP surface gains two arms: POST starts the job after preconditions (404 unknown model, 409 no baseline, 409 busy) and returns 202 — a request handler cannot hold a ~10-minute probe, the same reason the boot watch probes on its own thread; GET reads the job's state. A worker thread runs the flow: register the candidate under a scratch identity, probe it through the daemon's own `/v1` with the identical POST invocation, run `cover` against the blessed baseline, journal the verdict with digests, unload and unregister. Advisory only: nothing blocks, nothing auto-swaps, the admission policy table is untouched.

**Tech Stack:** Rust (workspace crates `bloomery-daemon`/`bloomery-core`), assay ≥ 0.13.0 via the daemon-process `PYTHONPATH` pin.

**Spec:** [`docs/superpowers/specs/2026-08-19-swap-candidate-seam-design.md`](../specs/2026-08-19-swap-candidate-seam-design.md) (§4 is this plan's contract; §3's `cover` ships first as assay v1.11 — assay PR #6's plan).

## Global Constraints

- **Sequencing:** assay v1.11 (`assay cover`, exits 0/1/2/3, precedence 2 > 3 > 1 > 0) must be MERGED to assay master before Task 5 (live acceptance) — the PYTHONPATH pin tracks assay master. Tasks 1–4 need no live assay (runner-injected fixtures).
- **Suite:** `cargo test -p bloomery-core -p bloomery-daemon` from the worktree root. Baseline on arrival: **532 passed**. Green before every commit.
- **Featured builds:** the daemon binary for live runs is `cargo build -p bloomery-daemon --features vulkan` — workspace-root `--features` does NOT reach the daemon crate, and a later `cargo test` OVERWRITES the featured binary (test first, featured build last). Evidence doc `2026-08-19-standing-v10-baseline.md` records both traps.
- **NEVER wrap a command in `timeout`** on this box (uutils segfault); never `pkill` by bare name — kill by verified PID (`readlink /proc/$PID/exe` first).
- **Advisory slice:** the admission policy table (`only_a_confirmed_cumulative_reading_blocks_admission`) must be byte-for-byte untouched, and a test pins that no swap outcome enters it.
- **Journal rows carry identity and prose, never transcribed measurements** — digests, paths, shas, exit codes, outcome words only.
- **assay's exit codes are readings; anything else is `Infra`** with code and stderr carried (the PR #14 discipline).
- **Conventional commits.** Attribution is disabled globally; add no co-author or "Generated with" trailer.

---

## File Structure

| File | Responsibility | Tasks |
| --- | --- | --- |
| `crates/bloomery-daemon/src/swap.rs` | `cover_argv`, `CoverGate`, `CoverOutcome`; `SwapJob`/`SwapSlot` state machine; the worker flow | 1, 2 |
| `crates/bloomery-daemon/tests/swap_test.rs` | Gate exit mapping; job flow with injected probe/gate; journal rows | 1, 2 |
| `crates/bloomery-core/src/journal.rs` | `Event::SwapCandidate` row | 2 |
| `crates/bloomery-daemon/src/api_native.rs` | The POST and GET arms, preconditions, response shapes | 3 |
| `crates/bloomery-daemon/tests/api_native_test.rs` (or the surface's existing test file) | Endpoint status table; busy lock; advisory pin | 3 |
| `docs/superpowers/specs/2026-08-19-swap-candidate-seam-design.md` | Dated amendment: strict instrument equality subsumes the straddle clause | 4 |
| `docs/CARRIED-DEBT.md`, `README.md` | The slice's entry; surface list gains the two routes | 4 |
| `docs/superpowers/evidence/2026-08-19-swap-candidate-live.md` | Live acceptance (HUMAN GATE) | 5 |

---

### Task 1: `CoverGate` — the subprocess wrapper and its four-code reading

Mirror `DriftGate` (`drift.rs:733-899`) exactly in structure: argv as an inspectable value, runner injection for tests, every exit a named outcome, anything undocumented = `Infra`.

**Files:**
- Create: `crates/bloomery-daemon/src/swap.rs` (declare `mod swap;` in `lib.rs` beside `mod drift;`)
- Test: `crates/bloomery-daemon/tests/swap_test.rs`

**Interfaces:**
- Consumes: `crate::post::run_bounded` (the bounded command runner `DriftGate::new` uses), the `CommandRunner` type alias (`drift.rs` — make it `pub(crate)` if it is not), `drift.rs`'s `with_stderr` helper (same visibility note).
- Produces: `swap::cover_argv(floor: &Path, candidate: &Path) -> Vec<String>`; `swap::CoverOutcome` (enum: `Covered`, `NotCovered`, `Refused { exit: i32 }`, `Incomplete`, `Infra { detail: String }`); `swap::CoverGate` with `new(python: String)`, test-only `with_runner(f: CommandRunner)`, and `check(&self, floor: &Path, candidate: &Path) -> CoverReading` where `CoverReading { outcome: CoverOutcome, exit_code: Option<i32> }`. Task 2 consumes `CoverGate` and `CoverOutcome`.

- [ ] **Step 1: Write the failing tests**

Create `crates/bloomery-daemon/tests/swap_test.rs`, borrowing `drift_test.rs`'s `exited(n)` / `output(...)` fixture helpers (copy them or share via the crate's `test-support` idiom — follow whichever the file review of `drift_test.rs:1100-1180` shows is available to a second integration test):

```rust
use bloomery_daemon::swap::{cover_argv, CoverGate, CoverOutcome};
use std::path::Path;

#[test]
fn cover_argv_is_the_documented_invocation() {
    let argv = cover_argv(Path::new("/d/floor.json"),
                          Path::new("/d/cand.json"));
    assert_eq!(argv, vec!["-m", "assay", "cover", "/d/floor.json",
                          "/d/cand.json"]);
}

#[test]
fn exit_zero_is_covered() {
    let (gate, _calls) = gate_answering(exited(0));
    let reading = gate.check(Path::new("f"), Path::new("c"));
    assert_eq!(reading.outcome, CoverOutcome::Covered);
    assert_eq!(reading.exit_code, Some(0));
}

#[test]
fn exit_one_is_not_covered() {
    let (gate, _calls) = gate_answering(exited(1));
    let reading = gate.check(Path::new("f"), Path::new("c"));
    assert_eq!(reading.outcome, CoverOutcome::NotCovered);
}

#[test]
fn exit_two_is_refused_and_never_a_pass() {
    let (gate, _calls) = gate_answering(exited(2));
    let reading = gate.check(Path::new("f"), Path::new("c"));
    assert_eq!(reading.outcome, CoverOutcome::Refused { exit: 2 });
}

#[test]
fn exit_three_is_incomplete_and_never_a_pass() {
    let (gate, _calls) = gate_answering(exited(3));
    let reading = gate.check(Path::new("f"), Path::new("c"));
    assert_eq!(reading.outcome, CoverOutcome::Incomplete);
}

#[test]
fn an_undocumented_exit_is_infrastructure_not_a_verdict() {
    let (gate, _calls) = gate_answering(exited(7));
    match gate.check(Path::new("f"), Path::new("c")).outcome {
        CoverOutcome::Infra { detail } => {
            assert!(detail.contains("undocumented exit 7"), "{detail}");
        }
        other => panic!("expected Infra, got {other:?}"),
    }
}

#[test]
fn a_signal_killed_cover_has_no_exit_code_and_is_infrastructure() {
    let (gate, _calls) = gate_answering(signalled());
    let reading = gate.check(Path::new("f"), Path::new("c"));
    assert_eq!(reading.exit_code, None);
    assert!(matches!(reading.outcome, CoverOutcome::Infra { .. }));
}
```

(`gate_answering` is the local fixture returning a `CoverGate::with_runner` plus a call recorder, modeled line-for-line on `drift_test.rs`'s same-named helper for `DriftGate`; `signalled()` builds the no-exit-code `ExitStatus` the drift tests already construct.)

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p bloomery-daemon --test swap_test`
Expected: FAIL — module `swap` does not exist.

- [ ] **Step 3: Implement `swap.rs`'s gate half**

```rust
//! The swap-candidate seam (spec: docs/superpowers/specs/
//! 2026-08-19-swap-candidate-seam-design.md §4): a coverage verdict on
//! a candidate model, evidenced by a daemon-run probe and
//! `assay cover`, consumed — like the drift gate — strictly through
//! documented exit codes. Advisory: nothing here blocks admission.

use std::path::Path;
use std::time::Duration;

/// `{python} -m assay cover {floor} {candidate}`
///
/// A value tests inspect rather than a side effect of spawning — the
/// same treatment `drift::diff_argv` and `post::argv` get. No flag:
/// cover IS a gate; exit codes are its whole interface.
pub fn cover_argv(floor: &Path, candidate: &Path) -> Vec<String> {
    [
        "-m",
        "assay",
        "cover",
        &floor.display().to_string(),
        &candidate.display().to_string(),
    ]
    .iter()
    .map(|s| (*s).to_string())
    .collect()
}

/// What one cover run said. assay documents exactly 0, 1, 2 and 3 for
/// `cover`; any other code is a tool this daemon does not understand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoverOutcome {
    /// Exit 0: every cell the floor measured, the candidate provides
    /// at least as well.
    Covered,
    /// Exit 1: at least one floor cell ranks below, beyond noise.
    NotCovered,
    /// Exit 2: cover refused the pair (hardware class or instrument
    /// mismatch). Never a pass.
    Refused { exit: i32 },
    /// Exit 3: a floor cell the candidate did not measure. Never a
    /// pass — the unmeasured cell may hide the regression the check
    /// exists to catch.
    Incomplete,
    /// The tool could not answer: spawn failure, signal, undocumented
    /// exit. Not a verdict in either direction.
    Infra { detail: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoverReading {
    pub outcome: CoverOutcome,
    pub exit_code: Option<i32>,
}
```

then `CoverGate` itself — copy `DriftGate`'s struct/impl shape (`drift.rs:733-899`) with: the same `CommandRunner` type, a `COVER_TIMEOUT` equal to drift's `DIFF_TIMEOUT`, `new(python)` spawning via `crate::post::run_bounded`, test-only `with_runner`, and a `check` whose exit-code match reads `0 => Covered`, `1 => NotCovered`, `2 => Refused { exit: 2 }`, `3 => Incomplete`, `Some(n) => Infra` ("undocumented exit {n} from `assay cover` (0, 1, 2 and 3 are the documented codes)", stderr appended via the same `with_stderr`), `None => Infra` (signal). Make `drift.rs`'s `CommandRunner` and `with_stderr` `pub(crate)` if they are private today — a visibility change only.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p bloomery-daemon --test swap_test`
Expected: PASS.

- [ ] **Step 5: Run both crates' suites, then commit**

Run: `cargo test -p bloomery-core -p bloomery-daemon`
Expected: 532 + new, 0 failed.

```bash
git add crates/bloomery-daemon/src/swap.rs crates/bloomery-daemon/src/lib.rs crates/bloomery-daemon/src/drift.rs crates/bloomery-daemon/tests/swap_test.rs
git commit -m "feat: CoverGate — assay cover consumed through its four documented exit codes"
```

---

### Task 2: The job — scratch probe, cover, journal, one slot

**Files:**
- Modify: `crates/bloomery-daemon/src/swap.rs`
- Modify: `crates/bloomery-core/src/journal.rs` (one new `Event` variant)
- Test: `crates/bloomery-daemon/tests/swap_test.rs`

**Interfaces:**
- Consumes: Task 1's `CoverGate`/`CoverOutcome`; `post::PostRunner::probe(port, model, tier, out)` (`post.rs:174`); `drift::ProfileStore::paths(model) -> ModelPaths` (`.baseline` field; `drift.rs:223-245`); the pager's model registration/unload — read `main.rs`'s configured-model registration block and `api_native.rs`'s `unload` handler first and reuse those exact calls (the GGUF metadata load included); `Pager::journal_degraded` and the journal append path the drift rows use (`drift/watch.rs`'s `journal_reading` is the model).
- Produces:
  - `Event::SwapCandidate { model: String, candidate_gguf_sha: String, floor_path: String, floor_sha: String, candidate_profile_path: String, candidate_profile_sha: Option<String>, exit_code: Option<i32>, outcome: String }` in `journal.rs`, serialized like every other row.
  - `swap::SwapOutcomeReport` (serializable summary: `outcome: String`, `exit_code: Option<i32>`, `candidate_gguf_sha: String`, `floor_sha: String`, `candidate_profile_path: String`, plus the two fixed advisory notes — see Step 3).
  - `swap::SwapSlot` — `Default`, holding `Mutex<SwapState>` where `SwapState` is `Idle | Running { model: String, gguf: PathBuf } | Done { model: String, report: SwapOutcomeReport }`; methods `try_start(model, gguf) -> Result<(), Busy>`, `finish(model, report)`, `snapshot() -> SwapState` (clone).
  - `swap::run_candidate_probe(...)` — the worker body, every collaborator passed in (pager mutex, runner, gate, store, port, tier, model, gguf path, slot), so tests drive it synchronously with injected fixtures; the HTTP layer (Task 3) is what puts it on a thread.
- Scratch identity: `format!("{model}!swap-candidate")` — `!` cannot collide with a configured model name (config keys are TOML bare keys/quoted names the operator writes; document the assumption where the constant lives). Registered before the probe, unloaded AND unregistered in every exit path, including probe failure — use a drop-guard or explicit cleanup at each `return`.

- [ ] **Step 1: Write the failing tests**

Add to `swap_test.rs`, using `drift_test.rs`'s boot/scripted fixtures as the model (`scripted_probes`, journal-row readers). The tests drive `run_candidate_probe` directly:

```rust
#[test]
fn a_covered_candidate_journals_the_verdict_with_digests() {
    // Arrange: a store seeded with a blessed baseline; a scripted
    // probe that writes a profile document; a gate answering 0.
    // Act: run_candidate_probe(...).
    // Assert: exactly one SwapCandidate row; outcome == "covered";
    // exit_code == Some(0); candidate_gguf_sha matches the sha256 of
    // the fixture GGUF bytes; floor_sha matches the baseline bytes;
    // the slot ends Done with the same report.
}

#[test]
fn a_probe_failure_journals_degraded_and_reports_no_verdict() {
    // Scripted probe answers Err; gate must never be spawned
    // (assert the gate's call recorder is empty); one Degraded row
    // naming the model and the probe's words; slot ends Done with
    // outcome == "infra: ..." and exit_code == None.
}

#[test]
fn the_scratch_registration_never_outlives_the_job() {
    // After run_candidate_probe returns — success AND probe-failure
    // paths — the pager's status() lists no model whose name contains
    // "!swap-candidate".
}

#[test]
fn the_slot_admits_one_job_at_a_time() {
    let slot = SwapSlot::default();
    assert!(slot.try_start("qwen", Path::new("/a.gguf")).is_ok());
    assert!(slot.try_start("qwen", Path::new("/b.gguf")).is_err());
}

#[test]
fn refused_incomplete_and_not_covered_all_journal_their_own_word() {
    // Table: gate exits 1, 2, 3 -> outcome words "not-covered",
    // "refused", "incomplete"; each a SwapCandidate row, none a
    // Degraded row — a verdict is not an infrastructure failure.
}
```

Write these as real tests against the fixtures you find in `drift_test.rs` — the sketch above names the assertions each must make; the arrange blocks reuse the existing helpers rather than inventing new ones.

- [ ] **Step 2: Run to verify they fail** (`cargo test -p bloomery-daemon --test swap_test`) — FAIL: missing types.

- [ ] **Step 3: Implement the job half of `swap.rs`**

The worker body, in order: read + sha256 the candidate GGUF bytes (the digest the journal row carries — the same full-file digest idiom KV images use); read + sha the floor (`store.paths(model).baseline`); register the scratch model (reusing `main.rs`'s registration calls verbatim); `runner.probe(port, &scratch_name, tier, &store.confirm_staging(&scratch_name))`; on success sha the written document, `gate.check(&floor_path, &candidate_profile_path)`; map the outcome to its word (`covered`, `not-covered`, `refused`, `incomplete`, `infra: {detail}`); journal one `Event::SwapCandidate` row (or `journal_degraded` for the probe-failure path, which has no comparison to record); unload + unregister the scratch model on every path; `slot.finish(...)` with the report. The report's two fixed notes (string constants beside the outcome mapping, returned in every report):

```rust
pub const NOTE_TASK_GATES: &str =
    "done_trust/G4/G5 are unmeasured for this candidate until its \
     first real boot with tasks enabled";
pub const NOTE_HANDOVER: &str =
    "on swap: edit config, restart; the next boot reads not-comparable \
     against the old lineage's baseline until you POST /models/{name}/bless";
```

- [ ] **Step 4: Run to verify they pass**, then the full suites.

- [ ] **Step 5: Commit**

```bash
git add crates/bloomery-daemon/src/swap.rs crates/bloomery-core/src/journal.rs crates/bloomery-daemon/tests/swap_test.rs
git commit -m "feat: the swap-candidate job — scratch probe, cover verdict, journaled with digests"
```

---

### Task 3: The HTTP arms

**Files:**
- Modify: `crates/bloomery-daemon/src/api_native.rs` (two arms in `dispatch` at `:55-65`, two handlers beside `bless` at `:194`)
- Modify: whatever wiring hands `dispatch` its state — the `SwapSlot`, `PostRunner`, `CoverGate`, `ProfileStore`, port and tier must reach the handler; follow how the existing surface reaches the pager (`&Mutex<Pager<S>>`) and extend that plumbing minimally (an `Arc<SwapContext>` beside the pager mutex).
- Test: the surface's existing endpoint test file (wherever `bless`'s table test lives — find it with `grep -rn 'no_current_profile' crates/bloomery-daemon/tests/`).

**Interfaces:**
- Consumes: Task 2's `SwapSlot`, `run_candidate_probe`, `SwapOutcomeReport`, notes constants.
- Produces the two routes:

| route | outcome | status | body |
|---|---|---|---|
| POST `/models/{name}/swap-candidate` | started | 202 | `{model, candidate, state: "running"}` |
| | unknown model | 404 | the surface's `unknown_model` shape |
| | malformed body / missing `gguf_path` / file unreadable | 400 | the surface's `bad_request` shape |
| | no blessed baseline | 409 | `{error: "no_baseline", model, detail}` |
| | job already running | 409 | `{error: "candidate_probe_in_progress", model}` |
| GET `/models/{name}/swap-candidate` | running | 200 | `{model, state: "running"}` |
| | done | 200 | `{model, state: "done", report: {outcome, exit_code, candidate_gguf_sha, floor_sha, candidate_profile_path, notes: [..]}}` |
| | never started | 404 | `{error: "no_swap_candidate", model}` |

- [ ] **Step 1: Write the failing endpoint tests** — the status table above, row by row, in the surface's existing test idiom (build the daemon state, call `dispatch`, assert status + body fields). The busy row uses a `SwapSlot` pre-set to `Running`. Include the advisory pin: after a `Done { outcome: "not-covered" }` slot state, the named model still admits an agent (`create_agent` succeeds) — the swap verdict must not have leaked into admission.
- [ ] **Step 2: Run to verify they fail.**
- [ ] **Step 3: Implement the two handlers.** POST: preconditions in the table's order (cheap checks before the slot claim; the slot claim before the thread spawn); on success spawn `std::thread::spawn` running `run_candidate_probe` with the context's collaborators — the handler returns 202 immediately and never joins the thread. GET: `slot.snapshot()` rendered per the table. Neither handler holds the pager lock across anything but its own precondition reads.
- [ ] **Step 4: Run the endpoint tests, then both crates' full suites.**
- [ ] **Step 5: Commit**

```bash
git add crates/bloomery-daemon/src/api_native.rs crates/bloomery-daemon/src/swap.rs crates/bloomery-daemon/tests/
git commit -m "feat: POST/GET /models/{name}/swap-candidate — the advisory swap verdict surface"
```

---

### Task 4: The record — spec amendment, debt, README

**Files:**
- Modify: `docs/superpowers/specs/2026-08-19-swap-candidate-seam-design.md`
- Modify: `docs/CARRIED-DEBT.md`, `README.md`

- [ ] **Step 1: Amend the seam spec (dated, not rewritten).** Under §3's instrument bullet add:

```markdown
> **Amendment (2026-08-19, assay v1.11 spec ruling):** instrument
> equality is STRICT — `probe_version` and schema exactly equal,
> absence fatal — which subsumes the straddle clause (equal versions
> cannot straddle a registered break). Strictness is the honest
> choice: v1.10's own record states the semantic-break registry is
> not a complete inventory, so a version-tolerant cover would trust
> an incomplete table. The registry check survives in `cover` as
> defense-in-depth should the gate ever loosen.
```

Also amend §4's response description to record the 202/GET job shape (a probe cannot ride a request handler — the boot watch's own rule), dated the same way.

- [ ] **Step 2: CARRIED-DEBT entries.** At minimum: the scratch-identity naming assumption (`!` never collides with configured names — an operator config that somehow names a model with `!` would collide, refused today by nothing); single-slot means no per-model concurrency (deliberate; revisit only with evidence); the advisory gap (an inadmissible candidate can still be swapped in by config edit — enforcement is the named future slice).
- [ ] **Step 3: README.** The two routes added to the surface list; one paragraph beside the drift watch's describing the swap verdict in the same register.
- [ ] **Step 4: Full suites green, commit.**

```bash
git add docs/superpowers/specs/2026-08-19-swap-candidate-seam-design.md docs/CARRIED-DEBT.md README.md
git commit -m "docs: the swap-candidate record — spec amendments, debt, surface"
```

---

### Task 5: Live acceptance — HUMAN GATE

**Files:**
- Create: `docs/superpowers/evidence/2026-08-19-swap-candidate-live.md`

**STOP before this task and confirm with Brice:** it loads two 14B models sequentially on the shared GPU (~15 min wall) and needs assay v1.11 merged to master first.

- [ ] **Step 1: Preflight.** assay master serves 0.13.0 (`PYTHONPATH=/home/brice/workspace/assay/src python3 -c "import assay; print(assay.__version__)"`); suites green both repos; featured daemon built LAST (`cargo test` first, then `cargo build -p bloomery-daemon --features vulkan`).
- [ ] **Step 2: Boot the standing config** (`~/.local/share/bloomery/drift/bloomery-drift.toml`, `PYTHONPATH` pin on the daemon process), wait for POST + drift rows to settle (within-noise expected — boot 3 of the standing lineage).
- [ ] **Step 3: The real question.** `POST /models/qwen3-14b-flywheel2/swap-candidate` with `{"gguf_path": "/home/brice/flywheel1/<the fw1 GGUF>"}` (exact filename from `ls ~/flywheel1/`). Poll GET until `done`. Record everything verbatim.
- [ ] **Step 4: Negative control.** A second POST while the first runs must 409 `candidate_probe_in_progress` (fire it early during the probe window); a POST for an unknown model must 404.
- [ ] **Step 5: Shutdown by verified PID; GPU back to desktop-only; journals + evidence doc** in the house evidence format (verdict table, rows verbatim, probe walls from provenance, GPU readings, what the answer MEANS for the fw1-vs-fw2 pair — and if the verdict surprises, record it rather than re-running until it doesn't).
- [ ] **Step 6: Commit the evidence.**

```bash
git add docs/superpowers/evidence/2026-08-19-swap-candidate-live.md
git commit -m "docs: swap-candidate live acceptance — fw1 candidate against the standing fw2 baseline"
```
