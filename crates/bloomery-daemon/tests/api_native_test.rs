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

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use bloomery_daemon::config::{MemoryConfig, Tier};
use bloomery_daemon::drift::ProfileStore;
use bloomery_daemon::memory::build_memory;
use bloomery_daemon::pager::Pager;
use bloomery_daemon::post::PostRunner;
use bloomery_daemon::swap::{
    scratch_identity, CoverGate, SwapContext, SwapOutcomeReport, SwapProbes, NOTE_HANDOVER,
    NOTE_TASK_GATES,
};
use bloomery_substrate::fake::FakeSubstrate;
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
///
/// `FREE_VRAM_BYTES` deliberately stays `1024 * 1024 * 1024`, *not*
/// `test_support::FIXTURE_FREE_VRAM_BYTES` (Task 3 bumped that private
/// constant by `+ 1000`, `qwen_like_meta`'s `weights_bytes`, so weights
/// entering the reservation budget don't change what `qwen` alone can
/// place). That `+ 1000` is exactly offset by the `− 1000` the real budget
/// now carries for `qwen`'s loaded weights, so the *observed* `free` this
/// test asserts against lands back on the original, unbumped number.
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
    // Task 5's live-run fix added the daemon-level overhead margin to the
    // placement budget (it had only ever been in the window law), so the
    // observed `free` is now short by exactly the fixture's
    // `FIXTURE_OVERHEAD_BYTES`. Nothing about *why* this refuses changed —
    // same-priority residents, `reclaimable: 0` — only the supply side of
    // the arithmetic gained the term it was always meant to hold back.
    const FIXTURE_OVERHEAD_BYTES: u64 = 16 * 1024 * 1024;
    assert_eq!(
        v["free"],
        FREE_VRAM_BYTES - FIXTURE_OVERHEAD_BYTES - 4 * PER_AGENT_KV_BYTES
    );
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

/// 422: a model with a standing drift block refuses new agent creation,
/// naming the reference baseline the regression was measured against — the
/// same status `Unprofiled` gets, because it is the same class of answer on
/// the same path clients already handle. A `PagerError` mapped on one
/// surface and not the other is a 500 waiting for whichever client hits the
/// unmapped path, so this pins the native side of that obligation.
#[test]
fn create_agent_on_a_drift_blocked_model_returns_422() {
    let (port, handle) = serve_drift_blocked_qwen();
    let addr = format!("127.0.0.1:{port}");
    let (st, body) = http(&addr, "POST", "/agents", r#"{"model":"qwen"}"#);
    assert_eq!(st, 422, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["error"], "drift_blocked");
    assert_eq!(v["model"], "qwen");
    assert_eq!(v["reference"], "base42");
    handle.shutdown();
}

/// Builds and serves a `Pager<FakeSubstrate>` with `qwen` registered,
/// profiled (so admission reaches the drift-block clause rather than
/// stopping at `Unprofiled`), and its cumulative drift reading set to a
/// `Confirmed` regression against baseline `"base42"` — the one shape
/// `set_drift` turns into an admission block (Task 2's invariant).
fn serve_drift_blocked_qwen() -> (u16, bloomery_daemon::http::ServerHandle) {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "bloomery-drift-blocked-test-{}-{seq}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("scratch dir");

    let journal = bloomery_core::journal::Journal::open(&dir.join("j.jsonl")).unwrap();
    let images = bloomery_daemon::agents::ImageStore::new(&dir.join("img")).unwrap();
    let mut pager = bloomery_daemon::pager::Pager::new(
        bloomery_substrate::fake::FakeSubstrate::new(),
        journal,
        images,
        Box::new(|| Some(1024 * 1024 * 1024)),
    );
    let gguf = dir.join("qwen.gguf");
    std::fs::write(&gguf, b"weights").unwrap();
    let meta = bloomery_core::gguf::GgufMeta {
        arch: "qwen2".into(),
        layers: 28,
        attention_layers: 28,
        kv_heads: 4,
        head_dim: 128,
        training_ctx: 4096,
        weights_bytes: 1000,
        recurrent_state_bytes: 0,
    };
    pager.register_model("qwen", &gguf, meta, None).unwrap();
    pager
        .attach_profile(
            "qwen",
            bloomery_core::profile::Profile::from_json(&profile_doc("qwen", 2048))
                .expect("fixture profile parses"),
            false,
        )
        .unwrap();
    pager
        .set_drift(
            "qwen",
            bloomery_daemon::drift::ModelDrift {
                step: bloomery_daemon::drift::DriftStatus::WithinNoise,
                cumulative: bloomery_daemon::drift::DriftStatus::Confirmed {
                    reference: "base42".to_string(),
                },
            },
        )
        .unwrap();

    let (port, mut handle) = bloomery_daemon::http::serve(pager, 0);
    handle.set_scratch_dir(dir);
    (port, handle)
}

/// [`serve_drift_blocked_qwen`], but also wires the profiles directory and
/// files `qwen`'s current profile on disk — the fixture "bless does not
/// unblock" needs to observe something real over HTTP: `bless` reads
/// `profiles_dir/qwen.json` from disk (`ProfileStore::bless`), and
/// `serve_drift_blocked_qwen` alone wires no profiles directory at all, so a
/// bless against it would 500 before the property could even be asked
/// about.
fn serve_drift_blocked_qwen_with_profiles() -> (u16, bloomery_daemon::http::ServerHandle, PathBuf) {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "bloomery-drift-blocked-profiled-test-{}-{seq}",
        std::process::id()
    ));
    std::fs::create_dir_all(dir.join("profiles")).expect("scratch dir");
    std::fs::write(
        dir.join("profiles").join("qwen.json"),
        profile_doc("qwen", 2048),
    )
    .unwrap();

    let journal = bloomery_core::journal::Journal::open(&dir.join("j.jsonl")).unwrap();
    let images = bloomery_daemon::agents::ImageStore::new(&dir.join("img")).unwrap();
    let mut pager = bloomery_daemon::pager::Pager::new(
        bloomery_substrate::fake::FakeSubstrate::new(),
        journal,
        images,
        Box::new(|| Some(1024 * 1024 * 1024)),
    );
    pager.set_profiles_dir(dir.join("profiles"));
    let gguf = dir.join("qwen.gguf");
    std::fs::write(&gguf, b"weights").unwrap();
    let meta = bloomery_core::gguf::GgufMeta {
        arch: "qwen2".into(),
        layers: 28,
        attention_layers: 28,
        kv_heads: 4,
        head_dim: 128,
        training_ctx: 4096,
        weights_bytes: 1000,
        recurrent_state_bytes: 0,
    };
    pager.register_model("qwen", &gguf, meta, None).unwrap();
    pager
        .attach_profile(
            "qwen",
            bloomery_core::profile::Profile::from_json(&profile_doc("qwen", 2048))
                .expect("fixture profile parses"),
            false,
        )
        .unwrap();
    pager
        .set_drift(
            "qwen",
            bloomery_daemon::drift::ModelDrift {
                step: bloomery_daemon::drift::DriftStatus::WithinNoise,
                cumulative: bloomery_daemon::drift::DriftStatus::Confirmed {
                    reference: "base42".to_string(),
                },
            },
        )
        .unwrap();

    let (port, mut handle) = bloomery_daemon::http::serve(pager, 0);
    handle.set_scratch_dir(dir.clone());
    (port, handle, dir)
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
        attention_layers: 28,
        kv_heads: 4,
        head_dim: 128,
        training_ctx: 4096,
        weights_bytes: 1000,
        recurrent_state_bytes: 0,
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

// ---------------------------------------------------------------------------
// The operator bless route (drift-watch design §2): "bless the current
// profile of model M as baseline", journaled with the profile's identity.
// ---------------------------------------------------------------------------

/// A minimal but real assay profile document, the same shape `drift_test.rs`
/// and `post_test.rs` use. `max_verified` is the knob that makes two documents
/// genuinely different bytes while still describing the same model measured by
/// the same instrument — which is what a baseline replacement actually meets.
fn profile_doc(model: &str, max_verified: u32) -> String {
    format!(
        r#"{{"assay_profile_version":3,"probe_version":"0.4.1","model":{{"name":"{model}"}},"ceiling":{{"max_verified":{max_verified}}},"verdicts":{{}}}}"#
    )
}

/// Builds and serves a `Pager<FakeSubstrate>` with `qwen` registered and —
/// when `wire_profiles_dir` — the profiles directory `main.rs` wires from
/// `config.data_dir/profiles`. Returns the scratch dir: the profiles directory
/// is `dir/profiles` and the boot journal is `dir/j.jsonl`.
///
/// Built from the same public pieces `serve_panicking` uses rather than from
/// `test_support::serve_fake`, which wires no profiles directory at all — and
/// `wire_profiles_dir: false` is exactly that daemon, the one this route must
/// refuse rather than serve by writing a baseline somewhere of its own
/// choosing.
fn serve_with_profiles(
    wire_profiles_dir: bool,
) -> (u16, bloomery_daemon::http::ServerHandle, PathBuf) {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let dir =
        std::env::temp_dir().join(format!("bloomery-bless-test-{}-{seq}", std::process::id()));
    std::fs::create_dir_all(dir.join("profiles")).expect("scratch dir");

    let journal = bloomery_core::journal::Journal::open(&dir.join("j.jsonl")).unwrap();
    let images = bloomery_daemon::agents::ImageStore::new(&dir.join("img")).unwrap();
    let mut pager = bloomery_daemon::pager::Pager::new(
        bloomery_substrate::fake::FakeSubstrate::new(),
        journal,
        images,
        Box::new(|| Some(1024 * 1024 * 1024)),
    );
    if wire_profiles_dir {
        pager.set_profiles_dir(dir.join("profiles"));
    }
    let gguf = dir.join("qwen.gguf");
    std::fs::write(&gguf, b"weights").unwrap();
    let meta = bloomery_core::gguf::GgufMeta {
        arch: "qwen2".into(),
        layers: 28,
        attention_layers: 28,
        kv_heads: 4,
        head_dim: 128,
        training_ctx: 4096,
        weights_bytes: 1000,
        recurrent_state_bytes: 0,
    };
    pager.register_model("qwen", &gguf, meta, None).unwrap();

    let (port, mut handle) = bloomery_daemon::http::serve(pager, 0);
    handle.set_scratch_dir(dir.clone());
    (port, handle, dir)
}

/// Every `Blessed` row in the fixture's journal as
/// `(model, profile_path, sha, provenance)`.
fn blessed_rows(dir: &Path) -> Vec<(String, String, String, String)> {
    bloomery_core::journal::replay(&dir.join("j.jsonl"))
        .unwrap()
        .iter()
        .filter_map(|e| match e {
            bloomery_core::journal::Event::Blessed {
                model,
                profile_path,
                sha,
                provenance,
            } => Some((
                model.clone(),
                profile_path.clone(),
                sha.clone(),
                provenance.clone(),
            )),
            _ => None,
        })
        .collect()
}

/// 200: the blessing copies this boot's profile to the baseline, answers with
/// the identity of the bytes that landed there, and journals `operator` as the
/// provenance — design §2's "the provenance of every baseline is explicit".
#[test]
fn blessing_a_current_profile_answers_its_identity_and_journals_the_operator() {
    let (port, handle, dir) = serve_with_profiles(true);
    let addr = format!("127.0.0.1:{port}");
    let profiles = dir.join("profiles");
    let doc = profile_doc("qwen", 2048);
    std::fs::write(profiles.join("qwen.json"), &doc).unwrap();

    let (st, body) = http(&addr, "POST", "/models/qwen/bless", "");
    assert_eq!(st, 200, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    let baseline = profiles.join("qwen.baseline.json");
    assert_eq!(v["model"], "qwen");
    assert_eq!(
        v["sha"],
        bloomery_core::journal::sha256_hex(&doc),
        "the sha is of the blessed bytes, so `sha256sum` on the path checks it"
    );
    assert_eq!(v["path"], baseline.display().to_string());
    assert_eq!(std::fs::read_to_string(&baseline).unwrap(), doc);
    assert!(
        profiles.join("qwen.json").exists(),
        "blessing copies the current profile, it does not consume it"
    );

    let rows = blessed_rows(&dir);
    assert_eq!(rows.len(), 1, "one blessing, one row: {rows:?}");
    assert_eq!(rows[0].0, "qwen");
    assert_eq!(rows[0].1, baseline.display().to_string());
    assert_eq!(rows[0].2, bloomery_core::journal::sha256_hex(&doc));
    assert_eq!(rows[0].3, "operator");
    handle.shutdown();
}

/// Design §2: "Re-blessing replaces the baseline and journals the old identity
/// beside the new." The replaced document's bytes are gone — overwritten by
/// this blessing — so its digest in the row is all that is left of it, and it
/// is what ties this row back to the earlier `Blessed` row that named the same
/// digest.
#[test]
fn re_blessing_replaces_the_baseline_and_journals_the_replaced_identity() {
    let (port, handle, dir) = serve_with_profiles(true);
    let addr = format!("127.0.0.1:{port}");
    let profiles = dir.join("profiles");
    let old = profile_doc("qwen", 1024);
    let new = profile_doc("qwen", 4096);
    std::fs::write(profiles.join("qwen.baseline.json"), &old).unwrap();
    std::fs::write(profiles.join("qwen.json"), &new).unwrap();

    let (st, body) = http(&addr, "POST", "/models/qwen/bless", "");
    assert_eq!(st, 200, "{body}");
    assert_eq!(
        std::fs::read_to_string(profiles.join("qwen.baseline.json")).unwrap(),
        new,
        "re-blessing replaces the baseline"
    );

    let rows = blessed_rows(&dir);
    assert_eq!(rows.len(), 1, "one blessing, one row: {rows:?}");
    assert_eq!(rows[0].2, bloomery_core::journal::sha256_hex(&new));
    assert_eq!(
        rows[0].3,
        format!(
            "operator (replaced {})",
            bloomery_core::journal::sha256_hex(&old)
        ),
        "the identity the blessing overwrote is journaled beside the new one"
    );
    handle.shutdown();
}

/// 404: a name this daemon was never configured with. Same body shape as every
/// other unknown-model refusal on this surface, and nothing is written — a
/// route that filed a baseline for a model the pager does not serve would be
/// inventing evidence about a model nobody measured.
#[test]
fn blessing_an_unknown_model_returns_404_and_files_nothing() {
    let (port, handle, dir) = serve_with_profiles(true);
    let addr = format!("127.0.0.1:{port}");

    let (st, body) = http(&addr, "POST", "/models/does-not-exist/bless", "");
    assert_eq!(st, 404, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["error"], "unknown_model");
    assert_eq!(v["model"], "does-not-exist");
    assert!(
        blessed_rows(&dir).is_empty(),
        "a refused blessing journals nothing"
    );
    assert!(!dir
        .join("profiles")
        .join("does-not-exist.baseline.json")
        .exists());
    handle.shutdown();
}

/// 409: there is no current profile to bless (POST never ran, or it failed for
/// this model). Named and refused — never a silent no-op, and never a 200 that
/// would tell an operator a baseline exists when nothing was written.
#[test]
fn blessing_with_no_current_profile_is_a_named_409_not_a_silent_no_op() {
    let (port, handle, dir) = serve_with_profiles(true);
    let addr = format!("127.0.0.1:{port}");
    let profiles = dir.join("profiles");

    let (st, body) = http(&addr, "POST", "/models/qwen/bless", "");
    assert_eq!(st, 409, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["error"], "no_current_profile");
    assert_eq!(v["model"], "qwen");
    let detail = v["detail"].as_str().unwrap_or_default().to_string();
    assert!(
        detail.contains("nothing to bless")
            && detail.contains(&profiles.join("qwen.json").display().to_string()),
        "the refusal names the document it looked for: {detail}"
    );
    assert!(
        !profiles.join("qwen.baseline.json").exists(),
        "a failed blessing writes no baseline"
    );
    assert!(
        blessed_rows(&dir).is_empty(),
        "a refused blessing journals nothing"
    );
    handle.shutdown();
}

/// A daemon with no profiles directory wired refuses by name rather than
/// blessing into whatever directory it happens to be running in. Unreachable
/// through `main.rs` (which always wires one), which is exactly why it is
/// pinned: the failure mode of a default here is a baseline nobody can find.
#[test]
fn blessing_without_a_configured_profiles_directory_is_a_named_500() {
    let (port, handle, dir) = serve_with_profiles(false);
    let addr = format!("127.0.0.1:{port}");
    std::fs::write(
        dir.join("profiles").join("qwen.json"),
        profile_doc("qwen", 2048),
    )
    .unwrap();

    let (st, body) = http(&addr, "POST", "/models/qwen/bless", "");
    assert_eq!(st, 500, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["error"], "internal");
    assert!(
        v["detail"]
            .as_str()
            .unwrap_or_default()
            .contains("profiles directory"),
        "{body}"
    );
    assert!(
        blessed_rows(&dir).is_empty(),
        "a refused blessing journals nothing"
    );
    assert!(!dir.join("profiles").join("qwen.baseline.json").exists());
    handle.shutdown();
}

/// The route table's `_ => 404` still catches everything the new arm does not:
/// a neighbouring verb under `/models/{name}/` and the same path under the
/// wrong method are `not_found`, not blessings.
#[test]
fn a_neighbouring_path_or_the_wrong_method_still_falls_through_to_not_found() {
    let (port, handle, dir) = serve_with_profiles(true);
    let addr = format!("127.0.0.1:{port}");
    std::fs::write(
        dir.join("profiles").join("qwen.json"),
        profile_doc("qwen", 2048),
    )
    .unwrap();

    for (method, path) in [
        ("POST", "/models/qwen/blessing"),
        ("GET", "/models/qwen/bless"),
        ("POST", "/models/qwen/bless/again"),
    ] {
        let (st, body) = http(&addr, method, path, "");
        assert_eq!(st, 404, "{method} {path}: {body}");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["error"], "not_found", "{method} {path}");
    }
    assert!(
        blessed_rows(&dir).is_empty(),
        "no near-miss request blessed anything"
    );
    assert!(!dir.join("profiles").join("qwen.baseline.json").exists());
    handle.shutdown();
}

// ---------------------------------------------------------------------------
// The operator unblock route (verdict-gated-admission design §4): "I know,
// let it run anyway" — clears THIS boot's admission block without touching
// the reading or the blessed baseline. Neither this route nor `bless`
// implies the other.
// ---------------------------------------------------------------------------

/// Every `Admission` row in the fixture's journal as
/// `(model, action, reference, provenance)`.
fn admission_rows(dir: &Path) -> Vec<(String, String, String, String)> {
    bloomery_core::journal::replay(&dir.join("j.jsonl"))
        .unwrap()
        .iter()
        .filter_map(|e| match e {
            bloomery_core::journal::Event::Admission {
                model,
                action,
                reference,
                provenance,
            } => Some((
                model.clone(),
                action.clone(),
                reference.clone(),
                provenance.clone(),
            )),
            _ => None,
        })
        .collect()
}

/// 200: clearing a standing block answers with what was cleared, journals
/// `"cleared"` with operator provenance, and admits new agents against the
/// model again — while the drift reading itself, still `Confirmed`, is left
/// exactly as measured.
#[test]
fn unblocking_a_blocked_model_admits_and_journals_the_operator() {
    let (port, handle) = serve_drift_blocked_qwen();
    let addr = format!("127.0.0.1:{port}");

    let (st, body) = http(&addr, "POST", "/models/qwen/unblock", "");
    assert_eq!(st, 200, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["model"], "qwen");
    assert_eq!(v["cleared"]["reference"], "base42");

    // Admission is open again…
    let (st, body) = http(&addr, "POST", "/agents", r#"{"model":"qwen"}"#);
    assert_eq!(st, 201, "{body}");

    // …and the reading itself is untouched.
    let (st, body) = http(&addr, "GET", "/status", "");
    assert_eq!(st, 200, "{body}");
    let status: serde_json::Value = serde_json::from_str(&body).unwrap();
    let model = status["models"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["name"] == "qwen")
        .unwrap();
    assert_eq!(
        model["drift"]["cumulative"]["status"], "confirmed",
        "{model}"
    );

    handle.shutdown();
}

/// 404: a name this daemon was never configured with. Same body shape as
/// every other unknown-model refusal on this surface.
#[test]
fn unblocking_an_unknown_model_returns_404() {
    let (port, handle) = bloomery_daemon::test_support::serve_fake();
    let addr = format!("127.0.0.1:{port}");

    let (st, body) = http(&addr, "POST", "/models/does-not-exist/unblock", "");
    assert_eq!(st, 404, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["error"], "unknown_model");
    assert_eq!(v["model"], "does-not-exist");
    handle.shutdown();
}

/// 409: a known, unblocked model. Answering 200 here would tell an operator
/// they cleared something when nothing was written — the silent no-op
/// design §4 forbids, the same reason `bless`'s 409 exists.
#[test]
fn unblocking_a_model_with_no_standing_block_is_a_named_409_not_a_silent_no_op() {
    let (port, handle) = bloomery_daemon::test_support::serve_fake();
    let addr = format!("127.0.0.1:{port}");

    let (st, body) = http(&addr, "POST", "/models/qwen/unblock", "");
    assert_eq!(st, 409, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["error"], "no_admission_block");
    assert_eq!(v["model"], "qwen");
    assert!(
        v["detail"].as_str().is_some_and(|d| !d.is_empty()),
        "{body}"
    );
    handle.shutdown();
}

/// Unblocking does not rebaseline: it takes the block down without filing a
/// new baseline anywhere, so there is nothing for a next-boot comparison to
/// read differently. And a bless on a blocked model does not, on its own,
/// admit anything this boot — the two routes answer different questions.
///
/// The fixture is a model that IS blocked
/// (`serve_drift_blocked_qwen_with_profiles`), not `serve_with_profiles`'s
/// unblocked one: against an unblocked model, "bless does not unblock" is
/// unobservable — there is nothing standing for a bless to (not) clear, so
/// the property can only be pinned by watching a real block survive a bless.
#[test]
fn unblock_does_not_bless_and_bless_does_not_unblock_over_http() {
    let (port, handle, dir) = serve_drift_blocked_qwen_with_profiles();
    let addr = format!("127.0.0.1:{port}");

    // The block stands before either route is touched.
    let block_reference = |body: &str| -> serde_json::Value {
        let v: serde_json::Value = serde_json::from_str(body).unwrap();
        v["models"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["name"] == "qwen")
            .unwrap()["admission_block"]
            .clone()
    };
    let (st, body) = http(&addr, "GET", "/status", "");
    assert_eq!(st, 200, "{body}");
    assert_eq!(block_reference(&body)["reference"], "base42", "{body}");
    // The fixture's own `set_drift` already journaled the "blocked" row that
    // put this block there — captured here so the next check is "bless adds
    // no row of its own", not the wrong claim "there is no row at all".
    let rows_before_bless = admission_rows(&dir);

    // Bless does not unblock: the block stands after a bless…
    let (st, body) = http(&addr, "POST", "/models/qwen/bless", "");
    assert_eq!(st, 200, "{body}");
    assert_eq!(
        admission_rows(&dir),
        rows_before_bless,
        "bless journals no Admission row of its own"
    );
    let (st, body) = http(&addr, "GET", "/status", "");
    assert_eq!(st, 200, "{body}");
    assert_eq!(
        block_reference(&body)["reference"],
        "base42",
        "bless must not clear the standing block: {body}"
    );

    // …observably: new agents are still refused after the bless.
    let (st, body) = http(&addr, "POST", "/agents", r#"{"model":"qwen"}"#);
    assert_eq!(
        st, 422,
        "a bless on a blocked model must not, on its own, admit anything this boot: {body}"
    );

    // Unblock does not rebaseline: the baseline bytes bless just wrote are
    // untouched by the unblock that follows.
    let baseline = dir.join("profiles").join("qwen.baseline.json");
    assert!(baseline.exists());
    let before = std::fs::read(&baseline).unwrap();
    let (st, body) = http(&addr, "POST", "/models/qwen/unblock", "");
    assert_eq!(st, 200, "{body}");
    assert_eq!(
        std::fs::read(&baseline).unwrap(),
        before,
        "unblock must not touch the blessed baseline"
    );

    // And now that unblock actually ran, admission is open again — the
    // fixture's block is gone, not merely unobserved.
    let (st, body) = http(&addr, "POST", "/agents", r#"{"model":"qwen"}"#);
    assert_eq!(st, 201, "{body}");

    handle.shutdown();
}

/// The route table's `_ => 404` still catches a neighbouring path or the
/// wrong method.
#[test]
fn unblock_neighbouring_path_or_wrong_method_falls_through_to_not_found() {
    let (port, handle) = serve_drift_blocked_qwen();
    let addr = format!("127.0.0.1:{port}");

    for (method, path) in [
        ("POST", "/models/qwen/unblocking"),
        ("GET", "/models/qwen/unblock"),
        ("POST", "/models/qwen/unblock/again"),
    ] {
        let (st, body) = http(&addr, method, path, "");
        assert_eq!(st, 404, "{method} {path}: {body}");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["error"], "not_found", "{method} {path}");
    }
    handle.shutdown();
}

/// The tier an operator declared is what every profile in this daemon is
/// marked with, so `/status` has to say which one it is — `null` when the
/// daemon was never told, never an invented name.
#[test]
fn status_reports_the_declared_tier() {
    let (port, handle) =
        bloomery_daemon::test_support::serve_fake_with_tier("mid-gamer-12gb", true);
    let (st, body) = http(&format!("127.0.0.1:{port}"), "GET", "/status", "");
    assert_eq!(st, 200, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["tier"]["name"], "mid-gamer-12gb");
    assert_eq!(v["tier"]["emulated"], true);
    handle.shutdown();
}

// ---------------------------------------------------------------------------
// Task 8: the memory organ's `/status` surface (memory-organ design §6).
// ---------------------------------------------------------------------------

/// A fresh, per-test tempdir for a `memory::build_memory` call's own
/// `data_dir` — separate from `serve_fake`'s own scratch dir, since the
/// context is built by the test before the fixture pager exists.
fn fresh_memory_dir(tag: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "bloomery-memory-status-{tag}-{}-{seq}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

/// A freshly booted, enabled organ with an empty store: `/status` carries
/// `memory.enabled == true` and every count at zero, with no
/// `disabled_reason` — spec §6's operator surface for the routine case.
#[test]
fn status_reports_memory_zero_counts_for_a_fresh_enabled_context() {
    let dir = fresh_memory_dir("fresh");
    let cfg = MemoryConfig {
        enabled: true,
        max_episodes: 256,
        refalsify: false,
    };
    let memory = build_memory(&cfg, &dir);
    assert!(memory.operational(), "{:?}", memory.disabled_reason);

    let (port, handle) = bloomery_daemon::test_support::serve_fake_with_memory(memory);
    let (st, body) = http(&format!("127.0.0.1:{port}"), "GET", "/status", "");
    assert_eq!(st, 200, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["memory"]["enabled"], true);
    assert_eq!(v["memory"]["episodes"], 0);
    assert_eq!(v["memory"]["verified"], 0);
    assert_eq!(v["memory"]["contradicted"], 0);
    assert_eq!(v["memory"]["parse_errors"], 0);
    assert!(v["memory"]["disabled_reason"].is_null());
    handle.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}

/// `build_memory` pointed at a store path that is a DIRECTORY (forcing
/// `MemoryStore::load`'s hard `io::Error` arm — spec §7's "store unreadable
/// at boot"): `/status` carries `memory.disabled_reason` and every count is
/// `null`, since there is no store to count. Boot proceeds regardless — this
/// test drives the daemon serving fine over HTTP with the organ in exactly
/// that state.
#[test]
fn status_reports_memory_disabled_reason_when_store_path_is_unreadable() {
    let dir = fresh_memory_dir("unreadable");
    std::fs::create_dir_all(dir.join("memory").join("episodes.jsonl")).expect("directory-as-file");
    let cfg = MemoryConfig {
        enabled: true,
        max_episodes: 256,
        refalsify: false,
    };
    let memory = build_memory(&cfg, &dir);
    assert!(!memory.operational());

    let (port, handle) = bloomery_daemon::test_support::serve_fake_with_memory(memory);
    let (st, body) = http(&format!("127.0.0.1:{port}"), "GET", "/status", "");
    assert_eq!(st, 200, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["memory"]["enabled"], true);
    assert!(v["memory"]["episodes"].is_null());
    assert!(v["memory"]["verified"].is_null());
    assert!(v["memory"]["contradicted"].is_null());
    assert!(v["memory"]["parse_errors"].is_null());
    let reason = v["memory"]["disabled_reason"].as_str().unwrap_or("");
    assert!(
        reason.starts_with("memory store unreadable: "),
        "reason: {reason:?}"
    );
    handle.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}

/// A daemon served through plain `serve_fake()` (no memory context wired at
/// all — the every-other-`/status`-test shape) carries no `memory` key,
/// rather than `null` or an invented zeroed object.
#[test]
fn status_has_no_memory_key_when_no_context_is_wired() {
    let (port, handle) = bloomery_daemon::test_support::serve_fake();
    let (st, body) = http(&format!("127.0.0.1:{port}"), "GET", "/status", "");
    assert_eq!(st, 200, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert!(v.get("memory").is_none(), "{body}");
    handle.shutdown();
}

// ---------------------------------------------------------------------------
// The swap-candidate routes (swap-candidate seam design §4): `POST
// /models/{name}/swap-candidate` starts one job and answers `202` at once — a
// probe holds VRAM for ~10 minutes, so it cannot ride a request handler — and
// `GET` reads the one slot that job finishes into.
//
// Both of the job's subprocesses are scripted through
// `SwapContext::with_probes`, the same seam `swap_test.rs` drives the job
// itself through, so every row below runs with no python, no assay and no GPU.
// ---------------------------------------------------------------------------

/// The configured model whose role every candidate below would take.
const SWAP_MODEL: &str = "qwen";

/// A wait status carrying exit code `code` — the encoding `waitpid` returns.
/// Copied from `swap_test.rs` rather than shared: each file under `tests/` is
/// its own crate, and this is three lines.
fn exited(code: i32) -> std::process::ExitStatus {
    use std::os::unix::process::ExitStatusExt;
    std::process::ExitStatus::from_raw(code << 8)
}

fn output(status: std::process::ExitStatus) -> std::process::Output {
    std::process::Output {
        status,
        stdout: Vec::new(),
        stderr: Vec::new(),
    }
}

/// The value following `flag` in an argv, or the empty string.
fn value_of(args: &[String], flag: &str) -> String {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .cloned()
        .unwrap_or_default()
}

fn kv_string(buf: &mut Vec<u8>, key: &str, val: &str) {
    buf.extend((key.len() as u64).to_le_bytes());
    buf.extend(key.as_bytes());
    buf.extend(8u32.to_le_bytes());
    buf.extend((val.len() as u64).to_le_bytes());
    buf.extend(val.as_bytes());
}

fn kv_u32(buf: &mut Vec<u8>, key: &str, val: u32) {
    buf.extend((key.len() as u64).to_le_bytes());
    buf.extend(key.as_bytes());
    buf.extend(4u32.to_le_bytes());
    buf.extend(val.to_le_bytes());
}

/// A **real, parseable** GGUF file, copied from `swap_test.rs`: the worker
/// registers the candidate through `parse_gguf_meta` + `register_model`, the
/// same pair `main.rs` registers every configured model with, so a placeholder
/// byte string would never get past the registration.
fn write_gguf(path: &Path, name: &str) {
    use std::io::Write;
    let mut kvs = Vec::new();
    kv_string(&mut kvs, "general.architecture", "qwen2");
    kv_string(&mut kvs, "general.name", name);
    kv_u32(&mut kvs, "qwen2.block_count", 28);
    kv_u32(&mut kvs, "qwen2.attention.head_count_kv", 4);
    kv_u32(&mut kvs, "qwen2.attention.key_length", 128);
    kv_u32(&mut kvs, "qwen2.context_length", 4096);
    let mut f = std::fs::File::create(path).expect("gguf fixture");
    f.write_all(b"GGUF").unwrap();
    f.write_all(&3u32.to_le_bytes()).unwrap();
    f.write_all(&0u64.to_le_bytes()).unwrap(); // tensor_count
    f.write_all(&6u64.to_le_bytes()).unwrap(); // kv_count
    f.write_all(&kvs).unwrap();
}

/// Every argv one scripted assay was handed, in order. `Arc`/`Mutex` rather
/// than `swap_test.rs`'s `Rc`/`RefCell` because these collaborators are built
/// on the spawned worker thread, not on the test's.
type Seen = Arc<Mutex<Vec<Vec<String>>>>;

/// A hook the fake probe runs before it writes its document — how the two
/// failure rows below break the world in the middle of a job. Installed after
/// the fixture exists (the interesting hooks need the fixture's own pager).
type Hook = Arc<Mutex<Option<Arc<dyn Fn() + Send + Sync>>>>;

/// Every `/v1/chat/completions` call the fake probe made, as `(model, status)`
/// — the admission the live acceptance found unreachable, observed rather than
/// assumed. Only ever non-empty under [`SwapCfg::drive_v1`].
type V1Calls = Arc<Mutex<Vec<(String, u16)>>>;

/// A daemon serving `qwen` with the swap-candidate surface wired: the profiles
/// directory `main.rs` wires, a real candidate GGUF on disk, and both of the
/// job's subprocesses scripted — the probe writing a real document for
/// whatever `--model` it is handed, `cover` answering `cover_exit`.
struct SwapFixture {
    port: u16,
    handle: bloomery_daemon::http::ServerHandle,
    dir: PathBuf,
    pager: Arc<Mutex<Pager<FakeSubstrate>>>,
    ctx: Arc<SwapContext>,
    candidate: PathBuf,
    probes: Seen,
    covers: Seen,
    hook: Hook,
    /// Fires inside the fake `cover`, i.e. strictly AFTER the probe step and
    /// while the scratch identity is still registered — the one moment a test
    /// can ask whether the probe's admission window is still open.
    cover_hook: Hook,
    v1: V1Calls,
}

/// How one fixture's daemon differs from the default. Both knobs exist for the
/// admission rows at the end of this file, and both default to the shape every
/// earlier row was written against.
#[derive(Clone, Copy)]
struct SwapCfg {
    /// `false` is the standing production config: `allow_unprofiled` unset, so
    /// law 5's gate really refuses and the candidate probe's own `/v1` call is
    /// admitted by the candidate window or not at all. The fixture's default
    /// `true` is `Pager::new`'s permissive default, which every earlier row in
    /// this file was written against.
    allow_unprofiled: bool,
    /// The fake probe first drives this daemon's real `/v1/chat/completions`
    /// under the identity it was handed, exactly as assay does, and reports a
    /// non-200 the way assay reported the live 422 — `exit 4`, no document.
    /// This is the whole point of the admission rows: with it off, the probe
    /// seam is scripted end to end and real admission never runs.
    drive_v1: bool,
    /// The fake probe exits 4 without writing a document *after* its `/v1`
    /// call — one job's probe-failure path, driven without touching admission,
    /// so a row can watch the window open and the job still end badly.
    probe_fails: bool,
}

impl Default for SwapCfg {
    fn default() -> Self {
        SwapCfg {
            allow_unprofiled: true,
            drive_v1: false,
            probe_fails: false,
        }
    }
}

fn serve_swap(cover_exit: i32) -> SwapFixture {
    serve_swap_cfg(cover_exit, SwapCfg::default())
}

fn serve_swap_cfg(cover_exit: i32, cfg: SwapCfg) -> SwapFixture {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("bloomery-swap-http-{}-{seq}", std::process::id()));
    let profiles = dir.join("profiles");
    std::fs::create_dir_all(&profiles).expect("scratch dir");

    let journal = bloomery_core::journal::Journal::open(&dir.join("j.jsonl")).unwrap();
    let images = bloomery_daemon::agents::ImageStore::new(&dir.join("img")).unwrap();
    let mut fake = FakeSubstrate::new();
    // Enough replies that every `/v1` call a probe makes is answered like a
    // real one: admission is what these rows measure, so an inference that
    // failed for want of a script would be noise in the status they read.
    for _ in 0..16 {
        fake.script_reply(bloomery_substrate::Reply {
            text: "ok".to_string(),
            prompt_tokens: Some(4),
            completion_tokens: Some(2),
            duration_ms: 1,
        });
    }
    let mut pager = Pager::new(fake, journal, images, Box::new(|| Some(1024 * 1024 * 1024)));
    pager.set_allow_unprofiled(cfg.allow_unprofiled);
    pager.set_profiles_dir(profiles.clone());
    let serving = dir.join("qwen.gguf");
    write_gguf(&serving, "the-model-in-service");
    pager
        .register_model(
            SWAP_MODEL,
            &serving,
            bloomery_core::gguf::parse_gguf_meta(&serving).expect("gguf"),
            None,
        )
        .unwrap();
    let candidate = dir.join("candidate.gguf");
    write_gguf(&candidate, "the-candidate");

    let probes: Seen = Seen::default();
    let covers: Seen = Seen::default();
    let hook: Hook = Hook::default();
    let cover_hook: Hook = Hook::default();
    let v1: V1Calls = V1Calls::default();
    let (probe_sink, cover_sink, probe_hook) = (probes.clone(), covers.clone(), hook.clone());
    let (v1_sink, after_probe) = (v1.clone(), cover_hook.clone());
    let factory = Box::new(move || {
        let (sink, hook, v1_sink) = (probe_sink.clone(), probe_hook.clone(), v1_sink.clone());
        let drive_v1 = cfg.drive_v1;
        let runner = PostRunner::with_runner(Box::new(move |_py, args: &[String]| {
            sink.lock().expect("probe sink").push(args.to_vec());
            let model = value_of(args, "--model");
            if drive_v1 {
                // assay's real first act: drive the endpoint it was pointed at,
                // under the `--model` it was handed. The base URL comes out of
                // the argv rather than a captured port, because the argv is
                // what the job really built.
                let base = args
                    .iter()
                    .find(|a| a.starts_with("http://"))
                    .cloned()
                    .expect("the probe argv names an endpoint");
                let (addr, path) = base
                    .trim_start_matches("http://")
                    .split_once('/')
                    .expect("the endpoint carries assay's /v1 suffix");
                let (st, _) = http(
                    addr,
                    "POST",
                    &format!("/{path}/chat/completions"),
                    &serde_json::json!({
                        "model": model,
                        "messages": [{"role": "user", "content": "hi"}],
                        "max_tokens": 8,
                    })
                    .to_string(),
                );
                v1_sink.lock().expect("v1 sink").push((model.clone(), st));
                if st != 200 {
                    // The live run's own words, in the live run's own shape:
                    // `PostError::NonZeroExit` renders this as `assay exited 4:
                    // …`, which is exactly what the endpoint reported twice
                    // against the real daemon.
                    return Ok(std::process::Output {
                        status: exited(4),
                        stdout: Vec::new(),
                        stderr: format!(
                            "assay: infrastructure failure: HTTP {st} from {base}/chat/completions"
                        )
                        .into_bytes(),
                    });
                }
            }
            // Cloned out from under the lock, so a hook that panics (the
            // wedge row below) cannot poison this mutex on the way past.
            let installed = hook.lock().expect("probe hook").clone();
            if let Some(f) = installed {
                f();
            }
            if cfg.probe_fails {
                return Ok(std::process::Output {
                    status: exited(4),
                    stdout: Vec::new(),
                    stderr: b"assay: scripted probe failure".to_vec(),
                });
            }
            let out = PathBuf::from(value_of(args, "--json"));
            std::fs::write(&out, profile_doc(&model, 2048)).expect("fake probe writes a document");
            Ok(output(exited(0)))
        }));
        let (sink, hook) = (cover_sink.clone(), after_probe.clone());
        let gate = CoverGate::with_runner(Box::new(move |_py, args: &[String]| {
            sink.lock().expect("cover sink").push(args.to_vec());
            let installed = hook.lock().expect("cover hook").clone();
            if let Some(f) = installed {
                f();
            }
            Ok(output(exited(cover_exit)))
        }));
        SwapProbes { runner, gate }
    });

    let pager = Arc::new(Mutex::new(pager));
    let ctx = Arc::new(SwapContext::with_probes(
        factory,
        ProfileStore::new(&profiles),
        Tier {
            name: "enthusiast-16gb".into(),
            emulated: false,
        },
    ));
    let (port, mut handle) =
        bloomery_daemon::http::serve_shared_with_swap(Arc::clone(&pager), 0, Arc::clone(&ctx));
    handle.set_scratch_dir(dir.clone());
    SwapFixture {
        port,
        handle,
        dir,
        pager,
        ctx,
        candidate,
        probes,
        covers,
        hook,
        cover_hook,
        v1,
    }
}

impl SwapFixture {
    fn addr(&self) -> String {
        format!("127.0.0.1:{}", self.port)
    }

    /// The blessed baseline the endpoint requires before it will probe
    /// anything — the operator-endorsed capability statement, never the
    /// merely-latest profile.
    fn seed_floor(&self) {
        std::fs::write(
            self.dir
                .join("profiles")
                .join(format!("{SWAP_MODEL}.baseline.json")),
            profile_doc(SWAP_MODEL, 2048),
        )
        .unwrap();
    }

    fn body(&self) -> String {
        serde_json::json!({"gguf_path": self.candidate.display().to_string()}).to_string()
    }

    fn post(&self, body: &str) -> (u16, String) {
        http(
            &self.addr(),
            "POST",
            &format!("/models/{SWAP_MODEL}/swap-candidate"),
            body,
        )
    }

    fn get(&self) -> (u16, String) {
        http(
            &self.addr(),
            "GET",
            &format!("/models/{SWAP_MODEL}/swap-candidate"),
            "",
        )
    }

    /// Polls `GET` until the job leaves `running`, bounded exactly like every
    /// other poll loop in this crate's tests (200 × 20 ms).
    fn poll_until_done(&self) -> serde_json::Value {
        let mut last = String::new();
        for _ in 0..200 {
            let (st, body) = self.get();
            assert_eq!(st, 200, "{body}");
            let v: serde_json::Value = serde_json::from_str(&body).unwrap();
            if v["state"] != "running" {
                return v;
            }
            last = body;
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        panic!("the candidate job never left `running`: {last}");
    }

    /// One `/v1/chat/completions` against this daemon under `model` — the
    /// request the candidate probe makes, made by hand so a test can ask the
    /// admission question at a moment of its own choosing.
    fn chat(&self, model: &str) -> (u16, String) {
        http(
            &self.addr(),
            "POST",
            "/v1/chat/completions",
            &serde_json::json!({
                "model": model,
                "messages": [{"role": "user", "content": "hi"}],
                "max_tokens": 8,
            })
            .to_string(),
        )
    }

    /// Whether the candidate probe's admission window is still open on the
    /// scratch identity — the window state itself, read off the pager, rather
    /// than inferred from what `/v1` happened to answer.
    fn window_open(&self) -> bool {
        self.pager
            .lock()
            .expect("the pager survives every candidate job")
            .probe_window_open(&scratch_identity(SWAP_MODEL))
    }

    fn events(&self) -> Vec<bloomery_core::journal::Event> {
        bloomery_core::journal::replay(&self.dir.join("j.jsonl")).unwrap()
    }

    fn degraded_reasons(&self) -> Vec<String> {
        self.events()
            .iter()
            .filter_map(|e| match e {
                bloomery_core::journal::Event::Degraded { reason } => Some(reason.clone()),
                _ => None,
            })
            .collect()
    }
}

/// The 202 row, end to end: the handler answers immediately, the worker runs
/// the whole design-§4 flow on its own thread, and `GET` hands back the report
/// the slot was finished with.
#[test]
fn posting_a_candidate_answers_202_and_the_job_reaches_a_verdict() {
    let fixture = serve_swap(0);
    fixture.seed_floor();

    let (st, body) = fixture.post(&fixture.body());
    assert_eq!(st, 202, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["model"], SWAP_MODEL);
    assert_eq!(v["candidate"], fixture.candidate.display().to_string());
    assert_eq!(v["state"], "running");

    let done = fixture.poll_until_done();
    assert_eq!(done["state"], "done", "{done}");
    assert_eq!(done["model"], SWAP_MODEL);
    let report = &done["report"];
    assert_eq!(report["outcome"], "covered");
    assert_eq!(report["exit_code"], 0);
    assert_eq!(
        report["candidate_gguf_sha"],
        bloomery_daemon::agents::model_digest(&fixture.candidate).unwrap(),
        "the report's digest is of the candidate's own bytes"
    );
    assert_eq!(
        report["notes"],
        serde_json::json!([NOTE_TASK_GATES, NOTE_HANDOVER]),
        "every report names both gaps, in design §4's order"
    );

    // The probe ran against THIS daemon's own `/v1`, under the scratch
    // identity — the only thing that makes the candidate addressable at all.
    let probes = fixture.probes.lock().unwrap();
    assert_eq!(probes.len(), 1, "one job probes exactly once: {probes:?}");
    assert!(
        probes[0].contains(&format!("http://127.0.0.1:{}/v1", fixture.port)),
        "the probe must target the bound port: {:?}",
        probes[0]
    );
    assert_eq!(
        value_of(&probes[0], "--model"),
        scratch_identity(SWAP_MODEL)
    );

    // ...and cover compared the blessed floor against the document that probe
    // wrote, which is the document the report names.
    let covers = fixture.covers.lock().unwrap();
    assert_eq!(covers.len(), 1, "one job covers exactly once: {covers:?}");
    assert_eq!(
        covers[0].last().map(String::as_str),
        report["candidate_profile_path"].as_str(),
        "the covered document is the one the report names"
    );

    let rows: Vec<_> = fixture
        .events()
        .into_iter()
        .filter(|e| matches!(e, bloomery_core::journal::Event::SwapCandidate { .. }))
        .collect();
    assert_eq!(rows.len(), 1, "one verdict, one row: {rows:?}");
    drop(probes);
    drop(covers);
    fixture.handle.shutdown();
}

/// 404: a name this daemon was never configured with, answered with the
/// surface's one `unknown_model` shape and nothing started.
#[test]
fn posting_a_candidate_for_an_unknown_model_is_404() {
    let fixture = serve_swap(0);
    fixture.seed_floor();

    let (st, body) = http(
        &fixture.addr(),
        "POST",
        "/models/does-not-exist/swap-candidate",
        &fixture.body(),
    );
    assert_eq!(st, 404, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["error"], "unknown_model");
    assert_eq!(v["model"], "does-not-exist");
    assert!(
        fixture.probes.lock().unwrap().is_empty(),
        "a refused request probes nothing"
    );
    fixture.handle.shutdown();
}

/// 400: the three ways a request body can fail to name a candidate this
/// daemon could probe — not JSON at all, JSON without `gguf_path`, and a
/// `gguf_path` naming bytes that cannot be read. All three are the surface's
/// one `bad_request` shape, and none of them starts a job.
#[test]
fn a_candidate_request_that_names_no_readable_gguf_is_400() {
    let fixture = serve_swap(0);
    fixture.seed_floor();
    let missing = fixture.dir.join("nothing-here.gguf");
    let names_nothing = serde_json::json!({"gguf_path": missing.display().to_string()}).to_string();

    for (body, expected) in [
        ("not json at all", "expected"),
        (r#"{"model":"qwen"}"#, "gguf_path"),
        (names_nothing.as_str(), "nothing-here.gguf"),
    ] {
        let (st, response) = fixture.post(body);
        assert_eq!(st, 400, "{body}: {response}");
        let v: serde_json::Value = serde_json::from_str(&response).unwrap();
        assert_eq!(v["error"], "bad_request", "{body}");
        assert!(
            v["message"].as_str().unwrap_or_default().contains(expected),
            "{body}: {response}"
        );
    }
    assert!(
        fixture.probes.lock().unwrap().is_empty(),
        "a refused request probes nothing"
    );
    fixture.handle.shutdown();
}

/// 409: no blessed baseline. The floor is the operator-endorsed capability
/// statement (design §4's precondition), so there is nothing to cover against
/// and the refusal names the document it looked for — never a probe run
/// against a floor nobody blessed.
#[test]
fn posting_a_candidate_with_no_blessed_baseline_is_409_no_baseline() {
    let fixture = serve_swap(0);

    let (st, body) = fixture.post(&fixture.body());
    assert_eq!(st, 409, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["error"], "no_baseline");
    assert_eq!(v["model"], SWAP_MODEL);
    assert!(
        v["detail"]
            .as_str()
            .unwrap_or_default()
            .contains(&format!("{SWAP_MODEL}.baseline.json")),
        "the refusal names the document it looked for: {body}"
    );
    assert!(
        fixture.probes.lock().unwrap().is_empty(),
        "a refused request probes nothing"
    );
    fixture.handle.shutdown();
}

/// 409: one candidate at a time (design §4 — a probe holds VRAM for ~10
/// minutes, and there is no queue). The slot is claimed by the request thread
/// before any worker starts, so the second request is refused synchronously
/// and names what is running.
#[test]
fn a_second_candidate_while_one_runs_is_409_candidate_probe_in_progress() {
    let fixture = serve_swap(0);
    fixture.seed_floor();
    fixture
        .ctx
        .slot()
        .try_start("some-other-model", Path::new("/models/other.gguf"))
        .expect("the slot starts idle");

    let (st, body) = fixture.post(&fixture.body());
    assert_eq!(st, 409, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["error"], "candidate_probe_in_progress");
    assert_eq!(v["model"], SWAP_MODEL);
    assert!(
        v["detail"]
            .as_str()
            .unwrap_or_default()
            .contains("some-other-model"),
        "the refusal says WHAT is running, not only that something is: {body}"
    );
    assert!(
        fixture.probes.lock().unwrap().is_empty(),
        "a refused request probes nothing"
    );
    fixture.handle.shutdown();
}

/// `GET` while a job runs: `running`, with no report — a verdict nobody
/// reached is never rendered as one.
#[test]
fn getting_a_running_job_reads_running() {
    let fixture = serve_swap(0);
    fixture
        .ctx
        .slot()
        .try_start(SWAP_MODEL, Path::new("/models/candidate.gguf"))
        .expect("the slot starts idle");

    let (st, body) = fixture.get();
    assert_eq!(st, 200, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["model"], SWAP_MODEL);
    assert_eq!(v["state"], "running");
    assert!(v["report"].is_null(), "{body}");
    fixture.handle.shutdown();
}

/// `GET` on a finished job renders the slot's report field for field —
/// including the `"unread"` sentinel, which is what a digest field carries
/// when the job never got a digest to put there. It only ever appears beside
/// an `"infra: …"` outcome, and it is a fixed word, not a short digest.
#[test]
fn getting_a_finished_job_reads_the_report_verbatim() {
    let fixture = serve_swap(0);
    fixture.ctx.slot().finish(
        SWAP_MODEL,
        SwapOutcomeReport {
            outcome: "infra: the candidate weights could not be read".to_string(),
            exit_code: None,
            candidate_gguf_sha: "unread".to_string(),
            floor_sha: "unread".to_string(),
            candidate_profile_path: "/profiles/qwen!swap-candidate.confirm.json".to_string(),
            notes: [NOTE_TASK_GATES, NOTE_HANDOVER],
        },
    );

    let (st, body) = fixture.get();
    assert_eq!(st, 200, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["model"], SWAP_MODEL);
    assert_eq!(v["state"], "done");
    assert_eq!(
        v["report"]["outcome"],
        "infra: the candidate weights could not be read"
    );
    assert!(v["report"]["exit_code"].is_null());
    assert_eq!(v["report"]["candidate_gguf_sha"], "unread");
    assert_eq!(v["report"]["floor_sha"], "unread");
    assert_eq!(
        v["report"]["candidate_profile_path"],
        "/profiles/qwen!swap-candidate.confirm.json"
    );
    assert_eq!(
        v["report"]["notes"],
        serde_json::json!([NOTE_TASK_GATES, NOTE_HANDOVER])
    );
    fixture.handle.shutdown();
}

/// 404: nothing was ever asked about this model. A slot holding some *other*
/// model's job reads the same way — that job says nothing about this name.
#[test]
fn getting_a_job_that_never_started_is_404() {
    let fixture = serve_swap(0);

    let (st, body) = fixture.get();
    assert_eq!(st, 404, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["error"], "no_swap_candidate");
    assert_eq!(v["model"], SWAP_MODEL);

    fixture
        .ctx
        .slot()
        .try_start("some-other-model", Path::new("/models/other.gguf"))
        .expect("the slot starts idle");
    let (st, body) = fixture.get();
    assert_eq!(st, 404, "another model's job is not this model's: {body}");
    fixture.handle.shutdown();
}

/// **The advisory pin** (design §4: "Nothing blocks, nothing auto-swaps").
/// A `not-covered` verdict standing in the slot changes nothing about
/// admission: the named model still admits an agent, exactly as it would have
/// with the slot empty.
#[test]
fn a_not_covered_verdict_never_blocks_admission() {
    let fixture = serve_swap(1);
    fixture.ctx.slot().finish(
        SWAP_MODEL,
        SwapOutcomeReport {
            outcome: "not-covered".to_string(),
            exit_code: Some(1),
            candidate_gguf_sha: "a".repeat(64),
            floor_sha: "b".repeat(64),
            candidate_profile_path: "/profiles/qwen!swap-candidate.transient-abcdef12.json"
                .to_string(),
            notes: [NOTE_TASK_GATES, NOTE_HANDOVER],
        },
    );

    let (st, body) = http(
        &fixture.addr(),
        "POST",
        "/agents",
        r#"{"model":"qwen","budget_tokens":1000}"#,
    );
    assert_eq!(
        st, 201,
        "a swap verdict is evidence for an operator, never an admission gate: {body}"
    );
    fixture.handle.shutdown();
}

/// A daemon served without a swap-candidate context (every `serve`/
/// `serve_shared` caller — test fixtures, embedders) says so by name rather
/// than answering a verdict it has no interpreter, no store and no port to
/// reach.
#[test]
fn a_daemon_wired_without_a_swap_context_says_so() {
    let (port, handle) = bloomery_daemon::test_support::serve_fake();
    let addr = format!("127.0.0.1:{port}");

    for (method, body) in [("POST", r#"{"gguf_path":"/tmp/c.gguf"}"#), ("GET", "")] {
        let (st, response) = http(&addr, method, "/models/qwen/swap-candidate", body);
        assert_eq!(st, 501, "{method}: {response}");
        let v: serde_json::Value = serde_json::from_str(&response).unwrap();
        assert_eq!(v["error"], "swap_candidate_unavailable", "{method}");
        assert_eq!(v["model"], "qwen", "{method}");
    }
    handle.shutdown();
}

/// **Obligation: a panicking worker must never wedge the one slot.** Step 7's
/// cleanup is explicit, not a drop guard, so an unwind past the registration
/// would otherwise leave the slot `Running` for the life of the process —
/// every later candidate answered `candidate_probe_in_progress` for a job
/// nobody can see. The spawn site catches it, finishes the slot with an
/// `infra:` report naming the panic, and the next candidate is admitted.
#[test]
fn a_panicking_candidate_job_never_wedges_the_slot() {
    let fixture = serve_swap(0);
    fixture.seed_floor();
    *fixture.hook.lock().unwrap() = Some(Arc::new(|| panic!("the probe blew up")));

    let (st, body) = fixture.post(&fixture.body());
    assert_eq!(st, 202, "{body}");
    let done = fixture.poll_until_done();
    assert_eq!(done["state"], "done", "{done}");
    let outcome = done["report"]["outcome"].as_str().unwrap_or_default();
    assert!(
        outcome.starts_with("infra:") && outcome.contains("the probe blew up"),
        "the caught panic is named, never rendered as a verdict: {outcome}"
    );
    assert!(
        done["report"]["exit_code"].is_null(),
        "a panic reached no exit code: {done}"
    );

    // The slot admits the next job — the whole point of catching it.
    *fixture.hook.lock().unwrap() = None;
    let (st, body) = fixture.post(&fixture.body());
    assert_eq!(st, 202, "a caught panic must not wedge the slot: {body}");
    let done = fixture.poll_until_done();
    assert_eq!(done["report"]["outcome"], "covered", "{done}");
    fixture.handle.shutdown();
}

/// **Obligation: the worker's `Err` is the only report that cleanup failed.**
/// A failed unregister leaves the scratch identity — possibly still holding
/// weights — standing after the job returned, which is the one thing design §4
/// says must not happen, and nothing in the report says so: the report carries
/// the verdict, which is unaffected. So the spawn site journals it.
///
/// Driven by having the probe remove the scratch registration out from under
/// the job: step 7 then fails against an otherwise healthy pager, which is
/// exactly the shape of the failure this row exists to catch.
#[test]
fn a_failed_cleanup_is_journaled_rather_than_dropped() {
    let fixture = serve_swap(0);
    fixture.seed_floor();
    let pager = Arc::clone(&fixture.pager);
    *fixture.hook.lock().unwrap() = Some(Arc::new(move || {
        pager
            .lock()
            .unwrap()
            .unregister_model(&scratch_identity(SWAP_MODEL))
            .expect("the scratch identity is registered while the probe runs");
    }));

    let (st, body) = fixture.post(&fixture.body());
    assert_eq!(st, 202, "{body}");
    let done = fixture.poll_until_done();
    assert_eq!(
        done["report"]["outcome"], "covered",
        "the verdict itself is unaffected by the cleanup failure: {done}"
    );
    let reasons = fixture.degraded_reasons();
    assert!(
        reasons
            .iter()
            .any(|r| r.contains(&scratch_identity(SWAP_MODEL)) && r.contains("could not")),
        "a failed cleanup is journaled, never dropped: {reasons:?}"
    );
    fixture.handle.shutdown();
}

// ---------------------------------------------------------------------------
// The candidate probe's admission window (bT5/F1). Design §4 step 2 requires
// the probe to reach the candidate "through the daemon's own `/v1` with the
// identical POST invocation", and the live acceptance
// (`docs/superpowers/evidence/2026-08-19-swap-candidate-live.md`) proved that
// path unreachable in production: the scratch identity has no profile — making
// one is the point of the probe — so `Pager::admit` refused it `422`, assay
// exited 4, and `cover` never spawned. Twice, byte-identically.
//
// The rows below are the ones the scripted-probe fixture could not fail on:
// every one runs `SwapCfg::allow_unprofiled = false` (the standing production
// config) so law 5's gate really refuses, and drives the REAL admission path
// over real HTTP. The probe SUBPROCESS is still scripted; the admission it
// exercises is not.
// ---------------------------------------------------------------------------

/// The production config these rows run under: law 5's gate really refuses,
/// and the probe really asks.
fn strict() -> SwapCfg {
    SwapCfg {
        allow_unprofiled: false,
        drive_v1: true,
        ..SwapCfg::default()
    }
}

/// **bT5/F1, the defect itself.** With `allow_unprofiled` unset — the standing
/// config on the box the live acceptance ran on — the candidate probe's own
/// `/v1/chat/completions` must be **admitted**, and the job must reach a
/// coverage verdict.
///
/// Before the fix this row reproduces the live failure exactly: `422` at the
/// door, `assay exited 4: … HTTP 422 …`, `outcome` an `infra:` sentence and
/// `exit_code: null`, because `cover` never ran.
#[test]
fn a_candidate_probe_is_admitted_through_this_daemons_own_v1() {
    let fixture = serve_swap_cfg(0, strict());
    fixture.seed_floor();
    let scratch = scratch_identity(SWAP_MODEL);

    let (st, body) = fixture.post(&fixture.body());
    assert_eq!(st, 202, "{body}");
    let done = fixture.poll_until_done();

    let calls = fixture.v1.lock().unwrap().clone();
    assert_eq!(
        calls,
        vec![(scratch.clone(), 200)],
        "the probe's own request must be admitted under the scratch identity, \
         not refused 422 at the door: {done}"
    );
    assert_eq!(
        done["report"]["outcome"], "covered",
        "an admitted probe reaches a real verdict: {done}"
    );
    assert_eq!(done["report"]["exit_code"], 0, "{done}");

    // And the window went with the job: closed, and the scratch identity is not
    // registered any more, so nothing is addressable under it at all.
    assert!(
        !fixture.window_open(),
        "the window is back to closed once the job ends: {done}"
    );
    let (st, body) = fixture.chat(&scratch);
    assert_eq!(
        st, 404,
        "the scratch identity never outlives the job: {body}"
    );
    fixture.handle.shutdown();
}

/// **The window admits the scratch identity and nothing else.** The near-miss
/// the live evidence names is worse than the failure: a candidate POST fired
/// *inside* the boot POST window would have been admitted by a daemon-global
/// flag, for a reason with nothing to do with this endpoint. So the fix is
/// scoped per identity, and this row is what says so — mid-probe, with the
/// window demonstrably open for the candidate, the configured-but-unprofiled
/// `qwen` is still refused `422`.
#[test]
fn the_candidate_window_admits_the_scratch_identity_and_nothing_else() {
    let fixture = serve_swap_cfg(0, strict());
    fixture.seed_floor();
    let neighbour: Arc<Mutex<Option<(u16, String)>>> = Arc::new(Mutex::new(None));

    let addr = fixture.addr();
    let seen = Arc::clone(&neighbour);
    // Runs inside the probe, i.e. inside the window: the candidate's own call
    // has already been admitted by the time this fires.
    *fixture.hook.lock().unwrap() = Some(Arc::new(move || {
        let answer = http(
            &addr,
            "POST",
            "/v1/chat/completions",
            r#"{"model":"qwen","messages":[{"role":"user","content":"hi"}],"max_tokens":8}"#,
        );
        *seen.lock().expect("neighbour slot") = Some(answer);
    }));

    let (st, body) = fixture.post(&fixture.body());
    assert_eq!(st, 202, "{body}");
    let done = fixture.poll_until_done();
    assert_eq!(done["report"]["outcome"], "covered", "{done}");
    assert_eq!(
        fixture.v1.lock().unwrap().clone(),
        vec![(scratch_identity(SWAP_MODEL), 200)],
        "the candidate itself was admitted: {done}"
    );

    let (st, body) = neighbour
        .lock()
        .unwrap()
        .clone()
        .expect("the hook ran inside the probe");
    assert_eq!(
        st, 422,
        "the window is the candidate's alone — an unprofiled configured model \
         must stay refused while it is open: {body}"
    );
    fixture.handle.shutdown();
}

/// **The window closes with the probe step, not with the job.** Nothing past
/// the probe drives `/v1`, so by the time `cover` runs — with the scratch
/// identity still registered, which is the only moment this is observable —
/// the window must already be shut.
#[test]
fn the_candidate_window_is_closed_before_the_verdict_is_reached() {
    let fixture = serve_swap_cfg(0, strict());
    fixture.seed_floor();
    let scratch = scratch_identity(SWAP_MODEL);
    let after: Arc<Mutex<Option<(u16, String)>>> = Arc::new(Mutex::new(None));

    let addr = fixture.addr();
    let (seen, named) = (Arc::clone(&after), scratch.clone());
    *fixture.cover_hook.lock().unwrap() = Some(Arc::new(move || {
        let answer = http(
            &addr,
            "POST",
            "/v1/chat/completions",
            &serde_json::json!({
                "model": named,
                "messages": [{"role": "user", "content": "hi"}],
                "max_tokens": 8,
            })
            .to_string(),
        );
        *seen.lock().expect("after-probe slot") = Some(answer);
    }));

    let (st, body) = fixture.post(&fixture.body());
    assert_eq!(st, 202, "{body}");
    let done = fixture.poll_until_done();
    assert_eq!(done["report"]["outcome"], "covered", "{done}");

    let (st, body) = after
        .lock()
        .unwrap()
        .clone()
        .expect("the hook ran inside the cover run");
    assert_eq!(
        st, 422,
        "the scratch identity is still registered here, and must already be \
         back under law 5's gate: {body}"
    );
    fixture.handle.shutdown();
}

/// **A failed probe still opens the window and still leaves nothing open.**
/// The job ends on its `infra:` path, and the daemon is back to refusing every
/// unprofiled model — the scratch identity because it is gone, `qwen` because
/// nothing daemon-wide was ever suspended for it.
#[test]
fn a_failed_candidate_probe_leaves_no_window_open() {
    let fixture = serve_swap_cfg(
        0,
        SwapCfg {
            probe_fails: true,
            ..strict()
        },
    );
    fixture.seed_floor();
    let scratch = scratch_identity(SWAP_MODEL);

    let (st, body) = fixture.post(&fixture.body());
    assert_eq!(st, 202, "{body}");
    let done = fixture.poll_until_done();
    let outcome = done["report"]["outcome"].as_str().unwrap_or_default();
    assert!(
        outcome.starts_with("infra:") && outcome.contains("assay exited 4"),
        "a failed probe is named, never rendered as a verdict: {done}"
    );
    assert_eq!(
        fixture.v1.lock().unwrap().clone(),
        vec![(scratch.clone(), 200)],
        "the window opened for the probe even on the path where it failed: {done}"
    );

    assert!(
        !fixture.window_open(),
        "the window is back to closed once the job ends: {done}"
    );
    let (st, body) = fixture.chat(&scratch);
    assert_eq!(
        st, 404,
        "the scratch identity never outlives the job: {body}"
    );
    let (st, body) = fixture.chat(SWAP_MODEL);
    assert_eq!(
        st, 422,
        "law 5's gate is exactly where the job found it: {body}"
    );
    fixture.handle.shutdown();
}

/// **A panicking worker must not leave the window open either.** The unwind
/// skips step 7, so the scratch identity really is still registered afterwards
/// (the spawn site says so and tells the operator to unload it) — and a window
/// left open on that identity would admit it, unprofiled, through `/v1` for the
/// life of the process. The spawn site closes it where it catches the panic.
#[test]
fn a_panicking_candidate_job_closes_the_admission_window() {
    let fixture = serve_swap_cfg(
        0,
        SwapCfg {
            allow_unprofiled: false,
            ..SwapCfg::default()
        },
    );
    fixture.seed_floor();
    let scratch = scratch_identity(SWAP_MODEL);
    *fixture.hook.lock().unwrap() = Some(Arc::new(|| panic!("the probe blew up")));

    let (st, body) = fixture.post(&fixture.body());
    assert_eq!(st, 202, "{body}");
    let done = fixture.poll_until_done();
    let outcome = done["report"]["outcome"].as_str().unwrap_or_default();
    assert!(
        outcome.starts_with("infra:") && outcome.contains("the probe blew up"),
        "the caught panic is named: {done}"
    );

    // The registration survived the unwind — that is the premise, and the
    // reason this path needs a close of its own.
    let (st, body) = http(&fixture.addr(), "GET", "/status", "");
    assert_eq!(st, 200, "{body}");
    let status: serde_json::Value = serde_json::from_str(&body).unwrap();
    let names: Vec<&str> = status["models"]
        .as_array()
        .expect("models")
        .iter()
        .filter_map(|m| m["name"].as_str())
        .collect();
    assert!(
        names.contains(&scratch.as_str()),
        "an unwind past step 2 leaks the registration; this row is about the \
         window on it: {names:?}"
    );

    assert!(
        !fixture.window_open(),
        "an unwind skips the job's own close; the spawn site owes this one"
    );
    let (st, body) = fixture.chat(&scratch);
    assert_eq!(
        st, 422,
        "a leaked registration must not also be a leaked admission: {body}"
    );
    fixture.handle.shutdown();
}

/// **Obligation: an agent minted inside the window must not outlive it.**
/// `unregister_model` used to only *suspend* agents bound to the identity it
/// forgot — they kept their id and their image, and were refused only because
/// no model of that name was registered any more. For a scratch identity that
/// refusal is temporary by construction: the next candidate job for the same
/// model registers exactly that name again, and admission is checked at agent
/// **creation**, so a stale agent would come back usable against a *different*
/// candidate's weights without passing any gate at all. Step 7 evicts instead.
#[test]
fn an_agent_minted_during_the_window_is_evicted_when_the_job_ends() {
    let fixture = serve_swap_cfg(0, strict());
    fixture.seed_floor();
    let minted: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));

    let (addr, seen) = (fixture.addr(), Arc::clone(&minted));
    // Inside the probe, i.e. inside the window: this is admitted for exactly
    // the same reason the probe's own call is.
    *fixture.hook.lock().unwrap() = Some(Arc::new(move || {
        let (st, body) = http(
            &addr,
            "POST",
            "/agents",
            &serde_json::json!({
                "model": scratch_identity(SWAP_MODEL),
                "budget_tokens": 1000,
            })
            .to_string(),
        );
        assert_eq!(st, 201, "the window admits agent creation too: {body}");
        let id = serde_json::from_str::<serde_json::Value>(&body).unwrap()["id"]
            .as_str()
            .expect("a created agent has an id")
            .to_string();
        *seen.lock().expect("minted slot") = Some(id);
    }));

    let (st, body) = fixture.post(&fixture.body());
    assert_eq!(st, 202, "{body}");
    let done = fixture.poll_until_done();
    assert_eq!(done["report"]["outcome"], "covered", "{done}");
    let id = minted.lock().unwrap().clone().expect("the hook ran");

    // GONE from the table, not merely refused: `/status` is the table.
    let (st, body) = http(&fixture.addr(), "GET", "/status", "");
    assert_eq!(st, 200, "{body}");
    let status: serde_json::Value = serde_json::from_str(&body).unwrap();
    let ids: Vec<&str> = status["agents"]
        .as_array()
        .expect("agents")
        .iter()
        .filter_map(|a| a["id"].as_str())
        .collect();
    assert!(
        !ids.contains(&id.as_str()),
        "an agent bound to the scratch identity cannot outlive it: {ids:?}"
    );
    let (st, body) = http(
        &fixture.addr(),
        "POST",
        &format!("/agents/{id}/infer"),
        r#"{"prompt":"hi","max_tokens":8}"#,
    );
    assert_eq!(st, 404, "the id is forgotten, not parked: {body}");
    fixture.handle.shutdown();
}

/// **Obligation: a second job revives nothing from the first.** Two things
/// have to hold across the re-registration that job 2 performs under the very
/// same scratch name: job 2 starts with a closed window (the structural
/// argument — a fresh entry's window is shut), and job 1's agent is not
/// waiting in the table to be revived by it (the eviction argument). The
/// second is checked from *inside* job 2's window, which is the exact moment a
/// survivor would become usable against the new candidate's weights.
#[test]
fn a_second_candidate_job_revives_nothing_from_the_first() {
    let fixture = serve_swap_cfg(0, strict());
    fixture.seed_floor();
    let minted: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));

    let (addr, seen) = (fixture.addr(), Arc::clone(&minted));
    *fixture.hook.lock().unwrap() = Some(Arc::new(move || {
        let (st, body) = http(
            &addr,
            "POST",
            "/agents",
            &serde_json::json!({
                "model": scratch_identity(SWAP_MODEL),
                "budget_tokens": 1000,
            })
            .to_string(),
        );
        assert_eq!(st, 201, "{body}");
        *seen.lock().expect("minted slot") = Some(
            serde_json::from_str::<serde_json::Value>(&body).unwrap()["id"]
                .as_str()
                .expect("a created agent has an id")
                .to_string(),
        );
    }));

    let (st, body) = fixture.post(&fixture.body());
    assert_eq!(st, 202, "{body}");
    let done = fixture.poll_until_done();
    assert_eq!(done["report"]["outcome"], "covered", "job 1: {done}");
    let id = minted.lock().unwrap().clone().expect("the hook ran");
    assert!(
        !fixture.window_open(),
        "job 2 must start from a closed window"
    );

    // Job 2, same model, same scratch name — the re-registration that would
    // revive a survivor. The probe hook asks whether job 1's agent works now.
    let revived: Arc<Mutex<Option<(u16, String)>>> = Arc::new(Mutex::new(None));
    let (addr, seen, stale) = (fixture.addr(), Arc::clone(&revived), id.clone());
    *fixture.hook.lock().unwrap() = Some(Arc::new(move || {
        let answer = http(
            &addr,
            "POST",
            &format!("/agents/{stale}/infer"),
            r#"{"prompt":"hi","max_tokens":8}"#,
        );
        *seen.lock().expect("revived slot") = Some(answer);
    }));

    let (st, body) = fixture.post(&fixture.body());
    assert_eq!(st, 202, "{body}");
    let done = fixture.poll_until_done();
    assert_eq!(done["report"]["outcome"], "covered", "job 2: {done}");

    let (st, body) = revived
        .lock()
        .unwrap()
        .clone()
        .expect("the hook ran inside job 2's probe");
    assert_eq!(
        st, 404,
        "job 1's agent must not come back usable against job 2's candidate, \
         which is what a re-registered name would otherwise do — admission is \
         checked at creation, and this agent would never be created again: {body}"
    );
    assert!(!fixture.window_open(), "the window closed with job 2 too");
    fixture.handle.shutdown();
}
