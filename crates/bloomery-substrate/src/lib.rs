//! The boundary between the bloomery kernel and inference.
//!
//! `Substrate` is the trait every inference backend implements (llama.cpp in
//! Task 11, a scripted `FakeSubstrate` here for GPU-free testing everywhere
//! else). `Reply` carries token counts as `Option<u32>` on purpose: a
//! substrate that returns a reply without stats is an infrastructure
//! failure, never a model failure (project law 4). The `contract` module
//! turns that into a first-class, catchable error via `enforce_contract`.

pub mod contract;
pub mod fake;
#[cfg(feature = "llama")]
pub mod llama;

/// The substring every KV-image rejection carries in its
/// [`SubstrateError::State`] message.
///
/// A cross-task contract, not decoration: the pager treats a state error
/// containing this as *image invalidated* — cold-start the agent, do not fail
/// the request. It is a `const` so both sides of that contract reference the
/// same symbol instead of retyping a string that could silently drift apart.
pub const STATE_SIZE_MISMATCH: &str = "size mismatch";

/// The substring every *window* refusal carries in its
/// [`SubstrateError::Infer`] message.
///
/// The same kind of cross-task contract as [`STATE_SIZE_MISMATCH`], for law
/// 2 rather than law 3. The kernel gates on a pre-tokenization estimate; a
/// substrate that knows the real tokenization and the real (post-padding)
/// window is the backstop that catches what the estimate let through. That
/// backstop is still a *refusal*, so it has to survive the trip across the
/// trait boundary as one instead of decaying into a generic infer failure —
/// the pager matches on this marker to keep it classified as
/// "too large for the window", never "the model broke".
pub const WINDOW_EXCEEDED: &str = "exceed the window";

/// Opaque handle to a loaded model, minted by [`Substrate::load_model`].
pub type ModelHandle = u64;

/// Opaque handle to an inference context, minted by [`Substrate::create_context`].
pub type CtxHandle = u64;

/// A single inference reply.
///
/// `prompt_tokens` and `completion_tokens` are `Option` because the
/// substrate might fail to report them; `None` is not "zero tokens", it is
/// "the substrate didn't tell us". [`contract::enforce_contract`] is the
/// only place that is allowed to treat a missing count as an error.
#[derive(Debug, Clone, PartialEq)]
pub struct Reply {
    pub text: String,
    pub prompt_tokens: Option<u32>,
    pub completion_tokens: Option<u32>,
    pub duration_ms: u64,
}

/// Errors a [`Substrate`] implementation can raise.
#[derive(Debug)]
pub enum SubstrateError {
    ModelLoad(String),
    Context(String),
    Infer(String),
    State(String),
}

/// The kernel/inference boundary.
///
/// Implementations own model and context lifecycle, run inference, and
/// snapshot/restore per-context state so the daemon can suspend and resume
/// contexts without losing conversation history.
pub trait Substrate {
    fn load_model(
        &mut self,
        path: &std::path::Path,
        n_gpu_layers: u32,
    ) -> Result<ModelHandle, SubstrateError>;

    fn unload_model(&mut self, m: ModelHandle) -> Result<(), SubstrateError>;

    fn create_context(&mut self, m: ModelHandle, n_ctx: u32) -> Result<CtxHandle, SubstrateError>;

    fn destroy_context(&mut self, c: CtxHandle) -> Result<(), SubstrateError>;

    /// `stop`, when `Some`, is a literal stop string (protocol §11's
    /// action-terminated envelope-v3, Amendment 3): generation terminates at
    /// the first occurrence of `stop` in the accumulated completion, the
    /// occurrence is INCLUDED in the returned text, and `completion_tokens`
    /// still counts every token actually generated (never fudged down to
    /// match the truncated text). `None` is today's behavior, unchanged —
    /// generation runs to `max_tokens` or an end-of-generation token.
    ///
    /// The law-3 ruling (protocol §11): a stop string is *termination, not
    /// constraint* — the model's distribution is untouched up to the tag,
    /// the same class as `max_tokens` and chat-template stop tokens, never
    /// grammar-forced decoding.
    fn infer(
        &mut self,
        c: CtxHandle,
        prompt: &str,
        max_tokens: u32,
        stop: Option<&str>,
    ) -> Result<Reply, SubstrateError>;

    fn save_state(&mut self, c: CtxHandle) -> Result<Vec<u8>, SubstrateError>;

    fn load_state(&mut self, c: CtxHandle, bytes: &[u8]) -> Result<(), SubstrateError>;
}
