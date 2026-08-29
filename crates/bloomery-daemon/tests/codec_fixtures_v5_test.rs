//! Structural validation for the frozen G5-v5 fixture set
//! `codec-tasks-v5-mixed` (turn-6 design doc §4; composition and pass
//! floor: `docs/superpowers/evidence/2026-08-29-g5v5-protocol.md` §3/§4).
//! Same GPU-free, seconds-fast **authored-artifact** posture as
//! `codec_fixtures_v4_test.rs`, which this file mirrors, seam included:
//!
//! - here: the carried v3/v4 rule set applied to v5 (composition, shapes,
//!   goals, landing through the REAL lenses, quoting, the em dash, names),
//!   PLUS turn 6's two fixture-record deltas (spec §4.2): the `family` key
//!   on every refuse row, and `refusal_reason` as the FULL ideal v5
//!   declared `done` — parsed, mapped to the family, and its `evidence:`
//!   lines checked verbatim against the frozen fixture bytes;
//! - `codec_fixtures_v5_rules_test.rs`: turn 4's two carried authoring
//!   rules (fresh framing, the EXECUTED run slice) + the exact grant;
//! - `codec_fixtures_v5_diversity_test.rs`: the diversity rule.
//!
//! Cross-set name uniqueness stays in `codec_fixtures_test.rs`
//! (`fixture_names_are_unique_across_all_shipped_sets`), the one assertion
//! that must see all FIVE shipped sets at once. `python3` must be on
//! `PATH`; `PythonLens::parses` fails closed if it is absent, so the
//! python-lens assertions fail loudly rather than silently skipping.

use bloomery_core::action::lens::{land, Landing, LandingLens, PlainText};
use bloomery_core::action::PatchBody;
use bloomery_daemon::codec_probe::fixtures::{
    shipped_fixture_set_v5_mixed, Expect, Fixture, V5_MIXED_PLACEHOLDER_SET_NAME,
};
use bloomery_daemon::task::lens_py::PythonLens;
use std::collections::{BTreeMap, BTreeSet};

/// The two canonical trailing instructions, one per class — deliberately
/// identical to the factory's `task.DONE_INSTRUCTION` /
/// `task.CHECK_INSTRUCTION`; the fresh-framing rule exempts this suffix.
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

/// The exact wire spellings the `family` key may carry (turn-6 spec §4.2)
/// and their fixed mapping to the v5 `done` card's operator-facing reason
/// vocabulary (spec §3.2) — the `reason_matches_family` table, restated so
/// the frozen bytes are checked independently of the recompute tool.
const FAMILY_REASON_MAP: [(&str, &str); 3] = [
    ("defect-absent", "no-defect"),
    ("missing-target", "no-such-file"),
    ("symptom-mismatch", "different-defect"),
];

fn v5() -> bloomery_daemon::codec_probe::fixtures::FixtureSet {
    shipped_fixture_set_v5_mixed().expect("shipped_fixture_set_v5_mixed")
}

/// The declared `target`'s contents, if among `files` — `None` for a
/// missing-target refuse fixture, where that absence is the whole point.
/// (Re-declared per v5 test file: each `tests/*.rs` is its own crate; the
/// duplication is on the backlog as a `tests/common/` candidate.)
fn target_contents(f: &Fixture) -> Option<&str> {
    f.files
        .iter()
        .find(|file| file.path == f.target)
        .map(|file| file.contents.as_str())
}

/// Every substring enclosed in a matched pair of backticks, in order —
/// the factory's `task.REFUSAL_QUOTED_RE` convention, restated (this crate
/// has no Python-factory dependency) so the frozen bytes are checked
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

/// A patch fixture's shape — v4's classifier, derived from two properties
/// INDEPENDENT of `files.len()` so the per-shape file-count assertions
/// below are not circular: a non-empty `commands` grant means run-granted;
/// among the rest, a goal that does NOT name its own target file is
/// find-shaped; everything else is plain.
fn patch_shape(f: &Fixture) -> PatchShape {
    if !f.commands.is_empty() {
        PatchShape::Run
    } else if !f.goal.contains(&f.target) {
        PatchShape::Find
    } else {
        PatchShape::Plain
    }
}

/// A refuse fixture's family, read from the NEW `family` key — never from
/// the name or reason text (v5's delta from v4's structural classifier):
/// the `reason_matches_family` endpoint reads this key, so the freeze test
/// reads the same source of truth. Presence, spelling, and structural
/// consistency each have their own pinning test below.
fn family(f: &Fixture) -> &str {
    f.family
        .as_deref()
        .unwrap_or_else(|| panic!("fixture {}: expect=\"refuse\" but no family key", f.name))
}

fn refusal_reason(f: &Fixture) -> &str {
    f.refusal_reason
        .as_deref()
        .unwrap_or_else(|| panic!("fixture {}: refuse-class but no refusal_reason", f.name))
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

/// The parsed view of a v5 `refusal_reason` — the full ideal declared
/// `done` (spec §4.2): header attributes, leading `evidence:` lines,
/// prose. Panics with the fixture's name on any malformation.
struct DoneBlock<'a> {
    reason: &'a str,
    evidence: Vec<&'a str>,
    prose: Vec<&'a str>,
}

const DONE_HEADER_PREFIX: &str = "<action verb=\"done\" outcome=\"refused\" reason=\"";
const DONE_HEADER_SUFFIX: &str = "\">";
const EVIDENCE_PREFIX: &str = "evidence: ";

fn parse_done_block(f: &Fixture) -> DoneBlock<'_> {
    let text = refusal_reason(f);
    let mut lines = text.split('\n');
    let header = lines
        .next()
        .unwrap_or_else(|| panic!("fixture {}: empty refusal_reason", f.name));
    assert!(
        header.starts_with(DONE_HEADER_PREFIX) && header.ends_with(DONE_HEADER_SUFFIX),
        "fixture {}: refusal_reason does not open with the declared done header: {header:?}",
        f.name
    );
    let reason = &header[DONE_HEADER_PREFIX.len()..header.len() - DONE_HEADER_SUFFIX.len()];
    let body: Vec<&str> = lines.collect();
    assert_eq!(
        body.last().copied(),
        Some("</action>"),
        "fixture {}: refusal_reason does not close with </action>",
        f.name
    );
    let body = &body[..body.len() - 1];
    let split = body
        .iter()
        .position(|line| !line.starts_with(EVIDENCE_PREFIX))
        .unwrap_or(body.len());
    DoneBlock {
        reason,
        evidence: body[..split].to_vec(),
        prose: body[split..].to_vec(),
    }
}

// ---------------------------------------------------------------------------
// Set identity and composition (protocol §3, pinned)
// ---------------------------------------------------------------------------

#[test]
fn shipped_v5_mixed_parses() {
    let result = shipped_fixture_set_v5_mixed();
    assert!(
        result.is_ok(),
        "shipped v5-mixed failed to parse: {result:?}"
    );
}

/// The frozen instrument is named `codec-tasks-v5-mixed` — no
/// `-PLACEHOLDER` suffix, which is exactly what `boot::run_boot_g5_probe`
/// checks before it will take a measurement at all.
#[test]
fn v5_mixed_set_name_is_exact_and_not_the_placeholder() {
    let set = v5();
    assert_eq!(set.set, "codec-tasks-v5-mixed");
    assert_ne!(
        set.set, V5_MIXED_PLACEHOLDER_SET_NAME,
        "the placeholder guard must never trigger on the frozen set"
    );
}

#[test]
fn v5_mixed_has_thirty_two_fixtures_sixteen_patch_sixteen_refuse() {
    let set = v5();
    assert_eq!(set.fixtures.len(), 32, "expected N=32 fixtures");
    assert_eq!(patch_fixtures(&set).len(), 16, "expected 16 patch fixtures");
    assert_eq!(
        refuse_fixtures(&set).len(),
        16,
        "expected 16 refuse fixtures"
    );
}

/// Both lenses (python, plaintext) represented in BOTH classes — the shape
/// requirement carried unchanged since the v2 set.
#[test]
fn v5_mixed_both_lenses_represented_in_both_classes() {
    let set = v5();
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
fn v5_mixed_every_lens_is_python_or_plaintext() {
    for f in &v5().fixtures {
        assert!(
            f.lens == "python" || f.lens == "plaintext",
            "fixture {} has unknown lens {:?}",
            f.name,
            f.lens
        );
    }
}

/// Protocol §3's pinned patch composition, per (shape, lens): 6 find
/// (3 python + 3 plaintext), 5 run-granted (all python — a planted
/// `unittest` has no plaintext analogue), 5 plain (2 + 3). Exact counts,
/// never floors: they are §5's secondary-endpoint denominators.
#[test]
fn v5_mixed_patch_shape_counts_are_exact() {
    let set = v5();
    let mut counts: BTreeMap<(PatchShape, &str), usize> = BTreeMap::new();
    for f in patch_fixtures(&set) {
        *counts.entry((patch_shape(f), f.lens.as_str())).or_default() += 1;
    }
    let expected: BTreeMap<(PatchShape, &str), usize> = BTreeMap::from([
        ((PatchShape::Find, "python"), 3),
        ((PatchShape::Find, "plaintext"), 3),
        ((PatchShape::Run, "python"), 5),
        ((PatchShape::Plain, "python"), 2),
        ((PatchShape::Plain, "plaintext"), 3),
    ]);
    assert_eq!(counts, expected);
}

/// Protocol §3's pinned refuse composition, per (family, lens), the family
/// read from the NEW `family` key: 6 defect-absent (3 + 3), 5
/// missing-target (2 python + 3 plaintext), 5 symptom-mismatch (3 python +
/// 2 plaintext).
#[test]
fn v5_mixed_refuse_family_counts_are_exact() {
    let set = v5();
    let mut counts: BTreeMap<(&str, &str), usize> = BTreeMap::new();
    for f in refuse_fixtures(&set) {
        *counts.entry((family(f), f.lens.as_str())).or_default() += 1;
    }
    let expected: BTreeMap<(&str, &str), usize> = BTreeMap::from([
        (("defect-absent", "python"), 3),
        (("defect-absent", "plaintext"), 3),
        (("missing-target", "python"), 2),
        (("missing-target", "plaintext"), 3),
        (("symptom-mismatch", "python"), 3),
        (("symptom-mismatch", "plaintext"), 2),
    ]);
    assert_eq!(counts, expected);
}

// ---------------------------------------------------------------------------
// The v5 `family` key (turn-6 spec §4.2)
// ---------------------------------------------------------------------------

/// Every refuse row carries `family`, spelled exactly one of the three
/// hyphenated wire spellings; no patch row carries one. The spec's
/// `reason_matches_family` endpoint reads this key and never infers family
/// from the name, so a missing or misspelled key would silently score a
/// whole family as unmeasurable — pinned here instead.
#[test]
fn v5_mixed_family_key_present_on_all_refuse_rows_with_the_exact_spellings() {
    let valid: BTreeSet<&str> = FAMILY_REASON_MAP.iter().map(|(f, _)| *f).collect();
    for f in &v5().fixtures {
        match f.expect {
            Expect::Refuse => assert!(
                valid.contains(family(f)),
                "fixture {}: family {:?} is not one of {valid:?}",
                f.name,
                family(f)
            ),
            Expect::Patch => assert!(
                f.family.is_none(),
                "fixture {}: a patch fixture must not carry a family key",
                f.name
            ),
        }
    }
}

/// The family key must agree with the structure it labels: missing-target
/// iff the target is genuinely absent from `files`. (Defect-absent and
/// symptom-mismatch share the target-present structure by design; the
/// reason attribute test below is what keeps THOSE two apart.)
#[test]
fn v5_mixed_family_key_is_structurally_consistent() {
    for f in refuse_fixtures(&v5()) {
        let fam = family(f);
        assert_eq!(
            fam == "missing-target",
            target_contents(f).is_none(),
            "fixture {}: family {fam:?} contradicts the target's presence among files",
            f.name
        );
    }
}

/// The shapes' own file-count contract, asserted separately from the
/// classifier that produced them. Carried from v4 unchanged: find needs
/// siblings to search past; run is EXACTLY the target plus its planted
/// test; plain is exactly one file.
#[test]
fn v5_mixed_patch_shapes_carry_the_file_counts_their_shape_implies() {
    let set = v5();
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
/// contract the training corpus shares with this gate.
#[test]
fn v5_mixed_goals_end_with_the_canonical_instruction_for_their_class() {
    for f in &v5().fixtures {
        let expected = match f.expect {
            Expect::Patch => PATCH_INSTRUCTION,
            Expect::Refuse => CHECK_INSTRUCTION,
        };
        assert!(
            f.goal.trim_end().ends_with(expected),
            "fixture {}: goal does not end with {expected:?}",
            f.name
        );
    }
}

/// Every goal that is allowed to name its target does, and is non-empty —
/// the plain rule, applied to every fixture except the find-shaped slice
/// (which inverts it, next test).
#[test]
fn v5_mixed_non_find_goals_name_their_target() {
    let set = v5();
    for f in &set.fixtures {
        assert!(!f.goal.trim().is_empty(), "fixture {}: empty goal", f.name);
        if f.expect == Expect::Patch && patch_shape(f) == PatchShape::Find {
            continue;
        }
        assert!(
            f.goal.contains(&f.target),
            "fixture {}: goal does not mention target {:?}",
            f.name,
            f.target
        );
    }
}

/// The find-shaped slice's two authoring rules, carried from v4 (and the
/// factory's `task._find_shape_violations`): (1) the goal names the
/// SYMPTOM, never the target filename; (2) the goal carries a pattern that
/// singles the target out — at least one identifier-shaped token in the
/// goal must occur on one line of the target's contents and in NO sibling.
///
/// One v5 delta: the witness threshold is 6 characters rather than v4's 8.
/// v5's hand-authored plaintext find goals lean on short domain nouns —
/// `v5-patch-find-txt-02`'s only target-unique goal tokens are `yealms`
/// and `middle` (6 chars each) — and a 6-char token in the target and no
/// sibling still singles the file out, which is the property the rule
/// exists for; the length floor only excludes glue words (`for`, `the`)
/// that would hit every file and prove nothing.
#[test]
fn v5_mixed_find_shaped_goals_name_the_symptom_and_carry_a_target_unique_pattern() {
    let set = v5();
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
            .filter(|tok| tok.len() >= 6)
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

/// The v4-era punctuation ruling carried (v5 TOML header, from bT5/R0),
/// goal half: goals use ASCII `--`, never the em dash — the em dash is the
/// refusal PROSE's marker (next section), and a goal carrying one would
/// blur the one typographic seam the ruling pins between the two.
#[test]
fn v5_mixed_goals_use_ascii_double_hyphen_never_the_em_dash() {
    for f in &v5().fixtures {
        assert!(
            !f.goal.contains('\u{2014}'),
            "fixture {}: goal contains an em dash (bT5/R0 pins ASCII `--` in goals)",
            f.name
        );
    }
}

// ---------------------------------------------------------------------------
// Class-level byte validity
// ---------------------------------------------------------------------------

/// The load-bearing patch-class check, mirroring every prior set's: every
/// `expect = "patch"` reference fix LANDS through the real production
/// instrument, using the real lens the fixture declares, and changes bytes.
#[test]
fn v5_mixed_patch_references_land_through_the_real_lenses() {
    let set = v5();
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

/// Refuse-class fixtures are byte-valid, per family (the byte-unchanged
/// expectation's precondition — a correct trajectory leaves these bytes
/// exactly as shipped): target-present families genuinely parse through
/// the REAL lens (deliberately NOT "the file is correct" for
/// symptom-mismatch, whose file really IS broken semantically; parsing is
/// the proxy for well-formed); missing-target is genuinely absent with
/// >= 1 real sibling present (never a suspiciously-empty dir).
#[test]
fn v5_mixed_refuse_fixtures_are_byte_valid() {
    let set = v5();
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

/// Every target that exists is a real file, not a stub: at least 5 lines
/// — the factory's `task.MIN_TARGET_LINES` floor, carried from the v4
/// suite unchanged. A near-empty target makes "read the file before
/// deciding" trivial and would flatter every model the gate scores.
#[test]
fn v5_mixed_present_targets_are_at_least_five_lines() {
    let mut violations = Vec::new();
    for f in &v5().fixtures {
        let Some(contents) = target_contents(f) else {
            continue;
        };
        let lines = contents.trim_end_matches('\n').split('\n').count();
        if lines < 5 {
            violations.push(format!(
                "fixture {}: target is only {lines} line(s)",
                f.name
            ));
        }
    }
    assert!(
        violations.is_empty(),
        "targets below the factory's MIN_TARGET_LINES floor of 5:\n{}",
        violations.join("\n")
    );
}

// ---------------------------------------------------------------------------
// Refusal reasons: the full ideal v5 declared `done` (turn-6 spec §4.2)
// ---------------------------------------------------------------------------

/// v5's headline record delta: `refusal_reason` is the FULL ideal declared
/// `done` — a `<action verb="done" outcome="refused" reason="...">` block
/// whose reason attribute matches the fixture's `family` through the fixed
/// spec-§3.2 mapping, with >= 1 `evidence:` line and non-empty prose.
/// Evidence lines are strictly LEADING (spec §3.2) — an `evidence:` line
/// after prose began would not parse as evidence in `validate_done`, so it
/// is refused here too.
#[test]
fn v5_mixed_refusal_reasons_are_full_declared_done_blocks_matching_their_family() {
    let mapping: BTreeMap<&str, &str> = FAMILY_REASON_MAP.iter().copied().collect();
    for f in refuse_fixtures(&v5()) {
        let block = parse_done_block(f);
        assert_eq!(
            block.reason,
            mapping[family(f)],
            "fixture {}: reason attribute does not match family {:?} (spec §3.2 mapping)",
            f.name,
            family(f)
        );
        assert!(
            !block.evidence.is_empty(),
            "fixture {}: ideal done carries no evidence: line",
            f.name
        );
        assert!(
            !block.prose.join("\n").trim().is_empty(),
            "fixture {}: ideal done carries no prose after its evidence lines",
            f.name
        );
        for line in &block.prose {
            assert!(
                !line.starts_with(EVIDENCE_PREFIX),
                "fixture {}: evidence line after prose began would parse as prose: {line:?}",
                f.name
            );
        }
    }
}

/// The instrument's own reasons must pass the instrument's own check (spec
/// §4.2): every leading `evidence:` line is either
/// - `evidence: <path>:<line> `quote`` — `path` names a file this fixture
///   really carries, `<line>` is 1-based, and the quote (the text between
///   the line's first and last backtick, per the card grammar) is a
///   verbatim substring of that file ON that line; or
/// - `evidence: <target> absent` — missing-target family only, with the
///   named target genuinely absent from `files`.
///
/// Every missing-target reason must carry the absent line (it is that
/// family's whole evidence), and no other family may.
#[test]
fn v5_mixed_refusal_evidence_lines_are_grounded_in_the_fixture_bytes() {
    for f in refuse_fixtures(&v5()) {
        let block = parse_done_block(f);
        let fam = family(f);
        let absent_form = format!("{}{} absent", EVIDENCE_PREFIX, f.target);
        let mut saw_absent = false;
        for line in &block.evidence {
            let ctx = |what: &str| format!("fixture {}: {what}: {line:?}", f.name);
            if **line == absent_form {
                assert_eq!(
                    fam,
                    "missing-target",
                    "{}",
                    ctx("absent-form outside family")
                );
                assert!(
                    target_contents(f).is_none(),
                    "{}",
                    ctx("target is among files")
                );
                saw_absent = true;
                continue;
            }
            let rest = &line[EVIDENCE_PREFIX.len()..];
            let first_bt = rest
                .find('`')
                .unwrap_or_else(|| panic!("{}", ctx("neither quote-form nor absent-form")));
            let last_bt = rest.rfind('`').expect("rfind after find cannot fail");
            assert!(first_bt < last_bt, "{}", ctx("no closed backtick quote"));
            assert!(
                rest[last_bt + 1..].is_empty(),
                "{}",
                ctx("text after the quote")
            );
            let quote = &rest[first_bt + 1..last_bt];
            let locator = rest[..first_bt].trim_end();
            let colon = locator
                .rfind(':')
                .unwrap_or_else(|| panic!("{}", ctx("locator has no `:line`")));
            let (path, line_no_str) = (&locator[..colon], &locator[colon + 1..]);
            let line_no: usize = line_no_str
                .parse()
                .unwrap_or_else(|_| panic!("{}", ctx("line number is not a number")));
            let file = f
                .files
                .iter()
                .find(|file| file.path == path)
                .unwrap_or_else(|| panic!("{}", ctx("path is not among this fixture's files")));
            let file_lines: Vec<&str> = file.contents.split('\n').collect();
            assert!(
                line_no >= 1 && line_no <= file_lines.len(),
                "{} ({} has {} lines)",
                ctx("line number out of range"),
                path,
                file_lines.len()
            );
            assert!(
                file_lines[line_no - 1].contains(quote),
                "{} (that line is {:?})",
                ctx("quote is not a verbatim substring of its stated line"),
                file_lines[line_no - 1]
            );
        }
        assert_eq!(
            saw_absent,
            fam == "missing-target",
            "fixture {}: the `<target> absent` line must appear iff the family is missing-target",
            f.name
        );
    }
}

/// Ruling bT5/R0 carried into the declared-done era, reason half: refusal
/// prose uses the em dash (U+2014), never the ASCII `--` the goals use.
/// Both halves pinned across the WHOLE block: no `--` anywhere in a
/// reason (none of the verbatim-quoted file lines carries one either),
/// and every reason's prose really does carry the em dash — v5 extends
/// this to missing-target too (v4 exempted it; v5's ideal dones all
/// carry the dash).
#[test]
fn v5_mixed_refusal_reasons_use_the_em_dash_never_ascii_double_hyphen() {
    for f in refuse_fixtures(&v5()) {
        let reason = refusal_reason(f);
        assert!(
            !reason.contains("--"),
            "fixture {}: refusal_reason uses ASCII `--` (ruling bT5/R0 pins U+2014): {reason:?}",
            f.name
        );
        let block = parse_done_block(f);
        assert!(
            block.prose.join("\n").contains('\u{2014}'),
            "fixture {}: refusal prose carries no em dash: {reason:?}",
            f.name
        );
    }
}

/// The plausibility rule at v4's strengthened strength, carried: a goal
/// claiming something ABOUT a file that is really there must
/// backtick-quote >= 1 identifier/value from it — and EVERY quoted span
/// must be a literal substring, not merely one of them (v3's mutation
/// testing found the "any" gap). Both target-present families.
#[test]
fn v5_mixed_target_present_refuse_goals_quote_only_real_identifiers() {
    let set = v5();
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

/// Each class carries exactly its own field: patch has `reference` and no
/// `refusal_reason`; refuse the mirror image. The parser already enforces
/// the "has" halves (`check_expect_fields`); the "has not" halves are
/// pinned only here.
#[test]
fn v5_mixed_expect_fields_are_class_consistent() {
    for f in &v5().fixtures {
        let (has_reference, has_reason) = (f.reference.is_some(), f.refusal_reason.is_some());
        let ok = match f.expect {
            Expect::Patch => has_reference && !has_reason,
            Expect::Refuse => has_reason && !has_reference,
        };
        assert!(
            ok,
            "fixture {} ({:?}): reference={has_reference}, refusal_reason={has_reason} — \
             each class must carry exactly its own field",
            f.name, f.expect
        );
    }
}

#[test]
fn v5_mixed_fixture_names_are_unique() {
    let mut seen = BTreeSet::new();
    for f in &v5().fixtures {
        assert!(seen.insert(f.name.clone()), "duplicate name: {}", f.name);
    }
}
