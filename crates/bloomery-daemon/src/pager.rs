//! The pager: the part of bloomery that decides what lives in VRAM.
//!
//! It owns the agent table, the KV image store, the model registry and the
//! substrate, and it is the only place where those meet. Four rules shape
//! everything below and in [`paging`]:
//!
//! 1. **Memory pressure is pre-checked, never inferred from a failure.**
//!    Every placement runs `plan_residency` against measured free VRAM
//!    *before* a context is created; a refusal returns the arithmetic and
//!    never touches the substrate. (llama.cpp #22629 — allocate first, read
//!    the wreckage afterwards — is the shipped counterexample.)
//! 2. **Refuse, never truncate.** An over-large prompt or an unplaceable
//!    agent is refused with numbers the caller can act on.
//! 3. **A rejected KV image is a cold start, not an error.** A stale digest,
//!    a corrupt spill, or a substrate that rejects the image with
//!    `STATE_SIZE_MISMATCH` all invalidate the image, journal `Degraded`, and
//!    serve the request from a fresh context.
//! 4. **Every decision is journaled**, including the ones that refuse, and a
//!    journal that cannot be written fails the request rather than letting
//!    the pager act unobserved.
//!
//! Everything here is generic over [`Substrate`], so the whole file is
//! exercised GPU-free against `FakeSubstrate`.

mod error;
mod journal;
mod paging;
mod status;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use bloomery_core::budget::{Budget, BudgetExhausted};
use bloomery_core::geometry::{kv_bytes_per_token, usable_window, GeometryInput};
use bloomery_core::gguf::GgufMeta;
use bloomery_core::journal::{Journal, PagerOpKind};
use bloomery_core::profile::Profile;
use bloomery_substrate::contract::{enforce_contract, ContractViolation, VerifiedReply};
use bloomery_substrate::{ModelHandle, Substrate};

use crate::agents::{model_digest, Agent, AgentState, AgentTable, ImageStore};
use error::sub;
use journal as jrnl;
use status::{bound_by_str, state_str};

pub use error::PagerError;
pub use status::{AgentInfo, AgentStatus, ModelStatus, StatusReport};

/// VRAM held back from the window law for allocator and compute buffers.
///
/// Zero by default: the pager has not measured this machine's overhead, and
/// an unmeasured term is not silently invented (law 5). The daemon wires the
/// operator's `config.overhead_mib` in via [`Pager::set_overhead_bytes`].
const DEFAULT_OVERHEAD_BYTES: u64 = 0;

/// Offload every layer by default — llama.cpp clamps a too-large value down
/// to the model's layer count. The pager's VRAM accounting assumes the KV
/// cache lives on the GPU, so anything less would make its arithmetic a lie.
const DEFAULT_N_GPU_LAYERS: u32 = u32::MAX;

/// Conservative chars-per-token floor for the pre-tokenization prompt gate.
const CHARS_PER_TOKEN: u64 = 3;

/// A registered model: its file, geometry, blob identity, optional profile,
/// and the substrate handle once its weights are actually loaded.
struct ModelEntry {
    path: PathBuf,
    meta: GgufMeta,
    digest: String,
    profile: Option<Profile>,
    kv_per_token: u64,
    handle: Option<ModelHandle>,
}

pub struct Pager<S: Substrate> {
    substrate: S,
    journal: Journal,
    images: ImageStore,
    table: AgentTable,
    models: HashMap<String, ModelEntry>,
    free_vram: Box<dyn Fn() -> Option<u64>>,
    overhead_bytes: u64,
    n_gpu_layers: u32,
    /// Monotonic: agent ids are the pager's to keep unique, because
    /// `plan_residency`'s behavior is unspecified for duplicate ids.
    next_agent_seq: u64,
    vram_unmeasured_logged: bool,
}

impl<S: Substrate> Pager<S> {
    pub fn new(
        substrate: S,
        journal: Journal,
        image_store: ImageStore,
        free_vram: Box<dyn Fn() -> Option<u64>>,
    ) -> Pager<S> {
        Pager {
            substrate,
            journal,
            images: image_store,
            table: AgentTable::new(),
            models: HashMap::new(),
            free_vram,
            overhead_bytes: DEFAULT_OVERHEAD_BYTES,
            n_gpu_layers: DEFAULT_N_GPU_LAYERS,
            next_agent_seq: 0,
            vram_unmeasured_logged: false,
        }
    }

    pub fn set_overhead_bytes(&mut self, bytes: u64) {
        self.overhead_bytes = bytes;
    }

    pub fn set_n_gpu_layers(&mut self, n: u32) {
        self.n_gpu_layers = n;
    }

    /// Read-only view of the substrate, for inspection and tests.
    pub fn substrate(&self) -> &S {
        &self.substrate
    }

    /// Registers (or re-registers) `name`, digesting the weights file so the
    /// image store can tell a resumable image from a stale one.
    ///
    /// Re-registering a name whose file changed invalidates everything built
    /// on it, so any loaded weights are unloaded and the next request reloads
    /// from disk. Agents still holding a context on the old weights make that
    /// unsafe, so re-registration is refused while any of them is resident —
    /// suspend them first.
    pub fn register_model(
        &mut self,
        name: &str,
        gguf: &Path,
        meta: GgufMeta,
        profile: Option<Profile>,
    ) -> Result<(), PagerError> {
        let digest = model_digest(gguf).map_err(|e| {
            PagerError::Substrate(format!("failed to digest {}: {e}", gguf.display()))
        })?;
        let previous = match self.models.get(name) {
            None => None,
            Some(entry) => {
                let resident = self
                    .table
                    .iter()
                    .filter(|a| a.model == name && matches!(a.state, AgentState::Resident { .. }))
                    .count();
                if resident > 0 {
                    return Err(PagerError::Substrate(format!(
                        "cannot re-register {name}: {resident} agent(s) still resident on it"
                    )));
                }
                entry.handle
            }
        };
        if let Some(handle) = previous {
            self.substrate.unload_model(handle).map_err(sub)?;
            jrnl::model_unloaded(&mut self.journal, name)?;
        }
        self.models.insert(
            name.to_string(),
            ModelEntry {
                path: gguf.to_path_buf(),
                kv_per_token: kv_bytes_per_token(&meta),
                meta,
                digest,
                profile,
                handle: None,
            },
        );
        Ok(())
    }

    /// Creates an agent and computes its window. No VRAM is committed: the
    /// agent starts `Fresh` and only becomes resident when it first infers.
    pub fn create_agent(
        &mut self,
        model: &str,
        priority: u8,
        window_cap: Option<u32>,
        budget_tokens: u64,
    ) -> Result<AgentInfo, PagerError> {
        let (kv_per_token, training_ctx, weights_bytes, measured_ceiling) = {
            let entry = self
                .models
                .get(model)
                .ok_or_else(|| PagerError::UnknownModel(model.to_string()))?;
            (
                entry.kv_per_token,
                entry.meta.training_ctx,
                entry.meta.weights_bytes,
                entry.profile.as_ref().and_then(|p| p.measured_ceiling()),
            )
        };
        let free_vram_bytes = self.probe_free_vram()?;
        let window = usable_window(&GeometryInput {
            training_ctx,
            kv_per_token,
            weights_bytes,
            free_vram_bytes,
            overhead_bytes: self.overhead_bytes,
            user_cap: window_cap,
            measured_ceiling,
        });

        self.next_agent_seq += 1;
        let id = format!("a{}", self.next_agent_seq);
        let bound_by = bound_by_str(window.bound_by);
        jrnl::agent_created(
            &mut self.journal,
            &id,
            model,
            priority,
            window.tokens,
            bound_by,
            budget_tokens,
        )?;
        let info = AgentInfo {
            id: id.clone(),
            window_tokens: window.tokens,
            bound_by: bound_by.to_string(),
        };
        self.table.insert(Agent {
            id,
            model: model.to_string(),
            priority,
            kv_bytes: u64::from(window.tokens).saturating_mul(kv_per_token),
            window,
            budget: Budget::new(budget_tokens),
            state: AgentState::Fresh,
        });
        Ok(info)
    }

    /// Runs one inference for `id`, paging it in first if needed.
    ///
    /// The gates run cheapest-and-most-certain first — budget, prompt size,
    /// then residency — and nothing reaches the substrate until all three
    /// pass. A reply that arrives without token stats is an infrastructure
    /// failure (law 4), so it is journaled as a contract violation and
    /// charged to nobody's budget.
    pub fn infer(
        &mut self,
        id: &str,
        prompt: &str,
        max_tokens: u32,
    ) -> Result<VerifiedReply, PagerError> {
        let requested = u64::from(max_tokens);
        let (window_tokens, budget_check) = {
            let a = self
                .table
                .get(id)
                .ok_or_else(|| PagerError::UnknownAgent(id.to_string()))?;
            (a.window.tokens, a.budget.check(requested))
        };
        if let Err(BudgetExhausted {
            remaining,
            requested,
        }) = budget_check
        {
            jrnl::budget_refused(&mut self.journal, id, remaining, requested)?;
            return Err(PagerError::Budget {
                remaining,
                requested,
            });
        }

        // Conservative chars->tokens floor. The substrate's real tokenizer is
        // the backstop; this gate exists so an obviously-too-large prompt is
        // refused with arithmetic instead of silently truncated (law 2).
        let needed_tokens = (prompt.len() as u64 / CHARS_PER_TOKEN).saturating_add(requested);
        if needed_tokens > u64::from(window_tokens) {
            jrnl::refusal(
                &mut self.journal,
                id,
                needed_tokens,
                window_tokens,
                "prompt + max_tokens exceeds the computed window".to_string(),
            )?;
            return Err(PagerError::PromptTooLarge {
                needed_tokens,
                window_tokens,
            });
        }

        let ctx = self.ensure_resident(id)?;
        jrnl::infer_started(&mut self.journal, id, prompt)?;
        let reply = self.substrate.infer(ctx, prompt, max_tokens).map_err(sub)?;
        let verified = match enforce_contract(reply) {
            Ok(v) => v,
            Err(ContractViolation::MissingStats) => {
                jrnl::contract_violation(&mut self.journal, id, "MissingStats")?;
                return Err(PagerError::Contract(
                    "substrate reply omitted token stats".to_string(),
                ));
            }
        };
        let charged =
            u64::from(verified.prompt_tokens).saturating_add(u64::from(verified.completion_tokens));
        self.table
            .get_mut(id)
            .expect("agent existence checked at entry")
            .budget
            .charge(charged);
        jrnl::infer_completed(&mut self.journal, id, &verified)?;
        Ok(verified)
    }

    /// Pages `id` out to NVMe: save its image, spill it, drop the context.
    /// An agent that isn't resident is already suspended, so this is a no-op
    /// rather than an error.
    pub fn suspend(&mut self, id: &str) -> Result<(), PagerError> {
        if self.table.get(id).is_none() {
            return Err(PagerError::UnknownAgent(id.to_string()));
        }
        self.save_image(id, PagerOpKind::SuspendSave, true)?;
        self.destroy_context(id)
    }

    /// Makes `id` resident without inferring.
    pub fn resume(&mut self, id: &str) -> Result<(), PagerError> {
        self.ensure_resident(id).map(|_| ())
    }

    /// Drops a model's weights, paging out every agent still holding a
    /// context on it first. `ModelUnloaded` is journaled only when weights
    /// were actually released — the cold-switch bench measures real unloads,
    /// not requests to unload something that was never loaded.
    pub fn unload_model(&mut self, name: &str) -> Result<(), PagerError> {
        let handle = self
            .models
            .get(name)
            .ok_or_else(|| PagerError::UnknownModel(name.to_string()))?
            .handle;
        let holders: Vec<String> = self
            .table
            .iter()
            .filter(|a| a.model == name && matches!(a.state, AgentState::Resident { .. }))
            .map(|a| a.id.clone())
            .collect();
        for id in holders {
            self.suspend(&id)?;
        }
        let Some(handle) = handle else {
            return Ok(());
        };
        self.substrate.unload_model(handle).map_err(sub)?;
        if let Some(entry) = self.models.get_mut(name) {
            entry.handle = None;
        }
        jrnl::model_unloaded(&mut self.journal, name)
    }

    /// A serializable snapshot of everything the pager is holding, sorted so
    /// two calls with the same state produce the same document.
    pub fn status(&self) -> StatusReport {
        let mut agents: Vec<AgentStatus> = self
            .table
            .iter()
            .map(|a| AgentStatus {
                id: a.id.clone(),
                model: a.model.clone(),
                priority: a.priority,
                state: state_str(&a.state),
                window_tokens: a.window.tokens,
                bound_by: bound_by_str(a.window.bound_by),
                vram_unmeasured: a.window.vram_unmeasured,
                kv_bytes: a.kv_bytes,
                budget_granted: a.budget.granted(),
                budget_spent: a.budget.spent(),
            })
            .collect();
        agents.sort_by(|x, y| x.id.cmp(&y.id));
        let mut models: Vec<ModelStatus> = self
            .models
            .iter()
            .map(|(name, m)| ModelStatus {
                name: name.clone(),
                digest: m.digest.clone(),
                loaded: m.handle.is_some(),
                kv_per_token: m.kv_per_token,
                training_ctx: m.meta.training_ctx,
            })
            .collect();
        models.sort_by(|x, y| x.name.cmp(&y.name));
        StatusReport {
            free_vram_bytes: (self.free_vram)(),
            resident_kv_bytes: self.resident_kv_bytes(),
            agents,
            models,
        }
    }
}
