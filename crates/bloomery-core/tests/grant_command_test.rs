use bloomery_core::grant::{Grant, GrantViolation};

fn g() -> Grant {
    Grant::from_json(
        r#"{"read_roots":[],"write_roots":[],"commands":[["cargo","test"],["python","-m","pytest"]]}"#,
    )
    .unwrap()
}

fn argv(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|s| s.to_string()).collect()
}

#[test]
fn a_prefix_match_with_appended_args_is_allowed() {
    assert!(g()
        .check_command(&argv(&["cargo", "test", "--", "mytest"]))
        .is_ok());
    assert!(g().check_command(&argv(&["cargo", "test"])).is_ok()); // exact prefix
    assert!(g()
        .check_command(&argv(&["python", "-m", "pytest", "-k", "foo"]))
        .is_ok());
}

#[test]
fn a_different_command_is_refused() {
    match g().check_command(&argv(&["cargo", "build"])) {
        // diverges at element 1
        Err(GrantViolation::CommandNotAllowed { argv }) => assert_eq!(argv[1], "build"),
        other => panic!("{other:?}"),
    }
    assert!(g().check_command(&argv(&["rm", "-rf", "/"])).is_err());
}

#[test]
fn argv_shorter_than_the_prefix_is_refused() {
    assert!(g().check_command(&argv(&["cargo"])).is_err()); // prefix is 2 long
}

#[test]
fn reordered_prefix_is_refused() {
    assert!(g().check_command(&argv(&["test", "cargo"])).is_err());
}

#[test]
fn empty_argv_is_refused() {
    assert!(matches!(
        g().check_command(&[]),
        Err(GrantViolation::CommandNotAllowed { .. })
    ));
}

#[test]
fn a_grant_with_no_commands_refuses_everything() {
    let none = Grant::from_json(r#"{"read_roots":[],"write_roots":[],"commands":[]}"#).unwrap();
    assert!(none.check_command(&argv(&["cargo", "test"])).is_err());
}
