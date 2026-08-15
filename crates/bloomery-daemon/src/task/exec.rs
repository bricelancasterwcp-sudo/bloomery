//! `read` and `find` executors, plus the shared plumbing every executor in
//! this module (this task's and later tasks') is built on.
//!
//! **The two binding security obligations, carried from P2 (this is the
//! whole point of this module's design):**
//!
//! 1. Every filesystem open uses the path a `Grant::check_read` /
//!    `check_write` call *returned* — the canonical, symlink-resolved
//!    path — never the raw, model-supplied target string. `Grant`'s
//!    boundary (`bloomery_core::grant::path::resolve_within`) is what
//!    decided the target is in-bounds; re-deriving a path from the model's
//!    string after that decision would let a TOCTOU symlink swap or a
//!    second, unchecked path silently widen scope.
//! 2. Every such open passes `O_NOFOLLOW` via
//!    `OpenOptionsExt::custom_flags` ([`open_nofollow_read`]) — final
//!    component symlink protection, so even a same-instant swap of the
//!    checked path's last component for a symlink is refused by the
//!    kernel (`ELOOP`) rather than followed. This is a *named v1 limit*,
//!    not a complete defense: `O_NOFOLLOW` only protects the final path
//!    component. A TOCTOU race against a *mid-path* component (some
//!    directory earlier in the path swapped for a symlink between the
//!    `check_read` canonicalization and this `open`) is not closed by
//!    this call — closing that requires `openat2(2)` with
//!    `RESOLVE_NO_SYMLINKS`, which is Linux-specific and not yet wired
//!    (tracked for a future pass, not silently assumed away).

use crate::task::lens_py::PythonLens;
use crate::task::{ExecBounds, Observation};
use bloomery_core::action::lens::{land, Landing, LandingLens, PlainText};
use bloomery_core::action::PatchBody;
use bloomery_core::grant::{Grant, GrantViolation};
use regex::Regex;
use std::io::{Read as _, Write as _};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// Per-file scan cap for [`exec_find`]'s file reads — independent of
/// [`ExecBounds::read_cap_bytes`], which bounds a single `read` action's
/// *returned* content. This one bounds how much of any *one* candidate
/// file `exec_find` will scan for matches while walking a whole tree, so a
/// single huge file sitting in a read root can't make a bounded `find`
/// unboundedly slow or memory-hungry. Named v1 limit: a match past this
/// many bytes into one file is missed, not silently invented — the same
/// stated-bound discipline as every other cap in this module.
const FIND_FILE_SCAN_CAP_BYTES: usize = 8 * 1024 * 1024;

/// Absolutize a possibly-relative model path against the task cwd, so the
/// result is ready for `grant.check_read`/`check_write` (both require an
/// absolute target — see `resolve_within`'s docs). An already-absolute `p`
/// is returned unchanged (relative to nothing, including `cwd`): a model
/// that names `/etc/passwd` should have that exact target checked, not a
/// `cwd`-relative reinterpretation of it.
pub(crate) fn absolutize(cwd: &Path, p: &str) -> PathBuf {
    let p = Path::new(p);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        cwd.join(p)
    }
}

/// Opens `canon` — which MUST already be the canonical path a `Grant`
/// check returned, per obligation 1 above — with `O_NOFOLLOW` and reads up
/// to `cap` bytes.
///
/// Reads `cap + 1` bytes (via `Read::take`) so truncation can be reported
/// truthfully: reading exactly `cap` bytes can never distinguish "the file
/// is exactly `cap` bytes" from "the file is longer and got cut off",
/// which is exactly the silent-truncation failure mode this crate's caps
/// exist to avoid (see [`super`]'s module docs). Returns the first `cap`
/// bytes plus whether more remained.
pub(crate) fn open_nofollow_read(canon: &Path, cap: usize) -> std::io::Result<(Vec<u8>, bool)> {
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(canon)?;
    let mut buf = Vec::new();
    file.take(cap as u64 + 1).read_to_end(&mut buf)?;
    let truncated = buf.len() > cap;
    buf.truncate(cap);
    Ok((buf, truncated))
}

/// Turns a [`GrantViolation`] into a short, repair-friendly string a model
/// can act on (e.g. "narrow the path" rather than a bare enum tag).
///
/// `pub(crate)`, not private: `exec_run` (in the sibling `exec_run` module)
/// uses this too, for the identical grant-violation-to-observation
/// translation every executor in this crate shares.
pub(crate) fn describe(v: &GrantViolation) -> String {
    match v {
        GrantViolation::PathOutsideRoots { path, kind } => {
            format!("{path} is outside the granted {kind:?} roots")
        }
        GrantViolation::PathParentMissing { path } => {
            format!("{path}'s parent directory does not exist within a granted root")
        }
        GrantViolation::CommandNotAllowed { argv } => {
            format!("command {argv:?} does not match a granted prefix")
        }
    }
}

/// Builds a `failed: true` [`Observation`] whose `outcome` and `content`
/// are both `text` — every failure path in this module reports the same
/// short reason to both the journal tag and the model transcript, so
/// there is nothing to keep in sync between the two.
///
/// `pub(crate)`, not private — see [`describe`]'s doc comment; `exec_run`
/// shares this too.
pub(crate) fn failed(text: String) -> Observation {
    Observation {
        outcome: text.clone(),
        content: text,
        failed: true,
    }
}

/// Applies an optional 1-indexed inclusive `lines` window to already-read
/// `text`. Out-of-range bounds clamp to the available lines rather than
/// erroring (a model guessing at a file's length should get *something*
/// useful back, not a hard failure over an off-by-one) — the clamp is
/// noted, appended to the returned text, so the model can tell its
/// request was adjusted rather than silently reading a clamped window as
/// if it were exactly what it asked for.
fn window_lines(text: &str, lines: Option<(u32, u32)>) -> String {
    let Some((start, end)) = lines else {
        return text.to_string();
    };
    let all: Vec<&str> = text.lines().collect();
    let total = all.len() as u32;
    if total == 0 {
        return format!("[note: requested lines {start}-{end} but the read content has no lines]");
    }
    let clamped_start = start.clamp(1, total);
    let clamped_end = end.clamp(clamped_start, total);
    let windowed = all[(clamped_start as usize - 1)..(clamped_end as usize)].join("\n");
    if clamped_start == start && clamped_end == end {
        windowed
    } else {
        format!(
            "{windowed}\n[note: requested lines {start}-{end} clamped to \
             {clamped_start}-{clamped_end} ({total} lines available)]"
        )
    }
}

/// Execute a `Read` action against `grant`. `cwd` is the task's working
/// directory, used only to absolutize a relative `path` — see
/// [`absolutize`].
pub fn exec_read(
    grant: &Grant,
    cwd: &Path,
    path: &str,
    lines: Option<(u32, u32)>,
    bounds: &ExecBounds,
) -> Observation {
    let abs = absolutize(cwd, path);
    let canon = match grant.check_read(&abs) {
        Ok(canon) => canon,
        Err(v) => return failed(format!("grant violation: {}", describe(&v))),
    };
    let (bytes, truncated) = match open_nofollow_read(&canon, bounds.read_cap_bytes) {
        Ok(v) => v,
        Err(e) if e.raw_os_error() == Some(libc::ELOOP) => {
            return failed(format!(
                "read failed: {e} — O_NOFOLLOW refused a symlink at the final path component \
                 (the target changed between the grant check and the open)"
            ));
        }
        Err(e) => return failed(format!("read failed: {e} ({:?})", e.kind())),
    };
    let text = String::from_utf8_lossy(&bytes).into_owned();
    let content = window_lines(&text, lines);
    let mut outcome = format!("read {} bytes", bytes.len());
    if truncated {
        outcome.push_str(" (truncated at cap)");
    }
    Observation {
        outcome,
        content,
        failed: false,
    }
}

/// Cap on the *current* file's size `exec_patch` will read before
/// attempting to land a patch against it. Unlike `ExecBounds::read_cap_bytes`
/// (which bounds what a `read` action hands *back to the model*),
/// `exec_patch`'s brief-mandated signature carries no `ExecBounds` — this is
/// a patch-specific safety bound, not a per-task-configurable one.
///
/// The reason a cap is needed at all, and why hitting it must fail closed
/// rather than silently truncate: `land()` applies the patch against
/// whatever `current` string it's given, then writes `new_contents` back
/// over the *entire* file. If the current-file read were silently truncated
/// at some cap the way `exec_read`'s is (truncation is fine there — it's
/// read-only, feeding the model a partial view), applying a patch against
/// that truncated view and writing the result back would destroy every byte
/// past the cap. So a file over this cap is refused outright — the same
/// "cannot verify, so do not claim it landed" posture as `PythonLens`'s
/// missing-`python3` case — rather than ever being patched from a
/// known-incomplete read.
const PATCH_READ_CAP_BYTES: usize = 16 * 1024 * 1024;

/// Chooses the landing lens for `path` by extension: `.py` gets the
/// verifying [`PythonLens`], everything else gets [`PlainText`] (accepts
/// anything). `path` is expected to be the *canonical* path a grant check
/// already returned, matching this module's usual discipline, though the
/// extension match itself doesn't depend on canonicalization.
pub(crate) fn lens_for(path: &Path) -> Box<dyn LandingLens> {
    if path.extension().and_then(|e| e.to_str()) == Some("py") {
        Box::new(PythonLens)
    } else {
        Box::new(PlainText)
    }
}

/// Builds a per-call-unique temp-file name for [`atomic_write`], living
/// alongside `canon` in the same directory (so the final rename is on one
/// filesystem — see that function's docs). PID plus a static counter, same
/// reasoning as `lens_py`'s scratch-path builder: many `exec_patch` calls
/// can run concurrently (parallel tests, or later a multi-task daemon) in
/// one process, all sharing a PID.
fn temp_sibling_name() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!(".bloomery-patch-{}-{n}.tmp", std::process::id())
}

/// Writes `contents` to a new temp file in the same directory as `canon`,
/// then atomically renames it over `canon`. A failed landing never reaches
/// this function at all (see [`exec_patch`]), so every call here is a
/// clean write meant to actually replace the file.
///
/// Two things make this safe against the attacks this module's docs call
/// out:
/// - **Same directory, one filesystem:** `std::fs::rename` is only atomic
///   when source and destination are on the same filesystem; picking a
///   sibling of `canon` (rather than, say, the OS temp dir) is what makes
///   the rename atomic rather than a copy-then-delete a reader could
///   observe half-done.
/// - **`create_new` + `O_NOFOLLOW` on the temp file itself:** the temp
///   name is guessable (PID + counter) in what's usually a shared,
///   world-writable-ish directory tree; `create_new` refuses to open a
///   path that already exists — including a pre-planted symlink — rather
///   than following or truncating it, and `O_NOFOLLOW` is the same
///   final-component belt-and-suspenders this module's other opens use.
///   Without this, a pre-planted symlink at the exact temp name could
///   redirect the write to an attacker-chosen target before the rename
///   ever runs.
///
/// If the rename fails, the temp file is cleaned up best-effort rather than
/// left behind — the caller reports the failure; a stray `.bloomery-patch-*`
/// file next to the target would be confusing debris from a failed attempt.
///
/// **Durability, not just atomicity:** the temp file is `sync_all`'d (a
/// full `fsync`, data and metadata) *before* the rename, not just written.
/// `rename` alone makes the swap atomic — a reader never observes a
/// half-written file — but on a real power loss (as opposed to a process
/// crash the OS survives) some filesystems can persist the rename ahead of
/// the temp file's data actually reaching disk, which would leave `canon`
/// pointing at an empty or stale-cached file after a crash. A coding
/// agent's patched source is exactly the kind of output that must not
/// vanish on a crash, so this write pays the `fsync` cost rather than
/// leaving that gap open. A `sync_all` failure is treated exactly like a
/// `write_all` failure: the temp file is cleaned up and the original
/// `canon` is never touched.
fn atomic_write(canon: &Path, contents: &str) -> std::io::Result<()> {
    let dir = canon.parent().ok_or_else(|| {
        std::io::Error::other(format!("{} has no parent directory", canon.display()))
    })?;
    let tmp = dir.join(temp_sibling_name());

    let write_result = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&tmp)
        .and_then(|mut file| {
            file.write_all(contents.as_bytes())?;
            file.sync_all()
        });

    if let Err(e) = write_result {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }

    if let Err(e) = std::fs::rename(&tmp, canon) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(())
}

/// Execute a `Patch` action against `grant`: atomic write-with-verify.
///
/// `cwd` absolutizes a relative `path` exactly as [`exec_read`] does. The
/// resolved, canonical write-target (`grant.check_write`'s return value —
/// never the raw `path` string, per this module's obligation 1) is what
/// every step below operates on:
///
/// 1. Read the *current* contents via [`open_nofollow_read`] (obligation 2:
///    `O_NOFOLLOW`), capped at [`PATCH_READ_CAP_BYTES`]. `NotFound` is
///    treated as an empty current file — the create-a-new-file case, which
///    `grant.check_write` already resolved to an in-bounds path via its
///    parent-fallback rule.
/// 2. [`land`] the patch: apply `body` to the current contents, then run
///    the lens [`lens_for`] chose for this path's extension.
/// 3. On [`Landing::Lands`], write the new contents back via
///    [`atomic_write`] — the file changes only here, only once, only after
///    both the apply and the verify steps succeeded.
/// 4. On [`Landing::DidNotApply`], [`Landing::DidNotParse`], or
///    [`Landing::Unparsed`], the file is left completely untouched (no
///    write is even attempted) and a failed [`Observation`] is returned
///    naming which step failed and the lens involved.
pub fn exec_patch(grant: &Grant, cwd: &Path, path: &str, body: &PatchBody) -> Observation {
    let abs = absolutize(cwd, path);
    let canon = match grant.check_write(&abs) {
        Ok(canon) => canon,
        Err(v) => return failed(format!("grant violation: {}", describe(&v))),
    };

    let current = match open_nofollow_read(&canon, PATCH_READ_CAP_BYTES) {
        Ok((_bytes, true)) => {
            return failed(format!(
                "patch failed: {} exceeds the {PATCH_READ_CAP_BYTES}-byte patch read cap — \
                 refusing to patch from a truncated read",
                canon.display()
            ));
        }
        Ok((bytes, false)) => String::from_utf8_lossy(&bytes).into_owned(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) if e.raw_os_error() == Some(libc::ELOOP) => {
            return failed(format!(
                "patch failed: {e} — O_NOFOLLOW refused a symlink at the final path component \
                 (the target changed between the grant check and the open)"
            ));
        }
        Err(e) => return failed(format!("patch failed: {e} ({:?})", e.kind())),
    };

    let lens = lens_for(&canon);
    match land(&current, body, lens.as_ref()) {
        Landing::Lands { new_contents, lens } => match atomic_write(&canon, &new_contents) {
            Ok(()) => Observation {
                outcome: format!("patched (lens: {lens})"),
                content: new_contents,
                failed: false,
            },
            Err(e) => failed(format!(
                "patch failed: could not write {}: {e}",
                canon.display()
            )),
        },
        Landing::DidNotApply { reason, lens } => {
            let detail = format!("{reason:?}");
            Observation {
                outcome: format!("patch did not land: did not apply (lens: {lens}): {detail}"),
                content: detail,
                failed: true,
            }
        }
        Landing::DidNotParse { detail, lens } => Observation {
            outcome: format!("patch did not land: did not parse (lens: {lens}): {detail}"),
            content: detail,
            failed: true,
        },
        Landing::Unparsed { language, lens } => {
            let detail = format!("lens {lens} cannot judge language {language}");
            Observation {
                outcome: format!("patch did not land: unparsed (lens: {lens}): {detail}"),
                content: detail,
                failed: true,
            }
        }
    }
}

/// Recursively walks `start` (a file or a directory), matching `re`
/// against every candidate file's lines, appending `"path:lineno: line"`
/// for each match to `out` until `out.len() >= cap`.
///
/// Security-critical: symlinks are never followed while walking — any
/// directory entry whose own `symlink_metadata` reports it as a symlink is
/// skipped outright, whether it points at a file or a directory. This is
/// stricter than "never follow symlinks *out*": it would be just as wrong
/// to recurse into a same-root-internal symlinked directory, because nothing
/// then stops a symlink *cycle* (e.g. a dir symlinked to its own ancestor)
/// from making this walk loop forever, or a symlink to a huge unrelated
/// subtree from making a bounded `find` unboundedly slow. On top of that
/// walk-level rule, every remaining (non-symlink) file candidate is
/// canonicalized and re-checked against `canonical_roots` immediately
/// before it is opened — defense in depth against anything the
/// symlink-skip alone wouldn't catch (a TOCTOU swap between the
/// `symlink_metadata` check and the read, for instance).
fn walk_and_match(
    start: &Path,
    re: &Regex,
    canonical_roots: &[PathBuf],
    cap: usize,
    out: &mut Vec<String>,
) {
    let mut stack = vec![start.to_path_buf()];
    while let Some(candidate) = stack.pop() {
        if out.len() >= cap {
            return;
        }
        let Ok(meta) = std::fs::symlink_metadata(&candidate) else {
            continue; // gone, or unreadable — skip, don't fail the whole walk
        };
        if meta.file_type().is_symlink() {
            continue; // never follow — see doc comment above
        }
        if meta.is_dir() {
            let Ok(entries) = std::fs::read_dir(&candidate) else {
                continue; // skip dirs that fail to read (permissions, races)
            };
            stack.extend(entries.flatten().map(|e| e.path()));
            continue;
        }
        if !meta.is_file() {
            continue; // sockets, fifos, devices — not searchable text
        }
        let Ok(canon_file) = std::fs::canonicalize(&candidate) else {
            continue;
        };
        if !canonical_roots
            .iter()
            .any(|root| canon_file.starts_with(root))
        {
            continue; // defense in depth — see doc comment above
        }
        match_file(&canon_file, re, cap, out);
    }
}

/// Matches `re` against every line of `path` (opened via the same
/// `O_NOFOLLOW` helper [`exec_read`] uses — see this module's binding
/// obligations), appending `"path:lineno: line"` for each hit to `out`
/// until `out.len() >= cap`.
fn match_file(path: &Path, re: &Regex, cap: usize, out: &mut Vec<String>) {
    let Ok((bytes, _truncated)) = open_nofollow_read(path, FIND_FILE_SCAN_CAP_BYTES) else {
        return; // unreadable (permissions, vanished, ELOOP) — skip, don't fail the walk
    };
    let text = String::from_utf8_lossy(&bytes);
    for (i, line) in text.lines().enumerate() {
        if re.is_match(line) {
            out.push(format!("{}:{}: {line}", path.display(), i + 1));
            if out.len() >= cap {
                return;
            }
        }
    }
}

/// Execute a `Find` action against `grant`: compile `pattern` fresh (it
/// was already validated as a regex by P1's `validate_find`, but this
/// executor is standalone per the brief and does not trust a caller's
/// prior validation), resolve `path_prefix` through `grant.check_read` —
/// reusing the exact same canonicalize-and-bound-check boundary
/// `exec_read` uses, rather than re-deriving root containment here — then
/// walk (see [`walk_and_match`]) collecting up to
/// `bounds.find_result_cap` `"path:lineno: line"` matches.
///
/// `path_prefix` has no `cwd` to absolutize against (unlike `exec_read`'s
/// `path`): the brief's `exec_find` signature carries no `cwd` parameter,
/// so a relative `path_prefix` falls back to the process's current
/// directory. In the intended P3 call path (Task 4's loop) the caller
/// always passes an already-absolute prefix (the task's own cwd, or a
/// path already absolutized against it), so this fallback is a defensive
/// default for a case the real call path shouldn't hit, not the primary
/// contract.
///
/// **Caveat debuggers/Task 4-5 authors need:** the walk never descends
/// into *any* symlinked directory, in-root or not (see [`walk_and_match`]'s
/// doc comment for why). So a zero-match result does not mean "no line in
/// the read root matches" — it can also mean a real match sits behind a
/// symlink this walk deliberately never opened. That is a stated v1 limit,
/// not a bug to chase if a `find` misses something reachable only through
/// a symlinked path.
pub fn exec_find(
    grant: &Grant,
    pattern: &str,
    path_prefix: &str,
    bounds: &ExecBounds,
) -> Observation {
    let re = match Regex::new(pattern) {
        Ok(re) => re,
        Err(e) => return failed(format!("bad regex {pattern:?}: {e}")),
    };

    let prefix = Path::new(path_prefix);
    let prefix = if prefix.is_absolute() {
        prefix.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(prefix)
    };

    let canon_prefix = match grant.check_read(&prefix) {
        Ok(canon) => canon,
        Err(v) => return failed(format!("grant violation: {}", describe(&v))),
    };

    let canonical_roots: Vec<PathBuf> = grant
        .read_roots()
        .iter()
        .filter_map(|r| std::fs::canonicalize(r).ok())
        .collect();

    // Walk for cap + 1 so we can truthfully report whether the result was
    // truncated (same reasoning as `open_nofollow_read`'s cap+1 read).
    let internal_cap = bounds.find_result_cap.saturating_add(1);
    let mut out = Vec::new();
    walk_and_match(&canon_prefix, &re, &canonical_roots, internal_cap, &mut out);

    let over_cap = out.len() > bounds.find_result_cap;
    if over_cap {
        out.truncate(bounds.find_result_cap);
    }
    let mut outcome = format!("found {} matches", out.len());
    if over_cap {
        outcome.push_str(&format!(" (capped at {} results)", bounds.find_result_cap));
    }
    Observation {
        outcome,
        content: out.join("\n"),
        failed: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a fresh tempdir under a caller-chosen name suffix (so two
    /// tests in this module never collide on the same directory).
    fn tempdir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "bloomery-exec-nofollow-{tag}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Pins the O_NOFOLLOW/ELOOP mechanism structurally.
    ///
    /// Every escape-symlink test in `task_exec_read_find_test.rs` refuses
    /// *before* `open_nofollow_read` is ever reached — `grant.check_read`
    /// already canonicalizes and rejects an out-of-root target, so those
    /// tests exercise the grant boundary, not this function's own
    /// `custom_flags(libc::O_NOFOLLOW)` call. Without a test that calls
    /// `open_nofollow_read` directly on a symlink, a future refactor could
    /// silently drop the flag (or typo the constant) and every existing
    /// test would stay green. This test bypasses the grant layer entirely
    /// on purpose, building a real symlink pointing at a real file and
    /// asserting the open itself is refused with `ELOOP` — the OS error
    /// `O_NOFOLLOW` produces when the final path component is a symlink.
    /// `raw_os_error()` is asserted rather than `ErrorKind`, because
    /// `ErrorKind` has no stably-named ELOOP variant to match on.
    #[test]
    fn open_nofollow_read_refuses_a_symlink_with_eloop() {
        let dir = tempdir("eloop");
        let target = dir.join("target.txt");
        std::fs::write(&target, b"hello").unwrap();
        let link = dir.join("link");
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let err = open_nofollow_read(&link, 1024).expect_err("O_NOFOLLOW must refuse a symlink");
        assert_eq!(
            err.raw_os_error(),
            Some(libc::ELOOP),
            "expected ELOOP, got {err:?}"
        );
    }

    /// Regression companion to the test above: `O_NOFOLLOW` must not
    /// affect opening a real, non-symlink file — only a symlink at the
    /// final path component is refused. Without this test, a fix for the
    /// test above could overcorrect (e.g. by refusing every open) and
    /// nothing would catch it.
    #[test]
    fn open_nofollow_read_opens_a_regular_file_fine() {
        let dir = tempdir("regular");
        let target = dir.join("target.txt");
        std::fs::write(&target, b"hello").unwrap();

        let (bytes, truncated) = open_nofollow_read(&target, 1024).unwrap();
        assert_eq!(bytes, b"hello");
        assert!(!truncated);
    }

    /// Pins the `create_new` + `O_NOFOLLOW` mechanism [`atomic_write`]
    /// relies on to defeat a pre-planted temp-file symlink, at the exact
    /// `OpenOptions` combination it uses — bypassing `atomic_write`'s own
    /// (unpredictable, counter-based) temp-name generation entirely by
    /// building the symlink at a path this test controls directly and
    /// opening it the same way `atomic_write` would.
    ///
    /// Without a test pinning this mechanism directly, a refactor that
    /// dropped `create_new` (e.g. falling back to plain
    /// `.write(true).create(true)`, which *would* follow and truncate an
    /// existing symlink) would silently reopen exactly the temp-file
    /// hijack `atomic_write`'s doc comment warns about — and no
    /// `exec_patch`-level test would catch it, because every one of those
    /// tests uses a fresh sandbox with nothing pre-planted at the
    /// counter-generated temp name.
    #[test]
    fn create_new_nofollow_refuses_a_preplanted_symlink() {
        let dir = tempdir("create-new-symlink");
        let outside = dir.join("outside-target.txt");
        std::fs::write(&outside, b"must never be overwritten").unwrap();
        let planted = dir.join("planted.tmp");
        std::os::unix::fs::symlink(&outside, &planted).unwrap();

        let err = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&planted)
            .expect_err("create_new + O_NOFOLLOW must refuse a pre-planted symlink");
        assert_eq!(err.kind(), std::io::ErrorKind::AlreadyExists);

        let untouched = std::fs::read(&outside).unwrap();
        assert_eq!(untouched, b"must never be overwritten");
    }

    /// `atomic_write` end to end, proving the temp file left no trace
    /// after a clean rename and that `canon` holds exactly the new
    /// contents — the two properties `exec_patch`'s `Landing::Lands` arm
    /// depends on.
    #[test]
    fn atomic_write_replaces_the_file_and_leaves_no_temp_sibling() {
        let dir = tempdir("atomic-write-clean");
        let canon = dir.join("target.txt");
        std::fs::write(&canon, "old\n").unwrap();

        atomic_write(&canon, "new\n").unwrap();

        assert_eq!(std::fs::read_to_string(&canon).unwrap(), "new\n");
        let stray: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|name| name != "target.txt")
            .collect();
        assert!(stray.is_empty(), "stray files left behind: {stray:?}");
    }
}
