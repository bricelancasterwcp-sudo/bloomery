//! The rendered task prompt, pinned byte-for-byte — one file for the
//! envelope-v4 change (turn-4 spec §2,
//! `docs/superpowers/specs/2026-08-21-flywheel4-turn4-design.md`).
//!
//! **The byte-identity law.** envelope-v4 adds a grant line to the rendered
//! prompt. v1, v2 and v3 must keep rendering exactly what they rendered
//! before it existed, because every earlier verdict (`codec-tasks-v1`,
//! `-v2-mixed`, `-v3-mixed`, and every G4/G5 number the ledger carries) was
//! measured against those bytes: a silent change to them would retroactively
//! invalidate comparisons nobody would notice were broken. The three
//! `*_render_is_byte_identical_to_the_captured_golden` tests below are that
//! law's pin. Their expected strings were captured from the binary at
//! `852b1fe` (turn 4's base, rendering identical to `129843e`) BEFORE the v4
//! branch was written, pasted here as literals, and were green against the
//! pre-change code — so they can only stay green if v1/v2/v3 rendering never
//! moved.
//!
//! Deliberately literals, not derived: a golden computed by calling the same
//! renderer it is checking would agree with any mutation of it, which is the
//! one thing this file exists to catch.
//!
//! **What is pinned through the public wrapper.** [`render_task_prompt`] is
//! the `pub` face of the private `render_prompt` the loop itself calls, and
//! the two share one body by construction (see `task_loop.rs`'s doc: "this
//! function and `render_prompt` MUST share one body"). That sharing is
//! independently pinned by the four anti-drift tests
//! (`flywheel_tool_test.rs`, `flywheel_tool_find_test.rs`,
//! `flywheel_tool_run_test.rs`, `flywheel_tool_refuse_test.rs`), each of
//! which drives a REAL `run_task` (or the real binary) and byte-compares the
//! prompt the substrate actually received against this wrapper's output. So
//! a golden pinned here is a golden pinned on the live loop.

use bloomery_core::action::PatchCodec;
use bloomery_daemon::config::EnvelopeLens;
use bloomery_daemon::task::grant_line::grant_line;
use bloomery_daemon::task::task_loop::render_task_prompt;

/// The golden spec's goal — fixed, so the captured literals below are a
/// function of the renderer alone.
const GOAL: &str = "fix the off-by-one in total() so the sum includes the last element";

/// The golden spec's transcript: one folded read step, in exactly
/// `transcript_entry`'s pinned shape.
const TRANSCRIPT: &str =
    "\n[step 1 read] read 44 bytes\ndef total(xs):\n    return sum(xs[:-1])\n\n";

/// The envelope-v2/v3 pre-seed literal (`task_loop.rs`'s private
/// `THINK_PRESEED`), restated here rather than imported: importing the
/// constant would make the v2/v3 goldens agree with any mutation of it.
const PRESEED: &str = "<think>\n\n</think>\n\n";

/// The v1 prompt for the golden spec, captured from the pre-v4 binary.
const V1_GOLDEN: &str = r#"fix the off-by-one in total() so the sum includes the last element

# Action verbs

Exactly one action per turn: exactly one action block from the five below,
nothing more. Narration before it is fine; a second action block in the same
turn is a single MultipleActions error (not applied piecemeal), and no
action block at all is NoAction.

## read — read a file, optionally a line range
<action verb="read" path="src/lib.rs" lines="1-40">
</action>

## find — search a path with a regex pattern
<action verb="find" pattern="fn \w+" path="src">
</action>

## patch — replace part or all of a file's contents
<action verb="patch" path="src/lib.rs">
<<<<<<< SEARCH
fn greeting() -> &'static str { "hi" }
=======
fn greeting() -> &'static str { "hello" }
>>>>>>> REPLACE
</action>

## run — execute a command; the body is a JSON array of argv strings
<action verb="run">
["cargo", "test"]
</action>

## done — end the task with a summary
<action verb="done">
fixed the failing test
</action>



[step 1 read] read 44 bytes
def total(xs):
    return sum(xs[:-1])

"#;

/// The granted-commands fixture: exactly the turn-4 run slice's grant
/// (spec §3), so the rendered line here is the one the corpus will carry.
fn granted() -> Vec<Vec<String>> {
    vec![vec![
        "python3".to_string(),
        "-m".to_string(),
        "unittest".to_string(),
    ]]
}

/// `V1_GOLDEN` minus its leading `"{GOAL}\n\n"` — everything envelope-v4
/// pushes down below the grant line, reused verbatim from the captured
/// literal so a v4 expectation can never quietly disagree with the golden
/// about the card or the transcript.
fn golden_below_the_goal() -> String {
    V1_GOLDEN
        .strip_prefix(&format!("{GOAL}\n\n"))
        .expect("the golden opens with the goal and a blank line")
        .to_string()
}

// ---------------------------------------------------------------------------
// The byte-identity law: v1/v2/v3 render exactly what they rendered before
// envelope-v4 existed — including when the task's grant DOES carry granted
// commands, which is the case v4 alone is allowed to render differently.
// ---------------------------------------------------------------------------

#[test]
fn v1_render_is_byte_identical_to_the_captured_golden() {
    let rendered = render_task_prompt(
        GOAL,
        PatchCodec::SearchReplace,
        EnvelopeLens::V1,
        &granted(),
        TRANSCRIPT,
    );
    assert_eq!(rendered, V1_GOLDEN);
}

#[test]
fn v2_render_is_byte_identical_to_the_captured_golden() {
    let rendered = render_task_prompt(
        GOAL,
        PatchCodec::SearchReplace,
        EnvelopeLens::V2,
        &granted(),
        TRANSCRIPT,
    );
    assert_eq!(rendered, format!("{V1_GOLDEN}{PRESEED}"));
}

#[test]
fn v3_render_is_byte_identical_to_the_captured_golden() {
    let rendered = render_task_prompt(
        GOAL,
        PatchCodec::SearchReplace,
        EnvelopeLens::V3,
        &granted(),
        TRANSCRIPT,
    );
    assert_eq!(rendered, format!("{V1_GOLDEN}{PRESEED}"));
}

#[test]
fn v1_renders_identically_whether_or_not_the_grant_carries_commands() {
    let with_commands = render_task_prompt(
        GOAL,
        PatchCodec::SearchReplace,
        EnvelopeLens::V1,
        &granted(),
        TRANSCRIPT,
    );
    let without = render_task_prompt(
        GOAL,
        PatchCodec::SearchReplace,
        EnvelopeLens::V1,
        &[],
        TRANSCRIPT,
    );
    assert_eq!(
        with_commands, without,
        "the grant line is envelope-v4 only — v1 must not see the commands at all"
    );
}

// ---------------------------------------------------------------------------
// envelope-v4: the grant line, rendered from the real grant (spec §2).
// ---------------------------------------------------------------------------

#[test]
fn grant_line_renders_the_granted_literal_space_joined() {
    assert_eq!(
        grant_line(&granted()),
        "Granted commands: python3 -m unittest"
    );
}

#[test]
fn grant_line_renders_the_none_literal_when_the_grant_allows_no_command() {
    assert_eq!(
        grant_line(&[]),
        "Granted commands: none — run is not available in this task"
    );
}

#[test]
fn grant_line_renders_one_line_per_argv_prefix() {
    let commands = vec![
        vec![
            "python3".to_string(),
            "-m".to_string(),
            "unittest".to_string(),
        ],
        vec!["cargo".to_string(), "test".to_string()],
    ];
    assert_eq!(
        grant_line(&commands),
        "Granted commands: python3 -m unittest\nGranted commands: cargo test"
    );
}

#[test]
fn v4_renders_the_granted_line_between_the_goal_and_the_verb_card() {
    let rendered = render_task_prompt(
        GOAL,
        PatchCodec::SearchReplace,
        EnvelopeLens::V4,
        &granted(),
        TRANSCRIPT,
    );
    let expected = format!(
        "{GOAL}\n\nGranted commands: python3 -m unittest\n\n{}{PRESEED}",
        golden_below_the_goal()
    );
    assert_eq!(rendered, expected);
}

#[test]
fn v4_renders_the_none_line_when_the_task_grants_no_command() {
    let rendered = render_task_prompt(
        GOAL,
        PatchCodec::SearchReplace,
        EnvelopeLens::V4,
        &[],
        TRANSCRIPT,
    );
    let expected = format!(
        "{GOAL}\n\nGranted commands: none — run is not available in this task\n\n{}{PRESEED}",
        golden_below_the_goal()
    );
    assert_eq!(rendered, expected);
}

#[test]
fn v4_is_v3_plus_exactly_the_grant_line() {
    let v3 = render_task_prompt(
        GOAL,
        PatchCodec::SearchReplace,
        EnvelopeLens::V3,
        &granted(),
        TRANSCRIPT,
    );
    let v4 = render_task_prompt(
        GOAL,
        PatchCodec::SearchReplace,
        EnvelopeLens::V4,
        &granted(),
        TRANSCRIPT,
    );
    assert_eq!(
        v4,
        v3.replacen(
            &format!("{GOAL}\n\n"),
            &format!("{GOAL}\n\nGranted commands: python3 -m unittest\n\n"),
            1
        ),
        "v4 must be v3 with the grant line inserted after the goal and nothing else"
    );
}

#[test]
fn v4_keeps_the_think_preseed_v3_ends_with() {
    let rendered = render_task_prompt(
        GOAL,
        PatchCodec::SearchReplace,
        EnvelopeLens::V4,
        &granted(),
        TRANSCRIPT,
    );
    assert!(
        rendered.ends_with(PRESEED),
        "envelope-v4 inherits v3's pre-seed: {rendered:?}"
    );
}

// ---------------------------------------------------------------------------
// Demotion beats the grant: a task that may not `run` says so, whatever its
// grant allows.
//
// Driven through a REAL `run_task` rather than `render_task_prompt`, because
// the wrapper hardcodes `mutating_verbs: true` — the demoted combination
// only exists on the live route, where `mutating_verbs` is fail-closed
// (`pager::codec_gate::resolve_mutating_verbs`: an unmeasured or demoted
// model reads `false`) while the task's grant is whatever the caller asked
// for. The fixture pattern below is `task_loop_test.rs`'s, restated (a
// separate `tests/*.rs` file is its own crate and cannot import it).
// ---------------------------------------------------------------------------

/// A demoted, v4-configured task whose grant DOES carry a command renders
/// the `none` line — never the granted one. Without this rule the model
/// would read `Granted commands: python3 -m unittest` directly above the
/// read-only card's `patch and run are not available in this task`.
#[test]
fn a_demoted_v4_task_renders_the_none_line_even_with_a_command_bearing_grant() {
    use bloomery_core::gguf::GgufMeta;
    use bloomery_core::grant::Grant;
    use bloomery_core::journal::Journal;
    use bloomery_daemon::agents::ImageStore;
    use bloomery_daemon::pager::Pager;
    use bloomery_daemon::task::{run_task, ExecBounds, TaskSpec, TaskStatus};
    use bloomery_substrate::fake::FakeSubstrate;
    use bloomery_substrate::Reply;

    let dir = std::env::temp_dir().join(format!(
        "bloomery-taskrender-demoted-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let sandbox = dir.join("sandbox");
    std::fs::create_dir_all(&sandbox).unwrap();
    let sandbox = std::fs::canonicalize(&sandbox).unwrap();

    // The grant a run-verified task carries — granted here, and deliberately
    // NOT what the prompt is allowed to advertise, because the task is
    // demoted.
    let grant = Grant::from_json(&format!(
        r#"{{"read_roots":["{s}"],"write_roots":[],"commands":[["python3","-m","unittest"]]}}"#,
        s = sandbox.display()
    ))
    .unwrap();

    let mut fake = FakeSubstrate::new();
    fake.script_reply(Reply {
        text: "<action verb=\"done\">\nnothing to do\n</action>".to_string(),
        prompt_tokens: Some(8),
        completion_tokens: Some(4),
        duration_ms: 1,
    });
    let mut pager = Pager::new(
        fake,
        Journal::open(&dir.join("pager.jsonl")).unwrap(),
        ImageStore::new(&dir.join("img")).unwrap(),
        Box::new(|| Some(1024 * 1024 * 1024)),
    );
    let gguf = dir.join("m.gguf");
    std::fs::write(&gguf, b"fake weights").unwrap();
    pager
        .register_model(
            "m",
            &gguf,
            GgufMeta {
                arch: "qwen2".into(),
                layers: 4,
                attention_layers: 4,
                kv_heads: 2,
                head_dim: 32,
                training_ctx: 65536,
                weights_bytes: 1000,
                recurrent_state_bytes: 0,
            },
            None,
        )
        .unwrap();
    let agent = pager.create_agent("m", 100, None, 1_000_000).unwrap();

    let spec = TaskSpec {
        goal: GOAL.to_string(),
        grant,
        budget_tokens: 1_000_000,
        max_steps: 5,
        cwd: sandbox.clone(),
        patch_codec: PatchCodec::SearchReplace,
        bounds: ExecBounds {
            read_cap_bytes: 256 * 1024,
            find_result_cap: 100,
            run_output_cap_bytes: 64 * 1024,
            run_timeout_secs: 120,
        },
        // Gate G4 demotion — the fail-closed value an unmeasured model gets.
        mutating_verbs: false,
        envelope: EnvelopeLens::V4,
        // The goldens above are memory-off bytes (memory-organ design spec
        // §4) — this live-loop check must be too.
        memory_block: None,
        window_ladder: false,
    };
    let mut task_journal = Journal::open(&dir.join("task.jsonl")).unwrap();
    let result = run_task(&mut pager, &agent.id, &spec, &mut task_journal);
    assert_eq!(result.status, TaskStatus::Done, "{:?}", result.steps);

    // One step, so the whole history IS the single rendered prompt.
    let prompt = pager
        .substrate()
        .ctx_history(1)
        .expect("context 1 is resident after a 1-step task");
    assert!(
        prompt.starts_with(&format!(
            "{GOAL}\n\nGranted commands: none — run is not available in this task\n\n"
        )),
        "a demoted v4 task must render the none line: {prompt:?}"
    );
    assert!(
        !prompt.contains("python3"),
        "a demoted task must never see its grant's commands advertised: {prompt:?}"
    );
    assert!(
        prompt.contains("patch and run are not available in this task"),
        "the demoted task gets the read-only verb card: {prompt:?}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
