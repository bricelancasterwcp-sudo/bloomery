//! `flywheel-tool` find-shaped trajectory tests (flywheel turn-3, task-6
//! brief; design doc §2's "find/run enter through repair ideals") — one of
//! four files in the `flywheel_tool_*_test.rs` family, alongside
//! `flywheel_tool_test.rs` (turn-1 patch mode),
//! `flywheel_tool_refuse_test.rs` (turn-2 refusals), and
//! `flywheel_tool_run_test.rs` (turn-3's run-verified shape).
//!
//! **The find-shaped trajectory** (`find_pattern` set on the wire): 4 pairs,
//! `find` -> `read` -> `patch` -> `done`. The `find` and `read` observations
//! both come from REAL [`exec_find`]/`exec_read` calls against a scratch dir
//! the binary materializes from the request's `files`.
//!
//! **Why `find` is checked for format-parity, not byte-parity.** Unlike the
//! failed-`read` observation the missing-target refusal family pins
//! byte-for-byte, `exec_find`'s per-hit line embeds a canonicalized,
//! absolute path (`exec.rs`'s `match_file`: `format!("{}:{}: {line}", ...)`),
//! so the same match in two different scratch directories renders different
//! bytes by construction. That is a recorded, pre-registered property of the
//! instrument (`docs/superpowers/evidence/2026-08-20-g5v3-protocol.md` §6.2),
//! not a defect — so the bin-level check here compares the *shape* of every
//! hit line (absolute path, line number, line text) plus the real match
//! count, while the anti-drift pin (which CAN hold the directory fixed)
//! compares bytes.
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
use bloomery_daemon::task::{exec_find, run_task, ExecBounds, TaskSpec, TaskStatus};
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
// The task fixture: one buggy `average()` plus two plausible siblings.
// ---------------------------------------------------------------------------

const TARGET: &str = "stats.py";
const TARGET_CONTENTS: &str = "def average(xs):\n    total = 0\n    for x in xs:\n        total += x\n    return total / (len(xs) - 1)\n";
const SEARCH: &str = "return total / (len(xs) - 1)";
const REPLACE: &str = "return total / len(xs)";
const SUMMARY: &str = "fixed the average() denominator";

const FIND_GOAL: &str = "one helper divides by one reading too few; fix it";
const FIND_PATTERN: &str = "average";
const FIND_PATH: &str = ".";
const SIBLING_PY: &str = "report.py";
const SIBLING_PY_CONTENTS: &str =
    "from stats import average\n\n\ndef report(xs):\n    return f\"avg={average(xs)}\"\n";
const SIBLING_TXT: &str = "notes.txt";
const SIBLING_TXT_CONTENTS: &str = "the averages looked wrong in last week's report\n";

fn find_files() -> serde_json::Value {
    serde_json::json!([
        {"path": TARGET, "contents": TARGET_CONTENTS},
        {"path": SIBLING_PY, "contents": SIBLING_PY_CONTENTS},
        {"path": SIBLING_TXT, "contents": SIBLING_TXT_CONTENTS},
    ])
}

fn find_request(envelope: &str) -> serde_json::Value {
    serde_json::json!({
        "cmd": "trajectory",
        "goal": FIND_GOAL,
        "patch_codec": "search_replace",
        "envelope": envelope,
        "target": TARGET,
        "target_contents": TARGET_CONTENTS,
        "search": SEARCH,
        "replace": REPLACE,
        "summary": SUMMARY,
        "files": find_files(),
        "find_pattern": FIND_PATTERN,
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
// find-shaped trajectories.
// ---------------------------------------------------------------------------

#[test]
fn bin_find_shaped_request_renders_four_pairs_as_find_read_patch_done() {
    let response = run_flywheel_tool(&find_request("v3"));

    assert_eq!(response["landed"], serde_json::json!(true), "{response}");
    assert!(
        response.get("verified").is_none(),
        "a patch-mode response never gains `verified`, got {response}"
    );
    let pairs = response["pairs"].as_array().expect("pairs array");
    assert_eq!(pairs.len(), 4, "{response}");

    // Pair 1's completion is the verb card's `find` grammar exactly
    // (`bloomery_core::action::card`: attributes in `pattern`, `path` order,
    // empty body, no trailing newline after the closing tag).
    assert_eq!(
        pairs[0]["completion"].as_str().unwrap(),
        format!(
            "<action verb=\"find\" pattern=\"{FIND_PATTERN}\" path=\"{FIND_PATH}\">\n</action>"
        )
    );
    assert_eq!(
        pairs[1]["completion"].as_str().unwrap(),
        format!("<action verb=\"read\" path=\"{TARGET}\">\n</action>")
    );
    let completion3 = pairs[2]["completion"].as_str().unwrap();
    assert!(
        completion3.contains(&format!(
            "<<<<<<< SEARCH\n{SEARCH}\n=======\n{REPLACE}\n>>>>>>> REPLACE"
        )),
        "pair 3's completion is missing the exact SEARCH block: {completion3:?}"
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

/// Normalizes `exec_find` content into sorted `(file name, line number, line
/// text)` triples, asserting each hit line's format on the way: the
/// `{absolute path}:{lineno}: {text}` shape `match_file` renders. The
/// absolute path is dropped down to its file name on purpose — it is the one
/// part of a real `find` observation that cannot match across two scratch
/// directories (see this module's docs), so comparing it would pin the
/// machine, not the format.
fn hits(content: &str) -> Vec<(String, usize, String)> {
    let mut out: Vec<(String, usize, String)> = content
        .lines()
        .map(|line| {
            let mut parts = line.splitn(3, ':');
            let path = parts.next().unwrap_or_default();
            let lineno = parts.next().unwrap_or_else(|| {
                panic!("a find hit line carries a line number: {line:?}");
            });
            let text = parts
                .next()
                .unwrap_or_else(|| panic!("a find hit line carries the matched text: {line:?}"));
            assert!(
                path.starts_with('/'),
                "a find hit line embeds an absolute path: {line:?}"
            );
            let lineno: usize = lineno
                .parse()
                .unwrap_or_else(|_| panic!("a find hit line's line number parses: {line:?}"));
            let text = text
                .strip_prefix(' ')
                .unwrap_or_else(|| panic!("a find hit line separates text with ': ': {line:?}"));
            let name = path.rsplit('/').next().unwrap_or_default().to_string();
            (name, lineno, text.to_string())
        })
        .collect();
    out.sort();
    out
}

/// Cross-checks the binary's OWN find observation against a directly-called
/// real [`exec_find`] over the same `files` in the test's own scratch dir.
/// The strongest check available for this shape: the binary never sees the
/// test's directory, so a hand-formatted `"found N matches"` (or a
/// hand-built hit line) could not survive it.
#[test]
fn bin_find_shaped_pair_2_prompt_carries_a_real_exec_find_observation() {
    let response = run_flywheel_tool(&find_request("v1"));
    let pairs = response["pairs"].as_array().expect("pairs array");
    let transcript = v1_transcript_of(FIND_GOAL, pairs[1]["prompt"].as_str().unwrap());

    let dir = fresh_dir("find-observation");
    materialize(&dir, &find_files());
    let ours = exec_find(
        &grant_for(&dir, &[]),
        FIND_PATTERN,
        &dir.to_string_lossy(),
        &bounds(),
    );
    assert!(
        !ours.failed,
        "a find over the scratch dir succeeds: {ours:?}"
    );

    // The real match count, derived from the real content — never a
    // hand-counted literal.
    let n = ours.content.lines().count();
    assert!(n > 1, "the fixture is multi-file on purpose: {ours:?}");
    assert_eq!(ours.outcome, format!("found {n} matches"));

    let head = format!("\n[step 1 find] found {n} matches\n");
    assert!(
        transcript.starts_with(&head),
        "pair 2's transcript does not open with the real find observation ({head:?}): \
         {transcript:?}"
    );
    let bin_content = transcript
        .strip_prefix(&head)
        .unwrap()
        .strip_suffix('\n')
        .expect("transcript_entry ends every entry with a newline");
    assert_eq!(
        hits(bin_content),
        hits(&ours.content),
        "the bin's find hits differ (modulo scratch dir) from a direct real exec_find call"
    );
}

#[test]
fn bin_find_shaped_pair_3_prompt_carries_the_real_read_of_the_found_target() {
    let response = run_flywheel_tool(&find_request("v1"));
    let pairs = response["pairs"].as_array().expect("pairs array");
    let transcript = v1_transcript_of(FIND_GOAL, pairs[2]["prompt"].as_str().unwrap());

    let expected_read = format!(
        "\n[step 2 read] read {} bytes\n{TARGET_CONTENTS}\n",
        TARGET_CONTENTS.len()
    );
    assert!(
        transcript.ends_with(&expected_read),
        "pair 3's transcript does not end with the real read of the target: {transcript:?}"
    );
}

/// **The determinism law, at the tool boundary** (controller ruling bT7/R1).
/// The factory's rule 3 is "same seed -> byte-identical corpus", and the find
/// shape breaks it the moment the scratch directory's name varies between
/// runs, because `exec_find` embeds that absolute path in every hit and the
/// hit lands verbatim in three of the four rendered prompts.
///
/// `run_flywheel_tool` spawns a fresh process per call, so the two responses
/// compared here come from two separate `flywheel-tool` invocations — the
/// exact cross-process case a pid-derived directory name failed. Byte
/// equality of the whole response is the assertion; nothing is normalized
/// away, which is the point.
#[test]
fn bin_find_shaped_request_renders_byte_identically_across_two_tool_processes() {
    let first = run_flywheel_tool(&find_request("v3"));
    let second = run_flywheel_tool(&find_request("v3"));

    assert_eq!(
        first, second,
        "two runs of the same find-shaped request differ — the scratch directory's name is \
         reaching the rendered bytes again, which breaks the factory's same-seed determinism law"
    );
}

/// The companion to the determinism pin above, and the reason it must be a
/// *separate* assertion: determinism could also be "achieved" by rewriting
/// the rendered observation (stripping the path, or making it relative),
/// and that would silently turn a real executor observation into
/// post-processed text — the thing this whole binary exists not to do.
///
/// So: the find hits must STILL embed a real, absolute, canonicalized
/// scratch path. Determinism comes from the path being reproducible, never
/// from it being erased.
#[test]
fn bin_find_shaped_observation_still_embeds_a_real_absolute_scratch_path() {
    let response = run_flywheel_tool(&find_request("v1"));
    let pairs = response["pairs"].as_array().expect("pairs array");
    let transcript = v1_transcript_of(FIND_GOAL, pairs[1]["prompt"].as_str().unwrap());

    let scratch_prefix = std::env::temp_dir().join("flywheel-tool-scratch-");
    let scratch_prefix = scratch_prefix.to_string_lossy().into_owned();
    let hit_lines: Vec<&str> = transcript
        .lines()
        .filter(|line| line.starts_with(&scratch_prefix))
        .collect();
    assert!(
        !hit_lines.is_empty(),
        "no find hit in pair 2's transcript starts with {scratch_prefix:?} — either the \
         observation is no longer real executor output, or the scratch dir moved: {transcript:?}"
    );
    // Every hit line is still `{absolute path}:{lineno}: {text}`, and the
    // digest segment is the fixed-width hex the ruling specified.
    for line in &hit_lines {
        let digest = line[scratch_prefix.len()..]
            .split('/')
            .next()
            .expect("the scratch dir name precedes the first '/'");
        assert_eq!(digest.len(), 16, "expected a 16-hex-char digest: {line:?}");
        assert!(
            digest.chars().all(|c| c.is_ascii_hexdigit()),
            "expected a hex digest: {line:?}"
        );
    }
    assert_eq!(
        hits(&hit_lines.join("\n")).len(),
        hit_lines.len(),
        "every hit line still parses as {{absolute path}}:{{lineno}}: {{text}}"
    );
}

#[test]
fn bin_find_shaped_request_whose_pattern_matches_nothing_is_a_named_error() {
    let mut request = find_request("v1");
    request["find_pattern"] = serde_json::json!("no-line-in-any-fixture-file-says-this");

    let error = error_of(&run_flywheel_tool(&request));
    assert!(
        error.contains("0 matches") || error.contains("no matches"),
        "{error}"
    );
}

#[test]
fn bin_find_shaped_request_whose_target_is_not_among_files_is_a_named_error() {
    let mut request = find_request("v1");
    request["files"] = serde_json::json!([
        {"path": SIBLING_PY, "contents": SIBLING_PY_CONTENTS},
    ]);

    let error = error_of(&run_flywheel_tool(&request));
    assert!(error.contains(TARGET), "{error}");
}

/// `files` is what the model actually reads; `target_contents` is what the
/// request says it will read. When they disagree the rendered `patch` would
/// act on bytes the rendered transcript never showed, so the disagreement
/// is a named error rather than a silent preference for either one.
#[test]
fn bin_find_shaped_request_whose_files_disagree_with_target_contents_is_a_named_error() {
    let mut request = find_request("v1");
    request["files"] = serde_json::json!([
        {"path": TARGET, "contents": format!("# an average of drift\n{TARGET_CONTENTS}")},
        {"path": SIBLING_PY, "contents": SIBLING_PY_CONTENTS},
    ]);

    let error = error_of(&run_flywheel_tool(&request));
    assert!(error.contains("target_contents"), "{error}");
    assert!(error.contains(TARGET), "{error}");
}

/// A reference patch that does not land is NOT a hard error in the
/// find-shaped path either: it answers with the pairs built so far
/// (find, read, patch — the three that exist without a landing), plus
/// `landed: false` and a `landing_detail`, and no `patched_contents`. Same
/// partial-response contract turn 1 pinned in
/// `flywheel_tool_test.rs::bin_trajectory_request_with_a_bad_search_yields_landed_false_with_detail`,
/// cloned here because that pin says nothing about this shape — the find and
/// read steps still really ran, so the pairs that precede the patch are real
/// and are still returned.
#[test]
fn bin_find_shaped_request_with_a_bad_search_yields_landed_false_with_detail() {
    let mut request = find_request("v1");
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

    // The find and read steps really happened, so their pairs survive: the
    // response is the 4-pair shape minus only the `done` pair the missing
    // landing made unrenderable.
    let pairs = response["pairs"].as_array().expect("pairs array");
    assert_eq!(pairs.len(), 3, "{response}");
    assert_eq!(
        pairs[0]["completion"].as_str().unwrap(),
        format!(
            "<action verb=\"find\" pattern=\"{FIND_PATTERN}\" path=\"{FIND_PATH}\">\n</action>"
        )
    );
    assert_eq!(
        pairs[1]["completion"].as_str().unwrap(),
        format!("<action verb=\"read\" path=\"{TARGET}\">\n</action>")
    );
}

/// A `files` entry names where bytes get written, so a path that climbs out
/// of the scratch dir is refused by name — before anything is written, and
/// whether or not the escape would have resolved to a real location.
#[test]
fn bin_request_whose_files_path_escapes_the_scratch_dir_is_a_named_error() {
    let mut request = find_request("v1");
    request["files"] = serde_json::json!([
        {"path": TARGET, "contents": TARGET_CONTENTS},
        {"path": "../escaped.py", "contents": "print('elsewhere')\n"},
    ]);

    let error = error_of(&run_flywheel_tool(&request));
    assert!(error.contains("../escaped.py"), "{error}");
}

// ---------------------------------------------------------------------------
// The find anti-drift pin: a real `run_task` run's SECOND prompt (rendered
// after a real `find` step) must byte-equal the tool path's reconstruction.
//
// Unlike the missing-target read pin (`flywheel_tool_refuse_test.rs`), which
// deliberately reconstructs from a DIFFERENT scratch dir to prove the
// observation text is path-independent, this pin holds the directory fixed:
// a find observation embeds absolute paths by construction, so cross-directory
// byte-equality is not a property `exec_find` has (see this module's docs).
// What this pin proves is the same thing that one does — that
// `render_task_prompt` + `transcript_entry` folded around a real executor
// observation reproduce the real loop's prompt exactly, for the `find` verb.
// ---------------------------------------------------------------------------

fn assert_find_second_prompt_matches_tool_path(envelope: EnvelopeLens) {
    let dir = fresh_dir(&format!("antidrift-find-{}", envelope_tag(envelope)));
    let sb = dir.join("sandbox");
    std::fs::create_dir_all(&sb).unwrap();
    materialize(&sb, &find_files());
    let sb = std::fs::canonicalize(&sb).unwrap();

    let (mut pager, agent_id) = fixture(
        &dir,
        vec![
            scripted(&format!(
                "<action verb=\"find\" pattern=\"{FIND_PATTERN}\" path=\"{FIND_PATH}\">\n</action>"
            )),
            scripted("<action verb=\"done\">\nfound the helper\n</action>"),
        ],
    );
    let mut task_journal = Journal::open(&dir.join("task.jsonl")).unwrap();
    let task_spec = TaskSpec {
        goal: FIND_GOAL.to_string(),
        grant: grant_for(&sb, &[]),
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
        "expected [find, done]: {:?}",
        result.steps
    );
    assert!(
        !result.steps[0].failed,
        "the find step must succeed: {:?}",
        result.steps[0]
    );

    let history = pager
        .substrate()
        .ctx_history(1)
        .expect("context 1 is resident after a 2-step task");
    let prompt1 = render_task_prompt(FIND_GOAL, PatchCodec::SearchReplace, envelope, &[], "");
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

    let observation = exec_find(
        &grant_for(&sb, &[]),
        FIND_PATTERN,
        &sb.to_string_lossy(),
        &bounds(),
    );
    assert!(!observation.failed, "{observation:?}");
    let transcript = transcript_entry(1, "find", &observation.outcome, &observation.content);
    let computed_second_prompt = render_task_prompt(
        FIND_GOAL,
        PatchCodec::SearchReplace,
        envelope,
        &[],
        &transcript,
    );

    assert_eq!(
        actual_second_prompt, computed_second_prompt,
        "flywheel-tool's find tool-path prompt has drifted from the real run_task prompt"
    );

    // A find-shaped task grants no command, so envelope-v4's grant line is
    // the `none` literal — pinned directly (the equality above would survive
    // deleting the render branch, since both sides share one renderer).
    if envelope == EnvelopeLens::V4 {
        assert!(
            actual_second_prompt.starts_with(&format!(
                "{FIND_GOAL}\n\nGranted commands: none — run is not available in this task\n\n"
            )),
            "the real run_task v4 find prompt must open with the goal then the none line — got: \
             {actual_second_prompt:?}"
        );
    }
}

#[test]
fn find_anti_drift_pin_matches_real_second_prompt_under_v1() {
    assert_find_second_prompt_matches_tool_path(EnvelopeLens::V1);
}

#[test]
fn find_anti_drift_pin_matches_real_second_prompt_under_v2() {
    assert_find_second_prompt_matches_tool_path(EnvelopeLens::V2);
}

#[test]
fn find_anti_drift_pin_matches_real_second_prompt_under_v3() {
    assert_find_second_prompt_matches_tool_path(EnvelopeLens::V3);
}

#[test]
fn find_anti_drift_pin_matches_real_second_prompt_under_v4() {
    assert_find_second_prompt_matches_tool_path(EnvelopeLens::V4);
}

/// The bin's find-shaped v4 prompts carry the `none` grant line — a find
/// task grants no command, and turn 4's whole point is that this is now
/// visible at the decision point (spec §1).
#[test]
fn bin_find_shaped_v4_prompts_carry_the_none_grant_line() {
    let response = run_flywheel_tool(&find_request("v4"));
    let prompt0 = response["pairs"][0]["prompt"]
        .as_str()
        .unwrap_or_else(|| panic!("missing pairs[0].prompt in {response}"));
    assert!(
        prompt0.starts_with(&format!(
            "{FIND_GOAL}\n\nGranted commands: none — run is not available in this task\n\n"
        )),
        "a find-shaped request grants no command: {prompt0:?}"
    );
    assert_eq!(
        prompt0,
        render_task_prompt(
            FIND_GOAL,
            PatchCodec::SearchReplace,
            EnvelopeLens::V4,
            &[],
            ""
        ),
        "the bin's v4 find prompt drifted from the loop's renderer"
    );
}

// ---------------------------------------------------------------------------
// Legacy shape: a request carrying none of the new fields is untouched.
// ---------------------------------------------------------------------------

#[test]
fn bin_request_without_any_new_field_still_renders_the_three_pair_patch_shape() {
    let mut request = find_request("v3");
    let object = request.as_object_mut().expect("request is an object");
    object.remove("files");
    object.remove("find_pattern");

    let response = run_flywheel_tool(&request);
    assert_eq!(response["landed"], serde_json::json!(true), "{response}");
    let pairs = response["pairs"].as_array().expect("pairs array");
    assert_eq!(pairs.len(), 3, "{response}");
    assert_eq!(
        pairs[0]["completion"].as_str().unwrap(),
        format!("<action verb=\"read\" path=\"{TARGET}\">\n</action>")
    );
}
