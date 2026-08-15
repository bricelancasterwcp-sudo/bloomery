//! The G4 codec probe engine (Phase 2b/2c P4 Task 9) — the instrument that
//! runs a model's fixture set through the real `run_task` and turns the
//! outcome into a `CodecGateResult`.
//!
//! Governing doc: `docs/superpowers/evidence/2026-08-15-g4-protocol.md`,
//! pre-registered before any of this existed. Every test here pins one of
//! its load-bearing measurement rules:
//! - §3's scoring conjunction — a fixture lands iff a `patch` step succeeded
//!   **and** the declared target file's bytes changed. Both legs get their
//!   own test (`identity_patch...`, `scratch_file_patch...`), because either
//!   leg alone would score a non-repair as a repair.
//! - §3's infrastructure-abort rule — a substrate failure or a refused
//!   agent creation is never a fixture failure: no verdict, no partial
//!   score, the model stays *unmeasured* (which reads fail-closed, and is
//!   never a confident zero).
//! - §5's decision rule — the integer form `landed * 5 >= n * 4`, and the
//!   Wilson-interval `provisional` mark that records but never changes it.
//!
//! GPU-free throughout: `FakeSubstrate` serves scripted `<action>` turns
//! FIFO, exactly like `task_loop_test.rs`, so the "model" is entirely
//! pre-canned and every run is deterministic.

use bloomery_core::journal::{replay, Event, Journal};
use bloomery_core::profile::Profile;
use bloomery_daemon::agents::ImageStore;
use bloomery_daemon::codec_probe::fixtures::{parse_fixture_set, FixtureSet};
use bloomery_daemon::codec_probe::{
    gate_decision, is_provisional, run_codec_probe, ENVELOPE_LENS, FIXTURE_BUDGET_TOKENS,
    FIXTURE_MAX_STEPS,
};
use bloomery_daemon::pager::Pager;
use bloomery_substrate::fake::FakeSubstrate;
use bloomery_substrate::Reply;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

/// Deliberately carries both characters invariant 2 maps to `-`: a model
/// name is a scratch *directory* name, and `qwen2.5-coder:7b` / an
/// org-prefixed HF id would otherwise nest or collide.
const MODEL: &str = "org/m:7b";
/// What [`MODEL`] must become on disk.
const MODEL_DIR: &str = "org-m-7b";

/// The pinned gate-G4 refusal outcome (Task 7's private const, re-typed here
/// the same way `task_loop_test.rs` re-types it): a probe spec must NEVER
/// produce this — the probe always runs `mutating_verbs: true`, or it would
/// be measuring its own demotion.
const MUTATING_VERB_DEMOTED: &str = "verb unavailable: mutating verbs demoted (gate G4)";

/// A profile whose `codecs` grid measures `whole_file` strictly better —
/// protocol §4's first rule, so `model_patch_codec` selects `WholeFile` and
/// the verdict's provenance is "codec from profile". Same document
/// `pager_codec_gate_test.rs` uses.
const WF_WINS_PROFILE: &str = r#"{
  "assay_profile_version": 3,
  "model": {"name": "m"},
  "codecs": {
    "search_replace": {"small": {"lands": 0.5, "lands_applies": 0.6, "n": 20}},
    "whole_file": {"small": {"lands": 0.8, "lands_applies": 0.9, "n": 20}}
  }
}"#;

/// A 2-fixture stand-in for the shipped N=20 set: the engine is generic over
/// set size (Task 5 owns the real set's content), and 2 fixtures is the
/// smallest number that still proves per-fixture isolation — fresh dir,
/// fresh agent, fresh journal handle — and that the verdict aggregates.
const TEST_SET: &str = r#"
set = "codec-tasks-test"

[[fixture]]
name = "t1-alpha"
lens = "plaintext"
target = "a.txt"
goal = "fix the broken line in a.txt"

[[fixture.file]]
path = "a.txt"
contents = "alpha\nbroken\n"

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
"#;

fn test_set() -> FixtureSet {
    parse_fixture_set(TEST_SET).expect("inline test fixture set parses")
}

fn meta() -> bloomery_core::gguf::GgufMeta {
    bloomery_core::gguf::GgufMeta {
        arch: "qwen2".into(),
        layers: 4,
        kv_heads: 2,
        head_dim: 32,
        training_ctx: 65536,
        weights_bytes: 1000,
    }
}

/// A fresh, per-test tempdir — PID + atomic counter, so parallel test
/// threads in one `cargo test` process never collide.
fn fresh_dir(tag: &str) -> PathBuf {
    static UNIQUE: AtomicU64 = AtomicU64::new(0);
    let unique = UNIQUE.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "bloomery-codecprobe-{tag}-{}-{unique}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn scripted(text: &str) -> Reply {
    Reply {
        text: text.to_string(),
        prompt_tokens: Some(8),
        completion_tokens: Some(4),
        duration_ms: 1,
    }
}

/// A `SearchReplace`-codec patch turn.
fn sr_patch(path: &str, search: &str, replace: &str) -> Reply {
    scripted(&format!(
        "<action verb=\"patch\" path=\"{path}\">\n\
         <<<<<<< SEARCH\n{search}\n=======\n{replace}\n>>>>>>> REPLACE\n\
         </action>"
    ))
}

/// A `WholeFile`-codec patch turn: the body is the file's whole new
/// contents, no markers. Only parses under `PatchCodec::WholeFile`, which is
/// what makes it a behavioral pin that the selected codec really did reach
/// the `TaskSpec`.
fn wf_patch(path: &str, contents: &str) -> Reply {
    scripted(&format!(
        "<action verb=\"patch\" path=\"{path}\">\n{contents}\n</action>"
    ))
}

fn done(summary: &str) -> Reply {
    scripted(&format!("<action verb=\"done\">\n{summary}\n</action>"))
}

fn read(path: &str) -> Reply {
    scripted(&format!(
        "<action verb=\"read\" path=\"{path}\">\n</action>"
    ))
}

/// A pager with [`MODEL`] registered, `replies` scripted FIFO, and its task
/// journal pointed at `dir/tasks.jsonl` (where `run_task`'s own `TaskStep`
/// events land — the probe's `CodecFixture`/`CodecVerdict` events go to the
/// *pager's* journal, `dir/pager.jsonl`, which is what every assertion below
/// replays).
fn build_pager(dir: &Path, replies: Vec<Reply>) -> Pager<FakeSubstrate> {
    let journal = Journal::open(&dir.join("pager.jsonl")).unwrap();
    let images = ImageStore::new(&dir.join("img")).unwrap();
    let mut fake = FakeSubstrate::new();
    for r in replies {
        fake.script_reply(r);
    }
    let mut pager = Pager::new(fake, journal, images, Box::new(|| Some(1024 * 1024 * 1024)));
    let gguf = dir.join("m.gguf");
    std::fs::write(&gguf, b"fake weights").unwrap();
    pager.register_model(MODEL, &gguf, meta(), None).unwrap();
    pager.set_task_journal_path(dir.join("tasks.jsonl"));
    pager
}

fn pager_events(dir: &Path) -> Vec<Event> {
    replay(&dir.join("pager.jsonl")).unwrap()
}

fn fixture_events(events: &[Event]) -> Vec<&Event> {
    events
        .iter()
        .filter(|e| matches!(e, Event::CodecFixture { .. }))
        .collect()
}

fn verdict_events(events: &[Event]) -> Vec<&Event> {
    events
        .iter()
        .filter(|e| matches!(e, Event::CodecVerdict { .. }))
        .collect()
}

fn removed_agents(events: &[Event]) -> Vec<&str> {
    events
        .iter()
        .filter_map(|e| match e {
            Event::AgentRemoved { id, .. } => Some(id.as_str()),
            _ => None,
        })
        .collect()
}

/// `(fixture, codec, landed, steps, detail)` for every `CodecFixture` event,
/// in journal order.
fn fixture_rows(events: &[Event]) -> Vec<(String, String, bool, u32, String)> {
    events
        .iter()
        .filter_map(|e| match e {
            Event::CodecFixture {
                fixture,
                codec,
                landed,
                steps,
                detail,
                ..
            } => Some((
                fixture.clone(),
                codec.clone(),
                *landed,
                *steps,
                detail.clone(),
            )),
            _ => None,
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Pure decision helpers (protocol §5)
// ---------------------------------------------------------------------------

/// The integer form exists precisely so the ≥80% boundary has no float edge:
/// 16/20 is exactly 0.80 and must KEEP; 15/20 is below and must demote.
#[test]
fn gate_decision_is_the_integer_eighty_percent_boundary() {
    assert!(gate_decision(16, 20), "16/20 is exactly 80% — a keep");
    assert!(!gate_decision(15, 20), "15/20 is below 80% — a demote");
    assert!(gate_decision(4, 5), "4/5 is exactly 80% — a keep");
    assert!(!gate_decision(3, 5), "3/5 is below 80% — a demote");
    assert!(gate_decision(20, 20));
    assert!(!gate_decision(0, 20));
}

/// `provisional` marks a record whose Wilson interval straddles 0.80; it
/// never changes the decision. The three cases are the protocol's own
/// derived-sanity numbers (§5).
#[test]
fn is_provisional_is_strict_straddle_of_the_threshold() {
    assert!(
        is_provisional(0.5840, 0.9193),
        "16/20's interval straddles 0.80"
    );
    assert!(
        !is_provisional(0.8389, 1.0),
        "20/20's interval sits entirely at or above 0.80"
    );
    assert!(
        !is_provisional(0.3866, 0.7812),
        "12/20's interval sits entirely below 0.80"
    );
}

/// The instrument's declared parameters (protocol §2) are constants, not
/// call-site literals, so a later caller cannot quietly probe under
/// different bounds than the pre-registered ones.
#[test]
fn instrument_parameters_match_the_pre_registered_protocol() {
    assert_eq!(FIXTURE_BUDGET_TOKENS, 30_000);
    assert_eq!(FIXTURE_MAX_STEPS, 6);
    assert_eq!(ENVELOPE_LENS, "bloomery-task-envelope-v1");
}

// ---------------------------------------------------------------------------
// The happy path: every fixture lands
// ---------------------------------------------------------------------------

#[test]
fn all_fixtures_landing_keeps_mutating_verbs_and_journals_one_verdict() {
    let dir = fresh_dir("all-land");
    let scratch = dir.join("scratch");
    // Invariant 2's determinism clause: a stale dir from a previous boot is
    // removed, not merged into.
    let stale = scratch.join(MODEL_DIR).join("t1-alpha");
    std::fs::create_dir_all(&stale).unwrap();
    std::fs::write(stale.join("leftover.txt"), b"from a previous boot").unwrap();

    let pager = Mutex::new(build_pager(
        &dir,
        vec![
            sr_patch("a.txt", "broken", "fixed"),
            done("repaired a.txt"),
            sr_patch("b.txt", "broken", "fixed"),
            done("repaired b.txt"),
        ],
    ));

    let result = run_codec_probe(&pager, MODEL, &test_set(), &scratch).expect("probe completes");

    assert_eq!(result.landed, 2);
    assert_eq!(result.n, 2);
    assert_eq!(result.fixture_set, "codec-tasks-test");
    assert!(result.mutating_verbs, "2/2 is a keep");

    // The gate is stored on the pager, so dispatch policy actually changes.
    let p = pager.lock().unwrap();
    assert!(p.model_mutating_verbs(MODEL));
    drop(p);

    // Scratch layout: '/' and ':' mapped to '-', one dir per fixture, left
    // in place for inspection, and the stale file gone.
    let t1 = scratch.join(MODEL_DIR).join("t1-alpha");
    let t2 = scratch.join(MODEL_DIR).join("t2-beta");
    assert!(!stale.join("leftover.txt").exists(), "stale state must go");
    assert_eq!(
        std::fs::read_to_string(t1.join("a.txt")).unwrap(),
        "alpha\nfixed\n"
    );
    assert_eq!(
        std::fs::read_to_string(t2.join("b.txt")).unwrap(),
        "beta\nfixed\n"
    );

    let events = pager_events(&dir);
    assert_eq!(fixture_events(&events).len(), 2, "one event per fixture");
    assert_eq!(
        verdict_events(&events).len(),
        1,
        "exactly one verdict per completed probe"
    );
    assert_eq!(
        removed_agents(&events).len(),
        2,
        "every fixture's ephemeral agent is removed"
    );

    let rows = fixture_rows(&events);
    assert_eq!(rows[0].0, "t1-alpha");
    assert_eq!(rows[1].0, "t2-beta");
    for row in &rows {
        assert_eq!(
            row.1, "search_replace",
            "the one selected codec, everywhere"
        );
        assert!(row.2, "both fixtures landed");
        assert_eq!(row.3, 2, "[patch, done]");
        assert!(
            row.4.starts_with("patched"),
            "detail is the last patch step's outcome, got {:?}",
            row.4
        );
    }

    match verdict_events(&events)[0] {
        Event::CodecVerdict {
            model,
            fixture_set,
            codec,
            landed,
            n,
            provisional,
            mutating_verbs,
            detail,
            ..
        } => {
            assert_eq!(model, MODEL);
            assert_eq!(fixture_set, "codec-tasks-test");
            assert_eq!(codec, "search_replace");
            assert_eq!((*landed, *n), (2, 2));
            // 2/2 is a *provisional* keep: wilson95(2, 2) = (0.342, 1.0),
            // which straddles 0.80. That is protocol §5 working as written
            // — the point estimate decides, the interval only marks the
            // record — and it is exactly why the shipped set is N=20 rather
            // than 2 (§5's sample-size justification).
            assert!(*provisional, "an N=2 interval cannot resolve 0.80");
            assert!(*mutating_verbs);
            assert!(
                detail.contains(ENVELOPE_LENS),
                "the verdict must name its lens: {detail}"
            );
            assert!(
                detail.contains("default (codecs unmeasured)"),
                "an unprofiled model's codec provenance must say so: {detail}"
            );
        }
        other => panic!("expected CodecVerdict, got {other:?}"),
    }

    // The probe always runs `mutating_verbs: true` — a refusal string in the
    // task journal would mean it measured its own demotion.
    let task_events = replay(&dir.join("tasks.jsonl")).unwrap();
    assert!(
        !task_events.iter().any(|e| matches!(
            e,
            Event::TaskStep { outcome, .. } if outcome == MUTATING_VERB_DEMOTED
        )),
        "a probe spec must never be demoted"
    );
}

// ---------------------------------------------------------------------------
// Scoring leg (a): no patch step at all
// ---------------------------------------------------------------------------

#[test]
fn a_model_that_never_patches_is_demoted_and_that_is_a_measurement() {
    let dir = fresh_dir("never-patch");
    let pager = Mutex::new(build_pager(
        &dir,
        vec![done("nothing to do"), done("nothing to do")],
    ));

    let result =
        run_codec_probe(&pager, MODEL, &test_set(), &dir.join("scratch")).expect("probe completes");

    assert_eq!(result.landed, 0);
    assert_eq!(result.n, 2);
    assert!(!result.mutating_verbs, "0/2 is a demote");

    let p = pager.lock().unwrap();
    assert!(!p.model_mutating_verbs(MODEL));
    assert!(
        p.status().models[0].codec_gate.is_some(),
        "a demoted gate is still a measurement, not unmeasured"
    );
    drop(p);

    let events = pager_events(&dir);
    let rows = fixture_rows(&events);
    assert_eq!(rows.len(), 2);
    for row in &rows {
        assert!(!row.2, "no patch step means not landed");
        assert_eq!(row.3, 1, "[done]");
        assert_eq!(
            row.4, "Done",
            "with no patch step, detail is the terminal status"
        );
    }
    assert_eq!(verdict_events(&events).len(), 1);
}

// ---------------------------------------------------------------------------
// Scoring leg (b): the bytes must actually change
// ---------------------------------------------------------------------------

/// Protocol §3's first recorded edge: an identity patch APPLIES cleanly (leg
/// (a) is satisfied — a successful, non-failed `patch` step) but changes no
/// bytes, so it is a non-repair and must score NOT landed. Drop leg (b) and
/// this test flips.
#[test]
fn an_identity_patch_applies_but_does_not_land() {
    let dir = fresh_dir("identity");
    let pager = Mutex::new(build_pager(
        &dir,
        vec![
            sr_patch("a.txt", "broken", "broken"),
            done("changed nothing"),
            sr_patch("b.txt", "broken", "broken"),
            done("changed nothing"),
        ],
    ));

    let scratch = dir.join("scratch");
    let result = run_codec_probe(&pager, MODEL, &test_set(), &scratch).expect("probe completes");

    assert_eq!(result.landed, 0, "an identity patch is not a repair");
    assert!(!result.mutating_verbs);
    assert_eq!(
        std::fs::read_to_string(scratch.join(MODEL_DIR).join("t1-alpha").join("a.txt")).unwrap(),
        "alpha\nbroken\n",
        "the target's bytes are unchanged — that is the whole point"
    );

    let rows = fixture_rows(&pager_events(&dir));
    for row in &rows {
        assert!(!row.2, "identity patch must score NOT landed");
        assert!(
            row.4.starts_with("patched"),
            "leg (a) really was satisfied — the patch step succeeded: {:?}",
            row.4
        );
    }
}

/// Protocol §3's second recorded edge: a patch that lands on some OTHER file
/// (here a brand-new scratch file in the granted dir) leaves the declared
/// target untouched, so it scores NOT landed — the gate licenses real edits
/// to the thing under repair, not scratch-file creation.
///
/// Doubles as the behavioral pin for invariant 1: this whole-file body only
/// parses under `PatchCodec::WholeFile`, so it can only succeed if the
/// profile-selected codec actually reached the `TaskSpec`.
#[test]
fn a_patch_landing_only_on_a_scratch_file_does_not_land() {
    let dir = fresh_dir("scratch-file");
    let mut p = build_pager(
        &dir,
        vec![
            wf_patch("notes.txt", "scratch content"),
            done("wrote notes"),
            wf_patch("notes.txt", "scratch content"),
            done("wrote notes"),
        ],
    );
    p.attach_profile(MODEL, Profile::from_json(WF_WINS_PROFILE).unwrap(), false)
        .unwrap();
    let pager = Mutex::new(p);

    let scratch = dir.join("scratch");
    let result = run_codec_probe(&pager, MODEL, &test_set(), &scratch).expect("probe completes");

    assert_eq!(result.landed, 0, "a scratch file is not the target");
    assert!(!result.mutating_verbs);

    let t1 = scratch.join(MODEL_DIR).join("t1-alpha");
    assert!(
        t1.join("notes.txt").exists(),
        "the patch really did land somewhere — that is what makes this a \
         target-file test and not a did-not-apply test"
    );
    assert_eq!(
        std::fs::read_to_string(t1.join("a.txt")).unwrap(),
        "alpha\nbroken\n",
        "the declared target is untouched"
    );

    let events = pager_events(&dir);
    for row in fixture_rows(&events) {
        assert!(!row.2, "a scratch-file patch must score NOT landed");
        assert_eq!(row.1, "whole_file", "the profile-selected codec, recorded");
        assert!(row.4.starts_with("patched"), "got {:?}", row.4);
    }
    match verdict_events(&events)[0] {
        Event::CodecVerdict { codec, detail, .. } => {
            assert_eq!(codec, "whole_file");
            assert!(
                detail.contains("codec from profile"),
                "a profile-selected codec's provenance must say so: {detail}"
            );
        }
        other => panic!("expected CodecVerdict, got {other:?}"),
    }
}

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
        "no partial score may be spliced"
    );
    assert_eq!(
        removed_agents(&events).len(),
        1,
        "the fixture's agent is still removed on the abort path"
    );
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
