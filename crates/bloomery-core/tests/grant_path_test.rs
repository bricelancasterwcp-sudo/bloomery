use bloomery_core::grant::{Grant, GrantViolation, PathKind};
use std::path::PathBuf;

// Build /tmp/bloomery-grant-<unique>/sandbox with a file inside, and an
// escape symlink sandbox/escape -> /etc. Returns the sandbox path (canonical).
//
// NOTE: the brief's helper keys the tempdir on std::process::id() alone,
// which collides across the parallel test threads cargo test runs within
// one process (observed: symlink-already-exists races). We add a
// per-call atomic counter to keep each test's sandbox isolated; the test
// bodies and assertions below are otherwise verbatim from the brief.
fn sandbox() -> PathBuf {
    static UNIQUE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let unique = UNIQUE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let base = std::env::temp_dir().join(format!("bloomery-grant-{}-{unique}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let sb = base.join("sandbox");
    std::fs::create_dir_all(sb.join("out")).unwrap();
    std::fs::write(sb.join("file.txt"), "hi").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink("/etc", sb.join("escape")).unwrap();
    std::fs::canonicalize(&sb).unwrap()
}

fn grant_for(sb: &std::path::Path) -> Grant {
    let json = format!(
        r#"{{"read_roots":["{s}"],"write_roots":["{s}/out"],"commands":[]}}"#,
        s = sb.display()
    );
    Grant::from_json(&json).unwrap()
}

#[test]
fn read_within_the_root_is_allowed_and_returns_canonical() {
    let sb = sandbox();
    let g = grant_for(&sb);
    let got = g.check_read(&sb.join("file.txt")).unwrap();
    assert_eq!(got, std::fs::canonicalize(sb.join("file.txt")).unwrap());
}

#[test]
fn a_dotdot_traversal_out_of_the_root_is_refused() {
    let sb = sandbox();
    let g = grant_for(&sb);
    // sandbox() nests two directories under the OS tempdir
    // (`bloomery-grant-<pid>-<n>/sandbox`), so three `..` are needed to
    // reach the real filesystem root and land on a path — /etc/passwd —
    // that actually exists and is unambiguously outside the sandbox.
    let escape = sb
        .join("..")
        .join("..")
        .join("..")
        .join("etc")
        .join("passwd");
    match g.check_read(&escape) {
        Err(GrantViolation::PathOutsideRoots {
            kind: PathKind::Read,
            ..
        }) => {}
        other => panic!("expected refusal, got {other:?}"),
    }
}

#[test]
fn a_symlink_pointing_out_of_the_root_is_refused() {
    let sb = sandbox();
    let g = grant_for(&sb);
    // sb/escape -> /etc ; reading sb/escape/hosts resolves to /etc/hosts, outside
    match g.check_read(&sb.join("escape").join("hosts")) {
        Err(GrantViolation::PathOutsideRoots { .. }) => {}
        other => panic!("expected refusal via symlink, got {other:?}"),
    }
}

#[test]
fn a_sibling_root_with_a_shared_string_prefix_does_not_match() {
    // Root /tmp/.../sandbox must NOT admit /tmp/.../sandbox-evil (string prefix,
    // not a path component boundary). Build both, grant only sandbox.
    let sb = sandbox();
    let evil = sb.parent().unwrap().join("sandbox-evil");
    std::fs::create_dir_all(&evil).unwrap();
    std::fs::write(evil.join("x"), "x").unwrap();
    let g = grant_for(&sb);
    assert!(matches!(
        g.check_read(&evil.join("x")),
        Err(GrantViolation::PathOutsideRoots { .. })
    ));
}

#[test]
fn write_to_a_new_file_in_a_granted_dir_is_allowed() {
    let sb = sandbox();
    let g = grant_for(&sb);
    let newfile = sb.join("out").join("created.txt"); // does not exist yet
    let got = g.check_write(&newfile).unwrap();
    assert_eq!(
        got,
        std::fs::canonicalize(sb.join("out"))
            .unwrap()
            .join("created.txt")
    );
}

#[test]
fn write_outside_the_write_root_is_refused_even_if_in_a_read_root() {
    let sb = sandbox();
    let g = grant_for(&sb);
    // sb/file.txt is under the READ root but not the WRITE root (sb/out)
    match g.check_write(&sb.join("file.txt")) {
        Err(GrantViolation::PathOutsideRoots {
            kind: PathKind::Write,
            ..
        }) => {}
        other => panic!("expected write refusal, got {other:?}"),
    }
}

#[test]
fn a_relative_target_is_refused() {
    let sb = sandbox();
    let g = grant_for(&sb);
    assert!(matches!(
        g.check_read(std::path::Path::new("relative/x")),
        Err(GrantViolation::PathOutsideRoots { .. })
    ));
}

#[test]
fn write_whose_parent_dir_is_missing_is_named() {
    let sb = sandbox();
    let g = grant_for(&sb);
    // sb/out/nope/deep.txt — parent sb/out/nope doesn't exist
    match g.check_write(&sb.join("out").join("nope").join("deep.txt")) {
        Err(GrantViolation::PathParentMissing { .. }) => {}
        other => panic!("expected PathParentMissing, got {other:?}"),
    }
}
