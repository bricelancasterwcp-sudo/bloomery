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
