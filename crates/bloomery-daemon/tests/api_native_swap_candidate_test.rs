//! Native HTTP API: the swap-candidate routes (swap-candidate seam design
//! §4).
//!
//! `POST /models/{name}/swap-candidate` starts one job and answers `202` at
//! once — a probe holds VRAM for ~10 minutes, so it cannot ride a request
//! handler — and `GET` on the same path is where the verdict appears.
//!
//! Split out of `api_native_test.rs` on 2026-09-01 (carried-debt slice D);
//! the probe's admission window is in `api_native_swap_window_test.rs`.

mod common;

use std::path::Path;
use std::sync::Arc;

use bloomery_daemon::swap::{scratch_identity, SwapOutcomeReport, NOTE_HANDOVER, NOTE_TASK_GATES};
use common::http;
use common::native::{serve_swap, value_of, SWAP_MODEL};

// ---------------------------------------------------------------------------

/// The 202 row, end to end: the handler answers immediately, the worker runs
/// the whole design-§4 flow on its own thread, and `GET` hands back the report
/// the slot was finished with.
#[test]
fn posting_a_candidate_answers_202_and_the_job_reaches_a_verdict() {
    let fixture = serve_swap(0);
    fixture.seed_floor();

    let (st, body) = fixture.post(&fixture.body());
    assert_eq!(st, 202, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["model"], SWAP_MODEL);
    assert_eq!(v["candidate"], fixture.candidate.display().to_string());
    assert_eq!(v["state"], "running");

    let done = fixture.poll_until_done();
    assert_eq!(done["state"], "done", "{done}");
    assert_eq!(done["model"], SWAP_MODEL);
    let report = &done["report"];
    assert_eq!(report["outcome"], "covered");
    assert_eq!(report["exit_code"], 0);
    assert_eq!(
        report["candidate_gguf_sha"],
        bloomery_daemon::agents::model_digest(&fixture.candidate).unwrap(),
        "the report's digest is of the candidate's own bytes"
    );
    assert_eq!(
        report["notes"],
        serde_json::json!([NOTE_TASK_GATES, NOTE_HANDOVER]),
        "every report names both gaps, in design §4's order"
    );

    // The probe ran against THIS daemon's own `/v1`, under the scratch
    // identity — the only thing that makes the candidate addressable at all.
    let probes = fixture.probes.lock().unwrap();
    assert_eq!(probes.len(), 1, "one job probes exactly once: {probes:?}");
    assert!(
        probes[0].contains(&format!("http://127.0.0.1:{}/v1", fixture.port)),
        "the probe must target the bound port: {:?}",
        probes[0]
    );
    assert_eq!(
        value_of(&probes[0], "--model"),
        scratch_identity(SWAP_MODEL)
    );

    // ...and cover compared the blessed floor against the document that probe
    // wrote, which is the document the report names.
    let covers = fixture.covers.lock().unwrap();
    assert_eq!(covers.len(), 1, "one job covers exactly once: {covers:?}");
    assert_eq!(
        covers[0].last().map(String::as_str),
        report["candidate_profile_path"].as_str(),
        "the covered document is the one the report names"
    );

    let rows: Vec<_> = fixture
        .events()
        .into_iter()
        .filter(|e| matches!(e, bloomery_core::journal::Event::SwapCandidate { .. }))
        .collect();
    assert_eq!(rows.len(), 1, "one verdict, one row: {rows:?}");
    drop(probes);
    drop(covers);
    fixture.handle.shutdown();
}

/// 404: a name this daemon was never configured with, answered with the
/// surface's one `unknown_model` shape and nothing started.
#[test]
fn posting_a_candidate_for_an_unknown_model_is_404() {
    let fixture = serve_swap(0);
    fixture.seed_floor();

    let (st, body) = http(
        &fixture.addr(),
        "POST",
        "/models/does-not-exist/swap-candidate",
        &fixture.body(),
    );
    assert_eq!(st, 404, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["error"], "unknown_model");
    assert_eq!(v["model"], "does-not-exist");
    assert!(
        fixture.probes.lock().unwrap().is_empty(),
        "a refused request probes nothing"
    );
    fixture.handle.shutdown();
}

/// 400: the three ways a request body can fail to name a candidate this
/// daemon could probe — not JSON at all, JSON without `gguf_path`, and a
/// `gguf_path` naming bytes that cannot be read. All three are the surface's
/// one `bad_request` shape, and none of them starts a job.
#[test]
fn a_candidate_request_that_names_no_readable_gguf_is_400() {
    let fixture = serve_swap(0);
    fixture.seed_floor();
    let missing = fixture.dir.join("nothing-here.gguf");
    let names_nothing = serde_json::json!({"gguf_path": missing.display().to_string()}).to_string();

    for (body, expected) in [
        ("not json at all", "expected"),
        (r#"{"model":"qwen"}"#, "gguf_path"),
        (names_nothing.as_str(), "nothing-here.gguf"),
    ] {
        let (st, response) = fixture.post(body);
        assert_eq!(st, 400, "{body}: {response}");
        let v: serde_json::Value = serde_json::from_str(&response).unwrap();
        assert_eq!(v["error"], "bad_request", "{body}");
        assert!(
            v["message"].as_str().unwrap_or_default().contains(expected),
            "{body}: {response}"
        );
    }
    assert!(
        fixture.probes.lock().unwrap().is_empty(),
        "a refused request probes nothing"
    );
    fixture.handle.shutdown();
}

/// 409: no blessed baseline. The floor is the operator-endorsed capability
/// statement (design §4's precondition), so there is nothing to cover against
/// and the refusal names the document it looked for — never a probe run
/// against a floor nobody blessed.
#[test]
fn posting_a_candidate_with_no_blessed_baseline_is_409_no_baseline() {
    let fixture = serve_swap(0);

    let (st, body) = fixture.post(&fixture.body());
    assert_eq!(st, 409, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["error"], "no_baseline");
    assert_eq!(v["model"], SWAP_MODEL);
    assert!(
        v["detail"]
            .as_str()
            .unwrap_or_default()
            .contains(&format!("{SWAP_MODEL}.baseline.json")),
        "the refusal names the document it looked for: {body}"
    );
    assert!(
        fixture.probes.lock().unwrap().is_empty(),
        "a refused request probes nothing"
    );
    fixture.handle.shutdown();
}

/// 409: one candidate at a time (design §4 — a probe holds VRAM for ~10
/// minutes, and there is no queue). The slot is claimed by the request thread
/// before any worker starts, so the second request is refused synchronously
/// and names what is running.
#[test]
fn a_second_candidate_while_one_runs_is_409_candidate_probe_in_progress() {
    let fixture = serve_swap(0);
    fixture.seed_floor();
    fixture
        .ctx
        .slot()
        .try_start("some-other-model", Path::new("/models/other.gguf"))
        .expect("the slot starts idle");

    let (st, body) = fixture.post(&fixture.body());
    assert_eq!(st, 409, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["error"], "candidate_probe_in_progress");
    assert_eq!(v["model"], SWAP_MODEL);
    assert!(
        v["detail"]
            .as_str()
            .unwrap_or_default()
            .contains("some-other-model"),
        "the refusal says WHAT is running, not only that something is: {body}"
    );
    assert!(
        fixture.probes.lock().unwrap().is_empty(),
        "a refused request probes nothing"
    );
    fixture.handle.shutdown();
}

/// `GET` while a job runs: `running`, with no report — a verdict nobody
/// reached is never rendered as one.
#[test]
fn getting_a_running_job_reads_running() {
    let fixture = serve_swap(0);
    fixture
        .ctx
        .slot()
        .try_start(SWAP_MODEL, Path::new("/models/candidate.gguf"))
        .expect("the slot starts idle");

    let (st, body) = fixture.get();
    assert_eq!(st, 200, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["model"], SWAP_MODEL);
    assert_eq!(v["state"], "running");
    assert!(v["report"].is_null(), "{body}");
    fixture.handle.shutdown();
}

/// `GET` on a finished job renders the slot's report field for field —
/// including the `"unread"` sentinel, which is what a digest field carries
/// when the job never got a digest to put there. It only ever appears beside
/// an `"infra: …"` outcome, and it is a fixed word, not a short digest.
#[test]
fn getting_a_finished_job_reads_the_report_verbatim() {
    let fixture = serve_swap(0);
    fixture.ctx.slot().finish(
        SWAP_MODEL,
        SwapOutcomeReport {
            outcome: "infra: the candidate weights could not be read".to_string(),
            exit_code: None,
            candidate_gguf_sha: "unread".to_string(),
            floor_sha: "unread".to_string(),
            candidate_profile_path: "/profiles/qwen!swap-candidate.confirm.json".to_string(),
            notes: [NOTE_TASK_GATES, NOTE_HANDOVER],
        },
    );

    let (st, body) = fixture.get();
    assert_eq!(st, 200, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["model"], SWAP_MODEL);
    assert_eq!(v["state"], "done");
    assert_eq!(
        v["report"]["outcome"],
        "infra: the candidate weights could not be read"
    );
    assert!(v["report"]["exit_code"].is_null());
    assert_eq!(v["report"]["candidate_gguf_sha"], "unread");
    assert_eq!(v["report"]["floor_sha"], "unread");
    assert_eq!(
        v["report"]["candidate_profile_path"],
        "/profiles/qwen!swap-candidate.confirm.json"
    );
    assert_eq!(
        v["report"]["notes"],
        serde_json::json!([NOTE_TASK_GATES, NOTE_HANDOVER])
    );
    fixture.handle.shutdown();
}

/// 404: nothing was ever asked about this model. A slot holding some *other*
/// model's job reads the same way — that job says nothing about this name.
#[test]
fn getting_a_job_that_never_started_is_404() {
    let fixture = serve_swap(0);

    let (st, body) = fixture.get();
    assert_eq!(st, 404, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["error"], "no_swap_candidate");
    assert_eq!(v["model"], SWAP_MODEL);

    fixture
        .ctx
        .slot()
        .try_start("some-other-model", Path::new("/models/other.gguf"))
        .expect("the slot starts idle");
    let (st, body) = fixture.get();
    assert_eq!(st, 404, "another model's job is not this model's: {body}");
    fixture.handle.shutdown();
}

/// **The advisory pin** (design §4: "Nothing blocks, nothing auto-swaps").
/// A `not-covered` verdict standing in the slot changes nothing about
/// admission: the named model still admits an agent, exactly as it would have
/// with the slot empty.
#[test]
fn a_not_covered_verdict_never_blocks_admission() {
    let fixture = serve_swap(1);
    fixture.ctx.slot().finish(
        SWAP_MODEL,
        SwapOutcomeReport {
            outcome: "not-covered".to_string(),
            exit_code: Some(1),
            candidate_gguf_sha: "a".repeat(64),
            floor_sha: "b".repeat(64),
            candidate_profile_path: "/profiles/qwen!swap-candidate.transient-abcdef12.json"
                .to_string(),
            notes: [NOTE_TASK_GATES, NOTE_HANDOVER],
        },
    );

    let (st, body) = http(
        &fixture.addr(),
        "POST",
        "/agents",
        r#"{"model":"qwen","budget_tokens":1000}"#,
    );
    assert_eq!(
        st, 201,
        "a swap verdict is evidence for an operator, never an admission gate: {body}"
    );
    fixture.handle.shutdown();
}

/// A daemon served without a swap-candidate context (every `serve`/
/// `serve_shared` caller — test fixtures, embedders) says so by name rather
/// than answering a verdict it has no interpreter, no store and no port to
/// reach.
#[test]
fn a_daemon_wired_without_a_swap_context_says_so() {
    let (port, handle) = bloomery_daemon::test_support::serve_fake();
    let addr = format!("127.0.0.1:{port}");

    for (method, body) in [("POST", r#"{"gguf_path":"/tmp/c.gguf"}"#), ("GET", "")] {
        let (st, response) = http(&addr, method, "/models/qwen/swap-candidate", body);
        assert_eq!(st, 501, "{method}: {response}");
        let v: serde_json::Value = serde_json::from_str(&response).unwrap();
        assert_eq!(v["error"], "swap_candidate_unavailable", "{method}");
        assert_eq!(v["model"], "qwen", "{method}");
    }
    handle.shutdown();
}

/// **Obligation: a panicking worker must never wedge the one slot.** Step 7's
/// cleanup is explicit, not a drop guard, so an unwind past the registration
/// would otherwise leave the slot `Running` for the life of the process —
/// every later candidate answered `candidate_probe_in_progress` for a job
/// nobody can see. The spawn site catches it, finishes the slot with an
/// `infra:` report naming the panic, and the next candidate is admitted.
#[test]
fn a_panicking_candidate_job_never_wedges_the_slot() {
    let fixture = serve_swap(0);
    fixture.seed_floor();
    *fixture.hook.lock().unwrap() = Some(Arc::new(|| panic!("the probe blew up")));

    let (st, body) = fixture.post(&fixture.body());
    assert_eq!(st, 202, "{body}");
    let done = fixture.poll_until_done();
    assert_eq!(done["state"], "done", "{done}");
    let outcome = done["report"]["outcome"].as_str().unwrap_or_default();
    assert!(
        outcome.starts_with("infra:") && outcome.contains("the probe blew up"),
        "the caught panic is named, never rendered as a verdict: {outcome}"
    );
    assert!(
        done["report"]["exit_code"].is_null(),
        "a panic reached no exit code: {done}"
    );

    // The slot admits the next job — the whole point of catching it.
    *fixture.hook.lock().unwrap() = None;
    let (st, body) = fixture.post(&fixture.body());
    assert_eq!(st, 202, "a caught panic must not wedge the slot: {body}");
    let done = fixture.poll_until_done();
    assert_eq!(done["report"]["outcome"], "covered", "{done}");
    fixture.handle.shutdown();
}

/// **Obligation: the worker's `Err` is the only report that cleanup failed.**
/// A failed unregister leaves the scratch identity — possibly still holding
/// weights — standing after the job returned, which is the one thing design §4
/// says must not happen, and nothing in the report says so: the report carries
/// the verdict, which is unaffected. So the spawn site journals it.
///
/// Driven by having the probe remove the scratch registration out from under
/// the job: step 7 then fails against an otherwise healthy pager, which is
/// exactly the shape of the failure this row exists to catch.
#[test]
fn a_failed_cleanup_is_journaled_rather_than_dropped() {
    let fixture = serve_swap(0);
    fixture.seed_floor();
    let pager = Arc::clone(&fixture.pager);
    *fixture.hook.lock().unwrap() = Some(Arc::new(move || {
        pager
            .lock()
            .unwrap()
            .unregister_model(&scratch_identity(SWAP_MODEL))
            .expect("the scratch identity is registered while the probe runs");
    }));

    let (st, body) = fixture.post(&fixture.body());
    assert_eq!(st, 202, "{body}");
    let done = fixture.poll_until_done();
    assert_eq!(
        done["report"]["outcome"], "covered",
        "the verdict itself is unaffected by the cleanup failure: {done}"
    );
    let reasons = fixture.degraded_reasons();
    assert!(
        reasons
            .iter()
            .any(|r| r.contains(&scratch_identity(SWAP_MODEL)) && r.contains("could not")),
        "a failed cleanup is journaled, never dropped: {reasons:?}"
    );
    fixture.handle.shutdown();
}
