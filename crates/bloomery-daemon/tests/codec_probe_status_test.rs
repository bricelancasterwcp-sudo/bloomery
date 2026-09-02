//! G4 codec probe: terminal statuses that ARE scored, and the infrastructure
//! aborts that never count as a fixture failure (protocol §3).
//!
//! The distinction is the measurement rule: a model that answers badly scores
//! badly, but a probe that could not run at all leaves the model *unmeasured*
//! -- which reads fail-closed, and is never a confident zero.
//!
//! Split out of `codec_probe_test.rs` on 2026-09-01 (carried-debt slice D).

mod common;

use bloomery_core::journal::Journal;
use bloomery_daemon::agents::ImageStore;
use bloomery_daemon::codec_probe::fixtures::{parse_fixture_set, FixtureSet};
use bloomery_daemon::codec_probe::{
    gate_decision, run_codec_probe, FIXTURE_BUDGET_TOKENS, FIXTURE_MAX_STEPS,
};
use bloomery_daemon::pager::Pager;
use bloomery_substrate::fake::FakeSubstrate;
use bloomery_substrate::Reply;
use std::sync::Mutex;

use common::codec::{
    build_pager, done, fixture_events, fixture_rows, fresh_dir, meta, pager_events, read,
    removed_agents, sr_patch, test_set, verdict_events, MODEL,
};

// ---------------------------------------------------------------------------
// Terminal statuses that ARE scored (protocol §3)
// ---------------------------------------------------------------------------

/// `StepsExhausted` is a scored outcome, not an abort — and the step count
/// proves `FIXTURE_MAX_STEPS` reached the `TaskSpec`.
#[test]
fn steps_exhausted_is_scored_not_aborted() {
    let dir = fresh_dir("steps-exhausted");
    let mut replies = Vec::new();
    for _ in 0..(FIXTURE_MAX_STEPS * 2) {
        replies.push(read("a.txt"));
    }
    let pager = Mutex::new(build_pager(&dir, replies));

    let result =
        run_codec_probe(&pager, MODEL, &test_set(), &dir.join("scratch")).expect("probe completes");

    assert_eq!(result.landed, 0);
    assert_eq!(result.n, 2);
    let rows = fixture_rows(&pager_events(&dir));
    assert_eq!(rows.len(), 2);
    for row in &rows {
        assert_eq!(row.3, FIXTURE_MAX_STEPS, "max_steps came from the constant");
        assert_eq!(row.4, "StepsExhausted");
    }
}

/// `BudgetExhausted` is a scored outcome too (protocol §3), sitting in the
/// exact same match arm as `Done`/`StepsExhausted` in `run_one_fixture` —
/// right beside the arm that aborts the whole probe on `Error`. Nothing else
/// in this file exercises it, so a mutation moving `BudgetExhausted` into the
/// abort arm would otherwise survive the suite unnoticed.
///
/// Fixture 1 (t1-alpha) gets a single scripted `read` whose reported
/// `completion_tokens` alone overruns `FIXTURE_BUDGET_TOKENS`; the agent's
/// pager-level `Budget` (`Pager::infer`'s pre-substrate `check`) is what
/// stops step 2 — never a "script exhausted" substrate error, which is the
/// trigger the abort tests below use instead. Fixture 2 (t2-beta) gets a
/// brand-new agent and a fresh budget from `create_agent`, and lands
/// normally, proving the probe kept going past fixture 1's exhaustion rather
/// than treating it as an infrastructure failure.
#[test]
fn budget_exhausted_is_scored_not_aborted() {
    let dir = fresh_dir("budget-exhausted");
    let exhausting_read = Reply {
        text: "<action verb=\"read\" path=\"a.txt\">\n</action>".to_string(),
        prompt_tokens: Some(8),
        completion_tokens: Some(FIXTURE_BUDGET_TOKENS as u32),
        duration_ms: 1,
    };
    let pager = Mutex::new(build_pager(
        &dir,
        vec![
            exhausting_read,
            sr_patch("b.txt", "broken", "fixed"),
            done("repaired b.txt"),
        ],
    ));

    let result = run_codec_probe(&pager, MODEL, &test_set(), &dir.join("scratch"))
        .expect("a BudgetExhausted fixture must be SCORED, never abort the probe");

    assert_eq!(result.n, 2, "both fixtures ran");
    assert_eq!(result.landed, 1, "only t2-beta landed");

    let events = pager_events(&dir);
    let rows = fixture_rows(&events);
    assert_eq!(rows.len(), 2, "both fixtures produced a CodecFixture row");
    assert_eq!(rows[0].0, "t1-alpha");
    assert!(
        !rows[0].2,
        "no patch step ran before the budget ran out — not landed"
    );
    assert_eq!(rows[0].3, 1, "only the exhausting read step ran");
    assert_eq!(
        rows[0].4, "BudgetExhausted",
        "with no patch step, detail falls back to the terminal status"
    );
    assert_eq!(rows[1].0, "t2-beta");
    assert!(rows[1].2, "t2-beta landed on its own fresh budget");

    assert_eq!(
        verdict_events(&events).len(),
        1,
        "a BudgetExhausted fixture is scored, so the probe still completes \
         and journals exactly one CodecVerdict — never an abort"
    );

    let p = pager.lock().unwrap();
    assert!(
        p.status().models[0].codec_gate.is_some(),
        "a completed probe is a measurement recorded on the pager, not \
         'unmeasured' — which is what an aborted probe would leave behind"
    );
    drop(p);
}

/// Amendment 1 (docs/superpowers/evidence/2026-08-15-g4-protocol.md §9): a
/// mid-fixture window exhaustion (`PagerError::PromptTooLarge`, mapped by
/// `run_task` to `TaskStatus::WindowExhausted`) is SCORED by the same §3 arm
/// as `Done`/`StepsExhausted`/`BudgetExhausted` — never an abort. Sits in
/// `run_one_fixture`'s status match beside `budget_exhausted_is_scored_
/// not_aborted` above, for the same "a mutation moving this into the abort
/// arm survives unnoticed" reason.
///
/// Fixture 1 (t1-alpha)'s target file is padded to ~4000 bytes so a single
/// `read` observation, folded into the transcript, pushes the SECOND turn's
/// prompt over a `training_ctx` window small enough (1600 tokens) to still
/// admit the first, short turn — `Pager::infer`'s own arithmetic gate is
/// what refuses for real, not a substrate-error stand-in (the abort tests
/// below use "script exhausted" instead). Fixture 2 (t2-beta) gets a
/// brand-new agent — same window, but a short transcript — and lands
/// normally in `[patch, done]`, proving the probe kept going past fixture
/// 1's window exhaustion rather than treating it as an infrastructure
/// failure.
#[test]
fn window_exhausted_is_scored_not_aborted() {
    let dir = fresh_dir("window-exhausted");
    let big = "z".repeat(4000);
    let set_toml = format!(
        r#"
set = "codec-tasks-window-test"

[[fixture]]
name = "t1-alpha"
lens = "plaintext"
target = "a.txt"
goal = "fix the broken line in a.txt"

[[fixture.file]]
path = "a.txt"
contents = "{big}"

[fixture.reference]
search = "broken"
replace = "fixed"

[[fixture]]
name = "t2-beta"
lens = "plaintext"
target = "b.txt"
goal = "fix the broken line in b.txt"

[[fixture.file]]
path = "b.txt"
contents = "beta\nbroken\n"

[fixture.reference]
search = "broken"
replace = "fixed"
"#
    );
    let set = parse_fixture_set(&set_toml).expect("inline window-test fixture set parses");

    let journal = Journal::open(&dir.join("pager.jsonl")).unwrap();
    let images = ImageStore::new(&dir.join("img")).unwrap();
    let mut fake = FakeSubstrate::new();
    for r in [
        read("a.txt"),
        sr_patch("b.txt", "broken", "fixed"),
        done("repaired b.txt"),
    ] {
        fake.script_reply(r);
    }
    let mut pager = Pager::new(fake, journal, images, Box::new(|| Some(1024 * 1024 * 1024)));
    let gguf = dir.join("m.gguf");
    std::fs::write(&gguf, b"fake weights").unwrap();
    let small_window_meta = bloomery_core::gguf::GgufMeta {
        training_ctx: 1600,
        ..meta()
    };
    pager
        .register_model(MODEL, &gguf, small_window_meta, None)
        .unwrap();
    pager.set_task_journal_path(dir.join("tasks.jsonl"));
    let pager = Mutex::new(pager);

    let result = run_codec_probe(&pager, MODEL, &set, &dir.join("scratch"))
        .expect("a WindowExhausted fixture must be SCORED, never abort the probe");

    assert_eq!(result.n, 2, "both fixtures ran");
    assert_eq!(result.landed, 1, "only t2-beta landed");

    let events = pager_events(&dir);
    let rows = fixture_rows(&events);
    assert_eq!(rows.len(), 2, "both fixtures produced a CodecFixture row");
    assert_eq!(rows[0].0, "t1-alpha");
    assert!(
        !rows[0].2,
        "no patch step landed before the window exhausted — not landed"
    );
    assert_eq!(rows[0].3, 1, "only the completed read step ran");
    assert_eq!(
        rows[0].4, "WindowExhausted",
        "with no patch step, detail falls back to the terminal status"
    );
    assert_eq!(rows[1].0, "t2-beta");
    assert!(
        rows[1].2,
        "t2-beta landed on its own fresh, short transcript"
    );
    assert_eq!(rows[1].3, 2, "[patch, done]");

    assert_eq!(
        verdict_events(&events).len(),
        1,
        "a WindowExhausted fixture is scored, so the probe still completes \
         and journals exactly one CodecVerdict — never an abort"
    );

    let p = pager.lock().unwrap();
    assert!(
        p.status().models[0].codec_gate.is_some(),
        "a completed probe is a measurement recorded on the pager, not \
         'unmeasured' — which is what an aborted probe would leave behind"
    );
    drop(p);
}

// ---------------------------------------------------------------------------
// Infrastructure aborts (protocol §3) — never a fixture failure
// ---------------------------------------------------------------------------

/// A substrate failure mid-probe is an INFRASTRUCTURE abort: the whole probe
/// stops, no verdict is written, no partial score is spliced, and the model
/// is left *unmeasured* — which reads fail-closed but is never a confident
/// zero.
#[test]
fn a_substrate_failure_aborts_the_probe_without_any_verdict() {
    let dir = fresh_dir("substrate-error");
    // Empty script: `FakeSubstrate` errors with "script exhausted" on the
    // very first `infer`, which `run_task` turns into `TaskStatus::Error`.
    let pager = Mutex::new(build_pager(&dir, vec![]));

    let err = run_codec_probe(&pager, MODEL, &test_set(), &dir.join("scratch"))
        .expect_err("a substrate failure must abort");
    assert!(
        err.reason.contains("t1-alpha"),
        "the abort reason must name the fixture it happened on: {}",
        err.reason
    );

    let p = pager.lock().unwrap();
    assert!(
        !p.model_mutating_verbs(MODEL),
        "an aborted probe leaves the model fail-closed"
    );
    assert!(
        p.status().models[0].codec_gate.is_none(),
        "unmeasured is the ABSENCE of a gate, never a zero-landed one"
    );
    drop(p);

    let events = pager_events(&dir);
    assert!(
        verdict_events(&events).is_empty(),
        "no verdict may be recorded for an aborted probe"
    );
    assert!(
        fixture_events(&events).is_empty(),
        "this abort lands on the FIRST fixture, so no fixture row was written \
         yet — see `a_mid_set_abort_leaves_orphan_fixture_rows_and_no_verdict` \
         for what an abort partway through a set really leaves behind"
    );
    assert_eq!(
        removed_agents(&events).len(),
        1,
        "the fixture's agent is still removed on the abort path"
    );
}

/// The abort case the first-fixture tests cannot show: fixture 1 lands and is
/// journaled, then fixture 2 aborts. The `CodecFixture` row for fixture 1
/// **stays** — an append-only journal cannot retract it, and it is a useful
/// diagnostic record of what actually ran.
///
/// That is protocol §3 exactly, and the honesty rule it implies is what this
/// test pins: what is forbidden is a *score*, and the only thing separating
/// these orphan rows from a completed probe — for a journal replayer, an
/// operator, or a later analyst — is the **absence of a matching
/// `CodecVerdict`**. Orphan rows must never be summed into a rate by hand;
/// 1-of-1 here is not a 100% landing rate, it is one row from a measurement
/// that never finished. The gate itself stays unmeasured and fail-closed.
#[test]
fn a_mid_set_abort_leaves_orphan_fixture_rows_and_no_verdict() {
    let dir = fresh_dir("mid-set-abort");
    // Exactly enough script for fixture 1 to land. Fixture 2's very first
    // `infer` drains the queue → "script exhausted" → `TaskStatus::Error`.
    let pager = Mutex::new(build_pager(
        &dir,
        vec![sr_patch("a.txt", "broken", "fixed"), done("repaired a.txt")],
    ));

    let err = run_codec_probe(&pager, MODEL, &test_set(), &dir.join("scratch"))
        .expect_err("the second fixture must abort the whole probe");
    assert!(
        err.reason.contains("t2-beta"),
        "the abort names the fixture it happened on, not the one that passed: {}",
        err.reason
    );

    let events = pager_events(&dir);
    let rows = fixture_rows(&events);
    assert_eq!(
        rows.len(),
        1,
        "the fixture that completed before the abort keeps its diagnostic row: {rows:?}"
    );
    assert_eq!(rows[0].0, "t1-alpha");
    assert!(rows[0].2, "and keeps its real landed value");

    assert!(
        verdict_events(&events).is_empty(),
        "the absence of a CodecVerdict is the ONLY thing marking those rows \
         as orphans — an aborted probe must never emit one"
    );

    let p = pager.lock().unwrap();
    assert!(
        p.status().models[0].codec_gate.is_none(),
        "1-of-1 orphan rows are not a 100% gate; the model stays unmeasured"
    );
    assert!(!p.model_mutating_verbs(MODEL), "and therefore read-only");
}

/// The other §3 abort trigger: `create_agent` itself refusing (here law 5's
/// unprofiled admission gate). Same consequence — no verdict, unmeasured.
#[test]
fn a_refused_agent_creation_aborts_the_probe_without_any_verdict() {
    let dir = fresh_dir("create-refused");
    let mut p = build_pager(&dir, vec![]);
    p.set_allow_unprofiled(false);
    let pager = Mutex::new(p);

    let err = run_codec_probe(&pager, MODEL, &test_set(), &dir.join("scratch"))
        .expect_err("a refused agent creation must abort");
    assert!(
        err.reason.contains("t1-alpha"),
        "the abort reason must name the fixture: {}",
        err.reason
    );
    assert!(
        err.reason.contains("capability profile"),
        "and carry the refusal's own diagnostic: {}",
        err.reason
    );

    let p = pager.lock().unwrap();
    assert!(!p.model_mutating_verbs(MODEL));
    assert!(p.status().models[0].codec_gate.is_none());
    drop(p);

    let events = pager_events(&dir);
    assert!(verdict_events(&events).is_empty());
    assert!(fixture_events(&events).is_empty());
}

/// A set with no fixtures is a broken instrument, not a vacuous keep:
/// `gate_decision(0, 0)` is arithmetically `true`, so scoring an empty set
/// would hand out mutating verbs on zero evidence — the exact fail-open this
/// gate exists to prevent.
#[test]
fn an_empty_fixture_set_aborts_rather_than_scoring_a_vacuous_keep() {
    let dir = fresh_dir("empty-set");
    let pager = Mutex::new(build_pager(&dir, vec![]));
    let empty = FixtureSet {
        set: "codec-tasks-empty".to_string(),
        fixtures: Vec::new(),
    };

    let err = run_codec_probe(&pager, MODEL, &empty, &dir.join("scratch"))
        .expect_err("an empty set must abort");
    assert!(err.reason.contains("no fixtures"), "got {}", err.reason);

    assert!(
        gate_decision(0, 0),
        "the arithmetic really is vacuously true"
    );
    let p = pager.lock().unwrap();
    assert!(!p.model_mutating_verbs(MODEL));
    assert!(p.status().models[0].codec_gate.is_none());
    drop(p);
    assert!(verdict_events(&pager_events(&dir)).is_empty());
}

/// A poisoned pager lock is infrastructure failure too, not a measurement:
/// the probe refuses rather than recovering into an unvouched-for pager
/// (`api_native::lock_pager`'s sticky-poison reasoning).
#[test]
fn a_poisoned_pager_lock_aborts_the_probe() {
    let dir = fresh_dir("poisoned");
    let pager = Mutex::new(build_pager(&dir, vec![]));
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _guard = pager.lock().unwrap();
        panic!("poison the pager lock");
    }));
    assert!(pager.is_poisoned());

    let err = run_codec_probe(&pager, MODEL, &test_set(), &dir.join("scratch"))
        .expect_err("a poisoned lock must abort");
    assert!(err.reason.contains("poisoned"), "got reason {}", err.reason);
}
