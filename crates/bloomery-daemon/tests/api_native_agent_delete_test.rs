//! `DELETE /agents/{id}` — the operator lifecycle endpoint the OpenAI-tools
//! adapter's live acceptance run (2026-08-31) needed and did not have. That
//! run left seven agents behind and cleared them by hand with
//! `POST /agents/{id}/suspend`, which *parks* an agent rather than removing
//! it: the id, its table entry and its KV image all survive a suspend.
//!
//! Split out of `api_native_test.rs` rather than appended to it. That file is
//! 2505 lines — the single worst offender against the 800-line ceiling this
//! project's carried debt already names — and a later slice has to split it;
//! growing it here would be work that slice then has to undo.
//!
//! `Pager::remove_agent`'s own semantics (resident context destroyed, image
//! dropped, id forgotten, reason journaled) are already pinned at the pager
//! layer by `pager_remove_agent_test.rs`. What is pinned *here* is the HTTP
//! surface it is reached through: the route arm, the status codes, the
//! refusal shape, and the reason this layer supplies.

mod common;

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use bloomery_core::journal::{Event, Journal};
use bloomery_daemon::agents::ImageStore;
use bloomery_daemon::http::{serve_shared, ServerHandle};
use bloomery_daemon::pager::Pager;
use bloomery_substrate::fake::FakeSubstrate;
use bloomery_substrate::Reply;
use common::http;

/// The journaled reason this layer supplies for an operator-initiated
/// removal, spelled here a second time on purpose.
///
/// It is a durable record an operator reads out of the journal to tell *why*
/// an agent left the table — a removal by this endpoint from an eviction, a
/// `/v1` ephemeral cleanup, or an `unregister_model` cascade. Duplicating the
/// literal rather than importing it (`api_native` is a private module, so an
/// integration test could not import it anyway) means a silent reword fails
/// here instead of quietly changing what the audit trail says.
const OPERATOR_DELETE_REASON: &str = "operator requested removal via DELETE /agents/{id}";

/// Creates an agent over HTTP and returns its id.
fn create_agent(addr: &str) -> String {
    let (st, body) = http(
        addr,
        "POST",
        "/agents",
        r#"{"model":"qwen","budget_tokens":1000}"#,
    );
    assert_eq!(st, 201, "{body}");
    serde_json::from_str::<serde_json::Value>(&body).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string()
}

/// A scratch dir unique to this process and call, so parallel tests never
/// share a journal or image store.
fn fresh_dir(name: &str) -> PathBuf {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("bloomery-{name}-{}-{seq}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

/// A served pager the test still holds a handle on, for the two tests that
/// need more than `serve_fake` exposes: the journal path (to read back what a
/// removal recorded) and the `Arc` itself (to poison the mutex on purpose).
///
/// The scratch dir is registered with the handle, so `shutdown()` removes it —
/// read the journal *before* shutting down.
fn serve_own_pager(
    name: &str,
) -> (
    String,
    ServerHandle,
    PathBuf,
    Arc<Mutex<Pager<FakeSubstrate>>>,
) {
    let dir = fresh_dir(name);
    let journal = Journal::open(&dir.join("j.jsonl")).expect("journal opens");
    let images = ImageStore::new(&dir.join("img")).expect("image store opens");

    let mut fake = FakeSubstrate::new();
    fake.script_reply(Reply {
        text: "ok".into(),
        prompt_tokens: Some(8),
        completion_tokens: Some(4),
        duration_ms: 3,
    });

    let mut pager = Pager::new(fake, journal, images, Box::new(|| Some(1024 * 1024 * 1024)));
    let gguf = dir.join("qwen.gguf");
    std::fs::write(&gguf, b"fake weights").expect("write fixture gguf");
    pager
        .register_model(
            "qwen",
            &gguf,
            bloomery_core::gguf::GgufMeta {
                arch: "qwen2".into(),
                layers: 28,
                attention_layers: 28,
                kv_heads: 4,
                head_dim: 128,
                training_ctx: 4096,
                weights_bytes: 1000,
                value_length: None,
                recurrent_state_bytes: 0,
            },
            None,
        )
        .expect("register fixture model");

    let shared = Arc::new(Mutex::new(pager));
    let (port, mut handle) = serve_shared(Arc::clone(&shared), 0);
    handle.set_scratch_dir(dir.clone());
    (format!("127.0.0.1:{port}"), handle, dir, shared)
}

/// The endpoint's whole point: a resident agent is *gone* afterwards, not
/// parked. The id no longer resolves — the exact difference from `suspend`,
/// which the adapter run had to use in its place.
#[test]
fn deleting_a_resident_agent_is_204_and_the_id_is_then_gone() {
    let (port, handle) = bloomery_daemon::test_support::serve_fake();
    let addr = format!("127.0.0.1:{port}");
    let id = create_agent(&addr);

    // Infer once so the agent is Resident, not Fresh: this is the case that
    // has a substrate context to destroy.
    let (st, body) = http(
        &addr,
        "POST",
        &format!("/agents/{id}/infer"),
        r#"{"prompt":"hi","max_tokens":4}"#,
    );
    assert_eq!(st, 200, "agent must be resident before the delete: {body}");

    let (st, body) = http(&addr, "DELETE", &format!("/agents/{id}"), "");
    assert_eq!(st, 204, "{body}");
    assert!(body.is_empty(), "204 carries no body, got {body:?}");

    // Gone, not parked: unlike a suspended agent, this id resolves to nothing.
    let (st, body) = http(
        &addr,
        "POST",
        &format!("/agents/{id}/infer"),
        r#"{"prompt":"hi","max_tokens":4}"#,
    );
    assert_eq!(st, 404, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["error"], "unknown_agent");
    handle.shutdown();
}

/// A `Fresh` agent has no context to destroy. Removing it is not an error —
/// `paging::destroy_context` returns `Ok(())` for a non-resident agent, and
/// "delete something that was never used" is an ordinary operator action, not
/// an edge case to refuse.
#[test]
fn deleting_a_fresh_agent_is_204() {
    let (port, handle) = bloomery_daemon::test_support::serve_fake();
    let addr = format!("127.0.0.1:{port}");
    let id = create_agent(&addr);

    let (st, body) = http(&addr, "DELETE", &format!("/agents/{id}"), "");
    assert_eq!(st, 204, "{body}");
    handle.shutdown();
}

/// A suspended agent holds a parked KV image. Deleting it drops that image
/// along with the entry — the adapter run's actual situation, since it had
/// suspended its leaked agents before wanting them gone.
#[test]
fn deleting_a_suspended_agent_is_204() {
    let (port, handle) = bloomery_daemon::test_support::serve_fake();
    let addr = format!("127.0.0.1:{port}");
    let id = create_agent(&addr);

    let (st, body) = http(
        &addr,
        "POST",
        &format!("/agents/{id}/infer"),
        r#"{"prompt":"hi","max_tokens":4}"#,
    );
    assert_eq!(st, 200, "{body}");
    let (st, body) = http(&addr, "POST", &format!("/agents/{id}/suspend"), "");
    assert_eq!(st, 204, "{body}");

    let (st, body) = http(&addr, "DELETE", &format!("/agents/{id}"), "");
    assert_eq!(st, 204, "{body}");
    handle.shutdown();
}

/// An unknown id gets the pager's own named refusal, *not* the router's
/// `not_found`. The two are different facts — "there is no such agent" versus
/// "there is no such route" — and an operator debugging a 404 needs to know
/// which one they hit.
#[test]
fn deleting_an_unknown_id_is_404_unknown_agent_not_the_routers_not_found() {
    let (port, handle) = bloomery_daemon::test_support::serve_fake();
    let addr = format!("127.0.0.1:{port}");

    let (st, body) = http(&addr, "DELETE", "/agents/nope", "");
    assert_eq!(st, 404, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["error"], "unknown_agent", "{body}");
    assert_eq!(v["agent"], "nope", "the refusal names the id it looked for");
    handle.shutdown();
}

/// A second DELETE is a 404, deliberately — not an idempotent 204.
///
/// The alternative (answer 204 whether or not anything was there) would
/// assert "removed" about an id that never existed: a success envelope over
/// nothing, which is the same class of dishonesty the `/v1` unimplemented-field
/// slice removed from this daemon on the same day this endpoint was added.
/// DELETE stays idempotent in the sense that matters — repeating it leaves the
/// state identical — without claiming to have done something it did not do.
#[test]
fn a_second_delete_is_404_not_a_silent_success() {
    let (port, handle) = bloomery_daemon::test_support::serve_fake();
    let addr = format!("127.0.0.1:{port}");
    let id = create_agent(&addr);

    let (st, body) = http(&addr, "DELETE", &format!("/agents/{id}"), "");
    assert_eq!(st, 204, "{body}");

    let (st, body) = http(&addr, "DELETE", &format!("/agents/{id}"), "");
    assert_eq!(st, 404, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["error"], "unknown_agent", "{body}");
    handle.shutdown();
}

/// The removal reaches the journal with a reason naming the surface that
/// asked for it, so an operator reading the journal can tell an operator
/// deletion from a `/v1` ephemeral cleanup or an `unregister_model` cascade —
/// all three land as the same `AgentRemoved` event and are distinguishable
/// only by this string.
#[test]
fn delete_journals_the_removal_with_a_reason_naming_the_surface() {
    let (addr, handle, dir, _shared) = serve_own_pager("agent-delete-journal");
    let id = create_agent(&addr);

    let (st, body) = http(&addr, "DELETE", &format!("/agents/{id}"), "");
    assert_eq!(st, 204, "{body}");

    // Read before shutdown: the handle owns the scratch dir and removes it.
    let events = bloomery_core::journal::replay(&dir.join("j.jsonl")).expect("journal replays");
    assert!(
        events.iter().any(|e| matches!(
            e,
            Event::AgentRemoved { id: rid, reason }
                if rid == &id && reason == OPERATOR_DELETE_REASON
        )),
        "no AgentRemoved for {id} carrying the operator reason; events: {events:?}"
    );
    handle.shutdown();
}

/// The route arm matches the bare `/agents/{id}` path and nothing else. A
/// DELETE aimed at the collection, or at one of the POST sub-resources, is a
/// routing miss — guarding against an over-broad arm that would, say, treat
/// `DELETE /agents/{id}/suspend` as a removal request.
#[test]
fn delete_is_matched_only_on_the_bare_agent_path() {
    let (port, handle) = bloomery_daemon::test_support::serve_fake();
    let addr = format!("127.0.0.1:{port}");
    let id = create_agent(&addr);

    for path in [
        "/agents".to_string(),
        format!("/agents/{id}/suspend"),
        format!("/agents/{id}/infer"),
    ] {
        let (st, body) = http(&addr, "DELETE", &path, "");
        assert_eq!(st, 404, "DELETE {path} must not route: {body}");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["error"], "not_found", "DELETE {path}: {body}");
    }

    // The agent is untouched by all of that — still deletable on its own path.
    let (st, body) = http(&addr, "DELETE", &format!("/agents/{id}"), "");
    assert_eq!(st, 204, "{body}");
    handle.shutdown();
}

/// A poisoned pager mutex must reach this route as the same sticky named 500
/// every other route gives, never a panic or a hang.
///
/// `lock_pager`'s poison contract is per-request rather than per-route, so a
/// newly added route is exactly where it could be forgotten. The poison is
/// induced by panicking a thread that holds the lock — the test keeps its own
/// clone of the served `Arc` — rather than by scripting a panicking substrate,
/// which would mean duplicating `api_native_test.rs`'s ~120-line harness for
/// one assertion.
#[test]
fn delete_on_a_poisoned_pager_is_a_named_500() {
    let (addr, handle, _dir, shared) = serve_own_pager("agent-delete-poison");
    let id = create_agent(&addr);

    let poisoner = Arc::clone(&shared);
    let panicked = std::thread::spawn(move || {
        let _guard = poisoner.lock().expect("lock is not poisoned yet");
        panic!("deliberate panic while holding the pager lock: poisons the mutex");
    })
    .join();
    assert!(panicked.is_err(), "the poisoning thread must have panicked");

    let (st, body) = http(&addr, "DELETE", &format!("/agents/{id}"), "");
    assert_eq!(st, 500, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["error"], "internal", "{body}");
    assert!(
        v["detail"].as_str().unwrap().contains("poisoned"),
        "the 500 must say what happened: {body}"
    );
    handle.shutdown();
}
