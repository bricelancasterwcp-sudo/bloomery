//! Daemon configuration.
//!
//! `load_config` parses a TOML file into [`Config`], applying defaults for
//! every field that has one. `data_dir`, `models`, `tier`, and `assay` have
//! no sane default — an operator must say where state lives and which
//! models exist — so a missing `models` table (or any other required field)
//! fails with serde's own "missing field `<name>`" error, which already
//! names the field, rather than a generic parse failure.
//!
//! Unknown TOML keys are intentionally ignored (no
//! `#[serde(deny_unknown_fields)]`): a config carrying a key this daemon
//! build doesn't recognize yet (forward-compatibility, operator scratch
//! notes, a newer schema field) should not hard-fail an older build.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn default_port() -> u16 {
    8181
}

fn default_overhead_mib() -> u64 {
    1024
}

fn default_priority() -> u8 {
    100
}

fn default_budget_tokens() -> u64 {
    200_000
}

/// Task 4's equal-priority time-sharing quantum, in seconds — how long a
/// qualifying refusal waits before the pager takes the LRU equal-priority
/// resident anyway. Matches `pager::DEFAULT_TIME_SHARE_QUANTUM_MS` (30s);
/// `main.rs` wires this through `Pager::set_time_share_quantum_ms` as
/// milliseconds.
fn default_time_share_quantum_secs() -> u64 {
    30
}

/// What each resident context reserves beyond its KV cache, in MiB.
///
/// **This default is derived from a measured floor; the active value is
/// configured, not measured per-run.** bloomery never reads this number back
/// from the substrate — see the honest limit in the README and carried-debt
/// item 7. The derivation is committed as
/// `docs/superpowers/evidence/2026-08-14-2a-daemon-log-excerpt.txt`: the
/// 2026-08-14 natural-pressure run's `daemon.log` recorded, for every
/// `n_ctx = 16384` context of qwen2.5-coder-7b-q8_0 on a Vulkan RTX 5080:
///
/// ```text
/// sched_reserve:    Vulkan0 compute buffer size =   304.00 MiB
/// sched_reserve: Vulkan_Host compute buffer size =    30.01 MiB
/// ```
///
/// against an 896 MiB KV cache. Charging the KV alone let the pager plan a
/// sixth resident where five fit; the sixth allocation returned
/// `ErrorOutOfDeviceMemory` and the run died. 384 MiB sits above the
/// observed 334 with room for a device whose buffers are larger, and an
/// operator who has measured their own may lower it.
///
/// **Item 7 closed 2026-08-15**: `usable_window`'s VRAM term now subtracts
/// this value too (`GeometryInput::ctx_overhead_bytes`), alongside `weights`
/// and `overhead_mib` — see
/// `docs/superpowers/specs/2026-08-15-partial-offload-capability-window-design.md`
/// §3b and `docs/CARRIED-DEBT.md` item 7's delivery note. A window that
/// comes out VRAM-bound is placeable by construction now, for a single
/// agent; the multi-model divergence item 7 also named remains open.
fn default_ctx_overhead_mib() -> u64 {
    384
}

/// `pub(crate)` so `post.rs` builds its test runner against the same
/// spelling this config defaults to, rather than a second literal that
/// could drift from it.
pub(crate) fn default_python() -> String {
    "python3".to_string()
}

/// Matches `bloomery_daemon::task::ExecBounds::default`'s
/// `read_cap_bytes` (256 KiB) — see that impl's doc comment for why the
/// number lives in one place and these serde defaults just mirror it.
fn default_read_cap_bytes() -> usize {
    256 * 1024
}

fn default_find_result_cap() -> usize {
    100
}

fn default_run_output_cap_bytes() -> usize {
    64 * 1024
}

fn default_run_timeout_secs() -> u64 {
    120
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct Config {
    #[serde(default = "default_port")]
    pub port: u16,
    /// Root of daemon state: `journal/`, `profiles/`, `images/`.
    pub data_dir: PathBuf,
    /// Model name -> path to its `.gguf` file.
    pub models: BTreeMap<String, PathBuf>,
    pub tier: Tier,
    #[serde(default = "default_overhead_mib")]
    pub overhead_mib: u64,
    #[serde(default = "default_ctx_overhead_mib")]
    pub ctx_overhead_mib: u64,
    #[serde(default = "default_priority")]
    pub default_priority: u8,
    #[serde(default = "default_budget_tokens")]
    pub default_budget_tokens: u64,
    #[serde(default)]
    pub allow_unprofiled: bool,
    #[serde(default = "default_time_share_quantum_secs")]
    pub time_share_quantum_secs: u64,
    pub assay: AssayConfig,
    /// Dark by default (Phase 2b/2c P3's binding constraint): the task HTTP
    /// surface (`POST`/`GET /agents/{id}/task`) answers `501
    /// tasks_disabled` on every request until an operator sets this `true`.
    #[serde(default)]
    pub tasks_enabled: bool,
    /// Max bytes a single `read` action returns.
    #[serde(default = "default_read_cap_bytes")]
    pub read_cap_bytes: usize,
    /// Max matches a single `find` action returns.
    #[serde(default = "default_find_result_cap")]
    pub find_result_cap: usize,
    /// Max bytes a single `run` action's captured output returns.
    #[serde(default = "default_run_output_cap_bytes")]
    pub run_output_cap_bytes: usize,
    /// Max wall-clock seconds a `run` action's subprocess gets.
    #[serde(default = "default_run_timeout_secs")]
    pub run_timeout_secs: u64,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct Tier {
    pub name: String,
    pub emulated: bool,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct AssayConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_python")]
    pub python: String,
}

/// Parses `path` as TOML into a [`Config`], applying defaults.
///
/// Errors are named: parse/deserialize failures surface serde's own
/// "missing field `<name>`" / "invalid type" messages via `toml`'s
/// `Display` impl, so a caller can point an operator straight at what's
/// wrong with their config rather than a bare "failed to parse".
pub fn load_config(path: &Path) -> Result<Config, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("failed to read config {}: {e}", path.display()))?;
    toml::from_str(&text).map_err(|e| format!("failed to parse config {}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_python_is_python3() {
        assert_eq!(default_python(), "python3");
    }
}
