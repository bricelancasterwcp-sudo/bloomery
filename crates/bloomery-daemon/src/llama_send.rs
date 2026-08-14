//! A `Send` wrapper around `LlamaSubstrate` for the daemon's HTTP worker
//! pool.
//!
//! `bloomery_substrate::llama::LlamaSubstrate` is not `Send` (`LlamaContext`
//! holds raw FFI pointers — `NonNull<llama_context>`, `*mut llama_sampler`
//! — which are conservatively `!Send`), but `http::serve` needs to move a
//! `Pager<S>` into an `Arc<Mutex<_>>` shared across worker threads. Rather
//! than assert `Send` on `LlamaSubstrate` itself — which would fold that
//! soundness claim into `bloomery-substrate`'s own contract, disconnected
//! from what actually discharges it — this newtype keeps the claim local to
//! the one place the obligation is met: right next to the `Mutex` that
//! guarantees exclusive access. `bloomery-substrate` stays honest:
//! `LlamaSubstrate` itself remains `!Send`.
//!
//! # Safety
//!
//! `unsafe impl Send for SendLlama` (below) is sound under all four of the
//! following, together:
//!
//! (a) **Exclusive access is structural, not assumed.** A `SendLlama` only
//!     ever lives inside `Arc<Mutex<Pager<SendLlama>>>` (`http::serve`);
//!     every `Substrate` method call on the wrapped `LlamaSubstrate` happens
//!     with the pager's mutex held, so two threads can never touch it
//!     concurrently. `Send` alone does not provide this — it only permits
//!     *ownership* to move between threads — so this property is what the
//!     rest of the argument leans on.
//! (b) **llama.cpp's C API has no documented thread-affinity or
//!     thread-local-storage requirement for sequential cross-thread use of
//!     a context** — create it on one thread, use it later from another,
//!     just never concurrently from two at once. This is exactly the
//!     pattern `llama-server` itself uses (a worker pool, contexts handed
//!     between threads, never touched by two at the same time).
//! (c) **`Send` only — `SendLlama` must never be `Sync`.** Nothing here
//!     claims it is safe to call methods from two threads *at once*.
//!     `Substrate`'s methods all take `&mut self`, and it is the `Mutex`
//!     that serializes them, not any property of `SendLlama`. Do **not**
//!     add `unsafe impl Sync for SendLlama` — that would be a materially
//!     different (and unjustified) claim.
//! (d) **Fallback if this is ever wrong.** If a driver or allocator
//!     thread-affinity fault surfaces in live testing (i.e. (b) turns out
//!     false on some GPU driver despite llama.cpp's own API contract being
//!     silent on it), the fix is not a smaller patch here — it's a
//!     different design: a dedicated pager thread owning the substrate for
//!     its entire lifetime, with worker threads sending requests over a
//!     channel and reading replies back (actor pattern) instead of locking
//!     a shared `Mutex` from arbitrary threads. Noted here so the next
//!     engineer who hits that doesn't have to rediscover it.
//!
//! Task 16's boot wiring and live smoke testing are what actually exercise
//! this against real hardware and a real driver; this crate's own gates
//! stay GPU-free, so this file is *compiled* (`cargo check --features
//! llama`, `cargo clippy --features llama`) but never *run* here.

use std::path::Path;

use bloomery_substrate::llama::LlamaSubstrate;
use bloomery_substrate::{CtxHandle, ModelHandle, Reply, Substrate, SubstrateError};

/// See the module doc for the full soundness argument behind this type's
/// `Send` impl.
pub struct SendLlama(LlamaSubstrate);

// SAFETY: see the module doc above, points (a)-(d). `Sync` is deliberately
// not implemented — see (c) — because nothing here justifies concurrent
// `&self` access, only single-owner-at-a-time cross-thread moves.
unsafe impl Send for SendLlama {}

impl SendLlama {
    /// Initializes the llama.cpp backend, wrapped for `Send`.
    ///
    /// # Errors
    ///
    /// See [`LlamaSubstrate::new`].
    pub fn new() -> Result<Self, SubstrateError> {
        LlamaSubstrate::new().map(SendLlama)
    }
}

impl Substrate for SendLlama {
    fn load_model(
        &mut self,
        path: &Path,
        n_gpu_layers: u32,
    ) -> Result<ModelHandle, SubstrateError> {
        self.0.load_model(path, n_gpu_layers)
    }

    fn unload_model(&mut self, m: ModelHandle) -> Result<(), SubstrateError> {
        self.0.unload_model(m)
    }

    fn create_context(&mut self, m: ModelHandle, n_ctx: u32) -> Result<CtxHandle, SubstrateError> {
        self.0.create_context(m, n_ctx)
    }

    fn destroy_context(&mut self, c: CtxHandle) -> Result<(), SubstrateError> {
        self.0.destroy_context(c)
    }

    fn infer(
        &mut self,
        c: CtxHandle,
        prompt: &str,
        max_tokens: u32,
    ) -> Result<Reply, SubstrateError> {
        self.0.infer(c, prompt, max_tokens)
    }

    fn save_state(&mut self, c: CtxHandle) -> Result<Vec<u8>, SubstrateError> {
        self.0.save_state(c)
    }

    fn load_state(&mut self, c: CtxHandle, bytes: &[u8]) -> Result<(), SubstrateError> {
        self.0.load_state(c, bytes)
    }
}
