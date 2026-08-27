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

/// One scripted model turn. The identity test scripts none (a refused prompt
/// never reaches the substrate), but every ladder-walk test below drives real
/// turns through it once a rung fits.
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
    // Every rung test sizes its window from its own expected strings, which
    // only measures anything if the requested cap is the term that WINS
    // `usable_window`. If VRAM, the training ctx, or a measured ceiling ever
    // bound lower, the caps would silently stop being the lever and the walk
    // tests would pass (or fail) for reasons unrelated to the ladder.
    if let Some(cap) = window_cap {
        assert_eq!(
            info.window_tokens, cap,
            "the requested cap must be the binding window term (bound_by {})",
            info.bound_by
        );
    }
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

/// `transcript_entry`'s pinned shape, restated (not imported) per the
/// golden rule: `"\n[step {step} {verb}] {outcome}\n{content}\n"`.
fn full_entry(step: u32, verb: &str, outcome: &str, content: &str) -> String {
    format!("\n[step {step} {verb}] {outcome}\n{content}\n")
}

/// Spec §2 rung 3/4: the elided form is the full form minus the content line.
fn elided(step: u32, verb: &str, outcome: &str) -> String {
    format!("\n[step {step} {verb}] {outcome}\n")
}

/// Spec §3: the pinned head note, `{a}-{b}` always, one trailing newline.
fn note(a: u32, b: u32) -> String {
    format!("[context note: contents of steps {a}-{b} elided to fit the window; outcomes retained — re-read files if needed]\n")
}

#[test]
fn ladder_on_lands_rung_2_by_dropping_the_memory_block() {
    // Spec §8 test 7 + §2 rung 2: the rung-2 bytes ARE the memory-off
    // rendering — which is exactly what the public serving-faithful wrapper
    // (permanently memory-None) renders. Full-prompt byte equality against
    // that independent comparator.
    let dir = fresh_dir("rung2");
    let big_memory = "memory ".repeat(2000);
    let rung2_expected =
        render_task_prompt(GOAL, PatchCodec::SearchReplace, EnvelopeLens::V1, &[], "");
    let (mut pager, agent_id) = fixture(
        &dir,
        Some(cap_fitting(&rung2_expected)),
        vec![scripted("<action verb=\"done\">\nok\n</action>")],
    );
    let mut journal = Journal::open(&dir.join("task.jsonl")).unwrap();
    let spec = TaskSpec {
        memory_block: Some(big_memory.clone()),
        ..ladder_spec(
            sandbox_grant(&dir),
            std::fs::canonicalize(&dir).unwrap(),
            true,
        )
    };
    let result = run_task(&mut pager, &agent_id, &spec, &mut journal);
    assert_eq!(result.status, TaskStatus::Done);
    let history = pager.substrate().ctx_history(1).expect("agent ctx exists");
    assert_eq!(
        history, rung2_expected,
        "rung-2 bytes == memory-off bytes, exactly"
    );
    assert!(!history.contains(&big_memory), "the memory block is gone");
    assert_eq!(
        refusals(&dir).len(),
        1,
        "rung 1 refused once, rung 2 accepted"
    );
    assert_eq!(result.steps.len(), 1);
    assert_eq!(result.steps[0].rung, 2, "the ledger records the SENT rung");
}

#[test]
fn every_attempt_rewalks_from_rung_1() {
    // Spec §4: no ratchet. Two steps, both forced past rung 1 by the same
    // big memory block — TWO refusals proves step 2 tried rung 1 again
    // (a ratchet would leave exactly one).
    let dir = fresh_dir("rewalk");
    let big_memory = "memory ".repeat(2000);
    // Step 2's rung-2 prompt carries step 1's read entry, so it is the
    // LARGER of the two rung-2 prompts; sizing the cap on it lets both
    // steps' rung 2 fit while both rung 1s still refuse.
    std::fs::write(dir.join("f.txt"), "alpha\n").unwrap();
    let step1_reply = "<action verb=\"read\" path=\"f.txt\">\n</action>";
    let step2_transcript = full_entry(1, "read", "read 6 bytes", "alpha\n");
    let rung2_step2 = render_task_prompt(
        GOAL,
        PatchCodec::SearchReplace,
        EnvelopeLens::V1,
        &[],
        &step2_transcript,
    );
    let (mut pager, agent_id) = fixture(
        &dir,
        Some(cap_fitting(&rung2_step2)),
        vec![
            scripted(step1_reply),
            scripted("<action verb=\"done\">\nok\n</action>"),
        ],
    );
    let mut journal = Journal::open(&dir.join("task.jsonl")).unwrap();
    let spec = TaskSpec {
        memory_block: Some(big_memory),
        ..ladder_spec(
            sandbox_grant(&dir),
            std::fs::canonicalize(&dir).unwrap(),
            true,
        )
    };
    let result = run_task(&mut pager, &agent_id, &spec, &mut journal);
    assert_eq!(result.status, TaskStatus::Done);
    assert_eq!(
        refusals(&dir).len(),
        2,
        "one rung-1 refusal PER STEP — step 2 re-walked from rung 1"
    );
    let rungs: Vec<u32> = result.steps.iter().map(|s| s.rung).collect();
    assert_eq!(rungs, vec![2, 2]);
}

#[test]
fn ladder_lands_rung_3_eliding_old_entries_behind_the_head_note() {
    // Spec §8 tests 2+6, §2 rung 3, §3: three entries (big, small, small);
    // the window fits last-2-full + entry-1's header + the note, but not
    // all three full. The degraded tail is pinned byte-for-byte.
    let dir = fresh_dir("rung3");
    let big = "x".repeat(2400);
    std::fs::write(dir.join("big.txt"), &big).unwrap();
    std::fs::write(dir.join("s.txt"), "small\n").unwrap();
    let read_big = "<action verb=\"read\" path=\"big.txt\">\n</action>";
    let read_small = "<action verb=\"read\" path=\"s.txt\">\n</action>";
    // Entry contents mirror exec_read's real outcome/content shapes.
    let e1 = full_entry(1, "read", "read 2400 bytes", &big);
    let e2 = full_entry(2, "read", "read 6 bytes", "small\n");
    let e3 = full_entry(3, "read", "read 6 bytes", "small\n");
    let rung1_step4 = render_task_prompt(
        GOAL,
        PatchCodec::SearchReplace,
        EnvelopeLens::V1,
        &[],
        &format!("{e1}{e2}{e3}"),
    );
    let rung3_tail = format!(
        "{}{}{e2}{e3}",
        note(1, 1),
        elided(1, "read", "read 2400 bytes")
    );
    let rung3_step4 = render_task_prompt(
        GOAL,
        PatchCodec::SearchReplace,
        EnvelopeLens::V1,
        &[],
        &rung3_tail,
    );
    // The cap is sized to STEP 3's rung-1 prompt (entries 1-2 full, the
    // big one included) — the largest prompt that must still fit at rung 1
    // so steps 1-3 stay undegraded. Step 4's rung-1 adds e3 (~25 tokens)
    // on top and refuses; its rung-3 rendering (big e1 elided) sits far
    // below the cap and fits.
    let rung1_step3 = render_task_prompt(
        GOAL,
        PatchCodec::SearchReplace,
        EnvelopeLens::V1,
        &[],
        &format!("{e1}{e2}"),
    );
    let cap = cap_fitting(&rung1_step3);
    assert!(
        cap < cap_fitting(&rung1_step4),
        "sizing sanity: step 4's rung 1 must refuse"
    );
    assert!(
        cap >= cap_fitting(&rung3_step4),
        "sizing sanity: step 4's rung 3 must fit"
    );
    let (mut pager, agent_id) = fixture(
        &dir,
        Some(cap),
        vec![
            scripted(read_big),
            scripted(read_small),
            scripted(read_small),
            scripted("<action verb=\"done\">\nok\n</action>"),
        ],
    );
    let mut journal = Journal::open(&dir.join("task.jsonl")).unwrap();
    let spec = ladder_spec(
        sandbox_grant(&dir),
        std::fs::canonicalize(&dir).unwrap(),
        true,
    );
    let result = run_task(&mut pager, &agent_id, &spec, &mut journal);
    assert_eq!(result.status, TaskStatus::Done);
    let history = pager.substrate().ctx_history(1).expect("agent ctx exists");
    assert!(
        history.ends_with(&rung3_step4),
        "step 4's prompt is the pinned rung-3 rendering (note + elided e1 + full e2,e3)"
    );
    let rungs: Vec<u32> = result.steps.iter().map(|s| s.rung).collect();
    assert_eq!(rungs, vec![1, 1, 1, 3]);
    // Fixed ladder: step 4 refused rungs 1 AND 2 (identical bytes — no
    // memory block — so identical needed_tokens: the no-skip pin, §2).
    let r = refusals(&dir);
    assert_eq!(r.len(), 2);
    assert_eq!(
        r[0].0, r[1].0,
        "rung 2 == rung 1 bytes when memory is absent"
    );
}

#[test]
fn ladder_lands_rung_4_when_two_full_entries_are_too_many() {
    // Spec §2 rung 4: two big entries; the window fits one full entry plus
    // the other's header, not two full. Kills a MAX_RUNG 4->3 mutant.
    let dir = fresh_dir("rung4");
    let big = "y".repeat(2400);
    std::fs::write(dir.join("b.txt"), &big).unwrap();
    let read_big = "<action verb=\"read\" path=\"b.txt\">\n</action>";
    let e1 = full_entry(1, "read", "read 2400 bytes", &big);
    let e2 = full_entry(2, "read", "read 2400 bytes", &big);
    let rung4_tail = format!("{}{}{e2}", note(1, 1), elided(1, "read", "read 2400 bytes"));
    let rung4_step3 = render_task_prompt(
        GOAL,
        PatchCodec::SearchReplace,
        EnvelopeLens::V1,
        &[],
        &rung4_tail,
    );
    let rung1_step2 =
        render_task_prompt(GOAL, PatchCodec::SearchReplace, EnvelopeLens::V1, &[], &e1);
    // One big entry (step 2's rung-1) must fit; two must not. rung-4's
    // prompt (~ one big + header + note) is the larger of the two "fits"
    // candidates, so cap on it admits both.
    let cap = cap_fitting(&rung4_step3).max(cap_fitting(&rung1_step2));
    let (mut pager, agent_id) = fixture(
        &dir,
        Some(cap),
        vec![
            scripted(read_big),
            scripted(read_big),
            scripted("<action verb=\"done\">\nok\n</action>"),
        ],
    );
    let mut journal = Journal::open(&dir.join("task.jsonl")).unwrap();
    let spec = ladder_spec(
        sandbox_grant(&dir),
        std::fs::canonicalize(&dir).unwrap(),
        true,
    );
    let result = run_task(&mut pager, &agent_id, &spec, &mut journal);
    assert_eq!(result.status, TaskStatus::Done);
    let history = pager.substrate().ctx_history(1).expect("agent ctx exists");
    assert!(
        history.ends_with(&rung4_step3),
        "step 3 sent the pinned rung-4 bytes"
    );
    let rungs: Vec<u32> = result.steps.iter().map(|s| s.rung).collect();
    assert_eq!(rungs, vec![1, 1, 4]);
    // Step 3 refused rungs 1, 2 (== 1: no memory), and 3 (two full entries
    // with only two entries total elides nothing — == rung 2, §2's
    // "renders identical ... refuses through it naturally").
    let r = refusals(&dir);
    assert_eq!(r.len(), 3);
    assert_eq!(r[0].0, r[1].0, "rung 2 bytes == rung 1 (no memory)");
    assert_eq!(
        r[1].0, r[2].0,
        "rung 3 with 2 entries elides nothing == rung 2"
    );
}

#[test]
fn rung_4_refusal_is_terminal_window_exhausted() {
    // Spec §2 refusal + §8 test 5: even rung 4 refuses -> WindowExhausted
    // with the pager arithmetic, after exactly 4 refusals (1,2,3,4).
    let dir = fresh_dir("terminal");
    let big_memory = "memory ".repeat(2000);
    // A cap below even the bare memory-less empty-transcript prompt: the
    // smallest thing rung 4 could render still refuses. Kept just above
    // STEP_MAX_TOKENS so the refusal is on prompt size, not on a window
    // that could not hold the completion budget alone.
    let (mut pager, agent_id) = fixture(&dir, Some(1030), vec![]);
    let mut journal = Journal::open(&dir.join("task.jsonl")).unwrap();
    let spec = TaskSpec {
        memory_block: Some(big_memory),
        ..ladder_spec(
            sandbox_grant(&dir),
            std::fs::canonicalize(&dir).unwrap(),
            true,
        )
    };
    let result = run_task(&mut pager, &agent_id, &spec, &mut journal);
    assert_eq!(result.status, TaskStatus::WindowExhausted);
    assert_eq!(refusals(&dir).len(), 4, "all four rungs tried, in order");
    assert_eq!(infer_count(&pager), 0);
    assert!(result
        .summary
        .expect("arithmetic summary")
        .contains("window"));
    assert!(
        result.steps.is_empty(),
        "no step row for a turn that never sent"
    );
}
