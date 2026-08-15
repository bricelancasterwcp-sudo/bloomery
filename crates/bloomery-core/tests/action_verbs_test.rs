use bloomery_core::action::{parse_action, Action, ActionError};

fn wrap(inner: &str) -> String {
    format!("<action {inner}</action>")
}

#[test]
fn read_with_valid_range() {
    let a = parse_action(&wrap("verb=\"read\" path=\"src/a.rs\" lines=\"10-20\">\n")).unwrap();
    assert_eq!(
        a,
        Action::Read {
            path: "src/a.rs".into(),
            lines: Some((10, 20))
        }
    );
}

#[test]
fn read_without_lines_is_whole_file() {
    let a = parse_action(&wrap("verb=\"read\" path=\"p\">\n")).unwrap();
    assert_eq!(
        a,
        Action::Read {
            path: "p".into(),
            lines: None
        }
    );
}

#[test]
fn read_inverted_range_is_named_with_expected_shape() {
    let e = parse_action(&wrap("verb=\"read\" path=\"p\" lines=\"20-10\">\n")).unwrap_err();
    match e {
        ActionError::BadRange { got, expected } => {
            assert_eq!(got, "20-10");
            assert!(expected.contains("A ≤ B"));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn find_requires_a_compiling_regex() {
    let e = parse_action(&wrap("verb=\"find\" pattern=\"(unclosed\" path=\"src\">\n")).unwrap_err();
    assert!(matches!(e, ActionError::BadRegex { .. }));
    let ok = parse_action(&wrap("verb=\"find\" pattern=\"fn \\w+\" path=\"src\">\n")).unwrap();
    assert_eq!(
        ok,
        Action::Find {
            pattern: "fn \\w+".into(),
            path: "src".into()
        }
    );
}

#[test]
fn run_parses_a_json_argv_array() {
    let a = parse_action(&wrap("verb=\"run\">\n[\"cargo\", \"test\"]\n")).unwrap();
    assert_eq!(
        a,
        Action::Run {
            argv: vec!["cargo".into(), "test".into()]
        }
    );
}

#[test]
fn run_rejects_non_array_body_with_expected_shape() {
    let e = parse_action(&wrap("verb=\"run\">\ncargo test\n")).unwrap_err();
    match e {
        ActionError::BadArgv { expected, .. } => assert!(expected.contains("JSON array")),
        other => panic!("{other:?}"),
    }
}

#[test]
fn done_needs_a_summary() {
    assert_eq!(
        parse_action(&wrap("verb=\"done\">\nfixed the bug\n")).unwrap(),
        Action::Done {
            summary: "fixed the bug".into()
        }
    );
    assert!(matches!(
        parse_action(&wrap("verb=\"done\">\n   \n")).unwrap_err(),
        ActionError::EmptyBody { .. }
    ));
}

#[test]
fn unknown_verb_lists_the_expected_set() {
    let e = parse_action(&wrap("verb=\"delete\" path=\"p\">\n")).unwrap_err();
    match e {
        ActionError::UnknownVerb { verb, expected } => {
            assert_eq!(verb, "delete");
            assert!(expected.contains(&"read") && expected.contains(&"done"));
        }
        other => panic!("{other:?}"),
    }
}
