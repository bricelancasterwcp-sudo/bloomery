//! The G4 codec-landing gate (Phase 2b/2c P4).
//!
//! Governing doc: `docs/superpowers/evidence/2026-08-15-g4-protocol.md`, a
//! pre-registered measurement protocol written before this module existed.
//! Per that protocol's §2 ("Instrument"), the gate measures whether each
//! configured model's chosen patch codec **lands** a small, frozen set of
//! single-defect repair tasks through this daemon's own serving path — not
//! whether the model can "solve" anything more general.
//!
//! Three pieces live here. [`fixtures`] (Task 5) ships the frozen set itself
//! (`codec-tasks-v1`, N=20) and its parser, every fixture's reference fix
//! proven to land through the real `bloomery_core::action::lens::land` path
//! by `tests/codec_fixtures_test.rs`. [`run_codec_probe`] (Task 9) is the
//! instrument: it runs one model against a set through the *real* `run_task`
//! loop — real prompts, real envelope decoding, real executors, real grants
//! — scores each fixture by §3's rule, and turns the result into one
//! [`CodecGateResult`] the pager stores and `/status` renders. `boot`
//! (Task 10) decides *when* the instrument runs at boot and what gets
//! journaled when it doesn't — see its own module docs for the decision
//! table.
//!
//! **The measurement-honesty rules this module is responsible for**, stated
//! once here because they are what the whole sub-phase rests on:
//! - **The scoring conjunction (§3).** A fixture lands iff a `patch` step
//!   succeeded AND the *declared target file's* bytes changed. Either leg
//!   alone scores non-repairs as repairs — see [`scoring::fixture_landed`].
//! - **Infrastructure failure is never a fixture failure (§3).** A
//!   `TaskStatus::Error`, a refused `create_agent`, a poisoned pager lock, an
//!   unwritable journal, a panicking task: each aborts the *whole* probe with
//!   [`ProbeAborted`]. No `CodecVerdict` is journaled, no gate is stored, no
//!   partial score is spliced. The model stays **unmeasured**, which reads
//!   fail-closed at dispatch (`Pager::model_mutating_verbs`) and is never a
//!   confident zero. **Amendment 1 (§9, 2026-08-15)** carves out exactly one
//!   exception: a mid-task `PagerError::PromptTooLarge` from `infer` — the
//!   model's measured context window filling before it finished — is a
//!   **scored** terminal (`TaskStatus::WindowExhausted`), joining
//!   `Done`/`StepsExhausted`/`BudgetExhausted` in [`run_one_fixture`]'s
//!   scored arm, because it is the same shape as `BudgetExhausted`: an
//!   envelope-bounded resource ran out, not the substrate. Every other
//!   `infer` failure still aborts as `Error`.
//! - **The point estimate decides (§5).** `gate_decision`'s integer form has
//!   no float edge; the Wilson interval is recorded with every verdict and
//!   `is_provisional` marks a straddling one, but never changes it.
//! - **`CodecFixture` rows are a rate ONLY under a matching `CodecVerdict`.**
//!   Each row is journaled as its fixture finishes, so an abort partway
//!   through a set leaves the rows for the fixtures that already ran —
//!   permanently, because the journal is append-only and cannot retract them.
//!   Those orphans are *diagnostic records of what ran*, *not* a partial
//!   measurement: the probe never scored them, and the only marker separating
//!   them from a completed probe is the **absence of a `CodecVerdict` for that
//!   model and set**. Anyone replaying the journal — an operator, a later
//!   analyst, a downstream tool — must therefore read a landing rate only from
//!   rows bounded by a verdict, and must never hand-sum orphan rows into a
//!   score. Splicing a partial score is precisely what §3 forbids, and doing it
//!   at read time is the same violation as doing it at write time.

mod boot;
pub mod fixtures;
mod scoring;

pub use boot::{
    fixture_set_unparseable_reason, probe_aborted_reason, run_boot_codec_probe,
    should_run_codec_probe, POST_DISABLED_CODEC_SKIP_REASON,
};

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use bloomery_core::action::PatchCodec;
use bloomery_core::grant::Grant;
use bloomery_core::journal::Journal;
use bloomery_core::stats::wilson95;
use bloomery_substrate::Substrate;

use crate::pager::{CodecGateResult, Pager};
use crate::task::registry::panic_message;
use crate::task::{run_task, TaskSpec, TaskStatus};
use fixtures::{Fixture, FixtureSet};
use scoring::{fixture_detail, fixture_landed, model_dir_name};
pub use scoring::{gate_decision, is_provisional};

/// One model's probe stopped on an infrastructure failure (protocol §3), not
/// on anything the model did. The distinction is load-bearing: a scored
/// fixture failure lowers a landing rate, this ends the measurement
/// entirely. `reason` always names the fixture it happened on, so an
/// operator reading Task 10's `Degraded` line can go straight to the scratch
/// dir that is still on disk.
#[derive(Debug, Clone)]
pub struct ProbeAborted {
    pub reason: String,
}

/// Per-fixture token budget (protocol §2, *chosen+sanity*: a backstop of
/// ≈ 6 steps × (prompt ≲4k + completion 1024); [`FIXTURE_MAX_STEPS`] is the
/// real bound). Also the ephemeral agent's own pager-level budget, so a
/// runaway fixture is refused by the pager rather than by nothing.
pub const FIXTURE_BUDGET_TOKENS: u64 = 30_000;

/// Per-fixture step cap (protocol §2, *chosen+sanity*: 2× the expected
/// read→patch→done path).
pub const FIXTURE_MAX_STEPS: u32 = 6;

/// The envelope the gate measures landing *under* — named in every verdict's
/// `detail` because "this model's codec lands 85%" is meaningless without
/// the envelope it landed through (protocol §2).
pub const ENVELOPE_LENS: &str = "bloomery-task-envelope-v1";

/// Journaled on every ephemeral probe agent's removal.
const AGENT_REMOVED_REASON: &str = "codec probe fixture complete";

/// Protocol §4's two provenance spellings, recorded in the verdict's
/// `detail`: a `SearchReplace` verdict means something quite different when
/// the profile measured it than when it is the untested fallback.
const CODEC_FROM_PROFILE: &str = "codec from profile";
const CODEC_DEFAULT: &str = "default (codecs unmeasured)";

fn abort(reason: impl Into<String>) -> ProbeAborted {
    ProbeAborted {
        reason: reason.into(),
    }
}

/// Locks the pager, refusing on poison rather than recovering with
/// `into_inner`. Same reasoning as `api_native::lock_pager` and
/// `task::registry`'s worker: a poisoned pager's in-memory state is not
/// vouched for, and a measurement taken against un-vouched-for state is
/// worse than no measurement — so this becomes an abort, and the model stays
/// unmeasured.
fn lock_pager<S: Substrate>(
    pager: &Mutex<Pager<S>>,
) -> Result<MutexGuard<'_, Pager<S>>, ProbeAborted> {
    pager.lock().map_err(|_| {
        abort("pager state poisoned by a prior panic; codec probe abandoned (model unmeasured)")
    })
}

/// Everything constant across one model's whole probe — read once, before
/// any fixture runs (invariant 1), and threaded to each fixture from there.
struct ProbeContext<'a> {
    model: &'a str,
    set_name: &'a str,
    codec: PatchCodec,
}

/// Runs `model` against `set` and returns its G4 verdict, storing the gate on
/// the pager and journaling one `CodecFixture` per fixture plus exactly one
/// `CodecVerdict`.
///
/// Per fixture, in set order: a fresh scratch dir under
/// `scratch_dir/<model>/<fixture>` (removed first if present, so a boot is
/// deterministic; left in place afterwards for inspection), a fresh
/// ephemeral agent, a fresh `Journal` handle, and a grant covering exactly
/// that one directory with no commands and no network.
///
/// **Locking.** The pager lock is held across `create_agent` + `run_task` +
/// `remove_agent` + the fixture's journal write, per fixture — the same
/// whole-task lock `task::registry` ratified (see its module docs for why
/// `run_task`'s `&mut Pager` signature leaves no finer choice). Holding it
/// across the whole fixture is also what makes the own-`Journal`-handle
/// pattern safe here: with the lock held there is never a second concurrent
/// writer on `tasks.jsonl` to interleave with.
///
/// Returns `Err(ProbeAborted)` on any infrastructure failure — see the
/// module docs; the caller (Task 10) journals it `Degraded` and moves to the
/// next model.
pub fn run_codec_probe<S: Substrate + Send + 'static>(
    pager: &Mutex<Pager<S>>,
    model: &str,
    set: &FixtureSet,
    scratch_dir: &Path,
) -> Result<CodecGateResult, ProbeAborted> {
    // A set with nothing to run is a broken instrument, not a 0-of-0 keep:
    // `gate_decision(0, 0)` is vacuously true, so scoring an empty set would
    // hand out mutating verbs on zero evidence — precisely the fail-open
    // this gate exists to prevent.
    if set.fixtures.is_empty() {
        return Err(abort(format!(
            "fixture set {} has no fixtures; nothing to measure (model unmeasured)",
            set.set
        )));
    }

    // Invariant 1: the codec (and its provenance) is read ONCE, before any
    // fixture, so every TaskSpec, every CodecFixture event, and the verdict
    // all describe the same measurement even if a profile were attached
    // mid-probe.
    let (codec, codec_from_profile) = {
        let guard = lock_pager(pager)?;
        (
            guard.model_patch_codec(model),
            guard.model_codec_from_profile(model),
        )
    };
    let ctx = ProbeContext {
        model,
        set_name: &set.set,
        codec,
    };

    let model_dir = scratch_dir.join(model_dir_name(model));
    let mut landed: u32 = 0;
    let mut n: u32 = 0;
    for fixture in &set.fixtures {
        let dir = materialize(&model_dir, fixture)?;
        let initial = read_bytes(&dir.join(&fixture.target), fixture)?;
        if run_one_fixture(pager, &ctx, fixture, &dir, &initial)? {
            landed += 1;
        }
        n += 1;
    }

    let interval95 = wilson95(landed, n);
    let gate = CodecGateResult {
        fixture_set: set.set.clone(),
        codec,
        landed,
        n,
        interval95,
        provisional: is_provisional(interval95.0, interval95.1),
        mutating_verbs: gate_decision(landed, n),
    };
    let detail = format!(
        "applies_and_parses under {ENVELOPE_LENS}; {}",
        if codec_from_profile {
            CODEC_FROM_PROFILE
        } else {
            CODEC_DEFAULT
        }
    );

    let mut guard = lock_pager(pager)?;
    // Journal first, store second: an unrecordable verdict must not become
    // an enforced-but-unobserved policy change (`pager.rs` rule 4).
    guard
        .journal_codec_verdict(
            model,
            &gate.fixture_set,
            gate.codec,
            gate.landed,
            gate.n,
            gate.interval95,
            gate.provisional,
            gate.mutating_verbs,
            &detail,
        )
        .map_err(|e| abort(format!("failed to journal the codec verdict: {e}")))?;
    guard
        .set_codec_gate(model, gate.clone())
        .map_err(|e| abort(format!("failed to store the codec gate: {e}")))?;
    Ok(gate)
}

/// Runs one fixture to a scored outcome and journals its `CodecFixture`
/// event, returning whether it landed. Every failure path here is
/// infrastructure (protocol §3) and aborts the whole probe.
fn run_one_fixture<S: Substrate + Send + 'static>(
    pager: &Mutex<Pager<S>>,
    ctx: &ProbeContext<'_>,
    fixture: &Fixture,
    dir: &Path,
    initial: &[u8],
) -> Result<bool, ProbeAborted> {
    let grant = fixture_grant(dir, fixture)?;
    let mut guard = lock_pager(pager)?;

    let priority = guard.default_priority();
    let bounds = guard.exec_bounds();
    let journal_path = guard.task_journal_path().to_path_buf();
    let agent = guard
        .create_agent(ctx.model, priority, None, FIXTURE_BUDGET_TOKENS)
        .map_err(|e| {
            abort(format!(
                "fixture {}: agent creation refused: {e}",
                fixture.name
            ))
        })?;

    let mut journal = match Journal::open(&journal_path) {
        Ok(j) => j,
        Err(e) => {
            let _ = guard.remove_agent(&agent.id, AGENT_REMOVED_REASON);
            return Err(abort(format!(
                "fixture {}: failed to open the task journal {}: {e}",
                fixture.name,
                journal_path.display()
            )));
        }
    };

    let spec = TaskSpec {
        goal: fixture.goal.clone(),
        grant,
        budget_tokens: FIXTURE_BUDGET_TOKENS,
        max_steps: FIXTURE_MAX_STEPS,
        cwd: dir.to_path_buf(),
        patch_codec: ctx.codec,
        // Always true: the probe measures whether mutating verbs *should* be
        // granted, so running it under a demoted card would measure this
        // gate's own previous verdict instead of the model.
        mutating_verbs: true,
        bounds,
    };
    // Caught inside the locked scope for exactly the reason
    // `task::registry`'s module docs give: a panic unwinding past a live
    // `MutexGuard` poisons the shared pager mutex and degrades every other
    // request on the daemon. A caught panic ends this probe as an
    // infrastructure abort (no verdict, unmeasured) instead.
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        run_task(&mut guard, &agent.id, &spec, &mut journal)
    }));
    let _ = guard.remove_agent(&agent.id, AGENT_REMOVED_REASON);
    let result = match outcome {
        Ok(result) => result,
        Err(payload) => {
            return Err(abort(format!(
                "fixture {}: {}",
                fixture.name,
                panic_message(payload.as_ref())
            )))
        }
    };

    match result.status {
        // Protocol §3, as amended by §9 (Amendment 1, 2026-08-15): all four
        // terminal outcomes are scored. `WindowExhausted` — a mid-fixture
        // `PagerError::PromptTooLarge` — joined this arm by amendment: the
        // model exhausted a measured, envelope-bounded resource (its
        // context window) without landing, exactly the shape
        // `BudgetExhausted` already scores; it is not an infrastructure
        // failure.
        TaskStatus::Done
        | TaskStatus::StepsExhausted
        | TaskStatus::BudgetExhausted
        | TaskStatus::WindowExhausted => {}
        // `Error` is §3's infrastructure abort — ONLY a substrate fault,
        // journal failure, or agent-creation refusal, never a window
        // exhaustion since Amendment 1. `Running` is unreachable from
        // `run_task`, and is treated the same way rather than scored: an
        // unfinished task has no honest score.
        TaskStatus::Error | TaskStatus::Running => {
            return Err(abort(format!(
                "fixture {}: task ended {:?}: {}",
                fixture.name,
                result.status,
                result.summary.unwrap_or_default()
            )))
        }
    }

    let final_bytes = read_bytes(&dir.join(&fixture.target), fixture)?;
    let landed = fixture_landed(&result.steps, initial, &final_bytes);
    let steps = u32::try_from(result.steps.len()).unwrap_or(u32::MAX);
    guard
        .journal_codec_fixture(
            ctx.model,
            ctx.set_name,
            &fixture.name,
            ctx.codec,
            landed,
            steps,
            &fixture_detail(&result),
        )
        .map_err(|e| {
            abort(format!(
                "fixture {}: failed to journal the fixture outcome: {e}",
                fixture.name
            ))
        })?;
    Ok(landed)
}

/// Writes `fixture`'s files into `model_dir/<fixture.name>/`, removing any
/// previous copy first (invariant 2: deterministic per boot), and returns
/// the canonicalized directory — canonical because it becomes both the
/// grant's only root and the task's `cwd`, and the executors compare
/// canonicalized paths.
fn materialize(model_dir: &Path, fixture: &Fixture) -> Result<PathBuf, ProbeAborted> {
    let dir = model_dir.join(&fixture.name);
    let io_err = |what: &str, e: std::io::Error| {
        abort(format!(
            "fixture {}: failed to {what} {}: {e}",
            fixture.name,
            dir.display()
        ))
    };
    if dir.exists() {
        std::fs::remove_dir_all(&dir).map_err(|e| io_err("remove the stale scratch dir", e))?;
    }
    std::fs::create_dir_all(&dir).map_err(|e| io_err("create the scratch dir", e))?;
    for file in &fixture.files {
        let path = dir.join(&file.path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| io_err("create a file's parent dir", e))?;
        }
        std::fs::write(&path, &file.contents).map_err(|e| {
            abort(format!(
                "fixture {}: failed to write {}: {e}",
                fixture.name,
                path.display()
            ))
        })?;
    }
    std::fs::canonicalize(&dir).map_err(|e| io_err("canonicalize the scratch dir", e))
}

/// The fixture's whole capability boundary (protocol §2): read+write on its
/// own scratch dir, **no** commands, no network. Built through
/// `Grant::from_json` because that is the type's only construction path, and
/// it validates.
fn fixture_grant(dir: &Path, fixture: &Fixture) -> Result<Grant, ProbeAborted> {
    let dir = dir.to_str().ok_or_else(|| {
        abort(format!(
            "fixture {}: scratch dir {} is not valid UTF-8",
            fixture.name,
            dir.display()
        ))
    })?;
    let json = serde_json::json!({
        "read_roots": [dir],
        "write_roots": [dir],
        "commands": [],
        "network": false,
    })
    .to_string();
    Grant::from_json(&json).map_err(|e| {
        abort(format!(
            "fixture {}: could not build its grant: {e}",
            fixture.name
        ))
    })
}

/// Reads a target file's bytes for the §3 scoring comparison. A target that
/// cannot be read is an infrastructure abort, never a score: "the bytes did
/// not change" and "the bytes could not be read" are different claims, and
/// only one of them is a measurement.
fn read_bytes(path: &Path, fixture: &Fixture) -> Result<Vec<u8>, ProbeAborted> {
    std::fs::read(path).map_err(|e| {
        abort(format!(
            "fixture {}: failed to read its target {}: {e}",
            fixture.name,
            path.display()
        ))
    })
}
