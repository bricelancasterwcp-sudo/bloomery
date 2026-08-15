//! The Phase 2b/2c P3 task surface's policy fields on [`Pager`]: the
//! `tasks_enabled` gate, the task loop's executor bounds, the task journal
//! path, and the one per-agent lookup Task 5's HTTP surface needs
//! (`agent_budget_granted`).
//!
//! Split out of `pager.rs` itself for the same reason `status.rs` is its
//! own file: every setter here has a doc comment explaining what config key
//! it mirrors, and keeping that beside the field declarations it would
//! otherwise separate from would just make `pager.rs` longer without making
//! either half easier to read.

use std::path::{Path, PathBuf};

use bloomery_substrate::Substrate;

use crate::task::ExecBounds;

impl<S: Substrate> crate::pager::Pager<S> {
    /// The Phase 2b/2c P3 task surface's gate (`config.tasks_enabled`,
    /// default `false`). `main.rs` is the only place that should ever pass
    /// `true` — a permissive default here would leave the task HTTP surface
    /// (Task 5) live on any daemon whose config predates it.
    pub fn set_tasks_enabled(&mut self, enabled: bool) {
        self.tasks_enabled = enabled;
    }

    pub fn tasks_enabled(&self) -> bool {
        self.tasks_enabled
    }

    /// Sets the task loop's executor bounds
    /// (`config.read_cap_bytes`/`find_result_cap`/`run_output_cap_bytes`/
    /// `run_timeout_secs`) — every task created after this call uses them.
    pub fn set_exec_bounds(&mut self, bounds: ExecBounds) {
        self.exec_bounds = bounds;
    }

    pub fn exec_bounds(&self) -> ExecBounds {
        self.exec_bounds
    }

    /// Sets where Task 5's task registry opens its per-task `Journal`
    /// handle (`config.data_dir/journal/tasks.jsonl` in `main.rs`).
    pub fn set_task_journal_path(&mut self, path: PathBuf) {
        self.task_journal_path = path;
    }

    pub fn task_journal_path(&self) -> &Path {
        &self.task_journal_path
    }

    /// The currently granted token budget for a known agent, or `None` if
    /// `id` names no agent. Task 5's task-creation route uses `None` here
    /// as its `404 unknown_agent` check, and `Some` as the default a
    /// request's omitted `budget_tokens` falls back to — `TaskSpec`'s own
    /// doc comment calls this field a mirror of the agent's pager-level
    /// `Budget`.
    pub fn agent_budget_granted(&self, id: &str) -> Option<u64> {
        self.table.get(id).map(|a| a.budget.granted())
    }
}
