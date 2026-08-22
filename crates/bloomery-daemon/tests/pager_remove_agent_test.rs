//! `Pager::remove_agent` (Task 15): the `/v1` shim's ephemeral-agent
//! cleanup, exercised directly against `Pager<FakeSubstrate>` — split out
//! of `pager_test.rs` rather than appended to it, since that file is
//! already at the file-size cap.

use bloomery_core::journal::Journal;
use bloomery_daemon::agents::ImageStore;
use bloomery_daemon::pager::*;
use bloomery_substrate::{fake::FakeSubstrate, Reply};
use std::path::{Path, PathBuf};

fn ok(text: &str) -> Reply {
    Reply {
        text: text.into(),
        prompt_tokens: Some(8),
        completion_tokens: Some(4),
        duration_ms: 3,
    }
}

fn meta() -> bloomery_core::gguf::GgufMeta {
    bloomery_core::gguf::GgufMeta {
        arch: "qwen2".into(),
        layers: 28,
        attention_layers: 28,
        kv_heads: 4,
        head_dim: 128,
        training_ctx: 4096,
        weights_bytes: 1000,
        recurrent_state_bytes: 0,
    }
}

/// A clean scratch dir per test, so runs never share journals or images.
fn fresh_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Builds a pager over a fake substrate with `replies` scripted and a
/// constant free-VRAM probe. Same shape as `pager_test.rs::pager_in`.
fn pager_in(dir: &Path, replies: usize, free_vram: Option<u64>) -> Pager<FakeSubstrate> {
    let journal = Journal::open(&dir.join("j.jsonl")).unwrap();
    let images = ImageStore::new(&dir.join("img")).unwrap();
    let mut fake = FakeSubstrate::new();
    for _ in 0..replies {
        fake.script_reply(ok("r"));
    }
    Pager::new(fake, journal, images, Box::new(move || free_vram))
}

fn write_gguf(dir: &Path, contents: &[u8]) -> PathBuf {
    let gguf = dir.join("fake.gguf");
    std::fs::write(&gguf, contents).unwrap();
    gguf
}

/// Removing a resident agent destroys its context (the substrate sees
/// `destroy_context`) and forgets it entirely: a second removal, or any
/// further `infer`, sees `UnknownAgent`, not a residual entry.
#[test]
fn remove_agent_destroys_context_and_forgets_the_agent() {
    let dir = fresh_dir("bloomery-pager-remove-resident");
    let mut p = pager_in(&dir, 1, Some(10u64.pow(9)));
    let gguf = write_gguf(&dir, b"w");
    p.register_model("qwen", &gguf, meta(), None).unwrap();
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
    let mut p = pager_in(&dir, 0, Some(10u64.pow(9)));
    let gguf = write_gguf(&dir, b"w");
    p.register_model("qwen", &gguf, meta(), None).unwrap();
    let a = p.create_agent("qwen", 50, None, 1000).unwrap();
    p.remove_agent(&a.id, "test teardown").unwrap();
    assert!(p.status().agents.is_empty());
}

/// Removing an unknown id is named, not a panic.
#[test]
fn remove_agent_on_unknown_id_is_named() {
    let dir = fresh_dir("bloomery-pager-remove-unknown");
    let mut p = pager_in(&dir, 0, Some(10u64.pow(9)));
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
    let mut p = pager_in(&dir, 0, Some(10u64.pow(9)));
    let gguf = write_gguf(&dir, b"w");
    p.register_model("qwen", &gguf, meta(), None).unwrap();
    let a = p.create_agent("qwen", 50, None, 1000).unwrap();

    p.remove_agent(&a.id, "test teardown").unwrap();

    let events = bloomery_core::journal::replay(&journal_path).unwrap();
    assert!(events.iter().any(|e| matches!(e,
        bloomery_core::journal::Event::AgentRemoved { id: rid, reason }
            if rid == &a.id && reason == "test teardown")));
}
