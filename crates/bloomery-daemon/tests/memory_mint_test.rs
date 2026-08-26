//! Failing-first tests for `memory::mint` (memory-organ Task 5; spec
//! `docs/superpowers/specs/2026-08-26-memory-organ-design.md` §2).
//!
//! Written before `src/memory/mint.rs` exists, per the task brief's Step 1
//! — the whole file fails to compile (no such module) until Step 3 lands.
//! That compile failure is this task's captured RED.
//!
//! Pure-function tests: the mint bar is "computed from steps alone" (task
//! brief), so every `TaskResult` here is hand-built directly — no
//! `FakeSubstrate`, no real `run_task`, no filesystem. `touched_files` and
//! `landed_patches` start empty and are populated only where a test needs
//! them (`uncomputable_touched_file_refuses_to_mint`,
//! `build_episode_renders_search_replace_wire_form`).

use std::collections::BTreeMap;

use bloomery_core::action::PatchBody;
use bloomery_daemon::memory::mint::{build_episode, verifying_run, MintInputs};
use bloomery_daemon::memory::record::Fingerprint;
use bloomery_daemon::task::{PreTouch, TaskResult, TaskStatus, TaskStepRecord};

/// One hand-built step. The numeric `step` field is set to `0` — deliberately
/// unused: the mint bar reads `result.steps`' own Vec order (a step's real
/// `step: u32` field can repeat across a re-ask, per `task_loop`'s
/// multi-attempt journaling), never this field, so these fixtures don't need
/// to fake it.
fn step(verb: &str, outcome: &str, failed: bool, args: &[&str]) -> TaskStepRecord {
    TaskStepRecord {
        step: 0,
        verb: verb.to_string(),
        outcome: outcome.to_string(),
        content: String::new(),
        failed,
        args: args.iter().map(|s| (*s).to_string()).collect(),
    }
}

fn task_result(status: TaskStatus, steps: Vec<TaskStepRecord>) -> TaskResult {
    TaskResult {
        status,
        steps,
        summary: None,
        touched_files: BTreeMap::new(),
        landed_patches: Vec::new(),
    }
}

#[test]
fn done_patch_then_run_exit_0_mints() {
    let steps = vec![
        step("read", "read 12 bytes", false, &["a.txt"]),
        step("patch", "patched a.txt", false, &["a.txt"]),
        step("run", "ran python3 exit 0", false, &["python3"]),
        step("done", "all set", false, &[]),
    ];
    let result = task_result(TaskStatus::Done, steps);

    let run = verifying_run(&result);
    assert_eq!(
        run.map(|s| s.outcome.as_str()),
        Some("ran python3 exit 0"),
        "{result:?}"
    );
}

/// The "refusal shape": the model read the file, ran a command that
/// happened to exit 0 (e.g. a test suite that already passed before any
/// change), and declared `done` — WITHOUT ever landing a patch. A passing
/// run alone must never mint: nothing was actually fixed. This is the
/// fixture the ≥1-successful-patch requirement exists to reject, so it is
/// also the one that must flip to `Some` if that requirement is dropped
/// (mutation check 3, task brief Step 5).
#[test]
fn refusal_shape_does_not_mint() {
    let steps = vec![
        step("read", "read 12 bytes", false, &["a.txt"]),
        step("run", "ran python3 exit 0", false, &["python3"]),
        step("done", "nothing to fix", false, &[]),
    ];
    let result = task_result(TaskStatus::Done, steps);

    assert!(
        verifying_run(&result).is_none(),
        "no successful patch step at all, even though the run passed: {result:?}"
    );
}

#[test]
fn run_nonzero_exit_does_not_mint() {
    let steps = vec![
        step("patch", "patched a.txt", false, &["a.txt"]),
        step("run", "ran python3 exit 1", false, &["python3"]),
        step("done", "tests failed", false, &[]),
    ];
    let result = task_result(TaskStatus::Done, steps);

    assert!(
        verifying_run(&result).is_none(),
        "a non-zero exit is not verifying evidence: {result:?}"
    );
}

#[test]
fn run_before_the_last_successful_patch_does_not_mint() {
    let steps = vec![
        step("patch", "patched a.txt", false, &["a.txt"]),
        step("run", "ran python3 exit 0", false, &["python3"]),
        step("patch", "patched a.txt", false, &["a.txt"]),
        step("done", "all set", false, &[]),
    ];
    let result = task_result(TaskStatus::Done, steps);

    assert!(
        verifying_run(&result).is_none(),
        "the run happened before the LAST successful patch: {result:?}"
    );
}

/// Ruling amendment (code review finding, 2026-08-26): the mint bar reads
/// the LAST completed run in the window, not the first passing one. A
/// stale earlier pass must not rescue a later completed failure — the
/// trajectory's own final evidence in the window is a failure, so this
/// must not mint.
#[test]
fn a_later_completed_failure_after_an_earlier_pass_does_not_mint() {
    let steps = vec![
        step("patch", "patched a.txt", false, &["a.txt"]),
        step("run", "ran python3 exit 0", false, &["python3"]),
        step("run", "ran python3 exit 1", false, &["python3"]),
        step("done", "all set", false, &[]),
    ];
    let result = task_result(TaskStatus::Done, steps);

    assert!(
        verifying_run(&result).is_none(),
        "the final completed run in the window failed, so an earlier pass must not save it: {result:?}"
    );
}

/// Mirror of the above: a later completed PASS rescues an earlier
/// completed failure in the same window — the final evidence is what
/// counts, and here it passes.
#[test]
fn a_later_completed_pass_after_an_earlier_failure_mints_citing_the_later_run() {
    let steps = vec![
        step("patch", "patched a.txt", false, &["a.txt"]),
        step("run", "ran python3 exit 1", false, &["python3"]),
        step("run", "ran python3 exit 0", false, &["python3"]),
        step("done", "all set", false, &[]),
    ];
    let result = task_result(TaskStatus::Done, steps);

    let run = verifying_run(&result);
    assert_eq!(
        run.map(|s| s.outcome.as_str()),
        Some("ran python3 exit 0"),
        "must cite the LATER, passing run, not the earlier failure: {result:?}"
    );
}

/// A structurally failed run (`failed: true` — a timeout here, never a
/// completion at all per `exec_run`) occurring AFTER an earlier completed
/// pass must not veto it: a run that never finished carries no evidence
/// about anything, so the last COMPLETED run in the window is still the
/// earlier pass.
#[test]
fn a_structurally_failed_run_after_a_completed_pass_does_not_veto_it() {
    let steps = vec![
        step("patch", "patched a.txt", false, &["a.txt"]),
        step("run", "ran python3 exit 0", false, &["python3"]),
        step("run", "ran python3 timed out", true, &["python3"]),
        step("done", "all set", false, &[]),
    ];
    let result = task_result(TaskStatus::Done, steps);

    let run = verifying_run(&result);
    assert_eq!(
        run.map(|s| s.outcome.as_str()),
        Some("ran python3 exit 0"),
        "a structurally-failed run is not a completion and must not veto the earlier pass: {result:?}"
    );
}

#[test]
fn non_done_status_and_failed_run_do_not_mint() {
    let steps_exhausted = task_result(
        TaskStatus::StepsExhausted,
        vec![
            step("patch", "patched a.txt", false, &["a.txt"]),
            step("run", "ran python3 exit 0", false, &["python3"]),
        ],
    );
    assert!(
        verifying_run(&steps_exhausted).is_none(),
        "a task that never reached Done must not mint: {steps_exhausted:?}"
    );

    let failed_run = task_result(
        TaskStatus::Done,
        vec![
            step("patch", "patched a.txt", false, &["a.txt"]),
            step("run", "ran python3 timed out", true, &["python3"]),
            step("done", "gave up", false, &[]),
        ],
    );
    assert!(
        verifying_run(&failed_run).is_none(),
        "a run step that never completed must not mint: {failed_run:?}"
    );
}

#[test]
fn uncomputable_touched_file_refuses_to_mint() {
    let steps = vec![
        step("read", "truncated at cap", false, &["a.txt"]),
        step("patch", "patched a.txt", false, &["a.txt"]),
        step("run", "ran python3 exit 0", false, &["python3"]),
        step("done", "all set", false, &[]),
    ];
    let mut result = task_result(TaskStatus::Done, steps);
    result
        .touched_files
        .insert("/w/a.txt".to_string(), PreTouch::Uncomputable);

    assert!(
        verifying_run(&result).is_some(),
        "the mint bar itself must be satisfied here, independent of touched_files: {result:?}"
    );

    let inputs = MintInputs {
        goal: "fix a.txt",
        model: "test-model",
        envelope: "v1",
        minted_at: 1,
    };
    assert!(
        build_episode(&result, &inputs).is_none(),
        "an Uncomputable pre-touch fingerprint must refuse the mint even though the bar is clear"
    );
}

#[test]
fn build_episode_renders_search_replace_wire_form() {
    let steps = vec![
        step("read", "read 12 bytes", false, &["a.txt"]),
        step("patch", "patched a.txt", false, &["a.txt"]),
        step(
            "run",
            "ran python3 exit 0",
            false,
            &["python3", "-m", "unittest"],
        ),
        step("done", "all set", false, &[]),
    ];
    let mut result = task_result(TaskStatus::Done, steps);
    result
        .touched_files
        .insert("/w/a.txt".to_string(), PreTouch::Sha256("aa".to_string()));
    result.landed_patches.push((
        "/w/a.txt".to_string(),
        PatchBody::SearchReplace {
            search: "old code".to_string(),
            replace: "new code".to_string(),
        },
    ));

    let inputs = MintInputs {
        goal: "fix a.txt",
        model: "test-model",
        envelope: "v1",
        minted_at: 42,
    };
    let episode = build_episode(&result, &inputs).expect("bar satisfied, no Uncomputable");

    assert_eq!(episode.landed_patches.len(), 1);
    let patch = &episode.landed_patches[0];
    assert_eq!(patch.path, "/w/a.txt");
    assert_eq!(patch.codec, "search_replace");
    assert_eq!(
        patch.body,
        "<<<<<<< SEARCH\nold code\n=======\nnew code\n>>>>>>> REPLACE",
        "the wire form bloomery_core::action::patch parses (patch.rs:17) must round-trip byte-exact"
    );

    assert_eq!(episode.cited_files.len(), 1);
    assert_eq!(episode.cited_files[0].path, "/w/a.txt");
    assert_eq!(
        episode.cited_files[0].fingerprint,
        Fingerprint::Sha256("aa".to_string())
    );
    assert_eq!(
        episode.run_evidence.argv,
        vec![
            "python3".to_string(),
            "-m".to_string(),
            "unittest".to_string()
        ]
    );
    assert_eq!(episode.run_evidence.outcome, "ran python3 exit 0");
    assert_eq!(episode.status, "verified");
    assert_eq!(episode.contradicted_by, None);
    assert_eq!(episode.minted_at, 42);
    assert_eq!(episode.minted_by_model, "test-model");
    assert_eq!(episode.minted_by_envelope, "v1");
    assert_eq!(episode.goal_text, "fix a.txt");
    assert_eq!(episode.trajectory, vec!["read", "patch", "run", "done"]);
}
