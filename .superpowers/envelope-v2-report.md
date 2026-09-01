# Envelope-v2 (think-preseeded lens) — implementation report

Branch: `feat/envelope-v2-think-preseed`
Governing docs: `.superpowers/envelope-v2-brief.md`, `docs/superpowers/evidence/2026-08-15-g4-protocol.md` §10 (Amendment 2)

## Summary

Implemented all 5 brief items in order, TDD throughout (tests written and
run to confirm failure/relevance before or alongside each implementation
change, full suite green at the end), plus both required mutation checks
with evidence (and restoration to the correct implementation).

## Per-item detail

### 1. Config (`crates/bloomery-daemon/src/config.rs`)

- `ModelSpec::Tuned` gained `#[serde(default)] think_preseed: bool`.
- New accessor `pub fn think_preseed(&self) -> bool` — `Path` variant
  returns `false`.
- Tests added/extended in `tests/config_test.rs`:
  - `think_preseed_true_parses` (new) — table entry with
    `think_preseed = true` parses and the accessor reflects it.
  - `tuned_model_entry_with_only_path_parses` extended with
    `assert!(!model.think_preseed())` (absent = false).
  - `bare_string_model_entry_parses` extended with
    `assert!(!model.think_preseed())` (bare path = false).

### 2. Pager plumbing

- `ModelEntry` (private, `pager.rs`) gained a `think_preseed: bool` field,
  defaulted `false` in `register_model`.
- Chose a **sibling setter** over growing `set_model_tuning`'s signature:
  `set_model_tuning` already has 8 call sites in
  `tests/pager_weights_test.rs` passing exactly two tuning args — a third
  positional bool there would be the larger diff for a conceptually
  independent flag (task-loop presentation, not VRAM accounting).
  Added in `crates/bloomery-daemon/src/pager/tuning.rs`:
  - `pub fn set_think_preseed(&mut self, model: &str, think_preseed: bool) -> Result<(), PagerError>`
  - `pub fn model_think_preseed(&self, model: &str) -> bool` (fail-closed
    to `false` for an unknown model, matching `model_patch_codec` /
    `model_mutating_verbs`'s collapse style).
- `agent_task_policy` (`pager/codec_gate.rs`) grew from
  `Option<(PatchCodec, bool)>` to `Option<(PatchCodec, bool, bool)>` —
  `(patch_codec, mutating_verbs, think_preseed)` — one source read through
  `agent_id`'s model, same one-source-of-truth rule as
  `effective_weights_bytes`.
- Callers updated:
  - `api_task.rs::create_task` destructures the 3-tuple and threads
    `think_preseed` into `TaskSpec`.
  - `codec_probe/mod.rs` reads `Pager::model_think_preseed` directly (it has
    no agent yet at probe time — same pattern as its existing
    `model_patch_codec`/`model_codec_from_profile` reads).
- `main.rs` wires `pager.set_think_preseed(name, spec.think_preseed())`
  alongside the existing `set_model_tuning` call, per model.
- Tests: `tests/pager_codec_gate_test.rs`'s two existing
  `agent_task_policy_*` tests updated to the 3-tuple shape; added
  `agent_task_policy_resolves_think_preseed_through_the_agents_model`
  (new) proving the third field resolves through `set_think_preseed`.

### 3. Task loop (`crates/bloomery-daemon/src/task/task_loop.rs`)

- `TaskSpec` gained `pub think_preseed: bool`.
- New named const:
  ```rust
  const THINK_PRESEED: &str = "<think>\n\n</think>\n\n";
  ```
  with a doc comment citing protocol §10 (Amendment 2) and the 2026-08-15
  eve feasibility probe (Q3 subject: `/no_think` did not suppress thinking
  under the raw-completion lens; a pre-closed think block did).
- `render_prompt` appends `THINK_PRESEED` at the very end of the rendered
  prompt (after the transcript, nothing after it) only when
  `spec.think_preseed` is set; unchanged (byte-for-byte v1) otherwise.
- All other `TaskSpec` construction sites updated for the new field:
  `api_task.rs`, `codec_probe/mod.rs`, `task/registry.rs` (3 sites, all
  `think_preseed: false` — unrelated to preseed behavior),
  `tests/task_loop_test.rs`'s `demoted_spec` helper (`false`) plus new
  `preseeded_spec` helper (`true`).
- Tests added in `tests/task_loop_test.rs` (FakeSubstrate harness):
  - `a_preseeded_spec_ends_the_rendered_prompt_with_the_think_preseed_literal`
    — asserts `FakeSubstrate::ctx_history(1).ends_with("<think>\n\n</think>\n\n")`
    (`ends_with`, not `contains`, per the brief).
  - `a_non_preseeded_spec_never_carries_the_think_preseed_literal` —
    asserts the literal is absent anywhere in the prompt.
  - Both use a single-turn `[done]` task so `ctx_history` holds exactly the
    one rendered prompt (no `\n`-joined prior turns ambiguity).

### 4. Codec probe (`crates/bloomery-daemon/src/codec_probe/mod.rs`)

- Kept `ENVELOPE_LENS` (v1) as-is; added
  `pub const ENVELOPE_LENS_V2: &str = "bloomery-task-envelope-v2"`.
- Added `fn envelope_lens(think_preseed: bool) -> &'static str` selecting
  between the two consts; replaced the single `ENVELOPE_LENS` usage in the
  verdict `detail` format string with `envelope_lens(ctx.think_preseed)`.
- `ProbeContext` gained a `think_preseed: bool` field, read once at
  "invariant 1" (the same locked section that reads `codec` and
  `codec_from_profile`, before any fixture runs) via the new
  `Pager::model_think_preseed`.
- `run_one_fixture`'s `TaskSpec` construction carries
  `think_preseed: ctx.think_preseed`, so every fixture prompt under a
  preseeded probe ends with the literal.
- Module docs updated with a new bullet citing §10/Amendment 2 in the
  "measurement-honesty rules" list.
- Tests in `tests/codec_probe_test.rs`:
  - `instrument_parameters_match_the_pre_registered_protocol` extended
    with `assert_eq!(ENVELOPE_LENS_V2, "bloomery-task-envelope-v2")`.
  - `all_fixtures_landing_keeps_mutating_verbs_and_journals_one_verdict`
    (the existing v1 scripted-pair test) extended with the minimal second
    assertion `!detail.contains(ENVELOPE_LENS_V2)`.
  - New companion test
    `a_preseeded_model_probe_journals_the_v2_lens_in_the_verdict_detail` —
    same fixture set / scripted replies / happy-path landing as the v1
    test, differing only by `p.set_think_preseed(MODEL, true)` before the
    probe runs; asserts the verdict's `detail` contains
    `ENVELOPE_LENS_V2` and never `ENVELOPE_LENS`.

### 5. HTTP task creation (`api_task.rs`)

- `create_task` threads `think_preseed` (the third element of the
  `agent_task_policy` tuple) into the built `TaskSpec`, resolved through
  the same one-source lookup as `patch_codec`/`mutating_verbs`.
- Tests added in `tests/api_task_test.rs` (HTTP-level, via
  `serve_codec_gate_fixture` — the same harness test (a)/(b)/(c) use):
  - `a_think_preseed_model_renders_its_task_prompt_with_the_preseed_literal`
    — `p.set_think_preseed("qwen", true)` before `create_agent`; the HTTP
    `POST /agents/{id}/task` task's rendered prompt (observed via
    `FakeSubstrate::ctx_history`) ends with the literal.
  - `a_non_preseeded_model_never_renders_the_preseed_literal_over_http` —
    counterpart with no `set_think_preseed` call (default `false`).

## Mutation checks (evidence)

Both performed by editing the implementation file directly, running the
targeted test, confirming failure, then restoring the file from a
pre-edit copy and re-running the full relevant test file to confirm green
again.

### Mutation 1 — remove the `ends_with` append (task_loop.rs)

Edited `render_prompt` to always return the un-preseeded `prompt` (dropped
the `if spec.think_preseed { ... }` branch entirely):

```
$ cargo test -p bloomery-daemon --test task_loop_test a_preseeded_spec_ends_the_rendered_prompt_with_the_think_preseed_literal
running 1 test
test a_preseeded_spec_ends_the_rendered_prompt_with_the_think_preseed_literal ... FAILED

thread '...' panicked at crates/bloomery-daemon/tests/task_loop_test.rs:649:5:
the rendered prompt must end with the literal pre-seed, got: "exercise the task loop\n\n# Action verbs\n\n...done\n</action>\n\n\n"

test result: FAILED. 0 passed; 1 failed; ...
```

Confirmed: the test fails exactly as expected when the append is removed.
Restored the file from the pre-edit copy; re-ran the full
`task_loop_test.rs` suite:

```
$ cargo test -p bloomery-daemon --test task_loop_test
running 12 tests
... (all 12) ... ok
test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

### Mutation 2 — hardcode `envelope_lens` to v1 (codec_probe/mod.rs)

Edited `envelope_lens` to ignore its argument and always return
`ENVELOPE_LENS`:

```rust
fn envelope_lens(_think_preseed: bool) -> &'static str {
    ENVELOPE_LENS
}
```

```
$ cargo test -p bloomery-daemon --test codec_probe_test a_preseeded_model_probe_journals_the_v2_lens_in_the_verdict_detail
running 1 test
test a_preseeded_model_probe_journals_the_v2_lens_in_the_verdict_detail ... FAILED

thread '...' panicked at crates/bloomery-daemon/tests/codec_probe_test.rs:481:13:
a preseeded model's verdict must name v2: applies_and_parses under bloomery-task-envelope-v1; default (codecs unmeasured)

test result: FAILED. 0 passed; 1 failed; ...
```

Confirmed: the v2 verdict test fails exactly as expected when the lens
function is hardcoded to v1. Restored the file from the pre-edit copy;
re-ran the full `codec_probe_test.rs` suite:

```
$ cargo test -p bloomery-daemon --test codec_probe_test
running 21 tests
... (all 21) ... ok
test result: ok. 21 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

## Constraint: pager.rs ≤800 lines

`pager.rs` was exactly 800 lines before this change (confirmed via
`git show HEAD:crates/bloomery-daemon/src/pager.rs | wc -l`). Since
`ModelEntry` is a private struct defined in `pager.rs` (submodules like
`tuning.rs` can `impl` methods on `Pager`/read `ModelEntry` fields but
cannot add fields to a type from outside the module that defines it), the
new `think_preseed: bool` field and its `register_model` initializer line
were structurally unavoidable additions to `pager.rs` itself (all *logic* —
the setter/getter — went into `pager/tuning.rs` per the brief). To stay at
the 800-line cap, three pre-existing blank `///` paragraph-separator lines
in nearby, low-stakes doc comments (`DEFAULT_OVERHEAD_BYTES`,
`DEFAULT_CTX_OVERHEAD_BYTES`, `FreeVramFn`) were merged into their
following paragraph — no content/wording removed, just the blank-line
paragraph break. Final line count: exactly 800.

## Commands run (final verification pass, in order)

```
$ cargo fmt --all
$ cargo fmt --all -- --check                                    # exit 0
$ cargo clippy --workspace --all-targets -- -D warnings          # clean, no warnings
$ cargo clippy --workspace --all-targets --features llama -- -D warnings   # clean, no warnings
$ cargo test --workspace                                         # 50/50 test-result blocks ok, 0 failed
$ cargo build --features llama                                   # clean build
```

No `timeout` binary was used to wrap any command.

## Files touched

- `crates/bloomery-daemon/src/config.rs`
- `crates/bloomery-daemon/src/main.rs`
- `crates/bloomery-daemon/src/pager.rs`
- `crates/bloomery-daemon/src/pager/tuning.rs`
- `crates/bloomery-daemon/src/pager/codec_gate.rs`
- `crates/bloomery-daemon/src/task/task_loop.rs`
- `crates/bloomery-daemon/src/task/registry.rs` (TaskSpec literal fixups only)
- `crates/bloomery-daemon/src/codec_probe/mod.rs`
- `crates/bloomery-daemon/src/api_task.rs`
- `crates/bloomery-daemon/tests/config_test.rs`
- `crates/bloomery-daemon/tests/pager_codec_gate_test.rs`
- `crates/bloomery-daemon/tests/task_loop_test.rs`
- `crates/bloomery-daemon/tests/codec_probe_test.rs`
- `crates/bloomery-daemon/tests/api_task_test.rs`

## Commit

`7d012d1` on branch `feat/envelope-v2-think-preseed`:
`feat: envelope-v2 think-preseeded lens (G4 protocol amendment 2, §10)`
(no attribution footer; this report file and the brief file were left
untracked / excluded from the commit — only the 14 source/test files
listed above were staged and committed).

## Concerns

None outstanding. All 5 brief items implemented, TDD followed (tests
extended/added and independently confirmed relevant via mutation
evidence), both required mutation checks performed with evidence and
restored, full constraint suite (fmt/clippy×2/test/build --features llama)
green, `pager.rs` held at the pre-existing 800-line cap.
