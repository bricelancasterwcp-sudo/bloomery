//! `flywheel-tool` run-verified trajectory tests (flywheel turn-3, task-6
//! brief; design doc §2's "find/run enter through repair ideals") — one of
//! four files in the `flywheel_tool_*_test.rs` family, alongside
//! `flywheel_tool_test.rs` (turn-1 patch mode),
//! `flywheel_tool_refuse_test.rs` (turn-2 refusals), and
//! `flywheel_tool_find_test.rs` (turn-3's find-shaped shape).
//!
//! **The run-verified trajectory** (`run_argv` set on the wire): 4 pairs,
//! `read` -> `patch` -> `run` -> `done`. The `run` observation comes from a
//! REAL [`exec_run`] of the request's `run_argv` against the PATCHED file,
//! under a grant carrying the request's `commands`. A non-zero exit is a
//! hard error response, never a rendered trajectory — an ideal whose own
//! verification fails is not an ideal, so the factory must abort that task
//! as structural rather than train on it.
//!
//! The check script this file verifies with is asymmetric on purpose
//! (see `CHECK_SCRIPT_CONTENTS`): it fails against the unpatched file and
//! passes against the patched one, so an observed `exit 0` is itself proof
//! the binary ran the verification against the patched file.
//!
//! Mirrors `task_loop_test.rs`'s fixture pattern (a real tempdir sandbox, a
//! real `Grant`, `Pager<FakeSubstrate>` with scripted `<action>` turns) —
//! a separate `tests/*.rs` file is its own crate and cannot import a
//! sibling's helpers, so the small set this file needs is restated here,
//! the same call every sibling in this family already documents making.

use bloomery_core::action::PatchCodec;
use bloomery_core::gguf::GgufMeta;
use bloomery_core::grant::Grant;
use bloomery_core::journal::Journal;
use bloomery_daemon::agents::ImageStore;
use bloomery_daemon::config::EnvelopeLens;
use bloomery_daemon::pager::Pager;
use bloomery_daemon::task::task_loop::{render_task_prompt, transcript_entry};
use bloomery_daemon::task::{exec_run, run_task, ExecBounds, Observation, TaskSpec, TaskStatus};
use bloomery_substrate::fake::FakeSubstrate;
use bloomery_substrate::Reply;
use std::io::Write as _;
use std::path::{Path, PathBuf};
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
        "bloomery-flywheelverbs-{tag}-{}-{unique}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn fixture(dir: &Path, replies: Vec<Reply>) -> (Pager<FakeSubstrate>, String) {
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

/// Builds a `Grant` over exactly `dir` with exactly `commands` granted —
/// the same shape the binary builds for its own scratch dir, so a test's
/// direct `exec_find`/`exec_run` call runs under an equivalent capability
/// boundary to the one the binary used.
fn grant_for(dir: &Path, commands: &[&[&str]]) -> Grant {
    let wire = serde_json::json!({
        "read_roots": [dir],
        "write_roots": [],
        "commands": commands,
    });
    Grant::from_json(&wire.to_string()).expect("scratch grant is valid")
}

/// Writes a `files` wire array (the exact JSON the request carries) into
/// `dir` — the test-side mirror of what the binary does with `files`.
fn materialize(dir: &Path, files: &serde_json::Value) {
    for f in files.as_array().expect("files is an array") {
        let path = dir.join(f["path"].as_str().expect("file path is a string"));
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&path, f["contents"].as_str().expect("contents is a string")).unwrap();
    }
}

/// Spawns the real `flywheel-tool` binary, sends one request line, and
/// parses the one response line it writes back (verbatim from
/// `flywheel_tool_test.rs`).
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

/// Extracts a response's `error` string, panicking with the whole response
/// when the binary rendered a trajectory instead.
fn error_of(response: &serde_json::Value) -> String {
    response["error"]
        .as_str()
        .unwrap_or_else(|| panic!("expected an error field, got {response}"))
        .to_string()
}

// ---------------------------------------------------------------------------
// The task fixture: one buggy `average()` plus the script that checks it.
// ---------------------------------------------------------------------------

const TARGET: &str = "stats.py";
const TARGET_CONTENTS: &str = "def average(xs):\n    total = 0\n    for x in xs:\n        total += x\n    return total / (len(xs) - 1)\n";
const SEARCH: &str = "return total / (len(xs) - 1)";
const REPLACE: &str = "return total / len(xs)";
const SUMMARY: &str = "fixed the average() denominator";

const RUN_GOAL: &str = "average() divides by one too few; fix it and prove it with check.py";
const CHECK_SCRIPT: &str = "check.py";
/// Passes only against the PATCHED `stats.py`: unpatched, `average([1,2,3])`
/// is `6/2 == 3.0` and the assert fails (exit 1); patched it is `6/3 == 2.0`
/// and the script exits 0. That asymmetry is what makes the run a real
/// verification rather than a decorative step — and what lets the tests
/// below prove the binary ran it against the patched file.
const CHECK_SCRIPT_CONTENTS: &str =
    "from stats import average\n\nassert average([1, 2, 3]) == 2, average([1, 2, 3])\nprint(\"average() checks out\")\n";
const PYTHON: &str = "python3";

fn run_files() -> serde_json::Value {
    serde_json::json!([
        {"path": TARGET, "contents": TARGET_CONTENTS},
        {"path": CHECK_SCRIPT, "contents": CHECK_SCRIPT_CONTENTS},
    ])
}

fn run_request(envelope: &str) -> serde_json::Value {
    serde_json::json!({
        "cmd": "trajectory",
        "goal": RUN_GOAL,
        "patch_codec": "search_replace",
        "envelope": envelope,
        "target": TARGET,
        "target_contents": TARGET_CONTENTS,
        "search": SEARCH,
        "replace": REPLACE,
        "summary": SUMMARY,
        "files": run_files(),
        "run_argv": [PYTHON, CHECK_SCRIPT],
        "commands": [[PYTHON]],
    })
}

/// The preamble every V1 prompt opens with — `render_task_prompt` with an
/// empty transcript. Under V1 (and only V1) there is no think-preseed
/// suffix, so a later prompt is exactly this preamble followed by the
/// accumulated transcript; stripping it isolates the transcript slice
/// (`flywheel_tool_test.rs::record_step_folds_in_exactly_transcript_entrys_output`
/// uses the same technique).
fn v1_transcript_of(goal: &str, prompt: &str) -> String {
    let preamble = render_task_prompt(goal, PatchCodec::SearchReplace, EnvelopeLens::V1, &[], "");
    prompt
        .strip_prefix(&preamble)
        .unwrap_or_else(|| panic!("prompt is not the V1 preamble plus a transcript: {prompt:?}"))
        .to_string()
}

// ---------------------------------------------------------------------------
// run-verified trajectories.
// ---------------------------------------------------------------------------

#[test]
fn bin_run_verified_request_renders_four_pairs_as_read_patch_run_done() {
    let response = run_flywheel_tool(&run_request("v3"));

    assert_eq!(response["landed"], serde_json::json!(true), "{response}");
    assert!(
        response.get("verified").is_none(),
        "a patch-mode response never gains `verified`, got {response}"
    );
    let pairs = response["pairs"].as_array().expect("pairs array");
    assert_eq!(pairs.len(), 4, "{response}");

    assert_eq!(
        pairs[0]["completion"].as_str().unwrap(),
        format!("<action verb=\"read\" path=\"{TARGET}\">\n</action>")
    );
    let completion2 = pairs[1]["completion"].as_str().unwrap();
    assert!(
        completion2.contains(&format!(
            "<<<<<<< SEARCH\n{SEARCH}\n=======\n{REPLACE}\n>>>>>>> REPLACE"
        )),
        "pair 2's completion is missing the exact SEARCH block: {completion2:?}"
    );
    // Pair 3's completion is the verb card's `run` grammar exactly: no
    // attributes, and a JSON array of argv strings as the body.
    assert_eq!(
        pairs[2]["completion"].as_str().unwrap(),
        format!("<action verb=\"run\">\n[\"{PYTHON}\",\"{CHECK_SCRIPT}\"]\n</action>")
    );
    assert_eq!(
        pairs[3]["completion"].as_str().unwrap(),
        format!("<action verb=\"done\">\n{SUMMARY}\n</action>")
    );

    let patched = response["patched_contents"]
        .as_str()
        .expect("patched_contents present");
    assert!(patched.contains(REPLACE));
    assert!(!patched.contains(SEARCH));
}

/// The binary's run observation, cross-checked against a directly-called
/// real [`exec_run`] — and, more importantly, against the SAME command run
/// against the UNPATCHED file, which must fail. That second half is what
/// proves the binary executed the verification against the patched file
/// rather than against the file the request shipped.
#[test]
fn bin_run_verified_pair_4_prompt_carries_a_real_run_of_the_patched_file() {
    let response = run_flywheel_tool(&run_request("v1"));
    let pairs = response["pairs"].as_array().expect("pairs array");
    let transcript = v1_transcript_of(RUN_GOAL, pairs[3]["prompt"].as_str().unwrap());
    let patched = response["patched_contents"].as_str().unwrap();

    let argv = [PYTHON.to_string(), CHECK_SCRIPT.to_string()];
    let granted: &[&[&str]] = &[&[PYTHON]];

    // Against the PATCHED file: exit 0, and byte-identical to what the bin
    // folded into pair 4's prompt.
    let patched_dir = fresh_dir("run-observation-patched");
    materialize(&patched_dir, &run_files());
    std::fs::write(patched_dir.join(TARGET), patched).unwrap();
    let ours: Observation = exec_run(
        &grant_for(&patched_dir, granted),
        &patched_dir,
        &argv,
        &bounds(),
    );
    assert!(!ours.failed, "{ours:?}");
    // Hand-written literals (NOT derived from `ours`) pinning exec_run's
    // outcome/content grammar directly — the brief's `"ran {program} exit
    // {code}"` and `"exit {code}\n{output}"`.
    assert_eq!(ours.outcome, format!("ran {PYTHON} exit 0"));
    assert!(
        ours.content.starts_with("exit 0\n"),
        "content: {:?}",
        ours.content
    );
    assert!(
        transcript.contains(&transcript_entry(3, "run", &ours.outcome, &ours.content)),
        "pair 4's transcript is missing the real run observation: {transcript:?}"
    );

    // Against the UNPATCHED file the very same command fails — so a run
    // observation reporting exit 0 could only have come from the patched
    // file.
    let unpatched_dir = fresh_dir("run-observation-unpatched");
    materialize(&unpatched_dir, &run_files());
    let theirs = exec_run(
        &grant_for(&unpatched_dir, granted),
        &unpatched_dir,
        &argv,
        &bounds(),
    );
    assert_eq!(
        theirs.outcome,
        format!("ran {PYTHON} exit 1"),
        "the check script must FAIL against the unpatched file, or this test proves nothing: \
         {theirs:?}"
    );
}

#[test]
fn bin_run_verified_pair_2_prompt_carries_the_real_read_of_the_target() {
    let response = run_flywheel_tool(&run_request("v1"));
    let pairs = response["pairs"].as_array().expect("pairs array");
    let transcript = v1_transcript_of(RUN_GOAL, pairs[1]["prompt"].as_str().unwrap());

    assert_eq!(
        transcript,
        format!(
            "\n[step 1 read] read {} bytes\n{TARGET_CONTENTS}\n",
            TARGET_CONTENTS.len()
        )
    );
}

#[test]
fn bin_run_request_whose_verification_exits_nonzero_is_a_hard_error() {
    // A patch that LANDS but does not fix the defect: `check.py` still
    // asserts, so the verification exits 1 — and an ideal whose verification
    // fails is not an ideal.
    let mut request = run_request("v1");
    request["replace"] = serde_json::json!("return total / (len(xs) - 1)  # checked");

    let response = run_flywheel_tool(&request);
    let error = error_of(&response);
    assert!(error.contains("exit 1"), "{error}");
    assert!(
        response.get("pairs").is_none(),
        "a failed verification renders no pairs at all, got {response}"
    );
}

#[test]
fn bin_run_request_with_ungranted_argv_is_a_named_error() {
    let mut request = run_request("v1");
    request["commands"] = serde_json::json!([["true"]]);

    let error = error_of(&run_flywheel_tool(&request));
    assert!(error.contains("grant violation"), "{error}");
}

/// A reference patch that does not land is NOT a hard error in the
/// run-verified path: it answers with the pairs built so far (read, patch —
/// the two that exist without a landing), plus `landed: false` and a
/// `landing_detail`, and no `patched_contents`. Same partial-response
/// contract turn 1 pinned in
/// `flywheel_tool_test.rs::bin_trajectory_request_with_a_bad_search_yields_landed_false_with_detail`.
///
/// Worth stating because this shape has a *second*, deliberately different
/// failure mode next door: a patch that lands but whose verification exits
/// non-zero is a hard error with no pairs at all
/// (`bin_run_request_whose_verification_exits_nonzero_is_a_hard_error`).
/// The two must not collapse into each other — a patch that never landed
/// was never verified, and there is nothing to run.
#[test]
fn bin_run_verified_request_with_a_bad_search_yields_landed_false_with_detail() {
    let mut request = run_request("v1");
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

    let pairs = response["pairs"].as_array().expect("pairs array");
    assert_eq!(pairs.len(), 2, "{response}");
    assert_eq!(
        pairs[0]["completion"].as_str().unwrap(),
        format!("<action verb=\"read\" path=\"{TARGET}\">\n</action>")
    );
    // No `run` pair: the verification step is only renderable once there is
    // a patched file to run against.
    for pair in pairs {
        let completion = pair["completion"].as_str().unwrap();
        assert!(
            !completion.contains("verb=\"run\""),
            "a trajectory whose patch never landed must not render a run pair: {completion:?}"
        );
    }
}

/// `find_pattern` and `run_argv` name two different 4-pair shapes; there is
/// no defined 5-pair shape that is both, so a request carrying both is a
/// named error rather than a silent pick-one.
#[test]
fn bin_request_carrying_both_find_pattern_and_run_argv_is_a_named_error() {
    let mut request = run_request("v1");
    request["find_pattern"] = serde_json::json!("average");

    let error = error_of(&run_flywheel_tool(&request));
    assert!(error.contains("find_pattern"), "{error}");
    assert!(error.contains("run_argv"), "{error}");
}

/// Neither shape selector applies under `expect="refuse"`: a refusal renders
/// exactly 2 pairs (read, done) and never patches anything, so there is
/// nothing for a `find` step or a verification `run` to attach to. Both
/// halves of that guard are exercised here — the arm is one `Err` behind an
/// or-pattern, and a test for only one half would leave the other free to
/// fall through to `handle_refuse_trajectory`, which would silently ignore
/// the selector and render a 2-pair refusal as if nothing were wrong.
#[test]
fn bin_refuse_request_carrying_a_shape_selector_is_a_named_error() {
    for (label, selector) in [
        (
            "run_argv",
            serde_json::json!({"run_argv": [PYTHON, CHECK_SCRIPT]}),
        ),
        (
            "find_pattern",
            serde_json::json!({"find_pattern": "average"}),
        ),
    ] {
        let mut request = run_request("v1");
        let object = request.as_object_mut().expect("request is an object");
        object.remove("run_argv");
        object.insert("expect".into(), serde_json::json!("refuse"));
        object.insert(
            "refusal_reason".into(),
            serde_json::json!("No change needed: average() already divides by len(xs)."),
        );
        for (k, v) in selector.as_object().expect("selector is an object") {
            object.insert(k.clone(), v.clone());
        }

        let error = error_of(&run_flywheel_tool(&request));
        assert!(error.contains("refuse"), "{label}: {error}");
        assert!(
            error.contains("find_pattern") && error.contains("run_argv"),
            "{label}: {error}"
        );
    }
}

// ---------------------------------------------------------------------------
// The run anti-drift pin: same shape as the find pin above, for a real
// `run` step.
// ---------------------------------------------------------------------------

const PIN_RUN_ARGV: [&str; 3] = [PYTHON, "-c", "print('verified')"];

fn assert_run_second_prompt_matches_tool_path(envelope: EnvelopeLens) {
    let dir = fresh_dir(&format!("antidrift-run-{}", envelope_tag(envelope)));
    let sb = dir.join("sandbox");
    std::fs::create_dir_all(&sb).unwrap();
    materialize(&sb, &run_files());
    let sb = std::fs::canonicalize(&sb).unwrap();
    let granted: &[&[&str]] = &[&[PYTHON]];
    // The same grant, in the shape `render_task_prompt` renders envelope-v4's
    // grant line from — one owned copy, so both sides of the pin below read
    // the SAME commands the task's real `Grant` was built with.
    let granted_commands: Vec<Vec<String>> = granted
        .iter()
        .map(|prefix| prefix.iter().map(|word| word.to_string()).collect())
        .collect();
    let argv: Vec<String> = PIN_RUN_ARGV.iter().map(|s| s.to_string()).collect();

    let (mut pager, agent_id) = fixture(
        &dir,
        vec![
            scripted(&format!(
                "<action verb=\"run\">\n{}\n</action>",
                serde_json::to_string(&argv).unwrap()
            )),
            scripted("<action verb=\"done\">\nverified\n</action>"),
        ],
    );
    let mut task_journal = Journal::open(&dir.join("task.jsonl")).unwrap();
    let task_spec = TaskSpec {
        goal: RUN_GOAL.to_string(),
        grant: grant_for(&sb, granted),
        budget_tokens: 1_000_000,
        max_steps: 5,
        cwd: sb.clone(),
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
        "expected [run, done]: {:?}",
        result.steps
    );
    assert!(
        !result.steps[0].failed,
        "the run step must succeed: {:?}",
        result.steps[0]
    );

    let history = pager
        .substrate()
        .ctx_history(1)
        .expect("context 1 is resident after a 2-step task");
    let prompt1 = render_task_prompt(
        RUN_GOAL,
        PatchCodec::SearchReplace,
        envelope,
        &granted_commands,
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

    let observation = exec_run(&grant_for(&sb, granted), &sb, &argv, &bounds());
    assert!(!observation.failed, "{observation:?}");
    let transcript = transcript_entry(1, "run", &observation.outcome, &observation.content);
    let computed_second_prompt = render_task_prompt(
        RUN_GOAL,
        PatchCodec::SearchReplace,
        envelope,
        &granted_commands,
        &transcript,
    );

    assert_eq!(
        actual_second_prompt, computed_second_prompt,
        "flywheel-tool's run tool-path prompt has drifted from the real run_task prompt"
    );

    // This is the shape turn 4 exists for: a run-granted task, whose prompt
    // must SAY so under envelope-v4 — rendered from the very `Grant` that
    // let the `run` step above succeed. Pinned against the literal, not
    // against `grant_line`'s output, so deleting the render branch fails
    // here (the equality above shares one renderer on both sides).
    if envelope == EnvelopeLens::V4 {
        assert!(
            actual_second_prompt
                .starts_with(&format!("{RUN_GOAL}\n\nGranted commands: {PYTHON}\n\n")),
            "the real run_task v4 prompt must open with the goal then the granted line — got: \
             {actual_second_prompt:?}"
        );
    }
}

#[test]
fn run_anti_drift_pin_matches_real_second_prompt_under_v1() {
    assert_run_second_prompt_matches_tool_path(EnvelopeLens::V1);
}

#[test]
fn run_anti_drift_pin_matches_real_second_prompt_under_v2() {
    assert_run_second_prompt_matches_tool_path(EnvelopeLens::V2);
}

#[test]
fn run_anti_drift_pin_matches_real_second_prompt_under_v3() {
    assert_run_second_prompt_matches_tool_path(EnvelopeLens::V3);
}

#[test]
fn run_anti_drift_pin_matches_real_second_prompt_under_v4() {
    assert_run_second_prompt_matches_tool_path(EnvelopeLens::V4);
}

/// The bin's run-verified v4 prompts carry the granted line built from the
/// request's own `commands` — the cue turn 4 adds so a run-granted task and
/// a plain one stop being token-indistinguishable (spec §1).
#[test]
fn bin_run_verified_v4_prompts_carry_the_granted_line_from_the_requests_commands() {
    let response = run_flywheel_tool(&run_request("v4"));
    assert_eq!(response["landed"], serde_json::json!(true), "{response}");
    let prompt0 = response["pairs"][0]["prompt"]
        .as_str()
        .unwrap_or_else(|| panic!("missing pairs[0].prompt in {response}"));
    assert!(
        prompt0.starts_with(&format!("{RUN_GOAL}\n\nGranted commands: {PYTHON}\n\n")),
        "a run-granted request must render its grant: {prompt0:?}"
    );
    let commands = vec![vec![PYTHON.to_string()]];
    assert_eq!(
        prompt0,
        render_task_prompt(
            RUN_GOAL,
            PatchCodec::SearchReplace,
            EnvelopeLens::V4,
            &commands,
            ""
        ),
        "the bin's v4 run prompt drifted from the loop's renderer"
    );
}
