# Drift watch — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Each boot, every model's POST profile is compared against the previous boot's profile and the operator-blessed baseline through `assay diff --gate`; a tripped gate is confirmed before it alarms; outcomes land in the journal and `ModelStatus.drift`. Observability only — admission and `done_trust` untouched.

**Architecture:** A `DriftGate` mirrors `PostRunner`'s subprocess pattern (injected `CommandRunner`, bounded wait, artifact-file contract) around `python3 -m assay diff <ref> <cur> --gate`, consuming its documented exit codes. Profile retention grows a per-model rotation (current/previous/baseline/transients) under the existing `profiles/` dir. New journal `Event` variants carry every outcome by name with profile *paths*, never transcribed numbers. The instrument-changed rule is a pure-function precheck on the two profiles' own version fields, applied before diff ever runs.

**Tech Stack:** Rust (bloomery-daemon + bloomery-core), serde, the existing subprocess-injection test seams; assay 0.9.0 (schema v8) via `python3 -m assay` on the deployment PYTHONPATH.

**Spec:** `docs/superpowers/specs/2026-08-17-drift-watch-design.md` — the plan argues from it; executors read both. House laws bind: law 5 (None-vs-zero, admission), law 7 (unwritable journal aborts).

## Global Constraints

- **TDD strictly**: failing test first with RED evidence, minimal green, mutation-check every load-bearing new test (break the pinned line → test FAILS → restore) and record the outputs in the report.
- **Test/build commands**: `cargo test -p bloomery-core -p bloomery-daemon` for suites; any binary the daemon actually runs is built with `cargo build --features vulkan` (a featureless daemon cannot load models — house gotcha). NEVER wrap any command in `timeout` (broken wrapper on this box, exit 139 = the wrapper crashed).
- **No admission changes, no `done_trust` writes** (spec §7). `ModelStatus.drift` is a new, separate field.
- **Journal rows carry paths, never numbers copied out of profiles** (spec §4).
- **Never parse `assay diff` prose** — exit codes and profile files are the whole contract.
- **Committed evidence/fixtures are never edited after commit**; new fixtures are real artifacts (a genuine assay v8 profile), not hand-mocked JSON, wherever a real one exists.
- Commit style: single-line conventional commits, no attribution trailers. Work on a feature branch (`feat/drift-watch`); no pushes until the wave's end (final review → PR, merge=Brice, per bloomery house flow).
- GPU: only Task 6 touches the daemon/GPU, and only after checking nothing else is resident (`curl -s localhost:11434/api/ps` is the OLLAMA daemon — irrelevant here; bloomery's substrate loads GGUFs directly — check `nvidia-smi` shows the GPU substantially free before booting).

## File Structure

- `crates/bloomery-core/src/profile.rs` — modify: `probe_version()` accessor; instrument-precheck pure function.
- `crates/bloomery-core/src/journal.rs` — modify: new `Event` variants (`Blessed`, `Drift`).
- `crates/bloomery-daemon/src/drift.rs` — create: `DriftGate` (subprocess + argv + bounded wait), retention/rotation, the comparison orchestration, confirm-then-alarm.
- `crates/bloomery-daemon/src/post.rs` — modify: hand the fresh profile + paths into the drift flow after each successful probe (wiring only; POST's own semantics untouched).
- `crates/bloomery-daemon/src/pager/status.rs` — modify: `ModelStatus.drift`.
- `crates/bloomery-daemon/src/api_native.rs` — modify: `("POST", ["models", name, "bless"])`.
- `crates/bloomery-daemon/src/config.rs` — modify only if a knob is genuinely needed (transient retention N=4 is a constant, not config — YAGNI).
- Tests beside each (bloomery keeps `tests/` files per concern: `crates/bloomery-daemon/tests/drift_test.rs` new; extend `post` tests where wiring shows).
- Fixture: `crates/bloomery-daemon/tests/fixtures/profile-v8-qwen15b.json` — create: a REAL assay 0.9.0 profile (copy `qwen2.5-coder-1.5b-instruct-q8_0.json` from the assay repo's `docs/superpowers/evidence/tier-enthusiast-2026-08/`, verbatim bytes, provenance noted in a sibling comment file or the test docstring).

---

### Task 1: Profile v8 compatibility + the instrument-changed precheck

**Files:**
- Modify: `crates/bloomery-core/src/profile.rs`
- Create: `crates/bloomery-daemon/tests/fixtures/profile-v8-qwen15b.json` (real artifact, see File Structure)
- Test: `crates/bloomery-core/src/profile.rs` (unit tests in-module, the crate's existing pattern) + a daemon-side fixture-parse test

**Interfaces:**
- Consumes: existing `Profile::from_json` (rejects schema < 2, serde-tolerant of unknown keys), `ProfileData` (private, serde-flattened).
- Produces:
  - `Profile::probe_version(&self) -> &str` — the document's `probe_version` field, newly deserialized into `ProfileData` (`pub(crate)` field, accessor public). Required — serde must FAIL a document without it (it has been in every assay schema since v1), so the accessor returns `&str`, not `Option`.
  - `pub fn instrument_precheck(reference: &Profile, current: &Profile) -> InstrumentPrecheck` with
    ```rust
    #[derive(Debug, Clone, PartialEq)]
    pub enum InstrumentPrecheck {
        Comparable,
        InstrumentChanged { reference: String, current: String }, // "0.5.0/v4" style: probe_version + "/v" + schema
    }
    ```
    Rule (spec §3): `InstrumentChanged` iff `probe_version` OR `schema_version()` differ; the strings carry both fields of both sides so the journal row can name them without re-reading files.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn a_real_v8_profile_parses_and_names_its_instrument() {
    let text = include_str!("../../bloomery-daemon/tests/fixtures/profile-v8-qwen15b.json");
    let p = Profile::from_json(text).unwrap();
    assert_eq!(p.schema_version(), 8);
    assert_eq!(p.probe_version(), "0.9.0");
    assert_eq!(p.model_name(), "qwen2.5-coder:1.5b-instruct-q8_0");
}

#[test]
fn same_instrument_is_comparable_different_is_named_never_scored() {
    // Two handles onto the same fixture: comparable.
    // A v8 profile vs a v4 fixture (the existing old-schema test fixture): InstrumentChanged
    // carrying BOTH sides' probe_version and schema, e.g. "0.5.0/v4" vs "0.9.0/v8".
}

#[test]
fn probe_version_is_mandatory_not_optional() {
    // Strip the probe_version key from the fixture text → from_json is Err (Parse),
    // never a default that looks like a version.
}
```

(Exact assertions for the second test written against whatever old-schema fixture the repo already carries — find it with `grep -rl assay_profile_version crates/*/tests`; if none exists at v4, take a committed v4 profile from the assay repo's `tier-enthusiast/` the same way as the v8 one.)

- [ ] **Step 2: RED** — `cargo test -p bloomery-core` fails on missing accessor/function.
- [ ] **Step 3: Implement** — add `probe_version` to `ProfileData` (no `Option`, no default), the accessor, and `instrument_precheck`.
- [ ] **Step 4: GREEN** — both crates' suites.
- [ ] **Step 5: Mutation checks** — (a) make the precheck compare only schema (ignore probe_version) → the precheck test FAILS on a same-schema/different-probe pair (add that pair to the test if not present); (b) default probe_version to "" on missing → mandatory test FAILS. Restore, record.
- [ ] **Step 6: Commit** — `feat: v8 profile compat — probe_version accessor and the instrument-changed precheck`

### Task 2: Retention, rotation, and blessing

**Files:**
- Create: `crates/bloomery-daemon/src/drift.rs` (module start: the `ProfileStore`)
- Modify: `crates/bloomery-core/src/journal.rs` (the `Blessed` variant), `crates/bloomery-daemon/src/lib.rs` (module decl)
- Test: `crates/bloomery-daemon/tests/drift_test.rs` (new)

**Interfaces:**
- Consumes: `config.data_dir.join("profiles")` (exists, created in main.rs), `Profile::from_json`, `Journal` (core).
- Produces:
  ```rust
  pub struct ProfileStore { root: PathBuf } // root = profiles dir
  impl ProfileStore {
      pub fn paths(&self, model: &str) -> ModelPaths;      // current/previous/baseline paths, slug = model with [:/]→'-' (POST's existing slug rule — read post.rs/main.rs for the exact helper and REUSE it, never a second slug)
      pub fn rotate(&self, model: &str) -> io::Result<Rotation>; // current→previous BEFORE the probe deletes/rewrites current; Rotation names what moved
      pub fn bless(&self, model: &str) -> Result<Blessing, DriftError>; // copy current→baseline; returns the blessed file's sha256 prefix + path
      pub fn retain_transient(&self, model: &str, path: &Path) -> io::Result<PathBuf>; // content-addressed name (sha256 prefix), keep newest 4, dropped ones returned for journaling
  }
  ```
  - Journal `Event::Blessed { model: String, profile_path: String, sha: String, provenance: String }` — provenance ∈ {"operator", "auto-first-profile"}.
- Rotation law (spec §5): rotation happens only after the CURRENT file parsed successfully this boot; POST's delete-before-probe still runs after rotation, so a stale current can never survive as current — pin with a test that a failed-parse boot leaves previous untouched.

- [ ] **Step 1: Failing tests** — paths/slug reuse (same slug function object as POST's, asserted by identity or by a shared-constant test); rotate moves current→previous and is a no-op with a named result when current is absent; bless copies + returns sha matching the bytes; transients: 5th retained drops the oldest and returns its path; a failed-parse scenario leaves previous untouched.
- [ ] **Step 2: RED.** **Step 3: implement.** **Step 4: GREEN** both crates.
- [ ] **Step 5: Mutation checks** — swap rotation order (delete before rotate) → the failed-parse test FAILS; bless returning a sha of the PATH string instead of the bytes → sha test FAILS.
- [ ] **Step 6: Commit** — `feat: profile retention and blessing — rotation, content-addressed transients, Blessed journal row`

### Task 3: The gate — subprocess diff, precheck first, two comparisons

**Files:**
- Modify: `crates/bloomery-daemon/src/drift.rs`, `crates/bloomery-core/src/journal.rs` (the `Drift` variant)
- Test: `crates/bloomery-daemon/tests/drift_test.rs`

**Interfaces:**
- Consumes: `PostRunner`'s pattern — copy the shape, not the code: injected `CommandRunner` (`post.rs` line ~140 `with_runner`), `run_bounded_for_test`, `argv`-as-inspectable-value; `instrument_precheck` (Task 1); `ProfileStore` (Task 2); `AssayConfig::probe_timeout_secs` (reused as the diff bound — diff is offline and fast; a tighter constant DIFF_TIMEOUT_SECS = 60 is fine, named).
- Produces:
  ```rust
  pub struct DriftGate { python: String, run: CommandRunner, timeout: Duration }
  pub enum Comparison { Step, Cumulative }
  pub enum GateOutcome {
      WithinNoise,                 // exit 0
      Drift,                       // exit 1 — the caller decides confirm (Task 4)
      NotComparable { exit: i32 }, // exit 2
      InstrumentChanged { reference: String, current: String }, // precheck fired; diff never ran
      Unmeasured { reason: String }, // reference file absent/unreadable — named, never a pass
      Infra { detail: String },    // spawn failure / timeout / signal-kill
  }
  pub fn compare(&self, reference: &Path, current: &Path) -> GateOutcome;
  fn diff_argv(reference: &Path, current: &Path) -> Vec<String>; // ["-m","assay","diff",ref,cur,"--gate"] — a value tests inspect, like post::argv
  ```
  - `compare` reads BOTH files through `Profile::from_json` first (this is also the identity check — a file that doesn't parse is `Unmeasured` with the parse error named), runs `instrument_precheck`, and only on `Comparable` spawns the diff.
  - Journal `Event::Drift { model: String, comparison: String, outcome: String, reference_path: String, current_path: String, exit_code: Option<i32> }` — `exit_code` None when diff never ran (precheck/unmeasured/infra), per None-vs-zero.

- [ ] **Step 1: Failing tests** — scripted runner (POST's seam) driving exits 0/1/2 → the three outcomes; precheck pair (v8 vs v4 fixtures) → `InstrumentChanged` AND the runner asserts diff was NEVER spawned; absent reference → `Unmeasured`, no spawn; timeout via `run_bounded_for_test` → `Infra`; `diff_argv` pinned exactly.
- [ ] **Step 2: RED.** **Step 3: implement.** **Step 4: GREEN.**
- [ ] **Step 5: Mutation checks** — (a) precheck after spawn instead of before → never-spawned assertion FAILS; (b) exit 2 mapped to `WithinNoise` → its test FAILS; (c) absent reference mapped to `WithinNoise` → its test FAILS (the silent-pass bug this family exists to refuse).
- [ ] **Step 6: Commit** — `feat: the drift gate — precheck-first assay diff subprocess with named outcomes`

### Task 4: Confirm-then-alarm, POST wiring, ModelStatus.drift

**Files:**
- Modify: `crates/bloomery-daemon/src/drift.rs` (orchestration), `crates/bloomery-daemon/src/post.rs` (call site in `run_post` after each successful probe), `crates/bloomery-daemon/src/pager/status.rs` (+ pager plumbing for the new field), `crates/bloomery-daemon/src/pager.rs` (setter mirroring `set_codec_gate`'s shape)
- Test: `crates/bloomery-daemon/tests/drift_test.rs`, existing post/status tests extended

**Interfaces:**
- Consumes: Tasks 1–3; `PostRunner::probe` (the confirm re-probe IS this call — same instrument, fresh out path from `retain_transient`'s naming); `Pager` + `ModelStatus`.
- Produces:
  ```rust
  pub enum DriftStatus { // per comparison, rendered in ModelStatus
      WithinNoise,
      Confirmed { reference: String },   // sha-prefix identity of the reference
      Transient,
      Unconfirmed { reason: String },    // confirm probe itself failed — first reading stands, NAMED
      InstrumentChanged { reference: String, current: String },
      Unmeasured { reason: String },
      NotComparable,
  }
  pub struct ModelDrift { pub step: DriftStatus, pub cumulative: DriftStatus }
  // ModelStatus gains: pub drift: Option<ModelDrift>  // None = drift never ran this boot (posting failed earlier) — absent ≠ clean
  ```
  - Orchestration per model per boot (in `run_post`'s per-model loop, AFTER the existing journal/admit bookkeeping): rotate → probe (existing) → on success: step-compare vs previous, cumulative-compare vs baseline; each `Drift` outcome triggers ONE confirm (fresh probe to a transient path, re-diff same reference); auto-bless when no baseline exists (journaled `auto-first-profile`). Every outcome journals; the pair lands in the pager via the new setter.
  - Confirm probe failure (PostError) → `Unconfirmed` with the PostError string — the first reading is never upgraded to `Confirmed` (spec §4).
- POST semantics untouched: admission, `posting` flag, degradation rows all exactly as before — pin by running the existing post tests unmodified (their assertions must not change; new expectations are new tests).

- [ ] **Step 1: Failing tests** — scripted end-to-end per model: clean boot (both WithinNoise, 2 Drift journal rows); step-drift that confirms (probe called exactly twice for that model, Confirmed in status + 3 Drift rows incl. the confirm); step-drift that doesn't reproduce (Transient, transient file retained); confirm-probe failure (Unconfirmed named); no-baseline first boot (auto-bless journaled, cumulative Unmeasured on THIS boot's compare or WithinNoise-vs-self — pick per spec: auto-bless happens after comparison, so cumulative reads Unmeasured this boot; pin that ordering); status renders both fields with None-honesty.
- [ ] **Step 2: RED.** **Step 3: implement.** **Step 4: GREEN** — including every pre-existing post/status test UNMODIFIED.
- [ ] **Step 5: Mutation checks** — (a) skip the confirm and journal Confirmed on first reading → the confirms-exactly-twice test FAILS; (b) upgrade Unconfirmed to Confirmed on probe failure → its test FAILS; (c) auto-bless before comparison → the ordering test FAILS.
- [ ] **Step 6: Commit** — `feat: confirm-then-alarm drift orchestration wired into POST, drift in ModelStatus`

### Task 5: The bless route

**Files:**
- Modify: `crates/bloomery-daemon/src/api_native.rs`
- Test: the daemon's existing api_native test file (find and extend in its idiom)

**Interfaces:**
- Consumes: `ProfileStore::bless`, `Pager` lock idiom (`lock_pager`), journal.
- Produces: route `("POST", ["models", name, "bless"])` → 200 `{"model", "sha", "path"}` on success; 404 unknown model; 409 with a named error when no current profile exists to bless (never a silent no-op). Journaled `Blessed { provenance: "operator" }`.

- [ ] **Step 1: Failing tests** — success (journal row + baseline file exists + sha matches); unknown model 404; no-current-profile 409 with the reason string; the route table's `_ => 404` still catches everything else (existing test untouched).
- [ ] **Step 2: RED.** **Step 3: implement.** **Step 4: GREEN.**
- [ ] **Step 5: Mutation check** — bless-on-missing-current returning 200 → the 409 test FAILS.
- [ ] **Step 6: Commit** — `feat: POST /models/{name}/bless — operator baseline blessing`

### Task 6: Live acceptance — two real boots (evidence committed)

**Files:**
- Evidence: `docs/superpowers/evidence/2026-08-17-drift-watch-live.md` + journal excerpts + profile shas

**Interfaces:** Consumes everything; produces the spec §10.5 acceptance record.

- [ ] **Step 1: Preflight** — `nvidia-smi` shows the GPU substantially free (< 2 GiB used) and nothing else needs it; daemon built `--features vulkan`; deployment PYTHONPATH points at assay 0.9.0 (`python3 -c "import assay; print(assay.__version__)"` → 0.9.0 — this replaces the old 74c5b71 pin; record how it is set on this box in the evidence doc).
- [ ] **Step 2: Boot 1** (one small model configured, e.g. the 1.5b) — first-ever profile: expect auto-bless journaled, step+cumulative `Unmeasured` (no references), status shows it. Capture journal excerpt.
- [ ] **Step 3: Boot 2** — same config: expect both comparisons run, `WithinNoise` end to end (or whatever the box truthfully measures — record what happens; a Transient here is a finding, not a failure).
- [ ] **Step 4: Instrument-changed path** — swap the PYTHONPATH to the OLD assay pin (74c5b71) for one boot on a scratch data_dir copy of the same references — expect `InstrumentChanged` on both comparisons, diff never spawned (journal exit_code null). Restore the 0.9.0 environment. (If the old pin's assay cannot probe this daemon cleanly, the equivalent test is a doctored-VERSION copy of the reference file on the scratch copy — doctored SCRATCH copy only, clearly named in the evidence, committed originals untouched.)
- [ ] **Step 5: Write the evidence doc** — what ran, journal rows verbatim, profile shas, any surprises recorded-never-tidied. Commit — `docs: drift-watch live acceptance — two boots + instrument-changed path`

---

## Self-review notes (already applied)

- Spec coverage: §2 → Tasks 2–4 (comparisons, blessing, auto-bless ordering pinned); §3 → Tasks 1, 3, 6.4; §4 → Tasks 3–4 (incl. first-diff exit-2, wedged-confirm `Unconfirmed`); §5 → Task 2; §6 → Tasks 1, 6.1; §7 → global constraints + Task 4's unmodified-post-tests rule; §8 → the test steps throughout; §10 order preserved.
- Ambiguity resolved in-plan: auto-bless happens AFTER this boot's comparisons (cumulative reads `Unmeasured` on the first boot) — pinned in Task 4.
- Type consistency: `InstrumentPrecheck` (T1) consumed by T3; `ProfileStore`/`Blessed` (T2) by T3–T5; `GateOutcome` (T3) folded into `DriftStatus` (T4); slug REUSED from POST, asserted.
- Every task ends independently green; no pushes until the wave completes (branch → PR, merge=Brice).
