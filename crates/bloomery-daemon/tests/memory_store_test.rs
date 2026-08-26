//! Failing-first tests for `memory::store::MemoryStore` (memory-organ Task 2).
//!
//! Written before `src/memory/store.rs` exists, per the task brief's Step 1 —
//! the whole file fails to compile (no such module) until Step 3 lands. That
//! compile failure is this task's captured RED.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use bloomery_daemon::memory::record::{
    CitedFile, EpisodeRecord, Fingerprint, RunEvidence, StoredRow,
};
use bloomery_daemon::memory::store::MemoryStore;

/// One fresh scratch dir per test, never shared, never reused across runs —
/// copied from `task::registry::tests::fresh_dir`'s pattern (`registry.rs:271`):
/// a monotonic `AtomicU64` plus the process id disambiguates parallel test
/// binaries and repeated runs without touching a clock.
fn fresh_dir(tag: &str) -> PathBuf {
    static UNIQUE: AtomicU64 = AtomicU64::new(0);
    let unique = UNIQUE.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "bloomery-memstore-{tag}-{}-{unique}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A minimal verified episode: every field filled, `status` "verified",
/// `contradicted_by` `None`. `id_seed` and `goal_hash` are the two axes the
/// tests below vary; `minted_at` drives retention ordering.
fn ep(id_seed: &str, goal_hash: &str, minted_at: u64) -> EpisodeRecord {
    EpisodeRecord {
        episode_id: id_seed.into(),
        goal_hash: goal_hash.into(),
        goal_text: "implement the thing".into(),
        cited_files: vec![CitedFile {
            path: "/w/main.rs".into(),
            fingerprint: Fingerprint::Sha256("abc123".into()),
        }],
        landed_patches: vec![],
        run_evidence: RunEvidence {
            argv: vec!["cargo".into(), "test".into()],
            outcome: "PASSED".into(),
        },
        trajectory: vec!["step1".into()],
        minted_by_model: "test-model".into(),
        minted_by_envelope: "v1".into(),
        status: "verified".into(),
        contradicted_by: None,
        minted_at,
    }
}

#[test]
fn load_of_missing_file_is_an_empty_store() {
    let dir = fresh_dir("missing");
    // The parent dir doesn't exist yet either — load must create it (brief:
    // "Parent dir is created if missing"), not error, and a missing file is
    // an empty store (first boot), not an error either.
    let path = dir.join("nested").join("episodes.jsonl");

    let store = MemoryStore::load(&path).unwrap();

    let counts = store.counts();
    assert_eq!(counts.episodes, 0);
    assert_eq!(counts.verified, 0);
    assert_eq!(counts.contradicted, 0);
    assert_eq!(counts.parse_errors, 0);
    assert_eq!(store.episodes().count(), 0);
    assert!(
        path.parent().unwrap().is_dir(),
        "load must create the parent dir"
    );
}

#[test]
fn mint_then_reload_round_trips_last_writer_wins() {
    let dir = fresh_dir("mint-reload");
    let path = dir.join("episodes.jsonl");
    let mut store = MemoryStore::load(&path).unwrap();

    store.mint(ep("e1", "gh1", 1), 10).unwrap();
    store.mint(ep("e1", "gh1", 2), 10).unwrap();

    let reloaded = MemoryStore::load(&path).unwrap();
    assert_eq!(reloaded.counts().episodes, 1);
    let survivors: Vec<_> = reloaded.episodes().collect();
    assert_eq!(survivors.len(), 1);
    assert_eq!(survivors[0].episode_id, "e1");
    assert_eq!(
        survivors[0].minted_at, 2,
        "reload must see the refreshed row"
    );
}

#[test]
fn contradiction_survives_reload_and_delete_tombstones() {
    let dir = fresh_dir("contradiction");
    let path = dir.join("episodes.jsonl");
    let mut store = MemoryStore::load(&path).unwrap();

    store.mint(ep("e1", "gh1", 1), 10).unwrap();
    assert!(store.mark_contradicted("e1", "task-9").unwrap());

    let reloaded = MemoryStore::load(&path).unwrap();
    let survivors: Vec<_> = reloaded.episodes().collect();
    assert_eq!(survivors.len(), 1);
    assert_eq!(survivors[0].status, "contradicted");
    assert_eq!(survivors[0].contradicted_by.as_deref(), Some("task-9"));
    drop(reloaded);

    let mut for_delete = MemoryStore::load(&path).unwrap();
    assert!(for_delete.delete("e1").unwrap());
    let after_delete = MemoryStore::load(&path).unwrap();
    assert_eq!(after_delete.counts().episodes, 0);

    let mut after_delete = after_delete;
    assert!(
        !after_delete.delete("unknown").unwrap(),
        "unknown id deletes as false"
    );
}

#[test]
fn corrupt_lines_are_counted_never_fatal() {
    let dir = fresh_dir("corrupt");
    let path = dir.join("episodes.jsonl");

    let good1 = StoredRow::Episode(Box::new(ep("e1", "gh1", 1)));
    let good2 = StoredRow::Episode(Box::new(ep("e2", "gh2", 2)));
    let mut contents = String::new();
    contents.push_str(&serde_json::to_string(&good1).unwrap());
    contents.push('\n');
    contents.push_str("{not json");
    contents.push('\n');
    contents.push_str(&serde_json::to_string(&good2).unwrap());
    contents.push('\n');
    std::fs::write(&path, contents).unwrap();

    let store = MemoryStore::load(&path).unwrap();
    assert_eq!(
        store.counts().episodes,
        2,
        "both valid rows survive the corrupt one"
    );
    assert_eq!(
        store.counts().parse_errors,
        1,
        "the corrupt line is counted, not silently dropped"
    );

    // A count match alone can't catch a bug that swaps or corrupts survivor
    // data while preserving the total, so pin the exact surviving ids and
    // their field values against what was minted.
    let mut survivors: Vec<_> = store.episodes().collect();
    survivors.sort_by(|a, b| a.episode_id.cmp(&b.episode_id));
    assert_eq!(survivors.len(), 2);
    assert_eq!(survivors[0].episode_id, "e1");
    assert_eq!(survivors[0].goal_hash, "gh1");
    assert_eq!(survivors[0].minted_at, 1);
    assert_eq!(survivors[1].episode_id, "e2");
    assert_eq!(survivors[1].goal_hash, "gh2");
    assert_eq!(survivors[1].minted_at, 2);
}

#[test]
fn retention_evicts_contradicted_oldest_first_then_verified_oldest() {
    let dir = fresh_dir("retention");
    let path = dir.join("episodes.jsonl");
    let mut store = MemoryStore::load(&path).unwrap();
    let max_episodes = 2;

    store.mint(ep("v1", "gh-v1", 10), max_episodes).unwrap();
    store.mint(ep("c1", "gh-c1", 5), max_episodes).unwrap();
    assert!(store.mark_contradicted("c1", "task-x").unwrap());
    store.mint(ep("v2", "gh-v2", 20), max_episodes).unwrap();

    // After v1, c1, v2 the store holds 3 distinct ids over the cap of 2.
    // c1 is contradicted, so it is evicted regardless of age comparisons
    // against the verified survivors — contradicted status outranks age.
    let ids: Vec<String> = store.episodes().map(|e| e.episode_id.clone()).collect();
    assert_eq!(ids.len(), 2);
    assert!(
        !ids.contains(&"c1".to_string()),
        "contradicted c1 evicted first"
    );
    assert!(ids.contains(&"v1".to_string()));
    assert!(ids.contains(&"v2".to_string()));

    store.mint(ep("v3", "gh-v3", 30), max_episodes).unwrap();
    // No contradicted survivor remains, so the oldest verified (v1, t=10) is
    // evicted next, ahead of v2 (t=20).
    let ids: Vec<String> = store.episodes().map(|e| e.episode_id.clone()).collect();
    assert_eq!(ids.len(), 2);
    assert!(
        !ids.contains(&"v1".to_string()),
        "oldest verified v1 evicted next"
    );
    assert!(ids.contains(&"v2".to_string()));
    assert!(ids.contains(&"v3".to_string()));

    // Eviction is durable — tombstones, not in-memory only.
    let reloaded = MemoryStore::load(&path).unwrap();
    let mut reloaded_ids: Vec<String> = reloaded.episodes().map(|e| e.episode_id.clone()).collect();
    reloaded_ids.sort();
    assert_eq!(reloaded_ids, vec!["v2".to_string(), "v3".to_string()]);
}
