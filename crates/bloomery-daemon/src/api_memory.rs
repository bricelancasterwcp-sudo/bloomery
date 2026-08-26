//! Route table for the memory organ's operator surface (memory-organ Task 9):
//! `GET /memory` and `DELETE /memory/{id}` — spec §6's "the operator can list
//! and evict episodes" requirement.
//!
//! Both routes are dark by default, in exactly the shape `api_task.rs`'s
//! module doc describes for `tasks_enabled`: [`dispatch`] gates on
//! [`MemoryContext::operational`]'s two halves — no context at all, or the
//! `[memory] enabled` switch off — before looking at the method or the id,
//! so a garbage id or the wrong verb against a dark organ still reads as
//! "memory is off", never "not found" or "method not allowed". A context
//! that is *enabled* but whose store failed to load at boot
//! (`disabled_reason`) answers `503` instead of `501` — the organ is
//! configured on, but there is genuinely nothing behind it to list or purge
//! (spec §7's "disabled-with-reason").
//!
//! [`dispatch`] returns `None` for any path that isn't one of these two
//! routes, so `http.rs`'s worker loop falls through to `api_native::dispatch`
//! for everything else — the same `None`-falls-through contract
//! `api_task::dispatch` gives its own two routes.

use std::sync::Arc;

use serde_json::{json, Value};

use crate::api_native::ApiResult;
use crate::memory::MemoryContext;

/// Routes one memory-surface request. `segments` is the same `/`-split,
/// non-empty path every other route table matches on, already collected to
/// `&str` (unlike `api_task`/`api_native`'s `&[String]`, since this surface
/// takes no id-shaped argument that needs owning past the call — both routes
/// answer synchronously, nothing here spawns a worker that outlives the
/// request).
pub fn dispatch(
    memory: Option<&Arc<MemoryContext>>,
    method: &str,
    segments: &[&str],
) -> Option<ApiResult> {
    match segments {
        ["memory"] => Some(collection_route(memory, method)),
        ["memory", id] => Some(item_route(memory, method, id)),
        _ => None,
    }
}

/// The dark-and-broken gate, shared by both routes. `Ok` only when the organ
/// is enabled *and* has a live store to talk to — everything downstream of
/// this can assume `ctx.store` is `Some` (see [`MemoryContext`]'s own doc
/// comment: `store` is `None` exactly when `disabled_reason` is `Some`, and
/// this returns `Err` before ever reaching that arm).
///
/// Checked before the method and before the id, for every caller — the dark
/// rule is "before anything else" (spec §6), not just before the store read.
fn gate(memory: Option<&Arc<MemoryContext>>) -> Result<&MemoryContext, ApiResult> {
    match memory {
        None => Err(memory_disabled()),
        Some(ctx) if !ctx.enabled => Err(memory_disabled()),
        Some(ctx) => match &ctx.disabled_reason {
            Some(reason) => Err(memory_unavailable(reason)),
            None => Ok(ctx.as_ref()),
        },
    }
}

fn collection_route(memory: Option<&Arc<MemoryContext>>, method: &str) -> ApiResult {
    let ctx = match gate(memory) {
        Ok(ctx) => ctx,
        Err(resp) => return resp,
    };
    match method {
        "GET" => list_episodes(ctx),
        _ => method_not_allowed(),
    }
}

fn item_route(memory: Option<&Arc<MemoryContext>>, method: &str, id: &str) -> ApiResult {
    let ctx = match gate(memory) {
        Ok(ctx) => ctx,
        Err(resp) => return resp,
    };
    match method {
        "DELETE" => delete_episode(ctx, id),
        _ => method_not_allowed(),
    }
}

fn memory_disabled() -> ApiResult {
    (501, Some(json!({"error": "memory_disabled"})))
}

fn memory_unavailable(reason: &str) -> ApiResult {
    (
        503,
        Some(json!({"error": "memory_unavailable", "reason": reason})),
    )
}

fn method_not_allowed() -> ApiResult {
    (405, Some(json!({"error": "method_not_allowed"})))
}

/// `GET /memory` — every live episode, operator display fields only (spec
/// §6): `episode_id`, `goal_text`, `cited_paths` (just the paths off
/// `cited_files`, not their fingerprints — an operator deciding whether to
/// evict an episode needs to know *which files*, not their content hashes),
/// `status`, `minted_at`, `minted_by_model`. Deliberately never
/// `landed_patches`, `run_evidence`, `trajectory`, `goal_hash`,
/// `minted_by_envelope`, or `contradicted_by` — those are retrieval/mint
/// internals, not this surface's contract.
///
/// Poison-recovered (`unwrap_or_else(PoisonError::into_inner)`) — same
/// discipline as `api_native::status`'s own store-mutex read: a poisoned
/// *store* mutex means an earlier request's worker panicked mid-mutation of
/// the store, which does not taint the pager, so this surface keeps
/// answering rather than joining the pager's sticky-poison 500.
fn list_episodes(ctx: &MemoryContext) -> ApiResult {
    let store = ctx
        .store
        .as_ref()
        .expect(
            "gate() only returns Ok(ctx) when ctx.disabled_reason is None, which by \
                 MemoryContext's own invariant means ctx.store is Some",
        )
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let episodes: Vec<Value> = store
        .episodes()
        .map(|e| {
            json!({
                "episode_id": e.episode_id,
                "goal_text": e.goal_text,
                "cited_paths": e.cited_files.iter().map(|f| f.path.clone()).collect::<Vec<_>>(),
                "status": e.status,
                "minted_at": e.minted_at,
                "minted_by_model": e.minted_by_model,
            })
        })
        .collect();
    (200, Some(json!({"episodes": episodes})))
}

/// `DELETE /memory/{id}` — the operator's manual eviction (spec §6):
/// tombstones `id` durably via [`crate::memory::store::MemoryStore::delete`].
///
/// | outcome | status | body |
/// |---|---|---|
/// | tombstoned | 200 | `{"deleted": id}` |
/// | unknown id | 404 | `{"error": "not_found"}` |
/// | the append itself failed | 500 | `{"error": "store_io", "detail": ...}` |
///
/// **This never tombstones the *identity*, only the current live row.** Spec
/// §6: a later verified completion of the same task may legitimately re-mint
/// the same `episode_id` (identity is goal + cited-file fingerprints, not
/// "has an operator ever deleted this before") — nothing here or in the
/// store records "this id was operator-deleted" as a permanent fact, so a
/// re-mint after this delete is not a bug to guard against.
fn delete_episode(ctx: &MemoryContext, id: &str) -> ApiResult {
    let mut store = ctx
        .store
        .as_ref()
        .expect(
            "gate() only returns Ok(ctx) when ctx.disabled_reason is None, which by \
                 MemoryContext's own invariant means ctx.store is Some",
        )
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    match store.delete(id) {
        Ok(true) => (200, Some(json!({"deleted": id}))),
        Ok(false) => (404, Some(json!({"error": "not_found"}))),
        Err(e) => (
            500,
            Some(json!({"error": "store_io", "detail": e.to_string()})),
        ),
    }
}
