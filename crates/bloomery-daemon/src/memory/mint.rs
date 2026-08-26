//! The mint bar and episode construction (memory-organ Task 5; spec
//! `docs/superpowers/specs/2026-08-26-memory-organ-design.md` §2): turns one
//! task's own `TaskStepRecord`s and captured evidence
//! (`crate::task::TaskResult`, Task 3's capture seam) into a storable
//! [`EpisodeRecord`] — or honestly refuses to, when the evidence does not
//! clear the bar.
//!
//! Two refusals, each load-bearing:
//!
//! - [`verifying_run`] is the mint bar itself (spec §2: the task ended
//!   `Done` AND landed at least one successful `patch` AND a granted `run`
//!   command exited 0 after the last successful `patch`), computed purely
//!   from `result.steps`'s own Vec order — never from the numeric
//!   `step: u32` field, which can repeat across a re-ask
//!   (`task_loop::propose_action`'s multi-attempt journaling records one
//!   entry per parse attempt, all under the same outer step number, before
//!   the eventually-executed action's own entry).
//! - [`build_episode`] additionally refuses over any `PreTouch::Uncomputable`
//!   in `result.touched_files` — deliberately conservative, never
//!   downgraded to a guessed hash. A truncated `read` pins `Uncomputable`
//!   even when a LATER `patch` of the same file had the complete pre-bytes
//!   in hand (`ExecBounds::read_cap_bytes` is 256 KiB, well under the patch
//!   apply path's own cap — `crate::task::PreTouch`'s own doc comment): the
//!   honest fingerprint of "what stood before the task's first touch"
//!   simply does not exist for that file, no matter what a later step
//!   happened to see in full. Spec §2's honesty rule ("nothing in the
//!   record is model prose") extends here to "nothing in the record is a
//!   guess" — it wins over recovering the mint.

use bloomery_core::action::PatchBody;

use crate::task::{PreTouch, TaskResult, TaskStatus, TaskStepRecord};

use super::record::{
    episode_id, goal_hash, CitedFile, EpisodeRecord, Fingerprint, RunEvidence, StoredPatch,
};

/// The mint bar (spec §2). `None` unless every one of these holds:
///
/// 1. `result.status == TaskStatus::Done`.
/// 2. At least one step with `verb == "patch" && !failed` exists.
/// 3. Strictly after that LAST such patch step (by position in
///    `result.steps`, not by the `step` field — see this module's docs), a
///    step with `verb == "run" && !failed && outcome.ends_with(" exit 0")`
///    exists.
///
/// The ` exit 0` suffix is the ONLY exit-code evidence available: `run`'s
/// executor (`crate::task::exec_run::exec_run`, the `Ok(status) =>` arm)
/// reports `failed: false` for a completed run at ANY exit code — zero or
/// not — and reserves `failed: true` for a grant violation, a spawn
/// failure, or a timeout (a command that never finished at all). So
/// `!failed` alone proves the run *completed*, never that it *succeeded*;
/// only the pinned `"ran {program} exit {code}"` outcome string
/// (`exec_run.rs`) carries the exit code, and matching its `" exit 0"`
/// suffix is the only way to read it back out.
///
/// Returns the qualifying run step itself (the first one found after the
/// last successful patch, in step order) — the caller's evidence for
/// `run_evidence`.
pub fn verifying_run(result: &TaskResult) -> Option<&TaskStepRecord> {
    if result.status != TaskStatus::Done {
        return None;
    }

    let last_successful_patch = result
        .steps
        .iter()
        .rposition(|s| s.verb == "patch" && !s.failed)?;

    result.steps[last_successful_patch + 1..]
        .iter()
        .find(|s| s.verb == "run" && !s.failed && s.outcome.ends_with(" exit 0"))
}

/// Everything [`build_episode`] needs beyond the task's own evidence: the
/// identity and provenance fields spec §2 stores but that a `TaskResult`
/// itself has no reason to carry (the goal belongs to the `TaskSpec` that
/// started the task; the model/envelope/mint-clock belong to whoever calls
/// the mint step, not to the loop that produced the evidence).
pub struct MintInputs<'a> {
    pub goal: &'a str,
    pub model: &'a str,
    pub envelope: &'a str,
    pub minted_at: u64,
}

/// Builds the stored [`EpisodeRecord`] for one task, or refuses.
///
/// `None` when [`verifying_run`] is `None` (the mint bar is not clear), OR
/// when any `result.touched_files` value is [`PreTouch::Uncomputable`] (see
/// this module's docs). Otherwise:
///
/// - `cited_files` — every `touched_files` entry, in the `BTreeMap`'s own
///   path-sorted order (spec §2's `episode_id` hash requires cited files
///   sorted by path first; a `BTreeMap<String, _>` iterates that order for
///   free).
/// - `landed_patches` — `result.landed_patches` rendered into
///   [`StoredPatch`]: `PatchBody::WholeFile` stores `contents` verbatim
///   under `codec: "whole_file"`; `PatchBody::SearchReplace` renders under
///   `codec: "search_replace"` into the exact conflict-marker wire form
///   `bloomery_core::action::patch` parses (`patch.rs:17` and that module's
///   own doc comment) — `PatchBody` derives `Serialize` but not
///   `Deserialize` (`bloomery-core/src/action/mod.rs`), which is exactly
///   why this renders into `StoredPatch`'s own string fields rather than
///   storing the codec type itself.
/// - `run_evidence` — `argv`/`outcome` from the verifying run step.
/// - `trajectory` — every step's `verb`, in step order (operator display
///   only, spec §2).
/// - `goal_hash`/`goal_text`/`episode_id` — via `super::record`'s Task 1
///   functions, computed from `inputs.goal` and the built `cited_files`.
/// - `status: "verified"`, `contradicted_by: None` — a freshly minted
///   episode has no contradiction yet (spec §5 assigns that later, on a
///   failed repeat).
pub fn build_episode(result: &TaskResult, inputs: &MintInputs<'_>) -> Option<EpisodeRecord> {
    let run_step = verifying_run(result)?;

    let mut cited_files = Vec::with_capacity(result.touched_files.len());
    for (path, pre) in &result.touched_files {
        let fingerprint = match pre {
            PreTouch::Sha256(hex) => Fingerprint::Sha256(hex.clone()),
            PreTouch::Absent => Fingerprint::Absent,
            // Deliberately conservative refusal — see this module's docs.
            PreTouch::Uncomputable => return None,
        };
        cited_files.push(CitedFile {
            path: path.clone(),
            fingerprint,
        });
    }

    let landed_patches = result
        .landed_patches
        .iter()
        .map(|(path, body)| render_patch(path, body))
        .collect();

    let run_evidence = RunEvidence {
        argv: run_step.args.clone(),
        outcome: run_step.outcome.clone(),
    };

    let trajectory = result.steps.iter().map(|s| s.verb.clone()).collect();

    let hash = goal_hash(inputs.goal);
    let id = episode_id(&hash, &cited_files);

    Some(EpisodeRecord {
        episode_id: id,
        goal_hash: hash,
        goal_text: inputs.goal.to_string(),
        cited_files,
        landed_patches,
        run_evidence,
        trajectory,
        minted_by_model: inputs.model.to_string(),
        minted_by_envelope: inputs.envelope.to_string(),
        status: "verified".to_string(),
        contradicted_by: None,
        minted_at: inputs.minted_at,
    })
}

/// Renders one landed patch into its stored wire form — see
/// [`build_episode`]'s doc comment for the codec/grammar citation.
fn render_patch(path: &str, body: &PatchBody) -> StoredPatch {
    match body {
        PatchBody::WholeFile { contents } => StoredPatch {
            path: path.to_string(),
            codec: "whole_file".to_string(),
            body: contents.clone(),
        },
        PatchBody::SearchReplace { search, replace } => StoredPatch {
            path: path.to_string(),
            codec: "search_replace".to_string(),
            body: format!("<<<<<<< SEARCH\n{search}\n=======\n{replace}\n>>>>>>> REPLACE"),
        },
    }
}
