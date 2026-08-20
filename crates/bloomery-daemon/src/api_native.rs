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

use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

use serde_json::{json, Value};

use bloomery_substrate::Substrate;

use crate::drift::DriftError;
use crate::pager::{BlessError, Pager, PagerError};
use crate::post::with_pager;
use crate::swap::{
    run_candidate_probe, scratch_identity, SwapContext, SwapOutcomeReport, SwapState, NOTES, UNREAD,
};
use crate::task::registry::panic_payload_message;

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

/// The swap-candidate seam design's body, whole: `{"gguf_path":
/// "/abs/path/to/candidate.gguf"}`. No other field — the candidate's evidence
/// is probe-only by design (§6: an operator-supplied profile is refused for
/// this slice), so there is nothing else for a request to carry.
#[derive(serde::Deserialize)]
struct SwapCandidateReq {
    gguf_path: String,
}

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
pub(crate) fn dispatch<S: Substrate + Send + 'static>(
    pager: &Arc<Mutex<Pager<S>>>,
    swap: Option<&Arc<SwapContext>>,
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
        ("POST", ["models", name, "swap-candidate"]) => swap_candidate(pager, swap, name, body),
        ("GET", ["models", name, "swap-candidate"]) => swap_candidate_status(swap, name),
        ("GET", ["status"]) => status(pager),
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
fn bad_request_message(message: String) -> ApiResult {
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

/// `POST /models/{name}/swap-candidate` — the swap-candidate seam design's
/// question (§4): *does this candidate cover what `{name}`'s blessed baseline
/// says `{name}` was relied on for?*
///
/// Body: `{"gguf_path": "/abs/path/to/candidate.gguf"}`.
///
/// | outcome | status | body |
/// |---|---|---|
/// | started | 202 | `{model, candidate, state: "running"}` |
/// | no such model | 404 | the surface's one `unknown_model` shape |
/// | not JSON / no `gguf_path` / unreadable weights | 400 | the surface's one `bad_request` shape |
/// | no blessed baseline | 409 | `{error: "no_baseline", model, detail}` |
/// | a candidate job already running | 409 | `{error: "candidate_probe_in_progress", model, detail}` |
/// | this daemon wired no candidate context | 501 | `{error: "swap_candidate_unavailable", model, detail}` |
///
/// **202, not 200, and never the verdict itself.** The job registers the
/// candidate, probes it through this daemon's own `/v1`, covers the pair and
/// unloads it — ~10 minutes holding VRAM (design §4). A request handler that
/// waited for that would hold an HTTP worker (one of four) for the whole run,
/// and the boot watch's own rule — a probe never rides a request — applies
/// unchanged. So the handler claims the slot, spawns the worker, and answers
/// what it started; `GET` on the same path is where the answer appears.
///
/// **The order of the refusals is the order they are cheap in.** Everything
/// above the slot claim is a read: the model's existence, the body, the
/// candidate's bytes, the floor's existence. The claim comes last, so a
/// request that was going to be refused anyway never takes the slot from a
/// job that would have run — and it comes *before* the spawn, so two workers
/// can never both be registering the same scratch identity.
fn swap_candidate<S: Substrate + Send + 'static>(
    pager: &Arc<Mutex<Pager<S>>>,
    swap: Option<&Arc<SwapContext>>,
    name: &str,
    body: &str,
) -> ApiResult {
    let Some(swap) = swap else {
        return swap_unavailable(name);
    };
    {
        let p = match lock_pager(pager) {
            Ok(p) => p,
            Err(poisoned) => return poisoned,
        };
        // Asked through `/status`'s own list rather than a second spelling of
        // "what does this daemon serve" — a whole report built for one name is
        // nothing beside the ~10-minute job it gates, and the refusal goes
        // through the shared table, so this 404 has the same shape as every
        // other unknown-model refusal on this surface.
        if !p.status().models.iter().any(|m| m.name == name) {
            return map_error(&PagerError::UnknownModel(name.to_string()));
        }
    }
    let req: SwapCandidateReq = match serde_json::from_str(body) {
        Ok(r) => r,
        Err(e) => return bad_request(e),
    };
    let gguf = PathBuf::from(&req.gguf_path);
    // Refused here rather than left to the worker: weights nobody can read are
    // the operator's typo, answerable synchronously, and a 202 followed by an
    // `infra:` report ten seconds later would be a worse answer to it. The
    // worker still digests and parses the file itself — this is a precondition,
    // not a substitute for the job's own reads.
    if let Err(e) = std::fs::metadata(&gguf) {
        return bad_request_message(format!(
            "the candidate weights {} cannot be read: {e}",
            gguf.display()
        ));
    }
    let floor = swap.floor(name);
    if !floor.exists() {
        return (
            409,
            Some(json!({
                "error": "no_baseline",
                "model": name,
                "detail": format!(
                    "{name} has no blessed baseline at {} to cover against; \
                     POST /models/{name}/bless first — the floor is the operator-endorsed \
                     capability statement, never the merely-latest profile",
                    floor.display()
                ),
            })),
        );
    }
    // Design §4: "One candidate at a time … no queue." Claimed by this request
    // thread, so the refusal is synchronous and names the job that holds it.
    if let Err(busy) = swap.slot().try_start(name, &gguf) {
        return (
            409,
            Some(json!({
                "error": "candidate_probe_in_progress",
                "model": name,
                "detail": format!(
                    "a candidate probe for {} ({}) is already running; one at a time, no queue",
                    busy.model,
                    busy.gguf.display()
                ),
            })),
        );
    }
    spawn_candidate_probe(pager, swap, name.to_string(), gguf.clone());
    (
        202,
        Some(json!({
            "model": name,
            "candidate": gguf.display().to_string(),
            "state": "running",
        })),
    )
}

/// Puts one candidate job on its own thread and answers for the two things
/// [`run_candidate_probe`] documents as the **spawn site's** to own.
///
/// **The `Err` is never dropped.** It is the only report that the job's step-7
/// cleanup failed, and a failed unregister means the scratch identity —
/// possibly still holding weights — outlived the job, which is the one thing
/// design §4 says must not happen. The report says nothing about it (the
/// report carries the *verdict*, which is unaffected), so it is journaled
/// here, and said on stderr if the journal is what broke.
///
/// **A panic is caught here.** The job's cleanup is explicit code on the path
/// that returns, not a drop guard, so an unwind past the scratch registration
/// would leave the one slot `Running` for the life of the process — every
/// later candidate answered `candidate_probe_in_progress` for a job nobody can
/// see. `TaskRegistry::spawn_task` solves the identical problem the identical
/// way; that module's "Panic containment" section carries the full reasoning.
/// The probes are built *inside* the caught scope too, so a factory that
/// panics cannot wedge the slot either.
fn spawn_candidate_probe<S: Substrate + Send + 'static>(
    pager: &Arc<Mutex<Pager<S>>>,
    swap: &Arc<SwapContext>,
    model: String,
    gguf: PathBuf,
) {
    let pager = Arc::clone(pager);
    let swap = Arc::clone(swap);
    std::thread::spawn(move || {
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            // Built here, on the worker's own thread: a `PostRunner`'s command
            // seam is deliberately not `Send` (see `swap::context`).
            let probes = swap.probes();
            run_candidate_probe(
                &pager,
                &probes.runner,
                &probes.gate,
                swap.store(),
                swap.port(),
                swap.tier(),
                &model,
                &gguf,
                swap.slot(),
            )
        }));
        match outcome {
            Ok(Ok(())) => {}
            Ok(Err(e)) => say_degraded(
                &pager,
                format!(
                    "swap: {model}'s candidate job could not finish cleanly: {e}; the scratch \
                     identity {} may still be registered — check /status and unload it",
                    scratch_identity(&model)
                ),
            ),
            Err(payload) => {
                // The job closes the candidate's admission window between its
                // probe and the branch on that probe's result, and closes it
                // structurally by unregistering the scratch identity at step
                // 7. An unwind runs neither: the registration survives (the
                // detail below tells the operator so) and its window would
                // survive with it, admitting that identity — unprofiled —
                // through `/v1` for the life of the process. Best-effort
                // because the one way this can fail is a poisoned pager, and
                // that daemon already answers every request with a named 500,
                // so there is no admission left to gate (`pager::probing`,
                // and `post::post_with_gate` for the same case at boot).
                let _ = with_pager(&pager, |p| {
                    p.close_probe_window(&scratch_identity(&model));
                    Ok(())
                });
                let said = panic_payload_message(payload.as_ref())
                    .unwrap_or_else(|| "no string message on the payload".to_string());
                let detail = format!(
                    "the swap-candidate worker for {model} panicked: {said}; no verdict was \
                     reached, and the scratch identity {} may still be registered — check \
                     /status and unload it",
                    scratch_identity(&model)
                );
                // The slot is released by the worker on every path it can
                // *return* through; an unwind is not one of them, so this is
                // the release. Digests are `UNREAD` because a panic reached no
                // reading, and the path is where the document would have been
                // written — the same answer the job's own pre-retention
                // failures give.
                swap.slot().finish(
                    &model,
                    SwapOutcomeReport {
                        outcome: format!("infra: {detail}"),
                        exit_code: None,
                        candidate_gguf_sha: UNREAD.to_string(),
                        floor_sha: UNREAD.to_string(),
                        candidate_profile_path: swap.staging(&model).display().to_string(),
                        notes: NOTES,
                    },
                );
                say_degraded(&pager, format!("swap: {detail}"));
            }
        }
    });
}

/// Journals one `Degraded` row for something the swap worker could not report
/// for itself, and says it on stderr if the journal is what would not take it
/// — the same last resort `main.rs` uses when POST cannot record its result.
fn say_degraded<S: Substrate>(pager: &Mutex<Pager<S>>, detail: String) {
    if let Err(e) = with_pager(pager, |p| p.journal_degraded(detail.clone())) {
        eprintln!("bloomery-daemon: {detail} (the journal refused this row: {e})");
    }
}

/// `GET /models/{name}/swap-candidate` — the answer to the POST above.
///
/// | outcome | status | body |
/// |---|---|---|
/// | running | 200 | `{model, state: "running"}` |
/// | done | 200 | `{model, state: "done", report: {…}}` |
/// | never started | 404 | `{error: "no_swap_candidate", model}` |
/// | this daemon wired no candidate context | 501 | `{error: "swap_candidate_unavailable", model, detail}` |
///
/// The `report` is [`SwapOutcomeReport`] verbatim: `outcome`, `exit_code`,
/// `candidate_gguf_sha`, `floor_sha`, `candidate_profile_path`, and the two
/// fixed `notes` design §4 requires every answer to carry.
///
/// **`candidate_gguf_sha` and `floor_sha` can be the literal word `"unread"`**
/// ([`UNREAD`]) — what a digest field carries when the job never got a digest
/// to put there. It reads like a digest until you know better, and it only
/// ever appears beside an `"infra: …"` outcome whose sentence names what
/// failed; a verdict never carries one.
///
/// **There is one slot, and it holds one model's job.** A `GET` for a model
/// while some *other* model's job runs (or while its answer stands) is a 404:
/// that job says nothing about this name, and rendering it under this name
/// would be an answer about the wrong model.
fn swap_candidate_status(swap: Option<&Arc<SwapContext>>, name: &str) -> ApiResult {
    let Some(swap) = swap else {
        return swap_unavailable(name);
    };
    match swap.slot().snapshot() {
        SwapState::Running { model, .. } if model == name => {
            (200, Some(json!({"model": name, "state": "running"})))
        }
        SwapState::Done { model, report } if model == name => (
            200,
            Some(json!({
                "model": name,
                "state": "done",
                "report": serde_json::to_value(report).expect("SwapOutcomeReport serializes"),
            })),
        ),
        _ => (
            404,
            Some(json!({"error": "no_swap_candidate", "model": name})),
        ),
    }
}

/// A daemon served through [`crate::http::serve`] / [`crate::http::serve_shared`]
/// has no candidate context: no interpreter to probe with, no profile store to
/// read a floor from, no port to reach itself on. Named, in the shape
/// `api_task`'s `tasks_disabled` uses for its own dark surface — anything else
/// would be a refusal about *this candidate* for a daemon that never had the
/// machinery to look at one.
fn swap_unavailable(name: &str) -> ApiResult {
    (
        501,
        Some(json!({
            "error": "swap_candidate_unavailable",
            "model": name,
            "detail": "this daemon was served without a swap-candidate context, so it can \
                       neither probe a candidate nor read a blessed floor",
        })),
    )
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
