//! `The swap-candidate routes (swap-candidate seam design §4).`
//!
//! Split out of `api_native.rs` on 2026-09-01 (carried-debt slice D); the
//! route table that reaches these, and the `map_error` table they answer
//! through, are in this module's parent.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde_json::json;

use bloomery_substrate::Substrate;

use crate::pager::{Pager, PagerError};
use crate::post::with_pager;
use crate::swap::{
    run_candidate_probe, scratch_identity, SwapContext, SwapOutcomeReport, SwapState, NOTES, UNREAD,
};
use crate::task::registry::panic_payload_message;

use super::{bad_request, bad_request_message, lock_pager, map_error, ApiResult};

/// The swap-candidate seam design's body, whole: `{"gguf_path":
/// "/abs/path/to/candidate.gguf"}`. No other field — the candidate's evidence
/// is probe-only by design (§6: an operator-supplied profile is refused for
/// this slice), so there is nothing else for a request to carry.
#[derive(serde::Deserialize)]
struct SwapCandidateReq {
    gguf_path: String,
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
pub(super) fn swap_candidate<S: Substrate + Send + 'static>(
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
pub(super) fn swap_candidate_status(swap: Option<&Arc<SwapContext>>, name: &str) -> ApiResult {
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
