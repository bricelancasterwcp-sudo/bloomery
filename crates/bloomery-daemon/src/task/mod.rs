//! The coding-agent task executors (Phase 2b/2c P3).
//!
//! This tree turns a validated `bloomery_core::action::Action` into an
//! [`Observation`] fed back to the model, enforcing the capability
//! boundary P2's `bloomery_core::grant::Grant` built. Task 1 (this file
//! plus [`exec`]) owns the `read`/`find` executors and the two binding
//! security obligations every later executor inherits — see [`exec`]'s
//! module docs for the full statement. Task 2 adds `exec_patch` (plus the
//! Python landing lens), Task 3 adds `exec_run`, Task 4 wires all four into
//! the propose→validate→execute loop, and Task 5 exposes it over HTTP
//! behind `tasks_enabled` (default `false`).

pub mod exec;
pub mod lens_py;

pub use exec::{exec_find, exec_patch, exec_read};

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
#[derive(Debug, Clone, PartialEq)]
pub struct Observation {
    pub outcome: String,
    pub content: String,
    pub failed: bool,
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
