//! Task 4: the equal-priority LRU time-sharing tiebreak.
//!
//! `plan_residency` (frozen, `bloomery-core/src/scheduler.rs`) never evicts a
//! resident whose priority is >= the requester's, so a same-priority
//! stand-off is a *permanent* refusal under the planner alone. This is the
//! pager's own layer on top of that: once every resident blocking a request
//! is idle and exactly the request's own priority (never mixed, never
//! higher), the first such refusal starts a clock; a later attempt that has
//! waited a full quantum evicts the least-recently-used equal-priority
//! resident anyway, so no equal-priority agent can starve another forever.
//!
//! Same "qwen" geometry and `KV_BYTES` pinning as `pager_weights_test.rs`
//! (28 layers x 4 kv-heads x 128 head-dim, `window_cap = 1024` ->
//! `kv_per_token = 57_344` -> `kv_bytes = 1024 * 57_344 = 58_720_256 B`
//! exactly, 56 MiB). Every scenario drives a controllable `Arc<AtomicU64>`
//! clock through `Pager::set_clock`, never a real wall clock, so elapsed
//! time is exact and the test is not flaky.
//!
//! **Fixture arithmetic (weights 200 MiB, budget 270 MiB):** before any
//! model is loaded, admitting the first agent costs `kv (56) + weights
//! (200) = 256 MiB <= 270 MiB` -> fits. Once the model is loaded, a second
//! same-model agent's own demand is `kv` alone (56 MiB), but `avail = 270 -
//! 200 (loaded weights) - 56 (first agent's resident kv) = 14 MiB < 56 MiB`
//! -> refused, and (same priority) not evictable under the frozen planner.
//! This is exactly the brief's "one 56 MiB context fits, two don't" shape,
//! and it closes exactly under `kv_bytes = 58_720_256` with no adjustment
//! to the brief's suggested 270 MiB — see the task report for the worked
//! numbers on every scenario, including the 330 MiB budget scenario 3 uses
//! so *two* contexts fit before the third contends.

use bloomery_core::journal::{replay, Event, Journal, PagerOpKind};
use bloomery_daemon::agents::ImageStore;
use bloomery_daemon::pager::*;
use bloomery_substrate::{fake::FakeSubstrate, Reply};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

const MIB: u64 = 1024 * 1024;
/// `window_cap = 1024` tokens at `kv_per_token = 57_344` — see the module
/// doc comment.
const WINDOW_CAP: u32 = 1024;
const WEIGHTS: u64 = 200 * MIB;

fn ok(text: &str) -> Reply {
    Reply {
        text: text.into(),
        prompt_tokens: Some(8),
        completion_tokens: Some(4),
        duration_ms: 3,
    }
}

/// The shared "qwen" geometry (28 layers, 4 kv-heads, 128 head-dim — see the
/// module doc comment).
fn meta(weights_bytes: u64) -> bloomery_core::gguf::GgufMeta {
    bloomery_core::gguf::GgufMeta {
        arch: "qwen2".into(),
        layers: 28,
        kv_heads: 4,
        head_dim: 128,
        training_ctx: 4096,
        weights_bytes,
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
/// constant free-VRAM probe — the fixture's static reservation budget.
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

fn write_gguf(dir: &Path, name: &str, contents: &[u8]) -> PathBuf {
    let gguf = dir.join(name);
    std::fs::write(&gguf, contents).unwrap();
    gguf
}

/// A deterministic clock backed by a shared `AtomicU64`, per the brief.
fn test_clock(t: Arc<AtomicU64>) -> ClockFn {
    Box::new(move || t.load(Ordering::SeqCst))
}

/// a1 infers (resident, priority 100). clock = 0. a2 infers -> Refused
/// (avail 14 MiB < kv 56 MiB, same priority, not evictable). clock =
/// 29_999. a2 infers again -> STILL Refused (one ms short of the 30_000 ms
/// quantum). No `evict_timeshare` decision and no `EvictSave` for a1 in the
/// journal.
#[test]
fn within_the_quantum_equal_priority_stays_refused() {
    let dir = fresh_dir("bloomery-pager-timeshare-within-quantum");
    let budget = 270 * MIB;
    let clock = Arc::new(AtomicU64::new(0));
    let (mut p, jpath, _) = pager_in(&dir, 1, Some(budget));
    p.set_clock(test_clock(Arc::clone(&clock)));
    p.set_time_share_quantum_ms(30_000);
    let gguf = write_gguf(&dir, "qwen.gguf", b"weights");
    p.register_model("qwen", &gguf, meta(WEIGHTS), None)
        .unwrap();
    let a1 = p
        .create_agent("qwen", 100, Some(WINDOW_CAP), 10_000)
        .unwrap();
    let a2 = p
        .create_agent("qwen", 100, Some(WINDOW_CAP), 10_000)
        .unwrap();

    p.infer(&a1.id, "hello from a1", 16)
        .expect("weights(200)+kv(56)=256 <= 270: fits, a1 resident");

    match p.infer(&a2.id, "hello from a2", 16) {
        Err(PagerError::Refused { .. }) => {}
        other => panic!("expected Refused (avail 14 MiB < kv 56 MiB, same priority): {other:?}"),
    }

    clock.store(29_999, Ordering::SeqCst);
    match p.infer(&a2.id, "hello from a2 again", 16) {
        Err(PagerError::Refused { .. }) => {}
        other => panic!("expected still-Refused one ms short of the quantum: {other:?}"),
    }

    let events = replay(&jpath).unwrap();
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, Event::SchedulerDecision { decision, .. }
                if decision.starts_with("evict_timeshare"))),
        "no time-share eviction should fire before the quantum elapses: {events:?}"
    );
    assert!(
        !events.iter().any(|e| matches!(e,
            Event::PagerOp { id, op: PagerOpKind::EvictSave, .. } if id == &a1.id)),
        "a1 must not have been evicted: {events:?}"
    );
}

/// a1 infers (resident, last-use 0). clock = 0: a2 infers -> Refused
/// (waiting starts). clock = 30_000: a2 infers -> succeeds; the journal
/// carries a `SchedulerDecision` whose `decision` starts with
/// `"evict_timeshare("` naming a1 as the victim, followed by a `PagerOp`
/// `EvictSave` for a1 (the same eviction machinery `Placement::Evict` uses).
#[test]
fn after_the_quantum_the_lru_equal_priority_resident_is_evicted() {
    let dir = fresh_dir("bloomery-pager-timeshare-after-quantum");
    let budget = 270 * MIB;
    let clock = Arc::new(AtomicU64::new(0));
    let (mut p, jpath, _) = pager_in(&dir, 2, Some(budget));
    p.set_clock(test_clock(Arc::clone(&clock)));
    p.set_time_share_quantum_ms(30_000);
    let gguf = write_gguf(&dir, "qwen.gguf", b"weights");
    p.register_model("qwen", &gguf, meta(WEIGHTS), None)
        .unwrap();
    let a1 = p
        .create_agent("qwen", 100, Some(WINDOW_CAP), 10_000)
        .unwrap();
    let a2 = p
        .create_agent("qwen", 100, Some(WINDOW_CAP), 10_000)
        .unwrap();

    p.infer(&a1.id, "hello from a1", 16).unwrap(); // a1 resident, last-use = 0

    match p.infer(&a2.id, "hello from a2", 16) {
        Err(PagerError::Refused { .. }) => {}
        other => panic!("expected Refused: {other:?}"),
    }

    clock.store(30_000, Ordering::SeqCst);
    p.infer(&a2.id, "hello from a2 again", 16)
        .expect("quantum elapsed: time-share evicts a1, freeing room for a2");

    let events = replay(&jpath).unwrap();
    let decision_idx = events.iter().position(|e| matches!(e,
        Event::SchedulerDecision { id, decision, evicted }
            if id == &a2.id && decision.starts_with("evict_timeshare(") && evicted == &vec![a1.id.clone()]));
    assert!(
        decision_idx.is_some(),
        "expected an evict_timeshare SchedulerDecision naming a1: {events:?}"
    );
    let evict_idx = events.iter().position(|e| {
        matches!(e,
        Event::PagerOp { id, op: PagerOpKind::EvictSave, .. } if id == &a1.id)
    });
    assert!(
        evict_idx.is_some(),
        "expected an EvictSave PagerOp for a1: {events:?}"
    );
    assert!(
        decision_idx.unwrap() < evict_idx.unwrap(),
        "the decision must be journaled before the eviction it names"
    );
}

/// Budget fits TWO contexts (330 MiB). a1 infers at t=0 (last-use 0), a2
/// infers at t=10_000 (last-use 10_000) — both resident, no contention. a3
/// (same priority) is refused at t=20_000 (avail 18 MiB < kv 56 MiB,
/// neither a1 nor a2 evictable under the frozen planner). At t=50_001 (a
/// full quantum past a3's t=20_000 refusal) a3 infers -> the victim must be
/// a1 (the older last-use), never a2.
#[test]
fn lru_picks_the_least_recently_used_among_equals() {
    let dir = fresh_dir("bloomery-pager-timeshare-lru");
    let budget = 330 * MIB;
    let clock = Arc::new(AtomicU64::new(0));
    let (mut p, jpath, _) = pager_in(&dir, 3, Some(budget));
    p.set_clock(test_clock(Arc::clone(&clock)));
    p.set_time_share_quantum_ms(30_000);
    let gguf = write_gguf(&dir, "qwen.gguf", b"weights");
    p.register_model("qwen", &gguf, meta(WEIGHTS), None)
        .unwrap();
    let a1 = p
        .create_agent("qwen", 100, Some(WINDOW_CAP), 10_000)
        .unwrap();
    let a2 = p
        .create_agent("qwen", 100, Some(WINDOW_CAP), 10_000)
        .unwrap();
    let a3 = p
        .create_agent("qwen", 100, Some(WINDOW_CAP), 10_000)
        .unwrap();

    p.infer(&a1.id, "hello from a1", 16).unwrap(); // last-use a1 = 0

    clock.store(10_000, Ordering::SeqCst);
    p.infer(&a2.id, "hello from a2", 16)
        .expect("avail 74 MiB fits a2's 56 MiB ctx beside a1: both resident, no eviction");
    // last-use a2 = 10_000

    clock.store(20_000, Ordering::SeqCst);
    match p.infer(&a3.id, "hello from a3", 16) {
        Err(PagerError::Refused { .. }) => {}
        other => panic!("expected Refused (avail 18 MiB < kv 56 MiB): {other:?}"),
    }

    clock.store(50_001, Ordering::SeqCst);
    p.infer(&a3.id, "hello from a3 again", 16)
        .expect("quantum elapsed: time-share evicts the LRU equal-priority resident");

    let events = replay(&jpath).unwrap();
    assert!(
        events.iter().any(|e| matches!(e,
            Event::SchedulerDecision { id, decision, evicted }
                if id == &a3.id && decision.starts_with("evict_timeshare(")
                    && evicted == &vec![a1.id.clone()])),
        "victim must be a1 (last-use 0), never a2 (last-use 10_000): {events:?}"
    );
    assert!(
        !events.iter().any(|e| matches!(e,
            Event::PagerOp { id, op: PagerOpKind::EvictSave, .. } if id == &a2.id)),
        "a2 must never have been evicted: {events:?}"
    );
}

/// Resident a1 has priority 150 (higher). a2 at priority 100 is refused at
/// t=0. At t=60_000 (double the quantum) a2 infers -> STILL Refused: the
/// spec's tiebreak only fires when every resident is exactly the request's
/// own priority, and a1 outranks a2 here, so this is a permanent plain
/// refusal — never a time-share eviction, no matter how long a2 waits.
#[test]
fn mixed_priorities_never_time_share() {
    let dir = fresh_dir("bloomery-pager-timeshare-mixed-priority");
    let budget = 270 * MIB;
    let clock = Arc::new(AtomicU64::new(0));
    let (mut p, jpath, _) = pager_in(&dir, 1, Some(budget));
    p.set_clock(test_clock(Arc::clone(&clock)));
    p.set_time_share_quantum_ms(30_000);
    let gguf = write_gguf(&dir, "qwen.gguf", b"weights");
    p.register_model("qwen", &gguf, meta(WEIGHTS), None)
        .unwrap();
    let a1 = p
        .create_agent("qwen", 150, Some(WINDOW_CAP), 10_000)
        .unwrap();
    let a2 = p
        .create_agent("qwen", 100, Some(WINDOW_CAP), 10_000)
        .unwrap();

    p.infer(&a1.id, "hello from a1", 16).unwrap(); // a1 resident, priority 150

    match p.infer(&a2.id, "hello from a2", 16) {
        Err(PagerError::Refused { .. }) => {}
        other => panic!("expected Refused (a1 outranks a2, never evictable): {other:?}"),
    }

    clock.store(60_000, Ordering::SeqCst);
    match p.infer(&a2.id, "hello from a2 again", 16) {
        Err(PagerError::Refused { .. }) => {}
        other => {
            panic!("mixed priority must never time-share, even well past the quantum: {other:?}")
        }
    }

    let events = replay(&jpath).unwrap();
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, Event::SchedulerDecision { decision, .. }
                if decision.starts_with("evict_timeshare"))),
        "a higher-priority resident must never be time-share evicted: {events:?}"
    );
}

/// a2 is refused at t=0 (qualifying: a1 is idle and equal priority — the
/// wait starts). a1 is removed (`remove_agent`), freeing the slot. a2
/// infers at t=5_000 -> succeeds via *ordinary* placement (a1 is gone, so
/// it just fits — no `evict_timeshare` decision in the journal), which must
/// clear a2's waiting tracker.
///
/// A later refusal cycle for a2 must then start the wait completely fresh:
/// a2 is suspended, a3 (same priority) takes the only slot, and a2 is
/// refused again at t=20_000. If the earlier t=0 mark had survived the
/// t=5_000 clear, almost any later time would already look like a full
/// quantum has elapsed (0 + 30_000 = 30_000, long past by t=49_999); this
/// asserts a2 is *still* refused at t=49_999 and only succeeds — evicting
/// a3 — at t=50_000, the genuinely fresh quantum boundary from the t=20_000
/// mark.
#[test]
fn successful_placement_clears_the_waiting_tracker() {
    let dir = fresh_dir("bloomery-pager-timeshare-clears-tracker");
    let budget = 270 * MIB;
    let clock = Arc::new(AtomicU64::new(0));
    let (mut p, jpath, _) = pager_in(&dir, 4, Some(budget));
    p.set_clock(test_clock(Arc::clone(&clock)));
    p.set_time_share_quantum_ms(30_000);
    let gguf = write_gguf(&dir, "qwen.gguf", b"weights");
    p.register_model("qwen", &gguf, meta(WEIGHTS), None)
        .unwrap();
    let a1 = p
        .create_agent("qwen", 100, Some(WINDOW_CAP), 10_000)
        .unwrap();
    let a2 = p
        .create_agent("qwen", 100, Some(WINDOW_CAP), 10_000)
        .unwrap();

    p.infer(&a1.id, "hello from a1", 16).unwrap(); // a1 resident

    match p.infer(&a2.id, "hello from a2", 16) {
        Err(PagerError::Refused { .. }) => {} // qualifying refusal: waiting_since[a2] = 0
        other => panic!("expected Refused: {other:?}"),
    }

    p.remove_agent(&a1.id, "test teardown").unwrap(); // frees the slot

    clock.store(5_000, Ordering::SeqCst);
    p.infer(&a2.id, "hello from a2 again", 16)
        .expect("a1 gone: avail 70 MiB fits a2 directly, ordinary placement");

    let events = replay(&jpath).unwrap();
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, Event::SchedulerDecision { decision, .. }
                if decision.starts_with("evict_timeshare"))),
        "a2's t=5_000 placement succeeded via ordinary Fits, not time-share: {events:?}"
    );

    p.suspend(&a2.id).unwrap();
    let a3 = p
        .create_agent("qwen", 100, Some(WINDOW_CAP), 10_000)
        .unwrap();
    clock.store(10_000, Ordering::SeqCst);
    p.infer(&a3.id, "hello from a3", 16)
        .expect("a2 suspended, model already loaded: avail 70 MiB fits a3");
    // last-use a3 = 10_000

    clock.store(20_000, Ordering::SeqCst);
    match p.infer(&a2.id, "a2 wants back in", 16) {
        Err(PagerError::Refused { .. }) => {} // fresh waiting_since[a2] = 20_000
        other => panic!("expected Refused (a3 occupies the only slot, same priority): {other:?}"),
    }

    clock.store(49_999, Ordering::SeqCst);
    match p.infer(&a2.id, "a2 still waiting", 16) {
        Err(PagerError::Refused { .. }) => {}
        other => panic!(
            "must still require the fresh quantum boundary at t=50_000, not a leftover \
             mark from the already-cleared t=0 refusal: {other:?}"
        ),
    }

    clock.store(50_000, Ordering::SeqCst);
    p.infer(&a2.id, "a2 finally back", 16)
        .expect("the fresh wait (20_000 -> 50_000) has now elapsed a full quantum");

    let events = replay(&jpath).unwrap();
    assert!(
        events.iter().any(|e| matches!(e,
            Event::SchedulerDecision { id, decision, evicted }
                if id == &a2.id && decision.starts_with("evict_timeshare(")
                    && evicted == &vec![a3.id.clone()])),
        "the fresh wait must end in a3 (the only, LRU resident) being time-share evicted: \
         {events:?}"
    );
}
