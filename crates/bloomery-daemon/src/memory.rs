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

use std::path::Path;
use std::sync::{Arc, Mutex};

use crate::config::MemoryConfig;

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
    /// The `[memory] refalsify` opt-in (refalsify-on-exact spec
    /// `docs/superpowers/specs/2026-08-27-refalsify-on-exact-design.md` —
    /// activation §5, mechanism §2; default `false`): whether the worker
    /// probes a retrieved episode's stored run command under the incoming
    /// task's grant, `cwd` and `ExecBounds` before injecting, and
    /// contradicts the episode on a clean nonzero exit.
    /// Meaningful only for an [`operational`](Self::operational) organ — the
    /// flag never turns an organ on, it only changes what an already-on
    /// organ does before it speaks.
    pub refalsify: bool,
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

/// Builds the daemon's live [`MemoryContext`] from config at boot
/// (memory-organ design §6/§7) — `main.rs`'s one call site, the same "a
/// config value takes effect exactly once, at boot" discipline every other
/// `main.rs` wiring line follows.
///
/// The store path is `<data_dir>/memory/episodes.jsonl` (§6). A load
/// failure — an unreadable file, or a line that is valid UTF-8 but fails to
/// even iterate as lines (both surface as a hard `io::Error` from
/// [`store::MemoryStore::load`], as distinct from a single corrupt JSON
/// *line*, which `load` already counts and skips rather than erroring on) —
/// becomes `disabled_reason: Some(..)` with `store: None`, and **boot
/// proceeds**: this is the system-level half of spec §7's "the organ being
/// broken can only ever produce memory-off behavior — never a failed task".
/// A daemon that refused to boot because its *advisory* memory store was
/// unreadable would have let the organ take down every other capability
/// with it.
///
/// Built **unconditionally**, even when `cfg.enabled` is `false`: the
/// resulting counts must stay renderable at `/status` for an operator who
/// wants to see what a disabled organ would have (spec §6's operator
/// surface), and [`MemoryContext::operational`] — not this function — is
/// the single gate every caller checks before retrieving, minting, or
/// contradicting.
pub fn build_memory(cfg: &MemoryConfig, data_dir: &Path) -> Arc<MemoryContext> {
    let store_path = data_dir.join("memory").join("episodes.jsonl");
    match store::MemoryStore::load(&store_path) {
        Ok(store) => Arc::new(MemoryContext {
            enabled: cfg.enabled,
            max_episodes: cfg.max_episodes,
            refalsify: cfg.refalsify,
            disabled_reason: None,
            store: Some(Mutex::new(store)),
        }),
        Err(e) => Arc::new(MemoryContext {
            enabled: cfg.enabled,
            max_episodes: cfg.max_episodes,
            refalsify: cfg.refalsify,
            disabled_reason: Some(format!("memory store unreadable: {e}")),
            store: None,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh, per-test tempdir — PID + atomic counter so parallel test
    /// threads in one `cargo test` process never collide. Same shape as
    /// `memory_task_test.rs`'s own `fresh_dir` helper.
    fn fresh_dir(tag: &str) -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static UNIQUE: AtomicU64 = AtomicU64::new(0);
        let unique = UNIQUE.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "bloomery-memory-ctx-test-{tag}-{}-{unique}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Task 7 left this conjunct untested: `enabled` alone does not make an
    /// organ operational once its store failed to load — the
    /// `disabled_reason` state this task's `build_memory` produces.
    /// `operational()` is the single gate every caller uses instead of
    /// reading `enabled` directly (spec §7), so this conjunct has to hold on
    /// its own, not just as a side effect of some other test's setup.
    #[test]
    fn operational_is_false_when_enabled_but_store_is_none() {
        let ctx = MemoryContext {
            enabled: true,
            max_episodes: 256,
            refalsify: false,
            disabled_reason: Some("memory store unreadable: test".to_string()),
            store: None,
        };
        assert!(!ctx.operational());
    }

    #[test]
    fn build_memory_on_a_fresh_dir_is_operational_and_creates_nothing_until_mint() {
        let dir = fresh_dir("fresh");
        let cfg = MemoryConfig {
            enabled: true,
            max_episodes: 256,
            refalsify: false,
        };
        let ctx = build_memory(&cfg, &dir);
        assert!(
            ctx.operational(),
            "disabled_reason: {:?}",
            ctx.disabled_reason
        );
        assert_eq!(ctx.disabled_reason, None);
        assert!(
            !dir.join("memory").join("episodes.jsonl").exists(),
            "loading a fresh dir must not create the store file — only a mint does"
        );
    }

    /// `cfg.enabled = false` still builds a context whose counts are
    /// renderable — `operational()`, not construction, is the gate.
    #[test]
    fn build_memory_disabled_by_config_is_still_built_with_zero_counts() {
        let dir = fresh_dir("config-off");
        let cfg = MemoryConfig {
            enabled: false,
            max_episodes: 256,
            refalsify: false,
        };
        let ctx = build_memory(&cfg, &dir);
        assert!(!ctx.enabled);
        assert!(!ctx.operational());
        assert_eq!(ctx.disabled_reason, None);
        let counts = ctx
            .store
            .as_ref()
            .expect("a config-off context still loads a store")
            .lock()
            .unwrap()
            .counts();
        assert_eq!(counts.episodes, 0);
    }

    /// The system-level boot-safety property both T2's store doc and spec §7
    /// require: an unreadable store path degrades to `disabled_reason`
    /// rather than failing boot. Forcing the store PATH to be a directory
    /// (rather than a corrupt line inside a real file) reaches this through
    /// `MemoryStore::load`'s hard `io::Error` arm — `File::open` on a
    /// directory succeeds on Linux, but the first `read` fails with
    /// `EISDIR`, which `load`'s `line?` propagates as exactly the same kind
    /// of `io::Error` an unreadable-permissions file would.
    #[test]
    fn build_memory_disabled_reason_when_store_path_is_a_directory() {
        let dir = fresh_dir("unreadable");
        std::fs::create_dir_all(dir.join("memory").join("episodes.jsonl")).unwrap();
        let cfg = MemoryConfig {
            enabled: true,
            max_episodes: 256,
            refalsify: false,
        };
        let ctx = build_memory(&cfg, &dir);
        assert!(!ctx.operational());
        assert!(ctx.store.is_none());
        let reason = ctx.disabled_reason.as_deref().unwrap_or("");
        assert!(
            reason.starts_with("memory store unreadable: "),
            "reason: {reason:?}"
        );
    }
}
