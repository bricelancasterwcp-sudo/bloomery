//! Confirm-then-alarm: what one boot's drift watch decides, and how it says
//! it (`docs/superpowers/specs/2026-08-17-drift-watch-design.md` §2, §4).
//!
//! The gate ([`super::DriftGate`]) answers one comparison at a time and never
//! re-probes. This module is the boot policy above it: rotate, compare against
//! both references, test a drift *reading* with one confirm re-probe before it
//! becomes a *finding*, and bless the first profile a model ever produces.
//! Everything it decides lands in two places — the journal (a row per
//! comparison, spec §4) and [`ModelDrift`] on the pager, which `/status`
//! renders beside `done_trust`.
//!
//! **A first reading is never an alarm.** Assay's founding finding is that
//! this daemon's own failures can be state-transient (the 11.5k ceiling that
//! vanished without a restart). So `Drift` from the gate is a hypothesis: the
//! confirm probe measures the serving state a second time and the same
//! reference is diffed again. Only agreement is [`DriftStatus::Confirmed`];
//! disagreement is [`DriftStatus::Transient`] (itself a finding); a confirm
//! that could not be made at all is [`DriftStatus::Unconfirmed`], named — the
//! first reading stands as what it was and is never upgraded.

use std::path::Path;
use std::sync::Mutex;

use bloomery_substrate::Substrate;

use super::{Comparison, DriftGate, GateOutcome, GateReading, ProfileStore, SHA_PREFIX_LEN};
use crate::config::Tier;
use crate::pager::{Pager, PagerError};
use crate::post::PostRunner;

/// What one comparison finally decided, after the confirm stage.
///
/// Wider than [`GateOutcome`] by exactly the confirm stage: the gate can say
/// `Drift`, but nothing here can — a drift reading either reproduces
/// ([`DriftStatus::Confirmed`]), does not ([`DriftStatus::Transient`]), or
/// could not be tested ([`DriftStatus::Unconfirmed`]). Every variant is named
/// and there is no default, for the same reason [`GateOutcome`] has none: the
/// failure worth designing against is a comparison nobody could make being
/// rendered as one that passed.
///
/// Serialized internally tagged, so `/status` reads
/// `{"status":"within-noise"}` / `{"status":"confirmed","reference":"a1b2…"}`
/// — the same kebab-case spellings the journal's `outcome` field uses.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
pub enum DriftStatus {
    /// The two documents differ by no more than assay's noise discipline
    /// allows.
    WithinNoise,
    /// Drift beyond noise, and the confirm re-probe reproduced it — spec §4's
    /// alarm.
    Confirmed {
        /// The reference's identity: the first [`SHA_PREFIX_LEN`] hex of the
        /// sha256 of its **bytes**, the same claim the journal row carries in
        /// full. Identity, never a transcribed measurement.
        reference: String,
    },
    /// Drift beyond noise that did NOT reproduce: the serving state moved
    /// between two probes of one boot. A finding of its own (assay's founding
    /// finding), not a clean bill of health.
    Transient,
    /// A drift reading whose confirm could not be made — the confirm probe
    /// failed, its document could not be retained, or the re-diff itself
    /// refused. The first reading stands as a reading; it is never promoted to
    /// [`DriftStatus::Confirmed`] on a confirm that did not happen.
    Unconfirmed {
        /// What prevented the confirm, in the failing layer's own words.
        reason: String,
    },
    /// Spec §3: the two documents were measured by different instruments, so
    /// nothing between them is a statement about the model. Never a pass,
    /// never a fail — it stays this way until the operator re-blesses.
    InstrumentChanged {
        /// The reference document's instrument identity.
        reference: String,
        /// The current document's instrument identity.
        current: String,
    },
    /// There was nothing to compare: a missing or unreadable reference (first
    /// boot ever, a baseline nobody blessed), a crossed pair, or a gate that
    /// could not run at all. Named with the reason — never a silent pass.
    Unmeasured {
        /// Which side, and why.
        reason: String,
    },
    /// `assay diff` itself refused the pair (its exit 2). Infrastructure-shaped
    /// and not a drift verdict, so no confirm is run: there is no drift
    /// hypothesis to test.
    NotComparable,
}

impl DriftStatus {
    /// The name this verdict takes in the journal — the same kebab-case
    /// vocabulary [`GateOutcome::journal_outcome`] uses, extended by the three
    /// words only the confirm stage can produce (`confirmed`, `transient`,
    /// `unconfirmed: …`).
    ///
    /// Read by [`confirm_event`](super::confirm_event): a confirm's row spells
    /// what was settled, not the raw gate outcome underneath it, so
    /// `confirmed` cannot be mistaken for the `drift` reading that triggered
    /// it and `transient` cannot be mistaken for a clean `within-noise` boot.
    /// The context-carrying variants fold their context in, exactly as the
    /// gate's own outcomes do — identity and prose, never a transcribed
    /// measurement.
    pub fn journal_outcome(&self) -> String {
        match self {
            DriftStatus::WithinNoise => "within-noise".to_string(),
            DriftStatus::Confirmed { .. } => "confirmed".to_string(),
            DriftStatus::Transient => "transient".to_string(),
            DriftStatus::Unconfirmed { reason } => format!("unconfirmed: {reason}"),
            DriftStatus::InstrumentChanged { reference, current } => {
                format!("instrument-changed ({reference} -> {current})")
            }
            DriftStatus::Unmeasured { reason } => format!("unmeasured: {reason}"),
            DriftStatus::NotComparable => "not-comparable".to_string(),
        }
    }
}

/// One model's pair of drift readings for this boot — spec §2's two
/// comparisons, always both, each with its own verdict.
///
/// They are kept side by side rather than folded into one "worst" status
/// because they answer different questions: step alone leaks the ratchet, and
/// cumulative alone goes stale the moment anything legitimately changes.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ModelDrift {
    /// This boot's profile against the previous boot's.
    pub step: DriftStatus,
    /// This boot's profile against the blessed baseline.
    pub cumulative: DriftStatus,
}

/// The provenance [`bloomery_core::journal::Event::Blessed`] carries when the
/// daemon blessed a model's first profile itself (spec §2) — the only blessing
/// this daemon ever decides on its own, and only when no baseline exists.
pub const PROVENANCE_AUTO_FIRST: &str = "auto-first-profile";

/// The provenance an operator's own blessing carries (spec §2's explicit
/// operator action, `POST /models/{name}/bless`).
///
/// A *replacing* blessing extends this rather than replacing it — see
/// [`operator_provenance`](crate::drift::operator_provenance), the one place
/// that spelling is built.
pub const PROVENANCE_OPERATOR: &str = "operator";

/// Rotates `model`'s current profile into `previous`, before POST probes it.
///
/// **The order is a law, not a preference** (spec §5). POST deletes the
/// current document before running assay, so a rotation after the probe either
/// finds nothing — this boot's step reference silently lost — or promotes this
/// boot's own measurement to be its own reference, which is a gate that can
/// only ever read within-noise. `tests/drift_test.rs` pins it by asserting
/// `previous` holds last boot's bytes.
///
/// Only a *degraded* rotation is journaled. A clean rotation adds no fact the
/// step row does not already carry (its `reference_path` and `reference_sha`
/// name exactly what rotation promoted), whereas a document that could not be
/// promoted degrades the drift record and would otherwise vanish: POST's
/// delete-before-probe reclaims those bytes moments later, so the journal row
/// is what remains of them.
pub(crate) fn rotate_for_boot<S: Substrate>(
    pager: &Mutex<Pager<S>>,
    store: &ProfileStore,
    model: &str,
) -> Result<(), PagerError> {
    let degraded = match store.rotate(model) {
        Ok(super::Rotation::Rotated { .. }) | Ok(super::Rotation::NothingToRotate { .. }) => {
            return Ok(())
        }
        Ok(super::Rotation::KeptUnparseable { current, reason }) => format!(
            "drift: {} is not a profile ({reason}), so it was not promoted to {model}'s previous \
             profile; the drift-step reference is now older than one boot",
            current.display()
        ),
        Err(e) => format!(
            "drift: could not rotate {model}'s profile: {e}; the drift-step reference is now \
             older than one boot"
        ),
    };
    crate::post::with_pager(pager, |p| p.journal_degraded(degraded))
}

/// Runs spec §2's two comparisons for `model`, confirms whatever tripped, and
/// stores the pair on the pager.
///
/// Called once per model per boot, only after POST's probe succeeded: with no
/// current document there is nothing to compare, and `ModelDrift` stays absent
/// rather than reading clean.
///
/// **Auto-blessing runs after the comparisons, by ruling.** A model with no
/// baseline blesses this boot's profile as one — but only once the cumulative
/// comparison has already answered, which on that boot is honestly
/// `unmeasured`. Blessing first would hand the gate a baseline byte-identical
/// to the current document and manufacture a within-noise pass out of nothing.
///
/// **The two comparisons confirm independently.** Each one that reads `Drift`
/// earns its own confirm probe (spec §4: "re-run the identical POST probe …
/// then diff again against the same reference"), so the rare boot where step
/// *and* cumulative both trip costs two confirm probes rather than reusing one
/// document for both answers. The alternative — one shared re-probe — is
/// cheaper and was considered; it is not what §4 describes, and a boot where
/// both references disagree with this one is exactly the boot worth spending
/// a second measurement on.
///
/// Returns `Err` only when the journal or the pager itself failed (law 7) —
/// every drift outcome, including the infrastructure-shaped ones, is a value.
pub(crate) fn watch_model<S: Substrate>(
    pager: &Mutex<Pager<S>>,
    runner: &PostRunner,
    gate: &DriftGate,
    store: &ProfileStore,
    model: &str,
    port: u16,
    tier: &Tier,
) -> Result<(), PagerError> {
    let paths = store.paths(model);
    let ctx = Ctx {
        runner,
        gate,
        store,
        model,
        port,
        tier,
    };
    let step = compare(
        pager,
        &ctx,
        Comparison::Step,
        &paths.previous,
        &paths.current,
    )?;
    let cumulative = compare(
        pager,
        &ctx,
        Comparison::Cumulative,
        &paths.baseline,
        &paths.current,
    )?;
    if !paths.baseline.exists() {
        auto_bless(pager, store, model)?;
    }
    crate::post::with_pager(pager, |p| {
        p.set_drift(model, ModelDrift { step, cumulative })
    })
}

/// Everything one model's comparisons need, gathered so the confirm path can
/// be a function of its own rather than a closure over seven bindings.
struct Ctx<'a> {
    runner: &'a PostRunner,
    gate: &'a DriftGate,
    store: &'a ProfileStore,
    model: &'a str,
    port: u16,
    tier: &'a Tier,
}

/// One comparison, journaled exactly once, plus the confirm it may earn.
fn compare<S: Substrate>(
    pager: &Mutex<Pager<S>>,
    ctx: &Ctx<'_>,
    comparison: Comparison,
    reference: &Path,
    current: &Path,
) -> Result<DriftStatus, PagerError> {
    let reading = ctx.gate.compare(reference, current);
    journal_reading(pager, ctx.model, comparison, &reading)?;
    match settled(&reading) {
        Some(status) => Ok(status),
        // The one outcome a first reading cannot settle. Structurally at most
        // one confirm per comparison: this is the only call, and it never
        // loops back into `compare`.
        None => confirm(pager, ctx, comparison, reference),
    }
}

/// The status a first reading settles on its own — `None` for the one outcome
/// (`Drift`) that spec §4 says must be tested before it means anything.
fn settled(reading: &GateReading) -> Option<DriftStatus> {
    match &reading.outcome {
        GateOutcome::WithinNoise => Some(DriftStatus::WithinNoise),
        GateOutcome::Drift => None,
        GateOutcome::NotComparable { .. } => Some(DriftStatus::NotComparable),
        GateOutcome::InstrumentChanged { reference, current } => {
            Some(DriftStatus::InstrumentChanged {
                reference: reference.clone(),
                current: current.clone(),
            })
        }
        GateOutcome::Unmeasured { reason } => Some(DriftStatus::Unmeasured {
            reason: reason.clone(),
        }),
        // A gate that could not run measured nothing, so it reads as
        // unmeasured with the infrastructure failure named — not as a verdict
        // in either direction, and not as a drift hypothesis worth a confirm.
        GateOutcome::Infra { detail } => Some(DriftStatus::Unmeasured {
            reason: format!("infra: {detail}"),
        }),
    }
}

/// Spec §4's confirm: one fresh probe of the same instrument, retained by
/// content, then the SAME reference diffed against it.
///
/// The confirm document is retained *before* it is compared, so the row's
/// `current_path` names a file that still exists — a path claim nobody can
/// check is not evidence. It is also never attached to the pager: this boot's
/// profile is the measurement POST already took, and a confirm run is a second
/// look at the serving state, not a replacement for it.
fn confirm<S: Substrate>(
    pager: &Mutex<Pager<S>>,
    ctx: &Ctx<'_>,
    comparison: Comparison,
    reference: &Path,
) -> Result<DriftStatus, PagerError> {
    let staging = ctx.store.confirm_staging(ctx.model);
    // The identical POST invocation (spec §4: "a confirmation under a
    // different instrument would be a different measurement"), to a fresh
    // path. The parsed profile is deliberately dropped — see above.
    if let Err(e) = ctx.runner.probe(ctx.port, ctx.model, ctx.tier, &staging) {
        // Spec §4: "a wedged confirm journals as infrastructure and the first
        // reading stands as unconfirmed (named)". Journaled rather than left
        // in `ModelStatus` alone, because this failure can cost the whole
        // `assay.probe_timeout_secs` window and then vanish with the process —
        // and a status field is not a record. There is no comparison to
        // journal (no second document exists), so the row is a `Degraded`
        // naming the model, the comparison and the probe's own words.
        let reason = e.to_string();
        crate::post::with_pager(pager, |p| {
            p.journal_degraded(format!(
                "drift: the confirm probe for {}'s {} comparison failed: {reason}; the first \
                 reading stands as unconfirmed and is never upgraded",
                ctx.model,
                comparison.as_str()
            ))
        })?;
        return Ok(DriftStatus::Unconfirmed { reason });
    }
    let retention = match ctx.store.retain_transient(ctx.model, &staging) {
        Ok(retention) => retention,
        Err(e) => {
            let reason = format!(
                "could not retain the confirm document {}: {e}",
                staging.display()
            );
            crate::post::with_pager(pager, |p| p.journal_degraded(format!("drift: {reason}")))?;
            return Ok(DriftStatus::Unconfirmed { reason });
        }
    };
    for dropped in &retention.dropped {
        crate::post::with_pager(pager, |p| {
            p.journal_degraded(format!(
                "drift: dropped {} to stay within {}'s bound of {} retained confirm profiles",
                dropped.display(),
                ctx.model,
                super::MAX_TRANSIENTS
            ))
        })?;
    }
    let again = ctx.gate.compare(reference, &retention.retained);
    let settled = match &again.outcome {
        GateOutcome::Drift => DriftStatus::Confirmed {
            reference: reference_identity(&again),
        },
        GateOutcome::WithinNoise => DriftStatus::Transient,
        // Anything else is a re-diff that did not answer the question. The
        // first reading stands, named by what the re-diff said instead —
        // never upgraded to Confirmed on a comparison that refused.
        other => DriftStatus::Unconfirmed {
            reason: other.journal_outcome(),
        },
    };
    // The verdict is decided before the row is written, because the row spells
    // the SETTLED verdict rather than the raw re-diff outcome — see
    // `drift::confirm_event`. One row per confirm, as spec §4's "+1 row"
    // allows; there is no third row.
    crate::post::with_pager(pager, |p| {
        p.journal_confirm(ctx.model, comparison, &again, &settled)
    })?;
    Ok(settled)
}

/// Blesses a model's first profile as its baseline and journals the
/// provenance (spec §2). A blessing that cannot be written degrades the drift
/// record — the next boot has no cumulative reference — so it is journaled and
/// the boot continues; POST never fails a model over its drift bookkeeping.
fn auto_bless<S: Substrate>(
    pager: &Mutex<Pager<S>>,
    store: &ProfileStore,
    model: &str,
) -> Result<(), PagerError> {
    match store.bless(model) {
        Ok(blessing) => crate::post::with_pager(pager, |p| {
            p.journal_blessed(
                model,
                &blessing.path.display().to_string(),
                &blessing.sha,
                PROVENANCE_AUTO_FIRST,
            )
        }),
        Err(e) => crate::post::with_pager(pager, |p| {
            p.journal_degraded(format!(
                "drift: could not bless {model}'s first profile as its baseline: {e}; the \
                 cumulative comparison stays unmeasured until a blessing succeeds"
            ))
        }),
    }
}

/// Spec §4's journal rule: every comparison records one row, built from the
/// reading itself so it cannot describe a different pair of documents than the
/// gate compared. Law 7 applies — a journal that will not write aborts the
/// boot path rather than proceeding unrecorded.
fn journal_reading<S: Substrate>(
    pager: &Mutex<Pager<S>>,
    model: &str,
    comparison: Comparison,
    reading: &GateReading,
) -> Result<(), PagerError> {
    crate::post::with_pager(pager, |p| p.journal_drift(model, comparison, reading))
}

/// The reference's identity for [`DriftStatus::Confirmed`]: the sha prefix of
/// the bytes the gate actually read. `unread` is unreachable on this path — a
/// `Drift` outcome means both sides parsed — and is a named placeholder rather
/// than an `unwrap` that would turn a bookkeeping surprise into a panic inside
/// the boot thread.
fn reference_identity(reading: &GateReading) -> String {
    match &reading.reference_sha {
        Some(sha) => sha[..SHA_PREFIX_LEN.min(sha.len())].to_string(),
        None => "unread".to_string(),
    }
}
