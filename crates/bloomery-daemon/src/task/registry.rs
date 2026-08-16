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

use std::any::Any;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use bloomery_core::journal::Journal;
use bloomery_substrate::Substrate;

use crate::pager::Pager;
use crate::task::{run_task, TaskResult, TaskSpec, TaskStatus};

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
    if let Some(s) = payload.downcast_ref::<&str>() {
        format!("task worker panicked: {s}")
    } else if let Some(s) = payload.downcast_ref::<String>() {
        format!("task worker panicked: {s}")
    } else {
        "task worker panicked (no string message on the payload)".to_string()
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
    pub fn spawn_task<S: Substrate + Send + 'static>(
        &self,
        pager: Arc<Mutex<Pager<S>>>,
        agent_id: String,
        spec: TaskSpec,
        journal_path: PathBuf,
    ) -> String {
        let n = self.next_id.fetch_add(1, Ordering::Relaxed);
        let task_id = format!("task-{n}");

        {
            let mut entries = lock_entries(&self.entries);
            entries.insert(
                task_id.clone(),
                TaskResult {
                    status: TaskStatus::Running,
                    steps: Vec::new(),
                    summary: None,
                },
            );
        }

        let entries = Arc::clone(&self.entries);
        let worker_task_id = task_id.clone();
        std::thread::spawn(move || {
            let mut journal = match Journal::open(&journal_path) {
                Ok(j) => j,
                Err(e) => {
                    let mut entries = lock_entries(&entries);
                    entries.insert(
                        worker_task_id,
                        TaskResult {
                            status: TaskStatus::Error,
                            steps: Vec::new(),
                            summary: Some(format!(
                                "failed to open task journal {}: {e}",
                                journal_path.display()
                            )),
                        },
                    );
                    return;
                }
            };

            // The v1 locking decision, exactly: one lock, held for the
            // whole task — see this module's docs.
            let result = match pager.lock() {
                Ok(mut guard) => {
                    // Catch a panic from `run_task` HERE, inside the locked
                    // scope, so it is caught before it would ever reach
                    // `guard`'s `Drop` — see this module's "Panic
                    // containment" doc section for the full reasoning. A
                    // caught panic lets `guard` drop the ordinary way
                    // (never via unwind), so the mutex is never poisoned.
                    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        run_task(&mut guard, &agent_id, &spec, &mut journal)
                    }));
                    match outcome {
                        Ok(result) => result,
                        Err(payload) => TaskResult {
                            status: TaskStatus::Error,
                            steps: Vec::new(),
                            summary: Some(panic_message(payload.as_ref())),
                        },
                    }
                }
                Err(_) => {
                    // Deliberately NOT `.into_inner()` here: this is the
                    // pager's own mutex, not the registry's bookkeeping
                    // one, and `api_native::lock_pager`'s sticky-poison
                    // reasoning applies in full — a poisoned pager's state
                    // is not vouched for, so this task did not run.
                    TaskResult {
                        status: TaskStatus::Error,
                        steps: Vec::new(),
                        summary: Some(
                            "pager state poisoned by a prior panic; restart the daemon".to_string(),
                        ),
                    }
                }
            };

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
    use crate::task::ExecBounds;
    use bloomery_core::action::PatchCodec;

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
            kv_heads: 2,
            head_dim: 32,
            training_ctx: 65536,
            weights_bytes: 1000,
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
            think_preseed: false,
        };

        let task_id =
            registry.spawn_task(Arc::clone(&pager), agent_id, spec, dir.join("tasks.jsonl"));
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
            think_preseed: false,
        };

        let id1 = registry.spawn_task(
            Arc::clone(&pager),
            agent_id.clone(),
            spec(std::fs::canonicalize(&dir).unwrap(), ok_grant(&dir)),
            dir.join("tasks.jsonl"),
        );
        let id2 = registry.spawn_task(
            Arc::clone(&pager),
            agent_id,
            spec(std::fs::canonicalize(&dir).unwrap(), ok_grant(&dir)),
            dir.join("tasks.jsonl"),
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
            think_preseed: false,
        };

        let task_id =
            registry.spawn_task(Arc::clone(&pager), agent_id, spec, dir.join("tasks.jsonl"));

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
