//! The admission block, end to end (drift-watch design, Tasks 2-4): the
//! watch setting the block under the enumerated policy, `admit()` consulting
//! it on both error surfaces, and `clear_admission_block` as the operator's
//! way out.
//!
//! Split out of `pager_test.rs` on 2026-09-01 (carried-debt slice D).

mod common;

use bloomery_core::journal::{replay, Event};
use bloomery_daemon::drift::{DriftStatus, ModelDrift};
use bloomery_daemon::pager::*;
use common::pager::{fresh_dir, meta, pager_in, write_gguf};
use std::path::Path;

/// A minimal but real assay profile document — the same shape `post_test.rs`
/// and `api_native_test.rs` use — just enough for a model to count as
/// profiled.
fn minimal_profile(model: &str) -> bloomery_core::profile::Profile {
    bloomery_core::profile::Profile::from_json(&format!(
        r#"{{"assay_profile_version":3,"probe_version":"0.4.1","model":{{"name":"{model}"}},"verdicts":{{}}}}"#
    ))
    .expect("fixture profile parses")
}

/// A confirmed-cumulative drift reading naming `reference` — the one shape
/// `set_drift` turns into an admission block (Task 2's invariant).
fn confirmed_cumulative(reference: &str) -> ModelDrift {
    ModelDrift {
        step: DriftStatus::WithinNoise,
        cumulative: DriftStatus::Confirmed {
            reference: reference.to_string(),
        },
    }
}

/// Every `Admission` row in the journal at `path` as
/// `(model, action, reference, provenance)`.
fn admission_rows(path: &Path) -> Vec<(String, String, String, String)> {
    replay(path)
        .unwrap()
        .iter()
        .filter_map(|e| match e {
            Event::Admission {
                model,
                action,
                reference,
                provenance,
            } => Some((
                model.clone(),
                action.clone(),
                reference.clone(),
                provenance.clone(),
            )),
            _ => None,
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Task 2: the watch sets the block — the enumerated policy
// (`.superpowers/sdd/2026-08-18-verdict-gated-admission/task-2-brief.md`)
// ---------------------------------------------------------------------------

/// The policy IS this table — enumerate it rather than sample it. Refuse
/// only what was established; name everything else. An outcome that
/// declines to conclude must not be laundered into a conclusion by the
/// admission path.
#[test]
fn only_a_confirmed_cumulative_reading_blocks_admission() {
    let cases: Vec<(DriftStatus, bool)> = vec![
        (DriftStatus::WithinNoise, false),
        (
            DriftStatus::Confirmed {
                reference: "abc1234".into(),
            },
            true,
        ),
        (DriftStatus::Transient, false),
        (
            DriftStatus::Unconfirmed {
                reason: "confirm probe failed".into(),
            },
            false,
        ),
        (DriftStatus::NotComparable, false),
        // Exit 3's incomplete comparison: no established drift, so no block —
        // "refuse only what was established; name everything else."
        (DriftStatus::Incomplete, false),
        (
            DriftStatus::InstrumentChanged {
                reference: "0.9.0/v8".into(),
                current: "0.10.0/v9".into(),
            },
            false,
        ),
        (
            DriftStatus::Unmeasured {
                reason: "no baseline blessed".into(),
            },
            false,
        ),
    ];

    let dir = fresh_dir("bloomery-pager-admission-enum");
    let gguf = write_gguf(&dir, "fake.gguf", b"weights");
    for (cumulative, expect_blocked) in cases {
        let (mut p, _, _) = pager_in(&dir, 0, Some(10u64.pow(9)));
        p.register_model("m", &gguf, meta(1000), None).unwrap();
        p.set_drift(
            "m",
            ModelDrift {
                step: DriftStatus::WithinNoise,
                cumulative: cumulative.clone(),
            },
        )
        .unwrap();
        let blocked = p.admission_block_for("m").is_some();
        assert_eq!(blocked, expect_blocked, "cumulative {cumulative:?}");
    }
}

/// `step` compares against the PREVIOUS BOOT, whose reference advances every
/// boot — a step-keyed block would clear itself next boot whether or not the
/// regression persisted. Slice 1: step "alone leaks the ratchet".
#[test]
fn a_confirmed_step_reading_alone_does_not_block() {
    let dir = fresh_dir("bloomery-pager-admission-step-only");
    let (mut p, _, _) = pager_in(&dir, 0, Some(10u64.pow(9)));
    let gguf = write_gguf(&dir, "fake.gguf", b"weights");
    p.register_model("m", &gguf, meta(1000), None).unwrap();
    p.set_drift(
        "m",
        ModelDrift {
            step: DriftStatus::Confirmed {
                reference: "step99".into(),
            },
            cumulative: DriftStatus::WithinNoise,
        },
    )
    .unwrap();
    assert!(p.admission_block_for("m").is_none());
}

/// The ratchet case: stable at a degraded level. `step` sees nothing because
/// last boot was degraded too; `cumulative` sees the drift from the blessed
/// baseline, and that is the claim that holds a model out.
#[test]
fn a_confirmed_cumulative_reading_blocks_even_when_step_is_clean() {
    let dir = fresh_dir("bloomery-pager-admission-cumulative-only");
    let (mut p, _, _) = pager_in(&dir, 0, Some(10u64.pow(9)));
    let gguf = write_gguf(&dir, "fake.gguf", b"weights");
    p.register_model("m", &gguf, meta(1000), None).unwrap();
    p.set_drift(
        "m",
        ModelDrift {
            step: DriftStatus::WithinNoise,
            cumulative: DriftStatus::Confirmed {
                reference: "base42".into(),
            },
        },
    )
    .unwrap();
    let block = p.admission_block_for("m").expect("blocked");
    assert_eq!(block.reference, "base42");
}

/// THE test of this slice. assay v1.8 (0.10.0/v9) lands against blessed v8
/// references, so the first boot after that merge reads `InstrumentChanged`
/// on EVERY model at once. Blocking on it would take the whole fleet out on
/// a routine instrument upgrade. Slice 1 §3: "never a pass, never a fail".
#[test]
fn an_instrument_change_never_blocks_the_fleet() {
    let dir = fresh_dir("bloomery-pager-admission-instrument-changed");
    let (mut p, _, _) = pager_in(&dir, 0, Some(10u64.pow(9)));
    let gguf = write_gguf(&dir, "fake.gguf", b"weights");
    p.register_model("m", &gguf, meta(1000), None).unwrap();
    p.set_drift(
        "m",
        ModelDrift {
            step: DriftStatus::InstrumentChanged {
                reference: "0.9.0/v8".into(),
                current: "0.10.0/v9".into(),
            },
            cumulative: DriftStatus::InstrumentChanged {
                reference: "0.9.0/v8".into(),
                current: "0.10.0/v9".into(),
            },
        },
    )
    .unwrap();
    assert!(p.admission_block_for("m").is_none());
}

// ---------------------------------------------------------------------------
// Task 3: admit() consults the block, on both error surfaces
// (`.superpowers/sdd/2026-08-18-verdict-gated-admission/task-3-brief.md`)
// ---------------------------------------------------------------------------

/// The block refuses new agents on a model that HAS a profile — the
/// enforcement half of Task 2's invariant. Distinct from `Unprofiled`:
/// something WAS measured, and what it measured was a reproduced
/// regression.
#[test]
fn a_drift_blocked_model_refuses_new_agents() {
    let dir = fresh_dir("bloomery-pager-drift-blocked-refuses");
    let (mut p, _, _) = pager_in(&dir, 0, Some(10u64.pow(9)));
    let gguf = write_gguf(&dir, "fake.gguf", b"weights");
    p.register_model("m", &gguf, meta(1000), None).unwrap();
    p.attach_profile("m", minimal_profile("m"), false).unwrap();
    p.set_drift("m", confirmed_cumulative("base42")).unwrap();

    let err = p.create_agent("m", 50, None, 10_000).unwrap_err();
    match err {
        PagerError::DriftBlocked { model, reference } => {
            assert_eq!(model, "m");
            assert_eq!(reference, "base42");
        }
        other => panic!("expected DriftBlocked, got {other:?}"),
    }
}

/// The gate is at agent CREATION, never per inference — the same argument
/// that already governs the POST window. An agent admitted before a block
/// appeared keeps working; only new work on that model is refused.
#[test]
fn an_agent_created_before_the_block_keeps_working() {
    let dir = fresh_dir("bloomery-pager-drift-blocked-preexisting");
    let (mut p, _, _) = pager_in(&dir, 1, Some(10u64.pow(9)));
    let gguf = write_gguf(&dir, "fake.gguf", b"weights");
    p.register_model("m", &gguf, meta(1000), None).unwrap();
    p.attach_profile("m", minimal_profile("m"), false).unwrap();

    let agent = p.create_agent("m", 50, None, 10_000).unwrap();
    p.set_drift("m", confirmed_cumulative("base42")).unwrap();

    // The existing agent still resolves and can still be inferred against.
    p.infer(&agent.id, "still here", 16, None)
        .expect("an agent admitted before the block keeps working");

    // New work on the same model is refused.
    assert!(matches!(
        p.create_agent("m", 50, None, 10_000),
        Err(PagerError::DriftBlocked { .. })
    ));
}

/// The two refusals stay distinguishable: drift-blocked means a profile
/// exists and a regression was reproduced against it; unprofiled means no
/// profile exists at all.
#[test]
fn an_unprofiled_model_still_refuses_as_unprofiled() {
    let dir = fresh_dir("bloomery-pager-unprofiled-distinct");
    let (mut p, _, _) = pager_in(&dir, 0, Some(10u64.pow(9)));
    p.set_allow_unprofiled(false);
    let gguf = write_gguf(&dir, "fake.gguf", b"weights");
    p.register_model("m", &gguf, meta(1000), None).unwrap();

    assert!(matches!(
        p.create_agent("m", 50, None, 10_000),
        Err(PagerError::Unprofiled(_))
    ));
}

// ---------------------------------------------------------------------------
// Task 4: `clear_admission_block` — the operator's way out
// (`.superpowers/sdd/2026-08-18-verdict-gated-admission/task-4-brief.md`)
// ---------------------------------------------------------------------------

/// The point of separating the block from the reading: an operator may
/// override the policy without any measurement changing. After clearing,
/// `/status` still says exactly what was measured.
#[test]
fn unblock_admits_and_leaves_the_reading_alone() {
    let dir = fresh_dir("bloomery-pager-unblock-leaves-reading");
    let (mut p, _, _) = pager_in(&dir, 1, Some(10u64.pow(9)));
    let gguf = write_gguf(&dir, "fake.gguf", b"weights");
    p.register_model("m", &gguf, meta(1000), None).unwrap();
    p.attach_profile("m", minimal_profile("m"), false).unwrap();
    p.set_drift("m", confirmed_cumulative("base42")).unwrap();

    let cleared = p
        .clear_admission_block("m")
        .unwrap()
        .expect("something was blocking");
    assert_eq!(cleared.reference, "base42");
    assert!(p.admission_block_for("m").is_none());
    assert!(p.create_agent("m", 50, None, 10_000).is_ok());

    let status = p.status();
    let model = status.models.iter().find(|m| m.name == "m").unwrap();
    assert_eq!(
        model.drift.as_ref().unwrap().cumulative,
        DriftStatus::Confirmed {
            reference: "base42".into()
        },
        "the reading is a measurement and must survive the override"
    );
}

/// Answering 200 where nothing was blocking would tell an operator they
/// cleared something when nothing was written — the silent no-op slice 1
/// §2 forbids, the same reason bless returns 409. `Ok(None)` on a known
/// model is that refusal, not an error.
#[test]
fn unblock_with_nothing_blocking_is_a_conflict_not_a_no_op() {
    let dir = fresh_dir("bloomery-pager-unblock-nothing-blocking");
    let (mut p, _, _) = pager_in(&dir, 0, Some(10u64.pow(9)));
    let gguf = write_gguf(&dir, "fake.gguf", b"weights");
    p.register_model("m", &gguf, meta(1000), None).unwrap();

    assert!(p.clear_admission_block("m").unwrap().is_none());
}

/// An unknown model refuses first, the same shape every other route in this
/// file uses.
#[test]
fn unblock_on_an_unknown_model_is_unknown_model() {
    let dir = fresh_dir("bloomery-pager-unblock-unknown-model");
    let (mut p, _, _) = pager_in(&dir, 0, Some(10u64.pow(9)));

    assert!(matches!(
        p.clear_admission_block("does-not-exist"),
        Err(PagerError::UnknownModel(m)) if m == "does-not-exist"
    ));
}

/// The two routes answer different questions and neither implies the
/// other. bless leaves the block standing, and unblock does not touch the
/// baseline bless just wrote.
#[test]
fn unblock_does_not_rebaseline_and_bless_does_not_unblock() {
    let dir = fresh_dir("bloomery-pager-unblock-does-not-rebaseline");
    let (mut p, _, _) = pager_in(&dir, 0, Some(10u64.pow(9)));
    let gguf = write_gguf(&dir, "fake.gguf", b"weights");
    p.register_model("m", &gguf, meta(1000), None).unwrap();
    p.attach_profile("m", minimal_profile("m"), false).unwrap();
    p.set_drift("m", confirmed_cumulative("base42")).unwrap();

    let profiles_dir = dir.join("profiles");
    std::fs::create_dir_all(&profiles_dir).unwrap();
    p.set_profiles_dir(profiles_dir.clone());
    std::fs::write(profiles_dir.join("m.json"), b"{}").unwrap();

    // bless leaves the block standing…
    p.bless_baseline("m").unwrap();
    assert!(p.admission_block_for("m").is_some());

    // …and unblock does not touch the baseline it just wrote.
    let baseline_path = profiles_dir.join("m.baseline.json");
    let before = std::fs::read(&baseline_path).unwrap();
    p.clear_admission_block("m").unwrap();
    let after = std::fs::read(&baseline_path).unwrap();
    assert_eq!(after, before, "unblock must not touch the blessed baseline");
    assert!(p.admission_block_for("m").is_none());
}

/// `set_drift` journals `"blocked"` the moment a confirmed cumulative
/// regression newly holds a model out — the row the operator's later
/// `"cleared"` row (task-4-brief.md §7) is paired against in a replay.
#[test]
fn set_drift_journals_a_blocked_row_when_it_newly_blocks() {
    let dir = fresh_dir("bloomery-pager-set-drift-journals-blocked");
    let (mut p, jpath, _) = pager_in(&dir, 0, Some(10u64.pow(9)));
    let gguf = write_gguf(&dir, "fake.gguf", b"weights");
    p.register_model("m", &gguf, meta(1000), None).unwrap();
    p.set_drift("m", confirmed_cumulative("base42")).unwrap();

    let rows = admission_rows(&jpath);
    assert_eq!(rows.len(), 1, "one block, one row: {rows:?}");
    assert_eq!(rows[0].0, "m");
    assert_eq!(rows[0].1, "blocked");
    assert_eq!(rows[0].2, "base42");
    assert_eq!(
        rows[0].3, "drift-watch",
        "the block's row must carry the watch's own provenance, not the operator's — \
         a replay needs to tell 'this newly blocked' from 'an operator did this'"
    );
}

/// `clear_admission_block` journals `"cleared"` with operator provenance —
/// the row that lets a replay say who let a held-out model back in.
#[test]
fn clear_admission_block_journals_a_cleared_row_with_operator_provenance() {
    let dir = fresh_dir("bloomery-pager-clear-journals-cleared");
    let (mut p, jpath, _) = pager_in(&dir, 0, Some(10u64.pow(9)));
    let gguf = write_gguf(&dir, "fake.gguf", b"weights");
    p.register_model("m", &gguf, meta(1000), None).unwrap();
    p.set_drift("m", confirmed_cumulative("base42")).unwrap();

    p.clear_admission_block("m").unwrap();

    let rows = admission_rows(&jpath);
    assert_eq!(rows.len(), 2, "one block, one clear: {rows:?}");
    assert_eq!(rows[1].0, "m");
    assert_eq!(rows[1].1, "cleared");
    assert_eq!(rows[1].2, "base42");
    assert_eq!(rows[1].3, "operator");
}
