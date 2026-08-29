//! Structural validation for the frozen G5-v4 fixture set
//! `codec-tasks-v4-mixed` (flywheel turn-4 design doc §4; the pinned
//! composition and the pass floor live in
//! `docs/superpowers/evidence/2026-08-21-g5v4-protocol.md` §3/§4).
//!
//! Same posture as `codec_fixtures_v3_test.rs`, which this file mirrors: a
//! GPU-free, seconds-fast **authored-artifact check** that measures no model
//! at all — it proves the frozen instrument is internally consistent, and
//! that every patch fixture's reference fix actually **lands** through the
//! real production landing path (`bloomery_core::action::lens::land` with
//! the real `PlainText` / `PythonLens`), before the set is ever used to
//! score anything.
//!
//! **Why three files rather than one.** v3 already needed two (the suite
//! plus its diversity rule). Turn 4 adds two whole new authoring rules —
//! fresh-framed refuse goals and a run slice whose planted `unittest` is
//! actually EXECUTED — and folding either into this file would breach the
//! 800-line house ceiling. The seam:
//!
//! - here: the v3 rule set carried forward to v4 (composition, shapes,
//!   goals, landing, refusal-reason shapes, quoting, the em dash, names);
//! - `codec_fixtures_v4_rules_test.rs`: turn 4's two NEW rules, plus the
//!   exact grant prefix, which belongs beside the run slice it describes;
//! - `codec_fixtures_v4_diversity_test.rs`: the diversity rule and its
//!   normalizer, exactly as v3 split it.
//!
//! The one v4 assertion that genuinely belongs in `codec_fixtures_test.rs`
//! stayed there: cross-set name uniqueness, which has to see every
//! shipped set at once
//! (`fixture_names_are_unique_across_all_shipped_sets`).
//!
//! `python3` must be on `PATH` for the python-lens assertions to mean
//! anything; `PythonLens::parses` fails closed if it is absent, so those
//! assertions fail loudly rather than silently skipping (see
//! `codec_fixtures_test.rs`'s header for the full reasoning).

use bloomery_core::action::lens::{land, Landing, LandingLens, PlainText};
use bloomery_core::action::PatchBody;
use bloomery_daemon::codec_probe::fixtures::{
    shipped_fixture_set_v4_mixed, Expect, Fixture, V4_MIXED_PLACEHOLDER_SET_NAME,
};
use bloomery_daemon::task::lens_py::PythonLens;
use std::collections::{BTreeMap, BTreeSet};

/// The two canonical trailing instructions, one per class. Identical to the
/// factory's `task.DONE_INSTRUCTION` / `task.CHECK_INSTRUCTION` —
/// deliberately so, and this is the ONE thing turn 4's fresh-framing rule
/// (`codec_fixtures_v4_rules_test.rs`) deliberately exempts: the trailing
/// instruction is the protocol contract shared by the training corpus and
/// the gate, and the refuse class's is the only part of a refusal goal that
/// tells the model to look before it leaps. Everything *before* it is
/// authored differently from the corpus on purpose.
const PATCH_INSTRUCTION: &str = "Patch the file, then emit done.";
const CHECK_INSTRUCTION: &str =
    "Check first, and only patch if it is genuinely wrong; then emit done.";

/// The three patch-class shapes the protocol pins counts for (§3: 6
/// multi-file find-shaped + 5 run-granted + 5 plain single-target).
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

fn v4() -> bloomery_daemon::codec_probe::fixtures::FixtureSet {
    shipped_fixture_set_v4_mixed().expect("shipped_fixture_set_v4_mixed")
}

/// The fixture's declared `target`'s contents, if `target` is among
/// `files` — `None` for a missing-target refuse fixture, where that
/// absence is the whole point. (Re-declared in each v4 test file rather
/// than shared: each `tests/*.rs` is its own crate, and the 4-way
/// duplication is already on the backlog as a `tests/common/` candidate.)
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
/// from two structural facts, exactly as the three slices are authored:
/// - target absent from `files` → missing-target (the parser's own doc
///   comment states this equivalence);
/// - otherwise the `refusal_reason`'s ruled opening: symptom-mismatch's
///   two-part "Checked: … Found instead: …" assembler
///   (`task.symptom_mismatch_reason`) versus defect-absent's plain "No
///   change needed: …".
///
/// The reason prefixes are asserted as an exact contract by
/// `v4_mixed_refusal_reasons_follow_their_family_shape` below, so this
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
fn shipped_v4_mixed_parses() {
    let result = shipped_fixture_set_v4_mixed();
    assert!(
        result.is_ok(),
        "shipped_fixture_set_v4_mixed() failed: {result:?}"
    );
}

/// The frozen instrument is named `codec-tasks-v4-mixed` — no
/// `-PLACEHOLDER` suffix, which is exactly what `boot::run_boot_g5_probe`
/// checks before it will take a measurement at all.
#[test]
fn v4_mixed_set_name_is_exact_and_not_the_placeholder() {
    let set = v4();
    assert_eq!(set.set, "codec-tasks-v4-mixed");
    assert_ne!(
        set.set, V4_MIXED_PLACEHOLDER_SET_NAME,
        "the placeholder guard must never trigger on the frozen set"
    );
}

#[test]
fn v4_mixed_has_thirty_two_fixtures_sixteen_patch_sixteen_refuse() {
    let set = v4();
    assert_eq!(set.fixtures.len(), 32, "expected N=32 fixtures");
    assert_eq!(patch_fixtures(&set).len(), 16, "expected 16 patch fixtures");
    assert_eq!(
        refuse_fixtures(&set).len(),
        16,
        "expected 16 refuse fixtures"
    );
}

/// Both lenses (python, plaintext) represented in BOTH classes — the shape
/// requirement carried unchanged from the v2 and v3 sets.
#[test]
fn v4_mixed_both_lenses_represented_in_both_classes() {
    let set = v4();
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
fn v4_mixed_every_lens_is_python_or_plaintext() {
    for f in &v4().fixtures {
        assert!(
            f.lens == "python" || f.lens == "plaintext",
            "fixture {} has unknown lens {:?}",
            f.name,
            f.lens
        );
    }
}

/// Protocol §3's pinned patch composition: 6 multi-file find-shaped, 5
/// run-granted, 5 plain single-target. These are the denominators §5's
/// secondary endpoints report against, so they are exact counts, never
/// floors.
#[test]
fn v4_mixed_patch_shape_counts_are_exact() {
    let set = v4();
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
fn v4_mixed_refuse_family_counts_are_exact() {
    let set = v4();
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
/// classifier that produced them (which reads `commands` and the goal,
/// never `files.len()`).
///
/// **Turn 4 changes the run slice's number, and that change is the point.**
/// Under v3 a run-granted fixture was single-file, because its verification
/// was `python3 -m py_compile <target>` — a check that needs nothing but the
/// target, and (spec §3) a check that could never fail. Turn 4's run slice
/// plants a real `unittest` beside the target, so a run-granted fixture is
/// EXACTLY two files: the target and its planted test. One file would mean
/// no test to run; three would mean something else is in the workspace that
/// the executed-run check in `codec_fixtures_v4_rules_test.rs` never
/// accounts for.
#[test]
fn v4_mixed_patch_shapes_carry_the_file_counts_their_shape_implies() {
    let set = v4();
    for f in patch_fixtures(&set) {
        match patch_shape(f) {
            PatchShape::Find => assert!(
                f.files.len() >= 3,
                "fixture {} (find-shaped): only {} file(s); a find needs siblings to search past",
                f.name,
                f.files.len()
            ),
            PatchShape::Run => assert_eq!(
                f.files.len(),
                2,
                "fixture {} (run-granted): must carry exactly the target and its planted test",
                f.name
            ),
            PatchShape::Plain => assert_eq!(
                f.files.len(),
                1,
                "fixture {}: the plain shape must carry exactly one file",
                f.name
            ),
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
fn v4_mixed_goals_end_with_the_canonical_instruction_for_their_class() {
    for f in &v4().fixtures {
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
fn v4_mixed_non_find_goals_name_their_target() {
    let set = v4();
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
/// The witness token is additionally required to be a single-line literal
/// in the target, which is what makes the substring check above imply
/// anything about `exec_find`'s line-by-line REGEX match
/// (`task.FIND_PATTERN_LITERAL_RE`'s own note).
#[test]
fn v4_mixed_find_shaped_goals_name_the_symptom_and_carry_a_target_unique_pattern() {
    let set = v4();
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
            .find(|tok| {
                contents.contains(*tok)
                    && !siblings.iter().any(|s| s.contains(*tok))
                    && contents.lines().any(|line| line.contains(*tok))
            });
        assert!(
            witness.is_some(),
            "fixture {}: no goal token occurs on one line of the target and in no sibling — \
             nothing to find",
            f.name
        );
    }
}

// ---------------------------------------------------------------------------
// Class-level byte validity
// ---------------------------------------------------------------------------

/// The load-bearing patch-class check, mirroring v1's, v2's and v3's: every
/// `expect = "patch"` reference fix LANDS through the real production
/// instrument, using the real lens the fixture declares, and changes bytes.
#[test]
fn v4_mixed_patch_references_land_through_the_real_lenses() {
    let set = v4();
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
fn v4_mixed_refuse_fixtures_are_byte_valid() {
    let set = v4();
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
fn v4_mixed_present_targets_are_at_least_five_lines() {
    for f in &v4().fixtures {
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
///   (`task.symptom_mismatch_reason`) — opens `Checked: `, carries
///   `Found instead: `, and closes with the assembler's own trailing
///   clause, because naming what IS there is the whole point of the family.
#[test]
fn v4_mixed_refusal_reasons_follow_their_family_shape() {
    let set = v4();
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
                assert!(
                    reason.ends_with("; no change made without a goal that matches."),
                    "fixture {}: symptom-mismatch reason drops the assembler's closing clause \
                     (task.symptom_mismatch_reason): {reason:?}",
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
fn v4_mixed_refusal_reasons_use_the_em_dash_never_ascii_double_hyphen() {
    let set = v4();
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
/// bytes and **strengthened for v4**: a goal that makes a claim ABOUT a
/// file that is really there must backtick-quote at least one
/// identifier/value from that file — and now EVERY quoted span must be a
/// literal substring of the file, not merely one of them.
///
/// Why stronger than v3's rule. v3's suite asserted `any` quoted span was
/// real, matching the factory's own `validate_refusal_task`. Its own
/// mutation testing then found the gap (task-8 report, mutation M11): a
/// goal quoting two spans, one real and one invented, satisfies "any" while
/// still teaching the model that a confabulated identifier in a goal is
/// normal. The reason-grounding endpoint turn 4 adds (spec §4) measures
/// exactly that property on the model's `done` text; an instrument whose
/// own goals could quote something absent would be a poor place to measure
/// it from. Applies to BOTH target-present families — defect-absent and
/// symptom-mismatch (the factory's `TARGET_PRESENT_FAMILIES`).
#[test]
fn v4_mixed_target_present_refuse_goals_quote_only_real_identifiers() {
    let set = v4();
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
        for span in &quoted {
            assert!(
                contents.contains(span),
                "fixture {}: quoted span {span:?} does not appear in the target's real contents",
                f.name
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Schema-level class consistency and names
// ---------------------------------------------------------------------------

#[test]
fn v4_mixed_expect_fields_are_class_consistent() {
    for f in &v4().fixtures {
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
fn v4_mixed_fixture_names_are_unique() {
    let mut seen = BTreeSet::new();
    for f in &v4().fixtures {
        assert!(
            seen.insert(f.name.clone()),
            "duplicate fixture name: {}",
            f.name
        );
    }
}
