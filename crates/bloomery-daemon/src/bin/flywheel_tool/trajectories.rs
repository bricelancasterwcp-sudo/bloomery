//! The four per-verb trajectory builders, and the real filesystem read that
//! keeps the refusal pair honest.
//!
//! Split out of `flywheel_tool.rs` on 2026-09-01 (carried-debt slice D),
//! following the same precedent `render.rs` and `scratch.rs` already set --
//! and for the same reason those did: the file had grown back past the repo's
//! 800-line ceiling. Each builder turns one request into the transcript a
//! factory will train on, so what lives here is the wire contract's payload
//! half; `flywheel_tool.rs` keeps the types, the parsers and the dispatcher.

use bloomery_core::action::lens::{land, Landing};
use bloomery_core::action::{PatchBody, PatchCodec};
use bloomery_daemon::config::EnvelopeLens;
use bloomery_daemon::task::task_loop::{render_task_prompt, transcript_entry};
use bloomery_daemon::task::{exec_find, exec_read, exec_run, ExecBounds};

use crate::render::{
    describe_landing_failure, done_completion, find_completion, land_reference_patch, lens_for,
    patch_completion, read_completion, run_completion, Pair, Trajectory, FIND_PATH,
};
use crate::scratch::{
    files_to_materialize, real_target_read, run_exit_code, safe_relative, Scratch, ScratchId,
};
use crate::{require, TrajectoryRequest, TrajectoryResponse, VERIFIED_REFUSAL};

/// Handles a find-shaped (`find_pattern` set) patch trajectory: 4 pairs,
/// `find` -> `read` -> `patch` -> `done`, over a scratch dir materialized
/// from the request's `files`.
///
/// Both of the first two observations are REAL executor output — the whole
/// point of the shape. Two conditions are hard errors rather than rendered
/// data, because each means the *ideal itself* is broken, which is always a
/// factory bug and never something to train on:
///
/// - a `find` that matches nothing (the trajectory's opening move would
///   teach the model to search and find nothing, then read the right file
///   anyway — a step that carries no information);
/// - a `target` that is not among `files` (there is nothing to read, so
///   there is no trajectory).
///
/// A reference patch that does not land is NOT one of them: that answers
/// with the pairs built so far plus `landed: false` and `landing_detail`,
/// exactly as patch mode does.
pub(crate) fn handle_find_trajectory(
    req: &TrajectoryRequest,
    codec: PatchCodec,
    envelope: EnvelopeLens,
    pattern: &str,
) -> Result<TrajectoryResponse, String> {
    let search = require(&req.search, "search", "patch")?;
    let replace = require(&req.replace, "replace", "patch")?;
    let summary = require(&req.summary, "summary", "patch")?;

    let files = files_to_materialize(req);
    let scratch = Scratch::materialize(&ScratchId {
        target: &req.target,
        target_contents: &req.target_contents,
        find_pattern: Some(pattern),
        files: &files,
    })?;
    let grant = scratch.grant(&req.commands)?;

    // `exec_find` takes no cwd: a relative prefix would silently fall back
    // to this process's own cwd, so the scratch dir is passed absolute —
    // the same absolutize-first obligation `task_loop.rs`'s dispatch
    // discharges before its own `exec_find` call.
    let find = exec_find(
        &grant,
        pattern,
        &scratch.path().to_string_lossy(),
        &ExecBounds::default(),
    );
    if find.failed {
        return Err(format!("the find step did not run: {}", find.outcome));
    }
    if find.content.is_empty() {
        return Err(format!(
            "find_pattern {pattern:?} found 0 matches across the request's \"files\" — a \
             find-shaped ideal whose opening find finds nothing is not an ideal; this is a \
             factory bug, not a tool bug"
        ));
    }
    let read = real_target_read(&scratch, &grant, req)?;

    let mut trajectory = Trajectory::new(&req.goal, codec, envelope, &req.commands);
    trajectory.emit(find_completion(pattern, FIND_PATH));
    trajectory.observe("find", &find.outcome, &find.content);
    trajectory.emit(read_completion(&req.target));
    trajectory.observe("read", &read.outcome, &read.content);
    trajectory.emit(patch_completion(&req.target, search, replace));

    let (new_contents, lens_name) =
        match land_reference_patch(&read.content, &req.target, search, replace) {
            Ok(landed) => landed,
            Err(detail) => {
                return Ok(TrajectoryResponse {
                    pairs: trajectory.pairs,
                    landed: false,
                    patched_contents: None,
                    landing_detail: Some(detail),
                    verified: None,
                })
            }
        };
    trajectory.observe(
        "patch",
        &format!("patched (lens: {lens_name})"),
        &new_contents,
    );
    trajectory.emit(done_completion(summary, envelope)?);

    Ok(TrajectoryResponse {
        pairs: trajectory.pairs,
        landed: true,
        patched_contents: Some(new_contents),
        landing_detail: None,
        verified: None,
    })
}

/// Handles a run-verified (`run_argv` set) patch trajectory: 4 pairs,
/// `read` -> `patch` -> `run` -> `done`, with the run executed for real
/// against the PATCHED file.
///
/// The order matters and is the whole content of the shape: the reference
/// patch is landed in memory, **written to the scratch copy of `target`**,
/// and only then is `run_argv` executed there under a grant carrying the
/// request's `commands`. A trajectory that ran the verification against the
/// unpatched file would be a trajectory whose final `done` is a lie.
///
/// **A run that does not exit 0 is a hard error response**, never a
/// rendered trajectory: an ideal whose own verification fails is not an
/// ideal, and the factory's contract is to abort that task as structural
/// rather than train on it. So is a run that never ran at all (a grant
/// violation, a spawn failure, a timeout — `exec_run`'s `failed: true`
/// cases, which are exactly the ones where the verb was not carried out).
pub(crate) fn handle_run_trajectory(
    req: &TrajectoryRequest,
    codec: PatchCodec,
    envelope: EnvelopeLens,
    argv: &[String],
) -> Result<TrajectoryResponse, String> {
    let search = require(&req.search, "search", "patch")?;
    let replace = require(&req.replace, "replace", "patch")?;
    let summary = require(&req.summary, "summary", "patch")?;

    let files = files_to_materialize(req);
    let scratch = Scratch::materialize(&ScratchId {
        target: &req.target,
        target_contents: &req.target_contents,
        find_pattern: None,
        files: &files,
    })?;
    let grant = scratch.grant(&req.commands)?;
    let read = real_target_read(&scratch, &grant, req)?;

    let mut trajectory = Trajectory::new(&req.goal, codec, envelope, &req.commands);
    trajectory.emit(read_completion(&req.target));
    trajectory.observe("read", &read.outcome, &read.content);
    trajectory.emit(patch_completion(&req.target, search, replace));

    let (new_contents, lens_name) =
        match land_reference_patch(&read.content, &req.target, search, replace) {
            Ok(landed) => landed,
            Err(detail) => {
                return Ok(TrajectoryResponse {
                    pairs: trajectory.pairs,
                    landed: false,
                    patched_contents: None,
                    landing_detail: Some(detail),
                    verified: None,
                })
            }
        };
    trajectory.observe(
        "patch",
        &format!("patched (lens: {lens_name})"),
        &new_contents,
    );
    trajectory.emit(run_completion(argv));

    let target_path = scratch.path().join(safe_relative(&req.target)?);
    std::fs::write(&target_path, &new_contents).map_err(|e| {
        format!("failed to write the patched {target_path:?} before verifying: {e}")
    })?;

    let run = exec_run(&grant, scratch.path(), argv, &ExecBounds::default());
    if run.failed {
        return Err(format!(
            "the run verification {argv:?} never ran: {} — a run-verified ideal must be able to \
             execute its own verification (check the request's \"commands\" grant)",
            run.outcome
        ));
    }
    let code = run_exit_code(&run)?;
    if code != 0 {
        return Err(format!(
            "the run verification {argv:?} did not pass against the patched {t:?} — an ideal \
             whose own verification fails is not an ideal, so no trajectory is rendered. The \
             real observation was: {o} / {c:?}",
            t = req.target,
            o = run.outcome,
            c = run.content
        ));
    }
    trajectory.observe("run", &run.outcome, &run.content);
    trajectory.emit(done_completion(summary, envelope)?);

    Ok(TrajectoryResponse {
        pairs: trajectory.pairs,
        landed: true,
        patched_contents: Some(new_contents),
        landing_detail: None,
        verified: None,
    })
}

/// Handles a patch-mode (`expect` absent or `"patch"`) `trajectory`
/// request end to end — task-1 brief, unchanged behavior: renders the
/// three prompts via the real [`render_task_prompt`]/[`transcript_entry`],
/// and land-verifies the reference patch via the real [`land`]. Pair
/// construction is pinned by the task-1 brief:
///
/// - Pair 1's prompt = `render_task_prompt(goal, codec, envelope, "")`.
/// - Pair 2's transcript = `transcript_entry(1, "read", "read {n} bytes",
///   target_contents)` — byte-parity with `exec_read`'s uncapped outcome
///   string (`task/exec.rs:~176`).
/// - Pair 3's transcript = pair 2's transcript + `transcript_entry(2,
///   "patch", "patched (lens: {lens_name})", patched_contents)`, where
///   `patched_contents`/`lens_name` come straight out of the real `land()`
///   call — never computed independently.
///
/// When the reference patch does not land, pair 3 cannot be built at all
/// (there is no verified `patched_contents` to render a transcript or a
/// `done` completion around), so the response carries only pairs 1-2,
/// `landed: false`, and `landing_detail` — the factory's contract (task-1
/// brief) is that this is always a fatal factory bug, never data to train
/// on.
pub(crate) fn handle_patch_trajectory(
    req: &TrajectoryRequest,
    codec: PatchCodec,
    envelope: EnvelopeLens,
) -> Result<TrajectoryResponse, String> {
    let search = require(&req.search, "search", "patch")?;
    let replace = require(&req.replace, "replace", "patch")?;
    let summary = require(&req.summary, "summary", "patch")?;

    let prompt1 = render_task_prompt(&req.goal, codec, envelope, &req.commands, "");
    let completion1 = read_completion(&req.target);

    let read_outcome = format!("read {} bytes", req.target_contents.len());
    let transcript_after_read = transcript_entry(1, "read", &read_outcome, &req.target_contents);
    let prompt2 = render_task_prompt(
        &req.goal,
        codec,
        envelope,
        &req.commands,
        &transcript_after_read,
    );
    let completion2 = patch_completion(&req.target, search, replace);

    let pairs_1_2 = || {
        vec![
            Pair {
                prompt: prompt1.clone(),
                completion: completion1.clone(),
            },
            Pair {
                prompt: prompt2.clone(),
                completion: completion2.clone(),
            },
        ]
    };

    let body = PatchBody::SearchReplace {
        search: search.to_string(),
        replace: replace.to_string(),
    };
    let lens = lens_for(&req.target);
    let landing = land(&req.target_contents, &body, lens.as_ref());

    let (new_contents, lens_name) = match &landing {
        Landing::Lands { new_contents, lens } => (new_contents.clone(), *lens),
        other => {
            return Ok(TrajectoryResponse {
                pairs: pairs_1_2(),
                landed: false,
                patched_contents: None,
                landing_detail: Some(describe_landing_failure(other)),
                verified: None,
            });
        }
    };

    let patch_outcome = format!("patched (lens: {lens_name})");
    let transcript_after_patch = format!(
        "{transcript_after_read}{}",
        transcript_entry(2, "patch", &patch_outcome, &new_contents)
    );
    let prompt3 = render_task_prompt(
        &req.goal,
        codec,
        envelope,
        &req.commands,
        &transcript_after_patch,
    );
    let completion3 = done_completion(summary, envelope)?;

    let mut pairs = pairs_1_2();
    pairs.push(Pair {
        prompt: prompt3,
        completion: completion3,
    });

    Ok(TrajectoryResponse {
        pairs,
        landed: true,
        patched_contents: Some(new_contents),
        landing_detail: None,
        verified: None,
    })
}

/// Handles a refuse-mode (`expect = "refuse"`) `trajectory` request
/// end to end (task-3 brief, G5 design doc §5): exactly 2 pairs (`read`,
/// `done`), never a `patch` — `search`/`replace` are absent from the
/// request and no [`land`] call ever happens. Both refusal families share
/// pair 1 (the model attempts the same `read` a repair trajectory would)
/// and completion 2 (`done_completion(refusal_reason, envelope)`); they differ only
/// in how pair 2's transcript is built:
///
/// - **defect-absent** (`target_missing == false`, the default): `target`
///   exists, so pair 2's transcript reuses patch-mode pair 2's exact
///   technique — `transcript_entry(1, "read", "read {n} bytes",
///   target_contents)` — real content the request already carries.
/// - **missing-target** (`target_missing == true`): `target` does not
///   exist, so pair 2's transcript is built from [`real_missing_target_read`],
///   which calls the REAL [`exec_read`] against a throwaway scratch dir
///   that lacks `target` — never a hand-formatted "not found" string.
///
/// `landed: true` unconditionally (self-consistency only — no landing
/// check applies to a trajectory that never patches anything) and
/// `verified: Some(VERIFIED_REFUSAL)`, so a factory reading the response
/// can assert it exercised the refuse path rather than reading a vacuous
/// `landed: true` as a patch success.
pub(crate) fn handle_refuse_trajectory(
    req: &TrajectoryRequest,
    codec: PatchCodec,
    envelope: EnvelopeLens,
) -> Result<TrajectoryResponse, String> {
    let refusal_reason = require(&req.refusal_reason, "refusal_reason", "refuse")?;

    let prompt1 = render_task_prompt(&req.goal, codec, envelope, &req.commands, "");
    let completion1 = read_completion(&req.target);

    let (read_outcome, read_content) = if req.target_missing {
        real_missing_target_read(&req.target)?
    } else {
        (
            format!("read {} bytes", req.target_contents.len()),
            req.target_contents.clone(),
        )
    };
    let transcript_after_read = transcript_entry(1, "read", &read_outcome, &read_content);
    let prompt2 = render_task_prompt(
        &req.goal,
        codec,
        envelope,
        &req.commands,
        &transcript_after_read,
    );
    let completion2 = done_completion(refusal_reason, envelope)?;

    Ok(TrajectoryResponse {
        pairs: vec![
            Pair {
                prompt: prompt1,
                completion: completion1,
            },
            Pair {
                prompt: prompt2,
                completion: completion2,
            },
        ],
        landed: true,
        patched_contents: None,
        landing_detail: None,
        verified: Some(VERIFIED_REFUSAL.to_string()),
    })
}

/// Runs the REAL [`exec_read`] against a fresh, empty scratch directory
/// that does not contain `target`, returning its `(outcome, content)` —
/// both texts a genuine `failed: true` [`bloomery_daemon::task::Observation`]
/// carries (`exec.rs`'s `failed()` helper: outcome and content are the same
/// string for every failure path in that module). This is this module's
/// whole design applied to the missing-target family (see the top-of-file
/// doc comment): the exact `NotFound` wording is OS/errno-sourced text —
/// e.g. `io::Error`'s `Display` plus its `ErrorKind` Debug — that must be
/// discovered by actually opening a missing file through the real
/// executor, never hand-transcribed into a format string here.
///
/// The scratch directory is unrelated to whatever fixture directory the
/// factory's own missing-target task uses: the wire request carries only
/// `target`'s name, never a whole directory listing, so this function
/// always builds and tears down its own throwaway directory per call —
/// harmless, since `exec_read`'s `NotFound` message text does not depend
/// on the specific path bytes (see `tests/flywheel_tool_test.rs`'s
/// missing-target anti-drift pin, which proves this independently against
/// a real `run_task`).
///
/// Turn 3 moved the directory and the grant onto the shared [`Scratch`]
/// type — the empty-`files` case of the very same materialize-and-tear-down
/// the find/run shapes use. That retires this function's hand-built grant
/// JSON, which interpolated a raw path into a string literal and would have
/// produced invalid JSON for any temp dir whose name contained a quote or a
/// backslash; [`Scratch::grant`] builds the same wire object through
/// `serde_json::json!`, which escapes it.
///
/// Ruling bT7/R1 made the scratch name content-derived, so this path's
/// directory is now named from `target` too. That was never part of the
/// determinism breakage — a failed `exec_read`'s message text carries no
/// path, which is exactly what the missing-target anti-drift pin proves by
/// reconstructing it in a *different* directory — but sharing one naming
/// scheme keeps `Scratch` a single story rather than two.
fn real_missing_target_read(target: &str) -> Result<(String, String), String> {
    let scratch = Scratch::materialize(&ScratchId {
        target,
        target_contents: "",
        find_pattern: None,
        files: &[],
    })?;
    let grant = scratch.grant(&[])?;

    let observation = exec_read(&grant, scratch.path(), target, None, &ExecBounds::default());

    if !observation.failed {
        return Err(format!(
            "target_missing=true but {target:?} was readable inside a fresh, empty scratch \
             dir — this is a factory bug (the target must not already exist), not a tool bug"
        ));
    }
    Ok((observation.outcome, observation.content))
}
