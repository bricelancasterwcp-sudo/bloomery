//! `/v1`: an OpenAI-compatible, honest surface over the same
//! `Arc<Mutex<Pager<S>>>` `api_native` (Task 14) uses.
//!
//! "Honest" is the load-bearing word (Task 15 brief): `usage` is always the
//! real `VerifiedReply` counts, never invented; an oversized prompt gets a
//! structured `400` in the OpenAI error envelope — never silent truncation
//! (project law 2) — and every `PagerError` maps to exactly one status and
//! envelope here, same discipline as [`map_error`] in `api_native.rs`, just
//! against OpenAI's shape instead of the native one.
//!
//! **Chat templating (D4):** the `Substrate` trait exposes no model-native
//! chat template today — `FakeSubstrate` has none, and neither does the
//! llama.cpp backend's current surface — so there is nothing to select
//! between yet. Every request uses D4's documented fallback concatenation,
//! `"{role}: {content}\n"` per message plus a trailing `"assistant: "`, and
//! every response says so via `X-Bloomery-Template: fallback`. Model-native
//! templates are a substrate capability for a later phase; when one lands,
//! this is the one place that picks between `model` and `fallback`. Task
//! 1's obligation to catch a template *failure* at apply time (not fetch
//! time) is moot while the fallback concatenation cannot fail — string
//! formatting has no error path — but it stays relevant the day a real,
//! fallible template arrives here.
//!
//! **Streaming (D3):** `stream:true` gets real SSE wire format
//! (`Content-Type: text/event-stream`, `data: ...` lines, terminal
//! `data: [DONE]`), but Phase 1 buffers: the whole reply is generated
//! first, then emitted as one delta chunk followed by a final chunk
//! carrying `usage`. Token-incremental streaming needs a streaming
//! `Substrate::infer`, which does not exist yet — a documented Phase 1
//! limit, not a hidden one.
//!
//! **Session binding:** `X-Bloomery-Agent: <id>` must name an agent that
//! already exists (created via the native `POST /agents`, Task 14) — this
//! shim never invents its own session-id namespace on top of the pager's
//! own agent ids, so there is exactly one id space to reason about. A
//! request without the header gets an ephemeral agent at the pager's
//! configured defaults ([`Pager::default_priority`] /
//! [`Pager::default_budget_tokens`], wired from config by `main.rs`),
//! created, used once, and
//! removed via [`Pager::remove_agent`] before the response is returned —
//! Phase 1 has no session GC, so an anonymous call must never leave an
//! agent behind. If that removal itself fails (a real double-fault: the
//! error that triggered cleanup, *plus* `destroy_context` failing), the
//! response still can't carry two errors — the leak is named on stderr
//! instead of swallowed (law 4's minimum honesty).
//!
//! A header-bound agent belongs to whatever model it was created with;
//! a request naming a *different* `model` would otherwise be silently run
//! against the bound agent's real model while the response echoed back
//! the caller's string. That is exactly the kind of quiet dishonesty this
//! module exists to refuse — see `model_mismatch` in [`map_error`]'s
//! neighborhood (`chat_completions`'s own check, not a `PagerError`
//! variant, since the pager has no opinion on this — it's a shim-level
//! promise about `model` matching the id the caller gave it).

use std::sync::{Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

use bloomery_substrate::Substrate;

use crate::api_native::lock_pager;
use crate::pager::{Pager, PagerError};

/// `max_tokens` a request omits entirely. Chosen as a small, safe default
/// rather than "as many as the window allows" — an unbounded default would
/// make a client's first request to a small-context model a surprise
/// `prompt_too_large` refusal driven entirely by a field they never set.
const DEFAULT_MAX_TOKENS: u32 = 256;

/// The wire's terminal SSE event, verbatim (D3): a bare `data: [DONE]`
/// line, blank-line terminated like every other SSE event.
const SSE_DONE: &str = "data: [DONE]\n\n";

/// One `/v1` response body: either a JSON document or a pre-rendered SSE
/// event stream. `http.rs` picks the `Content-Type` from which variant this
/// is — the one place that decision is made, so a route can't accidentally
/// send SSE framing with a JSON content type or vice versa.
pub(crate) enum V1Body {
    Json(Value),
    Sse(String),
}

/// `dispatch`'s result: a status, a body, and any headers this route wants
/// set (`X-Bloomery-Template`, currently the only one).
pub(crate) struct V1Result {
    pub status: u16,
    pub body: V1Body,
    pub headers: Vec<(&'static str, String)>,
}

impl V1Result {
    fn json(status: u16, value: Value) -> Self {
        Self {
            status,
            body: V1Body::Json(value),
            headers: Vec::new(),
        }
    }
}

#[derive(serde::Deserialize)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(serde::Deserialize)]
struct ChatCompletionReq {
    model: String,
    messages: Vec<ChatMessage>,
    #[serde(default)]
    max_tokens: Option<u32>,
    #[serde(default)]
    stream: Option<bool>,
}

/// Routes one `/v1` request. `segments` is the same `/`-split path
/// `http.rs` already computed for `api_native::dispatch` (e.g.
/// `/v1/chat/completions` -> `["v1", "chat", "completions"]`);
/// `agent_header` is the `X-Bloomery-Agent` header's value, if the request
/// carried one.
pub(crate) fn dispatch<S: Substrate>(
    pager: &Mutex<Pager<S>>,
    method: &str,
    segments: &[String],
    body: &str,
    agent_header: Option<&str>,
) -> V1Result {
    let parts: Vec<&str> = segments.iter().map(String::as_str).collect();
    match (method, parts.as_slice()) {
        ("GET", ["v1", "models"]) => models(pager),
        ("POST", ["v1", "chat", "completions"]) => chat_completions(pager, body, agent_header),
        _ => V1Result::json(
            404,
            error_envelope(
                "invalid_request_error",
                "not_found",
                "no such /v1 route".to_string(),
                None,
            ),
        ),
    }
}

fn lock_pager_v1<S: Substrate>(
    pager: &Mutex<Pager<S>>,
) -> Result<MutexGuard<'_, Pager<S>>, V1Result> {
    lock_pager(pager).map_err(|_| {
        // `api_native::lock_pager`'s own body is shaped for the native API;
        // only its sticky-poison *decision* (not its JSON spelling) is
        // reused here — `/v1` still owes callers the OpenAI envelope even
        // on a poisoned pager.
        V1Result::json(
            500,
            error_envelope(
                "server_error",
                "internal",
                "pager state poisoned by a prior panic; restart the daemon".to_string(),
                None,
            ),
        )
    })
}

fn models<S: Substrate>(pager: &Mutex<Pager<S>>) -> V1Result {
    let p = match lock_pager_v1(pager) {
        Ok(p) => p,
        Err(resp) => return resp,
    };
    let data: Vec<Value> = p
        .status()
        .models
        .into_iter()
        .map(|m| {
            json!({
                "id": m.name,
                "object": "model",
                "owned_by": "bloomery",
            })
        })
        .collect();
    V1Result::json(200, json!({"object": "list", "data": data}))
}

fn chat_completions<S: Substrate>(
    pager: &Mutex<Pager<S>>,
    body: &str,
    agent_header: Option<&str>,
) -> V1Result {
    let req: ChatCompletionReq = match serde_json::from_str(body) {
        Ok(r) => r,
        Err(e) => {
            return V1Result::json(
                400,
                error_envelope("invalid_request_error", "invalid_json", e.to_string(), None),
            )
        }
    };
    if req.messages.is_empty() {
        return V1Result::json(
            400,
            error_envelope(
                "invalid_request_error",
                "empty_messages",
                "messages must not be empty".to_string(),
                Some("messages"),
            ),
        );
    }

    let prompt = fallback_prompt(&req.messages);
    let max_tokens = req.max_tokens.unwrap_or(DEFAULT_MAX_TOKENS);

    let mut p = match lock_pager_v1(pager) {
        Ok(p) => p,
        Err(resp) => return resp,
    };

    // Ephemeral agents only: a header-named agent must already exist —
    // this shim never mints its own id namespace on top of the pager's
    // (see the module doc's "Session binding" note).
    let (agent_id, ephemeral) = match agent_header {
        Some(id) => (id.to_string(), false),
        None => {
            // Read the defaults out before the `&mut` borrow `create_agent`
            // needs; they are the pager's (config-wired), never this
            // layer's own constants.
            let (priority, budget) = (p.default_priority(), p.default_budget_tokens());
            match p.create_agent(&req.model, priority, None, budget) {
                Ok(info) => (info.id, true),
                Err(e) => {
                    let (status, value) = map_error(&e, None);
                    return V1Result::json(status, value);
                }
            }
        }
    };

    // Honest refusal, not a silent-echo: a header-bound agent's model is
    // whatever it was created with, not whatever `model` the caller typed
    // this time. An ephemeral agent was just minted *from* `req.model`, so
    // this can never fire for it — only the header path can disagree.
    // `UnknownAgent` (header names an id that doesn't exist) is left to
    // `infer` below, which already reports it as `agent_not_found`.
    if !ephemeral {
        let bound_model = p
            .status()
            .agents
            .into_iter()
            .find(|a| a.id == agent_id)
            .map(|a| a.model);
        if let Some(bound_model) = bound_model {
            if bound_model != req.model {
                return V1Result::json(
                    400,
                    error_envelope(
                        "invalid_request_error",
                        "model_mismatch",
                        format!(
                            "agent {agent_id} is bound to model {bound_model}; \
                             request names {}",
                            req.model
                        ),
                        Some("model"),
                    ),
                );
            }
        }
    }

    // Protocol §11: the `/v1` chat surface is untouched by envelope-v3 — it
    // always passes `stop: None`, never a task-loop stop sequence.
    let infer_result = p.infer(&agent_id, &prompt, max_tokens, None);

    // Needed only for an honest `PromptTooLarge` message's "(bound by
    // <term>)" parenthetical — looked up from the still-live agent, since
    // `PagerError::PromptTooLarge` itself doesn't carry the binding term.
    let bound_by: Option<String> = if infer_result.is_err() {
        p.status()
            .agents
            .into_iter()
            .find(|a| a.id == agent_id)
            .map(|a| a.bound_by.to_string())
    } else {
        None
    };

    if ephemeral {
        // Best-effort cleanup regardless of outcome: an anonymous call must
        // never leave an agent behind, whether it succeeded or was refused.
        // A failure here is a real double-fault (whatever error `infer`
        // already returned, *plus* `remove_agent`'s own `destroy_context`
        // failing) — the response can't carry two errors, so this is named
        // on stderr rather than swallowed: law 4's minimum is saying an
        // infrastructure failure, not guessing through it silently.
        if let Err(cleanup_err) = p.remove_agent(&agent_id, "ephemeral cleanup") {
            eprintln!(
                "bloomery-daemon: ephemeral agent {agent_id} cleanup failed: \
                 {cleanup_err:?} — agent leaked until restart"
            );
        }
    }
    drop(p);

    match infer_result {
        Ok(reply) => {
            let created = unix_now();
            let completion_id = format!("chatcmpl-{agent_id}-{created}");
            let total_tokens = u64::from(reply.prompt_tokens) + u64::from(reply.completion_tokens);
            let headers = vec![("X-Bloomery-Template", "fallback".to_string())];
            if req.stream.unwrap_or(false) {
                let sse = render_sse(
                    &completion_id,
                    created,
                    &req.model,
                    &reply.text,
                    reply.prompt_tokens,
                    reply.completion_tokens,
                    total_tokens,
                );
                V1Result {
                    status: 200,
                    body: V1Body::Sse(sse),
                    headers,
                }
            } else {
                let value = json!({
                    "id": completion_id,
                    "object": "chat.completion",
                    "created": created,
                    "model": req.model,
                    "choices": [{
                        "index": 0,
                        "message": {"role": "assistant", "content": reply.text},
                        "finish_reason": "stop",
                    }],
                    "usage": {
                        "prompt_tokens": reply.prompt_tokens,
                        "completion_tokens": reply.completion_tokens,
                        "total_tokens": total_tokens,
                    },
                });
                V1Result {
                    status: 200,
                    body: V1Body::Json(value),
                    headers,
                }
            }
        }
        Err(e) => {
            let (status, value) = map_error(&e, bound_by.as_deref());
            V1Result::json(status, value)
        }
    }
}

/// D4's documented fallback template: `"{role}: {content}\n"` per message,
/// plus a trailing `"assistant: "` to prompt the completion. See the module
/// doc's "Chat templating" note for why every model uses this in Phase 1.
fn fallback_prompt(messages: &[ChatMessage]) -> String {
    let mut out = String::new();
    for m in messages {
        out.push_str(&m.role);
        out.push_str(": ");
        out.push_str(&m.content);
        out.push('\n');
    }
    out.push_str("assistant: ");
    out
}

/// Renders the buffered-SSE wire format D3 calls for: one delta chunk
/// carrying the whole reply, one final chunk carrying real `usage`, then
/// the terminal `data: [DONE]`. See the module doc's "Streaming" note for
/// why this is buffered rather than token-incremental in Phase 1.
fn render_sse(
    id: &str,
    created: u64,
    model: &str,
    text: &str,
    prompt_tokens: u32,
    completion_tokens: u32,
    total_tokens: u64,
) -> String {
    let delta_chunk = json!({
        "id": id,
        "object": "chat.completion.chunk",
        "created": created,
        "model": model,
        "choices": [{
            "index": 0,
            "delta": {"role": "assistant", "content": text},
            "finish_reason": Value::Null,
        }],
    });
    let final_chunk = json!({
        "id": id,
        "object": "chat.completion.chunk",
        "created": created,
        "model": model,
        "choices": [{
            "index": 0,
            "delta": {},
            "finish_reason": "stop",
        }],
        "usage": {
            "prompt_tokens": prompt_tokens,
            "completion_tokens": completion_tokens,
            "total_tokens": total_tokens,
        },
    });
    format!("data: {delta_chunk}\n\ndata: {final_chunk}\n\n{SSE_DONE}")
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// The OpenAI-shaped error envelope every `/v1` error response uses:
/// `{"error": {"type", "code", "message", "param"?}}`.
fn error_envelope(kind: &str, code: &str, message: String, param: Option<&str>) -> Value {
    let mut error = json!({
        "type": kind,
        "code": code,
        "message": message,
    });
    if let Some(param) = param {
        error["param"] = json!(param);
    }
    json!({ "error": error })
}

/// The Task 15 brief's `/v1` error-code mapping: every [`PagerError`]
/// variant to exactly one status and OpenAI-envelope body. `bound_by`
/// (only meaningful for `PromptTooLarge`) is the agent's window-binding
/// term when the caller could still resolve it — omitted, not guessed,
/// otherwise (see the honest-refusal contract, law 2).
fn map_error(e: &PagerError, bound_by: Option<&str>) -> (u16, Value) {
    match e {
        PagerError::PromptTooLarge {
            needed_tokens,
            window_tokens,
        } => {
            let parenthetical = bound_by
                .map(|b| format!(" (bound by {b})"))
                .unwrap_or_default();
            (
                400,
                error_envelope(
                    "invalid_request_error",
                    "prompt_too_large",
                    format!(
                        "prompt needs {needed_tokens} tokens; window is {window_tokens}\
                         {parenthetical}; refusing rather than truncating"
                    ),
                    Some("messages"),
                ),
            )
        }
        PagerError::Budget {
            remaining,
            requested,
        } => (
            429,
            error_envelope(
                "insufficient_quota",
                "budget_exhausted",
                format!(
                    "agent's token budget is exhausted: {remaining} remaining, \
                     {requested} requested"
                ),
                None,
            ),
        ),
        PagerError::Contract(kind) => (
            500,
            error_envelope(
                "server_error",
                "contract_violation",
                format!("substrate reply omitted token stats ({kind})"),
                None,
            ),
        ),
        PagerError::Substrate(message) => (
            500,
            error_envelope("server_error", "substrate_error", message.clone(), None),
        ),
        PagerError::UnknownModel(model) => (
            404,
            error_envelope(
                "invalid_request_error",
                "model_not_found",
                format!("model '{model}' is not registered"),
                Some("model"),
            ),
        ),
        PagerError::UnknownAgent(agent) => (
            404,
            error_envelope(
                "invalid_request_error",
                "agent_not_found",
                format!("no agent with id '{agent}' (X-Bloomery-Agent)"),
                None,
            ),
        ),
        PagerError::Unprofiled(model) => (
            422,
            error_envelope(
                "invalid_request_error",
                "model_unprofiled",
                format!("model '{model}' has no capability profile"),
                Some("model"),
            ),
        ),
        PagerError::DriftBlocked { model, reference } => (
            422,
            error_envelope(
                "invalid_request_error",
                "model_drift_blocked",
                format!(
                    "model '{model}' is held out: its capability profile drifted \
                     from blessed baseline '{reference}' and the change reproduced"
                ),
                Some("model"),
            ),
        ),
        PagerError::Refused {
            needed,
            free,
            reclaimable,
        } => (
            503,
            error_envelope(
                "server_error",
                "residency_refused",
                format!(
                    "residency refused: needed {needed} B, free {free} B, \
                     reclaimable {reclaimable} B"
                ),
                None,
            ),
        ),
    }
}
