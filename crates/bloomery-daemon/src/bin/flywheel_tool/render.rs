//! Everything `flywheel-tool` renders: the completion text for every verb
//! a trajectory can contain, and the [`Trajectory`] accumulator that pairs
//! each prompt with the completion the model is supposed to produce from
//! it.
//!
//! Split out of `flywheel_tool.rs` when turn 3's two new shapes pushed that
//! file past the repo's 800-line ceiling, following the same
//! `exec_run.rs`-out-of-`exec.rs` precedent the daemon's task module set.
//! The boundary is deliberate rather than merely size-driven: nothing in
//! here touches the filesystem, spawns anything, or reads a request — it is
//! pure input-to-text, which is exactly the surface the briefs pin
//! byte-for-byte.
//!
//! The one rule every function here obeys is the parent module's whole
//! design: this module renders *completions* (what the model is trained to
//! emit) and delegates *prompts* to the real
//! [`render_task_prompt`]/[`transcript_entry`]. It never formats an
//! observation — those come from the real executors, in
//! `flywheel_tool.rs`'s handlers.

use bloomery_core::action::lens::{land, Landing, LandingLens, PlainText};
use bloomery_core::action::{PatchBody, PatchCodec};
use bloomery_daemon::config::EnvelopeLens;
use bloomery_daemon::task::lens_py::PythonLens;
use bloomery_daemon::task::task_loop::{render_task_prompt, transcript_entry};

/// One (prompt, completion) SFT pair.
#[derive(serde::Serialize)]
pub(crate) struct Pair {
    pub(crate) prompt: String,
    pub(crate) completion: String,
}

/// Chooses the landing lens for `target` by extension — see this module's
/// docs for why duplicating this one-line check (and not the landing logic
/// itself) is acceptable.
pub(crate) fn lens_for(target: &str) -> Box<dyn LandingLens> {
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
pub(crate) fn describe_landing_failure(landing: &Landing) -> String {
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
pub(crate) fn read_completion(target: &str) -> String {
    format!("<action verb=\"read\" path=\"{target}\">\n</action>")
}

/// Builds pair 2's completion: the `SearchReplace` conflict-marker body,
/// matching `bloomery_core::action::card`'s worked example grammar exactly
/// (`<<<<<<< SEARCH` / `=======` / `>>>>>>> REPLACE`).
pub(crate) fn patch_completion(target: &str, search: &str, replace: &str) -> String {
    format!(
        "<action verb=\"patch\" path=\"{target}\">\n<<<<<<< SEARCH\n{search}\n=======\n\
         {replace}\n>>>>>>> REPLACE\n</action>"
    )
}

/// Builds pair 3's completion: `<action verb="done">` with `summary` as
/// its one-line body.
pub(crate) fn done_completion(summary: &str) -> String {
    format!("<action verb=\"done\">\n{summary}\n</action>")
}

/// The `path` attribute a rendered `find` completion carries: the task's
/// own workspace root, workspace-relative. The model is trained to search
/// where it is standing, never to name this binary's scratch directory —
/// which is also what the real loop does with it (`task_loop.rs`'s
/// dispatch absolutizes a relative `find` path against the task's cwd
/// before calling `exec_find`, exactly as `flywheel_tool.rs`'s
/// `handle_find_trajectory` does).
pub(crate) const FIND_PATH: &str = ".";

/// Builds the find-shaped trajectory's pair-1 completion, matching the
/// verb card's `find` grammar exactly (`bloomery_core::action::card`:
/// `pattern` then `path`, empty body, no trailing newline after the
/// closing tag).
pub(crate) fn find_completion(pattern: &str, path: &str) -> String {
    format!("<action verb=\"find\" pattern=\"{pattern}\" path=\"{path}\">\n</action>")
}

/// Builds the run-verified trajectory's pair-3 completion, matching the
/// verb card's `run` grammar exactly: no attributes, and a JSON array of
/// argv strings as the body.
pub(crate) fn run_completion(argv: &[String]) -> String {
    format!(
        "<action verb=\"run\">\n{}\n</action>",
        serde_json::to_string(argv).expect("a Vec<String> always serializes")
    )
}

/// Accumulates a trajectory one step at a time, exactly as `run_task`'s own
/// loop does: render the prompt the model sees *now* (from the transcript
/// so far), pair it with the completion it is supposed to emit, then fold
/// that step's observation into the transcript for the next prompt.
///
/// This type owns no formatting of its own — every entry goes through the
/// real [`transcript_entry`] and every prompt through the real
/// [`render_task_prompt`]; all it adds is the running total and the step
/// counter, which is precisely the bookkeeping `record_step` does. Turn 1's
/// `handle_patch_trajectory` deliberately does NOT use it: that path is
/// pinned byte-for-byte by the turn-1 golden and is left exactly as it was.
pub(crate) struct Trajectory<'a> {
    goal: &'a str,
    codec: PatchCodec,
    envelope: EnvelopeLens,
    transcript: String,
    step: u32,
    pub(crate) pairs: Vec<Pair>,
}

impl<'a> Trajectory<'a> {
    pub(crate) fn new(goal: &'a str, codec: PatchCodec, envelope: EnvelopeLens) -> Self {
        Trajectory {
            goal,
            codec,
            envelope,
            transcript: String::new(),
            step: 0,
            pairs: Vec::new(),
        }
    }

    /// Emits one (prompt, completion) pair: the prompt the transcript so far
    /// renders to, and the completion the model is supposed to produce from
    /// it.
    pub(crate) fn emit(&mut self, completion: String) {
        self.pairs.push(Pair {
            prompt: render_task_prompt(self.goal, self.codec, self.envelope, &self.transcript),
            completion,
        });
    }

    /// Folds one step's real observation into the transcript.
    pub(crate) fn observe(&mut self, verb: &str, outcome: &str, content: &str) {
        self.step += 1;
        self.transcript
            .push_str(&transcript_entry(self.step, verb, outcome, content));
    }
}

/// Lands the reference patch through the real [`land`] — shared by both
/// turn-3 shapes, and the exact same call `handle_patch_trajectory` makes.
///
/// `Err` here carries a [`describe_landing_failure`] detail string, NOT a
/// request error: a patch that does not land is a `landed: false` response
/// (the pairs built so far, plus the detail), the partial-response contract
/// turn 1 established — so both turn-3 callers turn this `Err` into a
/// response rather than propagating it.
pub(crate) fn land_reference_patch(
    contents: &str,
    target: &str,
    search: &str,
    replace: &str,
) -> Result<(String, &'static str), String> {
    let body = PatchBody::SearchReplace {
        search: search.to_string(),
        replace: replace.to_string(),
    };
    let lens = lens_for(target);
    match land(contents, &body, lens.as_ref()) {
        Landing::Lands { new_contents, lens } => Ok((new_contents, lens)),
        other => Err(describe_landing_failure(&other)),
    }
}
