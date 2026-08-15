//! The G4 scoring, decision, and provisional rules as pure functions
//! (Phase 2b/2c P4 Task 9).
//!
//! Split out of `mod.rs` because these four expressions *are* the
//! measurement — everything around them is plumbing. Kept free of I/O, the
//! pager, and the task loop so each rule can be tested against literal
//! inputs (the boundary cases in `tests/codec_probe_test.rs`) rather than
//! only through a full probe run, and so a mutation to any one of them
//! shows up as a failing test rather than a quietly different verdict.
//!
//! Governing doc: `docs/superpowers/evidence/2026-08-15-g4-protocol.md`
//! §3 (scoring rule) and §5 (decision rule).

use crate::task::{TaskResult, TaskStatus, TaskStepRecord};

/// Protocol §5's threshold as an exact integer ratio: "landing ≥80%" is
/// `landed / n >= 4 / 5`, cross-multiplied to `landed * 5 >= n * 4` so the
/// decision has no float edge at all — 16/20 is a keep, not a
/// representation-dependent coin flip.
const KEEP_NUMERATOR: u64 = 4;
const KEEP_DENOMINATOR: u64 = 5;

/// The same threshold as a float, used *only* to mark `provisional`
/// (protocol §5: the Wilson interval records, it never decides). Kept
/// beside [`KEEP_NUMERATOR`]/[`KEEP_DENOMINATOR`] so the two spellings of
/// 0.80 cannot drift apart unnoticed.
const GATE_THRESHOLD: f64 = KEEP_NUMERATOR as f64 / KEEP_DENOMINATOR as f64;

/// Protocol §5's decision rule, in its pre-registered integer form: mutating
/// verbs are kept iff `landed * 5 >= n * 4`. Widened to `u64` internally so
/// the multiplication cannot overflow for any `u32` input; the comparison is
/// exact for every value either way.
///
/// `n == 0` yields `0 >= 0` → `true`. That is unreachable through
/// [`super::run_codec_probe`] (the shipped set is N=20 and an empty set
/// would produce no fixtures to score), and is deliberately NOT special-
/// cased here: this function's contract is the arithmetic rule, and inventing
/// a different answer for a case its caller cannot produce would be a second,
/// untested rule. The fail-closed protection for "no measurement" lives where
/// it belongs — a model with no *stored gate* is read-only
/// (`Pager::model_mutating_verbs`), and an aborted probe stores none.
pub fn gate_decision(landed: u32, n: u32) -> bool {
    u64::from(landed) * KEEP_DENOMINATOR >= u64::from(n) * KEEP_NUMERATOR
}

/// Protocol §5's provisional mark: the Wilson 95% interval strictly
/// straddles the 0.80 threshold. Strict on both sides on purpose — an
/// interval that merely touches 0.80 at an endpoint does not straddle it.
///
/// This never changes a decision; it records that the measurement could not
/// resolve which side of the threshold the true rate is on.
pub fn is_provisional(lo: f64, hi: f64) -> bool {
    lo < GATE_THRESHOLD && GATE_THRESHOLD < hi
}

/// Scoring leg (a) (protocol §3): the task produced at least one `patch`
/// step that did not fail. A refused (gate-demoted), grant-violating, or
/// did-not-apply patch is `failed: true` and does not count.
fn patch_step_succeeded(steps: &[TaskStepRecord]) -> bool {
    steps.iter().any(|s| s.verb == "patch" && !s.failed)
}

/// Protocol §3's scoring rule, whole: a fixture **lands** iff BOTH a `patch`
/// step succeeded AND the declared target file's final bytes differ from the
/// bytes captured before the task ran.
///
/// The conjunction is the honesty of this instrument, and each leg has its
/// own pinned edge case in `tests/codec_probe_test.rs`:
/// - leg (a) alone would score an untouched target as a repair whenever the
///   model patched a *different* file (`scratch_file` test);
/// - leg (b) alone would score a byte-identical "fix" as a repair
///   (`identity_patch` test) — and a repair fixture's reference always
///   changes bytes, so an identity result is a non-repair by construction.
pub(crate) fn fixture_landed(
    steps: &[TaskStepRecord],
    initial_target: &[u8],
    final_target: &[u8],
) -> bool {
    patch_step_succeeded(steps) && final_target != initial_target
}

/// The `detail` recorded on a scored fixture's `CodecFixture` event
/// (invariant 4): the last `patch` step's outcome — failed or not, because
/// *why* a patch did not land is the diagnostic an operator needs — else,
/// when the task took no patch step at all, the terminal status.
pub(crate) fn fixture_detail(result: &TaskResult) -> String {
    result
        .steps
        .iter()
        .rev()
        .find(|s| s.verb == "patch")
        .map_or_else(
            || terminal_status_str(&result.status).to_string(),
            |s| s.outcome.clone(),
        )
}

/// Stable wire spelling for a [`TaskStatus`], for the same reason
/// `pager::codec_gate::patch_codec_str` exists: a journaled `detail` is
/// operator-facing wire content, so it is spelled by an explicit match here
/// rather than by whatever `Debug` happens to derive.
///
/// `Running` and `Error` are unreachable in a scored record — `run_task`
/// never returns `Running`, and `Error` is an infrastructure abort that
/// produces no `CodecFixture` event at all (protocol §3) — but the match is
/// exhaustive rather than defaulted so adding a `TaskStatus` variant is a
/// compile error here instead of a silent fallthrough.
///
/// `WindowExhausted` (protocol §9, Amendment 1) is the fallback detail for a
/// fixture that exhausted its window before ever taking a patch step — the
/// invariant-2 shape `codec_probe_test.rs::window_exhausted_is_scored_not_
/// aborted` pins.
fn terminal_status_str(status: &TaskStatus) -> &'static str {
    match status {
        TaskStatus::Running => "Running",
        TaskStatus::Done => "Done",
        TaskStatus::BudgetExhausted => "BudgetExhausted",
        TaskStatus::StepsExhausted => "StepsExhausted",
        TaskStatus::WindowExhausted => "WindowExhausted",
        TaskStatus::Error => "Error",
    }
}

/// A model name as one scratch *directory* name (invariant 2): `/` and `:`
/// both become `-`, so an org-prefixed id (`org/model`) cannot silently nest
/// a directory level and an ollama-style tag (`model:7b-q8_0`) stays legible
/// on filesystems that dislike colons.
pub(crate) fn model_dir_name(model: &str) -> String {
    model.replace(['/', ':'], "-")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(verb: &str, failed: bool, outcome: &str) -> TaskStepRecord {
        TaskStepRecord {
            step: 1,
            verb: verb.to_string(),
            outcome: outcome.to_string(),
            content: String::new(),
            failed,
        }
    }

    #[test]
    fn landing_needs_both_a_successful_patch_step_and_changed_bytes() {
        let ok = vec![step("patch", false, "patched (lens: plaintext)")];
        assert!(fixture_landed(&ok, b"before", b"after"));
        assert!(!fixture_landed(&ok, b"same", b"same"), "leg (b)");
        let failed = vec![step("patch", true, "patch did not land")];
        assert!(!fixture_landed(&failed, b"before", b"after"), "leg (a)");
        let no_patch = vec![step("read", false, "read 2 lines")];
        assert!(!fixture_landed(&no_patch, b"before", b"after"), "leg (a)");
    }

    #[test]
    fn detail_prefers_the_last_patch_step_then_falls_back_to_the_status() {
        let result = TaskResult {
            status: TaskStatus::Done,
            steps: vec![
                step("patch", true, "first patch failed"),
                step("patch", false, "patched (lens: plaintext)"),
                step("done", false, "all set"),
            ],
            summary: None,
        };
        assert_eq!(fixture_detail(&result), "patched (lens: plaintext)");

        let no_patch = TaskResult {
            status: TaskStatus::StepsExhausted,
            steps: vec![step("read", false, "read 2 lines")],
            summary: None,
        };
        assert_eq!(fixture_detail(&no_patch), "StepsExhausted");
    }

    #[test]
    fn model_dir_name_maps_both_separators() {
        assert_eq!(model_dir_name("org/m:7b-q8_0"), "org-m-7b-q8_0");
        assert_eq!(model_dir_name("plain"), "plain");
    }
}
