//! Native HTTP API: mutex poisoning.
//!
//! A panicking request must degrade to named 500s, not a panic cascade or a
//! hang — `api_native::lock_pager`'s sticky-poison contract, driven over a
//! real socket.
//!
//! Split out of `api_native_test.rs` on 2026-09-01 (carried-debt slice D).

mod common;

use common::http;

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
        _stop: Option<&str>,
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
        attention_layers: 1,
        kv_heads: 1,
        head_dim: 1,
        training_ctx: 4096,
        weights_bytes: 1,
        value_length: None,
        recurrent_state_bytes: 0,
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

/// Task 14 left `config.default_priority` / `config.default_budget_tokens`
/// dead: this layer hardcoded 100 / 200 000 and an operator's configured
/// defaults reached nothing. The values now live on the `Pager` (wired from
/// config by `main.rs`), and a body that omits `priority` / `budget_tokens`
/// must land on *those*, not on a constant retyped here.
#[test]
fn an_agent_created_without_priority_or_budget_lands_on_the_pagers_defaults() {
    let (port, handle) = bloomery_daemon::test_support::serve_fake_with_defaults(7, 5000);
    let addr = format!("127.0.0.1:{port}");
    let (st, body) = http(&addr, "POST", "/agents", r#"{"model":"qwen"}"#);
    assert_eq!(st, 201, "{body}");
    let id = serde_json::from_str::<serde_json::Value>(&body).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();

    let (st, body) = http(&addr, "GET", "/status", "");
    assert_eq!(st, 200, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    let agent = v["agents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["id"] == serde_json::json!(id))
        .expect("the created agent is in status");
    assert_eq!(agent["priority"], 7, "configured default_priority carried");
    assert_eq!(
        agent["budget_granted"], 5000,
        "configured default_budget_tokens carried"
    );
    handle.shutdown();
}
