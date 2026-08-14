//! Gate G2's arithmetic: journal events in, warm/cold switch percentiles out.
//!
//! Pure by construction — no clock, no socket, no filesystem below the caller
//! that hands in the replayed events. Every millisecond here was measured by
//! the pager, inside the daemon, around the operation it names.
//!
//! # What one sample is (pre-registered, `docs/gates.md` G2)
//!
//! A **switch sample** is the sum of `duration_ms` over the contiguous
//! pager-op sequence that serves one resume: the `EvictSave` of the victim,
//! the `ResumeLoad` of the target, plus a `ModelLoaded` if the weights had to
//! come back too. "Contiguous" is load-bearing and is enforced here: any event
//! that is neither a `PagerOp` nor a `ModelLoaded` ends the sequence and
//! discards whatever had accumulated. Without that rule a `ModelLoaded` with
//! no image to restore after it (the very first cold start of a run) would be
//! carried across the intervening request and charged to the *next* switch,
//! inflating it by a whole weight load and flipping its class.
//!
//! # Which class it lands in
//!
//! * **warm** — the `ResumeLoad` read a RAM-tier image and no `ModelLoaded`
//!   appeared in the sequence (KV image in RAM, weights resident).
//! * **cold** — `image_tier == "nvme"` **or** a `ModelLoaded` was in the
//!   sequence (weights not resident, or the image came off NVMe).
//!
//! # The percentile index
//!
//! `p = sorted[ceil(p/100 * n) - 1]`, computed in integer arithmetic as
//! `(p*n + 99) / 100 - 1` so no float rounding can move the index that the
//! gate is read at. p50 uses the same convention. This formula is the
//! pre-registered commitment; changing it changes what the gate means.

use bloomery_core::journal::{Event, PagerOpKind};

/// One class's sample count and percentiles.
///
/// `n == 0` means *unmeasured*, and then `p50_ms`/`p95_ms` carry no
/// measurement at all — every consumer must check `n` first. [`report_json`]
/// is where that is made unmissable: an unmeasured class serializes as `null`
/// rather than as a `0` that reads like an instantaneous switch.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Stats {
    pub n: usize,
    pub p50_ms: u64,
    pub p95_ms: u64,
}

/// The gate reading. Warm and cold are always carried — and reported —
/// separately (G2's recorded obligation from the Phase 0 prior-art pass).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SwitchReport {
    pub warm: Stats,
    pub cold: Stats,
}

/// Classifies every switch in `events` and reduces each class to its
/// percentiles.
pub fn compute_report(events: &[Event]) -> SwitchReport {
    let mut warm: Vec<u64> = Vec::new();
    let mut cold: Vec<u64> = Vec::new();
    let mut seq = Sequence::default();

    for event in events {
        match event {
            Event::ModelLoaded { duration_ms, .. } => seq.model_loaded(*duration_ms),
            Event::PagerOp {
                op: PagerOpKind::ResumeLoad,
                duration_ms,
                image_tier,
                ..
            } => {
                let (total, is_cold) = seq.close(*duration_ms, image_tier);
                if is_cold {
                    cold.push(total);
                } else {
                    warm.push(total);
                }
            }
            Event::PagerOp { duration_ms, .. } => seq.saved(*duration_ms),
            // Anything else ends the sequence: the pager ops that serve one
            // resume are emitted back to back, so an intervening event means
            // whatever accumulated belonged to some other operation.
            _ => seq = Sequence::default(),
        }
    }

    SwitchReport {
        warm: summarize(warm),
        cold: summarize(cold),
    }
}

/// The pager ops seen since the last sequence boundary.
#[derive(Default)]
struct Sequence {
    accumulated_ms: u64,
    saw_model_load: bool,
}

impl Sequence {
    fn model_loaded(&mut self, duration_ms: u64) {
        self.accumulated_ms = self.accumulated_ms.saturating_add(duration_ms);
        self.saw_model_load = true;
    }

    fn saved(&mut self, duration_ms: u64) {
        self.accumulated_ms = self.accumulated_ms.saturating_add(duration_ms);
    }

    /// Ends the sequence on a `ResumeLoad`, returning `(total_ms, is_cold)`
    /// and resetting for the next one.
    fn close(&mut self, duration_ms: u64, image_tier: &str) -> (u64, bool) {
        let total = self.accumulated_ms.saturating_add(duration_ms);
        let is_cold = self.saw_model_load || image_tier == "nvme";
        *self = Sequence::default();
        (total, is_cold)
    }
}

fn summarize(mut samples: Vec<u64>) -> Stats {
    samples.sort_unstable();
    let n = samples.len();
    Stats {
        n,
        p50_ms: percentile_index(n, 50).map_or(0, |i| samples[i]),
        p95_ms: percentile_index(n, 95).map_or(0, |i| samples[i]),
    }
}

/// `ceil(pct/100 * n) - 1`, in integer arithmetic. `None` when there is
/// nothing to index into — an empty class has no percentile, not a zero one.
pub fn percentile_index(n: usize, pct: u64) -> Option<usize> {
    if n == 0 {
        return None;
    }
    let n = n as u64;
    let rank = pct.saturating_mul(n).saturating_add(99) / 100;
    Some(usize::try_from(rank.max(1) - 1).unwrap_or(0))
}

/// The gate artifact: `{"warm": {...}, "cold": {...}}`.
///
/// An unmeasured class (`n == 0`) emits `null` percentiles. A `0` there would
/// be exactly the bug class this project keeps naming — a value that looks
/// like a measurement and is not — and the gate is read off this document.
pub fn report_json(r: &SwitchReport) -> serde_json::Value {
    serde_json::json!({ "warm": stats_json(&r.warm), "cold": stats_json(&r.cold) })
}

fn stats_json(s: &Stats) -> serde_json::Value {
    let value = |v: u64| {
        if s.n == 0 {
            serde_json::Value::Null
        } else {
            serde_json::json!(v)
        }
    };
    serde_json::json!({
        "n": s.n,
        "p50_ms": value(s.p50_ms),
        "p95_ms": value(s.p95_ms),
    })
}
