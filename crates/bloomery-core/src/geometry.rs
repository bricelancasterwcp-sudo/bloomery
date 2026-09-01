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

/// `2 (K and V) * attention_layers * kv_heads * head_dim * 2 (f16 bytes)` —
/// the dense-model formula, which applies whenever K and V are stored at the
/// same width. Only layers that own a KV cache count — hybrid models'
/// recurrent layers are charged by `GgufMeta::recurrent_state_bytes` instead
/// (turn-5 spec §2).
///
/// gguf-geometry R9 (SPEC.md), MLA/separate widths: when
/// `GgufMeta.value_length` is stated and differs from `head_dim` (K's
/// width), the leading factor-of-2 is replaced by the explicit K+V sum —
/// `attention_layers * kv_heads * (head_dim + value_length) * 2`. K and V no
/// longer share a width, so "2x head_dim" silently over- or under-charges;
/// the sum is exact regardless of which side is wider. `value_length ==
/// head_dim` stays on the dense formula by identity (the sum equals `2 *
/// head_dim` exactly), and an unstated `value_length` (`None`) is read as
/// the pre-R9 dense case — both fall through to the return below unchanged.
///
/// All math is done in `u64` to avoid overflow on large models.
pub fn kv_bytes_per_token(m: &GgufMeta) -> u64 {
    if let Some(v) = m.value_length {
        if v != m.head_dim {
            // R9 (MLA, separate widths): K and V stated at different widths;
            // the 2-for-K-and-V factor is replaced by the explicit sum.
            // Measured: assay docs/superpowers/evidence/mla-kv-2026-08-27/
            // (ollama 0.32.13, llama runner) — gguf-geometry SPEC.md R9.
            return u64::from(m.attention_layers)
                * u64::from(m.kv_heads)
                * (u64::from(m.head_dim) + u64::from(v))
                * F16_BYTES;
        }
    }

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
    /// Since turn 5 the pager also folds the model's derived per-context
    /// recurrent-state charge (`GgufMeta::recurrent_state_bytes`) into this
    /// term (spec 2026-08-22 §2).
    pub ctx_overhead_bytes: u64,
    pub user_cap: Option<u32>,
    /// assay ceiling.max_verified
    pub measured_ceiling: Option<u32>,
}

/// The largest window, in tokens, that a residency request would actually be
/// able to place right now — the advice a refusal carries so recovery is a
/// re-ask rather than a guess.
///
/// Carried-debt item 7's "third half" is that the window law is blind to
/// sibling residency: `usable_window` subtracts only *this* model's weights,
/// while placement charges every loaded model's weights and every resident's
/// reservation. An agent windowed against the roomier arithmetic can be
/// permanently un-placeable, and the item's own complaint is that there is
/// then "no smaller window to fall back to and no recovery".
///
/// This function is the recovery. It deliberately does **not** change the
/// window law, and nothing here mutates an existing agent — a placement-time
/// downsize was designed, reviewed and rejected on 2026-09-01 (see
/// `docs/CARRIED-DEBT.md`; it destroyed suspended agents' KV images, among
/// four other defects). Advising is safe precisely because it changes no
/// state.
///
/// # Arguments
///
/// `headroom_bytes` is what placement could free at its most aggressive:
/// `avail` plus everything this request is entitled to evict. The advice is
/// therefore an upper bound that assumes maximum reclamation — the refusal
/// reports `reclaimable` alongside it, so the assumption is visible rather
/// than implied. `weights_bytes` is charged only when the model is cold (a
/// loaded model's weights are already outside `avail`), and
/// `per_ctx_extra_bytes` is the reservation beyond the KV cache itself
/// (`ctx_overhead` plus any recurrent state).
///
/// # Honesty rules
///
/// - `None` **only** when `kv_per_token == 0`: with no per-token cost there
///   is no VRAM-bound window to advise and the division is undefined. This is
///   the same case `usable_window` handles by skipping its `Vram` candidate
///   outright — never a confident zero.
/// - `Some(0)` is a real answer: nothing places even after evicting
///   everything eligible. The caller learns there is no window worth
///   retrying, which is a different fact from "we could not work it out".
/// - Never more than `window_cap_tokens`. The caller is recovering from a
///   refusal, so advice above its own window would be advice to ask for
///   something `training_ctx` / `user_cap` / `measured_ceiling` already ruled
///   out.
/// - Every subtraction saturates, so charges exceeding the headroom mean
///   `Some(0)` rather than a vast window conjured by a `u64` underflow.
pub fn max_placeable_window(
    headroom_bytes: u64,
    weights_bytes: u64,
    per_ctx_extra_bytes: u64,
    kv_per_token: u64,
    window_cap_tokens: u32,
) -> Option<u32> {
    if kv_per_token == 0 {
        return None;
    }
    let usable = headroom_bytes
        .saturating_sub(weights_bytes)
        .saturating_sub(per_ctx_extra_bytes);
    let tokens = usable / kv_per_token;
    // `min` before the narrowing cast: the cap is a u32, so a quotient beyond
    // u32::MAX is clamped by it rather than truncated into a small, wrong
    // number by an `as` conversion.
    Some(tokens.min(u64::from(window_cap_tokens)) as u32)
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
