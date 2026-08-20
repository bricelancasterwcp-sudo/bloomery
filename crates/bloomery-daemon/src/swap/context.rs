//! What the HTTP surface hands one candidate job: the collaborators
//! [`super::run_candidate_probe`] takes, the port and tier the probe runs
//! against, and the one slot the whole daemon shares.
//!
//! This exists because of a deliberate property of [`crate::post::CommandRunner`]:
//! it is **not `Send`**, on purpose ("a `PostRunner` is built *inside* the
//! thread that runs POST, never shipped across one" — `post.rs`). The
//! swap-candidate worker is the same shape: a request thread spawns it, so
//! nothing built from a `CommandRunner` can be carried into it. What crosses
//! the thread boundary is therefore a **factory** ([`ProbeFactory`]) that
//! builds both subprocess seams on the worker's own thread — exactly what
//! `main.rs` already does for POST's runner, promoted from a closure body to a
//! named field so tests can inject scripted probes through the same seam.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU16, Ordering};
use std::time::Duration;

use super::{CoverGate, SwapSlot};
use crate::config::Tier;
use crate::drift::ProfileStore;
use crate::post::PostRunner;

/// The two subprocess seams one candidate job drives: assay's probe (step 3)
/// and assay's cover (step 5). Built together because they must share one
/// interpreter — spec §4's "the gate's interpreter is the probe's
/// interpreter", which a pair built from two separate configurations could
/// quietly break.
pub struct SwapProbes {
    pub runner: PostRunner,
    pub gate: CoverGate,
}

/// How a [`SwapContext`] builds its [`SwapProbes`], called **on the worker
/// thread** — see this module's docs for why nothing here can be built ahead
/// of time and carried across.
pub type ProbeFactory = Box<dyn Fn() -> SwapProbes + Send + Sync>;

/// Everything the swap-candidate routes need beside the pager mutex.
///
/// Shared by every HTTP worker as one `Arc<SwapContext>`, so the slot they all
/// read is the slot the worker thread writes — there is exactly one candidate
/// job per daemon (design §4: "One candidate at a time"), and that bound is
/// only real if every request sees the same slot.
pub struct SwapContext {
    probes: ProbeFactory,
    store: ProfileStore,
    tier: Tier,
    /// This daemon's own bound port, filled in by
    /// [`crate::http::serve_shared_with_swap`] the moment the socket is bound
    /// and before any worker is spawned — the probe reaches the candidate
    /// through this daemon's own `/v1`, so a guessed or configured-but-unbound
    /// port would probe nothing (`config.port = 0` lets the OS pick).
    port: AtomicU16,
    slot: SwapSlot,
}

impl SwapContext {
    /// The real daemon's: `{python} -m assay probe …` and `{python} -m assay
    /// cover …`, both under `config.assay.python`.
    pub fn new(
        python: String,
        probe_timeout: Duration,
        store: ProfileStore,
        tier: Tier,
    ) -> SwapContext {
        SwapContext::with_probes(
            Box::new(move || SwapProbes {
                runner: PostRunner::new(python.clone(), probe_timeout),
                gate: CoverGate::new(python.clone()),
            }),
            store,
            tier,
        )
    }

    /// A context whose subprocesses are injected — the whole endpoint driven
    /// with no python, no assay and no GPU, through the same seam
    /// `swap_test.rs` drives the job itself through.
    pub fn with_probes(probes: ProbeFactory, store: ProfileStore, tier: Tier) -> SwapContext {
        SwapContext {
            probes,
            store,
            tier,
            port: AtomicU16::new(0),
            slot: SwapSlot::default(),
        }
    }

    /// Builds this job's pair of subprocess seams. Call this **on the thread
    /// that will run the job** — that is the whole point of the factory.
    pub fn probes(&self) -> SwapProbes {
        (self.probes)()
    }

    pub fn store(&self) -> &ProfileStore {
        &self.store
    }

    pub fn tier(&self) -> &Tier {
        &self.tier
    }

    /// The one slot this daemon's candidate jobs claim, in turn.
    pub fn slot(&self) -> &SwapSlot {
        &self.slot
    }

    /// The port the probe addresses this daemon on — see [`SwapContext::port`].
    /// `0` until the socket is bound, which no request can observe: the server
    /// sets it before the first worker exists.
    pub fn port(&self) -> u16 {
        self.port.load(Ordering::Relaxed)
    }

    /// Records the bound port. `http::serve_shared_with_swap` is the only
    /// caller — the port is not knowable until `tiny_http` has bound.
    pub fn set_port(&self, port: u16) {
        self.port.store(port, Ordering::Relaxed);
    }

    /// The blessed baseline a candidate for `model` would be measured against
    /// — the floor whose existence is the endpoint's own precondition, read
    /// here rather than respelled at the route so the path the refusal names
    /// is the path the job would really read.
    pub fn floor(&self, model: &str) -> PathBuf {
        self.store.paths(model).baseline
    }

    /// Where the candidate's document is written before it is retained — named
    /// here for the one report the *spawn site* has to build itself (a caught
    /// panic), so it names the same document the job would have.
    pub fn staging(&self, model: &str) -> PathBuf {
        self.store.confirm_staging(&super::scratch_identity(model))
    }
}
