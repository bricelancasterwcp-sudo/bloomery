//! The task loop under a demoted codec gate, and the prompt shapes the
//! envelope lens produces.
//!
//! A demoted spec refuses `patch` and `run` while still executing `read`
//! normally -- the demotion is per-verb, not per-task -- and a mid-task
//! window exhaustion is a scored terminal rather than an abort.
//!
//! Split out of `task_loop_test.rs` on 2026-09-01 (slice D).

mod common;

use bloomery_core::gguf::GgufMeta;
use bloomery_core::grant::Grant;
use bloomery_core::journal::{replay, Event, Journal};
use bloomery_daemon::agents::ImageStore;
use bloomery_daemon::config::EnvelopeLens;
use bloomery_daemon::pager::Pager;
use bloomery_daemon::task::{run_task, TaskStatus};
use bloomery_substrate::fake::FakeSubstrate;

use common::task_loop::{
    demoted_spec, fixture, fresh_dir, meta, preseeded_spec, sandbox, scripted, spec,
};

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
