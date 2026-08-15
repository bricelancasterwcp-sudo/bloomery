//! Route table for the task HTTP surface (Phase 2b/2c P3 Task 5):
//! `POST /agents/{id}/task` and `GET /agents/{id}/task/{task_id}`.
//!
//! Both routes are dark by default: [`create_task`] checks
//! `Pager::tasks_enabled` before anything else in the request — before the
//! body is even parsed — so an operator running with tasks off gets the
//! same `501` no matter what the client sent. Everything downstream of that
//! gate follows the Task 5 brief's status-code table: `400` for a body that
//! isn't the expected JSON shape, `422` for a `grants` object `Grant::from_json`
//! refuses (or a grant with nowhere for the task to run, or a `budget_tokens`
//! above the agent's own granted budget — `budget_exceeds_grant`, a review
//! addition past the original brief), `404` for an agent id nobody created,
//! `202` with the new task's id on success.
//!
//! [`dispatch`] returns `None` for any path that isn't one of the two task
//! routes, so `http.rs`'s worker loop falls through to `api_native::dispatch`
//! for everything else (including that surface's own `404 not_found` for a
//! malformed task-shaped path, e.g. a missing `task_id`).

use std::sync::{Arc, Mutex};

use serde_json::{json, Value};

use bloomery_core::action::PatchCodec;
use bloomery_core::grant::Grant;
use bloomery_substrate::Substrate;

use crate::api_native::{bad_request, lock_pager, ApiResult};
use crate::pager::Pager;
use crate::task::{TaskRegistry, TaskSpec};

/// A task-creation request with no `max_steps` gets this many. Not sourced
/// from `Config`: unlike the executor bounds (which are resource-safety
/// limits an operator tunes for their hardware), a step budget is closer to
/// a per-request default the same way `create_agent`'s `budget_tokens` is —
/// generous enough that the five-verb loop in practice terminates on `done`
/// long before it, small enough that a runaway re-ask loop still ends.
const DEFAULT_MAX_STEPS: u32 = 20;

#[derive(serde::Deserialize)]
struct CreateTaskReq {
    goal: String,
    /// Kept as raw JSON rather than deserialized straight to `Grant`: a
    /// `Grant` field failing validation would otherwise surface as this
    /// whole request's generic `400 bad_request` (serde wraps the
    /// `#[serde(try_from = "GrantWire")]` failure into an opaque parse
    /// error), losing the specific `422 invalid_grant` shape the brief
    /// requires. Re-serializing this back to a JSON string and calling
    /// `Grant::from_json` on it directly is what recovers the exact
    /// `GrantError`.
    grants: Value,
    #[serde(default)]
    budget_tokens: Option<u64>,
    #[serde(default)]
    max_steps: Option<u32>,
}

/// Routes one task-surface request. `segments` is the same `/`-split,
/// non-empty path `api_native::dispatch` matches on.
pub(crate) fn dispatch<S: Substrate + Send + 'static>(
    pager: &Arc<Mutex<Pager<S>>>,
    registry: &Arc<TaskRegistry>,
    method: &str,
    segments: &[String],
    body: &str,
) -> Option<ApiResult> {
    let parts: Vec<&str> = segments.iter().map(String::as_str).collect();
    match (method, parts.as_slice()) {
        ("POST", ["agents", id, "task"]) => Some(create_task(pager, registry, id, body)),
        ("GET", ["agents", _id, "task", task_id]) => Some(get_task(registry, task_id)),
        _ => None,
    }
}

fn tasks_disabled() -> ApiResult {
    (501, Some(json!({"error": "tasks_disabled"})))
}

fn invalid_grant(detail: String) -> ApiResult {
    (
        422,
        Some(json!({"error": "invalid_grant", "detail": detail})),
    )
}

fn unknown_agent(agent_id: &str) -> ApiResult {
    (
        404,
        Some(json!({"error": "unknown_agent", "agent": agent_id})),
    )
}

fn budget_exceeds_grant(requested: u64, granted: u64) -> ApiResult {
    (
        422,
        Some(json!({
            "error": "budget_exceeds_grant",
            "requested": requested,
            "granted": granted,
        })),
    )
}

fn create_task<S: Substrate + Send + 'static>(
    pager: &Arc<Mutex<Pager<S>>>,
    registry: &Arc<TaskRegistry>,
    agent_id: &str,
    body: &str,
) -> ApiResult {
    // The gate, first, per the Task 5 brief — before the body is even
    // parsed, so a malformed request against a tasks-disabled daemon still
    // reads as "tasks are off" rather than "your JSON was bad".
    let (bounds, journal_path) = {
        let p = match lock_pager(pager) {
            Ok(p) => p,
            Err(poisoned) => return poisoned,
        };
        if !p.tasks_enabled() {
            return tasks_disabled();
        }
        (p.exec_bounds(), p.task_journal_path().to_path_buf())
    };

    let req: CreateTaskReq = match serde_json::from_str(body) {
        Ok(r) => r,
        Err(e) => return bad_request(e),
    };

    let grant = match Grant::from_json(&req.grants.to_string()) {
        Ok(g) => g,
        Err(e) => return invalid_grant(e.to_string()),
    };

    // Task 5 brief: "the task's cwd = first write_root, else first
    // read_root". A grant with neither is structurally valid (a
    // command-only grant) but gives the executors nowhere to resolve a
    // relative path against, so it is refused the same shape as any other
    // grant problem this route reports.
    let cwd = match grant
        .write_roots()
        .first()
        .or_else(|| grant.read_roots().first())
    {
        Some(root) => root.clone(),
        None => {
            return invalid_grant(
                "grant has no read or write roots; the task loop needs a working directory"
                    .to_string(),
            )
        }
    };

    let existing_budget = {
        let p = match lock_pager(pager) {
            Ok(p) => p,
            Err(poisoned) => return poisoned,
        };
        p.agent_budget_granted(agent_id)
    };
    let granted = match existing_budget {
        Some(granted) => granted,
        None => return unknown_agent(agent_id),
    };
    // A request that names a `budget_tokens` above the agent's own granted
    // budget is incoherent — see `TaskSpec::budget_tokens`'s doc comment:
    // `run_task` never reads this field back, only the pager's own `Budget`
    // (set once, at `create_agent` time) governs what an `infer` call may
    // spend. Catching the incoherent request here, rather than silently
    // accepting a number that can never be honored, is cheap because
    // `granted` is already in hand.
    if let Some(requested) = req.budget_tokens {
        if requested > granted {
            return budget_exceeds_grant(requested, granted);
        }
    }
    let budget_tokens = req.budget_tokens.unwrap_or(granted);

    let spec = TaskSpec {
        goal: req.goal,
        grant,
        budget_tokens,
        max_steps: req.max_steps.unwrap_or(DEFAULT_MAX_STEPS),
        cwd,
        // Task 5 brief: "the agent's model profile codec (or default
        // SearchReplace if unprofiled — check what's available; default is
        // fine)". `bloomery_core::profile::Profile` carries no patch-codec
        // field for either a profiled or unprofiled model today, so this
        // always resolves to the default — stated here rather than
        // implying a lookup that doesn't exist.
        patch_codec: PatchCodec::SearchReplace,
        bounds,
    };

    let task_id = registry.spawn_task(Arc::clone(pager), agent_id.to_string(), spec, journal_path);
    (202, Some(json!({"task_id": task_id})))
}

fn get_task(registry: &Arc<TaskRegistry>, task_id: &str) -> ApiResult {
    match registry.get(task_id) {
        Some(result) => (
            200,
            Some(json!({
                "status": result.status,
                "steps": result.steps,
                "summary": result.summary,
            })),
        ),
        None => (404, Some(json!({"error": "not_found"}))),
    }
}
