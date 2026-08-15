//! The envelope scanner: finds the single `<action ...>...</action>` block
//! in a model turn and lifts it into an unvalidated [`RawAction`]. Verb
//! validation and attribute-specific parsing (ranges, regexes, argv, patch
//! bodies) are later P1 tasks' job — this module only implements the
//! envelope grammar itself.

use super::ActionError;
use regex::Regex;
use std::collections::BTreeMap;

const OPEN_TAG: &str = "<action";
const CLOSE_TAG: &str = "</action>";

/// The raw, un-validated contents of the single `<action>` block.
#[derive(Debug, Clone, PartialEq)]
pub struct RawAction {
    pub verb: String,
    pub attrs: BTreeMap<String, String>,
    /// Exactly the bytes between the opening tag's `>` and `</action>`,
    /// trimmed of one leading/trailing newline only.
    pub body: String,
}

/// Scans a model turn for exactly one `<action ...>...</action>` block.
///
/// Envelope grammar: the block opens with `<action` followed by
/// whitespace-separated `key="value"` attrs (double-quoted; a value may
/// contain anything but `"`) and a `>`, then arbitrary body bytes, then
/// `</action>`. Attributes parse into `attrs`; `verb` is the mandatory
/// `verb="…"` attr lifted out of that map into its own field. Prose outside
/// the block is ignored. Zero blocks is [`ActionError::NoAction`]; two or
/// more opening `<action` tags is [`ActionError::MultipleActions`].
pub fn scan_envelope(turn: &str) -> Result<RawAction, ActionError> {
    let found = turn.matches(OPEN_TAG).count();
    if found == 0 {
        return Err(ActionError::NoAction);
    }
    if found >= 2 {
        return Err(ActionError::MultipleActions { found });
    }

    // Exactly one `<action` in the turn from here on, so any malformed
    // structure (no closing `>`, no `</action>`) means the turn doesn't
    // actually contain a well-formed block — treat that the same as no
    // block found rather than inventing an unspecified error variant.
    let open_start = turn.find(OPEN_TAG).ok_or(ActionError::NoAction)?;
    let attrs_start = open_start + OPEN_TAG.len();
    let tag_close = turn[attrs_start..]
        .find('>')
        .map(|offset| attrs_start + offset)
        .ok_or(ActionError::NoAction)?;

    let body_start = tag_close + 1;
    let close_start = turn[body_start..]
        .find(CLOSE_TAG)
        .map(|offset| body_start + offset)
        .ok_or(ActionError::NoAction)?;

    let mut attrs = parse_attrs(&turn[attrs_start..tag_close]);
    let verb = attrs.remove("verb").ok_or(ActionError::MissingAttr {
        verb: "action",
        attr: "verb",
    })?;
    let body = trim_one_framing_newline(&turn[body_start..close_start]).to_string();

    Ok(RawAction { verb, attrs, body })
}

/// Parses whitespace-separated `key="value"` pairs (value may contain
/// anything but a double quote) into a deterministically-ordered map.
fn parse_attrs(attrs_str: &str) -> BTreeMap<String, String> {
    let re = Regex::new(r#"(?P<key>[A-Za-z0-9_]+)="(?P<value>[^"]*)""#)
        .expect("attr regex is a fixed valid pattern");
    re.captures_iter(attrs_str)
        .map(|caps| (caps["key"].to_string(), caps["value"].to_string()))
        .collect()
}

/// Strips exactly one leading `\n` and one trailing `\n`, if present —
/// nothing else. Turns `>\n…\n</action>` framing into the inner content.
fn trim_one_framing_newline(body: &str) -> &str {
    let body = body.strip_prefix('\n').unwrap_or(body);
    body.strip_suffix('\n').unwrap_or(body)
}
