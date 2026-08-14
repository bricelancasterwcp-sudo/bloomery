//! Route table for the native (non-OpenAI-compatible) HTTP surface: parse a
//! JSON body, call into `Pager<S>`, and map the result to `(status, body)`.
//!
//! [`map_error`] is the one place the Task 14 brief's error-code table
//! lives — every [`PagerError`] variant maps to exactly one status code and
//! JSON shape, so a refusal is always structured JSON with the arithmetic
//! that produced it, never a truncated success (law 2).

use std::sync::Mutex;

use serde_json::{json, Value};

use bloomery_substrate::Substrate;

use crate::pager::{Pager, PagerError};

/// Defaults for a `POST /agents` body that omits `priority` / `budget_tokens`.
///
/// These mirror `config.rs`'s `default_priority` / `default_budget_tokens`
/// (100, 200 000) but are not imported from there: `serve()` takes a bare
/// `Pager<S>`, not a `Config`, so the HTTP layer has no config to read a
/// caller's default from — only the request body's optional fields.
const DEFAULT_PRIORITY: u8 = 100;
const DEFAULT_BUDGET_TOKENS: u64 = 200_000;

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
        ("GET", ["status"]) => status(pager),
        _ => (404, Some(json!({"error": "not_found"}))),
    }
}

fn bad_request(e: serde_json::Error) -> ApiResult {
    (
        400,
        Some(json!({"error": "bad_request", "message": e.to_string()})),
    )
}

fn create_agent<S: Substrate>(pager: &Mutex<Pager<S>>, body: &str) -> ApiResult {
    let req: CreateAgentReq = match serde_json::from_str(body) {
        Ok(r) => r,
        Err(e) => return bad_request(e),
    };
    let mut p = pager.lock().expect("pager mutex poisoned");
    match p.create_agent(
        &req.model,
        req.priority.unwrap_or(DEFAULT_PRIORITY),
        req.window_cap,
        req.budget_tokens.unwrap_or(DEFAULT_BUDGET_TOKENS),
    ) {
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
    let mut p = pager.lock().expect("pager mutex poisoned");
    match p.infer(id, &req.prompt, req.max_tokens) {
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
    let mut p = pager.lock().expect("pager mutex poisoned");
    match p.suspend(id) {
        Ok(()) => (204, None),
        Err(e) => map_error(&e),
    }
}

fn resume<S: Substrate>(pager: &Mutex<Pager<S>>, id: &str) -> ApiResult {
    let mut p = pager.lock().expect("pager mutex poisoned");
    match p.resume(id) {
        Ok(()) => (204, None),
        Err(e) => map_error(&e),
    }
}

fn unload<S: Substrate>(pager: &Mutex<Pager<S>>, name: &str) -> ApiResult {
    let mut p = pager.lock().expect("pager mutex poisoned");
    match p.unload_model(name) {
        Ok(()) => (204, None),
        Err(e) => map_error(&e),
    }
}

fn status<S: Substrate>(pager: &Mutex<Pager<S>>) -> ApiResult {
    let p = pager.lock().expect("pager mutex poisoned");
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
/// | `Refused` | 409 | `{error, needed, free, reclaimable}` |
/// | `PromptTooLarge` | 413 | `{error, needed_tokens, window_tokens}` |
/// | `Budget` | 402 | `{error, remaining, requested}` |
/// | `Contract` | 502 | `{error, kind}` |
/// | `Substrate` | 500 | `{error, message}` |
fn map_error(e: &PagerError) -> ApiResult {
    let (status, body) = match e {
        PagerError::UnknownModel(model) => (404, json!({"error": "unknown_model", "model": model})),
        PagerError::UnknownAgent(agent) => (404, json!({"error": "unknown_agent", "agent": agent})),
        PagerError::Unprofiled(model) => (422, json!({"error": "unprofiled", "model": model})),
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
        PagerError::Contract(kind) => (502, json!({"error": "contract_violation", "kind": kind})),
        PagerError::Substrate(message) => {
            (500, json!({"error": "substrate_error", "message": message}))
        }
    };
    (status, Some(body))
}
