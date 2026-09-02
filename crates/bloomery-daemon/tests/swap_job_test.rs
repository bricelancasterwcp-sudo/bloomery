//! The swap-candidate job's happy path and lifecycle: a covered candidate
//! journals its verdict with both digests, the retained document outlives the
//! next job, a probe failure degrades without inventing a verdict, the
//! scratch registration never outlives the job, and the slot admits one job
//! at a time.
//!
//! **Split again 2026-09-01** (carried-debt slice D): the first split left
//! this at 794 lines -- six under the ceiling, which is how the count crept
//! up in the first place. The refusal and infrastructure rows are now in
//! `swap_job_refusal_test.rs`.

mod common;

use bloomery_core::journal::{sha256_hex_bytes, Event};
use bloomery_daemon::swap::{cover_argv, SwapSlot, SwapState, NOTE_HANDOVER, NOTE_TASK_GATES};
use std::path::Path;

use common::swap::{exited, gate_answering};

use common::swap_job::{job, profile_doc, scripted_probes, scripted_probes_measuring, MODEL};

/// A `SwapCandidate` row's `(candidate_profile_path, candidate_profile_sha)` —
/// the pair that has to keep naming real bytes for the row to be evidence.
fn row_document(row: &Event) -> (String, String) {
    match row {
        Event::SwapCandidate {
            candidate_profile_path,
            candidate_profile_sha,
            ..
        } => (
            candidate_profile_path.clone(),
            candidate_profile_sha
                .clone()
                .expect("a covered verdict digests the document it read"),
        ),
        other => panic!("expected SwapCandidate, got {other:?}"),
    }
}

/// A report to hand [`SwapSlot::finish`] in a slot-only test: the slot stores
/// whatever it is given and reads nothing out of it.
fn finished_report() -> bloomery_daemon::swap::SwapOutcomeReport {
    bloomery_daemon::swap::SwapOutcomeReport {
        outcome: "covered".to_string(),
        exit_code: Some(0),
        candidate_gguf_sha: "0".repeat(64),
        floor_sha: "1".repeat(64),
        candidate_profile_path: "/d/candidate.json".to_string(),
        notes: [NOTE_TASK_GATES, NOTE_HANDOVER],
    }
}

// ---------------------------------------------------------------------------
// The job — the verdict, journaled with digests (design §4)
// ---------------------------------------------------------------------------

#[test]
fn a_covered_candidate_journals_the_verdict_with_digests() {
    let job = job("covered");
    let floor_doc = profile_doc(MODEL);
    job.seed_floor(&floor_doc);
    let (runner, probes) = scripted_probes(vec![Ok(())]);
    let (gate, calls) = gate_answering(exited(0));
    let slot = SwapSlot::default();
    slot.try_start(MODEL, &job.candidate)
        .expect("the slot is idle");

    job.run(&runner, &gate, &slot)
        .expect("the job records its result");

    let staging = job.staging();
    assert_eq!(
        probes.borrow().as_slice(),
        std::slice::from_ref(&staging),
        "the candidate is probed exactly once, into its own staging document"
    );
    let retained = job.retained_candidates();
    assert_eq!(
        retained.len(),
        1,
        "the probed document is retained content-named: {retained:?}"
    );
    let candidate_profile = retained[0].clone();
    assert!(
        !staging.exists(),
        "the document is moved to its content name, not copied — nothing is left \
         at the shared staging path for the next job to overwrite"
    );
    let spawned: Vec<Vec<String>> = calls
        .borrow()
        .iter()
        .map(|(_, argv)| argv.clone())
        .collect();
    assert_eq!(
        spawned,
        vec![cover_argv(&job.floor(), &candidate_profile)],
        "cover is spawned once, over the floor and the document just probed"
    );

    let events = job.events();
    assert_eq!(events.len(), 1, "one job, one row: {events:?}");
    match &events[0] {
        Event::SwapCandidate {
            model,
            candidate_gguf_sha,
            floor_path,
            floor_sha,
            candidate_profile_path,
            candidate_profile_sha,
            exit_code,
            outcome,
        } => {
            assert_eq!(model, MODEL);
            assert_eq!(outcome, "covered");
            assert_eq!(*exit_code, Some(0));
            assert_eq!(
                *candidate_gguf_sha,
                job.sha_of(&job.candidate),
                "the row's candidate digest is of the candidate GGUF's own bytes"
            );
            assert_eq!(
                *floor_sha,
                sha256_hex_bytes(floor_doc.as_bytes()),
                "the row's floor digest is of the blessed baseline's bytes, so `sha256sum` \
                 on the path it names checks the row"
            );
            assert_eq!(*floor_path, job.floor().display().to_string());
            assert_eq!(
                *candidate_profile_path,
                candidate_profile.display().to_string()
            );
            assert_eq!(
                candidate_profile_sha.as_deref(),
                Some(job.sha_of(&candidate_profile).as_str())
            );
        }
        other => panic!("expected SwapCandidate, got {other:?}"),
    }

    match slot.snapshot() {
        SwapState::Done { model, report } => {
            assert_eq!(model, MODEL);
            assert_eq!(report.outcome, "covered");
            assert_eq!(report.exit_code, Some(0));
            assert_eq!(report.candidate_gguf_sha, job.sha_of(&job.candidate));
            assert_eq!(report.floor_sha, sha256_hex_bytes(floor_doc.as_bytes()));
            assert_eq!(
                report.candidate_profile_path,
                candidate_profile.display().to_string()
            );
            assert_eq!(
                report.notes,
                [NOTE_TASK_GATES, NOTE_HANDOVER],
                "every report names the two gaps §4 requires it to name"
            );
        }
        other => panic!("expected Done, got {other:?}"),
    }
}

/// Design §4 step 3, and the promise the journal row makes: "anyone can re-run
/// the identical `cover` from the row alone". That promise only holds if the
/// document the row names outlives the next job — and every candidate offered
/// for a model probes into the *same* staging path, which the next probe
/// deletes before writing. So the document is retained content-named beside the
/// drift transients, and two jobs leave two documents, each still checkable
/// against the digest its own row recorded.
#[test]
fn a_retained_candidate_document_outlives_the_next_job() {
    let job = job("retained");
    job.seed_floor(&profile_doc(MODEL));
    let (gate, _calls) = gate_answering(exited(0));

    let (first_runner, _p) = scripted_probes(vec![Ok(())]);
    job.run(&first_runner, &gate, &SwapSlot::default())
        .expect("the first job records");
    let (first_path, first_sha) = row_document(&job.swap_rows()[0]);
    assert_eq!(job.sha_of(Path::new(&first_path)), first_sha);

    // A second candidate for the same model, measuring differently — so its
    // document differs in bytes, and therefore in content name.
    let (second_runner, _p) = scripted_probes_measuring(vec![Ok(())], 4096);
    job.run(&second_runner, &gate, &SwapSlot::default())
        .expect("the second job records");

    let rows = job.swap_rows();
    assert_eq!(rows.len(), 2, "{rows:?}");
    let (second_path, second_sha) = row_document(&rows[1]);
    assert_ne!(
        second_path, first_path,
        "two documents that differ in bytes are retained under two names"
    );
    assert_eq!(job.retained_candidates().len(), 2);
    assert_eq!(
        job.sha_of(Path::new(&first_path)),
        first_sha,
        "the FIRST row's document still checks out after a later job ran — the row is \
         re-runnable, not merely re-runnable-until-next-time"
    );
    assert_eq!(job.sha_of(Path::new(&second_path)), second_sha);
}

/// Spec §7: a probe failure is journaled as degraded and **no verdict is
/// invented**. There is no second document, so there is nothing to compare and
/// no `SwapCandidate` row to write — and cover must never be asked about a
/// document that does not exist.
#[test]
fn a_probe_failure_journals_degraded_and_reports_no_verdict() {
    let job = job("probe-failure");
    job.seed_floor(&profile_doc(MODEL));
    let (runner, _probes) = scripted_probes(vec![Err(4)]);
    let (gate, calls) = gate_answering(exited(0));
    let slot = SwapSlot::default();
    slot.try_start(MODEL, &job.candidate)
        .expect("the slot is idle");

    job.run(&runner, &gate, &slot)
        .expect("the job records its result");

    assert!(
        calls.borrow().is_empty(),
        "cover is never spawned without a candidate document: {:?}",
        calls.borrow()
    );
    assert!(
        job.swap_rows().is_empty(),
        "a failed probe reached no verdict, so it journals no verdict row"
    );
    let degraded = job.degraded();
    assert_eq!(degraded.len(), 1, "{degraded:?}");
    assert!(
        degraded[0].contains(MODEL) && degraded[0].contains("cannot reach model"),
        "the degradation must name the model and the probe's own words: {degraded:?}"
    );

    match slot.snapshot() {
        SwapState::Done { report, .. } => {
            assert!(
                report.outcome.starts_with("infra: "),
                "an infrastructure failure is not a verdict in either direction: {report:?}"
            );
            assert!(
                report.outcome.contains("cannot reach model"),
                "the probe's own words reach the operator without a re-run: {report:?}"
            );
            assert_eq!(
                report.exit_code, None,
                "no cover ran, so there is no exit code — `None`, never 0"
            );
        }
        other => panic!("expected Done, got {other:?}"),
    }
}

/// Spec §4: "The scratch identity never outlives the request." Pinned on both
/// the path that reached a verdict and the path that failed before one.
#[test]
fn the_scratch_registration_never_outlives_the_job() {
    for (tag, script) in [("scratch-ok", Ok(())), ("scratch-failed", Err(4))] {
        let job = job(tag);
        job.seed_floor(&profile_doc(MODEL));
        let (runner, _probes) = scripted_probes(vec![script]);
        let (gate, _calls) = gate_answering(exited(0));
        let slot = SwapSlot::default();

        job.run(&runner, &gate, &slot).expect("the job records");

        assert_eq!(
            job.model_names(),
            vec![MODEL.to_string()],
            "{tag}: the scratch identity is unloaded AND unregistered on every exit path, \
             and the configured model it stood beside is untouched"
        );
    }
}

/// Spec §4: "One candidate at a time … a second request while one runs gets
/// 409 `candidate_probe_in_progress` — no queue."
#[test]
fn the_slot_admits_one_job_at_a_time() {
    let slot = SwapSlot::default();
    assert!(slot.try_start("qwen", Path::new("/a.gguf")).is_ok());
    assert!(
        slot.try_start("qwen", Path::new("/b.gguf")).is_err(),
        "a running job holds the slot against every other claim"
    );

    slot.finish("qwen", finished_report());
    assert!(
        slot.try_start("qwen", Path::new("/b.gguf")).is_ok(),
        "a finished job releases the slot — the bound is one at a time, not one ever"
    );
}
