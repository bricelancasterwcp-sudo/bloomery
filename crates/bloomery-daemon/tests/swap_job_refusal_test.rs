//! The swap-candidate job's refusal and infrastructure rows.
//!
//! `refused`, `incomplete` and `not covered` each journal their own word;
//! a refusal carries assay's own words into both row and report; and a cover
//! that could not run names the infrastructure rather than pretending to a
//! verdict. The slot is released even on an `Err` return, and a candidate
//! that is not a GGUF -- or a floor that cannot be read -- is named before
//! anything is probed.
//!
//! Split out of `swap_job_test.rs` on 2026-09-01 (slice D).

mod common;

use bloomery_core::journal::Event;
use bloomery_daemon::swap::{CoverGate, SwapSlot, SwapState};

use common::swap::{exited, gate_answering, gate_saying};

use common::swap_job::{job, job_with, profile_doc, scripted_probes, MODEL};

/// Each documented code gets its own word, and **a verdict is not an
/// infrastructure failure**: none of the three journals a `Degraded` row.
#[test]
fn refused_incomplete_and_not_covered_all_journal_their_own_word() {
    for (exit, word) in [(1, "not-covered"), (2, "refused"), (3, "incomplete")] {
        let job = job(&format!("verdict-{exit}"));
        job.seed_floor(&profile_doc(MODEL));
        let (runner, _probes) = scripted_probes(vec![Ok(())]);
        let (gate, _calls) = gate_answering(exited(exit));
        let slot = SwapSlot::default();

        job.run(&runner, &gate, &slot).expect("the job records");

        let rows = job.swap_rows();
        assert_eq!(rows.len(), 1, "exit {exit}: {rows:?}");
        match &rows[0] {
            Event::SwapCandidate {
                outcome, exit_code, ..
            } => {
                assert_eq!(outcome, word, "exit {exit} is spelled {word}");
                assert_eq!(*exit_code, Some(exit));
            }
            other => panic!("expected SwapCandidate, got {other:?}"),
        }
        assert!(
            job.degraded().is_empty(),
            "exit {exit} is a verdict, not an infrastructure failure: {:?}",
            job.degraded()
        );
        match slot.snapshot() {
            SwapState::Done { report, .. } => assert_eq!(report.outcome, word),
            other => panic!("expected Done, got {other:?}"),
        }
    }
}

/// The Task-1 review's ruling, carried through to the operator: exit 2 is also
/// what `argparse` returns for `invalid choice: 'cover'`, so a refusal's own
/// words must reach the journal row **and** the report — otherwise a stale
/// assay is indistinguishable from a considered refusal about the candidate,
/// and the operator has to re-run the command to find out which they got.
#[test]
fn a_refusal_carries_assays_words_into_the_row_and_the_report() {
    let job = job("refused-words");
    job.seed_floor(&profile_doc(MODEL));
    let (runner, _probes) = scripted_probes(vec![Ok(())]);
    let (gate, _calls) = gate_saying(exited(2), "invalid choice: 'cover'");
    let slot = SwapSlot::default();

    job.run(&runner, &gate, &slot).expect("the job records");

    let rows = job.swap_rows();
    assert_eq!(rows.len(), 1, "{rows:?}");
    match &rows[0] {
        Event::SwapCandidate {
            outcome, exit_code, ..
        } => {
            assert!(
                outcome.starts_with("refused") && outcome.contains("invalid choice: 'cover'"),
                "the row keeps the refusal's word AND assay's reason: {outcome}"
            );
            assert_eq!(*exit_code, Some(2));
        }
        other => panic!("expected SwapCandidate, got {other:?}"),
    }
    match slot.snapshot() {
        SwapState::Done { report, .. } => assert!(
            report.outcome.contains("invalid choice: 'cover'"),
            "the report says why the refusal happened without a re-run: {report:?}"
        ),
        other => panic!("expected Done, got {other:?}"),
    }
}

/// A cover that could not run at all is `infra: …` — still a row, because both
/// documents exist and the comparison was really attempted, unlike the
/// probe-failure path above.
#[test]
fn a_cover_that_cannot_run_is_a_row_naming_the_infrastructure_failure() {
    let job = job("cover-infra");
    job.seed_floor(&profile_doc(MODEL));
    let (runner, _probes) = scripted_probes(vec![Ok(())]);
    let gate = CoverGate::with_runner(Box::new(|_program, _args| {
        Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "no such file or directory",
        ))
    }));
    let slot = SwapSlot::default();

    job.run(&runner, &gate, &slot).expect("the job records");

    let rows = job.swap_rows();
    assert_eq!(rows.len(), 1, "{rows:?}");
    match &rows[0] {
        Event::SwapCandidate {
            outcome, exit_code, ..
        } => {
            assert!(
                outcome.starts_with("infra: ") && outcome.contains("no such file"),
                "the failing layer's own words, never a verdict: {outcome}"
            );
            assert_eq!(*exit_code, None);
        }
        other => panic!("expected SwapCandidate, got {other:?}"),
    }
}

/// The slot is released on **every** path this job returns through, the `Err`
/// ones included. A worker that returned still holding it would leave this
/// daemon answering `candidate_probe_in_progress` for a job nobody can see, for
/// the life of the process — no restart-free way back.
///
/// Driven through the poisoned-pager failure: a modeled, named condition
/// (`post::with_pager`), reached the same way `codec_probe_test.rs` reaches it,
/// and the one `Err` return a test can produce without scripting the
/// filesystem.
#[test]
fn an_err_return_still_releases_the_slot() {
    let job = job("err-releases-slot");
    job.seed_floor(&profile_doc(MODEL));
    let (runner, probes) = scripted_probes(vec![Ok(())]);
    let (gate, calls) = gate_answering(exited(0));
    let slot = SwapSlot::default();
    slot.try_start(MODEL, &job.candidate)
        .expect("the slot is idle");

    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _guard = job.pager.lock().unwrap();
        panic!("poison the pager lock");
    }));
    assert!(job.pager.is_poisoned());

    let err = job
        .run(&runner, &gate, &slot)
        .expect_err("a poisoned pager aborts the job");
    assert!(err.to_string().contains("poisoned"), "{err}");

    assert!(probes.borrow().is_empty(), "nothing is probed");
    assert!(calls.borrow().is_empty(), "nothing is covered");
    match slot.snapshot() {
        SwapState::Done { report, .. } => {
            assert!(
                report.outcome.starts_with("infra: "),
                "the failure is named, and it is not a verdict: {report:?}"
            );
            assert_eq!(report.exit_code, None);
        }
        other => panic!("the slot must not still be Running: {other:?}"),
    }
    assert!(
        slot.try_start(MODEL, &job.candidate).is_ok(),
        "the next candidate is admitted — one failed job does not wedge the slot for the \
         life of the process"
    );
}

/// Every failure named (spec §7), including the ones that happen before
/// anything is registered or probed: a candidate that is not a GGUF, and a
/// floor that cannot be read.
#[test]
fn a_candidate_that_is_not_a_gguf_is_named_and_nothing_is_probed() {
    let job = job_with("bad-gguf", None);
    job.seed_floor(&profile_doc(MODEL));
    let (runner, probes) = scripted_probes(vec![Ok(())]);
    let (gate, calls) = gate_answering(exited(0));
    let slot = SwapSlot::default();

    job.run(&runner, &gate, &slot).expect("the job records");

    assert!(probes.borrow().is_empty(), "nothing is probed");
    assert!(calls.borrow().is_empty(), "nothing is covered");
    assert!(job.swap_rows().is_empty(), "no verdict is invented");
    let degraded = job.degraded();
    assert_eq!(degraded.len(), 1, "{degraded:?}");
    assert!(
        degraded[0].contains("candidate.gguf"),
        "the degradation names the file it could not read: {degraded:?}"
    );
    assert_eq!(
        job.model_names(),
        vec![MODEL.to_string()],
        "a candidate that never registered leaves nothing to clean up"
    );
    match slot.snapshot() {
        SwapState::Done { report, .. } => {
            assert!(report.outcome.starts_with("infra: "), "{report:?}");
            assert_eq!(report.exit_code, None);
        }
        other => panic!("expected Done, got {other:?}"),
    }
}

#[test]
fn a_floor_that_cannot_be_read_is_named_and_nothing_is_probed() {
    let job = job("no-floor"); // no `seed_floor`: the baseline is simply absent
    let (runner, probes) = scripted_probes(vec![Ok(())]);
    let (gate, calls) = gate_answering(exited(0));
    let slot = SwapSlot::default();

    job.run(&runner, &gate, &slot).expect("the job records");

    assert!(probes.borrow().is_empty(), "nothing is probed");
    assert!(calls.borrow().is_empty(), "nothing is covered");
    assert!(job.swap_rows().is_empty(), "no verdict is invented");
    let degraded = job.degraded();
    assert_eq!(degraded.len(), 1, "{degraded:?}");
    assert!(
        degraded[0].contains("baseline"),
        "the degradation names the floor it could not read: {degraded:?}"
    );
}
