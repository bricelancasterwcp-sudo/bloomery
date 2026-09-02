//! The memory organ wired into the task loop: minting, injection, and the
//! contradiction path.
//!
//! A verified task mints an episode and the journal carries both stamp and
//! mint; a byte-identical repeat is injected while a stranger stays silent;
//! an injected task that then fails contradicts the episode, and the next
//! repeat is silent. `memory = none` and a disabled organ stamp off and never
//! touch the store at all.
//!
//! **Split 2026-09-01** (carried-debt slice D): this file was 995 lines. The
//! cases where an injected episode must NOT be blamed are in
//! `memory_task_resilience_test.rs`; fixtures shared with the refalsify tests
//! are in `tests/common/memory.rs`.

mod common;

use std::sync::{Arc, Mutex};

use bloomery_core::journal::{replay, sha256_hex_bytes};
use bloomery_daemon::memory::store::MemoryStore;
use bloomery_daemon::task::{TaskRegistry, TaskStatus};
use common::memory::{
    build_pager, contradicted_ids, fresh_dir, memory_ctx, memory_prompts, mint_ids, scripted,
    spec_for, store_path, BEFORE,
};

use common::memory_task::{drive, fixing_turns, sandbox, stamp_for};

/// Spec §2 + §4: a task that clears the mint bar stores exactly one verified
/// episode citing the pre-patch bytes of every file it touched, and the
/// journal carries the pair that makes the task → store trail walkable in
/// both directions — a `MemoryStamp` (mode `silent`, because a first run has
/// no candidates to match) and a `MemoryMint`.
#[test]
fn verified_task_mints_and_journal_carries_stamp_and_mint() {
    let dir = fresh_dir("memtask", "mint");
    let (sb, grant) = sandbox(&dir);
    let (pager, agent_id) = build_pager(&dir, fixing_turns());
    let pager = Arc::new(Mutex::new(pager));
    let registry = TaskRegistry::new();
    let journal_path = dir.join("tasks.jsonl");
    let ctx = memory_ctx(&dir, true, false);

    let (task_id, result) = drive(
        &registry,
        &pager,
        &agent_id,
        spec_for("make a.py say two", &grant, &sb),
        &journal_path,
        Some(Arc::clone(&ctx)),
    );
    assert_eq!(result.status, TaskStatus::Done, "{result:?}");

    // The store, re-read from disk: the mint is durable, not just in-memory.
    let store = MemoryStore::load(&store_path(&dir)).unwrap();
    let episodes: Vec<_> = store.episodes().collect();
    assert_eq!(episodes.len(), 1, "{episodes:?}");
    let ep = episodes[0];
    assert_eq!(ep.status, "verified");
    assert_eq!(ep.cited_files.len(), 1, "{:?}", ep.cited_files);
    assert_eq!(
        ep.cited_files[0].path,
        sb.join("a.py").display().to_string()
    );
    assert_eq!(
        ep.cited_files[0].fingerprint,
        bloomery_daemon::memory::record::Fingerprint::Sha256(sha256_hex_bytes(BEFORE)),
        "the citation must be the PRE-patch bytes"
    );
    // Step 4's provenance pair, recorded and never compared (spec §2): the
    // model comes from `Pager::agent_model`, the envelope from the spec.
    assert_eq!(ep.minted_by_model, "m");
    assert_eq!(ep.minted_by_envelope, "V1");
    assert!(ep.minted_at > 0, "a minted episode carries a mint clock");

    let events = replay(&journal_path).unwrap();
    assert_eq!(
        stamp_for(&events, &task_id),
        ("silent".to_string(), None, 0),
        "a first run has nothing to retrieve"
    );
    assert_eq!(
        mint_ids(&events),
        vec![(task_id, ep.episode_id.clone())],
        "exactly one mint row, naming this task and this episode"
    );
    assert!(contradicted_ids(&events).is_empty());
}

/// Spec §3/§4: the exact repeat of a verified goal against byte-identical
/// starting files is injected; a stranger goal and a drifted cited file are
/// both silent. The three negatives sit in one test because they share the
/// one store the first task minted into — separating them would prove only
/// that an empty store is silent.
#[test]
fn exact_repeat_is_injected_and_a_stranger_is_silent() {
    let dir = fresh_dir("memtask", "repeat");
    let (sb, grant) = sandbox(&dir);
    let mut replies = fixing_turns();
    replies.extend(fixing_turns());
    replies.push(scripted("<action verb=\"done\">\nnothing to do\n</action>"));
    replies.push(scripted("<action verb=\"done\">\nnothing to do\n</action>"));
    let (pager, agent_id) = build_pager(&dir, replies);
    let pager = Arc::new(Mutex::new(pager));
    let registry = TaskRegistry::new();
    let journal_path = dir.join("tasks.jsonl");
    let ctx = memory_ctx(&dir, true, false);
    let goal = "make a.py say two";

    // 1. Mint.
    let (first_id, first) = drive(
        &registry,
        &pager,
        &agent_id,
        spec_for(goal, &grant, &sb),
        &journal_path,
        Some(Arc::clone(&ctx)),
    );
    assert_eq!(first.status, TaskStatus::Done, "{first:?}");
    let minted = mint_ids(&replay(&journal_path).unwrap());
    assert_eq!(minted.len(), 1, "{minted:?}");
    let episode_id = minted[0].1.clone();
    let prompts_before = memory_prompts(&dir);
    assert_eq!(
        prompts_before, 0,
        "nothing was injectable yet, so no prompt may carry a memory block"
    );

    // 2. Reset the workspace to its pre-task bytes and repeat the same goal.
    std::fs::write(sb.join("a.py"), BEFORE).unwrap();
    let (repeat_id, repeat) = drive(
        &registry,
        &pager,
        &agent_id,
        spec_for(goal, &grant, &sb),
        &journal_path,
        Some(Arc::clone(&ctx)),
    );
    assert_eq!(repeat.status, TaskStatus::Done, "{repeat:?}");
    let events = replay(&journal_path).unwrap();
    assert_eq!(
        stamp_for(&events, &repeat_id),
        ("injected".to_string(), Some(episode_id.clone()), 1),
        "an exact repeat against byte-identical files is injected"
    );
    assert_ne!(first_id, repeat_id);
    // The stamp says the block was injected; this is what proves the model
    // was actually shown it. Without this, a pipeline that stamped
    // `"injected"` and forgot to set `TaskSpec::memory_block` would pass
    // every other assertion in this file.
    assert!(
        memory_prompts(&dir) > 0,
        "the injected task's prompts must carry the rendered memory block"
    );
    // Spec §5: "A repeat that succeeds refreshes `verified`" — a task that
    // received an episode and then cleared the bar must never contradict the
    // very episode it just confirmed.
    assert!(
        contradicted_ids(&events).is_empty(),
        "a verifying repeat refreshes, it never contradicts: {events:?}"
    );
    // A verified repeat refreshes the same identity (spec §5).
    assert_eq!(
        mint_ids(&events)
            .into_iter()
            .map(|(_, id)| id)
            .collect::<Vec<_>>(),
        vec![episode_id.clone(), episode_id.clone()],
    );

    // 3. A stranger goal: no candidate even reaches the fingerprint gate.
    let (stranger_id, stranger) = drive(
        &registry,
        &pager,
        &agent_id,
        spec_for("something else entirely", &grant, &sb),
        &journal_path,
        Some(Arc::clone(&ctx)),
    );
    assert_eq!(stranger.status, TaskStatus::Done, "{stranger:?}");
    assert_eq!(
        stamp_for(&replay(&journal_path).unwrap(), &stranger_id),
        ("silent".to_string(), None, 0),
        "a stranger goal hashes to nothing in the store"
    );

    // 4. One drifted byte in the one cited file: a candidate is checked and
    //    disqualified, so `candidates_checked` is 1 and the mode is silent.
    std::fs::write(sb.join("a.py"), b"x = 9\n").unwrap();
    let (drifted_id, drifted) = drive(
        &registry,
        &pager,
        &agent_id,
        spec_for(goal, &grant, &sb),
        &journal_path,
        Some(Arc::clone(&ctx)),
    );
    assert_eq!(drifted.status, TaskStatus::Done, "{drifted:?}");
    assert_eq!(
        stamp_for(&replay(&journal_path).unwrap(), &drifted_id),
        ("silent".to_string(), None, 1),
        "a drifted cited file is checked, then disqualified"
    );
}

/// Spec §5's passive falsification: a task that RECEIVED an injected episode
/// and then failed its own verification (here: no productive run after any
/// patch) marks that episode `contradicted`, journals the row, and the next
/// repeat of the same goal is silent — the status gate, not the fingerprint
/// gate, is what silences it, since the workspace bytes never moved.
#[test]
fn injected_then_failed_contradicts_and_next_repeat_is_silent() {
    let dir = fresh_dir("memtask", "contradict");
    let (sb, grant) = sandbox(&dir);
    let mut replies = fixing_turns();
    replies.push(scripted("<action verb=\"read\" path=\"a.py\">\n</action>"));
    replies.push(scripted(
        "<action verb=\"done\">\nlooks fine to me\n</action>",
    ));
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

    // The repeat is injected — and then ends `Done` with no patch and no
    // verifying run, which is exactly the shape spec §5 calls a failed
    // verification.
    std::fs::write(sb.join("a.py"), BEFORE).unwrap();
    let (failed_id, failed) = drive(
        &registry,
        &pager,
        &agent_id,
        spec_for(goal, &grant, &sb),
        &journal_path,
        Some(Arc::clone(&ctx)),
    );
    assert_eq!(failed.status, TaskStatus::Done, "{failed:?}");
    let events = replay(&journal_path).unwrap();
    assert_eq!(
        stamp_for(&events, &failed_id),
        ("injected".to_string(), Some(episode_id.clone()), 1),
    );
    assert_eq!(
        contradicted_ids(&events),
        vec![(failed_id.clone(), episode_id.clone())],
        "the injected episode is contradicted by the task that received it"
    );
    assert_eq!(
        mint_ids(&events).len(),
        1,
        "a contradicting task mints nothing"
    );

    let store = MemoryStore::load(&store_path(&dir)).unwrap();
    let ep = store
        .episodes()
        .find(|e| e.episode_id == episode_id)
        .expect("the episode survives contradiction");
    assert_eq!(ep.status, "contradicted");
    assert_eq!(ep.contradicted_by.as_deref(), Some(failed_id.as_str()));

    // The bytes never drifted, so only the status gate can silence this.
    assert_eq!(std::fs::read(sb.join("a.py")).unwrap(), BEFORE);
    let (next_id, next) = drive(
        &registry,
        &pager,
        &agent_id,
        spec_for(goal, &grant, &sb),
        &journal_path,
        Some(Arc::clone(&ctx)),
    );
    assert_eq!(next.status, TaskStatus::Done, "{next:?}");
    assert_eq!(
        stamp_for(&replay(&journal_path).unwrap(), &next_id),
        ("silent".to_string(), None, 1),
        "a contradicted episode is never injected again"
    );
}

/// Spec §4's "a stamp for every spawned task" and §7's "the organ being
/// broken can only ever produce memory-off behavior": both the no-context
/// case and the config-off case stamp `off`, and neither reads or writes the
/// store — proven against tasks that would otherwise have minted.
#[test]
fn memory_none_and_disabled_stamp_off_and_never_touch_the_store() {
    let dir = fresh_dir("memtask", "off");
    let (sb, grant) = sandbox(&dir);
    let mut replies = fixing_turns();
    replies.extend(fixing_turns());
    let (pager, agent_id) = build_pager(&dir, replies);
    let pager = Arc::new(Mutex::new(pager));
    let registry = TaskRegistry::new();
    let journal_path = dir.join("tasks.jsonl");
    let goal = "make a.py say two";

    let (none_id, none_result) = drive(
        &registry,
        &pager,
        &agent_id,
        spec_for(goal, &grant, &sb),
        &journal_path,
        None,
    );
    assert_eq!(none_result.status, TaskStatus::Done, "{none_result:?}");
    assert_eq!(
        stamp_for(&replay(&journal_path).unwrap(), &none_id),
        ("off".to_string(), None, 0),
    );

    std::fs::write(sb.join("a.py"), BEFORE).unwrap();
    let disabled = memory_ctx(&dir, false, false);
    assert!(!disabled.operational());
    let (off_id, off_result) = drive(
        &registry,
        &pager,
        &agent_id,
        spec_for(goal, &grant, &sb),
        &journal_path,
        Some(disabled),
    );
    assert_eq!(off_result.status, TaskStatus::Done, "{off_result:?}");
    let events = replay(&journal_path).unwrap();
    assert_eq!(
        stamp_for(&events, &off_id),
        ("off".to_string(), None, 0),
        "a disabled organ is off, never silent — silence is a retrieval outcome"
    );

    assert!(
        mint_ids(&events).is_empty() && contradicted_ids(&events).is_empty(),
        "a memory-off task writes no store rows: {events:?}"
    );
    assert!(
        !store_path(&dir).exists(),
        "both tasks cleared the mint bar and neither may have created a store"
    );
}
