//! Patch executor tests (Phase 2b/2c P3 Task 2).
//!
//! Mirrors `task_exec_read_find_test.rs`'s sandbox pattern: a real tempdir,
//! a real `Grant` scoped to it, real filesystem I/O — the whole point of
//! `exec_patch` is the atomic write-with-verify behavior against a real
//! filesystem, not a mocked one. The six tests below are the brief's
//! binding acceptance tests.
//!
//! **Python-availability determinism (brief's requirement):** the two
//! Python-lens tests must pass deterministically whether or not `python3`
//! is present on the box.
//! - `a_python_syntax_error_does_not_land_and_leaves_the_file` needs no
//!   guard: whether `python3` is present (and reports the real syntax
//!   error) or absent (fail-closed `"python3 unavailable"`),
//!   `PythonLens::parses` returns `Err(..)` either way, so `land()` always
//!   produces `DidNotParse{lens: "python", ..}` and the assertions hold in
//!   both worlds — this is `PythonLens`'s fail-closed contract exercised
//!   for free.
//! - `a_valid_python_patch_lands` genuinely needs a working `python3` to
//!   prove the success path (a valid file *landing*, not just failing
//!   closed), so it takes the brief's option (a): it probes `PATH` for a
//!   `python3` binary up front and skips (with an `eprintln!`) if none is
//!   found, rather than asserting something that would only be true when
//!   `python3` happens to be installed.

use bloomery_core::action::PatchBody;
use bloomery_core::grant::Grant;
use bloomery_daemon::task::exec_patch;
use std::path::PathBuf;

/// Builds `/tmp/bloomery-exec-patch-<pid>-<uniq>/sandbox` containing a
/// write root `out/` with a pre-existing `a.txt` and `x.py`. The grant
/// covers the whole sandbox for reads and `sandbox/out` for writes, with no
/// commands granted (this suite never execs anything through the grant —
/// `PythonLens` shells out to `python3` directly, outside the grant's
/// command surface, exactly like the brief specifies).
///
/// Per-call unique tempdir name (PID + atomic counter), same reasoning as
/// `task_exec_read_find_test.rs::sandbox`: parallel test threads in one
/// `cargo test` process must never collide on the sandbox directory.
fn sandbox() -> (PathBuf, Grant) {
    static UNIQUE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let unique = UNIQUE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let base = std::env::temp_dir().join(format!(
        "bloomery-exec-patch-{}-{unique}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&base);
    let sb = base.join("sandbox");
    std::fs::create_dir_all(sb.join("out")).unwrap();
    std::fs::write(sb.join("out/a.txt"), "hello old world\n").unwrap();
    std::fs::write(sb.join("out/x.py"), "x = 0\n").unwrap();
    let sb = std::fs::canonicalize(&sb).unwrap();
    let g = Grant::from_json(&format!(
        r#"{{"read_roots":["{s}"],"write_roots":["{s}/out"],"commands":[]}}"#,
        s = sb.display()
    ))
    .unwrap();
    (sb, g)
}

/// `true` iff some directory on `PATH` contains a `python3` entry. Used
/// only to decide whether `a_valid_python_patch_lands` can exercise the
/// real success path — never used by `exec_patch`/`PythonLens` itself,
/// which always attempts the real subprocess and fails closed if it's
/// absent.
fn python3_on_path() -> bool {
    std::env::var_os("PATH")
        .is_some_and(|paths| std::env::split_paths(&paths).any(|dir| dir.join("python3").is_file()))
}

#[test]
fn a_landing_search_replace_patch_writes_the_file() {
    let (sb, g) = sandbox();
    let body = PatchBody::SearchReplace {
        search: "old".into(),
        replace: "new".into(),
    };
    let obs = exec_patch(&g, &sb, "out/a.txt", &body);
    assert!(!obs.failed, "expected success, got {obs:?}");
    let contents = std::fs::read_to_string(sb.join("out/a.txt")).unwrap();
    assert_eq!(contents, "hello new world\n");
}

#[test]
fn a_non_applying_patch_leaves_the_file_untouched() {
    let (sb, g) = sandbox();
    let before = std::fs::read(sb.join("out/a.txt")).unwrap();
    let body = PatchBody::SearchReplace {
        search: "text that is not in the file".into(),
        replace: "x".into(),
    };
    let obs = exec_patch(&g, &sb, "out/a.txt", &body);
    assert!(obs.failed);
    assert!(
        obs.outcome.contains("did not land"),
        "outcome: {}",
        obs.outcome
    );
    let after = std::fs::read(sb.join("out/a.txt")).unwrap();
    assert_eq!(before, after, "file must be byte-for-byte untouched");
}

#[test]
fn a_python_syntax_error_does_not_land_and_leaves_the_file() {
    let (sb, g) = sandbox();
    let before = std::fs::read(sb.join("out/x.py")).unwrap();
    let body = PatchBody::WholeFile {
        contents: "def (:\n".into(),
    };
    let obs = exec_patch(&g, &sb, "out/x.py", &body);
    assert!(obs.failed);
    assert!(
        obs.outcome.contains("python"),
        "outcome should name the python lens: {}",
        obs.outcome
    );
    let after = std::fs::read(sb.join("out/x.py")).unwrap();
    assert_eq!(before, after, "file must be byte-for-byte untouched");
}

#[test]
fn a_valid_python_patch_lands() {
    if !python3_on_path() {
        eprintln!(
            "skipping a_valid_python_patch_lands: no python3 on PATH — \
             the fail-closed path is covered by \
             a_python_syntax_error_does_not_land_and_leaves_the_file instead"
        );
        return;
    }
    let (sb, g) = sandbox();
    let body = PatchBody::WholeFile {
        contents: "x = 1\n".into(),
    };
    let obs = exec_patch(&g, &sb, "out/x.py", &body);
    assert!(!obs.failed, "expected success, got {obs:?}");
    assert!(
        obs.outcome.contains("python"),
        "outcome should name the python lens: {}",
        obs.outcome
    );
    let after = std::fs::read_to_string(sb.join("out/x.py")).unwrap();
    assert_eq!(after, "x = 1\n");
}

#[test]
fn patch_outside_the_write_root_is_a_grant_violation() {
    let (sb, g) = sandbox();
    let target = "/etc/bloomery-p3-task2-should-never-exist";
    let body = PatchBody::WholeFile {
        contents: "malicious".into(),
    };
    let obs = exec_patch(&g, &sb, target, &body);
    assert!(obs.failed);
    assert!(
        obs.outcome.contains("grant violation"),
        "outcome: {}",
        obs.outcome
    );
    assert!(
        !std::path::Path::new(target).exists(),
        "exec_patch must never write outside the write root"
    );
}

#[test]
fn creating_a_new_file_in_the_write_root_lands() {
    let (sb, g) = sandbox();
    let target = sb.join("out/created.txt");
    assert!(!target.exists());
    let body = PatchBody::WholeFile {
        contents: "brand new contents\n".into(),
    };
    let obs = exec_patch(&g, &sb, "out/created.txt", &body);
    assert!(!obs.failed, "expected success, got {obs:?}");
    let contents = std::fs::read_to_string(&target).unwrap();
    assert_eq!(contents, "brand new contents\n");
}

/// Adversarial addition (self-review, not one of the brief's six): a
/// symlink planted *inside* the write root, pointing at a real file
/// outside it, is the write-side mirror of
/// `task_exec_read_find_test.rs::read_through_the_escape_symlink_is_refused_not_followed`.
/// `grant.check_write` canonicalizes and refuses this before `exec_patch`
/// ever reads or writes anything — asserted here at the `exec_patch` call
/// path itself, not just at the `Grant` layer underneath it (which
/// `grant_redteam_test.rs` already covers structurally).
#[test]
fn patching_through_an_escape_symlink_is_refused_not_followed() {
    let (sb, g) = sandbox();
    let outside = sb.parent().unwrap().join("outside-target.txt");
    std::fs::write(&outside, "original\n").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, sb.join("out/escape.txt")).unwrap();

    let body = PatchBody::WholeFile {
        contents: "attacker-controlled contents\n".into(),
    };
    let obs = exec_patch(&g, &sb, "out/escape.txt", &body);
    assert!(obs.failed);
    assert!(
        obs.outcome.contains("grant violation"),
        "outcome: {}",
        obs.outcome
    );
    assert_eq!(
        std::fs::read_to_string(&outside).unwrap(),
        "original\n",
        "exec_patch must never write through a symlink escaping the write root"
    );
}

/// Adversarial addition (self-review): after both a successful patch and a
/// non-applying (failed) one, the write root must contain exactly the
/// files it should — no `.bloomery-patch-*.tmp` scratch file left behind
/// either way. A leaked temp file on the failure path would mean
/// `atomic_write` (or its caller) isn't cleaning up after a failed write;
/// a leaked one on the success path would mean the rename didn't actually
/// consume it.
#[test]
fn no_temp_scratch_files_linger_after_success_or_failure() {
    let (sb, g) = sandbox();
    let out = sb.join("out");

    let ok_body = PatchBody::SearchReplace {
        search: "old".into(),
        replace: "new".into(),
    };
    let ok_obs = exec_patch(&g, &sb, "out/a.txt", &ok_body);
    assert!(!ok_obs.failed);

    let failing_body = PatchBody::SearchReplace {
        search: "text that is not in the file".into(),
        replace: "x".into(),
    };
    let failing_obs = exec_patch(&g, &sb, "out/a.txt", &failing_body);
    assert!(failing_obs.failed);

    let stray: Vec<_> = std::fs::read_dir(&out)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|name| name.contains("bloomery-patch"))
        .collect();
    assert!(stray.is_empty(), "stray temp files left behind: {stray:?}");
}
