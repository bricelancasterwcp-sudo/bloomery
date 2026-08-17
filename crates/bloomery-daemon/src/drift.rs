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
//! This module files documents and nothing else. It runs no gate, spawns no
//! subprocess, reads no clock beyond file mtimes, and writes no journal rows:
//! every method hands its outcome back as a named value for the caller to
//! journal (design deliverables 3 and 4 own the comparisons and the wiring).

use std::path::{Path, PathBuf};

use bloomery_core::journal::sha256_hex_bytes;
use bloomery_core::profile::Profile;

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
    pub fn rotate(&self, model: &str) -> std::io::Result<Rotation> {
        let paths = self.paths(model);
        let text = match std::fs::read_to_string(&paths.current) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Rotation::NothingToRotate {
                    current: paths.current,
                });
            }
            Err(e) => return Err(e),
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
