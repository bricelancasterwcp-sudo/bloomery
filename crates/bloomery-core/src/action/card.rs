//! The verb card (Task 5): the static human-readable verb reference shown to
//! the model each turn. Pure: builds a `String`, no I/O, no substrate.
//!
//! P3's prompt renderer includes this verbatim ahead of every turn so the
//! model always sees the current verb grammar and the exactly-one-action
//! rule. The `patch` verb's worked example follows whichever [`PatchCodec`]
//! the caller passes — P1 always passes the model profile's configured
//! codec; P4 is what actually *selects* that codec per model, this module
//! just renders whichever one it's given.
//!
//! P4 Task 7 adds the read-only variant ([`verb_card_for`] with
//! `mutating: false`): gate G4's structural demotion
//! (`docs/superpowers/evidence/2026-08-15-g4-protocol.md` §6 — "a read-only
//! verb card AND a structural dispatch refusal ... prompting alone is not
//! enforcement") is only half a structural gate if the model is still shown
//! `patch`/`run` worked examples it can never actually use; the read-only
//! card drops both sections entirely and says so.

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

/// The pinned gate-G4 read-only notice (Task 7 brief — exact bytes; this
/// string is wire/prompt content the model reads verbatim).
const READ_ONLY_NOTICE: &str =
    "patch and run are not available in this task (this model is read-only under gate G4)";

const READ_FIND_DONE_SECTIONS: &str = r#"## read — read a file, optionally a line range
<action verb="read" path="src/lib.rs" lines="1-40">
</action>

## find — search a path with a regex pattern
<action verb="find" pattern="fn \w+" path="src">
</action>

## done — end the task with a summary
<action verb="done">
fixed the failing test
</action>"#;

/// Builds the human-readable verb reference: a heading, the exactly-one-
/// action rule, and one worked `<action>` example per available verb.
///
/// When `mutating` is `true`, all five verbs (`read`, `find`, `patch`,
/// `run`, `done`) are shown — the `patch` example's body follows whichever
/// grammar `patch_codec` selects, `SearchReplace` shows the conflict-marker
/// block, `WholeFile` shows a plain replacement body with no markers.
///
/// When `mutating` is `false` (gate G4 demotion — see this module's docs),
/// only `read`, `find`, and `done` are shown; `patch_codec` is unused in
/// that branch (there is no patch example to render), and the card instead
/// carries the pinned [`READ_ONLY_NOTICE`] line explaining why `patch` and
/// `run` are absent, structurally rather than just by omission — the loop's
/// own dispatch gate (`bloomery-daemon`'s `run_task`) is what actually
/// refuses those verbs even if a model tries them anyway.
pub fn verb_card_for(patch_codec: PatchCodec, mutating: bool) -> String {
    if !mutating {
        return format!(
            r#"# Action verbs

Exactly one action per turn: exactly one action block from the three below,
nothing more. Narration before it is fine; a second action block in the same
turn is a single MultipleActions error (not applied piecemeal), and no
action block at all is NoAction.

{READ_FIND_DONE_SECTIONS}

{READ_ONLY_NOTICE}
"#
        );
    }

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

/// The always-mutating verb card — equivalent to `verb_card_for(c, true)`.
/// Kept as a thin wrapper so P1/P3 call sites that never think about
/// demotion are unaffected by Task 7.
pub fn verb_card(patch_codec: PatchCodec) -> String {
    verb_card_for(patch_codec, true)
}
