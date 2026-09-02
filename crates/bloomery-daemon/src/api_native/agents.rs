//! `The agent lifecycle routes: create, infer, suspend, resume, delete.`
//!
//! Split out of `api_native.rs` on 2026-09-01 (carried-debt slice D); the
//! route table that reaches these, and the `map_error` table they answer
//! through, are in this module's parent.

use std::sync::Mutex;

use serde_json::json;

use bloomery_substrate::Substrate;

use crate::pager::Pager;

use super::{bad_request, lock_pager, map_error, ApiResult};

#[derive(serde::Deserialize)]
struct CreateAgentReq {
    model: String,
    #[serde(default)]
    priority: Option<u8>,
    #[serde(default)]
    window_cap: Option<u32>,
    #[serde(default)]
    budget_tokens: Option<u64>,
}

#[derive(serde::Deserialize)]
struct InferReq {
    prompt: String,
    max_tokens: u32,
}

pub(super) fn create_agent<S: Substrate>(pager: &Mutex<Pager<S>>, body: &str) -> ApiResult {
    let req: CreateAgentReq = match serde_json::from_str(body) {
        Ok(r) => r,
        Err(e) => return bad_request(e),
    };
    let mut p = match lock_pager(pager) {
        Ok(p) => p,
        Err(poisoned) => return poisoned,
    };
    // A body that omits `priority` / `budget_tokens` lands on the *pager's*
    // defaults, which `main.rs` wires from `config.default_priority` /
    // `config.default_budget_tokens`. This layer deliberately owns no
    // constants of its own: it did in Task 14, and that is exactly what made
    // those config keys dead.
    let priority = req.priority.unwrap_or_else(|| p.default_priority());
    let budget_tokens = req
        .budget_tokens
        .unwrap_or_else(|| p.default_budget_tokens());
    match p.create_agent(&req.model, priority, req.window_cap, budget_tokens) {
        Ok(info) => (
            201,
            Some(serde_json::to_value(info).expect("AgentInfo serializes")),
        ),
        Err(e) => map_error(&e),
    }
}

pub(super) fn infer<S: Substrate>(pager: &Mutex<Pager<S>>, id: &str, body: &str) -> ApiResult {
    let req: InferReq = match serde_json::from_str(body) {
        Ok(r) => r,
        Err(e) => return bad_request(e),
    };
    let mut p = match lock_pager(pager) {
        Ok(p) => p,
        Err(poisoned) => return poisoned,
    };
    // Protocol §11: the native `/agents/{id}/infer` surface is POST-related
    // and untouched by envelope-v3 — always `stop: None`.
    match p.infer(id, &req.prompt, req.max_tokens, None) {
        Ok(reply) => (
            200,
            Some(json!({
                "text": reply.text,
                "prompt_tokens": reply.prompt_tokens,
                "completion_tokens": reply.completion_tokens,
                "duration_ms": reply.duration_ms,
            })),
        ),
        Err(e) => map_error(&e),
    }
}

pub(super) fn suspend<S: Substrate>(pager: &Mutex<Pager<S>>, id: &str) -> ApiResult {
    let mut p = match lock_pager(pager) {
        Ok(p) => p,
        Err(poisoned) => return poisoned,
    };
    match p.suspend(id) {
        Ok(()) => (204, None),
        Err(e) => map_error(&e),
    }
}

pub(super) fn resume<S: Substrate>(pager: &Mutex<Pager<S>>, id: &str) -> ApiResult {
    let mut p = match lock_pager(pager) {
        Ok(p) => p,
        Err(poisoned) => return poisoned,
    };
    match p.resume(id) {
        Ok(()) => (204, None),
        Err(e) => map_error(&e),
    }
}

/// The reason journaled for a removal this endpoint performed.
///
/// [`Pager::remove_agent`] writes its `reason` verbatim into the
/// `AgentRemoved` event, and three quite different things reach that same
/// event: a `/v1` ephemeral cleanup, an `unregister_model` cascade, and — now
/// — an operator asking directly. The string is the only thing that tells
/// them apart in the journal, so it names the surface rather than restating
/// what the event already says.
const OPERATOR_DELETE_REASON: &str = "operator requested removal via DELETE /agents/{id}";

/// `DELETE /agents/{id}` — remove an agent outright: destroy its resident
/// context, drop any parked KV image, forget the id.
///
/// | case | status |
/// |------|--------|
/// | removed (resident, suspended or fresh) | 204 |
/// | no such agent | 404 `unknown_agent` |
///
/// **This is not [`suspend`].** A suspended agent keeps its id, its table
/// entry and its image, and is meant to come back; a deleted one is gone. The
/// distinction had a cost before this route existed: the OpenAI-tools
/// adapter's live acceptance run (2026-08-31) leaked seven agents and cleared
/// them with `suspend`, which was the closest thing available and left every
/// entry standing.
///
/// **A second DELETE is a 404, not an idempotent 204.** Answering 204 for an
/// id that was never there would assert a removal that did not happen — a
/// success envelope over nothing, the same class this daemon's `/v1` shim
/// stopped doing on the day this route was added. Repeating the call still
/// leaves state identical, which is all DELETE's idempotence requires; it
/// just does not claim to have removed something twice.
///
/// **No caller-supplied reason.** [`crate::http`]'s request parser drops the
/// query string before dispatch ever sees it, so a `?reason=` would need a
/// parser change to reach here — deliberately out of scope for this route.
/// [`OPERATOR_DELETE_REASON`] is what the journal records.
///
/// Like every other route on this surface, this one takes the pager lock, so
/// it queues behind a running `infer` — an operator cannot delete their way
/// out of a wedged daemon. That blast radius is recorded in `CARRIED-DEBT.md`
/// and belongs to its own slice, not to this route.
pub(super) fn delete_agent<S: Substrate>(pager: &Mutex<Pager<S>>, id: &str) -> ApiResult {
    let mut p = match lock_pager(pager) {
        Ok(p) => p,
        Err(poisoned) => return poisoned,
    };
    match p.remove_agent(id, OPERATOR_DELETE_REASON) {
        Ok(()) => (204, None),
        Err(e) => map_error(&e),
    }
}
