//! G5 (refusal honesty): the mixed-set probe engine
//! (`docs/superpowers/evidence/2026-08-16-g5-protocol.md`).
//!
//! Split out of `mod.rs` for the same reason `boot.rs` and `pager/codec_gate.rs`
//! are split out of their own parent files: this is a distinct concern — the
//! MIXED-set (patch + refuse) probe, as opposed to `mod.rs`'s classic,
//! all-`patch` G4 engine (`run_codec_probe`) — and keeping it in its own
//! file is what keeps `mod.rs` from growing past its own house cap for a
//! reason unrelated to G4's own logic.
//!
//! **Why a sibling engine rather than a branch inside `run_codec_probe`**,
//! stated once here because it is this module's whole reason to exist: see
//! `mod.rs`'s own module doc for the full argument (`run_codec_probe`'s
//! return type is a load-bearing part of its public signature that protocol
//! §3 forbids reusing for a mixed set's two never-blended classes). This
//! module shares `mod.rs`'s per-fixture plumbing (`materialize`,
//! `fixture_grant`, `ProbeContext`, `abort`, `lock_pager`, the
//! `AGENT_REMOVED_REASON`/`CODEC_FROM_PROFILE`/`CODEC_DEFAULT` constants) by
//! reference — those items are private to `codec_probe` and this is one of
//! its child modules, so they are visible here without being made `pub` —
//! but duplicates the agent-lifecycle SHAPE (`run_one_fixture_mixed` mirrors
//! `run_one_fixture`) rather than extracting it, so `mod.rs`'s own classic
//! per-fixture function is never touched by anything in this file.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use bloomery_core::journal::Journal;
use bloomery_core::stats::wilson95;
use bloomery_substrate::Substrate;

use crate::pager::{Pager, RefusalGateResult};
use crate::task::registry::panic_message;
use crate::task::{run_task, TaskResult, TaskSpec, TaskStatus};

use super::fixtures::{Expect, Fixture, FixtureSet};
use super::scoring::{
    done_trust_from, expect_str, fixture_detail, fixture_landed, fixture_landed_refuse,
    gate_decision, is_provisional, model_dir_name,
};
use super::{
    abort, fixture_grant, lock_pager, materialize, ProbeAborted, ProbeContext,
    AGENT_REMOVED_REASON, CODEC_DEFAULT, CODEC_FROM_PROFILE, FIXTURE_BUDGET_TOKENS,
    FIXTURE_MAX_STEPS,
};

/// Runs `model` against a MIXED (G5) fixture set — one containing at least
/// one `expect = "refuse"` fixture — and returns its per-class verdict,
/// storing the gate on the pager and journaling one `CodecFixture` per
/// fixture plus exactly one `CodecVerdictMixed`. Never calls
/// [`Pager::set_codec_gate`] and never journals a classic `CodecVerdict` —
/// G5 is advisory (design doc §3) and must never touch `mutating_verbs`.
///
/// The sibling engine to `super::run_codec_probe`. Everything about
/// per-fixture isolation, locking, and infrastructure-abort handling
/// mirrors that function's (fresh scratch dir, fresh agent, fresh journal
/// handle per fixture; the pager lock held across `create_agent` +
/// `run_task` + `remove_agent` + the journal write, per fixture); what
/// differs is per-fixture scoring (branched by `fixture.expect`, see
/// [`run_one_fixture_mixed`]) and the verdict shape.
///
/// Refuses (aborts) a set with zero fixtures in EITHER class: the same
/// fail-open hazard `run_codec_probe`'s empty-set guard exists to prevent,
/// generalized per class — `gate_decision(0, 0)` is vacuously `true`, so a
/// class with no fixtures would otherwise score a vacuous keep for that
/// class alone.
pub fn run_refusal_probe<S: Substrate + Send + 'static>(
    pager: &Mutex<Pager<S>>,
    model: &str,
    set: &FixtureSet,
    scratch_dir: &Path,
) -> Result<RefusalGateResult, ProbeAborted> {
    let patch_total = set
        .fixtures
        .iter()
        .filter(|f| f.expect == Expect::Patch)
        .count();
    let refuse_total = set
        .fixtures
        .iter()
        .filter(|f| f.expect == Expect::Refuse)
        .count();
    if patch_total == 0 || refuse_total == 0 {
        return Err(abort(format!(
            "fixture set {} has {patch_total} patch and {refuse_total} refuse fixtures; a \
             mixed-set probe needs at least one of each class (never a vacuous per-class keep, \
             model unmeasured)",
            set.set
        )));
    }

    // Invariant 1 (same as `run_codec_probe`): read once, before any
    // fixture, so every fixture and the verdict describe one measurement.
    let (codec, codec_from_profile, envelope) = {
        let guard = lock_pager(pager)?;
        (
            guard.model_patch_codec(model),
            guard.model_codec_from_profile(model),
            guard.model_envelope(model),
        )
    };
    let ctx = ProbeContext {
        model,
        set_name: &set.set,
        codec,
        envelope,
    };

    let model_dir = scratch_dir.join(model_dir_name(model));
    let mut patch_landed: u32 = 0;
    let mut patch_n: u32 = 0;
    let mut refuse_landed: u32 = 0;
    let mut refuse_n: u32 = 0;
    for fixture in &set.fixtures {
        let dir = materialize(&model_dir, fixture)?;
        let landed = run_one_fixture_mixed(pager, &ctx, fixture, &dir)?;
        match fixture.expect {
            Expect::Patch => {
                patch_n += 1;
                if landed {
                    patch_landed += 1;
                }
            }
            Expect::Refuse => {
                refuse_n += 1;
                if landed {
                    refuse_landed += 1;
                }
            }
        }
    }

    let patch_interval95 = wilson95(patch_landed, patch_n);
    let refuse_interval95 = wilson95(refuse_landed, refuse_n);
    let gate = RefusalGateResult {
        fixture_set: set.set.clone(),
        codec,
        patch_landed,
        patch_n,
        patch_interval95,
        patch_provisional: is_provisional(patch_interval95.0, patch_interval95.1),
        refuse_landed,
        refuse_n,
        refuse_interval95,
        refuse_provisional: is_provisional(refuse_interval95.0, refuse_interval95.1),
        // Protocol §3: the AND of two ALREADY-decided per-class gates —
        // never a blended third count.
        done_trust: done_trust_from(
            gate_decision(patch_landed, patch_n),
            gate_decision(refuse_landed, refuse_n),
        ),
    };
    // Codec-selection provenance only (protocol §4, same two spellings G4
    // uses); the envelope lens travels structured on the event itself
    // rather than folded into this string, unlike the classic verdict.
    let detail = if codec_from_profile {
        CODEC_FROM_PROFILE
    } else {
        CODEC_DEFAULT
    };

    let mut guard = lock_pager(pager)?;
    guard
        .journal_codec_verdict_mixed(
            model,
            &gate.fixture_set,
            gate.codec,
            ctx.envelope.lens_name(),
            &gate,
            detail,
        )
        .map_err(|e| abort(format!("failed to journal the mixed codec verdict: {e}")))?;
    guard
        .set_refusal_gate(model, gate.clone())
        .map_err(|e| abort(format!("failed to store the refusal gate: {e}")))?;
    Ok(gate)
}

/// Runs one fixture inside a mixed (G5) set to a scored outcome and
/// journals its `CodecFixture` row with the fixture's real class, returning
/// whether it landed. Mirrors `super::run_one_fixture`'s agent-lifecycle
/// shape exactly (see this module's doc comment for why the two are
/// separate functions rather than one shared implementation): fresh grant,
/// one continuous pager lock across create_agent/run_task/remove_agent/
/// journal-write, the same panic-catch and Error/Running abort arm.
///
/// Scoring branches on `fixture.expect`: patch-class reads the declared
/// `target`'s before/after bytes out of the whole-dir snapshot (the exact
/// same bytes `run_one_fixture`'s `read_bytes` would read — this is
/// "patch-class = existing path untouched" applied to a mixed set) and
/// scores via [`fixture_landed`]; refuse-class scores the whole-dir
/// snapshot via [`fixture_landed_refuse`]'s trio.
fn run_one_fixture_mixed<S: Substrate + Send + 'static>(
    pager: &Mutex<Pager<S>>,
    ctx: &ProbeContext<'_>,
    fixture: &Fixture,
    dir: &Path,
) -> Result<bool, ProbeAborted> {
    // Captured BEFORE the task runs (materialize already wrote every
    // `fixture.files` entry): the whole dir's state, not just the target —
    // refuse leg (b) needs it, and patch-class reads its target's bytes
    // back out of this same snapshot below.
    let initial_files = snapshot_dir(dir, fixture)?;
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
        // Always true — same reasoning as `run_one_fixture`: G5 is advisory
        // and never demotes, so this measures the model, not a previous
        // gate's own verdict.
        mutating_verbs: true,
        bounds,
        envelope: ctx.envelope,
        // Memory-organ design spec §4's envelope rule: "every frozen
        // instrument — G4/G5 batteries, drift probes, swap cover — runs
        // memory-off". A probe rung measured with an episode in its prompt
        // would not be comparable to any rung already in the ledger.
        memory_block: None,
    };
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        run_task(&mut guard, &agent.id, &spec, &mut journal)
    }));
    let _ = guard.remove_agent(&agent.id, AGENT_REMOVED_REASON);
    let result: TaskResult = match outcome {
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
        // Same scored/abort split as `run_one_fixture` (protocol §3,
        // Amendment 1) — G5 reuses G4's terminal-status classification
        // verbatim, per class fixture.
        TaskStatus::Done
        | TaskStatus::StepsExhausted
        | TaskStatus::BudgetExhausted
        | TaskStatus::WindowExhausted => {}
        TaskStatus::Error | TaskStatus::Running => {
            return Err(abort(format!(
                "fixture {}: task ended {:?}: {}",
                fixture.name,
                result.status,
                result.summary.unwrap_or_default()
            )))
        }
    }

    let final_files = snapshot_dir(dir, fixture)?;
    let (landed, detail): (bool, String) = match fixture.expect {
        Expect::Patch => {
            let initial_target = target_bytes(&initial_files, fixture)?;
            let final_target = target_bytes(&final_files, fixture)?;
            (
                fixture_landed(&result.steps, initial_target, final_target),
                fixture_detail(&result),
            )
        }
        Expect::Refuse => {
            let (landed, detail) =
                fixture_landed_refuse(&result.steps, &result.status, &initial_files, &final_files);
            (landed, detail.to_string())
        }
    };
    let steps = u32::try_from(result.steps.len()).unwrap_or(u32::MAX);
    guard
        .journal_codec_fixture(
            ctx.model,
            ctx.set_name,
            &fixture.name,
            ctx.codec,
            landed,
            steps,
            &detail,
            expect_str(fixture.expect),
            &agent.id,
        )
        .map_err(|e| {
            abort(format!(
                "fixture {}: failed to journal the fixture outcome: {e}",
                fixture.name
            ))
        })?;
    Ok(landed)
}

/// Recursively snapshots every regular file under `dir`, as `(path relative
/// to `dir`, bytes)`, sorted by path for a deterministic comparison — G5
/// refuse-class leg (b) needs the WHOLE fixture dir's state, not just the
/// declared target: a model that mutates OR creates a sibling file while
/// leaving the target alone still fails a refusal. A read/list failure
/// aborts the probe (protocol §3): "the bytes could not be read" is not a
/// score.
fn snapshot_dir(dir: &Path, fixture: &Fixture) -> Result<Vec<(PathBuf, Vec<u8>)>, ProbeAborted> {
    let mut out = Vec::new();
    walk_dir(dir, dir, &mut out, fixture)?;
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

fn walk_dir(
    root: &Path,
    current: &Path,
    out: &mut Vec<(PathBuf, Vec<u8>)>,
    fixture: &Fixture,
) -> Result<(), ProbeAborted> {
    let entries = std::fs::read_dir(current).map_err(|e| {
        abort(format!(
            "fixture {}: failed to list {}: {e}",
            fixture.name,
            current.display()
        ))
    })?;
    for entry in entries {
        let entry = entry.map_err(|e| {
            abort(format!(
                "fixture {}: failed to read a dir entry under {}: {e}",
                fixture.name,
                current.display()
            ))
        })?;
        let path = entry.path();
        let file_type = entry.file_type().map_err(|e| {
            abort(format!(
                "fixture {}: failed to stat {}: {e}",
                fixture.name,
                path.display()
            ))
        })?;
        if file_type.is_dir() {
            walk_dir(root, &path, out, fixture)?;
        } else if file_type.is_file() {
            let bytes = std::fs::read(&path).map_err(|e| {
                abort(format!(
                    "fixture {}: failed to read {}: {e}",
                    fixture.name,
                    path.display()
                ))
            })?;
            let rel = path.strip_prefix(root).unwrap_or(&path).to_path_buf();
            out.push((rel, bytes));
        }
    }
    Ok(())
}

/// Extracts a fixture's declared `target` bytes from a whole-dir snapshot
/// ([`snapshot_dir`]) — the mixed engine's patch-class comparison reads the
/// SAME bytes `super::read_bytes` would, just sourced from the snapshot
/// already taken rather than a second read.
fn target_bytes<'a>(
    snapshot: &'a [(PathBuf, Vec<u8>)],
    fixture: &Fixture,
) -> Result<&'a [u8], ProbeAborted> {
    snapshot
        .iter()
        .find(|(p, _)| p == Path::new(&fixture.target))
        .map(|(_, bytes)| bytes.as_slice())
        .ok_or_else(|| {
            abort(format!(
                "fixture {}: target {} missing from the materialized dir",
                fixture.name, fixture.target
            ))
        })
}
