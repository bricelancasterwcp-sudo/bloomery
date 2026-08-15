# Partial offload + capability window Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close carried-debt item 7 (VRAM-bound windows placeable by construction), add per-model `n_gpu_layers` + declared `weights_vram_mib` so qwen3.8:27b can boot on the 16 GB card, then run the two G4 capability-window measurements (qwen3:14b, qwen3.8:27b).

**Architecture:** Spec `docs/superpowers/specs/2026-08-15-partial-offload-capability-window-design.md` (Brice-approved, incl. the 2026-08-15 §3b amendment). Geometry: `usable_window`'s VRAM term gains the ctx-overhead subtraction so the window law and placement charge the same four terms. Config: a `models` entry is a path string OR a `{path, n_gpu_layers, weights_vram_mib}` table (serde untagged). Pager: per-model overrides; one `effective_weights_bytes = min(declared, file)` used everywhere weights are charged.

**Tech Stack:** Existing Rust workspace; no new dependencies.

## Global Constraints

- The G4 protocol (`docs/superpowers/evidence/2026-08-15-g4-protocol.md`) is unamended; nothing in this slice touches instrument parameters, scoring, or decision rules.
- A declared number must never read as a measured one: the refusal arithmetic names `weights_vram_mib` as *declared* whenever the override is active.
- Fail-closed defaults: no override = full-weights charge = today's behavior; every existing config keeps parsing byte-for-byte.
- One value, both places: the effective weights charge feeds BOTH placement and the window law; the ctx-overhead term likewise appears in BOTH. No new asymmetry.
- `cargo fmt` + `cargo clippy --workspace --all-targets -- -D warnings` clean on BOTH feature sets (default and `--features llama`); `cargo test --workspace` green (GPU-free) before every commit. NEVER wrap builds/tests in the `timeout` command (uutils segfault). Conventional commits, no attribution footers. Files ≤800 lines.
- Mutation-test the load-bearing pins: the ctx-overhead subtraction, the both-places weights wiring, and the min-clamp each get a break-observe-restore check recorded in the task report.
- Standing rulings: static boot-time VRAM budget (never live reads); anti-ratchet; whole-task pager lock.

---

### Task 1: Item-7 geometry fix — the window law charges ctx overhead

**Files:**
- Modify: `crates/bloomery-core/src/geometry.rs` (`GeometryInput`, `usable_window`)
- Modify: `crates/bloomery-daemon/src/pager.rs` (`create_agent`'s `GeometryInput` literal, ~line 604)
- Modify: `crates/bloomery-daemon/src/config.rs` (`default_ctx_overhead_mib` doc comment: delete the "Asymmetry to know about" paragraph, replace with one line noting item 7 closed 2026-08-15)
- Modify: `docs/CARRIED-DEBT.md` (strike item 7, never delete, with a DELIVERED note citing the 14B attempt-1 `Refusal` evidence)
- Test: `crates/bloomery-core/tests/geometry_test.rs` (extend), `crates/bloomery-daemon/tests/pager_reservation_test.rs` (extend)

**Interfaces:**
- Consumes: nothing new.
- Produces: `GeometryInput` gains `pub ctx_overhead_bytes: u64` (all existing constructions in tests set it explicitly — no `Default`); the VRAM term becomes `free_vram − weights − overhead − ctx_overhead`, same saturating arithmetic. Task 3 relies on this field existing.

- [ ] **Step 1: Write the failing tests.**
  - `geometry_test.rs`: (a) with `free_vram = 1000`, `weights = 400`, `overhead = 100`, `ctx_overhead = 200`, `kv_per_token = 1`, `training_ctx` huge → window is 300 tokens, `BoundBy::Vram` (old code would say 500); (b) `ctx_overhead = 0` reproduces every pre-fix expectation (backward equivalence); (c) saturation: `ctx_overhead` larger than the remainder → 0-token window, no panic.
  - `pager_reservation_test.rs`: the **item-7 regression pin** — a FakeSubstrate model + budget shaped so the window comes out VRAM-bound; assert the agent both creates AND places (an `infer` succeeds). Choose numbers scaled from attempt 1's shape (weights + kv(window) + ctx_overhead exactly equal to the available budget; pre-fix this refuses with `needed − available == ctx_overhead_bytes`).
- [ ] **Step 2: Run to verify FAIL** (`cargo test -p bloomery-core --test geometry_test`; the daemon test fails on the refusal).
- [ ] **Step 3: Implement** — add the field, add `.saturating_sub(i.ctx_overhead_bytes)` to the `remaining` chain in `usable_window`, pass `self.ctx_overhead_bytes` in `create_agent`, fix every `GeometryInput` literal the compiler flags (tests: use the value the test's arithmetic needs, never a blanket 0 where the test exercises the term).
- [ ] **Step 4: Run the full workspace suite; fmt + clippy both feature sets.** Mutation check: remove the new `.saturating_sub(...)` → the geometry test (a) AND the regression pin must both fail; restore, note evidence.
- [ ] **Step 5: Update the two doc files** (config.rs comment, CARRIED-DEBT strike citing `~/.cache/bloomery-g4-14b`'s journaled `Refusal` — quote the needed/free line verbatim in the DELIVERED note).
- [ ] **Step 6: Commit**: `fix: window law charges ctx overhead — VRAM-bound windows placeable (closes carried-debt item 7)`

### Task 2: Config — per-model tuning entry (string | table)

**Files:**
- Modify: `crates/bloomery-daemon/src/config.rs`
- Modify: `crates/bloomery-daemon/src/main.rs` (the three `config.models` sites: iteration ~161, `len()` ~185, `keys()` ~203)
- Test: `crates/bloomery-daemon/tests/config_test.rs` (extend)

**Interfaces:**
- Produces (Task 3 and main.rs consume):

```rust
/// One `models` entry: either a bare path (today's shape) or a tuning table.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(untagged)]
pub enum ModelSpec {
    Path(PathBuf),
    Tuned {
        path: PathBuf,
        #[serde(default)]
        n_gpu_layers: Option<u32>,
        #[serde(default)]
        weights_vram_mib: Option<u64>,
    },
}

impl ModelSpec {
    pub fn path(&self) -> &Path;
    pub fn n_gpu_layers(&self) -> Option<u32>;      // Path variant → None
    pub fn weights_vram_mib(&self) -> Option<u64>;  // Path variant → None
}
```

- `Config.models` becomes `BTreeMap<String, ModelSpec>`.

- [ ] **Step 1: Write the failing tests** (config_test.rs, following its existing TOML-literal style): bare-string entry parses with `n_gpu_layers() == None`; table entry with both fields parses; table with only `path` parses (both options None); a config mixing both shapes parses; the spec §2 example block parses verbatim.
- [ ] **Step 2: Run to verify FAIL.**
- [ ] **Step 3: Implement** `ModelSpec` + accessors; update main.rs's three sites (`spec.path()` at the register loop; the other two unchanged in behavior). Doc comments cite spec §2.
- [ ] **Step 4: Full suite; fmt + clippy both feature sets.**
- [ ] **Step 5: Commit**: `feat: per-model config tuning — n_gpu_layers + weights_vram_mib (spec §2)`

### Task 3: Pager — per-model overrides, effective weights everywhere, plumbing

**Files:**
- Modify: `crates/bloomery-daemon/src/pager.rs` (model entry + `set_model_tuning`)
- Modify: `crates/bloomery-daemon/src/pager/paging.rs` (`loaded_weights_bytes` ~47-51, `place`'s demand side ~154-166, the `load_model` call's `n_gpu_layers` argument)
- Modify: `crates/bloomery-daemon/src/main.rs` (call `set_model_tuning` per configured model after `register_model`)
- Test: `crates/bloomery-daemon/tests/pager_weights_test.rs` (extend), plus the substrate-recording assertion wherever FakeSubstrate exposes load calls (`tests/common/` — read it first)

**Interfaces:**
- Consumes: Task 1's `GeometryInput.ctx_overhead_bytes` (already wired); Task 2's `ModelSpec` accessors.
- Produces:
  - `Pager::set_model_tuning(&mut self, model: &str, n_gpu_layers: Option<u32>, weights_vram_bytes: Option<u64>) -> Result<(), PagerError>` (`UnknownModel` on a bad name; `weights_vram_mib` is converted to bytes by the CALLER, main.rs, with `saturating_mul(1024*1024)` — the pager speaks bytes only).
  - Model-entry helper `effective_weights_bytes() = min(declared, meta.weights_bytes)` (declared absent → `meta.weights_bytes`), used in ALL FOUR weight-charge sites: `loaded_weights_bytes` (supply), `place`'s demand term, `create_agent`'s `GeometryInput.weights_bytes`, and the `/status` sum (which flows through `loaded_weights_bytes` automatically).
  - The per-model `n_gpu_layers` override wins over the pager-global default at the `load_model` call.
  - Refusal-string change in `place`: when the override is active, the weights term prints e.g. `weights 11811160064 B (declared weights_vram_mib; file 16810714464 B)` — the word `declared` is the binding requirement; keep the rest of the existing arithmetic format.

- [ ] **Step 1: Write the failing tests:**
  - Declared value smaller than file → `create_agent` window arithmetic AND placement both use the declared value (choose asymmetric numbers so one-sided wiring fails one assertion each — e.g. a window that only fits if geometry uses declared, and a second resident that only places if the budget charges declared).
  - Clamp: declared LARGER than file → file value used (no inflation).
  - No tuning call → byte-identical behavior to today (an existing-test re-run plus one explicit full-charge assertion).
  - Refusal string contains `declared` exactly when the override is active (drive a refusal with the override set; assert substring; drive one without; assert absent).
  - `set_model_tuning("nope", ...)` → `UnknownModel`.
  - FakeSubstrate records the per-model `n_gpu_layers` at load; absent override → the global default (`u32::MAX`) is recorded.
- [ ] **Step 2: Run to verify FAIL.**
- [ ] **Step 3: Implement.** Keep pager.rs under 800 lines — if the tuning setter + helper push it over, they live in a new `pager/tuning.rs` impl-block submodule (the `codec_gate.rs` pattern).
- [ ] **Step 4: Full suite; fmt + clippy both feature sets.** Mutation checks: (a) point ONE of the four charge sites back at `meta.weights_bytes` → the asymmetric both-places test fails; (b) drop the min-clamp → the clamp test fails. Restore, note evidence.
- [ ] **Step 5: Commit**: `feat: per-model n_gpu_layers + declared weights-VRAM charge, one value everywhere (spec §3-4)`

### Task 4: Operator docs

**Files:**
- Modify: `README.md` (config reference: the two new fields + the §5 derivation procedure; honest-limits: declared-not-measured weights charge — set it too low and the failure is an OOM, not a refusal; KV remains fully charged to VRAM under partial offload — conservative direction, recorded)
- Modify: `docs/CARRIED-DEBT.md` (append one new recorded item: KV full-charge under partial offload — the overcount direction and why it is safe)

**Interfaces:** consumes Tasks 1-3's shipped behavior; text must agree with the code (no over- or under-claiming).

- [ ] **Step 1: Write both doc updates** (README config example = the spec §2 block verbatim; derivation = spec §5's four numbered steps).
- [ ] **Step 2: `cargo test --workspace`** still green (docs only — run anyway, the P4 habit); fmt/clippy untouched-but-run.
- [ ] **Step 3: Commit**: `docs: partial-offload config reference, derivation procedure, honest limits`

### Task 5: LIVE — qwen3:14b through the gate (MAIN SESSION — GPU; not a subagent task)

**Files:**
- Create: `docs/superpowers/evidence/2026-08-15-g4-capability-14b.md` (+ committed journals beside it)

- [ ] **Step 1: Preflight** (rigorous-experiments §5 + box gotchas): free VRAM ≥ 12 GiB, no in-flight GPU runs, blob = `ollama show qwen3:14b --modelfile` FROM line (`sha256-a8cc1361…`), assay pinned at `74c5b71` via PYTHONPATH (the extract from the first G4 run), rebuild `cargo build --release --features llama,vulkan -p bloomery-daemon`.
- [ ] **Step 2: Run** — config as the attempt-1 file (`tasks_enabled = true`, `assay.enabled = true`, tier enthusiast-16gb real, fresh data_dir); wait for `CodecVerdict` or a `Degraded` abort; capture `/status`.
- [ ] **Step 3: Score check** — recompute the landing rate from `CodecFixture` events; must equal the verdict's `landed/n`; mismatch = instrument bug = STOP (infrastructure kill, rerunnable, no number consumed).
- [ ] **Step 4: Evidence doc** — subject/lens fully named (incl. raw-completion no-chat-template note and qwen3 thinking-as-is per spec §6); per-fixture table; verdict + Wilson + provisional; the decision applied; **attempt 1's item-7 refusal recorded as the motivating finding** (quote the `Refusal` line); caveats (boots-only, greedy, N=20 granularity). Commit journals + doc.
- [ ] **Step 5: Commit**: `docs: G4 capability window rung 1 — qwen3:14b evidence + journals`

### Task 6: LIVE — derive the 27B declared numbers (MAIN SESSION — GPU)

**Files:**
- Create: `docs/superpowers/evidence/2026-08-15-27b-offload-derivation.txt` (log excerpt, the ctx_overhead precedent)

- [ ] **Step 1:** Pick a starting `n_gpu_layers` from the GGUF's layer count (read it: `parse_gguf_meta` prints via the daemon boot log, or `llama.cpp`'s loader lines) aiming weights-in-VRAM ≈ 10-11 GiB.
- [ ] **Step 2:** One scratch boot (`allow_unprofiled = true`, `assay.enabled = false`, `tasks_enabled = false`, scratch data_dir) with the 27B entry in table form; record llama.cpp's buffer-size log lines + `nvidia-smi` delta; kill the boot.
- [ ] **Step 3:** Declare `weights_vram_mib` with headroom above the observed number; commit the excerpt with the chosen values and the arithmetic.

### Task 7: LIVE — qwen3.8:27b through the gate (MAIN SESSION — GPU)

**Files:**
- Create: `docs/superpowers/evidence/2026-08-15-g4-capability-27b.md` (+ committed journals)

- [ ] **Step 1: Preflight** as Task 5, plus: blob = the 15.65 GiB main GGUF (`sha256-f5f1dd89…`, NOT the 931 MB mmproj); config uses the Task 6 declared values.
- [ ] **Step 2: Cost projection before committing the hours** — time the POST + first two fixtures; extrapolate the full probe; if > 2 h, restart the daemon OS-detached (`setsid nohup … &` with a pid file) and watch via a harness-tracked waiter, per the long-run discipline.
- [ ] **Step 3-5:** Run, score-check, evidence doc (same shape as Task 5; state the partial-offload lens facts: `n_gpu_layers`, declared MiB, the KV-full-charge caveat) — commit.

---

## Self-Review (performed while writing)

- **Spec coverage:** §2 → Task 2; §3 → Task 3; §3b → Task 1; §4 → Task 3; §5 → Tasks 4/6; §6 → Tasks 5/7; §7 → Tasks 1-3 test steps; §8 non-goals respected (no auto-tuning, no live reads, no mmproj, no chat templates); §9 order = Tasks 1-7.
- **Placeholder scan:** clean; the two live-task configs reference the attempt-1 file and Task 6's derived values rather than inventing numbers.
- **Type consistency:** `ModelSpec` accessors (Task 2) are what Task 3's main.rs wiring calls; `set_model_tuning` bytes-only contract stated once and echoed in the caller step; `GeometryInput.ctx_overhead_bytes` named identically in Tasks 1 and 3.
