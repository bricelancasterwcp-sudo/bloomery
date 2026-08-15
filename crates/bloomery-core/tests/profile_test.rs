use bloomery_core::action::PatchCodec;
use bloomery_core::profile::{Profile, Verdict};

const FIXTURE: &str = r#"{
  "assay_profile_version": 3,
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
    let p = Profile::from_json(r#"{"assay_profile_version": 3, "model": {"name": "m"}}"#).unwrap();
    assert_eq!(p.measured_ceiling(), None);
}

#[test]
fn old_schema_rejected_by_name() {
    let e = Profile::from_json(r#"{"assay_profile_version": 1, "model": {"name": "m"}}"#);
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
    let p = Profile::from_json(r#"{"assay_profile_version": 3, "model": {"name": "m"}}"#).unwrap();
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
