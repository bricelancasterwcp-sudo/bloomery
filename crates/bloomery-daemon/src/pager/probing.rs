//! The candidate probe's admission window (swap-candidate seam design §4
//! step 2): one identity, provisionally admitted, for the length of one probe.
//!
//! Split out of `pager.rs` for the reason `codec_gate.rs` and `tuning.rs`
//! already are — that file is over its 800-line house cap, so new logic gets
//! its own submodule rather than growing the file it would otherwise share.
//! What stays in `pager.rs` is what has to: the field on `ModelEntry` and the
//! one line in `Pager::admit` that reads it.
//!
//! **The chicken-and-egg, a second time.** [`crate::post`] already states it
//! for boot: assay measures a serving state by *driving* it, so the daemon has
//! to answer `/v1` for a model before that model can have a profile, and law 5
//! is supposed to refuse an unprofiled model. Boot resolves it with a stated,
//! bounded suspension — the daemon-global `posting` flag, opened before the
//! socket binds and cleared the moment POST finishes.
//!
//! A swap candidate is the identical problem at a different time. It is
//! registered under a scratch identity ([`crate::swap::scratch_identity`]) so
//! assay can address it, and it has no profile — writing one is the entire
//! point of the probe. The live acceptance
//! (`docs/superpowers/evidence/2026-08-19-swap-candidate-live.md`) proved what
//! follows from having no answer for it: the probe's own request to
//! `/v1/chat/completions` was refused `422`, assay exited 4, `cover` never
//! spawned, and the endpoint reported the candidate `unmeasured, not refused`
//! — twice, byte-identically, on the real daemon.
//!
//! **Why this is scoped per identity and the boot flag is not.** The boot flag
//! has to be daemon-global: POST probes every configured model, and the
//! window's whole job is to cover all of them. A candidate probe measures
//! exactly one name. The live evidence names the near-miss that argues the
//! point better than the failure does — a swap-candidate POST fired *inside*
//! the boot POST window would have been admitted by the global flag, for a
//! reason with nothing to do with this endpoint, and this slice would have
//! appeared to work intermittently and by accident of timing. So the window
//! here is a field on the candidate's own [`super::ModelEntry`]:
//!
//! - **Nothing else becomes admissible.** A configured model that is unprofiled
//!   at the moment a candidate is probed stays refused, exactly as it was a
//!   second earlier. A model under a standing [`crate::drift::AdmissionBlock`]
//!   stays blocked — `admit` checks the block first, before any of this.
//! - **It cannot outlive the registration.** The window is *on* the entry, so
//!   the job's step-7 `unregister_model` closes it structurally on every path
//!   that job can return through, and re-registering a name starts a fresh
//!   entry with the window shut.
//! - **Nor can what it admitted.** An agent minted inside the window would
//!   otherwise be merely *suspended* by step 7 and revived — usable, ungated —
//!   by the next job's re-registration of the same scratch name, because law 5
//!   is checked at agent creation and never per inference. Step 7 evicts every
//!   agent bound to the identity instead; the argument is on
//!   [`Pager::unregister_model`](super::Pager::unregister_model).
//!
//! **What it does expose, honestly.** For the length of one probe — ~10
//! minutes on the enthusiast-16GB tier — anything that can reach this daemon's
//! loopback socket can `POST /v1/chat/completions` naming `{model}!swap-candidate`
//! and get inference on the candidate's weights, because the daemon cannot
//! tell assay's calls from a client's. That is precisely the exposure
//! [`crate::post`] already documents for the boot window ("a replay can bound
//! the window and see what was admitted during it — it cannot tell which of
//! those calls were assay's and which were a client's"), narrowed from every
//! configured model to one scratch name — which is unregistered when the job
//! ends, taking every agent bound to it along
//! ([`Pager::unregister_model`](super::Pager::unregister_model)). No *bare*
//! TOML key can hold that name; a quoted one can, and
//! [`crate::swap::SCRATCH_SUFFIX`] carries that caveat and the reasoning for
//! naming it rather than guarding it. The admission is journaled, once,
//! naming the identity and the reason.
//!
//! **Closing is the caller's discipline, and it is not optional.** The swap
//! job closes the window between the probe and the branch on its result, so it
//! spans exactly the step that drives `/v1` and both ways out of that step
//! share one close (`swap::job::judge`); the HTTP layer closes it where it
//! catches a panicking worker, because an unwind skips the job's own cleanup
//! *and* its unregister (`api_native::spawn_candidate_probe`). The one case
//! where it cannot be closed is a poisoned pager mutex, which needs the same
//! lock — and that daemon is already answering every request with a named
//! `500`, so there is no admission left to gate. Same argument, same wording,
//! as `post::post_with_gate`'s.

use bloomery_substrate::Substrate;

use super::{Pager, PagerError};

impl<S: Substrate> Pager<S> {
    /// Opens `model`'s candidate-probe admission window — see this module's
    /// doc comment for the whole argument.
    ///
    /// `UnknownModel` if `model` was never registered: a window is a property
    /// of a registration, and opening one for a name this pager does not hold
    /// would be a suspension of law 5 attached to nothing, which is exactly
    /// the state nobody could later find and close.
    pub fn open_probe_window(&mut self, model: &str) -> Result<(), PagerError> {
        let entry = self
            .models
            .get_mut(model)
            .ok_or_else(|| PagerError::UnknownModel(model.to_string()))?;
        entry.probe_window = true;
        Ok(())
    }

    /// Closes `model`'s candidate-probe admission window.
    ///
    /// **Infallible, and that is the point.** Every caller is on a path out —
    /// including the ones that are already handling a failure — and a close
    /// that could itself fail would give those paths a second error to decide
    /// what to do with. An unknown model is not a failure here but the
    /// strongest possible success: a window on an entry that no longer exists
    /// is already closed, because it went with the entry.
    pub fn close_probe_window(&mut self, model: &str) {
        if let Some(entry) = self.models.get_mut(model) {
            entry.probe_window = false;
        }
    }

    /// Whether `model`'s candidate-probe window is open. `false` for a model
    /// this pager does not hold — see [`Pager::close_probe_window`] for why
    /// "not registered" reads as "not open" rather than as a question this
    /// cannot answer.
    pub fn probe_window_open(&self, model: &str) -> bool {
        self.models
            .get(model)
            .is_some_and(|entry| entry.probe_window)
    }
}
