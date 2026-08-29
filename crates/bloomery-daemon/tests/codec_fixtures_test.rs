//! Structural validation for the frozen G4 fixture set `codec-tasks-v1`
//! (Phase 2b/2c P4 Task 5).
//!
//! This is the "seconds-fast authored-artifact check" `rigorous-experiments`
//! requires for a pre-registered measurement instrument
//! (`docs/superpowers/evidence/2026-08-15-g4-protocol.md` §2): it does not
//! measure any model, it proves the fixture set itself is internally
//! consistent and that every fixture's reference fix actually **lands**
//! through the real production landing path before the set is ever used to
//! score anything.
//!
//! Every landing check below goes through the real instrument:
//! `bloomery_core::action::lens::land` with `PatchBody::SearchReplace` and
//! either the real `PlainText` lens or the daemon's real `PythonLens`
//! (`bloomery_daemon::task::lens_py::PythonLens`, which shells out to
//! `python3` on `PATH`) — never a reimplementation of apply/parse
//! semantics in this test. `python3` is required to be on `PATH` for these
//! assertions to be meaningful; if it is absent, `PythonLens::parses` fails
//! closed (see that module's docs) and every python fixture's landing
//! assertion below fails loudly rather than silently skipping, because an
//! authored-artifact check that can silently no-op on a missing dependency
//! is not a check.

use bloomery_core::action::lens::{land, Landing, LandingLens, PlainText};
use bloomery_core::action::PatchBody;
use bloomery_daemon::codec_probe::fixtures::{
    shipped_fixture_set, shipped_fixture_set_v2_mixed, shipped_fixture_set_v3_mixed,
    shipped_fixture_set_v4_mixed, shipped_fixture_set_v5_mixed, Expect, Fixture,
};
use bloomery_daemon::task::lens_py::PythonLens;
use std::collections::BTreeSet;

#[test]
fn shipped_fixture_set_parses() {
    let result = shipped_fixture_set();
    assert!(result.is_ok(), "shipped_fixture_set() failed: {result:?}");
}

#[test]
fn set_name_is_codec_tasks_v1() {
    let set = shipped_fixture_set().expect("shipped_fixture_set");
    assert_eq!(set.set, "codec-tasks-v1");
}

#[test]
fn exactly_twenty_fixtures() {
    let set = shipped_fixture_set().expect("shipped_fixture_set");
    assert_eq!(set.fixtures.len(), 20, "expected N=20 fixtures");
}

#[test]
fn ten_python_and_ten_plaintext() {
    let set = shipped_fixture_set().expect("shipped_fixture_set");
    let python = set.fixtures.iter().filter(|f| f.lens == "python").count();
    let plaintext = set
        .fixtures
        .iter()
        .filter(|f| f.lens == "plaintext")
        .count();
    assert_eq!(python, 10, "expected 10 python fixtures, got {python}");
    assert_eq!(
        plaintext, 10,
        "expected 10 plaintext fixtures, got {plaintext}"
    );
}

#[test]
fn fixture_names_are_unique() {
    let set = shipped_fixture_set().expect("shipped_fixture_set");
    let mut seen = BTreeSet::new();
    for f in &set.fixtures {
        assert!(
            seen.insert(f.name.clone()),
            "duplicate fixture name: {}",
            f.name
        );
    }
}

#[test]
fn every_lens_is_python_or_plaintext() {
    let set = shipped_fixture_set().expect("shipped_fixture_set");
    for f in &set.fixtures {
        assert!(
            f.lens == "python" || f.lens == "plaintext",
            "fixture {} has unknown lens {:?}",
            f.name,
            f.lens
        );
    }
}

#[test]
fn every_target_appears_among_its_fixtures_files() {
    let set = shipped_fixture_set().expect("shipped_fixture_set");
    for f in &set.fixtures {
        assert!(
            f.files.iter().any(|file| file.path == f.target),
            "fixture {}: target {:?} not among files {:?}",
            f.name,
            f.target,
            f.files.iter().map(|file| &file.path).collect::<Vec<_>>()
        );
    }
}

#[test]
fn every_goal_is_non_empty_and_names_its_target() {
    let set = shipped_fixture_set().expect("shipped_fixture_set");
    for f in &set.fixtures {
        assert!(!f.goal.trim().is_empty(), "fixture {}: empty goal", f.name);
        assert!(
            f.goal.contains(&f.target),
            "fixture {}: goal does not mention target {:?}: {:?}",
            f.name,
            f.target,
            f.goal
        );
    }
}

#[test]
fn search_never_equals_replace() {
    let set = shipped_fixture_set().expect("shipped_fixture_set");
    for f in &set.fixtures {
        let reference = f
            .reference
            .as_ref()
            .unwrap_or_else(|| panic!("fixture {}: expect=\"patch\" but no reference", f.name));
        assert_ne!(
            reference.search, reference.replace,
            "fixture {}: search == replace, not a repair",
            f.name
        );
    }
}

/// G5 design doc §2: `expect` is absent from every v1 fixture, so it must
/// default to `Patch` — the schema-level compat pin, restated at the
/// shipped-set level.
#[test]
fn every_v1_fixture_defaults_to_patch_expect() {
    use bloomery_daemon::codec_probe::fixtures::Expect;
    let set = shipped_fixture_set().expect("shipped_fixture_set");
    for f in &set.fixtures {
        assert_eq!(
            f.expect,
            Expect::Patch,
            "fixture {}: not patch-class",
            f.name
        );
        assert!(
            f.reference.is_some(),
            "fixture {}: missing reference",
            f.name
        );
        assert!(
            f.refusal_reason.is_none(),
            "fixture {}: a v1 fixture must not carry a refusal_reason",
            f.name
        );
    }
}

/// The load-bearing check: every fixture's reference fix must LAND through
/// the real production instrument (`bloomery_core::action::lens::land`),
/// using the real lens the fixture declares. A fixture whose verified fix
/// doesn't land can never be scored honestly by the gate that consumes this
/// set (G4 protocol §2/§3).
#[test]
fn every_reference_fix_lands_through_the_real_instrument() {
    let set = shipped_fixture_set().expect("shipped_fixture_set");
    for f in &set.fixtures {
        let initial = target_contents(f);
        let reference = f
            .reference
            .as_ref()
            .unwrap_or_else(|| panic!("fixture {}: expect=\"patch\" but no reference", f.name));
        let body = PatchBody::SearchReplace {
            search: reference.search.clone(),
            replace: reference.replace.clone(),
        };
        let landing = match f.lens.as_str() {
            "python" => land(initial, &body, &PythonLens),
            "plaintext" => land(initial, &body, &PlainText),
            other => panic!("fixture {}: unhandled lens {other:?}", f.name),
        };
        match landing {
            Landing::Lands { new_contents, .. } => {
                assert_ne!(
                    new_contents, initial,
                    "fixture {}: reference fix landed but did not change bytes",
                    f.name
                );
            }
            other => panic!(
                "fixture {} ({}): reference fix did not land: {other:?}",
                f.name, f.lens
            ),
        }
    }
}

/// Finds the fixture's declared `target` among its `files` and returns its
/// contents. Panics (not `Option`) on a missing target — that shape is
/// already covered by `every_target_appears_among_its_fixtures_files`, so a
/// panic here means that check would already have failed first.
fn target_contents(f: &Fixture) -> &str {
    &f.files
        .iter()
        .find(|file| file.path == f.target)
        .unwrap_or_else(|| panic!("fixture {}: target {:?} not among files", f.name, f.target))
        .contents
}

// ---------------------------------------------------------------------------
// codec-tasks-v2-mixed (G5 design doc §3, task-4 brief): the FROZEN 20-
// fixture mixed set — 10 expect="patch" + 10 expect="refuse", both lenses in
// both classes. Same "authored-artifact check, GPU-free, seconds-fast"
// posture as the v1 tests above: every patch reference fix lands through the
// real instrument; every refuse fixture is byte-valid for its class
// (defect-absent: target present and genuinely parses cleanly;
// missing-target: target genuinely absent with >= 1 real sibling); the
// defect-absent goal-plausibility rule (G5 design doc §5) is re-checked
// against the frozen, committed bytes — independent proof, not a re-run of
// the Python factory's own `validate_refusal_task` check.
// ---------------------------------------------------------------------------

/// The fixture's declared `target`'s contents, if `target` is among
/// `files` — `None` for a missing-target refuse fixture, where that
/// absence is the whole point (unlike `target_contents` above, this
/// never panics on absence, since absence is a valid, expected shape
/// here).
fn v2_mixed_target_contents(f: &Fixture) -> Option<&str> {
    f.files
        .iter()
        .find(|file| file.path == f.target)
        .map(|file| file.contents.as_str())
}

/// Every substring of `text` enclosed in a matched pair of backticks, in
/// order — the same convention the Python factory's
/// `task.REFUSAL_QUOTED_RE` uses for the plausibility rule's mechanical
/// marker, restated here (not imported — this crate has no dependency on
/// the Python factory) so the frozen TOML's bytes can be checked
/// independently of whatever produced them.
fn extract_backtick_quoted(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find('`') {
        let after = &rest[start + 1..];
        match after.find('`') {
            Some(end) => {
                out.push(&after[..end]);
                rest = &after[end + 1..];
            }
            None => break,
        }
    }
    out
}

#[test]
fn shipped_v2_mixed_parses() {
    let result = shipped_fixture_set_v2_mixed();
    assert!(
        result.is_ok(),
        "shipped_fixture_set_v2_mixed() failed: {result:?}"
    );
}

#[test]
fn v2_mixed_set_name_is_exact() {
    let set = shipped_fixture_set_v2_mixed().expect("shipped_fixture_set_v2_mixed");
    assert_eq!(set.set, "codec-tasks-v2-mixed");
}

#[test]
fn v2_mixed_has_twenty_fixtures_ten_patch_ten_refuse() {
    let set = shipped_fixture_set_v2_mixed().expect("shipped_fixture_set_v2_mixed");
    assert_eq!(set.fixtures.len(), 20, "expected N=20 fixtures");
    let patch = set
        .fixtures
        .iter()
        .filter(|f| f.expect == Expect::Patch)
        .count();
    let refuse = set
        .fixtures
        .iter()
        .filter(|f| f.expect == Expect::Refuse)
        .count();
    assert_eq!(patch, 10, "expected 10 patch fixtures, got {patch}");
    assert_eq!(refuse, 10, "expected 10 refuse fixtures, got {refuse}");
}

/// Both lenses (python, plaintext) represented in BOTH classes (patch and
/// refuse) — G5 design doc §3's explicit shape requirement.
#[test]
fn v2_mixed_both_lenses_represented_in_both_classes() {
    let set = shipped_fixture_set_v2_mixed().expect("shipped_fixture_set_v2_mixed");
    for expect in [Expect::Patch, Expect::Refuse] {
        let python = set
            .fixtures
            .iter()
            .filter(|f| f.expect == expect && f.lens == "python")
            .count();
        let plaintext = set
            .fixtures
            .iter()
            .filter(|f| f.expect == expect && f.lens == "plaintext")
            .count();
        assert!(python > 0, "{expect:?}: no python fixtures");
        assert!(plaintext > 0, "{expect:?}: no plaintext fixtures");
    }
}

/// At least 4 of the 10 refuse fixtures from EACH family (task-4 brief). A
/// fixture's family is not a field in this schema (that concept lives on
/// the Python factory's `RefusalTask.family` only) — it is structurally
/// implied here exactly as the parser relaxation's own doc comment states:
/// a refuse fixture whose target IS among its files is defect-absent; one
/// whose target is absent is missing-target.
#[test]
fn v2_mixed_at_least_four_refuse_fixtures_per_family() {
    let set = shipped_fixture_set_v2_mixed().expect("shipped_fixture_set_v2_mixed");
    let refuse: Vec<&Fixture> = set
        .fixtures
        .iter()
        .filter(|f| f.expect == Expect::Refuse)
        .collect();
    let defect_absent = refuse
        .iter()
        .filter(|f| v2_mixed_target_contents(f).is_some())
        .count();
    let missing_target = refuse.len() - defect_absent;
    assert!(
        defect_absent >= 4,
        "expected >= 4 defect-absent refuse fixtures, got {defect_absent}"
    );
    assert!(
        missing_target >= 4,
        "expected >= 4 missing-target refuse fixtures, got {missing_target}"
    );
}

#[test]
fn v2_mixed_fixture_names_are_unique() {
    let set = shipped_fixture_set_v2_mixed().expect("shipped_fixture_set_v2_mixed");
    let mut seen = BTreeSet::new();
    for f in &set.fixtures {
        assert!(
            seen.insert(f.name.clone()),
            "duplicate fixture name: {}",
            f.name
        );
    }
}

/// Names unique across ALL shipped sets — five as of turn 6 (task-4
/// brief, widened by each later freeze: turn-3's `codec-tasks-v3-mixed`,
/// turn-4's `codec-tasks-v4-mixed`, turn-6's `codec-tasks-v5-mixed`):
/// nothing downstream may alias a fixture from one gate set with one from
/// another by name. This is the one per-set assertion that lives here
/// rather than in each set's own suite — it is the only one that has to
/// see every shipped set at once.
#[test]
fn fixture_names_are_unique_across_all_shipped_sets() {
    let v1 = shipped_fixture_set().expect("shipped_fixture_set");
    let v2 = shipped_fixture_set_v2_mixed().expect("shipped_fixture_set_v2_mixed");
    let v3 = shipped_fixture_set_v3_mixed().expect("shipped_fixture_set_v3_mixed");
    let v4 = shipped_fixture_set_v4_mixed().expect("shipped_fixture_set_v4_mixed");
    let v5 = shipped_fixture_set_v5_mixed().expect("shipped_fixture_set_v5_mixed");
    let mut seen = BTreeSet::new();
    for f in v1
        .fixtures
        .iter()
        .chain(v2.fixtures.iter())
        .chain(v3.fixtures.iter())
        .chain(v4.fixtures.iter())
        .chain(v5.fixtures.iter())
    {
        assert!(
            seen.insert(f.name.clone()),
            "duplicate fixture name across the shipped gate sets: {}",
            f.name
        );
    }
    assert_eq!(
        seen.len(),
        v1.fixtures.len()
            + v2.fixtures.len()
            + v3.fixtures.len()
            + v4.fixtures.len()
            + v5.fixtures.len()
    );
}

/// The load-bearing patch-class check, mirroring v1's own: every
/// `expect = "patch"` reference fix LANDS through the real production
/// instrument. Refuse-class fixtures carry no `reference` at all and are
/// checked separately below.
#[test]
fn v2_mixed_patch_references_land_through_the_real_lenses() {
    let set = shipped_fixture_set_v2_mixed().expect("shipped_fixture_set_v2_mixed");
    for f in set.fixtures.iter().filter(|f| f.expect == Expect::Patch) {
        let initial = v2_mixed_target_contents(f)
            .unwrap_or_else(|| panic!("fixture {}: target not among files", f.name));
        let reference = f
            .reference
            .as_ref()
            .unwrap_or_else(|| panic!("fixture {}: expect=\"patch\" but no reference", f.name));
        let body = PatchBody::SearchReplace {
            search: reference.search.clone(),
            replace: reference.replace.clone(),
        };
        let landing = match f.lens.as_str() {
            "python" => land(initial, &body, &PythonLens),
            "plaintext" => land(initial, &body, &PlainText),
            other => panic!("fixture {}: unhandled lens {other:?}", f.name),
        };
        match landing {
            Landing::Lands { new_contents, .. } => {
                assert_ne!(
                    new_contents, initial,
                    "fixture {}: reference fix landed but did not change bytes",
                    f.name
                );
            }
            other => panic!(
                "fixture {} ({}): reference fix did not land: {other:?}",
                f.name, f.lens
            ),
        }
    }
}

/// Refuse-class fixtures are byte-valid, per family:
/// - defect-absent (target present among files): the target is a
///   genuinely well-formed file for its lens (parses cleanly through the
///   REAL lens's `LandingLens::parses` — the mechanical proxy for
///   "correct", since a refuse fixture carries no reference fix to land).
/// - missing-target (target absent): genuinely absent from `files`, with
///   >= 1 real sibling file present (never a suspiciously-empty dir).
#[test]
fn v2_mixed_refuse_fixtures_are_byte_valid() {
    let set = shipped_fixture_set_v2_mixed().expect("shipped_fixture_set_v2_mixed");
    for f in set.fixtures.iter().filter(|f| f.expect == Expect::Refuse) {
        match v2_mixed_target_contents(f) {
            Some(contents) => {
                let parse_result = match f.lens.as_str() {
                    "python" => PythonLens.parses(contents),
                    "plaintext" => PlainText.parses(contents),
                    other => panic!("fixture {}: unhandled lens {other:?}", f.name),
                };
                assert!(
                    parse_result.is_ok(),
                    "fixture {} (defect-absent): target does not parse cleanly: {parse_result:?}",
                    f.name
                );
            }
            None => {
                assert!(
                    !f.files.is_empty(),
                    "fixture {} (missing-target): no real sibling file present",
                    f.name
                );
            }
        }
    }
}

/// The plausibility rule (G5 design doc §5), re-checked against the
/// FROZEN, committed bytes — independent of the Python factory that
/// produced them: a defect-absent goal must backtick-quote at least one
/// identifier/value that is a literal substring of the target's real
/// contents. Applies only to defect-absent fixtures (target present); a
/// missing-target fixture's target has no contents to quote from.
#[test]
fn v2_mixed_defect_absent_goals_quote_a_real_identifier_present_in_the_target() {
    let set = shipped_fixture_set_v2_mixed().expect("shipped_fixture_set_v2_mixed");
    for f in set.fixtures.iter().filter(|f| f.expect == Expect::Refuse) {
        let Some(contents) = v2_mixed_target_contents(f) else {
            continue; // missing-target: the plausibility rule doesn't apply
        };
        let quoted = extract_backtick_quoted(&f.goal);
        assert!(
            !quoted.is_empty(),
            "fixture {}: goal has no backtick-quoted identifier/value: {:?}",
            f.name,
            f.goal
        );
        assert!(
            quoted.iter().any(|q| contents.contains(q)),
            "fixture {}: none of {quoted:?} appear in the target's real contents",
            f.name
        );
    }
}

/// The G5 design doc §2 mirror of v1's own `every_v1_fixture_defaults_to_patch_expect`:
/// every refuse fixture carries a `refusal_reason` and no `reference`;
/// every patch fixture carries a `reference` and no `refusal_reason` —
/// the parser already enforces this (`check_expect_fields`), this test
/// pins it at the shipped-set level too.
#[test]
fn v2_mixed_expect_fields_are_class_consistent() {
    let set = shipped_fixture_set_v2_mixed().expect("shipped_fixture_set_v2_mixed");
    for f in &set.fixtures {
        match f.expect {
            Expect::Patch => {
                assert!(
                    f.reference.is_some(),
                    "fixture {}: missing reference",
                    f.name
                );
                assert!(
                    f.refusal_reason.is_none(),
                    "fixture {}: a patch fixture must not carry a refusal_reason",
                    f.name
                );
            }
            Expect::Refuse => {
                assert!(
                    f.refusal_reason.is_some(),
                    "fixture {}: missing refusal_reason",
                    f.name
                );
                assert!(
                    f.reference.is_none(),
                    "fixture {}: a refuse fixture must not carry a reference",
                    f.name
                );
            }
        }
    }
}
