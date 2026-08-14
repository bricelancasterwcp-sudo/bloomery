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
    /// The live VRAM probe at snapshot time; `None` = unmeasured, not zero.
    pub free_vram_bytes: Option<u64>,
    /// Sum of the KV footprints the pager believes are currently resident.
    pub resident_kv_bytes: u64,
    pub agents: Vec<AgentStatus>,
    pub models: Vec<ModelStatus>,
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
