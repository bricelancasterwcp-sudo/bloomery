//! The verb card (Task 5): the static human-readable verb reference shown to
//! the model each turn. Pure: builds a `String`, no I/O, no substrate.
//!
//! P3's prompt renderer includes this verbatim ahead of every turn so the
//! model always sees the current verb grammar and the exactly-one-action
//! rule. The `patch` verb's worked example follows whichever [`PatchCodec`]
//! the caller passes — P1 always passes the model profile's configured
//! codec; P4 is what actually *selects* that codec per model, this module
//! just renders whichever one it's given.

use super::PatchCodec;

/// A worked `patch` example under the `SearchReplace` codec: the
/// conflict-marker block from [`super::patch`], `<<<<<<< SEARCH` /
/// `=======` / `>>>>>>> REPLACE`.
const SEARCH_REPLACE_PATCH_EXAMPLE: &str = r#"<action verb="patch" path="src/lib.rs">
<<<<<<< SEARCH
fn greeting() -> &'static str { "hi" }
=======
fn greeting() -> &'static str { "hello" }
>>>>>>> REPLACE
</action>"#;

/// A worked `patch` example under the `WholeFile` codec: the body is the
/// file's entire new contents, verbatim, with no conflict markers.
const WHOLE_FILE_PATCH_EXAMPLE: &str = r#"<action verb="patch" path="src/lib.rs">
fn greeting() -> &'static str { "hello" }
</action>"#;

/// Builds the human-readable verb reference: a heading, the exactly-one-
/// action rule, and one worked `<action>` example per verb (`read`, `find`,
/// `patch`, `run`, `done`). The `patch` example's body follows whichever
/// grammar `patch_codec` selects — `SearchReplace` shows the conflict-marker
/// block; `WholeFile` shows a plain replacement body with no markers.
pub fn verb_card(patch_codec: PatchCodec) -> String {
    let patch_example = match patch_codec {
        PatchCodec::SearchReplace => SEARCH_REPLACE_PATCH_EXAMPLE,
        PatchCodec::WholeFile => WHOLE_FILE_PATCH_EXAMPLE,
    };

    format!(
        r#"# Action verbs

Exactly one action per turn: exactly one action block from the five below,
nothing more. Narration before it is fine; a second action block in the same
turn is a single MultipleActions error (not applied piecemeal), and no
action block at all is NoAction.

## read — read a file, optionally a line range
<action verb="read" path="src/lib.rs" lines="1-40">
</action>

## find — search a path with a regex pattern
<action verb="find" pattern="fn \w+" path="src">
</action>

## patch — replace part or all of a file's contents
{patch_example}

## run — execute a command; the body is a JSON array of argv strings
<action verb="run">
["cargo", "test"]
</action>

## done — end the task with a summary
<action verb="done">
fixed the failing test
</action>
"#
    )
}
