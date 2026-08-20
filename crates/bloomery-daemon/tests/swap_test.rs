//! The swap-candidate seam's cover gate (spec §3-§4,
//! `docs/superpowers/specs/2026-08-19-swap-candidate-seam-design.md`).
//!
//! No assay, no python, no GPU here: the gate's subprocess is injected, the
//! same seam `drift_test.rs` drives, so all four documented exit codes, the
//! undocumented-code path and the signal path are exercised with assay never
//! installed.
//!
//! The process fixtures below (`Calls`, `exited`, `output`) are copied from
//! `drift_test.rs` rather than imported: each file under `tests/` is its own
//! crate, `tests/common/mod.rs` carries only the shared HTTP client, and the
//! crate's `test-support` module (`src/test_support.rs`) carries only the
//! ready-to-serve pager. Moving three-line wait-status helpers into either
//! would widen a shared surface for less than it costs to restate them.

use bloomery_daemon::swap::{cover_argv, CoverGate, CoverOutcome};
use std::path::Path;

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

/// Every `(program, argv)` the gate spawned, in order. `Rc`/`RefCell` because
/// a [`bloomery_daemon::post::CommandRunner`] is deliberately not `Send` and
/// these tests are single-threaded.
type Calls = std::rc::Rc<std::cell::RefCell<Vec<(String, Vec<String>)>>>;

/// The signal `signalled` reports. Named rather than spelled `9` inline: the
/// number is a kernel constant, not a knob of this test.
const SIGKILL: i32 = 9;

/// A wait status carrying exit code `code` — the encoding `waitpid` returns.
fn exited(code: i32) -> std::process::ExitStatus {
    use std::os::unix::process::ExitStatusExt;
    std::process::ExitStatus::from_raw(code << 8)
}

/// A wait status for a child killed by `SIGKILL`: no exit code at all, which
/// is the case `exit_code: None` exists for. The same construction
/// `drift_test.rs::signalled` makes, with the signal fixed — this file has
/// exactly one signal case and nothing here varies by which signal it was.
fn signalled() -> std::process::ExitStatus {
    use std::os::unix::process::ExitStatusExt;
    std::process::ExitStatus::from_raw(SIGKILL)
}

fn output(status: std::process::ExitStatus, stderr: &str) -> std::process::Output {
    std::process::Output {
        status,
        stdout: Vec::new(),
        stderr: stderr.as_bytes().to_vec(),
    }
}

/// A gate whose subprocess is scripted: it records every spawn and answers
/// with `status` and no stderr.
fn gate_answering(status: std::process::ExitStatus) -> (CoverGate, Calls) {
    gate_saying(status, "")
}

/// The same, with assay given words of its own — what the infrastructure
/// details carry through verbatim for the operator.
fn gate_saying(status: std::process::ExitStatus, stderr: &str) -> (CoverGate, Calls) {
    let calls: Calls = Calls::default();
    let sink = calls.clone();
    let stderr = stderr.to_string();
    let gate = CoverGate::with_runner(Box::new(move |program: &str, args: &[String]| {
        sink.borrow_mut().push((program.to_string(), args.to_vec()));
        Ok(output(status, &stderr))
    }));
    (gate, calls)
}

// ---------------------------------------------------------------------------
// The invocation
// ---------------------------------------------------------------------------

#[test]
fn cover_argv_is_the_documented_invocation() {
    let argv = cover_argv(Path::new("/d/floor.json"), Path::new("/d/cand.json"));
    assert_eq!(
        argv,
        vec!["-m", "assay", "cover", "/d/floor.json", "/d/cand.json"]
    );
}

#[test]
fn check_spawns_exactly_that_invocation_once() {
    let (gate, calls) = gate_answering(exited(0));
    gate.check(Path::new("/d/floor.json"), Path::new("/d/cand.json"));
    let calls = calls.borrow();
    assert_eq!(calls.len(), 1, "{calls:?}");
    assert_eq!(
        calls[0].1,
        cover_argv(Path::new("/d/floor.json"), Path::new("/d/cand.json"))
    );
}

// ---------------------------------------------------------------------------
// The four documented codes (spec §3)
// ---------------------------------------------------------------------------

#[test]
fn exit_zero_is_covered() {
    let (gate, _calls) = gate_answering(exited(0));
    let reading = gate.check(Path::new("f"), Path::new("c"));
    assert_eq!(reading.outcome, CoverOutcome::Covered);
    assert_eq!(reading.exit_code, Some(0));
}

#[test]
fn exit_one_is_not_covered() {
    let (gate, _calls) = gate_answering(exited(1));
    let reading = gate.check(Path::new("f"), Path::new("c"));
    assert_eq!(reading.outcome, CoverOutcome::NotCovered);
}

#[test]
fn exit_two_is_refused_and_never_a_pass() {
    let (gate, _calls) = gate_answering(exited(2));
    let reading = gate.check(Path::new("f"), Path::new("c"));
    assert_eq!(
        reading.outcome,
        CoverOutcome::Refused {
            exit: 2,
            // A refusal that said nothing carries nothing: the empty string
            // is the expected shape, not a missing field.
            stderr: String::new(),
        }
    );
}

/// An assay too old to have the subcommand (anything < 0.13.0 under the
/// PYTHONPATH pin) makes `argparse` exit 2 with `invalid choice: 'cover'` —
/// byte-identical, by exit code alone, to a considered refusal about the
/// candidate. Carrying assay's sentence is the only thing that lets an
/// operator tell "your floor and candidate disagree on hardware class" from
/// "the tool you invoked has no cover command".
#[test]
fn a_missing_cover_subcommand_reads_refused_with_argparses_words() {
    let (gate, _calls) = gate_saying(exited(2), "invalid choice: 'cover'");
    match gate.check(Path::new("f"), Path::new("c")).outcome {
        CoverOutcome::Refused { exit, stderr } => {
            assert_eq!(exit, 2);
            assert!(stderr.contains("invalid choice"), "{stderr}");
        }
        other => panic!("expected Refused, got {other:?}"),
    }
}

#[test]
fn exit_three_is_incomplete_and_never_a_pass() {
    let (gate, _calls) = gate_answering(exited(3));
    let reading = gate.check(Path::new("f"), Path::new("c"));
    assert_eq!(reading.outcome, CoverOutcome::Incomplete);
}

// ---------------------------------------------------------------------------
// Everything else is infrastructure, not a verdict
// ---------------------------------------------------------------------------

#[test]
fn an_undocumented_exit_is_infrastructure_not_a_verdict() {
    let (gate, _calls) = gate_answering(exited(7));
    match gate.check(Path::new("f"), Path::new("c")).outcome {
        CoverOutcome::Infra { detail } => {
            assert!(detail.contains("undocumented exit 7"), "{detail}");
        }
        other => panic!("expected Infra, got {other:?}"),
    }
}

/// Mirrors `drift_test.rs::a_spawn_failure_is_infra_naming_the_command` for
/// this gate: a runner that never reached a child at all must name what it
/// tried to run, and carry the OS's own words — nothing downstream can
/// reconstruct either from an outcome that only said "infrastructure".
#[test]
fn a_spawn_failure_is_infra_naming_the_command() {
    let gate = CoverGate::with_runner(Box::new(|_program, _args| {
        Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "no such file or directory",
        ))
    }));

    let reading = gate.check(Path::new("/d/floor.json"), Path::new("/d/cand.json"));

    match &reading.outcome {
        CoverOutcome::Infra { detail } => {
            assert!(
                detail.contains("python3"),
                "the interpreter must be named: {detail:?}"
            );
            assert!(
                detail.contains("assay") && detail.contains("cover"),
                "the failed command must be named: {detail:?}"
            );
            assert!(
                detail.contains("/d/floor.json") && detail.contains("/d/cand.json"),
                "the argv must be named: {detail:?}"
            );
            assert!(
                detail.contains("no such file"),
                "the OS's own words must survive: {detail:?}"
            );
        }
        other => panic!("expected Infra for a spawn failure, got {other:?}"),
    }
    assert_eq!(reading.exit_code, None);
}

#[test]
fn a_signal_killed_cover_has_no_exit_code_and_is_infrastructure() {
    let (gate, _calls) = gate_answering(signalled());
    let reading = gate.check(Path::new("f"), Path::new("c"));
    assert_eq!(reading.exit_code, None);
    assert!(matches!(reading.outcome, CoverOutcome::Infra { .. }));
}

#[test]
fn assays_own_words_ride_along_with_an_infrastructure_detail() {
    let (gate, _calls) = gate_saying(exited(7), "cover: no such option");
    match gate.check(Path::new("f"), Path::new("c")).outcome {
        CoverOutcome::Infra { detail } => {
            assert!(detail.contains("cover: no such option"), "{detail}");
        }
        other => panic!("expected Infra, got {other:?}"),
    }
}
