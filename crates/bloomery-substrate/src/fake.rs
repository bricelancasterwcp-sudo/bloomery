//! `FakeSubstrate`: a scripted, in-memory [`Substrate`] for GPU-free tests.
//!
//! Every daemon-level test (Tasks 12-15) drives this instead of a real
//! llama.cpp backend: script replies with [`FakeSubstrate::script_reply`],
//! drive the daemon, then assert on [`FakeSubstrate::calls`] (call order)
//! and [`FakeSubstrate::ctx_history`] (per-context conversation state).

use std::collections::{HashMap, HashSet, VecDeque};

use crate::{CtxHandle, ModelHandle, Reply, Substrate, SubstrateError};

/// A scripted, in-process [`Substrate`] implementation.
///
/// - Replies are served FIFO from a script queue; draining the queue with no
///   more replies scripted is a hard error (`SubstrateError::Infer("script
///   exhausted")`), not a default reply, so tests fail loudly instead of
///   silently passing on placeholder output.
/// - Every call is logged (e.g. `"load_model"`, `"infer:c1"`) so tests can
///   assert on ordering.
/// - Each context accumulates the prompts it has seen as a plain string,
///   which `save_state`/`load_state` round-trip verbatim — this is the fake
///   stand-in for real KV-cache suspend/resume.
pub struct FakeSubstrate {
    scripted_replies: VecDeque<Reply>,
    calls: Vec<String>,
    history: HashMap<CtxHandle, String>,
    models: HashSet<ModelHandle>,
    contexts: HashMap<CtxHandle, ModelHandle>,
    next_model_handle: ModelHandle,
    next_ctx_handle: CtxHandle,
    /// Every `n_gpu_layers` value passed to [`Substrate::load_model`], in
    /// call order — Task 3's plumbing test needs to see which value each
    /// load actually used (a per-model override vs. the pager-global
    /// default), and `calls()` alone only records that a load happened, not
    /// with what argument.
    load_n_gpu_layers: Vec<u32>,
}

impl FakeSubstrate {
    pub fn new() -> Self {
        Self {
            scripted_replies: VecDeque::new(),
            calls: Vec::new(),
            history: HashMap::new(),
            models: HashSet::new(),
            contexts: HashMap::new(),
            next_model_handle: 0,
            next_ctx_handle: 0,
            load_n_gpu_layers: Vec::new(),
        }
    }

    /// Queue a reply to be returned by the next [`Substrate::infer`] call.
    /// Replies are served FIFO across the whole fake, regardless of context.
    pub fn script_reply(&mut self, r: Reply) {
        self.scripted_replies.push_back(r);
    }

    /// Every call made so far, in order (e.g. `"load_model"`, `"infer:c1"`).
    pub fn calls(&self) -> &[String] {
        &self.calls
    }

    /// Every `n_gpu_layers` value passed to [`Substrate::load_model`], in
    /// call order — index-aligned with the `"load_model"` entries in
    /// [`Self::calls`].
    pub fn load_n_gpu_layers(&self) -> &[u32] {
        &self.load_n_gpu_layers
    }

    /// The accumulated prompt history for context `c`, if it is currently
    /// known (created and not yet destroyed).
    pub fn ctx_history(&self, c: CtxHandle) -> Option<&str> {
        self.history.get(&c).map(String::as_str)
    }

    fn log(&mut self, call: impl Into<String>) {
        self.calls.push(call.into());
    }
}

impl Default for FakeSubstrate {
    fn default() -> Self {
        Self::new()
    }
}

impl Substrate for FakeSubstrate {
    fn load_model(
        &mut self,
        _path: &std::path::Path,
        n_gpu_layers: u32,
    ) -> Result<ModelHandle, SubstrateError> {
        self.log("load_model");
        self.load_n_gpu_layers.push(n_gpu_layers);
        self.next_model_handle += 1;
        let handle = self.next_model_handle;
        self.models.insert(handle);
        Ok(handle)
    }

    fn unload_model(&mut self, m: ModelHandle) -> Result<(), SubstrateError> {
        self.log(format!("unload_model:m{m}"));
        if !self.models.remove(&m) {
            return Err(SubstrateError::ModelLoad(format!(
                "unknown model handle {m}"
            )));
        }
        Ok(())
    }

    fn create_context(&mut self, m: ModelHandle, _n_ctx: u32) -> Result<CtxHandle, SubstrateError> {
        if !self.models.contains(&m) {
            return Err(SubstrateError::Context(format!("unknown model handle {m}")));
        }
        self.next_ctx_handle += 1;
        let handle = self.next_ctx_handle;
        self.log(format!("create_context:c{handle}"));
        self.contexts.insert(handle, m);
        self.history.insert(handle, String::new());
        Ok(handle)
    }

    fn destroy_context(&mut self, c: CtxHandle) -> Result<(), SubstrateError> {
        self.log(format!("destroy_context:c{c}"));
        if self.contexts.remove(&c).is_none() {
            return Err(SubstrateError::Context(format!(
                "unknown context handle {c}"
            )));
        }
        self.history.remove(&c);
        Ok(())
    }

    fn infer(
        &mut self,
        c: CtxHandle,
        prompt: &str,
        _max_tokens: u32,
    ) -> Result<Reply, SubstrateError> {
        self.log(format!("infer:c{c}"));
        if !self.contexts.contains_key(&c) {
            return Err(SubstrateError::Context(format!(
                "unknown context handle {c}"
            )));
        }
        let reply = self
            .scripted_replies
            .pop_front()
            .ok_or_else(|| SubstrateError::Infer("script exhausted".to_string()))?;
        let entry = self.history.entry(c).or_default();
        if entry.is_empty() {
            entry.push_str(prompt);
        } else {
            entry.push('\n');
            entry.push_str(prompt);
        }
        Ok(reply)
    }

    fn save_state(&mut self, c: CtxHandle) -> Result<Vec<u8>, SubstrateError> {
        self.log(format!("save_state:c{c}"));
        let history = self
            .history
            .get(&c)
            .ok_or_else(|| SubstrateError::Context(format!("unknown context handle {c}")))?;
        Ok(history.clone().into_bytes())
    }

    fn load_state(&mut self, c: CtxHandle, bytes: &[u8]) -> Result<(), SubstrateError> {
        self.log(format!("load_state:c{c}"));
        if !self.contexts.contains_key(&c) {
            return Err(SubstrateError::Context(format!(
                "unknown context handle {c}"
            )));
        }
        let restored = String::from_utf8(bytes.to_vec())
            .map_err(|e| SubstrateError::State(format!("invalid state bytes: {e}")))?;
        self.history.insert(c, restored);
        Ok(())
    }
}
