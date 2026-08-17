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

/// Appends one already-built event.
///
/// `pub(crate)` for exactly one caller: the drift watch's row is constructed
/// by `drift::drift_event`, beside the gate that produced the reading, so that
/// the row cannot describe a different pair of documents than the comparison
/// read. Every other event in this module is built here, where the `Event`
/// variant and its call site are one function apart.
pub(crate) fn append(j: &mut Journal, e: &Event) -> Result<(), PagerError> {
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

/// One blessing (drift-watch design §2). `sha` is the digest of the blessed
/// document's **bytes**, so the row's path claim is checkable with
/// `sha256sum`; `provenance` names who decided, never inferred.
pub(crate) fn blessed(
    j: &mut Journal,
    model: &str,
    profile_path: &str,
    sha: &str,
    provenance: &str,
) -> Result<(), PagerError> {
    append(
        j,
        &Event::Blessed {
            model: model.to_string(),
            profile_path: profile_path.to_string(),
            sha: sha.to_string(),
            provenance: provenance.to_string(),
        },
    )
}

pub(crate) fn agent_removed(j: &mut Journal, id: &str, reason: &str) -> Result<(), PagerError> {
    append(
        j,
        &Event::AgentRemoved {
            id: id.to_string(),
            reason: reason.to_string(),
        },
    )
}

/// One POST outcome. `profile_path` is `Some` only when a profile was
/// actually written and attached — a failed probe records `None` rather
/// than the path assay was *asked* to write, which may not exist.
pub(crate) fn post(
    j: &mut Journal,
    model: &str,
    outcome: &str,
    profile_path: Option<String>,
) -> Result<(), PagerError> {
    append(
        j,
        &Event::Post {
            model: model.to_string(),
            outcome: outcome.to_string(),
            profile_path,
        },
    )
}

/// One G4/G5 fixture run outcome (protocol §2/§3,
/// `codec_gate::CodecGateResult`'s per-fixture evidence). `codec` is already
/// the wire spelling (`"search_replace"`/`"whole_file"`) — callers convert
/// once via `codec_gate::patch_codec_str` rather than this module knowing
/// about [`bloomery_core::action::PatchCodec`]. `expect` is the fixture's
/// class wire spelling (`"patch"`/`"refuse"`, G5 design doc §2) — every
/// caller before G5 passed `"patch"` implicitly by being the only class that
/// existed; now it is explicit at every call site.
#[allow(clippy::too_many_arguments)]
pub(crate) fn codec_fixture(
    j: &mut Journal,
    model: &str,
    fixture_set: &str,
    fixture: &str,
    codec: &str,
    landed: bool,
    steps: u32,
    detail: &str,
    expect: &str,
) -> Result<(), PagerError> {
    append(
        j,
        &Event::CodecFixture {
            model: model.to_string(),
            fixture_set: fixture_set.to_string(),
            fixture: fixture.to_string(),
            codec: codec.to_string(),
            landed,
            steps,
            detail: detail.to_string(),
            expect: expect.to_string(),
        },
    )
}

/// The per-model G4 verdict (protocol §5), emitted exactly once per
/// completed probe — never for an aborted one, per protocol §3.
#[allow(clippy::too_many_arguments)]
pub(crate) fn codec_verdict(
    j: &mut Journal,
    model: &str,
    fixture_set: &str,
    codec: &str,
    landed: u32,
    n: u32,
    interval95: (f64, f64),
    provisional: bool,
    mutating_verbs: bool,
    detail: &str,
) -> Result<(), PagerError> {
    append(
        j,
        &Event::CodecVerdict {
            model: model.to_string(),
            fixture_set: fixture_set.to_string(),
            codec: codec.to_string(),
            landed,
            n,
            interval95: [interval95.0, interval95.1],
            provisional,
            mutating_verbs,
            detail: detail.to_string(),
        },
    )
}

/// The per-model G5 mixed-set verdict (design doc §2/§4), emitted exactly
/// once per completed mixed-set probe — same "never for an aborted one"
/// rule as [`codec_verdict`], and never emitted for the same probe that
/// also emits [`codec_verdict`] (a set is either all-`patch`, classic G4
/// path, or mixed, G5 path — never both).
#[allow(clippy::too_many_arguments)]
pub(crate) fn codec_verdict_mixed(
    j: &mut Journal,
    model: &str,
    fixture_set: &str,
    codec: &str,
    envelope: &str,
    patch_landed: u32,
    patch_n: u32,
    patch_interval95: (f64, f64),
    patch_provisional: bool,
    refuse_landed: u32,
    refuse_n: u32,
    refuse_interval95: (f64, f64),
    refuse_provisional: bool,
    done_trust: bool,
    detail: &str,
) -> Result<(), PagerError> {
    append(
        j,
        &Event::CodecVerdictMixed {
            model: model.to_string(),
            fixture_set: fixture_set.to_string(),
            codec: codec.to_string(),
            envelope: envelope.to_string(),
            patch_landed,
            patch_n,
            patch_interval95: [patch_interval95.0, patch_interval95.1],
            patch_provisional,
            refuse_landed,
            refuse_n,
            refuse_interval95: [refuse_interval95.0, refuse_interval95.1],
            refuse_provisional,
            done_trust,
            detail: detail.to_string(),
        },
    )
}
