//! Fixtures specific to the `memory_refalsify_*` tests.
//!
//! Split out on 2026-09-01 (carried-debt slice D). The helpers this pair
//! shares with `memory_task_test` live in `tests/common/memory.rs`; these are
//! the ones only refalsify needs.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use super::memory::{
    build_pager, contradicted_ids, memory_ctx, mint_ids, poll_to_terminal, scripted, spec_for,
    store_path, BEFORE,
};
use bloomery_core::grant::Grant;
use bloomery_core::journal::{replay, Event};
use bloomery_daemon::memory::store::MemoryStore;
use bloomery_daemon::memory::MemoryContext;
use bloomery_daemon::pager::Pager;
use bloomery_daemon::task::{TaskRegistry, TaskResult, TaskSpec, TaskStatus};
use bloomery_substrate::fake::FakeSubstrate;
use bloomery_substrate::Reply;

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
pub const VERDICTS: [&str; 4] = [
    "skipped_ungranted",
    "inconclusive",
    "premise_held",
    "premise_gone",
];

/// The granted `run` prefix every episode in this file is minted under.
pub const SH: [&str; 2] = ["sh", "-c"];

/// A verification command that exits 0 and leaves exactly one trace: the
/// file [`CANARY`]. See this module's docs — this is how "was the probe
/// executed?" is observed directly rather than inferred.
pub const CANARY_SCRIPT: &str = "echo ran > probe-ran.txt";

/// The file [`CANARY_SCRIPT`] writes, relative to the task's `cwd`.
pub const CANARY: &str = "probe-ran.txt";

/// One `done` turn — every second (and third) task in this file, whose only
/// job is to be the retrieval the probe acts on.
pub fn done_turn() -> Reply {
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
pub fn minting_turns(script: &str) -> Vec<Reply> {
    let argv = serde_json::to_string(&[SH[0], SH[1], script]).unwrap();
    vec![
        scripted("<action verb=\"read\" path=\"a.py\">\n</action>"),
        scripted("<action verb=\"patch\" path=\"a.py\">\nx = 2\n</action>"),
        scripted(&format!("<action verb=\"run\">\n{argv}\n</action>")),
        scripted("<action verb=\"done\">\nfixed\n</action>"),
    ]
}

/// A canonical sandbox under `dir` holding the planted `a.py`.
pub fn sandbox(dir: &Path) -> PathBuf {
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
pub fn grant_with(sb: &Path, commands: &[[&str; 2]]) -> Grant {
    let root = sb.display().to_string();
    let json = serde_json::json!({
        "read_roots": [root],
        "write_roots": [root],
        "commands": commands,
    });
    Grant::from_json(&json.to_string()).unwrap()
}

/// Spawns one task and polls to a terminal status.
pub fn drive(
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

/// Blocks until `task_id`'s `MemoryStamp` row is on disk, and returns it.
///
/// The stamp is appended after retrieval and the probe and before the worker
/// takes the pager lock, so its appearance is the one observable that says
/// "this task has retrieved, probed, and has not yet run a single step" —
/// which is the window [`probe`] reads the store in, without a
/// sleep-and-hope. Same seam `memory_task_test.rs`'s deleted-mid-task test
/// uses.
pub fn await_stamp(journal_path: &Path, task_id: &str) -> Stamp {
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
pub type Stamp = (String, Option<String>, u32, Option<String>);

/// The stamp for `task_id`, and the file's verdict-spelling guard.
///
/// Every stamp read in this file goes through here, for two reasons. The
/// journal round trip is one: the `refalsify` verdict has to survive the
/// worker's emit site and serde, so an emit site that dropped the field
/// (or a `None` hardcoded there) fails these tests rather than passing on
/// in-process state. The closed set is the other: refalsify v2 spec
/// (`docs/superpowers/specs/2026-08-28-refalsify-v2-class-aware-design.md`)
/// §2 fixes the four reachable spellings, and a verdict outside them fails
/// whichever test observed it — even a test asserting on some other field
/// entirely.
///
/// Exactly one row per task, same as `memory_task_test.rs`: spec §4 stamps
/// every spawned task once, so a duplicate is as much a bug as a miss.
pub fn stamp_for(events: &[Event], task_id: &str) -> Stamp {
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
             (refalsify v2 spec §2)"
        );
    }
    stamp
}

/// Lines in the store FILE — deliberately not `MemoryStore::episodes()`'s
/// count. The store is append-only with last-writer-wins per `episode_id`
/// (`memory/store.rs`), so a re-appended row for an episode already present
/// leaves the live map exactly as it was and is invisible to any
/// id-counting assertion. The line count is the only honest way to say
/// "nothing was written", which is what spec §2.3's no-re-append claims.
pub fn store_rows(dir: &Path) -> usize {
    std::fs::read_to_string(store_path(dir))
        .expect("the store file exists once something has been minted")
        .lines()
        .filter(|l| !l.trim().is_empty())
        .count()
}

/// The stored row for `episode_id`, re-read from disk — never from the
/// in-process store, so every store assertion is about durable bytes.
pub fn stored_status(dir: &Path, episode_id: &str) -> (String, Option<String>) {
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
pub struct Minted {
    pub sb: PathBuf,
    pub grant: Grant,
    pub registry: TaskRegistry,
    pub pager: Arc<Mutex<Pager<FakeSubstrate>>>,
    pub agent_id: String,
    pub journal_path: PathBuf,
    pub episode_id: String,
}

/// Runs the minting task for `script` and hands back the fixture in the
/// state the probe tests start from: exactly one verified episode whose
/// `run_evidence.argv` is `["sh","-c",script]`, `a.py` reset to [`BEFORE`]
/// so the fingerprint gate matches, and the canary (if `script` wrote one)
/// deleted so its reappearance can only be the probe's doing.
///
/// `tail` is scripted after the minting task's own four turns — one `done`
/// per follow-up task the caller intends to drive.
pub fn mint(dir: &Path, script: &str, tail: usize) -> Minted {
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
pub const GOAL: &str = "make a.py say two";

pub fn canary_exists(sb: &Path) -> bool {
    sb.join(CANARY).exists()
}

/// What one probed task did, split into the observation that isolates the
/// probe and the ordinary terminal result.
pub struct Probed {
    /// The probed task's own id — needed wherever a later assertion must
    /// name exactly which task an accusation cites (e.g. the passive path's
    /// `MemoryContradicted` row after an injected-but-unverified probe
    /// task), not merely that one exists.
    pub task_id: String,
    pub result: TaskResult,
    /// The stamp, from the replayed journal.
    pub stamp: Stamp,
    /// `(status, contradicted_by)` of the minted episode, re-read from the
    /// store FILE at the probe moment.
    pub stored: (String, Option<String>),
    /// Rows in the store file at the probe moment — see [`store_rows`].
    /// Taken in the same locked window as `stored`, and for the same reason:
    /// after the task finishes, passive falsification may have appended a row
    /// of its own, and a count taken then could not tell the two apart.
    pub store_rows: usize,
    /// The `MemoryContradicted` rows on the journal at the probe moment.
    pub contradicted: Vec<(String, String)>,
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
pub fn probe(m: &Minted, dir: &Path, spec: TaskSpec, ctx: Arc<MemoryContext>) -> Probed {
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
pub fn untouched() -> (String, Option<String>) {
    ("verified".to_string(), None)
}
