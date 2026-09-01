//! Task 5 (live-run finding): a context reserves more VRAM than its KV cache.
//!
//! The 2026-08-14 natural-pressure run died of `ErrorOutOfDeviceMemory` with
//! the pager believing it had room. Its `daemon.log` says why: on this box,
//! at `n_ctx = 16384`, llama.cpp allocated **896 MiB of KV cache plus a
//! 304 MiB `Vulkan0` compute buffer and a 30 MiB host buffer per context**.
//! Placement charged the 896 and nothing else, so the pager planned six
//! residents where five fit, and the sixth allocation OOM'd — a refusal that
//! should have been arithmetic became a substrate error.
//!
//! These scenarios pin the fix: residency reserves `kv_bytes +
//! ctx_overhead_bytes` per context, and the daemon-level `overhead_bytes`
//! margin is subtracted from the placement budget as well as from the window
//! law.
//!
//! Shared geometry is `pager_test.rs`'s "qwen" shape (28 layers × 4 kv-heads
//! × 128 head-dim → `kv_per_token = 57_344`) with `window_cap = 1024`, so a
//! context's KV is exactly `1024 × 57_344 = 58_720_256 B` (56 MiB) and every
//! budget below is an exact multiple of 1 MiB. GPU-free: everything drives
//! `FakeSubstrate`.

use bloomery_core::journal::{replay, Event, Journal, PagerOpKind};
use bloomery_daemon::agents::ImageStore;
use bloomery_daemon::pager::*;
use bloomery_substrate::{fake::FakeSubstrate, Reply};
use std::path::{Path, PathBuf};

const MIB: u64 = 1024 * 1024;
const WINDOW_CAP: u32 = 1024;
/// 1024 tokens × 57_344 B = 56 MiB.
const KV_BYTES: u64 = 1024 * 57_344;

fn ok(text: &str) -> Reply {
    Reply {
        text: text.into(),
        prompt_tokens: Some(8),
        completion_tokens: Some(4),
        duration_ms: 3,
    }
}

fn meta(weights_bytes: u64) -> bloomery_core::gguf::GgufMeta {
    bloomery_core::gguf::GgufMeta {
        arch: "qwen2".into(),
        layers: 28,
        attention_layers: 28,
        kv_heads: 4,
        head_dim: 128,
        training_ctx: 4096,
        weights_bytes,
        value_length: None,
        recurrent_state_bytes: 0,
    }
}

fn fresh_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn pager_in(dir: &Path, replies: usize, free_vram: Option<u64>) -> (Pager<FakeSubstrate>, PathBuf) {
    let jpath = dir.join("j.jsonl");
    let journal = Journal::open(&jpath).unwrap();
    let images = ImageStore::new(&dir.join("img")).unwrap();
    let mut fake = FakeSubstrate::new();
    for _ in 0..replies {
        fake.script_reply(ok("r"));
    }
    let p = Pager::new(fake, journal, images, Box::new(move || free_vram));
    (p, jpath)
}

fn write_gguf(dir: &Path, name: &str) -> PathBuf {
    let gguf = dir.join(name);
    std::fs::write(&gguf, b"weights").unwrap();
    gguf
}

/// **The live run's failure, as arithmetic.**
///
/// budget 260 MiB, weights 200 MiB, one context: KV 56 MiB, per-context
/// reservation 32 MiB.
///
/// * Charging KV only — Phase 2a as shipped — gives `200 + 56 = 256 ≤ 260`:
///   **fits**, and the substrate is asked for a context it cannot have.
/// * Charging the reservation gives `200 + 56 + 32 = 288 > 260`: **refused**,
///   before anything is allocated.
///
/// The gap between those two lines is exactly what OOM'd the GPU on
/// 2026-08-14, so the test asserts both halves: the refusal *and* the fact
/// that the un-reserved arithmetic would have said yes.
///
/// **Amended for item 7 (task 1, closed 2026-08-15).** This test originally
/// pinned that gap "by hand" on a single, otherwise-alone agent — an
/// explicit `window_cap` and a budget chosen to sit between "KV alone
/// fits" and "KV + ctx_overhead doesn't." Item 7's fix makes that exact
/// shape provably unreachable for a first agent windowed against an empty
/// pager: `usable_window`'s VRAM term now derives from the same four terms
/// placement charges, so `demand <= avail` holds by construction whichever
/// term binds the window (proved directly by
/// `a_vram_bound_window_is_placeable_item_7_regression` above — see its
/// doc comment for the general argument). Charging KV only would still say
/// yes here; the difference is that the *window law itself* now shrinks a
/// first agent's window to whatever the reservation can actually afford,
/// so it never gets the chance to ask for more.
///
/// **Scope, precisely** (narrowed after review — this does *not* pin item
/// 7's still-open "third half"; see
/// `a_sibling_blind_automatic_window_still_refuses_item_7_third_half`
/// below for that). Both agents here pass an explicit `Some(WINDOW_CAP)`:
/// a2's own sibling-blind VRAM candidate is `(320 − 200 − 32) MiB /
/// 57 344 B ≈ 1609 tokens`, far above `WINDOW_CAP` (1024), so `UserCap`
/// binds regardless of what the window law knows about a1's residency —
/// the window law's blindness never actually drives this outcome. What
/// this test *does* show: two agents, same model, same explicit
/// `window_cap`; the first becomes resident and reserves `KV_BYTES +
/// ctx_overhead`; the second's *placement* correctly subtracts the
/// first's whole reservation — not just its KV — from what's left, and
/// that whole-reservation subtraction (already delivered in Phase 2a,
/// item 1) is what tips the second agent into a refusal a KV-only charge
/// would not have produced. A multi-agent regression check on already-
/// delivered behavior, not a pin of item 7's remaining gap.
#[test]
fn a_second_agents_reservation_not_just_its_kv_is_what_refuses_it() {
    let dir = fresh_dir("bloomery-pager-reservation-refuse");
    let budget = 320 * MIB;
    let weights = 200 * MIB;
    let ctx_overhead = 32 * MIB;

    let (mut p, jpath) = pager_in(&dir, 2, Some(budget));
    p.set_ctx_overhead_bytes(ctx_overhead);
    let gguf = write_gguf(&dir, "qwen.gguf");
    p.register_model("qwen", &gguf, meta(weights), None)
        .unwrap();

    // a1 outranks a2 so it can never be evicted to make room for it — any
    // refusal below is a hard refusal, not a missed eviction opportunity.
    let a1 = p
        .create_agent("qwen", 100, Some(WINDOW_CAP), 10_000)
        .unwrap();
    p.infer(&a1.id, "hello from a1", 16, None).unwrap();
    let a1_reserved = KV_BYTES + ctx_overhead;
    assert_eq!(
        p.status().resident_kv_bytes,
        a1_reserved,
        "a1 must be resident, holding its whole reservation, before a2 is even created"
    );

    // The boundary this test lives on, stated as arithmetic so a later edit
    // to any constant cannot quietly move the scenario off it: KV alone
    // (both agents, weights loaded once) fits the budget; a2's whole
    // reservation, charged against what a1's whole reservation already
    // used up, does not.
    assert!(
        weights + 2 * KV_BYTES <= budget,
        "precondition: KV alone must fit both agents, or this tests nothing"
    );
    assert!(
        weights + KV_BYTES + a1_reserved > budget,
        "precondition: a1's whole reservation is what tips a2 over"
    );

    let a2 = p
        .create_agent("qwen", 50, Some(WINDOW_CAP), 10_000)
        .unwrap();
    let loads_before = p
        .substrate()
        .calls()
        .iter()
        .filter(|c| c.as_str() == "load_model")
        .count();
    let creates_before = p
        .substrate()
        .calls()
        .iter()
        .filter(|c| c.as_str() == "create_context")
        .count();

    let a2_reserved = KV_BYTES + ctx_overhead;
    let avail = budget - weights - a1_reserved;
    match p.infer(&a2.id, "hello from a2", 16, None) {
        Err(PagerError::Refused {
            needed,
            free,
            reclaimable,
            max_placeable_tokens,
        }) => {
            assert_eq!(
                needed, a2_reserved,
                "a2's demand is its whole reservation; qwen's weights are already loaded"
            );
            assert_eq!(
                free, avail,
                "avail is the budget minus loaded weights minus a1's whole reservation"
            );
            assert_eq!(reclaimable, 0, "a1 outranks a2, so nothing is reclaimable");
            // Slice C's advice. `avail` here is exactly 32 MiB — and so is
            // `ctx_overhead`, which every context reserves BEFORE its first
            // KV token. The per-context reservation alone consumes the whole
            // remainder, so there is no window at all, however small:
            // `Some(0)`.
            //
            // This is the reservation-shaped sibling of the weights-shaped
            // zero in `pager_weights_test.rs`. Both say the same actionable
            // thing — do not retry with a smaller `window_cap`, free
            // something first — which a bare 409 could not.
            assert_eq!(
                max_placeable_tokens,
                Some(0),
                "ctx_overhead alone eats the remaining budget: no window fits"
            );
        }
        other => panic!("expected Refused, got {other:?}"),
    }

    // Law 1: nothing reaches the substrate on a refused path. This is the
    // property the live run lost — it discovered the shortfall *from* a
    // failed allocation instead of before one.
    let loads_after = p
        .substrate()
        .calls()
        .iter()
        .filter(|c| c.as_str() == "load_model")
        .count();
    assert_eq!(
        loads_after, loads_before,
        "a2's weights must not reach the substrate"
    );
    let creates_after = p
        .substrate()
        .calls()
        .iter()
        .filter(|c| c.as_str() == "create_context")
        .count();
    assert_eq!(
        creates_after, creates_before,
        "no new context may be created on a refused path (a1's own context still counts)"
    );

    // The rendered arithmetic, byte for byte: a reader must be able to see
    // the reservation split rather than re-derive it.
    let expected_detail = format!(
        "residency: weights 0 B + reserved {a2_reserved} B (kv {KV_BYTES} B + ctx overhead \
         {ctx_overhead} B) vs budget {budget} B − overhead 0 B − loaded {weights} B − \
         resident {a1_reserved} B (needed {a2_reserved} B, free {avail} B, reclaimable 0 B, largest placeable window 0 tokens)"
    );
    let events = replay(&jpath).unwrap();
    assert!(
        events.iter().any(|e| matches!(e,
            Event::Refusal { id, detail, .. } if id == &a2.id && detail == &expected_detail)),
        "expected detail {expected_detail:?} not found in {events:?}"
    );
}

/// The daemon-level `overhead_bytes` margin is subtracted from the placement
/// budget too, not only from the window law.
///
/// budget 320 MiB, weights 200 MiB, overhead 32 MiB, two 56 MiB contexts,
/// per-context reservation zero (isolating the global term).
///
/// * Without the overhead in placement: `avail` for a2 is
///   `320 − 200 − 56 = 64 MiB ≥ 56 MiB` → **fits**, a1 stays resident.
/// * With it: `320 − 32 − 200 − 56 = 32 MiB < 56 MiB` → a1 (strictly lower
///   priority) is **evicted** to make room.
///
/// The window law is deliberately not the binding term here:
/// `(320 − 200 − 32) MiB / 57 344 = 1608 tokens`, well above the 1024 cap, so
/// both agents still get exactly `KV_BYTES`.
#[test]
fn the_global_overhead_margin_is_subtracted_from_placement_too() {
    let dir = fresh_dir("bloomery-pager-reservation-overhead");
    let budget = 320 * MIB;
    let weights = 200 * MIB;
    let overhead = 32 * MIB;

    assert!(
        weights + 2 * KV_BYTES <= budget,
        "precondition: both contexts fit when the overhead is ignored"
    );
    assert!(
        weights + overhead + 2 * KV_BYTES > budget,
        "precondition: the overhead is what forces the eviction"
    );

    let (mut p, jpath) = pager_in(&dir, 2, Some(budget));
    p.set_overhead_bytes(overhead);
    let gguf = write_gguf(&dir, "qwen.gguf");
    p.register_model("qwen", &gguf, meta(weights), None)
        .unwrap();
    let a1 = p
        .create_agent("qwen", 50, Some(WINDOW_CAP), 10_000)
        .unwrap();
    let a2 = p
        .create_agent("qwen", 100, Some(WINDOW_CAP), 10_000)
        .unwrap();
    assert_eq!(
        p.status()
            .agents
            .iter()
            .find(|a| a.id == a1.id)
            .expect("a1 in status")
            .window_tokens,
        WINDOW_CAP,
        "the window law must not be the binding term in this scenario"
    );

    p.infer(&a1.id, "hello from a1", 16, None)
        .expect("weights + one ctx fits inside the overhead-reduced budget");
    p.infer(&a2.id, "hello from a2", 16, None)
        .expect("a1 is strictly lower priority, so this evicts rather than refuses");

    let events = replay(&jpath).unwrap();
    assert!(
        events.iter().any(|e| matches!(e,
            Event::PagerOp { id, op: PagerOpKind::EvictSave, .. } if id == &a1.id)),
        "the overhead term must be what forces a1 out: {events:?}"
    );
}

/// `/status` surfaces what residency actually holds, and the two terms that
/// explain it.
///
/// `kv_bytes` and `resident_kv_bytes` keep their names and now carry the
/// *reserved* figure — the number placement decides on. Reporting the raw KV
/// there would be a value that looks like the accounting and is not.
#[test]
fn status_reports_reserved_bytes_and_both_overhead_terms() {
    let dir = fresh_dir("bloomery-pager-reservation-status");
    let ctx_overhead = 32 * MIB;
    let overhead = 16 * MIB;
    let (mut p, _) = pager_in(&dir, 1, Some(4096 * MIB));
    p.set_ctx_overhead_bytes(ctx_overhead);
    p.set_overhead_bytes(overhead);
    let gguf = write_gguf(&dir, "qwen.gguf");
    p.register_model("qwen", &gguf, meta(200 * MIB), None)
        .unwrap();
    let a1 = p
        .create_agent("qwen", 50, Some(WINDOW_CAP), 10_000)
        .unwrap();

    let status = p.status();
    assert_eq!(status.overhead_bytes, overhead);
    assert_eq!(status.ctx_overhead_bytes, ctx_overhead);
    assert_eq!(
        status.agents[0].kv_bytes,
        KV_BYTES + ctx_overhead,
        "per-agent figure is what residency reserves, not the bare KV"
    );
    assert_eq!(
        status.resident_kv_bytes, 0,
        "a fresh agent reserves nothing until it is placed"
    );

    p.infer(&a1.id, "hello", 16, None).unwrap();
    assert_eq!(
        p.status().resident_kv_bytes,
        KV_BYTES + ctx_overhead,
        "a resident holds its whole reservation"
    );
}

/// Evicting a context credits its *whole* reservation back, not just the KV
/// half — the compute buffer is freed with the context, which is the reason
/// the reservation is legitimate to plan against at all.
#[test]
fn eviction_credits_the_whole_reservation_back() {
    let dir = fresh_dir("bloomery-pager-reservation-credit");
    let ctx_overhead = 32 * MIB;
    let (mut p, _) = pager_in(&dir, 2, Some(4096 * MIB));
    p.set_ctx_overhead_bytes(ctx_overhead);
    let gguf = write_gguf(&dir, "qwen.gguf");
    p.register_model("qwen", &gguf, meta(200 * MIB), None)
        .unwrap();
    let a1 = p
        .create_agent("qwen", 50, Some(WINDOW_CAP), 10_000)
        .unwrap();
    p.infer(&a1.id, "hello", 16, None).unwrap();
    assert_eq!(p.status().resident_kv_bytes, KV_BYTES + ctx_overhead);

    p.suspend(&a1.id).unwrap();
    assert_eq!(
        p.status().resident_kv_bytes,
        0,
        "the reservation is released in full when the context goes"
    );
}

/// **Item 7 regression pin (task 1).** Before the geometry fix,
/// `usable_window`'s VRAM term charged `weights` and `overhead` but not
/// `ctx_overhead`, so an automatically VRAM-bound window (no `window_cap`)
/// was sized to consume the *entire* remaining budget and then reserved
/// exactly `ctx_overhead_bytes` more than that — permanently unplaceable,
/// refused every time with `needed − free == ctx_overhead_bytes`. This is
/// the live 2026-08-15 14B attempt's failure shape, scaled down and made
/// exact.
///
/// The numbers, chosen so both the pre-fix and post-fix windows land on
/// whole token counts: `weights = 200 MiB`, `kv_per_token = 57_344 B`
/// (this file's qwen shape), `ctx_overhead = 4 × kv_per_token =
/// 229_376 B`. `budget` is set to exactly `weights + 104 × kv_per_token`,
/// so:
///
/// * **Pre-fix**, the VRAM term ignores `ctx_overhead`: window =
///   `(budget − weights) / kv_per_token` = 104 tokens. Its reservation is
///   `104 × kv_per_token + ctx_overhead` = 4 tokens' worth over budget —
///   refused, with `needed − free` exactly `ctx_overhead_bytes`.
/// * **Post-fix**, the VRAM term also subtracts `ctx_overhead`: window =
///   `(budget − weights − ctx_overhead) / kv_per_token` = 100 tokens. Its
///   reservation (`100 × kv_per_token + ctx_overhead`) equals `budget`
///   exactly — placeable by construction (`plan_residency`'s `<=`), so the
///   agent both creates *and* places: `infer` succeeds.
#[test]
fn a_vram_bound_window_is_placeable_item_7_regression() {
    let dir = fresh_dir("bloomery-pager-reservation-item7");
    let weights = 200 * MIB;
    let kv_per_token = 57_344u64;
    let ctx_overhead = 4 * kv_per_token; // 229_376 B
    let post_fix_tokens = 100u64;
    let pre_fix_tokens = post_fix_tokens + ctx_overhead / kv_per_token; // 104
    let budget = weights + pre_fix_tokens * kv_per_token;

    let (mut p, _) = pager_in(&dir, 1, Some(budget));
    p.set_ctx_overhead_bytes(ctx_overhead);
    let gguf = write_gguf(&dir, "qwen.gguf");
    p.register_model("qwen", &gguf, meta(weights), None)
        .unwrap();

    // No window_cap: the window law itself must land on `Vram`, not an
    // operator-supplied number, for this to pin the automatic-window bug.
    let a1 = p.create_agent("qwen", 50, None, 10_000).unwrap();
    assert_eq!(
        a1.window_tokens, post_fix_tokens as u32,
        "the fixed VRAM term must charge ctx_overhead, landing on 100 tokens \
         (104 would mean the old, unfixed arithmetic)"
    );
    assert_eq!(
        a1.bound_by, "vram",
        "this scenario must be VRAM-bound to pin item 7"
    );

    p.infer(&a1.id, "hello", 16, None)
        .expect("a VRAM-bound window must be placeable by construction (item 7 closed)");
}

/// **Item 7's still-open "third half," pinned directly** (added on review —
/// see `a_second_agents_reservation_not_just_its_kv_is_what_refuses_it`
/// above, which was found not to exercise this after all: both its agents
/// use an explicit `window_cap` small enough that `UserCap` binds
/// regardless of what the window law knows about residency, so the window
/// law's sibling-blindness was never actually what drove that test's
/// refusal).
///
/// This test gives **both** agents no `window_cap`, so each one's own
/// window law runs the automatic `Vram` candidate — and that candidate is
/// computed from `budget − <this model's> weights − overhead −
/// ctx_overhead` alone (`crates/bloomery-core/src/geometry.rs`), with no
/// term for what a resident sibling already holds. `budget` is chosen so
/// a1 alone consumes it exactly: `budget = weights + a1_tokens ×
/// kv_per_token + ctx_overhead`, i.e. `a1`'s own automatic window (500
/// tokens) reserves the *entire* budget with nothing left over.
///
/// `a2` is created after `a1` is already resident. Because the window law
/// reads only `budget` and `qwen`'s own `weights_bytes` — the same two
/// numbers it read for `a1`, with no notion that `a1` is now resident —
/// it runs the identical arithmetic and lands on the identical answer:
/// `a2` is also sized to 500 tokens, `Vram`-bound, **exactly as if it were
/// the only agent**, despite `a1` already holding everything there is to
/// hold. That identical, sibling-blind 500-token answer is the defect
/// item 7's third half names, asserted directly below (not inferred from
/// a refusal that could have another cause).
///
/// Placement then does the correct, already-delivered thing: it subtracts
/// `a1`'s whole reservation from `budget`, leaving **zero** bytes free,
/// so `a2`'s demand (`a2`'s own reservation, weights already loaded)
/// refuses outright. A sibling-aware window law would have sized `a2` to
/// `⌊avail / kv_per_token⌋ = ⌊0 / 57_344⌋ = 0` tokens instead of 500 —
/// this test pins the current, honest gap between what the window law
/// *does* compute (500, oversized) and what a fix would have to compute
/// (0, correctly starved) so that a future third-half fix must
/// consciously change this test's `a2.window_tokens` assertion, not just
/// its refusal.
#[test]
fn a_sibling_blind_automatic_window_still_refuses_item_7_third_half() {
    let dir = fresh_dir("bloomery-pager-reservation-item7-third-half");
    let weights = 200 * MIB;
    let kv_per_token = 57_344u64;
    let ctx_overhead = 32 * MIB;
    let a1_tokens = 500u64;
    // Chosen so a1's own automatic window reserves the budget exactly,
    // leaving nothing for a2 to find even if a2's window law were somehow
    // right — the point is that it never gets the chance to be right,
    // because it never looks.
    let budget = weights + a1_tokens * kv_per_token + ctx_overhead;

    let (mut p, jpath) = pager_in(&dir, 2, Some(budget));
    p.set_ctx_overhead_bytes(ctx_overhead);
    let gguf = write_gguf(&dir, "qwen.gguf");
    p.register_model("qwen", &gguf, meta(weights), None)
        .unwrap();

    // a1 outranks a2 so it can never be evicted to make room for it.
    let a1 = p.create_agent("qwen", 100, None, 10_000).unwrap();
    assert_eq!(
        (a1.window_tokens, a1.bound_by.as_str()),
        (a1_tokens as u32, "vram"),
        "a1, alone against the whole budget, must land on the automatic Vram term"
    );
    p.infer(&a1.id, "hello from a1", 16, None).unwrap();
    let a1_reserved = a1_tokens * kv_per_token + ctx_overhead;
    assert_eq!(
        p.status().resident_kv_bytes,
        a1_reserved,
        "a1 must be resident, holding its whole reservation, before a2 is even created"
    );

    // The defect, asserted directly: a2's own window law, run after a1 is
    // already resident, computes the SAME 500-token Vram-bound answer a1
    // did — sibling-blind, as if a2 were alone.
    let a2 = p.create_agent("qwen", 50, None, 10_000).unwrap();
    assert_eq!(
        (a2.window_tokens, a2.bound_by.as_str()),
        (a1_tokens as u32, "vram"),
        "a2's window law is blind to a1's residency: it computes the identical, \
         oversized 500-token window a1 got, not a sibling-aware smaller one"
    );

    // The consequence: placement's whole-reservation subtraction (already
    // correct since Phase 2a's item 1) finds nothing left at all.
    let a2_reserved = a1_tokens * kv_per_token + ctx_overhead;
    let avail = budget - weights - a1_reserved;
    assert_eq!(
        avail, 0,
        "precondition: a1's automatic window was sized to consume the whole \
         budget exactly, or this test doesn't isolate the sibling-blindness"
    );

    match p.infer(&a2.id, "hello from a2", 16, None) {
        Err(PagerError::Refused {
            needed,
            free,
            reclaimable,
            max_placeable_tokens,
        }) => {
            assert_eq!(
                needed, a2_reserved,
                "a2's demand is its whole (oversized) reservation; qwen is already loaded"
            );
            assert_eq!(free, avail, "avail is exactly zero: a1 left nothing");
            assert_eq!(reclaimable, 0, "a1 outranks a2, so nothing is reclaimable");
            // **What slice C changes about item 7's third half, asserted on
            // the test that pins it.** The sibling-blindness is UNFIXED and
            // this test still passes unaltered around this line: a2's window
            // law is still blind to resident a1, still computes the same
            // oversized window, and placement still refuses it.
            //
            // What is no longer true is the item's other complaint — "no
            // smaller window to fall back to and no recovery". The refusal
            // now says `Some(0)`, which is the honest answer here because
            // `avail` is exactly zero: a1's automatic window consumed the
            // entire budget, so no `window_cap` a2 could name would place
            // while a1 is resident. The caller learns the blocker is the
            // SIBLING, not its own window size — that is the recovery, and
            // it is a different fact from the `Some(2048)` a caller gets when
            // a smaller window really would work.
            assert_eq!(
                max_placeable_tokens,
                Some(0),
                "a1 left nothing, so no window a2 could ask for would place"
            );
        }
        other => panic!("expected Refused, got {other:?}"),
    }

    let expected_detail = format!(
        "residency: weights 0 B + reserved {a2_reserved} B (kv {kv} B + ctx overhead \
         {ctx_overhead} B) vs budget {budget} B − overhead 0 B − loaded {weights} B − \
         resident {a1_reserved} B (needed {a2_reserved} B, free {avail} B, reclaimable 0 B, largest placeable window 0 tokens)",
        kv = a1_tokens * kv_per_token,
    );
    let events = replay(&jpath).unwrap();
    assert!(
        events.iter().any(|e| matches!(e,
            Event::Refusal { id, detail, .. } if id == &a2.id && detail == &expected_detail)),
        "expected detail {expected_detail:?} not found in {events:?}"
    );
}

fn hybrid_meta(weights_bytes: u64) -> bloomery_core::gguf::GgufMeta {
    bloomery_core::gguf::GgufMeta {
        arch: "qwen35moe".into(),
        layers: 40,
        attention_layers: 10,
        kv_heads: 2,
        head_dim: 256,
        training_ctx: 4096,
        weights_bytes,
        value_length: None,
        recurrent_state_bytes: 65_863_680,
    }
}

/// Turn-5 spec §2: a hybrid model's recurrent state is a per-context
/// constant charged beside `ctx_overhead_bytes` — in the window law AND in
/// the agent's reservation — and surfaced on `/status` per model.
#[test]
fn recurrent_state_is_charged_per_context_and_reported() {
    let dir = fresh_dir("bloomery-resv-recurrent");
    let (mut p, _j) = pager_in(&dir, 0, Some(4096 * MIB));
    p.set_ctx_overhead_bytes(8 * MIB);
    let gguf = write_gguf(&dir, "h.gguf");
    p.register_model("h", &gguf, hybrid_meta(200 * MIB), None)
        .unwrap();
    let a = p.create_agent("h", 100, Some(WINDOW_CAP), 1000).unwrap();
    assert_eq!(
        a.window_tokens, WINDOW_CAP,
        "the cap binds; the recurrent charge must not starve it at this budget"
    );
    let st = p.status();
    let agent = st.agents.iter().find(|x| x.id == a.id).unwrap();
    // kv = 1024 tokens * (2*10*2*256*2 = 20_480) = 20_971_520
    assert_eq!(
        agent.kv_bytes,
        20_971_520 + 8 * MIB + 65_863_680,
        "reserved = kv + ctx_overhead + recurrent_state"
    );
    let model = st.models.iter().find(|m| m.name == "h").unwrap();
    assert_eq!(model.recurrent_state_bytes, 65_863_680);
    assert_eq!(model.kv_per_token, 20_480);
}

/// The test above only ever asserts `window_tokens == WINDOW_CAP` — a value
/// that would be identical whether or not the window law's `Vram` term
/// charges `recurrent_state_bytes` at all, since the explicit `UserCap`
/// binds regardless. This test pins the actual charge SITE: an agent with
/// no `window_cap`, so the window law's own `Vram` candidate is what binds,
/// and its exact closed form is `(free_vram − weights − overhead_bytes −
/// ctx_overhead_bytes − recurrent_state_bytes) / kv_per_token`.
///
/// `free_vram` is chosen (via `post_fix_tokens`) so that closed form lands
/// on a clean 1_000 tokens, comfortably under `hybrid_meta`'s
/// `training_ctx` (4_096) so `Vram`, not `TrainingCtx`, is what binds.
///
/// `recurrent_state_bytes` (65_863_680) is itself an exact multiple of
/// `kv_per_token` (20_480) — 3_216 tokens' worth — so a window law that
/// forgot to fold it into `ctx_overhead_bytes` would land exactly 3_216
/// tokens higher; that's asserted directly as a second, independent closed
/// form, not merely restated from `window_tokens`.
#[test]
fn recurrent_state_binds_the_vram_term_of_the_window_law() {
    let dir = fresh_dir("bloomery-resv-recurrent-vram-bound");
    let weights = 200 * MIB;
    let ctx_overhead = 8 * MIB;
    let kv_per_token = 20_480u64;
    let recurrent = 65_863_680u64;
    let post_fix_tokens = 1_000u64;
    let free_vram = post_fix_tokens * kv_per_token + weights + ctx_overhead + recurrent;

    let (mut p, _j) = pager_in(&dir, 0, Some(free_vram));
    p.set_ctx_overhead_bytes(ctx_overhead);
    let gguf = write_gguf(&dir, "h.gguf");
    p.register_model("h", &gguf, hybrid_meta(weights), None)
        .unwrap();

    // No window_cap: the window law's own Vram candidate must bind.
    let a = p.create_agent("h", 100, None, 10_000).unwrap();
    assert_eq!(
        a.bound_by, "vram",
        "this scenario must be VRAM-bound to pin the recurrent charge site"
    );
    assert_eq!(
        a.window_tokens, post_fix_tokens as u32,
        "window = (free_vram - weights - ctx_overhead - recurrent_state_bytes) / kv_per_token"
    );

    let no_recurrent_tokens = (free_vram - weights - ctx_overhead) / kv_per_token;
    assert_eq!(
        a.window_tokens as u64 + 3_216,
        no_recurrent_tokens,
        "a window law that forgot the recurrent term would land exactly 3_216 \
         tokens (recurrent_state_bytes / kv_per_token) higher"
    );
}
