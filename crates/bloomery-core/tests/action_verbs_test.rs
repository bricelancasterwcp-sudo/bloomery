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
            summary: "fixed the bug".into(),
            outcome: None,
            reason: None,
            evidence: vec![],
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

// --- envelope-v5 done declarations (turn-6 spec §3.3) ---

#[test]
fn done_declaration_attributes_and_leading_evidence_are_collected() {
    let a = parse_action(&wrap(
        "verb=\"done\" outcome=\"refused\" reason=\"no-defect\">\n\
         evidence: src/lib.rs:12 `return total / len(values)`\n\
         evidence: notes.txt absent\n\
         The goal describes a defect that is not present.\n",
    ))
    .unwrap();
    match a {
        Action::Done {
            summary,
            outcome,
            reason,
            evidence,
        } => {
            assert_eq!(outcome.as_deref(), Some("refused"));
            assert_eq!(reason.as_deref(), Some("no-defect"));
            assert_eq!(
                evidence,
                vec![
                    "evidence: src/lib.rs:12 `return total / len(values)`".to_string(),
                    "evidence: notes.txt absent".to_string(),
                ]
            );
            // `summary` stays the FULL trimmed body, evidence lines included.
            assert!(summary.starts_with("evidence: src/lib.rs:12"));
            assert!(summary.ends_with("not present."));
        }
        other => panic!("expected Done, got {other:?}"),
    }
}

#[test]
fn done_unknown_attribute_values_are_kept_verbatim_never_an_error() {
    let a = parse_action(&wrap(
        "verb=\"done\" outcome=\"banana\" reason=\"because\">\nsome body\n",
    ))
    .unwrap();
    match a {
        Action::Done { outcome, reason, .. } => {
            assert_eq!(outcome.as_deref(), Some("banana"));
            assert_eq!(reason.as_deref(), Some("because"));
        }
        other => panic!("expected Done, got {other:?}"),
    }
}

#[test]
fn undeclared_done_yields_nones_and_empty_evidence() {
    let a = parse_action(&wrap("verb=\"done\">\nfixed the bug\n")).unwrap();
    match a {
        Action::Done {
            summary,
            outcome,
            reason,
            evidence,
        } => {
            assert_eq!(summary, "fixed the bug");
            assert!(outcome.is_none() && reason.is_none() && evidence.is_empty());
        }
        other => panic!("expected Done, got {other:?}"),
    }
}

#[test]
fn evidence_only_body_is_not_empty_but_a_blank_body_still_errors() {
    let a = parse_action(&wrap(
        "verb=\"done\" outcome=\"refused\" reason=\"no-such-file\">\nevidence: gone.txt absent\n",
    ))
    .unwrap();
    match a {
        Action::Done { evidence, .. } => assert_eq!(evidence.len(), 1),
        other => panic!("expected Done, got {other:?}"),
    }
    assert!(matches!(
        parse_action(&wrap("verb=\"done\" outcome=\"patched\">\n   \n")).unwrap_err(),
        ActionError::EmptyBody { .. }
    ));
}

#[test]
fn evidence_lines_are_leading_only() {
    let a = parse_action(&wrap(
        "verb=\"done\">\nProse first.\nevidence: src/lib.rs:1 `x`\n",
    ))
    .unwrap();
    match a {
        Action::Done { evidence, .. } => assert!(evidence.is_empty(), "leading lines only"),
        other => panic!("expected Done, got {other:?}"),
    }
}
