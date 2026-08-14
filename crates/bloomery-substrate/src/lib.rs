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

    fn infer(
        &mut self,
        c: CtxHandle,
        prompt: &str,
        max_tokens: u32,
    ) -> Result<Reply, SubstrateError>;

    fn save_state(&mut self, c: CtxHandle) -> Result<Vec<u8>, SubstrateError>;

    fn load_state(&mut self, c: CtxHandle, bytes: &[u8]) -> Result<(), SubstrateError>;
}
