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
