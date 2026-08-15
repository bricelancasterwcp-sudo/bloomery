//! The coding-agent action codec (Phase 2b/2c).
//!
//! A model turn carries at most one `<action verb="..." ...>...</action>`
//! envelope; this tree turns that envelope into a typed [`Action`] the
//! daemon's task loop (Phase 3) can execute. Task 1 (this file plus
//! [`envelope`]) owns the envelope scanner and the full `Action`/
//! `ActionError` vocabulary; later P1 tasks own verb validation (Task 2)
//! and the patch body codec (Task 3). Pure, GPU-free: no I/O, no substrate.

pub mod envelope;
pub mod verbs;

pub use envelope::{scan_envelope, RawAction};
pub use verbs::{validate_done, validate_find, validate_read, validate_run};

/// A single validated action a coding agent may take, decoded from one
/// `<action>` envelope.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub enum Action {
    Read {
        path: String,
        lines: Option<(u32, u32)>,
    },
    Find {
        pattern: String,
        path: String,
    },
    Patch {
        path: String,
        body: PatchBody,
    },
    Run {
        argv: Vec<String>,
    },
    Done {
        summary: String,
    },
}

/// Temporary stand-in for the patch body codec. Task 3 defines the real
/// `PatchBody` (search/divider/replace markers, per the envelope grammar's
/// patch-specific rules) and replaces this stub; kept minimal here purely so
/// `Action::Patch` has a concrete, constructible field type in Task 1.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub enum PatchBody {
    // Task 3 replaces this with the real search/replace payload.
    Unset,
}

/// Everything that can go wrong turning a model turn into a validated
/// [`Action`]. Declared in full now (Task 1) even though most variants are
/// only produced starting Task 2 (`BadRange`..`BadArgv`) and Task 3
/// (`PatchNoSearchMarker`..`BadCodec`), so later tasks reference this enum
/// rather than redeclare it.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub enum ActionError {
    /// No `<action>` block found in the turn.
    NoAction,
    /// More than one `<action` opening tag found in the turn.
    MultipleActions { found: usize },
    /// The envelope's `verb` attr isn't one of [`VERBS`].
    UnknownVerb {
        verb: String,
        expected: &'static [&'static str],
    },
    /// A verb-specific required attribute was absent.
    MissingAttr {
        verb: &'static str,
        attr: &'static str,
    },
    /// Task 2: a `lines="a-b"` range attr failed to parse.
    BadRange { got: String, expected: &'static str },
    /// Task 2: a `find` verb's `pattern` attr is not a valid regex.
    BadRegex { pattern: String, detail: String },
    /// Task 2: a verb that requires a body got an empty one.
    EmptyBody {
        verb: &'static str,
        expected: &'static str,
    },
    /// Task 2: a `run` verb's body failed to parse into an argv.
    BadArgv {
        detail: String,
        expected: &'static str,
    },
    /// Task 3: a patch body is missing its search marker.
    PatchNoSearchMarker { expected: &'static str },
    /// Task 3: a patch body is missing the search/replace divider.
    PatchNoDivider { expected: &'static str },
    /// Task 3: a patch body is missing its replace marker.
    PatchNoReplaceMarker { expected: &'static str },
    /// Task 3: a patch body's codec fence is malformed.
    BadCodec { detail: String },
}

/// The complete set of recognized `verb="..."` values.
pub const VERBS: &[&str] = &["read", "find", "patch", "run", "done"];

/// Scan + validate one action end to end. Patch is added in Task 3;
/// until then a "patch" verb returns ActionError::BadCodec with detail "patch not wired".
pub fn parse_action(turn: &str) -> Result<Action, ActionError> {
    let raw = scan_envelope(turn)?;

    match raw.verb.as_str() {
        "read" => validate_read(&raw),
        "find" => validate_find(&raw),
        "run" => validate_run(&raw),
        "done" => validate_done(&raw),
        "patch" => Err(ActionError::BadCodec {
            detail: "patch not wired".into(),
        }),
        verb => Err(ActionError::UnknownVerb {
            verb: verb.to_string(),
            expected: VERBS,
        }),
    }
}
