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
///    - Failure (path doesn't exist): fall back to canonicalizing
///      `target`'s parent. If the parent canonicalizes and is in-bounds,
///      the result is the canonical parent joined with the file name. If
///      the parent canonicalizes but is out-of-bounds, refuse. If the
///      parent itself doesn't canonicalize, name it as the missing piece.
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
