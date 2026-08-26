# Memory Organ (Slice 1) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the memory organ's mechanism end to end — verified tasks mint episodes, exact repeats get grant-gated injection, strangers get silence, everything is journal-stamped — behind config, default off.

**Architecture:** A new `memory` module in `bloomery-daemon` (sibling to `drift`/`swap`): a JSONL event-sourced store, a two-stage exact retriever (goal hash → pre-first-touch file fingerprints → grant gate), a deterministic prompt block injected by `render_prompt_from`, and a worker pipeline in `TaskRegistry::spawn_task` (retrieve → stamp → run → mint/contradict). The capture seam extends `Observation` with pre-first-touch fingerprints filled by `exec_read`/`exec_patch`.

**Tech Stack:** Rust (workspace crates `bloomery-daemon`/`bloomery-core`), serde/serde_json, sha2 via `bloomery_core::journal::sha256_hex_bytes`. **No new dependencies.**

**Spec:** `docs/superpowers/specs/2026-08-26-memory-organ-design.md` — read it first; every task below argues from it.

## Global Constraints

- Branch `memory-organ` in worktree `.worktrees/memory-organ` (create via superpowers:using-git-worktrees at execution start).
- No new crate dependencies; no SQLite (spec §6).
- The organ is advisory: it never gates admission, never touches `done_trust`, never executes anything, and any failure of the organ must produce memory-off behavior, never a failed task (spec §1, §7).
- Memory-off/silent prompt rendering must stay **byte-identical** to today's renderer — the existing goldens in `crates/bloomery-daemon/tests/task_render_test.rs` must stay green untouched (spec §4).
- Frozen instruments (G4/G5, drift, swap-cover) run memory-off: nothing in this plan wires memory into `codec_probe`, `post`, `drift`, or `swap` (spec §4).
- Box build traps (README + memory): plain `cargo test` for suites; NEVER the `timeout` wrapper; a featured daemon build is `cargo build -p bloomery-daemon --features vulkan` and must come LAST (a later `cargo test` overwrites the featured binary featureless) — only Task 10 boots the daemon.
- Commit format `<type>: <description>`, no attribution trailers (repo convention).
- Run `cargo fmt` and `cargo clippy --workspace -- -D warnings` before every commit.

---

### Task 1: Episode records (`memory/record.rs`)

**Files:**
- Create: `crates/bloomery-daemon/src/memory.rs` (module root: `pub mod record;` only, for now)
- Create: `crates/bloomery-daemon/src/memory/record.rs`
- Modify: `crates/bloomery-daemon/src/lib.rs` (add `pub mod memory;` alongside the existing `pub mod drift;` line)
- Test: inline `#[cfg(test)]` in `record.rs`

**Interfaces:**
- Consumes: `bloomery_core::journal::sha256_hex` (string sha256, `crates/bloomery-core/src/journal.rs:448`).
- Produces (later tasks use these exact names):
  - `pub fn normalize_goal(goal: &str) -> String` — trim, collapse every internal whitespace run to one space.
  - `pub fn goal_hash(goal: &str) -> String` — `sha256_hex(&normalize_goal(goal))`.
  - `pub enum Fingerprint { Sha256(String), Absent }` (derive `Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize`).
  - `pub struct CitedFile { pub path: String, pub fingerprint: Fingerprint }` — `path` is canonical-absolute (same derives).
  - `pub struct StoredPatch { pub path: String, pub codec: String, pub body: String }` — `codec` is `"search_replace"` or `"whole_file"`; `body` is the conflict-marker wire form for search/replace, the raw contents for whole-file (same derives).
  - `pub struct RunEvidence { pub argv: Vec<String>, pub outcome: String }` (same derives).
  - `pub struct EpisodeRecord { pub episode_id: String, pub goal_hash: String, pub goal_text: String, pub cited_files: Vec<CitedFile>, pub landed_patches: Vec<StoredPatch>, pub run_evidence: RunEvidence, pub trajectory: Vec<String>, pub minted_by_model: String, pub minted_by_envelope: String, pub status: String, pub contradicted_by: Option<String>, pub minted_at: u64 }` (same derives). `status` is `"verified"` or `"contradicted"` (spec §2).
  - `pub fn episode_id(goal_hash: &str, cited: &[CitedFile]) -> String` — the **task identity** (spec §2): sha256 over `goal_hash` plus each cited file's `path` and fingerprint, with `cited` sorted by `path` first so caller order can never change the id. Landed patches are deliberately excluded.
  - `pub enum StoredRow { Episode(EpisodeRecord), Tombstone { episode_id: String } }` with `#[serde(tag = "row", rename_all = "snake_case")]` (same derives).

- [ ] **Step 1: Write the failing tests** (inline `#[cfg(test)] mod tests`):

```rust
#[test]
fn normalize_goal_trims_and_collapses_whitespace() {
    assert_eq!(normalize_goal("  fix\t the\n\n bug "), "fix the bug");
    assert_eq!(goal_hash("fix the bug"), goal_hash(" fix\tthe  bug "));
    assert_ne!(goal_hash("fix the bug"), goal_hash("fix the bugs"));
}

#[test]
fn episode_id_is_the_task_identity_and_ignores_patches_and_order() {
    let a = CitedFile { path: "/w/a.py".into(), fingerprint: Fingerprint::Sha256("aa".into()) };
    let b = CitedFile { path: "/w/b.py".into(), fingerprint: Fingerprint::Absent };
    let id1 = episode_id("gh", &[a.clone(), b.clone()]);
    let id2 = episode_id("gh", &[b.clone(), a.clone()]);
    assert_eq!(id1, id2, "citation order must not change the identity");
    let c = CitedFile { path: "/w/a.py".into(), fingerprint: Fingerprint::Sha256("bb".into()) };
    assert_ne!(id1, episode_id("gh", &[c, b]), "a fingerprint change is a different task");
    assert_ne!(id1, episode_id("gh2", &[a, ]), "a goal change is a different task");
}

#[test]
fn stored_row_round_trips_and_is_tagged() {
    let row = StoredRow::Tombstone { episode_id: "e1".into() };
    let line = serde_json::to_string(&row).unwrap();
    assert!(line.contains("\"row\":\"tombstone\""), "{line}");
    let back: StoredRow = serde_json::from_str(&line).unwrap();
    assert_eq!(back, row);
}
```

Also write `episode_round_trips_through_json` constructing a full `EpisodeRecord` (every field populated), serializing via `StoredRow::Episode`, deserializing, and asserting equality — the payload-is-the-record rule (spec §6): no field may be lost between a write and the next read.

- [ ] **Step 2: Run to verify failure:** `cargo test -p bloomery-daemon memory::record` — expected: compile error (module does not exist).
- [ ] **Step 3: Implement** `record.rs` exactly to the Produces list. `normalize_goal` = `goal.split_whitespace().collect::<Vec<_>>().join(" ")`. `episode_id` = build a `String` of `goal_hash` then each sorted cited file as `\n{path}\n{"sha256:"+hex | "absent"}`, then `sha256_hex(&that)`.
- [ ] **Step 4: Run to verify pass:** `cargo test -p bloomery-daemon memory::record` — expected: all PASS.
- [ ] **Step 5: Commit:** `git add -A && git commit -m "feat: memory organ episode records — task identity, fingerprints, stored rows"`

---

### Task 2: The JSONL store (`memory/store.rs`)

**Files:**
- Create: `crates/bloomery-daemon/src/memory/store.rs`
- Modify: `crates/bloomery-daemon/src/memory.rs` (add `pub mod store;`)
- Test: Create `crates/bloomery-daemon/tests/memory_store_test.rs`

**Interfaces:**
- Consumes: Task 1's `EpisodeRecord`, `StoredRow`.
- Produces:
  - `pub struct MemoryStore` — fields private: `path: PathBuf`, `episodes: BTreeMap<String, EpisodeRecord>` (by `episode_id`, last-writer-wins), `parse_errors: u64`.
  - `pub fn load(path: &Path) -> std::io::Result<MemoryStore>` — a missing file is an EMPTY store (first boot), not an error; a corrupt line increments `parse_errors` and is skipped (spec §6 — deliberately unlike `journal::replay`'s law-7 hard error: the journal is evidence, this store is advisory memory whose reader must survive its own rot). Parent dir is created if missing.
  - `pub fn mint(&mut self, rec: EpisodeRecord, max_episodes: usize) -> std::io::Result<()>` — appends `StoredRow::Episode`, upserts the map, then retention: while distinct ids exceed `max_episodes`, evict contradicted-oldest-first then verified-oldest-first (oldest by `minted_at`; ties by `episode_id` for determinism) by appending a `Tombstone` row and removing from the map (spec §6).
  - `pub fn mark_contradicted(&mut self, episode_id: &str, task_id: &str) -> std::io::Result<bool>` — clones the record, sets `status: "contradicted"`, `contradicted_by: Some(task_id)`, appends it as a full `Episode` row, updates the map; `false` if unknown id.
  - `pub fn delete(&mut self, episode_id: &str) -> std::io::Result<bool>` — tombstone + remove; `false` if unknown.
  - `pub fn candidates(&self, goal_hash: &str) -> Vec<&EpisodeRecord>` — all episodes with that `goal_hash` (linear scan of the map is fine at `max_episodes` scale).
  - `pub fn episodes(&self) -> impl Iterator<Item = &EpisodeRecord>` and `pub fn counts(&self) -> MemoryCounts` where `pub struct MemoryCounts { pub episodes: u64, pub verified: u64, pub contradicted: u64, pub parse_errors: u64 }` (derive `Debug, Clone, Copy, PartialEq, serde::Serialize`).
  - Append style mirrors `Journal::append` (`journal.rs:409`): `OpenOptions::new().create(true).append(true)`, one JSON line, flush after every append.

- [ ] **Step 1: Write the failing tests** in `tests/memory_store_test.rs` (use a `fresh_dir` helper copied from `registry.rs:271`'s pattern — `AtomicU64` + pid, no clocks):

```rust
// Helper used by every test: a minimal verified episode.
fn ep(id_seed: &str, goal_hash: &str, minted_at: u64) -> EpisodeRecord { /* fill every field;
    episode_id = id_seed.into(), status = "verified".into(), contradicted_by = None */ }

#[test]
fn load_of_missing_file_is_an_empty_store() { /* counts all zero, parse_errors 0 */ }

#[test]
fn mint_then_reload_round_trips_last_writer_wins() {
    // mint e1(minted_at 1), then a refreshed e1(minted_at 2) — reload sees ONE episode, minted_at 2.
}

#[test]
fn contradiction_survives_reload_and_delete_tombstones() {
    // mint e1, mark_contradicted(e1, "task-9") -> reload: status "contradicted", contradicted_by "task-9".
    // delete(e1) -> reload: empty. delete unknown -> Ok(false).
}

#[test]
fn corrupt_lines_are_counted_never_fatal() {
    // Write a store file: one valid Episode row, then the line "{not json", then a valid row.
    // load: 2 episodes, parse_errors == 1.
}

#[test]
fn retention_evicts_contradicted_oldest_first_then_verified_oldest() {
    // max_episodes = 2. Mint v1(t=10), c1(t=5, then mark contradicted), v2(t=20), then mint v3(t=30).
    // After each mint the store holds <= 2: c1 evicted before v1; then v1 (oldest verified) before v2.
    // Reload and assert the same survivors — eviction is durable (tombstones), not in-memory only.
}
```

- [ ] **Step 2: Run to verify failure:** `cargo test -p bloomery-daemon --test memory_store_test` — expected: compile error.
- [ ] **Step 3: Implement** `store.rs` per Produces. Load: `BufReader::lines`, `serde_json::from_str::<StoredRow>` per line, `Err` → `parse_errors += 1; continue`.
- [ ] **Step 4: Run to verify pass:** same command, all PASS.
- [ ] **Step 5: Commit:** `git commit -am "feat: memory store — event-sourced JSONL, tombstones, retention, corrupt-line counting"`

---

### Task 3: The capture seam — pre-first-touch fingerprints and landed patches

**Files:**
- Modify: `crates/bloomery-daemon/src/task/mod.rs` (`Observation` at :40 gains a field; new `PreTouch`/`Touched` types)
- Modify: `crates/bloomery-daemon/src/task/exec.rs` (`exec_read` :152, `exec_patch` :318)
- Modify: `crates/bloomery-daemon/src/task/task_loop.rs` (`TaskState` :153, `TaskResult` :124, `run_task` :536)
- Test: Create `crates/bloomery-daemon/tests/memory_capture_test.rs`

**Interfaces:**
- Produces (in `task/mod.rs`):
  - `pub enum PreTouch { Sha256(String), Absent, Uncomputable }` (derive `Debug, Clone, PartialEq, serde::Serialize`) — `Uncomputable` = the file was touched but its full pre-touch bytes could not be honestly hashed (a truncated read); Task 5's mint bar refuses to mint over it.
  - `pub struct Touched { pub canonical: std::path::PathBuf, pub pre: PreTouch }` (same derives).
  - `Observation` gains `pub touched: Option<Touched>` — `Some` only on a SUCCESSFUL `read`/`patch`; the `failed()` helper (`exec.rs:112`) and every other construction site sets `None` (the compiler lists them; fix mechanically).
- `exec_read`: on success, `touched: Some(Touched { canonical: canon.clone(), pre: if truncated { PreTouch::Uncomputable } else { PreTouch::Sha256(sha256_hex_bytes(&bytes)) } })` — `bytes` is the full file exactly when not truncated (`exec.rs:164-179`). Use `bloomery_core::journal::sha256_hex_bytes`.
- `exec_patch`: the pre-patch read (`exec.rs:325-342`) already distinguishes `NotFound` (→ `current = ""`) from real bytes; carry that distinction into a local `pre: PreTouch` — `Absent` on the `NotFound` arm, else `Sha256(sha256_hex_bytes(current.as_bytes()))` — and set `touched: Some(Touched { canonical, pre })` on the `Landing::Lands` success arm only. (An over-cap pre-read already fails the patch at :327, so a successful patch is never `Uncomputable`.)
- `run_task`: `TaskState` gains `touched: std::collections::BTreeMap<String, PreTouch>` and `landed: Vec<(String, bloomery_core::action::PatchBody)>`. After the `execute_action` dispatch (:622): if `!obs.failed`, `if let Some(t) = &obs.touched { state.touched.entry(t.canonical.display().to_string()).or_insert(t.pre.clone()); }` — `or_insert` IS the first-touch rule (spec §2: a later patch of an already-read file must not overwrite the read-time fingerprint). Then, patch bodies: `if verb == "patch" && !obs.failed { if let (Action::Patch { body, .. }, Some(t)) = (&action, &obs.touched) { state.landed.push((t.canonical.display().to_string(), body.clone())); } }`.
- `TaskResult` gains `pub touched_files: std::collections::BTreeMap<String, PreTouch>` and `pub landed_patches: Vec<(String, bloomery_core::action::PatchBody)>` — populated from `TaskState` at EVERY return site of `run_task` (there are five; the two error-arm builders in `registry.rs` and the `Running` placeholder use empty collections). `PatchBody` already derives `Clone + Serialize` (`action/mod.rs:48`). **`get_task`'s response shape (`api_task.rs`, keys `status`/`steps`/`summary`) is deliberately untouched.**

- [ ] **Step 1: Write the failing tests** in `tests/memory_capture_test.rs`. Drive the REAL `run_task` against a scripted `FakeSubstrate` exactly like `registry.rs`'s test helpers (copy `fresh_dir`/`meta`/`build_pager`/`ok_grant` from `registry.rs:271-318`; scripted turns are `Reply` structs whose `text` is an `<action ...>` envelope — see `registry.rs:330` for the `done` shape; a read turn is `"<action verb=\"read\" path=\"a.py\"></action>"`-style — copy the exact envelope grammar from `tests/task_loop_test.rs`'s scripted turns rather than guessing):

```rust
#[test]
fn read_then_patch_fingerprints_at_first_touch() {
    // Workspace: a.py with bytes B0. Script: read a.py -> patch a.py (whole-file) -> done.
    // Assert result.touched_files["<canon a.py>"] == PreTouch::Sha256(sha256_hex_bytes(B0)) —
    // the READ-time hash, not the post-patch bytes (or_insert pinned).
    // Assert result.landed_patches == [(canon, PatchBody::WholeFile { contents })].
}

#[test]
fn patch_created_file_fingerprints_absent_and_failed_steps_capture_nothing() {
    // Script: patch b.py (whole-file, file does not exist) -> read outside_grant.txt (grant violation) -> done.
    // touched_files has b.py -> Absent; the refused read contributes NO entry.
}

#[test]
fn truncated_read_is_uncomputable() {
    // ExecBounds { read_cap_bytes: 4, ..default } and a 10-byte file: read -> done.
    // touched_files entry is PreTouch::Uncomputable.
}
```

- [ ] **Step 2: Run to verify failure:** `cargo test -p bloomery-daemon --test memory_capture_test` — expected: compile error (`touched` unknown field).
- [ ] **Step 3: Implement** per Interfaces. Fix every `Observation`/`TaskResult` construction site the compiler names (including `test_support.rs`, `codec_probe`, and existing tests) with `touched: None` / empty collections — mechanical, no behavior change.
- [ ] **Step 4: Run the full workspace suite** — `cargo test --workspace` — expected: new tests PASS, zero regressions (the goldens and anti-drift tests prove rendering and journaling untouched).
- [ ] **Step 5: Commit:** `git commit -am "feat: capture pre-first-touch fingerprints and landed patch bodies in the task loop"`

---

### Task 4: Retrieval (`memory/retrieve.rs`) — two-stage exact match plus the grant gate

**Files:**
- Create: `crates/bloomery-daemon/src/memory/retrieve.rs`
- Modify: `crates/bloomery-daemon/src/memory.rs` (add `pub mod retrieve;`)
- Test: Create `crates/bloomery-daemon/tests/memory_retrieve_test.rs`

**Interfaces:**
- Consumes: Task 1 records; Task 2 `MemoryStore::candidates`; `bloomery_core::grant::Grant` (`check_read` returns the canonical path — `grant/mod.rs:190`; `read_roots()` :165); `sha256_hex_bytes`.
- Produces:
  - `pub struct Retrieval { pub injected: Option<EpisodeRecord>, pub candidates_checked: u32 }`
  - `pub fn retrieve(store: &MemoryStore, goal: &str, grant: &Grant, _cwd: &Path) -> Retrieval`

Per-candidate gates, in spec §3's order, all of them silent-on-failure (any mismatch/error disqualifies the candidate, never errors the task — spec §7):
1. Status gate: `status == "verified"`.
2. Fingerprint + grant gate per `cited_files` entry (stored paths are already canonical-absolute from Task 3):
   - Expected `Sha256(h)`: `grant.check_read(Path::new(&cf.path))` must return `Ok(canon)` (this IS the grant gate for existing files), then `std::fs::read(&canon)` must succeed and `sha256_hex_bytes(&bytes) == h`.
   - Expected `Absent`: the path must NOT exist (`!Path::new(&cf.path).exists()`) AND must sit lexically under some `grant.read_roots()` entry (`path.starts_with(root)`) — a nonexistent path cannot be canonicalized, so lexical containment is the honest check; record this as a code comment citing spec §3.
3. Selection: among survivors, the greatest `minted_at` (ties by `episode_id`, descending, for determinism); at most ONE injected (spec §3).

`candidates_checked` counts every candidate examined (survivor or not).

- [ ] **Step 1: Write the failing tests.** Build real workspaces with `fresh_dir`, real `Grant`s via the `ok_grant` JSON pattern (`registry.rs:311`), and hand-built `EpisodeRecord`s whose `cited_files` point into the workspace:

```rust
#[test] fn exact_hit_injects_the_most_recently_verified_survivor() { /* two matching episodes, minted_at 1 and 2 -> injected is 2; candidates_checked == 2 */ }
#[test] fn one_changed_byte_is_silence() { /* flip one byte in one cited file -> injected None */ }
#[test] fn absent_expectation_matches_only_a_missing_file() { /* Absent + file exists -> silent; Absent + missing -> hit */ }
#[test] fn grant_not_covering_a_cited_path_is_silence() { /* grant rooted at dir_a, episode cites dir_b -> silent even though bytes match */ }
#[test] fn contradicted_is_silence() { /* status contradicted -> silent */ }
#[test] fn unreadable_cited_file_is_silence_not_error() { /* cited path is a dangling entry (e.g. a directory) -> Retrieval returns, injected None */ }
```

- [ ] **Step 2: Run to verify failure**, **Step 3: Implement**, **Step 4: Run to verify pass** (`cargo test -p bloomery-daemon --test memory_retrieve_test`).
- [ ] **Step 5: Mutation-check the two predicates this task carries (spec §8).** Apply each temporary edit, run the named test expecting FAIL, then `git checkout -- crates/bloomery-daemon/src/memory/retrieve.rs`:
  1. Make the fingerprint compare always succeed (`sha256_hex_bytes(&bytes) == h` → `true`) → `one_changed_byte_is_silence` must FAIL.
  2. Skip the `check_read` call (treat every existing path as granted) → `grant_not_covering_a_cited_path_is_silence` must FAIL.
  If either suite stays green, the test is not binding — fix the test, not the mutation.
- [ ] **Step 6: Commit:** `git commit -am "feat: memory retrieval — two-stage exact match, grant gate, single-survivor selection"`

---

### Task 5: The mint bar and episode construction (`memory/mint.rs`)

**Files:**
- Create: `crates/bloomery-daemon/src/memory/mint.rs`
- Modify: `crates/bloomery-daemon/src/memory.rs` (add `pub mod mint;`)
- Test: Create `crates/bloomery-daemon/tests/memory_mint_test.rs`

**Interfaces:**
- Consumes: `TaskResult`/`TaskStatus`/`TaskStepRecord`/`PreTouch` (Task 3), Task 1 records, `PatchBody`/`PatchCodec`.
- Produces:
  - `pub fn verifying_run(result: &TaskResult) -> Option<&TaskStepRecord>` — the mint bar (spec §2), computed from steps alone: `result.status == TaskStatus::Done`, AND at least one successful patch step (`verb == "patch" && !failed`), AND a step AFTER the last successful patch with `verb == "run" && !failed && outcome.ends_with(" exit 0")`. The outcome suffix is `exec_run`'s pinned format `"ran {program} exit {code}"` (`exec_run.rs`, success arm) — a completed run reports `failed: false` for EVERY exit code, so the suffix match is the only exit evidence; state that in a comment citing `exec_run.rs`. Returns that run step.
  - `pub struct MintInputs<'a> { pub goal: &'a str, pub model: &'a str, pub envelope: &'a str, pub minted_at: u64 }`
  - `pub fn build_episode(result: &TaskResult, inputs: &MintInputs<'_>) -> Option<EpisodeRecord>` — `None` when `verifying_run` is `None` OR any `touched_files` value is `PreTouch::Uncomputable` (an unpinnable input must not mint — spec §2 via Task 3's `Uncomputable`). Otherwise: `cited_files` from `touched_files` (`Sha256`→`Fingerprint::Sha256`, `Absent`→`Fingerprint::Absent`), `landed_patches` from `result.landed_patches` rendered as `StoredPatch` — `PatchBody::WholeFile { contents }` → `codec: "whole_file", body: contents`; `PatchBody::SearchReplace { search, replace }` → `codec: "search_replace"`, `body` in the conflict-marker wire form `"<<<<<<< SEARCH\n{search}\n=======\n{replace}\n>>>>>>> REPLACE"` (the grammar `bloomery_core::action::patch` parses — `patch.rs:17` and its module doc) — `run_evidence` from the verifying run (`argv` = its `args`, `outcome` = its `outcome`), `trajectory` = every step's verb in order, `goal_hash`/`goal_text` via Task 1, `episode_id` via Task 1, `status: "verified"`, `contradicted_by: None`.

- [ ] **Step 1: Write the failing tests.** Hand-build `TaskResult`s (no substrate needed — the bar is a pure function of steps):

```rust
fn step(verb: &str, outcome: &str, failed: bool, args: &[&str]) -> TaskStepRecord { /* step number by index */ }

#[test] fn done_patch_then_run_exit_0_mints() { /* read, patch(ok), run "ran python3 exit 0"(failed:false) , done -> Some */ }
#[test] fn refusal_shape_does_not_mint() { /* read, done; status Done; no patch -> None */ }
#[test] fn run_nonzero_exit_does_not_mint() { /* patch ok, run outcome "ran python3 exit 1", failed:false -> None */ }
#[test] fn run_before_the_last_successful_patch_does_not_mint() { /* patch, run exit 0, patch, done -> None */ }
#[test] fn non_done_status_and_failed_run_do_not_mint() { /* StepsExhausted -> None; run failed:true -> None */ }
#[test] fn uncomputable_touched_file_refuses_to_mint() { /* bar satisfied but touched_files has Uncomputable -> build_episode None */ }
#[test] fn build_episode_renders_search_replace_wire_form() { /* assert exact body bytes incl. markers */ }
```

- [ ] **Step 2/3/4:** failing run → implement → `cargo test -p bloomery-daemon --test memory_mint_test` PASS.
- [ ] **Step 5: Mutation-check the mint bar (spec §8).** Three temporary edits, each must fail a named test, then revert:
  1. `ends_with(" exit 0")` → `true` → `run_nonzero_exit_does_not_mint` FAILS.
  2. Drop the "after the last successful patch" ordering (accept any run) → `run_before_the_last_successful_patch_does_not_mint` FAILS.
  3. Drop the ≥1-successful-patch requirement → `refusal_shape_does_not_mint` FAILS.
- [ ] **Step 6: Commit:** `git commit -am "feat: memory mint bar and episode construction from task step evidence"`

---

### Task 6: Rendering and injection (`memory/render.rs` + the prompt seam)

**Files:**
- Create: `crates/bloomery-daemon/src/memory/render.rs`
- Modify: `crates/bloomery-daemon/src/memory.rs` (add `pub mod render;`)
- Modify: `crates/bloomery-daemon/src/task/task_loop.rs` (`TaskSpec` :50, `RenderInputs` :312, `render_prompt` :295, `render_prompt_from` :343)
- Test: Create `crates/bloomery-daemon/tests/memory_render_test.rs`

**Interfaces:**
- Produces:
  - `pub fn render_memory_block(e: &EpisodeRecord) -> String` — deterministic, quoted-evidence-only (spec §4: no advice prose, nothing model-written). Pinned shape (exact bytes, one trailing newline **not** included — the renderer's section formatting adds separation):

```text
[memory: verified prior attempt]
This exact goal was completed before against byte-identical starting files.
--- patch {path} ({codec})
{body}
Verification: {argv joined with spaces} -> {run_evidence.outcome}
[end memory]
```

  (one `--- patch` stanza per landed patch, in order).
  - `TaskSpec` gains `pub memory_block: Option<String>` (default at every existing construction site: `None` — compiler-guided; `api_task.rs`'s `create_task` builds it as `None`, the worker sets it — Task 7).
  - `RenderInputs` gains `memory_block: Option<&'a str>`; `render_prompt` passes `spec.memory_block.as_deref()`; `render_task_prompt` (:394, the flywheel-tool face) passes `None` **hardcoded, signature unchanged** — the factory and the goldens never see memory (spec §4).
  - `render_prompt_from` inserts the section immediately after the goal, before the grant section (spec §4): `let memory_section = match inputs.memory_block { Some(b) => format!("{b}\n\n"), None => String::new() };` and the format becomes `"{goal}\n\n{memory_section}{grant_section}{}\n\n{transcript}"`. `None` renders the empty string → byte-identical to today.

- [ ] **Step 1: Write the failing tests** in `tests/memory_render_test.rs`:

```rust
#[test]
fn render_memory_block_is_deterministic_and_pinned() {
    // Build an EpisodeRecord with one search_replace patch; assert the EXACT block string
    // (paste the expected literal — this is this surface's golden).
}

#[test]
fn absent_memory_renders_byte_identical_prompts() {
    // render_task_prompt(goal, codec, EnvelopeLens::V4, &commands, transcript) — the public face —
    // must equal its pre-change output. The existing task_render_test.rs goldens already pin
    // v1/v2/v3; assert here that a TaskSpec { memory_block: None } run and the wrapper agree
    // (drive run_task with a scripted done via a local RecordingSubstrate that appends every
    // infer prompt into an Arc<Mutex<Vec<String>>> — mirror registry.rs's local PanicSubstrate
    // pattern (:434) with infer recording instead of panicking — and byte-compare prompt[0]
    // against render_task_prompt's output for the same inputs).
}

#[test]
fn injected_memory_appears_after_goal_before_grant_line() {
    // Same RecordingSubstrate; TaskSpec { memory_block: Some("[memory: verified prior attempt]...".into()), envelope: V4 }.
    // Assert prompt[0] starts with "{goal}\n\n[memory: " and contains "[end memory]\n\n" before the
    // grant line's text.
}
```

- [ ] **Step 2/3/4:** failing → implement → `cargo test -p bloomery-daemon --test memory_render_test` PASS, then **`cargo test --workspace`** — the pre-existing goldens in `task_render_test.rs` and all four anti-drift tests MUST be green untouched; if any golden moved, the change is wrong (spec §4), fix the renderer, never the golden.
- [ ] **Step 5: Commit:** `git commit -am "feat: memory block rendering and prompt injection — off/silent renders byte-identical"`

---

### Task 7: Journal events and the worker pipeline

**Files:**
- Modify: `crates/bloomery-core/src/journal.rs` (three additive `Event` variants)
- Modify: `crates/bloomery-daemon/src/pager.rs` (small accessor `agent_model`)
- Modify: `crates/bloomery-daemon/src/task/registry.rs` (`spawn_task` :160 — the pipeline)
- Modify: `crates/bloomery-daemon/src/api_task.rs` (`create_task` :103 — thread the context)
- Test: Create `crates/bloomery-daemon/tests/memory_task_test.rs`; extend `crates/bloomery-core`'s journal tests (`journal_test.rs`) with a replay-compat case

**Interfaces:**
- New `Event` variants (additive — old journals carry none of these tags and replay unchanged, same mechanism as every variant added since `TaskStep`):

```rust
/// The memory organ's per-task stamp (memory-organ design §4). Written once
/// per spawned task, before step 1: `mode` is "off" | "silent" | "injected".
MemoryStamp { id: AgentId, task_id: String, mode: String, episode_id: Option<String>, candidates_checked: u32 },
/// A verified task minted (or refreshed) an episode (design §2).
MemoryMint { id: AgentId, task_id: String, episode_id: String },
/// An injected episode's task failed its own verification (design §5).
MemoryContradicted { id: AgentId, task_id: String, episode_id: String },
```

- `Pager::agent_model(&self, agent_id: &str) -> Option<String>` — the agent's model name; mirror `agent_task_policy`'s `self.table.get(agent_id)` lookup.
- `MemoryContext` — define in `crates/bloomery-daemon/src/memory.rs`:

```rust
pub struct MemoryContext {
    pub enabled: bool,               // config switch
    pub max_episodes: usize,
    pub disabled_reason: Option<String>, // store unreadable at boot (design §7)
    pub store: Option<std::sync::Mutex<MemoryStore>>, // None exactly when disabled_reason is Some
}
impl MemoryContext {
    pub fn operational(&self) -> bool { self.enabled && self.store.is_some() }
}
```

- `spawn_task` gains a final parameter `memory: Option<std::sync::Arc<MemoryContext>>`; `api_task::dispatch` and `create_task` gain the same parameter and thread it through (mirror how `swap: Option<&Arc<SwapContext>>` threads through `http.rs:196` — Task 8 does the `http.rs` end).
- Worker pipeline, inside the spawned thread, in this order (design §4/§5; all store IO failures journal a `Degraded { reason }` row and continue — the organ must never fail the task, design §7):
  1. Before locking the pager: if `memory` is `Some(ctx)` and `ctx.operational()`, lock the store briefly and `retrieve(&store, &spec.goal, &spec.grant, &spec.cwd)`; else mode `"off"`.
  2. Append `MemoryStamp` via the worker's own `Journal` handle (already opened at `registry.rs:185`), with `mode`/`episode_id`/`candidates_checked` from step 1. A stamp is written for EVERY spawned task, `"off"` included (design §4).
  3. If injected: `spec.memory_block = Some(render_memory_block(&episode))` (make `spec` `mut`); remember `injected_id`.
  4. While holding the pager guard, before `run_task`: `let model = guard.agent_model(&agent_id).unwrap_or_else(|| agent_id.clone());` and `let envelope = format!("{:?}", spec.envelope);` (recorded, never compared — design §2).
  5. Run the task exactly as today (catch_unwind untouched).
  6. After the pager guard drops, if `ctx.operational()`:
     - If `injected_id` is `Some(id)` and `verifying_run(&result).is_none()`: `store.mark_contradicted(&id, &task_id)` + append `MemoryContradicted` (design §5 — an injected task that fails its verification contradicts what it was shown; an injected task that verifies falls through to the mint below, which refreshes the same identity).
     - `minted_at` = the same `SystemTime`-derived millis expression `Journal::append` uses (`journal.rs:414-417`).
     - If `build_episode(&result, &MintInputs { goal: &spec.goal, model: &model, envelope: &envelope, minted_at })` returns `Some(ep)`: `store.mint(ep, ctx.max_episodes)` + append `MemoryMint`.
  7. Insert the registry entry (unchanged).

- [ ] **Step 1: Write the failing tests** in `tests/memory_task_test.rs` — the full-loop binding tests (spec §8), GPU-free. Build a workspace with a planted failing check the scripted model "fixes": file `a.py`; grant `{"read_roots":[dir],"write_roots":[dir],"commands":[["python3","-c"]]}` — but scripted turns never really need python: script the turns `read a.py` → `patch a.py` (whole-file) → `run ["python3","-c","pass"]` → `done`, and let the REAL `exec_run` run `python3 -c pass` (exit 0; python3 exists on this box and `check_command` grants the `["python3","-c"]` prefix). Use `registry.rs`'s helpers; pass `Some(ctx)` with a store in a fresh dir:

```rust
#[test]
fn verified_task_mints_and_journal_carries_stamp_and_mint() {
    // spawn -> poll to Done. Store: 1 verified episode; its cited_files include a.py's pre-patch sha.
    // Replay tasks.jsonl: exactly one MemoryStamp{mode:"silent"} (first run: no candidates) and one MemoryMint.
}

#[test]
fn exact_repeat_is_injected_and_a_stranger_is_silent() {
    // After the mint above, reset a.py to its original bytes; spawn the SAME goal again ->
    // replayed journal has MemoryStamp{mode:"injected", episode_id:Some(..)} and the task completes.
    // Then spawn a different goal -> MemoryStamp{mode:"silent"}.
    // Then drift a.py by one byte and spawn the same goal -> "silent".
}

#[test]
fn injected_then_failed_contradicts_and_next_repeat_is_silent() {
    // Repeat with scripted turns that end WITHOUT a verifying run (read -> done).
    // Journal: MemoryContradicted; store: episode status contradicted; a further repeat stamps "silent".
}

#[test]
fn memory_none_and_disabled_stamp_off_and_never_touch_the_store() {
    // spawn with memory: None -> stamp mode "off". With Some(ctx{enabled:false}) -> "off" too.
}
```

  In `bloomery-core`'s journal tests: a compat test appending each new variant, replaying, and asserting equality — plus re-run the existing `committed_g2_journal_still_replays` pin (old journals must be untouched by the new variants).
- [ ] **Step 2/3/4:** failing → implement → `cargo test -p bloomery-daemon --test memory_task_test && cargo test -p bloomery-core` PASS, then `cargo test --workspace` (the `spawn_task` signature change touches `api_task` tests — fix call sites with `None`).
- [ ] **Step 5: Commit:** `git commit -am "feat: memory journal events and the spawn_task retrieve-stamp-run-mint pipeline"`

---

### Task 8: Config, boot wiring, and `/status`

**Files:**
- Modify: `crates/bloomery-daemon/src/config.rs` (`Config` :427 gains `memory`; new `MemoryConfig`)
- Modify: `crates/bloomery-daemon/src/memory.rs` (`pub fn build_memory(cfg: &MemoryConfig, data_dir: &Path) -> Arc<MemoryContext>`)
- Modify: `crates/bloomery-daemon/src/http.rs` (build the context in `run_server` :95 next to the registry at :157; thread `Option<&Arc<MemoryContext>>` through `worker_loop` :196 into `api_task::dispatch` and `api_native::dispatch`)
- Modify: `crates/bloomery-daemon/src/api_native.rs` (`dispatch` gains the parameter; `status` :628 augments)
- Test: extend `crates/bloomery-daemon/tests/api_native_test.rs` (or the config test file if one exists — follow where `tasks_enabled` is tested); config parse test inline in `config.rs`'s test module

**Interfaces:**

```rust
// config.rs
#[derive(Debug, Clone, serde::Deserialize)]
pub struct MemoryConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_max_episodes")]
    pub max_episodes: usize, // default 256
}
impl Default for MemoryConfig { /* enabled: false, max_episodes: 256 */ }
// Config gains: #[serde(default)] pub memory: MemoryConfig,
```

- `build_memory`: store path is `<data_dir>/memory/episodes.jsonl` (design §6). `MemoryStore::load` failure → `MemoryContext { enabled: cfg.enabled, disabled_reason: Some(format!("memory store unreadable: {e}")), store: None, .. }` — boot proceeds, tasks run memory-off (design §7). Load success → `store: Some(Mutex::new(store))`, `disabled_reason: None`. When `cfg.enabled` is false, still build the context (counts stay renderable) — `operational()` is the single gate.
- `status`: signature `fn status<S: Substrate>(pager: &Mutex<Pager<S>>, memory: Option<&MemoryContext>) -> ApiResult`; after `to_value(p.status())`, insert:

```rust
if let Some(m) = memory {
    let counts = m.store.as_ref().map(|s| s.lock().unwrap_or_else(std::sync::PoisonError::into_inner).counts());
    v["memory"] = serde_json::json!({
        "enabled": m.enabled,
        "episodes": counts.map(|c| c.episodes), "verified": counts.map(|c| c.verified),
        "contradicted": counts.map(|c| c.contradicted), "parse_errors": counts.map(|c| c.parse_errors),
        "disabled_reason": m.disabled_reason,
    });
}
```

- [ ] **Step 1: Write the failing tests:** (a) config: a TOML without `[memory]` parses with `enabled == false && max_episodes == 256`; with `[memory] enabled = true` parses enabled (copy the file-less `Config` parse pattern from `config.rs`'s existing tests). (b) `/status` carries the `memory` object with zero counts for a fresh enabled context, and `disabled_reason` when `build_memory` is pointed at an unreadable path (create the store path as a DIRECTORY to force the load error). (c) `build_memory` on a fresh dir is operational and creates nothing until the first mint.
- [ ] **Step 2/3/4:** failing → implement → targeted tests PASS → `cargo test --workspace` (the `api_native::dispatch`/`status` signature changes are compiler-guided at the `http.rs` and test call sites; pass `None` where no context exists).
- [ ] **Step 5: Commit:** `git commit -am "feat: memory config, boot wiring, and /status surface"`

---

### Task 9: The operator routes (`api_memory.rs`)

**Files:**
- Create: `crates/bloomery-daemon/src/api_memory.rs`
- Modify: `crates/bloomery-daemon/src/lib.rs` (declare `pub mod api_memory;`)
- Modify: `crates/bloomery-daemon/src/http.rs` (`worker_loop` dispatch chain: try `api_memory::dispatch` before the `api_native` fallthrough, exactly where `api_task::dispatch` slots — mirror its `None`-falls-through contract, `api_task.rs` module doc)
- Test: Create `crates/bloomery-daemon/tests/api_memory_test.rs`

**Interfaces:**
- `pub fn dispatch(memory: Option<&Arc<MemoryContext>>, method: &str, segments: &[&str]) -> Option<ApiResult>` — `None` for any path that is not `["memory"]` or `["memory", id]`; import `ApiResult` from `api_native`.
- The dark rule FIRST (design §6, the `tasks_enabled` pattern — `api_task.rs` module doc): if the context is `None` or `!ctx.enabled` → `Some((501, json!({"error": "memory_disabled"})))` for both routes, before anything else. An enabled-but-broken store (`disabled_reason`) → `503 {"error": "memory_unavailable", "reason": ...}`.
- `GET /memory` → `200 {"episodes": [{episode_id, goal_text, cited_paths: [..], status, minted_at, minted_by_model}]}` (operator display fields only — design §6).
- `DELETE /memory/{id}` → `200 {"deleted": id}` on `Ok(true)`, `404 {"error": "not_found"}` on `Ok(false)`, `500 {"error": "store_io", ...}` on `Err` (a later verified completion may re-mint the id — design §6; nothing to enforce, just don't tombstone the identity anywhere else).
- Any other method on these paths → `405 {"error": "method_not_allowed"}`.

- [ ] **Step 1: Write the failing tests** (drive `dispatch` directly, no HTTP server — the route-table tests for `api_task` show the shape): disabled → 501 both routes even with garbage segments; enabled+fresh → GET lists `[]`; after a `mint` through the store handle → GET lists one with the exact fields above; DELETE it → 200 then GET lists `[]` and a reload of the store file agrees (tombstone durable); DELETE unknown → 404; `POST /memory` → 405; `["memories"]` → `None` (fallthrough).
- [ ] **Step 2/3/4:** failing → implement → `cargo test -p bloomery-daemon --test api_memory_test` PASS → `cargo test --workspace`.
- [ ] **Step 5: Commit:** `git commit -am "feat: memory operator routes — GET list and DELETE purge, dark when disabled"`

---

### Task 10: Live acceptance (HUMAN-GATED) and the evidence doc

**Files:**
- Create: `docs/superpowers/evidence/2026-08-26-memory-organ-acceptance.md`
- Modify: `README.md` (one line in the organ/feature list naming the memory organ, config-off by default, pointing at the spec)

**HUMAN GATE: get Brice's explicit go before booting anything on the GPU.**

- [ ] **Step 1: Merge-readiness check first** (this task boots the REAL daemon): whole-branch `cargo fmt --check`, `cargo clippy --workspace -- -D warnings`, `cargo test --workspace` all green, THEN the featured build LAST: `cargo build -p bloomery-daemon --features vulkan` (box law: a test run after this overwrites the binary featureless — do not re-run tests after this step).
- [ ] **Step 2: Prepare the scratch acceptance workspace** (mechanism-only, spec §8): a dir with `calc.py` (a planted off-by-one) and `test_calc.py` (a `unittest` that fails before, passes after); a config with `tasks_enabled = true`, `[memory] enabled = true`, a scratch `data_dir`; the resident model. Keep a byte copy of the workspace for the reset.
- [ ] **Step 3: Arm 1 — mint.** Create an agent; `POST /agents/{id}/task` with the goal `fix the off-by-one in calc.py so test_calc passes` and a grant carrying `[["python3","-m","unittest"]]`. Poll to `Done`. Record: `GET /memory` shows one verified episode; the tasks journal carries `MemoryStamp{mode:"silent"}` + `MemoryMint`; `/status.memory` counts agree.
- [ ] **Step 4: Arm 2 — exact repeat.** Restore the workspace bytes exactly; resubmit the identical goal. Record: `MemoryStamp{mode:"injected", episode_id}` and task `Done`.
- [ ] **Step 5: Arm 3 — stranger.** A different goal against the same workspace → `MemoryStamp{mode:"silent"}`.
- [ ] **Step 6: Arm 4 — drift.** Restore the workspace, then append one byte to `calc.py`; resubmit the arm-1 goal → `MemoryStamp{mode:"silent"}`.
- [ ] **Step 7: Write the evidence doc.** Per house norm: config used, every request/response, the journal rows verbatim (quote the stamp/mint rows), store-file row counts, `/status.memory` before/after, and NO capability sentence — mechanism claims only (spec §1: no number from this acceptance may appear in a capability sentence; the store file's whole-journal byte-identity across re-runs must never be pre-registered — `minted_at` is a row property, journal.rs precedent).
- [ ] **Step 8: Commit:** `git commit -am "docs: memory organ live acceptance — mint, injected repeat, stranger and drift silence"` — then STOP: merge/PR is Brice's call (finishing-a-development-branch).

---

## Self-Review (performed while writing)

- **Spec coverage:** §2 record+mint → Tasks 1/3/5; §3 retrieval → Task 4; §4 injection+stamp+envelope rule → Tasks 6/7 (instruments untouched — no probe/drift/swap file is modified anywhere in this plan); §5 passive contradiction → Task 7; §6 store/config/routes/retention → Tasks 2/8/9; §7 error handling → Tasks 2/4/7/8 (silence-not-error tests, `Degraded` on store IO, disabled-with-reason); §8 tests incl. the three mutation checks → Tasks 4/5 mutation steps + binding tests throughout; §9 out-of-scope respected (no non-exact retrieval, no active refalsify, no `/v1` memory); §10 delegated shapes are pinned here (Event layouts in Task 7, capture seam in Task 3, block bytes in Task 6, tombstone row in Task 1).
- **Type consistency:** `PreTouch`/`Touched` (task/mod.rs, Task 3) vs `Fingerprint` (record.rs, Task 1) are deliberately distinct types with the mapping in Task 5's `build_episode`; `verifying_run`/`build_episode`/`retrieve`/`render_memory_block`/`MemoryContext` names match across Tasks 5/6/7/8/9.
- **Placeholder scan:** every helper the tests reference is either quoted or named with its source location (`fresh_dir`/`ok_grant`/`build_pager` from `registry.rs:271-318`; envelope grammar from `task_loop_test.rs`); no TBDs.
