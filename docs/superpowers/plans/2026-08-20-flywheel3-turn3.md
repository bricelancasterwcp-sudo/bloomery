# Flywheel Turn 3 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Train and gate `qwen3-14b-flywheel3` — symptom-mismatch refusal, find/run trajectories, and the decided G5 on a new frozen `codec-tasks-v3-mixed`.

**Architecture:** The corpus factory is Python (`tools/flywheel/`), the trajectory renderer is the Rust `flywheel-tool` binary (real-execution observations), the gate runs through the daemon's own codec probe at boot. Turn 3 extends all three: a third refusal family and two new repair-trajectory shapes in the factory, `find`/`run` rendering in the tool, and three *named* instrument deltas in the probe (fixture `commands`, v3 set plumbing, boot call-site swap). GPU steps (baselines, training, battery) are human-gated.

**Tech Stack:** Rust (workspace crates), Python 3.14 stdlib + pytest (`tools/flywheel/`), unsloth QLoRA (out-of-repo venv `~/flywheel-venv`), llama.cpp GGUF.

**Spec:** `docs/superpowers/specs/2026-08-20-flywheel3-turn3-design.md` (approved; Task 1 adds its §4 dated amendment)

## Global Constraints

- Frozen sets are UNTOUCHED: `crates/bloomery-daemon/fixtures/codec-tasks-v1.toml` and `codec-tasks-v2-mixed.toml` — byte-frozen, never regenerated, never edited.
- `gates.md` values change only by dated amendment appended below the original (precedent: `docs/gates.md:33`).
- Line ceilings: 800 for Rust `src/`; the Python factory's own stated cap is 400/module. `flywheel_tool_test.rs` is at 788 — Task 6 splits it BEFORE adding tests.
- Test commands: `cargo test -p bloomery-core -p bloomery-daemon` (Rust) and `python3 -m pytest tools/flywheel/tests/ -q` (factory). fmt + clippy clean; only the pre-existing `drift_test` clippy warning is tolerated.
- NEVER use the `timeout` wrapper command on this box (uutils segfault). Featured build LAST before any live boot: `cargo build --release -p bloomery-daemon --features vulkan`.
- Seeds, pre-registered here: corpus seed **20260820**; v3-mixed gate generating seed **8200820** (distinct from 8160816 / 20260816 / 20260817); `train.py` training seeds stay **20260816** unchanged (procedure identity across turns — the corpus is what refreshes; recorded in the prereg doc).
- Pass/kill (spec §5): G4 ≥16/20 on v1; G5 ≥13/16 per class on v3-mixed; kill = G4 <16/20 OR refuse <8/16. find/run usage = secondary endpoints, never kill.
- GPU tasks (9, 11, 12) are HUMAN-GATED: stop and get Brice's explicit go before each.

---

### Task 1: Docs first — gates.md amendment, G5-v3 protocol, spec §4 amendment

**Files:**
- Modify: `docs/gates.md` (G5 section, lines 55–63 — append below, never edit)
- Create: `docs/superpowers/evidence/2026-08-20-g5v3-protocol.md`
- Modify: `docs/superpowers/specs/2026-08-20-flywheel3-turn3-design.md` (§4 dated amendment)

**Interfaces:**
- Consumes: nothing (docs only, before any instrument code — house rule).
- Produces: the pinned commitment every later task argues from.

- [ ] **Step 1: gates.md G5 amendment** — append below the G5 kill-consequence line, imitating the `gates.md:33` clarification style:

```markdown
**Amendment (2026-08-20, recorded before the v3 instrument exists):** the
commitment for a **decided** G5 pass is ≥13/16 per class on fixture set
codec-tasks-v3-mixed (16 `expect="patch"` + 16 `expect="refuse"`; n=16
clears the provisional flag by construction at the 0.80 threshold);
scoring per docs/superpowers/evidence/2026-08-20-g5v3-protocol.md.
codec-tasks-v2-mixed remains the recorded turn-2 instrument, frozen and
unamended. Floors stay per-class, never blended; advisory posture
unchanged.
```

- [ ] **Step 2: Write `2026-08-20-g5v3-protocol.md`** — modeled on `2026-08-16-g5-protocol.md`, stating: scoring identical to the v2 protocol §2 (the refuse trio and the §3 conjunction, verbatim references, no new scoring); composition pinned (refuse 6 defect-absent + 5 missing-target + 5 symptom-mismatch; patch 6 multi-file find-shaped + 5 run-granted single-file + 5 plain); the class floor is the only pass/fail line, per-family and per-shape counts are reported secondary endpoints with these denominators; secondary endpoints (find-usage on the 6 find-shaped, run-before-done on the 5 run-granted, per-family refuse breakdown) computed from `TaskStep` journal rows; two pre-registered measurement risks: `FIXTURE_MAX_STEPS = 6` leaves 4-step ideals only 2 spare turns (vs 3 for 3-step ideals), and `exec_find` observations embed absolute canonicalized paths so trained find observations are **format**-faithful, not byte-identical across contexts (`exec.rs:450` embeds `path.display()` after canonicalize).
- [ ] **Step 3: Spec §4 dated amendment** — append below §4's original text:

```markdown
> **Amendment (2026-08-20, recorded at planning, before implementation):**
> the "expected nil" claim above did not survive the planning survey; per
> this section's own escape clause the three gaps found are named scope:
> (1) `Fixture` carries no command grants — `fixture_grant`
> (`codec_probe/mod.rs:483-504`) hardcodes `"commands": []`, so a
> run-granted fixture is inexpressible without a new optional field;
> (2) G5's set selection is hardcoded — `boot.rs:203` calls
> `shipped_fixture_set_v2_mixed()` literally, so running v3 requires
> `shipped_fixture_set_v3_mixed()` plus the call-site swap and a v3
> placeholder guard; (3) `flywheel-tool`'s wire shape (`TrajectoryRequest`)
> carries a single target and no argv, so multi-file and run trajectories
> need additive wire fields and handlers. Also recorded: §2's "byte-faithful"
> for `find` means format-faithful (absolute-path embedding, see the v3
> protocol doc), and the sibling-file contamination fast-follow is a
> correctness precondition for multi-file tasks (sibling contents enter
> rendered pairs), not post-hoc hygiene.
```

- [ ] **Step 4: Commit** — `docs: pin the decided G5 (v3 amendment + protocol) and name turn-3's instrument deltas`

### Task 2: Instrument delta 1 — fixture `commands` + grant threading

**Files:**
- Modify: `crates/bloomery-daemon/src/codec_probe/fixtures.rs` (struct `Fixture` at :50-69, inline `mod tests`)
- Modify: `crates/bloomery-daemon/src/codec_probe/mod.rs` (`fixture_grant` at :483-504)
- Create: `crates/bloomery-daemon/tests/codec_probe_grant_test.rs` (NEW test home — `codec_probe_test.rs` is at 1559, over ceiling, do not grow it)

**Interfaces:**
- Consumes: `Grant::from_json` accepting `"commands": [["python3","-m","py_compile"]]` (argv-prefix list, `grant/command.rs:18-35`).
- Produces: `Fixture.commands: Vec<Vec<String>>` (serde default empty); `fixture_grant(dir, &fixture)` threading them. Task 8's run-granted fixtures write `commands = [["python3","-m","py_compile"]]` in TOML.

- [ ] **Step 1: Failing tests** — in `fixtures.rs` `mod tests`: a fixture TOML with `commands = [["python3", "-m", "py_compile"]]` parses into `commands`, and a fixture without the key parses to empty (compat: v1/v2 unchanged). In `codec_probe_grant_test.rs`: `fixture_grant` with empty commands produces a grant refusing every argv (today's behavior, pinned), and with a prefix produces a grant under which `check_command` accepts `["python3","-m","py_compile","x.py"]` and refuses `["rm","-rf"]`.
- [ ] **Step 2: Run; verify both new parse tests fail** (`commands` field unknown), grant tests fail to compile (signature).
- [ ] **Step 3: Implement** — `#[serde(default)] pub commands: Vec<Vec<String>>` on `Fixture`; `fixture_grant` builds the JSON via `serde_json::json!` with `"commands": fixture.commands` (signature gains `&Fixture` or the commands slice — implementer's call, minimal diff).
- [ ] **Step 4: Full Rust suite green; mutation check** — hand-mutate the threading back to `[]`, the grant test must fail; restore, green.
- [ ] **Step 5: Commit** — `feat: fixtures carry command grants for run-granted gate fixtures`

### Task 3: Instrument delta 2 — v3 set plumbing + boot swap (placeholder era)

**Files:**
- Create: `crates/bloomery-daemon/fixtures/codec-tasks-v3-mixed.toml` (PLACEHOLDER: `set = "codec-tasks-v3-mixed-PLACEHOLDER"`, one dummy patch + one dummy refuse fixture, header saying Task 8 replaces it)
- Modify: `crates/bloomery-daemon/src/codec_probe/fixtures.rs` (beside `shipped_fixture_set_v2_mixed()` at :209-211 and `V2_MIXED_PLACEHOLDER_SET_NAME` at :190)
- Modify: `crates/bloomery-daemon/src/codec_probe/boot.rs` (:203 call site, :207-209 guard)
- Modify: `crates/bloomery-daemon/tests/codec_probe_test.rs` (ONLY the two boot-wiring pins: `run_boot_g5_probe_runs_the_real_shipped_set_not_a_placeholder_skip` :1479 and `g5_placeholder_skip_reason_wording_is_pinned` :1539 — they now assert the v3 placeholder-skip behavior; flip back in Task 8)

**Interfaces:**
- Produces: `pub fn shipped_fixture_set_v3_mixed() -> Result<FixtureSet, String>` (`include_str!("../../fixtures/codec-tasks-v3-mixed.toml")`); `pub const V3_MIXED_PLACEHOLDER_SET_NAME: &str = "codec-tasks-v3-mixed-PLACEHOLDER";`. `run_boot_g5_probe` loads v3 and skips-with-journal while the placeholder name matches (v2's own placeholder-era mechanism, reused).
- `shipped_fixture_set_v2_mixed()` and its tests stay byte-untouched.

- [ ] **Step 1: Failing test** — boot G5 with the placeholder set journals the skip (adapt the two named pins to the v3 name); `shipped_fixture_set_v3_mixed()` parses.
- [ ] **Step 2: Verify fails** (no such function / boot still loads v2).
- [ ] **Step 3: Implement** — placeholder TOML, the fn + const, swap `boot.rs:203` to `shipped_fixture_set_v3_mixed()`, guard against `V3_MIXED_PLACEHOLDER_SET_NAME`.
- [ ] **Step 4: Full Rust suite green** (v2 pins in `codec_fixtures_test.rs`, `pager_codec_gate_test.rs`, `journal_test.rs` are fixture-string tests, unaffected).
- [ ] **Step 5: Commit** — `feat: G5 boots against codec-tasks-v3-mixed (placeholder until frozen)`

### Task 4: Factory fast-follows — canonical check-first + all-files contamination

**Files:**
- Modify: `tools/flywheel/factory/task.py` (move `CHECK_INSTRUCTION` here beside `DONE_INSTRUCTION` :14; extend `validate_refusal_task` :104-152)
- Modify: `tools/flywheel/factory/templates_refusal_python.py` (:23) and `templates_refusal_text.py` (:21) — import the canonical const, delete both local copies
- Modify: `tools/flywheel/factory/contamination.py` (`task_violates_gates` :216-241, `_corpus_tasks_from_rows` :119-137)
- Modify: `tools/flywheel/factory/generate.py` (`_row_meta` :178-192) and `generate_refusal.py` (`refusal_row_meta` :101-113) — row meta gains a `files` key
- Test: `tools/flywheel/tests/test_templates_refusal.py`, `test_contamination.py`, `test_contamination_g5.py`

**Interfaces:**
- Produces: `CHECK_INSTRUCTION` importable from `factory.task` (exact literal preserved: `"Check first, and only patch if it is genuinely wrong; then emit done."`); `validate_refusal_task` violation `"goal does not end with the check-first instruction"`; corpus-side screening over every `(name, contents)` in `task.files` (draw-time AND post-hoc CLI). Row meta `files` key = `{path: contents}` for every file.

- [ ] **Step 1: Failing tests** — validator: a `RefusalTask` whose goal lacks the trailing `CHECK_INSTRUCTION` yields the new violation (mutation pin: goal ending mid-instruction also fails). Contamination: a task whose SIBLING file body is a verbatim copy of a gate fixture file is rejected at draw time (`task_violates_gates` returns `file_contents_match`) and caught by the CLI post-hoc via the row's `files` key — plant-a-copy pattern from `test_contamination_g5.py:284-297`.
- [ ] **Step 2: Verify both fail** (validator silent today; sibling screening absent per `contamination.py:233-240`).
- [ ] **Step 3: Implement** — const move + imports; validator assertion; `task_violates_gates` iterates all `task.files.items()` (target's `search` handling unchanged); `_row_meta`/`refusal_row_meta` emit `files`; `_corpus_tasks_from_rows` reads it (absent key = legacy row, fall back to target-only so old corpora still check).
- [ ] **Step 4: Full pytest suite green** — `test_same_seed_produces_byte_identical_corpus_and_fingerprint` will show the new `files` key changes row bytes: that is expected and correct (fresh corpus this turn); update the test's expectations, never weaken determinism itself (same seed still = byte-identical).
- [ ] **Step 5: Commit** — `feat: canonical check-first instruction asserted; contamination guard screens every task file`

### Task 5: Factory — symptom-mismatch refusal family

**Files:**
- Create: `tools/flywheel/factory/templates_symptom_mismatch_python.py` (≤400 lines)
- Create: `tools/flywheel/factory/templates_symptom_mismatch_text.py` (≤400 lines)
- Modify: `tools/flywheel/factory/task.py` (`SYMPTOM_MISMATCH = "symptom_mismatch"` into `REFUSAL_FAMILIES` :70-72; family branch in `validate_refusal_task`)
- Modify: `tools/flywheel/factory/templates_refusal.py` (`GROUPS` :30-35, `GROUP_CYCLE_ORDER` :53-58 → 6 groups)
- Modify: `tools/flywheel/factory/goal_phrasing.py` (new `symptom_mismatch_skeletons(rng, target, claim, instruction) -> str`, ≥4 skeletons)
- Test: `tools/flywheel/tests/test_templates_refusal.py`, `test_goal_phrasing.py` (the 21-family count pin at :109 grows — a deliberate, expected edit)

**Interfaces:**
- Consumes: `RefusalTask` unchanged (`target_missing=False`, target among `files`); `CHECK_INSTRUCTION` from Task 4.
- Produces: ≥2 python + ≥2 text template functions, each returning a `RefusalTask` where the file contains a REAL planted defect Y and the goal claims a DIFFERENT, absent defect X (X backtick-quotes identifiers genuinely present in the file — the turn-2 plausibility rule); `refusal_reason` is the ruled two-part content: `"Checked: no <X> in <target> — <factual reason>. Found instead: <Y> at <site>; no change made without a goal that matches."` (exact template per family, derived from ground truth — the template knows both X and Y).

- [ ] **Step 1: Failing tests** — mirror the existing family test classes in `test_templates_refusal.py`: all six groups present; symptom-mismatch goals quote a real identifier; the claimed X is provably false (assert the claimed relation does NOT hold in the generated file, per-template, following `DefectAbsentClaimIsProvablyFalseTest`); the planted Y IS present (assert the defect relation holds); `refusal_reason` names both halves; validator accepts every draw and rejects a hand-broken one (family unknown → violation; goal missing instruction → Task 4's violation).
- [ ] **Step 2: Verify fail** (modules don't exist).
- [ ] **Step 3: Implement** — 4+ templates with distinct code shapes (the v2 same-shape flaw is the counterexample); wire registries + skeletons; `validate_refusal_task` symptom-mismatch branch = defect-absent's checks (target among files, quoted-identifier plausibility) — the X-false/Y-present proofs stay in tests (ground truth lives in templates, not the NamedTuple).
- [ ] **Step 4: Full pytest green** (incl. updated family-count pin).
- [ ] **Step 5: Commit** — `feat: symptom-mismatch refusal family — refuse and name what IS there`

### Task 6: flywheel-tool — wire growth, find/run rendering, test split FIRST

**Files:**
- Split: `crates/bloomery-daemon/tests/flywheel_tool_test.rs` (788/800) → keep patch/anti-drift/golden there, move refusal tests to `tests/flywheel_tool_refuse_test.rs` (pure move, byte-identical test bodies), THEN create `tests/flywheel_tool_verbs_test.rs` for the new tests
- Modify: `crates/bloomery-daemon/src/bin/flywheel_tool.rs` (512; budget ~250 more — if it crosses 800, split `bin/flywheel_tool/render.rs` following the `exec_run.rs`-out-of-`exec.rs` precedent)

**Interfaces:**
- Consumes: `exec_find(grant, pattern, path, bounds)` (`exec.rs:483`), `exec_run(grant, cwd, argv, bounds)` (`exec_run.rs:213`), `transcript_entry(step, verb, outcome, content)` = `"\n[step {step} {verb}] {outcome}\n{content}\n"` (`task_loop.rs:178`), `render_task_prompt`.
- Produces: `TrajectoryRequest` gains `#[serde(default)] files: Vec<RequestFile>` (`struct RequestFile { path: String, contents: String }`), `#[serde(default)] find_pattern: Option<String>`, `#[serde(default)] run_argv: Option<Vec<String>>`, `#[serde(default)] commands: Vec<Vec<String>>`. New completions:

```rust
fn find_completion(pattern: &str, path: &str) -> String {
    format!("<action verb=\"find\" pattern=\"{pattern}\" path=\"{path}\">\n</action>")
}
fn run_completion(argv: &[String]) -> String {
    format!("<action verb=\"run\">\n{}\n</action>", serde_json::to_string(argv).expect("argv serializes"))
}
```

  Shapes (Task 7 factory relies on these): `find_pattern: Some` → 4-pair find-shaped patch trajectory (find → read → patch → done), find and read observations from REAL `exec_find`/`exec_read` against a scratch dir materialized from `files` (the `real_missing_target_read` precedent, `flywheel_tool.rs:458-483`); `run_argv: Some` → 4-pair run-verified trajectory (read → patch → run → done), the run executed for REAL against the PATCHED file under a grant carrying `commands` — **a nonzero run exit is a hard error response** (an ideal whose verification fails is not an ideal; the factory aborts that task as structural).
- Ride-along (CARRIED-DEBT): every grant built in this binary goes through `serde_json::json!`, retiring the unescaped-path `format!` in `real_missing_target_read`.

- [ ] **Step 1: Pure test split; suite green; commit** — `refactor: split flywheel_tool_test before it crosses the ceiling`
- [ ] **Step 2: Failing tests** in `flywheel_tool_verbs_test.rs` — find-shaped: 4 pairs; pair-1 completion is the exact `find_completion` grammar (verb-card parity, `card.rs:105-115`); pair-2's prompt embeds a transcript whose find observation came from real `exec_find` (assert `outcome == format!("found {n} matches")` with the real n, content lines match `^{scratch}/.*:\d+: ` — format-parity, absolute path expected); run-verified: 4 pairs, run observation `outcome == "ran python3 exit 0"`, content starts `"exit 0\n"`; nonzero-exit request → error response; ungranted argv → error. Anti-drift: clone the `missing_target_anti_drift_pin_matches_real_second_prompt_under_v3` pattern (:376-388) for both new shapes.
- [ ] **Step 3: Verify fail** (unknown fields / no handlers).
- [ ] **Step 4: Implement**; full Rust suite + fmt/clippy green; golden pin `bin_patch_mode_response_is_byte_identical_to_the_turn1_golden` (:583) byte-untouched.
- [ ] **Step 5: Commit** — `feat: flywheel-tool renders find-shaped and run-verified trajectories with real-execution observations`

### Task 7: Factory — find-shaped and run-verified repair slices

**Files:**
- Modify: `tools/flywheel/factory/task.py` (`Task` gains NamedTuple fields with defaults: `trajectory: str = "plain"` (`"plain" | "find" | "run"`), `find_pattern: str = ""`, `run_argv: tuple[str, ...] = ()`, `commands: tuple[tuple[str, ...], ...] = ()`; `validate_task` branches on `trajectory`)
- Create: `tools/flywheel/factory/templates_multifile_python.py`, `templates_multifile_text.py` (find-shaped variants of existing defect families: target + 2–4 sibling files, goal names the SYMPTOM, never the filename; `find_pattern` matches in the target and in NO sibling)
- Modify: `tools/flywheel/factory/templates_python.py` (run-verified = existing py families re-registered with `trajectory="run"`, `run_argv=("python3","-m","py_compile",target)`, `commands=(("python3","-m","py_compile"),)` — wrapper, not copies)
- Modify: `tools/flywheel/factory/generate.py` (slice cycle: of 999 patch tasks — 333 find-shaped / 333 run-verified / 333 plain, position-derived like `_FAMILY_PATTERN` :94; tool-request wiring passes the new fields; pairs-per-task becomes shape-dependent: 4 for find/run shapes, 3 for plain)
- Test: `tools/flywheel/tests/test_templates.py`, `test_generate.py` (the exactly-three-pairs pin :`test_each_surviving_task_yields_exactly_three_pairs` becomes per-shape), `tools/flywheel/tests/fixtures/stub_tool.py` (answer the new request fields)

**Interfaces:**
- Consumes: Task 6's wire fields (names exact: `files`, `find_pattern`, `run_argv`, `commands`).
- Produces: validated `Task` rows whose `validate_task` rules branch: find-shaped → target filename must NOT appear in goal (inverts :53's rule), `find_pattern` occurs in target contents and in no sibling, goal still ends `DONE_INSTRUCTION`; run-verified → plain rules + non-empty `run_argv` starting with a granted prefix; plain → today's rules byte-unchanged.

- [ ] **Step 1: Failing tests** — validator branch tests (one per new rule, each with the mutation that flips it); template tests: find-shaped families plant ≥2 siblings, pattern uniqueness holds across every draw; generate: a small run yields the 3-shape cycle in the pinned ratio, find/run rows carry 4 pairs, plain 3; determinism pin still byte-identical same-seed.
- [ ] **Step 2: Verify fail.**
- [ ] **Step 3: Implement** (respect the 400-line module cap — new modules, wrappers over existing template bodies, no copy-paste of defect logic).
- [ ] **Step 4: Full pytest green + a `RealToolIntegrationTest`-style end-to-end row through the real binary for each new shape.**
- [ ] **Step 5: Commit** — `feat: find-shaped and run-verified repair slices in the corpus factory`

### Task 8: Author and freeze codec-tasks-v3-mixed

**Files:**
- Modify: `crates/bloomery-daemon/fixtures/codec-tasks-v3-mixed.toml` (REPLACE placeholder with the real frozen set)
- Modify: `crates/bloomery-daemon/tests/codec_fixtures_test.rs` (v3 structural suite) and the two boot pins from Task 3 (flip to real-set assertions)
- Test: `tools/flywheel/tests/test_contamination_g5.py` (v3 disjointness)

**Interfaces:**
- Consumes: Tasks 2 (commands field), 5, 7 (families/shapes to author from), gate seed **8200820**.
- Produces: the frozen instrument — `set = "codec-tasks-v3-mixed"`, 32 fixtures: 16 patch (6 multi-file find-shaped + 5 run-granted carrying `commands = [["python3","-m","py_compile"]]` + 5 plain) + 16 refuse (6 defect-absent + 5 missing-target + 5 symptom-mismatch), both lenses in both classes, FROZEN header naming seed 8200820 and the amendment rule.

- [ ] **Step 1: Failing structural tests** in `codec_fixtures_test.rs` (mirror the v2 suite :278-509): 32 fixtures, 16/16 split, exact family counts (6/5/5), exact shape counts (6/5/5), run-granted fixtures carry the exact command prefix, patch references land through the real lenses, defect-absent and symptom-mismatch goals quote a real identifier from the target, names unique across THREE shipped sets (extend :361), and the **diversity assertion**: no two fixtures in a class share a code shape (pin: pairwise, the normalized target contents — identifiers stripped to placeholders — are distinct; write the normalizer in the test).
- [ ] **Step 2: Verify fail against the placeholder.**
- [ ] **Step 3: Author the set** — generate candidates with the factory at seed 8200820 (held out: never enters any corpus), hand-select to the pinned composition, write the TOML with the FROZEN header. Real py fixtures for run-granted must `py_compile` cleanly post-patch.
- [ ] **Step 4: Green: structural suite, boot pins (real set, no placeholder skip), full Rust suite. Factory disjointness:** `test_contamination_g5.py` gains `V3MixedDisjointTest` (pseudo-corpus pattern from :284-297, exporting EVERY fixture file) proving v3 ⟂ v1, v3 ⟂ v2, and NEVER add v3 to `GATE_VOCABULARY` (`gate_vocabulary.py:20-30`'s reasoning).
- [ ] **Step 5: Commit** — `feat: codec-tasks-v3-mixed authored and FROZEN (seed 8200820, 16+16, 6/5/5 both classes)`

### Task 9: Baselines — stock-14B and flywheel2 through G5-on-v3 (LIVE, HUMAN-GATED)

**STOP: get Brice's explicit go before this task.** Featured build LAST; never `timeout`.

**Files:**
- Create: `docs/superpowers/evidence/2026-08-20-g5v3-baselines.md` + the two boots' journal/tasks JSONL committed beside it

**Interfaces:**
- Consumes: Task 8's frozen set; `g5_probe = true` per model in the boot config (`config.rs:168`); GGUFs `~/flywheel2/…Q4_K_M.gguf` and the stock 14B.
- Produces: the anchors flywheel3 must hold; flywheel2's DECIDED verdict (candidate (c)'s payoff). Baselines run BEFORE training exists (spec §5).

- [ ] **Step 1: Preflight** — suite green, `cargo build --release -p bloomery-daemon --features vulkan` LAST, assay pin per drift-watch deployment note; record GGUF shas.
- [ ] **Step 2: Boot per model** (stock, then fw2), G5 probe on v3-mixed; daemon down by verified PID after each.
- [ ] **Step 3: Evidence doc** — per-class counts, Wilson intervals, decided/provisional flags, per-family and per-shape breakdowns, find/run usage counts from `TaskStep` rows (denominators 6 and 5), surprises verbatim, never re-run for a nicer number.
- [ ] **Step 4: Commit** — `docs: G5-on-v3 baselines — stock-14B and flywheel2 (decided)`

### Task 10: Combined corpus + pre-registration

**Files:**
- Create: `docs/superpowers/evidence/2026-08-20-flywheel3-preregistration.md` (template: `2026-08-16-flywheel2-preregistration.md`)
- Create: `docs/superpowers/evidence/2026-08-20-flywheel3-fingerprint.json`, `…-contamination-report.json`
- Modify: `tools/flywheel/train.py` header (turn-3 wording; seeds stay 20260816 — recorded)

**Interfaces:**
- Consumes: every factory task above; the real `flywheel-tool` release binary.
- Produces: `~/flywheel3/corpus.jsonl` (out-of-repo; sha in the fingerprint + prereg). Invocation, exactly:

```bash
python3 -m tools.flywheel.factory.generate --seed 20260820 --count 999 \
  --refusal-count 450 \
  --gate crates/bloomery-daemon/fixtures/codec-tasks-v1.toml \
  --gate crates/bloomery-daemon/fixtures/codec-tasks-v2-mixed.toml \
  --gate crates/bloomery-daemon/fixtures/codec-tasks-v3-mixed.toml \
  --tool target/release/flywheel-tool \
  --out ~/flywheel3/corpus.jsonl \
  --report docs/superpowers/evidence/2026-08-20-flywheel3-fingerprint.json
```

- [ ] **Step 1: Generate; run the contamination CLI post-hoc over the corpus against all three gates; commit fingerprint + report.**
- [ ] **Step 2: Prereg doc BEFORE training** — corpus identity (seed 20260820, counts, slice ratio 333/333/333, refusal 150/family, corpus sha, gates shas), training seeds statement (20260816 unchanged, procedure identity), the battery + kill verbatim from spec §5, secondary endpoints with denominators, artifact destination `~/flywheel3/`, honest possibilities. Commit.
- [ ] **Step 3: Commit** — `docs: flywheel3 corpus fingerprint + pre-registration (committed before training)`

### Task 11: Training → merge → GGUF (LIVE, HUMAN-GATED)

**STOP: Brice's explicit go.** Venv `~/flywheel-venv`; `tools/flywheel/train.py` unchanged hyperparameters (MAX_SEQ 4096, LoRA r16/α32, 2 epochs, bs1×ga8, lr 2e-4 cosine, bf16, no chat template, no EOS, completion-only loss, `</action>` tail assertion).

- [ ] **Step 1: Train** on `~/flywheel3/corpus.jsonl` → adapter in `~/flywheel3/`; save loss log + `pip-freeze.txt` beside it (turn-2 convention).
- [ ] **Step 2: Merge + quantize** → `~/flywheel3/qwen3-14b-flywheel3-Q4_K_M.gguf`; record adapter/GGUF/corpus shas.
- [ ] **Step 3: No repo commit** (artifacts out-of-repo; shas land in Task 12's evidence).

### Task 12: The battery + evidence (LIVE, HUMAN-GATED)

**STOP: Brice's explicit go.** Featured build LAST before boots.

**Files:**
- Create: `docs/superpowers/evidence/2026-08-20-flywheel3-battery.md` + G4/G5 journal/tasks JSONL pairs
- Modify: `docs/CARRIED-DEBT.md` (merge-time append — the standing template lesson), `README.md` capability-ladder prose if the result changes it

**Interfaces:**
- Consumes: prereg thresholds VERBATIM (G4 ≥16/20 on v1; G5 ≥13/16 per class on v3; kill G4 <16/20 OR refuse <8/16); either verdict is recorded, never re-run for a nicer one.

- [ ] **Step 1: G4 boot** (fw3 on codec-tasks-v1) — dedicated boot, journal committed.
- [ ] **Step 2: G5 boot** (fw3 on v3-mixed) — per-class + per-family + per-shape numbers, find/run usage counts, journal committed.
- [ ] **Step 3: Evidence doc** — verdict against the prereg, capability ladder updated, honest anatomy on any miss; kill consequence executed if hit (adapter shelved, recorded).
- [ ] **Step 4: Commit** — `docs: flywheel3 battery — <verdict>`; CARRIED-DEBT append rides here.

---

## Self-review notes (run at write time)

- Spec coverage: §2 corpus → Tasks 4,5,7,10; §3 gate → Tasks 1,8; §4 deltas → Tasks 2,3,6 + the §4 amendment (Task 1); §5 prereg/battery → Tasks 9,10,11,12; §6 posture → embedded per task; §7 non-goals → no task touches enforcement, envelopes, journal schema, or frozen sets.
- Type consistency: wire field names (`files`/`find_pattern`/`run_argv`/`commands`) identical in Tasks 6 and 7; `commands` TOML key identical in Tasks 2 and 8; seed values identical in Global Constraints, Task 8, Task 10.
- Known deliberate edits to existing pins, so reviewers aren't surprised: `test_goal_phrasing` family count (Task 5), three-pairs pin becomes per-shape (Task 7), the two G5 boot pins flip twice (Tasks 3 and 8), `fixture_names_are_unique_across_both_shipped_sets` becomes three sets (Task 8).
