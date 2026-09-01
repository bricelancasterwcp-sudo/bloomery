//! Native HTTP API integration tests: core routes and the error-code
//! mapping table, plus `/status`.
//!
//! Every request goes over a real `TcpStream` (`tests/common::http`) against a
//! `serve_fake()`-provided ephemeral port, driving `Pager<FakeSubstrate>` end
//! to end.
//!
//! The first test is the Task 14 brief's own, verbatim. The rest pin the
//! error-code mapping table the brief's test didn't cover: 402 (budget), 409
//! (residency refusal, with the exact arithmetic), 404 (unknown agent), the
//! suspend/resume 204 round-trip, and the cold-switch bench's `unload` 204.
//!
//! **Split 2026-09-01** (carried-debt slice D): this file was 2505 lines, the
//! worst offender against the project's 800-line ceiling, and two earlier
//! slices put their tests elsewhere rather than grow it further. Its other
//! sections now live in `api_native_poison_test.rs`,
//! `api_native_bless_unblock_test.rs`, `api_native_swap_candidate_test.rs`
//! and `api_native_swap_window_test.rs`. Fixtures needed by more than one of
//! them moved to `tests/common/native.rs`.

mod common;

use std::path::PathBuf;

use bloomery_daemon::config::MemoryConfig;
use bloomery_daemon::memory::build_memory;
use common::http;
use common::native::serve_drift_blocked_qwen;

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
        value_length: None,
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
