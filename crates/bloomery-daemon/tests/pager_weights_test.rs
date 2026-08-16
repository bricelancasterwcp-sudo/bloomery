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

    p.infer(&a1.id, "hello from a1", 16, None)
        .expect("weights (200) + ctx (56) = 256 <= 300: fits without eviction");
    p.infer(&a2.id, "hello from a2", 16, None)
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

    p.infer(&a1.id, "hello from a1", 16, None)
        .expect("weights_a (200) + ctx (56) = 256 <= 300: fits");
    let load_calls_before = p
        .substrate()
        .calls()
        .iter()
        .filter(|c| c.as_str() == "load_model")
        .count();
    assert_eq!(load_calls_before, 1, "only modelA has loaded so far");

    match p.infer(&a2.id, "hello from a2", 16, None) {
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

    p.infer(&a1.id, "hello from a1", 16, None)
        .expect("weights_a (200) + ctx (56) = 256 <= 306: fits");

    match p.infer(&a2.id, "hello from a2", 16, None) {
        Err(PagerError::Refused { .. }) => {}
        other => panic!("expected a2 refused while modelA is still loaded, got {other:?}"),
    }

    p.unload_model("modelA")
        .expect("unload_model is a manual, always-available operation");

    p.infer(&a2.id, "hello from a2, retry", 16, None).expect(
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

    p.infer(&a1.id, "hello", 16, None).unwrap();
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

    p.infer(&a1.id, "hello from a1", 16, None)
        .expect("no residents yet: the unmeasured-budget path fits the first agent for free");

    match p.infer(&a2.id, "hello from a2", 16, None) {
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

    p.infer(&a1.id, "hello from a1", 16, None)
        .expect("weights + ctx fit the generous starting budget exactly");

    // Misconfigure: the probe now reports far less than what's already
    // loaded into VRAM. `loaded_weights (200 MiB) > budget (100 MiB)`.
    budget.store(weights / 2, Ordering::SeqCst);

    match p.infer(&a2.id, "hello from a2", 16, None) {
        Err(PagerError::Refused { free, .. }) => {
            assert_eq!(free, 0, "avail must saturate to 0, never underflow");
        }
        other => panic!("expected Refused with free: 0, got {other:?}"),
    }
}

// ---------------------------------------------------------------------
// Task 3: per-model n_gpu_layers + declared weights-VRAM charge.
// ---------------------------------------------------------------------

/// `create_agent`'s window law uses the declared, override value — not the
/// file's raw `weights_bytes` — as `GeometryInput.weights_bytes` (Task 3,
/// spec §3). Asymmetric numbers (declared 100 MiB, file 300 MiB, free VRAM
/// 500 MiB) are chosen so the window's bound (`training_ctx` vs `vram`) and
/// exact token count diverge sharply between the two: with declared (100
/// MiB) charged, `(500-100)/kv_per_token = 7314` tokens exceeds
/// `training_ctx` (4096), so the window is `training_ctx`-bound at exactly
/// 4096; with the file's raw weights (300 MiB) charged instead, `(500-300)
/// /kv_per_token = 3657` tokens is UNDER `training_ctx`, so the window would
/// instead be `vram`-bound at exactly 3657. No placement call happens in
/// this test at all, so it is sensitive to exactly one charge site:
/// `create_agent`'s `GeometryInput.weights_bytes`.
#[test]
fn create_agent_window_uses_the_declared_weights_not_the_file() {
    let dir = fresh_dir("bloomery-pager-weights-tuning-window");
    let free_vram = 500 * MIB;
    let file = 300 * MIB;
    let declared = 100 * MIB;
    let (mut p, _, _) = pager_in(&dir, 0, Some(free_vram));
    let gguf = write_gguf(&dir, "qwen.gguf", b"weights");
    p.register_model("qwen", &gguf, meta(file), None).unwrap();
    p.set_model_tuning("qwen", None, Some(declared)).unwrap();

    // No window_cap: the window is purely training_ctx-vs-vram bound, so
    // this is a clean read of which weight value the geometry math used.
    let a1 = p.create_agent("qwen", 50, None, 10_000).unwrap();

    assert_eq!(
        a1.window_tokens, 4096,
        "training_ctx (4096) must win over vram (7314 tokens at declared 100 MiB) \
         — if the geometry read the file's 300 MiB instead, vram (3657) would win"
    );
    assert_eq!(a1.bound_by, "training_ctx");
}

/// `place`'s demand term (cold-model load) AND the reservation budget's
/// supply term (`loaded_weights_bytes`) both use the declared, override
/// value — not the file's raw `weights_bytes` (Task 3, spec §3). One model,
/// declared 100 MiB vs file 300 MiB, free VRAM 212 MiB exactly, chosen so:
///
/// - a1 (cap 1024 tokens, 56 MiB ctx) loads the model cold: demand is
///   `weights_term + 56 MiB`. At declared (100), demand is 156 MiB <= 212
///   MiB budget: fits outright (nothing resident yet to evict). At the file
///   value (300), demand would be 356 MiB > 212 MiB: a hard refusal (no
///   resident to evict). This exercises `place`'s demand term.
/// - a2 (cap 2048 tokens, 112 MiB ctx, same already-loaded model so its own
///   demand carries no weights term) then needs `avail + reclaimable >=
///   112 MiB`. At declared (100) loaded, `avail = 212 - 100 - 56 = 56 MiB`;
///   `+ reclaimable (a1's 56 MiB kv) = 112 MiB` — an EXACT fit via evicting
///   a1. At the file value (300) loaded, `avail` saturates to 0 MiB;
///   `+ reclaimable (56) = 56 MiB < 112 MiB`: refused even after evicting
///   everything reclaimable. This exercises `loaded_weights_bytes`, the
///   supply-side site.
///
/// Each of the two `infer` calls below therefore only succeeds if its own
/// charge site reads the declared value — a one-sided wiring bug on either
/// site fails exactly one of the two assertions.
#[test]
fn placement_uses_the_declared_weights_not_the_file() {
    let dir = fresh_dir("bloomery-pager-weights-tuning-placement");
    let free_vram = 212 * MIB;
    let file = 300 * MIB;
    let declared = 100 * MIB;
    let (mut p, _, _) = pager_in(&dir, 2, Some(free_vram));
    let gguf = write_gguf(&dir, "qwen.gguf", b"weights");
    p.register_model("qwen", &gguf, meta(file), None).unwrap();
    p.set_model_tuning("qwen", None, Some(declared)).unwrap();

    let a1 = p.create_agent("qwen", 50, Some(1024), 10_000).unwrap();
    let a2 = p.create_agent("qwen", 100, Some(2048), 10_000).unwrap();

    p.infer(&a1.id, "hello from a1", 16, None).expect(
        "demand = declared (100 MiB) + ctx (56 MiB) = 156 MiB <= 212 MiB budget: fits \
         without eviction — place's demand term must read the declared value",
    );
    p.infer(&a2.id, "hello from a2", 16, None).expect(
        "avail (56 MiB, from budget - declared 100 - a1's 56 MiB) + reclaimable (a1's \
         56 MiB) == demand (112 MiB) exactly: evicts a1, fits — loaded_weights_bytes \
         must read the declared value on the supply side",
    );
}

/// Clamp: a declared value LARGER than the file's own `weights_bytes` is
/// clamped down to the file value — a misconfigured, over-large declaration
/// must never inflate the charge past physical reality (spec §3's
/// `min(declared, weights_bytes)`). Free VRAM is set to exactly `file +
/// ctx`, so this only fits if the charge is clamped to `file`; the
/// (uncapped) declared value would blow the budget by nearly 800 MiB.
#[test]
fn declared_larger_than_file_clamps_to_file_no_inflation() {
    let dir = fresh_dir("bloomery-pager-weights-tuning-clamp");
    let file = 200 * MIB;
    let declared = 999 * MIB;
    let free_vram = file + KV_BYTES; // fits iff the charge clamps to `file`
    let (mut p, _, _) = pager_in(&dir, 1, Some(free_vram));
    let gguf = write_gguf(&dir, "qwen.gguf", b"weights");
    p.register_model("qwen", &gguf, meta(file), None).unwrap();
    p.set_model_tuning("qwen", None, Some(declared)).unwrap();

    let a1 = p
        .create_agent("qwen", 50, Some(WINDOW_CAP), 10_000)
        .unwrap();
    p.infer(&a1.id, "hello", 16, None).expect(
        "budget == file + ctx exactly: fits only if the 999 MiB declared value was \
         clamped down to the 200 MiB file value, not charged raw",
    );

    assert_eq!(
        p.status().loaded_weights_bytes,
        file,
        "the clamp applies to the loaded/status sum too — never the raw declared value"
    );
}

/// No override at all: `create_agent`, `place`, and `/status` all charge
/// the model's full file weight — byte-identical to today's behavior. An
/// EXPLICIT `set_model_tuning(model, None, None)` call (as opposed to never
/// calling it) must be an equivalent no-op, since spec §2 says omitting
/// both tuning fields in config is byte-for-byte identical to a bare-path
/// entry.
#[test]
fn explicit_no_override_tuning_call_is_a_full_charge_no_op() {
    let dir = fresh_dir("bloomery-pager-weights-tuning-noop");
    let budget = 300 * MIB;
    let weights = 200 * MIB;
    let (mut p, _, _) = pager_in(&dir, 1, Some(budget));
    let gguf = write_gguf(&dir, "qwen.gguf", b"weights");
    p.register_model("qwen", &gguf, meta(weights), None)
        .unwrap();
    p.set_model_tuning("qwen", None, None)
        .expect("explicit no-op tuning call on a known model must succeed");

    let a1 = p
        .create_agent("qwen", 50, Some(WINDOW_CAP), 10_000)
        .unwrap();
    p.infer(&a1.id, "hello", 16, None)
        .expect("weights (200) + ctx (56) = 256 <= 300: full charge, no override applied");

    assert_eq!(
        p.status().loaded_weights_bytes,
        weights,
        "declared absent -> full file weight, exactly today's behavior"
    );
}

/// `/status`'s `loaded_weights_bytes` sum reflects the declared value when
/// an override is active — it flows through `loaded_weights_bytes`
/// automatically (the same method the supply side of `place` reads), so
/// this pins that the fourth charge site really does inherit the fix rather
/// than needing its own wiring.
#[test]
fn status_reports_declared_weights_when_override_active() {
    let dir = fresh_dir("bloomery-pager-weights-tuning-status");
    let budget = 300 * MIB;
    let file = 300 * MIB;
    let declared = 120 * MIB;
    let (mut p, _, _) = pager_in(&dir, 1, Some(budget));
    let gguf = write_gguf(&dir, "qwen.gguf", b"weights");
    p.register_model("qwen", &gguf, meta(file), None).unwrap();
    p.set_model_tuning("qwen", None, Some(declared)).unwrap();

    let a1 = p
        .create_agent("qwen", 50, Some(WINDOW_CAP), 10_000)
        .unwrap();
    assert_eq!(p.status().loaded_weights_bytes, 0, "nothing loaded yet");
    p.infer(&a1.id, "hello", 16, None).unwrap();

    assert_eq!(
        p.status().loaded_weights_bytes,
        declared,
        "declared (120 MiB), not the file's 300 MiB, must reach /status"
    );
}

/// The refusal string names the weights term "declared" exactly when an
/// override is active for the model being refused, and never fabricates
/// that label when there is none — spec §3: "a declared number must never
/// read as a measured one."
#[test]
fn refusal_names_declared_when_override_is_active() {
    let dir = fresh_dir("bloomery-pager-weights-tuning-refusal-declared");
    let file_a = 100 * MIB;
    let file_b = 500 * MIB;
    let declared_b = 90 * MIB;
    let budget = 160 * MIB;
    let (mut p, jpath, _) = pager_in(&dir, 1, Some(budget));
    let gguf_a = write_gguf(&dir, "modelA.gguf", b"weights-a");
    let gguf_b = write_gguf(&dir, "modelB.gguf", b"weights-b");
    p.register_model("modelA", &gguf_a, meta(file_a), None)
        .unwrap();
    p.register_model("modelB", &gguf_b, meta(file_b), None)
        .unwrap();
    p.set_model_tuning("modelB", None, Some(declared_b))
        .unwrap();

    let a1 = p
        .create_agent("modelA", 50, Some(WINDOW_CAP), 10_000)
        .unwrap();
    let a2 = p
        .create_agent("modelB", 100, Some(WINDOW_CAP), 10_000)
        .unwrap();

    p.infer(&a1.id, "hello from a1", 16, None)
        .expect("modelA: weights (100) + ctx (56) = 156 <= 160: fits");

    match p.infer(&a2.id, "hello from a2", 16, None) {
        Err(PagerError::Refused { .. }) => {}
        other => panic!(
            "expected modelB refused (declared 90 + ctx 56 = 146, avail only 4 \
                          + reclaimable 56 = 60 < 146), got {other:?}"
        ),
    }

    let events = replay(&jpath).unwrap();
    let expected_clause =
        format!("weights {declared_b} B (declared weights_vram_mib; file {file_b} B)");
    assert!(
        events.iter().any(|e| matches!(e,
            Event::Refusal { id, detail, .. }
                if id == &a2.id && detail.contains(&expected_clause))),
        "expected refusal detail to contain {expected_clause:?}: {events:?}"
    );
}

/// The mirror of the previous test: no override on the refused model means
/// the refusal string must NOT contain "declared" anywhere — the weights
/// term is plainly the file's own measured value.
#[test]
fn refusal_omits_declared_when_no_override_is_active() {
    let dir = fresh_dir("bloomery-pager-weights-tuning-refusal-no-declared");
    let file_a = 100 * MIB;
    let file_b = 150 * MIB;
    let budget = 160 * MIB;
    let (mut p, jpath, _) = pager_in(&dir, 1, Some(budget));
    let gguf_a = write_gguf(&dir, "modelA.gguf", b"weights-a");
    let gguf_b = write_gguf(&dir, "modelB.gguf", b"weights-b");
    p.register_model("modelA", &gguf_a, meta(file_a), None)
        .unwrap();
    p.register_model("modelB", &gguf_b, meta(file_b), None)
        .unwrap();
    // No set_model_tuning call for modelB at all.

    let a1 = p
        .create_agent("modelA", 50, Some(WINDOW_CAP), 10_000)
        .unwrap();
    let a2 = p
        .create_agent("modelB", 100, Some(WINDOW_CAP), 10_000)
        .unwrap();

    p.infer(&a1.id, "hello from a1", 16, None)
        .expect("modelA: weights (100) + ctx (56) = 156 <= 160: fits");

    match p.infer(&a2.id, "hello from a2", 16, None) {
        Err(PagerError::Refused { .. }) => {}
        other => panic!("expected modelB refused, got {other:?}"),
    }

    let events = replay(&jpath).unwrap();
    assert!(
        events.iter().any(|e| matches!(e,
            Event::Refusal { id, detail, .. }
                if id == &a2.id
                    && detail.contains(&format!("weights {file_b} B"))
                    && !detail.contains("declared"))),
        "no override on modelB: the refusal must not say 'declared' anywhere: {events:?}"
    );
}

/// `set_model_tuning` on an unregistered model name is refused with
/// `UnknownModel`, naming the model — same contract as `attach_profile` and
/// `register_model`'s own re-registration guard.
#[test]
fn set_model_tuning_on_unknown_model_is_refused() {
    let dir = fresh_dir("bloomery-pager-weights-tuning-unknown");
    let (mut p, _, _) = pager_in(&dir, 0, Some(300 * MIB));
    match p.set_model_tuning("nope", Some(1), Some(1)) {
        Err(PagerError::UnknownModel(name)) => assert_eq!(name, "nope"),
        other => panic!("expected UnknownModel, got {other:?}"),
    }
}

/// A per-model `n_gpu_layers` override wins over the pager-global default
/// at the `load_model` call; a model with no override gets the pager-global
/// default (`u32::MAX`, unless `Pager::set_n_gpu_layers` was called — it
/// wasn't, in this test, so this also pins that default's own value).
#[test]
fn per_model_n_gpu_layers_override_wins_absent_override_uses_global_default() {
    let dir = fresh_dir("bloomery-pager-weights-tuning-ngpulayers");
    let budget = 1000 * MIB; // generous: placement isn't the point here
    let (mut p, _, _) = pager_in(&dir, 2, Some(budget));
    let gguf_a = write_gguf(&dir, "modelA.gguf", b"weights-a");
    let gguf_b = write_gguf(&dir, "modelB.gguf", b"weights-b");
    p.register_model("modelA", &gguf_a, meta(50 * MIB), None)
        .unwrap();
    p.register_model("modelB", &gguf_b, meta(50 * MIB), None)
        .unwrap();
    p.set_model_tuning("modelA", Some(28), None).unwrap();
    // modelB: no tuning call at all -> pager-global default.

    let a1 = p
        .create_agent("modelA", 50, Some(WINDOW_CAP), 10_000)
        .unwrap();
    let b1 = p
        .create_agent("modelB", 50, Some(WINDOW_CAP), 10_000)
        .unwrap();
    p.infer(&a1.id, "hi", 8, None).unwrap();
    p.infer(&b1.id, "hi", 8, None).unwrap();

    assert_eq!(
        p.substrate().load_n_gpu_layers(),
        &[28, u32::MAX],
        "modelA's override (28) must win; modelB with no override must get the \
         pager-global default (u32::MAX)"
    );
}

// ---------------------------------------------------------------------------
// Part B (spec §10 addendum): declared kv_per_token override.
// ---------------------------------------------------------------------------

/// The `meta()` fixture's GGUF-derived `kv_per_token` (28 layers, 4
/// kv-heads, 128 head-dim — the module doc's "qwen" geometry): `2 * 28 * 4 *
/// 128 * 2 = 57_344` B/token. Named here so every test below states its
/// asymmetric numbers against this one constant rather than a magic 57_344
/// repeated silently.
const GGUF_KV_PER_TOKEN: u64 = 57_344;

/// A declared override roughly 4x smaller than [`GGUF_KV_PER_TOKEN`] — the
/// spec's own measured motivation (qwen3.8-27b real KV ≈0.064 MiB/token vs
/// the GGUF-derived-charged 0.254 MiB/token, a ~4x overcount on hybrid-
/// DeltaNet architectures).
const DECLARED_KV_PER_TOKEN: u64 = 14_336;

/// `create_agent`'s window law uses the declared `kv_per_token_bytes`
/// override — not the GGUF-derived `kv_per_token` — as
/// `GeometryInput.kv_per_token` (spec §10 addendum). Asymmetric numbers
/// (declared 14 336 B/token, GGUF-derived 57 344 B/token, free VRAM chosen so
/// the window's bound (`training_ctx` vs `vram`) and exact token count
/// diverge sharply between the two — mirrors
/// `create_agent_window_uses_the_declared_weights_not_the_file`'s template):
/// with declared (14 336) charged, `(100_048_576 - 1_048_576) / 14_336 =
/// 6975` tokens exceeds `training_ctx` (4096), so the window is
/// `training_ctx`-bound at exactly 4096; with the GGUF-derived value (57 344)
/// charged instead, `100_000_000 / 57_344 = 1743` tokens is UNDER
/// `training_ctx`, so the window would instead be `vram`-bound at 1743. No
/// placement call happens in this test at all, so it is sensitive to exactly
/// one charge site: `create_agent`'s `GeometryInput.kv_per_token`.
#[test]
fn create_agent_window_uses_the_declared_kv_per_token_not_the_gguf_value() {
    let dir = fresh_dir("bloomery-pager-kv-tuning-window");
    let weights = MIB;
    let remaining = 100_000_000u64;
    let free_vram = remaining + weights;
    let (mut p, _, _) = pager_in(&dir, 0, Some(free_vram));
    let gguf = write_gguf(&dir, "qwen.gguf", b"weights");
    p.register_model("qwen", &gguf, meta(weights), None)
        .unwrap();
    p.set_kv_per_token_bytes("qwen", Some(DECLARED_KV_PER_TOKEN))
        .unwrap();

    // No window_cap: the window is purely training_ctx-vs-vram bound, so
    // this is a clean read of which per-token value the geometry math used.
    let a1 = p.create_agent("qwen", 50, None, 10_000).unwrap();

    assert_eq!(
        a1.window_tokens, 4096,
        "training_ctx (4096) must win over vram (6975 tokens at declared 14336 B/token) \
         — if the geometry read the GGUF-derived 57344 B/token instead, vram (1743) would win"
    );
    assert_eq!(a1.bound_by, "training_ctx");
}

/// The `kv_bytes` reservation charge — the demand side of `place` AND the
/// supply side (`resident_reserved_bytes`, via the stored `Agent.kv_bytes`)
/// — ALSO uses the declared `kv_per_token_bytes` override, via a SECOND,
/// independent read from the one just pinned above (spec §10 addendum: "one
/// source, both places" — mirrors `placement_uses_the_declared_weights_not_
/// the_file`'s "each independent charge site" property).
///
/// A fixed `window_cap` of 2048 tokens, and a free-VRAM/budget figure large
/// enough that the vram term — computed correctly, from the DECLARED
/// per-token figure, at the window-law site this test does not touch —
/// still exceeds 2048 tokens (`(40_894_464) / 14_336 = 2853 > 2048`), so
/// `window.tokens` is deterministically `2048` (`user_cap`-bound) regardless
/// of any bug at the reservation site under test here. The SAME free-VRAM
/// figure is also the placement budget (40 MiB): it sits strictly between
/// `kv_bytes` computed from the declared figure (`2048 * 14_336 = 28.0 MiB`,
/// demand ≈ 29.0 MiB with weights — fits) and `kv_bytes` computed from the
/// GGUF-derived figure (`2048 * 57_344 = 112.0 MiB`, demand ≈ 113.0 MiB —
/// would refuse), so the single `infer` call below only succeeds if the
/// reservation charge read the declared value.
#[test]
fn placement_uses_the_declared_kv_per_token_not_the_gguf_value() {
    let dir = fresh_dir("bloomery-pager-kv-tuning-placement");
    let weights = MIB;
    let free_vram = 40 * MIB; // budget == free_vram: pager_in's one closure serves both.
    let (mut p, _, _) = pager_in(&dir, 1, Some(free_vram));
    let gguf = write_gguf(&dir, "qwen.gguf", b"weights");
    p.register_model("qwen", &gguf, meta(weights), None)
        .unwrap();
    p.set_kv_per_token_bytes("qwen", Some(DECLARED_KV_PER_TOKEN))
        .unwrap();

    let a1 = p.create_agent("qwen", 50, Some(2048), 10_000).unwrap();
    assert_eq!(
        a1.window_tokens, 2048,
        "user_cap must bind here (see doc comment), so kv_bytes below is deterministic"
    );

    p.infer(&a1.id, "hello", 16, None).expect(
        "demand = weights (1 MiB) + kv_bytes (2048 * declared 14336 B/token ≈ 28.0 MiB) ≈ \
         29.0 MiB, under the 40 MiB budget — if the reservation charge read the GGUF-derived \
         57344 B/token instead, demand (≈113.0 MiB) would blow well past the budget and refuse",
    );
}

/// `/status`'s `kv_per_token` reports the declared override, and
/// `kv_per_token_declared` is `true` — spec §10's naming rule: "a declared
/// number must never read as a measured one" restated the other way around
/// for `/status`.
#[test]
fn status_reports_declared_kv_per_token_and_the_declared_flag_when_override_active() {
    let dir = fresh_dir("bloomery-pager-kv-tuning-status-declared");
    let (mut p, _, _) = pager_in(&dir, 0, Some(1000 * MIB));
    let gguf = write_gguf(&dir, "qwen.gguf", b"weights");
    p.register_model("qwen", &gguf, meta(10 * MIB), None)
        .unwrap();
    p.set_kv_per_token_bytes("qwen", Some(DECLARED_KV_PER_TOKEN))
        .unwrap();

    let status = p.status();
    let model = &status.models[0];
    assert_eq!(model.kv_per_token, DECLARED_KV_PER_TOKEN);
    assert!(
        model.kv_per_token_declared,
        "a declared override must be flagged, never silently read as measured"
    );
}

/// The mirror: no override at all reports the GGUF-derived value, and the
/// declared flag is `false` — never a confident-looking declared reading for
/// a number nobody actually declared.
#[test]
fn status_reports_gguf_kv_per_token_and_no_declared_flag_when_absent() {
    let dir = fresh_dir("bloomery-pager-kv-tuning-status-absent");
    let (mut p, _, _) = pager_in(&dir, 0, Some(1000 * MIB));
    let gguf = write_gguf(&dir, "qwen.gguf", b"weights");
    p.register_model("qwen", &gguf, meta(10 * MIB), None)
        .unwrap();
    // No set_kv_per_token_bytes call at all.

    let status = p.status();
    let model = &status.models[0];
    assert_eq!(model.kv_per_token, GGUF_KV_PER_TOKEN);
    assert!(
        !model.kv_per_token_declared,
        "no override means the value is measured (GGUF-derived), not declared"
    );
}

/// A declared value LARGER than the GGUF-derived figure is NOT clamped
/// (spec §10 addendum: "No clamp against the GGUF value... a declared value
/// larger than GGUF is allowed (extra conservative), smaller is the point")
/// — unlike `weights_vram_mib`'s `min(declared, file)`. `/status` reports
/// the declared value raw, larger than the GGUF-derived one.
#[test]
fn a_declared_kv_per_token_larger_than_gguf_is_not_clamped() {
    let dir = fresh_dir("bloomery-pager-kv-tuning-no-clamp");
    let (mut p, _, _) = pager_in(&dir, 0, Some(1000 * MIB));
    let gguf = write_gguf(&dir, "qwen.gguf", b"weights");
    p.register_model("qwen", &gguf, meta(10 * MIB), None)
        .unwrap();
    let larger_declared = GGUF_KV_PER_TOKEN * 3;
    p.set_kv_per_token_bytes("qwen", Some(larger_declared))
        .unwrap();

    let status = p.status();
    let model = &status.models[0];
    assert_eq!(
        model.kv_per_token, larger_declared,
        "unlike weights, a larger declared kv_per_token is used as-is, not clamped down"
    );
    assert!(model.kv_per_token_declared);
}

/// `set_kv_per_token_bytes` on an unregistered model name is refused with
/// `UnknownModel`, naming the model — same contract as `set_model_tuning`.
#[test]
fn set_kv_per_token_bytes_on_unknown_model_is_refused() {
    let dir = fresh_dir("bloomery-pager-kv-tuning-unknown");
    let (mut p, _, _) = pager_in(&dir, 0, Some(300 * MIB));
    match p.set_kv_per_token_bytes("nope", Some(1)) {
        Err(PagerError::UnknownModel(name)) => assert_eq!(name, "nope"),
        other => panic!("expected UnknownModel, got {other:?}"),
    }
}
