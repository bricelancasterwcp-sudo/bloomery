//! `The model routes: unload, and the two operator routes that touch admission.`
//!
//! Split out of `api_native.rs` on 2026-09-01 (carried-debt slice D); the
//! route table that reaches these, and the `map_error` table they answer
//! through, are in this module's parent.

use std::sync::Mutex;

use serde_json::json;

use bloomery_substrate::Substrate;

use crate::drift::DriftError;
use crate::pager::{BlessError, Pager, PagerError};

use super::{lock_pager, map_error, ApiResult};

pub(super) fn unload<S: Substrate>(pager: &Mutex<Pager<S>>, name: &str) -> ApiResult {
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
pub(super) fn bless<S: Substrate>(pager: &Mutex<Pager<S>>, name: &str) -> ApiResult {
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
pub(super) fn unblock<S: Substrate>(pager: &Mutex<Pager<S>>, name: &str) -> ApiResult {
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
