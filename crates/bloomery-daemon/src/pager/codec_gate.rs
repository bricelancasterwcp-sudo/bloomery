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

use crate::config::EnvelopeLens;

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

/// One completed G5 refusal-honesty verdict for a model
/// (`docs/superpowers/evidence/2026-08-16-g5-protocol.md` §3): the per-class
/// Wilson-95 measurement — mirroring [`CodecGateResult`]'s shape, doubled,
/// one side per class — plus `done_trust`, the AND of both classes'
/// independent `gate_decision` calls (protocol §3: "Classes are never
/// blended"). **Advisory only**: unlike [`CodecGateResult::mutating_verbs`],
/// nothing here is read by `model_mutating_verbs` or any dispatch-time
/// enforcement — G5 does not demote, it only marks `/status`'s done-trust
/// field (design doc §3).
#[derive(Debug, Clone, serde::Serialize)]
pub struct RefusalGateResult {
    pub fixture_set: String,
    pub codec: PatchCodec,
    pub patch_landed: u32,
    pub patch_n: u32,
    pub patch_interval95: (f64, f64),
    pub patch_provisional: bool,
    pub refuse_landed: u32,
    pub refuse_n: u32,
    pub refuse_interval95: (f64, f64),
    pub refuse_provisional: bool,
    pub done_trust: bool,
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

    /// Stores `model`'s completed G5 refusal-honesty gate (`run_refusal_probe`'s
    /// result). Same wholesale-replace semantics as [`Pager::set_codec_gate`]:
    /// the newest completed mixed-set probe is always the one `/status`
    /// renders. **Never touches `codec_gate`** — G5 is advisory (design doc
    /// §3), so this has no effect on `model_mutating_verbs` at all.
    pub fn set_refusal_gate(
        &mut self,
        model: &str,
        gate: RefusalGateResult,
    ) -> Result<(), PagerError> {
        let entry = self
            .models
            .get_mut(model)
            .ok_or_else(|| PagerError::UnknownModel(model.to_string()))?;
        entry.refusal_gate = Some(gate);
        Ok(())
    }

    /// Whether `model` may dispatch mutating verbs (`patch`/`run`)
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

    /// Whether [`Pager::model_patch_codec`]'s answer for `model` came from
    /// its attached profile's measured `codecs` grid (protocol §4's first
    /// three rules) rather than the `SearchReplace` fallback (§4's fourth).
    ///
    /// The codec value alone cannot answer this — `SearchReplace` is both a
    /// legitimate measured selection *and* the default — and §4 requires the
    /// G4 verdict's `detail` to record which of the two it was ("codec from
    /// profile" vs "default (codecs unmeasured)"), so the probe reads this
    /// beside the codec itself. `false` for an unknown or unprofiled model,
    /// matching `resolve_patch_codec`'s own `unwrap_or` collapse.
    pub fn model_codec_from_profile(&self, model: &str) -> bool {
        self.models
            .get(model)
            .and_then(|entry| entry.profile.as_ref())
            .and_then(Profile::preferred_patch_codec)
            .is_some()
    }

    /// The `(patch_codec, mutating_verbs, envelope)` triple a running task
    /// dispatches under, resolved through `agent_id`'s model. `None` when
    /// `agent_id` names no agent — the same `404`-shaped signal
    /// [`Pager::agent_budget_granted`] gives Task 5's task-creation route,
    /// for the codec-gate policy Task 8's route needs alongside it.
    ///
    /// `envelope` (protocol §10/§11, Amendments 2 and 3) joined this tuple
    /// rather than growing a separate accessor path: `api_task.rs::create_task`
    /// is this method's one caller with a live `agent_id`, and resolving all
    /// three fields through the one lookup keeps `patch_codec`/
    /// `mutating_verbs`/`envelope` from ever being read through two
    /// different call paths that could drift apart (the same one-source rule
    /// `pager::tuning`'s module doc states for `effective_weights_bytes`).
    /// The codec probe has no agent yet when it needs this model's
    /// `envelope` (it reads it before creating one, alongside
    /// `model_patch_codec`/`model_codec_from_profile`), so it reads the same
    /// underlying [`Pager::model_envelope`] directly instead.
    pub fn agent_task_policy(&self, agent_id: &str) -> Option<(PatchCodec, bool, EnvelopeLens)> {
        let model = &self.table.get(agent_id)?.model;
        Some((
            self.model_patch_codec(model),
            self.model_mutating_verbs(model),
            self.model_envelope(model),
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
        expect: &str,
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
            expect,
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

    /// Journals the per-model G5 mixed-set verdict (protocol §3), emitted
    /// exactly once per completed mixed-set probe — same single-writer
    /// reason as [`Pager::journal_codec_fixture`]. `envelope` and `gate`'s
    /// per-class numbers travel structured on the event (unlike the classic
    /// verdict, which folds the envelope name into `detail`); `detail` here
    /// carries codec-selection provenance only.
    #[allow(clippy::too_many_arguments)]
    pub fn journal_codec_verdict_mixed(
        &mut self,
        model: &str,
        fixture_set: &str,
        codec: PatchCodec,
        envelope: &str,
        gate: &RefusalGateResult,
        detail: &str,
    ) -> Result<(), PagerError> {
        jrnl::codec_verdict_mixed(
            &mut self.journal,
            model,
            fixture_set,
            patch_codec_str(codec),
            envelope,
            gate.patch_landed,
            gate.patch_n,
            gate.patch_interval95,
            gate.patch_provisional,
            gate.refuse_landed,
            gate.refuse_n,
            gate.refuse_interval95,
            gate.refuse_provisional,
            gate.done_trust,
            detail,
        )
    }
}
