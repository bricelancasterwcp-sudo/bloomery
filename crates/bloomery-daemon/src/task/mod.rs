//! The coding-agent task executors (Phase 2b/2c P3).
//!
//! This tree turns a validated `bloomery_core::action::Action` into an
//! [`Observation`] fed back to the model, enforcing the capability
//! boundary P2's `bloomery_core::grant::Grant` built. Task 1 (this file
//! plus [`exec`]) owns the `read`/`find` executors and the two binding
//! security obligations every later executor inherits — see [`exec`]'s
//! module docs for the full statement. Task 2 adds `exec_patch` (plus the
//! Python landing lens), Task 3 adds `exec_run` (split into its own
//! [`exec_run`] module — see that module's docs for why), Task 4 wires all
//! four into the propose→validate→execute loop, and Task 5 exposes it over
//! HTTP behind `tasks_enabled` (default `false`).

pub mod exec;
pub mod exec_run;
pub mod grant_line;
pub mod lens_py;
pub mod registry;
mod run_capture;
pub mod task_loop;

pub use exec::{exec_find, exec_patch, exec_read};
pub use exec_run::exec_run;
pub use grant_line::grant_line;
pub use registry::TaskRegistry;
pub use task_loop::{run_task, TaskResult, TaskSpec, TaskStatus, TaskStepRecord};

/// A file's content fingerprint as it stood *immediately before* the task
/// first touched it — the memory organ's citation unit (memory-organ design
/// spec `docs/superpowers/specs/2026-08-26-memory-organ-design.md` §2: each
/// cited file carries "the sha256 of its bytes **before the task's first
/// touch** of that path, captured during execution").
///
/// Three variants, not two, because there are three honestly distinct
/// states and collapsing any pair would make a stored episode lie:
///
/// - `Sha256` — the executor held the file's *complete* bytes and hashed
///   them. The only variant a retrieval fingerprint gate can compare
///   against.
/// - `Absent` — the file did not exist when the task first touched it (an
///   `exec_patch` whose pre-read returned `NotFound`, i.e. the task created
///   it). Spec §2's "distinguished fingerprint `absent`". Distinct from
///   `Sha256` of the empty string: "no file" and "an empty file" are
///   different prior states, and a repeat that finds one where the episode
///   recorded the other is not the same situation.
/// - `Uncomputable` — the file was touched, but its full pre-touch bytes
///   were never in hand, so no honest whole-file hash exists. Produced by a
///   read truncated at `ExecBounds::read_cap_bytes`: the executor saw a
///   prefix, and hashing a prefix as if it were the file is exactly the
///   silent-truncation lie this crate's caps exist to prevent. Task 5's
///   mint bar refuses to mint an episode over one of these rather than
///   storing a fingerprint that can never legitimately match.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub enum PreTouch {
    Sha256(String),
    Absent,
    Uncomputable,
}

/// One successful `read`/`patch` step's capture: *which* file it touched,
/// and that file's [`PreTouch`] fingerprint.
///
/// `canonical` is the path the `Grant` check *returned* — never the model's
/// raw target string — for the same reason every open in [`exec`] uses it
/// (see that module's obligation 1): it is the single resolved identity of
/// the file the executor actually operated on, so two spellings of one path
/// cite one file rather than two.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Touched {
    pub canonical: std::path::PathBuf,
    pub pre: PreTouch,
}

/// The result of executing one action: what to feed back to the model, and
/// a short outcome tag for the `TaskStep` journal entry.
///
/// `outcome` is the short, single-line tag journaled per step (e.g. `"read
/// 412 bytes"`, `"grant violation: ..."`, `"found 3 matches"`); `content`
/// is the (possibly longer, possibly windowed/truncated) text appended to
/// the model's transcript as the observation of that step. `failed` is
/// `true` exactly when the step did not achieve its verb — a grant
/// violation or an execution failure — never for a verb that ran cleanly
/// but simply found nothing (e.g. a `find` with zero matches is not a
/// failure of `find`).
///
/// `touched` is the memory organ's capture seam (spec §2): `Some` exactly
/// when this step *successfully* `read` or `patch`ed one file, carrying
/// that file's canonical path and pre-first-touch fingerprint. It is
/// deliberately part of the `Observation` rather than recomputed later:
/// the fingerprint must be of the bytes the executor already held, under
/// the grant check and `O_NOFOLLOW` open that authorized reading them —
/// re-opening a model-supplied path a second time to hash it would both
/// re-open the TOCTOU window [`exec`]'s obligations exist to narrow and
/// hash the *wrong* (post-patch) bytes. Every other construction site —
/// `exec_find`, `exec_run`, and the shared `failed()` helper — sets `None`:
/// a step that did not touch a file's bytes has nothing to cite.
#[derive(Debug, Clone, PartialEq)]
pub struct Observation {
    pub outcome: String,
    pub content: String,
    pub failed: bool,
    pub touched: Option<Touched>,
}

/// The bounds an executor enforces. Sourced from `Config` (Task 5 wires
/// operator-configurable values in); the numbers in each field's doc are
/// the shipped defaults, not compiled-in constants here — this struct is
/// just the carrier.
#[derive(Debug, Clone, Copy)]
pub struct ExecBounds {
    /// Max bytes a single `read` action returns. Default 256 KiB.
    pub read_cap_bytes: usize,
    /// Max matches a single `find` action returns. Default 100.
    pub find_result_cap: usize,
    /// Max bytes a single `run` action's captured output returns. Default
    /// 64 KiB. Unused by Task 1's executors; carried here so `ExecBounds`
    /// is the one bounds type every executor (including Tasks 2–3's) reads
    /// through, rather than each task growing its own bounds struct.
    pub run_output_cap_bytes: usize,
    /// Max wall-clock seconds a `run` action's subprocess gets. Default
    /// 120. Unused by Task 1's executors; see `run_output_cap_bytes`.
    pub run_timeout_secs: u64,
}

/// The shipped defaults named in each field's own doc comment above —
/// `config.rs`'s `default_read_cap_bytes`/`default_find_result_cap`/
/// `default_run_output_cap_bytes`/`default_run_timeout_secs` serde defaults
/// mirror these exact numbers, so a config that omits every one of Task 5's
/// four exec-bound keys ends up here either way. Kept as one `Default` impl
/// (rather than four repeated literals in `config.rs`, `test_support.rs`,
/// and `Pager::new`) so the numbers live in exactly one place.
impl Default for ExecBounds {
    fn default() -> Self {
        ExecBounds {
            read_cap_bytes: 256 * 1024,
            find_result_cap: 100,
            run_output_cap_bytes: 64 * 1024,
            run_timeout_secs: 120,
        }
    }
}
