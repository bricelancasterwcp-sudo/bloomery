//! The patch body codec (Task 3): decoding a `patch` verb's raw body into a
//! validated [`PatchBody`], and applying a validated body to the current
//! file contents.
//!
//! Two grammars are supported, selected by [`PatchCodec`]:
//! - `SearchReplace`: a robigo/assay-style conflict-marker block. `search`
//!   is required to match the current file's contents exactly once when
//!   applied — the safety rule that makes this codec robigo-safe.
//! - `WholeFile`: the entire body is the new file contents, verbatim.
//!
//! This module is pure: no I/O, no substrate. [`apply_patch`] takes and
//! returns plain strings; the daemon's task loop (Phase 3) owns reading the
//! current file and writing the applied result back.

use super::{ActionError, PatchBody, PatchCodec};

const SEARCH_MARKER: &str = "<<<<<<< SEARCH";
const DIVIDER: &str = "=======";
const REPLACE_MARKER: &str = ">>>>>>> REPLACE";

/// Decodes a `patch` verb's raw body into a [`PatchBody`] under `codec`.
///
/// `SearchReplace` expects the three markers each on their own line, in
/// order: `<<<<<<< SEARCH`, then the search text, then `=======`, then the
/// replace text, then `>>>>>>> REPLACE`. The first missing marker (checked
/// in that order) is reported.
///
/// `WholeFile` always succeeds: the entire body becomes `contents`,
/// including an empty body (a valid whole-file replacement with no
/// content).
pub fn parse_patch_body(body: &str, codec: PatchCodec) -> Result<PatchBody, ActionError> {
    match codec {
        PatchCodec::SearchReplace => parse_search_replace(body),
        PatchCodec::WholeFile => Ok(PatchBody::WholeFile {
            contents: body.to_string(),
        }),
    }
}

/// Parses the `<<<<<<< SEARCH` / `=======` / `>>>>>>> REPLACE` conflict-marker
/// grammar. Each marker must appear as its own line, in order; text before
/// the search marker, between the markers, and after the replace marker is
/// otherwise unconstrained (the search/replace text may itself be empty or
/// span multiple lines).
fn parse_search_replace(body: &str) -> Result<PatchBody, ActionError> {
    let search_line_start =
        find_line(body, SEARCH_MARKER).ok_or(ActionError::PatchNoSearchMarker {
            expected: SEARCH_MARKER,
        })?;
    let after_search = search_line_start + SEARCH_MARKER.len();
    // Skip the newline that ends the marker line, if present.
    let search_start =
        after_search.saturating_add(usize::from(body[after_search..].starts_with('\n')));

    let divider_line_start = find_line(&body[search_start..], DIVIDER)
        .map(|rel| search_start + rel)
        .ok_or(ActionError::PatchNoDivider { expected: DIVIDER })?;
    let search = &body[search_start..divider_line_start];
    let search = search.strip_suffix('\n').unwrap_or(search);

    let after_divider = divider_line_start + DIVIDER.len();
    let replace_start =
        after_divider.saturating_add(usize::from(body[after_divider..].starts_with('\n')));

    let replace_marker_start = find_line(&body[replace_start..], REPLACE_MARKER)
        .map(|rel| replace_start + rel)
        .ok_or(ActionError::PatchNoReplaceMarker {
            expected: REPLACE_MARKER,
        })?;
    let replace = &body[replace_start..replace_marker_start];
    let replace = replace.strip_suffix('\n').unwrap_or(replace);

    Ok(PatchBody::SearchReplace {
        search: search.to_string(),
        replace: replace.to_string(),
    })
}

/// Finds the byte offset where `marker` appears as a complete line — i.e.
/// starting at the beginning of `text` or immediately after a `\n`, and
/// ending at the end of `text` or immediately before a `\n`. Returns the
/// first such match.
fn find_line(text: &str, marker: &str) -> Option<usize> {
    text.match_indices(marker).find_map(|(start, _)| {
        let line_start = start == 0 || text[..start].ends_with('\n');
        let end = start + marker.len();
        let line_end = end == text.len() || text[end..].starts_with('\n');
        (line_start && line_end).then_some(start)
    })
}

/// Everything that can go wrong applying an already-validated [`PatchBody`]
/// to the current file's contents. Distinct from [`ActionError`]: parsing a
/// patch body can succeed while applying it still fails, because
/// application depends on the current file's contents, not just the body's
/// own grammar.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub enum PatchApplyError {
    /// `SearchReplace`: `search` did not appear in the current contents.
    SearchNotFound { search: String },
    /// `SearchReplace`: `search` appeared more than once — ambiguous which
    /// occurrence to replace, so the patch is rejected rather than guessed.
    SearchNotUnique { search: String, occurrences: usize },
}

/// Applies a validated `body` to `current`, returning the new contents.
///
/// `SearchReplace` requires `search` to appear in `current` exactly once
/// (robigo's safety rule against ambiguous or silently-missed edits): zero
/// occurrences is [`PatchApplyError::SearchNotFound`], two or more is
/// [`PatchApplyError::SearchNotUnique`]. `WholeFile` always applies,
/// returning `contents` verbatim.
pub fn apply_patch(current: &str, body: &PatchBody) -> Result<String, PatchApplyError> {
    match body {
        PatchBody::SearchReplace { search, replace } => {
            let occurrences = current.matches(search.as_str()).count();
            match occurrences {
                0 => Err(PatchApplyError::SearchNotFound {
                    search: search.clone(),
                }),
                1 => Ok(current.replacen(search.as_str(), replace, 1)),
                occurrences => Err(PatchApplyError::SearchNotUnique {
                    search: search.clone(),
                    occurrences,
                }),
            }
        }
        PatchBody::WholeFile { contents } => Ok(contents.clone()),
    }
}
