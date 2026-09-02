//! The memory organ under conditions that must NOT be blamed on the episode.
//!
//! An episode deleted mid-task is not journaled as contradicted; a task
//! ending in an infra error does not contradict; an oversized memory block is
//! skipped and stamped silent; and a store I/O failure leaves the task result
//! intact and terminal. The common thread: the organ failing must never
//! corrupt the task's own verdict, and the task failing for its own reasons
//! must never be charged to the memory.
//!
//! Split out of `memory_task_test.rs` on 2026-09-01 (slice D).

mod common;

use std::path::Path;
use std::sync::{Arc, Mutex};

use bloomery_core::journal::{replay, sha256_hex_bytes, Event};
use bloomery_daemon::memory::record::{
    episode_id, goal_hash, CitedFile, EpisodeRecord, Fingerprint, RunEvidence, StoredPatch,
};
use bloomery_daemon::memory::render::render_memory_block;
use bloomery_daemon::memory::store::MemoryStore;
use bloomery_daemon::memory::MEMORY_BLOCK_MAX_BYTES;
use bloomery_daemon::task::{TaskRegistry, TaskStatus};
use common::memory::{
    build_pager, contradicted_ids, degraded_reasons, fresh_dir, memory_ctx, memory_prompts,
    mint_ids, poll_to_terminal, scripted, spec_for, store_path, BEFORE,
};

use common::memory_task::{drive, fixing_turns, sandbox, stamp_for};

/// Blocks until `task_id`'s `MemoryStamp` row is on disk, and returns it.
///
/// The stamp is appended between retrieval and the pager lock, so its
/// appearance is the one observable that says "this task has retrieved and
/// has not yet run" — which is what
/// [`an_injected_episode_deleted_mid_task_is_not_journaled_as_contradicted`]
/// needs to interleave against, without a sleep-and-hope.
fn await_stamp(journal_path: &Path, task_id: &str) -> (String, Option<String>, u32) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    while std::time::Instant::now() < deadline {
        if let Ok(events) = replay(journal_path) {
            if events
                .iter()
                .any(|e| matches!(e, Event::MemoryStamp { task_id: t, .. } if t == task_id))
            {
                return stamp_for(&events, task_id);
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    panic!("task {task_id} never wrote a MemoryStamp row");
}

/// Controller ruling R-PF-2: a `MemoryContradicted` row is journaled only
/// when `MemoryStore::mark_contradicted` actually changed a row
/// (`Ok(true)`). An operator's `DELETE /memory/{id}` (spec §6 — "the
/// operator's eviction right is part of the organ's trust story") can land
/// while a task that was injected with that id is still running; the store
/// then has nothing to mark, and journaling a contradiction anyway would
/// fabricate store history — a replay would see an episode change status
/// when no row ever did.
///
/// **Deterministic, not raced.** The worker retrieves and stamps *before* it
/// takes the pager lock (`task/registry.rs`'s pipeline, steps 1-2 vs. 5), so
/// this test holds the pager lock itself, waits for the stamp to land, and
/// deletes inside that window. The scripted turns end without a verifying
/// run, so the contradiction arm really is reached — without that, the test
/// would pass for the wrong reason.
#[test]
fn an_injected_episode_deleted_mid_task_is_not_journaled_as_contradicted() {
    let dir = fresh_dir("memtask", "deleted");
    let (sb, grant) = sandbox(&dir);
    let mut replies = fixing_turns();
    replies.push(scripted("<action verb=\"read\" path=\"a.py\">\n</action>"));
    replies.push(scripted(
        "<action verb=\"done\">\nlooks fine to me\n</action>",
    ));
    let (pager, agent_id) = build_pager(&dir, replies);
    let pager = Arc::new(Mutex::new(pager));
    let registry = TaskRegistry::new();
    let journal_path = dir.join("tasks.jsonl");
    let ctx = memory_ctx(&dir, true, false);
    let goal = "make a.py say two";

    let (_, first) = drive(
        &registry,
        &pager,
        &agent_id,
        spec_for(goal, &grant, &sb),
        &journal_path,
        Some(Arc::clone(&ctx)),
    );
    assert_eq!(first.status, TaskStatus::Done, "{first:?}");
    let episode_id = mint_ids(&replay(&journal_path).unwrap())[0].1.clone();

    std::fs::write(sb.join("a.py"), BEFORE).unwrap();
    let guard = pager.lock().expect("the pager mutex is healthy");
    let task_id = registry.spawn_task(
        Arc::clone(&pager),
        agent_id.clone(),
        spec_for(goal, &grant, &sb),
        journal_path.clone(),
        Some(Arc::clone(&ctx)),
    );
    // Retrieval and the stamp are done; the worker is now parked on the
    // pager lock this thread holds.
    assert_eq!(
        await_stamp(&journal_path, &task_id),
        ("injected".to_string(), Some(episode_id.clone()), 1),
    );
    {
        let store = ctx
            .store
            .as_ref()
            .expect("an operational organ has a store");
        let mut store = store.lock().expect("the store mutex is healthy");
        assert!(
            store.delete(&episode_id).unwrap(),
            "the operator deletes the very id this task was shown"
        );
    }
    drop(guard);

    let result = poll_to_terminal(&registry, &task_id);
    assert_eq!(result.status, TaskStatus::Done, "{result:?}");
    let events = replay(&journal_path).unwrap();
    assert!(
        contradicted_ids(&events).is_empty(),
        "no store row changed, so no contradiction may be journaled: {events:?}"
    );
    assert!(
        MemoryStore::load(&store_path(&dir))
            .unwrap()
            .episodes()
            .next()
            .is_none(),
        "the operator's deletion stands"
    );
}

/// Controller ruling (2026-08-26): **contradiction fires only on a SCORED
/// terminal status.** `TaskStatus::Error` is bloomery's infrastructure
/// bucket — substrate faults, journal failures, caught panics, a poisoned
/// pager — and the G4 protocol
/// (`docs/superpowers/evidence/2026-08-15-g4-protocol.md` §3) already
/// classifies it as "an infrastructure abort … the model is *unmeasured*".
/// Spec §5's "fails its own verification" requires a measurement, and an
/// unmeasured task made none, so the injected episode STANDS: no
/// `MemoryContradicted` row, and its stored status is still `verified`.
///
/// **The infra failure is produced deterministically, not simulated.** A
/// helper thread panics while holding the pager's mutex, which poisons it;
/// `spawn_task`'s worker then takes its `Err(_)` lock arm and returns
/// `TaskStatus::Error` — the real code path, not a hand-built result. The
/// worker retrieves and stamps *before* it touches the pager lock, so the
/// episode is genuinely injected first: without that, this test would pass
/// for the wrong reason. One panic message on stderr from the poisoning
/// thread is expected output.
#[test]
fn an_injected_task_that_ends_in_infra_error_does_not_contradict() {
    let dir = fresh_dir("memtask", "infra-error");
    let (sb, grant) = sandbox(&dir);
    let (pager, agent_id) = build_pager(&dir, fixing_turns());
    let pager = Arc::new(Mutex::new(pager));
    let registry = TaskRegistry::new();
    let journal_path = dir.join("tasks.jsonl");
    let ctx = memory_ctx(&dir, true, false);
    let goal = "make a.py say two";

    let (_, first) = drive(
        &registry,
        &pager,
        &agent_id,
        spec_for(goal, &grant, &sb),
        &journal_path,
        Some(Arc::clone(&ctx)),
    );
    assert_eq!(first.status, TaskStatus::Done, "{first:?}");
    let episode_id = mint_ids(&replay(&journal_path).unwrap())[0].1.clone();

    // Poison the pager mutex: a thread that panics while holding the guard.
    std::fs::write(sb.join("a.py"), BEFORE).unwrap();
    let poisoner = Arc::clone(&pager);
    let joined = std::thread::spawn(move || {
        let _guard = poisoner.lock().expect("the pager is healthy until now");
        panic!("deliberate: poisons the pager so the worker takes its infra-abort arm");
    })
    .join();
    assert!(joined.is_err(), "the poisoning thread must have panicked");
    assert!(
        pager.lock().is_err(),
        "the pager mutex must now be poisoned"
    );

    let (task_id, result) = drive(
        &registry,
        &pager,
        &agent_id,
        spec_for(goal, &grant, &sb),
        &journal_path,
        Some(Arc::clone(&ctx)),
    );
    assert_eq!(
        result.status,
        TaskStatus::Error,
        "the poisoned pager must produce bloomery's infra bucket: {result:?}"
    );

    let events = replay(&journal_path).unwrap();
    // The episode really was injected — otherwise there would be nothing to
    // contradict and the assertion below would be vacuous.
    assert_eq!(
        stamp_for(&events, &task_id),
        ("injected".to_string(), Some(episode_id.clone()), 1),
    );
    assert!(
        contradicted_ids(&events).is_empty(),
        "an unmeasured task falsifies nothing: {events:?}"
    );

    let store = MemoryStore::load(&store_path(&dir)).unwrap();
    let ep = store
        .episodes()
        .find(|e| e.episode_id == episode_id)
        .expect("the episode is still in the store");
    assert_eq!(ep.status, "verified", "the injected episode STANDS");
    assert_eq!(ep.contradicted_by, None);
}

/// The `MEMORY_BLOCK_MAX_BYTES` skip path (controller ruling, Task 6): a
/// matching, verified episode whose rendered block exceeds the injection
/// bound is **not** injected — the task is stamped `"silent"` and runs
/// memory-off, because an oversized block could push it into
/// `WindowExhausted` where memory-off would have finished (spec §7: the
/// organ must never damage the task).
///
/// The oversized episode is hand-minted straight into the store rather than
/// produced by landing a >16 KiB patch through the real executor: the branch
/// under test reads `render_memory_block`'s output length and nothing else,
/// so driving a giant patch through `exec_patch` would make the test slow
/// and mostly about the executor. Everything retrieval gates on is real —
/// the goal hash, the canonical cited path, and the sha256 of the actual
/// workspace bytes — so the episode is a genuine survivor that only the size
/// bound rejects.
#[test]
fn an_oversized_memory_block_is_skipped_and_stamped_silent() {
    let dir = fresh_dir("memtask", "oversize");
    let (sb, grant) = sandbox(&dir);
    let (pager, agent_id) = build_pager(
        &dir,
        vec![scripted("<action verb=\"done\">\nnothing to do\n</action>")],
    );
    let pager = Arc::new(Mutex::new(pager));
    let registry = TaskRegistry::new();
    let journal_path = dir.join("tasks.jsonl");
    let ctx = memory_ctx(&dir, true, false);
    let goal = "make a.py say two";

    let cited_path = sb.join("a.py").display().to_string();
    let cited = vec![CitedFile {
        path: cited_path.clone(),
        fingerprint: Fingerprint::Sha256(sha256_hex_bytes(BEFORE)),
    }];
    let hash = goal_hash(goal);
    let record = EpisodeRecord {
        episode_id: episode_id(&hash, &cited),
        goal_hash: hash,
        goal_text: goal.to_string(),
        cited_files: cited,
        landed_patches: vec![StoredPatch {
            path: cited_path,
            codec: "whole_file".to_string(),
            body: format!("x = {}", "9".repeat(20_000)),
        }],
        run_evidence: RunEvidence {
            argv: vec!["python3".into(), "-c".into(), "pass".into()],
            outcome: "ran python3 exit 0".into(),
        },
        trajectory: vec!["read".into(), "patch".into(), "run".into(), "done".into()],
        minted_by_model: "m".into(),
        minted_by_envelope: "V1".into(),
        status: "verified".into(),
        contradicted_by: None,
        minted_at: 1,
    };
    let oversized_id = record.episode_id.clone();
    let rendered = render_memory_block(&record).len();
    assert!(
        rendered > MEMORY_BLOCK_MAX_BYTES,
        "the fixture must actually be oversized: {rendered} bytes"
    );
    {
        let store = ctx
            .store
            .as_ref()
            .expect("an operational organ has a store");
        let mut store = store.lock().expect("the store mutex is healthy");
        store.mint(record, 64).unwrap();
    }

    let (task_id, result) = drive(
        &registry,
        &pager,
        &agent_id,
        spec_for(goal, &grant, &sb),
        &journal_path,
        Some(Arc::clone(&ctx)),
    );

    // The task runs normally — the organ declining to speak costs it nothing.
    assert_eq!(result.status, TaskStatus::Done, "{result:?}");
    let events = replay(&journal_path).unwrap();
    assert_eq!(
        stamp_for(&events, &task_id),
        ("silent".to_string(), None, 1),
        "the candidate is checked and then declined on size, not on match"
    );
    assert_eq!(
        memory_prompts(&dir),
        0,
        "no prompt may carry a block the organ declined to inject"
    );
    // Silence alone would leave an operator unable to tell a size skip from a
    // fingerprint miss, so the skip names itself.
    let reasons = degraded_reasons(&events);
    assert!(
        reasons
            .iter()
            .any(|r| r.contains(&oversized_id) && r.contains("injection bound")),
        "the skip must name the episode and the bound: {reasons:?}"
    );
}

/// Spec §7's "Mint-time store IO failure: journal a warning row; the task's
/// own result is unaffected", end to end — and the reachable half of the
/// review finding that put both organ regions under `catch_unwind`: whatever
/// the organ does wrong, **step 7's registry write still runs and the task's
/// earned result survives intact**.
///
/// The failure is real and deterministic, not mocked: the store loads
/// normally, then its file is replaced by a *directory* before the task
/// runs, so `MemoryStore::mint`'s append (`OpenOptions::append(true).open`)
/// gets a genuine `io::Error` from the OS. The task is one that clears the
/// mint bar, so the organ definitely reaches the failing call — a task with
/// nothing to mint would pass this test vacuously.
///
/// This does not exercise the *panic* path (every organ function is
/// unwrap-free and returns `Result` for each fallible operation, so an
/// integration test cannot honestly make one panic without a test-only
/// production seam — see `registry.rs`'s
/// `contained_catches_a_panic_journals_it_and_lets_the_caller_continue`,
/// which pins the guard itself). It does pin the property the guard exists
/// to protect, on the one organ-failure path that is reachable from outside.
#[test]
fn a_store_io_failure_leaves_the_task_result_intact_and_terminal() {
    let dir = fresh_dir("memtask", "store-io-fail");
    let (sb, grant) = sandbox(&dir);
    let (pager, agent_id) = build_pager(&dir, fixing_turns());
    let pager = Arc::new(Mutex::new(pager));
    let registry = TaskRegistry::new();
    let journal_path = dir.join("tasks.jsonl");
    let ctx = memory_ctx(&dir, true, false);

    // The store loaded fine; now make every future append fail at the OS
    // level by putting a directory where its file belongs.
    let store_file = store_path(&dir);
    let _ = std::fs::remove_file(&store_file);
    std::fs::create_dir_all(&store_file).unwrap();
    assert!(store_file.is_dir());

    let (task_id, result) = drive(
        &registry,
        &pager,
        &agent_id,
        spec_for("make a.py say two", &grant, &sb),
        &journal_path,
        Some(Arc::clone(&ctx)),
    );

    // The task is terminal (never wedged at `Running`) and its evidence is
    // whole — the organ discarded nothing.
    assert_eq!(result.status, TaskStatus::Done, "{result:?}");
    assert_eq!(result.steps.len(), 4, "{:?}", result.steps);
    assert!(result.steps.iter().all(|s| !s.failed), "{:?}", result.steps);
    assert_eq!(result.summary.as_deref(), Some("fixed"));
    assert_eq!(result.landed_patches.len(), 1);
    // And a later poll reads the same terminal entry back, which is the
    // property a wedged worker would break.
    let polled = registry.get(&task_id).expect("the entry is still there");
    assert_eq!(polled.status, TaskStatus::Done);
    assert_eq!(polled.steps.len(), 4);

    let events = replay(&journal_path).unwrap();
    // The organ tried and failed, loudly: no mint row, and a Degraded row
    // naming the task.
    assert!(
        mint_ids(&events).is_empty(),
        "the append failed, so nothing was minted: {events:?}"
    );
    let reasons = degraded_reasons(&events);
    assert!(
        reasons
            .iter()
            .any(|r| r.contains("could not mint") && r.contains(&task_id)),
        "the failure must be on the record, naming the task: {reasons:?}"
    );
}
