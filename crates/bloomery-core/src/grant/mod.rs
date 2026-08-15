//! Grant capability scoping for v1 execution security.
//!
//! The Grant type represents a capability grant: a JSON-specified set of filesystem
//! roots (read/write), commands (allowed prefixes), and network access.
//!
//! **The headline property:** grants are immutable for a task's life, accepted
//! once at task creation and never widened afterward, and every check here is
//! **structural** — it takes a path or argv and a `Grant`, never file content,
//! model instructions, or persuasive text. So the worst-case successful
//! injection spends the task's own budget inside its own grants: a laced file
//! can be *read* (if it's in a read root) and a model can be talked into
//! *trying* to obey it, but the attempt to read outside the roots or run an
//! unlisted command is refused before anything happens, regardless of how
//! convincing the text was. `tests/grant_redteam_test.rs` (Task 4) is the
//! adversarial proof of this: it builds a real sandbox containing an
//! injection-laced file and a `/`-pointing symlink, and asserts every classic
//! escape (absolute path, `..` traversal, symlink-to-root, exfil commands)
//! is refused structurally.
//!
//! **The honest boundary — read before trusting this further:** v1 is
//! grant-*scoping*, not OS-level sandboxing. There is no namespace, cgroup,
//! or seccomp isolation; a granted `run` command executes as a normal
//! subprocess with normal process privileges, and a granted command prefix
//! is *trusted* to be non-networking — `network: false` is enforced by
//! refusing the grant field, not by cutting subprocess network access at the
//! kernel level. If an operator grants `["curl", "https://example.com"]`,
//! that command runs and talks to the network; the grant boundary is a
//! contract about what the model can attempt, not a kernel-enforced sandbox
//! around what a granted command can do once it runs. That is the actual
//! boundary, stated here rather than overclaimed. Tasks 2–3 validate that
//! operations (path access, command execution) comply with the grant; P3
//! wires these checks into the task-loop's executors as the enforcement
//! point, and `tasks_enabled` (default `false`) gates the whole task
//! surface off until that wiring lands.

pub mod command;
pub mod path;

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Wire format for [`Grant`]: the raw, unvalidated shape deserialized from
/// JSON before validation runs.
///
/// This is the *only* type serde ever deserializes directly. `Grant` itself
/// deserializes via `#[serde(try_from = "GrantWire")]`, so every
/// deserialization path — `Grant::from_json` and any other call that
/// deserializes a `Grant` (directly or nested in a larger structure, e.g. a
/// task-request body) — is forced through [`Grant::try_from`] and its
/// validation. There is no way to obtain a `Grant` that skipped validation.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GrantWire {
    read_roots: Vec<PathBuf>,
    write_roots: Vec<PathBuf>,
    commands: Vec<Vec<String>>,
    #[serde(default)]
    network: bool,
}

/// A capability grant: read/write filesystem roots, allowed command prefixes, and network access.
///
/// All fields are private. Every construction path funnels through
/// `TryFrom<GrantWire> for Grant`: `from_json` parses to `GrantWire` then
/// calls `Grant::try_from` directly (so callers keep the exact `GrantError`
/// variant), and `#[serde(try_from = "GrantWire")]` routes the derived
/// `Deserialize` impl — used by any other deserialization path, e.g.
/// `serde_json::from_str::<Grant>(..)` or a `Grant` field nested in a larger
/// deserialized struct — through the same `TryFrom::try_from`. No path
/// yields an unvalidated `Grant`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "GrantWire")]
pub struct Grant {
    read_roots: Vec<PathBuf>,
    write_roots: Vec<PathBuf>,
    commands: Vec<Vec<String>>,
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

impl std::fmt::Display for GrantError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GrantError::NetworkNotSupported => {
                write!(f, "network access is not supported in v1 grants")
            }
            GrantError::NonAbsoluteRoot { root } => {
                write!(f, "grant root is not absolute: {root}")
            }
            GrantError::EmptyCommandPrefix => {
                write!(f, "grant command prefix must not be empty")
            }
            GrantError::Parse(msg) => write!(f, "failed to parse grant: {msg}"),
        }
    }
}

impl std::error::Error for GrantError {}

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
    /// Parses into the private `GrantWire` intermediate first — JSON syntax
    /// errors or unknown fields become `GrantError::Parse` — then calls
    /// `Grant::try_from(wire)`, the same validation the derived
    /// `Deserialize` impl uses. Going through `try_from` directly (rather
    /// than `serde_json::from_str::<Grant>`) means `from_json` surfaces the
    /// exact `GrantError` variant (`NetworkNotSupported` /
    /// `NonAbsoluteRoot` / `EmptyCommandPrefix`) instead of a generic
    /// parse-wrapped string, preserving its existing error contract.
    ///
    /// Validation order (in [`Grant::try_from`]):
    /// 1. Parse JSON into `GrantWire` (failure → `Parse`)
    /// 2. If `network == true` → `NetworkNotSupported`
    /// 3. Any read/write root where `!is_absolute()` → `NonAbsoluteRoot`
    /// 4. Any commands entry that is empty → `EmptyCommandPrefix`
    ///
    /// An empty commands *list* is valid (no commands granted).
    /// Only an empty commands *prefix* (empty Vec<String>) is rejected.
    pub fn from_json(s: &str) -> Result<Grant, GrantError> {
        let wire: GrantWire =
            serde_json::from_str(s).map_err(|e| GrantError::Parse(e.to_string()))?;
        Grant::try_from(wire)
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

    /// `argv` (the run action's exec vector) must start with one granted
    /// prefix, element-wise. It may append arguments but must not change or
    /// reorder the prefix. No shell interpretation — argv is exec'd directly.
    pub fn check_command(&self, argv: &[String]) -> Result<(), GrantViolation> {
        command::check_command(&self.commands, argv)
    }
}

/// The single validation gate for every `Grant` construction path.
///
/// `#[serde(try_from = "GrantWire")]` on `Grant` routes the derived
/// `Deserialize` impl through here, so `serde_json::from_str::<Grant>(..)`
/// (and any deserialization of a `Grant` nested in a larger struct, such as
/// a task-request body's `grants` field) validates exactly as `from_json`
/// does. `from_json` also calls this directly, after parsing to `GrantWire`
/// itself, so it can return the precise `GrantError` variant rather than a
/// serde-wrapped string.
impl std::convert::TryFrom<GrantWire> for Grant {
    type Error = GrantError;

    fn try_from(wire: GrantWire) -> Result<Self, GrantError> {
        // Step 1: Check network.
        if wire.network {
            return Err(GrantError::NetworkNotSupported);
        }

        // Step 2: Check absolute roots (read_roots first, then write_roots).
        if let Some(root) = Grant::first_non_absolute(&wire.read_roots) {
            return Err(GrantError::NonAbsoluteRoot { root });
        }
        if let Some(root) = Grant::first_non_absolute(&wire.write_roots) {
            return Err(GrantError::NonAbsoluteRoot { root });
        }

        // Step 3: Check for empty command prefixes.
        for cmd in &wire.commands {
            if cmd.is_empty() {
                return Err(GrantError::EmptyCommandPrefix);
            }
        }

        Ok(Grant {
            read_roots: wire.read_roots,
            write_roots: wire.write_roots,
            commands: wire.commands,
            network: wire.network,
        })
    }
}
