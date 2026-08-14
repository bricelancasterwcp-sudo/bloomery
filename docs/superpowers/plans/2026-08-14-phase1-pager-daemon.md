# bloomery Phase 1 — Pager Daemon Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the Phase 1 pager daemon — "the serving daemon that doesn't lie": multi-agent priority paging of weights + KV images on one consumer GPU, embedded assay POST, replayable journal, and an honest OpenAI-compatible `/v1` shim — passing gate G2.

**Architecture:** A Rust workspace with three library crates and two binaries. `bloomery-core` is pure logic (GGUF metadata, the window law, budgets, journal types, assay-profile ingestion, the residency planner). `bloomery-substrate` defines the `Substrate` trait with a scripted `FakeSubstrate` (all unit tests run GPU-free) and a feature-gated llama.cpp implementation (Vulkan kernels — own the daemon, rent the kernels). `bloomery-daemon` wires them into the pager, agent table, KV image store, native HTTP API, `/v1` shim, and assay-as-POST boot. `bloomery-bench` is the G2 instrument, computing its numbers from the journal alone.

**Tech Stack:** Rust (stable, edition 2021). Dependency allowlist (pinned in workspace `Cargo.toml`, nothing else without a plan amendment): `llama-cpp-2` (+ `llama-cpp-sys-2`, feature `vulkan`), `tiny_http`, `serde` + `serde_json`, `toml`, `sha2`. Python 3.12+ with `assay` installed is a runtime dependency of the POST only.

**Spec:** `docs/superpowers/specs/2026-08-14-bloomery-design.md` (approved 2026-08-14). Design laws §3 govern every task.

## Global Constraints

- **Law 1:** the window is computed, never read: `usable = min(training_ctx, (free_vram − weights − overhead)/kv_per_token, user_cap)`, always reporting which term bound it.
- **Law 2:** never send/accept a prompt that does not fit — refuse with the arithmetic, never truncate.
- **Law 4:** every inference reply is contract-checked (token stats present); `ContractViolation` is first-class; infrastructure failure is never recorded as model failure.
- **Law 5:** unmeasured ≠ failed: `Option`/named-reason for unmeasured values, never a defaulted number that looks like a measurement.
- **Law 6:** no inference without an explicit budget; spent-vs-granted accounted per agent.
- **Law 7:** every inference call (exact prompt), pager op, and scheduler decision is journaled and replayable.
- KV images are tagged with the model blob digest; a digest mismatch invalidates the image (cold start, never an error).
- The unit-test suite runs with **no GPU, no daemon, no sockets to the outside** (FakeSubstrate + tempdirs + loopback only). Live tests are `#[ignore]` and additionally require `BLOOMERY_LIVE=1`.
- `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` must be green at every commit.
- Files ≤ 800 lines; functions focused; conventional commit messages (`feat:`/`test:`/`docs:`/`chore:`).
- Commands below run from the repo root `~/workspace/bloomery` unless stated.

---

### Task 1: Phase 0 — prior-art verification and decision record

**Files:**
- Create: `docs/priorart/2026-08-14-phase0-priorart.md`

**Interfaces:**
- Consumes: nothing (research task).
- Produces: verified decisions D1–D4 that Tasks 11, 14, 15 build against; the exact llama.cpp state-API symbol names Task 11 calls.

This is a research task, not TDD. Its deliverable is a decision record that either **confirms or overturns** the following pre-registered defaults. Per the spec's reuse rule, an overturned D2 triggers a plan amendment before any pager code is written.

- [ ] **Step 1: Verify D1 — llama.cpp bindings and state APIs.**

Default: the `llama-cpp-2` crate (feature `vulkan`) can (a) load a GGUF with `n_gpu_layers`, (b) create a context with per-context `n_ctx`, (c) decode and expose real token counts, and (d) save/restore full context state. Check the crate docs/source (`docs.rs/llama-cpp-2`, `github.com/utilityai/llama-cpp-rs`) and llama.cpp's C API (`llama.h`) for the exact symbols: expected `llama_state_get_size`, `llama_state_get_data`, `llama_state_set_data` (and the `_seq_` variants). Record: the crate wrapper names if safe wrappers exist, else the `llama-cpp-sys-2` raw symbols Task 11 will call through `unsafe`. Also record whether the crate exposes the model's chat template (`llama_model_chat_template` / apply helpers) for Task 15's D4.

- [ ] **Step 2: Verify D2 — nobody already ships the pager.**

Default: no existing OSS system provides priority-driven paging of **weights + KV images across multiple agents on one consumer GPU**. Survey (gh search + docs, ~30 min, breadth not depth): vLLM PagedAttention (expected: VRAM-internal paging for serving farms), SGLang radix/prefix cache, LMCache, Ollama's scheduler/`keep_alive`, llama.cpp `llama-server` slot save/restore, exo, AIOS. For each: one paragraph — what it pages, for whom, and why it does or doesn't cover the bloomery pager. Verdict: if something covers ≥80% of Tasks 12–13, STOP and amend the plan to wrap it instead (spec §9 last risk).

- [ ] **Step 3: Verify D3 and D4 — HTTP and chat templating.**

D3 default: `tiny_http` supports chunked responses adequate for SSE streaming (Task 15); else fall back to `hyper` (allowed but must be recorded as an allowlist change). D4 default: chat templating uses the model's embedded template via llama.cpp if the binding exposes it; else the documented fallback is the plain concatenation `"{role}: {content}\n"` + `"assistant: "` (record which one Task 15 gets).

- [ ] **Step 4: Write the decision record and commit.**

`docs/priorart/2026-08-14-phase0-priorart.md` sections: D1 (with exact symbol names), D2 (survey table + verdict), D3, D4, and "Overturned defaults" (empty section if none — the section must exist so silence is distinguishable from omission).

```bash
git add docs/priorart/2026-08-14-phase0-priorart.md
git commit -m "docs: phase 0 prior-art verification (D1-D4)"
```

---

### Task 2: Phase 0 — pin the gates

**Files:**
- Create: `docs/gates.md`

**Interfaces:**
- Consumes: spec §6 provisional numbers; Task 1 feasibility notes.
- Produces: the pinned G1–G4 that Task 17's evidence doc is judged against.

- [ ] **Step 1: Write `docs/gates.md` pinning all four gates.**

Copy spec §6 and pin final values (adopt the provisional numbers unless Task 1 found a feasibility reason to change one — if changed, state the reason in the doc). Must include, verbatim commitments:

- **G1 (runs Phase 2, pinned now):** tiny-model policy beats the deterministic heuristic on useful-work-per-GPU-second by ≥10%, contract-violation rate ≤5%, per-decision latency ≤500 ms. Pinned metric: `useful_work = Σ over completed infer calls of completion_tokens × (priority + 1) / 256`, divided by wall-clock GPU-seconds of the run; computable from `InferCompleted` + `AgentCreated` journal events alone.
- **G2 (this plan):** p95 **warm** agent switch (KV image in RAM, weights resident) ≤ 2000 ms; p95 **cold** switch (weights not resident, image on NVMe) ≤ 5000 ms. Protocol: ≥50 switches per class on the enthusiast-16GB tier (declared `--real-hardware`), model qwen2.5-coder:7b-instruct-q8_0, computed by `bloomery-bench report` from `PagerOp` journal events only. Page-cache caveat for cold switches must be stated in the evidence doc.
- **G3 (future):** semantic view beats grep/fd baseline by ≥15pp top-5 hit rate on a frozen task set.
- **G4 (Phase 2):** per-model codec landing (applies-and-parses lens) ≥80% under the OS envelope, else demotion.

- [ ] **Step 2: Commit.**

```bash
git add docs/gates.md
git commit -m "docs: pin gates G1-G4 (pre-registered before instruments exist)"
```

---

### Task 3: Workspace scaffold + GGUF metadata parser

**Files:**
- Create: `Cargo.toml` (workspace), `crates/bloomery-core/Cargo.toml`, `crates/bloomery-core/src/lib.rs`, `crates/bloomery-core/src/gguf.rs`
- Test: `crates/bloomery-core/tests/gguf_test.rs`

**Interfaces:**
- Produces: `pub struct GgufMeta { pub arch: String, pub layers: u32, pub kv_heads: u32, pub head_dim: u32, pub training_ctx: u32, pub weights_bytes: u64 }`, `pub fn parse_gguf_meta(path: &Path) -> Result<GgufMeta, GgufError>`, `pub enum GgufError { BadMagic, UnsupportedVersion(u32), MissingKey(String), Io(std::io::Error) }` — consumed by Tasks 4, 12.

- [ ] **Step 1: Scaffold the workspace.**

Root `Cargo.toml`:

```toml
[workspace]
resolver = "2"
members = ["crates/bloomery-core"]

[workspace.dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
sha2 = "0.10"
```

`crates/bloomery-core/Cargo.toml`: name `bloomery-core`, edition 2021, deps `serde`, `serde_json` (workspace). `lib.rs`: `pub mod gguf;`.

- [ ] **Step 2: Write the failing test.**

GGUF v3 layout (metadata only; tensor data irrelevant here): magic `b"GGUF"`, `u32` version=3, `u64` tensor_count, `u64` kv_count, then kv pairs: key = `u64` len + UTF-8 bytes; value = `u32` type tag + payload. Type tags used: 4=`u32`, 8=`string` (`u64` len + bytes). The test builds a fixture in memory:

```rust
use std::io::Write;
use bloomery_core::gguf::{parse_gguf_meta, GgufError};

fn kv_string(buf: &mut Vec<u8>, key: &str, val: &str) {
    buf.extend((key.len() as u64).to_le_bytes());
    buf.extend(key.as_bytes());
    buf.extend(8u32.to_le_bytes());
    buf.extend((val.len() as u64).to_le_bytes());
    buf.extend(val.as_bytes());
}
fn kv_u32(buf: &mut Vec<u8>, key: &str, val: u32) {
    buf.extend((key.len() as u64).to_le_bytes());
    buf.extend(key.as_bytes());
    buf.extend(4u32.to_le_bytes());
    buf.extend(val.to_le_bytes());
}

fn write_qwen_like_gguf(path: &std::path::Path) {
    let mut kvs = Vec::new();
    kv_string(&mut kvs, "general.architecture", "qwen2");
    kv_u32(&mut kvs, "qwen2.block_count", 28);
    kv_u32(&mut kvs, "qwen2.attention.head_count_kv", 4);
    kv_u32(&mut kvs, "qwen2.attention.key_length", 128);
    kv_u32(&mut kvs, "qwen2.context_length", 32768);
    let mut f = std::fs::File::create(path).unwrap();
    f.write_all(b"GGUF").unwrap();
    f.write_all(&3u32.to_le_bytes()).unwrap();
    f.write_all(&0u64.to_le_bytes()).unwrap();          // tensor_count
    f.write_all(&5u64.to_le_bytes()).unwrap();          // kv_count
    f.write_all(&kvs).unwrap();
}

#[test]
fn parses_qwen_like_metadata() {
    let dir = std::env::temp_dir().join("bloomery-gguf-test");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("qwen.gguf");
    write_qwen_like_gguf(&path);
    let m = parse_gguf_meta(&path).unwrap();
    assert_eq!(m.arch, "qwen2");
    assert_eq!((m.layers, m.kv_heads, m.head_dim, m.training_ctx), (28, 4, 128, 32768));
    assert_eq!(m.weights_bytes, std::fs::metadata(&path).unwrap().len());
}

#[test]
fn rejects_bad_magic() {
    let dir = std::env::temp_dir().join("bloomery-gguf-test");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("bad.gguf");
    std::fs::write(&path, b"NOPE").unwrap();
    assert!(matches!(parse_gguf_meta(&path), Err(GgufError::BadMagic)));
}

#[test]
fn missing_key_is_named() {
    // fixture with architecture but no block_count
    let dir = std::env::temp_dir().join("bloomery-gguf-test");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("partial.gguf");
    let mut kvs = Vec::new();
    kv_string(&mut kvs, "general.architecture", "qwen2");
    let mut f = std::fs::File::create(&path).unwrap();
    f.write_all(b"GGUF").unwrap();
    f.write_all(&3u32.to_le_bytes()).unwrap();
    f.write_all(&0u64.to_le_bytes()).unwrap();
    f.write_all(&1u64.to_le_bytes()).unwrap();
    f.write_all(&kvs).unwrap();
    match parse_gguf_meta(&path) {
        Err(GgufError::MissingKey(k)) => assert_eq!(k, "qwen2.block_count"),
        other => panic!("expected MissingKey, got {other:?}"),
    }
}
```

- [ ] **Step 3: Run tests, verify they fail to compile (module absent).**

Run: `cargo test -p bloomery-core`
Expected: compile error, `gguf` unresolved.

- [ ] **Step 4: Implement `gguf.rs`.**

Reader over `BufReader<File>`: check magic; version must be 3 (else `UnsupportedVersion`); skip tensor_count; loop kv_count times reading key + typed value into a `HashMap<String, GgufValue>` where `enum GgufValue { U8(u8), I8(i8), U16(u16), I16(i16), U32(u32), I32(i32), F32(f32), Bool(bool), Str(String), U64(u64), I64(i64), F64(f64), Array }` — arrays (tag 9) are read as `u32` elem-type + `u64` len and **skipped element-by-element** (strings need per-element length reads), stored as `Array`. Then resolve: `arch` from `general.architecture` (string, else `MissingKey`); `layers` = `{arch}.block_count`; `kv_heads` = `{arch}.attention.head_count_kv`; `head_dim` = `{arch}.attention.key_length` if present, else `{arch}.embedding_length / {arch}.attention.head_count` (both required in that branch, `MissingKey` names whichever is absent); `training_ctx` = `{arch}.context_length`. Integer lookups accept `U32` or `U64` tags (real blobs vary). `weights_bytes` from `std::fs::metadata(path)?.len()`.

- [ ] **Step 5: Run tests, verify pass; commit.**

Run: `cargo test -p bloomery-core` → all pass; `cargo clippy --workspace --all-targets -- -D warnings`; `cargo fmt`.

```bash
git add Cargo.toml crates/
git commit -m "feat: workspace scaffold + GGUF metadata parser"
```

---

### Task 4: Geometry — the window law

**Files:**
- Create: `crates/bloomery-core/src/geometry.rs`
- Modify: `crates/bloomery-core/src/lib.rs` (add `pub mod geometry;`)
- Test: `crates/bloomery-core/tests/geometry_test.rs`

**Interfaces:**
- Consumes: `GgufMeta` (Task 3).
- Produces (consumed by Tasks 12–14):

```rust
pub fn kv_bytes_per_token(m: &GgufMeta) -> u64;          // 2 * layers * kv_heads * head_dim * 2 (f16)
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum BoundBy { TrainingCtx, Vram, UserCap, MeasuredCeiling }
#[derive(Debug, Clone, serde::Serialize)]
pub struct Window { pub tokens: u32, pub bound_by: BoundBy, pub vram_unmeasured: bool }
pub struct GeometryInput {
    pub training_ctx: u32,
    pub kv_per_token: u64,
    pub weights_bytes: u64,
    pub free_vram_bytes: Option<u64>,   // None = unmeasured (law 5), never 0
    pub overhead_bytes: u64,
    pub user_cap: Option<u32>,
    pub measured_ceiling: Option<u32>,  // assay ceiling.max_verified
}
pub fn usable_window(i: &GeometryInput) -> Window;
```

- [ ] **Step 1: Write the failing tests.**

```rust
use bloomery_core::geometry::*;

const KIB: u64 = 1024;
const GIB: u64 = 1024 * 1024 * 1024;

fn base() -> GeometryInput {
    GeometryInput {
        training_ctx: 32768,
        kv_per_token: 56 * KIB,          // qwen2.5-coder-7b, measured (robigo)
        weights_bytes: 8 * GIB,
        free_vram_bytes: Some(14 * GIB),
        overhead_bytes: 1 * GIB,
        user_cap: None,
        measured_ceiling: None,
    }
}

#[test]
fn training_ctx_binds_when_vram_is_ample() {
    let w = usable_window(&base());
    assert_eq!(w.tokens, 32768);
    assert_eq!(w.bound_by, BoundBy::TrainingCtx);
    assert!(!w.vram_unmeasured);
}

#[test]
fn vram_binds_when_scarce() {
    let mut i = base();
    i.free_vram_bytes = Some(4 * GIB);
    i.weights_bytes = 2 * GIB;
    // (4 - 2 - 1) GiB / 56 KiB = 18724 tokens
    let w = usable_window(&i);
    assert_eq!(w.tokens, 18724);
    assert_eq!(w.bound_by, BoundBy::Vram);
}

#[test]
fn user_cap_binds() {
    let mut i = base();
    i.user_cap = Some(8192);
    let w = usable_window(&i);
    assert_eq!((w.tokens, w.bound_by), (8192, BoundBy::UserCap));
}

#[test]
fn measured_ceiling_binds_below_everything() {
    let mut i = base();
    i.measured_ceiling = Some(11500);
    let w = usable_window(&i);
    assert_eq!((w.tokens, w.bound_by), (11500, BoundBy::MeasuredCeiling));
}

#[test]
fn unmeasured_vram_is_flagged_not_zeroed() {
    let mut i = base();
    i.free_vram_bytes = None;
    let w = usable_window(&i);
    assert_eq!(w.tokens, 32768);        // other terms still apply
    assert!(w.vram_unmeasured);          // law 5: named, never silently defaulted
}

#[test]
fn kv_arithmetic_matches_measured_qwen() {
    use bloomery_core::gguf::GgufMeta;
    let m = GgufMeta { arch: "qwen2".into(), layers: 28, kv_heads: 4, head_dim: 128,
                       training_ctx: 32768, weights_bytes: 0 };
    assert_eq!(kv_bytes_per_token(&m), 57344); // 56 KiB — robigo's measured row
}
```

- [ ] **Step 2: Run, verify compile failure.** `cargo test -p bloomery-core geometry` → unresolved module.

- [ ] **Step 3: Implement.**

`kv_bytes_per_token`: `2 * layers * kv_heads * head_dim * 2` as u64 math. `usable_window`: build candidate list of `(tokens, BoundBy)`: always `(training_ctx, TrainingCtx)`; if `free_vram_bytes` is `Some(v)` push `((v.saturating_sub(weights).saturating_sub(overhead) / kv_per_token) as u32, Vram)`; if `user_cap` push; if `measured_ceiling` push. Take the minimum by tokens — **ties resolve in declaration order** (TrainingCtx < Vram < UserCap < MeasuredCeiling wins only when strictly smaller; on equal tokens the earlier term is reported). `vram_unmeasured = free_vram_bytes.is_none()`.

- [ ] **Step 4: Run tests → pass; clippy + fmt clean.**

- [ ] **Step 5: Commit.**

```bash
git add crates/bloomery-core
git commit -m "feat: window law - usable_window with binding-term report"
```

---

### Task 5: VRAM probe (honest None)

**Files:**
- Create: `crates/bloomery-core/src/vram.rs`
- Modify: `crates/bloomery-core/src/lib.rs`
- Test: `crates/bloomery-core/tests/vram_test.rs`

**Interfaces:**
- Produces: `pub fn free_vram_bytes<R: Fn(&str, &[&str]) -> std::io::Result<String>>(run: R) -> Option<u64>` — the injectable runner keeps tests process-free; the daemon (Task 12) passes a real `std::process::Command` runner calling `nvidia-smi --query-gpu=memory.free --format=csv,noheader,nounits`.

- [ ] **Step 1: Failing tests.**

```rust
use bloomery_core::vram::free_vram_bytes;

#[test]
fn parses_nvidia_smi_mib() {
    let out = free_vram_bytes(|_, _| Ok("14558\n".to_string()));
    assert_eq!(out, Some(14558 * 1024 * 1024));
}

#[test]
fn multi_gpu_takes_first_line() {
    let out = free_vram_bytes(|_, _| Ok("14558\n8192\n".to_string()));
    assert_eq!(out, Some(14558 * 1024 * 1024));
}

#[test]
fn missing_binary_is_none_not_zero() {
    let out = free_vram_bytes(|_, _| Err(std::io::Error::from(std::io::ErrorKind::NotFound)));
    assert_eq!(out, None);
}

#[test]
fn garbage_output_is_none() {
    let out = free_vram_bytes(|_, _| Ok("N/A\n".to_string()));
    assert_eq!(out, None);
}
```

- [ ] **Step 2: Run → compile failure.**
- [ ] **Step 3: Implement** (call runner with `"nvidia-smi"` + the query args; first line, trim, `parse::<u64>().ok()? * 1024 * 1024`).
- [ ] **Step 4: Run → pass; clippy/fmt.**
- [ ] **Step 5: Commit** — `feat: VRAM probe with honest None on failure`.

---

### Task 6: Budgets

**Files:**
- Create: `crates/bloomery-core/src/budget.rs`
- Modify: `crates/bloomery-core/src/lib.rs`
- Test: `crates/bloomery-core/tests/budget_test.rs`

**Interfaces:**
- Produces (consumed by Tasks 13–15):

```rust
#[derive(Debug, Clone, serde::Serialize)]
pub struct Budget { granted_tokens: u64, spent_tokens: u64 }
impl Budget {
    pub fn new(granted_tokens: u64) -> Self;
    pub fn remaining(&self) -> u64;
    pub fn check(&self, requested: u64) -> Result<(), BudgetExhausted>; // pre-call gate
    pub fn charge(&mut self, actual: u64);                             // post-call, always records
    pub fn spent(&self) -> u64;
    pub fn granted(&self) -> u64;
}
#[derive(Debug, PartialEq, Eq)]
pub struct BudgetExhausted { pub remaining: u64, pub requested: u64 }
```

- [ ] **Step 1: Failing tests.**

```rust
use bloomery_core::budget::{Budget, BudgetExhausted};

#[test]
fn check_refuses_with_arithmetic() {
    let b = Budget::new(100);
    assert_eq!(b.check(101), Err(BudgetExhausted { remaining: 100, requested: 101 }));
    assert!(b.check(100).is_ok());
}

#[test]
fn charge_records_actuals_even_past_granted() {
    let mut b = Budget::new(100);
    b.charge(60);
    assert_eq!((b.spent(), b.remaining()), (60, 40));
    b.charge(60); // actual usage exceeded the estimate — record honestly
    assert_eq!((b.spent(), b.remaining()), (120, 0));
    assert!(b.check(1).is_err());
}
```

- [ ] **Step 2: Run → failure. Step 3: Implement (remaining = `granted.saturating_sub(spent)`). Step 4: Run → pass. Step 5: Commit** — `feat: per-agent token budgets (check/charge, spent-vs-granted)`.

---

### Task 7: Journal (JSONL, replayable)

**Files:**
- Create: `crates/bloomery-core/src/journal.rs`
- Modify: `crates/bloomery-core/src/lib.rs`
- Test: `crates/bloomery-core/tests/journal_test.rs`

**Interfaces:**
- Produces (consumed by Tasks 13–17; `bloomery-bench report` computes G2 from these events alone):

```rust
pub type AgentId = String;
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "event")]
pub enum Event {
    Boot { version: String },
    Post { model: String, outcome: String, profile_path: Option<String> },
    Degraded { reason: String },
    AgentCreated { id: AgentId, model: String, priority: u8, window_tokens: u32,
                   bound_by: String, budget_granted: u64 },
    SchedulerDecision { id: AgentId, decision: String, evicted: Vec<AgentId> },
    Refusal { id: AgentId, needed_tokens: u64, window_tokens: u32, detail: String },
    BudgetRefused { id: AgentId, remaining: u64, requested: u64 },
    InferStarted { id: AgentId, prompt: String, prompt_sha256: String },
    InferCompleted { id: AgentId, prompt_tokens: u32, completion_tokens: u32, duration_ms: u64 },
    ContractViolation { id: AgentId, kind: String },
    PagerOp { id: AgentId, op: PagerOpKind, bytes: u64, duration_ms: u64, image_tier: String },
    ModelLoaded { model: String, duration_ms: u64 },
    ModelUnloaded { model: String },
}
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum PagerOpKind { SuspendSave, ResumeLoad, EvictSave }
pub struct Journal { /* file handle */ }
impl Journal {
    pub fn open(path: &std::path::Path) -> std::io::Result<Journal>;
    pub fn append(&mut self, e: &Event) -> std::io::Result<()>;   // one JSON line, flushed
}
pub fn replay(path: &std::path::Path) -> std::io::Result<Vec<Event>>;
pub fn sha256_hex(s: &str) -> String;
```

`image_tier` on `PagerOp` is `"ram"` or `"nvme"` — this is what lets `bloomery-bench` classify warm vs cold switches from the journal alone (gate law: journal-computable).

- [ ] **Step 1: Failing test — append/replay round-trip.**

```rust
use bloomery_core::journal::*;

#[test]
fn append_then_replay_round_trips() {
    let dir = std::env::temp_dir().join("bloomery-journal-test");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("j.jsonl");
    let _ = std::fs::remove_file(&path);
    let mut j = Journal::open(&path).unwrap();
    let e1 = Event::Boot { version: "0.1.0".into() };
    let e2 = Event::PagerOp { id: "a1".into(), op: PagerOpKind::ResumeLoad,
                              bytes: 450_000_000, duration_ms: 20, image_tier: "ram".into() };
    j.append(&e1).unwrap();
    j.append(&e2).unwrap();
    assert_eq!(replay(&path).unwrap(), vec![e1, e2]);
}

#[test]
fn prompt_hash_is_stable() {
    assert_eq!(sha256_hex("abc").len(), 64);
    assert_eq!(sha256_hex("abc"), sha256_hex("abc"));
}
```

- [ ] **Step 2: Run → failure. Step 3: Implement** (`BufWriter` + `serde_json::to_string` + `\n` + `flush()` per append; `replay` = per-line `from_str`, error on any unparseable line — a corrupt journal must fail loudly, not skip silently). `sha256_hex` via `sha2`.
- [ ] **Step 4: Run → pass. Step 5: Commit** — `feat: replayable JSONL journal (events for infer/pager/scheduler)`.

---

### Task 8: assay profile ingestion

**Files:**
- Create: `crates/bloomery-core/src/profile.rs`
- Modify: `crates/bloomery-core/src/lib.rs`
- Test: `crates/bloomery-core/tests/profile_test.rs`

**Interfaces:**
- Consumes: assay profile JSON, schema v3 (assay repo `~/workspace/assay`, README "The profile").
- Produces (consumed by Tasks 12, 16):

```rust
#[derive(Debug, Clone, serde::Deserialize)]
pub struct Profile { /* private fields */ }
impl Profile {
    pub fn from_json(s: &str) -> Result<Profile, ProfileError>;
    pub fn schema_version(&self) -> u32;                  // must be >= 2, else ProfileError::UnsupportedSchema
    pub fn model_name(&self) -> &str;
    pub fn measured_ceiling(&self) -> Option<u32>;        // ceiling.max_verified; None honest
    pub fn verdict(&self, name: &str) -> Verdict;         // "structured_extraction" | "patch_editing" | "long_context"
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict { Ready, Risky, Unusable, Unmeasured }
#[derive(Debug)]
pub enum ProfileError { Parse(String), UnsupportedSchema(u32) }
```

- [ ] **Step 1: Failing tests with a representative v3 fixture.**

```rust
use bloomery_core::profile::{Profile, Verdict};

const FIXTURE: &str = r#"{
  "assay_profile_version": 3,
  "model": {"name": "qwen2.5-coder:7b-instruct-q8_0", "quant": "Q8_0"},
  "ceiling": {"max_verified": 15800, "first_failure": null, "failure_mode": "none_up_to_cap"},
  "verdicts": {
    "structured_extraction": {"verdict": "ready"},
    "patch_editing": {"verdict": "risky"},
    "long_context": {"verdict": "unmeasured"}
  }
}"#;

#[test]
fn parses_v3_fixture() {
    let p = Profile::from_json(FIXTURE).unwrap();
    assert_eq!(p.model_name(), "qwen2.5-coder:7b-instruct-q8_0");
    assert_eq!(p.measured_ceiling(), Some(15800));
    assert_eq!(p.verdict("structured_extraction"), Verdict::Ready);
    assert_eq!(p.verdict("patch_editing"), Verdict::Risky);
    assert_eq!(p.verdict("long_context"), Verdict::Unmeasured);
    assert_eq!(p.verdict("nonexistent"), Verdict::Unmeasured); // absent = unmeasured, law 5
}

#[test]
fn missing_ceiling_is_none() {
    let p = Profile::from_json(r#"{"assay_profile_version": 3, "model": {"name": "m"}}"#).unwrap();
    assert_eq!(p.measured_ceiling(), None);
}

#[test]
fn old_schema_rejected_by_name() {
    let e = Profile::from_json(r#"{"assay_profile_version": 1, "model": {"name": "m"}}"#);
    assert!(matches!(e, Err(bloomery_core::profile::ProfileError::UnsupportedSchema(1))));
}
```

- [ ] **Step 2: Run → failure. Step 3: Implement** (serde with `Option` everywhere; verdict strings map `"ready"→Ready` etc., anything else or absent → `Unmeasured`). **Step 4: Run → pass. Step 5: Commit** — `feat: assay profile ingestion (schema v2+, verdicts, measured ceiling)`.

---

### Task 9: Scheduler — the residency planner

**Files:**
- Create: `crates/bloomery-core/src/scheduler.rs`
- Modify: `crates/bloomery-core/src/lib.rs`
- Test: `crates/bloomery-core/tests/scheduler_test.rs`

**Interfaces:**
- Produces (consumed by Task 13):

```rust
use crate::journal::AgentId;
#[derive(Debug, Clone)]
pub struct Resident { pub id: AgentId, pub priority: u8, pub kv_bytes: u64, pub busy: bool }
#[derive(Debug, Clone)]
pub struct ResidencyRequest { pub id: AgentId, pub priority: u8, pub kv_bytes: u64 }
#[derive(Debug, Clone, PartialEq)]
pub enum Placement {
    Fits,
    Evict(Vec<AgentId>),                                  // lowest-priority-first victim order
    Refuse { needed: u64, free: u64, reclaimable: u64 },  // the arithmetic, law 2
}
pub fn plan_residency(residents: &[Resident], req: &ResidencyRequest, free_vram_bytes: u64) -> Placement;
```

Deterministic mechanism only (law 8 — no LLM policy in Phase 1). Rules: fits in free VRAM → `Fits`. Else evict **idle** residents with **strictly lower** priority, lowest priority first (ties: larger `kv_bytes` first, then lexical id for determinism), until the freed sum + free covers `req.kv_bytes`. Never evict `busy` residents or priority ≥ `req.priority`. If unreachable → `Refuse` with `reclaimable` = total evictable bytes.

- [ ] **Step 1: Failing tests.**

```rust
use bloomery_core::scheduler::*;

fn r(id: &str, pri: u8, kv: u64, busy: bool) -> Resident {
    Resident { id: id.into(), priority: pri, kv_bytes: kv, busy }
}
fn req(pri: u8, kv: u64) -> ResidencyRequest {
    ResidencyRequest { id: "new".into(), priority: pri, kv_bytes: kv }
}

#[test]
fn fits_when_free_vram_suffices() {
    assert_eq!(plan_residency(&[], &req(100, 500), 1000), Placement::Fits);
}

#[test]
fn evicts_lowest_priority_idle_first() {
    let residents = [r("low", 10, 400, false), r("mid", 50, 400, false)];
    assert_eq!(plan_residency(&residents, &req(100, 300), 0), Placement::Evict(vec!["low".into()]));
}

#[test]
fn evicts_multiple_in_priority_order() {
    let residents = [r("mid", 50, 300, false), r("low", 10, 300, false)];
    assert_eq!(plan_residency(&residents, &req(100, 550), 0),
               Placement::Evict(vec!["low".into(), "mid".into()]));
}

#[test]
fn never_evicts_busy_or_equal_priority() {
    let residents = [r("busy-low", 10, 400, true), r("peer", 100, 400, false)];
    match plan_residency(&residents, &req(100, 300), 0) {
        Placement::Refuse { needed, free, reclaimable } => {
            assert_eq!((needed, free, reclaimable), (300, 0, 0));
        }
        other => panic!("expected Refuse, got {other:?}"),
    }
}

#[test]
fn tie_break_is_deterministic() {
    let residents = [r("b", 10, 100, false), r("a", 10, 100, false)];
    // same priority, same size -> lexical id order
    assert_eq!(plan_residency(&residents, &req(100, 150), 0),
               Placement::Evict(vec!["a".into(), "b".into()]));
}
```

- [ ] **Step 2: Run → failure. Step 3: Implement** (collect evictable, sort by `(priority, Reverse(kv_bytes), id)`, accumulate until covered). **Step 4: Run → pass. Step 5: Commit** — `feat: deterministic residency planner (priority eviction, refusal arithmetic)`.

---

### Task 10: Substrate trait, FakeSubstrate, contract enforcement

**Files:**
- Create: `crates/bloomery-substrate/Cargo.toml`, `crates/bloomery-substrate/src/lib.rs`, `crates/bloomery-substrate/src/fake.rs`, `crates/bloomery-substrate/src/contract.rs`
- Modify: root `Cargo.toml` (add member)
- Test: `crates/bloomery-substrate/tests/fake_test.rs`

**Interfaces:**
- Produces (consumed by Tasks 11–15):

```rust
// lib.rs
pub type ModelHandle = u64;
pub type CtxHandle = u64;
#[derive(Debug, Clone, PartialEq)]
pub struct Reply {
    pub text: String,
    pub prompt_tokens: Option<u32>,      // None = the substrate failed to report; contract catches it
    pub completion_tokens: Option<u32>,
    pub duration_ms: u64,
}
#[derive(Debug)]
pub enum SubstrateError { ModelLoad(String), Context(String), Infer(String), State(String) }
pub trait Substrate {
    fn load_model(&mut self, path: &std::path::Path, n_gpu_layers: u32) -> Result<ModelHandle, SubstrateError>;
    fn unload_model(&mut self, m: ModelHandle) -> Result<(), SubstrateError>;
    fn create_context(&mut self, m: ModelHandle, n_ctx: u32) -> Result<CtxHandle, SubstrateError>;
    fn destroy_context(&mut self, c: CtxHandle) -> Result<(), SubstrateError>;
    fn infer(&mut self, c: CtxHandle, prompt: &str, max_tokens: u32) -> Result<Reply, SubstrateError>;
    fn save_state(&mut self, c: CtxHandle) -> Result<Vec<u8>, SubstrateError>;
    fn load_state(&mut self, c: CtxHandle, bytes: &[u8]) -> Result<(), SubstrateError>;
}

// contract.rs
#[derive(Debug, Clone, PartialEq)]
pub struct VerifiedReply { pub text: String, pub prompt_tokens: u32, pub completion_tokens: u32, pub duration_ms: u64 }
#[derive(Debug, PartialEq)]
pub enum ContractViolation { MissingStats }
pub fn enforce_contract(r: Reply) -> Result<VerifiedReply, ContractViolation>;

// fake.rs
pub struct FakeSubstrate { /* scripted replies, call log, per-ctx token history */ }
impl FakeSubstrate {
    pub fn new() -> Self;
    pub fn script_reply(&mut self, r: Reply);              // FIFO queue
    pub fn calls(&self) -> &[String];                      // e.g. "load_model", "infer:c1"
    pub fn ctx_history(&self, c: CtxHandle) -> Option<&str>; // accumulated prompt text, survives save/load
}
impl Substrate for FakeSubstrate { /* ... */ }
```

- [ ] **Step 1: Failing tests.**

```rust
use bloomery_substrate::{Substrate, Reply, fake::FakeSubstrate, contract::*};

fn ok_reply(text: &str) -> Reply {
    Reply { text: text.into(), prompt_tokens: Some(10), completion_tokens: Some(3), duration_ms: 5 }
}

#[test]
fn fake_serves_scripted_replies_and_logs_calls() {
    let mut s = FakeSubstrate::new();
    s.script_reply(ok_reply("hello"));
    let m = s.load_model(std::path::Path::new("/fake.gguf"), 99).unwrap();
    let c = s.create_context(m, 4096).unwrap();
    let r = s.infer(c, "hi", 32).unwrap();
    assert_eq!(r.text, "hello");
    assert!(s.calls().iter().any(|x| x.starts_with("infer")));
}

#[test]
fn state_round_trip_preserves_context_history() {
    let mut s = FakeSubstrate::new();
    s.script_reply(ok_reply("a"));
    let m = s.load_model(std::path::Path::new("/fake.gguf"), 99).unwrap();
    let c1 = s.create_context(m, 4096).unwrap();
    s.infer(c1, "first prompt", 32).unwrap();
    let img = s.save_state(c1).unwrap();
    s.destroy_context(c1).unwrap();
    let c2 = s.create_context(m, 4096).unwrap();
    s.load_state(c2, &img).unwrap();
    assert_eq!(s.ctx_history(c2).unwrap(), "first prompt");
}

#[test]
fn contract_rejects_missing_stats() {
    let bad = Reply { text: "plausible".into(), prompt_tokens: None, completion_tokens: None, duration_ms: 9 };
    assert_eq!(enforce_contract(bad), Err(ContractViolation::MissingStats));
    let good = enforce_contract(ok_reply("x")).unwrap();
    assert_eq!((good.prompt_tokens, good.completion_tokens), (10, 3));
}
```

- [ ] **Step 2: Run → failure. Step 3: Implement.** FakeSubstrate: `HashMap<CtxHandle, String>` history; `save_state` = history bytes; `load_state` = restore; handles from a counter; scripted replies popped FIFO (empty queue → `SubstrateError::Infer("script exhausted")`). `enforce_contract`: both stats present or `MissingStats`.
- [ ] **Step 4: Run → pass. Step 5: Commit** — `feat: substrate trait + scripted fake + stats contract enforcement`.

---

### Task 11: Llama substrate (feature-gated, live-tested)

**Files:**
- Create: `crates/bloomery-substrate/src/llama.rs`
- Modify: `crates/bloomery-substrate/Cargo.toml` (deps `llama-cpp-2`/`llama-cpp-sys-2` behind feature `llama`, feature `vulkan` forwarded), `crates/bloomery-substrate/src/lib.rs` (`#[cfg(feature = "llama")] pub mod llama;`)
- Test: `crates/bloomery-substrate/tests/llama_live_test.rs`

**Interfaces:**
- Consumes: exact symbol names recorded in Task 1's decision record (D1) — those supersede the sketch below if they differ.
- Produces: `pub struct LlamaSubstrate;` with `pub fn new() -> Result<LlamaSubstrate, SubstrateError>`, implementing `Substrate`.

- [ ] **Step 1: Write the live test (fails for the right reason first: type absent).**

```rust
// Requires: BLOOMERY_LIVE=1, BLOOMERY_TEST_GGUF=/path/to/qwen2.5-coder-7b-q8.gguf, --features llama
#[test]
#[ignore]
fn live_infer_reports_stats_and_state_round_trips() {
    if std::env::var("BLOOMERY_LIVE").as_deref() != Ok("1") { return; }
    use bloomery_substrate::{Substrate, llama::LlamaSubstrate};
    let gguf = std::env::var("BLOOMERY_TEST_GGUF").expect("set BLOOMERY_TEST_GGUF");
    let mut s = LlamaSubstrate::new().unwrap();
    let m = s.load_model(std::path::Path::new(&gguf), 99).unwrap();
    let c = s.create_context(m, 2048).unwrap();
    let r = s.infer(c, "Reply with exactly: OK", 8).unwrap();
    assert!(r.prompt_tokens.is_some() && r.completion_tokens.is_some(),
            "real counts by construction — we count decoded tokens ourselves");
    let img = s.save_state(c).unwrap();
    assert!(!img.is_empty());
    s.destroy_context(c).unwrap();
    let c2 = s.create_context(m, 2048).unwrap();
    s.load_state(c2, &img).unwrap();
    let r2 = s.infer(c2, " Again: OK", 8).unwrap(); // continues on restored KV
    assert!(r2.completion_tokens.unwrap() > 0);
}
```

- [ ] **Step 2: Implement `LlamaSubstrate`.**

Sketch (adjust names to Task 1's D1 record): hold `LlamaBackend` + maps `ModelHandle → LlamaModel`, `CtxHandle → LlamaContext`. `load_model`: `LlamaModel::load_from_file(&backend, path, &LlamaModelParams::default().with_n_gpu_layers(n))`, timed (Task 13 journals `ModelLoaded`). `create_context`: `model.new_context(&backend, LlamaContextParams::default().with_n_ctx(NonZeroU32::new(n_ctx)))`. `infer`: tokenize prompt (`model.str_to_token(prompt, AddBos::Always)` on a fresh context, `AddBos::Never` on a restored one — track per-ctx whether state was loaded), decode via `LlamaBatch`, sample greedily up to `max_tokens` or EOG; `prompt_tokens` = tokenized length, `completion_tokens` = decoded count — **real counts by construction** (we counted them; no path constructs a `Reply` with `None` here). `save_state`/`load_state`: safe wrappers if the crate has them, else `unsafe` through `llama_cpp_sys_2::{llama_state_get_size, llama_state_get_data, llama_state_set_data}` on the raw context pointer, per D1. Borrow-check note: `LlamaContext` borrows its model in `llama-cpp-2` — if the map-of-contexts fights the borrow checker, store contexts in a self-referential-free design: keep `(ModelHandle, LlamaContext<'static>)` via `Box::leak` of models is NOT acceptable; instead scope contexts inside an arena struct that owns models and contexts together, and expose only handles (implementer's choice; the trait surface is what's fixed).

- [ ] **Step 3: Verify GPU-free suite is untouched.**

Run: `cargo test --workspace` (no `--features llama`) → all pass, llama code not even compiled.

- [ ] **Step 4: Run the live test on the box.**

Run: `BLOOMERY_LIVE=1 BLOOMERY_TEST_GGUF=$HOME/.ollama/models/blobs/<qwen-q8-blob> cargo test -p bloomery-substrate --features llama,vulkan -- --ignored live_infer`
Expected: PASS. (Blob path: resolve via `ollama show qwen2.5-coder:7b-instruct-q8_0 --modelfile` FROM line, or any standalone GGUF on disk.)

- [ ] **Step 5: Commit** — `feat: llama.cpp substrate (vulkan) with state save/restore`.

---

### Task 12: Daemon skeleton — config, agent table, KV image store

**Files:**
- Create: `crates/bloomery-daemon/Cargo.toml` (deps: `bloomery-core`, `bloomery-substrate`, `tiny_http`, `serde`, `serde_json`, `toml`, `sha2`; feature `llama` forwarding to substrate), `crates/bloomery-daemon/src/main.rs`, `crates/bloomery-daemon/src/config.rs`, `crates/bloomery-daemon/src/agents.rs`
- Modify: root `Cargo.toml` (member)
- Test: `crates/bloomery-daemon/tests/agents_test.rs`, `crates/bloomery-daemon/tests/config_test.rs`

**Interfaces:**
- Consumes: `GgufMeta`, `Window`, `Budget`, `Journal`, `Profile` (Tasks 3–8).
- Produces (consumed by Tasks 13–16):

```rust
// config.rs
#[derive(Debug, Clone, serde::Deserialize)]
pub struct Config {
    pub port: u16,                       // default 8181
    pub data_dir: std::path::PathBuf,    // journal/, profiles/, images/
    pub models: std::collections::BTreeMap<String, std::path::PathBuf>, // name -> gguf
    pub tier: Tier,
    pub overhead_mib: u64,               // default 1024
    pub default_priority: u8,            // default 100
    pub default_budget_tokens: u64,      // default 200_000
    pub allow_unprofiled: bool,          // default false
    pub assay: AssayConfig,
}
#[derive(Debug, Clone, serde::Deserialize)]
pub struct Tier { pub name: String, pub emulated: bool }
#[derive(Debug, Clone, serde::Deserialize)]
pub struct AssayConfig { pub enabled: bool, pub python: String } // default "python3"
pub fn load_config(path: &std::path::Path) -> Result<Config, String>; // named errors, defaults applied

// agents.rs
pub struct Agent {
    pub id: bloomery_core::journal::AgentId,
    pub model: String,
    pub priority: u8,
    pub window: bloomery_core::geometry::Window,
    pub budget: bloomery_core::budget::Budget,
    pub kv_bytes: u64,                   // window.tokens * kv_per_token
    pub state: AgentState,
}
pub enum AgentState { Resident { ctx: bloomery_substrate::CtxHandle }, Suspended, Fresh }
pub struct AgentTable { /* map */ }     // new/insert/get/get_mut/remove/residents()->Vec<scheduler::Resident>
pub struct ImageStore { /* ram map + spill dir */ }
impl ImageStore {
    pub fn new(spill_dir: &std::path::Path) -> std::io::Result<ImageStore>;
    pub fn put_ram(&mut self, id: &str, digest: &str, bytes: Vec<u8>);
    pub fn spill(&mut self, id: &str) -> std::io::Result<()>;         // ram -> {id}.{digest}.kvimg
    pub fn take(&mut self, id: &str, expect_digest: &str) -> ImageFetch;
}
pub enum ImageFetch { Ram(Vec<u8>), Nvme(Vec<u8>), StaleDigest, Missing }
pub fn model_digest(gguf: &std::path::Path) -> std::io::Result<String>; // sha256(first 1 MiB || file_len)
```

- [ ] **Step 1: Failing tests.**

```rust
use bloomery_daemon::agents::{ImageStore, ImageFetch, model_digest};

#[test]
fn image_ram_then_spill_then_take() {
    let dir = std::env::temp_dir().join("bloomery-img-test");
    let _ = std::fs::remove_dir_all(&dir);
    let mut st = ImageStore::new(&dir).unwrap();
    st.put_ram("a1", "digestX", vec![1, 2, 3]);
    match st.take("a1", "digestX") { ImageFetch::Ram(b) => assert_eq!(b, vec![1, 2, 3]), o => panic!("{o:?}") }
    st.put_ram("a1", "digestX", vec![1, 2, 3]);
    st.spill("a1").unwrap();
    match st.take("a1", "digestX") { ImageFetch::Nvme(b) => assert_eq!(b, vec![1, 2, 3]), o => panic!("{o:?}") }
}

#[test]
fn stale_digest_is_invalidation_not_error() {
    let dir = std::env::temp_dir().join("bloomery-img-test2");
    let _ = std::fs::remove_dir_all(&dir);
    let mut st = ImageStore::new(&dir).unwrap();
    st.put_ram("a1", "old", vec![9]);
    assert!(matches!(st.take("a1", "new"), ImageFetch::StaleDigest));
    assert!(matches!(st.take("nobody", "d"), ImageFetch::Missing));
}

#[test]
fn digest_changes_with_content() {
    let dir = std::env::temp_dir().join("bloomery-digest-test");
    std::fs::create_dir_all(&dir).unwrap();
    let (p1, p2) = (dir.join("m1"), dir.join("m2"));
    std::fs::write(&p1, b"AAAA").unwrap();
    std::fs::write(&p2, b"BBBB").unwrap();
    assert_ne!(model_digest(&p1).unwrap(), model_digest(&p2).unwrap());
}
```

Config test: write a minimal TOML to a tempfile (`port`, one model, `tier = { name = "enthusiast-16gb", emulated = false }`, `assay = { enabled = false, python = "python3" }`), assert defaults fill (`overhead_mib == 1024`, `allow_unprofiled == false`), and a missing `models` table yields a named error containing `"models"`.

- [ ] **Step 2: Run → failure. Step 3: Implement.** `main.rs` for now: parse `--config path`, `load_config`, open journal at `data_dir/journal/boot-<unix_ts>.jsonl`, append `Event::Boot`, print listening message, exit 0 (server loop lands in Task 14).
- [ ] **Step 4: Run → pass; clippy/fmt. Step 5: Commit** — `feat: daemon config, agent table, digest-tagged KV image store`.

---

### Task 13: The pager

**Files:**
- Create: `crates/bloomery-daemon/src/pager.rs`
- Modify: `crates/bloomery-daemon/src/main.rs` (mod decl)
- Test: `crates/bloomery-daemon/tests/pager_test.rs`

**Interfaces:**
- Consumes: everything above; generic over `S: Substrate` so all tests run on `FakeSubstrate`.
- Produces (consumed by Tasks 14–15):

```rust
pub struct Pager<S: bloomery_substrate::Substrate> { /* table, images, journal, substrate, models, free_vram: Box<dyn Fn() -> Option<u64>> */ }
impl<S: Substrate> Pager<S> {
    pub fn new(substrate: S, journal: Journal, image_store: ImageStore,
               free_vram: Box<dyn Fn() -> Option<u64>>) -> Pager<S>;
    pub fn register_model(&mut self, name: &str, gguf: &std::path::Path,
                          meta: GgufMeta, profile: Option<Profile>) -> Result<(), PagerError>;
    pub fn create_agent(&mut self, model: &str, priority: u8, window_cap: Option<u32>,
                        budget_tokens: u64) -> Result<AgentInfo, PagerError>;
    pub fn infer(&mut self, id: &str, prompt: &str, max_tokens: u32) -> Result<VerifiedReply, PagerError>;
    pub fn suspend(&mut self, id: &str) -> Result<(), PagerError>;
    pub fn resume(&mut self, id: &str) -> Result<(), PagerError>;   // ensure resident, no infer
    pub fn unload_model(&mut self, name: &str) -> Result<(), PagerError>; // for cold-switch bench
    pub fn status(&self) -> StatusReport;                            // serializable snapshot
}
#[derive(Debug, serde::Serialize)]
pub struct AgentInfo { pub id: String, pub window_tokens: u32, pub bound_by: String }
#[derive(Debug)]
pub enum PagerError {
    UnknownModel(String), UnknownAgent(String),
    Unprofiled(String),                                   // model has no profile, allow_unprofiled=false
    Refused { needed: u64, free: u64, reclaimable: u64 }, // residency arithmetic
    PromptTooLarge { needed_tokens: u64, window_tokens: u32 },
    Budget { remaining: u64, requested: u64 },
    Contract(String),
    Substrate(String),
}
```

Behavior contract (this is the heart of Phase 1):

1. `create_agent`: window from `usable_window` (GeometryInput from meta + live `free_vram()` + overhead + cap + profile's `measured_ceiling`) → journal `AgentCreated`. No VRAM committed yet (`Fresh`).
2. `infer`: budget `check(max_tokens)` (refuse → journal `BudgetRefused`); prompt-size gate: `estimate ≤ window` where estimate = `prompt.len()/3 + max_tokens` chars→tokens floor — conservative, and the *substrate's* real tokenization refusal in Task 11 is the backstop (law 2: refuse, never truncate) → journal `Refusal`; ensure resident: `plan_residency` over `AgentTable::residents()` → on `Evict(victims)`: for each victim `save_state` → `put_ram` + journal `PagerOp{EvictSave, image_tier:"ram"}` → `destroy_context`; on `Refuse` → `PagerError::Refused` + journal; then create context (loading model first if needed, journal `ModelLoaded`), and if an image exists `take(id, digest)`: `Ram`→`load_state` + journal `PagerOp{ResumeLoad, image_tier:"ram"}`; `Nvme`→ same with `"nvme"`; `StaleDigest`→ journal `Degraded{reason:"stale image digest, cold start"}` and proceed fresh; run `substrate.infer` → `enforce_contract` (violation → journal `ContractViolation`, return `PagerError::Contract` — infrastructure, never model failure) → `budget.charge(prompt+completion)` → journal `InferStarted`/`InferCompleted` around the call.
3. `suspend`: save_state → `put_ram` → `spill()` → journal `PagerOp{SuspendSave, image_tier:"nvme"}` → destroy context.
4. VRAM accounting for `plan_residency`: `free = free_vram() − Σ resident kv_bytes` conservative model; `free_vram() == None` → journal `Degraded{reason:"vram unmeasured"}` once and fall back to residency-count cap of 1 (the most conservative honest behavior).

- [ ] **Step 1: Write the failing test — the journaled eviction story.**

```rust
use bloomery_daemon::pager::*;
use bloomery_substrate::{fake::FakeSubstrate, Reply};
use bloomery_core::journal::{replay, Event, PagerOpKind};

fn ok(text: &str) -> Reply {
    Reply { text: text.into(), prompt_tokens: Some(8), completion_tokens: Some(4), duration_ms: 3 }
}
fn meta() -> bloomery_core::gguf::GgufMeta {
    bloomery_core::gguf::GgufMeta { arch: "qwen2".into(), layers: 28, kv_heads: 4,
        head_dim: 128, training_ctx: 4096, weights_bytes: 1000 }
}

#[test]
fn eviction_under_pressure_saves_image_and_journals() {
    let dir = std::env::temp_dir().join("bloomery-pager-test");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let jpath = dir.join("j.jsonl");
    let journal = bloomery_core::journal::Journal::open(&jpath).unwrap();
    let images = bloomery_daemon::agents::ImageStore::new(&dir.join("img")).unwrap();
    let mut fake = FakeSubstrate::new();
    for _ in 0..4 { fake.script_reply(ok("r")); }
    // free VRAM fits exactly one 4096-token qwen-geometry context (4096 * 56 KiB = 224 MiB) + slack
    let mut p = Pager::new(fake, journal, images, Box::new(|| Some(300 * 1024 * 1024)));
    let gguf = dir.join("fake.gguf");
    std::fs::write(&gguf, b"weights").unwrap();
    p.register_model("qwen", &gguf, meta(), None).unwrap();
    // allow_unprofiled behavior is the daemon's (Task 16); Pager::register_model with profile None is permitted
    let a = p.create_agent("qwen", 50, None, 10_000).unwrap();
    let b = p.create_agent("qwen", 100, None, 10_000).unwrap();
    p.infer(&a.id, "hello from a", 16).unwrap();          // a becomes resident
    p.infer(&b.id, "hello from b", 16).unwrap();          // must evict a (lower priority)
    let events = replay(&jpath).unwrap();
    assert!(events.iter().any(|e| matches!(e,
        Event::PagerOp { id, op: PagerOpKind::EvictSave, .. } if id == &a.id)));
    p.infer(&a.id, "back again", 16).unwrap();            // resumes a from RAM image
    let events = replay(&jpath).unwrap();
    assert!(events.iter().any(|e| matches!(e,
        Event::PagerOp { id, op: PagerOpKind::ResumeLoad, image_tier, .. }
            if id == &a.id && image_tier == "ram")));
}

#[test]
fn oversized_prompt_is_refused_with_arithmetic_never_truncated() {
    let dir = std::env::temp_dir().join("bloomery-pager-test2");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let journal = bloomery_core::journal::Journal::open(&dir.join("j.jsonl")).unwrap();
    let images = bloomery_daemon::agents::ImageStore::new(&dir.join("img")).unwrap();
    let mut p = Pager::new(FakeSubstrate::new(), journal, images, Box::new(|| Some(10u64.pow(9))));
    let gguf = dir.join("fake.gguf");
    std::fs::write(&gguf, b"w").unwrap();
    p.register_model("qwen", &gguf, meta(), None).unwrap();
    let a = p.create_agent("qwen", 50, Some(64), 10_000).unwrap(); // 64-token window
    let big = "x".repeat(10_000);
    match p.infer(&a.id, &big, 16) {
        Err(PagerError::PromptTooLarge { needed_tokens, window_tokens }) => {
            assert!(needed_tokens > 64);
            assert_eq!(window_tokens, 64);
        }
        other => panic!("expected PromptTooLarge, got {other:?}"),
    }
}

#[test]
fn budget_exhaustion_refuses_before_the_call() {
    let dir = std::env::temp_dir().join("bloomery-pager-test3");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let journal = bloomery_core::journal::Journal::open(&dir.join("j.jsonl")).unwrap();
    let images = bloomery_daemon::agents::ImageStore::new(&dir.join("img")).unwrap();
    let mut p = Pager::new(FakeSubstrate::new(), journal, images, Box::new(|| Some(10u64.pow(9))));
    let gguf = dir.join("fake.gguf");
    std::fs::write(&gguf, b"w").unwrap();
    p.register_model("qwen", &gguf, meta(), None).unwrap();
    let a = p.create_agent("qwen", 50, None, 10).unwrap(); // 10-token budget
    match p.infer(&a.id, "hi", 100) {
        Err(PagerError::Budget { remaining: 10, requested: 100 }) => {}
        other => panic!("expected Budget, got {other:?}"),
    }
}
```

- [ ] **Step 2: Run → failure. Step 3: Implement `pager.rs`** per the behavior contract above (keep it under 400 lines; if it grows, split journaling helpers into `pager_journal.rs`).
- [ ] **Step 4: Run → pass; clippy/fmt. Step 5: Commit** — `feat: pager - priority eviction, KV image round-trip, refusals, journaled`.

---

### Task 14: Native HTTP API

**Files:**
- Create: `crates/bloomery-daemon/src/api_native.rs`, `crates/bloomery-daemon/src/http.rs` (tiny router + JSON helpers shared with Task 15)
- Modify: `crates/bloomery-daemon/src/main.rs` (serve loop)
- Test: `crates/bloomery-daemon/tests/api_native_test.rs`

**Interfaces:**
- Consumes: `Pager<S>` (Task 13) behind `Arc<Mutex<...>>`.
- Produces: HTTP surface (JSON bodies; also consumed by `bloomery-bench`):
  - `POST /agents` `{model, priority?, window_cap?, budget_tokens?}` → `201 {id, window_tokens, bound_by}` | `409 {error:"refused", needed, free, reclaimable}` | `422 {error:"unprofiled", model}`
  - `POST /agents/{id}/infer` `{prompt, max_tokens}` → `200 {text, prompt_tokens, completion_tokens, duration_ms}` | `402 {error:"budget_exhausted", remaining, requested}` | `413 {error:"prompt_too_large", needed_tokens, window_tokens}` | `409` refusal | `502 {error:"contract_violation", kind}`
  - `POST /agents/{id}/suspend` → `204`; `POST /agents/{id}/resume` → `204`
  - `POST /models/{name}/unload` → `204` (cold-switch bench support)
  - `GET /status` → `200` `StatusReport` JSON (residents, windows, budgets, vram, tier)
- Produces for tests + bench: `pub fn serve<S: Substrate + Send + 'static>(pager: Pager<S>, port: u16) -> (u16, ServerHandle)` — binds `127.0.0.1:port` (0 = ephemeral, returns actual), `ServerHandle::shutdown()`.

Error-code mapping is part of the contract: refusals are structured JSON with the arithmetic (law 2), never a truncated success.

- [ ] **Step 1: Failing test with a std-only HTTP helper.**

```rust
// tests/api_native_test.rs
use std::io::{Read, Write};

fn http(addr: &str, method: &str, path: &str, body: &str) -> (u16, String) {
    let mut s = std::net::TcpStream::connect(addr).unwrap();
    write!(s, "{method} {path} HTTP/1.1\r\nHost: x\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
    let mut buf = String::new();
    s.read_to_string(&mut buf).unwrap();
    let status: u16 = buf.split_whitespace().nth(1).unwrap().parse().unwrap();
    let body = buf.split("\r\n\r\n").nth(1).unwrap_or("").to_string();
    (status, body)
}

#[test]
fn create_infer_and_refusal_over_http() {
    let (port, _handle) = bloomery_daemon::test_support::serve_fake(); // helper: pager on FakeSubstrate, scripted replies
    let addr = format!("127.0.0.1:{port}");
    let (st, body) = http(&addr, "POST", "/agents", r#"{"model":"qwen","budget_tokens":1000}"#);
    assert_eq!(st, 201, "{body}");
    let id = serde_json::from_str::<serde_json::Value>(&body).unwrap()["id"].as_str().unwrap().to_string();
    let (st, body) = http(&addr, "POST", &format!("/agents/{id}/infer"), r#"{"prompt":"hi","max_tokens":16}"#);
    assert_eq!(st, 200, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert!(v["prompt_tokens"].as_u64().is_some());
    let big = format!(r#"{{"prompt":"{}","max_tokens":16}}"#, "x".repeat(100_000));
    let (st, body) = http(&addr, "POST", &format!("/agents/{id}/infer"), &big);
    assert_eq!(st, 413, "{body}");
    assert!(body.contains("prompt_too_large") && body.contains("window_tokens"));
}
```

`test_support` (in `bloomery-daemon/src/test_support.rs`, `#[cfg(any(test, feature = "test-support"))]` — the integration tests and bench enable feature `test-support`): builds a `Pager<FakeSubstrate>` with one registered fake model (qwen-like meta, small window), 32 scripted ok-replies, tempdir journal/images, `free_vram` = 1 GiB, and calls `serve(pager, 0)`.

- [ ] **Step 2: Run → failure. Step 3: Implement** `http.rs` (parse method/path/segments, read body, `respond_json(status, value)`) + `api_native.rs` (match routes → pager calls → error mapping table above) + `main.rs`: after boot, `serve(pager, config.port)` and block. Threading: `tiny_http` server on its own thread pool of 4, pager behind `Arc<Mutex<_>>` (coarse lock is Phase 1-correct: one GPU, serialized inference is reality, and G2 measures switches not concurrency).
- [ ] **Step 4: Run → pass; clippy/fmt. Step 5: Commit** — `feat: native HTTP API with structured refusals`.

---

### Task 15: `/v1` shim (OpenAI-compatible, honest)

**Files:**
- Create: `crates/bloomery-daemon/src/api_v1.rs`
- Modify: `crates/bloomery-daemon/src/http.rs` (route dispatch), `crates/bloomery-daemon/src/api_native.rs` (none — shared serve loop dispatches on path prefix)
- Test: `crates/bloomery-daemon/tests/api_v1_test.rs`

**Interfaces:**
- Consumes: `Pager` via the same `Arc<Mutex<_>>`; D3/D4 decisions from Task 1.
- Produces:
  - `GET /v1/models` → `{object:"list", data:[{id:"<model-name>", object:"model", owned_by:"bloomery"}]}`
  - `POST /v1/chat/completions` `{model, messages, max_tokens?, stream?}`:
    - Non-stream: `200` OpenAI-shaped response; **`usage` always populated with real counts** (`prompt_tokens`, `completion_tokens`, `total_tokens`).
    - Oversize: `400 {"error":{"type":"invalid_request_error","code":"prompt_too_large","message":"prompt needs <N> tokens; window is <W> (bound by <term>); refusing rather than truncating","param":"messages"}}` — the honest-refusal contract, never silent truncation.
    - `stream:true`: SSE per D3 — `data: {chunk with delta}` lines, final chunk carries `usage`, then `data: [DONE]`.
  - Prompt assembly per D4 (model chat template if exposed, else the documented `"{role}: {content}\n"` + `"assistant: "` fallback — record which in the response header `X-Bloomery-Template: model|fallback`).
  - Session binding: header `X-Bloomery-Agent: <id>` routes to an existing agent (KV/prefix reuse across calls); absent → ephemeral agent at `default_priority`/`default_budget_tokens`, removed after the response.

- [ ] **Step 1: Failing tests.**

```rust
#[test]
fn chat_completion_has_real_usage() {
    let (port, _h) = bloomery_daemon::test_support::serve_fake();
    let addr = format!("127.0.0.1:{port}");
    let (st, body) = http(&addr, "POST", "/v1/chat/completions",
        r#"{"model":"qwen","messages":[{"role":"user","content":"hi"}],"max_tokens":16}"#);
    assert_eq!(st, 200, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["object"], "chat.completion");
    assert!(v["usage"]["prompt_tokens"].as_u64().is_some());
    assert!(v["usage"]["completion_tokens"].as_u64().is_some());
    assert!(v["choices"][0]["message"]["content"].as_str().is_some());
}

#[test]
fn oversized_prompt_gets_honest_400_not_truncation() {
    let (port, _h) = bloomery_daemon::test_support::serve_fake();
    let addr = format!("127.0.0.1:{port}");
    let big = "x".repeat(100_000);
    let req = format!(r#"{{"model":"qwen","messages":[{{"role":"user","content":"{big}"}}],"max_tokens":16}}"#);
    let (st, body) = http(&addr, "POST", "/v1/chat/completions", &req);
    assert_eq!(st, 400, "{body}");
    assert!(body.contains("prompt_too_large"));
    assert!(body.contains("refusing rather than truncating"));
}

#[test]
fn models_lists_configured_models() {
    let (port, _h) = bloomery_daemon::test_support::serve_fake();
    let (st, body) = http(&format!("127.0.0.1:{port}"), "GET", "/v1/models", "");
    assert_eq!(st, 200);
    assert!(body.contains("qwen"));
}
```

(Reuse the `http` helper by moving it into `tests/common/mod.rs`; both integration test files `mod common;` it.)

- [ ] **Step 2: Run → failure. Step 3: Implement** non-stream + `/v1/models` first; then SSE streaming (Phase 1 streaming may buffer whole-reply and emit it as one delta chunk + usage + `[DONE]` — honest note in the code: chunked *token* streaming needs a streaming `Substrate::infer` and is recorded in the README as a known Phase 1 limit; the wire format is already SSE-correct so clients work today).
- [ ] **Step 4: Run → pass; clippy/fmt. Step 5: Commit** — `feat: /v1 shim - real usage, honest prompt_too_large refusal, SSE`.

---

### Task 16: assay-as-POST boot

**Files:**
- Create: `crates/bloomery-daemon/src/post.rs`
- Modify: `crates/bloomery-daemon/src/main.rs` (boot sequence), `crates/bloomery-daemon/src/pager.rs` (admission uses profiles: creating an agent for a model whose registered `Profile` is `None` fails `PagerError::Unprofiled` unless `allow_unprofiled`; wire the config flag through)
- Test: `crates/bloomery-daemon/tests/post_test.rs`

**Interfaces:**
- Consumes: `Profile` (Task 8), `Config.assay` (Task 12), the running `/v1` server (Task 15).
- Produces:

```rust
pub struct PostRunner { /* python path, injectable command runner for tests */ }
impl PostRunner {
    pub fn new(python: String) -> PostRunner;
    #[cfg(any(test, feature = "test-support"))]
    pub fn with_runner(f: Box<dyn Fn(&str, &[String]) -> std::io::Result<std::process::Output>>) -> PostRunner;
    /// Runs: {python} -m assay probe http://127.0.0.1:{port} --model {model} --quick \
    ///        --backend openai --json {data_dir}/profiles/{model}.json \
    ///        --tier {tier} {--real-hardware | --emulated}
    pub fn probe(&self, port: u16, model: &str, tier: &Tier, out: &std::path::Path)
        -> Result<Profile, PostError>;
}
#[derive(Debug)]
pub enum PostError { Spawn(String), NonZeroExit { code: i32, stderr: String }, BadProfile(String) }
```

Boot sequence in `main.rs` (the chicken-and-egg resolution from the spec discussion): load config → open journal → `Event::Boot` → register models **unprofiled** (POST-only provisional admission: the daemon marks itself `posting` and `/v1` accepts calls while `posting` regardless of profiles) → start server → if `assay.enabled`: for each model run `PostRunner::probe` against `127.0.0.1:{port}`, attach the returned `Profile` to the model, journal `Event::Post{outcome:"ok"}`; on `PostError`: journal `Post{outcome:"failed: …"}` + `Degraded{reason}` — the model stays unprofiled, and after POST completes the `posting` flag drops so normal admission (law 5) applies. `assay.enabled=false` → journal `Degraded{reason:"POST disabled by config"}`.

- [ ] **Step 1: Failing tests (fake runner — no python, no GPU).**

```rust
use bloomery_daemon::post::{PostRunner, PostError};
use bloomery_daemon::config::Tier;

fn fake_output(code: i32, stdout: &str, stderr: &str) -> std::process::Output {
    use std::os::unix::process::ExitStatusExt;
    std::process::Output {
        status: std::process::ExitStatus::from_raw(code << 8),
        stdout: stdout.into(), stderr: stderr.into(),
    }
}

#[test]
fn probe_success_parses_written_profile() {
    let dir = std::env::temp_dir().join("bloomery-post-test");
    std::fs::create_dir_all(&dir).unwrap();
    let out = dir.join("qwen.json");
    let profile_json = r#"{"assay_profile_version":3,"model":{"name":"qwen"},"verdicts":{}}"#;
    let out_clone = out.clone();
    let runner = PostRunner::with_runner(Box::new(move |_py, _args| {
        std::fs::write(&out_clone, profile_json).unwrap();   // assay writes --json path
        Ok(fake_output(0, "", ""))
    }));
    let tier = Tier { name: "enthusiast-16gb".into(), emulated: false };
    let p = runner.probe(8181, "qwen", &tier, &out).unwrap();
    assert_eq!(p.model_name(), "qwen");
}

#[test]
fn nonzero_exit_is_named_infrastructure_failure() {
    let runner = PostRunner::with_runner(Box::new(|_, _| Ok(fake_output(4, "", "no daemon"))));
    let tier = Tier { name: "t".into(), emulated: true };
    let out = std::env::temp_dir().join("bloomery-post-test").join("x.json");
    match runner.probe(8181, "m", &tier, &out) {
        Err(PostError::NonZeroExit { code: 4, stderr }) => assert!(stderr.contains("no daemon")),
        other => panic!("{other:?}"),
    }
}

#[test]
fn command_line_is_exactly_the_documented_invocation() {
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let seen2 = seen.clone();
    let runner = PostRunner::with_runner(Box::new(move |py, args| {
        let mut v = vec![py.to_string()];
        v.extend(args.iter().cloned());
        *seen2.lock().unwrap() = v;
        Err(std::io::Error::from(std::io::ErrorKind::NotFound)) // stop after capture
    }));
    let tier = Tier { name: "enthusiast-16gb".into(), emulated: false };
    let out = std::path::PathBuf::from("/tmp/p.json");
    let _ = runner.probe(9999, "qwen", &tier, &out);
    let cmd = seen.lock().unwrap().join(" ");
    assert!(cmd.contains("-m assay probe http://127.0.0.1:9999"));
    assert!(cmd.contains("--model qwen"));
    assert!(cmd.contains("--backend openai"));
    assert!(cmd.contains("--quick"));
    assert!(cmd.contains("--tier enthusiast-16gb"));
    assert!(cmd.contains("--real-hardware"));
    assert!(!cmd.contains("--emulated"));
}
```

Plus a pager-level test: `create_agent` on an unprofiled model with `allow_unprofiled=false` → `PagerError::Unprofiled`; with `true` → succeeds and the journal contains `Degraded{reason}` mentioning the model.

- [ ] **Step 2: Run → failure. Step 3: Implement** (`std::process::Command` real runner; profile read back from the `--json` path, not stdout). Wire boot sequence + `posting` flag + `allow_unprofiled` into pager admission.
- [ ] **Step 4: Run → pass; clippy/fmt. Step 5: Commit** — `feat: assay POST at boot, profile-gated admission, degraded boot`.

---

### Task 17: bloomery-bench, the live G2 run, evidence, README

**Files:**
- Create: `crates/bloomery-bench/Cargo.toml` (deps: `bloomery-core`, `serde_json`; binary), `crates/bloomery-bench/src/main.rs`, `crates/bloomery-bench/src/report.rs`
- Create: `docs/superpowers/evidence/2026-08-XX-g2-agent-switch.md` (dated on run day), `README.md`
- Modify: root `Cargo.toml` (member)
- Test: `crates/bloomery-bench/tests/report_test.rs`

**Interfaces:**
- Consumes: journal `Event`/`PagerOpKind` (Task 7), native API (Task 14).
- Produces:
  - `bloomery-bench switch --daemon http://127.0.0.1:8181 --model qwen --agents 4 --rounds 30 --window 2048` — creates N agents, round-robins one short infer each per round (N chosen so residency cap forces eviction every switch), prints nothing but progress; all measurement lives in the daemon journal.
  - `bloomery-bench report --journal <path>` — prints JSON: `{"warm": {"n": _, "p50_ms": _, "p95_ms": _}, "cold": {...}}`. Classification: a switch sample = the sum of `duration_ms` over the contiguous pager-op sequence serving one resume (`EvictSave` of the victim + `ResumeLoad` of the target, plus `ModelLoaded` if present); **warm** = `ResumeLoad` with `image_tier=="ram"` and no `ModelLoaded` in the sequence; **cold** = `image_tier=="nvme"` or a `ModelLoaded` present. p95 = value at index `ceil(0.95 * n) - 1` of the sorted sample (pin the formula in code comment — the gate is judged on it).
  - `pub fn compute_report(events: &[Event]) -> SwitchReport` (pure; the CLI wraps it).

- [ ] **Step 1: Failing report test (synthetic journal — the gate math is pinned by test).**

```rust
use bloomery_core::journal::{Event, PagerOpKind};
use bloomery_bench::report::compute_report;

fn evict(id: &str, ms: u64) -> Event {
    Event::PagerOp { id: id.into(), op: PagerOpKind::EvictSave, bytes: 1, duration_ms: ms, image_tier: "ram".into() }
}
fn resume(id: &str, ms: u64, tier: &str) -> Event {
    Event::PagerOp { id: id.into(), op: PagerOpKind::ResumeLoad, bytes: 1, duration_ms: ms, image_tier: tier.into() }
}

#[test]
fn classifies_warm_and_cold_and_computes_p95() {
    let mut events = Vec::new();
    for i in 0..20 {
        events.push(evict("v", 10));
        events.push(resume("t", 100 + i, "ram"));         // warm samples: 110..129 total ms
    }
    events.push(Event::ModelLoaded { model: "qwen".into(), duration_ms: 1200 });
    events.push(resume("t", 300, "nvme"));                 // one cold sample: 1500
    let r = compute_report(&events);
    assert_eq!(r.warm.n, 20);
    assert_eq!(r.warm.p95_ms, 129);                        // ceil(0.95*20)-1 = index 18 of 110..=129
    assert_eq!(r.cold.n, 1);
    assert_eq!(r.cold.p95_ms, 1500);
}

#[test]
fn empty_journal_reports_zero_n_not_zero_latency() {
    let r = compute_report(&[]);
    assert_eq!((r.warm.n, r.cold.n), (0, 0));              // n=0, no fake p95 (law 5)
    assert!(r.warm.p95_ms == 0 && r.warm.n == 0);          // consumer must check n
}
```

- [ ] **Step 2: Run → failure. Step 3: Implement** `report.rs` (pure) + `main.rs` (`switch` drives the native API with the std-TcpStream helper promoted into the bench crate; `report` reads via `replay`).
- [ ] **Step 4: Live G2 run on the box.**

Preconditions: qwen2.5-coder 7B Q8 GGUF path known; assay installed (`pip show assay` from `~/workspace/assay` checkout); nothing else on the GPU.

```bash
cargo build --release --features llama,vulkan -p bloomery-daemon
target/release/bloomery-daemon --config bench.toml   # models={qwen=...}, tier=enthusiast-16gb real
cargo run -p bloomery-bench -- switch --daemon http://127.0.0.1:8181 --model qwen --agents 4 --rounds 30 --window 2048
# cold class: POST /models/qwen/unload between rounds via a --cold flag on the bench
cargo run -p bloomery-bench -- report --journal <data_dir>/journal/boot-*.jsonl
```

Expected: ≥50 warm and ≥50 cold samples; p95 warm ≤ 2000 ms, p95 cold ≤ 5000 ms per `docs/gates.md`.

- [ ] **Step 5: Write the evidence doc and README; commit.**

`docs/superpowers/evidence/<date>-g2-agent-switch.md`: box (tier declared, GPU, driver), model + digest, daemon commit, bench invocation verbatim, the report JSON verbatim, verdict vs the pinned gate, **the page-cache caveat for cold switches stated**, and the journal file committed or referenced. If G2 **fails**: the evidence doc still ships with the numbers, and the plan stops for redesign per the gate — do not tune-and-rerun without a recorded protocol change.

`README.md`: what bloomery is (2 paragraphs from the spec), Phase 1 status (what works, what's honest-but-limited: buffered SSE, coarse lock, NVIDIA-only VRAM probe), quick start (config example, daemon, bench), links to spec/gates/evidence. Honest-status section mirrors the house style (robigo/assay READMEs).

```bash
git add crates/bloomery-bench docs/superpowers/evidence README.md
git commit -m "feat: G2 bench instrument + live evidence + README"
```

---

## Self-review (performed at plan-writing time)

- **Spec coverage:** Phase 1 scope from spec §5 — pager (Tasks 9, 12, 13), embedded assay POST (Task 16), journal (Task 7, exercised in 13), `/v1` shim (Task 15), G2 (Tasks 2, 17). Phase 0 (Task 1, 2). Laws 1–7 each land in a named task; law 8 is satisfied by the deterministic-only planner (no LLM policy in Phase 1); law 9 by Task 2. Capabilities/codec/policy plane are Phase 2 — correctly absent.
- **Placeholder scan:** no TBDs. Two deliberate deferred-decision points are *named and bounded*: Task 11's exact binding symbols (resolved by Task 1's D1 record, which is a hard prerequisite) and Task 15's streaming granularity (buffered SSE with the limit recorded in README).
- **Type consistency:** `Event`/`PagerOpKind` (Task 7) used by 13 and 17 match; `Placement` (9) consumed in 13; `Reply`/`VerifiedReply` (10) in 11/13/14; `ImageFetch` (12) in 13; `Profile` (8) in 12/16; `serve`/`test_support::serve_fake` (14) in 15.
