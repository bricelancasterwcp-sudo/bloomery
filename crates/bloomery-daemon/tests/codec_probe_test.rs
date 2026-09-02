//! G4 codec probe: the scoring rules (protocol §3, §5, §10, §11).
//!
//! Pure decision helpers, the happy path where every fixture lands, the
//! envelope-v2 and -v3 lenses, and both scoring legs -- no patch step at all,
//! and the bytes must actually change. Either leg alone would score a
//! non-repair as a repair, which is why each gets its own test.
//!
//! Governing doc: `docs/superpowers/evidence/2026-08-15-g4-protocol.md`,
//! pre-registered before any of this existed.
//!
//! **Split 2026-09-01** (carried-debt slice D): this file was 1634 lines.
//! Terminal statuses and infrastructure aborts moved to
//! `codec_probe_status_test.rs`, the boot decision table and G5 mixed-set
//! engine to `codec_probe_boot_test.rs`, and the shared fixture header to
//! `tests/common/codec.rs`.
//!
//! GPU-free throughout: `FakeSubstrate` serves scripted `<action>` turns
//! FIFO, so the "model" is entirely pre-canned and every run deterministic.

mod common;

use bloomery_core::journal::{replay, Event};
use bloomery_core::profile::Profile;
use bloomery_daemon::codec_probe::{
    gate_decision, is_provisional, run_codec_probe, ENVELOPE_LENS, ENVELOPE_LENS_V2,
    ENVELOPE_LENS_V3, ENVELOPE_LENS_V4, FIXTURE_BUDGET_TOKENS, FIXTURE_MAX_STEPS,
};
use bloomery_daemon::config::EnvelopeLens;
use bloomery_substrate::Reply;
use std::sync::Mutex;

use common::codec::{
    build_pager, done, fixture_agents, fixture_events, fixture_rows, fresh_dir, pager_events,
    removed_agents, scripted, sr_patch, test_set, verdict_events, MODEL,
};

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
  "probe_version": "0.4.1",
  "model": {"name": "m"},
  "codecs": {
    "search_replace": {"small": {"lands": 0.5, "lands_applies": 0.6, "n": 20}},
    "whole_file": {"small": {"lands": 0.8, "lands_applies": 0.9, "n": 20}}
  }
}"#;

/// A `WholeFile`-codec patch turn: the body is the file's whole new
/// contents, no markers. Only parses under `PatchCodec::WholeFile`, which is
/// what makes it a behavioral pin that the selected codec really did reach
/// the `TaskSpec`.
fn wf_patch(path: &str, contents: &str) -> Reply {
    scripted(&format!(
        "<action verb=\"patch\" path=\"{path}\">\n{contents}\n</action>"
    ))
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
    // Protocol §10, Amendment 2 — the second envelope lens's pinned name.
    assert_eq!(ENVELOPE_LENS_V2, "bloomery-task-envelope-v2");
    // Protocol §11, Amendment 3 — the third envelope lens's pinned name.
    assert_eq!(ENVELOPE_LENS_V3, "bloomery-task-envelope-v3");
    // Turn-4 spec §2 — the fourth envelope lens's pinned name (the visible
    // grant); every turn-4 verdict travels under this spelling.
    assert_eq!(ENVELOPE_LENS_V4, "bloomery-task-envelope-v4");
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
                !detail.contains(ENVELOPE_LENS_V2),
                "an unpreseeded model's verdict must name v1, never v2: {detail}"
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

/// Turn-5 spec §3: every CodecFixture row names the agent that ran it, and
/// the sequence equals the AgentCreated sequence — the keyed join.
#[test]
fn codec_fixture_rows_carry_the_agent_that_ran_them() {
    let dir = fresh_dir("agent-join");
    let pager = Mutex::new(build_pager(
        &dir,
        vec![
            sr_patch("a.txt", "broken", "fixed"),
            done("repaired a.txt"),
            sr_patch("b.txt", "broken", "fixed"),
            done("repaired b.txt"),
        ],
    ));

    run_codec_probe(&pager, MODEL, &test_set(), &dir.join("scratch")).expect("probe completes");

    let events = pager_events(&dir);
    let created: Vec<String> = events
        .iter()
        .filter_map(|e| match e {
            Event::AgentCreated { id, .. } => Some(id.clone()),
            _ => None,
        })
        .collect();
    let rows = fixture_agents(&events);
    assert_eq!(rows.len(), created.len(), "one agent per fixture");
    for (i, (fixture, agent)) in rows.iter().enumerate() {
        assert_eq!(
            agent.as_deref(),
            Some(created[i].as_str()),
            "fixture {fixture} joins to its own agent"
        );
    }
}

// ---------------------------------------------------------------------------
// Protocol §10, Amendment 2: the envelope-v2 (think-preseeded) lens
// ---------------------------------------------------------------------------

/// The v2 companion to `all_fixtures_landing_keeps_mutating_verbs_and_
/// journals_one_verdict` above: same fixture set, same scripted replies, same
/// happy-path landing — the only difference is `set_think_preseed(MODEL,
/// true)` before the probe runs. The verdict's `detail` must name
/// `ENVELOPE_LENS_V2`, never the v1 name, proving `run_codec_probe` reads the
/// model's configured lens (via `ProbeContext::think_preseed`) rather than
/// hardcoding v1.
#[test]
fn a_preseeded_model_probe_journals_the_v2_lens_in_the_verdict_detail() {
    let dir = fresh_dir("preseeded-verdict");
    let mut p = build_pager(
        &dir,
        vec![
            sr_patch("a.txt", "broken", "fixed"),
            done("repaired a.txt"),
            sr_patch("b.txt", "broken", "fixed"),
            done("repaired b.txt"),
        ],
    );
    p.set_model_envelope(MODEL, EnvelopeLens::V2).unwrap();
    let pager = Mutex::new(p);

    let result =
        run_codec_probe(&pager, MODEL, &test_set(), &dir.join("scratch")).expect("probe completes");

    assert_eq!(result.landed, 2);
    assert_eq!(result.n, 2);

    let events = pager_events(&dir);
    match verdict_events(&events)[0] {
        Event::CodecVerdict { detail, .. } => {
            assert!(
                detail.contains(ENVELOPE_LENS_V2),
                "a preseeded model's verdict must name v2: {detail}"
            );
            assert!(
                !detail.contains(ENVELOPE_LENS),
                "a v2 verdict must never also carry the v1 name: {detail}"
            );
        }
        other => panic!("expected CodecVerdict, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Protocol §11, Amendment 3: the envelope-v3 (action-terminated) lens
// ---------------------------------------------------------------------------

/// The v3 companion to the v1/v2 naming tests above: same fixture set, same
/// scripted replies, same happy-path landing — the only difference is
/// `set_model_envelope(MODEL, EnvelopeLens::V3)` before the probe runs. The
/// verdict's `detail` must name `ENVELOPE_LENS_V3`, never v1 or v2, proving
/// `run_codec_probe` reads the model's configured lens (via
/// `ProbeContext::envelope`) rather than hardcoding one.
#[test]
fn a_v3_configured_model_probe_journals_the_v3_lens_in_the_verdict_detail() {
    let dir = fresh_dir("v3-verdict");
    let mut p = build_pager(
        &dir,
        vec![
            sr_patch("a.txt", "broken", "fixed"),
            done("repaired a.txt"),
            sr_patch("b.txt", "broken", "fixed"),
            done("repaired b.txt"),
        ],
    );
    p.set_model_envelope(MODEL, EnvelopeLens::V3).unwrap();
    let pager = Mutex::new(p);

    let result =
        run_codec_probe(&pager, MODEL, &test_set(), &dir.join("scratch")).expect("probe completes");

    assert_eq!(result.landed, 2);
    assert_eq!(result.n, 2);

    let events = pager_events(&dir);
    match verdict_events(&events)[0] {
        Event::CodecVerdict { detail, .. } => {
            assert!(
                detail.contains(ENVELOPE_LENS_V3),
                "a v3-configured model's verdict must name v3: {detail}"
            );
            assert!(
                !detail.contains(ENVELOPE_LENS_V2),
                "a v3 verdict must never also carry the v2 name: {detail}"
            );
            assert!(
                !detail.contains(ENVELOPE_LENS),
                "a v3 verdict must never also carry the v1 name: {detail}"
            );
        }
        other => panic!("expected CodecVerdict, got {other:?}"),
    }
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
