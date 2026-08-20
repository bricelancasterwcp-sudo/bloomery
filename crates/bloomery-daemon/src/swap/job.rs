//! Design §4's flow for one swap candidate, start to finish: register the
//! candidate under a scratch identity, probe it, retain what the probe wrote,
//! cover it against the blessed floor, journal the verdict, and give the
//! registry back exactly as it was found.
//!
//! The gate ([`super::CoverGate`]) answers one comparison and never probes.
//! This module is the job above it — the swap-candidate counterpart of
//! `drift::watch`, and separated from the vocabulary it drives for the same
//! reason: what one comparison *means* and what one job *does* are two
//! subjects, and the file that holds both is the file nobody can read.
//!
//! Nothing here spawns a thread. Every collaborator is a parameter, so the
//! whole flow runs synchronously in tests with no python, no assay and no GPU;
//! the HTTP layer is what puts it on a thread.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use bloomery_core::gguf::parse_gguf_meta;
use bloomery_core::journal::sha256_hex_bytes;
use bloomery_substrate::Substrate;

use super::{
    scratch_identity, CandidateReading, CoverGate, SwapOutcomeReport, SwapSlot, NOTES, UNREAD,
};
use crate::agents::model_digest;
use crate::config::Tier;
use crate::drift::{ProfileStore, MAX_TRANSIENTS};
use crate::pager::{Pager, PagerError};
use crate::post::{with_pager, PostRunner};

/// Runs design §4's flow for one candidate, start to finish, on the caller's
/// thread.
///
/// Every collaborator is a parameter — the pager, both subprocess seams, the
/// profile store, the port and tier the probe runs against, and the slot — so
/// the whole job is driven synchronously in tests with no python, no assay and
/// no GPU. The HTTP layer is what puts it on a thread; nothing here spawns one.
///
/// **The order is the contract:**
///
/// 1. Digest the candidate's weights and the floor **first**. Both are cheap
///    reads that decide whether the job can mean anything, and doing them
///    before the scratch registration keeps the two failures they can produce
///    off the cleanup path entirely.
/// 2. Register the candidate under [`scratch_identity`] — the same
///    `parse_gguf_meta` + `Pager::register_model` pair `main.rs` registers
///    every configured model with, so a candidate is admitted through exactly
///    the arithmetic a configured model is.
/// 3. Probe it through this daemon's own `/v1` with POST's identical
///    invocation ([`PostRunner::probe`], which deletes the target document
///    first, so an earlier job's document can never be read back as this
///    one's).
/// 4. Retain the document content-named beside the drift transients, under the
///    same bound ([`ProfileStore::retain_transient`]) — design §4 step 3. Every
///    later mention of the candidate's profile, the cover invocation included,
///    is of the retained path.
/// 5. `assay cover <floor> <candidate profile>`, read as exit codes and
///    nothing else ([`CoverGate::check`]).
/// 6. Journal one verdict row.
/// 7. Unload and unregister the scratch identity — on **every** path past
///    step 2, including the ones that failed.
/// 8. Release the slot with the report — on **every** path this function can
///    return through, the `Err` ones included. A slot still held by a job that
///    already returned is a daemon that refuses every later candidate until it
///    restarts, so releasing it is not conditional on the job having gone well.
///
/// Returns `Err` only when the journal or the pager itself failed (law 7);
/// every *coverage* outcome, including the infrastructure-shaped ones, is a
/// value in the report and a row (or, where no comparison happened at all, a
/// `Degraded` row).
///
/// **What the caller owes this function.** Two things, both of which belong at
/// the site that puts this on a thread rather than here:
///
/// - **The `Err` must not be dropped.** It is the only report that step 7's
///   cleanup failed, and a failed unregister means the scratch identity —
///   possibly still holding weights — outlives the job after all, which is the
///   one thing design §4 says must not happen. Nothing in the report says so:
///   the report carries the *verdict*, which is unaffected.
/// - **A panic must be caught there, not here.** Step 7 is explicit cleanup on
///   the one path that returns, not a drop guard, so an unwind past step 2
///   leaks the registration *and* leaves the slot `Running` for the life of the
///   process — every later request answered `candidate_probe_in_progress` for a
///   job nobody can see. `TaskRegistry::spawn_task` solves the identical
///   problem with `std::panic::catch_unwind` at its spawn site, and that
///   module's "Panic containment" section carries the full reasoning.
#[allow(clippy::too_many_arguments)]
pub fn run_candidate_probe<S: Substrate>(
    pager: &Mutex<Pager<S>>,
    runner: &PostRunner,
    gate: &CoverGate,
    store: &ProfileStore,
    port: u16,
    tier: &Tier,
    model: &str,
    gguf: &Path,
    slot: &SwapSlot,
) -> Result<(), PagerError> {
    let scratch = scratch_identity(model);
    let floor = store.paths(model).baseline;
    // The candidate's document is filed under the SCRATCH identity, so it can
    // never collide with the real model's current/previous/baseline documents
    // or with a confirm run's staging file — the scratch name is unique to
    // this job by construction.
    let candidate_profile = store.confirm_staging(&scratch);

    let prepared = match prepare(pager, model, gguf, &scratch, &floor, &candidate_profile) {
        Ok(prepared) => prepared,
        // The preparation itself broke — a journal that would not write, or a
        // poisoned pager. Nothing was registered on any of those paths (the
        // failure is at or before the registration), so there is nothing to
        // clean up, but the slot is released before the error propagates for
        // the same reason the judge-failed arm below releases it: a worker
        // that returned still holding the slot would leave this daemon
        // answering `candidate_probe_in_progress` for a job nobody can see,
        // for the life of the process. No digest is claimed — the failure can
        // land before either read, and `unread` is the honest placeholder for
        // a digest this report does not carry.
        Err(e) => {
            slot.finish(
                model,
                SwapOutcomeReport {
                    outcome: format!(
                        "infra: {model}'s candidate job could not be prepared: {e}; nothing was \
                         registered and nothing was probed"
                    ),
                    exit_code: None,
                    candidate_gguf_sha: UNREAD.to_string(),
                    floor_sha: UNREAD.to_string(),
                    candidate_profile_path: candidate_profile.display().to_string(),
                    notes: NOTES,
                },
            );
            return Err(e);
        }
    };
    let (candidate_gguf_sha, floor_sha) = match prepared {
        Prepared::Registered {
            candidate_gguf_sha,
            floor_sha,
        } => (candidate_gguf_sha, floor_sha),
        // Nothing was registered, so there is nothing to clean up — the whole
        // reason the two digest reads come before the registration.
        Prepared::Aborted(report) => {
            slot.finish(model, report);
            return Ok(());
        }
    };

    let mut evidence = Evidence {
        candidate_gguf_sha,
        floor,
        floor_sha,
        candidate_profile,
    };
    let judged = judge(
        pager,
        runner,
        gate,
        store,
        port,
        tier,
        model,
        &scratch,
        &mut evidence,
    );
    // Design §4's "the scratch identity never outlives the request", run
    // unconditionally: `judged` may have failed, and a failure is exactly when
    // a leaked registration would be least noticed.
    let cleaned = with_pager(pager, |p| p.unregister_model(&scratch));

    let report = match &judged {
        Ok(report) => report.clone(),
        // The verdict could not be recorded. The slot is still released: a
        // worker that returned without releasing it would leave this daemon
        // answering `candidate_probe_in_progress` for a job nobody can see,
        // for the life of the process. The evidence this job *did* gather is
        // still named — only the verdict is missing, and the outcome says so.
        Err(e) => SwapOutcomeReport {
            outcome: format!(
                "infra: the swap-candidate verdict could not be reached or recorded: {e}"
            ),
            exit_code: None,
            candidate_gguf_sha: evidence.candidate_gguf_sha.clone(),
            floor_sha: evidence.floor_sha.clone(),
            candidate_profile_path: evidence.candidate_profile.display().to_string(),
            notes: NOTES,
        },
    };
    slot.finish(model, report);
    judged.and(cleaned)
}

/// The identity of everything one job compares, gathered so the judging half
/// takes one parameter instead of four.
struct Evidence {
    candidate_gguf_sha: String,
    floor: PathBuf,
    floor_sha: String,
    /// Where the candidate's document is: the staging path the probe writes to
    /// until [`judge`] retains it, the content-named retained path afterwards.
    /// One field rather than two, so nothing downstream — the cover run, the
    /// row, the report, the fallback report on a judging failure — can name a
    /// document the job is no longer talking about.
    candidate_profile: PathBuf,
}

/// What [`prepare`] left behind: either a registered scratch identity that
/// must now be cleaned up whatever happens, or a named failure that registered
/// nothing.
enum Prepared {
    Registered {
        candidate_gguf_sha: String,
        floor_sha: String,
    },
    Aborted(SwapOutcomeReport),
}

/// Steps 1-2: the two digests, then the scratch registration.
///
/// Each failure here is journaled as `Degraded` and reported as `infra: …`
/// rather than as a verdict, because none of them measured the candidate —
/// spec §7's rule that every failure is named and none is a verdict.
fn prepare<S: Substrate>(
    pager: &Mutex<Pager<S>>,
    model: &str,
    gguf: &Path,
    scratch: &str,
    floor: &Path,
    candidate_profile: &Path,
) -> Result<Prepared, PagerError> {
    // The same full-file digest `Pager::register_model` takes of every model's
    // weights, so the row's claim and `/status`'s `digest` for this candidate
    // are the same number computed by the same function. Taken here rather
    // than read back out of the registry afterwards, which is a second pass
    // over the same file: unreadable weights then fail before anything is
    // registered, and the pass is noise beside the ~10-minute probe it gates.
    let candidate_gguf_sha = match model_digest(gguf) {
        Ok(sha) => sha,
        Err(e) => {
            return Ok(Prepared::Aborted(degraded_report(
                pager,
                UNREAD,
                UNREAD,
                candidate_profile,
                format!(
                    "the candidate weights {} offered for {model} could not be read: {e}; \
                     nothing was registered and nothing was probed",
                    gguf.display()
                ),
            )?))
        }
    };
    let floor_sha = match std::fs::read(floor) {
        Ok(bytes) => sha256_hex_bytes(&bytes),
        Err(e) => {
            return Ok(Prepared::Aborted(degraded_report(
                pager,
                &candidate_gguf_sha,
                UNREAD,
                candidate_profile,
                format!(
                    "{model}'s blessed baseline {} could not be read: {e}; there is no floor \
                     to cover, so nothing was probed",
                    floor.display()
                ),
            )?))
        }
    };
    let meta = match parse_gguf_meta(gguf) {
        Ok(meta) => meta,
        Err(e) => {
            return Ok(Prepared::Aborted(degraded_report(
                pager,
                &candidate_gguf_sha,
                &floor_sha,
                candidate_profile,
                format!(
                    "the candidate weights {} offered for {model} are not a readable GGUF: {e}; \
                     nothing was registered and nothing was probed",
                    gguf.display()
                ),
            )?))
        }
    };
    // The inner `Ok` keeps a poisoned pager propagating as `Err` while a
    // *refused registration* comes back as a value to be named, not as this
    // function's error. The refusal that can really happen to a scratch name
    // is a re-registration blocked by a resident agent — which means an
    // earlier job's cleanup failed and left this identity standing (see
    // [`run_candidate_probe`]'s note on a failed unregister).
    let registration = with_pager(pager, |p| Ok(p.register_model(scratch, gguf, meta, None)))?;
    if let Err(e) = registration {
        return Ok(Prepared::Aborted(degraded_report(
            pager,
            &candidate_gguf_sha,
            &floor_sha,
            candidate_profile,
            format!(
                "the candidate for {model} could not be registered as {scratch}: {e}; \
                 nothing was probed"
            ),
        )?));
    }
    Ok(Prepared::Registered {
        candidate_gguf_sha,
        floor_sha,
    })
}

/// Steps 3-6: probe the scratch identity, retain its document, cover the pair,
/// journal the verdict.
#[allow(clippy::too_many_arguments)]
fn judge<S: Substrate>(
    pager: &Mutex<Pager<S>>,
    runner: &PostRunner,
    gate: &CoverGate,
    store: &ProfileStore,
    port: u16,
    tier: &Tier,
    model: &str,
    scratch: &str,
    evidence: &mut Evidence,
) -> Result<SwapOutcomeReport, PagerError> {
    // POST's identical invocation (design §4 step 2: "the gate's interpreter is
    // the probe's interpreter"), against this daemon's own `/v1`.
    if let Err(e) = runner.probe(port, scratch, tier, &evidence.candidate_profile) {
        // No second document exists, so there is nothing to compare and no
        // verdict row to write — the same shape the drift watch's wedged
        // confirm takes. The row is a `Degraded` naming the model and the
        // probe's own words.
        return degraded_report(
            pager,
            &evidence.candidate_gguf_sha,
            &evidence.floor_sha,
            &evidence.candidate_profile,
            format!(
                "the candidate probe for {model} (registered as {scratch}) failed: {e}; no \
                 coverage verdict was reached — this candidate is unmeasured, not refused"
            ),
        );
    }
    // Design §4 step 3. The probe writes to ONE fixed staging path per
    // identity (POST's delete-before-probe rule owns that path), so a row
    // naming it would stop being re-runnable the moment another candidate is
    // offered for this model — the next job's probe deletes the document this
    // row claims anyone can re-cover. Retained content-named beside the drift
    // transients, under the same bound and by the same call the confirm probe
    // uses, so the row's path and its digest name a document that stays put.
    let retention = match store.retain_transient(scratch, &evidence.candidate_profile) {
        Ok(retention) => retention,
        // No comparison is run: a verdict whose evidence cannot be re-read is
        // not the evidence design §4 asks this row to be, and inventing one
        // over a document about to be overwritten is worse than naming the
        // failure. The same shape the drift watch's own retention failure
        // takes (`drift::watch`: degraded, and the reading stands unconfirmed).
        Err(e) => {
            return degraded_report(
                pager,
                &evidence.candidate_gguf_sha,
                &evidence.floor_sha,
                &evidence.candidate_profile,
                format!(
                    "{model}'s candidate document {} could not be retained: {e}; no coverage \
                     verdict was reached, because a verdict whose evidence the next job \
                     overwrites is not evidence",
                    evidence.candidate_profile.display()
                ),
            )
        }
    };
    // The document moved. Everything downstream — the cover invocation, the
    // digest, the row, the report — is of the retained path from here on.
    evidence.candidate_profile = retention.retained;
    for dropped in &retention.dropped {
        with_pager(pager, |p| {
            p.journal_degraded(format!(
                "swap: dropped {} to stay within {scratch}'s bound of {MAX_TRANSIENTS} retained \
                 candidate profiles",
                dropped.display()
            ))
        })?;
    }
    // `None` only if the retained bytes cannot be re-read; the cover run below
    // reads them itself and answers for them either way, so this is an absent
    // digest rather than a failure of its own.
    let candidate_profile_sha = std::fs::read(&evidence.candidate_profile)
        .ok()
        .map(|bytes| sha256_hex_bytes(&bytes));
    let cover = gate.check(&evidence.floor, &evidence.candidate_profile);
    let reading = CandidateReading {
        model: model.to_string(),
        candidate_gguf_sha: evidence.candidate_gguf_sha.clone(),
        floor_path: evidence.floor.clone(),
        floor_sha: evidence.floor_sha.clone(),
        candidate_profile_path: evidence.candidate_profile.clone(),
        candidate_profile_sha,
        exit_code: cover.exit_code,
        outcome: cover.outcome.journal_outcome(),
    };
    with_pager(pager, |p| p.journal_swap_candidate(&reading))?;
    Ok(reading.report())
}

/// Journals one infrastructure failure of the job and turns it into the report
/// the slot will carry.
///
/// Never a verdict row: every caller is a path where no comparison happened,
/// and a `SwapCandidate` row exists only where one did.
fn degraded_report<S: Substrate>(
    pager: &Mutex<Pager<S>>,
    candidate_gguf_sha: &str,
    floor_sha: &str,
    candidate_profile: &Path,
    detail: String,
) -> Result<SwapOutcomeReport, PagerError> {
    with_pager(pager, |p| p.journal_degraded(format!("swap: {detail}")))?;
    Ok(SwapOutcomeReport {
        outcome: format!("infra: {detail}"),
        exit_code: None,
        candidate_gguf_sha: candidate_gguf_sha.to_string(),
        floor_sha: floor_sha.to_string(),
        candidate_profile_path: candidate_profile.display().to_string(),
        notes: NOTES,
    })
}
