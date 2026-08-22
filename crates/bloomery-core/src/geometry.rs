//! The window law: the usable context window is always *computed*, never
//! read from a config file or the model's advertised training context.
//!
//! `usable_window` evaluates every known constraint (training context, free
//! VRAM, an operator-set cap, and a previously measured ceiling) and reports
//! both the resulting token count and which constraint bound it. Callers
//! must never silently assume a term that wasn't actually measured — see
//! `Window::vram_unmeasured`.

use crate::gguf::GgufMeta;

/// Bytes-per-f16 element multiplier: 2 bytes per K or V entry.
const F16_BYTES: u64 = 2;
/// K and V are each stored, hence the leading factor of 2.
const KV_TENSORS: u64 = 2;

/// `2 (K and V) * attention_layers * kv_heads * head_dim * 2 (f16 bytes)`.
/// Only layers that own a KV cache count — hybrid models' recurrent layers
/// are charged by `GgufMeta::recurrent_state_bytes` instead (turn-5 spec §2).
///
/// All math is done in `u64` to avoid overflow on large models.
pub fn kv_bytes_per_token(m: &GgufMeta) -> u64 {
    KV_TENSORS
        * u64::from(m.attention_layers)
        * u64::from(m.kv_heads)
        * u64::from(m.head_dim)
        * F16_BYTES
}

/// Which term of the window law bound the final `usable_window` result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum BoundBy {
    /// The model's own training context length.
    TrainingCtx,
    /// Free VRAM after subtracting weights and overhead.
    Vram,
    /// An operator-supplied cap.
    UserCap,
    /// A previously measured ceiling (e.g. assay's `ceiling.max_verified`).
    MeasuredCeiling,
}

/// The computed usable context window, along with which term bound it.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Window {
    pub tokens: u32,
    pub bound_by: BoundBy,
    /// True when `GeometryInput.free_vram_bytes` was `None`: the VRAM term
    /// was skipped entirely rather than treated as zero available memory.
    pub vram_unmeasured: bool,
}

/// Inputs to the window law. `free_vram_bytes` and `measured_ceiling` are
/// optional because they may not have been measured yet; `None` means
/// "unmeasured", never "zero".
pub struct GeometryInput {
    pub training_ctx: u32,
    pub kv_per_token: u64,
    pub weights_bytes: u64,
    /// `None` = unmeasured (law 5), never 0.
    pub free_vram_bytes: Option<u64>,
    pub overhead_bytes: u64,
    /// Per-context runtime reservation (llama.cpp's per-context compute
    /// buffers) beyond the KV cache — placement already charges this via
    /// `Agent::reserved_bytes`, so the VRAM term must charge it too, or a
    /// `Vram`-bound window is sized to consume memory it can never actually
    /// get. Closes carried-debt item 7 (docs/CARRIED-DEBT.md) — see
    /// `docs/superpowers/specs/2026-08-15-partial-offload-capability-window-design.md`
    /// §3b for the derivation and the live 14B attempt that found it.
    pub ctx_overhead_bytes: u64,
    pub user_cap: Option<u32>,
    /// assay ceiling.max_verified
    pub measured_ceiling: Option<u32>,
}

/// Computes the usable context window: the minimum of every applicable
/// constraint, always reporting which one bound it.
///
/// Candidates are evaluated in declaration order (`TrainingCtx`, `Vram`,
/// `UserCap`, `MeasuredCeiling`); the minimum token count wins. On a tie,
/// the earlier-declared term is reported — a later term only displaces the
/// current winner when it is strictly smaller.
pub fn usable_window(i: &GeometryInput) -> Window {
    let mut candidates: Vec<(u32, BoundBy)> = vec![(i.training_ctx, BoundBy::TrainingCtx)];

    // kv_per_token == 0 would make the division below undefined. Zero cost
    // per token also means VRAM can never be the binding constraint (there
    // is nothing to divide free memory by), so the Vram candidate is
    // skipped entirely rather than panicking or reporting a zero window.
    // VRAM itself was still measured in that case, so `vram_unmeasured`
    // (set below from `free_vram_bytes.is_none()`) is unaffected.
    if let Some(free_vram_bytes) = i.free_vram_bytes.filter(|_| i.kv_per_token != 0) {
        let remaining = free_vram_bytes
            .saturating_sub(i.weights_bytes)
            .saturating_sub(i.overhead_bytes)
            .saturating_sub(i.ctx_overhead_bytes);
        // Saturate rather than truncate: an upstream units bug (e.g. bits
        // instead of bytes) could otherwise produce a quotient larger than
        // u32::MAX, which a raw `as u32` cast would silently wrap.
        let vram_tokens = u32::try_from(remaining / i.kv_per_token).unwrap_or(u32::MAX);
        candidates.push((vram_tokens, BoundBy::Vram));
    }

    if let Some(user_cap) = i.user_cap {
        candidates.push((user_cap, BoundBy::UserCap));
    }

    if let Some(measured_ceiling) = i.measured_ceiling {
        candidates.push((measured_ceiling, BoundBy::MeasuredCeiling));
    }

    let (tokens, bound_by) = candidates
        .into_iter()
        .reduce(|winner, candidate| {
            if candidate.0 < winner.0 {
                candidate
            } else {
                winner
            }
        })
        .expect("candidates always contains at least the training_ctx term");

    Window {
        tokens,
        bound_by,
        vram_unmeasured: i.free_vram_bytes.is_none(),
    }
}
