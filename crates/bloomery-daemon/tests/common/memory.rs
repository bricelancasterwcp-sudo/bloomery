//! Fixtures shared by the `memory_*` integration tests.
//!
//! Collected on 2026-09-01 (carried-debt slice D) from `memory_refalsify_test`
//! and `memory_task_test`, which between them carried 17 helpers of the same
//! name -- 12 byte-identical, 5 genuinely forked. Only the identical twelve
//! are here; the five that diverged (`await_stamp`, `drive`, `fresh_dir`,
//! `sandbox`, `stamp_for`) stay with their callers, because unifying a fork
//! is a behaviour decision rather than a chore.
//!
//! `fresh_dir` is the exception that was worth unifying: its six copies across
//! the family differed *only* in a hard-coded prefix string, so it takes the
//! scope as an argument here.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use bloomery_core::action::PatchCodec;
use bloomery_core::gguf::GgufMeta;
use bloomery_core::grant::Grant;
use bloomery_core::journal::{replay, Event, Journal};
use bloomery_daemon::agents::ImageStore;
use bloomery_daemon::config::EnvelopeLens;
use bloomery_daemon::memory::store::MemoryStore;
use bloomery_daemon::memory::MemoryContext;
use bloomery_daemon::pager::Pager;
use bloomery_daemon::task::{ExecBounds, TaskRegistry, TaskResult, TaskSpec, TaskStatus};
use bloomery_substrate::fake::FakeSubstrate;
use bloomery_substrate::Reply;

/// A clean scratch dir per call: `bloomery-{scope}-{tag}-{pid}-{seq}`.
///
/// The six copies this replaces were identical but for the `scope` literal --
/// `refalsify`, `memtask`, `memcapture` and so on -- which is now a parameter
/// so the directory still names the suite that made it.
pub fn fresh_dir(scope: &str, tag: &str) -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static UNIQUE: AtomicU64 = AtomicU64::new(0);
    let unique = UNIQUE.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "bloomery-{scope}-{tag}-{}-{unique}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// The bytes `a.py` carries before any task touches it — the pre-first-touch
/// fingerprint every minted episode in this file cites.
pub const BEFORE: &[u8] = b"x = 1\n";

pub fn build_pager(dir: &Path, replies: Vec<Reply>) -> (Pager<FakeSubstrate>, String) {
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
    let info = pager.create_agent("m", 100, None, 1_000_000).unwrap();
    (pager, info.id)
}

pub fn contradicted_ids(events: &[Event]) -> Vec<(String, String)> {
    events
        .iter()
        .filter_map(|e| match e {
            Event::MemoryContradicted {
                task_id,
                episode_id,
                ..
            } => Some((task_id.clone(), episode_id.clone())),
            _ => None,
        })
        .collect()
}

/// Every `Degraded` reason on the journal — how an operator-visible skip
/// names itself, and here how "silenced by the oversize rule" is told apart
/// from "silenced by a fingerprint miss", which stamp identically.
pub fn degraded_reasons(events: &[Event]) -> Vec<String> {
    events
        .iter()
        .filter_map(|e| match e {
            Event::Degraded { reason } => Some(reason.clone()),
            _ => None,
        })
        .collect()
}

/// An operational organ: config switch on, a store in `dir/memory`, and the
/// `[memory] refalsify` opt-in (refalsify spec §5) under the test's control.
pub fn memory_ctx(dir: &Path, enabled: bool, refalsify: bool) -> Arc<MemoryContext> {
    let store = MemoryStore::load(&store_path(dir)).unwrap();
    Arc::new(MemoryContext {
        enabled,
        max_episodes: 64,
        refalsify,
        disabled_reason: None,
        store: Some(Mutex::new(store)),
    })
}

/// How many prompts the pager has been handed that carry a rendered memory
/// block — read from the PAGER's journal, the only place the daemon records
/// a prompt verbatim. This is what separates "the stamp claims an injection"
/// from "the model was shown the block".
pub fn memory_prompts(dir: &Path) -> usize {
    replay(&dir.join("pager.jsonl"))
        .unwrap()
        .into_iter()
        .filter(|e| {
            matches!(e, Event::InferStarted { prompt, .. }
                if prompt.contains("[memory: verified prior attempt]"))
        })
        .count()
}

pub fn meta() -> GgufMeta {
    GgufMeta {
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

pub fn mint_ids(events: &[Event]) -> Vec<(String, String)> {
    events
        .iter()
        .filter_map(|e| match e {
            Event::MemoryMint {
                task_id,
                episode_id,
                ..
            } => Some((task_id.clone(), episode_id.clone())),
            _ => None,
        })
        .collect()
}

pub fn poll_to_terminal(registry: &TaskRegistry, task_id: &str) -> TaskResult {
    let mut entry = registry.get(task_id).expect("entry exists immediately");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    while entry.status == TaskStatus::Running && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(10));
        entry = registry.get(task_id).expect("entry still exists");
    }
    assert_ne!(
        entry.status,
        TaskStatus::Running,
        "task {task_id} never reached a terminal status"
    );
    entry
}

pub fn scripted(text: &str) -> Reply {
    Reply {
        text: text.to_string(),
        prompt_tokens: Some(8),
        completion_tokens: Some(4),
        duration_ms: 1,
    }
}

pub fn spec_for(goal: &str, grant: &Grant, cwd: &Path) -> TaskSpec {
    TaskSpec {
        goal: goal.to_string(),
        grant: grant.clone(),
        budget_tokens: 1_000_000,
        max_steps: 8,
        cwd: cwd.to_path_buf(),
        patch_codec: PatchCodec::WholeFile,
        bounds: ExecBounds::default(),
        mutating_verbs: true,
        envelope: EnvelopeLens::V1,
        memory_block: None,
        window_ladder: false,
    }
}

pub fn store_path(dir: &Path) -> PathBuf {
    dir.join("memory").join("episodes.jsonl")
}
