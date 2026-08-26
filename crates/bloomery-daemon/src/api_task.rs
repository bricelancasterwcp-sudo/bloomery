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

use bloomery_core::grant::Grant;
use bloomery_substrate::Substrate;

use crate::api_native::{bad_request, lock_pager, ApiResult};
use crate::memory::MemoryContext;
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
///
/// `memory` is the daemon's memory organ, threaded straight through to
/// `TaskRegistry::spawn_task` — the same borrowed-`Arc` shape
/// `api_native::dispatch` gives `swap`, and for the same reason: only the
/// spawned worker keeps it past the request, so the route never owns one.
/// `None` is a daemon with no organ wired, which every task then runs
/// memory-off against (memory-organ design §7).
pub(crate) fn dispatch<S: Substrate + Send + 'static>(
    pager: &Arc<Mutex<Pager<S>>>,
    registry: &Arc<TaskRegistry>,
    memory: Option<&Arc<MemoryContext>>,
    method: &str,
    segments: &[String],
    body: &str,
) -> Option<ApiResult> {
    let parts: Vec<&str> = segments.iter().map(String::as_str).collect();
    match (method, parts.as_slice()) {
        ("POST", ["agents", id, "task"]) => Some(create_task(pager, registry, memory, id, body)),
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
    memory: Option<&Arc<MemoryContext>>,
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

    // One lock, two reads: the budget check below needs `agent_budget_granted`
    // and Gate G4 (`docs/superpowers/evidence/2026-08-15-g4-protocol.md`
    // §4/§6) needs `agent_task_policy` — both resolve through the same
    // `agent_id` lookup, so both are fetched here rather than a third pager
    // lock acquisition just for the policy.
    let (existing_budget, task_policy) = {
        let p = match lock_pager(pager) {
            Ok(p) => p,
            Err(poisoned) => return poisoned,
        };
        (
            p.agent_budget_granted(agent_id),
            p.agent_task_policy(agent_id),
        )
    };
    let granted = match existing_budget {
        Some(granted) => granted,
        None => return unknown_agent(agent_id),
    };
    // `agent_task_policy` is `None` under exactly the same condition as
    // `agent_budget_granted` (both key off `self.table.get(agent_id)`), so
    // this arm is unreachable given the check above already passed — matched
    // explicitly rather than unwrapped so that invariant stays a compile-time
    // fact, not an assumption.
    let (patch_codec, mutating_verbs, envelope) = match task_policy {
        Some(policy) => policy,
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
        // Gate G4 protocol (docs/superpowers/evidence/2026-08-15-g4-protocol.md
        // §4/§6): both fields resolved above, in the same lock section as
        // the budget check, through `Pager::agent_task_policy` — the agent's
        // model's attached profile picks the codec (§4), and the model's
        // stored codec-gate verdict picks whether mutating verbs are even
        // available (§6's fail-closed rule: no stored gate reads as
        // demoted, never as permission).
        patch_codec,
        bounds,
        mutating_verbs,
        // Amendments 2/3 (docs/superpowers/evidence/2026-08-15-g4-protocol.md
        // §10/§11): the same `agent_task_policy` one-source tuple as
        // `patch_codec`/`mutating_verbs` above — a v2/v3-configured model's
        // HTTP task renders (and, for v3, stops generation) accordingly,
        // resolved through the identical lookup rather than a second,
        // potentially-drifting path.
        envelope,
        // Retrieval runs "at task start … before step 1" (memory-organ
        // design spec §3), which is the registry worker's moment, not this
        // handler's — it has neither the store nor the task's own journal
        // in hand. `None` is the honest value here and also the permanently
        // correct one: a route that could not retrieve must never claim it
        // did. The worker overwrites it iff it injects.
        memory_block: None,
    };

    let task_id = registry.spawn_task(
        Arc::clone(pager),
        agent_id.to_string(),
        spec,
        journal_path,
        memory.map(Arc::clone),
    );
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
