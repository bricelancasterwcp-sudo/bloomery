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
        kv_heads: 4,
        head_dim: 128,
        training_ctx: 4096,
        weights_bytes,
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
/// What survives is exactly item 7's still-open "third half" (see
/// `CARRIED-DEBT.md`): the window law sizes a *new* agent against the
/// whole static budget, blind to what a resident **sibling** already
/// holds. Two agents, same model, same explicit `window_cap`: the first
/// becomes resident and reserves `KV_BYTES + ctx_overhead`; the second's
/// own window law computes room fine (it has no notion of the first
/// agent's residency), but its *placement* correctly subtracts the first
/// agent's whole reservation — not just its KV — from what's left, and
/// that whole-reservation subtraction is what tips the second agent into
/// a refusal a KV-only charge would not have produced.
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
    p.infer(&a1.id, "hello from a1", 16).unwrap();
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
    match p.infer(&a2.id, "hello from a2", 16) {
        Err(PagerError::Refused {
            needed,
            free,
            reclaimable,
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
         resident {a1_reserved} B (needed {a2_reserved} B, free {avail} B, reclaimable 0 B)"
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

    p.infer(&a1.id, "hello from a1", 16)
        .expect("weights + one ctx fits inside the overhead-reduced budget");
    p.infer(&a2.id, "hello from a2", 16)
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

    p.infer(&a1.id, "hello", 16).unwrap();
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
    p.infer(&a1.id, "hello", 16).unwrap();
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

    p.infer(&a1.id, "hello", 16)
        .expect("a VRAM-bound window must be placeable by construction (item 7 closed)");
}
