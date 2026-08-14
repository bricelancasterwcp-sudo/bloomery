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
pub use status::{AgentInfo, AgentStatus, ModelStatus, StatusReport, TierStatus};

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

/// Priority and budget an API request that names neither is created with.
///
/// These are the pager's **initial** values, not the daemon's policy: they
/// mirror `config.rs`'s `default_priority` / `default_budget_tokens`, and
/// `main.rs` overwrites both from the operator's config via
/// [`Pager::set_defaults`]. Task 14 hardcoded these numbers in the HTTP
/// layer instead, which made the config keys dead; they live here now so
/// every surface reads one carried value rather than retyping a constant.
const DEFAULT_PRIORITY: u8 = 100;
const DEFAULT_BUDGET_TOKENS: u64 = 200_000;

/// Source of bloomery's static VRAM budget — see [`Pager::new`].
///
/// `Send + Sync` from the start: Task 14 shares one pager across request
/// threads behind a lock, and adding the bounds later would be a breaking
/// change to every caller that built one.
pub type FreeVramFn = Box<dyn Fn() -> Option<u64> + Send + Sync>;

/// A registered model: its file, geometry, blob identity, optional profile,
/// and the substrate handle once its weights are actually loaded.
struct ModelEntry {
    path: PathBuf,
    meta: GgufMeta,
    digest: String,
    profile: Option<Profile>,
    kv_per_token: u64,
    handle: Option<ModelHandle>,
    /// Whether this model's provisional (POST-window) admission and its
    /// `allow_unprofiled` admission have already been journaled. Said once
    /// per model per reason, not once per agent: assay alone makes ~110
    /// calls through the POST window, and a `Degraded` per call would bury
    /// the journal in one repeated sentence (same discipline as
    /// `probe_free_vram`'s one-shot unmeasured-VRAM note). Re-registering a
    /// model resets both, because it is a new entry.
    provisional_logged: bool,
    unprofiled_logged: bool,
}

pub struct Pager<S: Substrate> {
    substrate: S,
    journal: Journal,
    images: ImageStore,
    table: AgentTable,
    models: HashMap<String, ModelEntry>,
    /// bloomery's static VRAM budget — see [`Pager::new`].
    free_vram: FreeVramFn,
    overhead_bytes: u64,
    n_gpu_layers: u32,
    /// Monotonic: agent ids are the pager's to keep unique, because
    /// `plan_residency`'s behavior is unspecified for duplicate ids.
    next_agent_seq: u64,
    vram_unmeasured_logged: bool,
    /// True only while the boot-time POST is probing this daemon — see
    /// [`Pager::set_posting`].
    posting: bool,
    /// The operator's law-5 override — see [`Pager::set_allow_unprofiled`].
    allow_unprofiled: bool,
    default_priority: u8,
    default_budget_tokens: u64,
    /// The operator-declared hardware tier, for `/status`. `None` until the
    /// daemon wires one: an undeclared tier is reported as unknown, never
    /// invented (law 5's None-vs-zero, applied to a label).
    tier: Option<TierStatus>,
}

impl<S: Substrate> Pager<S> {
    /// Builds a pager.
    ///
    /// `free_vram` returns **bloomery's static VRAM budget** — the pool this
    /// daemon is allowed to fill, e.g. driver-reported free VRAM measured
    /// once at boot. It is reservation accounting: the pager subtracts its
    /// own residents from this number itself.
    ///
    /// It must **not** be a live driver read. A live read already excludes
    /// the contexts the pager allocated, so subtracting residents from it
    /// would count every resident twice and shrink the usable pool with each
    /// admission. `None` means unmeasured (never zero) and drops the pager
    /// to a residency-count cap of one.
    pub fn new(
        substrate: S,
        journal: Journal,
        image_store: ImageStore,
        free_vram: FreeVramFn,
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
            posting: false,
            // Permissive by default, exactly like `DEFAULT_OVERHEAD_BYTES`
            // is zero by default: the *pager* has no opinion on admission
            // policy — `register_model` accepts `profile: None` — and the
            // daemon is the one place that reads an operator's config. The
            // hazard is the same one Task 13 pinned for overhead: a
            // construction path that forgets to wire it runs permissive, so
            // `main.rs` sets it explicitly at its single construction site.
            allow_unprofiled: true,
            default_priority: DEFAULT_PRIORITY,
            default_budget_tokens: DEFAULT_BUDGET_TOKENS,
            tier: None,
        }
    }

    pub fn set_overhead_bytes(&mut self, bytes: u64) {
        self.overhead_bytes = bytes;
    }

    pub fn set_n_gpu_layers(&mut self, n: u32) {
        self.n_gpu_layers = n;
    }

    /// Marks the daemon as running its boot-time POST (or done with it).
    ///
    /// While set, a model with no profile is still admitted: assay has to be
    /// able to drive `/v1` before any profile can exist (see [`crate::post`]
    /// for the whole chicken-and-egg argument). Every such admission is
    /// journaled, and the flag is cleared as soon as POST finishes, on every
    /// path — a `posting` flag left set is law 5 suspended forever.
    pub fn set_posting(&mut self, posting: bool) {
        self.posting = posting;
    }

    /// The operator's law-5 override: when true, an unprofiled model is
    /// admitted anyway and the degradation is journaled by name.
    ///
    /// This is a *policy* input, so the daemon must always wire it from
    /// `config.allow_unprofiled` — see the note on the field.
    pub fn set_allow_unprofiled(&mut self, allow: bool) {
        self.allow_unprofiled = allow;
    }

    /// Sets the priority and budget an API request that names neither is
    /// created with (`config.default_priority` /
    /// `config.default_budget_tokens`).
    pub fn set_defaults(&mut self, priority: u8, budget_tokens: u64) {
        self.default_priority = priority;
        self.default_budget_tokens = budget_tokens;
    }

    pub fn default_priority(&self) -> u8 {
        self.default_priority
    }

    pub fn default_budget_tokens(&self) -> u64 {
        self.default_budget_tokens
    }

    /// Records the operator-declared hardware tier for `/status`. It is a
    /// label, not a measurement: it is what this daemon's profiles are
    /// marked with, so a reader can tell an enthusiast-16GB number from an
    /// emulated one.
    pub fn set_tier(&mut self, name: &str, emulated: bool) {
        self.tier = Some(TierStatus {
            name: name.to_string(),
            emulated,
        });
    }

    /// Attaches a measured [`Profile`] to a registered model — POST's whole
    /// output (see [`crate::post`]).
    ///
    /// Agents that already exist keep the window they were quoted at
    /// creation; the profile's ceiling binds every agent created *after*
    /// this. Re-quoting a live agent's window would silently change a number
    /// its caller was already told.
    pub fn attach_profile(&mut self, name: &str, profile: Profile) -> Result<(), PagerError> {
        let entry = self
            .models
            .get_mut(name)
            .ok_or_else(|| PagerError::UnknownModel(name.to_string()))?;
        entry.profile = Some(profile);
        Ok(())
    }

    /// Journals one POST outcome for `model` (`"ok"`, or `"failed: …"`).
    ///
    /// POST runs outside the pager but must not open a second writer to the
    /// journal the pager owns: two `BufWriter`s appending to one audit log
    /// is exactly the kind of interleaving nobody can replay. So the boot
    /// path records through the pager, and there stays one writer.
    pub fn journal_post(
        &mut self,
        model: &str,
        outcome: &str,
        profile_path: Option<String>,
    ) -> Result<(), PagerError> {
        jrnl::post(&mut self.journal, model, outcome, profile_path)
    }

    /// Journals a degradation raised outside the pager (POST failures at
    /// boot) — same single-writer reason as [`Pager::journal_post`].
    pub fn journal_degraded(&mut self, reason: String) -> Result<(), PagerError> {
        jrnl::degraded(&mut self.journal, reason)
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
                provisional_logged: false,
                unprofiled_logged: false,
            },
        );
        Ok(())
    }

    /// Creates an agent and computes its window. No VRAM is committed: the
    /// agent starts `Fresh` and only becomes resident when it first infers.
    ///
    /// Admission is profile-gated (law 5) — see [`Pager::admit`].
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
        self.admit(model)?;
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

    /// Law 5's gate: no model gets work without a measured capability
    /// profile.
    ///
    /// Three things can still admit an unprofiled model, and they are not
    /// interchangeable:
    ///
    /// 1. **`posting`** — the bounded window while POST probes this daemon,
    ///    which is the only way a profile can ever come to exist
    ///    ([`crate::post`]). Journaled per model.
    /// 2. **`allow_unprofiled`** — the operator's explicit override.
    ///    Journaled per model, naming it, because the daemon is then serving
    ///    a model whose real ceiling and codecs nobody measured.
    /// 3. Neither: refused as [`PagerError::Unprofiled`], with the model's
    ///    name, which the HTTP layers render as `422`.
    ///
    /// An unprofiled model is never *silently* admitted on any path.
    ///
    /// The gate is at agent **creation**, not per inference: an agent
    /// admitted inside the POST window keeps working after the window
    /// closes. Cutting a live conversation off mid-turn because POST
    /// finished would be its own dishonesty, and the window is a few
    /// seconds of boot. New work on a still-unprofiled model is refused.
    fn admit(&mut self, model: &str) -> Result<(), PagerError> {
        let has_profile = self
            .models
            .get(model)
            .is_some_and(|entry| entry.profile.is_some());
        if has_profile {
            return Ok(());
        }
        let posting = self.posting;
        if !posting && !self.allow_unprofiled {
            return Err(PagerError::Unprofiled(model.to_string()));
        }
        let first_time = match self.models.get_mut(model) {
            None => false,
            Some(entry) => {
                let said = if posting {
                    &mut entry.provisional_logged
                } else {
                    &mut entry.unprofiled_logged
                };
                let first = !*said;
                *said = true;
                first
            }
        };
        if first_time {
            let reason = if posting {
                format!("provisional admission: {model} has no profile yet; POST in progress")
            } else {
                format!(
                    "admitting agents on {model} with no capability profile \
                     (allow_unprofiled); its ceiling and codecs are unmeasured"
                )
            };
            jrnl::degraded(&mut self.journal, reason)?;
        }
        Ok(())
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
        let reply = match self.substrate.infer(ctx, prompt, max_tokens) {
            Ok(reply) => reply,
            Err(e) => {
                return Err(self.classify_infer_error(id, e, needed_tokens, window_tokens)?)
            }
        };
        let verified = match enforce_contract(reply) {
            Ok(v) => v,
            Err(ContractViolation::MissingStats) => {
                jrnl::contract_violation(&mut self.journal, id, "MissingStats")?;
                // The same spelling the journal just used: `PagerError::Contract`
                // carries the machine-readable violation kind (Task 14's HTTP
                // layer forwards this verbatim as the `kind` field of a `502`;
                // it attaches its own human sentence separately rather than
                // this crate inventing one that could drift from the journal's).
                return Err(PagerError::Contract("MissingStats".to_string()));
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

    /// Removes `id` from the agent table entirely: destroys its resident
    /// context if it has one, drops any KV image parked for it, then
    /// forgets it.
    ///
    /// Nothing in the native API (Task 14) needed this — an agent is
    /// suspended, not deleted — but Task 15's `/v1` shim mints an ephemeral
    /// agent for every header-less `/v1/chat/completions` call and must not
    /// let it accumulate in the table forever. Unlike [`Pager::suspend`],
    /// no image is *saved* on the way out: the context is being discarded,
    /// not paged out for a later resume, so persisting an image nobody will
    /// ever `take` back would just be wasted work. No existing [`Event`]
    /// variant fits "agent removed", so this journals nothing new — it is
    /// bookkeeping cleanup, not a paging decision.
    ///
    /// [`Event`]: bloomery_core::journal::Event
    pub fn remove_agent(&mut self, id: &str) -> Result<(), PagerError> {
        if self.table.get(id).is_none() {
            return Err(PagerError::UnknownAgent(id.to_string()));
        }
        self.destroy_context(id)?;
        self.images.drop_image(id);
        self.table.remove(id);
        Ok(())
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
                profiled: m.profile.is_some(),
                kv_per_token: m.kv_per_token,
                training_ctx: m.meta.training_ctx,
            })
            .collect();
        models.sort_by(|x, y| x.name.cmp(&y.name));
        StatusReport {
            free_vram_bytes: (self.free_vram)(),
            resident_kv_bytes: self.resident_kv_bytes(),
            tier: self.tier.clone(),
            posting: self.posting,
            agents,
            models,
        }
    }
}
