//! Task HTTP surface integration tests (Phase 2b/2c P3 Task 5's binding
//! four): every request goes over a real `TcpStream`
//! (`tests/common::http`) against `test_support::serve_fake_with_tasks`,
//! the same pattern `api_native_test.rs` and `task_loop_test.rs` each use
//! half of.

mod common;

use bloomery_daemon::config::EnvelopeLens;
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
    assert_eq!(steps[0]["failed"], false);
    assert_eq!(steps[1]["verb"], "done");
    assert_eq!(steps[1]["failed"], false);
    // Window-ladder spec §6: each step object exposes the ladder rung its
    // prompt was actually sent at. 1 here, and for both reasons at once:
    // this request carries no `window_ladder` field (spec §5's absent →
    // off), and this task's prompts all fit anyway.
    assert_eq!(steps[0]["rung"], 1, "{last_body}");
    assert_eq!(steps[1]["rung"], 1, "{last_body}");
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

/// `422 budget_exceeds_grant` when a request's `budget_tokens` asks for more
/// than the agent's own granted budget — `run_task` never reads this field
/// back (only the pager's `Budget`, fixed at `create_agent` time, governs
/// spend), so a number above that ceiling could never be honored; this is
/// the review fix that catches the incoherent request rather than silently
/// accepting it.
#[test]
fn a_budget_tokens_above_the_agents_grant_is_422() {
    let (port, handle, sandbox) =
        bloomery_daemon::test_support::serve_fake_with_tasks(true, Vec::new());
    let sandbox = std::fs::canonicalize(&sandbox).unwrap();
    let addr = format!("127.0.0.1:{port}");

    // No `budget_tokens` in the create-agent body, so the agent's granted
    // budget is the daemon's configured default (200_000 for the fixture
    // pager — see `test_support::serve_fake_with_tasks`).
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
            "commands": [],
        },
        "budget_tokens": 999_999_999_u64,
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
    assert_eq!(v["error"], "budget_exceeds_grant");
    assert_eq!(v["requested"], 999_999_999_u64);
    assert_eq!(v["granted"], 200_000);

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
        _stop: Option<&str>,
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
        attention_layers: 1,
        kv_heads: 1,
        head_dim: 1,
        training_ctx: 65536,
        weights_bytes: 1,
        recurrent_state_bytes: 0,
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

// ---------------------------------------------------------------------------
// Task 8: `create_task` resolves the real per-model patch codec and G4 verb
// policy through `Pager::agent_task_policy` (closes the carried-debt item
// "Profile has NO codec field") instead of the `PatchCodec::SearchReplace` +
// `mutating_verbs: true` literal this task replaced.
// ---------------------------------------------------------------------------

/// A `Profile` whose `codecs` grid picks `WholeFile` over `SearchReplace`
/// for "qwen" — the same fixture `pager_codec_gate_test.rs` uses to prove
/// `Pager::model_patch_codec`'s selection (protocol §4).
const WF_WINS_PROFILE: &str = r#"{
  "assay_profile_version": 3,
  "probe_version": "0.4.1",
  "model": {"name": "qwen"},
  "codecs": {
    "search_replace": {"small": {"lands": 0.5, "lands_applies": 0.6, "n": 20}},
    "whole_file": {"small": {"lands": 0.8, "lands_applies": 0.9, "n": 20}}
  }
}"#;

/// The `WholeFile` codec's worked `patch` example, verbatim from
/// `bloomery_core::action::card`'s private `WHOLE_FILE_PATCH_EXAMPLE` — that
/// constant isn't `pub`, so this is the same bytes duplicated at the
/// boundary this test actually observes (the rendered prompt), the same way
/// `task_loop_test.rs` duplicates its own scripted `<action>` bodies rather
/// than reaching into `bloomery-core`'s private internals.
const WHOLE_FILE_PATCH_EXAMPLE: &str = "<action verb=\"patch\" path=\"src/lib.rs\">\nfn greeting() -> &'static str { \"hello\" }\n</action>";

/// The pinned gate-G4 refusal outcome (P4 Task 7 brief — exact bytes; Task
/// 9's scoring and the journal read this string). Duplicated locally for the
/// same reason `task_loop_test.rs` duplicates it rather than importing a
/// private `task_loop` constant.
const MUTATING_VERB_DEMOTED: &str = "verb unavailable: mutating verbs demoted (gate G4)";

type FakePager = bloomery_daemon::pager::Pager<bloomery_substrate::fake::FakeSubstrate>;

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
fn serve_codec_gate_fixture(
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
fn wait_for_terminal(addr: &str, agent_id: &str, task_id: &str) -> String {
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

fn task_create_request(sandbox: &std::path::Path, goal: &str) -> String {
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

fn done_reply(summary: &str) -> bloomery_substrate::Reply {
    bloomery_substrate::Reply {
        text: format!("<action verb=\"done\">\n{summary}\n</action>"),
        prompt_tokens: Some(8),
        completion_tokens: Some(4),
        duration_ms: 1,
    }
}

/// Test (a): a model with an attached wf-wins profile AND a stored keep gate
/// (mutating verbs on — otherwise the demoted read-only card would drop the
/// `patch` section entirely and there would be no patch example to select
/// between) gets tasks whose verb card shows the `WholeFile` patch example,
/// not the `SearchReplace` default `create_task` used to hardcode. Observed
/// via `FakeSubstrate::ctx_history` — the harness's existing seam
/// (`api_v1_test.rs::x_bloomery_agent_header_reuses_the_same_substrate_context`)
/// — since the rendered prompt is exactly what the model turn receives, sent
/// before the scripted reply even matters.
#[test]
fn a_wf_wins_profile_with_a_keep_gate_selects_the_whole_file_patch_example() {
    let (port, handle, sandbox, pager) = serve_codec_gate_fixture(vec![done_reply("ok")]);
    let addr = format!("127.0.0.1:{port}");

    let agent_id = {
        let mut p = pager.lock().unwrap();
        let profile = bloomery_core::profile::Profile::from_json(WF_WINS_PROFILE).unwrap();
        p.attach_profile("qwen", profile, false).unwrap();
        p.set_codec_gate(
            "qwen",
            bloomery_daemon::pager::CodecGateResult {
                fixture_set: "codec-tasks-v1".to_string(),
                codec: bloomery_core::action::PatchCodec::SearchReplace,
                landed: 17,
                n: 20,
                interval95: (0.60, 0.94),
                provisional: false,
                mutating_verbs: true,
            },
        )
        .unwrap();
        p.create_agent("qwen", 100, None, 1_000_000).unwrap().id
    };

    let (st, body) = http(
        &addr,
        "POST",
        &format!("/agents/{agent_id}/task"),
        &task_create_request(&sandbox, "say done"),
    );
    assert_eq!(st, 202, "{body}");
    let task_id = serde_json::from_str::<serde_json::Value>(&body).unwrap()["task_id"]
        .as_str()
        .unwrap()
        .to_string();

    let last_body = wait_for_terminal(&addr, &agent_id, &task_id);
    let v: serde_json::Value = serde_json::from_str(&last_body).unwrap();
    assert_eq!(v["status"], "Done", "{last_body}");

    let p = pager.lock().unwrap();
    let history = p
        .substrate()
        .ctx_history(1)
        .expect("context 1 is still resident after the task's only step");
    assert!(
        history.contains(WHOLE_FILE_PATCH_EXAMPLE),
        "expected the WholeFile patch example in the verb card the model was \
         prompted with, got: {history}"
    );
    assert!(
        !history.contains("<<<<<<< SEARCH"),
        "a WholeFile-selected card must never also carry the SearchReplace \
         conflict-marker example: {history}"
    );
    drop(p);

    handle.shutdown();
}

/// Test (b): an unmeasured model (no `set_codec_gate` call at all — protocol
/// §3/§6's fail-closed default) still gets a task created (`202`), but a
/// scripted `patch` turn records the pinned G4 refusal rather than executing
/// — proving the fail-closed default set at agent-admission time actually
/// reaches `run_task`'s dispatch gate through this HTTP route, not just
/// through `Pager::agent_task_policy` in isolation (`pager_codec_gate_test.rs`
/// already covers that half).
#[test]
fn an_unmeasured_model_is_created_but_its_patch_turn_is_refused_by_gate_g4() {
    let patch_attempt = bloomery_substrate::Reply {
        text: "<action verb=\"patch\" path=\"file.txt\">\n\
               <<<<<<< SEARCH\n\
               hello\n\
               =======\n\
               goodbye\n\
               >>>>>>> REPLACE\n\
               </action>"
            .to_string(),
        prompt_tokens: Some(8),
        completion_tokens: Some(4),
        duration_ms: 1,
    };
    let (port, handle, sandbox, pager) =
        serve_codec_gate_fixture(vec![patch_attempt, done_reply("refused as expected")]);
    std::fs::write(sandbox.join("file.txt"), "hello\nworld\n").unwrap();
    let addr = format!("127.0.0.1:{port}");

    // No `set_codec_gate` call at all — this model is unmeasured, which
    // `agent_task_policy` must resolve to `mutating_verbs: false`.
    let agent_id = {
        let mut p = pager.lock().unwrap();
        p.create_agent("qwen", 100, None, 1_000_000).unwrap().id
    };

    let (st, body) = http(
        &addr,
        "POST",
        &format!("/agents/{agent_id}/task"),
        &task_create_request(&sandbox, "patch the file"),
    );
    assert_eq!(st, 202, "{body}");
    let task_id = serde_json::from_str::<serde_json::Value>(&body).unwrap()["task_id"]
        .as_str()
        .unwrap()
        .to_string();

    let last_body = wait_for_terminal(&addr, &agent_id, &task_id);
    let v: serde_json::Value = serde_json::from_str(&last_body).unwrap();
    assert_eq!(
        v["status"], "Done",
        "a refused verb must not abort the task: {last_body}"
    );
    let steps = v["steps"].as_array().unwrap();
    assert_eq!(steps.len(), 2, "{last_body}");
    assert_eq!(steps[0]["verb"], "patch", "must record the real verb name");
    assert_eq!(steps[0]["failed"], true);
    assert_eq!(steps[0]["outcome"], MUTATING_VERB_DEMOTED);
    assert_eq!(steps[1]["verb"], "done");

    let on_disk = std::fs::read_to_string(sandbox.join("file.txt")).unwrap();
    assert_eq!(
        on_disk, "hello\nworld\n",
        "an unmeasured model's refused patch must never touch the file"
    );

    handle.shutdown();
}

/// Test (c): a model with a stored keep gate (`mutating_verbs: true`) gets a
/// patch turn that actually executes — the counterpart to test (b), proving
/// `create_task` reaches the real per-model verdict in both directions, not
/// just the fail-closed one.
#[test]
fn a_stored_keep_gate_lets_the_patch_turn_execute_for_real() {
    let patch_attempt = bloomery_substrate::Reply {
        text: "<action verb=\"patch\" path=\"file.txt\">\n\
               <<<<<<< SEARCH\n\
               hello\n\
               =======\n\
               goodbye\n\
               >>>>>>> REPLACE\n\
               </action>"
            .to_string(),
        prompt_tokens: Some(8),
        completion_tokens: Some(4),
        duration_ms: 1,
    };
    let (port, handle, sandbox, pager) =
        serve_codec_gate_fixture(vec![patch_attempt, done_reply("patched it")]);
    std::fs::write(sandbox.join("file.txt"), "hello\nworld\n").unwrap();
    let addr = format!("127.0.0.1:{port}");

    let agent_id = {
        let mut p = pager.lock().unwrap();
        p.set_codec_gate(
            "qwen",
            bloomery_daemon::pager::CodecGateResult {
                fixture_set: "codec-tasks-v1".to_string(),
                codec: bloomery_core::action::PatchCodec::SearchReplace,
                landed: 17,
                n: 20,
                interval95: (0.60, 0.94),
                provisional: false,
                mutating_verbs: true,
            },
        )
        .unwrap();
        p.create_agent("qwen", 100, None, 1_000_000).unwrap().id
    };

    let (st, body) = http(
        &addr,
        "POST",
        &format!("/agents/{agent_id}/task"),
        &task_create_request(&sandbox, "patch the file"),
    );
    assert_eq!(st, 202, "{body}");
    let task_id = serde_json::from_str::<serde_json::Value>(&body).unwrap()["task_id"]
        .as_str()
        .unwrap()
        .to_string();

    let last_body = wait_for_terminal(&addr, &agent_id, &task_id);
    let v: serde_json::Value = serde_json::from_str(&last_body).unwrap();
    assert_eq!(v["status"], "Done", "{last_body}");
    let steps = v["steps"].as_array().unwrap();
    assert_eq!(steps.len(), 2, "{last_body}");
    assert_eq!(steps[0]["verb"], "patch");
    assert_eq!(steps[0]["failed"], false, "{last_body}");
    assert!(
        steps[0]["outcome"]
            .as_str()
            .unwrap_or_default()
            .starts_with("patched (lens:"),
        "{last_body}"
    );

    let on_disk = std::fs::read_to_string(sandbox.join("file.txt")).unwrap();
    assert_eq!(
        on_disk, "goodbye\nworld\n",
        "a keep-gated model's patch must actually land"
    );

    handle.shutdown();
}

// ---------------------------------------------------------------------------
// Protocol §10, Amendment 2: envelope-v2 (think-preseeded) tasks over HTTP
// ---------------------------------------------------------------------------

/// Test (d): a model configured `think_preseed = true`
/// (`Pager::set_think_preseed`) gets its HTTP-created tasks rendering with
/// the literal pre-seed appended — resolved through the same
/// `Pager::agent_task_policy` one-source triple `patch_codec`/
/// `mutating_verbs`/`think_preseed` already flows through `create_task`
/// (closing the same "one policy source" rule test (a) proved for
/// `patch_codec`, now for the third field). Observed the same way test (a)
/// does: `FakeSubstrate::ctx_history` holds exactly the one rendered prompt
/// the model was sent, since this task's only scripted turn is `done`.
#[test]
fn a_think_preseed_model_renders_its_task_prompt_with_the_preseed_literal() {
    let (port, handle, sandbox, pager) = serve_codec_gate_fixture(vec![done_reply("ok")]);
    let addr = format!("127.0.0.1:{port}");

    let agent_id = {
        let mut p = pager.lock().unwrap();
        p.set_model_envelope("qwen", EnvelopeLens::V2).unwrap();
        p.create_agent("qwen", 100, None, 1_000_000).unwrap().id
    };

    let (st, body) = http(
        &addr,
        "POST",
        &format!("/agents/{agent_id}/task"),
        &task_create_request(&sandbox, "say done"),
    );
    assert_eq!(st, 202, "{body}");
    let task_id = serde_json::from_str::<serde_json::Value>(&body).unwrap()["task_id"]
        .as_str()
        .unwrap()
        .to_string();

    let last_body = wait_for_terminal(&addr, &agent_id, &task_id);
    let v: serde_json::Value = serde_json::from_str(&last_body).unwrap();
    assert_eq!(v["status"], "Done", "{last_body}");

    let p = pager.lock().unwrap();
    let history = p
        .substrate()
        .ctx_history(1)
        .expect("context 1 is still resident after the task's only step");
    assert!(
        history.ends_with("<think>\n\n</think>\n\n"),
        "expected the rendered prompt to end with the think-preseed literal, \
         got: {history}"
    );
    drop(p);

    handle.shutdown();
}

/// The counterpart: a model with no `think_preseed` configured (the
/// default, `false`) never carries the literal — `create_task`'s policy
/// triple resolves the flag off just as reliably as it resolves it on.
#[test]
fn a_non_preseeded_model_never_renders_the_preseed_literal_over_http() {
    let (port, handle, sandbox, pager) = serve_codec_gate_fixture(vec![done_reply("ok")]);
    let addr = format!("127.0.0.1:{port}");

    let agent_id = {
        let mut p = pager.lock().unwrap();
        p.create_agent("qwen", 100, None, 1_000_000).unwrap().id
    };

    let (st, body) = http(
        &addr,
        "POST",
        &format!("/agents/{agent_id}/task"),
        &task_create_request(&sandbox, "say done"),
    );
    assert_eq!(st, 202, "{body}");
    let task_id = serde_json::from_str::<serde_json::Value>(&body).unwrap()["task_id"]
        .as_str()
        .unwrap()
        .to_string();

    let last_body = wait_for_terminal(&addr, &agent_id, &task_id);
    let v: serde_json::Value = serde_json::from_str(&last_body).unwrap();
    assert_eq!(v["status"], "Done", "{last_body}");

    let p = pager.lock().unwrap();
    let history = p
        .substrate()
        .ctx_history(1)
        .expect("context 1 is still resident after the task's only step");
    assert!(
        !history.contains("<think>\n\n</think>\n\n"),
        "a non-preseeded model must never carry the literal, got: {history}"
    );
    drop(p);

    handle.shutdown();
}

// ---------------------------------------------------------------------------
// Window ladder (docs/superpowers/specs/2026-08-27-window-ladder-design.md
// §5): the REQUEST half of the wire. The ladder's own behavior — which rung
// a refused prompt lands on, what it elides, when it stays terminal — is
// owned by `task_ladder_test.rs` against `run_task` directly; these two pin
// only that `"window_ladder"` is a real, typed field of the create-task
// request. The RESPONSE half (§6: every step object carries its `rung`) is
// pinned above, inside `a_task_runs_and_is_pollable_to_done`, where the
// ladder-off default it asserts is the same default that test's request
// already exercised — a second create-and-poll would re-assert it verbatim.
// ---------------------------------------------------------------------------

/// Spec §5: a live task opts in over HTTP. The field parses, the request is
/// accepted (`202`), and the task still runs to `Done` — an opt-in whose
/// prompts all fit is byte-identical work at rung 1, which its step row then
/// reports (spec §2: rung 1 IS today's rendering).
#[test]
fn create_task_accepts_window_ladder_true() {
    let (port, handle, sandbox) =
        bloomery_daemon::test_support::serve_fake_with_tasks(true, vec![done_reply("ladder on")]);
    let sandbox = std::fs::canonicalize(&sandbox).unwrap();
    let addr = format!("127.0.0.1:{port}");

    let (st, body) = http(&addr, "POST", "/agents", r#"{"model":"qwen"}"#);
    assert_eq!(st, 201, "{body}");
    let agent_id = serde_json::from_str::<serde_json::Value>(&body).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();

    let task_req = serde_json::json!({
        "goal": "say done",
        "grants": {
            "read_roots": [sandbox.to_string_lossy()],
            "write_roots": [sandbox.to_string_lossy()],
            "commands": [],
        },
        "window_ladder": true,
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

    let last_body = wait_for_terminal(&addr, &agent_id, &task_id);
    let v: serde_json::Value = serde_json::from_str(&last_body).unwrap();
    assert_eq!(v["status"], "Done", "{last_body}");
    assert_eq!(v["summary"], "ladder on", "{last_body}");
    let steps = v["steps"].as_array().unwrap();
    assert_eq!(steps.len(), 1, "{last_body}");
    assert_eq!(steps[0]["verb"], "done");
    assert_eq!(
        steps[0]["rung"], 1,
        "opting in must not degrade a prompt that already fits: {last_body}"
    );

    handle.shutdown();
}

/// The companion that keeps the test above from being vacuous: `CreateTaskReq`
/// declares no `#[serde(deny_unknown_fields)]`, so a `202` alone would be
/// just as green if `window_ladder` were never a field at all and serde
/// silently dropped it. A non-boolean value is refused with this route's one
/// `400 bad_request` shape — which only a really-declared `bool` field can
/// produce, making this the assertion that fails if the request wiring is
/// ever removed.
#[test]
fn a_non_boolean_window_ladder_is_400() {
    let (port, handle, sandbox) =
        bloomery_daemon::test_support::serve_fake_with_tasks(true, vec![]);
    let sandbox = std::fs::canonicalize(&sandbox).unwrap();
    let addr = format!("127.0.0.1:{port}");

    // A real agent, so the refusal below can only be the field's type: an
    // otherwise-identical body with `true` is the `202` above.
    let (st, body) = http(&addr, "POST", "/agents", r#"{"model":"qwen"}"#);
    assert_eq!(st, 201, "{body}");
    let agent_id = serde_json::from_str::<serde_json::Value>(&body).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();

    let task_req = serde_json::json!({
        "goal": "say done",
        "grants": {
            "read_roots": [sandbox.to_string_lossy()],
            "write_roots": [sandbox.to_string_lossy()],
            "commands": [],
        },
        "window_ladder": "yes",
    })
    .to_string();

    let (st, body) = http(
        &addr,
        "POST",
        &format!("/agents/{agent_id}/task"),
        &task_req,
    );
    assert_eq!(st, 400, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["error"], "bad_request");
    assert!(
        v["message"]
            .as_str()
            .unwrap_or_default()
            .contains("boolean"),
        "the parse must have failed on the bool field itself: {body}"
    );

    handle.shutdown();
}
