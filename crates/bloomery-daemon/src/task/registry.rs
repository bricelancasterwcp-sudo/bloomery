//! The task registry (Phase 2b/2c P3 Task 5): turns a request to run a task
//! into a background `std::thread` and a pollable id.
//!
//! **Concurrency, stated once, precisely — the binding decision for this
//! module.** `run_task` (Task 4) takes `&mut Pager<S>` for its whole call:
//! there is no way to lock the pager only for its per-step `infer` calls
//! without either rewriting `run_task`'s signature (out of Task 5's scope)
//! or wrapping every pager access behind a second, redundant lock. So
//! [`TaskRegistry::spawn_task`]'s worker thread takes the `Arc<Mutex<Pager<S>>>`
//! lock **once, for the task's entire duration** — including the time
//! `exec_run` spends blocked on its subprocess and the time every executor
//! spends on file I/O — rather than per `infer` call.
//!
//! This is a deliberate v1 compromise, not an oversight: one GPU already
//! serializes every `infer` call daemon-wide (see the README's "One coarse
//! lock" honest limit, which Task 5 extends), so a task that is itself
//! serialized behind that same lock adds no *new* serialization the daemon
//! didn't already have. The cost this does add: a long-running `run` step
//! (bounded by `ExecBounds::run_timeout_secs`, default 120s) now blocks
//! every *other* agent's inference for up to that long, not just the task's
//! own. That risk is real and is named here, in the README, and in the
//! Task 5 report rather than discovered later — revisiting it means
//! threading a lock-per-`infer` shape through `run_task` itself, deferred
//! past P3.
//!
//! A useful side effect of holding the lock for the whole task: at most one
//! task's `run_task` call is ever in flight at a time, which is also what
//! makes it safe for every task to open its **own** `Journal` handle onto
//! the *same* `tasks.jsonl` file (`Pager::task_journal_path`) rather than
//! sharing one writer — there is never a second concurrent writer to race
//! against, unlike `Pager::journal_post`'s boot-time concern (see that
//! method's doc comment) where POST really does run concurrently with
//! request-serving threads.
//!
//! **Panic containment (review fix).** `run_task` is unwrap-free in
//! production, but a worker thread holds the pager's `MutexGuard` for the
//! whole call above, so a panic anywhere under it would do two bad things
//! at once if left uncaught: the closure unwinds straight past the entry's
//! terminal write, so the registry entry is stuck `Running` forever (a
//! poller has no way to observe the failure); and the unwind carries past
//! the live `MutexGuard`, which poisons the shared `Mutex<Pager<S>>` —
//! `api_native::lock_pager`'s sticky-poison handling then degrades *every*
//! other request on the daemon to a `500`, so one task's panic takes down
//! everything, not just its own poll. [`TaskRegistry::spawn_task`] wraps
//! the `run_task` call itself in `std::panic::catch_unwind`, **inside** the
//! locked scope: the panic is caught before it ever reaches the guard's
//! `Drop`, so the guard is dropped the ordinary way (not via unwind) and
//! the mutex is never poisoned. A caught panic still ends the task —
//! `TaskStatus::Error`, with a message extracted from the payload where
//! possible — so a poller sees a terminal state either way. This closes
//! the availability gap; it does **not** prove the pager's own in-memory
//! state (`table`, `models`) is still fully consistent after a panic
//! mid-mutation — only that the *lock* itself survives clean, which is
//! what stops the failure from cascading to every other request.
//!
//! **The memory organ's pipeline lives here** (memory-organ design
//! `docs/superpowers/specs/2026-08-26-memory-organ-design.md` §4/§5), inside
//! the same worker thread and around the same `run_task` call: retrieve →
//! probe → stamp → inject → run → mint-or-contradict. This is the only place in the
//! daemon that has all three things the organ needs at once — the task's
//! `TaskSpec` (goal, grant, cwd) before step 1, the task's own `Journal`
//! handle, and the terminal `TaskResult` after. `api_task::create_task` has
//! the first but neither of the others, which is why the route threads a
//! [`crate::memory::MemoryContext`] down here instead of retrieving itself.
//!
//! The organ is **advisory in the strongest sense**: nothing it does can
//! change a task's status, steps or result. It writes to the journal, to the
//! store, and to `TaskSpec::memory_block` — and to nothing else. Every store
//! IO failure journals [`bloomery_core::journal::Event::Degraded`] and
//! continues, per design §7 ("the organ being broken can only ever produce
//! memory-off behavior — never a wrong injection, never a failed task").
//!
//! **The one execution the organ performs, and the law that permits it**
//! (refalsify-on-exact design
//! `docs/superpowers/specs/2026-08-27-refalsify-on-exact-design.md` §2/§3).
//! With `[memory] refalsify` on, [`organ_before_run`] re-runs a retrieved
//! episode's own stored verification command before injecting it. Organ
//! design §5's "the organ never executes anything" is revised by that spec
//! to **the organ never *initiates* execution**: the probe is task-scoped —
//! the incoming task's `Grant`, `cwd` and `ExecBounds`, through the same
//! `exec_run` that task's own `run` verb uses, at that task's moment.
//! Daemon-spontaneous execution stays banned, and everything else the organ
//! does is still read-outcomes-only. The probe never journals a `TaskStep`
//! and never renders into a prompt, so it is invisible to the transcript;
//! and with the flag off (the default) it does not exist at all.

use std::any::Any;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use bloomery_core::journal::{Event, Journal};
use bloomery_substrate::Substrate;

use crate::memory::mint::{build_episode, verifying_run, MintInputs};
use crate::memory::render::render_memory_block;
use crate::memory::store::MemoryStore;
use crate::memory::{MemoryContext, MEMORY_BLOCK_MAX_BYTES};
use crate::pager::Pager;
use crate::task::{exec_run, run_task, Observation, TaskResult, TaskSpec, TaskStatus};

/// `Arc<Mutex<HashMap<task id, TaskResult>>>` per the Task 5 brief.
/// `TaskResult` already carries `status`, so a `Running` entry is simply one
/// whose `status` hasn't reached a terminal variant yet, `steps` is still
/// empty, and `summary` is still `None` — no separate "in-flight" type is
/// needed alongside it.
type Entries = Arc<Mutex<HashMap<String, TaskResult>>>;

/// Extracts a human-readable message from a caught panic's payload.
/// `panic!("...")`, `.expect("...")`, and `.unwrap()` all carry either
/// `&'static str` or `String` — the two cases this checks — so this covers
/// every panic this codebase's own code can raise; a payload of any other
/// type (a foreign dependency's custom panic value) falls back to a named
/// generic message rather than guessing at its shape.
///
/// `pub(crate)` because P4's codec probe catches a panic from the *same*
/// `run_task` call under the *same* held pager guard, for the same
/// mutex-poisoning reason this module documents — one shared extractor, not
/// two spellings of the same message.
pub(crate) fn panic_message(payload: &(dyn Any + Send + 'static)) -> String {
    match panic_payload_message(payload) {
        Some(said) => format!("task worker panicked: {said}"),
        None => "task worker panicked (no string message on the payload)".to_string(),
    }
}

/// The words a caught panic's payload carries, when it carries any — the
/// reading half of [`panic_message`], without its subject.
///
/// `pub(crate)` for the swap-candidate spawn site (`api_native`), which
/// catches a panic from a *different* worker: describing that one as a "task
/// worker" would name the wrong subsystem in an operator's only record of it.
/// One extractor, three subjects.
pub(crate) fn panic_payload_message(payload: &(dyn Any + Send + 'static)) -> Option<String> {
    if let Some(s) = payload.downcast_ref::<&str>() {
        Some((*s).to_string())
    } else {
        payload.downcast_ref::<String>().cloned()
    }
}

/// Locks `entries`, recovering from poison rather than propagating it.
///
/// Unlike `api_native::lock_pager`'s *sticky* poison handling (which
/// protects the pager's law-4 mutation state — see that function's doc
/// comment), a poisoned registry lock only ever means some worker thread
/// panicked mid-`insert` into this plain bookkeeping map. Recovering with
/// `into_inner` loses at most the one write that was racing the panic, not
/// an unverifiable pager mutation, so it is not worth wedging every future
/// task lookup over.
fn lock_entries(
    entries: &Mutex<HashMap<String, TaskResult>>,
) -> MutexGuard<'_, HashMap<String, TaskResult>> {
    entries
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// A [`TaskResult`] for a task this registry has no execution evidence for:
/// the `Running` placeholder written before the worker thread starts, and
/// the three arms where `run_task` either never ran (journal open failed,
/// pager poisoned) or did not return (a caught panic).
///
/// Every field is empty, including the memory-organ capture
/// (`docs/superpowers/specs/2026-08-26-memory-organ-design.md` §2). That is
/// the honest value, not a placeholder to fill in later — most sharply for
/// the caught-panic arm: a task that died mid-flight touched files this
/// thread cannot enumerate, and the store may "only ever contain what has
/// execution evidence" (§2). Reporting nothing is correct; guessing is not.
fn without_evidence(status: TaskStatus, summary: Option<String>) -> TaskResult {
    TaskResult {
        status,
        steps: Vec::new(),
        summary,
        touched_files: std::collections::BTreeMap::new(),
        landed_patches: Vec::new(),
    }
}

/// Locks the memory store, recovering from poison rather than propagating
/// it, and journaling the recovery so it is never silent.
///
/// Same reasoning as [`lock_entries`], one layer up: a poisoned store mutex
/// means some worker panicked between the store's durable append and its
/// in-memory index update. The **file** is the source of truth and is
/// rebuilt at every `MemoryStore::load` (`memory::store`'s module docs), so
/// the worst a recovered lock carries is one index entry that the next boot
/// re-derives correctly — while refusing to recover would wedge the organ
/// for the rest of the process's life, which is a strictly worse outcome for
/// something advisory. The `Degraded` row is what keeps that trade visible
/// to an operator instead of buried.
fn lock_store<'a>(
    store: &'a Mutex<MemoryStore>,
    journal: &mut Journal,
) -> MutexGuard<'a, MemoryStore> {
    match store.lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            degrade(
                journal,
                "memory organ: store mutex poisoned by a prior panic; recovered from the \
                 durable file's own index"
                    .to_string(),
            );
            poisoned.into_inner()
        }
    }
}

/// Appends one memory-organ row, best-effort — the single spelling every
/// organ row in this module goes through.
///
/// **The append's own `io::Result` is deliberately dropped, and this is the
/// one place that says why.** The journal *is* the organ's reporting
/// channel, so a failed append has nowhere left to be reported: the obvious
/// "handle it" — journal the failure — is the very operation that just
/// failed. Escalating instead would let a broken journal fail a task the
/// organ is forbidden to touch (design §7). Nor is the condition lost:
/// `run_task` writes every one of its own `TaskStep` rows through this same
/// handle and *does* end the task on an append error, so a journal that
/// cannot be written surfaces through the task's own result — loudly, and
/// through the subsystem that owns the failure.
fn record(journal: &mut Journal, event: &Event) {
    let _ = journal.append(event);
}

/// Journals one memory-organ degradation and returns — the organ's only
/// reaction to any store failure (design §7: "Mint-time store IO failure:
/// journal a warning row; the task's own result is unaffected").
fn degrade(journal: &mut Journal, reason: String) {
    record(journal, &Event::Degraded { reason });
}

/// The words a caught panic carries, phrased for a `Degraded` reason — the
/// organ's counterpart to [`panic_message`]'s task-worker subject, built on
/// the same [`panic_payload_message`] extractor so there is one place that
/// knows how to read a payload.
fn panic_note(payload: &(dyn Any + Send + 'static)) -> String {
    panic_payload_message(payload).unwrap_or_else(|| "no string message on the payload".to_string())
}

/// Runs one memory-organ region under panic containment, journaling a
/// `Degraded` row and returning `None` if it unwinds.
///
/// **This is the module's "Panic containment" discipline applied to the
/// organ.** That section explains why a panic under the worker's `run_task`
/// call had to be caught: an unwind escapes the closure, so the thread dies
/// before the terminal registry write and `TaskRegistry::get` reports
/// `Running` forever — an unbounded wait for every poller. The organ's own
/// code sits in the same thread, on both sides of that call, and inherits
/// the identical hazard plus a second one: a panic in the POST-run region
/// would discard a `TaskResult` the task had already earned.
///
/// It also inherits the organ's own hard constraint (memory-organ design §7,
/// "the organ being broken can only ever produce memory-off behavior — never
/// a wrong injection, never a failed task"): the organ is advisory, so
/// nothing it does — including dying — may change a task's status, steps or
/// result. Containment is what makes that true of a *panic* and not merely
/// of the `Result`-returning failures every organ call already handles.
///
/// Every organ function is unwrap-free today and every fallible operation in
/// them returns `Result`, so this wrapper is defence in depth rather than a
/// live bug fix — which is exactly why it must be structural: the next
/// contributor to `retrieve`/`render`/`mint` should not have to know that an
/// index-out-of-bounds there wedges unrelated tasks.
///
/// `f` is handed the journal by reborrow (not by move) so the caller keeps
/// using the same handle afterward — including for the `Degraded` row this
/// writes on the failing path.
fn contained<T>(
    journal: &mut Journal,
    region: &str,
    f: impl FnOnce(&mut Journal) -> T,
) -> Option<T> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(&mut *journal))) {
        Ok(value) => Some(value),
        Err(payload) => {
            degrade(
                journal,
                format!(
                    "memory organ: {region} panicked ({}); the task itself is unaffected",
                    panic_note(payload.as_ref())
                ),
            );
            None
        }
    }
}

/// What the organ decided before the task ran: what its stamp will say, and
/// the block (if any) to inject into the spec.
///
/// Returned rather than written straight into the `TaskSpec` so that a panic
/// inside [`organ_before_run`] cannot leave a half-injected spec behind: the
/// caller assigns `spec.memory_block` only from a decision that was returned
/// intact, so an unwinding organ injects nothing at all rather than
/// something partial.
struct OrganDecision {
    /// `"off"` | `"silent"` | `"injected"` — see `Event::MemoryStamp`.
    mode: &'static str,
    candidates_checked: u32,
    injected_id: Option<String>,
    block: Option<String>,
    /// The refalsification verdict this retrieval earned — see
    /// `Event::MemoryStamp::refalsify` for the closed set of spellings.
    /// `None` means nothing was probed, which covers every decision the
    /// probe never reaches: memory off, nothing retrieved, an oversize skip
    /// (see [`organ_before_run`] on why the probe runs *after* that check),
    /// and every decision at all while `[memory] refalsify` is off.
    refalsify: Option<&'static str>,
}

impl OrganDecision {
    /// The memory-off decision: the organ said nothing and examined nothing.
    /// Also the honest fallback when the organ *panicked* before deciding —
    /// design §7 already folds "the organ is broken" into the `"off"` mode
    /// (that is what the store-unreadable-at-boot case stamps), and the
    /// accompanying `Degraded` row carries the why.
    fn off() -> OrganDecision {
        OrganDecision {
            mode: "off",
            candidates_checked: 0,
            injected_id: None,
            block: None,
            refalsify: None,
        }
    }
}

/// The probe's verdict, read off `exec_run`'s [`Observation`] (refalsify
/// spec `docs/superpowers/specs/2026-08-27-refalsify-on-exact-design.md`
/// §2.3).
///
/// **Only a genuine nonzero exit contradicts.** A run that *completed*
/// carries `failed: false` and `exec_run`'s pinned success-arm outcome
/// `"ran {program} exit {code}"` — the same load-bearing constant
/// `memory::mint::verifying_run` reads the mint bar out of. A `code` of `-1`
/// is that arm's "no exit code" sentinel for a child killed by a signal
/// (`status.code()` was `None`), not a real exit, so it can never be read as
/// evidence the lesson is wrong. Every `failed: true` arm — timeout, spawn
/// failure, wait failure — is environmental rather than semantic and
/// classifies `inconclusive`, so the probe's own infrastructure can never
/// cost a task its injection (spec §2.3; organ design §7).
///
/// Grant refusals never reach here: [`organ_before_run`]'s pre-check runs
/// BEFORE anything spawns, precisely because a refusal `Observation` is
/// shaped like a failed run and must never be mistakable for evidence.
fn classify_probe(obs: &Observation) -> &'static str {
    if obs.failed {
        return "inconclusive";
    }
    // `rfind`, not `find`: `argv[0]` is interpolated into the outcome ahead
    // of the suffix, and a program named with a literal " exit " in it would
    // otherwise capture the parse.
    match obs
        .outcome
        .rfind(" exit ")
        .and_then(|i| obs.outcome[i + " exit ".len()..].parse::<i64>().ok())
    {
        Some(0) => "passed",
        Some(code) if code > 0 => "failed",
        // The signal sentinel (-1), and any outcome this cannot parse at
        // all: neither is a clean nonzero exit, so neither accuses.
        _ => "inconclusive",
    }
}

/// Steps 1-3 of the pipeline (design §3/§4), plus refalsification's probe
/// (refalsify spec
/// `docs/superpowers/specs/2026-08-27-refalsify-on-exact-design.md` §2):
/// retrieve, then decide whether to inject, silence, or stay off.
///
/// Rendering happens here, before the caller writes the stamp, because the
/// oversize rule can turn an otherwise-injected episode into a silent one —
/// and a stamp claiming an injection the prompt never carried would be the
/// one lie that row exists to prevent.
///
/// `agent_id` is needed only by the probe's failing arm, which journals the
/// ordinary `Event::MemoryContradicted` row — the same row and the same
/// accusation shape passive contradiction uses in [`organ_after_run`].
fn organ_before_run(
    organ: Option<(&Mutex<MemoryStore>, usize, bool)>,
    spec: &TaskSpec,
    task_id: &str,
    agent_id: &str,
    journal: &mut Journal,
) -> OrganDecision {
    // Step 1: retrieve, holding the store lock only for the read itself —
    // never across `run_task`, which would serialize every other task's
    // retrieval behind this one's whole execution.
    //
    // The tuple's third element is the `[memory] refalsify` opt-in
    // (refalsify spec §5); the probe it gates is below, after the two-stage
    // exact gate and the oversize rule have both said this episode would
    // otherwise inject.
    let Some((store, _, refalsify)) = organ else {
        return OrganDecision::off();
    };
    let retrieval = {
        let store = lock_store(store, journal);
        crate::memory::retrieve::retrieve(&store, &spec.goal, &spec.grant, &spec.cwd)
    };

    let Some(episode) = retrieval.injected else {
        return OrganDecision {
            mode: "silent",
            candidates_checked: retrieval.candidates_checked,
            injected_id: None,
            block: None,
            refalsify: None,
        };
    };

    let block = render_memory_block(&episode);
    if block.len() > MEMORY_BLOCK_MAX_BYTES {
        // Controller ruling (Task 6), see `MEMORY_BLOCK_MAX_BYTES`: an
        // oversized block could push this task into `WindowExhausted` where
        // memory-off would have finished, and design §7 forbids the organ
        // damaging the task. Skipped, stamped silent, and named in a
        // `Degraded` row — `injected_id` stays `None`, so a later failure
        // can never be blamed on an episode this prompt did not carry.
        degrade(
            journal,
            format!(
                "memory organ: episode {} rendered {} bytes, over the \
                 {MEMORY_BLOCK_MAX_BYTES}-byte injection bound; task {task_id} runs memory-off",
                episode.episode_id,
                block.len(),
            ),
        );
        return OrganDecision {
            mode: "silent",
            candidates_checked: retrieval.candidates_checked,
            injected_id: None,
            block: None,
            refalsify: None,
        };
    }

    // Refalsify-on-exact (refalsify spec §2): re-run the episode's own
    // stored verification under THIS task's granted capability before
    // trusting it.
    //
    // **Order: deliberately AFTER the oversize check.** The probe runs only
    // on an episode that would otherwise inject, so a block this task was
    // never going to carry costs no execution at all — and the oversize skip
    // above keeps stamping `refalsify: None`, which is the truth of it:
    // nothing was probed. The other order (probe, then discover the block is
    // too big) would spend a subprocess to learn something the cheap check
    // already knew.
    //
    // The store lock is NOT held across this. Retrieval released it above,
    // and a probe may legitimately run for the whole of
    // `ExecBounds::run_timeout_secs` — holding the store across that would
    // serialize every other task's retrieval behind this one's subprocess.
    let verdict = if refalsify {
        // Coverage pre-check and demotion, BEFORE any execution attempt
        // (spec §2.1). An `exec_run` grant refusal is an `Observation`
        // shaped like a failed run, and a refusal must never be mistakable
        // for evidence — so the refusal case is decided here rather than
        // classified from an outcome string. Demotion outranks the grant: a
        // task that may not `run` has no commands executed at its moment,
        // whatever its grant happens to say.
        if !spec.mutating_verbs
            || spec
                .grant
                .check_command(&episode.run_evidence.argv)
                .is_err()
        {
            Some("skipped_ungranted")
        } else {
            // The task loop's own executor, with the incoming task's grant,
            // cwd and bounds — the identical capability check, output cap
            // and timeout this task's own `run` verb would get. A probe
            // never journals a `TaskStep` and never renders into a prompt:
            // it is not a model action.
            let obs = exec_run(
                &spec.grant,
                &spec.cwd,
                &episode.run_evidence.argv,
                &spec.bounds,
            );
            Some(classify_probe(&obs))
        }
    } else {
        None
    };

    if verdict == Some("failed") {
        // The same accusation mechanism passive contradiction uses (spec
        // §2.3): mark the store, journal the ordinary `MemoryContradicted`
        // row citing THIS task, and hand the task silence — byte-identical
        // to a stranger's prompt.
        //
        // The three arms mirror `organ_after_run`'s exactly, including
        // R-PF-2's `Ok(false)`: an operator's `DELETE /memory/{id}` can land
        // between this task's retrieval and its probe, and journaling a
        // contradiction for a row that never changed would fabricate store
        // history. The lock is taken for the mark alone and released before
        // the journal write.
        //
        // Whatever any of that reports, the silence stands (spec §7): the
        // task must not receive guidance the probe just refuted, even if
        // recording the refutation failed.
        let marked = {
            let mut store = lock_store(store, journal);
            store.mark_contradicted(&episode.episode_id, task_id)
        };
        match marked {
            Ok(true) => record(
                journal,
                &Event::MemoryContradicted {
                    id: agent_id.to_string(),
                    task_id: task_id.to_string(),
                    episode_id: episode.episode_id.clone(),
                },
            ),
            Ok(false) => {}
            Err(e) => degrade(
                journal,
                format!(
                    "memory organ: could not contradict episode {} refuted by task {task_id}'s \
                     refalsification probe: {e}",
                    episode.episode_id
                ),
            ),
        }
        return OrganDecision {
            mode: "silent",
            candidates_checked: retrieval.candidates_checked,
            injected_id: None,
            block: None,
            refalsify: verdict,
        };
    }

    OrganDecision {
        mode: "injected",
        candidates_checked: retrieval.candidates_checked,
        injected_id: Some(episode.episode_id.clone()),
        block: Some(block),
        refalsify: verdict,
    }
}

/// Everything [`organ_after_run`] needs beyond the task's own `TaskResult` —
/// bundled into one struct (rather than six `&str` params) for the same
/// too-many-arguments reason `task_loop::TaskState` is.
struct OrganOutcome<'a> {
    goal: &'a str,
    /// Provenance, recorded and never compared (design §2).
    model: &'a str,
    envelope: &'a str,
    agent_id: &'a str,
    task_id: &'a str,
    /// The episode this task's prompt actually carried, if any.
    injected_id: Option<&'a str>,
}

/// Step 6 of the pipeline (design §5, then §2): contradict what the task
/// falsified, then mint what it verified. Called after the pager guard has
/// dropped, so the store lock is never held alongside it.
fn organ_after_run(
    organ: Option<(&Mutex<MemoryStore>, usize, bool)>,
    result: &TaskResult,
    outcome: &OrganOutcome<'_>,
    journal: &mut Journal,
) {
    // The tuple's `refalsify` flag is a *before*-run concern only (refalsify
    // spec `docs/superpowers/specs/2026-08-27-refalsify-on-exact-design.md`
    // §2: the probe runs at the retrieval moment, before injection); nothing
    // after the run reads it.
    let Some((store, max_episodes, _)) = organ else {
        return;
    };
    let mut store = lock_store(store, journal);

    if let Some(id) = outcome.injected_id {
        // Design §5: a task that received an episode and then failed its own
        // verification contradicts it. Both conjuncts are load-bearing and
        // neither implies the other:
        //
        // - `is_scored_outcome` — the task must have *measured* something.
        //   `TaskStatus::Error` is bloomery's infra bucket, not a verdict
        //   about the episode; see that function's doc comment for the
        //   ruling, the G4 protocol citation, and why an infra hiccup must
        //   never read as a fresh contradiction.
        // - `verifying_run(..).is_none()` — within a scored outcome, the
        //   task produced no productive run after its last landed patch
        //   (spec §2's bar, read backwards).
        //
        // A task that verifies falls through to the mint below, which
        // refreshes the same identity.
        if is_scored_outcome(&result.status) && verifying_run(result).is_none() {
            match store.mark_contradicted(id, outcome.task_id) {
                Ok(true) => record(
                    journal,
                    &Event::MemoryContradicted {
                        id: outcome.agent_id.to_string(),
                        task_id: outcome.task_id.to_string(),
                        episode_id: id.to_string(),
                    },
                ),
                // R-PF-2: the id is gone from the store — an operator's
                // `DELETE /memory/{id}` can land while this task runs. No
                // row was written, so journaling a contradiction here would
                // fabricate store history: a replay would see an episode
                // change status when nothing did.
                Ok(false) => {}
                Err(e) => degrade(
                    journal,
                    format!(
                        "memory organ: could not contradict episode {id} from task {}: {e}",
                        outcome.task_id
                    ),
                ),
            }
        }
    }

    let inputs = MintInputs {
        goal: outcome.goal,
        model: outcome.model,
        envelope: outcome.envelope,
        minted_at: now_millis(),
    };
    if let Some(episode) = build_episode(result, &inputs) {
        let episode_id = episode.episode_id.clone();
        match store.mint(episode, max_episodes) {
            Ok(()) => record(
                journal,
                &Event::MemoryMint {
                    id: outcome.agent_id.to_string(),
                    task_id: outcome.task_id.to_string(),
                    episode_id,
                },
            ),
            Err(e) => degrade(
                journal,
                format!(
                    "memory organ: could not mint episode {episode_id} from task {}: {e}",
                    outcome.task_id
                ),
            ),
        }
    }
}

/// Whether `status` is a **scored** terminal outcome — one this daemon
/// actually measured — rather than its infrastructure bucket.
///
/// **Controller ruling (2026-08-26): passive contradiction fires only on a
/// scored outcome.** Memory-organ design §5 makes contradiction conditional
/// on a task that "fails its own verification", and a *failure to verify* is
/// a measurement: it says the injected episode did not reproduce. A
/// `TaskStatus::Error` says nothing of the kind. It is bloomery's own
/// infrastructure abort — substrate faults, journal failures, a caught
/// worker panic, a poisoned pager — and the daemon already draws exactly
/// this line elsewhere: the G4 protocol
/// (`docs/superpowers/evidence/2026-08-15-g4-protocol.md` §3) scores
/// `Done`/`StepsExhausted`/`BudgetExhausted` and classifies
/// `TaskStatus::Error` as "an **infrastructure abort** … the model is
/// *unmeasured*", and its Amendment 1 (§9) moved `WindowExhausted` into the
/// scored set for the same reason — an envelope-bounded resource ran out,
/// which is a result, where a fault is not.
///
/// The falsification principle underneath it: an infra hiccup must never
/// look like a fresh contradiction. Retiring a verified episode because the
/// daemon's own mutex was poisoned would put a measurement-shaped claim in
/// the store that no measurement supports — and §5's whole design is that
/// "the organ only ever reads outcomes, never creates them". An unmeasured
/// task produces no outcome to read, so the episode STANDS.
///
/// Matched exhaustively, with no wildcard arm, so a future `TaskStatus`
/// variant forces this classification to be decided rather than defaulted.
/// `Running` is unreachable here (the worker only ever classifies a terminal
/// result) and is grouped with the unscored side: a task still in flight has
/// verified nothing.
fn is_scored_outcome(status: &TaskStatus) -> bool {
    match status {
        TaskStatus::Done
        | TaskStatus::StepsExhausted
        | TaskStatus::BudgetExhausted
        | TaskStatus::WindowExhausted => true,
        TaskStatus::Error | TaskStatus::Running => false,
    }
}

/// The wall-clock mint stamp for an episode, in milliseconds since the Unix
/// epoch — the same expression `Journal::append` derives its row stamp from
/// (`bloomery-core/src/journal.rs`), including its two saturating edges: a
/// pre-1970 clock reads `0` (visibly absurd rather than silently plausible)
/// and a count past `u64` milliseconds saturates rather than truncating.
///
/// One spelling, so an episode's `minted_at` and the `MemoryMint` row's own
/// `epoch_ms` can never disagree about what "now" means.
fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

/// A running or finished task, keyed by a monotonic `task-<n>` id.
pub struct TaskRegistry {
    entries: Entries,
    /// `Date`/random generators are avoided deliberately (the P3 plan's own
    /// constraint): a process-wide monotonic counter gives a deterministic,
    /// unique-per-process id (`task-1`, `task-2`, ...) with no clock and no
    /// RNG dependency.
    next_id: AtomicU64,
}

impl Default for TaskRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl TaskRegistry {
    pub fn new() -> Self {
        TaskRegistry {
            entries: Arc::new(Mutex::new(HashMap::new())),
            next_id: AtomicU64::new(1),
        }
    }

    /// Allocates a new task id, records a `Running` placeholder entry for
    /// it, and spawns a background thread that runs `spec` to completion
    /// against `pager` and `agent_id`, then overwrites the entry with the
    /// terminal `TaskResult`. Returns the new id immediately — this call
    /// never blocks on the task itself.
    ///
    /// `journal_path` is opened fresh, inside the spawned thread — see this
    /// module's docs for why a second `Journal` handle onto the same file
    /// is safe here specifically (it would not be everywhere in this
    /// codebase; `Pager::journal_post`'s doc comment names the general
    /// hazard).
    ///
    /// `memory` is the daemon's memory organ, or `None` for a caller that
    /// has none to offer. `Some` does not mean the organ speaks: it speaks
    /// only when [`MemoryContext::operational`] holds, and even then only
    /// when retrieval finds a survivor. Every other case runs the task
    /// exactly as it ran before this parameter existed — see the module docs
    /// for the pipeline this drives.
    pub fn spawn_task<S: Substrate + Send + 'static>(
        &self,
        pager: Arc<Mutex<Pager<S>>>,
        agent_id: String,
        spec: TaskSpec,
        journal_path: PathBuf,
        memory: Option<Arc<MemoryContext>>,
    ) -> String {
        let n = self.next_id.fetch_add(1, Ordering::Relaxed);
        let task_id = format!("task-{n}");

        {
            let mut entries = lock_entries(&self.entries);
            entries.insert(task_id.clone(), without_evidence(TaskStatus::Running, None));
        }

        let entries = Arc::clone(&self.entries);
        let worker_task_id = task_id.clone();
        std::thread::spawn(move || {
            let mut spec = spec;
            let mut journal = match Journal::open(&journal_path) {
                Ok(j) => j,
                Err(e) => {
                    // No journal means no stamp: design §4's "every task is
                    // stamped" is a statement about tasks that run, and this
                    // one never reaches step 1. Inventing a stamp elsewhere
                    // would put a row about this task in a file this task
                    // could not open.
                    let mut entries = lock_entries(&entries);
                    entries.insert(
                        worker_task_id,
                        without_evidence(
                            TaskStatus::Error,
                            Some(format!(
                                "failed to open task journal {}: {e}",
                                journal_path.display()
                            )),
                        ),
                    );
                    return;
                }
            };

            // The organ's handle for this task: the store, the retention cap,
            // and the refalsify-on-exact opt-in
            // (`docs/superpowers/specs/2026-08-27-refalsify-on-exact-design.md`
            // §5). `Some` exactly when the config switch is on AND a store
            // loaded at boot. Re-deriving the store from `operational()`'s own
            // two conjuncts is what keeps this `expect`-free — the invariant is
            // documented on `MemoryContext`, and read here rather than trusted.
            // `refalsify` rides along unconditionally: it is only ever read
            // inside a `Some` arm, so an off organ can never act on it.
            let organ: Option<(&Mutex<MemoryStore>, usize, bool)> = memory
                .as_ref()
                .filter(|ctx| ctx.operational())
                .and_then(|ctx| {
                    ctx.store
                        .as_ref()
                        .map(|store| (store, ctx.max_episodes, ctx.refalsify))
                });

            // Steps 1-3 (design §3/§4), under panic containment: this whole
            // region runs in the worker thread ahead of the terminal
            // registry write, so an unwind here would wedge the task at
            // `Running` forever — the same failure this module's "Panic
            // containment" section closed for `run_task`, and the same
            // advisory-organ constraint (design §7) that forbids the organ
            // changing a task's outcome. A panicked organ decides `off`.
            let decision = contained(
                &mut journal,
                &format!("retrieval for task {worker_task_id}"),
                |journal| organ_before_run(organ, &spec, &worker_task_id, &agent_id, journal),
            )
            .unwrap_or_else(OrganDecision::off);
            // Step 3: injection is the caller's write, from a decision that
            // came back intact — see `OrganDecision`.
            spec.memory_block = decision.block;
            let injected_id = decision.injected_id;
            // Step 2: the stamp.
            record(
                &mut journal,
                &Event::MemoryStamp {
                    id: agent_id.clone(),
                    task_id: worker_task_id.clone(),
                    mode: decision.mode.to_string(),
                    episode_id: injected_id.clone(),
                    candidates_checked: decision.candidates_checked,
                    refalsify: decision.refalsify.map(String::from),
                },
            );

            // Recorded, never compared (design §2) — the envelope this task
            // ran under, read from the spec and so needing no pager guard.
            let envelope = format!("{:?}", spec.envelope);

            // Step 5. The v1 locking decision, exactly: one lock, held for
            // the whole task — see this module's docs.
            let (result, model) = match pager.lock() {
                Ok(mut guard) => {
                    // Step 4: the other half of the provenance pair, which
                    // *does* need the guard (it is an agent-table lookup).
                    // An agent that vanished mid-flight falls back to its
                    // own id rather than an empty string — a name that is
                    // visibly not a model beats a blank that reads as one.
                    let model = guard
                        .agent_model(&agent_id)
                        .unwrap_or_else(|| agent_id.clone());
                    // Catch a panic from `run_task` HERE, inside the locked
                    // scope, so it is caught before it would ever reach
                    // `guard`'s `Drop` — see this module's "Panic
                    // containment" doc section for the full reasoning. A
                    // caught panic lets `guard` drop the ordinary way
                    // (never via unwind), so the mutex is never poisoned.
                    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        run_task(&mut guard, &agent_id, &spec, &mut journal)
                    }));
                    let result = match outcome {
                        Ok(result) => result,
                        Err(payload) => without_evidence(
                            TaskStatus::Error,
                            Some(panic_message(payload.as_ref())),
                        ),
                    };
                    (result, model)
                }
                Err(_) => {
                    // Deliberately NOT `.into_inner()` here: this is the
                    // pager's own mutex, not the registry's bookkeeping
                    // one, and `api_native::lock_pager`'s sticky-poison
                    // reasoning applies in full — a poisoned pager's state
                    // is not vouched for, so this task did not run. There is
                    // no agent table to read a model out of either, so the
                    // same fallback the `Ok` arm uses stands in; nothing
                    // reads it, because an `Error` result can never mint.
                    (
                        without_evidence(
                            TaskStatus::Error,
                            Some(
                                "pager state poisoned by a prior panic; restart the daemon"
                                    .to_string(),
                            ),
                        ),
                        agent_id.clone(),
                    )
                }
            };

            // Step 6 (design §5, then §2), under the same containment as the
            // pre-run region — and here the stakes are higher: an unwind
            // would discard a `TaskResult` this task had already earned,
            // leaving `get` on `Running` forever with the work done and
            // thrown away. `contained`'s `Option` is only "did it finish";
            // the `Degraded` row it writes carries the why, so nothing here
            // needs to branch on it.
            let outcome = OrganOutcome {
                goal: &spec.goal,
                model: &model,
                envelope: &envelope,
                agent_id: &agent_id,
                task_id: &worker_task_id,
                injected_id: injected_id.as_deref(),
            };
            let _completed = contained(
                &mut journal,
                &format!("mint/contradiction for task {worker_task_id}"),
                |journal| organ_after_run(organ, &result, &outcome, journal),
            );

            // Step 7: the registry entry, unchanged — the organ has not
            // touched `result` on any path above, and cannot skip this write
            // on any path either.
            let mut entries = lock_entries(&entries);
            entries.insert(worker_task_id, result);
        });

        task_id
    }

    /// A snapshot of one task's current state, or `None` if `task_id` is
    /// unknown. Poll-based: this reads whatever the last write left behind,
    /// with no push/streaming path.
    pub fn get(&self, task_id: &str) -> Option<TaskResult> {
        let entries = lock_entries(&self.entries);
        entries.get(task_id).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bloomery_core::gguf::GgufMeta;
    use bloomery_core::grant::Grant;
    use bloomery_substrate::fake::FakeSubstrate;
    use bloomery_substrate::Reply;

    use crate::agents::ImageStore;
    use crate::config::EnvelopeLens;
    use crate::task::ExecBounds;
    use bloomery_core::action::PatchCodec;
    use bloomery_core::journal::replay;

    /// [`classify_probe`]'s rule, arm by arm (refalsify spec §2.3) — tested
    /// here rather than only through `tests/memory_refalsify_test.rs`
    /// because two of these arms have no honest full-fixture spelling.
    ///
    /// The `failed: true` guard is the sharp one. Today every `failed: true`
    /// outcome `exec_run` produces (`"ran {p} timed out"`, `"run failed:
    /// ..."`) happens to carry no `" exit "` substring, so the parse below
    /// falls through to `inconclusive` even with the guard deleted — a
    /// property of the current outcome *strings*, not of the rule. The rule
    /// is that a run which never COMPLETED carries no evidence about what
    /// the command would have reported, whatever its text says, so this
    /// hands the classifier a `failed: true` observation whose outcome
    /// *would* parse as a clean nonzero exit. Without that case, an
    /// executor that ever grew a timeout message containing an exit code
    /// would start contradicting episodes over a timeout, and nothing would
    /// notice.
    #[test]
    fn classify_probe_reads_only_completed_runs_and_only_real_exit_codes() {
        let obs = |outcome: &str, failed: bool| Observation {
            outcome: outcome.to_string(),
            content: String::new(),
            failed,
            touched: None,
        };

        assert_eq!(classify_probe(&obs("ran sh exit 0", false)), "passed");
        assert_eq!(classify_probe(&obs("ran sh exit 1", false)), "failed");
        assert_eq!(classify_probe(&obs("ran sh exit 137", false)), "failed");
        // `exec_run`'s signal-death sentinel: `status.code()` was `None`, so
        // -1 is "no exit code", not a nonzero one. It must never accuse.
        assert_eq!(
            classify_probe(&obs("ran sh exit -1", false)),
            "inconclusive"
        );
        // Never completed — environmental, not semantic, on both the arm
        // that exists today and the one that would parse if it did.
        assert_eq!(
            classify_probe(&obs("ran sh timed out", true)),
            "inconclusive"
        );
        assert_eq!(classify_probe(&obs("ran sh exit 1", true)), "inconclusive");
        assert_eq!(
            classify_probe(&obs("run failed: could not spawn \"sh\"", true)),
            "inconclusive"
        );
        // `rfind`, not `find`: argv[0] is interpolated ahead of the suffix,
        // so a program name containing " exit " must not capture the parse.
        assert_eq!(
            classify_probe(&obs("ran my exit 9 tool exit 0", false)),
            "passed"
        );
        // Unparseable is not an accusation either.
        assert_eq!(
            classify_probe(&obs("ran sh exit notanumber", false)),
            "inconclusive"
        );
    }

    /// The organ's panic containment, at the seam that carries it (review
    /// finding, 2026-08-26). Both organ regions run in the worker thread
    /// ahead of step 7's terminal registry write, so an unwind out of either
    /// would leave `TaskRegistry::get` reporting `Running` forever — and, in
    /// the post-run region, would discard a `TaskResult` the task had
    /// already earned. [`contained`] is what makes that impossible, and this
    /// pins its three properties:
    ///
    /// 1. a panicking region yields `None` rather than unwinding out;
    /// 2. the panic is journaled — a `Degraded` row naming the region and
    ///    carrying the payload's own words, so a swallowed panic is never
    ///    silent;
    /// 3. the caller keeps going, with the *same* journal handle still
    ///    usable — which is the whole point: step 7 must still run.
    ///
    /// Tested here rather than through the pipeline because every organ
    /// function is unwrap-free and returns `Result` for every fallible
    /// operation, so there is no honest way to make one panic from an
    /// integration test without adding a production seam that exists only
    /// for tests. This tests the guard itself, which both call sites use.
    #[test]
    fn contained_catches_a_panic_journals_it_and_lets_the_caller_continue() {
        let dir = fresh_dir("contained");
        let path = dir.join("j.jsonl");
        let mut journal = Journal::open(&path).unwrap();

        // Property 1: the panic does not escape, and the region reports it
        // did not finish.
        let outcome: Option<u8> = contained(&mut journal, "a scripted region", |_| {
            panic!("scripted organ panic");
        });
        assert!(outcome.is_none());

        // Property 3: the same handle still works afterward — a caller that
        // must write one more row (step 7's analogue) can.
        record(
            &mut journal,
            &Event::Degraded {
                reason: "the caller continued".to_string(),
            },
        );

        // The happy path returns the region's value untouched, so the guard
        // is not silently swallowing successes too.
        assert_eq!(contained(&mut journal, "a quiet region", |_| 7u8), Some(7));

        // Property 2: the panic is on the record, named and attributed.
        let reasons: Vec<String> = replay(&path)
            .unwrap()
            .into_iter()
            .filter_map(|e| match e {
                Event::Degraded { reason } => Some(reason),
                _ => None,
            })
            .collect();
        assert_eq!(reasons.len(), 2, "{reasons:?}");
        assert!(
            reasons[0].contains("a scripted region")
                && reasons[0].contains("scripted organ panic")
                && reasons[0].contains("the task itself is unaffected"),
            "the row must name the region, quote the payload, and say the \
             task is unaffected: {:?}",
            reasons[0]
        );
        assert_eq!(reasons[1], "the caller continued");
    }

    fn fresh_dir(tag: &str) -> PathBuf {
        static UNIQUE: AtomicU64 = AtomicU64::new(0);
        let unique = UNIQUE.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "bloomery-registry-{tag}-{}-{unique}",
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

    fn build_pager(dir: &std::path::Path, replies: Vec<Reply>) -> (Pager<FakeSubstrate>, String) {
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

    fn ok_grant(dir: &std::path::Path) -> Grant {
        let sb = std::fs::canonicalize(dir).unwrap();
        Grant::from_json(&format!(
            r#"{{"read_roots":["{s}"],"write_roots":["{s}"],"commands":[]}}"#,
            s = sb.display()
        ))
        .unwrap()
    }

    /// The registry's own contract, independent of HTTP: `spawn_task`
    /// returns immediately with a `task-<n>` id, `get` reports `Running`
    /// until the background thread finishes, and then the terminal
    /// `TaskResult` (steps included) is what a later `get` reads back.
    #[test]
    fn spawn_task_runs_in_background_and_get_reflects_completion() {
        let dir = fresh_dir("basic");
        let (pager, agent_id) = build_pager(
            &dir,
            vec![Reply {
                text: "<action verb=\"done\">\nall set\n</action>".to_string(),
                prompt_tokens: Some(8),
                completion_tokens: Some(4),
                duration_ms: 1,
            }],
        );
        let pager = Arc::new(Mutex::new(pager));
        let registry = TaskRegistry::new();
        // Demoted + WholeFile, deliberately not this module's other tests'
        // `true`/`SearchReplace` defaults: the registry's own contract
        // (background thread, pollable completion) does not depend on which
        // codec-gate policy a `TaskSpec` carries, and this test's single
        // `done` action never touches `patch_codec` or `mutating_verbs`
        // either way — so it doubles as coverage that the registry passes a
        // demoted, non-default-codec spec through to `run_task` untouched.
        let spec = TaskSpec {
            goal: "say done".to_string(),
            grant: ok_grant(&dir),
            budget_tokens: 1_000_000,
            max_steps: 3,
            cwd: std::fs::canonicalize(&dir).unwrap(),
            patch_codec: PatchCodec::WholeFile,
            bounds: ExecBounds::default(),
            mutating_verbs: false,
            envelope: EnvelopeLens::V1,
            memory_block: None,
            window_ladder: false,
        };

        let task_id = registry.spawn_task(
            Arc::clone(&pager),
            agent_id,
            spec,
            dir.join("tasks.jsonl"),
            None,
        );
        assert!(task_id.starts_with("task-"));

        let mut entry = registry.get(&task_id).expect("entry exists immediately");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while entry.status == TaskStatus::Running && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(10));
            entry = registry.get(&task_id).expect("entry still exists");
        }

        assert_eq!(entry.status, TaskStatus::Done, "{entry:?}");
        assert_eq!(entry.summary.as_deref(), Some("all set"));
        assert_eq!(entry.steps.len(), 1);
    }

    #[test]
    fn get_on_unknown_task_id_is_none() {
        let registry = TaskRegistry::new();
        assert!(registry.get("task-999").is_none());
    }

    /// Two `spawn_task` calls on the same registry never collide on an id —
    /// the monotonic counter, not a clock, is what guarantees this.
    #[test]
    fn task_ids_are_unique_and_monotonic() {
        let dir = fresh_dir("ids");
        let (pager, agent_id) = build_pager(
            &dir,
            vec![
                Reply {
                    text: "<action verb=\"done\">\nfirst\n</action>".to_string(),
                    prompt_tokens: Some(8),
                    completion_tokens: Some(4),
                    duration_ms: 1,
                },
                Reply {
                    text: "<action verb=\"done\">\nsecond\n</action>".to_string(),
                    prompt_tokens: Some(8),
                    completion_tokens: Some(4),
                    duration_ms: 1,
                },
            ],
        );
        let pager = Arc::new(Mutex::new(pager));
        let registry = TaskRegistry::new();
        let spec = |cwd: PathBuf, grant: Grant| TaskSpec {
            goal: "say done".to_string(),
            grant,
            budget_tokens: 1_000_000,
            max_steps: 3,
            cwd,
            patch_codec: PatchCodec::SearchReplace,
            bounds: ExecBounds::default(),
            mutating_verbs: true,
            envelope: EnvelopeLens::V1,
            memory_block: None,
            window_ladder: false,
        };

        let id1 = registry.spawn_task(
            Arc::clone(&pager),
            agent_id.clone(),
            spec(std::fs::canonicalize(&dir).unwrap(), ok_grant(&dir)),
            dir.join("tasks.jsonl"),
            None,
        );
        let id2 = registry.spawn_task(
            Arc::clone(&pager),
            agent_id,
            spec(std::fs::canonicalize(&dir).unwrap(), ok_grant(&dir)),
            dir.join("tasks.jsonl"),
            None,
        );
        assert_ne!(id1, id2);
    }

    /// A `Substrate` whose `infer` always panics — exists only to prove
    /// `spawn_task`'s `catch_unwind` does its job. Every other method is a
    /// trivial success; `run_task`'s first step reaches `infer` and nothing
    /// past it. Mirrors `api_native_test.rs`'s `PanicSubstrate`.
    struct PanicSubstrate;

    impl bloomery_substrate::Substrate for PanicSubstrate {
        fn load_model(
            &mut self,
            _path: &std::path::Path,
            _n_gpu_layers: u32,
        ) -> Result<bloomery_substrate::ModelHandle, bloomery_substrate::SubstrateError> {
            Ok(1)
        }

        fn unload_model(
            &mut self,
            _m: bloomery_substrate::ModelHandle,
        ) -> Result<(), bloomery_substrate::SubstrateError> {
            Ok(())
        }

        fn create_context(
            &mut self,
            _m: bloomery_substrate::ModelHandle,
            _n_ctx: u32,
        ) -> Result<bloomery_substrate::CtxHandle, bloomery_substrate::SubstrateError> {
            Ok(1)
        }

        fn destroy_context(
            &mut self,
            _c: bloomery_substrate::CtxHandle,
        ) -> Result<(), bloomery_substrate::SubstrateError> {
            Ok(())
        }

        fn infer(
            &mut self,
            _c: bloomery_substrate::CtxHandle,
            _prompt: &str,
            _max_tokens: u32,
            _stop: Option<&str>,
        ) -> Result<bloomery_substrate::Reply, bloomery_substrate::SubstrateError> {
            panic!("scripted panic: proves catch_unwind keeps the registry and pager healthy");
        }

        fn save_state(
            &mut self,
            _c: bloomery_substrate::CtxHandle,
        ) -> Result<Vec<u8>, bloomery_substrate::SubstrateError> {
            Ok(Vec::new())
        }

        fn load_state(
            &mut self,
            _c: bloomery_substrate::CtxHandle,
            _bytes: &[u8],
        ) -> Result<(), bloomery_substrate::SubstrateError> {
            Ok(())
        }
    }

    /// The regression this fix closes, stated as the two properties the
    /// review named: (1) a panicking worker still reaches a terminal
    /// `TaskStatus::Error` — `get` never reports `Running` forever, so a
    /// poller has a bounded wait, not an infinite one; and (2) the shared
    /// pager `Mutex` is NOT left poisoned — a completely ordinary
    /// subsequent lock-and-use of the *same* `Arc<Mutex<Pager<_>>>>`
    /// still succeeds, proving `catch_unwind` let the `MutexGuard` drop
    /// the normal way rather than via unwind.
    #[test]
    fn a_panicking_worker_becomes_error_not_stuck_running_and_does_not_poison_the_pager() {
        let dir = fresh_dir("panic");
        let journal = Journal::open(&dir.join("pager.jsonl")).unwrap();
        let images = ImageStore::new(&dir.join("img")).unwrap();
        let mut pager = Pager::new(
            PanicSubstrate,
            journal,
            images,
            Box::new(|| Some(1024 * 1024 * 1024)),
        );
        let gguf = dir.join("panic.gguf");
        std::fs::write(&gguf, b"weights").unwrap();
        pager
            .register_model("panic-model", &gguf, meta(), None)
            .unwrap();
        let info = pager
            .create_agent("panic-model", 100, None, 1_000_000)
            .unwrap();
        let agent_id = info.id;

        let pager = Arc::new(Mutex::new(pager));
        let registry = TaskRegistry::new();
        let spec = TaskSpec {
            goal: "trigger a panic".to_string(),
            grant: ok_grant(&dir),
            budget_tokens: 1_000_000,
            max_steps: 3,
            cwd: std::fs::canonicalize(&dir).unwrap(),
            patch_codec: PatchCodec::SearchReplace,
            bounds: ExecBounds::default(),
            mutating_verbs: true,
            envelope: EnvelopeLens::V1,
            memory_block: None,
            window_ladder: false,
        };

        let task_id = registry.spawn_task(
            Arc::clone(&pager),
            agent_id,
            spec,
            dir.join("tasks.jsonl"),
            None,
        );

        // Property 1: bounded wait to a terminal state, never a stuck
        // `Running`.
        let mut entry = registry.get(&task_id).expect("entry exists immediately");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while entry.status == TaskStatus::Running && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(10));
            entry = registry.get(&task_id).expect("entry still exists");
        }
        assert_eq!(entry.status, TaskStatus::Error, "{entry:?}");
        assert!(
            entry
                .summary
                .as_deref()
                .unwrap_or_default()
                .contains("panicked"),
            "{entry:?}"
        );

        // Property 2: the pager mutex is not poisoned. `.lock()` returning
        // `Err` here would mean the catch_unwind failed to stop the unwind
        // before the guard's `Drop`.
        let lock_result = pager.lock();
        assert!(
            lock_result.is_ok(),
            "pager mutex was poisoned by a caught worker panic"
        );
        // And it is not just lockable but usable — an ordinary read-only
        // call against the same pager still succeeds.
        let p = lock_result.unwrap();
        let _ = p.status();
    }
}
