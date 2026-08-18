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

use std::path::{Path, PathBuf};

use bloomery_substrate::Substrate;

use super::{journal as jrnl, PagerError};
use crate::drift::{
    confirm_event, drift_event, operator_provenance, Blessing, Comparison, DriftError, DriftStatus,
    GateReading, ModelDrift, ProfileStore,
};

/// Why an operator's blessing did not happen (or, for
/// [`BlessError::Journal`], did not get recorded).
///
/// Deliberately **not** folded into [`PagerError`]: that enum's variants are
/// fixed by the Task 13 brief and map one-to-one onto HTTP status codes on two
/// surfaces (`api_native`, `api_v1`), so widening it for a route only one
/// surface serves would ripple through both tables. Each variant here is a
/// different operator action, which is what a caller maps to a status code.
#[derive(Debug)]
pub enum BlessError {
    /// This daemon serves no model by that name. Carries the name so the
    /// caller can answer with the surface's one existing unknown-model shape
    /// rather than a second spelling of it.
    UnknownModel(String),
    /// No profiles directory was ever wired
    /// ([`Pager::set_profiles_dir`](crate::pager::Pager::set_profiles_dir)).
    /// Infrastructure: there is nowhere to file a baseline that a later boot
    /// would read back.
    NoProfilesDir,
    /// The store refused or failed — most often
    /// [`DriftError::NoCurrentProfile`], the expected answer on a daemon whose
    /// POST never ran or failed for this model.
    Store(DriftError),
    /// The baseline was written but the journal would not take the row. Law 7:
    /// said, never swallowed — and the blessing has already happened on disk,
    /// which is the part a caller must be able to tell an operator.
    Journal(PagerError),
}

impl std::fmt::Display for BlessError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BlessError::UnknownModel(model) => write!(f, "unknown model: {model}"),
            BlessError::NoProfilesDir => write!(
                f,
                "this daemon has no profiles directory configured, so there is nowhere \
                 to file a baseline"
            ),
            BlessError::Store(e) => write!(f, "{e}"),
            BlessError::Journal(e) => write!(
                f,
                "the baseline was replaced but the journal refused the row: {e}"
            ),
        }
    }
}

impl std::error::Error for BlessError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            BlessError::UnknownModel(_) | BlessError::NoProfilesDir => None,
            BlessError::Store(e) => Some(e),
            BlessError::Journal(e) => Some(e),
        }
    }
}

impl<S: Substrate> crate::pager::Pager<S> {
    /// Sets the profiles directory the drift watch files into
    /// (`config.data_dir/profiles` in `main.rs`, the same one `run_post` is
    /// handed).
    ///
    /// The pager needs it for exactly one thing: the operator bless route,
    /// which runs on a request thread with nothing but this shared pager to
    /// reach. The boot-time watch keeps taking the directory as an argument —
    /// it has one in hand already, and reading it back off the pager would
    /// invite the two to disagree about where a boot's profiles live.
    pub fn set_profiles_dir(&mut self, dir: PathBuf) {
        self.profiles_dir = Some(dir);
    }

    /// The profiles directory this daemon was wired with, or `None` when
    /// nothing wired one.
    pub fn profiles_dir(&self) -> Option<&Path> {
        self.profiles_dir.as_deref()
    }

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

        // Design §2: the CUMULATIVE comparison decides, and only `Confirmed`
        // blocks. Derived here, at the moment the reading settles, so the
        // block and the reading it came from are written in one place and
        // cannot disagree.
        let block = match &drift.cumulative {
            DriftStatus::Confirmed { reference } => Some(crate::drift::AdmissionBlock {
                reference: reference.clone(),
            }),
            // Every other outcome admits. An operator-cleared block is NOT
            // resurrected here: a later boot's non-Confirmed reading
            // legitimately clears it, because the comparison was re-run and
            // came back otherwise.
            _ => None,
        };

        entry.drift = Some(drift);
        entry.admission_block = block.clone();

        // Task 4's row, not Task 2's: a block newly standing is journaled
        // here, at the moment it is derived, with the drift watch's own
        // provenance — the counterpart to the "cleared" row an operator's
        // later `clear_admission_block` writes (design §4/§7). `entry`'s
        // borrow has already ended above, so this is free to take
        // `&mut self.journal`.
        if let Some(block) = block {
            jrnl::admission(
                &mut self.journal,
                model,
                "blocked",
                &block.reference,
                crate::drift::PROVENANCE_DRIFT_WATCH,
            )?;
        }
        Ok(())
    }

    /// The block currently holding `model` out of new admission, if any
    /// (design §2/§3). `Some` iff this boot's stored `ModelDrift.cumulative`
    /// settled `Confirmed` — see [`Pager::set_drift`], the only place that
    /// invariant is established.
    pub fn admission_block_for(&self, model: &str) -> Option<&crate::drift::AdmissionBlock> {
        self.models
            .get(model)
            .and_then(|e| e.admission_block.as_ref())
    }

    /// Clears this model's admission block on an operator's say-so
    /// (`POST /models/{name}/unblock`, design §4), and journals who did it.
    ///
    /// Touches neither the drift reading nor the blessed baseline: the
    /// reading is a measurement and never changes here, and re-baselining
    /// is `bless`'s job, taking effect at the next boot. This says only
    /// "admit it anyway, now" — `bless` and `unblock` answer different
    /// questions, and neither implies the other.
    ///
    /// `Ok(None)` is "known model, nothing was blocking" — **not** an error:
    /// the request is well-formed and the model exists, only the daemon's
    /// state conflicts with it. That is the route's 409, deliberately never
    /// a silent 200 — answering 200 where nothing was blocking would tell an
    /// operator they cleared something when nothing was written, the same
    /// reasoning `bless_baseline`'s 409 rests on.
    pub fn clear_admission_block(
        &mut self,
        model: &str,
    ) -> Result<Option<crate::drift::AdmissionBlock>, PagerError> {
        let entry = self
            .models
            .get_mut(model)
            .ok_or_else(|| PagerError::UnknownModel(model.to_string()))?;
        let Some(block) = entry.admission_block.take() else {
            return Ok(None);
        };
        jrnl::admission(
            &mut self.journal,
            model,
            "cleared",
            &block.reference,
            crate::drift::PROVENANCE_OPERATOR,
        )?;
        Ok(Some(block))
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

    /// Journals a confirm's re-diff (design §4's second row for one
    /// comparison).
    ///
    /// Separate from [`Pager::journal_drift`] because the two rows say
    /// different things: a first reading's row carries the gate's raw outcome,
    /// while a confirm's row carries the verdict that reading finally settled
    /// on — `confirmed` / `transient` / `unconfirmed: …`. Same reading behind
    /// both, so paths, digests and exit code still describe the comparison
    /// that produced them.
    ///
    /// [`Pager::journal_drift`]: crate::pager::Pager::journal_drift
    pub fn journal_confirm(
        &mut self,
        model: &str,
        comparison: Comparison,
        reading: &GateReading,
        settled: &DriftStatus,
    ) -> Result<(), PagerError> {
        jrnl::append(
            &mut self.journal,
            &confirm_event(model, comparison, reading, settled),
        )
    }

    /// Blesses `model`'s current profile as its drift-cumulative baseline on an
    /// operator's say-so (design §2's explicit operator action), and journals
    /// who decided.
    ///
    /// The order is the contract, and it is why this composes here rather than
    /// in the HTTP layer:
    ///
    /// 1. **Unknown model refuses first**, before anything is read or written.
    ///    A baseline filed for a model this daemon does not serve would be
    ///    evidence about a model nobody measured.
    /// 2. **The replaced identity is read before the copy** — after it, those
    ///    bytes are gone and no digest of them can ever be taken again (see
    ///    [`operator_provenance`]).
    /// 3. **Copy, then journal.** The same order the auto-blessing takes: a row
    ///    written first could claim a blessing that then failed, which is the
    ///    one direction that puts a false fact in the audit log. The other
    ///    direction — a real blessing whose row would not write — is
    ///    [`BlessError::Journal`], which says exactly that.
    ///
    /// **This boot's drift reading is deliberately not recomputed.** Blessing
    /// changes the *cumulative reference*, and the comparison that consumes it
    /// happens at the next boot; re-deriving a status here would either restate
    /// a comparison nobody re-ran or silently clear a `Confirmed` reading that
    /// still stands. Nothing on `ModelStatus` changes until the next boot
    /// measures against the new baseline.
    pub fn bless_baseline(&mut self, model: &str) -> Result<Blessing, BlessError> {
        if !self.models.contains_key(model) {
            return Err(BlessError::UnknownModel(model.to_string()));
        }
        let dir = self.profiles_dir.clone().ok_or(BlessError::NoProfilesDir)?;
        let store = ProfileStore::new(dir);
        let provenance = operator_provenance(&store, model);
        let blessing = store.bless(model).map_err(BlessError::Store)?;
        self.journal_blessed(
            model,
            &blessing.path.display().to_string(),
            &blessing.sha,
            &provenance,
        )
        .map_err(BlessError::Journal)?;
        Ok(blessing)
    }

    /// Journals a blessing (design §2): which document became the
    /// drift-cumulative baseline, its digest, and who decided.
    ///
    /// `provenance` is a **family, not a closed set** — a consumer prefix-matches
    /// it (the wire contract lives on
    /// [`Event::Blessed`](bloomery_core::journal::Event::Blessed)):
    ///
    /// - exactly [`PROVENANCE_AUTO_FIRST`](crate::drift::PROVENANCE_AUTO_FIRST),
    ///   from the boot watch's first-profile blessing, which never runs over an
    ///   existing baseline;
    /// - prefix [`PROVENANCE_OPERATOR`](crate::drift::PROVENANCE_OPERATOR), from
    ///   [`Pager::bless_baseline`](crate::pager::Pager::bless_baseline) — bare
    ///   when nothing was replaced, `"operator (replaced <sha256>)"` when this
    ///   blessing overwrote a baseline (see
    ///   [`operator_provenance`], the one place that string is built).
    ///
    /// Either way the provenance of every baseline is explicit, so a replay can
    /// always say who decided this document is the reference — and, when one was
    /// superseded, which document it displaced.
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
