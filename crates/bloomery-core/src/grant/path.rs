//! The canonicalization-based path escape defense.
//!
//! [`resolve_within`] is **the security boundary** for filesystem capability
//! grants: it is the only place that decides whether a model-chosen path is
//! actually inside a granted root. Everything downstream (P3's read/patch
//! executors) trusts its answer without re-checking.
//!
//! The defense is real `std::fs::canonicalize`, not string comparison:
//! canonicalize resolves symlinks and collapses `..` components against the
//! actual filesystem, so a symlink planted inside a granted root that points
//! outside it (or a `..` traversal) cannot produce a path that still looks
//! "inside". We deliberately do not mock or reimplement canonicalize — a
//! mock would encode our assumptions about the filesystem rather than the
//! filesystem's actual behavior, and the entire point of this module is to
//! match the real attack surface. Tests exercise real tempdirs and real
//! symlinks for the same reason.
//!
//! The comparison itself is component-wise [`Path::starts_with`], never a
//! string prefix check: `str::starts_with` would let `/root-evil` match a
//! `/root` grant (shared string prefix, no path component boundary), which
//! is exactly the kind of escape this module exists to prevent.

use super::{GrantViolation, PathKind};
use std::path::{Path, PathBuf};

/// Resolve `target` against `roots`, returning the canonical path if it is
/// within bounds.
///
/// The brief's signature carries an `allow_missing_target: bool` for
/// call-site clarity (true for write: creating a new file in a granted
/// directory). It is dropped here: per the brief's own algorithm, a read of
/// a not-yet-existing path under a granted root is in-bounds too (the
/// executor discovers "not found" later — the grant's job is bounds
/// checking, not existence checking), so both call sites always pass `true`
/// and the parameter can never change behavior. Keeping a parameter that is
/// always the same value and never read is dead weight; `check_read` and
/// `check_write` calling the same parent-fallback logic (see [`super::Grant`]
/// methods) documents the "both use parent-fallback" fact at least as
/// clearly as a constant `true` argument would.
///
/// Algorithm (binding, security-critical — see module docs):
/// 1. `target` must be absolute; relative paths are ambiguous here (P3
///    absolutizes model-supplied paths against the task cwd before calling).
/// 2. Canonicalize each root. Roots that fail to canonicalize (don't exist)
///    are skipped — a granted root that doesn't exist grants nothing, since
///    nothing can match it.
/// 3. Canonicalize `target`:
///    - Success: in-bounds iff it `starts_with` (component-wise) some
///      canonical root.
///    - Failure (path doesn't exist, or doesn't fully resolve): if
///      `target` nonetheless already exists as *some* filesystem entry
///      (checked via `symlink_metadata`, which does not itself follow
///      links) — i.e. a dangling or looping symlink — refuse outright.
///      A live symlink that resolves is caught by the branch above; the
///      only way to reach this point with an existing entry is a broken
///      link, and approving it as "new file in a granted dir" would let a
///      later plain `open`/`write` follow the link straight out of the
///      sandbox (see module docs and the fix-report entry for the
///      dangling-symlink escape this closes). Otherwise (`target` truly
///      does not exist at all) fall back to canonicalizing `target`'s
///      parent: in-bounds parent → the canonical parent joined with the
///      file name; out-of-bounds parent → refuse; parent itself doesn't
///      canonicalize → name it as the missing piece.
pub(crate) fn resolve_within(
    target: &Path,
    roots: &[PathBuf],
    kind: PathKind,
) -> Result<PathBuf, GrantViolation> {
    if !target.is_absolute() {
        return Err(GrantViolation::PathOutsideRoots {
            path: target.to_string_lossy().into_owned(),
            kind,
        });
    }

    let canonical_roots: Vec<PathBuf> = roots
        .iter()
        .filter_map(|root| std::fs::canonicalize(root).ok())
        .collect();

    if let Ok(canon) = std::fs::canonicalize(target) {
        return if canonical_roots.iter().any(|root| canon.starts_with(root)) {
            Ok(canon)
        } else {
            Err(GrantViolation::PathOutsideRoots {
                path: target.to_string_lossy().into_owned(),
                kind,
            })
        };
    }

    // `canonicalize` failed. If an entry already exists at `target` (a
    // dangling or looping symlink — the only way `symlink_metadata` can
    // succeed here, since a live, fully-resolving symlink would have taken
    // the `Ok(canon)` branch above), it is NOT a "new file" candidate: the
    // grant's job is bounds, and an entry that exists but escapes bounds is
    // out of bounds, full stop. Refuse before ever reaching the
    // parent-fallback / new-file path.
    if target.symlink_metadata().is_ok() {
        return Err(GrantViolation::PathOutsideRoots {
            path: target.to_string_lossy().into_owned(),
            kind,
        });
    }

    resolve_missing_target(target, &canonical_roots, kind)
}

/// Parent-fallback for a `target` that does not itself canonicalize (i.e.
/// does not currently exist). See [`resolve_within`] step 3.
fn resolve_missing_target(
    target: &Path,
    canonical_roots: &[PathBuf],
    kind: PathKind,
) -> Result<PathBuf, GrantViolation> {
    let Some(parent) = target.parent() else {
        return Err(GrantViolation::PathParentMissing {
            path: target.to_string_lossy().into_owned(),
        });
    };

    let Ok(parent_canon) = std::fs::canonicalize(parent) else {
        return Err(GrantViolation::PathParentMissing {
            path: target.to_string_lossy().into_owned(),
        });
    };

    if !canonical_roots
        .iter()
        .any(|root| parent_canon.starts_with(root))
    {
        return Err(GrantViolation::PathOutsideRoots {
            path: target.to_string_lossy().into_owned(),
            kind,
        });
    }

    match target.file_name() {
        Some(name) => Ok(parent_canon.join(name)),
        None => Err(GrantViolation::PathParentMissing {
            path: target.to_string_lossy().into_owned(),
        }),
    }
}
