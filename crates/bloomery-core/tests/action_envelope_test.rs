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
