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

use bloomery_core::action::{parse_action_with_codec, verb_card_for, Action, PatchCodec};
use bloomery_core::grant::Grant;
use bloomery_core::journal::{Event, Journal};
use bloomery_substrate::Substrate;

use crate::pager::{Pager, PagerError};
use crate::task::exec::absolutize;
use crate::task::{exec_find, exec_patch, exec_read, exec_run, ExecBounds, Observation};

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
    /// `render_prompt` reflects this in the verb card the model sees
    /// (`verb_card_for(spec.patch_codec, spec.mutating_verbs)`), and
    /// `run_task` enforces it structurally — a `patch`/`run` action is
    /// refused before `execute_action` ever dispatches it, regardless of
    /// what the card told the model. This task (P4 Task 7) always sets it
    /// `true`; a later task (Task 8) wires the real per-model value decided
    /// by the gate at agent-admission time.
    pub mutating_verbs: bool,
    /// Selects the envelope-v2 (think-preseeded) lens
    /// (`docs/superpowers/evidence/2026-08-15-g4-protocol.md` §10, Amendment
    /// 2): when `true`, [`render_prompt`] appends the literal
    /// [`THINK_PRESEED`] pre-seed at the very end of the rendered prompt.
    /// `false` renders exactly envelope-v1's prompt, byte-for-byte.
    pub think_preseed: bool,
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
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct TaskResult {
    pub status: TaskStatus,
    pub steps: Vec<TaskStepRecord>,
    pub summary: Option<String>,
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
}

/// One journaled-and-recorded step's content, bundled for the same
/// too-many-arguments reason as [`TaskState`].
struct StepReport<'a> {
    verb: &'a str,
    outcome: &'a str,
    content: &'a str,
    duration_ms: u64,
    failed: bool,
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
        })
        .map_err(|e| format!("journal write failed: {e}"))?;
    state.steps.push(TaskStepRecord {
        step,
        verb: report.verb.to_string(),
        outcome: report.outcome.to_string(),
        content: report.content.to_string(),
        failed: report.failed,
    });
    state.transcript.push_str(&format!(
        "\n[step {step} {}] {}\n{}\n",
        report.verb, report.outcome, report.content
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

/// Builds the prompt for one model turn: the task's goal, the verb card for
/// `spec.patch_codec`, and everything accumulated in `transcript` so far —
/// then, when `spec.think_preseed` is set (protocol §10, Amendment 2), the
/// literal [`THINK_PRESEED`] appended at the very end, after the transcript,
/// with nothing after it.
///
/// Deliberately does no windowing or truncation of its own. The pager's own
/// `infer` is what refuses — with arithmetic — a prompt too large for the
/// agent's measured window, and [`propose_action`] turns that refusal into
/// `TaskStatus::Error`. Truncating here instead would silently drop context
/// the model needs, exactly what the pager's "refuse, never truncate" rule
/// (see `pager.rs`'s module docs) forbids applying to this loop's own
/// prompt.
fn render_prompt(spec: &TaskSpec, transcript: &str) -> String {
    let prompt = format!(
        "{}\n\n{}\n\n{transcript}",
        spec.goal,
        verb_card_for(spec.patch_codec, spec.mutating_verbs)
    );
    if spec.think_preseed {
        format!("{prompt}{THINK_PRESEED}")
    } else {
        prompt
    }
}

/// What one call to [`propose_action`] produced.
enum ProposeOutcome {
    /// A validated action, and how long the successful `infer` call took.
    Action(Action, u64),
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
    for attempt in 1..=MAX_PARSE_ATTEMPTS {
        let prompt = render_prompt(spec, &state.transcript);
        let started = Instant::now();
        let reply = match pager.infer(agent_id, &prompt, STEP_MAX_TOKENS) {
            Ok(reply) => reply,
            Err(e) => {
                // Protocol Amendment 1 (docs/superpowers/evidence/
                // 2026-08-15-g4-protocol.md §9): ONLY `PromptTooLarge`
                // becomes the scored `WindowExhausted` terminal — every
                // other `infer` failure (substrate faults, journal
                // failures, contract violations, unknown agent) stays
                // `Error`, the infrastructure abort §3 already defines.
                let status = match &e {
                    PagerError::Budget { .. } => TaskStatus::BudgetExhausted,
                    PagerError::PromptTooLarge { .. } => TaskStatus::WindowExhausted,
                    _ => TaskStatus::Error,
                };
                return ProposeOutcome::Terminate(status, Some(e.to_string()));
            }
        };
        let duration_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);

        match parse_action_with_codec(&reply.text, spec.patch_codec) {
            Ok(action) => return ProposeOutcome::Action(action, duration_ms),
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
    };

    for step in 1..=spec.max_steps {
        let (action, propose_duration_ms) =
            match propose_action(pager, agent_id, spec, journal, &mut state, step) {
                ProposeOutcome::Action(action, duration_ms) => (action, duration_ms),
                ProposeOutcome::StepFailed => continue,
                ProposeOutcome::Terminate(status, summary) => {
                    return TaskResult {
                        status,
                        steps: state.steps,
                        summary,
                    };
                }
            };

        if let Action::Done { summary } = &action {
            let report = StepReport {
                verb: "done",
                outcome: summary,
                content: summary,
                duration_ms: propose_duration_ms,
                failed: false,
            };
            if let Err(msg) = record_step(journal, agent_id, &mut state, step, report) {
                return TaskResult {
                    status: TaskStatus::Error,
                    steps: state.steps,
                    summary: Some(msg),
                };
            }
            return TaskResult {
                status: TaskStatus::Done,
                steps: state.steps,
                summary: Some(summary.clone()),
            };
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
                };
                if let Err(msg) = record_step(journal, agent_id, &mut state, step, report) {
                    return TaskResult {
                        status: TaskStatus::Error,
                        steps: state.steps,
                        summary: Some(msg),
                    };
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
        };
        if let Err(msg) = record_step(journal, agent_id, &mut state, step, report) {
            return TaskResult {
                status: TaskStatus::Error,
                steps: state.steps,
                summary: Some(msg),
            };
        }
    }

    TaskResult {
        status: TaskStatus::StepsExhausted,
        steps: state.steps,
        summary: None,
    }
}
