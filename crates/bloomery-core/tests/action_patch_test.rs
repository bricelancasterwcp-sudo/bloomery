use bloomery_core::action::patch::{apply_patch, parse_patch_body, PatchApplyError};
use bloomery_core::action::{parse_action_with_codec, Action, ActionError, PatchBody, PatchCodec};

fn sr_block(path: &str, search: &str, replace: &str) -> String {
    format!("<action verb=\"patch\" path=\"{path}\">\n<<<<<<< SEARCH\n{search}\n=======\n{replace}\n>>>>>>> REPLACE\n</action>")
}

#[test]
fn search_replace_parses() {
    let a = parse_action_with_codec(&sr_block("f.py", "old", "new"), PatchCodec::SearchReplace)
        .unwrap();
    assert_eq!(
        a,
        Action::Patch {
            path: "f.py".into(),
            body: PatchBody::SearchReplace {
                search: "old".into(),
                replace: "new".into()
            }
        }
    );
}

#[test]
fn search_replace_missing_divider_is_named() {
    let bad =
        "<action verb=\"patch\" path=\"f\">\n<<<<<<< SEARCH\nold\nnew\n>>>>>>> REPLACE\n</action>";
    match parse_action_with_codec(bad, PatchCodec::SearchReplace).unwrap_err() {
        ActionError::PatchNoDivider { expected } => assert_eq!(expected, "======="),
        other => panic!("{other:?}"),
    }
}

#[test]
fn whole_file_takes_the_whole_body() {
    let turn = "<action verb=\"patch\" path=\"f\">\nnew contents\nline two\n</action>";
    let a = parse_action_with_codec(turn, PatchCodec::WholeFile).unwrap();
    assert_eq!(
        a,
        Action::Patch {
            path: "f".into(),
            body: PatchBody::WholeFile {
                contents: "new contents\nline two".into()
            }
        }
    );
}

#[test]
fn apply_search_replace_requires_a_unique_match() {
    let body = PatchBody::SearchReplace {
        search: "x".into(),
        replace: "y".into(),
    };
    assert_eq!(apply_patch("a x b", &body).unwrap(), "a y b");
    assert!(matches!(
        apply_patch("no match", &body),
        Err(PatchApplyError::SearchNotFound { .. })
    ));
    match apply_patch("x x", &body) {
        Err(PatchApplyError::SearchNotUnique { occurrences, .. }) => {
            assert_eq!(occurrences, 2)
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn apply_whole_file_always_replaces() {
    let body = PatchBody::WholeFile {
        contents: "brand new".into(),
    };
    assert_eq!(apply_patch("anything at all", &body).unwrap(), "brand new");
}

#[test]
fn parse_patch_body_direct_missing_search_marker() {
    match parse_patch_body("======\nx\n>>>>>>> REPLACE", PatchCodec::SearchReplace).unwrap_err() {
        ActionError::PatchNoSearchMarker { expected } => assert_eq!(expected, "<<<<<<< SEARCH"),
        other => panic!("{other:?}"),
    }
}
