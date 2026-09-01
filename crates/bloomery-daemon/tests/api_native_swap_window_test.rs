//! Native HTTP API: the candidate probe's admission window (bT5/F1).
//!
//! Design §4 step 2 requires the probe to reach the candidate "through the
//! daemon's own `/v1` with the identical POST invocation", which means a
//! scratch identity must be admissible for exactly as long as its job runs,
//! and no longer.
//!
//! Split out of `api_native_test.rs` on 2026-09-01 (carried-debt slice D).

mod common;

use std::sync::{Arc, Mutex};

use bloomery_daemon::swap::scratch_identity;
use common::http;
use common::native::{serve_swap_cfg, SwapCfg, SWAP_MODEL};

// ---------------------------------------------------------------------------

/// The production config these rows run under: law 5's gate really refuses,
/// and the probe really asks.
fn strict() -> SwapCfg {
    SwapCfg {
        allow_unprofiled: false,
        drive_v1: true,
        ..SwapCfg::default()
    }
}

/// **bT5/F1, the defect itself.** With `allow_unprofiled` unset — the standing
/// config on the box the live acceptance ran on — the candidate probe's own
/// `/v1/chat/completions` must be **admitted**, and the job must reach a
/// coverage verdict.
///
/// Before the fix this row reproduces the live failure exactly: `422` at the
/// door, `assay exited 4: … HTTP 422 …`, `outcome` an `infra:` sentence and
/// `exit_code: null`, because `cover` never ran.
#[test]
fn a_candidate_probe_is_admitted_through_this_daemons_own_v1() {
    let fixture = serve_swap_cfg(0, strict());
    fixture.seed_floor();
    let scratch = scratch_identity(SWAP_MODEL);

    let (st, body) = fixture.post(&fixture.body());
    assert_eq!(st, 202, "{body}");
    let done = fixture.poll_until_done();

    let calls = fixture.v1.lock().unwrap().clone();
    assert_eq!(
        calls,
        vec![(scratch.clone(), 200)],
        "the probe's own request must be admitted under the scratch identity, \
         not refused 422 at the door: {done}"
    );
    assert_eq!(
        done["report"]["outcome"], "covered",
        "an admitted probe reaches a real verdict: {done}"
    );
    assert_eq!(done["report"]["exit_code"], 0, "{done}");

    // And the window went with the job: closed, and the scratch identity is not
    // registered any more, so nothing is addressable under it at all.
    assert!(
        !fixture.window_open(),
        "the window is back to closed once the job ends: {done}"
    );
    let (st, body) = fixture.chat(&scratch);
    assert_eq!(
        st, 404,
        "the scratch identity never outlives the job: {body}"
    );
    fixture.handle.shutdown();
}

/// **The window admits the scratch identity and nothing else.** The near-miss
/// the live evidence names is worse than the failure: a candidate POST fired
/// *inside* the boot POST window would have been admitted by a daemon-global
/// flag, for a reason with nothing to do with this endpoint. So the fix is
/// scoped per identity, and this row is what says so — mid-probe, with the
/// window demonstrably open for the candidate, the configured-but-unprofiled
/// `qwen` is still refused `422`.
#[test]
fn the_candidate_window_admits_the_scratch_identity_and_nothing_else() {
    let fixture = serve_swap_cfg(0, strict());
    fixture.seed_floor();
    let neighbour: Arc<Mutex<Option<(u16, String)>>> = Arc::new(Mutex::new(None));

    let addr = fixture.addr();
    let seen = Arc::clone(&neighbour);
    // Runs inside the probe, i.e. inside the window: the candidate's own call
    // has already been admitted by the time this fires.
    *fixture.hook.lock().unwrap() = Some(Arc::new(move || {
        let answer = http(
            &addr,
            "POST",
            "/v1/chat/completions",
            r#"{"model":"qwen","messages":[{"role":"user","content":"hi"}],"max_tokens":8}"#,
        );
        *seen.lock().expect("neighbour slot") = Some(answer);
    }));

    let (st, body) = fixture.post(&fixture.body());
    assert_eq!(st, 202, "{body}");
    let done = fixture.poll_until_done();
    assert_eq!(done["report"]["outcome"], "covered", "{done}");
    assert_eq!(
        fixture.v1.lock().unwrap().clone(),
        vec![(scratch_identity(SWAP_MODEL), 200)],
        "the candidate itself was admitted: {done}"
    );

    let (st, body) = neighbour
        .lock()
        .unwrap()
        .clone()
        .expect("the hook ran inside the probe");
    assert_eq!(
        st, 422,
        "the window is the candidate's alone — an unprofiled configured model \
         must stay refused while it is open: {body}"
    );
    fixture.handle.shutdown();
}

/// **The window closes with the probe step, not with the job.** Nothing past
/// the probe drives `/v1`, so by the time `cover` runs — with the scratch
/// identity still registered, which is the only moment this is observable —
/// the window must already be shut.
#[test]
fn the_candidate_window_is_closed_before_the_verdict_is_reached() {
    let fixture = serve_swap_cfg(0, strict());
    fixture.seed_floor();
    let scratch = scratch_identity(SWAP_MODEL);
    let after: Arc<Mutex<Option<(u16, String)>>> = Arc::new(Mutex::new(None));

    let addr = fixture.addr();
    let (seen, named) = (Arc::clone(&after), scratch.clone());
    *fixture.cover_hook.lock().unwrap() = Some(Arc::new(move || {
        let answer = http(
            &addr,
            "POST",
            "/v1/chat/completions",
            &serde_json::json!({
                "model": named,
                "messages": [{"role": "user", "content": "hi"}],
                "max_tokens": 8,
            })
            .to_string(),
        );
        *seen.lock().expect("after-probe slot") = Some(answer);
    }));

    let (st, body) = fixture.post(&fixture.body());
    assert_eq!(st, 202, "{body}");
    let done = fixture.poll_until_done();
    assert_eq!(done["report"]["outcome"], "covered", "{done}");

    let (st, body) = after
        .lock()
        .unwrap()
        .clone()
        .expect("the hook ran inside the cover run");
    assert_eq!(
        st, 422,
        "the scratch identity is still registered here, and must already be \
         back under law 5's gate: {body}"
    );
    fixture.handle.shutdown();
}

/// **A failed probe still opens the window and still leaves nothing open.**
/// The job ends on its `infra:` path, and the daemon is back to refusing every
/// unprofiled model — the scratch identity because it is gone, `qwen` because
/// nothing daemon-wide was ever suspended for it.
#[test]
fn a_failed_candidate_probe_leaves_no_window_open() {
    let fixture = serve_swap_cfg(
        0,
        SwapCfg {
            probe_fails: true,
            ..strict()
        },
    );
    fixture.seed_floor();
    let scratch = scratch_identity(SWAP_MODEL);

    let (st, body) = fixture.post(&fixture.body());
    assert_eq!(st, 202, "{body}");
    let done = fixture.poll_until_done();
    let outcome = done["report"]["outcome"].as_str().unwrap_or_default();
    assert!(
        outcome.starts_with("infra:") && outcome.contains("assay exited 4"),
        "a failed probe is named, never rendered as a verdict: {done}"
    );
    assert_eq!(
        fixture.v1.lock().unwrap().clone(),
        vec![(scratch.clone(), 200)],
        "the window opened for the probe even on the path where it failed: {done}"
    );

    assert!(
        !fixture.window_open(),
        "the window is back to closed once the job ends: {done}"
    );
    let (st, body) = fixture.chat(&scratch);
    assert_eq!(
        st, 404,
        "the scratch identity never outlives the job: {body}"
    );
    let (st, body) = fixture.chat(SWAP_MODEL);
    assert_eq!(
        st, 422,
        "law 5's gate is exactly where the job found it: {body}"
    );
    fixture.handle.shutdown();
}

/// **A panicking worker must not leave the window open either.** The unwind
/// skips step 7, so the scratch identity really is still registered afterwards
/// (the spawn site says so and tells the operator to unload it) — and a window
/// left open on that identity would admit it, unprofiled, through `/v1` for the
/// life of the process. The spawn site closes it where it catches the panic.
#[test]
fn a_panicking_candidate_job_closes_the_admission_window() {
    let fixture = serve_swap_cfg(
        0,
        SwapCfg {
            allow_unprofiled: false,
            ..SwapCfg::default()
        },
    );
    fixture.seed_floor();
    let scratch = scratch_identity(SWAP_MODEL);
    *fixture.hook.lock().unwrap() = Some(Arc::new(|| panic!("the probe blew up")));

    let (st, body) = fixture.post(&fixture.body());
    assert_eq!(st, 202, "{body}");
    let done = fixture.poll_until_done();
    let outcome = done["report"]["outcome"].as_str().unwrap_or_default();
    assert!(
        outcome.starts_with("infra:") && outcome.contains("the probe blew up"),
        "the caught panic is named: {done}"
    );

    // The registration survived the unwind — that is the premise, and the
    // reason this path needs a close of its own.
    let (st, body) = http(&fixture.addr(), "GET", "/status", "");
    assert_eq!(st, 200, "{body}");
    let status: serde_json::Value = serde_json::from_str(&body).unwrap();
    let names: Vec<&str> = status["models"]
        .as_array()
        .expect("models")
        .iter()
        .filter_map(|m| m["name"].as_str())
        .collect();
    assert!(
        names.contains(&scratch.as_str()),
        "an unwind past step 2 leaks the registration; this row is about the \
         window on it: {names:?}"
    );

    assert!(
        !fixture.window_open(),
        "an unwind skips the job's own close; the spawn site owes this one"
    );
    let (st, body) = fixture.chat(&scratch);
    assert_eq!(
        st, 422,
        "a leaked registration must not also be a leaked admission: {body}"
    );
    fixture.handle.shutdown();
}

/// **Obligation: an agent minted inside the window must not outlive it.**
/// `unregister_model` used to only *suspend* agents bound to the identity it
/// forgot — they kept their id and their image, and were refused only because
/// no model of that name was registered any more. For a scratch identity that
/// refusal is temporary by construction: the next candidate job for the same
/// model registers exactly that name again, and admission is checked at agent
/// **creation**, so a stale agent would come back usable against a *different*
/// candidate's weights without passing any gate at all. Step 7 evicts instead.
#[test]
fn an_agent_minted_during_the_window_is_evicted_when_the_job_ends() {
    let fixture = serve_swap_cfg(0, strict());
    fixture.seed_floor();
    let minted: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));

    let (addr, seen) = (fixture.addr(), Arc::clone(&minted));
    // Inside the probe, i.e. inside the window: this is admitted for exactly
    // the same reason the probe's own call is.
    *fixture.hook.lock().unwrap() = Some(Arc::new(move || {
        let (st, body) = http(
            &addr,
            "POST",
            "/agents",
            &serde_json::json!({
                "model": scratch_identity(SWAP_MODEL),
                "budget_tokens": 1000,
            })
            .to_string(),
        );
        assert_eq!(st, 201, "the window admits agent creation too: {body}");
        let id = serde_json::from_str::<serde_json::Value>(&body).unwrap()["id"]
            .as_str()
            .expect("a created agent has an id")
            .to_string();
        *seen.lock().expect("minted slot") = Some(id);
    }));

    let (st, body) = fixture.post(&fixture.body());
    assert_eq!(st, 202, "{body}");
    let done = fixture.poll_until_done();
    assert_eq!(done["report"]["outcome"], "covered", "{done}");
    let id = minted.lock().unwrap().clone().expect("the hook ran");

    // GONE from the table, not merely refused: `/status` is the table.
    let (st, body) = http(&fixture.addr(), "GET", "/status", "");
    assert_eq!(st, 200, "{body}");
    let status: serde_json::Value = serde_json::from_str(&body).unwrap();
    let ids: Vec<&str> = status["agents"]
        .as_array()
        .expect("agents")
        .iter()
        .filter_map(|a| a["id"].as_str())
        .collect();
    assert!(
        !ids.contains(&id.as_str()),
        "an agent bound to the scratch identity cannot outlive it: {ids:?}"
    );
    let (st, body) = http(
        &fixture.addr(),
        "POST",
        &format!("/agents/{id}/infer"),
        r#"{"prompt":"hi","max_tokens":8}"#,
    );
    assert_eq!(st, 404, "the id is forgotten, not parked: {body}");
    fixture.handle.shutdown();
}

/// **Obligation: a second job revives nothing from the first.** Two things
/// have to hold across the re-registration that job 2 performs under the very
/// same scratch name: job 2 starts with a closed window (the structural
/// argument — a fresh entry's window is shut), and job 1's agent is not
/// waiting in the table to be revived by it (the eviction argument). The
/// second is checked from *inside* job 2's window, which is the exact moment a
/// survivor would become usable against the new candidate's weights.
#[test]
fn a_second_candidate_job_revives_nothing_from_the_first() {
    let fixture = serve_swap_cfg(0, strict());
    fixture.seed_floor();
    let minted: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));

    let (addr, seen) = (fixture.addr(), Arc::clone(&minted));
    *fixture.hook.lock().unwrap() = Some(Arc::new(move || {
        let (st, body) = http(
            &addr,
            "POST",
            "/agents",
            &serde_json::json!({
                "model": scratch_identity(SWAP_MODEL),
                "budget_tokens": 1000,
            })
            .to_string(),
        );
        assert_eq!(st, 201, "{body}");
        *seen.lock().expect("minted slot") = Some(
            serde_json::from_str::<serde_json::Value>(&body).unwrap()["id"]
                .as_str()
                .expect("a created agent has an id")
                .to_string(),
        );
    }));

    let (st, body) = fixture.post(&fixture.body());
    assert_eq!(st, 202, "{body}");
    let done = fixture.poll_until_done();
    assert_eq!(done["report"]["outcome"], "covered", "job 1: {done}");
    let id = minted.lock().unwrap().clone().expect("the hook ran");
    assert!(
        !fixture.window_open(),
        "job 2 must start from a closed window"
    );

    // Job 2, same model, same scratch name — the re-registration that would
    // revive a survivor. The probe hook asks whether job 1's agent works now.
    let revived: Arc<Mutex<Option<(u16, String)>>> = Arc::new(Mutex::new(None));
    let (addr, seen, stale) = (fixture.addr(), Arc::clone(&revived), id.clone());
    *fixture.hook.lock().unwrap() = Some(Arc::new(move || {
        let answer = http(
            &addr,
            "POST",
            &format!("/agents/{stale}/infer"),
            r#"{"prompt":"hi","max_tokens":8}"#,
        );
        *seen.lock().expect("revived slot") = Some(answer);
    }));

    let (st, body) = fixture.post(&fixture.body());
    assert_eq!(st, 202, "{body}");
    let done = fixture.poll_until_done();
    assert_eq!(done["report"]["outcome"], "covered", "job 2: {done}");

    let (st, body) = revived
        .lock()
        .unwrap()
        .clone()
        .expect("the hook ran inside job 2's probe");
    assert_eq!(
        st, 404,
        "job 1's agent must not come back usable against job 2's candidate, \
         which is what a re-registered name would otherwise do — admission is \
         checked at creation, and this agent would never be created again: {body}"
    );
    assert!(!fixture.window_open(), "the window closed with job 2 too");
    fixture.handle.shutdown();
}
