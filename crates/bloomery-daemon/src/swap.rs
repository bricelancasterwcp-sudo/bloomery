//! The swap-candidate seam (spec: docs/superpowers/specs/
//! 2026-08-19-swap-candidate-seam-design.md §4): a coverage verdict on
//! a candidate model, evidenced by a daemon-run probe and
//! `assay cover`, consumed — like the drift gate — strictly through
//! documented exit codes. Advisory: nothing here blocks admission.

use std::path::Path;
use std::time::Duration;

use crate::drift::{with_stderr, DIFF_TIMEOUT_SECS};
use crate::post::CommandRunner;

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
