//! Journal emission for the pager.
//!
//! Every one of these helpers returns `Result<(), PagerError>` on purpose: a
//! failed append is *not* swallowed. The journal is the record the whole
//! system is audited against (project law 7 — replayable, fail-loud), so a
//! pager that cannot write it is a broken pager, not a pager that quietly
//! keeps paging. Callers use `?`, which means an unwritable journal surfaces
//! as [`PagerError::Substrate`] — the enum's infrastructure catch-all — even
//! on refusal paths, where the alternative would be reporting a tidy refusal
//! we failed to record.

use std::time::Duration;

use bloomery_core::journal::{sha256_hex, AgentId, Event, Journal, PagerOpKind};
use bloomery_substrate::contract::VerifiedReply;

use super::PagerError;

fn append(j: &mut Journal, e: &Event) -> Result<(), PagerError> {
    j.append(e)
        .map_err(|err| PagerError::Substrate(format!("journal append failed: {err}")))
}

fn millis(d: Duration) -> u64 {
    u64::try_from(d.as_millis()).unwrap_or(u64::MAX)
}

pub(crate) fn agent_created(
    j: &mut Journal,
    id: &str,
    model: &str,
    priority: u8,
    window_tokens: u32,
    bound_by: &str,
    budget_granted: u64,
) -> Result<(), PagerError> {
    append(
        j,
        &Event::AgentCreated {
            id: id.to_string(),
            model: model.to_string(),
            priority,
            window_tokens,
            bound_by: bound_by.to_string(),
            budget_granted,
        },
    )
}

pub(crate) fn scheduler_decision(
    j: &mut Journal,
    id: &str,
    decision: &str,
    evicted: &[AgentId],
) -> Result<(), PagerError> {
    append(
        j,
        &Event::SchedulerDecision {
            id: id.to_string(),
            decision: decision.to_string(),
            evicted: evicted.to_vec(),
        },
    )
}

pub(crate) fn refusal(
    j: &mut Journal,
    id: &str,
    needed_tokens: u64,
    window_tokens: u32,
    detail: String,
) -> Result<(), PagerError> {
    append(
        j,
        &Event::Refusal {
            id: id.to_string(),
            needed_tokens,
            window_tokens,
            detail,
        },
    )
}

pub(crate) fn budget_refused(
    j: &mut Journal,
    id: &str,
    remaining: u64,
    requested: u64,
) -> Result<(), PagerError> {
    append(
        j,
        &Event::BudgetRefused {
            id: id.to_string(),
            remaining,
            requested,
        },
    )
}

/// The prompt is journaled verbatim *and* hashed: the hash is what survives
/// log truncation or redaction, the text is what makes a replay reproducible.
pub(crate) fn infer_started(j: &mut Journal, id: &str, prompt: &str) -> Result<(), PagerError> {
    append(
        j,
        &Event::InferStarted {
            id: id.to_string(),
            prompt: prompt.to_string(),
            prompt_sha256: sha256_hex(prompt),
        },
    )
}

pub(crate) fn infer_completed(
    j: &mut Journal,
    id: &str,
    reply: &VerifiedReply,
) -> Result<(), PagerError> {
    append(
        j,
        &Event::InferCompleted {
            id: id.to_string(),
            prompt_tokens: reply.prompt_tokens,
            completion_tokens: reply.completion_tokens,
            duration_ms: reply.duration_ms,
        },
    )
}

pub(crate) fn contract_violation(j: &mut Journal, id: &str, kind: &str) -> Result<(), PagerError> {
    append(
        j,
        &Event::ContractViolation {
            id: id.to_string(),
            kind: kind.to_string(),
        },
    )
}

/// `image_tier` is the tier the bytes *actually* moved through, not the one
/// that was intended: a spill that failed reports `"ram"`.
pub(crate) fn pager_op(
    j: &mut Journal,
    id: &str,
    op: PagerOpKind,
    bytes: u64,
    elapsed: Duration,
    image_tier: &str,
) -> Result<(), PagerError> {
    append(
        j,
        &Event::PagerOp {
            id: id.to_string(),
            op,
            bytes,
            duration_ms: millis(elapsed),
            image_tier: image_tier.to_string(),
        },
    )
}

pub(crate) fn model_loaded(j: &mut Journal, model: &str, d: Duration) -> Result<(), PagerError> {
    append(
        j,
        &Event::ModelLoaded {
            model: model.to_string(),
            duration_ms: millis(d),
        },
    )
}

pub(crate) fn model_unloaded(j: &mut Journal, model: &str) -> Result<(), PagerError> {
    append(
        j,
        &Event::ModelUnloaded {
            model: model.to_string(),
        },
    )
}

pub(crate) fn degraded(j: &mut Journal, reason: String) -> Result<(), PagerError> {
    append(j, &Event::Degraded { reason })
}
