//! The swap-candidate seam (spec: docs/superpowers/specs/
//! 2026-08-19-swap-candidate-seam-design.md §4): a coverage verdict on
//! a candidate model, evidenced by a daemon-run probe and
//! `assay cover`, consumed — like the drift gate — strictly through
//! documented exit codes. Advisory: nothing here blocks admission.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use bloomery_core::gguf::parse_gguf_meta;
use bloomery_core::journal::{sha256_hex_bytes, Event};
use bloomery_substrate::Substrate;

use crate::agents::model_digest;
use crate::config::Tier;
use crate::drift::{with_stderr, ProfileStore, DIFF_TIMEOUT_SECS};
use crate::pager::{Pager, PagerError};
use crate::post::{with_pager, CommandRunner, PostRunner};

/// The cap one `assay cover` runs under.
///
/// Derived from the drift gate's [`DIFF_TIMEOUT_SECS`] rather than respelled:
/// both bound one short assay subprocess that reads two profile documents and
/// exits, so a cover allowed to outlive a diff would be an unexplained
/// difference between two runs of the same tool under the same interpreter.
const COVER_TIMEOUT: Duration = Duration::from_secs(DIFF_TIMEOUT_SECS);

/// `{python} -m assay cover {floor} {candidate}`
///
/// A value tests inspect rather than a side effect of spawning — the
/// same treatment `drift::diff_argv` and `post::argv` get. No flag:
/// cover IS a gate; exit codes are its whole interface.
pub fn cover_argv(floor: &Path, candidate: &Path) -> Vec<String> {
    [
        "-m",
        "assay",
        "cover",
        &floor.display().to_string(),
        &candidate.display().to_string(),
    ]
    .iter()
    .map(|s| (*s).to_string())
    .collect()
}

/// What one cover run said. assay ≥ 0.13.0 documents exactly 0, 1, 2 and 3
/// for `cover` (the seam spec §3's contract); any other code is a tool this
/// daemon does not understand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoverOutcome {
    /// Exit 0: every cell the floor measured, the candidate provides
    /// at least as well.
    Covered,
    /// Exit 1: at least one floor cell ranks below, beyond noise.
    NotCovered,
    /// Exit 2: cover refused the pair (hardware class or instrument
    /// mismatch). Never a pass.
    ///
    /// `stderr` is assay's own words, trimmed, carried for the operator the
    /// way [`crate::drift::with_stderr`] carries them into an `Infra` detail:
    /// **operator detail, NEVER consulted for the verdict** — the verdict is
    /// the exit code and nothing else. Empty is fine and expected on a
    /// genuine refusal.
    ///
    /// It rides along because exit 2 is also what `argparse` returns for
    /// `invalid choice: 'cover'` — an assay too old to have the subcommand
    /// (anything < 0.13.0 under the PYTHONPATH pin) refuses in a way that is
    /// indistinguishable from a real refusal by code alone. Discarding the
    /// one sentence that says "this tool has no cover" would let a stale
    /// install masquerade as a considered verdict about the candidate.
    Refused { exit: i32, stderr: String },
    /// Exit 3: a floor cell the candidate did not measure. Never a
    /// pass — the unmeasured cell may hide the regression the check
    /// exists to catch.
    Incomplete,
    /// The tool could not answer: spawn failure, signal, undocumented
    /// exit. Not a verdict in either direction.
    Infra { detail: String },
}

impl CoverOutcome {
    /// The `outcome` string this verdict takes in [`Event::SwapCandidate`]
    /// and in [`SwapOutcomeReport::outcome`] — one spelling, so the row and
    /// the operator's answer can never disagree about what happened.
    ///
    /// The same shape [`crate::drift::GateOutcome::journal_outcome`] has: the
    /// variants that carry context fold it in after a colon rather than
    /// getting their own fields, and all of it is identity or prose — never a
    /// number transcribed out of a profile.
    ///
    /// **`Refused` folds assay's words in for a reason** (the Task-1 review's
    /// ruling): exit 2 is also what `argparse` answers for `invalid choice:
    /// 'cover'`, so a bare `"refused"` would spell a stale assay install and a
    /// considered refusal about the candidate identically, and an operator
    /// would have to re-run the command to find out which they got. The
    /// stderr is still never consulted for the *verdict* — it is carried, not
    /// read.
    ///
    /// **`Incomplete` deliberately carries nothing extra.** Exit 3 has exactly
    /// one meaning and no ambiguity to resolve, and *which* floor cells the
    /// candidate left unmeasured is a measurement that lives in assay's own
    /// render of the pair. The row already names both documents, both digests
    /// and the exit code, so `assay cover <floor> <candidate>` re-runs the
    /// identical comparison and answers that question from the tool rather
    /// than from a transcription that could drift from it (design §4: identity
    /// and prose, never transcribed measurements).
    pub fn journal_outcome(&self) -> String {
        match self {
            CoverOutcome::Covered => "covered".to_string(),
            CoverOutcome::NotCovered => "not-covered".to_string(),
            CoverOutcome::Refused { stderr, .. } if stderr.is_empty() => "refused".to_string(),
            CoverOutcome::Refused { stderr, .. } => format!("refused: {stderr}"),
            CoverOutcome::Incomplete => "incomplete".to_string(),
            CoverOutcome::Infra { detail } => format!("infra: {detail}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoverReading {
    pub outcome: CoverOutcome,
    pub exit_code: Option<i32>,
}

/// Runs design §4 step 4: `assay cover` over a blessed floor and a freshly
/// probed candidate.
///
/// Mirrors [`crate::drift::DriftGate`]'s shape — an injected
/// [`CommandRunner`], an inspectable argv, a bounded spawn — because it solves
/// the same problem: this daemon's verdicts must be testable without python,
/// without assay and without a GPU, and the exact invocation must be a value
/// rather than a side effect.
///
/// Unlike the drift gate, this one has no pre-spawn refusals to run. The drift
/// gate reads both documents first because a *crossed* pair — two different
/// models — is the failure it must catch before diffing. Coverage inverts that
/// rule on purpose (spec §3: a differing `model.name`, quant and
/// `weights_bytes` is the whole point of the command), and every remaining
/// refusal — hardware class, instrument, an unmeasured floor cell — is assay's
/// own to make, reported through exits 2 and 3. Duplicating any of it here
/// would be this daemon second-guessing the tool it is asking.
pub struct CoverGate {
    python: String,
    run: CommandRunner,
    timeout: Duration,
}

impl CoverGate {
    /// A gate that really spawns `{python} -m assay cover ...`, bounded by
    /// [`COVER_TIMEOUT`].
    ///
    /// `python` comes from `config.assay.python`, the same interpreter POST
    /// probes with and the drift gate diffs with — spec §4's "the gate's
    /// interpreter is the probe's interpreter". A cover run under a different
    /// assay install than the probe would be judging documents against rules
    /// the tool that wrote them never applied.
    pub fn new(python: String) -> CoverGate {
        CoverGate {
            python,
            run: Box::new(|program: &str, args: &[String]| {
                crate::post::run_bounded(program, args, COVER_TIMEOUT)
            }),
            timeout: COVER_TIMEOUT,
        }
    }

    /// A gate with the command execution injected — every outcome testable
    /// with no assay installed.
    #[cfg(any(test, feature = "test-support"))]
    pub fn with_runner(f: CommandRunner) -> CoverGate {
        CoverGate {
            // The same spelling `config.assay.python` defaults to, imported
            // rather than retyped so the two cannot drift.
            python: crate::config::default_python(),
            run: f,
            timeout: COVER_TIMEOUT,
        }
    }

    /// The cap this gate's subprocess runs under.
    pub fn timeout(&self) -> Duration {
        self.timeout
    }

    /// Asks whether `candidate` covers `floor`, and reports only what the
    /// exit code said.
    ///
    /// assay's prose is never parsed for a verdict — it rides along in
    /// [`CoverOutcome::Infra`] details and [`CoverOutcome::Refused`]'s
    /// `stderr` for the operator, and nowhere else. The four codes assay
    /// ≥ 0.13.0 documents each get a name, and everything else is
    /// infrastructure: a code this daemon does not understand cannot be
    /// resolved into "covered" or "not covered" without inventing an answer,
    /// and the safe-looking guess (treat it as a failure) is just as much an
    /// invention as the dangerous one.
    pub fn check(&self, floor: &Path, candidate: &Path) -> CoverReading {
        let args = cover_argv(floor, candidate);
        let output = match (self.run)(&self.python, &args) {
            Ok(output) => output,
            Err(e) => {
                return CoverReading {
                    outcome: CoverOutcome::Infra {
                        detail: format!("could not run {} {}: {e}", self.python, args.join(" ")),
                    },
                    exit_code: None,
                }
            }
        };
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let reading =
            |outcome: CoverOutcome, exit_code: Option<i32>| CoverReading { outcome, exit_code };
        match output.status.code() {
            Some(0) => reading(CoverOutcome::Covered, Some(0)),
            Some(1) => reading(CoverOutcome::NotCovered, Some(1)),
            Some(2) => reading(
                CoverOutcome::Refused {
                    exit: 2,
                    // The same trimmed stderr the Infra arms append: on a
                    // genuine refusal it is assay's reason, and on a stale
                    // assay it is argparse saying `cover` does not exist.
                    stderr: stderr.clone(),
                },
                Some(2),
            ),
            Some(3) => reading(CoverOutcome::Incomplete, Some(3)),
            Some(n) => reading(
                CoverOutcome::Infra {
                    detail: with_stderr(
                        format!(
                            "undocumented exit {n} from `assay cover` \
                             (0, 1, 2 and 3 are the documented codes)"
                        ),
                        &stderr,
                    ),
                },
                Some(n),
            ),
            // No code at all: the child was killed by a signal. `-1` would
            // look like a code; `None` is what happened.
            None => reading(
                CoverOutcome::Infra {
                    detail: with_stderr(
                        "`assay cover` was killed by a signal, leaving no exit code".to_string(),
                        &stderr,
                    ),
                },
                None,
            ),
        }
    }
}

// ---------------------------------------------------------------------------
// The job — design §4's flow, from scratch registration to journaled verdict
// ---------------------------------------------------------------------------

/// The suffix that turns a configured model name into the scratch identity its
/// candidate is probed under: `{model}!swap-candidate`.
///
/// **Why `!`, and the assumption it rests on.** The scratch entry lives in the
/// same registry as the operator's configured models for the length of one job
/// (design §4 step 1: it must be addressable through `/v1` for assay to probe
/// it at all), so its name has to be one no configured model can hold. Model
/// names are TOML table keys the operator writes: a *bare* key is
/// `[A-Za-z0-9_-]` only, and `!` is outside it — so no bare key can ever
/// collide. A *quoted* key may contain anything, `!` included, so an operator
/// who deliberately writes `"llama!swap-candidate"` as a model name would
/// collide with the scratch identity of a model called `llama`. That is named
/// here rather than guarded against: a guard would trade a real line of code
/// for a configuration nobody writes, and the collision is visible the moment
/// it happens (`/status` lists both names).
pub const SCRATCH_SUFFIX: &str = "!swap-candidate";

/// The identity a candidate is registered and probed under — see
/// [`SCRATCH_SUFFIX`] for the collision argument. Never journaled as the
/// subject of a verdict: the row names the configured model whose role the
/// candidate would take.
pub fn scratch_identity(model: &str) -> String {
    format!("{model}{SCRATCH_SUFFIX}")
}

/// The first of the two gaps every report names (design §4's response
/// contract, §6's first non-goal).
pub const NOTE_TASK_GATES: &str = "done_trust/G4/G5 are unmeasured for this candidate until its \
                                   first real boot with tasks enabled";

/// The second: design §5's handover, spelled out so the not-comparable boot
/// after a swap is expected rather than alarming.
pub const NOTE_HANDOVER: &str = "on swap: edit config, restart; the next boot reads \
                                 not-comparable against the old lineage's baseline until you \
                                 POST /models/{name}/bless";

/// The two notes, in the order §4 names them. Fixed: every report carries both,
/// whatever the verdict, because both gaps are true of every candidate.
const NOTES: [&str; 2] = [NOTE_TASK_GATES, NOTE_HANDOVER];

/// What a digest field carries in a *report* when the bytes it would describe
/// could not be read at all.
///
/// The same named placeholder `drift::watch`'s `reference_identity` uses, and
/// for the same reason: an empty string reads as "no digest was needed" and a
/// zero digest reads as a real one. It only ever appears beside an
/// `"infra: …"` outcome whose sentence names exactly what could not be read —
/// and never in a journal row, because those paths journal
/// [`Event::Degraded`] instead of a verdict row.
const UNREAD: &str = "unread";

/// Everything one candidate job learned — the single value both the journal
/// row and the operator's report are built from, so the two cannot come to
/// describe different documents.
///
/// The swap-candidate counterpart of [`crate::drift::GateReading`], and it
/// carries paths and digests for the same reason that one does: re-deriving
/// them on the way to the journal would make the row's digest describe a
/// *second* read, so a row could name bytes the comparison never consumed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateReading {
    /// The configured model whose role the candidate would take.
    pub model: String,
    /// Full-file sha256 of the candidate's weights.
    pub candidate_gguf_sha: String,
    /// The blessed baseline the candidate was measured against.
    pub floor_path: PathBuf,
    /// sha256 of the floor document's **bytes**.
    pub floor_sha: String,
    /// The document this job's probe wrote for the candidate.
    pub candidate_profile_path: PathBuf,
    /// sha256 of that document's **bytes**, or `None` when they could not be
    /// re-read after the probe wrote them.
    pub candidate_profile_sha: Option<String>,
    /// What `assay cover` exited with, `None` when it never answered.
    pub exit_code: Option<i32>,
    /// The named verdict — [`CoverOutcome::journal_outcome`]'s spelling.
    pub outcome: String,
}

impl CandidateReading {
    /// The operator's answer, built from the same reading the row is
    /// (design §4's response: the verdict, the evidence, and the two named
    /// gaps).
    pub fn report(&self) -> SwapOutcomeReport {
        SwapOutcomeReport {
            outcome: self.outcome.clone(),
            exit_code: self.exit_code,
            candidate_gguf_sha: self.candidate_gguf_sha.clone(),
            floor_sha: self.floor_sha.clone(),
            candidate_profile_path: self.candidate_profile_path.display().to_string(),
            notes: NOTES,
        }
    }
}

/// The journal row for one candidate job ([`Event::SwapCandidate`]).
///
/// Every field comes from the `reading` itself, so the row cannot describe a
/// different pair of documents than the cover run actually compared — the same
/// discipline `drift::drift_event` follows, for the same reason.
pub fn swap_candidate_event(reading: &CandidateReading) -> Event {
    Event::SwapCandidate {
        model: reading.model.clone(),
        candidate_gguf_sha: reading.candidate_gguf_sha.clone(),
        floor_path: reading.floor_path.display().to_string(),
        floor_sha: reading.floor_sha.clone(),
        candidate_profile_path: reading.candidate_profile_path.display().to_string(),
        candidate_profile_sha: reading.candidate_profile_sha.clone(),
        exit_code: reading.exit_code,
        outcome: reading.outcome.clone(),
    }
}

/// What one swap-candidate job tells the operator (design §4's response).
///
/// Advisory in the strongest sense: nothing in this daemon reads it back, and
/// no admission decision derives from it. It exists so an operator can decide,
/// and so the two things a verdict does NOT say are said out loud beside it —
/// see [`SwapOutcomeReport::notes`].
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct SwapOutcomeReport {
    /// The named verdict — [`CoverOutcome::journal_outcome`]'s spelling, the
    /// same string the journal row carries. Read by prefix: `"refused"` and
    /// `"infra"` both carry the failing layer's own words after a colon.
    pub outcome: String,
    /// What `assay cover` exited with, `None` when it never answered (no cover
    /// ran, the spawn failed, or the child was killed by a signal).
    pub exit_code: Option<i32>,
    /// Full-file sha256 of the candidate's weights, or the fixed word
    /// `"unread"` when those bytes could not be read at all — which is always
    /// an `"infra: …"` outcome naming what failed, never a verdict.
    pub candidate_gguf_sha: String,
    /// sha256 of the floor document's bytes, same `"unread"` rule.
    pub floor_sha: String,
    /// Where the candidate's profile document was written — or, on an
    /// `"infra: …"` outcome from a failed probe, where it *would* have been
    /// written. The outcome word is what says whether it exists; the path is
    /// carried either way because "which document" is the first thing an
    /// operator chasing a failed probe looks for.
    pub candidate_profile_path: String,
    /// The two gaps design §4 requires every answer to name: the task gates
    /// this probe did not measure, and the handover a swap actually needs.
    /// Fixed strings, always both, in this order.
    pub notes: [&'static str; 2],
}

/// The one candidate probe this daemon runs at a time (design §4: "A probe
/// holds VRAM for ~10 minutes. A second request while one runs gets 409
/// `candidate_probe_in_progress` — no queue").
///
/// A claim is made by the request thread ([`SwapSlot::try_start`]) *before* the
/// worker is spawned, so the refusal is answered synchronously and two workers
/// can never both be registering a scratch identity. The worker releases it
/// exactly once, by [`SwapSlot::finish`], on every path it can *return*
/// through — an unwind is not one of them, which is why the spawn site owes
/// this job a `catch_unwind` (see [`run_candidate_probe`]).
#[derive(Debug, Default)]
pub struct SwapSlot {
    state: Mutex<SwapState>,
}

/// What the slot holds. `Done` is kept rather than cleared back to `Idle`
/// because it is the only place a finished job's report lives — the GET side
/// of the endpoint reads it, and a slot that forgot its last answer would make
/// a completed 10-minute probe unreadable.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum SwapState {
    /// No job has run in this process yet.
    #[default]
    Idle,
    /// A job is running for `model`, against these weights.
    Running { model: String, gguf: PathBuf },
    /// The last job's answer, kept until another job replaces it.
    Done {
        model: String,
        report: SwapOutcomeReport,
    },
}

/// A refused claim on the slot, naming the job that holds it — so the 409 can
/// say what is running rather than only that something is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Busy {
    pub model: String,
    pub gguf: PathBuf,
}

impl SwapSlot {
    /// Claims the slot for one job, or refuses with what is already running.
    ///
    /// A finished job does not hold the slot: `Done` admits the next claim
    /// (the bound is one at a time, not one ever) and its report is replaced
    /// by the new job's when that job finishes.
    pub fn try_start(&self, model: &str, gguf: &Path) -> Result<(), Busy> {
        let mut state = self.lock();
        if let SwapState::Running { model, gguf } = &*state {
            return Err(Busy {
                model: model.clone(),
                gguf: gguf.clone(),
            });
        }
        *state = SwapState::Running {
            model: model.to_string(),
            gguf: gguf.to_path_buf(),
        };
        Ok(())
    }

    /// Releases the slot, leaving this job's answer in it.
    pub fn finish(&self, model: &str, report: SwapOutcomeReport) {
        *self.lock() = SwapState::Done {
            model: model.to_string(),
            report,
        };
    }

    /// The slot's current state, cloned — no lock is held across a caller's
    /// rendering of it.
    pub fn snapshot(&self) -> SwapState {
        self.lock().clone()
    }

    /// **Poison is recovered here, unlike `api_native::lock_pager`'s sticky
    /// refusal**, and the difference is what the two locks protect. The pager
    /// guards a substrate, an agent table and an image store whose invariants
    /// a panic mid-mutation can genuinely break. This guards one enum, and the
    /// only operations under this lock are a clone and a whole-value
    /// assignment — neither can leave a half-built [`SwapState`] behind, so a
    /// poisoned lock still holds an intact value. Refusing it forever would
    /// strand the slot in whatever state it happened to be in, which for
    /// `Running` means no candidate can ever be probed again without a
    /// restart.
    fn lock(&self) -> MutexGuard<'_, SwapState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

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
/// 4. `assay cover <floor> <candidate profile>`, read as exit codes and
///    nothing else ([`CoverGate::check`]).
/// 5. Journal one verdict row.
/// 6. Unload and unregister the scratch identity — on **every** path past
///    step 2, including the ones that failed.
/// 7. Release the slot with the report.
///
/// Returns `Err` only when the journal or the pager itself failed (law 7);
/// every *coverage* outcome, including the infrastructure-shaped ones, is a
/// value in the report and a row (or, where no comparison happened at all, a
/// `Degraded` row).
///
/// **What the caller owes this function.** Two things, both of which belong at
/// the site that puts this on a thread rather than here:
///
/// - **The `Err` must not be dropped.** It is the only report that step 6's
///   cleanup failed, and a failed unregister means the scratch identity —
///   possibly still holding weights — outlives the job after all, which is the
///   one thing design §4 says must not happen. Nothing in the report says so:
///   the report carries the *verdict*, which is unaffected.
/// - **A panic must be caught there, not here.** Step 6 is explicit cleanup on
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

    let prepared = prepare(pager, model, gguf, &scratch, &floor, &candidate_profile)?;
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

    let evidence = Evidence {
        candidate_gguf_sha,
        floor,
        floor_sha,
        candidate_profile,
    };
    let judged = judge(pager, runner, gate, port, tier, model, &scratch, &evidence);
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
            outcome: format!("infra: the swap-candidate verdict could not be recorded: {e}"),
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

/// Steps 3-5: probe the scratch identity, cover the pair, journal the verdict.
#[allow(clippy::too_many_arguments)]
fn judge<S: Substrate>(
    pager: &Mutex<Pager<S>>,
    runner: &PostRunner,
    gate: &CoverGate,
    port: u16,
    tier: &Tier,
    model: &str,
    scratch: &str,
    evidence: &Evidence,
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
    // `None` only if the bytes the probe just wrote cannot be re-read; the
    // cover run below reads them itself and answers for them either way, so
    // this is an absent digest rather than a failure of its own.
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
