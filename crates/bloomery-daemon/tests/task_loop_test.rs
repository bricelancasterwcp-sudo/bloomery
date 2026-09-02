//! The task loop's core mechanics: a task runs, its steps are journaled with
//! their arguments, and each way a step can go wrong is scored rather than
//! aborted.
//!
//! An unparseable turn is re-asked before the step fails; a grant violation
//! fails the step but not the task; `max_steps` and budget exhaustion each
//! terminate it; and a `find` resolves against the task's own cwd rather than
//! the process's.
//!
//! **Split 2026-09-01** (carried-debt slice D): this file was 1040 lines. The
//! codec-gate demotions and prompt preseeding are in
//! `task_loop_gate_test.rs`, envelope-v3's action-terminated parsing in
//! `task_loop_envelope_test.rs`.

mod common;

use bloomery_core::grant::Grant;
use bloomery_core::journal::{replay, Event, Journal};
use bloomery_daemon::task::{run_task, TaskStatus};
use std::path::PathBuf;

use common::task_loop::{fixture, fresh_dir, sandbox, scripted, spec};

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
