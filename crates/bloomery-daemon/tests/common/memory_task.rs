//! Fixtures specific to the `memory_task_*` tests.
//!
//! Split out on 2026-09-01 (carried-debt slice D). Helpers shared with the
//! refalsify tests live in `tests/common/memory.rs`.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use super::memory::{poll_to_terminal, scripted, BEFORE};
use bloomery_core::grant::Grant;
use bloomery_core::journal::Event;
use bloomery_daemon::memory::MemoryContext;
use bloomery_daemon::pager::Pager;
use bloomery_daemon::task::{TaskRegistry, TaskResult, TaskSpec};
use bloomery_substrate::fake::FakeSubstrate;
use bloomery_substrate::Reply;

/// The four turns that clear the mint bar: touch `a.py`, land a patch, run a
/// granted command that exits 0 afterward, finish `Done` (spec §2).
pub fn fixing_turns() -> Vec<Reply> {
    vec![
        scripted("<action verb=\"read\" path=\"a.py\">\n</action>"),
        scripted("<action verb=\"patch\" path=\"a.py\">\nx = 2\n</action>"),
        scripted("<action verb=\"run\">\n[\"python3\", \"-c\", \"pass\"]\n</action>"),
        scripted("<action verb=\"done\">\nfixed\n</action>"),
    ]
}

/// A canonical sandbox under `dir` holding the planted `a.py`, plus a grant
/// scoped to it that also grants the `["python3","-c"]` command prefix.
pub fn sandbox(dir: &Path) -> (PathBuf, Grant) {
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

/// Spawns one task and polls to a terminal status — the deadline loop from
/// `task/registry.rs`'s own tests, with a longer bound because these tasks
/// really spawn `python3`.
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
    let entry = poll_to_terminal(registry, &task_id);
    (task_id, entry)
}

/// `(mode, episode_id, candidates_checked)` for the one `MemoryStamp` row
/// naming `task_id` — and it must be exactly one: spec §4 stamps every
/// spawned task once, so a duplicate is as much a bug as a missing row.
pub fn stamp_for(events: &[Event], task_id: &str) -> (String, Option<String>, u32) {
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
