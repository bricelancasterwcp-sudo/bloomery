//! The window ladder (docs/superpowers/specs/2026-08-27-window-ladder-design.md):
//! opt-in client-side scope degradation on PromptTooLarge. Mirrors
//! `task_loop_test.rs`'s FakeSubstrate fixture pattern; windows are sized
//! at runtime from the test's own expected strings via the pager's
//! `needed_tokens = prompt.len()/3 + max_tokens` arithmetic (CHARS_PER_TOKEN
//! = 3, STEP_MAX_TOKENS = 1024), never hardcoded.

use bloomery_core::action::PatchCodec;
use bloomery_core::gguf::GgufMeta;
use bloomery_core::grant::Grant;
use bloomery_core::journal::{replay, Event, Journal};
use bloomery_daemon::agents::ImageStore;
use bloomery_daemon::config::EnvelopeLens;
use bloomery_daemon::pager::Pager;
use bloomery_daemon::task::task_loop::render_task_prompt;
use bloomery_daemon::task::{run_task, ExecBounds, TaskSpec, TaskStatus};
use bloomery_substrate::fake::FakeSubstrate;
use bloomery_substrate::Reply;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

/// Mirrors `task_loop.rs`'s STEP_MAX_TOKENS and pager.rs's CHARS_PER_TOKEN —
/// restated as literals (not imported) so a golden can't agree with a
/// mutation of them (task_render_test.rs's rule).
const MAX_TOKENS: u64 = 1024;
const CHARS_PER_TOKEN: u64 = 3;

/// The window cap that admits `prompt` exactly: needed = len/3 + 1024 fits.
fn cap_fitting(prompt: &str) -> u32 {
    u32::try_from(prompt.len() as u64 / CHARS_PER_TOKEN + MAX_TOKENS).unwrap()
}

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

/// Unused at rung-off (a refused prompt never reaches the substrate, so this
/// task's only test scripts no replies) — the ladder-walk tests that follow
/// in this file script turns through it.
#[allow(dead_code)]
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
        "bloomery-ladder-{tag}-{}-{unique}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A read-only sandbox grant over `dir` itself (ladder tests only read).
fn sandbox_grant(dir: &std::path::Path) -> Grant {
    let d = std::fs::canonicalize(dir).unwrap();
    Grant::from_json(&format!(
        r#"{{"read_roots":["{s}"],"write_roots":["{s}"],"commands":[]}}"#,
        s = d.display()
    ))
    .unwrap()
}

/// `task_loop_test::fixture` plus a `window_cap` — the ladder's lever.
fn fixture(
    dir: &std::path::Path,
    window_cap: Option<u32>,
    replies: Vec<Reply>,
) -> (Pager<FakeSubstrate>, String) {
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
    let info = pager.create_agent("m", 100, window_cap, 1_000_000).unwrap();
    (pager, info.id)
}

const GOAL: &str = "exercise the window ladder";

fn ladder_spec(grant: Grant, cwd: PathBuf, window_ladder: bool) -> TaskSpec {
    TaskSpec {
        goal: GOAL.to_string(),
        grant,
        budget_tokens: 1_000_000,
        max_steps: 8,
        cwd,
        patch_codec: PatchCodec::SearchReplace,
        bounds: bounds(),
        mutating_verbs: true,
        envelope: EnvelopeLens::V1,
        memory_block: None,
        window_ladder,
    }
}

/// The pager's journaled `Refusal` rows for this run, in order — each
/// intermediate rung-up leaves one (spec §6: "already journaled by the
/// pager's own refusal event").
fn refusals(dir: &std::path::Path) -> Vec<(u64, u32)> {
    replay(&dir.join("pager.jsonl"))
        .unwrap()
        .into_iter()
        .filter_map(|e| match e {
            Event::Refusal {
                needed_tokens,
                window_tokens,
                ..
            } => Some((needed_tokens, window_tokens)),
            _ => None,
        })
        .collect()
}

/// The count of infer calls that actually reached the substrate — refused
/// rungs never do (the pager's window check is pre-inference).
fn infer_count(pager: &Pager<FakeSubstrate>) -> usize {
    pager
        .substrate()
        .calls()
        .iter()
        .filter(|c| c.starts_with("infer"))
        .count()
}

#[test]
fn ladder_off_dies_window_exhausted_on_the_first_refusal() {
    // Spec §8 test 1: window_ladder=false keeps today's behavior exactly —
    // one refusal, zero substrate infers, terminal WindowExhausted with the
    // pager's arithmetic in the summary.
    let dir = fresh_dir("off-identity");
    let big_memory = "memory ".repeat(2000); // ~14k chars — never fits below
    let rung2 = render_task_prompt(GOAL, PatchCodec::SearchReplace, EnvelopeLens::V1, &[], "");
    // Window admits the memory-less prompt but not the memory-bearing one.
    let (mut pager, agent_id) = fixture(&dir, Some(cap_fitting(&rung2)), vec![]);
    let mut journal = Journal::open(&dir.join("task.jsonl")).unwrap();
    let spec = TaskSpec {
        memory_block: Some(big_memory),
        ..ladder_spec(
            sandbox_grant(&dir),
            std::fs::canonicalize(&dir).unwrap(),
            false,
        )
    };
    let result = run_task(&mut pager, &agent_id, &spec, &mut journal);
    assert_eq!(result.status, TaskStatus::WindowExhausted);
    let refused = refusals(&dir);
    assert_eq!(refused.len(), 1, "exactly one refusal — no ladder walk");
    assert_eq!(
        infer_count(&pager),
        0,
        "a refused prompt never reaches the substrate"
    );
    let summary = result
        .summary
        .expect("summary carries the pager arithmetic");
    // Spec §2 is specific: the terminal summary carries *the pager's
    // arithmetic*, which is both journaled numbers — a summary that kept
    // the word "window" but lost `needed`/`window` would not satisfy it.
    let (needed, window) = refused[0];
    assert!(
        summary.contains(&needed.to_string()) && summary.contains(&window.to_string()),
        "summary must carry the journaled arithmetic (needed {needed}, window {window}), got: {summary}"
    );
}
