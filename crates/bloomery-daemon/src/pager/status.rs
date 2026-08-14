//! The pager's serializable snapshot.
//!
//! [`StatusReport`] is what `/status` (Task 14) serves and what a human reads
//! when asking "what is this daemon holding right now". It reports measured
//! facts only: `free_vram_bytes` stays `Option` so an unmeasured probe is
//! rendered as `null` rather than a confident zero, and every window carries
//! the term that bound it.

use bloomery_core::geometry::BoundBy;

use crate::agents::AgentState;

/// What [`crate::pager::Pager::create_agent`] hands back: the id to use, the
/// window the window law computed, and the term that bound it.
#[derive(Debug, serde::Serialize)]
pub struct AgentInfo {
    pub id: String,
    pub window_tokens: u32,
    pub bound_by: String,
}

/// A whole-pager snapshot: agents sorted by id, models sorted by name.
#[derive(Debug, serde::Serialize)]
pub struct StatusReport {
    /// bloomery's static VRAM budget (see `Pager::new`), not a live driver
    /// read; `None` = unmeasured, not zero.
    pub free_vram_bytes: Option<u64>,
    /// Sum of what the pager believes currently-resident contexts
    /// **reserve**: KV cache plus the per-context runtime overhead
    /// ([`StatusReport::ctx_overhead_bytes`]). The name is Phase 1's and is
    /// kept for wire compatibility; the value is the residency term, which
    /// is the number placement decides on.
    pub resident_kv_bytes: u64,
    /// The daemon-level VRAM margin held back from *both* the window law and
    /// the placement budget (`config.overhead_mib`).
    pub overhead_bytes: u64,
    /// What each resident context reserves beyond its KV cache
    /// (`config.ctx_overhead_mib`) — llama.cpp's per-context compute and
    /// host buffers, which the 2026-08-14 natural-pressure run measured at
    /// 304 MiB + 30 MiB against an 896 MiB KV cache.
    pub ctx_overhead_bytes: u64,
    /// Sum of `weights_bytes` over every model whose weights are currently
    /// loaded into the substrate — the weights term of the reservation
    /// budget (Task 3: `avail = budget − Σ loaded weights − Σ resident kv`,
    /// see the accounting rule on `Pager::place`). Derived from the loaded
    /// set on every call rather than tracked as a counter, so `unload_model`
    /// crediting weights back is just this sum recomputing; `0` when
    /// nothing is loaded.
    pub loaded_weights_bytes: u64,
    /// The operator-declared hardware tier every profile here is marked
    /// with. `None` = the daemon was never told one, never a guessed name.
    pub tier: Option<TierStatus>,
    /// True while the boot-time POST is still probing: unprofiled models
    /// are provisionally admitted for exactly this long, and an operator
    /// reading `/status` deserves to know which regime is in force.
    pub posting: bool,
    pub agents: Vec<AgentStatus>,
    pub models: Vec<ModelStatus>,
}

/// The operator-declared tier, exactly as assay marks its profiles: a name
/// plus whether the hardware was emulated. The mark is not decoration — an
/// unmarked emulated number could masquerade as real hardware.
#[derive(Debug, Clone, serde::Serialize)]
pub struct TierStatus {
    pub name: String,
    pub emulated: bool,
}

#[derive(Debug, serde::Serialize)]
pub struct AgentStatus {
    pub id: String,
    pub model: String,
    pub priority: u8,
    /// `"resident"`, `"suspended"`, or `"fresh"`.
    pub state: &'static str,
    pub window_tokens: u32,
    pub bound_by: &'static str,
    /// True when this agent's window was computed without a VRAM measurement.
    pub vram_unmeasured: bool,
    /// What this agent reserves when resident: KV cache plus
    /// [`StatusReport::ctx_overhead_bytes`]. Name kept from Phase 1; the
    /// value now includes the reservation, because that is what residency
    /// plans against and reporting the bare KV here would be a number that
    /// looks like the accounting and is not.
    pub kv_bytes: u64,
    pub budget_granted: u64,
    pub budget_spent: u64,
}

#[derive(Debug, serde::Serialize)]
pub struct ModelStatus {
    pub name: String,
    pub digest: String,
    /// Whether the substrate currently holds weights for this model.
    pub loaded: bool,
    /// Whether a measured capability profile is attached. `false` means
    /// every request for it is refused unless the daemon is still `posting`
    /// or the operator set `allow_unprofiled` — so this is the field that
    /// explains a `422`.
    pub profiled: bool,
    pub kv_per_token: u64,
    pub training_ctx: u32,
}

/// Stable wire spelling for the binding term of the window law.
pub(crate) fn bound_by_str(b: BoundBy) -> &'static str {
    match b {
        BoundBy::TrainingCtx => "training_ctx",
        BoundBy::Vram => "vram",
        BoundBy::UserCap => "user_cap",
        BoundBy::MeasuredCeiling => "measured_ceiling",
    }
}

pub(crate) fn state_str(s: &AgentState) -> &'static str {
    match s {
        AgentState::Resident { .. } => "resident",
        AgentState::Suspended => "suspended",
        AgentState::Fresh => "fresh",
    }
}
