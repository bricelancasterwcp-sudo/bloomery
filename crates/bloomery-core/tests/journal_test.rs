use bloomery_core::journal::*;

#[test]
fn append_then_replay_round_trips() {
    let dir = std::env::temp_dir().join("bloomery-journal-test");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("j.jsonl");
    let _ = std::fs::remove_file(&path);
    let mut j = Journal::open(&path).unwrap();
    let e1 = Event::Boot {
        version: "0.1.0".into(),
    };
    let e2 = Event::PagerOp {
        id: "a1".into(),
        op: PagerOpKind::ResumeLoad,
        bytes: 450_000_000,
        duration_ms: 20,
        image_tier: "ram".into(),
    };
    j.append(&e1).unwrap();
    j.append(&e2).unwrap();
    assert_eq!(replay(&path).unwrap(), vec![e1, e2]);
}

#[test]
fn prompt_hash_is_stable() {
    assert_eq!(sha256_hex("abc").len(), 64);
    assert_eq!(sha256_hex("abc"), sha256_hex("abc"));
}

/// Regression pin for law 7: a corrupt journal line must fail loudly, never
/// be skipped. `append_then_replay_round_trips` above already proves a
/// fully-valid file replays `Ok`; this test proves the opposite — that a
/// single garbage line after valid events turns `replay` into `Err`, not a
/// truncated or partial `Vec`.
#[test]
fn replay_fails_loudly_on_corrupt_line() {
    let dir = std::env::temp_dir().join("bloomery-journal-test");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("j-corrupt.jsonl");
    let _ = std::fs::remove_file(&path);
    let mut j = Journal::open(&path).unwrap();
    let e1 = Event::Boot {
        version: "0.1.0".into(),
    };
    let e2 = Event::ModelUnloaded {
        model: "qwen".into(),
    };
    j.append(&e1).unwrap();
    j.append(&e2).unwrap();
    drop(j);

    // Append a garbage line directly, bypassing Journal, to simulate corruption.
    use std::io::Write as _;
    let mut raw = std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap();
    writeln!(raw, "not json").unwrap();

    assert!(replay(&path).is_err());
}

#[test]
fn agent_removed_and_task_step_round_trip() {
    let dir = std::env::temp_dir().join("bloomery-journal-2a");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("j2a.jsonl");
    let _ = std::fs::remove_file(&path);
    let mut j = Journal::open(&path).unwrap();
    let e1 = Event::AgentRemoved {
        id: "a1".into(),
        reason: "ephemeral cleanup".into(),
    };
    let e2 = Event::TaskStep {
        id: "a1".into(),
        step: 3,
        verb: "patch".into(),
        outcome: "applied".into(),
        duration_ms: 41,
    };
    j.append(&e1).unwrap();
    j.append(&e2).unwrap();
    assert_eq!(replay(&path).unwrap(), vec![e1, e2]);
}

#[test]
fn committed_g2_journal_still_replays() {
    // Backward-compatibility pin: schema changes must never orphan the
    // committed evidence. Path is relative to the workspace root.
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/superpowers/evidence/2026-08-14-g2-warm-journal.jsonl");
    let events = replay(&path).unwrap();
    assert!(
        events.len() > 100,
        "expected a real journal, got {} events",
        events.len()
    );
}
