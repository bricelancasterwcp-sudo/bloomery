//! Per-model tuning overrides (Task 3, spec §2-§4): the `n_gpu_layers`
//! override and the declared weights-VRAM ceiling that make qwen3.8:27b's
//! partial offload placeable. Split out of `pager.rs` for the same reason
//! `codec_gate.rs` is — `pager.rs` sits right under its 800-line house cap,
//! so new logic gets its own submodule rather than growing the file it
//! would otherwise share.
//!
//! **One value, everywhere (spec §3).** [`ModelEntry::effective_weights_bytes`]
//! is the single source of truth — `min(declared, meta.weights_bytes)`,
//! declared absent -> `meta.weights_bytes` — read at all four charge sites:
//! `loaded_weights_bytes` (`paging.rs`, the supply side of the reservation
//! budget), `place`'s demand term (`paging.rs`), `create_agent`'s
//! `GeometryInput.weights_bytes` (`pager.rs`), and the `/status` sum, which
//! reads `loaded_weights_bytes` and so inherits this for free. This is
//! exactly the shape Task 1 gave `ctx_overhead_bytes`: one field, one
//! method, every consumer reads through it, so a wiring gap at any one of
//! the four sites cannot silently let placement and geometry drift apart
//! again — the exact bug class carried-debt item 7 was.

use bloomery_substrate::Substrate;

use super::{ModelEntry, PagerError};

impl ModelEntry {
    /// The Task 3 accounting rule (spec §3): `min(declared, weights_bytes)`,
    /// declared absent -> the file's own measured `weights_bytes`. The ONE
    /// effective-weights number every charge site reads — see the module
    /// doc comment for the four call sites this must reach.
    pub(crate) fn effective_weights_bytes(&self) -> u64 {
        match self.weights_vram_bytes {
            Some(declared) => declared.min(self.meta.weights_bytes),
            None => self.meta.weights_bytes,
        }
    }

    /// `Some(file_bytes)` exactly when [`Self::effective_weights_bytes`]
    /// is reading out the DECLARED number (`declared <= weights_bytes` —
    /// the clamp did not have to act), paired with the file's own measured
    /// value for a refusal string's `file … B` clause.
    ///
    /// `None` when there is no override, or the clamp fell back to the
    /// file's own value (`declared > weights_bytes`): the printed number in
    /// that case IS the measured file value, and spec §3 is explicit that
    /// a declared number must never read as a measured one — so a measured
    /// one must equally never be mislabeled as declared here.
    pub(crate) fn declared_weights_label(&self) -> Option<u64> {
        self.weights_vram_bytes
            .filter(|&declared| declared <= self.meta.weights_bytes)
            .map(|_| self.meta.weights_bytes)
    }
}

impl<S: Substrate> crate::pager::Pager<S> {
    /// Sets `model`'s per-model tuning overrides (spec §2's `Tuned` config
    /// shape): an `n_gpu_layers` that wins over the pager-global default
    /// ([`Pager::set_n_gpu_layers`]) at `model_handle`'s `load_model` call,
    /// and a declared `weights_vram_bytes` ceiling that becomes the
    /// effective weights charge everywhere via
    /// [`ModelEntry::effective_weights_bytes`].
    ///
    /// `main.rs` is the only caller and converts `ModelSpec::weights_vram_mib`
    /// to bytes itself (`saturating_mul(1024 * 1024)`) before calling this —
    /// the pager speaks bytes only, exactly like every other VRAM setter on
    /// this type ([`Pager::set_overhead_bytes`], [`Pager::set_ctx_overhead_bytes`]).
    ///
    /// `None` for either argument means "no override": the pager-global
    /// `n_gpu_layers` default, and the file's full measured `weights_bytes`
    /// — today's behavior, unchanged. `UnknownModel` if `model` was never
    /// registered.
    pub fn set_model_tuning(
        &mut self,
        model: &str,
        n_gpu_layers: Option<u32>,
        weights_vram_bytes: Option<u64>,
    ) -> Result<(), PagerError> {
        let entry = self
            .models
            .get_mut(model)
            .ok_or_else(|| PagerError::UnknownModel(model.to_string()))?;
        entry.n_gpu_layers_override = n_gpu_layers;
        entry.weights_vram_bytes = weights_vram_bytes;
        Ok(())
    }
}
