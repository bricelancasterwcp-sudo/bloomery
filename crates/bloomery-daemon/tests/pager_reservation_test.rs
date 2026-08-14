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
#[test]
fn a_context_whose_kv_fits_but_whose_reservation_does_not_is_refused() {
    let dir = fresh_dir("bloomery-pager-reservation-refuse");
    let budget = 260 * MIB;
    let weights = 200 * MIB;
    let ctx_overhead = 32 * MIB;

    // The boundary this test lives on, stated as arithmetic so a later edit
    // to any constant cannot quietly move the scenario off it.
    assert!(
        weights + KV_BYTES <= budget,
        "precondition: KV alone must fit, or this tests nothing"
    );
    assert!(
        weights + KV_BYTES + ctx_overhead > budget,
        "precondition: the reservation must be what tips it over"
    );

    let (mut p, jpath) = pager_in(&dir, 1, Some(budget));
    p.set_ctx_overhead_bytes(ctx_overhead);
    let gguf = write_gguf(&dir, "qwen.gguf");
    p.register_model("qwen", &gguf, meta(weights), None)
        .unwrap();
    let a1 = p
        .create_agent("qwen", 50, Some(WINDOW_CAP), 10_000)
        .unwrap();
    let loads_before = p
        .substrate()
        .calls()
        .iter()
        .filter(|c| c.as_str() == "load_model")
        .count();

    match p.infer(&a1.id, "hello", 16) {
        Err(PagerError::Refused {
            needed,
            free,
            reclaimable,
        }) => {
            assert_eq!(
                needed,
                weights + KV_BYTES + ctx_overhead,
                "demand carries the weights and the whole reservation"
            );
            assert_eq!(free, budget, "nothing is loaded or resident yet");
            assert_eq!(reclaimable, 0, "no resident to reclaim from");
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
        "weights must not reach the substrate"
    );
    assert!(
        !p.substrate()
            .calls()
            .iter()
            .any(|c| c.as_str() == "create_context"),
        "no context may be created on a refused path"
    );

    // The rendered arithmetic, byte for byte: a reader must be able to see
    // the reservation split rather than re-derive it.
    let expected_detail = format!(
        "residency: weights {weights} B + reserved {reserved} B (kv {KV_BYTES} B + ctx overhead \
         {ctx_overhead} B) vs budget {budget} B − overhead 0 B − loaded 0 B − resident 0 B \
         (needed {needed} B, free {budget} B, reclaimable 0 B)",
        reserved = KV_BYTES + ctx_overhead,
        needed = weights + KV_BYTES + ctx_overhead,
    );
    let events = replay(&jpath).unwrap();
    assert!(
        events.iter().any(|e| matches!(e,
            Event::Refusal { id, detail, .. } if id == &a1.id && detail == &expected_detail)),
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
