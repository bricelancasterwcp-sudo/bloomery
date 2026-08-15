//! The envelope scanner: finds the single `<action ...>...</action>` block
//! in a model turn and lifts it into an unvalidated [`RawAction`]. Verb
//! validation and attribute-specific parsing (ranges, regexes, argv, patch
//! bodies) are later P1 tasks' job — this module only implements the
//! envelope grammar itself.
//!
//! The turn is untrusted model output, so the scanner is written to be
//! robust against grammar-legal content that a naive substring scan would
//! misparse: a `>` inside a quoted attribute value, and a `<action` that
//! merely *appears* inside another block's body text.

use super::ActionError;
use regex::Regex;
use std::collections::BTreeMap;
use std::sync::LazyLock;

/// Matches whitespace-separated `key="value"` attr pairs — compiled once
/// and reused across every [`parse_attrs`] call rather than recompiled per
/// call.
static ATTR_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?P<key>[A-Za-z0-9_]+)="(?P<value>[^"]*)""#)
        .expect("attr regex is a fixed valid pattern")
});

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

/// One `<action>` block's byte spans within the turn, located by
/// [`scan_top_level_blocks`].
struct BlockSpan {
    attrs_start: usize,
    tag_close: usize,
    body_start: usize,
    close_start: usize,
}

/// Scans a model turn for exactly one `<action ...>...</action>` block.
///
/// Envelope grammar: the block opens with `<action` followed by
/// whitespace-separated `key="value"` attrs (double-quoted; a value may
/// contain anything but `"` — including `>`) and a `>`, then body bytes,
/// then `</action>`. Attributes parse into `attrs`; `verb` is the mandatory
/// `verb="…"` attr lifted out of that map into its own field. Well-formed
/// top-level blocks parse correctly regardless of surrounding prose, and a
/// body parses correctly as long as it doesn't itself contain a stray
/// `<action` literal (see the known limitation below). Zero well-formed
/// top-level blocks is [`ActionError::NoAction`]; two or more is
/// [`ActionError::MultipleActions`].
///
/// "Top-level" excludes a `<action` that merely appears as literal text
/// inside another block's body (e.g. a patch touching a file that mentions
/// the tag, or a done-summary quoting it) — only tags outside every
/// already-open block's body count toward the multiple-blocks check. This
/// only works, though, when that embedded `<action` is itself part of a
/// well-formed nested block (paired with its own `</action>`); see below.
///
/// **Known limitation:** an *unpaired* or otherwise malformed `<action`
/// literal — one that never resolves to a well-formed block, whether it
/// sits in prose outside every block or inside a block's own body — stops
/// [`scan_top_level_blocks`] at that point (see its doc comment), which can
/// surface as [`ActionError::NoAction`] even though a well-formed block was
/// present earlier in the turn. Turns and bodies that don't contain a stray
/// `<action` literal are unaffected. This is an accepted gap in today's
/// scanner, not a claim that stray literals are handled; hardening the scan
/// to recover from them is tracked as a P3-era follow-up ("envelope
/// stray-literal hardening").
pub fn scan_envelope(turn: &str) -> Result<RawAction, ActionError> {
    let blocks = scan_top_level_blocks(turn);

    match blocks.len() {
        0 => Err(ActionError::NoAction),
        1 => {
            let block = &blocks[0];
            let mut attrs = parse_attrs(&turn[block.attrs_start..block.tag_close]);
            let verb = attrs.remove("verb").ok_or(ActionError::MissingAttr {
                verb: "action",
                attr: "verb",
            })?;
            let body =
                trim_one_framing_newline(&turn[block.body_start..block.close_start]).to_string();
            Ok(RawAction { verb, attrs, body })
        }
        found => Err(ActionError::MultipleActions { found }),
    }
}

/// Walks the turn left to right, identifying every well-formed top-level
/// `<action>...</action>` block: each block's opening tag is closed with a
/// quote-aware scan (so a `>` inside a quoted attr value doesn't end the
/// tag early), and its body is scanned for the *matching* `</action>` via
/// nesting depth (so a literal `<action>...</action>` embedded in the body
/// doesn't end the block early either). Scanning resumes strictly after
/// each found block's `</action>`, so nested tags are never recounted as
/// siblings.
///
/// A `<action` that never resolves to a well-formed block (no unquoted `>`
/// to close the opening tag, or no matching `</action>`) stops the scan at
/// that point — the malformed remainder is treated as ordinary trailing
/// prose rather than surfacing a new error variant, matching this task's
/// documented NoAction-fallback rule.
fn scan_top_level_blocks(turn: &str) -> Vec<BlockSpan> {
    let mut blocks = Vec::new();
    let mut pos = 0usize;

    while let Some(open_rel) = turn[pos..].find(OPEN_TAG) {
        let open_start = pos + open_rel;
        let attrs_start = open_start + OPEN_TAG.len();

        let Some(tag_close_rel) = find_unquoted_gt(&turn[attrs_start..]) else {
            break;
        };
        let tag_close = attrs_start + tag_close_rel;
        let body_start = tag_close + 1;

        let Some(close_rel) = find_matching_close(&turn[body_start..]) else {
            break;
        };
        let close_start = body_start + close_rel;

        blocks.push(BlockSpan {
            attrs_start,
            tag_close,
            body_start,
            close_start,
        });
        pos = close_start + CLOSE_TAG.len();
    }

    blocks
}

/// Finds the offset of the `>` that closes an opening tag's attribute list,
/// treating any `>` seen while inside a double-quoted attribute value as
/// ordinary text (the grammar permits anything but `"` inside a quoted
/// value). Returns the offset of the first `>` seen while not inside
/// quotes. A malformed tail with an odd number of `"` before the true close
/// can, in principle, misread a later `>` — an accepted limitation for
/// grammar-illegal input, since the grammar guarantees balanced quotes.
fn find_unquoted_gt(text: &str) -> Option<usize> {
    let mut in_quotes = false;
    for (i, ch) in text.char_indices() {
        match ch {
            '"' => in_quotes = !in_quotes,
            '>' if !in_quotes => return Some(i),
            _ => {}
        }
    }
    None
}

/// Given `text` starting immediately after an opening tag's `>`, finds the
/// byte offset of the `</action>` that matches that opening tag. Any
/// literal `<action` / `</action>` pair fully contained in the body is
/// nesting depth, not the block's own close: depth starts at 1 (already
/// inside the outer block); each further `<action` bumps it, each
/// `</action>` drops it, and the `</action>` that brings depth back to 0 is
/// the match. Returns `None` if the depth never returns to 0 (unbalanced).
fn find_matching_close(text: &str) -> Option<usize> {
    let mut depth = 1i32;
    let mut pos = 0usize;
    loop {
        let next_open = text[pos..].find(OPEN_TAG);
        let next_close = text[pos..].find(CLOSE_TAG);
        match (next_open, next_close) {
            (Some(o), Some(c)) if o < c => {
                depth += 1;
                pos += o + OPEN_TAG.len();
            }
            (_, Some(c)) => {
                let close_abs = pos + c;
                depth -= 1;
                if depth == 0 {
                    return Some(close_abs);
                }
                pos = close_abs + CLOSE_TAG.len();
            }
            _ => return None,
        }
    }
}

/// Parses whitespace-separated `key="value"` pairs (value may contain
/// anything but a double quote) into a deterministically-ordered map. If a
/// key appears more than once, the last occurrence wins (later entries
/// overwrite earlier ones as the map is built) — this is by design, not an
/// oversight, since the grammar does not forbid repeated attrs.
fn parse_attrs(attrs_str: &str) -> BTreeMap<String, String> {
    ATTR_RE
        .captures_iter(attrs_str)
        .map(|caps| (caps["key"].to_string(), caps["value"].to_string()))
        .collect()
}

/// Strips exactly one leading `\n` and one trailing `\n`, if present —
/// nothing else. Turns `>\n…\n</action>` framing into the inner content.
fn trim_one_framing_newline(body: &str) -> &str {
    let body = body.strip_prefix('\n').unwrap_or(body);
    body.strip_suffix('\n').unwrap_or(body)
}
