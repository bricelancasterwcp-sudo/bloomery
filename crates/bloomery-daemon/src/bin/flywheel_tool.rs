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
//!   wrappers task_loop.rs added around its own private `render_prompt`
//!   and `record_step` formatting, so this binary's prompts are the exact
//!   bytes a live task would render, not a second implementation of the
//!   envelope.
//! - Landing: [`bloomery_core::action::lens::land`], the real
//!   [`bloomery_core::action::lens::PlainText`] lens, and the real
//!   [`bloomery_daemon::task::lens_py::PythonLens`] — the same applier and
//!   lenses `exec_patch` uses in a live task.
//!
//! **A separate crate, on purpose.** A `[[bin]]` target in the same Cargo
//! package as a library is still its own compiled crate: it can only see
//! the library's `pub` surface, never `pub(crate)` internals. That is
//! exactly the boundary this file is built to respect — `render_prompt`
//! and `record_step` themselves stay private; only the two pinned wrappers
//! (and `land`/the lenses, already `pub`) cross into this binary. The one
//! piece of logic this file does duplicate is trivial and stated as such:
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

use std::io::{BufRead, Write};

use bloomery_core::action::lens::{land, Landing, LandingLens, PlainText};
use bloomery_core::action::{PatchBody, PatchCodec};
use bloomery_daemon::config::EnvelopeLens;
use bloomery_daemon::task::lens_py::PythonLens;
use bloomery_daemon::task::task_loop::{render_task_prompt, transcript_entry};

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

/// The `trajectory` request body — task-1 brief's wire format, verbatim.
#[derive(serde::Deserialize)]
struct TrajectoryRequest {
    goal: String,
    patch_codec: String,
    envelope: String,
    target: String,
    target_contents: String,
    search: String,
    replace: String,
    summary: String,
}

/// One (prompt, completion) SFT pair.
#[derive(serde::Serialize)]
struct Pair {
    prompt: String,
    completion: String,
}

/// The `trajectory` response body — task-1 brief's wire format, verbatim.
/// `patched_contents`/`landing_detail` are mutually exclusive with each
/// other (one is always `None`), so both are omitted from the JSON when
/// absent rather than serialized as `null`.
#[derive(serde::Serialize)]
struct TrajectoryResponse {
    pairs: Vec<Pair>,
    landed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    patched_contents: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    landing_detail: Option<String>,
}

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

/// Parses the wire `envelope` string — the same three values
/// `EnvelopeLens`'s config parser accepts (`config.rs`'s `EnvelopeLens::parse`
/// is private to that module, so this binary — a separate crate — restates
/// the same tiny string-to-enum mapping rather than reaching for it).
fn parse_envelope(raw: &str) -> Result<EnvelopeLens, String> {
    match raw {
        "v1" => Ok(EnvelopeLens::V1),
        "v2" => Ok(EnvelopeLens::V2),
        "v3" => Ok(EnvelopeLens::V3),
        other => Err(format!(
            "unknown envelope {other:?}: valid values are \"v1\", \"v2\", \"v3\""
        )),
    }
}

/// Chooses the landing lens for `target` by extension — see this module's
/// docs for why duplicating this one-line check (and not the landing logic
/// itself) is acceptable.
fn lens_for(target: &str) -> Box<dyn LandingLens> {
    if target.ends_with(".py") {
        Box::new(PythonLens)
    } else {
        Box::new(PlainText)
    }
}

/// Describes a non-`Lands` [`Landing`] outcome for the response's
/// `landing_detail` field, mirroring `exec_patch`'s own outcome-string
/// shape (`task/exec.rs:~357-377`) — reporting only, not a second landing
/// implementation: the `Landing` value itself came straight out of the
/// real [`land`] call.
fn describe_landing_failure(landing: &Landing) -> String {
    match landing {
        Landing::Lands { .. } => unreachable!("Lands is handled by the caller before this runs"),
        Landing::DidNotApply { reason, lens } => {
            format!("did not apply (lens: {lens}): {reason:?}")
        }
        Landing::DidNotParse { detail, lens } => format!("did not parse (lens: {lens}): {detail}"),
        Landing::Unparsed { language, lens } => {
            format!("lens {lens} cannot judge language {language}")
        }
    }
}

/// Builds pair 1's completion: `<action verb="read" path="{target}">` with
/// an empty body — no trailing newline after the closing tag (task-1
/// brief, pinned).
fn read_completion(target: &str) -> String {
    format!("<action verb=\"read\" path=\"{target}\">\n</action>")
}

/// Builds pair 2's completion: the `SearchReplace` conflict-marker body,
/// matching `bloomery_core::action::card`'s worked example grammar exactly
/// (`<<<<<<< SEARCH` / `=======` / `>>>>>>> REPLACE`).
fn patch_completion(target: &str, search: &str, replace: &str) -> String {
    format!(
        "<action verb=\"patch\" path=\"{target}\">\n<<<<<<< SEARCH\n{search}\n=======\n\
         {replace}\n>>>>>>> REPLACE\n</action>"
    )
}

/// Builds pair 3's completion: `<action verb="done">` with `summary` as
/// its one-line body.
fn done_completion(summary: &str) -> String {
    format!("<action verb=\"done\">\n{summary}\n</action>")
}

/// Handles one `trajectory` request end to end: renders the three prompts
/// via the real [`render_task_prompt`]/[`transcript_entry`], and
/// land-verifies the reference patch via the real [`land`]. Pair
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
fn handle_trajectory(req: &TrajectoryRequest) -> Result<TrajectoryResponse, String> {
    let codec = parse_patch_codec(&req.patch_codec)?;
    let envelope = parse_envelope(&req.envelope)?;

    let prompt1 = render_task_prompt(&req.goal, codec, envelope, "");
    let completion1 = read_completion(&req.target);

    let read_outcome = format!("read {} bytes", req.target_contents.len());
    let transcript_after_read = transcript_entry(1, "read", &read_outcome, &req.target_contents);
    let prompt2 = render_task_prompt(&req.goal, codec, envelope, &transcript_after_read);
    let completion2 = patch_completion(&req.target, &req.search, &req.replace);

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
        search: req.search.clone(),
        replace: req.replace.clone(),
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
            });
        }
    };

    let patch_outcome = format!("patched (lens: {lens_name})");
    let transcript_after_patch = format!(
        "{transcript_after_read}{}",
        transcript_entry(2, "patch", &patch_outcome, &new_contents)
    );
    let prompt3 = render_task_prompt(&req.goal, codec, envelope, &transcript_after_patch);
    let completion3 = done_completion(&req.summary);

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
    })
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
