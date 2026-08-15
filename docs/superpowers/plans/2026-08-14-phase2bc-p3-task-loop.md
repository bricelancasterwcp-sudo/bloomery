# bloomery Phase 2b/2c — P3: Task Loop + Executors + HTTP Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Wire the P1 action codec and P2 capability grants into a working coding-agent task loop — `POST /agents/{id}/task` runs propose→validate→execute (bounded read/find/patch/run executors that open the grant-returned canonical path with `O_NOFOLLOW`), journals every `TaskStep`, and is dark by default behind `tasks_enabled = false`.

**Architecture:** A new `task` module in `bloomery-daemon` (`src/task/`): `exec.rs` (the four executors + shared observation type), `lens_py.rs` (the Python landing lens the P1 codec deferred here), `loop.rs` (the propose→validate→execute state machine, generic over `Substrate` so it's FakeSubstrate-tested), and `registry.rs` (async task state, background worker, `Arc<Mutex<..>>`). The HTTP surface lands in the existing `api_*` dispatch. Executors consume a validated `Grant` and open **the path the grant check returned** (the canonical, symlink-resolved one), never the raw model-supplied path, with `O_NOFOLLOW` on the open — the two binding obligations P2 handed off.

**Tech Stack:** Existing workspace (Rust stable, edition 2021). One new **direct** dependency for `bloomery-daemon`: `libc` (already transitive via `llama-cpp-sys-2`; needed as a direct dep for `libc::O_NOFOLLOW` — hardcoding a syscall flag is exactly the magic-number the project forbids). Consumes `bloomery-core::action` (P1) and `bloomery-core::grant` (P2).

**Spec:** `docs/superpowers/specs/2026-08-14-phase2bc-task-abi-grants-design.md` §5 (approved 2026-08-14). Umbrella laws §3 bind everything.

## Global Constraints

- **Open the grant-returned canonical path, with `O_NOFOLLOW`** (P2 binding obligation): every executor that opens a file opens the `PathBuf` returned by `check_read`/`check_write`, never the raw model target, and passes `O_NOFOLLOW` via `OpenOptionsExt::custom_flags(libc::O_NOFOLLOW)`. `O_NOFOLLOW` protects the final component; mid-path-component TOCTOU is closed only by a future `openat2(RESOLVE_NO_SYMLINKS)` pass — documented as a named v1 limit, not silently ignored.
- **tasks_enabled default false** (spec §5): `POST /agents/{id}/task` returns `501 {"error":"tasks_disabled"}` unless the operator sets it. The whole task surface is dark by default.
- **Envelope-constrained, validate-and-reask, never grammar-forced** (law 3): the loop parses one action per turn via P1's `parse_action_with_codec`; on `ActionError` it journals the diagnostic and re-asks, **max 2 re-asks per step**, then the step fails honestly and the loop continues.
- **Every step journals `TaskStep { id, step, verb, outcome, duration_ms }`** (law 7, the P2a-added variant) — plus the existing infer events. A `GrantViolation` is a `TaskStep` outcome, the step fails, the task continues, the model is told.
- **Bounds are explicit and configurable, over-cap is truncated-with-notice never silent** (spec §5): read ≤256 KiB, find ≤100 results, run output ≤64 KiB, run timeout 120 s — all config, all over-cap producing a visible notice in the observation.
- **The pager lock is held per infer+apply, not across a `run` subprocess's wall-clock** (spec §5) — the subprocess executes outside the lock, like the assay POST.
- **Patch is atomic write-with-verify** (spec §5, robigo safety): apply the codec in memory, run the landing lens, and only on a clean landing write via temp-file + rename; a failed landing leaves the file untouched.
- Dependency allowlist gains only `libc` (bloomery-daemon). TDD with RED/GREEN evidence; `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings` (default AND `--features llama`), `cargo test --workspace` green before every commit. GPU-free unit tests (FakeSubstrate + real tempdirs for FS). Files ≤800 lines; conventional commits. Branch `feat/phase2bc-p3-taskloop` from master.

---

### Task 1: Executor scaffolding + read & find executors

**Files:**
- Create: `crates/bloomery-daemon/src/task/mod.rs`, `crates/bloomery-daemon/src/task/exec.rs`
- Modify: `crates/bloomery-daemon/src/lib.rs` (or `main.rs` mod tree — follow how `pager`/`agents` are declared), `crates/bloomery-daemon/Cargo.toml` (add `libc` direct dep), workspace `Cargo.toml` (`libc = "0.2"` in `[workspace.dependencies]`)
- Test: `crates/bloomery-daemon/tests/task_exec_read_find_test.rs`

**Interfaces:**
- Consumes: `bloomery_core::action::Action` (Read/Find variants), `bloomery_core::grant::{Grant, GrantViolation}` (check_read).
- Produces (consumed by Tasks 2–4):

```rust
// mod.rs
/// The result of executing one action: what to feed back to the model, and
/// a short outcome tag for the TaskStep journal.
#[derive(Debug, Clone, PartialEq)]
pub struct Observation {
    pub outcome: String,       // e.g. "read 412 bytes", "grant violation: ...", "found 3 matches"
    pub content: String,       // the observation text appended to the transcript
    pub failed: bool,          // true = the step did not achieve its verb (grant/exec failure)
}
/// The bounds an executor enforces (from Config; defaults in Task 5).
#[derive(Debug, Clone, Copy)]
pub struct ExecBounds {
    pub read_cap_bytes: usize,       // 256 * 1024
    pub find_result_cap: usize,      // 100
    pub run_output_cap_bytes: usize, // 64 * 1024
    pub run_timeout_secs: u64,       // 120
}

// exec.rs
/// Absolutize a possibly-relative model path against the task cwd, then it is
/// ready for grant.check_read/check_write.
pub(crate) fn absolutize(cwd: &std::path::Path, p: &str) -> std::path::PathBuf;
/// Open the grant-returned canonical path with O_NOFOLLOW (final-component
/// symlink protection) and read up to cap bytes; returns (bytes, truncated).
pub(crate) fn open_nofollow_read(canon: &std::path::Path, cap: usize)
    -> std::io::Result<(Vec<u8>, bool)>;
/// Execute a Read action against a grant. cwd is the task's working dir.
pub fn exec_read(grant: &Grant, cwd: &std::path::Path, path: &str,
                 lines: Option<(u32, u32)>, bounds: &ExecBounds) -> Observation;
/// Execute a Find action: walk the grant's read roots (bounded) matching the
/// compiled regex; returns up to find_result_cap "path:line: text" matches.
pub fn exec_find(grant: &Grant, pattern: &str, path_prefix: &str,
                 bounds: &ExecBounds) -> Observation;
```

Behavior (binding):
- `exec_read`: `absolutize(cwd, path)` → `grant.check_read(&abs)`; on `Err(v)` → `Observation { outcome: format!("grant violation: {}", describe(v)), content: ..., failed: true }`. On `Ok(canon)` → `open_nofollow_read(&canon, read_cap_bytes)`; on `Err(io)` → failed observation naming the io error (e.g. NotFound, or ELOOP if the final component became a symlink post-check — the `O_NOFOLLOW` refusal); on `Ok((bytes, truncated))` → apply the optional `lines` window (1-indexed inclusive; out-of-range clamps to available, noted), `content` = the (windowed) UTF-8-lossy text, `outcome` = `read N bytes` + `" (truncated at cap)"` if truncated, `failed: false`.
- `exec_find`: compile `pattern` (already validated by P1, but recompile here — the executor is standalone); for each canonical read root under which `path_prefix` (absolutized) falls, walk files (skip dirs that fail to read; never follow symlinks out — canonicalize each candidate file and confirm it's still under a read root before matching, so a symlink inside the tree can't leak an outside file's contents), collect `path:lineno: line` for regex matches up to `find_result_cap`; over-cap → append `" (capped at N results)"`. No shell, no external `grep`.
- `open_nofollow_read`: `std::fs::OpenOptions::new().read(true).custom_flags(libc::O_NOFOLLOW).open(canon)` then `Read::take(cap as u64 + 1)` into a Vec; truncated = read `> cap`; return the first `cap` bytes + the flag. Comment the mid-path-TOCTOU limit + the openat2 future note.

- [ ] **Step 1: Write the failing tests** (real tempdir sandbox + a grant over it, mirroring the P2 test pattern):

```rust
use bloomery_core::grant::Grant;
use bloomery_daemon::task::{exec_read, exec_find, ExecBounds};
// helper sandbox(): /tmp/bloomery-exec-<uniq>/sandbox with file.txt ("line1\nline2\nline3\n"),
// out/, an escape symlink -> /etc, and a grant granting read=sandbox, write=sandbox/out.

fn bounds() -> ExecBounds { ExecBounds { read_cap_bytes: 256*1024, find_result_cap: 100,
    run_output_cap_bytes: 64*1024, run_timeout_secs: 120 } }

#[test]
fn read_a_granted_file_returns_its_content() {
    let (sb, g) = sandbox();
    let obs = exec_read(&g, &sb, "file.txt", None, &bounds());
    assert!(!obs.failed);
    assert!(obs.content.contains("line2"));
    assert!(obs.outcome.starts_with("read "));
}

#[test]
fn read_a_line_window_returns_only_those_lines() {
    let (sb, g) = sandbox();
    let obs = exec_read(&g, &sb, "file.txt", Some((2, 2)), &bounds());
    assert_eq!(obs.content.trim(), "line2");
}

#[test]
fn read_outside_the_grant_is_a_failed_grant_violation_not_a_panic() {
    let (sb, g) = sandbox();
    let obs = exec_read(&g, &sb, "/etc/passwd", None, &bounds());
    assert!(obs.failed);
    assert!(obs.outcome.contains("grant violation"));
}

#[test]
fn read_respects_the_byte_cap_with_a_visible_notice() {
    let (sb, g) = sandbox();
    let mut small = bounds(); small.read_cap_bytes = 4;   // "line" then truncated
    let obs = exec_read(&g, &sb, "file.txt", None, &small);
    assert!(obs.outcome.contains("truncated"));
    assert!(obs.content.len() <= 4);
}

#[test]
fn find_matches_within_the_read_root_bounded() {
    let (sb, g) = sandbox();
    let obs = exec_find(&g, "line\\d", &sb.to_string_lossy(), &bounds());
    assert!(!obs.failed);
    assert!(obs.content.contains("file.txt"));
}
```

- [ ] **Step 2: RED run.** `cargo test -p bloomery-daemon --test task_exec_read_find_test` → module unresolved.
- [ ] **Step 3: Implement.** Add `libc` dep; the `task` module tree; `Observation`/`ExecBounds`; `absolutize`/`open_nofollow_read`/`exec_read`/`exec_find`. `describe(GrantViolation)` is a small helper turning a violation into a repair-friendly string.
- [ ] **Step 4: GREEN + gates.** Tests, fmt, clippy both configs. Confirm the escape-symlink in the sandbox is refused by `exec_read` (add that assertion if not already covered).
- [ ] **Step 5: Commit.** `git add crates/ Cargo.toml Cargo.lock && git commit -m "feat: task executors - read + find (O_NOFOLLOW, grant-bounded)"`

---

### Task 2: Patch executor + Python landing lens

**Files:**
- Create: `crates/bloomery-daemon/src/task/lens_py.rs`
- Modify: `crates/bloomery-daemon/src/task/exec.rs` (add `exec_patch`), `crates/bloomery-daemon/src/task/mod.rs` (`pub mod lens_py;`)
- Test: `crates/bloomery-daemon/tests/task_exec_patch_test.rs`

**Interfaces:**
- Consumes: `Action::Patch{path, body: PatchBody}`, `bloomery_core::action::patch::apply_patch`, `bloomery_core::action::lens::{LandingLens, PlainText, land, Landing}`, `Grant::check_write`.
- Produces:

```rust
// lens_py.rs — the P1-deferred Python lens
pub struct PythonLens;
impl bloomery_core::action::lens::LandingLens for PythonLens {
    fn name(&self) -> &'static str;                       // "python"
    fn parses(&self, contents: &str) -> Result<(), String>; // py_compile subprocess
}
// exec.rs
/// Atomic write-with-verify. Reads the current file (empty if creating),
/// applies the codec, runs the landing lens; on a clean landing writes via
/// temp-file + rename; a failed landing leaves the file untouched.
pub fn exec_patch(grant: &Grant, cwd: &std::path::Path, path: &str,
                  body: &bloomery_core::action::PatchBody) -> Observation;
/// Choose the lens for a path by extension (.py → Python, else PlainText).
pub(crate) fn lens_for(path: &std::path::Path) -> Box<dyn LandingLens>;
```

Behavior (binding):
- `PythonLens::parses`: write `contents` to a temp file, run `python3 -m py_compile <tmp>` (or `python3 -c "import ast,sys; ast.parse(sys.stdin.read())"` fed on stdin — pick one, document it) with a short timeout; exit 0 → `Ok(())`; non-zero → `Err(stderr-first-line)`. If `python3` is absent → return `Err("python3 unavailable")` — the patch does NOT land (fail-closed: we cannot verify Python, so we do not claim it landed). Name is `"python"`.
- `lens_for`: `.py` → `PythonLens`, everything else → `PlainText`.
- `exec_patch`: `absolutize(cwd, path)` → `grant.check_write(&abs)`; `Err` → failed grant-violation observation. `Ok(canon)`: read the current file (`open_nofollow_read`; NotFound → treat as empty, for the create case) → `land(&current, body, lens_for(&canon).as_ref())`. On `Landing::Lands{new_contents, lens}` → write `new_contents` to a temp file in the **same directory** (so rename is atomic on one filesystem), then `std::fs::rename(tmp, &canon)`; `outcome` = `patched (lens: python|plaintext)`, `failed: false`. On `DidNotApply{reason,lens}`/`DidNotParse{detail,lens}`/`Unparsed{..}` → the file is **untouched**, `Observation { failed: true, outcome: format!("patch did not land: {..}"), content: the reason/detail }`. The temp file write also uses `O_NOFOLLOW` + create-new semantics so a pre-planted temp symlink can't redirect the write.

- [ ] **Step 1: Write the failing tests.**

```rust
// sandbox with write root sandbox/out; a .py file and a .txt file inside out/.
#[test]
fn a_landing_search_replace_patch_writes_the_file() { /* patch out/a.txt: old→new; assert file now contains new; obs.failed=false */ }
#[test]
fn a_non_applying_patch_leaves_the_file_untouched() { /* search text absent; assert file bytes unchanged; obs.failed=true, outcome mentions "did not land" */ }
#[test]
fn a_python_syntax_error_does_not_land_and_leaves_the_file() {
    // whole-file patch out/x.py with "def (:" (invalid); assert file unchanged, obs.failed, obs mentions python
}
#[test]
fn a_valid_python_patch_lands() {
    // whole-file patch out/x.py with "x = 1\n"; assert file == "x = 1\n", obs.failed=false, outcome mentions lens python
}
#[test]
fn patch_outside_the_write_root_is_a_grant_violation() { /* patch /etc/x → failed grant violation, no write */ }
#[test]
fn creating_a_new_file_in_the_write_root_lands() { /* whole-file patch out/created.txt (absent) → file created with contents */ }
```
Fill each with concrete assertions against the sandbox (the comments are binding scenarios).

- [ ] **Step 2: RED run.** Unresolved `exec_patch`.
- [ ] **Step 3: Implement** per behavior. The Python-lens tests are gated: if `python3` is unavailable on the box they must still pass deterministically — so the two Python tests either (a) `return` early with an eprintln when `which python3` fails (documented), or (b) assert the fail-closed "python3 unavailable → did not land" path. Prefer (b) where it fits so the fail-closed contract is always exercised.
- [ ] **Step 4: GREEN + gates.**
- [ ] **Step 5: Commit.** `git commit -m "feat: task executor - patch (atomic write-verify) + python landing lens"`

---

### Task 3: Run executor (sandboxed subprocess)

**Files:**
- Modify: `crates/bloomery-daemon/src/task/exec.rs` (add `exec_run`)
- Test: `crates/bloomery-daemon/tests/task_exec_run_test.rs`

**Interfaces:**
- Consumes: `Action::Run{argv}`, `Grant::check_command`.
- Produces:

```rust
/// Execute a Run action: grant.check_command first (refuse on violation, no
/// exec), then run argv DIRECTLY (no shell) with a scrubbed environment, a
/// wall-clock timeout, bounded captured output, cwd = the task cwd.
pub fn exec_run(grant: &Grant, cwd: &std::path::Path, argv: &[String],
                bounds: &ExecBounds) -> Observation;
```

Behavior (binding, the most security-sensitive executor):
- `grant.check_command(argv)` → `Err` → failed grant-violation observation, **the subprocess is never spawned** (assert this in a test via a command that would have side effects).
- `Ok`: `std::process::Command::new(&argv[0]).args(&argv[1..])` — **no shell, never `sh -c`**. `.env_clear()` then set only a minimal safe env (`PATH` to a fixed system value, `HOME`=cwd, `LANG=C`); no proxy vars, no inherited secrets. `.current_dir(cwd)`. `.stdin(Stdio::null())`. Capture stdout+stderr (piped). Spawn, then the **2a poll-and-kill+reap timeout pattern** (`try_wait` loop with a sleep, `run_timeout_secs`, on expiry `child.kill()` + `child.wait()`); drain output after exit, bounded to `run_output_cap_bytes` (over-cap → truncated-with-notice). `Observation` `content` = the (bounded) combined output + a header line with the exit code (or "timed out after Ns"); `outcome` = `ran <argv[0]> exit <code>` or `ran <argv[0]> timed out`; `failed` = true iff timed out or non-zero exit (a non-zero exit is a legitimate observation the model acts on — set `failed: false` for a clean non-zero? No: `failed` means "the step didn't achieve its verb"; a `run` that executes and returns an exit code DID achieve `run` — set `failed: false` for any completed run regardless of exit code, `failed: true` only for timeout/spawn-failure/grant-violation. Document this.).
- The subprocess runs **outside the pager lock** (Task 4 wires this; this executor just doesn't touch the pager).

- [ ] **Step 1: Write the failing tests.**

```rust
// grant granting commands [["echo"], ["sh","-c"]... no — grant [["echo"], ["sleep"], ["cat"]] over a sandbox cwd.
#[test]
fn a_granted_command_runs_and_captures_output() { /* echo hello → obs.content contains "hello", outcome "ran echo exit 0", failed=false */ }
#[test]
fn a_nonzero_exit_is_reported_but_not_a_step_failure() { /* a command exiting 1 → failed=false, outcome mentions exit 1 */ }
#[test]
fn an_ungranted_command_is_refused_without_spawning() {
    // grant commands [["echo"]]; exec_run rm-shaped argv against a canary file in cwd; assert failed grant-violation AND the canary still exists (never spawned)
}
#[test]
fn a_command_exceeding_the_timeout_is_killed_and_named() {
    let mut b = bounds(); b.run_timeout_secs = 1;
    // grant [["sleep"]]; run ["sleep","10"] → obs.outcome contains "timed out", failed=true, returns in ~1-2s not 10
}
#[test]
fn output_over_the_cap_is_truncated_with_notice() {
    let mut b = bounds(); b.run_output_cap_bytes = 16;
    // grant a command that prints > 16 bytes → content <= ~16, outcome/content notes truncation
}
#[test]
fn no_shell_interpretation() {
    // grant [["echo"]]; argv ["echo","$HOME;rm -rf /"] → output is the LITERAL string, no expansion, no second command
}
```

- [ ] **Step 2: RED run.**
- [ ] **Step 3: Implement** per behavior — reuse the 2a timeout pattern (poll `try_wait`, kill+reap). Use only `std::process` + `libc` (already a dep). Env scrub via `.env_clear()` + explicit minimal set.
- [ ] **Step 4: GREEN + gates.** (These tests spawn real subprocesses of `echo`/`sleep`/`cat` — GPU-free, fast, deterministic.)
- [ ] **Step 5: Commit.** `git commit -m "feat: task executor - run (no shell, scrubbed env, timeout, bounded output)"`

---

### Task 4: The task loop

**Files:**
- Create: `crates/bloomery-daemon/src/task/task_loop.rs`
- Modify: `crates/bloomery-daemon/src/task/mod.rs` (`pub mod task_loop;` + the `TaskSpec`/`TaskResult` types)
- Test: `crates/bloomery-daemon/tests/task_loop_test.rs`

**Interfaces:**
- Consumes: `Pager<S>` (infer), P1 `parse_action_with_codec` + `verb_card`, the executors (Tasks 1–3), `Grant`, `Journal` (TaskStep), `PatchCodec` (from the model's profile).
- Produces (consumed by Task 5):

```rust
#[derive(Debug, Clone)]
pub struct TaskSpec {
    pub goal: String,
    pub grant: Grant,
    pub budget_tokens: u64,
    pub max_steps: u32,
    pub cwd: std::path::PathBuf,          // first write_root, else first read_root
    pub patch_codec: bloomery_core::action::PatchCodec,
    pub bounds: ExecBounds,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub enum TaskStatus { Running, Done, Refused, BudgetExhausted, StepsExhausted, Error }
#[derive(Debug, Clone, serde::Serialize)]
pub struct TaskStepRecord { pub step: u32, pub verb: String, pub outcome: String, pub content: String }
#[derive(Debug, Clone, serde::Serialize)]
pub struct TaskResult { pub status: TaskStatus, pub steps: Vec<TaskStepRecord>, pub summary: Option<String> }
/// Run one task to completion against a pager. Generic over Substrate so it's
/// FakeSubstrate-tested. Journals a TaskStep per step.
pub fn run_task<S: bloomery_substrate::Substrate>(
    pager: &mut Pager<S>, agent_id: &str, spec: &TaskSpec, journal: &mut Journal,
) -> TaskResult;
```

Loop (binding, robigo-proven shape):
1. Render the prompt: `spec.goal` + `verb_card(spec.patch_codec)` + accumulated observation transcript, windowed to fit the agent's measured window — reuse the pager's refuse-with-arithmetic (if goal+card alone won't fit, the task ends `Error` with the arithmetic, never truncated).
2. `pager.infer(agent_id, &prompt, max_tokens)` (budget-charged; on `PagerError::Budget` → `TaskStatus::BudgetExhausted`; on `PromptTooLarge` → `Error` with the arithmetic; on `Contract`/`Substrate` → `Error`).
3. `parse_action_with_codec(&reply.text, spec.patch_codec)`. On `Err(ActionError)` → journal a `TaskStep{ verb: "?", outcome: the diagnostic }`, append the diagnostic to the transcript, **re-ask (goto 1) up to 2 times per step**; on the 3rd failure the step fails (`outcome: "unparseable after 2 re-asks"`) and the loop continues to the next step (a stuck step is not a stuck task).
4. On `Ok(action)`: dispatch to the executor (`Read`→exec_read, `Find`→exec_find, `Patch`→exec_patch, `Run`→exec_run, `Done`→terminate `Done` with the summary). The executor returns an `Observation`; journal `TaskStep{ step, verb, outcome: obs.outcome, duration_ms }`; append `obs.content` to the transcript.
5. Repeat until `done`, `step == max_steps` (→ `StepsExhausted`), budget exhaustion, or a hard error.
- Concurrency note for Task 5: `run_task` holds `&mut Pager` for `infer`; the `run` executor's subprocess is invoked *between* pager calls, so Task 5 must not hold the pager lock across `exec_run` — but `run_task` itself is single-threaded per task, so it takes the lock, infers, releases, executes, re-takes. (Task 5 owns the lock discipline; `run_task` takes `&mut Pager` per call — Task 5 locks around each `run_task` step, or restructures. The plan's Task 5 resolves this; here `run_task` is written to touch the pager only via the passed `&mut`.)

- [ ] **Step 1: Write the failing tests** (FakeSubstrate with SCRIPTED action turns — the model's replies are pre-canned `<action>` blocks):

```rust
// Build a Pager<FakeSubstrate> (Phase-1 fixture pattern), a grant over a real
// sandbox, and script the fake's replies as a sequence of action turns.
#[test]
fn a_read_then_done_task_completes() {
    // script: turn1 = <action verb="read" path="file.txt"></action>; turn2 = <action verb="done">read it</action>
    // run_task → status Done, steps has a "read" then the loop ends; journal has 2 TaskStep events
}
#[test]
fn an_unparseable_turn_is_re_asked_then_the_step_fails() {
    // script: 3 garbage turns then a done → after 2 re-asks the step fails (TaskStep outcome "unparseable..."), loop continues, done reached
}
#[test]
fn a_grant_violation_is_a_failed_step_not_a_task_abort() {
    // script: turn1 = read /etc/passwd (violation) ; turn2 = done → status Done, the violation is a failed TaskStep, task not aborted
}
#[test]
fn max_steps_terminates_the_task() {
    // script: infinite reads (never done), max_steps=3 → status StepsExhausted, exactly 3 steps journaled
}
#[test]
fn budget_exhaustion_ends_the_task() {
    // tiny budget → first infer refuses → status BudgetExhausted
}
```

- [ ] **Step 2: RED run.**
- [ ] **Step 3: Implement** `run_task` per the loop. Keep it under 400 lines; the prompt renderer can be a private helper.
- [ ] **Step 4: GREEN + gates.**
- [ ] **Step 5: Commit.** `git commit -m "feat: task loop - propose/validate/execute with re-ask and TaskStep journaling"`

---

### Task 5: HTTP surface + tasks_enabled + async registry + docs

**Files:**
- Create: `crates/bloomery-daemon/src/task/registry.rs`
- Modify: `crates/bloomery-daemon/src/api_native.rs` (or a new `api_task.rs` wired into the dispatch), `crates/bloomery-daemon/src/config.rs` (`tasks_enabled` + `ExecBounds` fields), `crates/bloomery-daemon/src/main.rs` (wire config), `README.md`
- Test: `crates/bloomery-daemon/tests/api_task_test.rs`

**Interfaces:**
- Consumes: `run_task` (Task 4), the shared `Arc<Mutex<Pager<S>>>` (Task 14's serve), `Config`.
- Produces:
  - `POST /agents/{id}/task` `{goal, grants, budget_tokens?, max_steps?}` → `202 {"task_id": "..."}` (spawns a worker) | `501 {"error":"tasks_disabled"}` when `!tasks_enabled` | `422 {"error":"invalid_grant", detail}` when the grant JSON fails validation | `404` unknown agent.
  - `GET /agents/{id}/task/{task_id}` → `200 {status, steps:[...], summary}` | `404`.
  - `registry.rs`: `TaskRegistry` (`Arc<Mutex<HashMap<TaskId, TaskResult+status>>>`); `spawn_task` runs `run_task` on a background `std::thread`, updating the registry entry to `Done`/etc on completion; `get` reads a snapshot.
  - Config: `tasks_enabled: bool` (`#[serde(default)]` → false), `read_cap_bytes`/`find_result_cap`/`run_output_cap_bytes`/`run_timeout_secs` (serde defaults 262144/100/65536/120) → an `ExecBounds`.

Concurrency (binding): the worker thread takes the `Arc<Mutex<Pager>>` lock **per `run_task` pager call** (infer), not for the task's whole lifetime — the `run` subprocess and file I/O happen while the lock is released, so one long task doesn't wedge the daemon. Simplest correct shape: `run_task` is refactored (or wrapped) so each pager `infer` locks-infers-unlocks and the executors run lock-free; document the exact locking points. The grant is deserialized via `Grant::from_json` (or the sealed derive path) → `422` on `GrantError`.

- [ ] **Step 1: Write the failing tests** (std TcpStream `http()` helper; `test_support::serve_fake` extended, or a task-enabled variant):

```rust
#[test]
fn task_endpoint_is_501_when_disabled() {
    // serve_fake with tasks_enabled=false → POST /agents/{id}/task → 501 tasks_disabled
}
#[test]
fn a_task_runs_and_is_pollable_to_done() {
    // tasks_enabled=true, scripted fake (read then done), a grant over a tempdir →
    // POST → 202 {task_id}; poll GET until status Done (bounded retries); steps present
}
#[test]
fn an_invalid_grant_is_422() {
    // POST with grants {"commands":[[]]} (empty prefix) → 422 invalid_grant (the P2 seal rejects it)
}
#[test]
fn unknown_agent_is_404() { /* POST task for a nonexistent agent id → 404 */ }
```

- [ ] **Step 2: RED run.**
- [ ] **Step 3: Implement** `registry.rs` + the two routes + config + main wiring. Poll-based GET; the worker thread updates the registry. `tasks_enabled` gate first. Grant `422` on validation failure.
- [ ] **Step 4: Docs.** README "Task loop (Phase 2b/2c P3)" subsection: the propose→validate→execute loop, the five verbs wired to bounded executors, grants enforced with O_NOFOLLOW open-of-returned-path (+ the named mid-path-TOCTOU/openat2 limit), `tasks_enabled` default-off, and a pointer that P4 gates codec landing (G4). Honest: no G4 gating yet (P4), local-only, buffered.
- [ ] **Step 5: Gates + whole suite + commit.** `cargo test --workspace`, fmt, clippy both configs. `git commit -m "feat: task HTTP surface + async registry + tasks_enabled gate, docs"`

---

## Self-review (performed at plan-writing time)

- **Spec §5 coverage:** the loop shape (render→infer→parse→grant-check→execute→journal→repeat, re-ask ≤2, terminate on done/max_steps/budget) → Task 4; the four executors with their binding bounds (read cap, find cap, run output cap + timeout, atomic patch-with-verify) → Tasks 1–3; `POST`/`GET /agents/{id}/task` + `202`/`501`/`422`/`404` → Task 5; `tasks_enabled` default-off → Task 5; the concurrency rule (lock per infer, subprocess lock-free) → Task 5 (with Task 4's `run_task` written to touch the pager only via the passed `&mut`); the P1-deferred Python landing lens → Task 2; the two P2 handoff obligations (open the returned canonical path, `O_NOFOLLOW`) → Task 1 (`open_nofollow_read`) applied in Tasks 1–2, with the mid-path-TOCTOU limit named.
- **Placeholder scan:** Tasks 2/4's tests use scenario-comment bodies with the concrete assertions named in prose (the same style prior plans used successfully) — not TBDs; the implementer writes the concrete asserts against the established sandbox/FakeSubstrate fixtures. No "handle edge cases"-style gaps.
- **Type consistency:** `Observation`/`ExecBounds` (Task 1) consumed by every executor and the loop; `exec_read`/`exec_find` (Task 1), `exec_patch` (Task 2), `exec_run` (Task 3) are the exact names Task 4's dispatch calls; `TaskSpec`/`TaskResult`/`TaskStatus`/`run_task` (Task 4) are what Task 5's registry drives; `PatchCodec`/`PatchBody`/`parse_action_with_codec`/`verb_card`/`land`/`Landing`/`LandingLens`/`PlainText` come from P1 (merged); `Grant`/`check_read`/`check_write`/`check_command`/`GrantViolation` from P2 (merged); `Pager`/`Journal`/`Event::TaskStep`/`ExecBounds`→config from Phase 1/2a.
