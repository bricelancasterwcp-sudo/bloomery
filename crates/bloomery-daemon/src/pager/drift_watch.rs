//! The drift watch's surface on the pager: where this boot's readings are
//! stored, and the two journal rows the watch emits
//! (`docs/superpowers/specs/2026-08-17-drift-watch-design.md` §2, §4).
//!
//! Same shape and same reasons as `pager::codec_gate`'s: the watch runs
//! outside the pager (in POST's boot thread) but must not open a second writer
//! onto the journal the pager owns — two `BufWriter`s appending to one audit
//! log is exactly the interleaving nobody can replay. So it records through
//! the pager, and there stays one writer.
//!
//! Nothing here touches `done_trust`, `codec_gate` or admission. Design §7 is
//! explicit: drift answers "has what assay can measure about this serving path
//! changed", G4/G5 answer "does this model do bloomery's task honestly", and
//! the two are separate fields on purpose.

use bloomery_substrate::Substrate;

use super::{journal as jrnl, PagerError};
use crate::drift::{drift_event, Comparison, GateReading, ModelDrift};

impl<S: Substrate> crate::pager::Pager<S> {
    /// Stores `model`'s pair of drift readings for this boot.
    ///
    /// Wholesale replacement, exactly like [`Pager::set_codec_gate`]: drift is
    /// measured once per boot per model, so the newest pair is the only one
    /// there is — there is no notion of merging this boot's reading with a
    /// previous boot's, and the journal is where the history lives.
    ///
    /// [`Pager::set_codec_gate`]: crate::pager::Pager::set_codec_gate
    pub fn set_drift(&mut self, model: &str, drift: ModelDrift) -> Result<(), PagerError> {
        let entry = self
            .models
            .get_mut(model)
            .ok_or_else(|| PagerError::UnknownModel(model.to_string()))?;
        entry.drift = Some(drift);
        Ok(())
    }

    /// Journals one comparison (design §4's row).
    ///
    /// The row is built by `drift::drift_event` from the reading itself rather
    /// than from arguments spelled out here, so it cannot come to describe a
    /// different pair of documents — or different bytes — than the gate
    /// actually compared.
    pub fn journal_drift(
        &mut self,
        model: &str,
        comparison: Comparison,
        reading: &GateReading,
    ) -> Result<(), PagerError> {
        jrnl::append(&mut self.journal, &drift_event(model, comparison, reading))
    }

    /// Journals a blessing (design §2): which document became the
    /// drift-cumulative baseline, its digest, and who decided.
    ///
    /// `provenance` is `"operator"` or
    /// [`PROVENANCE_AUTO_FIRST`](crate::drift::PROVENANCE_AUTO_FIRST) — the
    /// provenance of every baseline is explicit, so a replay can always say
    /// who decided this document is the reference.
    pub fn journal_blessed(
        &mut self,
        model: &str,
        profile_path: &str,
        sha: &str,
        provenance: &str,
    ) -> Result<(), PagerError> {
        jrnl::blessed(&mut self.journal, model, profile_path, sha, provenance)
    }
}
