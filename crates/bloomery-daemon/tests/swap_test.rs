//! The cover gate: how `assay cover`'s exit codes become a verdict.
//!
//! The invocation itself, the four documented codes (spec §3), and the rule
//! that everything else is infrastructure rather than a verdict -- an
//! unreadable binary, a signal death or an unparseable reply must never be
//! rendered as "not covered", because a probe that could not run is not
//! evidence about the candidate.
//!
//! **Split 2026-09-01** (carried-debt slice D): this file was 1021 lines. The
//! job is in `swap_job_test.rs`, the shared fixtures in `tests/common/swap.rs`.

mod common;

use bloomery_daemon::swap::{cover_argv, CoverGate, CoverOutcome};
use std::path::Path;

use common::swap::{exited, gate_answering, gate_saying, signalled};

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
