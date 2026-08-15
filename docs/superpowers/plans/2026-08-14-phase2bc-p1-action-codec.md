# bloomery Phase 2b/2c — P1: Action Codec + Validators Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the pure, GPU-free action codec — parse exactly one typed `Action` from a model turn (envelope-constrained, never grammar-forced), with repair-loop diagnostics, both patch codecs (search/replace + whole-file), and the applies-and-parses landing lens as a trait — all in `bloomery-core` with no daemon, no I/O, no substrate.

**Architecture:** One new module tree under `crates/bloomery-core/src/action/`: `mod.rs` (the `Action`/`ActionError` types + the top-level `parse_action`), `envelope.rs` (find the single `<action>` block), `verbs.rs` (per-verb attribute/body validation), `patch.rs` (the two patch codecs + `PatchCodec` selection), `lens.rs` (the `LandingLens` trait + plain-text impl + `Unparsed` reason). Everything is a pure function or a trait with a pure impl; the only "landing" that touches a real language checker (Python) is deferred to P3 where a subprocess is allowed — P1 ships the trait and the plain-text lens so the shape is fixed and testable now.

**Tech Stack:** Existing workspace (Rust stable, edition 2021). Dependency allowlist gains exactly one: `regex` (for `find`'s pattern validation and search/replace scanning — add to `[workspace.dependencies]` as `regex = "1"`, used only by `bloomery-core`). No other additions.

**Spec:** `docs/superpowers/specs/2026-08-14-phase2bc-task-abi-grants-design.md` §3 (approved 2026-08-14). Umbrella laws §3 bind everything.

## Global Constraints

- **Pure and GPU-free:** every symbol in this plan is a pure function or a trait with a pure impl. No `std::fs`, no `std::process`, no sockets, no clocks, no substrate. `cargo test -p bloomery-core` exercises all of it.
- **Envelope-constrained, never grammar-forced** (law 3): the parser recognizes a fenced `<action …>…</action>` block and ignores all surrounding prose; it never rejects a token mid-stream — it parses what's there and returns a typed diagnostic naming the defect AND the expected shape.
- **Exactly one action per turn:** zero → `ActionError::NoAction`; two or more → `ActionError::MultipleActions { found }`. The parser never guesses which of several to use.
- **Diagnostics are for the repair loop:** every `ActionError` variant carries the expected shape as data (black-oxide's measured lesson — repair ergonomics dominate). No bare "parse error" strings.
- **Landing lens = applies-and-parses, named in every record** (assay discipline): a patch lands iff the codec applies to the current bytes AND the result parses for the file's language; unknown language → a named `Unparsed` reason, never a false "lands".
- **Dependency allowlist** gains only `regex`. TDD with RED/GREEN evidence per task; `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings` (default AND `--features llama` for clippy), `cargo test --workspace` green before every commit. Files ≤800 lines; conventional commits.
- Commands run from repo root `~/workspace/bloomery`, branch `feat/phase2bc-p1-codec` (create from master at start).

---

### Task 1: Action types + the envelope scanner

**Files:**
- Create: `crates/bloomery-core/src/action/mod.rs`, `crates/bloomery-core/src/action/envelope.rs`
- Modify: `crates/bloomery-core/src/lib.rs` (add `pub mod action;`)
- Test: `crates/bloomery-core/tests/action_envelope_test.rs`

**Interfaces:**
- Produces (consumed by Tasks 2–4):

```rust
// mod.rs
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub enum Action {
    Read  { path: String, lines: Option<(u32, u32)> },
    Find  { pattern: String, path: String },
    Patch { path: String, body: PatchBody },     // PatchBody defined in Task 3
    Run   { argv: Vec<String> },
    Done  { summary: String },
}
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub enum ActionError {
    NoAction,
    MultipleActions { found: usize },
    UnknownVerb { verb: String, expected: &'static [&'static str] },
    MissingAttr { verb: &'static str, attr: &'static str },
    BadRange { got: String, expected: &'static str },       // Task 2
    BadRegex { pattern: String, detail: String },           // Task 2
    EmptyBody { verb: &'static str, expected: &'static str },// Task 2
    BadArgv { detail: String, expected: &'static str },     // Task 2
    PatchNoSearchMarker { expected: &'static str },         // Task 3
    PatchNoDivider { expected: &'static str },              // Task 3
    PatchNoReplaceMarker { expected: &'static str },        // Task 3
    BadCodec { detail: String },                            // Task 3
}
pub const VERBS: &[&str] = &["read", "find", "patch", "run", "done"];

// envelope.rs
/// The raw, un-validated contents of the single <action> block.
#[derive(Debug, Clone, PartialEq)]
pub struct RawAction {
    pub verb: String,
    pub attrs: std::collections::BTreeMap<String, String>,
    pub body: String,   // exactly the bytes between the tag and </action>, trimmed of one leading/trailing newline only
}
/// Scans a model turn for exactly one <action ...>...</action> block.
pub fn scan_envelope(turn: &str) -> Result<RawAction, ActionError>;
```

Envelope grammar (binding): an action block opens with `<action` followed by whitespace-separated `key="value"` attrs (double-quoted; value may contain anything but `"`) and a `>`, then arbitrary body bytes, then `</action>`. Attributes parse into `attrs`; `verb` is the mandatory `verb="…"` attr lifted out (its absence when a block exists → `MissingAttr { verb: "action", attr: "verb" }`). Prose outside the block is ignored. Zero blocks → `NoAction`; ≥2 opening `<action` tags → `MultipleActions { found }`.

- [ ] **Step 1: Write the failing tests.**

```rust
use bloomery_core::action::{scan_envelope, ActionError};

#[test]
fn scans_a_single_block_ignoring_prose() {
    let turn = "I'll read the file.\n<action verb=\"read\" path=\"src/a.rs\">\n</action>\ndone thinking";
    let raw = scan_envelope(turn).unwrap();
    assert_eq!(raw.verb, "read");
    assert_eq!(raw.attrs.get("path").map(String::as_str), Some("src/a.rs"));
    assert_eq!(raw.body, "");
}

#[test]
fn body_bytes_are_preserved_minus_one_framing_newline() {
    let turn = "<action verb=\"patch\" path=\"p\">\nline1\nline2\n</action>";
    let raw = scan_envelope(turn).unwrap();
    assert_eq!(raw.body, "line1\nline2");
}

#[test]
fn no_block_is_no_action() {
    assert_eq!(scan_envelope("just talking, no action here"), Err(ActionError::NoAction));
}

#[test]
fn two_blocks_is_multiple_actions() {
    let turn = "<action verb=\"read\" path=\"a\"></action><action verb=\"done\">x</action>";
    assert_eq!(scan_envelope(turn), Err(ActionError::MultipleActions { found: 2 }));
}

#[test]
fn a_block_without_verb_attr_is_named() {
    let turn = "<action path=\"a\">\n</action>";
    assert_eq!(scan_envelope(turn),
        Err(ActionError::MissingAttr { verb: "action", attr: "verb" }));
}
```

- [ ] **Step 2: RED run.** `cargo test -p bloomery-core --test action_envelope_test` → module unresolved.

- [ ] **Step 3: Implement.** Add `regex` to workspace deps + `bloomery-core/Cargo.toml`. `mod.rs`: the `Action`/`ActionError`/`VERBS` declarations + `pub mod envelope;` + re-exports (`pub use envelope::{scan_envelope, RawAction};`). `envelope.rs`: count `<action` occurrences for the multiple check; a regex or hand scan captures the opening tag's attr string up to the first `>`, then the body up to `</action>`; parse attrs with a small `key="value"` regex into a `BTreeMap` (deterministic order); lift `verb`. Body framing: strip exactly one leading `\n` and one trailing `\n` if present (so `>\n…\n</action>` yields the inner content), nothing else.

- [ ] **Step 4: GREEN + gates.** Test, fmt, clippy (both configs).

- [ ] **Step 5: Commit.** `git add crates/ Cargo.toml Cargo.lock && git commit -m "feat: action codec - types + envelope scanner"`

---

### Task 2: Verb validation (read / find / run / done)

**Files:**
- Create: `crates/bloomery-core/src/action/verbs.rs`
- Modify: `crates/bloomery-core/src/action/mod.rs` (add `pub mod verbs;` + the top-level `parse_action` dispatching non-patch verbs; patch arm added in Task 3)
- Test: `crates/bloomery-core/tests/action_verbs_test.rs`

**Interfaces:**
- Consumes: `RawAction`, `Action`, `ActionError`, `scan_envelope` (Task 1).
- Produces (consumed by Task 3 which adds the `patch` arm, and by P3):

```rust
// mod.rs
/// Scan + validate one action end to end. Patch is added in Task 3;
/// until then a "patch" verb returns ActionError::UnknownVerb-free path
/// is NOT taken — Task 2 leaves the patch arm as `todo!()`-free by
/// dispatching patch to a stub that returns BadCodec{detail:"patch not wired"}.
pub fn parse_action(turn: &str) -> Result<Action, ActionError>;

// verbs.rs
pub fn validate_read(raw: &RawAction) -> Result<Action, ActionError>;
pub fn validate_find(raw: &RawAction) -> Result<Action, ActionError>;
pub fn validate_run(raw: &RawAction) -> Result<Action, ActionError>;
pub fn validate_done(raw: &RawAction) -> Result<Action, ActionError>;
```

Validation rules (binding, from spec §3 table):
- `read`: `path` attr required (`MissingAttr`); optional `lines="A-B"` → parse to `(A,B)` with `A ≤ B`, both ≥ 1, else `BadRange { got, expected: "lines=\"A-B\" with 1 ≤ A ≤ B" }`.
- `find`: `pattern` attr required and non-empty; `path` attr required; the pattern must compile as a regex, else `BadRegex { pattern, detail }` (detail = the regex error string).
- `run`: body is a JSON array of strings, non-empty, every element a string → `Action::Run { argv }`; else `BadArgv { detail, expected: "a JSON array of strings, e.g. [\"cargo\",\"test\"]" }`.
- `done`: body non-empty (trimmed) → `Action::Done { summary: body.trim().to_string() }`; else `EmptyBody { verb: "done", expected: "a non-empty summary" }`.
- Unknown verb → `UnknownVerb { verb, expected: VERBS-as-static }`.

- [ ] **Step 1: Write the failing tests.**

```rust
use bloomery_core::action::{parse_action, Action, ActionError};

fn wrap(inner: &str) -> String { format!("<action {inner}</action>") }

#[test]
fn read_with_valid_range() {
    let a = parse_action(&wrap("verb=\"read\" path=\"src/a.rs\" lines=\"10-20\">\n")).unwrap();
    assert_eq!(a, Action::Read { path: "src/a.rs".into(), lines: Some((10, 20)) });
}

#[test]
fn read_without_lines_is_whole_file() {
    let a = parse_action(&wrap("verb=\"read\" path=\"p\">\n")).unwrap();
    assert_eq!(a, Action::Read { path: "p".into(), lines: None });
}

#[test]
fn read_inverted_range_is_named_with_expected_shape() {
    let e = parse_action(&wrap("verb=\"read\" path=\"p\" lines=\"20-10\">\n")).unwrap_err();
    match e { ActionError::BadRange { got, expected } => {
        assert_eq!(got, "20-10");
        assert!(expected.contains("A ≤ B"));
    } other => panic!("{other:?}") }
}

#[test]
fn find_requires_a_compiling_regex() {
    let e = parse_action(&wrap("verb=\"find\" pattern=\"(unclosed\" path=\"src\">\n")).unwrap_err();
    assert!(matches!(e, ActionError::BadRegex { .. }));
    let ok = parse_action(&wrap("verb=\"find\" pattern=\"fn \\w+\" path=\"src\">\n")).unwrap();
    assert_eq!(ok, Action::Find { pattern: "fn \\w+".into(), path: "src".into() });
}

#[test]
fn run_parses_a_json_argv_array() {
    let a = parse_action(&wrap("verb=\"run\">\n[\"cargo\", \"test\"]\n")).unwrap();
    assert_eq!(a, Action::Run { argv: vec!["cargo".into(), "test".into()] });
}

#[test]
fn run_rejects_non_array_body_with_expected_shape() {
    let e = parse_action(&wrap("verb=\"run\">\ncargo test\n")).unwrap_err();
    match e { ActionError::BadArgv { expected, .. } => assert!(expected.contains("JSON array")),
        other => panic!("{other:?}") }
}

#[test]
fn done_needs_a_summary() {
    assert_eq!(parse_action(&wrap("verb=\"done\">\nfixed the bug\n")).unwrap(),
        Action::Done { summary: "fixed the bug".into() });
    assert!(matches!(parse_action(&wrap("verb=\"done\">\n   \n")).unwrap_err(),
        ActionError::EmptyBody { .. }));
}

#[test]
fn unknown_verb_lists_the_expected_set() {
    let e = parse_action(&wrap("verb=\"delete\" path=\"p\">\n")).unwrap_err();
    match e { ActionError::UnknownVerb { verb, expected } => {
        assert_eq!(verb, "delete");
        assert!(expected.contains(&"read") && expected.contains(&"done"));
    } other => panic!("{other:?}") }
}
```

- [ ] **Step 2: RED run.** Unresolved `parse_action`.

- [ ] **Step 3: Implement.** `parse_action` = `scan_envelope` then match `raw.verb`: read/find/run/done → the validators; `"patch"` → the temporary stub returning `BadCodec { detail: "patch not wired".into() }` (Task 3 replaces this arm); anything else → `UnknownVerb`. JSON parsing for `run` uses `serde_json::from_str::<Vec<String>>` (already a workspace dep). Regex compile via `regex::Regex::new`.

- [ ] **Step 4: GREEN + gates.**

- [ ] **Step 5: Commit.** `git commit -m "feat: action codec - read/find/run/done validation with repair diagnostics"`

---

### Task 3: Patch codecs + the patch verb arm

**Files:**
- Create: `crates/bloomery-core/src/action/patch.rs`
- Modify: `crates/bloomery-core/src/action/mod.rs` (add `PatchBody`, `PatchCodec`, `pub mod patch;`, wire the real `patch` arm in `parse_action`)
- Test: `crates/bloomery-core/tests/action_patch_test.rs`

**Interfaces:**
- Consumes: `RawAction`, `Action`, `ActionError` (Tasks 1–2).
- Produces (consumed by Task 4's lens and by P3's executor):

```rust
// mod.rs
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum PatchCodec { SearchReplace, WholeFile }
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub enum PatchBody {
    SearchReplace { search: String, replace: String },
    WholeFile { contents: String },
}
/// Validate a patch body under the given codec. `parse_action` calls this
/// with the codec the caller selected (P3 passes the model's profile codec;
/// P1 tests pass it explicitly). Signature of the public entry:
pub fn parse_action_with_codec(turn: &str, patch_codec: PatchCodec) -> Result<Action, ActionError>;
/// `parse_action(turn)` keeps working: it defaults patch_codec = SearchReplace.
pub fn parse_action(turn: &str) -> Result<Action, ActionError>;

// patch.rs
pub fn parse_patch_body(body: &str, codec: PatchCodec) -> Result<PatchBody, ActionError>;
/// Apply a validated patch to the current file bytes. Pure — returns the
/// new bytes or a named non-application reason. (Landing = this applies
/// AND Task 4's lens parses the result.)
pub fn apply_patch(current: &str, body: &PatchBody) -> Result<String, PatchApplyError>;
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub enum PatchApplyError {
    SearchNotFound { search: String },
    SearchNotUnique { search: String, occurrences: usize },
}
```

Rules (binding):
- SearchReplace body format: `<<<<<<< SEARCH\n{search}\n=======\n{replace}\n>>>>>>> REPLACE` (the robigo/assay codec). Missing `<<<<<<< SEARCH` → `PatchNoSearchMarker`; missing `=======` → `PatchNoDivider`; missing `>>>>>>> REPLACE` → `PatchNoReplaceMarker`. Each carries the expected marker string.
- WholeFile body: the entire body is the new file contents (may be empty — an empty file is a valid whole-file replacement; no error).
- `apply_patch` for SearchReplace: the `search` string must appear **exactly once** in `current` (robigo's safety rule) — zero → `SearchNotFound`, ≥2 → `SearchNotUnique { occurrences }`; exactly one → replace it, return the new string. WholeFile: return `contents` verbatim (always applies).

- [ ] **Step 1: Write the failing tests.**

```rust
use bloomery_core::action::{parse_action_with_codec, Action, ActionError, PatchBody, PatchCodec};
use bloomery_core::action::patch::{parse_patch_body, apply_patch, PatchApplyError};

fn sr_block(path: &str, search: &str, replace: &str) -> String {
    format!("<action verb=\"patch\" path=\"{path}\">\n<<<<<<< SEARCH\n{search}\n=======\n{replace}\n>>>>>>> REPLACE\n</action>")
}

#[test]
fn search_replace_parses() {
    let a = parse_action_with_codec(&sr_block("f.py", "old", "new"), PatchCodec::SearchReplace).unwrap();
    assert_eq!(a, Action::Patch { path: "f.py".into(),
        body: PatchBody::SearchReplace { search: "old".into(), replace: "new".into() } });
}

#[test]
fn search_replace_missing_divider_is_named() {
    let bad = "<action verb=\"patch\" path=\"f\">\n<<<<<<< SEARCH\nold\nnew\n>>>>>>> REPLACE\n</action>";
    match parse_action_with_codec(bad, PatchCodec::SearchReplace).unwrap_err() {
        ActionError::PatchNoDivider { expected } => assert_eq!(expected, "======="),
        other => panic!("{other:?}"),
    }
}

#[test]
fn whole_file_takes_the_whole_body() {
    let turn = "<action verb=\"patch\" path=\"f\">\nnew contents\nline two\n</action>";
    let a = parse_action_with_codec(turn, PatchCodec::WholeFile).unwrap();
    assert_eq!(a, Action::Patch { path: "f".into(),
        body: PatchBody::WholeFile { contents: "new contents\nline two".into() } });
}

#[test]
fn apply_search_replace_requires_a_unique_match() {
    let body = PatchBody::SearchReplace { search: "x".into(), replace: "y".into() };
    assert_eq!(apply_patch("a x b", &body).unwrap(), "a y b");
    assert!(matches!(apply_patch("no match", &body), Err(PatchApplyError::SearchNotFound { .. })));
    match apply_patch("x x", &body) {
        Err(PatchApplyError::SearchNotUnique { occurrences, .. }) => assert_eq!(occurrences, 2),
        other => panic!("{other:?}"),
    }
}

#[test]
fn apply_whole_file_always_replaces() {
    let body = PatchBody::WholeFile { contents: "brand new".into() };
    assert_eq!(apply_patch("anything at all", &body).unwrap(), "brand new");
}

#[test]
fn parse_patch_body_direct_missing_search_marker() {
    match parse_patch_body("======\nx\n>>>>>>> REPLACE", PatchCodec::SearchReplace).unwrap_err() {
        ActionError::PatchNoSearchMarker { expected } => assert_eq!(expected, "<<<<<<< SEARCH"),
        other => panic!("{other:?}"),
    }
}
```

- [ ] **Step 2: RED run.**

- [ ] **Step 3: Implement.** `patch.rs`: `parse_patch_body` splits on the three markers in order (find each as a line; report the first missing one); `apply_patch` counts occurrences with `str::matches`. `mod.rs`: add `PatchCodec`/`PatchBody`; `parse_action_with_codec` replaces Task 2's stub patch arm with `parse_patch_body(&raw.body, patch_codec)` then `Action::Patch`; `parse_action` delegates with `PatchCodec::SearchReplace`. Occurrences for SearchNotUnique: `current.matches(search).count()`.

- [ ] **Step 4: GREEN + gates.**

- [ ] **Step 5: Commit.** `git commit -m "feat: action codec - patch codecs (search/replace + whole-file) with apply safety"`

---

### Task 4: The landing lens trait + plain-text impl

**Files:**
- Create: `crates/bloomery-core/src/action/lens.rs`
- Modify: `crates/bloomery-core/src/action/mod.rs` (`pub mod lens;`)
- Test: `crates/bloomery-core/tests/action_lens_test.rs`

**Interfaces:**
- Consumes: `PatchBody`, `apply_patch`, `PatchApplyError` (Task 3).
- Produces (consumed by P3's patch executor and P4's G4 probe):

```rust
// lens.rs
/// The outcome of testing whether a patch LANDS: applies-and-parses.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub enum Landing {
    Lands { new_contents: String, lens: &'static str },   // applied AND parsed
    DidNotApply { reason: PatchApplyError, lens: &'static str },
    DidNotParse { detail: String, lens: &'static str },
    Unparsed { language: String, lens: &'static str },    // unknown language — never a false "lands"
}
/// A language-specific parse check. Pure; P1 ships PlainText. P3 adds a
/// Python impl (subprocess) behind the same trait.
pub trait LandingLens {
    fn name(&self) -> &'static str;
    /// Does this string parse as a valid document in the lens's language?
    fn parses(&self, contents: &str) -> Result<(), String>;   // Err(detail) on parse failure
}
pub struct PlainText;
impl LandingLens for PlainText { /* name "plaintext"; parses = always Ok */ }
/// Test whether a patch lands: apply it, then run the lens on the result.
pub fn land(current: &str, body: &PatchBody, lens: &dyn LandingLens) -> Landing;
```

Rules (binding, assay lens discipline): `land` first calls `apply_patch`; on `Err` → `DidNotApply { reason, lens: lens.name() }` (no parse attempted — it didn't apply). On `Ok(new)` → `lens.parses(&new)`: `Ok` → `Lands { new_contents: new, lens }`; `Err(detail)` → `DidNotParse { detail, lens }`. The `Unparsed` variant is produced by a lens whose `parses` cannot judge the language — `PlainText` never returns it (it accepts everything), but the variant exists so P3's Python lens can return `Unparsed` for a non-`.py` file handed to it rather than falsely passing. The lens name rides in every `Landing` (named-lens law).

- [ ] **Step 1: Write the failing tests.**

```rust
use bloomery_core::action::{PatchBody};
use bloomery_core::action::patch::PatchApplyError;
use bloomery_core::action::lens::{land, Landing, LandingLens, PlainText};

#[test]
fn plaintext_lands_a_unique_search_replace() {
    let body = PatchBody::SearchReplace { search: "old".into(), replace: "new".into() };
    match land("a old b", &body, &PlainText) {
        Landing::Lands { new_contents, lens } => {
            assert_eq!(new_contents, "a new b");
            assert_eq!(lens, "plaintext");
        } other => panic!("{other:?}"),
    }
}

#[test]
fn a_non_applying_patch_reports_did_not_apply_not_did_not_parse() {
    let body = PatchBody::SearchReplace { search: "absent".into(), replace: "x".into() };
    match land("nothing here", &body, &PlainText) {
        Landing::DidNotApply { reason, lens } => {
            assert!(matches!(reason, PatchApplyError::SearchNotFound { .. }));
            assert_eq!(lens, "plaintext");
        } other => panic!("expected DidNotApply, got {other:?}"),
    }
}

#[test]
fn a_lens_that_rejects_produces_did_not_parse_with_its_name() {
    struct AlwaysReject;
    impl LandingLens for AlwaysReject {
        fn name(&self) -> &'static str { "reject" }
        fn parses(&self, _c: &str) -> Result<(), String> { Err("syntax boom".into()) }
    }
    let body = PatchBody::WholeFile { contents: "whatever".into() };
    match land("x", &body, &AlwaysReject) {
        Landing::DidNotParse { detail, lens } => {
            assert_eq!(detail, "syntax boom");
            assert_eq!(lens, "reject");
        } other => panic!("{other:?}"),
    }
}

#[test]
fn an_unparsed_lens_never_falsely_lands() {
    // A lens that declines to judge returns Unparsed via its own logic;
    // land() surfaces whatever the lens's parses() decision maps to — here
    // we prove the wiring: a lens returning Err is DidNotParse, and a lens
    // can signal "not my language" by name; the Unparsed variant is
    // constructed by such a lens in P3. Here we assert PlainText never
    // yields Unparsed (it accepts all).
    let body = PatchBody::WholeFile { contents: "binary\0bytes".into() };
    assert!(matches!(land("x", &body, &PlainText), Landing::Lands { .. }));
}
```

- [ ] **Step 2: RED run.**

- [ ] **Step 3: Implement.** `lens.rs` per the interface. `PlainText::parses` returns `Ok(())` always; `name` = `"plaintext"`. `land` = apply then map as specified. (The `Unparsed` variant is part of the public enum for P3; P1 has no lens that emits it, and the test above documents that PlainText does not — that's correct, not a gap.)

- [ ] **Step 4: GREEN + gates.**

- [ ] **Step 5: Commit.** `git commit -m "feat: action codec - applies-and-parses landing lens (trait + plaintext)"`

---

### Task 5: Round-trip integration + the verb card + docs

**Files:**
- Create: `crates/bloomery-core/src/action/card.rs`, `crates/bloomery-core/tests/action_integration_test.rs`
- Modify: `crates/bloomery-core/src/action/mod.rs` (`pub mod card;` + re-exports), `README.md` (a short "Action codec (Phase 2b P1)" subsection under the status/limits area — what exists now, what P3/P4 add)
- Test: (the integration test above)

**Interfaces:**
- Produces (consumed by P3's prompt renderer):

```rust
// card.rs
/// The human-readable verb reference shown to the model each turn.
/// Static text (a &'static str) describing the five verbs and the
/// exactly-one-action rule, with one worked example per verb in the
/// SearchReplace codec. P3 renders it verbatim; P4 may swap the patch
/// example for the model's selected codec.
pub fn verb_card(patch_codec: crate::action::PatchCodec) -> String;
```

- [ ] **Step 1: Write the failing integration tests.**

```rust
use bloomery_core::action::{parse_action_with_codec, PatchCodec, Action, ActionError};
use bloomery_core::action::card::verb_card;

#[test]
fn every_verb_round_trips_from_a_realistic_turn() {
    // A turn with narration + one action, for each verb, asserting the parse.
    let read = "Let me look.\n<action verb=\"read\" path=\"src/lib.rs\" lines=\"1-40\">\n</action>";
    assert!(matches!(parse_action_with_codec(read, PatchCodec::SearchReplace).unwrap(),
        Action::Read { .. }));
    // ... find, run, done, and a whole-file patch under PatchCodec::WholeFile ...
    let done = "<action verb=\"done\">\nall tests pass\n</action>";
    assert!(matches!(parse_action_with_codec(done, PatchCodec::SearchReplace).unwrap(),
        Action::Done { .. }));
}

#[test]
fn the_verb_card_names_every_verb_and_the_one_action_rule() {
    let card = verb_card(PatchCodec::SearchReplace);
    for v in ["read", "find", "patch", "run", "done"] {
        assert!(card.contains(v), "card missing verb {v}");
    }
    assert!(card.to_lowercase().contains("one action"));
    assert!(card.contains("<<<<<<< SEARCH"));  // the SR example
    let wf = verb_card(PatchCodec::WholeFile);
    assert!(!wf.contains("<<<<<<< SEARCH"));    // whole-file card shows the other example
}

#[test]
fn multiple_actions_in_one_turn_is_a_single_named_error() {
    let turn = "<action verb=\"read\" path=\"a\">\n</action>\n<action verb=\"done\">\nx\n</action>";
    assert!(matches!(parse_action_with_codec(turn, PatchCodec::SearchReplace).unwrap_err(),
        ActionError::MultipleActions { found: 2 }));
}
```

- [ ] **Step 2: RED run.**

- [ ] **Step 3: Implement.** `card.rs`: `verb_card` builds the static reference — a heading, the exactly-one-action rule, and one worked `<action>` example per verb (the `patch` example uses the passed codec's shape). Fill the integration test's `// ...` with the find/run/whole-file cases (concrete, per Tasks 2–3 shapes). README: a ~10-line subsection listing the five verbs, "one action per turn, envelope-constrained, never grammar-forced", the two patch codecs + applies-and-parses lens, and a one-line pointer that P3 wires executors + grants and P4 gates codec landing (G4).

- [ ] **Step 4: GREEN + gates + whole-suite.** `cargo test --workspace`, fmt, clippy both configs.

- [ ] **Step 5: Commit.** `git commit -m "feat: action codec - verb card + round-trip integration, README"`

---

## Self-review (performed at plan-writing time)

- **Spec §3 coverage:** the verb table → Tasks 2–3; exactly-one-action + zero/multiple diagnostics → Task 1; typed repair diagnostics carrying expected shape → every `ActionError` variant across Tasks 1–3; patch codec per-model (both parsed, selection by param) → Task 3 (`parse_action_with_codec`); applies-and-parses landing lens named in every record with an `Unparsed`-never-false-lands guarantee → Task 4; the verb card the loop shows the model → Task 5. Python lens (subprocess) is explicitly deferred to P3 per the spec ("in P1 the lens is a trait with a plain-text impl") — not a gap.
- **Placeholder scan:** the integration test's `// ...` in Task 5 is filled in Step 3 with concrete cases named in the step text (find/run/whole-file per the Task 2/3 shapes) — not a TBD. No other placeholders.
- **Type consistency:** `ActionError` variants are declared in full in Task 1 and referenced (not redeclared) in Tasks 2–3; `PatchBody`/`PatchCodec` introduced in Task 3 and consumed by Task 4's `land`; `parse_action` (Task 2, SearchReplace default) and `parse_action_with_codec` (Task 3) coexist as specified; `Landing`/`LandingLens`/`PlainText` (Task 4) match between the lens tests and P3's stated consumer; `verb_card(PatchCodec)` (Task 5) matches its integration test. The Task 2 temporary patch stub (`BadCodec{detail:"patch not wired"}`) is explicitly replaced in Task 3 — a named handoff, not drift.
