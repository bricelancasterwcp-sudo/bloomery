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

/// The four task-loop prompt envelopes a model can be configured for
/// (`docs/superpowers/evidence/2026-08-15-g4-protocol.md` §10/§11,
/// Amendments 2 and 3; turn-4 spec §2 for `V4`).
///
/// - `V1`: the raw completion prompt, no pre-seed, no stop sequence
///   (`bloomery-task-envelope-v1`).
/// - `V2`: `V1` plus the literal `<think>\n\n</think>\n\n` pre-seed appended
///   to the rendered prompt (`bloomery-task-envelope-v2`, Amendment 2).
/// - `V3`: `V2` plus a stop sequence at the first `</action>` occurrence in
///   the completion (`bloomery-task-envelope-v3`, Amendment 3) — the
///   law-3 ruling: termination, not constraint.
/// - `V4`: `V3` plus the grant line — one line rendered from the task's real
///   [`bloomery_core::grant::Grant`], between the goal and the verb card
///   (`bloomery-task-envelope-v4`, `docs/superpowers/specs/2026-08-21-flywheel4-turn4-design.md`
///   §2). Its reason: before it, a run-granted task and a plain one were
///   token-indistinguishable at the post-patch decision point, so training
///   on both collapsed to the majority label (§1). `V4` inherits everything
///   `V3` does — the pre-seed AND the stop — and adds only that line.
///
/// `Default` is `V1` — an unconfigured model gets today's behavior,
/// byte-for-byte. Every earlier lens is byte-frozen: `V1`/`V2`/`V3` render
/// exactly what they rendered before `V4` existed (pinned by
/// `tests/task_render_test.rs`'s goldens), because every G4/G5 verdict the
/// ledger carries was measured against those bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EnvelopeLens {
    #[default]
    V1,
    V2,
    V3,
    V4,
    V5,
}

impl EnvelopeLens {
    /// The stable spelling every `CodecVerdict`/`/status` records — protocol
    /// §11: "the lens name travels ... from the same config read that
    /// drives the behavior" (Amendment 2's one-source rule, restated for
    /// v3). `const fn` so `codec_probe`'s pinned string constants can be
    /// derived from this single source rather than retyped.
    pub const fn lens_name(&self) -> &'static str {
        match self {
            EnvelopeLens::V1 => "bloomery-task-envelope-v1",
            EnvelopeLens::V2 => "bloomery-task-envelope-v2",
            EnvelopeLens::V3 => "bloomery-task-envelope-v3",
            EnvelopeLens::V4 => "bloomery-task-envelope-v4",
            EnvelopeLens::V5 => "bloomery-task-envelope-v5",
        }
    }

    /// Whether this lens appends the `THINK_PRESEED` literal
    /// (`task::task_loop::THINK_PRESEED`) to the rendered prompt — `V2`,
    /// `V3` and `V4` all do (`V3` = `V2` + the action stop; `V4` = `V3` +
    /// the grant line).
    pub const fn think_preseed(&self) -> bool {
        matches!(
            self,
            EnvelopeLens::V2 | EnvelopeLens::V3 | EnvelopeLens::V4 | EnvelopeLens::V5
        )
    }

    /// Whether this lens stops task-loop generation at the first
    /// `</action>` occurrence (protocol §11, Amendment 3) — `V3`, and `V4`
    /// which is defined as `V3` plus the grant line (turn-4 spec §2: v4's
    /// only delta is the rendered line, so dropping the stop here would be
    /// a silent regression against the lens turn 4 succeeds).
    pub const fn action_stop(&self) -> bool {
        matches!(self, EnvelopeLens::V3 | EnvelopeLens::V4 | EnvelopeLens::V5)
    }

    /// Whether this lens renders the grant line
    /// (`task::grant_line::grant_line`) between the goal and the verb card
    /// — `V4` only (turn-4 spec §2). The one predicate `render_prompt_from`
    /// branches on, so "which lenses show the grant" is stated here rather
    /// than spelled out at the render site.
    pub const fn grant_line(&self) -> bool {
        matches!(self, EnvelopeLens::V4 | EnvelopeLens::V5)
    }

    /// Whether the `done` verb card is the DECLARED card (outcome/reason
    /// attributes + leading `evidence:` lines) — `V5` only (turn-6 spec
    /// §3.1). `false` for v1–v4, whose prompts and cards stay
    /// byte-identical to what they always rendered.
    pub const fn done_declares(&self) -> bool {
        matches!(self, EnvelopeLens::V5)
    }

    /// Parses the TOML `envelope = "v1" | "v2" | "v3" | "v4" | "v5"` string value.
    /// Any other value is a named config error listing the valid set —
    /// never a silent fallback to a default.
    fn parse(raw: &str) -> Result<EnvelopeLens, String> {
        match raw {
            "v1" => Ok(EnvelopeLens::V1),
            "v2" => Ok(EnvelopeLens::V2),
            "v3" => Ok(EnvelopeLens::V3),
            "v4" => Ok(EnvelopeLens::V4),
            "v5" => Ok(EnvelopeLens::V5),
            other => Err(format!(
                "invalid envelope {other:?}: valid values are \"v1\", \"v2\", \"v3\", \"v4\", \"v5\""
            )),
        }
    }
}

/// One `models` entry: either a bare path (today's shape) or a tuning table.
///
/// Per `docs/superpowers/specs/2026-08-15-partial-offload-capability-window-design.md` §2,
/// a config entry accepts both shapes via serde untagged:
///
/// ```text
/// [models]
/// "qwen3:14b" = "/path/to/model.gguf"        # bare string → Path variant
///
/// [models."qwen3.8:27b"]                     # table → Tuned variant
/// path = "/path/to/model.gguf"
/// n_gpu_layers = 28        # optional; None → full offload
/// weights_vram_mib = 11264 # optional; None → full charge
/// ```
///
/// Both new fields are optional; omitting both is byte-for-byte today's behavior.
/// Every existing config keeps parsing.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(untagged)]
pub enum ModelSpec {
    /// Bare path string; all tuning fields are None.
    Path(PathBuf),
    /// Table with required `path` and optional tuning fields.
    Tuned {
        path: PathBuf,
        #[serde(default)]
        n_gpu_layers: Option<u32>,
        #[serde(default)]
        weights_vram_mib: Option<u64>,
        /// Selects the envelope-v2 (think-preseeded) lens for this model
        /// (`docs/superpowers/evidence/2026-08-15-g4-protocol.md` §10,
        /// Amendment 2). An explicit operator choice, never inferred:
        /// omitting the key (or using the bare-path shape) is `false` —
        /// envelope-v1, unchanged.
        ///
        /// **Shipped as an alias for `envelope = "v2"`** (protocol §11,
        /// Amendment 3): both keys may be set, but only when they agree —
        /// see [`ModelSpec::envelope_lens`] for the exact conflict rules.
        /// Kept as its own field (rather than folded away) so every config
        /// written against Amendment 2 keeps parsing unchanged.
        ///
        /// `Option<bool>`, not `bool`: the conflict check needs to tell
        /// "the operator never mentioned this key" (`None`, agrees with
        /// anything) from "the operator wrote `think_preseed = false`"
        /// (`Some(false)`, which DOES conflict with `envelope = "v2"`/`"v3"`)
        /// — a plain `bool` with a serde default can't distinguish those two
        /// cases, since both parse to the same `false`.
        #[serde(default)]
        think_preseed: Option<bool>,
        /// Selects the task-loop prompt envelope
        /// (`docs/superpowers/evidence/2026-08-15-g4-protocol.md` §11,
        /// Amendment 3; turn-4 spec §2 for `"v4"`): `"v1"`, `"v2"`, `"v3"`,
        /// or `"v4"`. Raw and unvalidated at
        /// the parse step on purpose — [`ModelSpec::envelope_lens`] is the
        /// one place that validates it (against `think_preseed` too), and
        /// [`load_config`] calls that accessor for every model before
        /// returning, so an unknown value or a conflicting pair is always a
        /// named config error, never a silent pick.
        #[serde(default)]
        envelope: Option<String>,
        /// The declared KV-per-token override, in bytes
        /// (`docs/superpowers/specs/2026-08-15-partial-offload-capability-window-design.md`
        /// §10 addendum). Present -> it IS the KV charge everywhere
        /// `kv_per_token` is read for this model (the window law AND the
        /// reservation charge — the same one-source discipline §3 applies to
        /// `weights_vram_mib`). Absent -> the GGUF-derived value, unchanged.
        ///
        /// **No clamp against the GGUF-derived value** (unlike
        /// `weights_vram_mib`'s `min(declared, file)`): a declared value is a
        /// *measured* override for geometries the formula does not model;
        /// since turn 5 the formula itself counts only attention layers
        /// (`GgufMeta::attention_layers`), so the override is no longer
        /// needed for Qwen3.5/3.6 hybrids, and a LARGER declared number is
        /// allowed too (extra conservative). **Declaring too small is the
        /// OOM direction**: the window law would then grant tokens whose
        /// real KV exceeds VRAM — this is a declared, measured-once-with-
        /// headroom number, not something this daemon ever verifies against
        /// the model's actual runtime KV footprint.
        #[serde(default)]
        kv_per_token_bytes: Option<u64>,
        /// Opts this model into the G5 refusal-honesty probe
        /// (`docs/superpowers/evidence/2026-08-16-g5-protocol.md` §1: "Each
        /// configured (model, envelope) pair, opt-in via `g5_probe = true`
        /// ... absent = false"). Explicit operator choice, never inferred —
        /// same shape as `think_preseed`/`envelope`: omitting the key (or
        /// using the bare-path shape) is `false`, and G5 does not run for
        /// that model at all.
        #[serde(default)]
        g5_probe: bool,
    },
}

impl ModelSpec {
    /// Returns a reference to the model's path.
    pub fn path(&self) -> &Path {
        match self {
            Self::Path(p) => p,
            Self::Tuned { path, .. } => path,
        }
    }

    /// Returns the per-model n_gpu_layers override, if configured.
    ///
    /// The `Path` variant returns `None` (no tuning configured).
    pub fn n_gpu_layers(&self) -> Option<u32> {
        match self {
            Self::Path(_) => None,
            Self::Tuned { n_gpu_layers, .. } => *n_gpu_layers,
        }
    }

    /// Returns the per-model weights_vram_mib override, if configured.
    ///
    /// The `Path` variant returns `None` (no tuning configured).
    /// Per spec §3, `min(declared, weights_bytes)` is the effective charge;
    /// see `pager` accounting and window-law VRAM terms for application.
    pub fn weights_vram_mib(&self) -> Option<u64> {
        match self {
            Self::Path(_) => None,
            Self::Tuned {
                weights_vram_mib, ..
            } => *weights_vram_mib,
        }
    }

    /// Returns whether this model is configured for the envelope-v2
    /// (think-preseeded) lens (protocol §10, Amendment 2).
    ///
    /// The `Path` variant returns `false` (no tuning configured), matching
    /// every other tuning accessor's bare-path default.
    pub fn think_preseed(&self) -> bool {
        match self {
            Self::Path(_) => false,
            Self::Tuned { think_preseed, .. } => think_preseed.unwrap_or(false),
        }
    }

    /// Resolves the configured [`EnvelopeLens`] (protocol §11, Amendment 3),
    /// validating both the raw `envelope` string AND its agreement with
    /// `think_preseed` — the one place that combines them, so every reader
    /// of a model's envelope (`main.rs`'s wiring, and this method's own
    /// tests) goes through the same rule:
    ///
    /// - `envelope` absent, `think_preseed` absent/`false` -> `V1` (today's
    ///   behavior, unchanged).
    /// - `envelope` absent, `think_preseed = true` -> `V2` (Amendment 2's
    ///   shipped alias, unchanged).
    /// - `envelope` present and `think_preseed` doesn't disagree with it ->
    ///   the parsed `envelope` value. Agreement is checked by the disagreeing
    ///   PAIRS named in Amendment 3, not by requiring an exact match:
    ///   `envelope = "v3"` with `think_preseed = true` is fine (`v3` implies
    ///   the pre-seed `v2` already requires), and `envelope = "v1"` with
    ///   `think_preseed` absent/`false` is fine too.
    /// - `envelope = "v1"` with `think_preseed = true`, or `envelope =
    ///   "v2"`/`"v3"`/`"v4"` with `think_preseed = false` -> a named config error:
    ///   the two keys disagree, and this is never resolved by silently
    ///   picking one.
    /// - An unrecognized `envelope` string -> a named config error listing
    ///   the valid set.
    ///
    /// The bare-path shape (`Self::Path`) always resolves to `V1` — it has
    /// no tuning fields to disagree with anything.
    pub fn envelope_lens(&self) -> Result<EnvelopeLens, String> {
        let (envelope, think_preseed) = match self {
            Self::Path(_) => return Ok(EnvelopeLens::V1),
            Self::Tuned {
                envelope,
                think_preseed,
                ..
            } => (envelope, *think_preseed),
        };
        let parsed = match envelope.as_deref() {
            None => None,
            Some(raw) => Some(EnvelopeLens::parse(raw)?),
        };
        // `think_preseed: None` means the operator never mentioned the key
        // at all — it agrees with any `envelope` value, unlike an
        // EXPLICIT `Some(true)`/`Some(false)`, which is what the conflict
        // rule actually checks against.
        match (parsed, think_preseed) {
            (Some(EnvelopeLens::V1), Some(true)) => Err(
                "envelope = \"v1\" conflicts with think_preseed = true (think_preseed is an \
                 alias for envelope = \"v2\")"
                    .to_string(),
            ),
            (Some(EnvelopeLens::V2), Some(false)) => {
                Err("envelope = \"v2\" conflicts with think_preseed = false".to_string())
            }
            (Some(EnvelopeLens::V3), Some(false)) => {
                Err("envelope = \"v3\" conflicts with think_preseed = false".to_string())
            }
            (Some(EnvelopeLens::V4), Some(false)) => {
                Err("envelope = \"v4\" conflicts with think_preseed = false".to_string())
            }
            (Some(lens), _) => Ok(lens),
            (None, Some(true)) => Ok(EnvelopeLens::V2),
            (None, _) => Ok(EnvelopeLens::V1),
        }
    }

    /// Returns the per-model declared KV-per-token override, in bytes, if
    /// configured (spec §10 addendum).
    ///
    /// The `Path` variant returns `None` (no tuning configured), matching
    /// every other bare-path tuning accessor's default.
    pub fn kv_per_token_bytes(&self) -> Option<u64> {
        match self {
            Self::Path(_) => None,
            Self::Tuned {
                kv_per_token_bytes, ..
            } => *kv_per_token_bytes,
        }
    }

    /// Whether this model opts into the G5 refusal-honesty probe
    /// (`docs/superpowers/evidence/2026-08-16-g5-protocol.md` §1).
    ///
    /// The `Path` variant returns `false` (no tuning configured), matching
    /// every other bare-path tuning accessor's default — G5 is opt-in, so
    /// the bare-path shape (which cannot express any opt-in) never runs it.
    pub fn g5_probe(&self) -> bool {
        match self {
            Self::Path(_) => false,
            Self::Tuned { g5_probe, .. } => *g5_probe,
        }
    }
}

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

/// 600s — matches the wall-clock cap `post.rs` shipped as `PROBE_TIMEOUT`
/// before this field existed, so every config that omits
/// `assay.probe_timeout_secs` keeps that behavior byte-for-byte.
fn default_probe_timeout_secs() -> u64 {
    600
}

/// `MemoryConfig::max_episodes`'s default — spec §6's retention cap on
/// distinct episode ids. 256 is generous headroom over what a single
/// operator's task volume plausibly mints between prunes, while still
/// bounding the store's replay cost at boot.
fn default_max_episodes() -> usize {
    256
}

/// `MemoryConfig::refalsify`'s default — `true` since the 2026-08-28
/// operator ruling on the refalsify-battery-v2 findings
/// (`docs/superpowers/evidence/2026-08-28-refalsify-battery-v2-findings.md`:
/// G1 token-preservation exact, G2 injection 50=50, probe cost sub-noise),
/// which flipped v1 spec §5's original `false`. A config written before the
/// flip that relied on the old default gets probing now; `refalsify = false`
/// opts back out.
fn default_refalsify() -> bool {
    true
}

/// The `[memory]` section (spec §6): the organ's on/off switch, its
/// retention cap, and the refalsify opt-out. Serde defaults on every field,
/// plus `Default` on the whole struct via `#[serde(default)]
/// pub memory: MemoryConfig` on [`Config`], means a TOML with no `[memory]`
/// table at all parses to `enabled: false, max_episodes: 256,
/// refalsify: true` — and since `refalsify` is read only under an enabled
/// organ, that stays byte-compatible in behavior with every config written
/// before this section existed.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct MemoryConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_max_episodes")]
    pub max_episodes: usize,
    /// Refalsify (v2 class-aware design
    /// `docs/superpowers/specs/2026-08-28-refalsify-v2-class-aware-design.md`
    /// §2; activation carried from the v1 spec
    /// `docs/superpowers/specs/2026-08-27-refalsify-on-exact-design.md` §5):
    /// `true` makes the worker probe a retrieved episode's stored run
    /// command under the incoming task's grant, `cwd` and `ExecBounds`
    /// before injecting. The probe informs injection, it never contradicts:
    /// a clean nonzero exit confirms the premise (`premise_held`) and
    /// injects, a clean exit 0 means the premise is gone (`premise_gone`)
    /// and the task runs memory-silent — either way the store is untouched.
    /// Default `true` (see [`default_refalsify`] for the flip's provenance);
    /// read only when `enabled` is true.
    #[serde(default = "default_refalsify")]
    pub refalsify: bool,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        MemoryConfig {
            enabled: false,
            max_episodes: default_max_episodes(),
            refalsify: default_refalsify(),
        }
    }
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct Config {
    #[serde(default = "default_port")]
    pub port: u16,
    /// Root of daemon state: `journal/`, `profiles/`, `images/`.
    pub data_dir: PathBuf,
    /// Model name -> path and optional tuning (per-model n_gpu_layers, weights_vram_mib).
    /// Per spec §2, accepts both bare-string and table shapes.
    pub models: BTreeMap<String, ModelSpec>,
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
    /// The memory organ's config switch and retention cap (spec §6). Dark
    /// by default, same convention as `tasks_enabled` — an operator opts in
    /// rather than the organ turning itself on under an existing config.
    #[serde(default)]
    pub memory: MemoryConfig,
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
    /// Wall-clock cap, in seconds, on one model's boot-POST assay probe
    /// (`post::PostRunner`'s `probe_timeout`) — the same bound `post.rs`'s
    /// module docs describe: a wedged assay must not hold the
    /// provisional-admission window open for the life of the daemon, so the
    /// child is killed and the probe recorded as a named failure once this
    /// many seconds pass.
    ///
    /// Defaults to 600 (`default_probe_timeout_secs`), which is ~5× the
    /// ~110 s a `--quick` probe measured on the enthusiast-16GB tier
    /// (2026-08-14, qwen2.5-coder:7b-q8_0 on an RTX 5080) — see `post.rs`
    /// for that measurement's full context. Every config that omits this
    /// key keeps that 600 s behavior byte-for-byte.
    ///
    /// **Raise this for slow, partially-offloaded models**, which are
    /// first-class under partial offload and can blow far past the
    /// quick-probe baseline above: a measured qwen3.8-27b Q3 at ~15.5
    /// tok/s (~3.4× slower than the baseline model) projects a `--quick`
    /// probe at ~25-30 min. Left at the 600 s default, POST would kill
    /// that probe before it finishes — the model stays unprofiled, and
    /// (with the codec probe gated strictly after POST succeeds) the G4
    /// codec probe aborts too.
    #[serde(default = "default_probe_timeout_secs")]
    pub probe_timeout_secs: u64,
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
    let config: Config = toml::from_str(&text)
        .map_err(|e| format!("failed to parse config {}: {e}", path.display()))?;
    // Protocol §11 (Amendment 3): "an explicit per-model config enum ...
    // setting both `envelope` and `think_preseed` inconsistently is a named
    // config error, never a silent pick" — validated here, once, for every
    // model, so a bad config fails at load rather than at first task-loop
    // use.
    for (name, spec) in &config.models {
        spec.envelope_lens()
            .map_err(|e| format!("model {name}: {e}"))?;
    }
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_python_is_python3() {
        assert_eq!(default_python(), "python3");
    }

    /// Task 8, spec §6: a config written before `[memory]` existed must keep
    /// parsing byte-compatibly — `toml::from_str` directly (the file-less
    /// counterpart to `tests/config_test.rs`'s `write_temp_toml` pattern,
    /// which exercises `load_config`'s file-reading path instead).
    #[test]
    fn memory_config_defaults_when_section_omitted() {
        let toml_str = r#"
port = 9000
data_dir = "/tmp/bloomery-daemon-test-data"
tier = { name = "enthusiast-16gb", emulated = false }
assay = { enabled = false, python = "python3" }

[models]
llama = "/models/llama.gguf"
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert!(!config.memory.enabled);
        assert_eq!(config.memory.max_episodes, 256);
    }

    /// The operator's explicit opt-in parses, and `max_episodes` still
    /// defaults when only `enabled` is set.
    #[test]
    fn memory_config_enabled_parses() {
        let toml_str = r#"
port = 9000
data_dir = "/tmp/bloomery-daemon-test-data"
tier = { name = "enthusiast-16gb", emulated = false }
assay = { enabled = false, python = "python3" }

[models]
llama = "/models/llama.gguf"

[memory]
enabled = true
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert!(config.memory.enabled);
        assert_eq!(config.memory.max_episodes, 256);
    }

    /// An operator-tuned `max_episodes` sticks too.
    #[test]
    fn memory_config_max_episodes_parses() {
        let toml_str = r#"
port = 9000
data_dir = "/tmp/bloomery-daemon-test-data"
tier = { name = "enthusiast-16gb", emulated = false }
assay = { enabled = false, python = "python3" }

[models]
llama = "/models/llama.gguf"

[memory]
enabled = true
max_episodes = 64
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert!(config.memory.enabled);
        assert_eq!(config.memory.max_episodes, 64);
    }
}
