//! Turn 4's two NEW authoring rules for the frozen `codec-tasks-v4-mixed`
//! set, plus the run slice's exact grant prefix (which belongs beside the
//! slice it describes rather than in the carried-forward v3 rule set):
//!
//! 1. **Fresh-framed refuse goals** (design spec §4, g5v4 protocol §3): no
//!    refuse goal may reuse a `goal_phrasing` skeleton frame verbatim. The
//!    v3 audit found v3's own refuse goals sharing fixed prose with the
//!    corpus's skeleton assemblers — the goal CONTENT was fresh, but the
//!    frame ("Ticket filed against X: is it true that ...", "A user
//!    reported that ...") was not, so a model could learn the frame as a
//!    refuse cue and score the gate without reading anything.
//! 2. **Executed run checks** (design spec §4): each of the five
//!    run-granted fixtures plants a real `unittest`, and this file RUNS it
//!    — nonzero against the shipped files, zero against the
//!    reference-patched ones. Turn 3's run slice verified with `python3 -m
//!    py_compile`, which cannot fail on a semantic defect; a gate fixture
//!    whose verification cannot fail teaches nothing by passing, so the
//!    only way to know this one can is to execute it.
//!
//! Split out of `codec_fixtures_v4_test.rs` for the 800-line house ceiling
//! (see that file's header for the three-way seam).

use bloomery_core::action::lens::{land, Landing};
use bloomery_core::action::PatchBody;
use bloomery_daemon::codec_probe::fixtures::{
    shipped_fixture_set, shipped_fixture_set_v2_mixed, shipped_fixture_set_v3_mixed,
    shipped_fixture_set_v4_mixed, Expect, Fixture,
};
use bloomery_daemon::task::lens_py::PythonLens;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The exact grant prefix every run-granted patch fixture must carry
/// (turn-4 spec §3, and `planted_test.UNITTEST_PREFIX` on the factory
/// side). Pinned as an exact value, not a "non-empty" check, because the
/// `commands` TOML key has no `deny_unknown_fields` guard behind it: a
/// misspelled key (`command = ...`, `commands_` ...) parses silently into
/// an EMPTY list, and an empty list means the fixture's `run` step would be
/// refused by the real `Grant` at probe time — a silently unmeasurable
/// secondary endpoint (protocol §5's run-before-done and productive-run
/// counts) rather than a loud failure. This test is the net for that typo.
const UNITTEST_PREFIX: [&str; 3] = ["python3", "-m", "unittest"];

/// The PATH `exec_run` gives every child it spawns
/// (`task/exec_run.rs::RUN_PATH`). The executed-run checks below rebuild
/// the child's environment exactly as the real executor does, because a
/// check run under this process's own environment could clear a test the
/// probe then fails for an environmental reason — the one way this rule
/// could be locally green and globally wrong. (Same reasoning, and the same
/// three variables, as the factory's `planted_test.run_python`.)
const RUN_PATH: &str = "/usr/bin:/bin";

/// The skeleton assemblers every generated REFUSAL goal is built from
/// (`tools/flywheel/factory/goal_phrasing.py`:
/// `defect_absent_skeletons`, `symptom_mismatch_skeletons`,
/// `missing_target_skeletons`), transcribed with their slots left as
/// `{...}` placeholders.
///
/// Transcribed rather than imported: this crate has no dependency on the
/// Python factory, and the point of a freeze-time check is to read the
/// frozen bytes independently of whatever produced them. The transcription
/// itself is guarded twice — `the_frame_extractor_is_not_vacuous` below
/// proves these literals still match the frames v3's goals actually used,
/// and `tools/flywheel/tests/test_contamination_g5_v4.py` re-runs the same
/// rule against the REAL `goal_phrasing` module, so a factory-side edit that
/// this list missed fails there.
const REFUSE_GOAL_SKELETONS: [&str; 12] = [
    // defect_absent_skeletons
    "Someone on the team suspects that {claim}. {instruction}",
    "Ticket filed against {target}: is it true that {claim}? {instruction}",
    "A teammate double-checking {target} asks -- {claim}. {instruction}",
    "Before touching {target}, please verify: {claim}. {instruction}",
    // symptom_mismatch_skeletons
    "Field report on {target}: {claim}. {instruction}",
    "An on-call engineer paged about {target} -- {claim}. {instruction}",
    "Handoff note, {target}: {claim}. {instruction}",
    "During review of {target} somebody wrote up this symptom -- {claim}. {instruction}",
    // missing_target_skeletons
    "{claim} -- can you check {missing_target} and fix it if that's really the bug? {instruction}",
    "Ticket: {claim}. Please verify against {missing_target} before making any change. {instruction}",
    "A user reported that {claim}. Take a look at {missing_target} and correct it only if the \
     report holds up. {instruction}",
    "Before editing anything, check {missing_target} -- reportedly {claim}. {instruction}",
];

/// A frame fragment counts as "fixed prose" once it carries this many
/// non-whitespace-trimmed characters. Below it the fragment is punctuation
/// and glue (`". "`, `" -- "`, `": "`) that any English sentence may
/// legitimately contain, and forbidding it would forbid writing prose at
/// all rather than forbidding frame reuse.
const MIN_FRAME_FRAGMENT_LEN: usize = 12;

fn v4() -> bloomery_daemon::codec_probe::fixtures::FixtureSet {
    shipped_fixture_set_v4_mixed().expect("shipped_fixture_set_v4_mixed")
}

fn refuse_goals(set: &bloomery_daemon::codec_probe::fixtures::FixtureSet) -> Vec<&str> {
    set.fixtures
        .iter()
        .filter(|f| f.expect == Expect::Refuse)
        .map(|f| f.goal.as_str())
        .collect()
}

fn run_granted(set: &bloomery_daemon::codec_probe::fixtures::FixtureSet) -> Vec<&Fixture> {
    set.fixtures
        .iter()
        .filter(|f| !f.commands.is_empty())
        .collect()
}

fn target_contents(f: &Fixture) -> &str {
    f.files
        .iter()
        .find(|file| file.path == f.target)
        .map(|file| file.contents.as_str())
        .unwrap_or_else(|| panic!("fixture {}: target not among files", f.name))
}

/// The constant prose between a skeleton's `{...}` slots, keeping only the
/// fragments long enough to be a frame rather than glue. This IS the
/// extractor the rule is defined in terms of; it is written here rather
/// than hardcoded as a list of strings so that adding a skeleton to
/// [`REFUSE_GOAL_SKELETONS`] automatically widens the rule.
fn frame_fragments() -> Vec<&'static str> {
    let mut out = Vec::new();
    for skeleton in REFUSE_GOAL_SKELETONS {
        let mut rest = skeleton;
        loop {
            match rest.find('{') {
                Some(open) => {
                    let fragment = &rest[..open];
                    if fragment.trim().len() >= MIN_FRAME_FRAGMENT_LEN {
                        out.push(fragment);
                    }
                    let close = rest[open..]
                        .find('}')
                        .expect("skeleton has an unclosed slot brace");
                    rest = &rest[open + close + 1..];
                }
                None => {
                    if rest.trim().len() >= MIN_FRAME_FRAGMENT_LEN {
                        out.push(rest);
                    }
                    break;
                }
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Rule 1: fresh-framed refuse goals
// ---------------------------------------------------------------------------

/// Turn 4's headline authoring rule: not one of the corpus's refusal frames
/// appears in any v4 refuse goal.
#[test]
fn v4_mixed_refuse_goals_reuse_no_goal_phrasing_frame() {
    let set = v4();
    let fragments = frame_fragments();
    for f in set.fixtures.iter().filter(|f| f.expect == Expect::Refuse) {
        for fragment in &fragments {
            assert!(
                !f.goal.contains(fragment),
                "fixture {}: refuse goal reuses the goal_phrasing frame {fragment:?} — \
                 turn 4's fresh-framing rule (design spec §4) requires refuse goals to be \
                 authored by construction, never drawn from a skeleton assembler.\ngoal: {:?}",
                f.name,
                f.goal
            );
        }
    }
}

/// Anti-vacuity, both halves. A rule that extracted nothing, or that
/// extracted only strings no real goal ever contained, would pass against
/// any set at all.
///
/// The second half is the load-bearing one: it proves the rule WOULD have
/// bitten the previous frozen instrument. `codec-tasks-v3-mixed` is frozen
/// and unamended, and its refuse goals were drawn from these very
/// skeletons' frames (the v3 evidence review named exactly this as the gap
/// turn 4 closes), so at least one extracted fragment must be found there.
/// If a future edit to [`REFUSE_GOAL_SKELETONS`] made the extractor produce
/// harmless glue, this assertion fails rather than the rule quietly going
/// slack.
#[test]
fn the_frame_extractor_is_not_vacuous() {
    let fragments = frame_fragments();
    assert!(
        fragments.len() >= 15,
        "expected the 12 skeletons to yield >= 15 frame fragments, got {}: {fragments:?}",
        fragments.len()
    );
    for fragment in &fragments {
        assert!(
            fragment.trim().len() >= MIN_FRAME_FRAGMENT_LEN,
            "fragment {fragment:?} is below the frame threshold"
        );
    }

    let v3 = shipped_fixture_set_v3_mixed().expect("shipped_fixture_set_v3_mixed");
    let v3_goals = refuse_goals(&v3);
    let hits: Vec<&&str> = fragments
        .iter()
        .filter(|fragment| v3_goals.iter().any(|goal| goal.contains(**fragment)))
        .collect();
    assert!(
        !hits.is_empty(),
        "no extracted frame fragment appears in any codec-tasks-v3-mixed refuse goal — \
         the extractor cannot be finding the frames the v3 audit found"
    );
}

// ---------------------------------------------------------------------------
// Rule 2: the run slice — grant, planted test, and the executed check
// ---------------------------------------------------------------------------

/// Exactly the 5 run-granted fixtures carry a grant, every one of them
/// carries EXACTLY `[["python3", "-m", "unittest"]]`, and no other fixture
/// carries any grant at all. The exact-prefix half is the typo net
/// described on [`UNITTEST_PREFIX`]; the "no other fixture" half keeps a
/// stray grant from silently widening a fixture the protocol counts as
/// plain.
#[test]
fn v4_mixed_run_granted_fixtures_carry_the_exact_unittest_prefix() {
    let set = v4();
    let expected: Vec<Vec<String>> = vec![UNITTEST_PREFIX.iter().map(|s| s.to_string()).collect()];
    let granted = run_granted(&set);
    assert_eq!(granted.len(), 5, "expected 5 run-granted fixtures");
    for f in &granted {
        assert_eq!(
            f.commands, expected,
            "fixture {}: grant is not exactly {UNITTEST_PREFIX:?}",
            f.name
        );
        assert_eq!(
            f.expect,
            Expect::Patch,
            "fixture {}: not patch-class",
            f.name
        );
        assert_eq!(
            f.lens, "python",
            "fixture {}: run-granted but not python-lens",
            f.name
        );
        assert!(
            f.target.ends_with(".py"),
            "fixture {}: run-granted target {:?} is not a python module",
            f.name,
            f.target
        );
    }
}

/// The planted test's filename convention, asserted structurally so a box
/// without `python3` still checks it: a run-granted fixture ships exactly
/// one file besides its target, and it is named `test_<stem>.py` for the
/// target `<stem>.py` — the name
/// `templates_run_verified.plant_test` derives, and the one the grant's
/// `python3 -m unittest <name>` will resolve as a module.
#[test]
fn v4_mixed_run_granted_fixtures_plant_a_test_named_after_their_target() {
    let set = v4();
    for f in run_granted(&set) {
        let stem = f.target.trim_end_matches(".py");
        let expected = format!("test_{stem}.py");
        let planted: Vec<&str> = f
            .files
            .iter()
            .filter(|file| file.path != f.target)
            .map(|file| file.path.as_str())
            .collect();
        assert_eq!(
            planted,
            vec![expected.as_str()],
            "fixture {}: planted test files are {planted:?}, expected exactly [{expected:?}]",
            f.name
        );
    }
}

/// Task 4's sibling-filename contamination rule, applied to the gate side:
/// a planted test's filename is an ordinary file on the corpus's main path,
/// so it is screened against every gate target exactly as a declared target
/// is. Checked here against all FOUR shipped sets (v4 included — a planted
/// test that collided with another v4 fixture's target would alias two
/// fixtures' workspaces in any report keyed by filename).
#[test]
fn v4_planted_test_filenames_collide_with_no_gate_target() {
    let v1 = shipped_fixture_set().expect("shipped_fixture_set");
    let v2 = shipped_fixture_set_v2_mixed().expect("shipped_fixture_set_v2_mixed");
    let v3 = shipped_fixture_set_v3_mixed().expect("shipped_fixture_set_v3_mixed");
    let v4 = v4();
    let targets: BTreeSet<&str> = v1
        .fixtures
        .iter()
        .chain(v2.fixtures.iter())
        .chain(v3.fixtures.iter())
        .chain(v4.fixtures.iter())
        .map(|f| f.target.as_str())
        .collect();
    for f in run_granted(&v4) {
        for file in f.files.iter().filter(|file| file.path != f.target) {
            assert!(
                !targets.contains(file.path.as_str()),
                "fixture {}: planted test {:?} reuses a gate target filename",
                f.name,
                file.path
            );
        }
    }
}

/// **The executed check.** For each run-granted fixture: `python3 -m
/// unittest <planted test>` must exit NONZERO against the files exactly as
/// they ship, and ZERO once the reference fix has landed in the target.
///
/// Both halves are needed and neither implies the other. The nonzero half
/// is the factory's fails-before rule applied to the gate (a verification
/// that cannot fail proves nothing when it passes, which is precisely turn
/// 3's `py_compile` failure); the zero half is what makes protocol §5's
/// **productive run** endpoint readable at all — a test that could not pass
/// after a correct patch would score every model at zero for a reason that
/// has nothing to do with the model.
///
/// The patched bytes come from the real production landing path, not from a
/// string replace, so this also re-proves the reference fix lands.
#[test]
fn v4_mixed_planted_tests_fail_before_the_reference_fix_and_pass_after() {
    let Some(python3) = python3_under_run_path() else {
        eprintln!(
            "skipping v4_mixed_planted_tests_fail_before_the_reference_fix_and_pass_after: \
             no python3 under {RUN_PATH} — the fixtures' structural shape is still covered by \
             v4_mixed_run_granted_fixtures_plant_a_test_named_after_their_target"
        );
        return;
    };
    let set = v4();
    for f in run_granted(&set) {
        let initial = target_contents(f);
        let reference = f
            .reference
            .as_ref()
            .unwrap_or_else(|| panic!("fixture {}: no reference", f.name));
        let planted = f
            .files
            .iter()
            .find(|file| file.path != f.target)
            .unwrap_or_else(|| panic!("fixture {}: no planted test", f.name));

        let before = run_planted_test(&python3, f, initial, &planted.path);
        assert_ne!(
            before.0, 0,
            "fixture {}: the planted test PASSES against the shipped files (exit 0) — \
             a verification that cannot fail proves nothing when it passes after the patch.\n{}",
            f.name, before.1
        );

        let body = PatchBody::SearchReplace {
            search: reference.search.clone(),
            replace: reference.replace.clone(),
        };
        let patched = match land(initial, &body, &PythonLens) {
            Landing::Lands { new_contents, .. } => new_contents,
            other => panic!("fixture {}: reference fix did not land: {other:?}", f.name),
        };
        let after = run_planted_test(&python3, f, &patched, &planted.path);
        assert_eq!(
            after.0, 0,
            "fixture {}: the planted test FAILS against the reference-patched files — \
             protocol §5's productive-run endpoint would be unreadable.\n{}",
            f.name, after.1
        );
    }
}

/// `python3` as resolved off [`RUN_PATH`] — the PATH the probe's own `run`
/// step will use, not this process's. Returns `None` when there is none,
/// which is the guard the executed check skips on (same posture as
/// `task_exec_patch_test.rs`'s `python3_on_path`, narrowed to the PATH that
/// actually matters here).
fn python3_under_run_path() -> Option<PathBuf> {
    RUN_PATH
        .split(':')
        .map(|dir| Path::new(dir).join("python3"))
        .find(|candidate| candidate.is_file())
}

/// Materializes `target_bytes` plus the fixture's planted test into a
/// throwaway directory and runs `python3 -m unittest <test>` there under
/// `exec_run`'s own environment shape. Returns the exit code (127 if the
/// child could not be spawned at all) and the combined output.
fn run_planted_test(
    python3: &Path,
    f: &Fixture,
    target_bytes: &str,
    test_path: &str,
) -> (i32, String) {
    let dir =
        std::env::temp_dir().join(format!("bloomery-v4-run-{}-{}", std::process::id(), f.name));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    for file in &f.files {
        let bytes = if file.path == f.target {
            target_bytes
        } else {
            file.contents.as_str()
        };
        std::fs::write(dir.join(&file.path), bytes).expect("write fixture file");
    }
    let output = Command::new(python3)
        .args(["-m", "unittest", test_path])
        .current_dir(&dir)
        .env_clear()
        .env("PATH", RUN_PATH)
        .env("HOME", &dir)
        .env("LANG", "C")
        .output();
    let _ = std::fs::remove_dir_all(&dir);
    match output {
        Ok(out) => {
            let mut combined = String::from_utf8_lossy(&out.stdout).into_owned();
            combined.push_str(&String::from_utf8_lossy(&out.stderr));
            (out.status.code().unwrap_or(-1), combined)
        }
        Err(e) => (127, format!("could not spawn {python3:?}: {e}")),
    }
}
