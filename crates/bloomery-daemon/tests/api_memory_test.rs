//! Failing-first tests for the memory operator routes (memory-organ Task 9):
//! `GET /memory` and `DELETE /memory/{id}`.
//!
//! Written before `src/api_memory.rs` exists, per the task brief's Step 1 —
//! the whole file fails to compile (no such module) until Step 3 lands. That
//! compile failure is this task's captured RED.
//!
//! These drive `api_memory::dispatch` directly (no HTTP server, no
//! `tests/common::http`) — the brief's explicit Step 1 instruction for this
//! task, since the two routes never need a spawned worker or a real socket
//! the way `api_task`'s create-task route does.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use bloomery_daemon::api_memory::dispatch;
use bloomery_daemon::config::MemoryConfig;
use bloomery_daemon::memory::record::{CitedFile, EpisodeRecord, Fingerprint, RunEvidence};
use bloomery_daemon::memory::store::MemoryStore;
use bloomery_daemon::memory::{build_memory, MemoryContext};

/// One fresh scratch dir per test — same PID + atomic-counter disambiguation
/// as `memory_store_test.rs::fresh_dir` and `memory.rs`'s own test helper.
fn fresh_dir(tag: &str) -> PathBuf {
    static UNIQUE: AtomicU64 = AtomicU64::new(0);
    let unique = UNIQUE.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "bloomery-api-memory-test-{tag}-{}-{unique}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A minimal verified episode — same shape as `memory_store_test.rs::ep`.
fn ep(id_seed: &str, goal_hash: &str, goal_text: &str, minted_at: u64) -> EpisodeRecord {
    EpisodeRecord {
        episode_id: id_seed.into(),
        goal_hash: goal_hash.into(),
        goal_text: goal_text.into(),
        cited_files: vec![CitedFile {
            path: "/w/main.rs".into(),
            fingerprint: Fingerprint::Sha256("abc123".into()),
        }],
        landed_patches: vec![],
        run_evidence: RunEvidence {
            argv: vec!["cargo".into(), "test".into()],
            outcome: "PASSED".into(),
        },
        trajectory: vec!["step1".into()],
        minted_by_model: "test-model".into(),
        minted_by_envelope: "v1".into(),
        status: "verified".into(),
        contradicted_by: None,
        minted_at,
    }
}

/// An operational context (`enabled: true`, store loads cleanly) on a fresh
/// tempdir — the store's own file path is returned alongside so a test can
/// reload it directly to check durability, the same way `memory_store_test.rs`
/// reloads a fresh `MemoryStore::load` after every mutation.
fn operational_ctx(tag: &str) -> (Arc<MemoryContext>, PathBuf) {
    let dir = fresh_dir(tag);
    let cfg = MemoryConfig {
        enabled: true,
        max_episodes: 256,
        refalsify: false,
    };
    let ctx = build_memory(&cfg, &dir);
    assert!(
        ctx.operational(),
        "disabled_reason: {:?}",
        ctx.disabled_reason
    );
    let store_path = dir.join("memory").join("episodes.jsonl");
    (ctx, store_path)
}

fn mint(ctx: &MemoryContext, rec: EpisodeRecord) {
    ctx.store
        .as_ref()
        .expect("operational_ctx built a store")
        .lock()
        .unwrap()
        .mint(rec, ctx.max_episodes)
        .unwrap();
}

// ---------------------------------------------------------------------------
// Dark rule: memory=None or ctx.enabled=false -> 501, before anything else.
// ---------------------------------------------------------------------------

#[test]
fn no_context_is_501_on_get_list_even_with_garbage_method() {
    let (status, body) = dispatch(None, "GET", &["memory"]).expect("route matches");
    assert_eq!(status, 501);
    assert_eq!(body.unwrap()["error"], "memory_disabled");
}

#[test]
fn no_context_is_501_on_delete_with_a_garbage_id() {
    let (status, body) = dispatch(
        None,
        "DELETE",
        &["memory", "definitely-not-a-real-episode-id"],
    )
    .expect("route matches");
    assert_eq!(status, 501);
    assert_eq!(body.unwrap()["error"], "memory_disabled");
}

#[test]
fn config_disabled_is_501_on_both_routes_even_with_the_wrong_method() {
    let dir = fresh_dir("config-disabled");
    let cfg = MemoryConfig {
        enabled: false,
        max_episodes: 256,
        refalsify: false,
    };
    let ctx = build_memory(&cfg, &dir);
    assert!(!ctx.operational());

    // A malformed/garbage-id DELETE against a disabled organ still reads as
    // "memory is off", never "id not found" or "method not allowed" — the
    // dark rule runs before the id is even looked at and before the method
    // is dispatched.
    let (status, body) = dispatch(Some(&ctx), "DELETE", &["memory", "garbage-id"]).unwrap();
    assert_eq!(status, 501);
    assert_eq!(body.unwrap()["error"], "memory_disabled");

    let (status, body) = dispatch(Some(&ctx), "GET", &["memory"]).unwrap();
    assert_eq!(status, 501);
    assert_eq!(body.unwrap()["error"], "memory_disabled");

    // Even a request whose method would otherwise be a 405 stays 501 while
    // the organ is dark.
    let (status, body) = dispatch(Some(&ctx), "POST", &["memory"]).unwrap();
    assert_eq!(status, 501);
    assert_eq!(body.unwrap()["error"], "memory_disabled");
}

/// Enabled by config but the store failed to load at boot — the
/// `disabled_reason` case, distinct from the plain dark rule: `503`, not
/// `501`, and the reason string is surfaced.
#[test]
fn enabled_but_broken_store_is_503_with_the_reason() {
    let dir = fresh_dir("broken-store");
    // Force the store path itself to be a directory, the same trick
    // `memory.rs`'s own
    // `build_memory_disabled_reason_when_store_path_is_a_directory` test
    // uses to reach `MemoryStore::load`'s hard `io::Error` arm.
    std::fs::create_dir_all(dir.join("memory").join("episodes.jsonl")).unwrap();
    let cfg = MemoryConfig {
        enabled: true,
        max_episodes: 256,
        refalsify: false,
    };
    let ctx = build_memory(&cfg, &dir);
    assert!(!ctx.operational());
    assert!(ctx.disabled_reason.is_some());

    let (status, body) = dispatch(Some(&ctx), "GET", &["memory"]).unwrap();
    assert_eq!(status, 503);
    let body = body.unwrap();
    assert_eq!(body["error"], "memory_unavailable");
    assert_eq!(body["reason"], ctx.disabled_reason.clone().unwrap());

    let (status, body) = dispatch(Some(&ctx), "DELETE", &["memory", "any-id"]).unwrap();
    assert_eq!(status, 503);
    assert_eq!(body.unwrap()["error"], "memory_unavailable");
}

// ---------------------------------------------------------------------------
// Operational: GET list and DELETE purge.
// ---------------------------------------------------------------------------

#[test]
fn operational_and_fresh_get_list_is_empty() {
    let (ctx, _path) = operational_ctx("fresh-list");
    let (status, body) = dispatch(Some(&ctx), "GET", &["memory"]).unwrap();
    assert_eq!(status, 200);
    assert_eq!(body.unwrap(), serde_json::json!({"episodes": []}));
}

#[test]
fn after_a_mint_get_list_carries_exactly_the_operator_display_fields() {
    let (ctx, _path) = operational_ctx("mint-list");
    mint(&ctx, ep("e1", "gh1", "implement feature X", 12345));

    let (status, body) = dispatch(Some(&ctx), "GET", &["memory"]).unwrap();
    assert_eq!(status, 200);
    let body = body.unwrap();
    assert_eq!(
        body,
        serde_json::json!({
            "episodes": [{
                "episode_id": "e1",
                "goal_text": "implement feature X",
                "cited_paths": ["/w/main.rs"],
                "status": "verified",
                "minted_at": 12345,
                "minted_by_model": "test-model",
            }]
        }),
        "GET /memory must carry exactly the operator display fields, nothing else"
    );
}

#[test]
fn delete_purges_and_the_deletion_is_durable_across_a_store_reload() {
    let (ctx, path) = operational_ctx("delete-durable");
    mint(&ctx, ep("e1", "gh1", "implement feature X", 1));

    let (status, body) = dispatch(Some(&ctx), "DELETE", &["memory", "e1"]).unwrap();
    assert_eq!(status, 200);
    assert_eq!(body.unwrap(), serde_json::json!({"deleted": "e1"}));

    let (status, body) = dispatch(Some(&ctx), "GET", &["memory"]).unwrap();
    assert_eq!(status, 200);
    assert_eq!(body.unwrap(), serde_json::json!({"episodes": []}));

    // Durability: a fresh `MemoryStore::load` off the same file — not the
    // live in-memory context — must also see the tombstone. This is what
    // proves the delete route actually calls through to the store's durable
    // tombstone append, not just an in-memory removal.
    let reloaded = MemoryStore::load(&path).unwrap();
    assert_eq!(
        reloaded.episodes().count(),
        0,
        "delete must be durable across a store reload"
    );
}

#[test]
fn delete_unknown_id_is_404() {
    let (ctx, _path) = operational_ctx("delete-unknown");
    let (status, body) = dispatch(Some(&ctx), "DELETE", &["memory", "no-such-episode"]).unwrap();
    assert_eq!(status, 404);
    assert_eq!(body.unwrap(), serde_json::json!({"error": "not_found"}));
}

// ---------------------------------------------------------------------------
// Method / route-table edges.
// ---------------------------------------------------------------------------

#[test]
fn post_on_the_collection_route_is_405() {
    let (ctx, _path) = operational_ctx("method-405-collection");
    let (status, body) = dispatch(Some(&ctx), "POST", &["memory"]).unwrap();
    assert_eq!(status, 405);
    assert_eq!(
        body.unwrap(),
        serde_json::json!({"error": "method_not_allowed"})
    );
}

#[test]
fn get_on_the_item_route_is_405() {
    let (ctx, _path) = operational_ctx("method-405-item");
    let (status, body) = dispatch(Some(&ctx), "GET", &["memory", "e1"]).unwrap();
    assert_eq!(status, 405);
    assert_eq!(
        body.unwrap(),
        serde_json::json!({"error": "method_not_allowed"})
    );
}

#[test]
fn a_path_that_is_not_the_memory_surface_falls_through_as_none() {
    let (ctx, _path) = operational_ctx("fallthrough");
    assert!(dispatch(Some(&ctx), "GET", &["memories"]).is_none());
    assert!(dispatch(Some(&ctx), "GET", &["memory", "e1", "extra"]).is_none());
    assert!(dispatch(Some(&ctx), "GET", &[]).is_none());
}
