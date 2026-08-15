//! The coding-agent action codec (Phase 2b/2c).
//!
//! A model turn carries at most one `<action verb="..." ...>...</action>`
//! envelope; this tree turns that envelope into a typed [`Action`] the
//! daemon's task loop (Phase 3) can execute. Task 1 (this file plus
//! [`envelope`]) owns the envelope scanner and the full `Action`/
//! `ActionError` vocabulary; verb validation is Task 2, the patch body codec
//! is Task 3, the applies-and-parses landing lens ([`lens`]) is Task 4, and
//! the human-readable verb reference shown to the model each turn
//! ([`card::verb_card`]) is Task 5. Pure, GPU-free: no I/O, no substrate.

pub mod card;
pub mod envelope;
pub mod lens;
pub mod patch;
pub mod verbs;

pub use card::{verb_card, verb_card_for};
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

/// A validated patch body, decoded under whichever [`PatchCodec`] the caller
/// selected.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub enum PatchBody {
    /// A robigo/assay-style conflict-marker block: `search` must match the
    /// current file's contents exactly once (enforced by
    /// [`patch::apply_patch`]); `replace` is what takes its place.
    SearchReplace { search: String, replace: String },
    /// The entire body is the file's new contents, verbatim.
    WholeFile { contents: String },
}

/// Selects which grammar a `patch` verb's body is decoded under. P3 passes
/// the model profile's configured codec; [`parse_action`] defaults to
/// [`PatchCodec::SearchReplace`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum PatchCodec {
    SearchReplace,
    WholeFile,
}

/// Everything that can go wrong turning a model turn into a validated
/// [`Action`]. Declared in full now (Task 1) even though most variants are
/// only produced starting Task 2 (`BadRange`..`BadArgv`) and Task 3
/// (`PatchNoSearchMarker`..`PatchNoReplaceMarker`), so later tasks reference
/// this enum rather than redeclare it.
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
}

/// The complete set of recognized `verb="..."` values.
pub const VERBS: &[&str] = &["read", "find", "patch", "run", "done"];

/// Scan + validate one action end to end, decoding a `patch` verb's body
/// under `patch_codec`. This is the entry point P3's task loop calls,
/// passing the model profile's configured codec.
pub fn parse_action_with_codec(turn: &str, patch_codec: PatchCodec) -> Result<Action, ActionError> {
    let raw = scan_envelope(turn)?;

    match raw.verb.as_str() {
        "read" => validate_read(&raw),
        "find" => validate_find(&raw),
        "run" => validate_run(&raw),
        "done" => validate_done(&raw),
        "patch" => {
            let path = raw
                .attrs
                .get("path")
                .cloned()
                .ok_or(ActionError::MissingAttr {
                    verb: "patch",
                    attr: "path",
                })?;
            let body = patch::parse_patch_body(&raw.body, patch_codec)?;
            Ok(Action::Patch { path, body })
        }
        verb => Err(ActionError::UnknownVerb {
            verb: verb.to_string(),
            expected: VERBS,
        }),
    }
}

/// Scan + validate one action end to end, defaulting a `patch` verb's body
/// to the [`PatchCodec::SearchReplace`] grammar.
pub fn parse_action(turn: &str) -> Result<Action, ActionError> {
    parse_action_with_codec(turn, PatchCodec::SearchReplace)
}
