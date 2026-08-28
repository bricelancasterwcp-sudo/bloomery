# Refalsify-on-Exact Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a task-scoped verification probe to the memory organ: at retrieval, before injection, re-run the episode's stored `run_evidence.argv` under the incoming task's grant; pass injects, clean nonzero exit contradicts and silences, ungranted/demoted skips, timeout/spawn-failure is inconclusive — all config-gated off by default.

**Architecture:** All behavior lives at the worker's `organ_before_run` seam (`task/registry.rs`): after the exact-match retrieval and before rendering, a covered probe executes through the existing `exec_run` (same grant check, env, bounds, and process-group discipline as a task's own `run` verb), with the store lock NOT held across the execution. One additive field on `Event::MemoryStamp` ledgers the verdict; `MemoryConfig`/`MemoryContext` gain the `refalsify` flag.

**Tech Stack:** Rust workspace, real-subprocess integration tests mirroring `memory_task_test.rs`, JSONL journal/store with serde-default compat pins.

**Spec:** `docs/superpowers/specs/2026-08-27-refalsify-on-exact-design.md` — read it first; every rule below argues from it. The organ's own spec (`2026-08-26-memory-organ-design.md`) is the surrounding law.

## Global Constraints

- Flag-off identity: with `[memory] refalsify` absent or `false`, behavior and journal bytes are identical to today. Existing memory suites (`memory_task_test.rs`, `memory_render_test.rs`, `memory_capture_test.rs`, `memory_mint_test.rs`, `memory_retrieve_test.rs`, `memory_store_test.rs`, `api_memory_test.rs`) pass UNTOUCHED except where a struct literal needs the new field — never an expected-string change.
- The probe executes ONLY through `exec_run` with the incoming task's `Grant`, `cwd`, and `ExecBounds` — no second executor, no env deviation, no bespoke timeout knob.
- The store `Mutex` is NEVER held across the probe's execution (a probe can run `run_timeout_secs`): lock→retrieve→unlock, probe, then on failure lock→`mark_contradicted`→unlock.
- Verdict classification: only a genuine nonzero exit code contradicts. `exec_run`'s clean-exit Observation is `failed: false` with pinned outcome `"ran {program} exit {code}"`; code 0 → `passed`, code > 0 → `failed`, code -1 (the signal-death "no code" sentinel) → `inconclusive`. Every `failed: true` Observation (timeout `"ran {program} timed out"`, spawn/wait failures `"run failed: ..."`) → `inconclusive`. The grant refusal arm is unreachable behind the pre-check — `grant.check_command(argv)` runs first and a non-Ok skips before anything spawns.
- Demotion outranks refalsification: `spec.mutating_verbs == false` → skip (`skipped_ungranted`), even with a covering grant.
- The retrieval MATCH is untouched: no change to `retrieve()`, the fingerprint gate, the single-injection cap, or the rendered block. A probe never renders into any prompt and never journals a `TaskStep`.
- `Event::MemoryStamp` gains `refalsify: Option<String>` with `#[serde(default)]` — absent-key replay as `None` (the house additive pattern). No new counters, no record-schema change.
- Claim discipline: no number from this arc's tests appears in a capability sentence, anywhere.
- Run verification with full output redirected to a file and check the exit code; NEVER pipe through tail/head without capturing the exit.
- Commit per green step, conventional commits, no attribution footers. Featured binary (`cargo build -p bloomery-daemon --features vulkan`) rebuilt only in the FINAL task, after the last test run. Do NOT push.

---

### Task 1: The flag — config, context, and organ plumbing (behavior-inert)

**Files:**
- Modify: `crates/bloomery-daemon/src/config.rs` (`MemoryConfig`, ~line 441)
- Modify: `crates/bloomery-daemon/src/memory.rs` (`MemoryContext`, ~line 56)
- Modify: `crates/bloomery-daemon/src/task/registry.rs` (the organ tuple threaded into `organ_before_run`)
- Modify: every `MemoryContext { .. }` / organ-tuple construction site the compiler names
- Test: `crates/bloomery-daemon/tests/config_test.rs` (extend, mirroring the existing `[memory]` parse tests)

**Interfaces:**
- Consumes: nothing new.
- Produces: `MemoryConfig { .., pub refalsify: bool }` (serde default false, `Default` impl updated); `MemoryContext { .., pub refalsify: bool }`; the organ parameter to `organ_before_run` becomes `Option<(&Mutex<MemoryStore>, usize, bool)>` (store, max_episodes, refalsify) with the bool UNREAD in this task (`_` in the destructure, replaced by Task 3). Every construction site passes the config/context value where one exists, literal `false` in tests.

- [ ] **Step 1: Write the failing config tests**

In `config_test.rs`, next to the existing `[memory]` tests (read them first and mirror their exact parse-fixture style):

```rust
#[test]
fn memory_refalsify_defaults_false_and_parses_true() {
    // Absent → false (spec §5: an enabled organ keeps battery-measured
    // behavior until the operator opts in).
    let absent: MemoryConfig = toml::from_str("enabled = true").unwrap();
    assert!(!absent.refalsify);
    assert!(!MemoryConfig::default().refalsify);
    let on: MemoryConfig = toml::from_str("enabled = true\nrefalsify = true").unwrap();
    assert!(on.refalsify);
}
```

Adapt the deserialization entry point to however the existing `[memory]` tests parse (whole-`Config` fixture vs direct `MemoryConfig` — copy their pattern; the assertions above are the contract).

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p bloomery-daemon --test config_test 2>&1 | tee /tmp/r1.log; echo exit=$?`
Expected: FAIL to compile — no field `refalsify`.

- [ ] **Step 3: Add the field and thread it**

`MemoryConfig` gains (after `max_episodes`, matching its doc style):

```rust
    /// Refalsify-on-exact (spec
    /// `docs/superpowers/specs/2026-08-27-refalsify-on-exact-design.md` §5):
    /// `true` makes the worker probe a retrieved episode's stored run
    /// command under the incoming task's grant before injecting. Default
    /// `false` — an enabled organ behaves exactly as the memory battery's
    /// GATE PASS measured (inject-without-refalsify) until the operator
    /// opts in. Read only when `enabled` is true.
    #[serde(default)]
    pub refalsify: bool,
```

Update the `Default` impl (`refalsify: false`). `MemoryContext` gains `pub refalsify: bool` with a doc pointing at the same spec §5. Then `cargo check --workspace` and fix every site the compiler names: the daemon's `MemoryContext` construction from config gets `refalsify: cfg.memory.refalsify` (find the real field path), test constructions get `false`. Extend the organ tuple to `(store, max_episodes, refalsify)` at its build site(s) in `registry.rs` and destructure with `_` for the bool inside `organ_before_run` (Task 3 reads it).

- [ ] **Step 4: Run the workspace**

Run: `cargo test --workspace 2>&1 > /tmp/r1full.log; echo exit=$?; grep -E "test result|FAILED" /tmp/r1full.log | head`
Expected: all green, zero ignored — the flag exists and changes nothing.

- [ ] **Step 5: Commit**

```bash
git add -A crates
git commit -m "feat: [memory] refalsify flag — config, context, organ plumbing, behavior-inert (refalsify spec §5)"
```

---

### Task 2: The stamp ledger — `refalsify` on `Event::MemoryStamp` (still behavior-inert)

**Files:**
- Modify: `crates/bloomery-core/src/journal.rs` (`Event::MemoryStamp`, ~line 371)
- Modify: `crates/bloomery-daemon/src/task/registry.rs` (`OrganDecision` + the stamp emit site)
- Test: the journal test file that pins `MemoryStamp` serialization (find via `grep -rn "MemoryStamp" crates/bloomery-core/tests crates/bloomery-daemon/tests` and extend the right one)

**Interfaces:**
- Consumes: nothing from Task 1.
- Produces: `Event::MemoryStamp { .., refalsify: Option<String> }` (`#[serde(default)]`); `OrganDecision { .., refalsify: Option<&'static str> }` with `None` in `OrganDecision::off()` and every current constructor — Task 3 sets real values. The stamp emit site maps `decision.refalsify.map(String::from)` into the event.

- [ ] **Step 1: Write the failing compat tests**

In the located journal test file, mirroring the `rung`/`expect` compat-pin style:

```rust
#[test]
fn a_pre_refalsify_memory_stamp_replays_with_refalsify_none() {
    // A stamp journaled before the field existed carries no "refalsify"
    // key; absent must replay as None — an un-probed stamp, which is what
    // every such row was (refalsify spec §4).
    let raw = /* copy a real serialized MemoryStamp row from an existing
                 test or serialize one pre-change, WITHOUT the field */;
    let ev: Event = serde_json::from_str(raw).unwrap();
    match ev {
        Event::MemoryStamp { refalsify, .. } => assert_eq!(refalsify, None),
        other => panic!("expected MemoryStamp, got {other:?}"),
    }
}

#[test]
fn a_memory_stamp_round_trips_its_refalsify_verdict() {
    let ev = Event::MemoryStamp {
        id: "a1".into(),
        task_id: "t1".into(),
        mode: "injected".into(),
        episode_id: Some("e1".into()),
        candidates_checked: 1,
        refalsify: Some("failed".into()),
    };
    let json = serde_json::to_string(&ev).unwrap();
    assert!(json.contains(r#""refalsify":"failed""#), "{json}");
    match serde_json::from_str::<Event>(&json).unwrap() {
        Event::MemoryStamp { refalsify, .. } => {
            assert_eq!(refalsify.as_deref(), Some("failed"))
        }
        other => panic!("expected MemoryStamp, got {other:?}"),
    }
}
```

Capture the `raw` literal by serializing a current `MemoryStamp` BEFORE adding the field (or copy from an existing test's bytes) — the pre-change wire format is the thing under pin.

- [ ] **Step 2: Run to verify failure** — compile error, no field `refalsify`.

- [ ] **Step 3: Implement**

`Event::MemoryStamp` gains (after `candidates_checked`):

```rust
        /// Refalsify-on-exact's verdict for this retrieval (refalsify spec
        /// §4): `None` when the flag is off, memory is off, or nothing was
        /// retrieved; `Some("passed" | "failed" | "skipped_ungranted" |
        /// "inconclusive")` when a hit was probed or skipped. Additive —
        /// absent-key rows replay as `None`, which is the truth of every
        /// pre-refalsify stamp. A `"failed"` stamp is always accompanied by
        /// an ordinary `Event::MemoryContradicted` citing the same task.
        #[serde(default)]
        refalsify: Option<String>,
```

`OrganDecision` gains `refalsify: Option<&'static str>` (doc: set only by the Task-3 probe; `None` = un-probed). Set `refalsify: None` in `OrganDecision::off()` and both in-function constructors; the stamp emit site passes `refalsify: decision.refalsify.map(String::from)`. `cargo check --workspace` and fix any other `MemoryStamp { .. }` literal (tests) with `refalsify: None`.

- [ ] **Step 4: Workspace green** — `cargo test --workspace 2>&1 > /tmp/r2full.log; echo exit=$?` all green, zero ignored.

- [ ] **Step 5: Commit**

```bash
git add -A crates
git commit -m "feat: refalsify verdict field on MemoryStamp, absent-key compat pinned (refalsify spec §4)"
```

---

### Task 3: The probe — pre-check, execution, four verdicts

**Files:**
- Modify: `crates/bloomery-daemon/src/task/registry.rs` (`organ_before_run` + its caller closure, which must now also pass the agent id for the contradiction row)
- Test: `crates/bloomery-daemon/tests/memory_refalsify_test.rs` (NEW — mirror `memory_task_test.rs`'s fixture: real pager + FakeSubstrate scripted turns, real store file, real journal, real tempdir sandbox; read that file FIRST and reuse its helper shapes)

**Interfaces:**
- Consumes: Task 1's organ-tuple bool; Task 2's `OrganDecision.refalsify`.
- Produces: the probe inside `organ_before_run`; a private `fn classify_probe(obs: &Observation) -> &'static str` in `registry.rs`; `organ_before_run` gains the agent id (so the fail path can journal `Event::MemoryContradicted { id, task_id, episode_id }`).

- [ ] **Step 1: Write the failing tests**

Create `memory_refalsify_test.rs`. Fixture: copy `memory_task_test.rs`'s pattern — a first task that reads a file, lands a patch, runs a granted command, and ends `done` (this MINTS the episode through the real mint path — never hand-build a record the system can produce), then a second, byte-identical task (same goal, same workspace bytes) whose retrieval is the thing under test. The grant for both tasks includes the command prefixes used below. Memory context: `enabled: true`, `refalsify` per test. The daemon-side scripted replies for task 2 need only a `done` turn (probe verdicts are visible in the stamp/store/prompt without further steps).

The seven binding tests (assertions are the contract; adapt fixture plumbing to `memory_task_test.rs`'s real helpers):

```rust
#[test]
fn flag_off_injects_without_probing_and_stamps_none() {
    // refalsify=false; episode's run command is ["false"] — a probe WOULD
    // fail, so injection here proves no probe ran (spec §5 flag-off
    // identity, stronger than byte-diffing).
    // mint with run ["false"]? NO — the mint bar requires exit 0. Mint
    // with ["true"], then the flag-off case simply asserts: injected,
    // stamp mode "injected", stamp refalsify == None, store still
    // verified, and the second task's prompt CONTAINS the memory block.
}

#[test]
fn a_passing_probe_injects_and_stamps_passed() {
    // refalsify=true; episode minted via run ["true"]. Second task:
    // injected (prompt contains the block), stamp refalsify ==
    // Some("passed"), store status still "verified", no
    // MemoryContradicted row in the journal.
}

#[test]
fn a_failing_probe_contradicts_silences_and_stamps_failed() {
    // refalsify=true; mint via a command that exits 0 at mint time but 1
    // at retrieval time: `["sh", "-c", "exit $(cat flag.txt)"]` with
    // flag.txt containing "0" for task 1, rewritten to "1" before task 2
    // — flag.txt is NOT a cited file (the model never reads it), so the
    // fingerprint gate still matches while the verification genuinely
    // fails. Assert: second task's prompt does NOT contain the block
    // (byte-identical to memory-silent), stamp mode ... refalsify ==
    // Some("failed"), store row status == "contradicted" with
    // contradicted_by == task 2's id, journal has MemoryContradicted
    // citing task 2, AND a third identical task retrieves silence
    // (contradicted is never injected again).
}

#[test]
fn an_ungranted_command_skips_and_injects() {
    // refalsify=true; mint with ["true"] under a grant that includes it;
    // the SECOND task's grant omits the ["true"] prefix (grants come from
    // the request, not the store). Assert: injected, stamp refalsify ==
    // Some("skipped_ungranted"), store untouched, and no run was
    // attempted (journal contains no trace of an execution between the
    // task-2 spawn and its first step; assert via the pager/task journal
    // rows if a cleaner seam is absent — state in a comment what was
    // checked).
}

#[test]
fn a_demoted_task_skips_even_with_a_covering_grant() {
    // refalsify=true; second task identical but mutating_verbs == false
    // (however memory_task_test's fixture spells demotion — if its spec
    // helper hardcodes true, add the field to the fixture). Assert:
    // injected, stamp Some("skipped_ungranted").
}

#[test]
fn a_timed_out_probe_is_inconclusive_and_injects() {
    // refalsify=true; mint via ["sh", "-c", "test -f slow || exit 0"]? —
    // simplest honest shape: mint with ["sh", "-c", "sleep $(cat d.txt)"]
    // where d.txt holds "0" at mint (instant exit 0) and "10" before task
    // 2, with the task's ExecBounds run_timeout_secs = 1 (d.txt uncited,
    // same trick as the fail test). Assert: injected, stamp
    // Some("inconclusive"), store still "verified", no
    // MemoryContradicted row.
}

#[test]
fn classify_probe_calls_signal_death_inconclusive() {
    // Unit-level: the -1 "no exit code" sentinel must never contradict
    // (spec §2.3: only a genuine nonzero exit). Exercise via a probe
    // command killed by signal: ["sh", "-c", "kill -9 $$"] exits via
    // SIGKILL → exec_run reports "ran sh exit -1" with failed: false.
    // Full-fixture shape like the tests above; assert injected + stamp
    // Some("inconclusive") + store still verified.
}
```

Every test asserts through public observables: the substrate-received prompt (`pager.substrate().ctx_history(..)` — block present/absent), the journal (`replay` → `MemoryStamp.refalsify`, `MemoryContradicted`), and the store file (re-open and check status), exactly as `memory_task_test.rs` already does. If `sh` is unavailable under `exec_run`'s cleared env (`PATH` = its pinned `RUN_PATH`), use `/bin/sh` absolute or whatever the existing run-exec tests use — mirror `task_exec_run_test.rs`.

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p bloomery-daemon --test memory_refalsify_test 2>&1 | tee /tmp/r3.log; echo exit=$?`
Expected: the flag-off test may pass (it pins existing behavior); every probe test FAILS — stamps are `None` and the fail test still injects (no probe exists).

- [ ] **Step 3: Implement the probe**

In `registry.rs`, the classifier:

```rust
/// The probe's verdict from `exec_run`'s Observation (refalsify spec §2.3).
/// Only a genuine nonzero exit contradicts: a clean exit is `failed: false`
/// with the pinned outcome `"ran {program} exit {code}"` (code -1 is the
/// signal-death "no code" sentinel — not a real exit, so inconclusive);
/// every `failed: true` arm — timeout, spawn failure, wait failure — is
/// environmental, not semantic, and injects rather than accuses. Grant
/// refusals never reach here: `check_command` runs BEFORE anything spawns,
/// because a refusal Observation must never be mistakable for evidence.
fn classify_probe(obs: &crate::task::Observation) -> &'static str {
    if obs.failed {
        return "inconclusive";
    }
    // `failed: false` is exec_run's clean-exit arm by construction; parse
    // the code from its pinned outcome format (rfind: argv[0] could
    // itself contain " exit ").
    match obs.outcome.rfind(" exit ").and_then(|i| obs.outcome[i + 6..].parse::<i64>().ok()) {
        Some(0) => "passed",
        Some(code) if code > 0 => "failed",
        _ => "inconclusive",
    }
}
```

In `organ_before_run` (signature gains the agent id; the organ tuple's bool is now read), after the oversize check decides the episode WOULD inject and before building the injected decision:

```rust
    // Refalsify-on-exact (refalsify spec §2): probe the episode's own
    // stored verification under THIS task's granted capability before
    // trusting it. The store lock is NOT held here — retrieval released it
    // above, and a probe can legitimately run for run_timeout_secs.
    let refalsify_verdict = if refalsify_flag {
        // Coverage pre-check + demotion, BEFORE any execution attempt: a
        // grant-refusal Observation is shaped like a failed run, and a
        // refusal must never read as evidence. Demotion outranks the
        // grant: a task that may not `run` has no commands executed at
        // its moment, whatever its grant says (spec §2.1).
        if !spec.mutating_verbs
            || spec.grant.check_command(&episode.run_evidence.argv).is_err()
        {
            Some("skipped_ungranted")
        } else {
            let obs = crate::task::exec_run(
                &spec.grant,
                &spec.cwd,
                &episode.run_evidence.argv,
                &spec.bounds,
            );
            Some(classify_probe(&obs))
        }
    } else {
        None
    };

    if refalsify_verdict == Some("failed") {
        // Same accusation mechanism as passive contradiction (spec §2.3):
        // mark the store, journal the ordinary MemoryContradicted row
        // citing THIS task, and give the task silence — byte-identical to
        // a stranger's prompt. The injection decision stands even if
        // recording fails (spec §7): the task must not receive guidance
        // the probe just refuted.
        {
            let mut store = lock_store(store, journal);
            let _ = store.mark_contradicted(&episode.episode_id, task_id);
        }
        let _ = journal.append(&Event::MemoryContradicted {
            id: agent_id.to_string(),
            task_id: task_id.to_string(),
            episode_id: episode.episode_id.clone(),
        });
        return OrganDecision {
            mode: "silent",
            candidates_checked: retrieval.candidates_checked,
            injected_id: None,
            block: None,
            refalsify: Some("failed"),
        };
    }
```

and the surviving injected decision carries `refalsify: refalsify_verdict`. ADAPT to the function's real local names and error-handling idioms — in particular, match how the existing code treats `lock_store` poisoning and journal append failures at this seam (the organ's failure is never the task's failure; follow the surrounding arms rather than the `let _ =` sketches above if the neighbors do it differently — but preserve the spec §7 rule that silence stands regardless). The silent and off decisions keep `refalsify: None`; a silent-because-oversize decision also keeps `None` (nothing was probed — the probe runs only on an episode that would otherwise inject; if the probe passed and THEN the oversize rule silences, the stamp keeps the oversize behavior with `refalsify: Some("passed")` — order: probe after the oversize check, so a too-big block skips the probe entirely; pick that order, it is the cheaper one, and document it in the code).

Thread the agent id from the caller closure (the worker has it; add a parameter).

- [ ] **Step 4: Run the new tests, iterate**

Run: `cargo test -p bloomery-daemon --test memory_refalsify_test 2>&1 | tee /tmp/r3b.log; echo exit=$?`
Expected: 7/7. Iterate on fixture plumbing (env, sh path, uncited-file trick) — never on the verdict rules.

- [ ] **Step 5: Workspace green**

Run: `cargo test --workspace 2>&1 > /tmp/r3full.log; echo exit=$?; grep -E "test result|FAILED" /tmp/r3full.log | head`
Expected: all green, zero ignored — flag-off suites untouched.

- [ ] **Step 6: Commit**

```bash
git add -A crates
git commit -m "feat: refalsify-on-exact — task-scoped verification probe before injection (refalsify spec §2-§3)"
```

---

### Task 4: Mutation spot-checks, spec cross-check, acceptance, featured binary

**Files:**
- No production edits expected. A surviving mutant's fix is a new/extended test.

**Interfaces:**
- Consumes: everything above.
- Produces: the acceptance evidence for the SDD ledger.

- [ ] **Step 1: Mutation spot-checks (spec §8 test 9)**

Discipline per mutant: single hand edit → `cargo test -p bloomery-daemon --test memory_refalsify_test 2>&1 | tee /tmp/m.log; echo exit=$?` → ≥1 named test FAILS → revert exactly → `touch` the file → re-run green. Record edit/command/failing-test/revert per mutant; every "because" in the record must be a quoted trace, never inference (the window-ladder arc's lesson).

- Mutant A — skip-vs-fail boundary: invert the pre-check (`.is_err()` → `.is_ok()`). Expected kills: `an_ungranted_command_skips_and_injects` (probe of an ungranted argv now attempted/refused → wrong stamp) and/or the pass test (granted argv now skipped).
- Mutant B — flag gate: replace `refalsify_flag` with `true`. Expected kill: `flag_off_injects_without_probing_and_stamps_none` (stamp becomes Some).
- Mutant C — contradiction citation: cite a literal `"wrong"` instead of `task_id` in `mark_contradicted`. Expected kill: `a_failing_probe_...`'s `contradicted_by` assert.
- Mutant D — verdict boundary: `Some(code) if code > 0` → `code >= 0`. Expected kill: the pass test (exit 0 now contradicts).

- [ ] **Step 2: Amendment pointer in the organ spec**

`docs/superpowers/specs/2026-08-26-memory-organ-design.md` §5 still reads
as permanently passive-only. Specs are historical records — do not rewrite
§5's text; append one errata-style line directly under the §5 heading:

```markdown
> **Amended 2026-08-27:** active refalsification exists now, as a
> task-scoped probe under the incoming task's grant — see
> `2026-08-27-refalsify-on-exact-design.md`. Daemon-spontaneous execution
> stays banned; everything else in this section stands.
```

Commit it with the mutation-evidence commit below.

- [ ] **Step 3: Spec cross-check**

Walk refalsify spec §2-§8 clause by clause against the branch (the four outcomes, lock discipline, demotion rule, stamp values, flag default, §8's nine tests) and record the walk in the report — any clause without an implementing line or pinning test is a finding to report, not to quietly patch.

- [ ] **Step 4: Full workspace acceptance**

Run: `cargo test --workspace 2>&1 > /tmp/r4full.log; echo exit=$?; grep -E "test result" /tmp/r4full.log`
Expected: all green, ZERO ignored.

- [ ] **Step 5: Featured binary (box rule — LAST)**

Run: `cargo build -p bloomery-daemon --features vulkan 2>&1 > /tmp/r4build.log; echo exit=$?`
Expected: exit 0.

- [ ] **Step 6: Commit anything outstanding; leave merge/push for Brice**

```bash
git status --short
git add -A crates docs && git commit -m "test: refalsify mutation-kill evidence" || true
git log --oneline -6
```
