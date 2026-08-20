//! Structural validation for the frozen G5-v3 fixture set
//! `codec-tasks-v3-mixed` (flywheel turn-3 design doc §3; the pinned
//! composition and the pass floor live in
//! `docs/superpowers/evidence/2026-08-20-g5v3-protocol.md` §3/§4).
//!
//! Same posture as `codec_fixtures_test.rs`'s v1 and v2 suites, which this
//! file mirrors: a GPU-free, seconds-fast **authored-artifact check** that
//! measures no model at all — it proves the frozen instrument is internally
//! consistent, and that every patch fixture's reference fix actually
//! **lands** through the real production landing path
//! (`bloomery_core::action::lens::land` with the real `PlainText` /
//! `PythonLens`), before the set is ever used to score anything.
//!
//! **Why three files rather than one.** `codec_fixtures_test.rs` is at 513
//! lines and this suite is ~770; together they would be well past the
//! 800-line house ceiling, so v3 got its own file. The diversity rule then
//! moved once more, into `codec_fixtures_v3_diversity_test.rs`, to keep this
//! one under the ceiling too — it is a self-contained concept with its own
//! normalizer. The one v3 assertion that genuinely belongs in
//! `codec_fixtures_test.rs` stayed there: cross-set name uniqueness, which
//! has to see all three shipped sets at once
//! (`fixture_names_are_unique_across_all_three_shipped_sets`).
//!
//! `python3` must be on `PATH` for the python-lens assertions to mean
//! anything; `PythonLens::parses` fails closed if it is absent, so those
//! assertions fail loudly rather than silently skipping (see
//! `codec_fixtures_test.rs`'s header for the full reasoning).

use bloomery_core::action::lens::{land, Landing, LandingLens, PlainText};
use bloomery_core::action::PatchBody;
use bloomery_daemon::codec_probe::fixtures::{
    shipped_fixture_set_v3_mixed, Expect, Fixture, V3_MIXED_PLACEHOLDER_SET_NAME,
};
use bloomery_daemon::task::lens_py::PythonLens;
use std::collections::{BTreeMap, BTreeSet};

/// The exact grant prefix every run-granted patch fixture must carry
/// (task-8 brief, and `templates_python::PY_COMPILE_PREFIX` on the factory
/// side). Pinned as an exact value, not a "non-empty" check, because the
/// `commands` TOML key has no `deny_unknown_fields` guard behind it: a
/// misspelled key (`command = ...`, `commands_` ...) parses silently into
/// an EMPTY list, and an empty list means the fixture's `run` step would be
/// refused by the real `Grant` at probe time — a silently unmeasurable
/// secondary endpoint (protocol §5's run-before-done count) rather than a
/// loud failure. This test is the net for that typo.
const PY_COMPILE_PREFIX: [&str; 3] = ["python3", "-m", "py_compile"];

/// The two canonical trailing instructions, one per class. Identical to the
/// factory's `task.DONE_INSTRUCTION` / `task.CHECK_INSTRUCTION` — deliberately
/// so: the trailing instruction is the protocol contract shared by the
/// training corpus and the gate, and the refuse class's is the ONLY part of
/// a refusal goal that tells the model to look before it leaps. Everything
/// *before* it is authored differently from the corpus on purpose (the gate
/// is the net for surface-cue learning); this sentence is not.
const PATCH_INSTRUCTION: &str = "Patch the file, then emit done.";
const CHECK_INSTRUCTION: &str =
    "Check first, and only patch if it is genuinely wrong; then emit done.";

/// The three patch-class shapes the protocol pins counts for (§3: 6
/// multi-file find-shaped + 5 run-granted single-file + 5 plain
/// single-target).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum PatchShape {
    Find,
    Run,
    Plain,
}

/// The three refuse-class families the protocol pins counts for (§3: 6
/// defect-absent + 5 missing-target + 5 symptom-mismatch).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum RefuseFamily {
    DefectAbsent,
    MissingTarget,
    SymptomMismatch,
}

fn v3() -> bloomery_daemon::codec_probe::fixtures::FixtureSet {
    shipped_fixture_set_v3_mixed().expect("shipped_fixture_set_v3_mixed")
}

/// The fixture's declared `target`'s contents, if `target` is among
/// `files` — `None` for a missing-target refuse fixture, where that
/// absence is the whole point. (Same helper `codec_fixtures_test.rs`
/// declares for v2; re-declared rather than shared because each
/// `tests/*.rs` file is its own crate, and a `tests/common/` module for
/// two 6-line helpers is not worth the indirection.)
fn target_contents(f: &Fixture) -> Option<&str> {
    f.files
        .iter()
        .find(|file| file.path == f.target)
        .map(|file| file.contents.as_str())
}

/// Every substring enclosed in a matched pair of backticks, in order — the
/// factory's `task.REFUSAL_QUOTED_RE` convention, restated here (this crate
/// has no dependency on the Python factory) so the frozen TOML's bytes are
/// checked independently of whatever produced them.
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

/// A patch fixture's shape, derived from two INDEPENDENT properties so the
/// per-shape file-count assertions below are not circular:
/// - a non-empty `commands` grant means run-granted (nothing else carries
///   one);
/// - among the rest, a goal that does NOT name its own target file is
///   find-shaped — that inversion IS the find shape (the factory's
///   `_find_shape_violations`: "a find-shaped task's goal must name the
///   SYMPTOM, never the file, or there is nothing to find");
/// - everything else is the plain single-target shape.
fn patch_shape(f: &Fixture) -> PatchShape {
    if !f.commands.is_empty() {
        PatchShape::Run
    } else if !f.goal.contains(&f.target) {
        PatchShape::Find
    } else {
        PatchShape::Plain
    }
}

/// A refuse fixture's family. `family` is not a field in this schema (that
/// concept lives on the factory's `RefusalTask.family`), so it is derived
/// from two structural facts, exactly as the two halves are authored:
/// - target absent from `files` → missing-target (the parser's own doc
///   comment states this equivalence);
/// - otherwise the `refusal_reason`'s ruled opening: symptom-mismatch's
///   two-part "Checked: … Found instead: …" assembler
///   (`task.symptom_mismatch_reason`) versus defect-absent's plain "No
///   change needed: …".
///
/// The reason prefixes are asserted as an exact contract by
/// `v3_mixed_refusal_reasons_follow_their_family_shape` below, so this
/// classifier cannot quietly mislabel a fixture whose text drifted.
fn refuse_family(f: &Fixture) -> RefuseFamily {
    if target_contents(f).is_none() {
        RefuseFamily::MissingTarget
    } else if refusal_reason(f).starts_with("Checked: ") {
        RefuseFamily::SymptomMismatch
    } else {
        RefuseFamily::DefectAbsent
    }
}

fn refusal_reason(f: &Fixture) -> &str {
    f.refusal_reason.as_deref().unwrap_or_else(|| {
        panic!(
            "fixture {}: expect=\"refuse\" but no refusal_reason",
            f.name
        )
    })
}

fn patch_fixtures(set: &bloomery_daemon::codec_probe::fixtures::FixtureSet) -> Vec<&Fixture> {
    set.fixtures
        .iter()
        .filter(|f| f.expect == Expect::Patch)
        .collect()
}

fn refuse_fixtures(set: &bloomery_daemon::codec_probe::fixtures::FixtureSet) -> Vec<&Fixture> {
    set.fixtures
        .iter()
        .filter(|f| f.expect == Expect::Refuse)
        .collect()
}

// ---------------------------------------------------------------------------
// Set identity and composition (protocol §3, pinned)
// ---------------------------------------------------------------------------

#[test]
fn shipped_v3_mixed_parses() {
    let result = shipped_fixture_set_v3_mixed();
    assert!(
        result.is_ok(),
        "shipped_fixture_set_v3_mixed() failed: {result:?}"
    );
}

/// The frozen instrument is named `codec-tasks-v3-mixed` — no
/// `-PLACEHOLDER` suffix, which is exactly what `boot::run_boot_g5_probe`
/// checks before it will take a measurement at all.
#[test]
fn v3_mixed_set_name_is_exact_and_not_the_placeholder() {
    let set = v3();
    assert_eq!(set.set, "codec-tasks-v3-mixed");
    assert_ne!(
        set.set, V3_MIXED_PLACEHOLDER_SET_NAME,
        "the placeholder guard must never trigger on the frozen set"
    );
}

#[test]
fn v3_mixed_has_thirty_two_fixtures_sixteen_patch_sixteen_refuse() {
    let set = v3();
    assert_eq!(set.fixtures.len(), 32, "expected N=32 fixtures");
    assert_eq!(patch_fixtures(&set).len(), 16, "expected 16 patch fixtures");
    assert_eq!(
        refuse_fixtures(&set).len(),
        16,
        "expected 16 refuse fixtures"
    );
}

/// Both lenses (python, plaintext) represented in BOTH classes — the shape
/// requirement carried unchanged from the v2 set.
#[test]
fn v3_mixed_both_lenses_represented_in_both_classes() {
    let set = v3();
    for expect in [Expect::Patch, Expect::Refuse] {
        for lens in ["python", "plaintext"] {
            let count = set
                .fixtures
                .iter()
                .filter(|f| f.expect == expect && f.lens == lens)
                .count();
            assert!(count > 0, "{expect:?}: no {lens} fixtures");
        }
    }
}

#[test]
fn v3_mixed_every_lens_is_python_or_plaintext() {
    for f in &v3().fixtures {
        assert!(
            f.lens == "python" || f.lens == "plaintext",
            "fixture {} has unknown lens {:?}",
            f.name,
            f.lens
        );
    }
}

/// Protocol §3's pinned patch composition: 6 multi-file find-shaped, 5
/// run-granted single-file, 5 plain single-target. These are the
/// denominators §5's secondary endpoints report against, so they are exact
/// counts, never floors.
#[test]
fn v3_mixed_patch_shape_counts_are_exact() {
    let set = v3();
    let mut counts: BTreeMap<PatchShape, usize> = BTreeMap::new();
    for f in patch_fixtures(&set) {
        *counts.entry(patch_shape(f)).or_default() += 1;
    }
    assert_eq!(
        counts.get(&PatchShape::Find).copied(),
        Some(6),
        "{counts:?}"
    );
    assert_eq!(counts.get(&PatchShape::Run).copied(), Some(5), "{counts:?}");
    assert_eq!(
        counts.get(&PatchShape::Plain).copied(),
        Some(5),
        "{counts:?}"
    );
}

/// Protocol §3's pinned refuse composition: 6 defect-absent, 5
/// missing-target, 5 symptom-mismatch.
#[test]
fn v3_mixed_refuse_family_counts_are_exact() {
    let set = v3();
    let mut counts: BTreeMap<RefuseFamily, usize> = BTreeMap::new();
    for f in refuse_fixtures(&set) {
        *counts.entry(refuse_family(f)).or_default() += 1;
    }
    assert_eq!(
        counts.get(&RefuseFamily::DefectAbsent).copied(),
        Some(6),
        "{counts:?}"
    );
    assert_eq!(
        counts.get(&RefuseFamily::MissingTarget).copied(),
        Some(5),
        "{counts:?}"
    );
    assert_eq!(
        counts.get(&RefuseFamily::SymptomMismatch).copied(),
        Some(5),
        "{counts:?}"
    );
}

/// The shapes' own file-count contract, asserted separately from the
/// classifier that produced them (which reads `commands` and the goal, never
/// `files.len()`): find-shaped fixtures are genuinely MULTI-file — target
/// plus at least two plausible siblings, so the opening `find` has a
/// directory worth searching — and the other two shapes are single-file, so
/// the classifier's "everything else" branch cannot be hiding a multi-file
/// fixture whose goal happens to name its target.
#[test]
fn v3_mixed_patch_shapes_carry_the_file_counts_their_shape_implies() {
    let set = v3();
    for f in patch_fixtures(&set) {
        match patch_shape(f) {
            PatchShape::Find => assert!(
                f.files.len() >= 3,
                "fixture {} (find-shaped): only {} file(s); a find needs siblings to search past",
                f.name,
                f.files.len()
            ),
            PatchShape::Run | PatchShape::Plain => assert_eq!(
                f.files.len(),
                1,
                "fixture {}: single-target shape must carry exactly one file",
                f.name
            ),
        }
    }
}

// ---------------------------------------------------------------------------
// The run-granted slice (instrument delta 1: `commands`)
// ---------------------------------------------------------------------------

/// Exactly the 5 run-granted fixtures carry a grant, every one of them
/// carries EXACTLY `[["python3", "-m", "py_compile"]]`, and no other
/// fixture carries any grant at all. The exact-prefix half is the typo net
/// described on [`PY_COMPILE_PREFIX`]; the "no other fixture" half keeps a
/// stray grant from silently widening a fixture the protocol counts as
/// plain.
#[test]
fn v3_mixed_run_granted_fixtures_carry_the_exact_py_compile_prefix() {
    let set = v3();
    let expected: Vec<Vec<String>> =
        vec![PY_COMPILE_PREFIX.iter().map(|s| s.to_string()).collect()];
    let granted: Vec<&Fixture> = set
        .fixtures
        .iter()
        .filter(|f| !f.commands.is_empty())
        .collect();
    assert_eq!(granted.len(), 5, "expected 5 run-granted fixtures");
    for f in &granted {
        assert_eq!(
            f.commands, expected,
            "fixture {}: grant is not exactly {PY_COMPILE_PREFIX:?}",
            f.name
        );
        assert_eq!(
            f.expect,
            Expect::Patch,
            "fixture {}: not patch-class",
            f.name
        );
    }
}

/// A run-granted fixture's `run` step is `python3 -m py_compile <target>`,
/// so the target must be a python module AND must still compile once the
/// reference fix has landed — otherwise the run step could never exit 0 on
/// a correct trajectory and protocol §5's run-before-done endpoint would be
/// unreadable for reasons that have nothing to do with the model.
///
/// This re-derives the post-patch bytes and re-parses them through the real
/// `PythonLens` (which is literally `python3 -m py_compile`), rather than
/// leaning on the class-wide landing test below to imply it.
#[test]
fn v3_mixed_run_granted_targets_py_compile_after_the_reference_fix_lands() {
    let set = v3();
    for f in set.fixtures.iter().filter(|f| !f.commands.is_empty()) {
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
        let initial = target_contents(f)
            .unwrap_or_else(|| panic!("fixture {}: target not among files", f.name));
        let reference = f
            .reference
            .as_ref()
            .unwrap_or_else(|| panic!("fixture {}: no reference", f.name));
        let body = PatchBody::SearchReplace {
            search: reference.search.clone(),
            replace: reference.replace.clone(),
        };
        match land(initial, &body, &PythonLens) {
            Landing::Lands { new_contents, .. } => {
                let parsed = PythonLens.parses(&new_contents);
                assert!(
                    parsed.is_ok(),
                    "fixture {}: patched bytes do not py_compile: {parsed:?}",
                    f.name
                );
            }
            other => panic!("fixture {}: reference fix did not land: {other:?}", f.name),
        }
    }
}

// ---------------------------------------------------------------------------
// Goals
// ---------------------------------------------------------------------------

/// Every goal ends with its class's canonical trailing instruction — the
/// contract the training corpus shares with this gate (see
/// [`PATCH_INSTRUCTION`]).
#[test]
fn v3_mixed_goals_end_with_the_canonical_instruction_for_their_class() {
    for f in &v3().fixtures {
        let expected = match f.expect {
            Expect::Patch => PATCH_INSTRUCTION,
            Expect::Refuse => CHECK_INSTRUCTION,
        };
        assert!(
            f.goal.trim_end().ends_with(expected),
            "fixture {}: goal does not end with {expected:?}: {:?}",
            f.name,
            f.goal
        );
    }
}

/// Every goal that is allowed to name its target does, and is non-empty —
/// the plain rule, applied to every fixture except the find-shaped slice
/// (which inverts it, next test).
#[test]
fn v3_mixed_non_find_goals_name_their_target() {
    let set = v3();
    for f in &set.fixtures {
        assert!(!f.goal.trim().is_empty(), "fixture {}: empty goal", f.name);
        if f.expect == Expect::Patch && patch_shape(f) == PatchShape::Find {
            continue;
        }
        assert!(
            f.goal.contains(&f.target),
            "fixture {}: goal does not mention target {:?}: {:?}",
            f.name,
            f.target,
            f.goal
        );
    }
}

/// The find-shaped slice's two authoring rules, taken verbatim from the
/// factory validator that enforces them for generated tasks
/// (`task._find_shape_violations`) and re-checked here against the frozen
/// bytes:
///
/// 1. the goal names the SYMPTOM, never the target filename — with the name
///    in the goal there is nothing to find;
/// 2. the goal carries a pattern that singles the target out — at least one
///    identifier-shaped token in the goal must occur in the target's
///    contents and in NO sibling, or the opening `find` cannot identify
///    which file to read.
///
/// Rule 2 is checked over every long token in the goal (a short one like
/// `for` would hit every file and prove nothing), and only needs ONE
/// witness — that token is the pattern the trajectory would search with.
#[test]
fn v3_mixed_find_shaped_goals_name_the_symptom_and_carry_a_target_unique_pattern() {
    let set = v3();
    for f in patch_fixtures(&set)
        .into_iter()
        .filter(|f| patch_shape(f) == PatchShape::Find)
    {
        assert!(
            !f.goal.contains(&f.target),
            "fixture {}: find-shaped goal names its target {:?}",
            f.name,
            f.target
        );
        let contents = target_contents(f)
            .unwrap_or_else(|| panic!("fixture {}: target not among files", f.name));
        let siblings: Vec<&str> = f
            .files
            .iter()
            .filter(|file| file.path != f.target)
            .map(|file| file.contents.as_str())
            .collect();
        let witness = f
            .goal
            .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .filter(|tok| tok.len() >= 8)
            .find(|tok| contents.contains(*tok) && !siblings.iter().any(|s| s.contains(*tok)));
        assert!(
            witness.is_some(),
            "fixture {}: no goal token occurs in the target and in no sibling — nothing to find",
            f.name
        );
    }
}

// ---------------------------------------------------------------------------
// Class-level byte validity
// ---------------------------------------------------------------------------

/// The load-bearing patch-class check, mirroring v1's and v2's: every
/// `expect = "patch"` reference fix LANDS through the real production
/// instrument, using the real lens the fixture declares, and changes bytes.
#[test]
fn v3_mixed_patch_references_land_through_the_real_lenses() {
    let set = v3();
    for f in patch_fixtures(&set) {
        let initial = target_contents(f)
            .unwrap_or_else(|| panic!("fixture {}: target not among files", f.name));
        let reference = f
            .reference
            .as_ref()
            .unwrap_or_else(|| panic!("fixture {}: expect=\"patch\" but no reference", f.name));
        assert_ne!(
            reference.search, reference.replace,
            "fixture {}: search == replace, not a repair",
            f.name
        );
        assert_eq!(
            initial.matches(&reference.search).count(),
            1,
            "fixture {}: search must occur exactly once in the target",
            f.name
        );
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
            Landing::Lands { new_contents, .. } => assert_ne!(
                new_contents, initial,
                "fixture {}: reference fix landed but did not change bytes",
                f.name
            ),
            other => panic!(
                "fixture {} ({}): reference fix did not land: {other:?}",
                f.name, f.lens
            ),
        }
    }
}

/// Refuse-class fixtures are byte-valid, per family:
/// - target-present (defect-absent and symptom-mismatch): the target
///   genuinely parses through the REAL lens. Note this is deliberately NOT
///   "the file is correct" for symptom-mismatch — that family's file really
///   IS broken, semantically; parsing is the mechanical proxy for
///   well-formed, which both target-present families must be.
/// - missing-target: target genuinely absent, with >= 1 real sibling
///   present (never a suspiciously-empty dir).
#[test]
fn v3_mixed_refuse_fixtures_are_byte_valid() {
    let set = v3();
    for f in refuse_fixtures(&set) {
        match target_contents(f) {
            Some(contents) => {
                let parsed = match f.lens.as_str() {
                    "python" => PythonLens.parses(contents),
                    "plaintext" => PlainText.parses(contents),
                    other => panic!("fixture {}: unhandled lens {other:?}", f.name),
                };
                assert!(
                    parsed.is_ok(),
                    "fixture {}: target does not parse cleanly: {parsed:?}",
                    f.name
                );
            }
            None => assert!(
                !f.files.is_empty(),
                "fixture {} (missing-target): no real sibling file present",
                f.name
            ),
        }
    }
}

/// Every target that exists is a real file, not a stub: at least 5 lines,
/// the same floor the factory's `task.MIN_TARGET_LINES` applies to every
/// generated task. A 1-2 line target makes "read the file before deciding"
/// trivial and would flatter every model the gate scores.
#[test]
fn v3_mixed_present_targets_are_at_least_five_lines() {
    for f in &v3().fixtures {
        let Some(contents) = target_contents(f) else {
            continue;
        };
        let lines = contents.trim_end_matches('\n').split('\n').count();
        assert!(
            lines >= 5,
            "fixture {}: target is only {lines} line(s)",
            f.name
        );
    }
}

// ---------------------------------------------------------------------------
// Refusal reasons
// ---------------------------------------------------------------------------

/// Each family's `refusal_reason` follows its ruled shape — the contract
/// `refuse_family` classifies on, asserted here so the classifier cannot
/// silently mislabel drifted text:
/// - defect-absent: opens `No change needed: `;
/// - missing-target: opens `Cannot: ` and NAMES the absent file (the
///   factory validator's own missing-target rule);
/// - symptom-mismatch: the ruled two-part assembler
///   (`task.symptom_mismatch_reason`) — opens `Checked: ` and carries
///   `Found instead: `, because naming what IS there is the whole point of
///   the family.
#[test]
fn v3_mixed_refusal_reasons_follow_their_family_shape() {
    let set = v3();
    for f in refuse_fixtures(&set) {
        let reason = refusal_reason(f);
        assert!(
            !reason.trim().is_empty(),
            "fixture {}: empty reason",
            f.name
        );
        match refuse_family(f) {
            RefuseFamily::DefectAbsent => assert!(
                reason.starts_with("No change needed: "),
                "fixture {}: {reason:?}",
                f.name
            ),
            RefuseFamily::MissingTarget => {
                assert!(
                    reason.starts_with("Cannot: "),
                    "fixture {}: {reason:?}",
                    f.name
                );
                assert!(
                    reason.contains(&f.target),
                    "fixture {}: reason does not name the absent file {:?}",
                    f.name,
                    f.target
                );
            }
            RefuseFamily::SymptomMismatch => {
                assert!(
                    reason.starts_with("Checked: "),
                    "fixture {}: {reason:?}",
                    f.name
                );
                assert!(
                    reason.contains("Found instead: "),
                    "fixture {}: symptom-mismatch reason never names what IS there: {reason:?}",
                    f.name
                );
            }
        }
    }
}

/// Ruling bT5/R0: `refusal_reason` prose uses the em dash (U+2014),
/// matching the factory's `task.symptom_mismatch_reason` shape — a
/// deliberate match, not a habit, so the gate's ideal `done` text and the
/// trained one are the same shape. The ASCII `--` the GOALS use (and every
/// factory goal skeleton uses) must therefore never appear in a reason.
/// Both halves are pinned: no `--` anywhere in a reason, and every
/// target-present family's reason really does carry the em dash that
/// separates the claim from the fact.
#[test]
fn v3_mixed_refusal_reasons_use_the_em_dash_never_ascii_double_hyphen() {
    let set = v3();
    for f in refuse_fixtures(&set) {
        let reason = refusal_reason(f);
        assert!(
            !reason.contains("--"),
            "fixture {}: refusal_reason uses ASCII `--` (ruling bT5/R0 pins U+2014): {reason:?}",
            f.name
        );
        if refuse_family(f) != RefuseFamily::MissingTarget {
            assert!(
                reason.contains('\u{2014}'),
                "fixture {}: refusal_reason carries no em dash: {reason:?}",
                f.name
            );
        }
    }
}

/// The plausibility rule (G5 design doc §5), re-checked against the FROZEN
/// bytes: a goal that makes a claim ABOUT a file that is really there must
/// backtick-quote at least one identifier/value that is a literal substring
/// of that file's real contents. Applies to BOTH target-present families —
/// defect-absent and symptom-mismatch (the factory's own
/// `TARGET_PRESENT_FAMILIES`) — because both would otherwise teach "weird
/// goal -> refuse" instead of "check the file before deciding".
#[test]
fn v3_mixed_target_present_refuse_goals_quote_a_real_identifier_from_the_target() {
    let set = v3();
    for f in refuse_fixtures(&set) {
        let Some(contents) = target_contents(f) else {
            continue; // missing-target: nothing to quote from
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

// ---------------------------------------------------------------------------
// Schema-level class consistency and names
// ---------------------------------------------------------------------------

#[test]
fn v3_mixed_expect_fields_are_class_consistent() {
    for f in &v3().fixtures {
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

#[test]
fn v3_mixed_fixture_names_are_unique() {
    let mut seen = BTreeSet::new();
    for f in &v3().fixtures {
        assert!(
            seen.insert(f.name.clone()),
            "duplicate fixture name: {}",
            f.name
        );
    }
}
