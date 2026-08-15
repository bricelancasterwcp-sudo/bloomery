//! Per-model codec-gate state (Phase 2b/2c P4, G4 codec-landing protocol —
//! `docs/superpowers/evidence/2026-08-15-g4-protocol.md`).
//!
//! [`CodecGateResult`] is what a completed G4 probe stores on a model; the
//! two accessors below are the whole enforcement surface a task-dispatch
//! caller needs, and both are **fail-closed**: a model that was never
//! probed, that is still probing, or that aborted mid-probe (§3's
//! infrastructure-abort case — no verdict recorded at all) reads exactly
//! like a demoted model, never like a permissive default. Split out of
//! `pager.rs` for the same reason `status.rs` and `task_config.rs` are —
//! `pager.rs` sits right under its 800-line house cap, so new logic gets
//! its own submodule rather than growing the file it would otherwise share.

use bloomery_core::action::PatchCodec;
use bloomery_core::profile::Profile;
use bloomery_substrate::Substrate;

use super::journal as jrnl;
use super::PagerError;

/// One completed G4 codec-gate verdict for a model (protocol §5):
/// `landed`/`n`/`interval95` are the Wilson-95 measurement, `provisional`
/// marks (but never changes) a decision whose interval straddled the 0.80
/// threshold, and `mutating_verbs` is the enforced decision itself —
/// `landed * 5 >= n * 4`. `codec` is the codec the probe was actually run
/// under (protocol §4's selection), recorded alongside the verdict because
/// a re-probe after a profile update could select a different one.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CodecGateResult {
    pub fixture_set: String,
    pub codec: PatchCodec,
    pub landed: u32,
    pub n: u32,
    pub interval95: (f64, f64),
    pub provisional: bool,
    pub mutating_verbs: bool,
}

/// Stable wire spelling for a [`PatchCodec`] — `/status` and the journal
/// both name it by these two strings (protocol §4), never the Rust variant
/// name.
pub(crate) fn patch_codec_str(codec: PatchCodec) -> &'static str {
    match codec {
        PatchCodec::SearchReplace => "search_replace",
        PatchCodec::WholeFile => "whole_file",
    }
}

/// Protocol §4's codec-selection rule: the attached profile's
/// [`Profile::preferred_patch_codec`] when it made a selection, else
/// [`PatchCodec::SearchReplace`] — the robigo-proven default, for "no
/// profile at all", "profile has no `codecs` grid", and "neither codec's
/// cell was measured" alike. Also what an unprofiled or unregistered model
/// resolves to, since `profile: None` collapses into the same `unwrap_or`.
pub(crate) fn resolve_patch_codec(profile: Option<&Profile>) -> PatchCodec {
    profile
        .and_then(Profile::preferred_patch_codec)
        .unwrap_or(PatchCodec::SearchReplace)
}

/// Protocol §3/§6's fail-closed rule, in one expression: mutating verbs are
/// enabled **only** when a gate is stored AND that gate's own verdict says
/// so. No stored gate is never read as permission — see the module doc for
/// the three ways a model can have none.
pub(crate) fn resolve_mutating_verbs(gate: Option<&CodecGateResult>) -> bool {
    gate.is_some_and(|g| g.mutating_verbs)
}

impl<S: Substrate> crate::pager::Pager<S> {
    /// Stores `model`'s completed G4 gate verdict (the probe driver's
    /// result, Task 9). Replaces any previous gate wholesale: protocol §6
    /// says demotion is per-boot state and a restart re-measures, so the
    /// newest completed probe is always the one in force — there is no
    /// notion of merging an old verdict with a new one.
    pub fn set_codec_gate(&mut self, model: &str, gate: CodecGateResult) -> Result<(), PagerError> {
        let entry = self
            .models
            .get_mut(model)
            .ok_or_else(|| PagerError::UnknownModel(model.to_string()))?;
        entry.codec_gate = Some(gate);
        Ok(())
    }

    /// Whether `model` may dispatch mutating verbs (`patch`/`write`/`run`)
    /// right now — protocol §3/§6's fail-closed gate. `true` iff a gate is
    /// stored **and** `gate.mutating_verbs`; `false` for an unmeasured
    /// model, an unknown model, and a model whose stored gate demoted it.
    /// The absence of a measurement is never permission.
    pub fn model_mutating_verbs(&self, model: &str) -> bool {
        resolve_mutating_verbs(
            self.models
                .get(model)
                .and_then(|entry| entry.codec_gate.as_ref()),
        )
    }

    /// The patch codec `model`'s tasks should run under — protocol §4. Also
    /// the answer for an unknown or unprofiled model: see
    /// [`resolve_patch_codec`].
    pub fn model_patch_codec(&self, model: &str) -> PatchCodec {
        resolve_patch_codec(
            self.models
                .get(model)
                .and_then(|entry| entry.profile.as_ref()),
        )
    }

    /// The `(patch_codec, mutating_verbs)` pair a running task dispatches
    /// under, resolved through `agent_id`'s model. `None` when `agent_id`
    /// names no agent — the same `404`-shaped signal
    /// [`Pager::agent_budget_granted`] gives Task 5's task-creation route,
    /// for the codec-gate policy Task 8's route needs alongside it.
    pub fn agent_task_policy(&self, agent_id: &str) -> Option<(PatchCodec, bool)> {
        let model = &self.table.get(agent_id)?.model;
        Some((
            self.model_patch_codec(model),
            self.model_mutating_verbs(model),
        ))
    }

    /// Journals one G4 fixture run outcome (protocol §2/§3). Same
    /// single-writer reason as [`Pager::journal_post`]: the codec probe
    /// (Task 9) runs outside the pager but must not open a second writer
    /// onto the journal the pager owns — two `BufWriter`s appending to one
    /// audit log is exactly the interleaving nobody can replay, so the
    /// probe records through the pager and there stays one writer.
    #[allow(clippy::too_many_arguments)]
    pub fn journal_codec_fixture(
        &mut self,
        model: &str,
        fixture_set: &str,
        fixture: &str,
        codec: PatchCodec,
        landed: bool,
        steps: u32,
        detail: &str,
    ) -> Result<(), PagerError> {
        jrnl::codec_fixture(
            &mut self.journal,
            model,
            fixture_set,
            fixture,
            patch_codec_str(codec),
            landed,
            steps,
            detail,
        )
    }

    /// Journals the per-model G4 verdict (protocol §5), emitted exactly
    /// once per completed probe — same single-writer reason as
    /// [`Pager::journal_codec_fixture`].
    #[allow(clippy::too_many_arguments)]
    pub fn journal_codec_verdict(
        &mut self,
        model: &str,
        fixture_set: &str,
        codec: PatchCodec,
        landed: u32,
        n: u32,
        interval95: (f64, f64),
        provisional: bool,
        mutating_verbs: bool,
        detail: &str,
    ) -> Result<(), PagerError> {
        jrnl::codec_verdict(
            &mut self.journal,
            model,
            fixture_set,
            patch_codec_str(codec),
            landed,
            n,
            interval95,
            provisional,
            mutating_verbs,
            detail,
        )
    }
}
