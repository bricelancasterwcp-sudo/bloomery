# bloomery Phase 2a — Hardening Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the Phase 1 carried-debt prerequisites — weights charged to the reservation budget, full-file model digest, journal schema additions (`AgentRemoved`, `TaskStep`), and the deterministic equal-priority LRU time-sharing tiebreak — then prove natural-pressure eviction live and record it as evidence.

**Architecture:** All changes land inside existing crates; no new crates, no new dependencies. The planner (`plan_residency`) semantics stay frozen; weights accounting and the time-sharing tiebreak live at the pager's request layer (`place()`), which is where the spec puts them. Time is injected as a millisecond closure so every tiebreak test runs without sleeping. The journal grows by addition only — a regression test replays the committed G2 evidence journal to pin backward compatibility forever.

**Tech Stack:** Existing workspace (Rust stable, edition 2021). Dependency allowlist unchanged: llama-cpp-2, self_cell, tiny_http, serde, serde_json, toml, sha2. No additions.

**Spec:** `docs/superpowers/specs/2026-08-14-phase2-os-surface-design.md` §2 (approved 2026-08-14). Umbrella laws §3 bind everything.

## Global Constraints

- **Frozen surfaces:** `plan_residency`'s semantics and signature (Task 9 tests pin them); every existing `Event` variant and field name (the G2 bench and committed journals read them); the pinned gates (`docs/gates.md`) — G2 is NOT re-read in this plan; the static-VRAM-budget closure convention (standing ruling: boot-time budget, never live reads).
- Journal changes are ADDITIONS ONLY; a replay of `docs/superpowers/evidence/2026-08-14-g2-warm-journal.jsonl` must succeed at every commit from Task 1 onward.
- TDD with RED/GREEN evidence in every task report; `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings` (default AND `--features llama` for clippy/check), `cargo test --workspace` green (GPU-free) before every commit.
- Files ≤800 lines; conventional commits; laws: refuse-with-arithmetic (never truncate/overcommit), unmeasured = None never zero, journal every mutating decision, deterministic mechanism only (no wall-clock reads inside decision logic — the injected clock is the only time source).
- Commands run from the repo root `~/workspace/bloomery`, branch `feat/phase2a-hardening` (create from master at start).

---

### Task 1: Journal additions + backward-compatibility pin

**Files:**
- Modify: `crates/bloomery-core/src/journal.rs` (add two `Event` variants)
- Modify: `crates/bloomery-daemon/src/pager.rs` (emit `AgentRemoved` in `remove_agent`)
- Modify: `crates/bloomery-daemon/src/api_v1.rs` (pass the ephemeral-cleanup reason)
- Test: `crates/bloomery-core/tests/journal_test.rs`, `crates/bloomery-daemon/tests/pager_remove_agent_test.rs`

**Interfaces:**
- Consumes: existing `Event` enum (internally tagged `#[serde(tag = "event")]`), `Journal`, `replay`; `Pager::remove_agent(&mut self, id: &str) -> Result<(), PagerError>`.
- Produces (later tasks and 2b rely on these exact shapes):

```rust
// New variants appended to Event (existing variants untouched):
AgentRemoved { id: AgentId, reason: String },
TaskStep { id: AgentId, step: u32, verb: String, outcome: String, duration_ms: u64 },
```

`Pager::remove_agent` gains a reason parameter: `pub fn remove_agent(&mut self, id: &str, reason: &str) -> Result<(), PagerError>` — call sites updated (api_v1 ephemeral cleanup passes `"ephemeral cleanup"`; its model-mismatch/other paths pass what they know). `TaskStep` is schema-only in 2a (doc comment: "emitted by the 2b task loop; defined now so the schema version is stable").

- [ ] **Step 1: Write the failing tests.**

Add to `crates/bloomery-core/tests/journal_test.rs`:

```rust
#[test]
fn agent_removed_and_task_step_round_trip() {
    let dir = std::env::temp_dir().join("bloomery-journal-2a");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("j2a.jsonl");
    let _ = std::fs::remove_file(&path);
    let mut j = Journal::open(&path).unwrap();
    let e1 = Event::AgentRemoved { id: "a1".into(), reason: "ephemeral cleanup".into() };
    let e2 = Event::TaskStep { id: "a1".into(), step: 3, verb: "patch".into(),
                               outcome: "applied".into(), duration_ms: 41 };
    j.append(&e1).unwrap();
    j.append(&e2).unwrap();
    assert_eq!(replay(&path).unwrap(), vec![e1, e2]);
}

#[test]
fn committed_g2_journal_still_replays() {
    // Backward-compatibility pin: schema changes must never orphan the
    // committed evidence. Path is relative to the workspace root.
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/superpowers/evidence/2026-08-14-g2-warm-journal.jsonl");
    let events = replay(&path).unwrap();
    assert!(events.len() > 100, "expected a real journal, got {} events", events.len());
}
```

Add to `crates/bloomery-daemon/tests/pager_remove_agent_test.rs` (follow the file's existing fixture pattern for building a `Pager<FakeSubstrate>` with a journal in a temp dir):

```rust
#[test]
fn remove_agent_journals_the_removal_with_its_reason() {
    // build pager + one agent per this file's existing fixture helpers
    // ... create agent, then:
    p.remove_agent(&id, "test teardown").unwrap();
    let events = bloomery_core::journal::replay(&journal_path).unwrap();
    assert!(events.iter().any(|e| matches!(e,
        bloomery_core::journal::Event::AgentRemoved { id: rid, reason }
            if rid == &id && reason == "test teardown")));
}
```

- [ ] **Step 2: Run to verify failure.** `cargo test --workspace` → compile errors (unknown variants / wrong arity on remove_agent).

- [ ] **Step 3: Implement.** Append the two variants to `Event` (bottom of the enum, existing variants byte-untouched). Change `remove_agent` to take `reason: &str` and journal `AgentRemoved` after the table entry is removed (successful path only). Update every call site: `api_v1.rs` ephemeral cleanup → `"ephemeral cleanup"`; `pager_remove_agent_test.rs` existing tests → any literal; grep for other call sites and update with honest literals.

- [ ] **Step 4: Green + gates.** `cargo test --workspace`, `cargo fmt --check`, clippy default + `--features llama` clean.

- [ ] **Step 5: Commit.**

```bash
git add crates/
git commit -m "feat: journal - AgentRemoved + TaskStep events, committed-journal compat pin"
```

---

### Task 2: Full-file model digest

**Files:**
- Modify: `crates/bloomery-daemon/src/agents.rs` (`model_digest`)
- Test: `crates/bloomery-daemon/tests/agents_test.rs`

**Interfaces:**
- Consumes/Produces: `pub fn model_digest(gguf: &Path) -> std::io::Result<String>` — signature unchanged; the digest becomes a full-file streamed sha256 (hex). All existing digest-tagged image mechanics keep working (the string just gets stronger).

- [ ] **Step 1: Write the failing test.**

```rust
#[test]
fn digest_covers_the_whole_file_not_just_the_prefix() {
    let dir = std::env::temp_dir().join("bloomery-digest-2a");
    std::fs::create_dir_all(&dir).unwrap();
    let (p1, p2) = (dir.join("m1"), dir.join("m2"));
    // Identical 2 MiB prefix, identical length, differing only in the last byte —
    // the Phase 1 digest (first 1 MiB + length) collides on these by construction.
    let mut a = vec![0xAAu8; 2 * 1024 * 1024 + 1];
    let mut b = a.clone();
    *b.last_mut().unwrap() = 0xBB;
    std::fs::write(&p1, &a).unwrap();
    std::fs::write(&p2, &b).unwrap();
    assert_ne!(model_digest(&p1).unwrap(), model_digest(&p2).unwrap());
    a.truncate(0); // silence unused-mut lint paranoia if any
}
```

- [ ] **Step 2: RED run.** The new test FAILS against the prefix digest (equal digests). Capture the failure output.

- [ ] **Step 3: Implement.** Stream the whole file through sha256 in 1 MiB chunks (`std::io::Read` loop into a fixed buffer; no whole-file allocation — blobs are 8 GB). Keep the hex encoding. Update `model_digest`'s doc comment: full-file; boot-time cost is seconds per model and is the pinned precondition for restart-survivable images (spec 2a item 2). The existing `digest_changes_with_content` test still passes.

- [ ] **Step 4: Green + gates.** Workspace suite, fmt, clippy (both feature configs).

- [ ] **Step 5: Commit.** `git commit -m "feat: agents - full-file streamed model digest"`

---

### Task 3: Weights enter the reservation budget

**Files:**
- Modify: `crates/bloomery-daemon/src/pager.rs` (track loaded-model weights)
- Modify: `crates/bloomery-daemon/src/pager/paging.rs` (`place()` arithmetic; on-demand load admission)
- Modify: `crates/bloomery-daemon/src/pager/status.rs` (expose `loaded_weights_bytes`)
- Test: `crates/bloomery-daemon/tests/pager_weights_test.rs` (new)

**Interfaces:**
- Consumes: `ModelEntry` (holds `GgufMeta` with `weights_bytes`, and the loaded `ModelHandle` option), `place()`'s current arithmetic `free.saturating_sub(resident_kv)`, `Placement`, journal events.
- Produces:
  - Placement arithmetic becomes: `avail = budget − Σ loaded_models.weights_bytes − Σ resident kv_bytes` (all `saturating_sub`). When the request's model is NOT yet loaded, its `weights_bytes` is additionally subtracted before planning the KV placement — loading is part of satisfying this request.
  - If the weights themselves cannot fit even with every evictable KV context reclaimed: `PagerError::Refused { needed, free, reclaimable }` where `needed` includes the weights term, and the journaled `Refusal.detail` names the weights arithmetic explicitly (law 2: the arithmetic, printed). **No automatic model unloading** — `unload_model` stays a manual/API operation in 2a (spec: weights eviction is not in scope).
  - `StatusReport` gains `pub loaded_weights_bytes: u64` (sum; 0 when none loaded).
  - `unload_model` already journals `ModelUnloaded`; after it, the weights are credited back (the sum recomputes from the loaded set — derive, don't store a counter).

- [ ] **Step 1: Write the failing tests** (new file `pager_weights_test.rs`; use the existing pager test fixture pattern — FakeSubstrate, temp journal/images, constant budget closure; qwen-like meta 28×4×128, `weights_bytes` set per test):

```rust
// Geometry used throughout: kv_per_token = 57_344; window_cap 1024 → kv_bytes
// per context = 58_720_256 (~56 MiB).

#[test]
fn loading_a_model_charges_its_weights_against_the_budget() {
    // budget 300 MiB; weights 200 MiB; context 56 MiB.
    // Agent 1 (priority 50) infers: model loads (200) + ctx (56) = 256 ≤ 300 → OK.
    // Agent 2 (same model, priority 100 — strictly higher, so the frozen
    // planner may evict a1) infers: 200 + 2×56 = 312 > 300
    // → planner must evict agent 1's context (not refuse):
    //   avail = 300 − 200 − 56 = 44 < 56, reclaimable 56 → Evict.
    // assert: infer(a2) succeeds AND journal has PagerOp{EvictSave, id: a1}.
}

#[test]
fn a_second_models_weights_that_cannot_fit_are_refused_with_the_arithmetic() {
    // budget 300 MiB; model A weights 200 MiB (loaded via a1's infer);
    // model B weights 250 MiB. a2 on model B infers:
    // avail even after evicting ALL kv = 300 − 200 = 100 < 250 + 56
    // → PagerError::Refused; journaled Refusal.detail contains "weights".
    // assert the substrate was never asked to load model B (call log).
}

#[test]
fn unload_credits_the_weights_back() {
    // Continues the scenario above: unload_model("modelA") → 204-path Ok;
    // now a2 on model B infers successfully (300 − 250 − 56 ≥ 0 fits).
    // assert ModelUnloaded journaled before the successful load of B.
}

#[test]
fn status_reports_loaded_weights() {
    // Before any infer: status().loaded_weights_bytes == 0.
    // After a1's infer on model A: == 200 MiB exactly.
}
```

Write these as real tests against the fixture (the comments above are the binding scenarios; express each assert concretely with the fixture's helpers).

- [ ] **Step 2: RED run.** First test fails today: without weights accounting, `avail = 300 − 56 = 244 ≥ 56` → no eviction happens (assert on EvictSave fails). Capture it.

- [ ] **Step 3: Implement.** In `place()`: compute `loaded_weights` by summing `meta.weights_bytes` over models whose handle is present; if the request's model is unloaded, add its weights to the demand side before the existing KV planning; refusal construction includes the weights term in `needed` and a detail string of the full arithmetic (`"weights <w> + kv <k> vs budget <b> − loaded <lw> − resident <rk>"`-shaped). Keep every subtraction saturating. `status.rs` gains the field. Doc-comment the accounting rule where it lives, citing the standing static-budget ruling.

- [ ] **Step 4: Green + gates.** Full suite (all Phase 1 pager tests must still pass — their fixtures either use one model with small weights or `weights_bytes` ≈ 0/1000; verify none of them accidentally relied on weights being free, and fix FIXTURES ONLY if a fixture's budget arithmetic needs a bump — never production code to make an old test pass; explain any fixture change in the report).

- [ ] **Step 5: Commit.** `git commit -m "feat: pager - weights charged to the reservation budget"`

---

### Task 4: Equal-priority LRU time-sharing tiebreak

**Files:**
- Modify: `crates/bloomery-daemon/src/pager.rs` (injected clock, last-use + waiting trackers, config quantum)
- Modify: `crates/bloomery-daemon/src/pager/paging.rs` (tiebreak in the Refuse branch of `place()`)
- Modify: `crates/bloomery-daemon/src/config.rs` (`time_share_quantum_secs`, default 30)
- Modify: `crates/bloomery-daemon/src/main.rs` (wire the config value)
- Test: `crates/bloomery-daemon/tests/pager_timeshare_test.rs` (new)

**Interfaces:**
- Consumes: `Placement::Refuse`, existing `evict()` path, `SchedulerDecision { id, decision, evicted }` (decision is a free-form String — no schema change).
- Produces:

```rust
pub type ClockFn = Box<dyn Fn() -> u64 + Send + Sync>;   // monotonic milliseconds
impl<S: Substrate> Pager<S> {
    pub fn set_clock(&mut self, clock: ClockFn);          // default: Instant-based since Pager::new
    pub fn set_time_share_quantum_ms(&mut self, ms: u64); // default 30_000
}
```

Semantics (spec §2 item 4, binding): when `plan_residency` returns `Refuse` AND every resident is idle AND every resident's priority == the request's priority (mixed or higher priorities present → plain refusal, no tiebreak), the pager records `waiting_since[agent] = now()` on the FIRST such refusal and refuses normally. On a later attempt where `now() − waiting_since ≥ quantum`, it evicts the least-recently-used equal-priority idle resident (last-use = the clock value recorded at that agent's most recent successful infer completion; ties on last-use broken by lexical id for determinism), journals `SchedulerDecision { id, decision: "evict_timeshare(waited_<N>ms)", evicted: [victim] }`, and proceeds with the normal eviction machinery (same `EvictSave` path). `waiting_since` is cleared on successful placement and on `remove_agent`. Determinism: identical clock sequences produce identical decisions; production code never calls `Instant::now()` outside the default clock closure.

- [ ] **Step 1: Write the failing tests** (new file; fixture: FakeSubstrate, budget sized so exactly ONE context fits after weights — e.g. weights 200 MiB, budget 270 MiB, window_cap 1024 → one 56 MiB context fits, two don't; both agents priority 100; controllable clock via `Arc<AtomicU64>`):

```rust
fn test_clock(t: std::sync::Arc<std::sync::atomic::AtomicU64>) -> bloomery_daemon::pager::ClockFn {
    Box::new(move || t.load(std::sync::atomic::Ordering::SeqCst))
}

#[test]
fn within_the_quantum_equal_priority_stays_refused() {
    // a1 infers (resident). clock = 0. a2 infers → Refused. clock = 29_999.
    // a2 infers again → STILL Refused. No EvictSave in the journal.
}

#[test]
fn after_the_quantum_the_lru_equal_priority_resident_is_evicted() {
    // a1 infers (resident, last-use 0). clock = 0: a2 infers → Refused (waiting starts).
    // clock = 30_000: a2 infers → succeeds; journal has
    // SchedulerDecision{ decision starting with "evict_timeshare(", evicted: [a1] }
    // followed by PagerOp{EvictSave, id: a1}.
}

#[test]
fn lru_picks_the_least_recently_used_among_equals() {
    // Budget fits TWO contexts (bump budget to 330 MiB). a1 infers at t=0,
    // a2 infers at t=10_000 (both resident). a3 (same priority) refused at
    // t=20_000; at t=50_001 a3 infers → victim must be a1 (older last-use),
    // never a2.
}

#[test]
fn mixed_priorities_never_time_share() {
    // Resident a1 has priority 150 (higher). a2 at 100 refused at t=0;
    // at t=60_000 a2 infers → STILL Refused (spec: tiebreak only when ALL
    // residents are equal priority). No evict_timeshare in the journal.
}

#[test]
fn successful_placement_clears_the_waiting_tracker() {
    // a2 refused at t=0; a1 is removed (remove_agent) freeing the slot;
    // a2 infers at t=5_000 → succeeds via normal placement (no
    // evict_timeshare decision in the journal); a later refusal cycle
    // starts the wait fresh (assert by requiring a full quantum again).
}
```

Express concretely against the fixture; the scenario comments are binding.

- [ ] **Step 2: RED run.** Tests 2/3 fail (refusal instead of timeshare eviction). Capture.

- [ ] **Step 3: Implement** per the semantics block. Keep the tiebreak logic in a named helper (`try_time_share(...)`) in `paging.rs` with a doc comment stating the spec rule and the determinism argument. Wire `config.time_share_quantum_secs` (serde default 30) through `main.rs` (`set_time_share_quantum_ms(secs * 1000)`).

- [ ] **Step 4: Green + gates** (full suite — the Phase 1 eviction tests use mixed priorities and stay untouched; both clippy configs).

- [ ] **Step 5: Commit.** `git commit -m "feat: pager - deterministic equal-priority LRU time-sharing tiebreak"`

---

### Task 5: Natural-pressure live evidence + bench preflight + docs

**Files:**
- Modify: `crates/bloomery-bench/src/switch.rs` (preflight: replace the refuse-if-measured-budget rule)
- Create: `docs/superpowers/evidence/2026-08-14-2a-natural-pressure.md`
- Modify: `README.md` (honest-limits list), `docs/CARRIED-DEBT.md` (mark items delivered)
- Test: `crates/bloomery-bench/tests/report_test.rs` (unchanged — confirm green), driver check below

**Interfaces:**
- Consumes: the bench `switch` driver, `/status` (now carrying `loaded_weights_bytes`), the daemon under `--features llama,vulkan`.
- Produces: an evidence doc — explicitly **not a gate re-read**; G2 stands as published.

- [ ] **Step 1: Update the bench preflight.** Phase 1's preflight refused a warm run against a measured-budget daemon (pre-2a, no evictions could occur → n=0). That rule is now wrong. Replace it: run the workload, and at `report` time (or driver exit) FAIL LOUDLY if the journal produced fewer switch samples than `agents − 1` per round expectation — print the observed n, the budget, and `loaded_weights_bytes` from `/status` so the operator sees the pressure arithmetic (never a silent n=0 success). Add/adjust a unit test on the failure message path if the driver has one; otherwise the live run is the check and the report says so.

- [ ] **Step 2: Live run (background, no `timeout` wrapper — the box's timeout binary segfaults on multithreaded children).** Build `--features llama,vulkan`. Config: qwen blob (via `ollama show qwen2.5-coder:7b-instruct-q8_0 --modelfile` FROM line), tier enthusiast-16gb real-hardware, assay disabled, allow_unprofiled, NO PATH stripping — nvidia-smi present, natural budget. Pressure arithmetic (record it): boot free ≈ 14 GiB; weights ≈ 8.1 GiB; remainder ≈ 6 GiB; `--window 16384` → context = 57_344 × 16384 ≈ 0.875 GiB → 6 contexts fit; `--agents 8 --rounds 8` forces evictions every lap. Verify from the journal that evictions occurred under the MEASURED budget (grep the journal for EvictSave + confirm no "vram unmeasured" Degraded line). Run the warm class only (cold is unchanged by 2a).

- [ ] **Step 3: Write the evidence doc.** Contents: purpose (2a acceptance: natural-pressure eviction under weights-in-reservation; NOT a gate re-read — G2 stands), the pressure arithmetic with live numbers, invocations verbatim, `bloomery-bench report` JSON, a qualitative comparison to the G2 warm numbers (window is 8× larger here — context create/destroy of ~0.9 GiB KV allocations may shift latencies; report what was measured, no gate language), journal committed alongside, and the confirmation lines (no unmeasured-VRAM Degraded; `loaded_weights_bytes` from `/status` during the run).

- [ ] **Step 4: Docs.** README: remove/replace the two honest-limit lines this plan closes (weights-not-in-reservation; prefix digest) and add the time-sharing rule one-liner; update the Reproducing section if the preflight change altered flags. CARRIED-DEBT: move items 1 (weights) and 2 (digest) plus the AgentRemoved bullet and item 4 (time-sharing) from "Phase 2 work items" to a dated "Delivered in 2a" strike-list (keep the text, mark delivered — the record stays).

- [ ] **Step 5: Gates + commit.**

```bash
cargo test --workspace && cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings
git add crates/bloomery-bench docs/superpowers/evidence README.md docs/CARRIED-DEBT.md
git commit -m "feat: 2a natural-pressure evidence, bench preflight for measured budgets, docs"
```

---

## Self-review (performed at plan-writing time)

- **Spec coverage (spec §2 items 1–4 + acceptance):** item 1 → Task 3; item 2 → Task 2; item 3 → Task 1 (AgentRemoved emitted, TaskStep schema-only as the spec's 2b note allows); item 4 → Task 4 (request-layer, planner frozen, quantum configurable, journal-recorded); acceptance (suite + live warm-class natural-pressure evidence, not a gate re-read) → Task 5.
- **Placeholder scan:** the scenario-comment tests in Tasks 3/4 are deliberate: the scenarios and assertions are stated as binding, with fixture mechanics delegated to the existing test-file patterns the implementer can see — no TBDs, no "add tests" without content.
- **Type consistency:** `remove_agent(&mut self, id: &str, reason: &str)` (Task 1) is the signature Task 4's tracker-clearing references; `ClockFn`/`set_clock`/`set_time_share_quantum_ms` (Task 4) match; `StatusReport.loaded_weights_bytes` (Task 3) is what Task 5's preflight reads; `Event::AgentRemoved`/`TaskStep` field names match between Task 1's tests and interfaces.
