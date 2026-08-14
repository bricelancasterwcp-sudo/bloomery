//! Pager behavior tests — every one of them GPU-free, driving
//! [`FakeSubstrate`] rather than llama.cpp.
//!
//! The first three tests come from the Task 13 brief. The rest pin the
//! obligations carried in from earlier tasks' reviews: a rejected KV image
//! is a cold start and never an error, a refusal never touches the
//! substrate, and every paging decision leaves a journal record.

use bloomery_core::journal::{replay, Event, Journal, PagerOpKind};
use bloomery_daemon::agents::ImageStore;
use bloomery_daemon::pager::*;
use bloomery_substrate::{fake::FakeSubstrate, Reply};
use std::path::{Path, PathBuf};

fn ok(text: &str) -> Reply {
    Reply {
        text: text.into(),
        prompt_tokens: Some(8),
        completion_tokens: Some(4),
        duration_ms: 3,
    }
}

fn meta() -> bloomery_core::gguf::GgufMeta {
    bloomery_core::gguf::GgufMeta {
        arch: "qwen2".into(),
        layers: 28,
        kv_heads: 4,
        head_dim: 128,
        training_ctx: 4096,
        weights_bytes: 1000,
    }
}

/// A clean scratch dir per test, so runs never share journals or images.
fn fresh_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Builds a pager over a fake substrate with `replies` scripted and a
/// constant free-VRAM probe.
fn pager_in(
    dir: &Path,
    replies: usize,
    free_vram: Option<u64>,
) -> (Pager<FakeSubstrate>, PathBuf, PathBuf) {
    let jpath = dir.join("j.jsonl");
    let journal = Journal::open(&jpath).unwrap();
    let imgdir = dir.join("img");
    let images = ImageStore::new(&imgdir).unwrap();
    let mut fake = FakeSubstrate::new();
    for _ in 0..replies {
        fake.script_reply(ok("r"));
    }
    let p = Pager::new(fake, journal, images, Box::new(move || free_vram));
    (p, jpath, imgdir)
}

fn write_gguf(dir: &Path, contents: &[u8]) -> PathBuf {
    let gguf = dir.join("fake.gguf");
    std::fs::write(&gguf, contents).unwrap();
    gguf
}

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
    let gguf = write_gguf(&dir, b"weights");
    p.register_model("qwen", &gguf, meta(), None).unwrap();
    let a = p.create_agent("qwen", 50, None, 10_000).unwrap();
    let b = p.create_agent("qwen", 100, None, 10_000).unwrap();
    p.infer(&a.id, "hello from a", 16).unwrap(); // a becomes resident
    p.infer(&b.id, "hello from b", 16).unwrap(); // must evict a (lower priority)
    let events = replay(&jpath).unwrap();
    assert!(events.iter().any(|e| matches!(e,
        Event::PagerOp { id, op: PagerOpKind::EvictSave, .. } if id == &a.id)));
    // an evicted image goes to RAM, and says so
    assert!(events.iter().any(|e| matches!(e,
        Event::PagerOp { id, op: PagerOpKind::EvictSave, image_tier, .. }
            if id == &a.id && image_tier == "ram")));

    p.suspend(&b.id).unwrap(); // see the deviation note above
    p.infer(&a.id, "back again", 16).unwrap(); // resumes a from RAM image
    let events = replay(&jpath).unwrap();
    assert!(events.iter().any(|e| matches!(e,
        Event::PagerOp { id, op: PagerOpKind::ResumeLoad, image_tier, .. }
            if id == &a.id && image_tier == "ram")));
}

#[test]
fn oversized_prompt_is_refused_with_arithmetic_never_truncated() {
    let dir = fresh_dir("bloomery-pager-test2");
    let (mut p, _, _) = pager_in(&dir, 0, Some(10u64.pow(9)));
    let gguf = write_gguf(&dir, b"w");
    p.register_model("qwen", &gguf, meta(), None).unwrap();
    let a = p.create_agent("qwen", 50, Some(64), 10_000).unwrap(); // 64-token window
    let big = "x".repeat(10_000);
    match p.infer(&a.id, &big, 16) {
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
    let gguf = write_gguf(&dir, b"w");
    p.register_model("qwen", &gguf, meta(), None).unwrap();
    let a = p.create_agent("qwen", 50, None, 10).unwrap(); // 10-token budget
    match p.infer(&a.id, "hi", 100) {
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

// ---------------------------------------------------------------------------
// Obligation tests
// ---------------------------------------------------------------------------

/// Residency arithmetic is pre-checked: a request that cannot be placed is
/// refused *before* any context is created, never inferred from an
/// allocation failure (llama.cpp #22629 is the shipped counterexample).
#[test]
fn residency_refusal_is_pre_checked_and_never_touches_the_substrate() {
    let dir = fresh_dir("bloomery-pager-refuse");
    let (mut p, jpath, _) = pager_in(&dir, 2, Some(300 * 1024 * 1024));
    let gguf = write_gguf(&dir, b"weights");
    p.register_model("qwen", &gguf, meta(), None).unwrap();
    let high = p.create_agent("qwen", 200, None, 10_000).unwrap();
    let low = p.create_agent("qwen", 10, None, 10_000).unwrap();
    p.infer(&high.id, "first", 16).unwrap();
    let calls_before = p.substrate().calls().len();

    match p.infer(&low.id, "second", 16) {
        Err(PagerError::Refused {
            needed,
            free,
            reclaimable,
        }) => {
            assert_eq!(needed, 4096 * 57344);
            assert_eq!(free, 300 * 1024 * 1024 - 4096 * 57344);
            assert_eq!(
                reclaimable, 0,
                "a higher-priority resident is not reclaimable"
            );
        }
        other => panic!("expected Refused, got {other:?}"),
    }
    assert_eq!(
        p.substrate().calls().len(),
        calls_before,
        "refusal must not create a context or call the substrate at all"
    );
    let events = replay(&jpath).unwrap();
    assert!(events.iter().any(|e| matches!(e,
        Event::SchedulerDecision { id, decision, .. } if id == &low.id && decision == "refuse")));
    assert!(events
        .iter()
        .any(|e| matches!(e, Event::Refusal { id, .. } if id == &low.id)));
}

/// An image saved under a different model digest is *invalidated*, not an
/// error: cold start, journal `Degraded`, keep serving.
#[test]
fn stale_image_digest_cold_starts_and_journals_degraded() {
    let dir = fresh_dir("bloomery-pager-stale");
    let (mut p, jpath, _) = pager_in(&dir, 2, Some(10u64.pow(9)));
    let gguf = write_gguf(&dir, b"weights-v1");
    p.register_model("qwen", &gguf, meta(), None).unwrap();
    let a = p.create_agent("qwen", 50, None, 10_000).unwrap();
    p.infer(&a.id, "one", 16).unwrap();
    p.suspend(&a.id).unwrap();

    // The weights file changed on disk: same name, new digest.
    write_gguf(&dir, b"weights-v2-with-a-different-length");
    p.register_model("qwen", &gguf, meta(), None).unwrap();

    p.infer(&a.id, "two", 16).unwrap(); // still serves, from a cold context
    let events = replay(&jpath).unwrap();
    assert!(
        events
            .iter()
            .any(|e| matches!(e, Event::Degraded { reason } if reason.contains("stale"))),
        "stale image must be journaled as degradation: {events:?}"
    );
    assert!(
        !events.iter().any(|e| matches!(
            e,
            Event::PagerOp {
                op: PagerOpKind::ResumeLoad,
                ..
            }
        )),
        "a stale image is never restored"
    );
}

/// A spilled image whose bytes no longer match its recorded length is the
/// same handling class as a stale digest: cold start + `Degraded`.
#[test]
fn corrupt_spilled_image_cold_starts_and_journals_degraded() {
    let dir = fresh_dir("bloomery-pager-corrupt");
    let (mut p, jpath, imgdir) = pager_in(&dir, 2, Some(10u64.pow(9)));
    let gguf = write_gguf(&dir, b"weights");
    p.register_model("qwen", &gguf, meta(), None).unwrap();
    let a = p.create_agent("qwen", 50, None, 10_000).unwrap();
    p.infer(&a.id, "one", 16).unwrap();
    p.suspend(&a.id).unwrap();

    // Corrupt the spilled image on disk (length no longer matches).
    let spilled = std::fs::read_dir(&imgdir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .next()
        .expect("suspend spills exactly one image file");
    let mut bytes = std::fs::read(&spilled).unwrap();
    bytes.extend_from_slice(b"garbage");
    std::fs::write(&spilled, &bytes).unwrap();

    p.infer(&a.id, "two", 16).unwrap();
    let events = replay(&jpath).unwrap();
    assert!(
        events
            .iter()
            .any(|e| matches!(e, Event::Degraded { reason } if reason.contains("corrupt"))),
        "corrupt image must be journaled as degradation: {events:?}"
    );
}

/// suspend -> NVMe -> resume round-trips the conversation, and says which
/// tier it actually used both ways.
#[test]
fn suspend_resume_round_trips_the_kv_image_through_nvme() {
    let dir = fresh_dir("bloomery-pager-roundtrip");
    let (mut p, jpath, _) = pager_in(&dir, 2, Some(10u64.pow(9)));
    let gguf = write_gguf(&dir, b"weights");
    p.register_model("qwen", &gguf, meta(), None).unwrap();
    let a = p.create_agent("qwen", 50, None, 10_000).unwrap();
    p.infer(&a.id, "one", 16).unwrap();
    p.suspend(&a.id).unwrap();
    p.resume(&a.id).unwrap(); // ensure resident, no infer
    p.infer(&a.id, "two", 16).unwrap();

    // FakeSubstrate mints context handles 1, 2, ...; the resumed context is
    // the second one it created.
    assert_eq!(p.substrate().ctx_history(2), Some("one\ntwo"));
    let events = replay(&jpath).unwrap();
    assert!(events.iter().any(|e| matches!(e,
        Event::PagerOp { id, op: PagerOpKind::SuspendSave, image_tier, .. }
            if id == &a.id && image_tier == "nvme")));
    assert!(events.iter().any(|e| matches!(e,
        Event::PagerOp { id, op: PagerOpKind::ResumeLoad, image_tier, .. }
            if id == &a.id && image_tier == "nvme")));
    // resume() does not infer
    assert_eq!(
        p.substrate()
            .calls()
            .iter()
            .filter(|c| c.starts_with("infer"))
            .count(),
        2
    );
}

/// `free_vram() == None` is unmeasured, never zero: say so once, then fall
/// back to a residency-count cap of one.
#[test]
fn unmeasured_vram_journals_degraded_once_and_caps_residency_at_one() {
    let dir = fresh_dir("bloomery-pager-novram");
    let (mut p, jpath, _) = pager_in(&dir, 3, None);
    let gguf = write_gguf(&dir, b"weights");
    p.register_model("qwen", &gguf, meta(), None).unwrap();
    let a = p.create_agent("qwen", 50, None, 10_000).unwrap();
    let b = p.create_agent("qwen", 100, None, 10_000).unwrap();
    // The VRAM term is skipped, not treated as zero free bytes.
    assert_eq!(a.window_tokens, 4096);
    assert_eq!(a.bound_by, "training_ctx");

    p.infer(&a.id, "hello", 16).unwrap();
    p.infer(&b.id, "hello", 16).unwrap(); // cap of 1 -> a is evicted

    let events = replay(&jpath).unwrap();
    let degraded = events
        .iter()
        .filter(|e| matches!(e, Event::Degraded { reason } if reason.contains("vram unmeasured")))
        .count();
    assert_eq!(degraded, 1, "said once, not once per probe");
    assert!(events.iter().any(|e| matches!(e,
        Event::PagerOp { id, op: PagerOpKind::EvictSave, .. } if id == &a.id)));
}

/// A reply without token stats is an infrastructure failure (law 4), never a
/// model failure: journaled as a contract violation and surfaced as one.
#[test]
fn missing_stats_is_a_contract_violation_not_a_reply() {
    let dir = fresh_dir("bloomery-pager-contract");
    let (mut p, jpath, _) = pager_in(&dir, 0, Some(10u64.pow(9)));
    let gguf = write_gguf(&dir, b"weights");
    p.register_model("qwen", &gguf, meta(), None).unwrap();
    let a = p.create_agent("qwen", 50, None, 10_000).unwrap();

    match p.infer(&a.id, "hi", 16) {
        // the fake's script is empty -> substrate error, not a silent reply
        Err(PagerError::Substrate(msg)) => assert!(msg.contains("script exhausted")),
        other => panic!("expected Substrate, got {other:?}"),
    }

    let dir2 = fresh_dir("bloomery-pager-contract2");
    let jpath2 = dir2.join("j.jsonl");
    let journal = Journal::open(&jpath2).unwrap();
    let images = ImageStore::new(&dir2.join("img")).unwrap();
    let mut fake = FakeSubstrate::new();
    fake.script_reply(Reply {
        text: "no stats".into(),
        prompt_tokens: None,
        completion_tokens: Some(4),
        duration_ms: 1,
    });
    let mut p2 = Pager::new(fake, journal, images, Box::new(|| Some(10u64.pow(9))));
    let gguf2 = write_gguf(&dir2, b"weights");
    p2.register_model("qwen", &gguf2, meta(), None).unwrap();
    let a2 = p2.create_agent("qwen", 50, None, 10_000).unwrap();
    match p2.infer(&a2.id, "hi", 16) {
        Err(PagerError::Contract(_)) => {}
        other => panic!("expected Contract, got {other:?}"),
    }
    let events = replay(&jpath2).unwrap();
    assert!(events.iter().any(|e| matches!(e,
        Event::ContractViolation { id, kind } if id == &a2.id && kind == "MissingStats")));
    // nothing was charged against the budget for a violated reply
    let status = p2.status();
    assert_eq!(status.agents[0].budget_spent, 0);
    let _ = jpath;
}

/// `unload_model` pages out every context still holding the model, then
/// journals the unload the cold-switch bench measures.
#[test]
fn unload_model_pages_out_holders_and_journals() {
    let dir = fresh_dir("bloomery-pager-unload");
    let (mut p, jpath, _) = pager_in(&dir, 2, Some(10u64.pow(9)));
    let gguf = write_gguf(&dir, b"weights");
    p.register_model("qwen", &gguf, meta(), None).unwrap();
    let a = p.create_agent("qwen", 50, None, 10_000).unwrap();
    p.infer(&a.id, "one", 16).unwrap();
    p.unload_model("qwen").unwrap();

    let events = replay(&jpath).unwrap();
    assert!(events
        .iter()
        .any(|e| matches!(e, Event::ModelUnloaded { model } if model == "qwen")));
    assert!(events.iter().any(|e| matches!(e,
        Event::PagerOp { id, op: PagerOpKind::SuspendSave, .. } if id == &a.id)));
    assert!(!p.status().models[0].loaded);
    // and it comes back: the model reloads on demand, image restores
    p.infer(&a.id, "two", 16).unwrap();
    assert_eq!(p.substrate().ctx_history(2), Some("one\ntwo"));
}

/// A substrate that rejects every image with the cross-task
/// [`bloomery_substrate::STATE_SIZE_MISMATCH`] marker — what llama.cpp does
/// when a saved state no longer fits the destination context's geometry.
/// `FakeSubstrate` restores anything, so this is the only way to reach that
/// path GPU-free.
#[derive(Default)]
struct MismatchSubstrate {
    calls: Vec<String>,
    next_ctx: u64,
}

impl MismatchSubstrate {
    fn calls(&self) -> &[String] {
        &self.calls
    }
}

impl bloomery_substrate::Substrate for MismatchSubstrate {
    fn load_model(
        &mut self,
        _p: &Path,
        _n: u32,
    ) -> Result<bloomery_substrate::ModelHandle, bloomery_substrate::SubstrateError> {
        self.calls.push("load_model".into());
        Ok(1)
    }
    fn unload_model(
        &mut self,
        _m: bloomery_substrate::ModelHandle,
    ) -> Result<(), bloomery_substrate::SubstrateError> {
        Ok(())
    }
    fn create_context(
        &mut self,
        _m: bloomery_substrate::ModelHandle,
        _n: u32,
    ) -> Result<bloomery_substrate::CtxHandle, bloomery_substrate::SubstrateError> {
        self.next_ctx += 1;
        self.calls
            .push(format!("create_context:c{}", self.next_ctx));
        Ok(self.next_ctx)
    }
    fn destroy_context(
        &mut self,
        c: bloomery_substrate::CtxHandle,
    ) -> Result<(), bloomery_substrate::SubstrateError> {
        self.calls.push(format!("destroy_context:c{c}"));
        Ok(())
    }
    fn infer(
        &mut self,
        c: bloomery_substrate::CtxHandle,
        prompt: &str,
        _max: u32,
    ) -> Result<Reply, bloomery_substrate::SubstrateError> {
        self.calls.push(format!("infer:c{c}"));
        Ok(ok(prompt))
    }
    fn save_state(
        &mut self,
        _c: bloomery_substrate::CtxHandle,
    ) -> Result<Vec<u8>, bloomery_substrate::SubstrateError> {
        Ok(vec![1, 2, 3])
    }
    fn load_state(
        &mut self,
        c: bloomery_substrate::CtxHandle,
        _b: &[u8],
    ) -> Result<(), bloomery_substrate::SubstrateError> {
        self.calls.push(format!("load_state:c{c}"));
        Err(bloomery_substrate::SubstrateError::State(format!(
            "state {}: 3 vs 4",
            bloomery_substrate::STATE_SIZE_MISMATCH
        )))
    }
}

/// An image the substrate rejects for size is invalidated, exactly like a
/// stale digest — and the half-written destination context is destroyed
/// rather than reused, because a failed restore leaves it partial.
#[test]
fn image_rejected_for_size_mismatch_cold_starts_on_a_fresh_context() {
    let dir = fresh_dir("bloomery-pager-mismatch");
    let jpath = dir.join("j.jsonl");
    let journal = Journal::open(&jpath).unwrap();
    let images = ImageStore::new(&dir.join("img")).unwrap();
    let mut p = Pager::new(
        MismatchSubstrate::default(),
        journal,
        images,
        Box::new(|| Some(10u64.pow(9))),
    );
    let gguf = write_gguf(&dir, b"weights");
    p.register_model("qwen", &gguf, meta(), None).unwrap();
    let a = p.create_agent("qwen", 50, None, 10_000).unwrap();
    p.infer(&a.id, "one", 16).unwrap();
    p.suspend(&a.id).unwrap();

    // Still serves — a rejected image is invalidation, never a failed request.
    p.infer(&a.id, "two", 16).unwrap();

    let events = replay(&jpath).unwrap();
    assert!(
        events
            .iter()
            .any(|e| matches!(e, Event::Degraded { reason } if reason.contains("invalidated"))),
        "{events:?}"
    );
    assert!(!events.iter().any(|e| matches!(
        e,
        Event::PagerOp {
            op: PagerOpKind::ResumeLoad,
            ..
        }
    )));

    let calls = p.substrate().calls();
    let at = calls.iter().position(|c| c == "load_state:c2").unwrap();
    assert_eq!(
        calls[at + 1],
        "destroy_context:c2",
        "never retried in place"
    );
    assert_eq!(calls[at + 2], "create_context:c3");
    assert_eq!(calls[at + 3], "infer:c3");
}

#[test]
fn unknown_model_and_unknown_agent_are_named() {
    let dir = fresh_dir("bloomery-pager-unknown");
    let (mut p, _, _) = pager_in(&dir, 0, Some(10u64.pow(9)));
    match p.create_agent("nope", 50, None, 10) {
        Err(PagerError::UnknownModel(m)) => assert_eq!(m, "nope"),
        other => panic!("expected UnknownModel, got {other:?}"),
    }
    match p.infer("a99", "hi", 1) {
        Err(PagerError::UnknownAgent(id)) => assert_eq!(id, "a99"),
        other => panic!("expected UnknownAgent, got {other:?}"),
    }
}

/// Agent ids are the pager's to guarantee unique — `plan_residency` is
/// unspecified for duplicates.
#[test]
fn agent_ids_are_unique_and_status_is_a_snapshot() {
    let dir = fresh_dir("bloomery-pager-status");
    let (mut p, _, _) = pager_in(&dir, 1, Some(10u64.pow(9)));
    let gguf = write_gguf(&dir, b"weights");
    p.register_model("qwen", &gguf, meta(), None).unwrap();
    let ids: Vec<String> = (0..3)
        .map(|_| p.create_agent("qwen", 50, None, 100).unwrap().id)
        .collect();
    assert_eq!(ids, vec!["a1", "a2", "a3"]);

    p.infer("a1", "hi", 16).unwrap();
    let status = p.status();
    assert_eq!(status.agents.len(), 3);
    assert_eq!(status.agents[0].state, "resident");
    assert_eq!(status.agents[1].state, "fresh");
    assert_eq!(status.agents[0].budget_granted, 100);
    assert_eq!(status.agents[0].budget_spent, 12);
    assert_eq!(status.resident_kv_bytes, 4096 * 57344);
    assert!(status.models[0].loaded);
    // serializable snapshot
    let json = serde_json::to_string(&status).unwrap();
    assert!(json.contains("\"bound_by\":\"training_ctx\""));
}
