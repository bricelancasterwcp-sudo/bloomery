//! POST — power-on self test: `assay` probing this daemon's own `/v1`
//! surface at boot, so law 5 (admission by *measured* verdict) has something
//! measured to admit against.
//!
//! **The chicken-and-egg.** assay measures a serving state by driving it:
//! it needs the daemon up, answering `/v1/chat/completions`, before any
//! profile can exist. But admission is supposed to refuse an unprofiled
//! model. The resolution is a *stated*, bounded suspension: the daemon marks
//! itself [`posting`](crate::pager::Pager::set_posting) before the server
//! starts, so unprofiled models are admitted while POST runs, and the flag
//! drops the moment POST finishes — after which normal admission applies.
//! Every provisional admission and every POST outcome is journaled, so a
//! replay shows exactly which calls ran inside that window.
//!
//! **The profile is read from the file, never from stdout.** assay writes
//! its result document to the `--json` path; stdout is a human-readable
//! slice of it. Parsing stdout would make this daemon's admission depend on
//! assay's *display* format rather than its documented artifact.
//!
//! **A failed probe is an infrastructure failure with a name** — `Spawn`
//! (assay could not be started), `NonZeroExit` (it ran and failed, with its
//! own stderr), or `BadProfile` (it exited 0 but its document is missing,
//! unparseable, or measured a different model). None of them is a
//! profile-less success: the model simply stays unprofiled, which is a
//! refusal under law 5, not a silent admission.

use std::path::Path;
use std::process::Output;
use std::sync::Mutex;

use bloomery_core::profile::Profile;
use bloomery_substrate::Substrate;

use crate::config::Tier;
use crate::pager::{Pager, PagerError};

/// How a [`PostRunner`] actually runs a program: `(program, args) ->
/// output`. The real one shells out via [`std::process::Command`]; tests
/// inject a closure instead, which is what keeps the exact invocation and
/// every failure class testable with no python, no assay, and no GPU.
///
/// Deliberately not `Send`: a `PostRunner` is built *inside* the thread that
/// runs POST (see [`run_post`]'s callers), never shipped across one, so the
/// pinned constructor signature stays as the brief wrote it.
pub type CommandRunner = Box<dyn Fn(&str, &[String]) -> std::io::Result<Output>>;

/// Everything that can go wrong running the POST probe. Each variant is a
/// distinct operator action: install/point at python, read assay's stderr,
/// or look at the document it produced.
#[derive(Debug)]
pub enum PostError {
    /// The process could not be started at all (no python, bad path).
    Spawn(String),
    /// assay ran and exited non-zero; `stderr` is its own words, verbatim.
    NonZeroExit { code: i32, stderr: String },
    /// assay exited 0 but its `--json` document is missing, unreadable,
    /// unparseable, or describes a different model.
    BadProfile(String),
}

impl std::fmt::Display for PostError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PostError::Spawn(msg) => write!(f, "could not run assay: {msg}"),
            PostError::NonZeroExit { code, stderr } => {
                write!(f, "assay exited {code}: {stderr}")
            }
            PostError::BadProfile(msg) => write!(f, "unusable profile document: {msg}"),
        }
    }
}

impl std::error::Error for PostError {}

/// Runs `assay probe` against a serving endpoint and hands back the profile
/// it wrote.
pub struct PostRunner {
    python: String,
    run: CommandRunner,
}

impl PostRunner {
    /// A runner that really spawns `{python} -m assay ...`.
    pub fn new(python: String) -> PostRunner {
        PostRunner {
            python,
            run: Box::new(|program: &str, args: &[String]| {
                std::process::Command::new(program).args(args).output()
            }),
        }
    }

    /// A runner with the command execution injected — the whole POST surface
    /// tested without python, assay, or a GPU.
    #[cfg(any(test, feature = "test-support"))]
    pub fn with_runner(f: CommandRunner) -> PostRunner {
        PostRunner {
            // The same spelling `config.assay.python` defaults to, imported
            // here rather than retyped so the two cannot drift.
            python: crate::config::default_python(),
            run: f,
        }
    }

    /// Probes `model` on this daemon's own `/v1` surface, writing the
    /// profile document to `out` and parsing it back.
    ///
    /// Runs exactly:
    ///
    /// ```text
    /// {python} -m assay probe http://127.0.0.1:{port}/v1 --model {model} --quick \
    ///     --backend openai --json {out} --tier {tier.name} {--real-hardware | --emulated}
    /// ```
    ///
    /// **The `/v1` suffix is assay's contract, not decoration.** Its
    /// OpenAI-compatible backend keeps the base URL verbatim and appends
    /// `/chat/completions` to it (`backends/__init__.py`: "OpenAICompat
    /// keeps whatever the user gave"). Handing it a bare
    /// `http://127.0.0.1:{port}` makes it call `/chat/completions`, which
    /// this daemon answers `404` — measured on the first live boot smoke,
    /// which journaled `Post{outcome: "failed: assay exited 4: … HTTP 404
    /// from http://127.0.0.1:8401/chat/completions"}`.
    ///
    /// `--quick` because POST runs on every boot and must not cost minutes;
    /// `--backend openai` because this daemon's `/v1` shim *is* the endpoint
    /// (never autodetect against ourselves); the tier mark is mandatory on
    /// assay's side precisely so an emulated number can never masquerade as
    /// real hardware, so it is carried from config rather than assumed.
    pub fn probe(
        &self,
        port: u16,
        model: &str,
        tier: &Tier,
        out: &Path,
    ) -> Result<Profile, PostError> {
        let args = argv(port, model, tier, out);
        let output = (self.run)(&self.python, &args)
            .map_err(|e| PostError::Spawn(format!("{} {}: {e}", self.python, args.join(" "))))?;
        if !output.status.success() {
            return Err(PostError::NonZeroExit {
                // `None` means killed by a signal; -1 is not a real exit
                // code and reads as "no code", which is the truth.
                code: output.status.code().unwrap_or(-1),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            });
        }
        let text = std::fs::read_to_string(out).map_err(|e| {
            PostError::BadProfile(format!(
                "assay exited 0 but {} could not be read: {e}",
                out.display()
            ))
        })?;
        let profile = Profile::from_json(&text)
            .map_err(|e| PostError::BadProfile(format!("{}: {e}", out.display())))?;
        // assay copies `--model` into the document verbatim, so a mismatch
        // means the wrong document was read (a stale file, a crossed path) —
        // attaching it would credit one model with another's measurements.
        if profile.model_name() != model {
            return Err(PostError::BadProfile(format!(
                "{} describes model {}, but {model} was probed",
                out.display(),
                profile.model_name()
            )));
        }
        Ok(profile)
    }
}

/// The documented invocation, in one place. Split out so the argument list
/// is a value a test can inspect rather than a side effect of spawning.
fn argv(port: u16, model: &str, tier: &Tier, out: &Path) -> Vec<String> {
    let marking = if tier.emulated {
        "--emulated"
    } else {
        "--real-hardware"
    };
    [
        "-m",
        "assay",
        "probe",
        &format!("http://127.0.0.1:{port}/v1"),
        "--model",
        model,
        "--quick",
        "--backend",
        "openai",
        "--json",
        &out.display().to_string(),
        "--tier",
        &tier.name,
        marking,
    ]
    .iter()
    .map(|s| (*s).to_string())
    .collect()
}

/// Runs POST for every model, then closes the provisional-admission window.
///
/// One model's failure never stops the others: a daemon with two models
/// where assay only manages to profile one boots **degraded for that one**
/// and fully admitted for the other, which is more useful — and more
/// honest — than an all-or-nothing boot.
///
/// Journal writes are the exception: an unwritable journal is a broken
/// daemon (law 7), so it aborts the remaining probes and surfaces. The
/// `posting` flag is still cleared on that path — leaving it set would
/// suspend law 5 for the life of the process, which is precisely the
/// silent-admission failure this module exists to prevent.
///
/// The one case where the flag cannot be cleared is a *poisoned* pager
/// mutex, because clearing it requires the same lock. That daemon is
/// already answering every request with a named `500` (see
/// `api_native::lock_pager`), so there is no admission left to gate.
///
/// Takes `&Mutex<Pager<S>>` rather than a locked pager because assay is
/// simultaneously driving this daemon's `/v1` surface through the same
/// mutex: POST holds the lock only long enough to record each outcome.
pub fn run_post<S: Substrate>(
    pager: &Mutex<Pager<S>>,
    runner: &PostRunner,
    models: &[String],
    port: u16,
    tier: &Tier,
    profiles_dir: &Path,
) -> Result<(), PagerError> {
    let outcome = probe_each(pager, runner, models, port, tier, profiles_dir);
    let cleared = with_pager(pager, |p| {
        p.set_posting(false);
        Ok(())
    });
    outcome.and(cleared)
}

fn probe_each<S: Substrate>(
    pager: &Mutex<Pager<S>>,
    runner: &PostRunner,
    models: &[String],
    port: u16,
    tier: &Tier,
    profiles_dir: &Path,
) -> Result<(), PagerError> {
    for model in models {
        let out = profiles_dir.join(format!("{model}.json"));
        match runner.probe(port, model, tier, &out) {
            Ok(profile) => with_pager(pager, |p| {
                p.attach_profile(model, profile)?;
                p.journal_post(model, "ok", Some(out.display().to_string()))
            })?,
            Err(e) => with_pager(pager, |p| {
                p.journal_post(model, &format!("failed: {e}"), None)?;
                p.journal_degraded(format!(
                    "POST failed for {model}: {e}; it stays unprofiled and is refused \
                     unless allow_unprofiled is set"
                ))
            })?,
        }
    }
    Ok(())
}

/// Locks the pager for one short critical section, turning a poisoned mutex
/// into a named error rather than a panic inside the boot worker.
fn with_pager<S: Substrate, T>(
    pager: &Mutex<Pager<S>>,
    f: impl FnOnce(&mut Pager<S>) -> Result<T, PagerError>,
) -> Result<T, PagerError> {
    let mut guard = pager.lock().map_err(|_| {
        PagerError::Substrate(
            "pager state poisoned by a prior panic; POST cannot record its result".to_string(),
        )
    })?;
    f(&mut guard)
}
