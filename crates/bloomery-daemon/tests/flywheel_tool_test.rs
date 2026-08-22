//! `flywheel-tool` tests (flywheel task-1 brief): the anti-drift pin
//! (load-bearing), `transcript_entry`'s pinned format, and bin-level
//! trajectory requests against the actual built binary.
//!
//! Mirrors `task_loop_test.rs`'s fixture pattern (a real tempdir sandbox, a
//! real `Grant`, `Pager<FakeSubstrate>` with scripted `<action>` turns) —
//! this file cannot import that one's private helpers (a separate `tests/
//! *.rs` file is its own crate), so the small set this file needs is
//! restated here rather than shared.
//!
//! **Three sibling files carry the rest.** The first was split out of this
//! one when it reached the 800-line ceiling (task-6 brief, Step 1) — a pure
//! move, no test body changed: `flywheel_tool_refuse_test.rs` holds the
//! task-3 brief's refusal
//! trajectories (both families, plus the missing-target anti-drift pin),
//! and `flywheel_tool_find_test.rs` / `flywheel_tool_run_test.rs` hold the
//! task-6 brief's two turn-3 shapes. What stays here is turn-1's material:
//! the patch-mode anti-drift pin, `transcript_entry`'s format pins, the
//! bin-level patch-mode requests, and the turn-1 golden.

use bloomery_core::action::PatchCodec;
use bloomery_core::gguf::GgufMeta;
use bloomery_core::grant::Grant;
use bloomery_core::journal::Journal;
use bloomery_daemon::agents::ImageStore;
use bloomery_daemon::config::EnvelopeLens;
use bloomery_daemon::pager::Pager;
use bloomery_daemon::task::task_loop::{render_task_prompt, transcript_entry};
use bloomery_daemon::task::{run_task, ExecBounds, TaskSpec, TaskStatus};
use bloomery_substrate::fake::FakeSubstrate;
use bloomery_substrate::Reply;
use std::io::Write as _;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

// ---------------------------------------------------------------------------
// Shared fixture helpers (task_loop_test.rs's pattern, restated).
// ---------------------------------------------------------------------------

const GOAL: &str = "exercise the flywheel-tool anti-drift pin";
const SANDBOX_CONTENTS: &str = "hello\nworld\n";

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
        attention_layers: 4,
        kv_heads: 2,
        head_dim: 32,
        training_ctx: 65536,
        weights_bytes: 1000,
        recurrent_state_bytes: 0,
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

/// Builds `<dir>/sandbox` containing `file.txt` (`SANDBOX_CONTENTS`) and a
/// `Grant` scoped to exactly that directory, allowing exactly `commands` —
/// mirrors `task_loop_test.rs::sandbox`. `commands` is what envelope-v4
/// renders its grant line from, so the v4 pins below drive both the empty
/// (`none`) and the granted shape through the REAL grant the loop enforces.
fn sandbox_granting(dir: &std::path::Path, commands: &[Vec<String>]) -> (PathBuf, Grant) {
    let sb = dir.join("sandbox");
    std::fs::create_dir_all(&sb).unwrap();
    std::fs::write(sb.join("file.txt"), SANDBOX_CONTENTS).unwrap();
    let sb = std::fs::canonicalize(&sb).unwrap();
    let g = Grant::from_json(&format!(
        r#"{{"read_roots":["{s}"],"write_roots":["{s}"],"commands":{c}}}"#,
        s = sb.display(),
        c = serde_json::to_string(commands).unwrap()
    ))
    .unwrap();
    (sb, g)
}

/// The no-commands sandbox every pre-v4 test already used.
fn sandbox(dir: &std::path::Path) -> (PathBuf, Grant) {
    sandbox_granting(dir, &[])
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

fn spec(grant: Grant, cwd: PathBuf, envelope: EnvelopeLens) -> TaskSpec {
    TaskSpec {
        goal: GOAL.to_string(),
        grant,
        budget_tokens: 1_000_000,
        max_steps: 5,
        cwd,
        patch_codec: PatchCodec::SearchReplace,
        bounds: bounds(),
        mutating_verbs: true,
        envelope,
    }
}

// ---------------------------------------------------------------------------
// The anti-drift pin (load-bearing): a real `run_task` run's SECOND prompt
// (the one rendered after the read step's real observation) must
// byte-equal `render_task_prompt` + `transcript_entry`'s reconstruction of
// that same prompt from the same inputs.
// ---------------------------------------------------------------------------

/// Drives one real two-step task (`read` then `done`) against
/// `FakeSubstrate`, extracts the SECOND prompt the substrate actually
/// received (`FakeSubstrate::infer` joins successive prompts on one context
/// with a single `\n` — see that module's docs — so the second prompt is
/// everything in `ctx_history` after `prompt1.len()` bytes plus that one
/// separator byte), and asserts it byte-equals the tool-path
/// reconstruction: `render_task_prompt` fed `transcript_entry`'s output for
/// the same read step.
fn assert_second_prompt_matches_tool_path(envelope: EnvelopeLens, commands: &[Vec<String>]) {
    let dir = fresh_dir(&format!(
        "antidrift-{}-{}",
        envelope_tag(envelope),
        commands.len()
    ));
    let (sb, g) = sandbox_granting(&dir, commands);
    let (mut pager, agent_id) = fixture(
        &dir,
        vec![
            scripted("<action verb=\"read\" path=\"file.txt\">\n</action>"),
            scripted("<action verb=\"done\">\nread it\n</action>"),
        ],
    );
    let task_journal_path = dir.join("task.jsonl");
    let mut task_journal = Journal::open(&task_journal_path).unwrap();
    let spec = spec(g, sb, envelope);

    let result = run_task(&mut pager, &agent_id, &spec, &mut task_journal);
    assert_eq!(result.status, TaskStatus::Done, "{:?}", result.steps);
    assert_eq!(
        result.steps.len(),
        2,
        "expected [read, done]: {:?}",
        result.steps
    );

    let history = pager
        .substrate()
        .ctx_history(1)
        .expect("context 1 is resident after a 2-step task");

    // Independently compute prompt 1 (empty transcript) via the SAME
    // wrapper the bin uses, purely to locate where the real second prompt
    // starts inside the joined history — `render_prompt`/`render_task_prompt`
    // is what actually produced prompt 1 inside `run_task`, so this is not
    // a second implementation, just a landmark.
    let prompt1 = render_task_prompt(GOAL, PatchCodec::SearchReplace, envelope, commands, "");
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

    // Hand-written literal (NOT derived by calling `transcript_entry` —
    // that would make this assertion self-consistent under any mutation of
    // that function's format string, catching nothing) pinning the real
    // `record_step` output's exact bytes: the mutation check (Step 4 of the
    // task-1 brief — drop the `\n` before `content`) must break THIS
    // assertion, independent of whether `render_task_prompt`'s assembly
    // itself has drifted from `render_prompt`.
    let expected_transcript_literal = format!(
        "\n[step 1 read] read {} bytes\n{SANDBOX_CONTENTS}\n",
        SANDBOX_CONTENTS.len()
    );
    assert!(
        actual_second_prompt.contains(&expected_transcript_literal),
        "the real run_task transcript segment doesn't match the pinned literal format \
         — got: {actual_second_prompt:?}"
    );

    // The tool-path reconstruction: the same real read outcome string
    // (byte-parity with exec_read's `"read {n} bytes"`, task/exec.rs:~176)
    // folded through `transcript_entry`, then through `render_task_prompt`.
    // This half of the test catches a DIFFERENT bug class than the literal
    // check above: drift between `render_task_prompt`'s assembly and the
    // real `render_prompt`'s (wrong separator, wrong verb card, wrong
    // placement of the transcript), even if `transcript_entry`'s own format
    // were correct.
    let read_outcome = format!("read {} bytes", SANDBOX_CONTENTS.len());
    let transcript = transcript_entry(1, "read", &read_outcome, SANDBOX_CONTENTS);
    let computed_second_prompt = render_task_prompt(
        GOAL,
        PatchCodec::SearchReplace,
        envelope,
        commands,
        &transcript,
    );

    assert_eq!(
        actual_second_prompt, computed_second_prompt,
        "flywheel-tool's tool-path prompt has drifted from the real run_task prompt"
    );

    // Under envelope-v4 the prompt also carries the grant line, rendered
    // from `spec.grant` — the SAME grant `run_task` enforces. Pinned against
    // a literal (not against `grant_line`'s own output, which would agree
    // with any mutation of it), so deleting the render branch fails here
    // even though both sides of the equality above would lose the line
    // together.
    if envelope == EnvelopeLens::V4 {
        let expected_line = if commands.is_empty() {
            "Granted commands: none — run is not available in this task".to_string()
        } else {
            format!("Granted commands: {}", commands[0].join(" "))
        };
        assert!(
            actual_second_prompt.starts_with(&format!("{GOAL}\n\n{expected_line}\n\n")),
            "the real run_task v4 prompt must open with the goal then the grant line — got: \
             {actual_second_prompt:?}"
        );
    }
}

fn envelope_tag(envelope: EnvelopeLens) -> &'static str {
    match envelope {
        EnvelopeLens::V1 => "v1",
        EnvelopeLens::V2 => "v2",
        EnvelopeLens::V3 => "v3",
        EnvelopeLens::V4 => "v4",
    }
}

/// The turn-4 run slice's grant (spec §3), as a real `Grant`'s commands.
fn unittest_commands() -> Vec<Vec<String>> {
    vec![vec![
        "python3".to_string(),
        "-m".to_string(),
        "unittest".to_string(),
    ]]
}

#[test]
fn anti_drift_pin_matches_real_second_prompt_under_v1() {
    assert_second_prompt_matches_tool_path(EnvelopeLens::V1, &[]);
}

#[test]
fn anti_drift_pin_matches_real_second_prompt_under_v2() {
    assert_second_prompt_matches_tool_path(EnvelopeLens::V2, &[]);
}

#[test]
fn anti_drift_pin_matches_real_second_prompt_under_v3() {
    assert_second_prompt_matches_tool_path(EnvelopeLens::V3, &[]);
}

#[test]
fn anti_drift_pin_matches_real_second_prompt_under_v4_with_no_granted_command() {
    assert_second_prompt_matches_tool_path(EnvelopeLens::V4, &[]);
}

#[test]
fn anti_drift_pin_matches_real_second_prompt_under_v4_with_a_granted_command() {
    assert_second_prompt_matches_tool_path(EnvelopeLens::V4, &unittest_commands());
}

// ---------------------------------------------------------------------------
// `transcript_entry`'s pinned format string.
// ---------------------------------------------------------------------------

/// Pins `transcript_entry`'s exact format against a literal (task-1 brief:
/// `"\n[step {step} {verb}] {outcome}\n{content}\n"`, pinned today at
/// task_loop.rs:~155) — the mutation check (Step 4 of the brief) targets
/// this exact string.
#[test]
fn transcript_entry_matches_the_pinned_format_string() {
    let got = transcript_entry(3, "patch", "patched (lens: python)", "new file body\n");
    assert_eq!(
        got,
        "\n[step 3 patch] patched (lens: python)\nnew file body\n\n"
    );
}

/// `transcript_entry` is what `record_step` actually folds into the
/// transcript during a real task — proven here by driving a real one-step
/// `read` task and asserting the transcript embedded in the SECOND prompt
/// (everything `render_prompt` appends after the goal+verb-card preamble,
/// before any think-preseed) is exactly `transcript_entry`'s output for
/// that step's real verb/outcome/content. Complements the anti-drift pin
/// above (which proves the same fact indirectly, via the whole prompt);
/// this test isolates the transcript slice itself.
#[test]
fn record_step_folds_in_exactly_transcript_entrys_output() {
    let dir = fresh_dir("record-step-transcript");
    let (sb, g) = sandbox(&dir);
    let (mut pager, agent_id) = fixture(
        &dir,
        vec![
            scripted("<action verb=\"read\" path=\"file.txt\">\n</action>"),
            scripted("<action verb=\"done\">\nread it\n</action>"),
        ],
    );
    let task_journal_path = dir.join("task.jsonl");
    let mut task_journal = Journal::open(&task_journal_path).unwrap();
    let spec = spec(g, sb, EnvelopeLens::V1); // V1: no preseed suffix to strip

    let result = run_task(&mut pager, &agent_id, &spec, &mut task_journal);
    assert_eq!(result.status, TaskStatus::Done);

    let history = pager.substrate().ctx_history(1).unwrap();
    // V1 has no preseed, so with an empty transcript `prompt1` IS exactly
    // the goal+verb-card preamble `render_prompt` re-renders at the head of
    // every turn — `prompt2` (the second entry in `history`, after prompt1
    // plus FakeSubstrate's single `\n` join) is that SAME preamble with the
    // real transcript appended, not the transcript alone. Stripping the
    // preamble prefix (whose length equals `prompt1.len()`, since the
    // preamble itself never changes across steps) isolates exactly the
    // transcript slice `record_step` appended.
    let prompt1 = render_task_prompt(GOAL, PatchCodec::SearchReplace, EnvelopeLens::V1, &[], "");
    let prompt2 = &history[prompt1.len() + 1..]; // +1 for FakeSubstrate's '\n' join
    let real_transcript = &prompt2[prompt1.len()..];

    // Hand-written literal — NOT built by calling `transcript_entry` (see
    // the anti-drift pin's matching comment for why that would be
    // self-consistent and mutation-blind). This is `record_step`'s real
    // output, pinned against the brief's literal format directly.
    let expected = format!(
        "\n[step 1 read] read {} bytes\n{SANDBOX_CONTENTS}\n",
        SANDBOX_CONTENTS.len()
    );
    assert_eq!(real_transcript, expected);
}

// ---------------------------------------------------------------------------
// Bin-level: spawn the actual built `flywheel-tool` binary.
// ---------------------------------------------------------------------------

const PY_TARGET: &str = "stats.py";
const PY_CONTENTS: &str = "def average(xs):\n    total = 0\n    for x in xs:\n        total += x\n    return total / (len(xs) - 1)\n";
const PY_SEARCH: &str = "return total / (len(xs) - 1)";
const PY_REPLACE: &str = "return total / len(xs)";

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

fn trajectory_request(envelope: &str) -> serde_json::Value {
    serde_json::json!({
        "cmd": "trajectory",
        "goal": "fix the off-by-one in average()",
        "patch_codec": "search_replace",
        "envelope": envelope,
        "target": PY_TARGET,
        "target_contents": PY_CONTENTS,
        "search": PY_SEARCH,
        "replace": PY_REPLACE,
        "summary": "fixed average() denominator",
    })
}

#[test]
fn bin_trajectory_request_lands_with_three_pairs_and_the_exact_search_block() {
    let response = run_flywheel_tool(&trajectory_request("v3"));

    assert_eq!(response["landed"], serde_json::json!(true), "{response}");
    let pairs = response["pairs"].as_array().expect("pairs array");
    assert_eq!(pairs.len(), 3, "{response}");

    let completion2 = pairs[1]["completion"].as_str().unwrap();
    let expected_search_block =
        format!("<<<<<<< SEARCH\n{PY_SEARCH}\n=======\n{PY_REPLACE}\n>>>>>>> REPLACE");
    assert!(
        completion2.contains(&expected_search_block),
        "pair 2's completion is missing the exact SEARCH block: {completion2:?}"
    );

    let patched = response["patched_contents"]
        .as_str()
        .expect("patched_contents present");
    assert!(patched.contains(PY_REPLACE));
    assert!(!patched.contains(PY_SEARCH));
}

#[test]
fn bin_trajectory_prompts_end_with_think_preseed_under_v2_v3_v4_not_v1() {
    const PRESEED: &str = "<think>\n\n</think>\n\n";

    let v1 = run_flywheel_tool(&trajectory_request("v1"));
    let v2 = run_flywheel_tool(&trajectory_request("v2"));
    let v3 = run_flywheel_tool(&trajectory_request("v3"));
    let v4 = run_flywheel_tool(&trajectory_request("v4"));

    for (label, response) in [("v1", &v1), ("v2", &v2), ("v3", &v3), ("v4", &v4)] {
        let prompt0 = response["pairs"][0]["prompt"]
            .as_str()
            .unwrap_or_else(|| panic!("{label}: missing pairs[0].prompt in {response}"));
        let ends_with_preseed = prompt0.ends_with(PRESEED);
        let expected = label != "v1";
        assert_eq!(
            ends_with_preseed, expected,
            "{label}: prompt ends_with(THINK_PRESEED) = {ends_with_preseed}, expected {expected}"
        );
    }
}

/// The bin's v4 prompts carry the grant line rendered from the request's
/// own `commands` — pinned two ways: against the literal line (so deleting
/// the render branch fails here) and against `render_task_prompt` fed the
/// same commands (so a tool that rendered the line ITSELF, rather than
/// through the loop's renderer, fails too).
#[test]
fn bin_v4_patch_mode_prompts_carry_the_grant_line_from_the_requests_commands() {
    const GOAL_TEXT: &str = "fix the off-by-one in average()";

    let none = run_flywheel_tool(&trajectory_request("v4"));
    let prompt0 = none["pairs"][0]["prompt"]
        .as_str()
        .expect("pairs[0].prompt");
    assert!(
        prompt0.starts_with(&format!(
            "{GOAL_TEXT}\n\nGranted commands: none — run is not available in this task\n\n"
        )),
        "a request with no commands must render the none line: {prompt0:?}"
    );
    assert_eq!(
        prompt0,
        render_task_prompt(
            GOAL_TEXT,
            PatchCodec::SearchReplace,
            EnvelopeLens::V4,
            &[],
            ""
        ),
        "the bin's v4 prompt drifted from the loop's renderer"
    );

    let mut request = trajectory_request("v4");
    request
        .as_object_mut()
        .expect("request is an object")
        .insert(
            "commands".to_string(),
            serde_json::json!([["python3", "-m", "unittest"]]),
        );
    let granted = run_flywheel_tool(&request);
    let prompt0 = granted["pairs"][0]["prompt"]
        .as_str()
        .expect("pairs[0].prompt");
    assert!(
        prompt0.starts_with(&format!(
            "{GOAL_TEXT}\n\nGranted commands: python3 -m unittest\n\n"
        )),
        "a granted request must render the granted line: {prompt0:?}"
    );
    assert_eq!(
        prompt0,
        render_task_prompt(
            GOAL_TEXT,
            PatchCodec::SearchReplace,
            EnvelopeLens::V4,
            &unittest_commands(),
            ""
        ),
        "the bin's v4 prompt drifted from the loop's renderer"
    );
}

/// An unusable argv prefix is a named error for EVERY mode — including
/// patch mode, which builds no scratch grant and so never validated
/// `commands` before envelope-v4 started rendering them into the prompt.
#[test]
fn bin_trajectory_request_with_an_unusable_command_prefix_is_a_named_error() {
    for (label, commands) in [
        ("empty prefix", serde_json::json!([[]])),
        ("blank word", serde_json::json!([["python3", "  "]])),
    ] {
        let mut request = trajectory_request("v4");
        request
            .as_object_mut()
            .expect("request is an object")
            .insert("commands".to_string(), commands);

        let response = run_flywheel_tool(&request);
        let error = response["error"]
            .as_str()
            .unwrap_or_else(|| panic!("{label}: expected an error field, got {response}"));
        assert!(error.contains("commands[0]"), "{label}: {error}");
        assert!(
            response.get("pairs").is_none(),
            "{label}: a refused request renders no pairs: {response}"
        );
    }
}

#[test]
fn bin_trajectory_request_with_a_bad_search_yields_landed_false_with_detail() {
    let mut request = trajectory_request("v1");
    request["search"] = serde_json::json!("this text is not in the file at all");

    let response = run_flywheel_tool(&request);

    assert_eq!(response["landed"], serde_json::json!(false), "{response}");
    let detail = response["landing_detail"]
        .as_str()
        .expect("landing_detail present when landed: false");
    assert!(
        detail.contains("did not apply"),
        "expected a did-not-apply detail, got: {detail:?}"
    );
    assert!(
        response.get("patched_contents").is_none(),
        "patched_contents must be absent when landed: false, got {response}"
    );
}

#[test]
fn bin_patch_request_without_search_is_a_named_error() {
    let mut request = trajectory_request("v1");
    request
        .as_object_mut()
        .expect("request is an object")
        .remove("search");

    let response = run_flywheel_tool(&request);
    let error = response["error"]
        .as_str()
        .unwrap_or_else(|| panic!("expected an error field, got {response}"));
    assert!(error.contains("search"), "{error}");
}

// ---------------------------------------------------------------------------
// Regression: `expect` absent must be byte-identical to turn 1's stored
// expected output (task-3 brief) — `expect="refuse"` must be a fully
// additive change to the wire protocol, never a behavior change to the
// existing patch path.
// ---------------------------------------------------------------------------

#[test]
fn bin_patch_mode_response_is_byte_identical_to_the_turn1_golden() {
    let golden: serde_json::Value = serde_json::from_str(include_str!(
        "fixtures/flywheel_tool_turn1_patch_golden.json"
    ))
    .expect("golden fixture is valid JSON");

    let response = run_flywheel_tool(&trajectory_request("v3"));

    assert_eq!(
        response, golden,
        "flywheel-tool's patch-mode (expect absent) response has drifted from turn 1's stored \
         expected output, captured from the binary before task-3's refusal-trajectory changes \
         landed — expect=\"refuse\" must be fully additive"
    );
}
