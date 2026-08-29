//! `flywheel-tool` envelope-aware `done` completion tests (turn-7 spec
//! §2.3) — a sibling of `flywheel_tool_test.rs`, following the same
//! split-by-turn pattern as `flywheel_tool_refuse_test.rs` /
//! `flywheel_tool_find_test.rs` / `flywheel_tool_run_test.rs`.
//!
//! The contract under test: under a lens where `done_declares()` is false
//! (v1–v4), the `done` completion is the pinned `<action verb="done">` wrap,
//! byte-identical to what those envelopes have always rendered; under v5,
//! the wire `summary`/`refusal_reason` must already BE a full declared done
//! block — parsed back with the real `bloomery_core::action::parse_action`,
//! required to carry `outcome`, `reason`, and at least one `evidence:` line
//! — and is emitted VERBATIM. Any v5 ideal that fails that parse is a
//! factory bug: the tool answers with its `{"error": ...}` line (no pairs)
//! and the factory run aborts.
//!
//! Every test spawns the actual built binary (the bin-level technique
//! `flywheel_tool_test.rs` established); a separate `tests/*.rs` file is
//! its own crate, so the small runner helper is restated here rather than
//! shared, the same call every sibling file already makes.

use std::io::Write as _;
use std::process::{Command, Stdio};

/// Spawns the real `flywheel-tool` binary, sends one request line, and
/// parses the one response line it writes back — restated from
/// `flywheel_tool_test.rs`.
fn run_flywheel_tool(request: &serde_json::Value) -> serde_json::Value {
    let bin = env!("CARGO_BIN_EXE_flywheel-tool");
    let mut child = Command::new(bin)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("flywheel-tool spawns");
    {
        let stdin = child.stdin.as_mut().expect("stdin piped");
        writeln!(stdin, "{}", request).expect("write request line");
    }
    let output = child
        .wait_with_output()
        .expect("flywheel-tool runs to completion");
    assert!(
        output.status.success(),
        "flywheel-tool exited non-zero: {output:?}"
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout is utf-8");
    let line = stdout
        .lines()
        .next()
        .expect("flywheel-tool wrote exactly one response line");
    serde_json::from_str(line).expect("response line is valid JSON")
}

// ---------------------------------------------------------------------------
// Fixtures: the same patch task `flywheel_tool_test.rs` uses, plus the same
// refuse task `flywheel_tool_refuse_test.rs` uses — restated per the
// sibling-file convention.
// ---------------------------------------------------------------------------

const PY_TARGET: &str = "stats.py";
const PY_CONTENTS: &str = "def average(xs):\n    total = 0\n    for x in xs:\n        total += x\n    return total / (len(xs) - 1)\n";
const PY_SEARCH: &str = "return total / (len(xs) - 1)";
const PY_REPLACE: &str = "return total / len(xs)";
const PROSE_SUMMARY: &str = "fixed average() denominator";

/// A valid declared v5 patch-outcome done block: outcome + reason
/// attributes, one `evidence:` line quoting the post-patch bytes, then
/// prose. Written as a literal (never assembled by any tool-side function)
/// so the verbatim assertions below compare against independent bytes.
const DECLARED_PATCH_DONE: &str = "<action verb=\"done\" outcome=\"patched\" reason=\"fixed\">\nevidence: stats.py:5 `return total / len(xs)`\nDivided by len(xs) instead of len(xs) - 1.\n</action>";

const REFUSE_GOAL: &str = "report.txt claims the totals column is wrong; fix report.txt if so";
const REFUSE_TARGET: &str = "report.txt";
const REFUSE_TARGET_CONTENTS: &str = "totals: 2 + 2 = 4\n";

/// A valid declared v5 refusal done block for the defect-absent family.
const DECLARED_REFUSE_DONE: &str = "<action verb=\"done\" outcome=\"refused\" reason=\"no-defect\">\nevidence: report.txt:1 `totals: 2 + 2 = 4`\nThe totals line already sums correctly; no change made.\n</action>";

fn patch_request(envelope: &str, summary: &str) -> serde_json::Value {
    serde_json::json!({
        "cmd": "trajectory",
        "goal": "fix the off-by-one in average()",
        "patch_codec": "search_replace",
        "envelope": envelope,
        "target": PY_TARGET,
        "target_contents": PY_CONTENTS,
        "search": PY_SEARCH,
        "replace": PY_REPLACE,
        "summary": summary,
    })
}

fn refuse_request(envelope: &str, refusal_reason: &str) -> serde_json::Value {
    serde_json::json!({
        "cmd": "trajectory",
        "goal": REFUSE_GOAL,
        "patch_codec": "search_replace",
        "envelope": envelope,
        "target": REFUSE_TARGET,
        "target_contents": REFUSE_TARGET_CONTENTS,
        "expect": "refuse",
        "refusal_reason": refusal_reason,
    })
}

/// Asserts a request produced the tool's error line — no pairs rendered,
/// the run aborted — and returns the error text for message assertions.
fn assert_error(label: &str, request: &serde_json::Value) -> String {
    let response = run_flywheel_tool(request);
    let error = response["error"]
        .as_str()
        .unwrap_or_else(|| panic!("{label}: expected an error field, got {response}"))
        .to_string();
    assert!(
        response.get("pairs").is_none(),
        "{label}: an aborted request renders no pairs: {response}"
    );
    error
}

// ---------------------------------------------------------------------------
// v5 success paths: the declared block rides through VERBATIM.
// ---------------------------------------------------------------------------

#[test]
fn bin_v5_patch_mode_emits_the_declared_done_block_verbatim() {
    let response = run_flywheel_tool(&patch_request("v5", DECLARED_PATCH_DONE));

    assert_eq!(response["landed"], serde_json::json!(true), "{response}");
    let pairs = response["pairs"].as_array().expect("pairs array");
    assert_eq!(pairs.len(), 3, "{response}");
    assert_eq!(
        pairs[2]["completion"].as_str().unwrap(),
        DECLARED_PATCH_DONE,
        "the v5 done completion must be the wire summary verbatim — no wrap, no whitespace change"
    );
}

#[test]
fn bin_v5_refuse_mode_emits_the_declared_done_block_verbatim() {
    let response = run_flywheel_tool(&refuse_request("v5", DECLARED_REFUSE_DONE));

    assert_eq!(response["landed"], serde_json::json!(true), "{response}");
    assert_eq!(
        response["verified"],
        serde_json::json!("refusal"),
        "{response}"
    );
    let pairs = response["pairs"].as_array().expect("pairs array");
    assert_eq!(pairs.len(), 2, "{response}");
    assert_eq!(
        pairs[1]["completion"].as_str().unwrap(),
        DECLARED_REFUSE_DONE,
        "the v5 refusal completion must be the wire refusal_reason verbatim"
    );
}

// ---------------------------------------------------------------------------
// v5 failure paths: each missed requirement is its own named error. One
// test per requirement, so deleting any single check in `done_completion`
// is caught by exactly the test that pins it.
// ---------------------------------------------------------------------------

#[test]
fn bin_v5_bare_prose_summary_is_a_named_error() {
    let error = assert_error("patch", &patch_request("v5", PROSE_SUMMARY));
    assert!(
        error.contains("declared"),
        "the error must name the declaration requirement: {error}"
    );

    let error = assert_error("refuse", &refuse_request("v5", PROSE_SUMMARY));
    assert!(
        error.contains("declared"),
        "the error must name the declaration requirement: {error}"
    );
}

#[test]
fn bin_v5_done_block_without_outcome_is_a_named_error() {
    // Parses as Action::Done (evidence line and all) but declares nothing.
    let undeclared = "<action verb=\"done\">\nevidence: stats.py:5 `return total / len(xs)`\nDivided by len(xs) instead of len(xs) - 1.\n</action>";
    let error = assert_error("no-outcome", &patch_request("v5", undeclared));
    assert!(
        error.contains("outcome"),
        "the error must name the missing outcome attribute: {error}"
    );
}

#[test]
fn bin_v5_done_block_without_reason_is_a_named_error() {
    let no_reason = "<action verb=\"done\" outcome=\"patched\">\nevidence: stats.py:5 `return total / len(xs)`\nDivided by len(xs) instead of len(xs) - 1.\n</action>";
    let error = assert_error("no-reason", &patch_request("v5", no_reason));
    assert!(
        error.contains("reason"),
        "the error must name the missing reason attribute: {error}"
    );
}

#[test]
fn bin_v5_done_block_without_evidence_lines_is_a_named_error() {
    let no_evidence = "<action verb=\"done\" outcome=\"patched\" reason=\"fixed\">\nDivided by len(xs) instead of len(xs) - 1.\n</action>";
    let error = assert_error("no-evidence", &patch_request("v5", no_evidence));
    assert!(
        error.contains("evidence"),
        "the error must name the missing evidence lines: {error}"
    );
}

// ---------------------------------------------------------------------------
// v4 through the same path: the wrap, byte-identical — pinned against a
// hand-written literal (never `done_completion`'s own output), so making
// the wrap envelope-aware cannot have changed a non-declaring lens's bytes.
// ---------------------------------------------------------------------------

#[test]
fn bin_v4_done_completion_stays_the_wrapped_summary_byte_identical() {
    let response = run_flywheel_tool(&patch_request("v4", PROSE_SUMMARY));
    let pairs = response["pairs"].as_array().expect("pairs array");
    assert_eq!(pairs.len(), 3, "{response}");
    assert_eq!(
        pairs[2]["completion"].as_str().unwrap(),
        format!("<action verb=\"done\">\n{PROSE_SUMMARY}\n</action>"),
        "a non-declaring lens must keep today's wrap byte-identical"
    );

    let refusal = "No change needed: the totals column already sums correctly.";
    let response = run_flywheel_tool(&refuse_request("v4", refusal));
    let pairs = response["pairs"].as_array().expect("pairs array");
    assert_eq!(pairs.len(), 2, "{response}");
    assert_eq!(
        pairs[1]["completion"].as_str().unwrap(),
        format!("<action verb=\"done\">\n{refusal}\n</action>"),
        "a non-declaring lens must keep today's wrap byte-identical in refuse mode too"
    );
}
