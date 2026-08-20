//! Boot-time wiring decisions for the G4 codec probe (Phase 2b/2c P4
//! Task 10).
//!
//! Split out of `mod.rs` for the same reason `pager/codec_gate.rs` and
//! `pager/status.rs` are split out of `pager.rs`: this is a distinct
//! concern — *when* the probe runs at boot, and what gets journaled when it
//! doesn't — from the probe engine itself ([`super::run_codec_probe`],
//! Task 9). `main.rs` stays thin glue: it computes `config.assay.enabled`
//! and `config.tasks_enabled`, calls [`should_run_codec_probe`] to decide,
//! and — inside the POST thread, strictly after `run_post` returns `Ok` —
//! calls [`run_boot_codec_probe`] to actually run it. The boolean and every
//! string the boot decision table can journal live here instead, so they
//! are pinned by `tests/codec_probe_test.rs` with no GPU, the same way
//! `post::argv` keeps POST's own invocation testable without spawning
//! assay — see that module's docs.
//!
//! The decision table itself (Task 10 brief, verbatim):
//! - `assay.enabled && tasks_enabled` → the probe runs, inside the POST
//!   thread, strictly after `run_post` returns `Ok`.
//! - `run_post` returns `Err` → the probe does not run at all (the daemon
//!   is already degraded-loudly; `main.rs` never calls into this module).
//! - The shipped fixture set fails to parse (a daemon build bug) →
//!   [`fixture_set_unparseable_reason`], no probe for any model.
//! - A model's probe returns `ProbeAborted` → [`probe_aborted_reason`],
//!   then the loop continues to the next model.
//! - `tasks_enabled && !assay.enabled` → one
//!   [`POST_DISABLED_CODEC_SKIP_REASON`] line beside the existing
//!   "POST disabled by config" line.
//! - `!tasks_enabled` → no probe, no journal line: the task surface is
//!   dark, and `/status`'s `mutating_verbs: false` plus `codec_gate: null`
//!   already tell the truth.

use std::path::Path;
use std::sync::Mutex;

use bloomery_substrate::Substrate;

use super::fixtures::{
    shipped_fixture_set, shipped_fixture_set_v3_mixed, V3_MIXED_PLACEHOLDER_SET_NAME,
};
use super::{run_codec_probe, run_refusal_probe, ProbeAborted};
use crate::pager::{Pager, PagerError};

/// The decision table's first line, as a pure boolean: the probe runs only
/// when POST itself ran (a profile might exist for it to read a codec
/// from) **and** the task surface is on (there is a mutating verb worth
/// gating in the first place). Neither condition alone is enough — gating
/// a surface that doesn't exist yet, or measuring a codec with no serving
/// window to measure it against, are both non-events.
pub fn should_run_codec_probe(assay_enabled: bool, tasks_enabled: bool) -> bool {
    assay_enabled && tasks_enabled
}

/// Journaled once, beside the existing `"POST disabled by config"` line,
/// when `tasks_enabled` is true but `assay.enabled` is false: the codec
/// probe measures the codec POST would have attached a profile for, and
/// there is no serving window for it to run against either, so every model
/// stays unmeasured for the mutating-verb gate too — one more line, not
/// silence, because the operator turned the task surface on.
pub const POST_DISABLED_CODEC_SKIP_REASON: &str =
    "codec probe skipped: POST disabled; all models unmeasured — mutating verbs refused";

/// Journaled when the fixture set frozen into the binary fails to parse —
/// a daemon build bug, not an operator mistake or a per-model measurement,
/// so it is recorded once (never per model) and names the parse error
/// verbatim.
pub fn fixture_set_unparseable_reason(err: &str) -> String {
    format!(
        "codec fixture set unparseable: {err}; codec probe skipped — mutating verbs stay refused"
    )
}

/// Journaled per model on a [`ProbeAborted`] (protocol §3's infrastructure
/// failure) — names the model that aborted, never every model still to be
/// probed, and carries `reason` (already naming the fixture it happened on)
/// verbatim rather than reformatting it.
pub fn probe_aborted_reason(model: &str, reason: &str) -> String {
    format!("codec probe aborted for {model}: {reason}; unmeasured — mutating verbs refused")
}

/// Runs the G4 codec probe for every model in `models`, in order, strictly
/// after `run_post` has returned `Ok` (profiles attached, `posting`
/// cleared) — the caller enforces that ordering; this function does not
/// re-check it.
///
/// The fixture set is parsed exactly once, before any model runs: a parse
/// failure is a daemon build bug, not a per-model measurement, so it is
/// journaled once ([`fixture_set_unparseable_reason`]) and the probe is
/// skipped entirely rather than retried per model.
///
/// Mirrors `post::probe_each`'s per-model isolation: a model's
/// [`ProbeAborted`] is journaled `Degraded` with [`probe_aborted_reason`]
/// and the loop moves to the next model — one model's abort never stops
/// another's probe, and never retracts a verdict already recorded.
///
/// Returns `Err` only when the *journal itself* fails to record an
/// outcome — the one failure that cannot itself be journaled. The caller
/// (mirroring `run_post`'s own caller in `main.rs`) reports it with
/// `eprintln!` and stops; every other outcome above is recorded in the
/// journal and this returns `Ok`.
pub fn run_boot_codec_probe<S: Substrate + Send + 'static>(
    pager: &Mutex<Pager<S>>,
    models: &[String],
    scratch_dir: &Path,
) -> Result<(), PagerError> {
    let set = match shipped_fixture_set() {
        Ok(set) => set,
        Err(e) => return journal_degraded(pager, fixture_set_unparseable_reason(&e)),
    };
    for model in models {
        if let Err(ProbeAborted { reason }) = run_codec_probe(pager, model, &set, scratch_dir) {
            journal_degraded(pager, probe_aborted_reason(model, &reason))?;
        }
    }
    Ok(())
}

/// Locks the pager for one short critical section to record a boot-time
/// degradation — same poisoned-lock-to-named-error conversion as
/// `post::with_pager`, so a poisoned pager reports a `PagerError` instead
/// of panicking inside the boot worker.
fn journal_degraded<S: Substrate>(
    pager: &Mutex<Pager<S>>,
    reason: String,
) -> Result<(), PagerError> {
    let mut guard = pager.lock().map_err(|_| {
        PagerError::Substrate(
            "pager state poisoned by a prior panic; codec probe cannot record its result"
                .to_string(),
        )
    })?;
    guard.journal_degraded(reason)
}

// ---------------------------------------------------------------------------
// G5 (refusal honesty) boot wiring — same decision table shape as G4 above,
// per-model opt-in (`config.g5_probe`) layered on top, run strictly after
// G4 at boot (protocol §1: "the same probe path G4 uses ... AFTER the G4
// probe completes"). `main.rs` computes `g5_models` (every configured model
// with `g5_probe = true`, per `ModelSpec::g5_probe`) the same way it
// already computes G4's `models` list, and calls this only when
// `should_run_codec_probe` (the SAME decision table G4 uses) says the
// serving window and task surface are both up.
// ---------------------------------------------------------------------------

/// Journaled once, beside [`POST_DISABLED_CODEC_SKIP_REASON`], when
/// `tasks_enabled` is true, `assay.enabled` is false, AND at least one
/// model has `g5_probe = true`: G5 needs the same serving window POST does
/// (protocol §1: "the same probe path G4 uses"), so an opted-in model stays
/// unmeasured for `done_trust` too. Gated on "at least one model opted in"
/// (unlike the unconditional G4 line) because G5 is per-model opt-in — a
/// daemon with no `g5_probe` model configured at all should never suggest
/// an operator asked for G5.
pub const G5_POST_DISABLED_SKIP_REASON: &str =
    "G5 refusal probe skipped: POST disabled; opted-in models unmeasured — done_trust stays \
     unmeasured";

/// Journaled once when the embedded G5 mixed set is still a placeholder
/// (its `set` field carries a `-PLACEHOLDER` suffix) — a placeholder must
/// never take a measurement, so every opted-in model is skipped rather
/// than scored against fixtures that are not the real instrument yet.
/// Wording is deliberately era-independent (no task number, no specific
/// gate-set name baked in beyond `set_name` itself): this helper outlives
/// any one gate-set generation's placeholder period — a wording tied to
/// "the current placeholder" (e.g. naming which real set is still
/// pending) goes stale the moment that generation freezes and the next
/// one's placeholder era begins, without this function itself changing.
pub fn g5_placeholder_skip_reason(set_name: &str) -> String {
    format!(
        "G5 refusal probe skipped: fixture set {set_name} is a placeholder, not the frozen \
         instrument; no model measured — done_trust stays unmeasured"
    )
}

/// Journaled per model on a [`ProbeAborted`] from the G5 (refusal) probe —
/// same shape as [`probe_aborted_reason`], named separately so a journal
/// reader can tell a G4 abort (which leaves `mutating_verbs` refused) from
/// a G5 one (which only leaves `done_trust` unmeasured — G5 is advisory)
/// without parsing free text.
pub fn g5_probe_aborted_reason(model: &str, reason: &str) -> String {
    format!("G5 refusal probe aborted for {model}: {reason}; done_trust unmeasured")
}

/// Runs the G5 (refusal-honesty) probe for every model in `g5_models`
/// (already filtered by the caller to `g5_probe = true` —
/// `crate::config::ModelSpec::g5_probe`), against the embedded mixed set.
///
/// The fixture set is parsed exactly once, before any model runs — same
/// reasoning as [`run_boot_codec_probe`]'s own G4 parse. A still-placeholder
/// set ([`V3_MIXED_PLACEHOLDER_SET_NAME`]) or an unparseable one is a
/// build-time/authoring concern, not a per-model measurement, so it is
/// journaled once and every opted-in model is skipped entirely — never
/// scored against fixtures that are not the real instrument.
///
/// G5 is advisory (design doc §3): unlike [`run_boot_codec_probe`], an
/// abort here never touches `mutating_verbs` — it only leaves `done_trust`
/// unmeasured for that model. An empty `g5_models` is a silent no-op (no
/// journal line): no model opted in, so there is nothing to skip or run.
pub fn run_boot_g5_probe<S: Substrate + Send + 'static>(
    pager: &Mutex<Pager<S>>,
    g5_models: &[String],
    scratch_dir: &Path,
) -> Result<(), PagerError> {
    if g5_models.is_empty() {
        return Ok(());
    }
    let set = match shipped_fixture_set_v3_mixed() {
        Ok(set) => set,
        Err(e) => return journal_degraded(pager, fixture_set_unparseable_reason(&e)),
    };
    if set.set == V3_MIXED_PLACEHOLDER_SET_NAME {
        return journal_degraded(pager, g5_placeholder_skip_reason(&set.set));
    }
    // A distinct scratch subtree from G4's: the two engines' fixture NAMES
    // are never guaranteed disjoint (different fixture sets, authored
    // independently), and `materialize`'s directory is keyed by fixture
    // name alone under the model's dir — nesting under `g5/` keeps a G5
    // fixture from ever colliding with a same-named G4 one.
    let g5_scratch = scratch_dir.join("g5");
    for model in g5_models {
        if let Err(ProbeAborted { reason }) = run_refusal_probe(pager, model, &set, &g5_scratch) {
            journal_degraded(pager, g5_probe_aborted_reason(model, &reason))?;
        }
    }
    Ok(())
}
