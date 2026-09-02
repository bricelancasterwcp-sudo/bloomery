//! `Pager::remove_agent` (Task 15): the `/v1` shim's ephemeral-agent
//! cleanup, exercised directly against `Pager<FakeSubstrate>` — split out
//! of `pager_test.rs` rather than appended to it, since that file is
//! already at the file-size cap.

mod common;

use bloomery_daemon::pager::*;
use common::pager::{fresh_dir, meta, pager_in, write_gguf};

/// Removing a resident agent destroys its context (the substrate sees
/// `destroy_context`) and forgets it entirely: a second removal, or any
/// further `infer`, sees `UnknownAgent`, not a residual entry.
#[test]
fn remove_agent_destroys_context_and_forgets_the_agent() {
    let dir = fresh_dir("bloomery-pager-remove-resident");
    let (mut p, _, _) = pager_in(&dir, 1, Some(10u64.pow(9)));
    let gguf = write_gguf(&dir, "fake.gguf", b"w");
    p.register_model("qwen", &gguf, meta(1000), None).unwrap();
    let a = p.create_agent("qwen", 50, None, 1000).unwrap();
    p.infer(&a.id, "hi", 16, None).unwrap();
    assert!(p
        .substrate()
        .calls()
        .iter()
        .any(|c| c.starts_with("create_context")));

    p.remove_agent(&a.id, "test teardown").unwrap();

    assert!(p
        .substrate()
        .calls()
        .iter()
        .any(|c| c.starts_with("destroy_context")));
    match p.infer(&a.id, "hi again", 16, None) {
        Err(PagerError::UnknownAgent(id)) => assert_eq!(id, a.id),
        other => panic!("expected UnknownAgent after removal, got {other:?}"),
    }
    match p.remove_agent(&a.id, "test teardown") {
        Err(PagerError::UnknownAgent(id)) => assert_eq!(id, a.id),
        other => panic!("expected UnknownAgent on double removal, got {other:?}"),
    }
}

/// Removing a `Fresh` agent (never inferred, no context to destroy) is not
/// an error — mirrors `suspend`'s own no-context-is-fine behavior.
#[test]
fn remove_agent_on_a_fresh_agent_is_not_an_error() {
    let dir = fresh_dir("bloomery-pager-remove-fresh");
    let (mut p, _, _) = pager_in(&dir, 0, Some(10u64.pow(9)));
    let gguf = write_gguf(&dir, "fake.gguf", b"w");
    p.register_model("qwen", &gguf, meta(1000), None).unwrap();
    let a = p.create_agent("qwen", 50, None, 1000).unwrap();
    p.remove_agent(&a.id, "test teardown").unwrap();
    assert!(p.status().agents.is_empty());
}

/// Removing an unknown id is named, not a panic.
#[test]
fn remove_agent_on_unknown_id_is_named() {
    let dir = fresh_dir("bloomery-pager-remove-unknown");
    let (mut p, _, _) = pager_in(&dir, 0, Some(10u64.pow(9)));
    match p.remove_agent("nope", "test teardown") {
        Err(PagerError::UnknownAgent(id)) => assert_eq!(id, "nope"),
        other => panic!("expected UnknownAgent, got {other:?}"),
    }
}

/// The reason travels into the journal verbatim on the successful path —
/// 2b's task loop and any operator reading the journal must see *why* an
/// agent left the table, not just that it did.
#[test]
fn remove_agent_journals_the_removal_with_its_reason() {
    let dir = fresh_dir("bloomery-pager-remove-journals-reason");
    let journal_path = dir.join("j.jsonl");
    let (mut p, _, _) = pager_in(&dir, 0, Some(10u64.pow(9)));
    let gguf = write_gguf(&dir, "fake.gguf", b"w");
    p.register_model("qwen", &gguf, meta(1000), None).unwrap();
    let a = p.create_agent("qwen", 50, None, 1000).unwrap();

    p.remove_agent(&a.id, "test teardown").unwrap();

    let events = bloomery_core::journal::replay(&journal_path).unwrap();
    assert!(events.iter().any(|e| matches!(e,
        bloomery_core::journal::Event::AgentRemoved { id: rid, reason }
            if rid == &a.id && reason == "test teardown")));
}
