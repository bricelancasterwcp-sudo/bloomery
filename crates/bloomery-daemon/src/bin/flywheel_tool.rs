//! `flywheel-tool` — the serving-faithful trajectory renderer/verifier
//! (design spec §2, `docs/superpowers/specs/2026-08-16-flywheel-14b-design.md`):
//! "training artifacts run through the serving code". This binary is the
//! ONLY place the fine-tune flywheel's Python factory (Task 2) touches
//! anything prompt- or landing-shaped — it reads one JSON request per line
//! on stdin, writes one JSON response per line on stdout, and every byte
//! it produces comes from the real daemon internals, never a
//! reimplementation:
//!
//! - Prompts: [`bloomery_daemon::task::task_loop::render_task_prompt`] and
//!   [`bloomery_daemon::task::task_loop::transcript_entry`] — thin `pub`
//!   wrappers task_loop.rs added around its own private
//!   `render_prompt_at_rung` and `record_step` formatting, so this
//!   binary's prompts are the exact bytes a live task would render, not a
//!   second implementation of the envelope.
//! - Landing: [`bloomery_core::action::lens::land`], the real
//!   [`bloomery_core::action::lens::PlainText`] lens, and the real
//!   [`bloomery_daemon::task::lens_py::PythonLens`] — the same applier and
//!   lenses `exec_patch` uses in a live task.
//!
//! **A separate crate, on purpose.** A `[[bin]]` target in the same Cargo
//! package as a library is still its own compiled crate: it can only see
//! the library's `pub` surface, never `pub(crate)` internals. That is
//! exactly the boundary this file is built to respect —
//! `render_prompt_at_rung` and `record_step` themselves stay private; only
//! the two pinned wrappers (and `land`/the lenses, already `pub`) cross into
//! this binary. The one piece of logic this file does duplicate is trivial
//! and stated as such:
//! choosing which lens applies to `target` (`.py` -> [`PythonLens`], else
//! [`PlainText`]) is a one-line extension check, not landing logic itself
//! — the actual apply-and-parse work is 100% delegated to the real
//! `land()` call.
//!
//! No GPU toolchain is required to build or run this binary: it never
//! touches `bloomery_substrate`, `Pager`, or any `llama`-feature code path
//! — `cargo build -p bloomery-daemon --bin flywheel-tool` succeeds with no
//! features enabled, which is the whole point (the factory box need not be
//! the same box the model serves from).
//!
//! **Turn 2 (flywheel task-3 brief,
//! `docs/superpowers/specs/2026-08-16-flywheel2-honest-refusal-design.md`
//! §4-§5) adds refusal trajectories**, additively: a `trajectory` request's
//! `"expect"` field selects `"patch"` (the default, absent case — every
//! turn-1 request byte-identical to today) or `"refuse"`. A refuse request
//! renders exactly 2 pairs (`read`, `done`) instead of patch mode's 3, and
//! carries no `search`/`replace`/`summary` — its `done` completion comes
//! from `refusal_reason` instead. Two refusal families, both still built
//! entirely from real daemon internals, never a reimplementation:
//! - **defect-absent** (the goal's claimed defect is false; `target`
//!   exists): pair 2's transcript reuses the same real-target-contents
//!   technique pair 2 already used in patch mode.
//! - **missing-target** (`"target_missing": true`; `target` does not
//!   exist): pair 2's transcript is built from a REAL [`exec_read`] call
//!   against a throwaway scratch directory that does not contain `target`
//!   — the exact `NotFound` wording is OS/errno-sourced text this binary
//!   discovers by actually calling the real executor, never hand-formats
//!   (see [`real_missing_target_read`]).
//!
//! **Turn 3 (flywheel task-6 brief,
//! `docs/superpowers/specs/2026-08-20-flywheel3-turn3-design.md` §2) adds
//! two more patch-mode trajectory shapes**, again additively — a request
//! carrying none of turn 3's new fields renders byte-identically to turn 1
//! (pinned by `tests/flywheel_tool_test.rs`'s turn-1 golden). Both extend
//! the same rule the refusal families established, to two more executors:
//! every observation in a rendered trajectory comes from a REAL executor
//! call against a throwaway scratch directory this binary materializes from
//! the request's `files`, never from a hand-written format string here.
//!
//! - **find-shaped** (`"find_pattern"` set): 4 pairs, `find` -> `read` ->
//!   `patch` -> `done`, the first two observations from the real
//!   [`exec_find`] and [`exec_read`] (see [`handle_find_trajectory`]).
//! - **run-verified** (`"run_argv"` set): 4 pairs, `read` -> `patch` ->
//!   `run` -> `done`, the run executed for real by [`exec_run`] against the
//!   PATCHED file under a grant carrying the request's `commands` (see
//!   [`handle_run_trajectory`]). **A non-zero exit is a hard error
//!   response, never a rendered trajectory**: an ideal whose own
//!   verification fails is not an ideal, and the factory must abort that
//!   task as structural rather than train on it.
//!
//! **Turn 4 (turn-4 spec §2) adds envelope-v4**, and this binary's only part
//! in it is to keep being faithful: `"envelope": "v4"` parses here too, and
//! every prompt is rendered by passing the request's own `commands` to
//! `render_task_prompt`, which is what the grant line is built from — so a
//! rendered trajectory states exactly the capability its real executor calls
//! ran under.
//!
//! **Turn 7 (turn-7 spec §2.3) makes the `done` completion envelope-aware**:
//! under a lens where `done_declares()` is false (v1–v4) nothing changes —
//! the pinned `<action verb="done">` wrap, byte-identical. Under a
//! `done_declares()` lens (v5) the wire `summary`/`refusal_reason` must
//! already BE a full declared done block, read back with the real
//! [`bloomery_core::action::parse_action`] and required to carry `outcome`,
//! `reason`, and at least one `evidence:` line, then emitted verbatim (see
//! [`render::done_completion`]). A v5 ideal that fails that parse is a
//! factory bug: the tool answers with its JSON error line and the factory
//! aborts the task, the same fail-loud posture every other factory bug has.
//!
//! One consequence worth stating, because it is the reason the turn-3 gate
//! protocol calls `find` observations *format*-faithful rather than
//! byte-faithful: `exec_find`'s per-hit line embeds a canonicalized,
//! absolute path, so a find observation rendered here carries this
//! binary's scratch-dir path bytes. That is recorded, pre-registered
//! instrument behavior (`docs/superpowers/evidence/2026-08-20-g5v3-protocol.md`
//! §6.2), not drift.

use std::io::{BufRead, Write};

use bloomery_core::action::lens::{land, Landing};
use bloomery_core::action::{PatchBody, PatchCodec};
use bloomery_daemon::config::EnvelopeLens;
use bloomery_daemon::task::task_loop::{render_task_prompt, transcript_entry};
use bloomery_daemon::task::{exec_find, exec_read, exec_run, ExecBounds};

// `src/bin/flywheel_tool.rs` is a crate ROOT (Cargo's `[[bin]] path`
// names it directly), and a crate root resolves a bare `mod foo;` to a
// SIBLING `src/bin/foo.rs` — not to `src/bin/flywheel_tool/foo.rs`, which
// is the rule for a non-root `foo.rs`. `#[path]` says where these two
// actually live, keeping them namespaced under this binary's own
// directory instead of loose in `src/bin/` where Cargo's autobin scan
// looks for other binaries.
#[path = "flywheel_tool/render.rs"]
mod render;
#[path = "flywheel_tool/scratch.rs"]
mod scratch;

use render::{
    describe_landing_failure, done_completion, find_completion, land_reference_patch, lens_for,
    patch_completion, read_completion, run_completion, Pair, Trajectory, FIND_PATH,
};
use scratch::{
    check_command_prefixes, files_to_materialize, real_target_read, run_exit_code, safe_relative,
    RequestFile, Scratch, ScratchId,
};

/// One request line's `"cmd"` discriminator. Only `trajectory` exists
/// today (task-1 brief: "the only one the factory needs") — declared as a
/// tagged enum anyway so an unrecognized `cmd` value is a named parse
/// error (`unknown variant`) rather than a silently-ignored field, and so
/// a later subcommand has a place to land without restructuring this file.
#[derive(serde::Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
enum Request {
    Trajectory(TrajectoryRequest),
}

/// The `trajectory` request's expected-outcome class — task-3 brief / G5
/// design doc §2's wire spelling (`"patch"` / `"refuse"`), restated here
/// (rather than imported from `codec_probe::fixtures::Expect`) because that
/// module pulls in the daemon's probe machinery — well outside what a
/// GPU-free, single-file trajectory renderer needs — the same "duplicate
/// the tiny enum, not the machinery" call this file already makes for
/// `EnvelopeLens` in [`parse_envelope`]. `#[default] Patch`: every request
/// that omits `"expect"` (every turn-1 request) reproduces today's shape
/// byte-for-byte.
#[derive(serde::Deserialize, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
enum TrajectoryExpect {
    #[default]
    Patch,
    Refuse,
}

/// The `trajectory` request body — task-1 brief's wire format, extended
/// additively by the task-3 brief: `search`/`replace`/`summary` are now
/// `Option` (required iff `expect == Patch`, checked in [`require`] rather
/// than by serde, so a factory bug names the missing field); `expect`
/// (default `Patch`), `refusal_reason` (required iff `expect == Refuse`),
/// and `target_missing` (refuse's missing-target family flag) are new.
/// `target_contents` stays a required `String` in both refuse families —
/// the real target's content for defect-absent, and (by convention) `""`
/// for missing-target, where it is never read.
#[derive(serde::Deserialize)]
struct TrajectoryRequest {
    goal: String,
    patch_codec: String,
    envelope: String,
    target: String,
    target_contents: String,
    #[serde(default)]
    search: Option<String>,
    #[serde(default)]
    replace: Option<String>,
    #[serde(default)]
    summary: Option<String>,
    #[serde(default)]
    expect: TrajectoryExpect,
    #[serde(default)]
    refusal_reason: Option<String>,
    #[serde(default)]
    target_missing: bool,
    /// Turn 3: the whole workspace this task's trajectory happens in —
    /// `target` plus whatever siblings the fixture carries. Materialized
    /// into a throwaway scratch dir whenever a shape needs a real
    /// `find`/`read`/`run` to happen ([`files_to_materialize`] falls back
    /// to `target`/`target_contents` alone when it is empty, so a
    /// single-file run-verified request need not restate its target here).
    #[serde(default)]
    files: Vec<RequestFile>,
    /// Turn 3: set => render the find-shaped 4-pair trajectory
    /// ([`handle_find_trajectory`]). Mutually exclusive with `run_argv`.
    #[serde(default)]
    find_pattern: Option<String>,
    /// Turn 3: set => render the run-verified 4-pair trajectory
    /// ([`handle_run_trajectory`]). Mutually exclusive with `find_pattern`.
    #[serde(default)]
    run_argv: Option<Vec<String>>,
    /// Turn 3: the granted command prefixes the scratch grant carries —
    /// the same `commands` shape a fixture's grant carries, so `run_argv`
    /// is checked against the real [`bloomery_core::grant::Grant`]
    /// allowlist rather than trusted.
    #[serde(default)]
    commands: Vec<Vec<String>>,
}

/// The `trajectory` response body — task-1 brief's wire format, extended
/// additively by the task-3 brief. `patched_contents`/`landing_detail`
/// remain mutually exclusive with each other (one is always `None`), so
/// both are omitted from the JSON when absent rather than serialized as
/// `null` — a patch-mode response never gains `verified` either, for the
/// same reason, which is what keeps `expect` absent byte-identical to
/// turn 1. `verified` is `Some("refusal")` for both refuse families and
/// `None` for patch: `landed` is unconditionally `true` for a refuse
/// response (no landing check applies — nothing was ever patched), so a
/// factory reading `landed` alone cannot tell a refusal from a vacuous
/// success; `verified` is the field that lets it assert it exercised the
/// right path.
#[derive(serde::Serialize)]
struct TrajectoryResponse {
    pairs: Vec<Pair>,
    landed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    patched_contents: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    landing_detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    verified: Option<String>,
}

/// The `verified` marker value for both refuse families (task-3 brief).
const VERIFIED_REFUSAL: &str = "refusal";

/// A malformed request line, or a request this binary cannot honor (an
/// unrecognized `patch_codec`/`envelope` value). Reported as its own JSON
/// line rather than crashing the process — a factory batching thousands of
/// lines needs one bad line to fail loudly and visibly, not take the whole
/// run down.
#[derive(serde::Serialize)]
struct ErrorResponse {
    error: String,
}

/// Parses the wire `patch_codec` string. Only `"search_replace"` is
/// supported: the `trajectory` request's wire shape only carries
/// `search`/`replace` fields (design spec §3's mechanically-derived
/// trajectories are exactly this shape — never whole-file), so there is no
/// input this binary could build a `WholeFile` completion from. Rejecting
/// `"whole_file"` here — rather than half-implementing it against fields
/// that do not exist — keeps this binary honest about what it can actually
/// verify.
fn parse_patch_codec(raw: &str) -> Result<PatchCodec, String> {
    match raw {
        "search_replace" => Ok(PatchCodec::SearchReplace),
        other => Err(format!(
            "unsupported patch_codec {other:?}: flywheel-tool only supports \"search_replace\""
        )),
    }
}

/// Parses the wire `envelope` string — the same five values
/// `EnvelopeLens`'s config parser accepts (`config.rs`'s `EnvelopeLens::parse`
/// is private to that module, so this binary — a separate crate — restates
/// the same tiny string-to-enum mapping rather than reaching for it).
fn parse_envelope(raw: &str) -> Result<EnvelopeLens, String> {
    match raw {
        "v1" => Ok(EnvelopeLens::V1),
        "v2" => Ok(EnvelopeLens::V2),
        "v3" => Ok(EnvelopeLens::V3),
        "v4" => Ok(EnvelopeLens::V4),
        "v5" => Ok(EnvelopeLens::V5),
        other => Err(format!(
            "unknown envelope {other:?}: valid values are \"v1\", \"v2\", \"v3\", \"v4\", \"v5\""
        )),
    }
}

/// Reads a request field required for `expect = "{expect_name}"`,
/// returning a named parse error (not a silent `None` a downstream branch
/// discovers) when the factory omitted it for that mode — the same
/// "unrecognized/missing input is a named error, never silently ignored"
/// posture [`parse_patch_codec`]/[`parse_envelope`] already use.
fn require<'a>(
    field: &'a Option<String>,
    name: &str,
    expect_name: &str,
) -> Result<&'a str, String> {
    field.as_deref().ok_or_else(|| {
        format!("trajectory request with expect=\"{expect_name}\" requires \"{name}\"")
    })
}

/// Dispatches on `req.expect` (task-3 brief), then — inside patch mode — on
/// which of turn 3's two shape selectors the request carries.
/// `patch_codec`/`envelope` are parsed once, up front, since every mode
/// renders prompts through the same [`render_task_prompt`] and needs them
/// regardless.
///
/// `find_pattern` and `run_argv` name two *different* 4-pair shapes, so a
/// request carrying both is a named error rather than a silent pick-one:
/// there is no defined trajectory that is both, and guessing one would hand
/// the factory training data it did not ask for. Both are equally a named
/// error under `expect = "refuse"`, which renders exactly 2 pairs and never
/// patches anything — there is nothing for a `find` or a verification `run`
/// to attach to.
fn handle_trajectory(req: &TrajectoryRequest) -> Result<TrajectoryResponse, String> {
    let codec = parse_patch_codec(&req.patch_codec)?;
    let envelope = parse_envelope(&req.envelope)?;
    // Checked for every mode, not just the two that build a scratch grant:
    // under envelope-v4 these words are rendered into the prompt.
    check_command_prefixes(&req.commands)?;

    match (
        req.expect,
        req.find_pattern.as_deref(),
        req.run_argv.as_deref(),
    ) {
        (TrajectoryExpect::Patch, None, None) => handle_patch_trajectory(req, codec, envelope),
        (TrajectoryExpect::Patch, Some(pattern), None) => {
            handle_find_trajectory(req, codec, envelope, pattern)
        }
        (TrajectoryExpect::Patch, None, Some(argv)) => {
            handle_run_trajectory(req, codec, envelope, argv)
        }
        (TrajectoryExpect::Patch, Some(_), Some(_)) => Err(
            "trajectory request carries both \"find_pattern\" and \"run_argv\": they select two \
             different 4-pair shapes (find/read/patch/done and read/patch/run/done) and there is \
             no trajectory that is both — send exactly one"
                .to_string(),
        ),
        (TrajectoryExpect::Refuse, Some(_), _) | (TrajectoryExpect::Refuse, _, Some(_)) => Err(
            "trajectory request with expect=\"refuse\" carries \"find_pattern\" or \"run_argv\": \
             a refusal renders exactly 2 pairs (read, done) and never patches anything, so \
             neither shape selector applies"
                .to_string(),
        ),
        (TrajectoryExpect::Refuse, None, None) => handle_refuse_trajectory(req, codec, envelope),
    }
}

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
fn handle_find_trajectory(
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
fn handle_run_trajectory(
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
fn handle_patch_trajectory(
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
fn handle_refuse_trajectory(
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

/// One JSON request per line on stdin, one JSON response per line on
/// stdout — see this module's docs. A line that fails to parse (or names
/// an unsupported codec/envelope) gets an `{"error": ...}` line rather than
/// aborting the process, so a factory driving thousands of lines learns
/// about one bad line without losing the rest of the batch.
fn main() {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let line = line.expect("stdin read failed");
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let response = match serde_json::from_str::<Request>(trimmed) {
            Ok(Request::Trajectory(req)) => match handle_trajectory(&req) {
                Ok(resp) => serde_json::to_string(&resp),
                Err(e) => serde_json::to_string(&ErrorResponse { error: e }),
            },
            Err(e) => serde_json::to_string(&ErrorResponse {
                error: format!("bad request: {e}"),
            }),
        }
        .expect("response always serializes");
        writeln!(stdout, "{response}").expect("stdout write failed");
        stdout.flush().expect("stdout flush failed");
    }
}
