//! Route table for the native (non-OpenAI-compatible) HTTP surface: parse a
//! JSON body, call into `Pager<S>`, and map the result to `(status, body)`.
//!
//! [`map_error`] is the one place the Task 14 brief's error-code table
//! lives — every [`PagerError`] variant maps to exactly one status code and
//! JSON shape, so a refusal is always structured JSON with the arithmetic
//! that produced it, never a truncated success (law 2).
//!
//! One route answers off a different error type: [`bless`] maps
//! [`BlessError`], whose variants are not `PagerError`'s, and carries its own
//! small table in its doc comment. Its unknown-model arm still goes through
//! [`map_error`], so that 404's shape has exactly one spelling on this surface.
//!
//! **Split 2026-09-01** (carried-debt slice D). This file was 816 lines, and
//! slice 1's own `DELETE /agents/{id}` handler is what pushed it over the
//! project's 800-line ceiling -- recorded in `docs/CARRIED-DEBT.md` rather
//! than quietly absorbed. The route handlers now live beside the route table
//! rather than inside it:
//!
//! | module | routes |
//! |--------|--------|
//! | [`agents`] | `POST /agents`, `infer`, `suspend`, `resume`, `DELETE /agents/{id}` |
//! | [`models`] | `unload`, `bless`, `unblock` |
//! | [`candidate`] | `POST`/`GET /models/{m}/swap-candidate` |
//!
//! What stays here is the spine every one of them goes through: [`dispatch`],
//! [`lock_pager`], [`bad_request`] and [`map_error`] -- so the error-code
//! table still has exactly one home, which is the property the paragraph
//! above is about.

mod agents;
mod candidate;
mod models;

use agents::{create_agent, delete_agent, infer, resume, suspend};
use candidate::{swap_candidate, swap_candidate_status};
use models::{bless, unblock, unload};

use std::sync::{Arc, Mutex, MutexGuard};

use serde_json::{json, Value};

use bloomery_substrate::Substrate;

use crate::memory::MemoryContext;
use crate::pager::{Pager, PagerError};
use crate::swap::SwapContext;

/// `dispatch`'s result: `None` for a body-less response (the `204`s),
/// `Some(value)` for a JSON one.
pub(crate) type ApiResult = (u16, Option<Value>);
/// Routes one HTTP request. `segments` is the `/`-split, non-empty path
/// (e.g. `/agents/a1/infer` -> `["agents", "a1", "infer"]`). Matching on
/// `&str` slices (rather than `String`) is what keeps this readable as a
/// later task adds more arms alongside these.
///
/// `pager` arrives as the `&Arc<Mutex<_>>` the server already holds (rather
/// than the bare `&Mutex<_>` every other handler still takes, by deref
/// coercion) for one reason: the swap-candidate route spawns a worker thread
/// that outlives the request and must own a handle on the pager — the same
/// reason `api_task::dispatch` takes it that way.
///
/// `swap` is `None` for a daemon served through [`crate::http::serve`] /
/// [`crate::http::serve_shared`], which wire no candidate context; the two
/// swap routes then say so by name rather than inventing a verdict.
///
/// `memory` is the daemon's memory organ (Task 8; memory-organ design §6),
/// read only by `status` below — `None` for a daemon served without one
/// (same set of entry points as `swap`'s `None` case above), in which case
/// `/status` simply carries no `memory` object rather than inventing one.
pub(crate) fn dispatch<S: Substrate + Send + 'static>(
    pager: &Arc<Mutex<Pager<S>>>,
    swap: Option<&Arc<SwapContext>>,
    memory: Option<&Arc<MemoryContext>>,
    method: &str,
    segments: &[String],
    body: &str,
) -> ApiResult {
    let parts: Vec<&str> = segments.iter().map(String::as_str).collect();
    match (method, parts.as_slice()) {
        ("POST", ["agents"]) => create_agent(pager, body),
        ("POST", ["agents", id, "infer"]) => infer(pager, id, body),
        ("POST", ["agents", id, "suspend"]) => suspend(pager, id),
        ("POST", ["agents", id, "resume"]) => resume(pager, id),
        ("DELETE", ["agents", id]) => delete_agent(pager, id),
        ("POST", ["models", name, "unload"]) => unload(pager, name),
        ("POST", ["models", name, "bless"]) => bless(pager, name),
        ("POST", ["models", name, "unblock"]) => unblock(pager, name),
        ("POST", ["models", name, "swap-candidate"]) => swap_candidate(pager, swap, name, body),
        ("GET", ["models", name, "swap-candidate"]) => swap_candidate_status(swap, name),
        ("GET", ["status"]) => status(pager, memory.map(Arc::as_ref)),
        _ => (404, Some(json!({"error": "not_found"}))),
    }
}

/// `pub(crate)` so `api_task.rs` (Task 5) reports a malformed request body
/// the same way this surface always has, rather than a second, drifting
/// spelling of `{"error":"bad_request", ...}`.
pub(crate) fn bad_request(e: serde_json::Error) -> ApiResult {
    bad_request_message(e.to_string())
}

/// The same shape for a request this layer refuses for a reason `serde` never
/// raised — today, a `gguf_path` naming bytes that cannot be read. One
/// spelling of `bad_request`, whoever noticed.
pub(super) fn bad_request_message(message: String) -> ApiResult {
    (
        400,
        Some(json!({"error": "bad_request", "message": message})),
    )
}

/// Locks `pager`, turning a poisoned mutex into a named 500 instead of a
/// panic cascade.
///
/// A poisoned pager mutex means some earlier request's worker thread
/// panicked while holding it — the pager's internal state (agent table,
/// image store, substrate handles) was left mid-mutation in a condition
/// nothing here can vouch for. `.into_inner()` is deliberately not used to
/// paper over that and keep serving: law 4 is "an infrastructure failure is
/// said, not guessed through", and a pager that might have half-applied a
/// mutation is not degraded, it's untrustworthy. So poison is sticky by
/// design — every request on every worker gets the same named 500 until the
/// daemon is restarted, rather than some requests succeeding against
/// state nobody can reason about.
///
/// `pub(crate)` so `api_v1.rs` (Task 15) shares this same sticky-poison
/// handling rather than re-implementing it against a second, drifting
/// spelling of the same failure.
pub(crate) fn lock_pager<S: Substrate>(
    pager: &Mutex<Pager<S>>,
) -> Result<MutexGuard<'_, Pager<S>>, ApiResult> {
    pager.lock().map_err(|_| {
        (
            500,
            Some(json!({
                "error": "internal",
                "detail": "pager state poisoned by a prior panic; restart the daemon",
            })),
        )
    })
}
/// `GET /status`. `memory` is `None` for a daemon served without a memory
/// organ (`crate::http::dispatch`'s callers that wire no context) — the
/// response then simply carries no `memory` key, rather than a `null` or an
/// invented all-zero object standing in for "there is no organ here".
///
/// When `memory` is `Some`, the `memory` object is spec §6's operator
/// surface: `enabled` (the config switch, independent of store state, so it
/// lives on `MemoryContext` rather than `MemoryCounts`), the four counts
/// (`None` — rendered as JSON `null` — when there is no store to count, the
/// `disabled_reason` case), and `disabled_reason` itself. Poison-recovered
/// (`unwrap_or_else(PoisonError::into_inner)`), same discipline as every
/// other store-mutex read in this organ: a poisoned memory-store mutex
/// means some earlier request's worker panicked mid-mutation of the store,
/// not the pager — unlike `lock_pager` above, that does not taint every
/// *other* pager operation, so `/status` still answers rather than joining
/// the pager's sticky-poison 500.
///
/// **The pager guard is dropped before the store is locked.** This is the
/// one systemic lock at a time discipline `task/registry.rs`'s organ
/// ordering also holds (the store lock is always fully released before
/// `pager.lock()`, and `organ_after_run` locks the store strictly after the
/// pager block has already closed) — never both systemic locks live on one
/// thread at once, so an AB-BA ordering between them can never arise.
fn status<S: Substrate>(pager: &Mutex<Pager<S>>, memory: Option<&MemoryContext>) -> ApiResult {
    let mut v = {
        let p = match lock_pager(pager) {
            Ok(p) => p,
            Err(poisoned) => return poisoned,
        };
        serde_json::to_value(p.status()).expect("StatusReport serializes")
    };
    if let Some(m) = memory {
        let counts = m.store.as_ref().map(|s| {
            s.lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .counts()
        });
        v["memory"] = serde_json::json!({
            "enabled": m.enabled,
            "episodes": counts.map(|c| c.episodes),
            "verified": counts.map(|c| c.verified),
            "contradicted": counts.map(|c| c.contradicted),
            "parse_errors": counts.map(|c| c.parse_errors),
            "disabled_reason": m.disabled_reason,
        });
    }
    (200, Some(v))
}

/// The Task 14 brief's error-code mapping table, the single place it lives:
///
/// | variant | status | body |
/// |---|---|---|
/// | `UnknownModel` / `UnknownAgent` | 404 | `{error, model\|agent}` |
/// | `Unprofiled` | 422 | `{error, model}` |
/// | `DriftBlocked` | 422 | `{error, model, reference}` |
/// | `Refused` | 409 | `{error, needed, free, reclaimable, max_placeable_tokens}` |
/// | `PromptTooLarge` | 413 | `{error, needed_tokens, window_tokens}` |
/// | `Budget` | 402 | `{error, remaining, requested}` |
/// | `Contract` | 502 | `{error, kind, detail}` |
/// | `Substrate` | 500 | `{error, message}` |
fn map_error(e: &PagerError) -> ApiResult {
    let (status, body) = match e {
        PagerError::UnknownModel(model) => (404, json!({"error": "unknown_model", "model": model})),
        PagerError::UnknownAgent(agent) => (404, json!({"error": "unknown_agent", "agent": agent})),
        PagerError::Unprofiled(model) => (422, json!({"error": "unprofiled", "model": model})),
        PagerError::DriftBlocked { model, reference } => (
            422,
            json!({"error": "drift_blocked", "model": model, "reference": reference}),
        ),
        PagerError::Refused {
            needed,
            free,
            reclaimable,
            max_placeable_tokens,
        } => (
            409,
            json!({
                "error": "refused",
                "needed": needed,
                "free": free,
                "reclaimable": reclaimable,
                // Always present, `null` when no window is advisable — an
                // absent key would be indistinguishable from a client that
                // forgot to read it. `null` says "we did not compute one",
                // which is a different fact from `0` ("nothing fits").
                "max_placeable_tokens": max_placeable_tokens,
            }),
        ),
        PagerError::PromptTooLarge {
            needed_tokens,
            window_tokens,
        } => (
            413,
            json!({
                "error": "prompt_too_large",
                "needed_tokens": needed_tokens,
                "window_tokens": window_tokens,
            }),
        ),
        PagerError::Budget {
            remaining,
            requested,
        } => (
            402,
            json!({
                "error": "budget_exhausted",
                "remaining": remaining,
                "requested": requested,
            }),
        ),
        PagerError::Contract(kind) => (
            502,
            json!({
                "error": "contract_violation",
                // `kind` is the pager's own spelling — currently always
                // "MissingStats", kept identical to the journal's
                // `ContractViolation`/`kind` field (pager.rs) rather than
                // reworded here, so the two are grep-able as the same fact.
                "kind": kind,
                "detail": "substrate reply omitted token stats",
            }),
        ),
        PagerError::Substrate(message) => {
            (500, json!({"error": "substrate_error", "message": message}))
        }
    };
    (status, Some(body))
}
