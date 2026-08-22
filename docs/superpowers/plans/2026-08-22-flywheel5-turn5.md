# Flywheel Turn 5 — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Train the first adapter of the REAP-48 line (`qwen36-reap48-flywheel5`) on a rented A100 under the frozen envelope-v4 instrument, after two ride-alongs (hybrid-aware pager geometry; a keyed, argument-carrying task journal + a tested recompute tool) and a pre-registered two-boot baseline of the untrained base.

**Architecture:** Two branches. Branch 1 `turn5-ride-alongs` (Tasks 1–5) changes `bloomery-core` (GGUF meta + geometry + journal events), the daemon's pager/probe/task-loop, and adds `tools/evidence/`; it merges before any boot. Task 6 (human-gated) boots the untrained base twice on master. Branch 2 `flywheel5-turn5` (Tasks 7–11) adds the bf16-LoRA trainer for `qwen3_5_moe`, the pod runbook, the pre-registration, the training run, and the battery.

**Tech Stack:** Rust (cargo workspace; `FakeSubstrate` tests; featured build `--features vulkan`), Python 3.12+ stdlib for `tools/evidence`, `~/flywheel-venv` (torch 2.11 / transformers 5.5.0 / peft 0.20.0) for trainer tests, RunPod REST (`https://rest.runpod.io/v1`) + GraphQL (`https://api.runpod.io/graphql`), llama.cpp `8672290` for convert/quantize.

**Spec:** `docs/superpowers/specs/2026-08-22-flywheel5-turn5-design.md` — read it first; every task below argues from it.

## Global Constraints

- **Frozen, never amended:** `crates/bloomery-daemon/fixtures/codec-tasks-v{1,2-mixed,3-mixed,4-mixed}.toml`; every file under `docs/superpowers/evidence/` dated before 2026-08-22; `train.py`'s hyperparameters and the two `20260816` seeds.
- **Instrument:** G4 on `codec-tasks-v1`, G5 on `codec-tasks-v4-mixed`, envelope `v4`; floors ≥16/20 and ≥13/16 per class; decided/provisional = two-sided Wilson strictly straddling 0.80 (`scoring::is_provisional`), always stated apart from the floor. **No cross-envelope sentence anywhere.**
- **Corpus:** `~/flywheel4/corpus.jsonl`, sha256 `9c51a8668b4ce861dbe3c8528f59655a9a78eee12523f397b33e28d5d7928a7d`, reused byte-identical; fingerprint `docs/superpowers/evidence/2026-08-21-flywheel4-fingerprint.json`.
- **Base:** `~/models/hf/Qwen3.6-35B-A3B-REAP48-ours/` (`model.safetensors` sha256 `8027ca0a8277b540cd4c62eb7a5bdf6028875e84b33ddcf4f9cd4b0e9d63423b`); served GGUF `~/models/gguf/qwen36-reap48-ours-Q4_K_M.gguf` (sha256 `90e2181e8c3175c7f59f911ee70dfcc58cd068977fc657be3a4101d041f591a5`, 11,755,624,288 B).
- **Recipe (spec §4.2):** bf16 LoRA via peft, r=16 α=32 dropout 0 bias none, targets `q_proj k_proj v_proj o_proj in_proj_qkv in_proj_z in_proj_b in_proj_a out_proj gate_proj up_proj down_proj`; experts + router frozen (asserted); unpacked bs 1, accum 8, 2 epochs, lr 2e-4 cosine warmup 20, eval every 100, seed 20260816 (`torch.manual_seed` before `get_peft_model`, `TrainingArguments(seed=)`); MAX_SEQ 4096; raw text, completion-only loss, no EOS, tail `</action>`.
- **Rental:** RunPod, A100-SXM4-80GB ($1.39/h), **150 GB** container disk, network volume mounted at `/workspace`, image `runpod/pytorch:1.1.0-cu1290-torch291-ubuntu2404`; pins transformers 5.5.0 / peft 0.20.0 / accelerate 1.14.0 / safetensors 0.8.0 / flash-linear-attention 0.5.2; **all installs before the job; nothing pip-installed beside a running job**; `setsid nohup`; poll log files, never `pgrep -f`; **turn cap $10**; stop-and-ask on any failure. API key `~/.config/runpod/api_key` (0600) — **never echoed, logged, or committed**; SSH key `~/.ssh/runpod_spike`.
- **Boot configs (Tasks 6, 11):** `envelope = "v4"`, `g5_probe = true`, no `kv_per_token_bytes`, `ctx_overhead_mib = 512`, dedicated scratch `data_dir` under `target/` (never `~/.local/share/bloomery/drift/`), `PYTHONPATH=/home/brice/workspace/assay/src` (assay 0.13.0), daemon `/status` `digest` must equal the configured GGUF's sha256 or the boot is BLOCKED.
- **House rules:** branch work in worktrees (`.worktrees/<name>`), never switch the shared checkout; `cargo test --workspace` FIRST, then `cargo build --release -p bloomery-daemon --features vulkan` LAST, never `cargo test` after it; daemons down by verified PID (`readlink /proc/<pid>/exe`), never `pkill`; never the `timeout` wrapper; idle `ollama serve` reported, not killed; Rust source ≤800 lines, Python modules ≤400 lines; commits conventional (`feat:`/`fix:`/`docs:`/`test:`), no attribution trailers.
- **Money/GPU steps are HUMAN-GATED:** Tasks 6, 8, 10, 11 stop for Brice's explicit go before any boot or pod cut.

---

## Branch 1 — `turn5-ride-alongs`

Worktree: `git worktree add .worktrees/ride-alongs -b turn5-ride-alongs master` (run from `~/workspace/bloomery`). All Task 1–5 commands run inside `.worktrees/ride-alongs`.

### Task 1: Docs first — `gates.md` dated amendment

**Files:**
- Modify: `docs/gates.md` (append after the 2026-08-21 amendment under `## G5`)

**Interfaces:**
- Produces: the dated amendment text Tasks 6, 9, 11 cite.

- [ ] **Step 1: Append the amendment** — insert this block immediately after the paragraph that begins `**Amendment (2026-08-21, recorded before the v4 instrument exists):**` and before the `---` that closes the G5 section:

```markdown
**Amendment (2026-08-22, recorded before any measurement of the new line):**
turn 5's decided-G5 instrument is **unchanged** — fixture set
codec-tasks-v4-mixed (16 `expect="patch"` + 16 `expect="refuse"`) under
`bloomery-task-envelope-v4`, scored per
docs/superpowers/evidence/2026-08-21-g5v4-protocol.md (with its dated §5
amendment), floor ≥13/16 per class, decided/provisional by the two-sided
Wilson rule (bT10/R1) stated apart from the floor — now applied to a second
model line, `qwen36-reap48` (REAP-48-pruned Qwen3.6-35B-A3B; base
`~/models/hf/Qwen3.6-35B-A3B-REAP48-ours`, served as Q4_K_M). Results
remain per-(model, envelope): turn-4's 14B numbers and turn-5's REAP-48
numbers are both envelope-v4 numbers and may appear in one descriptive
ladder; no causal sentence across bases, and no cross-envelope comparison,
is ever written. The anchors for the new line are the pre-registered
baseline boots of the **untrained** base recorded in
docs/superpowers/evidence/<date>-g5v4-reap48-baselines.md (two identical
boots; boot 1 is the anchor, declared before the first boot); the
2026-08-21 REAP-48 spike numbers are superseded as anchors. G4 on
codec-tasks-v1 is unchanged. No fixture set, scoring rule, or envelope is
amended by this note.
```

- [ ] **Step 2: Commit**

```bash
git add docs/gates.md
git commit -m "docs: gates.md — turn-5 amendment: v4-mixed@v4 is the decided-G5 instrument for the qwen36-reap48 line"
```

---

### Task 2: Hybrid-aware geometry — `attention_layers` and `recurrent_state_bytes`

**Files:**
- Modify: `crates/bloomery-core/src/gguf.rs` (struct at lines 15–23; `parse_gguf_meta` at ~277–300)
- Modify: `crates/bloomery-core/src/geometry.rs:17-23` (`kv_bytes_per_token`)
- Modify: `crates/bloomery-daemon/src/pager.rs` (`create_agent` ~575–645: window-law input + `reserved_bytes`)
- Modify: `crates/bloomery-daemon/src/pager/tuning.rs` (~55–70 doc comment; add accessor)
- Modify: `crates/bloomery-daemon/src/pager/status.rs` (`ModelStatus` ~95–125; build site ~233–240)
- Modify: `crates/bloomery-daemon/src/config.rs:175-186` (doc comment only)
- Modify: every `GgufMeta { ... }` literal (48 sites, `grep -rn "GgufMeta {" crates/ --include=*.rs`) — add the two new fields
- Test: `crates/bloomery-core/tests/gguf_test.rs`, `crates/bloomery-core/tests/geometry_test.rs`, `crates/bloomery-daemon/tests/pager_reservation_test.rs`, new `crates/bloomery-core/tests/gguf_real_hybrid_test.rs`

**Interfaces:**
- Produces: `GgufMeta { …, attention_layers: u32, recurrent_state_bytes: u64 }`; `kv_bytes_per_token(&GgufMeta)` counts `attention_layers`; `ModelEntry::recurrent_state_bytes(&self) -> u64`; `ModelStatus.recurrent_state_bytes: u64` (JSON field `"recurrent_state_bytes"`); `Agent.reserved_bytes = kv + ctx_overhead + recurrent`.

- [ ] **Step 1: Write the failing gguf tests** — append to `crates/bloomery-core/tests/gguf_test.rs` (it already has `kv_string`, `kv_u32`, `write_gguf`):

```rust
fn write_qwen35moe_like_gguf(path: &std::path::Path, full_attention_interval: Option<u32>, ssm: bool) {
    let mut kvs = Vec::new();
    let mut n = 0u64;
    kv_string(&mut kvs, "general.architecture", "qwen35moe"); n += 1;
    kv_u32(&mut kvs, "qwen35moe.block_count", 40); n += 1;
    kv_u32(&mut kvs, "qwen35moe.attention.head_count_kv", 2); n += 1;
    kv_u32(&mut kvs, "qwen35moe.attention.key_length", 256); n += 1;
    kv_u32(&mut kvs, "qwen35moe.context_length", 262144); n += 1;
    if let Some(k) = full_attention_interval {
        kv_u32(&mut kvs, "qwen35moe.full_attention_interval", k); n += 1;
    }
    if ssm {
        kv_u32(&mut kvs, "qwen35moe.ssm.conv_kernel", 4); n += 1;
        kv_u32(&mut kvs, "qwen35moe.ssm.state_size", 128); n += 1;
        kv_u32(&mut kvs, "qwen35moe.ssm.group_count", 16); n += 1;
        kv_u32(&mut kvs, "qwen35moe.ssm.inner_size", 4096); n += 1;
    }
    write_gguf(path, n, &kvs);
}

#[test]
fn hybrid_meta_counts_attention_layers_and_derives_recurrent_state() {
    let dir = std::env::temp_dir().join("bloomery-gguf-hybrid");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("hybrid.gguf");
    write_qwen35moe_like_gguf(&path, Some(4), true);
    let m = parse_gguf_meta(&path).unwrap();
    assert_eq!(m.layers, 40);
    assert_eq!(m.attention_layers, 10, "40 blocks / interval 4");
    // 30 recurrent layers x [(4-1)*(4096 + 2*16*128) + 128*4096] x 4 bytes
    assert_eq!(m.recurrent_state_bytes, 65_863_680);
}

#[test]
fn dense_meta_keeps_attention_layers_equal_to_layers_and_zero_recurrent() {
    let dir = std::env::temp_dir().join("bloomery-gguf-dense2");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("dense.gguf");
    write_qwen_like_gguf(&path);
    let m = parse_gguf_meta(&path).unwrap();
    assert_eq!(m.attention_layers, m.layers);
    assert_eq!(m.recurrent_state_bytes, 0);
}

#[test]
fn interval_without_ssm_keys_still_counts_attention_layers_and_charges_no_state() {
    let dir = std::env::temp_dir().join("bloomery-gguf-hybrid-nossm");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("h.gguf");
    write_qwen35moe_like_gguf(&path, Some(4), false);
    let m = parse_gguf_meta(&path).unwrap();
    assert_eq!((m.attention_layers, m.recurrent_state_bytes), (10, 0));
}

#[test]
fn zero_full_attention_interval_is_invalid_data() {
    let dir = std::env::temp_dir().join("bloomery-gguf-hybrid-zero");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("z.gguf");
    write_qwen35moe_like_gguf(&path, Some(0), true);
    match parse_gguf_meta(&path) {
        Err(GgufError::Io(e)) => assert_eq!(e.kind(), std::io::ErrorKind::InvalidData),
        other => panic!("expected InvalidData, got {other:?}"),
    }
}
```

And to `crates/bloomery-core/tests/geometry_test.rs`:

```rust
#[test]
fn kv_bytes_per_token_counts_attention_layers_only() {
    use bloomery_core::gguf::GgufMeta;
    let hybrid = GgufMeta {
        arch: "qwen35moe".into(),
        layers: 40,
        attention_layers: 10,
        kv_heads: 2,
        head_dim: 256,
        training_ctx: 262_144,
        weights_bytes: 11_755_624_288,
        recurrent_state_bytes: 65_863_680,
    };
    assert_eq!(kv_bytes_per_token(&hybrid), 20_480, "2 * 10 * 2 * 256 * 2");
    let dense = GgufMeta { attention_layers: 40, ..hybrid.clone() };
    assert_eq!(kv_bytes_per_token(&dense), 81_920, "the pre-fix over-count, for the record");
}
```

- [ ] **Step 2: Run, expect compile failure** — `cargo test -p bloomery-core --test gguf_test --test geometry_test 2>&1 | tail -5` → `no field attention_layers`.

- [ ] **Step 3: Implement in `gguf.rs`**. Struct:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GgufMeta {
    pub arch: String,
    pub layers: u32,
    /// Layers that own a KV cache. `block_count / {arch}.full_attention_interval`
    /// when that key is present (llama.cpp: layer i is full attention iff
    /// (i+1) % interval == 0), else `block_count`. Spec 2026-08-22 turn-5 §2.
    pub attention_layers: u32,
    pub kv_heads: u32,
    pub head_dim: u32,
    pub training_ctx: u32,
    pub weights_bytes: u64,
    /// Per-context constant for the recurrent (Gated-DeltaNet / SSM) layers:
    /// sum over `layers - attention_layers` of `[(conv_kernel-1) *
    /// (inner_size + 2*group_count*state_size) + state_size*inner_size] * 4`
    /// bytes (llama.cpp's `n_embd_r + n_embd_s`, f32). 0 when the
    /// `{arch}.ssm.*` keys are absent. Independent of the window.
    pub recurrent_state_bytes: u64,
}
```

Add a helper beside `lookup_u32`:

```rust
/// `Ok(None)` when `key` is absent; the usual error when it is present but not an integer.
fn lookup_u32_opt(kvs: &HashMap<String, GgufValue>, key: &str) -> Result<Option<u32>, GgufError> {
    if kvs.contains_key(key) { lookup_u32(kvs, key).map(Some) } else { Ok(None) }
}

fn resolve_attention_layers(kvs: &HashMap<String, GgufValue>, arch: &str, layers: u32) -> Result<u32, GgufError> {
    let key = format!("{arch}.full_attention_interval");
    match lookup_u32_opt(kvs, &key)? {
        None => Ok(layers),
        Some(0) => Err(GgufError::Io(io::Error::new(io::ErrorKind::InvalidData, format!("{key} is zero")))),
        Some(k) => Ok(layers / k),
    }
}

/// f32 bytes per recurrent layer x recurrent layers; 0 unless all four ssm keys are present.
fn resolve_recurrent_state_bytes(kvs: &HashMap<String, GgufValue>, arch: &str, recurrent_layers: u32) -> Result<u64, GgufError> {
    let (conv_kernel, state_size, group_count, inner_size) = (
        lookup_u32_opt(kvs, &format!("{arch}.ssm.conv_kernel"))?,
        lookup_u32_opt(kvs, &format!("{arch}.ssm.state_size"))?,
        lookup_u32_opt(kvs, &format!("{arch}.ssm.group_count"))?,
        lookup_u32_opt(kvs, &format!("{arch}.ssm.inner_size"))?,
    );
    let (Some(conv_kernel), Some(state_size), Some(group_count), Some(inner_size)) =
        (conv_kernel, state_size, group_count, inner_size) else { return Ok(0) };
    let conv_dim = u64::from(inner_size) + 2 * u64::from(group_count) * u64::from(state_size);
    let per_layer = u64::from(conv_kernel.saturating_sub(1)) * conv_dim + u64::from(state_size) * u64::from(inner_size);
    Ok(u64::from(recurrent_layers) * per_layer * 4)
}
```

In `parse_gguf_meta`, after `training_ctx`:

```rust
    let attention_layers = resolve_attention_layers(&kvs, &arch, layers)?;
    let recurrent_state_bytes =
        resolve_recurrent_state_bytes(&kvs, &arch, layers.saturating_sub(attention_layers))?;
    Ok(GgufMeta { arch, layers, attention_layers, kv_heads, head_dim, training_ctx,
                  weights_bytes: file_len, recurrent_state_bytes })
```

`geometry.rs`:

```rust
/// `2 (K and V) * attention_layers * kv_heads * head_dim * 2 (f16 bytes)`.
/// Only layers that own a KV cache count — hybrid models' recurrent layers
/// are charged by `GgufMeta::recurrent_state_bytes` instead (turn-5 spec §2).
pub fn kv_bytes_per_token(m: &GgufMeta) -> u64 {
    KV_TENSORS * u64::from(m.attention_layers) * u64::from(m.kv_heads) * u64::from(m.head_dim) * F16_BYTES
}
```

- [ ] **Step 4: Fix every `GgufMeta { … }` literal** — run `cargo test --workspace --no-run 2>&1 | grep -E "missing fields|-->" | head -80`; in each listed literal add `attention_layers: <the literal's layers value>,` immediately after its `layers:` line and `recurrent_state_bytes: 0,` as the last field. Do not change any other value. Re-run until the workspace compiles.

- [ ] **Step 5: Run the core tests** — `cargo test -p bloomery-core` → all pass incl. the five new ones.

- [ ] **Step 6: Write the failing pager test** — append to `crates/bloomery-daemon/tests/pager_reservation_test.rs` (uses its existing `pager_in`, `write_gguf`, `WINDOW_CAP`, `MIB`; mirror how its `status_reports_reserved_bytes_and_both_overhead_terms` test registers a model and reads `status()`):

```rust
fn hybrid_meta(weights_bytes: u64) -> bloomery_core::gguf::GgufMeta {
    bloomery_core::gguf::GgufMeta {
        arch: "qwen35moe".into(),
        layers: 40,
        attention_layers: 10,
        kv_heads: 2,
        head_dim: 256,
        training_ctx: 4096,
        weights_bytes,
        recurrent_state_bytes: 65_863_680,
    }
}

/// Turn-5 spec §2: a hybrid model's recurrent state is a per-context
/// constant charged beside `ctx_overhead_bytes` — in the window law AND in
/// the agent's reservation — and surfaced on `/status` per model.
#[test]
fn recurrent_state_is_charged_per_context_and_reported() {
    let dir = fresh_dir("bloomery-resv-recurrent");
    let (mut p, _j) = pager_in(&dir, 0, Some(4096 * MIB));
    p.set_ctx_overhead_bytes(8 * MIB);
    let gguf = write_gguf(&dir, "h.gguf");
    // Register exactly as the file's existing tests do (profile/allow flags
    // as they do), with `hybrid_meta(200 * MIB)` as the meta.
    register_like_the_other_tests(&mut p, "h", &gguf, hybrid_meta(200 * MIB));
    let a = p.create_agent("h", 100, Some(WINDOW_CAP), 1000).unwrap();
    let st = p.status();
    let agent = st.agents.iter().find(|x| x.id == a.id).unwrap();
    // kv = 1024 tokens * (2*10*2*256*2 = 20_480) = 20_971_520
    assert_eq!(agent.kv_bytes, 20_971_520 + 8 * MIB + 65_863_680,
        "reserved = kv + ctx_overhead + recurrent_state");
    let model = st.models.iter().find(|m| m.name == "h").unwrap();
    assert_eq!(model.recurrent_state_bytes, 65_863_680);
    assert_eq!(model.kv_per_token, 20_480);
}
```

(`register_like_the_other_tests` is a placeholder NAME only for the registration lines the file already uses — copy those lines verbatim; the file's `status_reports_reserved_bytes_and_both_overhead_terms` shows them.) Also add a window-law assertion in the same test if the file exposes the window: `agent`'s `window_tokens` must equal `WINDOW_CAP` (the cap binds; the recurrent charge must not starve it at this budget).

- [ ] **Step 7: Run, expect failure** — `cargo test -p bloomery-daemon --test pager_reservation_test recurrent 2>&1 | tail -5` → `no field recurrent_state_bytes` on `ModelStatus`.

- [ ] **Step 8: Implement the charge sites.** `pager/tuning.rs` — add to the `impl ModelEntry` block next to `effective_kv_per_token`:

```rust
    /// Per-context recurrent-state charge (turn-5 spec §2) — a GGUF-derived
    /// constant, never overridden: the override story is `kv_per_token_bytes`'s.
    pub(crate) fn recurrent_state_bytes(&self) -> u64 {
        self.meta.recurrent_state_bytes
    }
```

and rewrite the `effective_kv_per_token` doc paragraph that says the GGUF-derived formula "overcounts hybrid-DeltaNet architectures ~4×" to: "a declared value is a *measured* override for geometries the formula does not model; since turn 5 the formula itself counts only attention layers (`GgufMeta::attention_layers`), so the override is no longer needed for Qwen3.5/3.6 hybrids". Same edit to the `kv_per_token_bytes` doc comment in `config.rs` (keep the OOM-direction warning).

`pager.rs` `create_agent`: extend the scoped tuple to also read `entry.recurrent_state_bytes()` into `recurrent_state_bytes`; pass `ctx_overhead_bytes: self.ctx_overhead_bytes.saturating_add(recurrent_state_bytes)` to `usable_window`; and set `reserved_bytes: kv_bytes.saturating_add(self.ctx_overhead_bytes).saturating_add(recurrent_state_bytes)`. Update the `Agent::reserved_bytes` doc in `agents.rs` to name the third term.

`pager/status.rs`: add `pub recurrent_state_bytes: u64,` to `ModelStatus` (after `kv_per_token_declared`, with a doc comment citing spec §2) and `recurrent_state_bytes: m.recurrent_state_bytes(),` at the build site.

- [ ] **Step 9: Run the daemon suite** — `cargo test -p bloomery-daemon 2>&1 | tail -3` → all pass.

- [ ] **Step 10: Real-GGUF skip-if-absent test** — create `crates/bloomery-core/tests/gguf_real_hybrid_test.rs`:

```rust
//! Parses the real REAP-48-ours GGUF when it is on this box (turn-5 spec §2)
//! and pins the two derived hybrid numbers against the spike's measurements.
//! Skips (prints, passes) when the file is absent, so CI never depends on it.
use bloomery_core::geometry::kv_bytes_per_token;
use bloomery_core::gguf::parse_gguf_meta;

#[test]
fn reap48_ours_gguf_derives_the_measured_hybrid_geometry() {
    let path = std::env::var("BLOOMERY_HYBRID_GGUF").unwrap_or_else(|_| {
        format!("{}/models/gguf/qwen36-reap48-ours-Q4_K_M.gguf", std::env::var("HOME").unwrap_or_default())
    });
    let path = std::path::Path::new(&path);
    if !path.exists() {
        eprintln!("skipped: {} not present", path.display());
        return;
    }
    let m = parse_gguf_meta(path).expect("the real GGUF parses");
    assert_eq!(m.arch, "qwen35moe");
    assert_eq!((m.layers, m.attention_layers), (40, 10));
    assert_eq!(kv_bytes_per_token(&m), 20_480, "llama.cpp: 1070.00 MiB / 54,784 cells");
    assert_eq!(m.recurrent_state_bytes, 65_863_680, "llama.cpp: RS buffer 62.81 MiB");
    assert_eq!(m.training_ctx, 262_144);
}
```

Run `cargo test -p bloomery-core --test gguf_real_hybrid_test -- --nocapture` → passes (this box has the file).

- [ ] **Step 11: Full workspace + clippy** — `cargo test --workspace 2>&1 | tail -3` and `cargo clippy --workspace --all-targets -- -D warnings`.

- [ ] **Step 12: Commit**

```bash
git add -A crates/
git commit -m "feat(pager): hybrid-aware geometry — KV counts attention layers only, recurrent state derived from ssm.* and charged per context"
```

---

### Task 3: `TaskStep.args` and `CodecFixture.agent`

**Files:**
- Modify: `crates/bloomery-core/src/journal.rs` (`TaskStep` at 84–90, `CodecFixture` at 95–112)
- Modify: `crates/bloomery-daemon/src/task/task_loop.rs` (`TaskStepRecord` 114–120, `StepReport` 158–164, `record_step` 193–215, the four `StepReport {` sites at ~458/539/578/599, `execute_action` 480–505)
- Modify: `crates/bloomery-daemon/src/pager/journal.rs:274-300` (`codec_fixture`), `crates/bloomery-daemon/src/pager/codec_gate.rs:203-225` (`journal_codec_fixture`), call sites `crates/bloomery-daemon/src/codec_probe/mod.rs:432` and `crates/bloomery-daemon/src/codec_probe/refuse.rs:296`
- Modify: every other `TaskStepRecord { … }` literal (`grep -rn "TaskStepRecord {" crates/` — 4 sites incl. `codec_probe/scoring.rs:203` test helper) — add `args: Vec::new()`
- Test: `crates/bloomery-core/tests/journal_test.rs`, `crates/bloomery-daemon/tests/task_loop_test.rs`, `crates/bloomery-daemon/tests/codec_probe_test.rs`

**Interfaces:**
- Produces: `Event::TaskStep { id, step, verb, outcome, duration_ms, args: Vec<String> }` (`#[serde(default)]`); `Event::CodecFixture { …, expect, agent: Option<AgentId> }` (`#[serde(default)]`); `TaskStepRecord.args: Vec<String>`; `Pager::journal_codec_fixture(…, expect: &str, agent: &str)`; `jrnl::codec_fixture(…, expect, agent: Option<&str>)`; `task_loop::action_args(&Action) -> Vec<String>` (private).

- [ ] **Step 1: Failing journal tests** — append to `crates/bloomery-core/tests/journal_test.rs`:

```rust
#[test]
fn task_step_with_no_args_key_deserializes_with_empty_args() {
    let line = r#"{"event":"TaskStep","id":"a112","step":1,"verb":"read","outcome":"read 109 bytes","duration_ms":0}"#;
    let event: Event = serde_json::from_str(line).expect("a pre-turn-5 TaskStep line must parse");
    match event {
        Event::TaskStep { args, verb, .. } => { assert!(args.is_empty()); assert_eq!(verb, "read"); }
        other => panic!("expected TaskStep, got {other:?}"),
    }
}

#[test]
fn codec_fixture_with_no_agent_key_deserializes_as_none() {
    let line = r#"{"event":"CodecFixture","model":"m1","fixture_set":"codec-tasks-v1","fixture":"py-mean-off-by-one","codec":"search_replace","landed":true,"steps":3,"detail":"patched (lens: python)","expect":"patch"}"#;
    let event: Event = serde_json::from_str(line).unwrap();
    match event {
        Event::CodecFixture { agent, .. } => assert_eq!(agent, None),
        other => panic!("expected CodecFixture, got {other:?}"),
    }
}

#[test]
fn task_step_args_and_codec_fixture_agent_round_trip() {
    let dir = std::env::temp_dir().join("bloomery-journal-turn5");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("j.jsonl");
    let _ = std::fs::remove_file(&path);
    let mut j = Journal::open(&path).unwrap();
    let e1 = Event::TaskStep {
        id: "a7".into(), step: 3, verb: "run".into(), outcome: "ran python3 exit 0".into(),
        duration_ms: 87, args: vec!["python3".into(), "-m".into(), "unittest".into(), "test_x.py".into()],
    };
    let e2 = Event::CodecFixture {
        model: "m1".into(), fixture_set: "codec-tasks-v4-mixed".into(), fixture: "v4-patch-run-py-01".into(),
        codec: "search_replace".into(), landed: true, steps: 4, detail: "patched (lens: python)".into(),
        expect: "patch".into(), agent: Some("a7".into()),
    };
    j.append(&e1).unwrap();
    j.append(&e2).unwrap();
    assert_eq!(replay(&path).unwrap(), vec![e1, e2]);
}
```

- [ ] **Step 2: Run, expect compile failure** — `cargo test -p bloomery-core --test journal_test 2>&1 | tail -5`.

- [ ] **Step 3: Implement the event fields** in `journal.rs`:

```rust
    TaskStep {
        id: AgentId,
        step: u32,
        verb: String,
        outcome: String,
        duration_ms: u64,
        /// The action's model-supplied arguments, verbatim and in order
        /// (turn-5 spec §3): read -> [path] (+ "lines=a-b"); find ->
        /// [pattern, path]; patch -> [path] (never the body); run -> argv;
        /// done / unparseable -> []. `#[serde(default)]` so pre-turn-5 rows
        /// replay with an empty list.
        #[serde(default)]
        args: Vec<String>,
    },
```

and on `CodecFixture`, after `expect`:

```rust
        /// The agent that ran this fixture — the exact join key to its
        /// `TaskStep` rows (`CodecFixture.agent == TaskStep.id`), replacing
        /// the ordinal join (turn-5 spec §3). `None` on pre-turn-5 rows.
        #[serde(default)]
        agent: Option<AgentId>,
```

Fix any compile errors across the **whole workspace** — every `Event::TaskStep { … }` / `Event::CodecFixture { … }` destructuring pattern that lists fields exhaustively must gain `..` (or the new field); every construction site must supply the field (`grep -rn "Event::TaskStep {\|Event::CodecFixture {" crates/`). Run `cargo test -p bloomery-core` → green (daemon sites are finished in Steps 6 and 9).

- [ ] **Step 4: Failing task-loop test** — in `crates/bloomery-daemon/tests/task_loop_test.rs`, locate the existing test that scripts a `read` → `patch` → `done` (or any) trajectory and replays the journal (grep `Event::TaskStep`); add, using that test's own pager/agent/journal setup and a scripted trajectory of **read `<target>` → run `["python3","-c","print(1)"]` (grant the `python3` prefix the way `task_exec_run_test.rs` does) → done**, the assertions:

```rust
    let rows: Vec<(String, Vec<String>)> = replay(&journal_path).unwrap().into_iter()
        .filter_map(|e| match e { Event::TaskStep { verb, args, .. } => Some((verb, args)), _ => None })
        .collect();
    assert_eq!(rows[0].0, "read");
    assert_eq!(rows[0].1, vec![target_rel_path.to_string()], "read -> [path]");
    assert_eq!(rows[1].0, "run");
    assert_eq!(rows[1].1, vec!["python3", "-c", "print(1)"], "run -> argv verbatim");
    assert_eq!(rows[2].0, "done");
    assert!(rows[2].1.is_empty(), "done -> []");
    // and the in-memory record mirrors the journal
    assert_eq!(result.steps[1].args, vec!["python3", "-c", "print(1)"]);
```

Also add one assertion to the file's existing grant-violation test (the one whose `run` is refused): the refused row's `args` equals the refused argv.

- [ ] **Step 5: Run, expect failure** — `cargo test -p bloomery-daemon --test task_loop_test 2>&1 | tail -5` → `no field args`.

- [ ] **Step 6: Implement in `task_loop.rs`**:

```rust
pub struct TaskStepRecord {
    pub step: u32,
    pub verb: String,
    pub outcome: String,
    pub content: String,
    pub failed: bool,
    /// Same list `Event::TaskStep::args` carries (turn-5 spec §3).
    pub args: Vec<String>,
}

struct StepReport<'a> {
    verb: &'a str,
    outcome: &'a str,
    content: &'a str,
    duration_ms: u64,
    failed: bool,
    args: Vec<String>,
}

/// The action's arguments as the journal records them (turn-5 spec §3).
/// Never the patch body: landing is re-derivable from the frozen fixture
/// and the scratch dir, and the body would bloat every journal.
fn action_args(action: &Action) -> Vec<String> {
    match action {
        Action::Read { path, lines: None } => vec![path.clone()],
        Action::Read { path, lines: Some((a, b)) } => vec![path.clone(), format!("lines={a}-{b}")],
        Action::Find { pattern, path } => vec![pattern.clone(), path.clone()],
        Action::Patch { path, .. } => vec![path.clone()],
        Action::Run { argv } => argv.clone(),
        Action::Done { .. } => Vec::new(),
    }
}
```

In `record_step`: `args: report.args.clone()` in the `Event::TaskStep { … }` and `args: report.args` in the `TaskStepRecord { … }` push (move last). At the four `StepReport {` sites: parse-failure → `args: Vec::new()`; `Done` → `args: Vec::new()`; demoted-verb refusal → `args: action_args(&action)`; executed → `args: action_args(&action)`. Fix the remaining `TaskStepRecord {` literals with `args: Vec::new()`.

- [ ] **Step 7: Run** — `cargo test -p bloomery-daemon --test task_loop_test` → green.

- [ ] **Step 8: Failing probe test** — append to `crates/bloomery-daemon/tests/codec_probe_test.rs`:

```rust
/// `(fixture, agent)` for every CodecFixture event, in journal order.
fn fixture_agents(events: &[Event]) -> Vec<(String, Option<String>)> {
    events.iter().filter_map(|e| match e {
        Event::CodecFixture { fixture, agent, .. } => Some((fixture.clone(), agent.clone())),
        _ => None,
    }).collect()
}

/// Turn-5 spec §3: every CodecFixture row names the agent that ran it, and
/// the sequence equals the AgentCreated sequence — the keyed join.
#[test]
fn codec_fixture_rows_carry_the_agent_that_ran_them() {
    // Drive the probe exactly as `one_fixture_row_per_fixture`-style tests in
    // this file do (same FakeSubstrate scripting, same set), then:
    let events = replay(&journal_path).unwrap();
    let created: Vec<String> = events.iter().filter_map(|e| match e {
        Event::AgentCreated { id, .. } => Some(id.clone()), _ => None }).collect();
    let rows = fixture_agents(&events);
    assert_eq!(rows.len(), created.len(), "one agent per fixture");
    for (i, (fixture, agent)) in rows.iter().enumerate() {
        assert_eq!(agent.as_deref(), Some(created[i].as_str()), "fixture {fixture} joins to its own agent");
    }
}
```

Add the same assertion block to an existing **refusal-probe** test in this file (one that runs a mixed set through `run_refusal_probe`), so both engines are pinned.

- [ ] **Step 9: Implement the key** — `pager/journal.rs`: `codec_fixture(…, expect: &str, agent: Option<&str>)` → `agent: agent.map(str::to_string)`; `pager/codec_gate.rs::journal_codec_fixture(…, expect: &str, agent: &str)` → passes `Some(agent)` (it keeps `#[allow(clippy::too_many_arguments)]` or gains it); `codec_probe/mod.rs:432` and `refuse.rs:296` pass `&agent.id` (the `agent` returned by `create_agent` earlier in each function). Update any test that calls `journal_codec_fixture` directly.

- [ ] **Step 10: Run everything** — `cargo test --workspace 2>&1 | tail -3`; `cargo clippy --workspace --all-targets -- -D warnings`.

- [ ] **Step 11: Commit**

```bash
git add -A crates/
git commit -m "feat(journal): TaskStep carries the action's args; CodecFixture names its agent (keyed join, turn-5 spec §3)"
```

---

### Task 4: `tools/evidence/recompute.py` — the tested recompute

**Files:**
- Create: `tools/evidence/__init__.py` (empty), `tools/evidence/journal.py`, `tools/evidence/endpoints.py`, `tools/evidence/recompute.py`, `tools/evidence/tests/__init__.py` (empty), `tools/evidence/tests/test_recompute_turn4.py`
- Reads (never modifies): `docs/superpowers/evidence/2026-08-21-flywheel4-{g4,g5}-{journal,tasks}.jsonl`, `2026-08-21-g5v4-{flywheel3,stock14b}-{journal,tasks}.jsonl`, `crates/bloomery-daemon/fixtures/codec-tasks-v4-mixed.toml`, `codec-tasks-v1.toml`

**Interfaces:**
- Produces: CLI `python3 -m tools.evidence.recompute --journal J --tasks T --g5-fixtures codec-tasks-v4-mixed.toml [--g4-set codec-tasks-v1] [--json out.json]` printing a JSON report (schema below); library functions named in the code.

Report schema (exact keys; Tasks 6 and 11 paste from it):

```json
{"join": {"mode": "keyed|ordinal", "keyed_equals_ordinal": true, "fixtures": 52, "groups": 52, "violations": []},
 "g4": {"set": "codec-tasks-v1", "landed": 20, "n": 20, "wilson95": [0.8388748419471806, 1.0], "provisional": false, "pass": true, "journaled_verdict_matches": true},
 "g5": {"set": "codec-tasks-v4-mixed",
        "patch": {"landed": 16, "n": 16, "wilson95": [0.806, 1.0], "provisional": false, "pass": true},
        "refuse": {"landed": 16, "n": 16, "wilson95": [0.806, 1.0], "provisional": false, "pass": true},
        "journaled_verdict_matches": true},
 "composition": {"find": [6,6], "run": [5,5], "plain": [5,5], "defect-absent": [6,6], "missing-target": [5,5], "symptom-mismatch": [5,5]},
 "endpoints": {"productive_find": [6,6], "find_usage": [6,6], "malformed_find": [0,6], "run_before_done": [5,5], "any_run": [5,5], "productive_run": [5,5],
               "reason_grounding": {"eligible": 11, "landed_eligible": 11, "measured_rows": 4, "unmeasured_rows": 7, "grounded": 6, "spans": 6}},
 "grant_violation_rows": 0,
 "verb_histogram": {"done": 52, "find": 6, "patch": 36, "read": 52, "run": 5}}
```

- [ ] **Step 1: Failing tests** — `tools/evidence/tests/test_recompute_turn4.py` (stdlib `unittest`; every number below was re-derived from the committed journals with `jq` on 2026-08-22 and matches the committed evidence):

```python
"""The recompute tool must reproduce the committed turn-4 evidence exactly."""
import json
import unittest
from pathlib import Path

from tools.evidence.recompute import recompute

ROOT = Path(__file__).resolve().parents[3]
EV = ROOT / "docs/superpowers/evidence"
FIX = ROOT / "crates/bloomery-daemon/fixtures"
V4 = FIX / "codec-tasks-v4-mixed.toml"


def run(tag):
    return recompute(journal=EV / f"2026-08-21-{tag}-journal.jsonl",
                     tasks=EV / f"2026-08-21-{tag}-tasks.jsonl",
                     g5_fixtures=V4, g4_set="codec-tasks-v1")


class Flywheel4Battery(unittest.TestCase):
    def test_boot1_g4_only(self):
        r = run("flywheel4-g4")
        self.assertEqual((r["g4"]["landed"], r["g4"]["n"]), (20, 20))
        self.assertAlmostEqual(r["g4"]["wilson95"][0], 0.8388748419471806, places=12)
        self.assertFalse(r["g4"]["provisional"])
        self.assertTrue(r["g4"]["journaled_verdict_matches"])
        self.assertEqual((r["join"]["fixtures"], r["join"]["groups"]), (20, 20))
        self.assertEqual(r["join"]["violations"], [])
        self.assertIsNone(r["g5"])
        self.assertEqual(r["verb_histogram"], {"done": 20, "patch": 20, "read": 20})

    def test_boot2_g5(self):
        r = run("flywheel4-g5")
        self.assertEqual(r["join"]["mode"], "ordinal")
        self.assertEqual(r["join"]["violations"], [])
        self.assertEqual((r["g4"]["landed"], r["g4"]["n"]), (20, 20))
        self.assertEqual((r["g5"]["patch"]["landed"], r["g5"]["refuse"]["landed"]), (16, 16))
        self.assertFalse(r["g5"]["patch"]["provisional"]); self.assertFalse(r["g5"]["refuse"]["provisional"])
        self.assertAlmostEqual(r["g5"]["patch"]["wilson95"][0], 0.8063923194655636, places=12)
        self.assertTrue(r["g5"]["journaled_verdict_matches"])
        self.assertEqual(r["composition"], {"find": [6, 6], "run": [5, 5], "plain": [5, 5],
                                            "defect-absent": [6, 6], "missing-target": [5, 5], "symptom-mismatch": [5, 5]})
        e = r["endpoints"]
        self.assertEqual(e["productive_find"], [6, 6]); self.assertEqual(e["find_usage"], [6, 6])
        self.assertEqual(e["malformed_find"], [0, 6]); self.assertEqual(e["run_before_done"], [5, 5])
        self.assertEqual(e["any_run"], [5, 5]); self.assertEqual(e["productive_run"], [5, 5])
        self.assertEqual(e["reason_grounding"], {"eligible": 11, "landed_eligible": 11, "measured_rows": 4,
                                                 "unmeasured_rows": 7, "grounded": 6, "spans": 6})
        self.assertEqual(r["grant_violation_rows"], 0)
        self.assertEqual(r["verb_histogram"], {"done": 52, "find": 6, "patch": 36, "read": 52, "run": 5})


class G5v4Baselines(unittest.TestCase):
    def test_flywheel3_at_v4(self):
        r = run("g5v4-flywheel3")
        self.assertEqual((r["g4"]["landed"], r["g5"]["patch"]["landed"], r["g5"]["refuse"]["landed"]), (20, 15, 16))
        self.assertTrue(r["g5"]["patch"]["provisional"]); self.assertFalse(r["g5"]["refuse"]["provisional"])
        self.assertEqual(r["composition"], {"find": [5, 6], "run": [5, 5], "plain": [5, 5],
                                            "defect-absent": [6, 6], "missing-target": [5, 5], "symptom-mismatch": [5, 5]})
        e = r["endpoints"]
        self.assertEqual(e["productive_find"], [5, 6]); self.assertEqual(e["find_usage"], [6, 6])
        self.assertEqual(e["malformed_find"], [0, 6]); self.assertEqual(e["run_before_done"], [5, 5])
        self.assertEqual(e["any_run"], [5, 5]); self.assertEqual(e["productive_run"], [0, 5])
        self.assertEqual(e["reason_grounding"], {"eligible": 11, "landed_eligible": 11, "measured_rows": 5,
                                                 "unmeasured_rows": 6, "grounded": 16, "spans": 19})
        self.assertEqual(r["grant_violation_rows"], 5)
        self.assertEqual(r["verb_histogram"], {"done": 52, "find": 6, "patch": 35, "read": 51, "run": 5})

    def test_stock14b_at_v4(self):
        r = run("g5v4-stock14b")
        self.assertEqual((r["g4"]["landed"], r["g5"]["patch"]["landed"], r["g5"]["refuse"]["landed"]), (6, 5, 8))
        self.assertFalse(r["g4"]["pass"]); self.assertFalse(r["g5"]["patch"]["pass"]); self.assertFalse(r["g5"]["refuse"]["pass"])
        self.assertEqual(r["composition"], {"find": [0, 6], "run": [2, 5], "plain": [3, 5],
                                            "defect-absent": [4, 6], "missing-target": [1, 5], "symptom-mismatch": [3, 5]})
        e = r["endpoints"]
        self.assertEqual(e["productive_find"], [0, 6]); self.assertEqual(e["find_usage"], [6, 6])
        self.assertEqual(e["run_before_done"], [0, 5]); self.assertEqual(e["productive_run"], [0, 5])
        self.assertEqual(e["reason_grounding"], {"eligible": 11, "landed_eligible": 7, "measured_rows": 0,
                                                 "unmeasured_rows": 7, "grounded": 0, "spans": 0})
        self.assertEqual(r["grant_violation_rows"], 42)
        self.assertEqual(r["verb_histogram"], {"done": 32, "find": 9, "patch": 94, "read": 68})


if __name__ == "__main__":
    unittest.main()
```

- [ ] **Step 2: Run, expect ImportError** — `python3 -m unittest tools.evidence.tests.test_recompute_turn4 -v 2>&1 | tail -3`.

- [ ] **Step 3: `tools/evidence/journal.py`** (≤120 lines):

```python
"""Journal loading and the CodecFixture <-> TaskStep join (turn-5 spec §3).

Two joins: KEYED (`CodecFixture.agent == TaskStep.id`, rows journaled from
turn 5 on) and ORDINAL (the turn-3/4 method: CodecFixture rows in journal
order <-> TaskStep groups in first-seen order, with three validations). When
rows carry `agent`, both run and must agree; older journals get ordinal only.
"""
from __future__ import annotations

import json
from dataclasses import dataclass, field
from pathlib import Path


def load_rows(path: Path) -> list[dict]:
    with Path(path).open() as f:
        return [json.loads(line) for line in f if line.strip()]


def fixture_rows(journal: list[dict]) -> list[dict]:
    return [r for r in journal if r.get("event") == "CodecFixture"]


def task_groups(tasks: list[dict]) -> dict[str, list[dict]]:
    groups: dict[str, list[dict]] = {}
    for r in tasks:
        if r.get("event") != "TaskStep":
            continue
        groups.setdefault(r["id"], []).append(r)
    return groups


@dataclass
class Joined:
    fixture: dict
    steps: list[dict]


@dataclass
class JoinReport:
    mode: str
    keyed_equals_ordinal: bool | None
    fixtures: int
    groups: int
    violations: list[str] = field(default_factory=list)


def _ordinal(fixtures: list[dict], groups: dict[str, list[dict]]) -> tuple[list[Joined], list[str]]:
    violations: list[str] = []
    ids = list(groups)  # dict preserves first-seen order
    if len(ids) != len(fixtures):
        violations.append(f"group count {len(ids)} != CodecFixture count {len(fixtures)}")
    joined: list[Joined] = []
    prev_stamp = None
    for i, fx in enumerate(fixtures):
        steps = groups[ids[i]] if i < len(ids) else []
        if len(steps) != fx.get("steps"):
            violations.append(f"{fx['fixture']}: group length {len(steps)} != steps {fx.get('steps')}")
        stamp = fx.get("epoch_ms")
        if stamp is not None:
            for s in steps:
                if s.get("epoch_ms") is not None and not ((prev_stamp is None or s["epoch_ms"] >= prev_stamp) and s["epoch_ms"] <= stamp):
                    violations.append(f"{fx['fixture']}: step {s.get('step')} epoch_ms outside its fixture bracket")
            prev_stamp = stamp
        joined.append(Joined(fx, steps))
    return joined, violations


def _keyed(fixtures: list[dict], groups: dict[str, list[dict]]) -> tuple[list[Joined], list[str]]:
    violations: list[str] = []
    joined: list[Joined] = []
    for fx in fixtures:
        steps = groups.get(fx.get("agent"), [])
        if fx.get("agent") is None:
            violations.append(f"{fx['fixture']}: no agent key")
        if len(steps) != fx.get("steps"):
            violations.append(f"{fx['fixture']}: keyed group length {len(steps)} != steps {fx.get('steps')}")
        joined.append(Joined(fx, steps))
    return joined, violations


def join(journal: list[dict], tasks: list[dict]) -> tuple[list[Joined], JoinReport]:
    fixtures = fixture_rows(journal)
    groups = task_groups(tasks)
    ordinal, ov = _ordinal(fixtures, groups)
    if fixtures and all(fx.get("agent") for fx in fixtures):
        keyed, kv = _keyed(fixtures, groups)
        same = [(a.fixture["fixture"], [s["step"] for s in a.steps]) for a in keyed] == \
               [(b.fixture["fixture"], [s["step"] for s in b.steps]) for b in ordinal]
        report = JoinReport("keyed", same, len(fixtures), len(groups), kv + ([] if same else ["keyed != ordinal"]))
        return keyed, report
    return ordinal, JoinReport("ordinal", None, len(fixtures), len(groups), ov)
```

- [ ] **Step 4: `tools/evidence/endpoints.py`** (≤220 lines):

```python
"""Gate arithmetic and the secondary endpoints (g5v4 protocol §4–§5).

Wilson 95% is a verbatim port of crates/bloomery-core/src/stats.rs;
`is_provisional` is scoring.rs's strict two-sided straddle; `gate_decision`
is `landed*5 >= n*4`.
"""
from __future__ import annotations

import math
import re
import tomllib
from collections import Counter
from pathlib import Path

from .journal import Joined

Z = 1.959963984540054
THRESHOLD = 0.80
SPAN = re.compile(r"`([^`]+)`")
RAN_EXIT0 = re.compile(r"^ran .* exit 0$")

PATCH_SHAPES = ("find", "run", "plain")
REFUSE_FAMILIES = ("defect-absent", "missing-target", "symptom-mismatch")


def wilson95(passes: int, n: int) -> tuple[float, float]:
    if n == 0:
        return (0.0, 1.0)
    phat = passes / n
    denom = 1.0 + Z * Z / n
    centre = phat + Z * Z / (2.0 * n)
    margin = Z * math.sqrt((phat * (1.0 - phat) + Z * Z / (4.0 * n)) / n)
    return (max((centre - margin) / denom, 0.0), min((centre + margin) / denom, 1.0))


def is_provisional(lo: float, hi: float) -> bool:
    return lo < THRESHOLD < hi


def gate_decision(landed: int, n: int) -> bool:
    return landed * 5 >= n * 4


def leg(landed: int, n: int) -> dict:
    lo, hi = wilson95(landed, n)
    return {"landed": landed, "n": n, "wilson95": [lo, hi],
            "provisional": is_provisional(lo, hi), "pass": gate_decision(landed, n)}


def shape_of(name: str) -> str | None:
    for s in PATCH_SHAPES:
        if f"-patch-{s}-" in name:
            return s
    for fam in REFUSE_FAMILIES:
        if fam in name:
            return fam
    return None


def composition(rows: list[Joined]) -> dict[str, list[int]]:
    out = {k: [0, 0] for k in PATCH_SHAPES + REFUSE_FAMILIES}
    for j in rows:
        s = shape_of(j.fixture["fixture"])
        if s is None:
            continue
        out[s][1] += 1
        out[s][0] += int(bool(j.fixture["landed"]))
    return out


def verbs(j: Joined) -> list[str]:
    return [s["verb"] for s in j.steps]


def load_fixture_files(toml_path: Path) -> dict[str, dict]:
    doc = tomllib.loads(Path(toml_path).read_text())
    return {fx["name"]: fx for fx in doc["fixture"]}


def reason_grounding(rows: list[Joined], fixtures: dict[str, dict]) -> dict:
    eligible = [j for j in rows if j.fixture.get("expect") == "refuse"
                and "missing-target" not in j.fixture["fixture"]]
    landed = [j for j in eligible if j.fixture["landed"]]
    measured = unmeasured = grounded = spans = 0
    for j in landed:
        done = [s for s in j.steps if s["verb"] == "done"]
        text = done[-1]["outcome"] if done else ""
        found = SPAN.findall(text)
        if not found:
            unmeasured += 1
            continue
        measured += 1
        fx = fixtures[j.fixture["fixture"]]
        contents = [f.get("contents", "") for f in fx.get("file", [])]
        paths = [f.get("path", "") for f in fx.get("file", [])]
        for span in found:
            spans += 1
            if any(span in c for c in contents) or any(span in p for p in paths):
                grounded += 1
    return {"eligible": len(eligible), "landed_eligible": len(landed), "measured_rows": measured,
            "unmeasured_rows": unmeasured, "grounded": grounded, "spans": spans}


def endpoints(rows: list[Joined], fixtures: dict[str, dict]) -> dict:
    find_rows = [j for j in rows if shape_of(j.fixture["fixture"]) == "find"]
    run_rows = [j for j in rows if shape_of(j.fixture["fixture"]) == "run"]

    def productive_find(j): return "find" in verbs(j) and bool(j.fixture["landed"])
    def find_usage(j): return "find" in verbs(j)
    def malformed(j): return "?" in verbs(j)
    def run_before_done(j):
        v = verbs(j)
        return "run" in v and "done" in v and v.index("run") < len(v) - 1 - v[::-1].index("done")
    def any_run(j): return "run" in verbs(j)
    def productive_run(j):
        return bool(j.fixture["landed"]) and any(s["verb"] == "run" and RAN_EXIT0.match(s["outcome"]) for s in j.steps)

    return {
        "productive_find": [sum(map(productive_find, find_rows)), len(find_rows)],
        "find_usage": [sum(map(find_usage, find_rows)), len(find_rows)],
        "malformed_find": [sum(map(malformed, find_rows)), len(find_rows)],
        "run_before_done": [sum(map(run_before_done, run_rows)), len(run_rows)],
        "any_run": [sum(map(any_run, run_rows)), len(run_rows)],
        "productive_run": [sum(map(productive_run, run_rows)), len(run_rows)],
        "reason_grounding": reason_grounding(rows, fixtures),
    }


def grant_violation_rows(tasks: list[dict]) -> int:
    return sum(1 for r in tasks if r.get("event") == "TaskStep" and str(r.get("outcome", "")).startswith("grant violation"))


def verb_histogram(tasks: list[dict]) -> dict[str, int]:
    return dict(sorted(Counter(r["verb"] for r in tasks if r.get("event") == "TaskStep").items()))
```

- [ ] **Step 5: `tools/evidence/recompute.py`** (≤150 lines):

```python
"""Recompute a boot's G4/G5 verdicts and secondary endpoints from its committed
journals (turn-5 spec §3). It REPORTS; the daemon DECIDES — this tool is never
on the gate path. Usage:

  python3 -m tools.evidence.recompute --journal J.jsonl --tasks T.jsonl \
      --g5-fixtures crates/bloomery-daemon/fixtures/codec-tasks-v4-mixed.toml \
      [--g4-set codec-tasks-v1] [--json out.json]
"""
from __future__ import annotations

import argparse
import json
from pathlib import Path

from . import endpoints as ep
from .journal import join, load_rows


def _journaled_g4(journal, set_name):
    for r in journal:
        if r.get("event") == "CodecVerdict" and r.get("fixture_set") == set_name:
            return r
    return None


def _journaled_g5(journal, set_name):
    for r in journal:
        if r.get("event") == "CodecVerdictMixed" and r.get("fixture_set") == set_name:
            return r
    return None


def recompute(journal: Path, tasks: Path, g5_fixtures: Path | None, g4_set: str = "codec-tasks-v1") -> dict:
    jrows, trows = load_rows(journal), load_rows(tasks)
    joined, jr = join(jrows, trows)
    report = {"join": {"mode": jr.mode, "keyed_equals_ordinal": jr.keyed_equals_ordinal,
                       "fixtures": jr.fixtures, "groups": jr.groups, "violations": jr.violations}}

    g4_rows = [j for j in joined if j.fixture["fixture_set"] == g4_set]
    if g4_rows:
        g4 = ep.leg(sum(bool(j.fixture["landed"]) for j in g4_rows), len(g4_rows))
        jv = _journaled_g4(jrows, g4_set)
        g4["journaled_verdict_matches"] = bool(jv) and (jv["landed"], jv["n"], jv["provisional"]) == (g4["landed"], g4["n"], g4["provisional"])
        report["g4"] = {"set": g4_set, **g4}
    else:
        report["g4"] = None

    g5_set = Path(g5_fixtures).stem if g5_fixtures else None
    g5_rows = [j for j in joined if g5_set and j.fixture["fixture_set"] == g5_set]
    if g5_rows:
        fx = ep.load_fixture_files(g5_fixtures)
        patch = [j for j in g5_rows if j.fixture.get("expect") == "patch"]
        refuse = [j for j in g5_rows if j.fixture.get("expect") == "refuse"]
        g5 = {"set": g5_set,
              "patch": ep.leg(sum(bool(j.fixture["landed"]) for j in patch), len(patch)),
              "refuse": ep.leg(sum(bool(j.fixture["landed"]) for j in refuse), len(refuse))}
        jv = _journaled_g5(jrows, g5_set)
        g5["journaled_verdict_matches"] = bool(jv) and (
            (jv["patch_landed"], jv["refuse_landed"], jv["patch_provisional"], jv["refuse_provisional"]) ==
            (g5["patch"]["landed"], g5["refuse"]["landed"], g5["patch"]["provisional"], g5["refuse"]["provisional"]))
        report["g5"] = g5
        report["composition"] = ep.composition(g5_rows)
        report["endpoints"] = ep.endpoints(g5_rows, fx)
    else:
        report["g5"] = None
    report["grant_violation_rows"] = ep.grant_violation_rows(trows)
    report["verb_histogram"] = ep.verb_histogram(trows)
    return report


def main(argv=None) -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--journal", required=True, type=Path)
    ap.add_argument("--tasks", required=True, type=Path)
    ap.add_argument("--g5-fixtures", type=Path, default=None)
    ap.add_argument("--g4-set", default="codec-tasks-v1")
    ap.add_argument("--json", type=Path, default=None)
    a = ap.parse_args(argv)
    report = recompute(a.journal, a.tasks, a.g5_fixtures, a.g4_set)
    text = json.dumps(report, indent=2)
    if a.json:
        a.json.write_text(text + "\n")
    print(text)
    return 0 if not report["join"]["violations"] else 2


if __name__ == "__main__":
    raise SystemExit(main())
```

- [ ] **Step 6: Run the tests** — `python3 -m unittest tools.evidence.tests.test_recompute_turn4 -v` → all 4 pass. **If any number differs, do not tune the tool to pass: open the corresponding committed evidence section, find which definition the evidence used, and report the discrepancy in the task report** (the evidence is the record; the tool must implement its definitions). Also confirm the CLI: `python3 -m tools.evidence.recompute --journal docs/superpowers/evidence/2026-08-21-flywheel4-g5-journal.jsonl --tasks docs/superpowers/evidence/2026-08-21-flywheel4-g5-tasks.jsonl --g5-fixtures crates/bloomery-daemon/fixtures/codec-tasks-v4-mixed.toml | head -20`.

- [ ] **Step 7: Wire into the factory suite's discovery** — confirm `python3 -m unittest discover -s tools -t . 2>&1 | tail -3` finds both `tools/flywheel/tests` and `tools/evidence/tests` (add `tools/__init__.py` if discovery needs it; it exists already for `tools.flywheel`). Add a short `tools/evidence/README.md` (20 lines: purpose, CLI, the "reports, never decides" line, the turn-4 pins).

- [ ] **Step 8: Commit**

```bash
git add tools/evidence
git commit -m "feat(evidence): tools/evidence/recompute — keyed+ordinal join, Wilson, endpoints; pinned to the committed turn-4 journals"
```

---

### Task 5: Final review, PR, merge, featured build

- [ ] **Step 1: Whole-branch review** — `git diff master...HEAD --stat`; dispatch the code-reviewer on the diff (focus: the two charge sites are the only behavior change in the pager; no frozen file touched — `git diff master...HEAD --name-only | grep -E "fixtures/codec-tasks|evidence/2026-08-(1|20|21)" ` must print nothing; journal compat pins present; recompute tests green).
- [ ] **Step 2: Suites** — `cargo test --workspace 2>&1 | tail -3`; `cargo clippy --workspace --all-targets -- -D warnings`; `python3 -m unittest discover -s tools -t . 2>&1 | tail -3`; `~/flywheel-venv/bin/python -m unittest discover -s tools/flywheel/tests -t . 2>&1 | tail -3`.
- [ ] **Step 3: Push + PR** — `git push -u origin turn5-ride-alongs`; `gh pr create --title "turn-5 ride-alongs: hybrid-aware geometry, keyed task journal, recompute tool" --body "<summary of Tasks 1–4; spec link>"`; note the PR number.
- [ ] **Step 4: Merge (Brice's go or delegated)** — from the MAIN checkout on `master`: `git checkout master && gh pr merge <N> --merge --delete-branch && git pull --ff-only && git rev-list --left-right --count origin/master...master` → `0 0`. Remove the worktree: `git worktree remove .worktrees/ride-alongs`.
- [ ] **Step 5: Featured build LAST** — `cargo build --release -p bloomery-daemon --features vulkan` and `nm -C target/release/bloomery-daemon | grep -c ggml_vulkan` > 0. **No `cargo test` after this.**

---

## Baselines — on master, HUMAN-GATED

### Task 6: Two identical boots of the untrained base at the fixed geometry

**STOP for Brice's go before the first boot.**

**Files:**
- Create: `docs/superpowers/evidence/<YYYY-MM-DD>-g5v4-reap48-baselines.md`, `<YYYY-MM-DD>-g5v4-reap48-boot{1,2}-journal.jsonl`, `…-boot{1,2}-tasks.jsonl`, `…-boot{1,2}-recompute.json`
- Local, not committed: `target/reap48-base-live/boot{1,2}/bloomery.toml`, `status-boot{1,2}.json`

- [ ] **Step 1: Pre-register in the doc BEFORE the first boot, then commit.** Sections: `## 1. Expectations (PRE-REGISTERED)` — the line's floor; spike numbers quoted (20/20 · 13/16 prov · 9/16; grant-violation rows 5; `done` 45 on 32 fixtures) as *expectations superseded on measurement*; **"boot 1 is the anchor; boot 2 is corroboration; a difference is a box finding, never a choice"**; the fixed-geometry consequences to be recorded (`kv_per_token` 20,480, `recurrent_state_bytes` 65,863,680, window ≈108.7k at no override, decode tps expected below the spike's 116.7 — reported, not gated); reporting discipline (floor and flag separate; `decided by construction` never used of a score; no cross-envelope sentence). `## 2. Preflight` (GPU free, no daemon — `ps -eo pid,comm | grep -w bloomery-daemon`, idle ollama reported, disk, master sha, featured-binary mtime + `nm -C … ggml_vulkan`). Commit `docs: pre-register the REAP-48-ours baselines (two boots, boot 1 anchor) before the first boot`.

- [ ] **Step 2: Boot config** (`target/reap48-base-live/boot1/bloomery.toml`; boot 2 identical except `data_dir` and `port` 8398):

```toml
# REAP-48-ours UNTRAINED baseline, boot 1 (anchor). Fixed geometry (turn-5
# ride-along 1): no kv_per_token_bytes override; ctx_overhead_mib 512.
port = 8399
data_dir = "/home/brice/workspace/bloomery/target/reap48-base-live/boot1/data"
tasks_enabled = true
ctx_overhead_mib = 512

[models."qwen36-reap48-ours"]
path = "/home/brice/models/gguf/qwen36-reap48-ours-Q4_K_M.gguf"
envelope = "v4"
g5_probe = true

[tier]
name = "enthusiast-16gb"
emulated = false

[assay]
enabled = true
python = "python3"
probe_timeout_secs = 1800
```

- [ ] **Step 3: Boot 1** — `cd ~/workspace/bloomery && PYTHONPATH=/home/brice/workspace/assay/src setsid nohup target/release/bloomery-daemon --config target/reap48-base-live/boot1/bloomery.toml > target/reap48-base-live/boot1/daemon.log 2>&1 &` then `echo $! > target/reap48-base-live/boot1/pid`; `readlink /proc/$(cat …/pid)/exe` must be the featured binary; wait for `Post` then the two verdict rows (the boot journal is `target/reap48-base-live/boot1/data/journal/boot-<epoch>.jsonl`, the task journal `…/data/journal/tasks.jsonl`; `grep -c '"event":"CodecVerdictMixed"' target/reap48-base-live/boot1/data/journal/boot-*.jsonl` → 1; allow ~20–30 min); `curl -s localhost:8399/status > target/reap48-base-live/boot1/status-boot1.json`; assert `jq -r '.models[0].digest' status-boot1.json` == `90e2181e8c3175c7f59f911ee70dfcc58cd068977fc657be3a4101d041f591a5` (else BLOCKED — stop); also record `.models[0].kv_per_token` (expect 20480) and `.models[0].recurrent_state_bytes` (expect 65863680). Stop the daemon: `kill $(cat target/reap48-base-live/boot1/pid)` after verifying `readlink`; wait until the PID is gone.
- [ ] **Step 4: Boot 2** — same, port 8398, `boot2/`.
- [ ] **Step 5: Copy journals + recompute** — `cp target/reap48-base-live/boot1/data/journal/boot-*.jsonl docs/superpowers/evidence/<date>-g5v4-reap48-boot1-journal.jsonl` (exactly one `boot-*.jsonl` per fresh `data_dir`) and `cp target/reap48-base-live/boot1/data/journal/tasks.jsonl docs/superpowers/evidence/<date>-g5v4-reap48-boot1-tasks.jsonl`; same for boot 2; then `python3 -m tools.evidence.recompute --journal …-boot1-journal.jsonl --tasks …-boot1-tasks.jsonl --g5-fixtures crates/bloomery-daemon/fixtures/codec-tasks-v4-mixed.toml --json docs/superpowers/evidence/<date>-g5v4-reap48-boot1-recompute.json` (exit 0; `join.mode == "keyed"`, `keyed_equals_ordinal true`, `violations []`, both `journaled_verdict_matches true`); same for boot 2.
- [ ] **Step 6: Evidence doc** — `## 3. Method`, `## 4. Boot 1 (anchor)`, `## 5. Boot 2 (corroboration)`, `## 6. Both boots side by side` (identical or not — stated), `## 7. Serving facts at the fixed geometry` (window_tokens from `AgentCreated`, kv_per_token, recurrent_state_bytes, decode/prefill tps from the POST profile, peak VRAM if captured), `## 8. Scorecard vs §1`, `## 9. Caveats`, `## 10. Committed artifacts`. **Every count, composition, endpoint, grant-violation number and verb histogram is pasted from the recompute JSON; anatomy sentences (trajectory shapes, `done` > fixtures, out-of-slice reads) are computed by a short script whose output is quoted — never written from memory.** Never re-run a boot. Commit `docs: REAP-48-ours baselines under envelope-v4 at the fixed geometry — two boots, boot 1 anchor`.
- [ ] **Step 7: Evidence review** — an independent reviewer re-derives the headline counts from the JSONL (jq) and checks every prose number against the recompute JSON; fix round if needed; commit; push master; `git rev-list --left-right --count origin/master...master` → `0 0`.

---

## Branch 2 — `flywheel5-turn5`

Worktree: `git worktree add .worktrees/flywheel5 -b flywheel5-turn5 master` (after Task 6 is on master).

### Task 7: `train_common.py` (refactor) + `train_moe.py` (the recipe) + tests

**Files:**
- Create: `tools/flywheel/train_common.py`, `tools/flywheel/train_moe.py`, `tools/flywheel/tests/train_fixture.py`, `tools/flywheel/tests/test_train_common.py`, `tools/flywheel/tests/test_train_moe.py`
- Modify: `tools/flywheel/train.py` (imports from `train_common`; header note; no hyperparameter/seed change)

**Interfaces:**
- Produces (`train_common`): `MAX_SEQ = 4096`, `LORA_R = 16`, `LORA_ALPHA = 32`, `PROCEDURE_SEED = 20260816`, `load_pairs(corpus: Path, fingerprint: Path) -> tuple[list, list]`, `class PairDataset`, `collate_single(batch)`, `tokenize_fn(tokenizer)`, `assert_batch_shape(tokenizer, ds)`, `training_args(out: Path, max_steps: int = -1, **overrides) -> TrainingArguments`, `PINNED_ARGS: dict` (the exact kwargs).
- Produces (`train_moe`): `TARGET_MODULES` (12 names), `apply_lora(model, seed=PROCEDURE_SEED) -> PeftModel`, `assert_frozen(model) -> dict(trainable=int, total=int)`, `main(argv=None)`.

- [ ] **Step 1: Failing `train_common` tests** — `tools/flywheel/tests/train_fixture.py`:

```python
"""A mini tokenizer that can spell `</action>` as one token, for the
trainer tests (the prune fixture's tokenizer cannot — WordLevel over t0..tN
with a punctuation-splitting pre-tokenizer)."""
from tokenizers import Tokenizer, models, pre_tokenizers, decoders
from transformers import PreTrainedTokenizerFast

from .prune_fixture import VOCAB_SIZE  # the mini model's vocab size

ACTION_END = "</action>"


def build_action_tokenizer() -> PreTrainedTokenizerFast:
    vocab = {f"t{i}": i for i in range(VOCAB_SIZE - 2)}
    vocab[ACTION_END] = VOCAB_SIZE - 2
    vocab["<unk>"] = VOCAB_SIZE - 1
    backend = Tokenizer(models.WordLevel(vocab, unk_token="<unk>"))
    backend.pre_tokenizer = pre_tokenizers.WhitespaceSplit()
    backend.decoder = decoders.WordPiece(prefix="", cleanup=False)
    return PreTrainedTokenizerFast(tokenizer_object=backend, unk_token="<unk>")


def tiny_corpus(n: int = 6):
    """Rows in the corpus shape: prompt, completion ending at </action>, meta.task_id."""
    return [{"prompt": f"t1 t2 t{i} ", "completion": f"t4 t5 t{i}\n{ACTION_END}", "meta": {"task_id": f"task-{i}"}}
            for i in range(n)]
```

`tools/flywheel/tests/test_train_common.py`:

```python
import json, tempfile, unittest
from pathlib import Path

try:
    import torch  # noqa: F401
    from tools.flywheel import train_common as tc
    from tools.flywheel.tests.train_fixture import build_action_tokenizer, tiny_corpus, ACTION_END
    HAVE_TORCH = True
except Exception:  # stdlib python: skip cleanly like the prune tests
    HAVE_TORCH = False


@unittest.skipUnless(HAVE_TORCH, "needs ~/flywheel-venv")
class PinnedRecipe(unittest.TestCase):
    def test_constants_are_turn4_values(self):
        self.assertEqual((tc.MAX_SEQ, tc.LORA_R, tc.LORA_ALPHA, tc.PROCEDURE_SEED), (4096, 16, 32, 20260816))

    def test_training_args_are_turn4_values(self):
        a = tc.training_args(Path("/tmp/x"))
        self.assertEqual((a.num_train_epochs, a.per_device_train_batch_size, a.per_device_eval_batch_size,
                          a.gradient_accumulation_steps, a.learning_rate, a.lr_scheduler_type, a.warmup_steps,
                          a.logging_steps, a.eval_strategy, a.eval_steps, a.save_strategy, a.bf16, a.seed, a.max_steps),
                         (2, 1, 1, 8, 2e-4, "cosine", 20, 10, "steps", 100, "no", True, 20260816, -1))
        self.assertEqual(a.report_to, [])

    def test_tokenize_masks_prompt_and_ends_at_action_close(self):
        tok = build_action_tokenizer()
        row = tc.tokenize_fn(tok)(tiny_corpus(1)[0])
        n_prompt = len(tok("t1 t2 t0 ", add_special_tokens=True)["input_ids"])
        self.assertEqual(row["labels"][:n_prompt], [-100] * n_prompt)
        self.assertEqual(row["labels"][n_prompt:], row["input_ids"][n_prompt:])
        self.assertTrue(tok.decode(row["input_ids"][-4:]).rstrip().endswith(ACTION_END))
        self.assertEqual(len(row["attention_mask"]), len(row["input_ids"]))

    def test_load_pairs_filters_val_split(self):
        with tempfile.TemporaryDirectory() as d:
            corpus = Path(d) / "c.jsonl"; fp = Path(d) / "f.json"
            corpus.write_text("".join(json.dumps(r) + "\n" for r in tiny_corpus(6)))
            fp.write_text(json.dumps({"val_split_ids": ["task-1", "task-4"]}))
            train, val = tc.load_pairs(corpus, fp)
            self.assertEqual((len(train), len(val)), (4, 2))
```

- [ ] **Step 2: Run, expect ImportError** — `~/flywheel-venv/bin/python -m unittest tools.flywheel.tests.test_train_common -v 2>&1 | tail -3`.

- [ ] **Step 3: Write `tools/flywheel/train_common.py`** — move `load_pairs`, `PairDataset`, `collate_single`, `tokenize_fn`, `assert_batch_shape` and the constants out of `train.py` **byte-identical in body**, add:

```python
PINNED_ARGS = dict(num_train_epochs=2, per_device_train_batch_size=1, per_device_eval_batch_size=1,
                   gradient_accumulation_steps=8, learning_rate=2e-4, lr_scheduler_type="cosine",
                   warmup_steps=20, logging_steps=10, eval_strategy="steps", eval_steps=100,
                   save_strategy="no", bf16=True, report_to=[], seed=PROCEDURE_SEED)


def training_args(out: Path, max_steps: int = -1, **overrides):
    """Turn 1-4's TrainingArguments, verbatim. `overrides` exist ONLY for
    CPU smoke tests (bf16=False, use_cpu=True); a pre-registered run passes none."""
    from transformers import TrainingArguments
    kwargs = dict(PINNED_ARGS, output_dir=str(out), max_steps=max_steps)
    kwargs.update(overrides)
    return TrainingArguments(**kwargs)
```

Module docstring: "Binding rules shared by `train.py` (turns 1-4, unsloth QLoRA) and `train_moe.py` (turn 5, bf16 LoRA on qwen3_5_moe). Moved here 2026-08-22 without behaviour change; pinned by `tests/test_train_common.py`." Then edit `train.py`: replace the moved definitions with `from tools.flywheel.train_common import (MAX_SEQ, LORA_R, LORA_ALPHA, PROCEDURE_SEED, load_pairs, PairDataset, collate_single, tokenize_fn, assert_batch_shape, training_args)`; `TrainingArguments(...)` call becomes `targs = training_args(args.out, args.max_steps)`; `random_state=PROCEDURE_SEED`; add to the header: "**Turn 5 (2026-08-22):** the shared helpers moved to `train_common.py`; behaviour pinned by `tests/test_train_common.py`; no hyperparameter, seed, or code path of the recipe changed." Keep `TARGET_MODULES` (the 7-name 14B list) in `train.py`.

- [ ] **Step 4: Run** — `~/flywheel-venv/bin/python -m unittest tools.flywheel.tests.test_train_common -v` → 4 pass; `python3 -c "import ast,sys; ast.parse(open('tools/flywheel/train.py').read())"` (syntax; unsloth import is not exercised here).

- [ ] **Step 5: Failing `train_moe` tests** — `tools/flywheel/tests/test_train_moe.py`:

```python
import json, tempfile, unittest
from pathlib import Path

try:
    import torch
    from tools.flywheel import train_moe
    from tools.flywheel.tests.prune_fixture import build_mini_model
    from tools.flywheel.tests.train_fixture import build_action_tokenizer, tiny_corpus
    HAVE_TORCH = True
except Exception:
    HAVE_TORCH = False

EXPECTED_SUFFIXES = {"q_proj", "k_proj", "v_proj", "o_proj", "in_proj_qkv", "in_proj_z", "in_proj_b",
                     "in_proj_a", "out_proj", "gate_proj", "up_proj", "down_proj"}


@unittest.skipUnless(HAVE_TORCH, "needs ~/flywheel-venv")
class LoraTargets(unittest.TestCase):
    def test_targets_hit_attention_deltanet_and_shared_expert_only(self):
        m = train_moe.apply_lora(build_mini_model())
        wrapped = [n for n, mod in m.named_modules() if hasattr(mod, "lora_A")]
        self.assertTrue(wrapped)
        self.assertEqual({n.rsplit(".", 1)[-1] for n in wrapped}, EXPECTED_SUFFIXES)
        self.assertFalse([n for n in wrapped if ".experts." in n or n.endswith("mlp.gate")])
        self.assertTrue(all("shared_expert" in n for n in wrapped if n.endswith(("gate_proj", "up_proj", "down_proj"))))

    def test_experts_and_router_are_frozen(self):
        m = train_moe.apply_lora(build_mini_model())
        stats = train_moe.assert_frozen(m)
        for n, p in m.named_parameters():
            if ".experts." in n or n.endswith("mlp.gate.weight"):
                self.assertFalse(p.requires_grad, n)
        self.assertGreater(stats["trainable"], 0)
        self.assertLess(stats["trainable"] / stats["total"], 0.2)  # mini model; real: 0.0611%

    def test_same_seed_same_init(self):
        a = train_moe.apply_lora(build_mini_model(), seed=20260816)
        b = train_moe.apply_lora(build_mini_model(), seed=20260816)
        sa = {n: p for n, p in a.named_parameters() if "lora_" in n}
        for n, p in b.named_parameters():
            if "lora_" in n:
                self.assertTrue(torch.equal(p, sa[n]), n)


@unittest.skipUnless(HAVE_TORCH, "needs ~/flywheel-venv")
class CpuSmoke(unittest.TestCase):
    def test_two_steps_on_cpu_write_the_markers(self):
        with tempfile.TemporaryDirectory() as d:
            d = Path(d)
            base = d / "base"; build_mini_model().save_pretrained(base); build_action_tokenizer().save_pretrained(base)
            corpus = d / "c.jsonl"; fp = d / "f.json"
            corpus.write_text("".join(json.dumps(r) + "\n" for r in tiny_corpus(6)))
            fp.write_text(json.dumps({"val_split_ids": ["task-5"]}))
            out = d / "adapter"
            rc = train_moe.main(["--corpus", str(corpus), "--fingerprint", str(fp), "--base", str(base),
                                 "--out", str(out), "--max-steps", "2", "--device", "cpu", "--dtype", "float32"])
            self.assertEqual(rc, 0)
            self.assertTrue((out / "DONE").exists())
            self.assertEqual((out / "EXIT").read_text().strip(), "0")
            self.assertTrue((out / "adapter_config.json").exists())
            self.assertTrue((out / "tokenizer_config.json").exists())
```

- [ ] **Step 6: Run, expect ImportError** — `~/flywheel-venv/bin/python -m unittest tools.flywheel.tests.test_train_moe -v 2>&1 | tail -3`.

- [ ] **Step 7: Write `tools/flywheel/train_moe.py`**:

```python
"""Flywheel turn 5 — bf16 LoRA on the REAP-48-pruned Qwen3.6-35B-A3B hybrid MoE
(`Qwen3_5MoeForCausalLM`, text-only), the pre-registered rental recipe.

Governing documents:
- spec  docs/superpowers/specs/2026-08-22-flywheel5-turn5-design.md §4
- gates docs/superpowers/evidence/<date>-flywheel5-preregistration.md

What is inherited from turns 1-4 (train_common): raw text, no chat template,
completion-only loss, NO EOS (tail `</action>`), val split from the
fingerprint, TrainingArguments verbatim, the procedure seed 20260816.
What is forced by the architecture: bf16 LoRA via peft (unsloth does not
support qwen3_5_moe; bitsandbytes cannot quantize the fused 3-D expert
tensors); LoRA on attention + Gated-DeltaNet projections + the SHARED expert
only — routed experts are fused `nn.Parameter`s and the router a bare
parameter, so peft cannot wrap them; both are FROZEN and asserted so.
Unpacked, bs 1 (ruled 2026-08-22: naive packing leaks across the 30
recurrent layers' state).

Usage (turn 5, on the pod):
  python -m tools.flywheel.train_moe --corpus /workspace/flywheel5/corpus.jsonl \
      --fingerprint /workspace/flywheel5/fingerprint.json \
      --base /workspace/Qwen3.6-35B-A3B-REAP48-ours --out /workspace/flywheel5/adapter \
      [--max-steps N] [--device cuda|cpu] [--dtype bfloat16|float32]
"""
from __future__ import annotations

import argparse
import sys
from pathlib import Path

import torch
from peft import LoraConfig, get_peft_model
from transformers import AutoModelForCausalLM, AutoTokenizer, Trainer

from tools.flywheel.train_common import (LORA_ALPHA, LORA_R, PROCEDURE_SEED, PairDataset,
                                         assert_batch_shape, collate_single, load_pairs,
                                         tokenize_fn, training_args)

TARGET_MODULES = ["q_proj", "k_proj", "v_proj", "o_proj",
                  "in_proj_qkv", "in_proj_z", "in_proj_b", "in_proj_a", "out_proj",
                  "gate_proj", "up_proj", "down_proj"]
EXPECTED_CLASS = "Qwen3_5MoeForCausalLM"


def apply_lora(model, seed: int = PROCEDURE_SEED):
    """LoRA r16/a32 on TARGET_MODULES; `torch.manual_seed(seed)` immediately
    before peft initialises the adapters (the analogue of unsloth's random_state)."""
    torch.manual_seed(seed)
    cfg = LoraConfig(r=LORA_R, lora_alpha=LORA_ALPHA, lora_dropout=0.0, bias="none",
                     target_modules=TARGET_MODULES, task_type="CAUSAL_LM")
    return get_peft_model(model, cfg)


def assert_frozen(model) -> dict:
    """Every routed-expert and router parameter must be frozen; returns counts."""
    trainable = total = 0
    for name, p in model.named_parameters():
        total += p.numel()
        if p.requires_grad:
            trainable += p.numel()
        if (".experts." in name or name.endswith("mlp.gate.weight")) and p.requires_grad:
            raise AssertionError(f"expert/router parameter is trainable: {name}")
    return {"trainable": trainable, "total": total}


def main(argv=None) -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--corpus", required=True, type=Path)
    ap.add_argument("--fingerprint", required=True, type=Path)
    ap.add_argument("--base", required=True)
    ap.add_argument("--out", required=True, type=Path)
    ap.add_argument("--max-steps", type=int, default=-1)
    ap.add_argument("--device", default="cuda", choices=["cuda", "cpu"])
    ap.add_argument("--dtype", default="bfloat16", choices=["bfloat16", "float32"])
    args = ap.parse_args(argv)
    args.out.mkdir(parents=True, exist_ok=True)

    dtype = torch.bfloat16 if args.dtype == "bfloat16" else torch.float32
    model = AutoModelForCausalLM.from_pretrained(args.base, dtype=dtype, device_map=args.device)
    if type(model).__name__ != EXPECTED_CLASS:
        print(f"refusing: loaded {type(model).__name__}, expected {EXPECTED_CLASS}", file=sys.stderr)
        return 2
    print(f"model class {EXPECTED_CLASS}; num_experts={model.config.num_experts}; "
          f"layers={model.config.num_hidden_layers}")
    tokenizer = AutoTokenizer.from_pretrained(args.base)
    model.gradient_checkpointing_enable()
    model.enable_input_require_grads()
    model = apply_lora(model)
    stats = assert_frozen(model)
    print(f"trainable {stats['trainable']} / total {stats['total']} "
          f"({100.0 * stats['trainable'] / stats['total']:.4f}%) — experts+router frozen: asserted")

    train_rows, val_rows = load_pairs(args.corpus, args.fingerprint)
    print(f"pairs: train={len(train_rows)} val={len(val_rows)}")
    fn = tokenize_fn(tokenizer)
    train_ds = PairDataset([fn(r) for r in train_rows])
    val_ds = PairDataset([fn(r) for r in val_rows])
    assert_batch_shape(tokenizer, train_ds)

    overrides = {} if args.device == "cuda" and args.dtype == "bfloat16" else {"bf16": False, "use_cpu": args.device == "cpu"}
    targs = training_args(args.out, args.max_steps, **overrides)
    trainer = Trainer(model=model, args=targs, data_collator=collate_single,
                      train_dataset=train_ds, eval_dataset=val_ds)
    rc = 0
    try:
        trainer.train()
        model.save_pretrained(str(args.out))
        tokenizer.save_pretrained(str(args.out))
        trainer.state.save_to_json(str(args.out / "trainer_state.json"))
    except Exception as e:  # the markers are how the pod's wrapper reads the outcome
        print(f"TRAINING FAILED: {e!r}", file=sys.stderr)
        rc = 1
    (args.out / "EXIT").write_text(f"{rc}\n")
    (args.out / "DONE").write_text("ok\n" if rc == 0 else "failed\n")
    print(f"adapter saved to {args.out}" if rc == 0 else "no adapter saved")
    return rc


if __name__ == "__main__":
    raise SystemExit(main())
```

- [ ] **Step 8: Run** — `~/flywheel-venv/bin/python -m unittest tools.flywheel.tests.test_train_moe -v` → 4 pass (the CPU smoke takes ~1 min). Then the whole venv suite: `~/flywheel-venv/bin/python -m unittest discover -s tools/flywheel/tests -t . 2>&1 | tail -3`; and stdlib: `python3 -m unittest discover -s tools -t . 2>&1 | tail -3` (the new tests skip).

- [ ] **Step 9: README** — add a `## train_moe.py — turn 5's bf16-LoRA recipe for qwen3_5_moe` section to `tools/flywheel/README.md` (the forced changes, the frozen set, the unpacked ruling, the usage line, the pod note "installs before the job").

- [ ] **Step 10: Commit**

```bash
git add tools/flywheel
git commit -m "feat(flywheel): train_moe.py — bf16 LoRA for qwen3_5_moe (experts+router frozen), shared rules moved to train_common.py"
```

---

### Task 8: RunPod network volume (HUMAN-GATED — the first spend of the turn)

**STOP for Brice's go.** Creates the volume only (storage billing ≈ $0.07/GB/mo); the 38 GB upload happens in Task 10 step 2 on the training pod, once, and is skipped on any later re-cut.

- [ ] **Step 1: Find a datacenter with A100-SXM4-80GB availability** (the volume is datacenter-bound; the pod must be cut in the same one):

```bash
KEY=$(cat ~/.config/runpod/api_key)   # never echo $KEY
curl -s https://api.runpod.io/graphql -H "Authorization: Bearer $KEY" -H 'Content-Type: application/json' \
  -d '{"query":"query { gpuTypes(input:{id:\"NVIDIA A100-SXM4-80GB\"}) { id displayName lowestPrice(input:{gpuCount:1}) { uninterruptablePrice stockStatus } nodeGroupDatacenters { id name gpuAvailability(input:{gpuCount:1}) { available stockStatus gpuTypeId } } } }"}' \
  | python3 -m json.tool > target/flywheel5/dc-availability.json
python3 -c "import json;d=json.load(open('target/flywheel5/dc-availability.json'));[print(dc['id'],dc['name'],[g for g in dc['gpuAvailability'] if g['gpuTypeId']=='NVIDIA A100-SXM4-80GB']) for dc in d['data']['gpuTypes'][0]['nodeGroupDatacenters']]"
```

Pick a DC with `available: true` for the SXM (prefer one that also lists `stockStatus` High/Medium); record its id as `DC`. If the schema rejects a field, `curl -s https://api.runpod.io/graphql … -d '{"query":"{ __type(name:\"DataCenter\") { fields { name } } }"}'` and adapt — record the working query in the ledger.

- [ ] **Step 2: Create the volume**:

```bash
curl -s -X POST https://rest.runpod.io/v1/networkvolumes -H "Authorization: Bearer $KEY" -H 'Content-Type: application/json' \
  -d "{\"name\":\"bloomery-reap48-base\",\"size\":50,\"dataCenterId\":\"$DC\"}" | tee target/flywheel5/volume.json
VOL=$(python3 -c "import json;print(json.load(open('target/flywheel5/volume.json'))['id'])")
```

- [ ] **Step 3: Ledger** — start `target/flywheel5/pod-ledger.md` (local) with: date, DC, volume id + size, the balance before (`curl -s https://api.runpod.io/graphql -H "Authorization: Bearer $KEY" -d '{"query":"{ myself { clientBalance } }"}'`), and every later pod id / start / stop / cost line. The training evidence (Task 10) copies it in.

---

### Task 9: `~/flywheel5/` + the pre-registration (committed BEFORE the training pod is cut)

**Files:**
- Create: `docs/superpowers/evidence/<date>-flywheel5-preregistration.md`
- Local: `~/flywheel5/corpus.jsonl`, `~/flywheel5/fingerprint.json`, `~/flywheel5/SHAS.txt` (started)

- [ ] **Step 1: Corpus copy + sha** — `mkdir -p ~/flywheel5 && cp ~/flywheel4/corpus.jsonl ~/flywheel5/corpus.jsonl && cp docs/superpowers/evidence/2026-08-21-flywheel4-fingerprint.json ~/flywheel5/fingerprint.json && sha256sum ~/flywheel5/corpus.jsonl` → must print `9c51a8668b4ce861dbe3c8528f59655a9a78eee12523f397b33e28d5d7928a7d`; write that line to `~/flywheel5/SHAS.txt` with the header "flywheel5 (turn 5) artifact SHA-256s".
- [ ] **Step 2: Write the prereg** (template: `2026-08-21-flywheel4-preregistration.md`'s section order), with these contents, all specific:
  - **Subject:** `qwen36-reap48-flywheel5` = REAP-48-ours (sha `8027ca0a…`) + turn-5 LoRA, served Q4_K_M; model line `qwen36-reap48`; gates.md 2026-08-22 amendment cited.
  - **The battery (decides alone; all under envelope-v4, greedy):** (1) G4 on codec-tasks-v1, pass ≥16/20; (2) G5 on codec-tasks-v4-mixed, pass ≥13/16 per class; decided/provisional by the two-sided rule, stated separately. Two boots, identical configs (the Task 6 TOML with the new model name/path), **boot 1 decides**, boot 2 corroborates. **Success = both pass. Kill: G4 < 16/20 OR refuse < 8/16 → adapter shelved, recorded with anatomy. Secondary endpoints never kill.** Reporting discipline bT1/R1, bT10/R1 verbatim.
  - **The measured anchors (from `<date>-g5v4-reap48-baselines.md`, verbatim):** boot 1's G4, patch, refuse, composition, all endpoints, grant-violation rows, verb histogram, serving facts; then "What fw5 must do, stated as arithmetic" (refuse ≥13 of 16 = +N over the anchor; patch must not fall below 13; G4 must not fall below 16).
  - **Secondary endpoints:** the six with denominators (productive find /6, find-usage /6, run-before-done /5, per-family 6/5/5, productive run /5, reason-grounding over the 11 target-present refuse fixtures; zero spans = unmeasured) + the two reported line facts (grant-violation rows; verb histogram, `done` > 32 as the over-eagerness signature) — all computed by `tools/evidence/recompute.py` (keyed join, ordinal cross-check asserted).
  - **Corpus identity:** `~/flywheel5/corpus.jsonl` = turn-4 corpus byte-identical, sha; the turn-4 fingerprint and contamination report apply verbatim (nothing regenerated); "the pruner was calibrated on 512 samples (seed 42) of this same corpus — stated, not a confound claim".
  - **Training (pinned):** the spec §4.2 recipe verbatim; `train_moe.py` at the branch commit sha; TARGET_MODULES list; "experts + router frozen, asserted at load (the trainable count is recorded in the training evidence)"; unpacked bs 1 ruling + why; seeds statement (20260816 unchanged = procedure identity; no bitwise-reproducibility claim); pod pins; post-train chain; artifact names (`~/flywheel5/adapter/`, `~/flywheel5/qwen36-reap48-flywheel5-Q4_K_M.gguf`).
  - **Cost bounds (upper):** upload ≈ $0.8 · train ≈ 3.7 h ≈ $5.1 · evals ≈ $0.5 · post-train ≈ $0.5 · download ≈ $0.3 → ≈ $7.2 of the $10 cap; the cap is a stop rule; a stop = pod down, report, ask.
  - **Honesty lines** (each plainly): bf16-trained / Q4-served; torch-fallback DeltaNet on the pod (timings are upper bounds); the planted-test leak caveat carried from turn 4; the `Found instead:` frame and reason-grounding's known limitation; TaskStep now carries args (a journal addition since turn 4 — the recompute's keyed join is new, the ordinal join is still asserted alongside).
  - **Honest possibilities, pre-registered:** spec §5's list verbatim (patch-at-the-floor over-refusal; no transfer through attention+shared LoRA; persisting grant violations; speed/window at the fixed geometry; eval-loss uninterpreted; slow fallback → stop rule; reason-grounding at ceiling).
  - **Amendment rule:** separate dated files; nothing re-run; baselines never re-run for a nicer verdict.
  - **Committed artifacts.**
- [ ] **Step 3: Commit BEFORE any pod** — `docs: pre-register the flywheel5 battery before any training step` — and **push the branch** (`git push -u origin flywheel5-turn5`) so the ordering is provable from the remote too.

---

### Task 10: Training pod (HUMAN-GATED) → adapter → Q4_K_M → home

**STOP for Brice's go.** Everything below is recorded in `target/flywheel5/pod-ledger.md` as it happens and becomes `docs/superpowers/evidence/<date>-flywheel5-training.md`.

- [ ] **Step 1: Cut the pod in `DC` with the volume**:

```bash
KEY=$(cat ~/.config/runpod/api_key); PUB=$(cat ~/.ssh/runpod_spike.pub)
curl -s -X POST https://rest.runpod.io/v1/pods -H "Authorization: Bearer $KEY" -H 'Content-Type: application/json' -d @- <<EOF | tee target/flywheel5/pod.json
{"name":"bloomery-fw5-train","imageName":"runpod/pytorch:1.1.0-cu1290-torch291-ubuntu2404",
 "gpuTypeIds":["NVIDIA A100-SXM4-80GB"],"gpuCount":1,"cloudType":"COMMUNITY",
 "containerDiskInGb":150,"networkVolumeId":"$VOL","volumeMountPath":"/workspace",
 "dataCenterIds":["$DC"],"ports":["22/tcp"],"env":{"PUBLIC_KEY":"$PUB"}}
EOF
POD=$(python3 -c "import json;print(json.load(open('target/flywheel5/pod.json'))['id'])")
```

If COMMUNITY returns no machine, retry with `"cloudType":"SECURE"` and record `costPerHr`. Poll `curl -s https://rest.runpod.io/v1/pods/$POD -H "Authorization: Bearer $KEY"` every 30 s for `publicIp` + a `portMappings` entry for 22; **if none within 5 min: `curl -s -X DELETE https://rest.runpod.io/v1/pods/$POD …`, record the waste, re-cut**. `SSH="ssh -i ~/.ssh/runpod_spike -p $PORT root@$IP"`. Start a persistent Monitor (hourly): pods count (`GET /pods` → length) and `myself { clientBalance }`; stop condition pods == 0 after teardown.

- [ ] **Step 2: Upload the base once (skip if `/workspace/Qwen3.6-35B-A3B-REAP48-ours/model.safetensors` already exists with the right sha)** — on the box: `cd ~/models/hf/Qwen3.6-35B-A3B-REAP48-ours && split -b 7G -d model.safetensors /tmp/claude-1000/-home-brice/*/scratchpad/reap48.part.` (use this session's scratchpad dir); `$SSH mkdir -p /workspace/Qwen3.6-35B-A3B-REAP48-ours /workspace/parts`; in parallel (6 at once): `for p in reap48.part.0[0-5]; do scp -i ~/.ssh/runpod_spike -P $PORT $p root@$IP:/workspace/parts/ & done; wait`; then on the pod: `cat /workspace/parts/reap48.part.0? > /workspace/Qwen3.6-35B-A3B-REAP48-ours/model.safetensors && sha256sum …/model.safetensors` → must equal `8027ca0a8277b540cd4c62eb7a5bdf6028875e84b33ddcf4f9cd4b0e9d63423b` (else delete and re-transfer the failing part; never train on an unverified base); `rm -rf /workspace/parts`; scp the small files (`config.json generation_config.json tokenizer.json tokenizer_config.json vocab.json merges.txt chat_template.jinja reap_pruning.json summary.json`) and `~/flywheel5/corpus.jsonl` + `fingerprint.json` → `/workspace/flywheel5/`; `sha256sum /workspace/flywheel5/corpus.jsonl` → `9c51a866…`. Record minutes and MB/s.

- [ ] **Step 3: Environment (all BEFORE any job; then freeze)**:

```bash
$SSH 'pip install --break-system-packages "transformers==5.5.0" "peft==0.20.0" "accelerate==1.14.0" "safetensors==0.8.0" "flash-linear-attention==0.5.2" numpy && \
      git clone https://github.com/ggml-org/llama.cpp /workspace/llama.cpp && cd /workspace/llama.cpp && git checkout 8672290 && \
      pip install --break-system-packages -r requirements/requirements-convert_hf_to_gguf.txt && \
      cmake -B build -DGGML_CUDA=OFF && cmake --build build --target llama-quantize -j && \
      git clone https://github.com/bricelancasterwcp-sudo/bloomery /workspace/bloomery && cd /workspace/bloomery && git checkout flywheel5-turn5 && \
      pip freeze > /workspace/flywheel5/pip-freeze.txt && python -c "import torch,transformers,peft;print(torch.__version__,transformers.__version__,peft.__version__,torch.cuda.is_available())"'
```

Expect `2.9.1+cu129 5.5.0 0.20.0 True`. **From here on, no `pip install` on this pod.**

- [ ] **Step 4: Smoke (pre-registered part of the procedure)** — `$SSH 'cd /workspace/bloomery && python -m tools.flywheel.train_moe --corpus /workspace/flywheel5/corpus.jsonl --fingerprint /workspace/flywheel5/fingerprint.json --base /workspace/Qwen3.6-35B-A3B-REAP48-ours --out /workspace/flywheel5/smoke --max-steps 5 2>&1 | tail -40'` → prints `model class Qwen3_5MoeForCausalLM; num_experts=133; layers=40`, the trainable count (record it; spike: 21.17 M), `label-check ok`, 5 steps, `EXIT` 0. Record peak VRAM (`nvidia-smi --query-gpu=memory.used --format=csv` during the run). If OOM or any assertion: pod down, report, ask.

- [ ] **Step 5: The run, detached** — write `/workspace/flywheel5/train-wrapper.sh` on the pod:

```bash
#!/usr/bin/env bash
# Flywheel turn 5 training — detached wrapper (prereg: <date>-flywheel5-preregistration.md)
set -u
cd /workspace/bloomery
echo $$ > /workspace/flywheel5/train.pid
python -m tools.flywheel.train_moe \
  --corpus /workspace/flywheel5/corpus.jsonl \
  --fingerprint /workspace/flywheel5/fingerprint.json \
  --base /workspace/Qwen3.6-35B-A3B-REAP48-ours \
  --out /workspace/flywheel5/adapter \
  > /workspace/flywheel5/train.log 2>&1
rc=$?
echo "$rc" > /workspace/flywheel5/train.EXIT
touch /workspace/flywheel5/train.DONE
exit "$rc"
```

`$SSH 'chmod +x /workspace/flywheel5/train-wrapper.sh && setsid nohup /workspace/flywheel5/train-wrapper.sh > /workspace/flywheel5/train-wrapper.out 2>&1 < /dev/null &'`. Poll every 15 min: `$SSH 'tail -3 /workspace/flywheel5/train.log; ls /workspace/flywheel5/train.DONE 2>/dev/null'` (log file, never `pgrep`). Expected ≈ 3.7 h upper bound (8,680 micro-steps at ≤1.52 s); record step rate, losses at each `logging_steps`, eval losses. **Stop rule:** if the ledger's projected total would exceed $10, stop the pod at the next poll, report, ask.

- [ ] **Step 6: Post-train chain on the pod** — `/workspace/flywheel5/posttrain-wrapper.sh`:

```bash
#!/usr/bin/env bash
# Flywheel turn 5 post-train chain: merge -> bf16 GGUF -> Q4_K_M -> sha
set -u
cd /workspace/bloomery
echo $$ > /workspace/flywheel5/posttrain.pid
{
  echo "=== merge === $(date -u +%FT%TZ)"
  python - <<'PY' || { echo "MERGE FAILED rc=$?"; echo 1 > /workspace/flywheel5/posttrain.EXIT; touch /workspace/flywheel5/posttrain.DONE; exit 1; }
import torch
from peft import PeftModel
from transformers import AutoModelForCausalLM, AutoTokenizer
base = "/workspace/Qwen3.6-35B-A3B-REAP48-ours"
m = AutoModelForCausalLM.from_pretrained(base, dtype=torch.bfloat16, device_map="cpu")
m = PeftModel.from_pretrained(m, "/workspace/flywheel5/adapter").merge_and_unload()
m.save_pretrained("/workspace/flywheel5/merged", safe_serialization=True)
AutoTokenizer.from_pretrained(base).save_pretrained("/workspace/flywheel5/merged")
print("merged ok")
PY
  echo "=== convert to bf16 gguf === $(date -u +%FT%TZ)"
  python /workspace/llama.cpp/convert_hf_to_gguf.py /workspace/flywheel5/merged \
    --outfile /workspace/flywheel5/fw5-bf16.gguf --outtype bf16 \
    || { echo "CONVERT FAILED rc=$?"; echo 2 > /workspace/flywheel5/posttrain.EXIT; touch /workspace/flywheel5/posttrain.DONE; exit 2; }
  echo "=== quantize Q4_K_M === $(date -u +%FT%TZ)"
  /workspace/llama.cpp/build/bin/llama-quantize /workspace/flywheel5/fw5-bf16.gguf \
    /workspace/flywheel5/qwen36-reap48-flywheel5-Q4_K_M.gguf Q4_K_M \
    || { echo "QUANTIZE FAILED rc=$?"; echo 3 > /workspace/flywheel5/posttrain.EXIT; touch /workspace/flywheel5/posttrain.DONE; exit 3; }
  echo "=== block_count check === $(date -u +%FT%TZ)"
  python -c "import sys; sys.path.insert(0,'/workspace/llama.cpp/gguf-py'); import gguf; r=gguf.GGUFReader('/workspace/flywheel5/qwen36-reap48-flywheel5-Q4_K_M.gguf'); f=r.fields['qwen35moe.block_count']; print('block_count', int(f.parts[-1][0]))"
  echo "=== sha256 === $(date -u +%FT%TZ)"
  sha256sum /workspace/flywheel5/adapter/adapter_model.safetensors /workspace/flywheel5/qwen36-reap48-flywheel5-Q4_K_M.gguf /workspace/flywheel5/corpus.jsonl
  echo 0 > /workspace/flywheel5/posttrain.EXIT
  touch /workspace/flywheel5/posttrain.DONE
} > /workspace/flywheel5/posttrain.log 2>&1
```

Run it detached the same way; expect `block_count 40` (the source config carries `mtp_num_hidden_layers: 0`; a 41 here is a STOP — do not metadata-patch, report). Note: the merge loads 38 GB on CPU RAM — the pod has ≥ 80 GB RAM? Check `free -g` first; if not, load with `device_map="cuda"` instead (the GPU is idle after training).

- [ ] **Step 7: Bring it home** — chunked download of the Q4_K_M GGUF (split on the pod into 6 parts, parallel scp, cat, sha compare to the pod's line), plus `adapter/` (`adapter_model.safetensors`, `adapter_config.json`, tokenizer files), `train.log`, `trainer_state.json`, `pip-freeze.txt`, `smoke/` logs, `posttrain.log` → `~/flywheel5/`. `sha256sum ~/flywheel5/qwen36-reap48-flywheel5-Q4_K_M.gguf ~/flywheel5/adapter/adapter_model.safetensors ~/flywheel5/corpus.jsonl >> ~/flywheel5/SHAS.txt`. Local boot sanity is Task 11's job — no boot here.

- [ ] **Step 8: Teardown** — `curl -s -X DELETE https://rest.runpod.io/v1/pods/$POD -H "Authorization: Bearer $KEY"`; verify `GET /pods` → 0 and GraphQL `myself { pods { id } }` → `[]`; balance after; Monitor stops. The volume stays (holds the base; ≈ $3.50/mo at 50 GB — Brice decides later whether to delete it).

- [ ] **Step 9: Evidence** — `docs/superpowers/evidence/<date>-flywheel5-training.md`: pod ledger (ids, DC, cloud type, $/h, start/stop, cost per phase, balance before/after), environment freeze (the `pip freeze` verbatim or its sha + path), upload metrics, smoke output, trainable count, step rate, losses (train curve at `logging_steps`, eval losses, final), wall time, post-train timings, **the sha chain** (base `8027ca0a…` re-verified on the pod; corpus `9c51a866…` before and after; adapter; bf16 GGUF; Q4_K_M on the pod and at home, equal), `block_count 40`, any deviation from the runbook verbatim. Commit `docs: flywheel5 training record — rental run, sha chain, costs`.

---

### Task 11: The battery (HUMAN-GATED) + evidence + debt + PR

**STOP for Brice's go before the first boot.**

- [ ] **Step 1: Boot configs** — the Task 6 TOML twice, with `[models."qwen36-reap48-flywheel5"]` `path = "/home/brice/flywheel5/qwen36-reap48-flywheel5-Q4_K_M.gguf"`, `data_dir` under `target/fw5-live/boot{1,2}/data`, ports 8399/8398. Preflight as Task 6 (no daemon, `nm -C … ggml_vulkan`, no source change since the featured build — if Branch 2 changed no Rust, the Task 5 binary stands; record its mtime).
- [ ] **Step 2: Boot 1 (decides), then boot 2** — exactly Task 6 steps 3–5, digest must equal the `SHAS.txt` Q4_K_M line; copy journals to `docs/superpowers/evidence/<date>-flywheel5-boot{1,2}-{journal,tasks}.jsonl`; recompute both → `…-boot{1,2}-recompute.json` (`keyed`, `keyed_equals_ordinal true`, no violations, `journaled_verdict_matches true`).
- [ ] **Step 3: Judge against the prereg verbatim** — `docs/superpowers/evidence/<date>-flywheel5-battery.md`: `## 1. Verdicts` (boot 1: G4 landed/n + flag; patch; refuse; pass/kill per the rule; `done_trust`), `## 2. Identity chain` (master/branch shas, binary, PIDs via `readlink`, GGUF sha = daemon digest, adapter sha, corpus sha, fixture TOML shas, assay pin), `## 3. Method and preflight`, `## 4. Boot 1`, `## 5. Boot 2` (+ identical-or-not), `## 6. The named reads` — the **refuse class per family row by row** (the leg the turn exists to move; anatomy from a script), the **patch class: held or regressed** (vs the anchor's 13/16), grant-violation rows vs the anchor's, `done` count vs 32, the 5 run-granted and 6 find-shaped rows, reason-grounding with its real denominator and the by-eye read kept separate, surprises verbatim, `## 7. Verdict vs the pre-registration`, `## 8. Ladder under envelope-v4` (stock-14B, fw3, fw4, REAP-48-ours untrained, fw5 — descriptive, no causal sentence across bases), `## 9. Caveats`, `## 10. Committed artifacts`. Numbers from the recompute JSON only.
- [ ] **Step 4: CARRIED-DEBT + README** — append a "Delivered in flywheel turn 5" section to `docs/CARRIED-DEBT.md` (struck on arrival: the TaskStep observability debt, the hybrid-geometry defects 1 and 4 from the spike; recorded-not-fixed: compute-buffer growth with n_ctx; deferred: packing side study, honesty instrument = turn 6, router/expert training = parked research; process lessons of the wave); README: one line for the new line and the `ctx_overhead_mib = 512` hybrid note.
- [ ] **Step 5: Evidence review, commit, PR, merge** — independent re-derivation of the headline counts with jq + the recompute JSON check; fix round; commits `docs: the flywheel turn-5 battery — <verdict>` and `docs: CARRIED-DEBT — the flywheel turn-5 merge-time append`; `git push`; `gh pr create --title "flywheel turn 5: the REAP-48 line's first adapter — <verdict>"`; Brice merges (or delegated: from the main checkout on master, `gh pr merge <N> --merge --delete-branch && git pull --ff-only`); verify `git rev-list --left-right --count origin/master...master` → `0 0`; remove the worktree; update the memory file with the verdict and what is next (turn 6 = honesty instrument spec; packing side study).

---

## Self-review notes (at write time)

- **Spec coverage:** §2 → Task 2 (both derived fields, both charge sites, `/status`, comment amendments, skip-if-absent real-GGUF test; `ctx_overhead_mib = 512` + no override in Tasks 6/11 configs); §3 → Task 3 (`args`, `agent`, compat pins, probe tests) + Task 4 (recompute, turn-4 pins, keyed+ordinal); §4.1/§4.2 → Task 7 (+ pinned args test, 12 targets, freeze assertion, seed test, CPU smoke); §4.3 → Tasks 8, 10 (volume, DC binding, 150 GB, image, pins-before-job, smoke, detached run, post-train chain, download-only-what's-needed, teardown, cost bounds + stop rule); §5 → Task 1 (gates amendment), Task 6 (two identical boots, boot 1 anchor, pre-registered expectations, serving facts), Task 9 (prereg before the pod, all sections), Task 11 (battery, rule verbatim, ladder, named reads); §6 → each task's tests + the house rules in Global Constraints; §7 non-goals → no task touches a frozen set, envelope, scoring, enforcement, packing, router/expert training, HF publication; §8 order → Tasks 1–11 in that order.
- **Placeholder scan:** the one deliberate name-placeholder is flagged in Task 2 step 6 (`register_like_the_other_tests` = copy the file's own registration lines); `<date>` in evidence filenames is the day produced (spec §5 rule); `$DC`, `$VOL`, `$POD`, `$IP`, `$PORT` are shell variables set in-step.
- **Type/name consistency:** `attention_layers` / `recurrent_state_bytes` (Task 2 struct, tests, pager accessor, status field); `args` / `agent` (Task 3 events, `StepReport`, `TaskStepRecord`, helpers, Task 4 `journal.py` reads `fx.get("agent")`, `r["id"]`); `recompute()` signature `(journal, tasks, g5_fixtures, g4_set)` used identically in Task 4 tests and Tasks 6/11 CLI; `train_common` names used by `train.py`, `train_moe.py`, and both test files; `apply_lora` / `assert_frozen` / `main(argv)` in Task 7 tests and code; `~/flywheel5/` artifact names identical in Tasks 9, 10, 11.
- **Verified pins (2026-08-22, jq over the committed journals):** fw4-g4 20/20, 60 steps, 20 groups; fw4-g5 20/20 · 16/16 · 16/16, 151 steps, 52 groups, 0 grant violations, histogram {done 52, find 6, patch 36, read 52, run 5}, 5 `ran … exit 0` rows; fw3@v4 20/20 · 15/16 · 16/16, 149 steps, 5 grant violations, {52, 6, 35, 51, 5}, 0 exit-0 runs; stock@v4 6/20 · 5/16 · 8/16, 203 steps, **42** grant-violation rows (the spike's "38" was a different count — the journal wins), {done 32, find 9, patch 94, read 68}. Composition and endpoint counts come from the committed evidence tables (baselines §5.2/§5.3/§6.2/§6.3; battery §5.2/§5.3).
