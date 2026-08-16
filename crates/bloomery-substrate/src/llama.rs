//! The real inference backend: llama.cpp via `llama-cpp-2` 0.1.154.
//!
//! Compiled only under the `llama` feature so the rest of the workspace —
//! and its whole test suite — stays GPU-free and toolchain-free. Add the
//! `vulkan` feature on top to offload to the GPU.
//!
//! Three things here are load-bearing and deliberately not obvious:
//!
//! **Token counts are real by construction.** llama.cpp hands back no usage
//! struct, so there is no upstream field that could lie to us: `prompt_tokens`
//! is the length of the vector `str_to_token` returned, and `completion_tokens`
//! is the number of tokens this loop itself sampled and decoded. No path in
//! this module can build a [`Reply`] with `None` stats — the honesty required
//! by project law 4 is structural here, not a trusted upstream value.
//!
//! **Ownership.** `LlamaContext<'m>` genuinely borrows its `LlamaModel`, so
//! contexts cannot live in a map beside the models they borrow. Each model is
//! an arena ([`ModelCell`]) that owns the model *and* the contexts created
//! from it; only integer handles escape. Dropping the arena drops the
//! contexts before the model, which is also the order llama.cpp needs to free
//! VRAM. `Box::leak` is prohibited — a pager that leaks models can never give
//! VRAM back.
//!
//! **State.** Save/restore uses the *per-sequence* `state_seq_*_ext` calls, on
//! `seq_id` 0. The deprecated whole-context trio (`get_state_size` /
//! `copy_state_data` / `set_state_data`) is prohibited by the D1 decision
//! record and is not used: `set_state_data` drops `src.len()` on the floor, so
//! a truncated image would be an out-of-bounds read inside C rather than an
//! error. Both calls below are size-bounded — see [`LlamaSubstrate::save_state`]
//! and [`LlamaSubstrate::load_state`] for the exact invariants and for why the
//! safe `state_seq_get`/`state_seq_set` pair cannot be used across a
//! `Vec<u8>`-shaped trait boundary.

use std::collections::HashMap;
use std::num::NonZeroU32;
use std::path::Path;
use std::time::Instant;

use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::context::LlamaContext;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::{AddBos, LlamaModel};
use llama_cpp_2::sampling::LlamaSampler;
use llama_cpp_2::token::LlamaToken;
use llama_cpp_2::{LlamaStateSeqFlags, TokenToStringError};

use crate::{CtxHandle, ModelHandle, Reply, Substrate, SubstrateError, STATE_SIZE_MISMATCH};

/// The single sequence every bloomery context uses. Phase 1 gives each agent
/// its own context, so per-context state is exactly sequence 0's state.
const SEQ_ID: i32 = 0;

/// Flags for every state call. `empty()` on purpose: `ON_DEVICE` would keep
/// the bytes in GPU buffers where the host cannot read them, which is the
/// opposite of what a VRAM→RAM→NVMe pager needs.
fn state_flags() -> LlamaStateSeqFlags {
    LlamaStateSeqFlags::empty()
}

/// Contexts created from one model, keyed by handle.
type CtxMap<'m> = HashMap<CtxHandle, LlamaContext<'m>>;

self_cell::self_cell!(
    /// A loaded model plus every context borrowed from it.
    ///
    /// `self_cell` gives us the self-referential arena without a line of
    /// `unsafe` in this crate and with a guaranteed drop order: contexts
    /// first, then the model.
    struct ModelCell {
        owner: LlamaModel,
        #[covariant]
        dependent: CtxMap,
    }
);

/// llama.cpp-backed [`Substrate`].
pub struct LlamaSubstrate {
    backend: LlamaBackend,
    models: HashMap<ModelHandle, ModelCell>,
    /// Which model arena a context handle lives in.
    ctx_owner: HashMap<CtxHandle, ModelHandle>,
    next_handle: u64,
}

impl std::fmt::Debug for LlamaSubstrate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LlamaSubstrate")
            .field("models", &self.models.len())
            .field("contexts", &self.ctx_owner.len())
            .finish()
    }
}

impl LlamaSubstrate {
    /// Initialise the llama.cpp backend.
    ///
    /// # Errors
    ///
    /// [`SubstrateError::ModelLoad`] if the backend cannot be initialised —
    /// it is already initialised elsewhere in the process, or no compute
    /// backend could be brought up at all.
    pub fn new() -> Result<Self, SubstrateError> {
        let backend = LlamaBackend::init()
            .map_err(|e| SubstrateError::ModelLoad(format!("llama backend init failed: {e}")))?;
        Ok(Self {
            backend,
            models: HashMap::new(),
            ctx_owner: HashMap::new(),
            next_handle: 0,
        })
    }

    fn mint(&mut self) -> u64 {
        self.next_handle += 1;
        self.next_handle
    }

    /// The arena holding context `c`, or a clear error.
    fn ctx_arena(&mut self, c: CtxHandle) -> Result<&mut ModelCell, SubstrateError> {
        let m = *self
            .ctx_owner
            .get(&c)
            .ok_or_else(|| SubstrateError::Context(format!("unknown context handle {c}")))?;
        self.models
            .get_mut(&m)
            .ok_or_else(|| SubstrateError::Context(format!("context {c} outlived its model {m}")))
    }
}

impl Substrate for LlamaSubstrate {
    fn load_model(
        &mut self,
        path: &Path,
        n_gpu_layers: u32,
    ) -> Result<ModelHandle, SubstrateError> {
        let params = LlamaModelParams::default().with_n_gpu_layers(n_gpu_layers);
        let model = LlamaModel::load_from_file(&self.backend, path, &params)
            .map_err(|e| SubstrateError::ModelLoad(format!("{}: {e}", path.display())))?;
        let handle = self.mint();
        self.models
            .insert(handle, ModelCell::new(model, |_| HashMap::new()));
        Ok(handle)
    }

    /// Unload a model, destroying every context still borrowing it.
    ///
    /// Contexts are dropped before the model (the arena's drop order), which
    /// is what llama.cpp requires and what actually returns the VRAM.
    fn unload_model(&mut self, m: ModelHandle) -> Result<(), SubstrateError> {
        let arena = self
            .models
            .remove(&m)
            .ok_or_else(|| SubstrateError::ModelLoad(format!("unknown model handle {m}")))?;
        drop(arena);
        self.ctx_owner.retain(|_, owner| *owner != m);
        Ok(())
    }

    /// Create a context on model `m` with a requested window of `n_ctx`.
    ///
    /// **The granted window may be larger than the request**: llama.cpp pads
    /// `n_ctx` up (a request for 64 was granted 256 on this box), and there is
    /// no channel on this trait to report the granted size back. Callers must
    /// not assume they got exactly what they asked for — VRAM accounting
    /// absorbs the difference in its overhead margin, and `infer`'s own
    /// refusal check reads the real window back from llama.cpp rather than
    /// trusting this argument.
    fn create_context(&mut self, m: ModelHandle, n_ctx: u32) -> Result<CtxHandle, SubstrateError> {
        let n_ctx = NonZeroU32::new(n_ctx)
            .ok_or_else(|| SubstrateError::Context("n_ctx must be non-zero".to_string()))?;
        let handle = self.mint();
        // Split borrow: the backend and the arena map are disjoint fields, so
        // the lookup is inline rather than through `&mut self` helper.
        let backend = &self.backend;
        let arena = self
            .models
            .get_mut(&m)
            .ok_or_else(|| SubstrateError::ModelLoad(format!("unknown model handle {m}")))?;
        let params = LlamaContextParams::default().with_n_ctx(Some(n_ctx));
        arena.with_dependent_mut(|model, contexts| {
            let ctx = model
                .new_context(backend, params)
                .map_err(|e| SubstrateError::Context(format!("model {m}, n_ctx {n_ctx}: {e}")))?;
            contexts.insert(handle, ctx);
            Ok::<(), SubstrateError>(())
        })?;
        self.ctx_owner.insert(handle, m);
        Ok(handle)
    }

    fn destroy_context(&mut self, c: CtxHandle) -> Result<(), SubstrateError> {
        let m = self
            .ctx_owner
            .remove(&c)
            .ok_or_else(|| SubstrateError::Context(format!("unknown context handle {c}")))?;
        let arena = self.models.get_mut(&m).ok_or_else(|| {
            SubstrateError::Context(format!("context {c} outlived its model {m}"))
        })?;
        let removed = arena.with_dependent_mut(|_, contexts| contexts.remove(&c));
        if removed.is_none() {
            return Err(SubstrateError::Context(format!(
                "context {c} was indexed but absent from its model arena"
            )));
        }
        Ok(())
    }

    /// Tokenize, decode, sample greedily, and report what we counted.
    ///
    /// Continuation is automatic: the prompt gets a BOS only when the KV
    /// cache for this context is empty, so a context resumed from a saved
    /// image continues its conversation instead of restarting it.
    ///
    /// **Turn boundaries are the caller's job.** Two deliberate behaviours make
    /// this substrate's output *not* directly re-feedable as conversation:
    ///
    /// - an end-of-generation token stops the loop and is **not** fed back, so
    ///   it never enters the KV cache;
    /// - control tokens are stripped from [`Reply::text`] (rendering is
    ///   non-special), so the terminator is absent from the text too.
    ///
    /// A caller building a multi-turn conversation on one context must
    /// therefore supply the turn terminator itself at the head of the next
    /// prompt — see `tests/llama_semantic_test.rs`, which opens its follow-up
    /// prompt with `<|im_end|>`. Omit it and the model sees its own answer
    /// running straight into the next user turn.
    ///
    /// On failure the KV cache is rolled back to where this call found it, so
    /// a failed `infer` leaves the context exactly as usable as before.
    ///
    /// `stop` (protocol §11, envelope-v3): when `Some`, generation also
    /// terminates the instant the accumulated completion contains it — see
    /// [`generate_from`]'s loop for the exact truncation and KV-boundary
    /// semantics.
    fn infer(
        &mut self,
        c: CtxHandle,
        prompt: &str,
        max_tokens: u32,
        stop: Option<&str>,
    ) -> Result<Reply, SubstrateError> {
        let started = Instant::now();
        let arena = self.ctx_arena(c)?;
        let generated = arena.with_dependent_mut(|model, contexts| {
            let ctx = contexts
                .get_mut(&c)
                .ok_or_else(|| SubstrateError::Context(format!("unknown context handle {c}")))?;
            generate(model, ctx, prompt, max_tokens, stop)
        })?;
        Ok(Reply {
            text: generated.text,
            // Never `None`: both numbers were counted a few lines above, in
            // this process, by this loop.
            prompt_tokens: Some(generated.prompt_tokens),
            completion_tokens: Some(generated.completion_tokens),
            duration_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        })
    }

    /// Serialize this context's sequence-0 KV state.
    ///
    /// Size-bounded by construction: the buffer is allocated at exactly the
    /// size llama.cpp reports for this sequence, and a short or long write is
    /// surfaced as a "size mismatch" error rather than trusted.
    ///
    /// (The safe `LlamaContext::state_seq_get` cannot be used here: it returns
    /// an opaque `SeqState` with no byte accessor, and this trait's image is a
    /// `Vec<u8>` the pager must be able to spill to RAM and NVMe.)
    fn save_state(&mut self, c: CtxHandle) -> Result<Vec<u8>, SubstrateError> {
        let arena = self.ctx_arena(c)?;
        arena.with_dependent_mut(|_, contexts| {
            let ctx = contexts
                .get_mut(&c)
                .ok_or_else(|| SubstrateError::Context(format!("unknown context handle {c}")))?;
            let size = ctx.state_seq_get_size_ext(SEQ_ID, state_flags());
            // The one way the SAFETY invariant below could be violated: a zero
            // size makes `as_mut_ptr()` a dangling pointer with no capacity,
            // while the crate's wrapper hands C a `usize::MAX` write budget.
            // Refuse instead of pointing C at it.
            if size == 0 {
                return Err(SubstrateError::State(format!(
                    "state {STATE_SIZE_MISMATCH} on save: state size reported as zero"
                )));
            }
            let mut image = vec![0u8; size];
            // SAFETY: `image` is exactly `size` bytes and `size > 0`, so the
            // pointer is valid for `size` writes; `size` is the byte count
            // llama.cpp just reported it will write for this sequence and flag
            // set. The write is verified below.
            let written =
                unsafe { ctx.state_seq_get_data_ext(image.as_mut_ptr(), SEQ_ID, state_flags()) };
            if written != size {
                return Err(SubstrateError::State(format!(
                    "state seq {STATE_SIZE_MISMATCH} on save: expected {size}, actual {written}"
                )));
            }
            Ok(image)
        })
    }

    /// Restore a KV image previously produced by [`Self::save_state`].
    ///
    /// llama.cpp validates the image itself — a truncated buffer, a layer
    /// count or quantization that no longer matches, or more cells than the
    /// destination cache can hold — and a rejection comes back as
    /// [`SubstrateError::State`] containing "size mismatch". That string is
    /// the pager's cue to treat the image as invalidated (e.g. by a model
    /// upgrade) and cold-start instead, which is a degradation, never a crash.
    ///
    /// Note what is *not* a mismatch: an image is a sequence's cells, not a
    /// window-shaped blob, so a short image captured under a 2048-token window
    /// restores happily into a 256-token one (measured, not assumed).
    ///
    /// **A failed load leaves the destination sequence unusable.** llama.cpp
    /// clears sequence 0 before reading and abandons it wherever the read
    /// stopped, so on error the context holds a partial, meaningless cache —
    /// not the previous contents, and not nothing. Callers must treat a failed
    /// load as a cold start (destroy the context, or overwrite it with a good
    /// image); retrying in place, inferring, or saving from it are all wrong.
    fn load_state(&mut self, c: CtxHandle, bytes: &[u8]) -> Result<(), SubstrateError> {
        if bytes.is_empty() {
            return Err(SubstrateError::State(format!(
                "state seq {STATE_SIZE_MISMATCH} on load: image is empty"
            )));
        }
        let arena = self.ctx_arena(c)?;
        arena.with_dependent_mut(|_, contexts| {
            let ctx = contexts
                .get_mut(&c)
                .ok_or_else(|| SubstrateError::Context(format!("unknown context handle {c}")))?;
            // SAFETY: unlike the prohibited `set_state_data`, this wrapper
            // forwards `bytes.len()` to `llama_state_seq_set_data_ext`, so
            // llama.cpp's deserializer is bounded by the buffer it was given
            // and reports failure rather than reading past the end.
            let ok = unsafe { ctx.state_seq_set_data_ext(bytes, SEQ_ID, state_flags()) };
            if !ok {
                return Err(SubstrateError::State(format!(
                    "state seq {STATE_SIZE_MISMATCH} on load: llama.cpp rejected a {}-byte image \
                     (context shape changed, or the image is truncated); \
                     sequence {SEQ_ID} is now partial — cold-start, do not retry in place",
                    bytes.len()
                )));
            }
            Ok(())
        })
    }
}

/// What one `infer` call produced, with the counts it actually observed.
struct Generated {
    text: String,
    prompt_tokens: u32,
    completion_tokens: u32,
}

/// Run one prompt to completion, leaving the KV cache untouched on failure.
///
/// A decode that fails partway through does **not** unwind itself: llama.cpp
/// keeps whatever the successful chunks already wrote, and the next `infer` or
/// `save_state` on this handle would silently build on that wreckage — a
/// corrupt conversation and a corrupt image, with no error anywhere to explain
/// it. So anything that fails after the first successful decode is followed by
/// removing every cell this call added.
fn generate(
    model: &LlamaModel,
    ctx: &mut LlamaContext<'_>,
    prompt: &str,
    max_tokens: u32,
    stop: Option<&str>,
) -> Result<Generated, SubstrateError> {
    // Where this call found the cache: everything from here on is ours, so
    // this is exactly the rollback point.
    let entry_pos = ctx.kv_cache_seq_pos_max(SEQ_ID) + 1;
    let mut wrote_cache = false;
    match generate_from(
        model,
        ctx,
        prompt,
        max_tokens,
        stop,
        entry_pos,
        &mut wrote_cache,
    ) {
        Ok(generated) => Ok(generated),
        Err(failure) if wrote_cache => Err(roll_back(ctx, entry_pos, failure)),
        Err(failure) => Err(failure),
    }
}

/// Drop every cell at or after `entry_pos`, returning the error to report.
///
/// A rollback that itself fails is worse than the original failure — the
/// context is then holding a partial turn with no way to remove it — so it is
/// reported as a `State` error naming both failures, telling the pager to
/// destroy the context rather than reuse it.
fn roll_back(
    ctx: &mut LlamaContext<'_>,
    entry_pos: i32,
    failure: SubstrateError,
) -> SubstrateError {
    let from = u32::try_from(entry_pos).unwrap_or(0);
    match ctx.kv_cache_seq_rm(SEQ_ID, Some(from), None) {
        Ok(()) => failure,
        Err(rollback_failure) => SubstrateError::State(format!(
            "inference failed ({failure:?}) and the KV rollback of sequence {SEQ_ID} from \
             position {from} also failed ({rollback_failure}) — the context holds a partial \
             turn and must be destroyed, not reused"
        )),
    }
}

/// The body of [`generate`], starting from a known cache position.
///
/// Sets `wrote_cache` as soon as one decode has landed, which is what tells
/// the caller a rollback is owed.
#[allow(clippy::too_many_arguments)]
fn generate_from(
    model: &LlamaModel,
    ctx: &mut LlamaContext<'_>,
    prompt: &str,
    max_tokens: u32,
    stop: Option<&str>,
    entry_pos: i32,
    wrote_cache: &mut bool,
) -> Result<Generated, SubstrateError> {
    // Authoritative rather than remembered: whatever is in the KV cache right
    // now decides both the next position and whether a BOS is due. A context
    // restored from an image reports its restored length here, so
    // continuation needs no bookkeeping that could drift.
    let mut pos = entry_pos;
    let add_bos = if pos == 0 {
        AddBos::Always
    } else {
        AddBos::Never
    };

    let tokens = model
        .str_to_token(prompt, add_bos)
        .map_err(|e| SubstrateError::Infer(format!("tokenize: {e}")))?;
    if tokens.is_empty() {
        return Err(SubstrateError::Infer(
            "prompt tokenized to zero tokens".to_string(),
        ));
    }
    let prompt_tokens = u32::try_from(tokens.len()).map_err(|_| {
        SubstrateError::Infer(format!("prompt of {} tokens is absurd", tokens.len()))
    })?;

    // Law 2: refuse, never truncate. The kernel gates on an estimate; this is
    // the backstop that knows the real tokenization and the real window.
    // Read the window back rather than trusting what was requested: llama.cpp
    // pads it (a request for 64 came back as 256 on this box).
    //
    // The message carries `WINDOW_EXCEEDED` so the refusal survives the trip
    // across the trait boundary *as a refusal* — see that const's docs.
    let window = ctx.n_ctx();
    let needed = u64::try_from(pos).unwrap_or(0) + u64::from(prompt_tokens) + u64::from(max_tokens);
    if needed > u64::from(window) {
        return Err(SubstrateError::Infer(format!(
            "refusing: {pos} cached + {prompt_tokens} prompt + {max_tokens} requested tokens \
             {} of {window} tokens",
            crate::WINDOW_EXCEEDED
        )));
    }

    let n_batch = usize::try_from(ctx.n_batch()).unwrap_or(1).max(1);
    let mut batch = LlamaBatch::new(tokens.len().min(n_batch), 1);
    let last = tokens.len() - 1;

    // Decode the prompt, chunked to the context's batch size. Only the very
    // last token needs logits — it is the one we sample from.
    let mut offset = 0usize;
    while offset < tokens.len() {
        let end = (offset + n_batch).min(tokens.len());
        batch.clear();
        for (k, token) in tokens[offset..end].iter().enumerate() {
            let i = offset + k;
            let want_logits = i == last;
            let at = pos + i32::try_from(k).unwrap_or(0);
            batch
                .add(*token, at, &[SEQ_ID], want_logits)
                .map_err(|e| SubstrateError::Infer(format!("batch add at {at}: {e}")))?;
        }
        ctx.decode(&mut batch)
            .map_err(|e| SubstrateError::Infer(format!("decode prompt: {e}")))?;
        // From here on this call owns cells in the cache: a later failure has
        // something to roll back.
        *wrote_cache = true;
        pos += i32::try_from(end - offset).unwrap_or(0);
        offset = end;
    }

    let mut sampler = LlamaSampler::greedy();
    let mut bytes: Vec<u8> = Vec::new();
    let mut completion_tokens: u32 = 0;
    // Index of the logits row within the batch we just decoded.
    let mut logits_idx = batch.n_tokens() - 1;

    for _ in 0..max_tokens {
        let token = sampler.sample(ctx, logits_idx);
        if model.is_eog_token(token) {
            break;
        }
        bytes.extend_from_slice(&token_bytes(model, token)?);
        completion_tokens += 1;

        // Feed the token back so the KV cache — and therefore a later saved
        // image — includes everything we generated.
        batch.clear();
        batch
            .add(token, pos, &[SEQ_ID], true)
            .map_err(|e| SubstrateError::Infer(format!("batch add generated at {pos}: {e}")))?;
        pos += 1;
        ctx.decode(&mut batch)
            .map_err(|e| SubstrateError::Infer(format!("decode generated: {e}")))?;
        logits_idx = 0;

        // Envelope-v3 stop sequence (protocol §11, Amendment 3; the law-3
        // ruling: a stop string is *termination, not constraint* — nothing
        // above this point touched the model's distribution). Matched on
        // the accumulated bytes decoded STRICTLY (`str::from_utf8`, not
        // `_lossy`): a token whose bytes leave the tail of `bytes`
        // mid-multibyte-character makes this `Err`, which is read as "not
        // yet, wait for the next token's continuation bytes" rather than
        // "no match" — never a false miss AND never a false match against a
        // `U+FFFD` replacement char that isn't actually in the model's
        // output. Because the check only ever runs on a buffer that just
        // decoded successfully, the byte offset `find` returns is exact
        // against `bytes` itself, so truncating at it is exact too.
        //
        // The tag is INCLUDED (truncate at `idx + stop.len()`, not `idx`),
        // and `completion_tokens` already counted this token in full above
        // — an honest count of what was actually sampled, never adjusted
        // down to match the shorter returned text. Note the boundary this
        // leaves: the KV cache above already absorbed this whole token
        // (`batch.add`/`ctx.decode` just ran with the FULL token, not the
        // truncated text), so if a token's bytes carry content past the
        // stop tag, the model's own context has "seen" that trailing
        // content even though the caller never does — the same behavior
        // every stop-sequence implementation in a token-based serving stack
        // has, and not reachable without a token straddling the tag in the
        // shipped `codec-tasks-v1` fixtures (protocol §11's recorded
        // limit).
        if let Some(stop) = stop {
            if let Ok(text_so_far) = std::str::from_utf8(&bytes) {
                if let Some(idx) = text_so_far.find(stop) {
                    bytes.truncate(idx + stop.len());
                    break;
                }
            }
        }
    }

    Ok(Generated {
        // Bytes are accumulated and decoded once: a multi-byte character
        // split across two tokens still comes out whole.
        text: String::from_utf8_lossy(&bytes).into_owned(),
        prompt_tokens,
        completion_tokens,
    })
}

/// The raw bytes of one token, retrying once at the size llama.cpp asks for.
///
/// Rendering is non-special: a control token (`<|im_start|>` and friends) has
/// no plaintext piece and contributes nothing to the reply. llama.cpp says so
/// by writing zero bytes, which the crate reports as `UnknownTokenType`; that
/// is a rendering fact, not a failure, so it maps to an empty piece. The
/// token is still counted — the model really did emit it and we really did
/// decode it.
fn token_bytes(model: &LlamaModel, token: LlamaToken) -> Result<Vec<u8>, SubstrateError> {
    const FIRST_TRY: usize = 32;
    match model.token_to_piece_bytes(token, FIRST_TRY, false, None) {
        Ok(bytes) => Ok(bytes),
        Err(TokenToStringError::UnknownTokenType) => Ok(Vec::new()),
        Err(TokenToStringError::InsufficientBufferSpace(needed)) => {
            let size = usize::try_from(-needed).unwrap_or(FIRST_TRY);
            match model.token_to_piece_bytes(token, size, false, None) {
                Ok(bytes) => Ok(bytes),
                Err(TokenToStringError::UnknownTokenType) => Ok(Vec::new()),
                Err(e) => Err(SubstrateError::Infer(format!(
                    "detokenize {}: {e}",
                    token.0
                ))),
            }
        }
        Err(e) => Err(SubstrateError::Infer(format!(
            "detokenize {}: {e}",
            token.0
        ))),
    }
}
