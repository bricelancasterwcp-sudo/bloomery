//! The paging mechanics behind [`Pager`]'s public surface: placement,
//! eviction, image save/restore, and on-demand weight loading.
//!
//! Every entry point here is `&mut self` on the one pager instance, so the
//! whole file is a single-threaded state machine over three resources —
//! substrate contexts, the image store, and free VRAM — and it is written so
//! that no failure path can leave two of them disagreeing: a context is only
//! recorded as gone once the substrate says it is gone, and an image is only
//! trusted once the substrate says it loaded.

use std::time::Instant;

use bloomery_core::journal::{AgentId, PagerOpKind};
use bloomery_core::scheduler::{plan_residency, Placement, ResidencyRequest, Resident};
use bloomery_substrate::{
    CtxHandle, ModelHandle, Substrate, SubstrateError, STATE_SIZE_MISMATCH, WINDOW_EXCEEDED,
};

use crate::agents::{AgentState, ImageFetch};

use super::error::{sub, substrate_msg};
use super::journal as jrnl;
use super::{Pager, PagerError};

impl<S: Substrate> Pager<S> {
    /// Sum of what every resident context **reserves** — KV plus the
    /// per-context runtime overhead (`Agent::reserved_bytes`), because
    /// `AgentTable::residents` hands the planner the reserved figure. Named
    /// for the budget term it is, not for the KV half of it; `/status`
    /// reports it under `resident_kv_bytes` for wire compatibility and says
    /// there what it contains.
    pub(super) fn resident_reserved_bytes(&self) -> u64 {
        self.table
            .residents()
            .iter()
            .fold(0u64, |acc, r| acc.saturating_add(r.kv_bytes))
    }

    /// Sum of `weights_bytes` over every model whose handle is currently
    /// loaded — the weights term of the reservation budget (Task 3, see
    /// `Pager::place`'s doc comment for the accounting rule).
    ///
    /// Derived from the loaded set on every call rather than tracked as a
    /// running counter: `unload_model` dropping a handle is then the whole
    /// story for crediting its bytes back, with no separate decrement to
    /// forget.
    pub(super) fn loaded_weights_bytes(&self) -> u64 {
        self.models
            .values()
            .filter(|m| m.handle.is_some())
            .fold(0u64, |acc, m| acc.saturating_add(m.meta.weights_bytes))
    }

    /// Reads bloomery's static VRAM budget (see [`Pager::new`]). `None` is
    /// unmeasured, never zero — and it is said once, so a machine with no
    /// working probe doesn't drown its own journal in the same sentence.
    pub(super) fn probe_free_vram(&mut self) -> Result<Option<u64>, PagerError> {
        let free = (self.free_vram)();
        if free.is_none() && !self.vram_unmeasured_logged {
            self.vram_unmeasured_logged = true;
            jrnl::degraded(
                &mut self.journal,
                "vram unmeasured; residency capped at 1 agent".to_string(),
            )?;
        }
        Ok(free)
    }

    /// Makes `id` resident, paging it in if it isn't already.
    pub(super) fn ensure_resident(&mut self, id: &str) -> Result<CtxHandle, PagerError> {
        match self.table.get(id) {
            None => return Err(PagerError::UnknownAgent(id.to_string())),
            Some(a) => {
                if let AgentState::Resident { ctx } = a.state {
                    return Ok(ctx);
                }
            }
        }
        self.place(id)?;
        let ctx = self.open_context(id)?;
        self.table
            .get_mut(id)
            .expect("agent existence checked at entry")
            .state = AgentState::Resident { ctx };
        // The first of `last_use`'s two write points (see the field doc on
        // `Pager`): the transition to `Resident` itself, covering `resume`
        // (which never calls `infer` at all) as well as an ordinary
        // placement-before-infer. Without this, a just-resumed agent would
        // read back as `unwrap_or(0)` in `try_time_share` — indistinguishable
        // from an agent nobody has touched in ages — and be the first thing
        // evicted the moment any equal-priority contest reached the quantum.
        self.last_use.insert(id.to_string(), (self.clock)());
        Ok(ctx)
    }

    /// Pre-checks memory for `id` and evicts whatever the planner names.
    ///
    /// **The reservation-budget accounting rule (Task 3).** Free VRAM is
    /// `budget − Σ loaded_models.weights_bytes − Σ resident kv_bytes`, all
    /// `saturating_sub`. This is still reservation accounting, not a second
    /// measurement: `free_vram` returns bloomery's *static* budget (see
    /// [`Pager::new`]), so the pager is the only thing tracking what it has
    /// spent out of that pool — weights now spend from the same pool KV
    /// does, because both sit in the VRAM the static budget promised. A live
    /// driver read would already have both the resident contexts *and* the
    /// loaded weights subtracted from it and would make this double-count.
    ///
    /// When `id`'s model is not yet loaded, loading it is part of satisfying
    /// this request, so its `weights_bytes` is added to the *demand* side
    /// (alongside `id`'s own `kv_bytes`) before planning — the planner then
    /// evicts or refuses against the true cost of admitting this agent, not
    /// just its KV footprint. Once a model is loaded its weights stay
    /// charged against `loaded_weights_bytes` until an explicit
    /// [`Pager::unload_model`] call credits them back; there is **no**
    /// automatic eviction of weights in 2a — only KV contexts are evicted.
    ///
    /// When the budget is unmeasured there is no honest arithmetic left, so
    /// residency falls back to a count cap of one — planned as zero free
    /// bytes, which makes any second resident a priority decision or a
    /// refusal. One consequence worth naming: with any context already
    /// resident and the requested agent's model cold, demand (reserved bytes
    /// plus weights bytes) can never be covered under that flat zero-byte
    /// cap — such a cross-model request refuses regardless of priority,
    /// every time, for as long as the budget stays unmeasured. That is a
    /// deliberate, safe narrowing versus Phase 1's cap-of-1 swap behavior,
    /// not a regression.
    ///
    /// Nothing in this function touches the substrate on the refusal path.
    /// That is the point: memory pressure is decided from measured numbers
    /// before allocation, never inferred from an allocation that blew up —
    /// in particular, a weights-refusal never calls `load_model`.
    ///
    /// **The equal-priority tiebreak (Task 4).** A refusal the frozen
    /// planner hands back is not always final: [`Pager::try_time_share`]
    /// gets first look at it, and when every resident in the way is idle
    /// and exactly `id`'s own priority, a refusal that has already waited
    /// out the time-sharing quantum turns into an eviction of the LRU one
    /// instead — see that function's doc comment for the full rule and the
    /// determinism argument.
    fn place(&mut self, id: &str) -> Result<(), PagerError> {
        let (priority, kv_bytes, reserved_bytes, window_tokens, model) = {
            let a = self
                .table
                .get(id)
                .ok_or_else(|| PagerError::UnknownAgent(id.to_string()))?;
            (
                a.priority,
                a.kv_bytes,
                a.reserved_bytes,
                a.window.tokens,
                a.model.clone(),
            )
        };
        let (model_loaded, weights_bytes) = {
            let entry = self
                .models
                .get(&model)
                .ok_or_else(|| PagerError::UnknownModel(model.clone()))?;
            (entry.handle.is_some(), entry.meta.weights_bytes)
        };
        // Loading is part of satisfying this request when the model is
        // cold: the demand side carries its weights alongside this agent's
        // own KV footprint. An already-loaded model contributes nothing
        // extra here — its weights are already charged via
        // `loaded_weights_bytes` on the supply side below.
        let weights_term = if model_loaded { 0 } else { weights_bytes };
        // The demand side carries what this placement will actually hold:
        // the whole per-context reservation, not the KV half of it.
        let demand = reserved_bytes.saturating_add(weights_term);

        let residents = self.table.residents();
        let resident_reserved = self.resident_reserved_bytes();
        let loaded_weights = self.loaded_weights_bytes();
        let req = ResidencyRequest {
            id: id.to_string(),
            priority,
            kv_bytes: demand,
        };
        let budget = self.probe_free_vram()?;
        // Shared with `try_time_share` below: the `None` (unmeasured)
        // fallback plans against a flat `0` free bytes (the residency-
        // count-cap-of-one), so `avail` is `0` there too — same number,
        // computed once, rather than a second `0` literal that could drift.
        let overhead = self.overhead_bytes;
        let avail = match budget {
            Some(budget) => budget
                .saturating_sub(overhead)
                .saturating_sub(loaded_weights)
                .saturating_sub(resident_reserved),
            None => 0,
        };
        let placement = match budget {
            Some(_) => plan_residency(&residents, &req, avail),
            None if residents.is_empty() => Placement::Fits,
            None => plan_residency(&residents, &req, avail),
        };

        let outcome = match placement {
            Placement::Fits => {
                jrnl::scheduler_decision(&mut self.journal, id, "fits", &[])?;
                Ok(())
            }
            Placement::Evict(victims) => {
                jrnl::scheduler_decision(&mut self.journal, id, "evict", &victims)?;
                for victim in &victims {
                    self.evict(victim)?;
                }
                Ok(())
            }
            Placement::Refuse {
                needed,
                free,
                reclaimable,
            } => {
                if let Some((victim, waited_ms)) =
                    self.try_time_share(id, priority, &residents, demand, avail)
                {
                    jrnl::scheduler_decision(
                        &mut self.journal,
                        id,
                        &format!("evict_timeshare(waited_{waited_ms}ms)"),
                        std::slice::from_ref(&victim),
                    )?;
                    self.evict(&victim)?;
                    Ok(())
                } else {
                    jrnl::scheduler_decision(&mut self.journal, id, "refuse", &[])?;
                    // `Refusal` is token-shaped, so the residency arithmetic
                    // (in bytes) rides in `detail` rather than being rounded
                    // into a token field it doesn't belong in. The
                    // weights/kv/budget breakdown is spelled out explicitly
                    // (law 2: the arithmetic, printed) rather than left
                    // implicit in `needed`/`free`/`reclaimable` alone, which
                    // cannot show *why* — a reader should not have to
                    // re-derive that a cold model's weights were the reason
                    // this refused.
                    //
                    // `budget` is `None`-vs-zero honest (law 5): an
                    // unmeasured budget never fabricates a `budget 0 B` term
                    // or a subtraction this branch never actually performed
                    // (`avail` was planned as the residency-count-cap-of-one's
                    // flat `0`, not derived from `budget − loaded −
                    // resident`) — it says plainly that the budget is
                    // unmeasured instead.
                    let detail = match budget {
                        Some(budget) => format!(
                            "residency: weights {weights_term} B + reserved {reserved_bytes} B \
                             (kv {kv_bytes} B + ctx overhead {ctx_overhead} B) vs budget \
                             {budget} B − overhead {overhead} B − loaded {loaded_weights} B − \
                             resident {resident_reserved} B (needed {needed} B, free {free} B, \
                             reclaimable {reclaimable} B)",
                            ctx_overhead = reserved_bytes.saturating_sub(kv_bytes)
                        ),
                        None => format!(
                            "residency: budget unmeasured (residency capped at 1 agent); \
                             needed {needed} B, reclaimable {reclaimable} B"
                        ),
                    };
                    jrnl::refusal(
                        &mut self.journal,
                        id,
                        u64::from(window_tokens),
                        window_tokens,
                        detail,
                    )?;
                    Err(PagerError::Refused {
                        needed,
                        free,
                        reclaimable,
                    })
                }
            }
        };
        // Task 4: any placement that actually succeeds — whether by fitting
        // outright, the frozen planner's own eviction, or this file's
        // time-sharing tiebreak — clears `id`'s waiting mark. Without this,
        // a mark from a long-past, unrelated stand-off would sit in the map
        // forever and make some future, otherwise-ordinary refusal look
        // like it had already waited a full quantum the instant it started
        // (pinned by `pager_timeshare_test.rs::successful_placement_clears_the_waiting_tracker`).
        //
        // This clears on `place`'s own success alone — it has no visibility
        // into whether `ensure_resident`'s next step, `open_context`, then
        // goes on to fail. A failure there leaves the mark already cleared,
        // so a retry starts a brand-new quantum from zero rather than
        // resuming any elapsed wait. That is the conservative direction (an
        // extra wait in a rare failure case, never a spurious eviction from
        // stale elapsed time), so it is left as is rather than threaded
        // through `open_context`'s own error paths.
        if outcome.is_ok() {
            self.waiting_since.remove(id);
        }
        outcome
    }

    /// The equal-priority LRU time-sharing tiebreak (Phase 2a spec §2 item
    /// 4, Task 4).
    ///
    /// **The spec rule.** `plan_residency` (frozen) only ever evicts a
    /// resident whose priority is *strictly less* than the requester's —
    /// two equal-priority agents contending for the same room is therefore
    /// a refusal the planner alone can never resolve, no matter how long
    /// either one waits. This helper is the pager's own layer on top: when
    /// a refusal is *purely* equal-priority (every OTHER resident — `id`
    /// itself is filtered out of the candidate set, structurally impossible
    /// though self-eviction already is here — is idle *and* exactly the
    /// request's own priority; a single higher-priority or busy resident
    /// anywhere in the mix disqualifies it, and it stays a plain refusal
    /// forever), the first such refusal for `id` starts a clock
    /// (`waiting_since`); once a later attempt has waited at least
    /// `time_share_quantum_ms`, this picks the least-recently-used
    /// equal-priority resident as the *candidate* victim — last-use being
    /// the clock reading at that resident's most recent "use" (`last_use`;
    /// see the field doc on [`Pager`] for its two write points — a
    /// just-resumed, never-inferred resident is NOT the oldest thing on the
    /// table), ties broken by lexical id for a result that never depends on
    /// `AgentTable`'s `HashMap`-derived iteration order.
    ///
    /// **Sufficiency, not just eligibility.** Naming a victim is not enough
    /// on its own: this only returns `Some` when evicting that *specific*
    /// candidate would actually cover the request (`avail + victim.kv_bytes
    /// >= demand`). A cold model's weights are part of `demand` but are
    /// never freed by evicting a KV context, and an unequal `window_cap`
    /// can make the LRU resident the *smallest* one on the table — so the
    /// LRU pick can legitimately be too small to help. When it is, this
    /// returns `None` rather than either (a) evicting anyway and
    /// overcommitting the budget, or (b) silently substituting some other,
    /// larger resident in its place — the spec names "the" LRU resident,
    /// singular, so an insufficient LRU pick is a plain refusal, not a
    /// license to pick a different one. `waiting_since` is left untouched
    /// on this path (already recorded by an earlier qualifying refusal, or
    /// about to be by `place`'s own refusal branch) — the wait keeps
    /// accumulating toward a quantum that, on its own, can never make the
    /// LRU resident's `kv_bytes` any larger.
    ///
    /// This is a *single*-victim tiebreak, not `plan_residency`'s iterative
    /// accumulation, by the same "the LRU resident" reading.
    ///
    /// **Determinism.** Every timestamp this function reads comes from
    /// `self.clock` (see [`ClockFn`](super::ClockFn)) — never a bare
    /// `Instant::now()` — and `waiting_since`/`last_use` are plain,
    /// deterministically-updated maps. So the same sequence of clock
    /// readings, fed through the same sequence of calls, always produces
    /// the same tiebreak decision: replayable in tests with a fake clock,
    /// and never dependent on real wall-clock jitter or `HashMap` iteration
    /// order in production.
    fn try_time_share(
        &mut self,
        id: &str,
        req_priority: u8,
        residents: &[Resident],
        demand: u64,
        avail: u64,
    ) -> Option<(AgentId, u64)> {
        // `id` can never actually appear in `residents` today (`place` is
        // only reached from `ensure_resident` when `id` is NOT already
        // `Resident`), but filtering it out of the candidate set makes that
        // impossibility structural rather than an invariant a future
        // caller has to remember to preserve.
        let candidates: Vec<&Resident> = residents.iter().filter(|r| r.id != id).collect();
        let qualifies = !candidates.is_empty()
            && candidates
                .iter()
                .all(|r| !r.busy && r.priority == req_priority);
        if !qualifies {
            return None;
        }
        let now = (self.clock)();
        let started = *self.waiting_since.entry(id.to_string()).or_insert(now);
        let waited_ms = now.saturating_sub(started);
        if waited_ms < self.time_share_quantum_ms {
            return None;
        }
        let victim = candidates.into_iter().min_by(|a, b| {
            let a_use = self.last_use.get(&a.id).copied().unwrap_or(0);
            let b_use = self.last_use.get(&b.id).copied().unwrap_or(0);
            a_use.cmp(&b_use).then_with(|| a.id.cmp(&b.id))
        })?;
        if avail.saturating_add(victim.kv_bytes) < demand {
            // The LRU pick can't actually cover the request (a cold
            // model's weights on the demand side, or a smaller
            // `window_cap` on the victim's side). Refuse rather than
            // overcommit or substitute a different, larger resident —
            // `waiting_since` stays exactly where it was.
            return None;
        }
        Some((victim.id.clone(), waited_ms))
    }

    /// Evicts a victim: its image goes to RAM (cheapest to page back in),
    /// then its context is dropped.
    fn evict(&mut self, victim: &str) -> Result<(), PagerError> {
        self.save_image(victim, PagerOpKind::EvictSave, false)?;
        self.destroy_context(victim)
    }

    /// Snapshots a resident agent's KV image into the store.
    ///
    /// A failed `save_state` aborts rather than dropping the context anyway:
    /// losing a conversation to make room is worse than failing to make room.
    /// A failed *spill* keeps the image in RAM (the store guarantees that)
    /// and journals both the degradation and the tier the bytes actually
    /// reached — the journal must not claim an NVMe image that isn't there.
    pub(super) fn save_image(
        &mut self,
        id: &str,
        op: PagerOpKind,
        spill: bool,
    ) -> Result<(), PagerError> {
        let (ctx, model) = match self.table.get(id) {
            None => return Err(PagerError::UnknownAgent(id.to_string())),
            Some(a) => match a.state {
                AgentState::Resident { ctx } => (ctx, a.model.clone()),
                _ => return Ok(()),
            },
        };
        let digest = self
            .models
            .get(&model)
            .map(|m| m.digest.clone())
            .ok_or(PagerError::UnknownModel(model))?;

        let started = Instant::now();
        let bytes = match self.substrate.save_state(ctx) {
            Ok(bytes) => bytes,
            Err(e) => {
                // The `SchedulerDecision` that named this victim is already
                // in the journal. Without this record, a replay shows an
                // eviction that was decided and then simply never happened —
                // an orphaned decision the reader has to guess about.
                let detail = substrate_msg(&e);
                let what = match op {
                    PagerOpKind::EvictSave => "eviction",
                    PagerOpKind::SuspendSave => "suspend",
                    PagerOpKind::ResumeLoad => "resume",
                };
                jrnl::degraded(
                    &mut self.journal,
                    format!("{what} of {id} aborted: save_state failed: {detail}"),
                )?;
                return Err(PagerError::Substrate(format!(
                    "save_state for {id}: {detail}"
                )));
            }
        };
        let len = bytes.len() as u64;
        self.images.put_ram(id, &digest, bytes);
        let mut tier = "ram";
        if spill {
            match self.images.spill(id) {
                Ok(()) => tier = "nvme",
                Err(e) => jrnl::degraded(
                    &mut self.journal,
                    format!("spill failed for {id}: {e}; image retained in RAM"),
                )?,
            }
        }
        jrnl::pager_op(&mut self.journal, id, op, len, started.elapsed(), tier)
    }

    /// Drops `id`'s context, marking it suspended only once the substrate has
    /// confirmed the context is gone.
    pub(super) fn destroy_context(&mut self, id: &str) -> Result<(), PagerError> {
        let ctx = match self.table.get(id) {
            None => return Err(PagerError::UnknownAgent(id.to_string())),
            Some(a) => match a.state {
                AgentState::Resident { ctx } => ctx,
                _ => return Ok(()),
            },
        };
        self.substrate.destroy_context(ctx).map_err(sub)?;
        self.table
            .get_mut(id)
            .expect("agent existence checked above")
            .state = AgentState::Suspended;
        Ok(())
    }

    /// Creates `id`'s context and restores its image if one is still valid.
    fn open_context(&mut self, id: &str) -> Result<CtxHandle, PagerError> {
        let (model, n_ctx) = self.agent_model_and_window(id)?;
        let handle = self.model_handle(&model)?;
        let digest = self
            .models
            .get(&model)
            .map(|m| m.digest.clone())
            .ok_or(PagerError::UnknownModel(model))?;
        let ctx = self.substrate.create_context(handle, n_ctx).map_err(sub)?;

        match self.images.take(id, &digest) {
            ImageFetch::Missing => Ok(ctx),
            ImageFetch::StaleDigest => {
                jrnl::degraded(
                    &mut self.journal,
                    format!("stale image digest for {id}, cold start"),
                )?;
                Ok(ctx)
            }
            ImageFetch::Corrupt => {
                jrnl::degraded(
                    &mut self.journal,
                    format!("corrupt image for {id} (spilled length mismatch), cold start"),
                )?;
                Ok(ctx)
            }
            ImageFetch::Ram(bytes) => self.restore_image(id, ctx, bytes, "ram", &digest),
            ImageFetch::Nvme(bytes) => self.restore_image(id, ctx, bytes, "nvme", &digest),
        }
    }

    /// Classifies a failure returned by `substrate.infer`.
    ///
    /// The substrate is law 2's backstop: it knows the real tokenization and
    /// the real (post-padding) window, so it catches what the kernel's
    /// pre-tokenization estimate let through. That is still a *refusal*, and
    /// it has to stay one on this side of the boundary — an operator who
    /// sees `Substrate` goes looking for a broken backend, when the honest
    /// answer is "the prompt did not fit".
    ///
    /// Field provenance: `needed_tokens` is the **pager's** conservative
    /// estimate (`prompt.len()/3 + max_tokens`) and `window_tokens` is the
    /// window the pager computed and asked for — deliberately *not* the
    /// substrate's exact token count or its padded window, which it reports
    /// only inside its message. That message is journaled verbatim in the
    /// `Refusal`'s `detail`, so the exact numbers are recoverable while the
    /// typed fields stay in the same units the caller was quoted at
    /// `create_agent` time.
    ///
    /// The outer `Err` is reserved for a failed journal write; the classified
    /// refusal comes back as `Ok`.
    pub(super) fn classify_infer_error(
        &mut self,
        id: &str,
        e: SubstrateError,
        needed_tokens: u64,
        window_tokens: u32,
    ) -> Result<PagerError, PagerError> {
        let detail = substrate_msg(&e);
        if !detail.contains(WINDOW_EXCEEDED) {
            return Ok(PagerError::Substrate(detail));
        }
        jrnl::refusal(
            &mut self.journal,
            id,
            needed_tokens,
            window_tokens,
            format!("substrate backstop refused the real tokenization: {detail}"),
        )?;
        Ok(PagerError::PromptTooLarge {
            needed_tokens,
            window_tokens,
        })
    }

    /// Restores an image into a just-created context.
    ///
    /// A failed restore leaves the destination context partially written, so
    /// it is destroyed rather than retried in place. A rejection carrying
    /// [`STATE_SIZE_MISMATCH`] means the image no longer fits this geometry:
    /// that is invalidation, handled exactly like a stale digest — journal
    /// the degradation and cold-start on a fresh context. Any other failure
    /// is a real substrate fault and is surfaced.
    ///
    /// `ImageStore::take` is destructive, so anything short of invalidation
    /// puts the bytes **back** before the error propagates. Otherwise a
    /// transient fault (a substrate hiccup, a context that couldn't be
    /// created that instant) would silently consume the only copy of a
    /// conversation: the request fails, the operator retries, and the agent
    /// comes back cold with no record that anything was lost. An invalidated
    /// image is the one case that must *not* go back — it would fail
    /// identically forever.
    fn restore_image(
        &mut self,
        id: &str,
        ctx: CtxHandle,
        bytes: Vec<u8>,
        tier: &str,
        digest: &str,
    ) -> Result<CtxHandle, PagerError> {
        let started = Instant::now();
        let len = bytes.len() as u64;
        let failure = match self.substrate.load_state(ctx, &bytes) {
            Ok(()) => {
                jrnl::pager_op(
                    &mut self.journal,
                    id,
                    PagerOpKind::ResumeLoad,
                    len,
                    started.elapsed(),
                    tier,
                )?;
                return Ok(ctx);
            }
            Err(e) => substrate_msg(&e),
        };

        if !failure.contains(STATE_SIZE_MISMATCH) {
            self.images.put_ram(id, digest, bytes);
        }

        if let Err(destroy) = self.substrate.destroy_context(ctx) {
            let detail = substrate_msg(&destroy);
            jrnl::degraded(
                &mut self.journal,
                format!(
                    "context for {id} left partial after a failed restore, and destroying it \
                     also failed: {detail}"
                ),
            )?;
            return Err(PagerError::Substrate(format!(
                "restore failed ({failure}) and the partial context could not be destroyed \
                 ({detail})"
            )));
        }
        if !failure.contains(STATE_SIZE_MISMATCH) {
            jrnl::degraded(
                &mut self.journal,
                format!(
                    "image restore failed for {id}: {failure}; context destroyed, \
                     image kept for retry"
                ),
            )?;
            return Err(PagerError::Substrate(failure));
        }
        jrnl::degraded(
            &mut self.journal,
            format!("image for {id} invalidated ({failure}), cold start"),
        )?;
        let (model, n_ctx) = self.agent_model_and_window(id)?;
        let handle = self.model_handle(&model)?;
        self.substrate.create_context(handle, n_ctx).map_err(sub)
    }

    fn agent_model_and_window(&self, id: &str) -> Result<(String, u32), PagerError> {
        let a = self
            .table
            .get(id)
            .ok_or_else(|| PagerError::UnknownAgent(id.to_string()))?;
        Ok((a.model.clone(), a.window.tokens))
    }

    /// The model's substrate handle, loading its weights on demand and
    /// journaling how long that took (the cold-switch bench reads this).
    pub(super) fn model_handle(&mut self, model: &str) -> Result<ModelHandle, PagerError> {
        let entry = self
            .models
            .get(model)
            .ok_or_else(|| PagerError::UnknownModel(model.to_string()))?;
        if let Some(handle) = entry.handle {
            return Ok(handle);
        }
        let path = entry.path.clone();
        let started = Instant::now();
        let handle = self
            .substrate
            .load_model(&path, self.n_gpu_layers)
            .map_err(sub)?;
        jrnl::model_loaded(&mut self.journal, model, started.elapsed())?;
        if let Some(entry) = self.models.get_mut(model) {
            entry.handle = Some(handle);
        }
        Ok(handle)
    }
}
