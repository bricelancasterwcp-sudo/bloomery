//! Parser for the frozen G4 fixture set (Phase 2b/2c P4 Task 5).
//!
//! Deserializes the TOML wire format specified by
//! `.superpowers/sdd/2026-08-15-phase2bc-p4-codec-gate/task-5-brief.md`
//! (`set = "..."` plus repeated `[[fixture]]` tables, each carrying one or
//! more `[[fixture.file]]` tables and a single `[fixture.reference]`
//! table) directly onto the public [`FixtureSet`]/[`Fixture`] shape via
//! serde field renames — no separate "raw" struct, because the wire shape
//! and the public shape are the same shape.
//!
//! This module only parses and performs the one structural check a
//! malformed fixture can violate independent of any language lens (`target`
//! naming a file actually present in `files`); every semantic property a
//! *correctly shaped but wrong* fixture could still violate (unique names,
//! `lens` set membership, goal mentioning its target, `search != replace`,
//! and — the load-bearing one — whether the reference fix actually lands)
//! is asserted by `tests/codec_fixtures_test.rs` against
//! [`shipped_fixture_set`], not re-implemented here.

use std::collections::BTreeSet;

/// The full frozen fixture set: a name (`"codec-tasks-v1"`, travels into
/// every later G4 record) plus its fixtures, in file order.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct FixtureSet {
    pub set: String,
    #[serde(rename = "fixture")]
    pub fixtures: Vec<Fixture>,
}

/// One single-defect repair task: a goal describing the symptom, the
/// starting contents of one or more files, and the reference fix that is
/// known (by `codec_fixtures_test.rs`) to land.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct Fixture {
    pub name: String,
    pub lens: String,
    pub target: String,
    pub goal: String,
    #[serde(rename = "file")]
    pub files: Vec<FixtureFile>,
    pub reference: Reference,
}

/// One file's starting contents, keyed by its path relative to the task's
/// scratch dir.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct FixtureFile {
    pub path: String,
    pub contents: String,
}

/// The known-landing reference fix for a fixture's `target` file, in the
/// `SearchReplace` codec's own shape (`search` must match the target's
/// contents exactly once; `replace` is what takes its place).
#[derive(Debug, Clone, serde::Deserialize)]
pub struct Reference {
    pub search: String,
    pub replace: String,
}

/// Parses `toml_text` under the wire format above and checks that every
/// fixture's `target` names one of its own `files`. Any other malformed
/// shape (missing field, wrong type, etc.) surfaces as `toml`'s own parse
/// error, which already names the offending field.
///
/// Errors are 1-indexed and named with the fixture's position and name
/// (e.g. `"fixture 7 (py-...): target 'x.py' not among files"`) so an
/// authoring mistake in a 20-entry TOML file doesn't require a binary
/// search to locate.
pub fn parse_fixture_set(toml_text: &str) -> Result<FixtureSet, String> {
    let set: FixtureSet =
        toml::from_str(toml_text).map_err(|e| format!("failed to parse fixture set: {e}"))?;
    for (i, fixture) in set.fixtures.iter().enumerate() {
        let position = i + 1;
        if !fixture.files.iter().any(|f| f.path == fixture.target) {
            return Err(format!(
                "fixture {position} ({}): target '{}' not among files",
                fixture.name, fixture.target
            ));
        }
    }
    check_unique_names(&set.fixtures)?;
    Ok(set)
}

/// Duplicate fixture names would silently alias two different tasks under
/// one identifier in every later G4 record — checked here (not left solely
/// to the test suite) because it is a structural property of the parsed
/// set itself, independent of any lens.
fn check_unique_names(fixtures: &[Fixture]) -> Result<(), String> {
    let mut seen = BTreeSet::new();
    for (i, fixture) in fixtures.iter().enumerate() {
        if !seen.insert(fixture.name.as_str()) {
            return Err(format!(
                "fixture {} ({}): duplicate name",
                i + 1,
                fixture.name
            ));
        }
    }
    Ok(())
}

/// Parses the fixture set frozen into the daemon binary at
/// `fixtures/codec-tasks-v1.toml` (embedded via `include_str!`, so the
/// shipped set is exactly what was compiled, not whatever happens to be on
/// disk at runtime).
pub fn shipped_fixture_set() -> Result<FixtureSet, String> {
    parse_fixture_set(include_str!("../../fixtures/codec-tasks-v1.toml"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_two_brief_examples() {
        let toml_text = r#"
set = "codec-tasks-v1"

[[fixture]]
name = "py-mean-off-by-one"
lens = "python"
target = "stats.py"
goal = "fix mean() in stats.py"

[[fixture.file]]
path = "stats.py"
contents = "def mean(values):\n    return sum(values) / (len(values) + 1)\n"

[fixture.reference]
search = "    return sum(values) / (len(values) + 1)"
replace = "    return sum(values) / len(values)"
"#;
        let set = parse_fixture_set(toml_text).expect("should parse");
        assert_eq!(set.set, "codec-tasks-v1");
        assert_eq!(set.fixtures.len(), 1);
        assert_eq!(set.fixtures[0].files[0].path, "stats.py");
    }

    #[test]
    fn rejects_a_target_absent_from_files() {
        let toml_text = r#"
set = "codec-tasks-v1"

[[fixture]]
name = "bad"
lens = "plaintext"
target = "missing.txt"
goal = "goal mentioning missing.txt"

[[fixture.file]]
path = "other.txt"
contents = "hello"

[fixture.reference]
search = "hello"
replace = "goodbye"
"#;
        let err = parse_fixture_set(toml_text).expect_err("should reject");
        assert!(err.contains("fixture 1 (bad)"), "{err}");
        assert!(err.contains("missing.txt"), "{err}");
    }
}
