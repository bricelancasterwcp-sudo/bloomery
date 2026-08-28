//! The task loop's five binding tests (Phase 2b/2c P3 Task 4 brief), plus a
//! regression guard for a carried obligation the review flagged as untested
//! (`a_find_resolves_against_the_task_cwd_not_the_process_cwd`).
//!
//! Mirrors `task_exec_read_find_test.rs`'s real-sandbox pattern (a real
//! tempdir, a real `Grant` scoped to it) layered on `pager_test.rs`'s
//! `Pager<FakeSubstrate>` fixture pattern: a fresh pager per test, one
//! registered model, one created agent, and scripted `<action>`-shaped
//! turns fed through `FakeSubstrate`'s FIFO reply queue — the model's
//! output is entirely pre-canned, so every test is deterministic and
//! GPU-free.

use bloomery_core::action::PatchCodec;
use bloomery_core::gguf::GgufMeta;
use bloomery_core::grant::Grant;
use bloomery_core::journal::{replay, Event, Journal};
use bloomery_daemon::agents::ImageStore;
use bloomery_daemon::config::EnvelopeLens;
use bloomery_daemon::pager::Pager;
use bloomery_daemon::task::{run_task, ExecBounds, TaskSpec, TaskStatus};
use bloomery_substrate::fake::FakeSubstrate;
use bloomery_substrate::Reply;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

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
        // Generous: the window law's `TrainingCtx` term must never be what
        // an ordinary (non-budget) test refuses on — only the
        // budget-exhaustion test is meant to refuse, and it refuses on
        // `Budget`, checked before the window gate.
        training_ctx: 65536,
        weights_bytes: 1000,
        value_length: None,
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

/// A fresh, per-test tempdir — PID + atomic counter, so parallel test
/// threads in one `cargo test` process never collide.
fn fresh_dir(tag: &str) -> PathBuf {
    static UNIQUE: AtomicU64 = AtomicU64::new(0);
    let unique = UNIQUE.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "bloomery-taskloop-{tag}-{}-{unique}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Builds `<dir>/sandbox` containing `file.txt` ("hello\nworld\n") and a
/// `Grant` scoped to exactly that directory for both read and write —
/// mirroring `task_exec_read_find_test.rs::sandbox`.
fn sandbox(dir: &std::path::Path) -> (PathBuf, Grant) {
    let sb = dir.join("sandbox");
    std::fs::create_dir_all(&sb).unwrap();
    std::fs::write(sb.join("file.txt"), "hello\nworld\n").unwrap();
    let sb = std::fs::canonicalize(&sb).unwrap();
    let g = Grant::from_json(&format!(
        r#"{{"read_roots":["{s}"],"write_roots":["{s}"],"commands":[]}}"#,
        s = sb.display()
    ))
    .unwrap();
    (sb, g)
}

/// Builds a `Pager<FakeSubstrate>` with one registered model, `replies`
/// scripted in FIFO order, and one created agent with `budget_tokens` as
/// its pager-level budget. Returns the pager and the new agent's id;
/// `dir` is the caller's scratch dir (also where `sandbox` and the task's
/// own journal live).
fn fixture(
    dir: &std::path::Path,
    budget_tokens: u64,
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
    let info = pager.create_agent("m", 100, None, budget_tokens).unwrap();
    (pager, info.id)
}

fn spec(grant: Grant, cwd: PathBuf, max_steps: u32) -> TaskSpec {
    demoted_spec(grant, cwd, max_steps, true)
}

/// Like [`spec`], but with an explicit `mutating_verbs` — the gate-G4
/// demotion tests need `false`.
fn demoted_spec(grant: Grant, cwd: PathBuf, max_steps: u32, mutating_verbs: bool) -> TaskSpec {
    TaskSpec {
        goal: "exercise the task loop".to_string(),
        grant,
        budget_tokens: 1_000_000,
        max_steps,
        cwd,
        patch_codec: PatchCodec::SearchReplace,
        bounds: bounds(),
        mutating_verbs,
        envelope: EnvelopeLens::V1,
        memory_block: None,
        window_ladder: false,
    }
}

/// Like [`spec`], but with `envelope: EnvelopeLens::V2` — the think-preseeded
/// lens (`docs/superpowers/evidence/2026-08-15-g4-protocol.md` §10, Amendment
/// 2).
fn preseeded_spec(grant: Grant, cwd: PathBuf, max_steps: u32) -> TaskSpec {
    TaskSpec {
        envelope: EnvelopeLens::V2,
        ..demoted_spec(grant, cwd, max_steps, true)
    }
}

/// Like [`spec`], but with `envelope: EnvelopeLens::V3` — the
/// action-terminated lens (protocol §11, Amendment 3).
fn action_stopped_spec(grant: Grant, cwd: PathBuf, max_steps: u32) -> TaskSpec {
    TaskSpec {
        envelope: EnvelopeLens::V3,
        ..demoted_spec(grant, cwd, max_steps, true)
    }
}

#[test]
fn a_read_then_done_task_completes() {
    let dir = fresh_dir("read-done");
    let (sb, g) = sandbox(&dir);
    let (mut pager, agent_id) = fixture(
        &dir,
        1_000_000,
        vec![
            scripted("<action verb=\"read\" path=\"file.txt\">\n</action>"),
            scripted("<action verb=\"done\">\nread it\n</action>"),
        ],
    );
    let task_journal_path = dir.join("task.jsonl");
    let mut task_journal = Journal::open(&task_journal_path).unwrap();
    let spec = spec(g, sb, 5);

    let result = run_task(&mut pager, &agent_id, &spec, &mut task_journal);

    assert_eq!(result.status, TaskStatus::Done);
    assert_eq!(result.summary.as_deref(), Some("read it"));
    assert_eq!(
        result.steps.len(),
        2,
        "expected [read, done], got {:?}",
        result.steps
    );
    assert_eq!(result.steps[0].verb, "read");
    assert!(result.steps[0].content.contains("hello"));
    assert!(
        !result.steps[0].failed,
        "a clean read step must not be marked failed"
    );
    assert_eq!(result.steps[1].verb, "done");
    assert!(!result.steps[1].failed, "the done step is never failed");

    let events = replay(&task_journal_path).unwrap();
    let task_steps = events
        .iter()
        .filter(|e| matches!(e, Event::TaskStep { .. }))
        .count();
    assert_eq!(task_steps, 2, "journal has 2 TaskStep events");
}

/// Like [`sandbox`], but the `Grant`'s `commands` also grant the `python3`
/// prefix (mirroring `task_exec_run_test.rs::sandbox`'s command-grant
/// shape) — needed for a scripted `run` turn.
fn sandbox_with_python(dir: &std::path::Path) -> (PathBuf, Grant) {
    let sb = dir.join("sandbox");
    std::fs::create_dir_all(&sb).unwrap();
    std::fs::write(sb.join("file.txt"), "hello\nworld\n").unwrap();
    let sb = std::fs::canonicalize(&sb).unwrap();
    let g = Grant::from_json(&format!(
        r#"{{"read_roots":["{s}"],"write_roots":["{s}"],"commands":[["python3"]]}}"#,
        s = sb.display()
    ))
    .unwrap();
    (sb, g)
}

/// Turn-5 spec §3: `Event::TaskStep.args` and `TaskStepRecord.args` carry
/// the action's model-supplied arguments, verbatim and in order, per the
/// per-verb mapping (`read` -> `[path]`, `run` -> the argv, `done` -> `[]`).
/// Scripts read -> run -> done, and pins the args on all three journaled
/// rows plus the in-memory record for the `run` step.
#[test]
fn task_step_args_carry_the_action_arguments_per_verb() {
    let dir = fresh_dir("args");
    let (sb, g) = sandbox_with_python(&dir);
    let target_rel_path = "file.txt";
    let (mut pager, agent_id) = fixture(
        &dir,
        1_000_000,
        vec![
            scripted(&format!(
                "<action verb=\"read\" path=\"{target_rel_path}\">\n</action>"
            )),
            scripted("<action verb=\"run\">\n[\"python3\", \"-c\", \"print(1)\"]\n</action>"),
            scripted("<action verb=\"done\">\nall done\n</action>"),
        ],
    );
    let task_journal_path = dir.join("task.jsonl");
    let mut task_journal = Journal::open(&task_journal_path).unwrap();
    let spec = spec(g, sb, 5);

    let result = run_task(&mut pager, &agent_id, &spec, &mut task_journal);

    assert_eq!(result.status, TaskStatus::Done);
    assert_eq!(result.steps.len(), 3, "got {:?}", result.steps);

    let rows: Vec<(String, Vec<String>)> = replay(&task_journal_path)
        .unwrap()
        .into_iter()
        .filter_map(|e| match e {
            Event::TaskStep { verb, args, .. } => Some((verb, args)),
            _ => None,
        })
        .collect();
    assert_eq!(rows[0].0, "read");
    assert_eq!(
        rows[0].1,
        vec![target_rel_path.to_string()],
        "read -> [path]"
    );
    assert_eq!(rows[1].0, "run");
    assert_eq!(
        rows[1].1,
        vec!["python3", "-c", "print(1)"],
        "run -> argv verbatim"
    );
    assert_eq!(rows[2].0, "done");
    assert!(rows[2].1.is_empty(), "done -> []");
    // and the in-memory record mirrors the journal
    assert_eq!(result.steps[1].args, vec!["python3", "-c", "print(1)"]);
}

/// Turn-5 spec §3, the `read` verb's `lines="A-B"` shape:
/// `action_args(&Action::Read { path, lines: Some((a, b)) })` -> `[path,
/// "lines=a-b"]` — the one `read` shape the args-per-verb test above
/// doesn't script (it only covers the no-`lines` case).
#[test]
fn task_step_args_carry_a_read_lines_range() {
    let dir = fresh_dir("args-lines-range");
    let (sb, g) = sandbox(&dir);
    let (mut pager, agent_id) = fixture(
        &dir,
        1_000_000,
        vec![
            scripted("<action verb=\"read\" path=\"file.txt\" lines=\"1-2\">\n</action>"),
            scripted("<action verb=\"done\">\nread the range\n</action>"),
        ],
    );
    let task_journal_path = dir.join("task.jsonl");
    let mut task_journal = Journal::open(&task_journal_path).unwrap();
    let spec = spec(g, sb, 5);

    let result = run_task(&mut pager, &agent_id, &spec, &mut task_journal);

    assert_eq!(result.status, TaskStatus::Done);
    assert_eq!(result.steps.len(), 2, "got {:?}", result.steps);
    assert_eq!(result.steps[0].verb, "read");
    assert_eq!(
        result.steps[0].args,
        vec!["file.txt".to_string(), "lines=1-2".to_string()],
        "a lines-range read -> [path, \"lines=a-b\"], turn-5 spec §3"
    );

    let rows: Vec<(String, Vec<String>)> = replay(&task_journal_path)
        .unwrap()
        .into_iter()
        .filter_map(|e| match e {
            Event::TaskStep { verb, args, .. } => Some((verb, args)),
            _ => None,
        })
        .collect();
    assert_eq!(
        rows[0],
        (
            "read".to_string(),
            vec!["file.txt".to_string(), "lines=1-2".to_string()]
        ),
        "the journaled read row must carry the same lines-range args"
    );
}

#[test]
fn an_unparseable_turn_is_re_asked_then_the_step_fails() {
    let dir = fresh_dir("reask");
    let (sb, g) = sandbox(&dir);
    let (mut pager, agent_id) = fixture(
        &dir,
        1_000_000,
        vec![
            scripted("garbage turn one, no action block at all"),
            scripted("garbage turn two, still nothing"),
            scripted("garbage turn three, nope"),
            scripted("<action verb=\"done\">\nfinally\n</action>"),
        ],
    );
    let task_journal_path = dir.join("task.jsonl");
    let mut task_journal = Journal::open(&task_journal_path).unwrap();
    let spec = spec(g, sb, 5);

    let result = run_task(&mut pager, &agent_id, &spec, &mut task_journal);

    assert_eq!(result.status, TaskStatus::Done);
    let failed_step = result
        .steps
        .iter()
        .find(|s| s.outcome == "unparseable after 2 re-asks");
    assert!(
        failed_step.is_some(),
        "expected an 'unparseable after 2 re-asks' step, got {:?}",
        result.steps
    );
    assert!(
        failed_step.unwrap().args.is_empty(),
        "an unparseable ('?') step's args must be empty, turn-5 spec §3: got {:?}",
        failed_step.unwrap().args
    );
    assert_eq!(result.steps.last().unwrap().verb, "done");

    let events = replay(&task_journal_path).unwrap();
    let question_mark_steps: Vec<Vec<String>> = events
        .iter()
        .filter_map(|e| match e {
            Event::TaskStep { verb, args, .. } if verb == "?" => Some(args.clone()),
            _ => None,
        })
        .collect();
    assert!(
        !question_mark_steps.is_empty(),
        "expected at least one '?' TaskStep journaled for the re-asked step"
    );
    assert!(
        question_mark_steps.iter().all(|a| a.is_empty()),
        "every journaled '?' row's args must be empty: {question_mark_steps:?}"
    );
    let done_steps = events
        .iter()
        .filter(|e| matches!(e, Event::TaskStep { verb, .. } if verb == "done"))
        .count();
    assert_eq!(done_steps, 1, "the eventual done turn is still journaled");
}

#[test]
fn a_grant_violation_is_a_failed_step_not_a_task_abort() {
    let dir = fresh_dir("grant-violation");
    let (sb, g) = sandbox(&dir);
    let (mut pager, agent_id) = fixture(
        &dir,
        1_000_000,
        vec![
            scripted("<action verb=\"read\" path=\"/etc/passwd\">\n</action>"),
            scripted("<action verb=\"done\">\nnoted the violation\n</action>"),
        ],
    );
    let task_journal_path = dir.join("task.jsonl");
    let mut task_journal = Journal::open(&task_journal_path).unwrap();
    let spec = spec(g, sb, 5);

    let result = run_task(&mut pager, &agent_id, &spec, &mut task_journal);

    assert_eq!(
        result.status,
        TaskStatus::Done,
        "a grant violation must not abort the task"
    );
    assert_eq!(result.steps.len(), 2);
    assert_eq!(result.steps[0].verb, "read");
    assert!(
        result.steps[0].outcome.contains("grant violation"),
        "expected a grant-violation outcome, got {:?}",
        result.steps[0].outcome
    );
    assert!(
        result.steps[0].failed,
        "a grant-violating step must be marked failed"
    );
    assert_eq!(
        result.steps[0].args,
        vec!["/etc/passwd".to_string()],
        "the refused read step's args must equal the refused path, turn-5 spec §3"
    );
    assert_eq!(result.steps[1].verb, "done");
    assert!(!result.steps[1].failed, "the done step is never failed");
}

#[test]
fn max_steps_terminates_the_task() {
    let dir = fresh_dir("max-steps");
    let (sb, g) = sandbox(&dir);
    let (mut pager, agent_id) = fixture(
        &dir,
        1_000_000,
        vec![
            scripted("<action verb=\"read\" path=\"file.txt\">\n</action>"),
            scripted("<action verb=\"read\" path=\"file.txt\">\n</action>"),
            scripted("<action verb=\"read\" path=\"file.txt\">\n</action>"),
        ],
    );
    let task_journal_path = dir.join("task.jsonl");
    let mut task_journal = Journal::open(&task_journal_path).unwrap();
    let spec = spec(g, sb, 3);

    let result = run_task(&mut pager, &agent_id, &spec, &mut task_journal);

    assert_eq!(result.status, TaskStatus::StepsExhausted);
    assert_eq!(result.steps.len(), 3);
    assert!(result.steps.iter().all(|s| s.verb == "read"));

    let events = replay(&task_journal_path).unwrap();
    let task_steps = events
        .iter()
        .filter(|e| matches!(e, Event::TaskStep { .. }))
        .count();
    assert_eq!(task_steps, 3, "exactly 3 steps journaled");
}

#[test]
fn budget_exhaustion_ends_the_task() {
    let dir = fresh_dir("budget");
    let (sb, g) = sandbox(&dir);
    // Zero budget: the very first `pager.infer` refuses on `Budget` before
    // ever touching the substrate, so no replies need to be scripted.
    let (mut pager, agent_id) = fixture(&dir, 0, vec![]);
    let task_journal_path = dir.join("task.jsonl");
    let mut task_journal = Journal::open(&task_journal_path).unwrap();
    let spec = spec(g, sb, 5);

    let result = run_task(&mut pager, &agent_id, &spec, &mut task_journal);

    assert_eq!(result.status, TaskStatus::BudgetExhausted);
    assert!(result.steps.is_empty());

    let events = replay(&task_journal_path).unwrap();
    let task_steps = events
        .iter()
        .filter(|e| matches!(e, Event::TaskStep { .. }))
        .count();
    assert_eq!(task_steps, 0, "nothing executed, nothing journaled");
}

/// Regression guard for a carried obligation from Task 1: `exec_find` takes
/// no `cwd` parameter, so `execute_action`'s `Find` dispatch arm MUST
/// absolutize a relative `path` against `spec.cwd` before calling it — a
/// relative prefix that skipped that step would silently fall back to
/// `exec_find`'s own default, the *daemon/test process's* current
/// directory (the repo root under `cargo test`), not the task's sandbox.
///
/// The mechanism this test pins: a needle string unique to this test run
/// (`ZQXFINDME-<pid>`, guaranteed not to exist anywhere else on disk) is
/// planted in a file *inside* the sandbox, in a subdirectory (`subdir/`),
/// and the scripted `find` turn uses the relative path `"."` — deliberately
/// relative, so the test actually exercises the absolutize step rather than
/// trivially passing with an already-absolute path. Two things would go
/// wrong if `absolutize` were ever dropped from that dispatch arm:
/// - `"."` would resolve against the real process cwd instead of `spec.cwd`;
/// - that resolved path sits outside the sandbox's granted read root, so
///   `grant.check_read` refuses it — the `find` step's outcome would flip
///   from `"found 1 matches"` to a `"grant violation"` string.
///
/// That's a structural signal (a different, assertable outcome string), not
/// a fragile "did it happen to see real repo content" check.
#[test]
fn a_find_resolves_against_the_task_cwd_not_the_process_cwd() {
    let dir = fresh_dir("find-cwd");
    let (sb, g) = sandbox(&dir);
    let needle = format!("ZQXFINDME-{}", std::process::id());
    std::fs::create_dir_all(sb.join("subdir")).unwrap();
    std::fs::write(sb.join("subdir").join("needle.txt"), format!("{needle}\n")).unwrap();

    let (mut pager, agent_id) = fixture(
        &dir,
        1_000_000,
        vec![
            scripted(&format!(
                "<action verb=\"find\" pattern=\"{needle}\" path=\".\">\n</action>"
            )),
            scripted("<action verb=\"done\">\nfound it\n</action>"),
        ],
    );
    let task_journal_path = dir.join("task.jsonl");
    let mut task_journal = Journal::open(&task_journal_path).unwrap();
    let spec = spec(g, sb, 5);

    let result = run_task(&mut pager, &agent_id, &spec, &mut task_journal);

    assert_eq!(result.status, TaskStatus::Done);
    assert_eq!(
        result.steps.len(),
        2,
        "expected [find, done], got {:?}",
        result.steps
    );
    assert_eq!(result.steps[0].verb, "find");
    assert!(
        !result.steps[0].outcome.contains("grant violation"),
        "relative '.' must resolve against spec.cwd (the sandbox), not the \
         process cwd — got outcome {:?}",
        result.steps[0].outcome
    );
    assert!(
        result.steps[0].outcome.contains("found 1 matches"),
        "expected exactly 1 match for the unique needle, got {:?}",
        result.steps[0].outcome
    );
    assert!(
        result.steps[0].content.contains("needle.txt"),
        "expected the match to name the needle file, got {:?}",
        result.steps[0].content
    );
    assert_eq!(
        result.steps[0].args,
        vec![needle.clone(), ".".to_string()],
        "find -> [pattern, path], verbatim (not the absolutized path), turn-5 spec §3"
    );
    assert_eq!(result.steps[1].verb, "done");
}

/// The pinned refusal outcome (gate G4, Task 7 brief) — Task 9's scoring and
/// the journal read this exact string.
const MUTATING_VERB_DEMOTED: &str = "verb unavailable: mutating verbs demoted (gate G4)";

/// Gate G4 structural enforcement (docs/superpowers/evidence/
/// 2026-08-15-g4-protocol.md §6: "a structural dispatch refusal —
/// prompting alone is not enforcement"): under a demoted spec
/// (`mutating_verbs: false`), a `patch` action that would otherwise land is
/// refused before `execute_action` ever runs — the target file is left
/// completely untouched, the refused step is recorded with the pinned
/// outcome and `failed: true`, and the task still completes normally
/// (a refused verb is a failed step, not a dead task).
#[test]
fn a_demoted_spec_refuses_patch_and_leaves_the_file_untouched() {
    let dir = fresh_dir("demoted-patch");
    let (sb, g) = sandbox(&dir);
    let (mut pager, agent_id) = fixture(
        &dir,
        1_000_000,
        vec![
            scripted(
                "<action verb=\"patch\" path=\"file.txt\">\n\
                 <<<<<<< SEARCH\n\
                 hello\n\
                 =======\n\
                 goodbye\n\
                 >>>>>>> REPLACE\n\
                 </action>",
            ),
            scripted("<action verb=\"done\">\nrefused as expected\n</action>"),
        ],
    );
    let task_journal_path = dir.join("task.jsonl");
    let mut task_journal = Journal::open(&task_journal_path).unwrap();
    let spec = demoted_spec(g, sb.clone(), 5, false);

    let result = run_task(&mut pager, &agent_id, &spec, &mut task_journal);

    assert_eq!(
        result.status,
        TaskStatus::Done,
        "a refused verb must not abort the task"
    );
    assert_eq!(
        result.steps.len(),
        2,
        "expected [patch (refused), done], got {:?}",
        result.steps
    );
    assert_eq!(result.steps[0].verb, "patch", "must record the real verb");
    assert!(
        result.steps[0].failed,
        "a refused verb must be marked failed"
    );
    assert_eq!(result.steps[0].outcome, MUTATING_VERB_DEMOTED);
    assert_eq!(result.steps[0].content, MUTATING_VERB_DEMOTED);
    assert_eq!(
        result.steps[0].args,
        vec!["file.txt".to_string()],
        "patch -> [path], turn-5 spec §3"
    );
    assert!(
        !result.steps[0]
            .args
            .iter()
            .any(|a| a.contains("goodbye") || a.contains("SEARCH") || a.contains("REPLACE")),
        "the patch body must never leak into args, got {:?}",
        result.steps[0].args
    );
    assert_eq!(result.steps[1].verb, "done");
    assert!(!result.steps[1].failed);

    let on_disk = std::fs::read_to_string(sb.join("file.txt")).unwrap();
    assert_eq!(
        on_disk, "hello\nworld\n",
        "the demoted patch must never touch the file"
    );

    let events = replay(&task_journal_path).unwrap();
    let patch_steps: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            Event::TaskStep { verb, outcome, .. } if verb == "patch" => Some(outcome.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(
        patch_steps,
        vec![MUTATING_VERB_DEMOTED.to_string()],
        "the journal must carry the same pinned refusal outcome"
    );
    let patch_args: Vec<Vec<String>> = events
        .iter()
        .filter_map(|e| match e {
            Event::TaskStep { verb, args, .. } if verb == "patch" => Some(args.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(
        patch_args,
        vec![vec!["file.txt".to_string()]],
        "the journaled patch row's args must equal [path], never the body"
    );
}

/// Same gate, the `run` verb: a demoted spec refuses it the same way a
/// `patch` is refused — real verb recorded, `failed: true`, pinned outcome,
/// task continues.
#[test]
fn a_demoted_spec_refuses_run_the_same_way() {
    let dir = fresh_dir("demoted-run");
    let (sb, g) = sandbox(&dir);
    let (mut pager, agent_id) = fixture(
        &dir,
        1_000_000,
        vec![
            scripted("<action verb=\"run\">\n[\"echo\", \"hi\"]\n</action>"),
            scripted("<action verb=\"done\">\nrefused as expected\n</action>"),
        ],
    );
    let task_journal_path = dir.join("task.jsonl");
    let mut task_journal = Journal::open(&task_journal_path).unwrap();
    let spec = demoted_spec(g, sb, 5, false);

    let result = run_task(&mut pager, &agent_id, &spec, &mut task_journal);

    assert_eq!(result.status, TaskStatus::Done);
    assert_eq!(result.steps.len(), 2, "got {:?}", result.steps);
    assert_eq!(result.steps[0].verb, "run", "must record the real verb");
    assert!(result.steps[0].failed);
    assert_eq!(result.steps[0].outcome, MUTATING_VERB_DEMOTED);
    assert_eq!(result.steps[0].content, MUTATING_VERB_DEMOTED);
    assert_eq!(
        result.steps[0].args,
        vec!["echo".to_string(), "hi".to_string()],
        "a refused run step's args must equal the refused argv, turn-5 spec §3"
    );
    assert_eq!(result.steps[1].verb, "done");
}

/// Amendment 1 (docs/superpowers/evidence/2026-08-15-g4-protocol.md §9): a
/// mid-task `PagerError::PromptTooLarge` from `infer` is a SCORED terminal
/// (`TaskStatus::WindowExhausted`), not an infrastructure abort — the steps
/// recorded before the refusal are preserved, and the terminal's summary
/// carries the pager's own refusal text.
///
/// The window is exhausted for real, not injected via a substrate-error
/// stand-in (the mechanism the probe's abort tests use instead): a tiny
/// `training_ctx` (1600 tokens) admits the first turn's short prompt (goal +
/// verb card, empty transcript — ~958 chars, ~1343 needed tokens) but not a
/// second turn whose transcript has grown by a large `read` observation
/// (~4000 bytes, pushing the needed tokens past 2600) — `Pager::infer`'s own
/// arithmetic gate (`pager.rs`'s `CHARS_PER_TOKEN` law) is what actually
/// refuses, exactly the mechanism protocol §9 amends.
#[test]
fn a_mid_task_window_exhaustion_is_a_scored_terminal_not_an_abort() {
    let dir = fresh_dir("window-exhausted");
    let sb = dir.join("sandbox");
    std::fs::create_dir_all(&sb).unwrap();
    let big = "z".repeat(4000);
    std::fs::write(sb.join("big.txt"), &big).unwrap();
    let sb = std::fs::canonicalize(&sb).unwrap();
    let g = Grant::from_json(&format!(
        r#"{{"read_roots":["{s}"],"write_roots":["{s}"],"commands":[]}}"#,
        s = sb.display()
    ))
    .unwrap();

    let journal = Journal::open(&dir.join("pager.jsonl")).unwrap();
    let images = ImageStore::new(&dir.join("img")).unwrap();
    let mut fake = FakeSubstrate::new();
    fake.script_reply(scripted(
        "<action verb=\"read\" path=\"big.txt\">\n</action>",
    ));
    let mut pager = Pager::new(fake, journal, images, Box::new(|| Some(1024 * 1024 * 1024)));
    let gguf = dir.join("m.gguf");
    std::fs::write(&gguf, b"fake weights").unwrap();
    let small_window_meta = GgufMeta {
        training_ctx: 1600,
        ..meta()
    };
    pager
        .register_model("m", &gguf, small_window_meta, None)
        .unwrap();
    let agent_id = pager.create_agent("m", 100, None, 1_000_000).unwrap().id;

    let task_journal_path = dir.join("task.jsonl");
    let mut task_journal = Journal::open(&task_journal_path).unwrap();
    let spec = spec(g, sb, 5);

    let result = run_task(&mut pager, &agent_id, &spec, &mut task_journal);

    assert_eq!(result.status, TaskStatus::WindowExhausted);
    assert_eq!(
        result.steps.len(),
        1,
        "the read step completed before the refusal must be preserved, got {:?}",
        result.steps
    );
    assert_eq!(result.steps[0].verb, "read");
    assert!(
        !result.steps[0].failed,
        "the preserved step is the one that actually succeeded"
    );
    let summary = result
        .summary
        .expect("a WindowExhausted terminal must carry the pager's refusal text");
    assert!(
        summary.contains("window"),
        "expected the pager's PromptTooLarge text, got {summary:?}"
    );

    let events = replay(&task_journal_path).unwrap();
    let task_steps = events
        .iter()
        .filter(|e| matches!(e, Event::TaskStep { .. }))
        .count();
    assert_eq!(
        task_steps, 1,
        "only the completed read step is journaled, not the refused turn"
    );
}

/// A demoted spec still lets `read` execute normally — the gate only
/// refuses `patch`/`run`, never the read-only verbs.
#[test]
fn a_demoted_spec_still_executes_read_normally() {
    let dir = fresh_dir("demoted-read");
    let (sb, g) = sandbox(&dir);
    let (mut pager, agent_id) = fixture(
        &dir,
        1_000_000,
        vec![
            scripted("<action verb=\"read\" path=\"file.txt\">\n</action>"),
            scripted("<action verb=\"done\">\nread it\n</action>"),
        ],
    );
    let task_journal_path = dir.join("task.jsonl");
    let mut task_journal = Journal::open(&task_journal_path).unwrap();
    let spec = demoted_spec(g, sb, 5, false);

    let result = run_task(&mut pager, &agent_id, &spec, &mut task_journal);

    assert_eq!(result.status, TaskStatus::Done);
    assert_eq!(result.steps.len(), 2, "got {:?}", result.steps);
    assert_eq!(result.steps[0].verb, "read");
    assert!(
        !result.steps[0].failed,
        "a demoted spec must not refuse read"
    );
    assert!(result.steps[0].content.contains("hello"));
    assert_eq!(result.steps[1].verb, "done");
}

/// Protocol §10 (Amendment 2, envelope-v2): with `think_preseed: true`, the
/// prompt the substrate actually receives ENDS WITH the literal pre-seed
/// `<think>\n\n</think>\n\n` — asserted with `ends_with`, not `contains`, so
/// a mutation that appended the literal anywhere else in the prompt (rather
/// than strictly after the transcript, with nothing after it) would still
/// fail this test. A single-turn `[done]` task means `FakeSubstrate`'s
/// `ctx_history` holds exactly the one rendered prompt (no `\n`-joined prior
/// turns to introduce ambiguity about which turn "ends" the string).
#[test]
fn a_preseeded_spec_ends_the_rendered_prompt_with_the_think_preseed_literal() {
    let dir = fresh_dir("preseed-on");
    let (sb, g) = sandbox(&dir);
    let (mut pager, agent_id) = fixture(
        &dir,
        1_000_000,
        vec![scripted("<action verb=\"done\">\nok\n</action>")],
    );
    let task_journal_path = dir.join("task.jsonl");
    let mut task_journal = Journal::open(&task_journal_path).unwrap();
    let spec = preseeded_spec(g, sb, 5);

    let result = run_task(&mut pager, &agent_id, &spec, &mut task_journal);

    assert_eq!(result.status, TaskStatus::Done);
    let history = pager
        .substrate()
        .ctx_history(1)
        .expect("context 1 is still resident after the task's only step");
    assert!(
        history.ends_with("<think>\n\n</think>\n\n"),
        "the rendered prompt must end with the literal pre-seed, got: {history:?}"
    );
}

/// The counterpart: with `think_preseed: false` (envelope-v1, the default),
/// the pre-seed literal is absent ANYWHERE in the rendered prompt — not just
/// missing from the end.
#[test]
fn a_non_preseeded_spec_never_carries_the_think_preseed_literal() {
    let dir = fresh_dir("preseed-off");
    let (sb, g) = sandbox(&dir);
    let (mut pager, agent_id) = fixture(
        &dir,
        1_000_000,
        vec![scripted("<action verb=\"done\">\nok\n</action>")],
    );
    let task_journal_path = dir.join("task.jsonl");
    let mut task_journal = Journal::open(&task_journal_path).unwrap();
    let spec = spec(g, sb, 5);
    assert_eq!(
        spec.envelope,
        EnvelopeLens::V1,
        "spec()'s default must be V1"
    );

    let result = run_task(&mut pager, &agent_id, &spec, &mut task_journal);

    assert_eq!(result.status, TaskStatus::Done);
    let history = pager
        .substrate()
        .ctx_history(1)
        .expect("context 1 is still resident after the task's only step");
    assert!(
        !history.contains("<think>\n\n</think>\n\n"),
        "a non-preseeded prompt must never carry the literal, got: {history:?}"
    );
}

// ---------------------------------------------------------------------------
// Protocol §11 (Amendment 3): envelope-v3's action-terminated stop.
// ---------------------------------------------------------------------------

/// The Q3-27B `MultipleActions` ramble, scripted literally: a correct
/// `<action>` block, trailing prose the model kept talking into, and a
/// second `<action>` block — exactly the shape Amendment 3's motivating
/// observation names.
fn ramble(first: &str, second: &str) -> Reply {
    scripted(&format!(
        "<action verb=\"done\">\n{first}\n</action>\nSome trailing chatter the model kept \
         generating into the void.\n<action verb=\"done\">\n{second}\n</action>"
    ))
}

/// Under `EnvelopeLens::V3`, `pager.infer`'s stop sequence truncates the
/// ramble at the first `</action>` BEFORE `parse_action_with_codec` ever
/// sees it — so the turn parses as ONE clean action, and the task completes
/// in a single step. The `MultipleActions` ramble is structurally gone, not
/// merely recovered-from.
#[test]
fn under_v3_a_two_action_scripted_reply_parses_as_one_clean_action() {
    let dir = fresh_dir("v3-clean-parse");
    let (sb, g) = sandbox(&dir);
    let (mut pager, agent_id) = fixture(&dir, 1_000_000, vec![ramble("first", "second")]);
    let task_journal_path = dir.join("task.jsonl");
    let mut task_journal = Journal::open(&task_journal_path).unwrap();
    let spec = action_stopped_spec(g, sb, 5);
    assert_eq!(spec.envelope, EnvelopeLens::V3);

    let result = run_task(&mut pager, &agent_id, &spec, &mut task_journal);

    assert_eq!(
        result.status,
        TaskStatus::Done,
        "the truncated turn must parse as one clean Done action: {:?}",
        result.steps
    );
    assert_eq!(
        result.steps.len(),
        1,
        "exactly one step — no re-ask was ever needed: {:?}",
        result.steps
    );
    assert_eq!(result.steps[0].verb, "done");
    assert_eq!(
        result.summary.as_deref(),
        Some("first"),
        "the surviving action is the FIRST one, truncated before the second"
    );
}

/// The exact same scripted reply, under `EnvelopeLens::V2` (no stop
/// sequence — "the stop is v3-only"): `pager.infer` returns the reply
/// untouched, so `parse_action_with_codec` sees both blocks and fails with
/// `MultipleActions { found: 2 }`. With only one reply scripted, the second
/// re-ask attempt drains `FakeSubstrate`'s queue and the task ends in
/// `Error` — but the FIRST step's outcome (recorded before that) is what
/// this test pins: the raw `MultipleActions` failure the v3 stop makes
/// structurally impossible above.
#[test]
fn under_v2_the_same_script_still_yields_multiple_actions() {
    let dir = fresh_dir("v2-multiple-actions");
    let (sb, g) = sandbox(&dir);
    let (mut pager, agent_id) = fixture(&dir, 1_000_000, vec![ramble("first", "second")]);
    let task_journal_path = dir.join("task.jsonl");
    let mut task_journal = Journal::open(&task_journal_path).unwrap();
    let spec = preseeded_spec(g, sb, 5);
    assert_eq!(spec.envelope, EnvelopeLens::V2);

    let result = run_task(&mut pager, &agent_id, &spec, &mut task_journal);

    assert!(
        !result.steps.is_empty(),
        "the first (re-asked) attempt must still be recorded"
    );
    assert!(
        result.steps[0].outcome.contains("MultipleActions"),
        "v2 has no stop sequence, so the untruncated two-block reply must fail to parse \
         as MultipleActions: {:?}",
        result.steps[0].outcome
    );
}

/// The one-source rule at the substrate boundary: under v3, `pager.infer`
/// actually passes `Some(ACTION_STOP)` through to the substrate — observed
/// via `FakeSubstrate::infer_stops`, the same mechanism the `/v1`
/// stop-is-always-None pin uses in `api_v1_test.rs`.
#[test]
fn under_v3_the_infer_call_carries_the_action_stop_sequence() {
    let dir = fresh_dir("v3-stop-recorded");
    let (sb, g) = sandbox(&dir);
    let (mut pager, agent_id) = fixture(
        &dir,
        1_000_000,
        vec![scripted("<action verb=\"done\">\nok\n</action>")],
    );
    let task_journal_path = dir.join("task.jsonl");
    let mut task_journal = Journal::open(&task_journal_path).unwrap();
    let spec = action_stopped_spec(g, sb, 5);

    let result = run_task(&mut pager, &agent_id, &spec, &mut task_journal);
    assert_eq!(result.status, TaskStatus::Done);

    assert_eq!(
        pager.substrate().infer_stops(),
        &[Some("</action>".to_string())],
        "a v3 turn's infer call must carry the action stop sequence"
    );
}

/// The counterpart at the substrate boundary: under v1/v2, `pager.infer`
/// never passes a stop sequence — `None`, every time.
#[test]
fn under_v1_and_v2_the_infer_call_never_carries_a_stop_sequence() {
    let dir = fresh_dir("v1-v2-no-stop");
    let (sb, g) = sandbox(&dir);
    let (mut pager, agent_id) = fixture(
        &dir,
        1_000_000,
        vec![scripted("<action verb=\"done\">\nok\n</action>")],
    );
    let task_journal_path = dir.join("task.jsonl");
    let mut task_journal = Journal::open(&task_journal_path).unwrap();
    let spec = spec(g, sb, 5);

    let result = run_task(&mut pager, &agent_id, &spec, &mut task_journal);
    assert_eq!(result.status, TaskStatus::Done);

    assert_eq!(pager.substrate().infer_stops(), &[None]);
}
