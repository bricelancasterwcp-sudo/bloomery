//! The pager's serializable snapshot.
//!
//! [`StatusReport`] is what `/status` (Task 14) serves and what a human reads
//! when asking "what is this daemon holding right now". It reports measured
//! facts only: `free_vram_bytes` stays `Option` so an unmeasured probe is
//! rendered as `null` rather than a confident zero, and every window carries
//! the term that bound it.

use bloomery_core::geometry::BoundBy;

use super::codec_gate;
use crate::agents::AgentState;
use crate::drift::ModelDrift;

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
    /// Sum of the effective (declared-clamped) weights charge over every model
    /// whose weights are currently loaded into the substrate — the weights term
    /// of the reservation budget (`avail = budget − overhead − Σ loaded weights −
    /// Σ resident reservations`; Task 3 added the weights term, Task 5's live run
    /// added the other two — see the accounting rule on `Pager::place`). Derived
    /// from the loaded set on every call rather than tracked as a counter, so
    /// `unload_model` crediting weights back is just this sum recomputing; `0`
    /// when nothing is loaded.
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
    /// The effective per-token KV charge (spec §10 addendum): the declared
    /// override when [`ModelStatus::kv_per_token_declared`] is `true`, else
    /// the GGUF-derived value.
    pub kv_per_token: u64,
    /// Whether [`ModelStatus::kv_per_token`] is a DECLARED override
    /// (`kv_per_token_bytes` config) rather than the GGUF-derived value —
    /// spec §10's naming rule: "a declared number must never read as a
    /// measured one," restated for `/status` the same way
    /// `pager::tuning`'s refusal-string naming does for weights.
    pub kv_per_token_declared: bool,
    pub training_ctx: u32,
    /// The patch codec tasks on this model actually run under (protocol
    /// §4) — `"search_replace"` or `"whole_file"`. Always populated: an
    /// unprofiled model still has a codec, the default.
    pub patch_codec: &'static str,
    /// The enforced value of [`crate::pager::Pager::model_mutating_verbs`]
    /// — protocol §3/§6's fail-closed gate. `false` for an unmeasured
    /// model, exactly like [`ModelStatus::codec_gate`] being `None`.
    pub mutating_verbs: bool,
    /// This model's stored G4 gate, or `None` when it has never completed
    /// one. `None` renders as JSON `null` — never a confident zero — so a
    /// reader can tell "measured 0/20" from "never measured" at a glance.
    pub codec_gate: Option<CodecGateStatus>,
    /// The G5 done-trust mark (`docs/superpowers/evidence/2026-08-16-g5-protocol.md`
    /// §3): both class decisions cleared their ≥80% floor. `None` = never
    /// measured — the fail-closed-analog "unmeasured", never a fake pass
    /// (design doc §4). Advisory only: unlike [`ModelStatus::mutating_verbs`],
    /// nothing reads this for enforcement.
    pub done_trust: Option<bool>,
    /// This model's stored G5 gate, or `None` when it has never completed a
    /// mixed-set probe — same null-not-zero rule as [`ModelStatus::codec_gate`].
    pub refusal_gate: Option<RefusalGateStatus>,
    /// This boot's two drift comparisons (drift-watch design §2), or `None`
    /// when the watch never ran for this model — a boot where POST failed, or
    /// one before POST reached it. **Absent is not clean**, the same
    /// None-honesty [`ModelStatus::done_trust`] has: a comparison nobody made
    /// must never render as one that passed. Says nothing about `done_trust`
    /// and nothing about admission — design §7 keeps the two questions apart.
    pub drift: Option<ModelDrift>,
    /// What is currently holding this model out of new admission, or
    /// `None` when nothing is. Rendered beside `drift` because the two
    /// say different things: `drift` is what was measured, this is
    /// whether it is being enforced.
    pub admission_block: Option<crate::drift::AdmissionBlock>,
}

/// One model's stored G4 gate, as `/status` renders it (protocol §5).
/// `mutating_verbs` is deliberately not repeated here — it already lives on
/// [`ModelStatus::mutating_verbs`], the one enforced value, and copying it
/// onto this struct too would be two numbers the wire format promises agree
/// but nothing enforces.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CodecGateStatus {
    pub fixture_set: String,
    pub codec: &'static str,
    pub landed: u32,
    pub n: u32,
    pub interval95: [f64; 2],
    pub provisional: bool,
}

/// One model's stored G5 gate, as `/status` renders it (protocol §3).
/// `done_trust` is deliberately not repeated here — same reasoning as
/// [`CodecGateStatus`] not repeating `mutating_verbs`: it lives on
/// [`ModelStatus::done_trust`], the one rendered value.
#[derive(Debug, Clone, serde::Serialize)]
pub struct RefusalGateStatus {
    pub fixture_set: String,
    pub codec: &'static str,
    pub patch_landed: u32,
    pub patch_n: u32,
    pub patch_interval95: [f64; 2],
    pub patch_provisional: bool,
    pub refuse_landed: u32,
    pub refuse_n: u32,
    pub refuse_interval95: [f64; 2],
    pub refuse_provisional: bool,
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

/// The `/status` document builder.
///
/// Lives beside the types it fills rather than in `pager.rs`: every field
/// below has a doc comment a few lines up explaining what it means, and a
/// builder that drifts from those comments is the failure mode worth
/// designing against.
impl<S: bloomery_substrate::Substrate> crate::pager::Pager<S> {
    /// A serializable snapshot of everything the pager is holding, sorted so
    /// two calls with the same state produce the same document.
    pub fn status(&self) -> StatusReport {
        let mut agents: Vec<AgentStatus> = self
            .table
            .iter()
            .map(|a| AgentStatus {
                id: a.id.clone(),
                model: a.model.clone(),
                priority: a.priority,
                state: state_str(&a.state),
                window_tokens: a.window.tokens,
                bound_by: bound_by_str(a.window.bound_by),
                vram_unmeasured: a.window.vram_unmeasured,
                kv_bytes: a.reserved_bytes,
                budget_granted: a.budget.granted(),
                budget_spent: a.budget.spent(),
            })
            .collect();
        agents.sort_by(|x, y| x.id.cmp(&y.id));
        let mut models: Vec<ModelStatus> = self
            .models
            .iter()
            .map(|(name, m)| ModelStatus {
                name: name.clone(),
                digest: m.digest.clone(),
                loaded: m.handle.is_some(),
                profiled: m.profile.is_some(),
                kv_per_token: m.effective_kv_per_token(),
                kv_per_token_declared: m.kv_per_token_bytes.is_some(),
                training_ctx: m.meta.training_ctx,
                patch_codec: codec_gate::patch_codec_str(codec_gate::resolve_patch_codec(
                    m.profile.as_ref(),
                )),
                mutating_verbs: codec_gate::resolve_mutating_verbs(m.codec_gate.as_ref()),
                codec_gate: m.codec_gate.as_ref().map(|g| CodecGateStatus {
                    fixture_set: g.fixture_set.clone(),
                    codec: codec_gate::patch_codec_str(g.codec),
                    landed: g.landed,
                    n: g.n,
                    interval95: [g.interval95.0, g.interval95.1],
                    provisional: g.provisional,
                }),
                done_trust: m.refusal_gate.as_ref().map(|g| g.done_trust),
                refusal_gate: m.refusal_gate.as_ref().map(|g| RefusalGateStatus {
                    fixture_set: g.fixture_set.clone(),
                    codec: codec_gate::patch_codec_str(g.codec),
                    patch_landed: g.patch_landed,
                    patch_n: g.patch_n,
                    patch_interval95: [g.patch_interval95.0, g.patch_interval95.1],
                    patch_provisional: g.patch_provisional,
                    refuse_landed: g.refuse_landed,
                    refuse_n: g.refuse_n,
                    refuse_interval95: [g.refuse_interval95.0, g.refuse_interval95.1],
                    refuse_provisional: g.refuse_provisional,
                }),
                drift: m.drift.clone(),
                admission_block: m.admission_block.clone(),
            })
            .collect();
        models.sort_by(|x, y| x.name.cmp(&y.name));
        StatusReport {
            free_vram_bytes: (self.free_vram)(),
            overhead_bytes: self.overhead_bytes,
            ctx_overhead_bytes: self.ctx_overhead_bytes,
            resident_kv_bytes: self.resident_reserved_bytes(),
            loaded_weights_bytes: self.loaded_weights_bytes(),
            tier: self.tier.clone(),
            posting: self.posting,
            agents,
            models,
        }
    }
}
