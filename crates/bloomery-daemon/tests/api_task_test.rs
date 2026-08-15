//! Task HTTP surface integration tests (Phase 2b/2c P3 Task 5's binding
//! four): every request goes over a real `TcpStream`
//! (`tests/common::http`) against `test_support::serve_fake_with_tasks`,
//! the same pattern `api_native_test.rs` and `task_loop_test.rs` each use
//! half of.

mod common;

use common::http;

/// `501 tasks_disabled` when the daemon's config never turned the task
/// surface on — the dark-by-default gate, checked before anything else in
/// the request (a malformed body still reads as "tasks are off", not "your
/// JSON was bad").
#[test]
fn task_endpoint_is_501_when_disabled() {
    let (port, handle, _sandbox) =
        bloomery_daemon::test_support::serve_fake_with_tasks(false, Vec::new());
    let addr = format!("127.0.0.1:{port}");

    let (st, body) = http(
        &addr,
        "POST",
        "/agents/does-not-matter/task",
        r#"{"goal":"do something","grants":{"read_roots":[],"write_roots":[],"commands":[]}}"#,
    );
    assert_eq!(st, 501, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["error"], "tasks_disabled");

    handle.shutdown();
}

/// A real task, over HTTP end to end: `POST` spawns a background worker
/// (`202` with a `task_id`), and polling `GET` sees it move from `Running`
/// to `Done` with its steps recorded — the scripted model reads a file in
/// its sandbox, then says `done`.
#[test]
fn a_task_runs_and_is_pollable_to_done() {
    let replies = vec![
        bloomery_substrate::Reply {
            text: "<action verb=\"read\" path=\"file.txt\">\n</action>".to_string(),
            prompt_tokens: Some(8),
            completion_tokens: Some(4),
            duration_ms: 1,
        },
        bloomery_substrate::Reply {
            text: "<action verb=\"done\">\nread the file\n</action>".to_string(),
            prompt_tokens: Some(8),
            completion_tokens: Some(4),
            duration_ms: 1,
        },
    ];
    let (port, handle, sandbox) =
        bloomery_daemon::test_support::serve_fake_with_tasks(true, replies);
    std::fs::write(sandbox.join("file.txt"), "hello\nworld\n").unwrap();
    let sandbox = std::fs::canonicalize(&sandbox).unwrap();
    let addr = format!("127.0.0.1:{port}");

    let (st, body) = http(&addr, "POST", "/agents", r#"{"model":"qwen"}"#);
    assert_eq!(st, 201, "{body}");
    let agent_id = serde_json::from_str::<serde_json::Value>(&body).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();

    let grants = serde_json::json!({
        "read_roots": [sandbox.to_string_lossy()],
        "write_roots": [sandbox.to_string_lossy()],
        "commands": [],
    });
    let task_req = serde_json::json!({
        "goal": "read the file and say done",
        "grants": grants,
    })
    .to_string();

    let (st, body) = http(
        &addr,
        "POST",
        &format!("/agents/{agent_id}/task"),
        &task_req,
    );
    assert_eq!(st, 202, "{body}");
    let task_id = serde_json::from_str::<serde_json::Value>(&body).unwrap()["task_id"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(task_id.starts_with("task-"), "{task_id}");

    let mut status = String::new();
    let mut last_body = String::new();
    for _ in 0..200 {
        let (st, body) = http(
            &addr,
            "GET",
            &format!("/agents/{agent_id}/task/{task_id}"),
            "",
        );
        assert_eq!(st, 200, "{body}");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        status = v["status"].as_str().unwrap().to_string();
        last_body = body;
        if status != "Running" {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert_eq!(status, "Done", "task never reached Done: {last_body}");

    let v: serde_json::Value = serde_json::from_str(&last_body).unwrap();
    let steps = v["steps"].as_array().unwrap();
    assert_eq!(steps.len(), 2, "{last_body}");
    assert_eq!(steps[0]["verb"], "read");
    assert_eq!(steps[1]["verb"], "done");
    assert_eq!(v["summary"], "read the file");

    handle.shutdown();
}

/// `422 invalid_grant` when the grant JSON fails P2's seal — an empty
/// command prefix, the same shape `bloomery_core::grant`'s own red-team
/// suite pins as `GrantError::EmptyCommandPrefix`.
#[test]
fn an_invalid_grant_is_422() {
    let (port, handle, sandbox) =
        bloomery_daemon::test_support::serve_fake_with_tasks(true, Vec::new());
    let sandbox = std::fs::canonicalize(&sandbox).unwrap();
    let addr = format!("127.0.0.1:{port}");

    let (st, body) = http(&addr, "POST", "/agents", r#"{"model":"qwen"}"#);
    assert_eq!(st, 201, "{body}");
    let agent_id = serde_json::from_str::<serde_json::Value>(&body).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();

    let task_req = serde_json::json!({
        "goal": "do something",
        "grants": {
            "read_roots": [sandbox.to_string_lossy()],
            "write_roots": [sandbox.to_string_lossy()],
            "commands": [[]],
        },
    })
    .to_string();

    let (st, body) = http(
        &addr,
        "POST",
        &format!("/agents/{agent_id}/task"),
        &task_req,
    );
    assert_eq!(st, 422, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["error"], "invalid_grant");
    assert!(v["detail"].as_str().is_some(), "{body}");

    handle.shutdown();
}

/// `404` for a task request against an agent id nobody created.
#[test]
fn unknown_agent_is_404() {
    let (port, handle, sandbox) =
        bloomery_daemon::test_support::serve_fake_with_tasks(true, Vec::new());
    let sandbox = std::fs::canonicalize(&sandbox).unwrap();
    let addr = format!("127.0.0.1:{port}");

    let task_req = serde_json::json!({
        "goal": "do something",
        "grants": {
            "read_roots": [sandbox.to_string_lossy()],
            "write_roots": [sandbox.to_string_lossy()],
            "commands": [],
        },
    })
    .to_string();

    let (st, body) = http(&addr, "POST", "/agents/does-not-exist/task", &task_req);
    assert_eq!(st, 404, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["error"], "unknown_agent");
    assert_eq!(v["agent"], "does-not-exist");

    handle.shutdown();
}

/// `GET` on a task id nobody created (regardless of `tasks_enabled`) is a
/// plain `404`, not a poll that hangs or a `501`.
#[test]
fn unknown_task_id_is_404() {
    let (port, handle, _sandbox) =
        bloomery_daemon::test_support::serve_fake_with_tasks(true, Vec::new());
    let addr = format!("127.0.0.1:{port}");

    let (st, body) = http(&addr, "GET", "/agents/a1/task/task-999", "");
    assert_eq!(st, 404, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["error"], "not_found");

    handle.shutdown();
}

// ---------------------------------------------------------------------------
// Regression: a panicking task worker must not wedge its own poll or take
// the daemon down for everyone else (review fix — `TaskRegistry::spawn_task`
// now wraps `run_task` in `catch_unwind` inside the pager-lock scope).
// ---------------------------------------------------------------------------

/// A `Substrate` whose `infer` always panics. Exists only to prove the
/// registry's `catch_unwind` fix end to end, over real HTTP — mirrors
/// `api_native_test.rs`'s `PanicSubstrate` (same shape, same reasoning: only
/// `infer` panics, everything else is a trivial success). Built directly in
/// this test file rather than added to `test_support.rs`, the same call
/// `api_native_test.rs::serve_panicking` already made for the same reason:
/// this is a narrow, one-off fixture, not a shape `serve_fake*` should
/// carry.
struct PanicSubstrate;

impl bloomery_substrate::Substrate for PanicSubstrate {
    fn load_model(
        &mut self,
        _path: &std::path::Path,
        _n_gpu_layers: u32,
    ) -> Result<bloomery_substrate::ModelHandle, bloomery_substrate::SubstrateError> {
        Ok(1)
    }

    fn unload_model(
        &mut self,
        _m: bloomery_substrate::ModelHandle,
    ) -> Result<(), bloomery_substrate::SubstrateError> {
        Ok(())
    }

    fn create_context(
        &mut self,
        _m: bloomery_substrate::ModelHandle,
        _n_ctx: u32,
    ) -> Result<bloomery_substrate::CtxHandle, bloomery_substrate::SubstrateError> {
        Ok(1)
    }

    fn destroy_context(
        &mut self,
        _c: bloomery_substrate::CtxHandle,
    ) -> Result<(), bloomery_substrate::SubstrateError> {
        Ok(())
    }

    fn infer(
        &mut self,
        _c: bloomery_substrate::CtxHandle,
        _prompt: &str,
        _max_tokens: u32,
    ) -> Result<bloomery_substrate::Reply, bloomery_substrate::SubstrateError> {
        panic!("scripted panic: proves the task registry's catch_unwind keeps the daemon healthy");
    }

    fn save_state(
        &mut self,
        _c: bloomery_substrate::CtxHandle,
    ) -> Result<Vec<u8>, bloomery_substrate::SubstrateError> {
        Ok(Vec::new())
    }

    fn load_state(
        &mut self,
        _c: bloomery_substrate::CtxHandle,
        _bytes: &[u8],
    ) -> Result<(), bloomery_substrate::SubstrateError> {
        Ok(())
    }
}

/// Builds and serves a `Pager<PanicSubstrate>` with `tasks_enabled` on, one
/// registered model, one created agent, and a pre-created sandbox dir for a
/// grant to scope to — ready to have a task's first step panic.
fn serve_panicking_task() -> (
    u16,
    bloomery_daemon::http::ServerHandle,
    String,
    std::path::PathBuf,
) {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "bloomery-task-panic-test-{}-{seq}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("scratch dir");

    let journal = bloomery_core::journal::Journal::open(&dir.join("j.jsonl")).unwrap();
    let images = bloomery_daemon::agents::ImageStore::new(&dir.join("img")).unwrap();
    let mut pager = bloomery_daemon::pager::Pager::new(
        PanicSubstrate,
        journal,
        images,
        Box::new(|| Some(1024 * 1024 * 1024)),
    );
    pager.set_tasks_enabled(true);
    pager.set_exec_bounds(bloomery_daemon::task::ExecBounds::default());
    pager.set_task_journal_path(dir.join("tasks.jsonl"));

    let gguf = dir.join("panic.gguf");
    std::fs::write(&gguf, b"weights").unwrap();
    let meta = bloomery_core::gguf::GgufMeta {
        arch: "qwen2".into(),
        layers: 1,
        kv_heads: 1,
        head_dim: 1,
        training_ctx: 65536,
        weights_bytes: 1,
    };
    pager
        .register_model("panic-model", &gguf, meta, None)
        .unwrap();
    let info = pager
        .create_agent("panic-model", 100, None, 1_000_000)
        .unwrap();

    let sandbox = dir.join("sandbox");
    std::fs::create_dir_all(&sandbox).unwrap();

    let (port, mut handle) = bloomery_daemon::http::serve(pager, 0);
    handle.set_scratch_dir(dir);
    (port, handle, info.id, sandbox)
}

/// The regression this fix closes, over real HTTP: a task whose first step
/// panics still reaches a polled `Error` (never stuck `Running` forever),
/// AND a completely unrelated, ordinary request against the *same* daemon
/// afterward still succeeds — proving the caught panic did not poison the
/// shared pager mutex and take the whole daemon down with it.
#[test]
fn a_panicking_task_step_becomes_error_and_the_daemon_stays_healthy() {
    let (port, handle, agent_id, sandbox) = serve_panicking_task();
    let sandbox = std::fs::canonicalize(&sandbox).unwrap();
    let addr = format!("127.0.0.1:{port}");

    let task_req = serde_json::json!({
        "goal": "trigger a panic",
        "grants": {
            "read_roots": [sandbox.to_string_lossy()],
            "write_roots": [sandbox.to_string_lossy()],
            "commands": [],
        },
    })
    .to_string();

    let (st, body) = http(
        &addr,
        "POST",
        &format!("/agents/{agent_id}/task"),
        &task_req,
    );
    assert_eq!(st, 202, "{body}");
    let task_id = serde_json::from_str::<serde_json::Value>(&body).unwrap()["task_id"]
        .as_str()
        .unwrap()
        .to_string();

    let mut status = String::new();
    let mut last_body = String::new();
    for _ in 0..200 {
        let (st, body) = http(
            &addr,
            "GET",
            &format!("/agents/{agent_id}/task/{task_id}"),
            "",
        );
        assert_eq!(st, 200, "{body}");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        status = v["status"].as_str().unwrap().to_string();
        last_body = body;
        if status != "Running" {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert_eq!(status, "Error", "task never reached Error: {last_body}");
    let v: serde_json::Value = serde_json::from_str(&last_body).unwrap();
    assert!(
        v["summary"]
            .as_str()
            .unwrap_or_default()
            .contains("panicked"),
        "{last_body}"
    );

    // The whole point: a caught worker panic must not poison the shared
    // pager mutex. An ordinary, unrelated request against the same daemon
    // must still succeed — not the sticky `500` `lock_pager` would return
    // if the mutex really had been poisoned.
    let (st, body) = http(&addr, "GET", "/status", "");
    assert_eq!(
        st, 200,
        "daemon degraded after a caught worker panic: {body}"
    );

    handle.shutdown();
}
