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

use std::sync::{Mutex, MutexGuard};

use serde_json::{json, Value};

use bloomery_substrate::Substrate;

use crate::drift::DriftError;
use crate::pager::{BlessError, Pager, PagerError};

/// `dispatch`'s result: `None` for a body-less response (the `204`s),
/// `Some(value)` for a JSON one.
pub(crate) type ApiResult = (u16, Option<Value>);

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

/// Routes one HTTP request. `segments` is the `/`-split, non-empty path
/// (e.g. `/agents/a1/infer` -> `["agents", "a1", "infer"]`). Matching on
/// `&str` slices (rather than `String`) is what keeps this readable as a
/// later task adds more arms alongside these.
pub(crate) fn dispatch<S: Substrate>(
    pager: &Mutex<Pager<S>>,
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
        ("POST", ["models", name, "unload"]) => unload(pager, name),
        ("POST", ["models", name, "bless"]) => bless(pager, name),
        ("POST", ["models", name, "unblock"]) => unblock(pager, name),
        ("GET", ["status"]) => status(pager),
        _ => (404, Some(json!({"error": "not_found"}))),
    }
}

/// `pub(crate)` so `api_task.rs` (Task 5) reports a malformed request body
/// the same way this surface always has, rather than a second, drifting
/// spelling of `{"error":"bad_request", ...}`.
pub(crate) fn bad_request(e: serde_json::Error) -> ApiResult {
    (
        400,
        Some(json!({"error": "bad_request", "message": e.to_string()})),
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

fn create_agent<S: Substrate>(pager: &Mutex<Pager<S>>, body: &str) -> ApiResult {
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

fn infer<S: Substrate>(pager: &Mutex<Pager<S>>, id: &str, body: &str) -> ApiResult {
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

fn suspend<S: Substrate>(pager: &Mutex<Pager<S>>, id: &str) -> ApiResult {
    let mut p = match lock_pager(pager) {
        Ok(p) => p,
        Err(poisoned) => return poisoned,
    };
    match p.suspend(id) {
        Ok(()) => (204, None),
        Err(e) => map_error(&e),
    }
}

fn resume<S: Substrate>(pager: &Mutex<Pager<S>>, id: &str) -> ApiResult {
    let mut p = match lock_pager(pager) {
        Ok(p) => p,
        Err(poisoned) => return poisoned,
    };
    match p.resume(id) {
        Ok(()) => (204, None),
        Err(e) => map_error(&e),
    }
}

fn unload<S: Substrate>(pager: &Mutex<Pager<S>>, name: &str) -> ApiResult {
    let mut p = match lock_pager(pager) {
        Ok(p) => p,
        Err(poisoned) => return poisoned,
    };
    match p.unload_model(name) {
        Ok(()) => (204, None),
        Err(e) => map_error(&e),
    }
}

/// `POST /models/{name}/bless` — the drift-watch design's explicit operator
/// action (§2): make this model's current profile its drift-cumulative
/// baseline, journaled with the profile's identity.
///
/// | outcome | status | body |
/// |---|---|---|
/// | blessed | 200 | `{model, sha, path}` |
/// | no such model | 404 | the surface's one `unknown_model` shape |
/// | no current profile to bless | 409 | `{error: "no_current_profile", model, detail}` |
/// | nowhere to file it / I-O / journal | 500 | `{error: "internal", detail}` |
///
/// **The 409 is the load-bearing one.** A daemon whose POST never ran (or ran
/// and failed for this model) has no current profile, and answering `204`/`200`
/// there would tell an operator a baseline now exists when nothing was written
/// — precisely the silent no-op design §2 forbids ("never a silent skip, never
/// a pass"). It is a 409 rather than a 404 because the *model* is known and the
/// request is well-formed; the daemon's state is what conflicts with it.
///
/// **This takes effect at the NEXT boot's comparison.** Blessing replaces the
/// reference the cumulative gate will read; the reading already in
/// `ModelStatus.drift` is this boot's, measured against the baseline that stood
/// when POST ran, and it stands unchanged. Nothing here recomputes a status —
/// a comparison nobody re-ran must never acquire a new verdict.
fn bless<S: Substrate>(pager: &Mutex<Pager<S>>, name: &str) -> ApiResult {
    let mut p = match lock_pager(pager) {
        Ok(p) => p,
        Err(poisoned) => return poisoned,
    };
    // Holding the pager lock across this is deliberate and cheap: the work is
    // a copy of one small JSON document plus a journal append, the same order
    // of magnitude as any other route's, and nothing like the probes the boot
    // watch takes care never to hold the lock across.
    let e = match p.bless_baseline(name) {
        Ok(blessing) => {
            return (
                200,
                Some(json!({
                    "model": name,
                    "sha": blessing.sha,
                    "path": blessing.path.display().to_string(),
                })),
            )
        }
        Err(e) => e,
    };
    // Every `detail` is the failing layer's own sentence (`BlessError`'s
    // `Display` delegates to the store's), so the path a refusal names is the
    // path the store actually looked at — never a paraphrase built here that
    // could come to describe a different file.
    match &e {
        // Answered through the shared table rather than a second spelling of
        // `{"error":"unknown_model"}` — one shape for one fact.
        BlessError::UnknownModel(model) => map_error(&PagerError::UnknownModel(model.clone())),
        BlessError::Store(DriftError::NoCurrentProfile { model, .. }) => (
            409,
            Some(json!({
                "error": "no_current_profile",
                "model": model,
                "detail": e.to_string(),
            })),
        ),
        BlessError::NoProfilesDir
        | BlessError::Store(DriftError::Io { .. })
        | BlessError::Journal(_) => (
            500,
            Some(json!({"error": "internal", "detail": e.to_string()})),
        ),
    }
}

/// `POST /models/{name}/unblock` — clear this boot's admission block
/// (verdict-gated-admission design §4).
///
/// | outcome | status | body |
/// |---|---|---|
/// | cleared | 200 | `{model, cleared: {reference}}` |
/// | no such model | 404 | the surface's one `unknown_model` shape |
/// | no block to clear | 409 | `{error: "no_admission_block", model, detail}` |
///
/// **The 409 is the load-bearing one**, for the same reason bless's is:
/// answering 200 where nothing was blocking would tell an operator they
/// had cleared something when nothing was written.
///
/// This does NOT re-baseline. `bless` accepts a new normal for the next
/// boot; this admits the model now, with the reading left exactly as
/// measured. Neither implies the other.
fn unblock<S: Substrate>(pager: &Mutex<Pager<S>>, name: &str) -> ApiResult {
    let mut p = match lock_pager(pager) {
        Ok(p) => p,
        Err(poisoned) => return poisoned,
    };
    match p.clear_admission_block(name) {
        Ok(Some(block)) => (
            200,
            Some(json!({
                "model": name,
                "cleared": {"reference": block.reference},
            })),
        ),
        Ok(None) => (
            409,
            Some(json!({
                "error": "no_admission_block",
                "model": name,
                "detail": format!(
                    "model {name} has no standing admission block to clear"
                ),
            })),
        ),
        // Answered through the shared table rather than a second spelling of
        // `{"error":"unknown_model"}` — one shape for one fact.
        Err(e) => map_error(&e),
    }
}

fn status<S: Substrate>(pager: &Mutex<Pager<S>>) -> ApiResult {
    let p = match lock_pager(pager) {
        Ok(p) => p,
        Err(poisoned) => return poisoned,
    };
    (
        200,
        Some(serde_json::to_value(p.status()).expect("StatusReport serializes")),
    )
}

/// The Task 14 brief's error-code mapping table, the single place it lives:
///
/// | variant | status | body |
/// |---|---|---|
/// | `UnknownModel` / `UnknownAgent` | 404 | `{error, model\|agent}` |
/// | `Unprofiled` | 422 | `{error, model}` |
/// | `DriftBlocked` | 422 | `{error, model, reference}` |
/// | `Refused` | 409 | `{error, needed, free, reclaimable}` |
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
        } => (
            409,
            json!({
                "error": "refused",
                "needed": needed,
                "free": free,
                "reclaimable": reclaimable,
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
