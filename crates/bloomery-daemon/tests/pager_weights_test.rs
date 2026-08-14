//! Task 3: weights enter the reservation budget.
//!
//! Every scenario shares one geometry (`pager_test.rs`'s "qwen" shape:
//! 28 layers × 4 kv-heads × 128 head-dim → `kv_per_token = 57_344`) and one
//! `window_cap` of 1024 tokens, which pins every agent's KV footprint at
//! exactly `1024 * 57_344 = 58_720_256 B` (~56 MiB) regardless of which
//! model it's on. Budgets and `weights_bytes` are then chosen in exact
//! multiples of 1 MiB (`1024 * 1024`) so every arithmetic step below is
//! exact, not approximate — no rounding slop to explain away in an assert.
//!
//! GPU-free throughout, same as `pager_test.rs`: everything drives
//! `FakeSubstrate`.

use bloomery_core::journal::{replay, Event, Journal, PagerOpKind};
use bloomery_daemon::agents::ImageStore;
use bloomery_daemon::pager::*;
use bloomery_substrate::{fake::FakeSubstrate, Reply};
use std::path::{Path, PathBuf};

const MIB: u64 = 1024 * 1024;
/// `window_cap = 1024` tokens at `kv_per_token = 57_344` — see the module
/// doc comment.
const WINDOW_CAP: u32 = 1024;
const KV_BYTES: u64 = 1024 * 57_344;

fn ok(text: &str) -> Reply {
    Reply {
        text: text.into(),
        prompt_tokens: Some(8),
        completion_tokens: Some(4),
        duration_ms: 3,
    }
}

/// The shared "qwen" geometry (28 layers, 4 kv-heads, 128 head-dim — see the
/// module doc comment), parameterized only on `weights_bytes` so each
/// scenario can pick its own model size.
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

/// Loading a model charges its weights against the budget, so a second
/// agent on the *same, already-loaded* model can be squeezed out of room by
/// weights it didn't itself load.
///
/// budget 300 MiB; weights 200 MiB; a per-agent context is 56 MiB
/// (`KV_BYTES`).
/// - a1 (priority 50) infers: model loads (200 MiB) + its own ctx (56 MiB)
///   = 256 MiB ≤ 300 MiB → fits, no eviction.
/// - a2 (same model, priority 100 — strictly higher, so the frozen planner
///   may evict a1) infers: `avail = 300 − 200 − 56 = 44 MiB`, which is less
///   than a2's own 56 MiB ctx, but reclaiming a1's 56 MiB covers it → evict
///   a1, not refuse.
#[test]
fn loading_a_model_charges_its_weights_against_the_budget() {
    let dir = fresh_dir("bloomery-pager-weights-evict");
    let budget = 300 * MIB;
    let weights = 200 * MIB;
    let (mut p, jpath, _) = pager_in(&dir, 2, Some(budget));
    let gguf = write_gguf(&dir, "qwen.gguf", b"weights");
    p.register_model("qwen", &gguf, meta(weights), None)
        .unwrap();
    let a1 = p
        .create_agent("qwen", 50, Some(WINDOW_CAP), 10_000)
        .unwrap();
    let a2 = p
        .create_agent("qwen", 100, Some(WINDOW_CAP), 10_000)
        .unwrap();

    p.infer(&a1.id, "hello from a1", 16)
        .expect("weights (200) + ctx (56) = 256 <= 300: fits without eviction");
    p.infer(&a2.id, "hello from a2", 16)
        .expect("avail (44) < 56, but reclaiming a1's 56 MiB covers a2's ctx: evict, not refuse");

    let events = replay(&jpath).unwrap();
    assert!(
        events.iter().any(|e| matches!(e,
            Event::PagerOp { id, op: PagerOpKind::EvictSave, .. } if id == &a1.id)),
        "a1 must have been evicted to make room for a2: {events:?}"
    );
}

/// A second model's weights that cannot fit — even after every evictable KV
/// context is reclaimed — are refused with the arithmetic, and the
/// substrate is never asked to load them (law 1: pre-checked, never
/// inferred from an allocation failure).
///
/// model A weights 200 MiB (loaded via a1's infer); model B weights
/// 250 MiB. The budget is `weights_b + KV_BYTES` (306 MiB) — not the
/// brief's illustrative "300 MiB" — because `create_agent`'s own window law
/// (Phase 1, unaffected by Task 3) also sizes each agent's window against
/// `budget − that agent's own weights_bytes`; below 306 MiB that term binds
/// b's window under 1024 tokens before the reservation accounting under
/// test even runs, which would silently swap what this test is measuring.
/// At 306 MiB every agent's window still lands on exactly 1024 tokens
/// (`KV_BYTES`), so this stays the "≈300 MiB" scenario the brief describes:
/// a2 on model B infers with `avail = budget − 200 − 56 = 50 MiB` (56 MiB is
/// a1's resident ctx); reclaiming all of it gives
/// `avail + reclaimable = budget − 200 = 106 MiB`, still short of B's
/// demand (`250 + 56 = 306 MiB`) → refused, never evicts anything (eviction
/// only happens when it actually closes the gap).
#[test]
fn a_second_models_weights_that_cannot_fit_are_refused_with_the_arithmetic() {
    let dir = fresh_dir("bloomery-pager-weights-refuse");
    let weights_a = 200 * MIB;
    let weights_b = 250 * MIB;
    let budget = weights_b + KV_BYTES;
    let (mut p, jpath, _) = pager_in(&dir, 1, Some(budget));
    let gguf_a = write_gguf(&dir, "modelA.gguf", b"weights-a");
    let gguf_b = write_gguf(&dir, "modelB.gguf", b"weights-b");
    p.register_model("modelA", &gguf_a, meta(weights_a), None)
        .unwrap();
    p.register_model("modelB", &gguf_b, meta(weights_b), None)
        .unwrap();
    let a1 = p
        .create_agent("modelA", 50, Some(WINDOW_CAP), 10_000)
        .unwrap();
    let a2 = p
        .create_agent("modelB", 100, Some(WINDOW_CAP), 10_000)
        .unwrap();

    p.infer(&a1.id, "hello from a1", 16)
        .expect("weights_a (200) + ctx (56) = 256 <= 300: fits");
    let load_calls_before = p
        .substrate()
        .calls()
        .iter()
        .filter(|c| c.as_str() == "load_model")
        .count();
    assert_eq!(load_calls_before, 1, "only modelA has loaded so far");

    match p.infer(&a2.id, "hello from a2", 16) {
        Err(PagerError::Refused {
            needed,
            free,
            reclaimable,
        }) => {
            assert_eq!(
                needed,
                weights_b + KV_BYTES,
                "needed includes the weights term"
            );
            assert_eq!(free, budget - weights_a - KV_BYTES);
            assert_eq!(
                reclaimable, KV_BYTES,
                "a1's ctx is the only reclaimable KV; weights are never auto-evicted"
            );
        }
        other => panic!("expected Refused, got {other:?}"),
    }

    // Law 1: the substrate is never asked to load a model on a refused
    // path — memory pressure is pre-checked, not inferred from a failed
    // allocation.
    let load_calls_after = p
        .substrate()
        .calls()
        .iter()
        .filter(|c| c.as_str() == "load_model")
        .count();
    assert_eq!(
        load_calls_after, load_calls_before,
        "modelB must never reach the substrate on a refused path"
    );

    // M-2: pin the *rendered* detail string byte-for-byte, not just "it
    // mentions weights somewhere" — this is what would have caught I-1
    // (the unmeasured-budget branch fabricating a "budget 0 B" term).
    // Task 5's live-run fix widened this rendering: the demand side now names
    // the per-context reservation and its split, and the supply side names the
    // daemon-level overhead margin. Both new terms are `0 B` in this fixture
    // (it wires neither), and every other number is byte-identical to what
    // Task 3 pinned — the arithmetic did not move, the sentence explaining it
    // did.
    let expected_detail = format!(
        "residency: weights {weights_b} B + reserved {KV_BYTES} B (kv {KV_BYTES} B + ctx \
         overhead 0 B) vs budget {budget} B − overhead 0 B − loaded {weights_a} B − resident \
         {KV_BYTES} B (needed {expected_needed} B, free {expected_free} B, reclaimable \
         {expected_reclaimable} B)",
        expected_needed = weights_b + KV_BYTES,
        expected_free = budget - weights_a - KV_BYTES,
        expected_reclaimable = KV_BYTES,
    );
    let events = replay(&jpath).unwrap();
    assert!(
        events.iter().any(|e| matches!(e,
            Event::Refusal { id, detail, .. }
                if id == &a2.id && detail == &expected_detail)),
        "expected detail {expected_detail:?} not found in {events:?}"
    );
}

/// `unload_model` credits a model's weights back to the budget — the exact
/// refusal from the previous scenario is turned into a fit once modelA's
/// weights are dropped.
///
/// Same setup (budget 300 MiB, weights_a 200 MiB, weights_b 250 MiB) but
/// with the budget bumped to 306 MiB (`weights_b + KV_BYTES` exactly) so
/// the post-unload placement lands exactly at the fit/refuse boundary
/// (`avail == demand`, not just `avail > demand`) — the sharpest possible
/// demonstration that the credit-back is exact, not merely "enough".
/// modelA is still refused with weights_a loaded, mirroring the previous
/// scenario, and only *then* unloaded and retried.
#[test]
fn unload_credits_the_weights_back() {
    let dir = fresh_dir("bloomery-pager-weights-unload");
    let weights_a = 200 * MIB;
    let weights_b = 250 * MIB;
    let budget = weights_b + KV_BYTES; // 306 MiB exactly
    let (mut p, jpath, _) = pager_in(&dir, 2, Some(budget));
    let gguf_a = write_gguf(&dir, "modelA.gguf", b"weights-a");
    let gguf_b = write_gguf(&dir, "modelB.gguf", b"weights-b");
    p.register_model("modelA", &gguf_a, meta(weights_a), None)
        .unwrap();
    p.register_model("modelB", &gguf_b, meta(weights_b), None)
        .unwrap();
    let a1 = p
        .create_agent("modelA", 50, Some(WINDOW_CAP), 10_000)
        .unwrap();
    let a2 = p
        .create_agent("modelB", 100, Some(WINDOW_CAP), 10_000)
        .unwrap();

    p.infer(&a1.id, "hello from a1", 16)
        .expect("weights_a (200) + ctx (56) = 256 <= 306: fits");

    match p.infer(&a2.id, "hello from a2", 16) {
        Err(PagerError::Refused { .. }) => {}
        other => panic!("expected a2 refused while modelA is still loaded, got {other:?}"),
    }

    p.unload_model("modelA")
        .expect("unload_model is a manual, always-available operation");

    p.infer(&a2.id, "hello from a2, retry", 16).expect(
        "with modelA's weights credited back, avail == budget == weights_b + ctx: exact fit",
    );

    let events = replay(&jpath).unwrap();
    let unloaded_at = events
        .iter()
        .position(|e| matches!(e, Event::ModelUnloaded { model } if model == "modelA"))
        .expect("ModelUnloaded must be journaled");
    let loaded_b_at = events
        .iter()
        .position(|e| matches!(e, Event::ModelLoaded { model, .. } if model == "modelB"))
        .expect("modelB's on-demand load must be journaled");
    assert!(
        unloaded_at < loaded_b_at,
        "modelA's credit-back must be journaled before modelB's successful load: {events:?}"
    );
}

/// `StatusReport::loaded_weights_bytes` is derived from the loaded set, not
/// a counter: zero before anything loads, and exactly one model's weights
/// once that model has actually loaded.
#[test]
fn status_reports_loaded_weights() {
    let dir = fresh_dir("bloomery-pager-weights-status");
    let budget = 300 * MIB;
    let weights = 200 * MIB;
    let (mut p, _, _) = pager_in(&dir, 1, Some(budget));
    let gguf = write_gguf(&dir, "qwen.gguf", b"weights");
    p.register_model("qwen", &gguf, meta(weights), None)
        .unwrap();
    assert_eq!(p.status().loaded_weights_bytes, 0, "nothing loaded yet");

    let a1 = p
        .create_agent("qwen", 50, Some(WINDOW_CAP), 10_000)
        .unwrap();
    assert_eq!(
        p.status().loaded_weights_bytes,
        0,
        "create_agent alone never loads weights"
    );

    p.infer(&a1.id, "hello", 16).unwrap();
    assert_eq!(p.status().loaded_weights_bytes, weights);
}

/// I-1's covering test: when the budget is unmeasured, the refusal detail
/// must say so honestly rather than fabricating a `budget 0 B` term (law 5:
/// `None` is unmeasured, never zero — and that applies to prose, not just
/// typed fields). The residency-count-cap-of-one plans as zero free bytes
/// internally, but the *string* a human or a log-reader sees must not claim
/// a budget of `0` was ever measured.
#[test]
fn unmeasured_budget_refusal_detail_says_unmeasured_not_zero() {
    let dir = fresh_dir("bloomery-pager-weights-unmeasured");
    let (mut p, jpath, _) = pager_in(&dir, 1, None);
    let gguf = write_gguf(&dir, "qwen.gguf", b"weights");
    p.register_model("qwen", &gguf, meta(200 * MIB), None)
        .unwrap();
    // Same priority: neither is evictable for the other, so the second
    // agent is refused outright under the residency-count-cap-of-one.
    let a1 = p
        .create_agent("qwen", 50, Some(WINDOW_CAP), 10_000)
        .unwrap();
    let a2 = p
        .create_agent("qwen", 50, Some(WINDOW_CAP), 10_000)
        .unwrap();

    p.infer(&a1.id, "hello from a1", 16)
        .expect("no residents yet: the unmeasured-budget path fits the first agent for free");

    match p.infer(&a2.id, "hello from a2", 16) {
        Err(PagerError::Refused { .. }) => {}
        other => panic!(
            "expected a2 refused under the unmeasured-VRAM residency cap of one, got {other:?}"
        ),
    }

    let events = replay(&jpath).unwrap();
    assert!(
        events.iter().any(|e| matches!(e,
            Event::Refusal { id, detail, .. }
                if id == &a2.id
                    && detail.contains("budget unmeasured")
                    && !detail.contains("budget 0"))),
        "the unmeasured-budget refusal must not fabricate a zero budget: {events:?}"
    );
}

/// M-4: the saturating floor. `free_vram` is documented to be a static
/// budget (see `Pager::new`), but nothing in the type stops a misconfigured
/// or drifting probe from reporting a value smaller than the weights
/// already loaded. `avail`'s two `saturating_sub` calls must clamp to `0`
/// rather than underflow (`u64` wraparound) or panic (debug-mode overflow
/// check) when that happens.
#[test]
fn a_budget_smaller_than_already_loaded_weights_saturates_to_zero_free_and_never_panics() {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;

    let dir = fresh_dir("bloomery-pager-weights-saturate");
    let jpath = dir.join("j.jsonl");
    let journal = Journal::open(&jpath).unwrap();
    let images = ImageStore::new(&dir.join("img")).unwrap();
    let mut fake = FakeSubstrate::new();
    fake.script_reply(ok("r"));

    let weights = 200 * MIB;
    // Generous enough that both agents' windows land on exactly
    // `WINDOW_CAP` tokens and a1's own placement fits exactly at the
    // boundary — see `a_second_models_weights_that_cannot_fit...` for why
    // the window law needs this much headroom.
    let generous = weights + KV_BYTES;
    let budget = Arc::new(AtomicU64::new(generous));
    let budget_read = budget.clone();
    let mut p = Pager::new(
        fake,
        journal,
        images,
        Box::new(move || Some(budget_read.load(Ordering::SeqCst))),
    );
    let gguf = write_gguf(&dir, "qwen.gguf", b"weights");
    p.register_model("qwen", &gguf, meta(weights), None)
        .unwrap();
    // Both agents' windows are computed now, while the budget is still
    // generous — a window is fixed at creation and never re-quoted. Same
    // priority for both: a2 must not be able to evict a1 and mask the
    // saturating floor by reclaiming its way to a fit instead.
    let a1 = p
        .create_agent("qwen", 50, Some(WINDOW_CAP), 10_000)
        .unwrap();
    let a2 = p
        .create_agent("qwen", 50, Some(WINDOW_CAP), 10_000)
        .unwrap();

    p.infer(&a1.id, "hello from a1", 16)
        .expect("weights + ctx fit the generous starting budget exactly");

    // Misconfigure: the probe now reports far less than what's already
    // loaded into VRAM. `loaded_weights (200 MiB) > budget (100 MiB)`.
    budget.store(weights / 2, Ordering::SeqCst);

    match p.infer(&a2.id, "hello from a2", 16) {
        Err(PagerError::Refused { free, .. }) => {
            assert_eq!(free, 0, "avail must saturate to 0, never underflow");
        }
        other => panic!("expected Refused with free: 0, got {other:?}"),
    }
}
