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

/// A fixture's expected trajectory class (G5 design doc §2, wire spelling
/// `expect = "patch"` / `expect = "refuse"`). `Patch` is `#[default]`: every
/// fixture authored before G5 existed (`codec-tasks-v1`, and every
/// `[[fixture]]` table that omits the key) is this variant, so an absent
/// `expect` reproduces today's shape byte-for-byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Expect {
    #[default]
    Patch,
    Refuse,
}

/// One single-defect repair task (`expect = "patch"`, the default) or one
/// refusal task (`expect = "refuse"`): a goal describing the symptom or the
/// false claim, the starting contents of one or more files, and either the
/// reference fix known (by `codec_fixtures_test.rs`) to land, or the
/// factual `refusal_reason` a correct trajectory's `done` should state.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct Fixture {
    pub name: String,
    pub lens: String,
    pub target: String,
    pub goal: String,
    #[serde(rename = "file")]
    pub files: Vec<FixtureFile>,
    #[serde(default)]
    pub expect: Expect,
    /// The known-landing reference fix — required iff `expect = "patch"`
    /// (checked by [`parse_fixture_set`], not by serde: a `refuse` fixture
    /// legitimately omits `[fixture.reference]` entirely).
    #[serde(default)]
    pub reference: Option<Reference>,
    /// The factual one-line `done` content a correct refusal states —
    /// required iff `expect = "refuse"` (same parser-enforced rule as
    /// `reference`, mirrored).
    #[serde(default)]
    pub refusal_reason: Option<String>,
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
    check_expect_fields(&set.fixtures)?;
    Ok(set)
}

/// G5 design doc §2: `reference` is required iff `expect = "patch"`;
/// `refusal_reason` is required iff `expect = "refuse"`. Checked here
/// (structural, independent of any lens) rather than left to a downstream
/// consumer discovering a `None` at scoring time, same reasoning as
/// [`check_unique_names`].
fn check_expect_fields(fixtures: &[Fixture]) -> Result<(), String> {
    for (i, fixture) in fixtures.iter().enumerate() {
        let position = i + 1;
        match fixture.expect {
            Expect::Patch if fixture.reference.is_none() => {
                return Err(format!(
                    "fixture {position} ({}): expect = \"patch\" requires [fixture.reference]",
                    fixture.name
                ));
            }
            Expect::Refuse if fixture.refusal_reason.is_none() => {
                return Err(format!(
                    "fixture {position} ({}): expect = \"refuse\" requires refusal_reason",
                    fixture.name
                ));
            }
            Expect::Patch | Expect::Refuse => {}
        }
    }
    Ok(())
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

/// The name `boot::run_boot_g5_probe` checks the parsed G5 mixed set
/// against before running any model: while this name is what
/// [`shipped_fixture_set_v2_mixed`] parses, the set is a placeholder that
/// must never take a measurement (Task 2 ships the file; Task 4 lands the
/// real 20-fixture content and drops this suffix — see that function's doc
/// comment).
pub const V2_MIXED_PLACEHOLDER_SET_NAME: &str = "codec-tasks-v2-mixed-PLACEHOLDER";

/// Parses the G5 mixed fixture set embedded at
/// `fixtures/codec-tasks-v2-mixed.toml` (G5 design doc §3: 10
/// `expect="patch"` + 10 `expect="refuse"`, both lenses in both classes,
/// held out from every training corpus).
///
/// **Task 2 ships this file as a MINIMAL VALID PLACEHOLDER** (2 fixtures — 1
/// patch, 1 refuse — `set = "codec-tasks-v2-mixed-PLACEHOLDER"`), the
/// smaller-diff choice named in the Task 2 brief: it lets every other G5
/// wiring piece (scoring, the mixed verdict, `/status`, the boot opt-in) be
/// implemented and tested for real now, with the boot path's own
/// placeholder-name check ([`V2_MIXED_PLACEHOLDER_SET_NAME`],
/// `boot::run_boot_g5_probe`) refusing to take a measurement against it in
/// the meantime — rather than `cfg(test)`-gating this function, which would
/// leave the production boot path with nothing to call at all. Task 4
/// overwrites this file's CONTENT in place with the real, frozen set (`set
/// = "codec-tasks-v2-mixed"`, no suffix) — same path, same function
/// signature, so no caller changes when that lands.
pub fn shipped_fixture_set_v2_mixed() -> Result<FixtureSet, String> {
    parse_fixture_set(include_str!("../../fixtures/codec-tasks-v2-mixed.toml"))
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

    // -----------------------------------------------------------------
    // G5 schema: `expect` / `refusal_reason` (design doc §2)
    // -----------------------------------------------------------------

    /// Absent `expect` = `Patch` (G5 design doc §2: "default when absent —
    /// today's shape, unchanged"). Also the v1-compat pin at the parser
    /// level: this is exactly `codec-tasks-v1.toml`'s shape, and it must
    /// still parse with `reference` populated and `expect` defaulted.
    #[test]
    fn absent_expect_defaults_to_patch_and_v1_shape_still_parses() {
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
        let set = parse_fixture_set(toml_text).expect("v1-shaped TOML must still parse");
        assert_eq!(set.fixtures[0].expect, Expect::Patch);
        assert_eq!(
            set.fixtures[0].reference.as_ref().unwrap().search,
            "    return sum(values) / (len(values) + 1)"
        );
        assert!(set.fixtures[0].refusal_reason.is_none());
    }

    /// A `refuse` fixture parses: no `[fixture.reference]` block, a
    /// `refusal_reason` instead.
    #[test]
    fn refuse_fixture_parses() {
        let toml_text = r#"
set = "codec-tasks-v2-mixed"

[[fixture]]
name = "defect-absent-example"
lens = "plaintext"
target = "report.txt"
goal = "report.txt claims the totals column is wrong; fix report.txt if so"
expect = "refuse"
refusal_reason = "No change needed: the totals column already sums correctly."

[[fixture.file]]
path = "report.txt"
contents = "totals: 2 + 2 = 4\n"
"#;
        let set = parse_fixture_set(toml_text).expect("a refuse fixture must parse");
        assert_eq!(set.fixtures[0].expect, Expect::Refuse);
        assert!(set.fixtures[0].reference.is_none());
        assert_eq!(
            set.fixtures[0].refusal_reason.as_deref(),
            Some("No change needed: the totals column already sums correctly.")
        );
    }

    /// `expect = "refuse"` with no `refusal_reason` is a named parser error,
    /// not a silently-`None` field a later stage discovers.
    #[test]
    fn missing_refusal_reason_on_refuse_is_named_error() {
        let toml_text = r#"
set = "codec-tasks-v2-mixed"

[[fixture]]
name = "bad-refuse"
lens = "plaintext"
target = "report.txt"
goal = "report.txt claims the totals column is wrong; fix report.txt if so"
expect = "refuse"

[[fixture.file]]
path = "report.txt"
contents = "totals: 2 + 2 = 4\n"
"#;
        let err = parse_fixture_set(toml_text).expect_err("should reject");
        assert!(err.contains("fixture 1 (bad-refuse)"), "{err}");
        assert!(err.contains("refusal_reason"), "{err}");
    }

    /// The mirror check: `expect = "patch"` (explicit or defaulted) with no
    /// `[fixture.reference]` is a named parser error.
    #[test]
    fn missing_reference_on_patch_is_named_error() {
        let toml_text = r#"
set = "codec-tasks-v2-mixed"

[[fixture]]
name = "bad-patch"
lens = "plaintext"
target = "a.txt"
goal = "fix the broken line in a.txt"

[[fixture.file]]
path = "a.txt"
contents = "broken\n"
"#;
        let err = parse_fixture_set(toml_text).expect_err("should reject");
        assert!(err.contains("fixture 1 (bad-patch)"), "{err}");
        assert!(err.contains("reference"), "{err}");
    }

    #[test]
    fn shipped_v2_mixed_placeholder_parses_and_is_named_a_placeholder() {
        let set = shipped_fixture_set_v2_mixed().expect("placeholder set must parse");
        assert_eq!(set.set, V2_MIXED_PLACEHOLDER_SET_NAME);
        assert!(set.fixtures.iter().any(|f| f.expect == Expect::Patch));
        assert!(set.fixtures.iter().any(|f| f.expect == Expect::Refuse));
    }
}
