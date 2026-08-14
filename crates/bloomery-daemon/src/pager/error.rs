//! The pager's error surface.
//!
//! The variants are fixed by the Task 13 brief because Tasks 14–15 map them
//! onto HTTP status codes and refusal JSON. Two infrastructure failures the
//! pager owns rather than the substrate — an unreadable `.gguf` at
//! registration and an unwritable journal — therefore surface as
//! [`PagerError::Substrate`], the enum's infrastructure catch-all. They are
//! never downgraded to a degraded success: a pager that cannot identify its
//! weights, or cannot record what it did, is broken, not merely degraded.

use bloomery_substrate::SubstrateError;

#[derive(Debug)]
pub enum PagerError {
    UnknownModel(String),
    UnknownAgent(String),
    /// Model has no profile and `allow_unprofiled=false` (the daemon's call
    /// in Task 16 — the pager itself accepts `profile: None`).
    Unprofiled(String),
    /// Residency arithmetic, in bytes.
    Refused {
        needed: u64,
        free: u64,
        reclaimable: u64,
    },
    PromptTooLarge {
        needed_tokens: u64,
        window_tokens: u32,
    },
    Budget {
        remaining: u64,
        requested: u64,
    },
    Contract(String),
    Substrate(String),
}

impl std::fmt::Display for PagerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PagerError::UnknownModel(m) => write!(f, "unknown model: {m}"),
            PagerError::UnknownAgent(a) => write!(f, "unknown agent: {a}"),
            PagerError::Unprofiled(m) => write!(f, "model {m} has no capability profile"),
            PagerError::Refused {
                needed,
                free,
                reclaimable,
            } => write!(
                f,
                "residency refused: needed {needed} B, free {free} B, reclaimable {reclaimable} B"
            ),
            PagerError::PromptTooLarge {
                needed_tokens,
                window_tokens,
            } => write!(
                f,
                "prompt needs ~{needed_tokens} tokens, window is {window_tokens}"
            ),
            PagerError::Budget {
                remaining,
                requested,
            } => write!(f, "budget exhausted: {remaining} left, {requested} asked"),
            PagerError::Contract(m) => write!(f, "substrate contract violation: {m}"),
            PagerError::Substrate(m) => write!(f, "substrate error: {m}"),
        }
    }
}

impl std::error::Error for PagerError {}

/// The message inside any [`SubstrateError`], whatever its shape.
///
/// The pager matches on message *content* in exactly one place (the
/// `STATE_SIZE_MISMATCH` marker), so it needs the text without caring which
/// variant carried it.
pub(crate) fn substrate_msg(e: &SubstrateError) -> String {
    match e {
        SubstrateError::ModelLoad(m)
        | SubstrateError::Context(m)
        | SubstrateError::Infer(m)
        | SubstrateError::State(m) => m.clone(),
    }
}

pub(crate) fn sub(e: SubstrateError) -> PagerError {
    PagerError::Substrate(substrate_msg(&e))
}
