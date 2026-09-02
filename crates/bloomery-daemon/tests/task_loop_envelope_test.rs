//! Protocol §11 (Amendment 3): envelope-v3's action-terminated parsing.
//!
//! Under v3 a two-action scripted reply parses as one clean action and the
//! infer call carries the action stop sequence; under v1/v2 it does neither.
//! The same script yielding different action counts across lenses is the
//! point -- the lens is what decides where a turn ends.
//!
//! Split out of `task_loop_test.rs` on 2026-09-01 (slice D).

mod common;

use bloomery_core::grant::Grant;
use bloomery_core::journal::Journal;
use bloomery_daemon::config::EnvelopeLens;
use bloomery_daemon::task::{run_task, TaskSpec, TaskStatus};
use bloomery_substrate::Reply;
use std::path::PathBuf;

use common::task_loop::{
    demoted_spec, fixture, fresh_dir, preseeded_spec, sandbox, scripted, spec,
};

/// Like [`spec`], but with `envelope: EnvelopeLens::V3` — the
/// action-terminated lens (protocol §11, Amendment 3).
fn action_stopped_spec(grant: Grant, cwd: PathBuf, max_steps: u32) -> TaskSpec {
    TaskSpec {
        envelope: EnvelopeLens::V3,
        ..demoted_spec(grant, cwd, max_steps, true)
    }
}

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

// ---------------------------------------------------------------------------
// Protocol §11 (Amendment 3): envelope-v3's action-terminated stop.
// ---------------------------------------------------------------------------

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

/// Turn-6 spec §3.4: a declared `done`'s attributes land in
/// `TaskStep.args` as `["outcome=<v>", "reason=<v>"]` (present attributes
/// only); an undeclared `done` keeps today's empty args (pinned by the
/// existing args-per-verb test above).
#[test]
fn task_step_args_carry_done_declarations() {
    let dir = fresh_dir("args-done-declared");
    let (sb, g) = sandbox(&dir);
    let (mut pager, agent_id) = fixture(
        &dir,
        1_000_000,
        vec![scripted(
            "<action verb=\"done\" outcome=\"refused\" reason=\"no-defect\">\n\
             evidence: file.txt:1 `hello`\n\
             The goal describes a defect that is not present.\n</action>",
        )],
    );
    let task_journal_path = dir.join("task.jsonl");
    let mut task_journal = Journal::open(&task_journal_path).unwrap();
    let spec = spec(g, sb, 5);

    let result = run_task(&mut pager, &agent_id, &spec, &mut task_journal);

    assert_eq!(result.status, TaskStatus::Done);
    assert_eq!(result.steps.len(), 1);
    assert_eq!(result.steps[0].verb, "done");
    assert_eq!(
        result.steps[0].args,
        vec![
            "outcome=refused".to_string(),
            "reason=no-defect".to_string()
        ]
    );
}
