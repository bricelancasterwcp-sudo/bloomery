//! Profile retention and identity — the drift watch's filing cabinet
//! (`docs/superpowers/specs/2026-08-17-drift-watch-design.md` §5).
//!
//! Per model the daemon keeps four kinds of assay profile document, all in
//! the one profiles directory `main.rs` creates:
//!
//! | kind | file name | what it is |
//! |---|---|---|
//! | current | `{model}.json` | this boot's measurement, written by POST |
//! | previous | `{model}.previous.json` | last boot's, the drift-step reference |
//! | baseline | `{model}.baseline.json` | the blessed drift-cumulative reference |
//! | transient | `{model}.transient-{sha8}.json` | a confirm probe's document, bounded at [`MAX_TRANSIENTS`] |
//!
//! **One naming rule, not two.** POST writes this boot's document to
//! [`profile_file_name`] and this module reads it back through the same
//! function ([`ProfileStore::paths`]); the retention siblings are that name
//! with a qualifier spliced before the extension. A model key goes into a
//! file name verbatim, exactly as POST has always done it — a key containing
//! a path separator would already have broken POST's own paths, and this
//! module deliberately does not paper over that with a slug the journal's
//! path claims could not be checked against.
//!
//! **Content addressing is for verifiability, not for uniqueness.** Transient
//! file names carry the first [`SHA_PREFIX_LEN`] hex of the document's
//! sha256, so a journal row that names a file can be checked against the
//! bytes at that path with `sha256sum`. Blessing journals the *full* digest
//! for the same reason — the prefix is a naming device, the full digest is
//! the claim.
//!
//! Beside the filing cabinet sits the **gate** ([`DriftGate`], design §3-§4):
//! one comparison of a reference profile against a current one, run as
//! `assay diff --gate` and reported as a named [`GateOutcome`]. It reads both
//! documents *before* it spawns anything, so an unmeasurable comparison and a
//! changed instrument are refusals rather than subprocess results.
//!
//! Neither half writes a journal row: every method hands its outcome back as
//! a named value ([`Rotation`], [`Blessing`], [`Retention`], [`GateReading`])
//! for the caller to journal — design deliverable 4 owns the boot wiring and
//! the confirm-then-alarm loop.

use std::path::{Path, PathBuf};
use std::time::Duration;

use bloomery_core::journal::{sha256_hex_bytes, Event};
use bloomery_core::profile::{instrument_precheck, InstrumentPrecheck, Profile};

use crate::post::CommandRunner;

/// Every profile document is JSON, whatever role it plays.
const PROFILE_EXT: &str = "json";

/// The qualifier spliced into [`profile_file_name`] for last boot's document.
const PREVIOUS_QUALIFIER: &str = "previous";

/// The qualifier for the blessed drift-cumulative reference.
const BASELINE_QUALIFIER: &str = "baseline";

/// Leading text of every transient's qualifier; the sha prefix follows it.
const TRANSIENT_QUALIFIER_PREFIX: &str = "transient-";

/// How many hex characters of the sha256 go into a transient's file name.
/// Enough that two distinct confirm documents colliding is not a practical
/// concern, short enough that the name stays readable in a journal row — and
/// a collision would mean identical bytes anyway, since the full digest is
/// what any verification actually checks.
pub const SHA_PREFIX_LEN: usize = 8;

/// Spec §5's bound: the latest N=4 transient (confirm-run) profiles per
/// model, oldest dropped and journaled. Bounded because a model that trips
/// the gate every boot would otherwise fill the profiles directory forever;
/// four because a confirm run produces at most one per boot, so four is
/// several boots of history at the grain an operator actually looks at.
pub const MAX_TRANSIENTS: usize = 4;

/// The file name a model's **current** profile document takes inside the
/// profiles directory.
///
/// One function, two call sites: `post::probe_each` writes this boot's
/// document to it, and [`ProfileStore::paths`] reads it back. Extracted from
/// POST rather than reimplemented beside it, so the store's idea of "the
/// current profile" cannot drift from the file POST actually writes —
/// `tests/drift_test.rs` pins that agreement by running the real POST.
pub fn profile_file_name(model: &str) -> String {
    format!("{model}.{PROFILE_EXT}")
}

/// [`profile_file_name`] with `qualifier` spliced in before the extension —
/// the one place a retention sibling gets its name.
fn retained_file_name(model: &str, qualifier: &str) -> String {
    format!("{model}.{qualifier}.{PROFILE_EXT}")
}

/// Where a model's three named profile documents live. Transients are not
/// here: their names depend on their contents, so they are discovered rather
/// than derived (see [`ProfileStore::retain_transient`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelPaths {
    /// This boot's measurement — the file POST writes.
    pub current: PathBuf,
    /// Last boot's measurement, the drift-step reference.
    pub previous: PathBuf,
    /// The blessed drift-cumulative reference.
    pub baseline: PathBuf,
}

/// What [`ProfileStore::rotate`] did, always by name. There is no silent
/// outcome: a caller that journals this variant records what moved, what did
/// not, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rotation {
    /// The current document parsed and became the previous one.
    Rotated { from: PathBuf, to: PathBuf },
    /// There was no current document to rotate. The first boot ever for a
    /// model looks like this — and so does a caller that rotated *after*
    /// POST's delete-before-probe, which is why this is a named result rather
    /// than a silent `Ok(())`: the wrong order shows up in the journal every
    /// boot as a rotation that moved nothing.
    NothingToRotate { current: PathBuf },
    /// A current document existed but did not parse, so it was **not**
    /// promoted: spec §5 rotates on successful parse only, and a corrupt
    /// document must never become "the previous boot's measurement". The
    /// previous reference already on disk survives untouched, and the
    /// unparseable file stays where it is for an operator to look at.
    KeptUnparseable { current: PathBuf, reason: String },
}

/// A blessed baseline: where it was written, and the sha256 of the bytes
/// written there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Blessing {
    /// The baseline document this blessing produced.
    pub path: PathBuf,
    /// sha256 of the blessed **bytes**, full 64-hex. Full rather than the
    /// [`SHA_PREFIX_LEN`] prefix that appears in transient file names: the
    /// prefix is a naming device, whereas this is the durable claim a journal
    /// row makes, and `sha256sum` on [`path`](Blessing::path) checks it
    /// directly.
    pub sha: String,
}

/// What [`ProfileStore::retain_transient`] filed, and what it had to drop to
/// stay inside [`MAX_TRANSIENTS`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Retention {
    /// The content-addressed path the document now lives at.
    pub retained: PathBuf,
    /// Transients removed to stay within the bound, oldest first. Returned
    /// rather than journaled here so the drop is recorded by the same caller
    /// that recorded the confirm run it belonged to.
    pub dropped: Vec<PathBuf>,
}

/// Everything the retention layer can fail at, each variant a different
/// operator action.
#[derive(Debug)]
pub enum DriftError {
    /// There is no current profile for this model, so there is nothing to
    /// bless. Not an I/O error and not a panic: on a boot where POST failed,
    /// this is the expected answer.
    NoCurrentProfile { model: String, path: PathBuf },
    /// A filesystem operation failed, named with the path it failed on —
    /// which is the part `std::io::Error` alone never tells the operator.
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
}

impl std::fmt::Display for DriftError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DriftError::NoCurrentProfile { model, path } => write!(
                f,
                "no current profile for {model} at {}; nothing to bless",
                path.display()
            ),
            DriftError::Io { path, source } => {
                write!(f, "{}: {source}", path.display())
            }
        }
    }
}

impl std::error::Error for DriftError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            DriftError::NoCurrentProfile { .. } => None,
            DriftError::Io { source, .. } => Some(source),
        }
    }
}

/// The profiles directory, and the retention rules over it.
///
/// Holds a path and nothing else: no handles, no cache, no clock. Two stores
/// rooted at the same directory are interchangeable, and a store never
/// creates its root — `main.rs` does that once at boot, and a store that
/// silently created a missing profiles directory would hide a
/// misconfiguration behind an empty history.
pub struct ProfileStore {
    root: PathBuf,
}

impl ProfileStore {
    /// A store over `root`, the profiles directory
    /// (`config.data_dir.join("profiles")`).
    pub fn new(root: impl Into<PathBuf>) -> ProfileStore {
        ProfileStore { root: root.into() }
    }

    /// The profiles directory this store files into.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The three named documents for `model`. Pure: derives paths, touches
    /// nothing, and says nothing about which of them exist.
    pub fn paths(&self, model: &str) -> ModelPaths {
        ModelPaths {
            current: self.root.join(profile_file_name(model)),
            previous: self
                .root
                .join(retained_file_name(model, PREVIOUS_QUALIFIER)),
            baseline: self
                .root
                .join(retained_file_name(model, BASELINE_QUALIFIER)),
        }
    }

    /// Moves this model's current profile to `previous`, if and only if it
    /// parses.
    ///
    /// **When to call this (spec §5's rotation law).** Once per boot per
    /// model, *before* POST probes that model — POST deletes the current
    /// document before running assay, so a rotation after the probe would
    /// either find nothing (this boot's reference silently lost) or, worse
    /// under a different implementation, promote this boot's own measurement
    /// to be its own reference. Rotating first, on successful parse, is what
    /// makes "a stale file can never be read as this boot's measurement" and
    /// "the previous boot's measurement is a real previous boot's" both true.
    ///
    /// The parse gate lives here rather than in the caller on purpose: the
    /// law is a property of rotation, and a caller cannot get it wrong by
    /// forgetting to check. Every outcome is a named [`Rotation`], including
    /// both no-op cases — an absent current is never an error and never
    /// silent.
    ///
    /// **Corruption is a `Rotation`, not an `Err`.** The bytes are read raw
    /// and decoded here rather than through `read_to_string`, because a torn
    /// write leaves a file that is not valid UTF-8 (a NUL-filled block, a
    /// half-written multibyte sequence) — and `read_to_string` reports that
    /// as `InvalidData`, an `io::Error`. That would send the single most
    /// likely corruption mode down the one path [`Rotation::KeptUnparseable`]
    /// exists to keep it off: an error return the caller cannot distinguish
    /// from an unreadable disk, when the correct answer is "this document is
    /// not a profile, so previous stands". `Err` is now reserved for the
    /// filesystem genuinely failing.
    pub fn rotate(&self, model: &str) -> std::io::Result<Rotation> {
        let paths = self.paths(model);
        let bytes = match std::fs::read(&paths.current) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Rotation::NothingToRotate {
                    current: paths.current,
                });
            }
            Err(e) => return Err(e),
        };
        let text = match String::from_utf8(bytes) {
            Ok(text) => text,
            Err(e) => {
                return Ok(Rotation::KeptUnparseable {
                    current: paths.current,
                    reason: format!("not valid UTF-8: {e}"),
                });
            }
        };
        if let Err(e) = Profile::from_json(&text) {
            return Ok(Rotation::KeptUnparseable {
                current: paths.current,
                reason: e.to_string(),
            });
        }
        std::fs::rename(&paths.current, &paths.previous)?;
        Ok(Rotation::Rotated {
            from: paths.current,
            to: paths.previous,
        })
    }

    /// Copies this model's current profile to its baseline, making it the
    /// drift-cumulative reference, and reports the sha256 of the bytes that
    /// landed there.
    ///
    /// A copy, not a move: the current document is still this boot's
    /// measurement and the drift-step machinery still needs it. Written
    /// through a temporary sibling and renamed into place, so a crash
    /// mid-write cannot leave a truncated baseline that every future
    /// comparison would silently read as the reference.
    ///
    /// Blessing is journaled by the caller as
    /// [`Event::Blessed`](bloomery_core::journal::Event::Blessed) with the
    /// provenance that motivated it; this method does not decide or record
    /// provenance.
    pub fn bless(&self, model: &str) -> Result<Blessing, DriftError> {
        let paths = self.paths(model);
        let bytes = match std::fs::read(&paths.current) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(DriftError::NoCurrentProfile {
                    model: model.to_string(),
                    path: paths.current,
                });
            }
            Err(e) => {
                return Err(DriftError::Io {
                    path: paths.current,
                    source: e,
                })
            }
        };
        let sha = sha256_hex_bytes(&bytes);
        write_atomically(&paths.baseline, &bytes)
            .map_err(|(path, source)| DriftError::Io { path, source })?;
        Ok(Blessing {
            path: paths.baseline,
            sha,
        })
    }

    /// Files a confirm run's profile document under its content-addressed
    /// transient name, then prunes this model's transients back to
    /// [`MAX_TRANSIENTS`].
    ///
    /// The document is **moved**, not copied: the confirm probe wrote it to a
    /// fresh path of the caller's choosing, and leaving a second copy there
    /// would make two files claim to be the same measurement. Dropped paths
    /// come back in [`Retention::dropped`] for the caller to journal — a file
    /// this daemon deleted is a fact about the evidence trail, not
    /// housekeeping.
    ///
    /// Pruning is deterministic: oldest by mtime, ties broken by file name.
    /// The name is a content hash, so the tiebreak is arbitrary — but it is
    /// *stable*, which is the property that matters when a filesystem stamps
    /// several files in the same tick.
    pub fn retain_transient(&self, model: &str, path: &Path) -> std::io::Result<Retention> {
        let bytes = std::fs::read(path)?;
        let sha = sha256_hex_bytes(&bytes);
        let qualifier = format!("{TRANSIENT_QUALIFIER_PREFIX}{}", &sha[..SHA_PREFIX_LEN]);
        let retained = self.root.join(retained_file_name(model, &qualifier));

        if retained != path {
            move_file(path, &retained)?;
        }
        let dropped = self.prune_transients(model)?;
        Ok(Retention { retained, dropped })
    }

    /// Deletes this model's oldest transients until at most
    /// [`MAX_TRANSIENTS`] remain, returning what went, oldest first.
    fn prune_transients(&self, model: &str) -> std::io::Result<Vec<PathBuf>> {
        let prefix = format!("{model}.{TRANSIENT_QUALIFIER_PREFIX}");
        let suffix = format!(".{PROFILE_EXT}");

        let mut found: Vec<(std::time::SystemTime, String, PathBuf)> = Vec::new();
        for entry in std::fs::read_dir(&self.root)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            // Prefix-matching on `{model}.transient-` keeps another model's
            // transients (and this model's current/previous/baseline) out of
            // the bound: each model's confirm history is bounded on its own.
            if !name.starts_with(&prefix) || !name.ends_with(&suffix) {
                continue;
            }
            found.push((entry.metadata()?.modified()?, name, entry.path()));
        }

        found.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
        let over = found.len().saturating_sub(MAX_TRANSIENTS);
        let mut dropped = Vec::with_capacity(over);
        for (_, _, path) in found.into_iter().take(over) {
            std::fs::remove_file(&path)?;
            dropped.push(path);
        }
        Ok(dropped)
    }
}

/// Writes `bytes` to `path` without ever leaving a half-written file there:
/// into a sibling temporary first, then a rename, which is atomic within one
/// directory. On failure the temporary is cleaned up; the error names the
/// path it happened on so the caller can wrap it in [`DriftError::Io`].
fn write_atomically(path: &Path, bytes: &[u8]) -> Result<(), (PathBuf, std::io::Error)> {
    let mut tmp = path.as_os_str().to_os_string();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    if let Err(e) = std::fs::write(&tmp, bytes) {
        return Err((tmp, e));
    }
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err((path.to_path_buf(), e));
    }
    Ok(())
}

/// Moves `from` to `to`, falling back to copy-then-delete when the two are on
/// different filesystems (`rename` answers `EXDEV` there). The fallback is
/// the same operation, not a weaker one: `to` only exists once the copy
/// succeeded, and `from` only disappears after that.
fn move_file(from: &Path, to: &Path) -> std::io::Result<()> {
    match std::fs::rename(from, to) {
        Ok(()) => Ok(()),
        Err(e) if e.raw_os_error() == Some(libc::EXDEV) => {
            std::fs::copy(from, to)?;
            std::fs::remove_file(from)
        }
        Err(e) => Err(e),
    }
}

// ---------------------------------------------------------------------------
// The gate — design §3 (instrument precheck) and §4 (the diff subprocess)
// ---------------------------------------------------------------------------

/// Wall-clock cap on one `assay diff --gate` subprocess, in seconds.
///
/// **Deliberately not `AssayConfig::probe_timeout_secs`.** That cap bounds a
/// *probe*, which drives a model through many generations and legitimately
/// takes minutes — an operator serving a slow, partially-offloaded model
/// raises it into the tens of minutes (see `post::PostRunner::new`). `assay
/// diff` does none of that: it reads two JSON documents and does arithmetic
/// over them. No model, no GPU, no network. Inheriting the probe's cap would
/// mean that raising the probe timeout for a slow model silently also gave a
/// wedged diff half an hour to hold up the boot.
///
/// 60 s is two orders of magnitude above what the work costs and far below
/// any interval a boot can afford to lose, so it can only ever fire on a
/// genuinely wedged child — which is then named
/// [`GateOutcome::Infra`], never a verdict. Not operator-configurable,
/// because there is no measured workload that would need it to move.
pub const DIFF_TIMEOUT_SECS: u64 = 60;

/// [`DIFF_TIMEOUT_SECS`] as a `Duration`, so the seconds are spelled once.
const DIFF_TIMEOUT: Duration = Duration::from_secs(DIFF_TIMEOUT_SECS);

/// Which of design §2's two comparisons a reading belongs to. Both run every
/// boot and each journals its own row: step alone leaks the ratchet,
/// cumulative alone goes stale.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Comparison {
    /// This boot's profile against the previous boot's.
    Step,
    /// This boot's profile against the blessed baseline.
    Cumulative,
}

impl Comparison {
    /// The name this comparison takes in the journal.
    pub fn as_str(self) -> &'static str {
        match self {
            Comparison::Step => "step",
            Comparison::Cumulative => "cumulative",
        }
    }
}

/// What one comparison decided. Every case is named; there is no default and
/// no bare boolean, because the failure this family exists to refuse is a
/// comparison that could not be made being recorded as one that passed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GateOutcome {
    /// `assay diff --gate` exited 0: the two documents differ by no more than
    /// assay's own noise discipline allows.
    WithinNoise,
    /// Exited 1: drift beyond noise. A *reading*, not an alarm — design §4's
    /// confirm-then-alarm rule means the caller re-probes before this becomes
    /// a finding (deliverable 4).
    Drift,
    /// Exited 2: diff itself refused to compare the two (e.g. one-sided tier
    /// marking). Infrastructure-shaped, but diff's own documented answer, so
    /// it keeps its code.
    NotComparable {
        /// The exit code diff reported — 2 today; carried rather than assumed
        /// so the row records what actually happened.
        exit: i32,
    },
    /// Design §3: the two documents were measured by different instruments,
    /// so no comparison between them measures the model. Both identities in
    /// `"<probe_version>/v<schema>"` form. The diff is never spawned.
    InstrumentChanged {
        /// The reference document's instrument identity.
        reference: String,
        /// The current document's instrument identity.
        current: String,
    },
    /// There was nothing here to measure. Either one of the two documents
    /// could not be read as a profile — absent (first boot ever, a baseline
    /// nobody blessed, a boot where POST failed), unreadable, or not
    /// parseable — or the two describe **different models**, which makes any
    /// comparison between them a statement about neither. Named with the
    /// reason, and never a pass: this is the silent-pass bug the whole gate
    /// exists to refuse.
    Unmeasured {
        /// Which side failed and why, path included; for a crossed pair, both
        /// paths and both model names.
        reason: String,
    },
    /// The comparison could not be run: the subprocess would not start, was
    /// killed (by the timeout or by a signal), or answered with an exit code
    /// assay does not document. Not a verdict in either direction.
    Infra {
        /// What went wrong, in the failing layer's own words.
        detail: String,
    },
}

impl GateOutcome {
    /// The `outcome` string this verdict takes in
    /// [`Event::Drift`](bloomery_core::journal::Event::Drift).
    ///
    /// The variants that carry context fold it in here rather than getting
    /// their own journal fields: an instrument-changed row is useless without
    /// both identities, and an unmeasured row is useless without the reason.
    /// All of it is identity or prose — never a number transcribed out of a
    /// profile.
    pub fn journal_outcome(&self) -> String {
        match self {
            GateOutcome::WithinNoise => "within-noise".to_string(),
            GateOutcome::Drift => "drift".to_string(),
            GateOutcome::NotComparable { .. } => "not-comparable".to_string(),
            GateOutcome::InstrumentChanged { reference, current } => {
                format!("instrument-changed ({reference} -> {current})")
            }
            GateOutcome::Unmeasured { reason } => format!("unmeasured: {reason}"),
            GateOutcome::Infra { detail } => format!("infra: {detail}"),
        }
    }
}

/// Everything one comparison learned: the verdict, the exit code behind it,
/// and the identity of the exact bytes it compared.
///
/// The paths and digests ride here rather than being re-derived by the caller
/// on the way to the journal. Re-reading the files to hash them would make the
/// row's digest describe a *second* read — so a row could name bytes the gate
/// never compared, which is precisely the claim the digest exists to rule out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GateReading {
    /// The named verdict.
    pub outcome: GateOutcome,
    /// What `assay diff --gate` exited with, or `None` when no diff ran (or
    /// ran and was killed by a signal, which leaves no code at all).
    pub exit_code: Option<i32>,
    /// The reference document this comparison was asked about.
    pub reference_path: PathBuf,
    /// The current document this comparison was asked about.
    pub current_path: PathBuf,
    /// sha256 of the reference's **bytes**, full 64-hex, taken when the gate
    /// read them. `None` when that file was never read.
    pub reference_sha: Option<String>,
    /// sha256 of the current document's **bytes**. `None` when it was never
    /// read.
    pub current_sha: Option<String>,
}

/// The documented invocation, in one place:
///
/// ```text
/// {python} -m assay diff {reference} {current} --gate
/// ```
///
/// Split out so the argument list is a value tests inspect rather than a side
/// effect of spawning (the same treatment `post::argv` gets). `--gate` is what
/// makes assay answer in exit codes — design §4's contract is the exit code
/// and the documents, never diff's prose output, which this daemon does not
/// read at all.
pub fn diff_argv(reference: &Path, current: &Path) -> Vec<String> {
    [
        "-m",
        "assay",
        "diff",
        &reference.display().to_string(),
        &current.display().to_string(),
        "--gate",
    ]
    .iter()
    .map(|s| (*s).to_string())
    .collect()
}

/// One profile document as the gate read it: the digest of whatever bytes were
/// there, and either the parsed profile or why it is not one.
struct ProfileRead {
    /// sha256 of the file's bytes. `None` only when the file could not be read
    /// at all — bytes that exist but do not parse still have a digest, and an
    /// operator chasing an unparseable document wants exactly that digest.
    sha: Option<String>,
    profile: Result<Profile, String>,
}

/// Reads one document, hashing the bytes on the way through. Never an `Err`:
/// every failure is a reason string the caller turns into
/// [`GateOutcome::Unmeasured`], because "there is no reference yet" is the
/// normal first-boot case, not an error condition.
fn read_profile(path: &Path) -> ProfileRead {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return ProfileRead {
                sha: None,
                profile: Err(format!("{}: no such file", path.display())),
            };
        }
        Err(e) => {
            return ProfileRead {
                sha: None,
                profile: Err(format!("{}: {e}", path.display())),
            };
        }
    };
    let sha = Some(sha256_hex_bytes(&bytes));
    // Decoded here rather than via `read_to_string` for the same reason
    // `ProfileStore::rotate` does it: a torn write leaves bytes that are not
    // valid UTF-8, and that must read as "this is not a profile", not as an
    // unreadable disk.
    let text = match String::from_utf8(bytes) {
        Ok(text) => text,
        Err(e) => {
            return ProfileRead {
                sha,
                profile: Err(format!("{}: not valid UTF-8: {e}", path.display())),
            };
        }
    };
    let profile = Profile::from_json(&text).map_err(|e| format!("{}: {e}", path.display()));
    ProfileRead { sha, profile }
}

/// Runs design §4's comparison: `assay diff --gate` over two profile
/// documents, behind design §3's instrument precheck.
///
/// Mirrors [`crate::post::PostRunner`]'s shape — an injected
/// [`CommandRunner`], an inspectable argv, a bounded spawn — because the two
/// solve the same problem: this daemon's verdicts must be testable without
/// python, without assay and without a GPU, and the exact invocation must be
/// a value rather than a side effect.
pub struct DriftGate {
    python: String,
    run: CommandRunner,
    timeout: Duration,
}

impl DriftGate {
    /// A gate that really spawns `{python} -m assay diff ...`, bounded by
    /// [`DIFF_TIMEOUT_SECS`].
    ///
    /// `python` comes from `config.assay.python`, the same interpreter POST
    /// probes with: a diff run under a different assay install than the probe
    /// would be comparing documents with a tool that did not write them.
    pub fn new(python: String) -> DriftGate {
        DriftGate {
            python,
            run: Box::new(|program: &str, args: &[String]| {
                crate::post::run_bounded(program, args, DIFF_TIMEOUT)
            }),
            timeout: DIFF_TIMEOUT,
        }
    }

    /// A gate with the command execution injected — every outcome, including
    /// the ones that must never spawn, testable with no assay installed.
    #[cfg(any(test, feature = "test-support"))]
    pub fn with_runner(f: CommandRunner) -> DriftGate {
        DriftGate {
            // The same spelling `config.assay.python` defaults to, imported
            // rather than retyped so the two cannot drift.
            python: crate::config::default_python(),
            run: f,
            timeout: DIFF_TIMEOUT,
        }
    }

    /// The cap this gate's subprocess runs under.
    pub fn timeout(&self) -> Duration {
        self.timeout
    }

    /// Compares `reference` against `current`, spawning `assay diff --gate`
    /// only if that comparison can mean anything.
    ///
    /// **The order is the contract.** Both documents are read and parsed
    /// first, then three refusals run in a fixed order, and only what survives
    /// all three reaches the subprocess:
    ///
    /// 1. A side that is absent or unparseable is
    ///    [`GateOutcome::Unmeasured`] — named, never a pass. First boot ever
    ///    has no previous profile and an unblessed model has no baseline, so
    ///    this is the *normal* path on a fresh install, which is exactly why
    ///    it must not be reachable by any code path that returns "fine".
    /// 2. Two documents describing **different models** are
    ///    [`GateOutcome::Unmeasured`] too, naming both. This is
    ///    `post::PostRunner::probe`'s rule applied to the pair rather than to
    ///    one document ("attaching it would credit one model with another's
    ///    measurements"): a crossed pair — a mis-set model key, a hand-copied
    ///    baseline, a path built from the wrong name — would otherwise diff
    ///    cleanly and be journaled under *this* model's name, which is a
    ///    verdict about a model nobody measured.
    /// 3. A changed instrument is [`GateOutcome::InstrumentChanged`]. Measured
    ///    motivation, not theory: assay's 2026-08 campaign diffs showed 12 of
    ///    15 models "improving" because the ceiling cap moved between probe
    ///    versions. Running the diff anyway would report the instrument and
    ///    call it the model.
    ///
    /// **Identity before instrument** (2 before 3): asking whether two
    /// documents were measured by the same instrument only means something
    /// once they are about the same thing. On a crossed pair the instrument
    /// answer is noise either way — `Comparable` would wave a crossed pair
    /// through to the diff, and `InstrumentChanged` would send the operator
    /// to re-bless a baseline when the real fault is that the two documents
    /// are about different models.
    ///
    /// Reading first is also what makes the digests honest: they are of the
    /// bytes this comparison actually consumed, taken before anything else
    /// could touch the files.
    pub fn compare(&self, reference: &Path, current: &Path) -> GateReading {
        let reference_read = read_profile(reference);
        let current_read = read_profile(current);
        let reading = |outcome: GateOutcome, exit_code: Option<i32>| GateReading {
            outcome,
            exit_code,
            reference_path: reference.to_path_buf(),
            current_path: current.to_path_buf(),
            reference_sha: reference_read.sha.clone(),
            current_sha: current_read.sha.clone(),
        };

        let (reference_profile, current_profile) =
            match (&reference_read.profile, &current_read.profile) {
                (Err(why), _) => {
                    return reading(
                        GateOutcome::Unmeasured {
                            reason: format!("reference {why}"),
                        },
                        None,
                    )
                }
                (_, Err(why)) => {
                    return reading(
                        GateOutcome::Unmeasured {
                            reason: format!("current {why}"),
                        },
                        None,
                    )
                }
                (Ok(r), Ok(c)) => (r, c),
            };

        if reference_profile.model_name() != current_profile.model_name() {
            return reading(
                GateOutcome::Unmeasured {
                    reason: format!(
                        "crossed documents: reference {} describes model {}, \
                         current {} describes model {}",
                        reference.display(),
                        reference_profile.model_name(),
                        current.display(),
                        current_profile.model_name(),
                    ),
                },
                None,
            );
        }

        if let InstrumentPrecheck::InstrumentChanged { reference, current } =
            instrument_precheck(reference_profile, current_profile)
        {
            return reading(GateOutcome::InstrumentChanged { reference, current }, None);
        }

        let args = diff_argv(reference, current);
        let output = match (self.run)(&self.python, &args) {
            Ok(output) => output,
            Err(e) => {
                return reading(
                    GateOutcome::Infra {
                        detail: format!("could not run {} {}: {e}", self.python, args.join(" ")),
                    },
                    None,
                )
            }
        };
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        // assay documents exactly 0, 1 and 2 for `diff --gate`. Any other code
        // is a tool this daemon does not understand — treating it as a verdict
        // (in either direction) would be inventing one.
        match output.status.code() {
            Some(0) => reading(GateOutcome::WithinNoise, Some(0)),
            Some(1) => reading(GateOutcome::Drift, Some(1)),
            Some(2) => reading(GateOutcome::NotComparable { exit: 2 }, Some(2)),
            Some(n) => reading(
                GateOutcome::Infra {
                    detail: with_stderr(
                        format!(
                            "undocumented exit {n} from `assay diff --gate` \
                             (0, 1 and 2 are the documented codes)"
                        ),
                        &stderr,
                    ),
                },
                Some(n),
            ),
            // No code at all: the child was killed by a signal. `-1` would
            // look like a code; `None` is what happened.
            None => reading(
                GateOutcome::Infra {
                    detail: with_stderr(
                        "`assay diff --gate` was killed by a signal, leaving no exit code"
                            .to_string(),
                        &stderr,
                    ),
                },
                None,
            ),
        }
    }
}

/// Appends assay's own stderr to an infrastructure detail when there is any.
/// Its words, verbatim, for the operator — never parsed, and never consulted
/// for a verdict.
fn with_stderr(detail: String, stderr: &str) -> String {
    if stderr.is_empty() {
        detail
    } else {
        format!("{detail}: {stderr}")
    }
}

/// The journal row for one comparison
/// ([`Event::Drift`](bloomery_core::journal::Event::Drift)).
///
/// Every field comes from the `reading` itself — including the two paths — so
/// the row cannot describe a different pair of documents than the gate
/// compared. Lives beside the gate rather than in the boot wiring so the call
/// site there is one line.
pub fn drift_event(model: &str, comparison: Comparison, reading: &GateReading) -> Event {
    Event::Drift {
        model: model.to_string(),
        comparison: comparison.as_str().to_string(),
        outcome: reading.outcome.journal_outcome(),
        reference_path: reading.reference_path.display().to_string(),
        current_path: reading.current_path.display().to_string(),
        exit_code: reading.exit_code,
        reference_sha: reading.reference_sha.clone(),
        current_sha: reading.current_sha.clone(),
    }
}
