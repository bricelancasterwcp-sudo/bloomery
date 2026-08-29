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
    /// Command prefixes this fixture's grant should permit, threaded
    /// verbatim into `fixture_grant`'s `Grant` (`codec_probe/mod.rs`).
    /// Defaults empty — the whole-history shape, and what every fixture up
    /// to and including `codec-tasks-v1` and `codec-tasks-v2-mixed` still
    /// carries: read+write only, no commands, so those frozen files parse
    /// unchanged. Non-empty only for a *run-granted* gate fixture (flywheel
    /// turn-3 Task 8), whose goal cannot be scored by bytes alone and
    /// instead needs to actually run a command (e.g. `python3 -m
    /// py_compile`) to prove the fix works — see `grant/command.rs`'s
    /// `check_command` for the prefix-match semantics this list is checked
    /// against.
    #[serde(default)]
    pub commands: Vec<Vec<String>>,
    /// The refuse fixture's factory family (turn-6 spec §4.2, wire
    /// spellings `"defect-absent" | "missing-target" | "symptom-mismatch"`)
    /// — written from `RefusalTask.family`, which always existed and was
    /// simply never serialized. Serde-defaulted `None` so every v1–v4 TOML
    /// loads unchanged (the daemon stays permissive; the v5 real-fixture
    /// test asserts presence on all 16 v5 refuse rows, and the evidence
    /// tool errors if a v5 refuse row lacks it — the daemon itself never
    /// reads it).
    #[serde(default)]
    pub family: Option<String>,
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
/// `expect = "patch"` fixture's `target` names one of its own `files`.
///
/// This check applies ONLY to patch-class fixtures. A `refuse` fixture's
/// `target` MAY be absent from `files` — that absence IS the missing-target
/// family (G5 design doc §5): the goal names a file that genuinely does not
/// exist in the fixture dir. A `refuse` fixture whose target IS among files
/// is the other family (defect-absent) and remains representable too — this
/// function does not require either shape from a refuse fixture, only that
/// a *patch* fixture's target is real, since only a patch fixture's
/// `reference` fix is meant to land against it.
///
/// (Task 4 cross-task fix: this check used to be unconditional, which made
/// the missing-target refuse family unrepresentable in this schema — see
/// `.superpowers/sdd/2026-08-16-flywheel2-honest-refusal/task-3-report.md`'s
/// "Concerns" section, which flagged exactly this gap.)
///
/// Any other malformed shape (missing field, wrong type, etc.) surfaces as
/// `toml`'s own parse error, which already names the offending field.
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
        if fixture.expect == Expect::Patch
            && !fixture.files.iter().any(|f| f.path == fixture.target)
        {
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

/// The name `boot::run_boot_g5_probe` checks parsed mixed sets against
/// before running any model. This const exists so the boot guard can refuse
/// a placeholder fixture set if one ever returns — a guard that is
/// independent of which era's placeholder may exist. The placeholder era
/// for v2 ended when the real frozen set landed (commit cbe5886).
pub const V2_MIXED_PLACEHOLDER_SET_NAME: &str = "codec-tasks-v2-mixed-PLACEHOLDER";

/// Parses the real, frozen G5 mixed fixture set (the G5 design doc §3:
/// 20-fixture set with 10 `expect="patch"` + 10 `expect="refuse"`, both
/// lenses in both classes, held out from every training corpus).
///
/// The placeholder era for this set (which shipped a minimal 2-fixture
/// proof-of-concept to unblock wiring, before the real frozen set landed)
/// ended when commit cbe5886 landed the real frozen content in
/// `fixtures/codec-tasks-v2-mixed.toml` (`set = "codec-tasks-v2-mixed"`,
/// no suffix). This function parses that real frozen set.
pub fn shipped_fixture_set_v2_mixed() -> Result<FixtureSet, String> {
    parse_fixture_set(include_str!("../../fixtures/codec-tasks-v2-mixed.toml"))
}

/// The name `boot::run_boot_g5_probe` checks parsed mixed sets against
/// before running any model. This const exists so the boot guard can refuse
/// a placeholder fixture set if one ever returns — a guard that is
/// independent of which era's placeholder may exist. The placeholder era
/// for v3 ended when the real frozen set landed (commit e6c7637).
pub const V3_MIXED_PLACEHOLDER_SET_NAME: &str = "codec-tasks-v3-mixed-PLACEHOLDER";

/// Parses the real, frozen flywheel turn-3 G5 mixed fixture set (32-fixture
/// set with 16 `expect="patch"` + 16 `expect="refuse"`, both lenses in both
/// classes, held out from every training corpus), embedded at
/// `fixtures/codec-tasks-v3-mixed.toml`.
///
/// The placeholder era for this set (which shipped a minimal 2-fixture
/// proof-of-concept to unblock wiring, before the real frozen set landed)
/// ended when commit e6c7637 landed the real frozen content in
/// `fixtures/codec-tasks-v3-mixed.toml` (`set = "codec-tasks-v3-mixed"`,
/// no suffix). This function parses that real frozen set.
pub fn shipped_fixture_set_v3_mixed() -> Result<FixtureSet, String> {
    parse_fixture_set(include_str!("../../fixtures/codec-tasks-v3-mixed.toml"))
}

/// The name `boot::run_boot_g5_probe` checks parsed mixed sets against
/// before running any model. This const exists so the boot guard can refuse
/// a placeholder fixture set if one ever returns — a guard that is
/// independent of which era's placeholder may exist. The placeholder era
/// for v4 ended when flywheel turn-4 Task 5 replaced Task 3's minimal
/// 2-fixture proof-of-concept with the real frozen set — the same swap
/// commits cbe5886 and e6c7637 each made for v2 and v3.
pub const V4_MIXED_PLACEHOLDER_SET_NAME: &str = "codec-tasks-v4-mixed-PLACEHOLDER";

/// Parses the real, frozen flywheel turn-4 G5 mixed fixture set (32-fixture
/// set with 16 `expect="patch"` + 16 `expect="refuse"`, both lenses in both
/// classes, held out from every training corpus), embedded at
/// `fixtures/codec-tasks-v4-mixed.toml`.
///
/// The placeholder era for this set (which shipped a minimal 2-fixture
/// proof-of-concept to unblock Task 3's boot-wiring swap, before the real
/// frozen set landed) ended when turn-4 Task 5 landed the real frozen
/// content in `fixtures/codec-tasks-v4-mixed.toml`
/// (`set = "codec-tasks-v4-mixed"`, no suffix) — the same path/function/
/// struct shape v2's Task 4 and v3's Task 8 each used, so no caller changed
/// when it landed. This function parses that real frozen set.
///
/// Turn 4's one shape delta from v3: the five run-granted patch fixtures
/// each ship a planted `unittest` as an ordinary second
/// `[[fixture.file]]` and grant `[["python3", "-m", "unittest"]]`, so a
/// run-granted fixture is a two-file workspace rather than v3's one-file
/// one. No parser change was needed for either.
pub fn shipped_fixture_set_v4_mixed() -> Result<FixtureSet, String> {
    parse_fixture_set(include_str!("../../fixtures/codec-tasks-v4-mixed.toml"))
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

    /// Task 3's cross-task gap, fixed here: the target-among-files check
    /// (`parse_fixture_set`'s first loop) used to be unconditional, which
    /// made a missing-target *refuse* fixture unrepresentable — exactly the
    /// family this task's `templates_refusal.py` needs to author. The check
    /// now applies ONLY when `expect = "patch"`; a `refuse` fixture whose
    /// target is genuinely absent from `files` (the missing-target family)
    /// must parse cleanly.
    #[test]
    fn refuse_fixture_with_absent_target_parses() {
        let toml_text = r#"
set = "codec-tasks-v2-mixed"

[[fixture]]
name = "missing-target-example"
lens = "plaintext"
target = "does-not-exist.txt"
goal = "does-not-exist.txt needs a fix -- check it first"
expect = "refuse"
refusal_reason = "Cannot: does-not-exist.txt does not exist in this workspace."

[[fixture.file]]
path = "sibling.txt"
contents = "a real sibling file\n"
"#;
        let set = parse_fixture_set(toml_text)
            .expect("a refuse fixture with an absent target must parse");
        assert_eq!(set.fixtures[0].expect, Expect::Refuse);
        assert!(
            !set.fixtures[0]
                .files
                .iter()
                .any(|f| f.path == set.fixtures[0].target),
            "the whole point: target is genuinely absent from files"
        );
    }

    /// The mirror pin: a `refuse` fixture whose target DOES appear among
    /// `files` (the defect-absent family) must also still parse — the
    /// relaxed check does not accidentally forbid that case either.
    #[test]
    fn refuse_fixture_with_present_target_still_parses() {
        let toml_text = r#"
set = "codec-tasks-v2-mixed"

[[fixture]]
name = "defect-absent-example"
lens = "plaintext"
target = "report.txt"
goal = "report.txt claims the totals column is wrong -- check first"
expect = "refuse"
refusal_reason = "No change needed: the totals already sum correctly."

[[fixture.file]]
path = "report.txt"
contents = "totals: 2 + 2 = 4\n"
"#;
        let set = parse_fixture_set(toml_text)
            .expect("a refuse fixture with a present target must still parse");
        assert!(set.fixtures[0]
            .files
            .iter()
            .any(|f| f.path == set.fixtures[0].target));
    }

    /// The relaxation must NOT weaken `expect = "patch"` fixtures: a patch
    /// fixture with an absent target still errors with the existing named
    /// message (mirrors `rejects_a_target_absent_from_files`, but pins the
    /// explicit `expect = "patch"` spelling too, not just the defaulted
    /// case, so the class-conditional check is proven from both sides).
    #[test]
    fn explicit_patch_fixture_with_absent_target_still_errors() {
        let toml_text = r#"
set = "codec-tasks-v2-mixed"

[[fixture]]
name = "bad-patch"
lens = "plaintext"
target = "missing.txt"
goal = "goal mentioning missing.txt"
expect = "patch"

[[fixture.file]]
path = "other.txt"
contents = "hello"

[fixture.reference]
search = "hello"
replace = "goodbye"
"#;
        let err = parse_fixture_set(toml_text)
            .expect_err("a patch fixture still requires target among files");
        assert!(err.contains("fixture 1 (bad-patch)"), "{err}");
        assert!(err.contains("missing.txt"), "{err}");
        assert!(err.contains("not among files"), "{err}");
    }

    /// Task 4 lands the real, frozen `codec-tasks-v2-mixed` set (G5 design
    /// doc §3), replacing the Task-2 placeholder content in place — same
    /// path, same function, same struct shape, so no caller (`boot.rs`)
    /// changes. This test replaces
    /// `shipped_v2_mixed_placeholder_parses_and_is_named_a_placeholder`
    /// (which asserted the OPPOSITE: that the shipped file was still the
    /// placeholder) now that it no longer is — the exhaustive structural
    /// checks (20 fixtures, 10+10, both lenses per class, names unique
    /// across both shipped sets, references land, goal-plausibility) live
    /// in `tests/codec_fixtures_test.rs`; this unit test only pins the two
    /// facts scoped to this module: the set is no longer the placeholder,
    /// and its name is exactly `"codec-tasks-v2-mixed"`.
    #[test]
    fn shipped_v2_mixed_is_the_real_frozen_set_not_a_placeholder() {
        let set = shipped_fixture_set_v2_mixed().expect("the real v2-mixed set must parse");
        assert_eq!(set.set, "codec-tasks-v2-mixed");
        assert_ne!(
            set.set, V2_MIXED_PLACEHOLDER_SET_NAME,
            "boot::run_boot_g5_probe's placeholder guard must never trigger on the real set"
        );
        assert!(set.fixtures.iter().any(|f| f.expect == Expect::Patch));
        assert!(set.fixtures.iter().any(|f| f.expect == Expect::Refuse));
    }

    // -----------------------------------------------------------------
    // flywheel turn-3 Task 2: `commands` (instrument delta 1)
    // -----------------------------------------------------------------

    /// The exact shape Task 8's run-granted gate fixtures will author
    /// (Task 2 brief: `commands = [["python3", "-m", "py_compile"]]`) —
    /// pinned at the parser level so that TOML has a guaranteed parse
    /// target before Task 8 exists.
    #[test]
    fn fixture_with_commands_parses_into_commands_field() {
        let toml_text = r#"
set = "codec-tasks-test"

[[fixture]]
name = "py-mean-off-by-one"
lens = "python"
target = "stats.py"
goal = "fix mean() in stats.py"
commands = [["python3", "-m", "py_compile"]]

[[fixture.file]]
path = "stats.py"
contents = "def mean(values):\n    return sum(values) / (len(values) + 1)\n"

[fixture.reference]
search = "    return sum(values) / (len(values) + 1)"
replace = "    return sum(values) / len(values)"
"#;
        let set = parse_fixture_set(toml_text).expect("should parse");
        assert_eq!(
            set.fixtures[0].commands,
            vec![vec![
                "python3".to_string(),
                "-m".to_string(),
                "py_compile".to_string()
            ]]
        );
    }

    /// Compat pin: both frozen shipped sets predate `commands` and must
    /// still parse — with it defaulting empty for every fixture — since
    /// neither TOML file is touched by this task (brief interface: "serde
    /// default empty").
    #[test]
    fn shipped_fixture_sets_parse_with_empty_commands() {
        let v1 = shipped_fixture_set().expect("v1 must still parse");
        assert!(
            v1.fixtures.iter().all(|f| f.commands.is_empty()),
            "codec-tasks-v1 predates commands; every fixture's list must be empty"
        );
        let v2 = shipped_fixture_set_v2_mixed().expect("v2-mixed must still parse");
        assert!(
            v2.fixtures.iter().all(|f| f.commands.is_empty()),
            "codec-tasks-v2-mixed predates commands; every fixture's list must be empty"
        );
    }
}
