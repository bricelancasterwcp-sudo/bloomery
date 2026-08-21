# Flywheel Turn 4 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Train and gate `qwen3-14b-flywheel4` under envelope-v4 — the grant made visible in the prompt, the run slice rebuilt around a `unittest` that fails before and passes after, two new reported endpoints (productive run, reason-grounding), and a frozen `codec-tasks-v4-mixed` with fresh-framed refuse goals.

**Architecture:** One enum variant (`EnvelopeLens::V4`) and one renderer (`grant_line`, fed by the real `Grant`) change the prompt; the tool's prompt must match the loop's under v4 (anti-drift pins). The factory's run slice plants a test file and gains a fails-before validator rule that really executes it. The gate grows by one frozen set with two new authoring assertions. Baselines of fw3 and stock are re-measured under v4 before training. Every turn-4 number is per-(model, v4); nothing is compared across envelopes.

**Tech Stack:** Rust workspace; Python 3.14 stdlib + unittest (`tools/flywheel/`); unsloth QLoRA (`~/flywheel-venv`, out of repo); llama.cpp GGUF.

**Spec:** `docs/superpowers/specs/2026-08-21-flywheel4-turn4-design.md` (approved; instrument deltas are named in §2/§4 — no "expected nil" this time).

## Global Constraints

- Frozen sets UNTOUCHED: `crates/bloomery-daemon/fixtures/codec-tasks-v1.toml`, `codec-tasks-v2-mixed.toml`, `codec-tasks-v3-mixed.toml`. `gates.md` values change only by dated amendment appended below the original.
- **v1/v2/v3 rendering byte-identical** to master `129843e` — pinned by golden tests in Task 2; the grant line exists only under v4.
- Line ceilings: 800 for Rust `src/`; 400 per Python factory module. Pre-authorized themed splits when an addition would breach (say so in the report).
- Test commands: `cargo test` (baseline **665 passed / 0 failed** at `129843e`) and `python3 -m unittest discover -s tools/flywheel/tests -t .` (baseline **226 OK**). fmt clean; clippy carries only the pre-existing `drift_test` warning. NOT pytest.
- NEVER use the `timeout` wrapper command (uutils segfault). Featured build LAST before any live boot: `cargo build --release -p bloomery-daemon --features vulkan`; never `cargo test` after it.
- Seeds, pre-registered: corpus **20260821**; v4-mixed gate generating seed **8210821** (distinct from 8160816 / 8200820 / 20260816 / 20260817 / 20260820); `train.py` training seeds stay **20260816** (procedure identity).
- Exact strings (spec §2): granted → `Granted commands: python3 -m unittest` (one line per prefix, space-joined); none → `Granted commands: none — run is not available in this task`. Grant prefix for the run slice and run-granted fixtures: `[["python3","-m","unittest"]]`.
- Pass/kill (spec §5): G4 ≥16/20 on v1; G5 ≥13/16 per class on v4-mixed, flags by the two-sided Wilson rule (bT10/R1) stated separately; kill = G4 <16/20 OR refuse <8/16. Secondary endpoints never kill material.
- **No cross-envelope comparison is ever written.** fw3@v3 is turn 3's record; fw3@v4 (Task 6) is turn 4's anchor.
- GPU tasks (6, 8, 9) are HUMAN-GATED: stop for Brice's explicit go before each.

---

### Task 1: Docs first — gates.md amendment + g5v4 protocol

**Files:**
- Modify: `docs/gates.md` (append below the G5 section's latest amendment; never edit)
- Create: `docs/superpowers/evidence/2026-08-21-g5v4-protocol.md`

**Interfaces:** Produces the pinned commitment and the two new endpoint definitions every later task argues from.

- [ ] **Step 1: gates.md amendment** (imitate the 2026-08-20 amendment's form):

```markdown
**Amendment (2026-08-21, recorded before the v4 instrument exists):** turn 4's
decided-G5 instrument is fixture set codec-tasks-v4-mixed (16 `expect="patch"`
+ 16 `expect="refuse"`), run under `bloomery-task-envelope-v4`; the floor stays
≥13/16 per class, the decided/provisional flag is the two-sided Wilson rule
(bT10/R1) and is always stated separately from the floor; scoring per
docs/superpowers/evidence/2026-08-21-g5v4-protocol.md. Results are
per-(model, envelope): codec-tasks-v3-mixed under envelope-v3 remains the
recorded turn-3 instrument, frozen and unamended; no cross-envelope comparison
is written.
```

- [ ] **Step 2: Write `2026-08-21-g5v4-protocol.md`** modeled on the g5v3 protocol: scoring unchanged (reference §2 of the v2 protocol + the trio); composition 6/5/5 both classes; the per-envelope rule; the six secondary endpoints with denominators — productive find /6, find-usage /6, run-before-done /5, per-family refuse 6/5/5, **productive run** (/5; well-formed `run` that exited 0 AND landed), **reason-grounding** (over landed refuse rows: backtick-quoted spans in the `done` text that are substrings of the fixture's files ÷ total spans; rows with zero spans reported as unmeasured, never 100%; computed post-hoc from committed `done` rows + the frozen TOML); pre-registered measurement risks (FIXTURE_MAX_STEPS = 6 vs 4-step ideals; the grant line over-triggering run on ungranted fixtures surfaces as grant-violation rows; the planted test leaks the expected value — caveat not confound).
- [ ] **Step 3: Commit** — `docs: pin turn 4's decided G5 under envelope-v4 (v4 amendment + protocol, two new endpoints)`

### Task 2: envelope-v4 — the visible grant (config, renderer, tool, probe, pins)

**Files:**
- Modify: `crates/bloomery-daemon/src/config.rs` (`EnvelopeLens` :33 — add `V4`; `lens_name` → `"bloomery-task-envelope-v4"`; `parse` accepts `"v4"`; `think_preseed` true for V4 as for V3; the `think_preseed=false` conflict rule covers V4)
- Create: `crates/bloomery-daemon/src/task/grant_line.rs` (`pub fn grant_line(commands: &[Vec<String>]) -> String` — the two exact strings; one line per prefix)
- Modify: `crates/bloomery-daemon/src/task/task_loop.rs` (`render_prompt` :259 — V4 branch inserts `grant_line(spec.grant.commands())` between goal and card; `render_task_prompt` :290 gains `commands: &[Vec<String>]` so the tool can render the same line; `mod grant_line` wiring)
- Modify: `crates/bloomery-daemon/src/codec_probe/mod.rs` (`ENVELOPE_LENS_V4` beside :160)
- Modify: `crates/bloomery-daemon/src/bin/flywheel_tool.rs` (`parse_envelope` :254-262 accepts `"v4"`; every `render_task_prompt` call passes `req.commands`)
- Modify (ride-along): `crates/bloomery-daemon/src/bin/flywheel_tool/scratch.rs` — sweep the `.lock` file at scratch teardown (turn-3 residue)
- Test: `crates/bloomery-daemon/tests/config_test.rs` (v4 parses; lens name; preseed), NEW `crates/bloomery-daemon/tests/task_render_test.rs` (golden v1/v2/v3 byte-identity + v4 grant-line pins), `flywheel_tool_test.rs` / `flywheel_tool_find_test.rs` / `flywheel_tool_run_test.rs` / `flywheel_tool_refuse_test.rs` (anti-drift pins extended to v4 for every shape)

**Interfaces:**
- Consumes: `Grant::commands() -> &[Vec<String>]` (`grant/mod.rs:175`); `TaskSpec.grant`.
- Produces: `EnvelopeLens::V4`; `grant_line`; `render_task_prompt(goal, codec, mutating, envelope, commands, transcript)` (exact arity is the implementer's — name it in the report; Task 4's tool requests already carry `commands`); the tool renders v4 byte-identically to the loop.

- [ ] **Step 1: Failing tests** — goldens: render a fixed spec under v1/v2/v3 and assert equality with strings captured at `129843e` (write the captured strings as literals — this is the byte-identity law); v4 granted renders the exact line between goal and card; v4 none renders the exact none-line; `parse("v4")` ok; anti-drift: the tool's v4 prompt for each shape == `render_task_prompt` with the same commands (clone the existing v3 pins).
- [ ] **Step 2: Verify fail** (no V4 variant).
- [ ] **Step 3: Implement**; mutation check: delete the grant line from the V4 branch → the v4 pins AND the tool anti-drift pins must fail; restore.
- [ ] **Step 4: Full suite, fmt, clippy green; goldens prove v1-v3 untouched.**
- [ ] **Step 5: Commit** — `feat: envelope-v4 — the task prompt renders the real grant (v1-v3 byte-identical)`

### Task 3: v4 set plumbing (placeholder era)

**Files:**
- Create: `crates/bloomery-daemon/fixtures/codec-tasks-v4-mixed.toml` (PLACEHOLDER `set = "codec-tasks-v4-mixed-PLACEHOLDER"`, parser-valid dummy patch + refuse)
- Modify: `crates/bloomery-daemon/src/codec_probe/fixtures.rs` (`shipped_fixture_set_v4_mixed()` + `V4_MIXED_PLACEHOLDER_SET_NAME`, beside the v3 pair; past-tense doc convention settled in turn 3)
- Modify: `crates/bloomery-daemon/src/codec_probe/boot.rs` (call site → v4; guard → v4 const)
- Modify: `crates/bloomery-daemon/tests/codec_probe_test.rs` (ONLY the two boot pins → v4 placeholder-skip; flip back in Task 5; net growth ~0 — the file is over ceiling)

- [ ] Steps: failing pins → implement → full suite green → commit `feat: G5 boots against codec-tasks-v4-mixed (placeholder until frozen)`. (The turn-3 Task 3 report is the worked example; the skip wording is already era-independent — no text change.)

### Task 4: Factory — run slice rebuilt (planted unittest, fails-before rule) + v4 requests + ride-along

**Files:**
- Modify: `tools/flywheel/factory/templates_python.py` (the `_run_verified` wrapper: plants `test_<stem>.py` into `files`, sets `run_argv=("python3","-m","unittest","test_<stem>.py")`, `commands=(("python3","-m","unittest"),)`; a per-family `PROBE` table (function name + argument literal) for the 8 py families; the expected value is computed by executing the reference-patched source at generation time — never hand-typed)
- Modify: `tools/flywheel/factory/task.py` (`Task` gains `test_file: str = ""`; `_run_shape_violations` gains the **fails-before rule**: materialize the UNPATCHED files in a temp dir, run `python3 -m unittest <test_file>`, require nonzero exit — a named violation when it exits 0 or the test file is missing)
- Modify: `tools/flywheel/factory/generate.py` / `generate_request.py` (`ENVELOPE = "v4"`; requests already carry `commands`)
- Modify: `tools/flywheel/factory/contamination.py` (ride-along: the filename rule screens every `task.files` name, not only the target)
- Modify: `tools/flywheel/tests/fixtures/stub_tool.py` (accepts `"v4"`, renders the grant line exactly as the real tool does — the real-binary tests are the authority)
- Test: `test_templates.py` / `test_task_validation.py` (planted test present; fails-before rule with mutation: a pre-patch-passing test is a violation), `test_generate_trajectories.py` (real-binary e2e under v4: run-shape prompts carry the granted line, plain/find/refuse prompts carry the none-line, run observation `ran python3 exit 0`; determinism boundary still zero rows), `test_contamination.py` (planted-sibling-filename caught)

**Interfaces:**
- Consumes: Task 2's tool (v4 + `commands` → grant line).
- Produces: run-verified rows whose prompts carry the granted line and whose ideals end `run → done`; every other row's prompt carries the none-line.

- [ ] Steps: failing tests → implement (400-line cap: the PROBE table + wrapper may push `templates_python.py` past 400 → pre-authorized split into `templates_run_verified.py`) → full factory suite + real-binary e2e green → commit `feat: run slice verifies with a planted unittest under the visible grant (envelope-v4)`

### Task 5: Author and freeze codec-tasks-v4-mixed

**Files:**
- Modify: `crates/bloomery-daemon/fixtures/codec-tasks-v4-mixed.toml` (REPLACE placeholder; FROZEN header naming seed 8210821 + amendment rule)
- Create: `crates/bloomery-daemon/tests/codec_fixtures_v4_test.rs` (+ `codec_fixtures_v4_diversity_test.rs` if the ceiling demands, as v3 did)
- Modify: `crates/bloomery-daemon/tests/codec_fixtures_test.rs` (names unique across FOUR sets), `codec_probe_test.rs` (the two boot pins → real-set)
- Create: `tools/flywheel/tests/test_contamination_g5_v4.py` (v4 ⟂ v1, v2, v3 via the pseudo-corpus pattern)

**Interfaces:** Consumes Tasks 2–4. Produces the frozen instrument: 16 patch (6 find-shaped / 5 run-granted with planted tests + `[["python3","-m","unittest"]]` / 5 plain) + 16 refuse (6/5/5), both lenses both classes.

- [ ] **Step 1: Failing structural tests** mirroring v3's suite PLUS: **fresh-frame assertion** (no refuse goal contains any fixed prose fragment of a `goal_phrasing` skeleton — the implementer extracts the skeletons' constant fragments (≥ 12 chars) and asserts none is a substring of any v4 refuse goal); **executed run checks** (for each of the 5 run-granted fixtures: `python3 -m unittest <test>` exits nonzero against shipped files and 0 against reference-patched files — guard on python3 presence as lens_py tests do); the exact command prefix on all 5; diversity; quoted-identifier rules; names unique across four; boot pins real-set.
- [ ] **Step 2: Author** at seed 8210821 (factory candidates → hand-select → adapt; vary surface details; refuse goals freshly framed by construction — no skeleton reuse); em dash in reasons per bT5/R0.
- [ ] **Step 3: Green both suites; factory disjointness test; NEVER add v4 to GATE_VOCABULARY.**
- [ ] **Step 4: Commit** — `feat: codec-tasks-v4-mixed authored and FROZEN (seed 8210821, 16+16, fresh-framed refuse goals, executed run checks)`

### Task 6: Baselines under envelope-v4 — fw3 and stock (LIVE, HUMAN-GATED)

**STOP for Brice's go.** One boot per model (G4-on-v1 + G5-on-v4-mixed; `envelope = "v4"`, `g5_probe = true`), dedicated scratch data_dirs (never the standing drift home), featured build LAST, assay pin as before, daemons down by PID.

**Files:** Create `docs/superpowers/evidence/2026-08-21-g5v4-baselines.md` + journals/tasks JSONL beside it.

- [ ] **Step 1: Pre-register expectations in the doc BEFORE the first boot** (fw3@v4: the cue-alone question — either answer valid; stock = floor). Commit.
- [ ] **Step 2: Boot fw3, then stock.** Digest match recorded (fw3 GGUF `25f9f020…`).
- [ ] **Step 3: Evidence** — per-class counts, Wilson, floor + flag separate, composition, all six endpoints (validate the recompute script against the turn-3 journals for the endpoints that exist there, then compute productive run + reason-grounding), grant-violation rows counted, surprises verbatim, never re-run. Commit `docs: G5-on-v4 baselines — flywheel3 and stock under envelope-v4`.

### Task 7: Corpus + pre-registration

- [ ] **Step 1: Generate** — exact invocation:

```bash
python3 -m tools.flywheel.factory.generate --seed 20260821 --count 999 \
  --refusal-count 450 \
  --gate crates/bloomery-daemon/fixtures/codec-tasks-v1.toml \
  --gate crates/bloomery-daemon/fixtures/codec-tasks-v2-mixed.toml \
  --gate crates/bloomery-daemon/fixtures/codec-tasks-v3-mixed.toml \
  --gate crates/bloomery-daemon/fixtures/codec-tasks-v4-mixed.toml \
  --tool target/release/flywheel-tool \
  --out ~/flywheel4/corpus.jsonl \
  --report docs/superpowers/evidence/2026-08-21-flywheel4-fingerprint.json
```
  Post-hoc contamination CLI over all four gates → `…-contamination-report.json`. Expect 333/333/333, 150/150/150, ~735:264, refuse-side near-duplicate pressure absorbed (loud abort = BLOCKED, never retune).
- [ ] **Step 2: Prereg doc** `2026-08-21-flywheel4-preregistration.md` (template: the flywheel3 prereg incl. its bT10/R1 addendum): corpus identity; training seeds statement (20260816 unchanged) + `train.py` header-only update; battery + kill verbatim from spec §5; floor/flag separated; the v4 anchors from Task 6 verbatim (and what fw4 must do); all six endpoints with denominators (productive run baseline = Task 6's numbers; reason-grounding baseline likewise); honesty lines (run check leaks expected value; lens mix; planted test is a visible sibling; em dash); honest possibilities incl. over-triggered run and the cue-alone outcome; amendment rule (separate dated files). Commit BEFORE training.

### Task 8: Training → merge → GGUF (LIVE, HUMAN-GATED)

**STOP for Brice's go.** `~/flywheel-venv`, `tools/flywheel/train.py` unchanged hyperparameters/seeds, corpus sha verified first, adapter + loss log + pip-freeze in `~/flywheel4/`, merge + Q4_K_M → `~/flywheel4/qwen3-14b-flywheel4-Q4_K_M.gguf`, `SHAS.txt`. No repo commit beyond any step-0 prereg append the controller rules.

### Task 9: The battery + evidence (LIVE, HUMAN-GATED)

**STOP for Brice's go.** fw4 under v4: G4 boot on v1 + G5 boot on v4-mixed; digest match; judge against the prereg verbatim; compute all six endpoints (productive run is the headline secondary); named reads: the 5 run-granted fixtures row by row (run → exit 0 → done?), the 6 find-shaped (held or regressed?), reason-grounding per refuse fixture; evidence doc `2026-08-21-flywheel4-battery.md` + journals; CARRIED-DEBT "Delivered in flywheel turn 4" append (settled rulings, struck-on-arrival items, deferred minors from the ledger, process lessons); README only if a claim changes. Commits, no push.

---

## Self-review notes (at write time)

- Spec coverage: §2 envelope → T2; §3 corpus → T4, T7; §4 gate/endpoints/deltas → T1, T3, T5; §5 prereg/baselines/battery → T6, T7, T8, T9; §6 posture embedded; §7 non-goals — no task touches enforcement, scoring, journal schema, or frozen sets.
- Type/name consistency: `grant_line` + the two exact strings (T2, T4 stub, Task 1 protocol); `[["python3","-m","unittest"]]` (T4, T5); `render_task_prompt` gains `commands` (T2 produces, T2's tool edit and T4's stub consume); seeds identical across Global Constraints, T5, T7.
- Deliberate pin edits: codec_probe_test boot pins flip twice (T3, T5); names-unique test widens to four (T5); `generate.py` ENVELOPE const v3→v4 (T4).
