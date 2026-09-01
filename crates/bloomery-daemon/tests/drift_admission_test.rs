//! Verdict-gated admission, end to end: a confirmed regression blocks the
//! model, and `unblock` is the only thing that clears it.
//!
//! Split out of `drift_test.rs` on 2026-09-01 (carried-debt slice D).

mod common;

use bloomery_core::journal::Event;
use bloomery_daemon::drift::{DriftStatus, ModelDrift};
use bloomery_daemon::pager::PagerError;

use common::drift::{
    boot, boot_for, exited, gate_deciding, profile_doc, profile_doc_ceiling, scripted_probes, sha8,
    V4_QWEN3_8B, V8_QWEN3_8B,
};

// Verdict-gated-admission, end to end
// (`docs/superpowers/specs/2026-08-18-verdict-gated-admission-design.md` §7,
// "the single most important test").
//
// Every test above proves the drift watch measures correctly by feeding it
// documents and reading `ModelDrift` back. Neither of these two feeds
// `set_drift` directly, and neither hand-builds a `DriftStatus`: both drive
// `watch_model -> set_drift -> admission_block` through the real boot path
// (`Boot::run` -> `run_post_with_gate`), the way `main.rs` actually calls it,
// and then go one step further than every test above by asking the pager for
// an admission decision — `create_agent` — against what that boot produced.
// ---------------------------------------------------------------------------

/// THE fleet guard, run for real. assay upgrades move every model's
/// instrument identity at once (spec §3: "never a pass, never a fail"), and
/// slice 1 §8's committed mixed-version fixtures are the real bytes that
/// meet a daemon on the first boot after one: `fixtures/profile-v4-qwen3-8b.json`
/// (the pre-upgrade schema, instrument `"0.5.0/v4"`) seeded as both this
/// model's blessed baseline and last boot's document, against
/// `fixtures/profile-v8-qwen3-8b.json` (instrument `"0.9.0/v8"`) as this
/// boot's measurement — the same V4/V8 pair
/// `a_changed_instrument_is_named_before_the_diff_is_ever_spawned` above pins
/// at the gate level, driven here through the full orchestration instead.
/// Registered under the fixtures' own model name (`boot_for`, not `boot`):
/// `PostRunner::probe` refuses a document whose `model.name` does not match
/// the model it was asked to probe, so relabelling these bytes as `"qwen"`
/// would never reach the watch at all.
///
/// The diff gate is scripted to answer exit 1 — drift — if it is EVER
/// spawned, so a precheck that got bypassed would not read as a quiet
/// no-op: it would read as the fleet blocked, which is the failure this test
/// exists to catch.
#[test]
fn an_instrument_upgrade_never_blocks_the_fleet_end_to_end() {
    let b = boot_for("watch-fleet-guard-e2e", "qwen3:8b");
    b.seed("qwen3:8b.json", V4_QWEN3_8B); // last boot's -> becomes the step reference
    b.seed("qwen3:8b.baseline.json", V4_QWEN3_8B); // the blessed cumulative reference
    let (runner, probes) = scripted_probes(vec![Ok(V8_QWEN3_8B.to_string())]);
    let (gate, calls) = gate_deciding(|_reference, _current| exited(1));

    b.run(&runner, &gate);

    assert!(
        calls.borrow().is_empty(),
        "an instrument change must be named before the diff is ever spawned, on BOTH \
         comparisons, got {:?}",
        calls.borrow()
    );
    assert_eq!(
        probes.borrow().len(),
        1,
        "InstrumentChanged settles on the first reading; there is nothing to confirm"
    );
    let expected = DriftStatus::InstrumentChanged {
        reference: "0.5.0/v4".to_string(),
        current: "0.9.0/v8".to_string(),
    };
    assert_eq!(
        b.drift(),
        Some(ModelDrift {
            step: expected.clone(),
            cumulative: expected,
        }),
        "both comparisons read the instrument change independently"
    );

    let mut p = b.pager.lock().unwrap();
    assert!(
        p.admission_block_for("qwen3:8b").is_none(),
        "an instrument change must never derive a block"
    );
    p.create_agent("qwen3:8b", 50, None, 10_000)
        .expect("an assay upgrade must never take a model out of admission");
}

/// The block, run for real: a same-instrument pair where the CUMULATIVE
/// comparison drifts and the confirm reproduces it — spec §4's
/// confirm-then-alarm settling on `Confirmed`, and verdict-gated-admission
/// design §2's derivation from it, both through the real boot path this time
/// rather than a `set_drift` call built by hand.
#[test]
fn a_confirmed_cumulative_regression_blocks_admission_end_to_end() {
    let b = boot("watch-cumulative-blocks-e2e");
    let last_boot = profile_doc_ceiling("qwen", 1024);
    let baseline = profile_doc_ceiling("qwen", 900);
    b.seed("qwen.json", &last_boot);
    b.seed("qwen.baseline.json", &baseline);
    let (runner, probes) = scripted_probes(vec![Ok(profile_doc("qwen"))]);
    // The cumulative reference (baseline) drifts every time it is asked; the
    // step reference (previous) does not — so only the cumulative comparison
    // has a hypothesis to confirm.
    let (gate, _calls) = gate_deciding(|reference, _current| {
        if reference.ends_with(".baseline.json") {
            exited(1)
        } else {
            exited(0)
        }
    });

    b.run(&runner, &gate);

    assert_eq!(
        probes.borrow().len(),
        2,
        "the boot probe plus exactly one confirm — never zero, never a retry loop"
    );
    let block_reference = sha8(&baseline);
    assert_eq!(
        b.drift(),
        Some(ModelDrift {
            step: DriftStatus::WithinNoise,
            cumulative: DriftStatus::Confirmed {
                reference: block_reference.clone(),
            },
        })
    );

    let admission_rows: Vec<(String, String, String, String)> = b
        .events()
        .into_iter()
        .filter_map(|e| match e {
            Event::Admission {
                model,
                action,
                reference,
                provenance,
            } => Some((model, action, reference, provenance)),
            _ => None,
        })
        .collect();
    assert_eq!(
        admission_rows,
        vec![(
            "qwen".to_string(),
            "blocked".to_string(),
            block_reference.clone(),
            "drift-watch".to_string(),
        )],
        "exactly one blocked row, with the watch's own provenance: {admission_rows:?}"
    );

    let mut p = b.pager.lock().unwrap();
    let block = p
        .admission_block_for("qwen")
        .expect("the confirmed cumulative regression must stand as a block")
        .clone();
    assert_eq!(block.reference, block_reference);

    match p.create_agent("qwen", 50, None, 10_000).unwrap_err() {
        PagerError::DriftBlocked { model, reference } => {
            assert_eq!(model, "qwen");
            assert_eq!(reference, block_reference);
        }
        other => panic!("expected DriftBlocked, got {other:?}"),
    }

    p.clear_admission_block("qwen").unwrap();
    assert!(p.admission_block_for("qwen").is_none());
    p.create_agent("qwen", 50, None, 10_000)
        .expect("clearing the block re-admits");
}
