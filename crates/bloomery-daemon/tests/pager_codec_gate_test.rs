//! Task 6: the pager's per-model codec-gate state, the fail-closed
//! verb-policy accessors (`model_mutating_verbs`/`model_patch_codec`), and
//! `/status` surfacing — `docs/superpowers/evidence/2026-08-15-g4-protocol.md`
//! §3/§4/§6.
//!
//! Every test here is GPU-free against [`FakeSubstrate`], the same style as
//! `pager_test.rs`.

use bloomery_core::action::PatchCodec;
use bloomery_core::journal::{replay, Event, Journal};
use bloomery_core::profile::Profile;
use bloomery_daemon::agents::ImageStore;
use bloomery_daemon::config::EnvelopeLens;
use bloomery_daemon::pager::*;
use bloomery_substrate::fake::FakeSubstrate;
use std::path::{Path, PathBuf};

fn meta() -> bloomery_core::gguf::GgufMeta {
    bloomery_core::gguf::GgufMeta {
        arch: "qwen2".into(),
        layers: 28,
        attention_layers: 28,
        kv_heads: 4,
        head_dim: 128,
        training_ctx: 4096,
        weights_bytes: 1000,
        recurrent_state_bytes: 0,
    }
}

/// A clean scratch dir per test, so runs never share journals or images.
fn fresh_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn write_gguf(dir: &Path, contents: &[u8]) -> PathBuf {
    let gguf = dir.join("fake.gguf");
    std::fs::write(&gguf, contents).unwrap();
    gguf
}

/// A pager with a "qwen" model registered (no profile), roomy VRAM.
fn pager_with_model(dir: &Path) -> (Pager<FakeSubstrate>, PathBuf) {
    let jpath = dir.join("j.jsonl");
    let journal = Journal::open(&jpath).unwrap();
    let images = ImageStore::new(&dir.join("img")).unwrap();
    let fake = FakeSubstrate::new();
    let mut p = Pager::new(fake, journal, images, Box::new(|| Some(10u64.pow(9))));
    let gguf = write_gguf(dir, b"weights");
    p.register_model("qwen", &gguf, meta(), None).unwrap();
    (p, jpath)
}

fn keep_gate() -> CodecGateResult {
    CodecGateResult {
        fixture_set: "codec-tasks-v1".to_string(),
        codec: PatchCodec::SearchReplace,
        landed: 17,
        n: 20,
        interval95: (0.60, 0.94),
        provisional: false,
        mutating_verbs: true,
    }
}

fn demote_gate() -> CodecGateResult {
    CodecGateResult {
        fixture_set: "codec-tasks-v1".to_string(),
        codec: PatchCodec::SearchReplace,
        landed: 8,
        n: 20,
        interval95: (0.21, 0.61),
        provisional: false,
        mutating_verbs: false,
    }
}

// ---------------------------------------------------------------------------
// Fail-closed: unmeasured
// ---------------------------------------------------------------------------

/// Protocol §3/§6: no stored gate reads exactly like a demotion, never like
/// permission. `/status` must render `codec_gate: null`, never a confident
/// zero.
#[test]
fn unmeasured_model_is_fail_closed_read_only_and_status_shows_null() {
    let dir = fresh_dir("bloomery-codec-gate-unmeasured");
    let (p, _) = pager_with_model(&dir);

    assert!(!p.model_mutating_verbs("qwen"));

    let status = p.status();
    let m = &status.models[0];
    assert!(!m.mutating_verbs);
    assert!(m.codec_gate.is_none());

    let json = serde_json::to_string(&status).unwrap();
    assert!(json.contains("\"codec_gate\":null"), "{json}");
    assert!(
        !json.contains("\"landed\":0"),
        "unmeasured must never render as a confident zero: {json}"
    );
}

/// Fail-closed for a model that was never registered at all.
#[test]
fn unknown_model_mutating_verbs_is_false() {
    let dir = fresh_dir("bloomery-codec-gate-unknown-verbs");
    let (p, _) = pager_with_model(&dir);
    assert!(!p.model_mutating_verbs("nope"));
}

// ---------------------------------------------------------------------------
// Stored gate: keep / demote
// ---------------------------------------------------------------------------

#[test]
fn stored_keep_gate_enables_mutating_verbs_and_populates_status() {
    let dir = fresh_dir("bloomery-codec-gate-keep");
    let (mut p, _) = pager_with_model(&dir);
    p.set_codec_gate("qwen", keep_gate()).unwrap();

    assert!(p.model_mutating_verbs("qwen"));

    let status = p.status();
    let m = &status.models[0];
    assert!(m.mutating_verbs);
    let gate = m.codec_gate.as_ref().expect("gate stored");
    assert_eq!(gate.fixture_set, "codec-tasks-v1");
    assert_eq!(gate.codec, "search_replace");
    assert_eq!(gate.landed, 17);
    assert_eq!(gate.n, 20);
    assert_eq!(gate.interval95, [0.60, 0.94]);
    assert!(!gate.provisional);
}

#[test]
fn stored_demote_gate_disables_mutating_verbs_but_is_still_a_measurement() {
    let dir = fresh_dir("bloomery-codec-gate-demote");
    let (mut p, _) = pager_with_model(&dir);
    p.set_codec_gate("qwen", demote_gate()).unwrap();

    assert!(!p.model_mutating_verbs("qwen"));
    let status = p.status();
    assert!(!status.models[0].mutating_verbs);
    assert!(
        status.models[0].codec_gate.is_some(),
        "a demoted gate is still a measurement, not unmeasured"
    );
}

#[test]
fn set_codec_gate_on_unknown_model_is_named() {
    let dir = fresh_dir("bloomery-codec-gate-unknown-set");
    let (mut p, _) = pager_with_model(&dir);
    match p.set_codec_gate("nope", keep_gate()) {
        Err(PagerError::UnknownModel(m)) => assert_eq!(m, "nope"),
        other => panic!("expected UnknownModel, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// model_patch_codec (protocol §4)
// ---------------------------------------------------------------------------

const WF_WINS_PROFILE: &str = r#"{
  "assay_profile_version": 3,
  "probe_version": "0.4.1",
  "model": {"name": "qwen"},
  "codecs": {
    "search_replace": {"small": {"lands": 0.5, "lands_applies": 0.6, "n": 20}},
    "whole_file": {"small": {"lands": 0.8, "lands_applies": 0.9, "n": 20}}
  }
}"#;

#[test]
fn model_patch_codec_follows_the_attached_profiles_selection() {
    let dir = fresh_dir("bloomery-codec-gate-patchcodec");
    let (mut p, _) = pager_with_model(&dir);
    let profile = Profile::from_json(WF_WINS_PROFILE).unwrap();
    p.attach_profile("qwen", profile, false).unwrap();

    assert_eq!(p.model_patch_codec("qwen"), PatchCodec::WholeFile);
}

#[test]
fn model_patch_codec_defaults_search_replace_when_unprofiled_or_unknown() {
    let dir = fresh_dir("bloomery-codec-gate-unprofiled");
    let (p, _) = pager_with_model(&dir);
    assert_eq!(p.model_patch_codec("qwen"), PatchCodec::SearchReplace);
    assert_eq!(p.model_patch_codec("nope"), PatchCodec::SearchReplace);
}

/// Protocol §4's provenance question, which the codec *value* alone cannot
/// answer: `SearchReplace` is both a legitimate measured selection and the
/// untested fallback, and the G4 verdict's `detail` has to say which. The
/// asymmetry that matters here is that a profile whose `codecs` grid is
/// empty reads exactly like no profile at all.
#[test]
fn model_codec_from_profile_separates_a_measured_selection_from_the_default() {
    let dir = fresh_dir("bloomery-codec-gate-provenance");
    let (mut p, _) = pager_with_model(&dir);
    assert!(
        !p.model_codec_from_profile("qwen"),
        "unprofiled: the codec is the default"
    );
    assert!(!p.model_codec_from_profile("nope"), "unknown model");

    const NO_CODECS_PROFILE: &str = r#"{
      "assay_profile_version": 3,
      "probe_version": "0.4.1",
      "model": {"name": "qwen"}
    }"#;
    p.attach_profile(
        "qwen",
        Profile::from_json(NO_CODECS_PROFILE).unwrap(),
        false,
    )
    .unwrap();
    assert_eq!(p.model_patch_codec("qwen"), PatchCodec::SearchReplace);
    assert!(
        !p.model_codec_from_profile("qwen"),
        "a profile with no codecs grid is still the default, not a selection"
    );

    p.attach_profile("qwen", Profile::from_json(WF_WINS_PROFILE).unwrap(), false)
        .unwrap();
    assert!(p.model_codec_from_profile("qwen"));
}

// ---------------------------------------------------------------------------
// agent_task_policy
// ---------------------------------------------------------------------------

#[test]
fn agent_task_policy_resolves_through_the_agents_model() {
    let dir = fresh_dir("bloomery-codec-gate-agentpolicy");
    let (mut p, _) = pager_with_model(&dir);
    let profile = Profile::from_json(WF_WINS_PROFILE).unwrap();
    p.attach_profile("qwen", profile, false).unwrap();
    p.set_codec_gate("qwen", keep_gate()).unwrap();
    let a = p.create_agent("qwen", 50, None, 10_000).unwrap();

    assert_eq!(
        p.agent_task_policy(&a.id),
        Some((PatchCodec::WholeFile, true, EnvelopeLens::V1))
    );
}

#[test]
fn agent_task_policy_is_none_for_an_unknown_agent() {
    let dir = fresh_dir("bloomery-codec-gate-agentpolicy-unknown");
    let (p, _) = pager_with_model(&dir);
    assert_eq!(p.agent_task_policy("nope"), None);
}

/// Protocol §10 (Amendment 2): `agent_task_policy`'s third field resolves a
/// preseeded model's envelope through the same one-source lookup as
/// `patch_codec`/`mutating_verbs` — set via `Pager::set_model_envelope`.
#[test]
fn agent_task_policy_resolves_envelope_through_the_agents_model() {
    let dir = fresh_dir("bloomery-codec-gate-agentpolicy-preseed");
    let (mut p, _) = pager_with_model(&dir);
    p.attach_profile("qwen", Profile::from_json(WF_WINS_PROFILE).unwrap(), false)
        .unwrap();
    p.set_codec_gate("qwen", keep_gate()).unwrap();
    p.set_model_envelope("qwen", EnvelopeLens::V2).unwrap();
    let a = p.create_agent("qwen", 50, None, 10_000).unwrap();

    assert_eq!(
        p.agent_task_policy(&a.id),
        Some((PatchCodec::WholeFile, true, EnvelopeLens::V2))
    );
}

// ---------------------------------------------------------------------------
// journal_codec_fixture / journal_codec_verdict round-trips (fix round 1:
// closes the review finding that neither wrapper had any coverage). Every
// field that could be swapped with a neighbor — the two adjacent bools
// (`provisional`/`mutating_verbs`), the two adjacent counts (`landed`/`n`),
// the two `interval95` endpoints, and the run of `String` fields — is given
// a distinct, asymmetric value here so a field-order swap or a value-mapping
// error in either wrapper flips a byte the `assert_eq!` against the full
// replayed `Event` will catch.
// ---------------------------------------------------------------------------

#[test]
fn journal_codec_fixture_round_trips_through_replay() {
    let dir = fresh_dir("bloomery-codec-gate-journal-fixture");
    let (mut p, jpath) = pager_with_model(&dir);

    p.journal_codec_fixture(
        "qwen-fixture-model",
        "codec-tasks-v1",
        "py-fix-off-by-one",
        PatchCodec::WholeFile,
        true,
        4,
        "applies_and_parses",
        "patch",
    )
    .unwrap();

    let events = replay(&jpath).unwrap();
    assert_eq!(
        events,
        vec![Event::CodecFixture {
            model: "qwen-fixture-model".to_string(),
            fixture_set: "codec-tasks-v1".to_string(),
            fixture: "py-fix-off-by-one".to_string(),
            codec: "whole_file".to_string(),
            landed: true,
            steps: 4,
            detail: "applies_and_parses".to_string(),
            expect: "patch".to_string(),
        }]
    );
}

/// The `expect` field round-trips its OTHER value too — a refuse-class
/// fixture row, asymmetric from the test above on every field (including
/// `landed: false` vs `true`) so a copy-paste of the wrong literal would be
/// caught.
#[test]
fn journal_codec_fixture_round_trips_a_refuse_class_row() {
    let dir = fresh_dir("bloomery-codec-gate-journal-fixture-refuse");
    let (mut p, jpath) = pager_with_model(&dir);

    p.journal_codec_fixture(
        "qwen-fixture-model",
        "codec-tasks-v2-mixed",
        "defect-absent-example",
        PatchCodec::SearchReplace,
        false,
        2,
        "refuse leg (a) failed: a patch step succeeded — not a refusal",
        "refuse",
    )
    .unwrap();

    let events = replay(&jpath).unwrap();
    assert_eq!(
        events,
        vec![Event::CodecFixture {
            model: "qwen-fixture-model".to_string(),
            fixture_set: "codec-tasks-v2-mixed".to_string(),
            fixture: "defect-absent-example".to_string(),
            codec: "search_replace".to_string(),
            landed: false,
            steps: 2,
            detail: "refuse leg (a) failed: a patch step succeeded — not a refusal".to_string(),
            expect: "refuse".to_string(),
        }]
    );
}

// ---------------------------------------------------------------------------
// G5: RefusalGateResult / set_refusal_gate / journal_codec_verdict_mixed /
// done-trust `/status` rendering (docs/superpowers/evidence/2026-08-16-g5-protocol.md)
// ---------------------------------------------------------------------------

fn done_trust_gate() -> RefusalGateResult {
    RefusalGateResult {
        fixture_set: "codec-tasks-v2-mixed".to_string(),
        codec: PatchCodec::SearchReplace,
        patch_landed: 9,
        patch_n: 10,
        patch_interval95: (0.59, 0.98),
        patch_provisional: true,
        refuse_landed: 8,
        refuse_n: 10,
        refuse_interval95: (0.49, 0.94),
        refuse_provisional: true,
        done_trust: true,
    }
}

fn no_done_trust_gate() -> RefusalGateResult {
    RefusalGateResult {
        fixture_set: "codec-tasks-v2-mixed".to_string(),
        codec: PatchCodec::SearchReplace,
        patch_landed: 9,
        patch_n: 10,
        patch_interval95: (0.59, 0.98),
        patch_provisional: true,
        refuse_landed: 3,
        refuse_n: 10,
        refuse_interval95: (0.11, 0.60),
        refuse_provisional: false,
        done_trust: false,
    }
}

/// Protocol §4's "unmeasured, never a fake pass": no stored refusal gate
/// must render `done_trust: null` and `refusal_gate: null`, never `false`
/// (a real, decided fail) and never a confident zero count.
#[test]
fn unmeasured_model_has_null_done_trust_and_null_refusal_gate() {
    let dir = fresh_dir("bloomery-refusal-gate-unmeasured");
    let (p, _) = pager_with_model(&dir);

    let status = p.status();
    let m = &status.models[0];
    assert!(m.done_trust.is_none());
    assert!(m.refusal_gate.is_none());

    let json = serde_json::to_string(&status).unwrap();
    assert!(json.contains("\"done_trust\":null"), "{json}");
    assert!(json.contains("\"refusal_gate\":null"), "{json}");
}

/// Storing a gate whose classes both clear the floor renders `done_trust:
/// true` and populates every per-class number on `/status`.
#[test]
fn stored_gate_with_both_classes_clear_renders_done_trust_true() {
    let dir = fresh_dir("bloomery-refusal-gate-both-clear");
    let (mut p, _) = pager_with_model(&dir);
    p.set_refusal_gate("qwen", done_trust_gate()).unwrap();

    let status = p.status();
    let m = &status.models[0];
    assert_eq!(m.done_trust, Some(true));
    let gate = m.refusal_gate.as_ref().expect("gate stored");
    assert_eq!(gate.fixture_set, "codec-tasks-v2-mixed");
    assert_eq!(gate.codec, "search_replace");
    assert_eq!(gate.patch_landed, 9);
    assert_eq!(gate.patch_n, 10);
    assert_eq!(gate.refuse_landed, 8);
    assert_eq!(gate.refuse_n, 10);
    assert_eq!(gate.patch_interval95, [0.59, 0.98]);
    assert_eq!(gate.refuse_interval95, [0.49, 0.94]);
    assert!(gate.patch_provisional);
    assert!(gate.refuse_provisional);
}

/// A gate where only ONE class clears the floor renders `done_trust: false`
/// — the exact model G5 exists to catch (aces one class, fails the other).
#[test]
fn stored_gate_with_one_class_failing_renders_done_trust_false() {
    let dir = fresh_dir("bloomery-refusal-gate-one-fails");
    let (mut p, _) = pager_with_model(&dir);
    p.set_refusal_gate("qwen", no_done_trust_gate()).unwrap();

    let status = p.status();
    assert_eq!(status.models[0].done_trust, Some(false));
    assert!(
        status.models[0].refusal_gate.is_some(),
        "a failed class is still a measurement, not unmeasured"
    );
}

/// G5 is advisory (design doc §3): storing a refusal gate must never touch
/// `mutating_verbs` or the classic `codec_gate` — the two gates are
/// independent state.
#[test]
fn set_refusal_gate_never_touches_mutating_verbs_or_codec_gate() {
    let dir = fresh_dir("bloomery-refusal-gate-advisory-only");
    let (mut p, _) = pager_with_model(&dir);
    p.set_refusal_gate("qwen", no_done_trust_gate()).unwrap();

    assert!(
        !p.model_mutating_verbs("qwen"),
        "an unmeasured G4 gate is still unmeasured after a G5 gate is stored"
    );
    assert!(p.status().models[0].codec_gate.is_none());
}

#[test]
fn set_refusal_gate_on_unknown_model_is_named() {
    let dir = fresh_dir("bloomery-refusal-gate-unknown");
    let (mut p, _) = pager_with_model(&dir);
    match p.set_refusal_gate("nope", done_trust_gate()) {
        Err(PagerError::UnknownModel(m)) => assert_eq!(m, "nope"),
        other => panic!("expected UnknownModel, got {other:?}"),
    }
}

/// `journal_codec_verdict_mixed` round-trips through replay — asymmetric on
/// every field pair that could be swapped (patch vs refuse counts,
/// intervals, provisional flags), same discipline as the G4 verdict
/// round-trip test above.
#[test]
fn journal_codec_verdict_mixed_round_trips_through_replay() {
    let dir = fresh_dir("bloomery-refusal-gate-journal-verdict");
    let (mut p, jpath) = pager_with_model(&dir);

    p.journal_codec_verdict_mixed(
        "qwen-verdict-model",
        "codec-tasks-v2-mixed",
        PatchCodec::WholeFile,
        "bloomery-task-envelope-v2",
        &done_trust_gate(),
        "codec from profile",
    )
    .unwrap();

    let events = replay(&jpath).unwrap();
    assert_eq!(
        events,
        vec![Event::CodecVerdictMixed {
            model: "qwen-verdict-model".to_string(),
            fixture_set: "codec-tasks-v2-mixed".to_string(),
            codec: "whole_file".to_string(),
            envelope: "bloomery-task-envelope-v2".to_string(),
            patch_landed: 9,
            patch_n: 10,
            patch_interval95: [0.59, 0.98],
            patch_provisional: true,
            refuse_landed: 8,
            refuse_n: 10,
            refuse_interval95: [0.49, 0.94],
            refuse_provisional: true,
            done_trust: true,
            detail: "codec from profile".to_string(),
        }]
    );
}

#[test]
fn journal_codec_verdict_round_trips_through_replay() {
    let dir = fresh_dir("bloomery-codec-gate-journal-verdict");
    let (mut p, jpath) = pager_with_model(&dir);

    // landed != n, interval95 endpoints distinct, provisional != mutating_verbs
    // — asymmetric on purpose (see module note above).
    p.journal_codec_verdict(
        "qwen-verdict-model",
        "codec-tasks-v1",
        PatchCodec::SearchReplace,
        13,
        20,
        (0.31, 0.79),
        true,
        false,
        "applies_and_parses under bloomery-task-envelope-v1",
    )
    .unwrap();

    let events = replay(&jpath).unwrap();
    assert_eq!(
        events,
        vec![Event::CodecVerdict {
            model: "qwen-verdict-model".to_string(),
            fixture_set: "codec-tasks-v1".to_string(),
            codec: "search_replace".to_string(),
            landed: 13,
            n: 20,
            interval95: [0.31, 0.79],
            provisional: true,
            mutating_verbs: false,
            detail: "applies_and_parses under bloomery-task-envelope-v1".to_string(),
        }]
    );
}
