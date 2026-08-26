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

mod codec_gate;
mod drift_watch;
mod error;
mod journal;
mod paging;
mod probing;
mod status;
mod task_config;
mod tuning;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use bloomery_core::budget::{Budget, BudgetExhausted};
use bloomery_core::geometry::{kv_bytes_per_token, usable_window, GeometryInput};
use bloomery_core::gguf::GgufMeta;
use bloomery_core::journal::{Journal, PagerOpKind};
use bloomery_core::profile::Profile;
use bloomery_substrate::contract::{enforce_contract, ContractViolation, VerifiedReply};
use bloomery_substrate::{ModelHandle, Substrate};

use crate::agents::{model_digest, Agent, AgentState, AgentTable, ImageStore};
use crate::{config::EnvelopeLens, task::ExecBounds};
use error::sub;
use journal as jrnl;
use status::bound_by_str;

pub use codec_gate::{CodecGateResult, RefusalGateResult};
pub use drift_watch::BlessError;
pub use error::PagerError;
pub use status::{
    AgentInfo, AgentStatus, CodecGateStatus, ModelStatus, RefusalGateStatus, StatusReport,
    TierStatus,
};

/// VRAM held back from the window law for allocator and compute buffers.
/// Zero by default: the pager has not measured this machine's overhead, and
/// an unmeasured term is not silently invented (law 5). The daemon wires the
/// operator's `config.overhead_mib` in via [`Pager::set_overhead_bytes`].
const DEFAULT_OVERHEAD_BYTES: u64 = 0;

/// VRAM reserved per resident context *on top of* its KV cache.
/// Zero by default for the same reason as [`DEFAULT_OVERHEAD_BYTES`]: the
/// pager has measured nothing about this machine and will not invent a
/// number. The daemon wires `config.ctx_overhead_mib` in via
/// [`Pager::set_ctx_overhead_bytes`], and that config key carries the
/// measured rationale.
const DEFAULT_CTX_OVERHEAD_BYTES: u64 = 0;

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

/// Default equal-priority time-sharing quantum (Task 4, spec §2 item 4):
/// how long a qualifying refusal must wait before the pager takes the
/// least-recently-used equal-priority resident anyway. `main.rs` wires the
/// operator's `config.time_share_quantum_secs` in via
/// [`Pager::set_time_share_quantum_ms`].
const DEFAULT_TIME_SHARE_QUANTUM_MS: u64 = 30_000;

/// Source of bloomery's static VRAM budget — see [`Pager::new`].
/// `Send + Sync` from the start: Task 14 shares one pager across request
/// threads behind a lock, and adding the bounds later would be a breaking
/// change to every caller that built one.
pub type FreeVramFn = Box<dyn Fn() -> Option<u64> + Send + Sync>;

/// The pager's one source of "now" — monotonic milliseconds, never wall
/// time. `Pager::new` closes over an `Instant` captured at construction and
/// reports its own elapsed time by default; `set_clock` swaps in a
/// deterministic fake for tests. Every scheduling decision that needs a
/// timestamp (Task 4's time-sharing tiebreak) reads through this closure —
/// see `paging::try_time_share`'s doc comment for the determinism argument
/// this exists to support.
pub type ClockFn = Box<dyn Fn() -> u64 + Send + Sync>;

/// A registered model: its file, geometry, blob identity, optional profile,
/// and the substrate handle once its weights are actually loaded.
struct ModelEntry {
    path: PathBuf,
    meta: GgufMeta,
    digest: String,
    profile: Option<Profile>,
    /// True when `profile` came from this daemon probing **itself** (the
    /// boot POST). Such a profile's ceiling is not a model property — see
    /// the anti-ratchet rule in [`Pager::create_agent`].
    profile_self_measured: bool,
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
    /// True only while a swap-candidate probe is measuring **this** identity —
    /// see [`Pager::open_probe_window`] (`pager::probing`) for the whole
    /// argument. The per-model counterpart of the daemon-global
    /// [`posting`](Pager::posting) flag, and deliberately a field on the entry
    /// rather than a name held on the pager: a window that lives on the
    /// registration cannot outlive it, so the swap job's step-7 unregister
    /// closes it structurally on every path that job can return through.
    probe_window: bool,
    /// This model's completed G4 codec-gate verdict (`Pager::set_codec_gate`,
    /// Task 9's probe driver), or `None` when it has never completed one —
    /// the state `Pager::model_mutating_verbs` reads as fail-closed
    /// read-only (protocol §3/§6). Re-registering a model starts a fresh
    /// entry and so drops any previous gate, matching protocol §6's "restart
    /// re-measures": a model whose weights changed has not been re-probed
    /// under the new file, and carrying the old verdict forward would be
    /// exactly the silent reuse law 5 forbids.
    codec_gate: Option<codec_gate::CodecGateResult>,
    /// This model's completed G5 refusal-honesty gate (`Pager::set_refusal_gate`),
    /// or `None` when never measured — the done-trust source `/status` reads.
    /// Advisory only (unlike `codec_gate`): nothing enforces against it.
    refusal_gate: Option<codec_gate::RefusalGateResult>,
    /// This boot's pair of drift readings (`Pager::set_drift`), or `None` when
    /// the drift watch never ran for this model — a boot where POST failed, or
    /// one before the watch reached this model. Absent is not clean: the whole
    /// point of the drift-watch's named outcomes is that a comparison nobody
    /// made never renders as one that passed (drift-watch design §8). The
    /// cumulative reading is read for enforcement exactly once, at the moment
    /// it settles, to derive `admission_block` below (verdict-gated-admission
    /// design §2/§3); `done_trust` stays the sole property of the G4/G5 gates
    /// (design §7).
    drift: Option<crate::drift::ModelDrift>,
    /// Set when this boot's CUMULATIVE drift comparison settled `Confirmed`
    /// (design §2), cleared by the operator's explicit
    /// `POST /models/{name}/unblock`. While set, `admit` refuses new agents
    /// on this model.
    ///
    /// Separate from `drift` on purpose: the reading is a measurement and
    /// never changes; this is the policy derived from it, and a policy is
    /// the operator's to override.
    admission_block: Option<crate::drift::AdmissionBlock>,
    /// Per-model `n_gpu_layers` override + weights-VRAM ceiling (`pager::tuning`); both `None` -> default.
    n_gpu_layers_override: Option<u32>,
    weights_vram_bytes: Option<u64>,
    /// Task-loop envelope + declared KV-per-token override (`pager::tuning`, protocol §10/§11).
    envelope: EnvelopeLens,
    kv_per_token_bytes: Option<u64>,
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
    /// Per-context runtime reservation; see [`DEFAULT_CTX_OVERHEAD_BYTES`]
    /// and [`Pager::set_ctx_overhead_bytes`].
    ctx_overhead_bytes: u64,
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
    /// The pager's one clock — see [`ClockFn`]. Defaults to an `Instant`
    /// captured at [`Pager::new`]; [`Pager::set_clock`] swaps in a fake for
    /// deterministic tests.
    clock: ClockFn,
    /// How long a qualifying equal-priority refusal must wait before
    /// [`paging::try_time_share`] evicts the LRU resident anyway — see
    /// [`Pager::set_time_share_quantum_ms`].
    time_share_quantum_ms: u64,
    /// `agent id -> clock reading at the FIRST qualifying refusal it hit`
    /// (Task 4). Cleared on any successful placement and on
    /// [`Pager::remove_agent`], so a stale mark from an earlier stand-off
    /// can never make a later, unrelated refusal look like it has already
    /// waited a full quantum.
    waiting_since: HashMap<String, u64>,
    /// `agent id -> clock reading at that agent's most recent "use"`
    /// (Task 4), the LRU tiebreak's ordering key. **Set at two points, by
    /// ruling:** (1) the transition to `Resident` — a successful placement,
    /// whether via `resume` or via `infer` paging an agent in — and (2) a
    /// completed `infer`. Point (1) is what keeps a just-resumed-but-never-
    /// inferred agent from reading as "oldest" at the `unwrap_or(0)` map
    /// default and being picked as the instant victim; point (2) then keeps
    /// it accurate for an agent that goes on to actually do work.
    last_use: HashMap<String, u64>,
    /// The Phase 2b/2c P3 task surface's dark-by-default gate
    /// (`config.tasks_enabled`) — see [`Pager::set_tasks_enabled`].
    tasks_enabled: bool,
    /// The task loop's executor bounds (`config.read_cap_bytes` and
    /// friends) — see [`Pager::set_exec_bounds`].
    exec_bounds: ExecBounds,
    /// Where Task 5's task registry opens a fresh `Journal` handle per task
    /// run — see [`Pager::set_task_journal_path`]. Only read when
    /// `tasks_enabled` is true; an empty default is harmless otherwise.
    task_journal_path: PathBuf,
    /// The profiles directory the drift watch files into
    /// (`config.data_dir/profiles`) — see [`Pager::set_profiles_dir`].
    /// `Option`, unlike [`task_journal_path`](Pager::task_journal_path)'s
    /// empty-path default, because the one thing that reads it *writes* a file:
    /// an unset default that resolved to a relative path would bless a baseline
    /// into whatever directory the daemon happens to be running in, where no
    /// later boot's comparison would ever look for it. `None` is refused by
    /// name instead.
    profiles_dir: Option<PathBuf>,
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
        let start = Instant::now();
        Pager {
            substrate,
            journal,
            images: image_store,
            table: AgentTable::new(),
            models: HashMap::new(),
            free_vram,
            overhead_bytes: DEFAULT_OVERHEAD_BYTES,
            ctx_overhead_bytes: DEFAULT_CTX_OVERHEAD_BYTES,
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
            // The determinism law (Task 4): this closure is the *only*
            // place production code calls `Instant::now()`-derived time for
            // scheduling purposes — everywhere else reads through
            // `self.clock`. `set_clock` replaces it wholesale for tests, so
            // nothing here needs to be swappable piecemeal.
            clock: Box::new(move || u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX)),
            time_share_quantum_ms: DEFAULT_TIME_SHARE_QUANTUM_MS,
            waiting_since: HashMap::new(),
            last_use: HashMap::new(),
            // Dark by default (the P3 plan's binding constraint): a pager
            // nobody has told to enable tasks serves no task surface at
            // all. `main.rs` wires `config.tasks_enabled` in explicitly.
            tasks_enabled: false,
            exec_bounds: ExecBounds::default(),
            task_journal_path: PathBuf::new(),
            profiles_dir: None,
        }
    }

    pub fn set_overhead_bytes(&mut self, bytes: u64) {
        self.overhead_bytes = bytes;
    }

    /// Sets what each resident context reserves beyond its KV cache.
    ///
    /// Applies to agents created *after* this call: `reserved_bytes` is
    /// computed once, at creation, so an agent's reservation cannot change
    /// under a placement decision that already read it.
    pub fn set_ctx_overhead_bytes(&mut self, bytes: u64) {
        self.ctx_overhead_bytes = bytes;
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

    /// Replaces the pager's clock (Task 4). Production code never needs
    /// this — [`Pager::new`]'s default is an `Instant`-backed monotonic
    /// clock — but a deterministic fake lets tests drive the equal-priority
    /// time-sharing tiebreak (`paging::try_time_share`) with an exact,
    /// controllable notion of elapsed time instead of a real wall clock.
    pub fn set_clock(&mut self, clock: ClockFn) {
        self.clock = clock;
    }

    /// Sets the equal-priority time-sharing quantum
    /// (`config.time_share_quantum_secs`, default 30s / 30_000ms) — how
    /// long a qualifying refusal must wait before
    /// `paging::try_time_share` evicts the LRU resident anyway.
    ///
    /// `0` is a valid, if extreme, setting: it degrades the wait to nothing,
    /// so the LRU equal-priority resident is evicted on the very *first*
    /// qualifying refusal rather than after any wait at all — a pure
    /// round-robin escape hatch, not this module's default
    /// wait-one-quantum semantics. Intentional, not a special case in the
    /// implementation: `waited_ms (0) < quantum_ms (0)` is simply false.
    pub fn set_time_share_quantum_ms(&mut self, ms: u64) {
        self.time_share_quantum_ms = ms;
    }

    /// Attaches a measured [`Profile`] to a registered model — POST's whole
    /// output (see [`crate::post`]).
    ///
    /// Agents that already exist keep the window they were quoted at
    /// creation; the profile's ceiling binds every agent created *after*
    /// this. Re-quoting a live agent's window would silently change a number
    /// its caller was already told.
    /// `self_measured` says where the profile came from: `true` for the
    /// boot POST (this daemon probing itself — [`crate::post`]), `false`
    /// for any externally supplied document. It decides one thing only,
    /// the ceiling's effect on geometry — see the anti-ratchet rule in
    /// [`Pager::create_agent`]. Verdicts, and the fact that the model
    /// counts as profiled at all, are kept either way.
    pub fn attach_profile(
        &mut self,
        name: &str,
        profile: Profile,
        self_measured: bool,
    ) -> Result<(), PagerError> {
        let entry = self
            .models
            .get_mut(name)
            .ok_or_else(|| PagerError::UnknownModel(name.to_string()))?;
        entry.profile = Some(profile);
        entry.profile_self_measured = self_measured;
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

    /// Journals one swap-candidate coverage verdict (swap-candidate seam
    /// design §4's row) — same single-writer reason as
    /// [`Pager::journal_post`]: the job runs outside the pager, on a request
    /// thread, and must not open a second writer onto this journal.
    ///
    /// The row is built by `swap::swap_candidate_event` from the reading
    /// itself rather than from arguments spelled out here, so it cannot come
    /// to describe different documents — or different bytes — than the cover
    /// run actually compared. The same discipline (and the same reason)
    /// [`Pager::journal_drift`] follows.
    ///
    /// [`Pager::journal_drift`]: crate::pager::Pager::journal_drift
    pub fn journal_swap_candidate(
        &mut self,
        reading: &crate::swap::CandidateReading,
    ) -> Result<(), PagerError> {
        jrnl::append(
            &mut self.journal,
            &crate::swap::swap_candidate_event(reading),
        )
    }

    /// Read-only view of the substrate, for inspection and tests.
    pub fn substrate(&self) -> &S {
        &self.substrate
    }

    /// The model `agent_id` runs on, or `None` when `agent_id` names no
    /// agent — the same `self.table.get(agent_id)` lookup
    /// [`Pager::agent_task_policy`] and [`Pager::agent_budget_granted`] key
    /// off, so all three answer `None` under exactly one condition.
    ///
    /// The memory organ's one caller (`task::registry`'s worker) records
    /// this as a minted episode's `minted_by_model`
    /// (`docs/superpowers/specs/2026-08-26-memory-organ-design.md` §2):
    /// provenance that is **recorded, never compared** — retrieval gates on
    /// the goal hash, the cited fingerprints and the grant, never on which
    /// model minted the episode, so an episode minted by one model is
    /// injectable into a task running on another (that model-agnosticism is
    /// crucible's GATE-B finding, and the reason the field is provenance
    /// rather than a key).
    pub fn agent_model(&self, agent_id: &str) -> Option<String> {
        self.table.get(agent_id).map(|a| a.model.clone())
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
                // A profile handed to `register_model` came from outside
                // this daemon (an operator, an externally validated
                // document), so it keeps the clamping behavior. Only
                // `attach_profile` can mark one self-measured.
                profile_self_measured: false,
                handle: None,
                provisional_logged: false,
                unprofiled_logged: false,
                // Closed. A fresh registration is never mid-probe, and
                // re-registering a name drops any window the old entry held —
                // the same "it is a new entry" rule the two `_logged` flags
                // above follow.
                probe_window: false,
                codec_gate: None,
                refusal_gate: None,
                drift: None,
                admission_block: None,
                n_gpu_layers_override: None,
                weights_vram_bytes: None,
                envelope: EnvelopeLens::V1,
                kv_per_token_bytes: None,
            },
        );
        Ok(())
    }

    /// Creates an agent and computes its window. No VRAM is committed: the
    /// agent starts `Fresh` and only becomes resident when it first infers.
    ///
    /// Admission is profile-gated (law 5) — see [`Pager::admit`].
    ///
    /// **Anti-ratchet rule.** A ceiling measured by this daemon probing
    /// itself is skipped by the window law. The prompt gate in
    /// [`Pager::infer`] already applies a conservative estimate at request
    /// time; a self-probe hits that gate and records *its* limit as the
    /// model's ceiling, so feeding that number back into the geometry
    /// applies the same conservatism twice — and every re-probe would then
    /// measure a lower ceiling than the last, ratcheting the window down.
    /// (Measured live on 2026-08-14: a 32 768-token window self-probed to
    /// `max_verified: 14336`, which would have halved it.) Externally
    /// measured ceilings still bind: they are the model's property, not
    /// ours.
    pub fn create_agent(
        &mut self,
        model: &str,
        priority: u8,
        window_cap: Option<u32>,
        budget_tokens: u64,
    ) -> Result<AgentInfo, PagerError> {
        let (kv_per_token, training_ctx, weights_bytes, measured_ceiling, recurrent_state_bytes) = {
            let entry = self
                .models
                .get(model)
                .ok_or_else(|| PagerError::UnknownModel(model.to_string()))?;
            (
                entry.effective_kv_per_token(), // Part B, spec §10 addendum: see `pager::tuning`
                entry.meta.training_ctx,
                entry.effective_weights_bytes(), // Task 3: see `pager::tuning`
                // The anti-ratchet rule, in one expression: a self-probe
                // measures our own refusal gate, so clamping by it would
                // ratchet the window down on every re-probe.
                entry
                    .profile
                    .as_ref()
                    .filter(|_| !entry.profile_self_measured)
                    .and_then(|p| p.measured_ceiling()),
                entry.recurrent_state_bytes(), // turn-5 spec §2: see `pager::tuning`
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
            ctx_overhead_bytes: self
                .ctx_overhead_bytes
                .saturating_add(recurrent_state_bytes),
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
        // The KV cache alone (`reserved_bytes` below is what residency plans against).
        let kv_bytes = self.kv_reservation_bytes(model, window.tokens);
        self.table.insert(Agent {
            id,
            model: model.to_string(),
            priority,
            kv_bytes,
            reserved_bytes: kv_bytes
                .saturating_add(self.ctx_overhead_bytes)
                .saturating_add(recurrent_state_bytes),
            window,
            budget: Budget::new(budget_tokens),
            state: AgentState::Fresh,
        });
        Ok(info)
    }

    /// Law 5's gate: no model gets work without a measured capability
    /// profile — and the drift-watch design §2's standing block, checked
    /// first.
    ///
    /// Zero: a standing [`crate::drift::AdmissionBlock`] refuses outright,
    /// as [`PagerError::DriftBlocked`], before the existence check below
    /// ever runs — see the note on that check for why the order matters.
    ///
    /// Otherwise, four things can still admit an unprofiled model, and
    /// they are not interchangeable:
    ///
    /// 1. **`posting`** — the bounded window while POST probes this daemon,
    ///    which is the only way a profile can ever come to exist
    ///    ([`crate::post`]). Journaled per model.
    /// 2. **This entry's own [`probe_window`](ModelEntry::probe_window)** —
    ///    the same suspension, for the same chicken-and-egg, scoped to one
    ///    identity: a swap candidate registered under a scratch name is
    ///    probed through this daemon's own `/v1` and has no profile until
    ///    that probe writes one (`pager::probing`). Journaled per model.
    /// 3. **`allow_unprofiled`** — the operator's explicit override.
    ///    Journaled per model, naming it, because the daemon is then serving
    ///    a model whose real ceiling and codecs nobody measured.
    /// 4. None of them: refused as [`PagerError::Unprofiled`], with the
    ///    model's name, which the HTTP layers render as `422`.
    ///
    /// An unprofiled model is never *silently* admitted on any path.
    ///
    /// The gate is at agent **creation**, not per inference, for both
    /// refusals above: an agent admitted inside the POST window keeps
    /// working after the window closes, and an agent admitted before a
    /// drift block appeared keeps working after the block lands. Cutting a
    /// live conversation off mid-turn because POST finished, or because the
    /// watch settled, would be its own dishonesty. New work on a
    /// still-unprofiled or now-blocked model is refused.
    ///
    /// The window is not small: one `--quick` probe measured ~110 s per
    /// model (enthusiast-16GB tier, 2026-08-14), models are probed
    /// sequentially, so it lasts roughly that sum — minutes on a
    /// multi-model daemon. Anything admitted in that span outlives it.
    fn admit(&mut self, model: &str) -> Result<(), PagerError> {
        // Design §2. Checked before the existence gate so a blocked model
        // reports the reason that actually applies: it HAS a profile, and
        // that is precisely why a regression against it could be measured.
        if let Some(block) = self
            .models
            .get(model)
            .and_then(|e| e.admission_block.as_ref())
        {
            return Err(PagerError::DriftBlocked {
                model: model.to_string(),
                reference: block.reference.clone(),
            });
        }
        let has_profile = self
            .models
            .get(model)
            .is_some_and(|entry| entry.profile.is_some());
        if has_profile {
            return Ok(());
        }
        let posting = self.posting;
        let probing = self
            .models
            .get(model)
            .is_some_and(|entry| entry.probe_window);
        if !posting && !probing && !self.allow_unprofiled {
            return Err(PagerError::Unprofiled(model.to_string()));
        }
        let first_time = match self.models.get_mut(model) {
            None => false,
            Some(entry) => {
                let said = if posting || probing {
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
            } else if probing {
                format!(
                    "provisional admission: {model} has no profile yet; a candidate probe \
                     is measuring it"
                )
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
    /// charged to nobody's budget. `stop` (protocol §11) reaches `Substrate::infer` as-is.
    pub fn infer(
        &mut self,
        id: &str,
        prompt: &str,
        max_tokens: u32,
        stop: Option<&str>,
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
        let reply = match self.substrate.infer(ctx, prompt, max_tokens, stop) {
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
        // The second of `last_use`'s two write points (see the field doc):
        // a completed inference. `ensure_resident` (in `paging`) already
        // recorded the first write when this agent was placed, moments ago
        // in this same call — this one supersedes it with the more precise
        // "actually did work" timestamp. Read through the pager's own
        // clock (never `Instant::now()` ad hoc) so the eviction decision
        // built from it stays deterministic under a fake clock.
        self.last_use.insert(id.to_string(), (self.clock)());
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

    /// Unloads `name`'s weights and forgets its registration entirely — the
    /// exact inverse of [`Pager::register_model`].
    ///
    /// Exists for the swap-candidate seam (design §4: "the scratch identity
    /// never outlives the request"), which registers a candidate GGUF under a
    /// scratch name so assay can probe it through `/v1`, then must leave the
    /// registry exactly as it found it. The weights go back through
    /// [`Pager::unload_model`] — the same call `POST /models/{name}/unload`
    /// makes, so a scratch model's bytes are credited back by the one path
    /// that already knows how, including suspending anything still holding a
    /// context on it.
    ///
    /// **The registration survives an unload that failed**, deliberately. The
    /// placement budget subtracts every *loaded* model's weights
    /// (`loaded_weights_bytes`), so forgetting an entry whose weights the
    /// substrate still holds would silently under-count the pool by exactly
    /// those bytes — law 1's pre-checked memory pressure, quietly wrong. A
    /// named error with the entry still standing is the honest failure.
    ///
    /// **Every agent bound to `name` is evicted, not merely suspended** —
    /// [`Pager::remove_agent`], id, context and image, before the unload.
    ///
    /// Suspending them is not enough, and the reason is specific to what this
    /// call is for. A suspended agent keeps its `model` *string*, and its next
    /// request is refused only because no model of that name is registered any
    /// more (`paging::place`'s `UnknownModel`). For a scratch identity that
    /// refusal is temporary by construction: the next candidate job for the
    /// same model registers exactly that name again, and law 5 is checked at
    /// agent **creation**, never per inference — so a survivor would come back
    /// usable, against a *different* candidate's weights, without passing any
    /// gate at all. Design §4's "the scratch identity never outlives the
    /// request" has to mean the agents on it too, or it means nothing.
    ///
    /// **Before the unload, not after**, for two reasons. [`Pager::suspend`]
    /// saves a KV image on the way out, and [`Pager::remove_agent`] would drop
    /// it again a moment later — the wasted work that method's doc exists to
    /// avoid. And an eviction that has already emptied the table leaves
    /// [`Pager::unload_model`]'s suspend loop nothing that can fail.
    ///
    /// An unknown model is refused before anything is evicted, so a refused
    /// unregister still changes nothing.
    pub fn unregister_model(&mut self, name: &str) -> Result<(), PagerError> {
        if !self.models.contains_key(name) {
            return Err(PagerError::UnknownModel(name.to_string()));
        }
        let bound: Vec<String> = self
            .table
            .iter()
            .filter(|a| a.model == name)
            .map(|a| a.id.clone())
            .collect();
        for id in bound {
            self.remove_agent(
                &id,
                &format!(
                    "{name} was unregistered; an agent bound to it cannot outlive the \
                     registration, because re-registering that name would revive it"
                ),
            )?;
        }
        self.unload_model(name)?;
        self.models.remove(name);
        Ok(())
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
    /// ever `take` back would just be wasted work. `reason` is journaled
    /// verbatim via [`Event::AgentRemoved`] on the successful path only —
    /// a refused removal (unknown id) leaves nothing to explain.
    ///
    /// [`Event`]: bloomery_core::journal::Event
    pub fn remove_agent(&mut self, id: &str, reason: &str) -> Result<(), PagerError> {
        if self.table.get(id).is_none() {
            return Err(PagerError::UnknownAgent(id.to_string()));
        }
        self.destroy_context(id)?;
        self.images.drop_image(id);
        self.table.remove(id);
        // Task 4: a removed agent can never be "waiting" or have a
        // "last use" again — an id is never reused (monotonic
        // `next_agent_seq`), so leaving either mark behind would just be a
        // permanent, meaningless entry in both maps.
        self.waiting_since.remove(id);
        self.last_use.remove(id);
        jrnl::agent_removed(&mut self.journal, id, reason)?;
        Ok(())
    }
}
