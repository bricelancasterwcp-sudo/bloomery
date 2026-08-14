//! Stats contract enforcement.
//!
//! Project law 4: a substrate reply that omits token stats is an
//! infrastructure failure, never a model failure. `Reply` allows the gap so
//! backends can be built incrementally, but nothing downstream of
//! `enforce_contract` should ever see it — this is the single chokepoint
//! that turns a missing count into a first-class, catchable
//! `ContractViolation` instead of a silently wrong metric.

use crate::Reply;

/// A [`Reply`] whose stats have been verified present.
#[derive(Debug, Clone, PartialEq)]
pub struct VerifiedReply {
    pub text: String,
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub duration_ms: u64,
}

/// Ways a [`Reply`] can fail the stats contract.
#[derive(Debug, PartialEq)]
pub enum ContractViolation {
    MissingStats,
}

/// Verify that `r` reports both token counts, promoting it to a
/// [`VerifiedReply`]. Either count missing is a [`ContractViolation::MissingStats`].
pub fn enforce_contract(r: Reply) -> Result<VerifiedReply, ContractViolation> {
    match (r.prompt_tokens, r.completion_tokens) {
        (Some(prompt_tokens), Some(completion_tokens)) => Ok(VerifiedReply {
            text: r.text,
            prompt_tokens,
            completion_tokens,
            duration_ms: r.duration_ms,
        }),
        _ => Err(ContractViolation::MissingStats),
    }
}
