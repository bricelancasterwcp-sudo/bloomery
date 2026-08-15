# Phase 2b/2c P4 — G4 codec-landing gate Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build G4's instrument — a frozen fixture task set run through the real P3 task loop at admission time — score per-model applies-and-parses landing with Wilson-interval honesty, and enforce the pinned ≥80% gate: pass keeps mutating verbs, fail (or unmeasured) demotes the model to read-only, journaled and surfaced in `/status`.

**Architecture:** A daemon-side codec probe (the umbrella §3 pins "run through the daemon's own task loop — measured at admission time alongside the assay POST"). Boot sequence: assay POST attaches profiles → the probe reads each model's profile-selected patch codec, materializes each fixture into a scratch dir, runs the **real** `run_task` against the model under the pager lock (registry's ratified whole-task-lock pattern), scores landing, journals per-fixture + verdict events, and stores the gate result in the pager. Task creation then resolves per-model codec + verb policy from the pager (closing the carried-debt "Profile has no codec field" item). Everything except the final live run is GPU-free (`FakeSubstrate`).

**Tech Stack:** Rust workspace (bloomery-core / bloomery-daemon), `toml` for the fixture set, existing P1 codec + P2 grants + P3 loop, assay profile documents (the `codecs` grid already ships in `--quick` profiles).

## Global Constraints

- Pre-registration governs (rigorous-experiments): Task 1's protocol commits **before** any instrument code; the point estimate decides; no tune-and-rerun.
- Unmeasured is never zero: an aborted/skipped probe yields **no** `CodecVerdict` event and a fail-closed read-only policy — never a `0/N` score. Infrastructure failure never scores as model failure.
- Every landing record names its lens; the envelope lens name is `bloomery-task-envelope-v1` and the fixture-set name travels in every record.
- `cargo fmt` + `cargo clippy --workspace --all-targets -- -D warnings` clean on BOTH feature sets (default and `--features llama`) before every commit. Tests: `cargo test --workspace` (GPU-free; `python3` must be on PATH for the Python-lens tests — it is on this box, 3.14.4).
- NEVER wrap builds/tests in the `timeout` command (this box's uutils `timeout` segfaults reaping multithreaded children).
- Files ≤800 lines; new pager surface goes in `pager/` submodules (pager.rs is at 780).
- Conventional commits (`feat:`/`test:`/`docs:`), no attribution footers. Commit after every task.
- Mutation-test every load-bearing new test before it counts: break the pinned line, watch the test fail, restore (note the check in the task's report; the gate-decision boundary, the scoring conjunction, and the demotion gate MUST each get this).
- Standing rulings apply: static boot-time VRAM budget; anti-ratchet (self-measured profiles never clamp geometry); whole-task pager lock is ratified v1 debt.

---

### Task 1: G4 protocol pre-registration (docs only — MUST land before any instrument code)

**Files:**
- Create: `docs/superpowers/evidence/2026-08-15-g4-protocol.md`
- Modify: `docs/gates.md` (append a Protocol block under G4 — never touching the pinned ≥80%)

**Interfaces:**
- Consumes: `docs/gates.md` G4 (pinned commitment), spec §6, umbrella §3.
- Produces: the frozen protocol every later task implements; Tasks 5/9/11 cite it by section.

- [ ] **Step 1: Write the protocol doc** with exactly these pinned decisions (each with its one-line rationale; numbers marked *derived* or *chosen+sanity*):

  1. **Subject:** each configured model, on this daemon's own serving path (the same `Pager::infer` `/v1` uses), on the declared tier.
  2. **Instrument:** fixture set `codec-tasks-v1` — N=20 single-defect repair tasks (10 `python` lens, 10 `plaintext` lens), each with a verified reference landing, embedded in the daemon binary. Run through the real `run_task` (P3), envelope lens name `bloomery-task-envelope-v1`. Per fixture: fresh scratch dir, fresh ephemeral agent (default priority, `window_cap = None`, budget 30,000 tokens — *chosen+sanity:* backstop ≈ 6 steps × (prompt ≲4k + completion 1024); steps are the real bound), `max_steps = 6` (*chosen+sanity:* 2× the expected read→patch→done path), grants = read+write on the fixture dir only, **no commands**, `network: false`, patch codec = the model's profile-selected codec (rule 4), `mutating_verbs = true`.
  3. **Scoring rule (per fixture):** the fixture **lands** iff BOTH (a) the task produced at least one `patch` step with `failed == false`, AND (b) the declared target file's final bytes differ from its initial bytes. Recorded edges, accepted: an identity patch (applies, changes nothing) scores NOT landed — a repair fixture's reference always changes bytes, so a byte-identical result is a non-repair; a patch landed only on some other/new file scores NOT landed — the gate licenses real edits, not scratch-file creation. Terminal `Done`/`StepsExhausted`/`BudgetExhausted` are all scored; `TaskStatus::Error` (or agent-creation refusal) is an **infrastructure abort** — the model's whole probe stops, no verdict is recorded, the model is *unmeasured* (fail-closed read-only), and the abort reason is journaled `Degraded`. Never splice a partial score.
  4. **Codec selection rule:** from the attached assay profile's `codecs` grid at grade `"small"` (assay's `_GRADE_FOR_VERDICTS`), comparing `lands_applies`: whole_file strictly greater → `WholeFile`; otherwise `SearchReplace` when it is measured; only one measured → that one; neither measured or no profile → default `SearchReplace` (the robigo-proven default), and the verdict's `detail` says `"default (codecs unmeasured)"`.
  5. **Decision rule:** integer form `landed * 5 >= n * 4` (exactly "landing ≥80%", no float edge) → mutating verbs kept; else demoted to read/find/done. **The point estimate decides.** Wilson 95% interval recorded with every verdict; `provisional = (lo < 0.80 && 0.80 < hi)` — provisional marks the record, it never changes the decision. *Derived sanity (via assay's `wilson95`, the reference implementation):* 20/20 → lo 0.8389 (a decided keep is possible); 12/20 → hi 0.7812 (a decided demote is possible); 16/20 → (0.5840, 0.9193) straddles → provisional keep. N=20 can decide both directions; that is the sample-size justification.
  6. **Enforcement:** demoted/unmeasured models get a read-only verb card AND a structural dispatch refusal (prompting alone is not enforcement). Demotion is per-boot state (the gate runs at admission; restart re-measures), journaled (`CodecVerdict` / `Degraded`) and surfaced in `/status`.
  7. **Honest outcomes:** every local 7B demoted is a valid, pre-registered outcome, not a failure; the black-oxide fine-tune flywheel is the recorded escalation. The gate measures codec landing under the OS envelope — NOT task/repair success.
  8. **Amendment protocol:** identical to `docs/gates.md` — any change is a recorded amendment executed before re-running, never tune-and-rerun.

- [ ] **Step 2: Append to `docs/gates.md` under G4** (below the kill consequence), a dated block: `**Protocol (pre-registered 2026-08-15, before the instrument):** fixture set codec-tasks-v1 (N=20; 10 python + 10 plaintext lenses), run through the daemon's own task loop at admission; landing = applies-and-parses scored per docs/superpowers/evidence/2026-08-15-g4-protocol.md §3; decision landed*5 >= n*4 on the point estimate; Wilson 95% recorded, provisional when the interval straddles 0.80; infrastructure aborts yield unmeasured (fail-closed demotion), never a score.`

- [ ] **Step 3: Commit**: `docs: pre-register G4 codec-landing protocol (before the instrument exists)`

### Task 2: Wilson 95% interval in bloomery-core

**Files:**
- Create: `crates/bloomery-core/src/stats.rs`
- Modify: `crates/bloomery-core/src/lib.rs` (add `pub mod stats;`)
- Test: `crates/bloomery-core/tests/stats_test.rs`

**Interfaces:**
- Produces: `bloomery_core::stats::wilson95(passes: u32, n: u32) -> (f64, f64)` — Task 9 consumes it.

- [ ] **Step 1: Write the failing golden tests** — `wilson95` pinned against assay's reference implementation (`~/workspace/assay/src/assay/profile.py::wilson95`), tolerance 1e-6 per endpoint:

  | passes/n | lo | hi |
  |---|---|---|
  | 35/35 | 0.901099 | 1.000000 |
  | 20/20 | 0.838875 | 1.000000 |
  | 16/20 | 0.583983 | 0.919342 |
  | 15/20 | 0.531299 | 0.888138 |
  | 12/20 | 0.386582 | 0.781193 |
  | 0/20  | 0.000000 | 0.161125 |
  | 10/20 | 0.299298 | 0.700702 |
  | 0/0   | 0.0 | 1.0 (parity with the reference: n=0 → the vacuous interval) |

- [ ] **Step 2: Run to verify FAIL** (`cargo test -p bloomery-core --test stats_test`) — module doesn't exist.
- [ ] **Step 3: Implement** — a direct port of the reference (algorithm verbatim because a fresh implementer would guess a different z or clamping):

```rust
pub fn wilson95(passes: u32, n: u32) -> (f64, f64) {
    if n == 0 {
        return (0.0, 1.0);
    }
    debug_assert!(passes <= n);
    let z = 1.959_963_984_540_054_f64;
    let n_f = f64::from(n);
    let phat = f64::from(passes) / n_f;
    let denom = 1.0 + z * z / n_f;
    let centre = phat + z * z / (2.0 * n_f);
    let margin = z * ((phat * (1.0 - phat) + z * z / (4.0 * n_f)) / n_f).sqrt();
    (
        ((centre - margin) / denom).max(0.0),
        ((centre + margin) / denom).min(1.0),
    )
}
```

  Doc comment must name the source (assay `profile.py::wilson95`, the project's reference implementation) and why the interval exists (a verdict near a threshold SAYS so).
- [ ] **Step 4: Run tests to verify PASS**; fmt + clippy both feature sets.
- [ ] **Step 5: Commit**: `feat: wilson95 interval in bloomery-core (golden-pinned to assay's reference)`

### Task 3: Profile codecs grid + per-model codec selection

**Files:**
- Modify: `crates/bloomery-core/src/profile.rs`
- Test: `crates/bloomery-core/tests/profile_test.rs` (extend)

**Interfaces:**
- Consumes: assay profile JSON — top-level `"codecs"` key, shape `{codec_name: {grade: {"lands": f64|null, "lands_applies": f64|null, "n": u32}}}` (grades `tiny`/`small`/`medium`; codec names `search_replace`/`whole_file`/`json_object`). `--quick` profiles carry it (n_per_cell=5); older/handmade profiles may omit it entirely.
- Produces (Tasks 6/8/9 consume):
  - `pub const VERDICT_GRADE: &str = "small";` — assay's `_GRADE_FOR_VERDICTS`, named so the two cannot silently diverge.
  - `pub struct CodecCell { pub lands: Option<f64>, pub lands_applies: Option<f64>, pub n: u32 }`
  - `Profile::codec_cell(&self, codec: &str) -> Option<CodecCell>` — the cell at `VERDICT_GRADE`, `None` when the grid/codec/grade is absent.
  - `Profile::preferred_patch_codec(&self) -> Option<PatchCodec>` — protocol §4's rule exactly; `None` means "nothing measured, caller defaults SearchReplace".

- [ ] **Step 1: Write failing tests** pinning the selection rule (each case a JSON doc string):
  - wf `lands_applies` 0.9 vs sr 0.6 at `small` → `Some(WholeFile)`
  - sr 0.9 vs wf 0.6 → `Some(SearchReplace)`; tie 0.8/0.8 → `Some(SearchReplace)`
  - only sr measured (wf cell has `lands_applies: null`) → `Some(SearchReplace)`; only wf measured → `Some(WholeFile)`
  - `codecs` key absent → `None`; grid present but `small` grade missing → `None`
  - a cell at `tiny` must NOT influence the choice (wf wins at tiny, sr wins at small → `Some(SearchReplace)`) — this is the falsification test for grade selection
  - existing `from_json`/verdict tests keep passing (backward compat: the new field is `#[serde(default)]` inside `ProfileData`)
- [ ] **Step 2: Run to verify FAIL.**
- [ ] **Step 3: Implement** — extend `ProfileData` with `#[serde(default)] codecs: Option<HashMap<String, HashMap<String, CodecCellData>>>`; `CodecCellData` mirrors `CodecCell` for deserialization. Selection: let `sr`/`wf` = `codec_cell("search_replace"/"whole_file").and_then(|c| c.lands_applies)`; match `(sr, wf)`: `(None, None) → None`, `(Some(_), None) → Some(SearchReplace)`, `(None, Some(_)) → Some(WholeFile)`, `(Some(s), Some(w)) → if w > s { WholeFile } else { SearchReplace }`.
- [ ] **Step 4: Run tests to verify PASS**; fmt + clippy.
- [ ] **Step 5: Commit**: `feat: profile codecs grid + preferred_patch_codec selection (protocol §4)`

### Task 4: Record shapes — journal events + TaskStepRecord.failed

**Files:**
- Modify: `crates/bloomery-core/src/journal.rs` (two new `Event` variants)
- Modify: `crates/bloomery-daemon/src/task/task_loop.rs` (`TaskStepRecord`/`StepReport` gain `failed`)
- Modify: `crates/bloomery-daemon/src/api_task.rs` (GET step JSON — additive, automatic via serde)
- Test: `crates/bloomery-core/tests/journal_test.rs`, `crates/bloomery-daemon/tests/task_loop_test.rs` (extend)

**Interfaces:**
- Produces (wire format verbatim — Tasks 6/9/11 consume; replay tooling reads these forever):

```rust
/// One codec-probe fixture run (G4 instrument). `detail` is the last patch
/// step's outcome, or the terminal status when no patch step ran.
CodecFixture {
    model: String,
    fixture_set: String,
    fixture: String,
    codec: String,        // "search_replace" | "whole_file"
    landed: bool,
    steps: u32,
    detail: String,
},
/// The per-model G4 verdict, emitted exactly once per completed probe
/// (never for an aborted one — unmeasured is not an event, it is the
/// absence of this event plus a Degraded reason).
CodecVerdict {
    model: String,
    fixture_set: String,
    codec: String,
    landed: u32,
    n: u32,
    interval95: [f64; 2],
    provisional: bool,
    mutating_verbs: bool,
    detail: String,       // names the lens: "applies_and_parses under bloomery-task-envelope-v1" (+ codec-selection provenance)
},
```

- `TaskStepRecord` gains `pub failed: bool`; `StepReport` gains `failed: &bool`-equivalent (`failed: bool`); `record_step` threads it. Call-site values: parse-failure steps `true`; `done` step `false`; executor steps `obs.failed`.

- [ ] **Step 1: Write failing tests:** journal round-trip for both new variants (serialize → replay → equality); the existing committed-journal compat pin (`journal_test.rs`) must keep passing untouched — run it explicitly and say so in the report. Task-loop test: a grant-violating scripted turn yields a step with `failed == true`; a clean read step yields `failed == false`.
- [ ] **Step 2: Run to verify FAIL** (missing variants / missing field).
- [ ] **Step 3: Implement** — add the variants; thread `failed` through `record_step`'s three call sites in `run_task`/`propose_action`. GET `/task` steps now carry `"failed"` automatically (serde on `TaskStepRecord`) — extend one `api_task_test.rs` assertion to pin it.
- [ ] **Step 4: Run the full workspace suite** (registry/api tests compile against the new field); fmt + clippy.
- [ ] **Step 5: Commit**: `feat: CodecFixture/CodecVerdict journal events + TaskStepRecord.failed`

### Task 5: The frozen fixture set `codec-tasks-v1` + parser + structural validation

**Files:**
- Create: `crates/bloomery-daemon/fixtures/codec-tasks-v1.toml`
- Create: `crates/bloomery-daemon/src/codec_probe/mod.rs` (module skeleton: `pub mod fixtures;`)
- Create: `crates/bloomery-daemon/src/codec_probe/fixtures.rs`
- Modify: `crates/bloomery-daemon/src/lib.rs` (add `pub mod codec_probe;`)
- Test: `crates/bloomery-daemon/tests/codec_fixtures_test.rs`

**Interfaces:**
- Produces (Task 9 consumes):
  - `pub struct FixtureSet { pub set: String, pub fixtures: Vec<Fixture> }`
  - `pub struct Fixture { pub name: String, pub lens: String, pub target: String, pub goal: String, pub files: Vec<FixtureFile>, pub reference: Reference }` with `FixtureFile { path: String, contents: String }`, `Reference { search: String, replace: String }`
  - `pub fn parse_fixture_set(toml_text: &str) -> Result<FixtureSet, String>`
  - `pub fn shipped_fixture_set() -> Result<FixtureSet, String>` — parses `include_str!("../../fixtures/codec-tasks-v1.toml")`.

**Wire format (verbatim — the one thing an implementer must not improvise):**

```toml
set = "codec-tasks-v1"

[[fixture]]
name = "py-mean-off-by-one"
lens = "python"
target = "stats.py"
goal = """stats.py's mean() divides by len(values) + 1, so mean([2, 4]) returns 2.0 instead of 3.0. Fix mean() in stats.py so it divides by len(values). Patch the file, then emit done."""

[[fixture.file]]
path = "stats.py"
contents = """
def mean(values):
    total = 0
    for v in values:
        total += v
    return total / (len(values) + 1)
"""

[fixture.reference]
search = "    return total / (len(values) + 1)"
replace = "    return total / len(values)"
```

and a plaintext example:

```toml
[[fixture]]
name = "txt-listen-port-mismatch"
lens = "plaintext"
target = "serve.conf"
goal = """serve.conf's listen_port says 8080, but every other reference in the file uses 8181 — the daemon comes up on the wrong port. Change the listen_port line in serve.conf to 8181. Patch the file, then emit done."""

[[fixture.file]]
path = "serve.conf"
contents = """
listen_addr = 127.0.0.1
listen_port = 8080
health_path = /status
upstream = http://127.0.0.1:8181
"""

[fixture.reference]
search = "listen_port = 8080"
replace = "listen_port = 8181"
```

**Authoring requirements for the remaining 18 fixtures** (the structural test enforces the mechanical subset; the reviewer checks the rest):
- 10 `python` (`py-` prefix) + 10 `plaintext` (`txt-` prefix), unique names.
- One planted defect each, robigo shape: the goal states the failing symptom AND names the target file AND ends with the patch-then-done instruction.
- Target files 5–60 lines. Reference = one contiguous-region change, `search` matching the file exactly once (indentation included).
  [Recorded at final review: the mandated serve.conf example itself has 4 content lines; the 5–60 guidance governed the other 18 fixtures — plan-text inconsistency, protocol unaffected.]
- ≥3 python fixtures with indentation-sensitive `search` lines (leading spaces — the robigo-measured 7B failure mode).
- ≥2 python fixtures include a second distractor file the goal's symptom references (forces a `read` before the patch).
- Plaintext fixtures span config-file and prose shapes.
- TOML gotcha to state in a file comment: `"""` strips only the newline right after the opening delimiter; the reference `search` must match file bytes exactly — prefer single-line basic strings for one-line references.

- [ ] **Step 1: Write the failing structural-validation test** (`codec_fixtures_test.rs`, the seconds-fast authored-artifact check rigorous-experiments requires) asserting on `shipped_fixture_set()`:
  - parses; `set == "codec-tasks-v1"`; exactly 20 fixtures; 10 per lens; unique names; every `lens` ∈ {python, plaintext}; every `target` appears among that fixture's `files`; every goal contains its target filename and is non-empty; `search != replace`.
  - **Reference-landing verification via the REAL instrument:** for every fixture, `bloomery_core::action::lens::land(initial_target_contents, &PatchBody::SearchReplace{search, replace}, lens)` with the real lens for the fixture's `lens` field (`python` → the daemon's `lens_py` Python lens, which shells `python3`; `plaintext` → `PlainText`) returns `Landing::Lands` — a fixture whose verified fix doesn't land can never be scored honestly.
  - Applying the reference changes bytes (guaranteed by `search != replace` + exactly-one-match; assert anyway — it is the scoring rule's (b) leg).
- [ ] **Step 2: Run to verify FAIL** (no module, no file).
- [ ] **Step 3: Implement parser + author all 20 fixtures.** Parser errors are named (`"fixture 7 (py-...): target 'x.py' not among files"`). Keep `fixtures.rs` under 200 lines; the TOML carries the bulk.
- [ ] **Step 4: Run tests to verify PASS**; fmt + clippy.
- [ ] **Step 5: Commit**: `feat: frozen G4 fixture set codec-tasks-v1 (N=20) + parser + structural validation`

### Task 6: Pager codec-gate state + `/status` surfacing

**Files:**
- Create: `crates/bloomery-daemon/src/pager/codec_gate.rs` (impl-block submodule, `status.rs` pattern)
- Modify: `crates/bloomery-daemon/src/pager.rs` (declare module; `ModelEntry` gains `codec_gate: Option<CodecGateResult>`)
- Modify: `crates/bloomery-daemon/src/pager/status.rs` (`ModelStatus` additions)
- Test: `crates/bloomery-daemon/tests/pager_codec_gate_test.rs`

**Interfaces:**
- Produces (Tasks 8/9/10 consume):

```rust
#[derive(Debug, Clone, serde::Serialize)]
pub struct CodecGateResult {
    pub fixture_set: String,
    pub codec: PatchCodec,
    pub landed: u32,
    pub n: u32,
    pub interval95: (f64, f64),
    pub provisional: bool,
    pub mutating_verbs: bool,
}
```

  - `Pager::set_codec_gate(&mut self, model: &str, gate: CodecGateResult) -> Result<(), PagerError>` (`UnknownModel` on a bad name)
  - `Pager::model_mutating_verbs(&self, model: &str) -> bool` — `true` iff a gate is stored AND `gate.mutating_verbs`; **`false` for unmeasured** (fail-closed, protocol §3/§6)
  - `Pager::model_patch_codec(&self, model: &str) -> PatchCodec` — `profile.preferred_patch_codec()` else `SearchReplace` (also for unknown/unprofiled models)
  - `Pager::agent_task_policy(&self, agent_id: &str) -> Option<(PatchCodec, bool)>` — `(model_patch_codec, model_mutating_verbs)` for the agent's model; `None` for an unknown agent
  - `Pager::journal_codec_fixture(...)` + `Pager::journal_codec_verdict(...)` — thin single-writer wrappers appending the Task 4 events (same rule as `journal_post`: the probe runs outside the pager but never opens a second journal writer)
  - `ModelStatus` gains: `pub patch_codec: &'static str` ("search_replace"/"whole_file" — the value tasks will actually use), `pub mutating_verbs: bool` (the enforced value), `pub codec_gate: Option<CodecGateStatus>` where `CodecGateStatus { fixture_set: String, codec: &'static str, landed: u32, n: u32, interval95: [f64; 2], provisional: bool }` — `None` = unmeasured, rendered as JSON `null`, never zeros.

- [ ] **Step 1: Write failing tests:** unmeasured model → `model_mutating_verbs == false` AND status `codec_gate: null` + `mutating_verbs: false`; stored keep-gate → `true` + populated `codec_gate`; stored demote-gate → `false`; `set_codec_gate` on unknown model → `UnknownModel`; `model_patch_codec` follows an attached profile's selection (build a profile JSON with a wf-wins grid) and defaults `SearchReplace` unprofiled; `agent_task_policy` resolves through a created agent and is `None` for `"nope"`. Mutation check to run at review: flip the `false`-for-unmeasured line — the first test must fail.
- [ ] **Step 2: Run to verify FAIL.**
- [ ] **Step 3: Implement.** Keep `pager.rs` edits to the field + module declaration; logic lives in `codec_gate.rs`.
- [ ] **Step 4: Run tests to verify PASS**; fmt + clippy both feature sets.
- [ ] **Step 5: Commit**: `feat: pager codec-gate state, fail-closed verb policy, /status surfacing`

### Task 7: Read-only verb card + structural demotion enforcement in the loop

**Files:**
- Modify: `crates/bloomery-core/src/action/card.rs`
- Modify: `crates/bloomery-daemon/src/task/task_loop.rs` (`TaskSpec` gains `mutating_verbs: bool`; gate before dispatch)
- Modify: `crates/bloomery-daemon/src/task/registry.rs` + `crates/bloomery-daemon/src/api_task.rs` + `crates/bloomery-daemon/src/test_support.rs` (only as needed to set `mutating_verbs: true` so this task compiles standalone; Task 8 wires the real value)
- Test: `crates/bloomery-core/tests/action_card_test.rs` (new), `crates/bloomery-daemon/tests/task_loop_test.rs` (extend)

**Interfaces:**
- Produces:
  - `pub fn verb_card_for(patch_codec: PatchCodec, mutating: bool) -> String`; existing `verb_card(c)` becomes `verb_card_for(c, true)` (call sites unchanged). The read-only card contains ONLY the read/find/done sections plus the pinned line: `patch and run are not available in this task (this model is read-only under gate G4)`.
  - `TaskSpec.mutating_verbs: bool`; `render_prompt` uses `verb_card_for(spec.patch_codec, spec.mutating_verbs)`.
  - The pinned refusal outcome string (Task 9's scoring and the journal read it): `verb unavailable: mutating verbs demoted (gate G4)` — recorded as a step with the action's real verb (`"patch"`/`"run"`), `failed: true`, content = the same string, and the loop **continues** (a refused verb is a failed step, not a dead task).

- [ ] **Step 1: Write failing tests:**
  - card: `verb_card_for(SearchReplace, false)` contains "read" / "find" / "done" sections, does NOT contain `verb="patch"` or `verb="run"` examples, and contains the pinned G4 line; `verb_card_for(c, true)` == `verb_card(c)`.
  - loop (FakeSubstrate, existing `task_loop_test.rs` harness): spec with `mutating_verbs: false`, scripted turns = a valid patch action then a done — the patch step records the pinned outcome with `failed: true`, the target file is untouched, the task still terminates `Done`; a `run` action gets the same refusal; a `read` action still executes normally under a demoted spec.
- [ ] **Step 2: Run to verify FAIL.**
- [ ] **Step 3: Implement.** The gate sits in `run_task` after the `Done` branch, before `execute_action` — `execute_action` stays pure dispatch. Mutation check at review: invert the gate condition; the file-untouched test must fail.
- [ ] **Step 4: Run the full workspace suite**; fmt + clippy.
- [ ] **Step 5: Commit**: `feat: read-only verb card + structural G4 demotion enforcement in run_task`

### Task 8: Task creation resolves per-model codec + verb policy (closes carried-debt)

**Files:**
- Modify: `crates/bloomery-daemon/src/api_task.rs` (`create_task`)
- Modify: `crates/bloomery-daemon/src/task/registry.rs` tests only (real values now flow)
- Test: `crates/bloomery-daemon/tests/api_task_test.rs` (extend)

**Interfaces:**
- Consumes: `Pager::agent_task_policy` (Task 6), `TaskSpec.mutating_verbs` (Task 7).
- Produces: `create_task` builds `TaskSpec { patch_codec, mutating_verbs, .. }` from `agent_task_policy(agent_id)` — fetched in the same lock section as `agent_budget_granted` (one lock, both reads). The `PatchCodec::SearchReplace` literal and its "no codec field exists today" comment at `api_task.rs:184-190` are deleted.

- [ ] **Step 1: Write failing tests** (existing api_task HTTP harness): (a) a model with an attached wf-wins profile → the spawned task's verb card shows the WholeFile patch example (observable via the FakeSubstrate's recorded prompt, the harness's existing seam); (b) an unmeasured model → task is created (202) but a scripted patch turn records the pinned G4 refusal (fail-closed default reaches the loop); (c) a stored keep-gate → the patch turn executes.
- [ ] **Step 2: Run to verify FAIL.**
- [ ] **Step 3: Implement**; update registry test literals to exercise both policies rather than hardcoding.
- [ ] **Step 4: Run the full workspace suite**; fmt + clippy.
- [ ] **Step 5: Commit**: `feat: task creation resolves per-model patch codec + G4 verb policy (closes carried-debt item)`

### Task 9: The codec probe engine

**Files:**
- Modify: `crates/bloomery-daemon/src/codec_probe/mod.rs` (the engine)
- Test: `crates/bloomery-daemon/tests/codec_probe_test.rs`

**Interfaces:**
- Consumes: `run_task` (P3), `FixtureSet` (Task 5), `wilson95` (Task 2), pager gate state + journal wrappers (Task 6), `TaskSpec.mutating_verbs` (Task 7).
- Produces (Task 10 consumes):

```rust
pub struct ProbeAborted { pub reason: String }

pub const FIXTURE_BUDGET_TOKENS: u64 = 30_000;
pub const FIXTURE_MAX_STEPS: u32 = 6;
pub const ENVELOPE_LENS: &str = "bloomery-task-envelope-v1";

/// Pure decision + provisional helpers, extracted for direct testing.
pub fn gate_decision(landed: u32, n: u32) -> bool;          // landed * 5 >= n * 4
pub fn is_provisional(lo: f64, hi: f64) -> bool;            // lo < 0.80 && 0.80 < hi

pub fn run_codec_probe<S: Substrate + Send + 'static>(
    pager: &Mutex<Pager<S>>,
    model: &str,
    set: &FixtureSet,
    scratch_dir: &Path,
) -> Result<CodecGateResult, ProbeAborted>
```

**Behavior invariants (each numbered item gets a test or is covered by one below):**
1. Reads the model's selected codec ONCE (`model_patch_codec`) before any fixture; the same codec goes into every `TaskSpec`, every `CodecFixture` event, and the verdict.
2. Per fixture, in set order: materialize under `scratch_dir/<model with '/' and ':' mapped to '-'>/<fixture.name>/` (remove the dir first if present — deterministic per boot, left in place after for inspection); capture the target file's initial bytes; lock the pager (poisoned lock → `ProbeAborted`); `create_agent(model, default_priority(), None, FIXTURE_BUDGET_TOKENS)`; build `TaskSpec { goal, grant: read+write roots = [fixture dir], commands: [], network: false, budget_tokens: FIXTURE_BUDGET_TOKENS, max_steps: FIXTURE_MAX_STEPS, cwd: fixture dir, patch_codec: selected, bounds: pager.exec_bounds(), mutating_verbs: true }`; open an own `Journal` handle on `task_journal_path` (registry's pattern — safe: the probe holds the pager lock across `run_task`, so there is never a concurrent tasks.jsonl writer); `run_task`; `remove_agent(id, "codec probe fixture complete")` (best-effort on the abort path too).
3. Scoring (protocol §3, exactly): `landed = (∃ step: step.verb == "patch" && !step.failed) && (final target bytes != initial target bytes)`. `Done`/`StepsExhausted`/`BudgetExhausted` are scored; `TaskStatus::Error` or a `create_agent` refusal → `Err(ProbeAborted { reason })` naming the fixture — no verdict, no partial score.
4. After each scored fixture: `journal_codec_fixture(model, set.set, fixture.name, codec, landed, steps, detail)` where `detail` = the last patch step's outcome, else the terminal status.
5. After all fixtures: `wilson95(landed, n)`; `gate_decision`; `is_provisional`; `journal_codec_verdict(...)` with `detail` = `"applies_and_parses under bloomery-task-envelope-v1"` plus the codec-selection provenance (`"codec from profile"` / `"default (codecs unmeasured)"`); `set_codec_gate`; return the result.
6. No fixture state leaks into another: fresh dir, fresh agent, fresh journal handle.

- [ ] **Step 1: Write failing tests** (FakeSubstrate + a small inline 2-fixture set via `parse_fixture_set` with `set = "codec-tasks-test"` — the engine is generic over set size; the shipped N=20 is Task 5's concern):
  - all-land script (scripted correct patch turn + done per fixture) → `Ok`, `landed == 2`, `mutating_verbs == true`, journal contains 2 `CodecFixture` + exactly 1 `CodecVerdict`, pager `model_mutating_verbs == true`, both `AgentRemoved` present.
  - never-patches script (immediate done) → `landed == 0`, demoted, pager `false`.
  - **identity-patch script** (search == file line, replace = byte-identical content via a whole-file body equal to current) → scores NOT landed (pins scoring leg (b)).
  - **scratch-file script** (patch lands on a NEW file in the granted dir, then done) → scores NOT landed (pins the target-file condition).
  - substrate-error script (FakeSubstrate scripted failure) → `Err(ProbeAborted)`, NO `CodecVerdict` in the journal, pager gate stays unmeasured (`model_mutating_verbs == false`).
  - pure-fn boundaries: `gate_decision(16, 20) == true`, `gate_decision(15, 20) == false`, `gate_decision(4, 5) == true`, `gate_decision(3, 5) == false`; `is_provisional(0.5840, 0.9193) == true`, `is_provisional(0.8389, 1.0) == false`, `is_provisional(0.3866, 0.7812) == false`.
- [ ] **Step 2: Run to verify FAIL.**
- [ ] **Step 3: Implement.** Mutation checks at review: `>=` → `>` in `gate_decision` (the 16/20 test must fail); drop scoring leg (b) (the identity-patch test must fail); drop the target-file comparison (the scratch-file test must fail).
- [ ] **Step 4: Run the full workspace suite**; fmt + clippy both feature sets.
- [ ] **Step 5: Commit**: `feat: G4 codec probe engine — fixtures through the real task loop, fail-closed verdicts`

### Task 10: Boot wiring + operator docs + carried-debt bookkeeping

**Files:**
- Modify: `crates/bloomery-daemon/src/main.rs` (POST thread)
- Modify: `README.md` (honest limits + config docs)
- Modify: `docs/CARRIED-DEBT.md`
- Test: `crates/bloomery-daemon/tests/post_test.rs` or a new `codec_probe_boot_test.rs` if the seam fits better

**Interfaces:**
- Consumes: `run_codec_probe`, `shipped_fixture_set` (Task 5).
- Produces: the boot behavior —
  - Probe runs only when `config.assay.enabled && config.tasks_enabled`, inside the existing POST thread, strictly AFTER `run_post` returns Ok (profiles attached, posting cleared). Scratch dir = `data_dir/codec-probe`.
  - `run_post` Err (journal failure) → probe does not run (the daemon is already degraded-loudly).
  - Fixture-set parse failure (a daemon build bug) → `journal_degraded("codec fixture set unparseable: {e}; codec probe skipped — mutating verbs stay refused")`, no probe.
  - Per-model `ProbeAborted` → `journal_degraded("codec probe aborted for {model}: {reason}; unmeasured — mutating verbs refused")`, continue to the next model (one model's abort never stops another's probe — the POST rule).
  - `tasks_enabled && !assay.enabled` → one `journal_degraded("codec probe skipped: POST disabled; all models unmeasured — mutating verbs refused")` beside the existing POST-disabled line.
  - `!tasks_enabled` → no probe, no journal line (the surface is dark; `/status`'s `mutating_verbs: false` + `codec_gate: null` already tell the truth).

- [ ] **Step 1: Write failing tests** for the decision table above at whatever seam is testable without a GPU (extract a `should_run_codec_probe(assay_enabled, tasks_enabled) -> bool` + the skip-reason strings into `codec_probe` and pin those; `main.rs` stays thin glue, consistent with how POST's argv is tested via `argv()`).
- [ ] **Step 2: Run to verify FAIL.**
- [ ] **Step 3: Implement main.rs wiring + docs:**
  - README: extend the task-surface section — G4 gate semantics, fail-closed unmeasured default, per-boot measurement (boots-only, same honest limit as POST item 6), the ~N×steps GPU cost of boot with `tasks_enabled`, `/status` fields.
  - CARRIED-DEBT: strike (never delete) the "Profile has NO codec field" ruling with its delivery note; append new recorded items: (a) codec probe is boots-only (extends item 6); (b) scoring edges pinned by protocol §3 (identity patch / scratch-file — recorded, accepted); (c) the probe holds the whole-task pager lock per fixture, extending the ratified v1 lock debt — boot with `tasks_enabled` serializes that much longer; (d) demotion is per-boot state, re-measured at every boot, not persisted.
- [ ] **Step 4: Run the full workspace suite**; fmt + clippy both feature sets.
- [ ] **Step 5: Commit**: `feat: boot-time G4 codec probe wiring + honest-limits docs + carried-debt bookkeeping`

### Task 11: Live G4 measurement + evidence doc (MAIN SESSION — needs the GPU; not a subagent task)

**Files:**
- Create: `docs/superpowers/evidence/2026-08-15-g4-codec-landing.md`
- Create: `docs/superpowers/evidence/2026-08-15-g4-boot-journal.jsonl` (+ the tasks.jsonl slice if useful)

**Interfaces:**
- Consumes: everything above, merged and green; the protocol (Task 1) governs — re-read it BEFORE running.

- [ ] **Step 1: Preflight (rigorous-experiments §5; box gotchas):** GPU is shared — check `nvidia-smi` for in-flight runs; NEVER kill an in-flight pre-registered run; if busy, use the harness-tracked waiter pattern from the 2a session. Require free VRAM ≥ 12 GiB (qwen2.5-coder-7b-q8_0 weights ~8.1 GiB + ctx + overhead) before starting. Build `--features llama,vulkan` (~2 min incremental; do NOT wrap in `timeout`). Verify the model path is the ollama blob the profile subject pins (`ollama show qwen2.5-coder:7b-instruct-q8_0 --modelfile` FROM line) — served identity, not liveness.
- [ ] **Step 2: Run:** config with `tasks_enabled = true`, `assay.enabled = true`, tier `enthusiast-16gb` / `emulated = false`, the pinned model. Boot; wait for the `CodecVerdict` line in the journal (or a `Degraded` abort). Capture `/status`.
- [ ] **Step 3: Score check:** recompute the landing rate from the `CodecFixture` events alone (`grep '"CodecFixture"' journal | count landed:true / total`) and confirm it equals the verdict event's `landed/n` — the recomputability obligation. Any mismatch is an instrument bug: STOP, fix, and note that no measurement was consumed (an infrastructure kill, cleanly rerunnable from zero).
- [ ] **Step 4: Write the evidence doc:** subject (model, quant, tier, box), instrument fully named (fixture set `codec-tasks-v1`, envelope lens `bloomery-task-envelope-v1`, selected codec + its provenance, sampler/temperature as pinned by the substrate config — state the actual value), per-fixture table (name, lens, landed, steps, detail), the verdict with Wilson interval + provisional flag, the decision applied (per protocol: the point estimate decides), caveats (boots-only; page-cache state irrelevant here but GPU co-tenancy stated; N=20 granularity), and the pre-registered honest outcome text if demotion happened. Commit the journal(s) beside it.
- [ ] **Step 5: Commit**: `docs: G4 live codec-landing measurement — evidence + journals`

---

## Self-Review (performed while writing)

- **Spec §6 coverage:** frozen fixture set with set-name-in-every-record → Tasks 5/4/9; real-task-loop probe at admission per model → Tasks 9/10; ≥80% keep / demote to read-only, journaled + `/status` → Tasks 6/7/9; Wilson + provisional (assay v1.3 discipline) → Tasks 2/9; live-measured with own evidence doc + honest all-demoted outcome → Tasks 1/11; per-model codec selection from the assay profile ("which one a model is offered is P4's verdict") → Tasks 3/8. Carried-debt "Profile has NO codec field" → Tasks 3/8/10.
- **Type consistency:** `CodecGateResult` (Task 6) is what Task 9 returns and status renders; `TaskStepRecord.failed` (Task 4) is what Task 9's scoring and Task 7's refusal write; `verb_card_for` naming consistent across Tasks 7/8; `parse_fixture_set`/`shipped_fixture_set` consistent across Tasks 5/9/10.
- **Order:** 1 (protocol) strictly first; 2/3/4/5 independent after it; 6 needs 3+4; 7 needs 4; 8 needs 6+7; 9 needs 2+4+5+6+7; 10 needs 9; 11 last.
