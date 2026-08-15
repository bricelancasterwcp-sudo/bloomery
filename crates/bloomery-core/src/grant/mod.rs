//! Grant capability scoping for v1 execution security.
//!
//! The Grant type represents a capability grant: a JSON-specified set of filesystem
//! roots (read/write), commands (allowed prefixes), and network access.
//!
//! **v1 Boundary:** Grant provides structural validation and scoping. It is NOT an OS
//! sandbox. Tasks 2–3 validate that operations (path access, command execution) comply
//! with the grant; the daemon (P3) enforces this against the actual execution context.

pub mod path;

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// A capability grant: read/write filesystem roots, allowed command prefixes, and network access.
///
/// All fields are private; `from_json` is the only constructor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Grant {
    read_roots: Vec<PathBuf>,
    write_roots: Vec<PathBuf>,
    commands: Vec<Vec<String>>,
    #[serde(default)]
    network: bool,
}

/// Grant construction error.
#[derive(Debug, Clone, PartialEq)]
pub enum GrantError {
    /// Network access requested (reserved in v1).
    NetworkNotSupported,
    /// A read or write root is not absolute.
    NonAbsoluteRoot { root: String },
    /// A command prefix list is empty.
    EmptyCommandPrefix,
    /// JSON parse error.
    Parse(String),
}

/// A grant violation detected by path or command validation (Tasks 2–3 produce these).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum GrantViolation {
    /// A path operation falls outside the allowed roots.
    PathOutsideRoots { path: String, kind: PathKind },
    /// A path's parent directory is missing (pre-creation validation).
    PathParentMissing { path: String },
    /// A command is not in the granted prefixes.
    CommandNotAllowed { argv: Vec<String> },
}

/// Path access kind for violations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum PathKind {
    Read,
    Write,
}

impl Grant {
    /// Find the first non-absolute root in a list.
    fn first_non_absolute(roots: &[PathBuf]) -> Option<String> {
        roots
            .iter()
            .find(|root| !root.is_absolute())
            .map(|root| root.to_string_lossy().into_owned())
    }

    /// Parse JSON and validate according to v1 rules.
    ///
    /// Validation order:
    /// 1. Parse JSON (failure → `Parse`)
    /// 2. If `network == true` → `NetworkNotSupported`
    /// 3. Any read/write root where `!is_absolute()` → `NonAbsoluteRoot`
    /// 4. Any commands entry that is empty → `EmptyCommandPrefix`
    ///
    /// An empty commands *list* is valid (no commands granted).
    /// Only an empty commands *prefix* (empty Vec<String>) is rejected.
    pub fn from_json(s: &str) -> Result<Grant, GrantError> {
        // Step 1: Parse JSON
        let grant: Grant = serde_json::from_str(s).map_err(|e| GrantError::Parse(e.to_string()))?;

        // Step 2: Check network
        if grant.network {
            return Err(GrantError::NetworkNotSupported);
        }

        // Step 3: Check absolute roots (read_roots first, then write_roots)
        if let Some(root) = Self::first_non_absolute(&grant.read_roots) {
            return Err(GrantError::NonAbsoluteRoot { root });
        }
        if let Some(root) = Self::first_non_absolute(&grant.write_roots) {
            return Err(GrantError::NonAbsoluteRoot { root });
        }

        // Step 4: Check for empty command prefixes
        for cmd in &grant.commands {
            if cmd.is_empty() {
                return Err(GrantError::EmptyCommandPrefix);
            }
        }

        Ok(grant)
    }

    /// Read-permitted roots.
    pub fn read_roots(&self) -> &[PathBuf] {
        &self.read_roots
    }

    /// Write-permitted roots.
    pub fn write_roots(&self) -> &[PathBuf] {
        &self.write_roots
    }

    /// Allowed command prefixes.
    pub fn commands(&self) -> &[Vec<String>] {
        &self.commands
    }

    /// Network access permitted (always false in v1).
    pub fn network(&self) -> bool {
        self.network
    }

    /// Resolve `target` (which MUST be absolute) against the read roots.
    ///
    /// Returns the canonical path on success. Follows symlinks and collapses
    /// `..` via `std::fs::canonicalize`, so no traversal or symlink can
    /// escape a granted root — see [`path::resolve_within`] for the security
    /// boundary itself.
    pub fn check_read(&self, target: &std::path::Path) -> Result<PathBuf, GrantViolation> {
        path::resolve_within(target, self.read_roots(), PathKind::Read)
    }

    /// As [`Grant::check_read`], against write roots; if `target` does not
    /// exist yet, its immediate parent must exist and be within a write
    /// root (creating a new file in a granted directory).
    pub fn check_write(&self, target: &std::path::Path) -> Result<PathBuf, GrantViolation> {
        path::resolve_within(target, self.write_roots(), PathKind::Write)
    }
}
