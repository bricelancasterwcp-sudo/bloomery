//! The drift gate -- spec §3 (instrument precheck) and §4 (diff subprocess)
//! -- and the journal row it writes.
//!
//! Split out of `drift_test.rs` on 2026-09-01 (carried-debt slice D).

mod common;

use bloomery_core::journal::{replay, sha256_hex, sha256_hex_bytes, Event, Journal};
use bloomery_daemon::config::load_config;
use bloomery_daemon::drift::{
    diff_argv, drift_event, Comparison, DriftGate, GateOutcome, DIFF_TIMEOUT_SECS,
};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use common::drift::{exited, scratch, Calls, V4_QWEN3_8B, V8_QWEN3_8B};

/// Real committed assay documents; provenance for all four is tabulated in
/// `profile_v8_fixture_test.rs`, which also pins the identity claims made
/// here.
///
/// - `V8_QWEN15B` / `V8_QWEN15B_DRYRUN` — **one model, one instrument, two
///   documents** (the 2026-08 campaign row and the dry-run row it superseded,
///   measured ~26 minutes apart). This is what a drift comparison actually
///   compares, so it is what the happy paths below use.
/// - `V8_QWEN3_8B` — same instrument, a **different model**: a crossed pair,
///   used only to pin the refusal.
/// - `V4_QWEN3_8B` — same model as `V8_QWEN3_8B`, the pre-upgrade instrument
///   (`0.5.0` / schema 4) the first post-upgrade boot will actually meet.
const V8_QWEN15B: &str = include_str!("fixtures/profile-v8-qwen15b.json");

const V8_QWEN15B_DRYRUN: &str = include_str!("fixtures/profile-v8-qwen15b-dryrun.json");

/// A wait status for a child killed by `sig`: no exit code at all, which is
/// the case `exit_code: None` exists for.
fn signalled(sig: i32) -> std::process::ExitStatus {
    use std::os::unix::process::ExitStatusExt;
    std::process::ExitStatus::from_raw(sig)
}

/// A gate whose subprocess is scripted: it records every spawn and answers
/// with `status`. The recording is the point — several tests below assert the
/// diff was *never* spawned, which no assertion on the returned outcome could
/// establish.
fn gate_answering(status: std::process::ExitStatus) -> (DriftGate, Calls) {
    let calls: Calls = Calls::default();
    let sink = calls.clone();
    let gate = DriftGate::with_runner(Box::new(move |program: &str, args: &[String]| {
        sink.borrow_mut().push((program.to_string(), args.to_vec()));
        Ok(std::process::Output {
            status,
            stdout: Vec::new(),
            stderr: Vec::new(),
        })
    }));
    (gate, calls)
}

/// A reference/current pair on disk. `None` writes no file at all, which is
/// how "first boot ever" and "baseline never blessed" actually look.
fn pair(tag: &str, reference: Option<&str>, current: Option<&str>) -> (PathBuf, PathBuf) {
    let dir = scratch(tag);
    let r = dir.join("reference.json");
    let c = dir.join("current.json");
    if let Some(doc) = reference {
        std::fs::write(&r, doc).unwrap();
    }
    if let Some(doc) = current {
        std::fs::write(&c, doc).unwrap();
    }
    (r, c)
}

fn sha_of(path: &Path) -> Option<String> {
    Some(sha256_hex_bytes(&std::fs::read(path).unwrap()))
}

// The gate — spec §3 (instrument precheck) and §4 (diff subprocess)
// ---------------------------------------------------------------------------

/// Spec §4's invocation, to the letter. A value rather than a side effect of
/// spawning, so the contract with assay is readable in one line.
#[test]
fn diff_argv_is_the_documented_invocation() {
    assert_eq!(
        diff_argv(
            Path::new("/p/qwen.previous.json"),
            Path::new("/p/qwen.json")
        ),
        vec![
            "-m",
            "assay",
            "diff",
            "/p/qwen.previous.json",
            "/p/qwen.json",
            "--gate"
        ]
    );
}

/// A comparable pair really does spawn that invocation — and exactly once.
#[test]
fn a_comparable_pair_spawns_exactly_the_documented_diff() {
    let (r, c) = pair("gate-argv", Some(V8_QWEN15B), Some(V8_QWEN15B_DRYRUN));
    let (gate, calls) = gate_answering(exited(0));

    let reading = gate.compare(&r, &c);

    assert_eq!(reading.outcome, GateOutcome::WithinNoise);
    let calls = calls.borrow();
    assert_eq!(calls.len(), 1, "one comparison spawns one diff");
    // `config::default_python()`, which `with_runner` borrows so the two
    // spellings of "how this daemon runs python" cannot diverge.
    assert_eq!(calls[0].0, "python3");
    assert_eq!(calls[0].1, diff_argv(&r, &c));
}

#[test]
fn exit_zero_is_within_noise() {
    let (r, c) = pair("gate-exit0", Some(V8_QWEN15B), Some(V8_QWEN15B_DRYRUN));
    let (gate, _calls) = gate_answering(exited(0));

    let reading = gate.compare(&r, &c);

    assert_eq!(reading.outcome, GateOutcome::WithinNoise);
    assert_eq!(reading.exit_code, Some(0));
}

#[test]
fn exit_one_is_drift() {
    let (r, c) = pair("gate-exit1", Some(V8_QWEN15B), Some(V8_QWEN15B_DRYRUN));
    let (gate, _calls) = gate_answering(exited(1));

    let reading = gate.compare(&r, &c);

    assert_eq!(
        reading.outcome,
        GateOutcome::Drift,
        "exit 1 is a drift reading; whether it alarms is the confirm stage's call"
    );
    assert_eq!(reading.exit_code, Some(1));
}

/// Exit 2 is diff's own refusal to compare (one-sided tier marking and
/// friends). It is never a pass: the whole family of silent-pass bugs this
/// gate exists to refuse starts by folding a refusal into `WithinNoise`.
#[test]
fn exit_two_is_not_comparable_and_never_a_pass() {
    let (r, c) = pair("gate-exit2", Some(V8_QWEN15B), Some(V8_QWEN15B_DRYRUN));
    let (gate, _calls) = gate_answering(exited(2));

    let reading = gate.compare(&r, &c);

    assert_eq!(reading.outcome, GateOutcome::NotComparable { exit: 2 });
    assert_eq!(reading.exit_code, Some(2));
}

/// Exit 3 is assay ≥ 0.10's incomplete comparison: a cell measured on exactly
/// one side, outranking a measured drift (its precedence is 2 > 3 > 1 > 0).
/// It is a documented verdict, not an undocumented code — and it is never a
/// pass, because the cells it could not score may hide exactly the move the
/// gate exists to catch.
#[test]
fn exit_three_is_incomplete_and_never_a_pass() {
    let (r, c) = pair("gate-exit3", Some(V8_QWEN15B), Some(V8_QWEN15B_DRYRUN));
    let (gate, _calls) = gate_answering(exited(3));

    let reading = gate.compare(&r, &c);

    assert_eq!(reading.outcome, GateOutcome::Incomplete { exit: 3 });
    assert_eq!(reading.exit_code, Some(3));
    assert_eq!(reading.outcome.journal_outcome(), "incomplete");
}

/// assay documents 0, 1, 2 and (since 0.10) 3 for `diff --gate`. Anything
/// else is a tool this daemon does not understand, so it is infrastructure —
/// the one honest reading. The code is still recorded: it exists, so `None`
/// would be a lie.
#[test]
fn an_undocumented_exit_code_is_infrastructure_not_a_verdict() {
    let (r, c) = pair("gate-exit7", Some(V8_QWEN15B), Some(V8_QWEN15B_DRYRUN));
    let (gate, _calls) = gate_answering(exited(7));

    let reading = gate.compare(&r, &c);

    match &reading.outcome {
        GateOutcome::Infra { detail } => assert!(
            detail.contains("undocumented exit 7"),
            "the unknown code must be named: {detail:?}"
        ),
        other => panic!("expected Infra for an undocumented exit, got {other:?}"),
    }
    assert_eq!(reading.exit_code, Some(7));
}

/// A signal-killed diff has no exit code at all. `-1` would look like a code;
/// `None` is what actually happened (None-vs-zero, applied to exit status).
#[test]
fn a_signal_killed_diff_is_infra_with_no_exit_code() {
    let (r, c) = pair("gate-signal", Some(V8_QWEN15B), Some(V8_QWEN15B_DRYRUN));
    let (gate, _calls) = gate_answering(signalled(9));

    let reading = gate.compare(&r, &c);

    match &reading.outcome {
        GateOutcome::Infra { detail } => assert!(
            detail.contains("signal"),
            "the kill must be named: {detail:?}"
        ),
        other => panic!("expected Infra for a signal-killed diff, got {other:?}"),
    }
    assert_eq!(reading.exit_code, None);
}

#[test]
fn a_spawn_failure_is_infra_naming_the_command() {
    let (r, c) = pair("gate-spawn-fail", Some(V8_QWEN15B), Some(V8_QWEN15B_DRYRUN));
    let gate = DriftGate::with_runner(Box::new(|_program, _args| {
        Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "no such file or directory",
        ))
    }));

    let reading = gate.compare(&r, &c);

    match &reading.outcome {
        GateOutcome::Infra { detail } => {
            assert!(
                detail.contains("assay") && detail.contains("diff"),
                "the failed command must be named: {detail:?}"
            );
            assert!(
                detail.contains("no such file"),
                "the OS's own words must survive: {detail:?}"
            );
        }
        other => panic!("expected Infra for a spawn failure, got {other:?}"),
    }
    assert_eq!(reading.exit_code, None);
    // Both documents were read before the spawn was attempted, so both
    // digests are known even though the diff never ran.
    assert_eq!(reading.reference_sha, sha_of(&r));
    assert_eq!(reading.current_sha, sha_of(&c));
}

/// Two documents about different models measure nothing in common, so there
/// is no comparison to run — the same rule `post::PostRunner::probe` already
/// applies to a single document ("attaching it would credit one model with
/// another's measurements"), applied to the pair. Without this the crossed
/// pair diffs cleanly and `drift_event` stamps the caller's model name over
/// another model's document: a verdict about a model nobody measured.
///
/// Checked *before* the instrument precheck: on a crossed pair the instrument
/// answer is noise either way (these two happen to share an instrument, so it
/// would read `Comparable` and spawn).
#[test]
fn a_crossed_pair_of_models_is_unmeasured_and_never_spawns() {
    let (r, c) = pair("gate-crossed", Some(V8_QWEN3_8B), Some(V8_QWEN15B));
    let (gate, calls) = gate_answering(exited(0));

    let reading = gate.compare(&r, &c);

    match &reading.outcome {
        GateOutcome::Unmeasured { reason } => assert!(
            reason.contains("qwen3:8b") && reason.contains("qwen2.5-coder:1.5b-instruct-q8_0"),
            "both models must be named so the operator can see which pair crossed: {reason:?}"
        ),
        other => panic!("expected Unmeasured for a crossed pair, got {other:?}"),
    }
    assert!(
        calls.borrow().is_empty(),
        "a crossed pair must never reach the diff, got {:?}",
        calls.borrow()
    );
    assert_eq!(reading.exit_code, None);
    // Both documents were read, so both digests stand — the row stays
    // verifiable even though no comparison was made.
    assert_eq!(reading.reference_sha, sha_of(&r));
    assert_eq!(reading.current_sha, sha_of(&c));
}

/// Spec §3, and the load-bearing ordering claim of this whole task: the
/// precheck reads the two documents' own version fields and refuses *before*
/// any subprocess exists. A gate that spawned first and prechecked after would
/// return the same outcome here — only the never-spawned assertion catches it.
#[test]
fn a_changed_instrument_is_named_before_the_diff_is_ever_spawned() {
    let (r, c) = pair("gate-instrument", Some(V4_QWEN3_8B), Some(V8_QWEN3_8B));
    let (gate, calls) = gate_answering(exited(0));

    let reading = gate.compare(&r, &c);

    assert_eq!(
        reading.outcome,
        GateOutcome::InstrumentChanged {
            reference: "0.5.0/v4".to_string(),
            current: "0.9.0/v8".to_string(),
        }
    );
    assert!(
        calls.borrow().is_empty(),
        "the precheck must run BEFORE the diff is spawned, got {:?}",
        calls.borrow()
    );
    assert_eq!(reading.exit_code, None, "no diff ran, so there is no code");
    assert_eq!(reading.reference_sha, sha_of(&r));
    assert_eq!(reading.current_sha, sha_of(&c));
}

/// First boot ever, or a baseline nobody blessed. `unmeasured` by name — never
/// a pass, and never a spawn (there is nothing to hand assay).
#[test]
fn an_absent_reference_is_unmeasured_and_never_spawns() {
    let (r, c) = pair("gate-absent-ref", None, Some(V8_QWEN3_8B));
    let (gate, calls) = gate_answering(exited(0));

    let reading = gate.compare(&r, &c);

    match &reading.outcome {
        GateOutcome::Unmeasured { reason } => {
            assert!(
                reason.contains("reference") && reason.contains(&r.display().to_string()),
                "the missing side and its path must be named: {reason:?}"
            );
        }
        other => panic!("expected Unmeasured for an absent reference, got {other:?}"),
    }
    assert!(
        calls.borrow().is_empty(),
        "an absent reference must never spawn a diff"
    );
    assert_eq!(reading.exit_code, None);
    assert_eq!(
        reading.reference_sha, None,
        "a file that was never read has no digest — None, not a digest of nothing"
    );
    assert_eq!(reading.current_sha, sha_of(&c));
}

/// A reference whose bytes exist but are not a profile is also unmeasured —
/// and its digest is still recorded, because those bytes were read and an
/// operator may want to check exactly which document failed to parse.
#[test]
fn an_unparseable_reference_is_unmeasured_with_its_bytes_still_named() {
    let (r, c) = pair("gate-bad-ref", Some("{ truncated json"), Some(V8_QWEN3_8B));
    let (gate, calls) = gate_answering(exited(0));

    let reading = gate.compare(&r, &c);

    match &reading.outcome {
        GateOutcome::Unmeasured { reason } => {
            assert!(
                reason.contains(&r.display().to_string()),
                "the unparseable document must be named: {reason:?}"
            );
            assert!(!reason.is_empty(), "the refusal must say why");
        }
        other => panic!("expected Unmeasured for an unparseable reference, got {other:?}"),
    }
    assert!(calls.borrow().is_empty());
    assert_eq!(
        reading.reference_sha,
        sha_of(&r),
        "the bytes were read, so their digest is known"
    );
}

/// The current side too: a boot where POST failed has nothing to compare, and
/// that is `unmeasured`, not a clean bill of health.
#[test]
fn an_absent_current_is_unmeasured_too() {
    let (r, c) = pair("gate-absent-cur", Some(V8_QWEN3_8B), None);
    let (gate, calls) = gate_answering(exited(0));

    let reading = gate.compare(&r, &c);

    match &reading.outcome {
        GateOutcome::Unmeasured { reason } => assert!(
            reason.contains("current") && reason.contains(&c.display().to_string()),
            "the missing side and its path must be named: {reason:?}"
        ),
        other => panic!("expected Unmeasured for an absent current, got {other:?}"),
    }
    assert!(calls.borrow().is_empty());
    assert_eq!(reading.current_sha, None);
    assert_eq!(reading.reference_sha, sha_of(&r));
}

/// The controller ruling: a drift row's file claims are byte-verifiable the
/// way a `Blessed` row's are. The digests are of each file's **bytes**, taken
/// at comparison time — so `sha256sum` on the path in the row checks the row.
#[test]
fn the_digests_are_of_the_files_bytes_so_the_row_is_checkable() {
    let (r, c) = pair("gate-sha", Some(V8_QWEN15B), Some(V8_QWEN15B_DRYRUN));
    let (gate, _calls) = gate_answering(exited(0));

    let reading = gate.compare(&r, &c);

    assert_eq!(reading.reference_sha, sha_of(&r));
    assert_eq!(reading.current_sha, sha_of(&c));
    assert_ne!(
        reading.reference_sha, reading.current_sha,
        "two different documents must not share a digest"
    );
    // A digest of the *path string* would also be 64 hex characters and would
    // also differ between the two sides — only comparing against the bytes
    // tells the two apart.
    assert_ne!(
        reading.reference_sha,
        Some(sha256_hex(&r.display().to_string())),
        "the digest must be of the document's bytes, not of its path"
    );
    assert_eq!(reading.reference_sha.as_ref().unwrap().len(), 64);
}

/// A wedged diff is bounded and named, not a verdict. Drives the *real*
/// bounded-spawn layer (the injected runner replaces the whole subprocess and
/// so cannot exercise it) against a child that outlives its cap.
#[test]
fn a_wedged_diff_is_infra_not_a_verdict() {
    let (r, c) = pair("gate-wedged", Some(V8_QWEN15B), Some(V8_QWEN15B_DRYRUN));
    let gate = DriftGate::with_runner(Box::new(|_program, _args| {
        bloomery_daemon::post::run_bounded_for_test(
            "/bin/sh",
            &["-c".to_string(), "sleep 5".to_string()],
            Duration::from_millis(300),
        )
    }));

    let started = Instant::now();
    let reading = gate.compare(&r, &c);

    match &reading.outcome {
        GateOutcome::Infra { detail } => assert!(
            detail.contains("timed out"),
            "the expiry must be named: {detail:?}"
        ),
        other => panic!("expected Infra for a wedged diff, got {other:?}"),
    }
    assert_eq!(reading.exit_code, None, "a killed diff reported no code");
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "the cap must bound the wait, not the child"
    );
}

/// `assay diff` reads two JSON documents; it drives no model, so it is not
/// bounded by the probe cap an operator raises for slow, partially-offloaded
/// models. Pinned against the shipped default rather than against a literal,
/// so the relationship is what is asserted.
#[test]
fn the_diff_cap_is_its_own_constant_not_the_probe_cap() {
    let gate = DriftGate::new("python3".to_string());
    assert_eq!(gate.timeout(), Duration::from_secs(DIFF_TIMEOUT_SECS));

    let dir = scratch("gate-timeout-config");
    let cfg_path = dir.join("bloomery.toml");
    std::fs::write(
        &cfg_path,
        r#"
port = 9000
data_dir = "/tmp/bloomery-drift-gate-timeout"
tier = { name = "enthusiast-16gb", emulated = false }
assay = { enabled = true, python = "python3" }

[models]
qwen = "/models/qwen.gguf"
"#,
    )
    .unwrap();
    let config = load_config(&cfg_path).expect("config");
    assert!(
        DIFF_TIMEOUT_SECS < config.assay.probe_timeout_secs,
        "the offline diff must not inherit the model-driving probe's cap \
         ({DIFF_TIMEOUT_SECS}s vs {}s)",
        config.assay.probe_timeout_secs
    );
}

// ---------------------------------------------------------------------------
// The journal row
// ---------------------------------------------------------------------------

/// Every field of the row comes from the reading the gate actually produced —
/// the paths it compared, the digests of the bytes at those paths, and the
/// code diff reported. Nothing is re-derived by the caller, so the row cannot
/// describe a different read than the comparison did.
#[test]
fn a_drift_row_carries_the_readings_paths_digests_and_exit_code() {
    let (r, c) = pair("gate-row", Some(V8_QWEN15B), Some(V8_QWEN15B_DRYRUN));
    let (gate, _calls) = gate_answering(exited(1));
    let reading = gate.compare(&r, &c);

    match drift_event("qwen3:8b", Comparison::Step, &reading) {
        Event::Drift {
            model,
            comparison,
            outcome,
            reference_path,
            current_path,
            exit_code,
            reference_sha,
            current_sha,
        } => {
            assert_eq!(model, "qwen3:8b");
            assert_eq!(comparison, "step");
            assert_eq!(outcome, "drift");
            assert_eq!(reference_path, r.display().to_string());
            assert_eq!(current_path, c.display().to_string());
            assert_eq!(exit_code, Some(1));
            assert_eq!(reference_sha, sha_of(&r));
            assert_eq!(current_sha, sha_of(&c));
        }
        other => panic!("expected a Drift row, got {other:?}"),
    }
}

/// The unmeasured row names its comparison and leaves every number absent —
/// no zero exit code, no digest for a file that was never read.
#[test]
fn an_unmeasured_row_names_the_comparison_and_leaves_the_numbers_absent() {
    let (r, c) = pair("gate-row-unmeasured", None, Some(V8_QWEN3_8B));
    let (gate, _calls) = gate_answering(exited(0));
    let reading = gate.compare(&r, &c);

    match drift_event("qwen3:8b", Comparison::Cumulative, &reading) {
        Event::Drift {
            comparison,
            outcome,
            exit_code,
            reference_sha,
            ..
        } => {
            assert_eq!(comparison, "cumulative");
            assert!(
                outcome.starts_with("unmeasured:") && outcome.contains(&r.display().to_string()),
                "the row must name the refusal and its cause: {outcome:?}"
            );
            assert_eq!(exit_code, None);
            assert_eq!(reference_sha, None);
        }
        other => panic!("expected a Drift row, got {other:?}"),
    }
}

/// Spec §3's row: both instrument identities in the outcome, so a replay can
/// say what moved without re-reading either document. Identity strings, not
/// transcribed measurements.
#[test]
fn an_instrument_changed_row_names_both_instruments() {
    let (r, c) = pair("gate-row-instrument", Some(V4_QWEN3_8B), Some(V8_QWEN3_8B));
    let (gate, _calls) = gate_answering(exited(0));
    let reading = gate.compare(&r, &c);

    match drift_event("qwen3:8b", Comparison::Cumulative, &reading) {
        Event::Drift { outcome, .. } => assert!(
            outcome.contains("instrument-changed")
                && outcome.contains("0.5.0/v4")
                && outcome.contains("0.9.0/v8"),
            "both sides' instrument identities belong in the row: {outcome:?}"
        ),
        other => panic!("expected a Drift row, got {other:?}"),
    }
}

/// The row survives a journal round trip with its `None`s intact — an absent
/// digest must not come back as an empty string, and an absent exit code must
/// not come back as 0.
#[test]
fn a_drift_row_round_trips_through_the_journal() {
    let dir = scratch("gate-row-journal");
    let (r, c) = pair("gate-row-journal-docs", None, Some(V8_QWEN3_8B));
    let (gate, _calls) = gate_answering(exited(0));
    let event = drift_event("qwen3:8b", Comparison::Step, &gate.compare(&r, &c));

    let jpath = dir.join("j.jsonl");
    let mut j = Journal::open(&jpath).unwrap();
    j.append(&event).unwrap();

    assert_eq!(replay(&jpath).unwrap(), vec![event]);
}

// ---------------------------------------------------------------------------
