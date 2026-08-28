//! The memory organ's full-loop binding tests (memory-organ Task 7; spec
//! `docs/superpowers/specs/2026-08-26-memory-organ-design.md` §4/§5/§7, and
//! §8's "Journal: stamp rows for on/injected, on/silent, mint,
//! contradiction").
//!
//! Every test drives the REAL worker pipeline through
//! `TaskRegistry::spawn_task` — retrieve, stamp, inject, run, mint or
//! contradict — against a scripted `FakeSubstrate` and a REAL `exec_run`
//! spawning `python3 -c pass`. GPU-free, per spec §8 ("the whole loop
//! exercises GPU-free against `FakeSubstrate` with scripted `<action>`
//! turns"), but deliberately NOT executor-free: the mint bar reads a
//! completed run's own `" exit 0"` outcome
//! (`memory::mint::verifying_run`), so a faked run step would leave the one
//! predicate that gates every mint untested here.
//!
//! **Why `a.py` and a real interpreter.** `exec_patch` picks its landing
//! lens by extension and a `.py` file goes through `PythonLens`, which
//! shells out to `python3`; the granted command (`["python3","-c"]`) runs
//! through the real `exec_run`. Both need an interpreter on the box — the
//! same dependency `task_exec_patch_test.rs` and `task_loop_test.rs`
//! already carry.
//!
//! **Sequencing.** `FakeSubstrate` serves one FIFO reply queue for the whole
//! pager, and `spawn_task`'s worker holds the pager lock for its entire task
//! (see `task/registry.rs`'s module docs), so every test drives its tasks
//! strictly one at a time — spawn, poll to a terminal status, then spawn the
//! next — and scripts the concatenation of their turns in that order.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use bloomery_core::action::PatchCodec;
use bloomery_core::gguf::GgufMeta;
use bloomery_core::grant::Grant;
use bloomery_core::journal::{replay, sha256_hex_bytes, Event, Journal};
use bloomery_daemon::agents::ImageStore;
use bloomery_daemon::config::EnvelopeLens;
use bloomery_daemon::memory::record::{
    episode_id, goal_hash, CitedFile, EpisodeRecord, Fingerprint, RunEvidence, StoredPatch,
};
use bloomery_daemon::memory::render::render_memory_block;
use bloomery_daemon::memory::store::MemoryStore;
use bloomery_daemon::memory::{MemoryContext, MEMORY_BLOCK_MAX_BYTES};
use bloomery_daemon::pager::Pager;
use bloomery_daemon::task::{ExecBounds, TaskRegistry, TaskResult, TaskSpec, TaskStatus};
use bloomery_substrate::fake::FakeSubstrate;
use bloomery_substrate::Reply;

/// The bytes `a.py` carries before any task touches it — the pre-first-touch
/// fingerprint every minted episode in this file cites.
const BEFORE: &[u8] = b"x = 1\n";

/// A fresh, per-test tempdir — PID + atomic counter so parallel test threads
/// in one `cargo test` process never collide. Copied from
/// `task/registry.rs`'s test helpers.
fn fresh_dir(tag: &str) -> PathBuf {
    static UNIQUE: AtomicU64 = AtomicU64::new(0);
    let unique = UNIQUE.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "bloomery-memtask-{tag}-{}-{unique}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn meta() -> GgufMeta {
    GgufMeta {
        arch: "qwen2".into(),
        layers: 4,
        attention_layers: 4,
        kv_heads: 2,
        head_dim: 32,
        training_ctx: 65536,
        weights_bytes: 1000,
        recurrent_state_bytes: 0,
    }
}

fn build_pager(dir: &Path, replies: Vec<Reply>) -> (Pager<FakeSubstrate>, String) {
    let journal = Journal::open(&dir.join("pager.jsonl")).unwrap();
    let images = ImageStore::new(&dir.join("img")).unwrap();
    let mut fake = FakeSubstrate::new();
    for r in replies {
        fake.script_reply(r);
    }
    let mut pager = Pager::new(fake, journal, images, Box::new(|| Some(1024 * 1024 * 1024)));
    let gguf = dir.join("m.gguf");
    std::fs::write(&gguf, b"fake weights").unwrap();
    pager.register_model("m", &gguf, meta(), None).unwrap();
    let info = pager.create_agent("m", 100, None, 1_000_000).unwrap();
    (pager, info.id)
}

fn scripted(text: &str) -> Reply {
    Reply {
        text: text.to_string(),
        prompt_tokens: Some(8),
        completion_tokens: Some(4),
        duration_ms: 1,
    }
}

/// The four turns that clear the mint bar: touch `a.py`, land a patch, run a
/// granted command that exits 0 afterward, finish `Done` (spec §2).
fn fixing_turns() -> Vec<Reply> {
    vec![
        scripted("<action verb=\"read\" path=\"a.py\">\n</action>"),
        scripted("<action verb=\"patch\" path=\"a.py\">\nx = 2\n</action>"),
        scripted("<action verb=\"run\">\n[\"python3\", \"-c\", \"pass\"]\n</action>"),
        scripted("<action verb=\"done\">\nfixed\n</action>"),
    ]
}

/// A canonical sandbox under `dir` holding the planted `a.py`, plus a grant
/// scoped to it that also grants the `["python3","-c"]` command prefix.
fn sandbox(dir: &Path) -> (PathBuf, Grant) {
    let sb = dir.join("sandbox");
    std::fs::create_dir_all(&sb).unwrap();
    let sb = std::fs::canonicalize(&sb).unwrap();
    std::fs::write(sb.join("a.py"), BEFORE).unwrap();
    let grant = Grant::from_json(&format!(
        r#"{{"read_roots":["{s}"],"write_roots":["{s}"],"commands":[["python3","-c"]]}}"#,
        s = sb.display()
    ))
    .unwrap();
    (sb, grant)
}

fn spec_for(goal: &str, grant: &Grant, cwd: &Path) -> TaskSpec {
    TaskSpec {
        goal: goal.to_string(),
        grant: grant.clone(),
        budget_tokens: 1_000_000,
        max_steps: 8,
        cwd: cwd.to_path_buf(),
        patch_codec: PatchCodec::WholeFile,
        bounds: ExecBounds::default(),
        mutating_verbs: true,
        envelope: EnvelopeLens::V1,
        memory_block: None,
        window_ladder: false,
    }
}

/// An operational organ: config switch on, a store in `dir/memory`, and the
/// `[memory] refalsify` opt-in (refalsify spec
/// `docs/superpowers/specs/2026-08-27-refalsify-on-exact-design.md` §5).
///
/// Every call in THIS file passes `false`, deliberately: these are the
/// organ's own binding tests and they must keep measuring the
/// inject-without-refalsify behavior the memory battery's GATE PASS was
/// measured under. The parameter exists so that fact is stated at each call
/// site rather than hidden in the helper — the probe's own suite
/// (`memory_refalsify_test.rs`) mirrors this helper with the flag on.
fn memory_ctx(dir: &Path, enabled: bool, refalsify: bool) -> Arc<MemoryContext> {
    let store = MemoryStore::load(&store_path(dir)).unwrap();
    Arc::new(MemoryContext {
        enabled,
        max_episodes: 64,
        refalsify,
        disabled_reason: None,
        store: Some(Mutex::new(store)),
    })
}

fn store_path(dir: &Path) -> PathBuf {
    dir.join("memory").join("episodes.jsonl")
}

/// Spawns one task and polls to a terminal status — the deadline loop from
/// `task/registry.rs`'s own tests, with a longer bound because these tasks
/// really spawn `python3`.
fn drive(
    registry: &TaskRegistry,
    pager: &Arc<Mutex<Pager<FakeSubstrate>>>,
    agent_id: &str,
    spec: TaskSpec,
    journal_path: &Path,
    memory: Option<Arc<MemoryContext>>,
) -> (String, TaskResult) {
    let task_id = registry.spawn_task(
        Arc::clone(pager),
        agent_id.to_string(),
        spec,
        journal_path.to_path_buf(),
        memory,
    );
    let entry = poll_to_terminal(registry, &task_id);
    (task_id, entry)
}

fn poll_to_terminal(registry: &TaskRegistry, task_id: &str) -> TaskResult {
    let mut entry = registry.get(task_id).expect("entry exists immediately");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    while entry.status == TaskStatus::Running && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(10));
        entry = registry.get(task_id).expect("entry still exists");
    }
    assert_ne!(
        entry.status,
        TaskStatus::Running,
        "task {task_id} never reached a terminal status"
    );
    entry
}

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

/// `(mode, episode_id, candidates_checked)` for the one `MemoryStamp` row
/// naming `task_id` — and it must be exactly one: spec §4 stamps every
/// spawned task once, so a duplicate is as much a bug as a missing row.
fn stamp_for(events: &[Event], task_id: &str) -> (String, Option<String>, u32) {
    let mut found: Vec<(String, Option<String>, u32)> = events
        .iter()
        .filter_map(|e| match e {
            Event::MemoryStamp {
                task_id: t,
                mode,
                episode_id,
                candidates_checked,
                ..
            } if t == task_id => Some((mode.clone(), episode_id.clone(), *candidates_checked)),
            _ => None,
        })
        .collect();
    assert_eq!(
        found.len(),
        1,
        "expected exactly one MemoryStamp for {task_id}, got {found:?}"
    );
    found.remove(0)
}

/// How many prompts the pager has been handed so far that carry a rendered
/// memory block, read from the PAGER's journal (`Event::InferStarted` is the
/// only place the daemon records a prompt verbatim).
///
/// This is the assertion that separates "the stamp claims an injection" from
/// "the model was shown the block": the stamp and `TaskSpec::memory_block`
/// are set in the same branch of the pipeline, so no journal row on the task
/// side can tell a working injection from a stamp that lies about one.
fn memory_prompts(dir: &Path) -> usize {
    replay(&dir.join("pager.jsonl"))
        .unwrap()
        .into_iter()
        .filter(|e| {
            matches!(e, Event::InferStarted { prompt, .. }
                if prompt.contains("[memory: verified prior attempt]"))
        })
        .count()
}

fn mint_ids(events: &[Event]) -> Vec<(String, String)> {
    events
        .iter()
        .filter_map(|e| match e {
            Event::MemoryMint {
                task_id,
                episode_id,
                ..
            } => Some((task_id.clone(), episode_id.clone())),
            _ => None,
        })
        .collect()
}

fn degraded_reasons(events: &[Event]) -> Vec<String> {
    events
        .iter()
        .filter_map(|e| match e {
            Event::Degraded { reason } => Some(reason.clone()),
            _ => None,
        })
        .collect()
}

fn contradicted_ids(events: &[Event]) -> Vec<(String, String)> {
    events
        .iter()
        .filter_map(|e| match e {
            Event::MemoryContradicted {
                task_id,
                episode_id,
                ..
            } => Some((task_id.clone(), episode_id.clone())),
            _ => None,
        })
        .collect()
}

/// Spec §2 + §4: a task that clears the mint bar stores exactly one verified
/// episode citing the pre-patch bytes of every file it touched, and the
/// journal carries the pair that makes the task → store trail walkable in
/// both directions — a `MemoryStamp` (mode `silent`, because a first run has
/// no candidates to match) and a `MemoryMint`.
#[test]
fn verified_task_mints_and_journal_carries_stamp_and_mint() {
    let dir = fresh_dir("mint");
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
    let dir = fresh_dir("repeat");
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
    let dir = fresh_dir("contradict");
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
    let dir = fresh_dir("off");
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
    let dir = fresh_dir("deleted");
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
    let dir = fresh_dir("infra-error");
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
    let dir = fresh_dir("oversize");
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
    let dir = fresh_dir("store-io-fail");
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
