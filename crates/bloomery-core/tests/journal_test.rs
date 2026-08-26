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

/// Every appended row carries the writer's own wall-clock stamp (`epoch_ms`,
/// milliseconds since the Unix epoch — the `_ms` naming the journal already
/// uses for durations), so a row can be correlated with clocks *outside* the
/// journal: GPU sample logs, daemon stderr, an operator's notes. Found as
/// bA2/F2 (swap-candidate acceptance 2): a 10,933 MiB VRAM dip could not be
/// tied to any journal row because no row said *when* — the union of keys
/// over 1132 rows held no time at all.
///
/// The stamp is a **row** property, not an `Event` field: it records when the
/// writer wrote, not what happened, so `replay` returns events unchanged and
/// the raw JSONL is the correlation surface. The row layout keeps the `event`
/// tag first — rows stay greppable as `{"event":"…`.
#[test]
fn an_appended_row_carries_a_bounded_epoch_ms_stamp() {
    // Pid-suffixed: two concurrent `cargo test` runs (main repo + a worktree)
    // must not share one append-only file, or `lines.len()` reads 4.
    let dir = std::env::temp_dir().join(format!("bloomery-journal-stamp-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("j-stamp.jsonl");
    let _ = std::fs::remove_file(&path);
    let mut j = Journal::open(&path).unwrap();
    let e1 = Event::Boot {
        version: "0.1.0".into(),
    };
    let e2 = Event::ModelUnloaded {
        model: "qwen".into(),
    };
    let now_ms = || -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
    };
    let before = now_ms();
    j.append(&e1).unwrap();
    j.append(&e2).unwrap();
    let after = now_ms();

    let raw = std::fs::read_to_string(&path).unwrap();
    let lines: Vec<&str> = raw.lines().collect();
    assert_eq!(lines.len(), 2);
    for line in &lines {
        assert!(
            line.starts_with(r#"{"event":""#),
            "the event tag must stay first: {line}"
        );
        let value: serde_json::Value = serde_json::from_str(line).unwrap();
        let stamp = value["epoch_ms"]
            .as_u64()
            .unwrap_or_else(|| panic!("row carries no numeric epoch_ms: {line}"));
        // Known flake mode, accepted on purpose: a backwards wall-clock step
        // (NTP) landing inside this microseconds-wide window fails the bound.
        // Do NOT widen the bounds — a loose window is exactly what lets a
        // constant-stamp mutant live. A one-off red here means the clock
        // stepped, not that the stamp broke; re-run it.
        assert!(
            (before..=after).contains(&stamp),
            "epoch_ms {stamp} outside the append's own clock bounds [{before}, {after}]"
        );
    }

    // The stamp never enters the Event schema: replay returns the events
    // exactly as appended, and the stamp lives only in the row's bytes.
    assert_eq!(replay(&path).unwrap(), vec![e1, e2]);
}

/// The narrow-grain compat pin, both directions (the `CodecFixture`/`expect`
/// precedent): a stamped line and its unstamped ancestor deserialize to the
/// same `Event`. The unstamped direction is what keeps every journal written
/// before the stamp existed replaying; the stamped direction is serde's
/// unknown-key tolerance on the internally-tagged enum — behavior that
/// predates the stamp but is load-bearing from now on, so it is pinned here
/// (a `deny_unknown_fields` added to `Event` would fail this test, and would
/// orphan every stamped journal).
#[test]
fn a_stamped_line_and_its_unstamped_ancestor_deserialize_to_the_same_event() {
    let unstamped = r#"{"event":"ModelUnloaded","model":"qwen"}"#;
    let stamped = r#"{"event":"ModelUnloaded","model":"qwen","epoch_ms":1787197252123}"#;
    let from_unstamped: Event = serde_json::from_str(unstamped).unwrap();
    let from_stamped: Event = serde_json::from_str(stamped).unwrap();
    assert_eq!(from_unstamped, from_stamped);
    assert_eq!(
        from_stamped,
        Event::ModelUnloaded {
            model: "qwen".into()
        }
    );
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
        args: Vec::new(),
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
        agent: None,
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

/// The drift watch's baseline row (design §2/§5): a blessing is only as
/// good as its provenance and its bytes, so both ride in the row. Asymmetric
/// on every field so a copy-paste swap between `profile_path`, `sha`, and
/// `provenance` flips a byte the full-`Event` `assert_eq!` catches.
#[test]
fn blessed_round_trips() {
    let dir = std::env::temp_dir().join("bloomery-journal-drift");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("j-drift.jsonl");
    let _ = std::fs::remove_file(&path);
    let mut j = Journal::open(&path).unwrap();
    let e1 = Event::Blessed {
        model: "qwen3:8b".into(),
        profile_path: "/var/lib/bloomery/profiles/qwen3:8b.baseline.json".into(),
        sha: sha256_hex("the blessed bytes"),
        provenance: "auto-first-profile".into(),
    };
    let e2 = Event::Blessed {
        model: "qwen2.5-coder:7b-q8_0".into(),
        profile_path: "/var/lib/bloomery/profiles/qwen2.5-coder:7b-q8_0.baseline.json".into(),
        sha: sha256_hex("different bytes"),
        provenance: "operator".into(),
    };
    j.append(&e1).unwrap();
    j.append(&e2).unwrap();
    assert_eq!(replay(&path).unwrap(), vec![e1, e2]);
}

/// The drift watch's comparison row (design §4, plus the identity fields the
/// controller ruled in): a row must be re-runnable (both paths, the exit code)
/// *and* byte-verifiable (both digests), and its `None`s must survive the
/// round trip as `None` — an absent digest that came back as `""`, or an
/// absent exit code that came back as `0`, would each read as a measurement
/// nobody took. Asymmetric on every field so a copy-paste swap between the
/// reference and current sides flips a byte the full-`Event` `assert_eq!`
/// catches.
#[test]
fn drift_round_trips() {
    let dir = std::env::temp_dir().join("bloomery-journal-drift-gate");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("j-drift-gate.jsonl");
    let _ = std::fs::remove_file(&path);
    let mut j = Journal::open(&path).unwrap();
    let ran = Event::Drift {
        model: "qwen3:8b".into(),
        comparison: "step".into(),
        outcome: "drift".into(),
        reference_path: "/var/lib/bloomery/profiles/qwen3:8b.previous.json".into(),
        current_path: "/var/lib/bloomery/profiles/qwen3:8b.json".into(),
        exit_code: Some(1),
        reference_sha: Some(sha256_hex("last boot's bytes")),
        current_sha: Some(sha256_hex("this boot's bytes")),
    };
    let never_ran = Event::Drift {
        model: "qwen2.5-coder:7b-q8_0".into(),
        comparison: "cumulative".into(),
        outcome: "unmeasured: reference /var/lib/bloomery/profiles/\
                  qwen2.5-coder:7b-q8_0.baseline.json: no such file"
            .into(),
        reference_path: "/var/lib/bloomery/profiles/qwen2.5-coder:7b-q8_0.baseline.json".into(),
        current_path: "/var/lib/bloomery/profiles/qwen2.5-coder:7b-q8_0.json".into(),
        exit_code: None,
        reference_sha: None,
        current_sha: Some(sha256_hex("the current bytes")),
    };
    j.append(&ran).unwrap();
    j.append(&never_ran).unwrap();
    assert_eq!(replay(&path).unwrap(), vec![ran, never_ran]);
}

/// The memory organ's three rows (memory-organ design
/// `docs/superpowers/specs/2026-08-26-memory-organ-design.md` §4/§5) round
/// trip. Additive, like every variant since `TaskStep`: no existing row grew
/// a field, so old journals carry none of these tags and replay unchanged —
/// pinned per committed file by [`committed_g2_journal_still_replays`].
///
/// Asymmetric on every field that could be swapped with its neighbor — the
/// two ids (`id` is the agent, `task_id` the task, `episode_id` the store
/// row), and the two stamp modes with their `Some`/`None` episode — so a
/// copy-paste mistake flips a byte the full-`Event` `assert_eq!` catches. A
/// `silent` stamp's `None` in particular must survive as `None`: an absent
/// episode that came back as `Some("")` would read as an injection nobody
/// made.
#[test]
fn memory_organ_rows_round_trip() {
    let dir = std::env::temp_dir().join("bloomery-journal-memory");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("j-memory.jsonl");
    let _ = std::fs::remove_file(&path);
    let mut j = Journal::open(&path).unwrap();
    let injected = Event::MemoryStamp {
        id: "agent-3".into(),
        task_id: "task-11".into(),
        mode: "injected".into(),
        episode_id: Some("ep-a1".into()),
        candidates_checked: 4,
    };
    let silent = Event::MemoryStamp {
        id: "agent-5".into(),
        task_id: "task-12".into(),
        mode: "silent".into(),
        episode_id: None,
        candidates_checked: 2,
    };
    let off = Event::MemoryStamp {
        id: "agent-7".into(),
        task_id: "task-13".into(),
        mode: "off".into(),
        episode_id: None,
        candidates_checked: 0,
    };
    let mint = Event::MemoryMint {
        id: "agent-9".into(),
        task_id: "task-14".into(),
        episode_id: "ep-b2".into(),
    };
    let contradicted = Event::MemoryContradicted {
        id: "agent-11".into(),
        task_id: "task-15".into(),
        episode_id: "ep-c3".into(),
    };
    for e in [&injected, &silent, &off, &mint, &contradicted] {
        j.append(e).unwrap();
    }
    assert_eq!(
        replay(&path).unwrap(),
        vec![injected, silent, off, mint, contradicted]
    );
}

/// The digest a `Blessed` row carries is of the profile's **bytes**, so a
/// human with `sha256sum` can check the row's path claim against the file
/// (design §5). Pins that the byte-taking helper and the str-taking one
/// agree, since the daemon hashes bytes and every test expectation is
/// written against the text.
#[test]
fn sha256_of_bytes_and_of_str_agree() {
    assert_eq!(sha256_hex_bytes(b"abc"), sha256_hex("abc"));
    assert_eq!(sha256_hex_bytes(b"abc").len(), 64);
    assert_ne!(sha256_hex_bytes(b"abc"), sha256_hex_bytes(b"abd"));
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

/// Turn-5 spec §3 compat pin: a pre-turn-5 `TaskStep` row carries no `args`
/// key at all (exactly what every row journaled before this field existed
/// looks like on disk), and must still deserialize, with `args` defaulting
/// to empty — never an error, never a synthesized guess at what the
/// arguments were.
#[test]
fn task_step_with_no_args_key_deserializes_with_empty_args() {
    let line = r#"{"event":"TaskStep","id":"a112","step":1,"verb":"read","outcome":"read 109 bytes","duration_ms":0}"#;
    let event: Event = serde_json::from_str(line).expect("a pre-turn-5 TaskStep line must parse");
    match event {
        Event::TaskStep { args, verb, .. } => {
            assert!(args.is_empty());
            assert_eq!(verb, "read");
        }
        other => panic!("expected TaskStep, got {other:?}"),
    }
}

/// Turn-5 spec §3 compat pin, the `CodecFixture` half: a pre-turn-5 row
/// carries no `agent` key, and must deserialize with `agent` defaulting to
/// `None` — the ordinal join stays the only join for old journals; the new
/// keyed join is only available where the writer knew to record it.
#[test]
fn codec_fixture_with_no_agent_key_deserializes_as_none() {
    let line = r#"{"event":"CodecFixture","model":"m1","fixture_set":"codec-tasks-v1","fixture":"py-mean-off-by-one","codec":"search_replace","landed":true,"steps":3,"detail":"patched (lens: python)","expect":"patch"}"#;
    let event: Event = serde_json::from_str(line).unwrap();
    match event {
        Event::CodecFixture { agent, .. } => assert_eq!(agent, None),
        other => panic!("expected CodecFixture, got {other:?}"),
    }
}

/// New-writer round trip: `TaskStep.args` and `CodecFixture.agent` survive
/// append/replay unchanged, including the keyed join value itself
/// (`CodecFixture.agent == TaskStep.id`, turn-5 spec §3).
#[test]
fn task_step_args_and_codec_fixture_agent_round_trip() {
    let dir = std::env::temp_dir().join("bloomery-journal-turn5");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("j.jsonl");
    let _ = std::fs::remove_file(&path);
    let mut j = Journal::open(&path).unwrap();
    let e1 = Event::TaskStep {
        id: "a7".into(),
        step: 3,
        verb: "run".into(),
        outcome: "ran python3 exit 0".into(),
        duration_ms: 87,
        args: vec![
            "python3".into(),
            "-m".into(),
            "unittest".into(),
            "test_x.py".into(),
        ],
    };
    let e2 = Event::CodecFixture {
        model: "m1".into(),
        fixture_set: "codec-tasks-v4-mixed".into(),
        fixture: "v4-patch-run-py-01".into(),
        codec: "search_replace".into(),
        landed: true,
        steps: 4,
        detail: "patched (lens: python)".into(),
        expect: "patch".into(),
        agent: Some("a7".into()),
    };
    j.append(&e1).unwrap();
    j.append(&e2).unwrap();
    assert_eq!(replay(&path).unwrap(), vec![e1, e2]);
}
