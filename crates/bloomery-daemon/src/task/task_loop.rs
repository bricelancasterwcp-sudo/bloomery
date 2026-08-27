//! The propose→validate→execute task loop (Phase 2b/2c P3 Task 4).
//!
//! `run_task` is the state machine that stitches together everything Tasks
//! 1-3 built: it prompts the model with the verb card and an accumulated
//! observation transcript, decodes its turn through P1's
//! `parse_action_with_codec`, dispatches the decoded `Action` to the
//! matching executor (`exec_read`/`exec_find`/`exec_patch`/`exec_run`), and
//! journals a `TaskStep` per attempt. It never trusts the model's output as
//! anything more than a proposal: an unparseable turn is re-asked (up to
//! twice) rather than crashing the task, and a grant violation is a failed
//! step the model can see and recover from, not a task abort — the only
//! things that end a task early are the model itself emitting `done`, the
//! pager refusing on budget or window grounds, a substrate/contract
//! failure, or `max_steps` running out.
//!
//! Generic over `Substrate`, like every other daemon-level module, so the
//! whole loop is exercised GPU-free against `FakeSubstrate` with scripted
//! `<action>` turns — see `tests/task_loop_test.rs` for the five binding
//! tests.

use std::path::Path;
use std::time::Instant;

use bloomery_core::action::{
    parse_action_with_codec, verb_card_for, Action, PatchBody, PatchCodec,
};
use bloomery_core::grant::Grant;
use bloomery_core::journal::{Event, Journal};
use bloomery_substrate::Substrate;

use crate::config::EnvelopeLens;
use crate::pager::{Pager, PagerError};
use crate::task::exec::absolutize;
use crate::task::grant_line::grant_line;
use crate::task::{exec_find, exec_patch, exec_read, exec_run, ExecBounds, Observation, PreTouch};

/// Everything `run_task` needs to run one task to completion: what the
/// model is trying to do, the capability boundary it may act within, and
/// the operational limits.
///
/// `budget_tokens` mirrors the agent's own pager-level `Budget` (set at
/// `Pager::create_agent` time, by whoever builds both together — Task 5's
/// HTTP registry) — carried here so a caller has one place to read it back
/// from. `run_task` itself never reads this field: the pager's own budget
/// check on every `infer` call is what actually enforces it. Task 5's
/// `create_task` rejects a request `budget_tokens` above the agent's granted
/// budget with `422 budget_exceeds_grant` before a `TaskSpec` is even built
/// (a number this loop could never honor), but any value at or below that
/// ceiling is accepted and then simply unused here — it is not yet a
/// separate, enforced per-task cap.
#[derive(Debug, Clone)]
pub struct TaskSpec {
    pub goal: String,
    pub grant: Grant,
    pub budget_tokens: u64,
    pub max_steps: u32,
    /// The executors' working directory: first `write_root`, else first
    /// `read_root` (the caller computes this — see the P3 plan's Task 4
    /// brief). `run_task` uses it verbatim to absolutize every relative
    /// model-supplied path before it reaches a `Grant` check.
    pub cwd: std::path::PathBuf,
    pub patch_codec: PatchCodec,
    pub bounds: ExecBounds,
    /// Gate G4 (`docs/superpowers/evidence/2026-08-15-g4-protocol.md` §6):
    /// `false` means this model is demoted (or unmeasured — fail-closed
    /// read-only, same §6) and may not use `patch`/`run` this task.
    /// `render_prompt_at_rung` reflects this in the verb card the model sees
    /// (`verb_card_for(spec.patch_codec, spec.mutating_verbs)`), and
    /// `run_task` enforces it structurally — a `patch`/`run` action is
    /// refused before `execute_action` ever dispatches it, regardless of
    /// what the card told the model. This task (P4 Task 7) always sets it
    /// `true`; a later task (Task 8) wires the real per-model value decided
    /// by the gate at agent-admission time.
    pub mutating_verbs: bool,
    /// The task-loop prompt envelope
    /// (`docs/superpowers/evidence/2026-08-15-g4-protocol.md` §10/§11,
    /// Amendments 2 and 3; turn-4 spec §2 for `V4`): [`render_prompt_at_rung`]
    /// appends the literal [`THINK_PRESEED`] pre-seed at the very end of the
    /// rendered prompt for `V2` and up; [`propose_action`] passes
    /// [`ACTION_STOP`] as the substrate's stop sequence for `V3` and up; and
    /// `V4` alone renders [`grant_line`] from this spec's own [`Grant`],
    /// between the goal and the verb card. `V1` renders exactly
    /// envelope-v1's prompt, byte-for-byte, with no stop sequence.
    pub envelope: EnvelopeLens,
    /// The memory organ's injected block, already rendered
    /// (`crate::memory::render::render_memory_block`) — memory-organ design
    /// spec `docs/superpowers/specs/2026-08-26-memory-organ-design.md` §4.
    /// `None` is the organ off, the organ silent, and the organ broken, all
    /// three: [`render_prompt_from`] renders the empty string for it, so a
    /// `None` task's prompt is byte-identical to what the same task rendered
    /// before this field existed (§7: the organ's "total failure must be
    /// indistinguishable from memory-off").
    ///
    /// A rendered `String` rather than an `EpisodeRecord`, deliberately: the
    /// loop is not the place that decides what an episode looks like, and a
    /// spec carrying a record would make every construction site — including
    /// the codec probe and the flywheel factory — depend on the memory
    /// module. Retrieval and rendering happen once, before the task starts;
    /// the worker (Task 7) is what sets this, and every other construction
    /// site passes `None`, `api_task.rs`'s `create_task` included.
    ///
    /// Orthogonal to [`Self::envelope`] (§4's envelope rule): the block
    /// renders identically under every lens, and no lens version was minted
    /// for it.
    pub memory_block: Option<String>,
    /// The window ladder (spec
    /// `docs/superpowers/specs/2026-08-27-window-ladder-design.md`): `true`
    /// opts this task into fixed scope degradation on `PromptTooLarge` —
    /// `propose_action` re-renders one rung smaller and re-submits, refusing
    /// only when rung 4 still doesn't fit. `false` — the default at EVERY
    /// construction site except `api_task`'s request wiring — is today's
    /// behavior byte-for-byte: the first `PromptTooLarge` is terminal.
    /// Every frozen instrument (codec probe, flywheel factory, batteries)
    /// passes `false` permanently; their measured verdicts were taken under
    /// die-on-413 and stay comparable only if that never moves.
    pub window_ladder: bool,
}

/// How a task ended. `run_task` only ever returns `Done`, `BudgetExhausted`,
/// `StepsExhausted`, `WindowExhausted`, or `Error` — `Running` exists for a
/// caller that tracks task status outside a single `run_task` call (Task 5's
/// in-flight status reporting). A grant-validation failure at task creation
/// never produces a `TaskStatus` at all: Task 5's `create_task` returns a
/// `422` HTTP error before a task (and therefore a status) exists.
///
/// `WindowExhausted` is protocol Amendment 1
/// (`docs/superpowers/evidence/2026-08-15-g4-protocol.md` §9, 2026-08-15):
/// a mid-task `PagerError::PromptTooLarge` from `infer` — the model's
/// measured context window filled before it finished — is scored the same
/// way `BudgetExhausted` already is, not folded into `Error`'s
/// infrastructure-abort bucket. Every other `infer` failure (substrate
/// faults, journal failures, agent-creation refusals) stays `Error`.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub enum TaskStatus {
    Running,
    Done,
    BudgetExhausted,
    StepsExhausted,
    WindowExhausted,
    Error,
}

/// One step's outcome, as returned to `run_task`'s caller — richer than the
/// journaled `Event::TaskStep` (it carries `content`, the full observation
/// text, rather than `duration_ms`), so a caller like Task 5's HTTP surface
/// can render a step without re-reading the journal.
#[derive(Debug, Clone, serde::Serialize)]
pub struct TaskStepRecord {
    pub step: u32,
    pub verb: String,
    pub outcome: String,
    pub content: String,
    pub failed: bool,
    /// Same list `Event::TaskStep::args` carries (turn-5 spec §3).
    pub args: Vec<String>,
    /// The ladder rung this step's prompt was sent at (window-ladder spec
    /// §6) — the same value `Event::TaskStep::rung` carries. Always 1 for a
    /// ladder-off task. Serialized, so `get_task`'s `"steps"` array exposes
    /// it without any `api_task` change.
    pub rung: u32,
}

/// One task's terminal outcome, plus the evidence a later mint step reads.
///
/// `touched_files` and `landed_patches` are the memory organ's capture
/// (design spec `docs/superpowers/specs/2026-08-26-memory-organ-design.md`
/// §2), carried here rather than recomputed after the fact: the pre-touch
/// fingerprints only exist *during* execution, and the landed bodies are
/// deliberately absent from the journal (`action_args` excludes patch
/// bodies on purpose). Both are populated at every `run_task` return site,
/// including the terminal ones — a task that ran out of steps or window
/// still touched whatever it touched, and hiding that from a caller would
/// make the two fields silently mean "only on a clean finish".
///
/// **Serialized but not exposed:** `get_task`'s HTTP response
/// (`api_task.rs`) is built field-by-field from `status`/`steps`/`summary`
/// and is deliberately untouched by this addition — the capture is
/// in-process evidence for minting, not a public API surface.
#[derive(Debug, Clone, serde::Serialize)]
pub struct TaskResult {
    pub status: TaskStatus,
    pub steps: Vec<TaskStepRecord>,
    pub summary: Option<String>,
    /// Canonical path → the file's fingerprint as of this task's FIRST
    /// touch of it. A `BTreeMap` so iteration order is the path order spec
    /// §2's `episode_id` hash requires ("cited files sorted by path
    /// first"), rather than a hash order that would make the same task mint
    /// different ids on different runs.
    pub touched_files: std::collections::BTreeMap<String, PreTouch>,
    /// Every successfully landed patch, in step order: `(canonical path,
    /// the decoded body verbatim)`. Spec §2 stores these so "on an exact
    /// repeat the model can literally replay them" — which only works if
    /// the body is the codec text that actually landed, not a re-rendering.
    pub landed_patches: Vec<(String, PatchBody)>,
}

/// Per-step model-completion budget passed as `pager.infer`'s `max_tokens`.
/// Higher than `api_v1`'s `DEFAULT_MAX_TOKENS` (256): a task step's turn
/// carries a full `<action>` envelope, and a `patch` body in particular can
/// run to many lines, so a chat-reply-sized cap would truncate exactly the
/// verb this loop exists to execute.
const STEP_MAX_TOKENS: u32 = 1024;

/// How many times one step may attempt to produce a parseable action: the
/// first attempt plus 2 re-asks, per the binding brief ("re-ask ... up to 2
/// times per step").
const MAX_PARSE_ATTEMPTS: u32 = 3;

/// The fixed ladder's smallest rung (spec §2). Four rungs then refusal —
/// robigo's shape. A rung outside 1..=MAX_RUNG reaching the renderer is a
/// programming error and panics (spec §7: no silent clamping, either
/// direction, ever).
const MAX_RUNG: u32 = 4;

/// The pinned gate-G4 refusal outcome (Task 7 brief — exact bytes; Task 9's
/// scoring and the journal read this string). Recorded, not raised: a
/// refused `patch`/`run` is a failed step like a grant violation or a parse
/// failure, never a task-ending error — see [`run_task`]'s demotion gate.
const MUTATING_VERB_DEMOTED: &str = "verb unavailable: mutating verbs demoted (gate G4)";

/// Accumulated task state threaded through both `propose_action` and
/// `run_task`'s own dispatch — bundled into one struct (rather than two
/// separate `&mut` params) so neither function's argument count creeps
/// toward clippy's `too_many_arguments` threshold.
struct TaskState {
    steps: Vec<TaskStepRecord>,
    transcript: String,
    /// The memory-organ capture accumulating across this task's steps —
    /// see [`TaskResult`]'s fields of the same names, which these are
    /// moved into at every return site.
    touched: std::collections::BTreeMap<String, PreTouch>,
    landed: Vec<(String, PatchBody)>,
}

/// Consumes `state` into the terminal [`TaskResult`] for `status`.
///
/// **Every** one of [`run_task`]'s return sites goes through this function,
/// which is the point: the memory-organ capture (`touched_files`,
/// `landed_patches`) is carried out of the loop by construction, so a
/// future early-return arm cannot quietly drop the evidence the way it
/// could if each site built its own struct literal.
fn finish(state: TaskState, status: TaskStatus, summary: Option<String>) -> TaskResult {
    TaskResult {
        status,
        steps: state.steps,
        summary,
        touched_files: state.touched,
        landed_patches: state.landed,
    }
}

/// One journaled-and-recorded step's content, bundled for the same
/// too-many-arguments reason as [`TaskState`].
struct StepReport<'a> {
    verb: &'a str,
    outcome: &'a str,
    content: &'a str,
    duration_ms: u64,
    failed: bool,
    args: Vec<String>,
    /// The ladder rung the prompt behind this step was ACTUALLY sent at
    /// (window-ladder spec §6), carried into both the journal row and the
    /// [`TaskStepRecord`].
    rung: u32,
}

/// The action's arguments as the journal records them (turn-5 spec §3).
/// Never the patch body: landing is re-derivable from the frozen fixture
/// and the scratch dir, and the body would bloat every journal.
fn action_args(action: &Action) -> Vec<String> {
    match action {
        Action::Read { path, lines: None } => vec![path.clone()],
        Action::Read {
            path,
            lines: Some((a, b)),
        } => vec![path.clone(), format!("lines={a}-{b}")],
        Action::Find { pattern, path } => vec![pattern.clone(), path.clone()],
        Action::Patch { path, .. } => vec![path.clone()],
        Action::Run { argv } => argv.clone(),
        Action::Done { .. } => Vec::new(),
    }
}

/// One step's transcript entry — the exact text `record_step` folds into
/// the running transcript so the next prompt sees it. Pinned format (task-1
/// brief, flywheel spec §2, `docs/superpowers/specs/2026-08-16-flywheel-14b-design.md`):
/// `"\n[step {step} {verb}] {outcome}\n{content}\n"`.
///
/// Extracted out of `record_step`'s own `push_str` call — `record_step`
/// below calls this directly, so there is exactly one place this format
/// string is written. `flywheel-tool`
/// (`crates/bloomery-daemon/src/bin/flywheel_tool.rs`) is the other caller:
/// it reconstructs a training pair's transcript by calling this same
/// function with the same inputs a real task step would have produced, so
/// the rendered training prompt can never drift from what `record_step`
/// actually appends during a live task. `pub`, not `pub(crate)`: a bin
/// target is a separate crate from this library, even within the same
/// package (see `flywheel_tool.rs`'s module docs).
pub fn transcript_entry(step: u32, verb: &str, outcome: &str, content: &str) -> String {
    format!("\n[step {step} {verb}] {outcome}\n{content}\n")
}

/// Appends `report` to both the journal (as an `Event::TaskStep`) and
/// `state.steps` (as a richer `TaskStepRecord`), then folds `content` into
/// the running transcript so the next prompt sees it.
///
/// A journal write failure is treated the same way `pager.rs`'s module docs
/// treat its own journal (rule 4: "a journal that cannot be written fails
/// the request rather than letting the pager act unobserved") — the
/// returned `Err` is what `run_task` turns into `TaskStatus::Error`.
fn record_step(
    journal: &mut Journal,
    agent_id: &str,
    state: &mut TaskState,
    step: u32,
    report: StepReport<'_>,
) -> Result<(), String> {
    journal
        .append(&Event::TaskStep {
            id: agent_id.to_string(),
            step,
            verb: report.verb.to_string(),
            outcome: report.outcome.to_string(),
            duration_ms: report.duration_ms,
            args: report.args.clone(),
            rung: report.rung,
        })
        .map_err(|e| format!("journal write failed: {e}"))?;
    state.steps.push(TaskStepRecord {
        step,
        verb: report.verb.to_string(),
        outcome: report.outcome.to_string(),
        content: report.content.to_string(),
        failed: report.failed,
        args: report.args,
        rung: report.rung,
    });
    state.transcript.push_str(&transcript_entry(
        step,
        report.verb,
        report.outcome,
        report.content,
    ));
    Ok(())
}

/// The envelope-v2 pre-seed literal (protocol
/// `docs/superpowers/evidence/2026-08-15-g4-protocol.md` §10, Amendment 2):
/// a pre-closed `<think>` block appended at the very end of the rendered
/// prompt so the model continues generation already past its own thinking
/// phase, rather than opening a fresh one. Motivated by the 2026-08-15 eve
/// feasibility probe (Q3 subject) recorded in §10: `/no_think` did NOT
/// suppress thinking under the raw-completion lens, while appending this
/// pre-closed think block did. Exact bytes are load-bearing — [`run_task`]'s
/// mutation check pins them, and the codec probe's verdict `detail` records
/// which lens produced a result, never averaging v1 and v2 rungs together.
const THINK_PRESEED: &str = "<think>\n\n</think>\n\n";

/// The envelope-v3 stop sequence (protocol
/// `docs/superpowers/evidence/2026-08-15-g4-protocol.md` §11, Amendment 3):
/// generation of a task turn terminates at the first occurrence of this
/// literal in the completion, tag INCLUDED. Passed to [`Pager::infer`]'s
/// `stop` parameter for the lenses whose `action_stop()` is set — `V3` and
/// `V4`, which is defined as `V3` plus the grant line — never for `/v1` or
/// native HTTP inference (§11: "the `/v1` chat surface is untouched"). The
/// law-3 ruling (§11): a stop string is *termination, not constraint* — the
/// model's distribution is untouched up to the tag, the same class as
/// `max_tokens` and chat-template stop tokens, never grammar-forced
/// decoding.
const ACTION_STOP: &str = "</action>";

/// Builds the prompt for one model turn: the task's goal, the verb card for
/// `spec.patch_codec`, and everything accumulated in `transcript` so far —
/// then, when `spec.envelope.think_preseed()` is set (protocol §10, `V2`
/// and up), the literal [`THINK_PRESEED`] appended at the very end, after
/// the transcript, with nothing after it.
///
/// Under `spec.envelope.grant_line()` (envelope-v4 only, turn-4 spec §2) one
/// more thing goes in: [`grant_line`] rendered from **`spec.grant`, the very
/// grant `run_task` enforces**, placed between the goal and the verb card
/// with the same blank-line separation the card already uses. Same source of
/// truth as enforcement, so the model can never be told something the loop
/// refuses. Every earlier lens renders an empty section here and is
/// therefore byte-identical to what it rendered before v4 existed — a law,
/// not an accident: `tests/task_render_test.rs`'s goldens pin the exact
/// v1/v2/v3 bytes every G4/G5 verdict in the ledger was measured against.
///
/// The memory organ (memory-organ design spec §4) adds one more optional
/// section, `spec.memory_block`, rendered *before* the grant section and
/// governed by the same law: absent memory renders the empty string, so
/// every memory-off prompt — which is every prompt any frozen instrument
/// ever produces — is byte-identical to what it rendered before the organ
/// existed. `tests/memory_render_test.rs` pins that against a real
/// `run_task` under all four lenses.
///
/// Deliberately does no SILENT windowing or truncation. The pager's own
/// `infer` is what refuses — with arithmetic — a prompt too large for the
/// agent's measured window (its "refuse, never truncate" rule stands
/// untouched). What `rung` adds (window-ladder spec,
/// `docs/superpowers/specs/2026-08-27-window-ladder-design.md`) is the
/// CLIENT's honest response to that refusal: an explicit, fixed, journaled
/// re-scope — rung 1 is today's bytes exactly, rung 2 drops the memory
/// block, rungs 3/4 elide old entries to headers behind a pinned head note.
/// Silent truncation is still forbidden; this is neither silent (the note,
/// the `rung` field on every step row) nor heuristic (the ladder is fixed,
/// spec §2).
fn render_prompt_at_rung(
    spec: &TaskSpec,
    steps: &[TaskStepRecord],
    transcript: &str,
    rung: u32,
) -> String {
    assert!(
        (1..=MAX_RUNG).contains(&rung),
        "rung {rung} outside the fixed ladder 1..={MAX_RUNG} (spec §7: no silent clamping)"
    );
    let memory_block = if rung == 1 {
        spec.memory_block.as_deref()
    } else {
        None
    };
    let degraded;
    let transcript = match rung {
        1 | 2 => transcript,
        3 => {
            degraded = degraded_transcript(steps, 2);
            degraded.as_str()
        }
        _ => {
            degraded = degraded_transcript(steps, 1);
            degraded.as_str()
        }
    };
    render_prompt_from(
        &spec.goal,
        RenderInputs {
            patch_codec: spec.patch_codec,
            mutating_verbs: spec.mutating_verbs,
            envelope: spec.envelope,
            commands: spec.grant.commands(),
            memory_block,
        },
        transcript,
    )
}

/// Every input a rendered prompt depends on apart from the goal and the
/// transcript, bundled so [`render_prompt_from`] stays well inside clippy's
/// argument budget and so "what can change a prompt" is a list one can read
/// in one place.
struct RenderInputs<'a> {
    patch_codec: PatchCodec,
    mutating_verbs: bool,
    envelope: EnvelopeLens,
    /// The granted argv prefixes — `spec.grant.commands()` for the loop.
    /// Read only when `mutating_verbs` is set: a demoted task cannot `run`
    /// anything, so its grant line is `none` regardless (see
    /// [`render_prompt_from`]).
    commands: &'a [Vec<String>],
    /// The memory organ's already-rendered block — `spec.memory_block` for
    /// the loop, hardcoded `None` for [`render_task_prompt`]. See
    /// [`TaskSpec::memory_block`]; `None` renders nothing at all.
    memory_block: Option<&'a str>,
}

/// **The one and only prompt renderer.** Both the loop's
/// [`render_prompt_at_rung`] and the flywheel factory's
/// [`render_task_prompt`] are thin adapters over this body; there is no
/// second implementation of prompt assembly anywhere in the daemon or the
/// factory, which is the property the four anti-drift tests pin against
/// real `run_task` runs.
///
/// **Demotion wins over the grant.** A gate-G4-demoted task
/// (`mutating_verbs == false`) may not use `run` at all — [`run_task`]
/// refuses the verb structurally before `execute_action` dispatches it — so
/// the grant line renders as `none` no matter what the task's grant allows.
/// This is not cosmetic: `mutating_verbs` is fail-closed on the live HTTP
/// route (`pager::codec_gate::resolve_mutating_verbs` — an unmeasured or
/// demoted model reads `false`), so without this a v4-configured demoted
/// model handed a command-bearing grant would read `Granted commands:
/// python3 -m unittest` directly above the read-only card's `patch and run
/// are not available in this task`. Advertising a verb the loop will refuse
/// is exactly what rendering from the enforced grant exists to prevent; the
/// `none` line is the truthful statement for a task that cannot run
/// anything. Byte-neutral for every turn-4 surface, all of which are
/// `mutating_verbs: true`.
fn render_prompt_from(goal: &str, inputs: RenderInputs<'_>, transcript: &str) -> String {
    // The memory organ's section (memory-organ design spec §4): immediately
    // after the goal, before the grant section, and the EMPTY STRING when
    // absent — which is the whole of the organ's byte-identity guarantee.
    // `None` is not a special case handled elsewhere; it is the only case
    // every pre-organ surface can produce, and it must add nothing at all,
    // not even a blank line.
    let memory_section = match inputs.memory_block {
        Some(b) => format!("{b}\n\n"),
        None => String::new(),
    };
    let grant_section = if inputs.envelope.grant_line() {
        let runnable: &[Vec<String>] = if inputs.mutating_verbs {
            inputs.commands
        } else {
            &[]
        };
        format!("{}\n\n", grant_line(runnable))
    } else {
        String::new()
    };
    let prompt = format!(
        "{goal}\n\n{memory_section}{grant_section}{}\n\n{transcript}",
        verb_card_for(inputs.patch_codec, inputs.mutating_verbs)
    );
    if inputs.envelope.think_preseed() {
        format!("{prompt}{THINK_PRESEED}")
    } else {
        prompt
    }
}

/// Spec §2 rung 3/4: an elided entry is [`transcript_entry`]'s pinned shape
/// minus the content line — the record of what was done survives, the
/// re-obtainable content goes.
fn elided_entry(step: u32, verb: &str, outcome: &str) -> String {
    format!("\n[step {step} {verb}] {outcome}\n")
}

/// Spec §3: the pinned head note. Always the `{a}-{b}` form, even when
/// `a == b` — fixed format, no branching. One trailing newline; the first
/// entry's own leading newline supplies the blank line after it.
fn head_note(first_step: u32, last_step: u32) -> String {
    format!(
        "[context note: contents of steps {first_step}-{last_step} elided to fit the window; outcomes retained — re-read files if needed]\n"
    )
}

/// The rung-3/4 transcript: every entry except the last `full_window`
/// rendered elided, behind the head note — which renders ONLY when at
/// least one entry was actually elided (spec §3: absence adds nothing).
/// Rebuilt from `steps` rather than sliced out of the accumulated string;
/// [`record_step`] appends both from the same values, so the full entries
/// here are byte-identical to their accumulated originals by construction.
fn degraded_transcript(steps: &[TaskStepRecord], full_window: usize) -> String {
    let elide_end = steps.len().saturating_sub(full_window);
    let mut out = String::new();
    if elide_end > 0 {
        out.push_str(&head_note(steps[0].step, steps[elide_end - 1].step));
    }
    for (i, s) in steps.iter().enumerate() {
        if i < elide_end {
            out.push_str(&elided_entry(s.step, &s.verb, &s.outcome));
        } else {
            out.push_str(&transcript_entry(s.step, &s.verb, &s.outcome, &s.content));
        }
    }
    out
}

/// Serving-faithful prompt rendering for `flywheel-tool`
/// (`crates/bloomery-daemon/src/bin/flywheel_tool.rs`, design spec §2,
/// `docs/superpowers/specs/2026-08-16-flywheel-14b-design.md`): **this
/// function and [`render_prompt_at_rung`] MUST share one body** — the wrapper
/// constructs a minimal [`TaskSpec`] and calls the real function. No
/// second implementation of rendering, anywhere in the flywheel factory.
///
/// Only `goal`, `patch_codec`, `envelope`, the grant's `commands`, and
/// `transcript` affect the rendered text — that is exactly
/// [`RenderInputs`] plus the goal and the transcript, and this wrapper
/// hands them straight to [`render_prompt_from`], the same body
/// [`render_prompt_at_rung`] runs at rung 1. Every other `TaskSpec` field
/// (`budget_tokens`, `max_steps`, `cwd`, `bounds`, and the grant's *roots*)
/// is irrelevant to rendering and is not asked for.
///
/// `commands` became an argument with envelope-v4 (turn-4 spec §2): the
/// grant line is rendered from the real grant, so a caller that could not
/// say which commands its task grants could not render a v4 prompt at all —
/// and the factory's own trajectory requests already carry exactly this
/// field. It is passed as the raw prefixes rather than a [`Grant`] on
/// purpose: a `Grant` built here from wire input could be *invalid* (an
/// empty prefix is a `GrantError`), and a rendering function is the wrong
/// place to discover that — the tool's `Scratch::grant` is where a request's
/// commands become an enforced capability, and where a bad one becomes a
/// named error.
///
/// `mutating_verbs` is hardcoded `true`: the flywheel corpus trains the
/// read-then-patch habit, so every rendered prompt must show the model the
/// `patch` verb card, exactly like a real mutating-verbs task.
///
/// `memory_block` is hardcoded `None` and this signature did **not** change
/// when the memory organ landed (memory-organ design spec §4's envelope
/// rule: "every frozen instrument — G4/G5 batteries, drift probes, swap
/// cover — runs memory-off"). The factory renders training pairs and the
/// goldens pin measured bytes; neither may ever see an injected episode, and
/// making that unrepresentable here is cheaper than trusting every caller to
/// pass `None`. It is also what keeps this function usable as the
/// independent side of the four anti-drift byte-comparisons.
pub fn render_task_prompt(
    goal: &str,
    patch_codec: PatchCodec,
    envelope: EnvelopeLens,
    commands: &[Vec<String>],
    transcript: &str,
) -> String {
    render_prompt_from(
        goal,
        RenderInputs {
            patch_codec,
            mutating_verbs: true,
            envelope,
            commands,
            memory_block: None,
        },
        transcript,
    )
}

/// What one call to [`propose_action`] produced.
enum ProposeOutcome {
    /// A validated action, how long the successful `infer` took, and the
    /// ladder rung the prompt was sent at — 1 for every ladder-off task.
    Action(Action, u64, u32),
    /// Every attempt for this step failed to parse; the step has already
    /// been recorded as failed, and `run_task` moves on to the next step —
    /// a stuck step is not a stuck task.
    StepFailed,
    /// A hard stop: budget exhaustion, a too-large prompt (protocol
    /// Amendment 1, §9 — `WindowExhausted`, scored, not aborted), or a
    /// substrate/contract failure (`Error`). Carries the terminal status and
    /// an optional diagnostic for `TaskResult::summary`.
    Terminate(TaskStatus, Option<String>),
}

/// One step's propose-and-validate half: renders the prompt, calls
/// `pager.infer`, and decodes the reply through `parse_action_with_codec`,
/// re-asking on a parse failure up to [`MAX_PARSE_ATTEMPTS`] times total.
///
/// Every parse failure is journaled with its own diagnostic — an operator
/// debugging a stuck step needs to see what the model actually said each
/// time, not just the terminal verdict — but the final attempt's outcome is
/// overridden to the fixed, binding string `"unparseable after 2 re-asks"`
/// rather than that attempt's raw diagnostic.
fn propose_action<S: Substrate>(
    pager: &mut Pager<S>,
    agent_id: &str,
    spec: &TaskSpec,
    journal: &mut Journal,
    state: &mut TaskState,
    step: u32,
) -> ProposeOutcome {
    // Protocol §11 (Amendment 3): the substrate stop sequence applies ONLY
    // to `V3` task-loop turns — computed once per call (not per attempt)
    // since `spec.envelope` never changes across re-asks.
    let stop = spec.envelope.action_stop().then_some(ACTION_STOP);
    for attempt in 1..=MAX_PARSE_ATTEMPTS {
        // Spec §4: every attempt walks the fixed ladder from rung 1 — a
        // step-down-only ratchet can never step back up (robigo's
        // `_select_rung` lesson), and a re-walk costs at most three refused
        // pre-inference arithmetic checks. The pager is the ONLY measurer:
        // nothing here estimates tokens; a rung is rendered, submitted, and
        // the pager's accept/refuse IS the measurement. That covers both
        // refusal paths — the pre-inference window gate and a substrate-side
        // error classified `PromptTooLarge` after submission — identically:
        // a window refusal is a window refusal.
        let mut rung: u32 = 1;
        let (reply, duration_ms, sent_rung) = loop {
            let prompt = render_prompt_at_rung(spec, &state.steps, &state.transcript, rung);
            let started = Instant::now();
            match pager.infer(agent_id, &prompt, STEP_MAX_TOKENS, stop) {
                Ok(reply) => {
                    let d = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
                    break (reply, d, rung);
                }
                Err(PagerError::PromptTooLarge { .. }) if spec.window_ladder && rung < MAX_RUNG => {
                    rung += 1;
                }
                Err(e) => {
                    // Protocol Amendment 1 (docs/superpowers/evidence/
                    // 2026-08-15-g4-protocol.md §9), unchanged: ONLY
                    // `PromptTooLarge` becomes the scored `WindowExhausted`
                    // terminal (now reached at rung MAX_RUNG for a
                    // ladder-on task, at rung 1 otherwise); `Budget` stays
                    // `BudgetExhausted` at every rung; everything else stays
                    // `Error`, the infrastructure abort §3 already defines.
                    let status = match &e {
                        PagerError::Budget { .. } => TaskStatus::BudgetExhausted,
                        PagerError::PromptTooLarge { .. } => TaskStatus::WindowExhausted,
                        _ => TaskStatus::Error,
                    };
                    return ProposeOutcome::Terminate(status, Some(e.to_string()));
                }
            }
        };

        match parse_action_with_codec(&reply.text, spec.patch_codec) {
            Ok(action) => return ProposeOutcome::Action(action, duration_ms, sent_rung),
            Err(e) => {
                let final_attempt = attempt == MAX_PARSE_ATTEMPTS;
                let outcome = if final_attempt {
                    "unparseable after 2 re-asks".to_string()
                } else {
                    format!("{e:?}")
                };
                let report = StepReport {
                    verb: "?",
                    outcome: &outcome,
                    content: &outcome,
                    duration_ms,
                    failed: true,
                    args: Vec::new(),
                    rung: sent_rung,
                };
                if let Err(msg) = record_step(journal, agent_id, state, step, report) {
                    return ProposeOutcome::Terminate(TaskStatus::Error, Some(msg));
                }
                if final_attempt {
                    return ProposeOutcome::StepFailed;
                }
                // Otherwise: loop back and re-ask, now with the diagnostic
                // folded into the transcript the next prompt renders.
            }
        }
    }
    unreachable!("the loop above always returns within MAX_PARSE_ATTEMPTS iterations")
}

/// Dispatches a validated, non-`Done` action to its executor. `Done` is
/// handled by [`run_task`] itself (it terminates the task rather than
/// producing an `Observation`), so it never reaches here.
fn execute_action(
    grant: &Grant,
    cwd: &Path,
    action: &Action,
    bounds: &ExecBounds,
) -> (&'static str, Observation) {
    match action {
        Action::Read { path, lines } => ("read", exec_read(grant, cwd, path, *lines, bounds)),
        Action::Find { pattern, path } => {
            // `exec_find` takes no `cwd` (Task 1's carried obligation): a
            // relative `path` must be absolutized against the task's own
            // cwd *before* the call, or it would silently fall back to the
            // daemon process's own cwd instead.
            let abs_prefix = absolutize(cwd, path);
            (
                "find",
                exec_find(grant, pattern, &abs_prefix.to_string_lossy(), bounds),
            )
        }
        Action::Patch { path, body } => ("patch", exec_patch(grant, cwd, path, body)),
        Action::Run { argv } => ("run", exec_run(grant, cwd, argv, bounds)),
        Action::Done { .. } => unreachable!("Done is handled by run_task before dispatch"),
    }
}

/// Runs one task to completion against `pager`, from `spec.goal` to a
/// terminal [`TaskStatus`]. See this module's docs for the loop's shape.
///
/// Generic over [`Substrate`] so it's `FakeSubstrate`-tested. Journals an
/// `Event::TaskStep` for every model turn this loop takes, whether that
/// turn executed cleanly, failed a grant check, or failed to parse.
pub fn run_task<S: Substrate>(
    pager: &mut Pager<S>,
    agent_id: &str,
    spec: &TaskSpec,
    journal: &mut Journal,
) -> TaskResult {
    let mut state = TaskState {
        steps: Vec::new(),
        transcript: String::new(),
        touched: std::collections::BTreeMap::new(),
        landed: Vec::new(),
    };

    for step in 1..=spec.max_steps {
        let (action, propose_duration_ms, rung) =
            match propose_action(pager, agent_id, spec, journal, &mut state, step) {
                ProposeOutcome::Action(action, duration_ms, rung) => (action, duration_ms, rung),
                ProposeOutcome::StepFailed => continue,
                ProposeOutcome::Terminate(status, summary) => {
                    return finish(state, status, summary);
                }
            };

        if let Action::Done { summary } = &action {
            let report = StepReport {
                verb: "done",
                outcome: summary,
                content: summary,
                duration_ms: propose_duration_ms,
                failed: false,
                args: Vec::new(),
                rung,
            };
            if let Err(msg) = record_step(journal, agent_id, &mut state, step, report) {
                return finish(state, TaskStatus::Error, Some(msg));
            }
            return finish(state, TaskStatus::Done, Some(summary.clone()));
        }

        // Gate G4 structural enforcement (docs/superpowers/evidence/
        // 2026-08-15-g4-protocol.md §6: "a read-only verb card AND a
        // structural dispatch refusal — prompting alone is not
        // enforcement"). This check sits after the `Done` branch and before
        // `execute_action` on purpose: `execute_action` stays pure dispatch
        // with no gate knowledge of its own, and a demoted spec must still
        // let the model `done` at any time. A refused verb is recorded with
        // its real name (`"patch"`/`"run"`, never `"?"`), `failed: true`,
        // and the pinned outcome — then the loop CONTINUES to the next
        // step, exactly like a grant violation: a refused verb is a failed
        // step, not a dead task.
        if !spec.mutating_verbs {
            let refused_verb = match &action {
                Action::Patch { .. } => Some("patch"),
                Action::Run { .. } => Some("run"),
                _ => None,
            };
            if let Some(verb) = refused_verb {
                let report = StepReport {
                    verb,
                    outcome: MUTATING_VERB_DEMOTED,
                    content: MUTATING_VERB_DEMOTED,
                    duration_ms: propose_duration_ms,
                    failed: true,
                    args: action_args(&action),
                    rung,
                };
                if let Err(msg) = record_step(journal, agent_id, &mut state, step, report) {
                    return finish(state, TaskStatus::Error, Some(msg));
                }
                continue;
            }
        }

        let started = Instant::now();
        let (verb, obs) = execute_action(&spec.grant, &spec.cwd, &action, &spec.bounds);
        let duration_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        let report = StepReport {
            verb,
            outcome: &obs.outcome,
            content: &obs.content,
            duration_ms,
            failed: obs.failed,
            args: action_args(&action),
            rung,
        };
        if let Err(msg) = record_step(journal, agent_id, &mut state, step, report) {
            return finish(state, TaskStatus::Error, Some(msg));
        }

        // Memory-organ capture (design spec §2). Deliberately AFTER
        // `record_step`: a journal write that fails means this step was
        // not observed at all (this module's journal rule), and evidence
        // from an unobserved step is exactly what the store must not
        // contain — so that early return skips the capture rather than
        // racing it. Gated on `!obs.failed`
        // rather than on the verb: only an executor that actually achieved
        // its verb sets `obs.touched`, and only a succeeded step is
        // evidence. That gate is deliberately redundant with the executors'
        // own `touched: None` on every failure arm — belt and braces on the
        // one rule the whole store rests on ("only what has execution
        // evidence"), so neither side alone can readmit a failed step's
        // citation. `or_insert` IS the first-touch rule — the task's LATER
        // touches of a path it already touched (the read-then-patch habit
        // the flywheel trains) must not overwrite the fingerprint of the
        // bytes as they stood before the task ran, which is the only thing
        // a retrieval fingerprint gate can honestly compare an incoming
        // task's workspace against.
        if !obs.failed {
            if let Some(t) = &obs.touched {
                state
                    .touched
                    .entry(t.canonical.display().to_string())
                    .or_insert(t.pre.clone());
            }
        }

        // The landed bodies, in step order. Keyed off the *observation's*
        // canonical path, not the action's model-supplied string, so a
        // patch and the read that preceded it cite one identical path key.
        // The `obs.touched` match is what makes "landed" mean landed: a
        // patch that did not apply, did not parse, or failed its write
        // never sets it.
        if verb == "patch" && !obs.failed {
            if let (Action::Patch { body, .. }, Some(t)) = (&action, &obs.touched) {
                state
                    .landed
                    .push((t.canonical.display().to_string(), body.clone()));
            }
        }
    }

    finish(state, TaskStatus::StepsExhausted, None)
}
