//! The pager's brief tests: the Task 1 acceptance set, driven directly
//! against `Pager<FakeSubstrate>`.
//!
//! **Split 2026-09-01** (carried-debt slice D): this file was 1266 lines. The
//! obligation set is in `pager_obligation_test.rs` and the admission-block
//! arc (drift-watch Tasks 2-4) in `pager_admission_block_test.rs`. The
//! fixtures every `pager_*` file used to carry its own fork of are in
//! `tests/common/pager.rs`.

mod common;

use bloomery_core::journal::{replay, sha256_hex, Event, PagerOpKind};
use bloomery_daemon::pager::*;
use common::pager::{fresh_dir, meta, pager_in, write_gguf};

// ---------------------------------------------------------------------------
// Brief tests
// ---------------------------------------------------------------------------

/// The journaled eviction story from the brief.
///
/// **One line deviates from the brief's listing** (`p.suspend(&b.id)` before
/// the final infer) because the brief's version cannot pass against the
/// residency law it is built on. With both agents' windows equal (`K` bytes
/// each) and the free-VRAM probe pinned at `F = 300 MiB`:
///
/// - step 2 evicting `a` requires `K > F − K` (b doesn't fit beside a), while
/// - step 3 placing `a` beside the still-resident, higher-priority `b`
///   requires `K ≤ F − K`.
///
/// Those are mutually exclusive for any `K`, and `a` (priority 50) may never
/// evict `b` (priority 100) — `plan_residency` refuses to evict a
/// same-or-higher-priority resident, pinned by
/// `bloomery-core/tests/scheduler_test.rs::never_evicts_busy_or_equal_priority`.
/// Suspending `b` first frees the VRAM honestly and leaves both of the
/// brief's assertions (EvictSave for `a`, then ResumeLoad for `a` from
/// `"ram"`) exactly as written.
#[test]
fn eviction_under_pressure_saves_image_and_journals() {
    let dir = fresh_dir("bloomery-pager-test");
    // free VRAM fits exactly one 4096-token qwen-geometry context
    // (4096 * 56 KiB = 224 MiB) + slack
    let (mut p, jpath, _) = pager_in(&dir, 4, Some(300 * 1024 * 1024));
    let gguf = write_gguf(&dir, "fake.gguf", b"weights");
    p.register_model("qwen", &gguf, meta(1000), None).unwrap();
    let a = p.create_agent("qwen", 50, None, 10_000).unwrap();
    let b = p.create_agent("qwen", 100, None, 10_000).unwrap();
    p.infer(&a.id, "hello from a", 16, None).unwrap(); // a becomes resident
    p.infer(&b.id, "hello from b", 16, None).unwrap(); // must evict a (lower priority)
    let events = replay(&jpath).unwrap();
    assert!(events.iter().any(|e| matches!(e,
        Event::PagerOp { id, op: PagerOpKind::EvictSave, .. } if id == &a.id)));
    // an evicted image goes to RAM, and says so
    assert!(events.iter().any(|e| matches!(e,
        Event::PagerOp { id, op: PagerOpKind::EvictSave, image_tier, .. }
            if id == &a.id && image_tier == "ram")));

    p.suspend(&b.id).unwrap(); // see the deviation note above
    p.infer(&a.id, "back again", 16, None).unwrap(); // resumes a from RAM image
    let events = replay(&jpath).unwrap();
    assert!(events.iter().any(|e| matches!(e,
        Event::PagerOp { id, op: PagerOpKind::ResumeLoad, image_tier, .. }
            if id == &a.id && image_tier == "ram")));
}

/// The eviction story again, but pinned as an **ordered** journal, by index.
///
/// `.any()` assertions cannot tell "these events happened" from "these events
/// happened in this order", and order is the whole value of the journal: a
/// replay has to show the decision before the eviction, the eviction before
/// the inference it made room for, and the request before the answer.
#[test]
fn the_eviction_story_is_journaled_in_order_with_a_faithful_prompt() {
    let dir = fresh_dir("bloomery-pager-order");
    let (mut p, jpath, _) = pager_in(&dir, 4, Some(300 * 1024 * 1024));
    let gguf = write_gguf(&dir, "fake.gguf", b"weights");
    p.register_model("qwen", &gguf, meta(1000), None).unwrap();
    let a = p.create_agent("qwen", 50, None, 10_000).unwrap();
    let b = p.create_agent("qwen", 100, None, 10_000).unwrap();
    p.infer(&a.id, "hello from a", 16, None).unwrap();
    let prompt = "hello from b";
    p.infer(&b.id, prompt, 16, None).unwrap();

    let events = replay(&jpath).unwrap();
    // Weights load once and stay loaded — the cold-switch bench reads this.
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e, Event::ModelLoaded { model, .. } if model == "qwen"))
            .count(),
        1,
        "{events:?}"
    );

    let start = events
        .iter()
        .position(|e| matches!(e, Event::SchedulerDecision { decision, .. } if decision == "evict"))
        .expect("the eviction decision is journaled");
    match &events[start] {
        Event::SchedulerDecision { id, evicted, .. } => {
            assert_eq!(id, &b.id);
            assert_eq!(evicted, &vec![a.id.clone()]);
        }
        other => panic!("{other:?}"),
    }

    let mut i = start + 1;
    match &events[i] {
        Event::PagerOp {
            id,
            op: PagerOpKind::EvictSave,
            image_tier,
            ..
        } => {
            assert_eq!(id, &a.id);
            assert_eq!(image_tier, "ram");
        }
        other => panic!("expected EvictSave at {i}, got {other:?}"),
    }

    i += 1;
    // ModelLoaded appears here only when the model was still cold.
    if matches!(events[i], Event::ModelLoaded { .. }) {
        i += 1;
    }
    match &events[i] {
        Event::InferStarted {
            id,
            prompt: journaled,
            prompt_sha256,
        } => {
            assert_eq!(id, &b.id);
            assert_eq!(journaled, prompt, "the prompt is journaled verbatim");
            assert_eq!(
                prompt_sha256,
                &sha256_hex(prompt),
                "and hashed, so the record survives a redacted or truncated log"
            );
        }
        other => panic!("expected InferStarted at {i}, got {other:?}"),
    }

    i += 1;
    match &events[i] {
        Event::InferCompleted {
            id,
            prompt_tokens,
            completion_tokens,
            ..
        } => {
            assert_eq!(id, &b.id);
            assert_eq!((*prompt_tokens, *completion_tokens), (8, 4));
        }
        other => panic!("expected InferCompleted at {i}, got {other:?}"),
    }
}

#[test]
fn oversized_prompt_is_refused_with_arithmetic_never_truncated() {
    let dir = fresh_dir("bloomery-pager-test2");
    let (mut p, _, _) = pager_in(&dir, 0, Some(10u64.pow(9)));
    let gguf = write_gguf(&dir, "fake.gguf", b"w");
    p.register_model("qwen", &gguf, meta(1000), None).unwrap();
    let a = p.create_agent("qwen", 50, Some(64), 10_000).unwrap(); // 64-token window
    let big = "x".repeat(10_000);
    match p.infer(&a.id, &big, 16, None) {
        Err(PagerError::PromptTooLarge {
            needed_tokens,
            window_tokens,
        }) => {
            assert!(needed_tokens > 64);
            assert_eq!(window_tokens, 64);
        }
        other => panic!("expected PromptTooLarge, got {other:?}"),
    }
    // never truncated, and the substrate was never asked to do it
    assert!(!p.substrate().calls().iter().any(|c| c.starts_with("infer")));
}

#[test]
fn budget_exhaustion_refuses_before_the_call() {
    let dir = fresh_dir("bloomery-pager-test3");
    let (mut p, jpath, _) = pager_in(&dir, 0, Some(10u64.pow(9)));
    let gguf = write_gguf(&dir, "fake.gguf", b"w");
    p.register_model("qwen", &gguf, meta(1000), None).unwrap();
    let a = p.create_agent("qwen", 50, None, 10).unwrap(); // 10-token budget
    match p.infer(&a.id, "hi", 100, None) {
        Err(PagerError::Budget {
            remaining: 10,
            requested: 100,
        }) => {}
        other => panic!("expected Budget, got {other:?}"),
    }
    let events = replay(&jpath).unwrap();
    assert!(events.iter().any(|e| matches!(e,
        Event::BudgetRefused { id, remaining: 10, requested: 100 } if id == &a.id)));
    assert!(p.substrate().calls().is_empty(), "no substrate work at all");
}
