//! The diversity rule for the frozen `codec-tasks-v4-mixed` set (turn-3
//! design doc §3, carried into turn 4 by the g5v4 protocol §3), and the
//! shape normalizer it is defined in terms of.
//!
//! Split out of `codec_fixtures_v4_test.rs` for the same reason v3 split
//! it: together they would breach the 800-line house ceiling, and this rule
//! is a self-contained concept with its own machinery. Everything else
//! about the set — composition, shapes, goals, landing, refusal-reason
//! shapes — lives over there; turn 4's two new authoring rules live in
//! `codec_fixtures_v4_rules_test.rs`.

use bloomery_daemon::codec_probe::fixtures::{shipped_fixture_set_v4_mixed, Expect, Fixture};
use std::collections::BTreeMap;

fn v4() -> bloomery_daemon::codec_probe::fixtures::FixtureSet {
    shipped_fixture_set_v4_mixed().expect("shipped_fixture_set_v4_mixed")
}

/// The fixture's declared `target`'s contents, if `target` is among `files`
/// — `None` for a missing-target refuse fixture, where that absence is the
/// whole point. (Also declared in the sibling v4 test files: each
/// `tests/*.rs` is its own crate.)
fn target_contents(f: &Fixture) -> Option<&str> {
    f.files
        .iter()
        .find(|file| file.path == f.target)
        .map(|file| file.contents.as_str())
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
// The diversity rule (turn-3 design doc §3; g5v4 protocol §3)
// ---------------------------------------------------------------------------

/// Replaces every identifier-ish run with `X` and every digit run with `N`,
/// leaving punctuation, indentation and line structure intact. Blank lines
/// are dropped; `drop_comments` additionally drops python comment and
/// docstring lines.
///
/// **Why two passes rather than the factory's one.** The factory-side
/// `DistinctCodeShapesTest` always drops comment lines, because it compares
/// one family against ITSELF across seeds and a comment's wording wobbles
/// draw to draw. Here the bytes are frozen and hand-authored, so there is no
/// wobble to absorb — and in the plaintext lens a leading `#` is a heading,
/// i.e. real content, not a comment. Dropping them would throw away shape.
/// Keeping them, on the other hand, would let two fixtures with identical
/// code and different comments pass. Both passes are therefore required to
/// be pairwise-distinct, which closes both directions.
fn skeleton(text: &str, drop_comments: bool) -> String {
    let mut out = String::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if drop_comments && (trimmed.starts_with('#') || trimmed.starts_with("\"\"\"")) {
            continue;
        }
        out.push_str(&normalize_line(line));
        out.push('\n');
    }
    out
}

fn normalize_line(line: &str) -> String {
    let mut out = String::new();
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c.is_ascii_digit() {
            out.push('N');
            while matches!(chars.peek(), Some(d) if d.is_ascii_digit()) {
                chars.next();
            }
        } else if c.is_ascii_alphabetic() || c == '_' {
            out.push('X');
            while matches!(chars.peek(), Some(d) if d.is_ascii_alphanumeric() || *d == '_') {
                chars.next();
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// The bytes a fixture's code shape is read from: its target's contents
/// when the target exists, and otherwise (missing-target refuse fixtures,
/// where the absent target has no contents by construction) every sibling
/// file's contents concatenated in path order — the whole workspace the
/// model actually sees.
///
/// A run-granted patch fixture is read from its TARGET only, not from the
/// planted test beside it. That is deliberate: the planted test's shape is
/// a mechanical function of the target (`templates_run_verified._test_source`
/// renders the same five-statement module for every family), so including
/// it would add a constant to every run fixture's key and could only ever
/// hide a genuine target-shape collision, never reveal one.
fn shape_source(f: &Fixture) -> String {
    if let Some(contents) = target_contents(f) {
        return contents.to_string();
    }
    let mut by_path: Vec<(&str, &str)> = f
        .files
        .iter()
        .map(|file| (file.path.as_str(), file.contents.as_str()))
        .collect();
    by_path.sort_unstable();
    by_path
        .into_iter()
        .map(|(path, contents)| format!("{path}\n{contents}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The diversity rule (turn-3 design doc §3, learned from the v2 set's two
/// same-shaped defect-absent fixtures, and carried into turn 4 unchanged by
/// the g5v4 protocol §3): **no two fixtures in a class share a code
/// shape**, so a model cannot pattern-match the shape instead of reading
/// the file.
///
/// Scoped per class on purpose. A shape appearing in BOTH classes is not a
/// violation — it is the point: if every `scaled_*`-shaped file in the set
/// were a refusal, the shape alone would leak the label, and the gate would
/// stop measuring refusal honesty. v4 shares four shapes across the class
/// line for exactly this reason (the extremum loop, the `scaled_*`
/// multiplier, the dict-key lookup, and the two-section INI each appear once as a patch fixture
/// and once as a refusal).
#[test]
fn v4_mixed_no_two_fixtures_in_a_class_share_a_code_shape() {
    let set = v4();
    for (class, fixtures) in [
        ("patch", patch_fixtures(&set)),
        ("refuse", refuse_fixtures(&set)),
    ] {
        for drop_comments in [false, true] {
            let mut seen: BTreeMap<String, &str> = BTreeMap::new();
            for f in &fixtures {
                let key = skeleton(&shape_source(f), drop_comments);
                if let Some(other) = seen.insert(key, f.name.as_str()) {
                    panic!(
                        "{class} class: {} shares a code shape with {other} \
                         (drop_comments={drop_comments})",
                        f.name
                    );
                }
            }
        }
    }
}

/// The anti-vacuity companion to the rule above: cross-class shape sharing
/// is not merely tolerated, it is REQUIRED to actually occur. A set whose
/// every shape lived on one side of the class line would let a model score
/// the gate by shape alone, and the per-class scoping of the rule above
/// would silently permit it. Pinned as a floor rather than an exact count
/// so a later amendment can add more shared shapes without editing a
/// number.
#[test]
fn v4_mixed_shares_code_shapes_across_the_class_line() {
    let set = v4();
    let shapes_for = |fixtures: Vec<&Fixture>| -> Vec<String> {
        fixtures
            .into_iter()
            .map(|f| skeleton(&shape_source(f), true))
            .collect()
    };
    let patch = shapes_for(patch_fixtures(&set));
    let refuse = shapes_for(refuse_fixtures(&set));
    let shared = patch.iter().filter(|s| refuse.contains(s)).count();
    assert!(
        shared >= 3,
        "expected >= 3 code shapes present in BOTH classes, found {shared} — \
         a shape that only ever appears on one side leaks the label"
    );
}
