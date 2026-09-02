//! Fixtures shared by the `codec_probe_*` integration tests.
//!
//! Lifted out of `codec_probe_test.rs` when it was split (2026-09-01,
//! carried-debt slice D). Everything here is reached by at least two of the
//! resulting files -- most of it by all three -- which is what that file's
//! 273-line fixture header actually was.

use bloomery_core::journal::{replay, Event, Journal};
use bloomery_daemon::agents::ImageStore;
use bloomery_daemon::codec_probe::fixtures::{parse_fixture_set, FixtureSet};
use bloomery_daemon::pager::Pager;
use bloomery_substrate::fake::FakeSubstrate;
use bloomery_substrate::Reply;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// Deliberately carries both characters invariant 2 maps to `-`: a model
/// name is a scratch *directory* name, and `qwen2.5-coder:7b` / an
/// org-prefixed HF id would otherwise nest or collide.
pub const MODEL: &str = "org/m:7b";

/// A 2-fixture stand-in for the shipped N=20 set: the engine is generic over
/// set size (Task 5 owns the real set's content), and 2 fixtures is the
/// smallest number that still proves per-fixture isolation — fresh dir,
/// fresh agent, fresh journal handle — and that the verdict aggregates.
pub const TEST_SET: &str = r#"
set = "codec-tasks-test"

[[fixture]]
name = "t1-alpha"
lens = "plaintext"
target = "a.txt"
goal = "fix the broken line in a.txt"

[[fixture.file]]
path = "a.txt"
contents = "alpha\nbroken\n"

[fixture.reference]
search = "broken"
replace = "fixed"

[[fixture]]
name = "t2-beta"
lens = "plaintext"
target = "b.txt"
goal = "fix the broken line in b.txt"

[[fixture.file]]
path = "b.txt"
contents = "beta\nbroken\n"

[fixture.reference]
search = "broken"
replace = "fixed"
"#;

pub fn test_set() -> FixtureSet {
    parse_fixture_set(TEST_SET).expect("inline test fixture set parses")
}

/// A fresh, per-test tempdir — PID + atomic counter, so parallel test
/// threads in one `cargo test` process never collide.
pub fn fresh_dir(tag: &str) -> PathBuf {
    static UNIQUE: AtomicU64 = AtomicU64::new(0);
    let unique = UNIQUE.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "bloomery-codecprobe-{tag}-{}-{unique}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

pub fn scripted(text: &str) -> Reply {
    Reply {
        text: text.to_string(),
        prompt_tokens: Some(8),
        completion_tokens: Some(4),
        duration_ms: 1,
    }
}

/// A `SearchReplace`-codec patch turn.
pub fn sr_patch(path: &str, search: &str, replace: &str) -> Reply {
    scripted(&format!(
        "<action verb=\"patch\" path=\"{path}\">\n\
         <<<<<<< SEARCH\n{search}\n=======\n{replace}\n>>>>>>> REPLACE\n\
         </action>"
    ))
}

pub fn done(summary: &str) -> Reply {
    scripted(&format!("<action verb=\"done\">\n{summary}\n</action>"))
}

pub fn read(path: &str) -> Reply {
    scripted(&format!(
        "<action verb=\"read\" path=\"{path}\">\n</action>"
    ))
}

/// A pager with [`MODEL`] registered, `replies` scripted FIFO, and its task
/// journal pointed at `dir/tasks.jsonl` (where `run_task`'s own `TaskStep`
/// events land — the probe's `CodecFixture`/`CodecVerdict` events go to the
/// *pager's* journal, `dir/pager.jsonl`, which is what every assertion below
/// replays).
pub fn build_pager(dir: &Path, replies: Vec<Reply>) -> Pager<FakeSubstrate> {
    let journal = Journal::open(&dir.join("pager.jsonl")).unwrap();
    let images = ImageStore::new(&dir.join("img")).unwrap();
    let mut fake = FakeSubstrate::new();
    for r in replies {
        fake.script_reply(r);
    }
    let mut pager = Pager::new(fake, journal, images, Box::new(|| Some(1024 * 1024 * 1024)));
    let gguf = dir.join("m.gguf");
    std::fs::write(&gguf, b"fake weights").unwrap();
    pager.register_model(MODEL, &gguf, meta(), None).unwrap();
    pager.set_task_journal_path(dir.join("tasks.jsonl"));
    pager
}

pub fn pager_events(dir: &Path) -> Vec<Event> {
    replay(&dir.join("pager.jsonl")).unwrap()
}

pub fn fixture_events(events: &[Event]) -> Vec<&Event> {
    events
        .iter()
        .filter(|e| matches!(e, Event::CodecFixture { .. }))
        .collect()
}

pub fn verdict_events(events: &[Event]) -> Vec<&Event> {
    events
        .iter()
        .filter(|e| matches!(e, Event::CodecVerdict { .. }))
        .collect()
}

pub fn removed_agents(events: &[Event]) -> Vec<&str> {
    events
        .iter()
        .filter_map(|e| match e {
            Event::AgentRemoved { id, .. } => Some(id.as_str()),
            _ => None,
        })
        .collect()
}

/// `(fixture, codec, landed, steps, detail)` for every `CodecFixture` event,
/// in journal order.
pub fn fixture_rows(events: &[Event]) -> Vec<(String, String, bool, u32, String)> {
    events
        .iter()
        .filter_map(|e| match e {
            Event::CodecFixture {
                fixture,
                codec,
                landed,
                steps,
                detail,
                ..
            } => Some((
                fixture.clone(),
                codec.clone(),
                *landed,
                *steps,
                detail.clone(),
            )),
            _ => None,
        })
        .collect()
}

/// `(fixture, agent)` for every CodecFixture event, in journal order.
pub fn fixture_agents(events: &[Event]) -> Vec<(String, Option<String>)> {
    events
        .iter()
        .filter_map(|e| match e {
            Event::CodecFixture { fixture, agent, .. } => Some((fixture.clone(), agent.clone())),
            _ => None,
        })
        .collect()
}

pub fn meta() -> bloomery_core::gguf::GgufMeta {
    bloomery_core::gguf::GgufMeta {
        arch: "qwen2".into(),
        layers: 4,
        attention_layers: 4,
        kv_heads: 2,
        head_dim: 32,
        training_ctx: 65536,
        weights_bytes: 1000,
        value_length: None,
        recurrent_state_bytes: 0,
    }
}
