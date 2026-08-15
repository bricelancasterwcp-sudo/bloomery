//! Read + find executor tests (Phase 2b/2c P3 Task 1).
//!
//! Mirrors the P2 red-team test pattern (`grant_redteam_test.rs`): a real
//! tempdir sandbox, a real `Grant` scoped to it, real filesystem I/O — no
//! mocked `fs::canonicalize`, because the whole point of these executors is
//! to match the real attack surface (see `bloomery_core::grant::path`'s
//! module docs for the same argument at the grant layer). The five tests
//! below are the brief's binding acceptance tests; the last two are this
//! task's own adversarial addition, proving the escape symlink the sandbox
//! carries is refused by both `exec_read` and `exec_find`, not just by the
//! `Grant` layer underneath them.

use bloomery_core::grant::Grant;
use bloomery_daemon::task::{exec_find, exec_read, ExecBounds};
use std::path::PathBuf;

fn bounds() -> ExecBounds {
    ExecBounds {
        read_cap_bytes: 256 * 1024,
        find_result_cap: 100,
        run_output_cap_bytes: 64 * 1024,
        run_timeout_secs: 120,
    }
}

/// Builds `/tmp/bloomery-exec-<pid>-<uniq>/sandbox` containing `file.txt`
/// ("line1\nline2\nline3\n"), an `out/` write-only subdirectory, and an
/// `escape` symlink pointing at `/etc` (a real, existing, unambiguously
/// outside-the-sandbox target — mirroring `grant_redteam_test.rs`'s
/// `root_escape -> /`). The grant covers exactly `sandbox` for reads and
/// `sandbox/out` for writes, with no commands granted (this suite never
/// execs anything).
///
/// Per-call unique tempdir name (PID + atomic counter), same reasoning as
/// `grant_redteam_test.rs::locked_sandbox`: parallel test threads in one
/// `cargo test` process must never collide on the sandbox directory or the
/// `escape` symlink. Cleanup is best-effort pre-clean only.
fn sandbox() -> (PathBuf, Grant) {
    static UNIQUE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let unique = UNIQUE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let base = std::env::temp_dir().join(format!("bloomery-exec-{}-{unique}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let sb = base.join("sandbox");
    std::fs::create_dir_all(sb.join("out")).unwrap();
    std::fs::write(sb.join("file.txt"), "line1\nline2\nline3\n").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink("/etc", sb.join("escape")).unwrap();
    let sb = std::fs::canonicalize(&sb).unwrap();
    let g = Grant::from_json(&format!(
        r#"{{"read_roots":["{s}"],"write_roots":["{s}/out"],"commands":[]}}"#,
        s = sb.display()
    ))
    .unwrap();
    (sb, g)
}

#[test]
fn read_a_granted_file_returns_its_content() {
    let (sb, g) = sandbox();
    let obs = exec_read(&g, &sb, "file.txt", None, &bounds());
    assert!(!obs.failed);
    assert!(obs.content.contains("line2"));
    assert!(obs.outcome.starts_with("read "));
}

#[test]
fn read_a_line_window_returns_only_those_lines() {
    let (sb, g) = sandbox();
    let obs = exec_read(&g, &sb, "file.txt", Some((2, 2)), &bounds());
    assert_eq!(obs.content.trim(), "line2");
}

#[test]
fn read_outside_the_grant_is_a_failed_grant_violation_not_a_panic() {
    let (sb, g) = sandbox();
    let obs = exec_read(&g, &sb, "/etc/passwd", None, &bounds());
    assert!(obs.failed);
    assert!(obs.outcome.contains("grant violation"));
}

#[test]
fn read_respects_the_byte_cap_with_a_visible_notice() {
    let (sb, g) = sandbox();
    let mut small = bounds();
    small.read_cap_bytes = 4; // "line" then truncated
    let obs = exec_read(&g, &sb, "file.txt", None, &small);
    assert!(obs.outcome.contains("truncated"));
    assert!(obs.content.len() <= 4);
}

#[test]
fn find_matches_within_the_read_root_bounded() {
    let (sb, g) = sandbox();
    let obs = exec_find(&g, "line\\d", &sb.to_string_lossy(), &bounds());
    assert!(!obs.failed);
    assert!(obs.content.contains("file.txt"));
}

/// Adversarial addition: `escape` resolves to `/etc`, and `/etc/passwd`
/// really exists on this box (same construction `grant_redteam_test.rs`
/// relies on). `exec_read` must refuse this exactly like a direct
/// `/etc/passwd` request — the grant boundary, not a string check on the
/// requested path, is what decides this.
#[test]
fn read_through_the_escape_symlink_is_refused_not_followed() {
    let (sb, g) = sandbox();
    let obs = exec_read(&g, &sb, "escape/passwd", None, &bounds());
    assert!(obs.failed);
    assert!(obs.outcome.contains("grant violation"));
}

/// Adversarial addition: a naive walker that lists `escape`'s directory
/// entries (because `read_dir` on a symlinked directory transparently
/// follows it) would find `/etc/passwd` sitting right there and, if it
/// matched candidate files by raw joined path instead of by canonical path
/// under a read root, would leak its contents into a find result. Assert
/// the walk never reaches inside `/etc` by searching a pattern
/// (`root:`) `/etc/passwd` is virtually guaranteed to contain and
/// confirming it produced no `/etc` hits at all.
#[test]
fn find_does_not_walk_through_the_escape_symlink() {
    let (sb, g) = sandbox();
    let obs = exec_find(&g, "root:", &sb.to_string_lossy(), &bounds());
    assert!(!obs.failed);
    assert!(!obs.content.contains("/etc/"));
}

/// Adversarial addition: a directory symlink that cycles back to its own
/// ancestor (`loopy/back -> loopy`) is the classic way a naive recursive
/// walker hangs or blows its stack. Since `walk_and_match` never descends
/// into *any* symlink (see its doc comment), this must complete
/// immediately rather than loop — this test's real assertion is that it
/// returns at all (a hang would time out the test binary).
#[test]
fn find_does_not_hang_on_a_directory_symlink_cycle() {
    let (sb, g) = sandbox();
    let loopy = sb.join("loopy");
    std::fs::create_dir_all(&loopy).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&loopy, loopy.join("back")).unwrap();
    let obs = exec_find(&g, "line", &sb.to_string_lossy(), &bounds());
    assert!(!obs.failed);
    assert!(obs.content.contains("file.txt"));
}

/// Attention point called out in the brief: opening a directory with
/// `O_NOFOLLOW`+`read(true)` succeeds at the `open(2)` call (directories
/// open fine for reading their metadata), but the subsequent `read(2)`
/// call `open_nofollow_read` makes to pull bytes out of it fails with
/// `EISDIR` — a real OS error at a different point than the symlink/ELOOP
/// case. `exec_read` must surface that as a failed `Observation`, not
/// propagate a panic or an `Err` out of the function.
#[test]
fn read_a_directory_is_a_failed_observation_not_a_panic() {
    let (sb, g) = sandbox();
    // `out/` is a real directory inside the read root (read_roots covers
    // the whole sandbox; write_roots is the narrower `out/` scope, which
    // doesn't affect what's readable).
    let obs = exec_read(&g, &sb, "out", None, &bounds());
    assert!(obs.failed);
    assert!(
        obs.outcome.to_lowercase().contains("directory"),
        "expected an EISDIR-shaped message, got: {}",
        obs.outcome
    );
}
