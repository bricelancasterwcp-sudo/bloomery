use bloomery_core::grant::{Grant, GrantError};

const OK: &str = r#"{
  "read_roots": ["/tmp/sandbox", "/tmp/other"],
  "write_roots": ["/tmp/sandbox/out"],
  "commands": [["cargo", "test"], ["python", "-m", "pytest"]],
  "network": false
}"#;

#[test]
fn parses_a_valid_grant() {
    let g = Grant::from_json(OK).unwrap();
    assert_eq!(g.read_roots().len(), 2);
    assert_eq!(
        g.write_roots(),
        &[std::path::PathBuf::from("/tmp/sandbox/out")]
    );
    assert_eq!(
        g.commands()[1],
        vec!["python".to_string(), "-m".into(), "pytest".into()]
    );
    assert!(!g.network());
}

#[test]
fn network_absent_defaults_false() {
    let g = Grant::from_json(r#"{"read_roots":[],"write_roots":[],"commands":[]}"#).unwrap();
    assert!(!g.network());
}

#[test]
fn network_true_is_rejected() {
    let e = Grant::from_json(r#"{"read_roots":[],"write_roots":[],"commands":[],"network":true}"#);
    assert_eq!(e, Err(GrantError::NetworkNotSupported));
}

#[test]
fn a_relative_root_is_rejected() {
    let e = Grant::from_json(r#"{"read_roots":["relative/dir"],"write_roots":[],"commands":[]}"#);
    assert_eq!(
        e,
        Err(GrantError::NonAbsoluteRoot {
            root: "relative/dir".into()
        })
    );
}

#[test]
fn an_empty_command_prefix_is_rejected() {
    let e = Grant::from_json(r#"{"read_roots":[],"write_roots":[],"commands":[[]]}"#);
    assert_eq!(e, Err(GrantError::EmptyCommandPrefix));
}

#[test]
fn empty_commands_list_is_fine() {
    // No commands granted is a valid, safe grant (every run refused later).
    assert!(Grant::from_json(r#"{"read_roots":[],"write_roots":[],"commands":[]}"#).is_ok());
}

#[test]
fn malformed_json_is_a_named_parse_error() {
    assert!(matches!(
        Grant::from_json("not json"),
        Err(GrantError::Parse(_))
    ));
}

#[test]
fn an_unknown_field_is_rejected() {
    assert!(matches!(
        Grant::from_json(r#"{"read_roots":[],"write_roots":[],"commands":[],"sudo":true}"#),
        Err(GrantError::Parse(_))
    ));
}
