//! Fixtures shared by the `task_loop_*` tests.
//!
//! Split out on 2026-09-01 (carried-debt slice D).

use bloomery_core::action::PatchCodec;
use bloomery_core::gguf::GgufMeta;
use bloomery_core::grant::Grant;
use bloomery_core::journal::Journal;
use bloomery_daemon::agents::ImageStore;
use bloomery_daemon::config::EnvelopeLens;
use bloomery_daemon::pager::Pager;
use bloomery_daemon::task::{ExecBounds, TaskSpec};
use bloomery_substrate::fake::FakeSubstrate;
use bloomery_substrate::Reply;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

pub fn bounds() -> ExecBounds {
    ExecBounds {
        read_cap_bytes: 256 * 1024,
        find_result_cap: 100,
        run_output_cap_bytes: 64 * 1024,
        run_timeout_secs: 120,
    }
}

pub fn meta() -> GgufMeta {
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

pub fn scripted(text: &str) -> Reply {
    Reply {
        text: text.to_string(),
        prompt_tokens: Some(8),
        completion_tokens: Some(4),
        duration_ms: 1,
    }
}

/// A fresh, per-test tempdir — PID + atomic counter, so parallel test
/// threads in one `cargo test` process never collide.
pub fn fresh_dir(tag: &str) -> PathBuf {
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
pub fn sandbox(dir: &std::path::Path) -> (PathBuf, Grant) {
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
pub fn fixture(
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

pub fn spec(grant: Grant, cwd: PathBuf, max_steps: u32) -> TaskSpec {
    demoted_spec(grant, cwd, max_steps, true)
}

/// Like [`spec`], but with an explicit `mutating_verbs` — the gate-G4
/// demotion tests need `false`.
pub fn demoted_spec(grant: Grant, cwd: PathBuf, max_steps: u32, mutating_verbs: bool) -> TaskSpec {
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
pub fn preseeded_spec(grant: Grant, cwd: PathBuf, max_steps: u32) -> TaskSpec {
    TaskSpec {
        envelope: EnvelopeLens::V2,
        ..demoted_spec(grant, cwd, max_steps, true)
    }
}
