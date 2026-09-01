//! Confirm-then-alarm, wired into the boot (drift-watch design §2, §4, §5),
//! plus the POST wiring the first gate test needs.
//!
//! Split out of `drift_test.rs` on 2026-09-01 (carried-debt slice D).

mod common;

use bloomery_core::journal::{sha256_hex_bytes, Event};
use bloomery_daemon::drift::{DriftStatus, ModelDrift, MAX_TRANSIENTS};
use std::time::Duration;

use common::drift::{
    boot, exited, gate_deciding, is_transient, profile_doc, profile_doc_ceiling, scripted_probes,
    set_mtime, sha8,
};

// POST wiring for the first test
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Confirm-then-alarm, wired into the boot (spec §2, §4, §5)
//
// Every test below drives the *real* `run_post` orchestration against a fake
// assay and a scripted gate: the probe count, the journal rows and the
// rendered status all come from the shipping code path, not from a
// re-implementation of it in the test.
// ---------------------------------------------------------------------------

/// A boot where nothing moved: both comparisons run, both read within noise,
/// the model is probed exactly once, and a baseline that already exists is not
/// re-blessed behind the operator's back.
#[test]
fn a_clean_boot_reads_within_noise_on_both_comparisons_and_probes_once() {
    let b = boot("watch-clean");
    b.seed("qwen.json", &profile_doc_ceiling("qwen", 1024)); // last boot's
    b.seed("qwen.baseline.json", &profile_doc_ceiling("qwen", 900));
    let (runner, probes) = scripted_probes(vec![Ok(profile_doc("qwen"))]);
    let (gate, _calls) = gate_deciding(|_r, _c| exited(0));

    b.run(&runner, &gate);

    assert_eq!(
        probes.borrow().len(),
        1,
        "a boot with no drift reading probes once, never speculatively twice"
    );
    assert_eq!(
        b.drift(),
        Some(ModelDrift {
            step: DriftStatus::WithinNoise,
            cumulative: DriftStatus::WithinNoise,
        })
    );
    assert_eq!(
        b.drift_rows()
            .iter()
            .map(|(c, o, _)| (c.as_str(), o.as_str()))
            .collect::<Vec<_>>(),
        vec![("step", "within-noise"), ("cumulative", "within-noise")],
        "exactly one row per comparison, each naming its own verdict"
    );
    assert!(
        !b.events()
            .iter()
            .any(|e| matches!(e, Event::Blessed { .. })),
        "a baseline that already exists is never re-blessed by the daemon"
    );
}

/// A FIRST diff exiting 3 (assay ≥ 0.10's incomplete comparison) settles
/// without a confirm: spec §4 reserves the confirm for the Drift hypothesis,
/// and an incomplete comparison asserts no drift to reproduce. The row names
/// the settled verdict, and it is never a pass.
#[test]
fn a_first_diff_exiting_three_settles_incomplete_with_no_confirm() {
    let b = boot("watch-incomplete");
    b.seed("qwen.json", &profile_doc_ceiling("qwen", 1024)); // last boot's
    b.seed("qwen.baseline.json", &profile_doc_ceiling("qwen", 900));
    let (runner, probes) = scripted_probes(vec![Ok(profile_doc("qwen"))]);
    let (gate, _calls) = gate_deciding(|reference, _current| {
        if reference.ends_with(".previous.json") {
            exited(3)
        } else {
            exited(0)
        }
    });

    b.run(&runner, &gate);

    assert_eq!(
        probes.borrow().len(),
        1,
        "an incomplete first reading earns no confirm probe beyond the boot's own POST"
    );
    assert_eq!(
        b.drift(),
        Some(ModelDrift {
            step: DriftStatus::Incomplete,
            cumulative: DriftStatus::WithinNoise,
        })
    );
    assert_eq!(
        b.drift_rows()
            .iter()
            .map(|(c, o, _)| (c.as_str(), o.as_str()))
            .collect::<Vec<_>>(),
        vec![("step", "incomplete"), ("cumulative", "within-noise")],
        "one row per comparison; the step row spells the settled verdict, not a pass"
    );
    assert!(
        !b.events()
            .iter()
            .any(|e| matches!(e, Event::Blessed { .. })),
        "a baseline that already exists is never re-blessed by the daemon"
    );
}

/// The rotation law (spec §5), pinned behaviourally: the step comparison's
/// reference is LAST boot's document, because rotation runs before POST's
/// delete-before-probe. Rotating after the probe would leave the step
/// comparison diffing this boot's document against itself — a gate that can
/// only ever read within-noise.
#[test]
fn the_step_reference_is_last_boots_document_rotated_before_this_boots_probe() {
    let b = boot("watch-rotate-first");
    let last_boot = profile_doc_ceiling("qwen", 1024);
    b.seed("qwen.json", &last_boot);
    let (runner, _probes) = scripted_probes(vec![Ok(profile_doc("qwen"))]);
    let (gate, _calls) = gate_deciding(|_r, _c| exited(0));

    b.run(&runner, &gate);

    assert_eq!(
        b.read("qwen.previous.json"),
        last_boot,
        "previous must hold LAST boot's measurement"
    );
    assert_eq!(
        b.read("qwen.json"),
        profile_doc("qwen"),
        "current must hold THIS boot's measurement"
    );
    let step = b
        .events()
        .into_iter()
        .find(|e| matches!(e, Event::Drift { comparison, .. } if comparison == "step"))
        .expect("a step row");
    match step {
        Event::Drift {
            reference_sha,
            current_sha,
            ..
        } => {
            assert_eq!(
                reference_sha,
                Some(sha256_hex_bytes(last_boot.as_bytes())),
                "the step row's reference digest is last boot's document"
            );
            assert_ne!(
                reference_sha, current_sha,
                "a comparison of this boot's document against itself measures nothing"
            );
        }
        other => panic!("expected a Drift row, got {other:?}"),
    }
}

/// Spec §4's confirm-then-alarm: a drift reading is a hypothesis, and the
/// confirm re-probe tests it. Exactly two probes — the boot's, and the one
/// confirm — and the alarm is only raised because the second diff agreed.
#[test]
fn a_step_drift_that_reproduces_is_confirmed_after_exactly_one_re_probe() {
    let b = boot("watch-confirmed");
    let last_boot = profile_doc_ceiling("qwen", 1024);
    b.seed("qwen.json", &last_boot);
    b.seed("qwen.baseline.json", &profile_doc_ceiling("qwen", 900));
    let (runner, probes) = scripted_probes(vec![Ok(profile_doc("qwen"))]);
    // The step reference drifts every time it is asked; the cumulative one
    // does not — so exactly one comparison has a hypothesis to confirm.
    let (gate, _calls) = gate_deciding(|reference, _current| {
        if reference.ends_with(".previous.json") {
            exited(1)
        } else {
            exited(0)
        }
    });

    b.run(&runner, &gate);

    assert_eq!(
        probes.borrow().len(),
        2,
        "one boot probe plus exactly one confirm — never zero, never a retry loop"
    );
    assert_eq!(
        b.drift(),
        Some(ModelDrift {
            step: DriftStatus::Confirmed {
                reference: sha8(&last_boot),
            },
            cumulative: DriftStatus::WithinNoise,
        })
    );
    let rows = b.drift_rows();
    assert_eq!(
        rows.iter()
            .map(|(c, o, _)| (c.as_str(), o.as_str()))
            .collect::<Vec<_>>(),
        vec![
            ("step", "drift"),
            ("step", "confirmed"),
            ("cumulative", "within-noise")
        ],
        "the first reading journals what the gate said; its confirm journals the verdict that \
         settled it — `confirmed`, never the raw `drift` word again: {rows:?}"
    );
    assert!(
        is_transient(&rows[1].2),
        "the confirm's row must name the fresh document it compared, got {:?}",
        rows[1].2
    );
    assert!(
        std::path::Path::new(&rows[1].2).exists(),
        "the confirm document the row names must be on disk to be checkable"
    );
    assert_eq!(
        b.read("qwen.json"),
        profile_doc("qwen"),
        "the confirm probe never overwrites this boot's measurement"
    );
}

/// Spec §4's second outcome, and assay's founding finding: the serving state
/// moved between two probes of one boot. That is a finding of its own, not an
/// alarm — and the document that failed to reproduce is kept beside the row.
#[test]
fn a_step_drift_that_does_not_reproduce_is_transient_and_its_document_is_kept() {
    let b = boot("watch-transient");
    b.seed("qwen.json", &profile_doc_ceiling("qwen", 1024));
    b.seed("qwen.baseline.json", &profile_doc_ceiling("qwen", 900));
    let (runner, probes) = scripted_probes(vec![Ok(profile_doc("qwen"))]);
    let (gate, _calls) = gate_deciding(|reference, current| {
        if reference.ends_with(".previous.json") && !is_transient(current) {
            exited(1)
        } else {
            exited(0)
        }
    });

    b.run(&runner, &gate);

    assert_eq!(probes.borrow().len(), 2);
    assert_eq!(
        b.drift().map(|d| d.step),
        Some(DriftStatus::Transient),
        "a reading that does not reproduce is transient, never confirmed"
    );
    let kept = b.transients();
    assert_eq!(kept.len(), 1, "the confirm document is retained: {kept:?}");
    assert_eq!(
        std::fs::read_to_string(&kept[0]).unwrap(),
        profile_doc("qwen")
    );
    let step_rows: Vec<(String, String, String)> = b
        .drift_rows()
        .into_iter()
        .filter(|(c, _, _)| c == "step")
        .collect();
    assert_eq!(
        step_rows.len(),
        2,
        "both the reading and its confirm are journaled"
    );
    assert_eq!(
        step_rows[1].1, "transient",
        "the confirm's row spells the finding — a transient is NOT the `within-noise` a clean \
         boot gets, and the two must never share a word: {step_rows:?}"
    );
}

/// Spec §4's wedged-confirm rule: when the confirm probe itself fails there is
/// no second reading, so the first one stands as `unconfirmed` — NAMED, and
/// never silently upgraded to `Confirmed`.
#[test]
fn a_confirm_probe_that_fails_leaves_the_reading_unconfirmed_and_never_upgrades_it() {
    let b = boot("watch-unconfirmed");
    b.seed("qwen.json", &profile_doc_ceiling("qwen", 1024));
    b.seed("qwen.baseline.json", &profile_doc_ceiling("qwen", 900));
    let (runner, probes) = scripted_probes(vec![Ok(profile_doc("qwen")), Err(4)]);
    let (gate, _calls) = gate_deciding(|reference, _current| {
        if reference.ends_with(".previous.json") {
            exited(1)
        } else {
            exited(0)
        }
    });

    b.run(&runner, &gate);

    assert_eq!(probes.borrow().len(), 2, "the confirm was attempted");
    match b.drift().map(|d| d.step) {
        Some(DriftStatus::Unconfirmed { reason }) => assert!(
            reason.contains("assay exited 4") && reason.contains("cannot reach model"),
            "the failure that prevented the confirm must be named: {reason:?}"
        ),
        other => panic!("expected Unconfirmed after a failed confirm probe, got {other:?}"),
    }
    assert_eq!(
        b.drift_rows()
            .iter()
            .filter(|(c, _, _)| c == "step")
            .count(),
        1,
        "a confirm that never produced a document journals no second comparison"
    );
    // …but it does not vanish either: the probe can burn the whole
    // `probe_timeout_secs` window and die, and spec §4 says a confirm that
    // could not be made journals as infrastructure. A status field is not a
    // record.
    assert!(
        b.events().iter().any(|e| matches!(e,
            Event::Degraded { reason }
                if reason.contains("confirm probe")
                    && reason.contains("qwen")
                    && reason.contains("step")
                    && reason.contains("assay exited 4"))),
        "the failed confirm must leave a durable row naming the model, the comparison and the \
         failure: {:?}",
        b.events()
    );
}

/// Spec §4's third outcome: the confirm's re-diff refusing to compare is
/// infrastructure-shaped, not a drift verdict — so the reading stays
/// unconfirmed, naming what the re-diff answered.
#[test]
fn a_confirm_re_diff_that_refuses_is_unconfirmed_naming_the_refusal() {
    let b = boot("watch-unconfirmed-refusal");
    b.seed("qwen.json", &profile_doc_ceiling("qwen", 1024));
    b.seed("qwen.baseline.json", &profile_doc_ceiling("qwen", 900));
    let (runner, _probes) = scripted_probes(vec![Ok(profile_doc("qwen"))]);
    let (gate, _calls) = gate_deciding(|reference, current| {
        if !reference.ends_with(".previous.json") {
            exited(0)
        } else if is_transient(current) {
            exited(2)
        } else {
            exited(1)
        }
    });

    b.run(&runner, &gate);

    match b.drift().map(|d| d.step) {
        Some(DriftStatus::Unconfirmed { reason }) => assert!(
            reason.contains("not-comparable"),
            "the re-diff's own answer must be named: {reason:?}"
        ),
        other => panic!("expected Unconfirmed for a re-diff that refused, got {other:?}"),
    }
    let step_rows: Vec<(String, String, String)> = b
        .drift_rows()
        .into_iter()
        .filter(|(c, _, _)| c == "step")
        .collect();
    assert_eq!(
        step_rows[1].1, "unconfirmed: not-comparable",
        "the confirm's row names both the verdict and what the re-diff answered: {step_rows:?}"
    );
}

/// The pinned ordering (spec §2 + the controller's ruling): the first profile
/// auto-blesses AFTER this boot's comparisons have run, so the cumulative
/// comparison on that boot honestly reads `unmeasured` — there was no baseline
/// when it was asked. Blessing first would hand the gate a baseline byte-identical
/// to the current document and manufacture a within-noise pass out of nothing.
#[test]
fn the_first_profile_auto_blesses_after_the_comparisons_so_cumulative_reads_unmeasured() {
    let b = boot("watch-auto-bless");
    let (runner, _probes) = scripted_probes(vec![Ok(profile_doc("qwen"))]);
    let (gate, _calls) = gate_deciding(|_r, _c| exited(0));

    b.run(&runner, &gate);

    match b.drift().map(|d| d.cumulative) {
        Some(DriftStatus::Unmeasured { reason }) => assert!(
            reason.contains("qwen.baseline.json"),
            "the missing reference must be named: {reason:?}"
        ),
        other => panic!("expected Unmeasured cumulative on the blessing boot, got {other:?}"),
    }
    assert_eq!(
        b.read("qwen.baseline.json"),
        profile_doc("qwen"),
        "the baseline is this boot's document, byte for byte"
    );
    let blessed = b
        .events()
        .into_iter()
        .find(|e| matches!(e, Event::Blessed { .. }))
        .expect("the first profile is blessed");
    match blessed {
        Event::Blessed {
            model,
            profile_path,
            sha,
            provenance,
        } => {
            assert_eq!(model, "qwen");
            assert!(profile_path.ends_with("qwen.baseline.json"));
            assert_eq!(sha, sha256_hex_bytes(profile_doc("qwen").as_bytes()));
            assert_eq!(
                provenance, "auto-first-profile",
                "the provenance of every baseline is explicit"
            );
        }
        other => panic!("expected a Blessed row, got {other:?}"),
    }
}

/// `ModelStatus.drift` is `None` when the drift watch never ran for that model
/// this boot — the same None-honesty `done_trust` has: absent is not clean.
#[test]
fn a_model_whose_post_failed_has_no_drift_reading_at_all() {
    let b = boot("watch-post-failed");
    let (runner, _probes) = scripted_probes(vec![Err(4)]);
    let (gate, calls) = gate_deciding(|_r, _c| exited(0));

    b.run(&runner, &gate);

    assert_eq!(
        b.drift(),
        None,
        "no measurement means no verdict — absent, never a clean one"
    );
    assert!(
        b.drift_rows().is_empty(),
        "a boot with no current document has nothing to compare"
    );
    assert!(
        calls.borrow().is_empty(),
        "no comparison is attempted at all"
    );
}

/// The rendered surface: both fields present under their own names, and a
/// model that was never compared renders `null` rather than a verdict.
#[test]
fn status_renders_the_drift_pair_and_null_when_it_never_ran() {
    let b = boot("watch-status-json");
    b.seed("qwen.json", &profile_doc_ceiling("qwen", 1024));
    b.seed("qwen.baseline.json", &profile_doc_ceiling("qwen", 900));
    let (runner, _probes) = scripted_probes(vec![Ok(profile_doc("qwen"))]);
    let (gate, _calls) = gate_deciding(|reference, current| {
        if reference.ends_with(".previous.json") && !is_transient(current) {
            exited(1)
        } else {
            exited(0)
        }
    });

    let unmeasured: serde_json::Value = {
        let p = b.pager.lock().unwrap();
        serde_json::to_value(p.status()).unwrap()
    };
    assert_eq!(
        unmeasured["models"][0]["drift"],
        serde_json::Value::Null,
        "before the watch runs, drift is null — absent, not clean"
    );

    b.run(&runner, &gate);

    let rendered: serde_json::Value = {
        let p = b.pager.lock().unwrap();
        serde_json::to_value(p.status()).unwrap()
    };
    let drift = &rendered["models"][0]["drift"];
    assert_eq!(drift["step"]["status"], "transient");
    assert_eq!(drift["cumulative"]["status"], "within-noise");
    assert_eq!(
        rendered["models"][0]["done_trust"],
        serde_json::Value::Null,
        "drift is its own field and says nothing about done_trust"
    );
}

/// Spec §5's rotation-on-successful-parse rule, from the boot's side: a
/// corrupt current document is never promoted to "the previous boot's
/// measurement", the older good reference survives, and the degradation of the
/// drift record is journaled — POST's delete-before-probe then reclaims the
/// bytes, so the row is what remains of them.
#[test]
fn an_unparseable_current_document_is_kept_out_of_previous_and_journaled() {
    let b = boot("watch-corrupt-current");
    let older_good = profile_doc_ceiling("qwen", 512);
    b.seed("qwen.previous.json", &older_good);
    b.seed("qwen.json", "{ truncated json");
    let (runner, _probes) = scripted_probes(vec![Ok(profile_doc("qwen"))]);
    let (gate, _calls) = gate_deciding(|_r, _c| exited(0));

    b.run(&runner, &gate);

    assert_eq!(
        b.read("qwen.previous.json"),
        older_good,
        "the previous reference already on disk survives untouched"
    );
    assert!(
        b.events().iter().any(|e| matches!(e,
            Event::Degraded { reason }
                if reason.contains("qwen.json") && reason.contains("drift"))),
        "the unpromotable document must be named in the journal: {:?}",
        b.events()
    );
}

/// Spec §5's bound: retention keeps the latest N transients per model, and a
/// file this daemon deleted is a fact about the evidence trail — journaled,
/// never quiet housekeeping.
#[test]
fn a_dropped_transient_is_journaled_by_name() {
    let b = boot("watch-transient-bound");
    b.seed("qwen.json", &profile_doc_ceiling("qwen", 1024));
    b.seed("qwen.baseline.json", &profile_doc_ceiling("qwen", 900));
    // Fill the bound with older confirm documents from earlier boots.
    let old = std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
    for i in 0..MAX_TRANSIENTS {
        let name = format!("qwen.transient-0000000{i}.json");
        b.seed(&name, &profile_doc_ceiling("qwen", 100 + i as u32));
        set_mtime(&b.profiles.join(&name), old);
    }
    let (runner, _probes) = scripted_probes(vec![Ok(profile_doc("qwen"))]);
    let (gate, _calls) = gate_deciding(|reference, current| {
        if reference.ends_with(".previous.json") && !is_transient(current) {
            exited(1)
        } else {
            exited(0)
        }
    });

    b.run(&runner, &gate);

    assert_eq!(
        b.transients().len(),
        MAX_TRANSIENTS,
        "the bound holds after the confirm run files its document"
    );
    assert!(
        b.events().iter().any(|e| matches!(e,
            Event::Degraded { reason }
                if reason.contains("qwen.transient-00000000.json"))),
        "the dropped document must be named: {:?}",
        b.events()
    );
}

// ---------------------------------------------------------------------------
