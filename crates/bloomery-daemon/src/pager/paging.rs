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

use bloomery_core::journal::PagerOpKind;
use bloomery_core::scheduler::{plan_residency, Placement, ResidencyRequest};
use bloomery_substrate::{
    CtxHandle, ModelHandle, Substrate, SubstrateError, STATE_SIZE_MISMATCH, WINDOW_EXCEEDED,
};

use crate::agents::{AgentState, ImageFetch};

use super::error::{sub, substrate_msg};
use super::journal as jrnl;
use super::{Pager, PagerError};

impl<S: Substrate> Pager<S> {
    pub(super) fn resident_kv_bytes(&self) -> u64 {
        self.table
            .residents()
            .iter()
            .fold(0u64, |acc, r| acc.saturating_add(r.kv_bytes))
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
        Ok(ctx)
    }

    /// Pre-checks memory for `id` and evicts whatever the planner names.
    ///
    /// Free VRAM is `budget − Σ resident kv_bytes`, which is reservation
    /// accounting, not a second measurement: `free_vram` returns bloomery's
    /// *static* budget (see [`Pager::new`]), so the pager is the only thing
    /// tracking what it has spent out of that pool. A live driver read would
    /// already have the resident contexts subtracted from it and would make
    /// this double-count.
    ///
    /// When the budget is unmeasured there is no honest arithmetic left, so
    /// residency falls back to a count cap of one — planned as zero free
    /// bytes, which makes any second resident a priority decision or a
    /// refusal.
    ///
    /// Nothing in this function touches the substrate on the refusal path.
    /// That is the point: memory pressure is decided from measured numbers
    /// before allocation, never inferred from an allocation that blew up.
    fn place(&mut self, id: &str) -> Result<(), PagerError> {
        let (priority, kv_bytes, window_tokens) = {
            let a = self
                .table
                .get(id)
                .ok_or_else(|| PagerError::UnknownAgent(id.to_string()))?;
            (a.priority, a.kv_bytes, a.window.tokens)
        };
        let residents = self.table.residents();
        let resident_kv = self.resident_kv_bytes();
        let req = ResidencyRequest {
            id: id.to_string(),
            priority,
            kv_bytes,
        };
        let placement = match self.probe_free_vram()? {
            Some(free) => plan_residency(&residents, &req, free.saturating_sub(resident_kv)),
            None if residents.is_empty() => Placement::Fits,
            None => plan_residency(&residents, &req, 0),
        };

        match placement {
            Placement::Fits => jrnl::scheduler_decision(&mut self.journal, id, "fits", &[]),
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
                jrnl::scheduler_decision(&mut self.journal, id, "refuse", &[])?;
                // `Refusal` is token-shaped, so the residency arithmetic (in
                // bytes) rides in `detail` rather than being rounded into a
                // token field it doesn't belong in.
                jrnl::refusal(
                    &mut self.journal,
                    id,
                    u64::from(window_tokens),
                    window_tokens,
                    format!(
                        "residency: needed {needed} B, free {free} B, reclaimable {reclaimable} B"
                    ),
                )?;
                Err(PagerError::Refused {
                    needed,
                    free,
                    reclaimable,
                })
            }
        }
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
