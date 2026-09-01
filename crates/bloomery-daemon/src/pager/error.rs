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
    /// Model has a profile, and this boot's cumulative drift comparison
    /// settled `Confirmed` against the blessed baseline named here
    /// (design §2). Distinct from `Unprofiled`: something WAS measured,
    /// and what it measured was a reproduced regression.
    DriftBlocked {
        model: String,
        reference: String,
    },
    /// Residency arithmetic, in bytes.
    Refused {
        needed: u64,
        free: u64,
        reclaimable: u64,
        /// The largest window that WOULD place right now, so a refused caller
        /// can re-ask instead of guessing — carried-debt item 7's "third
        /// half" complains that a refusal leaves "no smaller window to fall
        /// back to and no recovery", and this is the recovery.
        ///
        /// Assumes maximum reclamation (every resident this request may
        /// evict); `reclaimable` sits beside it so that assumption is
        /// visible. `Some(0)` means nothing places even then. `None` means no
        /// window is advisable — either VRAM is unmeasured, so the refusal is
        /// residency-count-shaped rather than byte-shaped, or `kv_per_token`
        /// is 0 and no VRAM-bound window exists. Never a confident zero for
        /// a number nobody computed.
        max_placeable_tokens: Option<u32>,
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
            PagerError::DriftBlocked { model, reference } => write!(
                f,
                "model {model} is drift-blocked: cumulative comparison confirmed a regression against baseline {reference}"
            ),
            PagerError::Refused {
                needed,
                free,
                reclaimable,
                max_placeable_tokens,
            } => {
                write!(
                    f,
                    "residency refused: needed {needed} B, free {free} B, reclaimable {reclaimable} B"
                )?;
                // Omitted rather than rendered as "none": a caller reading
                // this line should see advice or nothing, never a word it has
                // to interpret as an absent number.
                match max_placeable_tokens {
                    Some(tokens) => write!(f, "; largest placeable window {tokens} tokens"),
                    None => Ok(()),
                }
            }
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
