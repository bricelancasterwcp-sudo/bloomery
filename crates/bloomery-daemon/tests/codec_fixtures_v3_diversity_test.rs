//! The turn-3 diversity rule for the frozen `codec-tasks-v3-mixed` set
//! (design doc §3), and the shape normalizer it is defined in terms of.
//!
//! Split out of `codec_fixtures_v3_test.rs` (which is at ~770 lines) rather
//! than folded into it: together they would breach the 800-line house
//! ceiling, and this rule is a self-contained concept with its own
//! machinery. Everything else about the set — composition, shapes, goals,
//! landing, refusal-reason shapes — lives over there.

use bloomery_daemon::codec_probe::fixtures::{shipped_fixture_set_v3_mixed, Expect, Fixture};
use std::collections::BTreeMap;

fn v3() -> bloomery_daemon::codec_probe::fixtures::FixtureSet {
    shipped_fixture_set_v3_mixed().expect("shipped_fixture_set_v3_mixed")
}

/// The fixture's declared `target`'s contents, if `target` is among `files`
/// — `None` for a missing-target refuse fixture, where that absence is the
/// whole point. (Also declared in `codec_fixtures_v3_test.rs`: each
/// `tests/*.rs` file is its own crate, and a `tests/common/` module for one
/// 6-line helper is not worth the indirection.)
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
// The diversity rule (turn-3 design doc §3)
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

/// The turn-3 diversity rule (design doc §3), learned from the v2 set's two
/// same-shaped defect-absent fixtures: **no two fixtures in a class share a
/// code shape**, so a model cannot pattern-match the shape instead of
/// reading the file.
///
/// Scoped per class on purpose. A shape appearing in BOTH classes is not a
/// violation — it is the point: if every `scaled_*`-shaped file in the set
/// were a refusal, the shape alone would leak the label, and the gate would
/// stop measuring refusal honesty.
#[test]
fn v3_mixed_no_two_fixtures_in_a_class_share_a_code_shape() {
    let set = v3();
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
