use bloomery_core::action::PatchCodec;
use bloomery_core::profile::{instrument_precheck, InstrumentPrecheck, Profile, Verdict};

const FIXTURE: &str = r#"{
  "assay_profile_version": 3,
  "probe_version": "0.4.1",
  "model": {"name": "qwen2.5-coder:7b-instruct-q8_0", "quant": "Q8_0"},
  "ceiling": {"max_verified": 15800, "first_failure": null, "failure_mode": "none_up_to_cap"},
  "verdicts": {
    "structured_extraction": {"verdict": "ready"},
    "patch_editing": {"verdict": "risky"},
    "long_context": {"verdict": "unmeasured"}
  }
}"#;

#[test]
fn parses_v3_fixture() {
    let p = Profile::from_json(FIXTURE).unwrap();
    assert_eq!(p.model_name(), "qwen2.5-coder:7b-instruct-q8_0");
    assert_eq!(p.measured_ceiling(), Some(15800));
    assert_eq!(p.verdict("structured_extraction"), Verdict::Ready);
    assert_eq!(p.verdict("patch_editing"), Verdict::Risky);
    assert_eq!(p.verdict("long_context"), Verdict::Unmeasured);
    assert_eq!(p.verdict("nonexistent"), Verdict::Unmeasured); // absent = unmeasured, law 5
}

#[test]
fn missing_ceiling_is_none() {
    let p = Profile::from_json(
        r#"{"assay_profile_version": 3, "probe_version": "0.4.1", "model": {"name": "m"}}"#,
    )
    .unwrap();
    assert_eq!(p.measured_ceiling(), None);
}

#[test]
fn old_schema_rejected_by_name() {
    let e = Profile::from_json(
        r#"{"assay_profile_version": 1, "probe_version": "0.1.0", "model": {"name": "m"}}"#,
    );
    assert!(matches!(
        e,
        Err(bloomery_core::profile::ProfileError::UnsupportedSchema(1))
    ));
}

#[test]
fn profile_derives_deserialize_directly() {
    let p: Profile = serde_json::from_str(FIXTURE).unwrap();
    assert_eq!(p.model_name(), "qwen2.5-coder:7b-instruct-q8_0");
}

// --- codecs grid + preferred_patch_codec (protocol §4) -----------------

fn codec_profile(codecs_body: &str) -> Profile {
    let doc = format!(
        r#"{{
  "assay_profile_version": 3,
  "probe_version": "0.4.1",
  "model": {{"name": "m"}},
  "codecs": {codecs_body}
}}"#
    );
    Profile::from_json(&doc).unwrap()
}

#[test]
fn whole_file_wins_at_small_when_strictly_greater() {
    let p = codec_profile(
        r#"{
          "search_replace": {"small": {"lands": 0.5, "lands_applies": 0.6, "n": 5}},
          "whole_file": {"small": {"lands": 0.8, "lands_applies": 0.9, "n": 5}}
        }"#,
    );
    assert_eq!(p.preferred_patch_codec(), Some(PatchCodec::WholeFile));
}

#[test]
fn search_replace_wins_at_small_when_strictly_greater() {
    let p = codec_profile(
        r#"{
          "search_replace": {"small": {"lands": 0.8, "lands_applies": 0.9, "n": 5}},
          "whole_file": {"small": {"lands": 0.5, "lands_applies": 0.6, "n": 5}}
        }"#,
    );
    assert_eq!(p.preferred_patch_codec(), Some(PatchCodec::SearchReplace));
}

#[test]
fn tie_at_small_prefers_search_replace() {
    let p = codec_profile(
        r#"{
          "search_replace": {"small": {"lands": 0.8, "lands_applies": 0.8, "n": 5}},
          "whole_file": {"small": {"lands": 0.8, "lands_applies": 0.8, "n": 5}}
        }"#,
    );
    assert_eq!(p.preferred_patch_codec(), Some(PatchCodec::SearchReplace));
}

#[test]
fn only_search_replace_measured_selects_search_replace() {
    let p = codec_profile(
        r#"{
          "search_replace": {"small": {"lands": 0.6, "lands_applies": 0.8, "n": 5}},
          "whole_file": {"small": {"lands": null, "lands_applies": null, "n": 5}}
        }"#,
    );
    assert_eq!(p.preferred_patch_codec(), Some(PatchCodec::SearchReplace));
}

#[test]
fn only_whole_file_measured_selects_whole_file() {
    let p = codec_profile(
        r#"{
          "search_replace": {"small": {"lands": null, "lands_applies": null, "n": 5}},
          "whole_file": {"small": {"lands": 0.6, "lands_applies": 0.8, "n": 5}}
        }"#,
    );
    assert_eq!(p.preferred_patch_codec(), Some(PatchCodec::WholeFile));
}

#[test]
fn codecs_key_absent_selects_none() {
    let p = Profile::from_json(
        r#"{"assay_profile_version": 3, "probe_version": "0.4.1", "model": {"name": "m"}}"#,
    )
    .unwrap();
    assert_eq!(p.preferred_patch_codec(), None);
    assert_eq!(p.codec_cell("search_replace"), None);
}

#[test]
fn grid_present_but_small_grade_missing_selects_none() {
    let p = codec_profile(
        r#"{
          "search_replace": {"tiny": {"lands": 0.9, "lands_applies": 0.9, "n": 5}},
          "whole_file": {"tiny": {"lands": 0.9, "lands_applies": 0.9, "n": 5}}
        }"#,
    );
    assert_eq!(p.preferred_patch_codec(), None);
}

#[test]
fn tiny_grade_cell_does_not_influence_selection() {
    // wf wins at tiny, sr wins at small -> selection must follow `small`
    // (VERDICT_GRADE) only, never fall back to tiny.
    let p = codec_profile(
        r#"{
          "search_replace": {
            "tiny": {"lands": 0.2, "lands_applies": 0.3, "n": 5},
            "small": {"lands": 0.8, "lands_applies": 0.9, "n": 5}
          },
          "whole_file": {
            "tiny": {"lands": 0.9, "lands_applies": 0.95, "n": 5},
            "small": {"lands": 0.5, "lands_applies": 0.6, "n": 5}
          }
        }"#,
    );
    assert_eq!(p.preferred_patch_codec(), Some(PatchCodec::SearchReplace));
}

#[test]
fn codec_cell_reads_fields_at_verdict_grade() {
    let p = codec_profile(
        r#"{
          "search_replace": {"small": {"lands": 0.6, "lands_applies": 0.8, "n": 5}}
        }"#,
    );
    let cell = p.codec_cell("search_replace").unwrap();
    assert_eq!(cell.lands, Some(0.6));
    assert_eq!(cell.lands_applies, Some(0.8));
    assert_eq!(cell.n, 5);
}

// --- v8 compatibility + the instrument-changed precheck (spec §3) -------

/// A REAL assay v8 profile, copied **byte-verbatim** from the assay repo at
/// `docs/superpowers/evidence/tier-enthusiast-2026-08/qwen2.5-coder-1.5b-instruct-q8_0.json`
/// (2026-08 campaign, probe 0.9.0). Never hand-edited: the point of this
/// fixture is that the parser is exercised against bytes an instrument
/// actually wrote, not against a hand-built approximation of them.
const V8_FIXTURE: &str =
    include_str!("../../bloomery-daemon/tests/fixtures/profile-v8-qwen15b.json");

/// A REAL assay v4 profile, copied **byte-verbatim** from the assay repo at
/// `docs/superpowers/evidence/tier-enthusiast/qwen3-8b.json` (probe 0.5.0,
/// schema 4) — the old-schema side of the instrument-changed precheck, i.e.
/// exactly the shape of reference profile every model carries across the
/// spec §6 assay pin upgrade.
const V4_FIXTURE: &str =
    include_str!("../../bloomery-daemon/tests/fixtures/profile-v4-qwen3-8b.json");

/// Rewrite the v8 fixture's `probe_version` in memory. Asserts the rewrite
/// actually bit, so a test built on it can never pass vacuously.
fn with_probe_version(doc: &str, version: &str) -> String {
    let out = doc.replace(
        r#""probe_version": "0.9.0""#,
        &format!(r#""probe_version": "{version}""#),
    );
    assert_ne!(out, doc, "probe_version rewrite matched nothing");
    out
}

/// Rewrite the v8 fixture's `assay_profile_version` in memory, same discipline.
fn with_schema_version(doc: &str, version: u32) -> String {
    let out = doc.replace(
        r#""assay_profile_version": 8"#,
        &format!(r#""assay_profile_version": {version}"#),
    );
    assert_ne!(out, doc, "assay_profile_version rewrite matched nothing");
    out
}

/// Drop the `probe_version` key from a pretty-printed profile document.
fn without_probe_version(doc: &str) -> String {
    let out = doc
        .lines()
        .filter(|l| !l.contains(r#""probe_version""#))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        !out.contains("probe_version"),
        "strip helper left the key behind"
    );
    assert!(out.len() < doc.len(), "strip helper changed nothing");
    out
}

#[test]
fn a_real_v8_profile_parses_and_names_its_instrument() {
    let p = Profile::from_json(V8_FIXTURE).unwrap();
    assert_eq!(p.schema_version(), 8);
    assert_eq!(p.probe_version(), "0.9.0");
    assert_eq!(p.model_name(), "qwen2.5-coder:1.5b-instruct-q8_0");
}

#[test]
fn a_real_v4_profile_still_parses_and_names_its_instrument() {
    let p = Profile::from_json(V4_FIXTURE).unwrap();
    assert_eq!(p.schema_version(), 4);
    assert_eq!(p.probe_version(), "0.5.0");
    assert_eq!(p.model_name(), "qwen3:8b");
}

#[test]
fn same_instrument_is_comparable_different_is_named_never_scored() {
    let v8 = Profile::from_json(V8_FIXTURE).unwrap();
    let v8_again = Profile::from_json(V8_FIXTURE).unwrap();
    let v4 = Profile::from_json(V4_FIXTURE).unwrap();

    // Two handles onto the same instrument: comparable.
    assert_eq!(
        instrument_precheck(&v8, &v8_again),
        InstrumentPrecheck::Comparable
    );

    // Old-schema reference vs this boot's v8 profile: named, never scored,
    // and carrying BOTH sides so the journal row needs no re-read.
    assert_eq!(
        instrument_precheck(&v4, &v8),
        InstrumentPrecheck::InstrumentChanged {
            reference: "0.5.0/v4".into(),
            current: "0.9.0/v8".into(),
        }
    );
    // ... and the mirrored direction names the sides the other way round.
    assert_eq!(
        instrument_precheck(&v8, &v4),
        InstrumentPrecheck::InstrumentChanged {
            reference: "0.9.0/v8".into(),
            current: "0.5.0/v4".into(),
        }
    );

    // Same SCHEMA, different probe: a precheck that only looked at schema
    // would call this comparable. Assay's 2026-08 campaign is the reason it
    // must not — the ceiling cap moved between probe versions (spec §3).
    let newer_probe = Profile::from_json(&with_probe_version(V8_FIXTURE, "0.9.1")).unwrap();
    assert_eq!(
        instrument_precheck(&v8, &newer_probe),
        InstrumentPrecheck::InstrumentChanged {
            reference: "0.9.0/v8".into(),
            current: "0.9.1/v8".into(),
        }
    );

    // Same PROBE, different schema: the symmetric guard.
    let older_schema = Profile::from_json(&with_schema_version(V8_FIXTURE, 7)).unwrap();
    assert_eq!(
        instrument_precheck(&v8, &older_schema),
        InstrumentPrecheck::InstrumentChanged {
            reference: "0.9.0/v8".into(),
            current: "0.9.0/v7".into(),
        }
    );
}

#[test]
fn probe_version_is_mandatory_not_optional() {
    let stripped = without_probe_version(V8_FIXTURE);
    let e = Profile::from_json(&stripped);
    match e {
        Err(bloomery_core::profile::ProfileError::Parse(msg)) => {
            assert!(
                msg.contains("probe_version"),
                "parse error must name the missing field, got: {msg}"
            );
        }
        other => panic!(
            "a profile without probe_version must be a parse error, never a \
             default that looks like a version; got {:?}",
            other.map(|p| p.probe_version().to_string())
        ),
    }
}
