//! Daemon configuration.
//!
//! `load_config` parses a TOML file into [`Config`], applying defaults for
//! every field that has one. `data_dir`, `models`, `tier`, and `assay` have
//! no sane default — an operator must say where state lives and which
//! models exist — so a missing `models` table (or any other required field)
//! fails with serde's own "missing field `<name>`" error, which already
//! names the field, rather than a generic parse failure.

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

fn default_python() -> String {
    "python3".to_string()
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
    #[serde(default = "default_priority")]
    pub default_priority: u8,
    #[serde(default = "default_budget_tokens")]
    pub default_budget_tokens: u64,
    #[serde(default)]
    pub allow_unprofiled: bool,
    pub assay: AssayConfig,
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
