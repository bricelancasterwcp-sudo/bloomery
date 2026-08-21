//! `flywheel-tool` refusal-trajectory tests (flywheel task-3 brief, G5
//! design doc §5) — split out of `flywheel_tool_test.rs` (which kept the
//! turn-1 patch-mode pins, the `transcript_entry` format pins, and the
//! turn-1 golden) when that file reached its 800-line ceiling. Nothing here
//! changed in the move: every test body below is byte-identical to the one
//! that file carried.
//!
//! Covers both refusal families the task-3 brief names — **defect-absent**
//! (the target exists; its real contents ride pair 2's transcript) and
//! **missing-target** (the target does not exist; pair 2's transcript comes
//! from a REAL failed [`exec_read`]) — at both the wrapper level (the
//! missing-target anti-drift pin, which never invokes the bin) and the bin
//! level (spawning the actual built binary and cross-checking its output
//! against a directly-called real `exec_read`).
//!
//! Mirrors `task_loop_test.rs`'s fixture pattern (a real tempdir sandbox, a
//! real `Grant`, `Pager<FakeSubstrate>` with scripted `<action>` turns) —
//! this file cannot import that one's private helpers (a separate `tests/
//! *.rs` file is its own crate), so the small set this file needs is
//! restated here rather than shared, the same call `flywheel_tool_test.rs`
//! already makes.

use bloomery_core::action::PatchCodec;
use bloomery_core::gguf::GgufMeta;
use bloomery_core::grant::Grant;
use bloomery_core::journal::Journal;
use bloomery_daemon::agents::ImageStore;
use bloomery_daemon::config::EnvelopeLens;
use bloomery_daemon::pager::Pager;
use bloomery_daemon::task::task_loop::{render_task_prompt, transcript_entry};
use bloomery_daemon::task::{exec_read, run_task, ExecBounds, TaskSpec, TaskStatus};
use bloomery_substrate::fake::FakeSubstrate;
use bloomery_substrate::Reply;
use std::io::Write as _;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

// ---------------------------------------------------------------------------
// Shared fixture helpers (task_loop_test.rs's pattern, restated).
// ---------------------------------------------------------------------------

fn bounds() -> ExecBounds {
    ExecBounds {
        read_cap_bytes: 256 * 1024,
        find_result_cap: 100,
        run_output_cap_bytes: 64 * 1024,
        run_timeout_secs: 120,
    }
}

fn meta() -> GgufMeta {
    GgufMeta {
        arch: "qwen2".into(),
        layers: 4,
        kv_heads: 2,
        head_dim: 32,
        training_ctx: 65536,
        weights_bytes: 1000,
    }
}

fn scripted(text: &str) -> Reply {
    Reply {
        text: text.to_string(),
        prompt_tokens: Some(8),
        completion_tokens: Some(4),
        duration_ms: 1,
    }
}

fn fresh_dir(tag: &str) -> PathBuf {
    static UNIQUE: AtomicU64 = AtomicU64::new(0);
    let unique = UNIQUE.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "bloomery-flywheeltool-{tag}-{}-{unique}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn fixture(dir: &std::path::Path, replies: Vec<Reply>) -> (Pager<FakeSubstrate>, String) {
    let journal = Journal::open(&dir.join("pager.jsonl")).unwrap();
    let images = ImageStore::new(&dir.join("img")).unwrap();
    let mut fake = FakeSubstrate::new();
    for r in replies {
        fake.script_reply(r);
    }
    let mut pager = Pager::new(fake, journal, images, Box::new(|| Some(1024 * 1024 * 1024)));
    let gguf = dir.join("m.gguf");
    std::fs::write(&gguf, b"fake weights").unwrap();
    pager.register_model("m", &gguf, meta(), None).unwrap();
    let info = pager.create_agent("m", 100, None, 1_000_000).unwrap();
    (pager, info.id)
}

fn envelope_tag(envelope: EnvelopeLens) -> &'static str {
    match envelope {
        EnvelopeLens::V1 => "v1",
        EnvelopeLens::V2 => "v2",
        EnvelopeLens::V3 => "v3",
        EnvelopeLens::V4 => "v4",
    }
}

// ---------------------------------------------------------------------------
// The missing-target anti-drift pin (task-3 brief, G5 design doc §4/§5):
// same pattern as the patch-mode pin above, but for a fixture dir that does
// NOT contain the read target. A real `run_task`'s second prompt (after the
// read step's REAL failed observation) must byte-equal `render_task_prompt`
// + `transcript_entry` folded around a directly-called, REAL `exec_read`
// failure — never a hand-transcribed literal, because the exact NotFound
// wording is OS/errno-sourced text (`exec.rs`'s `open_nofollow_read` /
// `exec_read`) this test must discover, not restate. `flywheel-tool`'s own
// implementation is expected to use this exact same real-exec_read
// technique for the missing-target family (see the bin-level cross-check
// test further below, which invokes the actual binary).
// ---------------------------------------------------------------------------

const MISSING_TARGET_GOAL: &str = "refuse: notes.txt is not in this workspace";
const MISSING_TARGET_NAME: &str = "notes.txt";
const MISSING_TARGET_SIBLING_CONTENTS: &str = "the only file actually here\n";

/// Drives one real two-step task (`read` a target that is NOT in the
/// fixture dir, then `done`) against `FakeSubstrate`, extracts the second
/// prompt the substrate actually received (same `ctx_history` technique as
/// [`assert_second_prompt_matches_tool_path`]), and asserts it byte-equals
/// a from-scratch reconstruction built by calling the REAL [`exec_read`]
/// against a separate, empty scratch dir (mirroring how `flywheel-tool`'s
/// own missing-target handling must work: its wire request carries only the
/// target's name, never the whole fixture dir, so it always builds its own
/// throwaway scratch dir per request — same shape, deliberately not the
/// same directory instance, to prove the observation text does not depend
/// on incidental path bytes).
fn assert_missing_target_second_prompt_matches_tool_path(envelope: EnvelopeLens) {
    let dir = fresh_dir(&format!("antidrift-missing-{}", envelope_tag(envelope)));
    let sb = dir.join("sandbox");
    std::fs::create_dir_all(&sb).unwrap();
    // A sibling file keeps the fixture dir non-empty (design spec §5's
    // missing-target shape) — `MISSING_TARGET_NAME` itself is never written
    // here, on purpose.
    std::fs::write(sb.join("sibling.txt"), MISSING_TARGET_SIBLING_CONTENTS).unwrap();
    let sb = std::fs::canonicalize(&sb).unwrap();
    let g = Grant::from_json(&format!(
        r#"{{"read_roots":["{s}"],"write_roots":["{s}"],"commands":[]}}"#,
        s = sb.display()
    ))
    .unwrap();

    let (mut pager, agent_id) = fixture(
        &dir,
        vec![
            scripted(&format!(
                "<action verb=\"read\" path=\"{MISSING_TARGET_NAME}\">\n</action>"
            )),
            scripted(
                "<action verb=\"done\">\nCannot: notes.txt does not exist in this workspace\n</action>",
            ),
        ],
    );
    let task_journal_path = dir.join("task.jsonl");
    let mut task_journal = Journal::open(&task_journal_path).unwrap();
    let task_spec = TaskSpec {
        goal: MISSING_TARGET_GOAL.to_string(),
        grant: g,
        budget_tokens: 1_000_000,
        max_steps: 5,
        cwd: sb,
        patch_codec: PatchCodec::SearchReplace,
        bounds: bounds(),
        mutating_verbs: true,
        envelope,
    };

    let result = run_task(&mut pager, &agent_id, &task_spec, &mut task_journal);
    assert_eq!(result.status, TaskStatus::Done, "{:?}", result.steps);
    assert_eq!(
        result.steps.len(),
        2,
        "expected [read, done]: {:?}",
        result.steps
    );
    assert!(
        result.steps[0].failed,
        "the read of a missing target must be recorded as a failed step: {:?}",
        result.steps[0]
    );

    let history = pager
        .substrate()
        .ctx_history(1)
        .expect("context 1 is resident after a 2-step task");

    let prompt1 = render_task_prompt(
        MISSING_TARGET_GOAL,
        PatchCodec::SearchReplace,
        envelope,
        &[],
        "",
    );
    assert!(
        history.starts_with(&prompt1),
        "prompt 1 landmark not found at the start of ctx_history — history: {history:?}"
    );
    let after_prompt1 = &history[prompt1.len()..];
    assert!(
        after_prompt1.starts_with('\n'),
        "FakeSubstrate joins successive prompts with a single '\\n'"
    );
    let actual_second_prompt = &after_prompt1[1..];

    // The tool-path reconstruction — a SEPARATE, empty scratch dir fed
    // through the REAL `exec_read`, never a hand-typed NotFound string.
    let scratch = fresh_dir(&format!(
        "antidrift-missing-scratch-{}",
        envelope_tag(envelope)
    ));
    let scratch_grant = Grant::from_json(&format!(
        r#"{{"read_roots":["{s}"],"write_roots":[],"commands":[]}}"#,
        s = scratch.display()
    ))
    .unwrap();
    let observation = exec_read(
        &scratch_grant,
        &scratch,
        MISSING_TARGET_NAME,
        None,
        &bounds(),
    );
    assert!(
        observation.failed,
        "the scratch dir must not contain the target — got a successful read: {observation:?}"
    );
    let transcript = transcript_entry(1, "read", &observation.outcome, &observation.content);
    let computed_second_prompt = render_task_prompt(
        MISSING_TARGET_GOAL,
        PatchCodec::SearchReplace,
        envelope,
        &[],
        &transcript,
    );

    assert_eq!(
        actual_second_prompt, computed_second_prompt,
        "flywheel-tool's missing-target tool-path prompt has drifted from the real run_task prompt"
    );

    // A refusal task grants no command, so envelope-v4 renders the `none`
    // line — pinned against the literal, since the equality above shares one
    // renderer on both sides and would survive deleting the render branch.
    if envelope == EnvelopeLens::V4 {
        assert!(
            actual_second_prompt.starts_with(&format!(
                "{MISSING_TARGET_GOAL}\n\nGranted commands: none — run is not available in this \
                 task\n\n"
            )),
            "the real run_task v4 refusal prompt must open with the goal then the none line — \
             got: {actual_second_prompt:?}"
        );
    }
}

#[test]
fn missing_target_anti_drift_pin_matches_real_second_prompt_under_v1() {
    assert_missing_target_second_prompt_matches_tool_path(EnvelopeLens::V1);
}

#[test]
fn missing_target_anti_drift_pin_matches_real_second_prompt_under_v2() {
    assert_missing_target_second_prompt_matches_tool_path(EnvelopeLens::V2);
}

#[test]
fn missing_target_anti_drift_pin_matches_real_second_prompt_under_v3() {
    assert_missing_target_second_prompt_matches_tool_path(EnvelopeLens::V3);
}

#[test]
fn missing_target_anti_drift_pin_matches_real_second_prompt_under_v4() {
    assert_missing_target_second_prompt_matches_tool_path(EnvelopeLens::V4);
}
// ---------------------------------------------------------------------------
// Bin-level: spawn the actual built `flywheel-tool` binary.
// ---------------------------------------------------------------------------

/// Spawns the real `flywheel-tool` binary (`env!("CARGO_BIN_EXE_flywheel-tool")`
/// — set by cargo because `Cargo.toml` declares an explicit `[[bin]] name =
/// "flywheel-tool"` target), sends one request line, and parses the one
/// response line it writes back.
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
// Refusal trajectories (task-3 brief, G5 design doc §5's two families).
// ---------------------------------------------------------------------------

const REFUSE_GOAL_DEFECT_ABSENT: &str =
    "report.txt claims the totals column is wrong; fix report.txt if so";
const REFUSE_TARGET: &str = "report.txt";
const REFUSE_TARGET_CONTENTS: &str = "totals: 2 + 2 = 4\n";
const REFUSAL_REASON: &str = "No change needed: the totals column already sums correctly.";

fn refuse_defect_absent_request(envelope: &str) -> serde_json::Value {
    serde_json::json!({
        "cmd": "trajectory",
        "goal": REFUSE_GOAL_DEFECT_ABSENT,
        "patch_codec": "search_replace",
        "envelope": envelope,
        "target": REFUSE_TARGET,
        "target_contents": REFUSE_TARGET_CONTENTS,
        "expect": "refuse",
        "refusal_reason": REFUSAL_REASON,
    })
}

/// The defect-absent family (target exists): 2 pairs (read, done), the
/// `done` completion is `refusal_reason` verbatim, pair 2's transcript
/// carries the real target contents (same uncapped `"read {n} bytes"`
/// byte-parity convention pair 2 already used in patch mode), and the
/// response is self-consistently `landed: true` with `verified: "refusal"`
/// — never `patched_contents`/`landing_detail` (no patch was ever
/// attempted).
#[test]
fn bin_refuse_defect_absent_request_yields_two_pairs_verified_refusal() {
    let response = run_flywheel_tool(&refuse_defect_absent_request("v3"));

    assert_eq!(response["landed"], serde_json::json!(true), "{response}");
    assert_eq!(
        response["verified"],
        serde_json::json!("refusal"),
        "{response}"
    );
    assert!(
        response.get("patched_contents").is_none(),
        "patched_contents must be absent for a refusal, got {response}"
    );
    assert!(
        response.get("landing_detail").is_none(),
        "landing_detail must be absent for a refusal, got {response}"
    );

    let pairs = response["pairs"].as_array().expect("pairs array");
    assert_eq!(pairs.len(), 2, "{response}");

    let completion1 = pairs[0]["completion"].as_str().unwrap();
    assert_eq!(
        completion1,
        format!("<action verb=\"read\" path=\"{REFUSE_TARGET}\">\n</action>")
    );

    let completion2 = pairs[1]["completion"].as_str().unwrap();
    assert_eq!(
        completion2,
        format!("<action verb=\"done\">\n{REFUSAL_REASON}\n</action>")
    );

    let prompt2 = pairs[1]["prompt"].as_str().unwrap();
    let expected_transcript = format!(
        "\n[step 1 read] read {} bytes\n{REFUSE_TARGET_CONTENTS}\n",
        REFUSE_TARGET_CONTENTS.len()
    );
    assert!(
        prompt2.contains(&expected_transcript),
        "pair 2's prompt is missing the real read transcript: {prompt2:?}"
    );
}

const MISSING_TARGET_REFUSE_GOAL: &str = "fix the typo in notes.txt";
const MISSING_TARGET_REFUSE_TARGET: &str = "notes.txt";
const MISSING_TARGET_REFUSAL_REASON: &str = "Cannot: notes.txt does not exist in this workspace";

fn refuse_missing_target_request(envelope: &str) -> serde_json::Value {
    serde_json::json!({
        "cmd": "trajectory",
        "goal": MISSING_TARGET_REFUSE_GOAL,
        "patch_codec": "search_replace",
        "envelope": envelope,
        "target": MISSING_TARGET_REFUSE_TARGET,
        "target_contents": "",
        "expect": "refuse",
        "refusal_reason": MISSING_TARGET_REFUSAL_REASON,
        "target_missing": true,
    })
}

/// The missing-target family: 2 pairs (read, done), same self-consistent
/// `landed: true` / `verified: "refusal"` shape as the defect-absent case,
/// but pair 2's prompt must carry the REAL failed-read observation. Proven
/// here by cross-checking the actual binary's output against a
/// directly-called real [`exec_read`] against a fresh, empty scratch dir —
/// this is the strongest check available, because unlike the wrapper-level
/// anti-drift pin (which never invokes the bin), this test proves
/// `flywheel-tool`'s OWN implementation used the real observation, not a
/// hand-formatted string that happens to look right.
#[test]
fn bin_refuse_missing_target_request_uses_the_real_failed_read_observation() {
    let response = run_flywheel_tool(&refuse_missing_target_request("v3"));

    assert_eq!(response["landed"], serde_json::json!(true), "{response}");
    assert_eq!(
        response["verified"],
        serde_json::json!("refusal"),
        "{response}"
    );
    assert!(
        response.get("patched_contents").is_none(),
        "patched_contents must be absent for a refusal, got {response}"
    );
    assert!(
        response.get("landing_detail").is_none(),
        "landing_detail must be absent for a refusal, got {response}"
    );

    let pairs = response["pairs"].as_array().expect("pairs array");
    assert_eq!(pairs.len(), 2, "{response}");

    let completion1 = pairs[0]["completion"].as_str().unwrap();
    assert_eq!(
        completion1,
        format!("<action verb=\"read\" path=\"{MISSING_TARGET_REFUSE_TARGET}\">\n</action>")
    );
    let completion2 = pairs[1]["completion"].as_str().unwrap();
    assert_eq!(
        completion2,
        format!("<action verb=\"done\">\n{MISSING_TARGET_REFUSAL_REASON}\n</action>")
    );

    let dir = fresh_dir("bin-missing-target-crosscheck");
    let grant = Grant::from_json(&format!(
        r#"{{"read_roots":["{d}"],"write_roots":[],"commands":[]}}"#,
        d = dir.display()
    ))
    .unwrap();
    let observation = exec_read(&grant, &dir, MISSING_TARGET_REFUSE_TARGET, None, &bounds());
    assert!(
        observation.failed,
        "the crosscheck scratch dir must not contain the target: {observation:?}"
    );
    let transcript = transcript_entry(1, "read", &observation.outcome, &observation.content);
    let expected_prompt2 = render_task_prompt(
        MISSING_TARGET_REFUSE_GOAL,
        PatchCodec::SearchReplace,
        EnvelopeLens::V3,
        &[],
        &transcript,
    );

    let actual_prompt2 = pairs[1]["prompt"].as_str().unwrap();
    assert_eq!(
        actual_prompt2, expected_prompt2,
        "the bin's missing-target pair 2 prompt drifted from a direct real exec_read call"
    );
}

/// The bin's refusal-family v4 prompts carry the `none` grant line: a
/// refusal task grants no command, and under envelope-v4 that is stated in
/// the prompt rather than left for the model to infer (spec §2).
#[test]
fn bin_refuse_v4_prompts_carry_the_none_grant_line() {
    let response = run_flywheel_tool(&refuse_defect_absent_request("v4"));
    let prompt0 = response["pairs"][0]["prompt"]
        .as_str()
        .unwrap_or_else(|| panic!("missing pairs[0].prompt in {response}"));
    assert!(
        prompt0.starts_with(&format!(
            "{REFUSE_GOAL_DEFECT_ABSENT}\n\nGranted commands: none — run is not available in \
             this task\n\n"
        )),
        "a refusal request grants no command: {prompt0:?}"
    );
    assert_eq!(
        prompt0,
        render_task_prompt(
            REFUSE_GOAL_DEFECT_ABSENT,
            PatchCodec::SearchReplace,
            EnvelopeLens::V4,
            &[],
            ""
        ),
        "the bin's v4 refusal prompt drifted from the loop's renderer"
    );
}

#[test]
fn bin_refuse_request_without_refusal_reason_is_a_named_error() {
    let mut request = refuse_defect_absent_request("v1");
    request
        .as_object_mut()
        .expect("request is an object")
        .remove("refusal_reason");

    let response = run_flywheel_tool(&request);
    let error = response["error"]
        .as_str()
        .unwrap_or_else(|| panic!("expected an error field, got {response}"));
    assert!(error.contains("refusal_reason"), "{error}");
}
