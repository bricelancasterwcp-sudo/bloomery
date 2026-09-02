//! Fixtures shared by the `api_task_*` integration tests.
//!
//! Lifted out of `api_task_test.rs` when it was split (2026-09-01,
//! carried-debt slice D): everything here is reached by at least two of the
//! resulting files.

use super::http;

pub type FakePager = bloomery_daemon::pager::Pager<bloomery_substrate::fake::FakeSubstrate>;

/// Builds a `Pager<FakeSubstrate>` with one registered "qwen" model, tasks
/// enabled, and a pre-created `sandbox` dir — the same shape as
/// `test_support::serve_fake_with_tasks` — but serves it through
/// `http::serve_shared` and hands the caller back its own
/// `Arc<Mutex<Pager<FakeSubstrate>>>` handle instead of an opaque
/// `ServerHandle`-only fixture.
///
/// That handle is the only way any of these three tests can reach back into
/// `FakeSubstrate::ctx_history` (test (a)'s observable seam) or call
/// `attach_profile`/`set_codec_gate` on the model before creating the agent
/// (tests (a)/(c)): once a plain `Pager` is handed to `http::serve` (every
/// other fixture in this crate) it is moved behind the socket with no way
/// back out — `test_support.rs`'s own docs name this. The agent is
/// deliberately NOT created here — a caller configures the model first
/// (profile, codec gate) via the returned handle, then creates the agent
/// itself, so `create_agent`'s window/policy-quoting happens after that
/// configuration, not before it.
pub fn serve_codec_gate_fixture(
    replies: Vec<bloomery_substrate::Reply>,
) -> (
    u16,
    bloomery_daemon::http::ServerHandle,
    std::path::PathBuf,
    std::sync::Arc<std::sync::Mutex<FakePager>>,
) {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "bloomery-task-codec-gate-test-{}-{seq}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("scratch dir");

    let journal = bloomery_core::journal::Journal::open(&dir.join("j.jsonl")).unwrap();
    let images = bloomery_daemon::agents::ImageStore::new(&dir.join("img")).unwrap();
    let mut fake = bloomery_substrate::fake::FakeSubstrate::new();
    for r in replies {
        fake.script_reply(r);
    }
    let mut pager = bloomery_daemon::pager::Pager::new(
        fake,
        journal,
        images,
        Box::new(|| Some(1024 * 1024 * 1024)),
    );
    pager.set_tasks_enabled(true);
    pager.set_exec_bounds(bloomery_daemon::task::ExecBounds::default());
    pager.set_task_journal_path(dir.join("tasks.jsonl"));

    let gguf = dir.join("qwen.gguf");
    std::fs::write(&gguf, b"fake weights").unwrap();
    let meta = bloomery_core::gguf::GgufMeta {
        arch: "qwen2".into(),
        layers: 4,
        attention_layers: 4,
        kv_heads: 2,
        head_dim: 32,
        training_ctx: 65536,
        weights_bytes: 1000,
        value_length: None,
        recurrent_state_bytes: 0,
    };
    pager.register_model("qwen", &gguf, meta, None).unwrap();

    let sandbox = dir.join("sandbox");
    std::fs::create_dir_all(&sandbox).unwrap();
    let sandbox = std::fs::canonicalize(&sandbox).unwrap();

    let pager = std::sync::Arc::new(std::sync::Mutex::new(pager));
    let (port, mut handle) = bloomery_daemon::http::serve_shared(std::sync::Arc::clone(&pager), 0);
    handle.set_scratch_dir(dir);
    (port, handle, sandbox, pager)
}

/// Polls `GET /agents/{agent_id}/task/{task_id}` until `status` moves off
/// `"Running"` (bounded, mirroring every other poll loop in this file) and
/// returns the final response body.
pub fn wait_for_terminal(addr: &str, agent_id: &str, task_id: &str) -> String {
    let mut last_body = String::new();
    for _ in 0..200 {
        let (st, body) = http(
            addr,
            "GET",
            &format!("/agents/{agent_id}/task/{task_id}"),
            "",
        );
        assert_eq!(st, 200, "{body}");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        last_body = body;
        if v["status"].as_str().unwrap_or("Running") != "Running" {
            return last_body;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    panic!("task never reached a terminal state: {last_body}");
}

pub fn task_create_request(sandbox: &std::path::Path, goal: &str) -> String {
    serde_json::json!({
        "goal": goal,
        "grants": {
            "read_roots": [sandbox.to_string_lossy()],
            "write_roots": [sandbox.to_string_lossy()],
            "commands": [],
        },
    })
    .to_string()
}

pub fn done_reply(summary: &str) -> bloomery_substrate::Reply {
    bloomery_substrate::Reply {
        text: format!("<action verb=\"done\">\n{summary}\n</action>"),
        prompt_tokens: Some(8),
        completion_tokens: Some(4),
        duration_ms: 1,
    }
}
