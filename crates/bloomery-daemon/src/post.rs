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
//! The window is *bracketed* in the journal, not enumerated: the
//! provisional admission is journaled once per model (`Degraded`), each
//! probe's outcome is journaled (`Post`), and the `AgentCreated` rows
//! between them are the agents admitted inside it. A replay can therefore
//! bound the window and see what was admitted during it — it cannot tell
//! which of those calls were assay's and which were a client's.
//!
//! **The profile is read from the file, never from stdout.** assay writes
//! its result document to the `--json` path; stdout is a human-readable
//! slice of it. Parsing stdout would make this daemon's admission depend on
//! assay's *display* format rather than its documented artifact. The path
//! is deleted before the probe runs, so a document left by an earlier boot
//! can never be read back as this one's measurement.
//!
//! **The subprocess is bounded.** A wedged assay would otherwise hold the
//! provisional-admission window open for the life of the process — the one
//! failure this module exists to prevent. The cap is operator-configurable
//! (`config::AssayConfig::probe_timeout_secs`, plumbed through
//! [`PostRunner::new`]'s `probe_timeout` parameter) precisely because slow,
//! partially-offloaded models can need much longer than the 600 s default
//! sized for a quick probe — see that parameter's doc for the measured
//! baseline and the measured motivation for raising it.
//!
//! **A failed probe is an infrastructure failure with a name** — `Spawn`
//! (assay could not be started), `NonZeroExit` (it ran and failed, with its
//! own stderr), or `BadProfile` (it exited 0 but its document is missing,
//! unparseable, or measured a different model). None of them is a
//! profile-less success: the model simply stays unprofiled, which is a
//! refusal under law 5, not a silent admission.

use std::io::Read;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use bloomery_core::profile::Profile;
use bloomery_substrate::Substrate;

use crate::config::Tier;
use crate::pager::{Pager, PagerError};

/// How often the spawn layer checks whether the child has exited. Cheap
/// (`waitpid(WNOHANG)`), and 500 ms is far below any interval that matters
/// against a ~110 s probe.
const PROBE_POLL_INTERVAL: Duration = Duration::from_millis(500);

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
    /// A runner that really spawns `{python} -m assay ...`, bounded by
    /// `probe_timeout`.
    ///
    /// **Why this needs a cap, and where the shipped default comes from:**
    /// a `--quick` probe measured ~110 s per model on the enthusiast-16GB
    /// tier (2026-08-14, qwen2.5-coder:7b-q8_0 on an RTX 5080). The
    /// operator-configured default (`config::AssayConfig::probe_timeout_secs`,
    /// 600 s when unset) is ~5× that: slow enough never to kill a working
    /// probe, short enough that a wedged one cannot hold the
    /// provisional-admission window open for the life of the daemon. On
    /// expiry the child is **killed**, and the timeout takes the same
    /// named-failure path as any other probe failure — the model stays
    /// unprofiled and `posting` still clears.
    ///
    /// **Configurable because slow, partially-offloaded models blow past
    /// the quick-probe baseline the default was sized for:** a measured
    /// qwen3.8-27b Q3 at ~15.5 tok/s (~3.4× slower than the baseline model
    /// above) projects a `--quick` probe at ~25-30 min, which the 600 s
    /// default would kill outright — the model stays unprofiled and the G4
    /// codec probe, gated strictly after POST succeeds, never runs either.
    /// An operator serving such a model raises `assay.probe_timeout_secs`
    /// in config; every existing config keeps the 600 s default
    /// byte-for-byte.
    pub fn new(python: String, probe_timeout: Duration) -> PostRunner {
        PostRunner {
            python,
            run: Box::new(move |program: &str, args: &[String]| {
                run_bounded(program, args, probe_timeout)
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
        // Delete any document left by an earlier boot *before* running the
        // probe. Without this, an assay that exits 0 having written nothing
        // would have yesterday's measurements read back and attached as
        // today's — a stale profile is indistinguishable from a fresh one
        // once it is in the pager. Now that case lands in `BadProfile`
        // (the file is simply not there), which is a named failure.
        // A failure to remove is not swallowed silently: it either means
        // the file was already absent (the common case, nothing to say) or
        // that the path is unwritable, which assay is about to fail on
        // anyway with its own error.
        let _ = std::fs::remove_file(out);
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

/// Runs `program args` and waits **at most** `timeout` for it, killing the
/// child if it overstays.
///
/// This is `Command::output()` with a deadline. `output()` itself blocks
/// forever, which for POST means a hung assay pins the daemon in
/// provisional admission until someone restarts it — see
/// [`PostRunner::new`]'s `probe_timeout` parameter for the operator-configured
/// cap that bounds it.
///
/// Three deliberate choices in the plumbing:
///
/// - **stdin is `/dev/null`.** A child that reads stdin gets EOF instead of
///   blocking on a terminal that isn't there.
/// - **stdout is discarded.** Nothing here ever parses it (the profile
///   comes from the `--json` file, by design), and not piping it removes
///   the only unbounded pipe a long-running probe could fill.
/// - **stderr is piped** because `NonZeroExit` reports assay's own words.
///   It is drained *after* exit rather than concurrently, so a pathological
///   child writing >64 KiB of stderr would block on a full pipe — bounded,
///   not unbounded: the timeout fires and kills it, which is exactly the
///   named failure path.
fn run_bounded(program: &str, args: &[String], timeout: Duration) -> std::io::Result<Output> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?;

    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            let mut stderr = Vec::new();
            if let Some(mut pipe) = child.stderr.take() {
                pipe.read_to_end(&mut stderr)?;
            }
            return Ok(Output {
                status,
                stdout: Vec::new(),
                stderr,
            });
        }
        if started.elapsed() >= timeout {
            // Kill, then reap: without the `wait` the child would be left a
            // zombie for the life of the daemon.
            let _ = child.kill();
            let _ = child.wait();
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                format!("assay probe timed out after {}s", timeout.as_secs()),
            ));
        }
        std::thread::sleep(PROBE_POLL_INTERVAL);
    }
}

/// [`run_bounded`] under a test-chosen deadline.
///
/// The injectable [`CommandRunner`] replaces the whole subprocess, so it
/// cannot exercise the spawn layer's timeout; this is the seam that lets a
/// test drive the real one against a real child without waiting out the
/// shipped default (600 s, `config::AssayConfig::probe_timeout_secs`).
#[cfg(any(test, feature = "test-support"))]
pub fn run_bounded_for_test(
    program: &str,
    args: &[String],
    timeout: Duration,
) -> std::io::Result<Output> {
    run_bounded(program, args, timeout)
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
                // `true`: this daemon measured itself, so the profile's
                // ceiling must not clamp its own geometry (the anti-ratchet
                // rule on `Pager::create_agent`). Everything else in the
                // document — verdicts, and being profiled at all — counts.
                p.attach_profile(model, profile, true)?;
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
