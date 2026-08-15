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

// --- Derived-Deserialize path: closes the bypass where
// `serde_json::from_str::<Grant>(..)` (the path P3 uses to deserialize a
// `grants` field nested in a larger task-request body) constructed a Grant
// without running `from_json`'s validation. A `[[]]` commands entry is an
// empty prefix, and `argv.starts_with(&[])` is always true, so an
// unvalidated Grant built this way allow-lists every command — a total
// allowlist bypass. These assert the derive path now rejects it, same as
// `from_json`.

#[test]
fn derived_deserialize_also_validates() {
    // The empty-prefix allowlist-bypass body: previously deserialized Ok
    // via the derived impl even though `from_json` rejects it.
    let bypass =
        serde_json::from_str::<Grant>(r#"{"read_roots":[],"write_roots":[],"commands":[[]]}"#);
    assert!(bypass.is_err(), "empty command prefix must not deserialize");

    // network:true must also be rejected via the derive path, not just from_json.
    let network = serde_json::from_str::<Grant>(
        r#"{"read_roots":[],"write_roots":[],"commands":[],"network":true}"#,
    );
    assert!(network.is_err(), "network:true must not deserialize");

    // A relative root must also be rejected via the derive path.
    let relative_root = serde_json::from_str::<Grant>(
        r#"{"read_roots":["relative/dir"],"write_roots":[],"commands":[]}"#,
    );
    assert!(relative_root.is_err(), "relative root must not deserialize");

    // A valid grant still deserializes fine via the derive path.
    let ok = serde_json::from_str::<Grant>(OK);
    assert!(ok.is_ok(), "a valid grant must still deserialize");
}

#[test]
fn an_empty_prefix_grant_cannot_be_constructed_by_any_path() {
    let body = r#"{"read_roots":[],"write_roots":[],"commands":[[]]}"#;

    assert_eq!(
        Grant::from_json(body),
        Err(GrantError::EmptyCommandPrefix),
        "from_json must reject the empty-prefix bypass"
    );
    assert!(
        serde_json::from_str::<Grant>(body).is_err(),
        "the derived Deserialize impl must also reject the empty-prefix bypass"
    );
}
