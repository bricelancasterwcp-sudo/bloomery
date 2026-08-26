//! The memory organ (spec
//! `docs/superpowers/specs/2026-08-26-memory-organ-design.md`): an
//! exact-repeat episodic store with grant-gated injection.
//!
//! The submodules are the organ's four moving parts — [`record`] (identity
//! and the stored row), [`store`] (the event-sourced JSONL), [`retrieve`]
//! (the two-stage exact match) and [`mint`]/[`render`] (the bar, and the
//! block a matched episode renders into). This module itself holds only what
//! ties them to the running daemon: the per-daemon [`MemoryContext`] handle
//! the task worker consumes, and the one injection-size policy constant.

pub mod mint;
pub mod record;
pub mod render;
pub mod retrieve;
pub mod store;

/// The largest rendered memory block that may be injected into a task's
/// prompt, in bytes.
///
/// **Controller ruling (Task 6, carried into Task 7's pipeline): an
/// oversized block is skipped, and the task is stamped `"silent"`.** The
/// organ is advisory, and spec §7's headline is that it "can only ever
/// produce memory-off behavior — never a wrong injection, never a failed
/// task". A memory block is prepended to a prompt whose size is already
/// bounded by the model's measured window, so a large enough block could
/// push a task into `TaskStatus::WindowExhausted` that would have completed
/// memory-off. That is the organ damaging the task, which no amount of
/// retrieval value buys back — so past this bound the organ declines to
/// speak rather than risk it.
///
/// The value is a constant rather than config for the same reason spec §6
/// makes the single-injection cap one: it is a property of what an injection
/// may cost a task, not a per-operator tuning knob. 16 KiB is comfortably
/// above any block a real episode renders (the block is patch bodies plus
/// one verification line) while staying a small fraction of even a modest
/// context window.
pub const MEMORY_BLOCK_MAX_BYTES: usize = 16384;

/// The daemon's live handle on the organ: the config switch, the retention
/// cap, and the single-mutex store (spec §6's concurrency note — "the store
/// lives behind a single mutex and has no background writer").
///
/// **`store` is `None` exactly when `disabled_reason` is `Some`.** Spec §7
/// requires that a store which could not be read at boot leaves the organ
/// "disabled-with-reason" and every task running memory-off; carrying the
/// reason beside an absent store — rather than an empty store and no reason
/// — is what keeps "the store is empty" (first boot, an honest zero)
/// distinguishable from "the store could not be read" (a failure an operator
/// must see at `/status`).
pub struct MemoryContext {
    /// The `[memory] enabled` config switch (spec §6, default `false`).
    pub enabled: bool,
    /// The `[memory] max_episodes` retention cap on distinct ids (spec §6),
    /// passed to [`store::MemoryStore::mint`] at every mint.
    pub max_episodes: usize,
    /// Why the organ is off despite `enabled`, when it is — the store was
    /// unreadable at boot (spec §7). Surfaced at `/status`, never inferred.
    pub disabled_reason: Option<String>,
    /// The store, or `None` when it could not be loaded — see the struct's
    /// own doc comment for the invariant this pairs with.
    pub store: Option<std::sync::Mutex<store::MemoryStore>>,
}

impl MemoryContext {
    /// Whether this organ may retrieve, mint or contradict for a task: the
    /// operator turned it on **and** there is a store to talk to. Every
    /// caller gates on this rather than on `enabled` alone, so a boot that
    /// lost its store behaves exactly like a boot with the switch off (spec
    /// §7).
    pub fn operational(&self) -> bool {
        self.enabled && self.store.is_some()
    }
}
