//! Native HTTP API integration tests: every request goes over a real
//! `TcpStream` (`tests/common::http`) against a `serve_fake()`-provided
//! ephemeral port, driving `Pager<FakeSubstrate>` end to end.
//!
//! The first test is the Task 14 brief's own, verbatim (its `http()` helper
//! moved to `tests/common/mod.rs`, unchanged, so the rest of this file can
//! share it). The rest pin the error-code mapping table the brief's test
//! didn't cover: 402 (budget), 409 (residency refusal, with the exact
//! arithmetic), 404 (unknown agent), the suspend/resume 204 round-trip, and
//! the cold-switch bench's `unload` 204.

mod common;

use common::http;

#[test]
fn create_infer_and_refusal_over_http() {
    let (port, _handle) = bloomery_daemon::test_support::serve_fake(); // helper: pager on FakeSubstrate, scripted replies
    let addr = format!("127.0.0.1:{port}");
    let (st, body) = http(
        &addr,
        "POST",
        "/agents",
        r#"{"model":"qwen","budget_tokens":1000}"#,
    );
    assert_eq!(st, 201, "{body}");
    let id = serde_json::from_str::<serde_json::Value>(&body).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let (st, body) = http(
        &addr,
        "POST",
        &format!("/agents/{id}/infer"),
        r#"{"prompt":"hi","max_tokens":16}"#,
    );
    assert_eq!(st, 200, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert!(v["prompt_tokens"].as_u64().is_some());
    let big = format!(r#"{{"prompt":"{}","max_tokens":16}}"#, "x".repeat(100_000));
    let (st, body) = http(&addr, "POST", &format!("/agents/{id}/infer"), &big);
    assert_eq!(st, 413, "{body}");
    assert!(body.contains("prompt_too_large") && body.contains("window_tokens"));
}

/// 402: an agent's token budget grants less than a request asks for.
#[test]
fn infer_over_budget_returns_402_with_arithmetic() {
    let (port, handle) = bloomery_daemon::test_support::serve_fake();
    let addr = format!("127.0.0.1:{port}");
    let (st, body) = http(
        &addr,
        "POST",
        "/agents",
        r#"{"model":"qwen","budget_tokens":10}"#,
    );
    assert_eq!(st, 201, "{body}");
    let id = serde_json::from_str::<serde_json::Value>(&body).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();

    let (st, body) = http(
        &addr,
        "POST",
        &format!("/agents/{id}/infer"),
        r#"{"prompt":"hi","max_tokens":16}"#,
    );
    assert_eq!(st, 402, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["error"], "budget_exhausted");
    assert_eq!(v["remaining"], 10);
    assert_eq!(v["requested"], 16);
    handle.shutdown();
}

/// 409: residency refusal with the exact byte arithmetic, over HTTP.
///
/// Mirrors `pager_test.rs::residency_refusal_is_pre_checked_and_never_touches_the_substrate`
/// against `serve_fake()`'s own fixture geometry: qwen-like meta gives
/// 57 344 B/token, and the training-ctx-bound window (4096 tokens, since
/// the fixture's 1 GiB budget dwarfs the VRAM term) makes every agent need
/// 234 881 024 B once resident. Five same-priority agents is the smallest
/// number that overflows a 1 GiB budget at that size: four fit with
/// 134 217 728 B left over, and the fifth — no higher priority than any
/// resident, so nothing is evictable for it — cannot fit in what's left.
#[test]
fn infer_residency_refusal_returns_409_with_arithmetic() {
    const KV_PER_TOKEN: u64 = 57_344;
    const WINDOW_TOKENS: u64 = 4096;
    const PER_AGENT_KV_BYTES: u64 = WINDOW_TOKENS * KV_PER_TOKEN;
    const FREE_VRAM_BYTES: u64 = 1024 * 1024 * 1024;

    let (port, handle) = bloomery_daemon::test_support::serve_fake();
    let addr = format!("127.0.0.1:{port}");

    let mut ids = Vec::new();
    for _ in 0..5 {
        let (st, body) = http(&addr, "POST", "/agents", r#"{"model":"qwen"}"#);
        assert_eq!(st, 201, "{body}");
        let id = serde_json::from_str::<serde_json::Value>(&body).unwrap()["id"]
            .as_str()
            .unwrap()
            .to_string();
        ids.push(id);
    }

    for id in &ids[..4] {
        let (st, body) = http(
            &addr,
            "POST",
            &format!("/agents/{id}/infer"),
            r#"{"prompt":"hi","max_tokens":16}"#,
        );
        assert_eq!(st, 200, "{body}");
    }

    let (st, body) = http(
        &addr,
        "POST",
        &format!("/agents/{}/infer", ids[4]),
        r#"{"prompt":"hi","max_tokens":16}"#,
    );
    assert_eq!(st, 409, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["error"], "refused");
    assert_eq!(v["needed"], PER_AGENT_KV_BYTES);
    assert_eq!(v["free"], FREE_VRAM_BYTES - 4 * PER_AGENT_KV_BYTES);
    assert_eq!(
        v["reclaimable"], 0,
        "same-priority residents are never evictable"
    );
    handle.shutdown();
}

/// 404: an id that was never created.
#[test]
fn infer_on_unknown_agent_returns_404() {
    let (port, handle) = bloomery_daemon::test_support::serve_fake();
    let addr = format!("127.0.0.1:{port}");
    let (st, body) = http(
        &addr,
        "POST",
        "/agents/does-not-exist/infer",
        r#"{"prompt":"hi","max_tokens":16}"#,
    );
    assert_eq!(st, 404, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["error"], "unknown_agent");
    assert_eq!(v["agent"], "does-not-exist");
    handle.shutdown();
}

/// 204/204: suspend then resume round-trips an agent, and it is still
/// usable afterward.
#[test]
fn suspend_and_resume_round_trip_as_204s() {
    let (port, handle) = bloomery_daemon::test_support::serve_fake();
    let addr = format!("127.0.0.1:{port}");
    let (st, body) = http(&addr, "POST", "/agents", r#"{"model":"qwen"}"#);
    assert_eq!(st, 201, "{body}");
    let id = serde_json::from_str::<serde_json::Value>(&body).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();

    let (st, body) = http(
        &addr,
        "POST",
        &format!("/agents/{id}/infer"),
        r#"{"prompt":"hi","max_tokens":16}"#,
    );
    assert_eq!(st, 200, "must be resident to suspend: {body}");

    let (st, body) = http(&addr, "POST", &format!("/agents/{id}/suspend"), "");
    assert_eq!((st, body.as_str()), (204, ""));

    let (st, body) = http(&addr, "POST", &format!("/agents/{id}/resume"), "");
    assert_eq!((st, body.as_str()), (204, ""));

    let (st, body) = http(
        &addr,
        "POST",
        &format!("/agents/{id}/infer"),
        r#"{"prompt":"again","max_tokens":16}"#,
    );
    assert_eq!(st, 200, "resumed agent must still be usable: {body}");
    handle.shutdown();
}

/// 204: `unload` supports the cold-switch bench (Task 17) and is reflected
/// in `/status`.
#[test]
fn unload_model_returns_204_and_status_reflects_it() {
    let (port, handle) = bloomery_daemon::test_support::serve_fake();
    let addr = format!("127.0.0.1:{port}");
    let (st, body) = http(&addr, "POST", "/agents", r#"{"model":"qwen"}"#);
    assert_eq!(st, 201, "{body}");
    let id = serde_json::from_str::<serde_json::Value>(&body).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let (st, body) = http(
        &addr,
        "POST",
        &format!("/agents/{id}/infer"),
        r#"{"prompt":"hi","max_tokens":16}"#,
    );
    assert_eq!(st, 200, "must be loaded before it can be unloaded: {body}");

    let (st, body) = http(&addr, "POST", "/models/qwen/unload", "");
    assert_eq!((st, body.as_str()), (204, ""));

    let (st, body) = http(&addr, "GET", "/status", "");
    assert_eq!(st, 200, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    let qwen = v["models"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["name"] == "qwen")
        .expect("qwen is still registered after unload");
    assert_eq!(qwen["loaded"], false);
    handle.shutdown();
}
