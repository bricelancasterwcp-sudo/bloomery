use bloomery_core::action::{scan_envelope, ActionError};

#[test]
fn scans_a_single_block_ignoring_prose() {
    let turn =
        "I'll read the file.\n<action verb=\"read\" path=\"src/a.rs\">\n</action>\ndone thinking";
    let raw = scan_envelope(turn).unwrap();
    assert_eq!(raw.verb, "read");
    assert_eq!(raw.attrs.get("path").map(String::as_str), Some("src/a.rs"));
    assert_eq!(raw.body, "");
}

#[test]
fn body_bytes_are_preserved_minus_one_framing_newline() {
    let turn = "<action verb=\"patch\" path=\"p\">\nline1\nline2\n</action>";
    let raw = scan_envelope(turn).unwrap();
    assert_eq!(raw.body, "line1\nline2");
}

#[test]
fn no_block_is_no_action() {
    assert_eq!(
        scan_envelope("just talking, no action here"),
        Err(ActionError::NoAction)
    );
}

#[test]
fn two_blocks_is_multiple_actions() {
    let turn = "<action verb=\"read\" path=\"a\"></action><action verb=\"done\">x</action>";
    assert_eq!(
        scan_envelope(turn),
        Err(ActionError::MultipleActions { found: 2 })
    );
}

#[test]
fn a_block_without_verb_attr_is_named() {
    let turn = "<action path=\"a\">\n</action>";
    assert_eq!(
        scan_envelope(turn),
        Err(ActionError::MissingAttr {
            verb: "action",
            attr: "verb"
        })
    );
}

/// Adversarial (binding, from code review finding #1 — Critical): the
/// grammar permits `>` inside a quoted attribute value. A naive "first `>`
/// closes the tag" scan would cut the tag off inside the `pattern` value's
/// `Result<T>`, silently losing the `path` attr and leaving the rest of the
/// tag text as leftover "body" — with no error raised. The scanner must be
/// quote-aware so a `>` (or `->`) inside a quoted value doesn't end the tag.
#[test]
fn quoted_attr_value_containing_angle_brackets_does_not_truncate_the_tag() {
    let turn =
        "<action verb=\"find\" pattern=\"fn \\w+\\(.*\\) -> Result<T>\" path=\"src/lib.rs\"></action>";
    let raw = scan_envelope(turn).unwrap();
    assert_eq!(raw.verb, "find");
    assert_eq!(
        raw.attrs.get("pattern").map(String::as_str),
        Some("fn \\w+\\(.*\\) -> Result<T>")
    );
    assert_eq!(
        raw.attrs.get("path").map(String::as_str),
        Some("src/lib.rs")
    );
    assert_eq!(raw.body, "");
}

/// Adversarial (binding, from code review finding #2a — Important): a
/// patch body can legitimately contain the literal text of an `<action>`
/// tag (e.g. editing a file that itself defines this grammar). A naive
/// substring count of `<action` over the whole turn would misreport this
/// as two blocks. The nested tag is body content of the one real top-level
/// block, not a sibling — it must parse as a single Patch action with the
/// nested text preserved verbatim in the body.
#[test]
fn nested_action_text_inside_a_body_is_not_multiple_actions() {
    let turn = "<action verb=\"patch\" path=\"p\">\nbefore\n<action verb=\"x\">nested</action>\nafter\n</action>";
    let raw = scan_envelope(turn).unwrap();
    assert_eq!(raw.verb, "patch");
    assert_eq!(
        raw.body,
        "before\n<action verb=\"x\">nested</action>\nafter"
    );
}

// Adversarial case 2b ("two genuine top-level blocks still report
// MultipleActions{found:2}") is already covered verbatim above by
// `two_blocks_is_multiple_actions` — no separate test needed.
