// bloomery — an operating layer for local LLMs.
// Copyright (C) 2026 Brice Lancaster
//
// This program is free software: you can redistribute it and/or modify it
// under the terms of the GNU Affero General Public License, version 3, as
// published by the Free Software Foundation.
//
// This program is distributed in the hope that it will be useful, but WITHOUT
// ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or
// FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// for more details.
//
// You should have received a copy of the GNU Affero General Public License
// along with this program. If not, see <https://www.gnu.org/licenses/>.
//
// Commercial licensing is available as an alternative to the AGPL — see
// LICENSING.md.

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

use bloomery_core::action::PatchCodec;
use bloomery_daemon::config::EnvelopeLens;

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
#[path = "flywheel_tool/trajectories.rs"]
mod trajectories;

use render::Pair;
use scratch::{check_command_prefixes, RequestFile};
use trajectories::{
    handle_find_trajectory, handle_patch_trajectory, handle_refuse_trajectory,
    handle_run_trajectory,
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
pub(crate) enum TrajectoryExpect {
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
pub(crate) struct TrajectoryRequest {
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
pub(crate) struct TrajectoryResponse {
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
pub(crate) const VERIFIED_REFUSAL: &str = "refusal";

/// A malformed request line, or a request this binary cannot honor (an
/// unrecognized `patch_codec`/`envelope` value). Reported as its own JSON
/// line rather than crashing the process — a factory batching thousands of
/// lines needs one bad line to fail loudly and visibly, not take the whole
/// run down.
#[derive(serde::Serialize)]
pub(crate) struct ErrorResponse {
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
pub(crate) fn require<'a>(
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
