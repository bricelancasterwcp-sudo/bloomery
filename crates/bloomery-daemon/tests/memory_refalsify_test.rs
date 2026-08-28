//! Refalsify's probe, end to end (retrieval/probe pipeline shape from
//! `docs/superpowers/specs/2026-08-27-refalsify-on-exact-design.md` §2/§3
//! and §8's nine-test list; verdict semantics from the v2 class-aware design
//! `docs/superpowers/specs/2026-08-28-refalsify-v2-class-aware-design.md`,
//! which closes the 2026-08-28 domain-of-validity erratum on the v1 design).
//! Every test drives the REAL worker pipeline through
//! `TaskRegistry::spawn_task` — retrieve, PROBE, stamp, inject, run,
//! mint-or-contradict — against a scripted `FakeSubstrate`, a real store, a
//! real journal, and a real `exec_run` spawning a real subprocess.
//!
//! **The v2 verdict model** (class-aware design §1/§2). Every mintable
//! episode is patch-class — the mint bar itself requires a landed patch
//! (`memory/mint.rs::verifying_run`) — so the stored verification is always
//! a post-condition of the fix, and the state the exact gate just matched is
//! the world BEFORE it. The probe's two clean outcomes therefore invert from
//! v1: a clean nonzero exit CONFIRMS the premise ("the defect is present")
//! and injects, stamped `premise_held`; a clean exit 0 means the premise is
//! already gone — silent, no injection, no store mutation, stamped
//! `premise_gone`. **No probe ever contradicts under v2** — only the
//! pre-existing passive path (`organ_after_run`: a task that received an
//! injection and then failed to re-verify it) still can, exactly as it could
//! before refalsify existed.
//!
//! Mirrors `memory_task_test.rs`'s fixture (same `fresh_dir`/`build_pager`/
//! `drive`/`memory_prompts` shapes, the same `a.py` + `python3` patch lens,
//! the same one-task-at-a-time sequencing forced by `FakeSubstrate`'s single
//! FIFO reply queue and the worker's whole-task pager lock), with three
//! additions this slice needs:
//!
//! 1. **The episode's verification command is the fixture's variable.** The
//!    probe re-runs `EpisodeRecord::run_evidence.argv`, which is whatever
//!    argv the MINTING task's own `run` action used — so each test mints
//!    with the `["sh","-c",script]` whose second run it wants to observe.
//!    `sh` is granted explicitly by the test's own grant (the operator
//!    choice `task_exec_run_test.rs`'s process-group test also makes), and
//!    every script uses shell builtins plus at most `cat`/`sleep`, all
//!    reachable under `exec_run`'s pinned `PATH` of `/usr/bin:/bin`.
//!
//! 2. **Staleness is induced ONLY through files the model never cites.** A
//!    minted episode cites the pre-touch bytes of every file the task
//!    touched — here just `a.py` — and retrieval's fingerprint gate compares
//!    them. So every "the verification no longer holds" trick in this file
//!    moves a file the model never read (`flag.txt`, `d.txt`, `boom.txt`),
//!    leaving the fingerprint gate honestly satisfied. That is the whole
//!    point of the slice: the citation set cannot see these files, which is
//!    exactly why passive falsification needs a wasted task to learn what
//!    the probe learns in one command. `a.py` is reset to [`BEFORE`] between
//!    tasks (as `memory_task_test.rs` does); the uncited files live beside
//!    it and are written by the test directly, outside that reset, so no
//!    reset can clobber the drift under test.
//!
//! 3. **A canary the probe itself writes.** [`CANARY_SCRIPT`] is
//!    `echo ran > probe-ran.txt`: a granted, exit-0 command whose only
//!    effect is a file. Deleting that file after the minting task and then
//!    checking for it after the second task is a *direct* observation of
//!    whether the probe executed — used positively (it must reappear when a
//!    probe ran) and negatively (it must stay gone when the pre-check
//!    skipped, or when the flag is off), instead of inferring "no execution"
//!    from the absence of journal rows.
//!
//! Every stamp assertion reads the REPLAYED journal (`replay` →
//! `Event::MemoryStamp`), never in-process state: the stamp's `refalsify`
//! key has to survive the emit site and the wire, or these tests fail.

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

/// The closed set of verdict spellings reachable after v2 (refalsify v2 spec
/// §2/§4). `"passed"`/`"failed"` retire from reachable probe verdicts — the
/// journal still parses rows written by v1 builds, but nothing this file's
/// own test runs can produce should ever stamp either spelling again, so
/// they are deliberately absent here rather than merely unused: a stamp
/// carrying one is the erratum's inversion regressing. Every stamp this file
/// reads is checked against this set in [`stamp_for`], so a verdict string
/// that drifts to a synonym — `"skipped"`, `"pass"`, `"timeout"` — or
/// regresses to a v1 spelling fails the test that observed it even when that
/// test only asserted on some other field.
const VERDICTS: [&str; 4] = [
    "skipped_ungranted",
    "inconclusive",
    "premise_held",
    "premise_gone",
];

/// The granted `run` prefix every episode in this file is minted under.
const SH: [&str; 2] = ["sh", "-c"];

/// A verification command that exits 0 and leaves exactly one trace: the
/// file [`CANARY`]. See this module's docs — this is how "was the probe
/// executed?" is observed directly rather than inferred.
const CANARY_SCRIPT: &str = "echo ran > probe-ran.txt";

/// The file [`CANARY_SCRIPT`] writes, relative to the task's `cwd`.
const CANARY: &str = "probe-ran.txt";

/// A fresh, per-test tempdir — PID + atomic counter so parallel test threads
/// in one `cargo test` process never collide.
fn fresh_dir(tag: &str) -> PathBuf {
    static UNIQUE: AtomicU64 = AtomicU64::new(0);
    let unique = UNIQUE.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "bloomery-refalsify-{tag}-{}-{unique}",
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
        value_length: None,
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

/// One `done` turn — every second (and third) task in this file, whose only
/// job is to be the retrieval the probe acts on.
fn done_turn() -> Reply {
    scripted("<action verb=\"done\">\nnothing to do\n</action>")
}

/// The four turns that clear the mint bar (memory-organ spec §2) with
/// `script` as the verifying run: touch `a.py`, land a patch, run
/// `["sh","-c",script]` — which MUST exit 0 here, or nothing is minted —
/// and finish `Done`.
///
/// `script` is the fixture's real variable: the minted episode's
/// `run_evidence.argv` is this task's own run argv verbatim, and that argv
/// is exactly what a later task's probe re-executes.
fn minting_turns(script: &str) -> Vec<Reply> {
    let argv = serde_json::to_string(&[SH[0], SH[1], script]).unwrap();
    vec![
        scripted("<action verb=\"read\" path=\"a.py\">\n</action>"),
        scripted("<action verb=\"patch\" path=\"a.py\">\nx = 2\n</action>"),
        scripted(&format!("<action verb=\"run\">\n{argv}\n</action>")),
        scripted("<action verb=\"done\">\nfixed\n</action>"),
    ]
}

/// A canonical sandbox under `dir` holding the planted `a.py`.
fn sandbox(dir: &Path) -> PathBuf {
    let sb = dir.join("sandbox");
    std::fs::create_dir_all(&sb).unwrap();
    let sb = std::fs::canonicalize(&sb).unwrap();
    std::fs::write(sb.join("a.py"), BEFORE).unwrap();
    sb
}

/// A grant over `sb` allowing exactly `commands`. Read/write roots are
/// always the whole sandbox: retrieval's own grant gate needs the cited
/// `a.py` readable, so a test that narrows the *command* set must not
/// accidentally narrow the *file* set too — that would silence retrieval
/// before the probe was ever reached, and the test would pass vacuously.
fn grant_with(sb: &Path, commands: &[[&str; 2]]) -> Grant {
    let root = sb.display().to_string();
    let json = serde_json::json!({
        "read_roots": [root],
        "write_roots": [root],
        "commands": commands,
    });
    Grant::from_json(&json.to_string()).unwrap()
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
/// `[memory] refalsify` opt-in (refalsify spec §5) under the test's control.
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

/// Spawns one task and polls to a terminal status.
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
    (task_id.clone(), poll_to_terminal(registry, &task_id))
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
/// The stamp is appended after retrieval and the probe and before the worker
/// takes the pager lock, so its appearance is the one observable that says
/// "this task has retrieved, probed, and has not yet run a single step" —
/// which is the window [`probe`] reads the store in, without a
/// sleep-and-hope. Same seam `memory_task_test.rs`'s deleted-mid-task test
/// uses.
fn await_stamp(journal_path: &Path, task_id: &str) -> Stamp {
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

/// `(mode, episode_id, candidates_checked, refalsify)` — one `MemoryStamp`
/// row, read out of the REPLAYED journal.
type Stamp = (String, Option<String>, u32, Option<String>);

/// The stamp for `task_id`, and the file's verdict-spelling guard.
///
/// Every stamp read in this file goes through here, for two reasons. The
/// journal round trip is one: the `refalsify` verdict has to survive the
/// worker's emit site and serde, so an emit site that dropped the field
/// (or a `None` hardcoded there) fails these tests rather than passing on
/// in-process state. The closed set is the other: refalsify spec §4 fixes
/// the four spellings, and a verdict outside them fails whichever test
/// observed it — even a test asserting on some other field entirely.
///
/// Exactly one row per task, same as `memory_task_test.rs`: spec §4 stamps
/// every spawned task once, so a duplicate is as much a bug as a miss.
fn stamp_for(events: &[Event], task_id: &str) -> Stamp {
    let mut found: Vec<Stamp> = events
        .iter()
        .filter_map(|e| match e {
            Event::MemoryStamp {
                task_id: t,
                mode,
                episode_id,
                candidates_checked,
                refalsify,
                ..
            } if t == task_id => Some((
                mode.clone(),
                episode_id.clone(),
                *candidates_checked,
                refalsify.clone(),
            )),
            _ => None,
        })
        .collect();
    assert_eq!(
        found.len(),
        1,
        "expected exactly one MemoryStamp for {task_id}, got {found:?}"
    );
    let stamp = found.remove(0);
    if let Some(verdict) = &stamp.3 {
        assert!(
            VERDICTS.contains(&verdict.as_str()),
            "stamp verdict {verdict:?} for {task_id} is outside the closed set {VERDICTS:?} \
             (refalsify spec §4)"
        );
    }
    stamp
}

/// How many prompts the pager has been handed that carry a rendered memory
/// block — read from the PAGER's journal, the only place the daemon records
/// a prompt verbatim. This is what separates "the stamp claims an injection"
/// from "the model was shown the block".
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

/// Every `Degraded` reason on the journal — how an operator-visible skip
/// names itself, and here how "silenced by the oversize rule" is told apart
/// from "silenced by a fingerprint miss", which stamp identically.
fn degraded_reasons(events: &[Event]) -> Vec<String> {
    events
        .iter()
        .filter_map(|e| match e {
            Event::Degraded { reason } => Some(reason.clone()),
            _ => None,
        })
        .collect()
}

/// The `verb` of every `TaskStep` row on the journal, in append order.
///
/// `Event::TaskStep` carries an `AgentId` but no `task_id`, so "how many
/// steps did THIS task journal" is necessarily read as a delta across the
/// task under test. That is sound in this file and nowhere near a general
/// rule: every test here drives its tasks strictly one at a time (module
/// docs), so nothing else can be appending steps in the window.
fn task_step_verbs(events: &[Event]) -> Vec<String> {
    events
        .iter()
        .filter_map(|e| match e {
            Event::TaskStep { verb, .. } => Some(verb.clone()),
            _ => None,
        })
        .collect()
}

/// Lines in the store FILE — deliberately not `MemoryStore::episodes()`'s
/// count. The store is append-only with last-writer-wins per `episode_id`
/// (`memory/store.rs`), so a re-appended row for an episode already present
/// leaves the live map exactly as it was and is invisible to any
/// id-counting assertion. The line count is the only honest way to say
/// "nothing was written", which is what spec §2.3's no-re-append claims.
fn store_rows(dir: &Path) -> usize {
    std::fs::read_to_string(store_path(dir))
        .expect("the store file exists once something has been minted")
        .lines()
        .filter(|l| !l.trim().is_empty())
        .count()
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

/// The stored row for `episode_id`, re-read from disk — never from the
/// in-process store, so every store assertion is about durable bytes.
fn stored_status(dir: &Path, episode_id: &str) -> (String, Option<String>) {
    let store = MemoryStore::load(&store_path(dir)).unwrap();
    let ep = store
        .episodes()
        .find(|e| e.episode_id == episode_id)
        .expect("the episode is in the store")
        .clone();
    (ep.status, ep.contradicted_by)
}

/// Everything a probe test needs after its minting task: the sandbox, the
/// grant it minted under, the registry/pager/journal handles, and the id of
/// the one episode now in the store.
struct Minted {
    sb: PathBuf,
    grant: Grant,
    registry: TaskRegistry,
    pager: Arc<Mutex<Pager<FakeSubstrate>>>,
    agent_id: String,
    journal_path: PathBuf,
    episode_id: String,
}

/// Runs the minting task for `script` and hands back the fixture in the
/// state the probe tests start from: exactly one verified episode whose
/// `run_evidence.argv` is `["sh","-c",script]`, `a.py` reset to [`BEFORE`]
/// so the fingerprint gate matches, and the canary (if `script` wrote one)
/// deleted so its reappearance can only be the probe's doing.
///
/// `tail` is scripted after the minting task's own four turns — one `done`
/// per follow-up task the caller intends to drive.
fn mint(dir: &Path, script: &str, tail: usize) -> Minted {
    let sb = sandbox(dir);
    let grant = grant_with(&sb, &[SH]);
    let mut replies = minting_turns(script);
    replies.extend(std::iter::repeat_with(done_turn).take(tail));
    let (pager, agent_id) = build_pager(dir, replies);
    let pager = Arc::new(Mutex::new(pager));
    let registry = TaskRegistry::new();
    let journal_path = dir.join("tasks.jsonl");
    let ctx = memory_ctx(dir, true, false);

    let (task_id, first) = drive(
        &registry,
        &pager,
        &agent_id,
        spec_for(GOAL, &grant, &sb),
        &journal_path,
        Some(ctx),
    );
    assert_eq!(first.status, TaskStatus::Done, "{first:?}");
    let minted = mint_ids(&replay(&journal_path).unwrap());
    assert_eq!(
        minted.len(),
        1,
        "the minting task must have minted exactly one episode: {minted:?}"
    );
    assert_eq!(minted[0].0, task_id, "the mint row names the minting task");
    let episode_id = minted[0].1.clone();

    // The minting run really executed — for the canary script that is
    // directly observable, and it is the same observation the probe tests
    // then re-use. Removing the canary here is what makes its later state a
    // statement about the PROBE and nothing else.
    if script == CANARY_SCRIPT {
        assert!(
            sb.join(CANARY).exists(),
            "the minting task's own run must have written the canary"
        );
    }
    let _ = std::fs::remove_file(sb.join(CANARY));
    // Back to the bytes the episode cites, so the exact gate matches on the
    // next task (`memory_task_test.rs` resets the same way). The uncited
    // files each test plants live beside `a.py` and are deliberately NOT
    // touched here — the drift under test must survive this reset.
    std::fs::write(sb.join("a.py"), BEFORE).unwrap();

    Minted {
        sb,
        grant,
        registry,
        pager,
        agent_id,
        journal_path,
        episode_id,
    }
}

/// The one goal every task in this file asks for — an exact repeat is the
/// only retrieval class that exists (GATE-C's standing prohibition).
const GOAL: &str = "make a.py say two";

fn canary_exists(sb: &Path) -> bool {
    sb.join(CANARY).exists()
}

/// What one probed task did, split into the observation that isolates the
/// probe and the ordinary terminal result.
struct Probed {
    /// The probed task's own id — needed wherever a later assertion must
    /// name exactly which task an accusation cites (e.g. the passive path's
    /// `MemoryContradicted` row after an injected-but-unverified probe
    /// task), not merely that one exists.
    task_id: String,
    result: TaskResult,
    /// The stamp, from the replayed journal.
    stamp: Stamp,
    /// `(status, contradicted_by)` of the minted episode, re-read from the
    /// store FILE at the probe moment.
    stored: (String, Option<String>),
    /// Rows in the store file at the probe moment — see [`store_rows`].
    /// Taken in the same locked window as `stored`, and for the same reason:
    /// after the task finishes, passive falsification may have appended a row
    /// of its own, and a count taken then could not tell the two apart.
    store_rows: usize,
    /// The `MemoryContradicted` rows on the journal at the probe moment.
    contradicted: Vec<(String, String)>,
}

/// Drives one task through its probe with the pager lock HELD, reads the
/// store and journal in that window, then lets the task finish.
///
/// **Why the interleave is necessary, not decorative.** The organ's
/// pre-existing *passive* falsification (memory-organ spec §5) contradicts
/// any injected episode whose receiving task then fails its own verification
/// — and every second task in this file ends `Done` with no patch, which is
/// exactly that shape. A store read taken after such a task finishes
/// therefore cannot tell the probe's accusation from the ordinary passive
/// one, and an "episode still verified" assertion would fail on every
/// injecting test for a reason that has nothing to do with this slice.
///
/// The worker retrieves, probes and stamps *before* it takes the pager lock
/// (`task/registry.rs`'s pipeline, steps 1-2 vs. 5), so holding that lock
/// and waiting for the stamp row is a deterministic — not raced —
/// observation point at which the probe is the only thing that can have
/// touched the store. `memory_task_test.rs`'s deleted-mid-task test uses the
/// same seam for the same reason.
fn probe(m: &Minted, dir: &Path, spec: TaskSpec, ctx: Arc<MemoryContext>) -> Probed {
    let guard = m.pager.lock().expect("the pager mutex is healthy");
    let task_id = m.registry.spawn_task(
        Arc::clone(&m.pager),
        m.agent_id.clone(),
        spec,
        m.journal_path.clone(),
        Some(ctx),
    );
    // Retrieval and the probe are done; the worker is now parked on the
    // pager lock this thread holds.
    let stamp = await_stamp(&m.journal_path, &task_id);
    let stored = stored_status(dir, &m.episode_id);
    let store_rows = store_rows(dir);
    let contradicted = contradicted_ids(&replay(&m.journal_path).unwrap());
    drop(guard);

    let result = poll_to_terminal(&m.registry, &task_id);
    Probed {
        task_id,
        result,
        stamp,
        stored,
        store_rows,
        contradicted,
    }
}

/// The store row a probe must have left alone, spelled once.
fn untouched() -> (String, Option<String>) {
    ("verified".to_string(), None)
}

/// **Flag-off identity (refalsify spec §5).** With `[memory] refalsify` off,
/// an exact repeat injects exactly as it did before this slice existed and
/// its stamp carries no verdict at all.
///
/// The canary is what makes this stronger than byte-diffing two prompts: the
/// episode's stored verification command WRITES a file, so a probe running
/// here — passing or not — would leave a trace. Its absence is a direct
/// observation that nothing was executed at the retrieval moment.
#[test]
fn flag_off_injects_without_probing_and_stamps_none() {
    let dir = fresh_dir("flag-off");
    let m = mint(&dir, CANARY_SCRIPT, 1);
    assert!(
        !canary_exists(&m.sb),
        "the fixture must start the second task with no canary"
    );

    let p = probe(
        &m,
        &dir,
        spec_for(GOAL, &m.grant, &m.sb),
        memory_ctx(&dir, true, false),
    );
    assert_eq!(p.result.status, TaskStatus::Done, "{:?}", p.result);
    assert_eq!(
        p.stamp,
        ("injected".to_string(), Some(m.episode_id.clone()), 1, None),
        "flag-off keeps today's behavior and stamps no verdict"
    );
    assert_eq!(
        memory_prompts(&dir),
        1,
        "the injected task's prompt must carry the rendered block — exactly \
         one prompt can, the probed task having exactly one turn and the \
         minting task having retrieved from an empty store"
    );
    assert!(
        !canary_exists(&m.sb),
        "no probe may run with the flag off — the canary would have reappeared"
    );
    assert_eq!(p.stored, untouched());
    assert!(p.contradicted.is_empty(), "{:?}", p.contradicted);
}

/// **The erratum pin (refalsify v2 spec §4).** A drift-free exact repeat of a
/// patch-class episode: the stored verification checks the CITED file's goal
/// state, and nothing changes after mint besides the fixture's own reset to
/// BEFORE — the match condition itself. v1 contradicted this true lesson
/// (2026-08-28 domain-of-validity erratum, demonstrated live); v2 reads the
/// failure as the premise holding and injects.
#[test]
fn a_drift_free_repeat_probes_premise_held_and_injects() {
    let dir = fresh_dir("premise-held");
    let m = mint(&dir, "grep -q 'x = 2' a.py", 1);
    assert_eq!(std::fs::read(m.sb.join("a.py")).unwrap(), BEFORE);

    let p = probe(
        &m,
        &dir,
        spec_for(GOAL, &m.grant, &m.sb),
        memory_ctx(&dir, true, true),
    );
    assert_eq!(p.result.status, TaskStatus::Done, "{:?}", p.result);
    assert_eq!(
        p.stamp,
        (
            "injected".to_string(),
            Some(m.episode_id.clone()),
            1,
            Some("premise_held".to_string())
        ),
        "the failing probe confirms the matched premise and injects"
    );
    assert_eq!(memory_prompts(&dir), 1, "the lesson reached the prompt");
    assert_eq!(p.stored, untouched(), "no probe ever contradicts under v2");
}

/// **premise_gone (v2 spec §2/§4).** The stored verification passes on the
/// matched state: the premise is gone, the lesson is NOT false — silent, no
/// injection, no store mutation, and the next identical retrieval re-probes
/// (observed by the canary the command writes: deleted between tasks, it can
/// only reappear if a probe ran).
#[test]
fn a_passing_probe_is_premise_gone_silent_unmutated_and_reprobes() {
    let dir = fresh_dir("premise-gone");
    let m = mint(&dir, CANARY_SCRIPT, 2);

    let p = probe(
        &m,
        &dir,
        spec_for(GOAL, &m.grant, &m.sb),
        memory_ctx(&dir, true, true),
    );
    assert_eq!(p.result.status, TaskStatus::Done, "{:?}", p.result);
    assert!(canary_exists(&m.sb), "the probe really executed");
    assert_eq!(
        p.stamp,
        (
            "silent".to_string(),
            None,
            1,
            Some("premise_gone".to_string())
        ),
        "a satisfied premise is silence, not evidence against the lesson"
    );
    assert_eq!(
        memory_prompts(&dir),
        0,
        "byte-identical to a stranger's prompt"
    );
    assert_eq!(
        p.stored,
        untouched(),
        "premise_gone never touches the store"
    );

    // Third identical task: nothing was contradicted, so retrieval matches
    // again and the probe runs again — no memoized skip.
    let _ = std::fs::remove_file(m.sb.join(CANARY));
    let (next_id, next) = drive(
        &m.registry,
        &m.pager,
        &m.agent_id,
        spec_for(GOAL, &m.grant, &m.sb),
        &m.journal_path,
        Some(memory_ctx(&dir, true, true)),
    );
    assert_eq!(next.status, TaskStatus::Done, "{next:?}");
    assert!(canary_exists(&m.sb), "the second probe also ran");
    let events = replay(&m.journal_path).unwrap();
    assert_eq!(
        stamp_for(&events, &next_id),
        (
            "silent".to_string(),
            None,
            1,
            Some("premise_gone".to_string())
        ),
    );
    assert_eq!(contradicted_ids(&events).len(), 0, "no accusation, ever");
}

/// **No re-append, no phantom step (v2 spec §2, "no store mutation").** Two
/// properties of `premise_gone` the pin above does not observe, kept alive
/// from the retired `a_passing_probe_injects_and_stamps_passed` because
/// nothing else in this file pins them: the store FILE gains no row (not
/// even an identical re-mint, which append-only last-writer-wins semantics
/// could hide from an `episodes()` count — see [`store_rows`]'s own doc
/// comment), and the probe itself journals no `TaskStep` — the probed task's
/// steps are exactly its own one `done`, because the probe is not a model
/// action and never renders into the transcript.
#[test]
fn a_premise_gone_probe_appends_no_store_row_and_journals_no_step() {
    let dir = fresh_dir("premise-gone-untouched");
    let m = mint(&dir, CANARY_SCRIPT, 1);

    // The fixture's own baseline: one minting task, one minted row, its four
    // real steps. Both later assertions are deltas against these.
    let rows_after_mint = store_rows(&dir);
    assert_eq!(rows_after_mint, 1, "the mint appended exactly one row");
    let steps_after_mint = task_step_verbs(&replay(&m.journal_path).unwrap());
    assert_eq!(
        steps_after_mint,
        ["read", "patch", "run", "done"],
        "the minting task's own four steps are the baseline"
    );

    let p = probe(
        &m,
        &dir,
        spec_for(GOAL, &m.grant, &m.sb),
        memory_ctx(&dir, true, true),
    );
    assert_eq!(p.result.status, TaskStatus::Done, "{:?}", p.result);
    assert!(
        canary_exists(&m.sb),
        "the probe must have really executed the episode's stored command"
    );
    assert_eq!(
        p.store_rows, rows_after_mint,
        "a premise_gone probe appends nothing to the store (v2 spec §2): the \
         stamp is the durable evidence"
    );
    assert_eq!(
        task_step_verbs(&replay(&m.journal_path).unwrap()),
        ["read", "patch", "run", "done", "done"],
        "the probed task journals exactly one TaskStep — its own `done` — and \
         the probe, which ran a real subprocess, journals none"
    );
}

/// **An uncited-drift failure reads premise_held (v2 spec §1's named
/// limitation).** `flag.txt` holds `0` when the episode is minted and `1`
/// when it is retrieved; the model never reads `flag.txt`, so it is not in
/// `cited_files` and the exact gate is honestly satisfied. Under v1 this was
/// the slice's whole point — early detection of stale-but-uncited state.
/// Under v2 a verification that is state-independent of what the patches
/// actually touched is indistinguishable from a genuinely held premise
/// without recorded pre-state evidence (out of scope, not foreclosed): the
/// stored command fails, so the probe reads `premise_held` and injects. The
/// injection is noise, not damage — if the lesson really is stale, the
/// pre-existing PASSIVE path (`organ_after_run`: this probed task received
/// the injection and then landed no run of its own to re-verify it) owns
/// the aftermath, exactly as it would have before refalsify existed.
#[test]
fn an_uncited_drift_failure_reads_premise_held_and_injects() {
    let dir = fresh_dir("uncited-drift");
    let m = {
        // `flag.txt` must hold "0" before the minting run, or the mint bar
        // (exit 0 after the last landed patch) is never cleared.
        let sb = dir.join("sandbox");
        std::fs::create_dir_all(&sb).unwrap();
        std::fs::write(sb.join("flag.txt"), b"0").unwrap();
        mint(&dir, "exit $(cat flag.txt)", 2)
    };

    // The one byte that turns the stored verification stale — in a file no
    // citation covers, so retrieval still matches exactly.
    std::fs::write(m.sb.join("flag.txt"), b"1").unwrap();

    let p = probe(
        &m,
        &dir,
        spec_for(GOAL, &m.grant, &m.sb),
        memory_ctx(&dir, true, true),
    );
    let probed_id = p.task_id.clone();
    assert_eq!(p.result.status, TaskStatus::Done, "{:?}", p.result);
    assert_eq!(
        p.stamp,
        (
            "injected".to_string(),
            Some(m.episode_id.clone()),
            1,
            Some("premise_held".to_string())
        ),
        "an uncited-drift failure is indistinguishable from a held premise \
         under v2 — it injects"
    );
    assert_eq!(memory_prompts(&dir), 1, "the lesson reached the prompt");
    assert_eq!(p.stored, untouched(), "no probe ever contradicts under v2");

    // A third identical task: the bytes never drifted again (still `1`), and
    // the fingerprint gate never covered `flag.txt` in the first place, so
    // whether retrieval still matches this third time depends only on
    // whether anything contradicted the episode in between — and the
    // PASSIVE path (not the probe) is exactly the mechanism that can.
    assert_eq!(std::fs::read(m.sb.join("a.py")).unwrap(), BEFORE);
    let (next_id, next) = drive(
        &m.registry,
        &m.pager,
        &m.agent_id,
        spec_for(GOAL, &m.grant, &m.sb),
        &m.journal_path,
        Some(memory_ctx(&dir, true, true)),
    );
    assert_eq!(next.status, TaskStatus::Done, "{next:?}");
    let events = replay(&m.journal_path).unwrap();
    assert_eq!(
        stamp_for(&events, &next_id),
        ("silent".to_string(), None, 1, None),
        "the passive path contradicted the injected-but-unverified probe \
         task, so this repeat retrieves silence — v2 never memoizes a \
         probe's own verdict, but it never disarms the pre-existing passive \
         path either"
    );
    assert_eq!(
        contradicted_ids(&events),
        vec![(probed_id.clone(), m.episode_id.clone())],
        "one accusation, citing the probed task itself — the passive path's, \
         not the probe's: {events:?}"
    );
    assert_eq!(mint_ids(&events).len(), 1, "no follow-up task minted");
}

/// **Ungranted skip (refalsify spec §2.1).** Grants come from the incoming
/// REQUEST, not from the store, so a task whose grant does not cover the
/// episode's stored argv cannot probe it. The episode injects anyway
/// (refalsification upgrades trust where possible; it never shrinks reach
/// below the battery-passing behavior) and the stamp says
/// `skipped_ungranted`.
///
/// "No run was attempted" is observed directly, not inferred: the stored
/// command's only effect is the canary file, and the canary stays gone. That
/// is a stronger check than scanning the journal for execution rows — the
/// pre-check is specified to run BEFORE anything spawns, and a spawn that
/// happened and was then refused by `exec_run` would still have left the
/// file if it had run at all.
#[test]
fn an_ungranted_command_skips_and_injects() {
    let dir = fresh_dir("ungranted");
    let m = mint(&dir, CANARY_SCRIPT, 1);

    // Same read/write roots (retrieval's own grant gate still has to pass),
    // no command prefixes at all.
    let narrow = grant_with(&m.sb, &[]);
    assert!(
        narrow
            .check_command(&[SH[0].to_string(), SH[1].to_string(), "true".to_string()])
            .is_err(),
        "the fixture's second grant must genuinely not cover the stored argv"
    );

    let p = probe(
        &m,
        &dir,
        spec_for(GOAL, &narrow, &m.sb),
        memory_ctx(&dir, true, true),
    );
    assert_eq!(p.result.status, TaskStatus::Done, "{:?}", p.result);
    assert_eq!(
        p.stamp,
        (
            "injected".to_string(),
            Some(m.episode_id.clone()),
            1,
            Some("skipped_ungranted".to_string())
        ),
    );
    assert_eq!(
        memory_prompts(&dir),
        1,
        "an unprobed episode still injects — into the probed task's one turn"
    );
    assert!(
        !canary_exists(&m.sb),
        "the pre-check runs before anything spawns — nothing may have executed"
    );
    assert_eq!(p.stored, untouched(), "a skip touches no record");
    assert!(p.contradicted.is_empty(), "{:?}", p.contradicted);
}

/// **Demoted skip (refalsify spec §2.1).** The demotion boundary outranks
/// refalsification: a read-only task (`mutating_verbs == false`) may not
/// have commands executed at its moment, whatever its grant says — so it
/// takes the ungranted-class skip even though its grant here covers the
/// stored argv exactly.
///
/// Same canary observation as the ungranted test, and the grant is
/// deliberately the SAME one the mint ran under, so the only thing that can
/// produce the skip is the demotion.
#[test]
fn a_demoted_task_skips_even_with_a_covering_grant() {
    let dir = fresh_dir("demoted");
    let m = mint(&dir, CANARY_SCRIPT, 1);
    assert!(
        m.grant
            .check_command(&[
                SH[0].to_string(),
                SH[1].to_string(),
                CANARY_SCRIPT.to_string()
            ])
            .is_ok(),
        "the grant must cover the stored argv, or this test proves nothing"
    );

    let mut spec = spec_for(GOAL, &m.grant, &m.sb);
    spec.mutating_verbs = false;

    let p = probe(&m, &dir, spec, memory_ctx(&dir, true, true));
    assert_eq!(p.result.status, TaskStatus::Done, "{:?}", p.result);
    assert_eq!(
        p.stamp,
        (
            "injected".to_string(),
            Some(m.episode_id.clone()),
            1,
            Some("skipped_ungranted".to_string())
        ),
    );
    assert_eq!(
        memory_prompts(&dir),
        1,
        "a demoted task still receives — into its one turn"
    );
    assert!(
        !canary_exists(&m.sb),
        "a demoted task has no commands executed at its moment"
    );
    assert_eq!(p.stored, untouched());
    assert!(p.contradicted.is_empty(), "{:?}", p.contradicted);
}

/// **Inconclusive by timeout (refalsify spec §2.3, third verdict).** A probe
/// that exceeds the task's own `run_timeout_secs` is environmental, not
/// semantic: it injects and stamps `inconclusive`, and the episode STAYS
/// verified. The organ's law forbids the probe's infrastructure costing a
/// task its injection, and only a genuine nonzero exit ever contradicts.
///
/// `d.txt` is the uncited-file trick again: `0` at mint (an instant exit 0
/// that clears the mint bar), `10` at retrieval, against a second task whose
/// `ExecBounds::run_timeout_secs` is 1.
#[test]
fn a_timed_out_probe_is_inconclusive_and_injects() {
    let dir = fresh_dir("timeout");
    let m = {
        let sb = dir.join("sandbox");
        std::fs::create_dir_all(&sb).unwrap();
        std::fs::write(sb.join("d.txt"), b"0").unwrap();
        mint(&dir, "sleep $(cat d.txt)", 1)
    };
    std::fs::write(m.sb.join("d.txt"), b"10").unwrap();

    let mut spec = spec_for(GOAL, &m.grant, &m.sb);
    spec.bounds.run_timeout_secs = 1;

    let p = probe(&m, &dir, spec, memory_ctx(&dir, true, true));
    assert_eq!(p.result.status, TaskStatus::Done, "{:?}", p.result);
    assert_eq!(
        p.stamp,
        (
            "injected".to_string(),
            Some(m.episode_id.clone()),
            1,
            Some("inconclusive".to_string())
        ),
    );
    assert_eq!(
        memory_prompts(&dir),
        1,
        "an inconclusive probe never costs the task its injection"
    );
    assert_eq!(
        p.stored,
        untouched(),
        "a timeout is not evidence the lesson is wrong"
    );
    assert!(p.contradicted.is_empty(), "{:?}", p.contradicted);
}

/// **Inconclusive by signal death (refalsify spec §2.3).** `exec_run`
/// reports a signal-killed child as `failed: false` with the pinned outcome
/// `"ran sh exit -1"` — `-1` being its "no exit code" sentinel, not a real
/// exit. Reading that as a clean nonzero exit would contradict a perfectly
/// good episode over a `SIGKILL`, so it must classify `inconclusive`.
///
/// Driven through the full fixture rather than by calling the classifier
/// directly: the classifier is private to `task::registry`, and the property
/// that matters is that a real signal death reaches it as the sentinel and
/// leaves the store alone. `boom.txt` is the uncited marker — absent at mint
/// (so `exit 0` clears the bar), present at retrieval (so the shell kills
/// itself).
#[test]
fn classify_probe_calls_signal_death_inconclusive() {
    let dir = fresh_dir("signal");
    let m = mint(&dir, "[ -f boom.txt ] && kill -9 $$; exit 0", 1);
    std::fs::write(m.sb.join("boom.txt"), b"").unwrap();

    let p = probe(
        &m,
        &dir,
        spec_for(GOAL, &m.grant, &m.sb),
        memory_ctx(&dir, true, true),
    );
    assert_eq!(p.result.status, TaskStatus::Done, "{:?}", p.result);
    assert_eq!(
        p.stamp,
        (
            "injected".to_string(),
            Some(m.episode_id.clone()),
            1,
            Some("inconclusive".to_string())
        ),
        "a signal death is not a nonzero exit and must never contradict"
    );
    assert_eq!(memory_prompts(&dir), 1);
    assert_eq!(
        p.stored,
        untouched(),
        "the episode STANDS — nothing measured it wrong"
    );
    assert!(p.contradicted.is_empty(), "{:?}", p.contradicted);
}

/// **Oversize outranks the probe (refalsify spec §2, as amended).** The
/// implemented order is retrieve → render → oversize gate → probe, and the
/// amendment states the behavioral consequence: *an episode the oversize rule
/// has already turned silent is never executed.* With `[memory] refalsify`
/// ON, a covering grant, and a stored command that would leave a trace, the
/// stamp is `("silent", None, refalsify: None)` and the canary stays gone.
///
/// The canary is what makes this a behavioral test rather than a restatement
/// of the stamp. `refalsify: None` alone is weak evidence — the oversize
/// return hardcodes it, so a probe hoisted above that gate would still stamp
/// `None` while having spent a subprocess. The file's absence is the only
/// observation that says *nothing ran*.
///
/// **Why the `Degraded` assertion is load-bearing, not decoration.** A
/// fingerprint miss stamps `("silent", None, 1, None)` too, and would leave
/// the canary equally absent — so without proof that the oversize branch is
/// the one that fired, this test could pass for a reason that has nothing to
/// do with the probe order. The degraded row naming the episode and the
/// injection bound is that proof.
///
/// The oversized episode is hand-minted straight into the store, mirroring
/// `memory_task_test.rs`'s `an_oversized_memory_block_is_skipped_and_stamped_silent`
/// and for its reason: the branch under test reads `render_memory_block`'s
/// output length and nothing else, so driving a >16 KiB patch through the
/// real executor would make the test slow and mostly about `exec_patch`.
/// Everything retrieval gates on is real — the goal hash, the canonical
/// cited path, the sha256 of the actual workspace bytes — and so is the
/// stored argv, which is the fixture's `["sh","-c",CANARY_SCRIPT]` verbatim.
#[test]
fn an_oversized_episode_is_never_probed_even_with_the_flag_on() {
    let dir = fresh_dir("oversize-flag-on");
    let sb = sandbox(&dir);
    let grant = grant_with(&sb, &[SH]);
    let (pager, agent_id) = build_pager(&dir, vec![done_turn()]);
    let pager = Arc::new(Mutex::new(pager));
    let registry = TaskRegistry::new();
    let journal_path = dir.join("tasks.jsonl");
    let ctx = memory_ctx(&dir, true, true);

    let cited_path = sb.join("a.py").display().to_string();
    let cited = vec![CitedFile {
        path: cited_path.clone(),
        fingerprint: Fingerprint::Sha256(sha256_hex_bytes(BEFORE)),
    }];
    let hash = goal_hash(GOAL);
    let argv: Vec<String> = vec![SH[0].into(), SH[1].into(), CANARY_SCRIPT.into()];
    let record = EpisodeRecord {
        episode_id: episode_id(&hash, &cited),
        goal_hash: hash,
        goal_text: GOAL.to_string(),
        cited_files: cited,
        // The one oversized field: a whole-file patch body well past the
        // 16 KiB injection bound.
        landed_patches: vec![StoredPatch {
            path: cited_path,
            codec: "whole_file".to_string(),
            body: format!("x = {}", "9".repeat(20_000)),
        }],
        run_evidence: RunEvidence {
            argv: argv.clone(),
            outcome: "ran sh exit 0".into(),
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
    // Nothing but the size may explain the skip: the grant covers the stored
    // argv exactly, so `skipped_ungranted` is off the table, and the task is
    // mutating, so the demotion boundary is too.
    assert!(
        grant.check_command(&argv).is_ok(),
        "the grant must cover the stored argv, or a skip proves nothing"
    );
    {
        let store = ctx
            .store
            .as_ref()
            .expect("an operational organ has a store");
        let mut store = store.lock().expect("the store mutex is healthy");
        store.mint(record, 64).unwrap();
    }
    assert!(
        !canary_exists(&sb),
        "the fixture must start with no canary — nothing has run it yet"
    );

    let (task_id, result) = drive(
        &registry,
        &pager,
        &agent_id,
        spec_for(GOAL, &grant, &sb),
        &journal_path,
        Some(Arc::clone(&ctx)),
    );

    assert_eq!(result.status, TaskStatus::Done, "{result:?}");
    let events = replay(&journal_path).unwrap();
    assert_eq!(
        stamp_for(&events, &task_id),
        ("silent".to_string(), None, 1, None),
        "an oversize skip stamps no verdict, because nothing was probed"
    );
    assert_eq!(
        memory_prompts(&dir),
        0,
        "no prompt may carry a block the organ declined to inject"
    );
    let reasons = degraded_reasons(&events);
    assert!(
        reasons
            .iter()
            .any(|r| r.contains(&oversized_id) && r.contains("injection bound")),
        "the size skip must be the branch that fired, and it must name itself: {reasons:?}"
    );
    assert!(
        !canary_exists(&sb),
        "an episode the oversize rule already silenced is NEVER executed \
         (refalsify spec §2 amendment) — the canary would have reappeared"
    );
    assert_eq!(
        stored_status(&dir, &oversized_id),
        untouched(),
        "an unprobed episode is accused of nothing"
    );
    assert!(
        contradicted_ids(&events).is_empty(),
        "{:?}",
        contradicted_ids(&events)
    );
}
