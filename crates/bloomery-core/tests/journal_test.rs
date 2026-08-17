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
fn codec_fixture_and_codec_verdict_round_trip() {
    let dir = std::env::temp_dir().join("bloomery-journal-g4");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("j-g4.jsonl");
    let _ = std::fs::remove_file(&path);
    let mut j = Journal::open(&path).unwrap();
    let e1 = Event::CodecFixture {
        model: "m1".into(),
        fixture_set: "core-v1".into(),
        fixture: "rename-var".into(),
        codec: "search_replace".into(),
        landed: true,
        steps: 2,
        detail: "applied".into(),
        expect: "patch".into(),
    };
    let e2 = Event::CodecVerdict {
        model: "m1".into(),
        fixture_set: "core-v1".into(),
        codec: "search_replace".into(),
        landed: 8,
        n: 10,
        interval95: [0.42, 0.94],
        provisional: false,
        mutating_verbs: true,
        detail: "applies_and_parses under bloomery-task-envelope-v1".into(),
    };
    j.append(&e1).unwrap();
    j.append(&e2).unwrap();
    assert_eq!(replay(&path).unwrap(), vec![e1, e2]);
}

/// G5 design doc §4's compat rule, at the narrowest possible grain: a raw
/// `CodecFixture` JSON line with no `expect` key at all (exactly what every
/// row journaled before this field existed looks like on disk) must still
/// deserialize, and must default to `"patch"` — never `"refuse"`, which
/// would silently reclassify every pre-G5 fixture row.
#[test]
fn codec_fixture_with_no_expect_key_deserializes_as_patch() {
    let line = r#"{"event":"CodecFixture","model":"m1","fixture_set":"core-v1","fixture":"rename-var","codec":"search_replace","landed":true,"steps":2,"detail":"applied"}"#;
    let event: Event = serde_json::from_str(line).expect("a pre-G5 CodecFixture line must parse");
    match event {
        Event::CodecFixture { expect, .. } => assert_eq!(expect, "patch"),
        other => panic!("expected CodecFixture, got {other:?}"),
    }
}

/// The G5 mixed-set verdict round-trips too — asymmetric on every field pair
/// that could be swapped with its neighbor (patch vs refuse counts,
/// intervals, provisional flags), same discipline as the G4 verdict test
/// above, so a field-order or copy-paste mistake in the class split flips a
/// byte the full-`Event` `assert_eq!` catches.
#[test]
fn codec_verdict_mixed_round_trips() {
    let dir = std::env::temp_dir().join("bloomery-journal-g5");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("j-g5.jsonl");
    let _ = std::fs::remove_file(&path);
    let mut j = Journal::open(&path).unwrap();
    let e1 = Event::CodecVerdictMixed {
        model: "m1".into(),
        fixture_set: "codec-tasks-v2-mixed".into(),
        codec: "search_replace".into(),
        envelope: "bloomery-task-envelope-v1".into(),
        patch_landed: 9,
        patch_n: 10,
        patch_interval95: [0.59, 0.98],
        patch_provisional: true,
        refuse_landed: 3,
        refuse_n: 10,
        refuse_interval95: [0.11, 0.60],
        refuse_provisional: false,
        done_trust: false,
        detail: "codec from profile".into(),
    };
    j.append(&e1).unwrap();
    assert_eq!(replay(&path).unwrap(), vec![e1]);
}

#[test]
fn committed_g2_journal_still_replays() {
    // Backward-compatibility pin: schema changes must never orphan the
    // committed evidence. Every committed `*.jsonl` under the evidence
    // directory is a real journal from a real run (G2's cold/warm/coldcache
    // journals carry `ModelUnloaded`, which the two hand-built round-trip
    // tests above never touch) — loop over the directory rather than one
    // named file, so a schema change is pinned against all of them, and a
    // *future* committed journal is picked up automatically without anyone
    // remembering to add a case for it. Path is relative to the workspace
    // root.
    let evidence_dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/superpowers/evidence");
    let journal_paths: Vec<_> = std::fs::read_dir(&evidence_dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("jsonl"))
        .collect();
    assert!(
        journal_paths.len() >= 4,
        "expected at least 4 committed journals under {}, found {}",
        evidence_dir.display(),
        journal_paths.len()
    );

    let mut total_events = 0usize;
    for path in &journal_paths {
        let events = replay(path).unwrap_or_else(|e| {
            panic!("committed journal {} failed to replay: {e}", path.display())
        });
        assert!(
            !events.is_empty(),
            "expected a non-empty journal at {}, got 0 events",
            path.display()
        );
        // The g2-warm journal specifically is large enough (hundreds of
        // events from the real G2 bench run) that a per-file `>0` alone
        // would let a truncation regression slip through unnoticed; keep
        // its original stronger bound as a named case rather than folding
        // it into the loop's generic assertion.
        if path.file_name().and_then(|n| n.to_str()) == Some("2026-08-14-g2-warm-journal.jsonl") {
            assert!(
                events.len() > 100,
                "expected a real journal, got {} events",
                events.len()
            );
        }
        total_events += events.len();
    }
    // Total-count sanity across all committed journals, so a regression
    // that zeroed out every file's *individual* count in some correlated
    // way (unlikely, but the `>0` checks above are per-file) still shows up
    // in aggregate.
    assert!(
        total_events > 100,
        "expected committed journals to carry a real number of events in \
         aggregate, got {total_events}"
    );
}
