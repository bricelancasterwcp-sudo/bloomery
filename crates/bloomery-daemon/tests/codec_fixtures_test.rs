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

use bloomery_core::action::lens::{land, Landing, PlainText};
use bloomery_core::action::PatchBody;
use bloomery_daemon::codec_probe::fixtures::{shipped_fixture_set, Fixture};
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
        assert_ne!(
            f.reference.search, f.reference.replace,
            "fixture {}: search == replace, not a repair",
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
        let body = PatchBody::SearchReplace {
            search: f.reference.search.clone(),
            replace: f.reference.replace.clone(),
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
