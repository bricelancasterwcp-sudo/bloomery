//! flywheel turn-3 Task 2 — fixture `commands` threading into
//! `fixture_grant`'s `Grant` (the first of three named instrument deltas;
//! see `.superpowers/sdd/2026-08-20-flywheel3-turn3/task-2-brief.md`).
//!
//! `codec_probe_test.rs` is already at the 800-line-plus ceiling other
//! files in this crate observe, so this is a new, narrowly-scoped test
//! home rather than an addition to that file (task-2 brief, `Files`).
//!
//! Exercises `fixture_grant` itself (via the `test-support`-gated
//! `fixture_grant_for_test` wrapper, the same pattern `post.rs`'s
//! `run_bounded_for_test` and `swap.rs`'s `CoverGate::with_runner` use to
//! expose one crate-private engine function to integration tests without
//! widening the crate's real public API) against a real, TOML-parsed
//! `Fixture` and a real `bloomery_core::grant::Grant` — no mocks: the
//! `Grant` returned is checked with its own real `check_command`.

use bloomery_daemon::codec_probe::fixture_grant_for_test;
use bloomery_daemon::codec_probe::fixtures::parse_fixture_set;

/// A fresh, absolute scratch directory for one test's `fixture_grant` call.
/// `fixture_grant` never touches the filesystem itself (only
/// `codec_probe::materialize` does, elsewhere), so this need not exist —
/// it only needs to be a real absolute path, which `Grant::from_json`
/// requires of every read/write root.
fn scratch_dir(label: &str) -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "bloomery-codec-probe-grant-test-{}-{label}-{seq}",
        std::process::id()
    ))
}

/// A one-fixture TOML set with no `commands` key.
const NO_COMMANDS_SET: &str = r#"
set = "codec-tasks-grant-test"

[[fixture]]
name = "no-commands"
lens = "plaintext"
target = "a.txt"
goal = "fix the broken line in a.txt"

[[fixture.file]]
path = "a.txt"
contents = "broken\n"

[fixture.reference]
search = "broken"
replace = "fixed"
"#;

/// A one-fixture TOML set carrying exactly Task 8's brief-pinned shape:
/// `commands = [["python3", "-m", "py_compile"]]`.
const WITH_COMMANDS_SET: &str = r#"
set = "codec-tasks-grant-test"

[[fixture]]
name = "py-compile-check"
lens = "python"
target = "stats.py"
goal = "fix mean() in stats.py"
commands = [["python3", "-m", "py_compile"]]

[[fixture.file]]
path = "stats.py"
contents = "def mean(values):\n    return sum(values) / (len(values) + 1)\n"

[fixture.reference]
search = "    return sum(values) / (len(values) + 1)"
replace = "    return sum(values) / len(values)"
"#;

/// Today's behavior, pinned: a fixture with no `commands` produces a grant
/// that refuses every argv, including the one Task 8's fixtures will later
/// grant. This is the regression the mutation check (task-2 brief Step 4)
/// exists to catch: if the threading silently reverted to a hardcoded
/// `"commands": []`, this test alone would NOT notice (it would still
/// pass) — `commands_prefix_is_accepted_by_check_command` below is the one
/// that would fail.
#[test]
fn empty_commands_grant_refuses_every_argv() {
    let set = parse_fixture_set(NO_COMMANDS_SET).expect("fixture set should parse");
    let fixture = &set.fixtures[0];
    let dir = scratch_dir("empty");

    let grant = fixture_grant_for_test(&dir, fixture).expect("grant should build");

    assert!(
        grant
            .check_command(&[
                "python3".to_string(),
                "-m".to_string(),
                "py_compile".to_string(),
            ])
            .is_err(),
        "no commands granted means every argv is refused"
    );
    assert!(
        grant
            .check_command(&["rm".to_string(), "-rf".to_string()])
            .is_err(),
        "no commands granted means every argv is refused"
    );
}

/// The load-bearing case: a fixture's `commands` prefix threads all the way
/// into the `Grant` `fixture_grant` builds, so `check_command` accepts an
/// argv extending the granted prefix and still refuses an unrelated one.
#[test]
fn commands_prefix_is_accepted_by_check_command() {
    let set = parse_fixture_set(WITH_COMMANDS_SET).expect("fixture set should parse");
    let fixture = &set.fixtures[0];
    let dir = scratch_dir("with-prefix");

    let grant = fixture_grant_for_test(&dir, fixture).expect("grant should build");

    assert!(
        grant
            .check_command(&[
                "python3".to_string(),
                "-m".to_string(),
                "py_compile".to_string(),
                "x.py".to_string(),
            ])
            .is_ok(),
        "argv extending the granted prefix must be accepted"
    );
    assert!(
        grant
            .check_command(&["rm".to_string(), "-rf".to_string()])
            .is_err(),
        "an argv outside every granted prefix must still be refused"
    );
}
