//! `/v1` (OpenAI-compatible) integration tests: every request goes over a
//! real `TcpStream` (`tests/common::http`) against a `serve_fake()`-provided
//! ephemeral port, driving `Pager<FakeSubstrate>` end to end.
//!
//! The first three tests are the Task 15 brief's own, verbatim. The rest
//! pin the shim's remaining obligations: buffered SSE streaming (D3) and
//! `X-Bloomery-Agent` session binding (an existing agent's KV/context is
//! reused across two calls, proven via `FakeSubstrate::ctx_history`).

mod common;

use std::io::{Read, Write};

use common::http;

#[test]
fn chat_completion_has_real_usage() {
    let (port, _h) = bloomery_daemon::test_support::serve_fake();
    let addr = format!("127.0.0.1:{port}");
    let (st, body) = http(
        &addr,
        "POST",
        "/v1/chat/completions",
        r#"{"model":"qwen","messages":[{"role":"user","content":"hi"}],"max_tokens":16}"#,
    );
    assert_eq!(st, 200, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["object"], "chat.completion");
    assert!(v["usage"]["prompt_tokens"].as_u64().is_some());
    assert!(v["usage"]["completion_tokens"].as_u64().is_some());
    assert!(v["choices"][0]["message"]["content"].as_str().is_some());
}

#[test]
fn oversized_prompt_gets_honest_400_not_truncation() {
    let (port, _h) = bloomery_daemon::test_support::serve_fake();
    let addr = format!("127.0.0.1:{port}");
    let big = "x".repeat(100_000);
    let req = format!(
        r#"{{"model":"qwen","messages":[{{"role":"user","content":"{big}"}}],"max_tokens":16}}"#
    );
    let (st, body) = http(&addr, "POST", "/v1/chat/completions", &req);
    assert_eq!(st, 400, "{body}");
    assert!(body.contains("prompt_too_large"));
    assert!(body.contains("refusing rather than truncating"));
}

#[test]
fn models_lists_configured_models() {
    let (port, _h) = bloomery_daemon::test_support::serve_fake();
    let (st, body) = http(&format!("127.0.0.1:{port}"), "GET", "/v1/models", "");
    assert_eq!(st, 200);
    assert!(body.contains("qwen"));
}

/// The honest-refusal shape in full: `type`, `code`, `param`, and the exact
/// wording law 2 requires — this is the one place the whole envelope (not
/// just two substrings) is pinned.
#[test]
fn oversized_prompt_400_has_the_full_openai_error_envelope() {
    let (port, _h) = bloomery_daemon::test_support::serve_fake();
    let addr = format!("127.0.0.1:{port}");
    let big = "x".repeat(100_000);
    let req = format!(
        r#"{{"model":"qwen","messages":[{{"role":"user","content":"{big}"}}],"max_tokens":16}}"#
    );
    let (st, body) = http(&addr, "POST", "/v1/chat/completions", &req);
    assert_eq!(st, 400, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["error"]["type"], "invalid_request_error");
    assert_eq!(v["error"]["code"], "prompt_too_large");
    assert_eq!(v["error"]["param"], "messages");
    let msg = v["error"]["message"].as_str().unwrap();
    assert!(msg.contains("prompt needs"), "{msg}");
    assert!(msg.contains("window is"), "{msg}");
    assert!(msg.contains("refusing rather than truncating"), "{msg}");
}

/// `GET /v1/models` shapes each entry per the OpenAI list envelope, not
/// just "the name shows up somewhere in the body".
#[test]
fn models_list_has_the_openai_list_shape() {
    let (port, _h) = bloomery_daemon::test_support::serve_fake();
    let (st, body) = http(&format!("127.0.0.1:{port}"), "GET", "/v1/models", "");
    assert_eq!(st, 200, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["object"], "list");
    let entry = v["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["id"] == "qwen")
        .expect("qwen listed");
    assert_eq!(entry["object"], "model");
    assert_eq!(entry["owned_by"], "bloomery");
}

/// `stream:true` is buffered SSE (Phase 1's documented limit, D3): the wire
/// format is real SSE (`Content-Type: text/event-stream`, `data:` lines),
/// but the whole reply lands in one delta chunk before the terminal chunk
/// (carrying real `usage`) and `data: [DONE]`.
#[test]
fn streaming_chat_completion_is_valid_sse_ending_in_done() {
    let (port, _h) = bloomery_daemon::test_support::serve_fake();
    let addr = format!("127.0.0.1:{port}");
    let body = r#"{"model":"qwen","messages":[{"role":"user","content":"hi"}],"max_tokens":16,"stream":true}"#;

    let mut s = std::net::TcpStream::connect(&addr).unwrap();
    write!(
        s,
        "POST /v1/chat/completions HTTP/1.1\r\nHost: x\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
    .unwrap();
    let mut buf = String::new();
    s.read_to_string(&mut buf).unwrap();

    let status: u16 = buf.split_whitespace().nth(1).unwrap().parse().unwrap();
    assert_eq!(status, 200, "{buf}");
    assert!(
        buf.to_ascii_lowercase()
            .contains("content-type: text/event-stream"),
        "{buf}"
    );

    let wire_body = buf.split("\r\n\r\n").nth(1).unwrap_or("");
    let data_lines: Vec<&str> = wire_body
        .lines()
        .filter(|l| l.starts_with("data: "))
        .collect();
    assert!(
        data_lines.len() >= 2,
        "expected at least a delta chunk and [DONE]: {wire_body}"
    );
    assert_eq!(*data_lines.last().unwrap(), "data: [DONE]");

    // Every non-terminal `data:` line parses as JSON, and the final one
    // before [DONE] carries real usage.
    let json_lines: Vec<serde_json::Value> = data_lines[..data_lines.len() - 1]
        .iter()
        .map(|l| serde_json::from_str(l.trim_start_matches("data: ")).unwrap())
        .collect();
    assert!(json_lines
        .iter()
        .any(|v| v["choices"][0]["delta"]["content"].as_str().is_some()));
    let usage_chunk = json_lines
        .iter()
        .find(|v| v["usage"].is_object())
        .expect("final chunk carries usage");
    assert!(usage_chunk["usage"]["prompt_tokens"].as_u64().is_some());
    assert!(usage_chunk["usage"]["completion_tokens"].as_u64().is_some());
    assert!(usage_chunk["usage"]["total_tokens"].as_u64().is_some());
}

/// `X-Bloomery-Agent: <id>` binds two separate `/v1/chat/completions` calls,
/// over real HTTP, to the same underlying pager agent: its budget is
/// charged on *both* calls, which can only happen if the header correctly
/// routed each call to the pre-existing agent rather than each one minting
/// (and immediately removing) its own ephemeral agent — an ephemeral flow
/// would never touch this agent's budget at all.
#[test]
fn x_bloomery_agent_header_binds_two_calls_to_the_same_agent_over_http() {
    let (port, h) = bloomery_daemon::test_support::serve_fake();
    let addr = format!("127.0.0.1:{port}");

    let (st, body) = http(&addr, "POST", "/agents", r#"{"model":"qwen"}"#);
    assert_eq!(st, 201, "{body}");
    let id = serde_json::from_str::<serde_json::Value>(&body).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();

    let (st, body1) = http_with_agent(
        &addr,
        "/v1/chat/completions",
        r#"{"model":"qwen","messages":[{"role":"user","content":"first turn"}],"max_tokens":16}"#,
        &id,
    );
    assert_eq!(st, 200, "{body1}");

    let (st, body2) = http_with_agent(
        &addr,
        "/v1/chat/completions",
        r#"{"model":"qwen","messages":[{"role":"user","content":"second turn"}],"max_tokens":16}"#,
        &id,
    );
    assert_eq!(st, 200, "{body2}");

    let (st, status_body) = http(&addr, "GET", "/status", "");
    assert_eq!(st, 200, "{status_body}");
    let status: serde_json::Value = serde_json::from_str(&status_body).unwrap();
    let agent = status["agents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["id"] == id)
        .expect("session agent still exists (never ephemeral-removed)");
    assert_eq!(agent["state"], "resident");
    // Each scripted reply charges 8 prompt + 4 completion = 12 tokens; two
    // calls against the *same* agent charge its budget twice.
    assert_eq!(
        agent["budget_spent"], 24,
        "budget charged on both calls means both routed to this agent, {status_body}"
    );

    h.shutdown();
}

/// Sends one request carrying an `X-Bloomery-Agent` header. `http()` in
/// `tests/common` doesn't take extra headers, so this is a local variant
/// rather than widening that shared helper for one caller.
fn http_with_agent(addr: &str, path: &str, body: &str, agent_id: &str) -> (u16, String) {
    let mut s = std::net::TcpStream::connect(addr).unwrap();
    write!(
        s,
        "POST {path} HTTP/1.1\r\nHost: x\r\nContent-Type: application/json\r\nX-Bloomery-Agent: {agent_id}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
    .unwrap();
    let mut buf = String::new();
    s.read_to_string(&mut buf).unwrap();
    let status: u16 = buf.split_whitespace().nth(1).unwrap().parse().unwrap();
    let body = buf.split("\r\n\r\n").nth(1).unwrap_or("").to_string();
    (status, body)
}

/// The ground-truth version of the header-binding claim, driven in-process
/// (`test_support::dispatch_v1_fake`, no socket) so `FakeSubstrate` stays
/// reachable afterward: two `/v1/chat/completions` calls with the same
/// `X-Bloomery-Agent` land in the *same substrate context*, proven by
/// reading `FakeSubstrate::ctx_history` and finding both prompts in it —
/// not just "some agent got charged twice", but "the KV/context itself was
/// reused".
#[test]
fn x_bloomery_agent_header_reuses_the_same_substrate_context() {
    let (_dir, pager) = bloomery_daemon::test_support::fake_pager_for_v1();
    let id = {
        let mut p = pager.lock().unwrap();
        p.create_agent("qwen", 100, None, 200_000).unwrap().id
    };

    let (st, body1) = bloomery_daemon::test_support::dispatch_v1_fake(
        &pager,
        "POST",
        "/v1/chat/completions",
        r#"{"model":"qwen","messages":[{"role":"user","content":"first turn"}],"max_tokens":16}"#,
        Some(&id),
    );
    assert_eq!(st, 200, "{body1}");

    let (st, body2) = bloomery_daemon::test_support::dispatch_v1_fake(
        &pager,
        "POST",
        "/v1/chat/completions",
        r#"{"model":"qwen","messages":[{"role":"user","content":"second turn"}],"max_tokens":16}"#,
        Some(&id),
    );
    assert_eq!(st, 200, "{body2}");

    let p = pager.lock().unwrap();
    // A fresh fixture with exactly one agent that has inferred: the first
    // (and only) context `FakeSubstrate` ever hands out is handle 1.
    let history = p
        .substrate()
        .ctx_history(1)
        .expect("context 1 is still resident after two same-agent calls");
    assert!(history.contains("first turn"), "{history}");
    assert!(history.contains("second turn"), "{history}");
}

/// A non-streaming completion carries `X-Bloomery-Template: fallback` (D4):
/// Phase 1 has no model-native template to select between, and this header
/// says so honestly on every response rather than a route implying a
/// `model` template arm exists today.
#[test]
fn non_streaming_completion_reports_the_fallback_template() {
    let (port, _h) = bloomery_daemon::test_support::serve_fake();
    let addr = format!("127.0.0.1:{port}");
    let body = r#"{"model":"qwen","messages":[{"role":"user","content":"hi"}],"max_tokens":16}"#;

    let mut s = std::net::TcpStream::connect(&addr).unwrap();
    write!(
        s,
        "POST /v1/chat/completions HTTP/1.1\r\nHost: x\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
    .unwrap();
    let mut buf = String::new();
    s.read_to_string(&mut buf).unwrap();
    assert!(
        buf.to_ascii_lowercase()
            .contains("x-bloomery-template: fallback"),
        "{buf}"
    );
}

/// A header-less call gets an ephemeral agent that must not survive the
/// response (brief: "removed after the response") — several anonymous
/// calls in a row must never accumulate agents in `/status`.
#[test]
fn header_less_calls_leave_no_agent_behind() {
    let (port, h) = bloomery_daemon::test_support::serve_fake();
    let addr = format!("127.0.0.1:{port}");
    for _ in 0..3 {
        let (st, body) = http(
            &addr,
            "POST",
            "/v1/chat/completions",
            r#"{"model":"qwen","messages":[{"role":"user","content":"hi"}],"max_tokens":16}"#,
        );
        assert_eq!(st, 200, "{body}");
    }
    let (st, body) = http(&addr, "GET", "/status", "");
    assert_eq!(st, 200, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(
        v["agents"].as_array().unwrap().len(),
        0,
        "ephemeral agents must not leak: {body}"
    );
    h.shutdown();
}

/// A header naming an agent that was never created is a named `404`, not a
/// surprise ephemeral agent silently created under someone else's chosen
/// id — the shim never invents its own id namespace (see the module doc's
/// "Session binding" note).
#[test]
fn x_bloomery_agent_header_naming_an_unknown_agent_is_404() {
    let (port, h) = bloomery_daemon::test_support::serve_fake();
    let addr = format!("127.0.0.1:{port}");
    let (st, body) = http_with_agent(
        &addr,
        "/v1/chat/completions",
        r#"{"model":"qwen","messages":[{"role":"user","content":"hi"}],"max_tokens":16}"#,
        "does-not-exist",
    );
    assert_eq!(st, 404, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["error"]["type"], "invalid_request_error");
    assert_eq!(v["error"]["code"], "agent_not_found");
    h.shutdown();
}

/// A header-bound agent belongs to whatever model it was created with — a
/// request naming a *different* `model` is refused with `400
/// model_mismatch` rather than silently run against the bound agent's real
/// model while echoing back the caller's mismatched string. The same
/// agent with the matching model still succeeds normally.
#[test]
fn header_agent_model_mismatch_is_refused_matching_model_passes() {
    let (dir, pager) = bloomery_daemon::test_support::fake_pager_for_v1();
    let id = {
        let mut p = pager.lock().unwrap();
        // A second model so there's something for "qwen" to mismatch
        // against.
        let gguf = dir.join("other.gguf");
        std::fs::write(&gguf, b"other weights").unwrap();
        let meta = bloomery_core::gguf::GgufMeta {
            arch: "qwen2".into(),
            layers: 28,
            kv_heads: 4,
            head_dim: 128,
            training_ctx: 4096,
            weights_bytes: 1000,
        };
        p.register_model("other", &gguf, meta, None).unwrap();
        p.create_agent("qwen", 100, None, 200_000).unwrap().id
    };

    let (st, body) = bloomery_daemon::test_support::dispatch_v1_fake(
        &pager,
        "POST",
        "/v1/chat/completions",
        r#"{"model":"other","messages":[{"role":"user","content":"hi"}],"max_tokens":16}"#,
        Some(&id),
    );
    assert_eq!(st, 400, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["error"]["type"], "invalid_request_error");
    assert_eq!(v["error"]["code"], "model_mismatch");
    assert_eq!(v["error"]["param"], "model");
    let msg = v["error"]["message"].as_str().unwrap();
    assert!(msg.contains(&id), "{msg}");
    assert!(msg.contains("qwen"), "{msg}");
    assert!(msg.contains("other"), "{msg}");

    let (st, body) = bloomery_daemon::test_support::dispatch_v1_fake(
        &pager,
        "POST",
        "/v1/chat/completions",
        r#"{"model":"qwen","messages":[{"role":"user","content":"hi"}],"max_tokens":16}"#,
        Some(&id),
    );
    assert_eq!(st, 200, "matching model must still succeed: {body}");
}

/// A `Contract` violation (the substrate replied without token stats)
/// happens *after* the agent has gone `Resident` (`infer` pages it in
/// before calling the substrate) — the harder case for ephemeral cleanup
/// than a pre-residency refusal like `PromptTooLarge`/`Budget`. The
/// ephemeral agent must still be gone afterward, same as any other outcome.
#[test]
fn contract_violation_still_cleans_up_the_resident_ephemeral_agent() {
    let pager = pager_with_missing_stats_reply();

    let (st, body) = bloomery_daemon::test_support::dispatch_v1_fake(
        &pager,
        "POST",
        "/v1/chat/completions",
        r#"{"model":"qwen","messages":[{"role":"user","content":"hi"}],"max_tokens":16}"#,
        None,
    );
    assert_eq!(st, 500, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["error"]["type"], "server_error");
    assert_eq!(v["error"]["code"], "contract_violation");

    let p = pager.lock().unwrap();
    assert!(
        p.status().agents.is_empty(),
        "ephemeral agent must be cleaned up even after going Resident \
         and then hitting a Contract violation: {:?}",
        p.status()
    );
}

/// Builds a `Pager<FakeSubstrate>` with one `qwen`-like model and a single
/// scripted reply that omits `prompt_tokens` — the one substrate-side
/// condition `enforce_contract` classifies as `MissingStats`. Mirrors
/// `api_native_test.rs::serve_with_missing_stats`, but returns a bare
/// `Mutex<Pager<_>>` (no socket) so `dispatch_v1_fake` can drive it and the
/// pager stays inspectable afterward.
fn pager_with_missing_stats_reply(
) -> std::sync::Mutex<bloomery_daemon::pager::Pager<bloomery_substrate::fake::FakeSubstrate>> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "bloomery-v1-contract-test-{}-{seq}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("scratch dir");

    let journal = bloomery_core::journal::Journal::open(&dir.join("j.jsonl")).unwrap();
    let images = bloomery_daemon::agents::ImageStore::new(&dir.join("img")).unwrap();
    let mut fake = bloomery_substrate::fake::FakeSubstrate::new();
    fake.script_reply(bloomery_substrate::Reply {
        text: "no stats".into(),
        prompt_tokens: None,
        completion_tokens: Some(4),
        duration_ms: 1,
    });
    let mut pager = bloomery_daemon::pager::Pager::new(
        fake,
        journal,
        images,
        Box::new(|| Some(1024 * 1024 * 1024)),
    );
    let gguf = dir.join("qwen.gguf");
    std::fs::write(&gguf, b"weights").unwrap();
    let meta = bloomery_core::gguf::GgufMeta {
        arch: "qwen2".into(),
        layers: 28,
        kv_heads: 4,
        head_dim: 128,
        training_ctx: 4096,
        weights_bytes: 1000,
    };
    pager.register_model("qwen", &gguf, meta, None).unwrap();
    std::sync::Mutex::new(pager)
}

/// The `/v1` half of the same obligation: an ephemeral agent (no
/// `X-Bloomery-Agent` header) is minted at the pager's configured defaults,
/// so a `max_tokens` above a 5000-token configured budget is refused with
/// that budget's own arithmetic — proof the number came from config and not
/// from the 200 000 this layer used to hardcode.
#[test]
fn an_ephemeral_agent_is_minted_at_the_pagers_configured_budget() {
    let (dir, pager) = bloomery_daemon::test_support::fake_pager_for_v1();
    pager.lock().unwrap().set_defaults(7, 5000);

    let (st, body) = bloomery_daemon::test_support::dispatch_v1_fake(
        &pager,
        "POST",
        "/v1/chat/completions",
        r#"{"model":"qwen","messages":[{"role":"user","content":"hi"}],"max_tokens":5001}"#,
        None,
    );
    assert_eq!(st, 429, "{body}");
    assert!(body.contains("budget_exhausted"), "{body}");
    assert!(body.contains("5000 remaining"), "{body}");

    // The same call under the stock 200 000 default would have passed the
    // budget gate — the refusal is the config's, not this layer's.
    let (st, body) = bloomery_daemon::test_support::dispatch_v1_fake(
        &pager,
        "POST",
        "/v1/chat/completions",
        r#"{"model":"qwen","messages":[{"role":"user","content":"hi"}],"max_tokens":16}"#,
        None,
    );
    assert_eq!(st, 200, "{body}");
    let _ = std::fs::remove_dir_all(dir);
}
