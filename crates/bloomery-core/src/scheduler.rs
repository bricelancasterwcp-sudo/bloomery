//! Deterministic residency planner (mechanism half of law 8).
//!
//! `plan_residency` is a pure function: no I/O, no clocks, no randomness.
//! Given the current residents, an incoming request, and the free VRAM
//! available, it decides whether the request fits as-is, requires evicting
//! idle lower-priority residents, or must be refused outright. LLM-driven
//! policy (which agents *should* be prioritized) is out of scope for Phase 1
//! and belongs to the pager (Task 13) and later phases — this module only
//! computes the arithmetic and ordering mandated by the eviction rules.

use crate::journal::AgentId;

#[derive(Debug, Clone)]
pub struct Resident {
    pub id: AgentId,
    pub priority: u8,
    pub kv_bytes: u64,
    pub busy: bool,
}

#[derive(Debug, Clone)]
pub struct ResidencyRequest {
    pub id: AgentId,
    pub priority: u8,
    pub kv_bytes: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Placement {
    Fits,
    /// lowest-priority-first victim order
    Evict(Vec<AgentId>),
    /// the arithmetic, law 2
    Refuse {
        needed: u64,
        free: u64,
        reclaimable: u64,
    },
}

/// Plan how to satisfy `req.kv_bytes` given `residents` and `free_vram_bytes`.
///
/// Rules (binding, see task brief):
/// - Fits in free VRAM as-is -> `Fits`.
/// - Otherwise, evict idle residents with strictly lower priority than the
///   request, lowest priority first (ties: larger `kv_bytes` first, then
///   lexical `id`), accumulating victims until freed bytes + free VRAM
///   covers `req.kv_bytes`.
/// - Busy residents and residents with priority >= the request's priority
///   are never evicted.
/// - If even evicting every eligible resident cannot cover the request,
///   refuse with the arithmetic: `needed` (requested bytes), `free`
///   (current free VRAM), and `reclaimable` (total bytes evictable from
///   idle, strictly-lower-priority residents).
pub fn plan_residency(
    residents: &[Resident],
    req: &ResidencyRequest,
    free_vram_bytes: u64,
) -> Placement {
    if req.kv_bytes <= free_vram_bytes {
        return Placement::Fits;
    }

    let mut evictable: Vec<&Resident> = residents
        .iter()
        .filter(|r| !r.busy && r.priority < req.priority)
        .collect();
    evictable.sort_by(|a, b| {
        a.priority
            .cmp(&b.priority)
            .then_with(|| b.kv_bytes.cmp(&a.kv_bytes))
            .then_with(|| a.id.cmp(&b.id))
    });

    let mut freed: u64 = 0;
    let mut victims: Vec<AgentId> = Vec::new();
    for resident in evictable.iter() {
        if freed.saturating_add(free_vram_bytes) >= req.kv_bytes {
            break;
        }
        freed = freed.saturating_add(resident.kv_bytes);
        victims.push(resident.id.clone());
    }

    if freed.saturating_add(free_vram_bytes) >= req.kv_bytes {
        return Placement::Evict(victims);
    }

    let reclaimable: u64 = evictable
        .iter()
        .fold(0u64, |acc, r| acc.saturating_add(r.kv_bytes));
    Placement::Refuse {
        needed: req.kv_bytes,
        free: free_vram_bytes,
        reclaimable,
    }
}
