//! The daemon side of spec §6's assay pin upgrade: the profile documents
//! POST will start receiving once the pin moves to assay 0.9.0 (schema v8)
//! must be readable by every accessor the daemon already depends on, and an
//! old-schema reference beside a new one must read as `instrument-changed`
//! (spec §3) rather than as a score.
//!
//! Both fixtures are REAL assay artifacts, copied **byte-verbatim** (sha256
//! checked against source) out of the assay repo and never hand-edited:
//!
//! - `fixtures/profile-v8-qwen15b.json` <-
//!   `docs/superpowers/evidence/tier-enthusiast-2026-08/qwen2.5-coder-1.5b-instruct-q8_0.json`
//!   (2026-08 campaign, probe 0.9.0 / schema 8)
//! - `fixtures/profile-v4-qwen3-8b.json` <-
//!   `docs/superpowers/evidence/tier-enthusiast/qwen3-8b.json`
//!   (probe 0.5.0 / schema 4)

use bloomery_core::action::PatchCodec;
use bloomery_core::profile::{instrument_precheck, InstrumentPrecheck, Profile, Verdict};

const V8_FIXTURE: &str = include_str!("fixtures/profile-v8-qwen15b.json");
const V4_FIXTURE: &str = include_str!("fixtures/profile-v4-qwen3-8b.json");

/// Every accessor the daemon reads off a POST profile, exercised against the
/// bytes assay 0.9.0 actually wrote.
#[test]
fn a_real_v8_profile_serves_every_daemon_accessor() {
    let p = Profile::from_json(V8_FIXTURE).expect("real v8 profile parses");

    assert_eq!(p.schema_version(), 8);
    assert_eq!(p.probe_version(), "0.9.0");
    assert_eq!(p.model_name(), "qwen2.5-coder:1.5b-instruct-q8_0");
    assert_eq!(p.measured_ceiling(), Some(32768));

    // Law 5's admission reads verdicts by name; v8 adds sibling keys
    // (`interval95`, `lens`, ...) that must not disturb the read.
    assert_eq!(p.verdict("structured_extraction"), Verdict::Ready);
    assert_eq!(p.verdict("loop_discipline"), Verdict::Risky);
    assert_eq!(p.verdict("tool_calling"), Verdict::Unusable);
    assert_eq!(p.verdict("not_a_capability"), Verdict::Unmeasured);

    // Protocol §4 codec selection off the real grid: whole_file's
    // lands_applies (1.0) strictly beats search_replace's (0.571) at `small`.
    let cell = p.codec_cell("whole_file").expect("v8 grid has whole_file");
    assert_eq!(cell.n, 35);
    assert_eq!(p.preferred_patch_codec(), Some(PatchCodec::WholeFile));
}

/// The first boot after the pin upgrade: last boot's v4 profile is the
/// reference, this boot's v8 profile is current. Never pass, never fail —
/// named, with both instruments in the row.
#[test]
fn the_pin_upgrade_boot_reads_instrument_changed() {
    let reference = Profile::from_json(V4_FIXTURE).expect("real v4 profile parses");
    let current = Profile::from_json(V8_FIXTURE).expect("real v8 profile parses");

    assert_eq!(
        instrument_precheck(&reference, &current),
        InstrumentPrecheck::InstrumentChanged {
            reference: "0.5.0/v4".into(),
            current: "0.9.0/v8".into(),
        }
    );
}
