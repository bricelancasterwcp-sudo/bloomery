//! Pager behavior tests — every one of them GPU-free, driving
//! [`FakeSubstrate`] rather than llama.cpp.
//!
//! The first three tests come from the Task 13 brief. The rest pin the
//! obligations carried in from earlier tasks' reviews: a rejected KV image
//! is a cold start and never an error, a refusal never touches the
//! substrate, and every paging decision leaves a journal record.

use bloomery_core::journal::{replay, sha256_hex, Event, Journal, PagerOpKind};
use bloomery_daemon::agents::ImageStore;
use bloomery_daemon::pager::*;
use bloomery_substrate::{fake::FakeSubstrate, CtxHandle, ModelHandle, Reply, SubstrateError};
use std::collections::{HashMap, VecDeque};
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
    let gguf = write_gguf(&dir, b"weights");
    p.register_model("qwen", &gguf, meta(), None).unwrap();
    let a = p.create_agent("qwen", 50, None, 10_000).unwrap();
    let b = p.create_agent("qwen", 100, None, 10_000).unwrap();
    p.infer(&a.id, "hello from a", 16).unwrap();
    let prompt = "hello from b";
    p.infer(&b.id, prompt, 16).unwrap();

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

    // InferStarted is written *before* the substrate call, not after it: a
    // call that fails (or hangs, or takes the process down) must still leave
    // the record of what was being asked.
    let failed = replay(&jpath).unwrap();
    assert!(
        failed
            .iter()
            .any(|e| matches!(e, Event::InferStarted { id, prompt, .. }
                if id == &a.id && prompt == "hi")),
        "a failed call still journals what it was about to run: {failed:?}"
    );
    assert!(!failed
        .iter()
        .any(|e| matches!(e, Event::InferCompleted { .. })));
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

/// A substrate whose failures are scripted, for the paths `FakeSubstrate`
/// cannot reach: it saves anything and restores anything, so image
/// rejections, save faults and the substrate's own window refusal have no
/// other GPU-free route.
#[derive(Default)]
struct ScriptedSubstrate {
    calls: Vec<String>,
    next_ctx: u64,
    history: HashMap<u64, String>,
    /// FIFO errors for `load_state`; exhausted (or empty) = restore works.
    load_failures: VecDeque<String>,
    /// If set, every `save_state` fails with this message.
    save_failure: Option<String>,
    /// If set, every `infer` fails with this message.
    infer_failure: Option<String>,
}

impl ScriptedSubstrate {
    fn calls(&self) -> &[String] {
        &self.calls
    }
    fn ctx_history(&self, c: u64) -> Option<&str> {
        self.history.get(&c).map(String::as_str)
    }
}

impl bloomery_substrate::Substrate for ScriptedSubstrate {
    fn load_model(&mut self, _p: &Path, _n: u32) -> Result<ModelHandle, SubstrateError> {
        self.calls.push("load_model".into());
        Ok(1)
    }
    fn unload_model(&mut self, _m: ModelHandle) -> Result<(), SubstrateError> {
        Ok(())
    }
    fn create_context(&mut self, _m: ModelHandle, _n: u32) -> Result<CtxHandle, SubstrateError> {
        self.next_ctx += 1;
        self.calls
            .push(format!("create_context:c{}", self.next_ctx));
        self.history.insert(self.next_ctx, String::new());
        Ok(self.next_ctx)
    }
    fn destroy_context(&mut self, c: CtxHandle) -> Result<(), SubstrateError> {
        self.calls.push(format!("destroy_context:c{c}"));
        self.history.remove(&c);
        Ok(())
    }
    fn infer(&mut self, c: CtxHandle, prompt: &str, _max: u32) -> Result<Reply, SubstrateError> {
        self.calls.push(format!("infer:c{c}"));
        if let Some(msg) = &self.infer_failure {
            return Err(SubstrateError::Infer(msg.clone()));
        }
        let entry = self.history.entry(c).or_default();
        if !entry.is_empty() {
            entry.push('\n');
        }
        entry.push_str(prompt);
        Ok(ok(prompt))
    }
    fn save_state(&mut self, c: CtxHandle) -> Result<Vec<u8>, SubstrateError> {
        self.calls.push(format!("save_state:c{c}"));
        if let Some(msg) = &self.save_failure {
            return Err(SubstrateError::State(msg.clone()));
        }
        Ok(self
            .history
            .get(&c)
            .cloned()
            .unwrap_or_default()
            .into_bytes())
    }
    fn load_state(&mut self, c: CtxHandle, b: &[u8]) -> Result<(), SubstrateError> {
        self.calls.push(format!("load_state:c{c}"));
        if let Some(msg) = self.load_failures.pop_front() {
            return Err(SubstrateError::State(msg));
        }
        self.history
            .insert(c, String::from_utf8(b.to_vec()).unwrap_or_default());
        Ok(())
    }
}

/// Builds a pager over a [`ScriptedSubstrate`] with a roomy VRAM budget.
fn scripted_pager(
    dir: &Path,
    substrate: ScriptedSubstrate,
    budget: u64,
) -> (Pager<ScriptedSubstrate>, PathBuf) {
    let jpath = dir.join("j.jsonl");
    let journal = Journal::open(&jpath).unwrap();
    let images = ImageStore::new(&dir.join("img")).unwrap();
    let mut p = Pager::new(substrate, journal, images, Box::new(move || Some(budget)));
    let gguf = write_gguf(dir, b"weights");
    p.register_model("qwen", &gguf, meta(), None).unwrap();
    (p, jpath)
}

/// An image the substrate rejects for size is invalidated, exactly like a
/// stale digest — and the half-written destination context is destroyed
/// rather than reused, because a failed restore leaves it partial.
#[test]
fn image_rejected_for_size_mismatch_cold_starts_on_a_fresh_context() {
    let dir = fresh_dir("bloomery-pager-mismatch");
    let substrate = ScriptedSubstrate {
        load_failures: VecDeque::from(vec![format!(
            "state {}: 3 vs 4",
            bloomery_substrate::STATE_SIZE_MISMATCH
        )]),
        ..Default::default()
    };
    let (mut p, jpath) = scripted_pager(&dir, substrate, 10u64.pow(9));
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

/// `ImageStore::take` is destructive, so a restore that fails for a
/// *transient* reason must put the bytes back: the retry has to find the same
/// conversation, not a cold agent and no record of what was lost.
#[test]
fn a_transient_restore_failure_keeps_the_image_for_the_retry() {
    let dir = fresh_dir("bloomery-pager-retry");
    let substrate = ScriptedSubstrate {
        // Not a size mismatch: a fault, not an invalidation.
        load_failures: VecDeque::from(vec!["transient device error".to_string()]),
        ..Default::default()
    };
    let (mut p, jpath) = scripted_pager(&dir, substrate, 10u64.pow(9));
    let a = p.create_agent("qwen", 50, None, 10_000).unwrap();
    p.infer(&a.id, "one", 16).unwrap();
    p.suspend(&a.id).unwrap();

    match p.infer(&a.id, "two", 16) {
        Err(PagerError::Substrate(msg)) => assert!(msg.contains("transient device error")),
        other => panic!("expected Substrate, got {other:?}"),
    }
    assert!(!replay(&jpath).unwrap().iter().any(|e| matches!(
        e,
        Event::PagerOp {
            op: PagerOpKind::ResumeLoad,
            ..
        }
    )));

    // Retry: the substrate is healthy again and the image is still there.
    p.infer(&a.id, "two", 16).unwrap();
    let events = replay(&jpath).unwrap();
    assert!(
        events.iter().any(|e| matches!(e,
            Event::PagerOp { id, op: PagerOpKind::ResumeLoad, .. } if id == &a.id)),
        "the retry restored from the same image: {events:?}"
    );
    assert_eq!(
        p.substrate().ctx_history(3),
        Some("one\ntwo"),
        "the conversation survived the failed attempt"
    );
}

/// An eviction that aborts mid-flight must say so: the `SchedulerDecision`
/// naming the victim is already on disk, and a replay that stops there shows
/// an eviction that was decided and never happened.
#[test]
fn an_aborted_eviction_is_journaled_not_left_orphaned() {
    let dir = fresh_dir("bloomery-pager-abort");
    let substrate = ScriptedSubstrate {
        save_failure: Some("disk on fire".to_string()),
        ..Default::default()
    };
    // 300 MiB budget: one 4096-token qwen context fits, two do not.
    let (mut p, jpath) = scripted_pager(&dir, substrate, 300 * 1024 * 1024);
    let a = p.create_agent("qwen", 50, None, 10_000).unwrap();
    let b = p.create_agent("qwen", 100, None, 10_000).unwrap();
    p.infer(&a.id, "hello from a", 16).unwrap();

    match p.infer(&b.id, "hello from b", 16) {
        Err(PagerError::Substrate(msg)) => assert!(msg.contains("disk on fire")),
        other => panic!("expected Substrate, got {other:?}"),
    }

    let events = replay(&jpath).unwrap();
    assert!(events.iter().any(|e| matches!(e,
        Event::SchedulerDecision { decision, evicted, .. }
            if decision == "evict" && evicted == &vec![a.id.clone()])));
    assert!(
        events.iter().any(|e| matches!(e, Event::Degraded { reason }
            if reason == &format!("eviction of {} aborted: save_state failed: disk on fire", a.id))),
        "{events:?}"
    );
    assert!(
        !events.iter().any(|e| matches!(
            e,
            Event::PagerOp {
                op: PagerOpKind::EvictSave,
                ..
            }
        )),
        "no EvictSave may be claimed for a save that failed"
    );
    // the victim keeps its context rather than losing its conversation
    assert_eq!(p.status().agents[0].state, "resident");
}

/// Law 2's backstop lives in the substrate, which knows the real
/// tokenization; the refusal has to arrive on this side still classified as a
/// refusal, not as a broken backend.
#[test]
fn the_substrate_window_backstop_stays_a_refusal_across_the_boundary() {
    let dir = fresh_dir("bloomery-pager-backstop");
    let substrate = ScriptedSubstrate {
        infer_failure: Some(format!(
            "refusing: 0 cached + 900 prompt + 16 requested tokens {} of 512 tokens",
            bloomery_substrate::WINDOW_EXCEEDED
        )),
        ..Default::default()
    };
    let (mut p, jpath) = scripted_pager(&dir, substrate, 10u64.pow(9));
    let a = p.create_agent("qwen", 50, None, 10_000).unwrap();

    // The pager's own estimate lets this through: 12/3 + 16 = 20 <= 4096.
    match p.infer(&a.id, "hello from a", 16) {
        Err(PagerError::PromptTooLarge {
            needed_tokens,
            window_tokens,
        }) => {
            // provenance: the pager's own estimate and its own window
            assert_eq!(needed_tokens, 20);
            assert_eq!(window_tokens, 4096);
        }
        other => panic!("expected PromptTooLarge, got {other:?}"),
    }
    let events = replay(&jpath).unwrap();
    assert!(
        events
            .iter()
            .any(|e| matches!(e, Event::Refusal { id, detail, .. }
            if id == &a.id && detail.contains(bloomery_substrate::WINDOW_EXCEEDED))),
        "the substrate's exact arithmetic is preserved in the journal: {events:?}"
    );
}

/// Task 14 serves requests from several threads against one pager behind a
/// lock. This test exists to fail *at compile time* if the free-VRAM
/// closure's `Send + Sync` bounds are ever dropped — by then every caller
/// that built a pager would have to change.
#[test]
fn a_pager_can_be_shared_across_threads() {
    let dir = fresh_dir("bloomery-pager-send");
    let (p, _, _) = pager_in(&dir, 0, Some(10u64.pow(9)));
    let shared = std::sync::Arc::new(std::sync::Mutex::new(p));
    let handle = {
        let shared = std::sync::Arc::clone(&shared);
        std::thread::spawn(move || shared.lock().unwrap().status().agents.len())
    };
    assert_eq!(handle.join().unwrap(), 0);
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
