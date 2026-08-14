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

/// 404: creating an agent against a model that was never registered.
#[test]
fn create_agent_with_unknown_model_returns_404() {
    let (port, handle) = bloomery_daemon::test_support::serve_fake();
    let addr = format!("127.0.0.1:{port}");
    let (st, body) = http(&addr, "POST", "/agents", r#"{"model":"does-not-exist"}"#);
    assert_eq!(st, 404, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["error"], "unknown_model");
    assert_eq!(v["model"], "does-not-exist");
    handle.shutdown();
}

/// 400: a body that isn't valid JSON gets a named error, not a panic or a
/// route-level 5xx.
#[test]
fn malformed_json_body_returns_400() {
    let (port, handle) = bloomery_daemon::test_support::serve_fake();
    let addr = format!("127.0.0.1:{port}");
    let (st, body) = http(&addr, "POST", "/agents", "{not valid json");
    assert_eq!(st, 400, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["error"], "bad_request");
    assert!(v["message"].as_str().is_some(), "{body}");
    handle.shutdown();
}

/// 413: a body over `http::MAX_BODY_BYTES` (1 MiB) is refused before it
/// ever reaches route handling, regardless of what `Content-Length`
/// claimed.
#[test]
fn oversized_body_returns_413() {
    let (port, handle) = bloomery_daemon::test_support::serve_fake();
    let addr = format!("127.0.0.1:{port}");
    let huge = format!(
        r#"{{"model":"qwen","padding":"{}"}}"#,
        "x".repeat(2 * 1024 * 1024)
    );
    let (st, body) = http(&addr, "POST", "/agents", &huge);
    assert_eq!(st, 413, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["error"], "body_too_large");
    assert_eq!(v["max_bytes"], 1_048_576);
    handle.shutdown();
}

/// 502: a substrate reply that omits token stats is an infrastructure
/// failure (project law 4), never a model failure — and its `kind` matches
/// the journal's own spelling ("MissingStats") rather than a HTTP-layer
/// paraphrase, so the two are grep-able as the same fact.
#[test]
fn infer_with_missing_stats_returns_502_with_journal_spelling() {
    let (port, handle, id) = serve_with_missing_stats();
    let addr = format!("127.0.0.1:{port}");
    let (st, body) = http(
        &addr,
        "POST",
        &format!("/agents/{id}/infer"),
        r#"{"prompt":"hi","max_tokens":16}"#,
    );
    assert_eq!(st, 502, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["error"], "contract_violation");
    assert_eq!(v["kind"], "MissingStats");
    assert!(v["detail"].as_str().is_some(), "{body}");
    handle.shutdown();
}

/// Builds and serves a `Pager<FakeSubstrate>` with one scripted reply that
/// omits `prompt_tokens` — the one substrate-side condition that trips
/// `enforce_contract`'s `MissingStats` violation. Separate from
/// `test_support::serve_fake` (whose script is all clean replies) but built
/// from the same public pieces.
fn serve_with_missing_stats() -> (u16, bloomery_daemon::http::ServerHandle, String) {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "bloomery-contract-test-{}-{seq}",
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
    let info = pager.create_agent("qwen", 100, None, 200_000).unwrap();

    let (port, mut handle) = bloomery_daemon::http::serve(pager, 0);
    handle.set_scratch_dir(dir);
    (port, handle, info.id)
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

// ---------------------------------------------------------------------------
// Mutex-poisoning: a panicking request must degrade to named 500s, not a
// panic cascade or a hang.
// ---------------------------------------------------------------------------

/// A `Substrate` whose `infer` always panics. Exists only to poison the
/// pager's mutex on purpose, so `api_native::lock_pager`'s poison handling
/// can be proven end to end over real HTTP rather than left as
/// inspection-only. Every other method is a trivial success — the test
/// doesn't care about their behavior, only about reaching `infer`.
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
        panic!("scripted panic: poisons the pager mutex for a test");
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

/// Builds and serves a `Pager<PanicSubstrate>` with one registered model and
/// one created agent, ready to have its `infer` route panic on command.
/// Deliberately not routed through `test_support::serve_fake` (that's tied
/// to `FakeSubstrate`) — built directly from the same public pieces
/// `test_support` itself uses (`Pager::new`, `ImageStore::new`,
/// `Journal::open`, `register_model`, `create_agent`), all reachable from an
/// integration test without any test-only feature.
fn serve_panicking() -> (u16, bloomery_daemon::http::ServerHandle, String) {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let dir =
        std::env::temp_dir().join(format!("bloomery-poison-test-{}-{seq}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("scratch dir");

    let journal = bloomery_core::journal::Journal::open(&dir.join("j.jsonl")).unwrap();
    let images = bloomery_daemon::agents::ImageStore::new(&dir.join("img")).unwrap();
    let mut pager = bloomery_daemon::pager::Pager::new(
        PanicSubstrate,
        journal,
        images,
        Box::new(|| Some(1024 * 1024 * 1024)),
    );
    let gguf = dir.join("panic.gguf");
    std::fs::write(&gguf, b"weights").unwrap();
    let meta = bloomery_core::gguf::GgufMeta {
        arch: "qwen2".into(),
        layers: 1,
        kv_heads: 1,
        head_dim: 1,
        training_ctx: 4096,
        weights_bytes: 1,
    };
    pager
        .register_model("panic-model", &gguf, meta, None)
        .unwrap();
    let info = pager
        .create_agent("panic-model", 100, None, 200_000)
        .unwrap();

    let (port, mut handle) = bloomery_daemon::http::serve(pager, 0);
    handle.set_scratch_dir(dir);
    (port, handle, info.id)
}

/// A panic inside one request must not cascade into every future request
/// silently: it poisons the pager's mutex, and every request after it must
/// get the same named 500 rather than a hang, a second panic, or (worse) a
/// "successful" response served against state nobody can vouch for.
#[test]
fn a_panicking_request_poisons_the_pager_and_subsequent_requests_get_a_named_500() {
    let (port, handle, id) = serve_panicking();
    let addr = format!("127.0.0.1:{port}");

    // This request panics inside the worker thread that services it.
    // tiny_http's `Drop for Request` sends a bare `500` if `respond()` was
    // never called, so the client still gets a well-formed HTTP response —
    // just not the JSON body `api_native` would send on a handled error.
    let (st, _body) = http(
        &addr,
        "POST",
        &format!("/agents/{id}/infer"),
        r#"{"prompt":"hi","max_tokens":16}"#,
    );
    assert_eq!(st, 500);

    // A second, independent request must see the poison, not a hang or a
    // second panic: `std::sync::Mutex` poisoning is a property of the
    // mutex, not the thread that caused it, so whichever of the three
    // still-live workers picks this up hits it too.
    let (st, body) = http(&addr, "GET", "/status", "");
    assert_eq!(st, 500, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["error"], "internal");
    assert!(v["detail"].as_str().unwrap().contains("poisoned"), "{body}");
    handle.shutdown();
}
