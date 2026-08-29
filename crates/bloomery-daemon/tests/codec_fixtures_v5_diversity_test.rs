//! The diversity rule for the frozen `codec-tasks-v5-mixed` set (turn-3
//! design doc §3, carried into turn 6 by the spec's §4.1 "the diversity
//! rule asserted at freeze"), and the shape normalizer it is defined in
//! terms of — the same normalizer the v3 and v4 suites use.
//!
//! Split out of `codec_fixtures_v5_test.rs` for the same reason every
//! prior set split it: the 800-line house ceiling, and this rule is a
//! self-contained concept with its own machinery.
//!
//! One deliberate delta from the v4 suite: v4's companion test
//! (`v4_mixed_shares_code_shapes_across_the_class_line`) pinned a FLOOR of
//! cross-class shape sharing, because v4 was authored with four shared
//! shapes and its header said so. v5's header and the turn-6 spec pin only
//! the per-class rule, and the frozen v5 bytes measurably share ZERO
//! shapes across the class line — all 32 shapes are pairwise distinct.
//! That satisfies the per-class rule maximally, so the v4 floor is not
//! carried; it was a pin on v4's authored property, not part of the rule.

use bloomery_daemon::codec_probe::fixtures::{shipped_fixture_set_v5_mixed, Expect, Fixture};
use std::collections::BTreeMap;

fn v5() -> bloomery_daemon::codec_probe::fixtures::FixtureSet {
    shipped_fixture_set_v5_mixed().expect("shipped_fixture_set_v5_mixed")
}

/// The fixture's declared `target`'s contents, if `target` is among `files`
/// — `None` for a missing-target refuse fixture, where that absence is the
/// whole point. (Also declared in the sibling v5 test files: each
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
// The diversity rule (turn-3 design doc §3; turn-6 spec §4.1)
// ---------------------------------------------------------------------------

/// Replaces every identifier-ish run with `X` and every digit run with `N`,
/// leaving punctuation, indentation and line structure intact. Blank lines
/// are dropped; `drop_comments` additionally drops python comment and
/// docstring lines.
///
/// Both passes (comments kept, comments dropped) are required to be
/// pairwise-distinct, closing both directions: dropped-only would let two
/// fixtures with identical code and different comments pass; kept-only
/// would throw away shape in the plaintext lens, where a leading `#` is a
/// heading, i.e. real content (see the v4 suite's fuller note).
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
/// when the target exists, and otherwise (missing-target refuse fixtures)
/// every sibling file's contents concatenated in path order — the whole
/// workspace the model actually sees.
///
/// A run-granted patch fixture is read from its TARGET only, not from the
/// planted test beside it: the planted test's shape is a mechanical
/// function of the target, so including it would add a constant to every
/// run fixture's key and could only ever hide a genuine target-shape
/// collision, never reveal one (the v4 suite's reasoning, unchanged).
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
/// same-shaped defect-absent fixtures, carried into turn 6 unchanged by
/// the spec's §4.1): **no two fixtures in a class share a code shape**, so
/// a model cannot pattern-match the shape instead of reading the file.
/// Scoped per class, exactly as every prior set scoped it.
#[test]
fn v5_mixed_no_two_fixtures_in_a_class_share_a_code_shape() {
    let set = v5();
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

/// Anti-vacuity for the normalizer itself: a normalizer that collapsed
/// everything to one key would make the rule above fail loudly, but one
/// that returned its INPUT unchanged (or a unique key per fixture by
/// accident of a bug) would pass vacuously. Pin the two properties the
/// rule depends on: identifier and digit runs really do collapse (two
/// differently-named, differently-numbered lines with the same punctuation
/// shape normalize identically), and line structure really is kept (a
/// two-line text never equals a one-line one).
#[test]
fn the_shape_normalizer_collapses_names_and_keeps_structure() {
    assert_eq!(
        skeleton("total_kg = weight * 3\n", false),
        skeleton("sum_mm = gauge * 45\n", false),
        "identifier/digit runs must collapse to X/N"
    );
    assert_ne!(
        skeleton("a = 1\nb = 2\n", false),
        skeleton("a = 1\n", false),
        "line structure must be preserved"
    );
    assert_eq!(
        skeleton("# a comment\nx = 1\n", true),
        skeleton("x = 1\n", true),
        "drop_comments must drop python comment lines"
    );
    assert_ne!(
        skeleton("# a comment\nx = 1\n", false),
        skeleton("x = 1\n", false),
        "comments-kept pass must keep them"
    );
}
