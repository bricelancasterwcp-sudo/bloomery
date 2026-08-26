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
//!   the eventually-executed action's own entry). **Amendment (code review
//!   finding, 2026-08-26):** when more than one run completes in the window
//!   after the last successful patch, the bar reads the LAST completed run
//!   in that window, not the first passing one — see [`verifying_run`]'s own
//!   doc comment for the honesty rationale.
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
/// 3. Within the window strictly after that LAST such patch step (by
///    position in `result.steps`, not by the `step` field — see this
///    module's docs), the LAST COMPLETED run — `verb == "run" && !failed`,
///    a run that finished, not necessarily one that passed — has an
///    `outcome` ending in `" exit 0"`.
///
/// **Last completed run wins, not first passing run** (amendment; code
/// review finding, 2026-08-26). Spec §2's own phrasing ("a granted `run`
/// command exited 0 after the last successful `patch`") is existential —
/// it does not say WHICH run, when several ran in the window. Taking the
/// first passing run in that window was wrong: `patch → run exit 0 → run
/// exit 1 → done` would mint citing a stale pass while the trajectory's own
/// final, completed evidence is a failure — record dishonesty exactly the
/// kind spec §2's "nothing in the record is model prose" line (extended in
/// [`build_episode`]'s docs to "nothing in the record is a guess") exists
/// to forbid. The ruling reads spec §2 strictly: the record must reflect
/// the task's own FINAL verifying evidence in the window, so this searches
/// backward from the end of the window for the last run that *completed*,
/// and mints only if THAT run's outcome ends `" exit 0"`. Consequences:
/// an earlier pass in the same window does not rescue a later completed
/// failure (no mint); a later completed pass DOES rescue an earlier
/// completed failure (mint, citing the later pass); and a later
/// STRUCTURALLY failed run (timeout, spawn failure, grant violation —
/// `failed: true`, meaning it never actually completed) does not veto an
/// earlier completed pass, because a run that never finished carries no
/// evidence about what the command would have reported — see the next
/// paragraph for why `exec_run` makes that distinction exact.
///
/// `!failed` distinguishes a run that *completed* (`exec_run`'s
/// `Ok(status) =>` arm, `exec_run.rs`) from one that never did — a grant
/// violation, a spawn failure, or a timeout (`Err(RunFailure::..)` arms) —
/// which `exec_run` always marks `failed: true` and which never carries a
/// real exit code in its `outcome` string. So a `failed: true` step is
/// invisible to this search entirely: it neither verifies (it isn't a
/// completion) nor vetoes (it says nothing about whether an earlier
/// completed run in the same window actually passed).
///
/// The ` exit 0` suffix remains the only exit-code evidence a completed run
/// carries: `exec_run`'s pinned success-arm format string is
/// `"ran {program} exit {code}"` (`exec_run.rs`, the `Ok(status) =>` arm),
/// and `failed: false` is reported for EVERY exit code, zero or not — so
/// `!failed` alone proves a run completed, never that it exited zero; only
/// matching the outcome's own `" exit 0"` suffix reads the exit code back
/// out.
///
/// Returns the qualifying run step itself — the last completed run in the
/// window, when its outcome passes — the caller's evidence for
/// `run_evidence`.
pub fn verifying_run(result: &TaskResult) -> Option<&TaskStepRecord> {
    if result.status != TaskStatus::Done {
        return None;
    }

    let last_successful_patch = result
        .steps
        .iter()
        .rposition(|s| s.verb == "patch" && !s.failed)?;

    let window = &result.steps[last_successful_patch + 1..];
    let last_completed_run = window.iter().rposition(|s| s.verb == "run" && !s.failed)?;
    let candidate = &window[last_completed_run];

    candidate.outcome.ends_with(" exit 0").then_some(candidate)
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
