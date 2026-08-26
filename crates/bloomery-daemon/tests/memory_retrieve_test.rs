//! Failing-first tests for `memory::retrieve` (memory-organ Task 4).
//!
//! Written before `src/memory/retrieve.rs` exists, per the task brief's
//! Step 1 — the whole file fails to compile (no such module) until Step 3
//! lands. That compile failure is this task's captured RED.
//!
//! Real workspaces (`fresh_dir`), real `Grant`s (the `ok_grant` JSON
//! pattern), and hand-built `EpisodeRecord`s whose `cited_files` point into
//! the workspace — copied from `task::registry::tests::fresh_dir`
//! (`registry.rs:271`) and `registry.rs:311`'s grant-JSON pattern.
//!
//! Per the Task 4 carry-note: stored `cited_files[].path` strings are
//! canonicalize-then-`display()` forms minted by the capture seam. These
//! hand-built episodes reproduce that shape directly with
//! `std::fs::canonicalize(..).display().to_string()` for existing files, and
//! `<canonical parent>.join(name)` for a path that must not exist yet
//! (spec §3's `absent` case) — the same "canonicalize what exists, join the
//! rest" shape the seam itself falls back to for a not-yet-existing target.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use bloomery_core::grant::Grant;
use bloomery_core::journal::sha256_hex_bytes;
use bloomery_daemon::memory::record::{
    goal_hash, CitedFile, EpisodeRecord, Fingerprint, RunEvidence,
};
use bloomery_daemon::memory::retrieve::retrieve;
use bloomery_daemon::memory::store::MemoryStore;

/// One fresh scratch dir per test, never shared, never reused across runs —
/// copied from `task::registry::tests::fresh_dir`'s pattern (`registry.rs:271`).
fn fresh_dir(tag: &str) -> PathBuf {
    static UNIQUE: AtomicU64 = AtomicU64::new(0);
    let unique = UNIQUE.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "bloomery-retrieve-{tag}-{}-{unique}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A `Grant` whose read (and write) roots are exactly `dir`, canonicalized —
/// the `ok_grant` JSON pattern (`registry.rs:311`).
fn grant_for(dir: &Path) -> Grant {
    let canon = std::fs::canonicalize(dir).unwrap();
    Grant::from_json(&format!(
        r#"{{"read_roots":["{s}"],"write_roots":["{s}"],"commands":[]}}"#,
        s = canon.display()
    ))
    .unwrap()
}

fn empty_store(dir: &Path) -> MemoryStore {
    MemoryStore::load(&dir.join("episodes.jsonl")).unwrap()
}

/// A minimal episode with every field filled; `episode_id`, `goal`,
/// `cited_files`, `status`, and `minted_at` are the axes the tests vary.
fn ep(
    episode_id: &str,
    goal: &str,
    cited: Vec<CitedFile>,
    status: &str,
    minted_at: u64,
) -> EpisodeRecord {
    EpisodeRecord {
        episode_id: episode_id.into(),
        goal_hash: goal_hash(goal),
        goal_text: goal.into(),
        cited_files: cited,
        landed_patches: vec![],
        run_evidence: RunEvidence {
            argv: vec!["cargo".into(), "test".into()],
            outcome: "PASSED".into(),
        },
        trajectory: vec!["step1".into()],
        minted_by_model: "test-model".into(),
        minted_by_envelope: "v1".into(),
        status: status.into(),
        contradicted_by: None,
        minted_at,
    }
}

const GOAL: &str = "implement the thing";

#[test]
fn exact_hit_injects_the_most_recently_verified_survivor() {
    let dir = fresh_dir("exact-hit");
    let file = dir.join("main.rs");
    std::fs::write(&file, b"fn main() {}").unwrap();
    let canon = std::fs::canonicalize(&file).unwrap();
    let hash = sha256_hex_bytes(b"fn main() {}");

    let cited = vec![CitedFile {
        path: canon.display().to_string(),
        fingerprint: Fingerprint::Sha256(hash),
    }];

    let mut store = empty_store(&dir);
    store
        .mint(ep("e1", GOAL, cited.clone(), "verified", 1), 10)
        .unwrap();
    store
        .mint(ep("e2", GOAL, cited, "verified", 2), 10)
        .unwrap();

    let grant = grant_for(&dir);
    let result = retrieve(&store, GOAL, &grant, &dir);

    assert_eq!(result.candidates_checked, 2);
    let injected = result.injected.expect("expected an injection");
    assert_eq!(
        injected.episode_id, "e2",
        "the most recently minted (greatest minted_at) survivor must win"
    );
}

#[test]
fn one_changed_byte_is_silence() {
    let dir = fresh_dir("one-byte");
    let file = dir.join("main.rs");
    std::fs::write(&file, b"fn main() {}").unwrap();
    let canon = std::fs::canonicalize(&file).unwrap();
    let original_hash = sha256_hex_bytes(b"fn main() {}");

    let cited = vec![CitedFile {
        path: canon.display().to_string(),
        fingerprint: Fingerprint::Sha256(original_hash),
    }];

    let mut store = empty_store(&dir);
    store
        .mint(ep("e1", GOAL, cited, "verified", 1), 10)
        .unwrap();

    // Flip one byte in the actual file after minting — the fingerprint gate
    // (spec §3) must catch this.
    std::fs::write(&file, b"fn main() {1}").unwrap();

    let grant = grant_for(&dir);
    let result = retrieve(&store, GOAL, &grant, &dir);

    assert_eq!(result.candidates_checked, 1);
    assert!(
        result.injected.is_none(),
        "a single changed byte must silently disqualify the episode"
    );
}

#[test]
fn absent_expectation_matches_only_a_missing_file() {
    let dir = fresh_dir("absent");
    let canon_dir = std::fs::canonicalize(&dir).unwrap();
    let grant = grant_for(&dir);
    let mut store = empty_store(&dir);

    // Scenario A: Absent expectation, but the cited path exists -> silent.
    let existing = canon_dir.join("ghost-a.txt");
    std::fs::write(&existing, b"i exist").unwrap();
    let cited_exists = vec![CitedFile {
        path: existing.display().to_string(),
        fingerprint: Fingerprint::Absent,
    }];
    store
        .mint(ep("e-exists", GOAL, cited_exists, "verified", 1), 10)
        .unwrap();
    let result = retrieve(&store, GOAL, &grant, &dir);
    assert!(
        result.injected.is_none(),
        "absent expectation against an existing file must be silent"
    );

    // Scenario B: Absent expectation, and the cited path really is missing
    // -> hit. A different goal keeps this out of scenario A's candidate
    // pool.
    let missing = canon_dir.join("ghost-b.txt"); // never created
    let goal_b = "implement a different thing";
    let cited_missing = vec![CitedFile {
        path: missing.display().to_string(),
        fingerprint: Fingerprint::Absent,
    }];
    store
        .mint(ep("e-missing", goal_b, cited_missing, "verified", 1), 10)
        .unwrap();
    let result_b = retrieve(&store, goal_b, &grant, &dir);
    assert_eq!(
        result_b.injected.map(|e| e.episode_id),
        Some("e-missing".to_string()),
        "absent expectation matching a truly missing file must inject"
    );
}

#[test]
fn grant_not_covering_a_cited_path_is_silence() {
    let dir_a = fresh_dir("grant-a");
    let dir_b = fresh_dir("grant-b");
    let file_b = dir_b.join("main.rs");
    std::fs::write(&file_b, b"fn main() {}").unwrap();
    let canon_b = std::fs::canonicalize(&file_b).unwrap();
    let hash = sha256_hex_bytes(b"fn main() {}");

    let cited = vec![CitedFile {
        path: canon_b.display().to_string(),
        fingerprint: Fingerprint::Sha256(hash),
    }];

    let mut store = empty_store(&dir_a);
    store
        .mint(ep("e1", GOAL, cited, "verified", 1), 10)
        .unwrap();

    // The grant is rooted only at dir_a — dir_b's file is never in bounds,
    // even though its bytes match exactly.
    let grant = grant_for(&dir_a);
    let result = retrieve(&store, GOAL, &grant, &dir_a);

    assert_eq!(result.candidates_checked, 1);
    assert!(
        result.injected.is_none(),
        "a cited path outside the grant's read roots must be silent"
    );
}

#[test]
fn contradicted_is_silence() {
    let dir = fresh_dir("contradicted");
    let file = dir.join("main.rs");
    std::fs::write(&file, b"fn main() {}").unwrap();
    let canon = std::fs::canonicalize(&file).unwrap();
    let hash = sha256_hex_bytes(b"fn main() {}");

    let cited = vec![CitedFile {
        path: canon.display().to_string(),
        fingerprint: Fingerprint::Sha256(hash),
    }];

    let mut store = empty_store(&dir);
    store
        .mint(ep("e1", GOAL, cited, "verified", 1), 10)
        .unwrap();
    assert!(store.mark_contradicted("e1", "task-9").unwrap());

    let grant = grant_for(&dir);
    let result = retrieve(&store, GOAL, &grant, &dir);

    assert_eq!(result.candidates_checked, 1);
    assert!(
        result.injected.is_none(),
        "a contradicted episode must never be injected"
    );
}

#[test]
fn unreadable_cited_file_is_silence_not_error() {
    let dir = fresh_dir("unreadable");
    let subdir = dir.join("a_directory");
    std::fs::create_dir(&subdir).unwrap();
    let canon = std::fs::canonicalize(&subdir).unwrap();

    // The cited path exists (as a directory), sits inside the grant's read
    // roots, but can never be read as file bytes — `std::fs::read` on a
    // directory fails. This must disqualify the candidate silently, not
    // panic or bubble up an error (spec §7).
    let cited = vec![CitedFile {
        path: canon.display().to_string(),
        fingerprint: Fingerprint::Sha256("deadbeef".into()),
    }];

    let mut store = empty_store(&dir);
    store
        .mint(ep("e1", GOAL, cited, "verified", 1), 10)
        .unwrap();

    let grant = grant_for(&dir);
    let result = retrieve(&store, GOAL, &grant, &dir);

    assert_eq!(result.candidates_checked, 1);
    assert!(
        result.injected.is_none(),
        "an unreadable cited path must be silence, not an error"
    );
}
