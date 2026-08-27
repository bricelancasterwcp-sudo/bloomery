# Window Ladder Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Port robigo's fixed degradation ladder into `run_task` as opt-in client behavior on `PromptTooLarge`: re-render one rung smaller and re-submit, refusing only when rung 4 still doesn't fit, with the sent rung ledgered on every step row.

**Architecture:** All behavior lives in `task_loop.rs`'s `propose_action`, which walks a fixed 4-rung ladder (full → drop memory block → elide all but last 2 transcript entries to headers → all but last 1) by re-submitting to `pager.infer` — the pager stays the ONLY measurer; the loop never estimates tokens. A new `TaskSpec::window_ladder` bool (default false everywhere) gates it; a new `rung` field on `Event::TaskStep`/`TaskStepRecord` ledgers it.

**Tech Stack:** Rust workspace, `FakeSubstrate` scripted-reply tests, JSONL journal with serde-default compat pins.

**Spec:** `docs/superpowers/specs/2026-08-27-window-ladder-design.md` — read it first; every rule below argues from it.

## Global Constraints

- Byte-identity law: rung-1 rendering and every ladder-off behavior must be bit-identical to today. Existing goldens (`task_render_test.rs`, `memory_render_test.rs`, the four flywheel anti-drift tests) must pass UNTOUCHED — never edit them to make them pass.
- No client-side token estimation anywhere — the pager's accept/refuse is the only measurement (spec §4).
- `Event::TaskStep::rung` serde default is a NAMED fn returning 1 (`default_rung_one`), never bare `#[serde(default)]` (which would replay old rows as nonexistent rung 0).
- Every `TaskSpec` construction site gets `window_ladder: false` except `api_task.rs`'s `create_task`, which wires the request field. `TaskSpec` has no `Default`, so `cargo check --workspace` names every site — use the compiler as the checklist.
- The ladder reacts to `PagerError::PromptTooLarge` ONLY. `Budget` stays terminal `BudgetExhausted`; everything else stays `Error` (g4 Amendment 1 carve-out, unchanged).
- Run verification as `cargo test --workspace 2>&1 > /tmp/claude-ladder-test.log; echo "exit=$?"` style — full output to a file, check the exit code. NEVER pipe through `tail`/`head` without capturing the exit (this box has masked failures twice that way).
- The featured daemon binary is clobbered by `cargo test`. Rebuild it (`cargo build -p bloomery-daemon --features vulkan`) only in the FINAL task, after the last test run.
- Commit after every green step; conventional-commit messages; no attribution footers (disabled globally on this box).
- Useful constants already in the codebase: `STEP_MAX_TOKENS = 1024` (task_loop.rs), `CHARS_PER_TOKEN = 3` (pager.rs — `needed_tokens = prompt.len()/3 + max_tokens`). Tests size windows from these, computed from the test's own expected strings, never hardcoded magic numbers.

---

### Task 1: The rung ledger — `rung` on journal rows and step records

**Files:**
- Modify: `crates/bloomery-core/src/journal.rs` (the `Event::TaskStep` variant, ~line 84)
- Modify: `crates/bloomery-daemon/src/task/task_loop.rs` (`TaskStepRecord`, `StepReport`, every `StepReport { .. }` literal)
- Test: `crates/bloomery-core/tests/journal_test.rs` (or wherever `grep -rn "default_expect_patch" crates/bloomery-core/tests` lands — the existing compat-pin file; add alongside)

**Interfaces:**
- Consumes: nothing new.
- Produces: `Event::TaskStep { .., rung: u32 }` with `fn default_rung_one() -> u32 { 1 }`; `TaskStepRecord { .., pub rung: u32 }`; `StepReport { .., rung: u32 }`. Task 3 sets real rung values; this task passes literal `1` everywhere, which is the truth today.

- [ ] **Step 1: Write the failing compat-pin test**

In the journal's existing test file (same file that pins `CodecFixture`'s `expect` default — find it with `grep -rln "default_expect_patch\|expect.*patch" crates/bloomery-core/tests`), add:

```rust
#[test]
fn a_pre_ladder_task_step_row_replays_as_rung_1() {
    // A TaskStep row journaled before the rung field existed carries no
    // "rung" key at all. The absent-key default must be 1 — what every
    // such row WAS — so old journals replay byte-identically (the same
    // compat pin CodecFixture's `expect` field carries).
    let raw = r#"{"event":"TaskStep","id":"a1","step":3,"verb":"read","outcome":"ok","duration_ms":5,"args":["src/lib.rs"]}"#;
    let ev: Event = serde_json::from_str(raw).expect("pre-ladder row parses");
    match ev {
        Event::TaskStep { rung, step, .. } => {
            assert_eq!(rung, 1, "absent rung must default to 1, never 0");
            assert_eq!(step, 3);
        }
        other => panic!("expected TaskStep, got {other:?}"),
    }
}

#[test]
fn a_task_step_row_round_trips_its_rung() {
    let ev = Event::TaskStep {
        id: "a1".to_string(),
        step: 7,
        verb: "read".to_string(),
        outcome: "ok".to_string(),
        duration_ms: 5,
        args: vec![],
        rung: 3,
    };
    let json = serde_json::to_string(&ev).unwrap();
    assert!(json.contains(r#""rung":3"#), "rung serializes: {json}");
    match serde_json::from_str::<Event>(&json).unwrap() {
        Event::TaskStep { rung, .. } => assert_eq!(rung, 3),
        other => panic!("expected TaskStep, got {other:?}"),
    }
}
```

Adapt the `raw` literal's exact key set to the variant's real serde shape — copy an existing `TaskStep` serialization from a neighboring test or serialize one first. If the enum uses a different tag layout (e.g. externally tagged `{"TaskStep":{...}}`), match whatever the existing compat-pin test in that file uses.

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p bloomery-core a_pre_ladder_task_step_row 2>&1 | tee /tmp/t1.log; echo exit=$?`
Expected: FAIL to compile — `Event::TaskStep` has no field `rung`.

- [ ] **Step 3: Add the field**

In `crates/bloomery-core/src/journal.rs`, on `Event::TaskStep` after `args`:

```rust
        /// The window-ladder rung (1-4) this step's prompt was ACTUALLY
        /// sent at (`docs/superpowers/specs/2026-08-27-window-ladder-design.md`
        /// §6). Named default, not bare `#[serde(default)]`: an absent key
        /// must replay as 1 — what every pre-ladder row was — never as a
        /// nonexistent rung 0 (the `default_expect_patch` compat pattern).
        #[serde(default = "default_rung_one")]
        rung: u32,
```

Next to `default_expect_patch` (~line 398):

```rust
fn default_rung_one() -> u32 {
    1
}
```

- [ ] **Step 4: Thread it through the daemon's records**

In `crates/bloomery-daemon/src/task/task_loop.rs`:

`TaskStepRecord` gains (after `args`):

```rust
    /// The ladder rung this step's prompt was sent at (spec §6). Always 1
    /// for a ladder-off task. Serialized, so `get_task`'s `"steps"` array
    /// exposes it without any api_task change.
    pub rung: u32,
```

`StepReport` gains `rung: u32`. `record_step` passes it into both the `Event::TaskStep` append (`rung: report.rung`) and the `TaskStepRecord` push (`rung: report.rung`). Every existing `StepReport { .. }` literal (`propose_action`'s parse-failure report, the `done` report, the demoted-verb report, the executed-action report) gets `rung: 1` — Task 3 replaces these with the real value.

Then `cargo check --workspace 2>&1 | tee /tmp/t1c.log; echo exit=$?` and fix every site the compiler names (test files constructing `TaskStepRecord` or `Event::TaskStep` literals get `rung: 1`).

- [ ] **Step 5: Run the workspace suite**

Run: `cargo test --workspace 2>&1 > /tmp/t1full.log; echo exit=$?; grep -E "test result|FAILED" /tmp/t1full.log | head -20`
Expected: all green, zero ignored. Both new tests pass.

- [ ] **Step 6: Commit**

```bash
git add -A crates
git commit -m "feat: rung field on TaskStep journal rows and step records (window-ladder spec §6)"
```

---

### Task 2: `TaskSpec::window_ladder` + HTTP wiring + the ladder-off identity pin

**Files:**
- Modify: `crates/bloomery-daemon/src/task/task_loop.rs` (`TaskSpec`)
- Modify: `crates/bloomery-daemon/src/api_task.rs` (`CreateTaskReq`, the `TaskSpec { .. }` literal ~line 204)
- Modify: every other `TaskSpec { .. }` site the compiler names (`codec_probe/mod.rs` ~363, `codec_probe/refuse.rs` ~231, `task/registry.rs` test ~904, flywheel_tool bin, all test files)
- Test: `crates/bloomery-daemon/tests/task_ladder_test.rs` (NEW — this file grows through Tasks 2-4)

**Interfaces:**
- Consumes: Task 1's `StepReport.rung` (test fixtures may construct records).
- Produces: `TaskSpec { .., pub window_ladder: bool }`; `CreateTaskReq { .., window_ladder: bool }` (serde default false). Task 3 reads `spec.window_ladder` in `propose_action`.

- [ ] **Step 1: Create the test file with its fixtures and the failing identity test**

Create `crates/bloomery-daemon/tests/task_ladder_test.rs`. The fixtures mirror `task_loop_test.rs`'s (same pattern, plus a `window_cap` parameter — `Pager::create_agent(model, priority, window_cap, budget)`'s third argument caps the agent's window, which is how every ladder test makes prompts refuse):

```rust
//! The window ladder (docs/superpowers/specs/2026-08-27-window-ladder-design.md):
//! opt-in client-side scope degradation on PromptTooLarge. Mirrors
//! `task_loop_test.rs`'s FakeSubstrate fixture pattern; windows are sized
//! at runtime from the test's own expected strings via the pager's
//! `needed_tokens = prompt.len()/3 + max_tokens` arithmetic (CHARS_PER_TOKEN
//! = 3, STEP_MAX_TOKENS = 1024), never hardcoded.

use bloomery_core::action::PatchCodec;
use bloomery_core::gguf::GgufMeta;
use bloomery_core::grant::Grant;
use bloomery_core::journal::{replay, Event, Journal};
use bloomery_daemon::agents::ImageStore;
use bloomery_daemon::config::EnvelopeLens;
use bloomery_daemon::pager::Pager;
use bloomery_daemon::task::task_loop::render_task_prompt;
use bloomery_daemon::task::{run_task, ExecBounds, TaskSpec, TaskStatus};
use bloomery_substrate::fake::FakeSubstrate;
use bloomery_substrate::Reply;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

/// Mirrors `task_loop.rs`'s STEP_MAX_TOKENS and pager.rs's CHARS_PER_TOKEN —
/// restated as literals (not imported) so a golden can't agree with a
/// mutation of them (task_render_test.rs's rule).
const MAX_TOKENS: u64 = 1024;
const CHARS_PER_TOKEN: u64 = 3;

/// The window cap that admits `prompt` exactly: needed = len/3 + 1024 fits.
fn cap_fitting(prompt: &str) -> u32 {
    u32::try_from(prompt.len() as u64 / CHARS_PER_TOKEN + MAX_TOKENS).unwrap()
}

fn bounds() -> ExecBounds {
    ExecBounds {
        read_cap_bytes: 256 * 1024,
        find_result_cap: 100,
        run_output_cap_bytes: 64 * 1024,
        run_timeout_secs: 120,
    }
}

fn meta() -> GgufMeta {
    GgufMeta {
        arch: "qwen2".into(),
        layers: 4,
        attention_layers: 4,
        kv_heads: 2,
        head_dim: 32,
        training_ctx: 65536,
        weights_bytes: 1000,
        recurrent_state_bytes: 0,
    }
}

fn scripted(text: &str) -> Reply {
    Reply {
        text: text.to_string(),
        prompt_tokens: Some(8),
        completion_tokens: Some(4),
        duration_ms: 1,
    }
}

fn fresh_dir(tag: &str) -> PathBuf {
    static UNIQUE: AtomicU64 = AtomicU64::new(0);
    let unique = UNIQUE.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "bloomery-ladder-{tag}-{}-{unique}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A read-only sandbox grant over `dir` itself (ladder tests only read).
fn sandbox_grant(dir: &std::path::Path) -> Grant {
    let d = std::fs::canonicalize(dir).unwrap();
    Grant::from_json(&format!(
        r#"{{"read_roots":["{s}"],"write_roots":["{s}"],"commands":[]}}"#,
        s = d.display()
    ))
    .unwrap()
}

/// `task_loop_test::fixture` plus a `window_cap` — the ladder's lever.
fn fixture(
    dir: &std::path::Path,
    window_cap: Option<u32>,
    replies: Vec<Reply>,
) -> (Pager<FakeSubstrate>, String) {
    let journal = Journal::open(&dir.join("pager.jsonl")).unwrap();
    let images = ImageStore::new(&dir.join("img")).unwrap();
    let mut fake = FakeSubstrate::new();
    for r in replies {
        fake.script_reply(r);
    }
    let mut pager = Pager::new(fake, journal, images, Box::new(|| Some(1024 * 1024 * 1024)));
    let gguf = dir.join("m.gguf");
    std::fs::write(&gguf, b"fake weights").unwrap();
    pager.register_model("m", &gguf, meta(), None).unwrap();
    let info = pager.create_agent("m", 100, window_cap, 1_000_000).unwrap();
    (pager, info.id)
}

const GOAL: &str = "exercise the window ladder";

fn ladder_spec(grant: Grant, cwd: PathBuf, window_ladder: bool) -> TaskSpec {
    TaskSpec {
        goal: GOAL.to_string(),
        grant,
        budget_tokens: 1_000_000,
        max_steps: 8,
        cwd,
        patch_codec: PatchCodec::SearchReplace,
        bounds: bounds(),
        mutating_verbs: true,
        envelope: EnvelopeLens::V1,
        memory_block: None,
        window_ladder,
    }
}

/// The pager's journaled `Refusal` rows for this run, in order — each
/// intermediate rung-up leaves one (spec §6: "already journaled by the
/// pager's own refusal event").
fn refusals(dir: &std::path::Path) -> Vec<(u64, u32)> {
    replay(&dir.join("pager.jsonl"))
        .unwrap()
        .into_iter()
        .filter_map(|e| match e {
            Event::Refusal {
                needed_tokens,
                window_tokens,
                ..
            } => Some((needed_tokens, window_tokens)),
            _ => None,
        })
        .collect()
}

/// The count of infer calls that actually reached the substrate — refused
/// rungs never do (the pager's window check is pre-inference).
fn infer_count(pager: &Pager<FakeSubstrate>) -> usize {
    pager
        .substrate()
        .calls()
        .iter()
        .filter(|c| c.starts_with("infer"))
        .count()
}

#[test]
fn ladder_off_dies_window_exhausted_on_the_first_refusal() {
    // Spec §8 test 1: window_ladder=false keeps today's behavior exactly —
    // one refusal, zero substrate infers, terminal WindowExhausted with the
    // pager's arithmetic in the summary.
    let dir = fresh_dir("off-identity");
    let big_memory = "memory ".repeat(2000); // ~14k chars — never fits below
    let rung2 = render_task_prompt(GOAL, PatchCodec::SearchReplace, EnvelopeLens::V1, &[], "");
    // Window admits the memory-less prompt but not the memory-bearing one.
    let (mut pager, agent_id) = fixture(&dir, Some(cap_fitting(&rung2)), vec![]);
    let mut journal = Journal::open(&dir.join("task.jsonl")).unwrap();
    let spec = TaskSpec {
        memory_block: Some(big_memory),
        ..ladder_spec(sandbox_grant(&dir), std::fs::canonicalize(&dir).unwrap(), false)
    };
    let result = run_task(&mut pager, &agent_id, &spec, &mut journal);
    assert_eq!(result.status, TaskStatus::WindowExhausted);
    assert_eq!(refusals(&dir).len(), 1, "exactly one refusal — no ladder walk");
    assert_eq!(infer_count(&pager), 0, "a refused prompt never reaches the substrate");
    let summary = result.summary.expect("summary carries the pager arithmetic");
    assert!(summary.contains("window"), "arithmetic summary, got: {summary}");
}
```

Note: check `Event::Refusal`'s real field names in `crates/bloomery-core/src/journal.rs` (`id`, `needed_tokens`, `window_tokens`, `detail` — see `pager/journal.rs::refusal`) and adapt the `filter_map` arm if they differ. Same for `replay`'s exact signature — `task_loop_test.rs` imports and uses it; copy that usage.

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p bloomery-daemon --test task_ladder_test 2>&1 | tee /tmp/t2.log; echo exit=$?`
Expected: FAIL to compile — `TaskSpec` has no field `window_ladder`.

- [ ] **Step 3: Add the field and wire every site**

In `task_loop.rs`, `TaskSpec` gains (after `memory_block`):

```rust
    /// The window ladder (spec
    /// `docs/superpowers/specs/2026-08-27-window-ladder-design.md`): `true`
    /// opts this task into fixed scope degradation on `PromptTooLarge` —
    /// `propose_action` re-renders one rung smaller and re-submits, refusing
    /// only when rung 4 still doesn't fit. `false` — the default at EVERY
    /// construction site except `api_task`'s request wiring — is today's
    /// behavior byte-for-byte: the first `PromptTooLarge` is terminal.
    /// Every frozen instrument (codec probe, flywheel factory, batteries)
    /// passes `false` permanently; their measured verdicts were taken under
    /// die-on-413 and stay comparable only if that never moves.
    pub window_ladder: bool,
```

In `api_task.rs`, `CreateTaskReq` gains:

```rust
    /// Spec §5: opt-in over HTTP, absent → false. A bare bool with
    /// `#[serde(default)]` (false), not an Option — there is no
    /// absent-vs-explicit-false distinction to preserve.
    #[serde(default)]
    window_ladder: bool,
```

and the `TaskSpec { .. }` literal gains `window_ladder: req.window_ladder,`.

Then `cargo check --workspace 2>&1 | tee /tmp/t2c.log; echo exit=$?` and add `window_ladder: false,` at every site the compiler names — `codec_probe/mod.rs`, `codec_probe/refuse.rs`, the flywheel_tool bin, `task/registry.rs`'s test, and every test file (`task_loop_test.rs`, `memory_render_test.rs`, `memory_task_test.rs`, the four `flywheel_tool_*_test.rs`, etc.). Struct-update-syntax sites (`..demoted_spec(...)`) need no edit.

- [ ] **Step 4: Run the new test and the workspace**

Run: `cargo test -p bloomery-daemon --test task_ladder_test 2>&1 | tee /tmp/t2b.log; echo exit=$?`
Expected: PASS (the identity test exercises existing behavior — it pins it before Task 3 changes the neighborhood).

Run: `cargo test --workspace 2>&1 > /tmp/t2full.log; echo exit=$?; grep -E "test result|FAILED" /tmp/t2full.log | head -20`
Expected: all green — proving the field addition changed no behavior anywhere.

- [ ] **Step 5: Commit**

```bash
git add -A crates
git commit -m "feat: TaskSpec::window_ladder opt-in flag, default false everywhere (window-ladder spec §5)"
```

---

### Task 3: The ladder walk — rung rendering, head note, re-submit on refusal

**Files:**
- Modify: `crates/bloomery-daemon/src/task/task_loop.rs` (`render_prompt` → `render_prompt_at_rung`, new `degraded_transcript`/`elided_entry`/`head_note` helpers, `propose_action`'s walk, `ProposeOutcome::Action` carries the rung, `run_task` threads it into every `StepReport`)
- Test: `crates/bloomery-daemon/tests/task_ladder_test.rs` (extend)

**Interfaces:**
- Consumes: Task 1's `StepReport.rung`/`TaskStepRecord.rung`; Task 2's `spec.window_ladder`.
- Produces: `const MAX_RUNG: u32 = 4`; `fn render_prompt_at_rung(spec: &TaskSpec, steps: &[TaskStepRecord], transcript: &str, rung: u32) -> String` (private; replaces `render_prompt`); `ProposeOutcome::Action(Action, u64, u32)` (action, duration_ms, sent rung). `render_prompt_from` and `render_task_prompt` are UNTOUCHED.

- [ ] **Step 1: Write the failing tests**

Append to `task_ladder_test.rs`:

```rust
/// `transcript_entry`'s pinned shape, restated (not imported) per the
/// golden rule: `"\n[step {step} {verb}] {outcome}\n{content}\n"`.
fn full_entry(step: u32, verb: &str, outcome: &str, content: &str) -> String {
    format!("\n[step {step} {verb}] {outcome}\n{content}\n")
}

/// Spec §2 rung 3/4: the elided form is the full form minus the content line.
fn elided(step: u32, verb: &str, outcome: &str) -> String {
    format!("\n[step {step} {verb}] {outcome}\n")
}

/// Spec §3: the pinned head note, `{a}-{b}` always, one trailing newline.
fn note(a: u32, b: u32) -> String {
    format!("[context note: contents of steps {a}-{b} elided to fit the window; outcomes retained — re-read files if needed]\n")
}

#[test]
fn ladder_on_lands_rung_2_by_dropping_the_memory_block() {
    // Spec §8 test 7 + §2 rung 2: the rung-2 bytes ARE the memory-off
    // rendering — which is exactly what the public serving-faithful wrapper
    // (permanently memory-None) renders. Full-prompt byte equality against
    // that independent comparator.
    let dir = fresh_dir("rung2");
    let big_memory = "memory ".repeat(2000);
    let rung2_expected =
        render_task_prompt(GOAL, PatchCodec::SearchReplace, EnvelopeLens::V1, &[], "");
    let (mut pager, agent_id) = fixture(
        &dir,
        Some(cap_fitting(&rung2_expected)),
        vec![scripted("<action verb=\"done\">\nok\n</action>")],
    );
    let mut journal = Journal::open(&dir.join("task.jsonl")).unwrap();
    let spec = TaskSpec {
        memory_block: Some(big_memory.clone()),
        ..ladder_spec(sandbox_grant(&dir), std::fs::canonicalize(&dir).unwrap(), true)
    };
    let result = run_task(&mut pager, &agent_id, &spec, &mut journal);
    assert_eq!(result.status, TaskStatus::Done);
    let history = pager.substrate().ctx_history(1).expect("agent ctx exists");
    assert_eq!(history, rung2_expected, "rung-2 bytes == memory-off bytes, exactly");
    assert!(!history.contains(&big_memory), "the memory block is gone");
    assert_eq!(refusals(&dir).len(), 1, "rung 1 refused once, rung 2 accepted");
    assert_eq!(result.steps.len(), 1);
    assert_eq!(result.steps[0].rung, 2, "the ledger records the SENT rung");
}

#[test]
fn every_attempt_rewalks_from_rung_1() {
    // Spec §4: no ratchet. Two steps, both forced past rung 1 by the same
    // big memory block — TWO refusals proves step 2 tried rung 1 again
    // (a ratchet would leave exactly one).
    let dir = fresh_dir("rewalk");
    let big_memory = "memory ".repeat(2000);
    // Step 2's rung-2 prompt is step 1's plus the done... no — use read
    // then done so step 2's transcript has one small entry. Size the cap
    // for the LARGER rung-2 prompt (step 2's), so both steps' rung-2 fit.
    std::fs::write(dir.join("f.txt"), "alpha\n").unwrap();
    let step1_reply = "<action verb=\"read\" path=\"f.txt\">\n</action>";
    let step2_transcript = full_entry(1, "read", "read 6 bytes", "alpha\n");
    let rung2_step2 =
        render_task_prompt(GOAL, PatchCodec::SearchReplace, EnvelopeLens::V1, &[], &step2_transcript);
    let (mut pager, agent_id) = fixture(
        &dir,
        Some(cap_fitting(&rung2_step2)),
        vec![
            scripted(step1_reply),
            scripted("<action verb=\"done\">\nok\n</action>"),
        ],
    );
    let mut journal = Journal::open(&dir.join("task.jsonl")).unwrap();
    let spec = TaskSpec {
        memory_block: Some(big_memory),
        ..ladder_spec(sandbox_grant(&dir), std::fs::canonicalize(&dir).unwrap(), true)
    };
    let result = run_task(&mut pager, &agent_id, &spec, &mut journal);
    assert_eq!(result.status, TaskStatus::Done);
    assert_eq!(
        refusals(&dir).len(),
        2,
        "one rung-1 refusal PER STEP — step 2 re-walked from rung 1"
    );
    let rungs: Vec<u32> = result.steps.iter().map(|s| s.rung).collect();
    assert_eq!(rungs, vec![2, 2]);
}

#[test]
fn ladder_lands_rung_3_eliding_old_entries_behind_the_head_note() {
    // Spec §8 tests 2+6, §2 rung 3, §3: three entries (big, small, small);
    // the window fits last-2-full + entry-1's header + the note, but not
    // all three full. The degraded tail is pinned byte-for-byte.
    let dir = fresh_dir("rung3");
    let big = "x".repeat(2400);
    std::fs::write(dir.join("big.txt"), &big).unwrap();
    std::fs::write(dir.join("s.txt"), "small\n").unwrap();
    let read_big = "<action verb=\"read\" path=\"big.txt\">\n</action>";
    let read_small = "<action verb=\"read\" path=\"s.txt\">\n</action>";
    // Entry contents mirror exec_read's real outcome/content shapes — run
    // once and adjust these to the actual observed outcome strings (they
    // are asserted below via ends_with, so a mismatch fails loudly).
    let e1 = full_entry(1, "read", "read 2400 bytes", &big);
    let e2 = full_entry(2, "read", "read 6 bytes", "small\n");
    let e3 = full_entry(3, "read", "read 6 bytes", "small\n");
    let rung1_step4 = render_task_prompt(
        GOAL,
        PatchCodec::SearchReplace,
        EnvelopeLens::V1,
        &[],
        &format!("{e1}{e2}{e3}"),
    );
    let rung3_tail = format!("{}{}{e2}{e3}", note(1, 1), elided(1, "read", "read 2400 bytes"));
    let rung3_step4 = render_task_prompt(
        GOAL,
        PatchCodec::SearchReplace,
        EnvelopeLens::V1,
        &[],
        &rung3_tail,
    );
    // The cap is sized to STEP 3's rung-1 prompt (entries 1-2 full, the
    // big one included) — the largest prompt that must still fit at rung 1
    // so steps 1-3 stay undegraded. Step 4's rung-1 adds e3 (~25 tokens)
    // on top and refuses; its rung-3 rendering (big e1 elided) sits far
    // below the cap and fits.
    let rung1_step3 = render_task_prompt(
        GOAL,
        PatchCodec::SearchReplace,
        EnvelopeLens::V1,
        &[],
        &format!("{e1}{e2}"),
    );
    let cap = cap_fitting(&rung1_step3);
    assert!(cap < cap_fitting(&rung1_step4), "sizing sanity: step 4's rung 1 must refuse");
    assert!(
        cap >= cap_fitting(&rung3_step4),
        "sizing sanity: step 4's rung 3 must fit"
    );
    let (mut pager, agent_id) = fixture(
        &dir,
        Some(cap),
        vec![
            scripted(read_big),
            scripted(read_small),
            scripted(read_small),
            scripted("<action verb=\"done\">\nok\n</action>"),
        ],
    );
    let mut journal = Journal::open(&dir.join("task.jsonl")).unwrap();
    let spec = ladder_spec(sandbox_grant(&dir), std::fs::canonicalize(&dir).unwrap(), true);
    let result = run_task(&mut pager, &agent_id, &spec, &mut journal);
    assert_eq!(result.status, TaskStatus::Done);
    let history = pager.substrate().ctx_history(1).expect("agent ctx exists");
    assert!(
        history.ends_with(&rung3_step4),
        "step 4's prompt is the pinned rung-3 rendering (note + elided e1 + full e2,e3)"
    );
    let rungs: Vec<u32> = result.steps.iter().map(|s| s.rung).collect();
    assert_eq!(rungs, vec![1, 1, 1, 3]);
    // Fixed ladder: step 4 refused rungs 1 AND 2 (identical bytes — no
    // memory block — so identical needed_tokens: the no-skip pin, §2).
    let r = refusals(&dir);
    assert_eq!(r.len(), 2);
    assert_eq!(r[0].0, r[1].0, "rung 2 == rung 1 bytes when memory is absent");
}

#[test]
fn ladder_lands_rung_4_when_two_full_entries_are_too_many() {
    // Spec §2 rung 4: two big entries; the window fits one full entry plus
    // the other's header, not two full. Kills a MAX_RUNG 4->3 mutant.
    let dir = fresh_dir("rung4");
    let big = "y".repeat(2400);
    std::fs::write(dir.join("b.txt"), &big).unwrap();
    let read_big = "<action verb=\"read\" path=\"b.txt\">\n</action>";
    let e1 = full_entry(1, "read", "read 2400 bytes", &big);
    let e2 = full_entry(2, "read", "read 2400 bytes", &big);
    let rung4_tail = format!("{}{}{e2}", note(1, 1), elided(1, "read", "read 2400 bytes"));
    let rung4_step3 = render_task_prompt(
        GOAL,
        PatchCodec::SearchReplace,
        EnvelopeLens::V1,
        &[],
        &rung4_tail,
    );
    let rung1_step2 =
        render_task_prompt(GOAL, PatchCodec::SearchReplace, EnvelopeLens::V1, &[], &e1);
    // One big entry (step 2's rung-1) must fit; two must not. rung-4's
    // prompt (~ one big + header + note) is the larger of the two "fits"
    // candidates, so cap on it admits both.
    let cap = cap_fitting(&rung4_step3).max(cap_fitting(&rung1_step2));
    let (mut pager, agent_id) = fixture(
        &dir,
        Some(cap),
        vec![
            scripted(read_big),
            scripted(read_big),
            scripted("<action verb=\"done\">\nok\n</action>"),
        ],
    );
    let mut journal = Journal::open(&dir.join("task.jsonl")).unwrap();
    let spec = ladder_spec(sandbox_grant(&dir), std::fs::canonicalize(&dir).unwrap(), true);
    let result = run_task(&mut pager, &agent_id, &spec, &mut journal);
    assert_eq!(result.status, TaskStatus::Done);
    let history = pager.substrate().ctx_history(1).expect("agent ctx exists");
    assert!(history.ends_with(&rung4_step3), "step 3 sent the pinned rung-4 bytes");
    let rungs: Vec<u32> = result.steps.iter().map(|s| s.rung).collect();
    assert_eq!(rungs, vec![1, 1, 4]);
    // Step 3 refused rungs 1, 2 (== 1: no memory), and 3 (two full entries
    // with only two entries total elides nothing — == rung 2, §2's
    // "renders identical ... refuses through it naturally").
    let r = refusals(&dir);
    assert_eq!(r.len(), 3);
    assert_eq!(r[0].0, r[1].0, "rung 2 bytes == rung 1 (no memory)");
    assert_eq!(r[1].0, r[2].0, "rung 3 with 2 entries elides nothing == rung 2");
}

#[test]
fn rung_4_refusal_is_terminal_window_exhausted() {
    // Spec §2 refusal + §8 test 5: even rung 4 refuses -> WindowExhausted
    // with the pager arithmetic, after exactly 4 refusals (1,2,3,4).
    let dir = fresh_dir("terminal");
    let big_memory = "memory ".repeat(2000);
    // A cap below even the bare memory-less empty-transcript prompt: the
    // smallest thing rung 4 could render still refuses. STEP_MAX_TOKENS
    // alone exceeds... keep cap just above max_tokens so the refusal is on
    // prompt size, not degenerate.
    let (mut pager, agent_id) = fixture(&dir, Some(1030), vec![]);
    let mut journal = Journal::open(&dir.join("task.jsonl")).unwrap();
    let spec = TaskSpec {
        memory_block: Some(big_memory),
        ..ladder_spec(sandbox_grant(&dir), std::fs::canonicalize(&dir).unwrap(), true)
    };
    let result = run_task(&mut pager, &agent_id, &spec, &mut journal);
    assert_eq!(result.status, TaskStatus::WindowExhausted);
    assert_eq!(refusals(&dir).len(), 4, "all four rungs tried, in order");
    assert_eq!(infer_count(&pager), 0);
    assert!(result.summary.expect("arithmetic summary").contains("window"));
    assert!(result.steps.is_empty(), "no step row for a turn that never sent");
}
```

- [ ] **Step 2: Run to verify the new tests fail**

Run: `cargo test -p bloomery-daemon --test task_ladder_test 2>&1 | tee /tmp/t3.log; echo exit=$?`
Expected: `ladder_off_dies...` still PASSES; every new test FAILS — `ladder_on_lands_rung_2...` gets `WindowExhausted` instead of `Done` (no walk exists yet).

- [ ] **Step 3: Implement the walk in `task_loop.rs`**

Add near `MAX_PARSE_ATTEMPTS`:

```rust
/// The fixed ladder's smallest rung (spec §2). Four rungs then refusal —
/// robigo's shape. A rung outside 1..=MAX_RUNG reaching the renderer is a
/// programming error and panics (spec §7: no silent clamping, either
/// direction, ever).
const MAX_RUNG: u32 = 4;
```

Add the rendering helpers (below `render_prompt_from`):

```rust
/// Spec §2 rung 3/4: an elided entry is `transcript_entry`'s pinned shape
/// minus the content line — the record of what was done survives, the
/// re-obtainable content goes.
fn elided_entry(step: u32, verb: &str, outcome: &str) -> String {
    format!("\n[step {step} {verb}] {outcome}\n")
}

/// Spec §3: the pinned head note. Always the `{a}-{b}` form, even when
/// `a == b` — fixed format, no branching. One trailing newline; the first
/// entry's own leading newline supplies the blank line after it.
fn head_note(first_step: u32, last_step: u32) -> String {
    format!(
        "[context note: contents of steps {first_step}-{last_step} elided to fit the window; outcomes retained — re-read files if needed]\n"
    )
}

/// The rung-3/4 transcript: every entry except the last `full_window`
/// rendered elided, behind the head note — which renders ONLY when at
/// least one entry was actually elided (spec §3: absence adds nothing).
/// Rebuilt from `steps` rather than sliced out of the accumulated string;
/// `record_step` appends both from the same values, so the full entries
/// here are byte-identical to their accumulated originals by construction.
fn degraded_transcript(steps: &[TaskStepRecord], full_window: usize) -> String {
    let elide_end = steps.len().saturating_sub(full_window);
    let mut out = String::new();
    if elide_end > 0 {
        out.push_str(&head_note(steps[0].step, steps[elide_end - 1].step));
    }
    for (i, s) in steps.iter().enumerate() {
        if i < elide_end {
            out.push_str(&elided_entry(s.step, &s.verb, &s.outcome));
        } else {
            out.push_str(&transcript_entry(s.step, &s.verb, &s.outcome, &s.content));
        }
    }
    out
}
```

Replace `render_prompt` with `render_prompt_at_rung`, MOVING its docstring and updating the final paragraph per spec §7 (keep the rest of the doc verbatim):

```rust
/// [former render_prompt docstring, verbatim, except the last paragraph
/// which becomes:]
///
/// Deliberately does no SILENT windowing or truncation. The pager's own
/// `infer` is what refuses — with arithmetic — a prompt too large for the
/// agent's measured window (its "refuse, never truncate" rule stands
/// untouched). What `rung` adds (window-ladder spec,
/// `docs/superpowers/specs/2026-08-27-window-ladder-design.md`) is the
/// CLIENT's honest response to that refusal: an explicit, fixed,
/// journaled re-scope — rung 1 is today's bytes exactly, rung 2 drops the
/// memory block, rungs 3/4 elide old entries to headers behind a pinned
/// head note. Silent truncation is still forbidden; this is neither
/// silent (the note, the `rung` field on every step row) nor heuristic
/// (the ladder is fixed, spec §2).
fn render_prompt_at_rung(
    spec: &TaskSpec,
    steps: &[TaskStepRecord],
    transcript: &str,
    rung: u32,
) -> String {
    assert!(
        (1..=MAX_RUNG).contains(&rung),
        "rung {rung} outside the fixed ladder 1..={MAX_RUNG} (spec §7: no silent clamping)"
    );
    let memory_block = if rung == 1 {
        spec.memory_block.as_deref()
    } else {
        None
    };
    let degraded;
    let transcript = match rung {
        1 | 2 => transcript,
        3 => {
            degraded = degraded_transcript(steps, 2);
            degraded.as_str()
        }
        _ => {
            degraded = degraded_transcript(steps, 1);
            degraded.as_str()
        }
    };
    render_prompt_from(
        &spec.goal,
        RenderInputs {
            patch_codec: spec.patch_codec,
            mutating_verbs: spec.mutating_verbs,
            envelope: spec.envelope,
            commands: spec.grant.commands(),
            memory_block,
        },
        transcript,
    )
}
```

Change `ProposeOutcome::Action(Action, u64)` to `Action(Action, u64, u32)` (doc: "A validated action, how long the successful `infer` took, and the ladder rung the prompt was sent at — 1 for every ladder-off task"). Rewrite `propose_action`'s loop body:

```rust
    for attempt in 1..=MAX_PARSE_ATTEMPTS {
        // Spec §4: every attempt walks the fixed ladder from rung 1 — a
        // step-down-only ratchet can never step back up (robigo's
        // `_select_rung` lesson), and a re-walk costs at most three refused
        // pre-inference arithmetic checks. The pager is the ONLY measurer:
        // nothing here estimates tokens; a rung is rendered, submitted,
        // and the pager's accept/refuse IS the measurement. That covers
        // both refusal paths — the pre-inference window gate and a
        // substrate-side error classified `PromptTooLarge` after
        // submission — identically: a window refusal is a window refusal.
        let mut rung: u32 = 1;
        let (reply, duration_ms, sent_rung) = loop {
            let prompt = render_prompt_at_rung(spec, &state.steps, &state.transcript, rung);
            let started = Instant::now();
            match pager.infer(agent_id, &prompt, STEP_MAX_TOKENS, stop) {
                Ok(reply) => {
                    let d = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
                    break (reply, d, rung);
                }
                Err(PagerError::PromptTooLarge { .. })
                    if spec.window_ladder && rung < MAX_RUNG =>
                {
                    rung += 1;
                }
                Err(e) => {
                    // Protocol Amendment 1 (docs/superpowers/evidence/
                    // 2026-08-15-g4-protocol.md §9), unchanged: ONLY
                    // `PromptTooLarge` becomes the scored `WindowExhausted`
                    // terminal (now reached at rung MAX_RUNG for a
                    // ladder-on task, at rung 1 otherwise); `Budget` stays
                    // `BudgetExhausted` at every rung; everything else
                    // stays `Error`.
                    let status = match &e {
                        PagerError::Budget { .. } => TaskStatus::BudgetExhausted,
                        PagerError::PromptTooLarge { .. } => TaskStatus::WindowExhausted,
                        _ => TaskStatus::Error,
                    };
                    return ProposeOutcome::Terminate(status, Some(e.to_string()));
                }
            }
        };

        match parse_action_with_codec(&reply.text, spec.patch_codec) {
            Ok(action) => return ProposeOutcome::Action(action, duration_ms, sent_rung),
            Err(e) => {
                // [existing re-ask body unchanged, except the StepReport
                //  literal gains:]  rung: sent_rung,
                ...
            }
        }
    }
```

In `run_task`, destructure the third element — `ProposeOutcome::Action(action, duration_ms, rung) => (action, duration_ms, rung)` — and set `rung` (instead of the Task 1 literal `1`) on the `done`, demoted-verb, and executed-action `StepReport` literals.

- [ ] **Step 4: Run the ladder tests, iterate on sizing**

Run: `cargo test -p bloomery-daemon --test task_ladder_test 2>&1 | tee /tmp/t3b.log; echo exit=$?`
Expected: all PASS. If a rung-3/4 test fails on `ends_with`, print the tail of `history` and fix the test's `e1`/`e2`/`e3` outcome strings to `exec_read`'s real outcome shape (e.g. the exact "read N bytes" spelling) — the IMPLEMENTATION's formats are pinned by spec; the test's fixture literals adapt to the executor's real outcome text, never the other way around.

- [ ] **Step 5: Run the workspace — the byte-identity gate**

Run: `cargo test --workspace 2>&1 > /tmp/t3full.log; echo exit=$?; grep -E "test result|FAILED" /tmp/t3full.log | head -20`
Expected: all green, in particular `task_render_test`, `memory_render_test`, and all four flywheel anti-drift tests — UNTOUCHED and green, proving rung-1 bytes never moved.

- [ ] **Step 6: Commit**

```bash
git add -A crates
git commit -m "feat: the window ladder — fixed rung walk on PromptTooLarge in propose_action (spec §2-§4)"
```

---

### Task 4: HTTP surface — request wiring proof and rung exposure via `get_task`

**Files:**
- Test: `crates/bloomery-daemon/tests/api_task_test.rs` (extend — mirror its existing fixture/server pattern exactly)

**Interfaces:**
- Consumes: Task 2's `CreateTaskReq.window_ladder`, Task 1's `TaskStepRecord.rung` (already serialized into `get_task`'s `"steps"` array — `get_task` returns `result.steps` whole, so no api_task code change exists in this task; it is test-only).
- Produces: nothing new — pins.

- [ ] **Step 1: Write the tests**

Read `api_task_test.rs`'s existing create-then-poll test first and copy its exact fixture, request, and poll helpers (it drives the real HTTP dispatch against a `FakeSubstrate` pager). Then add two tests:

```rust
#[test]
fn task_steps_expose_their_rung_and_default_to_1() {
    // Spec §6: get_task's "steps" objects carry "rung"; a normal (ladder
    // absent from the request => off) task's steps are all rung 1.
    // [Copy the existing happy-path create-task test body: create agent,
    //  POST /agents/{id}/task with a goal+grants body that scripts one
    //  done reply, poll GET until status != "running".]
    // Then assert on the final JSON:
    let steps = body["steps"].as_array().expect("steps array");
    assert!(!steps.is_empty());
    for s in steps {
        assert_eq!(s["rung"], 1, "ladder-off steps are rung 1: {s}");
    }
}

#[test]
fn create_task_accepts_window_ladder_true() {
    // Spec §5: the field parses and the task still completes. (The
    // degradation behavior itself is pinned by task_ladder_test.rs — this
    // pins the WIRE: a request carrying the field is not a 400.)
    // [Same body as above, with "window_ladder": true added to the POST
    //  JSON. Assert the POST returns 202 and the task reaches "done".]
}
```

These are deliberately wire-level pins, not behavior duplicates: `task_ladder_test.rs` owns behavior; this file owns "the request field exists and the response field exists."

- [ ] **Step 2: Run to verify**

Run: `cargo test -p bloomery-daemon --test api_task_test 2>&1 | tee /tmp/t4.log; echo exit=$?`
Expected: both PASS immediately (the wiring landed in Tasks 1-2 — these are regression pins; RED was played at Task 2 Step 2 when the field didn't compile). If `"rung"` is absent from the JSON, `TaskStepRecord`'s field isn't serializing — fix THAT (it must be a plain `pub rung: u32` with no skip attribute), not the test.

- [ ] **Step 3: Commit**

```bash
git add crates/bloomery-daemon/tests/api_task_test.rs
git commit -m "test: pin window_ladder request field and rung exposure on the task wire (spec §5-§6)"
```

---

### Task 5: Mutation spot-checks, full acceptance, featured binary

**Files:**
- No production edits expected — this task VERIFIES. (If a mutant survives, the fix is a new test in `task_ladder_test.rs`, then re-run.)

**Interfaces:**
- Consumes: everything above.
- Produces: the acceptance evidence for the SDD ledger.

- [ ] **Step 1: Mutation spot-check the two spec §8-test-9 boundaries**

For each mutant: apply by hand with Edit, run ONLY the ladder test file, confirm at least one test FAILS (the kill), revert the edit, and `touch` the reverted file (NEVER `cp -p`-style timestamp restore — cargo's fingerprint would run the stale mutant; this box has been burned).

Mutant A — elision boundary: in `render_prompt_at_rung`, change rung 3's `degraded_transcript(steps, 2)` to `degraded_transcript(steps, 1)`.
Run: `cargo test -p bloomery-daemon --test task_ladder_test 2>&1 | tee /tmp/m1.log; echo exit=$?`
Expected kill: `ladder_lands_rung_3_...` fails (e2 would render elided).

Mutant B — ladder length: change `const MAX_RUNG: u32 = 4` to `3`.
Run: same command.
Expected kill: `ladder_lands_rung_4_...` fails (terminal at rung 3 → WindowExhausted, no Done).

Mutant C — head-note range: in `degraded_transcript`, swap `steps[0].step` and `steps[elide_end - 1].step`.
Expected kill: with the current fixtures both rung-3/4 tests elide exactly one entry (`1-1`), which a swap cannot distinguish — so FIRST extend `ladder_lands_rung_4_...` to three big entries (add one `read_big` script and an `e3`; rung-4 tail becomes `note(1, 2)` + two elided + one full; note that step 3 itself now degrades too — two big entries no longer fit rung 1 — so the expected rung sequence becomes `[1, 1, 4, 4]` and the refusal count grows; re-green it), THEN apply the swap and confirm the kill (`2-1` renders). Revert, `touch`.

Record each kill (mutant, command, failing test name) in the task's ledger notes.

- [ ] **Step 2: Full workspace acceptance**

Run: `cargo test --workspace 2>&1 > /tmp/t5full.log; echo exit=$?; grep -E "test result" /tmp/t5full.log`
Expected: all green, ZERO ignored, zero failures — and eyeball that the totals line up with ~825+ tests plus this arc's additions.

- [ ] **Step 3: Rebuild the featured binary (box rule — LAST, after the final test run)**

Run: `cargo build -p bloomery-daemon --features vulkan 2>&1 > /tmp/t5build.log; echo exit=$?`
Expected: exit=0. `cargo test` clobbers the featured binary; this restores it so the daemon is safe to boot.

- [ ] **Step 4: Commit anything outstanding and verify sync state**

```bash
git status --short
git add -A crates docs
git commit -m "test: window-ladder mutation-kill extensions and acceptance evidence" || true
git log --oneline -6
```

Leave the branch/master state for Brice's merge ruling — do NOT push without an explicit go.
