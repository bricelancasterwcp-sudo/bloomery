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
use bloomery_daemon::pager::*;
use bloomery_substrate::fake::FakeSubstrate;
use std::path::{Path, PathBuf};

fn meta() -> bloomery_core::gguf::GgufMeta {
    bloomery_core::gguf::GgufMeta {
        arch: "qwen2".into(),
        layers: 28,
        kv_heads: 4,
        head_dim: 128,
        training_ctx: 4096,
        weights_bytes: 1000,
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
        Some((PatchCodec::WholeFile, true))
    );
}

#[test]
fn agent_task_policy_is_none_for_an_unknown_agent() {
    let dir = fresh_dir("bloomery-codec-gate-agentpolicy-unknown");
    let (p, _) = pager_with_model(&dir);
    assert_eq!(p.agent_task_policy("nope"), None);
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
