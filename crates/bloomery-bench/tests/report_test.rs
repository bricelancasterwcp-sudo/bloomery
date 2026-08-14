//! The G2 gate math, pinned against synthetic journals.
//!
//! These tests are the gate's definition of a "switch sample": if the
//! classification or the p95 index formula changes, the gate is being read
//! against different arithmetic and the change is a recorded protocol
//! amendment, not a refactor.

use bloomery_bench::report::{compute_report, report_json};
use bloomery_core::journal::{Event, PagerOpKind};

fn evict(id: &str, ms: u64) -> Event {
    Event::PagerOp {
        id: id.into(),
        op: PagerOpKind::EvictSave,
        bytes: 1,
        duration_ms: ms,
        image_tier: "ram".into(),
    }
}

fn resume(id: &str, ms: u64, tier: &str) -> Event {
    Event::PagerOp {
        id: id.into(),
        op: PagerOpKind::ResumeLoad,
        bytes: 1,
        duration_ms: ms,
        image_tier: tier.into(),
    }
}

fn suspend(id: &str, ms: u64) -> Event {
    Event::PagerOp {
        id: id.into(),
        op: PagerOpKind::SuspendSave,
        bytes: 1,
        duration_ms: ms,
        image_tier: "nvme".into(),
    }
}

fn started(id: &str) -> Event {
    Event::InferStarted {
        id: id.into(),
        prompt: "p".into(),
        prompt_sha256: "d".into(),
    }
}

#[test]
fn classifies_warm_and_cold_and_computes_p95() {
    let mut events = Vec::new();
    for i in 0..20 {
        events.push(evict("v", 10));
        events.push(resume("t", 100 + i, "ram")); // warm samples: 110..129 total ms
    }
    events.push(Event::ModelLoaded {
        model: "qwen".into(),
        duration_ms: 1200,
    });
    events.push(resume("t", 300, "nvme")); // one cold sample: 1500
    let r = compute_report(&events);
    assert_eq!(r.warm.n, 20);
    // ceil(0.95*20) - 1 = 19 - 1 = index 18 of the sorted sample 110..=129,
    // which is 128. (The task brief asserted 129 here while its own inline
    // comment said "index 18"; the *formula* is the pre-registered
    // commitment, so the literal was the transcription error and is corrected
    // here — see docs/superpowers/evidence/2026-08-14-g2-agent-switch.md.)
    assert_eq!(r.warm.p95_ms, 128);
    assert_eq!(r.cold.n, 1);
    assert_eq!(r.cold.p95_ms, 1500);
}

#[test]
fn empty_journal_reports_zero_n_not_zero_latency() {
    let r = compute_report(&[]);
    assert_eq!((r.warm.n, r.cold.n), (0, 0)); // n=0, no fake p95 (law 5)
    assert!(r.warm.p95_ms == 0 && r.warm.n == 0); // consumer must check n
}

/// The serialized artifact is what a later reader quotes, so the None-vs-zero
/// law is enforced *there*: an unmeasured class emits `null`, never a `0` that
/// reads as "switches were instantaneous".
#[test]
fn unmeasured_class_serializes_as_null_not_zero() {
    let json = report_json(&compute_report(&[]));
    assert!(json["warm"]["p50_ms"].is_null());
    assert!(json["warm"]["p95_ms"].is_null());
    assert_eq!(json["warm"]["n"], serde_json::json!(0));
    assert!(json["cold"]["p95_ms"].is_null());
}

#[test]
fn measured_class_serializes_numbers() {
    let events = vec![evict("v", 10), resume("t", 40, "ram")];
    let json = report_json(&compute_report(&events));
    assert_eq!(json["warm"]["n"], serde_json::json!(1));
    assert_eq!(json["warm"]["p50_ms"], serde_json::json!(50));
    assert_eq!(json["warm"]["p95_ms"], serde_json::json!(50));
}

/// The gate is judged at n=50, so the index that produces the number is pinned
/// at exactly that n: ceil(0.95*50) - 1 = 48 - 1 = 47, i.e. the 48th smallest
/// of 50 samples.
#[test]
fn p95_index_at_n_50_is_the_48th_smallest() {
    let mut events = Vec::new();
    for i in 0..50u64 {
        events.push(evict("v", 0));
        events.push(resume("t", i, "ram")); // samples 0..=49
    }
    let r = compute_report(&events);
    assert_eq!(r.warm.n, 50);
    assert_eq!(r.warm.p95_ms, 47);
    // ceil(0.50*50) - 1 = 25 - 1 = index 24
    assert_eq!(r.warm.p50_ms, 24);
}

/// A `ModelLoaded` that never reaches a resume (the very first cold start,
/// which has no image to restore) must not be carried across the intervening
/// request and charged to the *next* switch — that would both inflate that
/// sample by a whole weight load and misclassify a warm switch as cold. The
/// sequence is contiguous: any non-paging event ends it.
#[test]
fn an_orphan_model_load_does_not_leak_into_the_next_switch() {
    let events = vec![
        Event::ModelLoaded {
            model: "qwen".into(),
            duration_ms: 3000,
        },
        started("a1"), // first infer had no image: the sequence ends here
        evict("a1", 10),
        resume("a2", 40, "ram"),
    ];
    let r = compute_report(&events);
    assert_eq!(r.warm.n, 1);
    assert_eq!(r.warm.p95_ms, 50); // 10 + 40, not 3050
    assert_eq!(r.cold.n, 0);
}

/// A `SuspendSave` issued by a *separate* request (`POST /agents/{id}/suspend`,
/// or the page-out inside `POST /models/{m}/unload`) is not part of the pager-op
/// sequence that serves a later resume — a non-paging event always intervenes.
/// Pinned so the cold class cannot silently absorb a page-out it did not
/// measure, in either direction.
#[test]
fn a_suspend_from_another_request_is_not_part_of_the_next_switch() {
    let events = vec![
        suspend("a4", 90),
        Event::ModelUnloaded {
            model: "qwen".into(),
        },
        Event::ModelLoaded {
            model: "qwen".into(),
            duration_ms: 2000,
        },
        resume("a1", 200, "nvme"),
    ];
    let r = compute_report(&events);
    assert_eq!(r.cold.n, 1);
    assert_eq!(r.cold.p95_ms, 2200); // not 2290
    assert_eq!(r.warm.n, 0);
}

/// A `ModelLoaded` alone is cold even when the image came back from RAM: the
/// weights were not resident, which is the expensive half. This is not
/// hypothetical — it is exactly the first lap of the cold protocol, where the
/// images have not been spilled yet but every switch still reloads the
/// weights.
#[test]
fn a_model_load_is_cold_even_with_a_ram_image() {
    let events = vec![
        Event::ModelLoaded {
            model: "qwen".into(),
            duration_ms: 2500,
        },
        resume("t", 30, "ram"),
    ];
    let r = compute_report(&events);
    assert_eq!((r.warm.n, r.cold.n), (0, 1));
    assert_eq!(r.cold.p95_ms, 2530);
}

/// `image_tier == "nvme"` alone is cold even with the weights already
/// resident: the gate's cold class is "weights not resident **or** image on
/// NVMe", and a sample that satisfies either must never be counted as warm.
#[test]
fn an_nvme_image_is_cold_without_a_model_load() {
    let events = vec![evict("v", 5), resume("t", 300, "nvme")];
    let r = compute_report(&events);
    assert_eq!((r.warm.n, r.cold.n), (0, 1));
    assert_eq!(r.cold.p50_ms, 305);
}
